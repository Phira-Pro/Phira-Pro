//! Bounded, observer-only input tap. Never feeds or changes the judgement path.
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

pub const FRAME_LIMIT: usize = 16_384;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub source: String,
    pub phase: String,
    pub id: u64,
    /// Backend event time in the uptime clock, when actually supplied.
    pub event_time: Option<f64>,
    /// Time the input subscriber observed this event, not hardware arrival time.
    pub observed_time: f64,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Clone, Default)]
pub struct Frame {
    pub serial: u64,
    pub inputs: Vec<Input>,
    pub dropped: u64,
}

#[derive(Default)]
struct State {
    enabled: bool,
    owners: usize,
    frame: Frame,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

/// Scoped to a GameScene recorder closure, so menus do not retain the input tap.
pub struct Observer {
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Observer {
    pub fn new() -> Self {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.owners += 1;
            s.enabled = true;
        });
        Self {
            _not_send: std::marker::PhantomData,
        }
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.owners = s.owners.saturating_sub(1);
            if s.owners == 0 {
                s.enabled = false;
                s.frame.inputs.clear();
            }
        });
    }
}

#[cfg(test)]
fn enable(enabled: bool) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.enabled = enabled;
        state.frame.inputs.clear();
        state.frame.dropped = 0;
    });
}

pub fn begin_frame() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.frame.serial = state.frame.serial.wrapping_add(1);
        state.frame.inputs.clear();
        state.frame.dropped = 0;
    });
}

pub fn push(source: &str, phase: &str, id: u64, event_time: f64, position: Option<(f32, f32)>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.enabled {
            return;
        }
        if state.frame.inputs.len() >= FRAME_LIMIT {
            state.frame.dropped = state.frame.dropped.saturating_add(1);
            return;
        }
        state.frame.inputs.push(Input {
            source: source.into(),
            phase: phase.into(),
            id,
            event_time: event_time.is_finite().then_some(event_time),
            observed_time: crate::judge::get_uptime(),
            x: position.map(|p| p.0),
            y: position.map(|p| p.1),
        });
    });
}

pub fn snapshot() -> Frame {
    STATE.with(|state| state.borrow().frame.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observes_order_and_cancellation_without_fabricating_backend_timestamps() {
        enable(true);
        begin_frame();
        push("touch", "down", 7, 1., Some((2., 3.)));
        push("touch", "cancel", 7, 1.001, Some((2., 3.)));
        push("keyboard", "down", 2, f64::NEG_INFINITY, None);
        let frame = snapshot();
        assert_eq!(frame.inputs.iter().map(|e| e.phase.as_str()).collect::<Vec<_>>(), ["down", "cancel", "down"]);
        assert_eq!(frame.inputs[2].event_time, None);
        for _ in 0..FRAME_LIMIT {
            push("touch", "move", 7, 2., None);
        }
        assert_eq!(snapshot().inputs.len(), FRAME_LIMIT);
        assert_eq!(snapshot().dropped, 3);
        enable(false);
        begin_frame();
        push("touch", "down", 7, 3., None);
        assert!(snapshot().inputs.is_empty());
    }
}
