//! Stationary RPE notes must survive the real renderer and manual input path.
use super::*;
use crate::{
    core::ChartExtra,
    dir::Dir,
    fs::ExternalFileSystem,
    info::ChartInfo,
    ui::{FontArc, TextPainter, Ui},
};
use serde_json::json;
use std::sync::Arc;

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
fn stationary_notes_render_and_accept_manual_hits() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Stationary note regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            std::fs::create_dir_all("target/stationary-qa").unwrap();
            let directory = tempfile::tempdir_in("target/stationary-qa").unwrap();
            std::fs::write(directory.path().join("music.ogg"), include_bytes!("../../../assets/ending.ogg")).unwrap();
            let mut painter = TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap(), None);
            for algorithm in [JudgeAlgorithm::PhiraPro, JudgeAlgorithm::Phigros] {
                for (w, h) in [(960, 720), (1280, 720), (1170, 540)] {
                    let mut config = Config::default();
                    config.judge_algorithm = algorithm;
                    config.volume_music = 0.;
                    config.volume_sfx = 0.;
                    config.particle = false;
                    config.sample_count = 1;
                    config.aggressive = true;
                    let info = ChartInfo {
                        music: "music.ogg".into(),
                        aspect_ratio: w as f32 / h as f32,
                        ..Default::default()
                    };
                    let fs = ExternalFileSystem(Arc::new(Dir::new(directory.path()).unwrap()));
                    let image: crate::ext::SafeTexture = Texture2D::from_rgba8(1, 1, &[20, 20, 20, 255]).into();
                    let mut res = finish(Resource::new(config, info, Box::new(fs), None, image.clone(), image, true))
                        .await
                        .unwrap();
                    res.update_size((0, 0, w, h));
                    res.alpha = 1.;
                    for speed in [0., -0.] {
                        for above in [1, 2] {
                            for kind in [1, 2, 3, 4] {
                                let mut source: serde_json::Value =
                                    serde_json::from_str(include_str!("../../tests/fixtures/rpen-extensions/chart.json")).unwrap();
                                let mut line = source["judgeLineList"][0].clone();
                                line["bpmfactor"] = 1.into();
                                line["isCover"] = 0.into();
                                line["eventLayers"][0]["moveXEvents"][0].as_object_mut().unwrap().remove("trajectory");
                                line["eventLayers"][0]["rotateEvents"] = json!([{"startTime":[0,0,1],"endTime":[16,0,1],"start":23,"end":23}]);
                                for (name, field) in [
                                    ("alphaControl", "alpha"),
                                    ("sizeControl", "size"),
                                    ("posControl", "pos"),
                                    ("yControl", "y"),
                                ] {
                                    line[name] = json!([{"x":-1000000,field:1,"easing":1},{"x":1000000,field:1,"easing":1}]);
                                }
                                line.as_object_mut().unwrap().remove("skewControl");
                                line["notes"] = json!([{"type":kind,"above":above,"startTime":[4,0,1],"endTime":if kind==2 {[6,0,1]} else {[4,0,1]},"positionX":150,"yOffset":0,"alpha":255,"size":1,"speed":speed,"isFake":0,"visibleTime":9999}]);
                                source["judgeLineList"] = json!([line]);
                                for field in ["blockAreaList", "noiseAreaLayers", "globalEvents"] {
                                    source.as_object_mut().unwrap().remove(field);
                                }
                                let mut fs = ExternalFileSystem(Arc::new(Dir::new(directory.path()).unwrap()));
                                let mut chart = finish(crate::parse::parse_rpe(&source.to_string(), &mut fs, ChartExtra::default(), false))
                                    .await
                                    .unwrap();
                                assert_eq!(chart.lines[0].notes[0].speed, 0.);
                                res.time = 1.9;
                                chart.update(&mut res);
                                let mut draw = |chart: &Chart| {
                                    set_camera(&res.camera);
                                    clear_background(BLACK);
                                    chart.render(&mut Ui::new(&mut painter, Some((0, 0, w, h))), &mut res);
                                    unsafe { get_internal_gl() }.flush();
                                    let mut bytes = vec![0u8; (w * h * 4) as usize];
                                    unsafe {
                                        miniquad::gl::glReadPixels(
                                            0,
                                            0,
                                            w,
                                            h,
                                            miniquad::gl::GL_RGBA,
                                            miniquad::gl::GL_UNSIGNED_BYTE,
                                            bytes.as_mut_ptr() as _,
                                        );
                                    }
                                    bytes
                                };
                                let visible = draw(&chart);
                                chart.lines[0].notes[0].judge = JudgeStatus::Judged;
                                // Holds still draw after a miss, so hide the actual note alpha.
                                chart.lines[0].notes[0].object.alpha = crate::core::AnimFloat::fixed(0.);
                                let hidden = draw(&chart);
                                let changed = visible.chunks_exact(4).zip(hidden.chunks_exact(4)).filter(|(a, b)| a != b).count();
                                assert!(changed > 30, "missing stationary note pixels: {algorithm:?} {w}x{h} {above}/{kind}/{speed}");
                                chart.lines[0].notes[0].object.alpha = crate::core::AnimFloat::fixed(1.);
                                chart.reset();
                                let mut judge = Judge::new(&chart, false);
                                let mut bad = Vec::new();
                                res.time = 2.;
                                chart.update(&mut res);
                                let point = chart.lines[0]
                                    .now_transform(&res, &chart.lines)
                                    .transform_point(&Point::new(150. / 675., 0.));
                                let position = vec2((point.x + 1.) * w as f32 / 2., screen_height() - h as f32 / 2. - point.y * w as f32 / 2.);
                                let input = |phase, p| {
                                    TOUCHES.with(|s| {
                                        s.borrow_mut().touches = vec![Touch {
                                            id: 91,
                                            phase,
                                            position: p,
                                            time: f64::NEG_INFINITY,
                                        }]
                                    })
                                };
                                if kind == 3 {
                                    res.time = 1.96;
                                    judge.update(&mut res, &mut chart, &mut bad);
                                    res.time = 1.97;
                                }
                                input(TouchPhase::Started, position);
                                judge.update(&mut res, &mut chart, &mut bad);
                                if kind == 3 {
                                    res.time = 1.985;
                                    input(TouchPhase::Moved, position + vec2(25., 0.));
                                    judge.update(&mut res, &mut chart, &mut bad);
                                    res.time = 2.;
                                    input(TouchPhase::Moved, position + vec2(50., 0.));
                                    judge.update(&mut res, &mut chart, &mut bad);
                                }
                                if kind == 2 {
                                    assert!(matches!(chart.lines[0].notes[0].judge, JudgeStatus::Hold(..)), "manual Hold head {algorithm:?}");
                                    for frame in 1..=120 {
                                        res.time = 2. + frame as f64 / 120.;
                                        input(TouchPhase::Stationary, position);
                                        judge.update(&mut res, &mut chart, &mut bad);
                                    }
                                }
                                assert_eq!(judge.combo(), 1, "manual stationary {algorithm:?} {w}x{h} {above}/{kind}/{speed}");
                                assert!(judge.score(false) > 0);
                                input(TouchPhase::Ended, position);
                                res.time += 0.01;
                                judge.update(&mut res, &mut chart, &mut bad);
                                TOUCHES.with(|s| s.borrow_mut().touches.clear());
                            }
                        }
                    }
                }
            }
            println!(
                "Stationary notes: 2 engines × 3 aspects × ±0 speeds × above/below × Tap/Hold/Flick/Drag; rendered pixels and manual hits passed"
            );
        },
    );
}
