//! Capture result badges at every font display size, including a custom OTF.
use macroquad::prelude::*;
use prpr::{config::{Config, Mods}, core::{BOLD_FONT, PGR_FONT}, ext::SafeTexture, info::ChartInfo,
    judge::PlayResult, scene::{EndingScene, Scene}, time::TimeManager, ui::{FontArc, TextPainter, Ui}};

fn conf() -> Conf {
    Conf { window_title: "Phira Pro result layout probe".into(), window_width: 1600, window_height: 1000, headless: true, ..Default::default() }
}

#[macroquad::main(conf)]
async fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    set_pc_assets_folder("assets");
    let reference = FontArc::try_from_slice(include_bytes!("../../assets/harmonyos.ttf")).unwrap();
    let mut fonts = vec![("default", reference.clone())];
    if let Some(path) = std::env::var_os("PHIRA_TEST_FONT") {
        fonts.push(("custom", prpr::ui::parse_font(std::fs::read(path).unwrap()).unwrap()));
    }
    PGR_FONT.with(|f| *f.borrow_mut() = Some(TextPainter::new(FontArc::try_from_slice(include_bytes!("../../assets/phigros.ttf")).unwrap(), Some(reference.clone()))));
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
    let folder = "target/scarlet-audit/ending";
    std::fs::create_dir_all(folder).unwrap();
    for (name, font) in fonts {
        let mut painter = TextPainter::new(font.clone(), Some(reference.clone()));
        painter.normalize_to(&reference);
        let mut bold = TextPainter::new(font, Some(reference.clone()));
        bold.normalize_to(&reference);
        BOLD_FONT.with(|f| *f.borrow_mut() = Some(bold));
        for scale in [0.8f32, 1., 1.2] {
            prpr::ui::FONT_DISPLAY_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
            let mut config = Config::default();
            config.mods = Mods::STRICT_JUDGE | Mods::FLIP_X | Mods::FADE_OUT | Mods::FADE_IN | Mods::NO_SHADER;
            config.speed = 1.25;
            config.volume_music = 0.;
            config.volume_sfx = 0.;
            let result = PlayResult { score: 959000, accuracy: 0.95, max_combo: 2000, num_of_notes: 2000, counts: [1800, 200, 0, 0, 0], ..Default::default() };
            let mut scene = EndingScene::new(background.clone(), background.clone(), background.clone(), icons.clone(), retry.clone(), proceed.clone(),
                mods.clone(), ChartInfo { name: "Badge layout / 状态栏".into(), ..Default::default() }, result, &config,
                sasa::AudioClip::new(std::fs::read("assets/ending.ogg").unwrap()).unwrap(), None, None, 0, None, None, Some(165.)).unwrap();
            let mut tm = TimeManager::default();
            scene.enter(&mut tm, None).unwrap();
            tm.seek_to(3.);
            let mut ui = Ui::new(&mut painter, None);
            scene.render(&mut tm, &mut ui).unwrap();
            unsafe { get_internal_gl().flush(); miniquad::gl::glFinish(); }
            get_screen_data().export_png(&format!("{folder}/{name}-{scale:.1}.png"));
            next_frame().await;
        }
    }
    prpr::ui::cleanup_audio();
}
