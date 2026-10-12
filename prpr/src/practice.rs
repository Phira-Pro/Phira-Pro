//! Deterministic chart-order combo selection. Simultaneous notes are included
//! together; Hold ends remain playable even if they extend beyond the last head.
pub fn combo_range(notes: &[(f64, f64)], first: usize, last: usize, offset: f64, length: f64) -> Option<std::ops::Range<f64>> {
    if first == 0 || last < first || last > notes.len() {
        return None;
    }
    let start = notes[first - 1].0;
    let last_time = notes[last - 1].0;
    let end = notes
        .iter()
        .filter(|n| n.0 >= start && n.0 <= last_time)
        .map(|n| n.1)
        .fold(last_time, f64::max);
    let range = (start + offset - 0.25).max(offset.min(0.))..(end + offset + 0.25).min(length);
    (range.start.is_finite() && range.end.is_finite() && range.end > range.start).then_some(range)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combo_bounds_include_chords_and_hold_tails() {
        let notes = [(1., 1.), (2., 5.), (2., 2.), (6., 6.)];
        assert_eq!(combo_range(&notes, 2, 3, 0.1, 7.), Some(1.85..5.35));
        assert!(combo_range(&notes, 0, 2, 0., 7.).is_none());
        assert!(combo_range(&notes, 2, 5, 0., 7.).is_none());
    }
}
