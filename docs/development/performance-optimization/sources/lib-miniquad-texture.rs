use crate::{native::gl::*, native::*, Context};

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub struct Texture {
    pub(crate) texture: GLuint,
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

impl Texture {
    pub fn empty() -> Texture {
        Texture {
            texture: 0,
            width: 0,
            height: 0,
            format: TextureFormat::RGBA8,
        }
    }

    pub fn gl_internal_id(&self) -> GLuint {
        self.texture
    }

    pub unsafe fn from_raw_id(texture: GLuint, format: TextureFormat) -> Self {
        Self {
            texture,
            width: 0,
            height: 0,
            format,
        }
    }

    /// Delete GPU texture, leaving handle unmodified.
    ///
    /// More high-level code on top of miniquad probably is going to call this in Drop implementation of some
    /// more RAII buffer object.
    ///
    /// There is no protection against using deleted textures later. However its not an UB in OpenGl and thats why
    /// this function is not marked as unsafe
    pub fn delete(&self) {
        unsafe {
            glDeleteTextures(1, &self.texture as *const _);
        }
    }
}

/// List of all the possible formats of input data when uploading to texture.
/// The list is built by intersection of texture formats supported by 3.3 core profile and webgl1.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum TextureFormat {
    RGB8,
    RGBA8,
    Depth,
    Alpha,
    LuminanceAlpha,
}

impl TextureFormat {
    /// Converts from TextureFormat to (internal_format, format, pixel_type)
    pub fn into_gl_params(self, alpha_texture: bool) -> (GLenum, GLenum, GLenum) {
        match self {
            TextureFormat::RGB8 => (GL_RGB, GL_RGB, GL_UNSIGNED_BYTE),
            TextureFormat::RGBA8 => (GL_RGBA, GL_RGBA, GL_UNSIGNED_BYTE),
            TextureFormat::Depth => (GL_DEPTH_COMPONENT, GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT),

            #[cfg(target_arch = "wasm32")]
            TextureFormat::Alpha => (GL_ALPHA, GL_ALPHA, GL_UNSIGNED_BYTE),
            #[cfg(not(target_arch = "wasm32"))]
            TextureFormat::Alpha if alpha_texture => (GL_ALPHA, GL_ALPHA, GL_UNSIGNED_BYTE),
            #[cfg(not(target_arch = "wasm32"))]
            TextureFormat::Alpha => (GL_R8, GL_RED, GL_UNSIGNED_BYTE), // texture updates will swizzle Red -> Alpha to match WASM

            #[cfg(target_arch = "wasm32")]
            TextureFormat::LuminanceAlpha => {
                (GL_LUMINANCE_ALPHA, GL_LUMINANCE_ALPHA, GL_UNSIGNED_BYTE)
            }
            #[cfg(not(target_arch = "wasm32"))]
            TextureFormat::LuminanceAlpha if alpha_texture => {
                (GL_LUMINANCE_ALPHA, GL_LUMINANCE_ALPHA, GL_UNSIGNED_BYTE)
            }
            #[cfg(not(target_arch = "wasm32"))]
            TextureFormat::LuminanceAlpha => (GL_RG, GL_RG, GL_UNSIGNED_BYTE), // texture updates will swizzle Green -> Alpha to match WASM
        }
    }

    /// Returns the size in bytes of texture with `dimensions`.
    pub fn size(self, width: u32, height: u32) -> u32 {
        let square = width * height;
        match self {
            TextureFormat::RGB8 => 3 * square,
            TextureFormat::RGBA8 => 4 * square,
            TextureFormat::Depth => 2 * square,
            TextureFormat::Alpha => 1 * square,
            TextureFormat::LuminanceAlpha => 2 * square,
        }
    }
}

impl Default for TextureParams {
    fn default() -> Self {
        TextureParams {
            format: TextureFormat::RGBA8,
            wrap: TextureWrap::Clamp,
            filter: FilterMode::Linear,
            width: 0,
            height: 0,
        }
    }
}

