//! Frame-quantized movie time, separate from chart and audio time.
use super::PresentationOptions;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStage {
    Opening,
    Entering,
    Playing,
    Exiting,
    Ending,
}

impl RenderStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Opening => "开场",
            Self::Entering => "游玩入场",
            Self::Playing => "谱面播放",
            Self::Exiting => "游玩退场",
            Self::Ending => "结算",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub stage: RenderStage,
    pub first_frame: u64,
    pub frames: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameContext {
    pub stage: RenderStage,
    pub frame: u64,
    pub video_time: f64,
    pub stage_time: f64,
    pub stage_duration: f64,
    pub stage_progress: f32,
    /// None for presentation stages, music-relative seconds for chart stages.
    pub music_time: Option<f64>,
    pub delta: f64,
    pub width: u32,
    pub height: u32,
    pub preview: bool,
}

#[derive(Clone, Debug)]
pub struct RenderTimeline {
    pub segments: Vec<Segment>,
    pub fps: u32,
    pub music_start: f64,
    pub music_duration: f64,
    pub total_frames: u64,
}

impl RenderTimeline {
    pub fn new(options: &PresentationOptions, fps: u32, start: f64, duration: f64) -> Result<Self> {
        options.validate()?;
        ensure!(fps > 0 && start.is_finite() && start >= 0. && duration.is_finite() && duration > 0., "视频时间轴参数无效");
        let mut timeline = Self {
            segments: vec![],
            fps,
            music_start: start,
            music_duration: duration,
            total_frames: 0,
        };
        for (stage, enabled, seconds) in [
            (RenderStage::Opening, options.opening, options.opening_seconds),
            (RenderStage::Entering, options.playing, options.enter_seconds),
            (RenderStage::Playing, options.playing, duration),
            (RenderStage::Exiting, options.playing, options.exit_seconds),
            (RenderStage::Ending, options.ending, options.ending_seconds),
        ] {
            if enabled && seconds > 0. {
                let frames = (seconds * fps as f64).round().max(1.) as u64;
                timeline.segments.push(Segment {
                    stage,
                    first_frame: timeline.total_frames,
                    frames,
                });
                timeline.total_frames += frames;
            }
        }
        Ok(timeline)
    }

    pub fn duration(&self) -> f64 {
        self.total_frames as f64 / self.fps as f64
    }

    pub fn segment(&self, stage: RenderStage) -> Option<Segment> {
        self.segments.iter().find(|s| s.stage == stage).copied()
    }

    /// Include the last frame's coverage in HUD progress without advancing
    /// its music or chart sample. Partial exports retain the song position.
    pub fn hud_progress(&self, context: &FrameContext, track_length: f64) -> f32 {
        if !track_length.is_finite() || track_length <= 0. {
            return 0.;
        }
        let position = match context.stage {
            RenderStage::Opening | RenderStage::Entering => self.music_start,
            RenderStage::Playing => {
                let progress = if context.stage_duration <= context.delta { 1. } else { context.stage_progress as f64 };
                self.music_start + self.music_duration * progress
            }
            RenderStage::Exiting | RenderStage::Ending => self.music_start + self.music_duration,
        };
        (position / track_length).clamp(0., 1.) as f32
    }

    pub fn frame(&self, frame: u64, width: u32, height: u32) -> Option<FrameContext> {
        let segment = self
            .segments
            .iter()
            .find(|s| frame >= s.first_frame && frame < s.first_frame + s.frames)?;
        let local = frame - segment.first_frame;
        let music_time = match segment.stage {
            RenderStage::Entering => Some(self.music_start),
            RenderStage::Playing => Some(self.music_start + local as f64 / self.fps as f64),
            RenderStage::Exiting => Some(self.music_start + self.music_duration),
            _ => None,
        };
        Some(FrameContext {
            stage: segment.stage,
            frame,
            video_time: frame as f64 / self.fps as f64,
            stage_time: local as f64 / self.fps as f64,
            stage_duration: segment.frames as f64 / self.fps as f64,
            stage_progress: local as f32 / segment.frames.saturating_sub(1).max(1) as f32,
            music_time,
            delta: 1. / self.fps as f64,
            width,
            height,
            preview: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hud_reaches_the_end_without_advancing_the_last_audio_or_judgement_sample() {
        for fps in [1, 30, 60, 120, 240] {
            for duration in [2., 2.007, 0.25 / fps as f64] {
                let timeline = RenderTimeline::new(&PresentationOptions::phigros(), fps, 0., duration).unwrap();
                let playing = timeline.segment(RenderStage::Playing).unwrap();
                let mut previous = 0.;
                for index in 0..playing.frames {
                    let context = timeline.frame(playing.first_frame + index, 1280, 720).unwrap();
                    let progress = timeline.hud_progress(&context, duration);
                    assert!(progress >= previous && progress <= 1.);
                    previous = progress;
                    assert!(context.music_time.unwrap() < duration);
                }
                assert_eq!(previous, 1.);
            }
        }
    }

    #[test]
    fn hud_keeps_absolute_song_position_during_clips_and_presentation_stages() {
        let mut options = PresentationOptions::phigros();
        options.enter_seconds = 0.5;
        options.exit_seconds = 0.5;
        let timeline = RenderTimeline::new(&options, 60, 2., 4.).unwrap();
        for segment in &timeline.segments {
            let first = timeline.frame(segment.first_frame, 1280, 720).unwrap();
            let last = timeline.frame(segment.first_frame + segment.frames - 1, 1280, 720).unwrap();
            let (a, b) = match segment.stage {
                RenderStage::Opening | RenderStage::Entering => (0.2, 0.2),
                RenderStage::Playing => (0.2, 0.6),
                RenderStage::Exiting | RenderStage::Ending => (0.6, 0.6),
            };
            assert!((timeline.hud_progress(&first, 10.) - a).abs() < 1e-6);
            assert!((timeline.hud_progress(&last, 10.) - b).abs() < 1e-6);
        }
    }
    #[test]
    fn boundaries_have_no_duplicate_or_missing_frames_and_audio_starts_with_playing() {
        for fps in [30, 60, 120, 240] {
            let timeline = RenderTimeline::new(&PresentationOptions::phigros(), fps, 1.25, 2.).unwrap();
            let mut next = 0;
            for segment in &timeline.segments {
                assert_eq!(segment.first_frame, next);
                assert_eq!(timeline.frame(next, 1280, 720).unwrap().stage, segment.stage);
                next += segment.frames;
                assert_eq!(timeline.frame(next - 1, 1280, 720).unwrap().stage_progress, 1.);
            }
            assert!(timeline.frame(next, 1280, 720).is_none());
            assert_eq!(
                timeline
                    .frame(timeline.segment(RenderStage::Playing).unwrap().first_frame, 1280, 720)
                    .unwrap()
                    .music_time,
                Some(1.25)
            );
        }
    }
    #[test]
    fn legacy_and_presentation_only_exports_keep_their_expected_duration() {
        let timeline = RenderTimeline::new(&PresentationOptions::default(), 60, 0., 2.).unwrap();
        assert_eq!(timeline.total_frames, 120);
        let mut options = PresentationOptions::phigros();
        options.playing = false;
        let timeline = RenderTimeline::new(&options, 60, 0., 2.).unwrap();
        assert!(timeline.segment(RenderStage::Playing).is_none());
        assert_eq!(timeline.total_frames, 708);
        options.opening = false;
        options.ending = false;
        assert!(RenderTimeline::new(&options, 60, 0., 2.).is_err());
    }
}
