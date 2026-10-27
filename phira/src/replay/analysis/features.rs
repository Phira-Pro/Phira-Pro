//! Measurement layer. No anomaly thresholds or PRCA-specific verdicts live here.
use super::{samples, stats, Control, Dataset, Series, FEATURE_SCHEMA};
use crate::replay::Replay;
use anyhow::Result;
use prpr::core::{Chart, NoteKind};
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

pub(super) use super::chart::{find_chart, Bound};

#[derive(Clone)]
struct Sample {
    t: f64,
    chart: f64,
    x: Option<f64>,
    y: Option<f64>,
    precise: bool,
    segment: usize,
}
struct Contact {
    start: Sample,
    last: Sample,
    path: f64,
    moves: usize,
}
#[derive(Default)]
struct Inputs {
    durations: Vec<[f64; 2]>,
    contacts: Vec<samples::Contact>,
    primary: String,
    primary_events: usize,
    primary_precise: usize,
    presses: Vec<[f64; 2]>,
    speed: Vec<[f64; 2]>,
    paths: Vec<[f64; 2]>,
    traces: Vec<Vec<[f64; 2]>>,
    concurrency: Vec<[f64; 2]>,
    synchronization: Vec<[f64; 2]>,
    subscription_delay: Vec<[f64; 2]>,
    consumption_delay: Vec<[f64; 2]>,
    incomplete: usize,
    cancelled: usize,
    precise: usize,
    total: usize,
    backwards: usize,
    clock_mismatch: usize,
    max_concurrent: usize,
    sources: BTreeSet<String>,
}

