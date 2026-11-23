//! `.ppr` 回放容器解析。
//!
//! 文件头为 `PHIRAREC`、小端 `u32` 版本号，v5 / v6 的主体是 Zstandard 压缩的
//! UTF-8 JSON。这里只保留独立播放器需要的字段；原始输入记录（v6 `capture`）
//! 由录制端负责，播放以 `touches` / `judges` 为准。

use anyhow::{bail, Context, Result};
use prpr::config::{Config, JudgeAlgorithm, JudgeGrading, JudgeTiming, Mods, PhigrosRules};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

const MAGIC: &[u8; 8] = b"PHIRAREC";
const MAX_BYTES: u64 = 128 << 20;
const MAX_EVENTS: usize = 2_000_000;

/// 触摸相位，与 Phira Pro 的 `replay::PHASE_*` 一致。
pub const PHASE_DOWN: u8 = 0;
pub const PHASE_UP: u8 = 2;

/// 一条触摸帧。
///
/// `t` 是谱面时间（秒，0 = 谱面偏移处），`x` / `y` 是界面归一化坐标。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TouchEvent {
    pub phase: u8,
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub t: f32,
}

/// 一条判定事件。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct JudgeEvent {
    pub t: f64,
    pub line: u32,
    pub note: u32,
    /// 0=Perfect 1=Good 2=Bad 3=Miss 4=PerfectPlus 5=HoldPerfect
    /// 6=HoldGood 7=Great 8=Ok 9=Meh
    pub kind: u8,
    pub diff: f32,
    #[serde(default)]
    pub head_grade: Option<u8>,
}

/// 谱面引用。独立播放器用不到在线下载，只用于展示来源。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChartRef {
    Id(i32),
    Local(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    pub score: u32,
    pub accuracy: f64,
    pub max_combo: u32,
    pub num_of_notes: u32,
    pub counts: [u32; 4],
    /// 新版本成绩记录才含八档判定；旧文件或缺字段时为 `None`。
    pub grade_counts: Option<[u32; 8]>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub id: String,
    pub player: Option<String>,
    pub player_id: Option<i32>,
    pub recorded_at: Option<i64>,
    pub name: String,
    pub level: String,
    pub difficulty: f32,
    pub server: Option<String>,
    pub fingerprint: Option<String>,
    pub audio_fingerprint: Option<String>,
    pub result: Option<Record>,
}

/// 录制时的判定设置。字段名与 Phira Pro 的 `JudgeSettings` 保持一致，
/// 便于直接复用 `prpr` 的判定配置类型。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JudgeSettings {
    pub algorithm: JudgeAlgorithm,
    pub windows_ms: [f32; 4],
    pub timing: JudgeTiming,
    pub grading: JudgeGrading,
    pub late_ms: f32,
    pub drag_protect: bool,
    pub flick_protect: bool,
    pub hold_tail: bool,
    pub strict: bool,
    pub fullscreen: bool,
    pub no_combo_score: bool,
    pub phigros: PhigrosRules,
}

impl Default for JudgeSettings {
    fn default() -> Self {
        Self::capture(&Config::default())
    }
}

impl JudgeSettings {
    pub fn capture(c: &Config) -> Self {
        Self {
            algorithm: c.judge_algorithm,
            timing: c.judge_timing.clone(),
            grading: c.judge_grading,
            windows_ms: [
                c.lim_perfect_plus_ms,
                c.lim_perfect_ms,
                c.lim_good_ms,
                c.lim_bad_ms,
            ],
            late_ms: c.input_latency_compensation_ms,
            drag_protect: c.drag_protect,
            flick_protect: c.flick_protect,
            hold_tail: c.hold_tail_judge,
            strict: c.mods.contains(Mods::STRICT_JUDGE),
            fullscreen: c.mods.contains(Mods::FULLSCREEN_JUDGE),
            no_combo_score: c.mods.contains(Mods::NO_COMBO_SCORE),
            phigros: c.phigros_rules.clone(),
        }
    }

    /// 与 Phira Pro 回放播放器相同的应用顺序；只改判定相关字段，
    /// 不覆盖观看者的音量、资源包等本地偏好。
    pub fn apply(&self, c: &mut Config) {
        c.judge_grading = self.grading;
        c.judge_algorithm = self.algorithm;
        c.judge_timing = self.timing.clone();
        [
            c.lim_perfect_plus_ms,
            c.lim_perfect_ms,
            c.lim_good_ms,
            c.lim_bad_ms,
        ] = self.windows_ms;
        c.input_latency_compensation_ms = self.late_ms;
        c.drag_protect = self.drag_protect;
        c.flick_protect = self.flick_protect;
        c.hold_tail_judge = self.hold_tail;
        c.phigros_rules = self.phigros.clone();
        c.phigros_rules.sanitize();
        c.mods.set(Mods::STRICT_JUDGE, self.strict);
        c.mods.set(Mods::FULLSCREEN_JUDGE, self.fullscreen);
        c.mods.set(Mods::NO_COMBO_SCORE, self.no_combo_score);
        c.clamp_judge_windows();
        c.input_latency_compensation_ms = (c.input_latency_compensation() * 1000.) as f32;
    }
}

