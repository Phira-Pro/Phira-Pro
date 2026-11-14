//! Best-effort Linux scheduling priority requests.
//!
//! Every request here is optional: the kernel checks the process limits and
//! capabilities and the module logs the resolved state once. Nothing hard-fails
//! and no rendering behavior depends on it.
//!
//! - `PR_SET_TIMERSLACK` tightens wakeup/ sleep precision (always unprivileged).
//! - `SCHED_RR` and negative nice values are applied only when `RLIMIT_RTPRIO`
//!   / `RLIMIT_NICE` allow them (for example when launched through Gamemode or
//!   a privileged systemd unit).
//! - i915 DRM context priority is raised through `I915_CONTEXT_PARAM_PRIORITY`
//!   on the contexts of this process's own render node. Raising the priority
//!   above zero requires `CAP_SYS_NICE`; drivers without scheduler priority
//!   support (Haswell and older) reject every value with `ENODEV`.
//!
//! `PHIRA_DRM_PRIORITY` overrides the requested context priority (0 disables,
//! 1..=1023 is the i915 user range). The default is a conservative `1`, so the
//! request only matters when the capability has already been granted.

use std::fs;
use std::io;
use tracing::{debug, info, warn};

// DRM ioctl numbers from include/uapi/drm/i915_drm.h (x86_64), kept local so
// this module does not need libdrm headers at build time.
const I915_GETPARAM: libc::c_ulong = 0xc010_6446;
const I915_GEM_CONTEXT_GETPARAM: libc::c_ulong = 0xc018_6474;
const I915_GEM_CONTEXT_SETPARAM: libc::c_ulong = 0xc018_6475;

const I915_PARAM_HAS_SCHEDULER: libc::c_int = 41;
const I915_SCHEDULER_CAP_PRIORITY: libc::c_int = 1 << 1;
const I915_CONTEXT_PARAM_PRIORITY: u64 = 0x6;
const I915_CONTEXT_MAX_USER_PRIORITY: i64 = 1023;
/// i915 allocates context ids from a per-file idr; little is needed in practice.
const I915_CONTEXT_SCAN: u32 = 1024;

#[repr(C)]
struct I915Getparam {
    param: libc::c_int,
    value: *mut libc::c_int,
}

#[repr(C)]
struct I915GemContextParam {
    ctx_id: u32,
    size: u32,
    param: u64,
    value: u64,
}

pub fn install() {
    apply_timer_slack();
    apply_cpu_priority();
    apply_gpu_priority();
}

fn apply_timer_slack() {
    // 1 ns keeps timerfd/nanosleep wakeups precise, which complements the
    // frame pacer's short spins without touching global timer resolution.
    let rc = unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, 1 as libc::c_ulong, 0, 0, 0) };
    if rc == 0 {
        debug!("timer slack set to 1 ns");
    } else {
        debug!(error = %io::Error::last_os_error(), "failed to set timer slack");
    }
}

fn apply_cpu_priority() {
    let mut realtime_limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    let realtime_limit = if unsafe { libc::getrlimit(libc::RLIMIT_RTPRIO, &mut realtime_limit) } == 0 {
        realtime_limit.rlim_cur
    } else {
        0
    };
    if realtime_limit > 0 {
        // SCHED_RR's lowest priority is 1; staying at the bottom of the
        // real-time range keeps the render thread ahead of CFS work without
        // starving the compositor or audio if they stay on other cores.
        let priority = 1;
        let param = libc::sched_param { sched_priority: priority };
        // PID 0 changes the calling thread, which is the update/render thread.
        if unsafe { libc::sched_setscheduler(0, libc::SCHED_RR, &param) } == 0 {
            info!(priority, "linux: real-time scheduling for the render thread");
        } else {
            debug!(error = %io::Error::last_os_error(), "linux: SCHED_RR denied");
        }
    } else {
        debug!("linux: SCHED_RR unavailable (RLIMIT_RTPRIO=0)");
    }

    let mut nice_limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    let nice_limit = if unsafe { libc::getrlimit(libc::RLIMIT_NICE, &mut nice_limit) } == 0 {
        nice_limit.rlim_cur
    } else {
        0
    };
    // The lowest nice value RLIMIT_NICE permits is 20 - rlim_cur; the
    // infinity value means the full -20..19 range.
    let floor = if nice_limit == libc::RLIM_INFINITY {
        -20
    } else {
        20 - nice_limit as i32
    };
    let target = floor.min(0).max(-5);
    if target < 0 {
        if let Err(err) = lower_nice(target) {
            debug!(target, error = %err, "linux: negative nice denied");
        } else {
            info!(nice = target, "linux: lowered process nice");
        }
    } else {
        debug!("linux: negative nice unavailable (RLIMIT_NICE=0)");
    }
}

