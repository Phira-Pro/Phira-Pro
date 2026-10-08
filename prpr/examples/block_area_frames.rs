//! Capture the actual GameScene (background, lines, notes, HUD and block postprocess).
//! Hidden window, read-only chart data; no music playback or saved preferences.
use macroquad::prelude::*;
use prpr::{
    config::{Config, Mods},
    core::{NoteKind, BOLD_FONT, PGR_FONT},
    fs::{fs_from_file, load_info},
    judge::{JudgeStatus, Judgement},
    scene::{GameMode, GameScene, LoadingScene, Scene},
    time::TimeManager,
    ui::{FontArc, TextPainter, Ui},
};
use std::path::Path;

async fn finish<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = prpr::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}

fn conf() -> Conf {
    let mut conf = Conf {
        window_title: "Official block full-frame comparison".into(),
        window_width: std::env::var("BLOCK_CAPTURE_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(960),
        window_height: std::env::var("BLOCK_CAPTURE_HEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(720),
        headless: true,
        ..Default::default()
    };
    if std::env::var_os("BLOCK_CAPTURE_BENCH").is_some() {
        conf.platform.swap_interval = Some(0);
        conf.headless = std::env::var_os("BLOCK_BENCH_VISIBLE").is_none();
        conf.window_width = std::env::var("BLOCK_CAPTURE_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(960);
        conf.window_height = std::env::var("BLOCK_CAPTURE_HEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(720);
    }
    conf
}

#[macroquad::main(conf)]
async fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    set_pc_assets_folder("assets");
    let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
    let mut painter = TextPainter::new(font.clone(), None);
    PGR_FONT.with(|p| {
        *p.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/phigros.ttf").unwrap()).unwrap(), Some(font.clone())))
    });
    BOLD_FONT
        .with(|p| *p.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font))));
    let scale = std::env::var("BLOCK_CAPTURE_SCALE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 2);
    let capture_width = std::env::var("BLOCK_CAPTURE_WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(960 * scale);
    let capture_height = std::env::var("BLOCK_CAPTURE_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(720 * scale);
    let capture = render_target(capture_width, capture_height);
    let folder = std::env::var("BLOCK_CAPTURE_DIR").unwrap_or_else(|_| {
        if scale > 1 {
            "target/block-area-frames-2x"
        } else {
            "target/block-area-frames"
        }
        .to_owned()
    });
    let brightness = std::env::var("BLOCK_CAPTURE_BRIGHTNESS")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value));
    std::fs::create_dir_all(&folder).unwrap();
    next_frame().await;
    let mut charts: Vec<_> = [
        ("desultory", "data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13", vec![1., 4., 65., 65.64356, 67., 67.72277, 70., 71.8]),
        (
            "hate",
            "data/charts/custom/19d0e386-5929-4031-85fb-28ac8e6a472f",
            vec![
                0., 2., 4., 6., 8., 79., 79.2, 79.4, 79.46667, 79.9, 80.2, 81.9, 82.2, 134.96667, 136.9, 137., 138.8, 139.8, 141.8, 142.6,
            ],
        ),
    ]
    .into_iter()
    .map(|(name, path, times)| (name.to_owned(), path.to_owned(), times))
    .collect();
    if let Ok(path) = std::env::var("BLOCK_CAPTURE_CHART") {
        let times = std::env::var("BLOCK_CAPTURE_TIMES")
            .unwrap_or_else(|_| "0,1,6,7,10,15,25,50,90,115".into())
            .split(',')
            .map(|value| value.trim().parse::<f64>().expect("capture time in seconds"))
            .collect();
        charts = vec![("custom".to_owned(), path, times)];
    }
    for (name, path, times) in charts {
        let reference = std::env::var("BLOCK_REFERENCE_DIR").unwrap_or_else(|_| "target/block-area-reference".into());
        let metadata: Option<serde_json::Value> = std::fs::read(format!("{reference}/{name}/metadata.json"))
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap());
        if metadata.is_none() {
            println!("{name}: no video alignment metadata; using requested chart seconds");
        }
        let mut manifest = Vec::new();
        let mut scene = finish(async {
            let mut fs = fs_from_file(Path::new(&path)).unwrap();
            let mut info = load_info(fs.as_mut()).await.unwrap();
            // Phigros exposes remaining background brightness, while Phira's
            // chart metadata stores the black overlay's opacity.
            if let Some(brightness) = brightness {
                info.background_dim = 1.0 - brightness;
            }
            let (illustration, background, _) = LoadingScene::load(fs.as_mut(), &info.illustration).await.unwrap();
            let mut config = Config::default();
            config.mods = Mods::AUTOPLAY;
            config.particle = std::env::var_os("BLOCK_BENCH_AUTOPLAY").is_some();
            config.volume_sfx = 0.;
            config.volume_music = 0.;
            config.sample_count = std::env::var("BLOCK_CAPTURE_SAMPLES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(4);
            config.shader_pre_render = std::env::var_os("BLOCK_CAPTURE_PRE_RENDER").is_some();
            config.block_area_simple = std::env::var_os("BLOCK_CAPTURE_SIMPLE").is_some();
            GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None)
                .await
                .unwrap()
        })
        .await;
        if std::env::var_os("BLOCK_BENCH_BACKBUFFER").is_none() {
            scene.res.camera.render_target = Some(capture);
        }
        let mut tm = if std::env::var_os("BLOCK_CAPTURE_BENCH").is_some() {
            TimeManager::default()
        } else {
            TimeManager::manual(Box::new(|| 1000.))
        };
        if std::env::var_os("BLOCK_CAPTURE_BENCH").is_some() {
            let mut costs = Vec::new();
            let mut intervals = Vec::new();
            let mut previous = std::time::Instant::now();
            #[cfg(target_os = "windows")]
            let mut pacer = prpr::desktop_pacing::Pacer::new();
            let start = times[0];
            let frames = std::env::var("BLOCK_BENCH_FRAMES")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(480)
                .clamp(60, 10_000);
            let autoplay = std::env::var_os("BLOCK_BENCH_AUTOPLAY").is_some();
            let mut bad_notes = Vec::new();
            if autoplay {
                scene.judge.advance_to(&mut scene.chart, start);
            }
            for i in 0..frames {
                #[cfg(target_os = "windows")]
                pacer.wait();
                let begun = std::time::Instant::now();
                let t = start + i as f64 / 165.;
                scene.res.time = t;
                scene.res.alpha = 1.;
                tm.seek_to(t);
                if autoplay {
                    scene.judge.update(&mut scene.res, &mut scene.chart, &mut bad_notes);
                }
                scene.tick_replay();
                let mut ui = Ui::new(&mut painter, Some((0, 0, capture_width as i32, capture_height as i32)));
                scene.render(&mut tm, &mut ui).unwrap();
                unsafe {
                    get_internal_gl().flush();
                    miniquad::gl::glFinish();
                }
                if i >= 30 {
                    costs.push(begun.elapsed().as_secs_f64() * 1000.);
                    intervals.push(begun.duration_since(previous).as_secs_f64());
                }
                previous = begun;
                next_frame().await;
            }
            let avg = costs.iter().sum::<f64>() / costs.len() as f64;
            costs.sort_by(f64::total_cmp);
            println!("Full GameScene {capture_width}x{capture_height} chart={start}..{:.3}: render-loop FPS={:.2}, render avg={avg:.3}ms p95={:.3}ms p99={:.3}ms (desktop, MSAA {}, autoplay/particles {}, no music/live input)",
                start + frames as f64 / 165., intervals.len() as f64 / intervals.iter().sum::<f64>(),
                costs[costs.len() * 95 / 100], costs[costs.len() * 99 / 100], scene.res.config.sample_count, autoplay);
            continue;
        }
        for requested in times {
            let t = metadata
                .as_ref()
                .and_then(|m| m["extraction"].as_array())
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["requested_video_seconds"].as_f64().is_some_and(|v| (v - requested).abs() < 0.00002))
                })
                .and_then(|row| row["decoded_frame_pts_seconds"].as_f64())
                .unwrap_or(requested);
            scene.res.time = t;
            scene.res.alpha = 1.;
            for (li, line) in scene.chart.lines.iter_mut().enumerate() {
                for (ni, n) in line.notes.iter_mut().enumerate() {
                    if !n.fake && n.time < t && matches!(n.judge, JudgeStatus::NotJudged) {
                        scene.judge.commit(n.time as f64, Judgement::Perfect, li as u32, ni as u32, 0.);
                        n.judge = match n.kind {
                            NoteKind::Hold { end_time, .. } if end_time > t => JudgeStatus::Hold(true, n.time as f64, 0., false, f64::INFINITY),
                            _ => JudgeStatus::Judged,
                        };
                    }
                }
            }
            tm.seek_to(t);
            scene.tick_replay();
            let mut ui = Ui::new(&mut painter, Some((0, 0, capture_width as i32, capture_height as i32)));
            scene.render(&mut tm, &mut ui).unwrap();
            unsafe { get_internal_gl() }.flush();
            let (width, height) = (capture_width as usize, capture_height as usize);
            let mut bytes = vec![0; width * height * 4];
            unsafe {
                use miniquad::gl::*;
                let mut read = 0;
                glGetIntegerv(0x8CAA, &mut read);
                glBindFramebuffer(GL_READ_FRAMEBUFFER, capture.render_pass.gl_internal_id(get_internal_gl().quad_context));
                glReadPixels(0, 0, width as i32, height as i32, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
                glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
                assert_eq!(glGetError(), 0);
            }
            // PNG top row is at the top of the window; GL readback starts below.
            for y in 0..height / 2 {
                let (a, b) = bytes.split_at_mut((height - y - 1) * width * 4);
                a[y * width * 4..(y + 1) * width * 4].swap_with_slice(&mut b[..width * 4]);
            }
            let file = format!("{folder}/{name}-{requested:.5}.png");
            image::save_buffer(&file, &bytes, width as u32, height as u32, image::ColorType::Rgba8).unwrap();
            println!("{file}: combo={}, aspect={}, shader clock={:.6}s", scene.judge.combo(), scene.res.aspect_ratio, get_time());
            manifest.push(serde_json::json!({"requested_video_seconds":requested,"chart_seconds":t,"file":file,"width":width,"height":height,"combo":scene.judge.combo(),"background_brightness":1.0-scene.res.info.background_dim,"shader_clock_seconds_approx":get_time()}));
            if std::env::var_os("BLOCK_CAPTURE_WHITE").is_some() {
                // Keep the evaluated chart pose, but isolate its postprocess
                // over gray so colored notes/background cannot hide red rims.
                set_camera(&scene.res.camera);
                unsafe { get_internal_gl() }.quad_gl.render_pass(Some(capture.render_pass));
                clear_background(Color::new(0.1, 0.1, 0.1, 1.));
                scene.chart.render_block_overlay(&mut scene.res);
                unsafe { get_internal_gl() }.flush();
                let mut gray_bytes = vec![0_u8; width * height * 4];
                unsafe {
                    use miniquad::gl::*;
                    let mut read = 0;
                    glGetIntegerv(0x8CAA, &mut read);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, capture.render_pass.gl_internal_id(get_internal_gl().quad_context));
                    glReadPixels(0, 0, width as i32, height as i32, GL_RGBA, GL_UNSIGNED_BYTE, gray_bytes.as_mut_ptr() as _);
                    glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
                    assert_eq!(glGetError(), 0);
                }
                let tinted = gray_bytes.chunks_exact(4).filter(|p| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap() > 1).count();
                let gray_file = format!("{folder}/{name}-{requested:.5}-gray.png");
                for y in 0..height / 2 {
                    let (a, b) = gray_bytes.split_at_mut((height - y - 1) * width * 4);
                    a[y * width * 4..(y + 1) * width * 4].swap_with_slice(&mut b[..width * 4]);
                }
                image::save_buffer(&gray_file, &gray_bytes, width as u32, height as u32, image::ColorType::Rgba8).unwrap();
                manifest.last_mut().unwrap()["white_probe_tinted_pixels"] = tinted.into();
                println!("{gray_file}: tinted pixels={tinted}");
                assert_eq!(tinted, 0, "white chart pose contains colored rim pixels");
            }
            next_frame().await;
        }
        std::fs::write(format!("{folder}/{name}-manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }
}
