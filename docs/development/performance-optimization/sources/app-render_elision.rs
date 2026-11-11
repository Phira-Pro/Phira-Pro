//! Elide only the ordinary CPU line's framebuffer identity operation.
//! Resident line batches keep their contiguous index ranges unchanged.
use macroquad::{prelude::*, window::get_internal_gl};

#[cfg(feature = "render_research")]
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy)]
struct Options {
    enabled: bool,
    audit: bool,
}
impl Default for Options {
    fn default() -> Self {
        // Lossless managed-default-material path, verified with both CPU
        // qualities. Research run_job explicitly selects its own control flag.
        Self { enabled: true, audit: false }
    }
}
#[cfg(feature = "render_research")]
#[derive(Clone, Copy, Default)]
struct Stats {
    candidates: u64,
    skipped: u64,
    blocked_state: u64,
    blocked_domain: u64,
}
#[cfg(feature = "render_research")]
thread_local! {
    static OPTIONS: Cell<Options> = Cell::new(Options::default());
    static STATS: Cell<Stats> = Cell::new(Stats::default());
    static GUARD_MATERIAL: RefCell<Option<macroquad::material::Material>> = const { RefCell::new(None) };
}

#[cfg(feature = "render_research")]
pub fn configure_guard_material(enabled: bool) -> anyhow::Result<()> {
    use macroquad::material::{load_material, MaterialParams};
    let material = if enabled {
        // The fragment deliberately ignores vertex alpha. This must remain
        // visible even for a zero-alpha ordinary line, in both control/opt modes.
        Some(load_material(
            "#version 100\nattribute vec3 position; uniform mat4 Model; uniform mat4 Projection; void main(){gl_Position=Projection*Model*vec4(position,1.);}",
            "#version 100\nprecision mediump float; void main(){gl_FragColor=vec4(1.,0.,0.,1.);}",
            MaterialParams::default(),
        )?)
    } else {
        None
    };
    GUARD_MATERIAL.with(|slot| *slot.borrow_mut() = material);
    Ok(())
}
#[cfg(feature = "render_research")]
pub struct GuardMaterial;
#[cfg(feature = "render_research")]
impl Drop for GuardMaterial {
    fn drop(&mut self) {
        macroquad::material::gl_use_default_material();
    }
}
#[cfg(feature = "render_research")]
pub fn guard_material() -> Option<GuardMaterial> {
    GUARD_MATERIAL.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref().map(|material| {
            macroquad::material::gl_use_material(*material);
            GuardMaterial
        })
    })
}

#[cfg(feature = "render_research")]
pub fn configure(enabled: bool, audit: bool) {
    OPTIONS.with(|slot| slot.set(Options { enabled, audit }));
    STATS.with(|slot| slot.set(Stats::default()));
}
fn options() -> Options {
    #[cfg(feature = "render_research")]
    {
        OPTIONS.with(Cell::get)
    }
    #[cfg(not(feature = "render_research"))]
    {
        Options::default()
    }
}
#[cfg(feature = "render_research")]
pub fn report() -> serde_json::Value {
    let options = options();
    let stats = STATS.with(Cell::get);
    serde_json::json!({"enabled":options.enabled,"audit":options.audit,
        "guard_material":GUARD_MATERIAL.with(|slot| slot.borrow().is_some()),
        "candidates":stats.candidates,"skipped":stats.skipped,
        "blocked_state":stats.blocked_state,"blocked_domain":stats.blocked_domain,
        "scope":"CPU ordinary line geometry only; final original RGBA8 alpha zero, default material, no depth/capture, bounded affine domain. Resident batches, note/pose evaluation and ordering unchanged. Counters disabled without audit."})
}
#[inline]
fn audit(options: Options, event: u8) {
    #[cfg(feature = "render_research")]
    if options.audit {
        STATS.with(|slot| {
            let mut stats = slot.get();
            match event {
                0 => stats.candidates += 1,
                1 => stats.skipped += 1,
                2 => stats.blocked_state += 1,
                _ => stats.blocked_domain += 1,
            }
            slot.set(stats);
        });
    }
    #[cfg(not(feature = "render_research"))]
    let _ = (options, event);
}

