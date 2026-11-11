//! CPU-only loading work. Never capture textures, GL state or audio devices.
use anyhow::Result;
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
#[cfg(not(target_arch = "wasm32"))]
use std::future::Future;

/// The local loading future is polled once each frame. A silent waker keeps the
/// worker from waking macroquad's outer executor on the wrong thread.
pub async fn run<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let mut worker = Box::pin(runtime.spawn_blocking(work));
        return std::future::poll_fn(move |_| {
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            worker.as_mut().poll(&mut context)
        })
        .await
        .context("CPU loading worker failed")?;
    }
    // Runtime absence and wasm keep the same decoder and output algorithm.
    work()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    fn drive<T>(future: impl Future<Output = T>) -> T {
        let mut future = Box::pin(future);
        loop {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn absent_runtime_preserves_inline_result() {
        let origin = std::thread::current().id();
        assert_eq!(
            drive(run(move || {
                assert_eq!(std::thread::current().id(), origin);
                Ok(73)
            }))
            .unwrap(),
            73
        );
    }

    #[test]
    fn worker_suspends_local_future_and_returns_same_value() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).build().unwrap();
        let _guard = rt.enter();
        let origin = std::thread::current().id();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut future = Box::pin(run(move || {
            rx.recv().unwrap();
            assert_ne!(std::thread::current().id(), origin);
            Ok(73)
        }));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        tx.send(()).unwrap();
        assert_eq!(drive(future).unwrap(), 73);
    }

    #[test]
    fn error_and_panic_are_propagated_without_retry() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).build().unwrap();
        let _guard = rt.enter();
        assert_eq!(drive(run::<()>(|| anyhow::bail!("bad image"))).unwrap_err().to_string(), "bad image");
        assert!(format!("{:#}", drive(run::<()>(|| panic!("bad decoder"))).unwrap_err()).contains("CPU loading worker failed"));
    }
}
