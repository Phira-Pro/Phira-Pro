//! Judgement system

use crate::{
    config::{Config, JudgeWindows, Mods},
    core::{BadNote, Chart, NoteKind, Point, Resource, Vector, NOTE_WIDTH_RATIO_BASE},
    ext::{get_viewport, NotNanExt},
};
use macroquad::prelude::{
    utils::{register_input_subscriber, repeat_all_miniquad_input},
    *,
};
use miniquad::{EventHandler, MouseButton};
use once_cell::sync::Lazy;
use sasa::{PlaySfxParams, Sfx};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    mem,
    num::FpCategory,
};

pub const FLICK_SPEED_THRESHOLD: f32 = 0.8;
/// Perfect+（大 P）的默认窗口。比 Perfect 更严格，且不参与准确率/分数计算。
pub const LIMIT_PERFECT_PLUS: f64 = 0.016;
pub const LIMIT_PERFECT: f64 = 0.08;
pub const LIMIT_GOOD: f64 = 0.16;
pub const LIMIT_BAD: f64 = 0.22;
pub const UP_TOLERANCE: f64 = 0.05;
pub const DIST_FACTOR: f64 = 0.2;
/// 结算判定分布图的桶数。
pub const HIST_BUCKETS: usize = 21;
/// 结算判定分布图的半宽（毫秒）：横轴覆盖 -HIST_MAX_MS .. +HIST_MAX_MS。
pub const HIST_MAX_MS: f64 = 200.;

#[derive(Debug, Clone)]
pub enum HitSound {
    None,
    Click,
    Flick,
    Drag,
    Custom(String),
}

impl HitSound {
    pub fn play(&self, res: &mut Resource) {
        if res.config.has_mod(Mods::PERFECT_SOUND) {
            return;
        }
        self.play_chart_cue(res);
    }

    fn play_chart_cue(&self, res: &mut Resource) {
        match self {
            HitSound::None => {}
            HitSound::Click => play_sfx(&mut res.sfx_click, &res.config),
            HitSound::Flick => play_sfx(&mut res.sfx_flick, &res.config),
            HitSound::Drag => play_sfx(&mut res.sfx_drag, &res.config),
            HitSound::Custom(s) => {
                if let Some(sfx) = res.extra_sfxs.get_mut(s) {
                    play_sfx(sfx, &res.config);
                }
            }
        }
    }

    pub fn default_from_kind(kind: &NoteKind) -> Self {
        match kind {
            NoteKind::Click => HitSound::Click,
            NoteKind::Flick => HitSound::Flick,
            NoteKind::Drag => HitSound::Drag,
            NoteKind::Hold { .. } => HitSound::Click,
        }
    }
}

pub fn play_sfx(sfx: &mut Sfx, config: &Config) {
    if config.volume_sfx <= 1e-2 {
        return;
    }
    let _ = sfx.play(PlaySfxParams {
        amplifier: config.volume_sfx,
    });
}

#[cfg(all(not(target_os = "windows"), not(target_os = "ios")))]
fn get_uptime() -> f64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    assert!(ret == 0);
    time.tv_sec as f64 + time.tv_nsec as f64 * 1e-9
}

#[cfg(target_os = "ios")]
fn get_uptime() -> f64 {
    objc2_foundation::NSProcessInfo::processInfo().systemUptime()
}

#[cfg(target_os = "windows")]
fn get_uptime() -> f64 {
    miniquad::native::windows::get_uptime()
}

pub struct FlickTracker {
    threshold: f32,
    last_point: Point,
    last_delta: Option<Vector>,
    last_time: f32,
    flicked: bool,
    stopped: bool,
}

impl FlickTracker {
    pub fn new(_dpi: u32, time: f32, point: Point) -> Self {
        // TODO maybe a better approach?
        let dpi = 275;
        Self {
            threshold: FLICK_SPEED_THRESHOLD * dpi as f32 / 386.,
            last_point: point,
            last_delta: None,
            last_time: time,
            flicked: false,
            stopped: true,
        }
    }

    pub fn push(&mut self, time: f32, position: Point) {
        let delta = position - self.last_point;
        self.last_point = position;
        let dt = time - self.last_time;
        self.last_time = time;
        if dt <= 0. || !dt.is_finite() {
            return;
        }
        if delta.norm_squared() <= f32::EPSILON * f32::EPSILON {
            self.stopped = true;
            return;
        }
        if let Some(last_delta) = &self.last_delta {
            let speed = delta.dot(last_delta) / dt;
            if speed < self.threshold {
                self.stopped = true;
            }
            if self.stopped && !self.flicked {
                self.flicked = delta.magnitude() / dt >= self.threshold * 2.;
            }
            // if speed < self.threshold || self.stopped {
            // self.stopped = delta.magnitude() / dt < self.threshold * 5.;
            // self.flicked = self.threshold <= speed;
            // if self.flicked {
            // warn!("new flick!");
            // }
            // }
        }
        self.last_delta = Some(delta.normalize());
    }
}

#[derive(Debug)]
pub enum JudgeStatus {
    NotJudged,
    PreJudge,
    Judged,
    Hold(bool, f64, f64, bool, f64), // perfect, at, diff, pre-judge, up-time
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, Serialize)]
pub enum Judgement {
    Perfect,
    Good,
    Bad,
    Miss,
    /// Perfect+（大 P）：比 Perfect 更严格，但只作为 Perfect 的子分类，
    /// 不改变准确率与分数口径。追加在末尾以保持既有下标语义（0..=3）不变。
    PerfectPlus,
}

/// 局内 early/late 判定条保留的最近命中数。
pub const MAX_RECENT_HITS: usize = 24;

/// 把一个时间偏移（秒，负=偏早）按判定窗口折算成判定等级；超出 bad 窗即 Miss。
///
/// 尾判（mania 风格的松手判定）也走这套口径。
pub fn judgement_of_offset(off: f64, limits: &JudgeWindows) -> Judgement {
    let d = off.abs();
    if d <= limits.perfect_plus {
        Judgement::PerfectPlus
    } else if d <= limits.perfect {
        Judgement::Perfect
    } else if d <= limits.good {
        Judgement::Good
    } else if d <= limits.bad {
        Judgement::Bad
    } else {
        Judgement::Miss
    }
}

/// Positive offsets are late. Apply optional leniency only on that side,
/// leaving the raw offset available for timing statistics.
fn judge_distance(offset: f64, late_leniency: f64) -> f64 {
    if offset > 0. {
        (offset - late_leniency).max(0.)
    } else {
        -offset
    }
}

fn finger_blocked(infected: &mut HashSet<u64>, id: u64, phase: TouchPhase, inside: bool) -> bool {
    if matches!(phase, TouchPhase::Started | TouchPhase::Ended | TouchPhase::Cancelled) {
        infected.remove(&id);
    }
    if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
        return false;
    }
    if inside {
        infected.insert(id);
    }
    infected.contains(&id)
}

/// 一次命中的真实时间偏移，供局内 early/late 判定条绘制。
#[derive(Debug, Clone, Copy)]
pub struct RecentHit {
    /// 相对音符时间的时间差（秒）。负 = 偏早，正 = 偏晚。
    pub offset: f64,
    pub judgement: Judgement,
    /// 记录时的游戏时间，用于淡出。
    pub time: f64,
}

#[cfg(not(closed))]
#[derive(Default)]
pub(crate) struct JudgeInner {
    diffs: Vec<f64>,

