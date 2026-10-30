//! Offline soundtrack construction on the movie timeline.
use super::{AudioOptions, RenderStage, RenderTimeline};
use anyhow::{ensure, Context, Result};
use sasa::{AudioClip, Frame};
use std::io::{BufWriter, Write};

pub const SAMPLE_RATE: u32 = 44100;

pub struct AudioPlacement {
    pub clip: AudioClip,
    pub at: f64,
    pub source_start: f64,
    pub duration: f64,
    pub gain: f32,
    pub fade: f64,
}

pub fn soundtrack(timeline: &RenderTimeline, music: Option<AudioClip>, ending: AudioClip, options: &AudioOptions) -> Vec<AudioPlacement> {
    let mut tracks = vec![];
    if let (Some(segment), Some(clip)) = (timeline.segment(RenderStage::Playing), music) {
        tracks.push(AudioPlacement {
            clip,
            at: segment.first_frame as f64 / timeline.fps as f64,
            source_start: timeline.music_start,
            duration: timeline.music_duration.min(segment.frames as f64 / timeline.fps as f64),
            gain: options.music_volume,
            fade: options.fade_seconds,
        });
    }
    if let Some(segment) = timeline.segment(RenderStage::Ending) {
        let duration = (segment.frames as f64 / timeline.fps as f64 - options.ending_delay_seconds)
            .max(0.)
            .min(ending.length());
        tracks.push(AudioPlacement {
            clip: ending,
            at: segment.first_frame as f64 / timeline.fps as f64 + options.ending_delay_seconds,
            source_start: 0.,
            duration,
            gain: options.ending_volume,
            fade: options.fade_seconds,
        });
    }
    tracks
}

pub fn mix(duration: f64, tracks: Vec<AudioPlacement>) -> Result<Vec<Frame>> {
    let count = (duration * SAMPLE_RATE as f64).ceil() as usize;
    ensure!(count <= (u32::MAX as usize - 36) / 4, "音频长度超过 WAV 支持范围，请分段导出");
    let mut frames = Vec::new();
    frames.try_reserve_exact(count).context("没有足够内存合成视频音轨")?;
    frames.resize(count, Frame::default());
    for placement in tracks {
        let first = (placement.at * SAMPLE_RATE as f64).round() as i64;
        let samples = (placement.duration.max(0.) * SAMPLE_RATE as f64).ceil() as usize;
        if placement.gain == 0. {
            continue;
        }
        for i in 0..samples {
            let target = first + i as i64;
            if target < 0 {
                continue;
            }
            let Some(output) = frames.get_mut(target as usize) else {
                break;
            };
            let time = i as f64 / SAMPLE_RATE as f64;
            if let Some(sample) = placement.clip.sample(placement.source_start + time) {
                let fade = envelope(time, placement.duration, placement.fade);
                *output = *output + sample * (placement.gain * fade);
            }
        }
    }
    Ok(frames)
}

fn envelope(time: f64, duration: f64, fade: f64) -> f32 {
    if fade <= 0. {
        1.
    } else {
        (time / fade).min((duration - time) / fade).clamp(0., 1.) as f32
    }
}

pub fn write(samples: &[Frame]) -> Result<tempfile::TempPath> {
    let mut file = tempfile::Builder::new().prefix("phira-video-audio-").suffix(".wav").tempfile()?;
    {
        let mut out = BufWriter::new(file.as_file_mut());
        let data_len = u32::try_from(samples.len() * 4)?;
        out.write_all(b"RIFF")?;
        out.write_all(&(36 + data_len).to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        for bytes in [
            16u32.to_le_bytes(),
            0x0002_0001u32.to_le_bytes(),
            SAMPLE_RATE.to_le_bytes(),
            (SAMPLE_RATE * 4).to_le_bytes(),
            0x0010_0004u32.to_le_bytes(),
        ] {
            out.write_all(&bytes)?;
        }
        out.write_all(b"data")?;
        out.write_all(&data_len.to_le_bytes())?;
        let peak = samples.iter().map(|v| v.0.abs().max(v.1.abs())).fold(1., f32::max);
        for frame in samples {
            for sample in [frame.0, frame.1] {
                let sample = if sample.is_finite() { (sample / peak).clamp(-1., 1.) } else { 0. };
                out.write_all(&((sample * i16::MAX as f32).round() as i16).to_le_bytes())?;
            }
        }
        out.flush()?;
    }
    Ok(file.into_temp_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_audio_stays_silent_during_opening_and_transitions() {
        let mut options = super::super::PresentationOptions::phigros();
        options.audio.fade_seconds = 0.;
        options.enter_seconds = 0.8;
        options.exit_seconds = 0.6;
        let timeline = RenderTimeline::new(&options, 30, 0., 2.).unwrap();
        let tone = AudioClip::from_raw(vec![Frame(0.25, -0.25); SAMPLE_RATE as usize * 2], SAMPLE_RATE);
        let ending = AudioClip::from_raw(vec![Frame(-0.5, 0.5); SAMPLE_RATE as usize * 6], SAMPLE_RATE);
        let frames = mix(timeline.duration(), soundtrack(&timeline, Some(tone), ending, &options.audio)).unwrap();
        let at = |seconds: f64| frames[(seconds * SAMPLE_RATE as f64).round() as usize];
        for time in [0., 5.79, 5.81, 6.59, 8.61, 9.19, 10.28] {
            assert_eq!(at(time).0, 0., "time={time}");
        }
        assert_eq!(at(6.6).0, 0.25);
        assert_eq!(at(8.59).1, -0.25);
        assert!((at(10.285).0 + 0.4).abs() < 1e-6);
        assert_eq!(frames.len(), (15.2 * SAMPLE_RATE as f64).ceil() as usize);
    }
    #[test]
    fn fades_are_bounded_and_wav_length_matches_the_movie() {
        assert_eq!(envelope(0., 2., 0.2), 0.);
        assert_eq!(envelope(1., 2., 0.2), 1.);
        assert_eq!(envelope(2., 2., 0.2), 0.);
        let frames = mix(0.1, vec![]).unwrap();
        let path = write(&frames).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 4410 * 4);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 4410 * 4);
    }
}
