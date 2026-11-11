//! Conservative CPU-clip proof for canonical static note quads. It never
//! creates output vertices or changes the original renderer's numeric values.
use crate::core::Matrix;

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f32,
    hi: f32,
}

fn down(value: f32) -> f32 {
    if value == 0. {
        return -f32::from_bits(1);
    }
    f32::from_bits(if value > 0. { value.to_bits() - 1 } else { value.to_bits() + 1 })
}
fn up(value: f32) -> f32 {
    if value == 0. {
        return f32::from_bits(1);
    }
    f32::from_bits(if value > 0. { value.to_bits() + 1 } else { value.to_bits() - 1 })
}
impl Interval {
    fn range(lo: f32, hi: f32) -> Option<Self> {
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return None;
        }
        let mut interval = Self { lo, hi };
        interval.include_zero_for_subnormals();
        Some(interval)
    }
    fn point(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let mut interval = Self { lo: value, hi: value };
        interval.include_zero_for_subnormals();
        Some(interval)
    }
    fn include_zero_for_subnormals(&mut self) {
        if self.lo < f32::MIN_POSITIVE && self.hi > -f32::MIN_POSITIVE {
            self.lo = self.lo.min(0.);
            self.hi = self.hi.max(0.);
        }
    }
    /// One representable f32 step outwards covers conversion rounding and the
    /// much smaller f64-addition rounding. Bounds also contain the unrounded
    /// real intermediate, so contractions/FMA need no assumed extra rounding.
    fn outward(lo: f64, hi: f64) -> Option<Self> {
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return None;
        }
        let lower = lo as f32;
        let upper = hi as f32;
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        let mut result = Self {
            lo: down(lower),
            hi: up(upper),
        };
        if !result.lo.is_finite() || !result.hi.is_finite() {
            return None;
        }
        result.include_zero_for_subnormals();
        Some(result)
    }
    fn hull(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }
    fn add(self, other: Self) -> Option<Self> {
        Self::outward(self.lo as f64 + other.lo as f64, self.hi as f64 + other.hi as f64)
    }
    fn mul(self, other: Self) -> Option<Self> {
        // Products of finite f32 endpoints need at most 48 significand bits,
        // fitting exactly in f64, including underflow-scale products.
        let products = [
            self.lo as f64 * other.lo as f64,
            self.lo as f64 * other.hi as f64,
            self.hi as f64 * other.lo as f64,
            self.hi as f64 * other.hi as f64,
        ];
        let lo = products.into_iter().fold(f64::INFINITY, f64::min);
        let hi = products.into_iter().fold(f64::NEG_INFINITY, f64::max);
        Self::outward(lo, hi)
    }
}

fn dot3(a: [Interval; 3], b: [Interval; 3]) -> Option<Interval> {
    let p = [a[0].mul(b[0])?, a[1].mul(b[1])?, a[2].mul(b[2])?];
    // All three pair-first groups cover both associations and all index
    // orders. Each product bound contains its exact real product as well as
    // rounded/FTZ results; each addition similarly contains exact and rounded
    // results. Thus separate, single-FMA and chained-FMA reductions are covered.
    let left = p[0].add(p[1])?.add(p[2])?;
    let right = p[1].add(p[2])?.add(p[0])?;
    let alternate = p[0].add(p[2])?.add(p[1])?;
    Some(left.hull(right).hull(alternate))
}

/// Captured current side model, not a Chart::update or input-time snapshot.
/// Parent transformations may already be present; only a finite affine matrix
/// is admitted. This object has no allocations or native resources.
#[derive(Clone, Copy, Debug)]
pub struct Side {
    rows: [[Interval; 3]; 2],
    columns: [[Interval; 2]; 2],
    raw: [[f32; 3]; 2],
}
impl Side {
    pub fn new(parent: &Matrix) -> Option<Self> {
        if !parent.iter().all(|value| value.is_finite()) || parent[(2, 0)] != 0. || parent[(2, 1)] != 0. || parent[(2, 2)] != 1. {
            return None;
        }
        let rows = [
            [
                Interval::point(parent[(0, 0)])?,
                Interval::point(parent[(0, 1)])?,
                Interval::point(parent[(0, 2)])?,
            ],
            [
                Interval::point(parent[(1, 0)])?,
                Interval::point(parent[(1, 1)])?,
                Interval::point(parent[(1, 2)])?,
            ],
        ];
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let columns = [
            [dot3(rows[0], [one, zero, zero])?, dot3(rows[0], [zero, one, zero])?],
            [dot3(rows[1], [one, zero, zero])?, dot3(rows[1], [zero, one, zero])?],
        ];
        let raw = [
            [parent[(0, 0)], parent[(0, 1)], parent[(0, 2)]],
            [parent[(1, 0)], parent[(1, 1)], parent[(1, 2)]],
        ];
        Some(Self { rows, columns, raw })
    }

