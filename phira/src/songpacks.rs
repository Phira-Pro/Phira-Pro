//! Local song packs are references to library charts, independent of favourites.
use crate::{data::LocalChart, dir};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct SongPack {
    pub id: Uuid,
    pub name: String,
    pub charts: Vec<String>,
}
pub fn load() -> Result<Vec<SongPack>> {
    let path = PathBuf::from(dir::root()?).join("songpacks.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn save(packs: &[SongPack]) -> Result<()> {
    let path = PathBuf::from(dir::root()?).join("songpacks.json");
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(packs)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}
pub fn create(name: String, charts: Vec<String>) -> Result<SongPack> {
    let mut seen = std::collections::HashSet::new();
    let charts: Vec<_> = charts.into_iter().filter(|p| seen.insert(p.clone())).collect();
    if charts.is_empty() {
        bail!("请至少选择一张谱面");
    }
    let name = name.trim();
    if name.is_empty() {
        bail!("请输入曲包名称");
    }
    let pack = SongPack {
        id: Uuid::new_v4(),
        name: name.chars().take(80).collect(),
        charts,
    };
    let mut packs = load()?;
    packs.push(pack.clone());
    save(&packs)?;
    Ok(pack)
}

pub fn paths_for_refs(charts: &[crate::client::ChartRef]) -> Result<Vec<String>> {
    charts
        .iter()
        .map(|chart| {
            chart
                .find_local_path()?
                .map(|p| p.into_owned())
                .context("选中的谱面已不在本地，请刷新后重试")
        })
        .collect()
}

/// Import each nested chart through the production importer; returned charts are
/// registered on the UI thread only after the entire archive has succeeded.
pub struct ImportResult {
    pub name: String,
    pub charts: Vec<LocalChart>,
    pub warnings: Vec<String>,
}
pub async fn import(path: PathBuf) -> Result<ImportResult> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("新曲包").to_owned();
    let stage = tempfile::tempdir()?;
    let directory = prpr::dir::Dir::new(stage.path())?;
    prpr::ext::unzip_into(File::open(path)?, &directory, false)?;
    let mut archives = Vec::new();
    let mut roots = std::collections::BTreeSet::new();
    let mut raw = Vec::new();
    for entry in walkdir::WalkDir::new(stage.path()) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(ext.as_str(), "pez" | "zip") {
            archives.push(path.to_owned());
        } else if matches!(file, "info.yml" | "info.yaml" | "info.txt" | "info.csv") {
            roots.insert(path.parent().unwrap().to_owned());
        } else if ext == "pec" {
            raw.push(path.to_owned());
        } else if ext == "json" {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&std::fs::read(path)?) {
                if value.get("judgeLineList").is_some() {
                    raw.push(path.to_owned());
                }
            }
        }
    }
    let mut packages: Vec<(File, String)> = Vec::new();
    archives.sort();
    for archive in archives {
        packages.push((File::open(&archive)?, archive.file_name().unwrap().to_string_lossy().into_owned()));
    }
    // Prefer explicit chart metadata; do not import its chart JSON a second time.
    let roots: Vec<_> = roots
        .iter()
        .filter(|p| !roots.iter().any(|parent| parent != *p && p.starts_with(parent)))
        .cloned()
        .collect();
    for root in &roots {
        let mut file = tempfile::tempfile()?;
        crate::scene::compress_folder(root, &mut file)?;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        packages.push((file, root.file_name().unwrap_or_default().to_string_lossy().into_owned()));
    }
    for chart in raw.into_iter().filter(|p| !roots.iter().any(|r| p.starts_with(r))) {
        let root = chart.parent().unwrap();
        let find = |extensions: &[&str]| -> Result<String> {
            let mut candidates = Vec::new();
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                let p = entry.path();
                if p.extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
                {
                    candidates.push(p);
                }
            }
            let stem = chart.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let exact = candidates
                .iter()
                .find(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(stem)));
            let chosen = exact.or_else(|| (candidates.len() == 1).then(|| &candidates[0]));
            if let Some(path) = chosen {
                return Ok(path.file_name().unwrap().to_string_lossy().into_owned());
            }
            bail!("{} 的配套音乐或曲绘缺失或不明确；请使用同名媒体文件、独立谱面目录或 info.yml", chart.display())
        };
        let info = prpr::info::ChartInfo {
            name: chart.file_stem().unwrap().to_string_lossy().into_owned(),
            chart: chart.file_name().unwrap().to_string_lossy().into_owned(),
            music: find(&["ogg", "mp3", "wav", "flac", "m4a"])?,
            illustration: find(&["png", "jpg", "jpeg", "webp"])?,
            ..Default::default()
        };
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            if entry.file_type().is_file() {
                output.start_file(entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/"), zip::write::SimpleFileOptions::default())?;
                std::io::copy(&mut File::open(entry.path())?, &mut output)?;
            }
        }
        output.start_file("info.yml", zip::write::SimpleFileOptions::default())?;
        output.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
        let bytes = output.finish()?.into_inner();
        let mut file = tempfile::tempfile()?;
        file.write_all(&bytes)?;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        packages.push((file, info.name));
    }
    if packages.is_empty() {
        bail!("压缩包中未找到可导入的谱面");
    }
    let mut charts: Vec<LocalChart> = Vec::new();
    let mut notices = std::collections::BTreeSet::new();
    for (file, source) in packages {
        match crate::scene::import_chart(file)
            .await
            .with_context(|| format!("无法导入曲包内的 {source}"))
        {
            Ok((chart, warnings)) => {
                if let Some(message) = crate::scene::parse_warnings_to_string(&warnings) {
                    notices.insert(message);
                }
                charts.push(chart);
            }
            Err(err) => {
                // Only remove new UUID directories allocated by this operation.
                for chart in &charts {
                    let custom = PathBuf::from(dir::charts()?).join("custom").canonicalize()?;
                    let target = Path::new(&dir::charts()?).join(&chart.local_path).canonicalize()?;
                    if target.starts_with(&custom) && target != custom {
                        let _ = std::fs::remove_dir_all(target);
                    }
                }
                return Err(err);
            }
        }
    }
    Ok(ImportResult {
        name,
        charts,
        warnings: notices.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_nested_archives_register_persist_and_reject_traversal() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let root = tempfile::tempdir().unwrap();
        let previous = crate::DATA_PATH.lock().unwrap().replace(root.path().to_string_lossy().into_owned());
        rt.block_on(async {
            let chart = serde_json::json!({"formatVersion":3,"offset":0,"numOfNotes":0,"judgeLineList":[]});
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let info = prpr::info::ChartInfo {
                name: "Pack importer QA".into(),
                music: "song.wav".into(),
                ..Default::default()
            };
            for (name, bytes) in [
                ("info.yml", serde_yaml::to_string(&info).unwrap().into_bytes()),
                ("chart.json", serde_json::to_vec(&chart).unwrap()),
                ("song.wav", vec![0; 4]),
                ("background.png", include_bytes!("../../assets/icon.png").to_vec()),
            ] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(&bytes).unwrap();
            }
            let inner = zip.finish().unwrap().into_inner();
            let input = root.path().join("测试曲包.zip");
            let mut outer = zip::ZipWriter::new(File::create(&input).unwrap());
            for name in ["wrapper/one.pez", "wrapper/two.zip"] {
                outer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                outer.write_all(&inner).unwrap();
            }
            outer.finish().unwrap();
            let ImportResult { name, charts, .. } = import(input).await.unwrap();
            assert_eq!(name, "测试曲包");
            assert_eq!(charts.len(), 2);
            let paths: Vec<_> = charts.iter().map(|c| c.local_path.clone()).collect();
            assert_ne!(paths[0], paths[1]);
            let pack = create(name, vec![paths[0].clone(), paths[1].clone(), paths[0].clone()]).unwrap();
            assert_eq!(pack.charts, paths);
            assert_eq!(load().unwrap()[0].charts, paths);
            let bad = root.path().join("bad.zip");
            let mut zip = zip::ZipWriter::new(File::create(&bad).unwrap());
            zip.start_file("../../escape.pez", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(&inner).unwrap();
            zip.finish().unwrap();
            assert!(import(bad).await.is_err());
            assert!(paths.iter().all(|p| Path::new(&dir::charts().unwrap()).join(p).exists()));
            let raw = root.path().join("raw.zip");
            let mut zip = zip::ZipWriter::new(File::create(&raw).unwrap());
            for (name, bytes) in [
                ("raw/map.pec", b"0\nbp 0 120\nn1 0 2 1024 1 0\n".to_vec()),
                ("raw/song.wav", vec![0; 4]),
                ("raw/bg.png", include_bytes!("../../assets/icon.png").to_vec()),
            ] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(&bytes).unwrap();
            }
            zip.finish().unwrap();
            let imported = import(raw).await.unwrap();
            assert_eq!(imported.charts.len(), 1);
            let failed = root.path().join("partial.zip");
            let mut zip = zip::ZipWriter::new(File::create(&failed).unwrap());
            for (name, bytes) in [("01.pez", inner.clone()), ("02.pez", vec![1, 2, 3])] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(&bytes).unwrap();
            }
            zip.finish().unwrap();
            let custom = PathBuf::from(dir::charts().unwrap()).join("custom");
            let before = std::fs::read_dir(&custom).unwrap().count();
            assert!(import(failed).await.is_err());
            assert_eq!(std::fs::read_dir(custom).unwrap().count(), before);
        });
        *crate::DATA_PATH.lock().unwrap() = previous;
    }
}
