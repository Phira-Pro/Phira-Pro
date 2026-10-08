//! Named judgement settings, separate from account, audio and visual preferences.
use anyhow::{bail, Result};
use prpr::config::{Config, JudgeAlgorithm, JudgeGrading, JudgeTiming, Mods, PhigrosRules};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JudgeSettings {
    pub algorithm: JudgeAlgorithm,
    pub windows_ms: [f32; 4],
    pub timing: JudgeTiming,
    pub grading: JudgeGrading,
    pub late_ms: f32,
    pub drag_protect: bool,
    pub flick_protect: bool,
    pub hold_tail: bool,
    pub strict: bool,
    pub fullscreen: bool,
    pub no_combo_score: bool,
    pub phigros: PhigrosRules,
}

impl Default for JudgeSettings {
    fn default() -> Self {
        Self::capture(&Config::default())
    }
}

impl JudgeSettings {
    pub fn capture(c: &Config) -> Self {
        Self {
            algorithm: c.judge_algorithm,
            timing: c.judge_timing.clone(),
            grading: c.judge_grading,
            windows_ms: [c.lim_perfect_plus_ms, c.lim_perfect_ms, c.lim_good_ms, c.lim_bad_ms],
            late_ms: c.late_leniency_ms,
            drag_protect: c.drag_protect,
            flick_protect: c.flick_protect,
            hold_tail: c.hold_tail_judge,
            strict: c.mods.contains(Mods::STRICT_JUDGE),
            fullscreen: c.mods.contains(Mods::FULLSCREEN_JUDGE),
            no_combo_score: c.mods.contains(Mods::NO_COMBO_SCORE),
            phigros: c.phigros_rules.clone(),
        }
    }
    pub fn phigros() -> Self {
        Self {
            algorithm: JudgeAlgorithm::Phigros,
            windows_ms: [16., 80., 180., 220.],
            drag_protect: true,
            flick_protect: true,
            ..Self::default()
        }
    }
    pub fn detailed() -> Self {
        Self {
            windows_ms: [16., 40., 100., 220.],
            grading: JudgeGrading {
                detailed: true,
                ..Default::default()
            },
            ..Self::default()
        }
    }
    /// osu!mania nominal half-windows, extrapolated to the requested -15..15 range.
    /// The extra Perfect+ splits PERFECT; BAD uses mania's MISS hit-window boundary.
    /// This shortcut changes windows only, not score weights, matching or hold mechanics.
    pub fn apply_osu_mania_od(&mut self, od: i8) {
        let od = od.clamp(-15, 15) as f32;
        self.grading.detailed = true;
        self.windows_ms = [8., 16., 97. - 3. * od, 188. - 3. * od];
        self.timing = JudgeTiming::default();
        self.grading.early_ms = [64. - 3. * od, 127. - 3. * od, 151. - 3. * od];
        self.grading.late_ms = self.grading.early_ms;
        self.normalize();
    }
    pub fn selected_osu_mania_od(&self) -> Option<i8> {
        (-15..=15).find(|od| {
            let mut candidate = self.clone();
            candidate.apply_osu_mania_od(*od);
            self.grading.detailed
                && self.windows_ms == candidate.windows_ms
                && self.timing == candidate.timing
                && self.grading.early_ms == candidate.grading.early_ms
                && self.grading.late_ms == candidate.grading.late_ms
        })
    }
    pub fn apply(&self, c: &mut Config) {
        c.judge_grading = self.grading;
        c.judge_algorithm = self.algorithm;
        c.judge_timing = self.timing.clone();
        [c.lim_perfect_plus_ms, c.lim_perfect_ms, c.lim_good_ms, c.lim_bad_ms] = self.windows_ms;
        c.late_leniency_ms = self.late_ms;
        c.drag_protect = self.drag_protect;
        c.flick_protect = self.flick_protect;
        c.hold_tail_judge = self.hold_tail;
        c.phigros_rules = self.phigros.clone();
        c.phigros_rules.sanitize();
        c.mods.set(Mods::STRICT_JUDGE, self.strict);
        c.mods.set(Mods::FULLSCREEN_JUDGE, self.fullscreen);
        c.mods.set(Mods::NO_COMBO_SCORE, self.no_combo_score);
        c.clamp_judge_windows();
        c.late_leniency_ms = (c.late_leniency() * 1000.) as f32;
    }
    pub fn normalize(&mut self) {
        let mut c = Config::default();
        self.apply(&mut c);
        *self = Self::capture(&c);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgePreset {
    pub id: String,
    pub name: String,
    pub settings: JudgeSettings,
}

pub fn valid_name(name: &str, existing: &[JudgePreset], editing: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        bail!("preset-name-invalid");
    }
    if existing
        .iter()
        .any(|p| Some(p.id.as_str()) != editing && p.name.trim().to_lowercase() == name.to_lowercase())
    {
        bail!("preset-name-duplicate");
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn osu_mania_od_formula_covers_all_requested_values_and_keeps_seven_windows_ordered() {
        for od in -15..=15 {
            let mut s = JudgeSettings::detailed();
            s.apply_osu_mania_od(od);
            let g = 64. - 3. * od as f32;
            let o = 127. - 3. * od as f32;
            let m = 151. - 3. * od as f32;
            assert_eq!(s.grading.early_ms, [g, o, m]);
            assert_eq!(s.windows_ms, [8., 16., 97. - 3. * od as f32, 188. - 3. * od as f32]);
            assert_eq!(s.grading.early_ms, s.grading.late_ms);
            assert_eq!(s.selected_osu_mania_od(), Some(od));
            let cfg = {
                let mut c = Config::default();
                s.apply(&mut c);
                c
            };
            let w = cfg.judge_windows();
            assert!(
                w.perfect_plus <= w.perfect
                    && w.perfect <= w.extended[0]
                    && w.extended[0] <= w.good
                    && w.good <= w.extended[1]
                    && w.extended[1] <= w.extended[2]
                    && w.extended[2] <= w.bad
            );
            let restored: JudgeSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
            assert_eq!(restored.selected_osu_mania_od(), Some(od));
        }
        let mut s = JudgeSettings::detailed();
        s.apply_osu_mania_od(5);
        assert_eq!(s.grading.early_ms, [49., 112., 136.]);
        assert_eq!(s.windows_ms, [8., 16., 82., 173.]);
        s.grading.late_ms[0] = 50.;
        assert_eq!(s.selected_osu_mania_od(), None);
    }
    #[test]
    fn detailed_preset_normalization_and_grading_roundtrip() {
        let mut settings = JudgeSettings::detailed();
        settings.grading.perfect_plus = false;
        settings.grading.early_ms = [65., 125., 155.];
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: JudgeSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(settings, restored);
        let mut cfg = Config::default();
        cfg.theoretical_score = true;
        settings.apply(&mut cfg);
        assert!(!cfg.theoretical_score && cfg.judge_grading.detailed);
        assert_eq!(JudgeSettings::capture(&cfg), settings);
        assert_eq!(cfg.judge_windows().extended_early, [0.065, 0.125, 0.155]);
        assert!(cfg.non_official_items(Mods::empty()).contains(&"judge_grading"));
        settings.grading.early_ms = [f32::NAN, -5., f32::INFINITY];
        settings.normalize();
        assert!(settings.grading.early_ms.iter().all(|v| v.is_finite()));
        assert!(settings.grading.early_ms[0] >= 40. && settings.grading.early_ms[1] >= 100. && settings.grading.early_ms[2] <= 220.);
    }
    #[test]
    fn legacy_data_keeps_judgement_and_user_configuration() {
        let data: crate::data::Data =
            serde_json::from_str(r#"{"config":{"limPerfectMs":75,"limGoodMs":155,"dragProtect":true,"offset":0.123},"language":"zh-CN","theme":3}"#)
                .unwrap();
        assert!(data.judge_presets.is_empty());
        assert!(data.judge_preset_id.is_none());
        assert_eq!(data.config.judge_algorithm, JudgeAlgorithm::PhiraPro);
        assert_eq!(data.config.lim_perfect_ms, 75.);
        assert_eq!(data.config.lim_good_ms, 155.);
        assert!(data.config.drag_protect);
        assert_eq!(data.config.offset, 0.123);
        let again: crate::data::Data = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(again.theme, 3);
        assert_eq!(again.language.as_deref(), Some("zh-CN"));
        assert_eq!(JudgeSettings::capture(&again.config), JudgeSettings::capture(&data.config));
    }
    #[test]
    fn preset_roundtrip_and_scope() {
        let p = JudgePreset {
            id: "stable-id".into(),
            name: "我的 120Hz".into(),
            settings: JudgeSettings::phigros(),
        };
        let text = serde_json::to_string(&p).unwrap();
        let p: JudgePreset = serde_json::from_str(&text).unwrap();
        let mut cfg = Config::default();
        cfg.volume_music = 0.31;
        cfg.offset = 0.123;
        cfg.speed = 1.25;
        cfg.mods = Mods::FLIP_X | Mods::AUTOPLAY;
        p.settings.apply(&mut cfg);
        assert_eq!(JudgeSettings::capture(&cfg), p.settings);
        assert_eq!(cfg.volume_music, 0.31);
        assert_eq!(cfg.offset, 0.123);
        assert_eq!(cfg.speed, 1.25);
        assert!(cfg.mods.contains(Mods::FLIP_X | Mods::AUTOPLAY));
        assert!(!cfg.is_official_play(Mods::empty()));
        let mut mods = Mods::empty();
        cfg.force_official_play(&mut mods);
        assert_eq!(cfg.judge_algorithm, JudgeAlgorithm::PhiraPro);
        assert!(cfg.is_official_play(mods));
    }
    #[test]
    fn named_sided_presets_roundtrip_without_changing_other_preferences() {
        for algorithm in [JudgeAlgorithm::Phigros, JudgeAlgorithm::PhiraPro] {
            let mut settings = JudgeSettings::phigros();
            settings.algorithm = algorithm;
            settings.timing.early_ms = Some([16., 60., 100., 200.]);
            settings.timing.late_ms = Some([20., 90., 180., 220.]);
            settings.phigros.drag_sides_ms = Some([50., 150.]);
            settings.phigros.strict_early_ms = Some([30., 80., 130.]);
            settings.phigros.strict_late_ms = Some([50., 100., 150.]);
            settings.normalize();
            let preset = JudgePreset {
                id: "stable".into(),
                name: "提前16 延后20".into(),
                settings,
            };
            let restored: JudgePreset = serde_json::from_str(&serde_json::to_string(&preset).unwrap()).unwrap();
            assert_eq!(restored.settings, preset.settings);
            let mut cfg = Config::default();
            cfg.offset = 0.123;
            cfg.volume_music = 0.31;
            restored.settings.apply(&mut cfg);
            assert_eq!(JudgeSettings::capture(&cfg), restored.settings);
            assert_eq!(cfg.judge_windows().early[0], 0.016);
            assert_eq!(cfg.judge_windows().late[0], 0.020);
            assert_eq!(cfg.offset, 0.123);
            assert_eq!(cfg.volume_music, 0.31);
        }
        let legacy: JudgePreset = serde_json::from_str(r#"{"id":"old","name":"pro.11保存项","settings":{"windowsMs":[12,75,155,220]}}"#).unwrap();
        let mut cfg = Config::default();
        legacy.settings.apply(&mut cfg);
        assert_eq!(cfg.judge_windows().early, [0.012, 0.075, 0.155, 0.220]);
        assert_eq!(cfg.judge_windows().early, cfg.judge_windows().late);
    }

    #[test]
    fn names_and_invalid_values() {
        let list = vec![JudgePreset {
            id: "a".into(),
            name: "Test".into(),
            settings: JudgeSettings::default(),
        }];
        assert!(valid_name(" test ", &list, None).is_err());
        assert_eq!(valid_name(" Test ", &list, Some("a")).unwrap(), "Test");
        assert!(valid_name(" ", &list, None).is_err());
        assert!(valid_name("a\nb", &list, None).is_err());
        let mut settings = JudgeSettings::phigros();
        settings.windows_ms = [f32::NAN, 5., 3., 2.];
        settings.phigros.flick_ratio = f64::INFINITY;
        settings.phigros.hold_safe_frames = -99;
        settings.normalize();
        assert!(settings.windows_ms.iter().all(|n| n.is_finite()));
        assert!(settings.windows_ms.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(settings.phigros.flick_ratio, 1.75);
        assert_eq!(settings.phigros.hold_safe_frames, 0);
    }
}
