prpr_l10n::tl_file!("common" ttl crate::);

#[rustfmt::skip]
#[cfg(closed)]
mod inner;

mod anim;
mod blacklist;
mod censor;
mod chart_install;
mod charts_view;
mod client;
mod data;
pub mod deeplink;
mod font_store;
mod frame_profile;
mod history;
mod judgement_presets;
mod hud;
mod icons;
mod images;
mod login;
mod migrate;
mod menu_music;
mod page;
mod popup;
mod rate;
mod replay;
mod resource;
mod scene;
mod startup;
mod tabs;
mod tags;
mod threed;
mod transfer;
mod uml;

use anyhow::Result;
use data::Data;
use macroquad::prelude::*;
use prpr::{
    build_conf,
    core::{init_assets, PGR_FONT},
    ext::SafeTexture,
    log,
    scene::show_error,
    time::TimeManager,
    ui::{cleanup_audio, FontArc, TextPainter},
    Main,
};
use prpr_l10n::set_prefered_locale;
#[cfg(not(feature = "hykb"))]
use prpr_l10n::{GLOBAL, LANGS};
use scene::MainScene;
use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex},
};
use tracing::{error, info, warn};

#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JObject, JString},
    sys::jint,
    EnvUnowned,
};

static MESSAGES_TX: Mutex<Option<mpsc::Sender<bool>>> = Mutex::new(None);
static DATA_PATH: Mutex<Option<String>> = Mutex::new(None);
static CACHE_DIR: Mutex<Option<String>> = Mutex::new(None);
pub static mut DATA: Option<Data> = None;

#[cfg(target_env = "ohos")]
use napi_derive_ohos::napi;

#[cfg(closed)]
pub async fn load_res(name: &str) -> Vec<u8> {
    let bytes = load_file(name).await.unwrap();
    inner::resolve_data(bytes)
}

#[allow(unused)]
pub async fn load_res_tex(name: &str) -> SafeTexture {
    #[cfg(closed)]
    {
        let bytes = load_res(name).await;
        let image = image::load_from_memory(&bytes).unwrap();
        image.into()
    }
    #[cfg(not(closed))]
    prpr::ext::BLACK_TEXTURE.clone()
}

pub fn sync_data() {
    if get_data().language.is_none() {
        #[cfg(feature = "hykb")]
        let default_lang = "zh-CN".to_owned();
        #[cfg(not(feature = "hykb"))]
        let default_lang = LANGS[GLOBAL.order.lock().unwrap()[0]].to_owned();
        get_data_mut().language = Some(default_lang);
    }
    set_prefered_locale(get_data().language.as_ref().and_then(|it| it.parse().ok()));
    let _ = client::set_access_token_sync(get_data().tokens.as_ref().map(|it| &*it.0));
}

pub fn set_data(data: Data) {
    unsafe {
        DATA = Some(data);
    }
}

#[allow(static_mut_refs)]
pub fn get_data() -> &'static Data {
    unsafe { DATA.as_ref().unwrap() }
}

#[allow(static_mut_refs)]
pub fn get_data_mut() -> &'static mut Data {
    unsafe { DATA.as_mut().unwrap() }
}

pub fn save_data() -> Result<()> {
    let path = std::path::PathBuf::from(dir::root()?).join("data.json");
    transfer::write_atomic(&path, serde_json::to_string(get_data())?.as_bytes())
}

mod dir {
    use anyhow::Result;

    use crate::{CACHE_DIR, DATA_PATH};

    fn ensure(s: &str) -> Result<String> {
        let s = format!("{}/{}", DATA_PATH.lock().unwrap().as_ref().map(|it| it.as_str()).unwrap_or("."), s);
        let path = std::path::Path::new(&s);
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        Ok(s)
    }

    pub fn cache() -> Result<String> {
        if let Some(cache) = &*CACHE_DIR.lock().unwrap() {
            ensure(cache)
        } else {
            ensure("cache")
        }
    }

    pub fn bold_font_path() -> Result<String> {
        Ok(format!("{}/bold.ttf", root()?))
    }

    pub fn cache_image_local() -> Result<String> {
        ensure(&format!("{}/image", cache()?))
    }

