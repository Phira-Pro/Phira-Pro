//! Own one public CADisplayLink, renewing its scheduling state after suspension.
use crate::{
    diagnostics::{self, DisplayFrame, DisplayState},
    display_timing::Recovery,
};
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyClass, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSProcessInfo, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use objc2_ui_kit::{
    UIAccessibilityGuidedAccessStatusDidChangeNotification, UIAccessibilityIsGuidedAccessEnabled, UIApplication,
    UIApplicationDidBecomeActiveNotification, UIApplicationDidEnterBackgroundNotification, UIApplicationState,
    UIApplicationWillEnterForegroundNotification, UIApplicationWillResignActiveNotification, UIScreen, UIView, UIViewController,
    UIWindowDidBecomeKeyNotification,
};
use std::{
    cell::{Cell, RefCell},
    sync::atomic::{AtomicU16, Ordering},
    time::Instant,
};
static REQUESTED: AtomicU16 = AtomicU16::new(120);

/// Left, top, right, bottom in the Ui coordinate system (screen width = 2).
pub fn safe_margins() -> [f32; 4] {
    let Some(mtm) = MainThreadMarker::new() else { return [0.; 4] };
    #[allow(deprecated)]
    let Some(window) = UIApplication::sharedApplication(mtm).keyWindow() else {
        return [0.; 4];
    };
    let w = window.bounds().size.width.max(1.);
    let i = window.safeAreaInsets();
    [i.left, i.top, i.right, i.bottom].map(|v| (v * 2. / w) as f32)
}

pub fn set_requested(rate: u16) {
    REQUESTED.store(rate.max(1), Ordering::Relaxed);
    install();
    DRIVER.with(|driver| {
        if let Some(d) = driver.borrow_mut().as_mut() {
            // Reassigning the same range to a stale link was ineffective in #2.
            d.rebuild("refresh-rate request", true);
        }
    });
}

fn maximum(mtm: MainThreadMarker) -> u16 {
    #[allow(deprecated)]
    let maximum = UIScreen::mainScreen(mtm).maximumFramesPerSecond().clamp(1, u16::MAX as isize) as u16;
    crate::ui::MAX_DEVICE_REFRESH_RATE.store((maximum as f32).to_bits(), Ordering::Relaxed);
    maximum
}

struct Driver {
    link: Retained<CADisplayLink>,
    target: Retained<FrameTarget>,
    controller: Retained<UIViewController>,
    recovery: Recovery,
    state: DisplayState,
    last_state_sample: f64,
}
impl Driver {
    fn now(&self) -> f64 {
        self.target.ivars().origin.elapsed().as_secs_f64()
    }
    fn sample_state(&mut self, event: Option<&str>) {
        let previous = (self.state.low_power, self.state.thermal_state, self.state.guided_access);
        let mtm = self.target.mtm();
        self.state.maximum_hz = maximum(mtm);
        self.state.requested_hz = REQUESTED.load(Ordering::Relaxed).min(self.state.maximum_hz).max(1);
        self.state.application_state = UIApplication::sharedApplication(mtm).applicationState().0;
        self.state.guided_access = UIAccessibilityIsGuidedAccessEnabled();
        let process = NSProcessInfo::processInfo();
        // Public APIs since iOS 11 / 9, below our iOS 12 minimum.
        self.state.low_power = process.isLowPowerModeEnabled();
        self.state.thermal_state = process.thermalState().0;
        self.state.paused = self.link.isPaused();
        self.state.recovery_attempts = self.recovery.attempts;
        self.state.preferred_range = if self.link.respondsToSelector(sel!(preferredFrameRateRange)) {
            let r = self.link.preferredFrameRateRange();
            [r.minimum, r.maximum, r.preferred]
        } else {
            #[allow(deprecated)]
            let hz = self.link.preferredFramesPerSecond() as f32;
            [hz; 3]
        };
        let now = self.now();
        self.last_state_sample = now;
        let changed = previous != (self.state.low_power, self.state.thermal_state, self.state.guided_access);
        diagnostics::display_state(
            now,
            event.or(if changed {
                Some("system power/thermal/accessibility state changed")
            } else {
                None
            }),
            self.state.clone(),
        );
    }
    fn rebuild(&mut self, reason: &str, rearm: bool) {
        let paused = self.state.paused || UIApplication::sharedApplication(self.target.mtm()).applicationState() != UIApplicationState::Active;
        // Invalidate first so the old link cannot survive in the run loop or
        // keep rendering alongside its replacement. Retain the observer target.
        self.link.invalidate();
        let hz = REQUESTED.load(Ordering::Relaxed).min(maximum(self.target.mtm())).max(1);
        self.link = make_link(&self.target, hz, paused);
        self.state.generation += 1;
        self.target.ivars().last_timestamp.set(None);
        self.target.ivars().last_arrival.set(None);
        self.recovery.reset(self.now(), rearm);
        own_controller(&self.controller, Some(hz));
        self.sample_state(Some(reason));
    }
}
fn own_controller(controller: &UIViewController, hz: Option<u16>) {
    // UIKit may restart GLKViewController's private link. Keep one render loop.
    unsafe {
        if let Some(hz) = hz {
            let _: () = msg_send![controller, setPreferredFramesPerSecond: hz as isize];
        }
        let paused: bool = msg_send![controller, isPaused];
        if !paused {
            let _: () = msg_send![controller, setPaused: true];
        }
    }
}
fn make_link(target: &FrameTarget, hz: u16, paused: bool) -> Retained<CADisplayLink> {
    // SAFETY: Driver and the link retain target; selector is defined below.
    let link = unsafe { CADisplayLink::displayLinkWithTarget_selector(target, sel!(renderFrame:)) };
    link.setPaused(paused);
    configure_link(&link, hz);
    // SAFETY: Driver and all selector callbacks stay on the main thread.
    unsafe {
        link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
    }
    link
}
fn configure_link(link: &CADisplayLink, hz: u16) {
    if link.respondsToSelector(sel!(setPreferredFrameRateRange:)) {
        link.setPreferredFrameRateRange(CAFrameRateRange {
            minimum: hz as f32,
            maximum: hz as f32,
            preferred: hz as f32,
        });
    } else {
        #[allow(deprecated)]
        link.setPreferredFramesPerSecond(hz as isize);
    }
}
thread_local! { static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) }; }

