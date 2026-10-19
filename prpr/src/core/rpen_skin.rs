//! RPEN noise-skin v1 rendering. Named targets, channel writes and frame history
//! belong to each layer instance; no shared global framebuffer/cache state.
use super::{Resource, Vector, Zone};
use crate::{
    ext::SafeTexture,
    fs::FileSystem,
    parse::rpen::{flag, number},
};
use anyhow::{ensure, Context, Result};
use macroquad::prelude::*;
use miniquad::{PipelineParams, TextureWrap, UniformType};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

const VERTEX: &str = r#"#version 100
attribute vec3 position;attribute vec2 texcoord;attribute vec4 color0;
varying highp vec2 uv;uniform mat4 Model;uniform mat4 Projection;
void main(){gl_Position=Projection*Model*vec4(position,1.);uv=texcoord;}"#;
const COPY: &str =
    "precision highp float;varying highp vec2 uv;uniform sampler2D screenTexture;void main(){gl_FragColor=texture2D(screenTexture,uv);}";
fn path(path: &str) -> Result<&str> {
    ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains(['\\', ':', '\0', '?', '#'])
            && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
        "invalid RPEN skin path: {path}"
    );
    Ok(path)
}

async fn load_file(fs: &mut dyn FileSystem, root: &str, file: &str) -> Result<Vec<u8>> {
    match fs.load_file(&format!("{root}/{file}")).await {
        Ok(bytes) => Ok(bytes),
        Err(error) => {
            let name = root.strip_prefix("shader/").unwrap_or("");
            if ["NoiseField", "Ocean"].contains(&name) {
                Ok(macroquad::file::load_file(&format!("rpen/noise-skins/{name}/{file}"))
                    .await
                    .with_context(|| format!("missing bundled skin {root}/{file}"))?)
            } else {
                Err(error)
            }
        }
    }
}

// RGBA8 storage emulates R8/RG8 sampling, including their implicit alpha=1.
fn packed_samplers(source: &str, inputs: &HashMap<String, String>, spec: &Value) -> String {
    let mut source = source.to_owned();
    let mut helpers = String::new();
    for (name, target) in inputs {
        let channels = match spec["targets"][target]["format"].as_str() {
            Some("r8") => "c.r,0.,0.,1.",
            Some("rg8") => "c.rg,0.,1.",
            _ => continue,
        };
        let pattern = format!(r"texture2D\s*\(\s*{}\s*,", regex::escape(name));
        source = regex::Regex::new(&pattern)
            .unwrap()
            .replace_all(&source, format!("rpenSample_{name}("))
            .into_owned();
        helpers += &format!("\nvec4 rpenSample_{name}(vec2 p){{vec4 c=texture2D({name},p);return vec4({channels});}}\n");
    }
    // Keep byte offsets while excluding comments, including shaders whose
    // declarations share one line. Helpers need both precision and samplers.
    let declarations = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*")
        .unwrap()
        .replace_all(&source, |c: &regex::Captures| c[0].bytes().map(|b| if b == b'\n' { '\n' } else { ' ' }).collect::<String>());
    let end = regex::Regex::new(r"\buniform\s+(?:(?:highp|mediump|lowp)\s+)?\w+\s+\w+(?:\s*\[[^\]]+\])?\s*;")
        .unwrap()
        .find_iter(&declarations)
        .last()
        .map_or(0, |m| m.end());
    source.insert_str(end, &helpers);
    source
}

