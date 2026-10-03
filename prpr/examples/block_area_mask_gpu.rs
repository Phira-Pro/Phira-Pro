//! Independent mask oracle: executes exported native shaders in test-only FBOs.
//! Production uses CPU masks and never changes the chart's framebuffer.
use macroquad::prelude::*;
use miniquad::{TextureWrap, UniformType as U};
use prpr::core::{Vector, Zone};
#[path = "../src/core/block_mask.rs"]
mod mask;

fn conf() -> Conf {
    Conf {
        window_width: 960,
        window_height: 540,
        headless: true,
        window_title: "Native mask oracle".into(),
        ..Default::default()
    }
}
const VERT: &str = "#version 100\nattribute vec3 position; uniform vec4 spriteTint; uniform mat4 Projection; uniform mat4 Model; varying highp vec2 vs_TEXCOORD0; varying highp vec2 vs_TEXCOORD1; varying highp vec4 vs_COLOR0; void main(){ gl_Position=Projection*Model*vec4(position,1.0); vs_TEXCOORD0=position.xy*0.5+0.5; vs_TEXCOORD1=vs_TEXCOORD0*vec2(2.13,1.02); vs_COLOR0=spriteTint; }";
fn material(name: &str, pass: usize, kind: &str, uniforms: Vec<(String, U)>, textures: &[&str], blend: Option<miniquad::BlendState>) -> Material {
    let src = std::fs::read_to_string(format!("../_official_src/shader_code/Unlit_{name}.p0.txt"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = src.split("#ifdef FRAGMENT").nth(pass + 1).unwrap();
    let main_start = section.find("void main()").unwrap();
    let end = main_start + section[main_start..].find("\n}").unwrap() + 2;
    let frag = &section[..end];
    let out = match kind {
        "uv" | "terms" => "vec4(nativeResult,0.0,0.0,1.0)",
        "float" => "vec4(nativeResult,0.0,0.0,1.0)",
        "vec2" => "vec4(nativeResult,0.0,1.0)",
        _ => "nativeResult",
    };
    let declaration = if kind == "uv" || kind == "terms" { "float" } else { kind };
    let frag = frag
        .replace("#version 300 es", "#version 100")
        .replace("#define UNITY_LOCATION(x) layout(location = x)", "#define UNITY_LOCATION(x)")
        .replace("in highp", "varying highp")
        .replace(&format!("layout(location = 0) out mediump {declaration} SV_Target0;"), &format!("mediump {declaration} nativeResult;"))
        .replace("SV_Target0", "nativeResult")
        .replace("texture(", "texture2D(")
        .replace("_Time", "uTime")
        .replace("    return;", &format!("    gl_FragColor={out}; return;"));
    let frag = if kind == "uv" {
        frag.replace("nativeResult = abs(u_xlat16_0.x);", "gl_FragColor=vec4(u_xlat1.xy,vs_TEXCOORD1.xy); return;")
    } else if kind == "terms" {
        frag.replace("u_xlat16_5 = texture2D(_NormalBlockRT, u_xlat1.xy).x;", "gl_FragColor=vec4(u_xlat16_0.xy,u_xlat16_4,u_xlat16_6); return;")
    } else {
        frag
    };
    load_material(
        VERT,
        &frag,
        MaterialParams {
            uniforms,
            textures: textures.iter().map(|x| x.to_string()).collect(),
            pipeline_params: PipelineParams {
                color_blend: blend,
                ..Default::default()
            },
        },
    )
    .unwrap()
}
fn target(w: usize, h: usize) -> RenderTarget {
    let t = render_target(w as u32, h as u32);
    t.texture.set_filter(FilterMode::Nearest);
    // Native masks use R8/RG8, whose blend precision may differ from RGBA8.
    unsafe {
        use miniquad::gl::*;
        let mut bound = 0;
        glGetIntegerv(0x8069, &mut bound);
        glBindTexture(GL_TEXTURE_2D, t.texture.raw_miniquad_texture_handle().gl_internal_id());
        glTexImage2D(GL_TEXTURE_2D, 0, 0x822B, w as i32, h as i32, 0, 0x8227, GL_UNSIGNED_BYTE, std::ptr::null());
        glBindTexture(GL_TEXTURE_2D, bound as u32);
    }
    t
}
fn enter(t: RenderTarget) {
    set_camera(&Camera2D {
        zoom: vec2(1., 1.),
        render_target: Some(t),
        ..Default::default()
    });
    clear_background(BLACK);
}
fn blit(t: RenderTarget, m: Material) {
    enter(t);
    gl_use_material(m);
    draw_rectangle(-1., -1., 2., 2., WHITE);
    gl_use_default_material();
    unsafe { get_internal_gl() }.flush();
}
fn bytes(t: RenderTarget) -> Vec<u8> {
    unsafe { get_internal_gl() }.flush();
    let mut out = vec![0; (t.texture.width() * t.texture.height() * 4.) as usize];
    unsafe {
        use miniquad::gl::*;
        let mut f = 0;
        glGetIntegerv(0x8CAA, &mut f);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, t.render_pass.gl_internal_id(get_internal_gl().quad_context));
        glReadPixels(0, 0, t.texture.width() as i32, t.texture.height() as i32, GL_RGBA, GL_UNSIGNED_BYTE, out.as_mut_ptr() as _);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, f as u32);
        assert_eq!(glGetError(), 0);
    }
    out
}
fn compare(a: impl Iterator<Item = u8>, b: impl Iterator<Item = u8>, label: &str) {
    let diffs: Vec<_> = a.zip(b).enumerate().filter(|(_, (a, b))| a != b).collect();
    let max = diffs.iter().map(|(_, (a, b))| a.abs_diff(*b)).max().unwrap_or(0);
    println!("{label}: {} differing channels, max {max}, first {:?}", diffs.len(), diffs.first());
    // Native R8 blending can quantize each .1 contribution to 25 or 26 across
    // drivers; its threshold output is checked separately. Float interpolation
    // can also select opposite sides of a point-sample boundary. Report every
    // difference instead of calling this bounded audit pixel-exact.
    if label.contains("source 1") || label.contains("source 3") {
        assert!(max <= 3, "source blend quantization: {label}");
    } else {
        let bound = if label.contains("disabled compose") || label.contains("source") {
            0
        } else if label.contains("compose") {
            4
        } else if label.contains("edge") {
            24
        } else {
            80
        };
        assert!(diffs.len() <= bound, "native shader vs CPU: {label}");
    }
}
fn zone(x: f32, y: f32, inv: bool, active: bool) -> Zone {
    Zone {
        center: Vector::new(x, y),
        half: Vector::new(0.29, 0.15),
        angle: 0.37,
        invert: inv,
        active,
        ready: !active,
        opacity: 1.,
    }
}

