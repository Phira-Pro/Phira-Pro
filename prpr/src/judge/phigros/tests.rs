use super::*;

#[test]
fn asymmetric_grades_and_hold_heads_use_the_correct_side() {
    let mut cfg = config();
    cfg.judge_timing.early_ms = Some([16., 60., 100., 200.]);
    cfg.judge_timing.late_ms = Some([20., 90., 180., 220.]);
    cfg.phigros_rules.late_overrun = false;
    for (offset, expected) in [(-0.018, Judgement::Perfect), (0.018, Judgement::PerfectPlus), (-0.075, Judgement::Good), (0.075, Judgement::Perfect), (-0.150, Judgement::Bad), (0.150, Judgement::Good)] {
        let mut engine = Engine::new(vec![spec(0, 0., Kind::Tap)]);
        assert_eq!(scores(&step(&mut engine, &cfg, offset, &[finger(true, 0.)])), [(0, expected as u8)]);
        if !matches!(expected, Judgement::Bad) {
            let mut engine = Engine::new(vec![spec(0, 0., Kind::Hold)]);
            let events = step(&mut engine, &cfg, offset, &[finger(true, 0.)]);
            assert!(events.iter().any(|e| matches!(e.action, Action::Head { judgement, .. } if judgement as u8 == expected as u8)), "Hold offset {offset}");
        }
    }
    let mut engine = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    assert!(step(&mut engine, &cfg, 0.179, &[]).is_empty());
    assert_eq!(scores(&step(&mut engine, &cfg, 0.181, &[])), [(0, Judgement::Miss as u8)]);
}

#[test]
fn asymmetric_special_windows_and_strict_frame_pad() {
    let mut cfg = config();
    cfg.phigros_rules.late_overrun = false;
    cfg.phigros_rules.drag_sides_ms = Some([50., 150.]);
    cfg.phigros_rules.flick_ratio_sides = Some([1., 2.]);
    for (offset, armed) in [(-0.051, false), (-0.049, true), (0.149, true), (0.151, false)] {
        let mut engine = Engine::new(vec![spec(0, 0., Kind::Drag)]);
        step(&mut engine, &cfg, offset, &[finger(false, 0.)]);
        assert_eq!(engine.controls[0].special, armed, "Drag {offset}");
    }
    for (offset, armed) in [(-0.081, false), (-0.079, true), (0.159, true), (0.161, false)] {
        let mut engine = Engine::new(vec![spec(0, 0., Kind::Flick)]);
        let mut input = finger(false, 0.); input.keyboard = true;
        step(&mut engine, &cfg, offset, &[input]);
        assert_eq!(engine.controls[0].special, armed, "Flick {offset}");
    }
    cfg.mods.insert(Mods::STRICT_JUDGE);
    cfg.phigros_rules.strict_early_ms = Some([30., 80., 130.]);
    cfg.phigros_rules.strict_late_ms = Some([50., 100., 150.]);
    let (windows, _) = Engine::new(Vec::new()).step(&cfg, 0., 0.020, 200., &[], |_, _| None);
    for (actual, expected) in windows.early[1..].iter().zip([0.040, 0.090, 0.140]) { assert!((actual - expected).abs() < 1e-12); }
    for (actual, expected) in windows.late[1..].iter().zip([0.060, 0.110, 0.160]) { assert!((actual - expected).abs() < 1e-12); }
}

fn config() -> Config {
    let mut cfg = Config::default();
    cfg.judge_algorithm = JudgeAlgorithm::Phigros;
    cfg.lim_good_ms = 180.;
    cfg.drag_protect = true;
    cfg.flick_protect = true;
    cfg
}
fn spec(id: u32, time: f64, kind: Kind) -> Spec {
    Spec {
        target: (id as usize % 2, id),
        time,
        end: time + 1.,
        kind,
        skipped: false,
    }
}
fn finger(started: bool, x: f64) -> Finger {
    Finger {
        id: 17,
        started,
        world: [x, 0.],
        keyboard: false,
    }
}
fn scores(events: &[Event]) -> Vec<(u32, u8)> {
    events
        .iter()
        .filter_map(|e| match e.action {
            Action::Score { judgement, .. } => Some((e.target.1, judgement as u8)),
            _ => None,
        })
        .collect()
}
fn step(e: &mut Engine, cfg: &Config, now: f64, fingers: &[Finger]) -> Vec<Event> {
    e.step(cfg, now, 1. / 120., 200., fingers, |_, _| Some((0., 0.))).1
}

