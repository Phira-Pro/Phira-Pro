//! Platform-independent display-link recovery policy. Never override OS power limits.
#[derive(Default)]
pub(crate) struct Recovery {
    started: f64,
    elapsed: f64,
    work: f64,
    frames: u32,
    pub attempts: u8,
}

impl Recovery {
    pub fn reset(&mut self, now: f64, rearm: bool) {
        let attempts = if rearm { 0 } else { self.attempts };
        *self = Self {
            started: now,
            attempts,
            ..Self::default()
        };
    }

    /// A stable low callback rate with spare rendering time may be a stale link.
    /// Allow two rebuilds per activation/request, with a two-second warmup each.
    pub fn observe(&mut self, now: f64, interval: f64, work: f64, requested: u16, power_limited: bool) -> bool {
        if !interval.is_finite() || !(0.001..=0.1).contains(&interval) || !work.is_finite() || work < 0. {
            self.reset(now, false);
            return false;
        }
        if now - self.started < 2. {
            return false;
        }
        self.elapsed += interval;
        self.work += work;
        self.frames += 1;
        if self.elapsed < 1. {
            return false;
        }
        let hz = self.frames as f64 / self.elapsed;
        let spare = self.work / (self.frames as f64) < 0.8 / requested.max(1) as f64;
        let rebuild = self.attempts < 2 && !power_limited && spare && hz < requested as f64 * 0.8;
        self.elapsed = 0.;
        self.work = 0.;
        self.frames = 0;
        if rebuild {
            self.attempts += 1;
            self.reset(now, false);
        }
        rebuild
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(r: &mut Recovery, start: f64, hz: f64, work: f64, limited: bool) -> usize {
        (1..=600)
            .filter(|n| r.observe(start + *n as f64 / hz, 1. / hz, work, 120, limited))
            .count()
    }
    #[test]
    fn stable_sixty_rebuilds_only_twice_then_rearms_on_resume() {
        let mut r = Recovery::default();
        assert_eq!(feed(&mut r, 0., 60., 0.002, false), 2);
        assert_eq!(feed(&mut r, 10., 60., 0.002, false), 0);
        r.reset(20., true);
        assert_eq!(feed(&mut r, 20., 60., 0.002, false), 2);
    }
    #[test]
    fn healthy_busy_or_system_limited_links_are_not_rebuilt() {
        for (hz, work, limited) in [(120., 0.002, false), (60., 0.015, false), (60., 0.002, true)] {
            assert_eq!(feed(&mut Recovery::default(), 0., hz, work, limited), 0);
        }
    }
    #[test]
    fn pause_gaps_and_invalid_samples_do_not_trigger_recovery() {
        let mut r = Recovery::default();
        for interval in [f64::NAN, 5., 0., -1.] {
            assert!(!r.observe(10., interval, 0.002, 120, false));
        }
        assert!(!r.observe(10.1, 1. / 60., 0.002, 120, false));
        assert_eq!(r.attempts, 0);
    }
}
