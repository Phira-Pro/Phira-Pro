use super::*;
use crate::replay::{
    capture::{Anchor, Capture, Input},
    JudgeEvent, TouchEvent,
};

fn chart(count: usize, kind: u32) -> prpr::core::Chart {
    let base: Value = serde_json::from_str(include_str!("../../../../prpr/tests/fixtures/rpe-next/chart.json")).unwrap();
    let mut line = base["judgeLineList"][0].clone();
    line["bpmfactor"] = json!(1);
    let notes: Vec<_> = (0..count)
        .map(|i| {
            let mut n = line["notes"][0].clone();
            n["type"] = json!(kind);
            n["startTime"] = json!([i / 5, i % 5, 5]);
            n["endTime"] = n["startTime"].clone();
            n
        })
        .collect();
    line["notes"] = json!(notes);
    let source=json!({"META":base["META"],"BPMList":[{"bpm":120,"startTime":[0,0,1]}],"judgeLineGroup":["Default"],"judgeLineList":[line],"blockAreaList":[]}).to_string();
    let temp = tempfile::tempdir().unwrap();
    let mut fs = prpr::fs::fs_from_file(temp.path()).unwrap();
    pollster::block_on(prpr::parse::parse_rpe_for_analysis(&source, fs.as_mut(), false, Some("chart.json"), true)).unwrap()
}

fn bound(chart: Option<prpr::core::Chart>) -> features::Bound {
    features::Bound {
        chart,
        local: None,
        verified: true,
        audio: "未记录".into(),
        status: "测试谱面已校验".into(),
        fingerprint: Some("fixture".into()),
    }
}

fn tape(n: usize, zero: bool) -> Replay {
    let mut replay = Replay {
        speed: 1.,
        has_speed: true,
        has_diffs: true,
        source_version: 6,
        settings: Some(crate::judgement_presets::JudgeSettings::default()),
        aspect_ratio: Some(16. / 9.),
        ..Default::default()
    };
    let mut capture = Capture::new();
    for i in 0..n {
        let note = i as f64 * 0.1;
        let diff = if zero { 0. } else { ((i * 37 % 97) as f64 - 48.) * 0.0007 };
        let down = (note + diff).max(0.);
        let up = down + 0.025 + (i % 13) as f64 * 0.001;
        replay.judges.push(JudgeEvent {
            t: down,
            line: 0,
            note: i as u32,
            kind: 0,
            diff: diff as f32,
            head_grade: None,
        });
        for (phase, time) in [("down", down), ("up", up)] {
            let frame = capture.anchors.len() as u64;
            capture.anchors.push(Anchor {
                frame,
                uptime: time + 0.002,
                chart_time: time + 0.002,
                active: true,
                speed: 1.,
                viewport: [0, 0, 1920, 1080],
                screen_height: 1080.,
            });
            capture.inputs.push(Input {
                seq: capture.inputs.len() as u64,
                frame,
                active: true,
                event: prpr::input_capture::Input {
                    source: "touch".into(),
                    phase: phase.into(),
                    id: u64::MAX,
                    event_time: Some(time),
                    observed_time: time + 0.001,
                    x: Some(960.),
                    y: Some(540.),
                },
            });
        }
    }
    replay.frames = capture.anchors.iter().map(|a| a.chart_time).collect();
    replay.touches = capture.playback_touches();
    replay.capture = Some(capture);
    replay
}

#[test]
fn v6_preserves_backend_time_cancellation_and_frame_order() {
    let mut replay = tape(3, false);
    let capture = replay.capture.as_mut().unwrap();
    capture.inputs[1].event.phase = "cancel".into();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("原始输入.ppr");
    crate::replay::save(&replay, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 6);
    let read = crate::replay::load(&path).unwrap();
    assert_eq!(read.source_version, 6);
    assert_eq!(read.capture.as_ref().unwrap().inputs[1].event.phase, "cancel");
    assert_eq!(read.capture.as_ref().unwrap().inputs[0].event.id, u64::MAX);
    let start = read.capture.as_ref().unwrap().inputs[0].event.event_time.unwrap();
    assert!((read.touches[0].t as f64 - start).abs() < 1e-6);
    assert!((read.frames[0] - read.touches[0].t as f64 - 0.002).abs() < 1e-6);
    let mut invalid = read.clone();
    invalid.capture.as_mut().unwrap().inputs[0].frame = 999;
    assert!(crate::replay::bytes(&invalid).is_err());
}

