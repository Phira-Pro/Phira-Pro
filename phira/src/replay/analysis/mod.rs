//! Offline, observer-only replay jobs. Feature extraction and rendering have no UI state.
mod chart;
mod features;
mod report;
mod samples;
mod stats;
#[cfg(test)]
mod tests;
pub mod worker;

use super::Replay;
use crate::plugins::replay_analysis::Analyzer;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

pub const FEATURE_SCHEMA: &str = "replay-observations-1";
const MAX_JOB_SECONDS: u64 = 120;

#[derive(Clone)]
pub struct Control {
    cancelled: Arc<AtomicBool>,
    stage: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    started: Instant,
    detail: Arc<Mutex<String>>,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            stage: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            started: Instant::now(),
            detail: Arc::new(Mutex::new(String::new())),
        }
    }
}
impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<()> {
        anyhow::ensure!(!self.cancelled.load(Ordering::Relaxed), "分析已取消");
        anyhow::ensure!(self.started.elapsed().as_secs() < MAX_JOB_SECONDS, "分析超过 120 秒预算，未生成完整结论");
        Ok(())
    }
    pub fn set_stage(&self, stage: usize) -> Result<()> {
        self.check()?;
        self.completed.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.stage.store(stage, Ordering::Relaxed);
        self.detail.lock().unwrap().clear();
        Ok(())
    }
    pub fn detail(&self) -> String {
        self.detail.lock().unwrap().clone()
    }
    pub(super) fn describe(&self, text: impl Into<String>) -> Result<()> {
        self.check()?;
        *self.detail.lock().unwrap() = text.into();
        Ok(())
    }
    pub fn progress(&self, completed: usize, total: usize) -> Result<()> {
        self.check()?;
        self.total.store(total, Ordering::Relaxed);
        self.completed.store(completed.min(total), Ordering::Relaxed);
        Ok(())
    }
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        let part = if total > 0 {
            self.completed.load(Ordering::Relaxed).min(total) as f32 / total as f32
        } else {
            0.
        };
        ((self.stage.load(Ordering::Relaxed) as f32 + part) / 6.).clamp(0., 1.)
    }
    pub fn stage_number(&self) -> usize {
        self.stage.load(Ordering::Relaxed).min(5) + 1
    }
    pub fn stage(&self) -> &'static str {
        if self.stage.load(Ordering::Relaxed) == 0
            && self.total.load(Ordering::Relaxed) > 0
            && self.completed.load(Ordering::Relaxed) >= self.total.load(Ordering::Relaxed)
        {
            return "解析回放";
        }
        ["读取回放", "校验录制谱面", "提取操作特征", "执行插件检测", "生成报告图片", "保存报告"][self.stage.load(Ordering::Relaxed).min(5)]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Series {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub points: Vec<[f64; 2]>,
    pub scatter: bool,
    pub observed: usize,
}

