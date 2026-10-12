//! Real GameScene playback and management regression, confined to a temporary data root.
use super::*;
use crate::{
    data::{Data, LocalChart},
    replay::{JudgeEvent, Metadata, TouchEvent},
    DATA_PATH,
};
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    info::ChartInfo,
    ui::{FontArc, TextPainter},
};
use std::io::Write;

async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = prpr::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}
fn wave(seconds: u32) -> Vec<u8> {
    let n = 8000 * seconds * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    b.resize(b.len() + n as usize, 0);
    b
}
fn chart_fixture(root: &std::path::Path, name: &str) -> ChartInfo {
    let directory = root.join("data/charts").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let info = ChartInfo {
        name: "Replay transport QA".into(),
        level: "IN 15".into(),
        music: "song.wav".into(),
        ..Default::default()
    };
    std::fs::write(directory.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    std::fs::write(directory.join("song.wav"), wave(12)).unwrap();
    Image::gen_image_color(32, 32, Color::new(0.1, 0.2, 0.3, 1.)).export_png(directory.join("background.png").to_str().unwrap());
    let note = |kind: u8, time: i32, hold: i32| serde_json::json!({"type":kind,"time":time,"positionX":0,"holdTime":hold,"speed":1,"floorPosition":time as f32/64.});
    let event = |v: f32| serde_json::json!([{"startTime":0,"endTime":1000000,"start":v,"end":v,"start2":0.5,"end2":0.5}]);
    let chart = serde_json::json!({"formatVersion":3,"offset":0,"numOfNotes":4,"judgeLineList":[{"bpm":120,"notesAbove":[note(1,64,0),note(3,128,192),note(1,384,0),note(1,512,0)],"notesBelow":[],"speedEvents":[{"startTime":0,"endTime":1000000,"value":1}],"judgeLineMoveEvents":event(0.5),"judgeLineRotateEvents":event(0.),"judgeLineDisappearEvents":event(1.)}]});
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
    info
}
fn screenshot(painter: &mut TextPainter, scene: &mut ReplayScene, tm: &mut TimeManager, w: i32, h: i32, name: &str) -> Image {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    scene.render(tm, &mut ui).unwrap();
    assert!(scene.timeline.x >= -1. && scene.timeline.right() <= 1. && scene.timeline.bottom() <= ui.top);
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    let image = Image {
        width: w as u16,
        height: h as u16,
        bytes,
    };
    image.export_png(&format!("target/replay-qa/{name}.png"));
    image
}

fn click(scene: &mut ReplayScene, tm: &mut TimeManager, position: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Scene::touch(
            scene,
            tm,
            &Touch {
                id: 99,
                phase,
                position,
                time: 0.,
            },
        )
        .unwrap();
    }
    Scene::update(scene, tm).unwrap();
}

fn click_analysis(scene: &mut ReplayScene, tm: &mut TimeManager, index: usize, column: usize, second_row: bool) {
    let y = 720. / 1280. - 0.53 + 0.062 + 0.18 + if second_row { 0.08 } else { 0. };
    let point = panel_cell(column, 1, y, 0.067).center();
    assert!(scene.controls[index].inner.contains(point), "wrong hit region for control {index}");
    click(scene, tm, point);
}

fn pixel(image: &Image, x: i32, y: i32) -> &[u8] {
    let i = ((y * image.width as i32 + x) * 4) as usize;
    &image.bytes[i..i + 3]
}

