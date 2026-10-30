//! Deterministic chart-video exports with independent presentation and extension stages.
#[cfg(target_os = "android")]
use std::path::Path;
use std::path::PathBuf;

mod audio;
mod encoder;
pub mod extensions;
mod options;
pub mod presentation;
mod session;
mod timeline;

pub use options::*;
pub use session::ChartVideoRenderer;
pub use timeline::{FrameContext, RenderStage, RenderTimeline, Segment};

/// ffmpeg 可执行文件的候选列表（按优先级排列）。
///
/// Windows：环境变量 `PHIRA_FFMPEG` → 应用工具目录 → PATH 里的 `ffmpeg`。
/// 其他桌面端：环境变量 `PHIRA_FFMPEG` → PATH 里的 `ffmpeg`。
/// Android：APK 里的 `libffmpeg.so` 会随安装解压到 nativeLibraryDir，
/// 但这个文件通常是 0644（只够 `dlopen`），直接 `execve` 会拿到
/// `EACCES`；所以再准备一份拷到可写目录并 `chmod 755` 的副本兜底。
/// 两者都会尝试，谁能跑起来就用谁。
pub fn ffmpeg_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(path) = std::env::var_os("PHIRA_FFMPEG") {
        list.push(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    list.push(managed_ffmpeg_path());
    for path in bundled_ffmpeg_candidates() {
        if !list.contains(&path) {
            list.push(path);
        }
    }
    list.push(PathBuf::from("ffmpeg"));
    list
}

/// Windows 按需安装的编码器，不改动系统 PATH。
#[cfg(target_os = "windows")]
pub fn managed_ffmpeg_path() -> PathBuf {
    PathBuf::from("data/tools/ffmpeg/ffmpeg.exe")
}

#[cfg(target_os = "android")]
fn bundled_ffmpeg_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    let Some(native) = native_ffmpeg() else {
        return list;
    };
    list.push(native.clone());
    if let Some(copy) = copy_executable(&native) {
        list.push(copy);
    }
    list
}

/// nativeLibraryDir 里的 `libffmpeg.so`（和 `libphira.so` 同目录）。
#[cfg(target_os = "android")]
fn native_ffmpeg() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    for line in maps.lines() {
        let Some(path) = line.split_whitespace().nth(5) else {
            continue;
        };
        if !path.ends_with("libphira.so") {
            continue;
        }
        let candidate = Path::new(path).parent()?.join("libffmpeg.so");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// 把可执行文件复制到缓存目录并补上可执行位，返回副本路径。
#[cfg(target_os = "android")]
fn copy_executable(source: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join("phira-ffmpeg");
    std::fs::create_dir_all(&dir).ok()?;
    let target = dir.join("ffmpeg");
    let up_to_date = match (std::fs::metadata(source), std::fs::metadata(&target)) {
        (Ok(a), Ok(b)) => a.len() == b.len(),
        _ => false,
    };
    if !up_to_date {
        std::fs::copy(source, &target).ok()?;
        let mut perms = std::fs::metadata(&target).ok()?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&target, perms).ok()?;
    }
    Some(target)
}

#[cfg(not(target_os = "android"))]
fn bundled_ffmpeg_candidates() -> Vec<PathBuf> {
    Vec::new()
}
