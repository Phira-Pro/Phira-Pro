use super::{
    audio::{self, AudioPlacement},
    encoder::Encoder,
    extensions::*,
    presentation::{self, PresentationAssets},
    *,
};
use crate::{
    config::{Config, Mods},
    core::{BadNote, HitSound},
    ext::SafeTexture,
    fs::{fs_from_file, load_info},
    info::ChartInfo,
    judge::PlayResult,
    scene::{GameMode, GameScene, LoadingScene, Scene},
    time::TimeManager,
    ui::{IntoShading, TextPainter, Ui},
};
use ::rand::{seq::IteratorRandom, thread_rng};
use anyhow::{Context, Result};
use macroquad::{prelude::*, window::get_internal_gl};
use std::path::PathBuf;

pub struct ChartVideoRenderer {
    scene: GameScene,
    capture: RenderTarget,
    chart_capture: RenderTarget,
    opts: VideoOptions,
    timeline: RenderTimeline,
    assets: PresentationAssets,
    info: ChartInfo,
    tm: TimeManager,
    bad_notes: Vec<BadNote>,
    offset: f64,
    frame: u64,
    simulated_time: Option<f64>,
    encoder: Option<Encoder>,
    out_path: Option<PathBuf>,
    soundtrack: Option<tempfile::TempPath>,
    buf: Vec<u8>,
    extensions: ExtensionSet,
    previous_stage: Option<RenderStage>,
}

impl ChartVideoRenderer {
    pub async fn new(source: PathBuf, options: VideoOptions, out: PathBuf) -> Result<Self> {
        Self::new_with_extensions(source, options, out, vec![]).await
    }

    pub async fn new_with_extensions(
        source: PathBuf,
        options: VideoOptions,
        out: PathBuf,
        extensions: Vec<Box<dyn RenderExtension>>,
    ) -> Result<Self> {
        Self::load(source, options, Some(out), extensions).await
    }

    /// Preview uses the same renderer and clock, without FFmpeg or output files.
    pub async fn preview(source: PathBuf, options: VideoOptions) -> Result<Self> {
        Self::load(source, options, None, vec![]).await
    }

