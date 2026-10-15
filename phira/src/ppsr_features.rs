//! PPSR unified-model feature extraction.
//!
//! The base feature names and their order follow `services/chartFeatures.js`
//! (`modelFeatureNames`), and the seven appended values follow
//! `services/ppsrMoe.js` (`extraValues`). This module deliberately does not
//! cache or modify chart data.

use std::collections::{HashMap, HashSet};

use prpr::{
    core::{AnimFloat, Chart, NoteKind},
    info::ChartInfo,
};

pub const FEATURE_VERSION: u8 = 2;
pub const BASE_FEATURE_COUNT: usize = 114;
pub const AUGMENTED_FEATURE_COUNT: usize = 7;

pub const BASE_FEATURE_NAMES: [&str; BASE_FEATURE_COUNT] = [
    "g1_npsAvg",
    "g1_npsMechAvg",
    "g1_nps1sMax",
    "g1_npsMech1sMax",
    "g1_maxNotes2s",
    "g1_nps2sMax",
    "g1_maxNotes4s",
    "g1_nps4sMax",
    "g1_gapRatio",
    "g1_activeRatio",
    "g1_highDensityRatio",
    "g2_dtMin",
    "g2_dtP10",
    "g2_dtP50",
    "g2_streakMax",
    "g2_streakNoteRatio",
    "g3_jackRunMax",
    "g3_jackNoteRatio",
    "g3_trillRunMax",
    "g4_chordMax",
    "g4_chordNoteRatio",
    "g4_chord3NoteRatio",
    "g4_chordPerSecMax",
    "g5_distP50",
    "g5_distP90",
    "g5_speedP95",
    "g5_speedMax",
    "g5_crossScreenRatio",
    "g5_beyondToleranceRatio",
    "g5_handTravelP95",
    "g5_handTravelMax",
    "g5_handSpeedP95",
    "g5_handOverreachRatio",
    "g5_sameFingerRepeatMax",
    "g6_holdNoteRatio",
    "g6_holdTimeRatio",
    "g6_holdLenMean",
    "g6_holdLenMax",
    "g6_holdOverlapMax",
    "g6_holdOverlapDensityMax",
    "g6_holdOverlapAvg",
    "g6_holdMovingRatio",
    "g7_flickRatio",
    "g7_dragRatio",
    "g7_fakeRatio",
    "g7_lineCount",
    "g7_movingLineRatio",
    "g7_lineTravelXAvg",
    "g7_lineTravelXMax",
    "g7_lineRotAmpAvg",
    "g7_lineRotAmpMax",
    "g8_perfectRatio",
    "g8_goodRatio",
    "g8_badRatio",
    "g8_dtWindowP10",
    "g8_tightJackRatio",
    "g8_burstMax",
    "g8_burstP95",
    "g9_handSpeedP99",
    "g9_handSpeedMax",
    "g9_handExcessMean",
    "g9_handExcessP90",
    "g9_handImpossibleRatio",
    "g9_handStepP95",
    "g9_handStepMax",
    "g9_handTravel2sMax",
    "g9_handJump2sMax",
    "g9_crossSideRatio",
    "g9_sameSideFastRatio",
    "g9_dirChangeRatio",
    "g9_sideChordMax",
    "g9_sideChord3Ratio",
    "g10_bigramRepeatRatio",
    "g10_trigramRepeatRatio",
    "g10_distinctShapeRatio",
    "g10_sameIntervalRatio",
    "g10_sameShapeRatio",
    "g10_period2RunMax",
    "seg_npsMean",
    "seg_npsP90",
    "seg_npsMax",
    "seg_mechNpsMean",
    "seg_mechNpsP90",
    "seg_mechNpsMax",
    "seg_speedP95Mean",
    "seg_speedP95P90",
    "seg_speedP95Max",
    "seg_jackRatioMean",
    "seg_jackRatioP90",
    "seg_jackRatioMax",
    "seg_chordRatioMean",
    "seg_chordRatioP90",
    "seg_chordRatioMax",
    "seg_handSpeedP95Mean",
    "seg_handSpeedP95P90",
    "seg_handSpeedP95Max",
    "seg_handTravelMean",
    "seg_handTravelP90",
    "seg_handTravelMax",
    "seg_handJumpMean",
    "seg_handJumpP90",
    "seg_handJumpMax",
    "seg_patternRepeatMean",
    "seg_patternRepeatP90",
    "seg_patternRepeatMax",
    "seg_burstMaxMean",
    "seg_burstMaxP90",
    "seg_burstMaxMax",
    "seg_peakNps",
    "seg_peakPosition",
    "seg_activeRatio",
    "seg_headNps",
    "seg_tailNps",
    "seg_tailHeadRatio",
];

pub const AUGMENTED_FEATURE_NAMES: [&str; AUGMENTED_FEATURE_COUNT] = [
    "x_noteTotal",
    "x_mechTotal",
    "x_duration",
    "x_fakeRatio",
    "x_logNoteTotal",
    "x_logDuration",
    "x_meanNps",
];

const WINDOW_PERFECT: f64 = 0.08;
const WINDOW_GOOD: f64 = 0.16;
const WINDOW_BAD: f64 = 0.22;
const X_DIFF_MAX: f64 = 0.23625;
const STREAM_DT: f64 = 0.15;
const JACK_DT: f64 = 0.2;
const JACK_DX: f64 = 0.06;
const TRILL_DX: f64 = 0.12;
const CHORD_DT: f64 = 0.025;
const CROSS_SCREEN: f64 = 1.0;
const FINGER_REST: [f64; 4] = [-0.75, -0.25, 0.25, 0.75];
const FINGER_MAX_SPEED: f64 = 13.05;
const IMPOSSIBLE_EXCESS: f64 = 1.5;
const HAND_SPLIT: f64 = 0.0;
const HAND_WINDOW: f64 = 2.0;
const DT_BUCKET: f64 = 0.05;
const DX_BUCKET: f64 = 0.1;
const SAME_DT_TOL: f64 = 0.01;
const SAME_DX_TOL: f64 = 0.02;
const SEG_SIZE: f64 = 10.0;
const SEG_STEP: f64 = 5.0;

const GROUP_KEYS: [&str; 10] = [
    "density", "stream", "jack", "chord", "travel", "reach", "hold", "reading", "burst", "repeat",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationSource {
    /// The Rust `Chart` and `ChartInfo` types do not retain the raw chart's
    /// optional `duration`; this is the source extractor's `lastNoteTime` fallback.
    LastNonFakeNoteEndFallback,
}

#[derive(Debug, Clone)]
pub struct FeatureGroup {
    pub key: &'static str,
    /// Features are listed in the same order they occur in the model vector.
    pub features: Vec<NamedFeature>,
}

#[derive(Debug, Clone)]
pub struct NamedFeature {
    pub name: &'static str,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct PpsrMetadata {
    pub duration: f64,
    pub duration_source: DurationSource,
    pub note_total: usize,
    pub mech_total: usize,
    pub fake_total: usize,
    pub counts: NoteCounts,
    pub line_count: usize,
    pub moving_lines: usize,
    pub segment_count: usize,
    pub groups: Vec<FeatureGroup>,
    /// Limitations are explicit rather than silently inventing unavailable input.
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoteCounts {
    pub click: usize,
    pub hold: usize,
    pub flick: usize,
    pub drag: usize,
    pub fake: usize,
}

#[derive(Debug, Clone)]
pub struct PpsrFeaturePayload {
    pub version: u8,
    pub feature_names: &'static [&'static str; BASE_FEATURE_COUNT],
    /// Exact source-model order; all values have been rounded to four decimals.
    pub features: [f64; BASE_FEATURE_COUNT],
    pub metadata: PpsrMetadata,
}

impl PpsrFeaturePayload {
    /// Produce the compact Rhai payload. `features` is ordered exactly as the
    /// source `cache.names` vector; augmentation stays with the calling script.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": self.version,
            "features": self.features.to_vec(),
            "note_total": self.metadata.note_total,
            "mech_total": self.metadata.mech_total,
            "duration": self.metadata.duration,
            "fake_ratio": round4(self.metadata.fake_total as f64 / self.metadata.note_total.max(1) as f64),
            "metadata": {
                "duration_source": "last_non_fake_note_end_fallback",
                "fake_total": self.metadata.fake_total,
                "counts": {
                    "click": self.metadata.counts.click,
                    "hold": self.metadata.counts.hold,
                    "flick": self.metadata.counts.flick,
                    "drag": self.metadata.counts.drag,
                    "fake": self.metadata.counts.fake,
                },
                "line_count": self.metadata.line_count,
                "moving_lines": self.metadata.moving_lines,
                "segment_count": self.metadata.segment_count,
                "limitations": self.metadata.limitations,
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    InvalidAspectRatio,
    FeatureNameCountMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAspectRatio => write!(f, "ChartInfo.aspectRatio must be finite and positive"),
            Self::FeatureNameCountMismatch { expected, actual } => {
                write!(f, "expected {expected} model features, computed {actual}")
            }
        }
    }
}