struct Shader {
    material: Material,
    uniforms: HashMap<String, UniformType>,
    textures: Vec<String>,
}
impl Shader {
    fn new(source: &str, mask: (bool, bool, bool, bool)) -> Result<Self> {
        let source = regex::Regex::new(r"(?m)^\s*#version[^\n]*\n")
            .unwrap()
            .replace_all(source, "")
            .into_owned();
        let mut source = source;
        // The GLES2 material API has no array descriptors. Expand the protocol's
        // fixed touch array without losing dynamic indexing or touch identities.
        if source.contains("touchPoints[") {
            let re = regex::Regex::new(r"uniform\s+vec4\s+touchPoints\s*\[\s*32\s*\]\s*;").unwrap();
            let mut decl = String::new();
            for i in 0..32 {
                decl += &format!("uniform vec4 rpenTouch{i};\n");
            }
            decl += "vec4 rpenTouchAt(int i){\n";
            for i in 0..31 {
                decl += &format!("if(i=={i})return rpenTouch{i};\n");
            }
            decl += "return rpenTouch31;}\n";
            source = re.replace(&source, decl).into_owned();
            source = regex::Regex::new(r"touchPoints\s*\[([^\]]+)\]")
                .unwrap()
                .replace_all(&source, "rpenTouchAt($1)")
                .into_owned();
        }
        if !source.contains("precision ") {
            source = format!("precision highp float;\n{source}");
        }
        source = format!("#version 100\n{source}");
        let re = regex::Regex::new(r"uniform\s+(?:(?:highp|mediump|lowp)\s+)?(\w+)\s+(\w+)\s*;").unwrap();
        let mut uniforms = HashMap::new();
        let mut textures = Vec::new();
        for c in re.captures_iter(&source) {
            let name = c[2].to_owned();
            let kind = match &c[1] {
                "sampler2D" => {
                    textures.push(name);
                    continue;
                }
                "float" => UniformType::Float1,
                "vec2" => UniformType::Float2,
                "vec3" => UniformType::Float3,
                "vec4" => UniformType::Float4,
                "int" | "bool" => UniformType::Int1,
                _ => anyhow::bail!("unsupported skin uniform type {}", &c[1]),
            };
            uniforms.insert(name, kind);
        }
        ensure!(textures.len() <= 16, "too many skin texture samplers");
        let material = load_material(
            VERTEX,
            &source,
            MaterialParams {
                uniforms: uniforms.iter().map(|(n, t)| (n.clone(), *t)).collect(),
                textures: textures.clone(),
                pipeline_params: PipelineParams {
                    color_blend: None,
                    alpha_blend: None,
                    color_write: mask,
                    ..Default::default()
                },
                ..Default::default()
            },
        )?;
        Ok(Self {
            material,
            uniforms,
            textures,
        })
    }
    fn texture(&self, name: &str, texture: Texture2D) {
        if self.textures.iter().any(|s| s == name) {
            self.material.set_texture(name, texture);
        }
    }
    fn value(&self, name: &str, value: &Value) {
        let Some(t) = self.uniforms.get(name) else { return };
        let a = |i| number(&value[i], 0.) as f32;
        match t {
            UniformType::Int1 => self.material.set_uniform(
                name,
                if value.is_boolean() {
                    i32::from(flag(value))
                } else {
                    number(value, 0.) as i32
                },
            ),
            UniformType::Float2 => self.material.set_uniform(name, vec2(a(0), a(1))),
            UniformType::Float3 => self.material.set_uniform(name, vec3(a(0), a(1), a(2))),
            UniformType::Float4 => self.material.set_uniform(name, vec4(a(0), a(1), a(2), a(3))),
            _ => self.material.set_uniform(name, number(value, 0.) as f32),
        }
    }
}
impl Drop for Shader {
    fn drop(&mut self) {
        self.material.delete();
    }
}
struct Pass {
    shader: Shader,
    output: String,
    component: bool,
    inputs: HashMap<String, String>,
    constants: HashMap<String, Value>,
    images: Vec<(String, SafeTexture)>,
}
struct Target {
    front: RenderTarget,
    back: RenderTarget,
    history: RenderTarget,
    spec: Value,
}
impl Drop for Target {
    fn drop(&mut self) {
        self.front.delete();
        self.back.delete();
        self.history.delete();
    }
}
struct Instance {
    width: u32,
    height: u32,
    screen: [RenderTarget; 2],
    mask: SafeTexture,
    mask_zones: Vec<Zone>,
    targets: HashMap<String, Target>,
    clock: Option<f64>,
    touches: HashMap<u64, (Vector, f64, Option<f64>)>,
}
impl Drop for Instance {
    fn drop(&mut self) {
        for target in self.screen {
            target.delete();
        }
    }
}
pub struct Skin {
    passes: Vec<Pass>,
    spec: Value,
    parameters: HashMap<String, Value>,
    copy: Shader,
    instance: RefCell<Option<Instance>>,
    failed: Cell<bool>,
    pub time_offset: f64,
}
fn dims(w: u32, h: u32, spec: &Value) -> Result<(u32, u32)> {
    let a = number(&spec["scaleX"], 1.);
    let b = number(&spec["scaleY"], 1.);
    ensure!(a > 0. && b > 0., "skin target scale must be positive");
    let (w, h) = ((w as f64 * a).floor() as u32, (h as f64 * b).floor() as u32);
    ensure!(w > 0 && h > 0 && w <= 4096 && h <= 4096, "skin target dimensions exceed limit");
    Ok((w, h))
}
fn target(w: u32, h: u32, linear: bool) -> RenderTarget {
    let t = render_target(w, h);
    t.texture.set_filter(if linear { FilterMode::Linear } else { FilterMode::Nearest });
    t
}
fn camera(t: RenderTarget) {
    set_camera(&Camera2D {
        zoom: vec2(1., 1.),
        render_target: Some(t),
        ..Default::default()
    });
}
fn quad(shader: &Shader) {
    gl_use_material(shader.material);
    draw_rectangle(-1., -1., 2., 2., WHITE);
    gl_use_default_material();
    unsafe { get_internal_gl() }.flush();
}
fn copy(shader: &Shader, source: Texture2D, destination: RenderTarget) {
    camera(destination);
    shader.texture("screenTexture", source);
    quad(shader);
}
fn clear(t: RenderTarget) {
    camera(t);
    clear_background(BLANK);
}

