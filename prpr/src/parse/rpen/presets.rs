//! Index of RPEN's shader presets.
//!
//! The shader sources live in `assets/rpen/shaders` and are loaded at runtime as
//! separate resource files; they are never compiled into this program. See
//! `assets/rpen/NOTICE.txt` for their separate license.
use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

pub fn file(name: &str) -> Option<&'static str> {
    Some(match name {
        "bokeh" => "rpen/shaders/bokeh_pr.glsl",
        "burn" => "rpen/shaders/burn_pr.glsl",
        "camera" => "rpen/shaders/camera_pr.glsl",
        "color_shift" => "rpen/shaders/color_shift_pr.glsl",
        "distortion" => "rpen/shaders/distortion_pr.glsl",
        "emboss" => "rpen/shaders/emboss_pr.glsl",
        "flare" => "rpen/shaders/flare_pr.glsl",
        "flip" => "rpen/shaders/flip_pr.glsl",
        "flowing" => "rpen/shaders/flowing_pr.glsl",
        "fog" => "rpen/shaders/fog_pr.glsl",
        "glow_effect" => "rpen/shaders/glow_effect_pr.glsl",
        "heat_distortion" => "rpen/shaders/heat_distortion_pr.glsl",
        "hologram" => "rpen/shaders/hologram_pr.glsl",
        "image_noise" => "rpen/shaders/image_noise_pr.glsl",
        "kaleidoscope" => "rpen/shaders/kaleidoscope_pr.glsl",
        "lightning" => "rpen/shaders/lightning_pr.glsl",
        "liquid" => "rpen/shaders/liquid_pr.glsl",
        "neon" => "rpen/shaders/neon_pr.glsl",
        "night_vision" => "rpen/shaders/night_vision_pr.glsl",
        "oil_painting" => "rpen/shaders/oil_painting_pr.glsl",
        "old_tv" => "rpen/shaders/old_tv_pr.glsl",
        "rain" => "rpen/shaders/rain_pr.glsl",
        "rainbow" => "rpen/shaders/rainbow_pr.glsl",
        "rays" => "rpen/shaders/rays_pr.glsl",
        "scanline" => "rpen/shaders/scanline_pr.glsl",
        "snow" => "rpen/shaders/snow_pr.glsl",
        "sobel" => "rpen/shaders/sobel_pr.glsl",
        "two_tone" => "rpen/shaders/two_tone_pr.glsl",
        "underwater" => "rpen/shaders/underwater_pr.glsl",
        "wave" => "rpen/shaders/wave_pr.glsl",
        _ => return None,
    })
}

fn cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub async fn source(name: &str) -> Result<String> {
    let path = file(name).with_context(|| format!("unknown RPEN shader {name}"))?;
    if let Some(source) = cache().lock().unwrap().get(name) {
        return Ok(source.clone());
    }
    let bytes = macroquad::file::load_file(path)
        .await
        .with_context(|| format!("missing RPEN shader asset {path}"))?;
    let source = String::from_utf8(bytes).with_context(|| format!("invalid UTF-8 in RPEN shader asset {path}"))?;
    cache().lock().unwrap().insert(name.to_owned(), source.clone());
    Ok(source)
}
