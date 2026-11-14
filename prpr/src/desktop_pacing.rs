//! Window-local desktop frame scheduling. No global timer-resolution changes.
//!
//! Both desktop implementations expose the same `Pacer::wait(limit)` contract
//! driven by `config.desktop_fps_limit`: `0` follows the display, `UNLIMITED_FPS`
//! skips pacing, and anything else caps the frame rate.
//!
//! Windows always swaps with VSync disabled because the composed swap can
//! quantize presents, so the pacer owns the cadence there.
//!
//! Linux keeps the compositor-synchronized swap for "follow display" and for
//! explicit caps (the X11/GLX path is the smoothest presentation), and only
//! disables VSync for "unlimited" via `GLX_EXT_swap_control`. Explicit caps
//! sleep inside the pacer, which keeps the visible animation unchanged while
//! letting weak GPUs idle part of every frame and saving power.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::Pacer;
#[cfg(target_os = "windows")]
pub use windows::Pacer;
