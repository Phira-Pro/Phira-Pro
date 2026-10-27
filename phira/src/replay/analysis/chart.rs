//! Chart identity resolution on the analysis worker, without a frame executor.
use super::Control;
use crate::replay::{self, Replay};
use anyhow::Result;
use prpr::{core::Chart, scene::GameScene};
use std::{
    collections::HashSet,
    future::Future,
    task::{Context, Poll, Waker},
    time::Duration,
};

/// Gameplay loading uses a silent waker and is polled each frame. A blocking
/// executor waiting for a wake can sleep forever after that work completes.
/// Drive every pending future here on a bounded, cancellable worker cadence.
fn wait<T>(future: impl Future<Output = T>, control: &Control) -> Result<T> {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        control.check()?;
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            control.check()?;
            return Ok(value);
        }
        std::thread::park_timeout(Duration::from_millis(2));
    }
}

pub struct Bound {
    pub chart: Option<Chart>,
    pub local: Option<String>,
    pub verified: bool,
    pub audio: String,
    pub status: String,
    pub fingerprint: Option<String>,
}

pub fn find_chart(replay: &Replay, preferred: Option<&str>, candidates: &[String], control: &Control) -> Result<Bound> {
    find_chart_with(replay, preferred, candidates, control, crate::scene::fs_from_path)
}

