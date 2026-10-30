use crate::dir;
use prpr::chart_video::PresentationOptions;
use std::path::PathBuf;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Prefs {
    pub out_dir: String,
    pub res: usize,
    pub fps: usize,
    pub hitsound: bool,
    pub hud: bool,
    pub presentation: PresentationOptions,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            out_dir: String::new(),
            res: 1,
            fps: 1,
            hitsound: true,
            hud: false,
            presentation: PresentationOptions::default(),
        }
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
    prefs.res = prefs.res.min(super::RESOLUTIONS.len() - 1);
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
    }
}