    async fn load(source: PathBuf, mut options: VideoOptions, out: Option<PathBuf>, extensions: Vec<Box<dyn RenderExtension>>) -> Result<Self> {
        let mut extensions = ExtensionSet::new(extensions);
        for extension in &mut extensions.items {
            extension
                .configure(&mut options)
                .with_context(|| format!("渲染扩展 {} 配置失败", extension.id()))?;
        }
        options.validate()?;
        let mut fs = fs_from_file(&source).context("无法打开谱面")?;
        let mut info = load_info(fs.as_mut()).await.context("无法读取谱面信息")?;
        presentation::apply_metadata(&mut info, &options.presentation);
        if info.tip.as_ref().is_none_or(|tip| tip.trim().is_empty()) {
            info.tip = crate::config::TIPS
                .iter()
                .filter(|tip| !tip.trim().is_empty())
                .choose(&mut thread_rng())
                .cloned();
        }
        let (illustration, background) = match LoadingScene::load(fs.as_mut(), &info.illustration).await {
            Ok((ill, bg, _)) => (ill, bg),
            Err(err) => {
                tracing::warn!("曲绘加载失败，改用纯色背景：{err}");
                let texture: SafeTexture = Texture2D::from_rgba8(1, 1, &[24, 28, 36, 255]).into();
                (texture.clone(), texture)
            }
        };
        let music = if out.is_some() {
            Some(crate::loading_cpu::audio(fs.load_file(&info.music).await.context("无法读取谱面音乐")?).await?)
        } else {
            None
        };
        let mut config = Config::default();
        config.mods = Mods::AUTOPLAY;
        config.volume_music = 0.;
        config.volume_sfx = 0.;
        config.sample_count = options.sample_count.clamp(1, 8);
        config.show_fps = false;
        config.show_avg_fps = false;
        config.offset = 0.;
        config.speed = 1.;
        config.adjust_time = false;
        config.interactive = false;
        let mut scene = GameScene::new(GameMode::View, info.clone(), config, fs, None, background.clone(), illustration.clone(), None, None, None)
            .await
            .context("无法加载谱面场景")?;
        scene.hide_ui = !options.show_hud;
        let avatar = if let Some(path) = &options.presentation.result.avatar {
            let bytes = std::fs::read(path).context("无法读取导出头像，请重新选择图片")?;
            let image = crate::loading_cpu::image(bytes).await.context("导出头像不是有效图片")?;
            image.into()
        } else {
            scene.res.player.clone()
        };
        let (font, font_line_height) = if let Some(path) = &options.presentation.font {
            use crate::ui::ab_glyph::Font;
            let bytes = std::fs::read(path).context("无法读取演出字体，请重新选择字体文件")?;
            let font = crate::ui::parse_font(bytes).context("演出字体无法安全显示")?;
            let fallback = crate::ui::parse_font(macroquad::file::load_file("harmonyos.ttf").await?)?;
            let line_height = (font.height_unscaled() + font.line_gap_unscaled()) / font.units_per_em().unwrap_or(font.height_unscaled());
            (Some(std::cell::RefCell::new(TextPainter::new(font, Some(fallback)))), line_height)
        } else {
            (None, 1.5)
        };
        let mut challenge_frames = std::array::from_fn(|_| None);
        if options.presentation.ending && !options.presentation.result.challenge.is_empty() {
            for (slot, name) in challenge_frames.iter_mut().zip(["green", "blue", "red", "gold", "rainbow"]) {
                match load_texture(&format!("render/challenge/{name}.png")).await {
                    Ok(texture) => *slot = Some(texture.into()),
                    Err(error) => tracing::warn!("课题牌子素材不可用，改用纯色背景：{error}"),
                }
            }
        }
        let assets = PresentationAssets {
            illustration,
            background,
            avatar,
            icons: scene.res.icons.clone(),
            challenge_frames,
            font,
            font_line_height,
        };
        let start = options.start.clamp(0., scene.res.track_length);
        let end = options.end.unwrap_or(scene.res.track_length).clamp(start, scene.res.track_length);
        let timeline = RenderTimeline::new(&options.presentation, options.fps, start, end - start)?;
        for extension in &mut extensions.items {
            extension
                .prepare(&timeline, &assets)
                .with_context(|| format!("渲染扩展 {} 准备失败", extension.id()))?;
        }
        let soundtrack = if out.is_some() {
            let mut tracks = audio::soundtrack(&timeline, music, scene.res.res_pack.ending.clone(), &options.presentation.audio);
            if options.hitsound {
                if let Some(playing) = timeline.segment(RenderStage::Playing) {
                    for line in &scene.chart.lines {
                        for note in &line.notes {
                            if note.fake {
                                continue;
                            }
                            let at = note.time + scene.offset() as f64 - start;
                            if at < 0. || at >= timeline.music_duration {
                                continue;
                            }
                            let clip = match &note.hitsound {
                                HitSound::None => continue,
                                HitSound::Click => &scene.res.res_pack.sfx_click,
                                HitSound::Drag => &scene.res.res_pack.sfx_drag,
                                HitSound::Flick => &scene.res.res_pack.sfx_flick,
                                HitSound::Custom(name) => scene.chart.hitsounds.get(name).unwrap_or(&scene.res.res_pack.sfx_click),
                            };
                            tracks.push(AudioPlacement {
                                clip: clip.clone(),
                                at: playing.first_frame as f64 / options.fps as f64 + at,
                                source_start: 0.,
                                duration: clip.length().min(timeline.music_duration - at),
                                gain: options.presentation.audio.hitsound_volume,
                                fade: 0.,
                            });
                        }
                    }
                }
            }
            let duration = timeline.duration();
            let mut samples = tokio::task::spawn_blocking(move || audio::mix(duration, tracks)).await??;
            for (i, chunk) in samples.chunks_mut(4096).enumerate() {
                for extension in &mut extensions.items {
                    extension
                        .audio((i * 4096) as u64, chunk)
                        .with_context(|| format!("渲染扩展 {} 音频处理失败", extension.id()))?;
                }
            }
            Some(tokio::task::spawn_blocking(move || audio::write(&samples)).await??)
        } else {
            None
        };
        let encoder = if let Some(out) = &out {
            if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            Some(Encoder::spawn(&options, out, soundtrack.as_deref().context("没有可用的视频音轨")?, timeline.duration())?)
        } else {
            None
        };
        let capture = render_target(options.width, options.height);
        let chart_capture = render_target(options.width, options.height);
        capture.texture.set_filter(FilterMode::Linear);
        chart_capture.texture.set_filter(FilterMode::Linear);
        scene.res.camera.render_target = Some(chart_capture);
        let offset = scene.offset() as f64;
        let buf = vec![0; options.width as usize * options.height as usize * 4];
        Ok(Self {
            scene,
            capture,
            chart_capture,
            opts: options,
            timeline,
            assets,
            info,
            tm: TimeManager::manual(Box::new(|| 1000.)),
            bad_notes: vec![],
            offset,
            frame: 0,
            simulated_time: None,
            encoder,
            out_path: out,
            soundtrack,
            buf,
            extensions,
            previous_stage: None,
        })
    }