#[macroquad::main(conf)]
async fn main() {
    next_frame().await;
    let aspect = 16. / 9.;
    let (bw, bh) = (120, 67);
    let (ew, eh) = (bw * 2, bh * 2);
    let additive = miniquad::BlendState::new(
        miniquad::Equation::Add,
        miniquad::BlendFactor::Value(miniquad::BlendValue::SourceAlpha),
        miniquad::BlendFactor::One,
    );
    let sprite = material("BlockSprite", 0, "vec4", vec![("spriteTint".into(), U::Float4)], &["_MainTex"], Some(additive));
    let solid = Texture2D::from_rgba8(1, 1, &[255, 0, 0, 255]);
    sprite.set_texture("_MainTex", solid);
    let clamp = material(
        "SubtractBlockBlender",
        0,
        "float",
        vec![("_ClampThresholdLow".into(), U::Float1), ("_ClampThresholdHigh".into(), U::Float1)],
        &["_MainTex"],
        None,
    );
    let disabled = material(
        "SubtractBlockBlender",
        1,
        "vec2",
        vec![("_ClampThresholdLow".into(), U::Float1), ("_ClampThresholdHigh".into(), U::Float1)],
        &["_MainTex"],
        None,
    );
    for m in [clamp, disabled] {
        m.set_uniform("_ClampThresholdLow", 0.09_f32);
        m.set_uniform("_ClampThresholdHigh", 0.12_f32);
    }
    let compose = material(
        "BlockCompose",
        0,
        "float",
        vec![
            ("uTime".into(), U::Float4),
            ("_DisplaceDirection".into(), U::Float4),
            ("_DisplaceSpeed".into(), U::Float1),
            ("_DisplaceStrength".into(), U::Float1),
        ],
        &["_NormalBlockRT", "_SubtractBlockRT", "_DisplaceMap"],
        None,
    );
    let source = image::imageops::flip_vertical(
        &image::load_from_memory(include_bytes!("../../assets/blockarea/BlockNoise1.png"))
            .unwrap()
            .to_rgba8(),
    );
    let noise = Texture2D::from_rgba8(source.width() as u16, source.height() as u16, source.as_raw());
    noise.set_filter(FilterMode::Nearest);
    noise
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, TextureWrap::Mirror);
    compose.set_texture("_DisplaceMap", noise);
    compose.set_uniform("_DisplaceDirection", vec4(0.5, 0.5, 0., 0.));
    compose.set_uniform("_DisplaceSpeed", 2.59_f32);
    compose.set_uniform("_DisplaceStrength", 0.1_f32);
    let uv_material = material(
        "BlockCompose",
        0,
        "uv",
        vec![
            ("uTime".into(), U::Float4),
            ("_DisplaceDirection".into(), U::Float4),
            ("_DisplaceSpeed".into(), U::Float1),
            ("_DisplaceStrength".into(), U::Float1),
        ],
        &["_NormalBlockRT", "_SubtractBlockRT", "_DisplaceMap"],
        None,
    );
    uv_material.set_texture("_DisplaceMap", noise);
    uv_material.set_uniform("_DisplaceDirection", vec4(0.5, 0.5, 0., 0.));
    uv_material.set_uniform("_DisplaceSpeed", 2.59_f32);
    uv_material.set_uniform("_DisplaceStrength", 0.1_f32);
    let terms_material = material(
        "BlockCompose",
        0,
        "terms",
        vec![
            ("uTime".into(), U::Float4),
            ("_DisplaceDirection".into(), U::Float4),
            ("_DisplaceSpeed".into(), U::Float1),
            ("_DisplaceStrength".into(), U::Float1),
        ],
        &["_NormalBlockRT", "_SubtractBlockRT", "_DisplaceMap"],
        None,
    );
    terms_material.set_texture("_DisplaceMap", noise);
    terms_material.set_uniform("_DisplaceDirection", vec4(0.5, 0.5, 0., 0.));
    terms_material.set_uniform("_DisplaceSpeed", 2.59_f32);
    terms_material.set_uniform("_DisplaceStrength", 0.1_f32);
    let uv_target = render_target(bw as u32, bh as u32);
    unsafe {
        use miniquad::gl::*;
        let mut bound = 0;
        glGetIntegerv(0x8069, &mut bound);
        glBindTexture(GL_TEXTURE_2D, uv_target.texture.raw_miniquad_texture_handle().gl_internal_id());
        glTexImage2D(GL_TEXTURE_2D, 0, 0x8814, bw as i32, bh as i32, 0, GL_RGBA, GL_FLOAT, std::ptr::null());
        glBindTexture(GL_TEXTURE_2D, bound as u32);
    }
    let dcompose = material("BlockCompose", 1, "vec2", vec![], &["_DisabledNormalBlockRT", "_DisabledSubtractBlockRT"], None);
    let edge = material("EdgeMask", 1, "vec2", vec![("_DilateTexelSize".into(), U::Float4)], &["_MainTex", "_ComposeRT"], None);
    let glow = material(
        "GlowMask",
        0,
        "vec2",
        vec![
            ("_DilateTexelSize".into(), U::Float4),
            ("_PassWeight".into(), U::Float1),
            ("_GlowFirstPass".into(), U::Float1),
        ],
        &["_MainTex", "_ComposeRT"],
        None,
    );
    for m in [edge, glow] {
        m.set_uniform("_DilateTexelSize", vec4(1. / ew as f32, 1. / eh as f32, ew as f32, eh as f32));
    }
    let mut cases = vec![
        vec![zone(0.11, -0.09, false, true)],
        vec![zone(0., 0., true, true); 3],
        vec![zone(-0.05, 0.1, false, true), zone(0.14, 0.03, true, false)],
    ];
    let mut partial = zone(0.05, 0.0, false, true);
    partial.opacity = 0.23;
    cases.push(vec![partial]);
    let mut initial = zone(0., 0., true, false);
    initial.opacity = 0.25;
    cases.push(vec![initial]);
    for (ci, zones) in cases.iter().enumerate() {
        for time in [1., 9.] {
            let mut cpu = mask::Masks::default();
            cpu.render_displaced(960, 540, aspect, zones, time);
            let layers: Vec<_> = (0..4).map(|_| target(bw, bh)).collect();
            for (li, &t) in layers.iter().enumerate() {
                enter(t);
                gl_use_material(sprite);
                for z in zones.iter().filter(|z| usize::from(!z.active) * 2 + usize::from(z.invert) == li) {
                    let (s, c) = z.angle.sin_cos();
                    let m = Mat4::from_cols(
                        vec4(c, s * aspect, 0., 0.),
                        vec4(-s, c * aspect, 0., 0.),
                        vec4(0., 0., 1., 0.),
                        vec4(z.center.x, z.center.y * aspect, 0., 1.),
                    );
                    unsafe { get_internal_gl() }.quad_gl.push_model_matrix(m);
                    sprite.set_uniform("spriteTint", vec4(1., if z.invert { z.opacity } else { 1. }, 1., if z.invert { 0.1 } else { z.opacity }));
                    draw_rectangle(-z.half.x, -z.half.y, z.half.x * 2., z.half.y * 2., WHITE);
                    unsafe { get_internal_gl() }.flush();
                    unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
                }
                gl_use_default_material();
                compare(bytes(t).into_iter().step_by(4), cpu.sources_rgba.iter().skip(li).step_by(4).copied(), &format!("case {ci} source {li}"));
            }
            let sub = target(bw, bh);
            clamp.set_texture("_MainTex", layers[1].texture);
            blit(sub, clamp);
            let dsub = target(bw, bh);
            disabled.set_texture("_MainTex", layers[3].texture);
            blit(dsub, disabled);
            let cm = target(bw, bh);
            compose.set_texture("_NormalBlockRT", layers[0].texture);
            compose.set_texture("_SubtractBlockRT", sub.texture);
            compose.set_uniform("uTime", vec4(time / 20., time, time * 2., time * 3.));
            blit(cm, compose);
            if ci == 0 {
                uv_material.set_texture("_NormalBlockRT", layers[0].texture);
                uv_material.set_texture("_SubtractBlockRT", sub.texture);
                uv_material.set_uniform("uTime", vec4(time / 20., time, time * 2., time * 3.));
                blit(uv_target, uv_material);
                let mut coordinates = vec![0_f32; bw * bh * 4];
                unsafe {
                    use miniquad::gl::*;
                    let mut f = 0;
                    glGetIntegerv(0x8CAA, &mut f);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, uv_target.render_pass.gl_internal_id(get_internal_gl().quad_context));
                    glReadPixels(0, 0, bw as i32, bh as i32, GL_RGBA, GL_FLOAT, coordinates.as_mut_ptr() as _);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, f as u32);
                    assert_eq!(glGetError(), 0);
                }
                let mut diffs = 0;
                for y in 0..bh {
                    for x in 0..bw {
                        let i = (y * bw + x) * 4;
                        let uv = [(x as f32 + 0.5) / bw as f32, (y as f32 + 0.5) / bh as f32];
                        let actual = mask::compose_uv(uv, time);
                        let gpu = [coordinates[i], coordinates[i + 1]];
                        if (actual[0] * bw as f32).floor() != (gpu[0] * bw as f32).floor()
                            || (actual[1] * bh as f32).floor() != (gpu[1] * bh as f32).floor()
                        {
                            if diffs < 8 {
                                println!("UV {time}s ({x},{y}) cpu {actual:?} gpu {gpu:?} ST {:?}", &coordinates[i + 2..i + 4]);
                            }
                            diffs += 1;
                        }
                    }
                }
                println!("UV point-sample discrepancies at {time}s: {diffs}");
                terms_material.set_uniform("uTime", vec4(time / 20., time, time * 2., time * 3.));
                blit(uv_target, terms_material);
                unsafe {
                    use miniquad::gl::*;
                    let mut f = 0;
                    glGetIntegerv(0x8CAA, &mut f);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, uv_target.render_pass.gl_internal_id(get_internal_gl().quad_context));
                    glReadPixels(0, 0, bw as i32, bh as i32, GL_RGBA, GL_FLOAT, coordinates.as_mut_ptr() as _);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, f as u32);
                }
                for (x, y) in [(111, 0), (52, 1), (52, 2), (72, 2)] {
                    let i = (y * bw + x) * 4;
                    println!("native terms {time}s ({x},{y}): {:?}", &coordinates[i..i + 4]);
                }
            }
            let dc = target(bw, bh);
            dcompose.set_texture("_DisabledNormalBlockRT", layers[2].texture);
            dcompose.set_texture("_DisabledSubtractBlockRT", dsub.texture);
            blit(dc, dcompose);
            let reference: Vec<_> = (0..bh).flat_map(|y| (0..bw).map(move |x| (y * 2 * ew + x * 2) * 4)).collect();
            compare(bytes(cm).into_iter().step_by(4), reference.iter().map(|&i| cpu.rgba[i]), &format!("case {ci} compose {time}s"));
            compare(bytes(dc).into_iter().step_by(4), reference.iter().map(|&i| cpu.rgba[i + 3]), &format!("case {ci} disabled compose"));
            let er = target(ew, eh);
            edge.set_texture("_MainTex", cm.texture);
            edge.set_texture("_ComposeRT", cm.texture);
            blit(er, edge);
            compare(bytes(er).into_iter().step_by(4), cpu.rgba.iter().skip(1).step_by(4).copied(), &format!("case {ci} edge"));
            let (p, q) = (target(ew, eh), target(ew, eh));
            let mut source = cm.texture;
            let sum: f32 = (1..=6).map(|n| (n as f32).powf(2.65)).sum();
            for pass in 0..5 {
                glow.set_uniform("_GlowFirstPass", if pass == 0 { 1_f32 } else { 0_f32 });
                glow.set_uniform("_PassWeight", ((6 - pass) as f32).powf(2.65) / sum);
                glow.set_texture("_MainTex", source);
                glow.set_texture("_ComposeRT", cm.texture);
                let dst = if pass % 2 == 0 { p } else { q };
                blit(dst, glow);
                source = dst.texture;
            }
            compare(bytes(p).into_iter().skip(1).step_by(4), cpu.rgba.iter().skip(2).step_by(4).copied(), &format!("case {ci} glow"));
        }
    }
    println!("Independent native mask audit completed within reported bounds; it is NOT pixel-exact.");
}