fn find_chart_with(
    replay: &Replay,
    preferred: Option<&str>,
    candidates: &[String],
    control: &Control,
    mut open: impl FnMut(&str) -> Result<Box<dyn prpr::fs::FileSystem + Send + Sync>>,
) -> Result<Bound> {
    let original = match &replay.chart {
        Some(replay::ChartRef::Local(p)) => Some(p.clone()),
        Some(replay::ChartRef::Id(id)) => Some(format!("download/{id}")),
        None => None,
    };
    let mut seen = HashSet::new();
    let paths = preferred
        .into_iter()
        .map(str::to_owned)
        .chain(original)
        .chain(candidates.iter().cloned())
        .filter(|p| seen.insert(p.clone()))
        .collect::<Vec<_>>();
    let mut status = "未找到录制时谱面；仅分析输入特征".to_owned();
    let total = paths.len().max(1) * 5;
    for (index, path) in paths.into_iter().enumerate() {
        control.progress(index * 5, total)?;
        control.describe(format!("检查第 {} 个候选谱面", index + 1))?;
        control.check()?;
        let Ok(mut fs) = open(&path) else { continue };
        control.describe("读取谱面配置")?;
        let Ok(info) = wait(prpr::fs::load_info(fs.as_mut()), control)? else {
            continue;
        };
        control.progress(index * 5 + 1, total)?;
        control.describe("核对谱面内容")?;
        let Ok(fingerprint) = wait(replay::library::fingerprint(fs.as_mut(), &info), control)? else {
            continue;
        };
        let verified = if let Some(expected) = &replay.meta.fingerprint {
            if expected != &fingerprint {
                if preferred == Some(path.as_str()) {
                    status = "关联谱面内容不一致；已禁用依赖谱面的检测".into();
                }
                continue;
            }
            true
        } else {
            // No name-based fallback. Historical identity is useful for display,
            // but never treated as content verification.
            let identity = preferred == Some(path.as_str())
                || match &replay.chart {
                    Some(replay::ChartRef::Local(p)) => p == &path,
                    Some(replay::ChartRef::Id(id)) => info.id == Some(*id),
                    None => false,
                };
            if !identity {
                continue;
            }
            if replay.chart_updated.is_some() && info.chart_updated.map(|t| t.timestamp_millis()) != replay.chart_updated {
                continue;
            }
            false
        };
        control.progress(index * 5 + 2, total)?;
        control.describe("核对录制音频")?;
        let audio = match &replay.meta.audio_fingerprint {
            Some(expected) => match wait(replay::library::audio_fingerprint(fs.as_mut(), &info), control)? {
                Ok(actual) if &actual == expected => "一致",
                Ok(_) => "不一致（音频相关核对停用）",
                Err(_) => "无法读取",
            },
            None => "未记录",
        }
        .to_owned();
        control.check()?;
        control.progress(index * 5 + 3, total)?;
        control.describe("解析音符与判定线")?;
        match wait(GameScene::load_chart_for_analysis(fs.as_mut(), &info), control)? {
            Ok((chart, _, _)) => {
                control.progress(total, total)?;
                return Ok(Bound {
                    chart: Some(chart),
                    local: Some(path),
                    verified,
                    audio,
                    status: if verified {
                        "谱面内容校验一致"
                    } else {
                        "谱面关联未校验；计时与几何仅供观察"
                    }
                    .into(),
                    fingerprint: Some(fingerprint),
                });
            }
            Err(e) => {
                status = format!("谱面解析失败：{e}；仅分析输入特征");
                break;
            }
        }
    }
    control.progress(total, total)?;
    Ok(Bound {
        chart: None,
        local: None,
        verified: false,
        audio: "未核对".into(),
        status,
        fingerprint: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        io::Write,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        task::Wake,
    };

    #[test]
    fn silent_loading_future_completes_without_a_frame_loop() {
        struct Flag(AtomicBool);
        impl Wake for Flag {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut future = Box::pin(prpr::loading_work::run(move || {
            rx.recv().unwrap();
            Ok(73)
        }));
        let flag = Arc::new(Flag(AtomicBool::new(false)));
        let waker = Waker::from(flag.clone());
        assert!(future.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!flag.0.load(Ordering::Relaxed), "loading intentionally suppresses waking the caller");
        assert_eq!(wait(future, &Control::default()).unwrap().unwrap(), 73);
    }

    #[test]
    fn cancellation_interrupts_a_future_that_never_wakes() {
        let control = Control::default();
        let cancel = control.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancel.cancel();
        });
        let start = std::time::Instant::now();
        let error = wait(std::future::pending::<()>(), &control).unwrap_err();
        worker.join().unwrap();
        assert!(error.to_string().contains("取消"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    fn fixture() -> (tempfile::TempDir, Replay) {
        let dir = tempfile::tempdir().unwrap();
        let bytes = br#"{"formatVersion":3,"offset":0,"judgeLineList":[]}"#;
        let extra = b"{}";
        let music = b"audio identity fixture";
        std::fs::write(dir.path().join("chart.json"), bytes).unwrap();
        std::fs::write(dir.path().join("extra.json"), extra).unwrap();
        std::fs::write(dir.path().join("music.ogg"), music).unwrap();
        let info = prpr::info::ChartInfo {
            chart: "chart.json".into(),
            music: "music.ogg".into(),
            ..Default::default()
        };
        std::fs::write(dir.path().join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
        let mut hash = Sha256::new();
        hash.update(b"PhiraReplayChart1\0");
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        hash.update((extra.len() as u64).to_le_bytes());
        hash.update(extra);
        hash.update(info.offset.to_le_bytes());
        let replay = Replay {
            meta: replay::Metadata {
                fingerprint: Some(format!("{:x}", hash.finalize())),
                audio_fingerprint: Some(format!("{:x}", Sha256::digest(music))),
                ..Default::default()
            },
            ..Default::default()
        };
        (dir, replay)
    }

    #[test]
    fn real_directory_and_zip_pgr_validation_finish_and_reject_content_changes() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();
        let (dir, replay) = fixture();
        let archive = dir.path().join("fixture.pez");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            zip.add_directory("fixture/", zip::write::SimpleFileOptions::default()).unwrap();
            for name in ["info.yml", "chart.json", "extra.json", "music.ogg"] {
                zip.start_file(format!("fixture/{name}"), zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&std::fs::read(dir.path().join(name)).unwrap()).unwrap();
            }
            zip.finish().unwrap();
        }
        for path in [dir.path(), archive.as_path()] {
            let control = Control::default();
            control.set_stage(1).unwrap();
            let bound = find_chart_with(&replay, Some("fixture"), &[], &control, |_| prpr::fs::fs_from_file(path)).unwrap();
            assert!(bound.verified && bound.chart.is_some());
            assert_eq!(bound.audio, "一致");
            assert!((control.fraction() - 2. / 6.).abs() < 1e-6);
        }
        std::fs::write(dir.path().join("chart.json"), b"changed").unwrap();
        let bound = find_chart_with(&replay, Some("fixture"), &[], &Control::default(), |_| prpr::fs::fs_from_file(dir.path())).unwrap();
        assert!(!bound.verified && bound.chart.is_none());
        assert!(bound.status.contains("不一致"));
    }
}
