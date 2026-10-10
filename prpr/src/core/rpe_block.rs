//! Adapter for texture-line block markers. The PNGs are editor guides, not
//! runtime materials. Recorder speed codes select the block lifecycle; alpha
//! tracks remain a compatibility fallback for earlier charts.

use super::{Anim, AnimFloat, BlockPhase, BlockTransform, Vector};
use macroquad::prelude::Color;
use nalgebra::Rotation2;
use std::cell::RefCell;

pub(crate) const DEFAULT_COLOR: [f32; 3] = super::block_shader::DEFAULT_BLOCK_COLOR;

pub(crate) fn marker_kind(path: &str) -> Option<bool> {
    let name = path.rsplit(['/', '\\']).next()?;
    if name.eq_ignore_ascii_case("isSubtract0.png") {
        Some(false)
    } else if name.eq_ignore_ascii_case("isSubtract1.png") {
        Some(true)
    } else {
        None
    }
}

/// Only marker lines and their ancestors need animation evaluation before
/// touch filtering. Unrelated lines keep their normal judge/update schedule.
pub(crate) fn input_lines(parents: &[Option<usize>], markers: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut visited = vec![false; parents.len()];
    let mut result = Vec::new();
    for marker in markers {
        let mut current = Some(marker);
        while let Some(id) = current {
            let Some(seen) = visited.get_mut(id) else { break };
            if *seen {
                break;
            }
            *seen = true;
            result.push(id);
            current = parents[id];
        }
    }
    result
}

fn phase(alpha: f32) -> BlockPhase {
    if !alpha.is_finite() || alpha <= 0. {
        BlockPhase::Hidden
    } else if alpha <= 128. / 255. {
        // The supplied protocol example uses 121, 127 and 128 for preview,
        // and 255 for activation. These values are state codes, not fill alpha.
        BlockPhase::Disabled
    } else {
        BlockPhase::Active
    }
}

struct Span {
    start: f64,
    end: f64,
    phase: BlockPhase,
    timings: [f64; 4],
}

pub(crate) struct Marker {
    pub line: usize,
    pub invert: bool,
    pub anchor: Vector,
    pub color: RefCell<Anim<Color>>,
    spans: Vec<Span>,
}

