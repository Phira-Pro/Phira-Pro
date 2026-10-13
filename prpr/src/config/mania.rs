//! One source for the OD shortcut and result identification. These are windows
//! and the selected ACC weights. Matching and Hold mechanics stay Phira's.
use super::{Config, JudgeAlgorithm, JudgeTiming, Mods};

impl Config {
    pub fn apply_osu_mania_od(&mut self, od: i8) {
        let od = od.clamp(-15, 15) as f32;
        self.judge_grading.detailed = true;
        self.judge_grading.perfect_plus = false;
        self.judge_grading.mania = true;
        // Preserve osu!'s double-precision DifficultyRange evaluation order.
        // The expanded f32 formula rounds OD9 below 15ms before floor().
        let amount = (od as f64 - 5.) / 5.;
        let perfect = 19.4_f64 + if od > 5. { (13.9 - 19.4) * amount } else { (19.4 - 22.4) * amount };
        self.lim_perfect_plus_ms = 8.;
        // osu!lazer ManiaHitWindows: floor(nominal half-window) + 0.5ms.
        self.lim_perfect_ms = (perfect.floor() + 0.5) as f32;
        self.lim_good_ms = 97.5 - 3. * od;
        self.lim_bad_ms = 188.5 - 3. * od;
        self.judge_timing = JudgeTiming::default();
        self.judge_grading.early_ms = [64.5 - 3. * od, 127.5 - 3. * od, 151.5 - 3. * od];
        self.judge_grading.late_ms = self.judge_grading.early_ms;
        self.clamp_judge_windows();
    }

    /// Match the effective windows, ignoring the disabled Perfect+ threshold.
    /// Phigros STRICT replaces tap windows with its own rules, so those must not
    /// be presented as an OD even when the underlying shortcut is still stored.
    pub fn selected_osu_mania_od(&self) -> Option<i8> {
        if !self.judge_grading.detailed
            || !self.judge_grading.mania
            || self.judge_grading.perfect_plus
            || (self.judge_algorithm == JudgeAlgorithm::Phigros && self.mods.contains(Mods::STRICT_JUDGE))
        {
            return None;
        }
        let actual = self.judge_windows();
        (-15..=15).find(|od| {
            let mut candidate = self.clone();
            candidate.apply_osu_mania_od(*od);
            let expected = candidate.judge_windows();
            actual.early[1..] == expected.early[1..]
                && actual.late[1..] == expected.late[1..]
                && actual.extended_early == expected.extended_early
                && actual.extended_late == expected.extended_late
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_preserve_official_double_precision_boundaries() {
        let perfect = [
            31.5, 30.5, 30.5, 29.5, 29.5, 28.5, 27.5, 27.5, 26.5, 26.5, 25.5, 24.5, 24.5, 23.5, 23.5, 22.5, 21.5, 21.5, 20.5, 20.5, 19.5, 18.5, 17.5,
            16.5, 15.5, 13.5, 12.5, 11.5, 10.5, 9.5, 8.5,
        ];
        for (i, expected) in perfect.into_iter().enumerate() {
            let mut c = Config::default();
            c.apply_osu_mania_od(i as i8 - 15);
            assert_eq!(c.lim_perfect_ms, expected, "OD{}", i as i8 - 15);
        }
        let mut c = Config::default();
        c.apply_osu_mania_od(9);
        assert_eq!(c.lim_perfect_ms, 15.5);
        assert_eq!(c.judge_grading.early_ms, [37.5, 100.5, 124.5]);
        assert_eq!(c.lim_good_ms, 70.5);
        assert_eq!(c.lim_bad_ms, 161.5);
    }

    #[test]
    fn od_identity_follows_actual_windows_and_strict_override() {
        for od in -15..=15 {
            let mut c = Config::default();
            c.apply_osu_mania_od(od);
            assert_eq!(c.selected_osu_mania_od(), Some(od));
            c.mods.insert(Mods::STRICT_JUDGE);
            assert_eq!(c.selected_osu_mania_od(), Some(od));
            c.judge_algorithm = JudgeAlgorithm::Phigros;
            assert_eq!(c.selected_osu_mania_od(), None);
        }
        let mut c = Config::default();
        c.apply_osu_mania_od(15);
        c.judge_timing.set([8., 8.4, 52., 143.], 1, true, 9.);
        assert_eq!(c.selected_osu_mania_od(), None);
    }
}
