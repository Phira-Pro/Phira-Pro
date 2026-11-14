//! Linux desktop GPU memory telemetry and low-memory guard.
//!
//! On integrated GPUs "VRAM" is system memory: the kernel backs GEM/TTM
//! buffers with shmem pages that the reclaim path can swap out. The per-client
//! accounting is exposed through `/proc/self/fdinfo/<fd>` for every DRM render
//! node the process holds (`drm-total-*`, `drm-resident-*`, ... for drivers
//! using the region format, `drm-memory-*` for the legacy format).
//!
//! This module samples that accounting together with `/proc/meminfo`, emits a
//! compact baseline, and asks re-creatable GPU caches to release memory when
//! the system is critically short. Nothing here changes rendering: only
//! caches are dropped, and they are rebuilt from identical source data on the
//! next frame that needs them.

use once_cell::sync::Lazy;
use std::fs;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const LOG_INTERVAL: Duration = Duration::from_secs(60);
const TRIM_COOLDOWN: Duration = Duration::from_secs(30);

/// Below this much available RAM the system is considered under pressure.
const LOW_AVAILABLE_KIB: u64 = 512 * 1024;
/// Below this much available RAM the guard releases re-creatable GPU caches.
const CRITICAL_AVAILABLE_KIB: u64 = 256 * 1024;
/// Swap exhaustion only matters when available RAM is also low.
const CRITICAL_SWAP_FREE_KIB: u64 = 64 * 1024;

/// Set when a cache owner should release memory on its next GL-thread tick.
static TRIM_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static MONITOR_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GpuMemory {
    /// Sum of `drm-total-*` (or legacy `drm-memory-*`) values, in KiB.
    pub total_kib: u64,
    pub shared_kib: u64,
    pub resident_kib: u64,
    pub active_kib: u64,
    pub purgeable_kib: u64,
    /// Number of distinct DRM clients (dup'ed fds share one client id).
    pub clients: u32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemMemory {
    pub mem_total_kib: u64,
    pub mem_available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
    pub rss_kib: u64,
    pub vm_swap_kib: u64,
}

impl SystemMemory {
    /// `None` when the kernel does not expose `MemAvailable`.
    fn pressure(&self) -> Option<Pressure> {
        if self.mem_available_kib == 0 {
            return None;
        }
        // Swap being full on its own is not pressure: the kernel can reclaim
        // clean file pages. It only matters when available RAM is also low.
        let swap_starved = self.swap_total_kib > 0 && self.swap_free_kib < CRITICAL_SWAP_FREE_KIB;
        if self.mem_available_kib < CRITICAL_AVAILABLE_KIB || (swap_starved && self.mem_available_kib < LOW_AVAILABLE_KIB * 2) {
            Some(Pressure::Critical)
        } else if self.mem_available_kib < LOW_AVAILABLE_KIB {
            Some(Pressure::Low)
        } else {
            Some(Pressure::Normal)
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pressure {
    #[default]
    Normal,
    Low,
    Critical,
}

#[derive(Default)]
struct State {
    last_log: Option<Instant>,
    last_trim: Option<Instant>,
    pressure: Pressure,
}

fn mib(kib: u64) -> f64 {
    (kib as f64 / 1024.0 * 10.0).round() / 10.0
}

/// Parse one render-node `fdinfo` file. Returns `(driver, client-id, memory)`.
fn parse_fdinfo(text: &str) -> Option<(String, Option<u64>, GpuMemory)> {
    let mut driver = None;
    let mut client_id = None;
    let mut gpu = GpuMemory::default();
    let mut legacy_total_kib = 0;

    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "drm-driver" {
            driver = Some(value.to_owned());
            continue;
        }
        if key == "drm-client-id" {
            client_id = value.parse().ok();
            continue;
        }
        let Some(amount) = value.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        if key.starts_with("drm-total-") {
            gpu.total_kib += amount;
        } else if key.starts_with("drm-memory-") {
            legacy_total_kib += amount;
        } else if key.starts_with("drm-shared-") {
            gpu.shared_kib += amount;
        } else if key.starts_with("drm-resident-") {
            gpu.resident_kib += amount;
        } else if key.starts_with("drm-active-") {
            gpu.active_kib += amount;
        } else if key.starts_with("drm-purgeable-") {
            gpu.purgeable_kib += amount;
        }
    }

    let driver = driver?;
    // Newer drivers expose region counters; keep the legacy format working too.
    if gpu.total_kib == 0 {
        gpu.total_kib = legacy_total_kib;
    }
    Some((driver, client_id, gpu))
}

fn read_gpu_memory() -> GpuMemory {
    let mut out = GpuMemory::default();
    let mut seen: Vec<(String, u64)> = Vec::new();

    let Ok(entries) = fs::read_dir("/proc/self/fd") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(fd) = entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()) else {
            continue;
        };
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        if !target.to_string_lossy().starts_with("/dev/dri/renderD") {
            continue;
        }
        let Ok(info) = fs::read_to_string(format!("/proc/self/fdinfo/{fd}")) else {
            continue;
        };
        let Some((driver, client_id, gpu)) = parse_fdinfo(&info) else {
            continue;
        };
        // Mesa dups its render node for GLX/EGL; counting each fd would
        // multiply the same client's memory by the number of dups.
        let client = client_id.unwrap_or(fd as u64);
        if seen
            .iter()
            .any(|(seen_driver, seen_client)| *seen_driver == driver && *seen_client == client)
        {
            continue;
        }
        seen.push((driver, client));

        out.total_kib += gpu.total_kib;
        out.shared_kib += gpu.shared_kib;
        out.resident_kib += gpu.resident_kib;
        out.active_kib += gpu.active_kib;
        out.purgeable_kib += gpu.purgeable_kib;
        out.clients += 1;
    }
    out
}

fn read_system_memory() -> SystemMemory {
    let mut memory = SystemMemory::default();
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let Some(amount) = value.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) else {
                continue;
            };
            match key.trim() {
                "MemTotal" => memory.mem_total_kib = amount,
                "MemAvailable" => memory.mem_available_kib = amount,
                "SwapTotal" => memory.swap_total_kib = amount,
                "SwapFree" => memory.swap_free_kib = amount,
                _ => {}
            }
        }
    }
    if let Ok(text) = fs::read_to_string("/proc/self/status") {
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let Some(amount) = value.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) else {
                continue;
            };
            match key.trim() {
                "VmRSS" => memory.rss_kib = amount,
                "VmSwap" => memory.vm_swap_kib = amount,
                _ => {}
            }
        }
    }
    memory
}