    pub fn title(&self) -> &str {
        &self.info.name
    }
    pub fn progress(&self) -> (u64, u64) {
        (self.frame, self.timeline.total_frames)
    }
    pub fn duration(&self) -> f64 {
        self.timeline.duration()
    }
    pub fn options(&self) -> &VideoOptions {
        &self.opts
    }
    pub fn timeline(&self) -> &RenderTimeline {
        &self.timeline
    }
    pub fn stage(&self) -> Option<RenderStage> {
        self.timeline
            .frame(self.frame.min(self.timeline.total_frames - 1), self.opts.width, self.opts.height)
            .map(|c| c.stage)
    }
    pub fn texture(&self) -> Texture2D {
        self.capture.texture
    }
    pub fn frame_bytes(&self) -> &[u8] {
        &self.buf
    }
    pub fn finished(&self) -> bool {
        self.frame >= self.timeline.total_frames
    }

    pub fn render_next_frame(&mut self, painter: &mut TextPainter) -> Result<()> {
        let Some(context) = self.timeline.frame(self.frame, self.opts.width, self.opts.height) else {
            return Ok(());
        };
        if let Err(err) = self.draw_frame(painter, context) {
            self.notify(RenderOutcome::Failed);
            return Err(err);
        }
        self.read_pixels();
        if let Some(encoder) = &mut self.encoder {
            if let Err(err) = encoder.write(&self.buf) {
                self.notify(RenderOutcome::Failed);
                return Err(err);
            }
        }
        self.frame += 1;
        Ok(())
    }

    pub fn preview_frame(&mut self, painter: &mut TextPainter, stage: RenderStage, progress: f32) -> Result<()> {
        let mut options = self.opts.presentation.clone();
        options.opening = true;
        options.playing = true;
        options.ending = true;
        let timeline = RenderTimeline::new(&options, self.opts.fps, self.timeline.music_start, self.timeline.music_duration)?;
        let segment = timeline.segment(stage).context("该预览阶段没有时长")?;
        let frame = segment.first_frame + ((segment.frames - 1) as f32 * progress.clamp(0., 1.)).round() as u64;
        let mut context = timeline.frame(frame, self.opts.width, self.opts.height).unwrap();
        context.preview = true;
        self.draw_frame(painter, context)
    }

    fn advance_chart(&mut self, target: f64) {
        if self.simulated_time.is_some_and(|previous| target + 1e-6 < previous) {
            self.scene.chart.reset();
            self.scene.judge.reset();
            self.bad_notes.clear();
            self.scene.res.emitter.emitter.reset();
            self.scene.res.emitter.emitter_square.reset();
            self.simulated_time = None;
        }
        let step = 1. / self.opts.fps as f64;
        let mut next = self.simulated_time.map_or((-self.offset).min(target), |time| time + step);
        while next <= target + 1e-7 {
            self.scene.set_video_frame(next, 1., step as f32);
            self.scene.tick_replay();
            self.scene.judge.update(&mut self.scene.res, &mut self.scene.chart, &mut self.bad_notes);
            self.simulated_time = Some(next);
            next += step;
        }
        self.scene.set_video_frame(target, 1., step as f32);
        self.scene.tick_replay();
        self.tm.seek_to(target);
    }

