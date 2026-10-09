use crate::{icons::Icons, menu_music::MenuMusic};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt, ScaleType},
    ui::{DRectButton, RectButton, Ui},
};

pub struct MusicPanel {
    background: DRectButton,
    progress: RectButton,
    random: DRectButton,
    play: DRectButton,
    popup: bool,
    popup_rect: Option<Rect>,
    anchor: Rect,
}
impl Default for MusicPanel {
    fn default() -> Self {
        Self {
            background: DRectButton::new().with_radius(0.008).with_elevation(0.002),
            progress: RectButton::new(),
            random: DRectButton::new().with_radius(0.008).with_delta(-0.003),
            play: DRectButton::new().with_radius(0.008).with_delta(-0.003),
            popup: false,
            popup_rect: None,
            anchor: Rect::default(),
        }
    }
}
pub fn panel_rect(title_left: f32, top: f32, width: f32) -> Rect {
    let bottom_margin = if cfg!(feature = "hykb") { 0.10 } else { 0.04 };
    Rect::new(title_left, top - bottom_margin - 0.08, width, 0.08)
}
pub fn music_slot(ui: &Ui, width: f32) -> Rect {
    let default = panel_rect(crate::page::home_title_left(ui), ui.top, width);
    crate::hud::slot(
        ui,
        "home",
        crate::hud::SlotDef::at(
            "music",
            // HUD anchors retain mathematical-Y names; +ui.top is the screen bottom.
            crate::hud::Anchor::TopLeft,
            [default.center().x + 1., default.center().y - ui.top],
            [default.w, default.h],
            crate::hud::Cap(true, false, false),
        ),
    )
}
fn control_rects(panel: Rect) -> (Rect, Rect, Rect) {
    let width = 0.075;
    let play = Rect::new(panel.right() - 0.012 - width, panel.y, width, panel.h);
    let random = Rect::new(play.x - 0.008 - width, panel.y, width, panel.h);
    let progress = Rect::new(panel.x + 0.022, panel.y, (random.x - panel.x - 0.042).max(0.02), panel.h);
    (progress, random, play)
}
// Visible artwork bounds of the built-in 128px assets, excluding transparent padding.
const RETRY_ARTWORK: Rect = Rect {
    x: 20. / 128.,
    y: 20. / 128.,
    w: 88. / 128.,
    h: 88. / 128.,
};
const PLAY_ARTWORK: Rect = Rect {
    x: 31. / 128.,
    y: 19. / 128.,
    w: 74. / 128.,
    h: 90. / 128.,
};

