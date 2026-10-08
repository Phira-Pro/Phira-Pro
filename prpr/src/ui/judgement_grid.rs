use super::Ui;
use crate::{
    core::BOLD_FONT,
    ext::semi_white,
    judge::{visible_grades, PlayResult},
};
use macroquad::prelude::*;

/// The score panel's slanted content inset, safely to the right of the illustration.
pub fn judgement_panel_left(y: f32) -> f32 {
    -0.48 + (1.2 - y) / 1.9 * 0.4
}

/// Use the panel's full height left of RETRY, leaving the original combo bar above.
pub fn judgement_grid_area(top: f32) -> Rect {
    let y = -top + 0.4 + top * 0.3 + 0.062;
    let x = judgement_panel_left(y) + 0.012;
    Rect::new(x, y, 0.43 - x, (top - 0.04 - y).min(0.32).max(0.04))
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

pub struct GradeLayout {
    pub id: usize,
    pub name: &'static str,
    pub cell: Rect,
    pub label: Rect,
    pub label_size: f32,
    pub count_x: f32,
    pub count_width: f32,
    pub count_size: f32,
    pub number_gap: f32,
}

/// Size each column from its actual text, rather than allocating two wide equal cells.
pub fn judgement_grid_layout(ui: &mut Ui, result: &PlayResult, area: Rect, details: bool) -> Vec<GradeLayout> {
    let glyph_height = ui.text("PERFECT+").size(0.64).measure_using(&BOLD_FONT).h;
    let mut size = 0.64 * ((area.h / 4. * 0.85) / glyph_height).min(1.);
    let cells = grade_cells(result, area);
    let label_gap = if details { 0.015 } else { 0.025 };
    let column_gap = 0.030;
    let mut label_widths = [0f32; 2];
    let mut count_widths = [0f32; 2];
    // Re-measure if an unusually large display font needs to fit the panel.
    for attempt in 0..2 {
        label_widths = [0.; 2];
        count_widths = [0.; 2];
        for (n, (id, name, _)) in cells.iter().enumerate() {
            let col = n / 4;
            label_widths[col] = label_widths[col].max(ui.text(*name).size(size).measure_using(&BOLD_FONT).w);
            let count_width = if details && *id != 3 {
                ui.text(format!("-{}", result.early_kind[*id])).size(size).measure_using(&BOLD_FONT).w
                    + ui.text(format!("+{}", result.late_kind[*id])).size(size).measure_using(&BOLD_FONT).w
                    + 0.01
            } else {
                ui.text(result.counts[*id].to_string()).size(size).measure_using(&BOLD_FONT).w
            };
            count_widths[col] = count_widths[col].max(count_width);
        }
        let label_budget = (area.w - label_gap * 2. - column_gap - 0.06).max(0.001);
        let labels = label_widths.iter().sum::<f32>();
        if attempt == 0 && labels > label_budget {
            size *= label_budget / labels;
        } else {
            break;
        }
    }
    let count_budget = (area.w - label_widths.iter().sum::<f32>() - label_gap * 2. - column_gap - 0.006).max(0.001);
    let mut count_scale = (count_budget / count_widths.iter().sum::<f32>().max(0.001)).min(1.);
    if details && count_scale < 1. {
        let total = label_widths.iter().sum::<f32>() + count_widths.iter().sum::<f32>();
        let scale = ((area.w - label_gap * 2. - column_gap - 0.006) / total.max(0.001)).min(1.);
        size *= scale;
        for width in label_widths.iter_mut().chain(count_widths.iter_mut()) {
            *width *= scale;
        }
        count_scale = 1.;
    }
    let widths = [
        label_widths[0] + label_gap + count_widths[0] * count_scale,
        label_widths[1] + label_gap + count_widths[1] * count_scale,
    ];
    cells
        .into_iter()
        .enumerate()
        .map(|(n, (id, name, original))| {
            let col = n / 4;
            let slant = (original.y - area.y) / 1.9 * 0.4;
            let x = area.x - slant + if col == 0 { 0. } else { widths[0] + column_gap };
            let label_right = x + label_widths[col];
            let label = ui
                .text(name)
                .pos(label_right, original.y)
                .anchor(1., 0.)
                .size(size)
                .measure_using(&BOLD_FONT);
            GradeLayout {
                id,
                name,
                cell: Rect::new(x, original.y, widths[col], original.h),
                label,
                label_size: size,
                count_x: label_right + label_gap,
                count_width: count_widths[col] * count_scale,
                count_size: size * count_scale,
                number_gap: 0.01 * count_scale,
            }
        })
        .collect()
}

pub fn draw_judgement_grid(ui: &mut Ui, result: &PlayResult, area: Rect, details: bool) {
    for row in judgement_grid_layout(ui, result, area, details) {
        ui.text(row.name)
            .pos(row.label.right(), row.cell.y)
            .anchor(1., 0.)
            .size(row.label_size)
            .color(semi_white(0.6))
            .draw_using(&BOLD_FONT);
        if details && row.id != 3 {
            let r = ui
                .text(format!("-{}", result.early_kind[row.id]))
                .pos(row.count_x, row.label.center().y)
                .anchor(0., 0.5)
                .size(row.count_size)
                .color(Color::from_hex_rgb(0x81d4fa))
                .draw_using(&BOLD_FONT);
            ui.text(format!("+{}", result.late_kind[row.id]))
                .pos(r.right() + row.number_gap, row.label.center().y)
                .anchor(0., 0.5)
                .size(row.count_size)
                .color(Color::from_hex_rgb(0xffab91))
                .draw_using(&BOLD_FONT);
        } else {
            ui.text(result.counts[row.id].to_string())
                .pos(row.count_x, row.label.center().y)
                .anchor(0., 0.5)
                .size(row.count_size)
                .draw_using(&BOLD_FONT);
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
                assert!(r.y + r.h <= top - 0.04 + 1e-6 && r.right() <= 0.43 + 1e-6 && r.x >= judgement_panel_left(r.y) + 0.011);
            }
            result.grading.perfect_plus = false;
            assert_eq!(grade_cells(&result, area).len(), 7);
        }
    }
}