    /// True proves that the original four-corner CPU AABB is wholly outside
    /// [-1,1]^2. False means no proof: execute the original renderer. None is an
    /// unsupported/non-finite/overflow domain and also requires that fallback.
    /// Caller certifies rotation0, unit scale and the canonical note behavior.
    pub fn outside(&self, x: f32, base: f32, hf: [f32; 2]) -> Option<bool> {
        if !x.is_finite() || !base.is_finite() || !hf.iter().all(|value| value.is_finite() && *value >= 0.) {
            return None;
        }
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let two = Interval::point(2.)?;
        // The source canonical append_translation performs 0+x/0+base.
        // Preserve the operations as intervals rather than assigning raw x/y.
        let tx = zero.add(Interval::point(x)?)?;
        let ty = zero.add(Interval::point(base)?)?;
        let translation = [dot3(self.rows[0], [tx, ty, one])?, dot3(self.rows[1], [tx, ty, one])?];

        // Source draw_tex corners use -half and (-half + half*2), not an
        // algebraic +/-half replacement. Their rectangle hull encloses all
        // four source corners including f32 rounding and possible FTZ.
        let hx = Interval::point(hf[0])?;
        let hy = Interval::point(hf[1])?;
        let left = hx.neg();
        let top = hy.neg();
        let right = left.add(hx.mul(two)?)?;
        let bottom = top.add(hy.mul(two)?)?;
        let local = [left.hull(right), top.hull(bottom)];
        let horizontal = dot3([self.columns[0][0], self.columns[0][1], translation[0]], [local[0], local[1], one])?;
        let vertical = dot3([self.columns[1][0], self.columns[1][1], translation[1]], [local[0], local[1], one])?;
        // Affine bottom row remains exactly [0,0,1] in the source matrix
        // product: all other products are finite * zero. Homogeneous point
        // division, if used by nalgebra, is by exactly one.
        Some(horizontal.lo > 1. || horizontal.hi < -1. || vertical.lo > 1. || vertical.hi < -1.)
    }

    /// True proves that every original four-corner CPU AABB for the closed
    /// x/base domain and every supplied style is outside [-1,1]^2 on one
    /// common axis. A per-point OR of different reject axes cannot prove a
    /// whole tail. False requires the original renderer; None also falls back
    /// for an unsupported/non-finite/overflow domain. Styles are bounded to
    /// 1..=6 finite nonnegative half extents, matching the canonical textures.
    ///
    /// Caller certifies rotation0, unit scale and canonical note behavior, and
    /// supplies bounds containing the actual source f32 x/base values. In
    /// particular this method does not replace the caller's f64 height/base
    /// arithmetic. Every operation below is inclusion-monotone: shrinking
    /// x/base intervals or removing styles cannot invalidate a true proof.
    pub fn outside_range(&self, x_lo: f32, x_hi: f32, base_lo: f32, base_hi: f32, hfs: &[[f32; 2]]) -> Option<bool> {
        if hfs.is_empty() || hfs.len() > 6 {
            return None;
        }
        let x = Interval::range(x_lo, x_hi)?;
        let base = Interval::range(base_lo, base_hi)?;
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let two = Interval::point(2.)?;
        // Same append_translation and matrix product graph as `outside`.
        // Range operands also contain input DAZ results; outward arithmetic
        // contains rounded, unrounded/FMA and generated FTZ intermediates.
        let tx = zero.add(x)?;
        let ty = zero.add(base)?;
        let translation = [dot3(self.rows[0], [tx, ty, one])?, dot3(self.rows[1], [tx, ty, one])?];

        let mut local: Option<[Interval; 2]> = None;
        for &hf in hfs {
            if !hf.iter().all(|value| value.is_finite() && *value >= 0.) {
                return None;
            }
            let hx = Interval::point(hf[0])?;
            let hy = Interval::point(hf[1])?;
            let left = hx.neg();
            let top = hy.neg();
            // Keep -half + half*2 in its source order, including separate
            // rounding/FTZ or contraction. Never substitute +/-half.
            let right = left.add(hx.mul(two)?)?;
            let bottom = top.add(hy.mul(two)?)?;
            let style = [left.hull(right), top.hull(bottom)];
            local = Some(match local {
                Some(bounds) => [bounds[0].hull(style[0]), bounds[1].hull(style[1])],
                None => style,
            });
        }
        let local = local?;
        let horizontal = dot3([self.columns[0][0], self.columns[0][1], translation[0]], [local[0], local[1], one])?;
        let vertical = dot3([self.columns[1][0], self.columns[1][1], translation[1]], [local[0], local[1], one])?;
        // All intermediate bounds must remain finite, including zero times
        // an otherwise overflowing value. Side::new already excludes a
        // projective row; homogeneous division therefore remains by one.
        Some(horizontal.lo > 1. || horizontal.hi < -1. || vertical.lo > 1. || vertical.hi < -1.)
    }