fn icon_rect(button: Rect) -> Rect {
    let size = button.w.min(button.h) * 0.46;
    Rect::new(button.center().x - size / 2., button.center().y - size / 2., size, size)
}
fn texture_icon_rect(button: Rect, texture: Texture2D, artwork: Rect) -> Rect {
    let icon = icon_rect(button);
    let scale = icon.h / (artwork.h * texture.height());
    Rect::new(
        button.center().x - artwork.center().x * texture.width() * scale,
        button.center().y - artwork.center().y * texture.height() * scale,
        texture.width() * scale,
        texture.height() * scale,
    )
}
fn dropdown_rect(anchor: Rect, top: f32) -> Rect {
    let h = 0.18;
    let y = if anchor.bottom() + 0.012 + h <= top - 0.02 {
        anchor.bottom() + 0.012
    } else {
        anchor.y - h - 0.012
    };
    Rect::new(anchor.x.clamp(-0.98, 0.98 - anchor.w.min(0.68)), y.max(-top + 0.02), anchor.w.min(0.68), h)
}
fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.).floor() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
impl MusicPanel {
    pub fn touch(&mut self, touch: &Touch, t: f32, music: &mut MenuMusic) -> Result<bool> {
        if self.popup {
            if self.progress.contains(touch.position) {
                if self.progress.touch(touch) {
                    self.popup = false;
                }
                return Ok(true);
            }
            if self.popup_rect.is_some_and(|r| r.contains(touch.position)) {
                return Ok(true);
            }
            if touch.phase == TouchPhase::Started {
                self.popup = false;
                // Dismiss without activating the menu item underneath the dropdown.
                return Ok(true);
            }
        }
        if self.progress.touch(touch) {
            self.popup = !self.popup;
            return Ok(true);
        }
        if self.random.touch(touch, t) {
            music.random();
            return Ok(true);
        }
        if self.play.touch(touch, t) {
            music.toggle()?;
            return Ok(true);
        }
        Ok(self.progress.contains(touch.position) || self.random.inner.contains(touch.position) || self.play.inner.contains(touch.position))
    }
    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32, music: &MenuMusic, icons: &Icons) {
        self.background.render_shadow(ui, r, t, |ui, path| ui.fill_path(&path, semi_black(0.4)));
        let (progress, random, play) = control_rects(r);
        self.progress.set(ui, progress);
        let anchor = Rect::new(r.x, r.y, random.right() - r.x, r.h);
        let mat = prpr::ext::nalgebra_to_glm(&ui.transform) * ui.gl_transform;
        let corners = [
            (anchor.x, anchor.y),
            (anchor.right(), anchor.y),
            (anchor.x, anchor.bottom()),
            (anchor.right(), anchor.bottom()),
        ]
        .map(|(x, y)| {
            let p = mat * vec4(x, y, 0., 1.);
            p.xy() / p.w
        });
        let min = corners.iter().fold(vec2(f32::INFINITY, f32::INFINITY), |a, b| a.min(*b));
        let max = corners.iter().fold(vec2(f32::NEG_INFINITY, f32::NEG_INFINITY), |a, b| a.max(*b));
        self.anchor = Rect::new(min.x, min.y, max.x - min.x, max.y - min.y);
        let track = Rect::new(progress.x, r.center().y - 0.005, progress.w, 0.010);
        ui.fill_path(&track.rounded(0.005), Color::from_hex_rgb(0x424242));
        let fraction = if music.duration > 0. {
            (music.position() / music.duration) as f32
        } else {
            0.
        };
        if fraction > 0. {
            let fill = Rect::new(track.x, track.y, track.w * fraction.clamp(0., 1.), track.h);
            ui.fill_path(&fill.rounded(0.005_f32.min(fill.w / 2.)), WHITE);
        }
        let disabled = music.custom || music.loading();
        self.random.render_shadow(ui, random, t, |ui, _| {
            let icon = texture_icon_rect(random, *icons.retry, RETRY_ARTWORK);
            ui.fill_rect(icon, (*icons.retry, icon, ScaleType::Fit, semi_white(if disabled { 0.25 } else { 0.8 })));
        });
        self.play.render_shadow(ui, play, t, |ui, _| {
            if music.user_paused || music.duration == 0. {
                let icon = texture_icon_rect(play, *icons.play, PLAY_ARTWORK);
                ui.fill_rect(icon, (*icons.play, icon, ScaleType::Fit, semi_white(0.9)));
            } else {
                // Reuse the gameplay pause symbol (the same two bars and proportions).
                prpr::ui::draw_pause_icon(ui, icon_rect(play), semi_white(0.9));
            }
        });
    }
    pub fn render_top(&mut self, ui: &mut Ui, music: &MenuMusic, title_fallback: &str) {
        if !self.popup {
            self.popup_rect = None;
            return;
        }
        ui.abs_scope(|ui| {
            let r = dropdown_rect(self.anchor, ui.top);
            self.popup_rect = Some(r);
            ui.fill_path(&r.rounded(0.008), semi_black(0.48));
            let title = if music.title.is_empty() { title_fallback } else { &music.title };
            ui.text(title).pos(r.x + 0.02, r.y + 0.025).size(0.48).max_width(r.w - 0.04).draw();
            ui.text(format!("{} / {}", timestamp(music.position()), timestamp(music.duration)))
                .pos(r.x + 0.02, r.y + 0.10)
                .size(0.42)
                .color(semi_white(0.7))
                .draw();
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_and_dropdown_fit_bottom_left_on_tablet_and_phone() {
        for top in [0.75, 0.5625, 0.428] {
            let panel = panel_rect(-0.95, top, 0.83);
            assert!(panel.x == -0.95 && panel.w == 0.83 && panel.right() < 0.);
            assert!(panel.bottom() <= top - 0.039);
            let (progress, random, play) = control_rects(panel);
            assert_eq!(random.w, play.w);
            assert!(progress.right() < random.x && random.right() < play.x && play.right() < panel.right());
            let drop = dropdown_rect(panel, top);
            assert!(drop.y >= -top && drop.bottom() <= top);
        }
        assert_eq!(timestamp(125.9), "2:05");
    }
}

#[cfg(test)]
pub(crate) async fn render_regression(painter: &mut prpr::ui::TextPainter) {
    fn ink_size(bytes: &[u8], width: usize, top: f32, rect: Rect) -> (usize, usize) {
        let scale = width as f32 / 2.;
        let mut min = (usize::MAX, usize::MAX);
        let mut max = (0, 0);
        for y in ((top - rect.bottom()) * scale).ceil() as usize..((top - rect.y) * scale).floor() as usize {
            for x in ((rect.x + 1.) * scale).ceil() as usize..((rect.right() + 1.) * scale).floor() as usize {
                let pixel = &bytes[(y * width + x) * 4..][..3];
                if pixel.iter().all(|v| *v > 160) {
                    min = (min.0.min(x), min.1.min(y));
                    max = (max.0.max(x), max.1.max(y));
                }
            }
        }
        assert_ne!(min.0, usize::MAX, "icon must be visible");
        (max.0 - min.0 + 1, max.1 - min.1 + 1)
    }
    prpr::core::init_assets();
    let isolated_hud = crate::hud::IsolatedTestLayout::new();
    crate::scene::TEX_ICON_BACK.with(|it| *it.borrow_mut() = Some(Texture2D::from_rgba8(1, 1, &[255; 4]).into()));
    let icons = Icons::new().await.unwrap();
    let mut player = MenuMusic::default();
    player.title = "背景音乐测试 / Menu Music".into();
    player.duration = 180.;
    let mut music = prpr::ui::UI_AUDIO
        .with(|it| {
            it.borrow_mut()
                .create_music(sasa::AudioClip::from_raw(vec![sasa::Frame(0., 0.); 180000], 1000), Default::default())
        })
        .unwrap();
    music.seek_to(84.).unwrap();
    player.music = Some(music);
    for _ in 0..3 {
        next_frame().await;
    }
    let mut panel = MusicPanel::default();
    for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::from_hex_rgb(0x253748));
        let play = Rect::new(0., -0.33, 0.83, 0.45);
        let row_y = play.bottom() + 0.05;
        let settings = Rect::new(0.71, row_y + 0.12, 0.11, 0.11);
        let title_left = crate::page::home_title_left(&ui);
        crate::page::Fader::new().render_title(&mut ui, 2., crate::page::HOME_LABEL);
        crate::hud::begin_frame();
        let r = music_slot(&ui, play.w);
        assert!((r.x - title_left).abs() < 1e-6);
        assert!(r.y >= -ui.top && r.bottom() <= ui.top);
        assert!(crate::hud::frame_slots().iter().any(|slot| slot.key == "music" && slot.cap.0));
        ui.fill_path(&play.rounded(0.008), semi_black(0.4));
        ui.text("游玩").pos(play.x + 0.04, play.y + 0.04).draw();
        for (r, name) in [
            (Rect::new(0., row_y, 0.38, 0.23), "活动"),
            (Rect::new(0.4, row_y, 0.29, 0.23), "资源包"),
            (settings, "设置"),
        ] {
            ui.fill_path(&r.rounded(0.008), semi_black(0.4));
            ui.text(name).pos(r.x + 0.02, r.y + 0.02).size(0.44).draw();
        }
        panel.render(&mut ui, r, 2., &player, &icons);
        let (progress, random, pause) = control_rects(r);
        assert_eq!(random.w, pause.w);
        let p = progress.center();
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            assert!(panel
                .touch(
                    &Touch {
                        id: 211,
                        phase,
                        position: p,
                        time: 2.
                    },
                    2.,
                    &mut player
                )
                .unwrap());
        }
        assert!(panel.popup, "progress click opens metadata");
        panel.render_top(&mut ui, &player, "暂无音乐");
        let drop = panel.popup_rect.unwrap();
        assert!(drop.y >= -ui.top && drop.bottom() <= ui.top && drop.x >= -1. && drop.right() <= 1.);
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glBindFramebuffer(GL_READ_FRAMEBUFFER, 0);
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        let random_ink = ink_size(&bytes, w as usize, ui.top, random);
        let pause_ink = ink_size(&bytes, w as usize, ui.top, pause);
        assert!(random_ink.0.abs_diff(random_ink.1) <= 2, "random icon must remain circular at {suffix}: {random_ink:?}");
        assert!(random_ink.1.abs_diff(pause_ink.1) <= 2, "visible icon heights must match at {suffix}");
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/judgement-panel-qa/home-music-{suffix}.png"));
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            panel
                .touch(
                    &Touch {
                        id: 212,
                        phase,
                        position: vec2(-0.8, 0.),
                        time: 2.,
                    },
                    2.,
                    &mut player,
                )
                .unwrap();
        }
        assert!(!panel.popup);
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            assert!(panel
                .touch(
                    &Touch {
                        id: 213,
                        phase,
                        position: pause.center(),
                        time: 2.
                    },
                    2.,
                    &mut player
                )
                .unwrap());
        }
        assert!(player.user_paused);
        clear_background(Color::from_hex_rgb(0x253748));
        panel.render(&mut ui, r, 2., &player, &icons);
        unsafe { get_internal_gl() }.flush();
        let mut play_bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, play_bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        let play_ink = ink_size(&play_bytes, w as usize, ui.top, pause);
        assert!(random_ink.1.abs_diff(play_ink.1) <= 2, "random and play visible heights must match at {suffix}");
        assert!((play_ink.0 as f32 - play_ink.1 as f32 * 74. / 90.).abs() <= 2., "play icon retains asset proportions");
        player.user_paused = false;
        if suffix == "21x9" {
            // Exercise the actual HUD editor, persistence and hit boxes after moving.
            crate::hud::set_cur_page(crate::hud::PageId::Home);
            crate::hud::set_snap(false);
            let mut editor = crate::hud::Editor::default();
            editor.render(&mut ui);
            let mut touch = Touch {
                id: 215,
                phase: TouchPhase::Started,
                position: r.center(),
                time: 2.,
            };
            editor.touch(&touch);
            editor.render(&mut ui);
            touch.phase = TouchPhase::Moved;
            touch.position += vec2(0.25, -0.15);
            editor.touch(&touch);
            editor.render(&mut ui);
            touch.phase = TouchPhase::Ended;
            editor.touch(&touch);
            editor.render(&mut ui);
            crate::hud::begin_frame();
            let moved = music_slot(&ui, play.w);
            assert!((moved.x - r.x - 0.25).abs() < 1e-5 && (moved.y - r.y + 0.15).abs() < 1e-5);
            let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(isolated_hud.saved_path()).unwrap()).unwrap();
            assert!(stored["pages"]["home"]["slots"]["music"].is_object());
            isolated_hud.reload_saved();
            crate::hud::begin_frame();
            assert_eq!(music_slot(&ui, play.w), moved, "HUD position must survive reloading the saved file");
            panel.render(&mut ui, moved, 2., &player, &icons);
            let new_play = control_rects(moved).2;
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                panel
                    .touch(
                        &Touch {
                            id: 216,
                            phase,
                            position: new_play.center(),
                            time: 2.,
                        },
                        2.,
                        &mut player,
                    )
                    .unwrap();
            }
            assert!(player.user_paused, "play hit box follows the saved HUD position");
        }
    }
    println!("Home music panel: production controls and dropdown rendered and clicked at 4:3 / 16:9 / 21:9.");
}
