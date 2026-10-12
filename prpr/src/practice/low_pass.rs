// Reused from the project's sasa audio backend; identical noise-filter response.
use sasa::Frame;

/// Bilinear second-order resonant low-pass. TPT integrators keep cutoff sweeps
/// stable; coefficients are shared, but channel histories are independent.
pub(super) struct LowPass {
    pub(super) cutoff: f64,
    target: f64,
    from: f64,
    duration: f64,
    elapsed: f64,
    enabled: bool,
    releasing: bool,
    q: f64,
    sample_rate: u32,
    countdown: u32,
    a: [f64; 3],
    history: [[f64; 2]; 2],
}

impl Default for LowPass {
    fn default() -> Self {
        Self {
            cutoff: 22000.,
            target: 22000.,
            from: 22000.,
            duration: 0.,
            elapsed: 0.,
            enabled: false,
            releasing: false,
            q: 1.,
            sample_rate: 0,
            countdown: 0,
            a: [0.; 3],
            history: [[0.; 2]; 2],
        }
    }
}

impl LowPass {
    pub(super) fn set(&mut self, cutoff: Option<f32>, q: f32, seconds: f32) {
        self.from = self.cutoff;
        self.target = cutoff.unwrap_or(22000.).clamp(10., 22000.) as f64;
        self.q = q.clamp(0.1, 10.) as f64;
        self.duration = seconds.max(0.) as f64;
        self.elapsed = 0.;
        self.releasing = cutoff.is_none();
        self.enabled |= cutoff.is_some();
        self.countdown = 0;
        if self.duration == 0. {
            self.cutoff = self.target;
            if self.releasing {
                self.enabled = false;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(super) fn clear_history(&mut self) {
        self.history = [[0.; 2]; 2];
    }

    #[inline]
    pub(super) fn process(&mut self, frame: Frame, sample_rate: u32) -> Frame {
        if !self.enabled {
            // Seed a re-enabled filter from the last dry value, without ever
            // altering dry samples. This also prevents stale pause/seek tails.
            self.history = [[0., frame.0 as f64], [0., frame.1 as f64]];
            return frame;
        }
        let sweeping = self.elapsed < self.duration;
        if sweeping {
            self.elapsed = (self.elapsed + 1. / sample_rate as f64).min(self.duration);
            self.cutoff = self.from + (self.target - self.from) * (self.elapsed / self.duration);
            if self.elapsed == self.duration {
                self.cutoff = self.target;
                self.countdown = 0;
                if self.releasing {
                    self.enabled = false;
                    self.history = [[0., frame.0 as f64], [0., frame.1 as f64]];
                    return frame;
                }
            }
        }
        if self.countdown == 0 || self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            let frequency = self.cutoff.min(sample_rate as f64 * 0.49);
            let g = (std::f64::consts::PI * frequency / sample_rate as f64).tan();
            let a1 = 1. / (1. + g * (g + 1. / self.q));
            self.a = [a1, g * a1, g * g * a1];
            // At most 0.73 ms between coefficient updates at 44.1 kHz.
            // Settled filters need no trig work per sample.
            self.countdown = if sweeping { 32 } else { u32::MAX };
        }
        self.countdown -= 1;
        let [a1, a2, a3] = self.a;
        let mut outputs = [0.; 2];
        for (channel, input) in [frame.0, frame.1].into_iter().enumerate() {
            let [ic1, ic2] = self.history[channel];
            let v3 = input as f64 - ic2;
            let v1 = a1 * ic1 + a2 * v3;
            let v2 = ic2 + a2 * ic1 + a3 * v3;
            self.history[channel] = [2. * v1 - ic1, 2. * v2 - ic2];
            outputs[channel] = v2 as f32;
        }
        Frame(outputs[0], outputs[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(rate: u32, frequency: f64) -> f64 {
        let mut filter = LowPass::default();
        filter.set(Some(1500.), 1., 0.);
        let mut power = 0.;
        for i in 0..rate {
            let x = (std::f64::consts::TAU * frequency * i as f64 / rate as f64).sin() as f32;
            let y = filter.process(Frame(x, 0.), rate);
            assert_eq!(y.1, 0., "stereo cross-talk");
            if i >= rate / 2 {
                power += (y.0 as f64).powi(2);
            }
        }
        (power / (rate / 2) as f64 * 2.).sqrt()
    }

    #[test]
    fn low_frequencies_pass_and_high_frequencies_are_attenuated() {
        for rate in [44100, 48000, 96000] {
            assert!((0.99..1.02).contains(&gain(rate, 100.)));
            assert!(gain(rate, 8000.) < 0.04);
            assert!(gain(rate, 12000.) < gain(rate, 8000.));
        }
    }

    #[test]
    fn sweeps_reverse_from_current_cutoff_and_release_to_exact_bypass() {
        let mut filter = LowPass::default();
        filter.set(Some(1500.), 1., 0.1);
        for _ in 0..2400 {
            filter.process(Frame(0.2, -0.4), 48000);
        }
        assert!((filter.cutoff - 11750.).abs() < 0.01);
        let from = filter.cutoff;
        filter.set(None, 1., 0.1);
        for _ in 0..2400 {
            filter.process(Frame(0.2, -0.4), 48000);
        }
        assert!((filter.cutoff - (from + 22000.) * 0.5).abs() < 0.01);
        for _ in 0..2401 {
            filter.process(Frame(0.2, -0.4), 48000);
        }
        assert!(!filter.enabled);
        for x in [-0.7, 0., 0.2, 1.] {
            let y = filter.process(Frame(x, -x), 48000);
            assert_eq!(y.0.to_bits(), x.to_bits());
            assert_eq!(y.1.to_bits(), (-x).to_bits());
        }
    }

    #[test]
    fn rapid_reversals_rate_changes_and_reset_remain_finite() {
        let mut filter = LowPass::default();
        for i in 0..20000 {
            if i % 73 == 0 {
                filter.set((i % 146 == 0).then_some(1500.), 1., 0.1);
            }
            let rate = if i % 2000 < 1000 { 44100 } else { 48000 };
            let x = (i as f32 * 0.39).sin();
            let y = filter.process(Frame(x, -x), rate);
            assert!(y.0.is_finite() && y.1.is_finite());
            assert!(y.0.abs() < 2.);
            assert_eq!(y.0, -y.1);
        }
        filter.reset();
        let y = filter.process(Frame(0.25, -0.5), 48000);
        assert_eq!((y.0, y.1), (0.25, -0.5));
    }
}