/// Sets the wrap parameter for texture.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TextureWrap {
    /// Samples at coord x + 1 map to coord x.
    Repeat = GL_REPEAT as isize,
    /// Samples at coord x + 1 map to coord 1 - x.
    Mirror = GL_MIRRORED_REPEAT as isize,
    /// Samples at coord x + 1 map to coord 1.
    Clamp = GL_CLAMP_TO_EDGE as isize,
}

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub enum FilterMode {
    Linear = GL_LINEAR as isize,
    Nearest = GL_NEAREST as isize,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TextureAccess {
    /// Used as read-only from GPU
    Static,
    /// Can be written to from GPU
    RenderTarget,
}

#[derive(Debug, Copy, Clone)]
pub struct TextureParams {
    pub format: TextureFormat,
    pub wrap: TextureWrap,
    pub filter: FilterMode,
    pub width: u32,
    pub height: u32,
}

/// A ready owner, or a refusal after which the original mutable constructor is safe.
#[derive(Debug, PartialEq)]
pub enum ImmutableTextureAttempt {
    Ready(Texture),
    Declined(ImmutableTextureDecline),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImmutableTextureDecline {
    UnsupportedPlatform,
    MissingEntryPoints,
    UnsupportedApi,
    NameZero,
    Gl { stage: &'static str, code: u32 },
}
/// Fatal errors must not be silently retried through a legacy GL upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImmutableTextureFailure {
    InvalidData {
        width: u16,
        height: u16,
        bytes: usize,
    },
    InvalidMaximum(i32),
    TextureTooLarge {
        maximum: i32,
    },
    ForeignContext,
    Gl {
        stage: &'static str,
        code: u32,
    },
    Unpack(crate::PixelUnpackError),
    BindingRestore {
        stage: &'static str,
        code: u32,
        active_restore_code: Option<u32>,
    },
    Creation(ImmutableTextureDecline),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableTextureError {
    pub failure: ImmutableTextureFailure,
    pub unpack_cleanup: Option<crate::PixelUnpackError>,
    pub binding_cleanup: Option<ImmutableTextureFailure>,
    pub delete_code: Option<u32>,
}
impl ImmutableTextureError {
    fn new(failure: ImmutableTextureFailure) -> Self {
        Self {
            failure,
            unpack_cleanup: None,
            binding_cleanup: None,
            delete_code: None,
        }
    }
    pub fn is_cleanup_failed(&self) -> bool {
        let inner_unpack_cleanup = match &self.failure {
            ImmutableTextureFailure::Unpack(
                crate::PixelUnpackError::ClientMemoryBindingMismatch { cleanup_code, .. }
                | crate::PixelUnpackError::BufferNameZero { cleanup_code }
                | crate::PixelUnpackError::CapacityMismatch { cleanup_code, .. }
                | crate::PixelUnpackError::Gl { cleanup_code, .. },
            ) => cleanup_code.is_some(),
            _ => false,
        };
        inner_unpack_cleanup
            || self.unpack_cleanup.is_some()
            || self.binding_cleanup.is_some()
            || self.delete_code.is_some()
    }
}
impl std::fmt::Display for ImmutableTextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Immutable RGBA8 texture: {:?}; unpack cleanup: {:?}; binding cleanup: {:?}; delete: {:?}",
            self.failure, self.unpack_cleanup, self.binding_cleanup, self.delete_code)
    }
}
impl std::error::Error for ImmutableTextureError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImmutableApi {
    Es,
    Desktop,
}
fn immutable_api(version: &[u8]) -> Option<ImmutableApi> {
    let (version, es) = match version.strip_prefix(b"OpenGL ES ") {
        Some(version) => (version, true),
        None => (version, false),
    };
    let dot = version.iter().position(|&byte| byte == b'.')?;
    fn number(bytes: &[u8]) -> Option<u32> {
        if bytes.is_empty() {
            return None;
        }
        bytes.iter().try_fold(0u32, |value, &digit| {
            if !digit.is_ascii_digit() {
                return None;
            }
            value.checked_mul(10)?.checked_add((digit - b'0') as u32)
        })
    }
    let major = number(&version[..dot])?;
    let remaining = &version[dot + 1..];
    let minor_end = remaining
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(remaining.len());
    let minor = number(&remaining[..minor_end])?;
    if remaining
        .get(minor_end)
        .is_some_and(|byte| *byte != b'.' && *byte != b' ')
    {
        return None;
    }
    if es && major >= 3 {
        Some(ImmutableApi::Es)
    } else if !es && (major > 4 || (major == 4 && minor >= 2)) {
        Some(ImmutableApi::Desktop)
    } else {
        None
    }
}
fn rgba8_input_valid(width: u16, height: u16, bytes: usize) -> bool {
    width != 0
        && height != 0
        && (width as usize)
            .checked_mul(height as usize)
            .and_then(|size| size.checked_mul(4))
            == Some(bytes)
}

