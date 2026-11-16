use super::{Anim, Resource, Tweenable};
use crate::ext::{get_viewport, screen_aspect};
use anyhow::{anyhow, bail, Result};
use macroquad::prelude::*;
use miniquad::UniformType;
use once_cell::sync::Lazy;
use phf::phf_map;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};

/// Debug switch: optimized shader variants, shared effect materials and the
/// idle-chart bypass. Shader/material choices are made when effects are
/// created, so those parts apply on chart reload.
pub static EFFECT_RENDER_OPTIMIZED: AtomicBool = AtomicBool::new(true);

static SHADERS: phf::Map<&'static str, &'static str> = phf_map! {
    "chromatic" => include_str!("shaders/chromatic.glsl"),
    "circleBlur" => include_str!("shaders/circle_blur.glsl"),
    "fisheye" => include_str!("shaders/fisheye.glsl"),
    "glitch" => include_str!("shaders/glitch.glsl"),
    "grayscale" => include_str!("shaders/grayscale.glsl"),
    "noise" => include_str!("shaders/noise.glsl"),
    "pixel" => include_str!("shaders/pixel.glsl"),
    "radialBlur" => include_str!("shaders/radial_blur.glsl"),
    "shockwave" => include_str!("shaders/shockwave.glsl"),
    "vignette" => include_str!("shaders/vignette.glsl"),
};

pub trait UniformValue: Clone + Default {
    const UNIFORM_TYPE: UniformType;
}

impl UniformValue for f32 {
    const UNIFORM_TYPE: UniformType = UniformType::Float1;
}

impl UniformValue for Vec2 {
    const UNIFORM_TYPE: UniformType = UniformType::Float2;
}
impl UniformValue for Vec3 {
    const UNIFORM_TYPE: UniformType = UniformType::Float3;
}
impl UniformValue for i32 {
    const UNIFORM_TYPE: UniformType = UniformType::Int1;
}

impl UniformValue for Color {
    const UNIFORM_TYPE: UniformType = UniformType::Float4;
}

pub trait Uniform {
    fn uniform_pair(&self) -> (String, UniformType);
    fn set_time(&mut self, t: f64);
    fn apply(&self, material: &Material);
}

impl<T: UniformValue> Uniform for (String, T) {
    fn uniform_pair(&self) -> (String, UniformType) {
        (self.0.clone(), T::UNIFORM_TYPE)
    }

    fn set_time(&mut self, _t: f64) {}

    fn apply(&self, material: &Material) {
        material.set_uniform(&self.0, self.1.clone());
    }
}

impl<T: UniformValue + Tweenable> Uniform for (String, Anim<T>) {
    fn uniform_pair(&self) -> (String, UniformType) {
        (self.0.clone(), T::UNIFORM_TYPE)
    }

    fn set_time(&mut self, t: f64) {
        self.1.set_time(t);
    }

    fn apply(&self, material: &Material) {
        material.set_uniform(&self.0, self.1.now());
    }
}

struct SharedMaterial {
    material: Material,
}

impl Drop for SharedMaterial {
    fn drop(&mut self) {
        self.material.delete();
    }
}

static SHARED_MATERIALS: Lazy<Mutex<HashMap<String, Weak<SharedMaterial>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn load_effect_material(key: String, shader: &str, uniforms: Vec<(String, UniformType)>, optimized: bool) -> Result<Arc<SharedMaterial>> {
    if optimized {
        if let Some(existing) = SHARED_MATERIALS.lock().unwrap_or_else(|it| it.into_inner()).get(&key).and_then(Weak::upgrade) {
            return Ok(existing);
        }
    }
    let material = load_material(
        VERTEX_SHADER,
        shader,
        MaterialParams {
            uniforms,
            textures: vec!["screenTexture".to_owned()],
            ..Default::default()
        },
    )?;
    let shared = Arc::new(SharedMaterial { material });
    if !optimized {
        return Ok(shared);
    }
    let mut registry = SHARED_MATERIALS.lock().unwrap_or_else(|it| it.into_inner());
    registry.retain(|_, weak| weak.strong_count() > 0);
    registry.insert(key, Arc::downgrade(&shared));
    Ok(shared)
}

pub struct Effect {
    time_range: Range<f64>,
    t: f64,
    material: Arc<SharedMaterial>,
    defaults: Vec<Box<dyn Uniform>>,
    uniforms: Vec<Box<dyn Uniform>>,
    pub global: bool,
}

