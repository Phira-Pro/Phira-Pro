//! 谱面视频渲染页。
//!
//! 从谱面库挑一首（复用多人游戏那套选谱界面），把谱面以自动播放的方式逐帧渲染成
//! mp4（Phi Recorder 风格）。渲染内核在 [`prpr::chart_video`]。

use super::{Page, SharedState};
use crate::{dir, get_data};
use anyhow::{Context, Result};
use macroquad::{prelude::*, window::get_internal_gl};
use prpr::{
    chart_video::{ChartVideoRenderer, VideoOptions},
    ext::{semi_black, RectExt},
    scene::{show_error, show_message},
    ui::{DRectButton, Scroll, Ui},
};
use std::{
    borrow::Cow,
    future::Future,
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

type BoxFut<T> = Pin<Box<dyn Future<Output = T>>>;

#[cfg(target_os = "windows")]
mod ffmpeg;

/// 桌面用系统文件对话框挑输出目录。
#[cfg(not(target_os = "android"))]
fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// 移动端没有可用的系统对话框，直接返回 `None`，由调用方提示。
#[cfg(target_os = "android")]
fn pick_folder(_title: &str) -> Option<PathBuf> {
    None
}

const RESOLUTIONS: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
const FPS_CHOICES: [u32; 4] = [30, 60, 120, 240];
/// 每帧给渲染留出的时间片：留一点余量让界面保持响应。
const BUDGET: Duration = Duration::from_millis(45);
const GAP: f32 = 0.016;
const PADDING: f32 = 0.02;

const TITLE_SIZE: f32 = 0.46;
const BODY_SIZE: f32 = 0.36;
const SMALL_SIZE: f32 = 0.30;

/// 界面设置（保存在 `data/render.yml`）。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Prefs {
    out_dir: String,
    res: usize,
    fps: usize,
    hitsound: bool,
    hud: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            out_dir: String::new(),
            res: 1,
            fps: 1,
            hitsound: true,
            hud: false,
        }
    }
}

fn data_root() -> PathBuf {
    PathBuf::from(dir::root().unwrap_or_else(|_| "data".into()))
}

fn prefs_path() -> PathBuf {
    data_root().join("render.yml")
}

fn load_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|it| serde_yaml::from_str(&it).ok())
        .unwrap_or_default()
}

fn save_prefs(prefs: &Prefs) {
    match serde_yaml::to_string(prefs) {
        Ok(text) => {
            if let Err(err) = std::fs::write(prefs_path(), text) {
                tracing::warn!("保存渲染设置失败：{err}");
            }
        }
        Err(err) => tracing::warn!("序列化渲染设置失败：{err}"),
    }
}

/// 去掉文件名里不能用的字符。
fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.' | '(' | ')') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let out = out.trim().trim_matches('.').to_owned();
    if out.is_empty() {
        "chart".to_owned()
    } else {
        out
    }
}

/// 把秒数说成人话。
fn fmt_duration(secs: f64) -> String {
    let total = secs.max(0.).round() as u64;
    if total >= 3600 {
        format!("{} 小时 {} 分", total / 3600, (total % 3600) / 60)
    } else if total >= 60 {
        format!("{} 分 {} 秒", total / 60, total % 60)
    } else {
        format!("{total} 秒")
    }
}

/// 展示用路径：去掉开头的 `./`，让 `./data/charts` 显示成 `data/charts`。
fn pretty_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix("./").unwrap_or(&text).to_owned()
}

fn take_row(y: &mut f32, width: f32, h: f32) -> Rect {
    let r = Rect::new(0., *y, width, h);
    *y += h + GAP;
    r
}

fn section_title(ui: &mut Ui, y: &mut f32, title: &str) {
    let r = ui.text(title).pos(0., *y).size(TITLE_SIZE).no_baseline().draw();
    *y = r.bottom() + GAP;
}