#[test]
fn tap_open_windows_and_early_bad() {
    for (offset, result) in [
        (-0.079, Judgement::Perfect),
        (-0.08, Judgement::Good),
        (-0.179, Judgement::Good),
        (-0.18, Judgement::Bad),
        (-0.219, Judgement::Bad),
        (-0.22, Judgement::Miss),
        (0.079, Judgement::Perfect),
        (0.08, Judgement::Good),
        (0.179, Judgement::Good),
    ] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
        let events = step(&mut e, &config(), offset, &[finger(true, 0.)]);
        if matches!(result, Judgement::Miss) {
            assert!(scores(&events).is_empty());
        } else {
            assert_eq!(scores(&events), [(0, result as u8)], "offset {offset}");
        }
    }
}

#[test]
fn sparse_late_bad_and_dense_range() {
    let mut cfg = config();
    for enabled in [true, false] {
        cfg.phigros_rules.late_overrun = enabled;
        let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
        assert_eq!(scores(&step(&mut e, &cfg, 0.181, &[finger(true, 0.)])), [(0, if enabled { Judgement::Bad } else { Judgement::Miss } as u8)]);
    }
    cfg.phigros_rules.late_overrun = true;
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap), spec(1, 0.2, Kind::Tap)]);
    let events = e
        .step(&cfg, 0.181, 1. / 60., 200., &[finger(true, 0.)], |target, _| if target.1 == 0 { Some((0., 0.)) } else { None })
        .1;
    assert_eq!(scores(&events), [(0, Judgement::Miss as u8)], "future note prevents the stale singleton fallback");
}

#[test]
fn multiple_protectors_and_same_frame_clicks_follow_the_article_sequence() {
    for kind in [Kind::Drag, Kind::Flick] {
        for (head, protected) in [(0.010, false), (0.015, true)] {
            let mut e = Engine::new(vec![spec(2, head, Kind::Tap), spec(1, 0.005, kind), spec(0, 0., Kind::Flick)]);
            step(&mut e, &config(), -0.1, &[finger(true, 0.)]);
            assert_eq!(e.controls[2].click, !protected);
            assert_eq!(e.controls[1].click, protected && kind == Kind::Drag);
        }
        let mut e = Engine::new(vec![spec(0, 0., kind), spec(1, 0.05, Kind::Tap)]);
        let mut second = finger(true, 0.); second.id = 18;
        step(&mut e, &config(), -0.15, &[finger(true, 0.), second]);
        assert_eq!(e.controls[1].click, kind == Kind::Drag);
        assert!(!e.controls[0].special, "two clicks outside Drag's held window cannot arm it");
    }
}

#[test]
fn chronological_protection_gap_and_independent_drag_marks() {
    for kind in [Kind::Drag, Kind::Flick] {
        let mut e = Engine::new(vec![spec(1, 0.05, Kind::Tap), spec(0, 0., kind)]);
        assert!(scores(&step(&mut e, &config(), -0.15, &[finger(true, 0.)])).is_empty());
        assert!(!e.controls[1].click, "protected tap is not matched");
        assert_eq!(e.controls[0].click, kind == Kind::Drag);
        assert!(!e.controls[0].special, "click protection must not arm drag/flick scoring");
        let events = step(&mut e, &config(), -0.149, &[finger(true, 0.)]);
        assert_eq!(e.controls[1].click, kind == Kind::Drag, "drag absorbs one click, flick can absorb repeated clicks");
        assert_eq!(scores(&events).is_empty(), kind == Kind::Flick);
    }
    for (gap, protected) in [(0.009, false), (0.01, true), (0.011, true)] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Flick), spec(1, gap, Kind::Tap)]);
        step(&mut e, &config(), -0.1, &[finger(true, 0.)]);
        assert_eq!(!e.controls[1].click, protected, "gap {gap}");
    }
}

