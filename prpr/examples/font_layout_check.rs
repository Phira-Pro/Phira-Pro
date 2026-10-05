//! Render font regression labels through the real glyph atlas, then check their
//! visible pixel bounds. Set PHIRA_TEST_FONT to also test a local TTF/OTF.
use macroquad::prelude::*;
use prpr::core::{BOLD_FONT, PGR_FONT};
use prpr::ui::{parse_font, TextPainter, Ui};

fn verify_numeric_fade(painter: &mut TextPainter, metric: &str, detail: &str) {
    for alpha in [0., 0.5, 1., 0.25, 0.] {
        clear_background(BLACK);
        let mut ui = Ui::new(painter, None);
        set_camera(&ui.camera());
        ui.scissor(Rect::new(-0.8, -0.4, 1.6, 0.8), |ui| {
            ui.alpha(alpha, |ui| {
                // Equal labels reuse GlyphBrush's ReDraw path, just as tied scores
                // and identical accuracy details do in the fading leaderboard.
                for y in [-0.3, -0.1, 0.1, 0.3] {
                    ui.text(metric)
                        .pos(0.7, y - 0.035)
                        .anchor(1., 0.5)
                        .no_baseline()
                        .size(0.68)
                        .max_width(0.45)
                        .draw_using(&PGR_FONT);
                    ui.text(detail)
                        .pos(0.7, y + 0.035)
                        .anchor(1., 0.5)
                        .no_baseline()
                        .size(0.32)
                        .max_width(0.45)
                        .draw_using(&BOLD_FONT);
                }
            });
        });
        unsafe { get_internal_gl() }.flush();
        let mut pixels = vec![0u8; 1280 * 720 * 4];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, pixels.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        for y in [-0.3, -0.1, 0.1, 0.3] {
            for offset in [-0.035, 0.035] {
                let center = ((y + offset + 720. / 1280.) * 640.) as usize;
                // glReadPixels returns rows starting at the bottom.
                let max = (center - 12..=center + 12)
                    .flat_map(|row| (800..1090).map(move |x| ((719 - row) * 1280 + x) * 4))
                    .map(|i| pixels[i])
                    .max()
                    .unwrap();
                let expected = alpha * 255.;
                // Small numeric glyphs may have no pixel with 100% coverage.
                assert!((max as f32 - expected).abs() <= 6., "numeric fade at {alpha}, row {y}, offset {offset}: max={max}");
            }
        }
    }
    println!("PASS repeated {metric} / {detail} through fade in and out");
}

fn conf() -> Conf {
    Conf {
        window_title: "Font layout regression".into(),
        window_width: 1280,
        window_height: 720,
        headless: true,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    let output = "target/font-regression";
    std::fs::create_dir_all(output).unwrap();
    let reference = parse_font(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
    PGR_FONT.with(|p| *p.borrow_mut() = Some(TextPainter::new(parse_font(std::fs::read("assets/phigros.ttf").unwrap()).unwrap(), None)));
    BOLD_FONT
        .with(|p| *p.borrow_mut() = Some(TextPainter::new(parse_font(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(reference.clone()))));
    let mut numeric_painter = TextPainter::new(reference.clone(), None);
    for (metric, detail) in [("1000000", "100.00% · FC"), ("0987654", "5.54ms · 100.00%"), ("100.00%", "1000000")] {
        verify_numeric_fade(&mut numeric_painter, metric, detail);
    }
    let mut paths = vec![
        "assets/harmonyos.ttf".to_owned(),
        "assets/phigros.ttf".to_owned(),
        "assets/bold.ttf".to_owned(),
    ];
    if let Ok(path) = std::env::var("PHIRA_TEST_FONT") {
        paths.push(path);
    }
    paths.extend(std::env::args().skip(1));
    next_frame().await;
    for (index, path) in paths.iter().enumerate() {
        let font = parse_font(std::fs::read(path).unwrap()).unwrap();
        let mut painter = TextPainter::new(font, Some(reference.clone()));
        painter.normalize_to(&reference);
        for (si, scale) in [0.8f32, 0.9, 1., 1.1, 1.2].into_iter().enumerate() {
            prpr::ui::FONT_DISPLAY_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
            clear_background(BLACK);
            let mut ui = Ui::new(&mut painter, None);
            set_camera(&ui.camera());
            let mut rows = Vec::new();
            let title = "SETTINGS";
            let title_height = ui.text(title).size(1.2).no_baseline().measure().h;
            let title_center = -0.50;
            // Same per-character, clipped animation layout as the app header.
            ui.scissor(Rect::new(-1., title_center - title_height / 2., 2., title_height), |ui| {
                let mut x = -0.7;
                for c in title.chars() {
                    x += ui
                        .text(c.to_string())
                        .size(1.2)
                        .no_baseline()
                        .pos(x, title_center)
                        .anchor(0., 0.5)
                        .draw()
                        .w
                        + 0.012;
                }
            });
            rows.push((((title_center + 720. / 1280.) * 640.) as i32, title_height * 640.));
            for (row, label) in ["HIM", "gjpq", "中文 ABC", "MODS", "AP 全连 All Perfect 即死模式测试"].iter().enumerate() {
                let y = -0.40 + row as f32 * 0.19;
                let mut text = ui.text(*label).pos(-0.7, y).anchor(0., 0.5).no_baseline().size(0.6).max_width(0.46);
                let measured = text.measure();
                let drawn = text.draw();
                assert_eq!(measured, drawn);
                rows.push((((y + 720. / 1280.) * 640.) as i32, measured.h * 640.));
            }
            unsafe { get_internal_gl() }.flush();
            let mut pixels = vec![0u8; 1280 * 720 * 4];
            unsafe {
                use miniquad::gl::*;
                glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, pixels.as_mut_ptr() as _);
                assert_eq!(glGetError(), 0);
            }
            for y in 0..360 {
                let (a, b) = pixels.split_at_mut((719 - y) * 1280 * 4);
                a[y * 1280 * 4..(y + 1) * 1280 * 4].swap_with_slice(&mut b[..1280 * 4]);
            }
            for (center, height) in rows {
                let mut top = 720;
                let mut bottom = 0;
                for y in (center - 40).max(0)..=(center + 40).min(719) {
                    if (0..1280).any(|x| pixels[((y as usize * 1280 + x) * 4)..][0] > 80) {
                        top = top.min(y);
                        bottom = bottom.max(y);
                    }
                }
                assert!(top <= bottom, "missing label: {path}");
                assert!(((top + bottom) as f32 / 2. - center as f32).abs() <= 1.5, "{path}: ink={top}..{bottom}, center={center}");
                assert!(((bottom - top + 1) as f32 - height).abs() <= 3., "clipped glyphs in {path}: expected height {height}, ink={top}..{bottom}");
            }
            let png = format!("{output}/font-{index}-scale-{si}.png");
            image::save_buffer(&png, &pixels, 1280, 720, image::ColorType::Rgba8).unwrap();
            println!("PASS {path} at {scale}: {png}");
            next_frame().await;
        }
    }
}