#[cfg(all(
    not(target_arch = "wasm32"),
    any(
        target_os = "windows",
        target_os = "android",
        all(target_os = "linux", not(target_env = "ohos"))
    )
))]
mod immutable {
    use super::*;
    use crate::graphics::pixel_unpack::ClientMemoryUnpack;
    const ACTIVE_TEXTURE: GLenum = 0x84e0;
    const TEXTURE_BINDING_2D: GLenum = 0x8069;
    const CONTEXT_PROFILE_MASK: GLenum = 0x9126;
    const CORE_PROFILE_BIT: GLint = 1;

    fn gl_failure(stage: &'static str, code: u32) -> ImmutableTextureFailure {
        ImmutableTextureFailure::Gl { stage, code }
    }
    fn creation_failure(stage: &'static str, code: u32) -> ImmutableTextureFailure {
        // OOM leaves GL state undefined; context loss cannot safely use a fallback.
        if code == 0x0505 || code == 0x0507 {
            gl_failure(stage, code)
        } else {
            ImmutableTextureFailure::Creation(ImmutableTextureDecline::Gl { stage, code })
        }
    }
    unsafe fn check(stage: &'static str) -> Result<(), ImmutableTextureFailure> {
        match glGetError() {
            0 => Ok(()),
            code => Err(gl_failure(stage, code)),
        }
    }
    unsafe fn check_creation(stage: &'static str) -> Result<(), ImmutableTextureFailure> {
        match glGetError() {
            0 => Ok(()),
            code => Err(creation_failure(stage, code)),
        }
    }

