//! 离线视频导出。
//!
//! 画面流程直接复用 Phira Pro 的 `chart_video` 演出：开场加载画面、谱面播放、
//! 退场与结算都由 `prpr::chart_video::presentation` 绘制，时间轴与音频布局也
//! 沿用 Phira Pro 的规则。与 Phira Pro 的谱面渲染页不同，这里的谱面阶段不是
//! 自动播放，而是把回放录制好的判定、长条和触点交给同一个 `GameScene`，因此
//! 导出的仍然是这份回放的画面。
//!
//! 逐帧渲染到离屏 `RenderTarget`，用 `glReadPixels` 读回 RGBA，再通过管道交给
//! FFmpeg 编码为 H.264 + AAC。音轨离线合成：音乐按回放录制速率重采样，打击音
//! 按判定时刻混入，结算音乐按 Phira Pro 的时间轴放置。

use super::scene::ReplayScene;
use anyhow::{bail, ensure, Context, Result};
use macroquad::{miniquad::gl::*, prelude::*, window::get_internal_gl};
use prpr::{
    chart_video::{
        ffmpeg_candidates, presentation::PresentationAssets, AudioOptions, FrameContext,
        PresentationOptions, RenderStage, VisualOptions,
    },
    ext::ScaleType,
    judge::PlayResult,
    time::TimeManager,
    ui::{IntoShading, TextPainter, Ui},
};
use sasa::{AudioClip, Frame};
use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::Instant,
};

/// 导出音轨固定为 44.1 kHz 立体声，兼容性最好。
const SAMPLE_RATE: u32 = 44_100;

#[derive(Clone, Debug)]
pub struct Options {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// 谱面时间（秒）。
    pub start: f64,
    pub end: Option<f64>,
    pub show_hud: bool,
    pub show_touches: bool,
    pub audio: bool,
    pub crf: u32,
    /// Phira Pro 的开场、结算与演出选项。
    pub presentation: PresentationOptions,
    /// Phira Pro 谱面渲染页的游玩画面选项。
    pub visuals: VisualOptions,
}

impl Default for Options {
    fn default() -> Self {
        let mut presentation = PresentationOptions::default();
        presentation.opening = true;
        presentation.ending = true;
        Self {
            output: PathBuf::from("replay.mp4"),
            width: 1920,
            height: 1080,
            fps: 60,
            start: 0.,
            end: None,
            show_hud: true,
            show_touches: true,
            audio: true,
            crf: 18,
            presentation,
            visuals: VisualOptions::default(),
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=240).contains(&self.fps), "帧率必须在 1 到 240 之间");
        ensure!(
            (16..=7680).contains(&self.width)
                && (16..=4320).contains(&self.height)
                && self.width % 2 == 0
                && self.height % 2 == 0,
            "视频宽高必须是有效偶数（最大 7680×4320）"
        );
        ensure!(self.start.is_finite() && self.start >= 0., "起始时间无效");
        ensure!(
            self.end
                .is_none_or(|end| end.is_finite() && end > self.start),
            "结束时间必须晚于开始时间"
        );
        ensure!(self.crf <= 51, "CRF 必须在 0 到 51 之间");
        ensure!(!self.output.as_os_str().is_empty(), "视频输出路径为空");
        ensure!(self.presentation.playing, "回放视频必须包含谱面阶段");
        self.presentation.validate()
    }
}

/// 导出进度回调。GUI 用它画预览和进度条，CLI 用它打印日志。
pub trait Progress {
    /// `preview` 是当前已经编码完成的画面。返回 `false` 取消导出。
    fn update(&mut self, frame: u64, total: u64, elapsed: f64, preview: Texture2D) -> bool;
    /// 耗时阶段（例如合成音轨）的提示。返回 `false` 取消导出。
    fn stage(&mut self, _text: &str) -> bool {
        true
    }
}

/// 命令行导出的进度输出。
#[allow(dead_code)]
pub struct CliProgress;

impl Progress for CliProgress {
    fn update(&mut self, frame: u64, total: u64, elapsed: f64, _preview: Texture2D) -> bool {
        if frame == 1 || frame % 60 == 0 || frame == total {
            let rate = frame as f64 / elapsed.max(1e-6);
            println!(
                "  {}/{} 帧（{:.1}%）  {:.2} fps 渲染速度",
                frame,
                total,
                frame as f64 / total as f64 * 100.,
                rate
            );
        }
        true
    }

    fn stage(&mut self, text: &str) -> bool {
        println!("{text}");
        true
    }
}

#[derive(Clone, Copy)]
struct Segment {
    stage: RenderStage,
    first_frame: u64,
    frames: u64,
}

