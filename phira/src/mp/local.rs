//! Phira Pro：本地联机（把 phira-mp 服务端嵌进客户端）。
//!
//! 做法：直接用 `rphira-mp` 的 lib 目标（`phira-mp-next` 的协议/房间/会话实现，
//! 与官方协议逐字节兼容）在客户端进程里起一个服务端——房主开端口，同一局域网内
//! 的设备（包括官方 Phira 客户端）都能连进来，不需要官方服务器、也不需要登录。
//!
//! 免登录的落点：注册全局 `auth_provider`。
//! - 形如 `local:<昵称>` 的 token：本地直接合成用户，id 取昵称哈希的**负值**
//!   （官方 id 都是正数，不会撞）；
//! - 其它 token：仍然走官方 `/me` 校验 —— 登录了的玩家进本地房间时保持真实身份。
//!
//! 成绩 provider 也做了兜底：本地联机时对方可能没有上传成绩（record 不存在），
//! 这时给一条空成绩，保证对局能正常走完、不卡在结算。

use anyhow::{anyhow, Context, Result};
use phira_mp::phira::{GameRecord, PhiraFetcher, PhiraFetcherConfig, UserInfo};
use phira_mp::player::{set_auth_provider, set_record_provider};
use phira_mp::server::{run, ServerArgs};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;

/// 游戏端口（与官方服务端默认一致）。
pub const DEFAULT_PORT: u16 = 12346;
/// HTTP 查询 API 端口（`GET /api/rooms`，房间列表就走它）。
pub const DEFAULT_HTTP_PORT: u16 = 12347;
/// 免登录 token 前缀。
pub const LOCAL_TOKEN_PREFIX: &str = "local:";

static RUNNING: AtomicBool = AtomicBool::new(false);
static PORT: AtomicU16 = AtomicU16::new(0);

pub fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

pub fn port() -> u16 {
    PORT.load(Ordering::Relaxed)
}

/// 组装免登录 token。
pub fn local_token(name: &str) -> String {
    format!("{LOCAL_TOKEN_PREFIX}{name}")
}

/// 昵称 → 稳定的本地 id（负值，避免与官方 id 冲突）。
fn local_id(name: &str) -> i32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    -(((h >> 1) & 0x7fff_ffff) as i32 + 1)
}

/// 取本机在局域网里的 IPv4 地址。
/// 只让内核选一次出口路由，不真的发包。
pub fn local_ip() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

/// 起内嵌服务端（幂等）。返回实际使用的端口。
pub fn start(port: u16, http_port: u16) -> Result<u16> {
    if RUNNING.load(Ordering::Relaxed) {
        return Ok(PORT.load(Ordering::Relaxed));
    }

    let args = ServerArgs {
        config: String::new(),
        port,
        host: "0.0.0.0".to_owned(),
        http_port,
        proxy_protocol: false,
        language: "zh-CN".to_owned(),
        session_timeout: 300,
        phira_api: "https://phira.5wyxi.com/".to_owned(),
        record_dir: None,
        phira_max_attempts: 5,
        phira_retry_base_ms: 150,
        phira_token_cache_ttl: 600,
        phira_token_cache_cap: 10000,
        phira_user_cache_ttl: 600,
        phira_user_cache_cap: 5000,
        phira_chart_cache_ttl: 1800,
        phira_chart_cache_cap: 10000,
        phira_record_cache_ttl: 1800,
        phira_record_cache_cap: 50000,
        handshake_timeout: 5,
        read_timeout: 5,
        proxy_timeout: 5,
        default_max_player: 8,
    };

    // 官方数据源：给「已登录玩家」和谱面/成绩查询用。
    let fetcher = Arc::new(PhiraFetcher::new(PhiraFetcherConfig::from(&args)));

    // 认证：本地 token 直接合成；真实 token 走官方 /me。
    {
        let fetcher: Arc<PhiraFetcher> = Arc::clone(&fetcher);
        set_auth_provider(Arc::new(move |token: String| {
            let fetcher: Arc<PhiraFetcher> = Arc::clone(&fetcher);
            Box::pin(async move {
                if let Some(name) = token.strip_prefix(LOCAL_TOKEN_PREFIX) {
                    let name = name.trim();
                    let name = if name.is_empty() { "Player" } else { name };
                    return Ok(Arc::new(UserInfo {
                        id: local_id(name),
                        name: name.to_owned(),
                        ..Default::default()
                    }));
                }
                match fetcher.get_user_info(&token).await {
                    Ok(info) => Ok(info),
                    // 回源校验失败（token 过期 / 官方返回异常 / 网络不通）时退化成游客，
                    // 否则局域网里根本连不进来（客户端只会看到「鉴权失败」）。
                    Err(err) => {
                        eprintln!("[local-mp] 官方校验失败，按游客接入：{err}");
                        Ok(Arc::new(UserInfo {
                            id: local_id(&token),
                            name: format!("Player{:04}", local_id(&token).unsigned_abs() % 10000),
                            ..Default::default()
                        }))
                    }
                }
            })
        }));
    }

    // 成绩：官方查不到（本地联机时对方没上传）就给一条空成绩，别卡住结算。
    {
        let fetcher: Arc<PhiraFetcher> = Arc::clone(&fetcher);
        set_record_provider(Arc::new(move |id: i32| {
            let fetcher: Arc<PhiraFetcher> = Arc::clone(&fetcher);
            Box::pin(async move {
                match fetcher.get_record_info(id).await {
                    Ok(r) => Ok(r),
                    Err(_) => Ok(Arc::new(GameRecord {
                        id,
                        ..Default::default()
                    })),
                }
            })
        }));
    }

    RUNNING.store(true, Ordering::Relaxed);
    PORT.store(port, Ordering::Relaxed);
    tokio::spawn(async move {
        match run(args).await {
            Ok(()) => eprintln!("[local-mp] 服务端已停止"),
            Err(err) => eprintln!("[local-mp] 服务端退出：{err}"),
        }
        RUNNING.store(false, Ordering::Relaxed);
    });

    Ok(port)
}

