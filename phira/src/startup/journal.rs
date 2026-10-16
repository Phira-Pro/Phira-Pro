//! Bounded startup checkpoints; filesystem failures never stop application startup.
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const CURRENT: &str = "PhiraPro-startup-log.txt";
const PREVIOUS: &str = "PhiraPro-startup-previous.txt";
const LIMIT: u64 = 64 * 1024;
const README: &str = "Phira Pro 文件共享\n\niOS 的谱面、成绩及设置保存在应用内部的 Library/data 中，其他平台沿用各自的数据目录。请使用应用内的备份、导出功能管理。\n\n启动问题排查：\nPhiraPro-startup-log.txt：本次启动的版本、时间及阶段。\nPhiraPro-startup-previous.txt：上一次启动日志（重开应用后可用于查找上次退出的位置）。\nPhiraPro-startup-error.txt：捕获到的启动错误；仅发生相关错误时生成，请核对其中的时间和构建号。\n\n反馈黑屏或闪退时，请发送上述日志文件，并说明设备型号、系统版本及安装方式。日志只记录启动阶段与错误，不上传任何数据。\n";

pub(super) fn prepare(directory: &Path, identity: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    // Preserve a user's edits to the explanatory file on later launches.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("PhiraPro-说明.txt"))
    {
        Ok(mut file) => file.write_all(README.as_bytes())?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let path = directory.join(CURRENT);
    match fs::File::open(&path) {
        Ok(previous_file) => {
            let mut previous = Vec::new();
            previous_file.take(LIMIT).read_to_end(&mut previous)?;
            fs::write(directory.join(PREVIOUS), previous)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(&path, format!("{identity}\n"))?;
    Ok(path)
}

pub(super) fn append(path: &Path, message: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    let remaining = LIMIT.saturating_sub(file.metadata()?.len()) as usize;
    if remaining <= 1 {
        return Ok(());
    }
    let mut end = message.len().min(remaining - 1);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    file.write_all(message[..end].as_bytes())?;
    file.write_all(b"\n")?;
    // A crash before shutdown must not leave checkpoints only in a user-space buffer.
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_preserves_last_boot_and_does_not_replace_other_documents() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Documents");
        let path = prepare(&directory, "build 75 first").unwrap();
        assert!(directory.join("PhiraPro-说明.txt").is_file());
        fs::write(directory.join("my-file.txt"), "keep").unwrap();
        fs::write(directory.join("PhiraPro-startup-error.txt"), "older error").unwrap();
        fs::write(directory.join("PhiraPro-说明.txt"), "custom notes").unwrap();
        append(&path, "Stage: graphics bootstrap").unwrap();
        let first = fs::read(&path).unwrap();
        prepare(&directory, "build 75 second").unwrap();
        assert_eq!(fs::read(directory.join(PREVIOUS)).unwrap(), first);
        assert_eq!(fs::read_to_string(&path).unwrap(), "build 75 second\n");
        assert_eq!(fs::read_to_string(directory.join("my-file.txt")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(directory.join("PhiraPro-startup-error.txt")).unwrap(), "older error");
        assert_eq!(fs::read_to_string(directory.join("PhiraPro-说明.txt")).unwrap(), "custom notes");
    }
    #[test]
    fn log_is_bounded_and_keeps_valid_utf8() {
        let root = tempfile::tempdir().unwrap();
        let path = prepare(root.path(), "header").unwrap();
        append(&path, &"阶段".repeat(LIMIT as usize)).unwrap();
        append(&path, "must not exceed cap").unwrap();
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() <= LIMIT as usize);
        assert!(std::str::from_utf8(&bytes).is_ok());
    }
}
