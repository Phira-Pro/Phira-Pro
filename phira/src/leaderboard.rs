//! Rank complete record feeds before selecting the visible top rows.
//!
//! 混合榜：官服（`/record/query/{chart}`）与 Phira Pro 私服
//! （`/api/v1/charts/{chart}/leaderboard`）双向合并，按 player 去重后取较优，
//! 再统一排名。自己的名次只取私服（`/players/{player}/rank`）。
use super::LdbItem;
use crate::client::{recv_raw, Client, Ptr, Record};
use anyhow::{bail, Result};
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

type CacheKey = (i32, bool);
static CACHE: Lazy<Mutex<HashMap<CacheKey, (Instant, Vec<Record>)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
struct Page {
    count: usize,
    results: Vec<Record>,
}

async fn page(chart: i32, number: usize, std: bool) -> Result<Page> {
    let path = format!("/record/query/{chart}");
    Ok(recv_raw(Client::get(&path).query(&[("page", number.to_string()), ("pageNum", "30".into()), ("std", std.to_string())]))
        .await?
        .json()
        .await?)
}

/// 拉取官服该谱面的全部记录（官服每玩家只返回一条 best）。
async fn complete_official(chart: i32, std: bool) -> Result<Vec<Record>> {
    let first = page(chart, 1, std).await?;
    let pages = first.count.div_ceil(30);
    let mut records = first.results;
    // Bounded concurrency keeps both the UI thread and the server responsive.
    for start in (2..=pages).step_by(4) {
        let fetched = futures_util::future::try_join_all((start..=(start + 3).min(pages)).map(|number| page(chart, number, std))).await?;
        for next in fetched {
            if next.results.is_empty() && records.len() < first.count {
                bail!("排行榜分页未返回完整数据，请刷新后重试");
            }
            records.extend(next.results);
        }
    }
    if records.len() < first.count {
        bail!("排行榜分页未返回完整数据，请刷新后重试");
    }
    Ok(records)
}

/// 私服榜（`GET /api/v1/charts/{chart}/leaderboard?metric=score|stdScore`）。
#[derive(Deserialize)]
struct ProScore {
    id: i64,
    data: ProScoreData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProScoreData {
    player: i32,
    chart: i32,
    perfect: i32,
    good: i32,
    bad: i32,
    miss: i32,
    max_combo: i32,
    mods: i32,
    speed: f32,
    std: f32,
    score: i32,
    accuracy: f32,
    full_combo: bool,
    std_score: i32,
}

/// 把私服成绩转成展示用的 [`Record`]（官服榜与私服榜共用同一种展示模型）。
fn pro_to_record(it: ProScore) -> Record {
    let d = it.data;
    Record {
        id: it.id as i32,
        player: Ptr::from(d.player),
        chart: Ptr::from(d.chart),
        score: d.score,
        accuracy: d.accuracy,
        perfect: d.perfect,
        good: d.good,
        bad: d.bad,
        miss: d.miss,
        speed: d.speed,
        max_combo: d.max_combo,
        full_combo: d.full_combo,
        best: true,
        mods: d.mods,
        time: Utc::now(),
        std: Some(d.std),
        std_score: Some(d.std_score as f32),
    }
}

/// 拉取私服榜（最多 20 条，每玩家一条最佳）。
async fn pro_board(chart: i32, std: bool) -> Result<Vec<Record>> {
    let metric = if std { "stdScore" } else { "score" };
    let path = format!("/api/v1/charts/{chart}/leaderboard?metric={metric}");
    let scores: Vec<ProScore> = crate::client::pro_get(path).await?.json().await?;
    Ok(scores.into_iter().map(pro_to_record).collect())
}

fn value(record: &Record, mode: u8) -> f64 {
    match mode {
        1 => record.std_score.filter(|value| value.is_finite()).unwrap_or(0.) as f64,
        2 => record.accuracy as f64,
        _ => record.score as f64,
    }
}

fn compare(a: &Record, b: &Record, mode: u8) -> std::cmp::Ordering {
    value(b, mode)
        .total_cmp(&value(a, mode))
        .then(b.score.cmp(&a.score))
        .then(b.accuracy.total_cmp(&a.accuracy))
        .then(a.player.id.cmp(&b.player.id))
        .then(a.id.cmp(&b.id))
}

fn ranked(mut records: Vec<Record>, mode: u8, me: Option<i32>) -> Vec<LdbItem> {
    records.retain(|record| record.accuracy.is_finite() && (mode != 1 || record.std_score.is_some_and(|v| v.is_finite())));
    records.sort_by(|a, b| a.player.id.cmp(&b.player.id).then_with(|| compare(a, b, mode)));
    records.dedup_by_key(|record| record.player.id);
    records.sort_by(|a, b| compare(a, b, mode));
    let mut previous = None;
    let mut rank = 0;
    records
        .into_iter()
        .enumerate()
        .filter_map(|(i, inner)| {
            let key = value(&inner, mode);
            if previous != Some(key) {
                rank = i as u32 + 1;
                previous = Some(key);
            }
            (i < 20 || Some(inner.player.id) == me).then(|| LdbItem {
                inner,
                rank,
                btn: prpr::ui::RectButton::new(),
            })
        })
        .collect()
}

/// 玩家在私服榜的名次（自己的名次只看私服）。无成绩时返回 `None`。
async fn my_pro_rank(chart: i32, player: i32, std: bool) -> Option<u32> {
    let metric = if std { "stdScore" } else { "score" };
    crate::client::pro_player_rank(chart, player, metric).await.ok().map(|it| it as u32)
}

pub(super) async fn load(chart: i32, mode: u8, me: Option<i32>) -> Result<Vec<LdbItem>> {
    let std = mode == 1;
    let key = (chart, std);
    let cached = CACHE
        .lock()
        .unwrap()
        .get(&key)
        .filter(|(time, _)| time.elapsed() < Duration::from_secs(60))
        .map(|(_, records)| records.clone());
    let records = if let Some(records) = cached {
        records
    } else {
        // 官服全量 + 私服（最多 20）合并；任一边失败都按正常的请求失败处理。
        let mut records = complete_official(chart, std).await?;
        records.extend(pro_board(chart, std).await?);
        let mut cache = CACHE.lock().unwrap();
        cache.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(60));
        cache.insert(key, (Instant::now(), records.clone()));
        records
    };
    let mut items = ranked(records, mode, me);
    // 自己的名次只用私服值（不试图推算跨服全局名次）。
    if let Some(me) = me {
        if let Some(rank) = my_pro_rank(chart, me, std).await {
            if let Some(item) = items.iter_mut().find(|it| it.inner.player.id == me) {
                item.rank = rank;
            }
        }
    }
    Ok(items)
}

