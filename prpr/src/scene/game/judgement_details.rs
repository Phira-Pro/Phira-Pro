//! Cosmetic live counts; scoring, combo and grade selection stay in the judge/config.
use crate::{config::JudgeGrading, core::PGR_FONT, ui::Ui};
use macroquad::prelude::Color;

pub fn draw(ui: &mut Ui, grading: JudgeGrading, counts: [u32; 8], right: f32, top: f32, alpha: f32) {
    const LABELS: [&str; 8] = ["P", "GOOD", "BAD", "MISS", "P+", "GR", "OK", "MEH"];
    const COLORS: [u32; 8] = [0xf6ffac, 0xa7edff, 0xff4b4b, 0xafafaf, 0xffc600, 0x87c8ff, 0x9fffee, 0xffccb6];
    for (row, id) in grading.visible_grades().enumerate() {
        ui.text(format!("{}:{}", LABELS[id], counts[id]))
            .pos(right, top + row as f32 * 0.043)
            .anchor(1., 0.)
            .size(0.4)
            .color(Color {
                a: alpha * 0.3,
                ..Color::from_hex_rgb(COLORS[id])
            })
            .draw_using(&PGR_FONT);
    }
}
