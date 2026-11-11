//! Owner-scoped ordinary-note tail worksets. Loading owns all hashes and disk
//! work; rendering only searches immutable source IDs and a nested clip proof.
//! A public Chart renderer must never obtain this plan without its owner scope.
use super::{lines::CacheLease, visibility_bounds::Side, workset_ir as ir};
use crate::{
    core::{AnimFloat, Chart, JudgeLine, Note, NoteKind, RenderConfig, Resource, FADEOUT_TIME},
    judge::JudgeStatus,
    parse::RPE_HEIGHT,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(feature = "render_research")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::Cell, mem::size_of, time::Instant};

// Loading and production consumption follow chart_pre_compile independently of
// quality and line-GPU capability. Research builds retain an independent,
// default-off switch for same-backend comparisons.
#[cfg(feature = "render_research")]
static ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "render_research")]
static AUDIT: AtomicBool = AtomicBool::new(false);
pub(crate) fn configure(enabled: bool, audit: bool) {
    #[cfg(feature = "render_research")]
    {
        ENABLED.store(enabled, Ordering::Relaxed);
        AUDIT.store(audit, Ordering::Relaxed);
    }
    #[cfg(not(feature = "render_research"))]
    let _ = (enabled, audit);
}
pub(crate) fn execution_enabled() -> bool {
    #[cfg(feature = "render_research")]
    {
        ENABLED.load(Ordering::Relaxed) && !cfg!(feature = "render_chart_ir")
    }
    #[cfg(not(feature = "render_research"))]
    {
        true
    }
}
fn audit_enabled() -> bool {
    #[cfg(feature = "render_research")]
    {
        AUDIT.load(Ordering::Relaxed)
    }
    #[cfg(not(feature = "render_research"))]
    {
        false
    }
}

const MIN_GROUP_NOTES: usize = 4;
const EXTREME_LINE_NOTES: usize = 20_000;
const DESCRIPTOR_ABI: &[u8] = b"ordinary-tail-consumer-v1/complete-cache-groups/min4/typed-empty-ctrl/empty-alpha-rotation-y-scale/static-x/click-drag-flick/nonfake/ordered-f64-base/terminal-init-replay";

