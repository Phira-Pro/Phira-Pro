//! Optional, bounded ES3 pixel-unpack uploads. No map, orphan, clock, or GPU wait.
//!
//! Construct buffers during loading and explicitly delete them on the creating
//! render thread. This owner is neither Copy nor Clone; Drop performs
//! no GL work. It may move through a retirement queue, but begin/delete reject
//! a different thread. A foreign-context delete consumes the owner without deleting its
//! native name. This is not an EGL context-loss recovery mechanism.
//!
//! Before every begin, the caller MUST prove that the GPU has finished reading
//! this buffer. A fixed one-to-one texture-slot association may use that slot's
//! ready fence, provided the fence follows its last PBO upload and all consumers
//! in the same GL command stream. This API does not establish or poll that proof.
//! A session borrows both context and buffer for one publication. End it before
//! calling any legacy GL path. Each byte range is written once and uploaded once;
//! boolean returns mean CPU guards allowed API submission, not GL success.
//!
//! finish checks errors after restoring state. Drop also restores state but leaves
//! any errors for the caller to check. Never publish a texture version on error.
use crate::{native::gl::*, Context, Texture, TextureFormat};
use std::{error::Error, ffi::CStr, fmt, marker::PhantomData, rc::Rc, thread::ThreadId};

#[path = "pixel_unpack_guards.rs"]
mod guards;

/// Nominal byte limit per owner; excludes any in-flight driver storage.
pub const MAX_PIXEL_UNPACK_CAPACITY: usize = guards::MAX_CAPACITY;
const PIXEL_UNPACK_BUFFER: GLenum = 0x88ec;
const PIXEL_UNPACK_BUFFER_BINDING: GLenum = 0x88ef;
const BUFFER_SIZE: GLenum = 0x8764;
const UNPACK_ROW_LENGTH: GLenum = 0x0cf2;
const UNPACK_SKIP_ROWS: GLenum = 0x0cf3;
const UNPACK_SKIP_PIXELS: GLenum = 0x0cf4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelUnpackError {
    Unsupported(&'static str),
    InvalidCapacity,
    LoadingAllocation,
    ForeignContext,
    ForeignThread,
    InvalidTexture,
    IncompleteTransfer,
    ClientMemoryBindingMismatch {
        actual: i32,
        cleanup_code: Option<u32>,
    },
    BufferNameZero {
        cleanup_code: Option<u32>,
    },
    CapacityMismatch {
        expected: usize,
        actual: i32,
        cleanup_code: Option<u32>,
    },
    Gl {
        stage: &'static str,
        code: u32,
        cleanup_code: Option<u32>,
    },
}
impl fmt::Display for PixelUnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pixel unpack: {:?}", self)
    }
}
impl Error for PixelUnpackError {}

fn entries_loaded() -> bool {
    #[cfg(any(target_arch = "wasm32", target_os = "macos", target_os = "ios"))]
    {
        false
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "macos", target_os = "ios")))]
    {
        [
            "glGetString",
            "glGetError",
            "glGetIntegerv",
            "glGenBuffers",
            "glBindBuffer",
            "glBufferData",
            "glGetBufferParameteriv",
            "glBufferSubData",
            "glDeleteBuffers",
            "glPixelStorei",
            "glActiveTexture",
            "glBindTexture",
            "glTexSubImage2D",
        ]
        .iter()
        .all(|name| crate::native::gl::is_function_loaded(name))
    }
}
fn gl_error(stage: &'static str, code: u32) -> PixelUnpackError {
    PixelUnpackError::Gl {
        stage,
        code,
        cleanup_code: None,
    }
}
unsafe fn failed_creation(name: GLuint, previous: GLuint) -> Option<u32> {
    glBindBuffer(PIXEL_UNPACK_BUFFER, previous);
    if name != 0 {
        glDeleteBuffers(1, &name);
    }
    match glGetError() {
        0 => None,
        code => Some(code),
    }
}

