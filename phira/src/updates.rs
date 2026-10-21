//! Public release checks use Pro releases rather than the upstream Phira version.
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
}

pub fn version(tag: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let (base, pro) = tag.trim_start_matches('v').split_once("-pro.")?;
    let nums = base.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>().ok()?;
    if nums.len() != 3 {
        return None;
    }
    let (revision, fix) = pro.split_once("-fix").unwrap_or((pro, "0"));
    Some((nums[0], nums[1], nums[2], revision.parse().ok()?, fix.parse().ok()?))
}

pub async fn check() -> Result<Option<Release>> {
    let release: Release = crate::client::basic_client_builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(concat!("Phira-Pro/", env!("PHIRA_PRO_VERSION")))
        .build()?
        .get("https://api.github.com/repos/Phira-Pro/Phira-Pro/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let current = version(crate::PRO_VERSION).unwrap_or_default();
    Ok((!release.draft
        && !release.prerelease
        && version(&release.tag_name).is_some_and(|v| v > current)
        && release.html_url.starts_with("https://github.com/Phira-Pro/Phira-Pro/releases/"))
    .then_some(release))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_pro_revisions_and_fixes() {
        assert!(version("v0.8.2-pro.17") > version("0.8.2-pro.16"));
        assert!(version("v0.8.2-pro.16-fix1") > version("0.8.2-pro.16"));
        assert!(version("v0.8.2-pro.16") > version("0.8.2-pro.9"));
        assert_eq!(version("v0.8.2-pro.17-beta"), None);
    }
}
