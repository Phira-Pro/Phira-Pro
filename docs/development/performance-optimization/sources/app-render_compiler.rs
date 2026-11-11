//! Effective render-plan consumer, independently buildable without diagnostic
//! or negative note prototypes. Original judgement, audio and pose are retained.
use crate::core::{Chart, Resource};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
#[path = "render_lab/cache.rs"]
pub mod cache;
#[path = "render_lab/lines.rs"]
pub mod lines;
#[path = "render_reads.rs"]
pub mod reads;
#[path = "render_visibility.rs"]
pub mod visibility;
#[path = "visibility_bounds.rs"]
pub mod visibility_bounds;
// CPU workset format foundation. No runtime consumer is enabled by declaring
// this module; its certificates still require an owner-sealed chart scope.
#[path = "render_workset.rs"]
pub(crate) mod workset;
#[path = "render_workset_ir.rs"]
pub mod workset_ir;
#[cfg(feature = "render_research")]
pub fn research_workset_configure(enabled: bool, audit: bool) {
    workset::configure(enabled, audit);
}

static CACHE: Mutex<Option<Arc<cache::RenderCache>>> = Mutex::new(None);
#[cfg(feature = "render_research")]
thread_local! { static WARM_OPTIONS: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((true, false)) }; }
#[cfg(feature = "render_research")]
pub fn research_loading_warm(enabled: bool, force_unavailable: bool) {
    WARM_OPTIONS.with(|value| value.set((enabled, force_unavailable)));
}
fn warm_options() -> (bool, bool) {
    #[cfg(feature = "render_research")]
    {
        WARM_OPTIONS.with(|value| value.get())
    }
    #[cfg(not(feature = "render_research"))]
    {
        (true, false)
    }
}
/// Every producer and Settings share this instance. Configuration performs no
/// filesystem access; the host resolves the existing app-cache root.
pub fn configure_cache(root: PathBuf, enabled: bool) -> Result<Arc<cache::RenderCache>> {
    configure_cache_with_paths(root.clone(), vec![root], Vec::new(), Vec::new(), Vec::new(), enabled)
}
pub fn configure_cache_with_paths(
    root: PathBuf,
    clear_roots: Vec<PathBuf>,
    protected: Vec<PathBuf>,
    protected_trees: Vec<PathBuf>,
    anchors: Vec<PathBuf>,
    enabled: bool,
) -> Result<Arc<cache::RenderCache>> {
    let mut slot = CACHE.lock().unwrap();
    if let Some(cache) = slot.as_ref() {
        ensure!(
            cache.configuration_matches(&root, &clear_roots, &protected, &protected_trees, &anchors),
            "Compiler cache paths changed inside the process"
        );
        cache.set_enabled(enabled);
        return Ok(cache.clone());
    }
    let cache = Arc::new(cache::RenderCache::with_clear_roots_and_boundaries(root, clear_roots, protected, protected_trees, anchors, enabled)?);
    *slot = Some(cache.clone());
    Ok(cache)
}
pub fn cache() -> Option<Arc<cache::RenderCache>> {
    CACHE.lock().unwrap().clone()
}
pub fn set_enabled(enabled: bool) {
    if let Some(cache) = cache() {
        cache.set_enabled(enabled);
    }
}
pub fn report(chart: &Chart) -> Value {
    lines::preparation(chart)
}

/// Errors preserve original rendering. No hash, disk access or GPU creation
/// with the independent switch off; visual permission is separately probed.
pub fn prepare(chart: &Chart, res: &Resource, source_bytes: &[u8]) -> Value {
    lines::disable(chart);
    set_enabled(res.config.chart_pre_compile);
    reads::prepare(chart, res, source_bytes);
    visibility::prepare(chart, res, source_bytes);
    if !res.config.chart_pre_compile {
        let result = json!({"requested":false,"enabled":false,"installed":false,"cache_read":false,"cache_write":false});
        lines::record_preparation(chart, result.clone());
        return result;
    }
    let begun = Instant::now();
    let mut cache_read = false;
    let mut cache_write = false;
    let prepare = (|| -> Result<Value> {
        let palette = lines::capability(false, res)?;
        let manager = cache().context("Host has not configured the compiler cache")?;
        let source = Sha256::digest(source_bytes).into();
        let ids = lines::supported_ids(chart);
        let length = res.info.line_length;
        let key = lines::cache_key(source, length, palette, &ids);
        let ticket = manager.begin(&key)?.context("Compiler cache disabled")?;
        let mut cache_hit = false;
        let mut cache_error = Value::Null;
        cache_read = true;
        let cached = match manager.read(&ticket) {
            Ok(Some(bytes)) => match lines::decode(&bytes, source, length, palette, &ids) {
                Ok(mesh) => {
                    cache_hit = true;
                    Some((mesh, bytes.len()))
                }
                Err(error) => {
                    cache_error = json!(format!("{error:#}"));
                    manager.invalidate_blob(&ticket)?;
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                cache_error = json!(format!("{error:#}"));
                manager.invalidate_blob(&ticket)?;
                None
            }
        };
        let mut published = false;
        let (mesh, bytes) = match cached {
            Some(value) => value,
            None => {
                let mesh = lines::compile(source, length, palette, ids.clone())?;
                let blob = lines::encode(&mesh);
                let mesh = lines::decode(&blob, source, length, palette, &ids)?;
                cache_write = true;
                match manager.publish(&ticket, &blob) {
                    Ok(value) => published = value,
                    Err(error) => cache_error = json!(format!("{error:#}")),
                }
                (mesh, blob.len())
            }
        };
        ensure!(manager.is_current(&ticket), "Compiler work invalidated by clear/disable");
        lines::install(mesh, chart, res, lines::CacheLease::new(manager.clone(), ticket))?;
        let (warm, force) = warm_options();
        let loading_warm = if warm {
            lines::warm_loading(chart, force)?
        } else {
            json!({"requested":false})
        };
        Ok(json!({"requested":true,"enabled":true,"installed":true,"cache_hit":cache_hit,"published":published,
            "cache_error":cache_error,"key":key,"bytes":bytes,"root":manager.root(),"gpu_blob_persisted":cache_hit || published,"loading_warm":loading_warm,
            "quality_class":"mild-lossy; low quality required independently of compiler switch",
            "scope":"Actual immutable line GPU blob; original source owns judgement, audio and dynamic pose."}))
    })();
    let mut result = match prepare {
        Ok(report) => report,
        Err(error) => {
            lines::unavailable(chart, format!("{error:#}"));
            json!({"requested":true,"enabled":true,"installed":false,"fallback":format!("{error:#}")})
        }
    };
    result["cache_read"] = json!(cache_read);
    result["cache_write"] = json!(cache_write);
    result["prepare_ms"] = json!(begun.elapsed().as_secs_f64() * 1000.);
    lines::record_preparation(chart, result.clone());
    result
}
