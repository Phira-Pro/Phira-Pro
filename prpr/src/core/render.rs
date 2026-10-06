use macroquad::{
    texture::{RenderTarget, Texture2D},
    window::get_internal_gl,
};
use miniquad::{gl::GLuint, RenderPass, Texture, TextureFormat};
use std::cell::Cell;

#[derive(Clone, Copy)]
struct ImplicitTarget {
    fbo: GLuint,
    input: RenderTarget,
}

// TODO: doc
pub struct MSRenderTarget {
    dim: (u32, u32),
    fbo: GLuint,
    rbo: GLuint,
    dummy: RenderTarget,
    output: [Option<RenderTarget>; 2],
    implicit: [Option<ImplicitTarget>; 2],
    implicit_enabled: bool,
    sample_history_open: Cell<bool>,
    policy: super::render_lifetime::Policy,
}

pub fn copy_fbo(src: GLuint, dst: GLuint, dim: (u32, u32)) -> bool {
    unsafe {
        use miniquad::gl::*;
        glBindFramebuffer(GL_READ_FRAMEBUFFER, src);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, dst);
        let (w, h) = (dim.0 as i32, dim.1 as i32);
        glBlitFramebuffer(0, 0, w, h, 0, 0, w, h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
        glGetError() == GL_NO_ERROR
    }
}

pub fn internal_id(target: RenderTarget) -> GLuint {
    target.render_pass.gl_internal_id(unsafe { get_internal_gl() }.quad_context)
}

impl MSRenderTarget {
    pub fn new(dim: (u32, u32), samples: u32) -> Self {
        Self::with_policy(dim, samples, super::render_lifetime::Policy::LEGACY)
    }

