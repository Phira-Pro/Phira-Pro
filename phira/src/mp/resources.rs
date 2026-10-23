//! HTTP metadata and disk validation are independent of room/RPC state.
use crate::client::{Chart, Ptr};
use anyhow::{ensure, Context, Result};
use std::sync::Arc;
pub struct ResourceCheck {
    pub chart: Arc<Chart>,
    pub existing: Option<String>,
    pub current: Option<String>,
}
pub async fn resolve(id: i32, existing: Option<String>) -> Result<ResourceCheck> {
    let chart = tokio::time::timeout(std::time::Duration::from_secs(30), Ptr::<Chart>::new(id).fetch())
        .await
        .context("查询联机谱面资源超时")??;
    let current = if let Some(path) = existing.clone() {
        let root = crate::dir::charts()?;
        let updated = chart.chart_updated;
        tokio::task::spawn_blocking(move || -> Result<String> {
            validate(&format!("{root}/{path}"), id, updated)?;
            Ok(path)
        })
        .await?
        .ok()
    } else {
        None
    };
    Ok(ResourceCheck { chart, existing, current })
}
fn validate(root: &str, id: i32, updated: chrono::DateTime<chrono::Utc>) -> Result<()> {
    let directory = prpr::dir::Dir::new(root)?;
    let info: prpr::info::ChartInfo = serde_yaml::from_reader(directory.open("info.yml")?).context("缓存谱面信息无效")?;
    ensure!(info.id == Some(id) && info.chart_updated == Some(updated), "缓存谱面版本与服务端不一致");
    for name in [&info.chart, &info.music, &info.illustration] {
        ensure!(directory.open(name)?.metadata()?.len() > 0, "缓存谱面资源为空");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_cache_is_not_preparation_evidence() {
        assert!(super::validate("this-cache-does-not-exist", 7, chrono::Utc::now()).is_err());
    }
}
