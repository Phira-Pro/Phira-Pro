//! Linux frame scheduling on top of the X11/GLX presentation path.
//!
//! The swap chain is compositor-synchronized: with VSync on, `glXSwapBuffers`
//! blocks until the compositor is ready for the next frame, which is the
//! smoothest mode available and needs no help from us. Explicit caps therefore
//! only sleep before update/render; "unlimited" switches VSync off at runtime
//! through the GLX swap-control extension so the loop can present as fast as the
//! GPU allows.

use std::time::{Duration, Instant};

use crate::config::UNLIMITED_FPS;

/// Final precision wait left to the CPU after the bulk sleep.
const SPIN: Duration = Duration::from_micros(150);

pub struct Pacer {
    period: Duration,
    next: Instant,
    swap: SwapInterval,
    applied_interval: Option<i32>,
}

impl Pacer {
    pub fn new() -> Self {
        Self {
            period: Duration::ZERO,
            next: Instant::now(),
            swap: SwapInterval::load(),
            applied_interval: None,
        }
    }

    pub fn wait(&mut self, limit: u16) {
        // Only "unlimited" gives up VSync. "Follow display" and explicit caps
        // keep the compositor-synchronized swap so the phase stays stable.
        let interval = i32::from(limit != UNLIMITED_FPS);
        if self.applied_interval != Some(interval) {
            if self.swap.set_interval(interval) {
                self.applied_interval = Some(interval);
                tracing::info!("Linux present mode: swap interval {interval}");
            }
        }

        let Some(rate) = frame_rate(limit) else {
            if !self.period.is_zero() {
                let mode = if limit == UNLIMITED_FPS { "unlimited" } else { "follow display" };
                tracing::info!("Linux frame pacing: {mode}");
            }
            self.period = Duration::ZERO;
            self.next = Instant::now();
            return;
        };

        let period = Duration::from_secs_f64(1. / rate as f64);
        if self.period != period {
            self.period = period;
            self.next = Instant::now();
            tracing::info!("Linux frame pacing: {rate} FPS");
        }

        let now = Instant::now();
        if let Some(remaining) = self.next.checked_duration_since(now) {
            // Sleep for most of the interval and bound the busy wait; sleeping
            // the whole interval would overshoot by the nanosleep slack.
            let sleep = remaining.saturating_sub(SPIN);
            if !sleep.is_zero() {
                std::thread::sleep(sleep);
            }
            while Instant::now() < self.next {
                std::hint::spin_loop();
            }
        }
        self.next = next_deadline(self.next, Instant::now(), self.period);
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

/// `None` means the present path is already rate-limited, so the pacer must not
/// sleep: "follow display" relies on VSync and "unlimited" deliberately runs
/// free. Explicit caps obey the same 30..=1000 range as the Windows pacer.
fn frame_rate(limit: u16) -> Option<u32> {
    match limit {
        0 | UNLIMITED_FPS => None,
        _ => Some(u32::from(limit).clamp(30, 1000)),
    }
}

fn next_deadline(previous: Instant, now: Instant, period: Duration) -> Instant {
    // Keep phase for normal scheduling jitter. A genuinely missed frame starts
    // a fresh interval; it never causes catch-up bursts or a persistent half cap.
    if now.saturating_duration_since(previous) >= period {
        now + period
    } else {
        previous + period
    }
}

/// Runtime `glXSwapInterval*` access through the already-loaded desktop GL.
///
/// The window backend resolves its swap interval once at context creation and
/// exposes no setter, so the setting could only change with a restart. All GLX
/// swap-control entry points accept the interval without a context argument;
/// `GLX_EXT_swap_control` targets the caller's current drawable and is the
/// preferred variant, with the Mesa and SGI fallbacks covering other drivers.
struct SwapInterval {
    backend: Option<SwapBackend>,
    failed: bool,
}

enum SwapBackend {
    Ext {
        display: GlxGetCurrentDisplay,
        drawable: GlxGetCurrentDrawable,
        set: GlxSwapIntervalExt,
    },
    Mesa(GlxSwapIntervalMes),
    Sgi(GlxSwapIntervalSgi),
}

type GlxGetCurrentDisplay = unsafe extern "C" fn() -> *mut std::ffi::c_void;
type GlxGetCurrentDrawable = unsafe extern "C" fn() -> libc::c_ulong;
type GlxSwapIntervalExt = unsafe extern "C" fn(*mut std::ffi::c_void, libc::c_ulong, std::ffi::c_int);
type GlxSwapIntervalMes = unsafe extern "C" fn(std::ffi::c_int);
type GlxSwapIntervalSgi = unsafe extern "C" fn(std::ffi::c_int);

impl SwapInterval {
    fn load() -> Self {
        let backend = unsafe { load_backend() };
        match &backend {
            Some(SwapBackend::Ext { .. }) => tracing::info!("Linux present mode: GLX_EXT_swap_control"),
            Some(SwapBackend::Mesa(_)) => tracing::info!("Linux present mode: GLX_MESA_swap_control"),
            Some(SwapBackend::Sgi(_)) => tracing::info!("Linux present mode: GLX_SGI_swap_control"),
            None => tracing::warn!(
                "Linux present mode: live swap interval control unavailable; frame rate changes apply after restart"
            ),
        }
        Self { backend, failed: false }
    }

    /// Returns whether the interval was applied; a permanently unavailable
    /// backend is remembered so the setting is not retried every frame.
    fn set_interval(&mut self, interval: i32) -> bool {
        if self.failed {
            return false;
        }
        let applied = match &self.backend {
            Some(SwapBackend::Ext { display, drawable, set }) => unsafe {
                let display = display();
                let drawable = drawable();
                if display.is_null() || drawable == 0 {
                    false
                } else {
                    set(display, drawable, interval);
                    true
                }
            },
            Some(SwapBackend::Mesa(set)) => unsafe {
                set(interval);
                true
            },
            Some(SwapBackend::Sgi(set)) => unsafe {
                set(interval);
                true
            },
            None => false,
        };
        if !applied {
            self.failed = true;
            tracing::warn!("Linux present mode: cannot change swap interval at runtime; restart to apply the new limit");
        }
        applied
    }
}

unsafe fn load_backend() -> Option<SwapBackend> {
    // The window backend already loaded a GL library; this handle just resolves
    // the same exported entry points for runtime interval changes.
    let mut handle = std::ptr::null_mut();
    for name in ["libGL.so.1", "libGL.so", "libGLX.so.0"] {
        let Ok(name) = std::ffi::CString::new(name) else {
            continue;
        };
        handle = libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if !handle.is_null() {
            break;
        }
    }
    if handle.is_null() {
        return None;
    }
    let get_proc = symbol(handle, "glXGetProcAddressARB").or_else(|| symbol(handle, "glXGetProcAddress"))?;
    let display = symbol(handle, "glXGetCurrentDisplay")?;
    let drawable = symbol(handle, "glXGetCurrentDrawable")?;

    let get_proc: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_void = std::mem::transmute(get_proc);
    if let Some(set) = extension(get_proc, "glXSwapIntervalEXT") {
        return Some(SwapBackend::Ext {
            display: std::mem::transmute(display),
            drawable: std::mem::transmute(drawable),
            set: std::mem::transmute(set),
        });
    }
    if let Some(set) = symbol(handle, "glXSwapIntervalMESA").or_else(|| extension(get_proc, "glXSwapIntervalMESA")) {
        return Some(SwapBackend::Mesa(std::mem::transmute(set)));
    }
    if let Some(set) = symbol(handle, "glXSwapIntervalSGI").or_else(|| extension(get_proc, "glXSwapIntervalSGI")) {
        return Some(SwapBackend::Sgi(std::mem::transmute(set)));
    }
    None
}

unsafe fn symbol(handle: *mut std::ffi::c_void, name: &str) -> Option<*mut std::ffi::c_void> {
    let name = std::ffi::CString::new(name).ok()?;
    let symbol = libc::dlsym(handle, name.as_ptr());
    (!symbol.is_null()).then_some(symbol)
}

unsafe fn extension(
    get_proc: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_void,
    name: &str,
) -> Option<*mut std::ffi::c_void> {
    let name = std::ffi::CString::new(name).ok()?;
    let symbol = get_proc(name.as_ptr());
    (!symbol.is_null()).then_some(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UNLIMITED_FPS;

    #[test]
    fn follow_display_and_unlimited_never_pace() {
        assert_eq!(frame_rate(0), None);
        assert_eq!(frame_rate(UNLIMITED_FPS), None);
        assert_eq!(frame_rate(60), Some(60));
        assert_eq!(frame_rate(144), Some(144));
        assert_eq!(frame_rate(10), Some(30));
        assert_eq!(frame_rate(2000), Some(1000));
    }

    #[test]
    fn jitter_and_missed_frames_do_not_halve_the_target_or_create_bursts() {
        let start = Instant::now();
        let period = Duration::from_secs_f64(1. / 60.);
        assert_eq!(next_deadline(start, start + Duration::from_micros(100), period), start + period);
        let late = start + period * 3;
        assert_eq!(next_deadline(start, late, period), late + period);
        assert_eq!(next_deadline(start + period, start + period, period), start + period * 2);
    }
}
