//! Judgement-range presentation adapted from Adai-add/PhiraiAd (GPL-3.0),
//! prpr/src/judgement_range.rs at a60b49ad30b4748327df9a5fae794278a3978520.
//! Its disjoint fills, anchors, palette and Hold tails use this engine's windows.
use super::*;
use crate::{
    core::{AnimFloat, CtrlObject, JudgeLine, Note, NOTE_WIDTH_RATIO_BASE},
    judge::JudgeStatus,
    practice::RangeAnchor,
};

const OUTLINE_WIDTH: f32 = 0.004;
const MIN_RECT_SIZE: f32 = 0.0015;
const PERFECT: Color = Color::new(0.30, 0.88, 1.00, 1.);
const GOOD: Color = Color::new(0.42, 0.92, 0.43, 1.);
const BAD: Color = Color::new(1.00, 0.62, 0.18, 1.);
const MISS: Color = Color::new(1.00, 0.22, 0.28, 1.);
const DRAG: Color = Color::new(0.98, 0.88, 0.25, 1.);
const FLICK: Color = Color::new(0.95, 0.28, 0.72, 1.);
const HOLD_TAIL: Color = Color::new(0.67, 0.39, 1.00, 1.);

pub(super) fn visible_frame(res: &Resource, ui: &mut Ui) {
    let rect = res.config.practice.view.visible_frame(res.aspect_ratio);
    ui.stroke_path(&rect.rounded(0.), 0.007, Color::new(0., 0., 0., 0.8));
    ui.stroke_path(&rect.rounded(0.), 0.003, Color::new(1., 0.88, 0.3, 0.95));
}

#[derive(Clone, Copy, Debug, Default)]
struct Window {
    early: f64,
    late: f64,
}
impl Window {
    fn new(early: f64, late: f64) -> Self {
        Self { early, late }
    }
    fn visible(self, time: f64, now: f64, horizon: f64, speed: f64) -> bool {
        time + self.late * speed >= now && time - self.early * speed <= now + horizon * speed
    }
    // Paint outer minus inner once, so nesting never darkens the inner fill.
    fn difference(self, inner: Self, time: f64, speed: f64) -> [(f64, f64); 2] {
        [
            (time - self.early * speed, time - inner.early * speed),
            (time + inner.late * speed, time + self.late * speed),
        ]
    }
}
fn sample(anim: &AnimFloat, time: f64, default: f32) -> f32 {
    let mut copy = anim.clone();
    copy.set_time(time);
    copy.now_opt().unwrap_or(default)
}
fn ctrl_height(note: &Note, line_height: f64, y_offset: f32) -> f64 {
    (note.height - line_height
        + if note.speed.abs() <= f64::EPSILON {
            0.
        } else {
            y_offset as f64 / note.speed
        })
        * crate::parse::RPE_HEIGHT as f64
        / 2.
}
fn point_base_at(note: &Note, line: &JudgeLine, ctrl: &CtrlObject, res: &Resource, point_height: f64, time: f64) -> f32 {
    let line_height = sample(&line.height, time, 0.) as f64;
    let y_offset = sample(&note.object.translation.1, time, 0.);
    let height = ctrl_height(note, line_height, y_offset);
    let speed = note.speed * sample(&ctrl.y, height, 1.) as f64 * res.config.flow_speed as f64;
    ((point_height - line_height) * speed / res.aspect_ratio as f64 + y_offset as f64 / res.aspect_ratio as f64) as f32
}
fn half_width(note: &Note, line: &JudgeLine, ctrl: &CtrlObject, res: &Resource, special: bool) -> f32 {
    if res.windows.fullscreen {
        return 4. / res.config.practice.view.scale;
    }
    if res.config.judge_algorithm == crate::config::JudgeAlgorithm::Phigros {
        let world = crate::ext::get_viewport().2 as f64 / screen_height() as f64 * 5.;
        return ((if special {
            res.config.phigros_rules.special_width
        } else {
            res.config.phigros_rules.tap_width
        }) / world) as f32
            * note.judge_area.abs();
    }
    let height = ctrl_height(note, line.height.now() as f64, note.object.translation.1.now());
    let multiple = if res.config.double_hint && note.multiple_hint {
        res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
    } else {
        1.
    };
    // AD scales this display with note size, sizeControl and multiple textures.
    (res.note_width
        * multiple
        * note.object.scale.now_with_def(1., 1.).x.abs()
        * sample(&ctrl.size, height, 1.).abs()
        * (crate::judge::X_DIFF_MAX / NOTE_WIDTH_RATIO_BASE) as f32
        * note.judge_area.abs())
    .max(MIN_RECT_SIZE)
}
fn current_x(note: &Note, line: &JudgeLine, ctrl: &CtrlObject, res: &Resource, base: f32, on_note: bool) -> f32 {
    let x = note.object.translation.0.now();
    if !on_note || matches!(note.kind, NoteKind::Hold { .. }) {
        return x;
    }
    let height = ctrl_height(note, line.height.now() as f64, note.object.translation.1.now());
    let incline = line.incline.now_opt().unwrap_or_default().to_radians().sin();
    x * (1. - incline * base * res.aspect_ratio * crate::parse::RPE_HEIGHT / 2. / 360.) * sample(&ctrl.pos, height, 1.)
}
struct DrawContext<'a> {
    note: &'a Note,
    line: &'a JudgeLine,
    ctrl: &'a CtrlObject,
    res: &'a Resource,
    point_height: f64,
    point_time: f64,
    half_width: f32,
    anchor: RangeAnchor,
}
impl DrawContext<'_> {
    fn rect(&self, start: f64, end: f64) -> Rect {
        let a = point_base_at(self.note, self.line, self.ctrl, self.res, self.point_height, start);
        let b = point_base_at(self.note, self.line, self.ctrl, self.res, self.point_height, end);
        let current = point_base_at(self.note, self.line, self.ctrl, self.res, self.point_height, self.res.time);
        let on_note = self.anchor == RangeAnchor::Note;
        let (a, b) = if on_note { (current - a, current - b) } else { (a, b) };
        let x = current_x(self.note, self.line, self.ctrl, self.res, current, on_note);
        Rect::new(x - self.half_width, a.min(b), self.half_width * 2., (b - a).abs().max(MIN_RECT_SIZE))
    }
    fn fill(&self, ui: &mut Ui, start: f64, end: f64, color: Color) {
        if end > start {
            let r = self.rect(start, end);
            if [r.x, r.y, r.w, r.h].into_iter().all(f32::is_finite) {
                ui.fill_rect(
                    r,
                    Color {
                        a: self.res.config.practice.range_alpha,
                        ..color
                    },
                );
            }
        }
    }
    fn outline(&self, ui: &mut Ui, window: Window, color: Color, speed: f64) {
        let r = self.rect(self.point_time - window.early * speed, self.point_time + window.late * speed);
        if [r.x, r.y, r.w, r.h].into_iter().all(f32::is_finite) {
            ui.stroke_path(&r.rounded(0.), OUTLINE_WIDTH, color);
        }
    }
}
#[derive(Clone, Copy)]
struct Band {
    window: Window,
    color: Color,
    enabled: bool,
}

