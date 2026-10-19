use super::*;
use crate::{
    core::{Effect, Uniform},
    fs::FileSystem,
};
use macroquad::prelude::*;
use miniquad::UniformType;

pub const NAMES: &[&str] = &[
    "chromatic",
    "circle_blur",
    "fisheye",
    "glitch",
    "grayscale",
    "noise",
    "pixel",
    "radial_blur",
    "shockwave",
    "vignette",
    "chromatic_2",
    "circle_blur_2",
    "fisheye_2",
    "glitch_2",
    "grayscale_2",
    "noise_2",
    "pixel_2",
    "radial_blur_2",
    "shockwave_2",
    "vignette_2",
    "liquid",
    "flowing",
    "image_noise",
    "snow",
    "glow_effect",
    "flip",
    "night_vision",
    "scanline",
    "rain",
    "rainbow",
    "flare",
    "underwater",
    "fog",
    "kaleidoscope",
    "emboss",
    "sobel",
    "oil_painting",
    "distortion",
    "hologram",
    "burn",
    "camera",
    "lightning",
    "old_tv",
    "bokeh",
    "neon",
    "heat_distortion",
    "rays",
    "color_shift",
    "wave",
    "two_tone",
];
pub fn identity(e: &Value) -> Result<String> {
    let v = e
        .get("shader")
        .or_else(|| e.get("shaderType"))
        .or_else(|| e.get("name"))
        .context("missing shader identity")?;
    let mut name = if let Some(i) = v.as_u64() {
        NAMES.get(i as usize).context("unknown shader index")?.to_string()
    } else {
        let s = v.as_str().context("invalid shader name")?.trim().replace('\\', "/");
        let s = s
            .rsplit('/')
            .next()
            .unwrap_or(&s)
            .trim_end_matches(".glsl")
            .trim_end_matches(".frag")
            .trim_end_matches("_pr");
        let re = regex::Regex::new("([a-z])([A-Z])").unwrap();
        re.replace_all(s, "${1}_${2}").to_ascii_lowercase().replace('-', "_")
    };
    name = match name.as_str() {
        "radialblur" => "radial_blur".into(),
        "circleblur" => "circle_blur".into(),
        "oldtv" => "old_tv".into(),
        "oilpainting" => "oil_painting".into(),
        "heateffect" => "heat_distortion".into(),
        _ => name,
    };
    if flag(&e["clone"]) && NAMES[..10].contains(&name.as_str()) {
        name.push_str("_2");
    }
    Ok(name)
}

struct Parameter {
    name: String,
    kind: UniformType,
    track: Track,
    time: f64,
}
impl Uniform for Parameter {
    fn uniform_pair(&self) -> (String, UniformType) {
        (self.name.clone(), self.kind)
    }
    fn set_time(&mut self, t: f64) {
        self.time = t;
    }
    fn apply(&self, m: &Material) {
        let v = self.track.value(self.time);
        if v.is_null() {
            return;
        }
        let a = |i| number(&v[i], 0.) as f32;
        match self.kind {
            UniformType::Int1 => m.set_uniform(&self.name, if v.is_boolean() { i32::from(flag(&v)) } else { number(&v, 0.) as i32 }),
            UniformType::Float2 => m.set_uniform(&self.name, vec2(a(0), a(1))),
            UniformType::Float3 => m.set_uniform(&self.name, vec3(a(0), a(1), a(2))),
            UniformType::Float4 => m.set_uniform(&self.name, vec4(a(0), a(1), a(2), a(3))),
            _ => m.set_uniform(&self.name, number(&v, 0.) as f32),
        }
    }
}

