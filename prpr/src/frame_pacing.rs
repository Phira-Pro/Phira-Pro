//! Keep miniquad's upstream GLKViewController as the sole presentation driver.
use crate::{diagnostics::{self, DisplayFrame, DisplayState}, display_timing::Recovery};
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyClass, sel, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSProcessInfo};
use objc2_ui_kit::{UIAccessibilityGuidedAccessStatusDidChangeNotification, UIAccessibilityIsGuidedAccessEnabled,
    UIApplication, UIApplicationDidBecomeActiveNotification, UIApplicationDidEnterBackgroundNotification,
    UIApplicationState, UIApplicationWillResignActiveNotification, UIScreen, UIViewController, UIWindowDidBecomeKeyNotification};
use std::{cell::RefCell, sync::atomic::{AtomicU16, Ordering}, time::Instant};

static REQUESTED: AtomicU16 = AtomicU16::new(120);
thread_local! { static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) }; }

pub fn safe_margins() -> [f32; 4] {
    let Some(mtm) = MainThreadMarker::new() else { return [0.; 4] };
    #[allow(deprecated)]
    let Some(window) = UIApplication::sharedApplication(mtm).keyWindow() else { return [0.; 4] };
    let width = window.bounds().size.width.max(1.);
    let i = window.safeAreaInsets();
    [i.left, i.top, i.right, i.bottom].map(|v| (v * 2. / width) as f32)
}

fn maximum(mtm: MainThreadMarker) -> u16 {
    #[allow(deprecated)]
    let hz = UIScreen::mainScreen(mtm).maximumFramesPerSecond().clamp(1, u16::MAX as isize) as u16;
    crate::ui::MAX_DEVICE_REFRESH_RATE.store((hz as f32).to_bits(), Ordering::Relaxed);
    hz
}

struct Driver {
    controller: Retained<UIViewController>,
    // NSNotificationCenter does not retain the observer.
    observer: Retained<LifecycleObserver>,
    origin: Instant,
    last_frame: Option<f64>,
    last_sample: f64,
    state: DisplayState,
    recovery: Recovery,
}
impl Driver {
    fn sample(&mut self, event: Option<&str>) {
        let mtm = self.observer.mtm();
        let process = NSProcessInfo::processInfo();
        self.state.maximum_hz = maximum(mtm);
        self.state.requested_hz = REQUESTED.load(Ordering::Relaxed).min(self.state.maximum_hz).max(1);
        self.state.application_state = UIApplication::sharedApplication(mtm).applicationState().0;
        self.state.low_power = process.isLowPowerModeEnabled();
        self.state.thermal_state = process.thermalState().0;
        self.state.guided_access = UIAccessibilityIsGuidedAccessEnabled();
        let hz: isize = unsafe { msg_send![&self.controller, preferredFramesPerSecond] };
        self.state.preferred_range = [hz as f32; 3];
        self.state.recovery_attempts = self.recovery.attempts;
        self.last_sample = self.origin.elapsed().as_secs_f64();
        diagnostics::display_state(self.last_sample, event, self.state.clone());
    }
    fn apply(&mut self, paused: bool, rearm: bool, reason: &str) {
        let hz = REQUESTED.load(Ordering::Relaxed).min(maximum(self.observer.mtm())).max(1);
        // Leave UIKit in charge of drawing and presenting the drawable.
        // Do not pause its controller to manually call GLKView.display.
        unsafe {
            let _: () = msg_send![&self.controller, setPreferredFramesPerSecond: hz as isize];
            let _: () = msg_send![&self.controller, setPaused: paused];
        }
        self.state.paused = paused;
        self.state.generation += 1;
        self.last_frame = None;
        self.recovery.reset(self.origin.elapsed().as_secs_f64(), rearm);
        self.sample(Some(reason));
    }
}

