//! 铺面视频渲染（Phi Recorder 风格）。
//!
//! 把 `GameScene`（自动播放）逐帧画到离屏 `RenderTarget`，读回像素后经管道交给
//! 系统 `ffmpeg` 编码成 mp4，可选混入谱面音乐。整个过程不播放音频、不显示 HUD，
//! 输出的每一帧只取决于时间戳，因此可以任意断点/加速渲染。

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
};

use sasa::{AudioClip, Frame};

use anyhow::{bail, Context, Result};
use macroquad::{prelude::*, window::get_internal_gl};

use crate::{
    config::{Config, Mods},
    core::{BadNote, Chart, HitSound},
    ext::SafeTexture,
    fs::{fs_from_file, load_info},
    scene::{GameMode, GameScene, LoadingScene, Scene},
    time::TimeManager,
    ui::{TextPainter, Ui},
};

/// 渲染参数。
#[derive(Clone, Debug)]
pub struct VideoOptions {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// 起始时间（音乐秒）。
    pub start: f64,
    /// 结束时间（音乐秒），`None` 表示整曲。
    pub end: Option<f64>,
    /// 是否混入打击音（音乐始终会混入）。
    pub hitsound: bool,
    /// 是否画分数/连击/暂停按钮等 HUD（默认不画，只留谱面）。
    pub show_hud: bool,
    /// MSAA 采样数。
    pub sample_count: u32,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60,
            start: 0.,
            end: None,
            hitsound: true,
            show_hud: false,
            sample_count: 4,
        }
    }
}

impl VideoOptions {
    pub fn validate(&self) -> Result<()> {
        if self.fps == 0 || self.fps > 240 {
            bail!("帧率必须在 1~240 之间");
        }
        if self.width < 16 || self.height < 16 || self.width > 7680 || self.height > 4320 {
            bail!("分辨率超出支持范围");
        }
        if !self.start.is_finite() || self.start < 0. {
            bail!("起始时间无效");
        }
        Ok(())
    }

    pub fn frame_count(&self, duration: f64) -> u64 {
        ((duration.max(0.)) * self.fps as f64).round() as u64
    }
}

/// ffmpeg 可执行文件的候选列表（按优先级排列）。
///
/// 桌面端：环境变量 `PHIRA_FFMPEG` → PATH 里的 `ffmpeg`。
/// Android：APK 里的 `libffmpeg.so` 会随安装解压到 nativeLibraryDir，
/// 但这个文件通常是 0644（只够 `dlopen`），直接 `execve` 会拿到
/// `EACCES`；所以再准备一份拷到可写目录并 `chmod 755` 的副本兜底。
/// 两者都会尝试，谁能跑起来就用谁。
pub fn ffmpeg_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(path) = std::env::var_os("PHIRA_FFMPEG") {
        list.push(PathBuf::from(path));
    }
    for path in bundled_ffmpeg_candidates() {
        if !list.contains(&path) {
            list.push(path);
        }
    }
    list.push(PathBuf::from("ffmpeg"));
    list
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

/// ffmpeg 管道编码器。
struct Encoder {
    child: Child,
    stdin: ChildStdin,
}

impl Encoder {
    fn spawn(opts: &VideoOptions, out_path: &Path, music: Option<&Path>, hitsound: Option<&Path>, start: f64, duration: f64) -> Result<Self> {
        let mut errors = Vec::new();
        for program in ffmpeg_candidates() {
            match Self::spawn_with(&program, opts, out_path, music, hitsound, start, duration) {
                Ok(encoder) => return Ok(encoder),
                Err(err) => errors.push(format!("{}：{err}", program.display())),
            }
        }
        bail!("无法启动 ffmpeg（尝试过 {} 处可执行文件）：{}", errors.len(), errors.join("；"));
    }

