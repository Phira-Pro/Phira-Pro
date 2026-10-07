//! CPU mask benchmark at real chart times and tablet resolutions; no GL/audio.
use prpr::core::{BlockArea, Zone};
#[path = "../src/core/block_mask.rs"]
mod mask;
use std::{hint::black_box, time::Instant};

fn main() {
    let path = std::env::var("BLOCK_BENCH_CHART")
        .unwrap_or_else(|_| "data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13/DesultorySignals.technoplanet.0.json".into());
    let source = std::fs::read_to_string(path).unwrap();
    let times: Vec<f64> = std::env::var("BLOCK_BENCH_TIMES")
        .unwrap_or_else(|_| "65,67,71.8".into())
        .split(',')
        .map(|t| t.trim().parse().expect("chart seconds"))
        .collect();
    let chart = prpr::parse::parse_phigros(&source, Default::default()).unwrap();
    for (width, height) in [(960, 720), (1920, 1440), (2560, 1600)] {
        let aspect = width as f32 / height as f32;
        for &time in &times {
            let zones: Vec<_> = chart.block_areas.iter().filter_map(|b| Zone::from_area(b, time, aspect)).collect();
            let mut masks = mask::Masks::default();
            if std::env::var_os("BLOCK_BENCH_PREPARE").is_some() {
                masks.prepare_geometry(width, height, aspect, &chart.block_areas);
            }
            masks.render_displaced(width, height, aspect, &zones, 1.);
            let started = Instant::now();
            for i in 0..120 {
                masks.render_displaced(width, height, aspect, &zones, 1. + i as f32 / 120.);
                black_box(&masks.rgba);
            }
            println!("{width}x{height} chart={time} zones={} CPU mask ms={:.3}", zones.len(), started.elapsed().as_secs_f64() * 1000. / 120.);
            let started = Instant::now();
            for i in 0..120 {
                let chart_time = time + i as f64 / 120.;
                let zones: Vec<_> = chart.block_areas.iter().filter_map(|b| Zone::from_area(b, chart_time, aspect)).collect();
                masks.render_displaced(width, height, aspect, &zones, 2. + i as f32 / 120.);
                black_box(&masks.rgba);
            }
            println!(
                "{width}x{height} chart={time}..{} CPU geometry+dynamic mask ms={:.3}",
                time + 1.,
                started.elapsed().as_secs_f64() * 1000. / 120.
            );
        }
    }
}
