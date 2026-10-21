//! In-process real renderer and Scene::touch regression, with isolated test data.
use super::*;
use prpr::ext::poll_future;
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    scene::Scene,
    time::TimeManager,
    ui::{FontArc, TextPainter},
};

async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    loop {
        if let Some(result) = poll_future(future.as_mut()) {
            return result;
        }
        next_frame().await;
    }
}
fn capture(ui: &mut Ui, name: &str) {
    unsafe { get_internal_gl() }.flush();
    let (_, _, w, h) = ui.viewport;
    let mut bytes = vec![0; (w * h * 4) as usize];
    unsafe {
        miniquad::gl::glReadPixels(0, 0, w, h, miniquad::gl::GL_RGBA, miniquad::gl::GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
    }
    Image {
        width: w as _,
        height: h as _,
        bytes,
    }
    .export_png(&format!("target/songpack-qa/{name}.png"));
}
fn tap_page(page: &mut CollectionPage, s: &mut SharedState, p: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        page.touch(
            &Touch {
                id: 971,
                phase,
                position: p,
                time: 0.,
            },
            s,
        )
        .unwrap();
    }
}
fn tap_scene(scene: &mut crate::scene::SongScene, tm: &mut TimeManager, p: Vec2) {
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        scene
            .touch(
                tm,
                &Touch {
                    id: 972,
                    phase,
                    position: p,
                    time: 0.,
                },
            )
            .unwrap();
    }
}
fn render(page: &mut dyn Page, s: &mut SharedState, painter: &mut TextPainter, background: prpr::ext::SafeTexture, w: i32, h: i32, name: &str) {
    let mut ui = Ui::new(painter, Some((0, 0, w, h)));
    set_camera(&ui.camera());
    clear_background(BLACK);
    ui.fill_rect(ui.screen_rect(), (*background, ui.screen_rect()));
    s.fader.sub = true;
    s.fader.reset();
    page.render(&mut ui, s).unwrap();
    s.fader.sub = false;
    let r = ui.back_rect();
    crate::scene::TEX_ICON_BACK.with(|p| ui.fill_rect(r, (**p.borrow().as_ref().unwrap(), r, ScaleType::Fit)));
    s.fader.reset();
    let label = page.label();
    s.fader.for_sub(|f| f.render_title(&mut ui, s.t, &label));
    page.render_top(&mut ui, s).unwrap();
    capture(&mut ui, name);
}