    /// This guard touches only unit zero, never the draw queue or another owner.
    struct TextureBinding<'a> {
        ctx: &'a mut Context,
        context: usize,
        active: GLint,
        binding: GLuint,
        restored: bool,
    }
    impl<'a> TextureBinding<'a> {
        unsafe fn capture(ctx: &'a mut Context) -> Result<Self, ImmutableTextureError> {
            let mut active = 0;
            glGetIntegerv(ACTIVE_TEXTURE, &mut active);
            check("active texture capture").map_err(ImmutableTextureError::new)?;
            glActiveTexture(GL_TEXTURE0);
            let mut binding = 0;
            glGetIntegerv(TEXTURE_BINDING_2D, &mut binding);
            if let Err(failure) = check("unit zero binding capture") {
                glActiveTexture(active as GLenum);
                let cleanup = check("capture active texture restore").err();
                let mut error = ImmutableTextureError::new(failure);
                error.binding_cleanup = cleanup;
                return Err(error);
            }
            let context = ctx.context_id();
            Ok(Self {
                ctx,
                context,
                active,
                binding: binding as GLuint,
                restored: false,
            })
        }
        fn restore(&mut self) -> Result<(), ImmutableTextureFailure> {
            if self.restored {
                return Ok(());
            }
            self.restored = true;
            if self.ctx.context_id() != self.context {
                return Err(ImmutableTextureFailure::ForeignContext);
            }
            unsafe {
                glActiveTexture(GL_TEXTURE0);
                let selection_error = glGetError();
                let (stage, binding_error) = if selection_error != 0 {
                    ("unit zero restore selection", selection_error)
                } else {
                    glBindTexture(GL_TEXTURE_2D, self.binding);
                    let error = glGetError();
                    if error == 0 {
                        self.ctx.cache.textures[0] = self.binding;
                    }
                    ("unit zero binding restore", error)
                };
                glActiveTexture(self.active as GLenum);
                let active_error = glGetError();
                if binding_error != 0 {
                    return Err(ImmutableTextureFailure::BindingRestore {
                        stage,
                        code: binding_error,
                        active_restore_code: if active_error == 0 {
                            None
                        } else {
                            Some(active_error)
                        },
                    });
                }
                if active_error != 0 {
                    return Err(gl_failure("active texture restore", active_error));
                }
            }
            Ok(())
        }
        fn finish(mut self) -> Result<(), ImmutableTextureFailure> {
            self.restore()
        }
    }
    impl Drop for TextureBinding<'_> {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    pub(super) fn create(
        ctx: &mut Context,
        width: u16,
        height: u16,
        bytes: &[u8],
    ) -> Result<ImmutableTextureAttempt, ImmutableTextureError> {
        let loaded = [
            "glGetString",
            "glGetError",
            "glGetIntegerv",
            "glActiveTexture",
            "glBindTexture",
            "glGenTextures",
            "glDeleteTextures",
            "glTexStorage2D",
            "glTexSubImage2D",
            "glTexParameteri",
            "glBindBuffer",
            "glPixelStorei",
        ]
        .iter()
        .all(|name| crate::native::gl::is_function_loaded(name));
        if !loaded {
            return Ok(ImmutableTextureAttempt::Declined(
                ImmutableTextureDecline::MissingEntryPoints,
            ));
        }
        unsafe {
            check("preexisting immutable texture error").map_err(ImmutableTextureError::new)?;
            let version = glGetString(GL_VERSION);
            check("immutable version query").map_err(ImmutableTextureError::new)?;
            let api = if version.is_null() {
                None
            } else {
                immutable_api(std::ffi::CStr::from_ptr(version as _).to_bytes())
            };
            let Some(api) = api else {
                return Ok(ImmutableTextureAttempt::Declined(
                    ImmutableTextureDecline::UnsupportedApi,
                ));
            };
            if api == ImmutableApi::Desktop {
                let mut profile = 0;
                glGetIntegerv(CONTEXT_PROFILE_MASK, &mut profile);
                check("immutable desktop profile query").map_err(ImmutableTextureError::new)?;
                if profile != CORE_PROFILE_BIT {
                    return Ok(ImmutableTextureAttempt::Declined(
                        ImmutableTextureDecline::UnsupportedApi,
                    ));
                }
            }
            let mut maximum = 0;
            glGetIntegerv(GL_MAX_TEXTURE_SIZE, &mut maximum);
            check("immutable maximum size query").map_err(ImmutableTextureError::new)?;
            if maximum <= 0 {
                return Err(ImmutableTextureError::new(
                    ImmutableTextureFailure::InvalidMaximum(maximum),
                ));
            }
            if width as i32 > maximum || height as i32 > maximum {
                return Err(ImmutableTextureError::new(
                    ImmutableTextureFailure::TextureTooLarge { maximum },
                ));
            }
        }
        let binding = unsafe { TextureBinding::capture(ctx)? };
        let (operation, unpack_cleanup, name) = {
            match ClientMemoryUnpack::begin(&mut *binding.ctx) {
                Err(failure) => (Err(ImmutableTextureFailure::Unpack(failure)), None, 0),
                Ok(unpack) => {
                    let mut name = 0;
                    let operation = (|| unsafe {
                        glGenTextures(1, &mut name);
                        check_creation("immutable texture name")?;
                        if name == 0 {
                            return Err(ImmutableTextureFailure::Creation(
                                ImmutableTextureDecline::NameZero,
                            ));
                        }
                        glBindTexture(GL_TEXTURE_2D, name);
                        check("immutable texture bind")?;
                        glTexStorage2D(GL_TEXTURE_2D, 1, GL_RGBA8, width as i32, height as i32);
                        check_creation("immutable texture storage")?;
                        glTexSubImage2D(
                            GL_TEXTURE_2D,
                            0,
                            0,
                            0,
                            width as i32,
                            height as i32,
                            GL_RGBA,
                            GL_UNSIGNED_BYTE,
                            bytes.as_ptr().cast(),
                        );
                        check_creation("immutable full RGBA8 upload")?;
                        for (parameter, value) in [
                            (GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE),
                            (GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE),
                            (GL_TEXTURE_MIN_FILTER, GL_LINEAR),
                            (GL_TEXTURE_MAG_FILTER, GL_LINEAR),
                        ] {
                            glTexParameteri(GL_TEXTURE_2D, parameter, value as i32);
                            check_creation("immutable texture parameters")?;
                        }
                        Ok(())
                    })();
                    let unpack_cleanup = unpack.finish().err();
                    (operation, unpack_cleanup, name)
                }
            }
        };
        let foreign = matches!(
            unpack_cleanup,
            Some(crate::PixelUnpackError::ForeignContext)
        );
        let binding_cleanup = binding.finish().err();
        if operation.is_ok() && unpack_cleanup.is_none() && binding_cleanup.is_none() {
            return Ok(ImmutableTextureAttempt::Ready(Texture {
                texture: name,
                width: width as u32,
                height: height as u32,
                format: TextureFormat::RGBA8,
            }));
        }
        let foreign = foreign
            || matches!(
                binding_cleanup,
                Some(ImmutableTextureFailure::ForeignContext)
            );
        let delete_code = if name != 0 && !foreign {
            unsafe {
                glDeleteTextures(1, &name);
                match glGetError() {
                    0 => None,
                    code => Some(code),
                }
            }
        } else {
            None
        };
        let failure = operation.err().unwrap_or_else(|| {
            if foreign {
                ImmutableTextureFailure::ForeignContext
            } else if let Some(error) = unpack_cleanup.clone() {
                ImmutableTextureFailure::Unpack(error)
            } else {
                binding_cleanup.clone().unwrap()
            }
        });
        let error = ImmutableTextureError {
            failure,
            unpack_cleanup,
            binding_cleanup,
            delete_code,
        };
        if !error.is_cleanup_failed() {
            if let ImmutableTextureFailure::Creation(reason) = error.failure {
                return Ok(ImmutableTextureAttempt::Declined(reason));
            }
        }
        Err(error)
    }
}

