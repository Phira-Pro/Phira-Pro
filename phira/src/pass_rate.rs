use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub(crate) fn path(local_chart: &str) -> anyhow::Result<PathBuf> {
    let stem = hex::encode(Sha256::digest(local_chart.as_bytes()));
    Ok(PathBuf::from(crate::dir::root()?).join("passrate").join(format!("{stem}.passrate")))
}
