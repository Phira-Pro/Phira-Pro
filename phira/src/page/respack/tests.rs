use super::*;
use prpr::ui::{FontArc, TextPainter};

fn capture(page: &mut ResPackPage, state: &mut SharedState, painter: &mut TextPainter) -> Vec<u8> {
    let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
    set_camera(&ui.camera());
    clear_background(BLACK);
    state.fader.reset();
    crate::hud::begin_frame();
    page.render(&mut ui, state).unwrap();
    unsafe { get_internal_gl() }.flush();
    let mut pixels = vec![0; 1280 * 720 * 4];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, pixels.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    pixels
}

fn assert_translated(before: &[u8], after: &[u8], area: Rect, delta: Vec2) {
    let mut ink = 0;
    let mut matching = 0;
    for y in ((0.5625 - area.bottom()) * 640.).ceil() as i32..((0.5625 - area.y) * 640.).floor() as i32 {
        for x in ((area.x + 1.) * 640.).ceil() as i32..((area.right() + 1.) * 640.).floor() as i32 {
            let a = ((y * 1280 + x) * 4) as usize;
            let b = (((y - (delta.y * 640.).round() as i32) * 1280 + x + (delta.x * 640.).round() as i32) * 4) as usize;
            if before[a..a + 3].iter().any(|&v| v > 100) {
                ink += 1;
                if before[a..a + 3].iter().zip(&after[b..b + 3]).all(|(a, b)| a.abs_diff(*b) < 20) {
                    matching += 1;
                }
            }
        }
    }
    assert!(ink > 20, "preview must contain visible pixels in {area:?}");
    assert!(matching as f32 / ink as f32 > 0.95, "preview pixels must follow the HUD panel: {matching}/{ink} in {area:?}");
}

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn hold_and_live_hit_particles_follow_hud_panel() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Resource preview HUD regression".into(),
            window_width: 1280,
            window_height: 720,
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            prpr::core::init_assets();
            std::fs::create_dir_all("target/judgement-panel-qa").unwrap();
            let _isolated = crate::hud::IsolatedTestLayout::new();
            crate::set_data(crate::data::Data::default());
            crate::hud::set_cur_page(crate::hud::PageId::Respack);
            crate::scene::TEX_ICON_BACK.with(|it| *it.borrow_mut() = Some(Texture2D::from_rgba8(1, 1, &[255; 4]).into()));
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            let mut painter = TextPainter::new(font.clone(), None);
            let mut state = SharedState::new(font).await.unwrap();
            state.t = 1.1;
            let mut page = ResPackPage::new(Arc::new(Icons::new().await.unwrap())).unwrap();
            page.items[0].load_task = None;
            let pack = ResourcePack::from_path(None::<String>).await.unwrap();
            let mut emitter = build_emitter(&pack).unwrap();
            // Keep one visible burst alive while moving the panel; do not
            // regenerate it at the destination, which would hide the bug.
            emitter.hide_particles = true;
            emitter.emitter.config.lifetime = 100.;
            emitter.emitter.config.atlas = None;
            emitter.emitter.config.texture = Some(Texture2D::from_rgba8(1, 1, &[255; 4]));
            page.emitter = Some(emitter);
            page.items[0].loaded = Some(pack);
            capture(&mut page, &mut state, &mut painter);
            let before = capture(&mut page, &mut state, &mut painter);
            let ui = Ui::new(&mut painter, Some((0, 0, 1280, 720)));
            let mut rect = ui.content_rect();
            rect.x += 0.29;
            rect.w -= 0.29;
            drop(ui);
            let delta = vec2(-0.125, -0.0625);
            crate::hud::set(
                "respack",
                "preview",
                crate::hud::Slot {
                    anchor: crate::hud::Anchor::Center,
                    offset: [rect.center().x + delta.x, rect.center().y + delta.y],
                    size: [rect.w, rect.h],
                },
            );
            let after = capture(&mut page, &mut state, &mut painter);
            for x in [0.1, 0.3] {
                assert_translated(&before, &after, Rect::new(x - 0.01, rect.y + 0.04, 0.18, rect.h - 0.34), delta);
            }
            assert_translated(&before, &after, Rect::new(0.65, 0.04, 0.16, 0.065), delta);
        },
    );
}
