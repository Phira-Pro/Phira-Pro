//! Windows 编码器的检测、按需下载与应用内安装。
use anyhow::{bail, ensure, Context, Result};
use futures_util::StreamExt;
use prpr::{
    chart_video::{ffmpeg_candidates, managed_ffmpeg_path},
    task::Task,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const DOWNLOAD_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
const CHECKSUM_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256";
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;

enum State {
    Checking,
    Missing,
    Ready,
    Installing,
    Failed(String),
}

#[derive(Default)]
struct Progress {
    received: AtomicU64,
    total: AtomicU64,
    stage: AtomicU8,
    cancelled: AtomicBool,
}

pub(super) struct Ffmpeg {
    state: State,
    task: Option<Task<Result<bool>>>,
    progress: Arc<Progress>,
}

impl Ffmpeg {
    pub fn new() -> Self {
        Self {
            state: State::Checking,
            task: Some(Task::new(async {
                tokio::task::spawn_blocking(|| ffmpeg_candidates().iter().any(|path| probe(path).unwrap_or(false)))
                    .await
                    .context("检测任务失败")
            })),
            progress: Arc::default(),
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self.state, State::Ready)
    }

    pub fn update(&mut self) {
        if let Some(result) = self.task.as_mut().and_then(Task::take) {
            self.task = None;
            self.state = match result {
                Ok(true) => State::Ready,
                Ok(false) => State::Missing,
                Err(err) => State::Failed(format!("{err:#}")),
            };
        }
    }

    pub fn description(&self) -> String {
        match &self.state {
            State::Checking => "正在检测 FFmpeg…".into(),
            State::Missing => "尚未找到可用的 FFmpeg。首次使用时下载，其他功能无需安装。".into(),
            State::Ready => "FFmpeg 已就绪，可以导出视频。".into(),
            State::Failed(err) => format!("FFmpeg 安装失败：{err}。可重试下载，或自行安装后重新进入此页。"),
            State::Installing => {
                if self.progress.stage.load(Ordering::Relaxed) > 0 {
                    return "正在解压并验证编码器…".into();
                }
                let received = self.progress.received.load(Ordering::Relaxed) as f64 / 1048576.;
                let total = self.progress.total.load(Ordering::Relaxed) as f64 / 1048576.;
                if total > 0. {
                    format!("正在下载 FFmpeg：{received:.1} / {total:.1} MB")
                } else {
                    format!("正在连接下载源，已下载 {received:.1} MB")
                }
            }
        }
    }

    pub fn button_label(&self) -> Option<&'static str> {
        match self.state {
            State::Missing => Some("下载并安装 FFmpeg"),
            State::Failed(_) => Some("重试下载"),
            State::Installing if self.progress.stage.load(Ordering::Relaxed) == 0 => Some("取消下载"),
            _ => None,
        }
    }

    pub fn action(&mut self) {
        if matches!(self.state, State::Installing) {
            self.progress.cancelled.store(true, Ordering::Relaxed);
            self.task = None;
            self.state = State::Missing;
        } else if matches!(self.state, State::Missing | State::Failed(_)) {
            self.progress = Arc::default();
            let progress = Arc::clone(&self.progress);
            self.task = Some(Task::new(async move { install(progress, managed_ffmpeg_path()).await.map(|()| true) }));
            self.state = State::Installing;
        }
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        self.progress.cancelled.store(true, Ordering::Relaxed);
    }
}

fn check_cancel(progress: &Progress) -> Result<()> {
    ensure!(!progress.cancelled.load(Ordering::Relaxed), "下载已取消");
    Ok(())
}

/// 实际编码一小段静音黑帧，确认 H.264 与 AAC 都能使用。
fn probe(path: &Path) -> Result<bool> {
    let mut child = Command::new(path)
        .creation_flags(0x0800_0000)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=s=16x16:r=1",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=8000:cl=mono",
            "-t",
            "0.04",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return match result {
                    Err(err) => Err(err.into()),
                    _ => Ok(false),
                };
            }
        }
    }
}

fn checksum(text: &str) -> Result<Vec<u8>> {
    let value = text.split_whitespace().next().unwrap_or_default();
    ensure!(value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()), "下载源返回的校验信息无效");
    Ok(hex::decode(value)?)
}

async fn install(progress: Arc<Progress>, executable: PathBuf) -> Result<()> {
    // 独立客户端不携带游戏账号信息，也不继承忽略证书校验的游戏设置。
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(1800))
        .build()?;
    let digest_response = client.get(CHECKSUM_URL).send().await?.error_for_status()?;
    let digest_bytes = digest_response.bytes().await?;
    ensure!(digest_bytes.len() <= 4096, "下载源返回的校验信息过大");
    let expected = checksum(std::str::from_utf8(&digest_bytes)?)?;
    check_cancel(&progress)?;
    let response = client.get(DOWNLOAD_URL).send().await?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);
    ensure!(total <= MAX_DOWNLOAD, "FFmpeg 下载包超过大小限制");
    progress.total.store(total, Ordering::Relaxed);
    let mut archive = tempfile::tempfile()?;
    let mut stream = response.bytes_stream();
    let mut digest = Sha256::new();
    let mut received = 0;
    while let Some(chunk) = stream.next().await {
        check_cancel(&progress)?;
        let chunk = chunk.context("下载中断，请重试")?;
        received += chunk.len() as u64;
        ensure!(received <= MAX_DOWNLOAD, "FFmpeg 下载包超过大小限制");
        archive.write_all(&chunk)?;
        digest.update(&chunk);
        progress.received.store(received, Ordering::Relaxed);
    }
    ensure!(digest.finalize().as_slice() == expected, "下载包校验失败，请重新下载");
    archive.rewind()?;
    check_cancel(&progress)?;
    progress.stage.store(1, Ordering::Relaxed);
    tokio::task::spawn_blocking(move || install_archive(archive, &progress, &executable))
        .await
        .context("安装任务失败")?
}