    /// Faster opt-in proof with a narrower domain than `outside`. Raw parent
    /// coefficients and x/base/half extents must be normal-or-zero. The caller
    /// also certifies the canonical note and a normal positive aspect so its
    /// original zero/aspect operation cannot become 0/0 under DAZ.
    ///
    /// In this domain the parent*unit-note first two columns equal the captured
    /// parent coefficients numerically: multiplication by 1/0 and addition of
    /// signed zero introduce no error in a normal nonzero coefficient. For
    /// each coordinate, translation dot3 followed by point dot3 uses fewer than
    /// 16 f32 rounds; contraction/FMA removes rounds. For u=2^-24,
    /// gamma16=16*u/(1-16*u) is slightly above 2^-20 and below 2^-19.
    /// A 2^-16 relative budget is over 15 times larger, leaving ample allowance
    /// for the f64 centre/radius/bound arithmetic and all summation orders.
    ///
    /// Every generated f32 subnormal/FTZ loss is < MIN_POSITIVE. There are fewer
    /// than 16 such steps, translation error enters the point with coefficient
    /// 1, and the columns cannot generate a nonzero subnormal in this domain.
    /// 32*MIN_POSITIVE*(1+|a|+|b|) additionally covers potential local-coordinate
    /// underflow/DAZ propagation. This is a magnitude-dependent numeric budget,
    /// never a fixed screen/pixel padding. No allocations or interval dots run
    /// per note; only two scalar f64 coordinate budgets are evaluated.
    pub fn outside_fast(&self, x: f32, base: f32, hf: [f32; 2]) -> Option<bool> {
        let normal_or_zero = |value: f32| value == 0. || value.is_normal();
        if !normal_or_zero(x)
            || !normal_or_zero(base)
            || !hf.iter().all(|&value| normal_or_zero(value) && value >= 0.)
            || !self.raw.iter().flatten().all(|&value| normal_or_zero(value))
        {
            return None;
        }
        // Match the source corner operations even though, for normal halves
        // without overflow, these binary doublings/subtractions are exact.
        let left = -hf[0];
        let top = -hf[1];
        let width = hf[0] * 2.;
        let height = hf[1] * 2.;
        let right = left + width;
        let bottom = top + height;
        if ![left, top, width, height, right, bottom].iter().all(|value| value.is_finite()) {
            return None;
        }
        let extent_x = left.abs().max(right.abs()) as f64;
        let extent_y = top.abs().max(bottom.abs()) as f64;
        let x = x as f64;
        let base = base as f64;
        let mut outside = false;
        for &[a, b, c] in &self.raw {
            let (a, b, c) = (a as f64, b as f64, c as f64);
            let centre = a * x + b * base + c;
            let radius = a.abs() * extent_x + b.abs() * extent_y;
            let magnitude = a.abs() * (x.abs() + extent_x) + b.abs() * (base.abs() + extent_y) + c.abs();
            // All source products, translation sums and point sums are then
            // separated from f32 overflow by over a factor of 31. Rejection
            // avoids trying to reason about infinity/NaN cancellation.
            if !magnitude.is_finite() || magnitude >= f32::MAX as f64 / 32. {
                return None;
            }
            let error = magnitude / 65_536. + 32. * f32::MIN_POSITIVE as f64 * (1. + a.abs() + b.abs());
            let lower = centre - radius - error;
            let upper = centre + radius + error;
            if ![centre, radius, error, lower, upper].iter().all(|value| value.is_finite()) {
                return None;
            }
            outside |= lower > 1. || upper < -1.;
        }
        Some(outside)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Point, Vector};
    use nalgebra::Rotation2;

