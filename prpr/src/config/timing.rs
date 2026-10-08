//! Optional sided windows preserve the symmetric configuration of older builds.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JudgeTiming {
    pub early_ms: Option<[f32; 4]>,
    pub late_ms: Option<[f32; 4]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Mods};
    #[test]
    fn sided_values_preserve_legacy_and_reset_for_official_play() {
        let mut cfg: Config = serde_json::from_str(r#"{"limPerfectPlusMs":16,"limGoodMs":155}"#).unwrap();
        assert_eq!(cfg.judge_windows().early, cfg.judge_windows().late);
        let base = [16., 80., 155., 220.];
        cfg.judge_timing.set(base, 0, true, 20.);
        cfg.clamp_judge_windows();
        assert_eq!(cfg.lim_perfect_plus_ms, 16., "early customization must not rewrite the old symmetric base");
        assert_eq!(cfg.judge_windows().for_offset(-0.018).perfect_plus, 0.016);
        assert_eq!(cfg.judge_windows().for_offset(0.018).perfect_plus, 0.020);
        let restored: Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(restored.judge_windows(), cfg.judge_windows());
        assert!(cfg.non_official_items(Mods::empty()).contains(&"judge_timing"));
        cfg.force_official_play(&mut Mods::empty());
        assert_eq!(cfg.judge_timing, JudgeTiming::default());
        assert!(cfg.is_official_play(Mods::empty()));
    }
    #[test]
    fn each_side_is_finite_ordered_and_changes_independently() {
        let base = [16., 80., 160., 220.];
        let mut timing = JudgeTiming {
            early_ms: Some([f32::NAN, -5., 999., 2.]),
            late_ms: None,
        };
        timing.normalize(base);
        let sides = timing.sides(base);
        assert_eq!(sides[1], base);
        assert!(sides[0].iter().all(|v| v.is_finite()));
        assert!(sides[0].windows(2).all(|v| v[0] <= v[1]));
        timing.set(base, 1, true, 10.);
        assert_eq!(timing.sides(base)[1], [10., 10., 160., 220.]);
        assert_eq!(timing.sides(base)[0], sides[0]);
    }
}

impl JudgeTiming {
    pub fn sides(&self, base: [f32; 4]) -> [[f32; 4]; 2] {
        [self.early_ms.unwrap_or(base), self.late_ms.unwrap_or(base)].map(|mut values| {
            for i in 0..4 {
                values[i] = if values[i].is_finite() { values[i] } else { base[i] };
                values[i] = values[i].clamp(1., [80., 120., 250., 400.][i]);
                if i > 0 {
                    values[i] = values[i].max(values[i - 1]);
                }
            }
            values
        })
    }
    pub fn normalize(&mut self, base: [f32; 4]) {
        let [early, late] = self.sides(base);
        self.early_ms = (early != base).then_some(early);
        self.late_ms = (late != base).then_some(late);
    }
    pub fn set(&mut self, base: [f32; 4], index: usize, late: bool, value: f32) {
        let index = index.min(3);
        let mut side = self.sides(base)[usize::from(late)];
        side[index] = value;
        for i in (index + 1)..4 {
            side[i] = side[i].max(side[i - 1]);
        }
        for i in (0..index).rev() {
            side[i] = side[i].min(side[i + 1]);
        }
        if late {
            self.late_ms = Some(side);
        } else {
            self.early_ms = Some(side);
        }
        self.normalize(base);
    }
}