#[test]
fn protection_switches_and_late_head() {
    for kind in [Kind::Drag, Kind::Flick] {
        let mut cfg = config();
        cfg.drag_protect = false;
        cfg.flick_protect = false;
        let mut e = Engine::new(vec![spec(0, 0., kind), spec(1, 0.05, Kind::Tap)]);
        let events = step(&mut e, &cfg, -0.15, &[finger(true, 0.)]);
        assert_eq!(scores(&events), [(1, Judgement::Bad as u8)]);
        let mut e = Engine::new(vec![spec(0, 0., kind), spec(1, 0.05, Kind::Tap)]);
        assert!(scores(&step(&mut e, &config(), 0.06, &[finger(true, 0.)])).iter().any(|(id, _)| *id == 1));
    }
}

#[test]
fn weighted_metric_and_ten_ms_tie() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap), spec(1, 0., Kind::Tap)]);
    let events = e
        .step(&config(), 0., 1. / 120., 200., &[finger(true, 0.)], |target, _| Some(if target.1 == 0 { (0.1, 2.2) } else { (0.5, 0.) }))
        .1;
    assert_eq!(scores(&events), [(1, Judgement::PerfectPlus as u8)]);
    for (now, selected) in [(-0.1, 0), (0.1, 1)] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Tap), spec(1, 0.01, Kind::Tap)]);
        let events = e
            .step(&config(), now, 1. / 120., 200., &[finger(true, 0.)], |target, _| Some(if target.1 == 0 { (0.5, 0.) } else { (0., 0.) }))
            .1;
        assert_eq!(scores(&events)[0].0, selected);
    }
}

#[test]
fn widths_and_bad_shrink() {
    for (dx, hit) in [(1.899, true), (1.9, false), (1.901, false)] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
        let events = e.step(&config(), 0., 1. / 120., 200., &[finger(true, 0.)], |_, _| Some((dx, 0.))).1;
        assert_eq!(!scores(&events).is_empty(), hit);
    }
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    assert!(scores(&e.step(&config(), -0.19, 1. / 60., 200., &[finger(true, 0.)], |_, _| Some((1.899, 0.))).1).is_empty());
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    assert_eq!(scores(&step(&mut e, &config(), -0.19, &[finger(true, 0.)])), [(0, Judgement::Bad as u8)]);
}

#[test]
fn drag_closed_window_persistent_arm_and_early_score() {
    for (dt, dx, arm) in [(0.1, 2.099, true), (0.1, 2.1, false), (0.100001, 0., false)] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Drag)]);
        e.step(&config(), -dt, 1. / 120., 200., &[finger(false, 0.)], |_, _| Some((dx, 0.)));
        assert_eq!(e.controls[0].special, arm);
        assert!(scores(&step(&mut e, &config(), -0.005, &[])).is_empty());
        let events = step(&mut e, &config(), -0.0049, &[]);
        assert_eq!(!scores(&events).is_empty(), arm);
        if arm {
            assert_eq!(scores(&events), [(0, Judgement::Perfect as u8)]);
        }
    }
    let mut e = Engine::new(vec![spec(0, 0., Kind::Drag)]);
    assert_eq!(scores(&step(&mut e, &config(), 0.100001, &[])), [(0, Judgement::Miss as u8)]);
}

#[test]
fn early_hold_waits_for_good_and_never_bad() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    let events = step(&mut e, &config(), -0.21, &[finger(true, 0.)]);
    assert!(events.is_empty());
    assert!(e.controls[0].click);
    assert!(e.controls[0].head.is_none());
    let events = step(&mut e, &config(), -0.179, &[finger(false, 0.)]);
    assert!(matches!(
        events[0].action,
        Action::Head {
            judgement: Judgement::Good,
            ..
        }
    ));
    assert_eq!(scores(&step(&mut e, &config(), 0.781, &[])), [(0, Judgement::Good as u8)]);
}