impl std::error::Error for FeatureError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Click,
    Hold,
    Flick,
    Drag,
}

#[derive(Debug, Clone)]
struct NoteRow {
    t: f64,
    end: f64,
    kind: Kind,
    ax: f64,
    width: f64,
    line: usize,
    raw_t: f64,
    raw_end: f64,
    raw_x: f64,
}

impl NoteRow {
    fn is_mech(&self) -> bool {
        matches!(self.kind, Kind::Click | Kind::Hold)
    }
}

#[derive(Debug, Default)]
struct LineMotion {
    travel_x_avg: f64,
    travel_x_max: f64,
    rot_amp_avg: f64,
    rot_amp_max: f64,
}

#[derive(Debug, Default)]
struct JackStats {
    run_max: usize,
    covered: usize,
    pairs: usize,
    tight_pairs: usize,
}

#[derive(Debug, Default)]
struct HandStats {
    travel_p95: f64,
    travel_max: f64,
    speed_p95: f64,
    overreach_ratio: f64,
    same_finger_repeat_max: usize,
    speed_p99: f64,
    speed_max: f64,
    excess_mean: f64,
    excess_p90: f64,
    impossible_ratio: f64,
    step_p95: f64,
    step_max: f64,
    travel_2s_max: f64,
    jump_2s_max: f64,
    cross_side_ratio: f64,
    same_side_fast_ratio: f64,
    dir_change_ratio: f64,
    side_chord_max: usize,
    side_chord_3_ratio: f64,
}

#[derive(Debug, Default)]
struct HoldStats {
    count: usize,
    len_mean: f64,
    len_max: f64,
    hold_time: f64,
    overlap_max: usize,
    overlap_density_max: f64,
    inner_total: usize,
}

#[derive(Debug, Default)]
struct RepeatStats {
    bigram_repeat_ratio: f64,
    trigram_repeat_ratio: f64,
    distinct_shape_ratio: f64,
    same_interval_ratio: f64,
    same_shape_ratio: f64,
    period_2_run_max: usize,
}

#[derive(Debug, Default)]
struct SegmentMetrics {
    start: f64,
    nps: f64,
    mech_nps: f64,
    speed_p95: f64,
    jack_ratio: f64,
    chord_ratio: f64,
    hand_speed_p95: f64,
    hand_travel: f64,
    hand_jump: f64,
    pattern_repeat: f64,
    burst_max: usize,
}

/// Stable plugin entry point. The returned `features` member is a numeric
/// array in the exact 114-column order used by the unified-model cache.
pub fn extract(chart: &Chart, info: &ChartInfo) -> anyhow::Result<serde_json::Value> {
    extract_typed(chart, info).map(|payload| payload.to_json()).map_err(anyhow::Error::new)
}

/// Extract the 114 base model features and metadata needed by the caller.
///
/// `Chart` does not store the raw file's optional `duration`. Per the source
/// extractor's `chart.duration || lastNoteTime`, this uses the maximum end time
/// among non-fake notes, then clamps feature duration to at least 0.001 seconds.
fn extract_typed(chart: &Chart, info: &ChartInfo) -> Result<PpsrFeaturePayload, FeatureError> {
    let aspect = info.aspect_ratio as f64;
    if !aspect.is_finite() || aspect <= 0.0 {
        return Err(FeatureError::InvalidAspectRatio);
    }
    extract_inner(chart, info, aspect)
}

fn extract_inner(chart: &Chart, info: &ChartInfo, aspect: f64) -> Result<PpsrFeaturePayload, FeatureError> {
    let mut counts = NoteCounts::default();
    let mut notes = Vec::new();
    let mut moving_lines = 0;
    let line_count = chart.lines.len().max(1); // chart.js inserts one static line for an empty chart.
    let chart_line_count = chart.lines.len();
    let samplers = line_samplers(chart);

    for line_index in 0..chart_line_count {
        let line_moving = {
            let line = &chart.lines[line_index];
            has_event_track(&line.object.translation.0) || has_event_track(&line.object.translation.1) || has_event_track(&line.object.rotation)
        };
        if line_moving {
            moving_lines += 1;
        }
        let note_count = chart.lines[line_index].notes.len();
        for note_index in 0..note_count {
            let (raw_t, raw_end, raw_x, kind, fake, width, above) = {
                let note = &chart.lines[line_index].notes[note_index];
                let raw_t = finite_or_zero(note.time);
                let raw_end = match &note.kind {
                    NoteKind::Hold { end_time, .. } => finite_or_zero(*end_time),
                    _ => raw_t,
                };
                let raw_x = anim_at(&note.object.translation.0, raw_t).unwrap_or(0.0).clamp(-1.0, 1.0);
                let kind = match &note.kind {
                    NoteKind::Click => Kind::Click,
                    NoteKind::Hold { .. } => Kind::Hold,
                    NoteKind::Flick => Kind::Flick,
                    NoteKind::Drag => Kind::Drag,
                };
                (raw_t, raw_end, raw_x, kind, note.fake, finite_or_zero(note.judge_area as f64), note.above)
            };
            if fake {
                counts.fake += 1;
                continue;
            }
            match kind {
                Kind::Click => counts.click += 1,
                Kind::Hold => counts.hold += 1,
                Kind::Flick => counts.flick += 1,
                Kind::Drag => counts.drag += 1,
            }

            let (world_x, world_y, rot) = line_pose_at(&samplers, line_index, raw_t, aspect);
            let raw_ax = world_x + raw_x * rot.cos();
            let raw_ay = -world_y + raw_x * rot.sin();
            let _ = (raw_ay, above); // Y and side are retained by the source stream but not modeled.
            notes.push(NoteRow {
                t: round6(raw_t),
                end: round6(raw_end),
                kind,
                ax: round6(raw_ax),
                width: round6(width),
                line: line_index,
                raw_t,
                raw_end,
                raw_x,
            });
        }
    }

    notes.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.line.cmp(&b.line)).then_with(|| a.ax.total_cmp(&b.ax)));
    let note_total = notes.len();
    let mech_total = notes.iter().filter(|note| note.is_mech()).count();
    let fake_total = counts.fake;
    let last_note_time = notes.iter().fold(0.0_f64, |last, note| last.max(note.end));
    let duration = round6(last_note_time).max(0.001);
    let mech: Vec<_> = notes.iter().filter(|note| note.is_mech()).cloned().collect();
    let mech_times: Vec<_> = mech.iter().map(|note| note.t).collect();
    let note_times: Vec<_> = notes.iter().map(|note| note.t).collect();

    let line_motion = line_motion_stats(&samplers, duration, aspect);
    let hold_moving_ratio = hold_motion_ratio(&samplers, &notes, aspect);
    let (values, segment_count) =
        compute_features(&notes, &mech, &mech_times, &note_times, duration, counts, line_count, moving_lines, &line_motion, hold_moving_ratio);

    let present = BASE_FEATURE_NAMES.iter().filter(|name| values.contains_key(**name)).count();
    if present != BASE_FEATURE_COUNT {
        return Err(FeatureError::FeatureNameCountMismatch {
            expected: BASE_FEATURE_COUNT,
            actual: present,
        });
    }
    let ordered = BASE_FEATURE_NAMES.map(|name| round4(values[name]));
    let groups = build_groups(&ordered);

    let mut limitations = vec![String::from(
        "Chart/ChartInfo omit raw chart.duration and the format-specific loader maxTime; the chartNotes.js last-non-fake-note-end fallback is used and can omit trailing silence or a later fake note.",
    )];
    if !matches!(info.format.as_ref(), Some(prpr::info::ChartFormat::Rpe | prpr::info::ChartFormat::Pec)) {
        limitations.push(String::from(
            "Raw event-layer presence is unavailable (or the chart is not RPE/PEC); moving-line classification is inferred from non-default Rust translation/rotation animations.",
        ));
    }

    Ok(PpsrFeaturePayload {
        version: FEATURE_VERSION,
        feature_names: &BASE_FEATURE_NAMES,
        features: ordered,
        metadata: PpsrMetadata {
            duration: round4(duration),
            duration_source: DurationSource::LastNonFakeNoteEndFallback,
            note_total,
            mech_total,
            fake_total,
            counts,
            line_count,
            moving_lines,
            segment_count,
            groups,
            limitations,
        },
    })
}

