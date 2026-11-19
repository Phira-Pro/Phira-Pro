use crate::dir;
use anyhow::Result;
#[cfg(feature = "video")]
use prpr::core::Video;
use std::path::Path;
#[cfg(feature = "video")]
use std::sync::Arc;

const VIDEO_FILENAME: &str = "background-video.mp4";

fn video_path() -> Result<std::path::PathBuf> {
    Ok(Path::new(&dir::appearance()?).join(VIDEO_FILENAME))
}

pub(crate) fn import(source: &Path) -> Result<()> {
    #[cfg(feature = "video")]
    if is_movie(source)? {
        use std::{fs::File, io};
        let destination = video_path()?;
        let mut file = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
        io::copy(&mut File::open(source)?, &mut file)?;
        prpr::core::Video::validate_file(file.path())?;
        file.persist(destination).map_err(|err| err.error)?;
        return Ok(());
    }
    dir::import_appearance("background", source)?;
    clear_video()
}

pub(crate) fn clear() -> Result<()> {
    dir::clear_appearance("background")?;
    clear_video()
}

fn clear_video() -> Result<()> {
    match std::fs::remove_file(video_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(feature = "video")]
fn is_movie(path: &Path) -> Result<bool> {
    use std::{fs::File, io::Read};

    // The system picker may replace the original file extension.
    let mut header = [0; 8];
    match File::open(path)?.read_exact(&mut header) {
        Ok(()) => Ok(matches!(&header[4..], b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free")),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(feature = "video")]
pub(super) struct BackgroundVideo {
    source: Arc<tempfile::NamedTempFile>,
    primary: Playback,
    secondary: Option<Playback>,
    smooth_requested: bool,
    started: f64,
    frame: LoopFrame,
}

#[cfg(feature = "video")]
struct Playback {
    video: Video,
    cycle: Option<u64>,
    position: f64,
}

#[cfg(feature = "video")]
impl Playback {
    fn open(source: Arc<tempfile::NamedTempFile>) -> Result<Self> {
        let video = Video::open_background_layer(source)?;
        anyhow::ensure!(video.duration.is_finite() && video.duration > 0., "video has no playable duration");
        Ok(Self { video, cycle: None, position: 0. })
    }

    fn update(&mut self, cycle: u64, position: f64, keep_frame: bool) -> Result<()> {
        if self.cycle != Some(cycle) {
            if self.cycle.is_some() {
                if keep_frame {
                    self.video.rewind();
                } else {
                    self.video.reset()?;
                }
            }
            self.cycle = Some(cycle);
        }
        self.position = position;
        self.video.update(position)
    }

    fn render(&self, aspect: f32, alpha: f32) -> bool {
        if !self.video.has_frame() {
            return false;
        }
        self.video.render(self.position, aspect, macroquad::prelude::Color::new(1., 1., 1., alpha));
        true
    }
}

#[cfg(feature = "video")]
#[derive(Default)]
struct LoopFrame {
    cycle: u64,
    position: f64,
    // Time in the outgoing clip and the linear opacity of the incoming clip.
    overlap: Option<(f64, f32)>,
}

#[cfg(feature = "video")]
impl LoopFrame {
    fn at(elapsed: f64, duration: f64, smooth: bool) -> Self {
        // At most two loops can be active, including for clips under two seconds.
        let fade = if smooth { 1.0_f64.min(duration / 2.) } else { 0. };
        let interval = duration - fade;
        let cycle = (elapsed / interval).floor() as u64;
        let position = (elapsed - cycle as f64 * interval).clamp(0., interval);
        let overlap = (cycle > 0 && position < fade).then(|| {
            // Keep the outgoing timestamp inside the decoder's half-open interval
            // if rounding makes interval + position land exactly on duration.
            ((interval + position).min(duration.next_down()), (position / fade) as f32)
        });
        Self { cycle, position, overlap }
    }
}

#[cfg(feature = "video")]
impl BackgroundVideo {
    pub(super) fn load(now: f64) -> Result<Option<Self>> {
        use std::{fs::File, io};

        let source = match File::open(video_path()?) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        // The stored file can be replaced immediately, even while the current
        // decoder holds its playback file open on Windows.
        let mut playback = tempfile::NamedTempFile::new()?;
        io::copy(&mut io::BufReader::new(source), &mut playback)?;
        Self::new(playback, now).map(Some)
    }

    fn new(playback: tempfile::NamedTempFile, now: f64) -> Result<Self> {
        let source = Arc::new(playback);
        let primary = Playback::open(Arc::clone(&source))?;
        Ok(Self {
            source,
            primary,
            secondary: None,
            smooth_requested: false,
            started: now,
            frame: LoopFrame::default(),
        })
    }

    pub(super) fn update(&mut self, now: f64, smooth: bool) -> Result<()> {
        if self.smooth_requested != smooth {
            let secondary = if smooth {
                match Playback::open(Arc::clone(&self.source)) {
                    Ok(playback) => Some(playback),
                    Err(error) => {
                        tracing::warn!(?error, "could not start background video crossfade");
                        None
                    }
                }
            } else {
                None
            };
            if secondary.is_some() || self.secondary.is_some() {
                self.started = now;
                self.primary.video.reset()?;
                self.primary.cycle = None;
            }
            self.secondary = secondary;
            self.smooth_requested = smooth;
        }

        let elapsed = (now - self.started).max(0.);
        let frame = LoopFrame::at(elapsed, self.primary.video.duration, self.secondary.is_some());
        if let Some(secondary) = &mut self.secondary {
            let (current, other) = if frame.cycle % 2 == 0 {
                (&mut self.primary, secondary)
            } else {
                (secondary, &mut self.primary)
            };
            current.update(frame.cycle, frame.position, false)?;
            if let Some((position, _)) = frame.overlap {
                other.update(frame.cycle - 1, position, false)?;
            } else {
                // Decode the opening frame in advance so the fade needs no
                // synchronous seek or allocation at the loop boundary.
                other.update(frame.cycle + 1, 0., false)?;
            }
        } else {
            self.primary.update(frame.cycle, frame.position, true)?;
        }
        self.frame = frame;
        Ok(())
    }

    pub(super) fn render(&self, aspect: f32) -> bool {
        let Some(secondary) = &self.secondary else {
            return self.primary.render(aspect, 1.);
        };
        let (current, other) = if self.frame.cycle % 2 == 0 {
            (&self.primary, secondary)
        } else {
            (secondary, &self.primary)
        };
        if let Some((_, alpha)) = self.frame.overlap {
            // Drawing the incoming frame over an opaque outgoing frame gives
            // weights alpha and 1-alpha without dimming through the menu image.
            if other.render(aspect, 1.) {
                if alpha > 0. {
                    current.render(aspect, alpha);
                }
                return true;
            }
        }
        current.render(aspect, 1.)
    }
}

#[cfg(all(test, feature = "video"))]
mod tests {
    use super::LoopFrame;

    #[test]
    fn crossfade_overlaps_the_last_and_first_second_without_restarting_either_clip() {
        let before = LoopFrame::at(4.75, 6., true);
        assert_eq!((before.cycle, before.position, before.overlap), (0, 4.75, None));
        let start = LoopFrame::at(5., 6., true);
        assert_eq!((start.cycle, start.position, start.overlap), (1, 0., Some((5., 0.))));
        let middle = LoopFrame::at(5.5, 6., true);
        assert_eq!((middle.cycle, middle.position, middle.overlap), (1, 0.5, Some((5.5, 0.5))));
        let end = LoopFrame::at(6., 6., true);
        assert_eq!((end.cycle, end.position, end.overlap), (1, 1., None));
        let next = LoopFrame::at(10.75, 6., true);
        assert_eq!((next.cycle, next.position, next.overlap), (2, 0.75, Some((5.75, 0.75))));
    }

    #[test]
    fn short_clips_have_two_layers_at_most_and_disabled_mode_keeps_the_full_period() {
        let short = LoopFrame::at(0.375, 0.5, true);
        assert_eq!((short.cycle, short.position, short.overlap), (1, 0.125, Some((0.375, 0.5))));
        let plain = LoopFrame::at(6., 6., false);
        assert_eq!((plain.cycle, plain.position, plain.overlap), (1, 0., None));
        let config: prpr::config::Config = serde_json::from_str("{}").unwrap();
        assert!(!config.background_video_crossfade);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires desktop OpenGL and FFmpeg"]
    fn background_crossfade_blends_decoded_pixels_on_both_loop_boundaries() {
        use super::BackgroundVideo;
        use macroquad::prelude::*;
        use std::time::{Duration, Instant};

        async fn sample(background: &mut BackgroundVideo, time: f64, expected: impl Fn(Color) -> bool) -> Color {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                background.update(time, true).unwrap();
                set_camera(&Camera2D { zoom: vec2(1., -1.), ..Default::default() });
                clear_background(GREEN);
                background.render(1.);
                unsafe { get_internal_gl() }.flush();
                let frame = get_screen_data();
                let pixel = frame.get_pixel(u32::from(frame.width) / 2, u32::from(frame.height) / 2);
                if expected(pixel) {
                    return pixel;
                }
                assert!(Instant::now() < deadline, "unexpected pixel at {time}s: {pixel:?}");
                next_frame().await;
            }
        }

        let fixture = tempfile::Builder::new().suffix(".mp4").tempfile().unwrap();
        let ffmpeg = std::env::var_os("PHIRA_TEST_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        let status = std::process::Command::new(ffmpeg)
            .args([
                "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=blue:s=48x32:r=25:d=2",
                "-f", "lavfi", "-i", "color=c=red:s=48x32:r=25:d=2",
                "-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0", "-c:v", "libx264", "-bf", "0",
            ])
            .arg(fixture.path())
            .status()
            .unwrap();
        assert!(status.success());
        macroquad::Window::from_config(
            Conf { window_width: 160, window_height: 160, headless: true, ..Default::default() },
            async move {
                next_frame().await;
                let mut background = BackgroundVideo::new(fixture, 0.).unwrap();
                let blue = sample(&mut background, 0., |c| c.b > 0.75 && c.r < 0.1 && c.g < 0.1).await;
                let red = sample(&mut background, 3., |c| c.r > 0.75 && c.b < 0.1 && c.g < 0.1).await;
                let midpoint = Color::new((red.r + blue.r) / 2., (red.g + blue.g) / 2., (red.b + blue.b) / 2., 1.);
                for (time, expected) in [(3.5, midpoint), (4., blue), (6.5, midpoint)] {
                    sample(&mut background, time, |c| {
                        (c.r - expected.r).abs() < 0.04 && (c.g - expected.g).abs() < 0.04 && (c.b - expected.b).abs() < 0.04
                    }).await;
                }
            },
        );
    }
}
