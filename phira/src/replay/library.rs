//! Local bindings are deliberately outside the portable replay. Imported scores
//! never enter history.json, personal bests, RKS or the upload pipeline.
use super::{ChartRef, Metadata, Replay};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static REVISION: AtomicU64 = AtomicU64::new(0);
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}
fn root() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join("replays"))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub file: String,
    pub imported_at: i64,
    pub local_path: Option<String>,
    pub verified: bool,
    pub replay_id: Option<String>,
    pub event_hash: Option<String>,
    pub player: Option<String>,
    pub player_id: Option<i32>,
}
#[derive(Clone)]
pub struct Item {
    pub path: PathBuf,
    pub binding: Option<Binding>,
    pub meta: Metadata,
    pub record: Option<crate::history::Record>,
    pub mode: String,
    pub speed: Option<f32>,
    pub rules: Option<String>,
}
impl Item {
    pub fn imported(&self) -> bool {
        self.binding.as_ref().is_some_and(|b| b.imported_at > 0) || self.path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("import-"))
    }
    pub fn player(&self) -> &str {
        self.meta.player.as_deref().filter(|v| !v.is_empty()).unwrap_or("未记录")
    }
}
fn index(root: &Path) -> Result<Vec<Binding>> {
    let path = root.join("index.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&std::fs::read(path)?).context("读取回放索引失败")
}
fn write_index(root: &Path, bindings: &[Binding]) -> Result<()> {
    crate::transfer::write_atomic(&root.join("index.json"), &serde_json::to_vec_pretty(bindings)?)?;
    REVISION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
pub fn all() -> Result<Vec<Item>> {
    let root = root()?;
    let bindings = index(&root)?;
    let history = crate::history::all();
    let mut items = Vec::new();
    if !root.exists() {
        return Ok(items);
    }
    for entry in std::fs::read_dir(&root)? {
        let path = entry?.path();
        if !path.is_file() || !matches!(path.extension().and_then(|s| s.to_str()), Some("phirar" | "phirarec")) {
            continue;
        }
        match super::load(&path) {
            Ok(replay) => {
                let record = replay
                    .meta
                    .result
                    .clone()
                    .or_else(|| history.iter().find(|r| super::path_for(&r.key, r.time) == path).cloned());
                let mut meta = replay.meta;
                if let Some(record) = &record {
                    if meta.name.is_empty() {
                        meta.name = record.name.clone();
                        meta.level = record.level.clone();
                    }
                    if meta.recorded_at.is_none() && record.time > 0 {
                        meta.recorded_at = Some(record.time);
                    }
                }
                let mode = if replay.grading.detailed {
                    "细致判定"
                } else if let Some(settings) = &replay.settings {
                    match settings.algorithm {
                        prpr::config::JudgeAlgorithm::Phigros => "Phigros",
                        _ => "Phira Pro",
                    }
                } else {
                    "规则未记录"
                }
                .to_owned();
                let rules = replay
                    .settings
                    .as_ref()
                    .and_then(|s| serde_json::to_vec(&(s, replay.theoretical_score, replay.mods, replay.speed)).ok())
                    .map(|b| format!("{:x}", Sha256::digest(b)));
                items.push(Item {
                    binding: bindings
                        .iter()
                        .find(|b| Some(b.file.as_str()) == path.file_name().and_then(|v| v.to_str()))
                        .cloned(),
                    path,
                    meta,
                    record,
                    mode,
                    speed: replay.has_speed.then_some(replay.speed),
                    rules,
                });
            }
            Err(err) => tracing::warn!(?path, ?err, "invalid replay in library"),
        }
    }
    items.sort_by_key(|r| std::cmp::Reverse(r.meta.recorded_at.unwrap_or(0)));
    Ok(items)
}
fn import_into(root: &Path, source: &Path) -> Result<(PathBuf, bool)> {
    // Normalize legacy containers while preserving all "not recorded" flags.
    // Identity and tape content are both checked; a collision never overwrites another actor.
    let mut replay = super::load(source)?;
    if replay.meta.id.is_empty() {
        replay.meta.id = format!("legacy-{:x}", Sha256::digest(std::fs::read(source)?));
    }
    let event_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            &replay.chart,
            replay.offset,
            replay.speed,
            replay.mods,
            &replay.grading,
            &replay.settings,
            &replay.touches,
            &replay.judges,
            &replay.frames
        ))?)
    );
    let bytes = super::bytes(&replay)?;
    let file = format!("import-{:x}.phirar", Sha256::digest(&bytes));
    std::fs::create_dir_all(root)?;
    let path = root.join(&file);
    let mut bindings = index(root)?;
    if let Some(existing) = bindings.iter().find(|b| {
        b.replay_id.as_deref() == Some(&replay.meta.id)
            && b.event_hash.as_ref() == Some(&event_hash)
            && b.player == replay.meta.player
            && b.player_id == replay.meta.player_id
            && root.join(&b.file).is_file()
    }) {
        return Ok((root.join(&existing.file), false));
    }
    if bindings.iter().any(|b| b.file == file) && path.is_file() {
        return Ok((path, false));
    }
    crate::transfer::write_atomic(&path, &bytes)?;
    bindings.retain(|b| b.file != file);
    bindings.push(Binding {
        file,
        imported_at: chrono::Utc::now().timestamp_millis(),
        replay_id: Some(replay.meta.id),
        event_hash: Some(event_hash),
        player: replay.meta.player,
        player_id: replay.meta.player_id,
        ..Default::default()
    });
    write_index(root, &bindings)?;
    Ok((path, true))
}
pub fn import(source: &Path) -> Result<(PathBuf, bool)> {
    import_into(&root()?, source)
}
pub fn bind(path: &Path, local_path: &str, verified: bool) -> Result<()> {
    let root = root()?;
    let mut bindings = index(&root)?;
    anyhow::ensure!(path.parent() == Some(root.as_path()) && path.is_file(), "未找到本地回放");
    let file = path.file_name().context("缺少回放文件名")?.to_string_lossy().into_owned();
    if !bindings.iter().any(|b| b.file == file) {
        bindings.push(Binding {
            file: file.clone(),
            ..Default::default()
        });
    }
    let binding = bindings.iter_mut().find(|b| b.file == file).unwrap();
    binding.local_path = Some(local_path.to_owned());
    binding.verified = verified;
    write_index(&root, &bindings)
}
pub fn delete(path: &Path) -> Result<()> {
    let root = root()?;
    anyhow::ensure!(path.parent() == Some(root.as_path()), "回放路径不在管理目录内");
    std::fs::remove_file(path)?;
    let mut bindings = index(&root)?;
    bindings.retain(|b| Some(b.file.as_str()) != path.file_name().and_then(|v| v.to_str()));
    write_index(&root, &bindings)
}
pub async fn fingerprint(fs: &mut dyn prpr::fs::FileSystem, info: &prpr::info::ChartInfo) -> Result<String> {
    let chart = prpr::scene::GameScene::load_chart_bytes(fs, info).await?;
    let extra = fs.load_file("extra.json").await.ok();
    let mut hash = Sha256::new();
    hash.update(b"PhiraReplayChart1\0");
    hash.update((chart.len() as u64).to_le_bytes());
    hash.update(&chart);
    if let Some(extra) = extra {
        hash.update((extra.len() as u64).to_le_bytes());
        hash.update(extra);
    }
    hash.update(info.offset.to_le_bytes());
    Ok(format!("{:x}", hash.finalize()))
}
pub async fn audio_fingerprint(fs: &mut dyn prpr::fs::FileSystem, info: &prpr::info::ChartInfo) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs.load_file(&info.music).await?)))
}
pub async fn record_identity(fs: &mut dyn prpr::fs::FileSystem, info: &prpr::info::ChartInfo) -> Result<()> {
    let fingerprint = fingerprint(fs, info).await?;
    let audio_fingerprint = audio_fingerprint(fs, info).await?;
    super::NEXT_IDENTITY.with(|v| {
        *v.borrow_mut() = Some(Metadata {
            name: info.name.clone(),
            level: info.level.clone(),
            difficulty: info.difficulty,
            fingerprint: Some(fingerprint),
            audio_fingerprint: Some(audio_fingerprint),
            server: info.id.map(|_| crate::client::api_url().to_string()),
            ..Default::default()
        })
    });
    Ok(())
}
#[derive(Debug, PartialEq)]
pub enum Match {
    Found(String, bool),
    Missing,
    WrongVersion,
    WrongAudio,
    WrongServer,
}
pub async fn match_chart(replay: &Replay, preferred: Option<&str>, candidates: &[String]) -> Result<Match> {
    let wrong_server = matches!(replay.chart, Some(ChartRef::Id(_)))
        && replay
            .meta
            .server
            .as_ref()
            .is_some_and(|s| s.trim_end_matches('/') != crate::client::api_url().trim_end_matches('/'));
    let original = match &replay.chart {
        Some(ChartRef::Local(p)) if candidates.contains(p) => Some(p.clone()),
        Some(ChartRef::Id(id)) if *id > 0 => Some(format!("download/{id}")),
        _ => None,
    };
    let mut paths: Vec<String> = preferred
        .into_iter()
        .map(str::to_owned)
        .chain(original)
        .chain(candidates.iter().cloned())
        .collect();
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    let mut wrong = false;
    let mut wrong_audio = false;
    for path in paths {
        let Ok(mut fs) = crate::scene::fs_from_path(&path) else { continue };
        let Ok(info) = prpr::fs::load_info(fs.as_mut()).await else { continue };
        if let Some(expected) = &replay.meta.fingerprint {
            if fingerprint(fs.as_mut(), &info).await.is_ok_and(|h| &h == expected) {
                if let Some(expected) = &replay.meta.audio_fingerprint {
                    if !audio_fingerprint(fs.as_mut(), &info).await.is_ok_and(|h| &h == expected) {
                        wrong_audio = true;
                        continue;
                    }
                }
                return Ok(Match::Found(path, true));
            }
            if matches!(replay.chart, Some(ChartRef::Id(id)) if info.id == Some(id)) || preferred == Some(path.as_str()) {
                wrong = true;
            }
        } else if preferred == Some(path.as_str())
            || (!wrong_server
                && match &replay.chart {
                    Some(ChartRef::Local(p)) => p == &path,
                    Some(ChartRef::Id(id)) => info.id == Some(*id),
                    None => false,
                })
        {
            if replay.chart_updated.is_some() && info.chart_updated.map(|v| v.timestamp_millis()) != replay.chart_updated {
                wrong = true;
                continue;
            }
            return Ok(Match::Found(path, false));
        }
    }
    Ok(if wrong_audio {
        Match::WrongAudio
    } else if wrong {
        Match::WrongVersion
    } else if wrong_server {
        Match::WrongServer
    } else {
        Match::Missing
    })
}
pub fn board_records(key: &str) -> Vec<crate::history::Record> {
    let mut records: Vec<_> = crate::history::all().into_iter().filter(|r| r.key == key).collect();
    for item in all().unwrap_or_default() {
        if !item.imported() {
            if let Some(record) = records.iter_mut().find(|r| super::path_for(&r.key, r.time) == item.path) {
                record.replay_player = item.meta.player;
                record.replay_mode = Some(item.mode);
                record.replay_speed = item.speed;
                record.replay_rules = item.rules;
            }
            continue;
        }
        let Some(binding) = &item.binding else { continue };
        let Some(local) = &binding.local_path else { continue };
        let chart_id = crate::get_data().charts.iter().find(|c| &c.local_path == local).and_then(|c| c.info.id);
        if super::key_for(chart_id, Some(local)) != key {
            continue;
        }
        {
            let mut record = item.record.unwrap_or_else(|| crate::history::Record {
                name: item.meta.name.clone(),
                level: item.meta.level.clone(),
                difficulty: item.meta.difficulty,
                time: item.meta.recorded_at.unwrap_or(0),
                replay_result_unknown: true,
                ..Default::default()
            });
            record.key = key.to_owned();
            record.replay_file = Some(item.path.to_string_lossy().into_owned());
            record.replay_player = item.meta.player;
            record.replay_imported = true;
            record.replay_mode = Some(item.mode);
            record.replay_speed = item.speed;
            record.replay_rules = item.rules;
            records.push(record);
        }
    }
    records
}

