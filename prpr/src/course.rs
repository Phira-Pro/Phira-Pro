//! Course rules are applied to a session copy and never overwrite saved presets.
use crate::config::{Config, JudgeAlgorithm, Mods};
pub fn config(source: &Config) -> Config {
    let mut c = source.clone();
    c.judge_algorithm = JudgeAlgorithm::Phigros;
    c.phigros_rules = Default::default();
    c.judge_timing = Default::default();
    c.judge_grading = Default::default();
    c.judge_grading.perfect_plus = false;
    c.theoretical_score = false;
    c.mods = Mods::STRICT_JUDGE | Mods::NO_COMBO_SCORE;
    c.clear_legacy_autoplay();
    c.speed = 1.;
    c.late_leniency_ms = 0.;
    c.hold_tail_judge = false;
    c.drag_protect = true;
    c.flick_protect = true;
    c.practice_active = false;
    c.practice_multiplier = 1.;
    c.practice_ramp = false;
    c.practice = Default::default();
    c.hp_mode = false;
    c.auto_retry = 0.;
    c.retry_lead = 0.;
    c.use_keyboard = false;
    c.interactive = true;
    c
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locks_rules_without_mutating_preferences() {
        let mut source = Config::default();
        source.speed = 1.7;
        source.phigros_rules.strict_good_ms = 200.;
        source.mods = Mods::AUTOPLAY | Mods::NIGHTCORE | Mods::FULLSCREEN_JUDGE;
        source.judge_grading.detailed = true;
        let c = config(&source);
        assert_eq!(source.speed, 1.7);
        assert_eq!(c.speed, 1.);
        assert!(c.mods.contains(Mods::STRICT_JUDGE | Mods::NO_COMBO_SCORE));
        assert!(!c.autoplay());
        assert!(!c.judge_grading.perfect_plus);
        assert!(!c.judge_grading.detailed);
        assert_eq!(c.judge_algorithm, JudgeAlgorithm::Phigros);
        assert_eq!(c.judge_windows().early, [0.016, 0.040, 0.090, 0.140]);
    }
}
