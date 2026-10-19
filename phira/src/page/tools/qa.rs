use super::*;
use crate::page::Page;
pub(crate) fn run(painter: &mut prpr::ui::TextPainter, state: &mut SharedState) {
    let saved = get_data().config.clone();
    let mut quality = ToolsPage::new(false);
    state.fader = super::super::Fader::new();
    state.t = 3.;
    state.rt = 3.;
    for (w, h, label) in [(1280, 548, "phone"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::from_hex_rgb(0x192632));
        quality.render(&mut ui, state).unwrap();
        crate::practice_tools::qa::capture(&mut ui, &format!("pro15-quality-{label}"));
    }
    let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
    quality.render(&mut ui, state).unwrap();
    let r = ui.content_rect().feather(-0.025);
    let p = vec2(r.x + r.w / 6., r.y + 0.04);
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        quality
            .touch(
                &Touch {
                    id: 901,
                    phase,
                    position: p,
                    time: 0.,
                },
                state,
            )
            .unwrap();
    }
    assert!(!get_data().config.particle && !get_data().config.noise_glow && !get_data().config.noise_refraction);
    assert_eq!(get_data().config.sample_count, 1);
    drop(ui);
    crate::get_data_mut().config = saved;
    let mut page = ToolsPage::new(true);
    prpr::diagnostics::set_running(false);
    page.enter(state).unwrap();
    assert!(!prpr::diagnostics::running());
    prpr::diagnostics::set_running(true);
    let events = [10, 11, 12].map(|id| Touch {
        id,
        phase: TouchPhase::Started,
        position: vec2(id as f32 * 25., 400.),
        time: 0.,
    });
    prpr::diagnostics::frame(&events);
    prpr::diagnostics::frame(&[
        Touch {
            id: 10,
            phase: TouchPhase::Ended,
            ..events[0]
        },
        Touch {
            id: 11,
            phase: TouchPhase::Cancelled,
            ..events[1]
        },
    ]);
    page.update(state).unwrap();
    assert_eq!(page.sample.max_fingers, 3);
    assert_eq!(page.sample.active.len(), 1);
    assert_eq!(page.sample.cancels, 1);
    // The header belongs to MainScene; neither touch phase may be swallowed.
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        assert!(!page
            .touch(
                &Touch {
                    id: 99,
                    phase,
                    position: vec2(-0.94, -0.48),
                    time: 0.
                },
                state
            )
            .unwrap());
    }
    for (w, h, label) in [(1280, 548, "phone"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::from_hex_rgb(0x192632));
        page.render(&mut ui, state).unwrap();
        crate::practice_tools::qa::capture(&mut ui, &format!("pro15-diagnostics-{label}"));
    }
    prpr::diagnostics::set_persistent(false);
    page.exit().unwrap();
    println!("pro.15 tools: quality controls, phone/tablet layout and raw three-finger release/cancel diagnostics passed");
}
