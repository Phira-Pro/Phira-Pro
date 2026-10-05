// 数据迁移与备份还原。
//
// - 迁移：官方 Phira 与本改版同源代码，数据目录结构完全一致。选定官方版本的
//   `data.json` 后，即可从其同级目录导入谱面 / 皮肤 / 字体 / 外观，并可选择性
//   合并游玩与界面设置（保留本机 Pro 独有配置键）。
// - 备份：把整个 `<data>/` 打包为 zip。
// - 还原：把备份 zip 解包合并回 `<data>/`（同名覆盖，需重启生效）。
//
// 注：谱面 / 皮肤在下次启动时由 `Data::init` 重新扫描建索引，因此导入后需重启。

use crate::{dir, get_data_mut};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Write beside the destination, then atomically replace it after the data is flushed.
/// `save_data` uses this so a failed write cannot truncate the previous configuration.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("missing destination directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
mod atomic_write_tests {
    use super::*;

    #[test]
    fn replaces_existing_data_without_leaving_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new configuration").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new configuration");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_replace_preserves_the_destination_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("directory");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep"), b"existing data").unwrap();
        assert!(write_atomic(&destination, b"replacement").is_err());
        assert_eq!(std::fs::read(destination.join("keep")).unwrap(), b"existing data");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

fn data_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(dir::root()?))
}

fn count_entries(p: &Path) -> usize {
    std::fs::read_dir(p).map(|it| it.flatten().count()).unwrap_or(0)
}

/// 迁移源信息（扫描结果）。
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub root: PathBuf,
    pub charts: usize,
    pub respacks: usize,
    pub appearance: usize,
    pub has_font: bool,
    pub name: Option<String>,
}

/// 扫描一个「官方 data.json」所在目录，统计可迁移内容。
pub fn scan(data_json: &Path) -> Result<Scan> {
    let root = data_json.parent().context("无效路径")?.to_path_buf();
    if !root.is_dir() {
        anyhow::bail!("不是有效的目录");
    }
    let charts_dir = root.join("charts");
    let charts = count_entries(&charts_dir.join("custom")) + count_entries(&charts_dir.join("download"));
    let respacks = count_entries(&root.join("respack"));
    let appearance = count_entries(&root.join("appearance"));
    let has_font = root.join("font.ttf").is_file();
    let name = std::fs::read_to_string(data_json)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("me")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_owned)
        });
    Ok(Scan {
        root,
        charts,
        respacks,
        appearance,
        has_font,
        name,
    })
}

/// 递归把 src 的文件合并进 dst，**不覆盖**已存在的同名文件。返回新增文件数。
fn merge_tree(src: &Path, dst: &Path) -> Result<usize> {
    if !src.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    std::fs::create_dir_all(dst)?;
    for entry in WalkDir::new(src).min_depth(1) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let to = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&to)?;
        } else if entry.file_type().is_file() {
            if to.exists() {
                continue;
            }
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(entry.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

/// 导入结果统计。
#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub charts: usize,
    pub respacks: usize,
    pub appearance: usize,
    pub font: bool,
    pub config: bool,
}

/// 执行迁移。`import_config` 为真时额外合并源配置。
pub fn import(scan: &Scan, import_config: bool) -> Result<Imported> {
    let dst = data_dir()?;
    let mut out = Imported::default();

    let src_charts = scan.root.join("charts");
    out.charts += merge_tree(&src_charts.join("custom"), Path::new(&dir::custom_charts()?))?;
    out.charts += merge_tree(&src_charts.join("download"), Path::new(&dir::downloaded_charts()?))?;
    out.respacks = merge_tree(&scan.root.join("respack"), Path::new(&dir::respacks()?))?;
    out.appearance = merge_tree(&scan.root.join("appearance"), Path::new(&dir::appearance()?))?;

    if scan.has_font {
        let to = dst.join("font.ttf");
        if !to.exists() {
            std::fs::copy(scan.root.join("font.ttf"), &to)?;
            out.font = true;
        }
    }

    if import_config {
        let text = std::fs::read_to_string(scan.root.join("data.json"))?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let data = get_data_mut();
        if let Some(lang) = v.get("language").and_then(|it| it.as_str()) {
            data.language = Some(lang.to_owned());
        }
        if let Some(theme) = v.get("theme").and_then(|it| it.as_u64()) {
            data.theme = theme as usize;
        }
        if let Some(p) = v.get("preferReducedMotion").and_then(|it| it.as_bool()) {
            data.prefer_reduced_motion = p;
            prpr::ui::PREFER_REDUCED_MOTION.store(p, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(src_conf) = v.get("config").and_then(|it| it.as_object()) {
            // 逐键覆盖，保留本机 Pro 独有、而源配置没有的键。
            let mut cur = serde_json::to_value(&data.config)?;
            if let Some(cur_obj) = cur.as_object_mut() {
                for (k, val) in src_conf {
                    cur_obj.insert(k.clone(), val.clone());
                }
            }
            data.config = serde_json::from_value(cur)?;
            data.config.apply_ui_colors();
        }
        out.config = true;
    }

    crate::sync_data();
    Ok(out)
}

/// 把整个数据目录打包为 zip，返回打包的文件数。
pub fn create_backup(dest: &Path) -> Result<u64> {
    let root = data_dir()?;
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    let mut n = 0u64;
    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(&root) else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let name = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            zip.add_directory(format!("{name}/"), opts)?;
        } else if entry.file_type().is_file() {
            zip.start_file(name, opts)?;
            let mut f = std::fs::File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
            n += 1;
        }
    }
    zip.finish()?;
    Ok(n)
}

/// 从备份 zip 还原（同名覆盖），返回还原的文件数。需重启生效。
pub fn restore_backup(src: &Path) -> Result<u64> {
    let root = data_dir()?;
    let file = std::fs::File::open(src)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut n = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let to = root.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&to)?;
        } else {
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::File::create(&to)?;
            std::io::copy(&mut entry, &mut out)?;
            n += 1;
        }
    }
    Ok(n)
}
