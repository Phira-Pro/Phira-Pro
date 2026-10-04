//! Opaque upper/lower practice covers, rendered before gameplay HUD.
use super::Resource;
use crate::{
    config::{Config, Mods},
    ext::SafeTexture,
};
use macroquad::prelude::*;

pub(super) fn load(config: &Config) -> [Option<SafeTexture>; 2] {
    [&config.hide_upper_image, &config.hide_lower_image].map(|path| {
        let path = path.as_ref()?;
        if std::fs::metadata(path).ok()?.len() > 32 * 1024 * 1024 {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(2048);
        limits.max_image_height = Some(2048);
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().ok()?.to_rgba8();
        if image.width() > 2048 || image.height() > 2048 {
            return None;
        }
        Some(Texture2D::from_rgba8(image.width() as u16, image.height() as u16, image.as_raw()).into())
    })
}

pub(crate) fn draw(res: &Resource) {
    let side = if res.config.has_mod(Mods::FADE_IN) {
        0
    } else if res.config.has_mod(Mods::FADE_OUT) {
        1
    } else {
        return;
    };
    let full_height = 2. / res.aspect_ratio;
    let fraction = 0.1 + 0.8 * res.config.fade_strength.clamp(0., 1.);
    let height = full_height * fraction;
    let y = if side == 0 { -full_height / 2. } else { full_height / 2. - height };
    let color = if side == 0 {
        res.config.hide_upper_color
    } else {
        res.config.hide_lower_color
    };
    // Imported alpha never exposes the supposedly hidden notes.
    draw_rectangle(-1., y, 2., height, Color::from_hex_rgb(color));
    if let Some(texture) = &res.hide_covers[side] {
        let max_height = full_height * 0.9;
        let scale = (2. / texture.width()).max(max_height / texture.height());
        let w = 2. / scale;
        let max_h = max_height / scale;
        let h = height / scale;
        let x = (texture.width() - w) / 2.;
        let top = (texture.height() - max_h) / 2.;
        let source = Rect::new(x, if side == 0 { top } else { top + max_h - h }, w, h);
        draw_texture_ex(
            **texture,
            -1.,
            y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(2., height)),
                source: Some(source),
                ..Default::default()
            },
        );
    }
}
