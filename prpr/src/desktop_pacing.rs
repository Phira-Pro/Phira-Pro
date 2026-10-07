//! Window-local Windows frame scheduling. No global timer-resolution changes.
use std::{
    ptr,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
use winapi::{
    shared::{
        minwindef::FALSE,
        ntdef::{HANDLE, LARGE_INTEGER},
    },
    um::{
        handleapi::CloseHandle,
        synchapi::{CreateWaitableTimerExW, SetWaitableTimer, WaitForSingleObject},
        wingdi::{wglGetCurrentDC, DEVMODEW},
        winuser::{
            EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, WindowFromDC, ENUM_CURRENT_SETTINGS, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
        },
    },
};

/// Reads the current mode of the monitor containing this GL window. Checking
/// the primary monitor or another app's foreground window gives wrong results
/// when the game is moved between displays with different refresh rates.
fn current_monitor() -> usize {
    unsafe { MonitorFromWindow(WindowFromDC(wglGetCurrentDC()), MONITOR_DEFAULTTONEAREST) as usize }
}

fn refresh_rate(monitor: usize) -> Option<u32> {
    unsafe {
        if monitor == 0 {
            return None;
        }
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.cbSize = std::mem::size_of_val(&info) as u32;
        if GetMonitorInfoW(monitor as _, &mut info as *mut _ as _) == 0 {
            return None;
        }
        let mut mode: DEVMODEW = std::mem::zeroed();
        mode.dmSize = std::mem::size_of_val(&mode) as u16;
        if EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) == 0 {
            return None;
        }
        (10..=1000).contains(&mode.dmDisplayFrequency).then_some(mode.dmDisplayFrequency)
    }
}

pub struct Pacer {
    timer: HANDLE,
    period: Duration,
    next: Instant,
    next_refresh_check: Instant,
    refresh_worker: Option<(SyncSender<usize>, Receiver<(usize, u32)>)>,
}

impl Pacer {
    pub fn new() -> Self {
        // CREATE_WAITABLE_TIMER_HIGH_RESOLUTION is public on Win10 1803+.
        // Older systems get a normal waitable timer and the same deadline.
        let timer = unsafe {
            let mut timer = CreateWaitableTimerExW(ptr::null_mut(), ptr::null(), 2, 0x1F0003);
            if timer.is_null() {
                timer = CreateWaitableTimerExW(ptr::null_mut(), ptr::null(), 0, 0x1F0003);
            }
            timer
        };
        let now = Instant::now();
        let initial_rate = refresh_rate(current_monitor()).unwrap_or(60);
        let (requests, receive_requests) = mpsc::sync_channel(1);
        let (results, receive_results) = mpsc::channel();
        // Display enumeration can block in the driver. Only obtaining this
        // window's monitor stays on the GL thread; mode queries never stall it.
        let refresh_worker = std::thread::Builder::new()
            .name("display-refresh".into())
            .spawn(move || {
                while let Ok(monitor) = receive_requests.recv() {
                    if let Some(rate) = refresh_rate(monitor) {
                        if results.send((monitor, rate)).is_err() {
                            break;
                        }
                    }
                }
            })
            .ok()
            .map(|_| (requests, receive_results));
        tracing::info!("Windows frame pacing: {initial_rate} Hz");
        Self {
            timer,
            period: Duration::from_secs_f64(1. / initial_rate as f64),
            next: now,
            next_refresh_check: now,
            refresh_worker,
        }
    }

    pub fn wait(&mut self) {
        let now = Instant::now();
        if let Some((_, results)) = &self.refresh_worker {
            while let Ok((monitor, rate)) = results.try_recv() {
                if monitor != current_monitor() {
                    continue;
                }
                let period = Duration::from_secs_f64(1. / rate as f64);
                if self.period != period {
                    self.period = period;
                    self.next = now;
                    tracing::info!("Windows frame pacing: {rate} Hz");
                }
            }
        }
        if now >= self.next_refresh_check {
            if let Some((requests, _)) = &self.refresh_worker {
                let _ = requests.try_send(current_monitor());
            }
            self.next_refresh_check = now + Duration::from_secs(1);
        }
        let now = Instant::now();
        if let Some(remaining) = self.next.checked_duration_since(now) {
            // Sleep for most of the interval; bound the final precision wait
            // to 150 us, rather than spinning for the whole frame.
            let sleep = remaining.saturating_sub(Duration::from_micros(150));
            if !sleep.is_zero() {
                let mut due: LARGE_INTEGER = unsafe { std::mem::zeroed() };
                unsafe {
                    *due.QuadPart_mut() = -((sleep.as_nanos() / 100).max(1).min(i64::MAX as u128) as i64);
                }
                let armed = !self.timer.is_null() && unsafe { SetWaitableTimer(self.timer, &due, 0, None, ptr::null_mut(), FALSE) != 0 };
                if armed {
                    unsafe {
                        WaitForSingleObject(self.timer, 1000);
                    }
                } else {
                    std::thread::sleep(sleep);
                }
            }
            while Instant::now() < self.next {
                std::hint::spin_loop();
            }
        }
        self.next = next_deadline(self.next, Instant::now(), self.period);
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

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Pacer {
    fn drop(&mut self) {
        if !self.timer.is_null() {
            unsafe {
                CloseHandle(self.timer);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jitter_and_missed_frames_do_not_halve_the_target_or_create_bursts() {
        for rate in [60, 80, 120, 165, 240] {
            let period = Duration::from_secs_f64(1. / rate as f64);
            let start = Instant::now();
            assert_eq!(next_deadline(start, start + Duration::from_micros(100), period), start + period);
            let late = start + period * 3;
            assert_eq!(next_deadline(start, late, period), late + period);
            assert_eq!(next_deadline(start + period, start + period, period), start + period * 2);
        }
    }
}
