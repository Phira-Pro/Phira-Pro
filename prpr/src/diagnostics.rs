//! Explicitly enabled, bounded input/frame diagnostics. Nothing is uploaded.
use macroquad::prelude::*;
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
};
#[derive(Default)]
struct Capture {
    enabled: bool,
    frames: VecDeque<f32>,
    events: VecDeque<String>,
    active: HashMap<u64, Vec2>,
    max: usize,
    cancels: u32,
    persistent: bool,
    session: Option<serde_json::Value>,
}
thread_local! {static CAPTURE:RefCell<Capture>=RefCell::default();}
#[derive(Clone, Default, serde::Serialize)]
pub struct Snapshot {
    pub frames_ms: Vec<f32>,
    pub events: Vec<String>,
    pub active: Vec<(u64, [f32; 2])>,
    pub max_fingers: usize,
    pub cancels: u32,
    pub last_session: Option<serde_json::Value>,
}
pub fn running() -> bool {
    CAPTURE.with(|c| c.borrow().enabled)
}
pub fn persistent() -> bool {
    CAPTURE.with(|c| c.borrow().persistent)
}
pub fn set_persistent(value: bool) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        c.persistent = value;
        c.enabled = true;
    });
}
pub fn session(config: &crate::config::Config, info: &crate::info::ChartInfo) {
    CAPTURE.with(|c|{let mut c=c.borrow_mut();if c.enabled{c.session=Some(serde_json::json!({"chart":info.name,"algorithm":format!("{:?}",config.judge_algorithm),"configured_windows_seconds":config.judge_windows().early,"speed":config.speed,"mods":config.mods.bits(),"sample_count":config.sample_count,"noise_glow":config.noise_glow,"noise_refraction":config.noise_refraction}));}});
}
pub fn enable(enabled: bool) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        if enabled {
            let persistent = c.persistent;
            *c = Capture::default();
            c.persistent = persistent;
        }
        c.enabled = enabled;
    });
}
pub fn frame(events: &[Touch]) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        if !c.enabled {
            return;
        }
        c.frames.push_back(get_frame_time() * 1000.);
        if c.frames.len() > 7200 {
            c.frames.pop_front();
        }
        for t in events {
            c.events
                .push_back(format!("{:.6} ID={} {:?} x={:.1} y={:.1}", get_time(), t.id, t.phase, t.position.x, t.position.y));
            if c.events.len() > 20000 {
                c.events.pop_front();
            }
            match t.phase {
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    c.active.remove(&t.id);
                    if t.phase == TouchPhase::Cancelled {
                        c.cancels += 1;
                    }
                }
                _ => {
                    c.active.insert(t.id, t.position);
                }
            }
            c.max = c.max.max(c.active.len());
        }
    });
}
pub fn snapshot() -> Snapshot {
    CAPTURE.with(|c| {
        let c = c.borrow();
        Snapshot {
            frames_ms: c.frames.iter().copied().collect(),
            events: c.events.iter().cloned().collect(),
            active: c.active.iter().map(|(id, p)| (*id, [p.x, p.y])).collect(),
            max_fingers: c.max,
            cancels: c.cancels,
            last_session: c.session.clone(),
        }
    })
}
