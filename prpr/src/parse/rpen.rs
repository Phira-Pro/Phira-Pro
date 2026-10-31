//! RPEN format adapters. Chart data never executes JavaScript or installs a mod.
use crate::core::{AnimFloat, BpmList, Keyframe, TweenFunction};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

mod effects;
mod expression;
mod presets;
mod trajectory;
pub use effects::load_effects;
pub use trajectory::Trajectory;

pub fn features(chart: &Value) -> bool {
    let nonempty = |v: &Value| v.as_array().is_some_and(|a| !a.is_empty()) || v.as_object().is_some_and(|a| !a.is_empty());
    if number(&chart["META"]["RPEVersion"], 0.) >= 230.
        && chart["blockAreaList"]
            .as_array()
            .is_some_and(|a| a.iter().any(|a| a.get("bottomLeft").is_some()))
    {
        return true;
    }
    if ["noiseAreaLayers", "globalEvents", "effects", "shaderEvents"]
        .iter()
        .any(|k| nonempty(&chart[k]))
        || nonempty(&chart["META"]["effects"])
    {
        return true;
    }
    chart["judgeLineList"].as_array().into_iter().flatten().any(|line| {
        line["bpmfactor"].as_f64().is_some_and(|v| (v - 1.).abs() > 1e-12)
            || line["skewControl"]
                .as_array()
                .is_some_and(|a| a.iter().any(|p| p["skew"].as_f64().unwrap_or(0.) != 0.))
            || line["extended"]["paintEvents"].as_array().is_some_and(|a| a.iter().any(shader_event))
            || line["eventLayers"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|l| l["moveXEvents"].as_array().into_iter().flatten().any(|e| e["trajectory"].is_object()))
    }) || chart["blockAreaList"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|a| a.get("layerId").is_some())
}

pub fn shader_event(v: &Value) -> bool {
    v.get("shader").is_some() || v.get("shaderType").is_some() || v.get("name").is_some()
}

/// Keep shader paint events out of the unrelated legacy numeric Paint renderer.
pub fn legacy_source(chart: &Value) -> Value {
    let mut result = chart.clone();
    for line in result["judgeLineList"].as_array_mut().into_iter().flatten() {
        if let Some(events) = line["extended"]["paintEvents"].as_array_mut() {
            events.retain(|v| !shader_event(v));
        }
        if line["extended"]["paintEvents"].as_array().is_some_and(|v| v.is_empty()) {
            line["extended"].as_object_mut().unwrap().remove("paintEvents");
        }
    }
    result
}

pub fn beat(v: &Value) -> Result<f64> {
    let n = if let Some(v) = v.as_f64() {
        v
    } else {
        let a = v.as_array().context("expected beat triple")?;
        ensure!(a.len() == 3, "invalid beat triple");
        let d = a[2].as_f64().context("invalid beat denominator")?;
        ensure!(d != 0., "zero beat denominator");
        a[0].as_f64().context("invalid beat integer")? + a[1].as_f64().context("invalid beat numerator")? / d
    };
    ensure!(n.is_finite(), "non-finite beat");
    Ok(n)
}

pub fn number(v: &Value, fallback: f64) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(fallback)
}
pub fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_i64().unwrap_or(0) != 0)
}
pub fn ease(e: &Value, p: f64) -> f64 {
    let p = p.clamp(0., 1.);
    if flag(&e["bezier"]) {
        let pts = e["bezierPoints"].as_array();
        let v = |i, d| pts.and_then(|v| v.get(i)).map_or(d, |v| number(v, d));
        crate::core::bezier_progress(p, [v(0, 0.), v(1, 0.), v(2, 1.), v(3, 1.)])
    } else {
        let id = number(&e["easingType"], 1.) as u8;
        let f = |p| crate::core::rpe_next_block::curve(id, p);
        let l = number(&e["easingLeft"], 0.);
        let r = number(&e["easingRight"], 1.);
        let d = f(r) - f(l);
        if d.abs() < 1e-12 {
            p
        } else {
            (f(l + (r - l) * p) - f(l)) / d
        }
    }
}

