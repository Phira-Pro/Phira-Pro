//! CPU equivalent of the native mask, EdgeMask and GlowMask passes.
//! Bit rows keep the nine-tap dilation inexpensive without any offscreen FBOs.
use super::Zone;
use once_cell::sync::Lazy;

static DISPLACE: Lazy<image::RgbImage> = Lazy::new(|| {
    let source = image::load_from_memory(include_bytes!("../../../assets/blockarea/BlockNoise1.png"))
        .unwrap()
        .to_rgb8();
    image::imageops::flip_vertical(&source)
});

#[derive(Default)]
pub(super) struct Masks {
    pub rgba: Vec<u8>,
    /// Ready-only normal R, ready-only postprocessed subtract G, disabled compose, hover.
    pub aux_rgba: Vec<u8>,
    /// Point-sampled raw camera outputs, packed enabled N/S and disabled N/S.
    pub sources_rgba: Vec<u8>,
    pub raw_disabled_green: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub revision: u64,
    active: Vec<u64>,
    ping: Vec<u64>,
    pong: Vec<u64>,
    last_dim: (usize, usize, f32),
    last_zones: Vec<Zone>,
    last_time: Option<f32>,
    layers: [Vec<u8>; 6],
    ready_green: Vec<u8>,
    gray_ping: Vec<u8>,
    gray_pong: Vec<u8>,
    warp_x: Vec<(f32, usize, usize)>,
    warp_y: Vec<(f32, usize)>,
    enabled_compose: Vec<u8>,
    uniform_compose: Option<u8>,
    point_aux: Vec<[u8; 3]>,
    coverage_diff: [Vec<i32>; 2],
}

