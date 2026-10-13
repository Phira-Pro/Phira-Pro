//! Time manager for music time and real time synchronization.

use crate::config::Config;

pub struct TimeManager {
    pub adjust_time: bool,
    pub start_time: f64,
    pause_time: Option<f64>,
    pub speed: f64,
    pub force: f64,
    wait: f64,

    get_time_fn: Box<dyn Fn() -> f64>,
}

impl Default for TimeManager {
    fn default() -> Self {
        Self::new(1.0, false)
    }
}

impl TimeManager {
    pub fn from_config(config: &Config) -> Self {
        Self::new(1., config.adjust_time)
    }

    pub fn manual(get_time_fn: Box<dyn Fn() -> f64>) -> Self {
        let start_time = get_time_fn();
        Self {
            adjust_time: false,
            start_time,
            pause_time: None,
            speed: 1.0,
            wait: f64::NEG_INFINITY,
            force: 3e-3,

            get_time_fn,
        }
    }

    pub fn new(speed: f64, adjust_time: bool) -> Self {
        // we use performance.now() on web since audioContext.currentTime is not stable
        // and may cause serious latency problem
        #[cfg(target_arch = "wasm32")]
        let get_time_fn = {
            let perf = web_sys::window().unwrap().performance().unwrap();
            move || perf.now() / 1000.
        };
        #[cfg(not(target_arch = "wasm32"))]
        let get_time_fn = {
            let start = std::time::Instant::now();
            move || start.elapsed().as_secs_f64()
        };
        let t = get_time_fn();
        Self {
            adjust_time,
            start_time: t,
            pause_time: None,
            speed,
            wait: f64::NEG_INFINITY,
            force: 3e-3,

            get_time_fn: Box::new(get_time_fn),
        }
    }

    pub fn real_time(&self) -> f64 {
        (self.get_time_fn)()
    }

    pub fn reset(&mut self) {
        self.start_time = self.real_time();
        self.pause_time = None;
        self.wait = f64::NEG_INFINITY;
    }

    pub fn wait(&mut self) {
        self.wait = self.real_time() + 0.1;
    }

    pub fn dont_wait(&mut self) {
        self.wait = f64::NEG_INFINITY;
    }

    #[must_use]
    pub fn now(&self) -> f64 {
        (self.pause_time.unwrap_or_else(&self.get_time_fn) - self.start_time) * self.speed
    }

    pub fn update(&mut self, music_time: f64) {
        if self.adjust_time && self.real_time() > self.wait && self.pause_time.is_none() {
            self.start_time -= (music_time - self.now()) * self.force;
        }
    }

    #[must_use]
    pub fn paused(&self) -> bool {
        self.pause_time.is_some()
    }

    pub fn pause(&mut self) {
        self.pause_time = Some(self.real_time());
    }

    pub fn resume(&mut self) {
        self.start_time += self.real_time() - self.pause_time.take().unwrap();
        self.wait();
    }

    pub fn seek_to(&mut self, pos: f64) {
        self.start_time = self.pause_time.unwrap_or_else(&self.get_time_fn) - pos / self.speed;
        self.wait();
    }

    /// Interpolate running input without undoing an explicit seek/rate change.
    /// Paused controls own the clock (practice scrubber/bookmarks), so they
    /// must never be wrapped in a temporary clock that restores the old seek.
    pub fn with_event_time<T>(&mut self, time: f64, handle: impl FnOnce(&mut Self) -> T) -> T {
        if self.paused() {
            return handle(self);
        }
        let original_start = self.start_time;
        let original_speed = self.speed;
        self.seek_to(time);
        let temporary_start = self.start_time;
        let result = handle(self);
        if self.start_time == temporary_start && self.speed == original_speed {
            self.start_time = original_start;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_scrubber_and_speed_changes_survive_touch_dispatch() {
        let mut tm = TimeManager::manual(Box::new(|| 10.));
        tm.seek_to(1.);
        tm.pause();
        for (time, speed) in [(2., 1.), (3.5, 0.73), (0.5, 1.25)] {
            tm.with_event_time(-10., |tm| {
                tm.speed = speed;
                tm.seek_to(time);
            });
            assert!((tm.now() - time).abs() < 1e-9);
            assert!(tm.paused());
        }
    }

    #[test]
    fn ordinary_running_input_restores_frame_clock_but_explicit_seek_persists() {
        let mut tm = TimeManager::manual(Box::new(|| 10.));
        tm.seek_to(3.);
        tm.with_event_time(2.95, |tm| assert!((tm.now() - 2.95).abs() < 1e-9));
        assert_eq!(tm.now(), 3.);
        tm.with_event_time(2.95, |tm| tm.seek_to(5.));
        assert_eq!(tm.now(), 5.);
    }
}