/// 与 Phira Pro 相同的演出时间轴，但谱面阶段按回放录制速率换算。
struct Timeline {
    segments: Vec<Segment>,
    fps: u32,
    chart_start: f64,
    chart_end: f64,
    speed: f64,
    total_frames: u64,
}

impl Timeline {
    fn new(
        presentation: &PresentationOptions,
        fps: u32,
        chart_start: f64,
        chart_end: f64,
        speed: f64,
    ) -> Result<Self> {
        ensure!(fps > 0, "帧率无效");
        ensure!(
            chart_start.is_finite()
                && chart_end.is_finite()
                && speed.is_finite()
                && speed > 0.
                && chart_end > chart_start,
            "视频时间轴参数无效"
        );
        let playing_seconds = (chart_end - chart_start) / speed;
        let mut timeline = Self {
            segments: Vec::new(),
            fps,
            chart_start,
            chart_end,
            speed,
            total_frames: 0,
        };
        for (stage, enabled, seconds) in [
            (
                RenderStage::Opening,
                presentation.opening,
                presentation.opening_seconds,
            ),
            (
                RenderStage::Entering,
                presentation.playing,
                presentation.enter_seconds,
            ),
            (RenderStage::Playing, presentation.playing, playing_seconds),
            (
                RenderStage::Exiting,
                presentation.playing,
                presentation.exit_seconds,
            ),
            (
                RenderStage::Ending,
                presentation.ending,
                presentation.ending_seconds,
            ),
        ] {
            if !enabled || seconds <= 0. {
                continue;
            }
            let frames = (seconds * fps as f64).round().max(1.) as u64;
            timeline.segments.push(Segment {
                stage,
                first_frame: timeline.total_frames,
                frames,
            });
            timeline.total_frames += frames;
        }
        ensure!(!timeline.segments.is_empty(), "至少需要选择一个演出阶段");
        Ok(timeline)
    }

    fn duration(&self) -> f64 {
        self.total_frames as f64 / self.fps as f64
    }

    fn segment(&self, stage: RenderStage) -> Option<Segment> {
        self.segments.iter().find(|it| it.stage == stage).copied()
    }

    fn frame(&self, frame: u64, width: u32, height: u32) -> Option<FrameContext> {
        let segment = self
            .segments
            .iter()
            .find(|it| frame >= it.first_frame && frame < it.first_frame + it.frames)?;
        let local = frame - segment.first_frame;
        let music_time = match segment.stage {
            RenderStage::Entering => Some(self.chart_start),
            RenderStage::Playing => {
                Some(self.chart_start + local as f64 / self.fps as f64 * self.speed)
            }
            RenderStage::Exiting => Some(self.chart_end),
            _ => None,
        };
        Some(FrameContext {
            stage: segment.stage,
            frame,
            video_time: frame as f64 / self.fps as f64,
            stage_time: local as f64 / self.fps as f64,
            stage_duration: segment.frames as f64 / self.fps as f64,
            stage_progress: local as f32 / segment.frames.saturating_sub(1).max(1) as f32,
            music_time,
            delta: 1. / self.fps as f64,
            width,
            height,
            preview: false,
        })
    }
}

