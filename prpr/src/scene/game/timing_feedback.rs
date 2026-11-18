//! Timing feedback uses actual hits, never synthetic Miss/Drag/Flick offsets.
use crate::{core::{Resource, PGR_FONT}, judge::{Judgement, RecentHit}, ui::Ui};
use macroquad::prelude::Color;

const DISPLAY_TIME: f32 = 0.65;

fn visible_age(hit: &RecentHit, now: f64) -> Option<f32> {
    let age = (now - hit.time) as f32;
    (0. ..DISPLAY_TIME).contains(&age).then_some(age)
}

fn latest(recent: &[RecentHit], now: f64) -> Option<(&'static str, f32)> {
    let hit = recent.iter().rev().find(|hit| {
        !matches!(hit.judgement, Judgement::PerfectPlus | Judgement::Perfect | Judgement::Miss)
            && hit.offset.is_finite() && hit.offset != 0.
    })?;
    Some((if hit.offset < 0. { "EARLY" } else { "LATE" }, visible_age(hit, now)?))
}

pub(super) fn draw(ui: &mut Ui, res: &Resource, recent: &[RecentHit], top: f32) {
    let Some((text, age)) = latest(recent, res.time) else { return };
    let y = if res.config.early_late_position == 1 { -top - 0.055 } else { top + (-top) / 3. + 0.045 };
    let pop = (1. - age / 0.12).max(0.);
    let color = Color::from_hex_rgb(if text == "EARLY" { 0x85caff } else { 0xffad80 });
    ui.text(text).pos(0., y - pop * 0.012).anchor(0.5, 0.5)
        .size(0.42 + pop * 0.06)
        .color(Color { a: res.alpha * ((DISPLAY_TIME - age) / 0.25).clamp(0., 1.), ..color })
        .draw_using(&PGR_FONT);
}

fn format_offset(offset: f64) -> String {
    let ms = offset * 1000.;
    let ms = if ms.abs() < 0.05 { 0. } else { ms };
    format!("{ms:+.1} ms")
}

pub(super) fn draw_offsets(ui: &mut Ui, res: &Resource, recent: &[RecentHit], mean: Option<f64>, top: f32) {
    let mut y = top + (-top) / 3. + 0.045;
    if res.config.show_early_late && res.config.early_late_position != 1 {
        y += 0.048;
    }
    // The judge queues measured hits only, including an exactly on-time hit.
    let latest = recent.last().and_then(|hit| visible_age(hit, res.time).map(|age| (hit.offset, age)));
    if let Some((offset, age)) = latest {
        let pop = (1. - age / 0.12).max(0.);
        let hex = if offset < 0. { 0x85caff } else if offset > 0. { 0xffad80 } else { 0xffffff };
        let mut color = Color::from_hex_rgb(hex);
        color.a = res.alpha * ((DISPLAY_TIME - age) / 0.25).clamp(0., 1.);
        ui.text(format_offset(offset))
            .pos(0., y - pop * 0.012)
            .anchor(0.5, 0.5)
            .size(0.42 + pop * 0.06)
            .color(color)
            .draw_using(&PGR_FONT);
    }
    if let Some(mean) = mean {
        ui.text(format!("AVG {}", format_offset(mean)))
            .pos(0., y + 0.045)
            .anchor(0.5, 0.5)
            .size(0.30)
            .color(Color::new(1., 1., 1., res.alpha * 0.72))
            .draw_using(&PGR_FONT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hit(judgement: Judgement, offset: f64, time: f64) -> RecentHit { RecentHit { judgement, offset, time } }
    #[test]
    fn numeric_offset_uses_signed_milliseconds_without_negative_zero() {
        assert_eq!(format_offset(-0.01234), "-12.3 ms");
        assert_eq!(format_offset(0.01234), "+12.3 ms");
        assert_eq!(format_offset(0.), "+0.0 ms");
        assert_eq!(format_offset(-0.00001), "+0.0 ms");
        let exact = hit(Judgement::PerfectPlus, 0., 1.);
        assert_eq!(visible_age(&exact, 1.), Some(0.));
        assert!(visible_age(&exact, 2.).is_none());
    }
    #[test]
    fn nonperfect_hits_restart_feedback_and_perfects_do_not_erase_it() {
        let mut hits = vec![hit(Judgement::Good, -0.1, 1.)];
        assert_eq!(latest(&hits, 1.1).unwrap().0, "EARLY");
        hits.push(hit(Judgement::PerfectPlus, 0.01, 1.1));
        hits.push(hit(Judgement::Perfect, -0.02, 1.2));
        assert_eq!(latest(&hits, 1.2).unwrap().0, "EARLY");
        hits.push(hit(Judgement::Bad, 0.2, 1.3));
        assert_eq!(latest(&hits, 1.3), Some(("LATE", 0.)));
        assert!(latest(&hits, 2.).is_none());
        assert!(latest(&hits, 0.).is_none());
    }
    #[test]
    fn every_extended_grade_uses_the_real_offset() {
        for grade in [Judgement::Great, Judgement::Good, Judgement::Ok, Judgement::Meh, Judgement::Bad] {
            for offset in [-0.1, 0.1] {
                assert_eq!(latest(&[hit(grade, offset, 1.)], 1.).unwrap().0, if offset < 0. { "EARLY" } else { "LATE" });
            }
        }
        for grade in [Judgement::Perfect, Judgement::PerfectPlus, Judgement::Miss] {
            assert!(latest(&[hit(grade, 0.25, 1.)], 1.).is_none());
        }
    }
}
