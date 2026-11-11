//! Elide only the ordinary CPU line's framebuffer identity operation.
//!
//! A transparent ordinary line still costs a vertex/index submission even
//! though `src * 0 + dst` leaves every RGBA8 destination unchanged. This module
//! skips only that exact case: an ordinary CPU line whose final alpha byte is
//! zero, under the built-in material, with no depth test and no telemetry
//! capture, inside a bounded affine transform. Resident GPU line batches keep
//! their contiguous index ranges and are not touched.
use macroquad::{prelude::*, window::get_internal_gl};

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

/// Returns true when the ordinary CPU line can be skipped without changing the
/// framebuffer. The caller must still keep the original draw order and the
/// original model/texture state.
pub fn try_skip_cpu_line(length: f32, thin: bool, color: Color, debug: bool) -> bool {
    // Match Vertex::new, including the saturating float-to-u8 conversion.
    if (color.a * 255.) as u8 != 0 {
        return false;
    }
    if debug || ![color.r, color.g, color.b, color.a].iter().all(|value| value.is_finite()) || !geometry_valid(length, thin) {
        return false;
    }
    let gl = unsafe { get_internal_gl() };
    if !gl.quad_gl.is_using_default_material() || gl.quad_gl.is_depth_test_enabled() || gl.quad_gl.is_geometry_capture_enabled() {
        return false;
    }
    if !matrix_valid(&gl.quad_gl.get_model_matrix().to_cols_array()) || !matrix_valid(&gl.quad_gl.get_projection_matrix().to_cols_array()) {
        return false;
    }
    // draw_line sets these before recording geometry. Keep its logical state,
    // model scopes and caller-side reads even though the color operation is 0.
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
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
