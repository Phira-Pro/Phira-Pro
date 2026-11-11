//! Prototype cache lifecycle. Intended root: phira::dir::cache()/render-ir.
//! A single app-owned instance must coordinate Settings clear and every writer.
use crate::cache_paths;
use anyhow::{ensure, Result};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
// Corrupt cache files must not dictate an unbounded allocation before decode.
// Large valid charts can use the original path when this cache budget is exceeded.
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct Ticket {
    key: String,
    epoch: u64,
}
struct State {
    enabled: bool,
    epoch: u64,
}
pub struct RenderCache {
    app_cache_root: PathBuf,
    clear_roots: Vec<PathBuf>,
    protected: Vec<PathBuf>,
    protected_trees: Vec<PathBuf>,
    trusted_anchors: Vec<PathBuf>,
    state: Mutex<State>,
    next_temp: AtomicU64,
}

impl RenderCache {
    /// Constructing a disabled cache performs no filesystem writes.
    pub fn new(app_cache_root: PathBuf, enabled: bool) -> Result<Self> {
        Self::with_clear_roots(app_cache_root.clone(), vec![app_cache_root], enabled)
    }
    pub fn with_clear_roots(app_cache_root: PathBuf, clear_roots: Vec<PathBuf>, enabled: bool) -> Result<Self> {
        Self::with_clear_roots_and_guards(app_cache_root, clear_roots, Vec::new(), Vec::new(), enabled)
    }
    pub fn with_clear_roots_and_guards(
        app_cache_root: PathBuf,
        clear_roots: Vec<PathBuf>,
        protected: Vec<PathBuf>,
        trusted_anchors: Vec<PathBuf>,
        enabled: bool,
    ) -> Result<Self> {
        Self::with_clear_roots_and_boundaries(app_cache_root, clear_roots, protected, Vec::new(), trusted_anchors, enabled)
    }
    pub fn with_clear_roots_and_boundaries(
        app_cache_root: PathBuf,
        clear_roots: Vec<PathBuf>,
        protected: Vec<PathBuf>,
        protected_trees: Vec<PathBuf>,
        trusted_anchors: Vec<PathBuf>,
        enabled: bool,
    ) -> Result<Self> {
        cache_paths::check_configuration(&app_cache_root, &clear_roots, &protected, &protected_trees, &trusted_anchors)?;
        Ok(Self {
            app_cache_root,
            clear_roots,
            protected,
            protected_trees,
            trusted_anchors,
            state: Mutex::new(State { enabled, epoch: 0 }),
            next_temp: AtomicU64::new(0),
        })
    }
    pub fn root(&self) -> &Path {
        &self.app_cache_root
    }
    /// Exact configured values, with no filesystem lookup or path rewriting.
    pub fn configuration_matches(
        &self,
        root: &Path,
        clear_roots: &[PathBuf],
        protected: &[PathBuf],
        protected_trees: &[PathBuf],
        trusted_anchors: &[PathBuf],
    ) -> bool {
        self.app_cache_root == root
            && self.clear_roots == clear_roots
            && self.protected == protected
            && self.protected_trees == protected_trees
            && self.trusted_anchors == trusted_anchors
    }
    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled {
            state.epoch = state.epoch.wrapping_add(1);
            state.enabled = enabled;
        }
    }
    pub fn begin(&self, key: &str) -> Result<Option<Ticket>> {
        ensure!(key.len() == 64 && key.bytes().all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase()), "cache key must be lowercase SHA-256");
        let state = self.state.lock().unwrap();
        Ok(state.enabled.then(|| Ticket {
            key: key.to_owned(),
            epoch: state.epoch,
        }))
    }
    fn path(&self, ticket: &Ticket) -> PathBuf {
        self.app_cache_root.join("render-ir").join(format!("{}.ir", ticket.key))
    }
    pub fn is_current(&self, ticket: &Ticket) -> bool {
        let state = self.state.lock().unwrap();
        state.enabled && state.epoch == ticket.epoch
    }
    pub fn read(&self, ticket: &Ticket) -> Result<Option<Vec<u8>>> {
        self.read_bounded(ticket, MAX_CACHE_BYTES as usize)
    }
    /// A format can impose a smaller allocation budget before reading bytes.
    /// File growth is detected in a stack buffer before extending the Vec;
    /// read_to_end would be allowed to grow its allocation beyond that limit.
    pub fn read_bounded(&self, ticket: &Ticket, max_bytes: usize) -> Result<Option<Vec<u8>>> {
        let state = self.state.lock().unwrap();
        if !state.enabled || state.epoch != ticket.epoch {
            return Ok(None);
        }
        let file = match fs::File::open(self.path(ticket)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let limit = max_bytes.min(MAX_CACHE_BYTES as usize);
        let length = usize::try_from(file.metadata()?.len())?;
        ensure!(length <= limit, "IR cache exceeds allocation budget");
        let mut data = Vec::new();
        data.try_reserve_exact(length)?;
        let mut reader = file.take(limit as u64 + 1);
        let mut chunk = [0u8; 8192];
        loop {
            let count = match reader.read(&mut chunk) {
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            if count == 0 {
                break;
            }
            ensure!(data.len().checked_add(count).is_some_and(|length| length <= limit), "IR cache grew beyond allocation budget");
            data.try_reserve_exact(count)?;
            data.extend_from_slice(&chunk[..count]);
        }
        Ok(Some(data))
    }
    /// Compilation happens outside this lock. A cleared/disabled generation
    /// cannot create directories or republish a finished obsolete compilation.
    pub fn publish(&self, ticket: &Ticket, data: &[u8]) -> Result<bool> {
        let state = self.state.lock().unwrap();
        if !state.enabled || state.epoch != ticket.epoch {
            return Ok(false);
        }
        ensure!(data.len() as u64 <= MAX_CACHE_BYTES, "IR cache exceeds publication budget");
        let path = self.path(ticket);
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory)?;
        let temporary = directory.join(format!("{}.{}.{}.tmp", ticket.key, std::process::id(), self.next_temp.fetch_add(1, Ordering::Relaxed)));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(data)?;
            file.sync_all()?;
            drop(file);
            // All app writers share this mutex. Existing valid blobs can be reused.
            if path.exists() {
                fs::remove_file(&temporary)?;
            } else {
                fs::rename(&temporary, &path)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(true)
    }
    pub fn invalidate_blob(&self, ticket: &Ticket) -> Result<()> {
        let state = self.state.lock().unwrap();
        if state.epoch != ticket.epoch {
            return Ok(());
        }
        match fs::remove_file(self.path(ticket)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    /// Invalidate tickets first, including when disabled or when validation or
    /// deletion fails. All explicitly configured roots are then checked before
    /// any deletion, while the writer mutex remains held throughout the clear.
    pub fn clear_all(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.epoch = state.epoch.wrapping_add(1);
        cache_paths::clear_paths_with_boundaries(&self.clear_roots, &self.protected, &self.protected_trees, &self.trusted_anchors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    #[test]
    fn oversized_file_is_rejected_before_reading_the_payload() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = RenderCache::new(temporary.path().join("cache"), true).unwrap();
        let ticket = cache.begin(KEY).unwrap().unwrap();
        let path = cache.path(&ticket);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap().set_len(MAX_CACHE_BYTES + 1).unwrap();
        assert!(cache.read(&ticket).unwrap_err().to_string().contains("allocation budget"));
        cache.invalidate_blob(&ticket).unwrap();
        assert!(cache.read(&ticket).unwrap().is_none());
    }
    #[test]
    fn clear_removes_old_temp_and_other_cache_but_preserves_source() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("app");
        fs::create_dir(&app).unwrap();
        let source = app.join("chart.zip");
        fs::write(&source, b"source").unwrap();
        let cache = RenderCache::new(app.join("cache"), true).unwrap();
        let ticket = cache.begin(KEY).unwrap().unwrap();
        cache.publish(&ticket, b"compiled").unwrap();
        fs::write(cache.root().join("render-ir/old.tmp"), b"partial").unwrap();
        fs::write(cache.root().join("http-cache"), b"download").unwrap();
        cache.clear_all().unwrap();
        assert!(!cache.root().exists());
        assert_eq!(fs::read(source).unwrap(), b"source");
        assert!(!cache.publish(&ticket, b"obsolete").unwrap());
        assert!(!cache.root().exists());
        let current = cache.begin(KEY).unwrap().unwrap();
        assert!(cache.publish(&current, b"new").unwrap());
        assert_eq!(cache.read(&current).unwrap().unwrap(), b"new");
    }
    #[test]
    fn clear_wins_against_compilation_finishing_later() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = Arc::new(RenderCache::new(temporary.path().join("cache"), true).unwrap());
        let ticket = cache.begin(KEY).unwrap().unwrap();
        let other = cache.clone();
        let (go, wait) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            other.publish(&ticket, b"compiled after clear").unwrap()
        });
        cache.clear_all().unwrap();
        go.send(()).unwrap();
        assert!(!worker.join().unwrap());
        assert!(!cache.root().exists());
    }
    #[test]
    fn independent_switch_disables_read_and_invalidates_pending_work() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let cache = RenderCache::new(root.clone(), false).unwrap();
        assert!(cache.begin(KEY).unwrap().is_none());
        assert!(!root.exists());
        cache.set_enabled(true);
        let ticket = cache.begin(KEY).unwrap().unwrap();
        cache.publish(&ticket, b"valid").unwrap();
        cache.set_enabled(false);
        assert!(!cache.is_current(&ticket));
        assert!(cache.read(&ticket).unwrap().is_none());
        assert!(!cache.publish(&ticket, b"pending").unwrap());
        assert!(cache.begin(KEY).unwrap().is_none());
        cache.set_enabled(true);
        assert!(!cache.is_current(&ticket));
        assert!(!cache.publish(&ticket, b"old").unwrap());
        assert!(cache.read(&cache.begin(KEY).unwrap().unwrap()).unwrap().is_some());
    }
    #[test]
    fn disabled_clear_removes_every_root_and_preserves_source_and_fonts() {
        let temporary = tempfile::tempdir().unwrap();
        let app = temporary.path().join("app");
        let data = app.join("data");
        let roots = vec![app.join("cache"), data.join("legacy-images"), temporary.path().join("native-cache")];
        let protected = vec![app, data.clone()];
        let sources = vec![data.join("charts"), data.join("font.ttf"), data.join("bold.ttf"), data.join("data.json")];
        let cache =
            RenderCache::with_clear_roots_and_boundaries(roots[0].clone(), roots.clone(), protected.clone(), sources.clone(), Vec::new(), false)
                .unwrap();
        assert!(roots.iter().all(|root| !root.exists()));
        assert!(cache.configuration_matches(&roots[0], &roots, &protected, &sources, &[]));
        fs::create_dir_all(data.join("charts")).unwrap();
        fs::write(data.join("charts/source.zip"), b"source").unwrap();
        for name in ["font.ttf", "bold.ttf", "data.json"] {
            fs::write(data.join(name), name.as_bytes()).unwrap();
        }
        for root in &roots {
            fs::create_dir_all(root).unwrap();
            fs::write(root.join("cache.bin"), b"cache").unwrap();
        }
        cache.clear_all().unwrap();
        assert!(roots.iter().all(|root| !root.exists()));
        assert_eq!(fs::read(data.join("charts/source.zip")).unwrap(), b"source");
        for name in ["font.ttf", "bold.ttf", "data.json"] {
            assert_eq!(fs::read(data.join(name)).unwrap(), name.as_bytes());
        }
        assert!(cache.begin(KEY).unwrap().is_none());
    }
    #[test]
    fn failed_root_validation_still_invalidates_ticket_and_does_not_partially_clear() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let other = temporary.path().join("other-cache");
        let cache = RenderCache::with_clear_roots(root.clone(), vec![root.clone(), other.clone()], true).unwrap();
        let ticket = cache.begin(KEY).unwrap().unwrap();
        cache.publish(&ticket, b"keep until successful clear").unwrap();
        // This cache directory changed into a file after construction.
        fs::write(&other, b"not a directory").unwrap();
        assert!(cache.clear_all().is_err());
        assert!(!cache.is_current(&ticket));
        assert!(cache.read(&ticket).unwrap().is_none());
        assert!(!cache.publish(&ticket, b"late obsolete compilation").unwrap());
        assert_eq!(fs::read(cache.path(&ticket)).unwrap(), b"keep until successful clear");
        assert_eq!(fs::read(&other).unwrap(), b"not a directory");
        fs::remove_file(other).unwrap();
        cache.clear_all().unwrap();
        assert!(!root.exists());
    }
    #[test]
    fn constructor_rejects_uncovered_write_root_without_creating_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("write-cache");
        assert!(RenderCache::with_clear_roots(root.clone(), vec![temporary.path().join("other-cache")], true).is_err());
        assert!(!root.exists());
    }

    #[test]
    fn workset_cache_client_limit_preserves_legacy_reads_and_epoch_rejection() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = RenderCache::new(temporary.path().join("cache"), true).unwrap();
        let ticket = cache.begin(KEY).unwrap().unwrap();
        let bytes = [3u8; 65];
        assert!(cache.publish(&ticket, &bytes).unwrap());
        assert!(cache.read_bounded(&ticket, 64).is_err());
        assert!(cache.read_bounded(&ticket, 0).is_err());
        assert_eq!(cache.read_bounded(&ticket, 65).unwrap().unwrap(), bytes);
        assert_eq!(cache.read(&ticket).unwrap().unwrap(), bytes);
        cache.clear_all().unwrap();
        assert!(cache.read_bounded(&ticket, 65).unwrap().is_none());
        assert!(!cache.is_current(&ticket));
    }
}
