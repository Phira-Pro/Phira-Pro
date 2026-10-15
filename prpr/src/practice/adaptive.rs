use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Goal {
    pub enabled: bool,
    pub accuracy: f32,
    pub consecutive: u32,
    pub full_combo: bool,
    pub lower_on_failure: bool,
}
impl Default for Goal {
    fn default() -> Self {
        Self {
            enabled: false,
            accuracy: 0.98,
            consecutive: 3,
            full_combo: true,
            lower_on_failure: false,
        }
    }
}
#[derive(Default)]
pub struct Progress {
    pub streak: u32,
    pub level: u32,
}
impl Progress {
    pub fn finish(&mut self, goal: &Goal, accuracy: f64, counts: [u32; 8], partial: bool) {
        if !goal.enabled || partial || counts.iter().sum::<u32>() == 0 {
            return;
        }
        if accuracy + 1e-6 >= goal.accuracy as f64 && (!goal.full_combo || (counts[2] + counts[3] == 0)) {
            self.streak += 1;
            if self.streak >= goal.consecutive.clamp(1, 99) {
                self.level = self.level.saturating_add(1);
                self.streak = 0;
            }
        } else {
            self.streak = 0;
            if goal.lower_on_failure {
                self.level = self.level.saturating_sub(1);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal_threshold_qualifies() {
        let g = Goal {
            enabled: true,
            consecutive: 1,
            ..Default::default()
        };
        let mut p = Progress::default();
        p.finish(&g, 0.98, [10, 0, 0, 0, 0, 0, 0, 0], false);
        assert_eq!(p.level, 1);
    }
    #[test]
    fn only_complete_consecutive_qualified_rounds_advance() {
        let mut g = Goal {
            enabled: true,
            consecutive: 2,
            ..Default::default()
        };
        let mut p = Progress::default();
        let ok = [10, 0, 0, 0, 0, 0, 0, 0];
        p.finish(&g, 1., ok, true);
        assert_eq!(p.streak, 0);
        p.finish(&g, 1., ok, false);
        p.finish(&g, 0.8, ok, false);
        assert_eq!(p.level, 0);
        p.finish(&g, 1., ok, false);
        p.finish(&g, 1., ok, false);
        assert_eq!(p.level, 1);
        g.lower_on_failure = true;
        p.finish(&g, 0.8, ok, false);
        assert_eq!(p.level, 0);
    }
}
