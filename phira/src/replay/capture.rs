//! v6 preserves backend input separately from the v5-compatible playback tape.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Capture {
    pub schema: u32,
    pub recorder: String,
    pub inputs: Vec<Input>,
    pub anchors: Vec<Anchor>,
    pub dropped: u64,
    /// Events are subscriber observations; only touch backends may supply OS time.
    pub clock: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub seq: u64,
    pub frame: u64,
    pub active: bool,
    pub event: prpr::input_capture::Input,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub frame: u64,
    pub uptime: f64,
    pub chart_time: f64,
    pub active: bool,
    pub speed: f32,
    /// Pixel viewport and pixel framebuffer height for exact coordinate conversion.
    pub viewport: [i32; 4],
    pub screen_height: f32,
}

impl Capture {
    pub fn new() -> Self {
        Self {
            schema: 1,
            recorder: format!("{} ({}) / replay v6 / input observer 1", crate::PRO_VERSION, env!("GIT_HASH")),
            clock: "uptime-relative; subscriber observation; touch backend timestamp when present".into(),
            ..Default::default()
        }
    }

    pub fn validate(&self, limit: usize) -> anyhow::Result<()> {
        anyhow::ensure!(self.schema == 1 && self.inputs.len() <= limit && self.anchors.len() <= limit, "输入录制版本或数量无效");
        anyhow::ensure!(self.recorder.len() <= 1024 && self.clock.len() <= 1024, "输入录制说明过长");
        let time = |v: f64| v.is_finite() && v.abs() <= 172800.;
        anyhow::ensure!(
            self.inputs.iter().all(|input| {
                let e = &input.event;
                matches!(e.source.as_str(), "touch" | "mouse" | "keyboard")
                    && matches!(e.phase.as_str(), "down" | "move" | "up" | "cancel" | "repeat")
                    && time(e.observed_time)
                    && e.event_time.is_none_or(time)
                    && e.x.is_none_or(f32::is_finite)
                    && e.y.is_none_or(f32::is_finite)
            }) && self.inputs.windows(2).all(|w| w[0].seq < w[1].seq && w[0].frame <= w[1].frame),
            "原始输入序列无效"
        );
        anyhow::ensure!(
            self.anchors.iter().enumerate().all(|(i, a)| a.frame == i as u64
                && time(a.uptime)
                && time(a.chart_time)
                && a.speed.is_finite()
                && a.speed > 0.
                && a.speed <= 16.
                && a.viewport[2] > 0
                && a.viewport[3] > 0
                && a.screen_height.is_finite()
                && a.screen_height > 0.)
                && self.anchors.windows(2).all(|w| w[0].frame < w[1].frame && w[0].uptime <= w[1].uptime),
            "录制时钟锚点无效"
        );
        anyhow::ensure!(
            self.inputs
                .iter()
                .all(|i| self.anchors.get(i.frame as usize).is_some_and(|a| a.active == i.active)),
            "输入缺少对应的时钟锚点"
        );
        Ok(())
    }

    /// Derived display tape. Raw input stays immutable and retains cancellation.
    /// This maps source time through the consuming frame's clock anchor; it does
    /// not invent sub-frame timestamps for mouse/keyboard or rejudge any notes.
    pub fn playback_touches(&self) -> Vec<super::TouchEvent> {
        use std::collections::HashMap;
        let mut ids = HashMap::new();
        let mut next_id = 0u64;
        let mut active: HashMap<u64, (f32, f32)> = HashMap::new();
        let mut out = Vec::new();
        let mut cursor = 0;
        for a in &self.anchors {
            if !a.active {
                for (id, (x, y)) in active.drain() {
                    out.push(super::TouchEvent {
                        phase: super::PHASE_UP,
                        id,
                        x,
                        y,
                        t: a.chart_time as f32,
                    });
                }
            }
            while let Some(input) = self.inputs.get(cursor).filter(|i| i.frame == a.frame) {
                cursor += 1;
                let e = &input.event;
                if !input.active || e.source == "keyboard" {
                    continue;
                }
                let (Some(x), Some(y)) = (e.x, e.y) else { continue };
                let key = (e.source.clone(), e.id);
                let id = *ids.entry(key).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                });
                if e.phase == "move" && !active.contains_key(&id) {
                    continue;
                }
                if matches!(e.phase.as_str(), "up" | "cancel") && !active.contains_key(&id) {
                    continue;
                }
                let phase = match e.phase.as_str() {
                    "down" => super::PHASE_DOWN,
                    "move" => super::PHASE_MOVE,
                    "up" | "cancel" => super::PHASE_UP,
                    _ => continue,
                };
                let [vx, vy, vw, vh] = a.viewport;
                let x = (x - vx as f32) * 2. / vw as f32 - 1.;
                let y = (a.screen_height - (vy + vh) as f32 + vh as f32 / 2. - y) * 2. / vw as f32;
                let observed = e.event_time.unwrap_or(e.observed_time);
                let t = a.chart_time + (observed - a.uptime) * a.speed as f64;
                // An incompatible backend clock cannot silently move an input
                // to another part of the song. The raw value remains available.
                let t = if (observed - a.uptime).abs() <= 0.25 { t } else { a.chart_time };
                out.push(super::TouchEvent {
                    phase,
                    id,
                    x,
                    y,
                    t: t as f32,
                });
                if phase == super::PHASE_UP {
                    active.remove(&id);
                } else {
                    active.insert(id, (x, y));
                }
            }
        }
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }
}
