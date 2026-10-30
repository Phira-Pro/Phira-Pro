use super::{ffmpeg_candidates, VideoOptions};
use anyhow::{bail, Context, Result};
use std::{
    io::Write,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
};

pub(super) struct Encoder {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl Encoder {
    pub fn spawn(options: &VideoOptions, out: &Path, soundtrack: &Path, duration: f64) -> Result<Self> {
        let mut errors = vec![];
        for program in ffmpeg_candidates() {
            match Self::spawn_with(&program, options, out, soundtrack, duration) {
                Ok(encoder) => return Ok(encoder),
                Err(err) => errors.push(format!("{}：{err}", program.display())),
            }
        }
        bail!("无法启动 FFmpeg：{}", errors.join("；"))
    }

    fn spawn_with(program: &Path, options: &VideoOptions, out: &Path, soundtrack: &Path, duration: f64) -> Result<Self> {
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
            .arg("-framerate")
            .arg(options.fps.to_string())
            .args(["-i", "pipe:0", "-i"])
            .arg(soundtrack)
            .args([
                "-map", "0:v", "-map", "1:a", "-vf", "vflip", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a",
                "aac", "-b:a", "192k", "-t",
            ])
            .arg(format!("{duration:.9}"))
            .args(["-movflags", "+faststart"])
            .arg(out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().context("无法获取 FFmpeg 输入管道")?;
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.stdin.as_mut().context("编码器已关闭")?.write_all(bytes).context("写入视频帧失败")
    }

    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let status = self.child.take().context("编码器已关闭")?.wait()?;
        if !status.success() {
            bail!("FFmpeg 编码失败：{status}");
        }
        Ok(())
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
