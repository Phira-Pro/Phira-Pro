//! Stage-local animation decisions, evaluated from the movie clock only.
fn unit(value: f64) -> f32 {
    value.clamp(0., 1.) as f32
}
pub fn out_cubic(value: f32) -> f32 {
    1. - (1. - value.clamp(0., 1.)).powi(3)
}
pub fn in_out_quad(value: f32) -> f32 {
    let value = value.clamp(0., 1.);
    if value < 0.5 {
        2. * value * value
    } else {
        1. - (-2. * value + 2.).powi(2) / 2.
    }
}
pub fn in_out_cubic(value: f32) -> f32 {
    let value = value.clamp(0., 1.);
    if value < 0.5 {
        4. * value.powi(3)
    } else {
        1. - (-2. * value + 2.).powi(3) / 2.
    }
}
pub fn part(time: f64, start: f64, end: f64) -> f32 {
    unit((time - start) / (end - start))
}

pub struct LoadingFrame {
    pub slide: f32,
    pub tip: f32,
    pub reveal: f32,
    pub line: f32,
    pub hud: f32,
}
pub fn loading(time: f64, duration: f64, delta: f64) -> LoadingFrame {
    // Reference sequence: one-second slide, half-second pause, half-second
    // scene entrance, then one second holding the completed scene before music.
    let scale = 3. / duration.min(3.);
    let time = time * scale;
    let hold = (duration * scale - 3.).max(0.);
    let slide = part(time, hold, hold + 1.).powi(3);
    let tip = 1. - part(time, hold + 0.5, hold + 1.);
    // The reference divides this interval by its frame count minus one.
    let reveal = part(time, hold + 1.5, hold + 2. - (delta * scale).min(0.49));
    LoadingFrame {
        slide,
        tip,
        reveal,
        line: in_out_quad(reveal),
        hud: in_out_quad(reveal),
    }
}

/// Normalized left and right edges: extend the right edge first, then move
/// the left edge rightward. Both ends of the cycle have zero visible width.
pub fn loading_wipe(time: f64) -> (f32, f32) {
    let phase = time.rem_euclid(1.2);
    (in_out_quad(part(phase, 0.6, 1.1)), in_out_quad(part(phase, 0., 0.5)))
}
pub fn ending_time(time: f64, duration: f64) -> f64 {
    time * 6. / duration.min(6.) - 1.
}
pub fn result_slide(time: f64, start: f64, end: f64) -> f32 {
    1. - out_cubic(part(time, start, end))
}
pub fn result_alpha(time: f64, start: f64, end: f64) -> f32 {
    in_out_quad(part(time, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_finishes_the_real_scene_before_the_music_boundary() {
        let frame = |time| loading(time, 5.8, 1. / 30.);
        assert_eq!(frame(2.79).slide, 0.);
        assert!((frame(3.3).slide - 0.125).abs() < 1e-6);
        assert!((frame(3.55).tip - 0.5).abs() < 1e-6);
        assert!((frame(3.8).slide - 1.).abs() < 1e-6);
        assert!(frame(4.3).reveal < 1e-6);
        assert!((frame(4.3 + 7. / 30.).line - 0.5).abs() < 1e-6);
        assert!((frame(4.8 - 1. / 30.).hud - 1.).abs() < 1e-6);
        assert_eq!(frame(4.8).line, 1.);
        for duration in [1., 3., 5.8, 60.] {
            for fps in [30, 60, 120, 240] {
                let last = loading(duration - 1. / fps as f64, duration, 1. / fps as f64);
                assert_eq!(last.slide, 1.);
                assert_eq!(last.tip, 0.);
                assert_eq!(last.reveal, 1.);
                assert_eq!(last.line, 1.);
                assert_eq!(last.hud, 1.);
            }
        }
        assert_eq!(ending_time(2., 2.), 5.);
        assert_eq!(ending_time(6., 10.), 5.);
        assert_eq!(result_alpha(0.59, 0.6, 0.9), 0.);
        assert_eq!(result_alpha(0.9, 0.6, 0.9), 1.);
    }

    #[test]
    fn wipe_grows_left_to_right_then_disappears_to_the_right_without_a_reset_flash() {
        assert_eq!(loading_wipe(0.), (0., 0.));
        assert_eq!(loading_wipe(0.55), (0., 1.));
        assert_eq!(loading_wipe(1.15), (1., 1.));
        assert_eq!(loading_wipe(1.2), (0., 0.));
        for step in 0..120 {
            let (left, right) = loading_wipe(step as f64 / 100.);
            assert!(0. <= left && left <= right && right <= 1.);
            if step < 50 {
                assert_eq!(left, 0.);
            } else if step >= 60 {
                assert_eq!(right, 1.);
            }
        }
    }
}
