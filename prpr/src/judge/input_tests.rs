//! Synthetic input only; no player charts, recordings or saved settings.
use super::*;
use crate::core::{Anim, AnimFloat, BpmList, ChartExtra, JudgeLine, JudgeLineCache, Note, Object};

fn chart(notes: Vec<(f64, NoteKind)>) -> Chart {
    let mut notes: Vec<_> = notes
        .into_iter()
        .map(|(time, kind)| Note {
            object: Object::default(),
            kind,
            hitsound: HitSound::None,
            time,
            height: time,
            speed: 1.,
            color: WHITE,
            fx_color: None,
            judge_area: 1.,
            above: true,
            multiple_hint: false,
            fake: false,
            judge: JudgeStatus::NotJudged,
        })
        .collect();
    let cache = JudgeLineCache::new(&mut notes);
    let line = JudgeLine {
        object: Object::default(),
        ctrl_obj: RefCell::default(),
        kind: Default::default(),
        height: AnimFloat::fixed(0.),
        incline: AnimFloat::fixed(0.),
        notes,
        color: Anim::default(),
        parent: None,
        rot_with_parent: false,
        z_index: 0,
        show_below: true,
        attach_ui: None,
        cache,
    };
    Chart::new(0., vec![line], BpmList::new(vec![(0., 120.)]), Default::default(), ChartExtra::default(), Default::default())
}

fn touch(id: u64, phase: TouchPhase, position: Vec2) -> Touch {
    Touch {
        id,
        phase,
        position,
        time: f64::NEG_INFINITY,
    }
}

#[test]
fn keyboard_snapshots_exclude_pause_repeats_and_unmatched_releases() {
    let mut input = TouchStatus::default();
    input.press_key(KeyCode::Space, false);
    assert_eq!(input.keyboard_state(true, &Default::default()), (0, 0));
    input.press_key(KeyCode::A, false);
    input.press_key(KeyCode::A, false);
    input.press_key(KeyCode::A, true);
    input.press_key(KeyCode::S, false);
    assert_eq!(input.keyboard_state(true, &Default::default()), (2, 2));
    assert_eq!(input.keyboard_state(false, &Default::default()), (0, 0));
    let mut input = input.next_frame();
    assert_eq!(input.keyboard_state(true, &Default::default()), (0, 2));
    input.release_key(KeyCode::A);
    input.release_key(KeyCode::A);
    input.release_key(KeyCode::Space);
    assert_eq!(input.keyboard_state(true, &Default::default()), (0, 1));
    input.release_key(KeyCode::S);
    input.press_key(KeyCode::Space, false);
    assert_eq!(input.keyboard_state(true, &Default::default()), (0, 0));
    // A complete quick tap still has a click edge but no held finger.
    input.press_key(KeyCode::A, false);
    input.release_key(KeyCode::A);
    assert_eq!(input.keyboard_state(true, &Default::default()), (1, 0));
    assert_eq!(input.next_frame().keyboard_state(true, &Default::default()), (0, 0));
}

#[test]
fn paused_releases_clear_mouse_and_touch_without_clearing_another_contact() {
    let chart = chart(Vec::new());
    let mut judge = Judge::new(&chart, false);
    let mouse = button_to_id(MouseButton::Left);
    for id in [mouse, 71, 72, 73] {
        judge.blocked_finger_ids.insert(id);
        judge.trackers.insert(id, FlickTracker::new(200, 1., Point::origin()));
    }
    let mut input = TouchStatus::default();
    input.press_key(KeyCode::A, false);
    input.press_key(KeyCode::Space, false);
    TOUCHES.with(|s| *s.borrow_mut() = input.next_frame());
    judge.key_down_count = 1;
    judge.observe_paused_input();
    assert_eq!(judge.key_down_count, 0);
    TOUCHES.with(|s| {
        let mut input = s.borrow_mut();
        input.release_key(KeyCode::A);
        input.release_key(KeyCode::Space);
        input.touches = vec![
            touch(mouse, TouchPhase::Ended, Vec2::ZERO),
            touch(71, TouchPhase::Ended, Vec2::ZERO),
            touch(72, TouchPhase::Cancelled, Vec2::ZERO),
            touch(73, TouchPhase::Stationary, Vec2::ZERO),
        ];
    });
    judge.observe_paused_input();
    assert_eq!(judge.blocked_finger_ids, HashSet::from([73]));
    assert_eq!(judge.trackers.keys().copied().collect::<HashSet<_>>(), HashSet::from([73]));
    TOUCHES.with(|s| {
        let mut input = s.borrow().next_frame();
        input.press_key(KeyCode::Space, false);
        assert_eq!(input.keyboard_state(true, &Default::default()), (0, 0));
        *s.borrow_mut() = input;
    });
    judge.observe_paused_input();
    assert_eq!(judge.key_down_count, 0);
    TOUCHES.with(|s| *s.borrow_mut() = TouchStatus::default());
}

