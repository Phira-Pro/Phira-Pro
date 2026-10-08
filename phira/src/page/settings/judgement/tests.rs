//! Software-level regression using production controls in a hidden GL window.
//! No real user configuration or OS input is accessed.
use super::*;
use crate::{data::Data, DATA_PATH};
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    ui::{FontArc, TextPainter, PREFER_REDUCED_MOTION},
};
use std::sync::atomic::Ordering;

fn center(button: &DRectButton) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut count = 0;
    // Query the actual transformed hit rectangle registered by the renderer.
    for y in -100..=100 {
        for x in -100..=100 {
            let p = vec2(x as f32 / 100., y as f32 / 100.);
            if button.inner.contains(p) {
                sum += p;
                count += 1;
            }
        }
    }
    assert!(count > 0, "control must be visible within the viewport before it can be clicked");
    sum / count as f32
}

fn click(list: &mut JudgementList, p: Vec2) -> Option<bool> {
    let mut touch = Touch {
        id: 99,
        phase: TouchPhase::Started,
        position: p,
        time: f64::INFINITY,
    };
    list.touch(&touch, 2.).unwrap();
    touch.phase = TouchPhase::Ended;
    list.touch(&touch, 2.01).unwrap()
}

fn render(list: &mut JudgementList, painter: &mut TextPainter, w: i32, h: i32, scroll: f32, name: &str) -> f32 {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    clear_background(Color::new(0.10, 0.13, 0.18, 1.));
    let r = ui.content_rect();
    let r = prpr::ext::RectExt::feather(&r, -0.015);
    ui.fill_rect(r, Color::new(0.075, 0.11, 0.15, 0.94));
    let height = ui.scope(|ui| {
        ui.dx(r.x);
        ui.dy(r.y - scroll);
        list.render(ui, r, 2.).1
    });
    list.render_top(&mut ui, 2.5);
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glBindFramebuffer(GL_READ_FRAMEBUFFER, 0);
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    assert!(bytes.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200), "text must render");
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
    .export_png(&format!("target/judgement-panel-qa/{name}.png"));
    height
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn production_panel_save_cancel_reload_and_aspects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime_guard = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Judgement preset UI regression".into(),
            window_width: 1280,
            window_height: 720,
            headless: true,
            sample_count: 4,
            ..Default::default()
        },
        async {
            next_frame().await;
            std::fs::create_dir_all("target/judgement-panel-qa").unwrap();
            *DATA_PATH.lock().unwrap() = Some("target/judgement-panel-qa".into());
            let mut data: Data = serde_json::from_str(r#"{"config":{"limGoodMs":155},"language":"zh-CN"}"#).unwrap();
            assert!(data.judge_presets.is_empty());
            assert_eq!(data.config.judge_algorithm, JudgeAlgorithm::PhiraPro);
            assert_eq!(data.config.lim_good_ms, 155.);
            data.prefer_reduced_motion = true;
            crate::set_data(data);
            crate::sync_data();
            PREFER_REDUCED_MOTION.store(true, Ordering::Relaxed);
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|p| *p.borrow_mut() = Some(TextPainter::new(font.clone(), None)));
            BOLD_FONT.with(|p| {
                *p.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font)))
            });
            let mut list = JudgementList::new();
            assert_eq!(list.selection(), None, "legacy custom configuration must not be overwritten");
            list.selector.set_selected(0);
            list.activate();
            list.update(2.).unwrap();
            assert_eq!(get_data().config.lim_good_ms, 180.);
            for (w, h, name) in [(1280, 720, "select-16x9"), (960, 720, "select-4x3"), (1280, 548, "select-21x9")] {
                render(&mut list, &mut painter, w, h, 0., name);
            }
            render(&mut list, &mut painter, 1280, 720, 0., "before-add");
            let p = center(&list.add);
            click(&mut list, p);
            assert!(list.draft.is_some());
            let before = JudgeSettings::capture(&get_data().config);
            return_input("preset-name".into(), "我的测试预设".into());
            list.update(2.).unwrap();
            return_input("preset-side-2-0".into(), "-170".into());
            list.update(2.).unwrap();
            return_input("preset-side-2-1".into(), "+185".into());
            list.update(2.).unwrap();
            return_input("preset-side-0-1".into(), "+20".into());
            list.update(2.).unwrap();
            assert_eq!(JudgeSettings::capture(&get_data().config), before, "editing must not apply before saving");
            let height = render(&mut list, &mut painter, 1280, 720, 0., "editor-top");
            render(&mut list, &mut painter, 1280, 720, 0.5, "editor-sided");
            let p = center(&list.sides[1]);
            click(&mut list, p);
            return_input("preset-side-0-1".into(), "NaN".into());
            list.update(2.).unwrap();
            assert_eq!(side_value(&list.draft.as_ref().unwrap().settings, 0, true), 20.);
            render(&mut list, &mut painter, 960, 720, 0.5, "editor-sided-4x3");
            render(&mut list, &mut painter, 1280, 548, 0.5, "editor-sided-21x9");
            let visible = 720. / 1280. * 2. - 0.20;
            render(&mut list, &mut painter, 1280, 720, height - visible, "editor-bottom");
            let p = center(&list.save);
            assert_eq!(click(&mut list, p), Some(true));
            assert!(list.draft.is_none());
            assert_eq!(get_data().judge_presets.len(), 1);
            let id = get_data().judge_preset_id.clone().unwrap();
            assert_eq!(get_data().config.judge_windows().early[2], 0.170);
            assert_eq!(get_data().config.judge_windows().late[2], 0.185);
            assert_eq!(get_data().config.judge_windows().early[0], 0.016);
            assert_eq!(get_data().config.judge_windows().late[0], 0.020);
            let serialized = std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap();
            crate::set_data(serde_json::from_str(&serialized).unwrap());
            list.refresh();
            assert_eq!(list.selection(), Some(id.as_str()));
            assert_eq!(get_data().judge_presets[0].name, "我的测试预设");
            render(&mut list, &mut painter, 1280, 720, 0., "saved-custom");
            let p = center(&list.edit);
            click(&mut list, p);
            assert_eq!(list.draft.as_ref().unwrap().id.as_deref(), Some(id.as_str()));
            return_input("preset-name".into(), "取消后的名称".into());
            list.update(2.).unwrap();
            let height = render(&mut list, &mut painter, 1280, 720, 0., "edit-top");
            render(&mut list, &mut painter, 1280, 720, height - visible, "cancel-bottom");
            let p = center(&list.cancel);
            assert_eq!(click(&mut list, p), Some(false));
            assert_eq!(get_data().judge_presets[0].name, "我的测试预设");
            assert_eq!(std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap(), serialized);
            return_input("preset-side-2-1".into(), "+200".into());
            list.update(2.).unwrap();
            assert_eq!(get_data().config.judge_windows().late[2], 0.185, "cancelled asynchronous input must not become a live edit");
            render(&mut list, &mut painter, 1280, 720, 0., "edit-custom");
            let p = center(&list.edit);
            click(&mut list, p);
            return_input("preset-name".into(), "已改名".into());
            list.update(2.).unwrap();
            render(&mut list, &mut painter, 1280, 720, height - visible, "rename-bottom");
            let p = center(&list.save);
            // Failed persistence must preserve the active values, saved preset and file.
            std::fs::write("target/judgement-panel-qa/blocked-root", b"test fixture").unwrap();
            *DATA_PATH.lock().unwrap() = Some("target/judgement-panel-qa/blocked-root".into());
            assert_eq!(click(&mut list, p), Some(false));
            assert!(list.draft.is_some());
            assert_eq!(get_data().judge_presets[0].name, "我的测试预设");
            assert_eq!(std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap(), serialized);
            *DATA_PATH.lock().unwrap() = Some("target/judgement-panel-qa".into());
            click(&mut list, p);
            assert_eq!(get_data().judge_presets.len(), 1);
            assert_eq!(get_data().judge_presets[0].id, id);
            assert_eq!(get_data().judge_presets[0].name, "已改名");
            render(&mut list, &mut painter, 1280, 720, 0., "delete-custom");
            let p = center(&list.delete);
            assert_eq!(click(&mut list, p), Some(true));
            assert!(get_data().judge_presets.is_empty());
            assert_eq!(get_data().config.judge_windows().early[2], 0.170, "deleting a saved preset preserves current judgement values");
            save_data().unwrap();
            let reloaded: Data = serde_json::from_str(&std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap()).unwrap();
            assert!(reloaded.judge_presets.is_empty());
            prpr_l10n::set_prefered_locale(Some("en-US".parse().unwrap()));
            list.refresh();
            render(&mut list, &mut painter, 1280, 720, 0., "select-en");
            let p = center(&list.add);
            click(&mut list, p);
            render(&mut list, &mut painter, 1280, 720, 1.2, "editor-en-widths");
            render(&mut list, &mut painter, 1280, 720, 3.0, "editor-en-flick");
            // Live controls work without adding any saved preset, including signed windows.
            list = JudgementList::new();
            list.selector.set_selected(2);
            list.activate();
            list.update(2.).unwrap();
            assert!(get_data().config.judge_grading.detailed);
            assert!(get_data().judge_presets.is_empty());
            assert_eq!(list.selection(), Some(DETAILED));
            for (w, h, name) in [
                (1280, 720, "live-detailed-16x9"),
                (960, 720, "live-detailed-4x3"),
                (1280, 548, "live-detailed-21x9"),
            ] {
                render(&mut list, &mut painter, w, h, 0.65, name);
            }
            render(&mut list, &mut painter, 1280, 720, 0.65, "live-edit-window");
            let p = center(&list.sides[1]);
            click(&mut list, p);
            return_input("preset-side-0-1".into(), "+20".into());
            list.update(2.).unwrap();
            assert!(list.draft.is_none() && get_data().judge_presets.is_empty());
            assert_eq!(get_data().config.judge_windows().late[0], 0.020);
            assert_eq!(list.selection(), None);
            render(&mut list, &mut painter, 1280, 720, 0., "live-perfect-plus-switch");
            get_data_mut().config.theoretical_score = true;
            let p = center(&list.switches[8]);
            click(&mut list, p);
            assert!(!get_data().config.judge_grading.perfect_plus);
            assert!(!get_data().config.theoretical_score);
            let reloaded: Data = serde_json::from_str(&std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap()).unwrap();
            assert!(!reloaded.config.judge_grading.perfect_plus && reloaded.config.judge_grading.detailed);
            assert_eq!(reloaded.config.judge_windows().late[0], 0.020);
            render(&mut list, &mut painter, 1280, 720, 0., "live-perfect-plus-disabled");
            let p = center(&list.switches[8]);
            click(&mut list, p);
            assert!(get_data().config.judge_grading.perfect_plus);
            let unchanged = JudgeSettings::capture(&get_data().config);
            let stored = std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap();
            *DATA_PATH.lock().unwrap() = Some("target/judgement-panel-qa/blocked-root".into());
            return_input("preset-side-1-1".into(), "+45".into());
            list.update(2.).unwrap();
            assert_eq!(JudgeSettings::capture(&get_data().config), unchanged, "failed live save must roll back");
            assert_eq!(std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap(), stored);
            *DATA_PATH.lock().unwrap() = Some("target/judgement-panel-qa".into());
            // Open both real dropdowns and verify the popup bounds and an actual OD selection.
            for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
                render(&mut list, &mut painter, w, h, 0., &format!("preset-dropdown-{suffix}"));
                let p = center(list.selector.control_for_test());
                click(&mut list, p);
                render(&mut list, &mut painter, w, h, 0., &format!("preset-popup-{suffix}"));
                let bounds = Ui::new(&mut painter, Some((0, 0, w, h))).content_rect();
                let popup = list.selector.popup_rect();
                assert!(popup.x >= bounds.x && popup.right() <= bounds.right() && popup.y >= bounds.y && popup.bottom() <= bounds.bottom());
                assert!((popup.w - 0.56).abs() < 1e-5);
                let p = vec2(popup.x + popup.w / 2., popup.y + 0.05);
                for phase in [TouchPhase::Started, TouchPhase::Ended] {
                    assert!(list.top_touch(
                        &Touch {
                            id: 88,
                            phase,
                            position: p,
                            time: 0.
                        },
                        2.
                    ));
                }
                list.update(2.).unwrap();
                render(&mut list, &mut painter, w, h, 0.45, &format!("od-dropdown-{suffix}"));
                let p = center(list.od.control_for_test());
                click(&mut list, p);
                render(&mut list, &mut painter, w, h, 0.45, &format!("od-popup-{suffix}"));
                let popup = list.od.popup_rect();
                assert!(popup.x >= bounds.x && popup.right() <= bounds.right() && popup.y >= bounds.y && popup.bottom() <= bounds.bottom());
                let p = vec2(popup.x + popup.w / 2., popup.y + 0.15); // custom, then OD -15
                for phase in [TouchPhase::Started, TouchPhase::Ended] {
                    assert!(list.top_touch(
                        &Touch {
                            id: 88,
                            phase,
                            position: p,
                            time: 0.
                        },
                        2.
                    ));
                }
                list.update(2.).unwrap();
                assert_eq!(get_data().config.judge_grading.early_ms, [109., 172., 196.]);
                assert_eq!(JudgeSettings::capture(&get_data().config).selected_osu_mania_od(), Some(-15));
                assert!(get_data().judge_presets.is_empty());
            }
            // Use the production result grid and inspect its GPU-rendered pixels at phone/tablet ratios.
            let mut result = prpr::judge::PlayResult::default();
            result.grading.detailed = true;
            result.counts = [123456, 654321, 222222, 111111, 987654, 543210, 135790, 246800];
            result.early_kind = [11111; 8];
            result.late_kind = [22222; 8];
            for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
                for details in [false, true] {
                    let mut ui = Ui::new(&mut painter, Some((0, 0, w, h)));
                    set_camera(&ui.camera());
                    clear_background(Color::new(0.08, 0.12, 0.16, 1.));
                    let y = -ui.top + 0.4 + ui.top * 0.3;
                    let area = prpr::ui::judgement_grid_area(ui.top);
                    prpr::ui::draw_judgement_grid(&mut ui, &result, area, details);
                    ui.text("MAX COMBO")
                        .pos(0.41, y)
                        .anchor(1., 0.)
                        .size(0.64)
                        .color(semi_white(0.6))
                        .draw_using(&BOLD_FONT);
                    ui.fill_rect(Rect::new(0.44, y + 0.004, 0.45, 0.040), Color::new(0., 0., 0., 0.4));
                    ui.text("123456 / 987654")
                        .pos(0.66, y + 0.024)
                        .anchor(0.5, 0.5)
                        .size(0.40)
                        .draw_using(&BOLD_FONT);
                    unsafe { get_internal_gl() }.flush();
                    let mut bytes = vec![0; (w * h * 4) as usize];
                    unsafe {
                        miniquad::gl::glReadPixels(0, 0, w, h, miniquad::gl::GL_RGBA, miniquad::gl::GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
                    }
                    for row in prpr::ui::judgement_grid_layout(&mut ui, &result, area, details) {
                        let cell = row.cell;
                        let label = row.name;
                        assert!(row.label.x >= prpr::ui::judgement_panel_left(row.label.y) + 0.005, "label must stay in score panel");
                        assert!(row.cell.right() <= 0.43 + 1e-5, "counts must stay left of RETRY");
                        let count_right = if details && row.id != 3 {
                            let early = ui
                                .text(format!("-{}", result.early_kind[row.id]))
                                .pos(row.count_x, cell.y)
                                .size(row.count_size)
                                .measure_using(&BOLD_FONT);
                            ui.text(format!("+{}", result.late_kind[row.id]))
                                .pos(early.right() + row.number_gap, cell.y)
                                .size(row.count_size)
                                .measure_using(&BOLD_FONT)
                                .right()
                        } else {
                            ui.text(result.counts[row.id].to_string())
                                .pos(row.count_x, cell.y)
                                .size(row.count_size)
                                .measure_using(&BOLD_FONT)
                                .right()
                        };
                        assert!(count_right <= row.cell.right() + 0.004, "complete counter must fit {label}: {count_right} / {}", row.cell.right());
                        let x0 = ((cell.x + 1.) * w as f32 / 2.).ceil() as usize;
                        let x1 = ((cell.x + cell.w + 1.) * w as f32 / 2.).floor().min(w as f32) as usize;
                        let y0 = ((ui.top - cell.y - cell.h) * w as f32 / 2.).ceil() as usize;
                        let y1 = ((ui.top - cell.y) * w as f32 / 2.).floor().min(h as f32) as usize;
                        let mut ink = 0;
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let p = &bytes[(y * w as usize + x) * 4..][..4];
                                if p[0] > 140 && p[1] > 140 && p[2] > 140 {
                                    ink += 1;
                                }
                            }
                        }
                        assert!(ink > 20, "grade {label} must render in its cell at {suffix}, details={details}: {ink}");
                        if details {
                            assert!((row.label_size - row.count_size).abs() < 1e-5, "even stress details use one font size");
                        } else if w == 1280 {
                            assert!((row.label_size - 0.64).abs() < 1e-5, "stress counters must not reduce the grade label font");
                        }
                    }
                    unsafe {
                        assert_eq!(miniquad::gl::glGetError(), 0);
                    }
                    Image {
                        width: w as u16,
                        height: h as u16,
                        bytes,
                    }
                    .export_png(&format!("target/judgement-panel-qa/result-grid-{suffix}-{details}.png"));
                }
            }
            // Render the entire production result scene, including its original combo bar.
            {
                use prpr::{
                    config::Config,
                    ext::SafeTexture,
                    info::ChartInfo,
                    scene::{EndingScene, Scene},
                    time::TimeManager,
                };
                let texture: SafeTexture = Texture2D::from_image(&Image::gen_image_color(32, 32, Color::new(0.20, 0.30, 0.40, 1.))).into();
                for with_offset in [false, true] {
                    let mut result = prpr::judge::PlayResult::default();
                    result.grading.detailed = true;
                    result.counts = [123, 45, 6, 7, 890, 234, 12, 34];
                    result.num_of_notes = result.counts.iter().sum();
                    result.max_combo = 1234;
                    result.score = 952000;
                    result.accuracy = 0.946;
                    result.early_kind = [12; 8];
                    result.late_kind = [21; 8];
                    if with_offset {
                        result.mean = 0.012;
                        result.offsets = vec![0.012; 8];
                    }
                    let mut config = Config::default();
                    config.volume_music = 0.;
                    config.player_name = "Result Layout QA".into();
                    let mut scene = EndingScene::new(
                        texture.clone(),
                        texture.clone(),
                        texture.clone(),
                        std::array::from_fn(|_| texture.clone()),
                        texture.clone(),
                        texture.clone(),
                        std::array::from_fn(|_| texture.clone()),
                        ChartInfo {
                            name: "Detailed Judgement".into(),
                            level: "IN 15".into(),
                            ..Default::default()
                        },
                        result,
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
                    tm.seek_to(3.);
                    for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
                        for details in [false, true] {
                            let mut ui = Ui::new(&mut painter, Some((0, 0, w, h)));
                            scene.render(&mut tm, &mut ui).unwrap();
                            unsafe { get_internal_gl() }.flush();
                            let mut bytes = vec![0; (w * h * 4) as usize];
                            unsafe {
                                miniquad::gl::glBindFramebuffer(miniquad::gl::GL_READ_FRAMEBUFFER, 0);
                                miniquad::gl::glReadPixels(
                                    0,
                                    0,
                                    w,
                                    h,
                                    miniquad::gl::GL_RGBA,
                                    miniquad::gl::GL_UNSIGNED_BYTE,
                                    bytes.as_mut_ptr() as _,
                                );
                                assert_eq!(miniquad::gl::glGetError(), 0);
                            }
                            // The illustration sector must not cover the prefix of any left-aligned label.
                            if w == 1280 {
                                let area = prpr::ui::judgement_grid_area(ui.top);
                                let mut probe = prpr::judge::PlayResult::default();
                                probe.grading.detailed = true;
                                probe.counts = [123, 45, 6, 7, 890, 234, 12, 34];
                                probe.early_kind = [12; 8];
                                probe.late_kind = [21; 8];
                                let rows = prpr::ui::judgement_grid_layout(&mut ui, &probe, area, details);
                                assert!((rows[4].cell.x - rows[0].cell.right() - 0.030).abs() < 1e-5, "column gap must match compact layout");
                                for row in rows {
                                    let label = row.name;
                                    let rect = row.label;
                                    assert!(rect.x >= prpr::ui::judgement_panel_left(rect.y) + 0.005, "{label} must stay in score panel at {suffix}");
                                    assert!(row.cell.right() <= 0.43 + 1e-5);
                                    if !details { assert!((row.label_size - 0.64).abs() < 1e-5, "normal phone labels retain original size"); }
                                    else { assert!((row.label_size - row.count_size).abs() < 1e-5, "detail numbers share the label font size"); }
                                    let x0 = ((rect.x + 1.) * w as f32 / 2.).ceil() as usize;
                                    let x1 = ((rect.x + rect.w / 4. + 1.) * w as f32 / 2.).floor() as usize;
                                    let y0 = ((ui.top - rect.bottom()) * w as f32 / 2.).ceil() as usize;
                                    let y1 = ((ui.top - rect.y) * w as f32 / 2.).floor() as usize;
                                    let mut ink = 0;
                                    for y in y0..y1 {
                                        for x in x0..x1 {
                                            let px = &bytes[(y * w as usize + x) * 4..][..3];
                                            if px.iter().all(|v| *v > 130) {
                                                ink += 1;
                                            }
                                        }
                                    }
                                    assert!(ink > 8, "full scene must show {label} prefix at {suffix}, details={details}: {ink}");
                                }
                            }
                            let extra = if with_offset { "-offset" } else { "" };
                            let mode = if details { "-details" } else { "" };
                            Image {
                                width: w as u16,
                                height: h as u16,
                                bytes,
                            }
                            .export_png(&format!("target/judgement-panel-qa/ending-original-style-{suffix}{extra}{mode}.png"));
                            // Exercise the real DETAILS entry, including toggling back before the next ratio.
                            let p = vec2(0.91, -ui.top + 0.49);
                            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                                let handled = scene
                                    .touch(
                                        &mut tm,
                                        &Touch {
                                            id: 110,
                                            phase,
                                            position: p,
                                            time: 3.,
                                        },
                                    )
                                    .unwrap();
                                if phase == TouchPhase::Ended {
                                    assert!(handled, "real DETAILS entry must toggle");
                                }
                            }
                        }
                    }
                }
            }
            // The sole chart-settings entry produces an independent page.
            let mut chart = super::super::ChartList::new();
            let mut ui = Ui::new(&mut painter, Some((0, 0, 1280, 720)));
            ui.scope(|ui| {
                ui.dx(-0.9);
                ui.dy(-2.2);
                chart.render(ui, Rect::new(0., 0., 1.8, 10.), 2.);
            });
            let p = center(&chart.judgement_btn);
            let above = center(&chart.offset_indicator_btn);
            assert!((p.y - above.y - item_row_h()).abs() < 0.015, "entry spacing must match one regular settings row");
            let mut touch = Touch {
                id: 101,
                phase: TouchPhase::Started,
                position: p,
                time: f64::INFINITY,
            };
            chart.touch(&touch, 2.).unwrap();
            touch.phase = TouchPhase::Ended;
            chart.touch(&touch, 2.01).unwrap();
            match chart.next_page().unwrap() {
                super::super::NextPage::Overlay(page) => assert_eq!(page.label().as_ref(), tl!("judgement-settings").as_ref()),
                _ => panic!("entry must open standalone page"),
            }
            println!("Production panel: add / save / reload / edit / cancel / rename / delete passed; rendered 16:9, 4:3, 21:9.");
            drop(ui);
            crate::page::home::music_panel::render_regression(&mut painter).await;
            crate::menu_music::playback_regression().await;
            prpr::ui::cleanup_audio();
        },
    );
}