fn head_bands(res: &Resource, kind: usize) -> (Vec<Band>, bool) {
    let w = res.windows;
    let p = &res.config.practice;
    let hold = kind == 1;
    let phigros = res.config.judge_algorithm == crate::config::JudgeAlgorithm::Phigros;
    let leniency = res.config.late_leniency();
    if kind >= 2 {
        let sides = if phigros && kind == 2 {
            res.config.phigros_rules.drag_sides().map(|v| v / 1000. * res.config.practice_multiplier)
        } else if phigros {
            let r = res.config.phigros_rules.flick_sides();
            [w.early[1] * r[0], w.late[1] * r[1]]
        } else {
            [w.early[3], w.late[3]]
        };
        return (
            vec![Band {
                window: Window::new(sides[0], sides[1] + leniency),
                color: if kind == 2 { DRAG } else { FLICK },
                enabled: true,
            }],
            false,
        );
    }
    let flags = if hold { p.range_hold } else { p.range_tap };
    let mut bands = Vec::with_capacity(7);
    let mut add = |grade: usize, early: f64, late: f64, fallback: Color, enabled: bool| {
        bands.push(Band {
            window: Window::new(early, late + leniency),
            color: w.grading.color(grade, fallback),
            enabled,
        });
    };
    // AD's Perfect fill remains one band by default. Preserve custom colors
    // and finer schemes when this app actually uses those scoring grades.
    if w.grading.perfect_plus && (w.grading.detailed || w.grading.colors[4].is_some()) {
        add(4, w.early[0], w.late[0], PERFECT, flags[0]);
    }
    add(0, w.early[1], w.late[1], PERFECT, flags[0]);
    if w.grading.detailed {
        add(5, w.extended_early[0], w.extended_late[0], GOOD, flags[1]);
    }
    add(1, w.early[2], w.late[2], GOOD, flags[1]);
    if w.grading.detailed {
        add(6, w.extended_early[1], w.extended_late[1], GOOD, flags[1]);
        add(7, w.extended_early[2], w.extended_late[2], GOOD, flags[1]);
    }
    if !hold && !w.grading.mania {
        add(2, w.early[3], if phigros { w.late_acceptance() } else { w.late[3] }, BAD, flags[2]);
    }
    (bands, if hold { flags[2] } else { flags[3] })
}