impl Effect {
    pub fn get_preset(name: &str) -> Option<&'static str> {
        SHADERS.get(name).copied()
    }

    pub fn new(time_range: Range<f64>, preset: Option<&str>, shader: &str, uniforms: Vec<Box<dyn Uniform>>, global: bool) -> Result<Self> {
        let optimized = EFFECT_RENDER_OPTIMIZED.load(Ordering::Relaxed);
        // `sampleCount` is a dynamic uniform, so a driver cannot unroll the
        // sampling loop. When the chart leaves it at the shader default (3),
        // use the same arithmetic with a compile-time bound instead.
        let shader = if optimized && preset == Some("chromatic") && !uniforms.iter().any(|u| u.uniform_pair().0 == "sampleCount") {
            include_str!("shaders/chromatic_static3.glsl")
        } else if optimized && preset == Some("glitch") {
            include_str!("shaders/glitch_optimized.glsl")
        } else {
            shader
        };
        static DEF_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"uniform\s+(\w+)\s+(\w+);\s+//\s+%([^%]+)%").unwrap());
        let defaults = DEF_REGEX
            .captures_iter(shader)
            .map(|caps| -> Result<Box<dyn Uniform>> {
                let type_name = caps.get(1).unwrap().as_str();
                let name = caps.get(2).unwrap().as_str().to_owned();
                let value = caps.get(3).unwrap().as_str();
                Ok(match type_name {
                    "float" => Box::new((name, value.parse::<f32>()?)),
                    "int" => Box::new((name, value.parse::<i32>()?)),
                    "bool" => Box::new((name, i32::from(value.trim() == "true" || value.trim() == "1"))),
                    "vec3" => {
                        let v = value
                            .split(',')
                            .map(|v| v.trim().parse::<f32>())
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        if v.len() != 3 {
                            bail!("Expected x,y,z");
                        }
                        Box::new((name, vec3(v[0], v[1], v[2])))
                    }
                    "vec2" => Box::new((name, {
                        let (x, y) = value.split_once(',').ok_or_else(|| anyhow!("Expected x,y"))?;
                        vec2(x.trim().parse()?, y.trim().parse()?)
                    })),
                    "vec4" => Box::new((name, {
                        let values: Vec<_> = value.split(',').map(|it| it.trim()).collect();
                        if values.len() != 4 {
                            bail!("Expected r,g,b,a");
                        }
                        Color::new(values[0].parse()?, values[1].parse()?, values[2].parse()?, values[3].parse()?)
                    })),
                    _ => bail!("Unknown type: {type_name}"),
                })
            })
            .collect::<Result<Vec<Box<dyn Uniform>>>>()?;
        let mut ocurred_uniforms = HashSet::new();
        let mut new_uniforms = Vec::new();
        let mut add_uniform = |(name, its_type): (String, UniformType)| {
            if ocurred_uniforms.insert(name.clone()) {
                new_uniforms.push((name, its_type));
            }
        };
        for def in &defaults {
            add_uniform(def.uniform_pair());
        }
        add_uniform(("time".to_owned(), UniformType::Float1));
        add_uniform(("screenSize".to_owned(), UniformType::Float2));
        add_uniform(("UVScale".to_owned(), UniformType::Float2));
        for u in &uniforms {
            add_uniform(u.uniform_pair());
        }
        let mut key = shader.to_owned();
        key.push('#');
        for (name, ty) in &new_uniforms {
            key.push_str(name);
            key.push(':');
            key.push_str(&format!("{ty:?}"));
            key.push(';');
        }
        Ok(Self {
            time_range,
            t: f64::NEG_INFINITY,
            defaults,
            material: load_effect_material(key, shader, new_uniforms, optimized)?,
            uniforms,
            global,
        })
    }

    pub fn active(&self) -> bool {
        self.time_range.contains(&self.t)
    }

    pub fn update(&mut self, res: &Resource) {
        let t = res.time;
        self.t = t;
        if self.time_range.contains(&t) {
            for uniform in &mut self.uniforms {
                uniform.set_time(t);
            }
        }
    }

    /// Submit a real fragment draw during loading. Keep the current camera,
    /// pass, timing and uniforms; custom effects never sample a live chart here.
    pub(crate) fn prepare(&mut self, texture: Texture2D) {
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();
        for def in &self.defaults {
            def.apply(&self.material.material);
        }
        for uniform in &mut self.uniforms {
            uniform.set_time(self.time_range.start.max(0.));
            uniform.apply(&self.material.material);
        }
        self.material.material.set_texture("screenTexture", texture);
        self.material.material.set_uniform("time", self.time_range.start.max(0.) as f32);
        self.material.material.set_uniform("screenSize", vec2(texture.width(), texture.height()));
        self.material.material.set_uniform("UVScale", vec2(1., 1.));
        // One tiny draw under the next loading-screen redraw, never a full
        // resolution offscreen target or a delayed gameplay effect.
        gl_use_material(self.material.material);
        draw_rectangle(0., 0., 2. / screen_width().max(1.), 2. / screen_height().max(1.), WHITE);
        gl_use_default_material();
        gl.flush();
    }

    pub fn render(&self, res: &mut Resource) {
        if !self.time_range.contains(&self.t) {
            return;
        }
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();
        // macroquad's flush only submits its own batch. The previous pass must
        // be submitted before the driver samples its texture (Mali-G52).
        unsafe { miniquad::gl::glFlush() };
        let material = self.material.material;

        for def in &self.defaults {
            def.apply(&material);
        }
        for uniform in &self.uniforms {
            uniform.apply(&material);
        }
        material.set_uniform("time", self.t as f32);
        let target = res.chart_target.as_mut().unwrap();
        target.swap();
        let tex = target.old().texture;
        material.set_texture("screenTexture", tex);
        let screen_dim = vec2(tex.width(), tex.height());
        material.set_uniform("screenSize", screen_dim);
        gl.quad_gl.render_pass(Some(target.output().render_pass));

        let vp = get_viewport();
        material.set_uniform("UVScale", vec2(vp.2 as _, vp.3 as _) / screen_dim);

        // The captured chart already contains the practice view transform.
        // Postprocessing must cover the viewport once, rather than zoom/pan
        // the captured image a second time. Keep the chart's model reflection.
        let practice_view = !self.global && res.config.practice_active && res.config.practice.view.active();
        if practice_view {
            push_camera_state();
            set_camera(&Camera2D {
                zoom: vec2(1., -res.aspect_ratio),
                viewport: Some(vp),
                render_target: Some(target.output()),
                ..Default::default()
            });
        }
        gl_use_material(material);
        let top = 1. / if self.global { screen_aspect() } else { res.aspect_ratio };
        draw_rectangle(-1., -top, 2., top * 2., WHITE);
        gl_use_default_material();
        if practice_view {
            gl.flush();
            pop_camera_state();
        }
    }
}

const VERTEX_SHADER: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec2 UVScale;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    uv = (texcoord - vec2(0.5)) * UVScale + vec2(0.5);
}"#;
