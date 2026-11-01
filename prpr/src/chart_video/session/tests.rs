//! Real offline scene, pixel readback and encoded soundtrack regression.
use super::*;
use crate::core::{BOLD_FONT, PGR_FONT};
use crate::ui::FontArc;
use std::{io::Write, path::Path};

async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = crate::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}

fn wave(path: &Path, seconds: f32, amplitude: f32, frequency: f32) {
    let count = (44100. * seconds) as u32;
    let mut file = std::fs::File::create(path).unwrap();
    for bytes in [
        b"RIFF".as_slice(),
        &(36 + count * 2).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &44100u32.to_le_bytes(),
        &88200u32.to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
        b"data",
        &(count * 2).to_le_bytes(),
    ] {
        file.write_all(bytes).unwrap();
    }
    for i in 0..count {
        let sample = ((i as f32 * frequency * std::f32::consts::TAU / 44100.).sin() * amplitude * 32767.) as i16;
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}

fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let pack = root.join("skin");
    let chart = root.join("chart");
    std::fs::create_dir(&pack).unwrap();
    std::fs::create_dir(&chart).unwrap();
    for entry in std::fs::read_dir("assets/respack").unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), pack.join(entry.file_name())).unwrap();
    }
    let mut info: serde_yaml::Value = serde_yaml::from_slice(&std::fs::read(pack.join("info.yml")).unwrap()).unwrap();
    info["name"] = "Export regression skin".into();
    info["colorPerfect"] = 65535u32.into();
    std::fs::write(pack.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    Image::gen_image_color(24, 12, MAGENTA).export_png(pack.join("click.png").to_str().unwrap());
    Image::gen_image_color(24, 12, GREEN).export_png(pack.join("click_mh.png").to_str().unwrap());
    wave(&pack.join("click.wav"), 0.25, 0.4, 880.);
    wave(&pack.join("ending.wav"), 0.5, 0.3, 440.);
    wave(&chart.join("song.wav"), 2., 0., 0.);
    Image::gen_image_color(8, 8, BLACK).export_png(chart.join("background.png").to_str().unwrap());
    let info = ChartInfo {
        name: "Export regression".into(),
        music: "song.wav".into(),
        ..Default::default()
    };
    std::fs::write(chart.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
    let note = |time: u32, x: f32| serde_json::json!({"type":1,"time":time,"positionX":x,"holdTime":0,"speed":1,"floorPosition":time as f32/64.});
    let event = |value: f32| serde_json::json!([{"startTime":0,"endTime":100000,"start":value,"end":value,"start2":0.5,"end2":0.5}]);
    let json = serde_json::json!({"formatVersion":3,"offset":0,"numOfNotes":3,"judgeLineList":[{"bpm":120,"notesAbove":[note(64,-2.),note(64,2.),note(96,0.)],"notesBelow":[],"speedEvents":[{"startTime":0,"endTime":100000,"value":1}],"judgeLineMoveEvents":event(0.5),"judgeLineRotateEvents":event(0.),"judgeLineDisappearEvents":event(1.)}]});
    std::fs::write(chart.join("chart.json"), serde_json::to_vec(&json).unwrap()).unwrap();
    (chart, pack)
}

async fn pixels(chart: &Path, options: VideoOptions, painter: &mut TextPainter, name: &str) -> Vec<u8> {
    let mut renderer = local(ChartVideoRenderer::preview(chart.to_owned(), options.clone())).await.unwrap();
    assert!(renderer.scene.res.config.autoplay());
    assert_eq!(renderer.scene.res.config.offset_indicator, options.visuals.offset_visible(options.show_hud));
    assert_eq!(renderer.scene.res.config.ap_fc_indicator, options.visuals.ap_fc_indicator);
    assert_eq!(renderer.scene.res.config.double_hint, options.visuals.double_hint);
    assert_eq!(renderer.scene.res.config.particle, options.visuals.particle);
    assert_eq!(renderer.scene.res.config.show_acc, options.visuals.show_acc);
    assert_eq!(renderer.scene.res.config.show_judgement_details, options.visuals.show_judgement_details);
    assert_eq!(renderer.scene.res.config.show_perfect_combo, options.visuals.show_perfect_combo);
    if options.res_pack_path.is_some() {
        let texture = renderer.scene.res.res_pack.note_style.click.get_texture_data();
        assert_eq!((texture.width, texture.height), (24, 12));
        assert_eq!(&texture.bytes[..4], &[255, 0, 255, 255]);
        assert!((renderer.scene.res.res_pack.sfx_click.length() - 0.25).abs() < 0.01);
        assert!((renderer.scene.res.res_pack.ending.length() - 0.5).abs() < 0.01);
    }
    while !renderer.finished() {
        renderer.render_next_frame(painter).unwrap();
    }
    let bytes = renderer.frame_bytes().to_vec();
    save_video_png(&bytes, options.width, options.height, name);
    bytes
}

fn save_video_png(bytes: &[u8], width: u32, height: u32, name: &str) {
    // GL readback is bottom-up; match the encoder's vflip only for saved previews.
    let raw = image::RgbaImage::from_raw(width, height, bytes.to_vec()).unwrap();
    image::imageops::flip_vertical(&raw)
        .save(format!("target/render-resource-qa/{name}.png"))
        .unwrap();
}

#[test]
#[ignore = "requires desktop OpenGL/audio and FFmpeg; run separately with --ignored --test-threads=1"]
fn export_resource_pack_visuals_and_audio() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    std::env::set_current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Offline render regression".into(),
            window_width: 640,
            window_height: 480,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            std::fs::create_dir_all("target/render-resource-qa").unwrap();
            let root = tempfile::tempdir_in("target/render-resource-qa").unwrap();
            let (chart, pack) = fixture(root.path());
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|p| *p.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            BOLD_FONT.with(|p| *p.borrow_mut() = Some(TextPainter::new(font, None)));
            let options = VideoOptions {
                width: 640,
                height: 360,
                fps: 20,
                start: 0.4,
                end: Some(0.45),
                sample_count: 1,
                res_pack_path: Some(pack.to_string_lossy().into_owned()),
                ..Default::default()
            };
            let baseline = pixels(&chart, options.clone(), &mut painter, "custom-baseline").await;
            assert!(
                baseline.chunks_exact(4).filter(|p| p[1] > 200 && p[0] < 30 && p[2] < 80).count() > 20,
                "custom highlighted note must appear in the exported pixels"
            );
            for name in ["offset", "accuracy", "judgements", "early-late", "ap-fc", "double-hint"] {
                let mut changed = options.clone();
                match name {
                    "offset" => changed.visuals.offset_indicator = Some(true),
                    "accuracy" => changed.visuals.show_acc = true,
                    "judgements" => changed.visuals.show_judgement_details = true,
                    "early-late" => changed.visuals.show_early_late = true,
                    "ap-fc" => changed.visuals.ap_fc_indicator = false,
                    "double-hint" => changed.visuals.double_hint = false,
                    _ => unreachable!(),
                }
                let frame = pixels(&chart, changed, &mut painter, name).await;
                assert!(
                    frame.iter().zip(&baseline).filter(|(a, b)| a != b).count() > 20,
                    "{name} must change actual pixels with the main HUD hidden"
                );
            }
            let mut builtin = options.clone();
            builtin.res_pack_path = None;
            assert_ne!(pixels(&chart, builtin, &mut painter, "builtin").await, baseline);
            let mut hud = options.clone();
            hud.show_hud = true;
            pixels(&chart, hud, &mut painter, "legacy-hud").await;
            let mut effects = options.clone();
            effects.start = 0.95;
            effects.end = Some(1.15);
            let enabled = pixels(&chart, effects.clone(), &mut painter, "particles-on").await;
            effects.visuals.particle = false;
            assert_ne!(pixels(&chart, effects, &mut painter, "particles-off").await, enabled);

            let mut export = options;
            export.start = 0.;
            export.end = Some(2.);
            export.show_hud = true;
            export.visuals.offset_indicator = Some(true);
            export.visuals.show_acc = true;
            export.visuals.show_judgement_details = true;
            export.visuals.show_early_late = true;
            export.presentation.ending = true;
            export.presentation.ending_seconds = 2.;
            export.presentation.audio.ending_delay_seconds = 0.;
            export.presentation.audio.fade_seconds = 0.;
            let output = PathBuf::from("target/render-resource-qa/custom-resource.mp4");
            let mut renderer = local(ChartVideoRenderer::new(chart, export, output.clone())).await.unwrap();
            let mut reference = None;
            while !renderer.finished() {
                let frame = renderer.progress().0;
                renderer.render_next_frame(&mut painter).unwrap();
                if frame == 8 {
                    let bytes = renderer.frame_bytes().to_vec();
                    save_video_png(&bytes, 640, 360, "encoded-reference");
                    reference = Some(bytes);
                }
            }
            renderer.finish().unwrap();
            let ffmpeg = crate::chart_video::ffmpeg_candidates()
                .into_iter()
                .find(|path| {
                    std::process::Command::new(path)
                        .arg("-version")
                        .output()
                        .is_ok_and(|result| result.status.success())
                })
                .expect("FFmpeg is required for this regression");
            let screenshot = "target/render-resource-qa/decoded-mp4.png";
            let status = std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-y", "-ss", "0.4", "-i"])
                .arg(&output)
                .args(["-frames:v", "1", screenshot])
                .status()
                .unwrap();
            assert!(status.success());
            let decoded_frame = image::open(screenshot).unwrap().into_rgba8();
            let raw = image::RgbaImage::from_raw(640, 360, reference.unwrap()).unwrap();
            let upright = image::imageops::flip_vertical(&raw);
            let difference = |expected: &image::RgbaImage| {
                // H.264 keeps RGB appearance but does not preserve framebuffer alpha.
                decoded_frame
                    .pixels()
                    .zip(expected.pixels())
                    .map(|(a, b)| (0..3).map(|channel| (a[channel] as f64 - b[channel] as f64).abs()).sum::<f64>())
                    .sum::<f64>()
                    / (decoded_frame.width() * decoded_frame.height() * 3) as f64
            };
            let upright_error = difference(&upright);
            assert!(upright_error < 8., "decoded MP4 must match the upright reference, error={upright_error}");
            assert!(upright_error < difference(&raw) / 4., "encoded output must have the same orientation as the saved previews");
            let decoded = std::process::Command::new(ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(output)
                .args(["-f", "s16le", "-ac", "1", "-ar", "44100", "pipe:1"])
                .output()
                .unwrap();
            assert!(decoded.status.success());
            let samples: Vec<i16> = decoded.stdout.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]])).collect();
            let peak = |time: f32| {
                samples[(time * 44100.) as usize..((time + 0.05) * 44100.) as usize]
                    .iter()
                    .map(|sample| (*sample as i32).abs())
                    .max()
                    .unwrap()
            };
            assert!(peak(0.3) < 100, "silent chart music must stay silent");
            assert!(peak(1.05) > 2000, "custom click must be mixed into the MP4");
            assert!(peak(2.1) > 2000, "custom ending music must be mixed into the MP4");
            println!(
                "Custom texture, resource audio, independent indicators under autoplay, AP/FC, double highlighting, particles and encoded MP4 passed"
            );
        },
    );
}