impl Texture {
    /// Shorthand for `new(ctx, TextureAccess::RenderTarget, params)`
    pub fn new_render_texture(ctx: &mut Context, params: TextureParams) -> Texture {
        Self::new(ctx, TextureAccess::RenderTarget, None, params)
    }

    pub fn new(
        ctx: &mut Context,
        _access: TextureAccess,
        bytes: Option<&[u8]>,
        params: TextureParams,
    ) -> Texture {
        if let Some(bytes_data) = bytes {
            assert_eq!(
                params.format.size(params.width, params.height) as usize,
                bytes_data.len()
            );
        }

        let (internal_format, format, pixel_type) =
            params.format.into_gl_params(ctx.features().alpha_texture);

        ctx.cache.store_texture_binding(0);

        let mut texture: GLuint = 0;

        unsafe {
            glGenTextures(1, &mut texture as *mut _);
            ctx.cache.bind_texture(0, texture);
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1); // miniquad always uses row alignment of 1

            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                internal_format as i32,
                params.width as i32,
                params.height as i32,
                0,
                format,
                pixel_type,
                match bytes {
                    Some(bytes) => bytes.as_ptr() as *const _,
                    Option::None => std::ptr::null(),
                },
            );

            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, params.wrap as i32);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, params.wrap as i32);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, params.filter as i32);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, params.filter as i32);

            #[cfg(not(target_arch = "wasm32"))]
            match params.format {
                // on non-WASM alpha value is stored in red channel
                // swizzle red -> alpha, zero red
                TextureFormat::Alpha if !ctx.features().alpha_texture => {
                    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_A, GL_RED as _);
                    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_R, GL_ZERO as _);
                }
                // on non-WASM luminance is stored in red channel, alpha is stored in green channel
                // keep red, swizzle green -> alpha, zero green
                TextureFormat::LuminanceAlpha if !ctx.features().alpha_texture => {
                    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_A, GL_GREEN as _);
                    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_G, GL_ZERO as _);
                }
                _ => {}
            }
        }
        ctx.cache.restore_texture_binding(0);

        Texture {
            texture,
            width: params.width,
            height: params.height,
            format: params.format,
        }
    }

    /// Upload texture to GPU with given TextureParams
    pub fn from_data_and_format(ctx: &mut Context, bytes: &[u8], params: TextureParams) -> Texture {
        Self::new(ctx, TextureAccess::Static, Some(bytes), params)
    }

    /// Creates one initialized, fixed-size RGBA8 level using core immutable storage.
    /// Keeps Linear/Clamp sampling and exact caller bytes. Updates remain legal;
    /// resize is not legal for this texture. The returned owner still requires delete.
    /// A Declined result permits the original mutable constructor; Err does not.
    /// Unsupported platforms perform no GL calls. Requires the creating GL context
    /// to be current on its render thread, as for the existing Texture constructors.
    pub fn try_from_rgba8_immutable(
        ctx: &mut Context,
        width: u16,
        height: u16,
        bytes: &[u8],
    ) -> Result<ImmutableTextureAttempt, ImmutableTextureError> {
        if !rgba8_input_valid(width, height, bytes.len()) {
            return Err(ImmutableTextureError::new(
                ImmutableTextureFailure::InvalidData {
                    width,
                    height,
                    bytes: bytes.len(),
                },
            ));
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            any(
                target_os = "windows",
                target_os = "android",
                all(target_os = "linux", not(target_env = "ohos"))
            )
        ))]
        {
            immutable::create(ctx, width, height, bytes)
        }
        #[cfg(not(all(
            not(target_arch = "wasm32"),
            any(
                target_os = "windows",
                target_os = "android",
                all(target_os = "linux", not(target_env = "ohos"))
            )
        )))]
        {
            let _ = ctx;
            Ok(ImmutableTextureAttempt::Declined(
                ImmutableTextureDecline::UnsupportedPlatform,
            ))
        }
    }

    /// Upload RGBA8 texture to GPU
    pub fn from_rgba8(ctx: &mut Context, width: u16, height: u16, bytes: &[u8]) -> Texture {
        assert_eq!(width as usize * height as usize * 4, bytes.len());

        Self::from_data_and_format(
            ctx,
            bytes,
            TextureParams {
                width: width as _,
                height: height as _,
                format: TextureFormat::RGBA8,
                wrap: TextureWrap::Clamp,
                filter: FilterMode::Linear,
            },
        )
    }

    pub fn set_filter(&self, ctx: &mut Context, filter: FilterMode) {
        ctx.cache.store_texture_binding(0);
        ctx.cache.bind_texture(0, self.texture);
        unsafe {
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, filter as i32);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, filter as i32);
        }
        ctx.cache.restore_texture_binding(0);
    }

    pub fn set_wrap(&self, ctx: &mut Context, wrap: TextureWrap) {
        ctx.cache.store_texture_binding(0);
        ctx.cache.bind_texture(0, self.texture);
        unsafe {
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, wrap as i32);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, wrap as i32);
        }
        ctx.cache.restore_texture_binding(0);
    }

    pub fn resize(&mut self, ctx: &mut Context, width: u32, height: u32, bytes: Option<&[u8]>) {
        ctx.cache.store_texture_binding(0);
        ctx.cache.bind_texture(0, self.texture);

        let (internal_format, format, pixel_type) =
            self.format.into_gl_params(ctx.features().alpha_texture);

        self.width = width;
        self.height = height;

        unsafe {
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1); // miniquad always uses row alignment of 1

            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                internal_format as i32,
                self.width as i32,
                self.height as i32,
                0,
                format,
                pixel_type,
                match bytes {
                    Some(bytes) => bytes.as_ptr() as *const _,
                    Option::None => std::ptr::null(),
                },
            );
        }

        ctx.cache.restore_texture_binding(0);
    }

    /// Update whole texture content
    /// bytes should be width * height * 4 size - non rgba8 textures are not supported yet anyway
    pub fn update(&self, ctx: &mut Context, bytes: &[u8]) {
        assert_eq!(self.size(self.width, self.height), bytes.len());

        self.update_texture_part(
            ctx,
            0 as _,
            0 as _,
            self.width as _,
            self.height as _,
            bytes,
        )
    }

    pub fn update_texture_part(
        &self,
        ctx: &mut Context,
        x_offset: i32,
        y_offset: i32,
        width: i32,
        height: i32,
        bytes: &[u8],
    ) {
        assert_eq!(self.size(width as _, height as _), bytes.len());
        assert!(x_offset + width <= self.width as _);
        assert!(y_offset + height <= self.height as _);

        ctx.cache.store_texture_binding(0);
        ctx.cache.bind_texture(0, self.texture);

        let (_, format, pixel_type) = self.format.into_gl_params(ctx.features().alpha_texture);

        unsafe {
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1); // miniquad always uses row alignment of 1

            glTexSubImage2D(
                GL_TEXTURE_2D,
                0,
                x_offset as _,
                y_offset as _,
                width as _,
                height as _,
                format,
                pixel_type,
                bytes.as_ptr() as *const _,
            );
        }

        ctx.cache.restore_texture_binding(0);
    }

    /// Read texture data into CPU memory
    pub fn read_pixels(&self, bytes: &mut [u8]) {
        if self.format == TextureFormat::Alpha || self.format == TextureFormat::LuminanceAlpha {
            unimplemented!("read_pixels is not implement for Alpha and LuminanceAlpha textures");
        }
        let (_, format, pixel_type) = self.format.into_gl_params(false);

        let mut fbo = 0;
        unsafe {
            let mut binded_fbo: i32 = 0;
            glGetIntegerv(gl::GL_DRAW_FRAMEBUFFER_BINDING, &mut binded_fbo);
            glGenFramebuffers(1, &mut fbo);
            glBindFramebuffer(gl::GL_FRAMEBUFFER, fbo);
            glFramebufferTexture2D(
                gl::GL_FRAMEBUFFER,
                gl::GL_COLOR_ATTACHMENT0,
                gl::GL_TEXTURE_2D,
                self.texture,
                0,
            );

            glReadPixels(
                0,
                0,
                self.width as _,
                self.height as _,
                format,
                pixel_type,
                bytes.as_mut_ptr() as _,
            );

            glBindFramebuffer(gl::GL_FRAMEBUFFER, binded_fbo as _);
            glDeleteFramebuffers(1, &fbo);
        }
    }

    #[inline]
    fn size(&self, width: u32, height: u32) -> usize {
        self.format.size(width, height) as usize
    }
}

