use crate::{
    client::{Chart, Ptr, UserManager},
    dir, get_data,
    mp::L10N_LOCAL,
    scene::{Downloading, SongScene, RECORD_ID},
};
use anyhow::{anyhow, Context, Result};
use inputbox::{InputBox, InputMode};
use macroquad::prelude::*;
use phira_mp_client::Client;
use phira_mp_common::{RoomId, RoomState};
use prpr::{
    config::Mods,
    core::{Smooth, Tweenable},
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture},
    info::ChartInfo,
    scene::{request_input, return_input, show_error, show_message, take_input, GameMode, NextScene},
    task::Task,
    time::TimeManager,
    ui::{DRectButton, DrawText},
    ui::{Scroll, Ui},
};
use smallvec::SmallVec;
use std::{
    fs::File,
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tracing::warn;

#[cfg(feature = "local-mp")]
use super::local;
#[cfg(feature = "local-mp")]
use crate::{get_data_mut, save_data};

const ENTER_TRANSIT: f32 = 0.5;
const USER_LIST_TRANSIT: f32 = 0.4;
const WIDTH: f32 = 1.6;

const CHAT_ENABLED: bool = cfg!(feature = "chat");

/// Phira Pro：服务器列表里三种在线状态的颜色（在线浅绿 / 离线淡红 / 未知灰）。
#[cfg(feature = "local-mp")]
const SERVER_ONLINE: Color = Color::new(0.55, 0.92, 0.62, 1.);
#[cfg(feature = "local-mp")]
const SERVER_OFFLINE: Color = Color::new(0.96, 0.62, 0.62, 1.);
#[cfg(feature = "local-mp")]
const SERVER_UNKNOWN: Color = Color::new(0.78, 0.78, 0.78, 1.);

fn screen_size() -> (u32, u32) {
    (screen_width() as u32, screen_height() as u32)
}

struct Message {
    content: String,
    y: f32,
    bottom: f32,
    color: Color,
}

impl Message {
    pub fn text<'a, 's, 'ui>(&'s self, ui: &'ui mut Ui<'a>, mw: f32) -> DrawText<'a, 's, 'ui> {
        ui.text(&self.content)
            .pos(0., self.y)
            .size(0.4)
            .color(self.color)
            .max_width(mw)
            .multiline()
    }
}

pub struct MPPanel {
    pub client: Option<Arc<Client>>,

    side_enter_time: f32,

    msg_scroll: Scroll,
    msgs: Vec<Message>,
    msgs_dirty_from: usize,
    last_screen_size: (u32, u32),

    connect_btn: DRectButton,
    connect_task: Option<Task<Result<Client>>>,

    /// Phira Pro：本地联机（房主）——在客户端内起服务端，局域网内可连。
    #[cfg(feature = "local-mp")]
    local_btn: DRectButton,
    /// Phira Pro：房间列表（走服务端的 HTTP 查询 API `/api/rooms`）。
    #[cfg(feature = "local-mp")]
    room_list_btn: DRectButton,
    #[cfg(feature = "local-mp")]
    only_public_btn: DRectButton,
    #[cfg(feature = "local-mp")]
    room_list_task: Option<Task<Result<String>>>,
    #[cfg(feature = "local-mp")]
    only_public: bool,

    create_room_btn: DRectButton,
    create_room_task: Option<Task<Result<()>>>,
    join_room_btn: DRectButton,
    join_room_task: Option<Task<Result<RoomState>>>,
    /// Phira Pro：等待输入房间密码的待办（true=建房，false=进房）。
    pending_room: Option<(bool, RoomId)>,
    leave_room_btn: DRectButton,

    disconnect_btn: DRectButton,

    request_start_btn: DRectButton,
    lock_room_btn: DRectButton,
    cycle_room_btn: DRectButton,

    ready_btn: DRectButton,
    cancel_ready_btn: DRectButton,

    chat_text: String,
    chat_btn: DRectButton,
    chat_send_btn: DRectButton,
    chat_task: Option<Task<Result<()>>>,

    download_task: Option<Task<Result<Arc<Chart>>>>,
    downloading: Option<Downloading>,
    // true for request_start, false for ready
    download_next: bool,

    chart_id: Option<i32>,
    game_start_consumed: bool,
    need_upload: bool,
    entered: bool,

    next_scene: Option<NextScene>,

    task: Option<Task<Result<()>>>,

    scene_task: LocalTask<Result<NextScene>>,

    user_list_btn: DRectButton,
    user_list_p: Smooth<f32>,
    user_list_scroll: Scroll,
    icon_user: SafeTexture,

    /// Phira Pro：本地联机时，把供他人连接的局域网地址固定显示在信息面板第一行。
    #[cfg(feature = "local-mp")]
    local_addr: Option<String>,

    /// 当前正在连接 / 已连接的服务端地址（断线重连时用同一个地址）。
    conn_addr: Option<String>,

    /// Phira Pro：「服务器列表」——拉取状态站 → 可选可连的浮层。
    #[cfg(feature = "local-mp")]
    server_list_btn: DRectButton,
    #[cfg(feature = "local-mp")]
    server_list_task: Option<Task<Result<Vec<local::ServerEntry>>>>,
    #[cfg(feature = "local-mp")]
    servers: Vec<local::ServerEntry>,
    #[cfg(feature = "local-mp")]
    server_btns: Vec<DRectButton>,
    #[cfg(feature = "local-mp")]
    server_list_p: Smooth<f32>,
    #[cfg(feature = "local-mp")]
    server_list_scroll: Scroll,
}

