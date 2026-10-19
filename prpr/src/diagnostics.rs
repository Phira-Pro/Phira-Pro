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
    events: VecDeque<InputEvent>,
    active: HashMap<u64, Vec2>,
    max: usize,
    cancels: u32,
    persistent: bool,
    session: Option<serde_json::Value>,
    display_frames: VecDeque<DisplayFrame>,
    display_events: VecDeque<DisplayEvent>,
    display_state: Option<DisplayState>,
}
#[derive(Clone, Copy)]
struct InputEvent {
    at: f64,
    id: u64,
    phase: TouchPhase,
    position: Vec2,
}
impl InputEvent {
    fn text(&self) -> String {
        format!("{:.6} ID={} {:?} x={:.1} y={:.1}", self.at, self.id, self.phase, self.position.x, self.position.y)
    }
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
    });
}
/// Start/resume or stop without destroying the recorded session.
pub fn set_running(value: bool) {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        c.enabled = value;
        c.active.clear();
    });
}
/// Clear data without implicitly starting a stopped sampler.
pub fn clear() {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        let (enabled, persistent) = (c.enabled, c.persistent);
        *c = Capture {
            enabled,
            persistent,
            ..Capture::default()
        };
    });
}
pub fn active_touches() -> Vec<(u64, [f32; 2])> {
    CAPTURE.with(|c| c.borrow().active.iter().map(|(id, p)| (*id, [p.x, p.y])).collect())
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
            c.events.push_back(InputEvent {
                at: get_time(),
                id: t.id,
                phase: t.phase,
                position: t.position,
            });
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
    capture(7200, 20000, 7200, true)
}
/// Small live preview; full logs and JSON session data are copied only on export.
pub fn preview() -> Snapshot {
    capture(240, 12, 120, false)
}
fn capture(frames: usize, events: usize, display: usize, full: bool) -> Snapshot {
    CAPTURE.with(|c| {
        let c = c.borrow();
        Snapshot {
            frames_ms: c.frames.iter().skip(c.frames.len().saturating_sub(frames)).copied().collect(),
            events: c
                .events
                .iter()
                .skip(c.events.len().saturating_sub(events))
                .map(InputEvent::text)
                .collect(),
            active: c.active.iter().map(|(id, p)| (*id, [p.x, p.y])).collect(),
            max_fingers: c.max,
            cancels: c.cancels,
            last_session: if full { c.session.clone() } else { None },
            display_state: c.display_state.clone(),
            display_frames: c
                .display_frames
                .iter()
                .skip(c.display_frames.len().saturating_sub(display))
                .cloned()
                .collect(),
            display_events: if full { c.display_events.iter().cloned().collect() } else { Vec::new() },
        }
    })
}

#[cfg(test)]
mod display_capture_tests {
    use super::*;
    #[test]
    fn stopping_is_explicit_and_preview_does_not_copy_the_full_log() {
        enable(false);
        clear();
        set_persistent(true);
        assert!(!running(), "cross-page preference must not start capture");
        clear();
        assert!(!running(), "clear must not start capture");
        set_running(true);
        CAPTURE.with(|c| {
            let mut c = c.borrow_mut();
            for i in 0..2000 {
                c.frames.push_back(i as f32);
                c.events.push_back(InputEvent {
                    at: i as f64,
                    id: 7,
                    phase: TouchPhase::Moved,
                    position: vec2(i as f32, 20.),
                });
            }
        });
        set_running(false);
        let p = preview();
        assert_eq!(p.frames_ms.len(), 240);
        assert_eq!(p.events.len(), 12);
        assert_eq!(snapshot().events.len(), 2000);
        display_frame(DisplayFrame::default());
        assert!(snapshot().display_frames.is_empty());
        set_running(true);
        assert_eq!(snapshot().events.len(), 2000, "resume preserves existing data");
        clear();
        assert!(running());
        assert!(snapshot().events.is_empty());
        set_persistent(false);
        set_running(false);
    }
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
