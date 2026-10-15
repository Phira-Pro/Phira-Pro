//! The library and settings header use the same search control.
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, RectExt, SafeTexture, ScaleType},
    ui::{DRectButton, RectButton, Ui},
};

pub(super) const HEIGHT: f32 = 0.09;

pub(super) fn rect(right: f32, y: f32, height: f32, empty: bool) -> Rect {
    let width = 0.53 - if empty { height } else { 0. };
    Rect::new(right - width, y, width, height)
}

pub(super) fn render(
    ui: &mut Ui,
    button: &mut DRectButton,
    clear: &mut RectButton,
    r: Rect,
    t: f32,
    text: &str,
    search_icon: &SafeTexture,
    close_icon: &SafeTexture,
) {
    button.render_shadow(ui, r, t, |ui, path| ui.fill_path(&path, semi_black(0.4)));
    let mut icon = r.feather(-0.01);
    icon.w = icon.h;
    if text.is_empty() {
        clear.set(ui, Rect::new(10., 10., 0., 0.));
    } else {
        ui.fill_rect(icon, (**close_icon, icon, ScaleType::Fit));
        clear.set(ui, icon);
        icon.x += icon.w;
    }
    ui.fill_rect(icon, (**search_icon, icon, ScaleType::Fit));
    ui.text(text)
        .pos(icon.right() + 0.01, r.center().y)
        .anchor(0., 0.5)
        .no_baseline()
        .size(0.6)
        .max_width((r.right() - icon.right() - 0.03).max(0.02))
        .draw();
}
