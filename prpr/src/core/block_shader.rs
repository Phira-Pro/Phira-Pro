//! Rendering for official block-area ("noise field") zones.
//!
//! Visual masks use the native SubtractBlockBlender threshold window; this is
//! intentionally separate from the already verified input parity semantics.
//!
//! Masks retain the native effect RT dimensions, with compose pixels refined to
//! that grid instead of replicating each native 1/8-resolution pixel four times.
//! They are then dilated as in
//! `EdgeMask` / `GlowMask`. They are uploaded as an ordinary texture: no camera
//! changes or additional framebuffer/render passes are needed. The scene color
//! is copied from the existing chart pass (resolving its MSAA target first).
//! Material arithmetic and texture settings come from the exported GLSL/.mat.

use super::{BlockArea, BlockPhase, Resource, Vector};
use macroquad::prelude::*;
use miniquad::{TextureWrap, UniformType};
use once_cell::sync::Lazy;
use std::cell::RefCell;

#[path = "block_mask.rs"]
mod mask;
#[path = "block_touch.rs"]
mod touch;
#[path = "block_simple.rs"]
mod simple;

/// A resolved rectangle (axis-aligned in its own space).
#[derive(Clone, PartialEq)]
pub struct Zone {
    pub center: Vector,
    pub half: Vector,
    pub angle: f32,
    pub invert: bool,
    pub active: bool,
    /// Ready layer is selected in the last 0.5 seconds before enableTime.
    pub ready: bool,
    /// Initial DisabledBlockShow fades the sprite mask in over 0.5 seconds.
    pub opacity: f32,
}

impl Zone {
    pub fn from_area(area: &BlockArea, time: f64, aspect: f32) -> Option<Self> {
        let phase = area.phase(time);
        if phase == BlockPhase::Hidden {
            return None;
        }
        let tr = area.transform(time, aspect);
        let half = tr.size.map(|v| v.abs() * 0.5);
        if half.x == 0. || half.y == 0. {
            return None;
        }
        let active = phase == BlockPhase::Active;
        let fades_in = !area.is_active(area.appear_time);
        Some(Self {
            center: tr.center,
            half,
            angle: tr.rotation.to_radians(),
            invert: area.is_subtract,
            active,
            ready: !active && time < area.enable_time && time >= area.enable_time - 0.5,
            opacity: if !active && fades_in {
                ((time - area.appear_time) / 0.5).clamp(0., 1.) as f32
            } else {
                1.
            },
        })
    }
}

fn tex(bytes: &[u8], wrap: TextureWrap) -> Texture2D {
    // Exported PNG rows are top-down; Unity GLES samples these texture assets
    // with (0,0) at their bottom-left. Macroquad uploads image bytes unchanged.
    let source = image::load_from_memory(bytes).unwrap().to_rgba8();
    let source = image::imageops::flip_vertical(&source);
    let texture = Texture2D::from_rgba8(source.width() as u16, source.height() as u16, source.as_raw());
    texture.set_filter(FilterMode::Nearest);
    texture
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, wrap);
    texture
}

static DISPLACE_TEX: Lazy<Texture2D> = Lazy::new(|| tex(include_bytes!("../../../assets/blockarea/BlockNoise1.png"), TextureWrap::Mirror));
static SPARK_TEX: Lazy<Texture2D> = Lazy::new(|| tex(include_bytes!("../../../assets/blockarea/PointNoise.png"), TextureWrap::Repeat));
// This PNG is regenerated from original RGB565 with full 5/6-bit replication.
// The initial exporter only shifted bits, which darkened the hover dissolve.
static NOISE_TEX: Lazy<Texture2D> = Lazy::new(|| tex(include_bytes!("../../../assets/blockarea/FD_Noise_00000.png"), TextureWrap::Mirror));
static EMPTY_TEX: Lazy<Texture2D> = Lazy::new(|| Texture2D::from_rgba8(1, 1, &[0; 4]));

