//! CPU equivalent of the native mask, EdgeMask and GlowMask passes.
//! Bit rows keep the nine-tap dilation inexpensive without any offscreen FBOs.
use super::Zone;
use once_cell::sync::Lazy;
use std::collections::HashMap;

#[cfg(any(target_os = "windows", target_os = "ios"))]
static MASK_WORKERS: Lazy<Option<rayon::ThreadPool>> = Lazy::new(|| {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(2));
    (threads > 1)
        .then(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|n| format!("block-mask-{n}"))
                .build()
                .ok()
        })
        .flatten()
});

pub(super) fn prepare_workers() {
    #[cfg(any(target_os = "windows", target_os = "ios"))]
    Lazy::force(&MASK_WORKERS);
}

// Row ownership is disjoint. Workers read only the completed Compose plane;
// every GL call, texture upload and input evaluation stays on the main thread.
fn warp_rows(rgba: &mut [u8], active: &mut [u64], width: usize, stride: usize, apply: impl Fn((usize, (&mut [u8], &mut [u64]))) + Sync + Send) {
    #[cfg(any(target_os = "windows", target_os = "ios"))]
    if rgba.len() >= 400_000 {
        if let Some(pool) = &*MASK_WORKERS {
            use rayon::prelude::*;
            pool.install(|| {
                rgba.par_chunks_exact_mut(width * 4)
                    .zip(active.par_chunks_exact_mut(stride))
                    .enumerate()
                    .for_each(&apply)
            });
            return;
        }
    }
    rgba.chunks_exact_mut(width * 4)
        .zip(active.chunks_exact_mut(stride))
        .enumerate()
        .for_each(apply);
}

fn prefix_rows(plane: &mut [u8], diff: &[i32], width: usize, curve: Option<(&[u8; 256], i32)>) {
    let apply = |(dst, counts): (&mut [u8], &[i32])| {
        let mut sum = 0_i32;
        if let Some((curve, end)) = curve {
            for (pixel, count) in dst.iter_mut().zip(counts) {
                sum += count;
                *pixel = curve[sum.clamp(0, end) as usize];
            }
        } else {
            for (pixel, count) in dst.iter_mut().zip(counts) {
                sum += count;
                *pixel = sum.clamp(0, 255) as u8;
            }
        }
    };
    #[cfg(any(target_os = "windows", target_os = "ios"))]
    if plane.len() >= 100_000 {
        if let Some(pool) = &*MASK_WORKERS {
            use rayon::prelude::*;
            pool.install(|| plane.par_chunks_exact_mut(width).zip(diff.par_chunks_exact(width + 1)).for_each(&apply));
            return;
        }
    }
    plane.chunks_exact_mut(width).zip(diff.chunks_exact(width + 1)).for_each(apply);
}

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
    /// Native camera capture is needed only by the independent mask oracle.
    pub capture_sources: bool,
    pub raw_disabled_green: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub revision: u64,
    pub aux_revision: u64,
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
    gray_horizontal: Vec<u8>,
    warp_x: Vec<(f32, usize, usize)>,
    warp_y: Vec<(f32, usize)>,
    enabled_compose: Vec<u8>,
    uniform_compose: Option<u8>,
    base_rgba: Vec<u8>,
    coverage_diff: [Vec<i32>; 8],
    blend_steps: HashMap<u32, Option<u8>>,
    prepared_rows: HashMap<[u32; 5], Box<[(u16, u16, u16)]>>,
    prepared_dim: (usize, usize, f32),
    prepared_bytes: usize,
    source_bounds: Option<(usize, usize, usize, usize)>,
    group_index: HashMap<[u32; 5], usize>,
    groups: Vec<(Zone, [i32; 8])>,
}

