//! Dedicated CPU jobs publish completion even on panic; they never depend on
//! a UI-polled future or retain a semaphore after the user cancels a job.
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
static ANALYSES: AtomicUsize = AtomicUsize::new(0);
struct AnalysisSlot;
impl Drop for AnalysisSlot {
    fn drop(&mut self) {
        ANALYSES.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct Work<T> {
    receiver: Receiver<Result<T>>,
}
impl<T: Send + 'static> Work<T> {
    pub fn spawn(name: &str, job: impl FnOnce() -> Result<T> + Send + 'static) -> Result<Self> {
        let slot = if name == "prca-analysis" {
            ANALYSES
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| (n < 2).then_some(n + 1))
                .map_err(|_| anyhow::anyhow!("上一项分析仍在结束，请稍后重试"))?;
            Some(AnalysisSlot)
        } else {
            None
        };
        let (tx, receiver) = mpsc::sync_channel(1);
        let runtime = tokio::runtime::Handle::try_current().ok();
        std::thread::Builder::new()
            .name(name.into())
            .stack_size(8 << 20)
            .spawn(move || {
                let _slot = slot;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let owned = if runtime.is_none() {
                        Some(tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build()?)
                    } else {
                        None
                    };
                    let handle = runtime.unwrap_or_else(|| owned.as_ref().unwrap().handle().clone());
                    let _guard = handle.enter();
                    job()
                }))
                .unwrap_or_else(|panic| {
                    let reason = panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("未知错误");
                    Err(anyhow::anyhow!("后台任务异常：{reason}"))
                });
                let _ = tx.send(result);
            })
            .context("无法启动分析线程")?;
        Ok(Self { receiver })
    }
    pub fn take(&mut self) -> Option<Result<T>> {
        match self.receiver.try_recv() {
            Ok(v) => Some(v),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!("后台线程已结束，但未返回分析结果"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completes_without_ui_runtime_polling_and_surfaces_panics() {
        let mut work = Work::spawn("prca-test", || Ok(42)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(r) = work.take() {
                assert_eq!(r.unwrap(), 42);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut work = Work::<()>::spawn("prca-panic", || panic!("visible failure")).unwrap();
        loop {
            if let Some(r) = work.take() {
                assert!(r.unwrap_err().to_string().contains("visible failure"));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}
