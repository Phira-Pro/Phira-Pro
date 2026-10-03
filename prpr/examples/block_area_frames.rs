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
    Conf {
        window_title: "Official block full-frame comparison".into(),
        window_width: 960,
        window_height: 720,
        headless: true,
        ..Default::default()
    }
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
    let capture = render_target(960 * scale, 720 * scale);
    let folder = if scale > 1 {
        "target/block-area-frames-2x"
    } else {
        "target/block-area-frames"
    };
    std::fs::create_dir_all(folder).unwrap();
    next_frame().await;
    for (name, path, times) in [
        ("desultory", "data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13", vec![1., 4., 65., 65.64356, 67., 67.72277, 70., 71.8]),
        ("hate", "data/charts/custom/19d0e386-5929-4031-85fb-28ac8e6a472f", vec![134.96667, 136.9, 137., 138.8, 139.8, 141.8, 142.6]),
    ] {
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(format!("target/block-area-reference/{name}/metadata.json")).unwrap()).unwrap();
        let mut manifest = Vec::new();
        let mut scene = finish(async {
            let mut fs = fs_from_file(Path::new(path)).unwrap();
            let info = load_info(fs.as_mut()).await.unwrap();
            let (illustration, background, _) = LoadingScene::load(fs.as_mut(), &info.illustration).await.unwrap();
            let mut config = Config::default();
            config.mods = Mods::AUTOPLAY;
            config.particle = false;
            config.volume_sfx = 0.;
            config.volume_music = 0.;
            config.sample_count = 4;
            GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None)
                .await
                .unwrap()
        })
        .await;
        scene.res.camera.render_target = Some(capture);
        let mut tm = TimeManager::manual(Box::new(|| 1000.));
        for requested in times {
            let t = metadata["extraction"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["requested_video_seconds"].as_f64().is_some_and(|v| (v - requested).abs() < 0.00002))
                .and_then(|row| row["decoded_frame_pts_seconds"].as_f64())
                .unwrap_or(requested);
            scene.res.time = t;
            scene.res.alpha = 1.;
            for (li, line) in scene.chart.lines.iter_mut().enumerate() {
                for (ni, n) in line.notes.iter_mut().enumerate() {
                    if n.time < t && matches!(n.judge, JudgeStatus::NotJudged) {
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
            let mut ui = Ui::new(&mut painter, Some((0, 0, (960 * scale) as i32, (720 * scale) as i32)));
            scene.render(&mut tm, &mut ui).unwrap();
            unsafe { get_internal_gl() }.flush();
            let (width, height) = ((960 * scale) as usize, (720 * scale) as usize);
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
            manifest.push(serde_json::json!({"requested_video_seconds":requested,"chart_seconds":t,"file":file,"width":width,"height":height,"combo":scene.judge.combo(),"shader_clock_seconds_approx":get_time()}));
            next_frame().await;
        }
        std::fs::write(format!("{folder}/{name}-manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }
}
