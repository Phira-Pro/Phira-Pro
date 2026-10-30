//! Categorized chart-video settings and export task coordination.
use super::{Page, SharedState};
use anyhow::Result;
use macroquad::{prelude::*, window::get_internal_gl};
use prpr::{
    chart_video::{ChartVideoRenderer, VideoOptions},
    ui::{DRectButton, Scroll},
};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

mod config;
mod controller;
#[cfg(target_os = "windows")]
mod ffmpeg;
mod layout;
mod settings;
mod view;
use config::*;

type BoxFut<T> = Pin<Box<dyn Future<Output = T>>>;
const RESOLUTIONS: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
const FPS_CHOICES: [u32; 4] = [30, 60, 120, 240];
const BUDGET: Duration = Duration::from_millis(45);

struct Selected {
    local_path: String,
    name: String,
    subtitle: String,
    difficulty: f32,
}
enum Job {
    Idle,
    Loading(BoxFut<Result<ChartVideoRenderer>>),
    Rendering { renderer: ChartVideoRenderer, started: Instant },
    Done { path: String, frames: u64, secs: f64 },
    Failed(String),
}
pub struct RenderPage {
    prefs: Prefs,
    selected: Option<Selected>,
    pick_request: bool,
    category: settings::Category,
    scroll: Scroll,
    status_scroll: Scroll,
    tabs: [DRectButton; 6],
    fields: Vec<DRectButton>,
    pick: DRectButton,
    start: DRectButton,
    open: DRectButton,
    encoder_button: DRectButton,
    #[cfg(target_os = "windows")]
    ffmpeg: ffmpeg::Ffmpeg,
    job: Job,
}

fn pretty_path(path: &std::path::Path) -> String {
    path.display().to_string()
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
