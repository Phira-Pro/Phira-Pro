//! Music level matching, measured once on decoded PCM rather than per frame.
use sasa::AudioClip;

/// Gated stereo RMS: ignore silent 400ms blocks, target -18dBFS RMS and bound
/// gain to +/-12dB. Peak headroom prevents clipping. This is level matching,
/// not an EBU R128/LUFS meter or a dynamic compressor.
pub fn music_normalization_gain(clip: &AudioClip) -> f32 {
    let frames = clip.frames();
    let block = (clip.sample_rate() as usize * 2 / 5).max(1);
    let (mut energy, mut count, mut peak) = (0_f64, 0_usize, 0_f32);
    for chunk in frames.chunks(block) {
        let mut sum = 0_f64;
        for frame in chunk {
            let (l, r) = (frame.0, frame.1);
            if !l.is_finite() || !r.is_finite() {
                continue;
            }
            sum += (l as f64 * l as f64 + r as f64 * r as f64) * 0.5;
            peak = peak.max(l.abs()).max(r.abs());
        }
        if sum / chunk.len() as f64 > 0.00001 {
            energy += sum;
            count += chunk.len();
        }
    }
    if count == 0 || peak <= 0. {
        return 1.;
    }
    (0.12589254 / (energy / count as f64).sqrt() as f32).clamp(0.25, 4.).min(0.98 / peak)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sasa::Frame;

    #[test]
    fn silence_is_safe_and_matching_keeps_peak_headroom() {
        let silent = AudioClip::from_raw(vec![Frame(0., 0.); 100], 100);
        assert_eq!(music_normalization_gain(&silent), 1.);
        let quiet = AudioClip::from_raw(vec![Frame(0.05, -0.05); 100], 100);
        assert!((music_normalization_gain(&quiet) * 0.05 - 0.12589254).abs() < 1e-6);
        let loud = AudioClip::from_raw(vec![Frame(1., -1.); 100], 100);
        assert!(music_normalization_gain(&loud) <= 0.98);
    }
}
