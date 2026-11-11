//! Common RGBA font atlas versions. Bitmap representatives, processing order,
//! packing, UVs and filtering stay in TextPainter/glyph_brush. Only completed
//! image versions are updated; no client wait has a nonzero timeout.
use macroquad::{prelude::*, window::get_internal_gl};
#[path = "atlas_delta.rs"]
mod delta;
const SLOTS: usize = 4;
const BUDGET: usize = 64 * 1024 * 1024;

#[cfg(feature = "render_research")]
thread_local! {
    static IMMUTABLE_REQUEST: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((false, false)) };
    static IMMUTABLE_FAILURE_CONTEXT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
#[cfg(feature = "render_research")]
pub(crate) fn configure_immutable(requested: bool, force_missing: bool) {
    IMMUTABLE_REQUEST.with(|value| value.set((requested, force_missing)));
    IMMUTABLE_FAILURE_CONTEXT.with(|value| value.set(None));
}
#[cfg(feature = "render_research")]
pub(crate) fn immutable_context_blocked() -> bool {
    IMMUTABLE_FAILURE_CONTEXT.with(|value| value.get().is_some_and(|context| context == context_id()))
}
#[cfg(feature = "render_research")]
#[derive(Default)]
struct ImmutableStats {
    requested: bool,
    force_missing: bool,
    attempts: u64,
    accepted_images: u64,
    initialized_rgba_bytes: u64,
    declined_images: u64,
    mutable_images: u64,
    fatal_errors: u64,
    blocked: bool,
    reason: Option<String>,
}

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
        pub fn load() -> Result<Self, String> {
            unsafe {
                let pointer = miniquad::gl::glGetString(0x1f02);
                if pointer.is_null() {
                    return Err("No current GL context".into());
                }
                let version = CStr::from_ptr(pointer.cast()).to_string_lossy();
                let major = version
                    .strip_prefix("OpenGL ES ")
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<u32>().ok());
                if major.unwrap_or(0) < 3 {
                    return Err(format!("Core ES3 sync unavailable: {version}"));
                }
                macro_rules! load {
                    ($name:literal,$ty:ty) => {{
                        let pointer = eglGetProcAddress(concat!($name, "\0").as_ptr().cast());
                        if pointer.is_null() {
                            return Err(concat!("Missing ", $name).into());
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
                let checked = api.ready(fence);
                api.delete(fence);
                checked?;
                if miniquad::gl::glGetError() != 0 {
                    return Err("Core sync probe GL error".into());
                }
                Ok(api)
            }
        }
        pub fn insert(&self) -> Result<Fence, String> {
            let fence = unsafe { (self.insert)(0x9117, 0) };
            if fence.is_null() {
                Err("glFenceSync returned null".into())
            } else {
                Ok(fence)
            }
        }
        pub fn ready(&self, fence: Fence) -> Result<bool, String> {
            match unsafe { (self.poll)(fence, 0, 0) } {
                0x911a | 0x911c => Ok(true),
                0x911b => Ok(false),
                other => Err(format!("Sync poll failed {other:#x}")),
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
        pub fn load() -> Result<Self, String> {
            Err("Font pool sync backend unavailable on this target".into())
        }
        pub fn insert(&self) -> Result<Fence, String> {
            unreachable!()
        }
        pub fn ready(&self, _: Fence) -> Result<bool, String> {
            unreachable!()
        }
        pub fn delete(&self, _: Fence) {}
    }
}
use sync_api::{Api, Fence};
#[cfg(feature = "render_research")]
use crate::glyph_publish_timing::{self as publish_timing, Outcome, Phase};
#[cfg(feature = "render_research")]
use crate::{font_upload_pbo::{self as pbo, Decision as PboDecision}, glyph_publish_timing::PboPhase};
struct Slot {
    texture: Texture2D,
    version: u64,
    fence: Option<Fence>,
    #[cfg(feature = "render_research")]
    pbo: Option<pbo::Owner>,
}
pub struct Publication {
    pub texture: Texture2D,
    pub bytes: usize,
    /// Captured BEFORE retiring versions, so a caller cannot double-delete a
    /// recycled GLuint. Also true for a foreign-context previous texture:
    /// neither pool nor caller may delete that name in the new context.
    /// Same-texture results also never need a caller deletion.
    pub previous_owned: bool,
    pub pooled: bool,
    /// Persistent GL/context failure is observable, not a claimed good bitmap.
    pub valid: bool,
}
pub struct Pool {
    context: usize,
    api: Option<Api>,
    reason: Option<String>,
    slots: Vec<Slot>,
    current: Option<Texture2D>,
    shape: (u32, u32),
    history: Option<delta::Delta>,
    scratch: Vec<u8>,
    warm_bytes: u64,
    reused: u64,
    fresh: u64,
    busy: u64,
    failed: u64,
    polls: u64,
    delta_bytes: u64,
    full_bytes: u64,
    mutable_fallbacks: u64,
    #[cfg(feature = "render_research")]
    research_mode: u8,
    #[cfg(feature = "render_research")]
    research_runtime_fallback_triggered: bool,
    #[cfg(feature = "render_research")]
    pbo_stats: pbo::Stats,
    #[cfg(feature = "render_research")]
    pbo_client_core: bool,
    #[cfg(feature = "render_research")]
    pbo_client_blocked: bool,
    #[cfg(feature = "render_research")]
    immutable: ImmutableStats,
}
fn context_id() -> usize {
    unsafe { get_internal_gl() }.quad_context.context_id()
}
fn flush_retirement() {
    unsafe { get_internal_gl() }.flush();
    unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
}
fn drain_errors() {
    // A lost/broken context must not turn an optional optimization into an
    // unbounded render-thread loop. Recovery success is still checked later.
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
/// Selects only another version: the caller's previous handle must survive a
/// failed replacement allocation, and the current handle may still be sampled.
fn retirement_candidate<T: Copy + PartialEq>(textures: impl IntoIterator<Item = T>, previous: T, current: Option<T>) -> Option<usize> {
    textures.into_iter().position(|texture| texture != previous && Some(texture) != current)
}
fn image(width: u32, height: u32, shadow: &[u8], guarded: bool) -> Result<Texture2D, String> {
    if bitmap_bytes(width, height) != Some(shadow.len()) {
        return Err("Invalid font atlas dimensions".into());
    }
    #[cfg(not(feature = "render_research"))]
    let _ = guarded;
    #[cfg(feature = "render_research")]
    let raw = if guarded {
        let mut created = None;
        let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |context| {
            created = Some(miniquad::Texture::from_rgba8(context, width as u16, height as u16, shadow));
        });
        if let Err(error) = result {
            if let Some(raw) = created {
                flush_retirement();
                raw.delete();
            }
            return Err(format!("Font client atlas allocation guard failed: {error}"));
        }
        created.unwrap()
    } else {
        miniquad::Texture::from_rgba8(unsafe { get_internal_gl() }.quad_context, width as u16, height as u16, shadow)
    };
    #[cfg(not(feature = "render_research"))]
    let raw = miniquad::Texture::from_rgba8(
        unsafe { get_internal_gl() }.quad_context,
        width as u16,
        height as u16,
        shadow,
    );
    let texture = Texture2D::from_miniquad_texture(raw);
    texture.set_filter(FilterMode::Linear);
    texture
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Clamp);
    let error = unsafe { miniquad::gl::glGetError() };
    if error != 0 || texture.raw_miniquad_texture_handle().gl_internal_id() == 0 {
        flush_retirement();
        texture.delete();
        return Err(format!("Font atlas allocation failed: {error:x}"));
    }
    Ok(texture)
}
#[cfg(feature = "render_research")]
enum PboUpload { Applied(usize), Declined, Failed }

/// The session/context borrow ends here on every exit. In particular, callers
/// never issue a mutable client-pointer recovery while unpack state is live.
#[cfg(feature = "render_research")]
fn upload_pbo(
    owner: &mut pbo::Owner, texture: Texture2D, width: u32, shadow: &[u8],
    rectangles: &[delta::Rect], scratch: &mut Vec<u8>, timing: Option<publish_timing::Token>,
) -> PboUpload {
    let raw = texture.raw_miniquad_texture_handle();
    let context = unsafe { get_internal_gl() }.quad_context;
    let Some(buffer) = owner.buffer(context.context_id()) else { return PboUpload::Declined; };
    let update_span = timing.and_then(|token| token.start(Phase::Update));
    let guard_span = timing.and_then(|token| token.start_pbo(PboPhase::Guard));
    let session = buffer.begin(context, raw);
    if let Some(token) = timing {
        token.finish_pbo(PboPhase::Guard, guard_span);
        token.finish(Phase::Update, update_span);
    }
    let mut session = match session {
        Ok(session) => session,
        // Mini reports (and consumes) setup/preexisting GL errors. Preserve
        // the existing failure outcome rather than hiding it with a client
        // delta upload that could otherwise commit this target version.
        Err(error @ miniquad::PixelUnpackError::Gl { .. }) => {
            tracing::warn!(?error, "Font pixel-unpack session failed; rejecting target atlas version");
            return PboUpload::Failed;
        }
        Err(_) => return PboUpload::Declined,
    };
    if let Some(token) = timing { token.pbo_used(); }
    let mut offset = 0usize;
    let mut result = PboUpload::Applied(0);
    for rect in rectangles {
        let copy_span = timing.and_then(|token| token.start(Phase::Copy));
        scratch.clear();
        // Capacity and every row/range were checked for the whole publication.
        // No allocation is attempted after entering the PBO session.
        for y in rect.y..rect.y + rect.h {
            let start = ((y as usize * width as usize + rect.x as usize) * 4) as usize;
            scratch.extend_from_slice(&shadow[start..start + rect.w as usize * 4]);
        }
        if let Some(token) = timing { token.finish(Phase::Copy, copy_span); }
        let update_span = timing.and_then(|token| token.start(Phase::Update));
        let buffer_span = timing.and_then(|token| token.start_pbo(PboPhase::Buffer));
        let written = session.write_slice(offset, scratch);
        if let Some(token) = timing { token.finish_pbo(PboPhase::Buffer, buffer_span); }
        let uploaded = if written {
            let texture_span = timing.and_then(|token| token.start_pbo(PboPhase::Texture));
            let uploaded = session.upload_rgba8_rect(rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32, offset, scratch.len());
            if let Some(token) = timing { token.finish_pbo(PboPhase::Texture, texture_span); }
            uploaded
        } else { false };
        if let Some(token) = timing { token.finish(Phase::Update, update_span); }
        if !uploaded { result = PboUpload::Failed; break; }
        if let Some(token) = timing { token.uploaded(scratch.len()); }
        offset += scratch.len();
        result = PboUpload::Applied(offset);
    }
    let update_span = timing.and_then(|token| token.start(Phase::Update));
    let guard_span = timing.and_then(|token| token.start_pbo(PboPhase::Guard));
    drop(session);
    if let Some(token) = timing {
        token.finish_pbo(PboPhase::Guard, guard_span);
        token.finish(Phase::Update, update_span);
    }
    result
}
#[cfg(feature = "render_research")]
fn upload_client_delta(
    texture: miniquad::Texture, context: &mut miniquad::Context, width: u32, shadow: &[u8],
    rectangles: &[delta::Rect], scratch: &mut Vec<u8>, timing: Option<publish_timing::Token>,
) -> Result<usize, ()> {
    let mut bytes = 0usize;
    for rect in rectangles {
        let copy_span = timing.and_then(|token| token.start(Phase::Copy));
        scratch.clear();
        let needed = rect.w as usize * rect.h as usize * 4;
        if scratch.try_reserve(needed).is_err() {
            if let Some(token) = timing { token.finish(Phase::Copy, copy_span); }
            return Err(());
        }
        for y in rect.y..rect.y + rect.h {
            let start = (y as usize * width as usize + rect.x as usize) * 4;
            scratch.extend_from_slice(&shadow[start..start + rect.w as usize * 4]);
        }
        if let Some(token) = timing { token.finish(Phase::Copy, copy_span); }
        let update_span = timing.and_then(|token| token.start(Phase::Update));
        texture.update_texture_part(context, rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32, scratch);
        if let Some(token) = timing {
            token.finish(Phase::Update, update_span);
            token.uploaded(scratch.len());
        }
        bytes += scratch.len();
    }
    Ok(bytes)
}
impl Pool {
    pub fn new() -> Self {
        let context = context_id();
        let (api, reason) = match Api::load() {
            Ok(api) => (Some(api), None),
            Err(reason) => {
                // Optional capability probing cannot leave its errors for drawing.
                drain_errors();
                (None, Some(reason))
            }
        };
        Self::with_api(context, api, reason)
    }
    fn with_api(context: usize, api: Option<Api>, reason: Option<String>) -> Self {
        Self {
            context,
            api,
            reason,
            slots: Vec::new(),
            current: None,
            shape: (0, 0),
            history: None,
            scratch: Vec::new(),
            warm_bytes: 0,
            reused: 0,
            fresh: 0,
            busy: 0,
            failed: 0,
            polls: 0,
            delta_bytes: 0,
            full_bytes: 0,
            mutable_fallbacks: 0,
            #[cfg(feature = "render_research")]
            research_mode: 0,
            #[cfg(feature = "render_research")]
            research_runtime_fallback_triggered: false,
            #[cfg(feature = "render_research")]
            pbo_stats: pbo::Stats { requested: pbo::stats().requested, force_missing: pbo::stats().force_missing, ..pbo::Stats::default() },
            #[cfg(feature = "render_research")]
            pbo_client_core: api.is_some(),
            #[cfg(feature = "render_research")]
            pbo_client_blocked: false,
            #[cfg(feature = "render_research")]
            immutable: IMMUTABLE_REQUEST.with(|value| {
                let (requested, force_missing) = value.get();
                ImmutableStats { requested, force_missing, ..ImmutableStats::default() }
            }),
        }
    }
    /// Loading-only failure injection. Production builds have neither these
    /// fields nor the publication checks: 0 automatic, 1 unavailable without
    /// probing GL, 2 all alternate versions busy, 3 mutable first-publication
    /// fallback. Invalid modes select automatic behavior.
    #[cfg(feature = "render_research")]
    pub fn new_for_research(mode: u8) -> Self {
        let mode = if mode <= 3 { mode } else { 0 };
        let mut pool = if mode == 1 {
            Self::with_api(context_id(), None, Some("Research: forced sync unavailable".into()))
        } else {
            Self::new()
        };
        pool.research_mode = mode;
        pool
    }
    pub fn supported(&self) -> bool {
        self.api.is_some() && self.context == context_id()
    }
    pub fn owns(&self, texture: Texture2D) -> bool {
        self.slots.iter().any(|slot| slot.texture == texture)
    }
    pub fn can_warm(&self, width: u32, height: u32) -> bool {
        self.supported() && pool_fits_budget(width, height)
    }
    #[cfg(feature = "render_research")]
    pub fn client_guard_required(&self) -> bool {
        self.pbo_stats.requested && self.pbo_client_core && self.context == context_id()
    }
    #[cfg(feature = "render_research")]
    pub fn client_guard_blocked(&self) -> bool { self.pbo_client_blocked || self.immutable.blocked || immutable_context_blocked() }
    #[cfg(feature = "render_research")]
    pub fn immutable_blocked(&self) -> bool { self.immutable.blocked || immutable_context_blocked() }
    #[cfg(feature = "render_research")]
    pub fn block_client_guard(&mut self, reason: String) {
        if !self.pbo_client_blocked {
            self.pbo_count(|stats| stats.runtime_errors = stats.runtime_errors.saturating_add(1));
        }
        self.pbo_client_blocked = true;
        self.reason = Some(reason);
    }
    fn make_image(&mut self, width: u32, height: u32, shadow: &[u8], loading: bool) -> Result<Texture2D, String> {
        #[cfg(not(feature = "render_research"))]
        let _ = loading;
        #[cfg(feature = "render_research")]
        if self.immutable.requested && loading {
            if self.immutable.blocked {
                return Err("Immutable font atlas state is blocked".into());
            }
            self.immutable.attempts += 1;
            let result = if self.immutable.force_missing {
                Ok(miniquad::ImmutableTextureAttempt::Declined(miniquad::ImmutableTextureDecline::MissingEntryPoints))
            } else if bitmap_bytes(width, height) != Some(shadow.len()) {
                self.immutable.blocked = true;
                IMMUTABLE_FAILURE_CONTEXT.with(|value| value.set(Some(self.context)));
                self.immutable.fatal_errors += 1;
                self.immutable.reason = Some("Invalid immutable font atlas dimensions".into());
                return Err(self.immutable.reason.clone().unwrap());
            } else {
                miniquad::Texture::try_from_rgba8_immutable(
                    unsafe { get_internal_gl() }.quad_context,
                    width as u16,
                    height as u16,
                    shadow,
                )
            };
            match result {
                Ok(miniquad::ImmutableTextureAttempt::Ready(raw)) => {
                    self.immutable.accepted_images += 1;
                    self.immutable.initialized_rgba_bytes += shadow.len() as u64;
                    return Ok(Texture2D::from_miniquad_texture(raw));
                }
                Ok(miniquad::ImmutableTextureAttempt::Declined(reason)) => {
                    self.immutable.declined_images += 1;
                    self.immutable.reason = Some(format!("{reason:?}"));
                }
                Err(error) => {
                    self.immutable.fatal_errors += 1;
                    self.immutable.blocked = true;
                    IMMUTABLE_FAILURE_CONTEXT.with(|value| value.set(Some(self.context)));
                    self.immutable.reason = Some(error.to_string());
                    return Err(error.to_string());
                }
            }
        }
        #[cfg(feature = "render_research")]
        let guarded = self.client_guard_required();
        #[cfg(not(feature = "render_research"))]
        let guarded = false;
        let result = image(width, height, shadow, guarded);
        #[cfg(feature = "render_research")]
        if self.immutable.requested && result.is_ok() {
            self.immutable.mutable_images += 1;
        }
        #[cfg(feature = "render_research")]
        if guarded {
            if let Err(reason) = &result { self.block_client_guard(reason.clone()); }
        }
        result
    }
    fn retire_slots(&mut self) {
        if self.context != context_id() {
            // Foreign native names must never be deleted in the new context.
            self.slots.clear();
            self.current = None;
            self.history = None;
            self.shape = (0, 0);
            return;
        }
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
    }
    fn loading_failure(&mut self, reason: String) {
        self.failed += 1;
        self.reason = Some(reason);
        #[cfg(feature = "render_research")]
        if self.immutable.blocked || immutable_context_blocked() {
            // State restoration was not proven. Report failure and let the
            // native context own cleanup; do not issue another GL recovery.
            self.api = None;
            return;
        }
        self.retire_slots();
        self.api = None;
        drain_errors();
    }
    /// Loading preparation. The original mutable cache remains owned by the
    /// caller until this returns Some; failure never changes brush/shadow data.
    pub fn warm(&mut self, width: u32, height: u32, shadow: &[u8]) -> Option<Texture2D> {
        self.warm_internal(width, height, shadow, true)
    }
    fn warm_internal(&mut self, width: u32, height: u32, shadow: &[u8], loading: bool) -> Option<Texture2D> {
        #[cfg(not(feature = "render_research"))]
        let _ = loading;
        let api = self.api?;
        if self.context != context_id() {
            self.reason = Some("Managed context changed; recreate painter".into());
            self.api = None;
            return None;
        }
        if !self.slots.is_empty() {
            return (self.shape == (width, height) && bitmap_bytes(width, height) == Some(shadow.len()))
                .then_some(self.current)
                .flatten();
        }
        let Some(bytes) = bitmap_bytes(width, height).filter(|&bytes| bytes == shadow.len()) else {
            self.loading_failure("Invalid atlas bitmap".into());
            return None;
        };
        if !self.can_warm(width, height) {
            self.loading_failure("Font pool exceeds 64MiB nominal GPU budget".into());
            return None;
        }
        // Warming can replace an already sampled loading UI cache. Consume its
        // CPU commands before the caller deletes that original image.
        unsafe { get_internal_gl() }.flush();
        #[cfg(feature = "render_research")]
        if loading && self.pbo_stats.requested {
            // A loading rewarm first submits deletion of queued old owners;
            // it cannot accumulate a second set beyond this painter's cap.
            pbo::flush_pending_deletions();
        }
        self.shape = (width, height);
        self.history = Some(delta::Delta::new(width, height));
        if self.scratch.try_reserve_exact(1024 * 1024).is_err() {
            self.loading_failure("Font upload scratch allocation failed".into());
            return None;
        }
        self.scratch.resize(1024 * 1024, 0);
        self.scratch.clear();
        for _ in 0..SLOTS {
            let texture = match self.make_image(width, height, shadow, loading) {
                Ok(texture) => texture,
                Err(reason) => {
                    self.loading_failure(reason);
                    return None;
                }
            };
            // Loading is the sole resource-creation point. Late publication
            // warmups and busy replacements keep the original client path.
            #[cfg(feature = "render_research")]
            let pbo = if loading && self.pbo_stats.requested {
                self.pbo_count(|stats| stats.loading_slots = stats.loading_slots.saturating_add(1));
                let owner = match pbo::Owner::new(unsafe { get_internal_gl() }.quad_context, self.pbo_stats.force_missing) {
                    Ok(owner) => owner,
                    Err(error) => {
                        let unknown_state = match &error {
                            miniquad::PixelUnpackError::Gl { stage, cleanup_code, .. } => *stage == "loading binding restore" || cleanup_code.is_some(),
                            miniquad::PixelUnpackError::CapacityMismatch { cleanup_code, .. }
                            | miniquad::PixelUnpackError::BufferNameZero { cleanup_code } => cleanup_code.is_some(),
                            _ => false,
                        };
                        if unknown_state {
                            let reason = format!("Font PBO loading cleanup failed: {error}");
                            self.block_client_guard(reason.clone());
                            flush_retirement();
                            texture.delete();
                            self.loading_failure(reason);
                            return None;
                        }
                        None
                    }
                };
                if owner.is_some() {
                    self.pbo_count(|stats| stats.initialized_slots = stats.initialized_slots.saturating_add(1));
                } else {
                    self.pbo_count(|stats| stats.loading_missing_slots = stats.loading_missing_slots.saturating_add(1));
                }
                owner
            } else { None };
            let fence = match api.insert() {
                Ok(fence) => fence,
                Err(reason) => {
                    flush_retirement();
                    texture.delete();
                    self.loading_failure(reason);
                    return None;
                }
            };
            self.slots.push(Slot {
                texture,
                version: 0,
                fence: Some(fence),
                #[cfg(feature = "render_research")]
                pbo,
            });
            self.warm_bytes += bytes as u64;
            if unsafe { miniquad::gl::glGetError() } != 0 {
                self.loading_failure("Font loading upload/fence GL error".into());
                return None;
            }
        }
        self.current = Some(self.slots[0].texture);
        self.current
    }
    fn mutable_fallback(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], reason: String, previous_owned: bool) -> Publication {
        #[cfg(feature = "render_research")]
        if let Some(timing) = publish_timing::current() { timing.fallback(&reason); }
        self.failed += 1;
        self.mutable_fallbacks += 1;
        self.reason = Some(reason);
        #[cfg(feature = "render_research")]
        if self.client_guard_blocked() {
            return Publication { texture: previous, bytes: 0, previous_owned, pooled: false, valid: false };
        }
        if self.context != context_id() {
            self.slots.clear();
            self.current = None;
            self.history = None;
            self.shape = (0, 0);
            self.api = None;
            // The host must recreate a painter; neither update nor deletion of
            // a foreign name is permitted. This is not an EGL recovery path.
            return Publication {
                texture: previous,
                bytes: 0,
                previous_owned: true,
                pooled: false,
                valid: false,
            };
        }
        #[cfg(feature = "render_research")]
        let guarded = self.client_guard_required();
        flush_retirement();
        if let Some(index) = self.slots.iter().position(|slot| slot.texture == previous) {
            let slot = self.slots.remove(index);
            if let (Some(api), Some(fence)) = (self.api, slot.fence) {
                api.delete(fence);
            }
            // This native texture is transferred to the caller, not deleted.
        }
        self.retire_slots();
        self.api = None;
        drain_errors();
        let shape = previous.raw_miniquad_texture_handle();
        let dimensions_ok = shape.width == width && shape.height == height && bitmap_bytes(width, height) == Some(shadow.len());
        #[cfg(feature = "render_research")]
        if dimensions_ok && guarded {
            let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |context| shape.update(context, shadow));
            if let Err(error) = result {
                self.block_client_guard(format!("Font full client recovery guard failed: {error}"));
                return Publication { texture: previous, bytes: 0, previous_owned, pooled: false, valid: false };
            }
        }
        #[cfg(feature = "render_research")]
        let uploaded_by_guard = dimensions_ok && guarded;
        #[cfg(not(feature = "render_research"))]
        let uploaded_by_guard = false;
        if dimensions_ok && !uploaded_by_guard {
            shape.update(unsafe { get_internal_gl() }.quad_context, shadow);
        }
        let valid = dimensions_ok && unsafe { miniquad::gl::glGetError() } == 0;
        if valid {
            self.full_bytes += shadow.len() as u64;
        } else {
            self.reason = Some("Mutable font atlas recovery upload failed; recreate painter".into());
        }
        Publication {
            texture: previous,
            bytes: if valid { shadow.len() } else { 0 },
            previous_owned,
            pooled: false,
            valid,
        }
    }
    /// Caller preserves the original first-bitmap-callback preflush. No normal
    /// draw may be recorded between that flush and this atlas publication.
    pub fn publish(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], writes: &[[u32; 4]]) -> Publication {
        #[cfg(feature = "render_research")]
        let timing = publish_timing::current();
        #[cfg(feature = "render_research")]
        let prepare_span = timing.and_then(|timing| timing.start(Phase::Prepare));
        let previous_owned = self.owns(previous);
        if self.context != context_id() {
            return self.mutable_fallback(previous, width, height, shadow, "Managed context changed".into(), previous_owned);
        }
        if bitmap_bytes(width, height) != Some(shadow.len())
            || writes.iter().any(|&[x0, y0, x1, y1]| x0 >= x1 || y0 >= y1 || x1 > width || y1 > height)
        {
            return self.mutable_fallback(previous, width, height, shadow, "Invalid font bitmap/write".into(), previous_owned);
        }
        if self.api.is_none() {
            return self.mutable_fallback(previous, width, height, shadow, "Font pool unavailable".into(), previous_owned);
        }
        if self.shape != (width, height) && !self.slots.is_empty() {
            return self.mutable_fallback(previous, width, height, shadow, "Atlas shape changed without reset".into(), previous_owned);
        }
        #[cfg(feature = "render_research")]
        if self.research_mode == 3 && !self.research_runtime_fallback_triggered {
            self.research_runtime_fallback_triggered = true;
            return self.mutable_fallback(
                previous,
                width,
                height,
                shadow,
                "Research: forced first-publication mutable fallback".into(),
                previous_owned,
            );
        }
        if self.slots.is_empty() {
            if let Some(texture) = self.warm_internal(width, height, shadow, false) {
                self.fresh += 1;
                #[cfg(feature = "render_research")]
                if let Some(timing) = timing { timing.outcome(Outcome::Warm); }
                return Publication {
                    texture,
                    bytes: shadow.len() * SLOTS,
                    previous_owned,
                    pooled: true,
                    valid: true,
                };
            }
            return self.mutable_fallback(previous, width, height, shadow, "Font pool loading failed".into(), previous_owned);
        }
        let api = self.api.unwrap();
        self.history.as_mut().unwrap().advance(writes);
        let current = self.slots.iter().position(|slot| Some(slot.texture) == self.current).unwrap();
        let fence = match api.insert() {
            Ok(fence) => fence,
            Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
        };
        if let Some(old) = self.slots[current].fence.replace(fence) {
            api.delete(old);
        }
        let mut available = None;
        for (index, slot) in self.slots.iter().enumerate() {
            if index == current {
                continue;
            }
            #[cfg(feature = "render_research")]
            if self.research_mode == 2 {
                // Do not claim real fence polls for an injected busy outcome.
                continue;
            }
            let ready = if let Some(fence) = slot.fence {
                self.polls += 1;
                api.ready(fence)
            } else {
                Ok(true)
            };
            match ready {
                Ok(true) => {
                    available = Some(index);
                    break;
                }
                Ok(false) => {}
                Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
            }
        }
        if let Some(index) = available {
            if let Some(fence) = self.slots[index].fence.take() {
                api.delete(fence);
            }
            let version = self.history.as_ref().unwrap().generation();
            let rectangles = self.history.as_ref().unwrap().since(self.slots[index].version);
            let texture = self.slots[index].texture;
            let mut bytes = 0usize;
            #[cfg(feature = "render_research")]
            let pbo_choice = pbo::decision(self.pbo_stats.requested, self.slots[index].pbo.is_some(),
                self.slots[index].pbo.as_ref().map_or(0, pbo::Owner::capacity), self.scratch.capacity(),
                width, height, rectangles.iter().map(|rect|[rect.x, rect.y, rect.w, rect.h]));
            #[cfg(feature = "render_research")]
            if let Some(timing) = timing {
                timing.reuse(current, index, version, self.slots[index].version, rectangles.is_empty());
                timing.finish(Phase::Prepare, prepare_span);
            }
            #[cfg(feature = "render_research")]
            let used_pbo = {
                match pbo_choice {
                    PboDecision::Attempt { bytes: expected } => {
                        let result = upload_pbo(self.slots[index].pbo.as_mut().unwrap(), texture, width, shadow,
                            &rectangles, &mut self.scratch, timing);
                        match result {
                            PboUpload::Applied(uploaded) => { debug_assert_eq!(uploaded, expected); bytes = uploaded; true }
                            PboUpload::Declined => {
                                self.pbo_count(|stats| stats.declined_publications = stats.declined_publications.saturating_add(1));
                                false
                            }
                            PboUpload::Failed => {
                                self.pbo_count(|stats| stats.runtime_errors = stats.runtime_errors.saturating_add(1));
                                return self.mutable_fallback(previous, width, height, shadow, "Font PBO upload failure".into(), previous_owned);
                            }
                        }
                    }
                    PboDecision::Missing => {
                        self.pbo_count(|stats| stats.missing_publications = stats.missing_publications.saturating_add(1));
                        false
                    }
                    PboDecision::Oversize => {
                        self.pbo_count(|stats| stats.oversize_publications = stats.oversize_publications.saturating_add(1));
                        false
                    }
                    PboDecision::Off | PboDecision::Empty => false,
                }
            };
            #[cfg(not(feature = "render_research"))]
            let used_pbo = false;
            #[cfg(feature = "render_research")]
            let used_client_guard = if !used_pbo && !rectangles.is_empty() && self.client_guard_required() {
                let raw = texture.raw_miniquad_texture_handle();
                let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |context| {
                    upload_client_delta(raw, context, width, shadow, &rectangles, &mut self.scratch, timing)
                });
                match result {
                    Ok(Ok(uploaded)) => { bytes = uploaded; true }
                    Ok(Err(())) => return self.mutable_fallback(previous, width, height, shadow, "Font delta scratch allocation failed".into(), previous_owned),
                    Err(error) => {
                        let reason = format!("Font client delta guard failed: {error}");
                        self.block_client_guard(reason.clone());
                        return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned);
                    }
                }
            } else { false };
            #[cfg(not(feature = "render_research"))]
            let used_client_guard = false;
            if !used_pbo && !used_client_guard { for rect in rectangles {
                #[cfg(feature = "render_research")]
                let copy_span = timing.and_then(|timing| timing.start(Phase::Copy));
                self.scratch.clear();
                let needed = (rect.w as usize) * (rect.h as usize) * 4;
                if self.scratch.try_reserve(needed).is_err() {
                    #[cfg(feature = "render_research")]
                    if let Some(timing) = timing { timing.finish(Phase::Copy, copy_span); }
                    return self.mutable_fallback(previous, width, height, shadow, "Font delta scratch allocation failed".into(), previous_owned);
                }
                for y in rect.y..rect.y + rect.h {
                    let start = ((y * width + rect.x) * 4) as usize;
                    self.scratch.extend_from_slice(&shadow[start..start + (rect.w * 4) as usize]);
                }
                #[cfg(feature = "render_research")]
                if let Some(timing) = timing { timing.finish(Phase::Copy, copy_span); }
                #[cfg(feature = "render_research")]
                let update_span = timing.and_then(|timing| timing.start(Phase::Update));
                texture.raw_miniquad_texture_handle().update_texture_part(
                    unsafe { get_internal_gl() }.quad_context,
                    rect.x as i32,
                    rect.y as i32,
                    rect.w as i32,
                    rect.h as i32,
                    &self.scratch,
                );
                #[cfg(feature = "render_research")]
                if let Some(timing) = timing {
                    timing.finish(Phase::Update, update_span);
                    timing.uploaded(self.scratch.len());
                }
                bytes += self.scratch.len();
            } }
            #[cfg(feature = "render_research")]
            let error_span = timing.and_then(|timing| timing.start(Phase::GlError));
            let error = unsafe { miniquad::gl::glGetError() };
            #[cfg(feature = "render_research")]
            if let Some(timing) = timing { timing.finish(Phase::GlError, error_span); }
            if error != 0 {
                #[cfg(feature = "render_research")]
                if used_pbo { self.pbo_count(|stats| stats.runtime_errors = stats.runtime_errors.saturating_add(1)); }
                return self.mutable_fallback(previous, width, height, shadow, "Font delta GL error".into(), previous_owned);
            }
            self.slots[index].version = version;
            self.current = Some(texture);
            self.reused += 1;
            self.delta_bytes += bytes as u64;
            #[cfg(feature = "render_research")]
            if used_pbo {
                self.pbo_count(|stats| {
                    stats.accepted_publications = stats.accepted_publications.saturating_add(1);
                    stats.accepted_bytes = stats.accepted_bytes.saturating_add(bytes as u64);
                });
            }
            #[cfg(feature = "render_research")]
            if let Some(timing) = timing { timing.outcome(Outcome::Reused); }
            return Publication {
                texture,
                bytes,
                previous_owned,
                pooled: true,
                valid: true,
            };
        }
        // Keep the caller/current image alive if allocation of the replacement
        // fails. It can then receive a full authoritative mutable recovery.
        let Some(retire) = retirement_candidate(self.slots.iter().map(|slot| slot.texture), previous, self.current) else {
            return self.mutable_fallback(previous, width, height, shadow, "No retireable atlas version".into(), previous_owned);
        };
        self.busy += 1;
        flush_retirement();
        let old = self.slots.remove(retire);
        if let Some(fence) = old.fence {
            api.delete(fence);
        }
        old.texture.delete();
        let texture = match self.make_image(width, height, shadow, false) {
            Ok(texture) => texture,
            Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
        };
        self.slots.push(Slot {
            texture,
            version: self.history.as_ref().unwrap().generation(),
            fence: None,
            #[cfg(feature = "render_research")]
            pbo: None,
        });
        self.current = Some(texture);
        self.fresh += 1;
        self.full_bytes += shadow.len() as u64;
        #[cfg(feature = "render_research")]
        if let Some(timing) = timing { timing.outcome(Outcome::BusyFresh); }
        Publication {
            texture,
            bytes: shadow.len(),
            previous_owned,
            pooled: true,
            valid: true,
        }
    }
    pub fn reset(&mut self) {
        self.retire_slots();
    }
    #[cfg(feature = "render_research")]
    fn pbo_count(&mut self, edit: impl Fn(&mut pbo::Stats)) {
        edit(&mut self.pbo_stats);
        pbo::add(edit);
    }
    pub fn report(&self) -> serde_json::Value {
        let report = serde_json::json!({"supported":self.supported(),"reason":self.reason,"slots":self.slots.len(),"shape":self.shape,
            "warm_bytes":self.warm_bytes,"reused":self.reused,"fresh":self.fresh,"busy_fallbacks":self.busy,"failed_fallbacks":self.failed,
            "polls":self.polls,"delta_bytes":self.delta_bytes,"full_bytes":self.full_bytes,"mutable_fallbacks":self.mutable_fallbacks,
            "nominal_storage_bytes":self.slots.len() as u64*self.shape.0 as u64*self.shape.1 as u64*4,"blocking_waits":0,
            "scope":"RGBA policy0, conservative zero-timeout sync, full logical atlas and mutable recovery. Nominal live pool storage excludes CPU mirror and in-flight driver storage; not physical GPU memory."});
        #[cfg(feature = "render_research")]
        {
            let mut report = report;
            report["research"] = serde_json::json!({"mode":self.research_mode,"runtime_fallback_triggered":self.research_runtime_fallback_triggered});
            let immutable = &self.immutable;
            report["immutable_storage"] = serde_json::json!({
                "requested":immutable.requested,"force_missing":immutable.force_missing,
                "attempts":immutable.attempts,"accepted_images":immutable.accepted_images,
                "initialized_rgba_bytes":immutable.initialized_rgba_bytes,
                "declined_images":immutable.declined_images,"mutable_images":immutable.mutable_images,
                "fatal_errors":immutable.fatal_errors,"blocked":immutable.blocked,"reason":immutable.reason,
                "scope":"Loading allocation method only; late replacement keeps the original allocator. Glyph pixels, representatives, layout and pool budget are unchanged. Initialized RGBA bytes are logical payload, not measured physical storage."});
            let stats = self.pbo_stats;
            report["pbo"] = serde_json::json!({
                "requested":stats.requested,"force_missing":stats.force_missing,
                "capacity_bytes":pbo::CAPACITY,"painter_budget_bytes":pbo::PAINTER_BUDGET,
                "live_slots":self.slots.iter().filter(|slot|slot.pbo.is_some()).count(),
                "nominal_live_bytes":self.slots.iter().filter_map(|slot|slot.pbo.as_ref()).map(pbo::Owner::capacity).sum::<usize>(),
                "loading_slots":stats.loading_slots,"initialized_slots":stats.initialized_slots,
                "loading_missing_slots":stats.loading_missing_slots,
                "accepted_publications":stats.accepted_publications,"accepted_bytes":stats.accepted_bytes,
                "missing_publications":stats.missing_publications,"oversize_publications":stats.oversize_publications,
                "declined_publications":stats.declined_publications,"runtime_errors":stats.runtime_errors
            });
            report["pbo"]["client_core_proven"] = serde_json::json!(self.pbo_client_core);
            report["pbo"]["client_guard_blocked"] = serde_json::json!(self.pbo_client_blocked);
            report
        }
        #[cfg(not(feature = "render_research"))]
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_accepts_the_exact_limit_without_allocating_a_bitmap() {
        assert_eq!(bitmap_bytes(2048, 2048), Some(16 * 1024 * 1024));
        assert!(pool_fits_budget(2048, 2048));
        assert!(!pool_fits_budget(2048, 2049));
        assert!(!pool_fits_budget(2049, 2048));
        // Other valid aspect ratios are decided by area, not texture shape.
        assert!(pool_fits_budget(4096, 1024));
        assert!(pool_fits_budget(65535, 64));
        assert!(!pool_fits_budget(65535, 65));
        for shape in [(0, 1), (1, 0), (65536, 1), (1, 65536), (u32::MAX, u32::MAX)] {
            assert_eq!(bitmap_bytes(shape.0, shape.1), None);
            assert!(!pool_fits_budget(shape.0, shape.1));
        }
    }

    #[test]
    fn replacement_allocation_never_withdraws_caller_or_current_version() {
        // Abstract owner identities, not fake GL handles. Test all four current
        // and caller roles, including a caller outside the pool, and reordered
        // slots. Keeping both identities is the recovery precondition even
        // when the allocation of the chosen replacement subsequently fails.
        for order in [[7, 11, 13, 17], [17, 13, 11, 7], [11, 7, 17, 13]] {
            for previous in [7, 11, 13, 17, 23] {
                for current in [None, Some(7), Some(11), Some(13), Some(17), Some(23)] {
                    let index = retirement_candidate(order, previous, current).unwrap();
                    assert_ne!(order[index], previous);
                    assert_ne!(Some(order[index]), current);
                    let survivors: Vec<_> = order
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _)| *i != index)
                        .map(|(_, texture)| texture)
                        .collect();
                    if order.contains(&previous) {
                        assert!(survivors.contains(&previous));
                    }
                    if let Some(current) = current.filter(|c| order.contains(c)) {
                        assert!(survivors.contains(&current));
                    }
                }
            }
        }
        assert_eq!(retirement_candidate([], 7, Some(11)), None);
        assert_eq!(retirement_candidate([7], 7, Some(7)), None);
        assert_eq!(retirement_candidate([7, 11], 7, Some(11)), None);
        assert_eq!(retirement_candidate([11, 7], 7, Some(11)), None);
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        #[cfg(feature = "render_research")]
        if self.immutable.blocked || immutable_context_blocked() {
            return;
        }
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
            // TextPainter queues its cache/current exactly once; other versions
            // also use the existing main-thread queued texture deletion entry.
            if Some(slot.texture) != self.current {
                crate::ext::queue_texture_deletion(slot.texture);
            }
        }
    }
}