#[test]
fn hold_release_is_three_frames_not_fifty_ms() {
    for fps in [60., 120., 240.] {
        let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
        step(&mut e, &config(), 0., &[finger(true, 0.)]);
        for frame in 1..=3 {
            assert!(scores(&step(&mut e, &config(), frame as f64 / fps, &[])).is_empty());
        }
        assert_eq!(scores(&step(&mut e, &config(), 4. / fps, &[])), [(0, Judgement::Miss as u8)]);
    }
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    step(&mut e, &config(), 0., &[finger(true, 0.)]);
    for frame in 1..=3 {
        step(&mut e, &config(), frame as f64 / 120., &[]);
    }
    step(&mut e, &config(), 4. / 120., &[finger(false, 0.)]);
    assert_eq!(e.controls[0].safe, 2, "recontact resets protection");
}

#[test]
fn hold_tail_boundary_and_delayed_miss() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    step(&mut e, &config(), 0., &[finger(true, 0.)]);
    assert!(scores(&step(&mut e, &config(), 0.78, &[finger(false, 0.)])).is_empty());
    assert_eq!(scores(&step(&mut e, &config(), 0.780001, &[])), [(0, Judgement::PerfectPlus as u8)]);
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    assert!(step(&mut e, &config(), 0.181, &[finger(true, 0.)]).is_empty());
    assert!(e.controls[0].click && e.controls[0].head.is_none());
    assert!(step(&mut e, &config(), 1.25, &[]).is_empty());
    assert_eq!(scores(&step(&mut e, &config(), 1.250001, &[])), [(0, Judgement::Miss as u8)]);
}

#[test]
fn strict_rolling_ten_frames_and_independent_drag() {
    let mut cfg = config();
    cfg.mods.insert(Mods::STRICT_JUDGE);
    for fps in [60., 120.] {
        let mut e = Engine::new(vec![]);
        for _ in 0..12 {
            let (w, _) = e.step(&cfg, 0., 1. / fps, 200., &[], |_, _| None);
            assert!((w.perfect - (0.04 + 0.5 / fps)).abs() < 1e-12);
            assert!((w.good - (0.09 + 0.5 / fps)).abs() < 1e-12);
            assert!((w.bad - (0.14 + 0.5 / fps)).abs() < 1e-12);
        }
        for _ in 0..10 {
            e.step(&cfg, 0., 1. / 240., 200., &[], |_, _| None);
        }
        assert!((e.windows(&cfg, 1. / 240.).perfect - (0.04 + 0.5 / 240.)).abs() < 1e-12);
        let mut e = Engine::new(vec![spec(0, 0., Kind::Drag)]);
        e.step(&cfg, -0.1, 1. / fps, 200., &[finger(false, 0.)], |_, _| Some((0., 0.)));
        assert!(e.controls[0].special);
    }
}

#[test]
fn flick_direction_stop_and_low_dpi_high_fps() {
    let rules = PhigrosRules::default();
    let mut t = Flick {
        stopped: true,
        ..Default::default()
    };
    t.push([0.4, 0.], 1. / 120., 200., &rules);
    assert!(t.new_flick);
    t.new_flick = false;
    t.push([0.8, 0.], 1. / 120., 200., &rules);
    assert!(!t.new_flick);
    t.push([0.4, 0.], 1. / 120., 200., &rules);
    assert!(t.new_flick);
    t.new_flick = false;
    t.push([0.4, 0.], 1. / 120., 200., &rules);
    assert!(t.stopped);
    t.push([0.8, 0.], 1. / 120., 200., &rules);
    assert!(t.new_flick);
    let mut t = Flick {
        stopped: true,
        ..Default::default()
    };
    for i in 1..5 {
        t.new_flick = false;
        t.push([i as f64 * 0.09, 0.], 1. / 120., 200., &rules);
        assert!(t.new_flick);
    }
    let mut t = Flick {
        stopped: true,
        ..Default::default()
    };
    t.push([0.09, 0.], 1. / 60., 200., &rules);
    assert!(!t.new_flick);
}

