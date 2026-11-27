use super::*;
use serde_json::json;

#[test]
fn format_detection_accepts_editor_whitespace_and_bom() {
    let info = ChartInfo::default();
    for prefix in ["", "\r\n  ", "\u{feff}", "\u{feff}\n\t"] {
        for (source, expected) in [
            (r#"{"META":{},"BPMList":[],"judgeLineList":[]}"#, ChartFormat::Rpe),
            (r#"{"BPMList":[],"judgeLineList":[]}"#, ChartFormat::Rpe),
            (r#"{"formatVersion":3,"judgeLineList":[]}"#, ChartFormat::Pgr),
        ] {
            assert_eq!(GameScene::infer_chart_format(&info, format!("{prefix}{source}").as_bytes()), expected);
        }
    }
    assert_eq!(GameScene::infer_chart_format(&info, b"0\n bp 0 120"), ChartFormat::Pec);
    assert_eq!(GameScene::infer_chart_format(&info, &[0xff, 0, 0x80]), ChartFormat::Pbc);
    let explicit = ChartInfo { format: Some(ChartFormat::Pec), ..info };
    assert_eq!(GameScene::infer_chart_format(&explicit, b"{}"), ChartFormat::Pec);
}

async fn finish<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = crate::ext::poll_future(future.as_mut()) { return value; }
        next_frame().await;
    }
}

#[test]
#[ignore = "requires a local PHIRA_TEST_CHART and desktop OpenGL/audio; run separately"]
fn local_chart_entrance_alpha() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    let path = std::env::var_os("PHIRA_TEST_CHART").expect("set PHIRA_TEST_CHART to a local chart archive");
    macroquad::Window::from_config(
        Conf {
            window_title: "Chart entrance regression".into(),
            headless: true,
            window_width: 960,
            window_height: 540,
            ..Default::default()
        },
        async move {
            set_pc_assets_folder("assets");
            next_frame().await;
            let mut fs = crate::fs::fs_from_file(std::path::Path::new(&path)).unwrap();
            let info = finish(crate::fs::load_info(fs.as_mut())).await.unwrap();
            let bytes = finish(GameScene::load_chart_bytes(fs.as_mut(), &info)).await.unwrap();
            let raw: serde_json::Value = serde_json::from_slice(crate::parse::json_bytes(&bytes)).unwrap();
            // Compare against RPEN's stateless event evaluator, independent of
            // Anim's mutable cursor, using only the locally supplied source.
            let bpms = raw["BPMList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|bpm| (crate::parse::rpen::beat(&bpm["startTime"]).unwrap(), bpm["bpm"].as_f64().unwrap()))
                .collect();
            let mut bpm = BpmList::new(bpms);
            let reference: Vec<Vec<_>> = raw["judgeLineList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| {
                    let factor = if info.use_rpen.unwrap_or(true) {
                        crate::parse::rpen::number(&line["bpmfactor"], 1.)
                    } else {
                        1.
                    };
                    line["eventLayers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|layer| layer["alphaEvents"].as_array())
                        .map(|events| crate::parse::rpen::Track::new(events, &mut bpm, factor, json!(0), false).unwrap())
                        .collect()
                })
                .collect();
            let mut config = Config::default();
            config.volume_music = 0.;
            config.volume_sfx = 0.;
            config.particle = false;
            config.interactive = false;
            config.adjust_time = false;
            config.sample_count = std::env::var("PHIRA_TEST_SAMPLES").ok().and_then(|value| value.parse().ok()).unwrap_or(1);
            let isolate = std::env::var_os("PHIRA_TEST_ISOLATE_LINES").is_some();
            let texture: SafeTexture = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into();
            let mut scene = finish(GameScene::new(GameMode::Normal, info, config, fs, None, texture.clone(), texture, None, None, None))
                .await
                .unwrap();
            scene.hide_ui = true;
            if isolate {
                // Isolate alpha-zero geometry from the authored visible line
                // and notes, without changing the input archive.
                for line in &mut scene.chart.lines {
                    line.object.alpha = crate::core::AnimFloat::fixed(0.);
                    for note in &mut line.notes {
                        note.object.alpha = crate::core::AnimFloat::fixed(0.);
                    }
                }
            }
            let clock = std::rc::Rc::new(std::cell::Cell::new(0.));
            let clock_read = clock.clone();
            let mut tm = TimeManager::manual(Box::new(move || clock_read.get()));
            scene.enter(&mut tm, None).unwrap();
            let font = crate::ui::FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = crate::ui::TextPainter::new(font, None);
            for time in [0., 0.01, 0.2, 0.69, 0.7, 0.71, 0.8, 1., 1.1, 1.28, 1.3] {
                clock.set(time);
                scene.update(&mut tm).unwrap();
                let mut visible = Vec::new();
                for (id, (line, tracks)) in scene.chart.lines.iter().zip(&reference).enumerate() {
                    let expected = if isolate {
                        0.
                    } else {
                        tracks.iter().map(|track| track.value(scene.res.time).as_f64().unwrap()).sum::<f64>() as f32 / 255.
                    };
                    let actual = line.object.alpha.now();
                    assert!((actual - expected).abs() < 1e-5, "line={id}, entrance={time}: {actual} != {expected}");
                    if actual > 1e-6 {
                        visible.push((id, actual));
                    }
                }
                scene.render(&mut tm, &mut Ui::new(&mut painter, None)).unwrap();
                unsafe { get_internal_gl() }.flush();
                let pixels = get_screen_data();
                let bright_rows = pixels
                    .bytes
                    .chunks_exact(pixels.width as usize * 4)
                    .filter(|row| row.chunks_exact(4).any(|pixel| pixel[..3].iter().any(|&channel| channel > 10)))
                    .count();
                println!("entrance={time:.2} chart_time={:.4} visible_alpha={visible:?} bright_rows={bright_rows}", scene.res.time);
                if isolate {
                    assert_eq!(bright_rows, 0, "zero-alpha geometry must not leave visible pixels");
                }
                next_frame().await;
            }
        },
    );
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn editor_sidecars_load_in_production_with_chart_tempo_and_embedded_effects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    macroquad::Window::from_config(
        Conf { window_title: "Chart loading regression".into(), headless: true, ..Default::default() },
        async {
            let directory = tempfile::tempdir().unwrap();
            let mut raw = json!({
                "META":{"RPEVersion":240,"offset":0},
                "BPMList":[{"bpm":120,"startTime":[0,0,1]},{"bpm":60,"startTime":[4,0,1]}],
                "judgeLineList":[]
            });
            let mut fs = crate::fs::ExternalFileSystem(std::sync::Arc::new(crate::dir::Dir::new(directory.path()).unwrap()));
            let info = ChartInfo { chart:"notes.json".into(), ..Default::default() };
            for sidecar in [
                "{}", "{\n\"effects\": []\n}", "\u{feff}{\"videos\":[]}",
                r#"{"effects":[{"start":[4,0,1],"end":[8,0,1],"shader":"vignette"}]}"#,
                r#"{"bpm":90,"effects":[{"start":[4,0,1],"end":[8,0,1],"shader":"vignette","global":true}]}"#,
            ] {
                std::fs::write(directory.path().join("notes.json"), format!("\u{feff}\n{}", raw)).unwrap();
                std::fs::write(directory.path().join("extra.json"), sidecar).unwrap();
                let (chart, bytes, format) = finish(GameScene::load_chart(&mut fs, &info)).await.unwrap();
                assert_eq!(format, ChartFormat::Rpe);
                assert!(bytes.starts_with(b"\xef\xbb\xbf"));
                assert_eq!(chart.bpm_list.borrow().elements(), &[(0.,0.,120.),(4.,2.,60.)]);
                assert_eq!(chart.extra.effects.len() + chart.extra.global_effects.len(), usize::from(sidecar.contains("shader")));
            }
            // Embedded RPEN shaders survive assembly with sidecar effects.
            raw["effects"] = json!([{"name":"vignette","startTime":0,"endTime":8}]);
            std::fs::write(directory.path().join("notes.json"), raw.to_string()).unwrap();
            std::fs::write(directory.path().join("extra.json"), r#"{"effects":[{"start":[0,0,1],"end":[4,0,1],"shader":"vignette"}]}"#).unwrap();
            let (chart, _, _) = finish(GameScene::load_chart(&mut fs, &info)).await.unwrap();
            assert_eq!(chart.extra.effects.len() + chart.extra.global_effects.len(), 2);
            // Analysis remains independent of visual resources and bad sidecars.
            std::fs::write(directory.path().join("extra.json"), "{").unwrap();
            assert!(finish(GameScene::load_chart(&mut fs, &info)).await.is_err());
            assert!(finish(GameScene::load_chart_for_analysis(&mut fs, &info)).await.is_ok());
        },
    );
}

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn two_tone_colors_survive_the_optimized_chart_pass() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Two-tone shader regression".into(),
            headless: true,
            window_width: 640,
            window_height: 360,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            crate::core::init_assets();
            let dir = tempfile::tempdir().unwrap();
            std::fs::copy("assets/ending.ogg", dir.path().join("song.ogg")).unwrap();
            let preset = std::fs::read_to_string("assets/rpen/shaders/two_tone_pr.glsl").unwrap();
            std::fs::write(dir.path().join("two_tone-legacy-3.glsl"), format!("// @rpen-builtin\n{preset}")).unwrap();
            let font = crate::ui::FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = crate::ui::TextPainter::new(font, None);
            let background_bytes: Vec<_> = (0..16 * 9)
                .flat_map(|i| if i % 16 < 8 { [0, 0, 0, 255] } else { [255, 255, 255, 255] })
                .collect();
            let background: SafeTexture = Texture2D::from_rgba8(16, 9, &background_bytes).into();
            for (shader, global) in [("two_tones", false), ("/two_tone-legacy-3.glsl", true)] {
                let chart = json!({
                    "META": {"RPEVersion": 240, "offset": 0},
                    "BPMList": [{"bpm": 120, "startTime": [0,0,1]}],
                    "judgeLineList": [],
                    "effects": [{
                        "shader": shader, "start": [0,0,1], "end": [8,0,1], "global": global,
                        "vars": {"color1": [16,32,64,255], "color2": [1.,0.5,0.25,1.]}
                    }]
                });
                std::fs::write(dir.path().join("chart.json"), chart.to_string()).unwrap();
                let extra = json!({"bpm": 120, "effects": chart["effects"]});
                std::fs::write(dir.path().join("extra.json"), extra.to_string()).unwrap();
                let fs = Box::new(crate::fs::ExternalFileSystem(std::sync::Arc::new(crate::dir::Dir::new(dir.path()).unwrap())));
                let info = ChartInfo { music: "song.ogg".into(), background_dim: 0., ..Default::default() };
                let mut config = Config::default();
                config.sample_count = 1;
                config.volume_music = 0.;
                config.particle = false;
                let mut game = finish(GameScene::new(GameMode::Normal, info, config, fs, None, background.clone(), background.clone(), None, None, None))
                    .await
                    .unwrap();
                let mut tm = TimeManager::default();
                assert_eq!(game.effects.len() + game.chart.extra.effects.len(), 1);
                game.enter(&mut tm, None).unwrap();
                game.hide_ui = true;
                game.res.time = 1.;
                game.res.alpha = 1.;
                game.tick_replay();
                for optimized in [false, true] {
                    game.res.config.effect_render_optimized = optimized;
                    game.render(&mut tm, &mut Ui::new(&mut painter, None)).unwrap();
                    unsafe { get_internal_gl() }.flush();
                    let data = get_screen_data();
                    let y = data.height as usize / 2;
                    let width = data.width as usize;
                    for (x, expected) in [(width / 4, [16, 32, 64]), (width * 3 / 4, [255, 128, 64])] {
                        let pixel = &data.bytes[(y * width + x) * 4..][..3];
                        assert!(
                            pixel.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2),
                            "shader={shader}, optimized={optimized}, x={x}, actual={pixel:?}, expected={expected:?}"
                        );
                    }
                    next_frame().await;
                }
            }
            #[cfg(feature = "video")]
            if let Some(path) = std::env::var_os("PHIRA_TEST_MIRRORED_SHADER_CHART") {
                verify_archive_shader_frames(path.into(), &mut painter).await;
            }
        },
    );
}

