//! 本地成绩历史。
//!
//! 每次游玩结束后写一条记录到 `<data>/history.json`，给「成绩历史」页用
//! （列表 / 趋势图 / PB 对比 / 判定分布对比）。
//!
//! 设计上有意保持简单：
//! - 一个 JSON 文件装全部记录，超过 [`MAX_RECORDS`] 条时丢掉最旧的；
//! - 只在第一次访问时读盘，之后走内存，写入后立刻落盘（一次游玩写一次，不心疼）；
//! - 记录里的 `hist` 是判定误差直方图（早 ← → 晚），和结算画面那张图同源。
//!
//! 移植自上游改版 Phirc Mod++ 的 `phira/src/history.rs`。

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::dir;

/// 最多保留多少条记录（大概够两年的量）。
pub const MAX_RECORDS: usize = 3000;

/// 一条游玩记录。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    /// 结束时的 Unix 毫秒时间戳。
    pub time: i64,
    /// 谱面标识：官方谱用 `id:<id>`，本地导入谱用 `local:<local_path>`，
    /// 都拿不到时退化成 `name:<名称>|<难度>`，用来在趋势图里把同一首归到一起。
    pub key: String,
    pub name: String,
    pub level: String,
    pub difficulty: f32,
    pub score: u32,
    pub accuracy: f64,
    pub max_combo: u32,
    pub num_of_notes: u32,
    /// Perfect / Good / Bad / Miss（Perfect+ 已并入 Perfect）
    pub counts: [u32; 4],
    /// 判定误差分布（早 ← → 晚）
    pub hist: Vec<u32>,
}

impl Record {
    pub fn is_full_combo(&self) -> bool {
        self.num_of_notes > 0 && self.max_combo >= self.num_of_notes
    }

    /// 全 Perfect（收歌）。
    #[allow(dead_code)]
    pub fn is_all_perfect(&self) -> bool {
        self.num_of_notes > 0 && self.counts[0] >= self.num_of_notes
    }

    /// `YYYY-MM-DD HH:MM`（本地时区）。
    pub fn time_text(&self) -> String {
        use chrono::{Local, TimeZone};
        match Local.timestamp_millis_opt(self.time).single() {
            Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
            None => "-".to_owned(),
        }
    }
}

static RECORDS: Lazy<Mutex<Option<Vec<Record>>>> = Lazy::new(|| Mutex::new(None));

fn path() -> Result<String> {
    Ok(format!("{}/history.json", dir::root()?))
}

fn load_inner() -> Vec<Record> {
    (|| -> Result<Vec<Record>> {
        let path = path()?;
        if !std::path::Path::new(&path).exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&text).with_context(|| format!("failed to parse {path}"))?)
    })()
    .unwrap_or_default()
}

fn with_records<T>(f: impl FnOnce(&mut Vec<Record>) -> T) -> T {
    let mut guard = RECORDS.lock().unwrap();
    let records = guard.get_or_insert_with(load_inner);
    f(records)
}

pub fn ensure_loaded() {
    with_records(|_| ());
}

/// 全部记录（按时间从新到旧）。
pub fn all() -> Vec<Record> {
    with_records(|it| {
        let mut v = it.clone();
        v.sort_by_key(|it| std::cmp::Reverse(it.time));
        v
    })
}

/// 写入一条记录并落盘。
pub fn push(record: Record) -> Result<()> {
    with_records(|it| {
        it.push(record);
        if it.len() > MAX_RECORDS {
            let extra = it.len() - MAX_RECORDS;
            it.drain(..extra);
        }
    });
    save()
}

pub fn save() -> Result<()> {
    let text = with_records(|it| serde_json::to_string(it))?;
    let path = path()?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {path}"))?;
    Ok(())
}

/// 导出成 JSON 文本。
pub fn export_json() -> Result<String> {
    ensure_loaded();
    let text = with_records(|it| serde_json::to_string_pretty(it))?;
    Ok(text)
}

/// 从 JSON 文本导入（按 `time + key + score` 去重合并），返回新增条数。
pub fn import_json(text: &str) -> Result<usize> {
    let incoming: Vec<Record> = serde_json::from_str(text).context("invalid score history JSON")?;
    let mut added = 0;
    with_records(|it| {
        for rec in incoming {
            let dup = it.iter().any(|x| x.time == rec.time && x.key == rec.key && x.score == rec.score);
            if !dup {
                it.push(rec);
                added += 1;
            }
        }
        it.sort_by_key(|it| it.time);
        if it.len() > MAX_RECORDS {
            let extra = it.len() - MAX_RECORDS;
            it.drain(..extra);
        }
    });
    if added > 0 {
        save()?;
    }
    Ok(added)
}

/// 记录一次游玩（结算时调用）。
#[allow(clippy::too_many_arguments)]
pub fn record_play(
    chart_id: Option<i32>,
    local_path: Option<&str>,
    name: &str,
    level: &str,
    difficulty: f32,
    score: u32,
    accuracy: f64,
    max_combo: u32,
    num_of_notes: u32,
    counts: [u32; 4],
    hist: &[u32],
) -> Result<()> {
    let key = if let Some(id) = chart_id {
        format!("id:{id}")
    } else if let Some(path) = local_path {
        format!("local:{path}")
    } else {
        format!("name:{name}|{difficulty:.2}")
    };
    push(Record {
        time: chrono::Utc::now().timestamp_millis(),
        key,
        name: name.to_owned(),
        level: level.to_owned(),
        difficulty,
        score,
        accuracy,
        max_combo,
        num_of_notes,
        counts,
        hist: hist.to_vec(),
    })
}
