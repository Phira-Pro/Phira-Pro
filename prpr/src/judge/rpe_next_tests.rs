//! Production input filtering, not only a geometry helper test.
use super::*;
use crate::{core::ChartExtra, dir::Dir, fs::ExternalFileSystem, info::ChartInfo};
use std::{future::Future, sync::Arc};

async fn finish<T>(future: impl Future<Output = T>) -> T {
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
fn next_export_filters_real_touches_in_both_judgement_engines() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "rpe-next input regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            let directory = tempfile::tempdir_in("target").unwrap();
            std::fs::write(directory.path().join("music.ogg"), include_bytes!("../../../assets/ending.ogg")).unwrap();
            for algorithm in [JudgeAlgorithm::PhiraPro, JudgeAlgorithm::Phigros] {
                for (w, h) in [(960, 720), (1280, 720), (1170, 540)] {
                    for state in 0..3 {
                        let mut source: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/rpe-next/chart.json")).unwrap();
                        let line = &mut source["judgeLineList"][0];
                        line["bpmfactor"] = 1.into();
                        let note = line["notes"][0].clone();
                        line["notes"] = serde_json::json!([note]);
                        source["blockAreaList"] = serde_json::json!([{
                            "bottomLeft":{"x":-250,"y":-150},"topRight":{"x":250,"y":150},
                            "appearTime":[0,0,1],"disappearTime":[16,0,1],
                            "activeIntervals":[{"startTime":if state==2 {[2,0,1]} else if state==1 {[4,1,2]} else {[8,0,1]},
                                "endTime":[10,0,1],"readyFlash":0.5}]
                        }]);
                        let mut fs = ExternalFileSystem(Arc::new(Dir::new(directory.path()).unwrap()));
                        let mut chart = finish(crate::parse::parse_rpe(&source.to_string(), &mut fs, ChartExtra::default(), false))
                            .await
                            .unwrap();
                        let mut config = Config::default();
                        config.judge_algorithm = algorithm;
                        config.volume_music = 0.;
                        config.volume_sfx = 0.;
                        config.particle = false;
                        config.sample_count = 1;
                        let info = ChartInfo {
                            music: "music.ogg".into(),
                            aspect_ratio: w as f32 / h as f32,
                            ..Default::default()
                        };
                        let image: crate::ext::SafeTexture = Texture2D::from_rgba8(1, 1, &[20, 20, 20, 255]).into();
                        let mut res = finish(Resource::new(config, info, Box::new(fs), None, image.clone(), image, true))
                            .await
                            .unwrap();
                        res.update_size((0, 0, w, h));
                        unsafe { get_internal_gl() }.quad_gl.viewport(Some((0, 0, w, h)));
                        let mut judge = Judge::new(&chart, false);
                        let mut bad = Vec::new();
                        res.time = 2.;
                        // Judge's raw input uses physical screen Y, including the
                        // viewport's bottom origin conversion.
                        TOUCHES.with(|s| {
                            s.borrow_mut().touches = vec![Touch {
                                id: 71,
                                phase: TouchPhase::Started,
                                position: vec2(w as f32 / 2., screen_height() - h as f32 / 2.),
                                time: f64::NEG_INFINITY,
                            }]
                        });
                        assert_eq!(chart.next_block_samples(2., res.aspect_ratio)[0].state, state);
                        judge.update(&mut res, &mut chart, &mut bad);
                        assert_eq!(!chart.blocked_touches.is_empty(), state == 2, "algorithm={algorithm:?} state={state} {w}x{h}");
                        assert_eq!(judge.combo(), u32::from(state != 2), "algorithm={algorithm:?} state={state} {w}x{h}");
                        TOUCHES.with(|s| {
                            s.borrow_mut().touches = vec![Touch {
                                id: 71,
                                phase: TouchPhase::Ended,
                                position: vec2(w as f32 / 2., screen_height() - h as f32 / 2.),
                                time: f64::NEG_INFINITY,
                            }]
                        });
                        res.time = 2.01;
                        judge.update(&mut res, &mut chart, &mut bad);
                        TOUCHES.with(|s| s.borrow_mut().touches.clear());
                    }
                }
            }
            println!("rpe-next input: 2 engines × 3 aspects × disabled/ready/active passed");
        },
    );
}