impl Skin {
    pub async fn load(layer: &Value, fs: &mut dyn FileSystem) -> Result<Option<Self>> {
        let Some(root) = layer["skin"].as_str().filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        path(root)?;
        ensure!(root.starts_with("shader/") && root.split('/').count() == 2, "invalid skin root");
        let main = String::from_utf8(load_file(fs, root, "main.glsl").await?)?;
        let config = match load_file(fs, root, "config.json").await {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes)?,
            Err(_) => json!({"version":1,"targets":{},"passes":[{"shader":"main.glsl","output":"$screen"}]}),
        };
        ensure!(config["version"] == 1, "unsupported noise skin version");
        let spec = if layer["channel"] == "low" && !config["lowPerformance"].is_null() {
            config["lowPerformance"].clone()
        } else {
            config.clone()
        };
        let mut parameters = HashMap::new();
        let defaults = regex::Regex::new(r"uniform\s+(float|vec2|vec4)\s+(\w+)\s*;[^\r\n]*?//\s*%([^%\r\n]+)%").unwrap();
        for c in defaults.captures_iter(&main) {
            let values = c[3]
                .split(',')
                .map(|s| s.trim().parse::<f64>())
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let count = match &c[1] {
                "vec2" => 2,
                "vec4" => 4,
                _ => 1,
            };
            ensure!(values.len() == count && values.iter().all(|v| v.is_finite()), "invalid skin parameter default");
            parameters.insert(c[2].to_owned(), if count == 1 { json!(values[0]) } else { json!(values) });
        }
        for (k, v) in layer["parameters"].as_object().into_iter().flatten() {
            ensure!(
                v.is_number() || v.as_array().is_some_and(|a| [2, 4].contains(&a.len()) && a.iter().all(Value::is_number)),
                "invalid skin parameter {k}"
            );
            parameters.insert(k.clone(), v.clone());
        }
        ensure!(parameters.get("touchReleaseDuration").map_or(true, |v| number(v, 0.) >= 0.), "negative touch release duration");
        let mut passes = Vec::new();
        let defs = spec["passes"].as_array().context("missing skin passes")?;
        ensure!(!defs.is_empty() && defs.len() <= 64, "skin pass count must be 1..64");
        let image_re = regex::Regex::new(r"uniform\s+sampler2D\s+(\w+)\s*;\s*//\s*@texture\s+(\S+)([^\r\n]*)").unwrap();
        let mut written = std::collections::HashSet::new();
        for pass in defs {
            let file = path(pass["shader"].as_str().context("missing pass shader")?)?;
            let source = String::from_utf8(load_file(fs, root, file).await?)?;
            let output = pass["output"].as_str().context("missing skin output")?;
            let mut parts = output.split('.');
            let output = parts.next().unwrap();
            let component = parts.next();
            ensure!(parts.next().is_none(), "invalid skin component output");
            ensure!(output == "$screen" || spec["targets"].get(output).is_some(), "unknown skin target {output}");
            ensure!(
                component.is_none_or(|c| output != "$screen"
                    && c.len() == 1
                    && match spec["targets"][output]["format"].as_str().unwrap_or("rgba8") {
                        "r8" => "r",
                        "rg8" => "rg",
                        _ => "rgba",
                    }
                    .contains(c)),
                "invalid skin target component"
            );
            let mask = if let Some(c) = component {
                (c == "r", c == "g", c == "b", c == "a")
            } else {
                (true, true, true, true)
            };
            let inputs: HashMap<String, String> = pass["inputs"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| Ok((k.clone(), v.as_str().context("invalid pass input")?.to_owned())))
                .collect::<Result<_>>()?;
            for input in inputs.values() {
                ensure!(
                    input == "$screen"
                        || spec["targets"].get(input).is_some()
                            && (flag(&spec["targets"][input]["noClear"]) || written.contains(input) && input != output),
                    "skin reads target before initialization: {input}"
                );
            }
            let source = packed_samplers(&source, &inputs, &spec);
            let shader = Shader::new(&source, mask).with_context(|| format!("noise skin {root}/{file}"))?;
            let mut images = Vec::new();
            for c in image_re.captures_iter(&source) {
                let directory = file.rsplit_once('/').map_or("", |p| p.0);
                let image_path = if directory.is_empty() {
                    c[2].to_owned()
                } else {
                    format!("{directory}/{}", &c[2])
                };
                path(&image_path)?;
                let data = load_file(fs, root, &image_path).await?;
                let mut image = image::load_from_memory(&data)?.to_rgba8();
                for p in image.pixels_mut() {
                    for i in 0..3 {
                        let mut value = p[i] as f32 / 255.;
                        if c[3].contains("colorspace=srgb") {
                            value = if value <= 0.04045 {
                                value / 12.92
                            } else {
                                ((value + 0.055) / 1.055).powf(2.4)
                            }
                        }
                        if c[3].contains("alphaMode=premult") {
                            value *= p[3] as f32 / 255.;
                        }
                        p[i] = (value * 255.).round() as u8;
                    }
                }
                image::imageops::flip_vertical_in_place(&mut image);
                let tex = Texture2D::from_rgba8(image.width() as u16, image.height() as u16, image.as_raw());
                tex.set_filter(if c[3].contains("filter=nearest") {
                    FilterMode::Nearest
                } else {
                    FilterMode::Linear
                });
                tex.raw_miniquad_texture_handle().set_wrap(
                    unsafe { get_internal_gl() }.quad_context,
                    if c[3].contains("wrap=mirror") {
                        TextureWrap::Mirror
                    } else if c[3].contains("wrap=repeat") {
                        TextureWrap::Repeat
                    } else {
                        TextureWrap::Clamp
                    },
                );
                images.push((c[1].to_owned(), SafeTexture::from(tex)));
            }
            let constants = pass["constants"]
                .as_object()
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            passes.push(Pass {
                shader,
                output: output.to_owned(),
                component: component.is_some(),
                inputs,
                constants,
                images,
            });
            written.insert(output.to_owned());
        }
        Ok(Some(Self {
            passes,
            spec,
            parameters,
            copy: Shader::new(COPY, (true, true, true, true))?,
            instance: RefCell::new(None),
            failed: Cell::new(false),
            time_offset: 0.,
        }))
    }
    pub fn reset(&self) {
        self.instance.borrow_mut().take();
        self.failed.set(false);
    }
    pub fn failed(&self) -> bool {
        self.failed.get()
    }
    pub fn render(&self, res: &mut Resource, zones: &[Zone], touches: &[(u64, Vector)]) -> bool {
        if self.failed.get() {
            return false;
        }
        if let Err(error) = self.try_render(res, zones, touches) {
            self.failed.set(true);
            crate::scene::show_error(error.context("RPEN noise skin rendering failed; restored native noise material"));
            false
        } else {
            true
        }
    }
    fn try_render(&self, res: &mut Resource, zones: &[Zone], touches: &[(u64, Vector)]) -> Result<()> {
        let Some(chart_target) = res.chart_target.as_ref() else {
            anyhow::bail!("RPEN skin requires chart render target")
        };
        unsafe { get_internal_gl() }.flush();
        let destination = chart_target.output();
        let source = destination.texture;
        let vp = res.camera.viewport.unwrap_or(res.last_vp);
        let (width, height) = (vp.2.max(1) as u32, vp.3.max(1) as u32);
        let mut cell = self.instance.borrow_mut();
        if cell.as_ref().is_none_or(|s| s.width != width || s.height != height) {
            let mut targets = HashMap::new();
            let mut pixels = width as u64 * height as u64 * 2;
            for (name, spec) in self.spec["targets"].as_object().into_iter().flatten() {
                let (w, h) = dims(width, height, spec)?;
                pixels += w as u64 * h as u64 * 3;
                ensure!(pixels <= 32_000_000, "RPEN skin render targets exceed pixel budget");
                let linear = spec["filter"] == "linear";
                targets.insert(
                    name.clone(),
                    Target {
                        front: target(w, h, linear),
                        back: target(w, h, linear),
                        history: target(w, h, linear),
                        spec: spec.clone(),
                    },
                );
            }
            let (mask_w, mask_h) = dims(width, height, &self.spec["blockMask"])?;
            let mask = SafeTexture::from(Texture2D::from_rgba8(mask_w as u16, mask_h as u16, &vec![0; mask_w as usize * mask_h as usize * 4]));
            mask.set_filter(FilterMode::Nearest);
            *cell = Some(Instance {
                width,
                height,
                screen: [target(width, height, false), target(width, height, false)],
                mask,
                mask_zones: Vec::new(),
                targets,
                clock: None,
                touches: HashMap::new(),
            });
        }
        let s = cell.as_mut().unwrap();
        let parent = unsafe { get_internal_gl() }.quad_gl.get_model_matrix();
        let inverse = parent.inverse();
        ensure!(inverse.is_finite(), "non-invertible skin parent transform");
        unsafe { get_internal_gl() }.quad_gl.push_model_matrix(inverse);
        push_camera_state();
        let result = (|| -> Result<()> {
            if s.mask_zones != zones {
                let bytes = super::block_shader::skin_mask(s.mask.width() as usize, s.mask.height() as usize, res.aspect_ratio, zones);
                s.mask.update(&Image {
                    bytes,
                    width: s.mask.width() as u16,
                    height: s.mask.height() as u16,
                });
                s.mask_zones = zones.to_vec();
            }
            let repeated = s.clock == Some(res.time);
            let backwards = s.clock.is_none_or(|t| res.time < t);
            for t in s.targets.values_mut() {
                if backwards || !flag(&t.spec["noClear"]) {
                    clear(t.front);
                    clear(t.history);
                } else if repeated {
                    copy(&self.copy, t.history.texture, t.front);
                } else {
                    copy(&self.copy, t.front.texture, t.history);
                }
            }
            if backwards {
                s.touches.clear();
            }
            for &(id, p) in touches {
                let t = s.touches.entry(id).or_insert((p, res.time, None));
                t.0 = p;
                if t.2.is_some() {
                    t.1 = res.time;
                    t.2 = None;
                }
            }
            for (id, t) in &mut s.touches {
                if !touches.iter().any(|(i, _)| i == id) && t.2.is_none() {
                    t.2 = Some(res.time);
                }
            }
            let release = self.parameters.get("touchReleaseDuration").map_or(0.1, |v| number(v, 0.1));
            s.touches.retain(|_, t| t.2.is_none_or(|end| res.time - end <= release));
            let mut touch_data: Vec<_> = s.touches.iter().collect();
            touch_data.sort_by_key(|(id, _)| **id);
            touch_data.truncate(32);
            // Capture only the chart viewport; menus and letterbox bars never enter the skin pipeline.
            camera(s.screen[0]);
            clear_background(BLANK);
            draw_texture_ex(
                source,
                -1.,
                -1.,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(2., 2.)),
                    source: Some(Rect::new(vp.0 as f32, vp.1 as f32, width as f32, height as f32)),
                    ..Default::default()
                },
            );
            unsafe { get_internal_gl() }.flush();
            let mut screen = 0;
            for pass in &self.passes {
                let out = if pass.output == "$screen" {
                    s.screen[1 - screen]
                } else {
                    s.targets[&pass.output].back
                };
                if pass.component {
                    copy(&self.copy, s.targets[&pass.output].front.texture, out);
                }
                camera(out);
                let shader = &pass.shader;
                shader.texture("screenTexture", s.screen[screen].texture);
                shader.texture("blockMask", *s.mask);
                for (name, input) in &pass.inputs {
                    let tex = if input == "$screen" {
                        s.screen[screen].texture
                    } else {
                        s.targets[input].front.texture
                    };
                    shader.texture(name, tex);
                    shader.value(&format!("{name}Size"), &json!([tex.width(), tex.height()]));
                }
                for (name, image) in &pass.images {
                    shader.texture(name, **image);
                    shader.value(&format!("{name}Size"), &json!([image.width(), image.height()]));
                }
                for (name, value) in &self.parameters {
                    shader.value(name, value);
                }
                for (name, value) in &pass.constants {
                    shader.value(name, value);
                }
                for (name, value) in [
                    ("screenSize", json!([width, height])),
                    ("targetSize", json!([out.texture.width(), out.texture.height()])),
                    ("targetTexelSize", json!([1. / out.texture.width(), 1. / out.texture.height()])),
                    ("screenTextureSize", json!([width, height])),
                    ("blockMaskSize", json!([s.mask.width(), s.mask.height()])),
                    ("time", json!(res.time + self.time_offset)),
                    ("touchCount", json!(touch_data.len())),
                ] {
                    shader.value(name, &value);
                }
                for (i, (_, t)) in touch_data.iter().enumerate() {
                    let x = if res.config.flip_x() { -t.0.x } else { t.0.x };
                    shader.value(
                        &format!("rpenTouch{i}"),
                        &json!([
                            (x + 1.) / 2.,
                            (t.0.y * res.aspect_ratio + 1.) / 2.,
                            res.time - t.2.unwrap_or(t.1),
                            if t.2.is_some() { 0. } else { 1. }
                        ]),
                    );
                }
                quad(shader);
                if pass.output == "$screen" {
                    screen = 1 - screen;
                } else {
                    let t = s.targets.get_mut(&pass.output).unwrap();
                    std::mem::swap(&mut t.front, &mut t.back);
                }
            }
            set_camera(&Camera2D {
                zoom: vec2(1., res.aspect_ratio),
                viewport: Some(vp),
                render_target: Some(destination),
                ..Default::default()
            });
            self.copy.texture("screenTexture", s.screen[screen].texture);
            gl_use_material(self.copy.material);
            let h = 1. / res.aspect_ratio;
            draw_rectangle(-1., -h, 2., 2. * h, WHITE);
            gl_use_default_material();
            unsafe { get_internal_gl() }.flush();
            s.clock = Some(res.time);
            Ok(())
        })();
        pop_camera_state();
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        result
    }
}