#[derive(Clone, Copy)]
struct GroupIndex {
    line: u32,
    start: u32,
    end: u32,
    styles: u8,
}
#[derive(Default)]
struct Counters {
    attempts: Cell<u64>,
    proved: Cell<u64>,
    accepted: Cell<u64>,
    skipped: Cell<u64>,
    witnesses: Cell<u64>,
    false_culls: Cell<u64>,
}
pub(crate) struct Prepared {
    plan: ir::Workset,
    lease: CacheLease,
    index: Vec<GroupIndex>,
    counters: Counters,
    preparation: Value,
}
/// Render [current_cursor, end), then replay the unchanged init helper for
/// terminal_note_id. The proved suffix [end, group_end) produces no vertices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cut {
    pub end: usize,
    pub terminal_note_id: usize,
}
fn increment(value: &Cell<u64>, amount: u64) {
    value.set(value.get().saturating_add(amount));
}
impl Prepared {
    /// The chart owner checks the shared cache epoch once before opening its
    /// render scope. A clear/disable invalidates CPU worksets and GPU plans alike.
    pub(crate) fn is_current(&self) -> bool {
        self.lease.is_current()
    }
    pub(crate) fn audit_enabled(&self) -> bool {
        audit_enabled()
    }
    pub(crate) fn record_cut(&self, skipped: usize) {
        if audit_enabled() {
            increment(&self.counters.accepted, 1);
            increment(&self.counters.skipped, skipped as u64);
        }
    }
    pub(crate) fn record_witness(&self, valid: bool) {
        if audit_enabled() {
            increment(&self.counters.witnesses, 1);
            if !valid {
                increment(&self.counters.false_culls, 1);
            }
        }
    }
    /// Only reporting builds a JSON value; the render path has primitive cells.
    pub(crate) fn stats(&self) -> Value {
        let mut report = self.preparation.clone();
        report["attempted_groups"] = json!(self.counters.attempts.get());
        report["proved_groups"] = json!(self.counters.proved.get());
        report["accepted_groups"] = json!(self.counters.accepted.get());
        report["skipped_note_visits"] = json!(self.counters.skipped.get());
        report["witnesses"] = json!(self.counters.witnesses.get());
        report["false_culls"] = json!(self.counters.false_culls.get());
        report["execution_enabled"] = json!(execution_enabled());
        report
    }
    fn lookup(&self, line: usize, cursor: usize, above: bool) -> Option<(&ir::Group, GroupIndex)> {
        let key = (u32::try_from(line).ok()?, u32::try_from(cursor).ok()?);
        let position = self.index.partition_point(|group| (group.line, group.start) <= key).checked_sub(1)?;
        let index = self.index[position];
        if index.line != key.0 || key.1 < index.start || key.1 >= index.end || self.plan.groups[position].above != above {
            return None;
        }
        Some((&self.plan.groups[position], index))
    }
    /// Caller supplies a same-chart owner scope and the current side matrix.
    /// Mutable judgement/cursors remain in the source; no source hash, note
    /// scan, allocation, disk access or control mutation occurs in this method.
    pub(crate) fn cutoff(
        &self,
        line_id: usize,
        cursor: usize,
        above: bool,
        line: &JudgeLine,
        res: &Resource,
        config: &RenderConfig<'_>,
        side: Side,
    ) -> Option<Cut> {
        if !execution_enabled() {
            return None;
        }
        // A line's custom tween may have changed diagnostic options after
        // RenderConfig was sampled. Preserve those actual per-note callbacks.
        let reads = super::reads::execution();
        let visibility = super::visibility::execution();
        if reads.enabled || reads.audit || visibility.enabled || visibility.audit {
            return None;
        }
        if audit_enabled() {
            increment(&self.counters.attempts, 1);
        }
        if !res.config.chart_pre_compile
            || res.config.aggressive
            || line.notes.len() > EXTREME_LINE_NOTES
            || res.config.chart_debug
            || res.config.chart_debug_line
            || res.config.chart_debug_note
            || config.read_execution.audit
            || config.read_execution.enabled
            || config.visibility_execution.audit
            || config.visibility_execution.enabled
            || config.appear_before != f64::INFINITY
            || config.incline_sin != 0.
            || !super::visibility::ctrl_default(config.ctrl_obj)
            || !res.time.is_finite()
            || !res.aspect_ratio.is_normal()
            || res.aspect_ratio <= 0.
            || !res.config.flow_speed.is_normal()
            || res.config.flow_speed <= 0.
            || !normal_or_zero(config.line_height)
        {
            return None;
        }
        let (group, index) = self.lookup(line_id, cursor, above)?;
        // An above cursor can advance onto the first below note. The original
        // above loop then immediately breaks, so it has no terminal init to
        // replay. A containing group alone does not authorize another side.
        let terminal_note_id = index.end as usize - 1;
        let terminal = line.notes.get(terminal_note_id)?;
        // The final source note must actually reach Ctrl initialization in the
        // original loop. Equality at enormous times can pass the rounded
        // FADEOUT test, so repeat that exact test rather than assuming future.
        if matches!(terminal.judge, JudgeStatus::Judged)
            || !terminal.time.is_finite()
            || terminal.time < res.time
            || (!config.draw_below && res.time - FADEOUT_TIME >= terminal.time)
            || terminal_init_time(terminal.height, config.line_height, terminal.speed).is_none()
        {
            return None;
        }
        let speed = f64::from_bits(group.speed_bits);
        // Keep the original multiplication order, including the default y
        // control's multiplication by one. Subnormal speed/product falls back.
        let spd = speed * res.config.flow_speed as f64 * 1.0f64;
        if !spd.is_normal() || spd <= 0. {
            return None;
        }
        let aspect = res.aspect_ratio as f64;
        let line_height = config.line_height / aspect * spd;
        if !normal_or_zero(line_height) {
            return None;
        }
        let mut hfs = [[0.; 2]; 6];
        let mut styles = 0;
        for role in 0..6 {
            if index.styles & (1 << role) == 0 {
                continue;
            }
            let hint = res.config.double_hint && role >= 3;
            let style = if hint { &res.res_pack.note_style_mh } else { &res.res_pack.note_style };
            let texture = match role % 3 {
                0 => &style.click,
                1 => &style.drag,
                _ => &style.flick,
            };
            let click_width = res.res_pack.note_style.click.width();
            if !click_width.is_normal() || click_width <= 0. {
                return None;
            }
            let scale = (if hint {
                res.res_pack.note_style_mh.click.width() / click_width
            } else {
                1.0
            }) * res.note_width;
            let width = texture.width();
            let height = texture.height();
            if !width.is_normal() || width <= 0. || !height.is_finite() || height < 0. {
                return None;
            }
            let half = [scale, height * scale / width];
            if !half.iter().all(|value| value.is_finite() && *value >= 0.) {
                return None;
            }
            hfs[styles] = half;
            styles += 1;
        }
        let offset = cursor.checked_sub(index.start as usize)?;
        let relative = first_proved_tail(group, offset, aspect, spd, line_height, res.aspect_ratio, &hfs[..styles], side)?;
        let end = (index.start as usize).checked_add(relative)?;
        if audit_enabled() {
            increment(&self.counters.proved, 1);
        }
        Some(Cut { end, terminal_note_id })
    }
}