#[cfg(test)]
mod immutable_guard_tests {
    use super::*;
    #[test]
    fn core_storage_versions_are_exact_and_old_or_unknown_apis_decline() {
        for version in [b"OpenGL ES 3.0 Vendor".as_slice(), b"OpenGL ES 3.2"] {
            assert_eq!(immutable_api(version), Some(ImmutableApi::Es));
        }
        for version in [b"4.2 Vendor".as_slice(), b"4.6.0 Vendor"] {
            assert_eq!(immutable_api(version), Some(ImmutableApi::Desktop));
        }
        for version in [
            b"OpenGL ES 2.0".as_slice(),
            b"4.1 Vendor",
            b"WebGL 2.0",
            b"",
            b"OpenGL ES-CM 3.0",
            b"OpenGL ES 3.",
            b"4.2garbage",
            b"42949672960.2",
        ] {
            assert_eq!(immutable_api(version), None);
        }
    }
    #[test]
    fn exact_nonempty_rgba_input_is_required_before_gl() {
        assert!(rgba8_input_valid(2, 3, 24));
        assert!(!rgba8_input_valid(0, 3, 0));
        assert!(!rgba8_input_valid(2, 0, 0));
        assert!(!rgba8_input_valid(2, 3, 23));
        assert!(!rgba8_input_valid(2, 3, 25));
    }
}
#[cfg(all(
    test,
    not(target_arch = "wasm32"),
    any(
        target_os = "windows",
        target_os = "android",
        all(target_os = "linux", not(target_env = "ohos"))
    )
))]
#[path = "texture_immutable_tests.rs"]
mod immutable_transaction_tests;