#[test]
fn paused_and_cancelled_contacts_are_censored() {
    let mut replay = tape(3, false);
    let c = replay.capture.as_mut().unwrap();
    c.inputs[1].event.phase = "cancel".into();
    c.anchors[3].active = false;
    c.inputs[3].active = false;
    let data = features::extract(Arc::new(replay), bound(None), &Control::default()).unwrap();
    assert_eq!(data.payload["metrics"]["duration_ms"]["n"], 1);
    assert_eq!(data.payload["quality"]["cancelled_contacts"], 1);
    assert!(data.payload["quality"]["incomplete_contacts"].as_u64().unwrap() >= 1);
}

#[test]
fn synthetic_drag_zeroes_and_miss_placeholders_never_enter_timing_distribution() {
    let mut replay = tape(400, true);
    for e in &mut replay.judges {
        e.diff = 0.;
    }
    let data = features::extract(Arc::new(replay), bound(Some(chart(400, 4))), &Control::default()).unwrap();
    // RPE type 4 is Drag. The engine-generated 0 is not a timing measurement.
    assert_eq!(data.payload["coverage"]["timed_samples"], 0);
    let result = Analyzer::fixture().analyze(Arc::new(data), &Control::default()).unwrap();
    assert_eq!(result["status"], "clear");
    assert_eq!(result["checks"][1]["status"], "unavailable");
    assert!(result["evidence"].as_array().unwrap().is_empty());
    let mut replay = tape(400, true);
    for e in &mut replay.judges {
        e.kind = 3;
        e.diff = 0.25;
    }
    let data = features::extract(Arc::new(replay), bound(Some(chart(400, 1))), &Control::default()).unwrap();
    assert_eq!(data.payload["coverage"]["timed_samples"], 0);
}

#[test]
fn frame_quantization_and_legacy_tapes_do_not_trigger_precise_timing_rules() {
    let mut replay = tape(400, true);
    replay.capture = None;
    replay.source_version = 5;
    let data = features::extract(Arc::new(replay), bound(Some(chart(400, 1))), &Control::default()).unwrap();
    assert_eq!(data.payload["quality"]["timing_candidate_eligible"], false);
    let result = Analyzer::fixture().analyze(Arc::new(data), &Control::default()).unwrap();
    assert!(result["evidence"].as_array().unwrap().is_empty());
    let mut replay = tape(400, true);
    replay.settings = Some(crate::judgement_presets::JudgeSettings::phigros());
    let data = features::extract(Arc::new(replay), bound(Some(chart(400, 1))), &Control::default()).unwrap();
    assert_eq!(data.payload["quality"]["timing_candidate_eligible"], false);
}

#[test]
fn legitimate_hold_head_score_and_tail_are_not_duplicate_or_double_timing_samples() {
    let mut replay = tape(1, false);
    replay.settings.as_mut().unwrap().hold_tail = true;
    replay.judges = vec![
        JudgeEvent {
            t: 0.,
            kind: 5,
            diff: -0.01,
            ..Default::default()
        },
        JudgeEvent {
            t: 0.,
            kind: 0,
            diff: -0.01,
            ..Default::default()
        },
        JudgeEvent {
            t: 0.1,
            kind: 0,
            diff: 0.02,
            ..Default::default()
        },
    ];
    let data = features::extract(Arc::new(replay), bound(Some(chart(1, 2))), &Control::default()).unwrap();
    assert_eq!(data.payload["metrics"]["duplicate_stages"], 0);
    assert_eq!(data.payload["metrics"]["timing_ms"]["n"], 1);
    assert_eq!(data.payload["metrics"]["tail_timing_ms"]["n"], 1);
}

#[test]
fn independent_plugin_handles_normal_and_repeated_timing_as_review_only() {
    for (zero, expected) in [(false, "clear"), (true, "review")] {
        let data = features::extract(Arc::new(tape(400, zero)), bound(Some(chart(400, 1))), &Control::default()).unwrap();
        let result = Analyzer::fixture().analyze(Arc::new(data), &Control::default()).unwrap();
        assert_eq!(result["status"], expected, "{result}");
        assert_eq!(result["calibrated"], false);
    }
}

