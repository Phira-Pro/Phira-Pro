//! Separate, versioned replay tapes. v5 adds metadata, full judgement settings, real offsets and frame timestamps.
pub mod library;
pub mod transport;
pub mod capture;
pub mod analysis;

use anyhow::{Context, Result};
use prpr::{judge::Judge, scene::UpdateFn};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

use crate::dir;
use serde::{Deserialize, Serialize};
const MAX_BYTES: u64 = 128 << 20;
const MAX_EVENTS: usize = 2_000_000;

/// 触摸相位。
pub const PHASE_DOWN: u8 = 0;
pub const PHASE_MOVE: u8 = 1;
pub const PHASE_UP: u8 = 2;

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

/// 一条触摸帧。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TouchEvent {
    pub phase: u8,
    pub id: u64,
    pub x: f32,
    pub y: f32,
    /// 谱面时间（秒，0 = 谱面偏移处，与 `Resource.time` 同基准）。
    pub t: f32,
}

/// 一条判定事件。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct JudgeEvent {
    pub t: f64,
    pub line: u32,
    pub note: u32,
    /// 0=Perfect 1=Good 2=Bad 3=Miss 4=PerfectPlus 5=HoldPerfect 6=HoldGood 7=Great 8=Ok 9=Meh
    pub kind: u8,
    pub diff: f32,
    #[serde(default)]
    pub head_grade: Option<u8>,
}

/// 谱面引用。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChartRef {
    Id(i32),
    Local(String),
}

/// 回放文件。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Replay {
    pub capture: Option<capture::Capture>,
    /// Read diagnostics are never supplied by file metadata or hidden by sorting.
    #[serde(skip)]
    pub source_version: u32,
    #[serde(skip)]
    pub repaired_order: bool,
    /// Preserve the fact that an importer repaired order across re-export.
    pub normalized_order: bool,
    pub meta: Metadata,
    pub frames: Vec<f64>,
    pub aspect_ratio: Option<f32>,
    pub settings: Option<crate::judgement_presets::JudgeSettings>,
    pub gameplay: Option<Gameplay>,
    pub theoretical_score: bool,
    pub has_diffs: bool,
    pub has_speed: bool,
    pub chart: Option<ChartRef>,
    pub offset: f32,
    pub speed: f32,
    pub mods: u32,
    pub grading: prpr::config::JudgeGrading,
    pub touches: Vec<TouchEvent>,
    pub judges: Vec<JudgeEvent>,
    /// 录制时该在线谱面的压缩包 URL（内容寻址，每个版本不变），用于回放时下载对应版本。
    pub chart_file: Option<String>,
    /// 录制时谱面的 chart_updated（Unix 毫秒），用于判断本地谱面是否已更新。
    pub chart_updated: Option<i64>,
}