impl Marker {
    pub fn new(line: usize, invert: bool, alpha: &AnimFloat) -> Self {
        let mut knots = vec![0.];
        let mut layer = Some(alpha);
        while let Some(anim) = layer {
            knots.extend(anim.keyframes.iter().map(|kf| kf.time).filter(|t| t.is_finite()));
            layer = anim.next.as_deref();
        }
        knots.sort_by(f64::total_cmp);
        knots.dedup();
        let mut anim = alpha.clone();
        let mut sample = |time| {
            anim.set_time(time);
            anim.now_opt().unwrap_or(1.)
        };
        let mut boundaries = knots.clone();
        // Find state transitions inside interpolated events once at load time.
        // Hold/jump events are also preserved at their exact keyframe times.
        for pair in knots.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let before_b = b - ((b - a) * 1e-8).min(1e-8);
            let (left, right) = (sample(a), sample(before_b));
            for threshold in [0., 128. / 255.] {
                if left.is_finite() && right.is_finite() && (left > threshold) != (right > threshold) {
                    let mut lo = a;
                    let mut hi = before_b;
                    for _ in 0..40 {
                        let mid = (lo + hi) * 0.5;
                        if (sample(mid) > threshold) == (left > threshold) {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    boundaries.push(hi);
                }
            }
        }
        boundaries.sort_by(f64::total_cmp);
        boundaries.dedup();
        let mut spans: Vec<Span> = Vec::new();
        for (i, &start) in boundaries.iter().enumerate() {
            let end = boundaries.get(i + 1).copied().unwrap_or(f64::INFINITY);
            let state = phase(sample(if end.is_finite() { (start + end) * 0.5 } else { start }));
            if let Some(last) = spans.last_mut().filter(|last| last.phase == state) {
                last.end = end;
            } else {
                spans.push(Span {
                    start,
                    end,
                    phase: state,
                    timings: [0.; 4],
                });
            }
        }
        // Cache lifecycle timings too: rendering and multi-touch input should
        // only need a binary search, regardless of the chart's event count.
        let mut first = 0;
        while first < spans.len() {
            if spans[first].phase == BlockPhase::Hidden {
                first += 1;
                continue;
            }
            let end = spans[first..]
                .iter()
                .position(|span| span.phase == BlockPhase::Hidden)
                .map_or(spans.len(), |i| first + i);
            let appear = spans[first].start;
            let disappear = spans.get(end).map_or(f64::INFINITY, |span| span.start);
            let mut enable = f64::INFINITY;
            for span in spans[first..end].iter_mut().rev() {
                if span.phase == BlockPhase::Active {
                    enable = span.start;
                }
                let disable = if span.phase == BlockPhase::Active { span.end } else { f64::INFINITY };
                span.timings = [appear, enable, disable, disappear];
            }
            let mut was_active = false;
            for span in &mut spans[first..end] {
                if was_active {
                    span.timings[0] = appear.min(span.start - 0.5);
                }
                was_active |= span.phase == BlockPhase::Active;
            }
            first = end;
        }
        Self {
            line,
            invert,
            anchor: Vector::new(0.5, 0.5),
            color: RefCell::default(),
            spans,
        }
    }

    pub fn color_at(&self, time: f64) -> [f32; 3] {
        let mut color = self.color.borrow_mut();
        color.set_time(time);
        color.now_opt().map_or(DEFAULT_COLOR, |c| [c.r, c.g, c.b].map(|v| v.clamp(0., 1.)))
    }

    /// Recorder reads only speed-event starts: 1=appear, 2=enable,
    /// 3=disable, 4=disappear. A backwards step starts a new lifecycle.
    /// Charts without these codes retain the earlier alpha adapter.
    pub fn set_lifecycle(&mut self, events: &[(f64, f32)]) {
        let mut events: Vec<_> = events
            .iter()
            .copied()
            .filter(|(time, code)| time.is_finite() && [1., 2., 3., 4.].contains(code))
            .collect();
        if events.is_empty() {
            return;
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut spans = Vec::new();
        let mut timings = [f64::INFINITY; 4];
        let mut previous = 0;
        let mut first = 0;
        let finish = |spans: &mut Vec<Span>, timings: &mut [f64; 4], end: f64, first: usize| {
            timings[3] = end;
            for span in &mut spans[first..] {
                span.end = span.end.min(end);
                span.timings = *timings;
            }
            *timings = [f64::INFINITY; 4];
        };
        for (time, code) in events {
            let code = code as usize;
            if code <= previous && timings[0].is_finite() {
                finish(&mut spans, &mut timings, time, first);
                first = spans.len();
            }
            if let Some(last) = spans.last_mut() {
                last.end = last.end.min(time);
            }
            if code == 4 {
                finish(&mut spans, &mut timings, time, first);
                first = spans.len();
            } else {
                timings[0] = timings[0].min(time);
                timings[code - 1] = time;
                spans.push(Span {
                    start: time,
                    end: f64::INFINITY,
                    phase: if code == 2 { BlockPhase::Active } else { BlockPhase::Disabled },
                    timings: [0.; 4],
                });
            }
            previous = code;
        }
        finish(&mut spans, &mut timings, f64::INFINITY, first);
        self.spans = spans;
    }

    /// RPE move events position the image's anchor, not its centre. Preserve
    /// signed scale for the anchor offset (mirrored images swap their edges),
    /// then rotate in the RPE canvas, before the viewport Y/aspect conversion.
    pub fn transform(&self, position: Vector, scale: Vector, pixels: Vector, rotation: f32, aspect: f32) -> BlockTransform {
        let signed_size = pixels.component_mul(&scale);
        let y_scale = (1350. / 900.) / aspect;
        let local_center = (Vector::new(0.5, 0.5) - self.anchor).component_mul(&signed_size);
        BlockTransform {
            y_scale,
            center: position + (Rotation2::new(rotation.to_radians()) * local_center).component_mul(&Vector::new(1., y_scale)),
            size: signed_size.map(f32::abs),
            rotation,
        }
    }

    /// Timings for the existing Disabled/Ready/Active materials and input model.
    pub fn timings(&self, time: f64) -> Option<[f64; 4]> {
        let index = self.spans.partition_point(|span| span.start <= time).checked_sub(1)?;
        let current = &self.spans[index];
        if current.phase == BlockPhase::Hidden || time >= current.end {
            return None;
        }
        Some(current.timings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BlockArea, Keyframe, Vector, Zone};

    #[test]
    fn rotated_line2area_tiles_meet_and_canvas_gaps_survive_every_aspect() {
        let marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        for aspect in [3. / 4., 4. / 3., 1.5, 16. / 9., 20. / 9.] {
            for rotation in [0_f32, 30., 45., 90., 120.] {
                let rot = Rotation2::new(rotation.to_radians());
                let size = Vector::new(300., 150.);
                let first = marker.transform(Vector::zeros(), size / 900. * (2. / 1350.), Vector::repeat(900.), rotation, aspect);
                for gap in [0., 25.] {
                    let delta = (rot * Vector::new(0., size.y + gap)).component_mul(&Vector::new(2. / 1350., 2. / 900. / aspect));
                    let second = marker.transform(delta, size / 900. * (2. / 1350.), Vector::repeat(900.), rotation, aspect);
                    let actual = second.point(Vector::new(0., -0.5)) - first.point(Vector::new(0., 0.5));
                    let expected = (rot * Vector::new(0., gap)).component_mul(&Vector::new(2. / 1350., 2. / 900. / aspect));
                    assert!((actual - expected).norm() < 1e-6, "rotation={rotation}, aspect={aspect}, gap={gap}");
                }
            }
        }
    }

    #[test]
    fn line2area_preview_is_immediate_ready_then_active_then_dim() {
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(0.));
        marker.set_lifecycle(&[(1., 1.), (3., 2.), (5., 3.), (7., 4.)]);
        for (time, active, ready) in [(1., false, false), (2.49, false, false), (2.5, false, true), (2.99, false, true), (3., true, false), (5., false, false)] {
            let mut area = area(&marker, time).unwrap();
            area.rpe_canvas = true;
            let zone = Zone::from_area(&area, time, 16. / 9.).unwrap();
            assert_eq!((zone.active, zone.ready, zone.opacity), (active, ready, 1.));
        }
        assert!(area(&marker, 7.).is_none());
    }

    #[test]
    fn recorder_full_screen_rectangle_covers_every_viewport_and_blocks_its_corners() {
        let marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        for aspect in [3. / 4., 1., 4. / 3., 3. / 2., 16. / 9., 20. / 9.] {
            let tr = marker.transform(Vector::zeros(), Vector::new(1.5, 1.) * (2. / 1350.), Vector::repeat(900.), 0., aspect);
            assert!((tr.size.x - 2.).abs() < 1e-6);
            assert!((tr.size.y * tr.y_scale * aspect - 2.).abs() < 1e-6, "1350x900 must fill the RPE viewport at aspect {aspect}");
            let mut rect = area(&marker, 1.).unwrap();
            rect.bottom_left = Vector::zeros();
            rect.top_right = Vector::new(tr.size.x / 2., tr.size.y * tr.y_scale * aspect / 2.);
            // Input intentionally leaves a native safety inset at the rim.
            for x in [-0.8, 0.8] {
                for y in [-0.8 / aspect, 0.8 / aspect] {
                    assert!(crate::core::block_touch_blocked(std::slice::from_ref(&rect), Vector::new(x, y), 1., aspect));
                }
            }
        }
    }

    #[test]
    fn speed_codes_override_editor_alpha_and_restart_lifecycles() {
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(0.));
        marker.set_lifecycle(&[(0., 10.), (2., 1.), (4., 2.), (6., 3.), (8., 2.), (9., 4.), (10., 2.), (11., 4.)]);
        assert!(marker.timings(1.).is_none());
        let preview = area(&marker, 3.).unwrap();
        assert_eq!([preview.appear_time, preview.enable_time, preview.disable_time, preview.disappear_time], [2., 4., 6., 8.]);
        assert!(area(&marker, 4.).unwrap().is_active(4.));
        assert!(!area(&marker, 6.).unwrap().is_active(6.));
        assert!(area(&marker, 8.).unwrap().is_active(8.));
        assert!(area(&marker, 9.).is_none());
        assert!(area(&marker, 10.).unwrap().is_active(10.));
        assert!(area(&marker, 11.).is_none());
        assert_eq!(marker.timings(3.), Some([2., 4., 6., 8.]));
    }

    #[test]
    fn missing_lifecycle_keeps_alpha_compatibility_and_colors_seek_independently() {
        let mut marker = Marker::new(0, true, &AnimFloat::fixed(1.));
        marker.set_lifecycle(&[(0., 10.)]);
        assert!(area(&marker, 1.).unwrap().is_active(1.));
        assert_eq!(marker.color_at(1.), DEFAULT_COLOR);
        *marker.color.borrow_mut() = Anim::new(vec![
            Keyframe::new(0., Color::new(1., 0., 0., 1.), 2),
            Keyframe::new(2., Color::new(0., 0., 1., 1.), 0),
        ]);
        assert_eq!(marker.color_at(2.), [0., 0., 1.]);
        assert_eq!(marker.color_at(1.), [0.5, 0., 0.5]);
        assert!(area(&marker, 1.).unwrap().is_subtract);
    }

    fn area(marker: &Marker, time: f64) -> Option<BlockArea> {
        let [appear_time, enable_time, disable_time, disappear_time] = marker.timings(time)?;
        Some(BlockArea {
            rpe_canvas: false,
            top_right: Vector::new(0.75, 0.75),
            bottom_left: Vector::new(0.25, 0.25),
            appear_time,
            enable_time,
            disable_time,
            disappear_time,
            is_subtract: marker.invert,
            rotate_events: vec![],
            move_events: vec![],
            scale_events: vec![],
        })
    }

    #[test]
    fn texture_codes_are_shared_and_do_not_match_unrelated_files() {
        assert_eq!(marker_kind("folder/ISSUBTRACT0.PNG"), Some(false));
        assert_eq!(marker_kind("folder\\isSubtract1.png"), Some(true));
        assert_eq!(marker_kind("isSubtract1.png.backup"), None);
    }

    #[test]
    fn input_only_evaluates_markers_and_shared_ancestors() {
        let mut parents = vec![None; 10_000];
        parents[9000] = Some(9001);
        parents[9001] = Some(9002);
        parents[9003] = Some(9001);
        assert_eq!(input_lines(&parents, [9000, 9003].into_iter()), vec![9000, 9001, 9002, 9003]);
        assert!(input_lines(&parents, std::iter::empty()).is_empty());
    }

    #[test]
    fn invalid_or_cyclic_parent_dependencies_do_not_loop_during_loading() {
        assert_eq!(input_lines(&[Some(1), Some(0), Some(99)], [0, 2].into_iter()), vec![0, 1, 2]);
    }

    #[test]
    fn scarlet_combo_60_top_and_bottom_anchors_leave_the_flick_gap() {
        let aspect = 16. / 9.;
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        let scale = Vector::new(0.27777778, 1.5) * (2. / 1350.);
        let pixels = Vector::new(900., 900.);
        marker.anchor = Vector::new(0.5, 1.);
        let lower = marker.transform(Vector::new(0., -425. * 2. / 900. / aspect), scale, pixels, 0., aspect);
        marker.anchor.y = 0.;
        let upper = marker.transform(Vector::new(0., -175. * 2. / 900. / aspect), scale, pixels, 0., aspect);
        let flick_y = -300. * 2. / 900. / aspect;
        assert!(lower.center.y + lower.size.y * lower.y_scale / 2. < flick_y);
        assert!(upper.center.y - upper.size.y * upper.y_scale / 2. > flick_y);
        assert!((lower.center.y + lower.size.y * lower.y_scale / 2. + 0.53125).abs() < 1e-6);
        assert!((upper.center.y - upper.size.y * upper.y_scale / 2. + 0.21875).abs() < 1e-6);
    }

    #[test]
    fn scarlet_combo_273_upper_edge_stays_in_the_bottom_quarter() {
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        marker.anchor = Vector::new(0.5, 1.);
        for aspect in [4. / 3., 16. / 9., 20. / 9.] {
            let position = Vector::new(-630. * 2. / 1350., -225. * 2. / 900. / aspect);
            let tr = marker.transform(position, Vector::new(0.22222222, 1.) * (2. / 1350.), Vector::new(900., 900.), 2.5, aspect);
            let edge = tr.point(Vector::new(0., 0.5));
            assert!((edge - position).norm() < 1e-6, "top anchor must remain at move-event position");
            assert!((edge.y * aspect * 0.5 + 0.5 - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn anchor_offset_rotates_with_parents_and_preserves_negative_scale() {
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        marker.anchor = Vector::new(0., 1.);
        for scale in [Vector::new(2., 3.), Vector::new(-2., 3.), Vector::new(2., -3.)] {
            let tr = marker.transform(Vector::new(0.2, -0.3), scale, Vector::new(1., 1.), 120., 1.5);
            let anchor_local = (marker.anchor - Vector::new(0.5, 0.5)).component_mul(&scale);
            let anchor_world = tr.center + Rotation2::new(tr.rotation.to_radians()) * anchor_local;
            assert!((anchor_world - Vector::new(0.2, -0.3)).norm() < 1e-6);
            assert_eq!(tr.size, scale.map(f32::abs));
        }
        marker.anchor = Vector::new(0.5, 0.5);
        assert_eq!(marker.transform(Vector::zeros(), Vector::new(-2., 3.), Vector::new(1., 1.), 90., 1.5).center, Vector::zeros());
    }

    #[test]
    fn brainrot_64s_corner_anchors_preserve_the_rotated_inner_tips() {
        // Sample's beat 147: 900px guides, 1/3 scale, RPE rotation +45deg.
        // Their [1,1]/[0,0] corners are the inner tips of the two diamonds.
        let mut marker = Marker::new(0, false, &AnimFloat::fixed(1.));
        let size = Vector::new(900., 900.);
        let scale = Vector::repeat((1. / 3.) * 2. / 1350.);
        for (anchor, x) in [(Vector::repeat(1.), -212.132036), (Vector::zeros(), 212.132036)] {
            marker.anchor = anchor;
            let tip = Vector::new(x * 2. / 1350., 0.);
            let tr = marker.transform(tip, scale, size, -45., 16. / 9.);
            let corner = tr.point(anchor - Vector::repeat(0.5));
            assert!((corner - tip).norm() < 1e-6);
            // Rotate the authored square first; its inner tip stays on the
            // canvas horizontal axis at every viewport aspect.
            assert!(((tr.center.x - tip.x).abs() - 0.31426968).abs() < 1e-6);
            assert!(tr.center.y.abs() < 1e-6);
        }
    }

    #[test]
    fn sample_preview_codes_use_native_ready_and_active_materials() {
        for code in [121., 127., 128.] {
            let alpha = AnimFloat::new(vec![
                Keyframe::new(0., 0., 0),
                Keyframe::new(1., code / 255., 0),
                Keyframe::new(3., 1., 0),
                Keyframe::new(4., code / 255., 0),
                Keyframe::new(5., 0., 0),
            ]);
            let marker = Marker::new(0, false, &alpha);
            assert!(area(&marker, 0.5).is_none());
            let preview = area(&marker, 2.).unwrap();
            assert!(!preview.is_active(2.));
            assert!(!crate::core::block_touch_blocked(&[preview], Vector::zeros(), 2., 16. / 9.));
            let preview = area(&marker, 2.).unwrap();
            let preview = Zone::from_area(&preview, 2., 16. / 9.).unwrap();
            assert!(!preview.active && !preview.ready);
            let ready = Zone::from_area(&area(&marker, 2.75).unwrap(), 2.75, 16. / 9.).unwrap();
            assert!(!ready.active && ready.ready);
            let active = area(&marker, 3.).unwrap();
            assert!(active.is_active(3.));
            assert!(Zone::from_area(&active, 3., 16. / 9.).unwrap().active);
            assert!(crate::core::block_touch_blocked(&[active], Vector::zeros(), 3., 16. / 9.));
            assert!(!area(&marker, 4.).unwrap().is_active(4.));
            assert!(area(&marker, 5.).is_none());
            // Seeking backwards must restore the same preview interval.
            assert!(!area(&marker, 2.).unwrap().is_active(2.));
        }
    }

    #[test]
    fn animated_and_chained_alpha_crosses_activation_without_rounding_to_255() {
        let alpha = AnimFloat::chain(vec![
            AnimFloat::new(vec![Keyframe::new(0., 0., 2), Keyframe::new(2., 0.8, 2)]),
            AnimFloat::fixed(0.1),
        ]);
        let marker = Marker::new(0, true, &alpha);
        assert!(!area(&marker, 0.5).unwrap().is_active(0.5));
        assert!(area(&marker, 1.5).unwrap().is_active(1.5));
        assert!(area(&marker, 1.5).unwrap().is_subtract);
    }
}