impl MPPanel {
    pub fn new(icon_user: SafeTexture) -> Self {
        Self {
            client: None,

            side_enter_time: f32::INFINITY,

            msg_scroll: Scroll::new(),
            msgs: Vec::new(),
            msgs_dirty_from: 0,
            last_screen_size: screen_size(),

            connect_btn: DRectButton::new(),
            connect_task: None,

            #[cfg(feature = "local-mp")]
            local_btn: DRectButton::new(),
            #[cfg(feature = "local-mp")]
            room_list_btn: DRectButton::new(),
            #[cfg(feature = "local-mp")]
            only_public_btn: DRectButton::new(),
            #[cfg(feature = "local-mp")]
            room_list_task: None,
            #[cfg(feature = "local-mp")]
            only_public: true,

            create_room_btn: DRectButton::new(),
            create_room_task: None,
            join_room_btn: DRectButton::new(),
            join_room_task: None,
            pending_room: None,
            leave_room_btn: DRectButton::new(),

            disconnect_btn: DRectButton::new(),

            request_start_btn: DRectButton::new(),
            lock_room_btn: DRectButton::new(),
            cycle_room_btn: DRectButton::new(),

            ready_btn: DRectButton::new(),
            cancel_ready_btn: DRectButton::new(),

            chat_text: String::new(),
            chat_btn: DRectButton::new().with_delta(-0.002),
            chat_send_btn: DRectButton::new(),
            chat_task: None,

            download_task: None,
            downloading: None,
            download_next: false,

            chart_id: None,
            game_start_consumed: false,
            need_upload: false,
            entered: false,

            next_scene: None,

            task: None,

            scene_task: None,

            user_list_btn: DRectButton::new(),
            user_list_p: Smooth::default(),
            user_list_scroll: Scroll::new(),
            icon_user,

            #[cfg(feature = "local-mp")]
            local_addr: None,

            conn_addr: None,

            #[cfg(feature = "local-mp")]
            server_list_btn: DRectButton::new(),
            #[cfg(feature = "local-mp")]
            server_list_task: None,
            #[cfg(feature = "local-mp")]
            servers: Vec::new(),
            #[cfg(feature = "local-mp")]
            server_btns: Vec::new(),
            #[cfg(feature = "local-mp")]
            server_list_p: Smooth::default(),
            #[cfg(feature = "local-mp")]
            server_list_scroll: Scroll::new(),
        }
    }

    fn clone_client(&self) -> Arc<Client> {
        Arc::clone(self.client.as_ref().unwrap())
    }

    fn has_task(&self) -> bool {
        self.connect_task.is_some()
            || self.create_room_task.is_some()
            || self.chat_task.is_some()
            || self.download_task.is_some()
            || self.task.is_some()
            || self.scene_task.is_some()
    }

    /// Phira Pro：本地联机用的昵称。优先用登录账号名；否则用配置里保存的；
    /// 都没有就随机生成一个并存下来（保证稳定、且不同设备不会重名）。
    #[cfg(feature = "local-mp")]
    fn local_nickname() -> String {
        if let Some(name) = get_data()
            .me
            .as_ref()
            .map(|it| it.name.clone())
            .filter(|it| !it.is_empty())
        {
            return name;
        }
        let cur = get_data().config.mp_nickname.clone();
        if !cur.is_empty() {
            return cur;
        }
        let name = format!("Player{:04}", ::rand::random::<u16>() % 10000);
        get_data_mut().config.mp_nickname = name.clone();
        save_data().ok();
        name
    }

