// 本地回放系统（Phira Pro）。
//
// 游玩时通过 `UpdateFn` 钩子记录两类数据：
// - 触摸帧：每帧快照 `Judge::get_touches()`（屏幕坐标，已按判定线坐标系换算），
//   供回放时画出手指触点。
// - 判定事件：`judge.judgements` 增量取出，带时间戳，回放时按原样重放到谱面上，
//   保证分数 / 准度 / 连击与原始游玩完全一致。
//
// 回放文件 `.phirar` 是自包含的二进制格式（魔数沿用 PHIRAREC 家族）：
//   magic "PHIRAREC" (8B) | version u32 = 3 |
//   chart_kind u8 (0=谱面 id，1=本地路径) |
//   谱面引用：id → i32；路径 → u8 长度 + UTF-8 字节 |
//   offset f32 | speed f32 | mods u32 |
//   touch_count u32 | judge_count u32 |
//   touches[]: u8 phase(0=按下 1=移动 2=抬起) + i8 触点 id + f32 x + f32 y + f32 t
//   judges[]: f64 t + u32 判定线 + u32 音符 + u8 判定(0..=4) + f32 diff
//
// 存储位置：`<data>/replays/<chart-key>_<时间戳>.phirar`，文件名与成绩历史记录的
// `key + time` 一一对应，因此「本地成绩详情」页可以按名字直接找到对应的回放。

use anyhow::{Context, Result};
use prpr::{
    judge::Judge,
    scene::UpdateFn,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

use crate::dir;

/// 触摸相位。
pub const PHASE_DOWN: u8 = 0;
pub const PHASE_MOVE: u8 = 1;
pub const PHASE_UP: u8 = 2;

/// 一条触摸帧。
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchEvent {
    pub phase: u8,
    pub id: i8,
    pub x: f32,
    pub y: f32,
    /// 谱面时间（秒，0 = 谱面偏移处，与 `Resource.time` 同基准）。
    pub t: f32,
}

/// 一条判定事件。
#[derive(Clone, Copy, Debug, Default)]
pub struct JudgeEvent {
    pub t: f64,
    pub line: u32,
    pub note: u32,
    /// 0=Perfect 1=Good 2=Bad 3=Miss 4=PerfectPlus 5=HoldPerfect 6=HoldGood
    pub kind: u8,
    pub diff: f32,
}

/// 谱面引用。
#[derive(Clone, Debug, PartialEq)]
pub enum ChartRef {
    Id(i32),
    Local(String),
}

/// 回放文件。
#[derive(Clone, Debug, Default)]
pub struct Replay {
    pub chart: Option<ChartRef>,
    pub offset: f32,
    pub speed: f32,
    pub mods: u32,
    pub touches: Vec<TouchEvent>,
    pub judges: Vec<JudgeEvent>,
    /// 录制时该在线谱面的压缩包 URL（内容寻址，每个版本不变），用于回放时下载对应版本。
    pub chart_file: Option<String>,
    /// 录制时谱面的 chart_updated（Unix 毫秒），用于判断本地谱面是否已更新。
    pub chart_updated: Option<i64>,
}

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
    key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// 一条成绩记录对应的回放文件路径（不存在时也返回该路径，用于探测）。
pub fn path_for(key: &str, time_ms: i64) -> PathBuf {
    PathBuf::from(format!("{}/replays/{}_{}.phirar", dir::root().unwrap_or_default(), sanitize_key(key), time_ms))
}

pub fn save(replay: &Replay, path: &PathBuf) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut buf = Vec::new();
    buf.write_all(b"PHIRAREC")?;
    buf.write_all(&3u32.to_le_bytes())?;
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
    let bytes = std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() < 12 {
        anyhow::bail!("不是回放文件");
    }
    if &bytes[..8] != b"PHIRAREC" {
        anyhow::bail!("不是回放文件");
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    match version {
        3 => parse_v3(&bytes),
        0 | 1 => parse_phirarec(&bytes, version),
        _ => anyhow::bail!("不支持的版本号：{version}"),
    }
}

/// 解析 Phira Pro 自己的回放格式（v3）。
fn parse_v3(bytes: &[u8]) -> Result<Replay> {
    let mut r = FileCursor::new(bytes);
    r.take(8)?; // magic
    r.u32()?; // version
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
    let mut touches = Vec::with_capacity(tn as usize);
    for _ in 0..tn {
        touches.push(TouchEvent {
            phase: r.u8()?,
            id: r.i8()?,
            x: r.f32()?,
            y: r.f32()?,
            t: r.f32()?,
        });
    }
    let mut judges = Vec::with_capacity(jn as usize);
    for _ in 0..jn {
        judges.push(JudgeEvent {
            t: r.f64()?,
            line: r.u32()?,
            note: r.u32()?,
            kind: r.u8()?,
            diff: r.f32()?,
        });
    }
    Ok(Replay { chart, offset, speed, mods, touches, judges, chart_file, chart_updated })
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
            1 => zstd::stream::decode_all(rest).context("ZSTD 解压失败")?,
            2 => {
                let mut out = Vec::new();
                flate2::read::ZlibDecoder::new(rest).read_to_end(&mut out).context("DEFLATE 解压失败")?;
                out
            }
            other => anyhow::bail!("未知压缩类型：{other}"),
        }
    } else {
        c.rest().to_vec()
    };
    let mut r = FileCursor::new(payload.as_slice());
    let _id = r.i32()?;
    if version == 1 {
        let _time = r.i64()?;
    }
    let chart = r.i32()?;
    let _chart_name = r.str_uleb()?;
    let _user = r.i32()?;
    let _user_name = r.str_uleb()?;

    let touch_count = r.uleb()? as usize;
    let mut touches = Vec::with_capacity(touch_count);
    for _ in 0..touch_count {
        let t = r.f32()?;
        let pts = r.uleb()? as usize;
        for _ in 0..pts {
            let id = r.i8()?;
            let x = r.f16()?;
            let y = r.f16()?;
            // MP 协议里正 id=按下/移动，负 id=抬起（`!id`）；y 按 16:9 还原回 UI 坐标。
            if id >= 0 {
                touches.push(TouchEvent {
                    phase: PHASE_MOVE,
                    id,
                    x,
                    y: y / 1.7777778,
                    t,
                });
            } else {
                touches.push(TouchEvent {
                    phase: PHASE_UP,
                    id: !id,
                    x,
                    y: y / 1.7777778,
                    t,
                });
            }
        }
    }

    let judge_count = r.uleb()? as usize;
    let mut judges = Vec::with_capacity(judge_count);
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
            _ => 6, // HoldGood
        };
        judges.push(JudgeEvent { t: t as f64, line, note, kind, diff: 0. });
    }

    Ok(Replay {
        chart: Some(ChartRef::Id(chart)),
        offset: 0.,
        speed: 1.,
        mods: 0,
        touches,
        judges,
        chart_file: None,
        chart_updated: None,
    })
}