pub async fn load_effects(raw: &Value, bpm: &mut BpmList, fs: &mut dyn FileSystem) -> Result<Vec<Effect>> {
    let mut events: Vec<(Value, Option<usize>)> = Vec::new();
    for a in [&raw["effects"], &raw["shaderEvents"], &raw["META"]["effects"]] {
        events.extend(a.as_array().into_iter().flatten().filter(|e| shader_event(e)).map(|e| (e.clone(), None)));
    }
    for (i, line) in raw["judgeLineList"].as_array().into_iter().flatten().enumerate() {
        events.extend(
            line["extended"]["paintEvents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| shader_event(e))
                .map(|e| (e.clone(), Some(i))),
        );
    }
    events.sort_by(|(a, _), (b, _)| {
        number(&a["order"], 0.).total_cmp(&number(&b["order"], 0.)).then_with(|| {
            beat(a.get("startTime").or_else(|| a.get("start")).unwrap_or(&Value::from(0)))
                .unwrap_or(0.)
                .total_cmp(&beat(b.get("startTime").or_else(|| b.get("start")).unwrap_or(&Value::from(0))).unwrap_or(0.))
        })
    });
    let uniform_re = regex::Regex::new(r"uniform\s+(?:highp\s+|mediump\s+|lowp\s+)?(float|int|bool|vec[234])\s+(\w+)\s*;").unwrap();
    let mut result = Vec::new();
    for (e, owner) in events {
        let name = identity(&e)?;
        let index = NAMES.iter().position(|n| *n == name).unwrap_or(0);
        let count = raw["judgeLineList"].as_array().map_or(1, |l| l.len().max(1));
        let line = e["line"]
            .as_i64()
            .map(|i| i.rem_euclid(count as i64) as usize)
            .or(owner)
            .unwrap_or(index % count);
        let factor = number(&raw["judgeLineList"][line]["bpmfactor"], 1.);
        ensure!(factor > 0., "invalid shader line bpmfactor");
        let start = beat(
            e.get("startTime")
                .or_else(|| e.get("start"))
                .or_else(|| e.get("time1"))
                .unwrap_or(&Value::from(0)),
        )?;
        let end = beat(
            e.get("endTime")
                .or_else(|| e.get("end"))
                .or_else(|| e.get("time2"))
                .unwrap_or(&Value::from(start)),
        )?;
        ensure!(end >= start, "shader ends before start");
        let range = bpm.time_beats(start) * factor..bpm.time_beats(end) * factor;
        let base = name.strip_suffix("_2").unwrap_or(&name);
        let path = e["shader"].as_str().filter(|s| s.starts_with('/'));
        let source = if let Some(path) = path {
            match fs.load_file(path.trim_start_matches('/')).await {
                Ok(b) => String::from_utf8(b)?,
                Err(_) => preset(base).with_context(|| format!("missing RPEN shader {path}"))?.to_owned(),
            }
        } else {
            preset(base).with_context(|| format!("unknown RPEN shader {name}"))?.to_owned()
        };
        let declared: HashMap<_, _> = uniform_re
            .captures_iter(&source)
            .map(|c| {
                (
                    c[2].to_owned(),
                    match &c[1] {
                        "vec2" => UniformType::Float2,
                        "vec3" => UniformType::Float3,
                        "vec4" => UniformType::Float4,
                        "int" | "bool" => UniformType::Int1,
                        _ => UniformType::Float1,
                    },
                )
            })
            .collect();
        let mut vars: Vec<Box<dyn Uniform>> = Vec::new();
        for (k, v) in e["vars"].as_object().into_iter().flatten() {
            if ["time", "screenSize", "UVScale"].contains(&k.as_str()) {
                continue;
            }
            let Some(&kind) = declared.get(k) else { continue };
            let events = if v.as_array().is_some_and(|a| a.first().is_some_and(Value::is_object)) {
                v.as_array().unwrap().clone()
            } else {
                vec![serde_json::json!({"startTime":0,"endTime":0,"start":v,"end":v})]
            };
            let track = Track::new(&events, bpm, factor, Value::Null, true)?;
            vars.push(Box::new(Parameter {
                name: k.clone(),
                kind,
                track,
                time: 0.,
            }));
        }
        result.push(Effect::new(range, &source, vars, flag(&e["global"]))?);
    }
    Ok(result)
}

pub fn preset(name: &str) -> Option<&'static str> {
    let legacy = match name {
        "circle_blur" => "circleBlur",
        "radial_blur" => "radialBlur",
        _ => name,
    };
    Effect::get_preset(legacy).or_else(|| super::presets::source(name))
}
