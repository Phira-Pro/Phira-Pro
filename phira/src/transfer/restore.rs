//! Validate and stage every selected entry before changing live data. Roll back a failed apply.
use super::*;
use std::{
    collections::HashSet,
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
};
pub static CONFIG_RESTORED: AtomicBool = AtomicBool::new(false);
pub const CATEGORIES: [&str; 7] = ["谱面", "资源包", "回放与成绩", "插件", "设置与布局", "外观与字体", "其他数据"];
#[derive(Clone, Debug)]
pub struct Preview {
    pub files: [usize; 7],
    pub conflicts: [usize; 7],
    pub bytes: u64,
    pub version: Option<String>,
}
#[derive(Clone)]
pub struct Options {
    pub selected: [bool; 7],
    pub replace: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            selected: [true; 7],
            replace: false,
        }
    }
}
pub fn category(path: &Path) -> usize {
    let first = path
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    match first.as_str() {
        "charts" => 0,
        "respack" => 1,
        "replays" | "history.json" | "course-history.json" | "course-rating.json" => 2,
        "plugins" => 3,
        "data.json" | "hud.json" | "judgement-presets.json" | "practice-bookmarks.json" => 4,
        "appearance" | "font.ttf" | "font.otf" => 5,
        _ => 6,
    }
}
fn safe_path(entry: &zip::read::ZipFile<'_, std::fs::File>) -> Result<PathBuf> {
    let path = entry.enclosed_name().context("备份包含越界路径")?;
    anyhow::ensure!(!entry.name().contains('\\') && !entry.name().contains(':') && !entry.name().starts_with('/'), "备份路径无效");
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        anyhow::ensure!(!name.ends_with(['.', ' ']), "备份路径含有不兼容的结尾字符");
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        anyhow::ensure!(
            !matches!(
                stem.as_str(),
                "CON"
                    | "PRN"
                    | "AUX"
                    | "NUL"
                    | "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            ),
            "备份路径包含系统保留名称"
        );
    }
    anyhow::ensure!(entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000), "备份不支持符号链接");
    anyhow::ensure!(!path.starts_with("_restore_snapshots"), "快照目录不能从备份覆盖");
    Ok(path)
}
fn check_destination(root: &Path, rel: &Path) -> Result<()> {
    let mut path = root.to_owned();
    for component in rel.components() {
        path.push(component.as_os_str());
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            anyhow::ensure!(!meta.file_type().is_symlink(), "目标包含链接，无法安全还原");
        }
    }
    Ok(())
}
pub fn inspect_at(src: &Path, root: &Path) -> Result<Preview> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(src)?)?;
    let mut out = Preview {
        files: [0; 7],
        conflicts: [0; 7],
        bytes: 0,
        version: None,
    };
    let mut seen = HashSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let path = safe_path(&entry)?;
        anyhow::ensure!(seen.insert(path.to_string_lossy().to_lowercase()), "备份包含重复路径");
        if entry.is_dir() {
            continue;
        }
        check_destination(root, &path)?;
        if path == Path::new("pro-backup.json") {
            let mut s = String::new();
            entry.take(8192).read_to_string(&mut s)?;
            out.version = serde_json::from_str::<serde_json::Value>(&s)
                .ok()
                .and_then(|v| v["version"].as_str().map(str::to_owned));
            continue;
        }
        let c = category(&path);
        out.files[c] += 1;
        out.conflicts[c] += usize::from(root.join(&path).exists());
        out.bytes = out.bytes.checked_add(entry.size()).context("备份大小溢出")?;
        anyhow::ensure!(out.bytes <= 32 * 1024 * 1024 * 1024, "备份解压大小超过 32 GiB");
    }
    anyhow::ensure!(out.files.iter().sum::<usize>() > 0, "备份中没有数据");
    Ok(out)
}
pub fn inspect(src: &Path) -> Result<Preview> {
    inspect_at(src, &data_dir()?)
}
fn atomic_copy(src: &Path, dst: &Path) -> Result<()> {
    let parent = dst.parent().context("目标路径无效")?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut std::fs::File::open(src)?, &mut tmp)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dst).map_err(|e| e.error)?;
    Ok(())
}
pub fn apply_at(src: &Path, root: &Path, options: &Options) -> Result<(u64, PathBuf)> {
    inspect_at(src, root)?;
    std::fs::create_dir_all(root)?;
    let stage = tempfile::tempdir_in(root)?;
    let new = stage.path().join("new");
    let old = stage.path().join("old");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(src)?)?;
    let mut paths = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let rel = safe_path(&entry)?;
        if entry.is_dir()
            || rel == Path::new("pro-backup.json")
            || !options.selected[category(&rel)]
            || (!options.replace && root.join(&rel).exists())
        {
            continue;
        }
        let dst = new.join(&rel);
        std::fs::create_dir_all(dst.parent().unwrap())?;
        let mut f = std::fs::File::create(&dst)?;
        std::io::copy(&mut entry, &mut f)?;
        f.sync_all()?;
        if rel == Path::new("data.json") {
            let _: crate::data::Data = serde_json::from_reader(std::fs::File::open(&dst)?).context("备份中的 data.json 无效，未更改本机数据")?;
        }
        paths.push(rel);
    }
    anyhow::ensure!(!paths.is_empty(), "所选内容没有需要还原的文件（可调整同名文件处理方式）");
    for rel in &paths {
        if root.join(rel).exists() {
            let dst = old.join(rel);
            std::fs::create_dir_all(dst.parent().unwrap())?;
            std::fs::copy(root.join(rel), dst)?;
        }
    }
    let snapshots = root.join("_restore_snapshots");
    std::fs::create_dir_all(&snapshots)?;
    let snapshot = snapshots.join(format!("before-restore-{}.zip", uuid::Uuid::new_v4()));
    let mut z = zip::ZipWriter::new(std::fs::File::create(&snapshot)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for rel in &paths {
        if old.join(rel).is_file() {
            z.start_file(rel.to_string_lossy().replace('\\', "/"), opts)?;
            std::io::copy(&mut std::fs::File::open(old.join(rel))?, &mut z)?;
        }
    }
    z.start_file("restore-manifest.json", opts)?;
    z.write_all(serde_json::to_vec(&paths)?.as_slice())?;
    z.finish()?;
    let mut applied = Vec::new();
    for rel in &paths {
        if let Err(err) = atomic_copy(&new.join(rel), &root.join(rel)) {
            let mut rollback_errors = Vec::new();
            for rel in applied.iter().rev() {
                let result = if old.join(rel).is_file() {
                    atomic_copy(&old.join(rel), &root.join(rel))
                } else {
                    std::fs::remove_file(root.join(rel)).map_err(Into::into)
                };
                if let Err(e) = result {
                    rollback_errors.push(format!("{e:#}"));
                }
            }
            anyhow::bail!("还原中断，已尝试恢复原文件：{err:#} {}", rollback_errors.join("; "));
        }
        applied.push(rel.clone());
    }
    Ok((paths.len() as u64, snapshot))
}
pub fn apply(src: &Path, options: &Options) -> Result<(u64, PathBuf)> {
    let root = data_dir()?;
    let changes_config = options.selected[4]
        && (options.replace || !root.join("data.json").exists())
        && zip::ZipArchive::new(std::fs::File::open(src)?)?.by_name("data.json").is_ok();
    let previous = CONFIG_RESTORED.load(Ordering::Relaxed);
    if changes_config {
        CONFIG_RESTORED.store(true, Ordering::Relaxed);
    }
    match apply_at(src, &root, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            CONFIG_RESTORED.store(previous, Ordering::Relaxed);
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selective_restore_preserves_unselected_files_and_snapshots_conflicts() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("live");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("data.json"), "old").unwrap();
        let src = t.path().join("backup.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&src).unwrap());
        for (path, data) in [("data.json", r#"{"config":{"speed":0.8}}"#), ("replays/a.ppr", "tape")] {
            z.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let p = inspect_at(&src, &root).unwrap();
        assert_eq!(p.conflicts[4], 1);
        let mut o = Options::default();
        o.selected[4] = false;
        assert_eq!(apply_at(&src, &root, &o).unwrap().0, 1);
        assert_eq!(std::fs::read_to_string(root.join("data.json")).unwrap(), "old");
        o.selected = [false; 7];
        o.selected[4] = true;
        o.replace = true;
        let (_, snap) = apply_at(&src, &root, &o).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("data.json")).unwrap(), r#"{"config":{"speed":0.8}}"#);
        let mut z = zip::ZipArchive::new(std::fs::File::open(snap).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name("data.json").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "old");
    }
    #[test]
    fn malformed_configuration_aborts_before_any_live_write() {
        let t = tempfile::tempdir().unwrap();
        let live = t.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("data.json"), "original").unwrap();
        let src = t.path().join("bad.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&src).unwrap());
        for (p, data) in [("history.json", "[]"), ("data.json", "not JSON")] {
            z.start_file(p, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        assert!(apply_at(
            &src,
            &live,
            &Options {
                replace: true,
                ..Default::default()
            }
        )
        .is_err());
        assert!(!live.join("history.json").exists());
        assert_eq!(std::fs::read_to_string(live.join("data.json")).unwrap(), "original");
    }
    #[test]
    fn rejects_paths_that_could_escape_or_alias_on_windows_before_writing() {
        for path in ["../outside.json", "folder/.. /outside.json", "CON.txt"] {
            let t = tempfile::tempdir().unwrap();
            let live = t.path().join("live");
            std::fs::create_dir_all(&live).unwrap();
            std::fs::write(live.join("data.json"), "original").unwrap();
            let src = t.path().join("bad.zip");
            let mut z = zip::ZipWriter::new(std::fs::File::create(&src).unwrap());
            for (p, data) in [("data.json", "replacement"), (path, "bad")] {
                z.start_file(p, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data.as_bytes()).unwrap();
            }
            z.finish().unwrap();
            assert!(apply_at(
                &src,
                &live,
                &Options {
                    replace: true,
                    ..Default::default()
                }
            )
            .is_err());
            assert_eq!(std::fs::read_to_string(live.join("data.json")).unwrap(), "original");
        }
    }
}
