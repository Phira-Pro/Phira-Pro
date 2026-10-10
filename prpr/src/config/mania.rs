//! One source for the OD shortcut and result identification. These are windows
//! only: selecting them does not switch to osu!mania matching or scoring.
use super::{Config, JudgeAlgorithm, JudgeTiming, Mods};

impl Config {
    pub fn apply_osu_mania_od(&mut self, od: i8) {
        let od = od.clamp(-15, 15) as f32;
        self.judge_grading.detailed = true;
        self.judge_grading.perfect_plus = false;
        let perfect = if od <= 5. { 22.4 - 0.6 * od } else { 24.9 - 1.1 * od };
        self.lim_perfect_plus_ms = 8.;
        self.lim_perfect_ms = (perfect * 10.).round() / 10.;
        self.lim_good_ms = 97. - 3. * od;
        self.lim_bad_ms = 188. - 3. * od;
        self.judge_timing = JudgeTiming::default();
        self.judge_grading.early_ms = [64. - 3. * od, 127. - 3. * od, 151. - 3. * od];
        self.judge_grading.late_ms = self.judge_grading.early_ms;
        self.clamp_judge_windows();
    }

    /// Match the effective windows, ignoring the disabled Perfect+ threshold.
    /// Phigros STRICT replaces tap windows with its own rules, so those must not
    /// be presented as an OD even when the underlying shortcut is still stored.
    pub fn selected_osu_mania_od(&self) -> Option<i8> {
        if !self.judge_grading.detailed
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
