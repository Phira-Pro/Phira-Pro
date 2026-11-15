use crate::config::HealthFailureEffect;
use macroquad::prelude::Color;

const SLOWDOWN_SECS: f64 = 2.;
const SLOWDOWN_MENU_FADE: f64 = 0.5;
const RED_HOLD: f64 = 0.04;
const BLACK_AT: f64 = 0.30;
const FLASH_MENU_AT: f64 = 0.40;
const FLASH_MENU_FADE: f64 = 0.20;

pub(super) struct Failure {
    effect: HealthFailureEffect,
    started: f64,
    first_flash_frame: Option<f64>,
}

impl Failure {
    pub(super) fn new(effect: HealthFailureEffect, now: f64) -> Self {
        Self {
            effect,
            started: now,
            first_flash_frame: None,
        }
    }

    pub(super) fn is_flash(&self) -> bool {
        self.effect == HealthFailureEffect::RedFlash
    }

    pub(super) fn slowdown_factor(&self, now: f64) -> Option<f64> {
        let progress = ((now - self.started) / SLOWDOWN_SECS).clamp(0., 1.);
        (!self.is_flash() && progress < 1.).then(|| (1. - progress).powi(2))
    }

    pub(super) fn flash_color(&mut self, now: f64) -> Option<Color> {
        if !self.is_flash() {
            return None;
        }
        // Start at the first presented frame so a slow frame cannot skip the red flash.
        let elapsed = now - *self.first_flash_frame.get_or_insert(now);
        let fade = ((elapsed - RED_HOLD) / (BLACK_AT - RED_HOLD)).clamp(0., 1.) as f32;
        let red = 1. - fade * fade * (3. - 2. * fade);
        Some(Color::new(red, 0., 0., 1.))
    }

    pub(super) fn controls_alpha(&self, now: f64) -> f32 {
        let (elapsed, delay, duration) = if self.is_flash() {
            (self.first_flash_frame.map_or(0., |first| now - first), FLASH_MENU_AT, FLASH_MENU_FADE)
        } else {
            (now - self.started, SLOWDOWN_SECS, SLOWDOWN_MENU_FADE)
        };
        ((elapsed - delay) / duration).clamp(0., 1.) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::BLACK;

    #[test]
    fn flash_covers_the_first_frame_and_reaches_black_before_controls() {
        let mut failure = Failure::new(HealthFailureEffect::RedFlash, 0.);
        assert_eq!(failure.slowdown_factor(0.), None);
        assert_eq!(failure.controls_alpha(5.), 0.);
        assert_eq!(failure.flash_color(5.), Some(Color::new(1., 0., 0., 1.)));
        let fading = failure.flash_color(5.15).unwrap();
        assert!(fading.r > 0. && fading.r < 1.);
        assert_eq!(failure.flash_color(5.35), Some(BLACK));
        assert_eq!(failure.controls_alpha(5.35), 0.);
        assert_eq!(failure.controls_alpha(5.65), 1.);
    }

    #[test]
    fn legacy_failure_slows_the_chart_before_showing_controls() {
        let mut failure = Failure::new(HealthFailureEffect::Slowdown, 0.);
        assert_eq!(failure.flash_color(0.), None);
        assert_eq!(failure.slowdown_factor(1.), Some(0.25));
        assert_eq!(failure.slowdown_factor(2.), None);
        assert_eq!(failure.controls_alpha(2.), 0.);
        assert_eq!(failure.controls_alpha(2.5), 1.);
    }
}
