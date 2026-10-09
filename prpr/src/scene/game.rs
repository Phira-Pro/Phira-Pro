#![allow(unused)]

prpr_l10n::tl_file!("game");

use super::{
    draw_background,
    ending::RecordUpdateState,
    loading::{BasicPlayer, SaveFn, UpdateFn, UploadFn},
    request_input, return_input, show_message, take_input, EndingScene, NextScene, Scene,
};
use crate::{
    bin::BinaryReader,
    config::{Config, Mods},
    core::{copy_fbo, BadNote, Chart, ChartExtra, Effect, JudgeLineKind, Matrix, NoteKind, Point, Resource, UIElement, Vector, PGR_FONT},
    ext::{parse_time, screen_aspect, semi_white, spawn_task, RectExt, SafeTexture, ScaleType},
    fs::FileSystem,
    info::{ChartFormat, ChartInfo},
    judge::{Judge, Judgement, RecentHit},
    parse::{parse_extra, parse_pec, parse_rpe_with_path, SendChart},
    task::Task,
    time::TimeManager,
    ui::{OffsetAnalysisPanel, OffsetPanelAction, OffsetPanelLabels, RectButton, TextPainter, Ui},
};
use anyhow::{bail, Context, Result};
use concat_string::concat_string;
use inputbox::InputBox;
use macroquad::{prelude::*, window::InternalGlContext};
use sasa::{Music, MusicParams};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{Cursor, ErrorKind},
    ops::{Deref, DerefMut, Range},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::Arc,
    thread_local,
    time::Duration,
};
use tracing::{debug, warn};

/// 判定条指针的缓动状态（照搬皮肤里 `pointerMoveDuration = 500ms` 的指针动画）。
#[derive(Default)]
struct OffsetPointer {
    /// 已经缓动到的命中时刻，用来识别「出现新命中」。
    hit_time: f64,
    from: f32,
    to: f32,
    start: f64,
    now: f32,
}

impl OffsetPointer {
    const DURATION: f64 = 0.5;

    /// 缓动到最近一次命中的位置；`target` 为 `None`（本局没有命中，例如重开后）时归零。
    fn update(&mut self, time: f64, target: Option<(f64, f32)>) {
        match target {
            Some((hit_time, to)) => {
                if hit_time != self.hit_time {
                    self.hit_time = hit_time;
                    self.from = self.now;
                    self.to = to;
                    self.start = time;
                }
            }
            None => {
                if self.hit_time != 0. {
                    *self = Self::default();
                }
                return;
            }
        }
        let p = ((time - self.start) / Self::DURATION).clamp(0., 1.);
        let p = 1. - (1. - p).powi(3);
        self.now = self.from + (self.to - self.from) * p as f32;
    }
}

thread_local! {
    static OFFSET_POINTER: RefCell<OffsetPointer> = RefCell::default();
}

/// 局内 early/late 判定条。
///
/// 样式照搬 Malody 皮肤 "6513_Elaina_PC_4K" 的 `OFFSET_INDICATOR`：
/// 一条中线参考线 + 停在最近一次命中位置上的下指箭头 + 每次命中留下的竖条残影。
/// 残影按判定档着色、整体随时间淡出：Perfect+ 用贴图原始的三段渐变，其余档用单色。
/// 配色取自该皮肤的 `info.asm` 与 `shadow0_1.png` 实测值：
/// 渐变 #99D4EF→#F9CBF7→#FDE7BC、shadow1(#99D4EF)、shadow2(#D7A5C3)、shadow3(#EABE93)。
/// 指针用皮肤提供的下指箭头贴图 `assets/offset_indicator.png`。
fn draw_offset_indicator(ui: &mut Ui, res: &Resource, recent: &[RecentHit], top: f32) {
    /// 判定条横向覆盖的毫秒范围（中线两侧各一半）。
    const SPAN_MS: f64 = 100.;
    /// 整体尺寸缩放系数（相对最初版本）。
    const SCALE: f32 = 0.25;
    const BAR_W: f32 = 0.56 * SCALE;
    const BAR_H: f32 = 0.05 * SCALE;
    const LINE_H: f32 = 0.09 * SCALE;
    /// 竖线类元件按比例缩放后会不到一个像素，这里给一个可见性下限。
    const MIN_STROKE: f32 = 0.003;
    const POINTER_W: f32 = 0.1 * SCALE;
    const POINTER_ASPECT: f32 = 198. / 303.;
    const FADE: f64 = 3.;
    const MAX_ALPHA: f32 = 0.7;

    const C_GRAD_TOP: Color = Color {
        r: 0.6,
        g: 0.831,
        b: 0.937,
        a: 1.,
    };
    const C_GRAD_MID: Color = Color {
        r: 0.976,
        g: 0.796,
        b: 0.969,
        a: 1.,
    };
    const C_GRAD_BOTTOM: Color = Color {
        r: 0.992,
        g: 0.906,
        b: 0.737,
        a: 1.,
    };
    const C_PERFECT: Color = Color {
        r: 0.6,
        g: 0.831,
        b: 0.937,
        a: 1.,
    };
    const C_GOOD: Color = Color {
        r: 0.843,
        g: 0.647,
        b: 0.765,
        a: 1.,
    };
    const C_BAD: Color = Color {
        r: 0.918,
        g: 0.745,
        b: 0.576,
        a: 1.,
    };

    let cy = top + BAR_H / 2.;
    let to_x = |offset: f64| ((offset * 1000. / SPAN_MS).clamp(-1., 1.) as f32) * (BAR_W / 2.);
    let fill_v = |ui: &mut Ui, r: Rect, top: Color, bottom: Color, alpha: f32| {
        ui.fill_rect(
            r,
            (
                Color { a: top.a * alpha, ..top },
                (r.x, r.y),
                Color {
                    a: bottom.a * alpha,
                    ..bottom
                },
                (r.x, r.y + r.h),
            ),
        );
    };

    // 只有中线，没有背景板。
    fill_v(ui, Rect::new(-MIN_STROKE / 2., cy - LINE_H / 2., MIN_STROKE, LINE_H), WHITE, WHITE, 0.45);

    for hit in recent {
        let age = res.time - hit.time;
        if !(0. ..=FADE).contains(&age) {
            continue;
        }
        let alpha = (1. - age / FADE) as f32 * MAX_ALPHA;
        let x = to_x(hit.offset);
        let rect = Rect::new(x - MIN_STROKE / 2., cy - BAR_H / 2., MIN_STROKE, BAR_H);
        if matches!(hit.judgement, Judgement::PerfectPlus) {
            let mid = rect.y + rect.h * 0.5;
            fill_v(ui, Rect::new(rect.x, rect.y, rect.w, mid - rect.y), C_GRAD_TOP, C_GRAD_MID, alpha);
            fill_v(ui, Rect::new(rect.x, mid, rect.w, rect.y + rect.h - mid), C_GRAD_MID, C_GRAD_BOTTOM, alpha);
        } else {
            let color = match hit.judgement {
                Judgement::Perfect => C_PERFECT,
                Judgement::Good => C_GOOD,
                Judgement::Great => Color::from_hex_rgb(0x80cbc4),
                Judgement::Ok => Color::from_hex_rgb(0xffb74d),
                Judgement::Meh => Color::from_hex_rgb(0xff8a65),
                _ => C_BAD,
            };
            fill_v(ui, rect, color, color, alpha);
        }
    }

    let target = recent.last().map(|hit| (hit.time, to_x(hit.offset)));
    let px = OFFSET_POINTER.with(|it| {
        let mut it = it.borrow_mut();
        it.update(res.time, target);
        it.now
    });
    let pw = POINTER_W;
    let ph = pw * POINTER_ASPECT;
    let pr = Rect::new(px - pw / 2., cy - BAR_H / 2. - 0.004 - ph, pw, ph);
    ui.fill_rect(pr, (*res.offset_indicator, pr, ScaleType::Fit));
}

const PAUSE_CLICK_INTERVAL: f32 = 0.7;

/// 死亡过渡时长：谱面继续前进但逐渐减速的时长（秒）。
const DEATH_TIME: f64 = 2.;
/// 死亡过渡结束后，失败遮罩与按钮渐显的时长（秒）。
const DEATH_FADE: f64 = 0.5;

