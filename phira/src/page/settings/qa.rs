//! Production settings hit regions and FPS edge placement in the existing GL test.
use super::*;
use prpr::ext::BLACK_TEXTURE;
use prpr::ui::TextPainter;

fn center(button: &DRectButton) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut count = 0;
    for y in -100..=100 {
        for x in -100..=100 {
            let p = vec2(x as f32 / 100., y as f32 / 100.);
            if button.inner.contains(p) {
                sum += p;
                count += 1;
            }
        }
    }
    assert!(count > 0, "settings entry must be visible");
    sum / count as f32
}
fn capture(ui: &mut Ui, name: &str) {
    unsafe { get_internal_gl() }.flush();
    let (_, _, w, h) = ui.viewport;
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
    .export_png(&format!("target/judgement-panel-qa/{name}.png"));
}
fn render_debug(list: &mut DebugList, ui: &mut Ui, t: f32) {
    set_camera(&ui.camera());
    clear_background(Color::new(0.10, 0.13, 0.18, 1.));
    ui.scope(|ui| {
        ui.dx(-0.8);
        ui.dy(-item_row_h() * 5. - 0.15);
        list.render(ui, Rect::new(0., 0., 1.6, 5.), t);
    });
}

pub(super) fn render_regression(painter: &mut TextPainter) {
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        get_data_mut().config.desktop_fps_limit = 0;
        let mut debug = DebugList::new();
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        render_debug(&mut debug, &mut ui, 2.);
        let p = center(debug.desktop_fps_btn.control_for_test());
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            debug
                .touch(
                    &Touch {
                        id: 801,
                        phase,
                        position: p,
                        time: 0.,
                    },
                    2.,
                )
                .unwrap();
        }
        render_debug(&mut debug, &mut ui, 2.);
        render_debug(&mut debug, &mut ui, 2.5);
        let popup = debug.desktop_fps_btn.popup_rect();
        let bounds = ui.content_rect();
        assert!(popup.x >= bounds.x && popup.right() <= bounds.right() && popup.y >= bounds.y && popup.bottom() <= bounds.bottom());
        capture(&mut ui, &format!("windows-fps-dropdown-{ratio}"));
        let p = vec2(popup.center().x, popup.y + 0.15); // Follow display, then 60 FPS.
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            assert!(debug.top_touch(
                &Touch {
                    id: 802,
                    phase,
                    position: p,
                    time: 0.
                },
                2.5
            ));
        }
        assert!(debug.update(2.6).unwrap());
        assert_eq!(get_data().config.desktop_fps_limit, 60);
        save_data().unwrap();
        let reloaded: crate::data::Data =
            serde_json::from_str(&std::fs::read_to_string("target/judgement-panel-qa/data/data.json").unwrap()).unwrap();
        assert_eq!(reloaded.config.desktop_fps_limit, 60);
        get_data_mut().config.desktop_fps_limit = 0; // A settings reset must update the label too.
        debug.update(2.7).unwrap();
        assert_eq!(debug.desktop_fps_btn.selected(), 0);

        let mut general = GeneralList::new(BLACK_TEXTURE.clone());
        let height = ui.scope(|ui| general.render(ui, Rect::new(0., 0., 1.6, 8.), 2.).1);
        set_camera(&ui.camera());
        clear_background(Color::new(0.10, 0.13, 0.18, 1.));
        ui.scope(|ui| {
            ui.dx(-0.8);
            ui.dy(-height + item_row_h() * 2. - 0.10);
            general.render(ui, Rect::new(0., 0., 1.6, 8.), 2.);
        });
        let p = center(&general.plugins_btn);
        capture(&mut ui, &format!("general-plugin-entry-{ratio}"));
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            general
                .touch(
                    &Touch {
                        id: 803,
                        phase,
                        position: p,
                        time: 0.,
                    },
                    2.,
                )
                .unwrap();
        }
        match general.next_page().unwrap() {
            NextPage::Overlay(page) => assert_eq!(page.label(), "插件管理"),
            _ => panic!("general entry must open plugin manager"),
        }

        for (safe, suffix) in [([0.; 4], "normal"), ([0.12, 0.03, 0.06, 0.04], "safe-area")] {
            set_camera(&ui.camera());
            clear_background(Color::new(0.10, 0.13, 0.18, 1.));
            for position in 0..6 {
                let r = prpr::ui::draw_fps_overlay(&mut ui, 120., position, safe).unwrap();
                assert!(r.x >= -1. + safe[0] && r.right() <= 1. - safe[2]);
                assert!(r.y >= -ui.top + safe[1] && r.bottom() <= ui.top - safe[3]);
                assert!(r.h < 0.05, "FPS must be smaller than a normal UI row");
                if position == 0 {
                    assert!(r.y < -ui.top + 0.04);
                }
            }
            capture(&mut ui, &format!("fps-six-positions-{suffix}-{ratio}"));
        }
    }
    println!("Settings: Windows FPS dropdown click/save/reset, general plugin entry and six FPS positions passed at three aspects");
}