    pub fn root() -> Result<String> {
        ensure("data")
    }

    /// 自定义界面字体的路径（用户导入的，位于 `<data>/font.ttf`）。
    pub fn custom_font_path() -> Result<String> {
        Ok(format!("{}/font.ttf", root()?))
    }

    pub fn import_font(path: &std::path::Path) -> Result<()> {
        crate::font_store::import(path, std::path::Path::new(&custom_font_path()?))
    }

    pub fn reset_font() -> Result<()> {
        crate::font_store::reset(std::path::Path::new(&custom_font_path()?))
    }

    pub fn charts() -> Result<String> {
        ensure("data/charts")
    }

    pub fn collections() -> Result<String> {
        ensure("data/collections")
    }

    pub fn custom_charts() -> Result<String> {
        ensure("data/charts/custom")
    }

    pub fn downloaded_charts() -> Result<String> {
        ensure("data/charts/download")
    }

    pub fn respacks() -> Result<String> {
        ensure("data/respack")
    }

    pub fn appearance() -> Result<String> {
        ensure("data/appearance")
    }

    /// `data/appearance` 的只读定位。优先用运行时数据目录：移动端（Android / iOS /
    /// OHOS）的 `current_exe()` 指向系统进程，向上永远找不到 `assets` 目录，会返回
    /// `None` —— 那样导入的立绘 / 背景写进去了却读不出来。桌面端仍沿用「向上查找包含
    /// `assets` 的目录」的策略，因此在 cwd 尚未初始化的早期阶段也能正确定位。
    pub fn appearance_root() -> Option<std::path::PathBuf> {
        if let Some(base) = DATA_PATH.lock().unwrap().clone() {
            return Some(std::path::PathBuf::from(format!("{base}/data/appearance")));
        }
        let mut exe = std::env::current_exe().ok()?;
        while exe.pop() {
            if exe.join("assets").is_dir() {
                return Some(exe.join("data").join("appearance"));
            }
        }
        None
    }

    /// 在 `data/appearance` 下按候选扩展名探测自定义外观资源文件。
    pub fn find_appearance(stem: &str) -> Option<std::path::PathBuf> {
        let root = appearance_root()?;
        ["png", "jpg", "jpeg", "webp", "bmp"]
            .into_iter()
            .map(|ext| root.join(format!("{stem}.{ext}")))
            .find(|it| it.is_file())
    }

    /// 载入 `data/appearance/{stem}.*`；文件不存在或无法解码时返回 `None`。
    pub fn load_appearance_image(stem: &str) -> Option<image::DynamicImage> {
        let path = find_appearance(stem)?;
        match image::load_from_memory(&std::fs::read(&path).ok()?) {
            Ok(image) => Some(image),
            Err(err) => {
                tracing::warn!(?err, ?path, "failed to decode appearance image");
                None
            }
        }
    }