/// 导出整段（或指定区间）回放。
///
/// 返回 `None` 表示用户通过进度回调取消了导出。
pub async fn render(
    scene: &mut ReplayScene,
    options: &Options,
    painter: &mut TextPainter,
    progress: &mut dyn Progress,
) -> Result<Option<PathBuf>> {
    options.validate()?;

    let chart_end = scene.video_end();
    let end = options.end.unwrap_or(chart_end).clamp(0., chart_end);
    let start = options.start.clamp(0., end);
    ensure!(end > start, "导出区间为空（{start:.3}s - {end:.3}s）");

    let mut presentation = options.presentation.clone();
    if presentation.result.player_name.trim().is_empty()
        || presentation.result.player_name == "Player"
    {
        if let Some(player) = scene.video_player() {
            presentation.result.player_name = player.to_owned();
        }
    }
    let speed = scene.video_speed();
    let timeline = Timeline::new(&presentation, options.fps, start, end, speed)?;
    let duration = timeline.duration();
    ensure!(duration > 0.01, "导出区间太短");

    let stages = timeline
        .segments
        .iter()
        .map(|it| it.stage.label())
        .collect::<Vec<_>>()
        .join(" → ");

    println!("PPR Render · 视频导出");
    println!("  回放：{}", scene.video_title());
    println!("  区间：{start:.2}s - {end:.2}s（谱面时间，{speed:.2}×）");
    println!("  偏移：{:+.3}s", scene.video_offset());
    println!("  流程：{stages}（Phira Pro 演出）");
    println!(
        "  输出：{}  {}×{}  {} fps  约 {:.1} 秒 / {} 帧",
        options.output.display(),
        options.width,
        options.height,
        options.fps,
        duration,
        timeline.total_frames
    );

    let soundtrack = if options.audio {
        if !progress.stage("正在合成音轨…") {
            return Ok(None);
        }
        next_frame().await;
        // 噪域遮挡会改变音乐的低通状态；先按视频时间轴采样，再离线混音。
        let block_ranges = scene.video_block_ranges(start, end);
        let path = build_soundtrack(
            &timeline,
            scene.video_music(),
            scene.video_ending_audio(),
            scene.video_offset(),
            scene.video_hitsounds(start, end),
            &block_ranges,
            &presentation.audio,
        )?;
        if !progress.stage("音轨合成完成") {
            return Ok(None);
        }
        Some(path)
    } else {
        None
    };

    let mut encoder = Encoder::spawn(options, soundtrack.as_deref(), duration)?;
    let capture = render_target(options.width, options.height);
    capture.texture.set_filter(FilterMode::Linear);
    // 谱面先画到单独的 target，再按 Phira Pro 的演出规则叠到最终画面。
    let chart_capture = render_target(options.width, options.height);
    chart_capture.texture.set_filter(FilterMode::Linear);
    scene.set_video_target(chart_capture);
    scene.apply_video_visuals(&options.visuals, options.show_hud);
    scene.prepare_video(start, options.show_hud);

    let mut info = scene.video_info();
    prpr::chart_video::presentation::apply_metadata(&mut info, &presentation);
    // Phira Pro 的开场卡会随机显示一条 Tip；没有 Tip 的谱面补一条，
    // 避免开场左下角只剩一个「Tip:」标签。按谱面选择，保证重复导出一致。
    if info.tip.as_ref().is_none_or(|tip| tip.trim().is_empty()) {
        let seed = info.id.unwrap_or_default().unsigned_abs() as usize
            + info.name.bytes().map(usize::from).sum::<usize>();
        info.tip = prpr::config::TIPS
            .get(seed % prpr::config::TIPS.len())
            .cloned();
    }
    let assets = PresentationAssets {
        illustration: scene.video_illustration(),
        background: scene.video_background(),
        avatar: scene.video_avatar(),
        icons: scene.video_icons(),
        challenge_frames: std::array::from_fn(|_| None),
        font: None,
        font_line_height: 1.5,
    };
    let empty_result = PlayResult::default();

    let mut tm = TimeManager::manual(Box::new(|| 0.));
    let mut buffer = vec![0u8; options.width as usize * options.height as usize * 4];
    let mut ending_result: Option<PlayResult> = None;
    let started = Instant::now();

    for frame in 0..timeline.total_frames {
        let context = timeline
            .frame(frame, options.width, options.height)
            .context("演出时间轴帧越界")?;

        if let Some(music_time) = context.music_time {
            scene.video_frame(music_time, context.delta as f32);
            let mut ui = Ui::new(
                painter,
                Some((0, 0, options.width as i32, options.height as i32)),
            );
            tm.seek_to(music_time);
            scene.render_video(&mut tm, &mut ui, chart_capture, options.show_touches)?;
        }

        let mut ui = Ui::new(
            painter,
            Some((0, 0, options.width as i32, options.height as i32)),
        );
        set_camera(&Camera2D {
            render_target: Some(capture),
            ..ui.camera()
        });
        clear_background(BLACK);
        prpr::chart_video::presentation::background(&mut ui, &assets);

        let chart_alpha = match context.stage {
            RenderStage::Entering => 1. - (1. - context.stage_progress).powi(3),
            RenderStage::Playing => 1.,
            RenderStage::Exiting => (1. - context.stage_progress).powi(2),
            _ => 0.,
        };
        if chart_alpha > 0. {
            draw_chart(&mut ui, chart_capture.texture, chart_alpha);
        } else if context.stage == RenderStage::Ending && presentation.playing {
            // Phira Pro 在结算开头短暂保留最后一帧谱面，再淡入结算面板。
            let opacity = (1. - context.stage_time * 6. / context.stage_duration.min(6.))
                .clamp(0., 1.) as f32;
            if opacity > 0. {
                draw_chart(&mut ui, chart_capture.texture, opacity);
            }
        }

        match context.stage {
            RenderStage::Opening => prpr::chart_video::presentation::draw(
                &mut ui,
                &context,
                &presentation,
                &info,
                &assets,
                &empty_result,
            ),
            RenderStage::Ending => {
                let result = ending_result.get_or_insert_with(|| scene.video_result());
                prpr::chart_video::presentation::draw(
                    &mut ui,
                    &context,
                    &presentation,
                    &info,
                    &assets,
                    result,
                );
                // Phira Pro 的结算面板固定写 AUTOPLAY；回放视频改成 REPLAY。
                draw_replay_label(&mut ui, &context, &buffer, options.width, options.height);
            }
            _ => {}
        }

        unsafe { get_internal_gl() }.flush();
        read_pixels(capture, options.width, options.height, &mut buffer);
        encoder.write(&buffer)?;

        let elapsed = started.elapsed().as_secs_f64();
        if !progress.update(frame + 1, timeline.total_frames, elapsed, capture.texture) {
            drop(encoder);
            let _ = std::fs::remove_file(&options.output);
            return Ok(None);
        }

        // 每帧都让窗口事件循环运行，避免长时间离线渲染让系统事件和音频线程饥饿。
        next_frame().await;
    }

    let out = encoder.finish()?;
    println!("视频已保存：{}", out.display());
    Ok(Some(out))
}

