//! Original exercise controls; extra configuration lives in the side drawer.
use super::{format_time, panel::Action};
use crate::{
    ext::RectExt,
    ui::{RectButton, Ui},
};
use macroquad::prelude::*;
use std::collections::BTreeMap;
prpr_l10n::tl_file!("game");

#[derive(Default)]
pub struct Base {
    start: RectButton,
    end: RectButton,
    markers: [(Rect, i8); 3],
    drag: Option<(u64, i8)>,
    bounds: (f64, f64),
    range: (f64, f64),
    points: BTreeMap<String, Vec2>,
}
impl Base {
    pub fn cancel(&mut self) {
        self.start.cancel();
        self.end.cancel();
        self.drag = None;
    }
    pub fn touch(&mut self, touch: &Touch) -> Option<Action> {
        if touch.phase == TouchPhase::Started && self.drag.is_none() {
            self.drag = self
                .markers
                .iter()
                .find(|(r, _)| r.contains(touch.position))
                .map(|(_, kind)| (touch.id, *kind));
        }
        if let Some((_, kind)) = self.drag.filter(|(id, _)| *id == touch.id) {
            if touch.phase == TouchPhase::Cancelled {
                self.drag = None;
                return None;
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.drag = None;
            }
            let fraction = ((touch.position.x + 0.7) / 1.4).clamp(0., 1.) as f64;
            let time = self.bounds.0 + (self.bounds.1 - self.bounds.0) * fraction;
            return Some(if kind == 0 {
                Action::Seek(time)
            } else {
                Action::Boundary(kind < 0, time)
            });
        }
        if self.start.touch(touch) {
            return Some(Action::Input("exercise_start".into(), format_time(self.range.0)));
        }
        if self.end.touch(touch) {
            return Some(Action::Input("exercise_end".into(), format_time(self.range.1)));
        }
        None
    }
    pub fn render(
        &mut self,
        ui: &mut Ui,
        speed: f32,
        start: f64,
        end: f64,
        current: f64,
        earliest: f64,
        length: f64,
        interactive: bool,
    ) -> Option<Action> {
        self.points.clear();
        self.range = (start, end);
        self.bounds = (earliest, length);
        let hidden_touches = if interactive { None } else { Some(std::mem::take(ui.ensure_touches())) };
        let mut next_speed = speed;
        ui.scope(|ui| {
            ui.dx(0.3);
            ui.dy(-0.3);
            let r = ui.slider(tl!("speed"), 0.1..2.0, 0.01, &mut next_speed, Some(0.5));
            let cy = r.h - 0.025;
            for (key, x) in [("speed-start", 0.), ("speed-end", 0.5), ("speed-minus", 0.545), ("speed-plus", 0.605)] {
                let p = ui.to_global((x, cy));
                self.points.insert(key.into(), vec2(p.0, p.1));
            }
        });
        ui.scope(|ui| {
            ui.dy(0.06);
            let (hw, h, eh, rad) = (0.7, 0.06, 0.12, 0.03);
            let position = |time: f64| -hw + ((time - earliest) / (length - earliest).max(0.001)) as f32 * hw * 2.;
            let (st, en, cur) = (position(start), position(end), position(current));
            ui.fill_rect(Rect::new(-hw, -h, hw * 2., h * 2.), GRAY);
            ui.fill_rect(Rect::new(st, -h, en - st, h * 2.), WHITE);
            for (i, (x, y, color, kind)) in [(st, -eh, BLUE, -1), (en, eh, RED, 1), (cur, 0., GREEN, 0)].into_iter().enumerate() {
                ui.fill_rect(Rect::new(x, if kind < 0 { -eh } else { -h }, 0., if kind == 0 { h * 2. } else { eh + h }).feather(0.005), color);
                ui.fill_circle(x, y, rad, color);
                self.markers[i] = (ui.rect_to_global(Rect::new(x, y, 0., 0.).feather(rad)), kind);
                self.points.insert(format!("marker:{kind}"), self.markers[i].0.center());
            }
            ui.text(format_time(current)).pos(0., -0.23).anchor(0.5, 0.).size(0.8).draw();
            ui.dy(0.2);
            let r = ui.text(tl!("to")).size(0.8).anchor(0.5, 0.).draw();
            for (key, value, x, anchor, button) in [
                ("exercise_start", start, r.x - 0.02, 1., &mut self.start),
                ("exercise_end", end, r.right() + 0.02, 0., &mut self.end),
            ] {
                let mut text = ui.text(format_time(value)).pos(x, 0.).anchor(anchor, 0.).size(0.8).color(BLACK);
                let rect = text.measure();
                button.set(text.ui, rect);
                text.ui
                    .fill_rect(rect.feather(0.01), Color::new(1., 1., 1., if button.touching() { 0.5 } else { 1. }));
                self.points.insert(key.into(), text.ui.rect_to_global(rect).center());
                text.draw();
            }
        });
        if let Some(touches) = hidden_touches {
            *ui.ensure_touches() = touches;
        }
        (interactive && (next_speed - speed).abs() > 0.0001).then(|| Action::Value("practice_speed".into(), next_speed))
    }
    #[cfg(record)]
    pub fn control_point(&self, key: &str) -> Option<Vec2> {
        self.points.get(key).copied()
    }
}