fn install_archive(archive: File, progress: &Progress, executable: &Path) -> Result<()> {
    let directory = executable.parent().context("工具目录无效")?;
    std::fs::create_dir_all(directory).context("无法创建 FFmpeg 工具目录")?;
    let stage = tempfile::tempdir_in(directory)?;
    unpack(archive, stage.path())?;
    check_cancel(progress)?;
    ensure!(probe(&stage.path().join("ffmpeg.exe"))?, "下载的 FFmpeg 无法运行或不支持 H.264/AAC");
    check_cancel(progress)?;
    for name in ["LICENSE", "README.txt"] {
        if stage.path().join(name).is_file() {
            std::fs::copy(stage.path().join(name), directory.join(name))?;
        }
    }
    std::fs::write(
        directory.join("SOURCE.txt"),
        concat!(
            "FFmpeg Windows essentials build by Gyan Doshi\n",
            "Builds and source links: https://www.gyan.dev/ffmpeg/builds/\n",
            "License: GPLv3. See LICENSE and README.txt.\n"
        ),
    )?;
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    std::io::copy(&mut File::open(stage.path().join("ffmpeg.exe"))?, staged.as_file_mut())?;
    staged.as_file().sync_all()?;
    check_cancel(progress)?;
    staged
        .persist(executable)
        .map_err(|err| err.error)
        .context("无法保存 FFmpeg，请检查目录权限")?;
    Ok(())
}

/// 只输出固定文件名，压缩包里的目录结构不会成为本地写入路径。
fn unpack(archive: File, directory: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(archive).context("FFmpeg 下载包不是有效的 ZIP")?;
    let mut found = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(path) = entry.enclosed_name() else {
            bail!("FFmpeg 下载包包含无效路径");
        };
        let name = if path.ends_with("bin/ffmpeg.exe") {
            ensure!(!found, "FFmpeg 下载包包含重复的程序文件");
            found = true;
            "ffmpeg.exe"
        } else if path.file_name().is_some_and(|name| name == "LICENSE") {
            "LICENSE"
        } else if path.file_name().is_some_and(|name| name == "README.txt") {
            "README.txt"
        } else {
            continue;
        };
        ensure!(entry.is_file() && !entry.is_symlink(), "FFmpeg 下载包包含无效文件");
        let limit = if name == "ffmpeg.exe" { MAX_EXECUTABLE } else { 2 * 1024 * 1024 };
        ensure!(entry.size() > 0 && entry.size() <= limit, "FFmpeg 解压文件大小无效");
        let mut output = File::create(directory.join(name))?;
        let count = std::io::copy(&mut entry.by_ref().take(limit + 1), &mut output)?;
        ensure!(count <= limit, "FFmpeg 解压文件超过大小限制");
    }
    ensure!(found && directory.join("LICENSE").is_file(), "FFmpeg 下载包缺少程序或许可文件");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(files: &[(&str, &[u8])]) -> File {
        let mut writer = zip::ZipWriter::new(tempfile::tempfile().unwrap());
        for (name, bytes) in files {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(bytes).unwrap();
        }
        let mut file = writer.finish().unwrap();
        file.rewind().unwrap();
        file
    }

    #[test]
    fn extracts_only_encoder_and_not_other_executables() {
        let destination = tempfile::tempdir().unwrap();
        unpack(
            archive(&[
                ("build/bin/ffmpeg.exe", b"test"),
                ("build/LICENSE", b"GPL"),
                ("build/bin/ffplay.exe", b"ignored"),
            ]),
            destination.path(),
        )
        .unwrap();
        assert_eq!(std::fs::read(destination.path().join("ffmpeg.exe")).unwrap(), b"test");
        assert!(!destination.path().join("ffplay.exe").exists());
    }

    #[test]
    fn rejects_unsafe_incomplete_and_duplicate_archives() {
        for files in [
            vec![("../bin/ffmpeg.exe", b"test".as_slice())],
            vec![("build/bin/ffmpeg.exe", b"test".as_slice())],
            vec![("build/LICENSE", b"GPL".as_slice())],
            vec![("a/bin/ffmpeg.exe", b"one".as_slice()), ("b/bin/ffmpeg.exe", b"two".as_slice())],
        ] {
            let destination = tempfile::tempdir().unwrap();
            assert!(unpack(archive(&files), destination.path()).is_err());
        }
    }

    #[test]
    fn validates_checksum_and_cancellation() {
        assert_eq!(checksum(&format!("{}  file.zip", "ab".repeat(32))).unwrap(), vec![0xab; 32]);
        assert!(checksum("<html>download failed</html>").is_err());
        let progress = Progress::default();
        progress.cancelled.store(true, Ordering::Relaxed);
        assert!(check_cancel(&progress).is_err());
    }

    #[tokio::test]
    #[ignore = "downloads an external Windows encoder"]
    async fn downloads_and_validates_encoder() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = std::env::var_os("PHIRA_FFMPEG_TEST_DIRECTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_owned());
        let executable = directory.join("ffmpeg.exe");
        install(Arc::default(), executable.clone()).await.unwrap();
        assert!(probe(&executable).unwrap());
        assert!(directory.join("LICENSE").is_file());
    }
}