    fn draw_frame(&mut self, painter: &mut TextPainter, context: FrameContext) -> Result<()> {
        let _font_scale = presentation::FontScale::native();
        if context.stage == RenderStage::Ending
            && !context.preview
            && self.previous_stage != Some(RenderStage::Ending)
            && self.opts.presentation.playing
        {
            self.advance_chart(self.timeline.music_start + self.timeline.music_duration - self.offset);
        }
        if self.previous_stage != Some(context.stage) {
            for extension in &mut self.extensions.items {
                extension
                    .stage_enter(&context)
                    .with_context(|| format!("渲染扩展 {} 阶段切换失败", extension.id()))?;
            }
            self.previous_stage = Some(context.stage);
        }
        let mut behavior = FrameBehavior::default();
        for extension in &mut self.extensions.items {
            extension
                .update(&context, &mut behavior)
                .with_context(|| format!("渲染扩展 {} 帧更新失败", extension.id()))?;
        }
        anyhow::ensure!(
            behavior.opacity.is_finite()
                && [behavior.tint.r, behavior.tint.g, behavior.tint.b, behavior.tint.a]
                    .iter()
                    .all(|v| v.is_finite()),
            "扩展返回了无效的画面参数"
        );
        let viewport = Some((0, 0, self.opts.width as i32, self.opts.height as i32));
        let opening = (context.stage == RenderStage::Opening && self.opts.presentation.playing)
            .then(|| presentation::animation::loading(context.stage_time, context.stage_duration, context.delta));
        let opening_scene = opening.as_ref().is_some_and(|frame| frame.reveal > 0.);
        if let Some(music_time) = context.music_time {
            if context.stage != RenderStage::Entering {
                self.advance_chart(music_time - self.offset);
            } else {
                self.scene.set_video_frame(music_time - self.offset, 1., 0.);
                if !self.opts.presentation.opening {
                    let p = presentation::animation::out_cubic(context.stage_progress);
                    self.scene.res.alpha = p;
                    self.scene.set_video_entrance(p, p);
                }
                self.tm.seek_to(music_time - self.offset);
                self.scene.tick_replay();
            }
            self.scene.set_video_progress(self.timeline.hud_progress(&context, self.scene.res.track_length));
            self.scene.set_replay_frame_delta(if context.preview { 0. } else { context.delta as f32 });
            let mut ui = Ui::new(painter, viewport);
            self.scene.render(&mut self.tm, &mut ui)?;
        } else if opening_scene {
            let frame = opening.as_ref().unwrap();
            let time = self.timeline.music_start - self.offset;
            self.scene.set_video_frame(time, frame.line, 0.);
            self.scene.set_video_entrance(frame.line, frame.hud);
            self.tm.seek_to(time);
            self.scene.tick_replay();
            self.scene.set_video_progress(self.timeline.hud_progress(&context, self.scene.res.track_length));
            let mut ui = Ui::new(painter, viewport);
            self.scene.render(&mut self.tm, &mut ui)?;
        }
        let mut ui = Ui::new(painter, viewport);
        let camera = Camera2D {
            render_target: Some(self.capture),
            ..ui.camera()
        };
        set_camera(&camera);
        clear_background(BLACK);
        presentation::background(&mut ui, &self.assets);
        for layer in [RenderLayer::Background, RenderLayer::BeforeScene] {
            self.draw_extensions(layer, &context, &mut ui)?;
            set_camera(&camera);
        }
        if context.music_time.is_some() || opening_scene {
            let progress = context.stage_progress;
            let phase_alpha = match context.stage {
                RenderStage::Opening => opening.as_ref().unwrap().reveal,
                RenderStage::Entering if !self.opts.presentation.opening => 1. - (1. - progress).powi(3),
                RenderStage::Exiting => (1. - progress).powi(2),
                _ => 1.,
            };
            let area = ui.screen_rect();
            let mut tint = behavior.tint;
            tint.a *= behavior.opacity.clamp(0., 1.) * phase_alpha;
            ui.fill_rect(
                area,
                (self.chart_capture.texture, area, crate::ext::ScaleType::Fit, tint)
                    .into_shading()
                    .flip_y(),
            );
        }
        if context.music_time.is_none() {
            if context.stage == RenderStage::Ending && self.opts.presentation.playing && !context.preview {
                let opacity = (1. - context.stage_time * 6. / context.stage_duration.min(6.)).clamp(0., 1.) as f32;
                if opacity > 0. {
                    let area = ui.screen_rect();
                    ui.alpha(opacity, |ui| {
                        ui.fill_rect(area, (self.chart_capture.texture, area, crate::ext::ScaleType::Fit).into_shading().flip_y())
                    });
                }
            }
            let result = self.display_result(context.preview);
            ui.alpha(behavior.opacity.clamp(0., 1.), |ui| {
                presentation::draw(ui, &context, &self.opts.presentation, &self.info, &self.assets, &result)
            });
        }
        for layer in [RenderLayer::AfterScene, RenderLayer::AfterUi] {
            self.draw_extensions(layer, &context, &mut ui)?;
            set_camera(&camera);
        }
        unsafe { get_internal_gl() }.flush();
        Ok(())
    }

