//! Isolated real OpenGL, installation, event routing and background-job regression.
use super::*;
use prpr::ui::{FontArc, TextPainter};

fn snapshot(scene: &mut ReplayAnalysisScene, painter: &mut TextPainter, tm: &mut TimeManager, w: i32, h: i32, name: &str) {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    Scene::render(scene, tm, &mut ui).unwrap();
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; w as usize * h as usize * 4];
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
    .export_png(&format!("target/prca-qa/ui-{name}.png"));
}
fn center(button: &RectButton) -> Vec2 {
    let mut points = Vec::new();
    for x in -100..=100 {
        for y in -100..=100 {
            let p = vec2(x as f32 / 100., y as f32 / 100.);
            if button.contains(p) {
                points.push(p);
            }
        }
    }
    assert!(!points.is_empty(), "expected a visible production hitbox");
    points.iter().copied().sum::<Vec2>() / points.len() as f32
}
fn click(scene: &mut ReplayAnalysisScene, tm: &mut TimeManager, p: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Scene::touch(
            scene,
            tm,
            &Touch {
                id: 777,
                phase,
                position: p,
                time: f64::INFINITY,
            },
        )
        .unwrap();
    }
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn installed_prca_ui_background_report_and_cancel() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "PRCA UI regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            next_frame().await;
            std::fs::create_dir_all("target/prca-qa").unwrap();
            let isolated = tempfile::tempdir_in("target/prca-qa").unwrap();
            let old_root = crate::DATA_PATH.lock().unwrap().replace(isolated.path().to_string_lossy().into_owned());
            crate::set_data(Default::default());
            macroquad::file::set_pc_assets_folder("assets");
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            prpr::core::BOLD_FONT.with(|v| *v.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            let mut painter = TextPainter::new(font, None);
            let mut tm = TimeManager::default();
            let zip = isolated.path().join("PRCA.zip");
            {
                use std::io::Write;
                let mut archive = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
                for (name, data) in [
                    ("manifest.json", include_str!("../../../..//docs/plugins/examples/prca/manifest.json")),
                    ("main.rhai", include_str!("../../../..//docs/plugins/examples/prca/main.rhai")),
                ] {
                    archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                    archive.write_all(data.as_bytes()).unwrap();
                }
                archive.finish().unwrap();
            }
            crate::plugins::import(&zip).unwrap();
            assert!(replay_analysis::available().is_empty());
            crate::plugins::change("phira-pro.prca", Some(true), None, false).unwrap();
            assert_eq!(replay_analysis::available().len(), 1);
            let source = isolated.path().join("test.ppr");
            let tape = replay::Replay {
                speed: 1.,
                has_speed: true,
                meta: replay::Metadata {
                    name: "PRCA 回放导入与分析测试".into(),
                    player: Some("本地测试玩家".into()),
                    ..Default::default()
                },
                frames: vec![0., 1., 2.],
                judges: vec![replay::JudgeEvent { t: 1., ..Default::default() }],
                ..Default::default()
            };
            replay::save(&tape, &source).unwrap();
            let (path, _) = library::import(&source).unwrap();
            let mut scene = ReplayAnalysisScene::new(None).unwrap();
            snapshot(&mut scene, &mut painter, &mut tm, 1280, 720, "local-picker");
            for index in [3, 9] {
                assert!(
                    (-100..=100).all(|x| (-100..=100).all(|y| !scene.buttons[index].inner.contains(vec2(x as f32 / 100., y as f32 / 100.)))),
                    "removed button has no hitbox"
                );
            }
            let p = center(&scene.rows[0]);
            click(&mut scene, &mut tm, p);
            assert_eq!(scene.selected, Some(path));
            assert!(scene.task.is_none(), "选中回放不应立即分析");
            snapshot(&mut scene, &mut painter, &mut tm, 1280, 720, "selected");
            let p = center(&scene.buttons[4].inner);
            click(&mut scene, &mut tm, p);
            assert!(scene.task.is_some());
            let start = std::time::Instant::now();
            while scene.task.is_some() && start.elapsed().as_secs() < 20 {
                Scene::update(&mut scene, &mut tm).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
                next_frame().await;
            }
            assert!(scene.output.is_some(), "{}", scene.status);
            assert!(scene.output.as_ref().unwrap().png.is_file());
            assert!(scene.output.as_ref().unwrap().json.is_file());
            for (w, h, name) in [(960, 720, "report-4x3"), (1280, 720, "report-16x9"), (1280, 548, "report-21x9")] {
                snapshot(&mut scene, &mut painter, &mut tm, w, h, name);
            }
            snapshot(&mut scene, &mut painter, &mut tm, 1280, 720, "before-zoom");
            assert!(scene.preview.is_some(), "完成后自动显示图片窗口");
            let p = center(&scene.preview.as_ref().unwrap().buttons[2].inner);
            click(&mut scene, &mut tm, p);
            assert!(scene.preview.as_ref().unwrap().zoom > 1.);
            snapshot(&mut scene, &mut painter, &mut tm, 1280, 720, "zoomed");
            let p = center(&scene.preview.as_ref().unwrap().buttons[0].inner);
            click(&mut scene, &mut tm, p);
            assert!(scene.preview.is_none());
            scene.start().unwrap();
            snapshot(&mut scene, &mut painter, &mut tm, 1280, 720, "running");
            let p = center(&scene.buttons[4].inner);
            click(&mut scene, &mut tm, p);
            assert!(scene.task.is_none());
            assert!(scene.control.check().is_err());
            crate::plugins::change("phira-pro.prca", Some(false), None, false).unwrap();
            assert!(replay_analysis::available().is_empty());
            prpr::ui::cleanup_audio();
            *crate::DATA_PATH.lock().unwrap() = old_root;
        },
    );
}
