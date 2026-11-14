//! Rotating font-atlas versions for the text painter.
//!
//! The glyph brush keeps one authoritative RGBA mirror on the CPU. Every glyph
//! update patches the mirror and records a dirty tile; the painter then hands
//! the change to an atlas version the GPU has finished sampling. A version the
//! driver may still reference is never overwritten, so glyph uploads no longer
//! wait on the previous frame's draw.
//!
//! The optimization is strictly additive: when the fence backend or the
//! allocation fails, the pool restores the original single-texture contract
//! before handing control back to the caller.

use macroquad::{prelude::*, window::get_internal_gl};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Long-lived UI painters (PGR font, default, bold). Four versions keep two
/// frames of draw latency covered without ever blocking on a fence.
const SLOTS: usize = 4;
const BUDGET: usize = 64 * 1024 * 1024;
/// Process-wide cap across painters: the first two may pool, later ones fall
/// back instead of quietly adding another 64 MiB each.
const TOTAL_BUDGET: usize = 2 * BUDGET;
const TILE: u32 = 32;

static RESERVED_BYTES: AtomicUsize = AtomicUsize::new(0);

fn reserve(bytes: usize) -> bool {
    let mut current = RESERVED_BYTES.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(bytes) else {
            return false;
        };
        if next > TOTAL_BUDGET {
            return false;
        }
        match RESERVED_BYTES.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

fn unreserve(bytes: usize) {
    if bytes > 0 {
        RESERVED_BYTES.fetch_sub(bytes, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasRect {
    point: (u32, u32),
    size: (u32, u32),
}

/// Change history: each 32x32 tile stores the last generation that touched it,
/// so a lagging version can be patched from the mirror without a journal.
pub struct Delta {
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
    generation: u64,
    versions: Vec<u64>,
}

impl Delta {
    fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0);
        let columns = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        Self {
            columns,
            rows,
            width,
            height,
            generation: 0,
            versions: vec![0; (columns * rows) as usize],
        }
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn advance(&mut self, writes: &[[u32; 4]]) {
        self.generation = self.generation.checked_add(1).expect("atlas generation overflow");
        for &[x0, y0, x1, y1] in writes {
            debug_assert!(x0 < x1 && y0 < y1 && x1 <= self.width && y1 <= self.height);
            for y in y0 / TILE..y1.div_ceil(TILE) {
                for x in x0 / TILE..x1.div_ceil(TILE) {
                    self.versions[(y * self.columns + x) as usize] = self.generation;
                }
            }
        }
    }

    fn since(&self, version: u64) -> Vec<AtlasRect> {
        debug_assert!(version <= self.generation);
        let mut rectangles: Vec<AtlasRect> = Vec::new();
        for row in 0..self.rows {
            let mut x = 0;
            while x < self.columns {
                if self.versions[(row * self.columns + x) as usize] <= version {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.columns && self.versions[(row * self.columns + x) as usize] > version {
                    x += 1;
                }
                let rect = AtlasRect {
                    point: (start * TILE, row * TILE),
                    size: ((x * TILE).min(self.width) - start * TILE, TILE.min(self.height - row * TILE)),
                };
                if let Some(previous) = rectangles.iter_mut().rev().find(|r| {
                    r.point.0 == rect.point.0 && r.size.0 == rect.size.0 && r.point.1 + r.size.1 == rect.point.1
                }) {
                    previous.size.1 += rect.size.1;
                } else {
                    rectangles.push(rect);
                }
            }
        }
        rectangles
    }
}

/// Zero-timeout sync objects. Android exposes core ES3 fences through EGL;
/// other targets keep the pool unavailable and use the original path.
#[cfg(target_os = "android")]
mod sync_api {
    use std::ffi::{c_char, c_void, CStr};

    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    }

    type Insert = unsafe extern "C" fn(u32, u32) -> *mut c_void;
    type Poll = unsafe extern "C" fn(*mut c_void, u32, u64) -> u32;
    type Delete = unsafe extern "C" fn(*mut c_void);

    pub type Fence = *mut c_void;

    #[derive(Clone, Copy)]
    pub struct Api {
        insert: Insert,
        poll: Poll,
        delete: Delete,
    }

    impl Api {
        pub fn load() -> Result<Self, ()> {
            unsafe {
                let pointer = miniquad::gl::glGetString(0x1f02);
                if pointer.is_null() {
                    return Err(());
                }
                let version = CStr::from_ptr(pointer.cast()).to_string_lossy();
                let major = version
                    .strip_prefix("OpenGL ES ")
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<u32>().ok());
                if major.unwrap_or(0) < 3 {
                    return Err(());
                }
                macro_rules! load {
                    ($name:literal, $ty:ty) => {{
                        let pointer = eglGetProcAddress(concat!($name, "\0").as_ptr().cast());
                        if pointer.is_null() {
                            return Err(());
                        }
                        std::mem::transmute::<*const c_void, $ty>(pointer)
                    }};
                }
                let api = Self {
                    insert: load!("glFenceSync", Insert),
                    poll: load!("glClientWaitSync", Poll),
                    delete: load!("glDeleteSync", Delete),
                };
                let fence = api.insert()?;
                let ready = api.ready(fence);
                api.delete(fence);
                ready?;
                if miniquad::gl::glGetError() != 0 {
                    return Err(());
                }
                Ok(api)
            }
        }

        pub fn insert(&self) -> Result<Fence, ()> {
            let fence = unsafe { (self.insert)(0x9117, 0) };
            if fence.is_null() {
                Err(())
            } else {
                Ok(fence)
            }
        }

        pub fn ready(&self, fence: Fence) -> Result<bool, ()> {
            match unsafe { (self.poll)(fence, 0, 0) } {
                0x911a | 0x911c => Ok(true),
                0x911b => Ok(false),
                _ => Err(()),
            }
        }

        pub fn delete(&self, fence: Fence) {
            unsafe {
                (self.delete)(fence);
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
mod sync_api {
    pub type Fence = usize;

    #[derive(Clone, Copy)]
    pub struct Api;

    impl Api {
        pub fn load() -> Result<Self, ()> {
            Err(())
        }
        pub fn insert(&self) -> Result<Fence, ()> {
            unreachable!()
        }
        pub fn ready(&self, _: Fence) -> Result<bool, ()> {
            unreachable!()
        }
        pub fn delete(&self, _: Fence) {}
    }
}

use sync_api::{Api, Fence};

struct Slot {
    texture: Texture2D,
    version: u64,
    fence: Option<Fence>,
}

pub struct Publication {
    pub texture: Texture2D,
    pub pooled: bool,
    /// `false` means the caller must stop drawing text until it recovers.
    pub valid: bool,
}

pub struct Pool {
    context: usize,
    api: Option<Api>,
    slots: Vec<Slot>,
    current: Option<Texture2D>,
    shape: (u32, u32),
    history: Option<Delta>,
    scratch: Vec<u8>,
    disabled: bool,
    reserved: usize,
}

fn context_id() -> usize {
    // The context object outlives every painter; its address is a stable token
    // for "same GL context" without relying on a miniquad API.
    unsafe { get_internal_gl() }.quad_context as *mut miniquad::Context as usize
}

fn flush_retirement() {
    unsafe { get_internal_gl() }.flush();
    unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
}

fn drain_errors() {
    unsafe {
        for _ in 0..32 {
            if miniquad::gl::glGetError() == 0 {
                break;
            }
        }
    }
}

fn bitmap_bytes(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return None;
    }
    (width as usize).checked_mul(height as usize)?.checked_mul(4)
}

fn pool_fits_budget(width: u32, height: u32) -> bool {
    bitmap_bytes(width, height)
        .and_then(|bytes| bytes.checked_mul(SLOTS))
        .is_some_and(|bytes| bytes <= BUDGET)
}

fn create_slot(width: u32, height: u32, shadow: &[u8]) -> Option<Texture2D> {
    let texture = Texture2D::from_miniquad_texture(miniquad::Texture::from_rgba8(
        unsafe { get_internal_gl() }.quad_context,
        width as u16,
        height as u16,
        shadow,
    ));
    texture.set_filter(FilterMode::Linear);
    texture
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Clamp);
    if unsafe { miniquad::gl::glGetError() } != 0 || texture.raw_miniquad_texture_handle().gl_internal_id() == 0 {
        flush_retirement();
        texture.delete();
        return None;
    }
    Some(texture)
}

impl Pool {
    pub fn new() -> Self {
        let context = context_id();
        let api = match Api::load() {
            Ok(api) => Some(api),
            Err(()) => {
                #[cfg(target_os = "android")]
                drain_errors();
                None
            }
        };
        Self {
            context,
            api,
            slots: Vec::new(),
            current: None,
            shape: (0, 0),
            history: None,
            scratch: Vec::new(),
            disabled: api.is_none(),
            reserved: 0,
        }
    }

    pub fn supported(&self) -> bool {
        !self.disabled && self.api.is_some() && self.context == context_id()
    }

    pub fn owns(&self, texture: Texture2D) -> bool {
        self.slots.iter().any(|slot| slot.texture == texture)
    }

    /// Full authoritative upload for every version. Used at painter creation
    /// and when the runtime switch re-enables the pool.
    pub fn warm(&mut self, width: u32, height: u32, shadow: &[u8]) -> Option<Texture2D> {
        if !self.supported() {
            return None;
        }
        let api = self.api?;
        let Some(bytes) = bitmap_bytes(width, height).filter(|&bytes| bytes == shadow.len() && pool_fits_budget(width, height)) else {
            self.disabled = true;
            return None;
        };
        if !self.slots.is_empty() {
            return (self.shape == (width, height)).then_some(self.current).flatten();
        }
        let need = bytes * SLOTS;
        if !reserve(need) {
            self.disabled = true;
            return None;
        }
        self.reserved = need;
        unsafe { get_internal_gl() }.flush();
        self.shape = (width, height);
        self.history = Some(Delta::new(width, height));
        if self.scratch.try_reserve_exact(1024 * 1024).is_err() {
            self.loading_failure();
            return None;
        }
        self.scratch.clear();
        for _ in 0..SLOTS {
            let Some(texture) = create_slot(width, height, shadow) else {
                self.loading_failure();
                return None;
            };
            let Ok(fence) = api.insert() else {
                flush_retirement();
                texture.delete();
                self.loading_failure();
                return None;
            };
            self.slots.push(Slot { texture, version: 0, fence: Some(fence) });
        }
        if unsafe { miniquad::gl::glGetError() } != 0 {
            self.loading_failure();
            return None;
        }
        self.current = Some(self.slots[0].texture);
        self.current
    }

    fn loading_failure(&mut self) {
        unreserve(self.reserved);
        self.reserved = 0;
        if !self.slots.is_empty() {
            flush_retirement();
        }
        for slot in self.slots.drain(..) {
            if let (Some(api), Some(fence)) = (self.api, slot.fence) {
                api.delete(fence);
            }
            slot.texture.delete();
        }
        self.current = None;
        self.history = None;
        self.shape = (0, 0);
        self.disabled = true;
        drain_errors();
    }

    /// The caller keeps `previous` alive: it may still be sampled by draws
    /// already submitted, and it is the recovery target if this fails.
    pub fn publish(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], writes: &[[u32; 4]]) -> Publication {
        if !self.supported() || self.context != context_id() {
            return self.mutable_fallback(previous, width, height, shadow);
        }
        if bitmap_bytes(width, height) != Some(shadow.len())
            || writes.iter().any(|&[x0, y0, x1, y1]| x0 >= x1 || y0 >= y1 || x1 > width || y1 > height)
        {
            return self.mutable_fallback(previous, width, height, shadow);
        }
        if self.slots.is_empty() {
            return match self.warm(width, height, shadow) {
                Some(texture) => Publication { texture, pooled: true, valid: true },
                None => self.mutable_fallback(previous, width, height, shadow),
            };
        }
        let api = self.api.unwrap();
        self.history.as_mut().unwrap().advance(writes);
        let Some(current) = self.slots.iter().position(|slot| Some(slot.texture) == self.current) else {
            return self.mutable_fallback(previous, width, height, shadow);
        };
        let fence = match api.insert() {
            Ok(fence) => fence,
            Err(()) => return self.mutable_fallback(previous, width, height, shadow),
        };
        if let Some(old) = self.slots[current].fence.replace(fence) {
            api.delete(old);
        }
        let mut available = None;
        for (index, slot) in self.slots.iter().enumerate() {
            if index == current {
                continue;
            }
            let ready = match slot.fence {
                Some(fence) => api.ready(fence),
                None => Ok(true),
            };
            match ready {
                Ok(true) => {
                    available = Some(index);
                    break;
                }
                Ok(false) => {}
                Err(()) => return self.mutable_fallback(previous, width, height, shadow),
            }
        }
        if let Some(index) = available {
            if let Some(fence) = self.slots[index].fence.take() {
                api.delete(fence);
            }
            let version = self.history.as_ref().unwrap().generation();
            let rectangles = self.history.as_ref().unwrap().since(self.slots[index].version);
            let texture = self.slots[index].texture;
            if self.upload_delta(texture, width, shadow, &rectangles).is_err() {
                return self.mutable_fallback(previous, width, height, shadow);
            }
            self.slots[index].version = version;
            self.current = Some(texture);
            return Publication { texture, pooled: true, valid: true };
        }
        // Every alternate version may still be sampled. Retire one that is
        // neither the caller's image nor the current draw source, then pay for
        // a full upload instead of blocking.
        let retire = self
            .slots
            .iter()
            .position(|slot| slot.texture != previous && Some(slot.texture) != self.current);
        let Some(retire) = retire else {
            return self.mutable_fallback(previous, width, height, shadow);
        };
        flush_retirement();
        let old = self.slots.remove(retire);
        if let Some(fence) = old.fence {
            api.delete(fence);
        }
        old.texture.delete();
        let Some(texture) = create_slot(width, height, shadow) else {
            return self.mutable_fallback(previous, width, height, shadow);
        };
        self.slots.push(Slot { texture, version: self.history.as_ref().unwrap().generation(), fence: None });
        self.current = Some(texture);
        Publication { texture, pooled: true, valid: true }
    }

    fn upload_delta(&mut self, texture: Texture2D, width: u32, shadow: &[u8], rectangles: &[AtlasRect]) -> Result<(), ()> {
        let context = unsafe { get_internal_gl() }.quad_context;
        for rect in rectangles {
            self.scratch.clear();
            let needed = (rect.size.0 as usize) * (rect.size.1 as usize) * 4;
            if self.scratch.try_reserve(needed).is_err() {
                return Err(());
            }
            for y in rect.point.1..rect.point.1 + rect.size.1 {
                let start = ((y * width + rect.point.0) * 4) as usize;
                self.scratch.extend_from_slice(&shadow[start..start + (rect.size.0 * 4) as usize]);
            }
            texture.raw_miniquad_texture_handle().update_texture_part(
                context,
                rect.point.0 as i32,
                rect.point.1 as i32,
                rect.size.0 as i32,
                rect.size.1 as i32,
                &self.scratch,
            );
        }
        if unsafe { miniquad::gl::glGetError() } != 0 {
            return Err(());
        }
        Ok(())
    }

    /// Restore the single-texture contract: full upload into the caller's
    /// texture, drop every pool version, and stay mutable from now on.
    fn mutable_fallback(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8]) -> Publication {
        self.disabled = true;
        let valid = if bitmap_bytes(width, height) == Some(shadow.len())
            && previous.raw_miniquad_texture_handle().width == width
            && previous.raw_miniquad_texture_handle().height == height
        {
            previous
                .raw_miniquad_texture_handle()
                .update(unsafe { get_internal_gl() }.quad_context, shadow);
            (unsafe { miniquad::gl::glGetError() }) == 0
        } else {
            false
        };
        self.release(previous);
        drain_errors();
        Publication { texture: previous, pooled: false, valid }
    }

    /// Drop every version except `keep`, which the caller keeps using as an
    /// ordinary mutable texture.
    fn release(&mut self, keep: Texture2D) {
        unreserve(self.reserved);
        self.reserved = 0;
        if !self.slots.is_empty() {
            flush_retirement();
        }
        for slot in self.slots.drain(..) {
            if let (Some(api), Some(fence)) = (self.api, slot.fence) {
                api.delete(fence);
            }
            if slot.texture != keep {
                slot.texture.delete();
            }
        }
        self.current = None;
        self.history = None;
        self.shape = (0, 0);
    }

    /// The caller needs the texture back as a plain mutable image (switch off).
    pub fn release_to_caller(&mut self, keep: Texture2D, width: u32, height: u32, shadow: &[u8]) {
        if bitmap_bytes(width, height) == Some(shadow.len()) {
            keep.raw_miniquad_texture_handle()
                .update(unsafe { get_internal_gl() }.quad_context, shadow);
        }
        self.release(keep);
        drain_errors();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        unreserve(self.reserved);
        self.reserved = 0;
        if self.context != context_id() {
            return;
        }
        if !self.slots.is_empty() {
            flush_retirement();
        }
        for slot in self.slots.drain(..) {
            if let (Some(api), Some(fence)) = (self.api, slot.fence) {
                api.delete(fence);
            }
            crate::ext::queue_texture_deletion(slot.texture);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(image: &mut [u8], source: &[u8], width: u32, rect: AtlasRect) {
        for y in rect.point.1..rect.point.1 + rect.size.1 {
            let from = ((y * width + rect.point.0) * 4) as usize;
            let end = from + (rect.size.0 * 4) as usize;
            image[from..end].copy_from_slice(&source[from..end]);
        }
    }

    #[test]
    fn staggered_versions_reconstruct_the_exact_bitmap() {
        let (w, h) = (103, 77);
        let mut delta = Delta::new(w, h);
        let mut source = vec![0; (w * h * 4) as usize];
        let mut slots = vec![(0u64, source.clone()); 4];
        let mut random = 7u32;
        for frame in 0..1000 {
            let mut writes = Vec::new();
            for _ in 0..1 + frame % 5 {
                let mut next = || {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    random
                };
                let x0 = next() % w;
                let y0 = next() % h;
                let x1 = (x0 + 1 + next() % 39).min(w);
                let y1 = (y0 + 1 + next() % 35).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        for c in 0..4 {
                            source[((y * w + x) * 4 + c) as usize] = next() as u8;
                        }
                    }
                }
                writes.push([x0, y0, x1, y1]);
            }
            delta.advance(&writes);
            let index = (frame / 3 % 4) as usize;
            let (version, image) = &mut slots[index];
            for rect in delta.since(*version) {
                apply(image, &source, w, rect);
            }
            assert_eq!(*image, source, "publication {frame}");
            *version = delta.generation();
        }
    }

    #[test]
    fn budget_rounding_is_conservative() {
        assert_eq!(bitmap_bytes(2048, 2048), Some(16 * 1024 * 1024));
        assert!(pool_fits_budget(2048, 2048));
        assert!(!pool_fits_budget(2048, 2049));
        assert!(!pool_fits_budget(4096, 2048));
        assert_eq!(bitmap_bytes(0, 1), None);
        assert_eq!(bitmap_bytes(65536, 1), None);
    }
}
