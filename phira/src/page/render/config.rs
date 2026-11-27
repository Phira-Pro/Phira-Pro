use crate::dir;
use anyhow::{Context, Result};
use prpr::chart_video::{PresentationOptions, VideoOptions, VisualOptions};
use std::path::PathBuf;

pub(super) const RESOLUTION_PRESETS: [[u32; 2]; 12] = [
    [1280, 720], [1920, 1080], [2560, 1440], [3840, 2160],
    [960, 720], [1440, 1080], [1920, 1440], [2880, 2160],
    [1080, 720], [1620, 1080], [2160, 1440], [3240, 2160],
];
const DEFAULT_RESOLUTION: [u32; 2] = RESOLUTION_PRESETS[1];

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Aspect {
    #[default]
    Wide,
    Standard,
    Classic,
    Custom,
}

#[derive(Clone, serde::Deserialize)]
#[serde(default)]
struct LegacyResolution {
    res: usize,
    aspect: Aspect,
    custom_aspect: [f64; 2],
}

impl Default for LegacyResolution {
    fn default() -> Self {
        Self { res: 1, aspect: Aspect::Wide, custom_aspect: [16., 9.] }
    }
}

impl LegacyResolution {
    fn dimensions(&self) -> [u32; 2] {
        let index = self.res.min(3);
        match self.aspect {
            Aspect::Wide => RESOLUTION_PRESETS[index],
            Aspect::Standard => RESOLUTION_PRESETS[index + 4],
            Aspect::Classic => RESOLUTION_PRESETS[index + 8],
            Aspect::Custom => {
                let ratio = self.custom_aspect[0] / self.custom_aspect[1];
                if !ratio.is_finite() || ratio <= 0. || self.custom_aspect[0] <= 0. {
                    return RESOLUTION_PRESETS[index];
                }
                let base_height = f64::from(RESOLUTION_PRESETS[index][1]);
                let height = base_height.min(7680. / ratio);
                let even = |size: f64| ((size / 2.).round() * 2.) as u32;
                [even((height * ratio).min(7680.)), even(height)]
            }
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Prefs {
    pub out_dir: String,
    pub resolution: Option<[u32; 2]>,
    #[serde(flatten, skip_serializing)]
    legacy_resolution: LegacyResolution,
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
            resolution: None,
            legacy_resolution: LegacyResolution::default(),
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
    pub fn dimensions(&self) -> [u32; 2] {
        let [width, height] = self.resolution.unwrap_or_else(|| self.legacy_resolution.dimensions());
        if VideoOptions::validate_dimensions(width, height).is_ok() {
            [width, height]
        } else {
            DEFAULT_RESOLUTION
        }
    }

    pub fn resolution_text(&self) -> String {
        let [width, height] = self.dimensions();
        format!("{width} × {height}")
    }

    pub fn resolution_choice(&self) -> usize {
        RESOLUTION_PRESETS.iter().position(|&size| size == self.dimensions()).map_or(usize::MAX, |index| index + 1)
    }

    pub fn set_custom_resolution(&mut self, input: &str) -> Result<()> {
        let (width, height) = input.split_once(['x', 'X', '×']).context("请按宽×高输入像素尺寸")?;
        let parse = |value: &str| value.trim().parse::<u32>().context("宽高须为整数");
        let [width, height] = [parse(width)?, parse(height)?];
        VideoOptions::validate_dimensions(width, height)?;
        self.resolution = Some([width, height]);
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
    prefs.resolution = Some(prefs.dimensions());
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
