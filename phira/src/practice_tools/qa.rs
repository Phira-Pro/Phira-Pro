//! Exercise actual selectors, replay clocks, render targets and density interval return.
use super::*;
async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut f = Box::pin(future);
    loop {
        if let Some(v) = poll_future(f.as_mut()) {
            return v;
        }
        next_frame().await;
    }
}
pub(crate) fn capture(ui: &mut Ui, name: &str) {
    unsafe { get_internal_gl() }.flush();
    let (_, _, w, h) = ui.viewport;
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0, "{name}");
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
    .export_png(&format!("target/replay-qa/{name}.png"));
}
fn tap(scene: &mut AnalysisScene, tm: &mut TimeManager, p: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        scene
            .touch(
                tm,
                &Touch {
                    id: 705,
                    phase,
                    position: p,
                    time: 0.,
                },
            )
            .unwrap();
    }
}
pub(crate) async fn run(painter: &mut prpr::ui::TextPainter, path: &std::path::Path) {
    use sha2::{Digest, Sha256};
    let mut fs = fs_from_path("renamed-chart").unwrap();
    let info = local(prpr::fs::load_info(fs.as_mut())).await.unwrap();
    let bytes = local(GameBytes::read(fs.as_mut(), &info)).await.unwrap();
    let (chart, _, _) = local(prpr::scene::GameScene::load_chart(fs.as_mut(), &info)).await.unwrap();
    let notes: Vec<Note> = chart
        .lines
        .iter()
        .enumerate()
        .flat_map(|(l, line)| {
            line.notes.iter().enumerate().map(move |(i, n)| Note {
                line: l as u32,
                index: i as u32,
                time: n.time,
                end: if let prpr::core::NoteKind::Hold { end_time, .. } = n.kind {
                    end_time
                } else {
                    n.time
                },
                kind: match n.kind {
                    prpr::core::NoteKind::Click => 0,
                    prpr::core::NoteKind::Hold { .. } => 1,
                    prpr::core::NoteKind::Drag => 2,
                    prpr::core::NoteKind::Flick => 3,
                },
            })
        })
        .collect();
    let temporary = tempfile::tempdir().unwrap();
    let mut valid_left = replay::load(path).unwrap();
    valid_left.meta.id = "pro15-left-qa".into();
    for e in &mut valid_left.judges {
        let head_time = if e.kind == 5 || e.kind == 6 || e.note == 1 { 2. } else { e.t };
        e.note = notes.iter().find(|n| (n.time - head_time).abs() < 0.1).unwrap().index;
    }
    let valid_source = temporary.path().join("left.ppr");
    replay::save(&valid_left, &valid_source).unwrap();
    let (valid_path, _) = replay::library::import(&valid_source).unwrap();
    let path = valid_path.as_path();
    let request = Request {
        tool: 1,
        local_path: Some("renamed-chart".into()),
        info,
        key: format!("{:x}", Sha256::digest(bytes)),
        now: 1.05,
        duration: 12.,
        notes,
        events: Vec::new(),
        touches: Vec::new(),
    };
    let mut scene = AnalysisScene::new(request);
    let mut tm = TimeManager::default();
    scene.enter(&mut tm, None).unwrap();
    let (local_path, items) = local(scene.library_task.take().unwrap()).await.unwrap();
    scene.local = Some(local_path);
    scene.library = items;
    let index = scene.library.iter().position(|i| i.path == path).unwrap();
    scene.choosing = Some(0);
    scene.select_replay(index).unwrap();
    let (side, replay, name) = local(scene.load_task.take().unwrap()).await.unwrap();
    scene.panes[side] = Some(replay);
    scene.names[side] = name;
    let mut right = replay::load(path).unwrap();
    right.meta.id = "pro15-right-qa".into();
    right.meta.player = Some("Right comparison".into());
    right.judges[0].kind = 1;
    right.judges[0].diff = 0.10;
    let p = temporary.path().join("right.ppr");
    replay::save(&right, &p).unwrap();
    let (p, _) = replay::library::import(&p).unwrap();
    scene.library = replay::library::all().unwrap();
    let index = scene.library.iter().position(|i| i.path == p).unwrap();
    scene.choosing = Some(1);
    scene.select_replay(index).unwrap();
    let (side, replay, name) = local(scene.load_task.take().unwrap()).await.unwrap();
    scene.panes[side] = Some(replay);
    scene.names[side] = name;
    // New-bound validates fingerprints regardless of the list entry a caller supplies.
    let mut wrong = right.clone();
    wrong.meta.fingerprint = Some("different-chart".into());
    let wp = temporary.path().join("wrong.ppr");
    replay::save(&wrong, &wp).unwrap();
    assert!(local(ReplayScene::new_bound(wp, "renamed-chart".into(), true)).await.is_err());
    scene.current = 1.05;
    scene.update(&mut tm).unwrap();
    assert_ne!(scene.panes[0].as_ref().unwrap().analysis_state().1, scene.panes[1].as_ref().unwrap().analysis_state().1);
    // Real practice opens the overlay with its parent clock paused.
    tm.pause();
    let frozen = tm.now();
    for (w, h, label) in [(1280, 548, "phone"), (1280, 720, "wide"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        scene.render(&mut tm, &mut ui).unwrap();
        assert!(tm.paused() && tm.now() == frozen, "comparison rendering must preserve the paused parent clock");
        capture(&mut ui, &format!("pro15-compare-{label}"));
        let y = -ui.top + 0.035 + 0.105 + 0.0325;
        assert!(scene.buttons[6].inner.contains(vec2(-0.5, y)), "left selector must not be overwritten by transport");
        assert!(scene.buttons[7].inner.contains(vec2(0.5, y)));
        tap(&mut scene, &mut tm, vec2(-0.5, y));
        assert_eq!(scene.choosing, Some(0));
        scene.choosing = None;
    }
    tm.resume();
    let rect = scene.timeline;
    for (phase, fraction) in [(TouchPhase::Started, 0.2), (TouchPhase::Moved, 0.5), (TouchPhase::Ended, 0.7)] {
        scene
            .touch(
                &mut tm,
                &Touch {
                    id: 777,
                    phase,
                    position: vec2(rect.x + rect.w * fraction, rect.center().y),
                    time: 0.,
                },
            )
            .unwrap();
    }
    assert!((scene.current - 8.4).abs() < 0.001);
    assert!(scene.seek_drag.is_none());
    scene.seek(0.);
    scene.update(&mut tm).unwrap();
    assert_eq!(scene.panes[0].as_ref().unwrap().analysis_state().0, 0);
    assert_eq!(scene.panes[1].as_ref().unwrap().analysis_state().0, 0);
    scene.seek(8.1);
    scene.update(&mut tm).unwrap();
    assert!(scene.panes[0].as_ref().unwrap().analysis_state().0 > 0);
    scene.request.tool = 2;
    scene.source = 1;
    scene.note = 0;
    assert!(scene.selected_event().is_some());
    for (w, h, label) in [(1280, 548, "phone"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        scene.render(&mut tm, &mut ui).unwrap();
        capture(&mut ui, &format!("pro15-note-detail-{label}"));
    }
    scene.request.tool = 0;
    scene.source = 1;
    scene.range = (1.9, 2.1);
    let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
    scene.render(&mut tm, &mut ui).unwrap();
    capture(&mut ui, "pro15-density");
    let g = scene.graph;
    let touch = |phase, x| Touch {
        id: 701,
        phase,
        position: vec2(x, g.center().y),
        time: 0.,
    };
    scene.touch(&mut tm, &touch(TouchPhase::Started, g.x + g.w * 1.9 / 12.)).unwrap();
    scene.touch(&mut tm, &touch(TouchPhase::Moved, g.x + g.w * 2.1 / 12.)).unwrap();
    scene.touch(&mut tm, &touch(TouchPhase::Ended, g.x + g.w * 2.1 / 12.)).unwrap();
    let point = vec2(ui.screen_rect().feather(-0.035).x + ui.screen_rect().feather(-0.035).w * 0.73, g.bottom() + 0.035 + 0.0375);
    tap(&mut scene, &mut tm, point);
    let NextScene::PopWithResult(result) = scene.next_scene(&mut tm) else {
        panic!("density apply button must return interval")
    };
    let selection = result.downcast::<Selection>().unwrap();
    assert!(selection.start < 2. && selection.end >= 5., "Hold tail must be included");
    println!("pro.15 practice analysis: same-chart selectors, two real replay clocks, backward seeks, note offsets, density drag and Hold-safe interval passed");
    prpr::practice::tools::register(super::open);
    let mut config = crate::get_data().config.clone();
    config.practice_local_path = Some("renamed-chart".into());
    let mut fs = fs_from_path("renamed-chart").unwrap();
    let info = local(prpr::fs::load_info(fs.as_mut())).await.unwrap();
    let mut game = local(prpr::scene::GameScene::new(
        prpr::scene::GameMode::Exercise,
        info,
        config,
        fs,
        None,
        prpr::ext::BLACK_TEXTURE.clone(),
        prpr::ext::BLACK_TEXTURE.clone(),
        None,
        None,
        None,
    ))
    .await
    .unwrap();
    game.enter(&mut tm, None).unwrap();
    tm.seek_to(3.5);
    tm.pause();
    game.practice_open_analysis_for_qa(&mut tm, 0).unwrap();
    assert!(matches!(game.next_scene(&mut tm), NextScene::Overlay(_)));
    tm.reset();
    tm.seek_to(100.);
    game.enter(&mut tm, None).unwrap();
    assert!((tm.now() - 3.5).abs() < 0.01 && tm.paused(), "return from analysis must preserve practice seek and pause");
    println!("pro.15 analysis return: actual GameScene restores paused practice position without reset");
}