struct LineSampler<'a> {
    x: &'a AnimFloat,
    y: &'a AnimFloat,
    rotation: &'a AnimFloat,
}

fn line_samplers(chart: &Chart) -> Vec<LineSampler<'_>> {
    chart
        .lines
        .iter()
        .map(|line| LineSampler {
            x: &line.object.translation.0,
            y: &line.object.translation.1,
            rotation: &line.object.rotation,
        })
        .collect()
}

fn line_pose_at(lines: &[LineSampler<'_>], line_index: usize, time: f64, aspect: f64) -> (f64, f64, f64) {
    let line = &lines[line_index];
    let local_x = finite_or_zero(anim_at(line.x, time).unwrap_or_default());
    // chart.js treats moveY as RPE world units; prpr's rendered translation
    // divides Y by aspect ratio, so undo that conversion here.
    let local_y = finite_or_zero(anim_at(line.y, time).unwrap_or_default()) * aspect;
    let rot = finite_or_zero(anim_at(line.rotation, time).unwrap_or_default()).to_radians();
    (local_x, local_y, rot)
}

fn anim_at(anim: &AnimFloat, time: f64) -> Option<f64> {
    let keyframes = &anim.keyframes;
    let current = if keyframes.is_empty() {
        None
    } else if time <= keyframes[0].time {
        Some(keyframes[0].value as f64)
    } else {
        let right = keyframes.partition_point(|keyframe| keyframe.time <= time);
        if right >= keyframes.len() {
            Some(keyframes[keyframes.len() - 1].value as f64)
        } else {
            let left = right - 1;
            let first = &keyframes[left];
            let second = &keyframes[right];
            let span = second.time - first.time;
            if span <= 0.0 {
                Some(first.value as f64)
            } else {
                let progress = ((time - first.time) / span) as f32;
                let eased = first.tween.y(progress);
                Some((first.value + (second.value - first.value) * eased) as f64)
            }
        }
    };
    let next = anim.next.as_deref().and_then(|next| anim_at(next, time));
    match (current, next) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    }
}

fn has_event_track(anim: &AnimFloat) -> bool {
    anim.keyframes.len() > 1 || anim.next.as_deref().is_some_and(has_event_track)
}

