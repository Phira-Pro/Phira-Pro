//! CPU raster source for the official touch camera, before ActiveBlock's SDF.
//!
//! Source: Round10_Blur4 sprite from sharedassets12.assets, 44x44 pixels,
//! PPU=100, bilinear/clamp; prefab size=11.5, camera orthographic size=5.
//! Native Show is zero -> size and Hide is current -> zero in 0.1 seconds,
//! with linear clamped interpolation (Animation.MoveNext RVA 0x1D708D0).
//! The destination touch camera is point-filtered R8 at mask resolution.

use macroquad::prelude::Vec2;

const MAX_TOUCHES: usize = 10;
const DURATION: f32 = 0.1;
const SPRITE_HEIGHT: f32 = 44.0 / 100.0 * 11.5 / 10.0;

#[derive(Clone, Copy, Default)]
struct Slot {
    finger: Option<u64>,
    seen: bool,
    center: Vec2,
    start_time: f32,
    start_scale: f32,
    end_scale: f32,
    scale: f32,
}

impl Slot {
    fn advance(&mut self, time: f32) {
        let t = ((time - self.start_time) / DURATION).clamp(0.0, 1.0);
        self.scale = self.start_scale + (self.end_scale - self.start_scale) * t;
    }

    fn show(&mut self, finger: u64, center: Vec2, time: f32) {
        // Native Show restarts from Vector3.zero even if a disappearing slot
        // is reused before Hide's coroutine has completed.
        self.finger = Some(finger);
        self.center = center;
        self.seen = true;
        self.start_time = time;
        self.start_scale = 0.0;
        self.end_scale = 1.0;
        self.scale = 0.0;
    }

    fn hide(&mut self, time: f32) {
        self.finger = None;
        self.start_time = time;
        self.start_scale = self.scale;
        self.end_scale = 0.0;
    }
}

pub(super) struct TouchMask {
    slots: [Slot; MAX_TOUCHES],
    source: Vec<u8>,
    width: usize,
    height: usize,
    last_time: Option<f32>,
}

impl Default for TouchMask {
    fn default() -> Self {
        let source = image::load_from_memory(include_bytes!("../../../assets/blockarea/TouchHover.png"))
            .expect("official touch hover source")
            .to_rgba8();
        Self {
            slots: [Slot::default(); MAX_TOUCHES],
            width: source.width() as usize,
            height: source.height() as usize,
            source: source.into_raw(),
            last_time: None,
        }
    }
}

impl TouchMask {
    pub fn reset(&mut self) {
        self.slots.fill(Slot::default());
        self.last_time = None;
    }

    /// Positions use bottom-left screen UV, before chart-local flips.
    /// Preserve finger IDs: the judgement input originates in a HashMap and
    /// its iteration order can change between otherwise identical frames.
    pub fn update_fingers(&mut self, touches: &[(u64, Vec2)], time: f32) {
        if !time.is_finite() {
            return;
        }
        if self.last_time.is_some_and(|previous| time < previous) {
            // A deterministic replay may seek backwards. Native instances
            // are reset on initialization rather than retaining future scales.
            self.slots.fill(Slot::default());
        }
        self.last_time = Some(time);
        for slot in &mut self.slots {
            slot.advance(time);
            slot.seen = false;
        }
        for &(finger, center) in touches.iter().take(MAX_TOUCHES) {
            if !center.x.is_finite() || !center.y.is_finite() {
                continue;
            }
            if let Some(slot) = self.slots.iter_mut().find(|slot| slot.finger == Some(finger)) {
                slot.center = center;
                slot.seen = true;
            } else if let Some(slot) = self.slots.iter_mut().find(|slot| slot.finger.is_none()) {
                slot.show(finger, center, time);
            }
        }
        for slot in &mut self.slots {
            if slot.finger.is_some() && !slot.seen {
                slot.hide(time);
            }
        }
    }

    /// Convenience for deterministic probes whose list has stable order.
    #[cfg(test)]
    pub fn update_positions(&mut self, touches: &[Vec2], time: f32) {
        let fingers: Vec<_> = touches.iter().copied().enumerate().map(|(index, point)| (index as u64, point)).collect();
        self.update_fingers(&fingers, time);
    }

    pub fn visible(&self) -> bool {
        self.slots.iter().any(|slot| slot.finger.is_some() || slot.scale > 0.0)
    }

    /// Union of source sprite footprints in screen UV. Pixel evaluation outside
    /// this region always returns zero and need not scan ten finger slots.
    pub fn bounds(&self, aspect: f32) -> Option<(Vec2, Vec2)> {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for slot in &self.slots {
            if slot.scale <= 0. {
                continue;
            }
            let size = SPRITE_HEIGHT * slot.scale * 0.5;
            let half = Vec2::new(size / aspect, size);
            min = min.min(slot.center - half);
            max = max.max(slot.center + half);
        }
        if min.x.is_finite() {
            Some((min.max(Vec2::ZERO), max.min(Vec2::ONE)))
        } else {
            None
        }
    }

