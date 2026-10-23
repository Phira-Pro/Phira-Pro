//! Transport recovery only: no UI, downloads, game restarts or RPC replay.
use super::{
    protocol::{self, Event, Reply, Request},
    room::Snapshot,
    session::{self, Action},
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::mpsc,
};

type Restored = (OwnedReadHalf, OwnedWriteHalf, Reply, Vec<Event>);

async fn authenticate(addr: &str, token: &str) -> Result<Restored> {
    // Cancellation drops the whole socket; a partially read frame is never reused.
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        stream.write_u8(1).await?;
        protocol::write_frame(&mut stream, &Request::Authenticate(token.to_owned()).encode()?).await?;
        let mut buffered = Vec::new();
        loop {
            let bytes = protocol::read_frame(&mut stream).await?;
            let (event, _) = protocol::decode(&bytes)?;
            match event {
                Event::Reply(1, result) => {
                    let reply = result.map_err(anyhow::Error::msg)?;
                    ensure!(matches!(&reply, Reply::Auth(..)), "恢复鉴权响应类型无效");
                    let (read, write) = stream.into_split();
                    return Ok((read, write, reply, buffered));
                }
                Event::Reply(..) => bail!("恢复时收到不匹配的响应"),
                Event::Pong => {}
                event => {
                    ensure!(buffered.len() < 256, "恢复期间状态事件过多");
                    buffered.push(event);
                }
            }
        }
    })
    .await
    .context("恢复连接或鉴权超时")?
}

/// Wait for a delay/handshake while rejecting every old operation and live packet.
async fn cancellable<T>(future: impl std::future::Future<Output = T>, rx: &mut mpsc::Receiver<Action>) -> Option<T> {
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            action = rx.recv() => match action {
                None | Some(Action::Close) => return None,
                Some(Action::Rpc(_, tx, _)) => { let _ = tx.send(Err(anyhow!("连接正在恢复，旧操作已取消；恢复后请重新操作"))); }
                Some(Action::Live(..)) => {}
            },
            result = &mut future => return Some(result),
        }
    }
}

