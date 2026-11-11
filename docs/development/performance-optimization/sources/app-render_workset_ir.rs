//! CPU-only, lossless ordinary-note workset storage.
//!
//! The caller owns eligibility and captures an expected shape from the loaded
//! chart. No blob can authorize a note or a group absent from that shape. These
//! bytes contain source IDs and static values only; GPU objects, judgement and
//! clocks remain in their original owners. Cache lifecycle belongs to the
//! existing render cache manager, not this format.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::mem::size_of;

const MAGIC: &[u8; 8] = b"PHRWS001";
/// Bump for any change to eligibility, grouping, arithmetic or role meanings.
pub const ABI: u32 = 1;
const KEY_NAMESPACE: &[u8] = b"phira/ordinary-note-tail-workset/PHRWS001\0";
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_LINES: usize = 100_000;
pub const MAX_GROUPS: usize = 100_000;
pub const MAX_NOTES: usize = 1_000_000;
const HEADER: usize = 124;
const GROUP_RECORD: usize = 17;
const NOTE_RECORD: usize = 17;
const CHECKSUM_OFFSET: usize = 92;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub line_id: u32,
    pub above: bool,
    pub speed_bits: u64,
    /// Contiguous source IDs in original order. Grouping never sorts notes.
    pub note_ids: Vec<u32>,
    pub heights_bits: Vec<u64>,
    pub x_bits: Vec<u32>,
    /// Logical plain-note texture roles, independent of GPU texture IDs.
    pub style_roles: Vec<u8>,
    /// Derived from finite x values, including the current note in each tail.
    /// These certificates are deliberately omitted from the on-disk format.
    pub suffix_x_lo_bits: Vec<u32>,
    pub suffix_x_hi_bits: Vec<u32>,
}

