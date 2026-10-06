//! Official block data stored next to an RPE chart, outside its JSON.
use super::pgr::{parse_block_areas, PgrBlockArea};
use crate::{core::BlockArea, fs::FileSystem};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct BlockAreaDocument {
    #[serde(rename = "blockAreaList", alias = "BlockAreaList")]
    areas: Vec<PgrBlockArea>,
}

fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').map_or(("", path), |(dir, name)| (dir, name))
}

// Prefer a chart-specific file, then an explicitly shared file. A single
// exported file can also accompany a renamed chart; never pick arbitrarily
// when a package contains several exports for different difficulties.
fn select_file(files: &BTreeSet<String>, stem: Option<&str>) -> Result<Option<String>> {
    let named = stem.map(|stem| format!("{stem}.blockAreaList.json"));
    for expected in named.iter().map(String::as_str).chain(std::iter::once("blockAreaList.json")) {
        let matches: Vec<_> = files.iter().filter(|path| split_path(path).1.eq_ignore_ascii_case(expected)).collect();
        match matches.as_slice() {
            [path] => return Ok(Some((*path).clone())),
            [] => {}
            _ => bail!("Multiple blockAreaList files match {expected}: {matches:?}"),
        }
    }
    match files.len() {
        0 => Ok(None),
        1 => Ok(files.first().cloned()),
        _ => bail!("Multiple blockAreaList files found; name the file <chart-name>.blockAreaList.json: {files:?}"),
    }
}

pub(super) async fn load_external(fs: &mut dyn FileSystem, chart_path: Option<&str>) -> Result<Vec<BlockArea>> {
    let path = chart_path.unwrap_or("").replace('\\', "/");
    let (dir, name) = split_path(&path);
    let stem = (!name.is_empty()).then(|| name.rsplit_once('.').map_or(name, |(stem, _)| stem));
    let mut files: BTreeSet<_> = fs
        .list_root()
        .context("Cannot list blockAreaList files")?
        .into_iter()
        .filter(|path| split_path(path).0 == dir && crate::fs::is_block_area_sidecar(path))
        .collect();
    // list_root does not enumerate chart subdirectories, and asset file
    // systems cannot enumerate at all. Probe standard names in those cases.
    if !dir.is_empty() || files.is_empty() {
        let mut names = Vec::new();
        for list_name in ["blockAreaList.json", "BlockAreaList.json", "blockarealist.json"] {
            if let Some(stem) = stem {
                names.push(format!("{stem}.{list_name}"));
            }
            names.push(list_name.to_owned());
        }
        for name in names {
            let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if fs.exists(&path).await.with_context(|| format!("Cannot check {path}"))? {
                // Windows can resolve several casing variants to the same file.
                if !files.iter().any(|existing| existing.eq_ignore_ascii_case(&path)) {
                    files.insert(path);
                }
            }
        }
    }
    let Some(path) = select_file(&files, stem)? else {
        return Ok(Vec::new());
    };
    let bytes = fs.load_file(&path).await.with_context(|| format!("Cannot read {path}"))?;
    // Decode large lists on the loading worker; geometry/render resources
    // remain on the original thread. Time values are seconds, without BPM conversion.
    let areas = crate::loading_work::run(move || {
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
        Ok(serde_json::from_slice::<BlockAreaDocument>(bytes)?.areas)
    })
    .await
    .with_context(|| format!("Invalid blockAreaList file {path}"))?;
    Ok(parse_block_areas(areas))
}
