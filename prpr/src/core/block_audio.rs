//! Native JudgeControl/ProgressControl gesture transitions (4.0.1).
//! LevelControl's serialized preset is 1500 Hz / 0.1 s, overriding its ctor.
//! The filter is attached to the chart AudioSource, not the hit-sound mixer.
//! Unity's resonance control is not a portable DSP coefficient. Use a maximally
//! flat second-order response here, without a boost at the cutoff frequency.

#[derive(Default)]
pub(crate) struct BlockAudio {
    enabled: Option<bool>,
    used: bool,
}

impl BlockAudio {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn sync(&mut self, music: &mut sasa::Music, touching: bool) {
        self.sync_with(touching, |cutoff, seconds| {
            music.try_set_low_pass_filter(cutoff, std::f32::consts::FRAC_1_SQRT_2, seconds)
        });
    }

    pub(crate) fn suspend(&mut self, music: &mut sasa::Music) {
        if self.used {
            self.reset();
        }
        self.sync(music, false);
    }

    fn sync_with(&mut self, touching: bool, send: impl FnOnce(Option<f32>, f32) -> bool) {
        if self.enabled == Some(touching) {
            return;
        }
        // On the first touch Unity enables a disabled filter whose stored
        // cutoff is already 1500 Hz. Subsequent entries sweep from 22000 Hz
        // or the currently interrupted release, rather than from the preset.
        let seconds = if self.used { 0.1 } else { 0. };
        if send(touching.then_some(1500.), seconds) {
            self.enabled = Some(touching);
            self.used |= touching;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_entry_release_reentry_and_multi_touch_use_native_transitions() {
        let mut control = BlockAudio::default();
        let mut commands = Vec::new();
        for touching in [false, true, true, true, false, false, true] {
            control.sync_with(touching, |f, t| {
                commands.push((f, t));
                true
            });
        }
        assert_eq!(commands, [(None, 0.), (Some(1500.), 0.), (None, 0.1), (Some(1500.), 0.1)]);
        control.reset();
        control.sync_with(false, |f, t| {
            assert_eq!((f, t), (None, 0.));
            true
        });
    }

    #[test]
    fn full_queue_retries_without_marking_a_transition_complete() {
        let mut control = BlockAudio::default();
        control.sync_with(true, |_, _| false);
        assert_eq!(control.enabled, None);
        assert!(!control.used);
        control.sync_with(true, |f, t| {
            assert_eq!((f, t), (Some(1500.), 0.));
            true
        });
        control.sync_with(false, |_, _| false);
        control.sync_with(false, |f, t| {
            assert_eq!((f, t), (None, 0.1));
            true
        });
    }
}