fn draw_chart(ui: &mut Ui, texture: Texture2D, alpha: f32) {
    let area = ui.screen_rect();
    ui.alpha(alpha.clamp(0., 1.), |ui| {
        ui.fill_rect(
            area,
            (texture, area, ScaleType::Fit, WHITE)
                .into_shading()
                .flip_y(),
        );
    });
}

/// 覆盖 Phira Pro 结算面板里的 AUTOPLAY 标签。
///
/// 布局与 `prpr::chart_video::presentation::result` 中的标签完全一致，只把
/// 自动播放文案换成回放文案；位移、透明度和出现时机也跟随原标签的入场动画。
/// 先用面板底色盖住 AUTOPLAY，再写 REPLAY，避免叠字。底色取上一帧的像素：
/// 结算面板在标签出现前就已经画好，而这里读取当前帧会打断正在批处理的离屏
/// 渲染，所以不能直接在绘制途中调用 `glReadPixels`。
fn draw_replay_label(ui: &mut Ui, context: &FrameContext, frame: &[u8], width: u32, height: u32) {
    let time = (context.stage_time - context.delta) * 6. / context.stage_duration.min(6.) - 1.;
    let slide = result_slide(time, 0.1, 1.);
    let alpha = result_alpha(time, 0.6, 0.9);
    if alpha <= 0. {
        return;
    }

    const X: f32 = 0.58356;
    const Y: f32 = 0.46214;
    const EM: f32 = 0.01189;
    let mut position = vec2(X * 2. - 1., (Y - 0.5) * 1.5);
    // 与 `geometry::text` 的 GDI+ 基线补偿保持一致。
    position.x += EM / 3.;
    position.y += EM * (0.384 - 0.75);
    let size = EM * 25.;

    ui.scope(|ui| {
        ui.dx(2. * slide);
        let rect = ui
            .text("AUTOPLAY")
            .pos(position.x, position.y)
            .size(size)
            .no_baseline()
            .anchor(0., 0.)
            .measure_using(&prpr::core::PGR_FONT);
        // 取标签正下方的面板底色；那里没有文字，采样结果就是干净的背景色。
        let sample = sample_frame(
            frame,
            width,
            height,
            rect.x + rect.w / 2. + 2. * slide,
            rect.y + rect.h + 0.014,
        );
        let patch = Rect::new(
            rect.x - 0.006,
            rect.y - 0.004,
            rect.w + 0.012,
            rect.h + 0.008,
        );
        ui.fill_rect(
            patch,
            Color::new(
                sample[0] as f32 / 255.,
                sample[1] as f32 / 255.,
                sample[2] as f32 / 255.,
                1.,
            ),
        );

        ui.alpha(alpha, |ui| {
            ui.text("REPLAY")
                .pos(position.x, position.y)
                .size(size)
                .no_baseline()
                .anchor(0., 0.)
                .color(WHITE)
                .draw_using(&prpr::core::PGR_FONT);
        });
    });
}

fn result_slide(time: f64, start: f64, end: f64) -> f32 {
    let value = ((time - start) / (end - start)).clamp(0., 1.) as f32;
    // 与 Phira Pro 的 `1. - out_cubic(part(..))` 等价：1 表示还在右侧屏外。
    (1. - value).powi(3)
}

