//! 把谱面渲染成 mp4（或 PNG 序列），用于验证 `prpr::chart_video`。
//!
//! 例：
//! ```text
//! CHART=/path/to/chart OUT=/tmp/a.mp4 WIDTH=1280 HEIGHT=720 FPS=30 \
//!   cargo run -p prpr --example render_chart_video
//! PNG_DIR=/tmp/frames FRAMES=10 cargo run -p prpr --example render_chart_video
//! ```

use macroquad::prelude::*;
use prpr::{
    chart_video::{ChartVideoRenderer, VideoOptions},
    core::{BOLD_FONT, PGR_FONT},
    ui::{FontArc, TextPainter},
};
use std::path::PathBuf;

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
        window_title: "chart video".into(),
        window_width: 960,
        window_height: 720,
        headless: true,
        platform: miniquad::conf::Platform {
            swap_interval: Some(0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name).ok().and_then(|it| it.parse().ok()).unwrap_or(default)
}

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|it| it.parse().ok())
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
    next_frame().await;

    let chart = PathBuf::from(std::env::var("CHART").expect("CHART 未设置"));
    let out = PathBuf::from(std::env::var("OUT").unwrap_or_else(|_| "target/render-test.mp4".into()));
    let png_dir = std::env::var("PNG_DIR").ok();
    let per_frame_next = std::env::var_os("NEXT_FRAME").is_some();
    let limit = std::env::var("FRAMES").ok().and_then(|it| it.parse::<u64>().ok());
    let opts = VideoOptions {
        width: env_u32("WIDTH", 1280),
        height: env_u32("HEIGHT", 720),
        fps: env_u32("FPS", 30),
        start: env_f64("START").unwrap_or(0.),
        end: env_f64("END"),
        hitsound: std::env::var_os("NO_HITSOUND").is_none(),
        show_hud: std::env::var_os("HUD").is_some(),
        sample_count: env_u32("SAMPLES", 4),
    };
    let out = if png_dir.is_some() { out } else { out };
    let mut renderer = match finish(ChartVideoRenderer::new(chart, opts, out.clone())).await {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("加载谱面失败：{err:?}");
            std::process::exit(1);
        }
    };
    println!("title={} total={}", renderer.title(), renderer.progress().1);
    if let Some(dir) = &png_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut last_report = 0u64;
    let begun = std::time::Instant::now();
    loop {
        let (frame, total) = renderer.progress();
        if renderer.finished() || limit.is_some_and(|n| frame >= n) {
            break;
        }
        if let Err(err) = renderer.render_next_frame(&mut painter) {
            eprintln!("渲染第 {frame} 帧失败：{err:?}");
            std::process::exit(1);
        }
        if let Some(dir) = &png_dir {
            let path = format!("{dir}/frame-{frame:06}.png");
            image::save_buffer(&path, renderer.frame_bytes(), renderer.options().width, renderer.options().height, image::ColorType::Rgba8).unwrap();
        }
        if frame % 60 == 0 || frame + 1 >= total {
            println!(
                "{}/{} 帧，{:.1}s，{:.2} fps 渲染速度",
                frame + 1,
                total,
                begun.elapsed().as_secs_f64(),
                (frame + 1) as f64 / begun.elapsed().as_secs_f64().max(1e-6)
            );
            last_report = frame;
        }
        if per_frame_next {
            next_frame().await;
        }
    }
    println!("渲染结束：{last_report} -> {}/{}", renderer.progress().0, renderer.progress().1);
    if png_dir.is_none() {
        match renderer.finish() {
            Ok(path) => println!("输出：{}", path.display()),
            Err(err) => {
                eprintln!("编码失败：{err:?}");
                std::process::exit(1);
            }
        }
    }
    next_frame().await;
}