fn paragraph(ui: &mut Ui, y: &mut f32, width: f32, text: &str, color: Color) {
    let r = ui
        .text(text)
        .pos(0., *y)
        .size(SMALL_SIZE)
        .no_baseline()
        .max_width(width)
        .multiline()
        .color(color)
        .draw();
    *y = r.bottom() + GAP;
}

fn setting_tile(ui: &mut Ui, button: &mut DRectButton, t: f32, r: Rect, label: &str, value: &str, chosen: bool) {
    button.build(ui, t, r, |ui, path| {
        ui.fill_path(&path, if chosen { WHITE } else { semi_black(0.35) });
        let color = if chosen { Color::new(0.2, 0.2, 0.2, 1.) } else { WHITE };
        let label_rect = ui
            .text(label)
            .pos(r.x + PADDING, r.y + PADDING)
            .no_baseline()
            .size(SMALL_SIZE)
            .max_width(r.w - PADDING * 2.)
            .color(Color::new(color.r, color.g, color.b, 0.65))
            .draw();
        ui.text(value)
            .pos(r.x + PADDING, label_rect.bottom() + GAP * 0.5)
            .no_baseline()
            .size(BODY_SIZE)
            .max_width(r.w - PADDING * 2.)
            .color(color)
            .draw();
    });
}

/// 选中的谱面（列表来自谱面库，这里只留展示与定位需要的字段）。
struct Selected {
    local_path: String,
    name: String,
    subtitle: String,
    difficulty: f32,
}

enum Job {
    Idle,
    /// 正在加载谱面（读取 info / 曲绘 / 音乐）。
    Loading(BoxFut<Result<ChartVideoRenderer>>),
    Rendering {
        renderer: ChartVideoRenderer,
        started: Instant,
    },
    Done {
        /// 展示给玩家的保存位置（桌面端是文件路径，Android 是「下载/…」）。
        path: String,
        frames: u64,
        secs: f64,
    },
    Failed(String),
}

pub struct RenderPage {
    out_dir: PathBuf,
    selected: Option<Selected>,
    /// 本帧是否请求弹出选谱界面。
    pick_request: bool,
    scroll: Scroll,

    btn_pick: DRectButton,
    btn_res: DRectButton,
    btn_fps: DRectButton,
    btn_hitsound: DRectButton,
    btn_hud: DRectButton,
    btn_out: DRectButton,
    btn_start: DRectButton,
    btn_cancel: DRectButton,
    btn_open: DRectButton,
    btn_ffmpeg: DRectButton,

    #[cfg(target_os = "windows")]
    ffmpeg: ffmpeg::Ffmpeg,

    prefs: Prefs,
    job: Job,
}

impl RenderPage {
    pub fn new() -> Self {
        let prefs = load_prefs();
        let out_dir = if prefs.out_dir.is_empty() {
            data_root().join("render")
        } else {
            PathBuf::from(&prefs.out_dir)
        };
        Self {
            out_dir,
            selected: None,
            pick_request: false,
            scroll: Scroll::new(),
            btn_pick: DRectButton::new().with_radius(0.008),
            btn_res: DRectButton::new().with_radius(0.008),
            btn_fps: DRectButton::new().with_radius(0.008),
            btn_hitsound: DRectButton::new().with_radius(0.008),
            btn_hud: DRectButton::new().with_radius(0.008),
            btn_out: DRectButton::new().with_radius(0.008),
            btn_start: DRectButton::new().with_radius(0.010),
            btn_cancel: DRectButton::new().with_radius(0.010),
            btn_open: DRectButton::new().with_radius(0.008),
            btn_ffmpeg: DRectButton::new().with_radius(0.008),
            #[cfg(target_os = "windows")]
            ffmpeg: ffmpeg::Ffmpeg::new(),
            prefs,
            job: Job::Idle,
        }
    }

    fn remember(&mut self) {
        self.prefs.out_dir = self.out_dir.display().to_string();
        save_prefs(&self.prefs);
    }

    fn chart_path(&self) -> Option<PathBuf> {
        let selected = self.selected.as_ref()?;
        let charts = dir::charts().ok()?;
        Some(PathBuf::from(format!("{charts}/{}", selected.local_path)))
    }

