use serde::{Deserialize, Serialize};

/// Additional grades are opt-in. Existing grade IDs and legacy scoring stay stable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JudgeGrading {
    pub perfect_plus: bool,
    pub detailed: bool,
    /// GREAT, OK, MEH thresholds in milliseconds, independently on each side.
    pub early_ms: [f32; 3],
    pub late_ms: [f32; 3],
}
impl Default for JudgeGrading {
    fn default() -> Self {
        Self {
            perfect_plus: true,
            detailed: false,
            early_ms: [70., 130., 160.],
            late_ms: [70., 130., 160.],
        }
    }
}
impl JudgeGrading {
    pub fn normalize(&mut self, base: [[f32; 4]; 2]) {
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
