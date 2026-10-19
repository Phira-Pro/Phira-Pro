//! rpe-next v220 block areas. Times are global beats; coordinates are RPE
//! canvas units. This is deliberately separate from native percentage blocks.
use super::{Vector, Zone};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;

const EPS: f64 = 1e-9;
const UNIT: f64 = 2. / 1350.;

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Deserialize)]
pub struct Beat([i64; 3]);
impl Beat {
    fn value(self) -> Result<f64> {
        ensure!(self.0[2] > 0 && self.0.iter().all(|v| v.unsigned_abs() <= 9_007_199_254_740_991), "invalid beat fraction");
        Ok(self.0[0] as f64 + self.0[1] as f64 / self.0[2] as f64)
    }
}

/// A separate tempo map retains f64 precision, supports negative beats and
/// does not inherit a judge line's bpmfactor or the chart's audio offset.
pub struct Tempo(Vec<(f64, f64, f64)>);
impl Tempo {
    pub fn new(entries: impl IntoIterator<Item = (f64, f64)>) -> Result<Self> {
        let mut entries: Vec<_> = entries.into_iter().collect();
        ensure!(!entries.is_empty(), "empty BPMList");
        ensure!(entries.iter().all(|(b, bpm)| b.is_finite() && bpm.is_finite() && *bpm > 0.), "invalid BPMList");
        entries.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut result = Vec::with_capacity(entries.len());
        let mut seconds = entries[0].0 * 60. / entries[0].1;
        for (i, &(beat, bpm)) in entries.iter().enumerate() {
            if i != 0 {
                seconds += (beat - entries[i - 1].0) * 60. / entries[i - 1].1;
            }
            result.push((beat, bpm, seconds));
        }
        Ok(Self(result))
    }
    pub fn seconds(&self, beat: f64) -> f64 {
        let i = self.0.partition_point(|e| e.0 <= beat).saturating_sub(1);
        let (b, bpm, sec) = self.0[i];
        sec + (beat - b) * 60. / bpm
    }
    fn time(&self, beat: Beat) -> Result<f64> {
        Ok(self.seconds(beat.value()?))
    }
}