/// 关掉内嵌服务端（断开连接时调用）。
/// 立刻把运行标记复位，这样 UI 不会因为异步停止还没完成而误报「已在运行」。
pub fn stop() {
    if let Some(ctx) = phira_mp::server::global_ctx() {
        ctx.request_shutdown();
    }
    RUNNING.store(false, Ordering::Relaxed);
}

/// 供 UI 显示：`192.168.x.x:12346`。
pub fn display_addr(port: u16) -> String {
    match local_ip() {
        Some(ip) => format!("{ip}:{port}"),
        None => format!("0.0.0.0:{port}"),
    }
}

/// 房间列表（走内嵌服务端的 HTTP 查询 API）。
pub async fn fetch_rooms(host: &str, http_port: u16) -> Result<String> {
    let url = format!("http://{host}:{http_port}/api/rooms");
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|err| anyhow!("{url}：{err}"))?;
    let text = resp.text().await.context("读取响应失败")?;
    // 只有跑 phira-mp-next 的服务端才有这个查询接口；官方服务器会返回 404/HTML。
    if !text.trim_start().starts_with('{') {
        let head: String = text.chars().take(80).collect();
        return Err(anyhow!("{url} 未返回 JSON：{head}"));
    }
    Ok(text)
}

/// Phira Pro：服务器列表里的一个联机服务器。
#[derive(Clone, Debug)]
pub struct ServerEntry {
    /// 友好名称（状态站里的标签，如「Grand Tourer」）。
    pub name: String,
    /// 连接地址（`主机:端口`）。
    pub addr: String,
    /// 在线状态：`Some(true)` 在线、`Some(false)` 离线、`None` 未知（状态站没给心跳）。
    pub up: Option<bool>,
    /// 状态站最近一次探测到的延迟（毫秒）。
    pub ping: Option<i64>,
}

/// 排序权重：在线 → 未知 → 离线。
fn status_rank(up: Option<bool>) -> u8 {
    match up {
        Some(true) => 0,
        None => 1,
        Some(false) => 2,
    }
}

async fn get_json(url: &str) -> Result<String> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("Accept", "application/json")
        // 状态站对无 UA 的请求会返回 SPA 页面，这里带一个浏览器 UA。
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|err| anyhow!("{url}：{err}"))?;
    resp.text().await.context("读取响应失败")
}

/// 心跳接口地址：Uptime-Kuma 状态页从 `/api/status-page/{slug}` 派生
/// `/api/status-page/heartbeat/{slug}`。
fn heartbeat_url(url: &str) -> Option<String> {
    let out = url.replace("/status-page/", "/status-page/heartbeat/");
    (out != url).then_some(out)
}

