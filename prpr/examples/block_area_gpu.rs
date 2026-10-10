//! Render the production block shader in a hidden native GL context.
//! Run from the workspace: cargo run -p prpr --example block_area_gpu
use macroquad::prelude::*;
use prpr::core::{Anim, AnimFloat, BlockArea, BlockPhase, BlockTransform, MSRenderTarget, Matrix, Vector};
#[path = "../src/core/rpe_block.rs"]
mod rpe_block;

// Supply just the production renderer's model stack and optional chart target.
// No audio or resource pack; render targets below exercise the existing MSAA path.
struct Resource {
    time: f64,
    config: prpr::config::Config,
    camera: Camera2D,
    chart_target: Option<MSRenderTarget>,
    snapshot_blit_sources: Vec<(miniquad::RenderPass, bool)>,
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
use block_shader::Zone;
#[path = "../src/core/block_mask.rs"]
mod reference_mask;
#[path = "../src/core/block_touch.rs"]
mod reference_touch;

fn conf() -> Conf {
    Conf {
        window_title: "Block area GPU regression".into(),
        window_width: std::env::var("BLOCK_GPU_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(960),
        window_height: std::env::var("BLOCK_GPU_HEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(540),
        headless: true,
        sample_count: std::env::var("BLOCK_GPU_SAMPLES").ok().and_then(|s| s.parse().ok()).unwrap_or(4),
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    let aspect = 16. / 9.;
    let mut res = Resource {
        time: 0.,
        config: Default::default(),
        camera: Camera2D {
            zoom: vec2(1., aspect),
            ..Default::default()
        },
        chart_target: None,
        snapshot_blit_sources: Vec::new(),
    };
    next_frame().await;
    set_camera(&res.camera);
    std::fs::create_dir_all("target/block-area-gpu").unwrap();
    clear_background(Color::new(0.1, 0.1, 0.1, 1.));
    assert!(block_shader::prepare_block_effects(), "full shader must compile; simple fallback cannot pass this regression");
    assert!(screen_pixels().bytes.chunks_exact(4).all(|p| p == [25, 25, 25, 255]), "shader warmup must leave the loading frame intact");

    if std::env::var_os("BLOCK_GPU_MATRIX").is_some() {
        viewport_matrix_probe(&mut res);
        return;
    }
    let active = [zone(0., 0., 0.5, 0.25, false, true)];
    let pixels = render(&mut res, aspect, &active, "active-default", 1., false);
    assert!(pixel(&pixels, 580, 270)[0] > pixel(&pixels, 580, 270)[1] + 10, "active fill must be visible");
    assert_eq!(pixel(&pixels, 50, 50), [25, 25, 25, 255]);
    assert!(pixels.bytes.chunks_exact(4).any(|p| p[0] > 180 && p[0] > p[1] * 2), "bright ring must render");
    assert!(pixels.bytes != render(&mut res, aspect, &active, "dissolve-next-frame", 1.5, false).bytes, "compose boundary must evolve");
    native_reference(&active, &pixels, 1., &[]);
    let later = render(&mut res, aspect, &active, "active-later", 9., false);
    assert_ne!(pixels.bytes, later.bytes, "displacement/sparks must animate");
    native_reference(&active, &later, 9., &[]);

    // Production shader, including per-area color textures and coverage parity.
    let mut green_zone = active[0].clone();
    green_zone.color = [84. / 255., 1., 84. / 255.];
    let green = render(&mut res, aspect, &[green_zone.clone()], "active-green", 1., false);
    let green_pixel = pixel(&green, 580, 270);
    assert!(green_pixel[1] > green_pixel[0] + 10, "green event must recolor active fill");
    assert_eq!(pixel(&green, 50, 50), [25, 25, 25, 255], "tint must not leak outside coverage");
    green_zone.active = false;
    let green_preview = render(&mut res, aspect, &[green_zone], "disabled-green", 1., false);
    assert!(pixel(&green_preview, 580, 270)[1] > pixel(&green_preview, 580, 270)[0], "preview must retain event color");
    let mut pair = [zone(-0.5, 0., 0.3, 0.25, false, true), zone(0.5, 0., 0.3, 0.25, false, true)];
    pair[0].color = [84. / 255., 1., 84. / 255.];
    pair[1].color = [84. / 255., 84. / 255., 1.];
    let separate = render(&mut res, aspect, &pair, "separate-green-blue", 1., false);
    assert!(pixel(&separate, 240, 270)[1] > pixel(&separate, 240, 270)[2]);
    assert!(pixel(&separate, 720, 270)[2] > pixel(&separate, 720, 270)[1]);
    assert_eq!(pixel(&separate, 480, 270), [25, 25, 25, 255]);
    assert_eq!(
        render(&mut res, aspect, &active, "default-after-colored", 1., false).bytes,
        pixels.bytes,
        "returning to native red must not reuse a stale color texture"
    );

    // A gray underlay cannot detect channel/tint or HSV mistakes. Exercise the
    // Complete scene sampling with colored tiles and the native /6 Point grid.
    for time in [1., 9.] {
        palette_scene();
        let source = screen_pixels();
        block_shader::draw_layer_at(&mut res, aspect, &active, time, false, &[]);
        let port = screen_pixels();
        port.export_png(&format!("target/block-area-gpu/palette-{time}.png"));
        native_reference_scene(&active, &port, time, &[], Some(&source));
    }

    let holes = [
        zone(0., 0., 1., 1. / aspect, true, true),
        zone(-0.45, -0.22, 0.14, 0.10, false, true),
        zone(-0.45, 0.22, 0.14, 0.10, false, true),
        zone(0.45, -0.22, 0.14, 0.10, false, true),
        zone(0.45, 0.22, 0.14, 0.10, false, true),
    ];
    let inverted = render(&mut res, aspect, &holes, "invert-four-holes", 1., false);
    assert_eq!(pixel(&inverted, 264, 164), [25, 25, 25, 255], "normal zones must open holes in an inverted layer");
    assert!(pixel(&inverted, 480, 270)[0] > pixel(&inverted, 480, 270)[1] + 10);
    native_reference(&holes, &inverted, 1., &[]);
    // MilK combo 102: a white inverted field is cut by native-red normal
    // rectangles. Those rectangles are holes, so their RGB cannot tint rims.
    let mut white_holes = holes.clone();
    white_holes[0].color = [1.; 3];
    for disabled in [false, true] {
        for z in &mut white_holes {
            z.active = !disabled;
        }
        for time in [1., 9., 32.] {
            let name = format!("white-inverted-holes-{}-{time}", if disabled { "disabled" } else { "active" });
            let image = render(&mut res, aspect, &white_holes, &name, time, false);
            let tinted = image.bytes.chunks_exact(4).filter(|p| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap() > 1).count();
            assert_eq!(tinted, 0, "{name}: white inner and outer rims must stay neutral over gray, including displaced pixels");
        }
    }
    render(&mut res, aspect, &[zone(0., 0., 0.5, 0.25, false, false)], "disabled", 1., false);

    let mut ready_zone = zone(0., 0., 0.5, 0.25, false, false);
    ready_zone.ready = true;
    let ready = overlay_probe(&mut res, aspect, &[ready_zone.clone()], 12., &[], "ready-normal");
    native_reference(&[ready_zone.clone()], &ready, 12., &[]);
    let ready_later = overlay_probe(&mut res, aspect, &[ready_zone.clone()], 12.05, &[], "ready-pulse");
    assert_ne!(ready.bytes, ready_later.bytes, "Ready must pulse at native shine speed");
    let mut recorder_ready = ready_zone.clone();
    recorder_ready.line2area = true;
    for time in [2.5_f32, 2.6, 2.7, 2.8, 2.9] {
        let image = render(&mut res, aspect, &[recorder_ready.clone()], "line2area-ready", time, false);
        let baseline = render(&mut res, aspect, &[zone(0., 0., 0.5, 0.25, false, false)], "line2area-disabled", time, false);
        let actual = pixel(&image, 480, 270)[0] as f32 - pixel(&baseline, 480, 270)[0] as f32;
        let expected = 255. * 0.24 * ((time * 37.9).sin() * 0.5 + 1.);
        assert!((actual - expected).abs() <= 2., "Line2Area two-pass Ready pulse: {actual} vs {expected}");
    }
    ready_zone.invert = true;
    let ready_subtract = overlay_probe(&mut res, aspect, &[ready_zone.clone()], 12.1, &[], "ready-subtract");
    native_reference(&[ready_zone], &ready_subtract, 12.1, &[]);
    let touches = [(101_u64, vec2(0.55, 0.48)), (205, vec2(0.43, 0.55))];
    overlay_probe(&mut res, aspect, &active, 13.9, &touches, "hover-show-start");
    let hover = overlay_probe(&mut res, aspect, &active, 14., &touches, "hover-grown");
    native_reference(&active, &hover, 14., &touches);
    let hovered_again = overlay_probe(&mut res, aspect, &active, 14.05, &touches, "hover-sdf-next");
    native_reference(&active, &hovered_again, 14.05, &touches);
    assert_ne!(hover.bytes, hovered_again.bytes, "hover SDF and shine must evolve");
    let without_field = overlay_probe(&mut res, aspect, &[], 14.055, &touches, "hover-field-disappeared");
    native_reference(&[], &without_field, 14.055, &touches);
    assert!(
        without_field.bytes.chunks_exact(4).any(|p| p != [25, 25, 25, 255]),
        "infected held finger must retain hover after every field disappears"
    );
    overlay_probe(&mut res, aspect, &active, 14.06, &[], "hover-hide-start");
    let hidden = overlay_probe(&mut res, aspect, &active, 14.17, &[], "hover-hidden");
    native_reference(&active, &hidden, 14.17, &[]);
    overlay_probe(&mut res, aspect, &active, 14.18, &touches, "hover-reset-show");
    overlay_probe(&mut res, aspect, &active, 14.29, &touches, "hover-reset-grown");
    block_shader::reset_block_effects();
    let reset = overlay_probe(&mut res, aspect, &active, 14.30, &[], "hover-new-chart-reset");
    native_reference(&active, &reset, 14.30, &[]);

    // The underlay note must be captured and displaced by the Active postprocess.
    clear_background(Color::new(0.1, 0.1, 0.1, 1.));
    draw_rectangle(-0.4, -0.08, 0.8, 0.16, YELLOW);
    block_shader::draw_layer_at(&mut res, aspect, &active, 15., false, &[]);
    let covered_note = screen_pixels();
    covered_note.export_png("target/block-area-gpu/note-under-active.png");
    let n = pixel(&covered_note, 480, 270);
    assert_ne!(n, [255, 255, 0, 255], "a note below active block must receive its composite");

    // Exercise the actual manual chart render-pass path, including the dummy
    // multisample pass. Production draw_zones never calls set_camera.
    for samples in [1, 4] {
        res.chart_target = Some(MSRenderTarget::new((960, 540), samples));
        let target = res.chart_target.as_ref().unwrap();
        unsafe { get_internal_gl() }
            .quad_gl
            .render_pass(Some(if samples > 1 { target.input() } else { target.output() }.render_pass));
        clear_background(Color::new(0.1, 0.1, 0.1, 1.));
        draw_rectangle(-0.4, -0.08, 0.8, 0.16, YELLOW);
        if samples > 1 {
            unsafe { get_internal_gl() }.flush();
            target.blit();
        }
        unsafe { get_internal_gl() }.quad_gl.render_pass(Some(target.output().render_pass));
        block_shader::draw_layer_at(&mut res, aspect, &active, 15., false, &[]);
        unsafe { get_internal_gl() }.flush();
        let target = res.chart_target.as_ref().unwrap();
        let tex = target.output().texture.raw_miniquad_texture_handle();
        let mut rgb = vec![0; 960 * 540 * 3];
        tex.read_pixels(&mut rgb);
        let center = &rgb[(270 * 960 + 480) * 3..(270 * 960 + 480) * 3 + 3];
        assert!(center[1] > 180, "MSAA {samples}: scene copy must sample the note in the output pass: {center:?}");
        unsafe { get_internal_gl() }
            .quad_gl
            .render_pass(Some(if samples > 1 { target.input() } else { target.output() }.render_pass));
        let image = render(&mut res, aspect, &active, &format!("active-msaa-{samples}"), 1., true);
        assert!(pixel(&image, 580, 270)[0] > pixel(&image, 580, 270)[1] + 10);
        let note = pixel(&image, 480, 270);
        assert!(note[0] >= 250 && note[1] >= 247 && note[2] == 0, "following notes must use the default yellow material: {note:?}");
        let inverted = render(&mut res, aspect, &holes, &format!("invert-msaa-{samples}"), 1., false);
        assert_eq!(pixel(&inverted, 264, 164)[..3], [25, 25, 25]);
        unsafe { get_internal_gl() }.quad_gl.render_pass(None);
        res.chart_target = None;
    }

    unsafe { get_internal_gl() }.quad_gl.viewport(Some((80, 45, 800, 450)));
    let image = render(&mut res, aspect, &active, "letterbox", 1., false);
    assert_eq!(pixel(&image, 50, 50), [25, 25, 25, 255]);
    assert_eq!(pixel(&image, 100, 100), [25, 25, 25, 255], "viewport resize must retain the mask outside the rectangle");
    assert!(pixel(&image, 580, 270)[0] > pixel(&image, 580, 270)[1] + 10);
    unsafe { get_internal_gl() }.quad_gl.viewport(None);

    // Resize back in the same frame. Deleted GL names must not leave a stale
    // sampler binding in miniquad's cache.
    let resized = render(&mut res, aspect, &active, "resize-back", 1., false);
    assert_eq!(resized.bytes, pixels.bytes);

    // Match Chart::render's Y flip, including an off-center rotated zone.
    let rotated = [Zone {
        center: Vector::new(0.25, 0.16),
        angle: 0.4,
        ..zone(0., 0., 0.2, 0.08, false, true)
    }];
    let model = Matrix::identity().append_nonuniform_scaling(&Vector::new(-1., -1.));
    let mut reflected = None;
    res.apply_model_of(&model, |res| {
        reflected = Some(render(res, aspect, &rotated, "chart-model", 1., false));
    });
    let reflected = reflected.unwrap();
    assert_eq!(pixel(&reflected, 600, 347), [25, 25, 25, 255]);
    let red = pixel(&reflected, 360, 193);
    assert!(red[0] > red[1] + 10, "chart Y flip and flip_x must reflect the original zone");

    // Parse the real native JSON and use the existing, unchanged transforms.
    let source = std::fs::read_to_string("data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13/DesultorySignals.technoplanet.0.json").unwrap();
    let chart = prpr::parse::parse_phigros(&source, Default::default()).unwrap();
    assert_eq!(chart.block_areas.len(), 160);
    for (name, t) in [
        ("opening-1s", 1.),
        ("opening-4s", 4.),
        ("beat-221", 221. * 60. / 202.),
        ("beat-228", 228. * 60. / 202.),
        ("beat-277", 277. * 60. / 202.),
        ("beat-308", 308. * 60. / 202.),
    ] {
        let zones: Vec<_> = chart
            .block_areas
            .iter()
            .filter_map(|b| {
                let phase = b.phase(t);
                if phase == BlockPhase::Hidden {
                    return None;
                }
                let tr = b.transform(t, aspect);
                Some(block_shader::Zone {
                    line2area: false,
                    y_scale: 1.,
                    color: block_shader::DEFAULT_BLOCK_COLOR,
                    center: tr.center,
                    half: tr.size.map(|v| v.abs() * 0.5),
                    angle: tr.rotation.to_radians(),
                    invert: b.is_subtract,
                    active: phase == BlockPhase::Active,
                    ready: phase != BlockPhase::Active && t < b.enable_time && t >= b.enable_time - 0.5,
                    opacity: if phase == BlockPhase::Active {
                        1.
                    } else {
                        ((t - b.appear_time) / 0.5).clamp(0., 1.) as f32
                    },
                })
            })
            .collect();
        println!("{name}: t={t:.6}s, {} visible zones", zones.len());
        render(&mut res, aspect, &zones, name, t as f32, false);
    }
    // Show that a correctly typed scalar uniform works in this macroquad fork.
    scalar_uniform_probe();
    println!("All native GPU checks passed.");
}

// Recorder dimensions exercise the production marker adapter, then the real
// renderer. Gray input isolates forbidden red tint from colored scene pixels.
fn viewport_matrix_probe(res: &mut Resource) {
    let aspect = (screen_width() / screen_height()).min(16. / 9.);
    let viewport_width = (screen_height() * aspect).round() as i32;
    let x = (screen_width() as i32 - viewport_width) / 2;
    res.camera.zoom = vec2(1., aspect);
    res.camera.viewport = Some((x, 0, viewport_width, screen_height() as i32));
    set_camera(&res.camera);
    let marker = rpe_block::Marker::new(0, false, &AnimFloat::fixed(1.));
    let from_rpe = |x: f32, y: f32, width: f32, height: f32, invert, active| {
        let tr = marker.transform(Vector::new(x * 2. / 1350., y * 2. / 900. / aspect),
            Vector::new(width / 900., height / 900.) * (2. / 1350.), Vector::repeat(900.), 0., aspect);
        let mut zone = zone(tr.center.x, tr.center.y, tr.size.x / 2., tr.size.y / 2., invert, active);
        zone.line2area = true;
        zone.y_scale = tr.y_scale;
        zone
    };
    let mut full = from_rpe(0., 0., 1350., 900., false, true);
    full.color = [1.; 3];
    for simple in [false, true] {
        res.config.block_area_simple = simple;
        for active in [false, true] {
            full.active = active;
            let name = format!("matrix-full-{simple}-{active}");
            let image = render(res, aspect, &[full.clone()], &name, 32., false);
            // Every viewport pixel must be covered (letterbox bars excluded).
            for row in image.bytes.chunks_exact(image.width as usize * 4) {
                for p in row[x as usize * 4..(x + viewport_width) as usize * 4].chunks_exact(4) {
                    assert_ne!(p, [25, 25, 25, 255], "{name}: uncovered pixel in chart viewport");
                }
            }
        }
        // A slightly undersized white field and holes touching its top edge:
        // unlike the earlier probe this stresses actual MilK dimensions.
        for active in [false, true] {
            let mut field = from_rpe(0., 0., 1250., 800., true, active);
            field.color = [1.; 3];
            let holes = [field,
                from_rpe(-385., 250., 300., 300., false, active),
                from_rpe(-15., 50., 300., 300., false, active),
                from_rpe(355., 250., 300., 300., false, active)];
            for time in [1., 9., 32., 32.125, 1000.] {
                let name = format!("matrix-white-{simple}-{active}-{time}");
                let image = render(res, aspect, &holes, &name, time, false);
                let tinted = image.bytes.chunks_exact(4).filter(|p| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap() > 1).count();
                assert_eq!(tinted, 0, "{name}: red residue on white rims");
            }
        }
    }
    println!("Viewport matrix passed: {}x{}, chart aspect {aspect}", screen_width(), screen_height());
}

fn zone(x: f32, y: f32, hx: f32, hy: f32, invert: bool, active: bool) -> block_shader::Zone {
    block_shader::Zone {
        line2area: false,
        y_scale: 1.,
        color: block_shader::DEFAULT_BLOCK_COLOR,
        center: Vector::new(x, y),
        half: Vector::new(hx, hy),
        angle: 0.,
        invert,
        active,
        ready: false,
        opacity: 1.,
    }
}

fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
    let i = (y * image.width as usize + x) * 4;
    image.bytes[i..i + 4].try_into().unwrap()
}

fn screen_pixels() -> Image {
    // get_screen_data -> grab_screen binds its scratch texture without restoring
    // miniquad's cache. Direct readback avoids changing any sampler bindings.
    unsafe { get_internal_gl() }.flush();
    let (width, height) = (screen_width() as u16, screen_height() as u16);
    let mut bytes = vec![0; width as usize * height as usize * 4];
    unsafe {
        use miniquad::gl::*;
        let mut read = 0;
        glGetIntegerv(0x8CAA, &mut read);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, 0);
        glReadPixels(0, 0, width as i32, height as i32, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
    }
    Image { width, height, bytes }
}

fn render(res: &mut Resource, aspect: f32, zones: &[block_shader::Zone], name: &str, time: f32, note: bool) -> Image {
    clear_background(Color::new(0.1, 0.1, 0.1, 1.));
    let start = std::time::Instant::now();
    block_shader::draw_zones_at(res, aspect, zones, time);
    if note {
        draw_rectangle(-0.01, -0.01, 0.02, 0.02, YELLOW);
    }
    unsafe { get_internal_gl() }.flush();
    let image = if let Some(target) = &res.chart_target {
        if unsafe { get_internal_gl() }.quad_gl.get_active_render_pass() == Some(target.input().render_pass) {
            target.blit();
        }
        // MSRenderTarget is RGB8; macroquad's get_texture_data allocates RGBA
        // bytes without converting RGB, so read and expand the native format.
        let texture = target.output().texture.raw_miniquad_texture_handle();
        let mut rgb = vec![0; (texture.width * texture.height * 3) as usize];
        texture.read_pixels(&mut rgb);
        Image {
            width: texture.width as u16,
            height: texture.height as u16,
            bytes: rgb.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        }
    } else {
        screen_pixels()
    };
    let error = unsafe { miniquad::gl::glGetError() };
    assert_eq!(error, 0, "{name}: GL error {error:#x}");
    let elapsed = start.elapsed().as_secs_f64() * 1000.;
    image.export_png(&format!("target/block-area-gpu/{name}.png"));
    println!("{name}: center {:?}, draw + readback {:.2}ms", pixel(&image, 480, 270), elapsed);
    image
}

fn scalar_uniform_probe() {
    let vertex = "#version 100\nattribute vec3 position; uniform mat4 Model; uniform mat4 Projection; void main(){gl_Position=Projection*Model*vec4(position,1.0);}";
    let fragment = "#version 100\nprecision highp float; uniform float opacity; void main(){gl_FragColor=vec4(opacity,0.0,0.0,1.0);}";
    let m = load_material(
        vertex,
        fragment,
        MaterialParams {
            uniforms: vec![("opacity".into(), miniquad::UniformType::Float1)],
            ..Default::default()
        },
    )
    .unwrap();
    m.set_uniform("opacity", 0.8_f32);
    gl_use_material(m);
    draw_rectangle(-1., -1., 2., 2., WHITE);
    gl_use_default_material();
    let image = screen_pixels();
    assert_eq!(pixel(&image, 480, 270)[..3], [204, 0, 0]);
    println!("Float1 uniform opacity=0.8_f32: {:?}", pixel(&image, 480, 270));
}

/// Compare the production port with the real exported ActiveBlock fragment, only
/// adapting GLES3 syntax to GLES2 and supplying identical input textures.
fn overlay_probe(res: &mut Resource, aspect: f32, zones: &[Zone], time: f32, touches: &[(u64, Vec2)], name: &str) -> Image {
    clear_background(Color::new(0.1, 0.1, 0.1, 1.));
    block_shader::draw_layer_at(res, aspect, zones, time, false, touches);
    let result = screen_pixels();
    result.export_png(&format!("target/block-area-gpu/{name}.png"));
    result
}

fn native_reference(zones: &[Zone], port: &Image, time: f32, touches: &[(u64, Vec2)]) {
    native_reference_scene(zones, port, time, touches, None);
}

fn palette_scene() {
    clear_background(BLACK);
    let colors = [
        RED,
        GREEN,
        BLUE,
        YELLOW,
        MAGENTA,
        Color::new(0., 1., 1., 1.),
        WHITE,
        Color::new(0.3, 0.15, 0.07, 1.),
    ];
    for y in 0..4 {
        for x in 0..8 {
            draw_rectangle(-1. + x as f32 * 0.25, -9. / 16. + y as f32 * 9. / 32., 0.25, 9. / 32., colors[(x + y * 3) % 8]);
        }
    }
}

fn native_reference_scene(zones: &[Zone], port: &Image, time: f32, touches: &[(u64, Vec2)], input: Option<&Image>) {
    use miniquad::{TextureWrap, UniformType as U};
    let root = std::env::var("PHIRA_OFFICIAL_SRC").unwrap_or_else(|_| "../legacy files/research/official_src".into());
    let source = std::fs::read_to_string(format!("{root}/shader_code/Unlit_ActiveBlock.p0.txt")).unwrap();
    let fragment = source.split("#ifdef FRAGMENT").nth(1).unwrap();
    let fragment = &fragment[..fragment.rfind("\n#endif").unwrap()];
    let fragment = fragment
        .replace("#version 300 es", "#version 100")
        .replace("#define UNITY_LOCATION(x) layout(location = x)", "#define UNITY_LOCATION(x)")
        .replace("in highp", "varying highp")
        .replace("layout(location = 0) out mediump vec4 SV_Target0;", "")
        .replace("SV_Target0", "gl_FragColor")
        .replace("textureLod(", "sampleLod(")
        .replace("texture(", "texture2D(")
        .replace("texture2D(_SceneColor,", "sampleScene(")
        .replace("_Time", "uUnityTime")
        // Only texture plumbing changes: pack the four camera channels to fit
        // miniquad's sampler cache, preserving the complete native body.
        .replace("texture2D(_DisabledNormalBlockRT, vs_TEXCOORD0.xy)", "vec4(texture2D(_Aux, vs_TEXCOORD0.xy).r)")
        .replace("texture2D(_DisabledSubtractBlockRT, vs_TEXCOORD0.xy)", "vec4(0.0, texture2D(_Aux, vs_TEXCOORD0.xy).g, 0.0, 0.0)")
        .replace("texture2D(_ReadyComposeRT, vs_TEXCOORD0.xy)", "vec4(texture2D(_Aux, vs_TEXCOORD0.xy).b)")
        .replace("texture2D(_TouchHoverRT, vs_TEXCOORD0.xy)", "vec4(texture2D(_Aux, vs_TEXCOORD0.xy).a)")
        .replace("sampleLod(_TouchHoverRT, u_xlat0.xy, 0.0)", "vec4(texture2D(_Aux, u_xlat0.xy).a)")
        .replace("_TouchDisplaceMap", "_DisplaceMap")
        .replace("UNITY_LOCATION(9) uniform mediump sampler2D _DisplaceMap;", "")
        .replace(
            "void main()",
            "uniform mediump sampler2D _Aux;\nvec4 sampleLod(sampler2D s, vec2 uv, float lod) { return texture2D(s, uv); }\nvec4 sampleScene(vec2 uv) { vec2 size = floor(_ScreenParams.xy / 6.0); return texture2D(_SceneColor, (floor(clamp(uv, 0.0, 1.0) * size) + 0.5) / size); }\nvoid main()",
        );
    let vertex = r#"#version 100
attribute vec3 position;
uniform mat4 Projection;
uniform mat4 Model;
varying highp vec2 vs_TEXCOORD0;
varying highp vec2 vs_TEXCOORD1;
varying highp vec2 vs_TEXCOORD2;
varying highp vec2 vs_TEXCOORD4;
varying highp vec4 vs_TEXCOORD3;
varying highp vec2 vs_TEXCOORD5;
varying highp float vs_TEXCOORD6;
varying highp vec4 vs_COLOR0;
void main() {
 gl_Position = Projection * Model * vec4(position, 1.0);
 vec2 uv = gl_Position.xy / gl_Position.w * 0.5 + 0.5;
 vs_TEXCOORD0 = uv; vs_TEXCOORD1 = uv * vec2(0.8, 0.3);
 vs_TEXCOORD2 = uv * vec2(3.0, 1.2); vs_TEXCOORD4 = uv * vec2(0.55, 0.3);
 vs_TEXCOORD5 = uv * vec2(1.5, 1.46);
 vs_TEXCOORD3 = vec4(uv, 0.0, 1.0); vs_TEXCOORD6 = 0.5; vs_COLOR0 = vec4(1.0);
}"#;
    let floats: &[(&str, f32)] = &[
        ("_EdgeOpacity", 0.8),
        ("_FillStrength", 0.667),
        ("_FillOpacity", 0.667),
        ("_GlowIntensity", 0.8),
        ("_SparkMapOpacity", 5.69),
        ("_SparkHueShiftAmount", 0.2),
        ("_SparkDisplaceIntensity", 2.39),
        ("_DisplaceBlendIntensity", 0.411),
        ("_DisplaceSpeed", 1.5),
        ("_DisplaceStrength", 0.15),
        ("_TouchPosShine", (0.63 + 0.37 * ((time * 43.).sin() * 0.5 + 0.5)) * 2.),
        ("_TouchPosRadius", 0.5),
        ("_TouchPosSDFSmoothness", 0.47),
        ("_TouchPosSDFFalloff", 0.41),
        ("_BackgroundPixelScale", 6.),
        ("_ShineSpeed", 37.9),
        ("_ShineBrightness", 0.12),
        ("_TouchDisplaceSpeed", 2.9),
        ("_TouchDisplaceStrength", 0.08),
        ("_NoiseEvoSpeed", 0.03),
        ("_NoiseDirChangeSpeed", 60.),
        ("_NoiseDisplaceStrength", 1.),
        ("_NoiseRadius", 0.48),
        ("_NoiseSmoothness", 1.),
        ("_SDFCellSize", 0.11),
        ("_SDFSmoothness", 0.63),
        ("_SDFFalloff", 0.34),
        ("_SDFMoveSpeed", 9.3),
        ("_TouchBackgroundPixelScale", 8.),
    ];
    let vectors = [
        ("uUnityTime", vec4(time / 20., time, time * 2., time * 3.)),
        ("_ScreenParams", vec4(960., 540., 1. + 1. / 960., 1. + 1. / 540.)),
        ("_EffectRT_TexelSize", vec4(1. / 960., 1. / 540., 960., 540.)),
        ("_EdgeColor", vec4(1., 0.33018857, 0.33018857, 1.)),
        ("_FillColor", vec4(0.7132075, 0.23549296, 0.23549296, 1.)),
        ("_GlowColor", vec4(1., 0.17924517, 0.17924517, 1.)),
        ("_DisplaceDirection", vec4(1., 1., 0., 0.)),
        ("_TouchDisplaceDirection", vec4(1., 1., 0., 0.)),
        ("_ShineColor", vec4(1., 1., 1., 1.)),
        ("_TouchGlowColor", vec4(1., 0., 0., 1.)),
        ("_NoiseTint", vec4(1., 0., 0., 1.)),
    ];
    let textures = ["_ComposeRT", "_EffectRT", "_DisplaceMap", "_SparkMap", "_SceneColor", "_Aux", "_NoiseMap"];
    let mut uniforms: Vec<_> = floats.iter().map(|(name, _)| (name.to_string(), U::Float1)).collect();
    uniforms.extend(vectors.iter().map(|(name, _)| (name.to_string(), U::Float4)));
    uniforms.push(("_SparkTint".into(), U::Float3));
    uniforms.push(("_TouchPosCount".into(), U::Int1));
    for i in 0..10 {
        uniforms.push((format!("_TouchPos[{i}]"), U::Float2));
    }
    let material = load_material(
        vertex,
        &fragment,
        MaterialParams {
            uniforms,
            textures: textures.iter().map(|name| name.to_string()).collect(),
            pipeline_params: PipelineParams {
                color_blend: Some(miniquad::BlendState::new(
                    miniquad::Equation::Add,
                    miniquad::BlendFactor::One,
                    miniquad::BlendFactor::OneMinusValue(miniquad::BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
        },
    )
    .unwrap();
    for (name, value) in floats {
        material.set_uniform(name, *value);
    }
    for (name, value) in vectors {
        material.set_uniform(name, value);
    }
    material.set_uniform("_SparkTint", vec3(1., 0.28490567, 0.28490567));
    material.set_uniform("_TouchPosCount", touches.len() as i32);
    for i in 0..10 {
        material.set_uniform(&format!("_TouchPos[{i}]"), touches.get(i).map_or(vec2(0., 0.), |(_, p)| *p * vec2(16. / 9., 1.)));
    }
    let zero = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 0]);
    for name in textures {
        material.set_texture(name, zero);
    }
    let load = |bytes: &[u8], wrap| {
        let source = image::imageops::flip_vertical(&image::load_from_memory(bytes).unwrap().to_rgba8());
        let texture = Texture2D::from_rgba8(source.width() as u16, source.height() as u16, source.as_raw());
        texture.set_filter(FilterMode::Nearest);
        texture
            .raw_miniquad_texture_handle()
            .set_wrap(unsafe { get_internal_gl() }.quad_context, wrap);
        texture
    };
    let displace = load(include_bytes!("../../assets/blockarea/BlockNoise1.png"), TextureWrap::Mirror);
    let spark = load(include_bytes!("../../assets/blockarea/PointNoise.png"), TextureWrap::Repeat);
    material.set_texture("_DisplaceMap", displace);
    material.set_texture("_SparkMap", spark);
    let noise = load(include_bytes!("../../assets/blockarea/FD_Noise_00000.png"), TextureWrap::Mirror);
    // Accept original packed bytes for an independent asset audit. Without
    // them, round-trip the lossless bit-replicated PNG into native RGB565;
    // this checks shader arithmetic, not the original extraction process.
    let packed = if let Ok(path) = std::env::var("PHIRA_NOISE_RGB565") {
        std::fs::read(path).expect("PHIRA_NOISE_RGB565")
    } else {
        let image = image::load_from_memory(include_bytes!("../../assets/blockarea/FD_Noise_00000.png"))
            .unwrap()
            .to_rgb8();
        image::imageops::flip_vertical(&image)
            .pixels()
            .flat_map(|p| (((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3)).to_le_bytes())
            .collect::<Vec<_>>()
    };
    unsafe {
        use miniquad::gl::*;
        let mut bound = 0;
        glGetIntegerv(0x8069, &mut bound);
        glBindTexture(GL_TEXTURE_2D, noise.raw_miniquad_texture_handle().gl_internal_id());
        glTexImage2D(GL_TEXTURE_2D, 0, 0x8D62, 256, 256, 0, GL_RGB, 0x8363, packed.as_ptr() as _);
        glBindTexture(GL_TEXTURE_2D, bound as u32);
        assert_eq!(glGetError(), 0);
    }
    if time == 14. {
        let t = render_target(256, 256);
        let old = unsafe { get_internal_gl() }.quad_gl.get_active_render_pass();
        unsafe {
            use miniquad::gl::*;
            let mut bound = 0;
            glGetIntegerv(0x8069, &mut bound);
            glBindTexture(GL_TEXTURE_2D, t.texture.raw_miniquad_texture_handle().gl_internal_id());
            glTexImage2D(GL_TEXTURE_2D, 0, 0x8814, 256, 256, 0, GL_RGBA, GL_FLOAT, std::ptr::null());
            glBindTexture(GL_TEXTURE_2D, bound as u32);
        }
        let sample = load_material(
            "#version 100\nattribute vec3 position;uniform mat4 Model;uniform mat4 Projection;varying highp vec2 uv;void main(){gl_Position=Projection*Model*vec4(position,1.);uv=position.xy*vec2(0.5,16./18.)+0.5;}",
            "#version 100\nprecision highp float;varying highp vec2 uv;uniform mediump sampler2D t;void main(){gl_FragColor=texture2D(t,uv);}",
            MaterialParams { textures: vec!["t".into()], ..Default::default() }
        ).unwrap();
        sample.set_texture("t", noise);
        unsafe { get_internal_gl() }.quad_gl.render_pass(Some(t.render_pass));
        gl_use_material(sample);
        draw_rectangle(-1., -9. / 16., 2., 18. / 16., WHITE);
        gl_use_default_material();
        unsafe { get_internal_gl() }.flush();
        let mut values = vec![0_f32; 256 * 256 * 4];
        unsafe {
            use miniquad::gl::*;
            let mut f = 0;
            glGetIntegerv(0x8CAA, &mut f);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, t.render_pass.gl_internal_id(get_internal_gl().quad_context));
            glReadPixels(0, 0, 256, 256, GL_RGBA, GL_FLOAT, values.as_mut_ptr() as _);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, f as u32);
        }
        println!("Native RGB565 direct samples: {:?}", &values[..16]);
        unsafe { get_internal_gl() }.quad_gl.render_pass(old);
    }
    material.set_texture("_NoiseMap", noise);
    let mut masks = reference_mask::Masks::default();
    masks.render_displaced(960, 540, 16. / 9., zones, time);
    let mut hover = reference_touch::TouchMask::default();
    hover.update_fingers(touches, time - 0.101);
    hover.update_fingers(touches, time);
    for y in 0..masks.height {
        for x in 0..masks.width {
            let uv = vec2(((x / 2) as f32 + 0.5) / (masks.width / 2) as f32, ((y / 2) as f32 + 0.5) / (masks.height / 2) as f32);
            masks.aux_rgba[(y * masks.width + x) * 4 + 3] = hover.sample(uv, 16. / 9.);
        }
    }
    let aux = Texture2D::from_rgba8(masks.width as u16, masks.height as u16, &masks.aux_rgba);
    aux.set_filter(FilterMode::Nearest);
    material.set_texture("_Aux", aux);
    let compose: Vec<u8> = masks.rgba.chunks_exact(4).flat_map(|p| [p[0], 0, 0, 255]).collect();
    let effect: Vec<u8> = masks.rgba.chunks_exact(4).flat_map(|p| [p[1], p[2], 0, 255]).collect();
    // Emulate the /6 SceneColor camera's pixel-center samples against a full
    // copy, as production does without attaching a new framebuffer.
    let scene: Vec<u8> = input.map_or_else(|| [25, 25, 25, 255].repeat(960 * 540), |image| image.bytes.clone());
    let compose = Texture2D::from_rgba8(masks.width as u16, masks.height as u16, &compose);
    compose.set_filter(FilterMode::Nearest);
    let effect = Texture2D::from_rgba8(masks.width as u16, masks.height as u16, &effect);
    effect.set_filter(FilterMode::Linear);
    let scene = Texture2D::from_rgba8(960, 540, &scene);
    scene.set_filter(FilterMode::Linear);
    material.set_texture("_ComposeRT", compose);
    material.set_texture("_EffectRT", effect);
    material.set_texture("_SceneColor", scene);
    material.set_uniform("_EffectRT_TexelSize", vec4(1. / masks.width as f32, 1. / masks.height as f32, masks.width as f32, masks.height as f32));
    clear_background(Color::new(0.1, 0.1, 0.1, 1.));
    if input.is_some() {
        palette_scene();
    }
    gl_use_material(material);
    draw_rectangle(-1., -9. / 16., 2., 18. / 16., WHITE);
    gl_use_default_material();
    let reference = screen_pixels();
    reference.export_png("target/block-area-gpu/native-reference.png");
    let max_error = reference.bytes.iter().zip(&port.bytes).map(|(&a, &b)| a.abs_diff(b)).max().unwrap();
    let different = reference.bytes.iter().zip(&port.bytes).filter(|(a, b)| a != b).count();
    println!("Exported ActiveBlock GLSL comparison at {time}s: max channel error {max_error}, {different} differing channels");
    assert_eq!(max_error, 0, "material arithmetic must match the exported fragment pixel for pixel");
}