fn inputs(replay: &Replay, control: &Control) -> Result<Inputs> {
    let keyboard = replay.gameplay.as_ref().is_some_and(|g| g.use_keyboard);
    let has = |source: &str| {
        replay
            .capture
            .as_ref()
            .is_some_and(|c| c.inputs.iter().any(|i| i.active && i.event.source == source && i.event.phase == "down"))
    };
    let primary = if replay.capture.is_none() {
        "legacy"
    } else if keyboard || (has("keyboard") && !has("touch")) {
        "keyboard"
    } else if has("touch") {
        "touch"
    } else {
        "mouse"
    };
    let mut out = Inputs {
        primary: primary.into(),
        ..Default::default()
    };
    let mut active: HashMap<(String, u64), Contact> = HashMap::new();
    let mut trace: HashMap<(String, u64), Vec<[f64; 2]>> = HashMap::new();
    let mut clock_mismatch = 0usize;
    let mut subscription_delay = Vec::new();
    let mut consumption_delay = Vec::new();
    let mut observe = |source: &str, phase: &str, id: u64, s: Sample, enabled: bool| {
        let key = (source.to_owned(), id);
        if !enabled {
            out.incomplete += active.len();
            active.clear();
            trace.clear();
            return;
        }
        if active.values().any(|c| c.start.segment != s.segment) {
            out.incomplete += active.len();
            active.clear();
            trace.clear();
        }
        if phase == "move" && !active.contains_key(&key) {
            return;
        }
        out.sources.insert(source.into());
        out.total += 1;
        if s.precise {
            out.precise += 1;
        }
        if source != primary {
            return;
        }
        out.primary_events += 1;
        if s.precise {
            out.primary_precise += 1;
        }
        if phase == "repeat" {
            return;
        }
        if phase == "down" {
            if active.remove(&key).is_some() {
                out.incomplete += 1;
            }
            out.presses.push([s.chart, 0.]);
            if let (Some(x), Some(y)) = (s.x, s.y) {
                trace.insert(key.clone(), vec![[x, y]]);
            }
            active.insert(
                key.clone(),
                Contact {
                    start: s.clone(),
                    last: s.clone(),
                    path: 0.,
                    moves: 0,
                },
            );
        } else if let Some(c) = active.get_mut(&key) {
            if s.t < c.last.t {
                out.backwards += 1;
            }
            if let (Some(x), Some(y), Some(px), Some(py)) = (s.x, s.y, c.last.x, c.last.y) {
                let distance = (x - px).hypot(y - py);
                let dt = s.t - c.last.t;
                // No fabricated division at zero time; no motion across >100ms gaps.
                if dt > 0. && dt <= 0.1 {
                    c.path += distance;
                    if phase == "move" {
                        out.speed.push([s.chart, distance / dt]);
                        c.moves += 1;
                    }
                }
                if let Some(points) = trace.get_mut(&key) {
                    if points.len() < 256 {
                        points.push([x, y]);
                    }
                }
            }
            if phase == "up" {
                let dt = s.t - c.start.t;
                if dt >= 0. && dt <= 120. {
                    out.durations.push([c.start.chart, dt * 1000.]);
                    out.contacts.push(samples::Contact {
                        time: c.start.chart,
                        millis: dt * 1000.,
                        key: matches!(source, "keyboard" | "mouse").then_some(id),
                        segment: c.start.segment,
                    });
                } else {
                    out.incomplete += 1;
                }
                if c.moves >= 3 {
                    out.paths.push([c.start.chart, c.path]);
                }
            } else if phase == "cancel" {
                out.cancelled += 1;
            }
            c.last = s.clone();
        } else if phase != "move" {
            out.incomplete += 1;
        }
        if matches!(phase, "up" | "cancel") {
            active.remove(&key);
            if let Some(points) = trace.remove(&key) {
                if points.len() >= 4 && out.traces.len() < 12 {
                    out.traces.push(points);
                }
            }
        }
        out.max_concurrent = out.max_concurrent.max(active.len());
        out.concurrency.push([s.chart, active.len() as f64]);
    };
    if let Some(capture) = &replay.capture {
        let mut segments = Vec::with_capacity(capture.anchors.len());
        let mut segment = 0;
        let mut was_active = false;
        for a in &capture.anchors {
            if a.active != was_active {
                segment += 1;
                was_active = a.active;
            }
            segments.push(segment);
        }
        for (i, input) in capture.inputs.iter().enumerate() {
            if i % 4096 == 0 {
                control.check()?;
            }
            let frame = input.frame as usize;
            let Some(anchor) = capture.anchors.get(frame).filter(|a| a.frame == input.frame) else {
                continue;
            };
            let event = &input.event;
            // Keyboard events may exist for pause controls while gameplay keyboard is disabled.
            if event.source == "keyboard" && !replay.gameplay.as_ref().is_some_and(|g| g.use_keyboard) {
                continue;
            }
            let compatible = event.event_time.is_some_and(|t| (t - anchor.uptime).abs() <= 0.25);
            if event.event_time.is_some() && !compatible {
                clock_mismatch += 1;
            }
            let time = if compatible { event.event_time.unwrap() } else { event.observed_time };
            let chart = anchor.chart_time + (time - anchor.uptime) * anchor.speed as f64;
            if input.active && compatible {
                let delay = event.observed_time - time;
                if delay >= 0. {
                    subscription_delay.push([chart, delay * 1000.]);
                }
            }
            if input.active && anchor.uptime >= event.observed_time {
                consumption_delay.push([chart, (anchor.uptime - event.observed_time) * 1000.]);
            }
            let [vx, vy, vw, vh] = anchor.viewport;
            let x = event.x.map(|x| (x as f64 - vx as f64) * 2. / vw as f64 - 1.);
            let y = event
                .y
                .map(|y| (anchor.screen_height as f64 - (vy + vh) as f64 + vh as f64 / 2. - y as f64) * 2. / vw as f64);
            observe(
                &event.source,
                &event.phase,
                event.id,
                Sample {
                    t: time,
                    chart,
                    x,
                    y,
                    precise: compatible,
                    segment: segments[frame],
                },
                input.active,
            );
        }
    } else {
        for (i, e) in replay.touches.iter().enumerate() {
            if i % 4096 == 0 {
                control.check()?;
            }
            observe(
                "legacy",
                match e.phase {
                    0 => "down",
                    1 => "move",
                    _ => "up",
                },
                e.id,
                Sample {
                    t: e.t as f64 / replay.speed as f64,
                    chart: e.t as f64,
                    x: Some(e.x as f64),
                    y: Some(e.y as f64),
                    precise: false,
                    segment: 0,
                },
                true,
            );
        }
    }
    drop(observe);
    out.clock_mismatch = clock_mismatch;
    out.subscription_delay = subscription_delay;
    out.consumption_delay = consumption_delay;
    out.incomplete += active.len();
    // Only real backend timestamps can describe inter-contact onset spread.
    if replay.capture.is_some() {
        let mut groups: HashMap<u64, Vec<f64>> = HashMap::new();
        for c in &replay.capture.as_ref().unwrap().inputs {
            if c.active && c.event.phase == "down" && c.event.source == "touch" {
                if let Some(t) = c.event.event_time {
                    groups.entry(c.frame).or_default().push(t);
                }
            }
        }
        for (frame, mut times) in groups {
            if times.len() >= 2 {
                times.sort_by(f64::total_cmp);
                let t = replay.capture.as_ref().unwrap().anchors[frame as usize].chart_time;
                out.synchronization.push([t, (times[times.len() - 1] - times[0]) * 1000.]);
            }
        }
        out.synchronization.sort_by(|a, b| a[0].total_cmp(&b[0]));
    }
    Ok(out)
}