impl Group {
    pub fn new(
        line_id: u32,
        above: bool,
        speed_bits: u64,
        note_ids: Vec<u32>,
        heights_bits: Vec<u64>,
        x_bits: Vec<u32>,
        style_roles: Vec<u8>,
    ) -> Self {
        Self {
            line_id,
            above,
            speed_bits,
            note_ids,
            heights_bits,
            x_bits,
            style_roles,
            suffix_x_lo_bits: Vec::new(),
            suffix_x_hi_bits: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workset {
    pub source: [u8; 32],
    /// Hash of the actual loaded static descriptor, including parser effects.
    pub descriptor: [u8; 32],
    pub groups: Vec<Group>,
}

fn key_for_abi(abi: u32, source: [u8; 32], descriptor: [u8; 32]) -> String {
    let mut hash = Sha256::new();
    hash.update(KEY_NAMESPACE);
    hash.update(abi.to_le_bytes());
    hash.update(source);
    hash.update(descriptor);
    format!("{:x}", hash.finalize())
}

/// The shared cache manager accepts exactly 64 lowercase SHA-256 hex digits.
pub fn cache_key(source: [u8; 32], descriptor: [u8; 32]) -> String {
    key_for_abi(ABI, source, descriptor)
}

struct Layout {
    notes: usize,
    body_bytes: usize,
}

fn charge(budget: &mut usize, count: usize, width: usize) -> Result<()> {
    *budget = budget
        .checked_add(count.checked_mul(width).context("Workset size multiplication overflow")?)
        .context("Workset size addition overflow")?;
    ensure!(*budget <= MAX_BYTES, "Workset allocation/serial budget exceeded");
    Ok(())
}

fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).context("Workset allocation unavailable")?;
    Ok(values)
}

/// Validate counts before scratch allocation. This is a logical payload budget,
/// not a process RSS or allocator-capacity measurement.
fn validate(plan: &Workset, require_suffix: bool) -> Result<Layout> {
    ensure!(plan.groups.len() <= MAX_GROUPS, "Workset group limit exceeded");
    let mut storage_bytes = size_of::<Workset>();
    charge(&mut storage_bytes, plan.groups.len(), size_of::<Group>())?;
    // Contiguous note IDs allow duplicate detection using one range per group.
    charge(&mut storage_bytes, plan.groups.len(), size_of::<(u32, u32, u32)>())?;
    let mut body_bytes = 0;
    charge(&mut body_bytes, plan.groups.len(), GROUP_RECORD)?;
    let mut notes = 0usize;
    for group in &plan.groups {
        let count = group.note_ids.len();
        ensure!(count > 0, "Workset empty group");
        ensure!((group.line_id as usize) < MAX_LINES, "Workset line ID out of range");
        ensure!(
            group.heights_bits.len() == count && group.x_bits.len() == count && group.style_roles.len() == count,
            "Workset parallel arrays differ in length"
        );
        notes = notes.checked_add(count).context("Workset note count overflow")?;
        ensure!(notes <= MAX_NOTES, "Workset note limit exceeded");
        charge(&mut body_bytes, count, NOTE_RECORD)?;
        // IDs, heights, x, role and two recomputed suffix arrays.
        charge(&mut storage_bytes, count, size_of::<u32>() * 4 + size_of::<u64>() + size_of::<u8>())?;
        let speed = f64::from_bits(group.speed_bits);
        ensure!(speed.is_finite() && speed > 0., "Workset non-finite/non-positive speed");
        let mut previous_id = None;
        let mut previous_height = None;
        for index in 0..count {
            let id = group.note_ids[index];
            ensure!((id as usize) < MAX_NOTES, "Workset note ID out of range");
            if let Some(previous) = previous_id {
                ensure!(Some(id) == u32::checked_add(previous, 1), "Workset source note IDs are not contiguous and ordered");
            }
            previous_id = Some(id);
            let height = f64::from_bits(group.heights_bits[index]);
            ensure!(height.is_finite(), "Workset non-finite height");
            if let Some(previous) = previous_height {
                ensure!(height >= previous, "Workset heights are not ascending");
            }
            previous_height = Some(height);
            ensure!(f32::from_bits(group.x_bits[index]).is_finite(), "Workset non-finite x");
            ensure!(group.style_roles[index] <= 5, "Workset unsupported plain-note style role");
        }
        if require_suffix {
            ensure!(group.suffix_x_lo_bits.len() == count && group.suffix_x_hi_bits.len() == count, "Workset suffix array shape changed");
            let mut lo = group.x_bits[count - 1];
            let mut hi = lo;
            for index in (0..count).rev() {
                extend_bounds(&mut lo, &mut hi, group.x_bits[index]);
                ensure!(group.suffix_x_lo_bits[index] == lo && group.suffix_x_hi_bits[index] == hi, "Workset suffix certificate changed");
            }
        }
    }
    let serial_bytes = HEADER.checked_add(body_bytes).context("Workset serial size overflow")?;
    ensure!(serial_bytes <= MAX_BYTES, "Workset serial budget exceeded");
    let mut ranges = reserved(plan.groups.len())?;
    for group in &plan.groups {
        ranges.push((group.line_id, group.note_ids[0], group.note_ids[group.note_ids.len() - 1]));
    }
    ranges.sort_unstable();
    for adjacent in ranges.windows(2) {
        ensure!(adjacent[0].0 != adjacent[1].0 || adjacent[0].2 < adjacent[1].1, "Workset duplicate/overlapping source note IDs");
    }
    Ok(Layout { notes, body_bytes })
}

fn extend_bounds(lo: &mut u32, hi: &mut u32, bits: u32) {
    let value = f32::from_bits(bits);
    // total_cmp deterministically retains -0 as the low endpoint and +0 as
    // the high endpoint; ordinary comparisons still see the same interval.
    if value.total_cmp(&f32::from_bits(*lo)).is_lt() {
        *lo = bits;
    }
    if value.total_cmp(&f32::from_bits(*hi)).is_gt() {
        *hi = bits;
    }
}

fn recompute_suffix(group: &mut Group) -> Result<()> {
    let count = group.note_ids.len();
    let mut lows = reserved(count)?;
    let mut highs = reserved(count)?;
    lows.resize(count, 0);
    highs.resize(count, 0);
    let mut lo = group.x_bits[count - 1];
    let mut hi = lo;
    for index in (0..count).rev() {
        extend_bounds(&mut lo, &mut hi, group.x_bits[index]);
        lows[index] = lo;
        highs[index] = hi;
    }
    group.suffix_x_lo_bits = lows;
    group.suffix_x_hi_bits = highs;
    Ok(())
}

/// Consume groups already captured from eligible plain notes. Unsupported
/// groups must be omitted by the caller and retain original rendering.
pub fn compile(source: [u8; 32], descriptor: [u8; 32], groups: Vec<Group>) -> Result<Workset> {
    let mut plan = Workset { source, descriptor, groups };
    validate(&plan, false)?;
    for group in &mut plan.groups {
        recompute_suffix(group)?;
    }
    Ok(plan)
}

pub fn encode(plan: &Workset) -> Result<Vec<u8>> {
    let layout = validate(plan, true)?;
    let length = HEADER.checked_add(layout.body_bytes).context("Workset serial size overflow")?;
    let mut bytes = reserved(length)?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&ABI.to_le_bytes());
    bytes.extend_from_slice(&plan.source);
    bytes.extend_from_slice(&plan.descriptor);
    bytes.extend_from_slice(&u32::try_from(plan.groups.len())?.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(layout.notes)?.to_le_bytes());
    bytes.extend_from_slice(&u64::try_from(layout.body_bytes)?.to_le_bytes());
    bytes.extend_from_slice(&[0; 32]);
    for group in &plan.groups {
        bytes.extend_from_slice(&group.line_id.to_le_bytes());
        bytes.push(u8::from(group.above));
        bytes.extend_from_slice(&group.speed_bits.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(group.note_ids.len())?.to_le_bytes());
        for index in 0..group.note_ids.len() {
            bytes.extend_from_slice(&group.note_ids[index].to_le_bytes());
            bytes.extend_from_slice(&group.heights_bits[index].to_le_bytes());
            bytes.extend_from_slice(&group.x_bits[index].to_le_bytes());
            bytes.push(group.style_roles[index]);
        }
    }
    ensure!(bytes.len() == length, "Workset encoder size mismatch");
    let checksum = Sha256::digest(&bytes[HEADER..]);
    bytes[CHECKSUM_OFFSET..HEADER].copy_from_slice(&checksum);
    Ok(bytes)
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.cursor.checked_add(N).context("Workset read cursor overflow")?;
        ensure!(end <= self.bytes.len(), "Workset truncated record");
        let value = self.bytes[self.cursor..end].try_into().unwrap();
        self.cursor = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }
}

