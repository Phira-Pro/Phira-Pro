//! Phira Pro：数据迁移——从官服找出「我有成绩的谱面」并下载到本地。
//!
//! 官服没有「某玩家的全部成绩」接口（`/record?player=` 只给最近 20 条），但可以反过来查：
//! `GET /record?player={uid}&chart={cid}` 能直接判断「该玩家在这张谱面上有没有成绩」。
//! 所以流程是：枚举官服谱面 → 按条件筛掉不要的 → 逐张查该玩家有没有成绩 → 并行下载。
//!
//! 没设筛选时会扫描全部谱面（近万张），会比较慢；调高「等级下限 / 评分下限」可大幅提速。

prpr_l10n::tl_file!("settings");

use crate::{
    client::{basic_client_builder, recv_raw, Chart, Client, CLIENT_TOKEN},
    data::LocalChart,
    dir,
};
use anyhow::{Context, Result};
use futures_util::stream::{self, StreamExt};
use prpr::{ext::unzip_into, info::ChartInfo};
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::Cursor,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tracing::warn;

/// 等级下限预设（0 = 不限）。
pub const MIN_DIFF_PRESETS: [f32; 6] = [0., 14., 15., 16., 17., 18.];
/// 评分下限预设（5 分制；0 = 不限）。官服 `rating` 是 0~1，1.0 对应 5.00 分。
pub const MIN_RATING_PRESETS: [f32; 6] = [0., 3.5, 4.0, 4.25, 4.5, 4.75];

/// 官服谱面列表每页上限（实测超过会报 `Too many entities in one page`）。
const PAGE_SIZE: usize = 30;
/// 枚举谱面的并发数。
const ENUM_CONCURRENCY: usize = 12;
/// 查询「有没有成绩」的并发数。
const CHECK_CONCURRENCY: usize = 8;
/// 下载谱面的并发数。
const DL_CONCURRENCY: usize = 4;

/// 迁移筛选条件。
#[derive(Clone, Copy)]
pub struct Filter {
    pub min_difficulty: f32,
    pub min_rating: f32,
    pub ranked_only: bool,
    pub skip_downloaded: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            min_difficulty: 0.,
            min_rating: 0.,
            ranked_only: false,
            skip_downloaded: true,
        }
    }
}

/// 跨线程共享的迁移进度（UI 只读）。
#[derive(Default)]
pub struct Progress {
    pub cancel: AtomicBool,
    pub status: Mutex<String>,
    pub done: AtomicUsize,
    pub ok: AtomicUsize,
    pub failed: AtomicUsize,
}

impl Progress {
    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    fn set(&self, s: String) {
        *self.status.lock().unwrap() = s;
        self.done.store(0, Ordering::Relaxed);
    }

    fn set_keep(&self, s: String) {
        *self.status.lock().unwrap() = s;
    }
}

#[derive(Deserialize)]
struct Pool {
    #[serde(rename = "bestPool")]
    best_pool: Vec<PoolItem>,
    #[serde(rename = "recentPool")]
    recent_pool: Vec<PoolItem>,
}

#[derive(Deserialize)]
struct PoolItem {
    chart: i32,
}

#[derive(Deserialize)]
struct RecItem {
    chart: i32,
}

#[derive(Deserialize)]
struct ChartPage {
    count: u64,
    results: Vec<Chart>,
}

#[derive(Deserialize)]
struct PlayedItem {
    #[allow(dead_code)]
    id: i32,
}

/// 便宜的种子来源：Best 池 ∪ recent 池 ∪ 最近 20 条记录（3 个请求）。
async fn fetch_seed_ids(me: i32) -> Vec<i32> {
    let mut ids: Vec<i32> = Vec::new();
    if let Ok(resp) = recv_raw(Client::get(format!("/record/get-pool/{me}"))).await {
        if let Ok(pool) = resp.json::<Pool>().await {
            ids.extend(pool.best_pool.into_iter().map(|it| it.chart));
            ids.extend(pool.recent_pool.into_iter().map(|it| it.chart));
        }
    }
    if let Ok(resp) = recv_raw(Client::get(format!("/record?player={me}"))).await {
        if let Ok(recs) = resp.json::<Vec<RecItem>>().await {
            ids.extend(recs.into_iter().map(|it| it.chart));
        }
    }
    ids
}