fn result_alpha(time: f64, start: f64, end: f64) -> f32 {
    let value = ((time - start) / (end - start)).clamp(0., 1.) as f32;
    if value < 0.5 {
        2. * value * value
    } else {
        1. - (-2. * value + 2.).powi(2) / 2.
    }
}

/// 从上一帧的 RGBA 缓冲中取一个像素。`frame` 是 `glReadPixels` 的原始
/// 行序（从下到上），坐标参数使用的是画面内的世界坐标。
fn sample_frame(frame: &[u8], width: u32, height: u32, x: f32, y: f32) -> [u8; 4] {
    let top = height as f32 / width as f32;
    let px = ((x + 1.) * width as f32 / 2.).clamp(0., width as f32 - 1.) as usize;
    let py = ((y / top + 1.) * height as f32 / 2.).clamp(0., height as f32 - 1.) as usize;
    let index = ((height as usize - 1 - py) * width as usize + px) * 4;
    frame
        .get(index..index + 4)
        .and_then(|pixel| pixel.try_into().ok())
        .unwrap_or([0, 0, 0, 255])
}

fn ffmpeg_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn read_pixels(target: RenderTarget, width: u32, height: u32, buffer: &mut [u8]) {
    unsafe {
        let id = target
            .render_pass
            .gl_internal_id(get_internal_gl().quad_context);
        // Android 的 miniquad 使用 OpenGL ES 2 上下文，没有单独的
        // GL_READ_FRAMEBUFFER 绑定；GL_FRAMEBUFFER 在 ES2 / ES3 上都可用。
        #[cfg(target_os = "android")]
        {
            let mut previous = 0;
            glGetIntegerv(0x8CA6, &mut previous); // GL_FRAMEBUFFER_BINDING
            glBindFramebuffer(GL_FRAMEBUFFER, id);
            glReadPixels(
                0,
                0,
                width as i32,
                height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                buffer.as_mut_ptr() as _,
            );
            glBindFramebuffer(GL_FRAMEBUFFER, previous as u32);
        }
        #[cfg(not(target_os = "android"))]
        {
            let mut previous = 0;
            glGetIntegerv(0x8CAA, &mut previous); // GL_READ_FRAMEBUFFER_BINDING
            glBindFramebuffer(GL_READ_FRAMEBUFFER, id);
            glReadPixels(
                0,
                0,
                width as i32,
                height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                buffer.as_mut_ptr() as _,
            );
            glBindFramebuffer(GL_READ_FRAMEBUFFER, previous as u32);
        }
    }
}

struct Placement {
    clip: AudioClip,
    /// 视频时间轴上的起始时刻（秒）。
    at: f64,
    /// 音频文件内的起始位置（秒）。
    source_start: f64,
    /// 音频源播放速率；音乐跟随回放速率，打击音和结算音保持原始音高。
    rate: f64,
    duration: f64,
    gain: f32,
    fade: f64,
    /// 只有音乐受噪域低通影响，打击音和结算音保持原样。
    filtered: bool,
}

/// 与 `prpr` / sasa 相同的二阶 TPT 低通，以及噪域进出的 0.1 秒过渡。
struct BlockLowPass {
    last: Option<bool>,
    used: bool,
    cutoff: f64,
    target: f64,
    from: f64,
    duration: f64,
    elapsed: f64,
    q: f64,
    sample_rate: u32,
    countdown: u32,
    coefficients: [f64; 3],
    history: [[f64; 2]; 2],
    enabled: bool,
    releasing: bool,
}

impl Default for BlockLowPass {
    fn default() -> Self {
        Self {
            last: None,
            used: false,
            cutoff: 22000.,
            target: 22000.,
            from: 22000.,
            duration: 0.,
            elapsed: 0.,
            q: std::f64::consts::FRAC_1_SQRT_2,
            sample_rate: 0,
            countdown: 0,
            coefficients: [0.; 3],
            history: [[0.; 2]; 2],
            enabled: false,
            releasing: false,
        }
    }
}

impl BlockLowPass {
    /// 与 Phira Pro 的 `BlockAudio` 一致：首次进入立即切到 1500 Hz，
    /// 之后的进入 / 离开都用 0.1 秒扫频。
    fn sync(&mut self, blocked: bool) {
        if self.last == Some(blocked) {
            return;
        }
        let seconds = if self.used { 0.1 } else { 0. };
        self.set(blocked.then_some(1500.), seconds);
        self.last = Some(blocked);
        self.used |= blocked;
    }

