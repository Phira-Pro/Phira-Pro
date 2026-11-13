use super::with_effects;
use crate::client::{send_raw, Chart, File, MissingFile, Ptr};
use anyhow::{ensure, Context, Result};
use prpr::fs::{self, FileSystem, ZipFileSystem};
use sasa::AudioClip;

// A missing preview can be reconstructed from the chart archive. This is
// background work for the detail page, so bound the download and never install it.
const MAX_PREVIEW_ARCHIVE: usize = 100 * 1024 * 1024;

pub(super) async fn load_remote(id: i32) -> Result<AudioClip> {
    let chart = Ptr::<Chart>::new(id).fetch().await?;
    match chart.preview.fetch().await {
        Ok(bytes) => with_effects(AudioClip::decode(bytes.to_vec())?, None),
        Err(error) if error.is::<MissingFile>() => from_archive(&chart.file)
            .await
            .context("preview file not found; could not use chart music"),
        Err(error) => Err(error),
    }
}

async fn from_archive(file: &File) -> Result<AudioClip> {
    let mut response = send_raw(File::request_url(&file.url).timeout(std::time::Duration::from_secs(25)))
        .await?
        .error_for_status()?;
    ensure!(response.content_length().unwrap_or_default() <= MAX_PREVIEW_ARCHIVE as u64, "chart archive exceeds preview download limit");
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(chunk.len() <= MAX_PREVIEW_ARCHIVE - bytes.len(), "chart archive exceeds preview download limit");
        bytes.extend_from_slice(&chunk);
    }
    from_archive_bytes(bytes).await
}

async fn from_archive_bytes(bytes: Vec<u8>) -> Result<AudioClip> {
    // ZIP entries have their own expanded size; do not trust the HTTP size
    // limit alone when allocating and decoding an untrusted archive.
    let mut archive = ZipFileSystem::new(bytes)?.with_entry_limit(MAX_PREVIEW_ARCHIVE as u64);
    let info = fs::load_info(&mut archive).await?;
    let music = archive.load_file(&info.music).await?;
    with_effects(
        AudioClip::decode(music)?,
        Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn builds_preview_from_the_archived_song_and_declared_time_range() -> Result<()> {
        let rate = 8_000u32;
        let sample_bytes = rate * 2;
        let mut wave = Vec::new();
        wave.extend_from_slice(b"RIFF");
        wave.extend_from_slice(&(36 + sample_bytes).to_le_bytes());
        wave.extend_from_slice(b"WAVEfmt ");
        wave.extend_from_slice(&16u32.to_le_bytes());
        wave.extend_from_slice(&[1, 0, 1, 0]);
        wave.extend_from_slice(&rate.to_le_bytes());
        wave.extend_from_slice(&sample_bytes.to_le_bytes());
        wave.extend_from_slice(&[2, 0, 16, 0]);
        wave.extend_from_slice(b"data");
        wave.extend_from_slice(&sample_bytes.to_le_bytes());
        wave.resize(wave.len() + sample_bytes as usize, 0);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("info.yml", zip::write::SimpleFileOptions::default())?;
        zip.write_all(b"music: test.wav\npreviewStart: 0.25\npreviewEnd: 0.75\n")?;
        zip.start_file("test.wav", zip::write::SimpleFileOptions::default())?;
        zip.write_all(&wave)?;
        let bytes = zip.finish()?.into_inner();
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;
        let clip = runtime.block_on(from_archive_bytes(bytes))?;
        assert!((clip.length() - 0.5).abs() < 0.001);
        Ok(())
    }
}