fn series(title: &str, x: &str, y: &str, points: &[[f64; 2]], scatter: bool) -> Series {
    Series {
        title: title.into(),
        x_label: x.into(),
        y_label: y.into(),
        points: stats::envelope(points, 450),
        scatter,
        observed: points.len(),
    }
}

pub fn extract(replay: Arc<Replay>, mut bound: Bound, control: &Control) -> Result<Dataset> {
    let input = inputs(&replay, control)?;
    let mut quality = vec![
        bound.status.clone(),
        format!("音乐校验：{}", bound.audio),
        "本地回放元数据与输入来源未经可信第三方认证".into(),
    ];
    if replay.capture.is_none() {
        quality.push("旧回放触点仅有帧时间：时长与轨迹为粗粒度观察，多指同步检测停用".into());
    } else {
        quality.push("触摸使用后端实测时间（如有）；鼠标/键盘为输入订阅时刻，不能视为硬件时间".into());
    }
    if replay.repaired_order {
        quality.push("原文件事件乱序，兼容层已排序；原始顺序相关推断停用".into());
    }
    if !replay.has_diffs {
        quality.push("未记录真实有符号误差；计时异常检测停用".into());
    }
    if replay.settings.is_none() {
        quality.push("录制判定配置缺失；不能进行规则一致性认定".into());
    }
    if replay.gameplay.as_ref().is_some_and(|g| g.use_keyboard) {
        quality.push("启用了键盘玩法；不能以触点缺失推断无输入命中".into());
    }
    if input.incomplete > 0 || input.cancelled > 0 {
        quality.push(format!("截尾/不完整接触 {} 个，取消 {} 个；不计入完整按压时长", input.incomplete, input.cancelled));
    }
    let dropped = replay.capture.as_ref().map_or(0, |c| c.dropped);
    if dropped > 0 {
        quality.push(format!("录制缓冲缺口 {dropped} 条；涉及的输入统计不能作为强证据"));
    }
    if input.backwards > 0 {
        quality.push(format!("输入时间回退 {} 处；相关运动学仅供观察", input.backwards));
    }
    if input.clock_mismatch > 0 {
        quality.push(format!("后端时钟与消费锚点不相容 {} 条；已退回订阅时间，不宣称硬件时间精度", input.clock_mismatch));
    }
    let mut timing = Vec::new();
    let mut tails = Vec::new();
    let mut invalid = 0;
    let mut duplicate = 0;
    let mut seen = HashSet::new();
    let mut hold_scores: HashMap<(u32, u32), usize> = HashMap::new();
    let mut counts = [0usize; 4];
    let mut expected = Vec::new();
    if let Some(chart) = &bound.chart {
        for line in &chart.lines {
            for n in &line.notes {
                if n.fake {
                    continue;
                }
                let kind = match n.kind {
                    NoteKind::Click => 0,
                    NoteKind::Hold { .. } => 1,
                    NoteKind::Drag => 2,
                    NoteKind::Flick => 3,
                };
                counts[kind] += 1;
                if kind <= 1 {
                    expected.push(n.time);
                }
            }
        }
        for (i, e) in replay.judges.iter().enumerate() {
            if i % 4096 == 0 {
                control.check()?;
            }
            let Some(note) = chart
                .lines
                .get(e.line as usize)
                .and_then(|l| l.notes.get(e.note as usize))
                .filter(|n| !n.fake)
            else {
                invalid += 1;
                continue;
            };
            let head = matches!(e.kind, 5 | 6);
            let hold = matches!(note.kind, NoteKind::Hold { .. });
            // A hold with tail judgement legitimately records a visual head,
            // scored head and scored tail. These are not duplicate judgements.
            // Only exact repeated non-hold events are structural duplicates.
            if !hold && !seen.insert((e.line, e.note, e.kind, e.t.to_bits(), e.diff.to_bits())) {
                duplicate += 1;
            }
            let scored_hold = if hold && !head {
                let count = hold_scores.entry((e.line, e.note)).or_default();
                *count += 1;
                *count
            } else {
                0
            };
            if !replay.has_diffs || e.kind == 3 {
                continue;
            }
            match note.kind {
                NoteKind::Click if !head => timing.push([e.t, e.diff as f64 * 1000.]),
                NoteKind::Hold { .. } if head => timing.push([e.t, e.diff as f64 * 1000.]),
                NoteKind::Hold { .. } if scored_hold >= 2 && replay.settings.as_ref().is_some_and(|s| s.hold_tail) => {
                    tails.push([e.t, e.diff as f64 * 1000.])
                }
                _ => {}
            }
        }
    }
    if bound.chart.is_none() {
        quality.push("缺少可解析谱面，无法可靠区分 Tap/Hold 与合成 0 误差；命中误差图留空".into());
    }
    let mut frame_dt = Vec::new();
    for w in replay.frames.windows(2) {
        let d = (w[1] - w[0]) / replay.speed as f64 * 1000.;
        if d > 0. {
            frame_dt.push(d);
        }
    }
    let duration = replay
        .frames
        .last()
        .copied()
        .unwrap_or(0.)
        .max(replay.judges.last().map_or(0., |j| j.t))
        .max(replay.touches.last().map_or(0., |e| e.t as f64))
        .max(1.);
    let window_s = (duration / 512.).max(20.);
    let n_windows = (duration / window_s).ceil() as usize;
    let mut t_windows = vec![Vec::new(); n_windows];
    let mut d_windows = t_windows.clone();
    for &[t, v] in &timing {
        let i = ((t.max(0.) / window_s) as usize).min(n_windows - 1);
        t_windows[i].push(v);
    }
    for &[t, v] in &input.durations {
        let i = ((t.max(0.) / window_s) as usize).min(n_windows - 1);
        d_windows[i].push(v);
    }
    let windows:Vec<_>=(0..n_windows).map(|i|json!({"start":i as f64*window_s,"end":((i+1) as f64*window_s).min(duration),"timing":stats::describe(&t_windows[i]),"duration":stats::describe(&d_windows[i])})).collect();
    let values: Vec<_> = timing.iter().map(|p| p[1]).collect();
    let durations: Vec<_> = input.durations.iter().map(|p| p[1]).collect();
    let speeds: Vec<_> = input.speed.iter().map(|p| p[1]).collect();
    let mut geometry = Vec::new();
    let geometry_supported = bound.verified
        && bound.chart.as_ref().is_some_and(|c| !c.has_block_areas() && c.lines.len() <= 1024)
        && replay.settings.is_some()
        && !replay.gameplay.as_ref().is_some_and(|g| g.use_keyboard);
    if geometry_supported {
        geometry = geometry_observations(&replay, bound.chart.as_mut().unwrap(), control)?;
    }
    quality.push(
        if geometry_supported {
            "落点图为同帧候选接触的最小横向余量；未声称精确输入归属或完整重判"
        } else {
            if input.primary == "keyboard" {
                "键盘模式不适用触点落点与位移检查，已跳过"
            } else {
                "落点核对未启用：需要已校验谱面、完整配置及受支持机制"
            }
        }
        .into(),
    );
    let mut graphs = vec![
        series("命中偏差：仅 Tap / Hold 头判", "谱面时间 / s", "偏差 / ms", &timing, true),
        series("完整接触的按压时长", "谱面时间 / s", "时长 / ms", &input.durations, true),
        series("触点/按键并发：接触生命周期", "谱面时间 / s", "活动接触数", &input.concurrency, false),
        series("连续接触速度：不跨接触连接", "谱面时间 / s", "归一化坐标 / s", &input.speed, true),
        series("落点候选：横向判定余量", "谱面时间 / s", "横向距离 / 接纳半宽", &geometry, true),
        series("同帧触摸按下跨度：仅后端时间", "谱面时间 / s", "跨度 / ms", &input.synchronization, true),
    ];
    let h = stats::histogram(&values, -220., 220., 55);
    let mut graph = series("命中误差分布：不含合成误差", "偏差 / ms", "样本数", &h, false);
    graph.observed = values.len();
    graphs.push(graph);
    let dh = stats::histogram(&durations, 0., 500., 50);
    let mut graph = series("按压时长分布：图示 0–500ms", "时长 / ms", "样本数", &dh, false);
    graph.observed = durations.len();
    graphs.push(graph);
    // Input vs chart counts are descriptive, with no frequency-based verdict.
    let bins = (duration.ceil() as usize).clamp(1, 900);
    let width = duration / bins as f64;
    let mut observed = vec![0.; bins];
    let mut chart_counts = vec![0.; bins];
    for p in &input.presses {
        observed[((p[0].max(0.) / width) as usize).min(bins - 1)] += 1.;
    }
    for t in &expected {
        chart_counts[((t.max(0.) / width) as usize).min(bins - 1)] += 1.;
    }
    let cps: Vec<_> = observed.iter().enumerate().map(|(i, v)| [(i as f64 + 0.5) * width, *v / width]).collect();
    let density: Vec<_> = chart_counts
        .iter()
        .enumerate()
        .map(|(i, v)| [(i as f64 + 0.5) * width, *v / width])
        .collect();
    let mut graph = series("实际按下密度：非命中数", "谱面时间 / s", "按下 / s", &cps, false);
    graph.observed = input.presses.len();
    graphs.push(graph);
    let mut graph = series("谱面 Tap / Hold 头部密度", "谱面时间 / s", "音符 / s", &density, false);
    graph.observed = expected.len();
    graphs.push(graph);
    let spectrum = spectrum(&timing, control)?;
    let mut graph = series("误差序列周期：按音符序号，不是硬件 Hz", "周期 / 音符", "归一化幅度", &spectrum, false);
    graph.observed = values.len();
    graphs.push(graph);
    graphs.push(series("后端事件→输入订阅：不等于硬件延迟", "谱面时间 / s", "延迟 / ms", &input.subscription_delay, true));
    graphs.push(series("输入订阅→录制消费锚点", "谱面时间 / s", "延迟 / ms", &input.consumption_delay, true));
    // Display separate lifetimes; never join two fingers into a trajectory.
    for (i, trace) in input.traces.iter().take(1).enumerate() {
        graphs.push(series(&format!("代表性实测接触轨迹 {}", i + 1), "横向归一化坐标", "纵向归一化坐标", trace, false));
    }
    let precise_fraction = if input.primary_events > 0 {
        input.primary_precise as f64 / input.primary_events as f64
    } else {
        0.
    };
    let timing_available = bound.verified && replay.has_diffs && !values.is_empty() && replay.settings.is_some() && !replay.repaired_order;
    let timing_candidate_eligible = timing_available
        && replay.capture.is_some()
        && precise_fraction >= 0.95
        && dropped == 0
        && input.backwards == 0
        && replay
            .settings
            .as_ref()
            .is_some_and(|s| s.algorithm == prpr::config::JudgeAlgorithm::PhiraPro)
        && input.primary == "touch";
    let frame_summary = stats::describe(&frame_dt);
    let frame_ms = frame_summary["median"].as_f64().unwrap_or(0.);
    let compact = samples::compact(&timing, &input.contacts, frame_ms);
    let duration_candidate_eligible = replay.capture.is_some()
        && dropped == 0
        && input.backwards == 0
        && input.clock_mismatch == 0
        && !replay.repaired_order
        && (precise_fraction >= 0.95 || (frame_dt.len() >= 16 && (0.5..=100.).contains(&frame_ms)));
    let payload = json!({"api":3,"feature_schema":FEATURE_SCHEMA,"metadata":{"name":replay.meta.name,"player":replay.meta.player,"level":replay.meta.level,"speed":if replay.has_speed {Some(replay.speed)}else{None},"result":replay.meta.result.as_ref().map(|r|json!({"score":r.score,"accuracy":r.accuracy})),"source_version":replay.source_version,"duration":duration},
        "chart":{"verified":bound.verified,"fingerprint":bound.fingerprint,"status":bound.status,"audio":bound.audio,"note_counts":counts},
        "compact":compact,"quality":{"primary_source":input.primary,"primary_input_events":input.primary_events,"duration_candidate_eligible":duration_candidate_eligible,"clock_mismatch":input.clock_mismatch,"raw_input":replay.capture.is_some(),"backend_timestamp_fraction":precise_fraction,"dropped":dropped,"repaired_order":replay.repaired_order,"incomplete_contacts":input.incomplete,"cancelled_contacts":input.cancelled,"time_reversals":input.backwards,"sources":input.sources,"timing_available":timing_available,"timing_candidate_eligible":timing_candidate_eligible,"geometry_available":geometry_supported,"judgement_config":replay.settings},
        "metrics":{"timing_ms":stats::describe(&values),"duration_ms":stats::describe(&durations),"speed_units":stats::describe(&speeds),"frame_ms":stats::describe(&frame_dt),"tail_timing_ms":stats::describe(&tails.iter().map(|p|p[1]).collect::<Vec<_>>()),"lag1":stats::correlation(&values,1),"max_concurrent":input.max_concurrent,"press_count":input.presses.len(),"invalid_targets":invalid,"duplicate_stages":duplicate},
        "windows":windows,"coverage":{"timed_samples":values.len(),"complete_contacts":durations.len(),"geometry_samples":geometry.len(),"input_events":input.total,"judgement_events":replay.judges.len(),"full_rejudgement":false,"cross_replay_comparison":false,"calibrated_model":false}});
    Ok(Dataset {
        replay,
        payload,
        series: graphs,
        quality,
        hash: String::new(),
    })
}