/// `expected` is freshly captured from the actual loaded chart, not a blob.
/// Exact static comparison also provides actual chart bounds for every ID.
pub fn decode(bytes: &[u8], expected: &Workset) -> Result<Workset> {
    ensure!((HEADER..=MAX_BYTES).contains(&bytes.len()), "Workset serial size limit");
    let layout = validate(expected, true)?;
    let mut header = Reader {
        bytes: &bytes[..HEADER],
        cursor: 0,
    };
    ensure!(&header.take::<8>()? == MAGIC, "Workset magic mismatch");
    ensure!(header.u32()? == ABI, "Workset compiler ABI mismatch");
    ensure!(header.take::<32>()? == expected.source, "Workset source changed");
    ensure!(header.take::<32>()? == expected.descriptor, "Workset loaded descriptor changed");
    ensure!(header.u32()? as usize == expected.groups.len(), "Workset source group count changed");
    ensure!(header.u32()? as usize == layout.notes, "Workset source note count changed");
    let body_bytes = header.u64()?;
    ensure!(
        body_bytes == u64::try_from(layout.body_bytes)? && body_bytes == u64::try_from(bytes.len() - HEADER)?,
        "Workset counts/serial length mismatch"
    );
    ensure!(header.take::<32>()? == Sha256::digest(&bytes[HEADER..])[..], "Workset checksum mismatch");
    let mut reader = Reader {
        bytes: &bytes[HEADER..],
        cursor: 0,
    };
    let mut groups = reserved(expected.groups.len())?;
    for actual in &expected.groups {
        let line_id = reader.u32()?;
        let above = reader.byte()?;
        let speed_bits = reader.u64()?;
        let count = reader.u32()? as usize;
        ensure!(above <= 1, "Workset invalid above tag");
        ensure!(
            line_id == actual.line_id && (above != 0) == actual.above && speed_bits == actual.speed_bits && count == actual.note_ids.len(),
            "Workset source group identity/order changed"
        );
        let mut group = Group::new(line_id, above != 0, speed_bits, reserved(count)?, reserved(count)?, reserved(count)?, reserved(count)?);
        for index in 0..count {
            let note_id = reader.u32()?;
            let height_bits = reader.u64()?;
            let x_bits = reader.u32()?;
            let role = reader.byte()?;
            ensure!(
                note_id == actual.note_ids[index]
                    && height_bits == actual.heights_bits[index]
                    && x_bits == actual.x_bits[index]
                    && role == actual.style_roles[index],
                "Workset source note identity/order/static certificate changed"
            );
            group.note_ids.push(note_id);
            group.heights_bits.push(height_bits);
            group.x_bits.push(x_bits);
            group.style_roles.push(role);
        }
        recompute_suffix(&mut group)?;
        groups.push(group);
    }
    ensure!(reader.cursor == reader.bytes.len(), "Workset trailing bytes");
    Ok(Workset {
        source: expected.source,
        descriptor: expected.descriptor,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(line: u32, first: u32, heights: &[f64], xs: &[f32], roles: &[u8]) -> Group {
        Group::new(
            line,
            true,
            1.0000000001f64.to_bits(),
            (first..first + heights.len() as u32).collect(),
            heights.iter().map(|value| value.to_bits()).collect(),
            xs.iter().map(|value| value.to_bits()).collect(),
            roles.to_vec(),
        )
    }
    fn fixture() -> Workset {
        compile(
            [17; 32],
            [29; 32],
            vec![
                group(0, 3, &[0., 1.234567890123, 20.25], &[0.5, -0., 0.25], &[0, 2, 5]),
                group(0, 9, &[50., 50.], &[-0.75, 0.75], &[1, 4]),
                group(2, 0, &[1., 2.], &[-0., 0.], &[3, 0]),
            ],
        )
        .unwrap()
    }
    /// Make corruption internally checksummed so a test reaches shape checking.
    fn reseal(bytes: &mut [u8]) {
        let checksum = Sha256::digest(&bytes[HEADER..]);
        bytes[CHECKSUM_OFFSET..HEADER].copy_from_slice(&checksum);
    }
    fn invalid(groups: Vec<Group>, message: &str) {
        assert!(compile([1; 32], [2; 32], groups).unwrap_err().to_string().contains(message));
    }

    #[test]
    fn roundtrip_keeps_static_bits_and_recomputes_tail_bounds() {
        let expected = fixture();
        let bytes = encode(&expected).unwrap();
        let decoded = decode(&bytes, &expected).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(decoded.groups[0].speed_bits, 1.0000000001f64.to_bits());
        assert_eq!(decoded.groups[0].heights_bits[1], 1.234567890123f64.to_bits());
        assert_eq!(decoded.groups[0].suffix_x_lo_bits, vec![(-0.0f32).to_bits(), (-0.0f32).to_bits(), 0.25f32.to_bits()]);
        assert_eq!(decoded.groups[0].suffix_x_hi_bits, vec![0.5f32.to_bits(), 0.25f32.to_bits(), 0.25f32.to_bits()]);
        assert_eq!(decoded.groups[2].suffix_x_lo_bits, vec![(-0.0f32).to_bits(), 0.0f32.to_bits()]);
        assert_eq!(decoded.groups[2].suffix_x_hi_bits, vec![0.0f32.to_bits(), 0.0f32.to_bits()]);
        // Eight bytes per note of recomputed bounds would change this length.
        assert_eq!(bytes.len(), HEADER + 3 * GROUP_RECORD + 7 * NOTE_RECORD);
    }

    #[test]
    fn key_separates_namespace_abi_source_and_loaded_descriptor() {
        let source = [1; 32];
        let descriptor = [2; 32];
        let key = cache_key(source, descriptor);
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase()));
        assert_eq!(key, cache_key(source, descriptor));
        assert_ne!(key, key_for_abi(ABI + 1, source, descriptor));
        assert_ne!(key, cache_key([3; 32], descriptor));
        assert_ne!(key, cache_key(source, [3; 32]));
        let mut unnamespaced = Sha256::new();
        unnamespaced.update(ABI.to_le_bytes());
        unnamespaced.update(source);
        unnamespaced.update(descriptor);
        assert_ne!(key, format!("{:x}", unnamespaced.finalize()));
    }

    #[test]
    fn rejects_corrupt_truncated_extra_bytes_and_hostile_header_counts() {
        let expected = fixture();
        let bytes = encode(&expected).unwrap();
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], &expected).is_err(), "accepted truncated prefix {end}");
        }
        for offset in [0, 8, 12, 44, 76, 80, 84, 92, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[offset] ^= 0x80;
            assert!(decode(&corrupt, &expected).is_err(), "accepted changed byte {offset}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        let extra_length = (extra.len() - HEADER) as u64;
        extra[84..92].copy_from_slice(&extra_length.to_le_bytes());
        reseal(&mut extra);
        assert!(decode(&extra, &expected).is_err());
        let mut hostile = bytes;
        hostile[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&hostile, &expected).is_err());
    }

    #[test]
    fn checksummed_forgery_cannot_change_ids_order_bits_or_roles() {
        let expected = fixture();
        let bytes = encode(&expected).unwrap();
        let first_note = HEADER + GROUP_RECORD;
        // line, above, speed, count, note ID, height, x and logical role.
        for offset in [
            HEADER,
            HEADER + 4,
            HEADER + 5,
            HEADER + 13,
            first_note,
            first_note + 4,
            first_note + 12,
            first_note + 16,
        ] {
            let mut forged = bytes.clone();
            forged[offset] ^= 1;
            reseal(&mut forged);
            assert!(decode(&forged, &expected).is_err(), "accepted forged byte {offset}");
        }
        let mut duplicate = bytes.clone();
        let first_id = duplicate[first_note..first_note + 4].to_vec();
        duplicate[first_note + NOTE_RECORD..first_note + NOTE_RECORD + 4].copy_from_slice(&first_id);
        reseal(&mut duplicate);
        assert!(decode(&duplicate, &expected).is_err());
        let mut reordered = bytes.clone();
        for offset in 0..NOTE_RECORD {
            reordered.swap(first_note + offset, first_note + NOTE_RECORD + offset);
        }
        reseal(&mut reordered);
        assert!(decode(&reordered, &expected).is_err());
        // The same source bytes can load into a different static shape.
        let mut changed = expected.clone();
        changed.groups.swap(0, 1);
        assert!(decode(&bytes, &changed).is_err());
    }

    #[test]
    fn rejects_nonfinite_unordered_unknown_and_mismatched_input() {
        let original = group(0, 0, &[1., 2.], &[0., 1.], &[0, 5]);
        for speed in [f64::NAN, f64::INFINITY, 0., -1.] {
            let mut value = original.clone();
            value.speed_bits = speed.to_bits();
            invalid(vec![value], "speed");
        }
        for height in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut value = original.clone();
            value.heights_bits[0] = height.to_bits();
            invalid(vec![value], "height");
        }
        let mut value = original.clone();
        value.heights_bits.swap(0, 1);
        invalid(vec![value], "ascending");
        let mut value = original.clone();
        value.x_bits[0] = f32::INFINITY.to_bits();
        invalid(vec![value], "non-finite x");
        let mut value = original.clone();
        value.style_roles[1] = 6;
        invalid(vec![value], "style role");
        let mut value = original.clone();
        value.heights_bits.pop();
        invalid(vec![value], "parallel arrays");
        let mut value = original;
        value.note_ids[1] = 2;
        invalid(vec![value], "contiguous");
    }

    #[test]
    fn rejects_global_duplicates_without_reordering_valid_source_groups() {
        let first = group(7, 0, &[0., 1.], &[0., 0.], &[0, 0]);
        let duplicate = group(7, 1, &[1., 2.], &[0., 0.], &[0, 0]);
        invalid(vec![first.clone(), duplicate], "duplicate/overlapping");
        let later = group(7, 10, &[1.], &[0.], &[0]);
        let another_line = group(2, 0, &[0.], &[0.], &[0]);
        let plan = compile([1; 32], [2; 32], vec![later, another_line, first]).unwrap();
        assert_eq!(plan.groups.iter().map(|value| (value.line_id, value.note_ids[0])).collect::<Vec<_>>(), vec![(7, 10), (2, 0), (7, 0)]);
        assert_eq!(decode(&encode(&plan).unwrap(), &plan).unwrap(), plan);
    }

    #[test]
    fn enforces_limits_and_does_not_accept_forged_expected_certificates() {
        let mut value = group(MAX_LINES as u32, 0, &[0.], &[0.], &[0]);
        invalid(vec![value.clone()], "line ID");
        value.line_id = 0;
        value.note_ids[0] = MAX_NOTES as u32;
        invalid(vec![value], "note ID");
        invalid(vec![Group::new(0, true, 1.0f64.to_bits(), vec![], vec![], vec![], vec![])], "empty group");
        let expected = fixture();
        let bytes = encode(&expected).unwrap();
        let oversized = vec![0; MAX_BYTES + 1];
        assert!(decode(&oversized, &expected).is_err());
        let mut budget = MAX_BYTES;
        assert!(charge(&mut budget, 1, 1).is_err());
        let mut budget = 0;
        assert!(charge(&mut budget, usize::MAX, 2).is_err());
        let mut changed = expected;
        changed.groups[0].suffix_x_lo_bits[0] = 1.0f32.to_bits();
        assert!(encode(&changed).is_err());
        assert!(decode(&bytes, &changed).is_err());
    }

    #[test]
    fn empty_eligible_set_roundtrips_and_has_no_fake_group() {
        let expected = compile([0; 32], [0; 32], vec![]).unwrap();
        let bytes = encode(&expected).unwrap();
        assert_eq!(bytes.len(), HEADER);
        assert_eq!(decode(&bytes, &expected).unwrap(), expected);
    }
}