fn passes(entity: &Chart, f: &Filter) -> bool {
    if f.min_difficulty > 0. && entity.difficulty + 1e-4 < f.min_difficulty {
        return false;
    }
    if f.min_rating > 0. && entity.rating.unwrap_or(0.) * 5. + 1e-4 < f.min_rating {
        return false;
    }
    if f.ranked_only && !entity.ranked {
        return false;
    }
    true
}

/// 枚举官服全部谱面（并行翻页）。
async fn enumerate_charts(p: &Progress) -> Result<Vec<Chart>> {
    let first: ChartPage = recv_raw(Client::get(format!("/chart?page=1&pageNum={PAGE_SIZE}"))).await?.json().await?;
    let pages = (first.count as usize).div_ceil(PAGE_SIZE).max(1);

    let rest: Vec<Result<ChartPage>> = stream::iter(2..=pages)
        .map(|page| async move {
            Ok(recv_raw(Client::get(format!("/chart?page={page}&pageNum={PAGE_SIZE}")))
                .await?
                .json::<ChartPage>()
                .await?)
        })
        .buffer_unordered(ENUM_CONCURRENCY)
        .collect()
        .await;

    p.set(tl!("migrate-enumerating", "pages" => pages.to_string()));
    let mut out = first.results;
    for r in rest {
        match r {
            Ok(pg) => out.extend(pg.results),
            Err(err) => warn!(?err, "migrate: failed to fetch a chart page"),
        }
    }
    out.sort_by_key(|it| it.id);
    out.dedup_by_key(|it| it.id);
    Ok(out)
}

/// 并行查询「该玩家在这些谱面上有没有成绩」。
async fn check_played(me: i32, charts: Vec<Chart>, p: Arc<Progress>) -> Vec<Chart> {
    let total = charts.len();
    p.set(tl!("migrate-checking", "done" => "0".to_owned(), "total" => total.to_string()));

    let checked: Vec<(Chart, bool)> = stream::iter(charts)
        .map(|c| {
            let p = Arc::clone(&p);
            async move {
                if p.cancel.load(Ordering::Relaxed) {
                    return (c, false);
                }
                let mut played = false;
                for attempt in 0..2 {
                    let url = format!("/record?player={me}&chart={}", c.id);
                    match recv_raw(Client::get(&url)).await {
                        Ok(resp) => match resp.json::<Vec<PlayedItem>>().await {
                            Ok(v) => {
                                played = !v.is_empty();
                                break;
                            }
                            Err(err) => warn!(?err, "migrate: bad record response for chart {}", c.id),
                        },
                        Err(err) => {
                            if attempt == 1 {
                                warn!(?err, "migrate: failed to query chart {}", c.id);
                            }
                        }
                    }
                }
                let n = p.done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 20 == 0 || n == total {
                    p.set_keep(tl!("migrate-checking", "done" => n.to_string(), "total" => total.to_string()));
                }
                (c, played)
            }
        })
        .buffer_unordered(CHECK_CONCURRENCY)
        .collect()
        .await;

    checked.into_iter().filter_map(|(c, ok)| ok.then_some(c)).collect()
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let mut req = basic_client_builder().build()?.get(url);
    if let Some(token) = CLIENT_TOKEN.load().as_ref() {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let res = req.send().await?.error_for_status()?;
    Ok(res.bytes().await?.to_vec())
}

/// 下载并落盘一张谱面（结构与在线游玩下载一致：`data/charts/download/{id}`）。
async fn download_one(entity: &Chart) -> Result<LocalChart> {
    let id = entity.id;
    let path = format!("{}/{id}", dir::downloaded_charts()?);
    let path = std::path::Path::new(&path);
    let bytes = fetch_bytes(&entity.file.url)
        .await
        .with_context(|| format!("下载 {} 失败", entity.name))?;
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".migrate-").tempdir_in(parent)?;
    let dir = prpr::dir::Dir::new(staging.path())?;
    unzip_into(Cursor::new(bytes), &dir, true)?;

    let mut info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?).with_context(|| "info.yml 解析失败")?;
    info.id = Some(id);
    info.created = Some(entity.created);
    info.updated = Some(entity.updated);
    info.chart_updated = Some(entity.chart_updated);
    info.uploader = Some(entity.uploader.id);
    let mut fs = prpr::fs::fs_from_file(staging.path())?;
    prpr::fs::fix_info_with(fs.as_mut(), &mut info, false).await?;
    serde_yaml::to_writer(dir.create("info.yml")?, &info)?;
    crate::chart_install::publish(staging.path(), path)?;

    Ok(LocalChart {
        info: entity.to_info(),
        local_path: format!("download/{id}"),
        record: None,
        mods: Default::default(),
        played_unlock: false,
    })
}