    fn output_path(&self) -> Option<PathBuf> {
        let selected = self.selected.as_ref()?;
        let name = if selected.difficulty > 0. {
            format!("{} [{:.1}].mp4", sanitize(&selected.name), selected.difficulty)
        } else {
            format!("{}.mp4", sanitize(&selected.name))
        };
        Some(self.out_dir.join(name))
    }

    fn start(&mut self) -> Result<()> {
        if self.busy() {
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        if !self.ffmpeg.ready() {
            show_message("请先在编码器区域安装 FFmpeg，等待检测完成后再开始渲染");
            return Ok(());
        }
        let Some(chart) = self.chart_path() else {
            show_message("还没有选择谱面");
            return Ok(());
        };
        let Some(out) = self.output_path() else {
            return Ok(());
        };
        if let Some(parent) = out.parent().filter(|it| !it.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).with_context(|| format!("无法创建目录：{}", parent.display()))?;
        }
        let (width, height) = RESOLUTIONS[self.prefs.res.min(RESOLUTIONS.len() - 1)];
        let opts = VideoOptions {
            width,
            height,
            fps: FPS_CHOICES[self.prefs.fps.min(FPS_CHOICES.len() - 1)],
            start: 0.,
            end: None,
            hitsound: self.prefs.hitsound,
            show_hud: self.prefs.hud,
            sample_count: get_data().config.sample_count.max(1),
        };
        self.job = Job::Loading(Box::pin(ChartVideoRenderer::new(chart, opts, out)));
        Ok(())
    }

    fn busy(&self) -> bool {
        matches!(self.job, Job::Loading(_) | Job::Rendering { .. })
    }

    fn cancel(&mut self) {
        if let Job::Rendering { renderer, .. } = &mut self.job {
            renderer.cancel();
        }
        self.job = Job::Idle;
    }

    fn pick_out_dir(&mut self) {
        let Some(path) = pick_folder("选择视频输出目录") else {
            show_message("当前平台不支持选择目录，沿用默认输出目录");
            return;
        };
        self.out_dir = path;
        self.remember();
    }

    /// 渲染若干帧（每帧最多占用 [`BUDGET`] 一段时间）。
    fn advance(&mut self, painter: &mut prpr::ui::TextPainter) {
        let mut job = std::mem::replace(&mut self.job, Job::Idle);
        let mut err = None;
        let mut finished = false;
        let mut frames = 0u64;
        let mut secs = 0.;
        if let Job::Rendering { renderer, started } = &mut job {
            // 外层界面还有未提交的绘制指令，先让它们落到屏幕上。
            unsafe { get_internal_gl() }.flush();
            let begun = Instant::now();
            loop {
                if renderer.finished() {
                    break;
                }
                if let Err(e) = renderer.render_next_frame(painter) {
                    err = Some(e);
                    break;
                }
                if begun.elapsed() >= BUDGET {
                    break;
                }
            }
            unsafe { get_internal_gl() }.flush();
            finished = renderer.finished();
            frames = renderer.progress().0;
            secs = started.elapsed().as_secs_f64();
        }
        if let Some(err) = err {
            job = Job::Failed(format!("{err:#}"));
        } else if finished {
            job = match std::mem::replace(&mut job, Job::Idle) {
                Job::Rendering { renderer, .. } => match renderer.finish() {
                    Ok(path) => Job::Done {
                        path: publish_output(path),
                        frames,
                        secs,
                    },
                    Err(err) => Job::Failed(format!("{err:#}")),
                },
                other => other,
            };
        }
        self.job = job;
    }
}