#[cfg(feature = "video")]
async fn verify_archive_shader_frames(path: std::path::PathBuf, painter: &mut crate::ui::TextPainter) {
    use crate::chart_video::{ChartVideoRenderer, VideoOptions};
    use crate::scene::LoadingScene;
    let time: f64 = std::env::var("PHIRA_TEST_SHADER_TIME").unwrap().parse().unwrap();
    let options = VideoOptions { width: 640, height: 360, start: time, end: Some(time + 0.1), sample_count: 1, ..Default::default() };
    let assert_multiple_colors = |label: &str, bytes: &[u8]| {
        let mut colors = std::collections::HashMap::new();
        for pixel in bytes.chunks_exact(4) {
            *colors.entry([pixel[0], pixel[1], pixel[2]]).or_insert(0usize) += 1;
        }
        let mut frequent = colors.into_iter().filter(|(_, count)| *count > 100).collect::<Vec<_>>();
        frequent.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));
        println!("{label}: dominant colors {frequent:?}");
        assert!(frequent.len() > 1, "{label}: effect flattened the frame");
    };

    let mut fs = crate::fs::fs_from_file(&path).unwrap();
    let info = finish(crate::fs::load_info(fs.as_mut())).await.unwrap();
    let (illustration, background, _) = finish(LoadingScene::load(fs.as_mut(), &info.illustration)).await.unwrap();
    let mut config = Config::default();
    config.sample_count = 1;
    config.volume_music = 0.;
    config.particle = false;
    let mut game = finish(GameScene::new(GameMode::Normal, info, config.clone(), fs, None, background, illustration, None, None, None))
        .await
        .unwrap();
    game.hide_ui = true;
    let mut tm = TimeManager::default();
    game.enter(&mut tm, None).unwrap();
    game.res.time = time - f64::from(game.offset());
    game.res.alpha = 1.;
    game.tick_replay();
    game.render(&mut tm, &mut Ui::new(painter, None)).unwrap();
    unsafe { get_internal_gl() }.flush();
    assert_multiple_colors("Gameplay", &get_screen_data().bytes);
    drop(game);

    let mut renderer = finish(ChartVideoRenderer::preview_with_config(path, options, config)).await.unwrap();
    renderer.preview_music_frame(painter, time, false).unwrap();
    assert_multiple_colors("RPEN native preview", renderer.frame_bytes());
    renderer.render_next_frame(painter).unwrap();
    assert_multiple_colors("Chart video frame", renderer.frame_bytes());
}