impl Masks {
    /// Pre-rasterize exact keyframe poses, never quantizing animated geometry
    /// or chart time. Holds/static intervals reuse these poses; intermediates
    /// fall back to live rasterization. Hard 16 MiB budget, retained on retry.
    pub fn prepare_geometry(&mut self, width: usize, height: usize, aspect: f32, areas: &[super::BlockArea]) {
        self.clear_prepared_geometry();
        let (bw, bh) = ((width / 8).max(1) * 2, (height / 8).max(1) * 2);
        self.prepared_dim = (bw, bh, aspect);
        Lazy::force(&DISPLACE);
        prepare_workers();
        for area in areas {
            let times = [area.appear_time, area.enable_time, area.disable_time, area.disappear_time.next_down()]
                .into_iter()
                .chain(area.rotate_events.iter().map(|e| e.time))
                .chain(area.move_events.iter().map(|e| e.time))
                .chain(area.scale_events.iter().map(|e| e.time));
            for t in times {
                let Some(zone) = super::Zone::from_area(area, t, aspect) else {
                    continue;
                };
                let key = geometry_key(&zone);
                if self.prepared_rows.contains_key(&key) {
                    continue;
                }
                let mut rows = Vec::new();
                raster_rows(bw, bh, aspect, &zone, |y, first, last| rows.push((y as u16, first as u16, last as u16)));
                let bytes = rows.len() * std::mem::size_of::<(u16, u16, u16)>() + 96;
                if self.prepared_bytes + bytes > 16 * 1024 * 1024 {
                    return;
                }
                self.prepared_bytes += bytes;
                self.prepared_rows.insert(key, rows.into_boxed_slice());
            }
        }
    }