    fn display_result(&self, preview: bool) -> PlayResult {
        if preview || !self.opts.presentation.playing {
            let notes = self.scene.chart.lines.iter().flat_map(|l| &l.notes).filter(|n| !n.fake).count() as u32;
            PlayResult {
                score: 1_000_000,
                accuracy: 1.,
                max_combo: notes,
                num_of_notes: notes,
                counts: [notes, 0, 0, 0, 0, 0, 0, 0],
                ..Default::default()
            }
        } else {
            self.scene.judge.result(false)
        }
    }

    fn draw_extensions(&mut self, layer: RenderLayer, context: &FrameContext, ui: &mut Ui) -> Result<()> {
        for extension in &mut self.extensions.items {
            ui.scope(|ui| extension.draw(layer, context, ui))
                .with_context(|| format!("渲染扩展 {} 绘制失败", extension.id()))?;
        }
        Ok(())
    }

    fn read_pixels(&mut self) {
        unsafe {
            use miniquad::gl::*;
            let mut previous = 0;
            glGetIntegerv(0x8CAA, &mut previous);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, self.capture.render_pass.gl_internal_id(get_internal_gl().quad_context));
            glReadPixels(0, 0, self.opts.width as i32, self.opts.height as i32, GL_RGBA, GL_UNSIGNED_BYTE, self.buf.as_mut_ptr() as _);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, previous as u32);
        }
    }

    fn notify(&mut self, outcome: RenderOutcome) {
        self.extensions.finish(outcome);
    }

    pub fn finish(mut self) -> Result<PathBuf> {
        anyhow::ensure!(self.finished(), "视频尚未渲染完成");
        if let Err(err) = self.encoder.take().context("预览不能保存为视频")?.finish() {
            self.notify(RenderOutcome::Failed);
            return Err(err);
        }
        let out = self.out_path.as_ref().context("没有输出路径")?.clone();
        anyhow::ensure!(std::fs::metadata(&out)?.len() > 0, "没有生成有效的视频文件");
        self.out_path = None;
        self.soundtrack = None;
        self.notify(RenderOutcome::Completed);
        Ok(out)
    }

    pub fn cancel(&mut self) {
        self.encoder = None;
        self.soundtrack = None;
        if let Some(out) = self.out_path.take() {
            let _ = std::fs::remove_file(out);
        }
        self.notify(RenderOutcome::Cancelled);
    }
}

impl Drop for ChartVideoRenderer {
    fn drop(&mut self) {
        self.cancel();
        unsafe { get_internal_gl() }.flush();
        set_default_camera();
        self.scene.res.camera.render_target = None;
        self.capture.delete();
        self.chart_capture.delete();
    }
}
