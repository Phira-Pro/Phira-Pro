//! Bounded PCM fallback for WebViews that cannot decode a selected audio file.
use anyhow::{ensure, Context, Result};
use std::io::Cursor;
use symphonia::core::{audio::SampleBuffer, errors::Error, io::MediaSourceStream};

pub(crate) const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_PCM: usize = 128 * 1024 * 1024;

pub(crate) fn wave(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut format = symphonia::default::get_probe()
        .format(&Default::default(), source, &Default::default(), &Default::default())?.format;
    let track = format.default_track().context("No audio track")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &Default::default())?;
    let mut wav = vec![0; 44];
    let mut spec = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(Error::DecodeError(message)) if message.contains("invalid main_data offset") => continue,
            Err(error) => return Err(error.into()),
        };
        let rate = buffer.spec().rate;
        let channels = buffer.spec().channels.count();
        ensure!((1..=2).contains(&channels) && (8_000..=192_000).contains(&rate), "Unsupported audio layout");
        ensure!(*spec.get_or_insert((rate, channels)) == (rate, channels), "Audio layout changed");
        ensure!(buffer.capacity() <= (MAX_PCM + 44 - wav.len()) / (channels * 2), "Decoded audio too large");
        let mut samples = SampleBuffer::<i16>::new(buffer.capacity() as u64, *buffer.spec());
        samples.copy_interleaved_ref(buffer);
        for sample in samples.samples() {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }
    let (rate, channels) = spec.context("Empty audio")?;
    let size = (wav.len() - 44) as u32;
    ensure!(size != 0, "Empty audio");
    let alignment = channels as u16 * 2;
    wav[0..4].copy_from_slice(b"RIFF");
    wav[4..8].copy_from_slice(&(size + 36).to_le_bytes());
    wav[8..16].copy_from_slice(b"WAVEfmt ");
    wav[16..20].copy_from_slice(&16u32.to_le_bytes());
    wav[20..22].copy_from_slice(&1u16.to_le_bytes());
    wav[22..24].copy_from_slice(&(channels as u16).to_le_bytes());
    wav[24..28].copy_from_slice(&rate.to_le_bytes());
    wav[28..32].copy_from_slice(&(rate * alignment as u32).to_le_bytes());
    wav[32..34].copy_from_slice(&alignment.to_le_bytes());
    wav[34..36].copy_from_slice(&16u16.to_le_bytes());
    wav[36..40].copy_from_slice(b"data");
    wav[40..44].copy_from_slice(&size.to_le_bytes());
    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_builtin_vorbis_to_playable_pcm_and_rejects_invalid_files() {
        let vorbis = include_bytes!("../../assets/click.ogg");
        let decoded = wave(vorbis.to_vec()).unwrap();
        assert_eq!(&decoded[..4], b"RIFF");
        assert_eq!(&decoded[8..12], b"WAVE");
        assert_eq!(&decoded[20..22], &1u16.to_le_bytes());
        assert_eq!(u32::from_le_bytes(decoded[40..44].try_into().unwrap()) as usize, decoded.len() - 44);
        assert!(decoded[44..].iter().any(|&byte| byte != 0));
        assert!(wave(b"OggS invalid".to_vec()).is_err());
    }
}
