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
    illustration_with_blur(bytes, 0.5).await
}

pub async fn illustration_with_blur(bytes: Vec<u8>, amount: f32) -> Result<Illustration> {
    crate::loading_work::run(move || decode_illustration(&bytes, amount)).await
}

/// Synchronous CPU entry for ordinary background tasks (no GL and no frame executor).
pub fn decode_illustration(bytes: &[u8], amount: f32) -> Result<Illustration> {
    let image = image::load_from_memory(bytes).context("Failed to decode image")?;
    let (w, h) = (image.width(), image.height());
    let size = w as usize * h as usize;
    let mut blurred_rgb = image.to_rgb8();
    let color = color_thief::get_palette(&blurred_rgb, color_thief::ColorFormat::Rgb, 10, 2)?[0];
    // Preserve the existing kernel, radius, pixel format and iteration order.
    let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute::<*mut u8, *mut [u8; 3]>(blurred_rgb.as_mut_ptr()), size, size) };
    let amount = if amount.is_finite() { amount.clamp(0., 1.) } else { 0.5 };
    // 0.50 preserves the original 50px blur. Larger values expand its radius.
    let sigma = if amount <= 0.5 {
        amount * 100.
    } else {
        50. + (amount - 0.5) * 2. * (w.min(h) as f32 * 0.5).max(50.)
    };
    if sigma > 0. {
        fastblur::gaussian_blur(&mut vec, w as _, h as _, sigma);
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn background_blur_zero_is_sharp_and_midpoint_preserves_the_old_default() {
        fn finish<T>(future: impl std::future::Future<Output = T>) -> T {
            let mut future = Box::pin(future);
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            loop {
                if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                    return value;
                }
                std::thread::yield_now();
            }
        }
        finish(async {
            let source =
                image::RgbaImage::from_fn(256, 128, |x, y| image::Rgba([if x < 128 { 255 } else { 0 }, if y < 64 { 255 } else { 0 }, 64, 255]));
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(source.clone())
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            let sharp = super::illustration_with_blur(png.get_ref().clone(), 0.).await.unwrap();
            assert_eq!(sharp.blurred, source.into_raw());
            let original = super::illustration(png.get_ref().clone()).await.unwrap();
            let middle = super::illustration_with_blur(png.get_ref().clone(), 0.5).await.unwrap();
            assert_eq!(original.blurred, middle.blurred);
            assert_ne!(middle.blurred, sharp.blurred);
            let maximum = super::illustration_with_blur(png.into_inner(), 1.).await.unwrap();
            let contrast = |p: &[u8]| p.chunks_exact(4).map(|c| (c[0] as f32 - 127.5).abs()).sum::<f32>();
            assert!(contrast(&maximum.blurred) < contrast(&middle.blurred));
        });
    }
}