fn line_motion_stats(lines: &[LineSampler<'_>], duration: f64, aspect: f64) -> LineMotion {
    if lines.is_empty() {
        return LineMotion::default();
    }
    let span = duration.max(1.0);
    let step = (span / 200.0).max(0.05);
    let mut total_x = 0.0;
    let mut total_rot = 0.0;
    let mut max_x = 0.0_f64;
    let mut max_rot = 0.0_f64;

    for line_index in 0..lines.len() {
        let mut prev: Option<(f64, f64)> = None;
        let mut travel_x = 0.0;
        let mut min_rot = f64::INFINITY;
        let mut max_angle = f64::NEG_INFINITY;
        let mut time = 0.0;
        while time <= span {
            let (x, _, rot) = line_pose_at(lines, line_index, time, aspect);
            if let Some((prev_x, _)) = prev {
                travel_x += (x - prev_x).abs();
            }
            prev = Some((x, rot));
            min_rot = min_rot.min(rot);
            max_angle = max_angle.max(rot);
            time += step;
        }
        total_x += travel_x;
        total_rot += (max_angle - min_rot).max(0.0);
        max_x = max_x.max(travel_x);
        max_rot = max_rot.max((max_angle - min_rot).max(0.0));
    }
    let denom = lines.len().max(1) as f64;
    LineMotion {
        travel_x_avg: total_x / denom,
        travel_x_max: max_x,
        rot_amp_avg: total_rot / denom,
        rot_amp_max: max_rot,
    }
}

fn hold_motion_ratio(lines: &[LineSampler<'_>], notes: &[NoteRow], aspect: f64) -> f64 {
    let mut total = 0;
    let mut moving = 0;
    for note in notes.iter().filter(|note| note.kind == Kind::Hold) {
        total += 1;
        let start_pose = line_pose_at(lines, note.line, note.raw_t, aspect);
        let start_x = start_pose.0 + note.raw_x * start_pose.2.cos();
        let end_pose = line_pose_at(lines, note.line, note.raw_end, aspect);
        let end_x = end_pose.0 + note.raw_x * end_pose.2.cos();
        if (end_x - start_x).abs() > 0.1 {
            moving += 1;
        }
    }
    if total > 0 {
        moving as f64 / total as f64
    } else {
        0.0
    }
}

fn compute_features(
    notes: &[NoteRow],
    mech: &[NoteRow],
    mech_times: &[f64],
    times: &[f64],
    duration: f64,
    counts: NoteCounts,
    line_count: usize,
    moving_lines: usize,
    motion: &LineMotion,
    hold_moving_ratio: f64,
) -> (HashMap<&'static str, f64>, usize) {
    let seconds = duration.ceil().max(1.0);
    let mut per_second: HashMap<usize, usize> = HashMap::new();
    let mut mech_per_second: HashMap<usize, usize> = HashMap::new();
    for note in notes {
        let second = note.t.floor();
        if second >= 0.0 {
            let bucket = second.min(seconds - 1.0) as usize;
            *per_second.entry(bucket).or_default() += 1;
        }
    }
    for note in mech {
        let second = note.t.floor();
        if second >= 0.0 {
            let bucket = second.min(seconds - 1.0) as usize;
            *mech_per_second.entry(bucket).or_default() += 1;
        }
    }
    let active_seconds = per_second.len();
    let high_seconds = per_second.values().filter(|&&count| count >= 8).count();
    let max_nps = per_second.values().copied().max().unwrap_or(0);
    let max_mech_nps = mech_per_second.values().copied().max().unwrap_or(0);

    let mut mech_gaps = gaps(mech);
    mech_gaps.sort_by(f64::total_cmp);
    let chains = chain_runs(mech_times, STREAM_DT, 4);
    let jack = jack_stats(mech);
    let trill_run_max = trill_run_max(mech);

    let chords = chord_groups(notes);
    let chord_covered: usize = chords.iter().filter(|&&size| size >= 2).sum();
    let chord_3_covered: usize = chords.iter().filter(|&&size| size >= 3).sum();
    let mut chords_per_second: HashMap<usize, usize> = HashMap::new();
    let mut start = 0;
    for i in 1..=notes.len() {
        if i < notes.len() && notes[i].t - notes[start].t <= CHORD_DT {
            continue;
        }
        if i - start >= 2 {
            let second = notes[start].t.floor();
            if second >= 0.0 {
                let bucket = second.min(seconds - 1.0) as usize;
                *chords_per_second.entry(bucket).or_default() += 1;
            }
        }
        start = i;
    }
    let chord_per_sec_max = chords_per_second.values().copied().max().unwrap_or(0);

    let mut jump_dist = Vec::new();
    let mut jump_speed = Vec::new();
    let mut cross_screen = 0;
    let mut beyond_tolerance = 0;
    let mut pairs = 0;
    for pair in mech.windows(2) {
        let dt = pair[1].t - pair[0].t;
        if dt <= 0.001 {
            continue;
        }
        let dist = (pair[1].ax - pair[0].ax).abs();
        pairs += 1;
        jump_dist.push(dist);
        jump_speed.push(dist / dt);
        if dist >= CROSS_SCREEN {
            cross_screen += 1;
        }
        let previous_width = if pair[0].width == 0.0 { 1.0 } else { pair[0].width };
        let current_width = if pair[1].width == 0.0 { 1.0 } else { pair[1].width };
        if dist > (previous_width + current_width) * X_DIFF_MAX {
            beyond_tolerance += 1;
        }
    }
    jump_dist.sort_by(f64::total_cmp);
    jump_speed.sort_by(f64::total_cmp);
    let hand = hand_stats(mech);
    let repeat = repeat_stats(mech);
    let holds = hold_stats(notes, mech_times);
    let tight_jack_ratio = if jack.pairs > 0 {
        jack.tight_pairs as f64 / jack.pairs as f64
    } else {
        0.0
    };

    let (segments, segment_count, final_segment_end) = build_segments(notes, duration);
    let segment_summary = summarize_segments(&segments, segment_count, final_segment_end);
    let mut burst_values: Vec<f64> = segments.iter().map(|segment| segment.burst_max as f64).collect();
    burst_values.sort_by(f64::total_cmp);

    let n = notes.len() as f64;
    let m = mech.len() as f64;
    let mut f = HashMap::with_capacity(BASE_FEATURE_COUNT);
    let mut put = |name, value| {
        f.insert(name, finite_or_zero(value));
    };

    put("g1_npsAvg", n / duration);
    put("g1_npsMechAvg", m / duration);
    put("g1_nps1sMax", max_nps as f64);
    put("g1_npsMech1sMax", max_mech_nps as f64);
    put("g1_maxNotes2s", max_notes_in_window(times, 2.0) as f64);
    put("g1_nps2sMax", max_notes_in_window(times, 2.0) as f64 / 2.0);
    put("g1_maxNotes4s", max_notes_in_window(times, 4.0) as f64);
    put("g1_nps4sMax", max_notes_in_window(times, 4.0) as f64 / 4.0);
    put("g1_gapRatio", 1.0 - active_seconds as f64 / seconds as f64);
    put("g1_activeRatio", active_seconds as f64 / seconds as f64);
    put("g1_highDensityRatio", high_seconds as f64 / seconds as f64);

    put("g2_dtMin", mech_gaps.first().copied().unwrap_or(0.0));
    put("g2_dtP10", percentile(&mech_gaps, 0.1));
    put("g2_dtP50", percentile(&mech_gaps, 0.5));
    put("g2_streakMax", chains.0 as f64);
    put("g2_streakNoteRatio", if m > 0.0 { chains.1 as f64 / m } else { 0.0 });

    put("g3_jackRunMax", jack.run_max as f64);
    put("g3_jackNoteRatio", if m > 0.0 { jack.covered as f64 / m } else { 0.0 });
    put("g3_trillRunMax", trill_run_max as f64);

    put("g4_chordMax", chords.iter().copied().max().unwrap_or(0) as f64);
    put("g4_chordNoteRatio", if n > 0.0 { chord_covered as f64 / n } else { 0.0 });
    put("g4_chord3NoteRatio", if n > 0.0 { chord_3_covered as f64 / n } else { 0.0 });
    put("g4_chordPerSecMax", chord_per_sec_max as f64);

    put("g5_distP50", percentile(&jump_dist, 0.5));
    put("g5_distP90", percentile(&jump_dist, 0.9));
    put("g5_speedP95", percentile(&jump_speed, 0.95));
    put("g5_speedMax", jump_speed.last().copied().unwrap_or(0.0));
    put("g5_crossScreenRatio", ratio(cross_screen, pairs));
    put("g5_beyondToleranceRatio", ratio(beyond_tolerance, pairs));
    put("g5_handTravelP95", hand.travel_p95);
    put("g5_handTravelMax", hand.travel_max);
    put("g5_handSpeedP95", hand.speed_p95);
    put("g5_handOverreachRatio", hand.overreach_ratio);
    put("g5_sameFingerRepeatMax", hand.same_finger_repeat_max as f64);

    put("g6_holdNoteRatio", if n > 0.0 { holds.count as f64 / n } else { 0.0 });
    put("g6_holdTimeRatio", holds.hold_time / duration);
    put("g6_holdLenMean", holds.len_mean);
    put("g6_holdLenMax", holds.len_max);
    put("g6_holdOverlapMax", holds.overlap_max as f64);
    put("g6_holdOverlapDensityMax", holds.overlap_density_max);
    put(
        "g6_holdOverlapAvg",
        if holds.count > 0 {
            holds.inner_total as f64 / holds.count as f64
        } else {
            0.0
        },
    );
    put("g6_holdMovingRatio", hold_moving_ratio);

    put("g7_flickRatio", if n > 0.0 { counts.flick as f64 / n } else { 0.0 });
    put("g7_dragRatio", if n > 0.0 { counts.drag as f64 / n } else { 0.0 });
    put("g7_fakeRatio", if n > 0.0 { counts.fake as f64 / n } else { 0.0 });
    put("g7_lineCount", line_count as f64);
    put("g7_movingLineRatio", ratio(moving_lines, line_count));
    put("g7_lineTravelXAvg", motion.travel_x_avg);
    put("g7_lineTravelXMax", motion.travel_x_max);
    put("g7_lineRotAmpAvg", motion.rot_amp_avg);
    put("g7_lineRotAmpMax", motion.rot_amp_max);

    put("g8_perfectRatio", ratio(mech_gaps.iter().filter(|&&dt| dt < WINDOW_PERFECT).count(), mech_gaps.len()));
    put("g8_goodRatio", ratio(mech_gaps.iter().filter(|&&dt| dt < WINDOW_GOOD).count(), mech_gaps.len()));
    put("g8_badRatio", ratio(mech_gaps.iter().filter(|&&dt| dt < WINDOW_BAD).count(), mech_gaps.len()));
    put("g8_dtWindowP10", percentile(&mech_gaps, 0.1) / WINDOW_BAD);
    put("g8_tightJackRatio", tight_jack_ratio);
    put("g8_burstMax", max_notes_in_window(times, WINDOW_BAD) as f64);
    put("g8_burstP95", percentile(&burst_values, 0.95));

    put("g9_handSpeedP99", hand.speed_p99);
    put("g9_handSpeedMax", hand.speed_max);
    put("g9_handExcessMean", hand.excess_mean);
    put("g9_handExcessP90", hand.excess_p90);
    put("g9_handImpossibleRatio", hand.impossible_ratio);
    put("g9_handStepP95", hand.step_p95);
    put("g9_handStepMax", hand.step_max);
    put("g9_handTravel2sMax", hand.travel_2s_max);
    put("g9_handJump2sMax", hand.jump_2s_max);
    put("g9_crossSideRatio", hand.cross_side_ratio);
    put("g9_sameSideFastRatio", hand.same_side_fast_ratio);
    put("g9_dirChangeRatio", hand.dir_change_ratio);
    put("g9_sideChordMax", hand.side_chord_max as f64);
    put("g9_sideChord3Ratio", hand.side_chord_3_ratio);

    put("g10_bigramRepeatRatio", repeat.bigram_repeat_ratio);
    put("g10_trigramRepeatRatio", repeat.trigram_repeat_ratio);
    put("g10_distinctShapeRatio", repeat.distinct_shape_ratio);
    put("g10_sameIntervalRatio", repeat.same_interval_ratio);
    put("g10_sameShapeRatio", repeat.same_shape_ratio);
    put("g10_period2RunMax", repeat.period_2_run_max as f64);

    for (name, value) in segment_summary {
        put(name, value);
    }
    (f, segment_count)
}

fn build_segments(notes: &[NoteRow], duration: f64) -> (Vec<SegmentMetrics>, usize, f64) {
    if notes.is_empty() {
        return (Vec::new(), 0, 0.0);
    }
    let regular_count = if duration >= SEG_SIZE {
        ((duration - SEG_SIZE) / SEG_STEP).floor() as usize + 1
    } else {
        0
    };
    let tail = (duration - SEG_SIZE).max(0.0);
    let has_tail = regular_count == 0 || ((regular_count - 1) as f64 * SEG_STEP - tail).abs() >= 1e-6;
    let total_count = regular_count + usize::from(has_tail);
    let final_start = tail;
    let final_end = final_start + SEG_SIZE;

    // A 10-second interval on a 5-second stride can cover each note in at
    // most two regular windows. Index only occupied windows so very long,
    // sparse charts do not allocate or iterate one empty window per 5 seconds.
    let mut occupied: HashMap<i64, Vec<usize>> = HashMap::new();
    for (note_index, note) in notes.iter().enumerate() {
        let max_regular = regular_count.saturating_sub(1);
        let last = (note.t / SEG_STEP).floor();
        if last >= 0.0 && regular_count > 0 {
            let last = (last as usize).min(max_regular);
            let first = last.saturating_sub(2);
            for index in first..=last {
                let start = index as f64 * SEG_STEP;
                if note.t >= start && note.t < start + SEG_SIZE {
                    occupied.entry(index as i64).or_default().push(note_index);
                }
            }
        }
        if has_tail && note.t >= tail && note.t < tail + SEG_SIZE {
            occupied.entry(-1).or_default().push(note_index);
        }
    }
    let mut windows: Vec<_> = occupied
        .into_iter()
        .map(|(index, note_indices)| {
            let start = if index == -1 { tail } else { index as f64 * SEG_STEP };
            (start, note_indices)
        })
        .collect();
    windows.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut segments = Vec::with_capacity(windows.len());
    for (start, note_indices) in windows {
        let end = start + SEG_SIZE;
        let subset: Vec<_> = note_indices.into_iter().map(|index| notes[index].clone()).collect();
        debug_assert!(subset.iter().all(|note| note.t >= start && note.t < end));
        segments.push(window_metrics(&subset, start));
    }
    (segments, total_count, final_end)
}

fn window_metrics(subset: &[NoteRow], start: f64) -> SegmentMetrics {
    let mech: Vec<_> = subset.iter().filter(|note| note.is_mech()).cloned().collect();
    let times: Vec<_> = subset.iter().map(|note| note.t).collect();
    let chords = chord_groups(subset);
    let mut speeds = Vec::new();
    for pair in mech.windows(2) {
        let dt = pair[1].t - pair[0].t;
        if dt <= 0.001 {
            continue;
        }
        let dist = (pair[1].ax - pair[0].ax).abs();
        speeds.push(dist / dt);
    }
    speeds.sort_by(f64::total_cmp);
    let jack = jack_stats(&mech);
    let hand = hand_stats(&mech);
    let repeat = repeat_stats(&mech);
    SegmentMetrics {
        start,
        nps: subset.len() as f64 / SEG_SIZE,
        mech_nps: mech.len() as f64 / SEG_SIZE,
        speed_p95: percentile(&speeds, 0.95),
        jack_ratio: if !mech.is_empty() {
            jack.covered as f64 / mech.len() as f64
        } else {
            0.0
        },
        chord_ratio: if !subset.is_empty() {
            chords.iter().filter(|&&size| size >= 2).sum::<usize>() as f64 / subset.len() as f64
        } else {
            0.0
        },
        hand_speed_p95: hand.speed_p95,
        hand_travel: hand.travel_2s_max / HAND_WINDOW,
        hand_jump: hand.jump_2s_max,
        pattern_repeat: repeat.bigram_repeat_ratio,
        burst_max: max_notes_in_window(&times, WINDOW_BAD),
    }
}

fn summarize_segments(segments: &[SegmentMetrics], total_count: usize, final_end: f64) -> HashMap<&'static str, f64> {
    let active: Vec<_> = segments.iter().collect();
    let mut result = HashMap::new();
    let metrics: [(&str, fn(&SegmentMetrics) -> f64); 10] = [
        ("nps", |s| s.nps),
        ("mechNps", |s| s.mech_nps),
        ("speedP95", |s| s.speed_p95),
        ("jackRatio", |s| s.jack_ratio),
        ("chordRatio", |s| s.chord_ratio),
        ("handSpeedP95", |s| s.hand_speed_p95),
        ("handTravel", |s| s.hand_travel),
        ("handJump", |s| s.hand_jump),
        ("patternRepeat", |s| s.pattern_repeat),
        ("burstMax", |s| s.burst_max as f64),
    ];
    for (name, get) in metrics {
        let mut values: Vec<_> = active.iter().map(|segment| get(segment)).collect();
        values.sort_by(f64::total_cmp);
        let (mean_name, p90_name, max_name) = match name {
            "nps" => ("seg_npsMean", "seg_npsP90", "seg_npsMax"),
            "mechNps" => ("seg_mechNpsMean", "seg_mechNpsP90", "seg_mechNpsMax"),
            "speedP95" => ("seg_speedP95Mean", "seg_speedP95P90", "seg_speedP95Max"),
            "jackRatio" => ("seg_jackRatioMean", "seg_jackRatioP90", "seg_jackRatioMax"),
            "chordRatio" => ("seg_chordRatioMean", "seg_chordRatioP90", "seg_chordRatioMax"),
            "handSpeedP95" => ("seg_handSpeedP95Mean", "seg_handSpeedP95P90", "seg_handSpeedP95Max"),
            "handTravel" => ("seg_handTravelMean", "seg_handTravelP90", "seg_handTravelMax"),
            "handJump" => ("seg_handJumpMean", "seg_handJumpP90", "seg_handJumpMax"),
            "patternRepeat" => ("seg_patternRepeatMean", "seg_patternRepeatP90", "seg_patternRepeatMax"),
            _ => ("seg_burstMaxMean", "seg_burstMaxP90", "seg_burstMaxMax"),
        };
        result.insert(
            mean_name,
            if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f64>() / values.len() as f64
            },
        );
        result.insert(p90_name, percentile(&values, 0.9));
        result.insert(max_name, values.last().copied().unwrap_or(0.0));
    }

    let mut peak_nps = if total_count > 0 { 0.0 } else { -1.0 };
    let mut peak_start = 0.0;
    for segment in segments {
        if segment.nps > peak_nps {
            peak_nps = segment.nps;
            peak_start = segment.start;
        }
    }
    result.insert("seg_peakNps", peak_nps.max(0.0));
    result.insert("seg_peakPosition", if final_end > 0.0 { peak_start / final_end } else { 0.0 });
    result.insert(
        "seg_activeRatio",
        if total_count > 0 {
            active.len() as f64 / total_count as f64
        } else {
            0.0
        },
    );

    let mut by_time = active;
    by_time.sort_by(|a, b| a.start.total_cmp(&b.start));
    let count = ((by_time.len() as f64 * 0.2).round() as usize).max(1);
    let mean_nps = |iter: &[&SegmentMetrics]| {
        if iter.is_empty() {
            0.0
        } else {
            iter.iter().map(|s| s.nps).sum::<f64>() / iter.len() as f64
        }
    };
    let head = mean_nps(&by_time[..count.min(by_time.len())]);
    let tail = mean_nps(&by_time[by_time.len().saturating_sub(count)..]);
    result.insert("seg_headNps", head);
    result.insert("seg_tailNps", tail);
    result.insert("seg_tailHeadRatio", if head > 0.0 { tail / head } else { 0.0 });
    result
}

