use serde::{Deserialize, Serialize};

/// Additional grades are opt-in. Existing grade IDs and legacy scoring stay stable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JudgeGrading {
    pub perfect_plus: bool,
    pub detailed: bool,
    /// User-requested mania ACC weights; persisted in presets and replay tapes.
    pub mania: bool,
    /// None keeps resource-pack colors. Indices are Judgement's stable IDs.
    pub colors: [Option<u32>; 8],
    /// GREAT, OK, MEH thresholds in milliseconds, independently on each side.
    pub early_ms: [f32; 3],
    pub late_ms: [f32; 3],
}
impl Default for JudgeGrading {
    fn default() -> Self {
        Self {
            perfect_plus: true,
            detailed: false,
            mania: false,
            colors: [None; 8],
            early_ms: [70., 130., 160.],
            late_ms: [70., 130., 160.],
        }
    }
}
impl JudgeGrading {
    pub fn color(&self, id: usize, fallback: macroquad::prelude::Color) -> macroquad::prelude::Color {
        self.colors
            .get(id)
            .copied()
            .flatten()
            .map(|v| macroquad::prelude::Color::from_hex_rgb(v & 0xffffff))
            .unwrap_or(fallback)
    }
    pub fn normalize(&mut self, base: [[f32; 4]; 2]) {
        self.colors = self.colors.map(|v| v.map(|v| v & 0xffffff));
        if !self.detailed {
            return;
        }
        for (side, limits) in [&mut self.early_ms, &mut self.late_ms].into_iter().zip(base) {
            for (i, fallback) in [70., 130., 160.].into_iter().enumerate() {
                if !side[i].is_finite() {
                    side[i] = fallback;
                }
            }
            side[0] = side[0].clamp(limits[1], limits[2]);
            side[1] = side[1].clamp(limits[2], limits[3]);
            side[2] = side[2].clamp(side[1], limits[3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colors_roundtrip_and_legacy_settings_keep_resource_pack_fallbacks() {
        let mut g = JudgeGrading::default();
        for (i, c) in g.colors.iter_mut().enumerate() {
            *c = Some(0x123400 + i as u32);
        }
        let restored: JudgeGrading = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(g, restored);
        assert_eq!(restored.color(7, macroquad::prelude::WHITE), macroquad::prelude::Color::from_hex_rgb(0x123407));
        let old: JudgeGrading = serde_json::from_str("{}").unwrap();
        assert_eq!(old.color(0, macroquad::prelude::BLUE), macroquad::prelude::BLUE);
        assert!(!old.mania);
    }
}