    fn spawn_with(
        program: &Path,
        opts: &VideoOptions,
        out_path: &Path,
        music: Option<&Path>,
        hitsound: Option<&Path>,
        start: f64,
        duration: f64,
    ) -> Result<Self> {
        let mut cmd = Command::new(program);
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-video_size")
            .arg(format!("{}x{}", opts.width, opts.height))
            .arg("-framerate")
            .arg(opts.fps.to_string())
            .arg("-i")
            .arg("pipe:0");
        if let Some(music) = music {
            cmd.arg("-ss").arg(format!("{start:.6}")).arg("-i").arg(music);
        }
        if let Some(hitsound) = hitsound {
            cmd.arg("-f").arg("wav").arg("-i").arg(hitsound);
        }
        cmd.arg("-vf").arg("vflip");
        cmd.arg("-c:v").arg("libx264").arg("-preset").arg("veryfast").arg("-crf").arg("18");
        cmd.arg("-pix_fmt").arg("yuv420p");
        match (music.is_some(), hitsound.is_some()) {
            // 音乐按起始时间裁剪，打击音轨道已经对齐到视频起点。
            (true, true) => {
                cmd.arg("-filter_complex")
                    .arg("[1:a]apad[m];[m][2:a]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[a]")
                    .arg("-map")
                    .arg("0:v")
                    .arg("-map")
                    .arg("[a]");
                cmd.arg("-c:a").arg("aac").arg("-b:a").arg("192k");
            }
            (true, false) => {
                cmd.arg("-c:a").arg("aac").arg("-b:a").arg("192k").arg("-af").arg("apad");
            }
            (false, true) => {
                cmd.arg("-c:a").arg("aac").arg("-b:a").arg("192k");
            }
            (false, false) => {}
        }
        cmd.arg("-t").arg(format!("{duration:.6}"));
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(out_path);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::inherit());
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().context("无法获取 ffmpeg 输入管道")?;
        Ok(Self { child, stdin })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.stdin.write_all(bytes).context("写入视频帧失败（ffmpeg 可能已退出）")
    }

    fn finish(mut self) -> Result<()> {
        drop(self.stdin);
        let status = self.child.wait().context("等待 ffmpeg 退出失败")?;
        if !status.success() {
            bail!("ffmpeg 编码失败：{status}");
        }
        Ok(())
    }
}

/// 正在进行的铺面视频渲染。
pub struct ChartVideoRenderer {
    scene: GameScene,
    capture: RenderTarget,
    opts: VideoOptions,
    tm: TimeManager,
    bad_notes: Vec<BadNote>,
    /// 谱面时间 = 音乐时间 - offset。
    offset: f64,
    /// 实际起始时间（音乐秒，已按曲长裁过）。
    start: f64,
    duration: f64,
    total_frames: u64,
    frame: u64,
    encoder: Option<Encoder>,
    out_path: PathBuf,
    music_temp: Option<PathBuf>,
    hitsound_temp: Option<PathBuf>,
    buf: Vec<u8>,
    title: String,
}