    combo: u32,
    max_combo: u32,
    counts: [u32; 5],
    num_of_notes: u32,
    early_kind: [u32; 5],
    late_kind: [u32; 5],
    recent: Vec<RecentHit>,
    /// 本局所有有效命中的偏移（秒），供结算时的偏差统计使用。
    offsets: Vec<f64>,
    /// 判定时间误差分布（-HIST_MAX_MS .. +HIST_MAX_MS，共 HIST_BUCKETS 个桶）。
    hist: [u32; HIST_BUCKETS],
    /// 血条模式的当前血量（0..=1）。
    hp: f32,
    /// 血条扣血倍率。
    hp_amount: f32,
    /// 血条整体倍率（回血与扣血同乘）。
    hp_scale: f32,
}

#[cfg(not(closed))]
impl JudgeInner {
    pub fn new(num_of_notes: u32) -> Self {
        Self {
            diffs: Vec::new(),

            combo: 0,
            max_combo: 0,
            counts: [0; 5],
            num_of_notes,
            early_kind: [0; 5],
            late_kind: [0; 5],
            recent: Vec::new(),
            offsets: Vec::new(),
            hist: [0; HIST_BUCKETS],
            hp: 1.,
            hp_amount: 1.,
            hp_scale: 1.,
        }
    }

    pub fn set_hp_amount(&mut self, amount: f32) {
        self.hp_amount = amount.max(0.);
    }

    pub fn set_hp_scale(&mut self, scale: f32) {
        self.hp_scale = if scale.is_finite() { scale.max(0.) } else { 1. };
    }

    pub fn hp(&self) -> f32 {
        self.hp
    }

    /// 记录一次命中的真实偏移。Miss 与拖拽/滑动音符没有有意义的偏移，不记录。
    pub fn push_recent(&mut self, offset: f64, judgement: Judgement, time: f64) {
        if !offset.is_finite() {
            return;
        }
        // 累计本局所有有效命中的偏移；Miss 不代表击打精度，不入统计。
        if !matches!(judgement, Judgement::Miss) {
            self.offsets.push(offset);
            // 同一批「真实计时」的命中同时进结算分布图。
            self.push_hist(offset);
        }
        if matches!(judgement, Judgement::Miss) {
            return;
        }
        if self.recent.len() == MAX_RECENT_HITS {
            self.recent.remove(0);
        }
        self.recent.push(RecentHit { offset, judgement, time });
    }

    /// 记录一次真实计时的判定误差（秒），用于结算画面的判定分布图。
    pub fn push_hist(&mut self, diff: f64) {
        let width = HIST_MAX_MS * 2. / HIST_BUCKETS as f64;
        let idx = ((diff * 1000. + HIST_MAX_MS) / width).clamp(0., HIST_BUCKETS as f64 - 1.) as usize;
        self.hist[idx] += 1;
    }

    pub fn recent_hits(&self) -> &[RecentHit] {
        &self.recent
    }

    /// 本局所有有效命中的偏移统计，返回 `(命中数, 平均偏移, 标准差)`，单位：秒。
    /// 没有任何有效命中时返回 `None`。
    pub fn offset_stats(&self) -> Option<(usize, f64, f64)> {
        let n = self.offsets.len();
        if n == 0 {
            return None;
        }
        let mean = self.offsets.iter().sum::<f64>() / n as f64;
        let var = self.offsets.iter().map(|it| (it - mean).powi(2)).sum::<f64>() / n as f64;
        Some((n, mean, var.sqrt()))
    }