// Typed f32 values are essential: this macroquad fork rejects f64 for Float1.
// Keep these uniforms, rather than folding their values into GLSL, so that the
// native mediump arithmetic and GPU optimization stay identical to the export.
const FLOATS: &[(&str, f32)] = &[
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
    ("_TouchPosShine", 0.),
    ("_TouchPosRadius", 0.5),
    ("_TouchPosSDFSmoothness", 0.47),
    ("_TouchPosSDFFalloff", 0.41),
    ("_BackgroundPixelScale", 6.),
    ("uDisabledFillOpacity", 0.4),
    ("uDisabledSparkOpacity", 3.5),
    ("uDisabledSparkIntensity", 2.29),
    ("uDisabledSpeed", 0.3),
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
const COLORS: &[(&str, [f32; 4])] = &[
    ("_EdgeColor", [1., 0.33018857, 0.33018857, 1.]),
    ("_FillColor", [0.7132075, 0.23549296, 0.23549296, 1.]),
    ("_GlowColor", [1., 0.17924517, 0.17924517, 1.]),
    ("_DisplaceDirection", [1., 1., 0., 0.]),
    ("uDisabledFillColor", [0.497, 0.13766898, 0.13766898, 1.]),
    ("_ShineColor", [1., 1., 1., 1.]),
    ("_TouchDisplaceDirection", [1., 1., 0., 0.]),
    ("_NoiseTint", [1., 0., 0., 1.]),
    ("_TouchGlowColor", [1., 0., 0., 1.]),
];

#[derive(Default)]
struct FrameTextures {
    uploaded_masks: Option<u64>,
    masks: mask::Masks,
    effect: Option<Texture2D>,
    aux: Option<Texture2D>,
    scene: Option<Texture2D>,
    touch: touch::TouchMask,
    clock: Option<f32>,
}

thread_local! {
    static FRAME: RefCell<FrameTextures> = RefCell::new(FrameTextures::default());
}

fn load_block_material(disabled: bool, hover: bool) -> Result<Material, miniquad::ShaderError> {
    let mut uniforms = vec![
        ("uView".to_owned(), UniformType::Float3),
        ("uUnityTime".to_owned(), UniformType::Float4),
        ("_ScreenParams".to_owned(), UniformType::Float4),
        ("_EffectRT_TexelSize".to_owned(), UniformType::Float4),
        ("_SparkTint".to_owned(), UniformType::Float3),
        ("uDisabledSparkTint".to_owned(), UniformType::Float3),
        ("_TouchPosCount".to_owned(), UniformType::Int1),
        ("uLayer".to_owned(), UniformType::Int1),
    ];
    uniforms.extend(FLOATS.iter().map(|(name, _)| (name.to_string(), UniformType::Float1)));
    uniforms.extend(COLORS.iter().map(|(name, _)| (name.to_string(), UniformType::Float4)));
    for i in 0..10 {
        uniforms.push((format!("_TouchPos[{i}]"), UniformType::Float2));
    }
    let params = MaterialParams {
        pipeline_params: PipelineParams {
            color_blend: Some(miniquad::BlendState::new(
                miniquad::Equation::Add,
                miniquad::BlendFactor::One,
                if disabled {
                    miniquad::BlendFactor::One
                } else {
                    miniquad::BlendFactor::OneMinusValue(miniquad::BlendValue::SourceAlpha)
                },
            )),
            ..Default::default()
        },
        uniforms,
        textures: vec![
            "uDisplaceTex".to_owned(),
            "uSparkTex".to_owned(),
            "uMasks".to_owned(),
            "uScene".to_owned(),
            "uAuxMasks".to_owned(),
            "uNoiseTex".to_owned(),
        ],
    };
    // Specialize uniform-only branches. On mobile GPUs the native hover SDF
    // otherwise consumes registers/instructions even with zero fingers.
    let mut fragment = FRAGMENT.replace("uniform int uLayer;", if disabled { "const int uLayer = 0;" } else { "const int uLayer = 3;" });
    if !hover {
        fragment = fragment
            .replace("uniform \tint _TouchPosCount;", "const int _TouchPosCount = 0;")
            .replace("float hoverSample(vec2 uv) { return texture2D(uAuxMasks, basePixelUV(uv)).a; }", "float hoverSample(vec2 uv) { return 0.0; }");
    }
    load_material(VERTEX, &fragment, params).map(|material| {
        for (name, value) in FLOATS {
            material.set_uniform(name, *value);
        }
        for (name, value) in COLORS {
            material.set_uniform(name, *value);
        }
        material.set_uniform("_SparkTint", vec3(1., 0.28490567, 0.28490567));
        material.set_uniform("uDisabledSparkTint", vec3(0.31132078, 0.077830195, 0.077830195));
        material.set_uniform("_TouchPosCount", 0_i32);
        material.set_uniform("uLayer", if disabled { 0_i32 } else { 3_i32 });
        for i in 0..10 {
            material.set_uniform(&format!("_TouchPos[{i}]"), vec2(0., 0.));
        }
        material
    })
}

static MATERIAL: Lazy<Option<[Material; 3]>> = Lazy::new(|| {
    (|| {
        Ok([
            load_block_material(true, false)?,
            load_block_material(false, false)?,
            load_block_material(false, true)?,
        ])
    })()
    .map_err(|e: miniquad::ShaderError| {
        tracing::warn!("block-area shader failed: {e}");
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::write(exe.with_file_name("block_shader_error.txt"), format!("{e}"));
        }
    })
    .ok()
});

