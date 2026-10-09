//! Transport math is independent of rendering, audio and the judgement engine.
pub const RATES: [f32; 8] = [0.1, 0.25, 0.5, 0.75, 1., 1.5, 2., 3.];
pub fn frame_target(frames: &[f64], current: f64, delta: i32, end: f64) -> f64 {
    if frames.is_empty() {
        return (current + delta as f64 / 60.).clamp(0., end);
    }
    let index = if delta > 0 {
        frames.partition_point(|t| *t <= current + 1e-7) as i64 + delta as i64 - 1
    } else {
        frames.partition_point(|t| *t < current - 1e-7) as i64 + delta as i64
    };
    if index < 0 {
        0.
    } else {
        frames.get(index as usize).copied().unwrap_or(end).clamp(0., end)
    }
}
pub fn loop_target(current: f64, a: Option<f64>, b: Option<f64>, enabled: bool) -> Option<f64> {
    let (a, b) = (a?, b?);
    (enabled && b - a >= 0.05 && current >= b).then(|| a + (current - b).rem_euclid(b - a))
}
pub fn error_target(errors: &[f64], current: f64, next: bool) -> Option<f64> {
    if next {
        errors.get(errors.partition_point(|t| *t <= current + 1.01)).copied()
    } else {
        errors[..errors.partition_point(|t| *t < current + 0.99)].last().copied()
    }
    .map(|t| (t - 1.).max(0.))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_frames_and_old_tapes() {
        let frames = [0., 0.006, 0.017, 0.04, 0.041, 0.1];
        assert_eq!(frame_target(&frames, 0.017, 1, 2.), 0.04);
        assert_eq!(frame_target(&frames, 0.017, -1, 2.), 0.006);
        assert_eq!(frame_target(&frames, 0.02, -1, 2.), 0.017);
        assert_eq!(frame_target(&frames, 0., -10, 2.), 0.);
        assert_eq!(frame_target(&frames, 0., 10, 2.), 2.);
        assert_eq!(frame_target(&[], 0., 1, 2.), 1. / 60.);
    }
    #[test]
    fn loop_preserves_overrun_and_rejects_invalid_ranges() {
        assert!((loop_target(8.2, Some(3.), Some(8.), true).unwrap() - 3.2).abs() < 1e-12);
        assert_eq!(loop_target(8., Some(3.), Some(8.), true), Some(3.));
        assert_eq!(loop_target(8., Some(8.), Some(3.), true), None);
        assert_eq!(loop_target(8., Some(3.), Some(8.), false), None);
        assert_eq!(loop_target(8., None, Some(8.), true), None);
    }
}