    fn set(&mut self, cutoff: Option<f32>, seconds: f32) {
        self.from = self.cutoff;
        self.target = cutoff.unwrap_or(22000.).clamp(10., 22000.) as f64;
        self.duration = seconds.max(0.) as f64;
        self.elapsed = 0.;
        self.releasing = cutoff.is_none();
        self.enabled |= cutoff.is_some();
        self.countdown = 0;
        if self.duration == 0. {
            self.cutoff = self.target;
            if self.releasing {
                self.enabled = false;
            }
        }
    }

    fn process(&mut self, frame: Frame, sample_rate: u32) -> Frame {
        if !self.enabled {
            self.history = [[0., frame.0 as f64], [0., frame.1 as f64]];
            return frame;
        }
        let sweeping = self.elapsed < self.duration;
        if sweeping {
            self.elapsed = (self.elapsed + 1. / sample_rate as f64).min(self.duration);
            self.cutoff = self.from + (self.target - self.from) * (self.elapsed / self.duration);
            if self.elapsed == self.duration {
                self.cutoff = self.target;
                self.countdown = 0;
                if self.releasing {
                    self.enabled = false;
                    self.history = [[0., frame.0 as f64], [0., frame.1 as f64]];
                    return frame;
                }
            }
        }
        if self.countdown == 0 || self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            let frequency = self.cutoff.min(sample_rate as f64 * 0.49);
            let g = (std::f64::consts::PI * frequency / sample_rate as f64).tan();
            let a1 = 1. / (1. + g * (g + 1. / self.q));
            self.coefficients = [a1, g * a1, g * g * a1];
            self.countdown = if sweeping { 32 } else { u32::MAX };
        }
        self.countdown -= 1;
        let [a1, a2, a3] = self.coefficients;
        let mut outputs = [0.; 2];
        for (channel, input) in [frame.0, frame.1].into_iter().enumerate() {
            let [ic1, ic2] = self.history[channel];
            let v3 = input as f64 - ic2;
            let v1 = a1 * ic1 + a2 * v3;
            let v2 = ic2 + a2 * ic1 + a3 * v3;
            self.history[channel] = [2. * v1 - ic1, 2. * v2 - ic2];
            outputs[channel] = v2 as f32;
        }
        Frame(outputs[0], outputs[1])
    }
}

/// 合成 Phira Pro 演出时间轴上的音乐、打击音和结算音，写入临时 WAV。
fn build_soundtrack(
    timeline: &Timeline,
    music: AudioClip,
    ending: AudioClip,
    offset: f64,
    hitsounds: Vec<(f64, AudioClip)>,
    block_ranges: &[(f64, f64)],
    audio: &AudioOptions,
) -> Result<tempfile::TempPath> {
    let mut placements = Vec::new();
    let mut filter_span = None;
    let fps = timeline.fps as f64;

    if let Some(segment) = timeline.segment(RenderStage::Playing) {
        let at = segment.first_frame as f64 / fps;
        let playing_duration = segment.frames as f64 / fps;

        // 音乐：视频时间 t 对应音乐时间 offset + start + (t - at) * speed。
        let music_from = offset + timeline.chart_start;
        let (delay, source_start) = if music_from >= 0. {
            (0., music_from)
        } else {
            // 负偏移时先留静音，等音乐真正开始。
            (-music_from / timeline.speed, 0.)
        };
        let available = (music.length() - source_start).max(0.) / timeline.speed;
        let duration = available.min((playing_duration - delay).max(0.));
        if duration > 0. && audio.music_volume > 0. {
            filter_span = Some((at, timeline.chart_start, timeline.speed));
            placements.push(Placement {
                clip: music,
                at: at + delay,
                source_start,
                rate: timeline.speed,
                duration,
                gain: audio.music_volume,
                fade: audio.fade_seconds,
                filtered: true,
            });
        }

        let stage_end = at + playing_duration;
        for (time, clip) in hitsounds {
            let hit_at = at + (time - timeline.chart_start) / timeline.speed;
            if hit_at < at - 1e-9 || hit_at >= stage_end || audio.hitsound_volume <= 0. {
                continue;
            }
            placements.push(Placement {
                clip,
                at: hit_at,
                source_start: 0.,
                rate: 1.,
                duration: stage_end - hit_at,
                gain: audio.hitsound_volume,
                fade: audio.fade_seconds.min(0.004),
                filtered: false,
            });
        }
    }

    if let Some(segment) = timeline.segment(RenderStage::Ending) {
        let at = segment.first_frame as f64 / fps + audio.ending_delay_seconds;
        let duration = (segment.frames as f64 / fps - audio.ending_delay_seconds)
            .max(0.)
            .min(ending.length());
        if duration > 0. && audio.ending_volume > 0. {
            placements.push(Placement {
                clip: ending,
                at,
                source_start: 0.,
                rate: 1.,
                duration,
                gain: audio.ending_volume,
                fade: audio.fade_seconds,
                filtered: false,
            });
        }
    }

    let count = (timeline.duration() * SAMPLE_RATE as f64).ceil() as usize;
    ensure!(
        count <= (u32::MAX as usize - 36) / 4,
        "音轨长度超过 WAV 支持范围，请缩短导出区间"
    );
    let mut frames = vec![Frame::default(); count];
    let mut filter = BlockLowPass::default();
    let mut range_idx = 0usize;
    for placement in placements {
        let first = (placement.at * SAMPLE_RATE as f64).round() as i64;
        let samples = (placement.duration * SAMPLE_RATE as f64).ceil() as usize;
        for i in 0..samples {
            let target = first + i as i64;
            if target < 0 {
                continue;
            }
            let Some(output) = frames.get_mut(target as usize) else {
                break;
            };
            let time = i as f64 / SAMPLE_RATE as f64;
            let source = placement.source_start + time * placement.rate;
            let Some(sample) = placement.clip.sample(source) else {
                break;
            };
            let fade = envelope(time, placement.duration, placement.fade);
            if fade > 0. {
                let mut sample = sample * (placement.gain * fade);
                if placement.filtered {
                    if let Some((at, chart_start, speed)) = filter_span {
                        let video_time = target as f64 / SAMPLE_RATE as f64;
                        let chart_time = chart_start + (video_time - at) * speed;
                        while range_idx < block_ranges.len()
                            && block_ranges[range_idx].1 <= chart_time
                        {
                            range_idx += 1;
                        }
                        let blocked = block_ranges
                            .get(range_idx)
                            .is_some_and(|&(begin, end)| chart_time >= begin && chart_time < end);
                        filter.sync(blocked);
                        sample = filter.process(sample, SAMPLE_RATE);
                    }
                }
                *output = *output + sample;
            }
        }
    }

    write_wav(&frames)
}

