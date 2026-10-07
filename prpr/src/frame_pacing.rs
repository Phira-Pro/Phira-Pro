//! Drive GLKView directly with a public CADisplayLink. GLKViewController's
//! legacy automatic loop cannot expose ProMotion's preferredFrameRateRange.
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyClass, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use objc2_ui_kit::{UIApplication, UIApplicationState, UIScreen, UIView, UIViewController};
use std::cell::RefCell;

struct Driver {
    link: Retained<CADisplayLink>,
    controller: Retained<UIViewController>,
}

thread_local! { static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) }; }

#[derive(Default)]
struct FrameTargetIvars {
    view: RefCell<Option<Retained<UIView>>>,
}

define_class!(
    // SAFETY: NSObject imposes no additional subclass requirements. All UIKit
    // state and display-link callbacks stay on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = FrameTargetIvars]
    struct FrameTarget;
    unsafe impl NSObjectProtocol for FrameTarget {}
    impl FrameTarget {
        #[unsafe(method(renderFrame:))]
        fn render_frame(&self, _link: &CADisplayLink) {
            if UIApplication::sharedApplication(self.mtm()).applicationState() != UIApplicationState::Active { return; }
            // GLKViewController may resume its private link after UIKit's
            // lifecycle notifications. Reassert ownership before presenting.
            DRIVER.with(|driver| {
                if let Some(driver) = driver.borrow().as_ref() {
                    unsafe {
                        let paused: bool = msg_send![&driver.controller, isPaused];
                        if !paused { let _: () = msg_send![&driver.controller, setPaused: true]; }
                    }
                }
            });
            if let Some(view) = self.ivars().view.borrow().as_ref() {
                // SAFETY: install() verified this is a GLKView, whose public
                // display method presents once and invokes miniquad's delegate.
                unsafe { let _: () = msg_send![view, display]; }
            }
        }
    }
);

/// Called after miniquad has created its window, from the first GL callback.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    DRIVER.with(|driver| {
        if driver.borrow().is_some() {
            return;
        }
        #[allow(deprecated)]
        let Some(window) = UIApplication::sharedApplication(mtm).keyWindow() else {
            return;
        };
        let Some(controller) = window.rootViewController() else {
            return;
        };
        let Some(view) = controller.view() else {
            return;
        };
        let Some(view_class) = AnyClass::get(c"GLKView") else {
            return;
        };
        let Some(controller_class) = AnyClass::get(c"GLKViewController") else {
            return;
        };
        if !view.isKindOfClass(view_class) || !controller.isKindOfClass(controller_class) {
            return;
        }
        let target = FrameTarget::alloc(mtm).set_ivars(FrameTargetIvars {
            view: RefCell::new(Some(view)),
        });
        // SAFETY: NSObject init and CADisplayLink's target/selector signatures
        // match the method declared above. The link retains its target.
        let target: Retained<FrameTarget> = unsafe { msg_send![super(target), init] };
        let link = unsafe { CADisplayLink::displayLinkWithTarget_selector(&target, sel!(renderFrame:)) };
        #[allow(deprecated)]
        let maximum = UIScreen::mainScreen(mtm).maximumFramesPerSecond().clamp(1, 120);
        if link.respondsToSelector(sel!(setPreferredFrameRateRange:)) {
            link.setPreferredFrameRateRange(CAFrameRateRange {
                // An 80..120 range explicitly permits ProMotion to settle at
                // 80 Hz. Rhythm input and rendering request the display maximum;
                // iOS may still override this for power/thermal system policy.
                minimum: maximum as f32,
                maximum: maximum as f32,
                preferred: maximum as f32,
            });
        } else {
            #[allow(deprecated)]
            link.setPreferredFramesPerSecond(maximum);
        }
        // Prevent two render loops, including after a GLK automatic resume.
        unsafe {
            let _: () = msg_send![&controller, setPreferredFramesPerSecond: maximum];
            let _: () = msg_send![&controller, setPaused: true];
        }
        unsafe {
            link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
        }
        *driver.borrow_mut() = Some(Driver { link, controller });
        tracing::info!("iOS display link installed: requested {maximum} Hz");
    });
}

/// Miniquad's application lifecycle callback is on the same main thread.
pub fn set_paused(paused: bool) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    DRIVER.with(|driver| {
        if let Some(driver) = driver.borrow().as_ref() {
            if !paused {
                let maximum = UIScreen::mainScreen(MainThreadMarker::new().unwrap()).maximumFramesPerSecond().clamp(1, 120);
                if driver.link.respondsToSelector(sel!(setPreferredFrameRateRange:)) {
                    driver.link.setPreferredFrameRateRange(CAFrameRateRange { minimum: maximum as f32, maximum: maximum as f32, preferred: maximum as f32 });
                } else {
                    driver.link.setPreferredFramesPerSecond(maximum);
                }
            }
            unsafe {
                let _: () = msg_send![&driver.controller, setPaused: true];
            }
            driver.link.setPaused(paused);
        }
    });
}
