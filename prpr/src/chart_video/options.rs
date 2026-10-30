//! Export-only configuration. These values never modify gameplay preferences.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoOptions {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub start: f64,
    pub end: Option<f64>,
    pub hitsound: bool,
    pub show_hud: bool,
    pub sample_count: u32,
    pub presentation: PresentationOptions,
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
            presentation: PresentationOptions::default(),
        }
    }
}

impl VideoOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=240).contains(&self.fps), "帧率必须在 1 到 240 之间");
        ensure!(
            (16..=7680).contains(&self.width) && (16..=4320).contains(&self.height) && self.width % 2 == 0 && self.height % 2 == 0,
            "分辨率需为有效偶数尺寸"
        );
        ensure!(self.start.is_finite() && self.start >= 0., "起始时间无效");
        ensure!(self.end.is_none_or(|end| end.is_finite() && end > self.start), "结束时间必须晚于开始时间");
        self.presentation.validate()
    }
    pub fn frame_count(&self, duration: f64) -> u64 {
        (duration.max(0.) * self.fps as f64).round() as u64
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationStyle {
    #[default]
    Phira,
    Phigros,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioOptions {
    pub music_volume: f32,
    pub hitsound_volume: f32,
    pub ending_volume: f32,
    pub ending_delay_seconds: f64,
    pub fade_seconds: f64,
}

impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            music_volume: 1.,
            hitsound_volume: 0.7,
            ending_volume: 0.8,
            ending_delay_seconds: 1.085,
            fade_seconds: 0.15,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayMetadata {
    /// Empty fields inherit the chart's metadata.
    pub title: String,
    pub composer: String,
    pub charter: String,
    pub illustrator: String,
    pub difficulty: String,
    pub level: String,
    pub superscript: String,
    pub subscript: String,
    pub tip: String,
    pub loading_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultDisplay {
    pub player_name: String,
    pub avatar: Option<PathBuf>,
    pub rks: Option<f32>,
    pub challenge: String,
    /// 0 neutral, 1 green, 2 blue, 3 red, 4 gold, 5 rainbow.
    pub challenge_color: usize,
    pub score_increase: String,
    pub collected_title: String,
    pub collected_text: String,
    /// Display overrides only; no score records or uploads are created.
    pub custom: bool,
    pub score: u32,
    pub accuracy: f64,
    pub max_combo: u32,
    pub counts: [u32; 4],
}

impl Default for ResultDisplay {
    fn default() -> Self {
        Self {
            player_name: "Player".into(),
            avatar: None,
            rks: None,
            challenge: String::new(),
            challenge_color: 4,
            score_increase: String::new(),
            collected_title: "已获得".into(),
            collected_text: String::new(),
            custom: false,
            score: 1_000_000,
            accuracy: 100.,
            max_combo: 0,
            counts: [0; 4],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PresentationOptions {
    pub style: PresentationStyle,
    pub font: Option<PathBuf>,
    pub opening: bool,
    pub playing: bool,
    pub ending: bool,
    pub opening_seconds: f64,
    pub enter_seconds: f64,
    pub exit_seconds: f64,
    pub ending_seconds: f64,
    pub metadata: DisplayMetadata,
    pub result: ResultDisplay,
    pub audio: AudioOptions,
    /// Namespaced settings retained for future native or script adapters.
    pub effects: Vec<EffectSpec>,
}

impl Default for PresentationOptions {
    fn default() -> Self {
        Self {
            style: PresentationStyle::Phira,
            font: None,
            opening: false,
            playing: true,
            ending: false,
            opening_seconds: 5.8,
            enter_seconds: 0.,
            exit_seconds: 0.,
            ending_seconds: 6.,
            metadata: DisplayMetadata::default(),
            result: ResultDisplay::default(),
            audio: AudioOptions::default(),
            effects: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EffectSpec {
    pub id: String,
    pub version: u32,
    pub enabled: bool,
    pub settings: serde_json::Value,
}

impl PresentationOptions {
    pub fn phigros() -> Self {
        Self {
            style: PresentationStyle::Phigros,
            opening: true,
            ending: true,
            enter_seconds: 0.,
            exit_seconds: 0.,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.opening || self.playing || self.ending, "至少选择一个演出阶段");
        for (name, value, minimum, maximum) in [
            ("开场", self.opening_seconds, 1., 60.),
            ("入场", self.enter_seconds, 0., 5.),
            ("退场", self.exit_seconds, 0., 5.),
            ("结算", self.ending_seconds, 2., 120.),
        ] {
            ensure!(value.is_finite() && (minimum..=maximum).contains(&value), "{name}时长超出范围");
        }
        for gain in [self.audio.music_volume, self.audio.hitsound_volume, self.audio.ending_volume] {
            ensure!(gain.is_finite() && (0. ..=2.).contains(&gain), "音量必须在 0 到 2 之间");
        }
        ensure!(self.audio.fade_seconds.is_finite() && (0. ..=5.).contains(&self.audio.fade_seconds), "音频渐变时长无效");
        ensure!(
            self.audio.ending_delay_seconds.is_finite() && (0. ..=10.).contains(&self.audio.ending_delay_seconds),
            "结算音乐延迟必须在 0 到 10 秒之间"
        );
        ensure!(self.result.accuracy.is_finite() && (0. ..=100.).contains(&self.result.accuracy), "准度必须在 0 到 100 之间");
        ensure!(self.result.score <= 1_000_000, "展示分数不能超过 1000000");
        ensure!(self.result.rks.is_none_or(|v| v.is_finite() && (0. ..=100.).contains(&v)), "RKS 展示值无效");
        ensure!(self.result.challenge_color <= 5, "课题颜色无效");
        for text in [
            &self.metadata.title,
            &self.metadata.composer,
            &self.metadata.charter,
            &self.metadata.illustrator,
            &self.metadata.difficulty,
            &self.metadata.level,
            &self.metadata.superscript,
            &self.metadata.subscript,
            &self.metadata.tip,
            &self.metadata.loading_text,
            &self.result.player_name,
            &self.result.challenge,
            &self.result.score_increase,
            &self.result.collected_title,
            &self.result.collected_text,
        ] {
            ensure!(text.chars().count() <= 256, "展示文本不能超过 256 个字符");
        }
        Ok(())
    }
}
