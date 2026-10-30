use super::*;
use crate::{dir, get_data};
use anyhow::{Context, Result};
use prpr::scene::{request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input};
use std::{borrow::Cow, time::Instant};

impl RenderPage {
    pub fn new() -> Self {
        let prefs = load_prefs();
        Self {
            prefs,
            selected: None,
            pick_request: false,
            category: settings::Category::Output,
            scroll: Scroll::new(),
            status_scroll: Scroll::new(),
            tabs: std::array::from_fn(|_| DRectButton::new().with_radius(0.006)),
            fields: settings::FIELDS.iter().map(|_| DRectButton::new().with_radius(0.006)).collect(),
            pick: DRectButton::new().with_radius(0.008),
            start: DRectButton::new().with_radius(0.008),
            open: DRectButton::new().with_radius(0.008),
            encoder_button: DRectButton::new().with_radius(0.006),
            #[cfg(target_os = "windows")]
            ffmpeg: ffmpeg::Ffmpeg::new(),
            job: Job::Idle,
        }
    }

    pub(super) fn busy(&self) -> bool {
        matches!(self.job, Job::Loading(_) | Job::Rendering { .. })
    }
    fn out_dir(&self) -> PathBuf {
        if self.prefs.out_dir.is_empty() {
            data_root().join("render")
        } else {
            PathBuf::from(&self.prefs.out_dir)
        }
    }
    fn chart_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from(dir::charts().ok()?).join(&self.selected.as_ref()?.local_path))
    }
    fn output_path(&self) -> Option<PathBuf> {
        let selected = self.selected.as_ref()?;
        Some(
            self.out_dir()
                .join(format!("{} [{:.1}].mp4", sanitize(&selected.name), selected.difficulty)),
        )
    }
    fn options(&self) -> VideoOptions {
        let (width, height) = RESOLUTIONS[self.prefs.res];
        VideoOptions {
            width,
            height,
            fps: FPS_CHOICES[self.prefs.fps],
            hitsound: self.prefs.hitsound,
            show_hud: self.prefs.hud,
            sample_count: get_data().config.sample_count.max(1),
            presentation: self.prefs.presentation.clone(),
            ..Default::default()
        }
    }
    fn changed(&mut self) {
        save_prefs(&self.prefs);
    }
    fn cancel(&mut self) {
        if let Job::Rendering { renderer, .. } = &mut self.job {
            renderer.cancel();
        }
        self.job = Job::Idle;
    }

    fn start_export(&mut self) -> Result<()> {
        if self.busy() {
            self.cancel();
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        if !self.ffmpeg.ready() {
            show_message("请先在输出页安装 FFmpeg，并等待检测完成");
            self.category = settings::Category::Output;
            return Ok(());
        }
        let Some(chart) = self.chart_path() else {
            show_message("请先选择谱面");
            return Ok(());
        };
        let out = self.output_path().context("无法生成视频文件名")?;
        let options = self.options();
        options.validate()?;
        self.job = Job::Loading(Box::pin(ChartVideoRenderer::new(chart, options, out)));
        Ok(())
    }

    fn activate_field(&mut self, field: settings::Field) -> Result<()> {
        use settings::Kind;
        let (_, kind, _, _) = field.info();
        match kind {
            Kind::Text | Kind::Number => request_input(
                format!("video-field-{}", field as usize),
                inputbox::InputBox::new().default_text(settings::input_value(&self.prefs, field)),
            ),
            Kind::Folder => {
                #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
                if let Some(path) = rfd::FileDialog::new().set_title("选择视频输出目录").pick_folder() {
                    self.prefs.out_dir = path.display().to_string();
                    self.changed();
                }
                #[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
                show_message("此平台使用默认视频输出目录");
            }
            Kind::Avatar => request_file("render-avatar"),
            Kind::Font => request_file("render-font"),
            _ => {
                settings::change(&mut self.prefs, field, None)?;
                self.changed();
            }
        }
        Ok(())
    }

    fn advance(&mut self, painter: &mut prpr::ui::TextPainter) {
        unsafe { get_internal_gl() }.flush();
        let mut complete = false;
        let mut failure = None;
        if let Job::Rendering { renderer, .. } = &mut self.job {
            let started = Instant::now();
            while !renderer.finished() {
                if let Err(err) = renderer.render_next_frame(painter) {
                    failure = Some(format!("{err:#}"));
                    break;
                }
                if started.elapsed() >= BUDGET {
                    break;
                }
            }
            complete = renderer.finished();
        }
        if let Some(err) = failure {
            self.job = Job::Failed(err);
        } else if complete {
            let job = std::mem::replace(&mut self.job, Job::Idle);
            if let Job::Rendering { renderer, started } = job {
                let frames = renderer.progress().0;
                self.job = match renderer.finish() {
                    Ok(path) => Job::Done {
                        path: publish_output(path),
                        frames,
                        secs: started.elapsed().as_secs_f64(),
                    },
                    Err(err) => Job::Failed(format!("{err:#}")),
                };
            }
        }
        unsafe { get_internal_gl() }.flush();
    }
}