/// Loading-created PBO. No GL destructor: call delete explicitly.
pub struct PixelUnpackBuffer {
    name: GLuint,
    capacity: usize,
    context: usize,
    render_thread: ThreadId,
}
impl PixelUnpackBuffer {
    /// Allocates and initializes a fixed data store during loading. The actual
    /// ES version, callable entries, allocation size, and GL errors are checked.
    /// Invalid capacity and unavailable entries perform no GL calls.
    pub fn new(ctx: &mut Context, capacity: usize) -> Result<Self, PixelUnpackError> {
        if !guards::capacity_valid(capacity) {
            return Err(PixelUnpackError::InvalidCapacity);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Missing ES3 pixel-unpack entries",
            ));
        }
        unsafe {
            let prior = glGetError();
            if prior != 0 {
                return Err(gl_error("preexisting loading error", prior));
            }
            let version = glGetString(GL_VERSION);
            let version_error = glGetError();
            if version_error != 0 {
                return Err(gl_error("version query", version_error));
            }
            if version.is_null() || !guards::es3_version(CStr::from_ptr(version as _).to_bytes()) {
                return Err(PixelUnpackError::Unsupported(
                    "Active API is not OpenGL ES 3+",
                ));
            }
        }
        let mut initialized = Vec::new();
        initialized
            .try_reserve_exact(capacity)
            .map_err(|_| PixelUnpackError::LoadingAllocation)?;
        initialized.resize(capacity, 0u8);
        unsafe {
            let mut previous = 0i32;
            glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut previous);
            let error = glGetError();
            if error != 0 {
                return Err(gl_error("loading binding query", error));
            }
            let previous = previous as GLuint;
            let mut name = 0;
            glGenBuffers(1, &mut name);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "buffer name creation",
                    code: error,
                    cleanup_code,
                });
            }
            if name == 0 {
                return Err(PixelUnpackError::BufferNameZero {
                    cleanup_code: failed_creation(name, previous),
                });
            }
            glBindBuffer(PIXEL_UNPACK_BUFFER, name);
            glBufferData(
                PIXEL_UNPACK_BUFFER,
                capacity as _,
                initialized.as_ptr().cast(),
                GL_STREAM_DRAW,
            );
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "initialized buffer allocation",
                    code: error,
                    cleanup_code,
                });
            }
            let mut actual = 0i32;
            glGetBufferParameteriv(PIXEL_UNPACK_BUFFER, BUFFER_SIZE, &mut actual);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "actual buffer size query",
                    code: error,
                    cleanup_code,
                });
            }
            if actual < 0 || actual as usize != capacity {
                return Err(PixelUnpackError::CapacityMismatch {
                    expected: capacity,
                    actual,
                    cleanup_code: failed_creation(name, previous),
                });
            }
            glBindBuffer(PIXEL_UNPACK_BUFFER, previous);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "loading binding restore",
                    code: error,
                    cleanup_code,
                });
            }
            Ok(Self {
                name,
                capacity,
                context: ctx.context_id(),
                render_thread: std::thread::current().id(),
            })
        }
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Caller must supply completed GPU-read proof for this exact buffer.
    /// Restores the captured unpack binding and row/skip state exactly, which
    /// may be nondefault. Legacy client-pointer fallback separately requires
    /// its usual unpack-buffer-zero, tightly-packed input contract.
    /// Captures unpack state once. No other GL operations may be interleaved
    /// with this session; finish/drop it before any client-pointer fallback.
    pub fn begin<'a>(
        &'a mut self,
        ctx: &'a mut Context,
        texture: Texture,
    ) -> Result<PixelUnpackSession<'a>, PixelUnpackError> {
        if self.render_thread != std::thread::current().id() {
            return Err(PixelUnpackError::ForeignThread);
        }
        if self.context != ctx.context_id() {
            return Err(PixelUnpackError::ForeignContext);
        }
        if !guards::texture_valid(
            texture.gl_internal_id(),
            texture.width,
            texture.height,
            texture.format == TextureFormat::RGBA8,
        ) {
            return Err(PixelUnpackError::InvalidTexture);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Pixel-unpack entries unavailable",
            ));
        }
        let saved = unsafe {
            let prior = glGetError();
            if prior != 0 {
                return Err(gl_error("preexisting session error", prior));
            }
            let saved = SavedUnpack::capture();
            let error = glGetError();
            if error != 0 {
                return Err(gl_error("session state capture", error));
            }
            saved
        };
        let mut session = PixelUnpackSession {
            ctx,
            buffer: self,
            texture,
            saved,
            restored: false,
            cursor: guards::Cursor::default(),
            _render_thread: PhantomData,
        };
        session.ctx.cache.store_texture_binding(0);
        session.ctx.cache.bind_texture(0, texture.gl_internal_id());
        unsafe {
            // PBO is a distinct target, never use GlCache::bind_buffer here.
            glBindBuffer(PIXEL_UNPACK_BUFFER, session.buffer.name);
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
            glPixelStorei(UNPACK_ROW_LENGTH, 0);
            glPixelStorei(UNPACK_SKIP_ROWS, 0);
            glPixelStorei(UNPACK_SKIP_PIXELS, 0);
            let error = glGetError();
            if error != 0 {
                session.restore();
                let cleanup_code = match glGetError() {
                    0 => None,
                    code => Some(code),
                };
                return Err(PixelUnpackError::Gl {
                    stage: "session state setup",
                    code: error,
                    cleanup_code,
                });
            }
        }
        Ok(session)
    }

    /// Explicit render-thread retirement. On foreign context the owner is
    /// consumed, but its foreign native name is never deleted. The same applies
    /// to a different thread. No GL Drop runs.
    /// Any preexisting or deletion error prevents an Ok result.
    pub fn delete(self, ctx: &mut Context) -> Result<(), PixelUnpackError> {
        if self.render_thread != std::thread::current().id() {
            return Err(PixelUnpackError::ForeignThread);
        }
        if self.context != ctx.context_id() {
            return Err(PixelUnpackError::ForeignContext);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Pixel-unpack entries unavailable",
            ));
        }
        unsafe {
            let prior = glGetError();
            glDeleteBuffers(1, &self.name);
            let error = glGetError();
            if prior != 0 {
                return Err(PixelUnpackError::Gl {
                    stage: "preexisting retirement error",
                    code: prior,
                    cleanup_code: if error == 0 { None } else { Some(error) },
                });
            }
            if error != 0 {
                return Err(gl_error("buffer retirement", error));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct SavedUnpack {
    binding: GLint,
    alignment: GLint,
    row_length: GLint,
    skip_rows: GLint,
    skip_pixels: GLint,
}
impl SavedUnpack {
    unsafe fn capture() -> Self {
        let mut saved = Self {
            binding: 0,
            alignment: 0,
            row_length: 0,
            skip_rows: 0,
            skip_pixels: 0,
        };
        glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut saved.binding);
        glGetIntegerv(GL_UNPACK_ALIGNMENT, &mut saved.alignment);
        glGetIntegerv(UNPACK_ROW_LENGTH, &mut saved.row_length);
        glGetIntegerv(UNPACK_SKIP_ROWS, &mut saved.skip_rows);
        glGetIntegerv(UNPACK_SKIP_PIXELS, &mut saved.skip_pixels);
        saved
    }
    unsafe fn restore(self) {
        glPixelStorei(GL_UNPACK_ALIGNMENT, self.alignment);
        glPixelStorei(UNPACK_ROW_LENGTH, self.row_length);
        glPixelStorei(UNPACK_SKIP_ROWS, self.skip_rows);
        glPixelStorei(UNPACK_SKIP_PIXELS, self.skip_pixels);
        glBindBuffer(PIXEL_UNPACK_BUFFER, self.binding as GLuint);
    }
}

/// Runs existing client-pointer APIs with unpack-buffer zero and tight RGBA rows.
/// Requires actual core ES3 and callable unpack entries; unsupported APIs decline.
/// The closure must use the supplied context. Captured external unpack state is
/// restored exactly, including a nonzero buffer. Operation/restore errors return
/// Err; callers creating GL owners must retain their handles for error cleanup.
/// Panic restores state through RAII and leaves GL errors for the caller.
#[rustfmt::skip]
pub fn with_client_memory_unpack<T>(
    ctx: &mut Context, operation: impl FnOnce(&mut Context) -> T,
) -> Result<T, PixelUnpackError> {
    #[cfg(any(target_arch = "wasm32", target_os = "macos", target_os = "ios"))]
    let loaded = false;
    #[cfg(not(any(target_arch = "wasm32", target_os = "macos", target_os = "ios")))]
    let loaded = ["glGetString", "glGetError", "glGetIntegerv", "glBindBuffer", "glPixelStorei"]
        .iter().all(|name| crate::native::gl::is_function_loaded(name));
    if !loaded { return Err(PixelUnpackError::Unsupported("Missing ES3 client-unpack entries")); }
    unsafe {
        let prior = glGetError();
        if prior != 0 { return Err(gl_error("preexisting client memory error", prior)); }
        let version = glGetString(GL_VERSION);
        let error = glGetError();
        if error != 0 { return Err(gl_error("client memory version query", error)); }
        if version.is_null() || !guards::es3_version(CStr::from_ptr(version as _).to_bytes()) {
            return Err(PixelUnpackError::Unsupported("Active API is not OpenGL ES 3+"));
        }
    }
    let saved = unsafe {
        let saved = SavedUnpack::capture();
        let error = glGetError();
        if error != 0 { return Err(gl_error("client memory state capture", error)); }
        saved
    };
    // This local guard owns the exclusive context borrow until exact restore.
    struct Restore<'a>(&'a mut Context, SavedUnpack, usize);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            if self.0.context_id() == self.2 { unsafe { self.1.restore(); } }
        }
    }
    let context = ctx.context_id();
    let scope = Restore(ctx, saved, context);
    unsafe {
        glBindBuffer(PIXEL_UNPACK_BUFFER, 0);
        glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
        glPixelStorei(UNPACK_ROW_LENGTH, 0);
        glPixelStorei(UNPACK_SKIP_ROWS, 0);
        glPixelStorei(UNPACK_SKIP_PIXELS, 0);
        let mut actual = -1;
        glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut actual);
        let error = glGetError();
        if error != 0 || actual != 0 {
            drop(scope);
            let cleanup_code = match glGetError() { 0 => None, code => Some(code) };
            return Err(if error != 0 {
                PixelUnpackError::Gl { stage: "client memory state setup", code: error, cleanup_code }
            } else {
                PixelUnpackError::ClientMemoryBindingMismatch { actual, cleanup_code }
            });
        }
    }
    let value = operation(&mut *scope.0);
    if scope.0.context_id() != context { return Err(PixelUnpackError::ForeignContext); }
    let operation_error = unsafe { glGetError() };
    drop(scope);
    let restore_error = unsafe { glGetError() };
    if operation_error != 0 {
        return Err(PixelUnpackError::Gl {
            stage: "client memory operation", code: operation_error,
            cleanup_code: if restore_error == 0 { None } else { Some(restore_error) },
        });
    }
    if restore_error != 0 { return Err(gl_error("client memory state restore", restore_error)); }
    Ok(value)
}
/// One publication; holds exclusive context and PBO borrows until restoration.
pub struct PixelUnpackSession<'a> {
    ctx: &'a mut Context,
    buffer: &'a mut PixelUnpackBuffer,
    texture: Texture,
    saved: SavedUnpack,
    restored: bool,
    cursor: guards::Cursor,
    _render_thread: PhantomData<Rc<()>>,
}
impl PixelUnpackSession<'_> {
    /// Submits nonempty bytes into a fresh range. Invalid/overlapping writes
    /// and writes before the previous range's upload return false without GL.
    pub fn write_slice(&mut self, offset: usize, bytes: &[u8]) -> bool {
        if self.restored
            || self.ctx.context_id() != self.buffer.context
            || !self
                .cursor
                .can_write(self.buffer.capacity, offset, bytes.len())
        {
            return false;
        }
        unsafe {
            glBufferSubData(
                PIXEL_UNPACK_BUFFER,
                offset as _,
                bytes.len() as _,
                bytes.as_ptr().cast(),
            );
        }
        self.cursor.written(offset, bytes.len());
        true
    }
    /// Submits exactly the preceding write as a tightly packed RGBA8 rect.
    /// No clock/error query is added here; finish performs the error check.
    pub fn upload_rgba8_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        offset: usize,
        len: usize,
    ) -> bool {
        if self.restored
            || self.ctx.context_id() != self.buffer.context
            || !self.cursor.can_upload(
                self.buffer.capacity,
                self.texture.width,
                self.texture.height,
                x,
                y,
                width,
                height,
                offset,
                len,
            )
        {
            return false;
        }
        unsafe {
            glTexSubImage2D(
                GL_TEXTURE_2D,
                0,
                x,
                y,
                width,
                height,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                offset as *const GLvoid,
            );
        }
        self.cursor.uploaded(offset, len);
        true
    }
    fn restore(&mut self) -> bool {
        if self.restored {
            return true;
        }
        self.restored = true;
        if self.ctx.context_id() != self.buffer.context {
            return false;
        }
        unsafe {
            self.saved.restore();
        }
        self.ctx.cache.restore_texture_binding(0);
        true
    }
    /// Restores exact captured unpack state and Mini's texture binding, then
    /// checks upload/setup/restore errors. Caller must reject publication on Err.
    pub fn finish(mut self) -> Result<(), PixelUnpackError> {
        if !self.restore() {
            return Err(PixelUnpackError::ForeignContext);
        }
        let error = unsafe { glGetError() };
        if error != 0 {
            return Err(gl_error("session upload/restore", error));
        }
        if self.cursor.has_pending() {
            return Err(PixelUnpackError::IncompleteTransfer);
        }
        Ok(())
    }
}
impl Drop for PixelUnpackSession<'_> {
    fn drop(&mut self) {
        self.restore();
    }
}
