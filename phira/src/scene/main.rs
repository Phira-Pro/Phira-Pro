use super::{import_chart, L10N_LOCAL};
use crate::{
    charts_view::NEED_UPDATE,
    data::LocalChart,
    deeplink::{self, DeepLink, DeepLinkChartOpening, DeepLinkDownload, DeepLinkTarget},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{ChartItem, ExportInfo, HomePage, NextPage, Page, ResPackItem, SharedState},
    save_data,
    scene::{
        confirm_dialog, import_chart_to, parse_warnings_to_string, SongScene, TEX_BACKGROUND, TEX_BACKGROUND_BLUR, TEX_BACKGROUND_BLUR_DEFAULT,
        TEX_BACKGROUND_DEFAULT, TEX_ICON_BACK,
    },
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use prpr::{
    core::ResPackInfo,
    ext::{unzip_into, SafeTexture, ScaleType},
    info::ChartInfo,
    parse::ParseWarnings,
    scene::{return_file, show_error, show_message, take_file, NextScene, Scene, DIALOG},
    task::Task,
    time::TimeManager,
    ui::{button_hit, Dialog, FontArc, RectButton, Ui, UI_AUDIO},
};
use sasa::{AudioClip, Music};
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    mem,
    path::{Component, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread_local,
    time::{Duration, Instant},
};
use tempfile::tempfile;
use uuid::Uuid;

const LOW_PASS: f32 = 0.95;

/// Phira Pro：把图片缩到很小再当纹理放大画出来（线性过滤）＝一次廉价的模糊。
/// 供「服务器列表」浮层当磨砂背景用，避免浮层内容与背后界面糊在一起。
fn blurred_texture(image: &image::DynamicImage) -> SafeTexture {
    const W: u32 = 32;
    const H: u32 = 18;
    let small = image.resize_exact(W, H, image::imageops::FilterType::Triangle).into_rgba8();
    let tex = Texture2D::from_rgba8(W as u16, H as u16, small.as_raw());
    tex.set_filter(FilterMode::Linear);
    tex.into()
}

pub static BGM_VOLUME_UPDATED: AtomicBool = AtomicBool::new(false);

/// Phira Pro：自定义背景音乐被导入 / 恢复默认后置位；主场景在 `update` 里消费并换曲。
pub static BGM_UPDATED: AtomicBool = AtomicBool::new(false);

/// 外观资源（例如立绘）被导入或替换后置位；主页在 `update` 里消费它并重新加载。
pub static APPEARANCE_UPDATED: AtomicBool = AtomicBool::new(false);

/// Phira Pro：背景图被导入 / 恢复默认后置位；主场景在 `update` 里消费并重新加载。
pub static BACKGROUND_UPDATED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static RESPACK_ITEM: RefCell<Option<ResPackItem>> = RefCell::default();
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> *mut std::ffi::c_void;
    fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, pid: *mut u32) -> u32;
}

/// 当前前台窗口是否属于本进程（仅 Windows 可判定；其它平台视为始终聚焦）。
#[cfg(target_os = "windows")]
fn window_focused() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid == std::process::id()
    }
}

#[cfg(not(target_os = "windows"))]
fn window_focused() -> bool {
    true
}

/// 菜单静置降帧。
///
/// 满帧时（本机 165Hz）菜单静置也要占约 13% 单核 CPU，且其中大部分开销随帧数线性
/// 增长（事件轮询、缓冲交换、驱动工作线程）。因此这里按「有没有人在看」分四档：
/// - 有输入 → 满帧；
/// - 前台静置 → `TARGET_PERIOD`；
/// - 不在前台（可能被遮挡或在副屏）→ `BACKGROUND_PERIOD`；
/// - 已最小化（帧缓冲塌缩为 1×1，完全不可见）→ `MINIMIZED_PERIOD`。
///
/// 该逻辑只挂在 `MainScene`（菜单）上，谱面游玩场景 `SongScene` 完全不受影响。
struct IdleFps {
    last_frame_at: Instant,
    last_input_at: Instant,
    last_sleep: Duration,
    mouse: (f32, f32),
}

impl IdleFps {
    const IDLE_AFTER: Duration = Duration::from_secs(2);
    // Foreground pacing belongs to the display/VSync. A 16ms idle sleep
    // quantizes a 120Hz display to 60Hz and also stalls menu animations.
    const TARGET_PERIOD: Duration = Duration::ZERO;
    const BACKGROUND_PERIOD: Duration = Duration::from_millis(33);
    const MINIMIZED_PERIOD: Duration = Duration::from_millis(100);

    fn new() -> Self {
        let now = Instant::now();
        Self {
            last_frame_at: now,
            last_input_at: now,
            last_sleep: Duration::ZERO,
            mouse: mouse_position(),
        }
    }