/// An event track is evaluated by seconds, including holds through gaps and reverse seeks.
#[derive(Clone)]
pub struct Track {
    events: Vec<(f64, f64, Value)>,
    fallback: Value,
    before_default: bool,
}
impl Track {
    pub fn new(events: &[Value], bpm: &mut BpmList, factor: f64, fallback: Value, before_default: bool) -> Result<Self> {
        ensure!(factor.is_finite() && factor > 0., "invalid bpmfactor");
        let mut parsed = Vec::new();
        for e in events {
            let start = bpm.time_beats(beat(&e["startTime"])?) * factor;
            let end = bpm.time_beats(beat(&e["endTime"])?) * factor;
            ensure!(start <= end, "RPEN event ends before start");
            parsed.push((start, end, e.clone()));
        }
        parsed.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Self {
            events: parsed,
            fallback,
            before_default,
        })
    }
    pub fn value(&self, time: f64) -> Value {
        let n = self.events.partition_point(|v| v.0 <= time);
        if self.events.is_empty() || (n == 0 && self.before_default) {
            return self.fallback.clone();
        }
        let (a, b, e) = &self.events[n.saturating_sub(1)];
        let p = if b <= a { 1. } else { ((time - a) / (b - a)).clamp(0., 1.) };
        let p = ease(e, p);
        fn mix(a: &Value, b: &Value, p: f64) -> Value {
            if let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) {
                return Value::from(a + (b - a) * p);
            }
            if let (Some(a), Some(b)) = (a.as_array(), b.as_array()) {
                return Value::Array(a.iter().enumerate().map(|(i, a)| mix(a, b.get(i).unwrap_or(a), p)).collect());
            }
            if p >= 1. {
                b.clone()
            } else {
                a.clone()
            }
        }
        mix(&e["start"], &e["end"], p)
    }
}