define_class!(
    // SAFETY: UIKit notifications and all controller operations stay on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct LifecycleObserver;
    unsafe impl NSObjectProtocol for LifecycleObserver {}
    impl LifecycleObserver {
        #[unsafe(method(lifecycleChanged:))]
        fn lifecycle_changed(&self, notification: &NSNotification) {
            let name = notification.name();
            unsafe {
                if &*name == UIApplicationWillResignActiveNotification || &*name == UIApplicationDidEnterBackgroundNotification {
                    set_paused(true);
                } else if &*name == UIApplicationDidBecomeActiveNotification || &*name == UIAccessibilityGuidedAccessStatusDidChangeNotification {
                    set_paused(false);
                } else if &*name == UIWindowDidBecomeKeyNotification {
                    DRIVER.with(|driver| {
                        if let Some(d) = driver.borrow_mut().as_mut() {
                            if let (Some(object), Some(view)) = (notification.object(), d.controller.view()) {
                                if let Some(window) = view.window() {
                                    if Retained::as_ptr(&object).cast::<()>() == Retained::as_ptr(&window).cast::<()>() {
                                        let paused = UIApplication::sharedApplication(self.mtm()).applicationState() != UIApplicationState::Active;
                                        d.apply(paused, true, "game window became key");
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    }
);

pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    DRIVER.with(|driver| {
        if driver.borrow().is_some() { return; }
        #[allow(deprecated)]
        let Some(window) = UIApplication::sharedApplication(mtm).keyWindow() else { return };
        let Some(controller) = window.rootViewController() else { return };
        let Some(class) = AnyClass::get(c"GLKViewController") else { return };
        if !controller.isKindOfClass(class) { return; }
        let observer = LifecycleObserver::alloc(mtm).set_ivars(());
        let observer: Retained<LifecycleObserver> = unsafe { msg_send![super(observer), init] };
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            for name in [UIApplicationWillResignActiveNotification, UIApplicationDidEnterBackgroundNotification,
                UIApplicationDidBecomeActiveNotification, UIAccessibilityGuidedAccessStatusDidChangeNotification,
                UIWindowDidBecomeKeyNotification] {
                center.addObserver_selector_name_object(&observer, sel!(lifecycleChanged:), Some(name), None);
            }
        }
        let mut d = Driver { controller, observer, origin: Instant::now(), last_frame: None,
            last_sample: f64::NEG_INFINITY, state: DisplayState::default(), recovery: Recovery::default() };
        d.apply(UIApplication::sharedApplication(mtm).applicationState() != UIApplicationState::Active,
            true, "upstream GLKViewController driver installed");
        *driver.borrow_mut() = Some(d);
    });
}

pub fn set_requested(rate: u16) {
    REQUESTED.store(rate.max(1), Ordering::Relaxed);
    install();
    set_paused(false);
}

pub fn set_paused(paused: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let paused = paused || UIApplication::sharedApplication(mtm).applicationState() != UIApplicationState::Active;
    DRIVER.with(|driver| {
        if let Some(d) = driver.borrow_mut().as_mut() {
            d.apply(paused, true, if paused { "application inactive" } else { "refresh preference renewed" });
        }
    });
}

/// Observe actual game frames without adding another display link or present.
pub fn record_frame(work_seconds: f64) {
    DRIVER.with(|driver| {
        let mut driver = driver.borrow_mut();
        let Some(d) = driver.as_mut() else { return };
        let now = d.origin.elapsed().as_secs_f64();
        let interval = d.last_frame.replace(now).map(|last| now - last);
        if now - d.last_sample >= 1. { d.sample(None); }
        diagnostics::display_frame(DisplayFrame {
            at_seconds: now, timestamp: now, target_timestamp: 0.,
            callback_interval_ms: interval.map(|v| v * 1000.), arrival_interval_ms: interval.map(|v| v * 1000.),
            callback_work_ms: work_seconds * 1000., glk_display_ms: 0., generation: d.state.generation,
        });
        if !d.state.paused && d.recovery.observe(now, interval.unwrap_or(0.), work_seconds,
            d.state.requested_hz, d.state.low_power || d.state.thermal_state >= 2) {
            d.apply(false, false, "low frame rate: bounded preference renewal");
        }
    });
}