async fn practice_regression(info: ChartInfo, painter: &mut TextPainter) {
    prpr::practice::set_storage(std::path::PathBuf::from(crate::dir::root().unwrap()).join("practice-bookmarks.json"));
    let mut config = Config::default();
    config.volume_music = 0.;
    config.volume_sfx = 0.;
    config.sample_count = 1;
    config.fxaa = true;
    config.adjust_time = false;
    let mut game = local(GameScene::new(
        GameMode::Exercise,
        info,
        config,
        fs_from_path("renamed-chart").unwrap(),
        None,
        BLACK_TEXTURE.clone(),
        BLACK_TEXTURE.clone(),
        None,
        None,
        None,
    ))
    .await
    .unwrap();
    let real_clock = std::rc::Rc::new(std::cell::Cell::new(10.));
    let clock = real_clock.clone();
    let mut tm = TimeManager::manual(Box::new(move || clock.get()));
    game.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    game.update(&mut tm).unwrap();
    tm.seek_to(0.);
    game.update(&mut tm).unwrap();
    game.pause(&mut tm).unwrap();
    prpr::scene::return_input("exercise_combo".into(), "2-2".into());
    game.update(&mut tm).unwrap();
    // The selected Hold starts at 2s and ends at 5s; both receive 250ms margin.
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        game.render(&mut tm, &mut ui).unwrap();
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        let text_pixels = bytes
            .chunks_exact(4)
            .enumerate()
            .filter(|(i, p)| {
                let x = *i as i32 % w;
                let y = *i as i32 / w;
                x > w / 5 && x < w * 4 / 5 && y > h / 5 && y < h * 4 / 5 && p[..3].iter().all(|&v| v > 180)
            })
            .count();
        assert!(text_pixels > 300, "practice controls must remain visible under FXAA at {ratio}: {text_pixels}");
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/replay-qa/practice-{ratio}.png"));
    }
    // Render again at the real window aspect so touch coordinates match it.
    let w = screen_width() as i32;
    let h = screen_height() as i32;
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    game.render(&mut tm, &mut ui).unwrap();
    drop(ui);
    let start_marker = game.practice_control_point("marker:-1").unwrap();
    for (phase, position) in [
        (TouchPhase::Started, start_marker),
        (TouchPhase::Moved, start_marker + vec2(0.03, 0.)),
        (TouchPhase::Ended, start_marker + vec2(0.03, 0.)),
    ] {
        Scene::touch(
            &mut game,
            &mut tm,
            &Touch {
                id: 776,
                phase,
                position,
                time: 0.,
            },
        )
        .unwrap();
    }
    assert!(tm.now() > 1.75, "the original blue marker must adjust the range");
    prpr::scene::return_input("exercise_combo".into(), "2-2".into());
    game.update(&mut tm).unwrap();
    let loop_point = game.practice_control_point("practice_loop").unwrap();
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        let activated = Scene::touch(
            &mut game,
            &mut tm,
            &Touch {
                id: 777,
                phase,
                position: loop_point,
                time: 0.,
            },
        )
        .unwrap();
        if phase == TouchPhase::Ended {
            assert!(activated, "auto-loop must be reachable");
        }
    }
    tm.resume();
    tm.seek_to(5.3);
    game.update(&mut tm).unwrap();
    assert!(!tm.paused(), "enabled auto-loop must continue playing");
    assert!((tm.now() - 1.75).abs() < 0.02, "combo loop must include the selected Hold tail");
    for note in &game.chart.lines[0].notes {
        if note.time < 1.75 {
            assert!(
                matches!(note.judge, JudgeStatus::Judged),
                "notes before the combo range must be skipped: time={}, state={:?}",
                note.time,
                note.judge
            );
        }
        if (note.time - 2.).abs() < 0.01 {
            assert!(!matches!(note.judge, JudgeStatus::Judged), "the selected Hold must remain playable");
        }
    }
    assert_eq!(game.practice_round_count(), 1, "the last Hold miss must be captured before looping");
    game.pause(&mut tm).unwrap();
    fn input(game: &mut GameScene, tm: &mut TimeManager, id: &str, text: &str) {
        prpr::scene::return_input(id.into(), text.into());
        game.update(tm).unwrap();
    }
    fn render(game: &mut GameScene, tm: &mut TimeManager, painter: &mut TextPainter) {
        let mut ui = Ui::new(painter, None);
        game.render(tm, &mut ui).unwrap();
    }
    fn click(game: &mut GameScene, tm: &mut TimeManager, painter: &mut TextPainter, key: &str, scroll: &mut f32) {
        render(game, tm, painter);
        let point = game
            .practice_control_point(key)
            .unwrap_or_else(|| panic!("missing practice control: {key}"));
        *scroll += point.y * scale_for(game);
        game.practice_scroll_to(*scroll);
        render(game, tm, painter);
        let point = game.practice_control_point(key).unwrap();
        assert!(point.x.abs() < 0.95 && point.y.abs() < 0.2, "control must be on screen: {key}: {point:?}");
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            Scene::touch(
                game,
                tm,
                &Touch {
                    id: 779,
                    phase,
                    position: point,
                    time: 0.,
                },
            )
            .unwrap();
        }
    }
    fn scale_for(game: &GameScene) -> f32 {
        (screen_width() / screen_height()) / game.res.aspect_ratio
    }
    let mut scroll = 0.;
    input(&mut game, &mut tm, "practice_speed", "1.25");
    assert_eq!(game.practice_round_count(), 0, "changing conditions starts a new statistics group");
    render(&mut game, &mut tm, painter);
    let drawer = game.practice_control_point("drawer").unwrap();
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Scene::touch(
            &mut game,
            &mut tm,
            &Touch {
                id: 780,
                phase,
                position: drawer,
                time: 0.,
            },
        )
        .unwrap();
    }
    real_clock.set(10.6);
    click(&mut game, &mut tm, painter, "audio", &mut scroll);
    click(&mut game, &mut tm, painter, "practice_pitch", &mut scroll);
    click(&mut game, &mut tm, painter, "practice_correct", &mut scroll);
    assert!(game.res.config.practice.preserve_pitch && game.res.config.practice.correct_sound);
    let normal_windows = Config::default().judge_windows();
    input(&mut game, &mut tm, "practice_multiplier", "150");
    assert!((game.res.windows.perfect - normal_windows.perfect * 1.5).abs() < 1e-6);
    let originals: Vec<_> = game
        .chart
        .lines
        .iter()
        .flat_map(|l| &l.notes)
        .map(|n| (n.time, format!("{:?}", n.kind), format!("{:?}", n.hitsound)))
        .collect();
    click(&mut game, &mut tm, painter, "behavior", &mut scroll);
    for behavior in [
        prpr::practice::NoteBehavior::Tap,
        prpr::practice::NoteBehavior::Drag,
        prpr::practice::NoteBehavior::Flick,
        prpr::practice::NoteBehavior::Original,
    ] {
        click(&mut game, &mut tm, painter, "practice_behavior", &mut scroll);
        assert_eq!(game.res.config.practice.behavior, behavior);
        if behavior != prpr::practice::NoteBehavior::Original {
            assert!(game
                .chart
                .lines
                .iter()
                .flat_map(|l| &l.notes)
                .all(|n| !matches!(n.kind, prpr::core::NoteKind::Hold { .. })));
        }
    }
    let restored: Vec<_> = game
        .chart
        .lines
        .iter()
        .flat_map(|l| &l.notes)
        .map(|n| (n.time, format!("{:?}", n.kind), format!("{:?}", n.hitsound)))
        .collect();
    for n in &originals {
        assert!(restored.contains(n), "restoring note behavior must retain each original Hold tail and hitsound");
    }
    input(&mut game, &mut tm, "practice_bookmark", "Hold 区间");
    input(&mut game, &mut tm, "practice_speed", "0.85");
    click(&mut game, &mut tm, painter, "bookmarks", &mut scroll);
    click(&mut game, &mut tm, painter, "load:0", &mut scroll);
    assert_eq!(game.res.config.speed, 1.25);
    assert!(game.res.config.practice.preserve_pitch);
    input(&mut game, &mut tm, "practice_scale", "60");
    input(&mut game, &mut tm, "practice_x", "0.3");
    input(&mut game, &mut tm, "practice_y", "-0.15");
    // A pass-through postprocess must not apply the reveal camera a second
    // time, nor mirror its already transformed framebuffer.
    fn pixels(game: &mut GameScene, tm: &mut TimeManager, painter: &mut TextPainter) -> Vec<u8> {
        render(game, tm, painter);
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (screen_width() * screen_height()) as usize * 4];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, screen_width() as i32, screen_height() as i32, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        bytes
    }
    game.practice_scroll_to(0.);
    let before = pixels(&mut game, &mut tm, painter);
    let mut identity = prpr::core::Effect::new(
        -100. ..100.,
        "#version 100\nprecision mediump float; varying lowp vec2 uv; uniform sampler2D screenTexture; void main(){gl_FragColor=texture2D(screenTexture,uv);}",
        vec![],
        false,
    )
    .unwrap();
    identity.update(&game.res);
    game.chart.extra.effects.push(identity);
    let after = pixels(&mut game, &mut tm, painter);
    let differences = before.iter().zip(&after).filter(|(a, b)| a.abs_diff(**b) > 2).count();
    assert!(differences < before.len() / 1000, "pass-through effect changed the reveal transform: {differences} channels");
    game.chart.extra.effects.clear();
    game.res.config.practice.ranges = true;
    game.res.config.judge_grading.detailed = true;
    game.res.windows = game.res.config.judge_windows();
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        game.practice_scroll_to(0.);
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        game.render(&mut tm, &mut ui).unwrap();
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/replay-qa/practice-advanced-{ratio}.png"));
    }
    // Collapse the drawer through its real edge button, then verify the normal
    // chart bounds while the reveal camera is zoomed out and panned.
    render(&mut game, &mut tm, painter);
    let drawer = game.practice_control_point("drawer").unwrap();
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Scene::touch(
            &mut game,
            &mut tm,
            &Touch {
                id: 781,
                phase,
                position: drawer,
                time: 0.,
            },
        )
        .unwrap();
    }
    real_clock.set(11.2);
    tm.resume();
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        game.render(&mut tm, &mut ui).unwrap();
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        }
        let frame = game.res.config.practice.view.visible_frame(game.res.aspect_ratio);
        let (vx, vy, vw, vh) = game.res.camera.viewport.unwrap();
        let x = (vx as f32 + (frame.center().x + 1.) * vw as f32 / 2.).round() as i32;
        let y = (vy as f32 + vh as f32 / 2. - frame.y * vw as f32 / 2.).round() as i32;
        let highlighted = (-3..=3).any(|dy| {
            let i = (((y + dy).clamp(0, h - 1) * w + x.clamp(0, w - 1)) * 4) as usize;
            bytes[i] > 150 && bytes[i + 1] > 120 && bytes[i + 2] < 120
        });
        assert!(highlighted, "normal visible area must have a highlighted frame at {ratio}");
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/replay-qa/practice-frame-{ratio}.png"));
    }
    game.pause(&mut tm).unwrap();
    render(&mut game, &mut tm, painter);
    let drawer = game.practice_control_point("drawer").unwrap();
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Scene::touch(
            &mut game,
            &mut tm,
            &Touch {
                id: 782,
                phase,
                position: drawer,
                time: 0.,
            },
        )
        .unwrap();
    }
    real_clock.set(11.8);
    game.practice_scroll_to(scroll);
    click(&mut game, &mut tm, painter, "delete:0", &mut scroll);
    let library: serde_json::Value =
        serde_json::from_slice(&std::fs::read(std::path::PathBuf::from(crate::dir::root().unwrap()).join("practice-bookmarks.json")).unwrap())
            .unwrap();
    assert!(library.as_object().unwrap().values().all(|list| list.as_array().unwrap().is_empty()), "bookmark deletion must persist");
    input(&mut game, &mut tm, "practice_ramp_start", "0.50");
    input(&mut game, &mut tm, "practice_ramp_step", "0.02");
    click(&mut game, &mut tm, painter, "ramp", &mut scroll);
    click(&mut game, &mut tm, painter, "practice_ramp", &mut scroll);
    click(&mut game, &mut tm, painter, "judgement", &mut scroll);
    click(&mut game, &mut tm, painter, "practice_inverse", &mut scroll);
    assert!((tm.speed - 0.5).abs() < 1e-6);
    tm.resume();
    tm.seek_to(5.3);
    game.update(&mut tm).unwrap();
    assert!((tm.speed - 0.52).abs() < 1e-6);
    assert!((game.res.config.speed - 0.52).abs() < 1e-6);
    assert!((game.res.windows.perfect - normal_windows.perfect / 0.52).abs() < 1e-6);
    println!("Practice: combo input, Hold tail, auto-loop touch and three aspect ratios passed");
}

