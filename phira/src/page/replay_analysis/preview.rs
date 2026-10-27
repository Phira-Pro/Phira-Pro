//! Modal image viewer: drag, pinch or mouse wheel to inspect the full report.
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, RectExt, SafeTexture, ScaleType},
    ui::{DRectButton, Ui},
};
use std::{collections::BTreeMap, path::PathBuf};
pub struct Preview {
    pub path: PathBuf,
    texture: SafeTexture,
    pub zoom: f32,
    pan: Vec2,
    area: Rect,
    fingers: BTreeMap<u64, Vec2>,
    pub(super) buttons: Vec<DRectButton>,
    pub closed: bool,
    first_layout: bool,
}
impl Preview {
    pub fn new(path: PathBuf, image: image::RgbaImage) -> Self {
        let preview = image::DynamicImage::ImageRgba8(image).thumbnail(2048, 4096);
        Self {
            path,
            texture: SafeTexture::from(preview),
            zoom: 1.,
            pan: Vec2::ZERO,
            area: Rect::new(0., 0., 0., 0.),
            fingers: BTreeMap::new(),
            buttons: (0..5).map(|_| DRectButton::new()).collect(),
            closed: false,
            first_layout: true,
        }
    }
    pub fn update(&mut self) {
        let (_, wheel) = prpr::judge::take_wheel();
        if wheel != 0. {
            self.zoom = (self.zoom * (1. + wheel.signum() * 0.15)).clamp(1., 32.);
            if self.zoom == 1. {
                self.pan = Vec2::ZERO;
            }
        }
    }
    pub fn touch(&mut self, touch: &Touch, t: f32, saving: bool) -> bool {
        if self.buttons[0].touch(touch, t) {
            self.closed = true;
            return false;
        }
        if self.buttons[1].touch(touch, t) {
            self.zoom = (self.zoom / 1.35).max(1.);
            if self.zoom == 1. {
                self.pan = Vec2::ZERO;
            }
            return false;
        }
        if self.buttons[2].touch(touch, t) {
            self.zoom = (self.zoom * 1.35).min(32.);
            return false;
        }
        if self.buttons[3].touch(touch, t) {
            self.zoom = 1.;
            self.pan = Vec2::ZERO;
            return false;
        }
        if !saving && self.buttons[4].touch(touch, t) {
            return true;
        }
        let old: Vec<_> = self.fingers.values().copied().collect();
        match touch.phase {
            TouchPhase::Started if self.area.contains(touch.position) => {
                self.fingers.insert(touch.id, touch.position);
            }
            TouchPhase::Moved | TouchPhase::Stationary => {
                if let Some(previous) = self.fingers.insert(touch.id, touch.position) {
                    if self.fingers.len() == 1 {
                        self.pan += touch.position - previous;
                    } else if old.len() == 2 && self.fingers.len() == 2 {
                        let new: Vec<_> = self.fingers.values().copied().collect();
                        let before = old[0].distance(old[1]);
                        if before > 0.01 {
                            self.zoom = (self.zoom * new[0].distance(new[1]) / before).clamp(1., 32.);
                        }
                        self.pan += (new[0] + new[1] - old[0] - old[1]) / 2.;
                    }
                } else {
                    self.fingers.remove(&touch.id);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.fingers.remove(&touch.id);
            }
            _ => {}
        }
        false
    }
    pub fn render(&mut self, ui: &mut Ui, t: f32, saving: bool) {
        ui.fill_rect(ui.screen_rect(), semi_black(0.76));
        let window = ui.screen_rect().feather(-0.065);
        ui.fill_path(&window.rounded(0.015), Color::from_rgba(20, 28, 39, 255));
        ui.text("PRCA 分析结果").pos(window.x + 0.035, window.y + 0.026).size(0.46).draw();
        let bh = 0.065;
        let bw = 0.15;
        for (i, label) in [(0, "关闭"), (1, "缩小"), (2, "放大"), (3, "适应窗口")] {
            let col = match i {
                0 => 3,
                1 => 0,
                2 => 1,
                _ => 2,
            };
            self.buttons[i].render_text(
                ui,
                Rect::new(window.right() - 0.025 - (4 - col) as f32 * (bw + 0.012), window.y + 0.02, bw, bh),
                t,
                label,
                0.3,
                false,
            );
        }
        self.area = Rect::new(window.x + 0.025, window.y + 0.11, window.w - 0.05, (window.h - 0.25).max(0.1));
        ui.fill_rect(self.area, Color::from_rgba(31, 39, 50, 255));
        let fit = (self.area.w / self.texture.width()).min(self.area.h / self.texture.height());
        if self.first_layout {
            self.zoom = (self.area.w / self.texture.width() / fit).clamp(1., 32.);
        }
        let scale = fit * self.zoom;
        let size = vec2(self.texture.width() * scale, self.texture.height() * scale);
        let max = (size - vec2(self.area.w, self.area.h)).max(Vec2::ZERO) / 2.;
        if self.first_layout {
            self.pan = vec2(0., max.y);
            self.first_layout = false;
        }
        self.pan = self.pan.clamp(-max, max);
        let center = self.area.center() + self.pan;
        let image = Rect::new(center.x - size.x / 2., center.y - size.y / 2., size.x, size.y);
        ui.scissor(self.area, |ui| ui.fill_rect(image, (*self.texture, image, ScaleType::Fit)));
        self.buttons[4].render_text(
            ui,
            Rect::new(window.center().x - 0.25, window.bottom() - 0.105, 0.5, 0.075),
            t,
            if saving { "保存中…" } else { "保存图片" },
            0.38,
            false,
        );
        ui.text(format!("{:.0}%", self.zoom * 100.))
            .pos(window.x + 0.035, window.bottom() - 0.08)
            .size(0.29)
            .color(Color::from_rgba(180, 193, 210, 255))
            .draw();
    }
}
