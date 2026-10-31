use super::RPE_TWEEN_MAP;
use anyhow::{Context, Result};
use macroquad::prelude::{Color, Vec2};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

use super::L10N_LOCAL;
#[cfg(feature = "video")]
use crate::core::Video;
use crate::{
    core::{Anim, BpmList, ChartExtra, ClampedTween, Effect, Keyframe, StaticTween, Triple, Tweenable, Uniform, EPS},
    ext::ScaleType,
    fs::FileSystem,
};

// serde is weird...
fn f32_zero() -> f32 {
    0.
}

fn f32_one() -> f32 {
    1.
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtKeyframe<T> {
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    easing_type: i32,
    start: T,
    end: T,
    start_time: Triple,
    end_time: Triple,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum ExtAnim<V> {
    #[default]
    Default,
    Fixed(V),
    Keyframes(Vec<ExtKeyframe<V>>),
}

impl<V> ExtAnim<V> {
    fn into<T: Tweenable>(self, r: &mut BpmList, default: Option<T>) -> Anim<T>
    where
        V: Into<T>,
    {
        match self {
            ExtAnim::Default => Anim::default(),
            ExtAnim::Fixed(value) => Anim::fixed(value.into()),
            ExtAnim::Keyframes(events) => {
                let mut kfs = Vec::new();
                if let Some(default) = default {
                    if events[0].start_time.beats() != 0.0 {
                        kfs.push(Keyframe::new(0.0, default, 0));
                    }
                }
                for e in events {
                    kfs.push(Keyframe {
                        time: r.time(&e.start_time),
                        value: e.start.into(),
                        tween: {
                            let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                            if e.easing_left.abs() < EPS as f32 && (e.easing_right - 1.0).abs() < EPS as f32 {
                                StaticTween::get_rc(tween)
                            } else {
                                Arc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                            }
                        },
                    });
                    kfs.push(Keyframe::new(r.time(&e.end_time), e.end.into(), 0));
                }
                Anim::new(kfs)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtBpmItem {
    time: Triple,
    bpm: f64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BpmForm {
    Single(f64),
    List(Vec<ExtBpmItem>),
}

impl TryFrom<BpmForm> for BpmList {
    type Error = anyhow::Error;

    fn try_from(value: BpmForm) -> Result<Self> {
        let entries: Vec<_> = match value {
            BpmForm::Single(value) => vec![(0., value)],
            BpmForm::List(list) => list.into_iter().map(|it| (it.time.beats(), it.bpm)).collect(),
        };
        anyhow::ensure!(!entries.is_empty(), "extra.json: empty bpm list");
        anyhow::ensure!(entries.iter().all(|(beat, bpm)| beat.is_finite() && bpm.is_finite() && *bpm > 0.), "extra.json: invalid bpm");
        anyhow::ensure!(entries.windows(2).all(|pair| pair[0].0 <= pair[1].0), "extra.json: bpm times must be ordered");
        Ok(BpmList::new(entries))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Variable {
    Float(ExtAnim<f32>),
    Vec2(ExtAnim<(f32, f32)>),
    Color(ExtAnim<[u8; 4]>),
}

#[derive(Deserialize)]
struct ExtEffect {
    start: Triple,
    end: Triple,
    shader: String,
    #[serde(default)]
    vars: HashMap<String, Variable>,
    #[serde(default)]
    global: bool,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExtVideo {
    path: String,
    #[serde(default)]
    time: Triple,
    #[serde(default)]
    scale: ScaleType,
    #[serde(default)]
    alpha: ExtAnim<f32>,
    #[serde(default)]
    dim: ExtAnim<f32>,
    #[cfg(feature = "video")]
    #[serde(default)]
    attach: Option<crate::core::VideoAttach>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Extra {
    #[serde(default)]
    bpm: Option<BpmForm>,
    #[serde(default)]
    effects: Vec<ExtEffect>,
    #[serde(default)]
    videos: Vec<ExtVideo>,
}

async fn parse_effect(r: &mut BpmList, rpe: ExtEffect, fs: &mut dyn FileSystem) -> Result<Effect> {
    let range = r.time(&rpe.start)..r.time(&rpe.end);
    let vars = rpe
        .vars
        .into_iter()
        .map(|(name, var)| -> Result<Box<dyn Uniform>> {
            Ok(match var {
                Variable::Float(events) => Box::new((name, events.into::<f32>(r, None))),
                Variable::Vec2(events) => Box::new((name, events.into::<Vec2>(r, None))),
                Variable::Color(events) => Box::new((name, events.into::<Color>(r, None))),
            })
        })
        .collect::<Result<_>>()?;
    let string;
    Effect::new(
        range,
        if let Some(path) = rpe.shader.strip_prefix('/') {
            string = String::from_utf8(fs.load_file(path).await?).with_context(|| ptl!("shader-load-failed", "path" => path))?;
            &string
        } else {
            Effect::get_preset(&rpe.shader).ok_or_else(|| ptl!(err "shader-not-found", "shader" => rpe.shader))?
        },
        vars,
        rpe.global,
    )
}

pub async fn parse_extra(source: &str, fs: &mut dyn FileSystem) -> Result<ChartExtra> {
    parse_extra_with_bpm(source, fs, None).await
}

fn decode_extra(source: &str, chart_bpm: Option<BpmList>) -> Result<(Extra, BpmList)> {
    let bytes = super::json_bytes(source.as_bytes());
    anyhow::ensure!(bytes.trim_ascii_start().first() == Some(&b'{'), "extra.json: expected a JSON object");
    let mut ext: Extra = serde_json::from_slice(bytes).with_context(|| ptl!("json-parse-failed"))?;
    let bpm = if let Some(bpm) = ext.bpm.take() {
        BpmList::try_from(bpm)?
    } else if ext.effects.is_empty() && ext.videos.is_empty() {
        BpmList::default() // No timed content will access this map.
    } else {
        chart_bpm.filter(|bpm| !bpm.elements().is_empty()).context("extra.json: timed effects or videos require bpm or a chart tempo map")?
    };
    Ok((ext, bpm))
}

/// Empty editor sidecars need no tempo. Timed content may inherit the chart's
/// complete tempo map; an explicit sidecar BPM always takes precedence.
pub async fn parse_extra_with_bpm(source: &str, fs: &mut dyn FileSystem, chart_bpm: Option<BpmList>) -> Result<ChartExtra> {
    let (ext, mut r) = decode_extra(source, chart_bpm)?;
    let mut effects = Vec::new();
    let mut global_effects = Vec::new();
    for (id, effect) in ext.effects.into_iter().enumerate() {
        (if effect.global { &mut global_effects } else { &mut effects }).push(
            parse_effect(&mut r, effect, fs)
                .await
                .with_context(|| ptl!("effect-location", "id" => id))?,
        );
    }
    #[cfg(feature = "video")]
    let mut videos = Vec::new();
    #[cfg(feature = "video")]
    for video in ext.videos {
        videos.push((
            Video::new(
                fs.load_file(&video.path)
                    .await
                    .with_context(|| ptl!("video-load-failed", "path" => video.path.clone()))?,
                r.time(&video.time),
                video.scale,
                video.alpha.into(&mut r, Some(1.)),
                video.dim.into(&mut r, Some(0.)),
            )
            .with_context(|| ptl!("video-load-failed", "path" => video.path))?,
            video.attach,
        ));
    }
    #[cfg(not(feature = "video"))]
    if !ext.videos.is_empty() {
        tracing::warn!("video is disabled in this build");
    }
    Ok(ChartExtra {
        effects,
        global_effects,
        #[cfg(feature = "video")]
        videos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_editor_sidecars_do_not_require_bpm_or_depend_on_line_count() {
        for source in ["{}", "{\n\"effects\": []\n}", "\u{feff}{\"effects\":[],\"videos\":[]}"] {
            let (extra, bpm) = decode_extra(source, None).unwrap();
            assert!(extra.effects.is_empty() && extra.videos.is_empty());
            assert!(bpm.elements().is_empty());
        }
    }

    #[test]
    fn timed_extra_inherits_all_tempo_changes_and_honors_explicit_bpm() {
        let effect = r#""effects":[{"start":[4,0,1],"end":[8,0,1],"shader":"vignette"}]"#;
        let source = format!("{{{effect}}}");
        let (extra, mut bpm) = decode_extra(&source, Some(BpmList::new(vec![(0., 120.), (4., 60.)]))).unwrap();
        assert_eq!(bpm.time(&extra.effects[0].start), 2.);
        assert_eq!(bpm.time(&extra.effects[0].end), 6.);
        for explicit in ["60", r#"[{"time":[0,0,1],"bpm":60}]"#] {
            let (extra, mut bpm) = decode_extra(&format!("{{\"bpm\":{explicit},{effect}}}"), Some(BpmList::new(vec![(0., 120.)]))).unwrap();
            assert_eq!(bpm.time(&extra.effects[0].start), 4.);
            assert_eq!(bpm.time(&extra.effects[0].end), 8.);
        }
        assert!(decode_extra(&source, None).is_err());
    }

    #[test]
    fn malformed_sidecars_and_invalid_tempos_still_report_errors() {
        for source in ["{", "[]", r#"{"effects":{}}"#, r#"{"bpm":0}"#, r#"{"bpm":[]}"#,
            r#"{"bpm":[{"time":[0,0,0],"bpm":120}]}"#] {
            assert!(decode_extra(source, None).is_err(), "accepted {source}");
        }
    }
}