/// 与 `prpr::chart_video` 相同的淡入淡出包络。
fn envelope(time: f64, duration: f64, fade: f64) -> f32 {
    if fade <= 0. {
        1.
    } else {
        (time / fade).min((duration - time) / fade).clamp(0., 1.) as f32
    }
}

fn write_wav(samples: &[Frame]) -> Result<tempfile::TempPath> {
    let mut file = tempfile::Builder::new()
        .prefix("ppr-render-audio-")
        .suffix(".wav")
        .tempfile()
        .context("无法创建临时音轨")?;
    {
        let mut out = BufWriter::new(file.as_file_mut());
        let data_len = u32::try_from(samples.len() * 4).context("音轨太长")?;
        out.write_all(b"RIFF")?;
        out.write_all(&(36 + data_len).to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        for bytes in [
            16u32.to_le_bytes(),
            0x0002_0001u32.to_le_bytes(),
            SAMPLE_RATE.to_le_bytes(),
            (SAMPLE_RATE * 4).to_le_bytes(),
            0x0010_0004u32.to_le_bytes(),
        ] {
            out.write_all(&bytes)?;
        }
        out.write_all(b"data")?;
        out.write_all(&data_len.to_le_bytes())?;
        // 只衰减不放大，避免安静的视频被强行拉响。
        let peak = samples
            .iter()
            .map(|v| v.0.abs().max(v.1.abs()))
            .fold(0f32, f32::max);
        let scale = if peak > 1. { 1. / peak } else { 1. };
        for frame in samples {
            for sample in [frame.0, frame.1] {
                let sample = if sample.is_finite() {
                    (sample * scale).clamp(-1., 1.)
                } else {
                    0.
                };
                out.write_all(&((sample * i16::MAX as f32).round() as i16).to_le_bytes())?;
            }
        }
        out.flush()?;
    }
    Ok(file.into_temp_path())
}

struct Encoder {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    output: PathBuf,
}

impl Encoder {
    fn spawn(options: &Options, soundtrack: Option<&Path>, duration: f64) -> Result<Self> {
        if let Some(parent) = options
            .output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("无法创建输出目录 {}", parent.display()))?;
        }
        // 除 PATH 和 `PHIRA_FFMPEG` 外，也接受与程序放在一起的 FFmpeg，
        // 这样 Windows 的便携包解压后即可直接导出视频。
        let mut programs = ffmpeg_candidates();
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            let mut sidecars = vec![dir.join(ffmpeg_name())];
            sidecars.push(dir.join("data/tools/ffmpeg").join(ffmpeg_name()));
            let at = programs
                .iter()
                .position(|path| path == Path::new("ffmpeg"))
                .unwrap_or(programs.len());
            for path in sidecars.into_iter().rev() {
                if !programs.contains(&path) {
                    programs.insert(at, path);
                }
            }
        }
        let mut errors = Vec::new();
        for program in programs {
            match Self::spawn_with(&program, options, soundtrack, duration) {
                Ok(encoder) => return Ok(encoder),
                Err(err) => errors.push(format!("{}：{err}", program.display())),
            }
        }
        bail!(
            "无法启动 FFmpeg。请安装 ffmpeg 并加入 PATH，或用环境变量 PHIRA_FFMPEG 指定可执行文件。\n{}",
            errors.join("\n")
        )
    }

    fn spawn_with(
        program: &Path,
        options: &Options,
        soundtrack: Option<&Path>,
        duration: f64,
    ) -> Result<Self> {
        let mut command = Command::new(program);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-video_size",
            ])
            .arg(format!("{}x{}", options.width, options.height))
            .args(["-framerate", &options.fps.to_string(), "-i", "pipe:0"]);
        if let Some(soundtrack) = soundtrack {
            command.arg("-i").arg(soundtrack);
        }
        command.args(["-map", "0:v"]);
        if soundtrack.is_some() {
            command.args(["-map", "1:a"]);
        }
        command
            .args([
                "-vf",
                "vflip",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                &options.crf.to_string(),
                "-pix_fmt",
                "yuv420p",
            ])
            .args(["-t", &format!("{duration:.9}")]);
        if soundtrack.is_some() {
            command.args(["-c:a", "aac", "-b:a", "192k"]);
        }
        command
            .args(["-movflags", "+faststart"])
            .arg(&options.output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().context("无法获取 FFmpeg 输入管道")?;
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            output: options.output.clone(),
        })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.stdin
            .as_mut()
            .context("编码器已关闭")?
            .write_all(bytes)
            .context("写入视频帧失败（FFmpeg 可能已退出）")
    }

    fn finish(mut self) -> Result<PathBuf> {
        drop(self.stdin.take());
        let status = self.child.take().context("编码器已关闭")?.wait()?;
        ensure!(status.success(), "FFmpeg 编码失败：{status}");
        ensure!(
            std::fs::metadata(&self.output).is_ok_and(|meta| meta.len() > 0),
            "没有生成有效的视频文件"
        );
        Ok(self.output.clone())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlockLowPass;
    use sasa::Frame;

    fn sine_gain(filter: &mut BlockLowPass, frequency: f64) -> f64 {
        let rate = super::SAMPLE_RATE;
        let mut power = 0.;
        for i in 0..rate {
            let x = (std::f64::consts::TAU * frequency * i as f64 / rate as f64).sin() as f32;
            let y = filter.process(Frame(x, x), rate);
            if i >= rate / 2 {
                power += (y.0 as f64).powi(2);
            }
        }
        (power / (rate / 2) as f64 * 2.).sqrt()
    }

    #[test]
    fn block_audio_first_entry_is_immediate_and_release_is_smooth() {
        let mut filter = BlockLowPass::default();
        filter.sync(false);
        assert!((sine_gain(&mut filter, 100.) - 1.).abs() < 0.03);

        // 首次进入噪域时 Phira Pro 不做扫频，直接切到 1500 Hz。
        filter.sync(true);
        assert!(sine_gain(&mut filter, 8000.) < 0.05);

        // 离开后 0.1 秒内扫回原声，并最终完全旁路。
        filter.sync(false);
        let mut last = Frame::default();
        for _ in 0..(super::SAMPLE_RATE as f64 * 0.11) as usize {
            last = filter.process(Frame(0.2, -0.4), super::SAMPLE_RATE);
        }
        assert_eq!(last.0.to_bits(), 0.2f32.to_bits());
        assert_eq!(last.1.to_bits(), (-0.4f32).to_bits());
    }

    #[test]
    fn block_audio_reentry_during_release_starts_from_current_cutoff() {
        let mut filter = BlockLowPass::default();
        filter.sync(true);
        for _ in 0..1200 {
            filter.process(Frame(0.1, -0.1), super::SAMPLE_RATE);
        }
        let cutoff = filter.cutoff;
        filter.sync(false);
        for _ in 0..600 {
            filter.process(Frame(0.1, -0.1), super::SAMPLE_RATE);
        }
        assert!(filter.cutoff > cutoff);
        filter.sync(true);
        assert_eq!(filter.last, Some(true));
        assert!(filter.enabled);
    }
}