    /// 把任意图片导入为 `data/appearance/{stem}.{ext}`。会先清掉同名的其它扩展名，
    /// 保证探测时命中的是新文件；并且在覆盖前先验证能被解码，避免选中非图片文件后
    /// 把原有资源弄丢。
    pub fn import_appearance(stem: &str, src: &std::path::Path) -> Result<()> {
        // 不能只看扩展名：Android 上系统文件选择器返回的临时文件一律叫 `chart*.zip`
        // （官方 MainActivity 用 `File.createTempFile("chart", ".zip")` 复制选中内容），
        // 按扩展名判断会直接报「.zip 不是图片格式」。这里读进内存后按内容嗅探。
        let data = std::fs::read(src)?;
        let img = image::load_from_memory(&data)?;
        let root = std::path::PathBuf::from(appearance()?);
        // 只允许 `find_appearance` 会去探测的那几种扩展名；其它一律转存成 png。
        let ext = match image::guess_format(&data) {
            Ok(image::ImageFormat::Png) => Some("png"),
            Ok(image::ImageFormat::Jpeg) => Some("jpg"),
            Ok(image::ImageFormat::WebP) => Some("webp"),
            Ok(image::ImageFormat::Bmp) => Some("bmp"),
            _ => None,
        };
        for old in ["png", "jpg", "jpeg", "webp", "bmp"] {
            let _ = std::fs::remove_file(root.join(format!("{stem}.{old}")));
        }
        match ext {
            Some(ext) => std::fs::write(root.join(format!("{stem}.{ext}")), data)?,
            None => {
                let mut buf = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)?;
                std::fs::write(root.join(format!("{stem}.png")), buf)?;
            }
        }
        Ok(())
    }

    /// 删除 `data/appearance/{stem}.*` 的所有候选文件（用于「恢复默认」）。
    /// 返回是否删除过至少一个文件。
    pub fn clear_appearance(stem: &str) -> Result<bool> {
        let root = std::path::PathBuf::from(appearance()?);
        let mut removed = false;
        for ext in ["png", "jpg", "jpeg", "webp", "bmp"] {
            let path = root.join(format!("{stem}.{ext}"));
            if path.is_file() {
                std::fs::remove_file(&path)?;
                removed = true;
            }
        }
        Ok(removed)
    }

    /// 自定义背景音乐（`data/appearance/bgm.*`）的候选扩展名。
    /// sasa 走 symphonia 解码，这几个格式都在支持范围内。
    pub const AUDIO_EXTS: [&str; 6] = ["mp3", "ogg", "wav", "flac", "m4a", "aac"];

    /// 在 `data/appearance` 下按候选扩展名探测自定义外观**音频**文件。
    pub fn find_appearance_audio(stem: &str) -> Option<std::path::PathBuf> {
        let root = appearance_root()?;
        AUDIO_EXTS
            .into_iter()
            .map(|ext| root.join(format!("{stem}.{ext}")))
            .find(|it| it.is_file())
    }

    /// 把任意音频导入为 `data/appearance/{stem}.{ext}`。写之前先解码验证一次，
    /// 避免选中非音频文件后把原来的背景音乐弄丢；成功后再清掉同名的其它扩展名。
    pub fn import_appearance_audio(stem: &str, src: &std::path::Path) -> Result<()> {
        let data = std::fs::read(src)?;
        // 解码验证（sasa/symphonia 会按内容嗅探格式，不依赖扩展名）。
        sasa::AudioClip::new(data.clone())?;
        let ext = src
            .extension()
            .and_then(|it| it.to_str())
            .map(|it| it.to_ascii_lowercase())
            .filter(|it| AUDIO_EXTS.contains(&it.as_str()))
            .unwrap_or_else(|| "mp3".to_owned());
        let root = std::path::PathBuf::from(appearance()?);
        for old in AUDIO_EXTS {
            let _ = std::fs::remove_file(root.join(format!("{stem}.{old}")));
        }
        std::fs::write(root.join(format!("{stem}.{ext}")), data)?;
        if stem == "bgm" {
            let title = src.file_name().unwrap_or_default().to_string_lossy();
            std::fs::write(root.join("bgm-title.txt"), title.as_bytes())?;
        }
        Ok(())
    }

    /// 删除 `data/appearance/{stem}.*` 的所有音频候选文件（用于「恢复默认背景音乐」）。
    /// 返回是否删除过至少一个文件。
    pub fn clear_appearance_audio(stem: &str) -> Result<bool> {
        let root = std::path::PathBuf::from(appearance()?);
        if stem == "bgm" { let _ = std::fs::remove_file(root.join("bgm-title.txt")); }
        let mut removed = false;
        for ext in AUDIO_EXTS {
            let path = root.join(format!("{stem}.{ext}"));
            if path.is_file() {
                std::fs::remove_file(&path)?;
                removed = true;
            }
        }
        Ok(removed)
    }
}