pub(crate) fn prepare_block_effects() {
    // Link shaders and decode their textures during chart loading, before a
    // late first block would stall a live judgement frame.
    Lazy::force(&MATERIAL);
    Lazy::force(&DISPLACE_TEX);
    Lazy::force(&SPARK_TEX);
    Lazy::force(&NOISE_TEX);
    // Some GLES drivers defer shader compilation until the first draw. Use the
    // current loading pass, with empty masks which discard every fragment.
    // No framebuffer/camera switch or visible loading-screen output is needed.
    if let Some(materials) = MATERIAL.as_ref() {
        let empty = *EMPTY_TEX;
        for material in materials {
            material.set_texture("uDisplaceTex", *DISPLACE_TEX);
            material.set_texture("uSparkTex", *SPARK_TEX);
            material.set_texture("uNoiseTex", *NOISE_TEX);
            for sampler in ["uMasks", "uAuxMasks", "uScene"] {
                material.set_texture(sampler, empty);
            }
            material.set_uniform("uView", vec3(1., 1., 1.));
            material.set_uniform("uUnityTime", vec4(0., 0., 0., 0.));
            material.set_uniform("_ScreenParams", vec4(1., 1., 2., 2.));
            material.set_uniform("_EffectRT_TexelSize", vec4(1., 1., 1., 1.));
            material.set_uniform("_TouchPosCount", 0_i32);
            gl_use_material(*material);
            draw_rectangle(0., 0., 0.001, 0.001, WHITE);
        }
        gl_use_default_material();
        unsafe { get_internal_gl() }.flush();
    }
}

/// Draw the visible zones for the current frame.
pub fn draw_zones(res: &mut Resource, aspect: f32, zones: &[Zone]) {
    draw_layer_at(res, aspect, zones, get_time() as f32, false, &[]);
}

pub fn draw_disabled_zones(res: &mut Resource, aspect: f32, zones: &[Zone]) {
    // Unity's _Time is constant for all passes in a frame. Chart invokes this
    // before notes even when the disabled layer happens to be empty.
    let time = get_time() as f32;
    FRAME.with(|frame| frame.borrow_mut().clock = Some(time));
    draw_layer_at(res, aspect, zones, time, true, &[]);
}

/// Touch centers are chart coordinates; IDs preserve native slot lifetimes.
pub fn draw_zones_with_touches(res: &mut Resource, aspect: f32, zones: &[Zone], touches: &[(u64, Vector)], flip_x: bool) {
    if res.config.block_area_simple {
        simple::draw(aspect, zones, false);
        return;
    }
    let projection = unsafe { get_internal_gl() }.quad_gl.get_projection_matrix();
    let mut touches: Vec<_> = touches
        .iter()
        .map(|&(id, p)| {
            let p = projection * vec4(if flip_x { -p.x } else { p.x }, -p.y, 0., 1.);
            (id, vec2(p.x / p.w, p.y / p.w) * 0.5 + vec2(0.5, 0.5))
        })
        .collect();
    // Judgement collects touches from a HashMap. Keep SDF accumulation order
    // stable when another finger is added or removed.
    touches.sort_by_key(|(id, _)| *id);
    let time = FRAME.with(|frame| frame.borrow().clock).unwrap_or_else(|| get_time() as f32);
    draw_layer_at(res, aspect, zones, time, false, &touches);
}

pub(crate) fn reset_block_effects() {
    FRAME.with(|frame| {
        let mut frame = frame.borrow_mut();
        frame.touch.reset();
        frame.clock = None;
    });
}

// Shared with the native GPU probe, which supplies a fixed Unity _Time.y.
#[allow(dead_code)] // Also compiled directly by the hidden GPU regression example.
pub(crate) fn draw_zones_at(res: &mut Resource, aspect: f32, zones: &[Zone], time: f32) {
    draw_layer_at(res, aspect, zones, time, true, &[]);
    draw_layer_at(res, aspect, zones, time, false, &[]);
}