/// Phira Pro：抓取服务器列表。
///
/// 兼容两种 Uptime-Kuma 数据：
/// - 状态页：`{ "publicGroupList": [{ "monitorList": [...] }] }`，心跳在同源的
///   `/api/status-page/heartbeat/{slug}` 里；
/// - 旧接口：`{ "groups": [{ "monitors": [...] }], "heartbeatList": {...} }`。
pub async fn fetch_server_list(url: &str) -> Result<Vec<ServerEntry>> {
    let text = get_json(url).await?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        let head: String = text.chars().take(80).collect();
        anyhow!("{url} 未返回可解析的数据：{head}")
    })?;

    // 心跳：内联优先，否则从状态页派生接口再取一次。
    let fetched_hb;
    let heartbeat = if v.get("heartbeatList").is_some() {
        None
    } else if let Some(hb_url) = heartbeat_url(url) {
        fetched_hb = get_json(&hb_url).await.ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        fetched_hb.as_ref()
    } else {
        None
    };
    let heartbeat = heartbeat
        .and_then(|it| it.get("heartbeatList"))
        .or_else(|| v.get("heartbeatList"))
        .and_then(|it| it.as_object());

    let monitors = collect_monitors(&v);
    if monitors.is_empty() {
        return Err(anyhow!("{url} 里没有找到服务器列表"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ServerEntry> = Vec::new();
    for m in monitors {
        if let Some(e) = server_entry(m, heartbeat) {
            if seen.insert(e.addr.clone()) {
                out.push(e);
            }
        }
    }
    out.sort_by(|a, b| {
        status_rank(a.up)
            .cmp(&status_rank(b.up))
            .then_with(|| a.ping.unwrap_or(i64::MAX).cmp(&b.ping.unwrap_or(i64::MAX)))
            .then_with(|| a.addr.cmp(&b.addr))
    });
    Ok(out)
}

/// 从两种格式里收集 monitor 节点（它们的单个节点结构是一样的）。
fn collect_monitors(v: &serde_json::Value) -> Vec<&serde_json::Value> {
    let mut out = Vec::new();
    if let Some(groups) = v.get("publicGroupList").and_then(|it| it.as_array()) {
        for g in groups {
            if let Some(ms) = g.get("monitorList").and_then(|it| it.as_array()) {
                out.extend(ms.iter());
            }
        }
    } else if let Some(groups) = v.get("groups").and_then(|it| it.as_array()) {
        for g in groups {
            if let Some(ms) = g.get("monitors").and_then(|it| it.as_array()) {
                out.extend(ms.iter());
            }
        }
    }
    out
}

/// `主机:端口` 形式（端口 2~5 位数字）。
fn is_host_port(s: &str) -> bool {
    match s.rsplit_once(':') {
        Some((h, p)) => !h.is_empty() && !h.contains(char::is_whitespace) && (2..=5).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

fn server_entry(m: &serde_json::Value, heartbeat: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<ServerEntry> {
    let id = m.get("id").and_then(|it| it.as_i64());
    let addr = m.get("name").and_then(|it| it.as_str())?.trim();
    // 只有「主机:端口」形式才是联机服务器，其余（下载站 / 官网 / 登录…）跳过。
    if !is_host_port(addr) {
        return None;
    }
    // 友好名称存在 tags 里：名字那一条的颜色固定是 #2563EB（其它是功能标签 / 国旗）。
    let name = m
        .get("tags")
        .and_then(|it| it.as_array())
        .and_then(|tags| {
            tags.iter()
                .find(|t| t.get("color").and_then(|c| c.as_str()).is_some_and(|c| c.eq_ignore_ascii_case("#2563eb")))
                .or_else(|| tags.first())
        })
        .and_then(|t| t.get("name"))
        .and_then(|it| it.as_str())
        .filter(|it| !it.is_empty())
        .unwrap_or(addr)
        .to_owned();
    let last = id
        .and_then(|id| heartbeat.and_then(|h| h.get(&id.to_string())))
        .and_then(|arr| arr.as_array())
        .and_then(|a| a.last());
    let up = last
        .and_then(|e| e.get("status"))
        .and_then(|s| s.as_i64())
        .map(|s| s == 1);
    let ping = last.and_then(|e| e.get("ping")).and_then(|p| p.as_i64());
    Some(ServerEntry {
        name,
        addr: addr.to_owned(),
        up,
        ping,
    })
}
