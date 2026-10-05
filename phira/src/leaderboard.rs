//! Keep server ranks separate from the unranked, deduplicated mixed feed.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Origin {
    Official,
    Pro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoardSource {
    Pro,
    Mixed,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RankState {
    Loading,
    Ranked(u32),
    NoRecord,
    Unavailable,
    SignIn,
    Offline,
    Unsupported,
}

type CacheKey = (String, Origin, i32, bool, Option<i32>);
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
}

fn sorted(mut records: Vec<(Record, Origin)>, mode: u8) -> Vec<(Record, Origin)> {
    records.retain(|(record, _)| record.accuracy.is_finite() && (mode != 1 || record.std_score.is_some_and(|v| v.is_finite())));
    records.sort_by(|(a, ao), (b, bo)| {
        a.player.id.cmp(&b.player.id).then_with(|| compare(a, b, mode)).then_with(|| {
            // Prefer Pro for otherwise identical results, independent of feed arrival order.
            (*ao != Origin::Pro).cmp(&(*bo != Origin::Pro))
        })
    });
    records.dedup_by_key(|(record, _)| record.player.id);
    records.sort_by(|(a, _), (b, _)| compare(a, b, mode));
    records
}

fn ranked(records: Vec<Record>, mode: u8, me: Option<i32>, origin: Origin) -> Vec<LdbItem> {
    rows(sorted(records.into_iter().map(|it| (it, origin)).collect(), mode), mode, me, true)
}

