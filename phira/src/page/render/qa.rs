//! Exercise the actual export settings controls with an isolated synthetic library.
use super::*;
use crate::{data::Data, get_data};
use prpr::ui::{FontArc, RectButton, TextPainter, Ui};

async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(value) = prpr::ext::poll_future(future.as_mut()) {
            return value;
        }
        next_frame().await;
    }
}

fn center(button: &RectButton) -> Vec2 {
    let points: Vec<_> = (-100..=100)
        .flat_map(|x| (-100..=100).map(move |y| vec2(x as f32 / 100., y as f32 / 100.)))
        .filter(|point| button.contains(*point))
        .collect();
    assert!(!points.is_empty(), "control must have a visible hit region");
    points.iter().copied().sum::<Vec2>() / points.len() as f32
}

fn tap(page: &mut RenderPage, state: &mut SharedState, point: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        page.touch(
            &Touch {
                id: 630,
                phase,
                position: point,
                time: 0.,
            },
            state,
        )
        .unwrap();
    }
}

fn snapshot(page: &mut RenderPage, state: &mut SharedState, painter: &mut TextPainter, w: i32, h: i32, name: &str) {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    clear_background(Color::new(0.10, 0.13, 0.18, 1.));
    state.fader.reset();
    page.render(&mut ui, state).unwrap();
    page.render_top(&mut ui, state).unwrap();
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        miniquad::gl::glReadPixels(0, 0, w, h, miniquad::gl::GL_RGBA, miniquad::gl::GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
    .export_png(&format!("target/render-settings-qa/{name}.png"));
}

#[test]
#[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
fn export_settings_controls_and_three_aspects() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Export settings regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            std::fs::create_dir_all("target/render-settings-qa").unwrap();
            let root = tempfile::tempdir_in("target/render-settings-qa").unwrap();
            let previous = crate::DATA_PATH.lock().unwrap().replace(root.path().to_string_lossy().into_owned());
            let mut data = Data::default();
            data.config.offline_mode = true;
            data.respacks = vec!["skin-a".into()];
            data.respack_id = 1;
            crate::set_data(data);
            let pack = std::path::PathBuf::from(crate::dir::respacks().unwrap()).join("skin-a");
            std::fs::create_dir(&pack).unwrap();
            let mut info: serde_yaml::Value = serde_yaml::from_str(include_str!("../../../../assets/respack/info.yml")).unwrap();
            info["name"] = "测试资源包名称".into();
            std::fs::write(pack.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
            let original = serde_json::to_value(&get_data().config).unwrap();
            prpr::ui::set_multilingual_fallback(FontArc::try_from_vec(std::fs::read("assets/font.ttf").unwrap()).unwrap());
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            let mut state = local(SharedState::new(font)).await.unwrap();
            state.t = 2.;
            state.rt = 2.;
            let mut page = RenderPage::new();
            page.enter(&mut state).unwrap();
            for (w, h, name) in [(960, 720, "tablet"), (1280, 720, "wide"), (1280, 548, "phone")] {
                page.category = settings::Category::Output;
                page.scroll.y_scroller.reset();
                snapshot(&mut page, &mut state, &mut painter, w, h, &format!("output-{name}"));
                page.category = settings::Category::Visuals;
                page.scroll.y_scroller.reset();
                snapshot(&mut page, &mut state, &mut painter, w, h, &format!("visuals-{name}"));
            }
            page.category = settings::Category::Output;
            snapshot(&mut page, &mut state, &mut painter, 1280, 720, "output-before-selection");
            let point = center(&page.resource_pack_selector.control_for_test().inner);
            tap(&mut page, &mut state, point);
            snapshot(&mut page, &mut state, &mut painter, 1280, 720, "resource-popup");
            state.t += 0.5;
            page.update(&mut state).unwrap();
            snapshot(&mut page, &mut state, &mut painter, 1280, 720, "resource-popup-open");
            let popup = page.resource_pack_selector.popup_rect();
            tap(&mut page, &mut state, vec2(popup.center().x, popup.y + 0.17));
            page.update(&mut state).unwrap();
            assert_eq!(page.prefs.resource_pack, resource_pack::Selection::Builtin);
            state.t += 0.5;
            page.update(&mut state).unwrap();
            page.category = settings::Category::Visuals;
            page.scroll.y_scroller.reset();
            snapshot(&mut page, &mut state, &mut painter, 1280, 720, "visuals-before-toggle");
            for field in [
                settings::Field::OffsetIndicator,
                settings::Field::ApFcIndicator,
                settings::Field::DoubleHint,
            ] {
                let point = center(&page.fields[field as usize].inner);
                tap(&mut page, &mut state, point);
            }
            assert_eq!(page.prefs.visuals.offset_indicator, Some(true));
            assert!(!page.prefs.visuals.ap_fc_indicator && !page.prefs.visuals.double_hint);
            let restored = config::load_prefs();
            assert_eq!(restored.resource_pack, resource_pack::Selection::Builtin);
            assert_eq!(restored.visuals.offset_indicator, Some(true));
            assert!(!restored.visuals.ap_fc_indicator && !restored.visuals.double_hint);
            assert_eq!(get_data().respack_id, 1);
            assert_eq!(serde_json::to_value(&get_data().config).unwrap(), original);
            snapshot(&mut page, &mut state, &mut painter, 1280, 720, "visuals-after-toggle");
            *crate::DATA_PATH.lock().unwrap() = previous;
        },
    );
}
