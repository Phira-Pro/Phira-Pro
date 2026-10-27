//! Legacy v1 changes a role only through a real leave/join transaction.
use super::{protocol::{Phase, Request}, session::Session};
use anyhow::{Context, Result};

pub async fn switch(session: Session, monitor: bool) -> Result<()> {
    let snapshot = session.snapshot();
    let room = snapshot.room.as_ref().context("尚未加入房间")?;
    let me = snapshot.me.as_ref().context("尚未鉴权")?;
    anyhow::ensure!(snapshot.connected && !snapshot.reconnecting, "请等待连接恢复");
    anyhow::ensure!(room.phase == Phase::Selecting && !room.locked, "请在房间解锁后的选曲阶段切换游玩身份");
    if me.monitor == monitor { return Ok(()); }
    anyhow::ensure!(!monitor || room.users.values().any(|user| user.id != me.id && !user.monitor), "房间中还没有其他可观看的玩家");
    let id = room.id.clone();
    let previous = me.monitor;
    session.request(Request::Leave).await?;
    let request = if monitor { Request::JoinMonitor(id.clone()) } else { Request::Join(id.clone()) };
    if let Err(error) = session.request(request).await {
        let rollback = if previous { Request::JoinMonitor(id) } else { Request::Join(id) };
        return match session.request(rollback).await {
            Ok(()) => Err(anyhow::anyhow!("切换身份失败，已恢复原身份：{error:#}")),
            Err(rollback) => Err(anyhow::anyhow!("切换身份失败：{error:#}；恢复原身份失败：{rollback:#}")),
        };
    }
    Ok(())
}