#[test]
fn one_flick_one_note_and_endpoint_only() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Flick), spec(1, 0., Kind::Flick)]);
    step(&mut e, &config(), -0.1, &[finger(true, 0.)]);
    step(&mut e, &config(), -0.09, &[finger(false, 0.4)]);
    assert!(e.controls[0].special);
    assert!(!e.controls[1].special);
    step(&mut e, &config(), -0.08, &[finger(false, 0.8)]);
    assert!(!e.controls[1].special, "same straight gesture is already consumed");
    step(&mut e, &config(), -0.07, &[finger(false, 0.4)]);
    assert!(e.controls[1].special, "reversal is a new gesture");
    let mut e = Engine::new(vec![spec(0, 0., Kind::Flick)]);
    step(&mut e, &config(), -0.1, &[finger(true, 0.)]);
    e.step(&config(), -0.09, 1. / 120., 200., &[finger(false, 3.)], |_, _| Some((3., 0.)));
    assert!(!e.controls[0].special, "crossing the strip without ending inside does not match");
}

#[test]
fn input_order_skips_marks_and_practice_reset() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap), spec(1, 0., Kind::Tap)]);
    let fingers = [finger(true, 0.), Finger { id: 42, ..finger(true, 0.) }];
    let events = e
        .step(&config(), 0., 1. / 120., 200., &fingers, |target, f| {
            if f == 0 {
                Some((if target.1 == 0 { 0. } else { 0.5 }, 0.))
            } else if target.1 == 0 {
                Some((0., 0.))
            } else {
                None
            }
        })
        .1;
    assert_eq!(scores(&events), [(0, Judgement::PerfectPlus as u8)]);
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    e.advance_to(1.);
    assert!(step(&mut e, &config(), 1., &[finger(true, 0.)]).is_empty());
}

#[test]
fn separate_tail_extension_scores_second_part_and_missing_head_twice() {
    let mut cfg = config();
    cfg.hold_tail_judge = true;
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    assert!(matches!(step(&mut e, &cfg, 0., &[finger(true, 0.)])[0].action, Action::Head { .. }));
    assert!(scores(&step(&mut e, &cfg, 0.9, &[finger(false, 0.)])).is_empty());
    assert_eq!(scores(&step(&mut e, &cfg, 1., &[finger(false, 0.)])), [(0, Judgement::PerfectPlus as u8)]);
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    assert_eq!(scores(&step(&mut e, &cfg, 0.181, &[])), [(0, Judgement::Miss as u8), (0, Judgement::Miss as u8)]);
}

#[test]
fn device_dpi_override_and_speed_use_physical_units() {
    for (override_dpi, expected) in [(0., false), (200., true)] {
        let mut cfg = config();
        cfg.phigros_rules.device_dpi = override_dpi;
        let mut e = Engine::new(vec![spec(0, 0., Kind::Flick)]);
        e.step(&cfg, -0.1, 1. / 120., 500., &[finger(true, 0.)], |_, _| Some((0., 0.)));
        e.step(&cfg, -0.09, 1. / 120., 500., &[finger(false, 0.09)], |_, _| Some((0., 0.)));
        assert_eq!(e.controls[0].special, expected);
    }
    for speed in [0.5, 1., 2.] {
        let mut cfg = config();
        cfg.speed = speed as f32;
        let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
        assert_eq!(scores(&step(&mut e, &cfg, -0.079 * speed, &[finger(true, 0.)])), [(0, Judgement::Perfect as u8)]);
        let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
        step(&mut e, &cfg, 0., &[finger(true, 0.)]);
        assert!(scores(&step(&mut e, &cfg, 1. - 0.221 * speed, &[finger(false, 0.)])).is_empty());
        assert_eq!(scores(&step(&mut e, &cfg, 1. - 0.219 * speed, &[finger(false, 0.)])), [(0, Judgement::PerfectPlus as u8)]);
    }
}

#[test]
fn successful_hold_scores_once_and_ends_visually_at_its_tail() {
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    step(&mut e, &config(), 0., &[finger(true, 0.)]);
    let events = step(&mut e, &config(), 0.781, &[finger(false, 0.)]);
    assert_eq!(scores(&events), [(0, Judgement::PerfectPlus as u8)]);
    assert!(!events.iter().any(|e| matches!(e.action, Action::EndHold)));
    assert!(e.controls[0].hold_scored);
    assert!(step(&mut e, &config(), 0.99, &[]).is_empty());
    let events = step(&mut e, &config(), 1., &[]);
    assert!(scores(&events).is_empty());
    assert!(matches!(events[0].action, Action::EndHold));
    assert!(e.active.is_empty());
    assert!(step(&mut e, &config(), 1.01, &[]).is_empty());
}