fn build_groups(values: &[f64; BASE_FEATURE_COUNT]) -> Vec<FeatureGroup> {
    let mut groups: Vec<_> = GROUP_KEYS.iter().map(|key| FeatureGroup { key, features: Vec::new() }).collect();
    for (index, name) in BASE_FEATURE_NAMES.iter().enumerate() {
        if let Some(group_index) = part_group_index(name) {
            groups[group_index].features.push(NamedFeature { name, value: values[index] });
        }
    }
    groups
}

fn part_group_index(name: &str) -> Option<usize> {
    if name.starts_with("meta_") {
        return None;
    }
    if name.starts_with("seg_") {
        let metric_mapping = [
            ("seg_speedP95", 4),
            ("seg_jackRatio", 2),
            ("seg_chordRatio", 3),
            ("seg_handSpeedP95", 5),
            ("seg_handTravel", 5),
            ("seg_handJump", 5),
            ("seg_patternRepeat", 9),
            ("seg_burstMax", 8),
        ];
        if let Some((_, index)) = metric_mapping.iter().find(|(prefix, _)| name.starts_with(prefix)) {
            return Some(*index);
        }
        return Some(0);
    }
    let group = name.split('_').next()?;
    Some(match group {
        "g1" => 0,
        "g2" => 1,
        "g3" => 2,
        "g4" => 3,
        "g5" if name.contains("hand") => 5,
        "g5" => 4,
        "g6" => 6,
        "g7" => 7,
        "g8" => 8,
        "g9" => 5,
        "g10" => 9,
        _ => return None,
    })
}