pub(super) fn invalidate(chart: i32) {
    CACHE.lock().unwrap().retain(|key, _| key.0 != chart);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(player: i32, score: i32, accuracy: f32, std_score: f32) -> Record {
        serde_json::from_value(serde_json::json!({"id":player,"player":player,"chart":1,"score":score,"accuracy":accuracy,"perfect":1,"good":0,"bad":0,"miss":0,"speed":1,"max_combo":1,"full_combo":true,"best":true,"mods":0,"time":"2026-10-03T00:00:00Z","std":0.01,"std_score":std_score})).unwrap()
    }

    #[test]
    fn each_mode_uses_the_complete_feed_and_its_own_best_record() {
        let mut rows: Vec<_> = (1..=40).map(|i| record(i, 100 - i, i as f32 / 100., i as f32)).collect();
        rows.push(record(18, 1, 0.99, 99.));
        let score = ranked(rows.clone(), 0, Some(18));
        let std = ranked(rows.clone(), 1, Some(18));
        let accuracy = ranked(rows, 2, Some(18));
        assert_eq!(score.iter().find(|r| r.inner.player.id == 18).unwrap().rank, 18);
        assert_eq!(std.iter().find(|r| r.inner.player.id == 18).unwrap().rank, 1);
        assert_eq!(accuracy.iter().find(|r| r.inner.player.id == 18).unwrap().rank, 1);
        assert!(std.iter().any(|r| r.inner.player.id == 40));
    }

    #[test]
    fn ties_skip_positions_and_outside_top_twenty_keeps_real_rank() {
        let mut rows: Vec<_> = (1..=40).map(|i| record(i, 100 - i, 0.99, i as f32)).collect();
        rows[1].score = rows[0].score;
        let list = ranked(rows.clone(), 0, Some(30));
        assert_eq!(list[0].rank, 1);
        assert_eq!(list[1].rank, 1);
        assert_eq!(list[2].rank, 3);
        assert_eq!(list.last().unwrap().rank, 30);
        assert!(ranked(rows, 2, Some(30)).iter().all(|r| r.rank == 1));
    }
}