impl Masks {
    pub fn render_displaced(&mut self, width: usize, height: usize, aspect: f32, zones: &[Zone], time: f32) {
        // Refine Compose to the existing EffectRT grid: half-sized boundary
        // cells without larger effect textures, more shader work or narrower
        // edge/glow rings. Native geometry and displacement math stay intact.
        let (bw, bh) = ((width / 8).max(1) * 2, (height / 8).max(1) * 2);
        let (ew, eh) = (bw, bh);
        if self.last_dim == (ew, eh, aspect) && self.last_zones == zones && (self.last_time == Some(time) || self.uniform_compose.is_some()) {
            return;
        }
        if self.last_dim != (ew, eh, aspect) || self.last_zones != zones {
            self.last_dim = (ew, eh, aspect);
            self.last_zones.clear();
            self.last_zones.extend_from_slice(zones);
            for layer in &mut self.layers {
                layer.resize(bw * bh, 0);
                layer.fill(0);
            }
            self.raw_disabled_green.resize(bw * bh, 0);
            self.raw_disabled_green.fill(0);
            self.ready_green.resize(bw * bh, 0);
            self.ready_green.fill(0);
            for diff in &mut self.coverage_diff {
                diff.resize((bw + 1) * bh, 0);
                diff.fill(0);
            }
            // The six official cameras capture enabled N/S, disabled N/S
            // (including ready), and ready-only N/S. BlockSprite blends
            // SrcAlpha, One; each subtract sprite has alpha .1.
            for z in zones {
                let layer = if z.active { usize::from(z.invert) } else { 2 + usize::from(z.invert) };
                let opacity = if z.invert { 0.1 } else { z.opacity };
                if z.active && (z.invert || z.opacity == 1.) {
                    let diff = &mut self.coverage_diff[usize::from(z.invert)];
                    raster_rows(bw, bh, aspect, z, |y, first, last| {
                        diff[y * (bw + 1) + first] += 1;
                        diff[y * (bw + 1) + last] -= 1;
                    });
                    continue;
                }
                raster_rows(bw, bh, aspect, z, |y, first, last| {
                    for x in first..last {
                        let i = y * bw + x;
                        self.layers[layer][i] = unorm(self.layers[layer][i] as f32 / 255. + opacity);
                        if z.invert && !z.active {
                            self.raw_disabled_green[i] = unorm(self.raw_disabled_green[i] as f32 / 255. + 0.1 * z.opacity);
                        }
                        if z.ready {
                            let ready = 4 + usize::from(z.invert);
                            self.layers[ready][i] = unorm(self.layers[ready][i] as f32 / 255. + opacity);
                            if z.invert {
                                self.ready_green[i] = unorm(self.ready_green[i] as f32 / 255. + 0.1 * z.opacity);
                            }
                        }
                    }
                });
            }
            // Prefix sums make heavily overlapping active rectangles cost
            // O(rectangles * rows + pixels), rather than O(sum of their areas).
            // Retain the exact R8 quantization of every native additive blend.
            let mut subtract_values = [0_u8; 11];
            for i in 1..subtract_values.len() {
                subtract_values[i] = unorm(subtract_values[i - 1] as f32 / 255. + 0.1);
            }
            for y in 0..bh {
                let mut counts = [0_i32; 2];
                for x in 0..bw {
                    let i = y * bw + x;
                    for c in 0..2 {
                        counts[c] += self.coverage_diff[c][y * (bw + 1) + x];
                    }
                    if counts[0] > 0 {
                        self.layers[0][i] = 255;
                    }
                    self.layers[1][i] = subtract_values[counts[1].clamp(0, 10) as usize];
                }
            }
            self.sources_rgba.resize(bw * bh * 4, 0);
            self.enabled_compose.resize(bw * bh, 0);
            self.point_aux.resize(bw * bh, [0; 3]);
            for i in 0..bw * bh {
                for c in 0..4 {
                    self.sources_rgba[i * 4 + c] = self.layers[c][i];
                }
                self.enabled_compose[i] = self.layers[0][i].abs_diff(if subtract_enabled(self.layers[1][i]) == 1. { 255 } else { 0 });
                let disabled = if self.layers[2][i] == 0 && self.layers[3][i] == 0 && self.raw_disabled_green[i] == 0 {
                    0
                } else {
                    let (sr, sg) = subtract_disabled(self.layers[3][i], self.raw_disabled_green[i]);
                    unorm((sr * sg - self.layers[2][i] as f32 / 255.).abs())
                };
                let ready_s = if self.ready_green[i] == 0 {
                    0
                } else {
                    unorm(subtract_disabled(self.layers[5][i], self.ready_green[i]).1)
                };
                self.point_aux[i] = [self.layers[4][i], ready_s, disabled];
            }
            self.uniform_compose = self
                .enabled_compose
                .first()
                .copied()
                .filter(|v| self.enabled_compose.iter().all(|p| p == v));
        }
        self.width = ew;
        self.height = eh;
        self.rgba.resize(ew * eh * 4, 0);
        self.rgba.fill(0);
        self.aux_rgba.resize(ew * eh * 4, 0);
        self.aux_rgba.fill(0);
        let stride = ew.div_ceil(64);
        self.active.resize(stride * eh, 0);
        self.active.fill(0);
        // Both native displacement samples share their Y coordinate. Compute
        // mirrored texture indices per row/column, rather than running four
        // floating-point remainders and image lookups for every mask pixel.
        let texture = &*DISPLACE;
        let pixels = texture.as_raw();
        let texture_stride = texture.width() as usize * 3;
        let d = 0.70703125_f32;
        let dt = d * (time / 20. * 2.59);
        self.warp_x.clear();
        self.warp_y.clear();
        if self.uniform_compose.is_none() {
            self.warp_x.extend((0..bw).map(|x| {
                let u = (x as f32 + 0.5) / bw as f32;
                (u, noise_index(dt + u * 2.13, texture.width()) as usize * 3, noise_index(-dt + u * 2.13, texture.width()) as usize * 3)
            }));
            self.warp_y.extend((0..bh).map(|y| {
                let v = (y as f32 + 0.5) / bh as f32;
                (v, noise_index(dt + v * 1.02, texture.height()) as usize * texture_stride)
            }));
        }
        static CENTERED_NOISE: Lazy<[f32; 256]> = Lazy::new(|| std::array::from_fn(|value| medium(medium(value as f32 / 255.) - 0.5)));
        let centered = &*CENTERED_NOISE;
        for y in 0..bh {
            for x in 0..bw {
                let i = y * bw + x;
                let mask = if let Some(value) = self.uniform_compose {
                    value
                } else {
                    let (u, xa, xb) = self.warp_x[x];
                    let (v, row) = self.warp_y[y];
                    let a = centered[pixels[row + xa] as usize];
                    let b = centered[pixels[row + xb] as usize];
                    let duv = [(d * a + b * -d) * 0.1 + u, (d * a + b * d) * 0.1 + v];
                    let sx = (duv[0] * bw as f32).floor().clamp(0., (bw - 1) as f32) as usize;
                    let sy = (duv[1] * bh as f32).floor().clamp(0., (bh - 1) as f32) as usize;
                    self.enabled_compose[sy * bw + sx]
                };
                let [ready_n, ready_s, disabled] = self.point_aux[i];
                let dst = i * 4;
                self.rgba[dst..dst + 4].copy_from_slice(&[mask, 0, 0, disabled]);
                self.aux_rgba[dst..dst + 4].copy_from_slice(&[ready_n, ready_s, disabled, 0]);
                if mask != 0 {
                    self.active[y * stride + x / 64] |= 1 << (x % 64);
                }
            }
        }
        self.render_rings();
        self.last_time = Some(time);
        self.revision = self.revision.wrapping_add(1);
    }

