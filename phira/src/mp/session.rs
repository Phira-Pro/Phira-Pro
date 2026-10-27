//! One actor owns socket writes, response matching and snapshot installation.
use super::{
    protocol::{self, Event, Request},
    live::{LiveBuffer, LiveEvent, LiveScope},
    room::{Launch, Snapshot},
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

const RPC_TIMEOUT: Duration = Duration::from_secs(7);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) enum Action {
    Rpc(Request, oneshot::Sender<Result<()>>, u64),
    Live(Vec<u8>, Launch),
    Close,
}
struct Inner {
    state: Arc<Mutex<Snapshot>>,
    live: Arc<Mutex<LiveBuffer>>,
    actions: mpsc::Sender<Action>,
    actor: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(h) = self.actor.get_mut().unwrap().take() {
            h.abort();
        }
    }
}
#[derive(Clone)]
pub struct Session(Arc<Inner>);
pub fn address(input: &str) -> Result<String> {
    let v = input.trim();
    ensure!(
        !v.is_empty() && v.len() <= 253 && !v.contains(['/', '\\', '@', '?', '#']) && !v.chars().any(char::is_whitespace),
        "请输入 host:port 或 [IPv6]:port"
    );
    let (host, port) = v.rsplit_once(':').context("联机地址需要指定端口")?;
    ensure!(!host.is_empty() && port.parse::<u16>().is_ok_and(|p| p != 0), "联机地址端口无效");
    if host.contains(':') {
        ensure!(
            host.starts_with('[') && host.ends_with(']') && host[1..host.len() - 1].parse::<std::net::Ipv6Addr>().is_ok(),
            "IPv6 地址需要使用方括号"
        );
    } else {
        ensure!(host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'), "服务器名称无效");
    }
    Ok(v.to_owned())
}
impl Session {
    pub async fn connect(addr: &str, token: String) -> Result<Self> {
        let addr = address(addr)?;
        Request::Authenticate(token.clone()).encode()?;
        let mut stream = tokio::time::timeout(Duration::from_secs(8), TcpStream::connect(&addr))
            .await
            .context("连接联机服务器超时")??;
        stream.set_nodelay(true)?;
        stream.write_u8(1).await?;
        let (read, write) = stream.into_split();
        let (tx, rx) = mpsc::channel(32);
        let state = Arc::new(Mutex::new(Snapshot {
            connected: true,
            ..Default::default()
        }));
        let live = Arc::new(Mutex::new(LiveBuffer::default()));
        // The actor shares only state, never the Session itself (no Arc cycle).
        let actor_state = Arc::clone(&state);
        let actor_live = Arc::clone(&live);
        let recovery_token = token.clone();
        let actor = tokio::spawn(async move {
            super::recovery::supervise(read, write, rx, actor_state, actor_live, addr, recovery_token).await;
        });
        let this = Self(Arc::new(Inner {
            state,
            live,
            actions: tx,
            actor: Mutex::new(Some(actor)),
        }));
        if let Err(e) = this.request(Request::Authenticate(token)).await {
            this.close();
            return Err(e);
        }
        Ok(this)
    }
    fn state(&self) -> Arc<Mutex<Snapshot>> {
        Arc::clone(&self.0.state)
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state().lock().unwrap().clone()
    }
    /// Drain inbound touches and judgement batches for monitor consumers.
    pub fn take_live_events(&self, scope: LiveScope) -> Vec<LiveEvent> {
        self.0.live.lock().unwrap().drain(scope)
    }
    pub fn dropped_live_batches(&self) -> u64 {
        self.0.live.lock().unwrap().dropped_batches()
    }
    pub async fn request(&self, req: Request) -> Result<()> {
        req.encode()?;
        let generation = {
            let s = self.0.state.lock().unwrap();
            ensure!(s.connected && !s.reconnecting, "联机连接正在恢复或已关闭，请等待后重新操作");
            s.room_generation
        };
        let (tx, rx) = oneshot::channel();
        self.0.actions.send(Action::Rpc(req, tx, generation)).await.context("联机会话已关闭")?;
        rx.await.context("联机任务已结束")?
    }
    pub fn close(&self) {
        if self.0.actions.try_send(Action::Close).is_err() {
            // A full live queue must never prevent an explicit disconnect.
            if let Some(h) = self.0.actor.lock().unwrap().take() {
                h.abort();
            }
            let mut s = self.0.state.lock().unwrap();
            s.disconnected();
            s.reconnecting = false;
        }
    }
    pub fn live(&self, launch: Launch, bytes: Vec<u8>) -> bool {
        let s = self.0.state.lock().unwrap();
        if !s.accepts_live(launch) {
            return false;
        }
        self.0.actions.try_send(Action::Live(bytes, launch)).is_ok()
    }
    pub fn live_context_valid(&self, launch: Launch) -> bool {
        self.0.state.lock().unwrap().accepts_live(launch)
    }
    pub fn mark_prepared(&self, g: u64, chart: i32) -> Result<()> {
        let state = self.state();
        let mut s = state.lock().unwrap();
        ensure!(s.connected && s.authenticated && !s.reconnecting, "联机连接已失效，请重新检查资源");
        ensure!(s.room.as_ref().is_some_and(|r| r.phase != protocol::Phase::Playing), "本局已经开始，请等待下一局");
        s.mark_prepared(g, chart)?;
        s.check_launch();
        Ok(())
    }
    pub fn take_launch(&self) -> Option<Launch> {
        self.state().lock().unwrap().start.take()
    }
    pub fn finish(&self, launch: Launch) -> Result<()> {
        let state = self.state();
        let mut s = state.lock().unwrap();
        ensure!(s.room_generation == launch.room_generation && s.round == launch.round && s.active, "本局联机上下文已失效");
        s.active = false;
        s.finished = true;
        s.start = None;
        s.log("本局成绩由 Pro 保存；联机服务端按放弃结束");
        Ok(())
    }
}
struct Pending {
    req: Request,
    tx: oneshot::Sender<Result<()>>,
    deadline: Instant,
    generation: u64,
}
pub(super) async fn run(
    mut read: tokio::net::tcp::OwnedReadHalf,
    mut write: tokio::net::tcp::OwnedWriteHalf,
    rx: &mut mpsc::Receiver<Action>,
    state: Arc<Mutex<Snapshot>>,
    live: Arc<Mutex<LiveBuffer>>,
) -> bool {
    let (itx, mut incoming) = mpsc::channel(128);
    let reader = tokio::spawn(async move {
        loop {
            let result = protocol::read_frame(&mut read).await;
            let failed = result.is_err();
            if itx.send(result).await.is_err() || failed {
                break;
            }
        }
    });
    struct ReaderGuard(JoinHandle<()>);
    impl Drop for ReaderGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _reader = ReaderGuard(reader);
    let mut pending: Option<Pending> = None;
    let mut buffered = VecDeque::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut last_ping = Instant::now();
    let mut waiting_pong: Option<Instant> = None;
    let result:Result<()>=async {loop {tokio::select! {
        action=rx.recv()=>match action {
            None|Some(Action::Close)=>break,
            Some(Action::Live(bytes,launch))=>{
                let valid=state.lock().unwrap().accepts_live(launch);
                if valid {tokio::time::timeout(WRITE_TIMEOUT,protocol::write_frame(&mut write,&bytes)).await.context("实时流写入超时")??;}
            }
            Some(Action::Rpc(req,tx,epoch))=>{
                if state.lock().unwrap().room_generation != epoch {let _=tx.send(Err(anyhow!("连接或房间已改变，请重新操作")));continue;}
                if pending.is_some() {let _=tx.send(Err(anyhow!("另一个联机操作尚未结束")));continue;}
                if let Err(e)=state.lock().unwrap().validate(&req) {let _=tx.send(Err(e));continue;}
                let generation=state.lock().unwrap().chart_generation;
                let bytes=match req.encode(){Ok(b)=>b,Err(e)=>{let _=tx.send(Err(e));continue;}};
                // Register before writing. Room changes are applied by this same actor before returning.
                pending=Some(Pending{req,tx,deadline:Instant::now()+RPC_TIMEOUT,generation});state.lock().unwrap().busy=true;
                tokio::time::timeout(WRITE_TIMEOUT,protocol::write_frame(&mut write,&bytes)).await.context("联机写入超时")??;
            }
        },
        packet=incoming.recv()=>{
            let packet=packet.context("联机接收任务已结束")??;let (event,_tail)=protocol::decode(&packet)?;
            match event {
                Event::Pong=>{
                    if let Some(sent) = waiting_pong.take() {
                        state.lock().unwrap().rtt_ms = Some(sent.elapsed().as_secs_f32() * 1000.);
                    }
                },
                Event::Reply(tag,result)=>{
                    let Some(p)=pending.take() else {bail!("收到没有对应请求的联机响应 {tag}");};
                    ensure!(p.req.response_tag()==tag,"联机响应与未决请求不匹配");
                    let mut s=state.lock().unwrap();s.busy=false;
                    let lifecycle=matches!(p.req,Request::Authenticate(_)|Request::Create(_)|Request::Join(_)|Request::JoinMonitor(_));
                    let result=result.map_err(anyhow::Error::msg).and_then(|r|s.reply(&p.req,r,p.generation));
                    if lifecycle {if result.is_ok(){while let Some(e)=buffered.pop_front(){s.apply(e);}}else{buffered.clear();}}
                    let _=p.tx.send(result);
                },
                Event::Live(batch)=>{
                    let s = state.lock().unwrap();
                    if let Some(scope) = s.monitor_scope() {
                        live.lock().unwrap().push(scope, batch);
                    }
                },
                event=>{
                    let installing=pending.as_ref().is_some_and(|p|matches!(p.req,Request::Authenticate(_)|Request::Create(_)|Request::Join(_)|Request::JoinMonitor(_)));
                    if installing {ensure!(buffered.len()<256,"入房期间状态事件过多");buffered.push_back(event);}
                    else {state.lock().unwrap().apply(event);}
                }
            }
            let scope = state.lock().unwrap().monitor_scope();
            live.lock().unwrap().set_scope(scope);
        },
        _=tick.tick()=>{
            ensure!(!pending.as_ref().is_some_and(|p|Instant::now()>=p.deadline),"联机请求超时：已隔离连接，禁止重试旧操作");
            ensure!(!waiting_pong.is_some_and(|t|t.elapsed()>Duration::from_secs(2)),"联机心跳超时");
            if state.lock().unwrap().authenticated && waiting_pong.is_none() && last_ping.elapsed()>=Duration::from_secs(3) {
                tokio::time::timeout(WRITE_TIMEOUT,protocol::write_frame(&mut write,&[0])).await.context("心跳写入超时")??;
                last_ping=Instant::now();waiting_pong=Some(Instant::now());
            }
        }
    }} Ok(())}.await;
    let message = result.err().map(|e| format!("{e:#}"));
    if let Some(p) = pending {
        let _ = p.tx.send(Err(anyhow!(message.clone().unwrap_or_else(|| "联机会话已关闭".into()))));
    }
    let mut s = state.lock().unwrap();
    let recover = message.is_some() && s.authenticated;
    s.disconnected();
    live.lock().unwrap().set_scope(None);
    if let Some(error) = message {
        s.log(error.clone());
        s.error = Some(error);
    }
    recover
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{read_frame, write_frame, Phase};
    use super::*;
    use tokio::{io::AsyncReadExt, net::TcpListener};
    async fn login(listener: &TcpListener) -> TcpStream {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 1);
        assert_eq!(read_frame(&mut socket).await.unwrap(), [1, 4, b't', b'e', b's', b't']);
        write_frame(&mut socket, &[1, 1, 1, 0, 0, 0, 4, b't', b'e', b's', b't', 0, 0])
            .await
            .unwrap();
        socket
    }
    async fn until(s: &Session, condition: impl Fn(&Snapshot) -> bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !condition(&s.snapshot()) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn address_validation() {
        for v in ["127.0.0.1:1234", "example.com:1234", "[::1]:1234"] {
            assert!(address(v).is_ok());
        }
        for v in ["https://x:12", "::1:12", "x:0", "x", "x:65536", "x:12/path"] {
            assert!(address(v).is_err());
        }
    }
    #[tokio::test]
    async fn real_tcp_installs_room_before_immediate_pushes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut socket = login(&listener).await;
            assert_eq!(read_frame(&mut socket).await.unwrap(), [5, 1, b'r']);
            // One write: successful create, chart, then waiting before UI resumes.
            socket.write_all(&[2, 8, 1, 7, 6, 0, 1, 7, 0, 0, 0, 2, 6, 1]).await.unwrap();
            assert_eq!(read_frame(&mut socket).await.unwrap(), [7]);
            write_frame(&mut socket, &[11, 1]).await.unwrap();
            while let Ok(b) = read_frame(&mut socket).await {
                if b == [0] {
                    write_frame(&mut socket, &[0]).await.unwrap();
                }
            }
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        s.request(Request::Create("r".into())).await.unwrap();
        until(&s, |s| s.room.as_ref().is_some_and(|r| r.phase == Phase::Waiting)).await;
        assert_eq!(s.snapshot().room.unwrap().chart, Some(7));
        assert!(s.snapshot().start.is_none());
        s.request(Request::Leave).await.unwrap();
        assert!(s.snapshot().room.is_none());
        s.close();
        server.abort();
    }
    #[tokio::test]
    async fn overlapping_rpc_and_duplicate_reply_are_safe_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (seen_tx, seen_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut socket = login(&listener).await;
            read_frame(&mut socket).await.unwrap();
            write_frame(&mut socket, &[8, 1]).await.unwrap();
            assert_eq!(read_frame(&mut socket).await.unwrap(), [2, 1, b'a']);
            let _ = seen_tx.send(());
            release_rx.await.unwrap();
            write_frame(&mut socket, &[2, 1]).await.unwrap();
            write_frame(&mut socket, &[2, 1]).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        s.request(Request::Create("r".into())).await.unwrap();
        let a = s.clone();
        let first = tokio::spawn(async move { a.request(Request::Chat("a".into())).await });
        seen_rx.await.unwrap();
        assert!(s.request(Request::Chat("b".into())).await.is_err());
        release_tx.send(()).unwrap();
        assert!(first.await.unwrap().is_ok());
        until(&s, |s| !s.connected).await;
        assert!(s.snapshot().error.unwrap().contains("没有对应请求"));
        server.abort();
    }
    #[tokio::test]
    async fn rpc_timeout_closes_old_connection_and_never_retries() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut socket = login(&listener).await;
            read_frame(&mut socket).await.unwrap();
            write_frame(&mut socket, &[8, 1]).await.unwrap();
            assert_eq!(read_frame(&mut socket).await.unwrap(), [2, 1, b'a']);
            while let Ok(b) = read_frame(&mut socket).await {
                assert_eq!(b, [0], "timed-out operation was retried");
                write_frame(&mut socket, &[0]).await.unwrap();
            }
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        s.request(Request::Create("r".into())).await.unwrap();
        let error = s.request(Request::Chat("a".into())).await.unwrap_err();
        assert!(error.to_string().contains("超时"));
        until(&s, |s| !s.connected).await;
        assert!(s.request(Request::Chat("b".into())).await.is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn finished_game_uses_abort_once_and_can_start_second_round() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut socket = login(&listener).await;
            read_frame(&mut socket).await.unwrap();
            socket.write_all(&[2, 8, 1, 7, 6, 0, 1, 7, 0, 0, 0]).await.unwrap();
            for _ in 0..2 {
                assert_eq!(read_frame(&mut socket).await.unwrap(), [11]);
                socket.write_all(&[2, 15, 1, 2, 5, 10, 2, 6, 2]).await.unwrap();
                assert_eq!(read_frame(&mut socket).await.unwrap(), [15], "only Abort is used; never Played with a Pro ID");
                socket.write_all(&[2, 19, 1, 2, 5, 12, 7, 6, 0, 1, 7, 0, 0, 0]).await.unwrap();
            }
            while let Ok(b) = read_frame(&mut socket).await {
                if b == [0] {
                    write_frame(&mut socket, &[0]).await.unwrap();
                }
            }
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        s.request(Request::Create("r".into())).await.unwrap();
        for round in 1..=2 {
            until(&s, |s| s.room.as_ref().is_some_and(|r| r.phase == Phase::Selecting && r.chart == Some(7)) && !s.active).await;
            s.mark_prepared(s.snapshot().chart_generation, 7).unwrap();
            s.request(Request::Start).await.unwrap();
            until(&s, |s| s.start.is_some()).await;
            let launch = s.take_launch().unwrap();
            assert_eq!(launch.round, round);
            s.finish(launch).unwrap();
            assert!(s.finish(launch).is_err());
            s.request(Request::Abort).await.unwrap();
        }
        s.close();
        server.abort();
    }
}