// Exercise the recorder through the real GameScene state machine. Constructed
// replay tapes cannot catch a fade-out clearing the live recording.
async fn record_through_ending(info: ChartInfo, local_path: &str, recorded_config: Option<&Replay>) {
    let mut config = Config::default();
    if let Some(recorded) = recorded_config {
        if let Some(settings) = &recorded.settings {
            settings.apply(&mut config);
        }
    }
    config.mods = Mods::AUTOPLAY; // deterministic live hits, not a fabricated tape
    config.volume_music = 0.;
    config.volume_sfx = 0.;
    config.sample_count = 1;
    config.adjust_time = false;
    let hook = replay::recorder(Some(ChartRef::Local(local_path.into())), 0., 1., 0);
    let mut game = local(GameScene::new(
        GameMode::Normal,
        info,
        config,
        fs_from_path(local_path).unwrap(),
        None,
        BLACK_TEXTURE.clone(),
        BLACK_TEXTURE.clone(),
        None,
        Some(hook),
        None,
    ))
    .await
    .unwrap();
    let note_count = game.judge.result(false).num_of_notes as usize;
    let report_text = crate::plugins::reports()["example.density"].clone();
    let report: serde_json::Value = serde_json::from_str(&report_text).unwrap_or_else(|e| panic!("plugin report: {report_text}; {e}"));
    assert!(report["difficulty"].as_f64().unwrap() > 0., "real GameScene must invoke the installed analysis plugin");
    let mut tm = TimeManager::manual(Box::new(|| 10.));
    game.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    game.update(&mut tm).unwrap();
    tm.seek_to(0.);
    game.update(&mut tm).unwrap();
    tm.seek_to(1.1);
    game.update(&mut tm).unwrap();
    // A real retry must replace the partial first attempt, including its hit.
    game.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    game.update(&mut tm).unwrap();
    tm.seek_to(0.);
    game.update(&mut tm).unwrap();
    let end = game.res.track_length;
    for frame in 1..=((end + 1.4) * 120.) as usize {
        tm.seek_to(frame as f64 / 120.);
        game.update(&mut tm).unwrap();
        if frame == 180 {
            game.pause(&mut tm).unwrap();
            game.update(&mut tm).unwrap();
            tm.resume();
            game.resume(&mut tm).unwrap();
        }
    }
    assert!(game.res.time < end + 0.01, "fixture must have reached the ending clock clamp");
    let expected = game.judge.counts();
    let score = game.judge.score(false);
    assert_eq!(score, 1_000_000);
    let path = replay::save_recording(&format!("local:{local_path}"), 123457).unwrap();
    let recorded = replay::load(&path).unwrap();
    recorded.ensure_playable().unwrap();
    assert!(
        recorded.frames[0] < 0.01 && *recorded.frames.last().unwrap() > end,
        "ending must retain the complete attempt, including the final Playing frame"
    );
    assert_eq!(
        recorded.judges.iter().filter(|e| !matches!(e.kind, 5 | 6)).count(),
        note_count,
        "retry must not concatenate old hits and ending must not erase new hits"
    );
    // RETRY on the result page reuses this GameScene after save consumed PENDING.
    // This differs from retrying a partial attempt before the first save.
    for attempt in 0..2 {
        game.enter(&mut tm, None).unwrap();
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game.update(&mut tm).unwrap();
        tm.seek_to(0.);
        game.update(&mut tm).unwrap();
        for frame in 1..=((end + 1.4) * 120.) as usize {
            tm.seek_to(frame as f64 / 120.);
            game.update(&mut tm).unwrap();
        }
        let next =
            replay::save_recording(&format!("local:{local_path}"), 123459 + attempt).expect("result-page retry must create a new recording buffer");
        assert_eq!(next.extension().unwrap(), "ppr");
        let next = replay::load(&next).unwrap();
        assert_eq!(next.judges.len(), recorded.judges.len());
        assert_eq!(game.judge.counts(), expected);
        assert_eq!(next.meta.fingerprint, recorded.meta.fingerprint);
        assert!(replay::save_recording(&format!("local:{local_path}"), 123470).is_none(), "one attempt must only be saved once");
    }
    let export = path.with_file_name("complete-export.phirar");
    replay::save(&recorded, &export).unwrap();
    let (imported, _) = replay::library::import(&export).unwrap();
    let mut player = local(ReplayScene::new_bound(imported, local_path.into(), false)).await.unwrap();
    player.seek(end);
    assert_eq!(player.game_scene.judge.counts(), expected);
    assert_eq!(player.game_scene.judge.score(false), score);
    assert_eq!(player.game_scene.judge.real_time_accuracy(), 1.);
    player.seek(0.);
    assert_eq!(player.game_scene.judge.score(false), 0);
    player.seek(end);
    assert_eq!(player.game_scene.judge.score(false), score);
    println!("Live GameScene {local_path}: {note_count} notes, {} judge events, {} frames; retry, pause, ending clamp, export/import and playback score/counts matched",recorded.judges.len(),recorded.frames.len());
}

