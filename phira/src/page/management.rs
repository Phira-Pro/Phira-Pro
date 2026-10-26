//! Shared presentation for local management pages; contains no page state or actions.
use super::settings::item_row_h;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_white, RectExt},
    ui::Ui,
};

pub(super) const PANEL: Color = Color::new(0.075, 0.11, 0.15, 0.94);
pub(super) const ACCENT: Color = Color::new(0.35, 0.70, 0.95, 1.);

pub(super) fn row_title(ui: &mut Ui, title: &str, subtitle: Option<&str>, control_x: f32) {
    let width = (control_x - 0.07).max(0.1);
    ui.text(title)
        .pos(0.035, item_row_h() * if subtitle.is_some() { 0.32 } else { 0.5 })
        .anchor(0., 0.5)
        .size(0.48)
        .max_width(width)
        .draw();
    if let Some(subtitle) = subtitle {
        ui.text(subtitle)
            .pos(0.035, item_row_h() * 0.76)
            .anchor(0., 0.5)
            .size(0.28)
            .max_width(width)
            .color(semi_white(0.6))
            .draw();
    }
}

pub(super) fn card(ui: &mut Ui, w: f32, h: f32, selected: bool) {
    let r = Rect::new(0.015, 0.005, w - 0.03, h - 0.01);
    ui.fill_rect(r, Color::new(0.12, 0.17, 0.22, 0.95));
    ui.fill_rect(Rect::new(r.x, r.y, r.w, item_row_h() - 0.005), Color::new(0.16, 0.23, 0.30, 0.95));
    ui.stroke_path(&r.rounded(0.), 0.002, if selected { ACCENT } else { semi_white(0.25) });
    ui.fill_rect(Rect::new(r.x, r.y, 0.006, r.h), ACCENT);
}

pub(super) fn card_header(ui: &mut Ui, badge: &str, title: &str, subtitle: &str, control_x: f32) {
    let badge_rect = Rect::new(0.04, item_row_h() * 0.32 - 0.023, 0.11, 0.046);
    ui.fill_path(&badge_rect.rounded(0.008), ACCENT);
    ui.text(badge)
        .pos(badge_rect.center().x, badge_rect.center().y)
        .anchor(0.5, 0.5)
        .size(0.26)
        .color(BLACK)
        .draw();
    ui.text(title)
        .pos(0.175, item_row_h() * 0.32)
        .anchor(0., 0.5)
        .size(0.54)
        .max_width(control_x - 0.21)
        .draw();
    ui.text(subtitle)
        .pos(0.175, item_row_h() * 0.76)
        .anchor(0., 0.5)
        .size(0.28)
        .max_width(control_x - 0.21)
        .color(semi_white(0.7))
        .draw();
}
