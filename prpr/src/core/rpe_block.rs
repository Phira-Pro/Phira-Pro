//! Adapter for texture-line block markers. The PNGs are editor guides, not
//! runtime materials; their alpha tracks select the native block lifecycle.

use super::{AnimFloat, BlockPhase};

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
        Self { line, invert, spans }
    }

    /// Timings for the existing Disabled/Ready/Active materials and input model.
    pub fn timings(&self, time: f64) -> Option<[f64; 4]> {
        let index = self.spans.partition_point(|span| span.start <= time).checked_sub(1)?;
        let current = &self.spans[index];
        if current.phase == BlockPhase::Hidden {
            return None;
        }
        Some(current.timings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BlockArea, Keyframe, Vector, Zone};

    fn area(marker: &Marker, time: f64) -> Option<BlockArea> {
        let [appear_time, enable_time, disable_time, disappear_time] = marker.timings(time)?;
        Some(BlockArea {
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
