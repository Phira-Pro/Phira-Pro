//! MainScene owns one overlay. Hiding it never replaces the room Session.
use super::panel::MultiplayerPage;
use crate::{
    get_data, get_data_mut,
    icons::Icons,
    page::{Page, SharedState},
    save_data,
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{SafeTexture, ScaleType},
    scene::NextScene,
    ui::{button_hit, Ui},
};
use std::sync::Arc;

const RADIUS: f32 = 0.055;
const DRAG_THRESHOLD: f32 = 0.025;
#[derive(Clone, Copy)]
struct Drag {
    id: u64,
    start: Vec2,
    origin: Vec2,
    moved: bool,
}
#[derive(Debug, PartialEq)]
enum Gesture {
    None,
    Open,
    Saved([f32; 2]),
}

struct Launcher {
    anchor: Vec2,
    top: f32,
    drag: Option<Drag>,
}
impl Launcher {
    fn new(saved: Option<[f32; 2]>) -> Self {
        let anchor = saved
            .filter(|p| p.iter().all(|v| v.is_finite()))
            .map(Vec2::from)
            .unwrap_or(vec2(-0.9, 0.));
        Self {
            anchor,
            top: 0.5625,
            drag: None,
        }
    }
    fn center(&self) -> Vec2 {
        vec2(self.anchor.x, self.anchor.y * self.top)
    }
    fn clamp(&mut self, top: f32) {
        self.top = top.max(RADIUS * 2.);
        self.anchor.x = self.anchor.x.clamp(-1. + RADIUS + 0.01, 1. - RADIUS - 0.01);
        self.anchor.y = self.anchor.y.clamp(-1. + (RADIUS + 0.01) / self.top, 1. - (RADIUS + 0.01) / self.top);
    }
    fn touch(&mut self, touch: &Touch) -> (bool, Gesture) {
        if let Some(mut drag) = self.drag {
            if drag.id != touch.id {
                return (false, Gesture::None);
            }
            if touch.phase == TouchPhase::Cancelled {
                self.anchor = drag.origin;
                self.drag = None;
                return (true, Gesture::None);
            }
            if (touch.position - drag.start).length() > DRAG_THRESHOLD {
                drag.moved = true;
            }
            if drag.moved {
                let pos = vec2(drag.origin.x, drag.origin.y * self.top) + touch.position - drag.start;
                self.anchor = vec2(pos.x, pos.y / self.top);
                self.clamp(self.top);
            }
            if touch.phase == TouchPhase::Ended {
                self.drag = None;
                return (
                    true,
                    if drag.moved {
                        Gesture::Saved(self.anchor.to_array())
                    } else if touch.position.distance(self.center()) <= RADIUS {
                        Gesture::Open
                    } else {
                        Gesture::None
                    },
                );
            }
            self.drag = Some(drag);
            return (true, Gesture::None);
        }
        if touch.phase == TouchPhase::Started && touch.position.distance(self.center()) <= RADIUS {
            self.drag = Some(Drag {
                id: touch.id,
                start: touch.position,
                origin: self.anchor,
                moved: false,
            });
            return (true, Gesture::None);
        }
        (false, Gesture::None)
    }
}

