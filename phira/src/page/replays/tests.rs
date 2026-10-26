//! Isolated GPU and interaction checks for the management page.
use super::*;
use prpr::{
    core::BOLD_FONT,
    ui::{FontArc, TextPainter},
};

fn snapshot(manager: &mut ReplayManager, painter: &mut TextPainter, w: i32, h: i32, name: &str) {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    clear_background(Color::new(0.10, 0.13, 0.18, 1.));
    manager.render_inner(&mut ui, 2.);
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
    .export_png(&format!("target/replay-qa/replay-cards-{name}.png"));
}

fn center(button: &RectButton) -> Vec2 {
    let mut points = Vec::new();
    for x in -100..=100 {
        for y in -55..=55 {
            let point = vec2(x as f32 / 100., y as f32 / 100.);
            if button.contains(point) {
                points.push(point);
            }
        }
    }
    assert!(!points.is_empty(), "test must click a visible production control");
    points.iter().copied().sum::<Vec2>() / points.len() as f32
}

fn click(manager: &mut ReplayManager, point: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        manager
            .touch_inner(
                &Touch {
                    id: 121,
                    phase,
                    position: point,
                    time: f64::INFINITY,
                },
                2.,
            )
            .unwrap();
    }
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn replay_manager_cards_and_controls() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Replay manager UI regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            next_frame().await;
            std::fs::create_dir_all("target/replay-qa").unwrap();
            let isolated = tempfile::tempdir_in("target/replay-qa").unwrap();
            let old_root = crate::DATA_PATH.lock().unwrap().replace(isolated.path().to_string_lossy().into_owned());
            let mut data = crate::data::Data::default();
            data.charts.push(crate::data::LocalChart {
                info: prpr::info::ChartInfo {
                    name: "本地谱面测试关联列表".into(),
                    ..Default::default()
                }
                .into(),
                local_path: "custom/ui-test".into(),
                record: None,
                mods: Default::default(),
                played_unlock: false,
            });
            crate::set_data(data);
            macroquad::file::set_pc_assets_folder("assets");
            prpr::ui::set_multilingual_fallback(FontArc::try_from_vec(std::fs::read("assets/font.ttf").unwrap()).unwrap());
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            BOLD_FONT.with(|v| {
                *v.borrow_mut() =
                    Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font.clone())))
            });
            let mut painter = TextPainter::new(font.clone(), None);
            let mut empty = ReplayManager::new(None, false);
            snapshot(&mut empty, &mut painter, 1280, 720, "empty");
            let mut paths = Vec::new();
            for (i, name) in ["长曲名回放：光辉岁月与星空之下的记录", "自己录制的回放", "未记录成绩的导入回放"]
                .iter()
                .enumerate()
            {
                let tape = replay::Replay {
                    meta: replay::Metadata {
                        id: format!("replay-ui-{i}"),
                        name: (*name).into(),
                        level: "IN 15".into(),
                        player: Some("回放玩家".into()),
                        recorded_at: Some(1792857600000 - i as i64 * 60000),
                        result: (i != 2).then(|| crate::history::Record {
                            score: 987654,
                            accuracy: 0.9832,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    chart: Some(replay::ChartRef::Local("custom/ui-test".into())),
                    speed: 1.,
                    has_speed: true,
                    settings: Some(Default::default()),
                    judges: vec![replay::JudgeEvent { t: 1., ..Default::default() }],
                    ..Default::default()
                };
                let path = if i == 1 {
                    let root = PathBuf::from(crate::dir::root().unwrap()).join("replays");
                    std::fs::create_dir_all(&root).unwrap();
                    let path = root.join("local-test.ppr");
                    replay::save(&tape, &path).unwrap();
                    path
                } else {
                    let source = isolated.path().join(format!("source-{i}.phirar"));
                    replay::save(&tape, &source).unwrap();
                    library::import(&source).unwrap().0
                };
                paths.push(path);
            }
            let before = paths.iter().map(|path| std::fs::read(path).unwrap()).collect::<Vec<_>>();
            let mut state = SharedState::new(font).await.unwrap();
            render_regression(&mut painter, &paths[0], &mut state);
            let mut manager = ReplayManager::new(None, false);
            snapshot(&mut manager, &mut painter, 1280, 720, "before-selection");
            let point = center(&manager.rows[0]);
            click(&mut manager, point);
            assert_eq!(manager.selected.as_ref(), Some(&paths[0]));
            snapshot(&mut manager, &mut painter, 1280, 720, "expanded-toggle");
            let point = center(&manager.rows[0]);
            click(&mut manager, point);
            assert!(manager.selected.is_none(), "再次点击同一回放必须收起详情");
            snapshot(&mut manager, &mut painter, 1280, 720, "collapsed-toggle");
            let point = center(&manager.rows[0]);
            click(&mut manager, point);
            assert!(manager.scroll.y_scroller.goto.is_some(), "selection reveals the expanded actions");
            for (w, h, label) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
                manager.selected = None;
                manager.scroll.y_scroller.reset();
                snapshot(&mut manager, &mut painter, w, h, &format!("list-{label}"));
                manager.selected = Some(paths[0].clone());
                snapshot(&mut manager, &mut painter, w, h, &format!("selected-{label}"));
                manager.scroll.y_scroller.offset = 0.45;
                snapshot(&mut manager, &mut painter, w, h, &format!("actions-{label}"));
            }
            snapshot(&mut manager, &mut painter, 1280, 720, "actions");
            let point = center(&manager.buttons[9].inner);
            for (phase, position) in [
                (TouchPhase::Started, point),
                (TouchPhase::Moved, point - vec2(0., 0.08)),
                (TouchPhase::Ended, point),
            ] {
                manager
                    .touch_inner(
                        &Touch {
                            id: 122,
                            phase,
                            position,
                            time: f64::INFINITY,
                        },
                        2.,
                    )
                    .unwrap();
            }
            assert!(prpr::scene::DIALOG.with(|dialog| dialog.borrow().is_none()), "scrolling from an action must not activate it");
            manager.scroll.y_scroller.offset = 0.;
            snapshot(&mut manager, &mut painter, 1280, 720, "after-drag");
            let point = center(&manager.buttons[19].inner);
            click(&mut manager, point);
            assert!(manager.selected_item().unwrap().binding.unwrap().keep);
            snapshot(&mut manager, &mut painter, 1280, 720, "kept");
            let point = center(&manager.buttons[10].inner);
            click(&mut manager, point);
            assert!(manager.choosing_chart);
            snapshot(&mut manager, &mut painter, 1280, 720, "choose-chart");
            let point = center(&manager.buttons[10].inner);
            click(&mut manager, point);
            assert!(!manager.choosing_chart);
            snapshot(&mut manager, &mut painter, 1280, 720, "before-multi");
            let point = center(&manager.buttons[15].inner);
            click(&mut manager, point);
            assert!(manager.multi);
            snapshot(&mut manager, &mut painter, 1280, 720, "multi");
            let point = center(&manager.buttons[16].inner);
            click(&mut manager, point);
            assert_eq!(manager.chosen.len(), 3);
            snapshot(&mut manager, &mut painter, 1280, 720, "multi-selected");
            assert!(!manager.buttons[7].inner.contains(vec2(0., 0.)), "hidden single-record actions must be invalidated");
            let point = center(&manager.buttons[17].inner);
            click(&mut manager, point);
            assert!(prpr::scene::DIALOG.with(|dialog| dialog.borrow_mut().take().is_some()), "batch deletion retains confirmation");
            manager.multi = false;
            manager.selected = None;
            for (filter, expected) in [(1, 1), (2, 2), (3, 2)] {
                manager.filter = filter;
                assert_eq!(manager.visible().len(), expected);
            }
            manager.filter = 0;
            prpr::scene::return_input("replay_search".into(), "未记录成绩".into());
            manager.update_inner(2.);
            assert_eq!(manager.visible().len(), 1);
            snapshot(&mut manager, &mut painter, 1280, 720, "search");
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(std::fs::read(path).unwrap(), bytes, "UI must preserve replay files");
            }
            prpr::ui::cleanup_audio();
            *crate::DATA_PATH.lock().unwrap() = old_root;
        },
    );
}
