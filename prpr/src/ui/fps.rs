use super::Ui;
use crate::ext::RectExt;
use macroquad::prelude::*;

/// Small screen-edge overlay shared by the production renderer and visual QA.
pub fn draw_fps_overlay(ui: &mut Ui, fps: f32, position: u32, safe: [f32; 4]) -> Option<Rect> {
    if !fps.is_finite() || fps <= 0. {
        return None;
    }
    let half_h = ui.viewport.3 as f32 / ui.viewport.2.max(1) as f32;
    let position = position.min(5);
    let bottom = matches!(position, 1 | 4 | 5);
    let corner = position >= 2;
    let (x, ax) = match position {
        2 | 4 => (-1. + safe[0].max(0.05) + 0.004, 0.),
        3 | 5 => (1. - safe[2].max(0.05) - 0.004, 1.),
        _ => (0., 0.5),
    };
    let inset = if corner { 0.025 } else { 0.012 };
    let y = if bottom {
        half_h - safe[3].max(inset) - 0.004
    } else {
        -half_h + safe[1].max(inset) + 0.004
    };
    let text = format!("{fps:.0} FPS");
    let anchor_y = if bottom { 1. } else { 0. };
    let r = ui.text(&text).pos(x, y).anchor(ax, anchor_y).size(0.30).measure();
    ui.fill_rect(r.feather(0.004), Color::new(0., 0., 0., 0.35));
    ui.text(text).pos(x, y).anchor(ax, anchor_y).size(0.30).color(WHITE).draw();
    Some(r)
}