    fn settle(&mut self) {
        let now = Instant::now();
        let mouse = mouse_position();
        let touching = !touches().is_empty();
        let down = is_mouse_button_down(MouseButton::Left) || is_mouse_button_down(MouseButton::Right);
        let key = get_last_key_pressed().is_some();
        if touching || down || key || mouse != self.mouse {
            self.last_input_at = now;
        }
        self.mouse = mouse;

        let period = now.saturating_duration_since(self.last_frame_at);
        self.last_frame_at = now;

        // 扣掉上一次的睡眠，剩下的就是本帧真实耗时；据此把帧周期补足到目标值。
        let work = period.saturating_sub(self.last_sleep);
        let target = if now.saturating_duration_since(self.last_input_at) < Self::IDLE_AFTER {
            Duration::ZERO
        } else if screen_width() <= 1. || screen_height() <= 1. {
            Self::MINIMIZED_PERIOD
        } else if !window_focused() {
            Self::BACKGROUND_PERIOD
        } else {
            Self::TARGET_PERIOD
        };
        let sleep = target.saturating_sub(work);
        self.last_sleep = sleep;
        if !sleep.is_zero() {
            std::thread::sleep(sleep);
        }
    }
}

pub struct MainScene {
    state: SharedState,

    bgm: Option<Music>,
    bgm_normalization: f32,
    /// 内置背景音乐的字节（「恢复默认背景音乐」时回退用；open 构建没有）。
    bgm_default: Option<Vec<u8>>,

    background: SafeTexture,
    /// Phira Pro：内置背景图（含磨砂版），供「恢复默认背景」使用。
    bg_default: SafeTexture,
    bg_default_blur: SafeTexture,
    btn_back: RectButton,
    icon_back: SafeTexture,

    pages: Vec<Box<dyn Page>>,

    idle_fps: IdleFps,

    import_task: Option<Task<Result<(LocalChart, ParseWarnings)>>>,

    // deeplink import
    deeplink_pending: Option<DeepLinkTarget>,
    deeplink_confirm: Arc<AtomicBool>,
    deeplink_dl: Option<DeepLinkDownload>,

    // deeplink chart (open the details page of a chart by id)
    deeplink_chart: Option<DeepLinkChartOpening>,
    deeplink_scene: Option<NextScene>,

    icons: Arc<Icons>,

    // batch import
    batch_import_confirm: Arc<AtomicBool>,
    batch_import: Option<(String, ExportInfo)>,
    batch_import_task: Option<Task<Result<()>>>,
    batch_import_rx: Option<mpsc::Receiver<ImportChart>>,
    batch_imported_charts: Vec<ImportChart>,
    batch_import_total: usize,
}

enum ImportChart {
    Imported(Box<LocalChart>, ParseWarnings),
    Skipped(String),
    Failed(String),
}


/// 用音频字节创建一首可循环播放的背景音乐。返回 (music, 响度归一化系数)。
fn build_bgm(bytes: &[u8], loop_mix_time: f64) -> Result<(Music, f32)> {
    let clip = AudioClip::new(bytes.to_vec())?;
    let gain = prpr::audio::music_normalization_gain(&clip);
    let config = &get_data().config;
    let amplifier = config.music_volume(config.volume_bgm) * if config.uniform_loudness { gain } else { 1. };
    let music = UI_AUDIO.with(|it| {
        it.borrow_mut().create_music(
            clip,
            sasa::MusicParams {
                amplifier,
                loop_mix_time,
                command_buffer_size: 64,
                ..Default::default()
            },
        )
    })?;
    Ok((music, gain))
}

impl MainScene {
    // shall be call exactly once
    pub async fn new(fallback: FontArc) -> Result<Self> {
        Self::init().await?;
        crate::hud::selftest();

        // 内置背景音乐：只有 closed 构建自带 `res/bgm`（「恢复默认背景音乐」时用它）。
        #[cfg(closed)]
        let bgm_default: Option<Vec<u8>> = Some(crate::load_res("res/bgm").await);
        #[cfg(not(closed))]
        let bgm_default: Option<Vec<u8>> = None;
        // Phira Pro：自定义背景音乐（`data/appearance/bgm.*`）优先。
        // 自定义的那首按原样接循环（loop_mix_time = 0），内置 `res/bgm` 保留原本的 5.46 秒交叉循环。
        let custom_bgm = dir::load_appearance_audio("bgm");
        let loop_mix_time = if custom_bgm.is_some() { 0. } else { 5.46 };
        let (bgm, bgm_normalization) = match custom_bgm.or_else(|| bgm_default.clone()) {
            Some(bytes) => match build_bgm(&bytes, loop_mix_time) {
                Ok((music, gain)) => (Some(music), gain),
                Err(err) => {
                    tracing::warn!(?err, "failed to load background music");
                    (None, 1.)
                }
            },
            None => (None, 1.),
        };

        let mut sf = Self::new_inner(bgm, fallback).await?;
        sf.bgm_normalization = bgm_normalization;
        sf.pages.push(Box::new(HomePage::new(Arc::clone(&sf.icons)).await?));
        Ok(sf)
    }

