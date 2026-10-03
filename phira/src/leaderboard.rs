//! Rank complete record feeds before selecting the visible top rows.
use super::LdbItem;
use crate::client::{recv_raw, Client, Record};
use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

type CacheKey = (String, String, i32, bool);
static CACHE: Lazy<Mutex<HashMap<CacheKey, (Instant, Vec<Record>)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
struct Page {
    count: usize,
    results: Vec<Record>,
}

async fn page(chart: i32, number: usize, pro: bool, std: bool) -> Result<Page> {
    let path = format!("/record/query/{chart}");
    let request = if pro {
        crate::client::pro_get(&path).ok_or_else(|| anyhow::anyhow!("Pro leaderboard is unavailable"))?
    } else {
        Client::get(&path)
    };
    Ok(recv_raw(request.query(&[("page", number.to_string()), ("pageNum", "30".into()), ("std", std.to_string())]))
        .await?
        .json()
        .await?)
}

async fn complete_feed(chart: i32, pro: bool, std: bool) -> Result<Vec<Record>> {
    let first = page(chart, 1, pro, std).await?;
    let pages = first.count.div_ceil(30);
    let mut records = first.results;
    // Bounded concurrency keeps both the UI thread and the server responsive.
    for start in (2..=pages).step_by(4) {
        let fetched = futures_util::future::try_join_all((start..=(start + 3).min(pages)).map(|number| page(chart, number, pro, std))).await?;
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

pub(super) async fn load(chart: i32, mode: u8, me: Option<i32>) -> Result<Vec<LdbItem>> {
    let key = (crate::client::api_url(), crate::client::pro_api_url(), chart, mode == 1);
    let cached = CACHE
        .lock()
        .unwrap()
        .get(&key)
        .filter(|(time, _)| time.elapsed() < Duration::from_secs(60))
        .map(|(_, records)| records.clone());
    let records = if let Some(records) = cached {
        records
    } else {
        let mut records = complete_feed(chart, false, mode == 1).await?;
        if !key.1.is_empty() {
            records.extend(complete_feed(chart, true, mode == 1).await?);
        }
        let mut cache = CACHE.lock().unwrap();
        cache.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(60));
        cache.insert(key, (Instant::now(), records.clone()));
        records
    };
    Ok(ranked(records, mode, me))
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
