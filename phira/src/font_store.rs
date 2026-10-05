//! Persistent custom fonts. Validate before atomically replacing the current font.
use anyhow::{Context, Result};
use prpr::ui::{parse_font, FontArc};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: u64 = 32 * 1024 * 1024;

fn read(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).context("读取字体失败")?;
    let size = file.metadata()?.len();
    anyhow::ensure!(size > 0 && size <= MAX_BYTES, "字体文件为空或超过 32 MB");
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(!bytes.is_empty() && bytes.len() as u64 <= MAX_BYTES, "字体文件为空或超过 32 MB");
    Ok(bytes)
}

pub fn load(path: &Path) -> Result<FontArc> {
    parse_font(read(path)?)
}

pub fn import(source: &Path, destination: &Path) -> Result<()> {
    let bytes = read(source)?;
    parse_font(bytes.clone())?;
    let parent = destination.parent().context("字体保存路径无效")?;
    let mut staging = tempfile::NamedTempFile::new_in(parent).context("创建字体临时文件失败")?;
    staging.write_all(&bytes)?;
    staging.as_file().sync_all()?;
    // persist replaces on Windows as well; a failed write never damages the
    // current font. TempFile cleanup also covers all earlier error paths.
    staging.persist(destination).map_err(|e| e.error).context("保存字体失败")?;
    Ok(())
}

pub fn reset(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("恢复默认字体失败"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FONT: &[u8] = include_bytes!("../../assets/phigros.ttf");

    #[test]
    fn import_replace_reset_and_unicode_paths() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("字体.otf");
        let dest = dir.path().join("font.ttf");
        std::fs::write(&source, FONT).unwrap();
        import(&source, &dest).unwrap();
        assert!(load(&dest).is_ok());
        import(&source, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), FONT);
        // A failed import preserves the working font, without stale temp files.
        std::fs::write(&source, b"not a font").unwrap();
        assert!(import(&source, &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), FONT);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        reset(&dest).unwrap();
        reset(&dest).unwrap();
        assert!(!dest.exists());
    }

    #[test]
    fn missing_empty_oversized_corrupt_and_unwritable_fonts_report_errors() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("bad.ttf");
        assert!(load(&source).is_err());
        std::fs::write(&source, []).unwrap();
        assert!(load(&source).is_err());
        File::create(&source).unwrap().set_len(MAX_BYTES + 1).unwrap();
        assert!(load(&source).is_err());
        std::fs::write(&source, &FONT[..100]).unwrap();
        assert!(load(&source).is_err());
        std::fs::write(&source, FONT).unwrap();
        assert!(import(&source, &dir.path().join("missing/font.ttf")).is_err());
        assert!(reset(dir.path()).is_err());
    }
}