    /// Phira Pro：「本地联机」按钮。分两种：
    /// - 没填「本地联机地址」→ 在本机开内嵌服务端当房主，连到本机；
    /// - 填了地址 → 直接连过去（若是本机地址，连之前会把服务端起起来）。
    #[cfg(feature = "local-mp")]
    fn host_local(&mut self) {
        let local = get_data().config.mp_local_address.trim().to_owned();
        if local.is_empty() {
            if !local::is_running() {
                match local::start(local::DEFAULT_PORT, local::DEFAULT_HTTP_PORT) {
                    Ok(port) => {
                        let addr = local::display_addr(port);
                        // 固定显示在信息面板第一行（提示框太快消失，且宽度有限）。
                        self.local_addr = Some(addr.clone());
                        // 分两条发：提示框宽度有限，把地址单独放一条才不会显示成 `192.1...`。
                        show_message(mtl!("local-mp-started")).ok();
                        show_message(addr).ok();
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("local-mp-start-failed")));
                        return;
                    }
                }
            } else {
                self.local_addr = Some(local::display_addr(local::port()));
                show_message(mtl!("local-mp-started")).ok();
            }
            let port = local::port();
            self.connect_to(format!("127.0.0.1:{port}"));
        } else {
            // 信息面板第一行显示这个本地地址。
            self.local_addr = Some(local.clone());
            self.connect_to(local);
        }
    }

    /// Phira Pro：拉取房间列表（写进面板消息区）。
    #[cfg(feature = "local-mp")]
    fn fetch_rooms(&mut self) {
        // 查当前连的这台自建服务器（本地 / 局域网地址）；没有就退回多人地址的主机。
        let raw = if let Some(a) = &self.local_addr {
            a.clone()
        } else if local::is_running() {
            "127.0.0.1".to_owned()
        } else {
            get_data().config.mp_address.clone()
        };
        let host = Self::addr_host(&raw).unwrap_or("127.0.0.1").to_owned();
        let http_port = local::DEFAULT_HTTP_PORT;
        self.room_list_task = Some(Task::new(async move { local::fetch_rooms(&host, http_port).await }));
    }

    /// Phira Pro：拉取「服务器列表」（状态站）并弹浮层。
    #[cfg(feature = "local-mp")]
    fn fetch_servers(&mut self) {
        let configured = get_data().config.mp_server_list_url.clone();
        // 留空时回退到默认状态站。
        let url = if configured.trim().is_empty() {
            prpr::config::DEFAULT_MP_SERVER_LIST_URL.to_owned()
        } else {
            configured
        };
        show_message(mtl!("local-mp-servers-loading")).ok();
        self.server_list_task = Some(Task::new(async move { local::fetch_server_list(&url).await }));
    }

    /// Phira Pro：把 `/api/rooms` 的 JSON 渲染成消息区的几行。
    #[cfg(feature = "local-mp")]
    fn push_rooms(&mut self, text: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            show_message(mtl!("local-mp-rooms-failed")).error();
            return;
        };
        let rooms = v.get("rooms").and_then(|it| it.as_array()).cloned().unwrap_or_default();
        let only_public = self.only_public;
        let mut shown = 0;
        for r in &rooms {
            let locked = r.get("lock").and_then(|it| it.as_bool()).unwrap_or(false);
            if only_public && locked {
                continue;
            }
            let id = r.get("roomid").and_then(|it| it.as_str()).unwrap_or("?");
            let state = match r.get("state").and_then(|it| it.as_str()).unwrap_or("") {
                "select_chart" => mtl!("local-mp-state-select").into_owned(),
                "wait_for_ready" => mtl!("local-mp-state-ready").into_owned(),
                "playing" => mtl!("local-mp-state-playing").into_owned(),
                _ => "?".to_owned(),
            };
            let lock = if locked {
                mtl!("local-mp-private")
            } else {
                mtl!("local-mp-public")
            };
            // Phira Pro：设了密码的房间在列表里额外标一个「密码」。
            let password = if r.get("password").and_then(|it| it.as_bool()).unwrap_or(false) {
                format!("  {}", mtl!("local-mp-password"))
            } else {
                String::new()
            };
            let host = r.get("host").and_then(|it| it.get("name")).and_then(|it| it.as_str()).unwrap_or("-");
            let players = r.get("players").and_then(|it| it.as_array()).map(|it| it.len()).unwrap_or(0);
            let chart = r
                .get("chart")
                .and_then(|it| it.get("name"))
                .and_then(|it| it.as_str())
                .unwrap_or("-");
            let content = format!("#{id}  {lock}  {state}{password}  {players}人  {host}  {chart}");
            let i = self.msgs.len();
            self.msgs.push(Message {
                content,
                y: 0.,
                bottom: 0.,
                color: WHITE,
            });
            self.msgs_dirty_from = self.msgs_dirty_from.min(i);
            shown += 1;
        }
        if shown == 0 {
            show_message(mtl!("local-mp-rooms-empty")).warn();
        }
    }

    /// Phira Pro：目标地址是不是「本机」（本地联机）。
    #[cfg(feature = "local-mp")]
    fn is_loopback_addr(addr: &str) -> bool {
        let host = addr
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        let host = match host.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => host,
        };
        host == "localhost" || host.starts_with("127.")
    }

    /// Phira Pro：目标是不是「本机 / 局域网里自建的服务端」（回环或私有网段）。
    /// 这类服务器走免登录昵称 token 直接接入，不走官方回源校验。
    #[cfg(feature = "local-mp")]
    fn is_private_host(addr: &str) -> bool {
        let host = addr
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        let host = match host.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => host,
        };
        if host.eq_ignore_ascii_case("localhost") || host.starts_with("127.") {
            return true;
        }
        let octets: Vec<u8> = host.split('.').map(|s| s.parse::<u8>()).collect::<Result<_, _>>().unwrap_or_default();
        if octets.len() != 4 {
            return false;
        }
        matches!((octets[0], octets[1]), (10, _) | (192, 168) | (172, 16..=31))
    }

    /// 取地址里的主机名（去掉协议头 / 路径 / 端口）。
    #[cfg(feature = "local-mp")]
    fn addr_host(addr: &str) -> Option<&str> {
        let host = addr
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        let host = match host.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => host,
        };
        (!host.is_empty()).then_some(host)
    }

    /// 这个地址是不是指向「本机」（回环，或本机的局域网 IP）。指向本机时，
    /// 连之前要确保内嵌服务端已经在跑。
    #[cfg(feature = "local-mp")]
    fn is_own_server_addr(addr: &str) -> bool {
        if Self::is_loopback_addr(addr) {
            return true;
        }
        let ip = local::local_ip();
        Self::addr_host(addr).is_some_and(|h| ip.as_deref() == Some(h))
    }

    fn connect(&mut self) {
        self.connect_to(get_data().config.mp_address.clone());
    }

    /// Phira Pro：连接到指定地址。「连接」用「多人联机地址」，「本地联机」用「本地联机地址」。
    fn connect_to(&mut self, addr: String) {
        // Phira Pro：连的是本机 / 局域网里自建的服务端时，直接用免登录昵称 token。
        // 这种服务器对官方 token 要回源校验，一旦校验失败就整个「鉴权失败」连不上；
        // 用 `local:<昵称>` 则由服务端直接合成用户（昵称仍取登录账号名，身份不丢）。
        #[cfg(feature = "local-mp")]
        let token = if Self::is_private_host(&addr) {
            local::local_token(&Self::local_nickname())
        } else {
            match get_data().tokens.as_ref().map(|it| it.0.clone()) {
                Some(t) => t,
                None => local::local_token(&Self::local_nickname()),
            }
        };
        #[cfg(not(feature = "local-mp"))]
        let token = match get_data().tokens.as_ref().map(|it| it.0.clone()) {
            Some(t) => t,
            None => {
                show_message(mtl!("connect-must-login")).error();
                return;
            }
        };
        // 记住这次连的地址，断线重连时仍连它。
        self.conn_addr = Some(addr.clone());
        // 连的是本机 / 局域网里的服务器时，把地址固定显示在信息面板第一行。
        #[cfg(feature = "local-mp")]
        {
            if Self::is_loopback_addr(&addr) {
                let port = addr
                    .rsplit_once(':')
                    .and_then(|it| it.1.parse::<u16>().ok())
                    .unwrap_or(local::DEFAULT_PORT);
                self.local_addr = Some(local::display_addr(port));
            } else if Self::is_private_host(&addr) {
                self.local_addr = Some(addr.clone());
            }
        }
        #[cfg(feature = "local-mp")]
        let start_local = Self::is_own_server_addr(&addr);
        self.connect_task = Some(Task::new(async move {
            // Phira Pro：断开连接会把本地服务端关掉，所以这里若发现目标是本机、
            // 而服务端没在跑，就自动重新起来 —— 否则重连会直接「目标计算机积极拒绝」。
            #[cfg(feature = "local-mp")]
            if start_local && !local::is_running() {
                let port = addr
                    .rsplit_once(':')
                    .and_then(|it| it.1.parse::<u16>().ok())
                    .unwrap_or(local::DEFAULT_PORT);
                // 等上一轮的监听端口彻底释放再绑定。
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                local::start(port, local::DEFAULT_HTTP_PORT)?;
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            let client = Client::from_address(&addr).await?;
            client
                .authenticate(token)
                .await
                .with_context(|| anyhow!(mtl!("connect-authenticate-failed")))?;
            Ok(client)
        }));
    }

    fn create_room(&mut self, id: RoomId, password: Option<String>) {
        let client = self.clone_client();
        self.create_room_task = Some(Task::new(async move {
            client.create_room_with_password(id, password).await?;
            Ok(())
        }));
    }

    /// Phira Pro：加入房间（可选带密码，作为协议 Trailer 发送）。
    fn join_room(&mut self, id: RoomId, password: Option<String>) {
        let client = self.clone_client();
        self.join_room_task = Some(Task::new(async move {
            client.join_room_with_password(id, false, password).await?;
            client
                .room_state()
                .await
                .ok_or_else(|| anyhow!("expected room state"))
        }));
    }

    pub fn select_chart(&mut self, id: i32) {
        let client = self.clone_client();
        if !client.blocking_is_host().unwrap() {
            show_message(mtl!("select-chart-host-only")).error();
            return;
        }
        if !matches!(client.blocking_room_state(), Some(RoomState::SelectChart(_))) {
            show_message(mtl!("select-chart-not-now")).error();
            return;
        }
        self.task = Some(Task::new(async move {
            client.select_chart(id).await.with_context(|| mtl!("select-chart-failed"))?;
            Ok(())
        }));
    }

    fn request_start(&mut self) {
        if matches!(self.client.as_ref().unwrap().blocking_room_state().unwrap(), RoomState::SelectChart(None)) {
            show_message(mtl!("request-start-no-chart")).error();
            return;
        }
        self.check_download(true);
    }

    fn check_download(&mut self, next: bool) {
        // 兜底：重连 / 重启后可能拿到 Playing 但没有谱面信息，别直接 unwrap panic。
        let Some(id) = self.chart_id else {
            show_message(mtl!("request-start-no-chart")).error();
            return;
        };
        self.download_next = next;
        self.download_task = Some(Task::new(async move { Ptr::new(id).fetch().await }));
    }

    fn post_download(&mut self) {
        let client = self.clone_client();
        if self.download_next {
            self.task = Some(Task::new(async move {
                client.request_start().await.with_context(|| mtl!("request-start-failed"))?;
                Ok(())
            }));
        } else {
            self.task = Some(Task::new(async move {
                client.ready().await.with_context(|| mtl!("ready-failed"))?;
                Ok(())
            }));
        }
    }
}