struct FrameTargetIvars {
    view: Retained<UIView>,
    origin: Instant,
    last_timestamp: Cell<Option<f64>>,
    last_arrival: Cell<Option<f64>>,
}
define_class!(
    // SAFETY: NSObject imposes no subclass requirements. These UIKit
    // notifications and display-link callbacks run on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = FrameTargetIvars]
    struct FrameTarget;
    unsafe impl NSObjectProtocol for FrameTarget {}
    impl FrameTarget {
        #[unsafe(method(renderFrame:))]
        fn render_frame(&self, link: &CADisplayLink) {
            if UIApplication::sharedApplication(self.mtm()).applicationState() != UIApplicationState::Active { return; }
            let start = Instant::now();
            let now = self.ivars().origin.elapsed().as_secs_f64();
            // CFTimeInterval is double; msg_send avoids an extra framework
            // dependency just for the type alias.
            let timestamp: f64 = unsafe { msg_send![link, timestamp] };
            let target_timestamp: f64 = unsafe { msg_send![link, targetTimestamp] };
            let interval = self.ivars().last_timestamp.replace(Some(timestamp)).map(|previous| timestamp - previous);
            let arrival = self.ivars().last_arrival.replace(Some(now)).map(|previous| now - previous);
            let generation = DRIVER.with(|driver| {
                let mut driver = driver.borrow_mut();
                let Some(d) = driver.as_mut() else { return 0 };
                own_controller(&d.controller, None);
                // 1 Hz state polling keeps power/thermal recovery guards current.
                if now - d.last_state_sample >= 1. { d.sample_state(None); }
                d.state.generation
            });
            let display_start = Instant::now();
            // No DRIVER borrow across display: it invokes the game, which may
            // change refresh-rate settings or application state.
            unsafe { let _: () = msg_send![&self.ivars().view, display]; }
            let display_work = display_start.elapsed().as_secs_f64();
            let work = start.elapsed().as_secs_f64();
            diagnostics::display_frame(DisplayFrame {
                at_seconds: now, timestamp, target_timestamp,
                callback_interval_ms: interval.filter(|v| v.is_finite() && *v > 0.).map(|v| v * 1000.),
                arrival_interval_ms: arrival.filter(|v| v.is_finite() && *v > 0.).map(|v| v * 1000.),
                callback_work_ms: work * 1000., glk_display_ms: display_work * 1000., generation,
            });
            DRIVER.with(|driver| {
                let mut driver = driver.borrow_mut();
                let Some(d) = driver.as_mut() else { return };
                if d.state.generation != generation || d.state.paused { return; }
                if d.recovery.observe(now, interval.unwrap_or(0.), work, d.state.requested_hz,
                    d.state.low_power || d.state.thermal_state >= 2) {
                    d.rebuild("low callback rate: bounded recovery", false);
                }
            });
        }
        #[unsafe(method(lifecycleChanged:))]
        fn lifecycle_changed(&self, notification: &NSNotification) {
            let name = notification.name();
            let event = name.to_string();
            // A host such as LiveContainer may forward notifications without
            // forwarding miniquad's application-delegate callback.
            unsafe {
                if &*name == UIApplicationWillResignActiveNotification || &*name == UIApplicationDidEnterBackgroundNotification {
                    set_paused(true);
                } else if &*name == UIApplicationDidBecomeActiveNotification {
                    set_paused(false);
                } else if &*name == UIWindowDidBecomeKeyNotification {
                    // Ignore keyboard/other windows becoming key. iPad slide-over
                    // can restore our key window without an application transition.
                    if let (Some(object), Some(window)) = (notification.object(), self.ivars().view.window()) {
                        if Retained::as_ptr(&object).cast::<()>() == Retained::as_ptr(&window).cast::<()>() {
                            set_paused(false);
                        }
                    }
                }
            }
            DRIVER.with(|driver| {
                if let Some(d) = driver.borrow_mut().as_mut() {
                    unsafe {
                        if &*name == UIAccessibilityGuidedAccessStatusDidChangeNotification {
                            d.rebuild("guided-access status changed", true);
                        }
                    }
                    d.sample_state(Some(&event));
                }
            });
        }
    }
);