/// 并行下载，返回成功的谱面。
async fn download_all(charts: Vec<Chart>, p: Arc<Progress>) -> Vec<LocalChart> {
    let total = charts.len();
    p.set(tl!("migrate-progress", "done" => "0".to_owned(), "total" => total.to_string(), "name" => String::new()));

    let got: Vec<Option<LocalChart>> = stream::iter(charts)
        .map(|c| {
            let p = Arc::clone(&p);
            async move {
                if p.cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let name = c.name.clone();
                let r = download_one(&c).await;
                let n = p.done.fetch_add(1, Ordering::Relaxed) + 1;
                p.set_keep(tl!("migrate-progress", "done" => n.to_string(), "total" => total.to_string(), "name" => name));
                match r {
                    Ok(chart) => {
                        p.ok.fetch_add(1, Ordering::Relaxed);
                        Some(chart)
                    }
                    Err(err) => {
                        p.failed.fetch_add(1, Ordering::Relaxed);
                        warn!(?err, "migrate: failed to download chart {}", c.id);
                        None
                    }
                }
            }
        })
        .buffer_unordered(DL_CONCURRENCY)
        .collect()
        .await;

    got.into_iter().flatten().collect()
}

/// 执行一次迁移。返回成功下载的谱面，由调用方在主线程入库。
pub async fn run(me: i32, downloaded: HashSet<i32>, filter: Filter, p: Arc<Progress>) -> Result<Vec<LocalChart>> {
    p.set(tl!("migrate-fetching").into_owned());
    let seed: HashSet<i32> = fetch_seed_ids(me).await.into_iter().collect();

    let all = enumerate_charts(&p).await?;
    let candidates: Vec<Chart> = all
        .into_iter()
        .filter(|it| passes(it, &filter))
        .filter(|it| !(filter.skip_downloaded && downloaded.contains(&it.id)))
        .collect();
    if p.cancel.load(Ordering::Relaxed) {
        return Ok(Vec::new());
    }

    // 种子（已知有成绩）直接收下，其余逐张查榜单。
    let (known, unknown): (Vec<Chart>, Vec<Chart>) = candidates.into_iter().partition(|it| seed.contains(&it.id));
    let checked = check_played(me, unknown, Arc::clone(&p)).await;
    if p.cancel.load(Ordering::Relaxed) {
        return Ok(Vec::new());
    }

    let mut todo = known;
    todo.extend(checked);
    todo.sort_by_key(|it| it.id);
    todo.dedup_by_key(|it| it.id);

    if todo.is_empty() {
        p.set(tl!("migrate-none").into_owned());
        return Ok(Vec::new());
    }

    let out = download_all(todo, Arc::clone(&p)).await;
    p.set_keep(tl!(
        "migrate-done",
        "ok" => out.len().to_string(),
        "failed" => p.failed.load(Ordering::Relaxed).to_string()
    ));
    Ok(out)
}