fn hold_stats(notes: &[NoteRow], mech_times: &[f64]) -> HoldStats {
    let mut stats = HoldStats::default();
    let holds: Vec<_> = notes.iter().filter(|note| note.kind == Kind::Hold).collect();
    stats.count = holds.len();
    for hold in holds {
        let length = (hold.end - hold.t).max(0.0);
        stats.len_max = stats.len_max.max(length);
        stats.hold_time += length;
        let low = upper_bound(mech_times, hold.t + 0.001);
        let high = upper_bound(mech_times, hold.end);
        let inner = high.saturating_sub(low);
        stats.inner_total += inner;
        stats.overlap_max = stats.overlap_max.max(inner);
        if length > 0.0 {
            stats.overlap_density_max = stats.overlap_density_max.max(inner as f64 / length);
        }
    }
    if stats.count > 0 {
        stats.len_mean = stats.hold_time / stats.count as f64;
    }
    stats
}

fn hand_stats(mech: &[NoteRow]) -> HandStats {
    let mut fingers = FINGER_REST;
    let mut last_time = [f64::NEG_INFINITY; 4];
    let mut travels = Vec::with_capacity(mech.len());
    let mut speeds = Vec::with_capacity(mech.len());
    let mut repeat_max = 0;
    let mut repeat_run = 0;
    let mut last_finger: Option<usize> = None;
    for note in mech {
        let mut pick = 0;
        let mut pick_dist = f64::INFINITY;
        for finger in 0..4 {
            let dist = (fingers[finger] - note.ax).abs();
            if dist < pick_dist - 1e-9 {
                pick_dist = dist;
                pick = finger;
            }
        }
        let dt = note.t - last_time[pick];
        travels.push(pick_dist);
        speeds.push(if dt.is_finite() && dt > 0.001 { pick_dist / dt } else { 0.0 });
        if last_finger == Some(pick) {
            repeat_run += 1;
            repeat_max = repeat_max.max(repeat_run);
        } else {
            repeat_run = 1;
        }
        last_finger = Some(pick);
        fingers[pick] = note.ax;
        last_time[pick] = note.t;
    }
    travels.sort_by(f64::total_cmp);
    speeds.sort_by(f64::total_cmp);
    let impossible_line = FINGER_MAX_SPEED * IMPOSSIBLE_EXCESS;
    let mut overreach = 0;
    let mut impossible = 0;
    let mut excesses = Vec::with_capacity(speeds.len());
    let mut excess_sum = 0.0;
    for &speed in &speeds {
        if speed > FINGER_MAX_SPEED {
            overreach += 1;
        }
        if speed > impossible_line {
            impossible += 1;
        }
        let excess = (speed - FINGER_MAX_SPEED).max(0.0);
        excess_sum += excess;
        excesses.push(excess);
    }
    excesses.sort_by(f64::total_cmp);

    let mut steps: Vec<(f64, f64)> = Vec::new();
    let (mut pairs, mut cross_side, mut same_side_fast, mut dir_pairs, mut dir_changes) = (0, 0, 0, 0, 0);
    let (mut prev_side, mut prev_x, mut prev_t) = (None, 0.0, 0.0);
    let mut prev_sign = 0_i8;
    for note in mech {
        let side = side(note.ax);
        let dt = note.t - prev_t;
        if let Some(old_side) = prev_side {
            if dt > 0.0005 {
                pairs += 1;
                let dx = note.ax - prev_x;
                if side != old_side {
                    cross_side += 1;
                } else {
                    steps.push((note.t, dx.abs()));
                    if dt <= WINDOW_GOOD {
                        same_side_fast += 1;
                    }
                    let sign = if dx > 1e-9 {
                        1
                    } else if dx < -1e-9 {
                        -1
                    } else {
                        0
                    };
                    if sign != 0 {
                        dir_pairs += 1;
                        if prev_sign != 0 && sign != prev_sign {
                            dir_changes += 1;
                        }
                        prev_sign = sign;
                    }
                }
            }
        }
        prev_side = Some(side);
        prev_x = note.ax;
        prev_t = note.t;
    }

    let (mut travel_2s_max, mut jump_2s_max, mut sum, mut left, mut head) = (0.0_f64, 0.0_f64, 0.0_f64, 0_usize, 0_usize);
    let mut deque: Vec<usize> = Vec::new();
    for right in 0..steps.len() {
        sum += steps[right].1;
        while deque.len() > head && steps[*deque.last().unwrap()].1 <= steps[right].1 {
            deque.pop();
        }
        deque.push(right);
        while steps[right].0 - steps[left].0 > HAND_WINDOW {
            sum -= steps[left].1;
            left += 1;
            while head < deque.len() && deque[head] < left {
                head += 1;
            }
        }
        travel_2s_max = travel_2s_max.max(sum);
        if head < deque.len() {
            jump_2s_max = jump_2s_max.max(steps[deque[head]].1);
        }
    }
    let mut sorted_steps: Vec<_> = steps.iter().map(|step| step.1).collect();
    sorted_steps.sort_by(f64::total_cmp);

    let mut side_chord_max = 0;
    let mut side_chord_3_notes = 0;
    let mut group_start = 0;
    for i in 1..=mech.len() {
        if i < mech.len() && mech[i].t - mech[group_start].t <= CHORD_DT {
            continue;
        }
        if i - group_start >= 2 {
            let left_count = mech[group_start..i].iter().filter(|note| side(note.ax) == 0).count();
            let worst = left_count.max(i - group_start - left_count);
            side_chord_max = side_chord_max.max(worst);
            if worst >= 3 {
                side_chord_3_notes += i - group_start;
            }
        }
        group_start = i;
    }
    HandStats {
        travel_p95: percentile(&travels, 0.95),
        travel_max: travels.last().copied().unwrap_or(0.0),
        speed_p95: percentile(&speeds, 0.95),
        overreach_ratio: ratio(overreach, speeds.len()),
        same_finger_repeat_max: repeat_max,
        speed_p99: percentile(&speeds, 0.99),
        speed_max: speeds.last().copied().unwrap_or(0.0),
        excess_mean: if speeds.is_empty() { 0.0 } else { excess_sum / speeds.len() as f64 },
        excess_p90: percentile(&excesses, 0.9),
        impossible_ratio: ratio(impossible, speeds.len()),
        step_p95: percentile(&sorted_steps, 0.95),
        step_max: sorted_steps.last().copied().unwrap_or(0.0),
        travel_2s_max,
        jump_2s_max,
        cross_side_ratio: ratio(cross_side, pairs),
        same_side_fast_ratio: ratio(same_side_fast, pairs),
        dir_change_ratio: ratio(dir_changes, dir_pairs),
        side_chord_max,
        side_chord_3_ratio: ratio(side_chord_3_notes, mech.len()),
    }
}

