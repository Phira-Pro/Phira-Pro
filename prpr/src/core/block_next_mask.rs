//! rpe-next geometry composition only. The resulting coverage is consumed by
//! the existing native noise/displacement/edge/glow materials, never flat RGB.
use super::{raster_rows, Zone};

#[derive(Default)]
pub(super) struct Coverage {
    pub active: Vec<u8>,
    pub disabled: Vec<u8>,
    pub ready: Vec<u8>,
    counts: [Vec<i32>; 2],
}

impl Coverage {
    pub fn raster(&mut self, width: usize, height: usize, aspect: f32, zones: &[Zone], single: bool) {
        let len = width * height;
        let stride = width + 1;
        for plane in [&mut self.active, &mut self.disabled, &mut self.ready] {
            plane.resize(len, 0);
            plane.fill(0);
        }
        for counts in &mut self.counts {
            counts.resize(stride * height, 0);
        }
        for state in 0..3 {
            for counts in &mut self.counts {
                counts.fill(0);
            }
            for z in zones.iter().filter(|z| {
                if state == 2 {
                    z.active
                } else if state == 1 {
                    z.ready && !z.active
                } else {
                    !z.active && (single || !z.ready)
                }
            }) {
                let counts = &mut self.counts[usize::from(z.invert)];
                raster_rows(width, height, aspect, z, |y, start, end| {
                    counts[y * stride + start] += 1;
                    counts[y * stride + end] -= 1;
                });
            }
            for y in 0..height {
                let (mut n, mut s) = (0, 0);
                for x in 0..width {
                    n += self.counts[0][y * stride + x];
                    s += self.counts[1][y * stride + x];
                    if !((n > 0) ^ if single { s == 1 } else { s % 2 == 1 }) {
                        continue;
                    }
                    let i = y * width + x;
                    self.active[i] = if state == 2 { 255 } else { 0 };
                    self.disabled[i] = if state != 2 { 255 } else { 0 };
                    self.ready[i] = if state == 1 { 255 } else { 0 };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{rpe_next_block::contains, Vector};
    fn zone(x: f32, half: f32, invert: bool, state: u8) -> Zone {
        Zone {
            projection: None,
            line2area: true,
            y_scale: 1.,
            color: [1., 84. / 255., 84. / 255.],
            center: Vector::new(x, 0.),
            half: Vector::new(half, 0.3),
            angle: 0.,
            invert,
            active: state == 2,
            ready: state == 1,
            opacity: 1.,
        }
    }
    #[test]
    fn next_native_camera_coverage_matches_continuous_state_groups_and_inversion_options() {
        let (w, h) = (123, 87);
        for aspect in [4. / 3., 16. / 9., 22. / 9.] {
            for single in [false, true] {
                let mut zones = vec![
                    zone(-0.3, 0.5, false, 0),
                    zone(0.1, 0.4, false, 1),
                    zone(0.3, 0.3, true, 1),
                    zone(0.3, 0.2, true, 1),
                    zone(0.3, 0.1, true, 1),
                    zone(0., 0.3, false, 2),
                ];
                for (i, z) in zones.iter_mut().enumerate() {
                    z.y_scale = 1.5 / aspect;
                    z.angle = i as f32 * 0.17;
                }
                let mut masks = Coverage::default();
                masks.raster(w, h, aspect, &zones, single);
                for y in 0..h {
                    for x in 0..w {
                        let p = Vector::new((x as f32 + 0.5) * 2. / w as f32 - 1., ((y as f32 + 0.5) * 2. / h as f32 - 1.) / aspect);
                        let mut highest = None;
                        for state in 0..3 {
                            let (mut n, mut s) = (false, 0);
                            for z in zones.iter().filter(|z| {
                                if state == 2 {
                                    z.active
                                } else if state == 1 {
                                    z.ready && !z.active
                                } else {
                                    !z.active && (single || !z.ready)
                                }
                            }) {
                                if contains(z, p, 0.) {
                                    if z.invert {
                                        s += 1;
                                    } else {
                                        n = true;
                                    }
                                }
                            }
                            if n ^ if single { s == 1 } else { s % 2 == 1 } {
                                highest = Some(state);
                            }
                        }
                        let i = y * w + x;
                        assert_eq!(masks.active[i] != 0, highest == Some(2));
                        assert_eq!(masks.disabled[i] != 0, highest == Some(0) || highest == Some(1));
                        assert_eq!(masks.ready[i] != 0, highest == Some(1));
                    }
                }
            }
        }
    }
    #[test]
    fn next_adjacent_and_overlapping_rectangles_produce_one_native_mask() {
        let (w, h) = (180, 120);
        let mut masks = Coverage::default();
        let zones = [zone(-0.25, 0.25, false, 2), zone(0.25, 0.25, false, 2)];
        masks.raster(w, h, 1.5, &zones, false);
        for y in 45..75 {
            for x in 65..115 {
                assert_eq!(masks.active[y * w + x], 255);
            }
        }
        let reference = masks.active.clone();
        masks.raster(w, h, 1.5, &[zones[0].clone(), zones[1].clone(), zones[0].clone()], false);
        assert_eq!(reference, masks.active);
    }
}
