//! Measure render-loop frame intervals separately from shader throughput.
//! FRAME_PROBE_SWAP=0/1 and FRAME_PROBE_FULLSCREEN=1 select the Windows path.
use macroquad::prelude::*;
use std::time::Instant;

fn conf() -> Conf {
    let mut conf = prpr::build_conf();
    conf.window_title = "Phira Pro frame pacing probe".into();
    conf.window_width = 1600;
    conf.window_height = 1000;
    conf.fullscreen = std::env::var_os("FRAME_PROBE_FULLSCREEN").is_some();
    conf.platform.swap_interval = Some(std::env::var("FRAME_PROBE_SWAP").ok().and_then(|v| v.parse().ok()).unwrap_or(1));
    conf
}

#[macroquad::main(conf)]
async fn main() {
    let begin = Instant::now();
    let mut previous = begin;
    let mut intervals = Vec::new();
    #[cfg(target_os = "windows")]
    let mut pacer = prpr::desktop_pacing::Pacer::new();
    while begin.elapsed().as_secs_f64() < 6. {
        #[cfg(target_os = "windows")]
        if std::env::var_os("FRAME_PROBE_PACER").is_some() {
            pacer.wait();
        }
        let now = Instant::now();
        if begin.elapsed().as_secs_f64() > 1. {
            intervals.push(now.duration_since(previous).as_secs_f64());
        }
        previous = now;
        clear_background(BLACK);
        draw_rectangle((get_time() as f32 * 300.) % screen_width(), 50., 150., 150., RED);
        next_frame().await;
    }
    let fps = intervals.len() as f64 / intervals.iter().sum::<f64>();
    intervals.sort_by(f64::total_cmp);
    println!(
        "render-loop FPS={fps:.2}, p50={:.3}ms p95={:.3}ms p99={:.3}ms",
        intervals[intervals.len() / 2] * 1000.,
        intervals[intervals.len() * 95 / 100] * 1000.,
        intervals[intervals.len() * 99 / 100] * 1000.
    );
}