fn one() -> f64 {
    1.
}
fn linear() -> u8 {
    1
}
fn points() -> [f64; 4] {
    [0., 0., 1., 1.]
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(untagged)]
enum Flag {
    #[default]
    Absent,
    Bool(bool),
    Number(u8),
}
impl Flag {
    fn get(self) -> Result<bool> {
        match self {
            Self::Absent => Ok(false),
            Self::Bool(v) => Ok(v),
            Self::Number(v) => {
                ensure!(v <= 1, "flag must be 0/1 or boolean");
                Ok(v == 1)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    start_time: Beat,
    end_time: Beat,
    start: f64,
    end: f64,
    #[serde(default = "linear")]
    easing_type: u8,
    #[serde(default)]
    easing_left: f64,
    #[serde(default = "one")]
    easing_right: f64,
    #[serde(default)]
    bezier: Flag,
    #[serde(default = "points")]
    bezier_points: [f64; 4],
    #[serde(default)]
    inst: Flag,
    anchor: Option<Position>,
    transform_start: Option<f64>,
    transform_end: Option<f64>,
    #[serde(default)]
    transform_step: bool,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum Ready {
    #[default]
    Absent,
    Bool(bool),
    Seconds(f64),
}
impl Ready {
    fn seconds(self) -> f64 {
        match self {
            Self::Absent | Self::Bool(true) => 0.5,
            Self::Bool(false) => 0.,
            Self::Seconds(v) => (v.max(0.) * 1000.).round() / 1000.,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Interval {
    start_time: Beat,
    end_time: Beat,
    #[serde(default)]
    ready_flash: Ready,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaData {
    bottom_left: Position,
    top_right: Position,
    appear_time: Beat,
    disappear_time: Beat,
    #[serde(default)]
    is_invert: bool,
    #[serde(default)]
    active_intervals: Vec<Interval>,
    #[serde(default)]
    move_x_events: Vec<Event>,
    #[serde(default)]
    move_y_events: Vec<Event>,
    #[serde(default)]
    rotate_events: Vec<Event>,
    #[serde(default)]
    scale_x_events: Vec<Event>,
    #[serde(default)]
    scale_y_events: Vec<Event>,
}

type Affine = nalgebra::Matrix3<f64>;
#[derive(Clone, Copy)]
enum Axis {
    Move,
    X,
    Y,
    Rotate,
}
impl Axis {
    fn anchored(self) -> bool {
        !matches!(self, Self::Move)
    }
    fn relative(self, from: f64, to: f64, anchor: Position) -> Affine {
        let mut m = Affine::identity();
        match self {
            Self::Move => {}
            Self::Rotate => {
                let (s, c) = (to - from).to_radians().sin_cos();
                m[(0, 0)] = c;
                m[(0, 1)] = s;
                m[(1, 0)] = -s;
                m[(1, 1)] = c;
                m[(0, 2)] = anchor.x - c * anchor.x - s * anchor.y;
                m[(1, 2)] = anchor.y + s * anchor.x - c * anchor.y;
            }
            _ => {
                let ratio = to / from;
                let scale = if from == 0. || !ratio.is_finite() { 1. } else { ratio };
                let i = usize::from(matches!(self, Self::Y));
                m[(i, i)] = scale;
                m[(i, 2)] = (if i == 0 { anchor.x } else { anchor.y }) * (1. - scale);
            }
        }
        m
    }
}
struct TimedEvent {
    data: Event,
    start: f64,
    end: f64,
    prefix: Affine,
}
struct Track {
    events: Vec<TimedEvent>,
    axis: Axis,
    fallback: f64,
}
impl Track {
    fn new(events: Vec<Event>, axis: Axis, fallback: f64, center: Position, tempo: &Tempo) -> Result<Self> {
        let mut result = Self {
            events: Vec::with_capacity(events.len()),
            axis,
            fallback,
        };
        for data in events {
            let start = tempo.time(data.start_time)?;
            let end = tempo.time(data.end_time)?;
            ensure!(start <= end, "event starts after its end");
            ensure!(data.start.is_finite() && data.end.is_finite(), "non-finite event value");
            ensure!((1..=29).contains(&data.easing_type), "easingType must be 1..29");
            ensure!(data.easing_left >= 0. && data.easing_right <= 1. && data.easing_left < data.easing_right, "invalid easing bounds");
            ensure!(!data.inst.get()? || data.start == data.end, "inst requires equal start/end values");
            if data.bezier.get()? {
                ensure!(
                    data.bezier_points.iter().all(|v| v.is_finite())
                        && (0. ..=1.).contains(&data.bezier_points[0])
                        && (0. ..=1.).contains(&data.bezier_points[2]),
                    "invalid bezierPoints"
                );
            }
            if axis.anchored() {
                let a = data.anchor.context("missing transform anchor")?;
                ensure!(a.x.is_finite() && a.y.is_finite(), "invalid anchor");
            } else {
                ensure!(
                    data.transform_start.is_none() && data.transform_end.is_none() && !data.transform_step,
                    "transform metadata on movement track"
                );
            }
            ensure!(data.transform_start.into_iter().chain(data.transform_end).all(|v| v.is_finite()), "invalid transform reference");
            ensure!(!data.transform_step || start == end, "transformStep requires zero duration");
            result.events.push(TimedEvent {
                data,
                start,
                end,
                prefix: Affine::identity(),
            });
        }
        // Stable sorting preserves the order of instantaneous cumulative steps.
        result.events.sort_by(|a, b| a.start.total_cmp(&b.start));
        for i in 1..result.events.len() {
            let (before, after) = result.events.split_at_mut(i);
            let prev = &before[i - 1];
            let next = &mut after[0];
            let chain = axis.anchored() && prev.start == next.start && prev.start == prev.end && prev.data.transform_step;
            ensure!(next.start >= prev.end && (next.start != prev.start || chain), "overlapping events");
            if axis.anchored() {
                let anchor = prev.data.anchor.unwrap_or(center);
                let from = prev.data.transform_start.unwrap_or(prev.data.start);
                let end = prev.data.transform_end.unwrap_or(prev.data.end);
                let next_from = next.data.transform_start.unwrap_or(next.data.start);
                next.prefix = axis.relative(end, next_from, anchor) * axis.relative(from, end, anchor) * prev.prefix;
            }
        }
        Ok(result)
    }
    fn sample(&self, time: f64) -> (f64, Option<&TimedEvent>) {
        let n = self.events.partition_point(|e| e.start <= time + EPS);
        if n == 0 {
            return (self.fallback, None);
        }
        let entry = &self.events[n - 1];
        let e = &entry.data;
        if e.start == e.end {
            return (e.start, Some(entry));
        }
        let p = if entry.end == entry.start {
            1.
        } else {
            ((time - entry.start) / (entry.end - entry.start)).clamp(0., 1.)
        };
        let eased = if e.bezier.get().unwrap() {
            // Solve x(t), not y(progress); do not quantize control point keys.
            let [x1, y1, x2, y2] = e.bezier_points;
            let component = |t: f64, a, b| 3. * t * (1. - t).powi(2) * a + 3. * t * t * (1. - t) * b + t.powi(3);
            let (mut lo, mut hi) = (0., 1.);
            for _ in 0..40 {
                let mid = (lo + hi) / 2.;
                if component(mid, x1, x2) < p {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            component((lo + hi) / 2., y1, y2)
        } else {
            let f = |v: f64| curve(e.easing_type, v);
            let base = f(e.easing_left);
            let range = f(e.easing_right) - base;
            if range.abs() < 1e-12 {
                p
            } else {
                (f(e.easing_left + (e.easing_right - e.easing_left) * p) - base) / range
            }
        };
        (e.start + (e.end - e.start) * eased, Some(entry))
    }
    fn transform(&self, point: nalgebra::Vector3<f64>, sample: (f64, Option<&TimedEvent>), center: Position) -> nalgebra::Vector3<f64> {
        if let Some(e) = sample.1 {
            self.axis
                .relative(e.data.transform_start.unwrap_or(e.data.start), sample.0, e.data.anchor.unwrap_or(center))
                * e.prefix
                * point
        } else {
            point
        }
    }
}

// Standard easing equations, evaluated in f64. The older line tween table has
// different exponential endpoints and Back/Elastic InOut parameters, so using
// that table here would change the exported noise geometry.
pub(crate) fn curve(id: u8, p: f64) -> f64 {
    use std::f64::consts::PI;
    let (family, direction) = match id {
        1 => return p,
        2 => (1, 1),
        3 => (1, 0),
        4 => (2, 1),
        5 => (2, 0),
        6 => (1, 2),
        7 => (2, 2),
        8 => (3, 1),
        9 => (3, 0),
        10 => (4, 1),
        11 => (4, 0),
        12 => (3, 2),
        13 => (4, 2),
        14 => (5, 1),
        15 => (5, 0),
        16 => (6, 1),
        17 => (6, 0),
        18 => (7, 1),
        19 => (7, 0),
        20 => (8, 1),
        21 => (8, 0),
        22 => (7, 2),
        23 => (8, 2),
        24 => (9, 1),
        25 => (9, 0),
        26 => (10, 1),
        27 => (10, 0),
        28 => (10, 2),
        29 => (9, 2),
        _ => return p,
    };
    let bounce_out = |x: f64| {
        let (shift, offset) = if x < 4. / 11. {
            (0., 0.)
        } else if x < 8. / 11. {
            (6. / 11., 0.75)
        } else if x < 10. / 11. {
            (9. / 11., 0.9375)
        } else {
            (21. / 22., 0.984375)
        };
        121. / 16. * (x - shift).powi(2) + offset
    };
    let inside = |x: f64| match family {
        1 => 1. - (x * PI / 2.).cos(),
        2..=5 => x.powi(family),
        6 => {
            if x == 0. {
                0.
            } else {
                2_f64.powf(10. * (x - 1.))
            }
        }
        7 => 1. - (1. - x * x).abs().sqrt(),
        8 => x * x * ((1.70158 + 1.) * x - 1.70158),
        9 => {
            if x == 0. || x == 1. {
                x
            } else {
                -2_f64.powf(10. * x - 10.) * ((10. * x - 10.75) * 2. * PI / 3.).sin()
            }
        }
        10 => 1. - bounce_out(1. - x),
        _ => x,
    };
    match direction {
        0 => inside(p),
        1 => 1. - inside(1. - p),
        _ if family == 8 => {
            let k = 1.70158 * 1.525;
            if p < 0.5 {
                let x = 2. * p;
                x * x * ((k + 1.) * x - k) / 2.
            } else {
                let x = 2. * p - 2.;
                (x * x * ((k + 1.) * x + k) + 2.) / 2.
            }
        }
        _ if family == 9 => {
            if p == 0. || p == 1. {
                p
            } else {
                let wave = ((20. * p - 11.125) * 2. * PI / 4.5).sin() / 2.;
                if p < 0.5 {
                    -2_f64.powf(20. * p - 10.) * wave
                } else {
                    2_f64.powf(10. - 20. * p) * wave + 1.
                }
            }
        }
        _ => {
            if p < 0.5 {
                inside(2. * p) / 2.
            } else {
                1. - inside(2. - 2. * p) / 2.
            }
        }
    }
}

pub struct Area {
    pub layer: usize,
    center: Position,
    half: Position,
    appear: f64,
    disappear: f64,
    intervals: Vec<(f64, f64, f64)>,
    invert: bool,
    tracks: [Track; 5], // move X/Y, rotate, scale X/Y
}

#[derive(Clone, PartialEq)]
pub struct Sample {
    pub layer: usize,
    pub zone: Zone,
    pub state: u8,
    pub mix: f32,
}

impl Area {
    pub fn new(data: AreaData, tempo: &Tempo) -> Result<Self> {
        let center = Position {
            x: (data.bottom_left.x + data.top_right.x) / 2.,
            y: (data.bottom_left.y + data.top_right.y) / 2.,
        };
        let half = Position {
            x: (data.top_right.x - data.bottom_left.x).abs() / 2.,
            y: (data.top_right.y - data.bottom_left.y).abs() / 2.,
        };
        ensure!([center.x, center.y, half.x, half.y].iter().all(|v| v.is_finite()), "invalid rectangle");
        let appear = tempo.time(data.appear_time)?;
        let disappear = tempo.time(data.disappear_time)?;
        ensure!(appear <= disappear, "appearTime is after disappearTime");
        let mut raw_intervals = Vec::new();
        for i in data.active_intervals {
            let start = tempo.time(i.start_time)?;
            let end = tempo.time(i.end_time)?;
            ensure!(start <= end, "invalid active interval");
            if start < end {
                raw_intervals.push((start, end, i.ready_flash.seconds()));
            }
        }
        raw_intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        ensure!(raw_intervals.windows(2).all(|w| w[0].1 <= w[1].0), "overlapping active intervals");
        let intervals = raw_intervals
            .into_iter()
            .map(|(a, b, r)| (a.max(appear), b.min(disappear), r))
            .filter(|(a, b, _)| a < b)
            .collect();
        let track = |events, axis, fallback, name: &str| Track::new(events, axis, fallback, center, tempo).with_context(|| name.to_owned());
        Ok(Self {
            layer: 0,
            center,
            half,
            appear,
            disappear,
            intervals,
            invert: data.is_invert,
            tracks: [
                track(data.move_x_events, Axis::Move, center.x, "moveXEvents")?,
                track(data.move_y_events, Axis::Move, center.y, "moveYEvents")?,
                track(data.rotate_events, Axis::Rotate, 0., "rotateEvents")?,
                track(data.scale_x_events, Axis::X, 1., "scaleXEvents")?,
                track(data.scale_y_events, Axis::Y, 1., "scaleYEvents")?,
            ],
        })
    }
    pub fn sample(&self, time: f64, aspect: f32) -> Option<Sample> {
        if time + EPS < self.appear || time + EPS >= self.disappear {
            return None;
        }
        let n = self.intervals.partition_point(|i| i.0 <= time + EPS);
        let active = n != 0 && time + EPS < self.intervals[n - 1].1;
        let next = self.intervals.get(n);
        let ready = !active && next.is_some_and(|&(a, _, r)| r > 0. && time + EPS >= a - r);
        let mix = if active {
            1.
        } else if ready {
            let &(a, _, r) = next.unwrap();
            ((1. + (((time - a + r) / r).clamp(0., 1.) * std::f64::consts::PI * 6.).cos()) / 2.) as f32
        } else {
            0.
        };
        let values = self.tracks.each_ref().map(|tr| tr.sample(time));
        let mut p = nalgebra::Vector3::new(self.center.x, self.center.y, 1.);
        for i in [3, 4, 2] {
            p = self.tracks[i].transform(p, values[i], self.center);
        }
        p.x += values[0].0 - self.center.x;
        p.y += values[1].0 - self.center.y;
        let half = Vector::new((self.half.x * values[3].0).abs() as f32, (self.half.y * values[4].0).abs() as f32) * UNIT as f32;
        if half.x == 0. || half.y == 0. {
            return None;
        }
        Some(Sample {
            layer: self.layer,
            state: if active {
                2
            } else if ready {
                1
            } else {
                0
            },
            mix,
            zone: Zone {
                projection: None,
                line2area: true,
                y_scale: 1.5 / aspect,
                color: [1., 84. / 255., 84. / 255.],
                center: Vector::new((p.x * UNIT) as f32, (p.y * UNIT * 1.5 / aspect as f64) as f32),
                half,
                angle: -values[2].0.to_radians() as f32,
                invert: self.invert,
                active,
                ready,
                opacity: 1.,
            },
        })
    }
}

pub fn contains(zone: &Zone, p: Vector, inset: f32) -> bool {
    let (s, c) = zone.angle.sin_cos();
    let d = p - zone.center;
    let y = d.y / zone.y_scale;
    let local = Vector::new(c * d.x + s * y, -s * d.x + c * y);
    let sign = if zone.invert { 1. } else { -1. };
    let ix = (inset / c.hypot(s * zone.y_scale)).min(zone.half.x * 0.5);
    let iy = (inset / s.hypot(c * zone.y_scale)).min(zone.half.y * 0.5);
    local.x.abs() <= zone.half.x + sign * ix && local.y.abs() <= zone.half.y + sign * iy
}

pub fn blocked(samples: &[Sample], point: Vector, aspect: f32, single: bool) -> bool {
    let coverage = |inset, layer| {
        let (mut normal, mut inversions) = (false, 0);
        for s in samples.iter().filter(|s| s.state == 2 && s.layer == layer) {
            if contains(&s.zone, point, inset) {
                if s.zone.invert {
                    inversions += 1;
                } else {
                    normal = true;
                }
            }
        }
        normal ^ if single { inversions == 1 } else { inversions % 2 == 1 }
    };
    let mut layers: Vec<_> = samples.iter().map(|s| s.layer).collect();
    layers.sort_unstable();
    layers.dedup();
    layers
        .into_iter()
        .any(|layer| coverage(0., layer) && coverage(super::touch_inset_world(aspect), layer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn fixture() -> (Vec<Area>, Value) {
        let chart: Value = serde_json::from_str(include_str!("../../tests/fixtures/rpe-next/chart.json")).unwrap();
        let tempo = Tempo::new(
            chart["BPMList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (serde_json::from_value::<Beat>(v["startTime"].clone()).unwrap().value().unwrap(), v["bpm"].as_f64().unwrap())),
        )
        .unwrap();
        let areas = chart["blockAreaList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Area::new(serde_json::from_value(v.clone()).unwrap(), &tempo).unwrap())
            .collect();
        (areas, serde_json::from_str(include_str!("../../tests/fixtures/rpe-next/samples.json")).unwrap())
    }
    #[test]
    fn next_runtime_matches_upstream_export_samples_across_aspects_and_reverse_seek() {
        let (areas, reference) = fixture();
        for aspect in [1., 4. / 3., 1.5, 16. / 9., 19.5 / 9., 22. / 9.] {
            for sample in reference["samples"].as_array().unwrap().iter().rev() {
                let id = sample["id"].as_u64().unwrap() as usize;
                let time = sample["time"].as_f64().unwrap();
                let state = sample["state"].as_str().unwrap();
                let actual = areas[id].sample(time, aspect);
                if state.starts_with("hidden") {
                    assert!(actual.is_none());
                    continue;
                }
                // Zero scale is invisible and nonblocking although runtime
                // reference still reports its degenerate points.
                let points = sample["points"].as_array().unwrap();
                let expected: Vec<_> = points
                    .iter()
                    .map(|p| Vector::new(p["x"].as_f64().unwrap() as f32, p["y"].as_f64().unwrap() as f32))
                    .collect();
                if actual.is_none() {
                    assert!((expected[0] - expected[1]).norm() < 1e-4 || (expected[1] - expected[2]).norm() < 1e-4, "id={id} time={time}");
                    continue;
                }
                let actual = actual.unwrap();
                let z = &actual.zone;
                assert_eq!(
                    actual.state,
                    match state {
                        "disabled" => 0,
                        "ready" => 1,
                        "active" => 2,
                        _ => panic!(),
                    },
                    "id={id} time={time}"
                );
                assert!((actual.mix as f64 - sample["mix"].as_f64().unwrap()).abs() < 1e-6);
                let center = Vector::new(z.center.x / UNIT as f32, z.center.y / (UNIT as f32 * 1.5 / aspect));
                let target = Vector::new(sample["center"]["x"].as_f64().unwrap() as f32, sample["center"]["y"].as_f64().unwrap() as f32);
                assert!((center - target).norm() < 0.005, "id={id} time={time} aspect={aspect}, center {center:?} != {target:?}");
                let corners: Vec<_> = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                    .iter()
                    .map(|&(x, y)| center + nalgebra::Rotation2::new(z.angle) * Vector::new(x * z.half.x / UNIT as f32, y * z.half.y / UNIT as f32))
                    .collect();
                for p in expected {
                    let error = corners.iter().map(|q| (p - q).norm()).fold(f32::INFINITY, f32::min);
                    assert!(error < 0.01, "id={id} time={time} aspect={aspect}, point {p:?} error {error}");
                }
            }
        }
    }
    #[test]
    fn next_active_only_blocks_and_inversion_option_changes_three_overlaps() {
        let (areas, _) = fixture();
        let aspect = 1.5;
        let sample = areas[0].sample(1.1, aspect).unwrap();
        assert_eq!(sample.state, 2);
        let point = sample.zone.center;
        assert!(blocked(&[sample.clone()], point, aspect, false));
        let mut inactive = sample.clone();
        inactive.state = 1;
        assert!(!blocked(&[inactive], point, aspect, false));
        let mut inverse = sample.clone();
        inverse.zone.invert = true;
        let triple = [inverse.clone(), inverse.clone(), inverse.clone()];
        assert!(blocked(&triple, point, aspect, false));
        assert!(!blocked(&triple, point, aspect, true));
        assert!(!blocked(&[sample, inverse], point, aspect, false));
    }
    #[test]
    fn next_invalid_tracks_and_beats_report_errors() {
        let tempo = Tempo::new([(0., 120.)]).unwrap();
        let base: Value = serde_json::json!({"bottomLeft":{"x":0,"y":0},"topRight":{"x":100,"y":100},"appearTime":[0,0,1],"disappearTime":[4,0,1]});
        for (field, value) in [
            ("appearTime", serde_json::json!([0, 0, 0])),
            ("disappearTime", serde_json::json!([-1, 0, 1])),
            ("rotateEvents", serde_json::json!([{"startTime":[0,0,1],"endTime":[1,0,1],"start":0,"end":90}])),
        ] {
            let mut v = base.clone();
            v[field] = value;
            assert!(Area::new(serde_json::from_value(v).unwrap(), &tempo).is_err());
        }
    }
}