fn repeat_stats(mech: &[NoteRow]) -> RepeatStats {
    let mut steps: Vec<(f64, f64)> = Vec::new();
    let mut same_interval = 0;
    for pair in mech.windows(2) {
        let dt = pair[1].t - pair[0].t;
        if dt <= 0.0005 {
            continue;
        }
        let dx = pair[1].ax - pair[0].ax;
        if steps.last().is_some_and(|previous| (dt - previous.0).abs() <= SAME_DT_TOL) {
            same_interval += 1;
        }
        steps.push((dt, dx));
    }
    let tokens: Vec<_> = steps
        .iter()
        .map(|(dt, dx)| format!("{}:{}", js_round(dt / DT_BUCKET).min(40), js_round(dx / DX_BUCKET).clamp(-40, 40)))
        .collect();
    let mut seen2 = HashSet::new();
    let mut repeated2 = 0;
    let mut same_shape = 0;
    for i in 1..steps.len() {
        let key = format!("{}|{}", tokens[i - 1], tokens[i]);
        if !seen2.insert(key) {
            repeated2 += 1;
        }
        if (steps[i].0 - steps[i - 1].0).abs() <= SAME_DT_TOL && (steps[i].1 - steps[i - 1].1).abs() <= SAME_DX_TOL {
            same_shape += 1;
        }
    }
    let mut seen3 = HashSet::new();
    let mut repeated3 = 0;
    for i in 2..tokens.len() {
        if !seen3.insert(format!("{}|{}|{}", tokens[i - 2], tokens[i - 1], tokens[i])) {
            repeated3 += 1;
        }
    }
    let mut period2_max = 0;
    let mut run = 0;
    for i in 2..tokens.len() {
        if tokens[i] == tokens[i - 2] {
            run += 1;
            period2_max = period2_max.max(run + 2);
        } else {
            run = 0;
        }
    }
    let bigrams = tokens.len().saturating_sub(1);
    RepeatStats {
        bigram_repeat_ratio: ratio(repeated2, bigrams),
        trigram_repeat_ratio: ratio(repeated3, tokens.len().saturating_sub(2)),
        distinct_shape_ratio: ratio(seen2.len(), bigrams),
        same_interval_ratio: ratio(same_interval, steps.len().saturating_sub(1)),
        same_shape_ratio: ratio(same_shape, bigrams),
        period_2_run_max: period2_max,
    }
}

fn jack_stats(mech: &[NoteRow]) -> JackStats {
    let mut stats = JackStats {
        run_max: 0,
        covered: 0,
        pairs: 0,
        tight_pairs: 0,
    };
    let mut run = 1;
    for pair in mech.windows(2) {
        let dt = pair[1].t - pair[0].t;
        let dx = (pair[1].ax - pair[0].ax).abs();
        if dt > 0.0005 && dt <= JACK_DT && dx <= JACK_DX {
            stats.pairs += 1;
            if dt <= WINDOW_GOOD {
                stats.tight_pairs += 1;
            }
            run += 1;
            stats.run_max = stats.run_max.max(run);
            stats.covered += if run == 2 { 2 } else { 1 };
        } else {
            run = 1;
        }
    }
    stats
}

fn trill_run_max(mech: &[NoteRow]) -> usize {
    let mut max_run = 0;
    let mut run = 0;
    let mut prev_sign = 0;
    for pair in mech.windows(2) {
        let dt = pair[1].t - pair[0].t;
        let dx = pair[1].ax - pair[0].ax;
        if dt <= 0.0005 || dt > JACK_DT || dx.abs() < TRILL_DX {
            run = 0;
            prev_sign = 0;
            continue;
        }
        let sign = if dx > 0.0 { 1 } else { -1 };
        run = if prev_sign != 0 && sign != prev_sign { run + 1 } else { 2 };
        max_run = max_run.max(run);
        prev_sign = sign;
    }
    max_run
}

fn chord_groups(notes: &[NoteRow]) -> Vec<usize> {
    let mut groups = Vec::new();
    let mut start = 0;
    for i in 1..=notes.len() {
        if i < notes.len() && notes[i].t - notes[start].t <= CHORD_DT {
            continue;
        }
        groups.push(i - start);
        start = i;
    }
    groups
}

fn gaps(notes: &[NoteRow]) -> Vec<f64> {
    notes.windows(2).map(|pair| pair[1].t - pair[0].t).filter(|&gap| gap > 0.0005).collect()
}

fn chain_runs(times: &[f64], threshold: f64, min_run: usize) -> (usize, usize) {
    let (mut longest, mut covered, mut run_start) = (0, 0, 0);
    for i in 1..=times.len() {
        let broken = i == times.len() || times[i] - times[i - 1] > threshold;
        if !broken {
            continue;
        }
        let length = i - run_start;
        if length >= min_run {
            longest = longest.max(length);
            covered += length;
        }
        run_start = i;
    }
    (longest, covered)
}

fn max_notes_in_window(times: &[f64], window: f64) -> usize {
    let mut best = 0;
    let mut start = 0;
    for end in 0..times.len() {
        while times[end] - times[start] > window {
            start += 1;
        }
        best = best.max(end - start + 1);
    }
    best
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let position = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let low = position.floor() as usize;
    let high = position.ceil() as usize;
    if low == high {
        sorted[low]
    } else {
        sorted[low] + (sorted[high] - sorted[low]) * (position - low as f64)
    }
}