/// 谱面调试叠加层：判定线的编号 / 线高 / z-index / 类型，以及音符的时间 / 高度 / 类型
/// 与横向判定范围。移植自上游改版 Phirc Mod++。
fn debug_overlay(res: &Resource, chart: &Chart, ui: &mut Ui) {
    let lines = &chart.lines;
    if res.config.chart_debug_line {
        for (id, line) in lines.iter().enumerate() {
            let tr = line.now_transform(res, lines);
            let pos = debug_screen_transform(res.config.flip_x()) * tr;
            let pos = pos.transform_point(&Point::new(0., 0.));
            let h = line.height.now();
            // f32 的 ULP：越大说明这个线高在浮点上越不可靠（速度快到丢精度）。
            let ulp = if h == 0. { 0. } else { f32::from_bits(h.to_bits() + 1) - h };
            let color = if ulp > 0.018518519 {
                RED
            } else if ulp > 0.0018518519 {
                YELLOW
            } else {
                WHITE
            };
            let kind = match &line.kind {
                JudgeLineKind::Normal => "",
                JudgeLineKind::Texture(..) => " img",
                JudgeLineKind::TextureGif(..) => " gif",
                JudgeLineKind::Text(..) => " text",
                JudgeLineKind::Paint(..) => " paint",
            };
            let attach = if line.attach_ui.is_some() { " +ui" } else { "" };
            ui.text(format!("[{id}] h:{h:.2} z:{}{attach}{kind}", line.z_index))
                .pos(pos.x, pos.y - 0.012)
                .anchor(0.5, 1.)
                .size(0.045)
                .color(color)
                .draw_using(&PGR_FONT);
        }
    }
    if res.config.chart_debug_note {
        let x_diff = crate::judge::X_DIFF_MAX as f32;
        let limits = res.windows;
        let tiers = [
            (limits.bad, Color::new(0.55, 0.55, 0.55, 1.)),
            (limits.good, Color::new(0.45, 0.72, 1., 1.)),
            (limits.perfect, res.res_pack.info.fx_perfect()),
            (limits.perfect_plus, Color::new(1., 0.62, 0.12, 1.)),
        ];
        for (id, line) in lines.iter().enumerate() {
            let tr = debug_screen_transform(res.config.flip_x()) * line.now_transform(res, lines);
            let line_height = line.height.now() as f64;
            let incline = line.incline.now_opt().unwrap_or_default().to_radians().sin();
            let mut ctrl = line.ctrl_obj.borrow().clone();
            // Sample the real scrolling track: speed events and time scaling
            // must affect the displayed timing bands, including late leniency.
            let spans = debug_height_spans_sided(&line.height, res.time, [3, 2, 1, 0].map(|i| limits.early[i]), [3, 2, 1, 0].map(|i| limits.late[i]), res.config.speed as f64, res.config.late_leniency());
            for note in &line.notes {
                if note.fake || matches!(note.judge, crate::judge::JudgeStatus::Judged) || note.object.now_alpha() == 0. {
                    continue;
                }
                let (note_tr, speed) = note.debug_transform(res, &mut ctrl, line_height, incline);
                let phigros = res.config.judge_algorithm == crate::config::JudgeAlgorithm::Phigros;
                let world_scale = crate::ext::get_viewport().2 as f64 / screen_height() as f64 * 5.;
                let rules = &res.config.phigros_rules;
                let special_window = if phigros {
                    match note.kind {
                        NoteKind::Drag => Some(rules.drag_sides().map(|v| v / 1000.)),
                        NoteKind::Flick => Some([limits.early[1] * rules.flick_sides()[0], limits.late[1] * rules.flick_sides()[1]]),
                        _ => None,
                    }
                } else { None };
                let note_tiers = special_window.map_or(tiers, |window| [
                    (window[0].max(window[1]), res.res_pack.info.fx_perfect()), (0., WHITE), (0., WHITE), (0., WHITE),
                ]);
                let note_spans = if phigros {
                    debug_height_spans_sided(&line.height, res.time, special_window.map_or([3, 2, 1, 0].map(|i| limits.early[i]), |w| [w[0], 0., 0., 0.]), special_window.map_or([3, 2, 1, 0].map(|i| limits.late[i]), |w| [w[1], 0., 0., 0.]), res.config.speed as f64, res.config.late_leniency())
                } else { spans };
                let side = Matrix::identity().append_nonuniform_scaling(&Vector::new(1., if note.above { 1. } else { -1. }));
                let mat = tr * side * note_tr;
                let pos = mat.transform_point(&Point::new(0., 0.));
                if !pos.coords.iter().all(|v| v.is_finite()) || pos.x.abs() > 1.3 || pos.y.abs() > 1.3 / res.aspect_ratio {
                    continue;
                }
                let half = if phigros {
                    (if matches!(note.kind, NoteKind::Drag | NoteKind::Flick) { rules.special_width } else { rules.tap_width }) as f32
                        / world_scale as f32 * note.judge_area
                } else { x_diff * note.judge_area };
                let sy = note_tr.transform_vector(&Vector::new(0., 1.)).norm();
                if !half.is_finite() || half <= 0. || !sy.is_finite() || sy <= f32::EPSILON {
                    continue;
                }
                ui.with(mat, |ui| {
                    // Outer-to-inner tints and outlines show four timing bands.
                    // Rectangles stay local so rotations cannot produce a
                    // negative width or an axis-aligned, stationary box.
                    for ((window, tint), (early, late)) in note_tiers.iter().zip(note_spans) {
                        if *window <= 0. { continue; }
                        let factor = speed / res.aspect_ratio as f64 / sy as f64;
                        let a = (early * factor) as f32;
                        let b = (late * factor) as f32;
                        if !a.is_finite() || !b.is_finite() {
                            continue;
                        }
                        let rect = Rect::new(-half, a.min(b), half * 2., (b - a).abs().max(0.001));
                        ui.fill_rect(rect, Color { a: 0.38, ..*tint });
                        ui.stroke_path(&rect.rounded(0.), 0.0015, Color { a: 0.9, ..*tint });
                    }
                });
                let label = if let Some(window) = special_window {
                    format!("[{id}] {:.2}s {} -{:.0}/+{:.0}ms", note.time, if matches!(note.kind, NoteKind::Drag) { "Drag" } else { "Flick" }, window[0] * 1000., window[1] * 1000.)
                } else { format!(
                    "[{id}] {:.2}s Perfect+ -{:.0}/+{:.0} P -{:.0}/+{:.0} G -{:.0}/+{:.0} B -{:.0}/+{:.0}ms",
                    note.time,
                    limits.early[0] * 1000., limits.late[0] * 1000.,
                    limits.early[1] * 1000., limits.late[1] * 1000.,
                    limits.early[2] * 1000., limits.late[2] * 1000.,
                    limits.early[3] * 1000., limits.late[3] * 1000.
                ) };
                ui.text(label)
                .pos(pos.x, pos.y - 0.012)
                .anchor(0.5, 1.)
                .size(0.04)
                .color(WHITE)
                .draw_using(&PGR_FONT);
            }
        }
    }
}

fn debug_screen_transform(flip_x: bool) -> Matrix {
    Matrix::identity().append_nonuniform_scaling(&Vector::new(if flip_x { -1. } else { 1. }, -1.))
}

#[cfg(test)]
fn debug_height_spans(height: &crate::core::AnimFloat, time: f64, windows: [f64; 4], speed: f64, late: f64) -> [(f64, f64); 4] {
    debug_height_spans_sided(height, time, windows, windows, speed, late)
}

fn debug_height_spans_sided(height: &crate::core::AnimFloat, time: f64, early: [f64; 4], late_windows: [f64; 4], speed: f64, late: f64) -> [(f64, f64); 4] {
    let mut sample = height.clone();
    sample.set_time(time);
    let center = sample.now() as f64;
    std::array::from_fn(|i| {
        let window = early[i];
        sample.set_time(time - window * speed);
        let early = sample.now() as f64 - center;
        sample.set_time(time + (late_windows[i] + late) * speed);
        (early, sample.now() as f64 - center)
    })
}

#[cfg(test)]
mod debug_overlay_tests {
    use super::*;
    use crate::core::{AnimFloat, Keyframe};

    #[test]
    fn timing_bands_follow_speed_events_and_do_not_mutate_chart() {
        let mut height = AnimFloat::new(vec![Keyframe::new(0., 0., 2), Keyframe::new(1., 2., 2), Keyframe::new(2., 8., 2)]);
        height.set_time(1.);
        let spans = debug_height_spans(&height, 1., [0.22, 0.16, 0.08, 0.016], 1.5, 0.03);
        assert!((spans[0].0 + 0.66).abs() < 1e-6);
        assert!((spans[0].1 - 2.25).abs() < 1e-6);
        assert!(spans[0].0 < spans[1].0 && spans[1].0 < spans[2].0 && spans[2].0 < spans[3].0);
        assert!(spans[0].1 > spans[1].1 && spans[1].1 > spans[2].1 && spans[2].1 > spans[3].1);
        assert_eq!(height.time, 1.);
        assert_eq!(height.now(), 2.);
    }

    #[test]
    fn note_debug_uses_chart_flip_below_side_and_rotation() {
        let rotated = nalgebra::Rotation2::new(std::f32::consts::FRAC_PI_2).to_homogeneous();
        let below = Matrix::identity().append_nonuniform_scaling(&Vector::new(1., -1.));
        let moving = Matrix::new_translation(&Vector::new(0.2, 0.4));
        let p = Point::origin();
        let above = (debug_screen_transform(false) * rotated * moving).transform_point(&p);
        let under = (debug_screen_transform(false) * rotated * below * moving).transform_point(&p);
        let mirrored = (debug_screen_transform(true) * rotated * moving).transform_point(&p);
        assert!((above.x + 0.4).abs() < 1e-6 && (above.y + 0.2).abs() < 1e-6);
        assert!((under.x - 0.4).abs() < 1e-6 && (under.y + 0.2).abs() < 1e-6);
        assert!((mirrored.x - 0.4).abs() < 1e-6);
    }
}

