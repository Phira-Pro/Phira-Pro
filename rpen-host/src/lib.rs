//! Local editor boundary. No dependency on the game, accounts, plugins or scores.
mod server;
mod storage;
mod library;
mod linked;
mod preview;
mod audio;
pub use preview::{PreviewBridge, PreviewPosition, PreviewStart};

pub use server::{EditorEvent, EditorHost, PlayRequest};
pub use library::{LibraryReply, LibraryRequest};

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "ios")]
pub mod ios;