    async fn init() -> Result<()> {
        prpr::ui::UI_SFX_VOLUME.store(get_data().config.volume_sfx.to_bits(), Ordering::Relaxed);
        // init button hitsound
        macro_rules! load_sfx {
            ($name:ident, $path:literal) => {{
                let clip = AudioClip::new(load_file($path).await?)?;
                let sound = UI_AUDIO.with(|it| it.borrow_mut().create_sfx(clip, None))?;
                prpr::ui::$name.with(|it| *it.borrow_mut() = Some(sound));
            }};
        }
        load_sfx!(UI_BTN_HITSOUND_LARGE, "button_large.ogg");
        load_sfx!(UI_BTN_HITSOUND, "button.ogg");
        load_sfx!(UI_SWITCH_SOUND, "switch.ogg");

        // 内置背景图（「恢复默认背景」时用）。
        let bg_default: SafeTexture = load_texture("background.jpg").await?.into();
        let bg_default_blur = load_file("background.jpg")
            .await
            .ok()
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|image| blurred_texture(&image))
            .unwrap_or_else(|| bg_default.clone());
        // 自定义背景（`data/appearance/background.*`）优先。
        let bg_image = dir::load_appearance_image("background");
        let (background, background_blur): (SafeTexture, SafeTexture) = match &bg_image {
            Some(image) => (image.clone().into(), blurred_texture(image)),
            None => (bg_default.clone(), bg_default_blur.clone()),
        };
        let icon_back: SafeTexture = load_texture("back.png").await?.into();

        TEX_BACKGROUND.with(|it| *it.borrow_mut() = Some(background));
        TEX_ICON_BACK.with(|it| *it.borrow_mut() = Some(icon_back));
        TEX_BACKGROUND_BLUR.with(|it| *it.borrow_mut() = Some(background_blur));
        TEX_BACKGROUND_DEFAULT.with(|it| *it.borrow_mut() = Some(bg_default));
        TEX_BACKGROUND_BLUR_DEFAULT.with(|it| *it.borrow_mut() = Some(bg_default_blur));

        Ok(())
    }

    async fn new_inner(bgm: Option<Music>, fallback: FontArc) -> Result<Self> {
        let state = SharedState::new(fallback).await?;
        Ok(Self {
            state,

            bgm,
            bgm_normalization: 1.,
            bgm_default: None,

            background: TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap()),
            bg_default: TEX_BACKGROUND_DEFAULT.with(|it| it.borrow().clone().unwrap()),
            bg_default_blur: TEX_BACKGROUND_BLUR_DEFAULT.with(|it| it.borrow().clone().unwrap()),
            btn_back: RectButton::new(),
            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),

            pages: Vec::new(),

            import_task: None,

            deeplink_pending: None,
            deeplink_confirm: Arc::new(AtomicBool::new(false)),
            deeplink_dl: None,

            deeplink_chart: None,
            deeplink_scene: None,

            icons: Arc::new(Icons::new().await?),

            batch_import_confirm: Arc::default(),
            batch_import: None,
            batch_import_task: None,
            batch_import_rx: None,
            batch_imported_charts: Vec::new(),
            batch_import_total: 0,

            idle_fps: IdleFps::new(),
        })
    }

    fn pop(&mut self) {
        if !self.pages.last().unwrap().can_play_bgm() && self.pages[self.pages.len() - 2].can_play_bgm() {
            if let Some(bgm) = &mut self.bgm {
                let _ = bgm.fade_in(0.5);
            }
        }
        self.state.fader.back(self.state.t);
    }

    pub fn take_imported_respack() -> Option<ResPackItem> {
        RESPACK_ITEM.with(|it| it.borrow_mut().take())
    }
}

