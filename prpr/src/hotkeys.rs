//! Saved keyboard bindings shared by settings and gameplay.
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action { Hit, Pause, Backward, Forward, Exit, Disabled }
impl Action {
    pub const ALL: [Self; 6] = [Self::Hit, Self::Pause, Self::Backward, Self::Forward, Self::Exit, Self::Disabled];
    pub fn label(self) -> &'static str {
        match self { Self::Hit => "音符判定", Self::Pause => "暂停或继续", Self::Backward => "后退 1 秒", Self::Forward => "前进 5 秒", Self::Exit => "退出游玩", Self::Disabled => "禁用" }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings { pub overrides: BTreeMap<u16, Action> }
impl Bindings {
    pub fn action(&self, key: KeyCode) -> Action {
        self.overrides.get(&(key as u16)).copied().unwrap_or_else(|| Self::default_action(key))
    }
    pub fn default_action(key: KeyCode) -> Action {
        match key { KeyCode::Space => Action::Pause, KeyCode::Left => Action::Backward, KeyCode::Right => Action::Forward, KeyCode::Q => Action::Exit, _ => Action::Hit }
    }
    pub fn pressed(&self, action: Action) -> bool { pressed_keys().iter().any(|key| self.action(*key) == action) }
    pub fn set(&mut self, key: KeyCode, action: Action) {
        if action == Self::default_action(key) { self.overrides.remove(&(key as u16)); }
        else { self.overrides.insert(key as u16, action); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_overrides_deletion_and_restore() {
        assert!(KEYS.len() > 100);
        let mut b = Bindings::default();
        assert_eq!(b.action(KeyCode::Space), Action::Pause);
        b.set(KeyCode::Space, Action::Hit);
        b.set(KeyCode::A, Action::Pause);
        assert_eq!(b.action(KeyCode::Space), Action::Hit);
        assert_eq!(b.action(KeyCode::A), Action::Pause);
        b.set(KeyCode::A, Action::Disabled);
        let saved = serde_json::to_string(&b).unwrap();
        let loaded: Bindings = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.action(KeyCode::A), Action::Disabled);
        b.set(KeyCode::Space, Action::Pause);
        assert!(!b.overrides.contains_key(&(KeyCode::Space as u16)));
        b.overrides.clear();
        assert_eq!(b.action(KeyCode::A), Action::Hit);
    }
}

/// All named native keys, including keys outside the visual ANSI layout.
pub const KEYS: &[KeyCode] = &[
    KeyCode::Space,
    KeyCode::Apostrophe,
    KeyCode::Comma,
    KeyCode::Minus,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::Key0,
    KeyCode::Key1,
    KeyCode::Key2,
    KeyCode::Key3,
    KeyCode::Key4,
    KeyCode::Key5,
    KeyCode::Key6,
    KeyCode::Key7,
    KeyCode::Key8,
    KeyCode::Key9,
    KeyCode::Semicolon,
    KeyCode::Equal,
    KeyCode::A,
    KeyCode::B,
    KeyCode::C,
    KeyCode::D,
    KeyCode::E,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::I,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::M,
    KeyCode::N,
    KeyCode::O,
    KeyCode::P,
    KeyCode::Q,
    KeyCode::R,
    KeyCode::S,
    KeyCode::T,
    KeyCode::U,
    KeyCode::V,
    KeyCode::W,
    KeyCode::X,
    KeyCode::Y,
    KeyCode::Z,
    KeyCode::LeftBracket,
    KeyCode::Backslash,
    KeyCode::RightBracket,
    KeyCode::GraveAccent,
    KeyCode::World1,
    KeyCode::World2,
    KeyCode::Escape,
    KeyCode::Enter,
    KeyCode::Tab,
    KeyCode::Backspace,
    KeyCode::Insert,
    KeyCode::Delete,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::CapsLock,
    KeyCode::ScrollLock,
    KeyCode::NumLock,
    KeyCode::PrintScreen,
    KeyCode::Pause,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
    KeyCode::F13,
    KeyCode::F14,
    KeyCode::F15,
    KeyCode::F16,
    KeyCode::F17,
    KeyCode::F18,
    KeyCode::F19,
    KeyCode::F20,
    KeyCode::F21,
    KeyCode::F22,
    KeyCode::F23,
    KeyCode::F24,
    KeyCode::F25,
    KeyCode::Kp0,
    KeyCode::Kp1,
    KeyCode::Kp2,
    KeyCode::Kp3,
    KeyCode::Kp4,
    KeyCode::Kp5,
    KeyCode::Kp6,
    KeyCode::Kp7,
    KeyCode::Kp8,
    KeyCode::Kp9,
    KeyCode::KpDecimal,
    KeyCode::KpDivide,
    KeyCode::KpMultiply,
    KeyCode::KpSubtract,
    KeyCode::KpAdd,
    KeyCode::KpEnter,
    KeyCode::KpEqual,
    KeyCode::LeftShift,
    KeyCode::LeftControl,
    KeyCode::LeftAlt,
    KeyCode::LeftSuper,
    KeyCode::RightShift,
    KeyCode::RightControl,
    KeyCode::RightAlt,
    KeyCode::RightSuper,
    KeyCode::Menu,
];
pub fn pressed_keys() -> Vec<KeyCode> { KEYS.iter().copied().filter(|key| is_key_pressed(*key)).collect() }
