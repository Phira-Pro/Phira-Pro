//! Compare the old full-quad shadow with the equivalent border-only drawing.
use macroquad::{
    miniquad::{BlendFactor, BlendState, BlendValue, Equation},
    prelude::*,
};
use prpr::ui::{rounded_rect_shadow, FontArc, ShadowConfig, TextPainter, Ui};
use std::time::Instant;

fn conf() -> Conf {
    let mut c = prpr::build_conf();
    c.window_title = "Phira Pro UI benchmark".into();
    c.window_width = 2560;
    c.window_height = 1600;
    c.headless = true;
    c.platform.swap_interval = Some(0);
    c
}

fn shader(name: &str) -> &'static str {
    let source = include_str!("../src/ui/shadow.rs");
    let start = source.find(&format!("pub const {name}: &str = r#\"")).unwrap();
    source[start..].split_once("r#\"").unwrap().1.split_once("\"#;").unwrap().0
}

#[macroquad::main(conf)]
async fn main() {
    let mut painter = TextPainter::new(FontArc::try_from_slice(include_bytes!("../../assets/harmonyos.ttf")).unwrap(), None);
    let mat = load_material(
        shader("VERTEX"),
        shader("SHADOW_FRAGMENT"),
        MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
            uniforms: ShadowConfig::uniforms(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut timings = [Vec::new(), Vec::new()];
    let mut images = [None, None];
    // Alternate versions after warming both paths to avoid clock-ramp bias.
    for frame in 0..240 {
        let variant = frame % 2;
        clear_background(Color::new(0.3, 0.5, 0.7, 1.));
        let mut ui = Ui::new(&mut painter, None);
        set_camera(&ui.camera());
        let begin = Instant::now();
        for row in 0..4 {
            for col in 0..4 {
                let rect = Rect::new(-0.98 + col as f32 * 0.5, -0.58 + row as f32 * 0.3, 0.46, 0.26);
                let cfg = ShadowConfig::default();
                if variant == 0 {
                    mat.set_uniform("rect", vec4(rect.x, rect.y, rect.right(), rect.bottom()));
                    cfg.apply(&mat);
                    gl_use_material(mat);
                    let f = cfg.elevation * 3.;
                    draw_rectangle(rect.x - f, rect.y - f, rect.w + f * 2., rect.h + f * 2., WHITE);
                    gl_use_default_material();
                } else {
                    rounded_rect_shadow(&mut ui, rect, &cfg);
                }
            }
        }
        unsafe {
            get_internal_gl().flush();
            miniquad::gl::glFinish();
        }
        if frame >= 80 {
            timings[variant].push(begin.elapsed().as_secs_f64() * 1000.);
        }
        if frame >= 238 {
            images[variant] = Some(get_screen_data().bytes);
        }
        next_frame().await;
    }
    let a = images[0].as_ref().unwrap();
    let b = images[1].as_ref().unwrap();
    let changed = a.iter().zip(b).filter(|(a, b)| a != b).count();
    let max_error = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    for (name, t) in ["full quad", "border strips"].into_iter().zip(&mut timings) {
        let avg = t.iter().sum::<f64>() / t.len() as f64;
        t.sort_by(f64::total_cmp);
        println!("{name}: CPU+GPU avg={avg:.3}ms p95={:.3}ms", t[t.len() * 95 / 100]);
    }
    println!("R8 channel differences={changed}; max error={max_error}");
    assert!(max_error <= 1, "shadow optimization changed visible coverage");
}