async fn the_main() -> Result<()> {
    // Miniquad installs its Android panic hook while starting the render
    // thread, after the JNI bootstrap. Wrap that hook once rendering begins.
    #[cfg(target_os = "android")]
    startup::install();
    log::register();
    startup::stage("assets and platform initialization");
    #[cfg(target_env = "ohos")]
    {
        *DATA_PATH.lock().unwrap() = Some("/data/storage/el2/base".to_owned());
        *CACHE_DIR.lock().unwrap() = Some("/data/storage/el2/base/cache".to_owned());
        prpr::core::DPI_VALUE.store(250, std::sync::atomic::Ordering::Relaxed);
    };

    init_assets();
    #[cfg(target_os = "ios")]
    prpr::frame_pacing::install();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSSearchPathDirectory, NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains};

        let directories = NSSearchPathForDirectoriesInDomains(NSSearchPathDirectory::LibraryDirectory, NSSearchPathDomainMask::UserDomainMask, true);
        let path = directories.firstObject().unwrap().to_string();
        *DATA_PATH.lock().unwrap() = Some(path);
        *CACHE_DIR.lock().unwrap() = Some("Caches".to_owned());
    }

    startup::stage("saved data");
    let dir = dir::root()?;
    let mut data: Data = std::fs::read_to_string(format!("{dir}/data.json"))
        .map_err(anyhow::Error::new)
        .and_then(|s| Ok(serde_json::from_str(&s)?))
        .unwrap_or_default();
    data.init().await?;
    set_data(data);
    startup::stage("network clients");
    sync_data();
    save_data()?;
    if let Err(err) = dir::appearance() {
        warn!(?err, "failed to create appearance directory");
    }

    // Warm up the offline banned-word automaton so local edits can check
    // synchronously. No-op without the `aa` feature.
    tokio::spawn(censor::preload());

    let rx = {
        let (tx, rx) = mpsc::channel();
        *MESSAGES_TX.lock().unwrap() = Some(tx);
        rx
    };

    unsafe { get_internal_gl() }
        .quad_context
        .display_mut()
        .set_pause_resume_listener(on_pause_resume);

    startup::stage("fonts");
    match load_file("font.ttf").await {
        Ok(bytes) => match FontArc::try_from_vec(bytes) {
            Ok(font) => prpr::ui::set_multilingual_fallback(font),
            Err(err) => warn!(?err, "invalid multilingual fallback font"),
        },
        Err(err) => warn!(?err, "multilingual fallback font unavailable"),
    }
    let pgr_font = FontArc::try_from_vec(load_file("phigros.ttf").await?)?;
    PGR_FONT.with(move |it| *it.borrow_mut() = Some(TextPainter::new(pgr_font, None)));

    // 界面字体：优先用用户导入的 <data>/font.ttf；否则用内置字体。
    // 内置字体按 harmonyos.ttf → font.ttf → bold.ttf 依次尝试，缺哪一个都不应导致启动崩溃。
    // 用自定义字体时把内置字体作为逐字回退，缺字不会变成方块。
    let custom_font = dir::custom_font_path()
        .ok()
        .and_then(|path| match font_store::load(std::path::Path::new(&path)) {
            Ok(font) => Some(font),
            Err(err)
                if err
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(err) => {
                warn!(?err, "invalid imported font; using builtin");
                None
            }
        });
    let has_custom_font = custom_font.is_some();

    let mut builtin_font = None;
    for name in ["harmonyos.ttf", "font.ttf", "bold.ttf"] {
        match load_file(name).await {
            Ok(it) => {
                builtin_font = Some(it);
                break;
            }
            Err(err) => warn!(?err, "failed to load builtin font, trying next"),
        }
    }
    let builtin_font = builtin_font.ok_or_else(|| anyhow::anyhow!("no builtin font found in assets"))?;

    let reference = FontArc::try_from_vec(builtin_font.clone())?;
    let font = custom_font.unwrap_or_else(|| reference.clone());
    let fallback = if has_custom_font {
        FontArc::try_from_vec(builtin_font).ok()
    } else {
        None
    };
    let mut painter = TextPainter::new(font.clone(), fallback);
    if has_custom_font {
        painter.normalize_to(&reference);
    }
    prpr::ui::FONT_DISPLAY_SCALE.store(get_data().config.font_scale().to_bits(), std::sync::atomic::Ordering::Relaxed);

    // Bold headings also need the builtin CJK fallback when the imported font
    // contains Latin only. The custom font is already in the main painter.
    startup::stage("audio and main scene");
    let mut main = Main::new(Box::new(MainScene::new(reference).await?), TimeManager::default(), None).await?;
    startup::stage("running");

    let tm = TimeManager::default();
    let mut fps_time = -1;

    const FPS_BUF_SIZE: usize = 60;
    let mut fps_times = VecDeque::<f64>::with_capacity(FPS_BUF_SIZE);
    let mut last_frame_start = f64::NAN;
    let mut fps_time_sum = 0.;
    #[cfg(target_os = "windows")]
    let mut frame_pacer = prpr::desktop_pacing::Pacer::new();
    let mut frame_profile = frame_profile::FrameProfile::from_env();
    let mut profile_previous = std::time::Instant::now();

    'app: loop {
        let profile_start = frame_profile.as_ref().map(|_| std::time::Instant::now());
        #[cfg(target_os = "windows")]
        frame_pacer.wait();
        let profile_work = frame_profile.as_ref().map(|_| std::time::Instant::now());
        // 处理积压的暂停 / 恢复消息（以最后一条为准）。日志保留：暂停 / 恢复一旦不成对，
        // 就会出现「画面卡死、切后台也救不回来」，需要靠日志定位。
        let mut state = None;
        while let Ok(p) = rx.try_recv() {
            state = Some(p);
        }
        match state {
            Some(true) => {
                if !main.paused() {
                    info!("app paused (backgrounded)");
                    main.pause()?;
                }
            }
            Some(false) => {
                if main.paused() {
                    info!("app resumed (foregrounded)");
                    main.resume()?;
                    fps_times.clear();
                    fps_time_sum = 0.;
                    last_frame_start = f64::NAN;
                }
            }
            None => {}
        }
        if main.paused() {
            // 暂停中：跳过游戏更新与渲染，但必须把这一帧还给平台主循环。
            // 关键：这里绝对不能用阻塞式等待（recv / recv_timeout）来等恢复消息——那会把
            // 主线程一直占住，系统弹窗（例如 iOS 文件选择器）就收不到触摸事件，
            // 表现为「弹窗能打开但点不动」。改为非阻塞轮询 + 让出一帧。
            let mut resumed = false;
            loop {
                match rx.try_recv() {
                    Ok(p) => resumed = !p,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break 'app,
                }
            }
            if resumed {
                info!("app resumed (foregrounded)");
                main.resume()?;
                fps_times.clear();
                fps_time_sum = 0.;
                last_frame_start = f64::NAN;
            }
            next_frame().await;
            continue;
        }

        let frame_start = tm.real_time();
        if !last_frame_start.is_nan() {
            if fps_times.len() == FPS_BUF_SIZE {
                fps_time_sum -= fps_times.pop_front().unwrap();
            }
            let frame_time = frame_start - last_frame_start;
            fps_times.push_back(frame_time);
            fps_time_sum += frame_time;
        }
        last_frame_start = frame_start;
        // 供界面显示的帧率：近 FPS_BUF_SIZE 帧的平均值。
        let avg_frame_time = fps_time_sum / fps_times.len().max(1) as f64;
        let cur_fps = if avg_frame_time > 0. { 1. / avg_frame_time } else { 0. };
        prpr::ui::CURRENT_FPS.store((cur_fps as f32).to_bits(), std::sync::atomic::Ordering::Relaxed);
        let mut profile_update = 0.;
        let mut profile_render = 0.;
        let res = || -> Result<()> {
            main.update()?;
            // 结算界面的「应用推荐偏移」请求：落到全局配置并持久化。
            // `prpr` 拿不到客户端的 Data，只能通过这条单向通道把增量递过来。
            if let Some(delta) = prpr::config::take_offset_delta() {
                let config = &mut get_data_mut().config;
                config.offset = (config.offset + delta).clamp(-0.5, 0.5);
                save_data()?;
            }
            let render_start = frame_profile.as_ref().map(|_| std::time::Instant::now());
            if let (Some(work), Some(render)) = (profile_work, render_start) {
                profile_update = render.duration_since(work).as_secs_f64() * 1000.;
            }
            main.render(&mut painter)?;
            prpr::ext::flush_pending_texture_deletions();
            if let Some(start) = render_start { profile_render = start.elapsed().as_secs_f64() * 1000.; }
            Ok(())
        }();
        if let Err(err) = res {
            error!("uncaught error: {err:?}");
            show_error(err);
        }
        if main.should_exit() {
            break 'app;
        }

        let t = tm.real_time();

        let fps_now = t as i32;
        if fps_now != fps_time {
            fps_time = fps_now;
            if fps_times.len() == FPS_BUF_SIZE {
                let actual_fps = 1. / (fps_time_sum / FPS_BUF_SIZE as f64);
                let current_fps = 1. / (t - frame_start);
                info!("FPS {} (capped at {})", current_fps as u32, actual_fps as u32);
            }
        }

        // 暂停时会走上面的分支直接 `continue`（不更新不渲染），所以这里不用再额外处理。

        let present_start = frame_profile.as_ref().map(|_| std::time::Instant::now());
        next_frame().await;
        if let (Some(profile), Some(start), Some(work), Some(present)) = (&mut frame_profile, profile_start, profile_work, present_start) {
            let interval = start.duration_since(profile_previous).as_secs_f64() * 1000.;
            profile_previous = start;
            profile.record(start, interval, work.duration_since(start).as_secs_f64() * 1000., profile_update, profile_render, present.elapsed().as_secs_f64() * 1000.);
        }
    }
    Ok(())
}

