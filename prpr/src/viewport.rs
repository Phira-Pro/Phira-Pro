//! Fitted playfield geometry. Origins use OpenGL's bottom-left convention;
//! UI positions use the viewport width as their unit, with Y pointing down.
use macroquad::prelude::Vec2;

pub type Viewport = (i32, i32, i32, i32);

pub fn playfield(parent: Viewport, chart_ratio: f32, override_ratio: Option<f32>, forced: bool) -> (f32, Viewport) {
    let valid = |v: &f32| v.is_finite() && *v > 0.;
    let requested = override_ratio
        .filter(valid)
        .or_else(|| Some(chart_ratio).filter(valid))
        .unwrap_or(16. / 9.);
    let (x, y, w, h) = parent;
    let (w, h) = (w.max(1), h.max(1));
    let aspect = if forced { requested } else { requested.min(w as f32 / h as f32) };
    let (rw, rh) = if h as f32 * aspect > w as f32 {
        (w, (w as f32 / aspect).round().clamp(1., h as f32) as i32)
    } else {
        ((h as f32 * aspect).round().clamp(1., w as f32) as i32, h)
    };
    (aspect, (x + (w - rw) / 2, y + (h - rh) / 2, rw, rh))
}

/// Map a width-normalized UI point between viewports, including letterbox
/// offsets. Scaling alone loses the half-pixel offset of odd-sized viewports.
pub fn map_position(point: Vec2, from: Viewport, to: Viewport) -> Vec2 {
    let scale = from.2 as f32 / to.2.max(1) as f32;
    let dx = (from.0 as f32 + from.2 as f32 / 2.) - (to.0 as f32 + to.2 as f32 / 2.);
    let dy = (to.1 as f32 + to.3 as f32 / 2.) - (from.1 as f32 + from.3 as f32 / 2.);
    point * scale + Vec2::new(dx, dy) * (2. / to.2.max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_and_global_ratios_fit_phone_tablet_and_offset_viewports() {
        for parent in [(0, 0, 2400, 1080), (0, 0, 2048, 1536), (31, 107, 1279, 629)] {
            for ratio in [4. / 3., 16. / 9., 21. / 9., 1.] {
                for forced in [false, true] {
                    let chart = playfield(parent, ratio, None, forced);
                    let global = playfield(parent, 16. / 9., Some(ratio), forced);
                    assert_eq!(chart, global);
                    let (aspect, (x, y, w, h)) = chart;
                    assert!(x >= parent.0 && y >= parent.1 && w > 0 && h > 0);
                    assert!(x + w <= parent.0 + parent.2 && y + h <= parent.1 + parent.3);
                    assert!((w as f32 - h as f32 * aspect).abs() <= aspect.max(1.));
                    assert!(((x - parent.0) * 2 + w - parent.2).abs() <= 1);
                    assert!(((y - parent.1) * 2 + h - parent.3).abs() <= 1);
                }
            }
        }
    }

    #[test]
    fn touch_positions_match_rendered_pixels_and_round_trip() {
        let parent = (31, 107, 1279, 629);
        for ratio in [4. / 3., 16. / 9., 21. / 9.] {
            let (_, fitted) = playfield(parent, ratio, None, true);
            for p in [Vec2::ZERO, Vec2::new(-0.94, -1. / ratio + 0.05), Vec2::new(0.7, 0.3)] {
                let outer = map_position(p, fitted, parent);
                let back = map_position(outer, parent, fitted);
                assert!((back - p).length() < 1e-6);
                let pixel = |p: Vec2, vp: Viewport| {
                    Vec2::new(vp.0 as f32 + (p.x + 1.) * vp.2 as f32 / 2., vp.1 as f32 + vp.3 as f32 / 2. - p.y * vp.2 as f32 / 2.)
                };
                assert!((pixel(outer, parent) - pixel(p, fitted)).length() < 0.001);
            }
        }
    }

    #[test]
    fn invalid_saved_ratios_do_not_produce_an_invalid_camera() {
        for bad in [f32::NAN, f32::INFINITY, -1., 0.] {
            let (aspect, vp) = playfield((0, 0, 2400, 1080), bad, Some(bad), true);
            assert_eq!(aspect, 16. / 9.);
            assert!(vp.2 > 0 && vp.3 > 0);
        }
    }
}
