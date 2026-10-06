//! MSAA lifetime belongs to a chart target, never to a process-wide switch.
//! An implicit target is eligible only when no layer observes its samples before
//! the final resolve. Unsupported or mismatched capabilities keep explicit MSAA.

type Attach = unsafe extern "C" fn(u32, u32, u32, u32, i32, i32);
type AttachmentQuery = unsafe extern "C" fn(u32, u32, u32, *mut i32);
#[cfg(target_os = "android")]
type SampleQuery = unsafe extern "C" fn(u32, u32, *mut f32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub retain: bool,
    implicit: bool,
}

impl Policy {
    pub const LEGACY: Self = Self {
        retain: false,
        implicit: false,
    };

    pub fn continuous(eligible: bool, samples: u32) -> Self {
        Self {
            retain: samples > 1,
            implicit: samples > 1 && eligible,
        }
    }

    pub fn implicit_supported(self) -> bool {
        self.implicit && implicit_api().is_some()
    }

    pub fn source_unverified(self) {
        tracing::warn!("Keeping explicit MSAA: cannot verify original RGB8 renderbuffer");
    }
}

#[cfg(target_os = "android")]
unsafe fn entry(name: &std::ffi::CStr) -> *const std::ffi::c_void {
    use std::ffi::{c_char, c_void};
    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    }
    unsafe { eglGetProcAddress(name.as_ptr()) }
}

#[cfg(target_os = "android")]
fn gl_string(name: u32) -> Option<String> {
    let value = unsafe { miniquad::gl::glGetString(name) };
    if value.is_null() {
        return None;
    }
    Some(unsafe { std::ffi::CStr::from_ptr(value as _) }.to_string_lossy().into_owned())
}

fn implicit_api() -> Option<(Attach, AttachmentQuery)> {
    #[cfg(target_os = "android")]
    unsafe {
        // Query the current context when allocating, rather than caching support
        // across activity/context recreation. No capability query occurs per draw.
        if !gl_string(0x1F02)?.starts_with("OpenGL ES 3.") {
            return None;
        }
        if !gl_string(0x1F03)?
            .split_whitespace()
            .any(|value| value == "GL_EXT_multisampled_render_to_texture")
        {
            return None;
        }
        let attach = entry(c"glFramebufferTexture2DMultisampleEXT");
        let query = entry(c"glGetFramebufferAttachmentParameteriv");
        if !attach.is_null() && !query.is_null() {
            return Some((
                std::mem::transmute::<*const std::ffi::c_void, Attach>(attach),
                std::mem::transmute::<*const std::ffi::c_void, AttachmentQuery>(query),
            ));
        }
    }
    None
}

pub unsafe fn create_implicit_target(texture: u32, samples: i32, source: u32, policy: Policy) -> Option<(u32, i32)> {
    if !policy.implicit {
        return None;
    }
    let (attach, query) = implicit_api()?;
    let Some(source_positions) = (unsafe { sample_positions(source, samples) }) else {
        tracing::warn!("Keeping explicit MSAA: cannot verify original sample positions");
        return None;
    };
    unsafe {
        use miniquad::gl::*;
        let mut previous = 0;
        glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut previous);
        let mut fbo = 0;
        glGenFramebuffers(1, &mut fbo);
        glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        attach(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0, samples);
        let mut actual = 0;
        query(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, 0x8D6C, &mut actual);
        // Do not infer component precision or encoding from an extension name.
        // The original chart source and every effect boundary are linear RGB8.
        let mut color = [0; 6];
        for (value, name) in color.iter_mut().zip([0x8212, 0x8213, 0x8214, 0x8215, 0x8211, 0x8210]) {
            query(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, name, value);
        }
        let format_matches = color == [8, 8, 8, 0, 0x8C17, 0x2601];
        let complete = glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
        let error = glGetError();
        let positions_match = complete
            && actual == samples
            && format_matches
            && error == GL_NO_ERROR
            && sample_positions(fbo, actual).is_some_and(|positions| positions == source_positions);
        if positions_match {
            return Some((fbo, actual));
        }
        glBindFramebuffer(GL_FRAMEBUFFER, previous as _);
        glDeleteFramebuffers(1, &fbo);

        tracing::warn!("Keeping explicit MSAA: implicit samples {actual}/{samples}, color {color:?}, complete {complete}, sample positions match {positions_match}, GL error {error:#x}");
        None
    }
}

unsafe fn sample_positions(framebuffer: u32, samples: i32) -> Option<Vec<[f32; 2]>> {
    #[cfg(target_os = "android")]
    unsafe {
        use miniquad::gl::*;
        let version = gl_string(0x1F02)?;
        // glGetMultisamplefv is core in ES3.1. ES3.0 keeps explicit MSAA.
        if version.starts_with("OpenGL ES 3.0") || !(1..=64).contains(&samples) {
            return None;
        }
        let pointer = entry(c"glGetMultisamplefv");
        if pointer.is_null() {
            return None;
        }
        let query = std::mem::transmute::<*const std::ffi::c_void, SampleQuery>(pointer);
        let mut previous = 0;
        glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut previous);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, framebuffer);
        let mut actual = 0;
        glGetIntegerv(0x80A9, &mut actual);
        let mut positions = vec![[0.; 2]; samples as usize];
        if actual == samples {
            for (index, position) in positions.iter_mut().enumerate() {
                query(0x8E50, index as _, position.as_mut_ptr());
            }
        }
        let error = glGetError();
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, previous as _);
        if actual == samples && error == GL_NO_ERROR && positions.iter().flatten().all(|v| v.is_finite() && (0. ..=1.).contains(v)) {
            return Some(positions);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = (framebuffer, samples);
    None
}

#[cfg(target_os = "android")]
unsafe fn renderbuffer_info(rbo: u32) -> Option<serde_json::Value> {
    type Query = unsafe extern "C" fn(u32, u32, *mut i32);
    unsafe {
        let pointer = entry(c"glGetRenderbufferParameteriv");
        if pointer.is_null() {
            return None;
        }
        let query = std::mem::transmute::<*const std::ffi::c_void, Query>(pointer);
        let mut old = 0;
        miniquad::gl::glGetIntegerv(0x8CA7, &mut old);
        miniquad::gl::glBindRenderbuffer(0x8D41, rbo);
        let mut values = [0; 7];
        for (value, name) in values.iter_mut().zip([0x8CAB, 0x8D42, 0x8D43, 0x8D44, 0x8D50, 0x8D51, 0x8D52]) {
            query(0x8D41, name, value);
        }
        miniquad::gl::glBindRenderbuffer(0x8D41, old as _);
        if miniquad::gl::glGetError() != 0 {
            return None;
        }
        Some(serde_json::json!({"actual_samples":values[0], "width":values[1], "height":values[2],
            "internal_format":values[3], "red_bits":values[4], "green_bits":values[5], "blue_bits":values[6]}))
    }
}

#[cfg(target_os = "android")]
pub unsafe fn renderbuffer_samples(rbo: u32) -> Option<i32> {
    let info = unsafe { renderbuffer_info(rbo) }?;
    let samples = i32::try_from(info["actual_samples"].as_i64()?).ok()?;
    (samples > 1
        && info["internal_format"].as_i64()? == 0x8051
        && ["red_bits", "green_bits", "blue_bits"].iter().all(|key| info[*key].as_i64() == Some(8)))
    .then_some(samples)
}