fn normal_or_zero(value: f64) -> bool {
    value == 0. || value.is_normal()
}
fn terminal_init_time(height: f64, line_height: f64, speed: f64) -> Option<f64> {
    // This is exactly Note::init_ctrl_obj_with_y with a certified empty Y
    // track, including its addition of zero/speed and f32 RPE_HEIGHT cast.
    let value = (height - line_height + 0.0f32 as f64 / speed) * RPE_HEIGHT as f64 / 2.;
    (value.is_finite() && value != 0.).then_some(value)
}
fn source_base(height: f64, aspect: f64, spd: f64, line_height: f64) -> Option<f32> {
    // Do not reassociate to (height - source_line_height) / aspect * spd:
    // cancellation and rounding differ from the original Note renderer.
    let scaled_height = height / aspect * spd;
    let value = scaled_height - line_height;
    if !normal_or_zero(scaled_height) || !normal_or_zero(value) {
        return None;
    }
    let value = value as f32;
    value.is_finite().then_some(value)
}
fn first_proved_tail(
    group: &ir::Group,
    start: usize,
    aspect: f64,
    spd: f64,
    line_height: f64,
    aspect_f32: f32,
    hfs: &[[f32; 2]],
    side: Side,
) -> Option<usize> {
    let last = group.note_ids.len().checked_sub(1)?;
    if start > last {
        return None;
    }
    let lower = source_base(f64::from_bits(*group.heights_bits.get(start)?), aspect, spd, line_height)?;
    let upper = source_base(f64::from_bits(*group.heights_bits.get(last)?), aspect, spd, line_height)?;
    if lower > upper || !(lower.abs().max(upper.abs()) * aspect_f32).is_finite() {
        // Even incline_sin=0 may produce NaN through 0*infinity in the source
        // transform, so an overflowing base*aspect is outside this certificate.
        return None;
    }
    let proof = |offset: usize| -> Option<bool> {
        let base = source_base(f64::from_bits(*group.heights_bits.get(offset)?), aspect, spd, line_height)?;
        side.outside_range(
            f32::from_bits(*group.suffix_x_lo_bits.get(offset)?),
            f32::from_bits(*group.suffix_x_hi_bits.get(offset)?),
            base,
            upper,
            hfs,
        )
    };
    if !proof(last)? {
        return None;
    }
    // Both the height interval and X suffix shrink as offset increases. The
    // one-common-axis whole-tail proof is inclusion-monotone; pointwise
    // outside OR is not a valid binary-search predicate.
    let (mut lo, mut hi) = (start, last);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if proof(mid)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}