    /// R8 value at one native touch-camera pixel center. The caller duplicates
    /// each value to 2x2 pixels when packing it in the effect-sized aux texture.
    pub fn sample(&self, uv: Vec2, aspect: f32) -> u8 {
        let mut result = 0.0_f32;
        for slot in &self.slots {
            if slot.scale <= 0.0 {
                continue;
            }
            let size = SPRITE_HEIGHT * slot.scale;
            let source_uv = Vec2::new((uv.x - slot.center.x) * aspect / size + 0.5, (uv.y - slot.center.y) / size + 0.5);
            if source_uv.x < 0.0 || source_uv.x > 1.0 || source_uv.y < 0.0 || source_uv.y > 1.0 {
                continue;
            }
            let (red, alpha) = self.source_sample(source_uv);
            // Sprites/Default has One, OneMinusSrcAlpha and multiplies RGB by
            // texture alpha. Quantize after each blend, as the R8 RT does.
            result = ((red * alpha + result * (1.0 - alpha)) * 255.0).round().clamp(0.0, 255.0) / 255.0;
        }
        (result * 255.0).round() as u8
    }

    fn source_sample(&self, uv: Vec2) -> (f32, f32) {
        let x = uv.x * self.width as f32 - 0.5;
        // Decoded PNG rows are top-down, native sprite texture UV is Y-up.
        let y = (1.0 - uv.y) * self.height as f32 - 0.5;
        let x0 = x.floor() as isize;
        let y0 = y.floor() as isize;
        let fx = x - x.floor();
        let fy = y - y.floor();
        let pixel = |x: isize, y: isize, channel: usize| {
            let x = x.clamp(0, self.width as isize - 1) as usize;
            let y = y.clamp(0, self.height as isize - 1) as usize;
            self.source[(y * self.width + x) * 4 + channel] as f32 / 255.0
        };
        let bilinear = |channel| {
            let bottom = pixel(x0, y0, channel) * (1.0 - fx) + pixel(x0 + 1, y0, channel) * fx;
            let top = pixel(x0, y0 + 1, channel) * (1.0 - fx) + pixel(x0 + 1, y0 + 1, channel) * fx;
            bottom * (1.0 - fy) + top * fy
        };
        (bilinear(0), bilinear(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_bitmap_show_and_hide_use_native_scale() {
        let mut touch = TouchMask::default();
        let center = Vec2::new(0.5, 0.5);
        let edge = center + Vec2::new(0.0, 0.2);
        touch.update_fingers(&[(42, center)], 0.0);
        assert_eq!(touch.sample(center, 16.0 / 9.0), 0);
        touch.update_fingers(&[(42, center)], 0.05);
        assert_eq!(touch.sample(edge, 16.0 / 9.0), 0);
        touch.update_fingers(&[(42, center)], 0.1);
        assert!(touch.sample(edge, 16.0 / 9.0) > 0);
        assert_eq!(touch.sample(center, 16.0 / 9.0), 251);
        touch.update_fingers(&[], 0.1);
        touch.update_fingers(&[], 0.15);
        assert_eq!(touch.sample(edge, 16.0 / 9.0), 0);
        touch.update_fingers(&[], 0.201);
        assert_eq!(touch.sample(center, 16.0 / 9.0), 0);
        assert!(!touch.visible());
    }

    #[test]
    fn finger_reordering_does_not_restart_show_and_overlap_is_source_over() {
        let mut touch = TouchMask::default();
        let center = Vec2::new(0.5, 0.5);
        touch.update_fingers(&[(1, center), (2, center)], 1.0);
        touch.update_fingers(&[(2, center), (1, center)], 1.1);
        assert_eq!(touch.sample(center, 1.0), 255);
        assert_eq!(touch.slots[0].start_time, 1.0);
        assert_eq!(touch.slots[1].start_time, 1.0);
    }

    #[test]
    fn disappearing_slots_restart_from_zero_when_reused() {
        let mut touch = TouchMask::default();
        let center = Vec2::new(0.5, 0.5);
        touch.update_fingers(&[(1, center)], 0.0);
        touch.update_fingers(&[(1, center)], 0.1);
        touch.update_fingers(&[], 0.1);
        touch.update_fingers(&[(2, center)], 0.15);
        assert_eq!(touch.sample(center, 1.0), 0);
        touch.update_fingers(&[(2, center)], 0.25);
        assert_eq!(touch.sample(center, 1.0), 251);
    }
}
