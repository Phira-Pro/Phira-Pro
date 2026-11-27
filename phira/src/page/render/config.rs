use crate::dir;
use anyhow::{ensure, Context, Result};
use prpr::chart_video::{PresentationOptions, VideoOptions, VisualOptions};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Aspect {
    #[default]
    Wide,
    Standard,
    Classic,
    Custom,
}

impl Aspect {
    pub fn next(self) -> Self {
        match self {
            Self::Wide => Self::Standard,
            Self::Standard => Self::Classic,
            Self::Classic => Self::Custom,
            Self::Custom => Self::Wide,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wide => "16:9",
            Self::Standard => "4:3",
            Self::Classic => "3:2",
            Self::Custom => "自定义",
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Prefs {
    pub out_dir: String,
    pub res: usize,
    pub aspect: Aspect,
    pub custom_aspect: [f64; 2],
    pub fps: usize,
    pub hitsound: bool,
    pub hud: bool,
    pub resource_pack: super::resource_pack::Selection,
    pub visuals: VisualOptions,
    pub presentation: PresentationOptions,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            out_dir: String::new(),
            res: 1,
            aspect: Aspect::Wide,
            custom_aspect: [16., 9.],
            fps: 1,
            hitsound: true,
            hud: false,
            resource_pack: Default::default(),
            visuals: Default::default(),
            presentation: PresentationOptions::default(),
        }
    }
}

impl Prefs {
    pub fn dimensions(&self) -> Result<(u32, u32)> {
        let [width, height] = match self.aspect {
            Aspect::Wide => [16., 9.],
            Aspect::Standard => [4., 3.],
            Aspect::Classic => [3., 2.],
            Aspect::Custom => self.custom_aspect,
        };
        let ratio = width / height;
        ensure!(width > 0. && height > 0. && ratio.is_finite() && ratio > 0., "画面比例需要两个正数");
        let base = f64::from(super::RESOLUTION_HEIGHTS[self.res.min(super::RESOLUTION_HEIGHTS.len() - 1)]);
        let max_width = f64::from(VideoOptions::MAX_WIDTH);
        let h = base.min(max_width / ratio);
        let w = (h * ratio).min(max_width);
        let even = |size: f64| ((size / 2.).round() * 2.) as u32;
        let (w, h) = (even(w), even(h));
        ensure!(w >= VideoOptions::MIN_DIMENSION && h >= VideoOptions::MIN_DIMENSION, "此比例在当前分辨率下无法输出有效画面");
        Ok((w, h))
    }

    pub fn custom_aspect_text(&self) -> String {
        format!("{}:{}", self.custom_aspect[0], self.custom_aspect[1])
    }

    pub fn set_custom_aspect(&mut self, input: &str) -> Result<()> {
        let (w, h) = input.split_once([':', '：']).context("请按宽:高输入，例如 21:9")?;
        let parse = |value: &str| -> Result<f64> {
            let value = value.trim().parse::<f64>().context("画面比例需要两个正数")?;
            ensure!(value.is_finite() && value > 0., "画面比例需要两个正数");
            Ok(value)
        };
        self.custom_aspect = [parse(w)?, parse(h)?];
        self.aspect = Aspect::Custom;
        Ok(())
    }
}
pub(super) fn data_root() -> PathBuf {
    PathBuf::from(dir::root().unwrap_or_else(|_| "data".into()))
}
pub(super) fn load_prefs() -> Prefs {
    let mut prefs: Prefs = std::fs::read_to_string(data_root().join("render.yml"))
        .ok()
        .and_then(|v| serde_yaml::from_str(&v).ok())
        .unwrap_or_default();
    prefs.res = prefs.res.min(super::RESOLUTION_HEIGHTS.len() - 1);
    if prefs.dimensions().is_err() {
        prefs.aspect = Aspect::Wide;
        prefs.custom_aspect = Prefs::default().custom_aspect;
    }
    prefs.fps = prefs.fps.min(super::FPS_CHOICES.len() - 1);
    if prefs.presentation.validate().is_err() {
        prefs.presentation = PresentationOptions::default();
    }
    prefs
}
pub(super) fn save_prefs(prefs: &Prefs) {
    let result = serde_yaml::to_string(prefs)
        .map_err(anyhow::Error::from)
        .and_then(|value| std::fs::write(data_root().join("render.yml"), value).map_err(anyhow::Error::from));
    if let Err(err) = result {
        tracing::warn!("保存渲染设置失败：{err}");
    }
}
pub(super) fn sanitize(name: &str) -> String {
    let value: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "chart".into()
    } else {
        value.into()
    }
}
pub(super) fn fmt_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.).round() as u64;
    if seconds >= 60 {
        format!("{} 分 {} 秒", seconds / 60, seconds % 60)
    } else {
        format!("{seconds} 秒")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_keep_the_original_chart_only_timeline() {
        let prefs: Prefs = serde_yaml::from_str("res: 1\nfps: 1\nhitsound: true\nhud: false\nout_dir: ''\n").unwrap();
        assert!(!prefs.presentation.opening && prefs.presentation.playing && !prefs.presentation.ending);
        assert_eq!(prefs.presentation.enter_seconds, 0.);
        assert_eq!(prefs.resource_pack, super::super::resource_pack::Selection::FollowGame);
        assert!(!prefs.visuals.offset_visible(prefs.hud));
        let hud: Prefs = serde_yaml::from_str("hud: true").unwrap();
        assert!(hud.visuals.offset_visible(hud.hud));
    }

    #[test]
    fn export_pack_and_indicators_round_trip_without_gameplay_preferences() {
        let mut prefs = Prefs::default();
        for selection in [
            super::super::resource_pack::Selection::FollowGame,
            super::super::resource_pack::Selection::Builtin,
            super::super::resource_pack::Selection::Imported("skin-b".into()),
        ] {
            prefs.resource_pack = selection.clone();
            prefs.visuals.offset_indicator = Some(true);
            prefs.visuals.show_acc = true;
            prefs.visuals.show_judgement_details = true;
            let restored: Prefs = serde_yaml::from_str(&serde_yaml::to_string(&prefs).unwrap()).unwrap();
            assert_eq!(restored.resource_pack, selection);
            assert!(restored.visuals.offset_visible(false));
            assert!(restored.visuals.show_acc && restored.visuals.show_judgement_details);
        }
    }
}