/// 连击数下方显示的文字。
///
/// 开启 AUTOPLAY 时**硬编码**为 `AUTOPLAY`：绝不允许被配置改写成 `combo` / `C0MB0`
/// 这类形似「手动游玩」的变体 —— 否则可以用自动演奏录出看起来像手元 / 屏元的视频。
/// 其余情况用配置里的自定义文字，留空则回退到 `COMBO`。
fn combo_label(config: &crate::config::Config) -> &str {
    if config.autoplay() {
        "AUTOPLAY"
    } else if !config.combo_text.is_empty() {
        config.combo_text.as_str()
    } else {
        "COMBO"
    }
}

const WAIT_TIME: f64 = 0.5;
const AFTER_TIME: f64 = 0.7;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SimpleRecord {
    pub score: i32,
    pub accuracy: f32,
    pub full_combo: bool,
    /// 最大连击（供本地成绩历史使用）。
    pub max_combo: u32,
    /// 总音符数（供本地成绩历史使用）。
    pub num_of_notes: u32,
    /// Perfect / Good / Bad / Miss（Perfect+ 已并入 Perfect）。
    pub counts: [u32; 4],
    pub grade_counts: Option<[u32; 8]>,
    pub grading: Option<crate::config::JudgeGrading>,
    /// 判定误差分布（早 ← → 晚）。
    pub hist: Vec<u32>,
    /// 本局有效命中的偏差标准差（秒），用于成绩详情页显示「无瑕度」。
    pub std: f32,
}

impl SimpleRecord {
    pub fn update(&mut self, other: &SimpleRecord) -> bool {
        let mut changed = false;
        if other.score > self.score {
            self.score = other.score;
            changed = true;
        }
        if other.accuracy > self.accuracy {
            self.accuracy = other.accuracy;
            changed = true;
        }
        if other.full_combo & !self.full_combo {
            self.full_combo = other.full_combo;
            changed = true;
        }
        changed
    }
}

/// 上传到 Phira Pro 私服（`api.phira.pro`）的一局成绩载荷。
///
/// 字段名与私服契约一致（camelCase）。`notes` 与 `noteCount` 目前都取本局音符总数，
/// 其中 `noteCount` 是服务端计算 `stdScore` 的分母。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadScore {
    pub chart: i32,
    pub perfect: i32,
    pub good: i32,
    pub bad: i32,
    pub miss: i32,
    pub max_combo: i32,
    pub mods: i32,
    pub notes: i32,
    pub speed: f32,
    pub std: f32,
    pub score: i32,
    pub accuracy: f32,
    pub full_combo: bool,
    pub note_count: i32,
}

fn fmt_time(t: f32) -> String {
    let f = t < 0.;
    let t = t.abs();
    let secs = t % 60.;
    let mut t = (t / 60.) as u64;
    let mins = t % 60;
    t /= 60;
    let hrs = t % 100;
    format!("{}{hrs:02}:{mins:02}:{secs:05.2}", if f { "-" } else { "" })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    fn on_game_start();
}

#[derive(PartialEq, Eq)]
pub enum GameMode {
    Normal,
    TweakOffset,
    Exercise,
    NoRetry,
    View,
}

#[derive(Clone)]
enum State {
    Starting,
    BeforeMusic,
    Playing,
    Ending,
}

pub struct GameScene {
    should_exit: bool,
    next_scene: Option<NextScene>,
    /// 失败（血条或即时死亡）发生的时刻，用于死亡过渡动画。
    death_time: f64,
    /// 本次游玩已经自动重试了几次。
    auto_retry_count: u32,
    /// 断点续练的起始时刻；由 Starting -> BeforeMusic 的转场消费。
    retry_from: Option<f64>,
    /// 变速练习：已经完成的圈数。
    practice_loops: u32,

    pub mode: GameMode,
    pub res: Resource,
    pub chart: Chart,
    pub judge: Judge,
    pub gl: InternalGlContext<'static>,
    player: Option<BasicPlayer>,
    chart_bytes: Vec<u8>,
    chart_format: ChartFormat,
    info_offset: f32,
    effects: Vec<Effect>,
    offset_analysis: OffsetAnalysisPanel,

    first_in: bool,
    entered: bool,
    rendered_first_frame: bool,
    exercise_range: Range<f64>,
    exercise_press: Option<(i8, u64)>,
    exercise_btns: (RectButton, RectButton),

    pub music: Music,

    state: State,
    pub last_update_time: f64,
    replay_frame_delta: Option<f32>,
    pause_rewind: Option<f64>,
    pause_first_time: f32,

    pub bad_notes: Vec<BadNote>,

    upload_fn: Option<UploadFn>,
    update_fn: Option<UpdateFn>,
    save_fn: Option<SaveFn>,

    best_record: Option<SimpleRecord>,

    pub touch_points: Vec<(f32, f32)>,
    block_audio: crate::core::BlockAudio,
    fps_frame_count: u32,
    fps_total_time: f64,
    fps_last_frame_time: f64,

    dead: bool,
}

macro_rules! reset {
    ($self:ident, $res:expr, $tm:ident) => {{
        $self.bad_notes.clear();
        $self.judge.reset();
        $self.chart.reset();
        $self.block_audio.reset();
        $res.judge_line_color = $res.res_pack.info.color_perfect();
        $self.music.pause()?;
        $self.music.seek_to(0.)?;
        $tm.speed = $res.config.speed as _;
        $tm.reset();
        $self.last_update_time = $tm.real_time();
        $self.state = State::Starting;
        $self.fps_frame_count = 0;
        $self.fps_total_time = 0.0;
        $self.fps_last_frame_time = $tm.real_time();
        $self.dead = false;
    }};
}

impl GameScene {
    pub const BEFORE_TIME: f64 = 0.7;
    pub const FADEOUT_TIME: f64 = WAIT_TIME + AFTER_TIME + 0.3;

