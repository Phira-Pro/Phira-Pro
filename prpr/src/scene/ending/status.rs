//! Completed-play snapshot: never read the user's subsequently edited preset.
use crate::config::{Config, JudgeAlgorithm, Mods, PhigrosRules};
use crate::core::ModIcon;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum BadgeItem {
    Text(String),
    Icon(ModIcon),
}

fn push_text(items: &mut Vec<BadgeItem>, text: impl AsRef<str>) {
    let text = text.as_ref();
    if let Some(BadgeItem::Text(previous)) = items.last_mut() {
        previous.push(' ');
        previous.push_str(text);
    } else {
        items.push(BadgeItem::Text(text.to_owned()));
    }
}

fn badge_items(c: &Config, rated: bool, upload_route_available: bool) -> Vec<BadgeItem> {
    let mut items = Vec::new();
    // Strict's approved label is a display exception only. The upload gate and
    // the full details continue to report the actual unrated result.
    let mut without_strict = c.clone();
    without_strict.mods.remove(Mods::STRICT_JUDGE);
    let strict_label_exception = upload_route_available && c.mods.contains(Mods::STRICT_JUDGE) && without_strict.is_official_play(Mods::empty());
    if !rated && !c.autoplay() && !strict_label_exception {
        push_text(&mut items, "UNRATED");
    }
    let mods = super::mod_badges::active(c.mods);
    let use_icons = mods.len() > 1;
    for badge in mods {
        match badge.icon.filter(|_| use_icons) {
            Some(icon) => items.push(BadgeItem::Icon(icon)),
            None => push_text(&mut items, badge.name),
        }
    }
    if let Some(od) = c.selected_osu_mania_od() {
        push_text(&mut items, format!("OD {od}"));
    }
    if (c.speed - 1.).abs() > 1e-4 {
        push_text(&mut items, format!("{:.2}x", c.speed));
    }
    items
}

pub(super) struct PlayStatus {
    pub items: Vec<BadgeItem>,
    pub details: String,
}

