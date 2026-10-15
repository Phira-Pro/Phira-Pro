use super::*;
use crate::page::Page;
pub(crate) fn run(painter: &mut prpr::ui::TextPainter, state: &mut SharedState) {
    let title = catalog().into_iter().find(|(tab, _)| *tab == 2).unwrap().1;
    let mut page = SettingsPage::new(
        prpr::ext::BLACK_TEXTURE.clone(),
        prpr::ext::BLACK_TEXTURE.clone(),
        prpr::ext::BLACK_TEXTURE.clone(),
        prpr::ext::BLACK_TEXTURE.clone(),
    );
    prpr::scene::return_input("settings_search".into(), title.clone());
    page.update(state).unwrap();
    assert!(page.search.active());
    assert!(page.search.results.iter().any(|(_, s)| s == &title));
    for (w, h, label) in [(1280, 548, "phone"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::from_hex_rgb(0x192632));
        page.render(&mut ui, state).unwrap();
        crate::practice_tools::qa::capture(&mut ui, &format!("pro15-settings-search-{label}"));
    }
    let i = page.search.results.iter().position(|(tab, s)| *tab == 2 && s == &title).unwrap();
    let row = &page.search.rows[i];
    let mut sum = Vec2::ZERO;
    let mut n = 0.;
    for y in -72..=72 {
        for x in -96..=96 {
            let p = vec2(x as f32 / 100., y as f32 / 100.);
            if row.contains(p) {
                sum += p;
                n += 1.;
            }
        }
    }
    assert!(n > 0.);
    let p = sum / n;
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        page.touch(
            &Touch {
                id: 902,
                phase,
                position: p,
                time: 0.,
            },
            state,
        )
        .unwrap();
    }
    assert_eq!(*page.tabs.selected(), SettingListType::Chart);
    assert!(!page.search.active());
    state.t += 1.;
    state.rt += 1.;
    page.update(state).unwrap();
    let mut ui = Ui::new(painter, Some((0, 0, 960, 720)));
    page.render(&mut ui, state).unwrap();
    assert!(page.search.target.is_none(), "search navigation must find and scroll to the live control");
    crate::practice_tools::qa::capture(&mut ui, "pro15-settings-search-focus");
    println!("pro.15 settings: translated cross-category search and navigation to production controls passed");
}
