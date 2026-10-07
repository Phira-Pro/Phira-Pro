//! Flat block ranges without texture uploads, scene copies or effect shaders.
//! A planar sweep unions normal rectangles before applying the visual subtract
//! threshold. Overlapping rectangles therefore do not darken the flat fill.
use super::Zone;
use macroquad::prelude::*;
use std::{cell::RefCell, collections::BTreeSet};

#[derive(Clone, Copy)]
struct Boundary {
    slope: f64,
    intercept: f64,
    bottom: f64,
    top: f64,
    delta: f64,
    invert: bool,
    opacity: f64,
    zone: usize,
}

impl Boundary {
    fn x(self, y: f64) -> f64 {
        self.slope * y + self.intercept
    }
}

#[derive(Default)]
struct FlatRanges {
    zones: Vec<Zone>,
    aspect: f32,
    layers: [Vec<([Vec2; 4], f32, [f32; 3])>; 2],
}

thread_local! {
    static RANGES: RefCell<FlatRanges> = RefCell::new(FlatRanges::default());
}

pub(super) fn draw(aspect: f32, zones: &[Zone], disabled: bool) {
    RANGES.with(|cached| {
        let mut cached = cached.borrow_mut();
        if cached.aspect != aspect || cached.zones != zones {
            cached.aspect = aspect;
            cached.zones.clear();
            cached.zones.extend_from_slice(zones);
            cached.layers = [tessellate(aspect, zones, false), tessellate(aspect, zones, true)];
        }
        gl_use_default_material();
        for &(p, opacity, rgb) in &cached.layers[usize::from(disabled)] {
            let fill = Color::new(rgb[0], rgb[1], rgb[2], opacity * if disabled { 0.12 } else { 0.4 });
            draw_triangle(p[0], p[1], p[2], fill);
            draw_triangle(p[0], p[2], p[3], fill);
        }
    });
}