async fn finish<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = crate::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn pause_release_sequence_has_no_phantom_fingers_or_revived_holds() {
    use crate::{dir::Dir, fs::ExternalFileSystem, info::ChartInfo};
    use std::sync::Arc;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Pause input regression".into(),
            window_width: 1280,
            window_height: 720,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            let directory = tempfile::tempdir_in("target").unwrap();
            std::fs::write(directory.path().join("music.ogg"), include_bytes!("../../../assets/ending.ogg")).unwrap();
            for (course, algorithm) in [
                (false, JudgeAlgorithm::PhiraPro),
                (false, JudgeAlgorithm::Phigros),
                (true, JudgeAlgorithm::Phigros),
            ] {
                for keyboard in [false, true] {
                    let mut config = Config::default();
                    config.judge_algorithm = algorithm;
                    config.use_keyboard = keyboard;
                    if course {
                        config = crate::course::config(&config);
                    }
                    config.volume_music = 0.;
                    config.volume_sfx = 0.;
                    config.particle = false;
                    config.sample_count = 1;
                    let info = ChartInfo {
                        music: "music.ogg".into(),
                        aspect_ratio: 16. / 9.,
                        ..Default::default()
                    };
                    let fs = ExternalFileSystem(Arc::new(Dir::new(directory.path()).unwrap()));
                    let image: crate::ext::SafeTexture = Texture2D::from_rgba8(1, 1, &[20, 20, 20, 255]).into();
                    let mut res = finish(Resource::new(config, info, Box::new(fs), None, image.clone(), image, true))
                        .await
                        .unwrap();
                    let vp = (0, 0, screen_width() as i32, screen_height() as i32);
                    res.update_size(vp);
                    unsafe { get_internal_gl() }.quad_gl.viewport(Some(vp));
                    let position = vec2(screen_width() / 2., screen_height() / 2.);
                    let mouse = button_to_id(MouseButton::Left);
                    let mut chart = chart(vec![
                        (
                            1.,
                            NoteKind::Hold {
                                end_time: 3.,
                                end_height: 3.,
                            },
                        ),
                        (1.4, NoteKind::Drag),
                        (1.5, NoteKind::Flick),
                        (2.5, NoteKind::Click),
                    ]);
                    let mut judge = Judge::new(&chart, false);
                    let mut bad = Vec::new();
                    let mut input = TouchStatus::default();
                    input.press_key(KeyCode::A, false);
                    input.touches.push(touch(mouse, TouchPhase::Started, position));
                    TOUCHES.with(|s| *s.borrow_mut() = input);
                    res.time = 1.;
                    judge.update(&mut res, &mut chart, &mut bad);
                    assert!(matches!(chart.lines[0].notes[0].judge, JudgeStatus::Hold(..)), "Hold head: {course}/{algorithm:?}/{keyboard}");

                    // The pause edge is processed after the last playing update.
                    TOUCHES.with(|s| {
                        let mut input = s.borrow().next_frame();
                        input.press_key(KeyCode::Space, false);
                        input.touches.push(touch(mouse, TouchPhase::Stationary, position));
                        *s.borrow_mut() = input;
                    });
                    res.time = 1.01;
                    judge.update(&mut res, &mut chart, &mut bad);
                    // All gameplay sources are released while judgement is paused.
                    TOUCHES.with(|s| {
                        let mut input = s.borrow().next_frame();
                        input.release_key(KeyCode::A);
                        input.release_key(KeyCode::Space);
                        input.touches = vec![touch(mouse, TouchPhase::Ended, position), touch(71, TouchPhase::Ended, position)];
                        *s.borrow_mut() = input;
                    });
                    judge.observe_paused_input();
                    TOUCHES.with(|s| {
                        let mut input = s.borrow().next_frame();
                        input.press_key(KeyCode::Space, false);
                        *s.borrow_mut() = input;
                    });
                    judge.observe_paused_input();
                    // Keep the resume key held for the first playing frames.
                    for frame in 2..=100 {
                        TOUCHES.with(|s| {
                            let input = s.borrow().next_frame();
                            *s.borrow_mut() = input;
                        });
                        res.time = 1. + frame as f64 / 100.;
                        judge.update(&mut res, &mut chart, &mut bad);
                        assert_eq!(judge.key_down_count, 0, "phantom keyboard: {course}/{algorithm:?}/{keyboard}");
                    }
                    assert_eq!(judge.counts()[Judgement::Miss as usize], 3, "released Hold/Drag/Flick: {course}/{algorithm:?}/{keyboard}");
                    assert!(chart.lines[0].notes[..3].iter().all(|note| matches!(note.judge, JudgeStatus::Judged)));

                    // A real key can hit the next note, never revive the missed Hold.
                    TOUCHES.with(|s| {
                        let mut input = s.borrow().next_frame();
                        input.release_key(KeyCode::Space);
                        input.press_key(KeyCode::A, false);
                        *s.borrow_mut() = input;
                    });
                    res.time = 2.5;
                    judge.update(&mut res, &mut chart, &mut bad);
                    assert_eq!(judge.combo(), u32::from(keyboard), "keyboard preference: {course}/{algorithm:?}");
                    assert!(matches!(chart.lines[0].notes[0].judge, JudgeStatus::Judged));
                    assert_eq!(judge.counts()[Judgement::Miss as usize], 3);
                    TOUCHES.with(|s| {
                        let mut input = s.borrow().next_frame();
                        input.release_key(KeyCode::A);
                        *s.borrow_mut() = input;
                    });
                    res.time = 3.6;
                    judge.update(&mut res, &mut chart, &mut bad);
                    assert_eq!(judge.key_down_count, 0);
                    assert_eq!(judge.counts().iter().sum::<u32>(), 4, "each note scores once");
                    assert_eq!(judge.counts()[Judgement::Miss as usize], if keyboard { 3 } else { 4 });
                    assert_eq!(judge.judgements.borrow().iter().filter(|event| event.2 == 0 && event.3.is_err()).count(), 1, "Hold has one head");
                    TOUCHES.with(|s| *s.borrow_mut() = TouchStatus::default());
                }
            }
        },
    );
}