impl Page for RenderPage {
    fn label(&self) -> Cow<'static, str> {
        "谱面渲染".into()
    }
    fn can_play_bgm(&self) -> bool {
        !self.busy()
    }
    fn take_render_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.pick_request)
    }
    fn on_render_chart_picked(&mut self, local_path: String) {
        if let Some(chart) = get_data().charts.iter().find(|c| c.local_path == local_path) {
            self.selected = Some(Selected {
                local_path,
                name: chart.info.name.clone(),
                difficulty: chart.info.difficulty,
                subtitle: [chart.info.composer.as_str(), chart.info.level.as_str(), chart.info.charter.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" / "),
            });
        }
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        self.status_scroll.update(s.t);
        #[cfg(target_os = "windows")]
        self.ffmpeg.update();
        if let Some((id, text)) = take_input() {
            if let Some(field) = id
                .strip_prefix("video-field-")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| settings::FIELDS.get(i))
                .copied()
            {
                if !self.busy() {
                    match settings::change(&mut self.prefs, field, Some(&text)) {
                        Ok(()) => self.changed(),
                        Err(err) => {
                            show_message(format!("设置未保存：{err}"));
                        }
                    }
                }
            } else {
                return_input(id, text);
            }
        }
        if let Some((id, path)) = take_file() {
            if id == "render-avatar" {
                if !path.is_empty() && !self.busy() {
                    self.prefs.presentation.result.avatar = Some(path.into());
                    self.changed();
                }
            } else if id == "render-font" {
                if !path.is_empty() && !self.busy() {
                    self.prefs.presentation.font = Some(path.into());
                    self.changed();
                }
            } else {
                return_file(id, path);
            }
        }
        if let Job::Loading(future) = &mut self.job {
            if let Some(result) = prpr::ext::poll_future(future.as_mut()) {
                self.job = match result {
                    Ok(renderer) => Job::Rendering {
                        renderer,
                        started: Instant::now(),
                    },
                    Err(err) => Job::Failed(format!("{err:#}")),
                };
            }
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) || self.status_scroll.touch(touch, t) {
            for button in self.tabs.iter_mut().chain(self.fields.iter_mut()) {
                button.cancel(t);
            }
            for button in [&mut self.pick, &mut self.start, &mut self.open, &mut self.encoder_button] {
                button.cancel(t);
            }
            return Ok(true);
        }
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if tab.touch(touch, t) {
                self.category = settings::CATEGORIES[i];
                self.scroll.y_scroller.reset();
                return Ok(true);
            }
        }
        if self.start.touch(touch, t) {
            if let Err(err) = self.start_export() {
                show_error(err);
            }
            return Ok(true);
        }
        if !self.busy() && self.pick.touch(touch, t) {
            self.pick_request = true;
            return Ok(true);
        }
        if self.open.touch(touch, t) {
            #[cfg(not(target_os = "android"))]
            {
                let path = self.out_dir();
                std::fs::create_dir_all(&path)?;
                if let Err(err) = open_in_file_manager(&path) {
                    show_error(err.into());
                }
            }
            return Ok(true);
        }
        if self.scroll.contains(touch) && !self.busy() {
            #[cfg(target_os = "windows")]
            if self.encoder_button.touch(touch, t) {
                self.ffmpeg.action();
                return Ok(true);
            }
            for (i, button) in self.fields.iter_mut().enumerate() {
                if button.touch(touch, t) {
                    if let Err(err) = self.activate_field(settings::FIELDS[i]) {
                        show_message(format!("设置未保存：{err}"));
                    }
                    return Ok(true);
                }
            }
        } else {
            for button in &mut self.fields {
                button.cancel(t);
            }
            self.encoder_button.cancel(t);
        }
        Ok(false)
    }

    fn render(&mut self, ui: &mut prpr::ui::Ui, s: &mut SharedState) -> Result<()> {
        let camera = ui.camera();
        self.advance(ui.text_painter);
        set_camera(&camera);
        self.render_view(ui, s);
        Ok(())
    }
}