pub struct Dataset {
    pub replay: Arc<Replay>,
    pub payload: Value,
    pub series: Vec<Series>,
    pub quality: Vec<String>,
    pub hash: String,
}
impl Dataset {
    pub fn observation_page(&self, kind: &str, start: i64, count: i64) -> Vec<Value> {
        if !matches!(kind, "timing" | "duration") || start < 0 || !(1..=256).contains(&count) {
            return vec![];
        }
        let Some(blocks) = self.payload["compact"][kind].as_array() else {
            return vec![];
        };
        blocks[safe_page_bounds(start, count, blocks.len())].to_vec()
    }
    pub fn plugin_payload(&self) -> Value {
        let mut payload = serde_json::Map::new();
        for (key, value) in self.payload.as_object().unwrap() {
            let value = if key == "compact" {
                Value::Object(
                    value
                        .as_object()
                        .unwrap()
                        .iter()
                        .filter(|(k, _)| !matches!(k.as_str(), "timing" | "duration"))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            } else {
                value.clone()
            };
            payload.insert(key.clone(), value);
        }
        Value::Object(payload)
    }
    pub fn input_page(&self, start: i64, count: i64) -> Vec<Value> {
        if start < 0 || !(1..=256).contains(&count) {
            return vec![];
        }
        if let Some(c) = &self.replay.capture {
            c.inputs[safe_page_bounds(start, count, c.inputs.len())]
                .iter()
                .map(|e| {
                    let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                    // Full u64 pointer/button IDs cannot fit Rhai's signed integer.
                    v["event"]["id"] = json!(e.event.id.to_string());
                    v
                })
                .collect()
        } else {
            self.replay.touches[safe_page_bounds(start, count, self.replay.touches.len())]
                .iter()
                .map(|e| json!({"legacy":true,"phase":e.phase,"id":e.id.to_string(),"chart_time":e.t,"x":e.x,"y":e.y,"precision":"frame"}))
                .collect()
        }
    }
    pub fn judge_page(&self, start: i64, count: i64) -> Vec<Value> {
        if start < 0 || !(1..=256).contains(&count) {
            return vec![];
        }
        self.replay.judges[safe_page_bounds(start, count, self.replay.judges.len())]
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
            .collect()
    }
}

pub struct Job {
    pub path: PathBuf,
    pub preferred: Option<String>,
    pub candidates: Vec<String>,
    pub analyzer: Analyzer,
    pub destination: PathBuf,
    pub control: Control,
}

pub struct Output {
    pub png: PathBuf,
    pub json: PathBuf,
    pub image: image::RgbaImage,
    pub verdict: String,
    pub evidence: Vec<Value>,
    pub quality: Vec<String>,
    pub replay_path: PathBuf,
    pub local_chart: Option<String>,
    pub chart_verified: bool,
}

pub fn validate_report(value: &Value) -> Result<()> {
    anyhow::ensure!(value.is_object(), "回放报告必须为对象");
    anyhow::ensure!(matches!(value["status"].as_str(), Some("clear" | "review" | "limited" | "conflict" | "cheat")), "回放报告状态无效");
    anyhow::ensure!(value["summary"].as_str().is_some_and(|v| v.len() <= 2048), "报告摘要无效");
    let evidence = value["evidence"].as_array().context("报告 evidence 必须为数组")?;
    if value["status"] == "cheat" {
        let families = evidence
            .iter()
            .filter(|e| e["confirmed"] == true)
            .filter_map(|e| e["family"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            value["calibrated"] == true && value["confirmed"] == true && families.len() >= 2,
            "存在作弊结论需要经过校准并有不同证据方向的确认支持"
        );
    }
    anyhow::ensure!(evidence.len() <= 24, "报告证据超过 24 条");
    for e in evidence {
        anyhow::ensure!(
            e["title"].as_str().is_some_and(|v| v.len() <= 240)
                && e["detail"].as_str().is_some_and(|v| v.len() <= 2048)
                && e["family"].as_str().is_some_and(|v| v.len() <= 80)
                && e["start"].as_f64().is_some_and(|v| v.is_finite() && (-3600. ..=86400.).contains(&v))
                && e["end"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v >= e["start"].as_f64().unwrap_or(f64::INFINITY) && v <= 86400.),
            "报告证据字段无效"
        );
    }
    Ok(())
}

impl Job {
    pub fn execute(self) -> Result<Output> {
        self.control.set_stage(0)?;
        // Hash the exact bounded file bytes, not reserialized repaired content.
        anyhow::ensure!(std::fs::metadata(&self.path)?.len() <= 128 << 20, "回放超过 128MiB");
        let mut raw = Vec::new();
        use std::io::Read;
        let mut file = std::fs::File::open(&self.path)?;
        let size = file.metadata()?.len() as usize;
        let mut buffer = [0u8; 65536];
        loop {
            self.control.check()?;
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buffer[..n]);
            anyhow::ensure!(raw.len() <= 128 << 20, "回放超过 128MiB");
            self.control.progress(raw.len(), size)?;
        }
        anyhow::ensure!(raw.len() <= 128 << 20, "回放超过 128MiB");
        let hash = format!("{:x}", Sha256::digest(&raw));
        let replay = Arc::new(super::from_bytes(&raw)?);
        drop(raw);
        self.control.set_stage(1)?;
        let bound = features::find_chart(&replay, self.preferred.as_deref(), &self.candidates, &self.control)?;
        let local_chart = bound.local.clone();
        let verified = bound.verified;
        self.control.set_stage(2)?;
        let mut data = features::extract(replay, bound, &self.control)?;
        data.hash = hash.clone();
        data.payload["replay_hash"] = json!(hash);
        self.control.set_stage(3)?;
        let result = self.analyzer.analyze(Arc::new(data.clone_for_plugin()), &self.control)?;
        self.control.set_stage(4)?;
        let mut document = json!({"report_schema":1,"feature_schema":FEATURE_SCHEMA,"experimental":true,"host":{"version":crate::PRO_VERSION,"source":env!("GIT_HASH")},
            "plugin":{"id":self.analyzer.id,"name":self.analyzer.name,"version":self.analyzer.version(),"fingerprint":self.analyzer.fingerprint(),"settings":self.analyzer.settings()},
            "replay_sha256":hash,"input":data.payload,"quality":data.quality,"series":data.series,"report":result});
        let image = report::render(&data, &result, &self.analyzer, &self.control)?;
        self.control.set_stage(5)?;
        anyhow::ensure!(self.analyzer.current(), "插件已变更，结果未保存");
        let id = format!("{}-{}", &hash[..12], &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let png = self.destination.join(format!("PRCA-{id}.png"));
        let json_path = self.destination.join(format!("PRCA-{id}.json"));
        document["report_id"] = json!(id);
        std::fs::create_dir_all(&self.destination)?;
        let mut png_bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png_bytes, image::ImageFormat::Png)?;
        self.control.check()?;
        crate::transfer::write_atomic(&json_path, &serde_json::to_vec_pretty(&document)?)?;
        if let Err(e) = crate::transfer::write_atomic(&png, png_bytes.get_ref()) {
            let _ = std::fs::remove_file(&json_path);
            return Err(e);
        }
        self.control.progress(1, 1)?;
        Ok(Output {
            png,
            json: json_path,
            image,
            verdict: result["summary"].as_str().unwrap_or_default().into(),
            evidence: result["evidence"].as_array().unwrap().clone(),
            quality: data.quality,
            replay_path: self.path,
            local_chart,
            chart_verified: verified,
        })
    }
}

impl Dataset {
    fn clone_for_plugin(&self) -> Self {
        Self {
            replay: Arc::clone(&self.replay),
            payload: self.payload.clone(),
            series: Vec::new(),
            quality: Vec::new(),
            hash: self.hash.clone(),
        }
    }
}

pub fn safe_page_bounds(start: i64, count: i64, total: usize) -> std::ops::Range<usize> {
    if start < 0 || !(1..=256).contains(&count) {
        return 0..0;
    }
    let start = (start as usize).min(total);
    start..start.saturating_add(count as usize).min(total)
}

pub fn export(path: &Path) -> Result<()> {
    let name = path.file_name().context("报告文件名无效")?.to_string_lossy().into_owned();
    super::request_export_bytes(name, std::fs::read(path)?);
    Ok(())
}
