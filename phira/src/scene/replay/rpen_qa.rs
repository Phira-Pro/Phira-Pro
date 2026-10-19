use super::*;
use serde_json::{json, Value};
use std::{
    io::{Seek, Write},
    path::Path,
};

fn image(painter: &mut TextPainter, game: &mut GameScene, tm: &mut TimeManager, w: i32, h: i32, name: &str) -> Image {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    game.render(tm, &mut ui).unwrap();
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0, "{name}");
    }
    let image = Image {
        width: w as u16,
        height: h as u16,
        bytes,
    };
    image.export_png(&format!("target/replay-qa/{name}.png"));
    image
}
async fn game(info: ChartInfo) -> GameScene {
    let mut c = Config::default();
    c.volume_music = 0.;
    c.volume_sfx = 0.;
    c.sample_count = 1;
    c.interactive = false;
    c.adjust_time = false;
    local(GameScene::new(
        GameMode::Normal,
        info,
        c,
        fs_from_path("custom/rpen-qa").unwrap(),
        None,
        BLACK_TEXTURE.clone(),
        BLACK_TEXTURE.clone(),
        None,
        None,
        None,
    ))
    .await
    .unwrap()
}
fn difference(a: &Image, b: &Image) -> usize {
    a.bytes.chunks_exact(4).zip(b.bytes.chunks_exact(4)).filter(|(a, b)| a != b).count()
}
pub async fn run(root: &Path, painter: &mut TextPainter) {
    let directory = root.join("data/charts/custom/rpen-qa");
    let mut info = chart_fixture(root, "custom/rpen-qa");
    info.name = "RPEN extensions QA".into();
    info.format = Some(prpr::info::ChartFormat::Rpe);
    info.use_rpe_170_speed = Some(true);
    info.use_rpen = Some(true);
    let source: Value = serde_json::from_str(include_str!("../../../../prpr/tests/fixtures/rpen-extensions/chart.json")).unwrap();
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&source).unwrap()).unwrap();
    std::fs::write(directory.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    let warnings = local(prpr::parse::lint(&source.to_string())).await.unwrap();
    assert!(warnings.has_rpen_features);
    // Exercise the production ZIP importer, persistence, default-on and explicit opt-out.
    let mut zip = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    let mut import_info = info.clone();
    import_info.use_rpen = None;
    for (name, bytes) in [
        ("chart.json", serde_json::to_vec(&source).unwrap()),
        ("info.yml", serde_yaml::to_string(&import_info).unwrap().into_bytes()),
        ("song.wav", wave(12)),
        ("background.png", std::fs::read(directory.join("background.png")).unwrap()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let mut archive = zip.finish().unwrap();
    archive.rewind().unwrap();
    let stage = tempfile::tempdir_in(root).unwrap();
    let (_, warnings) = local(crate::scene::import_chart_to(stage.path(), "custom/rpen-imported".into(), archive))
        .await
        .unwrap();
    assert!(warnings.has_rpen_features);
    assert!(crate::scene::parse_warnings_to_string(&warnings).unwrap().contains("RPEN"));
    let imported: ChartInfo = serde_yaml::from_slice(&std::fs::read(stage.path().join("info.yml")).unwrap()).unwrap();
    assert_eq!(imported.use_rpen, Some(true));
    let mut enabled = game(info.clone()).await;
    assert!((enabled.chart.lines[0].notes.iter().map(|n| n.time).fold(f64::INFINITY, f64::min) - 4.).abs() < 1e-6);
    assert_eq!(enabled.chart.next_layer_renderers.len(), 2);
    assert!(enabled.chart.next_single_inversion);
    let mut tm = TimeManager::default();
    enabled.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    enabled.update(&mut tm).unwrap();
    tm.seek_to(0.);
    enabled.update(&mut tm).unwrap();
    for (w, h, name) in [(960, 720, "tablet"), (1280, 720, "wide"), (1280, 548, "phone")] {
        enabled.res.time = 2.;
        enabled.res.alpha = 1.;
        enabled.tick_replay();
        tm.seek_to(2.);
        let on = image(painter, &mut enabled, &mut tm, w, h, &format!("rpen-layered-camera-{name}"));
        let camera = enabled.chart.rpen.as_mut().unwrap().camera.take();
        let off = image(painter, &mut enabled, &mut tm, w, h, &format!("rpen-flat-{name}"));
        assert!(difference(&on, &off) > 300, "camera must change real chart/noise pixels");
        enabled.chart.rpen.as_mut().unwrap().camera = camera;
    }
    let point = enabled
        .chart
        .camera_screen_matrix(2., enabled.res.aspect_ratio)
        .transform_point(&prpr::core::Point::new(0., 0.));
    enabled.res.time = 2.;
    enabled.update_replay_block_touches(&[(17, vec2(point.x, point.y))]);
    assert_eq!(enabled.chart.blocked_touches.len(), 1, "separate layers must not cancel each other's masks");
    let mut disabled_info = info.clone();
    disabled_info.use_rpen = Some(false);
    let disabled = game(disabled_info).await;
    assert_eq!(disabled.chart.lines[0].notes.iter().map(|n| n.time).fold(f64::INFINITY, f64::min), 2.);
    assert!(disabled.chart.rpen.is_none());
    assert!(disabled.chart.next_layer_renderers.is_empty());
    assert!(disabled.chart.lines[0].ctrl_obj.borrow().skew.is_default());
    // All 30 new presets must compile; duplicate slots keep their own ordering/global flag.
    let names = [
        "liquid",
        "flowing",
        "image_noise",
        "snow",
        "glow_effect",
        "flip",
        "night_vision",
        "scanline",
        "rain",
        "rainbow",
        "flare",
        "underwater",
        "fog",
        "kaleidoscope",
        "emboss",
        "sobel",
        "oil_painting",
        "distortion",
        "hologram",
        "burn",
        "camera",
        "lightning",
        "old_tv",
        "bokeh",
        "neon",
        "heat_distortion",
        "rays",
        "color_shift",
        "wave",
        "two_tone",
    ];
    for name in names {
        let chart = json!({"effects":[{"shader":format!("/{name}_pr.glsl"),"start":0,"end":8,"vars":{}}],"judgeLineList":[{}]});
        let effects = local(prpr::parse::rpen::load_effects(
            &chart,
            &mut prpr::core::BpmList::new(vec![(0., 120.)]),
            fs_from_path("custom/rpen-qa").unwrap().as_mut(),
        ))
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(effects.len(), 1);
    }
    // Real built-in skin packages: multi-pass component targets, images and dimensions.
    for skin in ["NoiseField", "Ocean"] {
        let mut chart = source.clone();
        chart.as_object_mut().unwrap().remove("globalEvents");
        chart["noiseAreaLayers"] = json!([{"id":"default","name":"skin","skin":format!("shader/{skin}"),"parameters":{}}]);
        chart["blockAreaList"].as_array_mut().unwrap().truncate(1);
        std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
        let mut game = game(info.clone()).await;
        let mut clock = TimeManager::default();
        game.enter(&mut clock, None).unwrap();
        clock.seek_to(GameScene::BEFORE_TIME + 0.1);
        game.update(&mut clock).unwrap();
        clock.seek_to(0.);
        game.update(&mut clock).unwrap();
        for (time, w, h, suffix) in [
            (1., 960, 720, "tablet"),
            (1., 960, 720, "paused"),
            (2., 1280, 548, "phone"),
            (0.5, 960, 720, "reverse"),
        ] {
            game.res.time = time;
            game.res.alpha = 1.;
            game.tick_replay();
            game.chart.blocked_touches = vec![(3, prpr::core::Vector::new(0.1, 0.1))];
            clock.seek_to(time);
            let on = image(painter, &mut game, &mut clock, w, h, &format!("rpen-skin-{skin}-{suffix}"));
            assert!(!game.chart.next_layer_skins[0].as_ref().unwrap().failed(), "skin must render successfully");
            let colored = on
                .bytes
                .chunks_exact(4)
                .filter(|p| {
                    if skin == "NoiseField" {
                        p[0] > p[1].saturating_add(15) && p[0] > p[2].saturating_add(15)
                    } else {
                        p[2] > p[0].saturating_add(15) && p[1] > p[0].saturating_add(10)
                    }
                })
                .count();
            assert!(colored > 2000, "{skin} must draw its actual colored region, not merely flip the scene: {colored}");
            let saved = std::mem::take(&mut game.chart.next_block_areas);
            let saved_skins = std::mem::take(&mut game.chart.next_layer_skins);
            let off = image(painter, &mut game, &mut clock, w, h, &format!("rpen-skin-{skin}-{suffix}-off"));
            assert!(difference(&on, &off) > 300, "skin must draw real pixels");
            game.chart.next_block_areas = saved;
            game.chart.next_layer_skins = saved_skins;
        }
    }
    // A custom skin verifies frame history and the low-performance channel with exact pixels.
    let skin_dir = directory.join("shader/HistoryQA");
    std::fs::create_dir_all(&skin_dir).unwrap();
    let main="precision highp float;varying highp vec2 uv;uniform sampler2D amount;uniform float tint; // %0.25%\nvoid main(){gl_FragColor=vec4(texture2D(amount,uv).r,tint,0.,1.);}";
    let tick="precision highp float;varying highp vec2 uv;uniform sampler2D previous;void main(){gl_FragColor=vec4(texture2D(previous,uv).r+.1,0.,0.,1.);}";
    let low="precision highp float;varying highp vec2 uv;uniform sampler2D amount;uniform float tint;void main(){gl_FragColor=vec4(texture2D(amount,uv).r,0.,tint,1.);}";
    for (name, bytes) in [
        ("main.glsl", main.as_bytes()),
        ("tick.glsl", tick.as_bytes()),
        ("low.glsl", low.as_bytes()),
    ] {
        std::fs::write(skin_dir.join(name), bytes).unwrap();
    }
    let channel = |shader: &str| json!({"targets":{"history":{"format":"r8","scaleX":0.25,"scaleY":0.25,"noClear":true}},"passes":[{"shader":"tick.glsl","output":"history","inputs":{"previous":"history"}},{"shader":shader,"output":"$screen","inputs":{"amount":"history"}}]});
    let mut config = channel("main.glsl");
    config["version"] = json!(1);
    config["lowPerformance"] = channel("low.glsl");
    std::fs::write(skin_dir.join("config.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    for low in [false, true] {
        let mut chart = source.clone();
        chart.as_object_mut().unwrap().remove("globalEvents");
        chart["noiseAreaLayers"] =
            json!([{"id":"default","name":"history","skin":"shader/HistoryQA","channel":if low{"low"}else{"normal"},"parameters":{"tint":0.4}}]);
        chart["blockAreaList"].as_array_mut().unwrap().truncate(1);
        std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
        let mut game = game(info.clone()).await;
        let mut tm = TimeManager::default();
        game.enter(&mut tm, None).unwrap();
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game.update(&mut tm).unwrap();
        tm.seek_to(0.);
        game.update(&mut tm).unwrap();
        for (time, red, suffix) in [(1., 26, "first"), (1., 26, "paused"), (2., 52, "next"), (0.5, 26, "reverse")] {
            game.res.time = time;
            game.res.alpha = 1.;
            game.tick_replay();
            tm.seek_to(time);
            let image = image(painter, &mut game, &mut tm, 640, 480, &format!("rpen-history-{}-{suffix}", if low { "low" } else { "normal" }));
            let p = &image.bytes[(240 * 640 + 320) * 4..][..4];
            assert!((p[0] as i32 - red).abs() <= 2, "history red {time}: {p:?}");
            assert!((p[if low { 2 } else { 1 }] as i32 - 102).abs() <= 2, "parameters and channel: {p:?}");
            assert!(p[if low { 1 } else { 2 }] <= 1);
        }
    }
    // Shader parameter tracks use their owner line's factor exactly once.
    std::fs::write(
        directory.join("qa.glsl"),
        "#version 100\nprecision mediump float;varying vec2 uv;uniform vec3 tint; // %0.0,0.0,0.0%\nvoid main(){gl_FragColor=vec4(tint,1.);}",
    )
    .unwrap();
    let mut chart = source.clone();
    chart.as_object_mut().unwrap().remove("globalEvents");
    chart["blockAreaList"] = json!([]);
    chart.as_object_mut().unwrap().remove("noiseAreaLayers");
    chart["judgeLineList"][0]["extended"] = json!({"paintEvents":[{"shader":"/qa.glsl","startTime":0,"endTime":10,"line":0,"vars":{"tint":[{"startTime":0,"endTime":4,"start":[0,0,0],"end":[1,0.5,0.25],"easingType":1}]}}]});
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
    let mut shader_game = game(info.clone()).await;
    let mut tm = TimeManager::default();
    shader_game.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    shader_game.update(&mut tm).unwrap();
    tm.seek_to(0.);
    shader_game.update(&mut tm).unwrap();
    for (time, rgb) in [(1., [64, 32, 16]), (2., [128, 64, 32])] {
        shader_game.res.time = time;
        shader_game.res.alpha = 1.;
        shader_game.tick_replay();
        tm.seek_to(time);
        let image = image(painter, &mut shader_game, &mut tm, 640, 480, &format!("rpen-shader-vars-{time}"));
        let p = &image.bytes[(240 * 640 + 320) * 4..][..3];
        for (i, expected) in rgb.into_iter().enumerate() {
            assert!((p[i] as i32 - expected).abs() <= 2, "shader vars at {time}: {p:?}");
        }
    }
    // Clip a plane inside the near plane, beyond the far plane and behind the lens.
    for (z, yaw, visible, name) in [
        (1000., 0., true, "normal"),
        (0.5, 0., false, "near"),
        (-1000., 0., false, "behind"),
        (-1000., 180., true, "backface"),
    ] {
        let mut chart = source.clone();
        chart["blockAreaList"] = json!([]);
        chart.as_object_mut().unwrap().remove("noiseAreaLayers");
        chart["judgeLineList"][0]["eventLayers"][0]["moveXEvents"] =
            json!([{"startTime":[0,0,1],"endTime":[16,0,1],"start":0,"end":0,"easingType":1}]);
        chart["globalEvents"] = json!({"version":1,"types":{"spatial-camera/position":{"columns":{"z":{"events":[{"startTime":0,"endTime":10,"start":z,"end":z}]}}},"spatial-camera/rotation":{"columns":{"yaw":{"events":[{"startTime":0,"endTime":10,"start":yaw,"end":yaw}]}}}}});
        std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
        let mut game = game(info.clone()).await;
        let mut tm = TimeManager::default();
        game.enter(&mut tm, None).unwrap();
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game.update(&mut tm).unwrap();
        tm.seek_to(0.);
        game.update(&mut tm).unwrap();
        game.res.time = 1.;
        game.res.alpha = 1.;
        game.tick_replay();
        tm.seek_to(1.);
        let image = image(painter, &mut game, &mut tm, 640, 480, &format!("rpen-clip-{name}"));
        let gold = image
            .bytes
            .chunks_exact(4)
            .filter(|p| p[0] > 180 && p[1] > 130 && p[0] > p[2].saturating_add(40) && p[1] > p[2].saturating_add(30))
            .count();
        assert_eq!(gold > 100, visible, "{name}: {gold}");
    }
    // Test the actual homogeneous GPU clip model without relying on a pose clamp.
    push_camera_state();
    for (depth, visible) in [(0.0005_f32, false), (0.002, true), (1., true), (149., true), (151., false)] {
        set_camera(&Camera2D {
            zoom: vec2(1., 1.),
            viewport: Some((0, 0, 64, 64)),
            ..Default::default()
        });
        clear_background(BLACK);
        let matrix = prpr::core::Matrix::identity() * depth;
        let model = prpr::core::rpen_scene::clip_model(&matrix);
        unsafe { get_internal_gl() }.quad_gl.push_model_matrix(model);
        draw_rectangle(-0.5, -0.5, 1., 1., WHITE);
        unsafe { get_internal_gl() }.flush();
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        let mut pixel = [0_u8; 4];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(32, 32, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        assert_eq!(pixel[0] > 200, visible, "GPU depth {depth}: {pixel:?}");
    }
    pop_camera_state();
    // A camera pan can bring notes far outside the original 2D viewport on screen.
    let mut chart = source.clone();
    chart["blockAreaList"] = json!([]);
    chart.as_object_mut().unwrap().remove("noiseAreaLayers");
    chart["judgeLineList"][0]["eventLayers"][0]["moveXEvents"] = json!([{"startTime":[0,0,1],"endTime":[16,0,1],"start":0,"end":0,"easingType":1}]);
    chart["judgeLineList"][0]["notes"].as_array_mut().unwrap().truncate(1);
    chart["judgeLineList"][0]["notes"][0]["positionX"] = json!(5000);
    chart["globalEvents"] =
        json!({"version":1,"types":{"spatial-camera/position":{"columns":{"x":{"events":[{"startTime":0,"endTime":10,"start":5000,"end":5000}]}}}}});
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&chart).unwrap()).unwrap();
    let mut panned = game(info.clone()).await;
    let mut tm = TimeManager::default();
    panned.enter(&mut tm, None).unwrap();
    tm.seek_to(GameScene::BEFORE_TIME + 0.1);
    panned.update(&mut tm).unwrap();
    tm.seek_to(0.);
    panned.update(&mut tm).unwrap();
    panned.res.time = 4.;
    panned.res.alpha = 1.;
    panned.tick_replay();
    tm.seek_to(4.);
    let shot = image(painter, &mut panned, &mut tm, 640, 480, "rpen-pan-outside-original-viewport");
    let blue = shot
        .bytes
        .chunks_exact(4)
        .filter(|p| p[2] > p[0].saturating_add(40) && p[1] > p[0].saturating_add(30))
        .count();
    assert!(blue > 20, "offscreen source note must remain visible after camera pan: {blue}");
    std::fs::write(directory.join("chart.json"), serde_json::to_vec(&source).unwrap()).unwrap();
    // Native desktop QA can import this same package without touching real user data.
    let preview = Path::new("target/replay-qa/rpen-native-fixture");
    chart_fixture(preview, "custom/rpen-qa");
    let out = preview.join("data/charts/custom/rpen-qa");
    std::fs::write(out.join("chart.json"), serde_json::to_vec(&source).unwrap()).unwrap();
    std::fs::write(out.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    println!("RPEN: ZIP warning/default-on, opt-out, trajectory/factor/skew, layers, camera at three aspects, 30 shader presets and both multi-pass skins passed");
}