impl Page for RenderPage {
    fn label(&self) -> Cow<'static, str> {
        "谱面渲染".into()
    }

    fn can_play_bgm(&self) -> bool {
        // 渲染时不要有背景音乐盖掉预览。
        !matches!(self.job, Job::Rendering { .. })
    }

    fn take_render_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.pick_request)
    }

    fn on_render_chart_picked(&mut self, local_path: String) {
        let data = get_data();
        let Some(chart) = data.charts.iter().find(|it| it.local_path == local_path) else {
            return;
        };
        let mut parts = Vec::new();
        if !chart.info.composer.is_empty() {
            parts.push(chart.info.composer.clone());
        }
        if !chart.info.level.is_empty() {
            parts.push(chart.info.level.clone());
        }
        if !chart.info.charter.is_empty() {
            parts.push(format!("谱师 {}", chart.info.charter));
        }
        self.selected = Some(Selected {
            local_path,
            name: chart.info.name.clone(),
            subtitle: parts.join(" / "),
            difficulty: chart.info.difficulty,
        });
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        #[cfg(target_os = "windows")]
        self.ffmpeg.update();

        // 加载谱面（一次性 future，逐帧 poll）。
        if matches!(self.job, Job::Loading(_)) {
            let mut job = std::mem::replace(&mut self.job, Job::Idle);
            if let Job::Loading(mut fut) = job {
                job = match prpr::ext::poll_future(fut.as_mut()) {
                    Some(Ok(renderer)) => Job::Rendering {
                        renderer,
                        started: Instant::now(),
                    },
                    Some(Err(err)) => Job::Failed(format!("{err:#}")),
                    None => Job::Loading(fut),
                };
            }
            self.job = job;
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            for b in [
                &mut self.btn_pick,
                &mut self.btn_res,
                &mut self.btn_fps,
                &mut self.btn_hitsound,
                &mut self.btn_hud,
                &mut self.btn_out,
                &mut self.btn_start,
                &mut self.btn_cancel,
                &mut self.btn_open,
                &mut self.btn_ffmpeg,
            ] {
                b.cancel(t);
            }
            return Ok(true);
        }
        // 滚动区外的隐藏控件不能抢占底部操作区或页面导航的点击。
        if !self.scroll.contains(touch) {
            for button in [
                &mut self.btn_pick,
                &mut self.btn_res,
                &mut self.btn_fps,
                &mut self.btn_hitsound,
                &mut self.btn_hud,
                &mut self.btn_out,
                &mut self.btn_ffmpeg,
            ] {
                button.cancel(t);
            }
        } else {
            #[cfg(target_os = "windows")]
            if self.btn_ffmpeg.touch(touch, t) {
                self.ffmpeg.action();
                return Ok(true);
            }
            if !self.busy() && self.btn_pick.touch(touch, t) {
                self.pick_request = true;
                return Ok(true);
            }
            if self.btn_res.touch(touch, t) {
                self.prefs.res = (self.prefs.res + 1) % RESOLUTIONS.len();
                self.remember();
                return Ok(true);
            }
            if self.btn_fps.touch(touch, t) {
                self.prefs.fps = (self.prefs.fps + 1) % FPS_CHOICES.len();
                self.remember();
                return Ok(true);
            }
            if self.btn_hitsound.touch(touch, t) {
                self.prefs.hitsound = !self.prefs.hitsound;
                self.remember();
                return Ok(true);
            }
            if self.btn_hud.touch(touch, t) {
                self.prefs.hud = !self.prefs.hud;
                self.remember();
                return Ok(true);
            }
            if self.btn_out.touch(touch, t) {
                self.pick_out_dir();
                return Ok(true);
            }
        }
        // 「开始」和「取消」占同一块位置，只让当前显示的那个接收点击。
        if self.busy() {
            if self.btn_cancel.touch(touch, t) {
                self.cancel();
                return Ok(true);
            }
        } else if self.btn_start.touch(touch, t) {
            self.start()?;
            return Ok(true);
        }
        if !cfg!(target_os = "android") && self.btn_open.touch(touch, t) {
            let res = std::fs::create_dir_all(&self.out_dir)
                .map_err(anyhow::Error::from)
                .and_then(|_| open_in_file_manager(&self.out_dir).map_err(anyhow::Error::from));
            if let Err(err) = res {
                show_error(err);
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        // 先推进视频渲染，再画界面（这样预览贴图是刚渲染好的那一帧）。
        let camera = ui.camera();
        self.advance(ui.text_painter);
        set_camera(&camera);

        s.render_fader(ui, |ui| {
            let cr = ui.content_rect();
            let outer = Rect::new(cr.x + 0.025, cr.y + 0.015, cr.w - 0.05, cr.h - 0.03);
            ui.fill_path(&outer.feather(0.012).rounded(0.012), semi_black(0.30));
            let inner = outer.feather(-PADDING);
            let footer_h = 0.085;
            let footer = Rect::new(inner.x, inner.bottom() - footer_h, inner.w, footer_h);
            // 滚动区域单独拿出来，避免和闭包里对 self 其它字段的借用冲突。
            let mut scroll = std::mem::replace(&mut self.scroll, Scroll::new());
            scroll.size((inner.w, (inner.h - footer_h - GAP).max(0.01)));
            let width = inner.w;
            ui.scope(|ui| {
                ui.dx(inner.x);
                ui.dy(inner.y);
                scroll.render(ui, |ui| {
                    let mut y = 0.;

                    // 谱面：列表直接复用多人游戏那套选谱界面。
                    section_title(ui, &mut y, "谱面");
                    let (name, subtitle, action) = self
                        .selected
                        .as_ref()
                        .map_or(("尚未选择谱面", "从谱面库选择要导出的视频内容", "选择"), |selected| {
                            (selected.name.as_str(), selected.subtitle.as_str(), "更换")
                        });
                    let name_h = ui.text(name).size(BODY_SIZE).no_baseline().measure().h;
                    let subtitle_h = ui.text(subtitle).size(SMALL_SIZE).no_baseline().measure().h;
                    let r = take_row(&mut y, width, (PADDING * 2. + name_h + GAP * 0.5 + subtitle_h).max(0.11));
                    self.btn_pick.build(ui, t, r, |ui, path| {
                        ui.fill_path(&path, semi_black(0.35));
                        let text_width = r.w - PADDING * 3. - 0.12;
                        let title = ui
                            .text(name)
                            .pos(PADDING, r.y + PADDING)
                            .size(BODY_SIZE)
                            .no_baseline()
                            .max_width(text_width)
                            .draw();
                        ui.text(subtitle)
                            .pos(PADDING, title.bottom() + GAP * 0.5)
                            .size(SMALL_SIZE)
                            .no_baseline()
                            .max_width(text_width)
                            .color(Color::new(1., 1., 1., 0.65))
                            .draw();
                        ui.text(action)
                            .pos(r.right() - PADDING, r.center().y)
                            .size(BODY_SIZE)
                            .no_baseline()
                            .anchor(1., 0.5)
                            .max_width(0.12)
                            .color(Color::new(1., 1., 1., 0.7))
                            .draw();
                    });
                    if self.busy() {
                        self.btn_pick.invalidate();
                    }

                    // 所有设置共享列宽和行高，空间不足时换成两列或单列。
                    y += GAP;
                    section_title(ui, &mut y, "输出设置");
                    let (w, h) = RESOLUTIONS[self.prefs.res.min(RESOLUTIONS.len() - 1)];
                    let min_tile = ui.text("3840 × 2160").size(BODY_SIZE).no_baseline().measure();
                    let columns = [4, 2, 1]
                        .into_iter()
                        .find(|&n| (width - GAP * (n - 1) as f32) / n as f32 >= min_tile.w + PADDING * 2.)
                        .unwrap_or(1);
                    let tile_w = (width - GAP * (columns - 1) as f32) / columns as f32;
                    let label_h = ui.text("分辨率").size(SMALL_SIZE).no_baseline().measure().h;
                    let value_h = ui.text("显示界面").size(BODY_SIZE).no_baseline().measure().h.max(min_tile.h);
                    let tile_h = PADDING * 2. + label_h + GAP * 0.5 + value_h;
                    for (i, (button, label, value, chosen)) in [
                        (&mut self.btn_res, "分辨率", format!("{w} × {h}"), false),
                        (&mut self.btn_fps, "帧率", format!("{} 帧/秒", FPS_CHOICES[self.prefs.fps.min(FPS_CHOICES.len() - 1)]), false),
                        (&mut self.btn_hitsound, "打击音", if self.prefs.hitsound { "开启" } else { "关闭" }.to_owned(), self.prefs.hitsound),
                        (&mut self.btn_hud, "画面内容", if self.prefs.hud { "显示界面" } else { "纯谱面" }.to_owned(), self.prefs.hud),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let r = Rect::new((i % columns) as f32 * (tile_w + GAP), y + (i / columns) as f32 * (tile_h + GAP), tile_w, tile_h);
                        setting_tile(ui, button, t, r, label, &value, chosen);
                    }
                    y += 4usize.div_ceil(columns) as f32 * (tile_h + GAP) + GAP;

                    section_title(ui, &mut y, "保存位置");
                    let out = if cfg!(target_os = "android") {
                        "下载/PhiraPro".to_owned()
                    } else {
                        self.output_path()
                            .map(|it| pretty_path(&it))
                            .unwrap_or_else(|| pretty_path(&self.out_dir))
                    };
                    let button_w = if cfg!(target_os = "android") { 0. } else { 0.24 + GAP };
                    let path_w = width - PADDING * 2. - button_w;
                    let path_h = ui.text(&out).size(SMALL_SIZE).no_baseline().max_width(path_w).multiline().measure().h;
                    let r = take_row(&mut y, width, (path_h + PADDING * 2.).max(0.085));
                    ui.fill_path(&r.rounded(0.008), semi_black(0.25));
                    ui.text(out)
                        .pos(PADDING, r.center().y)
                        .size(SMALL_SIZE)
                        .no_baseline()
                        .anchor(0., 0.5)
                        .max_width(path_w)
                        .multiline()
                        .draw();
                    if cfg!(target_os = "android") {
                        self.btn_out.invalidate();
                    } else {
                        self.btn_out.render_text(
                            ui,
                            Rect::new(width - 0.24 - PADDING, r.center().y - 0.0325, 0.24, 0.065),
                            t,
                            "更改目录",
                            SMALL_SIZE,
                            false,
                        );
                    }

                    #[cfg(target_os = "windows")]
                    {
                        y += GAP;
                        section_title(ui, &mut y, "视频编码器");
                        paragraph(ui, &mut y, width, &self.ffmpeg.description(), Color::new(1., 1., 1., 0.75));
                        if !self.ffmpeg.ready() {
                            paragraph(
                                ui,
                                &mut y,
                                width,
                                "下载来源：gyan.dev。仅安装到应用工具目录，不需要管理员权限。",
                                Color::new(1., 1., 1., 0.55),
                            );
                        }
                        if let Some(label) = self.ffmpeg.button_label() {
                            let r = take_row(&mut y, width, 0.07);
                            self.btn_ffmpeg.render_text(ui, r, t, label, BODY_SIZE, false);
                        } else {
                            self.btn_ffmpeg.invalidate();
                        }
                    }

                    // 状态 / 进度。
                    y += GAP;
                    section_title(ui, &mut y, "渲染状态");
                    let muted = Color::new(1., 1., 1., 0.65);
                    match &self.job {
                        Job::Idle => {
                            paragraph(ui, &mut y, width, "视频包含谱面音乐，打击音按开关混入。渲染过程不播放声音，可随时取消。", muted);
                        }
                        Job::Loading(_) => {
                            paragraph(ui, &mut y, width, "正在加载谱面…", WHITE);
                        }
                        Job::Rendering { renderer, started } => {
                            let (frame, total) = renderer.progress();
                            let p = if total > 0 { frame as f32 / total as f32 } else { 0. }.clamp(0., 1.);
                            let elapsed = started.elapsed().as_secs_f64();
                            let eta = if frame > 0 {
                                elapsed / frame as f64 * total.saturating_sub(frame) as f64
                            } else {
                                0.
                            };
                            let bar = take_row(&mut y, width, 0.016);
                            ui.fill_path(&bar.rounded(0.008), semi_black(0.5));
                            if p > 0. {
                                ui.fill_path(&Rect::new(bar.x, bar.y, width * p, bar.h).rounded(0.008), WHITE);
                            }
                            paragraph(ui, &mut y, width, &format!("{frame} / {total} 帧（{:.1}%）", p * 100.), WHITE);
                            paragraph(
                                ui,
                                &mut y,
                                width,
                                &format!("已用 {}，预计还需 {}", fmt_duration(elapsed), if frame > 0 { fmt_duration(eta) } else { "…".to_owned() }),
                                muted,
                            );
                            let pw = width.min(0.68);
                            let ph = pw * renderer.options().height as f32 / renderer.options().width as f32;
                            let pr = Rect::new((width - pw) / 2., y, pw, ph);
                            ui.fill_path(&pr.rounded(0.004), semi_black(0.4));
                            let gr = ui.rect_to_global(pr);
                            draw_texture_ex(
                                renderer.texture(),
                                gr.x,
                                gr.y,
                                WHITE,
                                DrawTextureParams {
                                    dest_size: Some(vec2(gr.w, gr.h)),
                                    flip_y: true,
                                    ..Default::default()
                                },
                            );
                            y += ph + 0.01;
                        }
                        Job::Done { path, frames, secs } => {
                            paragraph(ui, &mut y, width, &format!("渲染完成：{frames} 帧，用时 {}", fmt_duration(*secs)), WHITE);
                            paragraph(ui, &mut y, width, path, muted);
                        }
                        Job::Failed(err) => {
                            paragraph(ui, &mut y, width, &format!("渲染失败：{err}"), Color::new(1., 0.65, 0.6, 1.));
                        }
                    }

                    (width, y + GAP)
                });
            });
            self.scroll = scroll;

            // 操作区固定在滚动区域外，长路径、错误和预览不会遮住取消按钮。
            let action_w = (footer.w - GAP) / 2.;
            let primary = Rect::new(footer.x, footer.y, action_w, footer.h);
            let secondary = Rect::new(footer.x + action_w + GAP, footer.y, action_w, footer.h);
            if self.busy() {
                self.btn_start.invalidate();
                self.btn_cancel.render_text(ui, primary, t, "取消渲染", BODY_SIZE, true);
            } else {
                self.btn_cancel.invalidate();
                self.btn_start.render_text(ui, primary, t, "开始渲染", BODY_SIZE, true);
            }
            if cfg!(target_os = "android") {
                self.btn_open.invalidate();
                ui.text("视频保存到下载目录")
                    .pos(secondary.center().x, secondary.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(SMALL_SIZE)
                    .max_width(secondary.w)
                    .draw();
            } else {
                self.btn_open.render_text(ui, secondary, t, "打开输出目录", BODY_SIZE, false);
            }
        });
        Ok(())
    }
}

/// 渲染完成后把文件交给系统：桌面端原样返回路径，Android 复制进「下载」目录
/// （渲染时的临时文件直接清掉）。
fn publish_output(path: PathBuf) -> String {
    #[cfg(target_os = "android")]
    {
        let name = path.file_name().and_then(|it| it.to_str()).unwrap_or("render.mp4").to_owned();
        match crate::android_publish_video(&path.display().to_string(), &name) {
            Ok(saved) if !saved.is_empty() => {
                let _ = std::fs::remove_file(&path);
                saved
            }
            Ok(_) => format!("{}（未能复制到下载目录，文件留在应用缓存里）", pretty_path(&path)),
            Err(err) => format!("{}（保存到下载目录失败：{err:#}）", pretty_path(&path)),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        pretty_path(&path)
    }
}

/// 用系统文件管理器打开目录。
fn open_in_file_manager(path: &std::path::Path) -> Result<(), std::io::Error> {
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let cmd = "open";
    std::process::Command::new(cmd)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}
