//! Transport math is independent of rendering, audio and the judgement engine.
pub const RATES: [f32; 8] = [0.1, 0.25, 0.5, 0.75, 1., 1.5, 2., 3.];
/// Target time and whether it comes from a continuous recorded frame table.
pub fn frame_step(frames: &[f64], current: f64, delta: i32, end: f64) -> (f64, bool) {
    let fallback = (current + delta as f64 / 60.).clamp(0., end);
    if delta == 0 { return (current.clamp(0., end), false); }
    let index = if delta > 0 {
        frames.partition_point(|t| *t <= current + 1e-7) as i64 + delta as i64 - 1
    } else {
        frames.partition_point(|t| *t < current - 1e-7) as i64 + delta as i64
    };
    // Old/sparse tapes may have only touch-sample timestamps, or stop before
    // the audio ends. Missing frames must not turn one step into a song seek.
    let Some(target) = usize::try_from(index).ok().and_then(|i| frames.get(i)).copied() else { return (fallback, false) };
    if (target - current).abs() > delta.unsigned_abs() as f64 * 0.25 {
        (fallback, false)
    } else { (target.clamp(0., end), true) }
}
#[cfg(test)]
pub fn frame_target(frames: &[f64], current: f64, delta: i32, end: f64) -> f64 {
    frame_step(frames, current, delta, end).0
}
pub fn loop_target(current: f64, a: Option<f64>, b: Option<f64>, enabled: bool) -> Option<f64> {
    let (a, b) = (a?, b?);
    (enabled && b - a >= 0.05 && current >= b).then(|| a + (current - b).rem_euclid(b - a))
}
pub fn error_index(errors: &[f64], current: f64, next: bool, selected: Option<usize>) -> Option<usize> {
    if let Some(i) = selected.filter(|&i| errors.get(i).is_some_and(|t| current >= (t - 1.).max(0.) - 1e-7 && current <= *t + 1e-7)) {
        return if next { (i + 1 < errors.len()).then_some(i + 1) } else { i.checked_sub(1) };
    }
    if next {
        let i = errors.partition_point(|t| *t <= current + 1e-7);
        (i < errors.len()).then_some(i)
    } else {
        errors.partition_point(|t| *t < current - 1e-7).checked_sub(1)
    }
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
        assert_eq!(frame_target(&frames, 0., 10, 2.), 10. / 60.);
        assert_eq!(frame_target(&[], 0., 1, 2.), 1. / 60.);
    }
    #[test]
    fn sparse_and_exhausted_frames_never_seek_to_song_boundaries() {
        for frames in [&[][..], &[0.][..], &[0., 120.][..]] {
            assert!((frame_target(frames, 30., -1, 120.) - (30. - 1. / 60.)).abs() < 1e-9);
            assert!((frame_target(frames, 30., 1, 120.) - (30. + 1. / 60.)).abs() < 1e-9);
        }
        assert_eq!(frame_target(&[0., 0., 0.01, 0.01, 0.02], 0.01, 1, 2.), 0.02);
        assert_eq!(frame_target(&[0., 0., 0.01, 0.01, 0.02], 0.01, -1, 2.), 0.);
    }
    #[test]
    fn error_navigation_keeps_a_cursor_separate_from_preroll() {
        let errors = [0.1, 0.2, 3., 3.002, 8.];
        assert_eq!(error_index(&errors, 0., true, None), Some(0));
        assert_eq!(error_index(&errors, 0., true, Some(0)), Some(1));
        assert_eq!(error_index(&errors, 0., false, Some(1)), Some(0));
        assert_eq!(error_index(&errors, 2., true, Some(2)), Some(3));
        assert_eq!(error_index(&errors, 2.002, false, Some(3)), Some(2));
        assert_eq!(error_index(&errors, 7., true, Some(4)), None);
        assert_eq!(error_index(&errors, 4., false, Some(2)), Some(3));
        assert_eq!(error_index(&[], 0., true, None), None);
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
