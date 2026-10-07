//! Color ownership is independent of coverage: tint never changes subtraction,
//! input, opacity or the native edge/glow masks. Extend the nearest rectangle's
//! RGB from visible composed regions into holes and outside raw boundaries so
//! displaced fills and glow cannot pick up a canceled rectangle's tint.
use super::{
    mask::{raster_rows, Masks},
    Zone, DEFAULT_BLOCK_COLOR,
};

#[derive(Default)]
pub(super) struct Colors {
    pub rgba: [Vec<u8>; 2],
    pub width: usize,
    pub height: usize,
    pub revision: u64,
    aspect: f32,
    zones: Vec<Zone>,
    seeds: Vec<usize>,
    owners: Vec<usize>,
}

impl Colors {
    pub fn update(&mut self, masks: &Masks, aspect: f32, zones: &[Zone]) -> bool {
        let (width, height) = (masks.width, masks.height);
        let colored = zones.iter().any(|z| z.opacity > 0. && z.color != DEFAULT_BLOCK_COLOR);
        if !colored {
            return false;
        }
        if (self.width, self.height, self.aspect) == (width, height, aspect) && self.zones == zones {
            return true;
        }
        self.width = width;
        self.height = height;
        self.aspect = aspect;
        self.zones.clear();
        self.zones.extend_from_slice(zones);
        self.seeds.resize(width * height, usize::MAX);
        self.owners.resize(width * height, usize::MAX);
        for layer in 0..2 {
            self.seeds.fill(usize::MAX);
            self.owners.fill(usize::MAX);
            // Only the side surviving native composition owns color. Canceled
            // rectangles carve holes without seeding their RGB into the rim.
            // Within the visible kind, later z-order wins.
            for invert in [true, false] {
                for (id, z) in zones
                    .iter()
                    .enumerate()
                    .filter(|(_, z)| z.active == (layer == 0) && z.opacity > 0. && z.invert == invert)
                {
                    let (seeds, owners) = (&mut self.seeds, &mut self.owners);
                    raster_rows(width, height, aspect, z, |y, first, last| {
                        for i in y * width + first..y * width + last {
                            if masks.color_source(layer, i) == Some(invert) {
                                seeds[i] = i;
                                owners[i] = id;
                            }
                        }
                    });
                }
            }
            // Two chamfer sweeps propagate nearest seeds in O(pixels), rather
            // than testing every zone at every glow pixel.
            for reverse in [false, true] {
                for n in 0..width * height {
                    let i = if reverse { width * height - 1 - n } else { n };
                    if self.seeds[i] == i {
                        continue;
                    }
                    let (x, y) = (i % width, i / width);
                    let distance = |seed: usize| -> usize {
                        if seed == usize::MAX {
                            usize::MAX
                        } else {
                            x.abs_diff(seed % width).pow(2) + y.abs_diff(seed / width).pow(2)
                        }
                    };
                    let mut best = self.seeds[i];
                    let mut dist = distance(best);
                    let dy = if reverse { 1_isize } else { -1 };
                    for (dx, dy) in [(dy, 0), (-1, dy), (0, dy), (1, dy)] {
                        let (nx, ny) = (x as isize + dx, y as isize + dy);
                        if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                            continue;
                        }
                        let seed = self.seeds[ny as usize * width + nx as usize];
                        let d = distance(seed);
                        if d < dist {
                            best = seed;
                            dist = d;
                        }
                    }
                    self.seeds[i] = best;
                }
            }
            let bytes = &mut self.rgba[layer];
            bytes.resize(width * height * 4, 0);
            for (pixel, &seed) in bytes.chunks_exact_mut(4).zip(&self.seeds) {
                let rgb = if seed == usize::MAX {
                    DEFAULT_BLOCK_COLOR
                } else {
                    zones[self.owners[seed]].color
                };
                for (dst, c) in pixel[..3].iter_mut().zip(rgb) {
                    *dst = (c.clamp(0., 1.) * 255.).round() as u8;
                }
                pixel[3] = 255;
            }
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Vector;

    fn zone(x: f32, color: [f32; 3], active: bool, invert: bool) -> Zone {
        Zone {
            center: Vector::new(x, 0.),
            half: Vector::new(0.2, 0.2),
            angle: 0.,
            color,
            active,
            invert,
            ready: !active,
            opacity: 1.,
        }
    }

    fn update(colors: &mut Colors, width: usize, height: usize, aspect: f32, zones: &[Zone]) -> bool {
        let mut masks = Masks::default();
        masks.render_displaced(width * 4, height * 4, aspect, zones, 1.);
        colors.update(&masks, aspect, zones)
    }

    #[test]
    fn independent_colors_extend_to_glow_and_keep_preview_separate() {
        let mut colors = Colors::default();
        let zones = [
            zone(-0.5, [0., 1., 0.], true, false),
            zone(0.5, [0., 0., 1.], true, false),
            zone(0., [1.; 3], false, false),
        ];
        assert!(update(&mut colors, 64, 32, 2., &zones));
        let rgb = |layer: usize, x: usize| &colors.rgba[layer][(16 * 64 + x) * 4..(16 * 64 + x) * 4 + 3];
        assert_eq!(rgb(0, 1), &[0, 255, 0]);
        assert_eq!(rgb(0, 62), &[0, 0, 255]);
        assert_eq!(rgb(1, 32), &[255; 3]);
        let revision = colors.revision;
        assert!(update(&mut colors, 64, 32, 2., &zones));
        assert_eq!(colors.revision, revision);
        let mut changed = zones.clone();
        changed[0].color = [1., 1., 0.];
        update(&mut colors, 64, 32, 2., &changed);
        assert_ne!(colors.revision, revision);
    }

    #[test]
    fn subtract_color_does_not_repaint_normal_fill_and_default_needs_no_texture() {
        let mut colors = Colors::default();
        let mut normal = zone(0., [0., 1., 0.], true, false);
        normal.half = Vector::repeat(0.5);
        let zones = [normal, zone(0., [1.; 3], true, true)];
        update(&mut colors, 32, 32, 1., &zones);
        assert_eq!(&colors.rgba[0][(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 3], &[0, 255, 0]);
        assert!(!update(&mut colors, 32, 32, 1., &[zone(0., DEFAULT_BLOCK_COLOR, true, false)]));
    }

    #[test]
    fn white_inverted_field_keeps_white_inside_canceled_red_holes() {
        for active in [true, false] {
            let mut inverted = zone(0., [1.; 3], active, true);
            inverted.half = Vector::repeat(1.);
            let zones = [inverted, zone(0., DEFAULT_BLOCK_COLOR, active, false)];
            let mut colors = Colors::default();
            update(&mut colors, 64, 32, 2., &zones);
            let layer = usize::from(!active);
            assert!(colors.rgba[layer].chunks_exact(4).all(|p| p == [255; 4]));
        }
    }
}