/// Log the post-startup baseline once the scene has finished creating its
/// textures, then start the background monitor.
pub fn install() {
    let system = read_system_memory();
    let gpu = read_gpu_memory();
    let pressure = system.pressure().unwrap_or(Pressure::Normal);
    info!(
        gpu_mib = mib(gpu.total_kib),
        gpu_resident_mib = mib(gpu.resident_kib),
        gpu_shared_mib = mib(gpu.shared_kib),
        gpu_purgeable_mib = mib(gpu.purgeable_kib),
        gpu_clients = gpu.clients,
        mem_available_mib = mib(system.mem_available_kib),
        swap_free_mib = mib(system.swap_free_kib),
        rss_mib = mib(system.rss_kib),
        vm_swap_mib = mib(system.vm_swap_kib),
        ?pressure,
        "linux memory baseline"
    );

    {
        let mut state = STATE.lock().unwrap();
        state.pressure = pressure;
        state.last_log = Some(Instant::now());
    }
    // If the process starts while the system is already critical, react on the
    // first submit instead of waiting for a transition that will never come.
    if pressure == Pressure::Critical {
        let mut state = STATE.lock().unwrap();
        state.last_trim = Some(Instant::now());
        drop(state);
        TRIM_GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    if !MONITOR_STARTED.swap(true, Ordering::SeqCst) {
        let _ = std::thread::Builder::new().name("linux-mem-monitor".into()).spawn(monitor_loop);
    }
}

/// Sample system/GPU memory every [`SAMPLE_INTERVAL`] on a dedicated thread.
/// File reads never run on the GL thread; cache owners only load an atomic
/// generation counter on their next draw.
fn monitor_loop() {
    loop {
        std::thread::sleep(SAMPLE_INTERVAL);
        sample();
    }
}

fn sample() {
    let now = Instant::now();
    let mut state = STATE.lock().unwrap();

    let system = read_system_memory();
    let gpu = read_gpu_memory();
    let Some(pressure) = system.pressure() else {
        return;
    };

    let previous = state.pressure;
    if pressure != previous {
        match pressure {
            Pressure::Critical => warn!(
                gpu_mib = mib(gpu.total_kib),
                mem_available_mib = mib(system.mem_available_kib),
                swap_free_mib = mib(system.swap_free_kib),
                "system memory critically low; releasing re-creatable GPU caches"
            ),
            Pressure::Low => info!(
                gpu_mib = mib(gpu.total_kib),
                mem_available_mib = mib(system.mem_available_kib),
                swap_free_mib = mib(system.swap_free_kib),
                "system memory low"
            ),
            Pressure::Normal => info!(
                gpu_mib = mib(gpu.total_kib),
                mem_available_mib = mib(system.mem_available_kib),
                swap_free_mib = mib(system.swap_free_kib),
                "system memory pressure cleared"
            ),
        }
        state.pressure = pressure;
    }

    // Only react to a transition into Critical: trimming again every few
    // seconds would make the atlas regrow and hitch repeatedly without
    // reducing the working set for good.
    if pressure == Pressure::Critical && previous != Pressure::Critical {
        let due = state.last_trim.map_or(true, |last_trim| now.duration_since(last_trim) >= TRIM_COOLDOWN);
        if due {
            state.last_trim = Some(now);
            TRIM_GENERATION.fetch_add(1, Ordering::Relaxed);
        }
    }

    let due = state.last_log.map_or(true, |last_log| now.duration_since(last_log) >= LOG_INTERVAL);
    if due {
        state.last_log = Some(now);
        debug!(
            gpu_mib = mib(gpu.total_kib),
            gpu_resident_mib = mib(gpu.resident_kib),
            gpu_shared_mib = mib(gpu.shared_kib),
            gpu_purgeable_mib = mib(gpu.purgeable_kib),
            gpu_clients = gpu.clients,
            mem_available_mib = mib(system.mem_available_kib),
            swap_free_mib = mib(system.swap_free_kib),
            rss_mib = mib(system.rss_kib),
            vm_swap_mib = mib(system.vm_swap_kib),
            ?pressure,
            "linux memory"
        );
    }
}

/// Generation of the last low-memory cache-release request. Each cache owner
/// remembers the generation it last trimmed at, so every painter reacts once
/// to each request instead of racing for a single consumed flag.
pub fn trim_generation() -> u64 {
    TRIM_GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_region_fdinfo() {
        let text = "\
pos:\t0
drm-driver:\ti915
drm-client-id:\t619
drm-pdev:\t0000:00:02.0
drm-total-system0:\t152608 KiB
drm-shared-system0:\t7680 KiB
drm-active-system0:\t0 KiB
drm-resident-system0:\t152608 KiB
drm-purgeable-system0:\t1424 KiB
drm-total-stolen-system0:\t0 KiB
";
        let (driver, client, gpu) = parse_fdinfo(text).unwrap();
        assert_eq!(driver, "i915");
        assert_eq!(client, Some(619));
        assert_eq!(gpu.total_kib, 152608);
        assert_eq!(gpu.shared_kib, 7680);
        assert_eq!(gpu.resident_kib, 152608);
        assert_eq!(gpu.purgeable_kib, 1424);
    }

    #[test]
    fn parses_legacy_fdinfo() {
        let text = "\
drm-driver:\tamdgpu
drm-client-id:\t7
drm-memory-vram:\t1024 KiB
drm-memory-gtt:\t2048 KiB
drm-memory-cpu:\t512 KiB
";
        let (driver, client, gpu) = parse_fdinfo(text).unwrap();
        assert_eq!(driver, "amdgpu");
        assert_eq!(client, Some(7));
        assert_eq!(gpu.total_kib, 3584);
    }

    #[test]
    fn classifies_pressure_from_available_memory() {
        let mut memory = SystemMemory {
            mem_available_kib: 2 * 1024 * 1024,
            swap_total_kib: 4 * 1024 * 1024,
            swap_free_kib: 2 * 1024 * 1024,
            ..SystemMemory::default()
        };
        assert_eq!(memory.pressure(), Some(Pressure::Normal));
        memory.mem_available_kib = 400 * 1024;
        assert_eq!(memory.pressure(), Some(Pressure::Low));
        memory.mem_available_kib = 128 * 1024;
        assert_eq!(memory.pressure(), Some(Pressure::Critical));
    }

    #[test]
    fn swap_exhaustion_only_matters_with_low_available_memory() {
        let mut memory = SystemMemory {
            mem_available_kib: 2 * 1024 * 1024,
            swap_total_kib: 1024 * 1024,
            swap_free_kib: 32 * 1024,
            ..SystemMemory::default()
        };
        // Plenty of RAM: a full swap is not pressure by itself.
        assert_eq!(memory.pressure(), Some(Pressure::Normal));
        memory.mem_available_kib = 768 * 1024;
        assert_eq!(memory.pressure(), Some(Pressure::Critical));
    }
}
