//! Explicit copy-only boundary between RPEN projects and the native chart library.
use crate::{data::LocalChart, page::SharedState};
use anyhow::{ensure, Context, Result};
use prpr::ext::poll_future;
use rpen_host::{EditorHost, LibraryReply, LibraryRequest};
use serde_json::json;
use std::{
    collections::HashMap,
    future::Future,
    io::{Cursor, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    pin::Pin,
};
use uuid::Uuid;

enum Outcome {
    Read(Vec<u8>),
    Installed(Box<LocalChart>),
}
type Work = Pin<Box<dyn Future<Output = Result<Outcome>>>>;
#[derive(Default)]
pub(super) struct LibraryAdapter {
    keys: HashMap<String, String>,
    pending: Option<(String, Work)>,
}
impl LibraryAdapter {
    pub fn request(&mut self, host: &EditorHost, id: String, request: LibraryRequest, bytes: Option<Vec<u8>>) {
        if self.pending.is_some() {
            host.complete_library(&id, Err("谱面库操作尚未完成，请稍后重试".into()));
            return;
        }
        match request {
            LibraryRequest::List => {
                self.keys.clear();
                let charts: Vec<_> = crate::get_data()
                    .charts
                    .iter()
                    .map(|chart| {
                        let key = Uuid::new_v4().to_string();
                        self.keys.insert(key.clone(), chart.local_path.clone());
                        json!({"key":key,"name":chart.info.name,"level":chart.info.level,"charter":chart.info.charter,"composer":chart.info.composer})
                    })
                    .collect();
                host.complete_library(&id, Ok(LibraryReply::Json(json!({"charts":charts}))));
            }
            LibraryRequest::Read { key } => {
                let Some(path) = self.keys.get(&key).cloned() else {
                    host.complete_library(&id, Err("谱面列表已更新，请刷新后重新选择".into()));
                    return;
                };
                self.pending = Some((
                    id,
                    Box::pin(async move {
                        let root = PathBuf::from(crate::dir::charts()?);
                        let bytes = prpr::ext::spawn_task(move || package_chart(&root, &path)).await?;
                        Ok(Outcome::Read(bytes))
                    }),
                ));
            }
            LibraryRequest::Write { .. } => {
                self.pending = Some((
                    id,
                    Box::pin(async move { Ok(Outcome::Installed(Box::new(install_copy(bytes.context("Missing chart package")?).await?))) }),
                ));
            }
        }
    }
    pub fn update(&mut self, host: &EditorHost, shared: &mut SharedState) {
        let Some((_, task)) = &mut self.pending else {
            return;
        };
        let Some(result) = poll_future(task.as_mut()) else {
            return;
        };
        let (id, _) = self.pending.take().unwrap();
        let reply = match result {
            Ok(Outcome::Read(bytes)) => Ok(LibraryReply::Package(bytes)),
            Ok(Outcome::Installed(chart)) => {
                let name = chart.info.name.clone();
                let local_path = chart.local_path.clone();
                crate::get_data_mut().charts.push(*chart);
                match crate::save_data() {
                    Ok(()) => {
                        shared.reload_local_charts();
                        Ok(LibraryReply::Json(json!({"name":name,"created":true})))
                    }
                    Err(_) => {
                        crate::get_data_mut().charts.retain(|chart| chart.local_path != local_path);
                        let _ = remove_created_copy(&local_path);
                        Err("新谱面未能保存，请重试".into())
                    }
                }
            }
            Err(_) => Err("谱面库操作失败，请检查谱面文件是否完整后重试".into()),
        };
        host.complete_library(&id, reply);
    }
}

fn chart_path(root: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(
        !relative.is_empty() && !relative.contains(['\\', ':']) && Path::new(relative).components().all(|c| matches!(c, Component::Normal(_))),
        "Invalid chart key"
    );
    let path = root.join(relative).canonicalize()?;
    ensure!(path.starts_with(root.canonicalize()?), "Chart is outside library");
    Ok(path)
}
fn package_chart(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = chart_path(root, relative)?;
    if path.is_file() {
        ensure!(path.metadata()?.len() <= 512 * 1024 * 1024, "Chart package is too large");
        return Ok(std::fs::read(path)?);
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let info = std::fs::read(path.join("info.yml"))
        .ok()
        .map(|bytes| serde_yaml::from_slice::<prpr::info::ChartInfo>(&bytes))
        .transpose()?;
    let mut total = 0u64;
    let mut count = 0;
    for entry in walkdir::WalkDir::new(&path).follow_links(false) {
        let entry = entry?;
        ensure!(!entry.file_type().is_symlink(), "Chart contains a symbolic link");
        if !entry.file_type().is_file() {
            continue;
        }
        count += 1;
        total = total.checked_add(entry.metadata()?.len()).context("Chart size overflow")?;
        ensure!(count <= 20000 && total <= 512 * 1024 * 1024, "Chart is too large");
        let name = entry
            .path()
            .strip_prefix(&path)?
            .to_str()
            .context("Invalid chart filename")?
            .replace('\\', "/");
        if info.is_some() && name.eq_ignore_ascii_case("info.txt") {
            continue;
        }
        archive.start_file(name, options)?;
        std::io::copy(&mut std::fs::File::open(entry.path())?, &mut archive)?;
    }
    if let Some(info) = info {
        let clean = |value: &str| value.replace(['\r', '\n', '\0'], " ");
        let fields = [
            ("Name", info.name),
            ("Chart", info.chart),
            ("Song", info.music),
            ("Picture", info.illustration),
            ("Level", info.level),
            ("Composer", info.composer),
            ("Charter", info.charter),
            ("Illustration", info.illustrator),
        ];
        let text = fields
            .into_iter()
            .map(|(key, value)| format!("{key}: {}\n", clean(&value)))
            .collect::<String>();
        archive.start_file("info.txt", options)?;
        archive.write_all(text.as_bytes())?;
    }
    let bytes = archive.finish()?.into_inner();
    ensure!(bytes.len() <= 512 * 1024 * 1024, "Chart package is too large");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn repeated_edits_get_distinct_names_without_stacking_suffixes() {
        let names = ["Fixture（RPEN 编辑副本）", "Fixture（RPEN 编辑副本 2）"];
        assert_eq!(copy_name("Fixture（RPEN 编辑副本 2）", names.into_iter()), "Fixture（RPEN 编辑副本 3）");
        assert_eq!(copy_name("Fixture（RPEN 编辑副本）（RPEN 编辑副本）", names.into_iter()), "Fixture（RPEN 编辑副本 3）");
    }
    #[test]
    fn library_package_keeps_source_and_uses_native_resource_bindings() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("fixture");
        std::fs::create_dir(&source).unwrap();
        let mut info = prpr::info::ChartInfo::default();
        info.name = "Fixture".into();
        info.chart = "chart.json".into();
        info.music = "music.wav".into();
        let metadata = serde_yaml::to_string(&info).unwrap();
        std::fs::write(source.join("info.yml"), &metadata).unwrap();
        std::fs::write(source.join("info.txt"), "Name: stale\n").unwrap();
        std::fs::write(source.join("chart.json"), "{}").unwrap();
        let package = package_chart(root.path(), "fixture").unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).unwrap();
        let mut text = String::new();
        zip.by_name("info.txt").unwrap().read_to_string(&mut text).unwrap();
        assert!(text.contains("Name: Fixture\n") && text.contains("Song: music.wav\n"));
        assert_eq!(std::fs::read_to_string(source.join("info.yml")).unwrap(), metadata);
        assert_eq!(std::fs::read_to_string(source.join("info.txt")).unwrap(), "Name: stale\n");
        for path in ["", "../fixture", "fixture/../fixture", "C:/fixture", "fixture\\chart.json"] {
            assert!(chart_path(root.path(), path).is_err());
        }
    }
}
async fn install_copy(bytes: Vec<u8>) -> Result<LocalChart> {
    let mut file = tempfile::tempfile()?;
    file.write_all(&bytes)?;
    file.seek(SeekFrom::Start(0))?;
    let (directory, id) = crate::scene::gen_custom_dir()?;
    let local_path = format!("custom/{id}");
    let mut rollback = CopyDirectory {
        relative: local_path.clone(),
        keep: false,
    };
    let (mut chart, _) = crate::scene::import_chart_to(&directory, local_path, file).await?;
    let result: Result<()> = async {
        let root = PathBuf::from(crate::dir::charts()?);
        let path = chart_path(&root, &chart.local_path)?;
        let mut fs = prpr::fs::fs_from_file(&path)?;
        let mut info = prpr::fs::load_info(fs.as_mut()).await?;
        info.id = None;
        info.uploader = None;
        info.use_rpen = Some(true);
        if info.illustration.is_empty() || !fs.exists(&info.illustration).await? {
            let mut name = "rpen-cover.png".to_string();
            let mut number = 2;
            while path.join(&name).exists() {
                name = format!("rpen-cover-{number}.png");
                number += 1;
            }
            image::RgbaImage::from_pixel(16, 9, image::Rgba([24, 28, 36, 255])).save(path.join(&name))?;
            info.illustration = name;
        }
        info.name = copy_name(&info.name, crate::get_data().charts.iter().map(|chart| chart.info.name.as_str()));
        crate::transfer::write_atomic(&path.join("info.yml"), serde_yaml::to_string(&info)?.as_bytes())?;
        chart.info = info.into();
        Ok(())
    }
    .await;
    result?;
    rollback.keep = true;
    Ok(chart)
}
fn copy_name<'a>(name: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let mut stem = name;
    while let Some((base, suffix)) = stem.rsplit_once("（RPEN 编辑副本") {
        let Some(number) = suffix.strip_suffix('）') else {
            break;
        };
        if !number.is_empty() && number.trim().parse::<usize>().is_err() {
            break;
        }
        stem = base;
    }
    let names: std::collections::HashSet<_> = existing.collect();
    let mut candidate = format!("{stem}（RPEN 编辑副本）");
    let mut number = 2;
    while names.contains(candidate.as_str()) {
        candidate = format!("{stem}（RPEN 编辑副本 {number}）");
        number += 1;
    }
    candidate
}
struct CopyDirectory {
    relative: String,
    keep: bool,
}
impl Drop for CopyDirectory {
    fn drop(&mut self) {
        if !self.keep {
            let _ = remove_created_copy(&self.relative);
        }
    }
}
fn remove_created_copy(relative: &str) -> Result<()> {
    let suffix = relative.strip_prefix("custom/").context("Invalid copy directory")?;
    Uuid::parse_str(suffix)?;
    let root = PathBuf::from(crate::dir::charts()?);
    let path = chart_path(&root, relative)?;
    std::fs::remove_dir_all(path)?;
    Ok(())
}