/// 录制时的玩法设置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Gameplay {
    pub hp_mode: bool,
    pub hp_disable_healing: bool,
    /// 旧录像保存的是原始误差，判定窗口更宽。
    pub input_time_corrected: bool,
    pub hp_amount: f32,
    pub hp_scale: f32,
    pub points_damage: bool,
    pub use_keyboard: bool,
    pub note_scale: f32,
    pub flow_speed: f32,
    pub fade_strength: f32,
}

impl Default for Gameplay {
    fn default() -> Self {
        Self {
            hp_mode: false,
            hp_disable_healing: false,
            // 旧录制器没有这个字段，按“未校正”处理，与 Phira Pro 一致。
            input_time_corrected: false,
            hp_amount: 1.,
            hp_scale: 1.,
            points_damage: false,
            use_keyboard: false,
            note_scale: 1.,
            flow_speed: 1.,
            fade_strength: 0.,
        }
    }
}

impl Gameplay {
    pub fn apply(&self, c: &mut Config) {
        c.hp_mode = self.hp_mode;
        c.hp_disable_healing = self.hp_disable_healing;
        c.hp_amount = self.hp_amount;
        c.hp_scale = self.hp_scale;
        c.legacy_hp_damage = !self.points_damage;
        c.use_keyboard = self.use_keyboard;
        c.note_scale = self.note_scale;
        c.flow_speed = self.flow_speed;
        c.fade_strength = self.fade_strength;
    }
}

/// 回放文件。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Replay {
    #[serde(skip)]
    pub source_version: u32,
    pub meta: Metadata,
    pub frames: Vec<f64>,
    pub aspect_ratio: Option<f32>,
    pub settings: Option<JudgeSettings>,
    pub gameplay: Option<Gameplay>,
    pub theoretical_score: bool,
    pub has_diffs: bool,
    pub has_speed: bool,
    pub chart: Option<ChartRef>,
    pub offset: f32,
    pub speed: f32,
    pub mods: u32,
    pub grading: JudgeGrading,
    pub touches: Vec<TouchEvent>,
    pub judges: Vec<JudgeEvent>,
    /// 录制时该在线谱面的压缩包 URL；仅用于来源展示。
    pub chart_file: Option<String>,
    pub chart_updated: Option<i64>,
}

impl Replay {
    pub fn ensure_playable(&self) -> Result<()> {
        anyhow::ensure!(
            !self.judges.is_empty(),
            "该回放缺少逐音符判定事件，无法还原打击和实时分数；原成绩仍在文件中，但旧版录制故障丢失的数据无法恢复"
        );
        Ok(())
    }

    pub fn chart_id(&self) -> Option<i32> {
        match &self.chart {
            Some(ChartRef::Id(id)) => Some(*id),
            _ => None,
        }
    }
}

/// 为触点分配界面编号（F1、F2……），同一次按住期间保持稳定。
pub fn contact_numbers(events: &[TouchEvent]) -> Vec<usize> {
    let mut active = std::collections::HashMap::new();
    let mut next = 0;
    events
        .iter()
        .map(|e| {
            if e.phase == PHASE_DOWN || !active.contains_key(&e.id) {
                next += 1;
                active.insert(e.id, next);
            }
            let number = active[&e.id];
            if e.phase == PHASE_UP {
                active.remove(&e.id);
            }
            number
        })
        .collect()
}

