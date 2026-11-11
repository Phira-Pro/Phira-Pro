//! Bounded, latest-frame transport. Rendering remains on the game's GL thread.
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Request {
    Start { snapshot: String, width: u32, height: u32 },
    Seek { id: String, time: f64, #[serde(rename = "showHud")] show_hud: bool },
    Stop { id: String },
}
pub struct PreviewStart {
    pub id: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Default, PartialEq)]
pub struct PreviewPosition { pub time: f64, pub show_hud: bool }
#[derive(Default)]
struct State {
    id: String,
    start: Option<PreviewStart>,
    position: PreviewPosition,
    frame: Vec<u8>,
    revision: u32,
    error: Option<String>,
}
#[derive(Clone, Default)]
pub struct PreviewBridge(Arc<Mutex<State>>);
impl PreviewBridge {
    pub(crate) fn start(&self, bytes: Vec<u8>, width: u32, height: u32) -> Result<Value> {
        ensure!((64..=1280).contains(&width) && (64..=720).contains(&height) && width % 2 == 0 && height % 2 == 0, "Invalid preview size");
        let id = uuid::Uuid::new_v4().to_string();
        *self.0.lock().unwrap() = State { id: id.clone(), start: Some(PreviewStart { id: id.clone(), bytes, width, height }), ..State::default() };
        Ok(json!({"ok":true,"id":id}))
    }
    pub(crate) fn seek(&self, id: &str, time: f64, show_hud: bool) -> Result<Value> {
        ensure!(time.is_finite() && (0. ..=86400.).contains(&time), "Invalid preview time");
        let mut s = self.0.lock().unwrap();
        ensure!(s.id == id && !id.is_empty(), "Expired preview");
        s.position = PreviewPosition { time, show_hud };
        Ok(json!({"ok":true,"revision":s.revision,"error":s.error,"ready":!s.frame.is_empty()}))
    }
    pub fn stop(&self, id: &str) { let mut s = self.0.lock().unwrap(); if s.id == id { *s = State::default(); } }
    pub fn take_start(&self) -> Option<PreviewStart> { self.0.lock().unwrap().start.take() }
    pub fn position(&self, id: &str) -> Option<PreviewPosition> { let s = self.0.lock().unwrap(); (s.id == id && !id.is_empty()).then_some(s.position) }
    pub fn fail(&self, id: &str, error: String) { let mut s = self.0.lock().unwrap(); if s.id == id { s.error = Some(error); s.frame.clear(); } }
    pub fn publish(&self, id: &str, width: u32, height: u32, rgba: &[u8]) {
        if rgba.len() != width as usize * height as usize * 4 { return; }
        let mut s = self.0.lock().unwrap();
        if s.id != id { return; }
        s.revision = s.revision.wrapping_add(1);
        let revision = s.revision;
        s.frame.clear();
        s.frame.extend_from_slice(b"PRPV");
        for value in [width, height, revision] { s.frame.extend_from_slice(&value.to_le_bytes()); }
        s.frame.extend_from_slice(rgba);
    }
    pub(crate) fn frame(&self) -> Vec<u8> { self.0.lock().unwrap().frame.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_session_rejects_stale_frames_and_commands() {
        let bridge = PreviewBridge::default();
        assert!(bridge.start(vec![], 4096, 720).is_err());
        let a = bridge.start(vec![1], 64, 64).unwrap()["id"].as_str().unwrap().to_owned();
        let b = bridge.start(vec![2], 64, 64).unwrap()["id"].as_str().unwrap().to_owned();
        assert_eq!(bridge.take_start().unwrap().bytes, vec![2]);
        assert!(bridge.take_start().is_none());
        assert!(bridge.seek(&a, 0., false).is_err());
        assert!(bridge.seek(&b, f64::NAN, false).is_err());
        bridge.publish(&a, 64, 64, &vec![0; 64 * 64 * 4]);
        assert!(bridge.frame().is_empty());
        bridge.publish(&b, 64, 64, &vec![1; 64 * 64 * 4]);
        assert_eq!(&bridge.frame()[..4], b"PRPV");
        bridge.stop(&a);
        assert!(bridge.position(&b).is_some());
        bridge.stop(&b);
        assert!(bridge.frame().is_empty());
    }
}
