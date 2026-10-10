//! Capture result badges at every font display size, including a custom OTF.
use macroquad::prelude::*;
use prpr::{
    config::{Config, Mods},
    core::{BOLD_FONT, PGR_FONT},
    ext::SafeTexture,
    info::ChartInfo,
    judge::PlayResult,
    scene::{EndingScene, Scene},
    time::TimeManager,
    ui::{FontArc, TextPainter, Ui},
};

fn conf() -> Conf {
    Conf {
        window_title: "Phira Pro result layout probe".into(),
        window_width: std::env::var("PHIRA_QA_WIDTH").ok().and_then(|s| s.parse().ok()).unwrap_or(1280),
        window_height: std::env::var("PHIRA_QA_HEIGHT").ok().and_then(|s| s.parse().ok()).unwrap_or(720),
        headless: true,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    set_pc_assets_folder("assets");
    let reference = FontArc::try_from_slice(include_bytes!("../../assets/harmonyos.ttf")).unwrap();
    let bold_reference = FontArc::try_from_slice(include_bytes!("../../assets/bold.ttf")).unwrap();
    let mut fonts = vec![("default", bold_reference.clone()), ("fallback", reference.clone())];
    if let Some(path) = std::env::var_os("PHIRA_TEST_FONT") {
        fonts.push(("custom", prpr::ui::parse_font(std::fs::read(path).unwrap()).unwrap()));
    }
    PGR_FONT.with(|f| {
        *f.borrow_mut() =
            Some(TextPainter::new(FontArc::try_from_slice(include_bytes!("../../assets/phigros.ttf")).unwrap(), Some(reference.clone())))
    });
    let background: SafeTexture = Texture2D::from_rgba8(1, 1, &[40, 40, 48, 255]).into();
    let mut icons = Vec::new();
    for rank in ["F", "C", "B", "A", "S", "V", "FC", "phi"] {
        icons.push(SafeTexture::from(load_texture(&format!("rank/{rank}.png")).await.unwrap()));
    }
    let icons: [SafeTexture; 8] = icons.try_into().ok().unwrap();
    let retry: SafeTexture = load_texture("retry.png").await.unwrap().into();
    let proceed: SafeTexture = load_texture("proceed.png").await.unwrap().into();
    let mut mods = Vec::new();
    for name in ["flip_x", "fade_out", "fade_in", "nightcore", "rainbow", "autoplay", "no-shader"] {
        mods.push(SafeTexture::from(load_texture(&format!("mod/{name}.png")).await.unwrap()));
    }
    let mods: [SafeTexture; 7] = mods.try_into().ok().unwrap();
    let folder = format!("target/result-status-qa/{}x{}", screen_width() as u32, screen_height() as u32);
    std::fs::create_dir_all(&folder).unwrap();
    let mut captures = 0;
    for (name, font) in fonts {
        let mut painter = TextPainter::new(font.clone(), Some(reference.clone()));
        painter.normalize_to(&reference);
        let mut bold = TextPainter::new(font, Some(reference.clone()));
        bold.normalize_to(&bold_reference);
        BOLD_FONT.with(|f| *f.borrow_mut() = Some(bold));
        for scale in [0.8f32, 1., 1.2] {
            for case in ["default", "od15", "strict", "many-mods"] {
                prpr::ui::FONT_DISPLAY_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
                let mut config = Config::default();
                match case {
                    "od15" => config.apply_osu_mania_od(15),
                    "strict" => {
                        config.apply_osu_mania_od(15);
                        config.mods = Mods::STRICT_JUDGE;
                    }
                    "many-mods" => {
                        config.mods = Mods::all();
                        config.speed = 1.25;
                        config.judge_algorithm = prpr::config::JudgeAlgorithm::Phigros;
                        config.judge_grading.detailed = true;
                        config.drag_protect = true;
                        config.flick_protect = true;
                        config.hold_tail_judge = true;
                    }
                    _ => {}
                }
                config.volume_music = 0.;
                config.volume_sfx = 0.;
                let result = PlayResult {
                    grading: config.judge_grading,
                    score: 959000,
                    accuracy: 0.95,
                    max_combo: 2000,
                    num_of_notes: 2000,
                    counts: [1800, 200, 0, 0, 0, 0, 0, 0],
                    ..Default::default()
                };
                let mut scene = EndingScene::new(
                    background.clone(),
                    background.clone(),
                    background.clone(),
                    icons.clone(),
                    retry.clone(),
                    proceed.clone(),
                    mods.clone(),
                    ChartInfo {
                        name: "Badge layout / 状态栏".into(),
                        ..Default::default()
                    },
                    result,
                    &config,
                    sasa::AudioClip::new(std::fs::read("assets/ending.ogg").unwrap()).unwrap(),
                    None,
                    None,
                    0,
                    None,
                    None,
                    Some(165.),
                )
                .unwrap();
                let mut tm = TimeManager::default();
                scene.enter(&mut tm, None).unwrap();
                tm.seek_to(3.);
                let mut ui = Ui::new(&mut painter, None);
                scene.render(&mut tm, &mut ui).unwrap();
                unsafe {
                    get_internal_gl().flush();
                    miniquad::gl::glFinish();
                }
                get_screen_data().export_png(&format!("{folder}/{name}-{scale:.1}-{case}.png"));
                captures += 1;
                // Use the actual button routing, then render the scrollable details.
                let ty = 0.46 - screen_height() / screen_width();
                let base_x = -0.55 + (1.2 - ty) / 1.9 * 0.4;
                for phase in [TouchPhase::Started, TouchPhase::Ended] {
                    scene
                        .touch(
                            &mut tm,
                            &Touch {
                                id: 991,
                                phase,
                                time: 0.,
                                position: vec2(base_x + 0.1, ty),
                            },
                        )
                        .unwrap();
                }
                prpr::scene::DIALOG.with(|slot| {
                    let mut dialog = slot.borrow_mut().take().expect("status badge did not open details");
                    dialog.render(&mut ui, 3.);
                });
                unsafe {
                    get_internal_gl().flush();
                    miniquad::gl::glFinish();
                }
                get_screen_data().export_png(&format!("{folder}/{name}-{scale:.1}-{case}-details.png"));
                captures += 1;
                next_frame().await;
            }
        }
    }
    std::fs::write(format!("{folder}/passed.txt"), format!("{captures} captures; badge bounds and details button passed")).unwrap();
    prpr::ui::cleanup_audio();
}