    fn render_rings(&mut self) {
        let (width, height) = (self.width, self.height);
        if self.rgba.chunks_exact(4).any(|p| p[0] != 0 && p[0] != 255) {
            self.render_gray_rings();
            return;
        }
        let stride = width.div_ceil(64);
        self.ping.clone_from(&self.active);
        self.pong.resize(self.active.len(), 0);
        for (pass, weight) in glow_weights().into_iter().enumerate() {
            if weight < 0.01 {
                break;
            }
            dilate(&self.ping, &mut self.pong, width, height);
            for y in 0..height {
                for word in 0..stride {
                    let i = y * stride + word;
                    let mut ring = self.pong[i] & !self.ping[i];
                    while ring != 0 {
                        let bit = ring.trailing_zeros() as usize;
                        let x = word * 64 + bit;
                        if x < width {
                            let dst = (y * width + x) * 4;
                            if pass == 0 {
                                self.rgba[dst + 1] = 255;
                            }
                            self.rgba[dst + 2] = unorm(weight);
                        }
                        ring &= ring - 1;
                    }
                }
            }
            std::mem::swap(&mut self.ping, &mut self.pong);
        }
    }

    fn render_gray_rings(&mut self) {
        let (w, h) = (self.width, self.height);
        self.gray_ping.resize(w * h, 0);
        self.gray_pong.resize(w * h, 0);
        for (i, p) in self.rgba.chunks_exact(4).enumerate() {
            self.gray_ping[i] = p[0];
        }
        for (pass, weight) in glow_weights().into_iter().enumerate() {
            if weight < 0.01 {
                break;
            }
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let mut maximum = self.gray_ping[i];
                    for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                        for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                            maximum = maximum.max(self.gray_ping[yy * w + xx]);
                        }
                    }
                    self.gray_pong[i] = maximum;
                    let delta = (maximum - self.gray_ping[i]) as f32 / 255.;
                    if pass == 0 {
                        self.rgba[i * 4 + 1] = maximum - self.gray_ping[i];
                    }
                    let outside = 1. - self.rgba[i * 4] as f32 / 255.;
                    self.rgba[i * 4 + 2] = unorm(weight * (outside * delta) + self.rgba[i * 4 + 2] as f32 / 255.);
                }
            }
            std::mem::swap(&mut self.gray_ping, &mut self.gray_pong);
        }
    }
}