    // Independent source renderer oracle. It deliberately does not call the
    // interval helpers, use center/radius shortcuts or change corner order.
    fn original_outside(parent: &Matrix, x: f32, base: f32, hf: [f32; 2]) -> bool {
        let note = Rotation2::new((0.0f32).to_radians())
            .to_homogeneous()
            .append_nonuniform_scaling(&Vector::new(1., 1.))
            .append_translation(&Vector::new(x, base));
        let model = parent * note;
        let left = -hf[0];
        let top = -hf[1];
        let width = hf[0] * 2.;
        let height = hf[1] * 2.;
        let points = [
            Point::new(left, top),
            Point::new(left + width, top),
            Point::new(left + width, top + height),
            Point::new(left, top + height),
        ]
        .map(|point| model.transform_point(&point));
        points[0].x.min(points[1].x.min(points[2].x.min(points[3].x))) > 1.
            || points[0].x.max(points[1].x.max(points[2].x.max(points[3].x))) < -1.
            || points[0].y.min(points[1].y.min(points[2].y.min(points[3].y))) > 1.
            || points[0].y.max(points[1].y.max(points[2].y.max(points[3].y))) < -1.
    }
    fn verify(parent: &Matrix, x: f32, base: f32, hf: [f32; 2]) -> bool {
        if let Some(side) = Side::new(parent) {
            if side.outside(x, base, hf) == Some(true) {
                assert!(original_outside(parent, x, base, hf), "false cull: parent={parent:?}, x={x:?}, base={base:?}, half={hf:?}");
                return true;
            }
        }
        false
    }
    #[test]
    fn thousands_of_affine_rotated_flipped_scaled_and_singular_models_have_no_false_cull() {
        let mut random = 23u32;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            (random >> 8) as f32 / 16_777_216.
        };
        let mut proved = 0;
        for round in 0..12_000 {
            let angle = (next() * 2. - 1.) * 6.3;
            let mut sx = (next() * 2. - 1.) * 4.;
            let mut sy = (next() * 2. - 1.) * 4.;
            if round % 19 == 0 {
                sx = 0.;
            }
            if round % 31 == 0 {
                sy = 0.;
            }
            let parent = Rotation2::new(angle)
                .to_homogeneous()
                .append_nonuniform_scaling(&Vector::new(sx, sy))
                .append_translation(&Vector::new((next() * 2. - 1.) * 8., (next() * 2. - 1.) * 8.));
            let x = (next() * 2. - 1.) * 16.;
            let base = (next() * 2. - 1.) * 32.;
            let half = [next() * 1.5, next() * 2.];
            proved += verify(&parent, x, base, half) as usize;
        }
        assert!(proved > 1000, "proof never produced useful culls: {proved}");
    }
    #[test]
    fn border_signed_zero_subnormal_and_large_values_remain_conservative() {
        let values = [
            -0.,
            0.,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::MIN_POSITIVE / 2.,
            -f32::MIN_POSITIVE,
            down(1.),
            1.,
            up(1.),
            -1.,
            down(-1.),
            up(-1.),
            1e30,
            -1e30,
            f32::MAX,
            -f32::MAX,
        ];
        let parents = [
            Matrix::identity(),
            Matrix::new_nonuniform_scaling(&Vector::new(-1., 1.)),
            Matrix::new_nonuniform_scaling(&Vector::new(0., -1.)),
            Matrix::new_translation(&Vector::new(up(1.), down(-1.))),
        ];
        for parent in &parents {
            for &x in &values {
                for &base in &values {
                    for half in [[0., 0.], [f32::from_bits(1), f32::MIN_POSITIVE], [0.01, 0.03], [1., 1.], [1e20, 1e20]] {
                        verify(parent, x, base, half);
                    }
                }
            }
        }
        let side = Side::new(&Matrix::identity()).unwrap();
        assert_eq!(side.outside(0., 10., [0.1, 0.2]), Some(true));
        assert_eq!(side.outside(0., 0., [0.1, 0.2]), Some(false));
        assert_eq!(side.outside(1., 0., [0., 0.]), Some(false));
    }
    #[test]
    fn arbitrary_finite_affine_coefficients_and_fma_reductions_are_enclosed() {
        let mut random = 91u32;
        let mut next = || {
            random = random.wrapping_mul(1103515245).wrapping_add(12345);
            (random >> 8) as f32 / 16_777_216. * 200. - 100.
        };
        for _ in 0..4000 {
            let parent = Matrix::new(next(), next(), next(), next(), next(), next(), 0., 0., 1.);
            verify(&parent, next(), next(), [1.25, 0.03125]);
            let a = [next(), next(), next()];
            let b = [next(), next(), next()];
            let bounds = dot3(a.map(|v| Interval::point(v).unwrap()), b.map(|v| Interval::point(v).unwrap())).unwrap();
            let candidates = [
                (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2],
                a[0] * b[0] + (a[1] * b[1] + a[2] * b[2]),
                a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2])),
                a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1])),
            ];
            for value in candidates {
                assert!(value >= bounds.lo && value <= bounds.hi);
            }
        }
    }
    #[test]
    fn unsupported_nonfinite_nonaffine_or_overflow_inputs_fall_back() {
        let side = Side::new(&Matrix::identity()).unwrap();
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(side.outside(invalid, 0., [1., 1.]), None);
            assert_eq!(side.outside(0., invalid, [1., 1.]), None);
            assert_eq!(side.outside(0., 0., [invalid, 1.]), None);
            let mut parent = Matrix::identity();
            parent[(0, 2)] = invalid;
            assert!(Side::new(&parent).is_none());
        }
        assert_eq!(side.outside(0., 0., [-1., 1.]), None);
        assert_eq!(side.outside(0., 0., [f32::MAX, 1.]), None);
        let mut parent = Matrix::identity();
        parent[(2, 0)] = 0.01;
        assert!(Side::new(&parent).is_none());
        parent = Matrix::identity();
        parent[(2, 2)] = 2.;
        assert!(Side::new(&parent).is_none());
    }

    fn verify_fast(parent: &Matrix, x: f32, base: f32, hf: [f32; 2]) -> bool {
        if let Some(side) = Side::new(parent) {
            if side.outside_fast(x, base, hf) == Some(true) {
                assert!(original_outside(parent, x, base, hf), "fast false cull: parent={parent:?}, x={x:?}, base={base:?}, half={hf:?}");
                return true;
            }
        }
        false
    }

    #[test]
    fn fast_sixteen_thousand_original_affine_oracles_produce_useful_culls() {
        let mut random = 47u32;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            (random >> 8) as f32 / 16_777_216.
        };
        let mut proved = 0;
        for round in 0..16_000 {
            let angle = (next() * 2. - 1.) * 6.3;
            let mut sx = (next() * 2. - 1.) * 4.;
            let mut sy = (next() * 2. - 1.) * 4.;
            if round % 19 == 0 {
                sx = 0.;
            }
            if round % 31 == 0 {
                sy = 0.;
            }
            let parent = Rotation2::new(angle)
                .to_homogeneous()
                .append_nonuniform_scaling(&Vector::new(sx, sy))
                .append_translation(&Vector::new((next() * 2. - 1.) * 8., (next() * 2. - 1.) * 8.));
            proved += verify_fast(&parent, (next() * 2. - 1.) * 16., (next() * 2. - 1.) * 32., [next() * 1.5, next() * 2.]) as usize;
        }
        assert!(proved > 1000, "fast proof was vacuous: {proved}");
    }

    #[test]
    fn fast_near_border_cancellation_large_and_generated_subnormal_cases_do_not_false_cull() {
        let values = [-0., 0., -1., down(-1.), up(-1.), 1., down(1.), up(1.), 1e10, -1e10, 1e30, -1e30];
        let parents = [
            Matrix::identity(),
            Matrix::new_nonuniform_scaling(&Vector::new(-1., 1.)),
            Matrix::new_nonuniform_scaling(&Vector::new(0., -1.)),
            Matrix::new_translation(&Vector::new(up(1.), down(-1.))),
            Matrix::new(1., -1., up(1.), 1., 1., down(-1.), 0., 0., 1.),
            Matrix::new(f32::MIN_POSITIVE, -f32::MIN_POSITIVE, up(1.), 0., f32::MIN_POSITIVE, down(-1.), 0., 0., 1.),
        ];
        for parent in &parents {
            for &x in &values {
                for &base in &values {
                    for half in [[0., 0.], [0.01, 0.03], [1., 1.], [f32::MIN_POSITIVE, f32::MIN_POSITIVE], [1e20, 1e20]] {
                        verify_fast(parent, x, base, half);
                    }
                }
            }
        }
        // Construct centres mathematically close to a clip boundary while
        // much larger products cancel; these must not trust f64 centre alone.
        for exponent in [0, 5, 10, 20, 30, 50, 80, 100] {
            let coefficient = 2.0f32.powi(exponent);
            for value in [0.5, 1., up(1.), down(1.), 3.] {
                let parent = Matrix::new(coefficient, -coefficient, 1., coefficient, coefficient, -1., 0., 0., 1.);
                for base in [value, up(value), down(value)] {
                    verify_fast(&parent, value, base, [0., 0.]);
                }
            }
        }
        let side = Side::new(&Matrix::identity()).unwrap();
        assert_eq!(side.outside_fast(0., 10., [0.1, 0.2]), Some(true));
        assert_eq!(side.outside_fast(0., 0., [0.1, 0.2]), Some(false));
        assert_eq!(side.outside_fast(1., 0., [0., 0.]), Some(false));
    }

    #[test]
    fn fast_input_subnormals_and_unsafe_magnitudes_fall_back_without_changing_old_proof() {
        let side = Side::new(&Matrix::identity()).unwrap();
        for invalid in [f32::from_bits(1), -f32::from_bits(1), f32::MIN_POSITIVE / 2., f32::NAN, f32::INFINITY] {
            assert_eq!(side.outside_fast(invalid, 0., [1., 1.]), None);
            assert_eq!(side.outside_fast(0., invalid, [1., 1.]), None);
            assert_eq!(side.outside_fast(0., 0., [invalid, 1.]), None);
        }
        let mut parent = Matrix::identity();
        parent[(0, 1)] = f32::from_bits(1);
        let side_subnormal = Side::new(&parent).unwrap();
        assert_eq!(side_subnormal.outside_fast(0., 10., [0.1, 0.2]), None);
        assert_eq!(side_subnormal.outside(0., 10., [0.1, 0.2]), Some(true));
        assert_eq!(side.outside_fast(0., 0., [-1., 1.]), None);
        assert_eq!(side.outside_fast(0., 0., [f32::MAX, 1.]), None);
        assert_eq!(side.outside_fast(f32::MAX / 16., 0., [0., 0.]), None);
    }

    #[test]
    fn fast_proposals_also_reject_explicit_fma_and_generated_ftz_point_oracles() {
        fn ftz(value: f32) -> f32 {
            if value.is_subnormal() {
                0f32.copysign(value)
            } else {
                value
            }
        }
        let mut random = 137u32;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            (random >> 8) as f32 / 16_777_216. * 32. - 16.
        };
        let mut proposals = 0;
        for round in 0..8000 {
            let mut parent = Matrix::new(next(), next(), next(), next(), next(), next(), 0., 0., 1.);
            if round % 7 == 0 {
                parent[(0, 0)] = f32::MIN_POSITIVE;
                parent[(1, 1)] = -f32::MIN_POSITIVE;
            }
            let x = if round % 11 == 0 { f32::MIN_POSITIVE } else { next() };
            let base = if round % 13 == 0 { -f32::MIN_POSITIVE } else { next() };
            let hf = [0.125, 0.25];
            let Some(side) = Side::new(&parent) else {
                continue;
            };
            if side.outside_fast(x, base, hf) != Some(true) {
                continue;
            }
            proposals += 1;
            let left = -hf[0];
            let top = -hf[1];
            let right = left + hf[0] * 2.;
            let bottom = top + hf[1] * 2.;
            let local = [(left, top), (right, top), (right, bottom), (left, bottom)];
            // Alternative independent machine-evaluation oracles, not the
            // centre/radius/error approximation used by the fast proposal.
            for mode in 0..3 {
                let mut points = [[0f32; 2]; 4];
                for row in 0..2 {
                    let (a, b, c) = (parent[(row, 0)], parent[(row, 1)], parent[(row, 2)]);
                    let translation = match mode {
                        0 => a.mul_add(x, b.mul_add(base, c)),
                        1 => b.mul_add(base, a.mul_add(x, c)),
                        _ => ftz(ftz(ftz(a * x) + ftz(b * base)) + c),
                    };
                    for (index, &(qx, qy)) in local.iter().enumerate() {
                        points[index][row] = match mode {
                            0 => a.mul_add(qx, b.mul_add(qy, translation)),
                            1 => b.mul_add(qy, a.mul_add(qx, translation)),
                            _ => ftz(ftz(ftz(a * qx) + ftz(b * qy)) + translation),
                        };
                    }
                }
                let rejected = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) > 1.
                    || points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) < -1.
                    || points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) > 1.
                    || points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) < -1.;
                assert!(rejected, "fast proposal admitted an FMA/FTZ-visible quad: mode={mode}, parent={parent:?}, x={x}, base={base}");
            }
        }
        assert!(proposals > 1000, "explicit-machine oracle check was vacuous: {proposals}");
    }

    fn samples(lo: f32, hi: f32) -> [f32; 5] {
        // f64 interpolation only selects actual finite f32 test inputs; the
        // independent original oracle retains every renderer operation.
        [
            lo,
            (lo as f64 * 0.75 + hi as f64 * 0.25) as f32,
            (lo as f64 * 0.5 + hi as f64 * 0.5) as f32,
            (lo as f64 * 0.25 + hi as f64 * 0.75) as f32,
            hi,
        ]
    }

    fn verify_range(parent: &Matrix, x: [f32; 2], base: [f32; 2], hfs: &[[f32; 2]]) -> bool {
        let Some(side) = Side::new(parent) else {
            return false;
        };
        if side.outside_range(x[0], x[1], base[0], base[1], hfs) != Some(true) {
            return false;
        }
        for x_sample in samples(x[0], x[1]) {
            for base_sample in samples(base[0], base[1]) {
                for &hf in hfs {
                    assert!(
                        original_outside(parent, x_sample, base_sample, hf),
                        "range false cull: parent={parent:?}, x={x:?}, base={base:?}, half={hf:?}, sample=({x_sample},{base_sample})"
                    );
                }
            }
        }
        true
    }

    #[test]
    fn range_two_thousand_affine_domains_enclose_original_style_corners() {
        let mut random = 173u32;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            (random >> 8) as f32 / 16_777_216.
        };
        let mut proved = 0;
        for round in 0..2000 {
            let mut a = (next() * 2. - 1.) * 4.;
            let mut b = (next() * 2. - 1.) * 4.;
            if round % 19 == 0 {
                a = 0.;
            }
            if round % 31 == 0 {
                b = 0.;
            }
            // Arbitrary affine rows admit rotations, flips, shear, scaling,
            // singular transforms and cancellation without pose assumptions.
            let parent =
                Matrix::new(a, b, (next() * 2. - 1.) * 8., (next() * 2. - 1.) * 4., (next() * 2. - 1.) * 4., (next() * 2. - 1.) * 8., 0., 0., 1.);
            let x_lo = (next() * 2. - 1.) * 16.;
            let base_lo = (next() * 2. - 1.) * 32.;
            let x = [x_lo, x_lo + next() * 2.];
            let base = [base_lo, base_lo + next() * 8.];
            let hfs = [
                [0., 0.],
                [0.03, 0.01],
                [0.125, 0.25],
                [0.5, 0.1],
                [next(), next()],
                [next() * 1.5, next() * 2.],
            ];
            proved += verify_range(&parent, x, base, &hfs) as usize;
        }
        assert!(proved > 500, "range proof was vacuous: {proved}");
    }

    #[test]
    fn range_requires_a_common_reject_axis_and_keeps_clip_boundaries() {
        let side = Side::new(&Matrix::identity()).unwrap();
        let half = [[0.01, 0.01]];
        assert_eq!(side.outside(-2., 0., half[0]), Some(true));
        assert_eq!(side.outside(2., 0., half[0]), Some(true));
        assert_eq!(side.outside_range(-2., 2., 0., 0., &half), Some(false));
        assert!(!original_outside(&Matrix::identity(), 0., 0., half[0]));

        // Both styles are outside, each on a different axis. The common hull
        // intentionally fails to prove the union; no per-style OR is used.
        let hfs = [[0.25, 2.], [2., 0.25]];
        assert_eq!(side.outside(2., 2., hfs[0]), Some(true));
        assert_eq!(side.outside(2., 2., hfs[1]), Some(true));
        assert_eq!(side.outside_range(2., 2., 2., 2., &hfs), Some(false));
        assert_eq!(side.outside_range(1., 1., 0., 0., &[[0., 0.]]), Some(false));
        assert_eq!(side.outside_range(down(1.), up(1.), 0., 0., &[[0., 0.]]), Some(false));
        assert_eq!(side.outside_range(-0.5, 0.5, 4., 8., &[[0.25, 0.5]]), Some(true));
    }

    #[test]
    fn range_nested_tail_domains_are_monotone_including_negative_direction_and_style_removal() {
        let hfs = [[0., 0.], [0.03, 0.01], [0.125, 0.25], [0.5, 0.1], [0.25, 0.5], [0.1, 0.3]];
        let parents = [
            Matrix::identity(),
            Matrix::new_nonuniform_scaling(&Vector::new(-1., -1.)),
            Matrix::new(0.5, -0.25, 0.1, 0.25, 0.75, -0.25, 0., 0., 1.),
            Matrix::new(1., -1., 1., -0.5, 0.5, -1., 0., 0., 1.),
        ];
        let mut transitions = 0;
        for parent in &parents {
            let side = Side::new(parent).unwrap();
            for direction in [-1., 1.] {
                let mut previous = false;
                for index in 0..65 {
                    let near = -8. + index as f32 * 0.25;
                    // Ascending heights with negative speed map to a range
                    // whose upper endpoint shrinks rather than its lower.
                    let base = if direction > 0. { [near, 8.] } else { [-8., -near] };
                    let x_radius = 0.5 * (1. - index as f32 / 64.);
                    let styles = &hfs[..6 - (index / 16).min(5)];
                    let current = side.outside_range(-x_radius, x_radius, base[0], base[1], styles) == Some(true);
                    assert!(!previous || current, "tail proof reverted after shrinking: parent={parent:?}, direction={direction}, index={index}");
                    transitions += (!previous && current) as usize;
                    if current {
                        assert!(verify_range(parent, [-x_radius, x_radius], base, styles));
                    }
                    previous = current;
                }
                assert!(previous, "nested tail never reached an outside proof");
            }
        }
        assert_eq!(transitions, parents.len() * 2);
    }

    #[test]
    fn range_signed_zero_subnormal_large_and_invalid_domains_are_conservative() {
        let sub = f32::from_bits(1);
        let endpoints = [
            -1e30,
            -2.,
            down(-1.),
            -1.,
            up(-1.),
            -f32::MIN_POSITIVE,
            -sub,
            -0.,
            0.,
            sub,
            f32::MIN_POSITIVE,
            down(1.),
            1.,
            up(1.),
            2.,
            1e30,
        ];
        let hfs = [[0., 0.], [sub, f32::MIN_POSITIVE], [0.01, 0.03], [1., 1.], [1e20, 1e20]];
        for parent in [
            Matrix::identity(),
            Matrix::new(f32::MIN_POSITIVE, -f32::MIN_POSITIVE, up(1.), -1., 1., down(-1.), 0., 0., 1.),
        ] {
            for interval in endpoints.windows(2) {
                verify_range(&parent, [interval[0], interval[1]], [-sub, sub], &hfs);
                verify_range(&parent, [-sub, sub], [interval[0], interval[1]], &hfs);
            }
        }
        let side = Side::new(&Matrix::identity()).unwrap();
        assert_eq!(side.outside_range(-0., 0., -0., 0., &[[0., 0.]]), Some(false));
        assert_eq!(side.outside_range(0., 0., 10., 10., &[[sub, sub]]), Some(true));
        assert_eq!(side.outside_range(0., 0., 10., 10., &[]), None);
        assert_eq!(side.outside_range(0., 0., 10., 10., &[[0., 0.]; 7]), None);
        assert_eq!(side.outside_range(1., -1., 0., 0., &[[0., 0.]]), None);
        assert_eq!(side.outside_range(0., 0., 1., -1., &[[0., 0.]]), None);
        assert_eq!(side.outside_range(0., 0., 0., 0., &[[-1., 0.]]), None);
        assert_eq!(side.outside_range(0., 0., 0., 0., &[[f32::MAX, 0.]]), None);
        assert_eq!(side.outside_range(0., f32::MAX, 0., 0., &[[0., 0.]]), None);
        let large = Side::new(&Matrix::new(1e30, 0., 0., 0., 1., 0., 0., 0., 1.)).unwrap();
        assert_eq!(large.outside_range(0., 1e30, 0., 0., &[[0., 0.]]), None);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for (x, base) in [
                ([invalid, 0.], [0., 0.]),
                ([0., invalid], [0., 0.]),
                ([0., 0.], [invalid, 0.]),
                ([0., 0.], [0., invalid]),
            ] {
                assert_eq!(side.outside_range(x[0], x[1], base[0], base[1], &[[0., 0.]]), None);
            }
            assert_eq!(side.outside_range(0., 0., 0., 0., &[[invalid, 0.]]), None);
        }
    }

    #[test]
    fn range_proposals_enclose_explicit_fma_and_daz_ftz_corner_oracles() {
        fn flush(value: f32) -> f32 {
            if value.is_subnormal() {
                0f32.copysign(value)
            } else {
                value
            }
        }
        let mut random = 193u32;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            (random >> 8) as f32 / 16_777_216. * 32. - 16.
        };
        let hfs = [[0.125, 0.25], [f32::from_bits(1), f32::MIN_POSITIVE / 2.], [f32::MIN_POSITIVE, 0.5]];
        let mut proposals = 0;
        for round in 0..2000 {
            let mut parent = Matrix::new(next(), next(), next(), next(), next(), next(), 0., 0., 1.);
            if round % 7 == 0 {
                parent[(0, 0)] = f32::MIN_POSITIVE;
                parent[(1, 1)] = -f32::MIN_POSITIVE / 2.;
            }
            let x = if round % 11 == 0 {
                [-f32::MIN_POSITIVE, f32::MIN_POSITIVE]
            } else {
                let lo = next();
                [lo, lo + 0.5]
            };
            let base = if round % 13 == 0 {
                [-f32::from_bits(1), f32::from_bits(1)]
            } else {
                let lo = next();
                [lo, lo + 1.]
            };
            let Some(side) = Side::new(&parent) else {
                continue;
            };
            if side.outside_range(x[0], x[1], base[0], base[1], &hfs) != Some(true) {
                continue;
            }
            proposals += 1;
            for x_sample in samples(x[0], x[1]) {
                for base_sample in samples(base[0], base[1]) {
                    for hf in hfs {
                        for mode in 0..3 {
                            let half = if mode == 2 { hf.map(flush) } else { hf };
                            let left = -half[0];
                            let top = -half[1];
                            let right = if mode == 2 {
                                flush(left + flush(half[0] * 2.))
                            } else {
                                half[0].mul_add(2., left)
                            };
                            let bottom = if mode == 2 {
                                flush(top + flush(half[1] * 2.))
                            } else {
                                half[1].mul_add(2., top)
                            };
                            let mut points = [[0f32; 2]; 4];
                            for row in 0..2 {
                                let (mut a, mut b, mut c) = (parent[(row, 0)], parent[(row, 1)], parent[(row, 2)]);
                                let (mut tx, mut ty) = (0. + x_sample, 0. + base_sample);
                                if mode == 2 {
                                    (a, b, c) = (flush(a), flush(b), flush(c));
                                    (tx, ty) = (flush(tx), flush(ty));
                                }
                                let translation = match mode {
                                    0 => a.mul_add(tx, b.mul_add(ty, c)),
                                    1 => b.mul_add(ty, a.mul_add(tx, c)),
                                    _ => flush(flush(flush(a * tx) + flush(b * ty)) + c),
                                };
                                for (index, (qx, qy)) in [(left, top), (right, top), (right, bottom), (left, bottom)].into_iter().enumerate() {
                                    points[index][row] = match mode {
                                        0 => a.mul_add(qx, b.mul_add(qy, translation)),
                                        1 => b.mul_add(qy, a.mul_add(qx, translation)),
                                        _ => flush(flush(flush(a * qx) + flush(b * qy)) + translation),
                                    };
                                }
                            }
                            let rejected = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) > 1.
                                || points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) < -1.
                                || points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) > 1.
                                || points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) < -1.;
                            assert!(rejected, "range proposal admitted an FMA/DAZ/FTZ-visible quad: mode={mode}, parent={parent:?}, x={x_sample}, base={base_sample}, half={hf:?}");
                        }
                    }
                }
            }
        }
        assert!(proposals > 500, "range explicit-machine oracle check was vacuous: {proposals}");
    }
}