#[test]
#[ignore = "requires desktop OpenGL and audio; run separately with --ignored --test-threads=1"]
fn production_replay_transport_import_and_aspects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Replay regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            sample_count: 4,
            ..Default::default()
        },
        async {
            next_frame().await;
            std::fs::create_dir_all("target/replay-qa").unwrap();
            let isolated = tempfile::tempdir_in("target/replay-qa").unwrap();
            let old_root = DATA_PATH.lock().unwrap().replace(isolated.path().to_string_lossy().into_owned());
            macroquad::file::set_pc_assets_folder("assets");
            prpr::ui::set_multilingual_fallback(FontArc::try_from_vec(std::fs::read("assets/font.ttf").unwrap()).unwrap());
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|v| *v.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            BOLD_FONT.with(|v| {
                *v.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font)))
            });
            let info = chart_fixture(isolated.path(), "renamed-chart");
            let mut data = Data::default();
            data.config.volume_music = 0.;
            data.config.volume_sfx = 0.;
            data.charts.push(LocalChart {
                info: info.clone().into(),
                local_path: "renamed-chart".into(),
                record: None,
                mods: Default::default(),
                played_unlock: false,
            });
            crate::set_data(data);
            let mut fs = fs_from_path("renamed-chart").unwrap();
            let fingerprint = local(replay::library::fingerprint(fs.as_mut(), &info)).await.unwrap();
            let mut tape = Replay {
                chart: Some(ChartRef::Local("old-name".into())),
                speed: 1.,
                has_speed: true,
                aspect_ratio: Some(16. / 9.),
                has_diffs: true,
                settings: Some(crate::judgement_presets::JudgeSettings::default()),
                frames: vec![0., 0.006, 0.017, 0.04, 1., 2., 3., 4., 4.8, 5., 6., 8., 12.],
                meta: Metadata {
                    id: "qa-other-player".into(),
                    player: Some("Other Player".into()),
                    name: info.name.clone(),
                    fingerprint: Some(fingerprint),
                    result: Some(crate::history::Record {
                        key: "local:old-name".into(),
                        score: 500000,
                        accuracy: 0.5,
                        num_of_notes: 4,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            for (t, note, kind, diff) in [
                (1., 0, 4, -0.012),
                (2., 1, 5, 0.018),
                (4.8, 1, 0, 0.018),
                (6., 2, 2, 0.210),
                (8., 3, 3, 0.),
            ] {
                tape.judges.push(JudgeEvent {
                    t,
                    note,
                    kind,
                    diff,
                    ..Default::default()
                });
            }
            // Distinct, very large IDs; another finger's next event must not affect interpolation.
            for (t, id, x, phase) in [
                (1., u64::MAX, 0., 0),
                (1., 256, 0.6, 0),
                (1.010, 900, -0.5, 0),
                (1.011, 900, -0.5, 2),
                (2., 256, 0.8, 1),
                (3., u64::MAX, 0.2, 1),
                (4., 256, 0.8, 2),
                (5., u64::MAX, 0.2, 2),
            ] {
                tape.touches.push(TouchEvent { t, id, x, phase, y: 0.1 });
            }
            tape.meta.audio_fingerprint = Some(local(replay::library::audio_fingerprint(fs.as_mut(), &info)).await.unwrap());
            let music_path = isolated.path().join("data/charts/renamed-chart/song.wav");
            let music_bytes = std::fs::read(&music_path).unwrap();
            std::fs::OpenOptions::new()
                .append(true)
                .open(&music_path)
                .unwrap()
                .write_all(b"different")
                .unwrap();
            assert_eq!(local(replay::library::match_chart(&tape, Some("renamed-chart"), &[])).await.unwrap(), replay::library::Match::WrongAudio);
            std::fs::write(&music_path, &music_bytes).unwrap();
            let source = isolated.path().join("foreign.phirar");
            replay::save(&tape, &source).unwrap();
            let (path, added) = replay::library::import(&source).unwrap();
            assert!(added);
            assert!(!replay::library::import(&source).unwrap().1);
            assert_eq!(
                local(replay::library::match_chart(&tape, None, &["renamed-chart".into()])).await.unwrap(),
                replay::library::Match::Found("renamed-chart".into(), true)
            );
            replay::library::bind(&path, "renamed-chart", true).unwrap();
            let board = replay::library::board_records("local:renamed-chart");
            assert_eq!(board.len(), 1);
            assert!(board[0].replay_imported);
            assert_eq!(board[0].replay_player.as_deref(), Some("Other Player"));
            assert!(!isolated.path().join("data/history.json").exists());
            let mut scene = local(ReplayScene::new(path.clone())).await.unwrap();
            let clock = std::rc::Rc::new(std::cell::Cell::new(10.));
            let source_clock = clock.clone();
            let mut tm = TimeManager::manual(Box::new(move || source_clock.get()));
            Scene::enter(&mut scene, &mut tm, None).unwrap();
            // Exercise rendered hit regions, not only control() calls.
            scene.expanded = true;
            scene.seek(0.017);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "buttons-before");
            assert_eq!(prpr::ext::get_viewport(), (0, 0, 1280, 720));
            for (index, position, expected) in [
                (4, vec2(-0.594, 0.308), 0.006),
                (5, vec2(-0.356, 0.308), 0.017),
                (13, vec2(0.831, 0.128), 5.),
                (13, vec2(0.831, 0.128), 7.),
                (12, vec2(0.594, 0.128), 5.),
            ] {
                click(&mut scene, &mut tm, position);
                assert_eq!(scene.current, expected, "button {index}");
            }
            let original_frames = std::mem::replace(&mut scene.replay.frames, (0..=1440).map(|i| i as f64 / 120.).collect());
            scene.seek(3.);
            for (index, column, frame) in [(3, 0, 350), (4, 1, 349), (5, 2, 350), (6, 3, 360)] {
                click_analysis(&mut scene, &mut tm, index, column, false);
                assert!((scene.current - frame as f64 / 120.).abs() < 1e-9);
                assert!(!scene.playing && !scene.frame_fallback);
            }
            scene.replay.frames = vec![0., 12.];
            click_analysis(&mut scene, &mut tm, 5, 2, false);
            assert!((scene.current - (3. + 1. / 60.)).abs() < 1e-9 && scene.frame_fallback);
            click_analysis(&mut scene, &mut tm, 4, 1, false);
            assert!((scene.current - 3.).abs() < 1e-9);
            scene.replay.frames = original_frames;
            click_analysis(&mut scene, &mut tm, 7, 4, false);
            assert_eq!(scene.a, Some(3.));
            scene.seek(5.);
            click_analysis(&mut scene, &mut tm, 8, 5, false);
            assert_eq!(scene.b, Some(5.));
            assert!(scene.looping);
            click_analysis(&mut scene, &mut tm, 9, 6, false);
            assert!(!scene.looping);
            click_analysis(&mut scene, &mut tm, 18, 7, false);
            assert!(scene.a.is_none() && scene.b.is_none());
            click_analysis(&mut scene, &mut tm, 14, 4, true);
            assert!(scene.ranges && scene.game_scene.res.config.chart_debug_note);
            click_analysis(&mut scene, &mut tm, 14, 4, true);
            assert!(!scene.ranges);
            click_analysis(&mut scene, &mut tm, 20, 6, true);
            assert!(scene.info_mode);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "info-open-by-button");
            click_analysis(&mut scene, &mut tm, 20, 6, false);
            assert!(!scene.info_mode);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "analysis-restored-by-button");
            // Open the actual rate popup and choose its first option.
            click(&mut scene, &mut tm, panel_cell(5, 1, 0.0945, 0.067).center());
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "rate-opening");
            clock.set(clock.get() + 0.5);
            Scene::update(&mut scene, &mut tm).unwrap();
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "rate-open");
            let popup = scene.rate.popup_rect();
            click(&mut scene, &mut tm, vec2(popup.center().x, popup.y + 0.05));
            assert_eq!(scene.speed, 0.1);
            clock.set(clock.get() + 0.5);
            Scene::update(&mut scene, &mut tm).unwrap();
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "rate-selected");
            click(&mut scene, &mut tm, panel_cell(1, 1, 0.0945, 0.067).center());
            assert!(scene.playing);
            let before = scene.current;
            clock.set(clock.get() + 0.125);
            Scene::update(&mut scene, &mut tm).unwrap();
            assert!((scene.current - before - 0.0125).abs() < 1e-8);
            click(&mut scene, &mut tm, panel_cell(1, 1, 0.0945, 0.067).center());
            assert!(!scene.playing && scene.game_scene.music.paused());
            scene.rate.set_selected(4);
            scene.speed = 1.;
            scene.seek(3.);
            click(&mut scene, &mut tm, panel_cell(4, 1, 0.0945, 0.067).center());
            assert_eq!(scene.current, 8.);
            click(&mut scene, &mut tm, panel_cell(3, 1, 0.0945, 0.067).center());
            assert_eq!(scene.current, 3.);
            click(&mut scene, &mut tm, panel_cell(2, 1, 0.0945, 0.067).center());
            assert_eq!(scene.current, 0.);
            let hide = panel_cell(7, 1, -720. / 1280. + 0.065, 0.055).center();
            click(&mut scene, &mut tm, hide);
            assert!(!scene.controls_visible);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "hidden-by-button");
            let previous_size = scene.game_scene.res.config.touch_point_size;
            click(&mut scene, &mut tm, panel_cell(2, 1, 0.3545, 0.067).center());
            assert_eq!(scene.game_scene.res.config.touch_point_size, previous_size, "hidden controls must not react");
            click(&mut scene, &mut tm, hide);
            assert!(scene.controls_visible);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "shown-by-button");
            scene.seek(1.5);
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-before");
            let on = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-on");
            let fitted = scene.game_scene.res.camera.viewport.unwrap();
            let pos = scene.visible_touches().iter().find(|p| p.0 == 1).unwrap().1;
            let x = (fitted.0 as f32 + (pos.x + 1.) * fitted.2 as f32 / 2.).round() as i32;
            let y = (fitted.1 as f32 + fitted.3 as f32 / 2. - pos.y * fitted.2 as f32 / 2.).round() as i32;
            click_analysis(&mut scene, &mut tm, 10, 0, true);
            assert!(!scene.fingers);
            let off = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-off");
            assert!(pixel(&on, x, y)[2] > pixel(&off, x, y)[2] + 20, "touch toggle must change rendered chart pixels");
            click_analysis(&mut scene, &mut tm, 10, 0, true);
            click_analysis(&mut scene, &mut tm, 15, 1, true);
            assert!(scene.fingers && scene.finger_ids);
            let ids = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-ids");
            let changed_chart_pixels = (fitted.1..fitted.1 + fitted.3)
                .flat_map(|y| (fitted.0..fitted.0 + fitted.2).map(move |x| (x, y)))
                .filter(|&(x, y)| pixel(&on, x, y) != pixel(&ids, x, y))
                .count();
            assert!(changed_chart_pixels > 20, "finger IDs must be visible in the chart");
            let size = scene.game_scene.res.config.touch_point_size;
            scene.finger_ids = false;
            let before_size = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-size-before");
            scene.fingers = false;
            click_analysis(&mut scene, &mut tm, 16, 2, true);
            assert_ne!(scene.game_scene.res.config.touch_point_size, size);
            assert!(scene.fingers, "size adjustment must show the touch being adjusted");
            let after_size = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-size-after");
            let blue_pixels = |image: &Image| {
                (y - 40..y + 40)
                    .flat_map(|y| (x - 40..x + 40).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let p = pixel(image, x, y);
                        p[2] as u16 > p[0] as u16 + 20 && p[2] as u16 > p[1] as u16 + 20
                    })
                    .count()
            };
            assert_ne!(blue_pixels(&before_size), blue_pixels(&after_size), "size must change the rendered touch area");
            let alpha = scene.game_scene.res.config.touch_point_alpha;
            click_analysis(&mut scene, &mut tm, 17, 3, true);
            assert_ne!(scene.game_scene.res.config.touch_point_alpha, alpha);
            let after_alpha = screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "touch-alpha-after");
            assert_ne!(pixel(&after_size, x, y), pixel(&after_alpha, x, y), "opacity must change actual touch pixels");
            scene.seek(1.03);
            assert!(scene.visible_touches().iter().any(|p| p.0 == 3 && p.2 > 0.));
            scene.seek(1.2);
            assert!(!scene.visible_touches().iter().any(|p| p.0 == 3));
            scene.seek(1.5);
            assert_eq!(scene.game_scene.judge.counts()[4], 1);
            scene.apply_touches(2.);
            let &(pos, index) = scene.current_touches.get(&u64::MAX).unwrap();
            assert!((scene.touch_position(pos, index, 2.).x - 0.1).abs() < 0.001);
            scene.seek(3.);
            assert!(matches!(scene.game_scene.chart.lines[0].notes[1].judge, JudgeStatus::Hold(..)));
            scene.checkpoint();
            scene.seek(5.5);
            scene.checkpoint();
            scene.seek(8.5);
            let counts = scene.game_scene.judge.counts();
            assert_eq!(counts, [1, 0, 1, 1, 1, 0, 0, 0]);
            let score = scene.game_scene.judge.score(false);
            assert_eq!(score, tape.meta.result.as_ref().unwrap().score);
            let last_time = scene.replay.judges.last().unwrap().t;
            scene.replay.judges.last_mut().unwrap().t = 12.2;
            assert!(scene.end() > 12.2);
            scene.seek(scene.end());
            assert_eq!(scene.game_scene.judge.counts(), counts);
            scene.replay.judges.last_mut().unwrap().t = last_time;
            scene.seek(1.5);
            assert_eq!(scene.game_scene.judge.counts().iter().sum::<u32>(), 1);
            scene.seek(8.5);
            assert_eq!(scene.game_scene.judge.counts(), counts);
            assert_eq!(scene.game_scene.judge.score(false), score);
            scene.seek(3.);
            scene.control(7);
            scene.seek(5.);
            scene.control(8);
            assert!(scene.looping);
            scene.seek(4.99);
            scene.playing = true;
            scene.last_t = tm.real_time() - 0.2;
            Scene::update(&mut scene, &mut tm).unwrap();
            assert!(scene.current >= 3. && scene.current < 3.3);
            scene.playing = false;
            Scene::pause(&mut scene, &mut tm).unwrap();
            Scene::resume(&mut scene, &mut tm).unwrap();
            assert!(!scene.playing);
            scene.control(18);
            scene.seek(0.017);
            scene.control(5);
            Scene::update(&mut scene, &mut tm).unwrap();
            assert_eq!(scene.current, 0.04);
            for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
                scene.seek(2.5);
                scene.expanded = false;
                screenshot(&mut painter, &mut scene, &mut tm, w, h, &format!("player-{ratio}"));
                scene.expanded = true;
                scene.ranges = true;
                scene.finger_ids = true;
                Scene::update(&mut scene, &mut tm).unwrap();
                screenshot(&mut painter, &mut scene, &mut tm, w, h, &format!("analysis-{ratio}"));
            }
            scene.expanded = false;
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 720, "drag-before");
            let p = vec2(scene.timeline.x + scene.timeline.w * 0.5, scene.timeline.center().y);
            for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
                Scene::touch(
                    &mut scene,
                    &mut tm,
                    &Touch {
                        id: 99,
                        phase,
                        position: p,
                        time: 0.,
                    },
                )
                .unwrap();
            }
            Scene::update(&mut scene, &mut tm).unwrap();
            assert!(!scene.playing);
            assert!((scene.current - 6.).abs() < 0.01);
            scene.info_mode = true;
            scene.expanded = true;
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 548, "recording-info-21x9");
            scene.controls_visible = false;
            screenshot(&mut painter, &mut scene, &mut tm, 1280, 548, "controls-hidden-21x9");
            scene.controls_visible = true;
            // Recording the live engine queue preserves the actual (non-zero) signed offset.
            let mut live = prpr::judge::Judge::new(&scene.game_scene.chart, false);
            live.set_grading(scene.game_scene.res.config.judge_grading);
            let mut hook = replay::recorder(Some(ChartRef::Local("renamed-chart".into())), 0., 1., 0);
            live.commit(1., TJ::Perfect, 0, 0, -0.019);
            hook(1., &mut scene.game_scene.res, &mut live);
            hook(1.017, &mut scene.game_scene.res, &mut live);
            hook(0.999, &mut scene.game_scene.res, &mut live);
            let recorded_path = replay::save_recording("local:renamed-chart", 123456).unwrap();
            let recorded = replay::load(&recorded_path).unwrap();
            assert!(recorded.has_diffs && recorded.has_speed);
            assert_eq!(recorded.judges[0].diff, -0.019);
            assert_eq!(recorded.frames, [1., 1.017, 1.017]);
            assert_eq!(recorded.settings, Some(crate::judgement_presets::JudgeSettings::capture(&scene.game_scene.res.config)));
            // Reset is authoritative even if its new clock is later than the
            // prior attempt (for example retry-from-here).
            live.reset();
            live.commit(2., TJ::Good, 0, 2, 0.05);
            let mut hook = replay::recorder(None, 0., 1., 0);
            hook(1., &mut scene.game_scene.res, &mut live);
            live.reset();
            live.commit(2., TJ::Perfect, 0, 3, 0.);
            hook(2., &mut scene.game_scene.res, &mut live);
            let retried = replay::load(&replay::save_recording("retry", 123458).unwrap()).unwrap();
            assert_eq!(retried.judges.len(), 1);
            assert_eq!(retried.judges[0].note, 3);
            let plugin_package = isolated.path().join("density.zip");
            let mut archive = zip::ZipWriter::new(std::fs::File::create(&plugin_package).unwrap());
            for (name, body) in [
                ("manifest.json", include_str!("../../../../docs/plugins/examples/density/manifest.json")),
                ("main.rhai", include_str!("../../../../docs/plugins/examples/density/main.rhai")),
            ] {
                archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                archive.write_all(body.as_bytes()).unwrap();
            }
            archive.finish().unwrap();
            crate::plugins::import(&plugin_package).unwrap();
            crate::plugins::change("example.density", Some(true), None, false).unwrap();
            crate::plugins::init();
            record_through_ending(info.clone(), "renamed-chart", None).await;
            practice_regression(info.clone(), &mut painter).await;
            if let (Ok(tape_path), Ok(chart_path)) = (std::env::var("REPLAY_QA_TAPE"), std::env::var("REPLAY_QA_CHART")) {
                let provided = replay::load(std::path::Path::new(&tape_path)).unwrap();
                println!(
                    "Provided replay: {} judges, {} touches, {} frames; original score {:?}",
                    provided.judges.len(),
                    provided.touches.len(),
                    provided.frames.len(),
                    provided.meta.result.as_ref().map(|r| r.score)
                );
                let (imported_chart, _) = local(crate::scene::import_chart(std::fs::File::open(chart_path).unwrap())).await.unwrap();
                let local_path = imported_chart.local_path.as_str();
                let mut fs = fs_from_path(local_path).unwrap();
                let info = local(prpr::fs::load_info(fs.as_mut())).await.unwrap();
                if let Some(expected) = &provided.meta.fingerprint {
                    assert_eq!(&local(replay::library::fingerprint(fs.as_mut(), &info)).await.unwrap(), expected);
                }
                if let Some(expected) = &provided.meta.audio_fingerprint {
                    assert_eq!(&local(replay::library::audio_fingerprint(fs.as_mut(), &info)).await.unwrap(), expected);
                }
                if provided.judges.is_empty() {
                    assert!(local(ReplayScene::new_bound(PathBuf::from(tape_path), local_path.into(), true))
                        .await
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("判定事件"));
                }
                record_through_ending(info, local_path, Some(&provided)).await;
            }
            // Historical packages are verified before registration; source chart and PB stay untouched.
            let mut package = std::io::Cursor::new(Vec::new());
            crate::scene::compress_folder(&isolated.path().join("data/charts/renamed-chart"), &mut package).unwrap();
            let mut package_file = tempfile::tempfile().unwrap();
            package_file.write_all(package.get_ref()).unwrap();
            std::io::Seek::rewind(&mut package_file).unwrap();
            let archived = local(replay::library::install_archived(&tape, package_file)).await.unwrap();
            assert!(archived.info.id.is_none() && archived.record.is_none());
            assert_ne!(archived.local_path, "renamed-chart");
            let mut archived_fs = fs_from_path(&archived.local_path).unwrap();
            let archived_info = local(prpr::fs::load_info(archived_fs.as_mut())).await.unwrap();
            assert_eq!(
                local(replay::library::fingerprint(archived_fs.as_mut(), &archived_info)).await.unwrap(),
                tape.meta.fingerprint.clone().unwrap()
            );
            assert!(crate::get_data().charts[0].record.is_none());
            let mut state = local(crate::page::SharedState::new(FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap()))
                .await
                .unwrap();
            crate::page::replays::render_regression(&mut painter, &path, &mut state);
            crate::page::plugins::render_regression(&mut painter, &mut state);
            // Content drift cannot silently bind to the same location.
            std::fs::OpenOptions::new()
                .append(true)
                .open(isolated.path().join("data/charts/renamed-chart/chart.json"))
                .unwrap()
                .write_all(b" ")
                .unwrap();
            assert_eq!(local(replay::library::match_chart(&tape, Some("renamed-chart"), &[])).await.unwrap(), replay::library::Match::WrongVersion);
            drop(scene);
            prpr::ui::cleanup_audio();
            *DATA_PATH.lock().unwrap() = old_root;
            println!("Replay: real frames, independent fingers, holds, cached seeks, pause, AB loop, chart drift, import deduplication and 3 aspect ratios passed");
        },
    );
}
