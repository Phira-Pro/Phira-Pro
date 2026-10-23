//! Multiplayer owns its protocol, session and room state. UI is only a consumer.
pub mod game_bridge;
pub mod overlay;
pub mod panel;
pub mod protocol;
mod recovery;
pub mod resources;
pub mod room;
pub mod session;
mod view;