/// 界面显示的改版版本号。仅用于本地展示，绝不上报服务端：
/// 与服务器交互的版本号一律仍取 `CARGO_PKG_VERSION`（见 `client.rs`、`home.rs`、`event.rs`）。
#[cfg(not(flash))]
pub const PRO_VERSION: &str = env!("PHIRA_PRO_VERSION");
/// Phira Pro Flash（轻量版）的展示用版本号。
#[cfg(flash)]
pub const PRO_VERSION: &str = env!("PHIRA_FLASH_VERSION");
/// 带 `v` 前缀的展示用版本号。
#[cfg(not(flash))]
pub const PRO_VERSION_TAG: &str = env!("PHIRA_PRO_VERSION_TAG");
#[cfg(flash)]
pub const PRO_VERSION_TAG: &str = env!("PHIRA_FLASH_VERSION_TAG");

fn build_global_window_conf() -> Conf {
    let mut conf = build_conf();
    conf.window_title = "Phira Pro".to_owned();
    conf.icon = Some(custom_window_icon().unwrap_or_else(default_window_icon));

    #[cfg(target_os = "windows")]
    {
        // Windows' composed OpenGL swap can quantize missed presents to
        // 60/80 Hz even on a 165 Hz monitor. Pace explicitly at this window's
        // current monitor refresh rate, shared by menus and gameplay.
        conf.platform.swap_interval = Some(0);
        conf.fullscreen = dir::root()
            .ok()
            .and_then(|r| std::fs::read_to_string(std::path::Path::new(&r).join("data.json")).ok())
            .and_then(|s| serde_json::from_str::<Data>(&s).ok())
            .is_some_and(|d| d.config.fullscreen_mode);
    }

    conf
}