#[test]
fn ten_ms_tie_survives_chart_timestamp_subtraction() {
    for base in [0., 1., 123.45] {
        for (gap, expected) in [(0.009, 1), (0.01, 1), (0.011, 0)] {
            let mut e = Engine::new(vec![spec(0, base, Kind::Tap), spec(1, base + gap, Kind::Tap)]);
            let events = e.step(&config(), base + 0.1, 1. / 120., 200., &[finger(true, 0.)],
                |target, _| Some((if target.1 == 0 { 0.5 } else { 0. }, 0.))).1;
            assert_eq!(scores(&events)[0].0, expected, "base {base}, gap {gap}");
        }
        for (gap, protected) in [(0.009, false), (0.01, true), (0.011, true)] {
            let mut e = Engine::new(vec![spec(0, base, Kind::Flick), spec(1, base + gap, Kind::Tap)]);
            step(&mut e, &config(), base - 0.1, &[finger(true, 0.)]);
            assert_eq!(!e.controls[1].click, protected, "base {base}, gap {gap}");
        }
    }
}

#[test]
fn custom_late_leniency_applies_to_all_head_note_types() {
    let mut cfg = config(); cfg.late_leniency_ms = 50.;
    let mut e = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    assert_eq!(scores(&step(&mut e, &cfg, 0.22, &[finger(true, 0.)])), [(0, Judgement::Good as u8)]);
    let mut e = Engine::new(vec![spec(0, 0., Kind::Hold)]);
    assert!(matches!(step(&mut e, &cfg, 0.22, &[finger(true, 0.)])[0].action,
        Action::Head { judgement: Judgement::Good, .. }));
    let mut e = Engine::new(vec![spec(0, 0., Kind::Drag)]);
    assert_eq!(scores(&step(&mut e, &cfg, 0.14, &[finger(false, 0.)])), [(0, Judgement::Perfect as u8)]);
    // A future non-Flick keeps the normal range nonempty, so this checks real
    // late-window expansion instead of passing accidentally via sparse fallback.
    let mut e = Engine::new(vec![spec(0, 0., Kind::Flick), spec(1, 0.18, Kind::Tap)]);
    step(&mut e, &cfg, 0.15, &[finger(true, 0.)]);
    assert_eq!(scores(&step(&mut e, &cfg, 0.16, &[finger(false, 0.4)])), [(0, Judgement::Perfect as u8)]);
}

#[test]
fn extended_taps_and_hold_heads_keep_their_grade_and_perfect_plus_toggle() {
    let mut cfg = config();
    cfg.lim_perfect_ms = 40.; cfg.lim_good_ms = 100.;
    cfg.judge_grading.detailed = true;
    for (off, grade) in [(0.01, Judgement::PerfectPlus), (0.03, Judgement::Perfect), (0.05, Judgement::Great), (0.09, Judgement::Good), (0.12, Judgement::Ok), (0.15, Judgement::Meh)] {
        for sign in [-1., 1.] {
            let mut engine = Engine::new(vec![spec(0, 0., Kind::Tap)]);
            assert_eq!(scores(&step(&mut engine, &cfg, off * sign, &[finger(true, 0.)])), [(0, grade as u8)]);
            let mut engine = Engine::new(vec![spec(0, 0., Kind::Hold)]);
            let events = step(&mut engine, &cfg, off * sign, &[finger(true, 0.)]);
            assert!(events.iter().any(|e| matches!(e.action, Action::Head { judgement, .. } if judgement as u8 == grade as u8)));
            assert_eq!(scores(&step(&mut engine, &cfg, 0.81, &[finger(false, 0.)])), [(0, grade as u8)]);
        }
    }
    cfg.judge_grading.perfect_plus = false;
    let mut engine = Engine::new(vec![spec(0, 0., Kind::Tap)]);
    assert_eq!(scores(&step(&mut engine, &cfg, 0., &[finger(true, 0.)])), [(0, Judgement::Perfect as u8)]);
}