    pub(crate) fn with_policy(dim: (u32, u32), samples: u32, policy: super::render_lifetime::Policy) -> Self {
        let mut fbo = 0;
        let mut rbo = 0;
        unsafe {
            use miniquad::gl::*;
            glGenRenderbuffers(1, &mut rbo as *mut _);
            glBindRenderbuffer(GL_RENDERBUFFER, rbo);
            glRenderbufferStorageMultisample(GL_RENDERBUFFER, samples as _, GL_RGB8, dim.0 as _, dim.1 as _);
            glGenFramebuffers(1, &mut fbo as *mut _);
            glBindFramebuffer(GL_FRAMEBUFFER, fbo);
            glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, rbo);
        }
        let gl = unsafe { get_internal_gl() };
        let texture = Texture::new_render_texture(
            gl.quad_context,
            miniquad::TextureParams {
                width: dim.0,
                height: dim.1,
                format: TextureFormat::RGB8,
                ..Default::default()
            },
        );
        let render_pass = RenderPass::new(gl.quad_context, texture, None);
        // Keep the persistent source available for allocation/capability failure.
        // Each output owns a matching input; effects only swap resolved outputs.
        let dummy_render_pass = RenderPass::from_raw(gl.quad_context, fbo, texture);
        let implicit = Self::implicit_target(texture, fbo, rbo, policy);
        Self {
            dim,
            fbo,
            rbo,
            implicit_enabled: implicit.is_some(),
            implicit: [implicit, None],
            sample_history_open: Cell::new(false),
            policy,
            dummy: RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass: dummy_render_pass,
            },
            output: [
                Some(RenderTarget {
                    texture: Texture2D::from_miniquad_texture(texture),
                    render_pass,
                }),
                None,
            ],
        }
    }

    fn implicit_target(texture: Texture, fbo: GLuint, rbo: GLuint, policy: super::render_lifetime::Policy) -> Option<ImplicitTarget> {
        let (fbo, _samples): (GLuint, i32) = {
            #[cfg(target_os = "android")]
            {
                if policy.implicit_supported() {
                    let Some(samples) = (unsafe { super::render_lifetime::renderbuffer_samples(rbo) }) else {
                        policy.source_unverified();
                        return None;
                    };
                    unsafe { super::render_lifetime::create_implicit_target(texture.gl_internal_id(), samples, fbo, policy) }
                } else {
                    None
                }
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = (texture, fbo, rbo, policy);
                None
            }
        }?;
        let gl = unsafe { get_internal_gl() };
        Some(ImplicitTarget {
            fbo,
            input: RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass: RenderPass::from_raw(gl.quad_context, fbo, texture),
            },
        })
    }

    /// Prepare before the complete chart clear, never in the middle of a sample
    /// history. The second pair is allocated only when an effect has introduced
    /// a second output, so charts without effects retain a single implicit pair.
    pub(crate) fn prepare_chart(&mut self) {
        if !self.implicit_enabled {
            return;
        }
        assert!(!self.sample_history_open.get(), "Previous chart was not resolved");
        if self.implicit[0].is_none() {
            unsafe { get_internal_gl() }.flush();
            let texture = self.output().texture.raw_miniquad_texture_handle();
            self.implicit[0] = Self::implicit_target(texture, self.fbo, self.rbo, self.policy);
            if self.implicit[0].is_none() {
                // No frame has started yet: the original RBO can take over with
                // the original format and sample count, without copying samples.
                self.implicit_enabled = false;
                for target in &mut self.implicit {
                    if let Some(target) = target.take() {
                        unsafe {
                            miniquad::gl::glDeleteFramebuffers(1, &target.fbo);
                        }
                    }
                }
                tracing::warn!("Keeping explicit MSAA: second effect output could not be verified");
                return;
            }
        }
        self.sample_history_open.set(true);
    }

    pub fn blit(&self) {
        assert!(!self.implicit_enabled, "Implicit MSAA cannot preserve samples across an early snapshot");
        copy_fbo(self.fbo, internal_id(self.output[0].unwrap()), self.dim);
    }

    /// Final chart resolve. Early snapshots must continue to use `blit` and
    /// retain individual samples for the later chart layers.
    pub fn resolve_final(&self) {
        if self.implicit_enabled {
            assert!(self.sample_history_open.get() && self.implicit[0].is_some(), "Implicit chart input was not prepared");
            // The explicit attachment owner maintained one continuous sample
            // history. Switching to the single-sample output is its only resolve.
            unsafe {
                miniquad::gl::glBindFramebuffer(miniquad::gl::GL_FRAMEBUFFER, internal_id(self.output[0].unwrap()));
                assert_eq!(miniquad::gl::glGetError(), 0, "Implicit MSAA resolve failed");
            }
            self.sample_history_open.set(false);
            return;
        }
        assert!(copy_fbo(self.fbo, internal_id(self.output[0].unwrap()), self.dim), "Final MSAA resolve failed");
    }

    pub fn swap(&mut self) {
        assert!(!self.sample_history_open.get(), "Cannot swap an unresolved MSAA sample history");
        self.output.swap(0, 1);
        self.implicit.swap(0, 1);
        if self.output[0].is_none() {
            let gl = unsafe { get_internal_gl() };
            let texture = miniquad::Texture::new_render_texture(
                gl.quad_context,
                miniquad::TextureParams {
                    width: self.dim.0,
                    height: self.dim.1,
                    format: TextureFormat::RGB8,
                    ..Default::default()
                },
            );
            let render_pass = RenderPass::new(gl.quad_context, texture, None);
            self.output[0] = Some(RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass,
            });
            copy_fbo(internal_id(self.output[1].unwrap()), internal_id(self.output[0].unwrap()), self.dim);
        }
    }

    pub(crate) fn retains_pass(&self) -> bool {
        self.policy.retain
    }

    pub fn input(&self) -> RenderTarget {
        self.implicit[0]
            .filter(|_| self.implicit_enabled)
            .map_or(self.dummy, |target| target.input)
    }

    pub fn output(&self) -> RenderTarget {
        self.output[0].unwrap()
    }

    pub fn old(&self) -> RenderTarget {
        self.output[1].unwrap()
    }
}

impl Drop for MSRenderTarget {
    fn drop(&mut self) {
        unsafe {
            use miniquad::gl::*;
            glDeleteRenderbuffers(1, &self.rbo as *const _);
            glDeleteFramebuffers(1, &self.fbo as *const _);
            for target in self.implicit.iter().flatten() {
                glDeleteFramebuffers(1, &target.fbo);
            }
        }
        for target in self.output.iter().flatten() {
            target.delete();
        }
    }
}