#[test]
fn cancellation_report_schema_and_paging_are_bounded() {
    let c = Control::default();
    c.cancel();
    assert!(c.check().is_err());
    let data = features::extract(Arc::new(tape(2, false)), bound(None), &Control::default()).unwrap();
    assert!(data.input_page(-1, 256).is_empty());
    assert!(data.input_page(0, 257).is_empty());
    assert!(data.input_page(i64::MAX, 2).is_empty());
    assert_eq!(data.input_page(0, 1)[0]["event"]["id"], u64::MAX.to_string());
    assert!(validate_report(&json!({"status":"cheat","summary":"x","evidence":[]})).is_err());
    assert_eq!(report::badge("clear"), ("正常", [22, 163, 74]));
    assert_eq!(report::badge("review"), ("存疑", [251, 191, 36]));
    assert_eq!(report::badge("limited").0, "数据不足");
    assert_eq!(report::badge("conflict").0, "数据异常");
    assert_eq!(report::badge("cheat"), ("存在作弊", [229, 72, 77]));
    assert!(
        validate_report(&json!({"status":"review","summary":"x","evidence":[{"title":"x","family":"x","detail":"x","start":5,"end":2}]})).is_err()
    );
}

#[test]
fn normalization_provenance_survives_export() {
    let mut r = Replay {
        speed: 1.,
        frames: vec![2., 1.],
        ..Default::default()
    };
    // Simulate historical raw container, bypassing the modern ordered writer.
    let mut bytes = b"PHIRAREC".to_vec();
    bytes.extend(5u32.to_le_bytes());
    bytes.extend(zstd::stream::encode_all(serde_json::to_vec(&r).unwrap().as_slice(), 1).unwrap());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("old.ppr");
    std::fs::write(&path, bytes).unwrap();
    r = crate::replay::load(&path).unwrap();
    assert!(r.repaired_order);
    crate::replay::save(&r, &path).unwrap();
    assert!(crate::replay::load(&path).unwrap().repaired_order);
}

#[test]
fn complete_job_writes_matching_png_and_json_without_modifying_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.ppr");
    crate::replay::save(&tape(100, false), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let out = Job {
        path: path.clone(),
        preferred: None,
        candidates: vec![],
        analyzer: Analyzer::fixture(),
        destination: dir.path().join("reports"),
        control: Control::default(),
    }
    .execute()
    .unwrap();
    assert!(out.png.is_file() && out.json.is_file());
    let json: Value = serde_json::from_slice(&std::fs::read(&out.json).unwrap()).unwrap();
    assert_eq!(json["replay_sha256"], format!("{:x}", Sha256::digest(&before)));
    assert_eq!(json["report"]["status"], "clear");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(image::open(&out.png).unwrap().height() > 2000);
}