fn static_x(track: &AnimFloat) -> Option<f32> {
    if track.next.is_some() {
        return None;
    }
    let value = match track.keyframes.len() {
        0 => 0.,
        1 if track.cursor == 0 => track.keyframes[0].value,
        _ => return None,
    };
    value.is_finite().then_some(value)
}
fn role(note: &Note) -> Option<u8> {
    let kind = match note.kind {
        NoteKind::Click => 0,
        NoteKind::Drag => 1,
        NoteKind::Flick => 2,
        _ => return None,
    };
    Some(kind + if note.multiple_hint { 3 } else { 0 })
}
fn note_certificate(note: &Note) -> Option<(f32, u8)> {
    if !note.plain()
        || note.fake
        || !note.height.is_finite()
        || !note.speed.is_normal()
        || note.speed <= 0.
        || !note.time.is_finite()
        || !note.object.alpha.is_default()
        || !note.object.scale.0.is_default()
        || !note.object.scale.1.is_default()
        || !note.object.rotation.is_default()
        || !note.object.translation.1.is_default()
    {
        return None;
    }
    Some((static_x(&note.object.translation.0)?, role(note)?))
}

// The exact immutable boundaries formed by JudgeLineCache::reset: first plain
// note, above runs of equal speed, then below runs of equal speed. Never split
// an unsupported member out of one of those runs to manufacture eligibility.
struct SourceGroups<'a> {
    notes: &'a [Note],
    cursor: usize,
    above: bool,
}
impl<'a> SourceGroups<'a> {
    fn new(line: &'a JudgeLine) -> Self {
        Self {
            notes: &line.notes,
            cursor: line.notes.iter().position(Note::plain).unwrap_or(line.notes.len()),
            above: true,
        }
    }
}
impl Iterator for SourceGroups<'_> {
    type Item = (usize, usize, bool);
    fn next(&mut self) -> Option<Self::Item> {
        let first = self.notes.get(self.cursor)?;
        if self.above && !first.above {
            self.above = false;
        }
        let above = self.above;
        let start = self.cursor;
        let speed = first.speed;
        self.cursor += 1;
        while self
            .notes
            .get(self.cursor)
            .is_some_and(|note| (!above || note.above) && note.speed == speed)
        {
            self.cursor += 1;
        }
        Some((start, self.cursor, above))
    }
}
fn eligible_group(line: &JudgeLine, start: usize, end: usize, above: bool) -> bool {
    if end - start < MIN_GROUP_NOTES || end > ir::MAX_NOTES {
        return false;
    }
    let speed = line.notes[start].speed.to_bits();
    let mut previous = f64::NEG_INFINITY;
    for note in &line.notes[start..end] {
        if note.above != above || note.speed.to_bits() != speed || note_certificate(note).is_none() || note.height < previous {
            return false;
        }
        previous = note.height;
    }
    true
}
fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut result = Vec::new();
    result.try_reserve_exact(count).context("Tail workset allocation unavailable")?;
    Ok(result)
}
fn charge(bytes: &mut usize, count: usize, width: usize) -> Result<()> {
    *bytes = bytes
        .checked_add(count.checked_mul(width).context("Tail workset size overflow")?)
        .context("Tail workset size overflow")?;
    ensure!(*bytes <= ir::MAX_BYTES, "Tail workset logical payload budget exceeded");
    Ok(())
}
fn capture(chart: &Chart, source_bytes: &[u8]) -> Result<ir::Workset> {
    ensure!(chart.lines.len() <= ir::MAX_LINES, "Tail workset line count exceeded");
    let mut descriptor = Sha256::new();
    descriptor.update(DESCRIPTOR_ABI);
    descriptor.update(ir::ABI.to_le_bytes());
    descriptor.update(u64::try_from(chart.lines.len())?.to_le_bytes());
    let (mut groups_count, mut notes_count, mut all_notes) = (0usize, 0usize, 0usize);
    let mut payload = size_of::<ir::Workset>();
    for (line_id, line) in chart.lines.iter().enumerate() {
        all_notes = all_notes.checked_add(line.notes.len()).context("Tail source count overflow")?;
        ensure!(all_notes <= ir::MAX_NOTES, "Tail source note count exceeded");
        descriptor.update(u32::try_from(line_id)?.to_le_bytes());
        descriptor.update(u64::try_from(line.notes.len())?.to_le_bytes());
        // RefCell mutation through &Chart is possible; runtime rechecks these
        // four typed-empty tracks within the line's original control borrow.
        descriptor.update([super::visibility::ctrl_default(&line.ctrl_obj.borrow()) as u8]);
        for (note_id, note) in line.notes.iter().enumerate() {
            descriptor.update(u32::try_from(note_id)?.to_le_bytes());
            descriptor.update([note.plain() as u8, note.above as u8, note.fake as u8, note.multiple_hint as u8]);
            descriptor.update(note.height.to_bits().to_le_bytes());
            descriptor.update(note.speed.to_bits().to_le_bytes());
            descriptor.update(note.time.to_bits().to_le_bytes());
            descriptor.update([match note.kind {
                NoteKind::Click => 0,
                NoteKind::Drag => 1,
                NoteKind::Flick => 2,
                NoteKind::Hold { .. } => 3,
            }]);
            match note_certificate(note) {
                Some((x, role)) => {
                    descriptor.update([1, role]);
                    descriptor.update(x.to_bits().to_le_bytes());
                }
                None => descriptor.update([0]),
            }
        }
        for (start, end, above) in SourceGroups::new(line) {
            let eligible = eligible_group(line, start, end, above);
            descriptor.update(u32::try_from(start)?.to_le_bytes());
            descriptor.update(u32::try_from(end)?.to_le_bytes());
            descriptor.update([above as u8, eligible as u8]);
            if !eligible {
                continue;
            }
            groups_count = groups_count.checked_add(1).context("Tail group count overflow")?;
            notes_count = notes_count.checked_add(end - start).context("Tail note count overflow")?;
            ensure!(groups_count <= ir::MAX_GROUPS && notes_count <= ir::MAX_NOTES, "Tail workset count exceeded");
            // Include the IR validation's range scratch and our lookup table,
            // as well as all recomputed suffix arrays, before any allocation.
            charge(&mut payload, 1, size_of::<ir::Group>() + size_of::<(u32, u32, u32)>() + size_of::<GroupIndex>())?;
            charge(&mut payload, end - start, size_of::<u32>() * 4 + size_of::<u64>() + size_of::<u8>())?;
        }
    }
    let mut groups = reserved(groups_count)?;
    for (line_id, line) in chart.lines.iter().enumerate() {
        for (start, end, above) in SourceGroups::new(line) {
            if !eligible_group(line, start, end, above) {
                continue;
            }
            let count = end - start;
            let mut ids = reserved(count)?;
            let mut heights = reserved(count)?;
            let mut xs = reserved(count)?;
            let mut roles = reserved(count)?;
            for (offset, note) in line.notes[start..end].iter().enumerate() {
                let (x, role) = note_certificate(note).context("Tail source changed while capturing")?;
                ids.push(u32::try_from(start + offset)?);
                heights.push(note.height.to_bits());
                xs.push(x.to_bits());
                roles.push(role);
            }
            groups.push(ir::Group::new(u32::try_from(line_id)?, above, line.notes[start].speed.to_bits(), ids, heights, xs, roles));
        }
    }
    ir::compile(Sha256::digest(source_bytes).into(), descriptor.finalize().into(), groups)
}
fn index(plan: &ir::Workset) -> Result<Vec<GroupIndex>> {
    let mut result = reserved(plan.groups.len())?;
    for group in &plan.groups {
        let start = *group.note_ids.first().context("Tail group empty")?;
        let end = group
            .note_ids
            .last()
            .context("Tail group empty")?
            .checked_add(1)
            .context("Tail group end overflow")?;
        let styles = group.style_roles.iter().fold(0u8, |mask, role| mask | (1 << role));
        result.push(GroupIndex {
            line: group.line_id,
            start,
            end,
            styles,
        });
    }
    ensure!(
        result
            .windows(2)
            .all(|pair| (pair[0].line, pair[0].start) < (pair[1].line, pair[1].start)),
        "Tail groups not in source order"
    );
    Ok(result)
}

