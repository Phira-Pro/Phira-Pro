use super::*;

pub(super) fn visible_frame(res: &Resource, ui: &mut Ui) {
    let rect = res.config.practice.view.visible_frame(res.aspect_ratio);
    // Constant screen-space strokes remain legible when the chart is tiny.
    ui.stroke_path(&rect.rounded(0.), 0.007, Color::new(0., 0., 0., 0.8));
    ui.stroke_path(&rect.rounded(0.), 0.003, Color::new(1., 0.88, 0.3, 0.95));
}

/// A diagram of timing and horizontal gates at each note's judgement line.
/// It samples the actual scroll animation, not a constant-speed approximation.
pub(super) fn draw(res: &Resource, chart: &Chart, ui: &mut Ui) {
    let options = &res.config.practice;
    let limits = res.windows;
    let mut rules = res.config.phigros_rules.clone();
    rules.sanitize();
    let phigros = res.config.judge_algorithm == crate::config::JudgeAlgorithm::Phigros;
    let world = crate::ext::get_viewport().2 as f64 / screen_height() as f64 * 5.;
    let view = options.view;
    let camera = Matrix::new_translation(&Vector::new(-view.x, -view.y)).append_scaling(view.scale);
    for line in &chart.lines {
        let tr = camera * debug_screen_transform(res.config.flip_x()) * line.now_transform(res, &chart.lines);
        let line_height = line.height.now() as f64;
        let incline = line.incline.now_opt().unwrap_or_default().to_radians().sin();
        let mut ctrl = line.ctrl_obj.borrow().clone();
        for note in &line.notes {
            let kind = match note.kind {
                NoteKind::Click => 0,
                NoteKind::Hold { .. } => 1,
                NoteKind::Drag => 2,
                NoteKind::Flick => 3,
            };
            if !options.range_kinds[kind]
                || note.fake
                || matches!(note.judge, crate::judge::JudgeStatus::Judged)
                || (note.time - res.time).abs() > options.range_horizon as f64 * res.config.speed as f64
            {
                continue;
            }
            let (note_tr, speed) = note.debug_transform(res, &mut ctrl, line_height, incline);
            let x = note_tr.transform_point(&Point::origin()).x;
            let side = Matrix::identity().append_nonuniform_scaling(&Vector::new(1., if note.above { 1. } else { -1. }));
            let mat = tr * side * Matrix::new_translation(&Vector::new(x, 0.));
            let half = if limits.fullscreen {
                4. / view.scale
            } else if phigros {
                (if kind >= 2 {
                    rules.special_width / world
                } else {
                    rules.tap_width / world
                }) as f32
                    * note.judge_area
            } else {
                crate::judge::X_DIFF_MAX as f32 * note.judge_area
            };
            let mut bands = Vec::with_capacity(7);
            if kind >= 2 {
                let gates = if phigros && kind == 2 {
                    rules.drag_sides().map(|v| v / 1000. * res.config.practice_multiplier)
                } else if phigros {
                    [limits.early[1] * rules.flick_sides()[0], limits.late[1] * rules.flick_sides()[1]]
                } else {
                    [limits.early[3], limits.late[3]]
                };
                bands.push((0, gates[0], gates[1]));
            } else {
                // Tap has an early BAD region; Hold heads accept only the
                // scoring grades. Phigros's late Tap gate is acceptance().
                if kind == 0 && !limits.grading.mania {
                    bands.push((2, limits.early[3], if phigros { limits.late_acceptance() } else { limits.late[3] }));
                }
                if limits.grading.detailed {
                    bands.push((7, limits.extended_early[2], limits.extended_late[2]));
                    bands.push((6, limits.extended_early[1], limits.extended_late[1]));
                }
                bands.push((1, limits.early[2], limits.late[2]));
                if limits.grading.detailed {
                    bands.push((5, limits.extended_early[0], limits.extended_late[0]));
                }
                bands.push((0, limits.early[1], limits.late[1]));
                if limits.grading.perfect_plus {
                    bands.push((4, limits.early[0], limits.late[0]));
                }
            }
            let fallback = [
                res.res_pack.info.fx_perfect(),
                res.res_pack.info.fx_good(),
                GRAY,
                RED,
                YELLOW,
                GREEN,
                ORANGE,
                VIOLET,
            ];
            ui.with(mat, |ui| {
                for (grade, early, late) in bands {
                    let span =
                        debug_height_spans_sided(&line.height, res.time, [early; 4], [late; 4], res.config.speed as f64, res.config.late_leniency())
                            [0];
                    let a = (span.0 * speed / res.aspect_ratio as f64) as f32;
                    let b = (span.1 * speed / res.aspect_ratio as f64) as f32;
                    if !a.is_finite() || !b.is_finite() || !half.is_finite() {
                        continue;
                    }
                    let tint = limits.grading.color(grade, fallback[grade]);
                    let r = Rect::new(-half, a.min(b), half * 2., (b - a).abs().max(0.002));
                    ui.fill_rect(
                        r,
                        Color {
                            a: options.range_alpha,
                            ..tint
                        },
                    );
                    ui.stroke_path(
                        &r.rounded(0.),
                        0.0015,
                        Color {
                            a: (options.range_alpha + 0.3).min(1.),
                            ..tint
                        },
                    );
                }
            });
        }
    }
}