impl Replay {
    pub fn ensure_playable(&self) -> Result<()> {
        let score = self
            .meta
            .result
            .as_ref()
            .map(|r| format!("原成绩 {} 分、准度 {:.4}% 仍已保存。", r.score, r.accuracy * 100.))
            .unwrap_or_default();
        anyhow::ensure!(
            !self.judges.is_empty(),
            "该回放缺少逐音符判定事件，无法还原打击和实时分数。{score}旧版录制故障丢失的数据无法恢复，请更新后重新录制；导出或导入不会补回这些数据。"
        );
        Ok(())
    }
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
    pub result: Option<crate::history::Record>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gameplay {
    pub hp_mode: bool,
    pub hp_amount: f32,
    pub hp_scale: f32,
    pub use_keyboard: bool,
    pub note_scale: f32,
    pub flow_speed: f32,
    pub fade_strength: f32,
}
impl Gameplay {
    fn capture(c: &prpr::config::Config) -> Self {
        Self {
            hp_mode: c.hp_mode,
            hp_amount: c.hp_amount,
            hp_scale: c.hp_scale,
            use_keyboard: c.use_keyboard,
            note_scale: c.note_scale,
            flow_speed: c.flow_speed,
            fade_strength: c.fade_strength,
        }
    }
    pub fn apply(&self, c: &mut prpr::config::Config) {
        c.hp_mode = self.hp_mode;
        c.hp_amount = self.hp_amount;
        c.hp_scale = self.hp_scale;
        c.use_keyboard = self.use_keyboard;
        c.note_scale = self.note_scale;
        c.flow_speed = self.flow_speed;
        c.fade_strength = self.fade_strength;
    }
}

fn read_bounded(reader: impl Read) -> Result<Vec<u8>> {
    read_with_limit(reader, MAX_BYTES)
}
fn read_with_limit(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.take(limit + 1).read_to_end(&mut out)?;
    anyhow::ensure!(out.len() as u64 <= limit, "回放文件超过大小上限");
    Ok(out)
}

pub fn bytes(replay: &Replay) -> Result<Vec<u8>> {
    validate(replay)?;
    let mut out = b"PHIRAREC".to_vec();
    out.extend_from_slice(&(if replay.capture.is_some() { 6u32 } else { 5u32 }).to_le_bytes());
    let json = serde_json::to_vec(replay)?;
    anyhow::ensure!(json.len() as u64 <= MAX_BYTES, "回放数据过大");
    out.extend(zstd::stream::encode_all(json.as_slice(), 3)?);
    Ok(out)
}
pub fn save(replay: &Replay, path: &PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::transfer::write_atomic(path, &bytes(replay)?)
}
fn validate(replay: &Replay) -> Result<()> {
    validate_values(replay)?;
    anyhow::ensure!(
        replay.frames.windows(2).all(|w| w[0] <= w[1])
            && replay.judges.windows(2).all(|w| w[0].t <= w[1].t)
            && replay.touches.windows(2).all(|w| w[0].t <= w[1].t),
        "回放时间顺序无效"
    );
    Ok(())
}

fn canonicalize(replay: &mut Replay) -> Result<()> {
    // Old recorders appended judgements from independent lines and clock
    // corrections in delivery order. Validate values before stable sorting;
    // equal-time Hold head/tail and touch phases retain their original order.
    validate_values(replay)?;
    replay.repaired_order |= replay.normalized_order || replay.frames.windows(2).any(|w| w[0] > w[1])
        || replay.judges.windows(2).any(|w| w[0].t > w[1].t)
        || replay.touches.windows(2).any(|w| w[0].t > w[1].t);
    replay.normalized_order = replay.repaired_order;
    replay.frames.sort_by(f64::total_cmp);
    replay.judges.sort_by(|a, b| a.t.total_cmp(&b.t));
    replay.touches.sort_by(|a, b| a.t.total_cmp(&b.t));
    validate(replay)
}

fn validate_values(replay: &Replay) -> Result<()> {
    if let Some(capture) = &replay.capture { capture.validate(MAX_EVENTS)?; }
    anyhow::ensure!(replay.touches.len() <= MAX_EVENTS && replay.judges.len() <= MAX_EVENTS && replay.frames.len() <= MAX_EVENTS, "回放事件过多");
    anyhow::ensure!(replay.offset.is_finite() && replay.speed.is_finite() && replay.speed > 0. && replay.speed <= 16., "回放速度或偏移无效");
    anyhow::ensure!(replay.aspect_ratio.is_none_or(|v| v.is_finite() && v > 0. && v <= 10.), "回放比例无效");
    if let Some(g) = &replay.gameplay {
        anyhow::ensure!(
            [g.hp_amount, g.hp_scale, g.note_scale, g.flow_speed, g.fade_strength]
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
            && replay
                .judges
                .iter()
                .all(|e| time(e.t) && e.diff.is_finite() && e.kind <= 9 && e.head_grade.is_none_or(|g| matches!(g, 0 | 1 | 2 | 3 | 4 | 7 | 8 | 9)))
            && replay.frames.iter().all(|t| time(*t)),
        "回放含无效事件"
    );
    Ok(())
}

#[cfg(test)]
impl TouchEvent {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        w.write_all(&[self.phase])?;
        w.write_all(&[self.id as u8])?;
        w.write_all(&self.x.to_le_bytes())?;
        w.write_all(&self.y.to_le_bytes())?;
        w.write_all(&self.t.to_le_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
impl JudgeEvent {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        w.write_all(&self.t.to_le_bytes())?;
        w.write_all(&self.line.to_le_bytes())?;
        w.write_all(&self.note.to_le_bytes())?;
        w.write_all(&[self.kind])?;
        w.write_all(&self.diff.to_le_bytes())?;
        Ok(())
    }
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// 一条成绩记录对应的回放文件路径（不存在时也返回该路径，用于探测）。
pub fn path_for(key: &str, time_ms: i64) -> PathBuf {
    use sha2::{Digest, Sha256};
    let root = dir::root().unwrap_or_default();
    let legacy = PathBuf::from(format!("{root}/replays/{}_{}.phirar", sanitize_key(key), time_ms));
    if legacy.is_file() {
        return legacy;
    }
    let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    PathBuf::from(format!("{root}/replays/{}_{time_ms}.ppr", &hash[..24]))
}

#[cfg(test)]
fn save_legacy(replay: &Replay, path: &PathBuf) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut buf = Vec::new();
    buf.write_all(b"PHIRAREC")?;
    buf.write_all(&4u32.to_le_bytes())?;
    match &replay.chart {
        None => buf.write_all(&[0])?,
        Some(ChartRef::Id(id)) => {
            buf.write_all(&[0])?;
            buf.write_all(&id.to_le_bytes())?;
        }
        Some(ChartRef::Local(p)) => {
            buf.write_all(&[1])?;
            buf.write_all(&[(p.len() as u8)])?;
            buf.write_all(p.as_bytes())?;
        }
    }
    buf.write_all(&replay.offset.to_le_bytes())?;
    buf.write_all(&replay.speed.to_le_bytes())?;
    buf.write_all(&replay.mods.to_le_bytes())?;
    buf.write_all(&[u8::from(replay.grading.perfect_plus), u8::from(replay.grading.detailed)])?;
    buf.write_all(&replay.chart_updated.unwrap_or(i64::MIN).to_le_bytes())?;
    let file = replay.chart_file.as_deref().unwrap_or("");
    let fb = file.as_bytes();
    buf.write_all(&(fb.len() as u32).to_le_bytes())?;
    buf.write_all(fb)?;
    buf.write_all(&(replay.touches.len() as u32).to_le_bytes())?;
    buf.write_all(&(replay.judges.len() as u32).to_le_bytes())?;
    for e in &replay.touches {
        e.encode(&mut buf)?;
    }
    for e in &replay.judges {
        e.encode(&mut buf)?;
    }
    std::fs::write(path, &buf).with_context(|| format!("failed to write replay to {}", path.display()))?;
    Ok(())
}

pub fn load(path: &std::path::Path) -> Result<Replay> {
    let bytes = read_bounded(std::fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?)?;
    from_bytes(&bytes)
}

/// Parse the exact byte snapshot used for a forensic content hash.
pub fn from_bytes(bytes: &[u8]) -> Result<Replay> {
    anyhow::ensure!(bytes.len() as u64 <= MAX_BYTES, "回放文件超过大小上限");
    if bytes.len() < 12 {
        anyhow::bail!("不是回放文件");
    }
    if &bytes[..8] != b"PHIRAREC" {
        anyhow::bail!("不是回放文件");
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let mut replay = match version {
        5 | 6 => serde_json::from_slice(&read_bounded(zstd::stream::read::Decoder::new(&bytes[12..])?)?).context("回放元数据无效"),
        3 | 4 => parse_v3(bytes),
        0 | 1 => parse_phirarec(bytes, version),
        _ => anyhow::bail!("不支持的版本号：{version}"),
    }?;
    replay.source_version = version;
    if version < 6 { replay.capture = None; }
    if version == 6 { anyhow::ensure!(replay.capture.is_some(), "v6 回放缺少原始输入记录"); }
    canonicalize(&mut replay)?;
    if let Some(capture) = replay.capture.as_ref().filter(|c| c.dropped == 0) {
        replay.touches = capture.playback_touches();
        validate(&replay)?;
    }
    Ok(replay)
}

/// 解析 Phira Pro 自己的回放格式（v3 / v4）。
fn parse_v3(bytes: &[u8]) -> Result<Replay> {
    let mut r = FileCursor::new(bytes);
    r.take(8)?; // magic
    let version = r.u32()?; // version
    let mut chart = None;
    match r.u8()? {
        0 => chart = Some(ChartRef::Id(r.i32()?)),
        1 => {
            let len = r.u8()? as usize;
            let p = String::from_utf8(r.take(len)?.to_vec()).unwrap_or_default();
            chart = Some(ChartRef::Local(p));
        }
        _ => {}
    }
    let offset = r.f32()?;
    let speed = r.f32()?;
    let mods = r.u32()?;
    let grading = if version >= 4 {
        let p = r.u8()?;
        let d = r.u8()?;
        anyhow::ensure!(p <= 1 && d <= 1, "invalid replay grading flags");
        prpr::config::JudgeGrading {
            perfect_plus: p != 0,
            detailed: d != 0,
            ..Default::default()
        }
    } else {
        Default::default()
    };
    let cu = r.i64()?;
    let chart_updated = if cu == i64::MIN { None } else { Some(cu) };
    let flen = r.u32()? as usize;
    let chart_file = if flen == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(r.take(flen)?).into_owned())
    };
    let tn = r.u32()?;
    let jn = r.u32()?;
    anyhow::ensure!(tn as usize <= MAX_EVENTS && jn as usize <= MAX_EVENTS, "回放事件过多");
    let mut touches = Vec::new();
    for _ in 0..tn {
        touches.push(TouchEvent {
            phase: r.u8()?,
            id: r.i8()? as u8 as u64,
            x: r.f32()?,
            y: r.f32()?,
            t: r.f32()?,
        });
    }
    let mut judges = Vec::new();
    for _ in 0..jn {
        judges.push(JudgeEvent {
            t: r.f64()?,
            line: r.u32()?,
            note: r.u32()?,
            kind: r.u8()?,
            diff: r.f32()?,
            head_grade: None,
        });
    }
    Ok(Replay {
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

/// 解析官方 Java 回放器 / 联机监视器生成的 `.phirarec`（JPhiraRec v0 / v1）。
///
/// 字节布局来自 `PhiraRecordPlayer.zip` 里的 `PhiraRecord.java` 与官方
/// `phira-mp` 的 Rust 协议编码（ULEB128 长度前缀、小端定长、f16 触点坐标）：
/// - 文件头：魔数 "PHIRAREC" + 版本 i32 + （v1）压缩类型 u8
/// - 载荷：id、time(i64, v1)、chart、chartName、user、userName、触摸帧表、判定事件表
/// - 触摸帧：f32 时间 + ULEB 数量 + [i8 触点 id + f16 x + f16 y]*
/// - 判定事件：f32 时间 + u32 线 + u32 音符 + u8 判定(0..=5)
fn parse_phirarec(bytes: &[u8], version: u32) -> Result<Replay> {
    let mut c = FileCursor::new(bytes);
    c.take(8)?; // magic
    c.u32()?; // version
    let payload: Vec<u8> = if version == 1 {
        let comp = c.u8()?;
        let rest = c.rest();
        match comp {
            0 => rest.to_vec(),
            1 => read_bounded(zstd::stream::read::Decoder::new(rest)?).context("ZSTD 解压失败")?,
            2 => read_bounded(flate2::read::ZlibDecoder::new(rest)).context("DEFLATE 解压失败")?,
            other => anyhow::bail!("未知压缩类型：{other}"),
        }
    } else {
        c.rest().to_vec()
    };
    let mut r = FileCursor::new(payload.as_slice());
    let record_id = r.i32()?;
    let recorded_at = if version == 1 { Some(r.i64()?) } else { None };
    let chart = r.i32()?;
    let chart_name = r.str_uleb()?;
    let user = r.i32()?;
    let user_name = r.str_uleb()?;

    let touch_count = r.uleb()? as usize;
    anyhow::ensure!(touch_count <= MAX_EVENTS, "回放帧数过多");
    let mut touches = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..touch_count {
        let t = r.f32()?;
        frames.push(t as f64);
        let pts = r.uleb()? as usize;
        anyhow::ensure!(pts <= 256 && touches.len() + pts <= MAX_EVENTS, "触点数据过多");
        for _ in 0..pts {
            let id = r.i8()?;
            let x = r.f16()?;
            let y = r.f16()?;
            // MP 协议里正 id=按下/移动，负 id=抬起（`!id`）；y 按 16:9 还原回 UI 坐标。
            if id >= 0 {
                touches.push(TouchEvent {
                    phase: PHASE_MOVE,
                    id: id as u64,
                    x,
                    y: y / 1.7777778,
                    t,
                });
            } else {
                touches.push(TouchEvent {
                    phase: PHASE_UP,
                    id: (!id) as u64,
                    x,
                    y: y / 1.7777778,
                    t,
                });
            }
        }
    }

    let judge_count = r.uleb()? as usize;
    anyhow::ensure!(judge_count <= MAX_EVENTS, "回放判定过多");
    let mut judges = Vec::new();
    for _ in 0..judge_count {
        let t = r.f32()?;
        let line = r.u32()?;
        let note = r.u32()?;
        let j = r.u8()?;
        let kind = match j {
            0 => 0, // Perfect
            1 => 1, // Good
            2 => 2, // Bad
            3 => 3, // Miss
            4 => 5, // HoldPerfect
            5 => 6, // HoldGood
            _ => anyhow::bail!("未知旧回放判定：{j}"),
        };
        judges.push(JudgeEvent {
            t: t as f64,
            line,
            note,
            kind,
            diff: 0.,
            head_grade: None,
        });
    }

    Ok(Replay {
        meta: Metadata {
            id: format!("jphirarec-{user}-{record_id}"),
            player: Some(user_name),
            player_id: Some(user),
            recorded_at,
            name: chart_name,
            ..Default::default()
        },
        frames,
        chart: Some(ChartRef::Id(chart)),
        offset: 0.,
        speed: 1.,
        mods: 0,
        grading: Default::default(),
        touches,
        judges,
        chart_file: None,
        chart_updated: None,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_unsorted_tapes_are_stably_repaired_and_contact_lifetimes_are_numbered() {
        let mut replay = Replay {
            speed: 1.,
            frames: vec![2., 1., 1., 3.],
            judges: vec![
                JudgeEvent {
                    t: 2.,
                    kind: 0,
                    ..Default::default()
                },
                JudgeEvent {
                    t: 1.,
                    kind: 5,
                    ..Default::default()
                },
                JudgeEvent {
                    t: 1.,
                    kind: 0,
                    ..Default::default()
                },
            ],
            touches: vec![
                TouchEvent {
                    t: 2.,
                    id: 42,
                    phase: PHASE_UP,
                    ..Default::default()
                },
                TouchEvent {
                    t: 1.,
                    id: 42,
                    phase: PHASE_DOWN,
                    ..Default::default()
                },
                TouchEvent {
                    t: 3.,
                    id: 42,
                    phase: PHASE_DOWN,
                    ..Default::default()
                },
                TouchEvent {
                    t: 3.,
                    id: 42,
                    phase: PHASE_MOVE,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unordered.ppr");
        let payload = serde_json::to_vec(&replay).unwrap();
        let mut bytes = b"PHIRAREC".to_vec();
        bytes.extend(5u32.to_le_bytes());
        bytes.extend(zstd::stream::encode_all(payload.as_slice(), 1).unwrap());
        std::fs::write(&path, &bytes).unwrap();
        let fixed = load(&path).unwrap();
        assert_eq!(fixed.frames, [1., 1., 2., 3.]);
        assert_eq!(fixed.judges.iter().map(|j| j.kind).collect::<Vec<_>>(), [5, 0, 0]);
        assert_eq!(contact_numbers(&fixed.touches), [1, 1, 2, 2]);
        assert_eq!(std::fs::read(path).unwrap(), bytes, "reading must not rewrite the original file");
        replay.frames.push(f64::NAN);
        assert!(canonicalize(&mut replay).is_err(), "sorting must not hide invalid timestamps");
    }
    #[test]
    fn suspended_recording_closes_play_fingers_without_recording_menu_touches() {
        let mut fingers = std::collections::HashSet::new();
        assert!(accept_touch(&mut fingers, 71, PHASE_DOWN, true));
        assert!(!accept_touch(&mut fingers, 99, PHASE_DOWN, false));
        assert!(!accept_touch(&mut fingers, 99, PHASE_UP, false));
        assert!(accept_touch(&mut fingers, 71, PHASE_UP, false));
        assert!(fingers.is_empty());
        assert!(!accept_touch(&mut fingers, 71, PHASE_MOVE, false));
        assert!(accept_touch(&mut fingers, 71, PHASE_DOWN, true));
    }
    #[test]
    fn retry_discards_all_unsettled_data_and_settlement_is_consumed_once() {
        let mut recording = Recording::new(Pending {
            chart: Some(ChartRef::Id(42)),
            capture: capture::Capture::new(),
            speed: 1.,
            ..Default::default()
        });
        recording.begin_attempt(1);
        {
            let mut buffer = recording.buffer.borrow_mut();
            let p = buffer.as_mut().unwrap();
            p.frames.push(25.);
            p.touches.push(TouchEvent { t: 25., id: 7, phase: PHASE_DOWN, ..Default::default() });
            p.judges.push(JudgeEvent { t: 25., ..Default::default() });
            p.fingers.insert(7);
            p.capture.dropped = 3;
            p.capture_frame = Some(100);
        }
        recording.begin_attempt(1);
        assert_eq!(recording.buffer.borrow().as_ref().unwrap().frames, [25.], "pauses and clock corrections keep this attempt");
        recording.begin_attempt(2);
        {
            let buffer = recording.buffer.borrow();
            let p = buffer.as_ref().unwrap();
            assert!(p.frames.is_empty() && p.touches.is_empty() && p.judges.is_empty());
            assert!(p.fingers.is_empty() && p.capture.inputs.is_empty() && p.capture.anchors.is_empty());
            assert_eq!(p.capture.dropped, 0);
            assert_eq!(p.capture_frame, None);
            assert!(matches!(p.chart, Some(ChartRef::Id(42))));
        }
        assert!(take_recording().is_some());
        recording.begin_attempt(2);
        assert!(take_recording().is_none(), "ending frames must not recreate a settled replay");
        recording.begin_attempt(3);
        assert!(take_recording().is_some(), "result-page retry starts a fresh attempt");
    }
    #[test]
    fn exiting_drops_unsettled_recording_and_old_scenes_cannot_discard_new_ones() {
        let mut old = Recording::new(Pending::default());
        old.begin_attempt(1);
        drop(old);
        assert!(take_recording().is_none(), "unfinished scene cannot leave a saveable tape");
        let old = Recording::new(Pending::default());
        let mut current = Recording::new(Pending::default());
        current.begin_attempt(1);
        drop(old);
        assert!(take_recording().is_some());
        current.begin_attempt(2);
        discard_recording();
        current.begin_attempt(2);
        assert!(take_recording().is_none());
    }
    #[test]
    fn empty_judgement_tape_keeps_original_result_but_cannot_play() {
        let replay = Replay {
            speed: 1.,
            meta: Metadata {
                result: Some(crate::history::Record {
                    score: 895359,
                    accuracy: 0.9621569514274596,
                    ..Default::default()
                }),
                ..Default::default()
            },
            touches: vec![TouchEvent {
                t: 178.78204,
                ..Default::default()
            }],
            frames: vec![178.78204081632654; 113],
            ..Default::default()
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("empty.phirar");
        save(&replay, &path).unwrap();
        let loaded = load(&path).unwrap();
        let message = loaded.ensure_playable().unwrap_err().to_string();
        assert!(message.contains("895359") && message.contains("判定事件"));
        assert_eq!(loaded.meta.result.unwrap().score, 895359);
    }
    #[test]
    fn v5_preserves_real_offsets_wide_ids_unicode_and_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new.phirar");
        let mut config = prpr::config::Config::default();
        config.judge_algorithm = prpr::config::JudgeAlgorithm::Phigros;
        config.hold_tail_judge = true;
        config.drag_protect = true;
        let settings = crate::judgement_presets::JudgeSettings::capture(&config);
        let replay = Replay {
            chart: Some(ChartRef::Local("本地谱面/".repeat(80))),
            speed: 1.25,
            offset: -0.023,
            meta: Metadata {
                id: "distinct-player-record".into(),
                player: Some("回放作者".into()),
                recorded_at: Some(123456),
                ..Default::default()
            },
            frames: vec![0., 0.008, 0.024],
            settings: Some(settings.clone()),
            has_diffs: true,
            touches: vec![TouchEvent {
                id: u64::MAX,
                phase: 0,
                ..Default::default()
            }],
            judges: vec![JudgeEvent {
                kind: 5,
                diff: -0.021,
                head_grade: Some(7),
                ..Default::default()
            }],
            ..Default::default()
        };
        save(&replay, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.chart, replay.chart);
        assert_eq!(loaded.frames, replay.frames);
        assert_eq!(loaded.touches[0].id, u64::MAX);
        assert_eq!(loaded.judges[0].diff, -0.021);
        assert_eq!(loaded.judges[0].head_grade, Some(7));
        assert!(loaded.has_diffs);
        assert_eq!(loaded.settings, Some(settings));
        assert_eq!(loaded.meta.player, replay.meta.player);
        let mut bad = replay.clone();
        bad.frames = vec![1., 0.];
        assert!(save(&bad, &path).is_err());
        bad = replay;
        bad.judges[0].diff = f32::NAN;
        assert!(save(&bad, &path).is_err());
        assert!(load(&path).is_ok(), "failed save must not truncate the previous tape");
    }
    #[test]
    fn jphirarec_metadata_and_compressed_variants_are_not_discarded() {
        let mut payload = Vec::new();
        payload.extend(71i32.to_le_bytes());
        payload.extend(123456789i64.to_le_bytes());
        payload.extend(338i32.to_le_bytes());
        payload.push(4);
        payload.extend(b"Milk");
        payload.extend(999i32.to_le_bytes());
        payload.push(3);
        payload.extend(b"Ken");
        payload.push(1);
        payload.extend(1f32.to_le_bytes());
        payload.push(2);
        for (id, x) in [(4u8, 0x3c00u16), (!4u8, 0u16)] {
            payload.push(id);
            payload.extend(x.to_le_bytes());
            payload.extend(0u16.to_le_bytes());
        }
        payload.push(1);
        payload.extend(1f32.to_le_bytes());
        payload.extend(0u32.to_le_bytes());
        payload.extend(0u32.to_le_bytes());
        payload.push(0);
        for compression in 0..=2 {
            let mut bytes = b"PHIRAREC".to_vec();
            bytes.extend(1u32.to_le_bytes());
            bytes.push(compression);
            let encoded = match compression {
                0 => payload.clone(),
                1 => zstd::stream::encode_all(payload.as_slice(), 1).unwrap(),
                _ => {
                    let mut w = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
                    w.write_all(&payload).unwrap();
                    w.finish().unwrap()
                }
            };
            bytes.extend(encoded);
            let replay = parse_phirarec(&bytes, 1).unwrap();
            validate(&replay).unwrap();
            assert_eq!(replay.meta.player.as_deref(), Some("Ken"));
            assert_eq!(replay.meta.player_id, Some(999));
            assert_eq!(replay.meta.recorded_at, Some(123456789));
            assert_eq!(replay.frames, [1.]);
            assert_eq!(replay.touches[0].id, 4);
            assert_eq!(replay.touches[1].phase, PHASE_UP);
            assert!(!replay.has_diffs, "legacy zero placeholders are not measured offsets");
            let mut truncated = bytes.clone();
            truncated.truncate(15);
            assert!(parse_phirarec(&truncated, 1).is_err());
        }
        assert_eq!(half16_to_f32(1), 2f32.powi(-24));
        assert_eq!(half16_to_f32(0x8001), -2f32.powi(-24));
    }
    #[test]
    fn malformed_data_is_rejected_before_allocation_or_overflow() {
        assert!(read_with_limit(&b"12345"[..], 4).is_err());
        assert!(FileCursor::new(&[]).take(usize::MAX).is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad.phirar");
        std::fs::write(&path, b"PHIRAREC\x05\0\0\0broken-zstd").unwrap();
        assert!(load(&path).is_err());
        let mut replay = Replay {
            chart: Some(ChartRef::Id(1)),
            speed: 1.,
            ..Default::default()
        };
        save_legacy(&replay, &path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let count = bytes.len() - 8;
        bytes[count..count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_v3(&bytes).is_err());
        replay.touches.push(TouchEvent {
            x: f32::INFINITY,
            ..Default::default()
        });
        assert!(validate(&replay).is_err());
    }
    #[test]
    fn eight_grade_replay_roundtrip_and_v3_compatibility() {
        let file = std::env::temp_dir().join(format!("phira-grades-{}.phirar", uuid::Uuid::new_v4()));
        let mut replay = super::Replay {
            chart: Some(super::ChartRef::Id(338)),
            speed: 1.,
            ..Default::default()
        };
        replay.grading.perfect_plus = false;
        replay.grading.detailed = true;
        for id in [4, 0, 5, 1, 6, 7, 2, 3] {
            let grade = match id {
                4 => prpr::judge::Judgement::PerfectPlus,
                0 => prpr::judge::Judgement::Perfect,
                5 => prpr::judge::Judgement::Great,
                1 => prpr::judge::Judgement::Good,
                6 => prpr::judge::Judgement::Ok,
                7 => prpr::judge::Judgement::Meh,
                2 => prpr::judge::Judgement::Bad,
                _ => prpr::judge::Judgement::Miss,
            };
            replay.judges.push(super::JudgeEvent {
                kind: super::judge_kind(&Ok(grade)),
                ..Default::default()
            });
        }
        super::save(&replay, &file).unwrap();
        let loaded = super::load(&file).unwrap();
        assert!(!loaded.grading.perfect_plus && loaded.grading.detailed);
        assert_eq!(loaded.judges.iter().map(|e| e.kind).collect::<Vec<_>>(), [4, 0, 7, 1, 8, 9, 2, 3]);
        super::save_legacy(&replay, &file).unwrap();
        let mut bytes = std::fs::read(&file).unwrap();
        bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
        bytes.drain(29..31); // v4 flags after chart ID, offset, speed and mods
        let legacy = super::parse_v3(&bytes).unwrap();
        assert!(legacy.grading.perfect_plus && !legacy.grading.detailed);
        assert_eq!(legacy.judges.len(), 8);
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn parse_sample_phirarec() {
        // 用随回放器附带的真实样本验证 JPhiraRec v0 解析。
        let p = std::path::Path::new(r"e:\Phira Pro\Phira游戏录制回放器\records\133801240.phirarec");
        if !p.exists() {
            return;
        }
        match super::load(p) {
            Ok(r) => {
                eprintln!("OK chart={:?} touches={} judges={}", r.chart, r.touches.len(), r.judges.len());
                assert_eq!(r.chart, Some(super::ChartRef::Id(338)));
                assert!(r.judges.len() > 0);
                assert!(r.touches.len() > 0);
            }
            Err(e) => panic!("parse failed: {e:#}"),
        }
    }
}

/// 顺序读字节的小游标（小端）。
struct FileCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> FileCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.data.len().saturating_sub(self.pos) {
            anyhow::bail!("回放文件数据不足");
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f16(&mut self) -> Result<f32> {
        let bits = u16::from_le_bytes(self.take(2)?.try_into().unwrap());
        Ok(half16_to_f32(bits))
    }

    fn uleb(&mut self) -> Result<u64> {
        let mut result = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift > 56 {
                anyhow::bail!("ULEB128 溢出");
            }
        }
    }

    fn str_uleb(&mut self) -> Result<String> {
        let len = self.uleb()? as usize;
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
}

/// 半精度浮点（f16）转 f32。
fn half16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) as u32 & 1;
    let exp = (bits >> 10) as u32 & 0x1f;
    let frac = (bits & 0x3ff) as u32;
    let f = if exp == 0 {
        return if sign == 0 { 1. } else { -1. } * frac as f32 * 2f32.powi(-24);
    } else if exp == 31 {
        (sign << 31) | (0xff << 23) | (frac << 13)
    } else {
        (sign << 31) | ((exp as i32 + (127 - 15)) as u32) << 23 | frac << 13
    };
    f32::from_bits(f)
}

fn phase_of(p: macroquad::prelude::TouchPhase) -> u8 {
    use macroquad::prelude::TouchPhase::*;
    match p {
        Started => PHASE_DOWN,
        Moved => PHASE_MOVE,
        Ended | Cancelled => PHASE_UP,
        Stationary => PHASE_UP,
    }
}

fn accept_touch(fingers: &mut std::collections::HashSet<u64>, id: u64, phase: u8, active: bool) -> bool {
    if !active && (phase != PHASE_UP || !fingers.contains(&id)) {
        return false;
    }
    if phase == PHASE_UP {
        fingers.remove(&id);
    } else {
        fingers.insert(id);
    }
    true
}

fn judge_kind(res: &Result<prpr::judge::Judgement, bool>) -> u8 {
    match res {
        Ok(prpr::judge::Judgement::Perfect) => 0,
        Ok(prpr::judge::Judgement::Good) => 1,
        Ok(prpr::judge::Judgement::Bad) => 2,
        Ok(prpr::judge::Judgement::Miss) => 3,
        Ok(prpr::judge::Judgement::PerfectPlus) => 4,
        Ok(prpr::judge::Judgement::Great) => 7,
        Ok(prpr::judge::Judgement::Ok) => 8,
        Ok(prpr::judge::Judgement::Meh) => 9,
        Err(true) => 5,  // HoldPerfect
        Err(false) => 6, // HoldGood
    }
}

/// 录制中的回放数据（主线程独占）。
#[derive(Clone, Default)]
struct Pending {
    capture: capture::Capture,
    capture_origin: f64,
    capture_frame: Option<u64>,
    fingers: std::collections::HashSet<u64>,
    chart: Option<ChartRef>,
    offset: f32,
    speed: f32,
    mods: u32,
    frames: Vec<f64>,
    aspect_ratio: Option<f32>,
    settings: Option<crate::judgement_presets::JudgeSettings>,
    gameplay: Option<Gameplay>,
    theoretical_score: bool,
    player: Option<String>,
    grading: prpr::config::JudgeGrading,
    chart_file: Option<String>,
    chart_updated: Option<i64>,
    touches: Vec<TouchEvent>,
    judges: Vec<JudgeEvent>,
}

thread_local! {
    static NEXT_IDENTITY: std::cell::RefCell<Option<Metadata>> = const { std::cell::RefCell::new(None) };
    // The scene's recorder owns the buffer. Leaving the scene releases an
    // unfinished attempt; this handle can never keep it alive or save it later.
    static PENDING: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<Option<Pending>>>> = Default::default();
    /// 下一次开始录制时要附带的谱面元信息（由 `SongScene` 在启动游玩前写入）。
    static NEXT_META: std::cell::RefCell<Option<(Option<String>, Option<i64>)>> = const { std::cell::RefCell::new(None) };
}

struct Recording {
    buffer: std::rc::Rc<std::cell::RefCell<Option<Pending>>>,
    template: Pending,
    attempt: Option<u64>,
}

impl Recording {
    fn new(template: Pending) -> Self {
        let buffer = std::rc::Rc::new(std::cell::RefCell::new(None));
        PENDING.with(|it| *it.borrow_mut() = std::rc::Rc::downgrade(&buffer));
        Self { buffer, template, attempt: None }
    }

    fn begin_attempt(&mut self, attempt: u64) {
        if self.attempt == Some(attempt) {
            return;
        }
        let mut pending = self.template.clone();
        pending.capture_origin = prpr::judge::get_uptime();
        *self.buffer.borrow_mut() = Some(pending);
        self.attempt = Some(attempt);
    }
}

fn take_recording() -> Option<Pending> {
    PENDING.with(|it| it.borrow().upgrade()).and_then(|buffer| buffer.borrow_mut().take())
}

/// `SongScene` 启动游玩前调用：登记本次谱面的下载 URL 与 chart_updated，供录制写入回放。
pub fn set_next_chart_meta(file: Option<String>, chart_updated: Option<i64>) {
    NEXT_META.with(|it| *it.borrow_mut() = Some((file, chart_updated)));
}

/// 构建一个游玩时录制回放的 `UpdateFn` 钩子（随全局游玩闭包一起挂到 LoadingScene 上）。
/// 仅在 `GameMode::Normal` 且非 autoplay 时挂载。数据先攒在内存里，谱面结束后由
/// [`save_recording`] 用与成绩历史**同一个时间戳**落盘，保证详情页能按 `key+time` 找到。
pub fn recorder(chart: Option<ChartRef>, offset: f32, speed: f32, mods: u32) -> UpdateFn {
    let input_observer = prpr::input_capture::Observer::new();
    let (chart_file, chart_updated) = NEXT_META.with(|it| it.borrow_mut().take()).unwrap_or((None, None));
    let template = Pending {
        capture: capture::Capture::new(),
        capture_origin: prpr::judge::get_uptime(),
        capture_frame: None,
        fingers: Default::default(),
        chart,
        offset,
        speed,
        mods,
        frames: Vec::new(),
        aspect_ratio: None,
        settings: None,
        gameplay: None,
        theoretical_score: false,
        player: None,
        grading: crate::get_data().config.judge_grading,
        chart_file,
        chart_updated,
        touches: Vec::new(),
        judges: Vec::new(),
    };
    let mut recording = Recording::new(template);
    Box::new(move |t, res, judge| {
        let _keep_observer_alive = &input_observer;
        recording.begin_attempt(judge.recording_attempt());
        {
            let mut g = recording.buffer.borrow_mut();
            let Some(p) = g.as_mut() else { return };
            let active = judge.recording_active();
            let input_frame = prpr::input_capture::snapshot();
            if p.capture_frame != Some(input_frame.serial) {
                p.capture_frame = Some(input_frame.serial);
                let frame = p.capture.anchors.len() as u64;
                if p.capture.anchors.len() < 240_000 {
                    let vp = prpr::ext::get_viewport();
                    let vp = res.camera.viewport.unwrap_or(vp);
                    p.capture.anchors.push(capture::Anchor {
                        frame, uptime: prpr::judge::get_uptime() - p.capture_origin,
                        chart_time: t, active, speed: res.config.speed,
                        viewport: [vp.0, vp.1, vp.2, vp.3], screen_height: macroquad::prelude::screen_height(),
                    });
                    p.capture.dropped = p.capture.dropped.saturating_add(input_frame.dropped);
                    for mut event in input_frame.inputs {
                        if p.capture.inputs.len() >= 200_000 {
                            p.capture.dropped = p.capture.dropped.saturating_add(1);
                            continue;
                        }
                        event.event_time = event.event_time.map(|v| v - p.capture_origin);
                        event.observed_time -= p.capture_origin;
                        p.capture.inputs.push(capture::Input { seq: p.capture.inputs.len() as u64, frame, active, event });
                    }
                } else {
                    p.capture.dropped = p.capture.dropped.saturating_add(input_frame.inputs.len() as u64 + input_frame.dropped + 1);
                }
            }
            let t = if active {
                t.max(p.frames.last().copied().unwrap_or(t))
            } else {
                p.frames.last().copied().unwrap_or(t)
            };
            if active {
                p.frames.push(t);
            }
            p.aspect_ratio = Some(res.aspect_ratio);
            p.settings = Some(crate::judgement_presets::JudgeSettings::capture(&res.config));
            p.gameplay = Some(Gameplay::capture(&res.config));
            p.theoretical_score = res.config.theoretical_score;
            p.player = Some(res.config.player_name.clone());
            p.speed = res.config.speed;
            p.grading = res.config.judge_grading;
            // 触摸帧：非 Stationary 才记录，避免一帧刷好几条重复数据。
            for touch in Judge::get_touches() {
                if matches!(touch.phase, macroquad::prelude::TouchPhase::Stationary) {
                    continue;
                }
                let phase = phase_of(touch.phase);
                if !accept_touch(&mut p.fingers, touch.id, phase, active) {
                    continue;
                }
                let (sx, sy, sw, sh) = prpr::ext::get_viewport();
                let (vx, vy, vw, vh) = res.camera.viewport.unwrap_or((sx, sy, sw, sh));
                let px = sx as f32 + (touch.position.x + 1.) * sw as f32 / 2.;
                let py = sy as f32 + sh as f32 / 2. - touch.position.y * sw as f32 / 2.;
                p.touches.push(TouchEvent {
                    phase,
                    id: touch.id,
                    x: (px - vx as f32) * 2. / vw as f32 - 1.,
                    y: (vy as f32 + vh as f32 / 2. - py) * 2. / vw as f32,
                    t: t as f32,
                });
            }
            p.judges.extend(
                judge
                    .judgements
                    .borrow_mut()
                    .drain(..)
                    .map(|(t, line, note, kind, diff, head)| JudgeEvent {
                        t,
                        line,
                        note,
                        kind: judge_kind(&kind),
                        diff: diff as f32,
                        head_grade: head.map(|j| judge_kind(&Ok(j))),
                    }),
            );
        }
    })
}

/// 谱面结束后落盘：用与成绩历史相同的时间戳命名，使详情页能按 `key+time` 命中。
pub fn save_recording(key: &str, time_ms: i64) -> Option<PathBuf> {
    let p = take_recording()?;
    if p.judges.is_empty() && p.touches.is_empty() {
        return None;
    }
    let mut replay = Replay {
        capture: Some(p.capture),
        frames: p.frames,
        aspect_ratio: p.aspect_ratio,
        settings: p.settings,
        gameplay: p.gameplay,
        theoretical_score: p.theoretical_score,
        has_diffs: true,
        has_speed: true,
        chart: p.chart,
        offset: p.offset,
        speed: p.speed,
        mods: p.mods,
        grading: p.grading,
        touches: p.touches,
        judges: p.judges,
        chart_file: p.chart_file,
        chart_updated: p.chart_updated,
        ..Default::default()
    };
    // Preserve the real event times, with stable ordering for multiple events
    // in a frame. Clock correction can otherwise put a later frame first.
    if let Err(err) = canonicalize(&mut replay) {
        tracing::error!(?err, "invalid recording");
        return None;
    }
    replay.meta = NEXT_IDENTITY.with(|v| v.borrow().clone()).unwrap_or_default();
    replay.meta.id = uuid::Uuid::new_v4().to_string();
    replay.meta.recorded_at = Some(time_ms);
    replay.meta.player = p.player;
    replay.meta.player_id = crate::get_data().me.as_ref().map(|u| u.id);
    replay.meta.result = crate::history::all().into_iter().find(|r| r.key == key && r.time == time_ms);
    let path = path_for(key, time_ms);
    match save(&replay, &path) {
        Ok(()) => Some(path),
        Err(err) => {
            tracing::warn!(?err, "failed to save replay");
            None
        }
    }
}

/// 丢弃当前录制（例如未保存成绩时调用）。
pub fn discard_recording() {
    let _ = take_recording();
}

/// 便捷：按谱面引用构造 key（与 `history::record_play` 一致）。
pub fn key_for(chart_id: Option<i32>, local_path: Option<&str>) -> String {
    if let Some(id) = chart_id {
        format!("id:{id}")
    } else if let Some(p) = local_path {
        format!("local:{p}")
    } else {
        "unknown".to_owned()
    }
}

thread_local! {
    static EXPORT_BYTES: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
}
pub fn request_export(path: &std::path::Path) -> Result<()> {
    let mut replay = load(path)?;
    if let Some(item) = library::all()?.into_iter().find(|i| i.path == path) {
        replay.meta = item.meta;
        replay.meta.result = item.record.map(|mut r| {
            r.replay_file = None;
            r.replay_player = None;
            r.replay_imported = false;
            r
        });
    }
    if replay.meta.id.is_empty() {
        use sha2::{Digest, Sha256};
        replay.meta.id = format!("legacy-{:x}", Sha256::digest(std::fs::read(path)?));
    }
    let bytes = bytes(&replay)?;
    let name = format!("replay-{}.ppr", sanitize_key(&replay.meta.id).chars().take(64).collect::<String>());
    EXPORT_BYTES.with(|v| *v.borrow_mut() = Some(bytes));
    crate::page::request_export(name);
    Ok(())
}
pub fn export_batch(paths: &[std::path::PathBuf]) -> Result<()> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for path in paths {
        let tape = load(path)?;
        zip.start_file(
            path.file_name().context("缺少文件名")?.to_string_lossy(),
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        zip.write_all(&bytes(&tape)?)?;
    }
    request_export_bytes(format!("replays-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S")), zip.finish()?.into_inner());
    Ok(())
}
pub fn request_export_bytes(name: String, bytes: Vec<u8>) {
    EXPORT_BYTES.with(|v| *v.borrow_mut() = Some(bytes));
    crate::page::request_export(name);
}
pub fn poll_export() {
    if !EXPORT_BYTES.with(|v| v.borrow().is_some()) {
        return;
    }
    if let Some(config) = crate::page::take_export() {
        let bytes = EXPORT_BYTES.with(|v| v.borrow_mut().take()).unwrap();
        match config {
            Ok(mut config) => match config.file.write_all(&bytes).and_then(|_| config.file.flush()) {
                Ok(()) => crate::page::resolve_export(),
                Err(err) => {
                    let _ = (config.deleter)();
                    prpr::scene::show_error(err.into());
                }
            },
            Err(err) => prpr::scene::show_error(err.into()),
        }
    }
}
