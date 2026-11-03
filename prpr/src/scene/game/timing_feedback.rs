//! Timing feedback uses actual hits, never synthetic Miss/Drag/Flick offsets.
use crate::{core::{Resource, PGR_FONT}, judge::{Judgement, RecentHit}, ui::Ui};
use macroquad::prelude::Color;

fn latest(recent: &[RecentHit], now: f64) -> Option<(&'static str, f32)> {
    let hit = recent.iter().rev().find(|hit| {
        !matches!(hit.judgement, Judgement::PerfectPlus | Judgement::Perfect | Judgement::Miss)
            && hit.offset.is_finite() && hit.offset != 0.
    })?;
    let age = now - hit.time;
    if !(0. ..0.65).contains(&age) { return None; }
    Some((if hit.offset < 0. { "EARLY" } else { "LATE" }, age as f32))
}

pub(super) fn draw(ui: &mut Ui, res: &Resource, recent: &[RecentHit], top: f32) {
    let Some((text, age)) = latest(recent, res.time) else { return };
    let y = if res.config.early_late_position == 1 { -top - 0.055 } else { top + (-top) / 3. + 0.045 };
    let pop = (1. - age / 0.12).max(0.);
    let color = Color::from_hex_rgb(if text == "EARLY" { 0x85caff } else { 0xffad80 });
    ui.text(text).pos(0., y - pop * 0.012).anchor(0.5, 0.5)
        .size(0.42 + pop * 0.06)
        .color(Color { a: res.alpha * ((0.65 - age) / 0.25).clamp(0., 1.), ..color })
        .draw_using(&PGR_FONT);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hit(judgement: Judgement, offset: f64, time: f64) -> RecentHit { RecentHit { judgement, offset, time } }
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