fn tessellate(aspect: f32, zones: &[Zone], disabled: bool) -> Vec<([Vec2; 4], f32, [f32; 3])> {
    if !aspect.is_finite() || aspect <= 0. {
        return Vec::new();
    }
    let half_height = 1. / aspect as f64;
    let mut cuts = vec![-half_height, half_height];
    let mut edges = Vec::new();
    for (id, z) in zones.iter().enumerate().filter(|(_, z)| z.active != disabled && z.opacity > 0.) {
        let (s, c) = (z.angle as f64).sin_cos();
        let corners = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)].map(|(x, y)| {
            let (x, y) = (x * z.half.x as f64, y * z.half.y as f64);
            (z.center.x as f64 + c * x - s * y, z.center.y as f64 + s * x + c * y)
        });
        if corners.iter().any(|&(x, y)| !x.is_finite() || !y.is_finite()) {
            continue;
        }
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            if a.1 == b.1 {
                continue;
            }
            let (bottom, top) = (a.1.min(b.1).max(-half_height), a.1.max(b.1).min(half_height));
            if bottom >= top {
                continue;
            }
            let slope = (b.0 - a.0) / (b.1 - a.1);
            let edge = Boundary {
                slope,
                intercept: a.0 - slope * a.1,
                bottom,
                top,
                delta: if b.1 > a.1 { -1. } else { 1. },
                invert: z.invert,
                opacity: z.opacity as f64,
                zone: id,
            };
            cuts.extend([bottom, top]);
            // Clipping must also split where a slanted side crosses the viewport.
            if slope != 0. {
                for x in [-1., 1.] {
                    let y = (x - edge.intercept) / slope;
                    if y > bottom && y < top {
                        cuts.push(y);
                    }
                }
            }
            edges.push(edge);
        }
    }
    // Split at crossings so the left-to-right order is constant in each band.
    for (i, a) in edges.iter().enumerate() {
        for b in &edges[..i] {
            let divisor = a.slope - b.slope;
            if divisor == 0. {
                continue;
            }
            let y = (b.intercept - a.intercept) / divisor;
            if y > a.bottom.max(b.bottom) && y < a.top.min(b.top) {
                cuts.push(y);
            }
        }
    }
    cuts.sort_unstable_by(f64::total_cmp);
    cuts.dedup();
    let mut result = Vec::new();
    let mut crossings = Vec::new();
    let colored = zones.iter().any(|z| z.color != super::DEFAULT_BLOCK_COLOR);
    let mut owners: [BTreeSet<usize>; 2] = Default::default();
    for band in cuts.windows(2) {
        let (lo, hi) = (band[0], band[1]);
        let mid = (lo + hi) * 0.5;
        if mid == lo || mid == hi {
            continue;
        }
        crossings.clear();
        crossings.extend(edges.iter().copied().filter(|e| e.bottom < mid && mid < e.top));
        crossings.sort_unstable_by(|a, b| a.x(mid).total_cmp(&b.x(mid)));
        let (mut normal, mut subtract, mut subtract_opacity) = (0_f64, 0_f64, 0_f64);
        owners.iter_mut().for_each(BTreeSet::clear);
        for pair in crossings.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if colored {
                let owners = &mut owners[usize::from(left.invert)];
                if left.delta > 0. {
                    owners.insert(left.zone);
                } else {
                    owners.remove(&left.zone);
                }
            }
            if left.invert {
                subtract += left.delta;
                subtract_opacity += left.delta * left.opacity;
            } else {
                normal += left.delta * left.opacity;
            }
            // Native visual inversion selects exactly one subtract layer;
            // input parity remains in block.rs and is unaffected by this mode.
            let inverted = (subtract - 1.).abs() < 0.5;
            let inverse = if disabled {
                let green = (subtract_opacity * 0.1).clamp(0., 1.);
                let t = ((green - 0.2) * -10.).clamp(0., 1.);
                let r = (if inverted { 1. } else { 0. }) + t * t * (3. - 2. * t);
                r.clamp(0., 1.) * (green * r * 10.).clamp(0., 1.)
            } else {
                if inverted {
                    1.
                } else {
                    0.
                }
            };
            let opacity = (normal.clamp(0., 1.) - inverse).abs() as f32;
            if opacity < 1e-6 || left.x(mid).max(-1.) >= right.x(mid).min(1.) {
                continue;
            }
            let p = [
                vec2(left.x(lo).clamp(-1., 1.) as f32, lo as f32),
                vec2(right.x(lo).clamp(-1., 1.) as f32, lo as f32),
                vec2(right.x(hi).clamp(-1., 1.) as f32, hi as f32),
                vec2(left.x(hi).clamp(-1., 1.) as f32, hi as f32),
            ];
            let kind = usize::from(normal < inverse);
            let owner = owners[kind].last().or_else(|| owners[1 - kind].last());
            let rgb = owner.map_or(super::DEFAULT_BLOCK_COLOR, |&id| zones[id].color);
            // Preserve the established flat red for the default palette.
            let rgb = if rgb == super::DEFAULT_BLOCK_COLOR { [1., 0., 0.] } else { rgb };
            result.push((p, opacity, rgb));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Vector;

    fn zone(x: f32, y: f32, hx: f32, hy: f32, angle: f32, invert: bool) -> Zone {
        Zone {
            color: super::super::DEFAULT_BLOCK_COLOR,
            center: Vector::new(x, y),
            half: Vector::new(hx, hy),
            angle,
            invert,
            active: true,
            ready: false,
            opacity: 1.,
        }
    }

    #[test]
    fn flat_ranges_match_native_coverage_without_darkening_overlaps() {
        let zones = [
            zone(-0.3, 0.05, 0.42, 0.15, 0.2, false),
            zone(0.1, -0.1, 0.34, 0.29, -0.9, false),
            zone(0., 0., 1.5, 1., 0., true),
            zone(0.6, 0.2, 0.35, 0.13, 1.2, true),
            zone(0.6, 0.2, 0.15, 0.09, -0.4, true),
        ];
        let aspect = 16. / 9.;
        let ranges = tessellate(aspect, &zones, false);
        for y in 0..53 {
            for x in 0..97 {
                let p = Vector::new((x as f32 + 0.37) * 2. / 97. - 1., ((y as f32 + 0.41) * 2. / 53. - 1.) / aspect);
                let (mut normal, mut subtract) = (false, 0);
                for z in &zones {
                    let q = nalgebra::Rotation2::new(-z.angle) * (p - z.center);
                    if q.x.abs() < z.half.x && q.y.abs() < z.half.y {
                        if z.invert {
                            subtract += 1;
                        } else {
                            normal = true;
                        }
                    }
                }
                let hits: Vec<_> = ranges
                    .iter()
                    .filter(|(q, _, _)| {
                        if p.y < q[0].y || p.y >= q[2].y {
                            return false;
                        }
                        let t = (p.y - q[0].y) / (q[2].y - q[0].y);
                        let left = q[0].x + (q[3].x - q[0].x) * t;
                        let right = q[1].x + (q[2].x - q[1].x) * t;
                        p.x > left && p.x < right
                    })
                    .collect();
                assert_eq!(hits.len(), usize::from(normal ^ (subtract == 1)), "({x},{y})");
                assert!(hits.iter().all(|(_, a, _)| (*a - 1.).abs() < 1e-5));
            }
        }
    }

    #[test]
    fn disabled_flat_range_retains_appear_fade_and_cancels_subtract_overlap() {
        let mut z = zone(0., 0., 0.5, 0.25, 0., false);
        z.active = false;
        z.opacity = 0.3;
        let ranges = tessellate(1., &[z], true);
        assert_eq!(ranges.len(), 1);
        assert!((ranges[0].1 - 0.3).abs() < 1e-6);
        let s = zone(0., 0., 1., 1., 0., true);
        assert!(tessellate(1., &[s.clone(), s.clone(), s], false).is_empty());
    }

    #[test]
    fn colored_flat_ranges_keep_separate_colors_and_union_overlaps() {
        let mut green = zone(-0.25, 0., 0.5, 0.25, 0., false);
        green.color = [0., 1., 0.];
        let mut blue = zone(0.25, 0., 0.5, 0.25, 0., false);
        blue.color = [0., 0., 1.];
        let ranges = tessellate(1., &[green, blue], false);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].2, [0., 1., 0.]);
        assert_eq!(ranges[1].2, [0., 0., 1.]);
        assert_eq!(ranges[2].2, [0., 0., 1.]);
        assert!(ranges.iter().all(|(_, opacity, _)| *opacity == 1.));
    }
}
