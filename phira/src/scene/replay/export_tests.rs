use super::{
    fixtures::{chart_fixture, local},
    VideoReplay,
};
use crate::replay::{self, JudgeEvent, Replay, TouchEvent};
use macroquad::prelude::*;
use prpr::ui::{FontArc, TextPainter};
use std::path::PathBuf;

#[test]
#[ignore = "requires desktop OpenGL/audio and FFmpeg; run separately with --ignored --test-threads=1"]
fn chart_video_export_uses_recorded_contacts() {
    use prpr::chart_video::{ChartVideoRenderer, VideoOptions};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            let root = tempfile::tempdir().unwrap();
            chart_fixture(root.path(), "chart");
            let chart = root.path().join("data/charts/chart");
            let tape_path = root.path().join("test.ppr");
            replay::save(
                &Replay {
                    speed: 2.,
                    has_speed: true,
                    aspect_ratio: Some(16. / 9.),
                    judges: vec![JudgeEvent {
                        t: 1.,
                        kind: 3,
                        ..Default::default()
                    }],
                    touches: vec![
                        TouchEvent {
                            t: 0.2,
                            id: 1,
                            x: 0.45,
                            y: 0.2,
                            phase: replay::PHASE_DOWN,
                        },
                        TouchEvent {
                            t: 1.9,
                            id: 1,
                            x: 0.45,
                            y: 0.2,
                            phase: replay::PHASE_UP,
                        },
                    ],
                    ..Default::default()
                },
                &tape_path,
            )
            .unwrap();
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font, None);
            let options = VideoOptions {
                width: 480,
                height: 270,
                fps: 30,
                end: Some(2.),
                sample_count: 1,
                ..Default::default()
            };
            let recorded = local(VideoReplay::load(tape_path, &chart, None)).await.unwrap();
            let out = root.path().join("replay.mp4");
            let mut replay = local(ChartVideoRenderer::new_with_replay(chart.clone(), options.clone(), out, Box::new(recorded)))
                .await
                .unwrap();
            let out = root.path().join("autoplay.mp4");
            let mut autoplay = local(ChartVideoRenderer::new(chart, options, out)).await.unwrap();
            assert_eq!(replay.progress().1, 30);
            assert_eq!(autoplay.progress().1, 60);
            for renderer in [&mut replay, &mut autoplay] {
                while !renderer.finished() {
                    renderer.render_next_frame(&mut painter).unwrap();
                }
            }
            let ffmpeg = std::env::var_os("PHIRA_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
            let frame = |path: PathBuf| {
                let pixels = std::process::Command::new(&ffmpeg)
                    .args(["-v", "error", "-ss", "0.4", "-i"])
                    .arg(path)
                    .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
                    .output()
                    .unwrap();
                assert!(pixels.status.success());
                assert_eq!(pixels.stdout.len(), 480 * 270 * 3);
                pixels.stdout
            };
            let recorded_frame = frame(replay.finish().unwrap());
            let autoplay_frame = frame(autoplay.finish().unwrap());
            // The synthetic contact is well to the right of any chart note.
            let touch_pixels = recorded_frame
                .chunks_exact(3)
                .zip(autoplay_frame.chunks_exact(3))
                .enumerate()
                .filter(|(index, (recorded, auto))| (330..366).contains(&(index % 480)) && recorded[2] > auto[2].saturating_add(25))
                .count();
            let max_delta = recorded_frame
                .chunks_exact(3)
                .zip(autoplay_frame.chunks_exact(3))
                .enumerate()
                .filter(|(index, _)| (330..366).contains(&(index % 480)))
                .map(|(_, (recorded, auto))| recorded[2] as i16 - auto[2] as i16)
                .max();
            assert!(
                touch_pixels > 6,
                "expected the recorded contact in its viewport position, got {touch_pixels} pixels with up to {max_delta:?} blue-channel gain"
            );
            println!("MP4 replay export verified recorded contacts and 2x playback alongside autoplay");
            drop(root);
            std::process::exit(0);
        },
    );
}
