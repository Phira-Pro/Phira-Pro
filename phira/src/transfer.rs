// 数据迁移与备份还原。
//
// - 迁移：官方 Phira 与本改版同源代码，数据目录结构完全一致。选定官方版本的
//   `data.json` 后，即可从其同级目录导入谱面 / 皮肤 / 字体 / 外观，并可选择性
//   合并游玩与界面设置（保留本机 Pro 独有配置键）。
// - 备份：把整个 `<data>/` 打包为 zip。
// - 还原：把备份 zip 解包合并回 `<data>/`（同名覆盖，需重启生效）。
//
// 注：谱面 / 皮肤在下次启动时由 `Data::init` 重新扫描建索引，因此导入后需重启。

use crate::{data::Data, dir, get_data_mut};
use anyhow::{Context, Result};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(data_json)?).context("data.json 格式无效")?;
    anyhow::ensure!(v.is_object(), "data.json 必须是对象");
    let name = { v.get("me").and_then(|m| m.get("name")).and_then(|n| n.as_str()).map(str::to_owned) };
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
        if let Some(p) = v
            .get("prefer_reduced_motion")
            .or_else(|| v.get("preferReducedMotion"))
            .and_then(|it| it.as_bool())
        {
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

/// Snapshot comes from the main thread, so a backup includes unsaved settings
/// and never reads the application's mutable global state on a worker.
pub fn create_backup<W: Write + Seek>(output: W, snapshot: &[u8]) -> Result<u64> {
    backup_tree(&data_dir()?, output, snapshot)
}

fn backup_tree<W: Write + Seek>(root: &Path, output: W, snapshot: &[u8]) -> Result<u64> {
    let _: Data = serde_json::from_slice(snapshot).context("备份 data.json 无法读取")?;
    let mut zip = zip::ZipWriter::new(output);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    zip.start_file("data.json", opts)?;
    zip.write_all(snapshot)?;
    let mut n = 1u64;
    for entry in WalkDir::new(root) {
        let entry = entry.context("读取备份目录失败")?;
        let path = entry.path();
        let rel = path.strip_prefix(root)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Saved JSON can lag behind the main-thread snapshot. Cache files can
        // contain transient exports and are rebuilt by the app.
        if rel == Path::new("data.json") || rel.starts_with("cache") {
            continue;
        }
        let name = rel.to_string_lossy().replace('\\', "/");
        if name.starts_with("phira-backup-") && name.ends_with(".zip") && !name.contains('/') {
            continue;
        }
        if entry.file_type().is_dir() {
            zip.add_directory(format!("{name}/"), opts)?;
        } else if entry.file_type().is_file() {
            zip.start_file(name, opts)?;
            let mut f = std::fs::File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
            n += 1;
        }
    }
    let mut output = zip.finish()?;
    output.flush()?;
    Ok(n)
}

/// Validate and fully extract before publishing. Return the restored state to
/// the main thread, so the next save cannot overwrite it with the old state.
pub fn restore_backup(src: &Path) -> Result<PreparedRestore> {
    prepare_restore(src, &data_dir()?)
}

pub struct PreparedRestore {
    root: PathBuf,
    staging: tempfile::TempDir,
    bytes: Vec<u8>,
    data: Data,
    count: u64,
}

impl PreparedRestore {
    /// Commit and replace the in-memory state together on the main thread.
    /// Dropping a pending result leaves the existing data untouched.
    pub fn publish(self) -> Result<(u64, Data)> {
        for entry in WalkDir::new(self.staging.path()).min_depth(1) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(self.staging.path())?;
            if rel == Path::new("data.json") {
                continue;
            }
            copy_atomic(entry.path(), &self.root.join(rel))?;
        }
        write_atomic(&self.root.join("data.json"), &self.bytes)?;
        Ok((self.count, self.data))
    }
}

fn prepare_restore(src: &Path, root: &Path) -> Result<PreparedRestore> {
    let file = std::fs::File::open(src)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut bytes = Vec::new();
    zip.by_name("data.json").context("备份缺少根目录 data.json")?.read_to_end(&mut bytes)?;
    let data: Data = serde_json::from_slice(&bytes).context("备份 data.json 无法读取")?;
    let staging = tempfile::tempdir_in(root.parent().context("还原目录无效")?)?;
    let mut n = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let rel = entry.enclosed_name().context("备份包含不安全的文件路径")?;
        anyhow::ensure!(!rel.as_os_str().is_empty(), "备份包含空路径");
        let to = staging.path().join(rel);
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
    Ok(PreparedRestore {
        root: root.to_path_buf(),
        staging,
        bytes,
        data,
        count: n,
    })
}

fn copy_atomic(source: &Path, dest: &Path) -> Result<()> {
    let parent = dest.parent().context("还原路径无效")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut std::fs::File::open(source)?, &mut file)?;
    file.as_file().sync_all()?;
    file.persist(dest).map_err(|err| err.error).context("还原文件失败")?;
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("保存路径无效")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|err| err.error).context("保存文件失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn backup_restores_current_settings_records_and_unicode_assets() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("charts/custom/谱面")).unwrap();
        std::fs::write(src.path().join("charts/custom/谱面/chart.json"), b"chart").unwrap();
        std::fs::write(src.path().join("data.json"), b"old invalid state").unwrap();
        let mut data = Data::default();
        data.language = Some("zh-CN".into());
        data.config.shader_pre_render = true;
        data.local_records.insert("custom/谱面".into(), None);
        let bytes = serde_json::to_vec(&data).unwrap();
        let mut archive = Cursor::new(Vec::new());
        assert_eq!(backup_tree(src.path(), &mut archive, &bytes).unwrap(), 2);
        let file = src.path().join("backup.zip");
        std::fs::write(&file, archive.into_inner()).unwrap();
        let prepared = prepare_restore(&file, dst.path()).unwrap();
        assert!(!dst.path().join("data.json").exists());
        let (n, restored) = prepared.publish().unwrap();
        assert_eq!(n, 2);
        assert_eq!(restored.language, data.language);
        assert!(restored.config.shader_pre_render);
        assert!(restored.local_records.contains_key("custom/谱面"));
        let _: Data = serde_json::from_slice(&std::fs::read(dst.path().join("data.json")).unwrap()).unwrap();
        assert_eq!(std::fs::read(dst.path().join("charts/custom/谱面/chart.json")).unwrap(), b"chart");
    }

    #[test]
    fn invalid_backup_is_rejected_before_existing_files_are_modified() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("data");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("data.json"), b"keep").unwrap();
        for invalid in [true, false] {
            let file = root.path().join("bad.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("data.json", opts).unwrap();
            zip.write_all(if invalid { b"broken" } else { b"{}" }).unwrap();
            zip.start_file("../escape", opts).unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap();
            assert!(prepare_restore(&file, &dest).is_err());
            assert_eq!(std::fs::read(dest.join("data.json")).unwrap(), b"keep");
            assert!(!root.path().join("escape").exists());
        }
    }
}