pub(crate) fn prepare(chart: &Chart, res: &Resource, source_bytes: &[u8]) -> (Option<Prepared>, Value) {
    super::set_enabled(res.config.chart_pre_compile);
    if !res.config.chart_pre_compile {
        return (None, json!({"requested":false,"installed":false,"cache_read":false,"cache_write":false}));
    }
    let begun = Instant::now();
    let (mut cache_read, mut cache_write) = (false, false);
    let result = (|| -> Result<(Prepared, Value)> {
        let manager = super::cache().context("Host has not configured the compiler cache")?;
        let expected = capture(chart, source_bytes)?;
        let key = ir::cache_key(expected.source, expected.descriptor);
        let ticket = manager.begin(&key)?.context("Compiler cache disabled")?;
        cache_read = true;
        let (mut cache_hit, mut published, mut cache_error) = (false, false, Value::Null);
        let cached = match manager.read_bounded(&ticket, ir::MAX_BYTES) {
            Ok(Some(bytes)) => match ir::decode(&bytes, &expected) {
                Ok(plan) => {
                    cache_hit = true;
                    Some((plan, bytes.len()))
                }
                Err(error) => {
                    cache_error = json!(format!("{error:#}"));
                    manager.invalidate_blob(&ticket)?;
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                cache_error = json!(format!("{error:#}"));
                manager.invalidate_blob(&ticket)?;
                None
            }
        };
        let (plan, bytes) = match cached {
            Some(value) => value,
            None => {
                let blob = ir::encode(&expected)?;
                cache_write = true;
                match manager.publish(&ticket, &blob) {
                    Ok(value) => published = value,
                    Err(error) => cache_error = json!(format!("{error:#}")),
                }
                (expected, blob.len())
            }
        };
        ensure!(manager.is_current(&ticket), "Tail workset invalidated by cache clear/disable");
        let lookup = index(&plan)?;
        let groups = plan.groups.len();
        let notes: usize = plan.groups.iter().map(|group| group.note_ids.len()).sum();
        let report = json!({"requested":true,"installed":true,"cache_hit":cache_hit,"published":published,
            "cache_read":cache_read,"cache_write":cache_write,"cache_error":cache_error,"key":key,"bytes":bytes,
            "groups":groups,"notes":notes,"root":manager.root(),"cpu_blob_persisted":cache_hit || published,
            "execution_enabled":execution_enabled(),"lossless":true,"scope":"Owner-sealed canonical plain-note tails; original judge/audio/control initialization."});
        let prepared = Prepared {
            plan,
            lease: CacheLease::new(manager.clone(), ticket),
            index: lookup,
            counters: Counters::default(),
            preparation: report.clone(),
        };
        Ok((prepared, report))
    })();
    let (mut plan, mut report) = match result {
        Ok((plan, report)) => (Some(plan), report),
        Err(error) => {
            (None, json!({"requested":true,"installed":false,"cache_read":cache_read,"cache_write":cache_write,"fallback":format!("{error:#}")}))
        }
    };
    report["elapsed_ms"] = json!(begun.elapsed().as_secs_f64() * 1000.);
    if let Some(plan) = plan.as_mut() {
        plan.preparation = report.clone();
    }
    (plan, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BpmList, ChartExtra, ChartSettings, CtrlObject, HitSound, JudgeLineCache, JudgeLineKind, Matrix, Object};
    use std::{cell::RefCell, sync::Arc};

    fn note(height: f64, speed: f64, above: bool, kind: NoteKind) -> Note {
        Note {
            object: Object::default(),
            kind,
            hitsound: HitSound::None,
            time: height + 10.,
            height,
            speed,
            color: macroquad::prelude::WHITE,
            fx_color: None,
            judge_area: 1.,
            above,
            multiple_hint: false,
            fake: false,
            judge: JudgeStatus::NotJudged,
        }
    }
    fn fixture() -> Chart {
        let mut notes = vec![note(
            0.,
            1.,
            true,
            NoteKind::Hold {
                end_time: 1.,
                end_height: 1.,
            },
        )];
        for (above, speed) in [(true, 1.), (true, 2.), (false, 1.)] {
            notes.extend((0..4).map(|i| note(i as f64, speed, above, NoteKind::Click)));
        }
        // Fixed zero Y still satisfies plain(), but is outside the typed-empty
        // canonical domain. Its entire speed run must retain the original loop.
        notes[2].object.translation.1 = AnimFloat::fixed(0.);
        let cache = JudgeLineCache::new(&mut notes);
        let line = JudgeLine {
            object: Object::default(),
            ctrl_obj: RefCell::new(CtrlObject::default()),
            kind: JudgeLineKind::Normal,
            height: AnimFloat::default(),
            incline: AnimFloat::default(),
            notes,
            color: Default::default(),
            parent: None,
            rot_with_parent: false,
            z_index: 0,
            show_below: false,
            attach_ui: None,
            cache,
        };
        Chart::new(0., vec![line], BpmList::new(vec![(0., 120.)]), ChartSettings::default(), ChartExtra::default(), Default::default())
    }

    fn group(heights: &[f64], xs: &[f32]) -> ir::Group {
        ir::compile(
            [1; 32],
            [2; 32],
            vec![ir::Group::new(
                0,
                true,
                1f64.to_bits(),
                (0..heights.len() as u32).collect(),
                heights.iter().map(|h| h.to_bits()).collect(),
                xs.iter().map(|x| x.to_bits()).collect(),
                vec![0; heights.len()],
            )],
        )
        .unwrap()
        .groups
        .remove(0)
    }
    #[test]
    fn original_base_order_survives_large_cancellation() {
        let (height, source_line, aspect, speed) = (10_000_000_000_000_002f64, 10_000_000_000_000_000f64, 3f64, 0.7f64);
        let line = source_line / aspect * speed;
        let expected = (height / aspect * speed - line) as f32;
        assert_eq!(source_base(height, aspect, speed, line).unwrap().to_bits(), expected.to_bits());
        assert_ne!(expected.to_bits(), (((height - source_line) / aspect * speed) as f32).to_bits());
    }
    #[test]
    fn tail_search_matches_exhaustive_nested_interval_proofs() {
        let heights: Vec<_> = (0..257).map(|i| i as f64 / 64.).collect();
        let xs: Vec<_> = (0..257).map(|i| if i % 3 == 0 { -0.5 } else { 0.5 }).collect();
        let group = group(&heights, &xs);
        let side = Side::new(&Matrix::identity()).unwrap();
        let hfs = [[0.1, 0.2], [0.17, 0.08]];
        for start in [0, 32, 64, 128, 256] {
            let exhaustive = (start..257).find(|&i| {
                side.outside_range(f32::from_bits(group.suffix_x_lo_bits[i]), f32::from_bits(group.suffix_x_hi_bits[i]), heights[i] as f32, 4., &hfs)
                    == Some(true)
            });
            assert_eq!(first_proved_tail(&group, start, 1., 1., 0., 1., &hfs, side), exhaustive);
        }
    }
    #[test]
    fn a_shared_tail_axis_is_required_and_empty_or_overflow_falls_back() {
        let side = Side::new(&Matrix::identity()).unwrap();
        let group = group(&[0., 2., 3., 4.], &[2., -2., 0., 0.]);
        assert_eq!(side.outside(2., 0., [0.1, 0.1]), Some(true));
        assert_eq!(side.outside(-2., 2., [0.1, 0.1]), Some(true));
        assert_eq!(side.outside_range(-2., 2., 0., 4., &[[0.1, 0.1]]), Some(false));
        assert_eq!(first_proved_tail(&group, 0, 1., 1., 0., 1., &[[0.1, 0.1]], side), Some(1));
        assert_eq!(first_proved_tail(&group, 4, 1., 1., 0., 1., &[[0.1, 0.1]], side), None);
        assert_eq!(first_proved_tail(&group, 0, 1., 1., 0., f32::MAX, &[[0.1, 0.1]], side), None);
        assert_eq!(first_proved_tail(&group, 0, 1., 1., 0., 1., &[], side), None);
    }
    #[test]
    fn terminal_initialization_preserves_original_order_and_rejects_zero() {
        for (height, line, speed) in [(7., 3., 1.0000000001), (-7., -3., 0.3), (1e100, 0., 2.)] {
            let original = (height - line + 0.0f32 as f64 / speed) * RPE_HEIGHT as f64 / 2.;
            assert_eq!(terminal_init_time(height, line, speed).unwrap().to_bits(), original.to_bits());
        }
        assert!(terminal_init_time(3., 3., 1.).is_none());
        assert!(terminal_init_time(-0., 0., 1.).is_none());
        assert!(terminal_init_time(f64::MAX, -f64::MAX, 1.).is_none());
    }
    #[test]
    fn capture_omits_the_whole_unsupported_cache_group() {
        let chart = fixture();
        let line = &chart.lines[0];
        assert_eq!(SourceGroups::new(line).collect::<Vec<_>>(), vec![(1, 5, true), (5, 9, true), (9, 13, false)]);
        assert!(!eligible_group(line, 1, 5, true));
        let plan = capture(&chart, b"same source").unwrap();
        assert_eq!(plan.groups.len(), 2);
        assert_eq!(plan.groups[0].note_ids, vec![5, 6, 7, 8]);
        assert_eq!(plan.groups[1].note_ids, vec![9, 10, 11, 12]);
        assert!(!plan.groups[1].above);
        assert_eq!(ir::decode(&ir::encode(&plan).unwrap(), &plan).unwrap(), plan);
    }
    #[test]
    fn loaded_descriptor_distinguishes_parser_changes_and_excludes_runtime_state() {
        let mut chart = fixture();
        let first = capture(&chart, b"same source").unwrap();
        chart.lines[0].notes[8].judge = JudgeStatus::Judged;
        chart.lines[0].notes[8].object.set_time(123.);
        chart.lines[0].ctrl_obj.borrow_mut().set_height(456.);
        let runtime_changed = capture(&chart, b"same source").unwrap();
        assert_eq!(runtime_changed, first);
        chart.lines[0].notes[8].time += 0.5;
        let loaded_changed = capture(&chart, b"same source").unwrap();
        assert_eq!(loaded_changed.source, first.source);
        assert_ne!(loaded_changed.descriptor, first.descriptor);
        assert_ne!(ir::cache_key(loaded_changed.source, loaded_changed.descriptor), ir::cache_key(first.source, first.descriptor));
        assert!(ir::decode(&ir::encode(&first).unwrap(), &loaded_changed).is_err());
    }
    #[test]
    fn advanced_cursor_lookup_and_shared_cache_epoch_revoke_the_plan() {
        let chart = fixture();
        let plan = capture(&chart, b"same source").unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let manager = Arc::new(super::super::cache::RenderCache::new(temporary.path().join("cache"), true).unwrap());
        let key = ir::cache_key(plan.source, plan.descriptor);
        let prepared = Prepared {
            index: index(&plan).unwrap(),
            plan,
            lease: CacheLease::new(manager.clone(), manager.begin(&key).unwrap().unwrap()),
            counters: Counters::default(),
            preparation: json!({"installed":true,"key":key}),
        };
        assert!(prepared.lookup(0, 3, true).is_none());
        assert_eq!(prepared.lookup(0, 7, true).unwrap().1.start, 5);
        assert_eq!(prepared.lookup(0, 9, false).unwrap().1.start, 9);
        assert!(prepared.lookup(0, 9, true).is_none());
        assert!(prepared.lookup(0, 7, false).is_none());
        assert!(prepared.lookup(0, 13, false).is_none());
        assert!(prepared.lookup(1, 7, true).is_none());
        assert!(prepared.is_current());
        manager.clear_all().unwrap();
        assert!(!prepared.is_current());
        let lease = CacheLease::new(manager.clone(), manager.begin(&key).unwrap().unwrap());
        manager.set_enabled(false);
        manager.set_enabled(true);
        assert!(!lease.is_current());
        assert_eq!(prepared.stats()["installed"], true);
    }
    #[test]
    fn payload_limits_reject_before_allocation_and_unsorted_runs_are_omitted() {
        let mut bytes = 0;
        assert!(charge(&mut bytes, usize::MAX, 2).is_err());
        let mut bytes = ir::MAX_BYTES - 8;
        assert!(charge(&mut bytes, 2, 8).is_err());
        let mut chart = fixture();
        chart.lines[0].notes[7].height = -1.;
        assert!(!eligible_group(&chart.lines[0], 5, 9, true));
        let plan = capture(&chart, b"same source").unwrap();
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].note_ids, vec![9, 10, 11, 12]);
    }
}
