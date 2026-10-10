//! Materialize picker files without loading an entire chart collection into RAM.
use anyhow::{Context, Result};
use std::{fs::{self, File, OpenOptions}, io::{self, Write}, path::{Path, PathBuf}};

pub(crate) fn prepare(source: &Path, temporary: &Path) -> Result<PathBuf> {
    let mut input = File::open(source).context("无法打开所选文件")?;
    anyhow::ensure!(input.metadata()?.is_file(), "所选项目不是文件");
    fs::create_dir_all(temporary).context("无法创建导入临时目录")?;
    // UIDocumentPicker's asCopy/Import mode already owns a copy in our sandbox.
    // Reuse it: a second copy can exhaust storage for a batch archive.
    if source.canonicalize()?.starts_with(temporary.canonicalize()?) {
        return Ok(source.to_path_buf());
    }
    let target = temporary.join(format!("phira-import-{}", uuid::Uuid::new_v4()));
    let mut output = OpenOptions::new().write(true).create_new(true).open(&target).context("无法创建导入临时文件")?;
    let copied = (|| -> Result<()> {
        io::copy(&mut input, &mut output).context("复制所选文件失败，请检查可用存储空间")?;
        output.flush().context("写入导入临时文件失败")?;
        Ok(())
    })();
    drop(output);
    if let Err(err) = copied {
        let _ = fs::remove_file(&target);
        return Err(err);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn picker_copy_is_reused_and_external_archive_is_streamed() {
        let root = tempfile::tempdir().unwrap();
        let temporary = root.path().join("应用临时目录");
        fs::create_dir(&temporary).unwrap();
        let picked = temporary.join("批量谱面.zip");
        fs::write(&picked, b"already imported").unwrap();
        assert_eq!(prepare(&picked, &temporary).unwrap(), picked);
        assert_eq!(fs::read_dir(&temporary).unwrap().count(), 1);

        let external = root.path().join("外部文件.zip");
        let chunk = [0x5a; 64 * 1024];
        let mut file = File::create(&external).unwrap();
        for _ in 0..512 { file.write_all(&chunk).unwrap(); }
        drop(file);
        let copied = prepare(&external, &temporary).unwrap();
        assert_ne!(copied, external);
        let mut file = File::open(copied).unwrap();
        let mut actual = [0; 64 * 1024];
        for _ in 0..512 {
            file.read_exact(&mut actual).unwrap();
            assert_eq!(actual, chunk);
        }
        assert_eq!(file.read(&mut actual).unwrap(), 0);
        assert!(external.exists());
    }

    #[test]
    fn missing_files_and_invalid_destination_report_io_errors() {
        let root = tempfile::tempdir().unwrap();
        assert!(prepare(&root.path().join("missing.zip"), root.path()).is_err());
        assert!(prepare(root.path(), root.path()).is_err());
        let file = root.path().join("file");
        fs::write(&file, b"archive").unwrap();
        assert!(prepare(&file, &file.join("temporary")).is_err());
        assert_eq!(fs::read(file).unwrap(), b"archive");
    }
}
