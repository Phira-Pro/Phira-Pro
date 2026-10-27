//! Draw approved status elements in order without dropping crowded Mod icons.
use super::status::{BadgeItem, PlayStatus};
use crate::{
    core::{ModIcons, BOLD_FONT},
    ext::{semi_black, RectExt, ScaleType},
    ui::Ui,
};
use macroquad::prelude::*;

const HEIGHT: f32 = 0.052;
const GAP: f32 = 0.012;
const TEXT_SIZE: f32 = 0.38;
const TEXT_HEIGHT: f32 = 0.027;
const ICON_SIZE: f32 = 0.036;
const ICON_WIDTH: f32 = 0.06;
const RIGHT: f32 = 0.94;

#[cfg(all(test, target_os = "windows"))]
#[path = "status_badge/tests.rs"]
mod gpu_tests;

fn cells(widths: &[f32], left: f32, center_y: f32) -> Vec<Rect> {
    let mut x = left;
    let mut y = center_y - HEIGHT / 2.;
    widths
        .iter()
        .map(|&width| {
            let width = width.min(RIGHT - left);
            if x > left && x + width > RIGHT {
                x = left;
                y += HEIGHT + GAP;
            }
            let rect = Rect::new(x, y, width, HEIGHT);
            x += width + GAP;
            rect
        })
        .collect()
}

pub(super) fn draw(ui: &mut Ui, status: &PlayStatus, mod_icons: &ModIcons, center_y: f32) -> Option<Rect> {
    if status.items.is_empty() {
        return None;
    }
    let left = -0.55 + (1.2 - center_y) / 1.9 * 0.4;
    let widths: Vec<_> = status
        .items
        .iter()
        .map(|item| match item {
            BadgeItem::Icon(_) => ICON_WIDTH,
            BadgeItem::Text(value) => {
                let measured = ui.text(value).size(TEXT_SIZE).no_baseline().measure_using(&BOLD_FONT);
                measured.w * (TEXT_HEIGHT / measured.h.max(0.0001)).min(1.) + 0.04
            }
        })
        .collect();
    let layout = cells(&widths, left, center_y);
    let mut right = left;
    let mut bottom = center_y;
    for (index, rect) in layout.iter().enumerate() {
        let first_in_row = (rect.x - left).abs() < 1e-5;
        let skew = HEIGHT * 0.4 / 1.9;
        let mut b = ui.builder(WHITE);
        b.add(if first_in_row { -1. } else { rect.x + skew }, rect.y);
        b.add(rect.right(), rect.y);
        b.add(rect.right() - skew, rect.bottom());
        b.add(if first_in_row { -1. } else { rect.x }, rect.bottom());
        b.triangle(0, 1, 2);
        b.triangle(0, 2, 3);
        b.commit();
        match status.items.get(index) {
            Some(BadgeItem::Text(value)) => {
                let measured = ui.text(value).size(TEXT_SIZE).no_baseline().measure_using(&BOLD_FONT);
                let fit = (TEXT_HEIGHT / measured.h.max(0.0001))
                    .min((rect.w - 0.04) / measured.w.max(0.0001))
                    .min(1.);
                let drawn = ui
                    .text(value)
                    .size(TEXT_SIZE * fit)
                    .no_baseline()
                    .pos(rect.x + 0.02, rect.center().y)
                    .anchor(0., 0.5)
                    .color(semi_black(0.6))
                    .draw_using(&BOLD_FONT);
                debug_assert!(drawn.h <= TEXT_HEIGHT + 0.0001 && drawn.w <= rect.w - 0.0399);
            }
            Some(BadgeItem::Icon(icon)) => {
                let icon_rect = Rect::new(rect.center().x - ICON_SIZE / 2., rect.center().y - ICON_SIZE / 2., ICON_SIZE, ICON_SIZE);
                ui.fill_rect(icon_rect, (*mod_icons[*icon as usize], icon_rect, ScaleType::Fit, semi_black(0.6)));
            }
            None => {}
        }
        right = right.max(rect.right());
        bottom = bottom.max(rect.bottom());
    }
    Some(Rect::new(left, center_y - HEIGHT / 2., right - left, bottom - center_y + HEIGHT / 2.).feather(0.004))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crowded_badges_wrap_without_hiding_or_overlapping_elements() {
        let widths = [0.25, 0.06, 0.06, 0.06, 0.06, 0.06, 0.06, 0.06, 0.06, 0.3, 0.06, 0.06, 0.25];
        let placed = cells(&widths, -0.25, 0.);
        assert_eq!(placed.len(), widths.len());
        assert!(placed.last().unwrap().y > placed[0].y);
        for rect in &placed {
            assert!(rect.x >= -0.25 && rect.right() <= RIGHT + 1e-5);
            assert_eq!(rect.h, HEIGHT);
        }
        for pair in placed.windows(2) {
            assert!(pair[1].y > pair[0].y || pair[1].x >= pair[0].right() + GAP - 1e-5);
        }
    }
}
