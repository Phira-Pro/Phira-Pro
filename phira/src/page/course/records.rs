//! Persistent course results, separate from single-chart PBs and ordinary history.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ChartScore {
    pub name: String,
    pub level: String,
    pub score: u32,
    pub accuracy: f64,
    pub counts: [u32; 8],
    pub early: u32,
    pub late: u32,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Record {
    pub id: String,
    pub time: i64,
    pub score: u32,
    pub difficulty: u32,
    pub grade: String,
    pub full_combo: bool,
    pub charts: Vec<ChartScore>,
}
pub(crate) fn load_at(root: &Path) -> Result<Vec<Record>> {
    let path = root.join("course-history.json");
    let mut records: Vec<Record> = if path.exists() {
        serde_json::from_slice(&std::fs::read(&path)?).context("读取课题成绩历史失败")?
    } else {
        Vec::new()
    };
    let legacy = root.join("course-rating.json");
    if legacy.exists() {
        let mut record: Record = serde_json::from_slice(&std::fs::read(legacy)?).context("读取旧课题成绩失败")?;
        record.id = format!("legacy-{}", record.time);
        if !records.iter().any(|r| r.id == record.id) {
            records.push(record);
        }
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.time));
    Ok(records)
}
pub(crate) fn save_at(root: &Path, record: &Record) -> Result<()> {
    let mut records = load_at(root)?;
    if !records.iter().any(|r| r.id == record.id) {
        records.push(record.clone());
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.time));
    records.truncate(3000);
    crate::transfer::write_atomic(&root.join("course-history.json"), &serde_json::to_vec_pretty(&records)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_old_rating_and_deduplicates_repeat_save() {
        let root = tempfile::tempdir().unwrap();
        let legacy = Record {
            time: 1,
            score: 2_850_000,
            ..Default::default()
        };
        std::fs::write(root.path().join("course-rating.json"), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let new = Record {
            id: "session-id".into(),
            time: 2,
            score: 3_000_000,
            ..Default::default()
        };
        save_at(root.path(), &new).unwrap();
        save_at(root.path(), &new).unwrap();
        let records = load_at(root.path()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].score, 3_000_000);
        assert_eq!(records[1].score, 2_850_000);
    }
    #[test]
    fn damaged_history_is_never_overwritten_on_save() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("course-history.json");
        std::fs::write(&path, "damaged").unwrap();
        assert!(save_at(root.path(), &Record::default()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "damaged");
    }
}
