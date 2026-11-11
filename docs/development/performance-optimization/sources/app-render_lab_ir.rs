//! First semantic Render IR: constant note data, stable IDs and logical batches.
//! Input to experimental CPU/GPU consumers, not production settings yet.
use crate::core::{AnimFloat, Chart, JudgeLineKind, NoteKind};
use anyhow::{ensure, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const MAGIC: &[u8; 8] = b"PHRIR001";
const HEADER: usize = 116;
const RECORD: usize = 56;
pub fn abi() -> [u8; 32] {
    Sha256::digest(b"phira-render-ir-prototype-v1/cb70d09/lossless-static-notes/f64-time-height-speed").into()
}
pub fn cache_key(source: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(abi());
    digest.update(source);
    format!("{:x}", digest.finalize())
}
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub line: u32,
    pub note: u32,
    pub flags: u32,
    pub x: f32,
    pub time: f64,
    pub height: f64,
    pub speed: f64,
    pub color: [f32; 4],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub source: [u8; 32],
    pub line_notes: Vec<u32>,
    pub line_order: Vec<u32>,
    pub packets: Vec<Packet>,
}

fn constant(anim: &AnimFloat) -> bool {
    anim.keyframes.len() <= 1 && anim.next.is_none()
}
pub fn compile(chart: &Chart, source: [u8; 32]) -> (Plan, BTreeMap<String, usize>) {
    let mut packets = Vec::new();
    let mut fallback = BTreeMap::new();
    for (line_id, line) in chart.lines.iter().enumerate() {
        let ctrl = line.ctrl_obj.borrow();
        let basic_line = matches!(line.kind, JudgeLineKind::Normal)
            && line.parent.is_none()
            && line.attach_ui.is_none()
            && line.incline.is_default()
            && ctrl.alpha.is_default()
            && ctrl.size.is_default()
            && ctrl.pos.is_default()
            && ctrl.y.is_default()
            && line.object.alpha.next.is_none()
            && line.object.alpha.keyframes.iter().all(|k| k.value >= 0.);
        for (note_id, note) in line.notes.iter().enumerate() {
            let reason = if matches!(note.kind, NoteKind::Hold { .. }) {
                Some("hold")
            } else if !basic_line {
                Some("complex-line")
            } else if note.fake
                || !constant(&note.object.translation.0)
                || !note.object.translation.1.is_default()
                || !note.object.rotation.is_default()
                || !note.object.scale.0.is_default()
                || !note.object.scale.1.is_default()
                || !note.object.alpha.is_default()
            {
                Some("complex-note")
            } else if !note.time.is_finite() || !note.height.is_finite() || !note.speed.is_finite() || note.speed <= 0. {
                Some("invalid-fast-path-domain")
            } else {
                None
            };
            if let Some(reason) = reason {
                *fallback.entry(reason.to_owned()).or_default() += 1;
                continue;
            }
            let x = note.object.translation.0.now_opt().unwrap_or(0.);
            if !x.is_finite() || ![note.color.r, note.color.g, note.color.b, note.color.a].iter().all(|v| v.is_finite()) {
                *fallback.entry("non-finite-color-position".to_owned()).or_default() += 1;
                continue;
            }
            let flags = note.kind.order() as u32 | ((note.above as u32) << 8) | ((note.multiple_hint as u32) << 9);
            packets.push(Packet {
                line: line_id as _,
                note: note_id as _,
                flags,
                x,
                time: note.time,
                height: note.height,
                speed: note.speed,
                color: [note.color.r, note.color.g, note.color.b, note.color.a],
            });
        }
    }
    (
        Plan {
            source,
            line_notes: chart.lines.iter().map(|l| l.notes.len() as u32).collect(),
            line_order: chart.order.iter().map(|v| *v as u32).collect(),
            packets,
        },
        fallback,
    )
}

pub fn encode(plan: &Plan) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(plan.line_order.len() as u32).to_le_bytes());
    for value in &plan.line_notes {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in &plan.line_order {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for p in &plan.packets {
        for value in [p.line, p.note, p.flags] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&p.x.to_le_bytes());
        for value in [p.time, p.height, p.speed] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        for value in p.color {
            body.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&abi());
    bytes.extend_from_slice(&plan.source);
    for value in [plan.line_notes.len() as u32, plan.packets.len() as u32, plan.line_notes.iter().sum()] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&Sha256::digest(&body));
    bytes.extend_from_slice(&body);
    bytes
}