pub(crate) fn draw_layer_at(res: &mut Resource, aspect: f32, zones: &[Zone], time: f32, disabled: bool, touches: &[(u64, Vec2)]) {
    if res.config.block_area_simple {
        simple::draw(aspect, zones, disabled);
        return;
    }
    let needed = if disabled {
        zones.iter().any(|z| !z.active && z.opacity > 0.)
    } else {
        zones.iter().any(|z| z.active || z.ready) || !touches.is_empty() || FRAME.with(|frame| frame.borrow().touch.visible())
    };
    if !needed {
        return;
    }
    let Some(m) = MATERIAL.as_ref() else {
        simple::draw(aspect, zones, disabled);
        return;
    };
    // ActiveBlock is postprocessing after sprites, notes and HUD; Disabled is
    // the Background layer before judge lines. Restore the existing pass state.
    let mut gl = unsafe { get_internal_gl() };
    let pass = gl.quad_gl.get_active_render_pass();
    let (width, height) = if let Some(pass) = pass {
        let texture = pass.texture(gl.quad_context);
        (texture.width as f32, texture.height as f32)
    } else {
        gl.quad_context.screen_size()
    };
    let viewport = gl.quad_gl.get_viewport().unwrap_or((0, 0, width as i32, height as i32));
    gl.flush();
    FRAME.with(|frame| {
        let mut frame = frame.borrow_mut();
        let (width, height) = (viewport.2.max(1) as usize, viewport.3.max(1) as usize);
        frame.masks.render_displaced(width, height, aspect, zones, time);
        if !disabled {
            frame.touch.update_fingers(touches, time);
        }
        let hover = !disabled && frame.touch.visible();
        let m = m[if disabled {
            0
        } else if hover {
            2
        } else {
            1
        }];
        if hover {
            let bw = frame.masks.width / 2;
            let bh = frame.masks.height / 2;
            for y in 0..bh {
                for x in 0..bw {
                    let value = frame
                        .touch
                        .sample(vec2((x as f32 + 0.5) / bw as f32, (y as f32 + 0.5) / bh as f32), width as f32 / height as f32);
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let i = ((y * 2 + dy) * frame.masks.width + x * 2 + dx) * 4 + 3;
                            frame.masks.aux_rgba[i] = value;
                        }
                    }
                }
            }
        }
        let dim = (width as u32, height as u32);
        let effect_dim = (frame.masks.width as u32, frame.masks.height as u32);
        let resized = frame
            .effect
            .is_none_or(|texture| (texture.width() as u32, texture.height() as u32) != effect_dim);
        if resized {
            // Ordinary sampled textures, never attached to a new framebuffer.
            // Allocate before deleting: GL may reuse a deleted name while the
            // miniquad sampler cache still remembers its old binding.
            let texture = Texture2D::from_rgba8(effect_dim.0 as u16, effect_dim.1 as u16, &frame.masks.rgba);
            texture.set_filter(FilterMode::Linear);
            if let Some(old) = frame.effect.replace(texture) {
                old.delete();
            }
        } else if frame.uploaded_masks != Some(frame.masks.revision) {
            frame
                .effect
                .unwrap()
                .raw_miniquad_texture_handle()
                .update(unsafe { get_internal_gl() }.quad_context, &frame.masks.rgba);
        }
        let aux = frame.aux;
        let resized_aux = aux.is_none_or(|t| (t.width() as u32, t.height() as u32) != effect_dim);
        if resized_aux {
            let texture = Texture2D::from_rgba8(effect_dim.0 as u16, effect_dim.1 as u16, &frame.masks.aux_rgba);
            texture.set_filter(FilterMode::Linear);
            if let Some(old) = frame.aux.replace(texture) {
                old.delete();
            }
        } else if hover || frame.uploaded_masks != Some(frame.masks.revision) {
            frame
                .aux
                .unwrap()
                .raw_miniquad_texture_handle()
                .update(unsafe { get_internal_gl() }.quad_context, &frame.masks.aux_rgba);
        }
        frame.uploaded_masks = Some(frame.masks.revision);
        if frame.scene.is_none_or(|texture| (texture.width() as u32, texture.height() as u32) != dim) {
            let texture = miniquad::Texture::new(
                unsafe { get_internal_gl() }.quad_context,
                miniquad::TextureAccess::Static,
                None,
                miniquad::TextureParams {
                    width: dim.0,
                    height: dim.1,
                    // Native SceneColor downsamples the camera with a linear
                    // blit, then point-samples that smaller RT. Sample its
                    // quantized texel centers from this full-sized copy.
                    filter: miniquad::FilterMode::Linear,
                    ..Default::default()
                },
            );
            if let Some(old) = frame.scene.replace(Texture2D::from_miniquad_texture(texture)) {
                old.delete();
            }
        }
        let scene = frame.scene.unwrap();
        if !disabled {
            copy_scene(res, pass, viewport, scene);
        }
        m.set_texture("uDisplaceTex", *DISPLACE_TEX);
        m.set_texture("uSparkTex", *SPARK_TEX);
        m.set_texture("uMasks", frame.effect.unwrap());
        m.set_texture("uScene", scene);
        m.set_texture("uAuxMasks", frame.aux.unwrap());
        m.set_texture("uNoiseTex", *NOISE_TEX);
        m.set_uniform("uUnityTime", vec4(time / 20., time, time * 2., time * 3.));
        m.set_uniform("uView", vec3(width as f32, height as f32, aspect));
        m.set_uniform("_ScreenParams", vec4(width as f32, height as f32, 1. + 1. / width as f32, 1. + 1. / height as f32));
        m.set_uniform("_EffectRT_TexelSize", vec4(1. / effect_dim.0 as f32, 1. / effect_dim.1 as f32, effect_dim.0 as f32, effect_dim.1 as f32));
        m.set_uniform("_TouchPosCount", touches.len().min(10) as i32);
        for (i, (_, uv)) in touches.iter().take(10).enumerate() {
            m.set_uniform(&format!("_TouchPos[{i}]"), *uv * vec2(width as f32 / height as f32, 1.));
        }
        m.set_uniform("_TouchPosShine", (0.63 + 0.37 * ((time * 43.).sin() * 0.5 + 0.5)) * 2.);
        gl_use_material(m);
        draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE);
        gl_use_default_material();
    });
}

