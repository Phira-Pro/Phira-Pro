//! Cooperative checkpoints for a local future polled by the loading scene.
//! No resource crosses threads; never yield inside a GL command or a profiler
//! scope. A checkpoint suspends once, so the normal scene gets to render.
use std::{future::poll_fn, task::Poll, time::{Duration, Instant}};

pub struct FrameBudget {
    enabled: bool,
    began: Instant,
}

impl FrameBudget {
    pub fn new(enabled: bool) -> Self { Self { enabled, began: Instant::now() } }

    pub async fn checkpoint(&mut self, boundary: bool) {
        if !self.enabled || (!boundary && self.began.elapsed() < Duration::from_millis(2)) { return; }
        let mut suspended = false;
        poll_fn(move |cx| {
            if suspended { Poll::Ready(()) } else {
                suspended = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }).await;
        self.began = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, task::{Context, Waker}};

    #[test]
    fn enabled_boundary_preserves_work_across_a_render_opportunity() {
        let mut state = 0;
        let mut future = Box::pin(async {
            let mut budget = FrameBudget::new(true);
            state = 1;
            budget.checkpoint(true).await;
            state = 2;
            budget.checkpoint(true).await;
            state = 3;
        });
        let mut cx = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_ready());
        drop(future);
        assert_eq!(state, 3);
    }

    #[test]
    fn disabled_schedule_is_synchronous_and_cancellation_does_not_finish_work() {
        let mut budget = FrameBudget::new(false);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Box::pin(budget.checkpoint(true)).as_mut().poll(&mut cx).is_ready());
        let mut completed = false;
        let mut future = Box::pin(async {
            FrameBudget::new(true).checkpoint(true).await;
            completed = true;
        });
        assert!(future.as_mut().poll(&mut cx).is_pending());
        drop(future);
        assert!(!completed);
    }
}