fn glow_weights() -> [f32; 6] {
    static WEIGHTS: Lazy<[f32; 6]> = Lazy::new(|| {
        let sum: f32 = (1..=6).map(|k| (k as f32).powf(2.65)).sum();
        std::array::from_fn(|pass| ((6 - pass) as f32).powf(2.65) / sum)
    });
    *WEIGHTS
}

fn unorm(v: f32) -> u8 {
    (v.clamp(0., 1.) * 255.).round() as u8
}

fn subtract_enabled(red: u8) -> f32 {
    // Native SubtractBlockBlender pass 0: a threshold window, not parity.
    let r = red as f32 / 255.;
    if (0.09..0.12).contains(&r) {
        1.
    } else {
        0.
    }
}

fn subtract_disabled(red: u8, green: u8) -> (f32, f32) {
    let g = green as f32 / 255.;
    let t = ((g - 0.2) * -10.).clamp(0., 1.);
    let r = subtract_enabled(red) + t * t * (3. - 2. * t);
    // The intermediate is RG16, clamped and rounded after shader output.
    (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
}

pub(super) fn compose_uv(uv: [f32; 2], time: f32) -> [f32; 2] {
    // BlockCompose pass 0, preserving the native normalized direction and ST.
    // Native u_xlat16_* are mediump, not full-precision CPU floats. In the
    // exported GLES shader the inverse length, direction, sampled noise and
    // centered noise are separately rounded to binary16. Keeping the warp
    // coordinates highp after those steps is essential at point boundaries.
    let d = medium(0.5 * medium((0.5_f32).sqrt().recip()));
    let t = time / 20. * 2.59;
    let st = [uv[0] * 2.13, uv[1] * 1.02];
    let a = medium(medium(noise([d * t + st[0], d * t + st[1]])) - 0.5);
    let b = medium(medium(noise([-d * t + st[0], d * t + st[1]])) - 0.5);
    [(d * a + b * -d) * 0.1 + uv[0], (d * a + b * d) * 0.1 + uv[1]]
}

fn medium(value: f32) -> f32 {
    // The normalized noise/direction intermediates are zero or normal f16
    // numbers. Round ten fraction bits with ties-to-even; no heap or new dep.
    let bits = value.to_bits();
    f32::from_bits((bits + 0xfff + ((bits >> 13) & 1)) & !0x1fff)
}

fn noise_index(v: f32, size: u32) -> u32 {
    let v = v.rem_euclid(2.);
    let v = if v > 1. { 2. - v } else { v };
    (v * size as f32).floor().clamp(0., (size - 1) as f32) as u32
}

fn noise(uv: [f32; 2]) -> f32 {
    // Match the vertically flipped rows uploaded by the material renderer.
    DISPLACE.get_pixel(noise_index(uv[0], DISPLACE.width()), noise_index(uv[1], DISPLACE.height()))[0] as f32 / 255.
}

fn raster_rows(width: usize, height: usize, aspect: f32, zone: &Zone, mut write: impl FnMut(usize, usize, usize)) {
    if zone.half.x <= 0. || zone.half.y <= 0. {
        return;
    }
    let (s, c) = zone.angle.sin_cos();
    let y_half = s.abs() * zone.half.x + c.abs() * zone.half.y;
    let start = (((zone.center.y - y_half) * aspect + 1.) * height as f32 * 0.5 - 0.5)
        .ceil()
        .clamp(0., height as f32) as usize;
    let end = ((((zone.center.y + y_half) * aspect + 1.) * height as f32 * 0.5 - 0.5).floor() + 1.).clamp(0., height as f32) as usize;
    for y in start..end {
        let dy = ((y as f32 + 0.5) * 2. / height as f32 - 1.) / aspect - zone.center.y;
        let (mut lo, mut hi) = (-1.0_f32, 1.0_f32);
        for (a, b, half) in [(c, s * dy, zone.half.x), (-s, c * dy, zone.half.y)] {
            if a.abs() < 1e-7 {
                if b.abs() > half {
                    hi = lo - 1.;
                    break;
                }
            } else {
                let a0 = (-half - b) / a + zone.center.x;
                let a1 = (half - b) / a + zone.center.x;
                lo = lo.max(a0.min(a1));
                hi = hi.min(a0.max(a1));
            }
        }
        if lo > hi {
            continue;
        }
        let first = ((lo + 1.) * width as f32 * 0.5 - 0.5).ceil().clamp(0., width as f32) as usize;
        let last = (((hi + 1.) * width as f32 * 0.5 - 0.5).floor() + 1.).clamp(0., width as f32) as usize;
        write(y, first, last);
    }
}

fn dilate(source: &[u64], dest: &mut [u64], width: usize, height: usize) {
    let stride = width.div_ceil(64);
    let last_mask = u64::MAX >> ((64 - width % 64) % 64);
    for y in 0..height {
        for x in 0..stride {
            let mut value = 0;
            for row in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                let idx = row * stride + x;
                let mid = source[idx];
                let left = if x > 0 { source[idx - 1] >> 63 } else { 0 };
                let right = if x + 1 < stride { source[idx + 1] << 63 } else { 0 };
                value |= mid | (mid << 1) | left | (mid >> 1) | right;
            }
            dest[y * stride + x] = if x + 1 == stride { value & last_mask } else { value };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Vector;

    #[test]
    fn compose_uses_native_mediump_intermediate_rounding() {
        assert_eq!(medium(0.5 * medium((0.5_f32).sqrt().recip())), 0.70703125);
        assert_eq!(medium(medium(155. / 255.) - 0.5), 0.10791015625);
        assert_eq!(medium(1. + 1. / 2048.), 1., "ties round to even");
        assert_eq!(medium(1. + 3. / 2048.), 1.001953125);
    }

    fn zone(x: f32, y: f32, half_x: f32, half_y: f32, angle: f32, invert: bool) -> Zone {
        Zone {
            center: Vector::new(x, y),
            half: Vector::new(half_x, half_y),
            angle,
            invert,
            active: true,
            ready: false,
            opacity: 1.,
        }
    }

    #[test]
    fn nine_tap_dilation_matches_pixel_reference_across_word_boundaries() {
        for width in [1_usize, 63, 64, 65, 129] {
            let height = 9;
            let stride = width.div_ceil(64);
            let mut source = vec![0_u64; stride * height];
            for y in 0..height {
                for x in 0..width {
                    if (x * 11 + y * 7) % 17 == 0 {
                        source[y * stride + x / 64] |= 1 << (x % 64);
                    }
                }
            }
            let mut dest = vec![0; source.len()];
            dilate(&source, &mut dest, width, height);
            for y in 0..height {
                for x in 0..width {
                    let mut expected = false;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let sx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
                            let sy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
                            expected |= source[sy * stride + sx / 64] & (1 << (sx % 64)) != 0;
                        }
                    }
                    assert_eq!(dest[y * stride + x / 64] & (1 << (x % 64)) != 0, expected, "{width}: ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn raw_camera_layers_match_resolved_rectangle_coverage() {
        let zones = [
            zone(0.1, -0.1, 0.45, 0.23, 0.57, false),
            zone(-0.5, 0.13, 0.33, 0.09, -1.17, false),
            zone(0.0, 0.0, 1.0, 0.6, 0.0, true),
            zone(0.15, 0.2, 0.12, 0.21, 1.32, true),
        ];
        let (width, height, aspect) = (130, 76, 16. / 9.);
        let mut masks = Masks::default();
        masks.render_displaced(width * 4, height * 4, aspect, &zones, 0.);
        for y in 0..height {
            for x in 0..width {
                let p = Vector::new((x as f32 + 0.5) * 2. / width as f32 - 1., ((y as f32 + 0.5) * 2. / height as f32 - 1.) / aspect);
                let mut normal = false;
                let mut subtract_count = 0_usize;
                for z in &zones {
                    let local = nalgebra::Rotation2::new(-z.angle) * (p - z.center);
                    if local.x.abs() <= z.half.x && local.y.abs() <= z.half.y {
                        if z.invert {
                            subtract_count += 1;
                        } else {
                            normal = true;
                        }
                    }
                }
                assert_eq!(masks.sources_rgba[(y * width + x) * 4] != 0, normal, "normal ({x}, {y})");
                assert_eq!(masks.sources_rgba[(y * width + x) * 4 + 1], [0, 26, 52][subtract_count], "subtract ({x}, {y})");
            }
        }
    }

    #[test]
    fn exterior_rings_leave_fill_and_union_interiors_unchanged() {
        let mut masks = Masks::default();
        masks.render_displaced(800, 800, 1., &[zone(-0.2, 0., 0.3, 0.3, 0., false), zone(0.2, 0., 0.3, 0.3, 0., false)], 1.);
        let p = (100 * masks.width + 100) * 4;
        assert_eq!(&masks.rgba[p..p + 4], &[255, 0, 0, 0]);
        for p in masks.rgba.chunks_exact(4) {
            if p[0] == 255 {
                assert_eq!(&p[1..3], &[0, 0]);
            }
            if p[1] == 255 {
                assert_eq!(p[0], 0);
                assert!(p[2] > 0);
            }
        }
        assert!(masks.rgba.chunks_exact(4).any(|p| p[1] == 255));
    }

    #[test]
    fn subtract_cancellation_has_no_phantom_edges_and_zone_count_is_unbounded() {
        let mut masks = Masks::default();
        masks.render_displaced(800, 800, 1., &[zone(0., 0., 0.5, 0.3, 0., true), zone(0., 0., 0.5, 0.3, 0., true)], 1.);
        assert!(masks.rgba.iter().all(|&v| v == 0));
        let mut zones: Vec<_> = (0..159).map(|_| zone(2., 2., 0.1, 0.1, 0., false)).collect();
        zones.push(zone(0., 0., 0.2, 0.2, 0., false));
        masks.render_displaced(800, 800, 1., &zones, 1.);
        assert_eq!(masks.rgba[(100 * masks.width + 100) * 4], 255);
        let previous = masks.rgba.clone();
        masks.render_displaced(800, 800, 1., &zones, 1.);
        assert_eq!(masks.rgba, previous);
        masks.render_displaced(520, 560, 1., &zones, 1.);
        assert_eq!((masks.width, masks.height), (130, 140));
    }

    #[test]
    fn glow_weight_matches_native_material_and_skips_last_ring() {
        let weights = glow_weights();
        assert!((weights.iter().sum::<f32>() - 1.).abs() < 1e-6);
        assert!((weights[4] - 0.024947807).abs() < 1e-7);
        assert!(weights[5] < 0.01);
    }

    #[test]
    fn refined_compose_reuses_effect_dimensions_without_two_by_two_replication() {
        let mut masks = Masks::default();
        masks.render_displaced(960, 540, 16. / 9., &[zone(0., 0., 0.5, 0.25, 0., false)], 1.);
        assert_eq!((masks.width, masks.height), (240, 134));
        assert_eq!(masks.sources_rgba.len(), 240 * 134 * 4);
        // Detail must come from additional samples, not smoothed coarse cells.
        assert!((0..134).step_by(2).any(|y| (0..240).step_by(2).any(|x| {
            let sample = |px: usize, py: usize| masks.rgba[(py * 240 + px) * 4];
            sample(x, y) != sample(x + 1, y) || sample(x, y) != sample(x, y + 1)
        })));
    }

    #[test]
    fn disabled_subtract_does_not_clip_enabled_region() {
        let active = zone(0., 0., 0.5, 0.25, 0., false);
        let disabled = Zone {
            active: false,
            invert: true,
            ..active.clone()
        };
        let mut original = Masks::default();
        original.render_displaced(960, 540, 16. / 9., &[active.clone()], 1.);
        let mut mixed = Masks::default();
        mixed.render_displaced(960, 540, 16. / 9., &[active, disabled], 1.);
        assert!(original
            .rgba
            .chunks_exact(4)
            .zip(mixed.rgba.chunks_exact(4))
            .all(|(a, b)| a[..3] == b[..3]));
        assert!(mixed.rgba.chunks_exact(4).any(|p| p[0] == 255 && p[3] == 255));
    }

    #[test]
    fn visual_subtract_uses_threshold_window_including_three_overlaps() {
        let sub = zone(0., 0., 1., 1., 0., true);
        for (count, value) in [(1, 255), (2, 0), (3, 0)] {
            let mut mask = Masks::default();
            mask.render_displaced(128, 96, 4. / 3., &vec![sub.clone(); count], 2.);
            assert!(mask.rgba.chunks_exact(4).all(|p| p[0] == value));
        }
    }

    #[test]
    fn ready_inputs_use_ready_only_cameras_and_disabled_compose() {
        let disabled = Zone {
            active: false,
            ..zone(-0.5, 0., 0.2, 0.2, 0., false)
        };
        let ready = Zone {
            active: false,
            ready: true,
            ..zone(0.5, 0., 0.2, 0.2, 0., false)
        };
        let mut mask = Masks::default();
        mask.render_displaced(128, 96, 4. / 3., &[disabled, ready], 1.);
        let pixel = |x: usize| &mask.aux_rgba[(12 * mask.width + x) * 4..(12 * mask.width + x) * 4 + 4];
        assert_eq!(&pixel(8)[..3], &[0, 0, 255]);
        assert_eq!(&pixel(24)[..3], &[255, 0, 255]);
    }

    #[test]
    fn compose_displacement_animates_only_enabled_masks_and_uses_red_channel() {
        let active = zone(0., 0., 0.5, 0.2, 0., false);
        let disabled = Zone {
            active: false,
            ..active.clone()
        };
        let mut mask = Masks::default();
        mask.render_displaced(960, 540, 16. / 9., &[active.clone(), disabled.clone()], 1.);
        let before = mask.rgba.clone();
        mask.render_displaced(960, 540, 16. / 9., &[active, disabled], 2.);
        assert!(before.chunks_exact(4).zip(mask.rgba.chunks_exact(4)).any(|(a, b)| a[0] != b[0]));
        assert!(before.chunks_exact(4).zip(mask.rgba.chunks_exact(4)).all(|(a, b)| a[3] == b[3]));
        let pixel = DISPLACE.get_pixel(0, 0);
        assert_eq!(noise([0.0001, 0.0001]), pixel[0] as f32 / 255.);
        assert_ne!(pixel[0], pixel[1]);
    }

    #[test]
    fn initial_subtract_show_fades_green_while_alpha_stays_point_one() {
        let sub = Zone {
            active: false,
            opacity: 0.25,
            ..zone(0., 0., 1., 1., 0., true)
        };
        let mut mask = Masks::default();
        mask.render_displaced(128, 96, 4. / 3., &[sub], 1.);
        assert_eq!(mask.sources_rgba[3], 26);
        assert_eq!(mask.raw_disabled_green[0], 6);
        assert_eq!(mask.rgba[3], 120);
    }

    #[test]
    fn uniform_compose_reuses_masks_across_time_but_invalidates_on_geometry_change() {
        let full = zone(0., 0., 3., 3., 0., false);
        let mut masks = Masks::default();
        masks.render_displaced(960, 540, 16. / 9., &[full.clone()], 1.);
        let revision = masks.revision;
        assert_eq!(masks.uniform_compose, Some(255));
        masks.render_displaced(960, 540, 16. / 9., &[full], 2.);
        assert_eq!(masks.revision, revision);
        masks.render_displaced(960, 540, 16. / 9., &[zone(0., 0., 0.3, 0.2, 0., false)], 2.);
        assert_eq!(masks.uniform_compose, None);
        assert!(masks.revision > revision);
    }
}