pub fn decode(bytes: &[u8], source: [u8; 32]) -> Result<Plan> {
    ensure!(bytes.len() >= HEADER + 4, "truncated IR header");
    ensure!(&bytes[..8] == MAGIC, "IR version mismatch");
    ensure!(bytes[8..40] == abi(), "compiler ABI mismatch");
    ensure!(bytes[40..72] == source, "source changed");
    let read32 = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let lines = read32(72) as usize;
    let count = read32(76) as usize;
    let total = read32(80);
    let order = read32(HEADER) as usize;
    let shape = HEADER
        .checked_add(4)
        .and_then(|n| lines.checked_add(order).and_then(|v| v.checked_mul(4)).and_then(|v| n.checked_add(v)))
        .and_then(|n| count.checked_mul(RECORD).and_then(|v| n.checked_add(v)))
        .ok_or_else(|| anyhow::anyhow!("IR size overflow"))?;
    ensure!(shape == bytes.len(), "IR counts/size mismatch");
    ensure!(bytes[84..116] == Sha256::digest(&bytes[HEADER..])[..], "IR checksum mismatch");
    let mut cursor = HEADER + 4;
    let mut line_notes = Vec::with_capacity(lines);
    for _ in 0..lines {
        line_notes.push(read32(cursor));
        cursor += 4;
    }
    ensure!(line_notes.iter().try_fold(0u32, |sum, v| sum.checked_add(*v)) == Some(total), "IR note count overflow/mismatch");
    let mut line_order = Vec::with_capacity(order);
    let mut ordered = HashSet::new();
    for _ in 0..order {
        let value = read32(cursor);
        cursor += 4;
        ensure!((value as usize) < lines && ordered.insert(value), "invalid/duplicate line order");
        line_order.push(value);
    }
    let mut packets = Vec::with_capacity(count);
    let mut ids = HashSet::new();
    for _ in 0..count {
        let p = &bytes[cursor..cursor + RECORD];
        cursor += RECORD;
        let u = |o| u32::from_le_bytes(p[o..o + 4].try_into().unwrap());
        let f = |o| f32::from_le_bytes(p[o..o + 4].try_into().unwrap());
        let d = |o| f64::from_le_bytes(p[o..o + 8].try_into().unwrap());
        let packet = Packet {
            line: u(0),
            note: u(4),
            flags: u(8),
            x: f(12),
            time: d(16),
            height: d(24),
            speed: d(32),
            color: [f(40), f(44), f(48), f(52)],
        };
        ensure!(
            (packet.line as usize) < lines && packet.note < line_notes[packet.line as usize] && ids.insert((packet.line, packet.note)),
            "IR invalid/duplicate source note"
        );
        ensure!(packet.flags & !0x303 == 0 && packet.flags & 3 != 0, "IR unknown flags/hold in simple stream");
        ensure!(
            packet.x.is_finite()
                && packet.time.is_finite()
                && packet.height.is_finite()
                && packet.speed.is_finite()
                && packet.speed > 0.
                && packet.color.iter().all(|v| v.is_finite()),
            "IR non-finite/invalid parameters"
        );
        packets.push(packet);
    }
    Ok(Plan {
        source,
        line_notes,
        line_order,
        packets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Plan {
        Plan {
            source: [7; 32],
            line_notes: vec![1],
            line_order: vec![0],
            packets: vec![Packet {
                line: 0,
                note: 0,
                flags: 2,
                x: 0.1234567,
                time: 1.234567890123,
                height: 123456.789012345,
                speed: 1.0000000001,
                color: [1., 0.5, 0.25, 1.],
            }],
        }
    }
    #[test]
    fn preserves_fractional_time_and_height_bits() {
        let plan = fixture();
        let decoded = decode(&encode(&plan), plan.source).unwrap();
        assert_eq!(decoded, plan);
        assert_eq!(decoded.packets[0].time.to_bits(), plan.packets[0].time.to_bits());
        assert_eq!(decoded.packets[0].height.to_bits(), plan.packets[0].height.to_bits());
    }
    #[test]
    fn rejects_stale_corrupt_truncated_and_hostile_counts() {
        let plan = fixture();
        let mut bytes = encode(&plan);
        assert!(decode(&bytes, [8; 32]).is_err());
        let last = bytes.len() - 1;
        assert!(decode(&bytes[..last], plan.source).is_err());
        bytes[last] ^= 1;
        assert!(decode(&bytes, plan.source).is_err());
        let mut bytes = encode(&plan);
        bytes[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes, plan.source).is_err());
    }
}