fn copy_scene(res: &Resource, pass: Option<miniquad::RenderPass>, viewport: (i32, i32, i32, i32), texture: Texture2D) {
    use miniquad::gl::*;
    // These GLES3 constants are absent from this miniquad fork's desktop table.
    const READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
    const ACTIVE_TEXTURE: u32 = 0x84E0;
    const TEXTURE_BINDING_2D: u32 = 0x8069;
    unsafe {
        let restore = if let Some(target) = res.chart_target.as_ref().filter(|target| Some(target.input().render_pass) == pass) {
            let mut read = 0;
            let mut draw = 0;
            glGetIntegerv(READ_FRAMEBUFFER_BINDING, &mut read);
            glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw);
            target.blit();
            glBindFramebuffer(GL_READ_FRAMEBUFFER, target.output().render_pass.gl_internal_id(get_internal_gl().quad_context));
            Some((read, draw))
        } else {
            // Miniquad end_render_pass restores its default framebuffer after
            // flush. Select the logical pass explicitly for this copy, using
            // GLES2-compatible FRAMEBUFFER APIs, then restore the GL binding.
            let mut binding = 0;
            glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut binding);
            if let Some(pass) = pass {
                glBindFramebuffer(GL_FRAMEBUFFER, pass.gl_internal_id(get_internal_gl().quad_context));
            }
            Some((binding, -1))
        };
        let mut active = 0;
        let mut bound = 0;
        glGetIntegerv(ACTIVE_TEXTURE, &mut active);
        glActiveTexture(GL_TEXTURE0);
        glGetIntegerv(TEXTURE_BINDING_2D, &mut bound);
        glBindTexture(GL_TEXTURE_2D, texture.raw_miniquad_texture_handle().gl_internal_id());
        // Storage is allocated on viewport changes above; do not reallocate
        // a full-resolution texture on every frame.
        glCopyTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, viewport.0, viewport.1, viewport.2, viewport.3);
        glBindTexture(GL_TEXTURE_2D, bound as u32);
        glActiveTexture(active as u32);
        if let Some((read, draw)) = restore {
            if draw < 0 {
                glBindFramebuffer(GL_FRAMEBUFFER, read as u32);
            } else {
                glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
                glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw as u32);
            }
        }
    }
}

const VERTEX: &str = include_str!("block_shader_full.vert");
const FRAGMENT: &str = include_str!("block_shader_full.frag");