impl PlayStatus {
    pub fn new(c: &Config, rated: bool, upload_route_available: bool) -> Self {
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
            (Mods::NOTE_TAP, "ALL TAP"),
            (Mods::NOTE_DRAG, "ALL DRAG"),
            (Mods::NOTE_FLICK, "ALL FLICK"),
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
        let mut details = primary.join(" ");
        if od.is_some() {
            details.push_str("\nosu!mania ScoreV2 timing shortcut");
        }
        let summary = format!("\n\nPERFECT+: {}\nDrag protection: {}\nFlick protection: {}\nHold tail judgement: {}\nLate leniency: {:.1}ms\nSpeed: {:.2}x\nFlow: {:.2}x\nOffset: {:+.1}ms\n",
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
            details.push_str(&options.join("\n"));
        }
        details.push_str(&summary);
        if c.judge_algorithm == JudgeAlgorithm::Phigros {
            let r = rules;
            details.push_str(&format!("\nPHIGROS rules\nDrag: −{:.1} / +{:.1}ms\nFlick window multiplier: −{:.2} / +{:.2}\nTap width: {:.2}\nSpecial width: {:.2}\nBAD edge: {:.2}\nShrink: {:.2}\nMatching divisor: {:.2}\nProtection: {:.1}ms\nHold grace: {} frames\nHold tail tolerance: {:.1}ms\nDelayed Hold MISS: {:.1}ms\nSpecial early trigger: {:.1}ms\nFrame compensation: {}{}\nLate overrun: {}\nFlick speed: {:.3}\nReference DPI: {:.1}\nDevice DPI: {}\nFlick sampling: {:.1}Hz\nMultiplier: {:.2}\nFlick projection: {:.2}\n",
                r.drag_sides()[0], r.drag_sides()[1], r.flick_sides()[0], r.flick_sides()[1],
                r.tap_width, r.special_width, r.bad_edge, r.bad_shrink_factor, r.metric_divisor,
                r.protection_ms, r.hold_safe_frames, r.hold_tail_ms, r.hold_delayed_miss_ms, r.special_early_ms,
                on_off(r.frame_compensation && c.mods.contains(Mods::STRICT_JUDGE)),
                if r.frame_compensation && c.mods.contains(Mods::STRICT_JUDGE) { " (dynamic)" } else { "" },
                on_off(r.late_overrun), r.flick_speed, r.flick_dpi,
                if r.device_dpi == 0. { "AUTO".to_owned() } else { format!("{:.1}", r.device_dpi) },
                r.flick_sample_hz, r.flick_multiplier, r.flick_projection_min));
        }
        Self {
            items: badge_items(c, rated, upload_route_available),
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(status: &PlayStatus) -> String {
        status
            .items
            .iter()
            .filter_map(|item| match item {
                BadgeItem::Text(value) => Some(value.as_str()),
                BadgeItem::Icon(_) => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn approved_empty_speed_and_autoplay_labels() {
        let mut c = Config::default();
        assert!(PlayStatus::new(&c, true, true).items.is_empty());
        assert_eq!(text(&PlayStatus::new(&c, false, false)), "UNRATED");
        c.speed = 0.9;
        assert_eq!(text(&PlayStatus::new(&c, false, true)), "UNRATED 0.90x");
        c.speed = 1.1;
        assert_eq!(text(&PlayStatus::new(&c, true, true)), "1.10x");
        for speed in [1., 0.9, 1.1] {
            c.speed = speed;
            c.mods = Mods::AUTOPLAY;
            for route in [false, true] {
                let s = PlayStatus::new(&c, false, route);
                let expected = if speed == 1. {
                    "AUTOPLAY".into()
                } else {
                    format!("AUTOPLAY {speed:.2}x")
                };
                assert_eq!(text(&s), expected);
                assert!(s.details.starts_with("UNRATED"), "the details keep actual eligibility");
            }
        }
    }

    #[test]
    fn strict_display_exception_never_changes_upload_eligibility() {
        let mut c = Config::default();
        c.mods = Mods::STRICT_JUDGE;
        c.speed = 1.1;
        assert_eq!(text(&PlayStatus::new(&c, false, true)), "STRICT MODE 1.10x");
        assert!(!c.is_official_play(Mods::empty()));
        assert_eq!(text(&PlayStatus::new(&c, false, false)), "UNRATED STRICT MODE 1.10x");
        c.speed = 0.9;
        assert_eq!(text(&PlayStatus::new(&c, false, true)), "UNRATED STRICT MODE 0.90x");
        c.speed = 1.;
        c.offline_mode = true;
        assert_eq!(text(&PlayStatus::new(&c, false, true)), "UNRATED STRICT MODE");
    }

    #[test]
    fn all_31_approved_od_overrides_keep_spaces_and_no_preset_label() {
        for od in -15..=15 {
            let mut c = Config::default();
            c.apply_osu_mania_od(od);
            assert_eq!(text(&PlayStatus::new(&c, false, true)), format!("UNRATED OD {od}"));
            c.speed = 1.1;
            assert_eq!(text(&PlayStatus::new(&c, false, true)), format!("UNRATED OD {od} 1.10x"));
        }
        for algorithm in [JudgeAlgorithm::PhiraPro, JudgeAlgorithm::Phigros] {
            let mut c = Config::default();
            c.judge_algorithm = algorithm;
            c.judge_grading.detailed = true;
            assert_eq!(text(&PlayStatus::new(&c, false, true)), "UNRATED");
        }
    }

    #[test]
    fn multiple_mods_use_all_selected_icons_and_keep_speed_after_them() {
        let mut c = Config::default();
        c.mods = Mods::FLIP_X | Mods::PERFECT_SOUND | Mods::INSTANT_DEATH_FC | Mods::FULLSCREEN_JUDGE | Mods::NO_FAIL | Mods::NO_COMBO_SCORE;
        c.speed = 1.1;
        let s = PlayStatus::new(&c, false, true);
        assert_eq!(
            s.items,
            vec![
                BadgeItem::Text("UNRATED".into()),
                BadgeItem::Icon(ModIcon::Mirror),
                BadgeItem::Icon(ModIcon::SuddenDeath),
                BadgeItem::Icon(ModIcon::FullscreenJudge),
                BadgeItem::Icon(ModIcon::NoFail),
                BadgeItem::Icon(ModIcon::NoComboScore),
                BadgeItem::Icon(ModIcon::PerfectSound),
                BadgeItem::Text("1.10x".into()),
            ]
        );
        c.mods = Mods::INSTANT_DEATH_AP;
        assert_eq!(text(&PlayStatus::new(&c, true, true)), "AP CHALLENGE 1.10x");
        c.mods = Mods::INSTANT_DEATH_FC;
        assert_eq!(text(&PlayStatus::new(&c, true, true)), "FC CHALLENGE 1.10x");
        c.mods = Mods::FLIP_X | Mods::STRICT_JUDGE | Mods::NOTE_DRAG;
        let s = PlayStatus::new(&c, false, true);
        assert_eq!(
            s.items,
            vec![
                BadgeItem::Text("UNRATED".into()),
                BadgeItem::Icon(ModIcon::Mirror),
                BadgeItem::Text("ALL DRAG STRICT MODE 1.10x".into())
            ]
        );
        c.mods = Mods::FLIP_X | Mods::STRICT_JUDGE | Mods::FULLSCREEN_JUDGE | Mods::PERFECT_SOUND | Mods::NO_FAIL;
        let s = PlayStatus::new(&c, false, true);
        assert_eq!(s.items.last(), Some(&BadgeItem::Text("STRICT MODE 1.10x".into())));
        assert_eq!(
            s.items[1..s.items.len() - 1],
            [
                BadgeItem::Icon(ModIcon::Mirror),
                BadgeItem::Icon(ModIcon::FullscreenJudge),
                BadgeItem::Icon(ModIcon::NoFail),
                BadgeItem::Icon(ModIcon::PerfectSound),
            ]
        );
    }

    #[test]
    fn status_identifies_actual_od_and_sided_customization() {
        let mut c = Config::default();
        c.apply_osu_mania_od(15);
        let status = PlayStatus::new(&c, false, false);
        assert_eq!(text(&status), "UNRATED OD 15");
        assert!(status.details.contains("PERFECT: −8.5 / +8.5"));
        assert!(status.details.contains("PERFECT+ OFF"));
        c.judge_timing.set([8., 8.4, 52., 143.], 1, true, 9.);
        let status = PlayStatus::new(&c, false, false);
        assert!(!text(&status).contains("OD 15"));
        assert!(status.details.contains("CUSTOM WINDOWS"));
        assert!(status.details.contains("ASYMMETRIC WINDOWS"));
    }

    #[test]
    fn strict_reports_effective_windows_not_saved_shortcut() {
        let mut c = Config::default();
        c.apply_osu_mania_od(15);
        c.mods = Mods::STRICT_JUDGE | Mods::NO_FAIL | Mods::FULLSCREEN_JUDGE;
        let status = PlayStatus::new(&c, false, false);
        assert!(text(&status).contains("STRICT MODE OD 15"));
        assert!(status.details.contains("PERFECT: −4.2 / +4.2"));
        assert!(status.details.contains("NO FAIL"));
        assert!(status.details.contains("FULLSCREEN JUDGE"));
        c.judge_algorithm = JudgeAlgorithm::Phigros;
        let status = PlayStatus::new(&c, false, false);
        assert!(!text(&status).contains("OD 15"));
        assert!(status.details.contains("PERFECT: −40.0 / +40.0"));
        assert!(status.details.contains("Frame compensation: ON (dynamic)"));
    }

    #[test]
    fn all_mods_still_have_full_details_and_badges_never_show_info_markers() {
        let mut c = Config::default();
        c.mods = Mods::all();
        c.use_keyboard = true;
        c.hold_tail_judge = true;
        let s = PlayStatus::new(&c, false, false);
        assert!(!text(&s).contains("UNRATED"));
        assert!(!text(&s).contains("[i]") && !text(&s).contains('·'));
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
        assert!(PlayStatus::new(&Config::default(), true, true).items.is_empty());
    }
}
