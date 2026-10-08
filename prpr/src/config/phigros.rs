//! Parameters of the article-compatible judgement engine. Times are milliseconds.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PhigrosRules {
    pub drag_ms: f64,
    pub drag_sides_ms: Option<[f64; 2]>,
    pub flick_ratio_sides: Option<[f64; 2]>,
    pub strict_early_ms: Option<[f64; 3]>,
    pub strict_late_ms: Option<[f64; 3]>,
    pub flick_ratio: f64,
    pub tap_width: f64,
    pub special_width: f64,
    pub bad_edge: f64,
    pub bad_shrink_factor: f64,
    pub metric_divisor: f64,
    pub protection_ms: f64,
    pub hold_safe_frames: i32,
    pub hold_tail_ms: f64,
    pub hold_delayed_miss_ms: f64,
    pub special_early_ms: f64,
    pub strict_perfect_ms: f64,
    pub strict_good_ms: f64,
    pub strict_bad_ms: f64,
    pub frame_compensation: bool,
    pub late_overrun: bool,
    pub flick_speed: f64,
    pub flick_dpi: f64,
    /// 0 uses platform DPI; platforms without a physical measurement expose a fallback.
    pub device_dpi: f64,
    pub flick_sample_hz: f64,
    pub flick_multiplier: f64,
    pub flick_projection_min: f64,
}

impl Default for PhigrosRules {
    fn default() -> Self {
        Self {
            drag_ms: 100.,
            drag_sides_ms: None,
            flick_ratio_sides: None,
            strict_early_ms: None,
            strict_late_ms: None,
            flick_ratio: 1.75,
            tap_width: 1.9,
            special_width: 2.1,
            bad_edge: 0.9,
            bad_shrink_factor: 0.5,
            metric_divisor: 2.2,
            protection_ms: 10.,
            hold_safe_frames: 2,
            hold_tail_ms: 220.,
            hold_delayed_miss_ms: 250.,
            special_early_ms: 5.,
            strict_perfect_ms: 40.,
            strict_good_ms: 90.,
            strict_bad_ms: 140.,
            frame_compensation: true,
            late_overrun: true,
            flick_speed: 0.06,
            flick_dpi: 380.,
            device_dpi: 0.,
            flick_sample_hz: 60.,
            flick_multiplier: 5.,
            flick_projection_min: 0.1,
        }
    }
}

impl PhigrosRules {
    pub fn drag_sides(&self) -> [f64; 2] {
        self.drag_sides_ms.unwrap_or([self.drag_ms; 2])
    }
    pub fn flick_sides(&self) -> [f64; 2] {
        self.flick_ratio_sides.unwrap_or([self.flick_ratio; 2])
    }
    pub fn strict_sides(&self) -> [[f64; 3]; 2] {
        let base = [self.strict_perfect_ms, self.strict_good_ms, self.strict_bad_ms];
        [self.strict_early_ms.unwrap_or(base), self.strict_late_ms.unwrap_or(base)]
    }
    pub fn sanitize(&mut self) {
        let d = Self::default();
        macro_rules! value {
            ($field:ident, $min:expr, $max:expr) => {
                self.$field = if self.$field.is_finite() {
                    self.$field.clamp($min, $max)
                } else {
                    d.$field
                };
            };
        }
        value!(drag_ms, 1., 400.);
        value!(flick_ratio, 0.1, 5.);
        value!(tap_width, 0.1, 10.);
        value!(special_width, 0.1, 10.);
        value!(bad_edge, 0., 10.);
        value!(metric_divisor, 0.1, 20.);
        value!(bad_shrink_factor, 0., 10.);
        value!(protection_ms, 0., 100.);
        self.hold_safe_frames = self.hold_safe_frames.clamp(0, 30);
        value!(hold_tail_ms, 0., 1000.);
        value!(hold_delayed_miss_ms, 0., 1000.);
        value!(special_early_ms, 0., 100.);
        value!(strict_perfect_ms, 1., 120.);
        value!(strict_good_ms, 1., 250.);
        value!(strict_bad_ms, 1., 400.);
        self.strict_good_ms = self.strict_good_ms.max(self.strict_perfect_ms);
        self.strict_bad_ms = self.strict_bad_ms.max(self.strict_good_ms);
        value!(flick_speed, 0.001, 10.);
        value!(flick_dpi, 1., 2000.);
        value!(device_dpi, 0., 2000.);
        value!(flick_sample_hz, 1., 1000.);
        value!(flick_multiplier, 0.1, 100.);
        value!(flick_projection_min, 0., 10.);
        for (field, base, min, max) in [
            (&mut self.drag_sides_ms, self.drag_ms, 1., 400.),
            (&mut self.flick_ratio_sides, self.flick_ratio, 0.1, 5.),
        ] {
            if let Some(values) = field {
                for v in values.iter_mut() {
                    *v = if v.is_finite() { v.clamp(min, max) } else { base };
                }
                if *values == [base; 2] {
                    *field = None;
                }
            }
        }
        let base = [self.strict_perfect_ms, self.strict_good_ms, self.strict_bad_ms];
        for field in [&mut self.strict_early_ms, &mut self.strict_late_ms] {
            if let Some(values) = field {
                for i in 0..3 {
                    values[i] = if values[i].is_finite() {
                        values[i].clamp(1., [120., 250., 400.][i])
                    } else {
                        base[i]
                    };
                    if i > 0 {
                        values[i] = values[i].max(values[i - 1]);
                    }
                }
                if *values == base {
                    *field = None;
                }
            }
        }
    }
}
