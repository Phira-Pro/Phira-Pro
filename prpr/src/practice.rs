//! Practice-only state. None of these controls changes a saved judgement preset
//! or the chart on disk.
pub mod adaptive;
pub mod audio;
pub mod base;
pub mod panel;
pub mod tools;
pub fn format_time(time: f64) -> String {
    let t = time.abs();
    let seconds = t % 60.;
    let minutes = (t / 60.) as u64;
    format!("{}{:02}:{:02}:{:05.2}", if time < 0. { "-" } else { "" }, minutes / 60, minutes % 60, seconds)
}
use crate::{
    config::Config,
    core::{Chart, NoteKind},
    judge::HitSound,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct View {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}
impl Default for View {
    fn default() -> Self {
        Self { scale: 1., x: 0., y: 0. }
    }
}
impl View {
    pub fn screen_to_chart(self, p: macroquad::prelude::Vec2) -> macroquad::prelude::Vec2 {
        p / self.scale + macroquad::prelude::vec2(self.x, self.y)
    }
    pub fn chart_to_screen(self, p: macroquad::prelude::Vec2) -> macroquad::prelude::Vec2 {
        (p - macroquad::prelude::vec2(self.x, self.y)) * self.scale
    }
    pub fn active(self) -> bool {
        self != Self::default()
    }
    pub fn visible_frame(self, aspect: f32) -> macroquad::prelude::Rect {
        use macroquad::prelude::*;
        let a = self.chart_to_screen(vec2(-1., -1. / aspect));
        Rect::new(a.x, a.y, 2. * self.scale, 2. * self.scale / aspect)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoteBehavior {
    #[default]
    Original,
    Tap,
    Drag,
    Flick,
}
impl NoteBehavior {
    pub fn next(self) -> Self {
        match self {
            Self::Original => Self::Tap,
            Self::Tap => Self::Drag,
            Self::Drag => Self::Flick,
            Self::Flick => Self::Original,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    pub preserve_pitch: bool,
    pub adaptive: adaptive::Goal,
    pub judgement_multiplier: f32,
    pub inverse_speed: bool,
    pub correct_sound: bool,
    pub view: View,
    pub behavior: NoteBehavior,
    pub ranges: bool,
    pub range_kinds: [bool; 4],
    pub range_alpha: f32,
    pub range_horizon: f32,
    pub range_anchor: RangeAnchor,
    /// Perfect / Good / Bad / Miss; extended scoring bands follow Good.
    pub range_tap: [bool; 4],
    /// Head Perfect / head Good / head Miss / tail.
    pub range_hold: [bool; 4],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeAnchor {
    #[default]
    JudgeLine,
    Note,
    Both,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            preserve_pitch: false,
            adaptive: Default::default(),
            judgement_multiplier: 1.,
            inverse_speed: false,
            correct_sound: false,
            view: View::default(),
            behavior: NoteBehavior::Original,
            ranges: false,
            range_kinds: [true; 4],
            range_alpha: 0.12,
            range_horizon: 2.,
            range_anchor: RangeAnchor::JudgeLine,
            range_tap: [true; 4],
            range_hold: [true; 4],
        }
    }
}
impl Options {
    pub fn sanitize(&mut self) {
        fn bound(v: &mut f32, low: f32, high: f32, default: f32) {
            *v = if v.is_finite() { v.clamp(low, high) } else { default };
        }
        bound(&mut self.adaptive.accuracy, 0., 1., 0.98);
        self.adaptive.consecutive = self.adaptive.consecutive.clamp(1, 99);
        bound(&mut self.judgement_multiplier, 0.1, 3., 1.);
        bound(&mut self.view.scale, 0.1, 5., 1.);
        bound(&mut self.view.x, -4., 4., 0.);
        bound(&mut self.view.y, -4., 4., 0.);
        bound(&mut self.range_alpha, 0.02, 0.6, 0.12);
        bound(&mut self.range_horizon, 0.1, 10., 2.);
    }
    pub fn multiplier(&self, speed: f32) -> f64 {
        if self.inverse_speed {
            1. / speed.max(0.1) as f64
        } else {
            self.judgement_multiplier as f64
        }
    }
}

/// Backups follow note identity, even when the render cache sorts the notes.
pub struct OriginalNotes(pub Vec<Vec<(NoteKind, HitSound)>>);
impl OriginalNotes {
    pub fn capture(chart: &Chart) -> Self {
        Self(
            chart
                .lines
                .iter()
                .map(|l| l.notes.iter().map(|n| (n.kind.clone(), n.hitsound.clone())).collect())
                .collect(),
        )
    }
    pub fn apply(&mut self, chart: &mut Chart, behavior: NoteBehavior) {
        for (line, originals) in chart.lines.iter_mut().zip(&mut self.0) {
            for (note, (kind, sound)) in line.notes.iter_mut().zip(originals.iter()) {
                note.kind = match behavior {
                    NoteBehavior::Original => kind.clone(),
                    NoteBehavior::Tap => NoteKind::Click,
                    NoteBehavior::Drag => NoteKind::Drag,
                    NoteBehavior::Flick => NoteKind::Flick,
                };
                note.hitsound = if behavior == NoteBehavior::Original {
                    sound.clone()
                } else {
                    HitSound::default_from_kind(&note.kind)
                };
            }
            line.cache = crate::core::JudgeLineCache::with_companion(&mut line.notes, originals);
        }
        chart.reset();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub combo: Option<(usize, usize)>,
    pub speed: f32,
    pub auto_loop: bool,
    pub ramp: bool,
    pub ramp_start: f32,
    pub ramp_step: f32,
    pub options: Options,
}
impl Bookmark {
    pub fn valid(&self, length: f64, earliest: f64) -> bool {
        self.start.is_finite()
            && self.end.is_finite()
            && self.start >= earliest
            && self.end <= length
            && self.end > self.start + 0.049
            && self.speed.is_finite()
            && (0.1..=2.).contains(&self.speed)
            && self.ramp_start.is_finite()
            && (0.1..=2.).contains(&self.ramp_start)
            && self.ramp_step.is_finite()
            && (0.01..=0.5).contains(&self.ramp_step)
    }
}
type Library = std::collections::BTreeMap<String, Vec<Bookmark>>;
pub fn clamp_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        ((speed.clamp(0.1, 2.) * 100.).round() / 100.).clamp(0.1, 2.)
    } else {
        1.
    }
}

/// The slider, music and chart clock use the same effective practice rate.
pub fn round_speed(config: &Config, target: f32, completed_rounds: u32) -> f32 {
    let target = clamp_speed(target);
    if config.practice_ramp {
        clamp_speed(config.practice_speed_start + config.practice_speed_step * completed_rounds as f32).min(target)
    } else {
        target
    }
}

static STORAGE: OnceLock<PathBuf> = OnceLock::new();
pub fn set_storage(path: PathBuf) {
    let _ = STORAGE.set(path);
}
fn library() -> anyhow::Result<Library> {
    let path = STORAGE.get().ok_or_else(|| anyhow::anyhow!("Practice storage is unavailable"))?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Library::new()),
        Err(e) => Err(e.into()),
    }
}
pub fn bookmarks(key: &str) -> anyhow::Result<Vec<Bookmark>> {
    Ok(library()?.remove(key).unwrap_or_default())
}
pub fn store_bookmarks(key: &str, list: &[Bookmark]) -> anyhow::Result<()> {
    let mut data = library()?;
    data.insert(key.into(), list.to_vec());
    let path = STORAGE.get().unwrap();
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec(&data)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

static PENDING: Mutex<Option<Config>> = Mutex::new(None);
pub fn save_preferences(config: &Config) {
    *PENDING.lock().unwrap() = Some(config.clone());
}
pub fn take_preferences(config: &mut Config) -> bool {
    let Some(saved) = PENDING.lock().unwrap().take() else { return false };
    config.practice = saved.practice;
    config.practice_ramp = saved.practice_ramp;
    config.practice_speed_start = saved.practice_speed_start;
    config.practice_speed_step = saved.practice_speed_step;
    config.auto_retry = saved.auto_retry;
    config.retry_lead = saved.retry_lead;
    true
}

#[derive(Clone, Debug)]
pub struct Round {
    pub speed: f32,
    pub accuracy: f64,
    pub counts: [u32; 8],
    pub score: u32,
}
#[derive(Default)]
pub struct Statistics {
    pub rounds: Vec<Round>,
    pub total: u32,
    pub best: Option<Round>,
}
impl Statistics {
    pub fn push(&mut self, round: Round) {
        self.total += 1;
        if self
            .best
            .as_ref()
            .is_none_or(|b| round.accuracy > b.accuracy || (round.accuracy == b.accuracy && round.score > b.score))
        {
            self.best = Some(round.clone());
        }
        if self.rounds.len() == 100 {
            self.rounds.remove(0);
        }
        self.rounds.push(round);
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
pub fn combo_range(notes: &[(f64, f64)], first: usize, last: usize, offset: f64, length: f64) -> Option<std::ops::Range<f64>> {
    if first == 0 || last < first || last > notes.len() {
        return None;
    }
    let start = notes[first - 1].0;
    let last_time = notes[last - 1].0;
    let end = notes
        .iter()
        .filter(|n| n.0 >= start && n.0 <= last_time)
        .map(|n| n.1)
        .fold(last_time, f64::max);
    let range = (start + offset - 0.25).max(offset.min(0.))..(end + offset + 0.25).min(length);
    (range.start.is_finite() && range.end.is_finite() && range.end > range.start).then_some(range)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn practice_round_rates_use_centisteps_and_cap_at_target() {
        let mut config = Config::default();
        config.practice_ramp = true;
        config.practice_speed_start = 0.7;
        config.practice_speed_step = 0.03;
        for (round, expected) in [(0, 0.7), (1, 0.73), (9, 0.97), (10, 1.), (100, 1.)] {
            assert_eq!(round_speed(&config, 1., round), expected);
        }
        assert_eq!(round_speed(&config, 0.5, 0), 0.5);
        config.practice_ramp = false;
        assert_eq!(round_speed(&config, 1.25, 100), 1.25);
        assert_eq!(clamp_speed(f32::NAN), 1.);
        assert_eq!(clamp_speed(4.), 2.);
        assert_eq!(clamp_speed(0.01), 0.1);
    }

    #[test]
    fn practice_preferences_never_write_effective_speed_or_mods_back_to_global() {
        let mut global = Config::default();
        global.speed = 1.23;
        global.mods.insert(crate::config::Mods::NIGHTCORE);
        let mut session = global.clone();
        session.speed = 0.61;
        session.mods.remove(crate::config::Mods::NIGHTCORE);
        session.practice_ramp = true;
        save_preferences(&session);
        assert!(take_preferences(&mut global));
        assert_eq!(global.speed, 1.23);
        assert!(global.mods.contains(crate::config::Mods::NIGHTCORE));
        assert!(global.practice_ramp);
    }
    #[test]
    fn saved_ranges_accept_old_options_and_preserve_new_controls() {
        let mut old: Options = serde_json::from_value(serde_json::json!({
            "ranges": true, "rangeAlpha": 0.22, "rangeHorizon": 3.,
            "rangeKinds": [true, false, true, false]
        }))
        .unwrap();
        assert_eq!(old.range_anchor, RangeAnchor::JudgeLine);
        assert_eq!(old.range_tap, [true; 4]);
        assert_eq!(old.range_hold, [true; 4]);
        assert_eq!(old.range_alpha, 0.22, "keep an existing user's opacity");
        assert_eq!(old.range_horizon, 3.);
        old.range_anchor = RangeAnchor::Both;
        old.range_tap[2] = false;
        old.range_hold[3] = false;
        let saved = serde_json::to_value(&old).unwrap();
        let restored: Options = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), saved);
    }
    #[test]
    fn normal_view_frame_tracks_reveal_zoom_pan_and_aspect() {
        use macroquad::prelude::*;
        for aspect in [4. / 3., 16. / 9., 21. / 9.] {
            for scale in [0.1, 0.5, 0.99] {
                let view = View { scale, x: 0.3, y: -0.15 };
                let r = view.visible_frame(aspect);
                assert!((r.w - 2. * scale).abs() < 1e-6);
                assert!((r.h - 2. * scale / aspect).abs() < 1e-6);
                for (point, expected) in [
                    (vec2(r.x, r.y), vec2(-1., -1. / aspect)),
                    (vec2(r.right(), r.bottom()), vec2(1., 1. / aspect)),
                ] {
                    assert!((view.screen_to_chart(point) - expected).length() < 1e-5);
                }
            }
        }
    }
    #[test]
    fn view_roundtrips_across_zoom_pan_and_aspect() {
        use macroquad::prelude::*;
        for aspect in [4. / 3., 16. / 9., 21. / 9.] {
            for scale in [0.1, 0.5, 1., 2., 5.] {
                let view = View { scale, x: 0.37, y: -0.2 };
                for p in [vec2(-1., -1. / aspect), Vec2::ZERO, vec2(1., 1. / aspect)] {
                    assert!((view.screen_to_chart(view.chart_to_screen(p)) - p).length() < 1e-5);
                }
            }
        }
    }
    #[test]
    fn practice_multiplier_is_scoped_and_preserves_sided_presets() {
        for algorithm in [crate::config::JudgeAlgorithm::PhiraPro, crate::config::JudgeAlgorithm::Phigros] {
            for strict in [false, true] {
                let mut config = Config::default();
                config.judge_algorithm = algorithm;
                config.mods.set(crate::config::Mods::STRICT_JUDGE, strict);
                config.judge_grading.detailed = true;
                config.judge_timing.early_ms = Some([12., 60., 140., 220.]);
                config.judge_timing.late_ms = Some([18., 90., 170., 250.]);
                let before = serde_json::to_value(&config).unwrap();
                let normal = config.judge_windows();
                config.practice_active = true;
                config.practice_multiplier = 1.7;
                let practice = config.judge_windows();
                for i in 0..4 {
                    assert!((normal.early[i] * 1.7 - practice.early[i]).abs() < 1e-9);
                    assert!((normal.late[i] * 1.7 - practice.late[i]).abs() < 1e-9);
                }
                for i in 0..3 {
                    assert!((normal.extended_early[i] * 1.7 - practice.extended_early[i]).abs() < 1e-9);
                }
                assert_eq!(serde_json::to_value(&config).unwrap(), before, "runtime multiplier must never overwrite saved preset windows");
                config.practice_active = false;
                assert_eq!(config.judge_windows(), normal);
            }
        }
        let p = Options {
            inverse_speed: true,
            ..Default::default()
        };
        assert_eq!(p.multiplier(0.5), 2.);
    }
    #[test]
    fn bookmarks_roundtrip_and_round_history_remains_bounded() {
        let dir = tempfile::tempdir().unwrap();
        set_storage(dir.path().join("bookmarks.json"));
        let b = Bookmark {
            name: "分段 · Hold".into(),
            start: 1.75,
            end: 5.25,
            combo: Some((2, 3)),
            speed: 0.75,
            auto_loop: true,
            ramp: true,
            ramp_start: 0.5,
            ramp_step: 0.02,
            options: Options {
                preserve_pitch: true,
                behavior: NoteBehavior::Drag,
                ..Default::default()
            },
        };
        assert!(b.valid(6., 0.));
        store_bookmarks("chart-a", &[b]).unwrap();
        assert!(bookmarks("chart-b").unwrap().is_empty());
        let saved = bookmarks("chart-a").unwrap();
        assert_eq!(saved[0].name, "分段 · Hold");
        assert_eq!(saved[0].combo, Some((2, 3)));
        assert!(saved[0].options.preserve_pitch);
        assert_eq!(saved[0].options.behavior, NoteBehavior::Drag);
        assert!(!saved[0].valid(4., 0.));
        let mut stats = Statistics::default();
        for i in 0..105 {
            stats.push(Round {
                speed: 0.5,
                accuracy: i as f64 / 105.,
                counts: [i; 8],
                score: i,
            });
        }
        assert_eq!(stats.total, 105);
        assert_eq!(stats.rounds.len(), 100);
        assert_eq!(stats.best.as_ref().unwrap().score, 104);
        stats.clear();
        assert_eq!(stats.total, 0);
        assert!(stats.best.is_none());
        std::fs::write(STORAGE.get().unwrap(), "broken").unwrap();
        assert!(store_bookmarks("chart-a", &[]).is_err(), "corrupt library must be reported, never silently overwritten");
    }
    #[test]
    fn combo_bounds_include_chords_and_hold_tails() {
        let notes = [(1., 1.), (2., 5.), (2., 2.), (6., 6.)];
        assert_eq!(combo_range(&notes, 2, 3, 0.1, 7.), Some(1.85..5.35));
        assert!(combo_range(&notes, 0, 2, 0., 7.).is_none());
        assert!(combo_range(&notes, 2, 5, 0., 7.).is_none());
    }
}

/// Editing the end marker keeps the paused cursor inside the updated segment.
pub fn boundary_position(range: &std::ops::Range<f64>, current: f64) -> f64 {
    if current.is_finite() {
        current.clamp(range.start, range.end)
    } else {
        range.start
    }
}

#[cfg(test)]
mod boundary_tests {
    #[test]
    fn end_edit_preserves_cursor_and_clamps_only_when_outside() {
        for (range, current, expected) in [
            (10. ..40., 25., 25.),
            (10. ..20., 25., 20.),
            (10. ..40., 5., 10.),
            (10. ..40., 40., 40.),
            (-2. ..8., -1., -1.),
            (10. ..40., f64::NAN, 10.),
        ] {
            assert_eq!(super::boundary_position(&range, current), expected);
        }
    }
}