#[cfg(test)]
mod tests {
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
        if self.pos + n > self.data.len() {
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
        if frac == 0 {
            sign << 31
        } else {
            // 次正规
            let e = 127 - 15;
            let m = frac;
            (sign << 31) | (e << 23) | m << 13
        }
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

fn judge_kind(res: &Result<prpr::judge::Judgement, bool>) -> u8 {
    match res {
        Ok(prpr::judge::Judgement::Perfect) => 0,
        Ok(prpr::judge::Judgement::Good) => 1,
        Ok(prpr::judge::Judgement::Bad) => 2,
        Ok(prpr::judge::Judgement::Miss) => 3,
        Ok(prpr::judge::Judgement::PerfectPlus) => 4,
        Err(true) => 5,  // HoldPerfect
        Err(false) => 6, // HoldGood
    }
}

/// 录制中的回放数据（主线程独占）。
struct Pending {
    chart: Option<ChartRef>,
    offset: f32,
    speed: f32,
    mods: u32,
    chart_file: Option<String>,
    chart_updated: Option<i64>,
    touches: Vec<TouchEvent>,
    judges: Vec<JudgeEvent>,
}

thread_local! {
    static PENDING: std::cell::RefCell<Option<Pending>> = std::cell::RefCell::new(None);
    /// 下一次开始录制时要附带的谱面元信息（由 `SongScene` 在启动游玩前写入）。
    static NEXT_META: std::cell::RefCell<Option<(Option<String>, Option<i64>)>> = const { std::cell::RefCell::new(None) };
}

/// `SongScene` 启动游玩前调用：登记本次谱面的下载 URL 与 chart_updated，供录制写入回放。
pub fn set_next_chart_meta(file: Option<String>, chart_updated: Option<i64>) {
    NEXT_META.with(|it| *it.borrow_mut() = Some((file, chart_updated)));
}

/// 构建一个游玩时录制回放的 `UpdateFn` 钩子（随全局游玩闭包一起挂到 LoadingScene 上）。
/// 仅在 `GameMode::Normal` 且非 autoplay 时挂载。数据先攒在内存里，谱面结束后由
/// [`save_recording`] 用与成绩历史**同一个时间戳**落盘，保证详情页能按 `key+time` 找到。
pub fn recorder(chart: Option<ChartRef>, offset: f32, speed: f32, mods: u32) -> UpdateFn {
    let (chart_file, chart_updated) = NEXT_META.with(|it| it.borrow_mut().take()).unwrap_or((None, None));
    PENDING.with(|it| {
        *it.borrow_mut() = Some(Pending {
            chart,
            offset,
            speed,
            mods,
            chart_file,
            chart_updated,
            touches: Vec::new(),
            judges: Vec::new(),
        })
    });
    Box::new(move |t, res, judge| {
        PENDING.with(|it| {
            let mut g = it.borrow_mut();
            let Some(p) = g.as_mut() else { return };
            let _ = res;
            // 触摸帧：非 Stationary 才记录，避免一帧刷好几条重复数据。
            for touch in Judge::get_touches() {
                if matches!(touch.phase, macroquad::prelude::TouchPhase::Stationary) {
                    continue;
                }
                p.touches.push(TouchEvent {
                    phase: phase_of(touch.phase),
                    id: touch.id as i8,
                    x: touch.position.x,
                    y: touch.position.y,
                    t: t as f32,
                });
            }
            p.judges.extend(
                judge
                    .judgements
                    .borrow_mut()
                    .drain(..)
                    .map(|(t, line, note, kind)| JudgeEvent {
                        t,
                        line,
                        note,
                        kind: judge_kind(&kind),
                        diff: 0.,
                    }),
            );
        });
    })
}

/// 谱面结束后落盘：用与成绩历史相同的时间戳命名，使详情页能按 `key+time` 命中。
pub fn save_recording(key: &str, time_ms: i64) -> Option<PathBuf> {
    let p = PENDING.with(|it| it.borrow_mut().take())?;
    if p.judges.is_empty() && p.touches.is_empty() {
        return None;
    }
    let replay = Replay {
        chart: p.chart,
        offset: p.offset,
        speed: p.speed,
        mods: p.mods,
        touches: p.touches,
        judges: p.judges,
        chart_file: p.chart_file,
        chart_updated: p.chart_updated,
    };
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
    PENDING.with(|it| *it.borrow_mut() = None);
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
