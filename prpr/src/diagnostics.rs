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
    display_frames: VecDeque<DisplayFrame>,
    display_events: VecDeque<DisplayEvent>,
    display_state: Option<DisplayState>,
}
#[derive(Clone, Default, serde::Serialize)]
pub struct DisplayState {
    pub requested_hz: u16,
    pub maximum_hz: u16,
    /// Read back from the link; this remains a preference, not actual Hz.
    pub preferred_range: [f32; 3],
    pub paused: bool,
    pub application_state: isize,
    pub guided_access: bool,
    pub low_power: bool,
    /// NSProcessInfoThermalState: 0 nominal, 1 fair, 2 serious, 3 critical.
    pub thermal_state: isize,
    pub recovery_attempts: u8,
    pub generation: u64,
}
#[derive(Clone, Default, serde::Serialize)]
pub struct DisplayFrame {
    /// Monotonic seconds since display driver installation (not wall-clock).
    pub at_seconds: f64,
    pub timestamp: f64,
    pub target_timestamp: f64,
    pub callback_interval_ms: Option<f64>,
    pub arrival_interval_ms: Option<f64>,
    pub callback_work_ms: f64,
    pub glk_display_ms: f64,
    pub generation: u64,
}
#[derive(Clone, serde::Serialize)]
pub struct DisplayEvent {
    pub at_seconds: f64,
    pub event: String,
    pub state: DisplayState,
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
    pub display_state: Option<DisplayState>,
    pub display_frames: Vec<DisplayFrame>,
    pub display_events: Vec<DisplayEvent>,
}
pub fn display_state(at_seconds: f64, event: Option<&str>, state: DisplayState) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        if !c.enabled {
            return;
        }
        if let Some(event) = event {
            c.display_events.push_back(DisplayEvent {
                at_seconds,
                event: event.into(),
                state: state.clone(),
            });
            if c.display_events.len() > 256 {
                c.display_events.pop_front();
            }
        }
        c.display_state = Some(state);
    });
}
pub fn display_frame(frame: DisplayFrame) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        if !c.enabled {
            return;
        }
        c.display_frames.push_back(frame);
        if c.display_frames.len() > 7200 {
            c.display_frames.pop_front();
        }
    });
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
            display_state: c.display_state.clone(),
            display_frames: c.display_frames.iter().cloned().collect(),
            display_events: c.display_events.iter().cloned().collect(),
        }
    })
}

#[cfg(test)]
mod display_capture_tests {
    use super::*;
    #[test]
    fn display_capture_is_opt_in_bounded_and_clearable() {
        enable(false);
        display_frame(DisplayFrame::default());
        assert!(snapshot().display_frames.is_empty());
        enable(true);
        for i in 0..7300 {
            display_frame(DisplayFrame {
                at_seconds: i as f64,
                ..Default::default()
            });
        }
        for i in 0..300 {
            display_state(
                i as f64,
                Some("resume"),
                DisplayState {
                    generation: i,
                    ..Default::default()
                },
            );
        }
        let sample = snapshot();
        assert_eq!(sample.display_frames.len(), 7200);
        assert_eq!(sample.display_frames[0].at_seconds, 100.);
        assert_eq!(sample.display_events.len(), 256);
        assert_eq!(sample.display_events[0].at_seconds, 44.);
        assert_eq!(sample.display_state.unwrap().generation, 299);
        enable(true);
        assert!(snapshot().display_frames.is_empty());
        assert!(snapshot().display_events.is_empty());
        enable(false);
    }
    #[test]
    fn display_export_separates_preference_from_measured_interval_and_work() {
        enable(true);
        display_state(
            0.,
            Some("active"),
            DisplayState {
                requested_hz: 120,
                preferred_range: [120.; 3],
                ..Default::default()
            },
        );
        display_frame(DisplayFrame {
            timestamp: 30.,
            target_timestamp: 30. + 1. / 60.,
            callback_interval_ms: Some(1000. / 60.),
            glk_display_ms: 2.,
            ..Default::default()
        });
        let value = serde_json::to_value(snapshot()).unwrap();
        assert_eq!(value["display_state"]["requested_hz"], 120);
        assert!(value["display_frames"][0]["callback_interval_ms"].as_f64().unwrap() > 16.6);
        assert_eq!(value["display_frames"][0]["glk_display_ms"], 2.);
        enable(false);
    }
}