pub(super) fn draw(res: &Resource, chart: &Chart, ui: &mut Ui, indices: &[(Vec<u32>, usize)]) {
    let p = &res.config.practice;
    let speed = res.config.speed.max(f32::EPSILON) as f64;
    let horizon = p.range_horizon as f64;
    let camera = Matrix::new_translation(&Vector::new(-p.view.x, -p.view.y)).append_scaling(p.view.scale);
    let transform = camera * debug_screen_transform(res.config.flip_x());
    let profiles: [_; 4] = std::array::from_fn(|kind| head_bands(res, kind));
    for (line, (sorted, start)) in chart.lines.iter().zip(indices) {
        let line_tr = transform * line.now_transform(res, &chart.lines);
        let ctrl = line.ctrl_obj.borrow();
        for id in &sorted[*start..] {
            let note = &line.notes[*id as usize];
            let kind = match note.kind {
                NoteKind::Click => 0,
                NoteKind::Hold { .. } => 1,
                NoteKind::Drag => 2,
                NoteKind::Flick => 3,
            };
            let (bands, miss) = &profiles[kind];
            let outer = bands.last().unwrap().window;
            if note.time - outer.early * speed > res.time + horizon * speed {
                continue;
            }
            if !p.range_kinds[kind] || note.fake || matches!(note.judge, JudgeStatus::Judged | JudgeStatus::PreJudge) {
                continue;
            }
            let side = Matrix::identity().append_nonuniform_scaling(&Vector::new(1., if note.above { 1. } else { -1. }));
            let half = half_width(note, line, &ctrl, res, kind >= 2);
            for anchor in [RangeAnchor::JudgeLine, RangeAnchor::Note] {
                if p.range_anchor != RangeAnchor::Both && p.range_anchor != anchor {
                    continue;
                }
                let ctx = DrawContext {
                    note,
                    line,
                    ctrl: &ctrl,
                    res,
                    point_height: note.height,
                    point_time: note.time,
                    half_width: half,
                    anchor,
                };
                ui.with(line_tr * side, |ui| {
                    if !matches!(note.judge, JudgeStatus::Hold(..)) && outer.visible(note.time, res.time, horizon, speed) {
                        let mut inner = Window::default();
                        for band in bands {
                            if band.enabled {
                                for (start, end) in band.window.difference(inner, note.time, speed) {
                                    ctx.fill(ui, start, end, band.color);
                                }
                            }
                            inner = band.window;
                        }
                        for (i, band) in bands.iter().enumerate() {
                            if i + 1 == bands.len() && *miss {
                                ctx.outline(ui, band.window, MISS, speed);
                            } else if band.enabled {
                                ctx.outline(ui, band.window, band.color, speed);
                            }
                        }
                    }
                    if let NoteKind::Hold { end_time, end_height } = note.kind {
                        if p.range_hold[3] && matches!(note.judge, JudgeStatus::Hold(..)) {
                            let tail = Window::new(
                                if res.config.hold_tail_judge {
                                    res.windows.early[3]
                                } else if res.config.judge_algorithm == crate::config::JudgeAlgorithm::Phigros {
                                    res.config.phigros_rules.hold_tail_ms / 1000.
                                } else {
                                    res.windows.early[3]
                                },
                                0.,
                            );
                            if tail.visible(end_time, res.time, horizon, speed) {
                                let tail_ctx = DrawContext {
                                    point_height: end_height,
                                    point_time: end_time,
                                    ..ctx
                                };
                                tail_ctx.fill(ui, end_time - tail.early * speed, end_time, HOLD_TAIL);
                                tail_ctx.outline(ui, tail, HOLD_TAIL, speed);
                            }
                        }
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asymmetric_difference_bands_cover_outer_once_and_visibility_uses_real_seconds() {
        let perfect = Window::new(0.08, 0.10);
        let good = Window::new(0.16, 0.18);
        let bad = Window::new(0.22, 0.20);
        let mut segments = vec![];
        let mut inner = Window::default();
        for window in [perfect, good, bad] {
            segments.extend(window.difference(inner, 10., 0.5));
            inner = window;
        }
        for i in 1..1000 {
            let time = 10. - bad.early * 0.5 + (bad.early + bad.late) * 0.5 * i as f64 / 1000.;
            assert_eq!(segments.iter().filter(|(start, end)| *start <= time && time < *end).count(), 1);
        }
        assert!(bad.visible(11.09, 10., 2., 0.5));
        assert!(!bad.visible(11.2, 10., 2., 0.5));
    }
}