    pub async fn load_chart_bytes(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<Vec<u8>> {
        if let Ok(bytes) = fs.load_file(&info.chart).await {
            return Ok(bytes);
        }
        if let Some(name) = info.chart.strip_suffix(".pec") {
            if let Ok(bytes) = fs.load_file(&concat_string!(name, ".json")).await {
                return Ok(bytes);
            }
        }
        bail!("Cannot find chart file")
    }

    pub fn infer_chart_format(info: &ChartInfo, bytes: &[u8]) -> ChartFormat {
        info.format.clone().unwrap_or_else(|| {
            // 只做无损探测：绝不对整份谱面做 `to_vec()` 复制（巨型谱面可达数百 MB）。
            if bytes.first() == Some(&b'{') {
                if bytes.windows(6).any(|w| w == b"\"META\"") {
                    ChartFormat::Rpe
                } else {
                    ChartFormat::Pgr
                }
            } else if std::str::from_utf8(bytes).is_ok() {
                ChartFormat::Pec
            } else {
                ChartFormat::Pbc
            }
        })
    }

    pub async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<(Chart, Vec<u8>, ChartFormat)> {
        let extra = fs.load_file("extra.json").await.ok().map(String::from_utf8).transpose()?;
        let extra = if let Some(extra) = extra {
            parse_extra(&extra, fs).await.context("Failed to parse extra")?
        } else {
            ChartExtra::default()
        };
        let mut bytes = Self::load_chart_bytes(fs, info).await.context("Failed to load chart")?;
        let format = Self::infer_chart_format(info, &bytes);
        let mut chart = match format {
            ChartFormat::Rpe => {
                parse_rpe_with_path(&String::from_utf8_lossy(&bytes), fs, extra, info.use_rpe_170_speed.unwrap_or_default(), Some(&info.chart)).await
            }
            ChartFormat::Pgr => {
                let (chart, owned) = crate::parse::parse_phigros_loading(std::mem::take(&mut bytes), extra).await?;
                bytes = owned;
                Ok(chart)
            }
            ChartFormat::Pec => {
                // 巨型 PEC 谱（可达数百 MB、物量百万级）逐行解析要 2~3 秒。放到后台线程解析，
                // 主线程只等待结果，加载界面在解析期间保持流畅，不再整帧冻结。
                // `extra` 可能含 effect 等非 Send 内容，留在主线程、拿到结果后再装配。
                let b = std::mem::take(&mut bytes);
                let (c, b) = spawn_task(move || -> Result<(SendChart, Vec<u8>)> {
                    let chart = {
                        let text = String::from_utf8_lossy(&b);
                        parse_pec(&text, ChartExtra::default())?
                    };
                    Ok((SendChart(chart), b))
                })
                .await?;
                bytes = b;
                let mut chart = c.0;
                chart.extra = extra;
                Ok(chart)
            }
            ChartFormat::Pbc => {
                let mut r = BinaryReader::new(Cursor::new(&bytes));
                r.read()
            }
        }?;
        chart.load_textures(fs).await?;
        chart.settings.hold_partial_cover = info.hold_partial_cover;
        Ok((chart, bytes, format))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        mode: GameMode,
        mut info: ChartInfo,
        mut config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        background: SafeTexture,
        illustration: SafeTexture,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
        save_fn: Option<SaveFn>,
    ) -> Result<Self> {
        if config.fixed_background {
            info.background_dim = config.background_dim.clamp(0., 1.);
        }
        match mode {
            GameMode::TweakOffset => {
                config.mods.insert(Mods::AUTOPLAY);
            }
            GameMode::Exercise => {
                config.mods.remove(Mods::AUTOPLAY);
            }
            _ => {}
        }

        tracing::info!("game loading: parse chart begins");
        let (mut chart, chart_bytes, chart_format) = Self::load_chart(fs.deref_mut(), &info).await?;
        tracing::info!("game loading: chart parsed; preparing shaders and resources");
        if chart.has_block_areas() && !config.block_area_simple {
            crate::core::prepare_block_effects();
        }
        if config.mods.contains(Mods::NO_SHADER) {
            chart.extra.effects.clear();
            chart.extra.global_effects.clear();
        }
        let mut effects = std::mem::take(&mut chart.extra.global_effects);
        if config.fxaa {
            chart
                .extra
                .effects
                .push(Effect::new(0.0..f64::INFINITY, include_str!("fxaa.glsl"), Vec::new(), false).unwrap());
        }

        if config.has_mod(Mods::NIGHTCORE) {
            config.speed *= 1.5;
        }

        if config.has_mod(Mods::RAINBOW) {
            chart
                .extra
                .effects
                .push(Effect::new(0.0..f64::INFINITY, include_str!("rainbow.glsl"), Vec::new(), false).unwrap());
        }

        let info_offset = info.offset;
        let mut res = Resource::new(
            config,
            info,
            fs,
            player.as_ref().and_then(|it| it.avatar.clone()),
            background,
            illustration,
            chart.extra.effects.is_empty() && effects.is_empty(),
        )
        .await
        .context("Failed to load resources")?;
        tracing::info!("game loading: resources ready, samples={}, shader_pre_render={}", res.config.sample_count, res.config.shader_pre_render);

        let eligible = Self::implicit_msaa_eligible(&chart, &res);
        res.chart_render_policy = crate::core::render_lifetime::Policy::continuous(eligible, res.config.sample_count);

        crate::core::clear_prepared_block_geometry();
        if res.config.shader_pre_render {
            let begun = std::time::Instant::now();
            unsafe { get_internal_gl() }.flush();
            res.update_size(crate::ext::get_viewport());
            if !res.config.block_area_simple && !chart.block_areas.is_empty() {
                let vp = res.camera.viewport.unwrap_or_else(crate::ext::get_viewport);
                crate::core::prepare_block_geometry(&chart.block_areas, vp.2.max(1) as usize, vp.3.max(1) as usize, res.aspect_ratio);
                next_frame().await;
            }
            if !res.no_effect {
                for effect in chart.extra.effects.iter_mut().chain(effects.iter_mut()) {
                    effect.prepare(*res.background);
                    next_frame().await;
                }
            }
            tracing::info!("shader loading preparation: {:.1} ms", begun.elapsed().as_secs_f64() * 1000.);
        }

        // Prepare extra sfx from chart.hitsounds
        chart.hitsounds.drain().for_each(|(name, clip)| {
            if let Ok(clip) = res.create_sfx(clip) {
                res.extra_sfxs.insert(name, clip);
            }
        });

        let exercise_range = (chart.offset + info_offset + res.config.offset) as f64..res.track_length;

        let mut judge = Judge::new(&chart, res.config.hold_tail_judge);
        judge.set_hp_amount(res.config.hp_amount);
        judge.set_hp_scale(res.config.hp_scale);
        judge.set_grading(res.config.judge_grading);

        let music = Self::new_music(&mut res)?;
        tracing::info!("game loading: audio ready");
        crate::core::reset_block_effects();
        Ok(Self {
            should_exit: false,
            next_scene: None,
            death_time: 0.,
            auto_retry_count: 0,
            retry_from: None,
            practice_loops: 0,

            mode,
            res,
            chart,
            judge,
            gl: unsafe { get_internal_gl() },
            player,
            chart_bytes,
            chart_format,
            effects,
            info_offset,

            offset_analysis: OffsetAnalysisPanel::new(),

            first_in: false,
            entered: false,
            rendered_first_frame: false,
            exercise_range,
            exercise_press: None,
            exercise_btns: (RectButton::new(), RectButton::new()),

            music,

            state: State::Starting,
            replay_frame_delta: None,
            last_update_time: 0.,
            pause_rewind: None,
            pause_first_time: f32::NEG_INFINITY,

            bad_notes: Vec::new(),

            upload_fn,
            update_fn,
            save_fn,

            best_record: None,

            touch_points: Vec::new(),
            block_audio: Default::default(),

            fps_frame_count: 0,
            fps_total_time: 0.0,
            fps_last_frame_time: 0.0,

            dead: false,
        })
    }

    fn new_music(res: &mut Resource) -> Result<Music> {
        res.audio.create_music(
            res.music.clone(),
            MusicParams {
                amplifier: res.config.music_amplifier(&res.music, res.config.volume_music),
                playback_rate: res.config.speed as _,
                ..Default::default()
            },
        )
    }

    fn touch_scale(&self) -> f32 {
        (screen_width() / screen_height()) / self.res.aspect_ratio
    }

    fn ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now();
        let p = match self.state {
            State::Starting => {
                if time <= Self::BEFORE_TIME {
                    1. - (1. - time / Self::BEFORE_TIME).powi(3)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Playing => 1.,
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                1. - (t / (AFTER_TIME + 0.3)).min(1.).powi(2)
            }
        } as f32;
        let res = &mut self.res;
        let eps = 2e-2 / res.aspect_ratio;
        let top = -1. / res.aspect_ratio;
        let pause_w = 0.015;
        let pause_h = pause_w * 3.2;
        let pause_center = Point::new(pause_w * 4.0 - 1., top + eps * 3.5 - (1. - p) * 0.4 + pause_h / 2.);
        if res.config.interactive
            && !tm.paused()
            && self.pause_rewind.is_none()
            && Judge::get_touches().iter().any(|touch| {
                touch.phase == TouchPhase::Started && {
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    (pause_center - p).norm() < 0.05
                }
            })
        {
            let t = tm.now() as f32;
            if t - self.pause_first_time > PAUSE_CLICK_INTERVAL && res.config.double_click_to_pause {
                self.pause_first_time = t;
            } else {
                self.pause_first_time = f32::NEG_INFINITY;
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
                #[cfg(target_env = "ohos")]
                miniquad::native::set_interceptor_state(false);
            }
        }
        ui.alpha(res.alpha, |ui| {
            ui.text("MAGIC BUGFIX TEXT").color(Color::new(0., 0., 0., 0.)).draw();
            if tm.now() as f32 - self.pause_first_time <= PAUSE_CLICK_INTERVAL {
                ui.fill_circle(pause_center.x, pause_center.y, 0.05, Color::new(1., 1., 1., 0.5));
            }

            let margin = 0.03;

            let legacy_aui = !res.info.use_attach_ui_fix.unwrap_or_default();
            let unit_h = if legacy_aui { ui.text("0").measure_using(&PGR_FONT).h } else { 0. };

            // score
            let h = 0.07;
            let score_top = top + eps * 2.2 - (1. - p) * 0.4;
            let score_right = 1. - margin;
            let score = format!(
                "{:07}",
                self.judge
                    .displayed_score(res.config.has_mod(Mods::NO_COMBO_SCORE), res.config.theoretical_score)
            );
            let scale_point = legacy_aui.then(|| {
                let ct = ui.text(&score).size(0.8).measure_using(&PGR_FONT).center();
                (score_right - ct.x, score_top + ct.y)
            });
            self.chart
                .with_element(ui, res, UIElement::Score, scale_point, (score_right, score_top), |ui, c| {
                    ui.text(&score)
                        .pos(score_right, score_top)
                        .anchor(1., 0.)
                        .size(0.8)
                        .color(c)
                        .draw_using(&PGR_FONT);
                    if res.config.show_acc {
                        ui.text(format!("{:05.2}%", self.judge.real_time_accuracy() * 100.))
                            .pos(1. - margin, score_top + h)
                            .anchor(1., 0.)
                            .size(0.4)
                            .color(Color { a: c.a * 0.7, ..c })
                            .draw_using(&PGR_FONT);
                    }
                });

            self.chart.with_element(
                ui,
                res,
                UIElement::Pause,
                legacy_aui.then(|| (pause_center.x, pause_center.y)),
                (pause_center.x - pause_w * 1.5, pause_center.y - pause_h / 2.),
                |ui, c| {
                    crate::ui::draw_pause_icon(ui, Rect::new(pause_center.x - pause_w * 1.5, pause_center.y - pause_h / 2., pause_w * 3., pause_h), c);
                },
            );
            if self.judge.combo() >= 3 {
                if legacy_aui {
                    let combo_top = top + eps * 2. - (1. - p) * 0.4;
                    let btm = self
                        .chart
                        .with_element(ui, res, UIElement::ComboNumber, None, (0., combo_top + unit_h / 2.), |ui, c| {
                            ui.text(self.judge.combo().to_string())
                                .pos(0., combo_top)
                                .anchor(0.5, 0.)
                                .color(c)
                                .draw_using(&PGR_FONT)
                                .bottom()
                        });
                    let combo_top = btm + 0.01;
                    self.chart
                        .with_element(ui, res, UIElement::Combo, None, (0., combo_top + unit_h * 0.2), |ui, c| {
                            ui.text(combo_label(&res.config))
                                .pos(0., combo_top)
                                .anchor(0.5, 0.)
                                .size(0.4)
                                .color(c)
                                .draw_using(&PGR_FONT);
                        });
                } else {
                    let combo = self.judge.combo().to_string();
                    let ct = ui.text(&combo).size(1.0).measure().center();
                    let combo_y = top + eps * 2. - (1. - p) * 0.4 + ct.y;
                    let btm = self.chart.with_element(ui, res, UIElement::ComboNumber, None, (0., combo_y), |ui, c| {
                        ui.text(&combo)
                            .pos(0., combo_y)
                            .anchor(0.5, 0.5)
                            .size(1.0)
                            .color(c)
                            .draw_using(&PGR_FONT)
                            .bottom()
                    });
                    let ct = ui.text("COMBO").size(0.4).measure().center();
                    let combo_top = btm + 0.01 + ct.y;
                    self.chart.with_element(ui, res, UIElement::Combo, None, (0., combo_top), |ui, c| {
                        ui.text(combo_label(&res.config))
                            .pos(0., combo_top)
                            .anchor(0.5, 0.5)
                            .size(0.4)
                            .color(c)
                            .draw_using(&PGR_FONT);
                    });
                }
            }
            // 判定条独立于 combo 是否显示；位置取屏高的 1/6 处（`-top` 即 ui 空间里的半个屏高）。
            // 可在 设置 → 谱面 里关掉（`item-offset-indicator`）。
            if res.config.offset_indicator {
                draw_offset_indicator(ui, res, self.judge.recent_hits(), top + (-top) / 3.);
            }
            // 血条模式：与暂停按钮同高、紧贴其右侧；长度由配置控制，高度是相对暂停按钮的倍率。
            if res.config.hp_mode {
                let h = pause_h * res.config.hp_height;
                let w = res.config.hp_width;
                let r = Rect::new(pause_center.x + pause_w * 1.5 + 0.04, pause_center.y - h / 2., w, h);
                ui.fill_rect(r, Color::new(0., 0., 0., 0.35));
                let (cr, cg, cb) = res.config.hp_color.rgb();
                ui.fill_rect(Rect::new(r.x, r.y, r.w * self.judge.hp().clamp(0., 1.), r.h), Color::new(cr, cg, cb, 0.9));
            }
            // 谱面调试叠加层（判定线 / 音符的编号、线高、时间、横向判定范围）。
            if res.config.chart_debug_line || res.config.chart_debug_note {
                debug_overlay(res, &self.chart, ui);
            }
            // magic to make score visible, refer to phira/src/rate.rs#L219
            ui.text("").draw_using(&PGR_FONT);
            let lf = -1. + margin;
            let bt = -top - eps * 2.8 + (1. - p) * 0.4;
            let scale_point = legacy_aui.then(|| {
                let ct = ui.text(&res.info.name).size(0.5).measure().center();
                (lf + ct.x, bt - ct.y)
            });
            self.chart.with_element(ui, res, UIElement::Name, scale_point, (lf, bt), |ui, c| {
                ui.text(&res.info.name)
                    .pos(lf, bt)
                    .anchor(0., 1.)
                    .size(0.5)
                    .color(c)
                    .max_width(0.8)
                    .draw();
            });

            let scale_point = legacy_aui.then(|| {
                let ct = ui.text(&res.info.level).size(0.5).measure().center();
                (-lf - ct.x, bt - ct.y)
            });
            self.chart.with_element(ui, res, UIElement::Level, scale_point, (-lf, bt), |ui, c| {
                ui.text(&res.info.level).pos(-lf, bt).anchor(1., 1.).size(0.5).color(c).draw();
            });

            let hw = 0.003;
            let height = eps * 1.0;
            let dest = (2. * res.time / res.track_length).clamp(0., 2.) as f32;
            self.chart
                .with_element(ui, res, UIElement::Bar, Some((-1., top + height / 2.)), (-1., top + height / 2.), |ui, color| {
                    ui.fill_rect(Rect::new(-1., top, dest, height), semi_white(0.6));
                    ui.fill_rect(Rect::new(-1. + dest - hw, top, hw * 2., height), WHITE);
                });
        });
        Ok(())
    }

    fn debug_overlay(&self, ui: &mut Ui) {
        debug_overlay(&self.res, &self.chart, ui);
    }

    fn overlay_ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        // 失败时，遮罩与按钮在减速过渡结束后渐显；普通暂停保持原样。
        let fade = if self.dead {
            (((tm.real_time() - self.death_time) - DEATH_TIME) / DEATH_FADE).clamp(0., 1.) as f32
        } else {
            1.
        };
        let c = semi_white(self.res.alpha * fade);
        let res = &mut self.res;
        if tm.paused() {
            let h = 1. / res.aspect_ratio;
            draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., 0.6 * fade));
            let o = if self.mode == GameMode::Exercise { -0.3 } else { 0. };
            let s = 0.06;
            let w = 0.05;
            let no_retry = self.mode == GameMode::NoRetry;
            draw_texture_ex(
                *res.icon_back,
                -s * 3. - w,
                -s + o,
                c,
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            let r = Rect::new(0., o, 0., 0.).feather(s);
            let disabled_color = semi_white(res.alpha * 0.4 * fade);
            ui.fill_rect(r, (*res.icon_retry, r.feather(0.02), ScaleType::Fit, if no_retry { disabled_color } else { c }));
            draw_texture_ex(
                *res.icon_resume,
                s + w,
                -s + o,
                if self.dead { disabled_color } else { c },
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            if res.config.interactive {
                let mut clicked = None;
                for touch in Judge::get_touches() {
                    if touch.phase != TouchPhase::Started {
                        continue;
                    }
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    for i in -1..=1 {
                        let ct = Point::new((s * 2. + w) * i as f32, o);
                        let d = p - ct;
                        if d.x.abs() <= s && d.y.abs() <= s {
                            clicked = Some(i);
                            break;
                        }
                    }
                }
                if no_retry && clicked == Some(0) || self.dead && clicked == Some(1) {
                    clicked = None;
                }
                let mut pos = self.music.position();
                if self.mode == GameMode::Exercise {
                    pos = tm.now();
                }
                if clicked.is_some_and(|it| it != -1) && (tm.speed - res.config.speed as f64).abs() > 0.01 {
                    debug!("recreating music");
                    self.music = res.audio.create_music(
                        res.music.clone(),
                        MusicParams {
                            amplifier: res.config.music_amplifier(&res.music, res.config.volume_music),
                            playback_rate: res.config.speed as _,
                            ..Default::default()
                        },
                    )?;
                    self.block_audio.reset();
                }
                match clicked {
                    Some(-1) => {
                        self.should_exit = true;
                        #[cfg(target_env = "ohos")]
                        miniquad::native::set_interceptor_state(false);
                    }
                    Some(0) => {
                        self.auto_retry_count = 0;
                        self.practice_loops = 0;
                        reset!(self, res, tm);
                        if self.mode == GameMode::Exercise {
                            self.judge.advance_to(&mut self.chart, self.exercise_range.start);
                        }
                        #[cfg(target_env = "ohos")]
                        miniquad::native::set_interceptor_state(true);
                    }
                    Some(1) => {
                        if self.mode == GameMode::Exercise && (tm.now() > self.exercise_range.end || tm.now() < self.exercise_range.start) {
                            tm.seek_to(self.exercise_range.start);
                            self.music.seek_to(self.exercise_range.start)?;
                            pos = self.exercise_range.start;
                        }
                        self.music.play()?;
                        res.time -= 3.;
                        let dst = pos - 3.;
                        if dst < 0. {
                            self.music.pause()?;
                            self.state = State::BeforeMusic;
                        } else {
                            self.music.seek_to(dst)?;
                        }
                        let now = tm.now();
                        tm.speed = res.config.speed as _;
                        tm.resume();
                        tm.seek_to(now - 3.);
                        self.pause_rewind = Some(tm.now() - 0.2);
                        #[cfg(target_env = "ohos")]
                        miniquad::native::set_interceptor_state(true);
                    }
                    _ => {}
                }
            }
            if self.mode == GameMode::Exercise {
                let asp = self.touch_scale();
                for touch in ui.ensure_touches() {
                    touch.position *= asp;
                }
                ui.scope(|ui| {
                    ui.dx(0.3);
                    ui.dy(-0.3);
                    ui.slider(tl!("speed"), 0.5..2.0, 0.05, &mut self.res.config.speed, Some(0.5));
                });
                ui.dy(0.06);
                let hw = 0.7;
                let h = 0.06;
                let eh = 0.12;
                let rad = 0.03;
                let sp = self.offset().min(0.) as f64;
                ui.fill_rect(Rect::new(-hw, -h, hw * 2., h * 2.), GRAY);
                let st = -hw + ((self.exercise_range.start - sp) / (self.res.track_length - sp)) as f32 * hw * 2.;
                let en = -hw + ((self.exercise_range.end - sp) / (self.res.track_length - sp)) as f32 * hw * 2.;
                let t = tm.now();
                let cur = -hw + ((t - sp) / (self.res.track_length - sp)) as f32 * hw * 2.;
                ui.fill_rect(Rect::new(st, -h, en - st, h * 2.), WHITE);
                ui.fill_rect(Rect::new(st, -eh, 0., eh + h).feather(0.005), BLUE);
                ui.fill_circle(st, -eh, rad, BLUE);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(st, -eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (-1, it.id));
                }
                ui.fill_rect(Rect::new(en, -h, 0., eh + h).feather(0.005), RED);
                ui.fill_circle(en, eh, rad, RED);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(en, eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (1, it.id));
                }
                ui.fill_rect(Rect::new(cur, -h, 0., h * 2.).feather(0.005), GREEN);
                ui.fill_circle(cur, 0., rad, GREEN);
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(cur, 0., 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (0, it.id));
                }
                ui.text(fmt_time(t as f32)).pos(0., -0.23).anchor(0.5, 0.).size(0.8).draw();
                if let Some((ctrl, id)) = &self.exercise_press {
                    if let Some(touch) = Judge::get_touches().iter().rfind(|it| it.id == *id) {
                        let x = touch.position.x;
                        let p = (x + hw) as f64 / (hw * 2.) as f64 * (self.res.track_length - sp) + sp;
                        let p = if self.res.track_length - sp <= 3. || *ctrl == 0 {
                            p.clamp(sp, self.res.track_length)
                        } else {
                            p.clamp(
                                if *ctrl == -1 { sp } else { self.exercise_range.start + 3. },
                                if *ctrl == -1 {
                                    self.exercise_range.end - 3.
                                } else {
                                    self.res.track_length
                                },
                            )
                        };
                        if *ctrl == 0 {
                            tm.seek_to(p);
                            self.music.seek_to(p)?;
                            self.bad_notes.clear();
                            self.judge.reset();
                            self.chart.reset();
                            self.res.judge_line_color = self.res.res_pack.info.color_perfect();
                        } else {
                            *(if *ctrl == -1 {
                                &mut self.exercise_range.start
                            } else {
                                &mut self.exercise_range.end
                            }) = p;
                        }
                        if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                            self.exercise_press = None;
                        }
                    }
                }
                ui.dy(0.2);
                let r = ui.text(tl!("to")).size(0.8).anchor(0.5, 0.).draw();
                let mut tx = ui
                    .text(fmt_time(self.exercise_range.start as f32))
                    .pos(r.x - 0.02, 0.)
                    .anchor(1., 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.0.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.0.touching() { 0.5 } else { 1. }));
                tx.draw();

                let mut tx = ui
                    .text(fmt_time(self.exercise_range.end as f32))
                    .pos(r.right() + 0.02, 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.1.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.1.touching() { 0.5 } else { 1. }));
                tx.draw();
                for touch in ui.ensure_touches() {
                    touch.position /= asp;
                }
            }
        }
        if let Some(time) = self.pause_rewind {
            let dt = tm.now() - time;
            let t = 3 - dt.floor() as i32;
            if t <= 0 {
                self.pause_rewind = None;
            } else {
                let a = (1. - dt as f32 / 3.) * 1.;
                let h = 1. / self.res.aspect_ratio;
                draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., a));
                ui.text(t.to_string()).anchor(0.5, 0.5).size(1.).color(c).draw();
            }
        }
        let touch_size = self.res.config.touch_point_size;
        let touch_alpha = self.res.config.touch_point_alpha;
        if self.res.config.touch_debug {
            // 颜色可调；透明度与半径和回放触点共用同一组配置。
            let color = Color {
                a: touch_alpha,
                ..Color::from_hex_rgb(self.res.config.touch_point_color)
            };
            for touch in Judge::get_touches() {
                ui.fill_circle(touch.position.x, touch.position.y, touch_size, color);
            }
        }
        for pos in &self.touch_points {
            ui.fill_circle(pos.0, pos.1, touch_size, Color { a: touch_alpha, ..BLUE });
        }
        Ok(())
    }

    fn interactive(res: &Resource, state: &State) -> bool {
        res.config.interactive && matches!(state, State::Playing)
    }

    pub fn offset(&self) -> f32 {
        self.chart.offset + self.res.config.offset + self.info_offset
    }

    /// 回放 / 观战专用：按调用方已设置好的 `res.time` 刷新谱面动画、特效与判定线颜色。
    ///
    /// 不推进状态机、不处理输入判定（判定由回放器自行 `judge.commit`）。
    /// 曲名/分数等 HUD 由 `render` 内的 `ui()` 正常绘制，此处只补状态机之外的那部分。
    pub fn tick_replay(&mut self) {
        let counts = self.judge.counts();
        self.res.judge_line_color = if counts[2] + counts[3] == 0 && self.res.config.ap_fc_indicator {
            if counts[1] + counts[5] + counts[6] + counts[7] == 0 {
                self.res.res_pack.info.color_perfect()
            } else {
                self.res.res_pack.info.color_good()
            }
        } else {
            WHITE
        };
        self.res.judge_line_color.a *= self.res.alpha;
        self.chart.update(&mut self.res);
        for e in &mut self.effects {
            e.update(&self.res);
        }
    }

    /// Replay transport can freeze effects while paused or after a discontinuous seek.
    pub fn set_replay_frame_delta(&mut self, delta: f32) {
        self.replay_frame_delta = Some(delta.max(0.));
    }

    fn tweak_offset(&mut self, ui: &mut Ui, ita: bool) {
        let labels = OffsetPanelLabels {
            adjust_offset: tl!("adjust-offset"),
            auto_offset: tl!("auto-offset-btn"),
            analysis_prompt: tl!("analysis-prompt"),
            analysis_computing: tl!("analysis-computing"),
            cancel: tl!("offset-cancel"),
            reset: tl!("offset-reset"),
            save: tl!("offset-save"),
        };
        match self.offset_analysis.render(ui, &self.chart, &mut self.info_offset, ita, &labels) {
            Some(OffsetPanelAction::Cancel) => self.next_scene = Some(NextScene::PopWithResult(Box::new(None::<f32>))),
            Some(OffsetPanelAction::Reset) => self.info_offset = 0.,
            Some(OffsetPanelAction::Save(offset)) => self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(offset)))),
            None => {}
        }
    }
    pub fn get_avg_fps(&self) -> Option<f32> {
        if self.fps_frame_count > 0 && self.fps_total_time > 0.0 {
            Some(self.fps_frame_count as f32 / self.fps_total_time as f32)
        } else {
            None
        }
    }

    fn implicit_msaa_eligible(chart: &Chart, res: &Resource) -> bool {
        // Disabled blocks read only masks/noise; their scene snapshot is in the
        // active overlay after final resolve. Glyph/Paint/video may observe or
        // switch the target mid-chart; debug labels also upload glyphs. Paired
        // outputs follow the final resolve and swap as verified input/output
        // pairs, with an explicit-MSAA fallback if either allocation fails.
        #[cfg(feature = "video")]
        if !chart.extra.videos.is_empty() {
            return false;
        }
        !res.config.chart_debug
            && chart
                .lines
                .iter()
                .all(|line| matches!(line.kind, JudgeLineKind::Normal | JudgeLineKind::Texture(..) | JudgeLineKind::TextureGif(..)))
    }
}

