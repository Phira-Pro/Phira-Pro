//! 本地玩家黑名单。
//!
//! 名单里记录的是 Phira 的数字用户 ID。命中名单的玩家不会出现在本机的
//! 任何排行榜（曲目 / 活动）、多人房间玩家列表，以及用户搜索与 `@` 补全里。
//!
//! 只影响本机显示，纯客户端过滤，不向服务器发送任何东西。
//!
//! 设计上跟 `history` 一样保持简单：
//! - 一个 JSON 文件 `<data>/blacklist.json` 装全部条目；
//! - 首次访问读盘，写入后立刻落盘（增删不频繁，不心疼）；
//! - 每条的 `name` 只是加入时的昵称快照，供管理页展示，判定一律以数字 ID 为准。

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};

use crate::dir;

/// 黑名单里的一条。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    /// Phira 数字用户 ID —— 唯一判定依据。
    pub id: i32,
    /// 加入时的昵称快照（仅用于管理页展示）。
    pub name: String,
    /// 加入时的 Unix 毫秒时间戳。
    pub time: i64,
}

static LIST: Lazy<Mutex<Option<Vec<Entry>>>> = Lazy::new(|| Mutex::new(None));

/// 每次增删自增。已经加载过排行榜的场景可以据此判断要不要重新拉取。
static VERSION: AtomicU32 = AtomicU32::new(0);

/// 当前版本号；变化说明黑名单被改过。
pub fn version() -> u32 {
    VERSION.load(Ordering::Relaxed)
}

fn path() -> Result<String> {
    Ok(format!("{}/blacklist.json", dir::root()?))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn load_inner() -> Vec<Entry> {
    (|| -> Result<Vec<Entry>> {
        let path = path()?;
        if !std::path::Path::new(&path).exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&text).with_context(|| format!("failed to parse {path}"))?)
    })()
    .unwrap_or_default()
}

fn with_list<T>(f: impl FnOnce(&mut Vec<Entry>) -> T) -> T {
    let mut guard = LIST.lock().unwrap();
    let list = guard.get_or_insert_with(load_inner);
    f(list)
}

pub fn ensure_loaded() {
    with_list(|_| ());
}

/// 该玩家是否在黑名单里（判定入口，尽量轻量）。
pub fn contains(id: i32) -> bool {
    with_list(|it| it.iter().any(|e| e.id == id))
}

/// 全部条目（按 ID 升序）。
pub fn all() -> Vec<Entry> {
    with_list(|it| {
        let mut v = it.clone();
        v.sort_by_key(|e| e.id);
        v
    })
}

/// 加入黑名单；已存在则刷新昵称与时间。返回是否是新增。
pub fn add(id: i32, name: &str) -> Result<bool> {
    let added = with_list(|it| {
        if let Some(e) = it.iter_mut().find(|e| e.id == id) {
            e.name = name.to_owned();
            e.time = now();
            false
        } else {
            it.push(Entry {
                id,
                name: name.to_owned(),
                time: now(),
            });
            true
        }
    });
    save()?;
    VERSION.fetch_add(1, Ordering::Relaxed);
    Ok(added)
}

/// 移出黑名单。
pub fn remove(id: i32) -> Result<()> {
    with_list(|it| it.retain(|e| e.id != id));
    save()?;
    VERSION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn save() -> Result<()> {
    let text = with_list(|it| serde_json::to_string_pretty(it))?;
    let path = path()?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {path}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Entry;

    #[test]
    fn parse_and_default() {
        let list: Vec<Entry> = serde_json::from_str(r#"[{"id":123,"name":"cheater","time":1}]"#).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, 123);
        // 缺字段时靠 serde(default) 兜底
        let list: Vec<Entry> = serde_json::from_str(r#"[{"id":7}]"#).unwrap();
        assert_eq!(list[0].id, 7);
        assert_eq!(list[0].name, "");
        assert_eq!(list[0].time, 0);
    }
}
