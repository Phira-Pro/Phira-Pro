//! Compare the actual range renderer against an independently evaluated
//! constant-scroll fixture from PhiraiAd's pinned judgement_range.rs.
use super::*;
use prpr::judge::Judgement;
use prpr::{
    core::{AnimFloat, Keyframe, NoteKind, Object},
    practice::RangeAnchor,
};

pub(super) fn run(game: &mut GameScene, painter: &mut TextPainter) {
    game.res.config.practice = Default::default();
    game.res.config.practice.ranges = true;
    game.res.config.practice_active = true;
    game.res.config.practice_multiplier = 1.;
    game.res.config.speed = 1.;
    game.res.config.flow_speed = 1.;
    game.res.config.late_leniency_ms = 0.;
    game.res.config.judge_algorithm = prpr::config::JudgeAlgorithm::PhiraPro;
    game.res.config.judge_grading = Default::default();
    game.res.windows = game.res.config.judge_windows();
    game.res.time = 1.;
    let line = &mut game.chart.lines[0];
    line.object = Object::default();
    line.height = AnimFloat::new(vec![Keyframe::new(0., 0., 2), Keyframe::new(10., 5., 2)]);
    line.height.set_time(1.);
    for (i, note) in line.notes.iter_mut().enumerate() {
        note.object = Object::default();
        note.object.translation.0 = AnimFloat::fixed(-0.75 + i as f32 * 0.5);
        note.time = 2.;
        note.height = 1.;
        note.speed = 1.;
        note.fake = false;
        note.judge_area = 1.;
        note.multiple_hint = false;
        note.above = true;
        note.judge = JudgeStatus::NotJudged;
        note.kind = match i {
            0 => NoteKind::Click,
            1 => NoteKind::Hold {
                end_time: 2.,
                end_height: 1.,
            },
            2 => NoteKind::Drag,
            _ => NoteKind::Flick,
        };
    }
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        game.res.aspect_ratio = w as f32 / h as f32;
        for (anchor, label) in [(RangeAnchor::JudgeLine, "line"), (RangeAnchor::Note, "note"), (RangeAnchor::Both, "both")] {
            game.res.config.practice.range_anchor = anchor;
            let actual = capture(game, painter, w, h, false);
            let reference = capture(game, painter, w, h, true);
            actual.export_png(&format!("target/replay-qa/practice-ranges-{ratio}-{label}.png"));
            reference.export_png(&format!("target/replay-qa/phiraiad-reference-{ratio}-{label}.png"));
            let diff = actual.bytes.iter().zip(&reference.bytes).filter(|(a, b)| a.abs_diff(**b) > 2).count();
            assert!(diff < actual.bytes.len() / 1000, "AD reference mismatch {ratio}/{label}: {diff}");
            assert!(actual.bytes.chunks_exact(4).filter(|p| p[..3].iter().any(|v| *v > 20)).count() > 1000);
        }
    }
    // Tail coverage applies only after the head was caught.
    game.res.config.practice.range_anchor = RangeAnchor::JudgeLine;
    game.chart.lines[0].notes[1].judge = JudgeStatus::Hold(true, 1.8, 0., false, f64::INFINITY, Judgement::Perfect);
    for i in [0, 2, 3] {
        game.chart.lines[0].notes[i].judge = JudgeStatus::Judged;
    }
    game.res.time = 1.9;
    let tail = capture(game, painter, 1280, 548, false);
    assert!(tail.bytes.chunks_exact(4).filter(|p| p[0] > 100 && p[2] > 150 && p[1] < 150).count() > 100, "Hold tail must use purple outline");
    game.res.config.practice.range_hold[3] = false;
    let hidden = capture(game, painter, 1280, 548, false);
    assert!(hidden.bytes.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]), "tail toggle must hide the caught Hold range");
    tail.export_png("target/replay-qa/practice-ranges-hold-tail.png");
    println!("Ranges: AD source-reference pixels, disjoint alpha, line/note/both anchors, four types, three aspects and caught Hold tail passed");
}

fn capture(game: &GameScene, painter: &mut TextPainter, w: i32, h: i32, reference: bool) -> Image {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    clear_background(BLACK);
    if reference {
        // Source point_base_at and default palette evaluated for linear height
        // h(t)=t/2. This path never calls production range geometry or bands.
        let aspect = game.res.aspect_ratio;
        let alpha = game.res.config.practice.range_alpha;
        let anchor = game.res.config.practice.range_anchor;
        let colors = [
            Color::new(0.30, 0.88, 1., 1.),
            Color::new(0.42, 0.92, 0.43, 1.),
            Color::new(1., 0.62, 0.18, 1.),
            Color::new(1., 0.22, 0.28, 1.),
        ];
        for i in 0..4 {
            let half = game.res.note_width * (prpr::judge::X_DIFF_MAX / prpr::core::NOTE_WIDTH_RATIO_BASE) as f32;
            let x = -0.75 + i as f32 * 0.5;
            let bands: Vec<_> = match i {
                0 => vec![(0.08, colors[0]), (0.16, colors[1]), (0.22, colors[2])],
                1 => vec![(0.08, colors[0]), (0.16, colors[1])],
                2 => vec![(0.22, Color::new(0.98, 0.88, 0.25, 1.))],
                _ => vec![(0.22, Color::new(0.95, 0.28, 0.72, 1.))],
            };
            for on_note in [false, true] {
                if anchor == RangeAnchor::JudgeLine && on_note || anchor == RangeAnchor::Note && !on_note {
                    continue;
                }
                let y = |t: f32| if on_note { -0.5 / aspect - 0.5 * t / aspect } else { 0.5 * t / aspect };
                let rect = |a: f32, b: f32| Rect::new(x - half, y(a).min(y(b)), half * 2., (y(a) - y(b)).abs().max(0.0015));
                let mut inner = 0.;
                for &(outer, color) in &bands {
                    ui.fill_rect(rect(-outer, -inner), Color { a: alpha, ..color });
                    ui.fill_rect(rect(inner, outer), Color { a: alpha, ..color });
                    inner = outer;
                }
                for (n, &(outer, color)) in bands.iter().enumerate() {
                    let r = rect(-outer, outer);
                    ui.stroke_path(&prpr::ext::RectExt::rounded(&r, 0.), 0.004, if i < 2 && n + 1 == bands.len() { colors[3] } else { color });
                }
            }
        }
    } else {
        game.practice_draw_ranges_for_qa(&mut ui);
    }
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
}