impl MPPanel {
    #[inline]
    pub fn in_room(&self) -> bool {
        self.client.as_ref().is_some_and(|it| it.blocking_room_id().is_some())
    }

    #[inline]
    pub fn show(&mut self, rt: f32) {
        self.side_enter_time = rt;
    }

    pub fn enter(&mut self) {
        self.entered = true;
    }

    pub fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> bool {
        let t = tm.now() as f32;
        if self.side_enter_time.is_infinite() {
            return false;
        }
        if self.user_list_p.transiting(t) {
            return true;
        }
        if *self.user_list_p.to() > 0.5 {
            if self.user_list_scroll.touch(touch, t) {
                return true;
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.user_list_p.goto(0., t, USER_LIST_TRANSIT);
            }
            return true;
        }
        // Phira Pro：服务器列表浮层（未连接时也能用）。点某一行 → 写入「多人地址」并连接。
        #[cfg(feature = "local-mp")]
        {
            if self.server_list_p.transiting(t) {
                return true;
            }
            if *self.server_list_p.to() > 0.5 {
                // 拖动列表时把按钮的按压取消掉：拖动一开始，后续触控事件就不再交给按钮，
                // 否则按钮会卡在「按下」形态一直不回弹（看起来比别的按钮小一圈）。
                if self.server_list_scroll.touch(touch, t) {
                    for btn in &mut self.server_btns {
                        btn.cancel(t);
                    }
                    return true;
                }
                // 起始点落在视口外（例如被裁掉的半行）不触发按钮，避免误点。
                if touch.phase == TouchPhase::Started && !self.server_list_scroll.contains(touch) {
                    return true;
                }
                for i in 0..self.server_btns.len() {
                    if self.server_btns[i].touch(touch, t) {
                        if let Some(srv) = self.servers.get(i) {
                            let addr = srv.addr.clone();
                            get_data_mut().config.mp_address = addr.clone();
                            save_data().ok();
                            self.server_list_p.goto(0., t, USER_LIST_TRANSIT);
                            self.connect_to(addr);
                        }
                        return true;
                    }
                }
                if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.server_list_p.goto(0., t, USER_LIST_TRANSIT);
                }
                return true;
            }
        }
        if !(self.side_enter_time > 0. && tm.real_time() as f32 > self.side_enter_time + ENTER_TRANSIT) {
            return true;
        }
        if self.has_task() {
            return true;
        }
        if let Some(dl) = &mut self.downloading {
            if dl.touch(touch, t) {
                self.downloading = None;
                return true;
            }
        }
        if touch.position.x + 1. > WIDTH {
            self.side_enter_time = -tm.real_time() as f32;
            return true;
        }
        // 未连接时的联机面板：连接 / 本地联机。
        // 关键：这些按钮只有在「对应面板正在显示」时才能做命中判定。DRectButton 会保留
        // 上一次绘制时的坐标；若不限制，离开房间后按钮的残留坐标会和房间里的
        // 「离开房间 / 用户列表」重合，把点击抢走（导致按离开房间却弹出列表、按钮也不回弹）。
        if self.client.is_none() {
            #[cfg(feature = "local-mp")]
            if self.local_btn.touch(touch, t) {
                self.host_local();
                return true;
            }
            #[cfg(feature = "local-mp")]
            if self.server_list_btn.touch(touch, t) {
                self.fetch_servers();
                return true;
            }
            if self.connect_btn.touch(touch, t) {
                self.connect();
                return true;
            }
        }
        if let Some(client) = &self.client {
            if self.msg_scroll.touch(touch, t) {
                return true;
            }
            // 聊天栏在「已连接」的任何状态下都渲染，所以命中判定也不能依赖房间状态
            // （之前嵌在 blocking_state() 里，房间状态还没到时点了没反应）。
            if CHAT_ENABLED {
                if self.chat_btn.touch(touch, t) {
                    request_input("chat", InputBox::new().default_text(&self.chat_text));
                    return true;
                }
                if self.chat_send_btn.touch(touch, t) {
                    if self.chat_text.is_empty() {
                        show_message(mtl!("chat-empty")).error();
                    } else {
                        let client = Arc::clone(client);
                        let text = self.chat_text.clone();
                        self.chat_task = Some(Task::new(async move { client.chat(text).await }));
                    }
                    return true;
                }
            }
            if let Some(state) = client.blocking_state() {
                let is_host = state.is_host;
                match state.state {
                    RoomState::SelectChart(_) => {
                        if is_host {
                            if self.request_start_btn.touch(touch, t) {
                                self.request_start();
                                return true;
                            }
                            if self.lock_room_btn.touch(touch, t) {
                                let to = !state.locked;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.lock_room(to).await.with_context(|| mtl!("lock-room-failed")) }));
                                return true;
                            }
                            if self.cycle_room_btn.touch(touch, t) {
                                let to = !state.cycle;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.cycle_room(to).await.with_context(|| mtl!("cycle-room-failed")) }));
                                return true;
                            }
                        }
                        if self.leave_room_btn.touch(touch, t) {
                            // 顺手收起用户列表面板，避免离开后还残留一层遮罩挡住按钮。
                            self.user_list_p.goto(0., t, 0.01);
                            let client = self.clone_client();
                            self.task = Some(Task::new(async move { client.leave_room().await }));
                            return true;
                        }
                    }
                    RoomState::WaitingForReady => {
                        if client.blocking_is_ready().unwrap() {
                            if self.cancel_ready_btn.touch(touch, t) {
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.cancel_ready().await }));
                                return true;
                            }
                        } else if self.ready_btn.touch(touch, t) {
                            self.check_download(false);
                            return true;
                        }
                    }
                    _ => {}
                }
                if self.user_list_btn.touch(touch, t) {
                    self.user_list_scroll.y_scroller.reset();
                    self.user_list_p.goto(1., t, USER_LIST_TRANSIT);
                    if let Some(state) = client.blocking_state() {
                        state.users.keys().copied().for_each(UserManager::request);
                    }
                    return true;
                }
            } else {
                // 已连接但尚未进入房间：房间列表 / 仅公开。和渲染分支一一对应。
                #[cfg(feature = "local-mp")]
                if self.room_list_btn.touch(touch, t) {
                    self.fetch_rooms();
                    return true;
                }
                #[cfg(feature = "local-mp")]
                if self.only_public_btn.touch(touch, t) {
                    self.only_public = !self.only_public;
                    self.fetch_rooms();
                    return true;
                }
                if self.create_room_btn.touch(touch, t) {
                    request_input("room_id", InputBox::new());
                    return true;
                }
                if self.join_room_btn.touch(touch, t) {
                    request_input("join_room", InputBox::new());
                    return true;
                }
                if self.disconnect_btn.touch(touch, t) {
                    self.client = None;
                    self.msgs.clear();
                    self.msgs_dirty_from = 0;
                    self.user_list_p.goto(0., t, 0.01);
                    self.local_addr = None;
                    // Phira Pro：断开时顺手把本地联机服务端关掉，
                    // 否则下次点「本地联机」会一直提示"已在运行"。
                    #[cfg(feature = "local-mp")]
                    if local::is_running() {
                        local::stop();
                    }
                    return true;
                }
            }
            if client.ping_fail_count() >= 2 && self.connect_task.is_none() {
                warn!("lost connection, reconnecting…");
                show_message(mtl!("reconnect")).warn();
                // 重连回原来那个地址（可能是本地/局域网地址，不一定是「多人地址」）。
                let addr = self.conn_addr.clone().unwrap_or_else(|| get_data().config.mp_address.clone());
                self.connect_to(addr);
            }
        }
        true
    }

    pub fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        if self.side_enter_time < 0. && -tm.real_time() as f32 + ENTER_TRANSIT < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }
        let new_size = screen_size();
        if self.last_screen_size != new_size {
            self.last_screen_size = new_size;
            self.msgs_dirty_from = 0;
        }
        self.msg_scroll.update(t);
        if self.user_list_p.now(t) > 1e-4 {
            self.user_list_scroll.update(t);
        }
        #[cfg(feature = "local-mp")]
        if self.server_list_p.now(t) > 1e-4 {
            self.server_list_scroll.update(t);
        }
        if let Some(client) = &self.client {
            self.msgs.extend(client.blocking_take_messages().into_iter().map(|msg| {
                use phira_mp_common::Message as M;
                match msg {
                    M::Chat { user, content, .. } => Message {
                        content: format!("{}：{content}", client.user_name(user)),
                        y: 0.,
                        bottom: 0.,
                        color: WHITE,
                    },
                    msg => {
                        let content = match msg {
                            M::Chat { .. } => unreachable!(),
                            M::CreateRoom { user } => {
                                mtl!("msg-create-room", "user" => client.user_name(user))
                            }
                            M::JoinRoom { name, .. } => {
                                mtl!("msg-join-room", "user" => name)
                            }
                            M::LeaveRoom { name, .. } => {
                                mtl!("msg-leave-room", "user" => name)
                            }
                            M::NewHost { user } => {
                                mtl!("msg-new-host", "user" => client.user_name(user))
                            }
                            M::SelectChart { user, name, id } => {
                                mtl!("msg-select-chart", "user" => client.user_name(user), "chart" => name, "id" => id)
                            }
                            M::GameStart { user } => {
                                mtl!("msg-game-start", "user" => client.user_name(user))
                            }
                            M::Ready { user } => {
                                mtl!("msg-ready", "user" => client.user_name(user))
                            }
                            M::CancelReady { user } => {
                                mtl!("msg-cancel-ready", "user" => client.user_name(user))
                            }
                            M::CancelGame { user } => {
                                mtl!("msg-cancel-game", "user" => client.user_name(user))
                            }
                            M::StartPlaying => mtl!("msg-start-playing").into_owned(),
                            M::Played { user, score, accuracy, full_combo } => {
                                mtl!("msg-played", "user" => client.user_name(user), "score" => format!("{score:07}"), "accuracy" => format!("{:.2}%", accuracy * 100.), "full-combo" => full_combo.to_string())
                            }
                            M::GameEnd => mtl!("msg-game-end").into_owned(),
                            M::Abort { user } => mtl!("msg-abort", "user" => client.user_name(user)),
                            M::LockRoom { lock } => mtl!("msg-room-lock", "lock" => lock.to_string()),
                            M::CycleRoom { cycle } => mtl!("msg-room-cycle", "cycle" => cycle.to_string()),
                        };
                        Message {
                            content,
                            y: 0.,
                            bottom: 0.,
                            color: semi_white(0.7),
                        }
                    }
                }
            }));
            let state = client.blocking_room_state();
            if matches!(state, Some(RoomState::Playing)) {
                if !self.game_start_consumed {
                    self.game_start_consumed = true;
                    // 兜底：例如重连 / 重启后进入一个已经在进行中的房间，本地没有谱面信息，
                    // 此时不能再 unwrap（会 panic），只能提示用户。
                    if let Some(id) = self.chart_id {
                        RECORD_ID.store(-1, Ordering::Relaxed);
                        self.need_upload = true;
                        self.entered = false;
                        self.scene_task = SongScene::global_launch(
                            Some(id),
                            &format!("download/{id}"),
                            Mods::default(),
                            GameMode::NoRetry,
                            self.client.as_ref().map(Arc::clone),
                            None,
                            None,
                            false,
                        )?;
                    } else {
                        self.need_upload = false;
                        show_message(mtl!("request-start-no-chart")).error();
                    }
                }
            } else {
                self.game_start_consumed = false;
            }
            if let Some(RoomState::SelectChart(chart)) = state {
                self.chart_id = chart;
            }
        }
        if let Some(task) = &mut self.connect_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(client) => {
                        show_message(mtl!("connect-success")).ok();
                        self.client = Some(client.into());
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("connect-failed")));
                    }
                }
                self.connect_task = None;
            }
        }
        #[cfg(feature = "local-mp")]
        if let Some(task) = &mut self.room_list_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(text) => self.push_rooms(&text),
                    Err(err) => show_error(err.context(mtl!("local-mp-rooms-failed"))),
                }
                self.room_list_task = None;
            }
        }
        #[cfg(feature = "local-mp")]
        if let Some(task) = &mut self.server_list_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(list) if !list.is_empty() => {
                        self.servers = list;
                        self.server_btns.clear();
                        self.server_btns.resize_with(self.servers.len(), DRectButton::new);
                        self.server_list_scroll.y_scroller.reset();
                        self.server_list_p.goto(1., t, USER_LIST_TRANSIT);
                    }
                    Ok(_) => {
                        show_message(mtl!("local-mp-servers-empty")).warn();
                    }
                    Err(err) => show_error(err.context(mtl!("local-mp-servers-failed"))),
                }
                self.server_list_task = None;
            }
        }
        if let Some(task) = &mut self.create_room_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(_) => {
                        show_message(mtl!("create-room-success")).ok();
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("create-room-failed")));
                    }
                }
                self.create_room_task = None;
            }
        }
        if let Some(task) = &mut self.download_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(entity) => {
                        let path = format!("download/{}", entity.id);
                        let info_path = format!("{}/{path}/info.yml", dir::charts()?);
                        let should_download = if Path::new(&info_path).exists() {
                            let local_info: ChartInfo = serde_yaml::from_reader(File::open(info_path)?)?;
                            local_info
                                .updated
                                .map_or(entity.updated != entity.created, |local_updated| local_updated != entity.updated)
                        } else {
                            true
                        };
                        if should_download {
                            let info = entity.to_info();
                            self.downloading = Some(SongScene::global_start_download(info, Chart::clone(&entity), {
                                if Path::new(&format!("{}/{path}", dir::charts()?)).exists() {
                                    Some(path)
                                } else {
                                    None
                                }
                            })?);
                        } else {
                            self.post_download();
                        }
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("download-failed")));
                    }
                }
                self.download_task = None;
            }
        }
        if let Some(dl) = &mut self.downloading {
            if let Some(res) = dl.check()? {
                if res.is_some() {
                    self.post_download();
                }
                self.downloading = None;
            }
        }
        if let Some(task) = &mut self.chat_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(_) => {
                        show_message(mtl!("chat-sent")).ok();
                        self.chat_text.clear();
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("chat-send-failed")));
                    }
                }
                self.chat_task = None;
            }
        }
        if let Some(task) = &mut self.task {
            if let Some(res) = task.take() {
                if let Err(err) = res {
                    show_error(err);
                }
                self.task = None;
            }
        }
        if let Some(task) = &mut self.join_room_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(mtl!("join-room-failed")));
                    }
                    Ok(state) => {
                        self.chart_id = match state {
                            RoomState::SelectChart(id) => id,
                            _ => None,
                        };
                    }
                }
                // 之前这里错写成 `self.task = None`（复制粘贴漏改），既没清掉 join_room_task，
                // 又可能把仍在进行中的另一个 task 直接丢掉。
                self.join_room_task = None;
            }
        }
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "chat" => {
                    self.chat_text = text;
                }
                // Phira Pro：先输房间 ID，再输房间密码（可留空 = 无密码）。
                "room_id" => {
                    let room_id: RoomId = text.try_into().with_context(|| mtl!("create-invalid-id"))?;
                    self.pending_room = Some((true, room_id));
                    request_input(
                        "room_pw",
                        InputBox::new()
                            .title(mtl!("room-password"))
                            .prompt(mtl!("room-password-prompt"))
                            .mode(InputMode::Password),
                    );
                }
                "join_room" => {
                    match RoomId::try_from(text) {
                        Ok(room_id) => {
                            self.pending_room = Some((false, room_id));
                            request_input(
                                "room_pw",
                                InputBox::new()
                                    .title(mtl!("room-password"))
                                    .prompt(mtl!("room-password-prompt"))
                                    .mode(InputMode::Password),
                            );
                        }
                        Err(_) => {
                            show_message(mtl!("join-room-invalid-id")).error();
                        }
                    }
                }
                "room_pw" => {
                    if let Some((is_create, room_id)) = self.pending_room.take() {
                        let password = if text.trim().is_empty() { None } else { Some(text) };
                        if is_create {
                            self.create_room(room_id, password);
                        } else {
                            self.join_room(room_id, password);
                        }
                    }
                }
                _ => return_input(id, text),
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        show_error(err);
                    }
                    Ok(scene) => self.next_scene = Some(scene),
                }
                self.scene_task = None;
            }
        }
        if self.need_upload && self.entered {
            let id = RECORD_ID.load(Ordering::Relaxed);
            if id != -1 {
                let client = self.clone_client();
                self.task = Some(Task::new(async move { client.played(id).await }));
            } else {
                let client = self.clone_client();
                self.task = Some(Task::new(async move { client.abort().await }));
            }
            self.need_upload = false;
        }
        Ok(())
    }

    pub fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) {
        let rt = tm.real_time() as f32;
        let t = tm.now() as f32;
        if self.side_enter_time.is_finite() {
            let p = ((rt - self.side_enter_time.abs()) / ENTER_TRANSIT).min(1.);
            let p = 1. - (1. - p).powi(3);
            let p = if self.side_enter_time < 0. { 1. - p } else { p };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
            let w = WIDTH;
            let rt = f32::tween(&-1., &(w - 1.), p);
            ui.scope(|ui| {
                ui.dx(rt - w);
                ui.dy(-ui.top);
                let h = ui.top * 2.;
                let r = Rect::new(0., 0., w, h).feather(-0.02);
                ui.fill_path(&r.rounded(0.02), ui.background());
                if let Some(id) = self.client.as_ref().and_then(|it| it.blocking_room_id()) {
                    ui.text(mtl!("room-id", "id" => id.to_string()))
                        .pos(r.right() - 0.02, r.y + 0.02)
                        .anchor(1., 0.)
                        .size(0.44)
                        .color(semi_white(0.4))
                        .draw();
                }
                let tr = ui.text(mtl!("multiplayer")).pos(0.05, 0.05).draw();
                let r = Rect::new(r.x, tr.bottom(), r.w, r.bottom() - tr.bottom()).feather(-0.02);
                if self.client.is_none() {
                    let ct = r.center();
                    #[cfg(feature = "local-mp")]
                    {
                        // 连接 / 本地联机 / 服务器列表，三个作为整体垂直居中。
                        let (w, h, gap) = (0.28f32, 0.12f32, 0.03f32);
                        let total = 3. * h + 2. * gap;
                        let mut br = Rect::new(ct.x - w / 2., ct.y - total / 2., w, h);
                        self.connect_btn.render_text(ui, br, t, mtl!("connect"), 0.5, true);
                        br.y += h + gap;
                        self.local_btn.render_text(ui, br, t, mtl!("local-mp-host"), 0.5, true);
                        br.y += h + gap;
                        self.server_list_btn.render_text(ui, br, t, mtl!("local-mp-servers"), 0.5, true);
                    }
                    #[cfg(not(feature = "local-mp"))]
                    self.connect_btn
                        .render_text(ui, Rect::new(ct.x, ct.y, 0., 0.).nonuniform_feather(0.14, 0.06), t, mtl!("connect"), 0.5, true);
                } else {
                    self.render_main(tm, ui, r);
                }
            });
            // Phira Pro：服务器列表浮层（覆盖在面板之上；未连接时也能用）。
            #[cfg(feature = "local-mp")]
            {
                let lp = self.server_list_p.now(t);
                if lp > 1e-4 {
                    // 磨砂背景：主界面背景图的低分辨率版本铺满全屏，把背后界面盖住，
                    // 免得列表内容和主界面糊在一起。每帧取一次（切背景后自动跟着变）。
                    let blur_bg = crate::scene::TEX_BACKGROUND_BLUR.with(|it| it.borrow().clone());
                    ui.abs_scope(|ui| {
                        ui.alpha(lp, |ui| {
                            if let Some(bg) = &blur_bg {
                                ui.fill_rect(ui.screen_rect(), (**bg, ui.screen_rect()));
                            }
                            ui.fill_rect(ui.screen_rect(), semi_black(lp * 0.55));

                            let top = ui.top;
                            ui.text(mtl!("local-mp-servers"))
                                .pos(0., -top + 0.06)
                                .anchor(0.5, 0.)
                                .size(0.6)
                                .color(WHITE)
                                .draw();

                            // 两列布局：逐行左右排布。
                            let col_gap = 0.04;
                            let row_h = 0.12;
                            let row_gap = 0.02;
                            let w_total = 1.9;
                            let w = (w_total - col_gap) / 2.;
                            let n = self.servers.len();
                            let rows = n.div_ceil(2);
                            let viewport = (top * 2. - 0.5).max(row_h);
                            ui.dx(-w_total / 2.);
                            ui.dy(-top + 0.24);
                            self.server_list_scroll.size((w_total, viewport));
                            self.server_list_scroll.render(ui, |ui| {
                                for (i, srv) in self.servers.iter().enumerate() {
                                    let x = (i % 2) as f32 * (w + col_gap);
                                    let y = (i / 2) as f32 * (row_h + row_gap);
                                    let r = Rect::new(x, y, w, row_h);
                                    let ping = match srv.ping {
                                        Some(p) => format!("  {p}ms"),
                                        None => String::new(),
                                    };
                                    let label = match srv.up {
                                        Some(false) => format!(
                                            "{}  {}{}（{}）",
                                            srv.name,
                                            srv.addr,
                                            ping,
                                            mtl!("local-mp-servers-offline")
                                        ),
                                        _ => format!("{}  {}{ping}", srv.name, srv.addr),
                                    };
                                    let color = match srv.up {
                                        Some(true) => SERVER_ONLINE,
                                        Some(false) => SERVER_OFFLINE,
                                        None => SERVER_UNKNOWN,
                                    };
                                    if let Some(btn) = self.server_btns.get_mut(i) {
                                        btn.render_text_color(ui, r, t, label, 0.45, false, color);
                                    }
                                }
                                (
                                    w_total,
                                    (rows as f32 * (row_h + row_gap) - row_gap).max(0.),
                                )
                            });
                        });
                    });
                }
            }
        }
        if let Some(dl) = &mut self.downloading {
            dl.render(ui, t);
        }
        if self.has_task() {
            ui.full_loading_simple(t);
        }
    }

    fn render_main(&mut self, tm: &mut TimeManager, ui: &mut Ui, r: Rect) {
        let t = tm.now() as f32;
        let client = self.client.as_ref().unwrap();
        let mut mr = Rect::new(r.x, r.y, r.w * 0.8, r.h - if CHAT_ENABLED { 0.11 } else { 0. });
        ui.fill_path(&mr.rounded(0.01), semi_black(0.4));
        // Phira Pro：本地联机时把局域网地址钉在信息面板第一行（提示框消失太快、且显示不全）。
        #[cfg(feature = "local-mp")]
        if let Some(addr) = &self.local_addr {
            ui.text(addr)
                .pos(mr.x + 0.03, mr.y + 0.025)
                .size(0.42)
                .color(semi_white(0.85))
                .max_width(mr.w - 0.06)
                .draw();
            mr.y += 0.05;
            mr.h = (mr.h - 0.05).max(0.1);
        }
        ui.scope(|ui| {
            let mut mr = mr.feather(-0.015);
            mr.y -= 0.015;
            mr.h += 0.015;
            ui.dx(mr.x);
            ui.dy(mr.y);
            let mut y = if self.msgs_dirty_from == 0 {
                0.
            } else {
                self.msgs.get(self.msgs_dirty_from - 1).map_or(0., |it| it.bottom)
            };
            let old_dirty = self.msgs_dirty_from != self.msgs.len();
            for msg in &mut self.msgs[self.msgs_dirty_from..] {
                msg.y = y + 0.02;
                msg.bottom = msg.text(ui, mr.w).measure().bottom();
                y = msg.bottom;
            }
            if old_dirty {
                let o = y - mr.h;
                if o >= 0. {
                    self.msg_scroll.y_scroller.goto = Some(o);
                }
            }
            self.msgs_dirty_from = self.msgs.len();
            self.msg_scroll.size((mr.w, mr.h));
            let offset = self.msg_scroll.y_scroller.offset;
            self.msg_scroll.render(ui, |ui| {
                for msg in &self.msgs {
                    if msg.bottom < offset {
                        continue;
                    }
                    if msg.y > offset + mr.h {
                        break;
                    }
                    msg.text(ui, mr.w).draw();
                }
                (mr.w, self.msgs.last().map(|it| it.bottom).unwrap_or_default() + 0.03)
            });
        });

        if CHAT_ENABLED {
            let lw = 0.16;
            let h = 0.09;
            let br = Rect::new(r.x, r.bottom() - h, mr.w - lw - 0.02, h);
            self.chat_btn.render_input(ui, br, t, &self.chat_text, mtl!("chat-placeholder"), 0.5);
            let br = Rect::new(mr.right() - lw, br.y, lw, br.h);
            self.chat_send_btn.render_text(ui, br, t, mtl!("chat-send"), 0.5, true);
        }

        let mut br = Rect::new(mr.right() + 0.02, mr.y, r.right() - mr.right() - 0.02, 0.1);
        let mut btns = SmallVec::<[(&mut DRectButton, String); 5]>::new();
        if let Some(state) = client.blocking_state() {
            match state.state {
                RoomState::SelectChart(_) => {
                    if client.blocking_is_host().unwrap() {
                        btns.push((&mut self.request_start_btn, mtl!("request-start").into_owned()));
                        btns.push((&mut self.lock_room_btn, mtl!("lock-room", "current" => state.locked.to_string())));
                        btns.push((&mut self.cycle_room_btn, mtl!("cycle-room", "current" => state.cycle.to_string())));
                    }
                    btns.push((&mut self.leave_room_btn, mtl!("leave-room").into_owned()));
                }
                RoomState::WaitingForReady => {
                    if client.blocking_is_ready().unwrap() {
                        btns.push((&mut self.cancel_ready_btn, mtl!("cancel-ready").into_owned()));
                    } else {
                        btns.push((&mut self.ready_btn, mtl!("ready").into_owned()));
                    }
                }
                _ => {}
            }
            btns.push((&mut self.user_list_btn, mtl!("user-list").into_owned()));
        } else {
            btns.push((&mut self.create_room_btn, mtl!("create-room").into_owned()));
            btns.push((&mut self.join_room_btn, mtl!("join-room").into_owned()));
            btns.push((&mut self.disconnect_btn, mtl!("disconnect").into_owned()));
            #[cfg(feature = "local-mp")]
            {
                btns.push((&mut self.room_list_btn, mtl!("local-mp-rooms").into_owned()));
                let label = if self.only_public {
                    mtl!("local-mp-only-public").into_owned()
                } else {
                    mtl!("local-mp-all").into_owned()
                };
                btns.push((&mut self.only_public_btn, label));
            }
        }
        for (btn, text) in btns {
            btn.render_text(ui, br, t, text, 0.5, true);
            br.y += br.h + 0.02;
        }

        let p = self.user_list_p.now(t);
        if p > 1e-4 {
            ui.abs_scope(|ui| {
                ui.alpha(p, |ui| {
                    // 黑名单：房间玩家列表里也不显示名单内的玩家。
                    let users: Vec<_> = client
                        .blocking_state()
                        .unwrap()
                        .users
                        .values()
                        .filter(|u| !crate::blacklist::contains(u.id))
                        .cloned()
                        .collect();
                    let n = users.len();
                    let columns = n.clamp(2, 4);
                    let rn = n.div_ceil(columns);
                    ui.fill_rect(ui.screen_rect(), semi_black(p * 0.4));

                    let mut iter = users.into_iter();
                    let h = 0.14;
                    let w = 0.48;
                    let pad = 0.03;
                    let width = w * columns as f32 + pad * (columns - 1) as f32;
                    let viewport_height = (ui.top * 2. - 0.16).max(h);
                    ui.dx(-width / 2.);
                    ui.dy(-ui.top + 0.08);
                    self.user_list_scroll.size((width, viewport_height));
                    self.user_list_scroll.render(ui, |ui| {
                        for i in 0..rn {
                            let cn = (n - i * columns).min(columns);
                            let row_width = w * cn as f32 + pad * (cn - 1) as f32;
                            let row_offset = (width - row_width) / 2.;
                            for j in 0..cn {
                                let r = Rect::new(row_offset + j as f32 * (w + pad), i as f32 * (h + pad), w, h);
                                let Some(user) = iter.next() else { unreachable!() };
                                // 本地联机的玩家 id 是负值、在官方服务器上查不到，直接给默认头像；
                                // 否则会永远转圈，整个列表看起来就只剩名字（像纯文字）。
                                let avatar = if user.id < 0 {
                                    Err(self.icon_user.clone())
                                } else {
                                    UserManager::opt_avatar(user.id, &self.icon_user)
                                };
                                ui.avatar(r.x + 0.055, r.center().y, 0.04, t, avatar);
                                ui.text(user.name)
                                    .pos(r.x + 0.105, r.center().y)
                                    .anchor(0., 0.5)
                                    .no_baseline()
                                    .max_width(0.36)
                                    .size(0.55)
                                    .draw();
                            }
                        }
                        (width, (rn as f32 * (h + pad) - pad).max(0.))
                    });
                });
            });
        }
    }

    #[inline]
    pub fn next_scene(&mut self) -> Option<NextScene> {
        self.next_scene.take()
    }
}