fn upper_bound(sorted: &[f64], value: f64) -> usize {
    let mut low = 0;
    let mut high = sorted.len();
    while low < high {
        let mid = (low + high) / 2;
        if sorted[mid] <= value {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn side(x: f64) -> usize {
    usize::from(x >= HAND_SPLIT)
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator > 0 {
        numerator as f64 / denominator as f64
    } else {
        0.0
    }
}

fn round6(value: f64) -> f64 {
    (finite_or_zero(value) * 1_000_000.0).round() / 1_000_000.0
}

fn js_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

fn round4(value: f64) -> f64 {
    (finite_or_zero(value) * 10_000.0).round() / 10_000.0
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prpr::{
        core::{BpmList, ChartExtra, ChartSettings, Color, CtrlObject, HitSound, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, Object},
        judge::JudgeStatus,
    };
    use std::{cell::RefCell, collections::HashMap};

    fn synthetic_note(t: f64, end: f64, kind: Kind, ax: f64) -> NoteRow {
        NoteRow {
            t,
            end,
            kind,
            ax,
            width: 1.0,
            line: 0,
            raw_t: t,
            raw_end: end,
            raw_x: ax,
        }
    }

    #[test]
    fn model_order_and_synthetic_feature_output_are_stable() {
        assert_eq!(BASE_FEATURE_NAMES.len(), 114);
        assert_eq!(BASE_FEATURE_NAMES[0], "g1_npsAvg");
        assert_eq!(BASE_FEATURE_NAMES[77], "g10_period2RunMax");
        assert_eq!(BASE_FEATURE_NAMES[78], "seg_npsMean");
        assert_eq!(BASE_FEATURE_NAMES[113], "seg_tailHeadRatio");

        let notes = vec![
            synthetic_note(0.0, 0.0, Kind::Click, -0.5),
            synthetic_note(0.5, 0.5, Kind::Click, 0.5),
            synthetic_note(1.0, 2.0, Kind::Hold, 0.5),
            synthetic_note(1.5, 1.5, Kind::Flick, 0.0),
        ];
        let mech: Vec<_> = notes.iter().filter(|note| note.is_mech()).cloned().collect();
        let times: Vec<_> = notes.iter().map(|note| note.t).collect();
        let mech_times: Vec<_> = mech.iter().map(|note| note.t).collect();
        let (features, _) = compute_features(
            &notes,
            &mech,
            &mech_times,
            &times,
            2.0,
            NoteCounts {
                click: 2,
                hold: 1,
                flick: 1,
                drag: 0,
                fake: 0,
            },
            1,
            0,
            &LineMotion::default(),
            0.0,
        );
        assert_eq!(round4(features["g1_npsAvg"]), 2.0);
        assert_eq!(round4(features["g1_npsMechAvg"]), 1.5);
        assert_eq!(round4(features["g6_holdNoteRatio"]), 0.25);
        assert_eq!(round4(features["g7_flickRatio"]), 0.25);
        assert_eq!(BASE_FEATURE_NAMES.iter().filter(|name| !features.contains_key(**name)).count(), 0);
        let ordered = BASE_FEATURE_NAMES.map(|name| round4(features[name]));
        assert_eq!(ordered.len(), BASE_FEATURE_COUNT);
    }

    #[test]
    fn synthetic_local_coordinate_and_arbitrary_time_motion_match_source_engine() {
        let x = AnimFloat::new(vec![Keyframe::new(1.0, 0.0, 2), Keyframe::new(3.0, 0.2, 2)]);
        let y = AnimFloat::fixed(0.0);
        let rotation = AnimFloat::new(vec![Keyframe::new(1.0, 0.0, 2), Keyframe::new(3.0, -90.0, 2)]);
        let lines = [LineSampler {
            x: &x,
            y: &y,
            rotation: &rotation,
        }];

        // Before the first event, chart.js holds the first event's start value.
        let start_pose = line_pose_at(&lines, 0, 0.0, 16.0 / 9.0);
        assert_eq!(start_pose, (0.0, 0.0, 0.0));
        // At the midpoint, moveX is 0.1 and the line has rotated by -45 degrees.
        let middle = line_pose_at(&lines, 0, 2.0, 16.0 / 9.0);
        let ax = middle.0 + 0.5 * middle.2.cos();
        assert!((middle.0 - 0.1).abs() < 1e-6);
        assert!((ax - (0.1 + std::f64::consts::FRAC_1_SQRT_2 * 0.5)).abs() < 1e-6);
        // Seeking backward after a later query remains deterministic.
        let end = line_pose_at(&lines, 0, 4.0, 16.0 / 9.0);
        assert!((end.0 - 0.2).abs() < 1e-6);
        assert!((end.2 + std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(has_event_track(&x));
        assert!(!has_event_track(&y));
    }

    #[test]
    fn augmented_values_and_all_ten_feature_groups_are_emitted() {
        let values = [0.0; BASE_FEATURE_COUNT];
        let groups = build_groups(&values);
        assert_eq!(AUGMENTED_FEATURE_NAMES.len(), 7);
        assert_eq!(groups.len(), 10);
        assert_eq!(groups[0].key, "density");
        assert_eq!(groups[5].key, "reach");
        assert!(part_group_index("g5_handTravelP95") == Some(5));
        assert!(part_group_index("seg_patternRepeatMax") == Some(9));
    }

    #[test]
    fn sparse_long_chart_segment_work_scales_with_notes_not_duration() {
        let notes = vec![synthetic_note(1.0, 1.0, Kind::Click, 0.0)];
        let (segments, total, final_end) = build_segments(&notes, 1_000_000_000.0);
        assert_eq!(segments.len(), 1);
        assert!(total > 100_000_000);
        assert_eq!(final_end, 1_000_000_000.0);
        let summary = summarize_segments(&segments, total, final_end);
        assert_eq!(summary["seg_activeRatio"], 1.0 / total as f64);
        assert_eq!(summary["seg_peakPosition"], 0.0);
    }

    #[test]
    fn json_payload_uses_a_numeric_ordered_array_and_top_level_metadata() {
        let feature_values = std::array::from_fn(|index| index as f64);
        let payload = PpsrFeaturePayload {
            version: FEATURE_VERSION,
            feature_names: &BASE_FEATURE_NAMES,
            features: feature_values,
            metadata: PpsrMetadata {
                duration: 12.5,
                duration_source: DurationSource::LastNonFakeNoteEndFallback,
                note_total: 4,
                mech_total: 3,
                fake_total: 2,
                counts: NoteCounts {
                    click: 2,
                    hold: 1,
                    flick: 1,
                    drag: 0,
                    fake: 2,
                },
                line_count: 2,
                moving_lines: 1,
                segment_count: 1,
                groups: build_groups(&feature_values),
                limitations: Vec::new(),
            },
        };
        let json = payload.to_json();
        let values = json["features"].as_array().unwrap();
        assert_eq!(values.len(), 114);
        assert_eq!(values[0].as_f64(), Some(0.0));
        assert_eq!(values[113].as_f64(), Some(113.0));
        assert_eq!(json["note_total"], 4);
        assert_eq!(json["mech_total"], 3);
        assert_eq!(json["duration"], 12.5);
        assert_eq!(json["fake_ratio"], 0.5);
    }

    #[test]
    fn public_extract_accepts_an_immutable_chart_and_returns_plugin_shape() {
        let mut notes = vec![test_note(0.0, -0.5, false), test_note(0.5, 0.5, false), test_note(2.0, 0.0, true)];
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
        let chart = Chart::new(0.0, vec![line], BpmList::new(vec![(0.0, 120.0)]), ChartSettings::default(), ChartExtra::default(), HashMap::new());
        let info = ChartInfo::default();
        let payload = extract(&chart, &info).unwrap();
        assert_eq!(payload["features"].as_array().unwrap().len(), 114);
        assert!(payload["features"].as_array().unwrap().iter().all(|value| value.is_number()));
        assert_eq!(payload["note_total"], 2);
        assert_eq!(payload["mech_total"], 2);
        assert_eq!(payload["duration"], 0.5);
        assert_eq!(payload["fake_ratio"], 0.5);
        assert_eq!(payload["metadata"]["fake_total"], 1);
    }

    fn test_note(time: f64, x: f32, fake: bool) -> Note {
        let kind = NoteKind::Click;
        Note {
            object: Object {
                translation: prpr::core::AnimVector(AnimFloat::fixed(x), AnimFloat::default()),
                ..Default::default()
            },
            kind: kind.clone(),
            hitsound: HitSound::default_from_kind(&kind),
            time,
            height: 0.0,
            speed: 1.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            fx_color: None,
            judge_area: 1.0,
            above: true,
            multiple_hint: false,
            fake,
            judge: JudgeStatus::NotJudged,
        }
    }
}