impl ChartVideoRenderer {
    /// 加载谱面并启动编码器。`source` 可以是谱面目录，也可以是 `.pez` / `.zip` 文件。
    pub async fn new(source: PathBuf, opts: VideoOptions, out_path: PathBuf) -> Result<Self> {
        opts.validate()?;
        let mut fs = fs_from_file(&source).with_context(|| format!("无法打开谱面：{}", source.display()))?;
        let info = load_info(fs.as_mut())
            .await
            .with_context(|| format!("无法读取谱面信息：{}", source.display()))?;
        // 没有曲绘时用一个纯色贴图兜底，谱面本身仍然可以渲染。
        let (illustration, background) = match LoadingScene::load(fs.as_mut(), &info.illustration).await {
            Ok((illustration, background, _)) => (illustration, background),
            Err(err) => {
                tracing::warn!("曲绘加载失败，改用纯色背景：{err:?}");
                let tex: SafeTexture = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into();
                (tex.clone(), tex)
            }
        };

        let music_temp = if !info.music.is_empty() {
            match fs.load_file(&info.music).await {
                Ok(bytes) => {
                    let ext = Path::new(&info.music)
                        .extension()
                        .and_then(|it| it.to_str())
                        .filter(|it| !it.is_empty())
                        .unwrap_or("dat");
                    let path = std::env::temp_dir().join(format!("phira-render-{}.{ext}", uuid::Uuid::new_v4()));
                    match std::fs::write(&path, &bytes) {
                        Ok(()) => Some(path),
                        Err(err) => {
                            tracing::warn!("音乐写入临时文件失败，本次没有音乐：{err:?}");
                            None
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!("谱面音乐读取失败，本次没有音乐：{err:?}");
                    None
                }
            }
        } else {
            None
        };

        let mut config = Config::default();
        config.mods = Mods::AUTOPLAY;
        config.volume_music = 0.;
        config.volume_sfx = 0.;
        config.sample_count = opts.sample_count.clamp(1, 8);
        config.show_avg_fps = false;
        config.show_fps = false;
        config.offset = 0.;
        config.speed = 1.;
        config.adjust_time = false;
        config.interactive = false;

        let mut scene = GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None)
            .await
            .context("无法加载谱面场景")?;
        scene.hide_ui = !opts.show_hud;
        let capture = render_target(opts.width, opts.height);
        capture.texture.set_filter(FilterMode::Linear);
        scene.res.camera.render_target = Some(capture);

        let offset = scene.offset() as f64;
        let track_length = scene.res.track_length;
        let start = opts.start.clamp(0., track_length);
        let end = opts.end.unwrap_or(track_length).clamp(start, track_length);
        let duration = (end - start).max(0.);
        let total_frames = opts.frame_count(duration);
        if total_frames == 0 {
            bail!("渲染时长为 0，请检查音乐文件或起始/结束时间");
        }
        // 打击音的时间点只和谱面有关，渲染前先混成一条音轨，
        // 再由 ffmpeg 跟音乐一起 amix。
        let hitsound_temp = if opts.hitsound {
            match write_hitsound(&scene.chart, offset, start, duration).await {
                Ok(path) => Some(path),
                Err(err) => {
                    tracing::warn!("打击音混音失败，本次不带打击音：{err:?}");
                    None
                }
            }
        } else {
            None
        };
        let title = scene.res.info.name.clone();
        if let Some(dir) = out_path.parent().filter(|it| !it.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("无法创建输出目录：{}", dir.display()))?;
        }
        let encoder = Encoder::spawn(&opts, &out_path, music_temp.as_deref(), hitsound_temp.as_deref(), start, duration)?;
        let buf = vec![0; opts.width as usize * opts.height as usize * 4];

        Ok(Self {
            scene,
            capture,
            opts,
            tm: TimeManager::manual(Box::new(|| 1000.)),
            bad_notes: Vec::new(),
            offset,
            start,
            duration,
            total_frames,
            frame: 0,
            encoder: Some(encoder),
            out_path,
            music_temp,
            hitsound_temp,
            buf,
            title,
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn progress(&self) -> (u64, u64) {
        (self.frame, self.total_frames)
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn options(&self) -> &VideoOptions {
        &self.opts
    }

    /// 上一次渲染完成的画面，可用于界面预览。
    pub fn texture(&self) -> Texture2D {
        self.capture.texture
    }

    /// 上一次渲染完成的原始像素（自下而上，RGBA）。
    pub fn frame_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn finished(&self) -> bool {
        self.frame >= self.total_frames
    }

    /// 渲染下一帧（不调用 `next_frame`，可在同一渲染帧内连续调用）。
    pub fn render_next_frame(&mut self, painter: &mut TextPainter) -> Result<()> {
        if self.finished() {
            return Ok(());
        }
        let music_time = self.start + self.frame as f64 / self.opts.fps as f64;
        let time = music_time - self.offset;
        self.scene.res.time = time;
        self.scene.res.alpha = 1.;
        self.tm.seek_to(time);
        self.scene.judge.update(&mut self.scene.res, &mut self.scene.chart, &mut self.bad_notes);
        self.scene.tick_replay();
        self.scene.set_replay_frame_delta(1.0 / self.opts.fps as f32);
        {
            let mut ui = Ui::new(painter, Some((0, 0, self.opts.width as i32, self.opts.height as i32)));
            self.scene.render(&mut self.tm, &mut ui).context("渲染谱面失败")?;
        }
        unsafe { get_internal_gl() }.flush();
        self.read_pixels();
        if let Some(encoder) = &mut self.encoder {
            encoder.write(&self.buf)?;
        }
        self.frame += 1;
        Ok(())
    }

    fn read_pixels(&mut self) {
        let (width, height) = (self.opts.width as usize, self.opts.height as usize);
        unsafe {
            use miniquad::gl::*;
            let mut previous = 0;
            glGetIntegerv(0x8CAA, &mut previous);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, self.capture.render_pass.gl_internal_id(get_internal_gl().quad_context));
            glReadPixels(0, 0, width as i32, height as i32, GL_RGBA, GL_UNSIGNED_BYTE, self.buf.as_mut_ptr() as _);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, previous as u32);
        }
    }

    /// 结束编码并校验输出文件。必须在渲染完所有帧后调用。
    pub fn finish(mut self) -> Result<PathBuf> {
        let encoder = self.encoder.take().context("编码器已关闭")?;
        let result = encoder.finish();
        let path = self.out_path.clone();
        self.cleanup_temp_audio();
        result?;
        if !path.exists() || std::fs::metadata(&path).map(|it| it.len()).unwrap_or(0) == 0 {
            bail!("视频文件未能生成：{}", path.display());
        }
        Ok(path)
    }

    /// 取消渲染：杀掉 ffmpeg、删除半成品与临时文件。
    pub fn cancel(&mut self) {
        if let Some(mut encoder) = self.encoder.take() {
            drop(encoder.stdin);
            let _ = encoder.child.kill();
            let _ = encoder.child.wait();
        }
        self.cleanup_temp_audio();
        let _ = std::fs::remove_file(&self.out_path);
    }

    fn cleanup_temp_audio(&mut self) {
        for path in [self.music_temp.take(), self.hitsound_temp.take()].into_iter().flatten() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for ChartVideoRenderer {
    fn drop(&mut self) {
        if self.encoder.is_some() {
            // 没走 finish / cancel 就销毁（例如加载新任务），不能留下半成品。
            self.cancel();
        } else {
            self.cleanup_temp_audio();
        }
    }
}

/// 打击音的混音采样率。
const HITSOUND_RATE: u32 = 44100;

/// 把这些帧写成一个 16 位 PCM 的 wav 文件（sasa 的帧是 f32 立体声）。
fn write_wav(path: &Path, frames: &[Frame], rate: u32) -> std::io::Result<()> {
    let data_len = (frames.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for frame in frames {
        for sample in [frame.0, frame.1] {
            let value = (sample.clamp(-1., 1.) * i16::MAX as f32) as i16;
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    std::fs::write(path, out)
}

/// 读一个默认打击音。顺序和 `ResourcePack` 里一致：ogg、wav、mp3，最后退回内置资源。
async fn load_default_clip(fs: &mut dyn crate::fs::FileSystem, name: &str) -> Result<AudioClip> {
    for ext in ["ogg", "wav", "mp3"] {
        let path = format!("{name}.{ext}");
        if let Ok(bytes) = fs.load_file(&path).await {
            return AudioClip::new(bytes).with_context(|| format!("无法解析打击音 {path}"));
        }
    }
    let bytes = load_file(&format!("{name}.ogg")).await?;
    AudioClip::new(bytes).with_context(|| format!("无法解析内置打击音 {name}.ogg"))
}

/// 把谱面的打击音按时间点混成一条音轨，返回临时 wav 路径。
///
/// 视频从 `start` 开始，音轨整体前移，和首帧对齐；`offset` 是谱面时间到音乐时间的偏移。
async fn write_hitsound(chart: &Chart, offset: f64, start: f64, duration: f64) -> Result<PathBuf> {
    let total = (duration * HITSOUND_RATE as f64).ceil().max(0.) as usize;
    if total == 0 {
        bail!("渲染时长为 0");
    }
    let mut fs = crate::fs::fs_from_assets("respack/")?;
    let mut clips = std::collections::HashMap::new();
    for name in ["click", "drag", "flick"] {
        clips.insert(name, load_default_clip(fs.as_mut(), name).await?);
    }
    let mut mix = vec![Frame::default(); total];
    for line in &chart.lines {
        for note in &line.notes {
            if note.fake {
                continue;
            }
            let at = note.time + offset - start;
            if at < 0. || at > duration {
                continue;
            }
            let clip = match &note.hitsound {
                HitSound::None => continue,
                HitSound::Click => &clips["click"],
                HitSound::Flick => &clips["flick"],
                HitSound::Drag => &clips["drag"],
                // 没找到的自定义打击音退回点击音，至少不会静音。
                HitSound::Custom(name) => chart.hitsounds.get(name).unwrap_or(&clips["click"]),
            };
            let begin = (at * HITSOUND_RATE as f64).round() as usize;
            let len = (clip.length() * HITSOUND_RATE as f64).ceil() as usize;
            for i in 0..len {
                let Some(slot) = mix.get_mut(begin + i) else {
                    break;
                };
                if let Some(frame) = clip.sample(i as f64 / HITSOUND_RATE as f64) {
                    *slot = *slot + frame;
                }
            }
        }
    }
    // 和弦叠加容易冲顶，留一点余量。
    let peak = mix.iter().map(|it| it.0.abs().max(it.1.abs())).fold(0., f32::max);
    if peak > 0.7 {
        let gain = 0.7 / peak;
        for frame in &mut mix {
            *frame = *frame * gain;
        }
    }
    let path = std::env::temp_dir().join(format!("phira-hitsound-{}.wav", uuid::Uuid::new_v4()));
    write_wav(&path, &mix, HITSOUND_RATE).with_context(|| format!("无法写入打击音轨道：{}", path.display()))?;
    Ok(path)
}