fn geometry_observations(replay: &Replay, chart: &mut Chart, control: &Control) -> Result<Vec<[f64; 2]>> {
    let aspect = replay.aspect_ratio.unwrap_or(16. / 9.);
    let mut out = Vec::new();
    let mut active: HashMap<u64, (f64, macroquad::prelude::Vec2)> = HashMap::new();
    let mut cursor = 0;
    let mut last_t = f64::NEG_INFINITY;
    let fullscreen = replay.settings.as_ref().is_some_and(|s| s.fullscreen);
    let flip = replay.mods as i32 & prpr::config::Mods::FLIP_X.bits() != 0;
    for (i, e) in replay.judges.iter().enumerate() {
        if i % 1024 == 0 {
            control.check()?;
        }
        // Only descriptive candidates in the same recorded frame, not guessed IDs.
        while let Some(touch) = replay.touches.get(cursor).filter(|t| t.t as f64 <= e.t + 0.0001) {
            if touch.phase == 2 {
                active.remove(&touch.id);
            } else {
                active.insert(touch.id, (touch.t as f64, macroquad::prelude::vec2(touch.x, touch.y)));
            }
            cursor += 1;
        }
        if e.kind == 3 {
            continue;
        }
        let Some(note) = chart.lines.get(e.line as usize).and_then(|l| l.notes.get(e.note as usize)) else {
            continue;
        };
        if !matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. }) {
            continue;
        }
        if matches!(note.kind, NoteKind::Hold { .. }) && !matches!(e.kind, 5 | 6) {
            continue;
        }
        if e.t != last_t {
            for line in &mut chart.lines {
                line.object.set_time(e.t);
            }
            last_t = e.t;
        }
        let line = &chart.lines[e.line as usize];
        let Some(inv) = line.transform_at_aspect(aspect, &chart.lines).try_inverse() else {
            continue;
        };
        let note = &mut chart.lines[e.line as usize].notes[e.note as usize];
        note.object.translation.0.set_time(e.t);
        let x = note.object.translation.0.now();
        let area = note.judge_area;
        let mut best = f64::INFINITY;
        for &(time, mut p) in active.values() {
            if (e.t - time).abs() > 0.1 {
                continue;
            }
            if flip {
                p.x = -p.x;
            }
            let Some(p) = chart.unproject_touch(p, e.t, aspect) else { continue };
            let local = inv.transform_point(&prpr::core::Point::new(p.x, -p.y));
            let dx = if fullscreen {
                0.
            } else {
                (local.x - x).abs() as f64 / (area as f64 * prpr::judge::X_DIFF_MAX)
            };
            if dx.is_finite() {
                best = best.min(dx);
            }
        }
        if best.is_finite() {
            out.push([e.t, best]);
        }
    }
    Ok(out)
}

fn spectrum(timing: &[[f64; 2]], control: &Control) -> Result<Vec<[f64; 2]>> {
    if timing.len() < 128 {
        return Ok(vec![]);
    }
    // Bounded sequence spectrum of full-data block means, explicitly not OS Hz.
    let group = timing.len().div_ceil(2048);
    let v: Vec<_> = timing
        .chunks(group)
        .map(|w| w.iter().map(|p| p[1]).sum::<f64>() / w.len() as f64)
        .collect();
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let scale = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>().sqrt();
    if scale < 1e-9 {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for k in 1..=64.min(v.len() / 2) {
        control.check()?;
        let mut re = 0.;
        let mut im = 0.;
        for (i, x) in v.iter().enumerate() {
            let a = std::f64::consts::TAU * k as f64 * i as f64 / v.len() as f64;
            re += (x - mean) * a.cos();
            im += (x - mean) * a.sin();
        }
        out.push([v.len() as f64 * group as f64 / k as f64, re.hypot(im) / scale / (v.len() as f64).sqrt()]);
    }
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(out)
}