impl Scene for MainScene {
    fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn Any>) -> Result<()> {
        self.pages.last_mut().unwrap().on_result(result, &mut self.state)
    }

    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            let _ = bgm.fade_in(1.3);
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().enter(&mut self.state)?;
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        if let Some(bgm) = &mut self.bgm {
            bgm.play()?;
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().resume()?;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.pause();
        if let Some(bgm) = &mut self.bgm {
            bgm.pause()?;
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().pause()?;
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.state.fader.transiting() {
            return Ok(false);
        }
        if self.import_task.is_some() {
            return Ok(true);
        }
        if self.deeplink_dl.is_some() {
            let t = tm.real_time() as f32;
            let cancelled = self.deeplink_dl.as_mut().is_some_and(|dl| dl.touch(touch, t));
            if cancelled {
                // dropping the overlay aborts the transfer
                self.deeplink_dl = None;
            }
            return Ok(true);
        }
        if self.deeplink_chart.is_some() {
            let t = tm.real_time() as f32;
            let cancelled = self.deeplink_chart.as_mut().is_some_and(|it| it.touch(touch, t));
            if cancelled {
                // dropping the overlay discards the fetch
                self.deeplink_chart = None;
            }
            return Ok(true);
        }

        let s = &mut self.state;
        s.update(tm);
        if crate::hud::edit_active() {
            return Ok(crate::hud::editor_touch(touch));
        }
        if self.pages.last_mut().unwrap().touch(touch, s)? {
            return Ok(true);
        }
        if self.btn_back.touch(touch) && self.pages.len() > 1 {
            button_hit();
            if !self.pages.last_mut().unwrap().on_back_pressed(&mut self.state) {
                if self.pages.len() == 2 {
                    if let Some(bgm) = &mut self.bgm {
                        bgm.set_low_pass(0.)?;
                    }
                }
                self.pop();
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        /// HUD 编辑器请求切页：重建页面栈（根页面保留、不重建，因此不需要异步）。
        fn goto_hud_page(pages: &mut Vec<Box<dyn Page>>, icons: &Arc<Icons>, state: &mut SharedState, p: crate::hud::PageId) -> Result<()> {
            use crate::hud::PageId;
            pages.truncate(1);
            crate::hud::set_cur_page(p);
            match p {
                PageId::Home => {}
                PageId::Library => {
                    let page = crate::page::LibraryPage::new(Arc::clone(icons), state.icons.clone())?;
                    pages.push(Box::new(page));
                }
                PageId::Settings => {
                    let page = crate::page::SettingsPage::new(icons.icon.clone(), icons.lang.clone());
                    pages.push(Box::new(page));
                }
                PageId::Favorites => {
                    let page = crate::page::FavoritesPage::new(Arc::clone(icons), state.icons.clone(), None, None);
                    pages.push(Box::new(page));
                }
                PageId::Message => {
                    let page = crate::page::MessagePage::new(Arc::clone(icons), state.icons.clone());
                    pages.push(Box::new(page));
                }
                PageId::History => {
                    pages.push(Box::new(crate::page::HistoryPage::new()));
                }
                PageId::Respack => {
                    let page = crate::page::ResPackPage::new(Arc::clone(icons))?;
                    pages.push(Box::new(page));
                }
                PageId::Blacklist => {
                    pages.push(Box::new(crate::page::BlacklistPage::new()));
                }
            }
            pages.last_mut().unwrap().enter(state)?;
            Ok(())
        }
        if let Some(p) = crate::hud::take_goto() {
            goto_hud_page(&mut self.pages, &self.icons, &mut self.state, p)?;
        }
        UI_AUDIO.with(|it| it.borrow_mut().recover_if_needed())?;
        let s = &mut self.state;
        s.update(tm);
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            self.pages[pos].update(s)?;
        }
        self.pages.last_mut().unwrap().update(s)?;
        if !s.fader.transiting() {
            match self.pages.last_mut().unwrap().next_page() {
                NextPage::Overlay(mut sub) => {
                    if self.pages.len() == 1 {
                        if let Some(bgm) = &mut self.bgm {
                            bgm.set_low_pass(LOW_PASS)?;
                        }
                    }
                    sub.enter(s)?;
                    if !sub.can_play_bgm() {
                        if let Some(bgm) = &mut self.bgm {
                            let _ = bgm.fade_out(0.5);
                        }
                    }
                    self.pages.push(sub);
                    s.fader.sub(s.t);
                }
                NextPage::Pop => {
                    self.pop();
                }
                NextPage::None => {}
            }
        } else if let Some(true) = s.fader.done(s.t) {
            self.pages.pop().unwrap().exit()?;
            self.pages.last_mut().unwrap().enter(s)?;
        }
        // Phira Pro：自定义背景被导入 / 恢复默认 → 就地重载背景与磨砂背景。
        if BACKGROUND_UPDATED.swap(false, Ordering::Relaxed) {
            let (bg, blur) = match dir::load_appearance_image("background") {
                Some(image) => (image.clone().into(), blurred_texture(&image)),
                None => (self.bg_default.clone(), self.bg_default_blur.clone()),
            };
            self.background = bg;
            TEX_BACKGROUND.with(|it| *it.borrow_mut() = Some(self.background.clone()));
            TEX_BACKGROUND_BLUR.with(|it| *it.borrow_mut() = Some(blur));
        }
        // Phira Pro：自定义背景音乐被导入 / 恢复默认 → 就地换一首（自定义优先，没有就回内置）。
        if BGM_UPDATED.swap(false, Ordering::Relaxed) {
            self.bgm = None;
            self.bgm_normalization = 1.;
            let custom_bgm = dir::load_appearance_audio("bgm");
            let loop_mix_time = if custom_bgm.is_some() { 0. } else { 5.46 };
            if let Some(bytes) = custom_bgm.or_else(|| self.bgm_default.clone()) {
                match build_bgm(&bytes, loop_mix_time) {
                    Ok((music, gain)) => {
                        self.bgm = Some(music);
                        self.bgm_normalization = gain;
                        if self.pages.last().map(|it| it.can_play_bgm()).unwrap_or(false) {
                            if let Some(bgm) = &mut self.bgm {
                                let _ = bgm.fade_in(0.5);
                            }
                        }
                    }
                    Err(err) => tracing::warn!(?err, "failed to load background music"),
                }
            }
        }
        if let Some(bgm) = &mut self.bgm {
            if BGM_VOLUME_UPDATED.fetch_and(false, Ordering::Relaxed) {
                let config = &get_data().config;
                bgm.set_amplifier(config.music_volume(config.volume_bgm) * if config.uniform_loudness { self.bgm_normalization } else { 1. })?;
            }
        }
        if let Some(task) = &mut self.import_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(itl!("import-failed")));
                    }
                    Ok((chart, warnings)) => {
                        if let Some(warn) = parse_warnings_to_string(&warnings) {
                            Dialog::plain(itl!("warning"), warn).show();
                        }
                        show_message(itl!("import-success")).ok();
                        get_data_mut().charts.push(chart);
                        save_data()?;
                        self.state.reload_local_charts();
                        NEED_UPDATE.store(true, Ordering::Relaxed);
                    }
                }
                self.import_task = None;
            }
        }
        if let Some((id, file)) = take_file() {
            match id.as_str() {
                "_import_auto" => {
                    let new_id = match File::open(&file).map(BufReader::new).map(zip::ZipArchive::new) {
                        Ok(Ok(zip)) => {
                            if zip.file_names().any(|name| name.ends_with("click.png")) {
                                "_import_respack"
                            } else {
                                "_import"
                            }
                        }
                        _ => "_import",
                    };
                    return_file(new_id.to_owned(), file);
                }
                "_import" => {
                    let export_info = (|| -> Result<Option<(ExportInfo, usize)>> {
                        let file = File::open(&file)?;
                        let mut archive = zip::ZipArchive::new(file)?;
                        let export_info = match archive.by_name("export.json") {
                            Err(zip::result::ZipError::FileNotFound) => {
                                return Ok(None);
                            }
                            Err(err) => {
                                return Err(err.into());
                            }
                            Ok(file) => serde_json::from_reader(file)?,
                        };
                        let mut count = 0;
                        for i in 0..archive.len() {
                            let file = archive.by_index(i)?;
                            if file.enclosed_name().is_some_and(|it| it.extension().is_some_and(|ext| ext == "zip")) {
                                count += 1;
                            }
                        }
                        Ok(Some((export_info, count)))
                    })();
                    match export_info {
                        Err(err) => {
                            show_error(err.context(itl!("import-failed")));
                        }
                        Ok(None) => {
                            self.import_task = Some(Task::new(async move {
                                let file = File::open(&file).context("cannot open file")?;
                                import_chart(file).await
                            }));
                        }
                        Ok(Some((info, count))) => {
                            self.batch_import = Some((file, info));
                            self.batch_import_total = count;
                            confirm_dialog(itl!("batch-import"), itl!("batch-import-confirm", "count" => count), self.batch_import_confirm.clone());
                        }
                    };
                }
                "_import_respack" => {
                    let root = dir::respacks()?;
                    let dir = prpr::dir::Dir::new(&root)?;
                    let mut dir_id: Option<String> = None;
                    let item: Result<ResPackItem> = (|| {
                        let config = {
                            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(&file)?))?;
                            let config: ResPackInfo =
                                serde_yaml::from_reader(zip.by_name("info.yml").context("missing info.yml")?).context("invalid info.yml")?;
                            config.verify()?;
                            let mut buffer = Vec::new();
                            for file in [
                                "click.png",
                                "click_mh.png",
                                "drag.png",
                                "drag_mh.png",
                                "flick.png",
                                "flick_mh.png",
                                "hold.png",
                                "hold_mh.png",
                                "hit_fx.png",
                            ] {
                                let mut entry = zip.by_name(file).with_context(|| format!("missing file: {file}"))?;
                                buffer.clear();
                                entry.read_to_end(&mut buffer)?;
                                image::load_from_memory(&buffer).with_context(|| format!("failed to load image: {file}"))?;
                            }

                            for audio in ["click", "drag", "flick", "ending"] {
                                for ext in [".ogg", ".wav", ".mp3"] {
                                    let mut entry = match zip.by_name(format!("{audio}{ext}").as_str()) {
                                        Err(zip::result::ZipError::FileNotFound) => continue,
                                        Err(err) => return Err(err.into()),
                                        Ok(file) => file,
                                    };
                                    buffer.clear();
                                    entry.read_to_end(&mut buffer)?;
                                    AudioClip::new(mem::take(&mut buffer)).with_context(|| format!("failed to load audio: {audio}"))?;
                                    break;
                                }
                            }
                            config
                        };

                        let mut uuid = Uuid::new_v4();
                        while dir.exists(uuid.to_string())? {
                            uuid = Uuid::new_v4();
                        }
                        let id = uuid.to_string();
                        dir.create_dir_all(&id)?;
                        let dir = dir.open_dir(&id)?;
                        dir_id = Some(id.clone());
                        unzip_into(BufReader::new(File::open(file)?), &dir, false).context("failed to unzip")?;
                        get_data_mut().respacks.push(id.clone());
                        save_data()?;
                        Ok(ResPackItem::new(Some(format!("{root}/{id}").into()), config.name))
                    })();
                    match item {
                        Err(err) => {
                            show_error(err.context(itl!("import-respack-failed")));
                            if let Some(id) = &dir_id {
                                dir.remove_dir_all(id)?;
                            }
                        }
                        Ok(item) => {
                            RESPACK_ITEM.with(|it| *it.borrow_mut() = Some(item));
                            show_message(itl!("import-respack-success"));
                        }
                    }
                }
                _ => return_file(id, file),
            }
        }
        // Wait until any dialog is gone and no import is running: `Dialog::show`
        // replaces the current dialog, and a pending deeplink can simply wait.
        if self.deeplink_dl.is_none()
            && self.import_task.is_none()
            && self.deeplink_chart.is_none()
            && self.deeplink_scene.is_none()
            && DIALOG.with(|it| it.borrow().is_none())
        {
            if let Some(input) = deeplink::take_deeplink() {
                match deeplink::parse_deeplink(&input) {
                    Err(err) => {
                        show_error(err.context(itl!("deeplink-bad-url")));
                    }
                    Ok(DeepLink::Chart(id)) => {
                        // Viewing a chart's details is safe (same as tapping a
                        // chart in a message), so no confirmation is needed.
                        self.deeplink_chart = Some(deeplink::start_chart_opening(id));
                    }
                    Ok(DeepLink::Import(target)) => {
                        let message = if target.official {
                            format!("{}\n{}", itl!("deeplink-confirm"), target.url)
                        } else {
                            format!(
                                "{}\n\n{}\n{}",
                                itl!("deeplink-unofficial", "host" => deeplink::official_host()),
                                itl!("deeplink-confirm"),
                                target.url
                            )
                        };
                        Dialog::plain(itl!("deeplink-title"), message)
                            .buttons(vec![ttl!("cancel").into_owned(), itl!("deeplink-download").into_owned()])
                            .listener({
                                let res = self.deeplink_confirm.clone();
                                move |_dialog, id| {
                                    if id == -1 {
                                        return true;
                                    }
                                    if id == 1 {
                                        res.store(true, Ordering::SeqCst);
                                    }
                                    false
                                }
                            })
                            .show();
                        self.deeplink_pending = Some(target);
                    }
                }
            }
        }
        if let Some(res) = self.deeplink_chart.as_mut().and_then(|it| it.take_result()) {
            match res {
                Err(err) => show_error(err.context(itl!("deeplink-open-failed"))),
                Ok(chart) => {
                    let (local_path, mods) = {
                        let data = get_data();
                        data.charts
                            .iter()
                            .find(|it| it.info.id == Some(chart.id))
                            .map(|it| (Some(it.local_path.clone()), it.mods))
                            .unwrap_or_default()
                    };
                    self.deeplink_scene = Some(NextScene::Overlay(Box::new(SongScene::new(
                        ChartItem::from_remote(chart.as_ref()),
                        local_path,
                        Arc::clone(&self.icons),
                        self.state.icons.clone(),
                        mods,
                    ))));
                }
            }
            self.deeplink_chart = None;
        }
        if self.deeplink_confirm.load(Ordering::Relaxed) && self.deeplink_dl.is_none() && self.import_task.is_none() {
            self.deeplink_confirm.store(false, Ordering::Relaxed);
            if let Some(target) = self.deeplink_pending.take() {
                self.deeplink_dl = Some(deeplink::start_deeplink_download(target)?);
            }
        }
        let dl_result = self.deeplink_dl.as_mut().and_then(|dl| dl.take_result());
        if let Some(res) = dl_result {
            match res {
                Ok(file) => self.import_task = Some(Task::new(import_chart(file))),
                Err(err) => show_error(err.context(itl!("deeplink-dl-failed"))),
            }
            self.deeplink_dl = None;
        }
        if self.batch_import_confirm.swap(false, Ordering::Relaxed) {
            if let Some((file, _info)) = self.batch_import.take() {
                let (tx, rx) = mpsc::channel();
                self.batch_import_rx = Some(rx);
                self.batch_imported_charts.clear();
                let registered: std::collections::HashSet<_> = get_data().charts.iter().map(|it| it.local_path.clone()).collect();
                self.batch_import_task = Some(Task::new(async move {
                    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(&file)?))?;
                    let charts_dir = dir::charts()?;
                    for i in 0..archive.len() {
                        let mut file = archive.by_index(i)?;
                        let Some(name) = file.enclosed_name() else {
                            continue;
                        };
                        if name.extension().is_none_or(|it| it != "zip") {
                            continue;
                        }
                        let [Component::Normal(dir), Component::Normal(name)] = name.components().collect::<Vec<_>>()[..] else {
                            continue;
                        };
                        let mut to_tempfile = || -> std::io::Result<_> {
                            let mut tf = tempfile()?;
                            std::io::copy(&mut file, &mut tf)?;
                            tf.seek(SeekFrom::Start(0))?;
                            Ok(tf)
                        };
                        let result: Result<Option<ImportChart>> = async {
                            match dir.to_str() {
                                Some("custom") => {
                                    let tf = to_tempfile()?;
                                    let (chart, warnings) = import_chart(tf)
                                        .await
                                        .with_context(|| itl!("batch-import-failed-chart", "chart" => name.display().to_string()))?;
                                    Ok(Some(ImportChart::Imported(Box::new(chart), warnings)))
                                }
                                Some("download") => {
                                    let Some(id) = name.to_str().and_then(|it| it.strip_suffix(".zip")).and_then(|it| it.parse::<i32>().ok()) else {
                                        warn!("invalid batch import download id: {:?}", name);
                                        return Ok(None);
                                    };
                                    let local_path = format!("download/{id}");
                                    let path = PathBuf::from(format!("{charts_dir}/{local_path}"));
                                    if registered.contains(&local_path) {
                                        if let Ok(info) = File::open(path.join("info.yml"))
                                            .map_err(anyhow::Error::from)
                                            .and_then(|file| serde_yaml::from_reader::<_, ChartInfo>(file).map_err(Into::into))
                                        {
                                            if [&info.chart, &info.music, &info.illustration]
                                                .into_iter()
                                                .all(|asset| path.join(asset).is_file())
                                            {
                                                return Ok(Some(ImportChart::Skipped(info.name)));
                                            }
                                        }
                                    }
                                    // A previous failed import may have left a directory
                                    // without info.yml. Validate the new archive elsewhere.
                                    let parent = path.parent().unwrap();
                                    std::fs::create_dir_all(parent)?;
                                    let staging = tempfile::Builder::new().prefix(".import-").tempdir_in(parent)?;
                                    let tf = to_tempfile()?;
                                    let (chart, warnings) = import_chart_to(staging.path(), local_path, tf)
                                        .await
                                        .with_context(|| itl!("batch-import-failed-chart", "chart" => name.display().to_string()))?;
                                    crate::chart_install::publish(staging.path(), &path)?;
                                    Ok(Some(ImportChart::Imported(Box::new(chart), warnings)))
                                }
                                _ => {
                                    warn!("invalid batch import dir: {:?}", dir);
                                    Ok(None)
                                }
                            }
                        }
                        .await;
                        let result = match result {
                            Ok(None) => continue,
                            Ok(Some(result)) => result,
                            Err(error) => ImportChart::Failed(format!("{}: {error:#}", name.display())),
                        };
                        let _ = tx.send(result);
                    }
                    Ok(())
                }));
            }
        }

        if let Some(rx) = &mut self.batch_import_rx {
            while let Ok(chart) = rx.try_recv() {
                self.batch_imported_charts.push(chart);
            }
        }

        if let Some(task) = &mut self.batch_import_task {
            if let Some(res) = task.take() {
                // The worker may finish after the first drain. Keep every
                // imported chart before dropping its result channel.
                if let Some(rx) = &self.batch_import_rx {
                    self.batch_imported_charts.extend(rx.try_iter());
                }
                {
                    let mut warning_messages = vec![];
                    let mut failed = vec![];
                    let data = get_data_mut();
                    let mut count = 0;
                    let mut skipped = String::new();
                    for chart in self.batch_imported_charts.drain(..) {
                        match chart {
                            ImportChart::Imported(chart, warnings) => {
                                if let Some(warn) = parse_warnings_to_string(&warnings) {
                                    warning_messages.push(format!("{}\n{warn}", chart.info.name));
                                }
                                if let Some(existing) = data.charts.iter_mut().find(|it| it.local_path == chart.local_path) {
                                    existing.info = chart.info;
                                } else {
                                    data.charts.push(*chart);
                                }
                                count += 1;
                            }
                            ImportChart::Skipped(name) => {
                                if !skipped.is_empty() {
                                    skipped.push_str(", ");
                                }
                                skipped.push_str(&name);
                            }
                            ImportChart::Failed(error) => failed.push(error),
                        }
                    }
                    save_data()?;
                    self.state.reload_local_charts();
                    NEED_UPDATE.store(true, Ordering::Relaxed);

                    let mut message = itl!("batch-import-success", "count" => count);
                    if !skipped.is_empty() {
                        message.push('\n');
                        message += &itl!("batch-import-downloaded-skipped", "charts" => skipped);
                    }

                    if !warning_messages.is_empty() {
                        message += "\n\n";
                        message += &warning_messages.join("\n\n");
                    }
                    if let Err(error) = res {
                        failed.push(format!("{error:#}"));
                    }
                    if !failed.is_empty() {
                        message += "\n\n";
                        message += &itl!("batch-import-failed");
                        message.push('\n');
                        message += &failed.join("\n\n");
                    }
                    Dialog::simple(message).show();
                }
                self.batch_import_task = None;
                self.batch_import_rx = None;
            }
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());

        STRIPE_MATERIAL.set_uniform("time", ((tm.real_time() * 0.025) % (std::f64::consts::PI * 2.)) as f32);
        gl_use_material(*STRIPE_MATERIAL);
        ui.fill_rect(ui.screen_rect(), (*self.background, ui.screen_rect()));
        gl_use_default_material();

        let s = &mut self.state;
        s.update(tm);

        // 1. page
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            let old = s.fader.distance;
            s.fader.distance *= -0.6;
            self.pages[pos].render(ui, s)?;
            s.fader.distance = old;
        }
        s.fader.sub = true;
        s.fader.reset();
        crate::hud::begin_frame();
        self.pages.last_mut().unwrap().render(ui, s)?;
        s.fader.sub = false;

        // 2. title
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            s.fader.reset();
            s.fader.render_title(ui, s.t, &self.pages[pos].label());
        }
        s.fader.for_sub(|f| f.render_title(ui, s.t, &self.pages.last().unwrap().label()));

        // 3. back
        if self.pages.len() >= 2 {
            let r = ui.back_rect();
            self.btn_back.set(ui, r);
            let dy = (match self.pages.len() {
                1 => 1.,
                2 => s.fader.for_sub(|f| f.progress(s.t)),
                _ => 0.,
            } * r.h)
                .clamp(0., r.h);
            let ir = Rect::new(r.x, r.y + dy, r.w, r.h);
            ui.fill_rect(Rect::new(r.x, r.y + dy, r.w, r.h - dy), (*self.icon_back, ir, ScaleType::Fit));
        }

        self.pages.last_mut().unwrap().render_top(ui, s)?;
        if crate::hud::edit_active() {
            crate::hud::editor_render(ui);
        }

        if self.import_task.is_some() {
            ui.full_loading(itl!("importing"), s.t);
        }
        if let Some(dl) = &mut self.deeplink_dl {
            dl.render(ui, s.t);
        }
        if let Some(it) = &mut self.deeplink_chart {
            it.render(ui, s.t);
        }
        if self.batch_import_task.is_some() {
            let current = self.batch_imported_charts.len();
            let total = self.batch_import_total;
            ui.full_loading(itl!("batch-importing", "current" => current, "total" => total), s.t);
        }

        self.idle_fps.settle();

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if let Some(next) = self.deeplink_scene.take() {
            if let Some(bgm) = &mut self.bgm {
                let _ = bgm.fade_out(0.5);
            }
            return next;
        }
        let res = self.pages.last_mut().unwrap().next_scene(&mut self.state);
        if !matches!(res, NextScene::None) {
            if let Some(bgm) = &mut self.bgm {
                let _ = bgm.fade_out(0.5);
            }
        }
        res
    }
}

static STRIPE_MATERIAL: Lazy<Material> = Lazy::new(|| {
    load_material(
        shader::VERTEX,
        shader::FRAGMENT,
        MaterialParams {
            uniforms: vec![("time".to_owned(), UniformType::Float1)],
            ..Default::default()
        },
    )
    .unwrap()
});

mod shader {
    pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    pos0 = position.xy;
    uv = texcoord;
}"#;

    pub const FRAGMENT: &str = r#"#version 100
precision highp float;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform sampler2D Texture;
uniform float time;

void main() {
    float angle = 0.66;
    float w = sin(angle) * pos0.y + cos(angle) * pos0.x - time;
    float t = mod(w, 0.02);
    float p = step(t, 0.012) * 0.07;
    gl_FragColor = texture2D(Texture, uv);
    gl_FragColor += (vec4(1.0) - gl_FragColor) * p;
}"#;
}