#[test]
fn render_actual_report_fixture_for_visual_review() {
    let c = Control::default();
    let mut data = features::extract(Arc::new(tape(400, true)), bound(Some(chart(400, 1))), &c).unwrap();
    data.hash = "0123456789abcdef".repeat(4);
    let analyzer = Analyzer::fixture();
    let result = analyzer.analyze(Arc::new(data.clone_for_plugin()), &c).unwrap();
    let image = report::render(&data, &result, &analyzer, &c).unwrap();
    assert_eq!(image.width(), 1440);
    assert!(image.height() > 2500);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("target/prca-qa");
    std::fs::create_dir_all(&root).unwrap();
    image.save(root.join("prca-review.png")).unwrap();
    std::fs::write(root.join("prca-review.json"), serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    let mut legacy = Replay {
        speed: 1.,
        source_version: 5,
        touches: vec![TouchEvent {
            phase: 0,
            id: 1,
            t: 1.,
            x: 0.,
            y: 0.,
        }],
        ..Default::default()
    };
    legacy.frames = vec![1.];
    let mut data = features::extract(Arc::new(legacy), bound(None), &c).unwrap();
    data.hash = "f".repeat(64);
    let result = analyzer.analyze(Arc::new(data.clone_for_plugin()), &c).unwrap();
    report::render(&data, &result, &analyzer, &c)
        .unwrap()
        .save(root.join("prca-legacy.png"))
        .unwrap();
}

#[test]
#[ignore = "read-only integration check; set PRCA_QA_DATA_ROOT to an existing data directory"]
fn existing_local_replays_finish_full_analysis() {
    let root = PathBuf::from(std::env::var_os("PRCA_QA_DATA_ROOT").expect("PRCA_QA_DATA_ROOT is required"));
    let old_root = crate::DATA_PATH
        .lock()
        .unwrap()
        .replace(root.parent().unwrap().to_string_lossy().into_owned());
    let destination = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/prca-qa/real-reports-v4");
    let candidates: Vec<String> = ["custom", "download"]
        .iter()
        .flat_map(|group| {
            std::fs::read_dir(root.join("charts").join(group))
                .unwrap()
                .filter_map(Result::ok)
                .map(move |entry| format!("{group}/{}", entry.file_name().to_string_lossy()))
        })
        .collect();
    let paths: Vec<_> = std::fs::read_dir(root.join("replays"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|v| v == "ppr"))
        .collect();
    assert!(!paths.is_empty());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    for path in paths {
        let before = std::fs::read(&path).unwrap();
        let control = Control::default();
        let job = Job {
            path: path.clone(),
            preferred: None,
            candidates: candidates.clone(),
            analyzer: Analyzer::fixture(),
            destination: destination.clone(),
            control: control.clone(),
        };
        let started = std::time::Instant::now();
        let mut work = worker::Work::spawn("prca-analysis", move || job.execute()).unwrap();
        let output = loop {
            if let Some(result) = work.take() {
                break result.unwrap();
            }
            assert!(started.elapsed().as_secs() < 90, "{}: {} {}", path.display(), control.stage(), control.detail());
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(output.chart_verified, "recorded chart must actually match: {}", path.display());
        let report: Value = serde_json::from_slice(&std::fs::read(&output.json).unwrap()).unwrap();
        if std::env::var_os("PRCA_QA_NORMAL_REPLAY").as_deref() == path.file_name() {
            assert_eq!(report["report"]["status"], "clear", "normal keyboard replay: {}", report["report"]);
            assert!(report["report"]["evidence"].as_array().unwrap().is_empty());
        }
        println!(
            "status={} effective={} contacts={}",
            report["report"]["status"], report["input"]["compact"]["independent_timing_samples"], report["input"]["compact"]["complete_contacts"]
        );
        assert!(output.png.is_file() && output.json.is_file());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        println!(
            "{}: verified={} elapsed={:.2}s report={}",
            path.file_name().unwrap().to_string_lossy(),
            output.chart_verified,
            started.elapsed().as_secs_f32(),
            output.png.display()
        );
    }
    *crate::DATA_PATH.lock().unwrap() = old_root;
}

fn keyboard_tape(n: usize) -> Replay {
    let mut r = tape(n, false);
    let mut config = prpr::config::Config::default();
    config.use_keyboard = true;
    r.gameplay = Some(crate::replay::Gameplay::capture(&config));
    for (i, input) in r.capture.as_mut().unwrap().inputs.iter_mut().enumerate() {
        input.event.source = "keyboard".into();
        input.event.event_time = None;
        input.event.x = None;
        input.event.y = None;
        input.event.id = (i / 2 % 4) as u64;
    }
    r
}
fn plugin_result(r: Replay, count: usize) -> (Value, Value) {
    let data = features::extract(Arc::new(r), bound(Some(chart(count, 1))), &Control::default()).unwrap();
    let payload = data.payload.clone();
    let result = Analyzer::fixture().analyze(Arc::new(data), &Control::default()).unwrap();
    (payload, result)
}
#[test]
fn short_and_long_keyboard_tapes_with_auxiliary_mouse_are_not_suspect() {
    for n in [16, 32, 64, 400] {
        let mut r = keyboard_tape(n);
        let capture = r.capture.as_mut().unwrap();
        let mut down = capture.inputs[0].clone();
        down.event.source = "mouse".into();
        down.event.id = 99;
        let mut up = down.clone();
        up.event.phase = "up".into();
        up.event.observed_time += 0.001;
        capture.inputs.splice(0..0, [down, up]);
        let (payload, result) = plugin_result(r, n);
        assert_eq!(payload["quality"]["primary_source"], "keyboard");
        assert_eq!(payload["compact"]["complete_contacts"], n);
        assert_eq!(payload["quality"]["backend_timestamp_fraction"], 0.);
        assert_eq!(result["status"], "clear", "{n}: {result}");
        assert!(result["evidence"].as_array().unwrap().is_empty());
        assert_eq!(result["checks"][3]["status"], "not_applicable");
    }
}
#[test]
fn compact_blocks_detect_repeated_patterns_on_a_short_chart_without_inflating_chords() {
    let (_, result) = plugin_result(tape(64, true), 64);
    assert_eq!(result["status"], "review", "two short non-overlapping blocks: {result}");
    let (_, tiny) = plugin_result(tape(8, false), 8);
    assert_eq!(tiny["status"], "limited");
    let mut chord = tape(1, true);
    chord.judges = (0..128)
        .map(|note| JudgeEvent {
            t: 0.,
            note,
            ..Default::default()
        })
        .collect();
    let (payload, result) = plugin_result(chord, 128);
    assert_eq!(payload["compact"]["independent_timing_samples"], 1);
    assert_eq!(result["status"], "limited");
}
#[test]
fn keyboard_frame_quantization_is_distinguished_from_a_repeated_non_lattice_pattern() {
    for (hold_ms, expected) in [(30., "clear"), (33., "review")] {
        let mut r = keyboard_tape(128);
        r.frames = (0..2000).map(|i| i as f64 * 0.01).collect();
        let c = r.capture.as_mut().unwrap();
        for pair in c.inputs.chunks_mut(2) {
            pair[0].event.id = 65;
            pair[1].event.id = 65;
            pair[1].event.observed_time = pair[0].event.observed_time + hold_ms / 1000.;
        }
        let (payload, result) = plugin_result(r, 128);
        assert_eq!(result["status"], expected, "hold={hold_ms} compact={} result={result}", payload["compact"]);
        if expected == "review" {
            assert_eq!(result["evidence"][0]["family"], "duration");
        }
    }
}
#[test]
fn per_key_patterns_are_not_created_by_pooling_two_different_keys() {
    let mut r = keyboard_tape(64);
    for (i, pair) in r.capture.as_mut().unwrap().inputs.chunks_mut(2).enumerate() {
        pair[0].event.id = (i % 2) as u64;
        pair[1].event.id = (i % 2) as u64;
        pair[1].event.observed_time = pair[0].event.observed_time + if i % 2 == 0 { 0.033 } else { 0.073 };
    }
    let (payload, result) = plugin_result(r, 64);
    assert_eq!(payload["compact"]["duration"].as_array().unwrap().len(), 2);
    assert_eq!(result["status"], "clear");
}

#[test]
fn paged_observations_keep_full_blocks_out_of_the_script_context() {
    let data = features::extract(Arc::new(tape(400, false)), bound(Some(chart(400, 1))), &Control::default()).unwrap();
    let compact = &data.payload["compact"];
    assert!(compact["timing"].as_array().unwrap().len() > 1);
    assert_eq!(data.observation_page("timing", 0, 1).len(), 1);
    assert!(data.observation_page("timing", i64::MAX, 1).is_empty());
    assert!(data.observation_page("invalid", 0, 1).is_empty());
    assert!(data.observation_page("duration", 0, 257).is_empty());
    let ctx = data.plugin_payload();
    assert!(ctx["compact"]["timing"].is_null() && ctx["compact"]["duration"].is_null());
    assert_eq!(ctx["compact"]["timing_blocks"], compact["timing"].as_array().unwrap().len());
}
#[test]
fn a_local_keyboard_macro_is_not_diluted_by_a_long_normal_prefix() {
    let mut r = keyboard_tape(4096);
    r.frames = (0..50000).map(|i| i as f64 * 0.01).collect();
    for pair in r.capture.as_mut().unwrap().inputs[4000 * 2..].chunks_mut(2) {
        pair[0].event.id = 65;
        pair[1].event.id = 65;
        pair[1].event.observed_time = pair[0].event.observed_time + 0.033;
    }
    let (payload, result) = plugin_result(r, 4096);
    assert_eq!(payload["compact"]["timing_blocks"], 128);
    assert_eq!(result["status"], "review", "{result}");
    assert!(result["evidence"][0]["start"].as_f64().unwrap() > 399.);
}
#[test]
fn a_keyboard_timing_cluster_alone_does_not_claim_subframe_evidence() {
    let mut r = keyboard_tape(128);
    for e in &mut r.judges {
        e.diff = 0.;
    }
    let (_, result) = plugin_result(r, 128);
    assert_eq!(result["status"], "clear");
    assert_eq!(result["checks"][1]["status"], "observe");
}

#[test]
fn missing_frame_timing_does_not_create_keyboard_duration_evidence() {
    let mut r = keyboard_tape(128);
    r.frames.clear();
    for pair in r.capture.as_mut().unwrap().inputs.chunks_mut(2) {
        pair[0].event.id = 65;
        pair[1].event.id = 65;
        pair[1].event.observed_time = pair[0].event.observed_time + 0.033;
    }
    let (payload, result) = plugin_result(r, 128);
    assert_eq!(payload["quality"]["duration_candidate_eligible"], false);
    assert_eq!(result["status"], "clear");
    assert_eq!(result["checks"][2]["status"], "observe");
}
