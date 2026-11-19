//! Linux frame scheduling on top of the X11/GLX presentation path.
//!
//! The swap chain is compositor-synchronized: with VSync on, `glXSwapBuffers`
//! blocks until the compositor is ready for the next frame, which is the
//! smoothest mode available and needs no help from us. Explicit caps therefore
//! only sleep before update/render; "unlimited" switches VSync off at runtime
//! through the GLX swap-control extension so the loop can present as fast as the
//! GPU allows.

use std::{
    ffi::{c_char, c_int, c_uint, c_void, CStr},
    time::{Duration, Instant},
};

use crate::config::UNLIMITED_FPS;

/// Final precision wait left to the CPU after the bulk sleep.
const SPIN: Duration = Duration::from_micros(150);

pub struct Pacer {
    period: Duration,
    next: Instant,
    swap: Option<SwapInterval>,
    requested_interval: Option<i32>,
}

impl Pacer {
    pub fn new() -> Self {
        Self {
            period: Duration::ZERO,
            next: Instant::now(),
            swap: SwapInterval::load(),
            requested_interval: None,
        }
    }

    pub fn wait(&mut self, limit: u16) {
        // Only "unlimited" gives up VSync. "Follow display" and explicit caps
        // keep the compositor-synchronized swap so the phase stays stable.
        let interval = i32::from(limit != UNLIMITED_FPS);
        if self.requested_interval != Some(interval) {
            let first_request = self.requested_interval.replace(interval).is_none();
            match self.swap.as_ref().is_some_and(|swap| swap.set_interval(interval)) {
                true => tracing::debug!("Linux present mode: swap interval {interval}"),
                false if !first_request || interval == 0 => {
                    tracing::warn!("Linux present mode: driver could not apply swap interval {interval}");
                }
                false => {} // Keep the window backend's initial interval.
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

type Display = *mut c_void;
type Drawable = libc::c_ulong;
type GetProc = unsafe extern "C" fn(*const u8) -> *mut c_void;
type GetDrawable = unsafe extern "C" fn() -> Drawable;
type SwapExt = unsafe extern "C" fn(Display, Drawable, c_int);
type QueryDrawable = unsafe extern "C" fn(Display, Drawable, c_int, *mut c_uint);
type SwapMesa = unsafe extern "C" fn(c_uint) -> c_int;
type SwapSgi = unsafe extern "C" fn(c_int) -> c_int;

const GLX_SCREEN: c_int = 0x800C;
const GLX_SWAP_INTERVAL_EXT: c_int = 0x20F1;

struct GlLibrary(*mut c_void);

impl GlLibrary {
    fn open() -> Option<Self> {
        for name in [c"libGL.so.1", c"libGL.so", c"libGLX.so.0"] {
            let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if !handle.is_null() {
                return Some(Self(handle));
            }
        }
        None
    }

    fn symbol(&self, name: &CStr) -> Option<*mut c_void> {
        let address = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        (!address.is_null()).then_some(address)
    }
}

impl Drop for GlLibrary {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}

/// Runtime GLX swap control. A proc address alone does not mean the active
/// screen supports that extension (GLVND also exports dispatch stubs).
struct SwapInterval {
    _library: GlLibrary,
    display: Display,
    drawable: GetDrawable,
    ext: Option<(SwapExt, QueryDrawable)>,
    mesa: Option<SwapMesa>,
    sgi: Option<SwapSgi>,
}

fn has_extension(extensions: &[u8], name: &[u8]) -> bool {
    extensions.split(|byte| byte.is_ascii_whitespace()).any(|ext| ext == name)
}

impl SwapInterval {
    fn load() -> Option<Self> {
        let library = GlLibrary::open()?;
        // The caller creates the pacer on the render thread with a current context.
        unsafe {
            let get_proc: GetProc = std::mem::transmute(library.symbol(c"glXGetProcAddressARB").or_else(|| library.symbol(c"glXGetProcAddress"))?);
            let get_display: unsafe extern "C" fn() -> Display = std::mem::transmute(library.symbol(c"glXGetCurrentDisplay")?);
            let get_context: unsafe extern "C" fn() -> *mut c_void = std::mem::transmute(library.symbol(c"glXGetCurrentContext")?);
            let drawable: GetDrawable = std::mem::transmute(library.symbol(c"glXGetCurrentDrawable")?);
            let query_context: unsafe extern "C" fn(Display, *mut c_void, c_int, *mut c_int) -> c_int =
                std::mem::transmute(library.symbol(c"glXQueryContext")?);
            let query_extensions: unsafe extern "C" fn(Display, c_int) -> *const c_char =
                std::mem::transmute(library.symbol(c"glXQueryExtensionsString")?);
            let display = get_display();
            let context = get_context();
            let mut screen = 0;
            if display.is_null() || context.is_null() || query_context(display, context, GLX_SCREEN, &mut screen) != 0 {
                return None;
            }
            let extensions = query_extensions(display, screen);
            if extensions.is_null() {
                return None;
            }
            let extensions = CStr::from_ptr(extensions).to_bytes();
            let resolve = |extension: &[u8], symbol: &CStr| {
                if !has_extension(extensions, extension) {
                    return None;
                }
                let address = get_proc(symbol.as_ptr().cast());
                (!address.is_null()).then_some(address)
            };
            let ext = resolve(b"GLX_EXT_swap_control", c"glXSwapIntervalEXT")
                .zip(library.symbol(c"glXQueryDrawable"))
                .map(|(set, query)| (std::mem::transmute::<_, SwapExt>(set), std::mem::transmute::<_, QueryDrawable>(query)));
            let mesa = resolve(b"GLX_MESA_swap_control", c"glXSwapIntervalMESA").map(|f| std::mem::transmute::<_, SwapMesa>(f));
            let sgi = resolve(b"GLX_SGI_swap_control", c"glXSwapIntervalSGI").map(|f| std::mem::transmute::<_, SwapSgi>(f));
            Some(Self {
                _library: library,
                display,
                drawable,
                ext,
                mesa,
                sgi,
            })
        }
    }

    fn set_interval(&self, interval: c_int) -> bool {
        unsafe {
            let drawable = (self.drawable)();
            if drawable == 0 {
                return false;
            }
            if let Some((set, query)) = self.ext {
                set(self.display, drawable, interval);
                let mut current = c_uint::MAX;
                query(self.display, drawable, GLX_SWAP_INTERVAL_EXT, &mut current);
                if current == interval as c_uint {
                    return true;
                }
            }
            if self.mesa.is_some_and(|set| set(interval as c_uint) == 0) {
                return true;
            }
            // SGI rejects zero and therefore cannot implement unlimited mode.
            interval > 0 && self.sgi.is_some_and(|set| set(interval) == 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UNLIMITED_FPS;

    #[test]
    fn only_matches_the_advertised_extension_name() {
        let extensions = b"GLX_EXT_swap_control_tear GLX_MESA_swap_control";
        assert!(!has_extension(extensions, b"GLX_EXT_swap_control"));
        assert!(has_extension(extensions, b"GLX_MESA_swap_control"));
    }

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
