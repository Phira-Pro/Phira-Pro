//! Ordered room reducer. No UI frames, network writes or asynchronous continuations.
use super::protocol::{Event, Message, Phase, Reply, Request, Room, User};
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub connected: bool,
    pub authenticated: bool,
    pub reconnecting: bool,
    pub reconnect_attempt: u8,
    pub me: Option<User>,
    pub room: Option<Room>,
    pub chart_generation: u64,
    pub room_generation: u64,
    pub prepared: Option<(u64, i32)>,
    pub round: u64,
    pub start: Option<Launch>,
    pub active: bool,
    pub finished: bool,
    pub messages: VecDeque<String>,
    pub error: Option<String>,
    pub busy: bool,
    pub rtt_ms: Option<f32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launch {
    pub room_generation: u64,
    pub chart_generation: u64,
    pub round: u64,
    pub chart: i32,
}
impl Snapshot {
    pub fn accepts_live(&self, launch: Launch) -> bool {
        self.connected
            && self.active
            && self.room_generation == launch.room_generation
            && self.chart_generation == launch.chart_generation
            && self.round == launch.round
            && self.room.as_ref().is_some_and(|r| r.phase == Phase::Playing)
    }
    /// Invalidate every resource/game continuation owned by the old transport.
    pub fn disconnected(&mut self) {
        self.connected = false;
        self.authenticated = false;
        self.busy = false;
        self.rtt_ms = None;
        self.room_generation += 1;
        self.chart_generation += 1;
        self.start = None;
        self.prepared = None;
        self.active = false;
        self.finished = false;
    }
    pub fn validate(&self, req: &Request) -> Result<()> {
        if matches!(req, Request::Authenticate(_)) {
            ensure!(!self.authenticated, "会话已经鉴权");
            return Ok(());
        }
        ensure!(self.authenticated, "尚未完成联机鉴权");
        if matches!(req, Request::Create(_) | Request::Join(_)) {
            ensure!(self.room.is_none(), "请先离开当前房间");
            return Ok(());
        }
        let room = self.room.as_ref().ok_or_else(|| anyhow::anyhow!("尚未加入房间"))?;
        match req {
            Request::Select(_) => ensure!(room.host && room.phase == Phase::Selecting, "只有选曲阶段的房主可以选曲"),
            Request::Lock(_) | Request::Cycle(_) => ensure!(room.host, "只有房主可以修改房间设置"),
            Request::Start | Request::Ready => {
                ensure!(self.prepared == room.chart.map(|c| (self.chart_generation, c)), "请先完成当前谱面的资源检查");
                ensure!(room.chart.is_some(), "尚未选曲");
                if matches!(req, Request::Start) {
                    ensure!(room.host && room.phase == Phase::Selecting, "当前无法发起开局");
                } else {
                    ensure!(room.phase == Phase::Waiting, "当前不在等待准备阶段");
                }
            }
            Request::CancelReady => ensure!(room.phase == Phase::Waiting, "当前不在等待准备阶段"),
            Request::Abort => ensure!(room.phase == Phase::Playing && self.finished, "当前没有待结束的联机参与"),
            _ => {}
        }
        Ok(())
    }
    pub fn log(&mut self, text: impl Into<String>) {
        if self.messages.len() == 128 {
            self.messages.pop_front();
        }
        self.messages.push_back(text.into());
    }
    fn select(&mut self, chart: Option<i32>) {
        if self.room.as_ref().and_then(|r| r.chart) != chart {
            self.chart_generation += 1;
            self.prepared = None;
        }
        if let Some(r) = &mut self.room {
            r.chart = chart;
        }
    }
    pub fn install(&mut self, room: Option<Room>) {
        self.room_generation += 1;
        self.chart_generation += 1;
        self.prepared = None;
        self.start = None;
        self.active = false;
        self.finished = false;
        self.room = room;
    }
    pub fn mark_prepared(&mut self, generation: u64, chart: i32) -> Result<()> {
        ensure!(self.chart_generation == generation && self.room.as_ref().and_then(|r| r.chart) == Some(chart), "下载期间房间已换谱，请重新准备");
        self.prepared = Some((generation, chart));
        Ok(())
    }
    pub fn check_launch(&mut self) {
        if self.active || self.finished || self.start.is_some() {
            return;
        }
        let Some(r) = &self.room else {
            return;
        };
        if r.phase != Phase::Playing {
            return;
        }
        match r.chart {
            Some(chart) if self.prepared == Some((self.chart_generation, chart)) => {
                self.round += 1;
                self.start = Some(Launch {
                    room_generation: self.room_generation,
                    chart_generation: self.chart_generation,
                    round: self.round,
                    chart,
                });
                self.active = true;
            }
            _ => {
                self.error = Some("服务端已开局，但当前谱面未准备完成；已阻止启动旧谱面".into());
            }
        }
    }
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::State(p, c) => {
                if p == Phase::Selecting {
                    if self.room.as_ref().is_some_and(|r| r.phase != Phase::Selecting) {
                        // A return to selection ends the old participation even if
                        // GameEnd was omitted. Invalidate delayed preparation too.
                        self.chart_generation += 1;
                        self.prepared = None;
                        self.start = None;
                        self.active = false;
                        self.finished = false;
                    }
                    self.select(c);
                }
                if let Some(r) = &mut self.room {
                    if r.phase != p {
                        r.ready = r.host;
                    }
                    r.phase = p;
                }
                self.check_launch();
            }
            Event::Host(v) => {
                if let Some(r) = &mut self.room {
                    r.host = v;
                }
            }
            Event::Joined(u) => {
                if let Some(r) = &mut self.room {
                    r.live |= u.monitor;
                    r.users.insert(u.id, u);
                }
            }
            Event::Message(m) => {
                match &m {
                    Message::Selected(_, _, chart) => self.select(Some(*chart)),
                    Message::Member(3, id, _) => {
                        if let Some(r) = &mut self.room {
                            r.users.remove(id);
                        }
                    }
                    Message::Lock(v) => {
                        if let Some(r) = &mut self.room {
                            r.locked = *v;
                        }
                    }
                    Message::Cycle(v) => {
                        if let Some(r) = &mut self.room {
                            r.cycle = *v;
                        }
                    }
                    Message::GameEnd => {
                        self.finished = false;
                        self.active = false;
                        self.start = None;
                        self.prepared = None;
                    }
                    _ => {}
                }
                self.log(match m {
                    Message::Chat(user, text) => format!("{user}: {text}"),
                    Message::Member(t, user, name) => format!("{name} ({user}) {}", if t == 2 { "加入房间" } else { "离开房间" }),
                    Message::Selected(_, name, id) => format!("选曲：{name} · {id}"),
                    Message::StartPlaying => "开始游玩".into(),
                    Message::GameEnd => "本回合结束".into(),
                    Message::Played(id, score, acc, _) => format!("{id} · {score:07} · {:.2}%", acc * 100.),
                    Message::Player(tag, id) => format!(
                        "{id} · {}",
                        match tag {
                            1 => "创建房间",
                            4 => "成为房主",
                            6 => "等待准备",
                            7 => "已准备",
                            8 => "取消准备",
                            9 => "取消开局",
                            13 => "放弃本局",
                            _ => "状态更新",
                        }
                    ),
                    Message::Lock(v) => if v { "房间已锁定" } else { "房间已解锁" }.into(),
                    Message::Cycle(v) => if v { "已开启轮换房主" } else { "已关闭轮换房主" }.into(),
                });
            }
            // Live streams are validated by the protocol decoder; consumers get a separate bounded path later.
            Event::Live { .. } | Event::Pong | Event::Reply(..) => {}
        }
    }
    pub fn reply(&mut self, req: &Request, reply: Reply, generation: u64) -> Result<()> {
        match (req, reply) {
            (Request::Authenticate(_), Reply::Auth(me, room)) => {
                self.me = Some(me);
                self.authenticated = true;
                self.install(room);
            }
            (Request::Create(id), Reply::Unit) => {
                let me = self.me.clone().ok_or_else(|| anyhow::anyhow!("尚未鉴权"))?;
                self.install(Some(Room {
                    id: id.clone(),
                    phase: Phase::Selecting,
                    chart: None,
                    live: false,
                    locked: false,
                    cycle: false,
                    host: true,
                    ready: false,
                    users: BTreeMap::from([(me.id, me)]),
                }));
            }
            (Request::Join(id), Reply::Join(phase, chart, users, live)) => self.install(Some(Room {
                id: id.clone(),
                phase,
                chart,
                live,
                locked: false,
                cycle: false,
                host: false,
                ready: false,
                users: users.into_iter().map(|u| (u.id, u)).collect(),
            })),
            (Request::Leave, Reply::Unit) => self.install(None),
            (Request::Ready | Request::Start, Reply::Unit) => {
                if self.chart_generation == generation {
                    if let Some(r) = &mut self.room {
                        r.ready = true;
                    }
                }
            }
            (Request::CancelReady, Reply::Unit) => {
                if self.chart_generation == generation {
                    if let Some(r) = &mut self.room {
                        r.ready = false;
                    }
                }
            }
            (_, Reply::Unit) => {}
            _ => anyhow::bail!("联机响应类型与请求不匹配"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Snapshot {
        let mut s = Snapshot::default();
        s.me = Some(User {
            id: 1,
            name: "test".into(),
            monitor: false,
        });
        s.reply(&Request::Create("test".into()), Reply::Unit, 0).unwrap();
        s
    }
    #[test]
    fn all_phases_in_one_frame_keep_chart_and_start_once() {
        let mut s = state();
        s.apply(Event::State(Phase::Selecting, Some(7)));
        let g = s.chart_generation;
        s.mark_prepared(g, 7).unwrap();
        s.apply(Event::State(Phase::Waiting, None));
        s.apply(Event::State(Phase::Playing, None));
        let launch = s.start.take().unwrap();
        s.apply(Event::State(Phase::Playing, None));
        assert_eq!(s.round, launch.round);
        assert!(s.start.is_none());
        s.finished = true;
        s.active = false;
        s.apply(Event::State(Phase::Selecting, Some(7)));
        s.apply(Event::State(Phase::Playing, None));
        assert!(s.start.is_none());
    }
    #[test]
    fn stale_download_and_missing_chart_never_start() {
        let mut s = state();
        s.apply(Event::State(Phase::Selecting, Some(1)));
        let g = s.chart_generation;
        s.apply(Event::State(Phase::Selecting, Some(2)));
        assert!(s.mark_prepared(g, 1).is_err());
        s.apply(Event::State(Phase::Playing, None));
        assert!(s.start.is_none());
        assert!(s.error.is_some());
    }
    #[test]
    fn leaving_invalidates_tasks_and_game_end_allows_next_round() {
        let mut s = state();
        s.apply(Event::State(Phase::Selecting, Some(1)));
        s.mark_prepared(s.chart_generation, 1).unwrap();
        s.apply(Event::State(Phase::Playing, None));
        s.finished = true;
        s.active = false;
        s.apply(Event::Message(Message::GameEnd));
        s.apply(Event::State(Phase::Selecting, Some(2)));
        s.mark_prepared(s.chart_generation, 2).unwrap();
        s.apply(Event::State(Phase::Playing, None));
        assert_eq!(s.round, 2);
        let g = s.chart_generation;
        s.install(None);
        assert!(s.mark_prepared(g, 2).is_err());
    }

    #[test]
    fn returning_to_selection_without_game_end_invalidates_old_round_and_live() {
        let mut s = state();
        s.connected = true;
        s.apply(Event::State(Phase::Selecting, Some(7)));
        s.mark_prepared(s.chart_generation, 7).unwrap();
        s.apply(Event::State(Phase::Playing, None));
        let old = s.start.take().unwrap();
        assert!(s.accepts_live(old));
        s.apply(Event::State(Phase::Selecting, Some(7)));
        assert!(!s.accepts_live(old));
        assert!(!s.active);
        assert!(s.mark_prepared(old.chart_generation, 7).is_err());
        s.mark_prepared(s.chart_generation, 7).unwrap();
        s.apply(Event::State(Phase::Playing, None));
        let next = s.start.take().unwrap();
        assert!(s.accepts_live(next));
        assert!(!s.accepts_live(old));
        assert_ne!(old.round, next.round);
        assert_ne!(old.chart_generation, next.chart_generation);
    }
}
