//! Conservative support of the final masks, not of the source rectangles.
//! Displacement, subtraction, dilation and ready processing have already run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bounds {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Bounds {
    fn include(bounds: &mut Option<Self>, x: usize, y: usize) {
        if let Some(b) = bounds {
            b.x0 = b.x0.min(x);
            b.y0 = b.y0.min(y);
            b.x1 = b.x1.max(x + 1);
            b.y1 = b.y1.max(y + 1);
        } else {
            *bounds = Some(Self {
                x0: x,
                y0: y,
                x1: x + 1,
                y1: y + 1,
            });
        }
    }

    pub fn uv(self, width: usize, height: usize) -> [f32; 4] {
        // Linear reads have half-texel support. basePixelUV floors to a two-
        // texel cell. Four texels cover both and include a rounding guard.
        const GUARD: usize = 4;
        [
            self.x0.saturating_sub(GUARD) as f32 / width as f32,
            self.y0.saturating_sub(GUARD) as f32 / height as f32,
            (self.x1 + GUARD).min(width) as f32 / width as f32,
            (self.y1 + GUARD).min(height) as f32 / height as f32,
        ]
    }
}

pub(super) struct Cache {
    key: Option<(u64, u64, usize, usize)>,
    pub active: Option<Bounds>,
    pub disabled: Option<Bounds>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            key: None,
            active: None,
            disabled: None,
        }
    }
}

impl Cache {
    pub fn update(&mut self, rgba: &[u8], aux: &[u8], width: usize, height: usize, revision: u64, aux_revision: u64) {
        let key = (revision, aux_revision, width, height);
        if self.key == Some(key) {
            return;
        }
        assert_eq!(rgba.len(), width * height * 4);
        assert_eq!(aux.len(), rgba.len());
        self.active = None;
        self.disabled = None;
        for (y, (row, aux_row)) in rgba.chunks_exact(width * 4).zip(aux.chunks_exact(width * 4)).enumerate() {
            for (x, (p, a)) in row.chunks_exact(4).zip(aux_row.chunks_exact(4)).enumerate() {
                if p[3] != 0 {
                    Bounds::include(&mut self.disabled, x, y);
                }
                // Ready contributes |aux.r-aux.g|*aux.b. Unioning R/G is wider
                // than its true support, which is safe. Hover is handled elsewhere.
                if p[..3] != [0, 0, 0] || a[..2] != [0, 0] {
                    Bounds::include(&mut self.active, x, y);
                }
            }
        }
        self.key = Some(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_contains_linear_and_two_texel_point_cells_at_every_edge() {
        for (w, h) in [(2, 2), (6, 10), (480, 300)] {
            for (x, y) in [(0, 0), (1, 1), (w / 2, h / 2), (w - 1, h - 1)] {
                let mut rgba = vec![0; w * h * 4];
                let mut aux = rgba.clone();
                let i = (y * w + x) * 4;
                // Isolate all coverage inputs and the disabled layer.
                for channel in 0..6 {
                    rgba.fill(0);
                    aux.fill(0);
                    if channel < 4 {
                        rgba[i + channel] = 1;
                    } else {
                        aux[i + channel - 4] = 1;
                    }
                    let mut cache = Cache::default();
                    cache.update(&rgba, &aux, w, h, 1, 1);
                    let b = if channel == 3 { cache.disabled } else { cache.active }.unwrap().uv(w, h);
                    for (s, dimension, lo, hi) in [(x, w, b[0], b[2]), (y, h, b[1], b[3])] {
                        let cell = s / 2 * 2;
                        let expected_lo = ((s as f32 - 0.5).min(cell as f32) / dimension as f32).max(0.);
                        let expected_hi = ((s as f32 + 1.5).max((cell + 2) as f32) / dimension as f32).min(1.);
                        assert!(lo <= expected_lo && hi >= expected_hi);
                    }
                }
            }
        }
    }

    #[test]
    fn revision_invalidates_bounds_and_hover_does_not_define_field_support() {
        let mut rgba = vec![0; 8 * 8 * 4];
        let mut aux = rgba.clone();
        aux[3] = 255;
        let mut cache = Cache::default();
        cache.update(&rgba, &aux, 8, 8, 1, 1);
        assert!(cache.active.is_none() && cache.disabled.is_none());
        rgba[(7 * 8 + 7) * 4] = 255;
        cache.update(&rgba, &aux, 8, 8, 2, 1);
        assert!(cache.active.is_some());
        rgba.fill(0);
        cache.update(&rgba, &aux, 8, 8, 3, 1);
        assert!(cache.active.is_none());
        aux[1] = 255;
        cache.update(&rgba, &aux, 8, 8, 3, 2);
        assert!(cache.active.is_some());
    }
}