#[test]
fn repeat_flag_after_focus_loss_recovers_keyboard_on_each_stage() {
    for _stage in 0..3 {
        let mut input = TouchStatus::default();
        input.press_key(KeyCode::A, true);
        assert_eq!(input.keyboard_state(true, &Default::default()), (1, 1));
        let mut input = input.next_frame();
        input.press_key(KeyCode::A, true);
        assert_eq!(input.keyboard_state(true, &Default::default()), (0, 1));
        input.release_key(KeyCode::A);
        input.press_key(KeyCode::A, true);
        assert_eq!(input.keyboard_state(true, &Default::default()), (1, 1));
    }
}

#[test]
fn remapped_pause_and_disabled_keys_never_judge_and_space_can_be_assigned() {
    let mut input = TouchStatus::default();
    let mut bindings = crate::hotkeys::Bindings::default();
    bindings.set(KeyCode::A, crate::hotkeys::Action::Pause);
    bindings.set(KeyCode::S, crate::hotkeys::Action::Disabled);
    bindings.set(KeyCode::Space, crate::hotkeys::Action::Hit);
    input.press_key(KeyCode::A, false);
    input.press_key(KeyCode::S, false);
    input.press_key(KeyCode::Space, false);
    assert_eq!(input.keyboard_state(true, &bindings), (1, 1));
    assert_eq!(input.keyboard_state(false, &bindings), (0, 0));
}