pub fn load(path: &Path) -> Result<Replay> {
    let mut file = File::open(path).with_context(|| format!("无法打开回放 {}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref().take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_BYTES, "回放文件超过 128MiB 上限");
    from_bytes(&bytes).with_context(|| format!("解析回放 {}", path.display()))
}

pub fn from_bytes(bytes: &[u8]) -> Result<Replay> {
    anyhow::ensure!(bytes.len() >= 12, "不是回放文件");
    anyhow::ensure!(&bytes[..8] == MAGIC, "不是回放文件（缺少 PHIRAREC 文件头）");
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let mut replay = match version {
        5 | 6 => {
            let json = read_zstd(&bytes[12..])?;
            serde_json::from_slice(&json).context("回放元数据无效")?
        }
        3 | 4 => parse_v3(bytes)?,
        _ => bail!("不支持的回放版本号：{version}"),
    };
    replay.source_version = version;
    if version >= 5 && replay.speed <= 0. {
        replay.speed = 1.;
    }
    canonicalize(&mut replay)?;
    Ok(replay)
}

fn read_zstd(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(bytes)?;
    let mut out = Vec::new();
    decoder.by_ref().take(MAX_BYTES + 1).read_to_end(&mut out)?;
    anyhow::ensure!(out.len() as u64 <= MAX_BYTES, "回放解压后超过 128MiB 上限");
    Ok(out)
}

/// 早期 Pro v3 / v4 是定长二进制容器，仅含判定与触点。
fn parse_v3(bytes: &[u8]) -> Result<Replay> {
    let mut r = Cursor::new(bytes);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    let version = read_u32(&mut r)?;
    let chart = match read_u8(&mut r)? {
        0 => Some(ChartRef::Id(read_i32(&mut r)?)),
        1 => {
            let len = read_u8(&mut r)? as usize;
            let mut data = vec![0; len];
            r.read_exact(&mut data)?;
            Some(ChartRef::Local(String::from_utf8_lossy(&data).into_owned()))
        }
        _ => None,
    };
    let offset = read_f32(&mut r)?;
    let speed = read_f32(&mut r)?;
    let mods = read_u32(&mut r)?;
    let grading = if version >= 4 {
        let perfect_plus = read_u8(&mut r)? != 0;
        let detailed = read_u8(&mut r)? != 0;
        JudgeGrading {
            perfect_plus,
            detailed,
            ..Default::default()
        }
    } else {
        Default::default()
    };
    let chart_updated = match read_i64(&mut r)? {
        i64::MIN => None,
        v => Some(v),
    };
    let file_len = read_u32(&mut r)? as usize;
    anyhow::ensure!(file_len <= 16 << 10, "回放谱面地址过长");
    let chart_file = if file_len == 0 {
        None
    } else {
        let mut data = vec![0; file_len];
        r.read_exact(&mut data)?;
        Some(String::from_utf8_lossy(&data).into_owned())
    };
    let touch_count = read_u32(&mut r)? as usize;
    let judge_count = read_u32(&mut r)? as usize;
    anyhow::ensure!(
        touch_count <= MAX_EVENTS && judge_count <= MAX_EVENTS,
        "回放事件过多"
    );
    let mut touches = Vec::with_capacity(touch_count);
    for _ in 0..touch_count {
        touches.push(TouchEvent {
            phase: read_u8(&mut r)?,
            id: read_u8(&mut r)? as u64,
            x: read_f32(&mut r)?,
            y: read_f32(&mut r)?,
            t: read_f32(&mut r)?,
        });
    }
    let mut judges = Vec::with_capacity(judge_count);
    for _ in 0..judge_count {
        judges.push(JudgeEvent {
            t: read_f64(&mut r)?,
            line: read_u32(&mut r)?,
            note: read_u32(&mut r)?,
            kind: read_u8(&mut r)?,
            diff: read_f32(&mut r)?,
            head_grade: None,
        });
    }
    Ok(Replay {
        source_version: version,
        chart,
        offset,
        speed,
        mods,
        grading,
        touches,
        judges,
        chart_file,
        chart_updated,
        has_speed: true,
        ..Default::default()
    })
}

/// 旧录制器可能按线号顺序写入判定；稳定排序保留同一时刻的原始顺序。
fn canonicalize(replay: &mut Replay) -> Result<()> {
    validate_values(replay)?;
    replay.frames.sort_by(f64::total_cmp);
    replay.judges.sort_by(|a, b| a.t.total_cmp(&b.t));
    replay.touches.sort_by(|a, b| a.t.total_cmp(&b.t));
    Ok(())
}

fn validate_values(replay: &Replay) -> Result<()> {
    anyhow::ensure!(
        replay.touches.len() <= MAX_EVENTS
            && replay.judges.len() <= MAX_EVENTS
            && replay.frames.len() <= MAX_EVENTS,
        "回放事件过多"
    );
    anyhow::ensure!(
        replay.offset.is_finite()
            && replay.speed.is_finite()
            && replay.speed > 0.
            && replay.speed <= 16.,
        "回放速度或偏移无效"
    );
    anyhow::ensure!(
        replay
            .aspect_ratio
            .is_none_or(|v| v.is_finite() && v > 0. && v <= 10.),
        "回放比例无效"
    );
    if let Some(g) = &replay.gameplay {
        anyhow::ensure!(
            [
                g.hp_amount,
                g.hp_scale,
                g.note_scale,
                g.flow_speed,
                g.fade_strength
            ]
            .iter()
            .all(|v| v.is_finite() && (0. ..=100.).contains(v)),
            "回放玩法参数无效"
        );
    }
    let time = |t: f64| t.is_finite() && (-3600. ..=86400.).contains(&t);
    anyhow::ensure!(
        replay
            .touches
            .iter()
            .all(|e| time(e.t as f64) && e.x.is_finite() && e.y.is_finite() && e.phase <= 2)
            && replay.judges.iter().all(|e| time(e.t)
                && e.diff.is_finite()
                && e.kind <= 9
                && e.head_grade
                    .is_none_or(|g| matches!(g, 0 | 1 | 2 | 3 | 4 | 7 | 8 | 9)))
            && replay.frames.iter().all(|t| time(*t)),
        "回放含无效事件"
    );
    Ok(())
}

fn read_u8(r: &mut Cursor<&[u8]>) -> Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
fn read_u32(r: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn read_i32(r: &mut Cursor<&[u8]>) -> Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}
fn read_i64(r: &mut Cursor<&[u8]>) -> Result<i64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(i64::from_le_bytes(b))
}
fn read_f32(r: &mut Cursor<&[u8]>) -> Result<f32> {
    Ok(f32::from_bits(read_u32(r)?))
}
fn read_f64(r: &mut Cursor<&[u8]>) -> Result<f64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(f64::from_le_bytes(b))
}