/// Validate a downloaded historical package in a temporary directory before
/// adding it to the local chart library. A wrong response never replaces a chart.
pub async fn install_archived(replay: &Replay, mut file: std::fs::File) -> Result<crate::data::LocalChart> {
    use std::io::{Seek, SeekFrom};
    anyhow::ensure!(replay.meta.fingerprint.is_some() || replay.chart_updated.is_some(), "旧回放没有可用于校验录制版本的信息");
    let staging = tempfile::tempdir_in(crate::dir::cache()?)?;
    let dir = prpr::dir::Dir::new(staging.path())?;
    prpr::ext::unzip_into(std::io::BufReader::new(&mut file), &dir, true)?;
    let mut fs = prpr::fs::fs_from_file(staging.path())?;
    let info = prpr::fs::load_info(fs.as_mut()).await?;
    if let Some(expected) = &replay.meta.fingerprint {
        anyhow::ensure!(fingerprint(fs.as_mut(), &info).await? == *expected, "下载的谱面不是录制版本");
    } else {
        anyhow::ensure!(info.chart_updated.map(|v| v.timestamp_millis()) == replay.chart_updated, "下载的谱面更新时间与录制不一致");
    }
    if let Some(expected) = &replay.meta.audio_fingerprint {
        anyhow::ensure!(audio_fingerprint(fs.as_mut(), &info).await? == *expected, "下载的音乐与录制不一致");
    }
    file.seek(SeekFrom::Start(0))?;
    let (mut chart, _) = crate::scene::import_chart(file).await?;
    // Historical copies must not replace the current online chart or its PB.
    chart.info.id = None;
    chart.info.name.push_str("（录制版本）");
    let path = format!("{}/{}/info.yml", crate::dir::charts()?, chart.local_path);
    let mut local_info = info;
    local_info.id = None;
    local_info.name = chart.info.name.clone();
    crate::transfer::write_atomic(Path::new(&path), serde_yaml::to_string(&local_info)?.as_bytes())?;
    Ok(chart)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_is_separate_deduplicated_and_atomic() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("replays");
        let source = temp.path().join("other.phirar");
        super::super::save(
            &Replay {
                speed: 1.,
                meta: Metadata {
                    id: "same-id".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &source,
        )
        .unwrap();
        let (path, added) = import_into(&root, &source).unwrap();
        assert!(added);
        assert!(!import_into(&root, &source).unwrap().1);
        assert_eq!(index(&root).unwrap().len(), 1);
        let mut other = super::super::load(&path).unwrap();
        other.meta.player = Some("Another player".into());
        super::super::save(&other, &source).unwrap();
        let (other_path, added) = import_into(&root, &source).unwrap();
        assert!(added);
        assert_ne!(other_path, path);
        assert_eq!(index(&root).unwrap().len(), 2);
        assert!(index(&root).unwrap()[0].local_path.is_none());
        assert!(super::super::load(&path).is_ok());
        assert!(!temp.path().join("history.json").exists());
        std::fs::write(&source, b"invalid").unwrap();
        assert!(import_into(&root, &source).is_err());
        assert_eq!(index(&root).unwrap().len(), 2);
    }
}
