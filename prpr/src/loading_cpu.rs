//! The original decoders and full-resolution blur, separated from GL uploads.
use anyhow::{Context, Result};

pub struct Illustration {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub blurred: Vec<u8>,
    pub color: [u8; 3],
}

pub async fn illustration(bytes: Vec<u8>) -> Result<Illustration> {
    crate::loading_work::run(move || {
        let image = image::load_from_memory(&bytes).context("Failed to decode image")?;
        let (w, h) = (image.width(), image.height());
        let size = w as usize * h as usize;
        let mut blurred_rgb = image.to_rgb8();
        let color = color_thief::get_palette(&blurred_rgb, color_thief::ColorFormat::Rgb, 10, 2)?[0];
        // Preserve the existing kernel, radius, pixel format and iteration order.
        let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute::<*mut u8, *mut [u8; 3]>(blurred_rgb.as_mut_ptr()), size, size) };
        fastblur::gaussian_blur(&mut vec, w as _, h as _, 50.);
        std::mem::forget(vec);
        let mut blurred = Vec::with_capacity(size * 4);
        for input in blurred_rgb.chunks_exact(3) {
            blurred.extend_from_slice(input);
            blurred.push(255);
        }
        Ok(Illustration {
            width: w,
            height: h,
            rgba: image.into_rgba8().into_raw(),
            blurred,
            color: [color.r, color.g, color.b],
        })
    })
    .await
}

pub async fn image(bytes: Vec<u8>) -> Result<image::DynamicImage> {
    crate::loading_work::run(move || Ok(image::load_from_memory(&bytes)?)).await
}

pub async fn audio(bytes: Vec<u8>) -> Result<sasa::AudioClip> {
    crate::loading_work::run(move || {
        // AudioClip::new is exactly decode -> from_raw, with no device access.
        let (frames, rate) = sasa::AudioClip::decode(bytes)?;
        Ok(sasa::AudioClip::from_raw(frames, rate))
    })
    .await
}