fn lower_nice(nice: i32) -> io::Result<()> {
    let Ok(tasks) = fs::read_dir("/proc/self/task") else {
        return Err(io::Error::last_os_error());
    };
    let mut applied = 0;
    let mut last_error = None;
    for task in tasks.flatten() {
        let Some(tid) = task.file_name().to_str().and_then(|name| name.parse::<libc::id_t>().ok()) else {
            continue;
        };
        // Linux nice values are per-thread; cover the threads that already
        // exist. Threads created later inherit the creator's nice value.
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, nice) } == 0 {
            applied += 1;
        } else {
            last_error = Some(io::Error::last_os_error());
        }
    }
    if applied == 0 {
        return Err(last_error.unwrap_or_else(|| io::Error::from_raw_os_error(libc::EACCES)));
    }
    Ok(())
}

fn parse_priority(value: Option<&str>) -> Option<u64> {
    let requested = match value {
        Some(value) => value.trim().parse::<i64>().unwrap_or(1),
        None => 1,
    };
    if requested <= 0 {
        None
    } else {
        Some(requested.min(I915_CONTEXT_MAX_USER_PRIORITY) as u64)
    }
}

fn requested_gpu_priority() -> Option<u64> {
    parse_priority(std::env::var("PHIRA_DRM_PRIORITY").ok().as_deref())
}

fn apply_gpu_priority() {
    let Some(requested) = requested_gpu_priority() else {
        debug!("drm context priority disabled by PHIRA_DRM_PRIORITY");
        return;
    };

    let Ok(entries) = fs::read_dir("/proc/self/fd") else {
        return;
    };
    let mut seen: Vec<(String, u64)> = Vec::new();
    for entry in entries.flatten() {
        let Some(fd) = entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()) else {
            continue;
        };
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        if !target.starts_with("/dev/dri/renderD") {
            continue;
        }
        let Ok(info) = fs::read_to_string(format!("/proc/self/fdinfo/{fd}")) else {
            continue;
        };
        let mut driver = None;
        let mut client_id = None;
        for line in info.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim() {
                "drm-driver" => driver = Some(value.trim().to_owned()),
                "drm-client-id" => client_id = value.trim().parse().ok(),
                _ => {}
            }
        }
        let driver = driver.unwrap_or_else(|| "unknown".to_owned());
        let client = client_id.unwrap_or(fd as u64);
        // Mesa dups the render node; only touch each DRM client once.
        if seen
            .iter()
            .any(|(seen_driver, seen_client)| *seen_driver == driver && *seen_client == client)
        {
            continue;
        }
        seen.push((driver.clone(), client));

        if driver == "i915" {
            apply_i915_priority(fd, requested);
        } else {
            debug!(driver = %driver, "drm context priority not implemented for this driver");
        }
    }
}

fn apply_i915_priority(fd: i32, priority: u64) {
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        debug!(error = %io::Error::last_os_error(), "failed to dup render node");
        return;
    }

    let mut caps: libc::c_int = 0;
    let mut getparam = I915Getparam {
        param: I915_PARAM_HAS_SCHEDULER,
        value: &mut caps,
    };
    let caps_ok = unsafe { libc::ioctl(dup, I915_GETPARAM, &mut getparam) } == 0;
    if !caps_ok {
        debug!(
            error = %io::Error::last_os_error(),
            "i915 scheduler capability query failed"
        );
    } else if caps & I915_SCHEDULER_CAP_PRIORITY == 0 {
        debug!(caps = format_args!("{caps:#x}"), "i915 reports no context-priority support; DRM priority is a no-op");
    } else {
        raise_i915_context_priorities(dup, priority);
    }

    unsafe {
        libc::close(dup);
    }
}

fn raise_i915_context_priorities(fd: i32, priority: u64) {
    let mut raised = 0u32;
    for ctx_id in 0..I915_CONTEXT_SCAN {
        let mut param = I915GemContextParam {
            ctx_id,
            size: 0,
            param: I915_CONTEXT_PARAM_PRIORITY,
            value: 0,
        };
        if unsafe { libc::ioctl(fd, I915_GEM_CONTEXT_GETPARAM, &mut param) } != 0 {
            continue;
        }
        let current = param.value as i64;
        if current >= priority as i64 {
            continue;
        }
        param.value = priority;
        if unsafe { libc::ioctl(fd, I915_GEM_CONTEXT_SETPARAM, &param) } == 0 {
            raised += 1;
            continue;
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EPERM) => {
                warn!(priority, "drm context priority requires CAP_SYS_NICE; leaving contexts unchanged");
                return;
            }
            Some(libc::ENODEV) | Some(libc::EINVAL) => {
                debug!(error = %error, "i915 rejected context priority");
                return;
            }
            _ => {
                debug!(error = %error, ctx_id, "i915 context priority request failed");
            }
        }
    }
    if raised > 0 {
        info!(contexts = raised, priority, "raised DRM context priority");
    } else {
        debug!(priority, "no DRM contexts needed a priority change");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_priority_respects_the_user_range() {
        assert_eq!(parse_priority(Some("2048")), Some(1023));
        assert_eq!(parse_priority(Some("0")), None);
        assert_eq!(parse_priority(Some("-4")), None);
        assert_eq!(parse_priority(Some("not-a-number")), Some(1));
        assert_eq!(parse_priority(None), Some(1));
    }
}
