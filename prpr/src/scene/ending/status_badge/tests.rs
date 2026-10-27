//! Render actual exported textures and exercise the production details button.
use super::*;
use crate::{
    config::{Config, Mods},
    core::{Resource, PGR_FONT},
    ext::SafeTexture,
    info::ChartInfo,
    judge::PlayResult,
    scene::{ending::EndingScene, Scene, DIALOG},
    time::TimeManager,
    ui::{FontArc, TextPainter},
};
use std::sync::atomic::Ordering;

#[test]
#[ignore = "requires desktop OpenGL and audio; run separately with --ignored --test-threads=1"]
fn approved_status_badges_render_and_open_details_at_all_aspects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Result status regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            next_frame().await;
            set_pc_assets_folder("assets");
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|f| *f.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            BOLD_FONT.with(|f| {
                *f.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font)))
            });
            let mod_icons = Resource::load_mod_icons().await.unwrap();
            let background: SafeTexture = Texture2D::from_rgba8(1, 1, &[40, 50, 65, 255]).into();
            let icons = Resource::load_icons().await.unwrap();
            let mut config = Config::default();
            config.volume_music = 0.;
            config.volume_sfx = 0.;
            let mut scene = EndingScene::new(
                background.clone(),
                background.clone(),
                background.clone(),
                icons,
                load_texture("retry.png").await.unwrap().into(),
                load_texture("proceed.png").await.unwrap().into(),
                mod_icons,
                ChartInfo {
                    name: "结算状态条回归".into(),
                    level: "IN 15".into(),
                    ..Default::default()
                },
                PlayResult {
                    score: 952000,
                    accuracy: 0.95,
                    num_of_notes: 2000,
                    max_combo: 1900,
                    counts: [1800, 200, 0, 0, 0, 0, 0, 0],
                    ..Default::default()
                },
                &config,
                sasa::AudioClip::from_raw(vec![sasa::Frame(0., 0.); 8000], 8000),
                None,
                None,
                0,
                None,
                None,
                None,
            )
            .unwrap();
            let mut tm = TimeManager::manual(Box::new(|| 3.));
            scene.enter(&mut tm, None).unwrap();
            tm.seek_to(3.);
            let output = std::path::Path::new("target/ending-status-integration-qa");
            std::fs::create_dir_all(output).unwrap();
            let mut count = 0;
            for (w, h) in [(1280, 720), (960, 720), (1280, 548)] {
                for scale in [0.8f32, 1., 1.2] {
                    crate::ui::FONT_DISPLAY_SCALE.store(scale.to_bits(), Ordering::Relaxed);
                    for case in ["empty", "unrated", "autoplay", "strict", "od", "new-icons", "no-fail-combo", "all-mods"] {
                        let mut c = config.clone();
                        match case {
                            "unrated" => c.speed = 0.9,
                            "autoplay" => {
                                c.mods = Mods::AUTOPLAY;
                                c.speed = 0.9;
                            }
                            "strict" => {
                                c.mods = Mods::STRICT_JUDGE;
                                c.speed = 1.1;
                            }
                            "od" => c.apply_osu_mania_od(-4),
                            "new-icons" => c.mods = Mods::FLIP_X | Mods::PERFECT_SOUND | Mods::INSTANT_DEATH_FC | Mods::FULLSCREEN_JUDGE,
                            "no-fail-combo" => c.mods = Mods::NO_FAIL | Mods::NO_COMBO_SCORE,
                            "all-mods" => {
                                c.mods = Mods::all();
                                c.speed = 1.1;
                                c.apply_osu_mania_od(5);
                            }
                            _ => {}
                        }
                        scene.status = PlayStatus::new(&c, case == "empty", true);
                        scene.autoplay = c.autoplay();
                        let mut ui = Ui::new(&mut painter, Some((0, 0, w, h)));
                        scene.render(&mut tm, &mut ui).unwrap();
                        unsafe { get_internal_gl() }.flush();
                        let mut bytes = vec![0; (w * h * 4) as usize];
                        unsafe {
                            use miniquad::gl::*;
                            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
                            assert_eq!(glGetError(), 0);
                        }
                        if scale == 1. {
                            Image {
                                bytes,
                                width: w as u16,
                                height: h as u16,
                            }
                            .export_png(output.join(format!("{case}-{w}x{h}.png")).to_str().unwrap());
                        }
                        // A blank state clears the previous badge's hit target.
                        let ys: Vec<_> = (-100..=100)
                            .map(|y| y as f32 / 100.)
                            .filter(|y| scene.btn_status.contains(vec2(-0.2, *y)))
                            .collect();
                        if case == "empty" {
                            assert!(ys.is_empty(), "blank status must not retain a white-strip hit target");
                            count += 1;
                            continue;
                        }
                        assert!(!ys.is_empty(), "missing white-strip hit target: {case}/{w}x{h}/{scale}");
                        let point = vec2(-0.2, ys[ys.len() / 2]);
                        for phase in [TouchPhase::Started, TouchPhase::Ended] {
                            scene
                                .touch(
                                    &mut tm,
                                    &Touch {
                                        id: 991,
                                        phase,
                                        time: 0.,
                                        position: point,
                                    },
                                )
                                .unwrap();
                        }
                        DIALOG.with(|slot| assert!(slot.borrow_mut().take().is_some(), "details must open for {case}"));
                        count += 1;
                    }
                }
            }
            std::fs::write(output.join("passed.txt"), format!("{count} real result renders and details-button checks passed")).unwrap();
            crate::ui::FONT_DISPLAY_SCALE.store(1f32.to_bits(), Ordering::Relaxed);
            std::process::exit(0);
        },
    );
}
