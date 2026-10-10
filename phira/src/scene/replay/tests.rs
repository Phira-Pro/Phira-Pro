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
        Scene::touch(scene, tm, &Touch { id: 99, phase, position, time: 0. }).unwrap();
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
    &image.bytes[i..i+3]
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
            for (index, column, frame) in [(3,0,350), (4,1,349), (5,2,350), (6,3,360)] {
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
            let pos = scene.visible_touches().iter().find(|p| p.0 == u64::MAX).unwrap().1;
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
            let changed_chart_pixels = (fitted.1..fitted.1 + fitted.3).flat_map(|y| (fitted.0..fitted.0 + fitted.2).map(move |x| (x,y)))
                .filter(|&(x,y)| pixel(&on,x,y) != pixel(&ids,x,y)).count();
            assert!(changed_chart_pixels > 20, "finger IDs must be visible in the chart");
            let size = scene.game_scene.res.config.touch_point_size;
            click_analysis(&mut scene, &mut tm, 16, 2, true);
            assert_ne!(scene.game_scene.res.config.touch_point_size, size);
            let alpha = scene.game_scene.res.config.touch_point_alpha;
            click_analysis(&mut scene, &mut tm, 17, 3, true);
            assert_ne!(scene.game_scene.res.config.touch_point_alpha, alpha);
            scene.seek(1.03);
            assert!(scene.visible_touches().iter().any(|p| p.0 == 900 && p.2 > 0.));
            scene.seek(1.2);
            assert!(!scene.visible_touches().iter().any(|p| p.0 == 900));
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
            let recorded_path = replay::save_recording("local:renamed-chart", 123456).unwrap();
            let recorded = replay::load(&recorded_path).unwrap();
            assert!(recorded.has_diffs && recorded.has_speed);
            assert_eq!(recorded.judges[0].diff, -0.019);
            assert_eq!(recorded.frames, [1., 1.017]);
            assert_eq!(recorded.settings, Some(crate::judgement_presets::JudgeSettings::capture(&scene.game_scene.res.config)));
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
            let mut state = local(crate::page::SharedState::new(FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap())).await.unwrap();
            crate::page::replays::render_regression(&mut painter, &path, &mut state);
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