pub(super) async fn supervise(
    mut read: OwnedReadHalf,
    mut write: OwnedWriteHalf,
    mut rx: mpsc::Receiver<Action>,
    state: Arc<Mutex<Snapshot>>,
    addr: String,
    token: String,
) {
    'connection: loop {
        if !session::run(read, write, &mut rx, Arc::clone(&state)).await {
            break;
        }
        state.lock().unwrap().reconnecting = true;
        // Bounded retries, then expose a deliberate reconnect action to the user.
        for attempt in 1..=3 {
            {
                let mut s = state.lock().unwrap();
                s.reconnect_attempt = attempt;
                s.log(format!("连接中断，正在尝试恢复（{attempt}/3）"));
            }
            if cancellable(tokio::time::sleep(Duration::from_secs(1 << (attempt - 1))), &mut rx)
                .await
                .is_none()
            {
                break 'connection;
            }
            let Some(result) = cancellable(authenticate(&addr, &token), &mut rx).await else {
                break 'connection;
            };
            match result {
                Ok((next_read, next_write, reply, buffered)) => {
                    let mut s = state.lock().unwrap();
                    // Install before publishing connected=true. All Session clones see this same snapshot.
                    if let Err(e) = s.reply(&Request::Authenticate(token.clone()), reply, 0) {
                        s.error = Some(format!("{e:#}"));
                        continue;
                    }
                    s.connected = true;
                    s.reconnecting = false;
                    s.reconnect_attempt = 0;
                    s.error = None;
                    for event in buffered {
                        s.apply(event);
                    }
                    s.log("连接已恢复，已重新核对房间；旧操作不会自动重试");
                    if s.room.as_ref().is_some_and(|r| r.phase == protocol::Phase::Playing) {
                        s.log("房间正在游玩，请等待下一局；本地旧局仍可完成并保存");
                    }
                    drop(s);
                    read = next_read;
                    write = next_write;
                    continue 'connection;
                }
                Err(e) => {
                    let mut s = state.lock().unwrap();
                    let message = format!("恢复失败（{attempt}/3）：{e:#}");
                    s.log(message.clone());
                    s.error = Some(message);
                }
            }
        }
        state.lock().unwrap().log("自动恢复已停止，请手动连接");
        break;
    }
    let mut s = state.lock().unwrap();
    s.reconnecting = false;
    s.reconnect_attempt = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp::session::Session;
    use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot};

    fn auth(playing: bool) -> Vec<u8> {
        let mut user = 1i32.to_le_bytes().to_vec();
        protocol::string(&mut user, "test");
        user.push(0);
        let mut bytes = vec![1, 1];
        bytes.extend(&user);
        bytes.push(1); // room exists
        protocol::string(&mut bytes, "r");
        if playing {
            bytes.push(2);
        } else {
            bytes.extend([0, 1]);
            bytes.extend(7i32.to_le_bytes());
        }
        // live, locked, cycle, host, ready, users length, user map key.
        bytes.extend([1, 0, 0, 1, 0, 1]);
        bytes.extend(1i32.to_le_bytes());
        bytes.extend(user);
        bytes
    }
    async fn accept(listener: &TcpListener) -> TcpStream {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 1);
        assert_eq!(protocol::read_frame(&mut socket).await.unwrap(), [1, 4, b't', b'e', b's', b't']);
        socket
    }
    async fn until(s: &Session, condition: impl Fn(&Snapshot) -> bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !condition(&s.snapshot()) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn reconnect_keeps_handle_invalidates_old_game_and_never_replays_rpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut first = accept(&listener).await;
            protocol::write_frame(&mut first, &auth(false)).await.unwrap();
            assert_eq!(protocol::read_frame(&mut first).await.unwrap(), [11]);
            protocol::write_frame(&mut first, &[15, 1]).await.unwrap();
            protocol::write_frame(&mut first, &[6, 2]).await.unwrap();
            assert_eq!(protocol::read_frame(&mut first).await.unwrap(), [2, 1, b'a']);
            drop(first); // request outcome is unknown; it must never be replayed
            let mut second = accept(&listener).await;
            protocol::write_frame(&mut second, &auth(true)).await.unwrap();
            while let Ok(b) = protocol::read_frame(&mut second).await {
                assert_eq!(b, [0], "old Chat/Start/Abort or live packet crossed the connection boundary");
                protocol::write_frame(&mut second, &[0]).await.unwrap();
            }
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        let game_handle = s.clone();
        let old_generation = s.snapshot().room_generation;
        s.mark_prepared(s.snapshot().chart_generation, 7).unwrap();
        s.request(Request::Start).await.unwrap();
        until(&s, |s| s.start.is_some()).await;
        let launch = s.take_launch().unwrap();
        assert!(s.request(Request::Chat("a".into())).await.is_err());
        until(&game_handle, |s| s.connected && s.room_generation > old_generation).await;
        assert_eq!(s.snapshot().room_generation, game_handle.snapshot().room_generation);
        assert!(s.snapshot().prepared.is_none());
        assert!(s.take_launch().is_none());
        assert!(!game_handle.live_context_valid(launch));
        assert!(!game_handle.live(launch, vec![4, 0]));
        assert!(game_handle.finish(launch).is_err());
        assert!(s.request(Request::Abort).await.is_err());
        assert!(s.mark_prepared(s.snapshot().chart_generation, 7).is_err());
        s.close();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn explicit_close_cancels_backoff_without_opening_another_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (drop_tx, drop_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut first = accept(&listener).await;
            protocol::write_frame(&mut first, &auth(false)).await.unwrap();
            drop_rx.await.unwrap();
            drop(first);
            assert!(tokio::time::timeout(Duration::from_millis(1500), listener.accept()).await.is_err());
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        drop_tx.send(()).unwrap();
        until(&s, |s| s.reconnecting).await;
        assert!(s.request(Request::Start).await.is_err());
        s.close();
        until(&s, |s| !s.reconnecting).await;
        assert!(!s.snapshot().connected);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn recovery_installs_auth_snapshot_before_applying_early_pushes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (drop_tx, drop_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut first = accept(&listener).await;
            protocol::write_frame(&mut first, &auth(false)).await.unwrap();
            drop_rx.await.unwrap();
            drop(first);
            let mut second = accept(&listener).await;
            protocol::write_frame(&mut second, &[6, 0, 1, 8, 0, 0, 0]).await.unwrap();
            protocol::write_frame(&mut second, &auth(false)).await.unwrap();
            protocol::write_frame(&mut second, &[6, 1]).await.unwrap();
            while let Ok(b) = protocol::read_frame(&mut second).await {
                assert_eq!(b, [0]);
                protocol::write_frame(&mut second, &[0]).await.unwrap();
            }
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        let g = s.snapshot().chart_generation;
        s.mark_prepared(g, 7).unwrap();
        drop_tx.send(()).unwrap();
        until(&s, |s| s.connected && s.room.as_ref().is_some_and(|r| r.phase == protocol::Phase::Waiting && r.chart == Some(8))).await;
        assert!(s.mark_prepared(g, 7).is_err());
        assert!(s.snapshot().prepared.is_none());
        s.close();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn close_cancels_an_unresponsive_reauthentication() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (drop_tx, drop_rx) = oneshot::channel();
        let (auth_tx, auth_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut first = accept(&listener).await;
            protocol::write_frame(&mut first, &auth(false)).await.unwrap();
            drop_rx.await.unwrap();
            drop(first);
            let mut second = accept(&listener).await;
            auth_tx.send(()).unwrap();
            // Deliberately never reply. Cancel must close the new socket promptly.
            assert!(tokio::time::timeout(Duration::from_secs(1), protocol::read_frame(&mut second))
                .await
                .unwrap()
                .is_err());
        });
        let s = Session::connect(&addr, "test".into()).await.unwrap();
        drop_tx.send(()).unwrap();
        auth_rx.await.unwrap();
        s.close();
        until(&s, |s| !s.reconnecting).await;
        server.await.unwrap();
        assert!(!s.snapshot().authenticated);
    }
}