fn rows(records: Vec<(Record, Origin)>, mode: u8, me: Option<i32>, show_rank: bool) -> Vec<LdbItem> {
    let mut previous = None;
    let mut rank = 0;
    records
        .into_iter()
        .enumerate()
        .filter_map(|(i, (inner, origin))| {
            let key = value(&inner, mode);
            if previous != Some(key) {
                rank = i as u32 + 1;
                previous = Some(key);
            }
            (i < 20 || Some(inner.player.id) == me).then(|| LdbItem {
                inner,
                rank: show_rank.then_some(rank),
                origin,
                btn: prpr::ui::RectButton::new(),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct OfficialRankedRecord {
    #[serde(flatten)]
    inner: Record,
    rank: u32,
}

fn official_rank(records: &[OfficialRankedRecord], player: i32) -> RankState {
    records
        .iter()
        .find(|it| it.inner.player.id == player)
        .map_or(RankState::NoRecord, |it| RankState::Ranked(it.rank))
}

/// Match the original client: list15 includes the current player's server rank.
pub(super) async fn load_official_rank(chart: i32, mode: u8, player: i32) -> Result<RankState> {
    let records: Vec<OfficialRankedRecord> = recv_raw(Client::get(format!("/record/list15/{chart}")).query(&[("std", mode == 1)]))
        .await?
        .json()
        .await?;
    Ok(official_rank(&records, player))
}

pub(super) fn select(official: &[Record], pro: &[Record], source: BoardSource, mode: u8, me: Option<i32>) -> Vec<LdbItem> {
    match source {
        BoardSource::Pro => {
            let mut items = ranked(pro.to_vec(), mode, me, Origin::Pro);
            if mode == 2 {
                // The Pro API has no accuracy rank. Sorting returned records is not a rank.
                for item in &mut items {
                    item.rank = None;
                }
            }
            items
        }
        BoardSource::Mixed => {
            let records = official
                .iter()
                .cloned()
                .map(|it| (it, Origin::Official))
                .chain(pro.iter().cloned().map(|it| (it, Origin::Pro)))
                .collect();
            rows(sorted(records, mode), mode, me, false)
        }
        BoardSource::Local => Vec::new(),
    }
}

pub(super) async fn load(chart: i32, mode: u8, origin: Origin, me: Option<i32>) -> Result<Vec<Record>> {
    let std = mode == 1;
    let key = (crate::client::api_url(), origin, chart, std, me);
    let cached = CACHE
        .lock()
        .unwrap()
        .get(&key)
        .filter(|(time, _)| time.elapsed() < Duration::from_secs(60))
        .map(|(_, records)| records.clone());
    if let Some(records) = cached {
        Ok(records)
    } else {
        let records = match origin {
            Origin::Official => complete_official(chart, std).await?,
            Origin::Pro => pro_board(chart, std).await?,
        };
        let mut cache = CACHE.lock().unwrap();
        cache.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(60));
        cache.insert(key, (Instant::now(), records.clone()));
        Ok(records)
    }
}

pub(super) fn invalidate(chart: i32) {
    CACHE.lock().unwrap().retain(|key, _| key.2 != chart);
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
        let score = ranked(rows.clone(), 0, Some(18), Origin::Official);
        let std = ranked(rows.clone(), 1, Some(18), Origin::Official);
        let accuracy = ranked(rows, 2, Some(18), Origin::Official);
        assert_eq!(score.iter().find(|r| r.inner.player.id == 18).unwrap().rank, Some(18));
        assert_eq!(std.iter().find(|r| r.inner.player.id == 18).unwrap().rank, Some(1));
        assert_eq!(accuracy.iter().find(|r| r.inner.player.id == 18).unwrap().rank, Some(1));
        assert!(std.iter().any(|r| r.inner.player.id == 40));
    }

    #[test]
    fn ties_skip_positions_and_outside_top_twenty_keeps_real_rank() {
        let mut rows: Vec<_> = (1..=40).map(|i| record(i, 100 - i, 0.99, i as f32)).collect();
        rows[1].score = rows[0].score;
        let list = ranked(rows.clone(), 0, Some(30), Origin::Official);
        assert_eq!(list[0].rank, Some(1));
        assert_eq!(list[1].rank, Some(1));
        assert_eq!(list[2].rank, Some(3));
        assert_eq!(list.last().unwrap().rank, Some(30));
        assert!(ranked(rows, 2, Some(30), Origin::Official).iter().all(|r| r.rank == Some(1)));
    }

    #[test]
    fn mixed_feed_keeps_the_better_source_without_ranks() {
        let official = vec![record(1, 980_000, 0.98, 900_000.), record(2, 990_000, 0.99, 910_000.)];
        let pro = vec![record(1, 970_000, 0.995, 950_000.), record(3, 995_000, 0.999, 980_000.)];
        let mixed = select(&official, &pro, BoardSource::Mixed, 0, Some(1));
        assert!(mixed.iter().all(|it| it.rank.is_none()));
        assert_eq!(mixed.len(), 3);
        assert_eq!(mixed[0].origin, Origin::Pro);
        assert_eq!(mixed.iter().find(|it| it.inner.player.id == 1).unwrap().origin, Origin::Official);
        let std = select(&official, &pro, BoardSource::Mixed, 1, Some(1));
        assert_eq!(std.iter().find(|it| it.inner.player.id == 1).unwrap().origin, Origin::Pro);
    }

    #[test]
    fn pro_board_never_includes_official_records_or_invents_accuracy_ranks() {
        let official = vec![record(9, 1_000_000, 1., 1_000_000.)];
        let pro = vec![record(1, 900_000, 0.99, 800_000.)];
        let items = select(&official, &pro, BoardSource::Pro, 0, Some(9));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].inner.player.id, 1);
        assert_eq!(items[0].rank, Some(1));
        assert!(select(&official, &pro, BoardSource::Pro, 2, None).iter().all(|it| it.rank.is_none()));
    }

    #[test]
    fn official_personal_rank_keeps_the_server_rank_outside_the_top_list() {
        let records = vec![
            OfficialRankedRecord {
                inner: record(1, 990_000, 0.99, 950_000.),
                rank: 1,
            },
            OfficialRankedRecord {
                inner: record(2, 980_000, 0.98, 940_000.),
                rank: 2,
            },
            OfficialRankedRecord {
                inner: record(9, 970_000, 0.97, 990_000.),
                rank: 2415,
            },
        ];
        assert_eq!(official_rank(&records, 9), RankState::Ranked(2415));
        assert_eq!(official_rank(&records, 10), RankState::NoRecord);
    }

    #[test]
    fn official_list15_response_reads_the_flattened_rank() {
        let record: OfficialRankedRecord = serde_json::from_value(serde_json::json!({
            "id": 77, "player": 9, "chart": 1, "rank": 2415, "score": 970000,
            "accuracy": 0.97, "perfect": 1, "good": 0, "bad": 0, "miss": 0,
            "speed": 1, "max_combo": 1, "full_combo": true, "best": true,
            "mods": 0, "time": "2026-10-03T00:00:00Z", "std": 0.01, "std_score": 990000
        }))
        .unwrap();
        assert_eq!(official_rank(&[record], 9), RankState::Ranked(2415));
    }
}
