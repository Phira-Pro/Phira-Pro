//! 回放渲染页：选择 `.ppr` 回放和对应谱面，按录制判定与触点导出 MP4。
//!
//! 渲染内核来自 PPR Render（见 `crate::replay_render`），页面只负责选择文件、
//! 配置导出参数、显示进度和取消。回放本身仍可在应用内的回放播放器查看；
//! 这里额外提供「预览」按钮，直接以 PPR Render 的播放场景打开。

use super::{Page, SharedState};
use crate::replay_render::{
    self,
    replay::{self, Replay},
    scene::ReplayScene,
    video,
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    chart_video::{PresentationOptions, VisualOptions},
    ext::{poll_future, semi_black, semi_white},
    scene::{request_file, show_error, show_message, take_file, NextScene, Scene},
    ui::{DRectButton, Scroll, Ui},
};
use std::{
    borrow::Cow,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

type BoxFut<T> = Pin<Box<dyn Future<Output = T>>>;

const RESOLUTIONS: [(u32, u32, &str); 4] = [(1280, 720, "720p"), (1920, 1080, "1080p"), (2560, 1440, "1440p"), (3840, 2160, "2160p")];
const FPS_CHOICES: [u32; 4] = [30, 60, 120, 240];
const QUALITY_CHOICES: [(u32, &str); 3] = [(18, "高画质"), (23, "标准"), (28, "小体积")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Replay,
    Chart,
    ChartZip,
    Resolution,
    Fps,
    Quality,
    Opening,
    Ending,
    Hud,
    Touches,
    Audio,
    ApFc,
    DoubleHint,
    Particle,
    Acc,
    Judgement,
    Start,
    Preview,
}

const ACTIONS: [Act; 18] = [
    Act::Replay,
    Act::Chart,
    Act::ChartZip,
    Act::Resolution,
    Act::Fps,
    Act::Quality,
    Act::Opening,
    Act::Ending,
    Act::Hud,
    Act::Touches,
    Act::Audio,
    Act::ApFc,
    Act::DoubleHint,
    Act::Particle,
    Act::Acc,
    Act::Judgement,
    Act::Start,
    Act::Preview,
];

fn act_index(act: Act) -> usize {
    ACTIONS.iter().position(|it| *it == act).unwrap_or(0)
}

#[derive(Clone)]
struct Prefs {
    resolution: usize,
    fps: usize,
    quality: usize,
    opening: bool,
    ending: bool,
    hud: bool,
    touches: bool,
    audio: bool,
    visuals: VisualOptions,
}

impl Default for Prefs {
    fn default() -> Self {
        let mut visuals = VisualOptions::default();
        visuals.ap_fc_indicator = true;
        visuals.double_hint = true;
        visuals.particle = true;
        Self {
            resolution: 1,
            fps: 1,
            quality: 0,
            opening: true,
            ending: true,
            hud: true,
            touches: true,
            audio: true,
            visuals,
        }
    }
}

#[derive(Default)]
struct ProgressSnapshot {
    stage: String,
    frame: u64,
    total: u64,
    elapsed: f64,
}

struct ProgressSink {
    state: Arc<Mutex<ProgressSnapshot>>,
    cancel: Arc<AtomicBool>,
}

impl video::Progress for ProgressSink {
    fn update(&mut self, frame: u64, total: u64, elapsed: f64, _preview: Texture2D) -> bool {
        if let Ok(mut state) = self.state.lock() {
            state.frame = frame;
            state.total = total;
            state.elapsed = elapsed;
        }
        !self.cancel.load(Ordering::Relaxed)
    }

    fn stage(&mut self, text: &str) -> bool {
        if let Ok(mut state) = self.state.lock() {
            state.stage = text.to_owned();
        }
        !self.cancel.load(Ordering::Relaxed)
    }
}

enum Job {
    Idle,
    Rendering {
        task: BoxFut<Result<Option<PathBuf>>>,
        progress: Arc<Mutex<ProgressSnapshot>>,
        cancel: Arc<AtomicBool>,
    },
    Done(String),
    Failed(String),
}

enum Preview {
    Loading(BoxFut<Result<ReplayScene>>),
}

pub struct ReplayRenderPage {
    scroll: Scroll,
    buttons: Vec<DRectButton>,
    prefs: Prefs,
    replay_path: Option<PathBuf>,
    replay: Option<Replay>,
    chart_path: Option<PathBuf>,
    /// 从应用谱面库选中的 `local_path`，用于显示曲名。
    chart_local: Option<String>,
    job: Job,
    preview: Option<Preview>,
    preview_scene: Option<Box<dyn Scene>>,
    pick_request: bool,
    status: Option<(String, bool)>,
}

impl ReplayRenderPage {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            buttons: (0..ACTIONS.len()).map(|_| DRectButton::new().with_radius(0.008)).collect(),
            prefs: Prefs::default(),
            replay_path: None,
            replay: None,
            chart_path: None,
            chart_local: None,
            job: Job::Idle,
            preview: None,
            preview_scene: None,
            pick_request: false,
            status: None,
        }
    }

    fn busy(&self) -> bool {
        matches!(self.job, Job::Rendering { .. })
    }

    fn set_replay(&mut self, path: PathBuf) {
        if self.busy() {
            return;
        }
        match replay::load(&path) {
            Ok(value) => {
                let chart = replay_render::detect_chart(&value, &path);
                self.chart_local = None;
                self.replay_path = Some(path);
                self.replay = Some(value);
                self.chart_path = chart;
                self.status = Some(("已载入回放".into(), false));
            }
            Err(err) => {
                self.status = Some((format!("无法读取回放：{err:#}"), true));
            }
        }
    }

    fn set_chart(&mut self, path: PathBuf) {
        match replay_render::normalize_chart_path(path) {
            Ok(path) => {
                self.chart_local = None;
                self.chart_path = Some(path);
                self.status = Some(("已选择谱面".into(), false));
            }
            Err(err) => self.status = Some((format!("无法使用该谱面：{err:#}"), true)),
        }
    }

    fn options(&self) -> video::Options {
        let (width, height, _) = RESOLUTIONS[self.prefs.resolution.min(RESOLUTIONS.len() - 1)];
        let mut presentation = PresentationOptions::default();
        presentation.opening = self.prefs.opening;
        presentation.ending = self.prefs.ending;
        video::Options {
            width,
            height,
            fps: FPS_CHOICES[self.prefs.fps.min(FPS_CHOICES.len() - 1)],
            crf: QUALITY_CHOICES[self.prefs.quality.min(QUALITY_CHOICES.len() - 1)].0,
            show_hud: self.prefs.hud,
            show_touches: self.prefs.touches,
            audio: self.prefs.audio,
            presentation,
            visuals: self.prefs.visuals.clone(),
            ..video::Options::default()
        }
    }

    fn output_path(&self) -> Result<PathBuf> {
        let replay = self.replay.as_ref().context("尚未载入回放")?;
        Ok(replay_render::output_dir()?.join(replay_render::output_name(replay)))
    }

    fn start_export(&mut self) -> Result<()> {
        if self.busy() {
            self.cancel_export();
            return Ok(());
        }
        let replay_path = self.replay_path.clone().context("请先选择回放")?;
        let chart_path = self.chart_path.clone().context("请先选择谱面")?;
        let options = self.options();
        options.validate()?;
        let output = self.output_path()?;
        let mut options = options;
        options.output = output.clone();
        let painter = replay_render::take_painter().context("导出绘制器尚未初始化")?;
        let progress = Arc::new(Mutex::new(ProgressSnapshot {
            stage: "正在加载回放与谱面…".into(),
            ..Default::default()
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let mut sink = ProgressSink {
            state: Arc::clone(&progress),
            cancel: Arc::clone(&cancel),
        };
        let mut painter = painter;
        let out = output.clone();
        let task: BoxFut<Result<Option<PathBuf>>> = Box::pin(async move {
            let result = match ReplayScene::new(replay_path, chart_path).await {
                Ok(mut scene) => {
                    if let Ok(mut state) = sink.state.lock() {
                        state.stage = "正在渲染…".into();
                    }
                    video::render(&mut scene, &options, painter.get(), &mut sink).await
                }
                Err(err) => Err(err),
            };
            // `PainterGuard` 在离开作用域时自动放回，提前失败或被丢弃也一样。
            drop(painter);
            result
        });
        self.job = Job::Rendering {
            task,
            progress,
            cancel,
        };
        self.status = Some((format!("开始导出：{}", out.display()), false));
        Ok(())
    }

    fn cancel_export(&mut self) {
        if let Job::Rendering { cancel, .. } = &self.job {
            cancel.store(true, Ordering::Relaxed);
            self.status = Some(("正在取消导出…".into(), false));
        }
    }

    fn start_preview(&mut self) -> Result<()> {
        if self.preview.is_some() || self.preview_scene.is_some() {
            return Ok(());
        }
        let replay_path = self.replay_path.clone().context("请先选择回放")?;
        let chart_path = self.chart_path.clone().context("请先选择谱面")?;
        self.preview = Some(Preview::Loading(Box::pin(ReplayScene::new(
            replay_path,
            chart_path,
        ))));
        self.status = Some(("正在加载回放画面…".into(), false));
        Ok(())
    }

    fn poll(&mut self) {
        if let Some(Preview::Loading(task)) = &mut self.preview {
            if let Some(result) = poll_future(task.as_mut()) {
                self.preview = None;
                match result {
                    Ok(scene) => {
                        self.preview_scene = Some(Box::new(replay_render::ReplayOverlay::new(scene)));
                    }
                    Err(err) => self.status = Some((format!("预览失败：{err:#}"), true)),
                }
            }
        }
        let finished = if let Job::Rendering { task, .. } = &mut self.job {
            poll_future(task.as_mut())
        } else {
            None
        };
        if let Some(result) = finished {
            let job = std::mem::replace(&mut self.job, Job::Idle);
            let output = match job {
                Job::Rendering { .. } => self
                    .replay
                    .as_ref()
                    .and_then(|_| self.output_path().ok()),
                _ => None,
            };
            self.job = match result {
                Ok(Some(path)) => {
                    let shown = publish_output(path);
                    self.preview_scene = None;
                    Job::Done(shown)
                }
                Ok(None) => {
                    self.status = Some(("已取消导出".into(), false));
                    if let Some(path) = output {
                        let _ = std::fs::remove_file(path);
                    }
                    Job::Idle
                }
                Err(err) => Job::Failed(format!("{err:#}")),
            };
        }
    }

    fn render_card(&mut self, ui: &mut Ui, t: f32, act: Act, title: &str, subtitle: &str, button: &str) {
        let r = Rect::new(0.012, 0., ui.content_rect().w - 0.024, 0.086);
        self.buttons[act_index(act)].build(ui, t, r, |ui, path| {
            ui.fill_path(&path, semi_black(0.35));
            ui.text(title)
                .pos(r.x + 0.016, r.y + 0.013)
                .size(0.38)
                .no_baseline()
                .max_width(r.w - 0.24)
                .draw();
            ui.text(subtitle)
                .pos(r.x + 0.016, r.y + 0.055)
                .size(0.28)
                .no_baseline()
                .max_width(r.w - 0.24)
                .color(semi_white(0.6))
                .draw();
            ui.text(button)
                .pos(r.right() - 0.02, r.center().y)
                .anchor(1., 0.5)
                .no_baseline()
                .size(0.32)
                .draw();
        });
    }

    fn render_toggle(&mut self, ui: &mut Ui, t: f32, act: Act, title: &str, value: bool) {
        let r = Rect::new(0.012, 0., ui.content_rect().w - 0.024, 0.062);
        self.buttons[act_index(act)].render_text(
            ui,
            r,
            t,
            format!("{title}    {}", if value { "开" } else { "关" }),
            0.33,
            value,
        );
    }

    fn render_choice(&mut self, ui: &mut Ui, t: f32, act: Act, title: &str, value: &str) {
        let r = Rect::new(0.012, 0., ui.content_rect().w - 0.024, 0.062);
        self.buttons[act_index(act)].render_text(ui, r, t, format!("{title}    {value}"), 0.33, false);
    }
}

fn publish_output(path: PathBuf) -> String {
    #[cfg(target_os = "android")]
    {
        let name = path.file_name().and_then(|it| it.to_str()).unwrap_or("replay-render.mp4").to_owned();
        match crate::android_publish_video(&path.display().to_string(), &name) {
            Ok(saved) if !saved.is_empty() => {
                let _ = std::fs::remove_file(&path);
                saved
            }
            Ok(_) => format!("{}（未能复制到下载目录，文件留在应用缓存里）", path.display()),
            Err(err) => format!("{}（保存到下载目录失败：{err:#}）", path.display()),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        path.display().to_string()
    }
}

impl Page for ReplayRenderPage {
    fn label(&self) -> Cow<'static, str> {
        "回放渲染".into()
    }

    fn can_play_bgm(&self) -> bool {
        !self.busy()
    }

    fn take_render_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.pick_request)
    }

    fn on_render_chart_picked(&mut self, local_path: String) {
        match replay_render::chart_from_library(&local_path) {
            Ok(path) => {
                self.chart_local = Some(local_path);
                self.chart_path = Some(path);
                self.status = Some(("已从谱面库选择谱面".into(), false));
            }
            Err(err) => self.status = Some((format!("无法打开谱面：{err:#}"), true)),
        }
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if let Some((id, path)) = take_file() {
            match id.as_str() {
                "replay_render_replay" if !path.is_empty() => self.set_replay(path.into()),
                "replay_render_chart" if !path.is_empty() => self.set_chart(path.into()),
                _ => prpr::scene::return_file(id, path),
            }
        }
        self.poll();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            for button in &mut self.buttons {
                button.cancel(t);
            }
            return Ok(true);
        }
        for act in ACTIONS {
            if !self.buttons[act_index(act)].touch(touch, t) {
                continue;
            }
            if self.busy() && !matches!(act, Act::Start) {
                continue;
            }
            match act {
                Act::Replay => request_file("replay_render_replay"),
                Act::Chart => self.pick_request = true,
                Act::ChartZip => request_file("replay_render_chart"),
                Act::Resolution => {
                    self.prefs.resolution = (self.prefs.resolution + 1) % RESOLUTIONS.len();
                }
                Act::Fps => self.prefs.fps = (self.prefs.fps + 1) % FPS_CHOICES.len(),
                Act::Quality => self.prefs.quality = (self.prefs.quality + 1) % QUALITY_CHOICES.len(),
                Act::Opening => self.prefs.opening ^= true,
                Act::Ending => self.prefs.ending ^= true,
                Act::Hud => self.prefs.hud ^= true,
                Act::Touches => self.prefs.touches ^= true,
                Act::Audio => self.prefs.audio ^= true,
                Act::ApFc => self.prefs.visuals.ap_fc_indicator ^= true,
                Act::DoubleHint => self.prefs.visuals.double_hint ^= true,
                Act::Particle => self.prefs.visuals.particle ^= true,
                Act::Acc => self.prefs.visuals.show_acc ^= true,
                Act::Judgement => self.prefs.visuals.show_judgement_details ^= true,
                Act::Start => {
                    if let Err(err) = self.start_export() {
                        show_error(err);
                    }
                }
                Act::Preview => {
                    if let Err(err) = self.start_preview() {
                        show_message(format!("{err:#}"));
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let r = ui.content_rect();
        let t = s.t;
        s.render_fader(ui, |ui| {
            self.scroll.size((r.w, r.h));
            ui.dx(r.x);
            ui.dy(r.y);
            let mut scroll = std::mem::take(&mut self.scroll);
            scroll.render(ui, |ui| {
                let w = r.w - 0.03;
                let mut h = 0.;
                ui.text("把回放按录制时的判定与触点导出为 MP4，画面沿用 Phira Pro 的谱面渲染与开场 / 结算演出。")
                    .pos(0.012, 0.005)
                    .size(0.30)
                    .max_width(w - 0.024)
                    .multiline()
                    .draw();
                ui.dy(0.05);
                h += 0.05;

                let replay_title = self
                    .replay
                    .as_ref()
                    .map(|it| format!("回放：{} · {}", it.meta.name, it.meta.player.as_deref().unwrap_or("未记录玩家")))
                    .unwrap_or_else(|| "还没有选择回放".into());
                let replay_sub = self
                    .replay_path
                    .as_ref()
                    .map(|it| it.display().to_string())
                    .unwrap_or_else(|| "选择 Phira Pro 导出的 .ppr 回放".into());
                self.render_card(ui, t, Act::Replay, &replay_title, &replay_sub, if self.replay.is_some() { "更换" } else { "选择" });
                ui.dy(0.092);
                h += 0.092;

                let (chart_title, chart_sub) = match (&self.chart_path, &self.chart_local) {
                    (Some(path), Some(local)) => (format!("谱面：{}", local), path.display().to_string()),
                    (Some(path), None) => ("已找到谱面".into(), path.display().to_string()),
                    _ => ("谱面：未选择".into(), "按回放自动匹配，或从谱面库 / ZIP 选择".into()),
                };
                self.render_card(ui, t, Act::Chart, &chart_title, &chart_sub, "谱面库");
                ui.dy(0.092);
                h += 0.092;
                self.buttons[act_index(Act::ChartZip)].render_text(
                    ui,
                    Rect::new(0.012, 0., w - 0.024, 0.06),
                    t,
                    "选择谱面 ZIP / 压缩包",
                    0.31,
                    false,
                );
                ui.dy(0.068);
                h += 0.068;

                self.render_choice(ui, t, Act::Resolution, "分辨率", RESOLUTIONS[self.prefs.resolution].2);
                ui.dy(0.068);
                h += 0.068;
                self.render_choice(ui, t, Act::Fps, "帧率", &format!("{} fps", FPS_CHOICES[self.prefs.fps]));
                ui.dy(0.068);
                h += 0.068;
                self.render_choice(ui, t, Act::Quality, "编码质量", QUALITY_CHOICES[self.prefs.quality].1);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Opening, "开场演出", self.prefs.opening);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Ending, "结算演出", self.prefs.ending);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Hud, "分数 HUD", self.prefs.hud);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Touches, "回放触点", self.prefs.touches);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Audio, "音轨", self.prefs.audio);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::ApFc, "AP / FC 指示", self.prefs.visuals.ap_fc_indicator);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::DoubleHint, "双押提示", self.prefs.visuals.double_hint);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Particle, "打击特效", self.prefs.visuals.particle);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Acc, "实时准确率", self.prefs.visuals.show_acc);
                ui.dy(0.068);
                h += 0.068;
                self.render_toggle(ui, t, Act::Judgement, "判定详情", self.prefs.visuals.show_judgement_details);
                ui.dy(0.078);
                h += 0.078;

                let busy = self.busy();
                let start_label = if busy { "取消导出" } else { "开始导出" };
                self.buttons[act_index(Act::Start)].render_text(
                    ui,
                    Rect::new(0.012, 0., w - 0.024, 0.082),
                    t,
                    start_label,
                    0.42,
                    !busy,
                );
                ui.dy(0.09);
                h += 0.09;
                if self.busy() {
                    let (percent, stage, frame, total, speed) = if let Job::Rendering { progress, .. } = &self.job {
                        let state = progress.lock().ok();
                        match state.as_deref() {
                            Some(state) => (
                                if state.total > 0 { state.frame as f32 / state.total as f32 } else { 0. },
                                state.stage.clone(),
                                state.frame,
                                state.total,
                                if state.elapsed > 0. { state.frame as f64 / state.elapsed } else { 0. },
                            ),
                            None => (0., String::new(), 0, 0, 0.),
                        }
                    } else {
                        (0., String::new(), 0, 0, 0.)
                    };
                    ui.fill_rect(
                        Rect::new(0.012, 0., w - 0.024, 0.014),
                        semi_white(0.18),
                    );
                    ui.fill_rect(
                        Rect::new(0.012, 0., (w - 0.024) * percent.clamp(0., 1.), 0.014),
                        Color::new(0.55, 0.85, 1., 1.),
                    );
                    ui.text(format!(
                        "{:.1}%  {frame}/{total} 帧  {speed:.1} fps  {}",
                        percent * 100.,
                        stage
                    ))
                    .pos(0.012, 0.025)
                    .size(0.28)
                    .max_width(w - 0.024)
                    .color(semi_white(0.8))
                    .draw();
                    ui.dy(0.06);
                    h += 0.06;
                } else {
                    self.buttons[act_index(Act::Preview)].render_text(
                        ui,
                        Rect::new(0.012, 0., w - 0.024, 0.068),
                        t,
                        "预览回放画面",
                        0.33,
                        false,
                    );
                    ui.dy(0.078);
                    h += 0.078;
                }

                if let Some((text, error)) = &self.status {
                    ui.text(text)
                        .pos(0.012, 0.005)
                        .size(0.30)
                        .max_width(w - 0.024)
                        .multiline()
                        .color(if *error {
                            Color::new(1., 0.55, 0.55, 1.)
                        } else {
                            semi_white(0.85)
                        })
                        .draw();
                    ui.dy(0.05);
                    h += 0.05;
                }
                if let Job::Done(path) = &self.job {
                    ui.text(format!("视频已保存：{path}"))
                        .pos(0.012, 0.005)
                        .size(0.30)
                        .max_width(w - 0.024)
                        .multiline()
                        .color(Color::new(0.6, 1., 0.7, 1.))
                        .draw();
                    ui.dy(0.05);
                    h += 0.05;
                }
                if let Job::Failed(err) = &self.job {
                    ui.text(format!("导出失败：{err}"))
                        .pos(0.012, 0.005)
                        .size(0.30)
                        .max_width(w - 0.024)
                        .multiline()
                        .color(Color::new(1., 0.55, 0.55, 1.))
                        .draw();
                    ui.dy(0.05);
                    h += 0.05;
                }
                (w, h + 0.04)
            });
            self.scroll = scroll;
        });
        Ok(())
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        match self.preview_scene.take() {
            Some(scene) => NextScene::Overlay(scene),
            None => NextScene::None,
        }
    }

    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        if self.busy() {
            self.cancel_export();
            true
        } else {
            false
        }
    }

    fn exit(&mut self) -> Result<()> {
        self.cancel_export();
        self.preview = None;
        self.preview_scene = None;
        Ok(())
    }
}