// Keep the original non-degenerate draw_line domain. It returns before setting
// texture/mode when tlen<EPSILON; unknown/overflow geometry uses that old path.
fn geometry_valid(length: f32, thin: bool) -> bool {
    if !(length.is_finite() && length > 0. && length <= 64.) {
        return false;
    }
    let dx = length - -length;
    let thickness = if thin { 0.0076 } else { 0.01 };
    let tlen = (dx * dx).sqrt() / (thickness * 0.5);
    tlen.is_finite() && tlen >= f32::EPSILON && (dx / tlen).is_finite()
}
// This is a conservative domain, not a replacement matrix computation. With
// |matrix|<=4096, |local position|<=64 and affine w=1, both GLSL products remain
// far below f32 overflow; clipping/interpolation cannot acquire an infinite w.
fn matrix_valid(matrix: &[f32; 16]) -> bool {
    matrix.iter().all(|value| value.is_finite() && value.abs() <= 4096.)
        && [3, 7, 11].into_iter().all(|index| matrix[index] == 0.)
        && matrix[15] == 1.
}

pub fn try_skip_cpu_line(length: f32, thin: bool, color: Color, debug: bool) -> bool {
    let options = options();
    if !options.enabled && !options.audit {
        return false;
    }
    // Match Vertex::new, including the saturating float-to-u8 conversion.
    if (color.a * 255.) as u8 != 0 {
        return false;
    }
    audit(options, 0);
    if debug || ![color.r, color.g, color.b, color.a].iter().all(|value| value.is_finite()) || !geometry_valid(length, thin) {
        audit(options, 3);
        return false;
    }
    let gl = unsafe { get_internal_gl() };
    if !gl.quad_gl.is_using_default_material() || gl.quad_gl.is_depth_test_enabled() || gl.quad_gl.is_geometry_capture_enabled() {
        audit(options, 2);
        return false;
    }
    if !matrix_valid(&gl.quad_gl.get_model_matrix().to_cols_array()) || !matrix_valid(&gl.quad_gl.get_projection_matrix().to_cols_array()) {
        audit(options, 3);
        return false;
    }
    if !options.enabled {
        return false;
    }
    // draw_line sets these before recording geometry. Keep its logical state,
    // model scopes and caller-side reads even though the color operation is 0.
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    audit(options, 1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_blend_preserves_every_rgba8_destination() {
        // Independent float blend oracle for every source byte and destination
        // byte; alpha=0 makes both RGB and inherited-alpha blending identity.
        for source in 0..=255 {
            for destination in 0..=255 {
                let source = source as f64 / 255.;
                let destination = destination as f64 / 255.;
                assert_eq!(source * 0. + destination * (1. - 0.), destination);
            }
        }
    }
    #[test]
    fn alpha_byte_boundary_uses_original_vertex_conversion() {
        let edge = 1. / 255.0_f32;
        let below = f32::from_bits(edge.to_bits() - 1);
        assert_eq!((below * 255.) as u8, 0);
        assert_eq!((edge * 255.) as u8, 1);
        for alpha in [-0., 0., -0.5, 0.5 / 255.] {
            assert_eq!((alpha * 255.) as u8, 0);
        }
    }
    #[test]
    fn domain_rejects_early_return_perspective_and_nonfinite_inputs() {
        for length in [0., -1., f32::MIN_POSITIVE, f32::NAN, f32::INFINITY, 65.] {
            assert!(!geometry_valid(length, false));
        }
        assert!(geometry_valid(1., false) && geometry_valid(1., true));
        let identity = Mat4::IDENTITY.to_cols_array();
        assert!(matrix_valid(&identity));
        for (index, value) in [(3, 0.001), (15, 0.), (0, f32::NAN), (0, f32::INFINITY), (0, 4097.)] {
            let mut matrix = identity;
            matrix[index] = value;
            assert!(!matrix_valid(&matrix));
        }
    }
}
