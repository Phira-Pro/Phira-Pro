use super::Ui;
use crate::{
    core::BOLD_FONT,
    ext::semi_white,
    judge::{visible_grades, PlayResult},
};
use macroquad::prelude::*;

/// Use the full height left of RETRY, while leaving the original combo bar above.
pub fn judgement_grid_area(top: f32) -> Rect {
    let y = -top + 0.4 + top * 0.3 + 0.062;
    Rect::new(-0.48, y, 0.88, (top - 0.04 - y).min(0.32).max(0.04))
}

/// Column-major: four grades in each column, with the original slanted alignment.
pub fn grade_cells(result: &PlayResult, area: Rect) -> Vec<(usize, &'static str, Rect)> {
    visible_grades(result.grading)
        .enumerate()
        .map(|(n, (id, name))| {
            let row = n % 4;
            let step = area.h / 4.;
            (
                id,
                name,
                Rect::new(area.x + (n / 4) as f32 * area.w / 2. - row as f32 * step / 1.9 * 0.4, area.y + row as f32 * step, area.w / 2., step),
            )
        })
        .collect()
}

pub fn draw_judgement_grid(ui: &mut Ui, result: &PlayResult, area: Rect, details: bool) {
    // Keep the original typography; scale only when four rows cannot fit at full size.
    let glyph_height = ui.text("PERFECT+").size(0.64).measure_using(&BOLD_FONT).h;
    let size = 0.64 * ((area.h / 4. * 0.85) / glyph_height).min(1.);
    for (id, name, cell) in grade_cells(result, area) {
        let x = cell.x + cell.w * 0.54;
        ui.text(name)
            .pos(x, cell.y)
            .anchor(1., 0.)
            .size(size)
            .color(semi_white(0.6))
            .max_width(cell.w * 0.54)
            .draw_using(&BOLD_FONT);
        let count_x = x + 0.03;
        let available = cell.right() - count_x - 0.01;
        if details && id != 3 {
            let early = format!("-{}", result.early_kind[id]);
            let late = format!("+{}", result.late_kind[id]);
            let early_w = ui.text(&early).size(size).measure_using(&BOLD_FONT).w;
            let late_w = ui.text(&late).size(size).measure_using(&BOLD_FONT).w;
            // Fit complete numbers, rather than truncating counters with an ellipsis.
            let count_size = size * ((available - 0.01) / (early_w + late_w).max(0.0001)).min(1.);
            let r = ui
                .text(early)
                .pos(count_x, cell.y)
                .size(count_size)
                .color(Color::from_hex_rgb(0x81d4fa))
                .draw_using(&BOLD_FONT);
            ui.text(late)
                .pos(r.right() + 0.01, cell.y)
                .size(count_size)
                .color(Color::from_hex_rgb(0xffab91))
                .draw_using(&BOLD_FONT);
        } else {
            let count = result.counts[id].to_string();
            let width = ui.text(&count).size(size).measure_using(&BOLD_FONT).w;
            let count_size = size * (available / width.max(0.0001)).min(1.);
            ui.text(count).pos(count_x, cell.y).size(count_size).draw_using(&BOLD_FONT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_grades_fit_four_rows_in_phone_tablet_and_wide_layouts() {
        for top in [0.75, 0.5625, 0.45, 0.43] {
            let mut result = PlayResult::default();
            result.grading.detailed = true;
            let area = judgement_grid_area(top);
            assert!(area.h / 4. >= 0.055, "phone rows must retain readable height");
            let cells = grade_cells(&result, area);
            assert_eq!(cells.iter().map(|(i, _, _)| *i).collect::<Vec<_>>(), [4, 0, 5, 1, 6, 7, 2, 3]);
            assert_eq!(cells[0].2.y, cells[4].2.y);
            assert!(cells[3].2.y > cells[0].2.y && cells[3].2.x < cells[0].2.x);
            for (_, _, r) in cells {
                assert!(r.y + r.h <= top - 0.04 + 1e-6 && r.right() <= 0.40 + 1e-6);
            }
            result.grading.perfect_plus = false;
            assert_eq!(grade_cells(&result, area).len(), 7);
        }
    }
}