struct MotionTween {
    trajectory: Option<Arc<Trajectory>>,
    event: Value,
    axis: usize,
    phase: f64,
}
impl TweenFunction for MotionTween {
    fn y(&self, p: f32) -> f32 {
        let p = p as f64 * self.phase;
        if let Some(t) = &self.trajectory {
            t.value(p)[self.axis] as f32
        } else {
            let start = number(&self.event["start"], 0.);
            (start + (number(&self.event["end"], 0.) - start) * ease(&self.event, p)) as f32
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Exact expression evaluation; RPEN's export segment count never quantizes playback.
pub fn motion(events: &[Value], axis: usize, bpm: &mut BpmList, factor: f64) -> Result<AnimFloat> {
    let mut entries = events
        .iter()
        .map(|e| Ok((bpm.time_beats(beat(&e["startTime"])?) * factor, bpm.time_beats(beat(&e["endTime"])?) * factor, e)))
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut kfs = Vec::new();
    for (i, &(start, end, e)) in entries.iter().enumerate() {
        ensure!(start <= end, "invalid trajectory time range");
        let trajectory = if e["trajectory"].is_object() {
            ensure!(e["trajectory"]["version"] == 1, "unsupported trajectory version");
            Some(Arc::new(Trajectory::new(&e["trajectory"]["options"])?))
        } else {
            None
        };
        let cut = entries.get(i + 1).map_or(end, |n| end.min(n.0));
        let phase = if end <= start {
            1.
        } else {
            ((cut - start) / (end - start)).clamp(0., 1.)
        };
        let tween = Arc::new(MotionTween {
            trajectory,
            event: e.clone(),
            axis,
            phase,
        });
        if kfs.is_empty() {
            kfs.push(Keyframe::new(start.min(0.) - 1., tween.y(0.), 0));
        }
        if cut > start {
            kfs.push(Keyframe {
                time: start,
                value: 0.,
                tween: tween.clone(),
            });
            kfs.push(Keyframe::new(cut, 1., 0));
        }
        kfs.push(Keyframe::new(cut, tween.y(1.), 0));
    }
    if kfs.is_empty() {
        return Ok(AnimFloat::fixed(0.));
    }
    let mut anim = AnimFloat::new(kfs);
    anim.time = f64::NEG_INFINITY;
    anim.set_time(0.);
    Ok(anim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn feature_detection_requires_actual_extensions() {
        assert!(!features(&json!({"META":{"RPEVersion":240},"judgeLineList":[]})));
        assert!(features(&json!({"judgeLineList":[{"bpmfactor":2}]})));
        assert!(features(&json!({"noiseAreaLayers":[{"id":"default"}]})));
        assert!(!features(&json!({"judgeLineList":[{"bpmfactor":1,"skewControl":[{"skew":0}]}]})));
    }
    #[test]
    fn expression_motion_keeps_closed_curves_and_reverse_seeks() {
        let e = json!({"startTime":[0,0,1],"endTime":[4,0,1],"trajectory":{"version":1,"options":{"xExpression":"200*cos(2*pi*t)","yExpression":"200*sin(2*pi*t)"}}});
        let mut bpm = BpmList::new(vec![(0., 120.)]);
        let mut x = motion(&[e.clone()], 0, &mut bpm, 2.).unwrap();
        let mut y = motion(&[e], 1, &mut bpm, 2.).unwrap();
        for (time, a, b) in [(0., 200., 0.), (1., 0., 200.), (4., 200., 0.), (3., 0., -200.), (1., 0., 200.)] {
            x.set_time(time);
            y.set_time(time);
            assert!((x.now() - a).abs() < 0.001);
            assert!((y.now() - b).abs() < 0.001);
        }
    }
    #[test]
    fn trajectory_values_match_upstream_reference() {
        let reference: Value = serde_json::from_str(include_str!("../../tests/fixtures/rpen-extensions/reference.json")).unwrap();
        for case in reference["trajectories"].as_array().unwrap() {
            let trajectory = Trajectory::new(&case["options"]).unwrap();
            for sample in case["samples"].as_array().unwrap().iter().rev() {
                let value = trajectory.value(sample["p"].as_f64().unwrap());
                for (i, k) in ["x", "y", "rotation"].iter().enumerate() {
                    let expected = sample[k].as_f64().unwrap();
                    assert!(
                        (value[i] - expected).abs() < 0.0001,
                        "axis {k}, case {}, p {}, actual {} expected {expected}",
                        case["options"],
                        sample["p"],
                        value[i]
                    );
                }
            }
        }
    }
    #[test]
    fn numeric_tracks_use_elapsed_seconds_across_tempo_changes() {
        let raw: Value = serde_json::from_str(include_str!("../../tests/fixtures/rpen-extensions/reference.json")).unwrap();
        let points = raw["bpm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| (beat(&b["startTime"]).unwrap(), number(&b["bpm"], 120.)))
            .collect::<Vec<_>>();
        for case in raw["timings"].as_array().unwrap() {
            let factor = number(&case["factor"], 1.);
            let mut bpm = BpmList::new(points.clone());
            for value in case["values"].as_array().unwrap().iter().rev() {
                assert!((bpm.time_beats(number(&value["beat"], 0.)) * factor - number(&value["time"], 0.)).abs() < 1e-9);
            }
            let track = Track::new(
                &[serde_json::json!({"startTime":0,"endTime":10,"start":0,"end":100,"easingType":1})],
                &mut bpm,
                factor,
                Value::Null,
                true,
            )
            .unwrap();
            let end = bpm.time_beats(10.) * factor;
            assert!((number(&track.value(end / 2.), 0.) - 50.).abs() < 1e-9);
            assert!(track.value(-1.).is_null());
        }
    }
    #[test]
    fn overlapping_motion_uses_latest_event_without_unsorted_keyframes() {
        let mut bpm = BpmList::new(vec![(0., 120.)]);
        let mut anim = motion(
            &[
                json!({"startTime":0,"endTime":20,"start":0,"end":100}),
                json!({"startTime":4,"endTime":8,"start":200,"end":300}),
            ],
            0,
            &mut bpm,
            1.,
        )
        .unwrap();
        for (time, expected) in [(1., 10.), (2., 200.), (3., 250.), (5., 300.), (1., 10.)] {
            anim.set_time(time);
            assert!((anim.now() - expected).abs() < 1e-5);
        }
    }
}

pub fn apply_motion(raw: &Value, line: &mut crate::core::JudgeLine, r: &mut BpmList, bpm_factor: f64) -> Result<()> {
    for (axis, track, factor) in [(0, "moveXEvents", 2. / 1350.), (1, "moveYEvents", 2. / 900.), (2, "rotateEvents", -1.)] {
        let mut anis = Vec::new();
        let mut has_trajectory = false;
        for layer in raw["eventLayers"].as_array().into_iter().flatten().filter(|v| v.is_object()) {
            let mut events = layer[track].as_array().cloned().unwrap_or_default();
            if axis != 0 {
                events.extend(
                    layer["moveXEvents"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|e| {
                            e["trajectory"].is_object()
                                && (axis == 1
                                    || flag(&e["trajectory"]["options"]["tangentRotation"])
                                    || e["trajectory"]["options"]["rotationExpression"]
                                        .as_str()
                                        .is_some_and(|s| !s.trim().is_empty()))
                        })
                        .cloned(),
                );
            }
            has_trajectory |= events.iter().any(|e| e["trajectory"].is_object());
            anis.push(motion(&events, axis, r, bpm_factor)?);
        }
        if has_trajectory {
            let mut anim = AnimFloat::chain(anis);
            anim.map_value(|v| v * factor);
            match axis {
                0 => line.object.translation.0 = anim,
                1 => line.object.translation.1 = anim,
                _ => line.object.rotation = anim,
            }
        }
    }

    let mut points = raw["skewControl"].as_array().cloned().unwrap_or_default();
    points.sort_by(|a, b| number(&a["x"], 0.).total_cmp(&number(&b["x"], 0.)));
    if points.len() > 1 {
        line.ctrl_obj.get_mut().skew = AnimFloat::new(
            points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let id = points.get(i + 1).map_or(1, |p| number(&p["easing"], 1.) as usize).min(29);
                    Keyframe::new(number(&p["x"], 0.), number(&p["skew"], 0.) as f32, crate::parse::RPE_TWEEN_MAP[id])
                })
                .collect(),
        );
    }
    Ok(())
}