#[test]
#[ignore = "requires desktop OpenGL/audio and target/setup_pack_desktop.py fixtures"]
fn pack_pages_real_selection_toolbar_play_and_three_aspects() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Song packs UI regression".into(),
            window_width: 1280,
            window_height: 960,
            headless: true,
            sample_count: 4,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            std::fs::create_dir_all("target/songpack-qa").unwrap();
            let previous = crate::DATA_PATH.lock().unwrap().replace("target/pro16-packs-qa".into());
            let mut data: crate::data::Data = serde_json::from_slice(&std::fs::read("target/pro16-packs-qa/data/data.json").unwrap()).unwrap();
            data.prefer_reduced_motion = true;
            crate::set_data(data);
            crate::sync_data();
            local(get_data_mut().init()).await.unwrap();
            let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
            prpr::ui::set_multilingual_fallback(FontArc::try_from_vec(std::fs::read("assets/font.ttf").unwrap()).unwrap());
            let mut painter = TextPainter::new(font.clone(), None);
            PGR_FONT.with(|p| {
                *p.borrow_mut() = Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/phigros.ttf").unwrap()).unwrap(), None))
            });
            BOLD_FONT.with(|p| {
                *p.borrow_mut() =
                    Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font.clone())))
            });
            let back = load_texture("back.png").await.unwrap().into();
            crate::scene::TEX_ICON_BACK.with(|p| *p.borrow_mut() = Some(back));
            let icons = Arc::new(local(Icons::new()).await.unwrap());
            let background: prpr::ext::SafeTexture = load_texture("background.jpg").await.unwrap().into();
            let mut state = local(SharedState::new(font)).await.unwrap();
            crate::page::settings::verify_background_blur(&mut painter);
            state.reload_local_charts();
            state.t = 2.;
            state.rt = 2.;
            state.fader.sub(0.);
            let mut page = local(CollectionPage::new(icons.clone())).await.unwrap();
            for _ in 0..45 {
                page.update(&mut state).unwrap();
                next_frame().await;
            }
            for (w, h, name) in [
                (960, 720, "collections-tablet"),
                (1280, 720, "collections-wide"),
                (1280, 548, "collections-phone"),
            ] {
                render(&mut page, &mut state, &mut painter, background.clone(), w, h, name);
            }
            // The plus uses its actual hit region and opens the real dialog.
            // Locate the centre through the same RectButton region used by input.
            let mut found = Vec::new();
            for y in -80..80 {
                for x in -100..100 {
                    let p = vec2(x as f32 / 100., y as f32 / 100.);
                    if page.plus.contains(p) {
                        found.push(p);
                    }
                }
            }
            assert!(!found.is_empty(), "plus must have a visible hit region");
            let plus = found.iter().copied().sum::<Vec2>() / found.len() as f32;
            tap_page(&mut page, &mut state, plus);
            assert!(prpr::scene::DIALOG.with(|d| d.borrow().is_some()));
            prpr::scene::DIALOG.with(|d| d.borrow_mut().take());
            page.action.store(2, Ordering::Relaxed);
            page.update(&mut state).unwrap();
            render(&mut page, &mut state, &mut painter, background.clone(), 1280, 720, "select-before");
            tap_page(&mut page, &mut state, vec2(-0.72, -0.5625 + 0.18 + 0.15));
            tap_page(&mut page, &mut state, vec2(-0.24, -0.5625 + 0.18 + 0.15));
            assert_eq!(page.choose.as_ref().unwrap().multi_select.as_ref().unwrap().len(), 2);
            render(&mut page, &mut state, &mut painter, background.clone(), 1280, 720, "select-two");
            page.pending_charts = page
                .choose
                .as_ref()
                .unwrap()
                .multi_select
                .as_ref()
                .unwrap()
                .iter()
                .map(|c| c.path.clone())
                .collect();
            page.finish("回归创建曲包".into()).unwrap();
            assert_eq!(songpacks::load().unwrap().last().unwrap().charts.len(), 2);
            let pack = songpacks::load().unwrap()[0].clone();
            let mut scene = crate::scene::SongScene::from_pack(&pack, icons, state.icons.clone()).unwrap();
            let mut tm = TimeManager::default();
            scene.enter(&mut tm, None).unwrap();
            tm.seek_to(2.);
            for _ in 0..90 {
                scene.update(&mut tm).unwrap();
                scene.render(&mut tm, &mut Ui::new(&mut painter, Some((0, 0, 1280, 720)))).unwrap();
                next_frame().await;
            }
            for (w, h, name) in [(960, 720, "detail-tablet"), (1280, 720, "detail-wide"), (1280, 548, "detail-phone")] {
                let mut ui = Ui::new(&mut painter, Some((0, 0, w, h)));
                scene.render(&mut tm, &mut ui).unwrap();
                capture(&mut ui, name);
            }
            // Select the second card: no SongScene navigation and no full-screen transition.
            tap_scene(&mut scene, &mut tm, vec2(-0.27, -548. / 1280. + 0.22 + 0.13));
            scene.update(&mut tm).unwrap();
            assert_eq!(scene.pack_selected_path(), Some(pack.charts[1].as_str()));
            for _ in 0..80 {
                scene.update(&mut tm).unwrap();
                scene.render(&mut tm, &mut Ui::new(&mut painter, Some((0, 0, 1280, 720)))).unwrap();
                next_frame().await;
            }
            let mut ui = Ui::new(&mut painter, Some((0, 0, 1280, 720)));
            scene.render(&mut tm, &mut ui).unwrap();
            capture(&mut ui, "detail-selected-second");
            // Existing Mods and Info drawers must still respond from the new layout.
            tap_scene(&mut scene, &mut tm, vec2(0.49, -ui.top + 0.07));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            capture(&mut ui, "detail-mods");
            tap_scene(&mut scene, &mut tm, vec2(-0.85, 0.));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            tap_scene(&mut scene, &mut tm, vec2(0.82, -ui.top + 0.07));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            capture(&mut ui, "detail-info");
            tap_scene(&mut scene, &mut tm, vec2(-0.85, 0.));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            tap_scene(&mut scene, &mut tm, vec2(0.60, -ui.top + 0.07));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            capture(&mut ui, "detail-edit");
            tap_scene(&mut scene, &mut tm, vec2(-0.85, 0.));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            tap_scene(&mut scene, &mut tm, vec2(0.71, -ui.top + 0.07));
            scene.update(&mut tm).unwrap();
            assert!(get_data().collections().any(|col| col.charts.iter().any(|c| c.path == pack.charts[1])));
            tap_scene(&mut scene, &mut tm, vec2(0.93, -ui.top + 0.07));
            scene.render(&mut tm, &mut ui).unwrap();
            capture(&mut ui, "detail-menu");
            tap_scene(&mut scene, &mut tm, vec2(-0.85, 0.));
            for _ in 0..30 {
                scene.update(&mut tm).unwrap();
                next_frame().await;
            }
            scene.render(&mut tm, &mut ui).unwrap();
            tap_scene(&mut scene, &mut tm, vec2(0.82, ui.top - 0.18));
            let mut launched = false;
            for _ in 0..240 {
                scene.update(&mut tm).unwrap();
                scene.render(&mut tm, &mut ui).unwrap();
                if matches!(scene.next_scene(&mut tm), NextScene::Overlay(_)) {
                    launched = true;
                    break;
                }
                next_frame().await;
            }
            assert!(launched, "play must launch the selected chart through the normal loading scene");
            drop(scene);
            prpr::ui::cleanup_audio();
            *crate::DATA_PATH.lock().unwrap() = previous;
            println!("Packs: shared card selection, persistence, 3 aspect renders, toolbar drawers and selected-chart launch passed");
        },
    );
}
