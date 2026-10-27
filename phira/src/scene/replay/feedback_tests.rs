//! Real GPU regression for noise compositing over all gameplay HUD elements.
use super::*;
use crate::{data::Data, DATA_PATH};
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    info::{ChartFormat, ChartInfo},
    ui::{FontArc, TextPainter},
};
use serde_json::json;

async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = prpr::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}

fn wave() -> Vec<u8> {
    let n = 8000u32 * 12 * 2;
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

fn capture(game: &mut GameScene, tm: &mut TimeManager, painter: &mut TextPainter, w: i32, h: i32) -> Image {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    game.render(tm, &mut ui).unwrap();
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    Image {
        bytes,
        width: w as u16,
        height: h as u16,
    }
}

#[test]
#[ignore = "requires desktop OpenGL and audio; run separately with --ignored --test-threads=1"]
fn noise_covers_gameplay_hud_across_formats_and_render_targets() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Gameplay HUD noise regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            sample_count: 4,
            ..Default::default()
        },
        async {
            next_frame().await;
            let output = std::path::Path::new("target/feedback-noise-qa");
            std::fs::create_dir_all(output).unwrap();
            let root = tempfile::tempdir_in(output).unwrap();
            let old_root = DATA_PATH.lock().unwrap().replace(root.path().to_string_lossy().into_owned());
            let mut data = Data::default();
            data.language = Some("zh-CN".into());
            crate::set_data(data);
            macroquad::file::set_pc_assets_folder("assets");
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|v| *v.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            BOLD_FONT.with(|v| {
                *v.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font)))
            });
            for format in ["pgr", "rpe", "rpen"] {
                let path = format!("custom/hud-{format}");
                let directory = root.path().join("data/charts").join(&path);
                std::fs::create_dir_all(&directory).unwrap();
                let native_block = json!({"topRightPercentage":{"x":1,"y":1},"bottomLeftPercentage":{"x":0,"y":0},
                "appearTime":-2,"enableTime":-1,"disableTime":20,"disappearTime":21});
                let chart = if format == "pgr" {
                    let event = |v: f32| json!([{"startTime":0,"endTime":1000000,"start":v,"end":v,"start2":0.5,"end2":0.5}]);
                    let notes: Vec<_> = (0..4)
                        .map(|i| json!({"type":1,"time":640+i*64,"positionX":0,"holdTime":0,"speed":1,"floorPosition":10+i}))
                        .collect();
                    json!({"formatVersion":3,"offset":0,"numOfNotes":4,"BlockAreaList":[native_block],"judgeLineList":[{
                    "bpm":120,"notesAbove":notes,"notesBelow":[],"speedEvents":[{"startTime":0,"endTime":1000000,"value":1}],
                    "judgeLineMoveEvents":event(0.5),"judgeLineRotateEvents":event(0.),"judgeLineDisappearEvents":event(1.)}]})
                } else {
                    let mut chart: serde_json::Value =
                        serde_json::from_str(include_str!("../../../../prpr/tests/fixtures/rpen-extensions/chart.json")).unwrap();
                    chart.as_object_mut().unwrap().remove("globalEvents");
                    chart["judgeLineList"][0]["eventLayers"][0]["moveXEvents"] = json!([]);
                    chart["judgeLineList"][0]["skewControl"] = json!([]);
                    chart["blockAreaList"] = if format == "rpen" {
                        json!([{
                    "bottomLeft":{"x":-675,"y":-450},"topRight":{"x":675,"y":450},
                    "appearTime":[0,0,1],"disappearTime":[40,0,1],"activeIntervals":[{"startTime":[0,0,1],"endTime":[40,0,1]}],
                    "layerId":"default","isInvert":false}])
                    } else {
                        json!([native_block])
                    };
                    chart["noiseAreaLayers"] = json!([{"id":"default","name":"default","skin":"","parameters":{}}]);
                    chart
                };
                std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
                std::fs::write(directory.join("song.wav"), wave()).unwrap();
                Image::gen_image_color(32, 32, Color::new(0.1, 0.2, 0.3, 1.)).export_png(directory.join("background.png").to_str().unwrap());
                let info = ChartInfo {
                    name: "Noise HUD regression".into(),
                    level: "IN 15".into(),
                    music: "song.wav".into(),
                    format: Some(if format == "pgr" { ChartFormat::Pgr } else { ChartFormat::Rpe }),
                    use_rpen: Some(format == "rpen"),
                    ..Default::default()
                };
                for (pipeline, samples, fxaa) in [("direct", 1, false), ("fxaa", 1, true), ("msaa", 4, false)] {
                    let mut config = Config::default();
                    config.volume_music = 0.;
                    config.volume_sfx = 0.;
                    config.sample_count = samples;
                    config.fxaa = fxaa;
                    config.interactive = false;
                    config.adjust_time = false;
                    config.show_acc = true;
                    config.show_judgement_details = true;
                    config.offset_indicator = true;
                    let mut game = local(GameScene::new(
                        GameMode::Normal,
                        info.clone(),
                        config,
                        fs_from_path(&path).unwrap(),
                        None,
                        BLACK_TEXTURE.clone(),
                        BLACK_TEXTURE.clone(),
                        None,
                        None,
                        None,
                    ))
                    .await
                    .unwrap();
                    let mut tm = TimeManager::manual(Box::new(|| 10.));
                    game.enter(&mut tm, None).unwrap();
                    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
                    game.update(&mut tm).unwrap();
                    tm.seek_to(0.);
                    game.update(&mut tm).unwrap();
                    tm.seek_to(3.);
                    game.update(&mut tm).unwrap();
                    for note in 0..3 {
                        game.judge.commit(3., prpr::judge::Judgement::Perfect, 0, note, 0.01);
                    }
                    for (w, h) in [(1280, 720), (960, 720), (1280, 548)] {
                        let blocks = std::mem::take(&mut game.chart.block_areas);
                        let next = std::mem::take(&mut game.chart.next_block_areas);
                        game.chart.reset();
                        game.chart.update(&mut game.res);
                        let clean = capture(&mut game, &mut tm, &mut painter, w, h);
                        game.chart.block_areas = blocks;
                        game.chart.next_block_areas = next;
                        game.chart.reset();
                        game.chart.update(&mut game.res);
                        let noisy = capture(&mut game, &mut tm, &mut painter, w, h);
                        let tag = format!("{format}-{pipeline}-{w}x{h}");
                        clean.export_png(output.join(format!("{tag}-clean.png")).to_str().unwrap());
                        noisy.export_png(output.join(format!("{tag}-noise.png")).to_str().unwrap());
                        for (element, x0, y0, x1, y1) in [
                            ("pause", 0., 0.01, 0.15, 0.18),
                            ("score/accuracy/details", 0.65, 0.01, 1., 0.45),
                            ("combo/indicator", 0.3, 0.01, 0.7, 0.38),
                            ("title", 0., 0.82, 0.6, 1.),
                            ("difficulty", 0.7, 0.82, 1., 1.),
                            ("progress", 0., 0., 0.5, 0.04),
                        ] {
                            let mut bright = 0;
                            let mut changed = 0;
                            for y in (y0 * h as f32) as i32..(y1 * h as f32) as i32 {
                                for x in (x0 * w as f32) as i32..(x1 * w as f32) as i32 {
                                    let i = ((h - 1 - y) * w + x) as usize * 4;
                                    let a = &clean.bytes[i..i + 3];
                                    let b = &noisy.bytes[i..i + 3];
                                    if a.iter().all(|v| *v > 140) {
                                        bright += 1;
                                        if a.iter().zip(b).any(|(a, b)| a.abs_diff(*b) > 35) {
                                            changed += 1;
                                        }
                                    }
                                }
                            }
                            assert!(bright > 5, "missing {element}: {tag}, bright={bright}");
                            assert!(changed * 4 > bright, "noise must affect {element}: {tag}, changed={changed}/{bright}");
                        }
                    }
                }
            }
            *DATA_PATH.lock().unwrap() = old_root;
            std::process::exit(0);
        },
    );
}