    pub fn clear_prepared_geometry(&mut self) {
        self.prepared_rows.clear();
        self.prepared_bytes = 0;
    }

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
            if self.prepared_dim != (bw, bh, aspect) {
                self.clear_prepared_geometry();
            }
            if self.blend_steps.len() > 512 {
                self.blend_steps.clear();
            }
            let mut used = [false; 8];
            let mut additive = [true; 8];
            let mut same_opacity = [true; 8];
            let mut first_opacity = [None; 8];
            for z in zones {
                for (channel, opacity) in contributions(z).into_iter().flatten() {
                    used[channel] = true;
                    if let Some(first) = first_opacity[channel] {
                        same_opacity[channel] &= first == opacity.to_bits();
                    } else {
                        first_opacity[channel] = Some(opacity.to_bits());
                    }
                    let step = *self.blend_steps.entry(opacity.to_bits()).or_insert_with(|| blend_step(opacity));
                    additive[channel] &= step.is_some();
                }
            }
            // Identical opacities need only a count and their reachable R8
            // sequence, including .1's half-integer rounding. Mixed opacities
            // use weighted sums only when every possible R8 input was proven.
            let fast: [bool; 8] = std::array::from_fn(|c| used[c] && (same_opacity[c] || additive[c]));
            let mut curves = [[0_u8; 256]; 8];
            let mut curve_end = [0; 8];
            for c in 0..8 {
                if !used[c] || !same_opacity[c] {
                    continue;
                }
                let opacity = f32::from_bits(first_opacity[c].unwrap());
                for n in 1..256 {
                    let value = unorm(curves[c][n - 1] as f32 / 255. + opacity);
                    if value == curves[c][n - 1] {
                        break;
                    }
                    curves[c][n] = value;
                    curve_end[c] = n;
                    if value == 255 {
                        break;
                    }
                }
            }
            for channel in 0..8 {
                if fast[channel] {
                    self.coverage_diff[channel].resize((bw + 1) * bh, 0);
                    self.coverage_diff[channel].fill(0);
                }
            }
            self.group_index.clear();
            self.groups.clear();
            // The six official cameras capture enabled N/S, disabled N/S
            // (including ready), and ready-only N/S. BlockSprite blends
            // SrcAlpha, One; each subtract sprite has alpha .1.
            for z in zones {
                let mut weights = [0_i32; 8];
                let mut slow = false;
                for (channel, opacity) in contributions(z).into_iter().flatten() {
                    if fast[channel] {
                        weights[channel] += if same_opacity[channel] {
                            1
                        } else {
                            self.blend_steps[&opacity.to_bits()].unwrap() as i32
                        };
                    } else {
                        slow = true;
                    }
                }
                if weights != [0; 8] {
                    let key = geometry_key(z);
                    let group = *self.group_index.entry(key).or_insert_with(|| {
                        self.groups.push((z.clone(), [0; 8]));
                        self.groups.len() - 1
                    });
                    for c in 0..8 {
                        self.groups[group].1[c] += weights[c];
                    }
                }
                if !slow {
                    continue;
                }
                let mut write = |y: usize, first: usize, last: usize| {
                    for (channel, opacity) in contributions(z).into_iter().flatten() {
                        if !fast[channel] {
                            // Half-integer blends can round differently depending
                            // on the previous R8 value. Preserve source ordering.
                            let plane = match channel {
                                6 => &mut self.raw_disabled_green,
                                7 => &mut self.ready_green,
                                _ => &mut self.layers[channel],
                            };
                            for pixel in &mut plane[y * bw + first..y * bw + last] {
                                *pixel = unorm(*pixel as f32 / 255. + opacity);
                            }
                        }
                    }
                };
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
                    for &(y, first, last) in rows.iter() {
                        write(y as usize, first as usize, last as usize);
                    }
                } else {
                    raster_rows(bw, bh, aspect, z, write);
                }
            }
            // Merge equal poses before visiting their rows. Source order is
            // irrelevant only for the proven integer/count channels above.
            for (z, weights) in &self.groups {
                let mut write = |y: usize, first: usize, last: usize| {
                    for c in 0..8 {
                        if weights[c] == 0 {
                            continue;
                        }
                        self.coverage_diff[c][y * (bw + 1) + first] += weights[c];
                        self.coverage_diff[c][y * (bw + 1) + last] -= weights[c];
                    }
                };
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
                    for &(y, first, last) in rows.iter() {
                        write(y as usize, first as usize, last as usize);
                    }
                } else {
                    raster_rows(bw, bh, aspect, z, write);
                }
            }
            // Prefix sums make heavily overlapping active rectangles cost
            // O(rectangles * rows + pixels), rather than O(sum of their areas).
            // Retain the exact R8 quantization of every native additive blend.
            for channel in 0..8 {
                if !fast[channel] {
                    continue;
                }
                let plane = match channel {
                    6 => &mut self.raw_disabled_green,
                    7 => &mut self.ready_green,
                    _ => &mut self.layers[channel],
                };
                prefix_rows(plane, &self.coverage_diff[channel], bw, same_opacity[channel].then_some((&curves[channel], curve_end[channel] as i32)));
            }
            self.enabled_compose.resize(bw * bh, 0);
            self.base_rgba.resize(bw * bh * 4, 0);
            self.base_rgba.fill(0);
            self.aux_rgba.resize(bw * bh * 4, 0);
            self.aux_rgba.fill(0);
            let mut bounds = (bw, bh, 0, 0);
            let has_disabled = used[2] || used[3] || used[6];
            let has_ready_subtract = used[5] || used[7];
            // R8 samples in the native [.09,.12) window are exactly bytes
            // 23..=30. This independent byte loop can vectorize; no floating
            // conversion or per-pixel support reduction blocks it.
            for ((dst, &normal), &subtract) in self.enabled_compose.iter_mut().zip(&self.layers[0]).zip(&self.layers[1]) {
                *dst = normal.abs_diff(if (23..=30).contains(&subtract) { 255 } else { 0 });
            }
            for (y, row) in self.enabled_compose.chunks_exact(bw).enumerate() {
                if let Some(first) = row.iter().position(|&p| p != 0) {
                    let last = row.iter().rposition(|&p| p != 0).unwrap();
                    bounds.0 = bounds.0.min(first);
                    bounds.1 = bounds.1.min(y);
                    bounds.2 = bounds.2.max(last + 1);
                    bounds.3 = y + 1;
                }
            }
            // Active-only frames don't sample Disabled/Ready masks. Avoid
            // rewriting their already-zero RGBA planes on the common path.
            if has_disabled || used[4] || has_ready_subtract {
                for i in 0..bw * bh {
                    let disabled = if !has_disabled || (self.layers[2][i] == 0 && self.layers[3][i] == 0 && self.raw_disabled_green[i] == 0) {
                        0
                    } else {
                        let (sr, sg) = subtract_disabled(self.layers[3][i], self.raw_disabled_green[i]);
                        unorm((sr * sg - self.layers[2][i] as f32 / 255.).abs())
                    };
                    let ready_s = if !has_ready_subtract || self.ready_green[i] == 0 {
                        0
                    } else {
                        unorm(subtract_disabled(self.layers[5][i], self.ready_green[i]).1)
                    };
                    self.base_rgba[i * 4 + 3] = disabled;
                    self.aux_rgba[i * 4..i * 4 + 3].copy_from_slice(&[self.layers[4][i], ready_s, disabled]);
                }
            }
            self.source_bounds = (bounds.0 < bounds.2 && bounds.1 < bounds.3).then_some(bounds);
            if cfg!(test) || self.capture_sources {
                self.sources_rgba.resize(bw * bh * 4, 0);
                for i in 0..bw * bh {
                    for c in 0..4 {
                        self.sources_rgba[i * 4 + c] = self.layers[c][i];
                    }
                }
            }
            self.uniform_compose = self
                .enabled_compose
                .first()
                .copied()
                .filter(|v| self.enabled_compose.iter().all(|p| p == v));
            self.aux_revision = self.aux_revision.wrapping_add(1);
        }
        self.width = ew;
        self.height = eh;
        // Only Compose/Edge/Glow evolve with noise time. Ready/Disabled camera
        // channels stay unchanged until geometry/phase/opacity changes.
        self.rgba.clone_from(&self.base_rgba);
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
        // Both centered noise channels lie in [-.5,.5]. The native normalized
        // direction therefore displaces UV by at most .070704 per axis. A
        // conservative .072 envelope skips empty pixels with identical samples.
        let pad_x = (bw as f32 * 0.072).ceil() as usize + 1;
        let pad_y = (bh as f32 * 0.072).ceil() as usize + 1;
        let (x0, y0, x1, y1) = self
            .source_bounds
            .map(|(x0, y0, x1, y1)| (x0.saturating_sub(pad_x), y0.saturating_sub(pad_y), (x1 + pad_x).min(bw), (y1 + pad_y).min(bh)))
            .unwrap_or((0, 0, 0, 0));
        let uniform = self.uniform_compose;
        let warp_x = &self.warp_x;
        let warp_y = &self.warp_y;
        let compose = &self.enabled_compose;
        warp_rows(&mut self.rgba, &mut self.active, bw, stride, |(y, (rgba, active))| {
            if y < y0 || y >= y1 {
                return;
            }
            for x in x0..x1 {
                let mask = if let Some(value) = uniform {
                    value
                } else {
                    let (u, xa, xb) = warp_x[x];
                    let (v, row) = warp_y[y];
                    let a = centered[pixels[row + xa] as usize];
                    let b = centered[pixels[row + xb] as usize];
                    let duv = [(d * a + b * -d) * 0.1 + u, (d * a + b * d) * 0.1 + v];
                    // The clamped values are nonnegative; integer truncation
                    // already computes floor. Avoid two scalar floor calls
                    // per pixel without changing point-sampled texel indices.
                    let sx = (duv[0] * bw as f32).clamp(0., (bw - 1) as f32) as usize;
                    let sy = (duv[1] * bh as f32).clamp(0., (bh - 1) as f32) as usize;
                    compose[sy * bw + sx]
                };
                rgba[x * 4] = mask;
                if mask != 0 {
                    active[x / 64] |= 1 << (x % 64);
                }
            }
        });
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
        self.gray_horizontal.resize(w * h, 0);
        for (i, p) in self.rgba.chunks_exact(4).enumerate() {
            self.gray_ping[i] = p[0];
        }
        for (pass, weight) in glow_weights().into_iter().enumerate() {
            if weight < 0.01 {
                break;
            }
            // A 3x3 maximum is exactly two separable maxima. Keep R8 values
            // throughout; this reduces nine reads per pixel to six and lets
            // the contiguous horizontal pass vectorize on desktop and ARM.
            for (src, dst) in self.gray_ping.chunks_exact(w).zip(self.gray_horizontal.chunks_exact_mut(w)) {
                dst[0] = src[0].max(src[1.min(w - 1)]);
                if w > 2 {
                    for (out, triple) in dst[1..w - 1].iter_mut().zip(src.windows(3)) {
                        *out = triple[0].max(triple[1]).max(triple[2]);
                    }
                }
                dst[w - 1] = src[w - 1].max(src[w.saturating_sub(2)]);
            }
            for y in 0..h {
                let above = y.saturating_sub(1) * w;
                let below = (y + 1).min(h - 1) * w;
                for x in 0..w {
                    let i = y * w + x;
                    let maximum = self.gray_horizontal[above + x]
                        .max(self.gray_horizontal[i])
                        .max(self.gray_horizontal[below + x]);
                    self.gray_pong[i] = maximum;
                    let delta = maximum - self.gray_ping[i];
                    if pass == 0 {
                        self.rgba[i * 4 + 1] = delta;
                    }
                    // No change to the quantized glow when this pass adds zero.
                    if delta == 0 || self.rgba[i * 4] == 255 {
                        continue;
                    }
                    let outside = 1. - self.rgba[i * 4] as f32 / 255.;
                    self.rgba[i * 4 + 2] = unorm(weight * (outside * (delta as f32 / 255.)) + self.rgba[i * 4 + 2] as f32 / 255.);
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
    let scaled = v.clamp(0., 1.) * 255.;
    let whole = scaled as u8;
    // Nonnegative R8 round-to-nearest, preserving exact half ties without a
    // scalar roundf call or the precision loss of adding .5 before truncation.
    whole + u8::from(scaled - whole as f32 >= 0.5)
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
    // Red affects this shader only through the enabled threshold's 0/1
    // result. All possible RG8 inputs therefore fit an exact 1 KiB table.
    // Preserve the two native intermediate roundings, including green's use
    // of the unquantized red expression before its own R8 conversion.
    static TABLE: Lazy<[[[u8; 2]; 256]; 2]> = Lazy::new(|| {
        std::array::from_fn(|enabled| {
            std::array::from_fn(|green| {
                let g = green as f32 / 255.;
                let t = ((g - 0.2) * -10.).clamp(0., 1.);
                let r = enabled as f32 + t * t * (3. - 2. * t);
                [unorm(r), unorm(g * r * 10.)]
            })
        })
    });
    let [r, g] = TABLE[usize::from((23..=30).contains(&red))][green as usize];
    (r as f32 / 255., g as f32 / 255.)
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

fn geometry_key(zone: &Zone) -> [u32; 5] {
    [
        zone.center.x.to_bits(),
        zone.center.y.to_bits(),
        zone.half.x.to_bits(),
        zone.half.y.to_bits(),
        zone.angle.to_bits(),
    ]
}

fn contributions(z: &Zone) -> [Option<(usize, f32)>; 4] {
    let layer = if z.active { usize::from(z.invert) } else { 2 + usize::from(z.invert) };
    let opacity = if z.invert { 0.1 } else { z.opacity };
    [
        Some((layer, opacity)),
        (z.invert && !z.active).then_some((6, 0.1 * z.opacity)),
        z.ready.then_some((4 + usize::from(z.invert), opacity)),
        (z.ready && z.invert).then_some((7, 0.1 * z.opacity)),
    ]
}

fn blend_step(opacity: f32) -> Option<u8> {
    let step = unorm(opacity);
    (0..=255_u8)
        .all(|previous| unorm(previous as f32 / 255. + opacity) == previous.saturating_add(step))
        .then_some(step)
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
    fn byte_quantization_preserves_rounding_at_every_half_boundary() {
        let reference = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        for i in 0..=65535 {
            let v = i as f32 / 65535.;
            assert_eq!(unorm(v), reference(v));
        }
        for i in 0..255 {
            let tie = (i as f32 + 0.5) / 255.;
            for v in [tie.next_down(), tie, tie.next_up()] {
                assert_eq!(unorm(v), reference(v), "R8 half boundary {i}, {v}");
            }
        }
        for v in [-1., 0., 1., 2., f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert_eq!(unorm(v), reference(v));
        }
    }

    #[test]
    fn disabled_lookup_matches_native_formula_for_every_rg8_input() {
        for red in 0..=255 {
            for green in 0..=255 {
                let g = green as f32 / 255.;
                let t = ((g - 0.2) * -10.).clamp(0., 1.);
                let r = subtract_enabled(red) + t * t * (3. - 2. * t);
                let reference = ((r.clamp(0., 1.) * 255.).round() / 255., ((g * r * 10.).clamp(0., 1.) * 255.).round() / 255.);
                assert_eq!(subtract_disabled(red, green), reference, "red={red}, green={green}");
            }
        }
    }

    #[test]
    fn separable_gray_rings_match_ordered_nine_tap_r8_passes() {
        for (w, h) in [(1, 1), (1, 7), (2, 9), (65, 13), (129, 31)] {
            let mut mask = Masks {
                width: w,
                height: h,
                rgba: vec![0; w * h * 4],
                ..Default::default()
            };
            for (i, p) in mask.rgba.chunks_exact_mut(4).enumerate() {
                p[0] = ((i * 71 + i / w * 53) % 256) as u8;
            }
            let mut expected = mask.rgba.clone();
            let mut src: Vec<_> = expected.chunks_exact(4).map(|p| p[0]).collect();
            let mut dst = vec![0; w * h];
            for (pass, weight) in glow_weights().into_iter().enumerate() {
                if weight < 0.01 {
                    break;
                }
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let mut maximum = src[i];
                        for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                            for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                                maximum = maximum.max(src[yy * w + xx]);
                            }
                        }
                        dst[i] = maximum;
                        let delta = (maximum - src[i]) as f32 / 255.;
                        if pass == 0 {
                            expected[i * 4 + 1] = maximum - src[i];
                        }
                        let outside = 1. - expected[i * 4] as f32 / 255.;
                        expected[i * 4 + 2] = unorm(weight * (outside * delta) + expected[i * 4 + 2] as f32 / 255.);
                    }
                }
                std::mem::swap(&mut src, &mut dst);
            }
            mask.render_gray_rings();
            assert_eq!(mask.rgba, expected, "{w}x{h}: quantized mask/edge/glow changed");
        }
    }

    #[test]
    fn displaced_masks_match_fresh_sampling_through_seeks_geometry_and_resize() {
        let mut cached = Masks::default();
        for (w, h) in [(320, 192), (320, 192), (192, 320), (8, 8), (320, 192)] {
            for time in [0., 0.008, 0.016, 0.5, 12., 0.1] {
                let zones = [Zone {
                    opacity: 0.667,
                    ..zone(time * 0.002, -0.2, 0.3, 0.35, time * 0.03, false)
                }];
                let mut fresh = Masks::default();
                let aspect = w as f32 / h as f32;
                cached.render_displaced(w, h, aspect, &zones, time);
                fresh.render_displaced(w, h, aspect, &zones, time);
                assert_eq!(cached.rgba, fresh.rgba, "{w}x{h} t={time}");
            }
        }
    }

    #[test]
    fn large_parallel_mask_matches_native_point_samples_and_bit_rows() {
        let (width, height, aspect, time) = (2560, 1440, 16. / 9., 3.17);
        let zones = [Zone {
            opacity: 0.667,
            ..zone(-0.1, 0., 0.5, 0.3, 0.2, false)
        }];
        let mut mask = Masks::default();
        mask.render_displaced(width, height, aspect, &zones, time);
        let (w, h) = (mask.width, mask.height);
        assert!(w * h >= 100_000, "exercise the worker threshold");
        let stride = w.div_ceil(64);
        for y in 0..h {
            for x in 0..w {
                let uv = compose_uv([(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32], time);
                let sx = (uv[0] * w as f32).floor().clamp(0., (w - 1) as f32) as usize;
                let sy = (uv[1] * h as f32).floor().clamp(0., (h - 1) as f32) as usize;
                let expected = mask.enabled_compose[sy * w + sx];
                assert_eq!(mask.rgba[(y * w + x) * 4], expected, "pixel {x},{y}");
                assert_eq!(mask.active[y * stride + x / 64] & (1 << (x % 64)) != 0, expected != 0);
            }
        }
    }

    #[test]
    fn weighted_scanlines_match_ordered_r8_blending_with_fades_and_ready() {
        let mut zones = Vec::new();
        for i in 0..120 {
            zones.push(Zone {
                active: i % 4 == 0,
                ready: i % 4 == 1,
                opacity: [0., 0.1, 0.25, 0.5, 0.667, 1., 0.123456][i % 7],
                ..zone((i % 11) as f32 * 0.09 - 0.45, (i % 5) as f32 * 0.1 - 0.2, 0.3, 0.11, i as f32 * 0.17, i % 3 == 0)
            });
        }
        let (w, h) = (80, 48);
        let mut reference: [Vec<u8>; 8] = std::array::from_fn(|_| vec![0; w * h]);
        for z in &zones {
            raster_rows(w, h, 16. / 9., z, |y, first, last| {
                for (channel, opacity) in contributions(z).into_iter().flatten() {
                    for p in &mut reference[channel][y * w + first..y * w + last] {
                        *p = unorm(*p as f32 / 255. + opacity);
                    }
                }
            });
        }
        let mut mask = Masks::default();
        mask.render_displaced(w * 4, h * 4, 16. / 9., &zones, 1.);
        for channel in 0..6 {
            assert_eq!(mask.layers[channel], reference[channel], "camera {channel}");
        }
        assert_eq!(mask.raw_disabled_green, reference[6]);
        assert_eq!(mask.ready_green, reference[7]);
    }

    #[test]
    fn prepared_geometry_is_exact_and_invalidates_on_resolution_change() {
        let area = super::super::BlockArea {
            top_right: Vector::new(0.8, 0.7),
            bottom_left: Vector::new(0.2, 0.3),
            appear_time: 0.,
            enable_time: 1.,
            disable_time: 3.,
            disappear_time: 4.,
            is_subtract: true,
            rotate_events: vec![],
            move_events: vec![],
            scale_events: vec![],
        };
        let mut prepared = Masks::default();
        let z = Zone::from_area(&area, 2., 16. / 9.).unwrap();
        prepared.prepare_geometry(960, 540, 16. / 9., &[area]);
        assert!(!prepared.prepared_rows.is_empty());
        assert!(prepared.prepared_rows.contains_key(&geometry_key(&z)));
        let mut live = Masks::default();
        for time in [1., 1.1, 2.] {
            prepared.render_displaced(960, 540, 16. / 9., &[z.clone()], time);
            live.render_displaced(960, 540, 16. / 9., &[z.clone()], time);
            assert_eq!(prepared.rgba, live.rgba);
            assert_eq!(prepared.aux_rgba, live.aux_rgba);
        }
        prepared.render_displaced(800, 600, 4. / 3., &[z], 3.);
        assert!(prepared.prepared_rows.is_empty());
        assert_eq!(prepared.prepared_bytes, 0);
    }

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