/// 内置窗口图标（与官方一致）。
fn default_window_icon() -> miniquad::conf::Icon {
    miniquad::conf::Icon {
        small: *include_bytes!("../icon/small"),
        medium: *include_bytes!("../icon/medium"),
        big: *include_bytes!("../icon/big"),
    }
}

/// 载入 `data/appearance/icon.*` 作为窗口图标；文件缺失或无法解码时返回 `None`，
/// 由调用方回落到 [`default_window_icon`]。窗口图标只在创建窗口时设置一次。
fn custom_window_icon() -> Option<miniquad::conf::Icon> {
    let image = dir::load_appearance_image("icon")?;

    fn scaled<const N: usize>(image: &image::DynamicImage, size: u32) -> Option<[u8; N]> {
        image
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8()
            .into_raw()
            .try_into()
            .ok()
    }

    Some(miniquad::conf::Icon {
        small: scaled(&image, 16)?,
        medium: scaled(&image, 32)?,
        big: scaled(&image, 64)?,
    })
}

#[no_mangle]
pub extern "C" fn quad_main() {
    #[cfg(not(target_os = "android"))]
    startup::install();
    macroquad::Window::from_config(build_global_window_conf(), async {
        if let Err(err) = the_main().await {
            startup::show_failure(err).await;
        }
    });
    cleanup_audio();
}

