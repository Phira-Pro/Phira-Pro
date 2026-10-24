//! Thin multiplayer page. Network state stays in Session while gameplay covers this page.
use super::{
    game_bridge::GameContext,
    protocol::{Phase, Request},
    room::Launch,
    session::Session,
    view::RoomView,
};
use crate::{
    data::BriefChartInfo,
    get_data, get_data_mut,
    icons::Icons,
    page::{Page, SharedState},
    save_data,
    scene::{Downloading, SongScene},
};
use anyhow::{Context, Result};
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::poll_future,
    scene::{request_input, return_input, show_error, show_message, take_input, LocalSceneTask, NextScene},
    task::Task,
    ui::Ui,
};
use std::{borrow::Cow, sync::Arc};

struct Preparation {
    room: u64,
    generation: u64,
    chart: i32,
    start: bool,
}
pub struct MultiplayerPage {
    address: String,
    session: Option<Session>,
    connect: Option<Task<Result<Session>>>,
    operation: Option<Task<Result<()>>>,
    view: RoomView,
    close_requested: bool,
    metadata: Option<Task<Result<super::resources::ResourceCheck>>>,
    download: Option<Downloading>,
    preparation: Option<Preparation>,
    prepared_path: Option<(u64, u64, String)>,
    scene_task: LocalSceneTask,
    next: Option<NextScene>,
    game: Option<GameContext>,
    return_pending: bool,
    last_error: Option<String>,
    chart_picker_requested: bool,
}
impl MultiplayerPage {
    pub fn new(icons: Arc<Icons>) -> Self {
        Self {
            address: get_data().multiplayer_address.clone(),
            session: None,
            connect: None,
            operation: None,
            view: RoomView::new(icons),
            close_requested: false,
            metadata: None,
            download: None,
            preparation: None,
            prepared_path: None,
            scene_task: None,
            next: None,
            game: None,
            return_pending: false,
            last_error: None,
            chart_picker_requested: false,
        }
    }
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }
    pub fn take_chart_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.chart_picker_requested)
    }
    pub fn select_chart(&mut self, id: i32) {
        self.request(Request::Select(id));
    }
    pub fn shutdown(&self) {
        if let Some(s) = &self.session {
            s.close();
        }
    }
    pub fn invalidate_controls(&mut self) {
        self.view.invalidate();
    }
    pub fn hide(&mut self, t: f32) {
        self.view.suspend(t);
    }
    fn request(&mut self, req: Request) {
        if self.operation.is_some() {
            return;
        }
        if let Some(s) = &self.session {
            let s = s.clone();
            self.operation = Some(Task::new(async move { s.request(req).await }));
        }
    }
    fn begin_prepare(&mut self) -> Result<()> {
        let s = self.session.as_ref().context("尚未连接")?.snapshot();
        anyhow::ensure!(s.connected && s.authenticated && !s.reconnecting, "请等待联机连接恢复");
        let room = s.room.context("尚未加入房间")?;
        anyhow::ensure!(room.phase != Phase::Playing, "本局已经开始，请等待下一局");
        let chart = room.chart.context("请先选择谱面")?;
        self.preparation = Some(Preparation {
            room: s.room_generation,
            generation: s.chart_generation,
            chart,
            start: room.host && room.phase == Phase::Selecting,
        });
        let existing = get_data().charts.iter().find(|c| c.info.id == Some(chart)).map(|c| c.local_path.clone());
        self.metadata = Some(Task::new(super::resources::resolve(chart, existing)));
        Ok(())
    }
    fn preparation_valid(&self) -> bool {
        match (&self.session, &self.preparation) {
            (Some(s), Some(p)) => {
                let s = s.snapshot();
                s.connected
                    && s.room_generation == p.room
                    && s.chart_generation == p.generation
                    && s.room.as_ref().and_then(|r| r.chart) == Some(p.chart)
            }
            _ => false,
        }
    }
    fn ready(&mut self, path: String) -> Result<()> {
        if !self.preparation_valid() {
            anyhow::bail!("下载期间房间或谱面已改变，请重新准备");
        }
        let p = self.preparation.take().unwrap();
        let session = self.session.as_ref().unwrap();
        session.mark_prepared(p.generation, p.chart)?;
        self.prepared_path = Some((p.room, p.generation, path));
        self.request(if p.start { Request::Start } else { Request::Ready });
        Ok(())
    }
    fn start_scene(&mut self, launch: Launch) -> Result<()> {
        let path = self
            .prepared_path
            .as_ref()
            .filter(|(r, g, _)| *r == launch.room_generation && *g == launch.chart_generation)
            .map(|(_, _, p)| p.clone())
            .context("开局谱面资源已过期")?;
        let context = GameContext::new(self.session.as_ref().unwrap().clone(), launch);
        self.scene_task = SongScene::multiplayer_launch(launch.chart, &path, context.clone())?;
        self.game = Some(context);
        Ok(())
    }
}
impl Page for MultiplayerPage {
    fn label(&self) -> Cow<'static, str> {
        "多人联机".into()
    }
    fn enter(&mut self, _: &mut SharedState) -> Result<()> {
        if self.return_pending {
            self.return_pending = false;
            if let Some(game) = self.game.take() {
                let completed = game.completed();
                if game.session.finish(game.launch).is_ok() {
                    self.request(Request::Abort);
                    show_message(if completed {
                        "已完成本局；联机服务端按放弃结束，成绩由 Pro 保存"
                    } else {
                        "已退出本局"
                    })
                    .ok();
                } else {
                    show_message(if completed {
                        "本局已在 Pro 保存；连接或回合已改变，未向新回合发送结束请求"
                    } else {
                        "已退出本局；连接或回合已改变，未向新回合发送结束请求"
                    })
                    .warn();
                }
            }
        }
        Ok(())
    }
    fn on_back_pressed(&mut self, _: &mut SharedState) -> bool {
        if self.connect.is_some()
            || self.operation.is_some()
            || self
                .session
                .as_ref()
                .is_some_and(|s| s.snapshot().connected && s.snapshot().room.is_some())
        {
            show_message("请先取消当前操作并离开房间").warn();
            true
        } else {
            if let Some(s) = &self.session {
                s.close();
            }
            false
        }
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if let Some(d) = &mut self.download {
            if d.touch(touch, s.t) {
                self.download = None;
                self.preparation = None;
                self.metadata = None;
            }
            return Ok(true);
        }
        let (handled, action) = self.view.touch(touch, s.t);
        let Some(i) = action else {
            return Ok(handled);
        };
        if i == 11 {
            self.close_requested = true;
            return Ok(true);
        }
        if self.connect.is_some() || self.operation.is_some() {
            return Ok(true);
        }
        let snapshot = self.session.as_ref().map(Session::snapshot).unwrap_or_default();
        match i {
            0 => request_input("mp_address", InputBox::new().default_text(self.address.clone())),
            1 => {
                if snapshot.connected || snapshot.reconnecting {
                    if let Some(session) = self.session.take() {
                        session.close();
                    }
                    self.prepared_path = None;
                } else {
                    anyhow::ensure!(!get_data().config.offline_mode, "请先关闭离线模式");
                    let token = get_data().tokens.as_ref().context("请先登录 Phira 账号")?.0.clone();
                    let address = self.address.clone();
                    self.connect = Some(Task::new(async move { Session::connect(&address, token).await }));
                }
            }
            2 => request_input("mp_create", InputBox::new()),
            3 => request_input("mp_join", InputBox::new()),
            4 => {
                self.chart_picker_requested = true;
                self.close_requested = true;
            }
            5 => {
                if let Err(e) = self.begin_prepare() {
                    show_error(e);
                }
            }
            6 => self.request(Request::CancelReady),
            7 => self.request(Request::Leave),
            8 => {
                if let Some(r) = snapshot.room {
                    self.request(Request::Lock(!r.locked));
                }
            }
            9 => {
                if let Some(r) = snapshot.room {
                    self.request(Request::Cycle(!r.cycle));
                }
            }
            10 => request_input("mp_chat", InputBox::new()),
            _ => {}
        }
        Ok(true)
    }
    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "mp_address" => match super::session::address(&text) {
                    Ok(address) => {
                        self.address = address;
                        get_data_mut().multiplayer_address = self.address.clone();
                        save_data()?;
                    }
                    Err(e) => show_error(e),
                },
                "mp_create" => self.request(Request::Create(text)),
                "mp_join" => self.request(Request::Join(text)),
                "mp_chart" => match text.trim().parse::<i32>() {
                    Ok(id) => self.request(Request::Select(id)),
                    Err(_) => {
                        show_message("请输入有效谱面 ID").warn();
                    }
                },
                "mp_chat" => self.request(Request::Chat(text)),
                _ => return_input(id, text),
            }
        }
        if let Some(result) = self.connect.as_mut().and_then(Task::take) {
            self.connect = None;
            match result {
                Ok(session) => {
                    self.session = Some(session);
                    self.last_error = None;
                }
                Err(e) => show_error(e),
            }
        }
        if let Some(result) = self.operation.as_mut().and_then(Task::take) {
            self.operation = None;
            if let Err(e) = result {
                show_error(e);
            }
        }
        if self.preparation.is_some() && !self.preparation_valid() {
            self.metadata = None;
            self.download = None;
            self.preparation = None;
            self.prepared_path = None;
        }
        if let Some(result) = self.metadata.as_mut().and_then(Task::take) {
            self.metadata = None;
            match result {
                Err(e) => {
                    self.preparation = None;
                    show_error(e);
                }
                Ok(check) => {
                    if let Some(path) = check.current {
                        if let Err(e) = self.ready(path) {
                            show_error(e);
                        }
                    } else {
                        self.download =
                            Some(SongScene::global_start_download(BriefChartInfo::from_chart(&check.chart), (*check.chart).clone(), check.existing)?);
                    }
                }
            }
        }
        if let Some(d) = &mut self.download {
            if let Some(result) = d.check()? {
                self.download = None;
                if let Some((path, _, _, _)) = result {
                    if let Err(e) = self.ready(path) {
                        show_error(e);
                    }
                } else {
                    self.preparation = None;
                }
            }
        }
        if let Some(session) = &self.session {
            let snapshot = session.snapshot();
            if snapshot.error != self.last_error {
                self.last_error = snapshot.error.clone();
                if let Some(e) = &snapshot.error {
                    show_message(e).warn();
                }
            }
            if self.scene_task.is_none() && !self.return_pending {
                if let Some(launch) = session.take_launch() {
                    if let Err(e) = self.start_scene(launch) {
                        if let Some(s) = &self.session {
                            let _ = s.finish(launch);
                        }
                        show_error(e);
                        self.request(Request::Abort);
                    }
                }
            }
        }
        if let Some(result) = self.scene_task.as_mut().and_then(|f| poll_future(f.as_mut())) {
            self.scene_task = None;
            match result {
                Ok(next) => {
                    if self.game.as_ref().is_some_and(|g| g.session.live_context_valid(g.launch)) {
                        self.next = Some(next);
                        self.return_pending = true;
                    } else {
                        self.game = None;
                        show_message("加载期间连接或回合已改变，已取消启动旧局").warn();
                    }
                }
                Err(e) => {
                    if let Some(g) = self.game.take() {
                        let _ = g.session.finish(g.launch);
                    }
                    self.request(Request::Abort);
                    show_error(e);
                }
            }
        }
        Ok(())
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let snapshot = self.session.as_ref().map(Session::snapshot).unwrap_or_default();
        self.view.update(&snapshot, s.t);
        self.view.render(
            ui,
            &snapshot,
            &self.address,
            self.connect.is_some(),
            self.metadata.is_some() || self.download.is_some(),
            self.operation.is_some() || snapshot.busy,
            s.t,
        );
        if let Some(d) = &mut self.download {
            d.render(ui, s.t);
        }
        Ok(())
    }
    fn next_scene(&mut self, _: &mut SharedState) -> NextScene {
        self.next.take().unwrap_or_default()
    }
}
