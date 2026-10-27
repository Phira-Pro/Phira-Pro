//! phira-mp v1 wire format. No rendering or application state belongs here.
use anyhow::{bail, ensure, Result};
use std::collections::BTreeMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_MEMBERS: usize = 4096;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Selecting,
    Waiting,
    Playing,
}
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub monitor: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub id: String,
    pub phase: Phase,
    pub chart: Option<i32>,
    pub live: bool,
    pub locked: bool,
    pub cycle: bool,
    pub host: bool,
    pub ready: bool,
    pub users: BTreeMap<i32, User>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Authenticate(String),
    Chat(String),
    Create(String),
    Join(String),
    JoinMonitor(String),
    Leave,
    Lock(bool),
    Cycle(bool),
    Select(i32),
    Start,
    Ready,
    CancelReady,
    Abort,
}
impl Request {
    pub fn response_tag(&self) -> u8 {
        match self {
            Self::Authenticate(_) => 1,
            Self::Chat(_) => 2,
            Self::Create(_) => 8,
            Self::Join(_) | Self::JoinMonitor(_) => 9,
            Self::Leave => 11,
            Self::Lock(_) => 12,
            Self::Cycle(_) => 13,
            Self::Select(_) => 14,
            Self::Start => 15,
            Self::Ready => 16,
            Self::CancelReady => 17,
            Self::Abort => 19,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut b = vec![match self {
            Self::Authenticate(_) => 1,
            Self::Chat(_) => 2,
            Self::Create(_) => 5,
            Self::Join(_) | Self::JoinMonitor(_) => 6,
            Self::Leave => 7,
            Self::Lock(_) => 8,
            Self::Cycle(_) => 9,
            Self::Select(_) => 10,
            Self::Start => 11,
            Self::Ready => 12,
            Self::CancelReady => 13,
            Self::Abort => 15,
        }];
        match self {
            Self::Authenticate(v) => {
                ensure!(!v.is_empty() && v.len() <= 32, "联机鉴权需要有效的 Phira access token（最多 32 字节）");
                string(&mut b, v);
            }
            Self::Chat(v) => {
                ensure!(!v.is_empty() && v.len() <= 200, "聊天内容应为 1–200 字节");
                string(&mut b, v);
            }
            Self::Create(v) | Self::Join(v) | Self::JoinMonitor(v) => {
                validate_room(v)?;
                string(&mut b, v);
                if matches!(self, Self::Join(_) | Self::JoinMonitor(_)) {
                    b.push(matches!(self, Self::JoinMonitor(_)) as u8);
                }
            }
            Self::Lock(v) | Self::Cycle(v) => b.push(*v as u8),
            Self::Select(id) => {
                ensure!(*id > 0, "谱面 ID 无效");
                b.extend(id.to_le_bytes());
            }
            _ => {}
        }
        Ok(b)
    }
}
pub fn validate_room(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 20 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
        "房间号只能包含 1–20 位英文字母、数字、- 和 _"
    );
    Ok(())
}
pub fn uleb(b: &mut Vec<u8>, mut n: usize) {
    loop {
        let x = (n & 127) as u8;
        n >>= 7;
        b.push(x | if n != 0 { 128 } else { 0 });
        if n == 0 {
            break;
        }
    }
}
pub fn string(b: &mut Vec<u8>, s: &str) {
    uleb(b, s.len());
    b.extend(s.as_bytes());
}
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, payload: &[u8]) -> Result<()> {
    ensure!(!payload.is_empty() && payload.len() <= MAX_FRAME, "联机帧长度无效");
    let mut prefix = Vec::new();
    uleb(&mut prefix, payload.len());
    w.write_all(&prefix).await?;
    w.write_all(payload).await?;
    Ok(())
}
// This function is run by a dedicated reader, never cancelled in a select loop.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>> {
    let mut n = 0u32;
    for i in 0..5 {
        let v = r.read_u8().await?;
        ensure!(i != 4 || v < 16, "联机帧长度溢出");
        n |= u32::from(v & 127) << (i * 7);
        if v & 128 == 0 {
            ensure!(n > 0 && n as usize <= MAX_FRAME, "联机帧超过大小限制");
            let mut b = vec![0; n as usize];
            r.read_exact(&mut b).await?;
            return Ok(b);
        }
    }
    bail!("联机帧长度前缀过长")
}
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Unit,
    Auth(User, Option<Room>),
    Join(Phase, Option<i32>, Vec<User>, bool),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Chat(i32, String),
    Member(u8, i32, String),
    Selected(i32, String, i32),
    Player(u8, i32),
    StartPlaying,
    GameEnd,
    Played(i32, i32, f32, bool),
    Lock(bool),
    Cycle(bool),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Pong,
    Reply(u8, std::result::Result<Reply, String>),
    State(Phase, Option<i32>),
    Host(bool),
    Joined(User),
    Message(Message),
    Live(LiveBatch),
}
#[derive(Clone, Debug, PartialEq)]
pub enum LiveBatch {
    Touches { player: i32, frames: Vec<TouchFrame> },
    Judges { player: i32, events: Vec<JudgeEvent> },
}
#[derive(Clone, Debug, PartialEq)]
pub struct TouchFrame {
    pub time: f32,
    pub points: Vec<TouchPoint>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchPoint {
    /// Negative IDs are touch-end markers encoded by bitwise negation.
    pub id: i8,
    /// IEEE-754 binary16 bits, little-endian on the wire.
    pub x: u16,
    pub y: u16,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JudgeEvent {
    pub time: f32,
    pub line_id: u32,
    pub note_id: u32,
    /// 0 Perfect, 1 Good, 2 Bad, 3 Miss, 4 HoldPerfect, 5 HoldGood.
    pub judgement: u8,
}
pub struct Reader<'a> {
    b: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| anyhow::anyhow!("联机帧截断"))?;
        let b = &self.b[self.pos..end];
        self.pos = end;
        Ok(b)
    }
    pub fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn boolean(&mut self) -> Result<bool> {
        let b = self.byte()?;
        ensure!(b <= 1, "无效的布尔值");
        Ok(b == 1)
    }
    fn int(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn uint(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn float(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.int()? as u32);
        ensure!(v.is_finite(), "实时事件包含非有限数值");
        Ok(v)
    }
    fn count(&mut self, limit: usize) -> Result<usize> {
        let mut n = 0u32;
        for i in 0..5 {
            let b = self.byte()?;
            ensure!(i != 4 || b < 16, "长度溢出");
            n |= u32::from(b & 127) << (i * 7);
            if b & 128 == 0 {
                ensure!(n as usize <= limit, "集合或字符串过大");
                return Ok(n as usize);
            }
        }
        bail!("无效长度")
    }
    fn text(&mut self, max: usize) -> Result<String> {
        let n = self.count(max)?;
        Ok(std::str::from_utf8(self.take(n)?)?.to_owned())
    }
    fn user(&mut self) -> Result<User> {
        Ok(User {
            id: self.int()?,
            name: self.text(4096)?,
            monitor: self.boolean()?,
        })
    }
    fn phase(&mut self) -> Result<(Phase, Option<i32>)> {
        Ok(match self.byte()? {
            0 => (Phase::Selecting, if self.boolean()? { Some(self.int()?) } else { None }),
            1 => (Phase::Waiting, None),
            2 => (Phase::Playing, None),
            _ => bail!("未知房间阶段"),
        })
    }
    fn room(&mut self) -> Result<Room> {
        let id = self.text(20)?;
        validate_room(&id)?;
        let (phase, chart) = self.phase()?;
        let mut room = Room {
            id,
            phase,
            chart,
            live: self.boolean()?,
            locked: self.boolean()?,
            cycle: self.boolean()?,
            host: self.boolean()?,
            ready: self.boolean()?,
            users: BTreeMap::new(),
        };
        for _ in 0..self.count(MAX_MEMBERS)? {
            let id = self.int()?;
            let user = self.user()?;
            ensure!(user.id == id && room.users.insert(id, user).is_none(), "成员快照无效");
        }
        Ok(room)
    }
    fn message(&mut self) -> Result<Message> {
        let t = self.byte()?;
        Ok(match t {
            0 => Message::Chat(self.int()?, self.text(64 * 1024)?),
            1 | 4 | 6..=9 | 13 => Message::Player(t, self.int()?),
            2 | 3 => Message::Member(t, self.int()?, self.text(4096)?),
            5 => Message::Selected(self.int()?, self.text(4096)?, self.int()?),
            10 => Message::StartPlaying,
            12 => Message::GameEnd,
            11 => Message::Played(self.int()?, self.int()?, self.float()?, self.boolean()?),
            14 => Message::Lock(self.boolean()?),
            15 => Message::Cycle(self.boolean()?),
            _ => bail!("未知房间消息"),
        })
    }
}
/// Returns the unconsumed frame tail for future negotiated extensions.
pub fn decode(b: &[u8]) -> Result<(Event, &[u8])> {
    ensure!(b.len() <= MAX_FRAME, "联机帧过大");
    let mut r = Reader::new(b);
    let tag = r.byte()?;
    let e = match tag {
        0 => Event::Pong,
        5 => Event::Message(r.message()?),
        6 => {
            let (p, c) = r.phase()?;
            Event::State(p, c)
        }
        7 => Event::Host(r.boolean()?),
        10 => Event::Joined(r.user()?),
        1 | 2 | 8 | 9 | 11..=19 => {
            let result = if !r.boolean()? {
                Err(r.text(64 * 1024)?)
            } else {
                Ok(match tag {
                    1 => {
                        let u = r.user()?;
                        let room = if r.boolean()? { Some(r.room()?) } else { None };
                        Reply::Auth(u, room)
                    }
                    9 => {
                        let (p, c) = r.phase()?;
                        let mut users = Vec::new();
                        for _ in 0..r.count(MAX_MEMBERS)? {
                            users.push(r.user()?);
                        }
                        Reply::Join(p, c, users, r.boolean()?)
                    }
                    _ => Reply::Unit,
                })
            };
            Event::Reply(tag, result)
        }
        3 | 4 => {
            let player = r.int()?;
            let n = r.count(65536)?;
            if tag == 3 {
                let mut frames = Vec::with_capacity(n);
                for _ in 0..n {
                    let time = r.float()?;
                    let points_len = r.count(256)?;
                    let mut points = Vec::with_capacity(points_len);
                    for _ in 0..points_len {
                        let id = r.byte()? as i8;
                        let pos = r.take(4)?;
                        points.push(TouchPoint {
                            id,
                            x: u16::from_le_bytes([pos[0], pos[1]]),
                            y: u16::from_le_bytes([pos[2], pos[3]]),
                        });
                    }
                    frames.push(TouchFrame { time, points });
                }
                Event::Live(LiveBatch::Touches { player, frames })
            } else {
                let mut events = Vec::with_capacity(n);
                for _ in 0..n {
                    let time = r.float()?;
                    let line_id = r.uint()?;
                    let note_id = r.uint()?;
                    let judgement = r.byte()?;
                    ensure!(judgement <= 5, "未知实时判定");
                    events.push(JudgeEvent { time, line_id, note_id, judgement });
                }
                Event::Live(LiveBatch::Judges { player, events })
            }
        }
        _ => bail!("未知联机命令 {tag}"),
    };
    Ok((e, &b[r.pos..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_vectors_and_tail() {
        assert_eq!(Request::Select(7).encode().unwrap(), [10, 7, 0, 0, 0]);
        assert_eq!(decode(&[6, 0, 1, 7, 0, 0, 0]).unwrap().0, Event::State(Phase::Selecting, Some(7)));
        assert_eq!(decode(&[6, 1, 0xde, 0xad]).unwrap(), (Event::State(Phase::Waiting, None), &[0xde, 0xad][..]));
        assert_eq!(decode(&[16, 1]).unwrap().0, Event::Reply(16, Ok(Reply::Unit)));
    }
    #[test]
    fn malformed_frames_never_allocate_unbounded() {
        for b in [&[][..], &[255], &[6, 9], &[1, 1], &[5, 0, 1, 0, 0, 0, 255, 255, 255, 255, 127]] {
            assert!(decode(b).is_err());
        }
        assert!(Request::Create("含汉字".into()).encode().is_err());
        assert!(Request::Chat("字".repeat(67)).encode().is_err());
        assert!(Request::Authenticate("x".repeat(33)).encode().is_err());
    }
    #[tokio::test]
    async fn fragmentation_coalescing_and_frame_limit() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let task = tokio::spawn(async move {
            for byte in [4, 6, 1, 0xde, 0xad, 1, 0] {
                a.write_all(&[byte]).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        assert_eq!(decode(&read_frame(&mut b).await.unwrap()).unwrap().0, Event::State(Phase::Waiting, None));
        assert_eq!(read_frame(&mut b).await.unwrap(), [0]);
        task.await.unwrap();
        for bad in [vec![255, 255, 255, 255, 127], vec![0], vec![255, 255, 255, 1]] {
            assert!(read_frame(&mut bad.as_slice()).await.is_err());
        }
    }
}
