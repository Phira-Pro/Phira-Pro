//! Infer Compose's animation phase from several video masks, without altering
//! production clocks or tuning material colors. This is an estimate, not an
//! observation of the recorder's Unity _Time or a pixel-exact validation.
use macroquad::prelude::*;
use prpr::core::{BlockArea, Zone};

#[path = "../src/core/block_mask.rs"]
mod mask;

fn conf() -> Conf {
    Conf {
        headless: true,
        window_width: 960,
        window_height: 720,
        ..Default::default()
    }
}

struct Frame {
    time: f64,
    zones: Vec<Zone>,
    observed: Vec<(usize, bool)>,
}

fn score(masks: &mut mask::Masks, frames: &[Frame], offset: f32) -> usize {
    let mut wrong = 0;
    for frame in frames {
        masks.render_displaced(1920, 1440, 4. / 3., &frame.zones, frame.time as f32 + offset);
        wrong += frame.observed.iter().filter(|&&(i, on)| (masks.rgba[i * 4] > 127) != on).count();
    }
    wrong
}

#[macroquad::main(conf)]
async fn main() {
    next_frame().await;
    let mut reports = Vec::new();
    for (name, path, times) in [
        ("desultory", "data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13/DesultorySignals.technoplanet.0.json", [67., 70., 71.8]),
        ("hate", "data/charts/custom/19d0e386-5929-4031-85fb-28ac8e6a472f/ハテ.rNFrums.0.json", [137., 139.8, 141.8]),
    ] {
        let chart = prpr::parse::parse_phigros(&std::fs::read_to_string(path).unwrap(), Default::default()).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(format!("target/block-area-reference/{name}/metadata.json")).unwrap()).unwrap();
        let frames: Vec<_> = times
            .into_iter()
            .map(|requested| {
                let row = metadata["extraction"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| (r["requested_video_seconds"].as_f64().unwrap() - requested).abs() < 0.00002)
                    .unwrap();
                let time = row["decoded_frame_pts_seconds"].as_f64().unwrap();
                let image = image::open(row["file"].as_str().unwrap()).unwrap().to_rgb8();
                assert_eq!(image.dimensions(), (960, 720));
                let mut observed = Vec::new();
                // Remove HUD and corners. Average each 4x4 video cell so codec
                // ringing does not become a claimed exact mask boundary.
                for y in 24..162 {
                    for x in 6..234 {
                        let mut rgb = [0_i32; 3];
                        for dy in 0..4 {
                            for dx in 0..4 {
                                let p = image.get_pixel(x * 4 + dx, y * 4 + dy).0;
                                for c in 0..3 {
                                    rgb[c] += p[c] as i32;
                                }
                            }
                        }
                        let on = rgb[0] - rgb[1] > 5 * 16 && rgb[0] - rgb[2] > 8 * 16;
                        // Production packs each 1/8 mask texel into a 2x2 cell
                        // of its 480x360 effect texture; PNG rows run top-down.
                        observed.push((((179 - y) * 2 * 480 + x * 2) as usize, on));
                    }
                }
                Frame {
                    time,
                    zones: chart.block_areas.iter().filter_map(|b| Zone::from_area(b, time, 4. / 3.)).collect(),
                    observed,
                }
            })
            .collect();
        let period = 40. / (2.59 * std::f32::consts::FRAC_1_SQRT_2);
        let mut masks = mask::Masks::default();
        let mut scores = Vec::new();
        for step in 0..=(period / 0.05) as usize {
            let offset = step as f32 * 0.05;
            scores.push((score(&mut masks, &frames, offset), offset));
        }
        scores.sort_by(|a, b| a.0.cmp(&b.0));
        let (_, coarse) = scores[0];
        let mut fine = Vec::new();
        for step in -50..=50 {
            let offset = (coarse + step as f32 * 0.001).rem_euclid(period);
            fine.push((score(&mut masks, &frames, offset), offset));
        }
        fine.sort_by(|a, b| a.0.cmp(&b.0));
        let (cost, offset) = fine[0];
        let baseline = score(&mut masks, &frames, 0.);
        for frame in &frames {
            masks.render_displaced(1920, 1440, 4. / 3., &frame.zones, frame.time as f32 + offset);
            std::fs::write(format!("target/block-area-phase-{name}-{:.5}.sources", frame.time), &masks.sources_rgba).unwrap();
            let mut comparison = image::RgbImage::from_pixel(240, 180, image::Rgb([96, 96, 96]));
            for &(i, on) in &frame.observed {
                let pred = masks.rgba[i * 4] > 127;
                let color = match (pred, on) {
                    (true, true) => [0, 180, 0],
                    (true, false) => [255, 0, 0],
                    (false, true) => [0, 80, 255],
                    (false, false) => [0, 0, 0],
                };
                comparison.put_pixel((i % 480 / 2) as u32, (179 - i / 480 / 2) as u32, image::Rgb(color));
            }
            comparison.save(format!("target/block-area-phase-{name}-{:.5}.png", frame.time)).unwrap();
        }
        let per_frame: Vec<_> = frames.iter().map(|frame| {
            let own: Vec<_> = scores.iter().map(|&(_, off)| (score(&mut masks, std::slice::from_ref(frame), off), off)).collect();
            let best = own.into_iter().min_by_key(|v| v.0).unwrap();
            serde_json::json!({"chart_seconds":frame.time,"joint_offset_cost":score(&mut masks,std::slice::from_ref(frame),offset),"individual_best_offset":best.1,"individual_best_cost":best.0})
        }).collect();
        let report = serde_json::json!({"chart":name,"compose_period_seconds":period,"estimated_clock_offset_modulo_period_seconds":offset,"joint_mask_mismatch_cells":cost,"zero_offset_mismatch_cells":baseline,"sampled_cells":frames.iter().map(|f|f.observed.len()).sum::<usize>(),"video_mask_threshold":"averaged R-G>5 and R-B>8; no HUD/corners","not_pixel_exact":true,"limitations":"Heuristic RGB segmentation includes glow and cannot identify all fill pixels independently of scene color. No production clock was changed.","difference_image_legend":"green=both on, red=prediction only, blue=video threshold only, black=both off, gray=excluded","per_frame":per_frame});
        println!("{report}");
        reports.push(report);
    }
    std::fs::write("target/block-area-phase-audit.json", serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}