pub struct MultiplayerOverlay {
    page: MultiplayerPage,
    icons: Arc<Icons>,
    icon: SafeTexture,
    launcher: Launcher,
    open: bool,
    progress: f32,
    last_time: f32,
    enabled: bool,
    next: Option<NextScene>,
    chart_picker_requested: bool,
}
impl MultiplayerOverlay {
    pub async fn new(icons: Arc<Icons>) -> Result<Self> {
        Ok(Self {
            page: MultiplayerPage::new(Arc::clone(&icons)),
            icons,
            icon: load_texture("multiplayer.png").await?.into(),
            launcher: Launcher::new(get_data().multiplayer_button_position),
            open: false,
            progress: 0.,
            last_time: 0.,
            enabled: get_data().multiplayer_enabled,
            next: None,
            chart_picker_requested: false,
        })
    }
    pub fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        self.page.enter(s)
    }
    pub fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let enabled = get_data().multiplayer_enabled;
        if self.enabled && !enabled {
            self.page.shutdown();
            self.page = MultiplayerPage::new(Arc::clone(&self.icons));
            self.open = false;
            self.progress = 0.;
            self.launcher = Launcher::new(get_data().multiplayer_button_position);
        }
        self.enabled = enabled;
        let dt = (s.rt - self.last_time).clamp(0., 0.05);
        self.last_time = s.rt;
        let target = if self.open { 1. } else { 0. };
        self.progress = if get_data().prefer_reduced_motion {
            target
        } else if self.progress < target {
            (self.progress + dt / 0.22).min(target)
        } else {
            (self.progress - dt / 0.18).max(target)
        };
        if !enabled {
            return Ok(());
        }
        self.page.update(s)?;
        let next = self.page.next_scene(s);
        if !matches!(next, NextScene::None) {
            self.open = true;
            self.progress = 1.;
            self.next = Some(next);
        }
        Ok(())
    }
    pub fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if !self.enabled {
            return Ok(false);
        }
        if self.open || self.progress > 0. {
            if self.progress >= 1. && self.open {
                self.page.touch(touch, s)?;
                if self.page.take_chart_picker_request() {
                    self.chart_picker_requested = true;
                }
                if self.page.take_close_request() {
                    self.open = false;
                    self.page.hide(s.t);
                }
            }
            return Ok(true);
        }
        let (handled, gesture) = self.launcher.touch(touch);
        match gesture {
            Gesture::Open => {
                button_hit();
                self.open = true;
            }
            Gesture::Saved(position) => {
                get_data_mut().multiplayer_button_position = Some(position);
                save_data()?;
            }
            Gesture::None => {}
        }
        Ok(handled)
    }
    pub fn render(&mut self, ui: &mut Ui, s: &mut SharedState, background: &SafeTexture) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.launcher.clamp(ui.top);
        let p = self.launcher.center();
        if self.progress < 1. {
            ui.fill_circle(p.x, p.y, RADIUS, ui.background());
            let r = Rect::new(p.x - 0.032, p.y - 0.032, 0.064, 0.064);
            ui.fill_rect(r, (*self.icon, r, ScaleType::Fit));
        }
        if self.progress > 0. {
            let eased = 1. - (1. - self.progress).powi(3);
            ui.alpha(eased, |ui| {
                let screen = ui.screen_rect();
                ui.fill_rect(screen, (**background, screen, ScaleType::CropCenter));
                ui.fill_rect(screen, Color::new(0.055, 0.045, 0.075, 0.94));
                ui.scope(|ui| {
                    ui.dx((1. - eased) * 0.22);
                    self.page.render(ui, s)
                })
            })?;
            if !self.open || self.progress < 1. {
                self.page.invalidate_controls();
            }
        }
        Ok(())
    }
    pub fn next_scene(&mut self) -> NextScene {
        self.next.take().unwrap_or_default()
    }
    pub fn take_chart_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.chart_picker_requested)
    }
    pub fn chart_selected(&mut self, id: i32) {
        self.page.select_chart(id);
        self.open = true;
        self.progress = 1.;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn touch(id: u64, phase: TouchPhase, position: Vec2) -> Touch {
        Touch {
            id,
            phase,
            position,
            time: 0.,
        }
    }
    #[test]
    fn click_opens_but_drag_and_cancel_never_open() {
        let mut b = Launcher::new(None);
        let p = b.center();
        assert_eq!(b.touch(&touch(1, TouchPhase::Started, p)), (true, Gesture::None));
        assert_eq!(b.touch(&touch(1, TouchPhase::Ended, p)), (true, Gesture::Open));
        b.touch(&touch(2, TouchPhase::Started, p));
        assert!(!b.touch(&touch(3, TouchPhase::Ended, p)).0);
        b.touch(&touch(2, TouchPhase::Moved, vec2(0.4, 0.2)));
        assert!(matches!(b.touch(&touch(2, TouchPhase::Ended, vec2(0.4, 0.2))).1, Gesture::Saved(_)));
        let anchor = b.anchor;
        let p = b.center();
        b.touch(&touch(4, TouchPhase::Started, p));
        b.touch(&touch(4, TouchPhase::Moved, vec2(0., 0.)));
        b.touch(&touch(4, TouchPhase::Cancelled, vec2(0., 0.)));
        assert_eq!(b.anchor, anchor);
    }
    #[test]
    fn restored_position_stays_inside_screen_after_aspect_changes() {
        let mut b = Launcher::new(Some([20., -20.]));
        for top in [0.75, 0.5625, 0.4286, 0.28125] {
            b.clamp(top);
            let p = b.center();
            assert!(p.x.abs() + RADIUS < 1.);
            assert!(p.y.abs() + RADIUS < top);
        }
        assert_eq!(Launcher::new(Some([f32::NAN, 1.])).anchor, vec2(-0.9, 0.));
    }
}
