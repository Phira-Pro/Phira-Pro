//! Completed-play snapshot: never read the user's subsequently edited preset.
use crate::config::{Config, JudgeAlgorithm, Mods, PhigrosRules};

pub(super) struct PlayStatus {
    pub label: String,
    pub details: String,
}

impl PlayStatus {
    pub fn new(c: &Config, rated: bool) -> Self {
        let mut primary = Vec::new();
        if !rated {
            primary.push("UNRATED".to_owned());
        }
        let od = c.selected_osu_mania_od();
        if let Some(od) = od {
            primary.push(format!("OD{od}"));
        }
        if c.mods.contains(Mods::STRICT_JUDGE) {
            primary.push("STRICT MODE".to_owned());
        }
        primary.push(
            match c.judge_algorithm {
                JudgeAlgorithm::PhiraPro => "PHIRA PRO",
                JudgeAlgorithm::Phigros => "PHIGROS",
            }
            .to_owned(),
        );
        if od.is_none() {
            if c.judge_grading.detailed {
                primary.push("DETAILED".to_owned());
            }
            let mut default = Config::default();
            default.judge_algorithm = c.judge_algorithm;
            default.mods = c.mods;
            let actual = c.judge_windows();
            let base = default.judge_windows();
            if actual.early[1..] != base.early[1..]
                || actual.late[1..] != base.late[1..]
                || (c.judge_grading.perfect_plus && (actual.early[0] != base.early[0] || actual.late[0] != base.late[0]))
                || (c.judge_grading.detailed && (actual.extended_early != base.extended_early || actual.extended_late != base.extended_late))
            {
                primary.push("CUSTOM WINDOWS".to_owned());
            }
        }
        if (c.speed - 1.).abs() > 1e-4 {
            primary.push(format!("{:.2}x", c.speed));
        }

        let mut options = Vec::new();
        for (flag, name) in [
            (Mods::AUTOPLAY, "AUTOPLAY"),
            (Mods::FLIP_X, "MIRROR"),
            (Mods::FADE_OUT, "FADE OUT"),
            (Mods::FADE_IN, "FADE IN"),
            (Mods::NIGHTCORE, "NIGHTCORE"),
            (Mods::RAINBOW, "RAINBOW"),
            (Mods::NO_SHADER, "NO SHADER"),
            (Mods::INSTANT_DEATH_AP, "AP CHALLENGE"),
            (Mods::INSTANT_DEATH_FC, "FC CHALLENGE"),
            (Mods::FULLSCREEN_JUDGE, "FULLSCREEN JUDGE"),
            (Mods::NO_FAIL, "NO FAIL"),
            (Mods::NO_COMBO_SCORE, "NO COMBO SCORE"),
            (Mods::PERFECT_SOUND, "PERFECT SOUND"),
        ] {
            if c.mods.contains(flag) {
                options.push(name.to_owned());
            }
        }
        let on_off = |enabled| if enabled { "ON" } else { "OFF" };
        if c.use_keyboard {
            options.push("KEYBOARD".to_owned());
        }
        if c.offline_mode {
            options.push("OFFLINE".to_owned());
        }
        if !c.judge_grading.perfect_plus {
            options.push("PERFECT+ OFF".to_owned());
        }
        if c.theoretical_score && c.judge_grading.perfect_plus {
            options.push("THEORETICAL SCORE".to_owned());
        }
        if c.drag_protect {
            options.push("DRAG PROTECTION".to_owned());
        }
        if c.flick_protect {
            options.push("FLICK PROTECTION".to_owned());
        }
        if c.hold_tail_judge {
            options.push("HOLD TAIL".to_owned());
        }
        if c.late_leniency() > 0. {
            options.push(format!("LATE +{:.1}ms", c.late_leniency() * 1000.));
        }
        if (c.flow_speed - 1.).abs() > 1e-3 {
            options.push(format!("FLOW {:.2}x", c.flow_speed));
        }
        if c.hp_mode {
            options.push(format!("HP ×{:.2} / {:.2}", c.hp_amount, c.hp_scale));
        }
        let w = c.judge_windows();
        if w.early != w.late || (w.grading.detailed && w.extended_early != w.extended_late) {
            options.push("ASYMMETRIC WINDOWS".to_owned());
        }
        let mut rules = c.phigros_rules.clone();
        rules.sanitize();
        if c.judge_algorithm == JudgeAlgorithm::Phigros && rules != PhigrosRules::default() {
            options.push("CUSTOM PHIGROS RULES".to_owned());
        }
        let mut details = primary.join(" · ");
        if od.is_some() {
            details.push_str("\nosu!mania ScoreV2 timing shortcut");
        }
        let summary = format!("\n\nPERFECT+: {}\nDrag protection: {}\nFlick protection: {}\nHold tail judgement: {}\nLate leniency: {:.1}ms\nSpeed: {:.2}x · Flow: {:.2}x\nOffset: {:+.1}ms\n",
            on_off(c.judge_grading.perfect_plus), on_off(c.drag_protect), on_off(c.flick_protect),
            on_off(c.hold_tail_judge), c.late_leniency()*1000., c.speed, c.flow_speed, c.offset*1000.);
        // JudgeWindows already includes strict overrides and normalization.
        // Late leniency is separate and intentionally not added twice here.
        if c.judge_algorithm == JudgeAlgorithm::Phigros && c.mods.contains(Mods::STRICT_JUDGE) && rules.frame_compensation {
            details.push_str("\nBase windows below; frame compensation adds half the recent frame duration.\n");
        }
        details.push_str("\nTiming windows (−early / +late, ms)\n");
        for (name, early, late, enabled) in [
            ("PERFECT+", w.early[0], w.late[0], w.grading.perfect_plus),
            ("PERFECT", w.early[1], w.late[1], true),
            ("GREAT", w.extended_early[0], w.extended_late[0], w.grading.detailed),
            ("GOOD", w.early[2], w.late[2], true),
            ("OK", w.extended_early[1], w.extended_late[1], w.grading.detailed),
            ("MEH", w.extended_early[2], w.extended_late[2], w.grading.detailed),
            ("BAD", w.early[3], w.late[3], true),
        ] {
            if enabled {
                details.push_str(&format!("{name}: −{:.1} / +{:.1}\n", early * 1000., late * 1000.));
            }
        }
        if !options.is_empty() {
            details.push_str("\nOther active settings\n");
            details.push_str(&options.join(" · "));
        }
        details.push_str(&summary);
        if c.judge_algorithm == JudgeAlgorithm::Phigros {
            let r = rules;
            details.push_str(&format!("\nPHIGROS rules\nDrag: −{:.1} / +{:.1}ms\nFlick window multiplier: −{:.2} / +{:.2}\nTap width: {:.2} · Special width: {:.2}\nBAD edge: {:.2} · Shrink: {:.2}\nMatching divisor: {:.2}\nProtection: {:.1}ms\nHold grace: {} frames\nHold tail tolerance: {:.1}ms\nDelayed Hold MISS: {:.1}ms\nSpecial early trigger: {:.1}ms\nFrame compensation: {}{}\nLate overrun: {}\nFlick speed: {:.3} · Reference DPI: {:.1}\nDevice DPI: {}\nFlick sampling: {:.1}Hz · Multiplier: {:.2}\nFlick projection: {:.2}\n",
                r.drag_sides()[0], r.drag_sides()[1], r.flick_sides()[0], r.flick_sides()[1],
                r.tap_width, r.special_width, r.bad_edge, r.bad_shrink_factor, r.metric_divisor,
                r.protection_ms, r.hold_safe_frames, r.hold_tail_ms, r.hold_delayed_miss_ms, r.special_early_ms,
                on_off(r.frame_compensation && c.mods.contains(Mods::STRICT_JUDGE)),
                if r.frame_compensation && c.mods.contains(Mods::STRICT_JUDGE) { " (dynamic)" } else { "" },
                on_off(r.late_overrun), r.flick_speed, r.flick_dpi,
                if r.device_dpi == 0. { "AUTO".to_owned() } else { format!("{:.1}", r.device_dpi) },
                r.flick_sample_hz, r.flick_multiplier, r.flick_projection_min));
        }
        if !options.is_empty() {
            primary.push(format!("+{}", options.len()));
        }
        Self {
            label: format!("{}  [i]", primary.join(" · ")),
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_identifies_actual_od_and_sided_customization() {
        let mut c = Config::default();
        c.apply_osu_mania_od(15);
        let status = PlayStatus::new(&c, false);
        assert!(status.label.starts_with("UNRATED · OD15 · PHIRA PRO"));
        assert!(status.details.contains("PERFECT: −8.4 / +8.4"));
        assert!(status.details.contains("PERFECT+ OFF"));
        c.judge_timing.set([8., 8.4, 52., 143.], 1, true, 9.);
        let status = PlayStatus::new(&c, false);
        assert!(!status.label.contains("OD15"));
        assert!(status.label.contains("CUSTOM WINDOWS"));
        assert!(status.details.contains("ASYMMETRIC WINDOWS"));
    }

    #[test]
    fn strict_reports_effective_windows_not_saved_shortcut() {
        let mut c = Config::default();
        c.apply_osu_mania_od(15);
        c.mods = Mods::STRICT_JUDGE | Mods::NO_FAIL | Mods::FULLSCREEN_JUDGE;
        let status = PlayStatus::new(&c, false);
        assert!(status.label.contains("OD15 · STRICT MODE"));
        assert!(status.details.contains("PERFECT: −4.2 / +4.2"));
        assert!(status.details.contains("NO FAIL"));
        assert!(status.details.contains("FULLSCREEN JUDGE"));
        c.judge_algorithm = JudgeAlgorithm::Phigros;
        let status = PlayStatus::new(&c, false);
        assert!(!status.label.contains("OD15"));
        assert!(status.details.contains("PERFECT: −40.0 / +40.0"));
        assert!(status.details.contains("Frame compensation: ON (dynamic)"));
    }

    #[test]
    fn all_mods_are_named_and_automatic_play_remains_explicitly_unrated() {
        let mut c = Config::default();
        c.mods = Mods::all();
        c.use_keyboard = true;
        c.hold_tail_judge = true;
        let s = PlayStatus::new(&c, false);
        assert!(s.label.starts_with("UNRATED"));
        for label in [
            "AUTOPLAY",
            "KEYBOARD",
            "MIRROR",
            "FADE OUT",
            "FADE IN",
            "NIGHTCORE",
            "RAINBOW",
            "NO SHADER",
            "AP CHALLENGE",
            "FC CHALLENGE",
            "FULLSCREEN JUDGE",
            "NO FAIL",
            "NO COMBO SCORE",
            "PERFECT SOUND",
            "HOLD TAIL",
        ] {
            assert!(s.details.contains(label), "missing {label}");
        }
        assert!(!PlayStatus::new(&Config::default(), true).label.contains("UNRATED"));
    }
}
