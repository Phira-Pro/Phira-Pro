use super::PresentationAssets;
use crate::{
    core::PGR_FONT,
    ext::ScaleType,
    ui::{DrawText, IntoShading, Ui},
};
use macroquad::prelude::*;

/// Horizontal coordinates use canvas width. Vertical coordinates share a
/// 4:3 reference centered on the chosen top/center/bottom height anchor.
pub fn point(top: f32, x: f32, y: f32, height_anchor: f32) -> Vec2 {
    vec2(x * 2. - 1., top * (2. * height_anchor - 1.) + (y - height_anchor) * 1.5)
}
pub fn rect(top: f32, x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(x * 2. - 1., y * 2. - top, width * 2., height * 2.)
}
pub fn centered(_top: f32, x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(x * 2. - 1., y * 2., width * 2., height * 2.)
}
fn paint(builder: &mut DrawText, assets: &PresentationAssets, measure: bool) -> Rect {
    if let Some(font) = &assets.font {
        let mut font = font.borrow_mut();
        if measure {
            builder.measure_with_font(Some(&mut font))
        } else {
            builder.draw_with_font(Some(&mut font))
        }
    } else if measure {
        builder.measure_using(&PGR_FONT)
    } else {
        builder.draw_using(&PGR_FONT)
    }
}
pub fn width(ui: &mut Ui, assets: &PresentationAssets, text: &str, em: f32) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    paint(&mut ui.text(text).size(em * 25.).no_baseline(), assets, true).w / 2. + em / 3.
}
/// Single-line shrink-to-fit retains the layout bands even for very long titles.
#[allow(clippy::too_many_arguments)]
pub fn text(
    ui: &mut Ui,
    assets: &PresentationAssets,
    value: &str,
    x: f32,
    y: f32,
    height_anchor: f32,
    em: f32,
    max_width: f32,
    anchor: Vec2,
    color: Color,
) -> Rect {
    let measured = width(ui, assets, value, em);
    let em = em * (max_width / measured.max(1e-6)).min(1.);
    let mut position = point(ui.top, x, y, height_anchor);
    // GDI+ layout reserves one sixth of an em on either side. Its vertical
    // calibration places the top of a capital L at 192/1000 of the em.
    position.x += em * (1. - 2. * anchor.x) / 3.;
    position.y += em * (0.384 - 1.5 * anchor.y);
    paint(
        &mut ui
            .text(value)
            .pos(position.x, position.y)
            .size(em * 25.)
            .no_baseline()
            .anchor(anchor.x, 0.)
            .color(color),
        assets,
        false,
    )
}
pub fn bottom_shade(ui: &mut Ui, r: Rect, fraction: f32, opacity: f32) {
    let height = r.h * fraction;
    let start = r.bottom() - height;
    let slant = r.h * 0.26f32.tan();
    let mut mesh = ui.builder((Color::new(0., 0., 0., 0.), (0., start), Color::new(0., 0., 0., opacity), (0., r.bottom())));
    for p in [
        vec2(r.x + slant * fraction, start),
        vec2(r.right() - slant * (1. - fraction), start),
        vec2(r.right() - slant, r.bottom()),
        vec2(r.x, r.bottom()),
    ] {
        mesh.add(p.x, p.y);
    }
    mesh.triangle(0, 1, 2);
    mesh.triangle(0, 2, 3);
    mesh.commit();
}
/// The panel geometry includes the same transparent padding as the reference
/// layout; reconstruct the shadow and solid interior without a raster asset.
pub fn panel(ui: &mut Ui, outer: Rect, reference_width: f32) {
    let padding = outer.w * 50. / reference_width;
    let inner = Rect::new(outer.x + padding, outer.y + padding, outer.w - padding * 2., outer.h - padding * 2.);
    for i in (1..=8).rev() {
        let margin = padding * i as f32 / 8.;
        slanted(ui, Rect::new(inner.x - margin, inner.y - margin, inner.w + margin * 2., inner.h + margin * 2.), None, Color::new(0., 0., 0., 0.014));
    }
    slanted(ui, inner, None, Color::new(0., 0., 0., 0.44));
}
pub fn slanted(ui: &mut Ui, r: Rect, texture: Option<Texture2D>, color: Color) {
    let slant = r.h * 0.26f32.tan();
    fn shape(ui: &mut Ui, r: Rect, slant: f32, shading: impl IntoShading) {
        let mut mesh = ui.builder(shading);
        mesh.add(r.x + slant, r.y);
        mesh.add(r.right(), r.y);
        mesh.add(r.right() - slant, r.bottom());
        mesh.add(r.x, r.bottom());
        mesh.triangle(0, 1, 2);
        mesh.triangle(0, 2, 3);
        mesh.commit();
    }
    if let Some(texture) = texture {
        shape(ui, r, slant, (texture, r, ScaleType::CropCenter, color));
    } else {
        shape(ui, r, slant, color);
    }
}
pub fn line(ui: &mut Ui, from: Vec2, to: Vec2, width: f32, color: Color) {
    let direction = (to - from).normalize_or_zero();
    let side = vec2(-direction.y, direction.x) * width / 2.;
    let mut mesh = ui.builder(color);
    for p in [from - side, to - side, to + side, from + side] {
        mesh.add(p.x, p.y);
    }
    mesh.triangle(0, 1, 2);
    mesh.triangle(0, 2, 3);
    mesh.commit();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn center_anchor_keeps_composition_centered_when_aspect_changes() {
        for top in [0.375, 0.5625, 0.75, 1.] {
            assert_eq!(point(top, 0.5, 0.5, 0.5), Vec2::ZERO);
            assert_eq!(point(top, 0.5, 0., 0.).y, -top);
            assert_eq!(point(top, 0.5, 1., 1.).y, top);
        }
    }
}