fn on_pause_resume(pause: bool) {
    #[cfg(target_os = "ios")]
    prpr::frame_pacing::set_paused(pause);
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(pause);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_initializeEnvironment(mut env: EnvUnowned, _class: JClass, context: JObject) {
    // Startup runs before the normal tracing subscriber is installed. Keep
    // native failures visible in logcat instead of only writing to stderr.
    std::panic::set_hook(Box::new(|info| startup::report(&info.to_string())));
    env.with_env(|env| -> jni::errors::Result<()> {
        inputbox::backend::Android::initialize(env)?;
        // reqwest's verifier (0.7 / JNI 0.22) is separate from miniquad's
        // verifier (0.6 / JNI 0.21). Initialize it using the real Java caller
        // frame, before spawning render/network threads. Do not reinterpret
        // ndk_context's global reference as an owned local JNI reference.
        rustls_platform_verifier::android::init_with_env(env, context)
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnPause(_env: EnvUnowned, _class: JClass) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(true);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnResume(_env: EnvUnowned, _class: JClass) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(false);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnDestroy(_env: EnvUnowned, _class: JClass) {
    std::process::exit(0);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDataPath(mut env: EnvUnowned, _class: JClass, path: JString) {
    // This callback runs before initializeEnvironment. Reading via the caller's
    // environment initializes JNI 0.22; Display would otherwise return a placeholder.
    env.with_env(|env| -> jni::errors::Result<()> {
        *DATA_PATH.lock().unwrap() = Some(path.try_to_string(env)?);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setTempDir(mut env: EnvUnowned, _class: JClass, path: JString) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let path = path.try_to_string(env)?;
        std::env::set_var("TMPDIR", path.clone());
        *CACHE_DIR.lock().unwrap() = Some(path);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDpi(_env: EnvUnowned, _class: JClass, dpi: jint) {
    prpr::core::DPI_VALUE.store(dpi as _, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setChosenFile(_env: EnvUnowned, _class: JClass, file: JString) {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().1 = Some(file.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDeepLink(_env: EnvUnowned, _class: JClass, url: JString) {
    deeplink::set_deeplink(url.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_markImport(_env: EnvUnowned, _class: JClass) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_markImportRespack(_env: EnvUnowned, _class: JClass) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import_respack".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setInputText(_env: EnvUnowned, _class: JClass, text: JString) {
    use prpr::scene::INPUT_TEXT;
    INPUT_TEXT.lock().unwrap().1 = Some(text.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_preprocessInput(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    _motion_event: *mut std::ffi::c_void,
    _x: jni::sys::jfloat,
    _y: jni::sys::jfloat,
    _z: jni::sys::jboolean,
    _z2: jni::sys::jboolean,
) {
}

/// Credentials obtained from the native HYKB (好游快爆) login SDK.
pub struct HykbCredential {
    /// SDK result code: 0 on success, otherwise an error / user cancellation.
    pub code: i32,
    pub uid: i64,
    pub nick: String,
    pub access_token: String,
}

impl HykbCredential {
    /// Map the SDK result code to an error, or yield the credential on success.
    /// Centralizes the code → user-facing message translation shared by every
    /// HYKB login/bind entry point.
    #[cfg(feature = "hykb")]
    pub fn ok_or_err(self) -> Result<Self> {
        if self.code == 0 {
            Ok(self)
        } else {
            // A non-zero code is any failure the HYKB SDK reports: 2001 auth
            // failed, 2002 login failed, 2003 cancelled, 2004 exception, 2005
            // developer-requested exit / account logout. A HYKB build mandates a
            // valid, matching HYKB session, so every one of these must tear the
            // in-game session down — otherwise cancelling the HYKB prompt during
            // a silent re-verify would leave the player signed in and bypass the
            // gate entirely.
            force_logout();
            anyhow::bail!("{}", crate::ttl!("hykb-login-cancelled"))
        }
    }
}

/// Slot for the pending HYKB login result. The native callback fulfills it.
static HYKB_TX: Mutex<Option<tokio::sync::oneshot::Sender<HykbCredential>>> = Mutex::new(None);

/// Call a no-arg `void` method on the Android host activity (the HYKB shell).
#[cfg(all(target_os = "android", feature = "hykb"))]
fn call_activity_void(method: &'static jni::strings::JNIStr) {
    use jni::{jni_sig, objects::JObject, vm::JavaVM};

    JavaVM::singleton()
        .unwrap()
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let ctx = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
            env.call_method(ctx, method, jni_sig!("()V"), &[])?;
            Ok(())
        })
        .unwrap();
}

/// Ask the Android shell to pop the HYKB account picker (`MainActivity.hykbSwitchAccount`).
/// Used by the explicit login / switch-account flow.
#[cfg(all(target_os = "android", feature = "hykb"))]
fn request_hykb_login() {
    call_activity_void(jni::jni_str!("hykbSwitchAccount"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
fn request_hykb_login() {}

/// Ask the Android shell to sign in using the cached HYKB account without
/// popping the picker (`MainActivity.hykbLogin`). The credentials the SDK
/// reports flow back through `HYKB_TX`, so the caller can verify them against
/// the restored Phira session. Used by the silent startup restore.
#[cfg(all(target_os = "android", feature = "hykb"))]
fn request_hykb_login_silent() {
    call_activity_void(jni::jni_str!("hykbLogin"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
fn request_hykb_login_silent() {}

/// Tell the native HYKB SDK to sign out (`MainActivity.hykbLogout`). Called when the
/// player logs out from their profile.
#[cfg(all(target_os = "android", feature = "hykb"))]
pub fn hykb_logout() {
    call_activity_void(jni::jni_str!("hykbLogout"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
pub fn hykb_logout() {}

/// Tear down the local session: sign out of the native HYKB SDK, clear the
/// stored account and tokens, then re-sync. Shared by every path that must
/// reject a login — a failed/cancelled HYKB verification, a uid mismatch, or
/// the player logging out from their profile.
pub fn force_logout() {
    hykb_logout();
    get_data_mut().me = None;
    get_data_mut().tokens = None;
    let _ = save_data();
    sync_data();
}

/// Trigger the native HYKB login and await its credentials.
#[allow(unused)]
pub async fn obtain_hykb_credential() -> Result<HykbCredential> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    *HYKB_TX.lock().unwrap() = Some(tx);
    request_hykb_login();
    let cred = rx.await.map_err(|_| anyhow::anyhow!("hykb login cancelled"))?;
    Ok(cred)
}

/// Silently restore the HYKB session from the cached account and await its
/// credentials. Unlike [`obtain_hykb_credential`], this does not pop the account
/// picker; used by the blocking startup check to verify the restored session.
#[allow(unused)]
pub async fn obtain_hykb_credential_silent() -> Result<HykbCredential> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    *HYKB_TX.lock().unwrap() = Some(tx);
    request_hykb_login_silent();
    let cred = rx.await.map_err(|_| anyhow::anyhow!("hykb login cancelled"))?;
    Ok(cred)
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_hykbLoginCallback(
    _env: EnvUnowned,
    _class: JClass,
    code: jint,
    uid: jni::sys::jlong,
    nick: JString,
    access_token: JString,
) {
    let nick = if nick.is_null() { String::new() } else { nick.to_string() };
    let access_token = if access_token.is_null() {
        String::new()
    } else {
        access_token.to_string()
    };
    if let Some(tx) = HYKB_TX.lock().unwrap().take() {
        let _ = tx.send(HykbCredential {
            code: code as i32,
            uid: uid as i64,
            nick,
            access_token,
        });
    } else if code == 2005 {
        // No login is in flight, so this is the SDK's asynchronous
        // anti-addiction "exit game" action: the player hit a play-time limit
        // and chose to quit from the SDK's own dialog. Honor it by exiting.
        // Other async codes (e.g. 2008 "continue playing") are handled inside
        // the SDK and need no response here. A request-less success (code 0, the
        // SDK switching accounts on its own) is likewise ignored: any signed-in
        // HYKB account is accepted, so a switch no longer tears the session down.
    }
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn set_input_text(text: String) {
    use prpr::scene::INPUT_TEXT;
    INPUT_TEXT.lock().unwrap().1 = Some(text);
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn set_chosen_file(file: String) {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().1 = Some(file);
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn mark_auto_import() {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().0 = Some("_import_auto".to_owned());
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn on_foreground() {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(false);
    }
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn on_background() {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(true);
    }
}
