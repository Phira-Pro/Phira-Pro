//! Phigros 9th-chapter "block area" (touch-blocking zones) support.
//!
//! Field layout and the transform/judgement math are reverse-engineered from the
//! official client (`PreviewBlockControl` / `JudgeControl` / `BlockArea`); the
//! semantics below match the game:
//!   * the zone is a rectangle expressed in screen percentages, transformed by
//!     rotate / move / scale events, each of which happens *around its own anchor*
//!     (so the anchor point stays put while the rect grows / turns);
//!   * `IsTimeValid(t) = appearTime <= t < disappearTime`,
//!     `IsActive(t) = enableTime <= t < disableTime`;
//!   * a touch inside an active zone is *removed from the touch list*, so notes
//!     under it simply fail to be hit (they miss).

use super::{Matrix, Point, Vector};
use nalgebra::Rotation2;

#[derive(Debug, Clone, Copy)]
pub struct BlockRotateEvent {
    pub anchor: Vector,
    pub time: f64,
    pub ease: i32,
    pub rotation: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockMoveEvent {
    pub end: Vector,
    pub time: f64,
    pub ease_x: i32,
    pub ease_y: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockScaleEvent {
    pub anchor: Vector,
    pub time: f64,
    pub ease_x: i32,
    pub ease_y: i32,
    pub scale: Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPhase {
    /// Not shown at all.
    Hidden,
    /// Shown (dimmed) but does not block touches.
    Disabled,
    /// Blocks touches.
    Active,
}

/// A block area plus its animated transform, resolved at a given time.
pub struct BlockArea {
    pub top_right: Vector,
    pub bottom_left: Vector,
    pub appear_time: f64,
    pub enable_time: f64,
    pub disable_time: f64,
    pub disappear_time: f64,
    pub is_subtract: bool,
    pub rotate_events: Vec<BlockRotateEvent>,
    pub move_events: Vec<BlockMoveEvent>,
    pub scale_events: Vec<BlockScaleEvent>,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockTransform {
    pub center: Vector,
    pub size: Vector,
    /// Degrees, counter-clockwise.
    pub rotation: f32,
}

/// Progress of the official block easing enum (0..=14):
/// `0` Linear, `1..=12` In/Out/InOut Quad..Quint, `13` HoldStart, `14` JumpToEnd.
fn eased_progress(ease: i32, x: f64) -> f32 {
    let u = (x as f32).clamp(0., 1.);
    match ease {
        0 => u,
        1..=12 => {
            let power = (ease - 1) / 3 + 2;
            match (ease - 1) % 3 {
                0 => u.powi(power),
                1 => 1. - (1. - u).powi(power),
                _ => {
                    if u < 0.5 {
                        (1 << (power - 1)) as f32 * u.powi(power)
                    } else {
                        1. - (2. - 2. * u).powi(power) / 2.
                    }
                }
            }
        }
        13 => 0.,
        14 => 1.,
        _ => u,
    }
}

/// `last index whose time <= t`, assuming `times` is ascending (with a step, the
/// official `FindCurrentEventIndex` returns `-1` when `t` is before the first).
fn last_index<I: Iterator<Item = f64>>(times: I, t: f64) -> Option<usize> {
    let mut idx = None;
    for (i, time) in times.enumerate() {
        if time <= t {
            idx = Some(i);
        } else {
            break;
        }
    }
    idx
}

/// Map a screen percentage into the chart coordinate space (x right, y up,
/// full screen = `[-1, 1] x [-1/aspect, 1/aspect]`).
#[inline]
fn pct_to_chart(p: Vector, aspect: f32) -> Vector {
    Vector::new(2. * p.x - 1., (2. * p.y - 1.) / aspect)
}

impl BlockArea {
    pub fn phase(&self, t: f64) -> BlockPhase {
        if t < self.appear_time || t >= self.disappear_time {
            BlockPhase::Hidden
        } else if t >= self.enable_time && t < self.disable_time {
            BlockPhase::Active
        } else {
            BlockPhase::Disabled
        }
    }

    pub fn is_active(&self, t: f64) -> bool {
        self.enable_time <= t && t < self.disable_time
    }

    fn scale_at(&self, t: f64) -> Vector {
        let ev = &self.scale_events;
        if ev.is_empty() {
            return Vector::new(1., 1.);
        }
        match last_index(ev.iter().map(|e| e.time), t) {
            None => ev[0].scale,
            Some(i) if i + 1 >= ev.len() => ev[i].scale,
            Some(i) => {
                let cur = &ev[i];
                let next = &ev[i + 1];
                let px = eased_progress(cur.ease_x, norm(cur.time, next.time, t));
                let py = eased_progress(cur.ease_y, norm(cur.time, next.time, t));
                Vector::new(
                    cur.scale.x + px * (next.scale.x - cur.scale.x),
                    cur.scale.y + py * (next.scale.y - cur.scale.y),
                )
            }
        }
    }

    fn rotation_at(&self, t: f64) -> f32 {
        let ev = &self.rotate_events;
        if ev.is_empty() {
            return 0.;
        }
        match last_index(ev.iter().map(|e| e.time), t) {
            None => ev[0].rotation,
            Some(i) if i + 1 >= ev.len() => ev[i].rotation,
            Some(i) => {
                let cur = &ev[i];
                let next = &ev[i + 1];
                let p = eased_progress(cur.ease, norm(cur.time, next.time, t));
                cur.rotation + p * (next.rotation - cur.rotation)
            }
        }
    }

    /// Move events *replace* the base center; the animation interpolates the
    /// `endPosition` between keyframes. Before the first event there is no move.
    fn move_center(&self, t: f64, aspect: f32) -> Option<Vector> {
        let ev = &self.move_events;
        if ev.is_empty() {
            return None;
        }
        let p = match last_index(ev.iter().map(|e| e.time), t) {
            None => return Some(pct_to_chart(ev[0].end, aspect)),
            Some(i) if i + 1 >= ev.len() => ev[i].end,
            Some(i) => {
                let cur = &ev[i];
                let next = &ev[i + 1];
                let px = eased_progress(cur.ease_x, norm(cur.time, next.time, t));
                let py = eased_progress(cur.ease_y, norm(cur.time, next.time, t));
                Vector::new(cur.end.x + px * (next.end.x - cur.end.x), cur.end.y + py * (next.end.y - cur.end.y))
            }
        };
        Some(pct_to_chart(p, aspect))
    }

    /// Anchor of the *current* scale keyframe (held until the next one).
    fn scale_anchor(&self, t: f64, aspect: f32) -> Option<Vector> {
        let ev = &self.scale_events;
        if ev.is_empty() {
            return None;
        }
        let i = last_index(ev.iter().map(|e| e.time), t).unwrap_or(0);
        Some(pct_to_chart(ev[i].anchor, aspect))
    }

    /// Anchor of the *current* rotate keyframe (held until the next one).
    fn rotate_anchor(&self, t: f64, aspect: f32) -> Option<Vector> {
        let ev = &self.rotate_events;
        if ev.is_empty() {
            return None;
        }
        let i = last_index(ev.iter().map(|e| e.time), t).unwrap_or(0);
        Some(pct_to_chart(ev[i].anchor, aspect))
    }

    /// Resolve the animated rect at `t`.
    ///
    /// The shape is scaled then rotated about its own centre. A move track
    /// *assigns* the absolute centre (overriding any anchor offset); without one
    /// the centre is the anchored scale/rotation of the initial centre:
    /// `C' = Ar + R(θ)·(As + S(C - As) - Ar)`.
    pub fn transform(&self, t: f64, aspect: f32) -> BlockTransform {
        let bl = pct_to_chart(self.bottom_left, aspect);
        let tr = pct_to_chart(self.top_right, aspect);
        let base_size = tr - bl;
        let c = (bl + tr) * 0.5;

        let scale = self.scale_at(t);
        let rotation = self.rotation_at(t);
        let r = Rotation2::new(rotation.to_radians());

        let center = if let Some(m) = self.move_center(t, aspect) {
            m
        } else {
            let as_ = self.scale_anchor(t, aspect).unwrap_or(c);
            let ar = self.rotate_anchor(t, aspect).unwrap_or(c);
            let s = Vector::new(scale.x * (c.x - as_.x), scale.y * (c.y - as_.y));
            ar + r * (as_ + s - ar)
        };

        BlockTransform {
            center,
            size: Vector::new(base_size.x * scale.x, base_size.y * scale.y),
            rotation,
        }
    }

    /// Model matrix mapping the unit quad (`[-0.5, 0.5]²`) onto the current rect,
    /// in chart space (to be used inside the chart's model transform).
    pub fn matrix(&self, t: f64, aspect: f32) -> Matrix {
        matrix_of(&self.transform(t, aspect))
    }

    /// Whether `p` (chart space) lies inside the block. `inset_world` is a chart
    /// space margin (see [`touch_inset_world`]); the test area shrinks for normal
    /// blocks and expands for subtract ones, matching the official inset logic.
    pub fn contains(&self, p: Vector, t: f64, aspect: f32, inset_world: f32) -> bool {
        if self.phase(t) == BlockPhase::Hidden {
            return false;
        }
        let tr = self.transform(t, aspect);
        let Some(inv) = matrix_of(&tr).try_inverse() else {
            return false;
        };
        let lp: Point = inv.transform_point(&Point::new(p.x, p.y));
        let sign = if self.is_subtract { 1. } else { -1. };
        let hx = 0.5 + sign * inset_local(tr.size.x.abs(), inset_world);
        let hy = 0.5 + sign * inset_local(tr.size.y.abs(), inset_world);
        lp.x.abs() <= hx && lp.y.abs() <= hy
    }
}

fn matrix_of(tr: &BlockTransform) -> Matrix {
    Matrix::new_translation(&tr.center)
        * Rotation2::new(tr.rotation.to_radians()).to_homogeneous()
        * Matrix::identity().append_nonuniform_scaling(&tr.size)
}

/// Official `JudgeControl.maxBlockTouchInsetLocal`.
pub const TOUCH_INSET_LOCAL: f32 = 0.05;
/// Official `JudgeControl.blockTouchInsetScreenHeightRatio` (also the clamp bound).
pub const TOUCH_INSET_SCREEN_HEIGHT_RATIO: f32 = 0.25;

/// Chart-space touch inset: `maxBlockTouchInsetLocal * screenHeight`
/// (screen height in chart units is `2 / aspect`).
#[inline]
pub fn touch_inset_world(aspect: f32) -> f32 {
    TOUCH_INSET_LOCAL * 2.0 / aspect
}

/// Convert a chart-space inset into local units for one axis (clamped, as the
/// official `TryGetBlockTouchHalfSize` does).
#[inline]
fn inset_local(size: f32, inset_world: f32) -> f32 {
    if size.abs() < 1e-6 {
        TOUCH_INSET_SCREEN_HEIGHT_RATIO
    } else {
        (inset_world / size).abs().clamp(0., TOUCH_INSET_SCREEN_HEIGHT_RATIO)
    }
}

/// Even-odd rule equivalent to the official `JudgeControl.TryGetBlockingBlock`:
/// a point is blocked iff `anyNormal XOR (subtractCount odd)` holds for both the
/// full rect and the inset rect.
pub fn block_touch_blocked(areas: &[BlockArea], p: Vector, t: f64, aspect: f32) -> bool {
    let inset = touch_inset_world(aspect);
    let mut orig_non = false;
    let mut orig_sub = 0u32;
    let mut ins_non = false;
    let mut ins_sub = 0u32;
    for b in areas {
        if !b.is_active(t) {
            continue;
        }
        if b.contains(p, t, aspect, 0.) {
            if b.is_subtract {
                orig_sub += 1;
            } else {
                orig_non = true;
            }
        }
        if b.contains(p, t, aspect, inset) {
            if b.is_subtract {
                ins_sub += 1;
            } else {
                ins_non = true;
            }
        }
    }
    (orig_non as u32) != (orig_sub & 1) && (ins_non as u32) != (ins_sub & 1)
}

#[inline]
fn norm(cur: f64, next: f64, t: f64) -> f64 {
    if next <= cur {
        1.
    } else {
        (t - cur) / (next - cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blk(
        tr: (f32, f32),
        bl: (f32, f32),
        move_events: Vec<BlockMoveEvent>,
        scale_events: Vec<BlockScaleEvent>,
        rotate_events: Vec<BlockRotateEvent>,
    ) -> BlockArea {
        BlockArea {
            top_right: Vector::new(tr.0, tr.1),
            bottom_left: Vector::new(bl.0, bl.1),
            appear_time: 0.,
            enable_time: 0.,
            disable_time: 100.,
            disappear_time: 100.,
            is_subtract: false,
            rotate_events,
            move_events,
            scale_events,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// Official sample regression: with a move track the centre is the absolute
    /// move target and the geometry is scaled then rotated about it
    /// (`corner = M + R(θ)·S·(P−C)`), easing per the official enum.
    #[test]
    fn matches_official_move_scale_rotate() {
        let aspect = 16.0f32 / 9.0;
        let b = BlockArea {
            top_right: Vector::new(0.6, 0.7),
            bottom_left: Vector::new(0.4, 0.3),
            appear_time: 0.,
            enable_time: 1.,
            disable_time: 3.,
            disappear_time: 4.,
            is_subtract: false,
            rotate_events: vec![
                BlockRotateEvent { anchor: Vector::new(0.6, 0.4), time: 0., ease: 0, rotation: 0. },
                BlockRotateEvent { anchor: Vector::new(0.6, 0.4), time: 3., ease: 0, rotation: 90. },
            ],
            move_events: vec![
                BlockMoveEvent { end: Vector::new(0.5, 0.5), time: 0., ease_x: 1, ease_y: 2 },
                BlockMoveEvent { end: Vector::new(0.75, 0.6), time: 4., ease_x: 0, ease_y: 0 },
            ],
            scale_events: vec![
                BlockScaleEvent { anchor: Vector::new(0.25, 0.75), time: 0., ease_x: 4, ease_y: 5, scale: Vector::new(1., 1.) },
                BlockScaleEvent { anchor: Vector::new(0.25, 0.75), time: 2., ease_x: 0, ease_y: 0, scale: Vector::new(2., 0.5) },
            ],
        };
        for &t in &[0.0f64, 0.3, 1.5, 2.0, 2.75, 3.0, 3.99] {
            let u = (t / 4.) as f32;
            let v = ((t / 2.) as f32).min(1.);
            let cx = 480. * u * u;
            let cy = 108. * (1. - (1. - u) * (1. - u));
            let sx = 1. + v * v * v;
            let sy = 1. - 0.5 * (1. - (1. - v).powi(3));
            let ang = ((t / 3.) as f32).min(1.) * std::f32::consts::FRAC_PI_2;
            let rx = sx * -192.;
            let ry = sy * -216.;
            let wx = cx + rx * ang.cos() - ry * ang.sin();
            let wy = cy + rx * ang.sin() + ry * ang.cos();
            let p = b.matrix(t, aspect).transform_point(&nalgebra::Point2::new(-0.5, -0.5));
            assert!(
                (p.x * 960. - wx).abs() < 0.1 && (p.y * 960. - wy).abs() < 0.1,
                "t={t} got ({},{}) want ({wx},{wy})",
                p.x * 960.,
                p.y * 960.
            );
        }
    }

    /// `HoldStart` (13) holds the outgoing key, `JumpToEnd` (14) jumps to it.
    #[test]
    fn official_hold_and_jump_easings() {
        assert!((eased_progress(13, 0.3) - 0.).abs() < 1e-6);
        assert!((eased_progress(14, 0.3) - 1.).abs() < 1e-6);
        assert!((eased_progress(0, 0.3) - 0.3).abs() < 1e-6);
        assert!((eased_progress(1, 0.5) - 0.25).abs() < 1e-6);
        assert!((eased_progress(4, 0.5) - 0.125).abs() < 1e-6);
    }

    /// A centered 4%x4% block maps to the chart origin with the right size.
    #[test]
    fn centered_block_maps_to_origin() {
        let b = blk((0.52, 0.52), (0.48, 0.48), vec![], vec![], vec![]);
        let tr = b.transform(50., 2.0);
        assert!(close(tr.center.x, 0.) && close(tr.center.y, 0.), "{tr:?}");
        assert!(close(tr.size.x, 0.08) && close(tr.size.y, 0.04), "{tr:?}");
        assert!(close(tr.rotation, 0.));
        assert!(b.contains(Vector::new(0., 0.), 50., 2.0, 0.));
        assert!(!b.contains(Vector::new(0.5, 0.), 50., 2.0, 0.));
    }

    /// Move events replace the base center (`pct=1.0` -> `chart x = 1`).
    #[test]
    fn move_event_replaces_center() {
        let b = blk(
            (0.52, 0.52),
            (0.48, 0.48),
            vec![BlockMoveEvent {
                end: Vector::new(1.0, 0.5),
                time: 0.,
                ease_x: 0,
                ease_y: 0,
            }],
            vec![],
            vec![],
        );
        let tr = b.transform(1., 2.0);
        assert!(close(tr.center.x, 1.) && close(tr.center.y, 0.), "{tr:?}");
    }

    /// Rotation happens around the event anchor, so the center orbits it.
    #[test]
    fn rotation_orbits_anchor() {
        let ev = vec![
            BlockRotateEvent {
                anchor: Vector::new(1.0, 0.5),
                time: 0.,
                ease: 0,
                rotation: 0.,
            },
            BlockRotateEvent {
                anchor: Vector::new(1.0, 0.5),
                time: 10.,
                ease: 0,
                rotation: 90.,
            },
        ];
        let b = blk((0.52, 0.52), (0.48, 0.48), vec![], vec![], ev);
        let tr = b.transform(10., 2.0);
        assert!(close(tr.rotation, 90.), "{tr:?}");
        assert!(close(tr.center.x, 1.) && close(tr.center.y, -1.), "{tr:?}");
    }

    /// Even-odd rule: a lone subtract zone blocks, but a subtract zone punches a
    /// hole through a normal one.
    #[test]
    fn even_odd_blocking() {
        let aspect = 2.0;
        let p = Vector::new(0., 0.);
        let mk = |sub: bool| {
            let mut b = blk((0.52, 0.52), (0.48, 0.48), vec![], vec![], vec![]);
            b.is_subtract = sub;
            b
        };
        assert!(block_touch_blocked(&[mk(false)], p, 50., aspect));
        assert!(block_touch_blocked(&[mk(true)], p, 50., aspect));
        assert!(!block_touch_blocked(&[mk(false), mk(true)], p, 50., aspect));
    }
}
