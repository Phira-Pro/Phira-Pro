//! The original decoders and full-resolution blur, separated from GL uploads.
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub struct Illustration {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub blurred: Vec<u8>,
    pub color: [u8; 3],
}

pub async fn illustration(bytes: Vec<u8>) -> Result<Illustration> {
    let validate = crate::loading_work::validate_outputs();
    crate::loading_work::run_with_proof("illustration_decode_palette_blur", move || {
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
        let value = Illustration {
            width: w,
            height: h,
            rgba: image.into_rgba8().into_raw(),
            blurred,
            color: [color.r, color.g, color.b],
        };
        let proof = validate.then(|| {
            serde_json::json!({"width":w,"height":h,"color":value.color,
            "rgba_sha256":format!("{:x}",Sha256::digest(&value.rgba)),
            "blurred_sha256":format!("{:x}",Sha256::digest(&value.blurred))})
        });
        Ok((value, proof))
    })
    .await
}

pub async fn image(bytes: Vec<u8>, label: &str) -> Result<image::DynamicImage> {
    let validate = crate::loading_work::validate_outputs();
    crate::loading_work::run_with_proof(label, move || {
        let image = image::load_from_memory(&bytes)?;
        let proof = validate.then(|| {
            serde_json::json!({"width":image.width(),"height":image.height(),
            "rgba_sha256":format!("{:x}",Sha256::digest(image.to_rgba8().as_raw()))})
        });
        Ok((image, proof))
    })
    .await
}

/// Use macroquad's original decoder/version, yielding only owned CPU pixels.
/// No texture, JNI file access, GL state or audio device enters this worker.
pub async fn ui_image(bytes: Vec<u8>, path: &'static str, allow_worker: bool) -> Result<macroquad::texture::Image> {
    let validate = crate::loading_work::validate_outputs();
    crate::loading_work::run_selected_with_proof(&format!("ui_asset:{path}"), allow_worker, move || {
        let image = macroquad::texture::Image::from_file_with_format(&bytes, None);
        let proof = validate.then(|| serde_json::json!({
            "width":image.width,"height":image.height,
            "rgba_sha256":format!("{:x}",Sha256::digest(&image.bytes))
        }));
        Ok((image, proof))
    }).await
}

pub async fn audio(bytes: Vec<u8>, label: &str) -> Result<sasa::AudioClip> {
    let validate = crate::loading_work::validate_outputs();
    crate::loading_work::run_with_proof(label, move || {
        // AudioClip::new is exactly decode -> from_raw, with no device access.
        let (frames, rate) = sasa::AudioClip::decode(bytes)?;
        let proof = validate.then(|| {
            let mut hash = Sha256::new();
            for frame in &frames {
                hash.update(frame.0.to_bits().to_le_bytes());
                hash.update(frame.1.to_bits().to_le_bytes());
            }
            serde_json::json!({"sample_rate":rate,"frames":frames.len(),"stereo_f32_bits_sha256":format!("{:x}",hash.finalize())})
        });
        Ok((sasa::AudioClip::from_raw(frames, rate), proof))
    })
    .await
}