/// Called after miniquad has created its window, from the first GL callback.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    DRIVER.with(|driver| {
        if driver.borrow().is_some() {
            return;
        }
        #[allow(deprecated)]
        let Some(window) = UIApplication::sharedApplication(mtm).keyWindow() else {
            return;
        };
        let Some(controller) = window.rootViewController() else { return };
        let Some(view) = controller.view() else { return };
        let Some(view_class) = AnyClass::get(c"GLKView") else { return };
        let Some(controller_class) = AnyClass::get(c"GLKViewController") else {
            return;
        };
        if !view.isKindOfClass(view_class) || !controller.isKindOfClass(controller_class) {
            return;
        }
        let target = FrameTarget::alloc(mtm).set_ivars(FrameTargetIvars {
            view,
            origin: Instant::now(),
            last_timestamp: Cell::new(None),
            last_arrival: Cell::new(None),
        });
        let target: Retained<FrameTarget> = unsafe { msg_send![super(target), init] };
        let hz = REQUESTED.load(Ordering::Relaxed).min(maximum(mtm)).max(1);
        let paused = UIApplication::sharedApplication(mtm).applicationState() != UIApplicationState::Active;
        let link = make_link(&target, hz, paused);
        own_controller(&controller, Some(hz));
        // SAFETY: valid NSNotification selector; target is retained by Driver
        // for the process lifetime, so observer references cannot dangle.
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            for name in [
                UIApplicationWillResignActiveNotification,
                UIApplicationDidEnterBackgroundNotification,
                UIApplicationWillEnterForegroundNotification,
                UIApplicationDidBecomeActiveNotification,
                UIAccessibilityGuidedAccessStatusDidChangeNotification,
                UIWindowDidBecomeKeyNotification,
            ] {
                center.addObserver_selector_name_object(&target, sel!(lifecycleChanged:), Some(name), None);
            }
        }
        let mut d = Driver {
            link,
            target,
            controller,
            recovery: Recovery::default(),
            state: DisplayState {
                generation: 1,
                ..DisplayState::default()
            },
            last_state_sample: f64::NEG_INFINITY,
        };
        d.sample_state(Some("display link installed"));
        *driver.borrow_mut() = Some(d);
    });
}

/// Duplicate active notifications are idempotent and cannot add render loops.
pub fn set_paused(paused: bool) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    DRIVER.with(|driver| {
        if let Some(d) = driver.borrow_mut().as_mut() {
            if paused {
                d.link.setPaused(true);
                d.state.paused = true;
                d.sample_state(Some("pause callback"));
            } else if d.state.paused {
                d.state.paused = false;
                d.rebuild("resume: new display link", true);
            } else {
                own_controller(&d.controller, None);
                let hz = REQUESTED.load(Ordering::Relaxed).min(maximum(d.target.mtm())).max(1);
                configure_link(&d.link, hz);
                // Re-arm after overlays/control center as well as full background
                // transitions; a previous exhausted recovery budget is not sticky.
                d.recovery.reset(d.now(), true);
                d.sample_state(Some("focus restored: refresh request renewed"));
            }
        }
    });
}