    pub fn commit(&mut self, what: Judgement, diff: f64) {
        use Judgement::*;
        if matches!(what, Judgement::Good) {
            self.diffs.push(diff);
        }
        if diff < 0. {
            self.early_kind[what as usize] += 1;
        } else if diff > 0. {
            self.late_kind[what as usize] += 1;
        }
        self.counts[what as usize] += 1;
        // 血条：大 P / Perfect 回血，Good 微增，Bad 小扣，Miss 大扣。
        let base = match what {
            PerfectPlus => 0.02,
            Perfect => 0.01,
            Good => 0.002,
            Bad => -0.06,
            Miss => -0.12,
        };
        // 扣血再乘 hp_amount；回血 / 扣血统一再乘 hp_scale（总体倍率）。
        let delta = base * self.hp_scale * if base < 0. { self.hp_amount } else { 1. };
        self.hp = (self.hp + delta).clamp(0., 1.);
        match what {
            Perfect | PerfectPlus | Good => {
                self.combo += 1;
                if self.combo > self.max_combo {
                    self.max_combo = self.combo;
                }
            }
            _ => {
                self.combo = 0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.combo = 0;
        self.max_combo = 0;
        self.counts = [0; 5];
        self.diffs.clear();
        self.early_kind = [0; 5];
        self.late_kind = [0; 5];
        self.recent.clear();
        self.offsets.clear();
        self.hist = [0; HIST_BUCKETS];
        self.hp = 1.;
    }

    /// Perfect+ 与 Perfect 同权，因此结果与接入 Perfect+ 之前逐位相同。
    fn perfect_count(&self) -> u32 {
        self.counts[0] + self.counts[4]
    }

    pub fn accuracy(&self) -> f64 {
        (self.perfect_count() as f64 + self.counts[1] as f64 * 0.65) / self.num_of_notes as f64
    }

    pub fn real_time_accuracy(&self) -> f64 {
        let cnt = self.counts.iter().sum::<u32>();
        if cnt == 0 {
            return 1.;
        }
        (self.perfect_count() as f64 + self.counts[1] as f64 * 0.65) / cnt as f64
    }

    /// `no_combo_score` 为真时不把最大连击计入分数（分数 = 准确率 × 1,000,000）。
    pub fn score(&self, no_combo_score: bool) -> u32 {
        const TOTAL: u32 = 1000000;
        if self.perfect_count() == self.num_of_notes {
            TOTAL
        } else if no_combo_score {
            (self.accuracy() * TOTAL as f64).round() as u32
        } else {
            let score = (0.9 * self.accuracy() + self.max_combo as f64 / self.num_of_notes as f64 * 0.1) * TOTAL as f64;
            score.round() as u32
        }
    }

    pub fn result(&self, no_combo_score: bool) -> PlayResult {
        let early = self.diffs.iter().filter(|it| **it < 0.).count() as u32;
        // The score protocol measures RMS about the note time, including
        // 250ms for misses and zero for automatic judgements. A centered
        // standard deviation would misleadingly report zero for uniformly
        // late hits, and previously disagreed with the uploaded record.
        let mean = self.offset_stats().map_or(0., |(_, mean, _)| mean as f32);
        let std = timing_mean_square(&self.offsets, self.counts[3], self.num_of_notes).sqrt() as f32;
        PlayResult {
            score: self.score(no_combo_score),
            accuracy: self.accuracy(),
            max_combo: self.max_combo,
            num_of_notes: self.num_of_notes,
            counts: self.counts,
            early,
            late: self.diffs.len() as u32 - early,
            std,
            mean,
            offsets: self.offsets.clone(),
            hist: self.hist,
            early_kind: self.early_kind,
            late_kind: self.late_kind,
        }
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn counts(&self) -> [u32; 5] {
        self.counts
    }
}

/// Timing statistic shared by the result screen and score token (seconds²).
pub fn timing_mean_square(offsets: &[f64], misses: u32, notes: u32) -> f64 {
    if notes == 0 {
        return 0.;
    }
    (offsets.iter().filter(|it| it.is_finite()).map(|it| it * it).sum::<f64>() + misses as f64 * 0.25 * 0.25) / notes as f64
}

#[cfg(test)]
mod tests {
    use super::{judgement_of_offset, JudgeInner, Judgement, MAX_RECENT_HITS};
    use crate::config::JudgeWindows;

    #[test]
    fn finger_infection_survives_field_disappearance_until_release() {
        use macroquad::prelude::TouchPhase::*;
        let mut infected = std::collections::HashSet::new();
        assert!(super::finger_blocked(&mut infected, 7, Started, true));
        assert!(super::finger_blocked(&mut infected, 7, Stationary, false));
        assert!(super::finger_blocked(&mut infected, 7, Moved, false));
        assert!(!super::finger_blocked(&mut infected, 8, Started, false));
        assert!(!super::finger_blocked(&mut infected, 7, Ended, true));
        assert!(!super::finger_blocked(&mut infected, 7, Started, false));
        assert!(super::finger_blocked(&mut infected, 7, Moved, true));
        assert!(!super::finger_blocked(&mut infected, 7, Cancelled, true));
    }

    #[test]
    fn configured_windows_and_late_leniency_keep_exact_boundaries() {
        let mut config = crate::config::Config::default();
        config.lim_perfect_plus_ms = 12.;
        config.lim_perfect_ms = 40.;
        config.lim_good_ms = 90.;
        config.lim_bad_ms = 180.;
        let w = config.judge_windows();
        for (ms, expected) in [
            (12., Judgement::PerfectPlus),
            (40., Judgement::Perfect),
            (90., Judgement::Good),
            (180., Judgement::Bad),
            (181., Judgement::Miss),
        ] {
            for sign in [-1., 1.] {
                assert_eq!(judgement_of_offset(sign * ms / 1000., &w) as u8, expected as u8);
            }
        }
        assert_eq!(super::judge_distance(-0.050, 0.030), 0.050);
        assert!((super::judge_distance(0.050, 0.030) - 0.020).abs() < 1e-12);
        assert_eq!(super::judge_distance(0.020, 0.030), 0.);
    }

    #[test]
    fn stationary_and_duplicate_timestamp_flick_samples_stay_finite() {
        let mut tracker = super::FlickTracker::new(275, 0., super::Point::new(0., 0.));
        tracker.push(0., super::Point::new(0., 0.));
        tracker.push(0.01, super::Point::new(0., 0.));
        tracker.push(0.02, super::Point::new(0.05, 0.));
        tracker.push(0.03, super::Point::new(0.10, 0.));
        assert!(tracker.flicked);
        assert!(tracker.last_delta.unwrap().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn theoretical_result_bonus_does_not_change_saved_score() {
        let result = super::PlayResult {
            score: 1_000_000,
            counts: [0, 0, 0, 0, 2000],
            ..Default::default()
        };
        assert_eq!(result.displayed_score(true), 1_002_000);
        assert_eq!(result.displayed_score(false), 1_000_000);
        assert_eq!(result.score, 1_000_000);
    }

    /// 时间偏移 → 判定等级的分档，早/晚对称，超过 bad 窗即 Miss。尾判走同一口径。
    #[test]
    fn offset_tiering() {
        let limits = JudgeWindows {
            perfect_plus: 0.016,
            perfect: 0.08,
            good: 0.16,
            bad: 0.22,
            fullscreen: false,
        };
        assert!(matches!(judgement_of_offset(0.0, &limits), Judgement::PerfectPlus));
        assert!(matches!(judgement_of_offset(0.016, &limits), Judgement::PerfectPlus));
        assert!(matches!(judgement_of_offset(0.05, &limits), Judgement::Perfect));
        assert!(matches!(judgement_of_offset(-0.05, &limits), Judgement::Perfect));
        assert!(matches!(judgement_of_offset(0.12, &limits), Judgement::Good));
        assert!(matches!(judgement_of_offset(0.20, &limits), Judgement::Bad));
        assert!(matches!(judgement_of_offset(0.30, &limits), Judgement::Miss));
        assert!(matches!(judgement_of_offset(-0.30, &limits), Judgement::Miss));
    }

    /// Perfect+ 与 Perfect 同权：同样的「完美」个数落在哪一档，准确率、分数、连击都应一致。
    #[test]
    fn perfect_plus_weighs_like_perfect() {
        let mut a = JudgeInner::new(4);
        for _ in 0..3 {
            a.commit(Judgement::Perfect, 0.01);
        }
        a.commit(Judgement::Good, 0.1);

        let mut b = JudgeInner::new(4);
        for _ in 0..3 {
            b.commit(Judgement::PerfectPlus, 0.005);
        }
        b.commit(Judgement::Good, 0.1);

        assert_eq!(a.counts(), [3, 1, 0, 0, 0]);
        assert_eq!(b.counts(), [0, 1, 0, 0, 3]);
        assert_eq!(a.accuracy(), b.accuracy());
        assert_eq!(a.score(false), b.score(false));
        assert_eq!(a.combo(), b.combo());
        assert_eq!(a.score(false), 921250);
    }

    /// 全 Perfect+ 与全 Perfect 一样拿满分。
    #[test]
    fn all_perfect_plus_is_full_score() {
        let mut j = JudgeInner::new(10);
        for _ in 0..10 {
            j.commit(Judgement::PerfectPlus, 0.0);
        }
        assert_eq!(j.score(false), 1000000);
        assert!((j.accuracy() - 1.).abs() < 1e-12);
    }

    #[test]
    fn offset_stats_ignores_miss() {
        let mut j = JudgeInner::new(64);
        assert!(j.offset_stats().is_none());
        j.push_recent(0.01, Judgement::Perfect, 1.);
        j.push_recent(0.03, Judgement::Perfect, 2.);
        j.push_recent(0.5, Judgement::Miss, 3.);
        let (n, mean, sd) = j.offset_stats().unwrap();
        assert_eq!(n, 2);
        assert!((mean - 0.02).abs() < 1e-9);
        assert!((sd - 0.01).abs() < 1e-9);
        j.reset();
        assert!(j.offset_stats().is_none());
    }

    #[test]
    fn result_error_matches_the_score_protocol_with_bias_misses_and_auto_notes() {
        let mut j = JudgeInner::new(4);
        for _ in 0..2 {
            j.push_recent(0.02, Judgement::Perfect, 1.);
            j.commit(Judgement::Perfect, 0.02);
        }
        j.commit(Judgement::Perfect, 0.); // Automatic drag/flick: no timing sample.
        j.commit(Judgement::Miss, 0.);
        assert_eq!(j.offset_stats().unwrap().2, 0.);
        let expected = ((2. * 0.02f64.powi(2) + 0.25f64.powi(2)) / 4.).sqrt();
        assert!((j.result(false).std as f64 - expected).abs() < 1e-8);
        assert!((j.result(false).mean - 0.02).abs() < 1e-8);
        assert_eq!(super::timing_mean_square(&[], 4, 4).sqrt(), 0.25);
        assert_eq!(super::timing_mean_square(&[], 0, 0), 0.);
        j.push_recent(f64::NAN, Judgement::Perfect, 2.);
        assert_eq!(j.offset_stats().unwrap().0, 2);
    }

    /// 判定条只记录真实偏移：Miss 不入队，队列长度封顶。
    #[test]
    fn recent_hits_records_real_offsets_only() {
        let mut j = JudgeInner::new(64);
        j.push_recent(0.01, Judgement::Perfect, 1.);
        j.push_recent(0.2, Judgement::Miss, 2.);
        assert_eq!(j.recent_hits().len(), 1);
        assert!(matches!(j.recent_hits()[0].judgement, Judgement::Perfect));

        for i in 0..MAX_RECENT_HITS + 5 {
            j.push_recent(i as f64 * 0.001, Judgement::Good, i as f64);
        }
        assert_eq!(j.recent_hits().len(), MAX_RECENT_HITS);
    }

    /// 血条：大 P/Perfect 回血、Bad/Miss 扣血；扣血受倍率影响，且夹在 0..=1，重开回到满血。
    #[test]
    fn hp_drains_and_heals() {
        let mut j = JudgeInner::new(64);
        assert!((j.hp() - 1.).abs() < 1e-5);

        j.commit(Judgement::Miss, 0.);
        assert!((j.hp() - 0.88).abs() < 1e-5);
        j.commit(Judgement::Bad, 0.);
        assert!((j.hp() - 0.82).abs() < 1e-5);
        j.commit(Judgement::Good, 0.);
        assert!((j.hp() - 0.822).abs() < 1e-5);
        j.commit(Judgement::Perfect, 0.);
        assert!((j.hp() - 0.832).abs() < 1e-5);
        j.commit(Judgement::PerfectPlus, 0.);
        assert!((j.hp() - 0.852).abs() < 1e-5);

        j.set_hp_amount(2.);
        j.commit(Judgement::Miss, 0.);
        assert!((j.hp() - 0.612).abs() < 1e-5);

        j.reset();
        assert!((j.hp() - 1.).abs() < 1e-5);
    }
}

#[rustfmt::skip]
#[cfg(closed)]
pub mod inner;
#[cfg(closed)]
use inner::*;

type Judgements = Vec<(f64, u32, u32, Result<Judgement, bool>)>;

#[repr(C)]
pub struct Judge {
    // notes of each line in order
    // LinkedList::drain_filter is unstable...
    pub notes: Vec<(Vec<u32>, usize)>,
    pub trackers: HashMap<u64, FlickTracker>,
    pub last_time: f64,
    pub infected: HashSet<u64>,
    held_touches: HashMap<u64, Touch>,
    sound_schedule: Vec<(f64, HitSound)>,
    sound_cursor: usize,

    key_down_count: u32,

    pub(crate) inner: JudgeInner,
    pub judgements: RefCell<Judgements>,
}

#[derive(Default)]
struct TouchStatus {
    touches: Vec<Touch>,
    key_delta: i32,
    keys_down: u32,
}

static SUBSCRIBER_ID: Lazy<usize> = Lazy::new(register_input_subscriber);
thread_local! {
    static TOUCHES: RefCell<TouchStatus> = RefCell::default();
    static WHEEL: RefCell<(f32, f32)> = RefCell::default();
}

pub fn take_wheel() -> (f32, f32) {
    WHEEL.with(|it| mem::take(&mut *it.borrow_mut()))
}

impl Judge {
    pub fn new(chart: &Chart, hold_tail_judge: bool) -> Self {
        let mut sound_schedule: Vec<_> = chart
            .lines
            .iter()
            .flat_map(|line| line.notes.iter())
            .filter(|note| !note.fake)
            .map(|note| (note.time, note.hitsound.clone()))
            .collect();
        sound_schedule.sort_by(|a, b| a.0.total_cmp(&b.0));
        let notes = chart
            .lines
            .iter()
            .map(|line| {
                let mut idx: Vec<u32> = (0..(line.notes.len() as u32)).filter(|it| !line.notes[*it as usize].fake).collect();
                idx.sort_by_key(|id| line.notes[*id as usize].time.not_nan());
                (idx, 0)
            })
            .collect();
        let mut num_of_notes: u32 = chart.lines.iter().map(|it| it.notes.iter().filter(|it| !it.fake).count() as u32).sum();
        if hold_tail_judge {
            // 尾判模式下每个 hold 头尾各判一次，谱面音符数也相应增加。
            num_of_notes += chart
                .lines
                .iter()
                .map(|it| it.notes.iter().filter(|it| !it.fake && matches!(it.kind, NoteKind::Hold { .. })).count() as u32)
                .sum::<u32>();
        }
        Self {
            notes,
            trackers: HashMap::new(),
            last_time: 0.,
            infected: HashSet::new(),
            held_touches: HashMap::new(),
            sound_schedule,
            sound_cursor: 0,

            key_down_count: 0,

            inner: JudgeInner::new(num_of_notes),
            judgements: RefCell::new(Vec::new()),
        }
    }

    pub fn reset(&mut self) {
        self.notes.iter_mut().for_each(|it| it.1 = 0);
        self.trackers.clear();
        self.infected.clear();
        self.held_touches.clear();
        self.sound_cursor = 0;
        self.key_down_count = 0;
        self.inner.reset();
        self.judgements.borrow_mut().clear();
    }

    /// Releases still arrive while gameplay judgement is paused. Consuming
    /// their lifecycle prevents phantom infected fingers after resuming.
    pub fn observe_paused_input(&mut self) {
        TOUCHES.with(|status| {
            let status = status.borrow();
            self.key_down_count = self.key_down_count.saturating_add_signed(status.key_delta);
            for touch in &status.touches {
                if matches!(touch.phase, TouchPhase::Started | TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.held_touches.remove(&touch.id);
                    self.infected.remove(&touch.id);
                    self.trackers.remove(&touch.id);
                }
            }
        });
    }

    /// Advance note pointers past notes before time `t`, marking them as judged.
    /// Used in exercise mode to skip notes before the exercise range start.
    pub fn advance_to(&mut self, chart: &mut Chart, t: f64) {
        for (line, (idx, st)) in chart.lines.iter_mut().zip(self.notes.iter_mut()) {
            while *st < idx.len() {
                let note = &mut line.notes[idx[*st] as usize];
                if note.time >= t {
                    break;
                }
                note.judge = JudgeStatus::Judged;
                *st += 1;
            }
        }
        self.last_time = t;
        self.sound_cursor = self.sound_schedule.partition_point(|(time, _)| *time < t);
    }

    pub fn commit(&mut self, t: f64, what: Judgement, line_id: u32, note_id: u32, diff: f64) {
        self.judgements.borrow_mut().push((t, line_id, note_id, Ok(what)));
        self.inner.commit(what, diff);
    }

    #[inline]
    pub fn accuracy(&self) -> f64 {
        self.inner.accuracy()
    }

    #[inline]
    pub fn real_time_accuracy(&self) -> f64 {
        self.inner.real_time_accuracy()
    }

    #[inline]
    pub fn score(&self, no_combo_score: bool) -> u32 {
        self.inner.score(no_combo_score)
    }

    pub(crate) fn on_new_frame() {
        let mut handler = Handler {
            status: TouchStatus::default(),
            wheel: (0., 0.),
        };
        repeat_all_miniquad_input(&mut handler, *SUBSCRIBER_ID);
        handler.finalize();
        TOUCHES.with(|it| {
            *it.borrow_mut() = handler.status;
        });
        WHEEL.with(|it| {
            *it.borrow_mut() = handler.wheel;
        });
    }

    fn touch_transform(flip_x: bool) -> impl Fn(&mut Touch) {
        let vp = get_viewport();
        move |touch| {
            let p = touch.position;
            touch.position = vec2(
                (p.x - vp.0 as f32) / vp.2 as f32 * 2. - 1.,
                ((p.y - (screen_height() - (vp.1 + vp.3) as f32)) / vp.3 as f32 * 2. - 1.) / (vp.2 as f32 / vp.3 as f32),
            );
            if flip_x {
                touch.position.x *= -1.;
            }
        }
    }

    pub fn get_touches() -> Vec<Touch> {
        TOUCHES.with(|it| {
            let guard = it.borrow();
            let tr = Self::touch_transform(false);
            guard
                .touches
                .iter()
                .cloned()
                .map(|mut it| {
                    tr(&mut it);
                    it
                })
                .collect()
        })
    }

    pub fn update(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>) {
        chart.blocked_touches.clear();
        if res.config.has_mod(Mods::PERFECT_SOUND) {
            while let Some((time, sound)) = self.sound_schedule.get(self.sound_cursor) {
                if *time > res.time {
                    break;
                }
                sound.play_chart_cue(res);
                self.sound_cursor += 1;
            }
        }
        if res.config.autoplay() {
            self.auto_play_update(res, chart);
            return;
        }
        const X_DIFF_MAX: f64 = 0.21 / (16. / 9.) * 2.;
        let spd = res.config.speed as f64;
        // 晚按补偿（秒）：晚按一侧额外放宽；默认 0 = 与早按完全对称。
        let late_leniency = res.config.late_leniency();
        // 黄键 / 红键保护：蓝键不会被叠在附近的黄 / 红键抢走判定。
        let drag_protect = res.config.drag_protect;
        let flick_protect = res.config.flick_protect;

        let uptime = get_uptime();

        let t = res.time;
        // TODO optimize
        let mut touches: HashMap<u64, Touch> = {
            let mut touches = touches();
            let btn = MouseButton::Left;
            let id = button_to_id(btn);
            if is_mouse_button_pressed(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Started,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            } else if is_mouse_button_down(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Moved,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            } else if is_mouse_button_released(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Ended,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            }
            let tr = Self::touch_transform(res.config.flip_x());
            let mut current: HashMap<_, _> = touches
                .into_iter()
                .map(|mut it| {
                    tr(&mut it);
                    (it.id, it)
                })
                .collect();
            for (&id, touch) in &self.held_touches {
                current.entry(id).or_insert_with(|| Touch {
                    phase: TouchPhase::Stationary,
                    time: f64::NEG_INFINITY,
                    ..touch.clone()
                });
            }
            current
        };
        let (events, keys_down, key_delta) = TOUCHES.with(|it| {
            let guard = it.borrow();
            let events = guard.touches.clone();
            if res.config.use_keyboard {
                (events, guard.keys_down, guard.key_delta)
            } else {
                (events, 0, 0)
            }
        });
        self.key_down_count = self.key_down_count.saturating_add_signed(key_delta);
        {
            fn to_local(Vec2 { x, y }: Vec2) -> Point {
                Point::new(x / screen_width() * 2. - 1., y / screen_height() * 2. - 1.)
            }
            let delta = (t / spd - self.last_time) / (events.len() + 1) as f64;
            let mut t = self.last_time;
            for Touch {
                id,
                phase,
                position: p,
                time,
            } in events.into_iter()
            {
                t += delta;
                let t = t as f32;
                let mut event = Touch {
                    id,
                    phase,
                    position: p,
                    time,
                };
                Self::touch_transform(res.config.flip_x())(&mut event);
                let p = to_local(p);
                match phase {
                    TouchPhase::Started => {
                        self.infected.remove(&id);
                        self.trackers.insert(id, FlickTracker::new(res.dpi, t, p));
                        touches.insert(id, event);
                    }
                    TouchPhase::Moved | TouchPhase::Stationary => {
                        let phase = touches.get(&id).map(|it| it.phase);
                        if phase == Some(TouchPhase::Started) {
                            event.phase = TouchPhase::Started;
                        }
                        touches.insert(id, event);
                        if let Some(tracker) = self.trackers.get_mut(&id) {
                            tracker.push(t, p);
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.trackers.remove(&id);
                        self.infected.remove(&id);
                        touches.remove(&id);
                    }
                }
            }
        }
        touches.retain(|id, touch| {
            let down = !matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled);
            if !down {
                self.infected.remove(id);
            }
            down
        });
        self.held_touches.clone_from(&touches);
        let mut touches: Vec<Touch> = touches
            .into_values()
            .map(|mut it| {
                it.time = if it.time.is_infinite() {
                    f64::NEG_INFINITY
                } else {
                    t - (uptime - it.time) * spd
                };
                it
            })
            .collect();
        // Phigros 9th-chapter block areas: a touch that lands inside an active
        // zone (`enableTime <= t < disableTime`) is removed from the touch list,
        // so the notes underneath it are never hit and end up as misses.
        if !chart.block_areas.is_empty() {
            let aspect = res.aspect_ratio;
            let mut blocked = Vec::new();
            // Infection is a finger lifetime, not a field lifetime. Quiet
            // frames must retain it until an explicit Ended/Cancelled event.
            touches.retain(|touch| {
                let p = Vector::new(touch.position.x, -touch.position.y);
                let inside = !self.infected.contains(&touch.id) && chart.touch_blocked(p, t, aspect);
                if finger_blocked(&mut self.infected, touch.id, TouchPhase::Stationary, inside) {
                    blocked.push((touch.id, p));
                    false
                } else {
                    true
                }
            });
            chart.blocked_touches = blocked;
        }
        // pos[line][touch]
        let mut pos = Vec::<Vec<Option<Point>>>::with_capacity(chart.lines.len());
        for id in 0..chart.lines.len() {
            chart.lines[id].object.set_time(t);
            let inv = chart.lines[id].now_transform(res, &chart.lines).try_inverse().unwrap();
            pos.push(
                touches
                    .iter()
                    .map(|touch| {
                        let p = touch.position;
                        let p = inv.transform_point(&Point::new(p.x, -p.y));
                        fn ok(f: f32) -> bool {
                            matches!(f.classify(), FpCategory::Zero | FpCategory::Subnormal | FpCategory::Normal)
                        }
                        if ok(p.x) && ok(p.y) {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
        }
        let time_of = |touch: &Touch| {
            if touch.time.is_infinite() {
                t
            } else {
                touch.time
            }
        };
        let limits = res.windows;
        let mut judgements = Vec::new();
        // 尾判模式：hold 的尾判需要额外结算一次（`(line_id, note_id, 尾判偏移)`）。
        // 和 `judgements` 分开收集，避免在后面那个循环里再借 `self.notes`。
        let mut tail_judgements: Vec<(usize, u32, f64)> = Vec::new();
        // clicks & flicks
        for (id, touch) in touches.iter().enumerate() {
            let click = touch.phase == TouchPhase::Started;
            let flick =
                matches!(touch.phase, TouchPhase::Moved | TouchPhase::Stationary) && self.trackers.get_mut(&touch.id).is_some_and(|it| it.flicked);
            if !(click || flick) {
                continue;
            }
            let t = time_of(touch);
            let mut closest = (None, X_DIFF_MAX, limits.bad, limits.bad + (X_DIFF_MAX / NOTE_WIDTH_RATIO_BASE - 1.).max(0.) * DIST_FACTOR);
            for (line_id, ((line, pos), (idx, st))) in chart.lines.iter_mut().zip(pos.iter()).zip(self.notes.iter_mut()).enumerate() {
                let Some(pos) = pos[id] else {
                    continue;
                };
                for id in &idx[*st..] {
                    let note = &mut line.notes[*id as usize];
                    if !matches!(note.judge, JudgeStatus::NotJudged | JudgeStatus::PreJudge) {
                        continue;
                    }
                    if !click && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. }) {
                        continue;
                    }
                    // 开启红 / 黄保护时，它们不参与普通点击的候选竞争；有效的蓝键仍可正常命中。
                    let protected =
                        click && ((drag_protect && matches!(note.kind, NoteKind::Drag)) || (flick_protect && matches!(note.kind, NoteKind::Flick)));
                    let dt = (note.time - t) / spd;
                    if dt >= closest.3.max(limits.bad) {
                        break;
                    }
                    // 晚按（dt < 0）时按配置放宽；默认 0 → 和早按完全对称。
                    let dt = judge_distance(-dt, late_leniency);
                    let x = &mut note.object.translation.0;
                    x.set_time(t);
                    let dist = if limits.fullscreen {
                        0.
                    } else {
                        (x.now() - pos.x).abs() as f64 / note.judge_area as f64
                    };
                    if dist > X_DIFF_MAX {
                        continue;
                    }
                    let gate = if matches!(note.kind, NoteKind::Click) {
                        limits.bad - limits.perfect * (dist - 0.9).max(0.)
                    } else {
                        limits.good
                    };
                    if dt > gate {
                        continue;
                    }
                    if protected {
                        // 保护只屏蔽红 / 黄音符本身，不应吞掉同范围内可判定的蓝键。
                        continue;
                    }
                    let dt = if matches!(note.kind, NoteKind::Flick | NoteKind::Drag) {
                        dt + limits.good
                    } else {
                        dt
                    };
                    let key = dt + (dist / NOTE_WIDTH_RATIO_BASE - 1.).max(0.) * DIST_FACTOR;
                    if key < closest.3 {
                        closest = (Some((line_id, *id)), dist, dt, key);
                    }
                }
            }
            if let (Some((line_id, id)), _, dt, _) = closest {
                let line = &mut chart.lines[line_id];
                if matches!(line.notes[id as usize].kind, NoteKind::Drag) {
                    continue;
                }
                if click {
                    // click & hold
                    let note = &mut line.notes[id as usize];
                    if matches!(note.kind, NoteKind::Flick) {
                        continue; // to next loop
                    }
                    if dt <= limits.good || matches!(note.kind, NoteKind::Hold { .. }) {
                        let perfect = dt <= limits.perfect;
                        match note.kind {
                            NoteKind::Click => {
                                note.judge = JudgeStatus::Judged;
                                let judgement = if dt <= limits.perfect_plus {
                                    Judgement::PerfectPlus
                                } else if perfect {
                                    Judgement::Perfect
                                } else {
                                    Judgement::Good
                                };
                                judgements.push((judgement, line_id, id, Some(t)));
                            }
                            NoteKind::Hold { .. } => {
                                note.hitsound.play(res);
                                self.judgements.borrow_mut().push((t, line_id as _, id, Err(perfect)));
                                // 头判的偏移：按下时就进判定条（原来要等按住结束才显示）。
                                let head_j = if perfect {
                                    if dt <= limits.perfect_plus {
                                        Judgement::PerfectPlus
                                    } else {
                                        Judgement::Perfect
                                    }
                                } else {
                                    Judgement::Good
                                };
                                self.inner.push_recent((t - note.time) / spd, head_j, t);
                                // 尾判模式：头判在按下时立即结算，尾判等松手 / 结尾再结算。
                                if res.config.hold_tail_judge {
                                    self.commit(t, head_j, line_id as _, id, (t - note.time) / spd);
                                }
                                note.judge = JudgeStatus::Hold(perfect, t, t, false, f64::INFINITY);
                            }
                            _ => unreachable!(),
                        };
                    } else {
                        // prevent extra judgements
                        if matches!(note.judge, JudgeStatus::NotJudged) {
                            // keep the note after bad judgement
                            line.notes[id as usize].judge = JudgeStatus::PreJudge;
                            judgements.push((Judgement::Bad, line_id, id, None));
                        }
                    }
                } else {
                    // flick
                    line.notes[id as usize].judge = JudgeStatus::PreJudge;
                    if let Some(tracker) = self.trackers.get_mut(&touch.id) {
                        tracker.flicked = false;
                    }
                }
            }
        }
        for _ in 0..keys_down {
            // find the earliest not judged click / hold note
            if let Some((line_id, id)) = chart
                .lines
                .iter()
                .zip(self.notes.iter())
                .enumerate()
                .filter_map(|(line_id, (line, (idx, st)))| {
                    idx[*st..]
                        .iter()
                        .cloned()
                        .find(|id| {
                            let note = &line.notes[*id as usize];
                            matches!(note.judge, JudgeStatus::NotJudged) && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. })
                        })
                        .map(|id| (line_id, id))
                })
                .min_by_key(|(line_id, id)| chart.lines[*line_id].notes[*id as usize].time.not_nan())
            {
                let note = &mut chart.lines[line_id].notes[id as usize];
                let dt = judge_distance((t - note.time) / spd, late_leniency);
                if dt <= if matches!(note.kind, NoteKind::Click) { limits.bad } else { limits.good } {
                    let perfect = dt <= limits.perfect;
                    match note.kind {
                        NoteKind::Click => {
                            note.judge = JudgeStatus::Judged;
                            judgements.push((
                                if dt <= limits.perfect_plus {
                                    Judgement::PerfectPlus
                                } else if perfect {
                                    Judgement::Perfect
                                } else if dt <= limits.good {
                                    Judgement::Good
                                } else {
                                    Judgement::Bad
                                },
                                line_id,
                                id,
                                None,
                            ));
                        }
                        NoteKind::Hold { .. } => {
                            note.hitsound.play(res);
                            self.judgements.borrow_mut().push((t, line_id as _, id, Err(perfect)));
                            // 头判的偏移：按下时就进判定条（原来要等按住结束才显示）。
                            let head_j = if perfect {
                                if dt <= limits.perfect_plus {
                                    Judgement::PerfectPlus
                                } else {
                                    Judgement::Perfect
                                }
                            } else {
                                Judgement::Good
                            };
                            self.inner.push_recent((t - note.time) / spd, head_j, t);
                            // 尾判模式：头判在按下时立即结算，尾判等松手 / 结尾再结算。
                            if res.config.hold_tail_judge {
                                self.commit(t, head_j, line_id as _, id, (t - note.time) / spd);
                            }
                            note.judge = JudgeStatus::Hold(perfect, t, t, false, f64::INFINITY);
                        }
                        _ => unreachable!(),
                    };
                }
            } else {
                break;
            }
        }
        for (line_id, ((line, pos), (idx, st))) in chart.lines.iter_mut().zip(pos.iter()).zip(self.notes.iter()).enumerate() {
            line.object.set_time(t);
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let NoteKind::Hold { end_time, .. } = &note.kind {
                    if res.config.hold_tail_judge {
                        // 尾判（mania 风格）：头判在按下时已结算，这里只跟踪「松手时刻」，
                        // 到结尾时刻再按松手早晚结算尾判（见下面的 `tail_judgements`）。
                        if let JudgeStatus::Hold(_, _, _, _, ref mut up_time) = note.judge {
                            let x = &mut note.object.translation.0;
                            x.set_time(t);
                            let x = x.now();
                            let on_note = self.key_down_count != 0
                                || pos.iter().any(|it| {
                                    it.is_some_and(|it| limits.fullscreen || (it.x - x).abs() as f64 / note.judge_area as f64 <= X_DIFF_MAX)
                                });
                            if on_note {
                                *up_time = f64::INFINITY;
                            } else {
                                if up_time.is_infinite() {
                                    *up_time = t;
                                }
                                // 松手位置离结尾还远（超出 bad 窗）→ 尾判直接 Miss。
                                if t > *up_time + UP_TOLERANCE && (*end_time - t) / spd > limits.bad {
                                    note.judge = JudgeStatus::Judged;
                                    judgements.push((Judgement::Miss, line_id, *id, None));
                                }
                            }
                            continue;
                        }
                    }
                    if let JudgeStatus::Hold(.., ref mut pre_judge, ref mut up_time) = note.judge {
                        if (*end_time - t) / spd <= limits.bad {
                            *pre_judge = true;
                            continue;
                        }
                        let x = &mut note.object.translation.0;
                        x.set_time(t);
                        let x = x.now();
                        if self.key_down_count == 0
                            && !pos
                                .iter()
                                .any(|it| it.is_some_and(|it| limits.fullscreen || (it.x - x).abs() as f64 / note.judge_area as f64 <= X_DIFF_MAX))
                        {
                            if t > *up_time + UP_TOLERANCE {
                                note.judge = JudgeStatus::Judged;
                                judgements.push((Judgement::Miss, line_id, *id, None));
                            } else if up_time.is_infinite() {
                                *up_time = t;
                            }
                        } else {
                            *up_time = f64::INFINITY;
                        }
                        continue;
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                // process miss
                let dt = (t - note.time) / spd;
                if dt > limits.bad + late_leniency {
                    note.judge = JudgeStatus::Judged;
                    judgements.push((Judgement::Miss, line_id, *id, None));
                    if res.config.hold_tail_judge && matches!(note.kind, NoteKind::Hold { .. }) {
                        // 尾判模式下 hold 头尾各算一次
                        judgements.push((Judgement::Miss, line_id, *id, None));
                    }
                    continue;
                }
                if -dt > limits.bad {
                    break;
                }
                if !matches!(note.kind, NoteKind::Drag) && (self.key_down_count == 0 || !matches!(note.kind, NoteKind::Flick)) {
                    continue;
                }
                let dt = judge_distance(dt, late_leniency);
                let x = &mut note.object.translation.0;
                x.set_time(t);
                let x = x.now();
                if self.key_down_count != 0
                    || pos.iter().any(|it| {
                        it.is_some_and(|it| {
                            let dx = if limits.fullscreen {
                                0.
                            } else {
                                (it.x - x).abs() as f64 / note.judge_area as f64
                            };
                            dx <= X_DIFF_MAX && dt <= (limits.bad - limits.perfect * (dx - 0.9).max(0.))
                        })
                    })
                {
                    note.judge = JudgeStatus::PreJudge;
                }
            }
        }
        // process pre-judge
        for (line_id, (line, (idx, st))) in chart.lines.iter_mut().zip(self.notes.iter()).enumerate() {
            line.object.set_time(t);
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                // 尾判模式：按松手时刻相对结尾的早晚结算尾判（头判已在按下时结算）。
                if res.config.hold_tail_judge {
                    if let JudgeStatus::Hold(_, _, _, _, up) = note.judge {
                        if let NoteKind::Hold { end_time, .. } = note.kind {
                            if end_time <= t {
                                note.judge = JudgeStatus::Judged;
                                let off = if up.is_infinite() { 0. } else { (up - end_time) / spd };
                                tail_judgements.push((line_id, *id, off));
                                continue;
                            }
                        }
                    }
                }
                if let JudgeStatus::Hold(perfect, .., diff, true, _) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = &note.kind {
                        if *end_time <= t {
                            note.judge = JudgeStatus::Judged;
                            let judgement = if perfect {
                                if judge_distance((diff - note.time) / spd, late_leniency) <= limits.perfect_plus {
                                    Judgement::PerfectPlus
                                } else {
                                    Judgement::Perfect
                                }
                            } else {
                                Judgement::Good
                            };
                            judgements.push((judgement, line_id, *id, Some(diff)));
                            continue;
                        }
                    }
                }
                // TODO adjust
                let ghost_t = t + limits.good;
                if matches!(note.kind, NoteKind::Click) {
                    if ghost_t < note.time {
                        break;
                    }
                } else if t < note.time {
                    // `idx` 按时间有序：遇到未来音符后，后面全是未来音符，直接 break 退出即可。
                    // （原来是 `continue`，会在**每帧**把该线从 `st` 到结尾的**全部**音符扫一遍，
                    // 非 Click 音符极多的谱面（Flick/Drag 观赏谱）会因此每帧扫描数百万个音符而卡死。）
                    break;
                }
                if matches!(note.judge, JudgeStatus::PreJudge) {
                    let diff = if let JudgeStatus::Hold(.., diff, _, _) = note.judge {
                        Some(diff)
                    } else {
                        None
                    };
                    note.judge = JudgeStatus::Judged;
                    if !matches!(note.kind, NoteKind::Click) {
                        let judgement = if diff.is_some_and(|d| (d - note.time).abs() / spd <= limits.perfect_plus) {
                            Judgement::PerfectPlus
                        } else {
                            Judgement::Perfect
                        };
                        judgements.push((judgement, line_id, *id, diff));
                    }
                }
            }
        }
        for (judgement, line_id, id, diff) in judgements {
            let line = &mut chart.lines[line_id];
            let note = &mut line.notes[id as usize];
            line.object.set_time(t);
            note.object.set_time(t);
            let line = &chart.lines[line_id];
            let note = &line.notes[id as usize];
            let line_tr = line.now_transform(res, &chart.lines);
            let offset = if matches!(judgement, Judgement::Miss) {
                0.25
            } else if matches!(note.kind, NoteKind::Drag | NoteKind::Flick) {
                0.
            } else {
                (diff.unwrap_or(t) - note.time) / spd
            };
            // 拖拽/滑动的偏移是补齐出来的合成值，不进判定条；
            // hold 的头判偏移在按下时已经进过判定条了，这里不再重复。
            if !matches!(note.kind, NoteKind::Drag | NoteKind::Flick | NoteKind::Hold { .. }) {
                self.inner.push_recent(offset, judgement, t);
            }
            self.commit(t, judgement, line_id as _, id, offset);
            if matches!(note.kind, NoteKind::Hold { .. }) {
                continue;
            }
            if match judgement {
                // 大 P 与 P 都算命中：都要出打击特效与音效。
                // （漏掉 `PerfectPlus` 会让大 P 以及自动判定的拖拽 / 滑动音符完全没有特效和音效，
                // 多押时几路按键的时间戳略有差异、常常一个大 P 一个不是，看起来就像「只渲染了一个」。）
                Judgement::Perfect | Judgement::PerfectPlus => {
                    res.with_model(line_tr * note.object.now(res), |res| {
                        res.emit_at_origin(note.rotation(line), note.fx_color.unwrap_or_else(|| res.res_pack.info.fx_perfect()))
                    });
                    true
                }
                Judgement::Good => {
                    res.with_model(line_tr * note.object.now(res), |res| {
                        res.emit_at_origin(note.rotation(line), note.fx_color.unwrap_or_else(|| res.res_pack.info.fx_good()))
                    });
                    true
                }
                Judgement::Bad => {
                    if !matches!(note.kind, NoteKind::Hold { .. }) {
                        bad_notes.push(BadNote {
                            time: t,
                            kind: note.kind.clone(),
                            matrix: {
                                let mut mat = line_tr;
                                if !note.above {
                                    mat.append_nonuniform_scaling_mut(&Vector::new(1., -1.));
                                }
                                let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                                mat *= note.now_transform(
                                    res,
                                    &line.ctrl_obj.borrow_mut(),
                                    ((note.height - line.height.now() as f64) / res.aspect_ratio as f64 * note.speed * res.config.flow_speed as f64)
                                        as f32,
                                    incline_sin,
                                );
                                mat
                            },
                        });
                    }
                    false
                }
                _ => false,
            } {
                note.hitsound.play(res);
            }
        }
        for (line, (idx, st)) in chart.lines.iter().zip(self.notes.iter_mut()) {
            while idx
                .get(*st)
                .is_some_and(|id| matches!(line.notes[*id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
        // 尾判模式：hold 的尾判统一在这里结算——按松手时刻相对结尾的早晚定档。
        for (line_id, id, off) in tail_judgements {
            let j = judgement_of_offset(off, &limits);
            self.inner.push_recent(off, j, t);
            self.commit(t, j, line_id as _, id, off);
        }
        self.last_time = t / spd;
    }

    fn auto_play_update(&mut self, res: &mut Resource, chart: &mut Chart) {
        let t = res.time;
        let spd = res.config.speed as f64;
        let mut judgements = Vec::new();
        for (line_id, (line, (idx, st))) in chart.lines.iter_mut().zip(self.notes.iter_mut()).enumerate() {
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let JudgeStatus::Hold(..) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = note.kind {
                        if t >= end_time {
                            note.judge = JudgeStatus::Judged;
                            judgements.push((line_id, *id));
                            continue;
                        }
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                if note.time > t {
                    break;
                }
                note.judge = if matches!(note.kind, NoteKind::Hold { .. }) {
                    note.hitsound.play(res);
                    self.judgements.borrow_mut().push((t, line_id as _, *id, Err(true)));
                    JudgeStatus::Hold(true, t, (t - note.time) / spd, false, f64::INFINITY)
                } else {
                    judgements.push((line_id, *id));
                    JudgeStatus::Judged
                };
            }
            while idx
                .get(*st)
                .is_some_and(|id| matches!(line.notes[*id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
        for (line_id, id) in judgements.into_iter() {
            self.commit(t, Judgement::PerfectPlus, line_id as _, id, 0.);
            // 尾判模式：hold 头尾各算一次。
            if res.config.hold_tail_judge && matches!(chart.lines[line_id].notes[id as usize].kind, NoteKind::Hold { .. }) {
                self.commit(t, Judgement::PerfectPlus, line_id as _, id, 0.);
            }
            let (note_transform, note_hitsound) = {
                let line = &mut chart.lines[line_id];
                let note = &mut line.notes[id as usize];
                let nt = if matches!(note.kind, NoteKind::Hold { .. }) { t } else { note.time };
                line.object.set_time(nt);
                note.object.set_time(nt);
                (note.object.now(res), note.hitsound.clone())
            };
            let line = &chart.lines[line_id];
            res.with_model(line.now_transform(res, &chart.lines) * note_transform, |res| {
                res.emit_at_origin(line.notes[id as usize].rotation(line), res.res_pack.info.fx_perfect())
            });
            if !matches!(chart.lines[line_id].notes[id as usize].kind, NoteKind::Hold { .. }) {
                note_hitsound.play(res);
            }
        }
    }

    #[inline]
    pub fn result(&self, no_combo_score: bool) -> PlayResult {
        self.inner.result(no_combo_score)
    }

    #[inline]
    pub fn combo(&self) -> u32 {
        self.inner.combo()
    }

    #[inline]
    pub fn counts(&self) -> [u32; 5] {
        self.inner.counts()
    }

    pub fn recent_hits(&self) -> &[RecentHit] {
        self.inner.recent_hits()
    }

    pub fn hp(&self) -> f32 {
        self.inner.hp()
    }

    pub fn set_hp_amount(&mut self, amount: f32) {
        self.inner.set_hp_amount(amount);
    }

    pub fn set_hp_scale(&mut self, scale: f32) {
        self.inner.set_hp_scale(scale);
    }
}

struct Handler {
    status: TouchStatus,
    wheel: (f32, f32),
}
impl Handler {
    fn finalize(&mut self) {
        if is_mouse_button_down(MouseButton::Left) {
            self.status.touches.push(Touch {
                id: button_to_id(MouseButton::Left),
                phase: TouchPhase::Moved,
                position: mouse_position().into(),
                time: f64::NEG_INFINITY,
            });
        }
    }
}

fn button_to_id(button: MouseButton) -> u64 {
    u64::MAX
        - match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Unknown => 3,
        }
}

impl EventHandler for Handler {
    fn update(&mut self, _: &mut miniquad::Context) {}
    fn draw(&mut self, _: &mut miniquad::Context) {}
    fn touch_event(&mut self, _: &mut miniquad::Context, phase: miniquad::TouchPhase, id: u64, x: f32, y: f32, time: f64) {
        self.status.touches.push(Touch {
            id,
            phase: phase.into(),
            position: vec2(x, y),
            time,
        });
    }

    fn mouse_wheel_event(&mut self, _ctx: &mut miniquad::Context, x: f32, y: f32) {
        self.wheel.0 += x;
        self.wheel.1 += y;
    }

    fn mouse_button_down_event(&mut self, _ctx: &mut miniquad::Context, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Started,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn mouse_button_up_event(&mut self, _ctx: &mut miniquad::Context, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Ended,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn key_down_event(&mut self, _ctx: &mut miniquad::Context, _keycode: KeyCode, _keymods: miniquad::KeyMods, repeat: bool) {
        if !repeat {
            self.status.key_delta += 1;
            self.status.keys_down += 1;
        }
    }

    fn key_up_event(&mut self, _ctx: &mut miniquad::Context, _keycode: KeyCode, _keymods: miniquad::KeyMods) {
        self.status.key_delta -= 1;
    }
}

#[derive(Default)]
pub struct PlayResult {
    pub score: u32,
    pub accuracy: f64,
    pub max_combo: u32,
    pub num_of_notes: u32,
    pub counts: [u32; 5],
    pub early: u32,
    pub late: u32,
    /// Score-protocol RMS timing error (seconds), including 250ms misses.
    pub std: f32,
    /// 本局所有有效命中偏差的平均值（秒），负数偏早、正数偏晚。
    pub mean: f32,
    /// 本局所有有效命中的偏移（秒），供结算界面绘制偏差分布直方图。
    pub offsets: Vec<f64>,
    /// 判定时间误差分布（-HIST_MAX_MS .. +HIST_MAX_MS，共 HIST_BUCKETS 个桶）。
    pub hist: [u32; HIST_BUCKETS],
    pub early_kind: [u32; 5],
    pub late_kind: [u32; 5],
}

impl PlayResult {
    pub fn displayed_score(&self, theoretical: bool) -> u32 {
        self.score
            .saturating_add(if theoretical { self.counts[Judgement::PerfectPlus as usize] } else { 0 })
    }
}

pub fn icon_index(score: u32, full_combo: bool) -> usize {
    match (score, full_combo) {
        (x, _) if x < 700000 => 0,
        (x, _) if x < 820000 => 1,
        (x, _) if x < 880000 => 2,
        (x, _) if x < 920000 => 3,
        (x, _) if x < 960000 => 4,
        (1000000, _) => 7,
        (_, false) => 5,
        (_, true) => 6,
    }
}