impl Scene for GameScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        tracing::info!("game entry begins");
        #[cfg(target_arch = "wasm32")]
        on_game_start();
        #[cfg(target_env = "ohos")]
        miniquad::native::set_interceptor_state(true);
        // Loading already created the first renderer; recreate on re-entry.
        if self.entered {
            self.music = Self::new_music(&mut self.res)?;
        }
        self.res.camera.render_target = target;
        tm.speed = self.res.config.speed as _;
        tm.adjust_time = self.res.config.adjust_time;
        reset!(self, self.res, tm);
        set_camera(&self.res.camera);
        self.first_in = true;
        self.entered = true;
        self.rendered_first_frame = false;
        tracing::info!("game entry complete");
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        if !tm.paused() {
            self.pause_rewind = None;
            self.music.pause()?;
            tm.pause();
            self.chart.blocked_touches.clear();
            self.block_audio.suspend(&mut self.music);
        }
        #[cfg(target_env = "ohos")]
        miniquad::native::set_interceptor_state(false);
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.last_update_time = tm.real_time();
        self.fps_last_frame_time = tm.real_time();
        if !matches!(self.state, State::Playing) {
            tm.resume();
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.offset_analysis
            .update(&self.chart, &self.res, self.info_offset, tm.real_time() as f32);

        self.res.audio.recover_if_needed()?;
        if matches!(self.state, State::Playing) {
            tm.update(self.music.position());
        }
        if self.mode == GameMode::Exercise && tm.now() > self.exercise_range.end && !tm.paused() {
            let state = self.state.clone();
            reset!(self, self.res, tm);
            self.state = state;
            // 变速练习：每完成一圈提升一档速度并封顶在正常速度；谱面与音频同步变速。
            if self.res.config.practice_ramp {
                self.practice_loops += 1;
                let cfg = &self.res.config;
                let speed = (cfg.practice_speed_start + cfg.practice_speed_step * self.practice_loops as f32).clamp(0.1, cfg.speed.max(0.1));
                tm.speed = speed as f64;
                self.music.try_set_playback_rate(speed as f64);
            }
            tm.seek_to(self.exercise_range.start);
            tm.pause();
            self.music.pause()?;
            #[cfg(target_env = "ohos")]
            miniquad::native::set_interceptor_state(false);
        }
        let offset = self.offset();
        let time = tm.now();
        let time = match self.state {
            State::Starting => {
                if time >= Self::BEFORE_TIME {
                    self.res.alpha = 1.;
                    self.state = State::BeforeMusic;
                    tm.reset();
                    let mut start = if self.mode == GameMode::Exercise {
                        self.exercise_range.start
                    } else {
                        offset.min(0.) as f64
                    };
                    let retry = self.retry_from.take();
                    if let Some(target) = retry {
                        start = target;
                    }
                    tm.seek_to(start);
                    if retry.is_some() {
                        // 断点续练：判定只能向前跳，这里正好是从头快进到目标时刻。
                        self.music.seek_to(start)?;
                        self.judge.advance_to(&mut self.chart, start);
                    }
                    self.last_update_time = tm.real_time();
                    if self.first_in && self.mode == GameMode::Exercise {
                        tm.pause();
                        self.first_in = false;
                    }
                    tm.now()
                } else {
                    #[cfg(target_os = "windows")]
                    {
                        // wtf bro. why must particles exist on Windows?
                        let emitter_config = self.res.emitter.emitter.config.clone();
                        let emitter_square_config = self.res.emitter.emitter_square.config.clone();
                        self.res.emitter.emitter.config.size = 0.0;
                        self.res.emitter.emitter_square.config.size = 0.0;
                        self.res.emitter.emitter.emit(vec2(0.0, 0.0), 1);
                        self.res.emitter.emitter_square.emit(vec2(0.0, 0.0), 1);
                        self.res.emitter.emitter.config = emitter_config;
                        self.res.emitter.emitter_square.config = emitter_square_config;
                    }
                    self.res.alpha = (1. - (1. - time / Self::BEFORE_TIME).powi(3)) as f32;
                    if self.mode == GameMode::Exercise {
                        self.exercise_range.start
                    } else {
                        offset as f64
                    }
                }
            }
            State::BeforeMusic => {
                if time >= 0.0 {
                    self.music.seek_to(time)?;
                    if !tm.paused() {
                        self.music.play()?;
                    }
                    self.state = State::Playing;
                }
                time
            }
            State::Playing => {
                if time > self.res.track_length + WAIT_TIME {
                    self.state = State::Ending;
                    #[cfg(target_env = "ohos")]
                    miniquad::native::set_interceptor_state(false);
                }
                time
            }
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                if t >= AFTER_TIME + 0.3 {
                    let result = self.judge.result(self.res.config.has_mod(Mods::NO_COMBO_SCORE));
                    // 成绩上传（固定到 Phira Pro 私服）：只有「未改动判定 / 玩法」的对局
                    // （`is_official_play`）才构造可上传的成绩；其余对局只存本机。
                    let record_data = if self.upload_fn.is_some() && self.res.config.is_official_play(self.res.config.mods) {
                        self.res.info.id.map(|chart| UploadScore {
                            chart,
                            perfect: (result.counts[0] + result.counts[4]) as i32,
                            good: result.counts[1] as i32,
                            bad: result.counts[2] as i32,
                            miss: result.counts[3] as i32,
                            max_combo: result.max_combo as i32,
                            mods: self.res.config.mods.bits(),
                            notes: result.num_of_notes as i32,
                            speed: self.res.config.speed,
                            std: result.std,
                            score: result.score as i32,
                            accuracy: result.accuracy as f32,
                            full_combo: result.max_combo == result.num_of_notes,
                            note_count: result.num_of_notes as i32,
                        })
                    } else {
                        None
                    };
                    let record = if self.res.config.mods.intersects(Mods::UNRATED) || self.res.config.speed < 1.0 - 1e-3 {
                        None
                    } else {
                        Some(SimpleRecord {
                            score: result.score as _,
                            accuracy: result.accuracy as _,
                            full_combo: result.max_combo == result.num_of_notes,
                            max_combo: result.max_combo,
                            num_of_notes: result.num_of_notes,
                            // Perfect+ 并入 Perfect，让历史记录保持官方那套 4 档。
                            counts: [result.counts[0] + result.counts[4], result.counts[1], result.counts[2], result.counts[3]],
                            grade_counts: Some(result.counts),
                            grading: Some(result.grading),
                            hist: result.hist.to_vec(),
                            std: result.std,
                        })
                    };
                    self.next_scene = match self.mode {
                        GameMode::Normal | GameMode::NoRetry | GameMode::View => {
                            let historic_best = self.player.as_ref().map_or(0, |it| it.historic_best);
                            if let Some(new_rec) = &record {
                                if let Some(f) = &self.save_fn {
                                    f(new_rec.clone())?;
                                }
                                if let Some(best) = &mut self.best_record {
                                    if !new_rec.grading.is_some_and(|g| g.detailed) { best.update(new_rec); }
                                } else if !new_rec.grading.is_some_and(|g| g.detailed) {
                                    self.best_record = record.clone();
                                }
                                if let Some(best) = &self.best_record {
                                    if let Some(player) = &mut self.player {
                                        player.historic_best = player.historic_best.max(best.score as _);
                                    }
                                }
                            }
                            Some(NextScene::Overlay(Box::new(EndingScene::new(
                                self.res.background.clone(),
                                self.res.illustration.clone(),
                                self.res.player.clone(),
                                self.res.icons.clone(),
                                self.res.icon_retry.clone(),
                                self.res.icon_proceed.clone(),
                                self.res.mod_icons.clone(),
                                self.res.info.clone(),
                                self.judge.result(self.res.config.has_mod(Mods::NO_COMBO_SCORE)),
                                &self.res.config,
                                self.res.res_pack.ending.clone(),
                                self.upload_fn.as_ref().map(Arc::clone),
                                self.player.as_ref().map(|it| it.rks),
                                historic_best,
                                record_data,
                                self.best_record.clone(),
                                if self.res.config.show_avg_fps { self.get_avg_fps() } else { None },
                            )?)))
                        }
                        GameMode::TweakOffset => Some(NextScene::PopWithResult(Box::new(None::<f32>))),
                        GameMode::Exercise => None,
                    };
                }
                self.res.alpha = (1. - (t / AFTER_TIME).min(1.).powi(2)) as f32;
                self.res.track_length
            }
        };
        let time = (time - offset as f64).max(0.);
        self.res.time = time;
        if !tm.paused() && self.pause_rewind.is_none() && self.mode != GameMode::View {
            self.gl.quad_gl.viewport(self.res.camera.viewport);
            self.judge.update(&mut self.res, &mut self.chart, &mut self.bad_notes);
            self.gl.quad_gl.viewport(None);
        } else {
            self.judge.observe_paused_input();
            // Pause hides hover and suspends the filter, while release events
            // still end the infected finger's lifetime.
            self.chart.blocked_touches.clear();
        }
        if tm.paused() || self.pause_rewind.is_some() {
            self.block_audio.suspend(&mut self.music);
        } else {
            self.block_audio.sync(
                &mut self.music,
                !self.res.config.block_area_simple && matches!(self.state, State::Playing) && !self.chart.blocked_touches.is_empty(),
            );
        }
        if let Some(update) = &mut self.update_fn {
            update(self.res.time, &mut self.res, &mut self.judge);
        }
        let counts = self.judge.counts();
        self.res.judge_line_color = if counts[2] + counts[3] == 0 && self.res.config.ap_fc_indicator {
            if counts[1] + counts[5] + counts[6] + counts[7] == 0 {
                self.res.res_pack.info.color_perfect()
            } else {
                self.res.res_pack.info.color_good()
            }
        } else {
            WHITE
        };
        if !self.dead
            && matches!(self.state, State::Playing)
            && !self.res.config.mods.contains(Mods::NO_FAIL)
            && (self.res.config.mods.contains(Mods::INSTANT_DEATH_AP) && counts[1] + counts[2] + counts[3] + counts[5] + counts[6] + counts[7] > 0
                || self.res.config.mods.contains(Mods::INSTANT_DEATH_FC) && counts[2] + counts[3] > 0
                || self.res.config.hp_mode && self.judge.hp() <= 0.)
        {
            // 这里不立刻冻结：交给下面的死亡过渡，让谱面减速后再停下。
            self.dead = true;
            self.death_time = tm.real_time();
            #[cfg(target_env = "ohos")]
            miniquad::native::set_interceptor_state(false);
            show_message(tl!("game-over")).error();
        }
        if self.dead {
            // 死亡过渡：谱面与音乐同步减速；结束后冻结，失败 UI 由 overlay_ui 渐显。
            let p = ((tm.real_time() - self.death_time) / DEATH_TIME).clamp(0., 1.);
            if p < 1. {
                let factor = (1. - p).powi(2);
                let speed = self.res.config.speed as f64 * factor;
                if (tm.speed - speed).abs() > 1e-6 {
                    // TimeManager 的时间是「真实流逝时间 × speed」，所以改速度必须重锚
                    // start_time，否则整条时间轴会被重新缩放，表现为跳回开头。
                    let now = tm.now();
                    tm.speed = speed;
                    tm.seek_to(now);
                }
                self.music.try_set_playback_rate(speed.max(0.01));
            } else if !tm.paused() {
                self.music.pause()?;
                tm.pause();
            } else if self.res.config.auto_retry as u32 > self.auto_retry_count && tm.real_time() - self.death_time >= DEATH_TIME + DEATH_FADE {
                // 自动重试：沿用与手动重试完全相同的一条路径（reset! 会重置判定、时间与音乐）。
                // 续练的起始时刻交给 Starting -> BeforeMusic 的转场统一处理——那里才是真正设定
                // 起始时间的地方，在这里改会被随后的 tm.reset() 覆盖。
                let target = (tm.now() - self.res.config.retry_lead as f64).max(0.);
                self.auto_retry_count += 1;
                self.retry_from = (target > 0.).then_some(target);
                let res = &mut self.res;
                reset!(self, res, tm);
            }
        }
        self.res.judge_line_color.a *= self.res.alpha;
        self.chart.update(&mut self.res);
        let res = &mut self.res;
        if res.config.interactive && is_key_pressed(KeyCode::Space) {
            if tm.paused() {
                if matches!(self.state, State::Playing) {
                    self.music.play()?;
                    tm.resume();
                }
            } else if matches!(self.state, State::Playing | State::BeforeMusic) {
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
            }
        }
        if Self::interactive(res, &self.state) {
            if is_key_pressed(KeyCode::Left) && res.config.use_keyboard {
                res.time -= 1.;
                let dst = (self.music.position() - 1.).max(0.);
                self.music.seek_to(dst)?;
                tm.seek_to(dst);
            }
            if is_key_pressed(KeyCode::Right) && res.config.use_keyboard {
                res.time += 5.;
                let dst = (self.music.position() + 5.).min(res.track_length);
                self.music.seek_to(dst)?;
                tm.seek_to(dst);
            }
            if is_key_pressed(KeyCode::Q) {
                self.should_exit = true;
            }
        }
        for e in &mut self.effects {
            e.update(&self.res);
        }
        if let Some((id, text)) = take_input() {
            let offset = self.offset().min(0.);
            match id.as_str() {
                "exercise_start" => {
                    if let Some(t) = parse_time(&text) {
                        if !(offset as f64..self.res.track_length.min(self.exercise_range.end - 3.).max(offset as f64)).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.start = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                "exercise_end" => {
                    if let Some(t) = parse_time(&text) {
                        if !((self.exercise_range.start + 3.).max(offset as f64).min(self.res.track_length)..self.res.track_length).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.end = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                _ => return_input(id, text),
            }
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.mode == GameMode::TweakOffset {
            self.offset_analysis.touch(touch, tm.real_time() as f32);
        }
        if self.mode == GameMode::Exercise && tm.paused() {
            let touch = Touch {
                position: touch.position * self.touch_scale(),
                ..touch.clone()
            };
            if self.exercise_btns.0.touch(&touch) {
                request_input("exercise_start", InputBox::new().default_text(fmt_time(self.exercise_range.start as f32)));
                return Ok(true);
            }
            if self.exercise_btns.1.touch(&touch) {
                request_input("exercise_end", InputBox::new().default_text(fmt_time(self.exercise_range.end as f32)));
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        if !self.rendered_first_frame {
            tracing::info!("game first frame begins");
        }
        if self.res.config.show_avg_fps {
            let current_time = tm.real_time();
            if matches!(self.state, State::Playing) && !tm.paused() {
                let frame_delta = current_time - self.fps_last_frame_time;
                self.fps_total_time += frame_delta;
                self.fps_frame_count += 1;
            }
            self.fps_last_frame_time = current_time;
        }

        let res = &mut self.res;
        let asp = ui.viewport.2 as f32 / ui.viewport.3 as f32;
        if res.update_size(ui.viewport) || self.mode == GameMode::View {
            set_camera(&res.camera);
        }

        let msaa = res.config.sample_count > 1;
        if msaa {
            if let Some(target) = &mut res.chart_target {
                target.prepare_chart();
            }
        }

        let chart_onto = res
            .chart_target
            .as_ref()
            .map(|it| if msaa { it.input() } else { it.output() })
            .or(res.camera.render_target);

        unsafe {
            self.gl
                .quad_gl
                .retain_render_pass_on_flush(if msaa && res.chart_target.as_ref().is_some_and(|target| target.retains_pass()) {
                    res.chart_target.as_ref().map(|target| target.input().render_pass)
                } else {
                    None
                });
        }
        push_camera_state();
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            viewport: if res.chart_target.is_some() { None } else { Some(ui.viewport) },
            render_target: chart_onto,
            ..Default::default()
        });
        clear_background(BLACK);
        draw_background(*res.background);
        pop_camera_state();

        let chart_target_vp = if res.chart_target.is_some() {
            let vp = res.camera.viewport.unwrap();
            Some((vp.0 - ui.viewport.0, vp.1 - ui.viewport.1, vp.2, vp.3))
        } else {
            res.camera.viewport
        };
        self.gl.quad_gl.render_pass(chart_onto.map(|it| it.render_pass));
        self.gl.quad_gl.viewport(chart_target_vp);

        let h = 1. / res.aspect_ratio;
        draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));

        self.chart.render(ui, res);

        self.gl.quad_gl.render_pass(
            res.chart_target
                .as_ref()
                .map(|it| it.output().render_pass)
                .or_else(|| res.camera.render_pass()),
        );

        unsafe {
            self.gl.quad_gl.retain_render_pass_on_flush(None);
        }

        self.bad_notes.retain(|dummy| dummy.render(res));
        let t = tm.real_time();
        let real_dt = (t - std::mem::replace(&mut self.last_update_time, t)) as f32;
        let dt = self.replay_frame_delta.take().unwrap_or(real_dt);
        if res.config.particle {
            res.emitter.draw(dt);
        }
        crate::core::hide_cover::draw(res);
        self.ui(ui, tm)?;
        self.overlay_ui(ui, tm)?;
        // Official ActiveBlock runs at CameraEvent.AfterForwardAlpha, after
        // notes and HUD. It captures the complete underlay before compositing.

        self.chart.render_block_overlay(&mut self.res);

        if self.mode == GameMode::TweakOffset {
            push_camera_state();
            self.gl.quad_gl.viewport(None);
            set_camera(&Camera2D {
                zoom: vec2(1., -screen_aspect()),
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            self.tweak_offset(ui, Self::interactive(&self.res, &self.state));
            pop_camera_state();
        }

        if !self.res.no_effect && !self.effects.is_empty() {
            push_camera_state();
            set_camera(&Camera2D {
                zoom: vec2(1., asp),
                ..Default::default()
            });
            for e in &self.effects {
                e.render(&mut self.res);
            }
            pop_camera_state();
        }
        if msaa || !self.res.no_effect {
            // render the texture onto screen
            if let Some(target) = &self.res.chart_target {
                self.gl.flush();

                unsafe {
                    self.gl.quad_gl.retain_render_pass_on_flush(None);
                }
                push_camera_state();
                self.gl.quad_gl.viewport(None);
                set_camera(&Camera2D {
                    zoom: vec2(1., asp),
                    render_target: self.res.camera.render_target,
                    viewport: Some(ui.viewport),
                    ..Default::default()
                });
                draw_texture_ex(
                    target.output().texture,
                    -1.,
                    -ui.top,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(2., ui.top * 2.)),
                        ..Default::default()
                    },
                );
                pop_camera_state();
            }
        }

        if !self.rendered_first_frame {
            self.rendered_first_frame = true;
            tracing::info!("game first frame complete");
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if self.should_exit {
            if tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            match self.mode {
                // return result to update score and refresh
                GameMode::Normal => {
                    if let Some(rec) = &self.best_record {
                        NextScene::PopWithResult(Box::new(rec.clone()))
                    } else {
                        NextScene::Pop
                    }
                }
                // not sure if they need result. just keep it
                GameMode::Exercise | GameMode::NoRetry | GameMode::View => NextScene::Pop,
                GameMode::TweakOffset => NextScene::PopWithResult(Box::new(None::<f32>)),
            }
        } else if let Some(next_scene) = self.next_scene.take() {
            if !matches!(next_scene, NextScene::None) && tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            next_scene
        } else {
            NextScene::None
        }
    }
}
