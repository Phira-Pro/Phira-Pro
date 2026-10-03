//! Hidden desktop GL benchmark of the full block postprocess, including CPU
//! masks, uploads and scene copy. This is not a mobile/full-game FPS benchmark.
use macroquad::prelude::*;
use prpr::core::{BlockArea, BlockPhase, MSRenderTarget, Matrix, Vector};
use std::time::Instant;

struct Resource {
    camera: Camera2D,
    chart_target: Option<MSRenderTarget>,
}
impl Resource {
    fn apply_model_of(&mut self, mat: &Matrix, f: impl FnOnce(&mut Self)) {
        unsafe { get_internal_gl() }.quad_gl.push_model_matrix(prpr::ext::nalgebra_to_glm(mat));
        f(self);
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
    }
}
#[path = "../src/core/block_shader.rs"]
mod block_shader;

fn conf() -> Conf {
    Conf {
        window_title: "Block postprocess benchmark".into(),
        window_width: 2560,
        window_height: 1600,
        headless: true,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    next_frame().await;
    let mut res = Resource {
        camera: Camera2D::default(),
        chart_target: None,
    };
    block_shader::prepare_block_effects();
    let chart = prpr::parse::parse_phigros(
        &std::fs::read_to_string("data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13/DesultorySignals.technoplanet.0.json").unwrap(),
        Default::default(),
    )
    .unwrap();
    for (width, height) in [(960, 720), (1920, 1440), (2560, 1600)] {
        let aspect = width as f32 / height as f32;
        res.camera.zoom = vec2(1., aspect);
        set_camera(&res.camera);
        unsafe { get_internal_gl() }.quad_gl.viewport(Some((0, 0, width, height)));
        for start in [65., 67., 71.8] {
            let mut elapsed = 0.;
            for i in 0..72 {
                let chart_time = start + i as f64 / 120.;
                let begun = Instant::now();
                let zones: Vec<_> = chart
                    .block_areas
                    .iter()
                    .filter_map(|b| block_shader::Zone::from_area(b, chart_time, aspect))
                    .collect();
                clear_background(Color::new(0.08, 0.10, 0.16, 1.));
                draw_rectangle(-0.4, -0.04, 0.8, 0.08, YELLOW);
                let clock = 20. + i as f32 / 120.;
                block_shader::draw_layer_at(&mut res, aspect, &zones, clock, true, &[]);
                block_shader::draw_layer_at(&mut res, aspect, &zones, clock, false, &[]);
                unsafe {
                    get_internal_gl().flush();
                    miniquad::gl::glFinish();
                    assert_eq!(miniquad::gl::glGetError(), 0);
                }
                if i >= 12 {
                    elapsed += begun.elapsed().as_secs_f64();
                }
            }
            println!(
                "{width}x{height} chart={start}..{:.3} full block CPU+GPU ms={:.3} (desktop, no MSAA/notes/HUD)",
                start + 0.6,
                elapsed * 1000. / 60.
            );
        }
    }
}
