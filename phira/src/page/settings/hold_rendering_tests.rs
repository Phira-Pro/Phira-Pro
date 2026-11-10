use super::*;

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn hold_switch_click_save_reload_and_search() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Hold display setting regression".into(),
            headless: true,
            window_width: 1280,
            window_height: 720,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            prpr::core::init_assets();
            std::fs::create_dir_all("target/judgement-panel-qa").unwrap();
            let isolated = crate::hud::IsolatedTestLayout::new();
            crate::set_data(crate::data::Data::default());
            let font = prpr::ui::FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = prpr::ui::TextPainter::new(font, None);
            for lang in ["zh-CN", "en-US", "zh-TW"] {
                prpr_l10n::set_prefered_locale(Some(lang.parse().unwrap()));
                assert!(search::catalog()
                    .iter()
                    .any(|(tab, title)| *tab == 3 && title == &tl!("item-legacy-hold").to_string()));
                for (w, h) in [(960, 720), (1280, 720), (1280, 548)] {
                    let mut list = DebugList::new();
                    list.section_open = [true, false, false];
                    let mut ui = Ui::new(&mut painter, Some((0, 0, w, h)));
                    for target in [true, false] {
                        get_data_mut().config.legacy_hold_rendering = !target;
                        set_camera(&ui.camera());
                        clear_background(BLACK);
                        ui.scope(|ui| {
                            ui.dx(-0.8);
                            ui.dy(-1.05);
                            list.render(ui, Rect::new(0., 0., 1.6, 5.), 2.);
                        });
                        let position = super::qa::center(&list.legacy_hold_btn);
                        assert!(position.x.abs() < 1. && position.y.abs() < ui.top);
                        for phase in [TouchPhase::Started, TouchPhase::Ended] {
                            list.touch(
                                &Touch {
                                    id: 717,
                                    phase,
                                    position,
                                    time: 2.,
                                },
                                2.,
                            )
                            .unwrap();
                        }
                        assert_eq!(get_data().config.legacy_hold_rendering, target);
                        save_data().unwrap();
                        let path = isolated.saved_path().parent().unwrap().join("data.json");
                        let reloaded: crate::data::Data = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                        assert_eq!(reloaded.config.legacy_hold_rendering, target);
                        list.section_open[0] = false;
                        for phase in [TouchPhase::Started, TouchPhase::Ended] {
                            list.touch(
                                &Touch {
                                    id: 718,
                                    phase,
                                    position,
                                    time: 2.,
                                },
                                2.,
                            )
                            .unwrap();
                        }
                        assert_eq!(get_data().config.legacy_hold_rendering, target, "collapsed section must not toggle a hidden control");
                        list.section_open[0] = true;
                    }
                }
            }
        },
    );
}
