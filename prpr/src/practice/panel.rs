//! Shared Phira rectangular controls, with a single clipped scroll surface.
prpr_l10n::tl_file!("game");
use super::{Bookmark, NoteBehavior, RangeAnchor, Statistics};
use crate::{
    config::Config,
    core::Matrix,
    ext::{RectExt, ScaleType},
    ui::{DRectButton, RectButton, Scroll, Ui},
};
use macroquad::prelude::*;
fn fmt_time(t: f32) -> String {
    super::format_time(t as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawer_cubic_transition_reverses_continuously_and_blocks_until_closed() {
        let mut panel = Panel::default();
        assert!(!panel.modal(10.));
        panel.set_open(true, 10.);
        assert_eq!(panel.fraction(10.), 0.);
        assert!((panel.fraction(10.25) - 0.875).abs() < 1e-6);
        assert_eq!(panel.fraction(10.5), 1.);
        let current = panel.fraction(10.25);
        panel.set_open(false, 10.25);
        assert_eq!(panel.fraction(10.25), current);
        assert!(panel.modal(10.5));
        assert!(!panel.modal(10.75));
        panel.hide();
        assert_eq!(panel.fraction(20.), 0.);
    }
}
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub enum Action {
    Input(String, String),
    Value(String, f32),
    Toggle(String),
    Load(usize),
    Delete(usize),
    Save,
    Clear,
    Seek(f64),
    Boundary(bool, f64),
    External(usize),
}
pub struct Panel {
    pub scroll: Scroll,
    buttons: BTreeMap<String, DRectButton>,
    actions: BTreeMap<String, Action>,
    points: BTreeMap<String, Vec2>,
    open: BTreeSet<String>,
    drawer_open: bool,
    slide_from: f32,
    slide_started: f32,
    toggle: RectButton,
    toggle_rect: Rect,
    outside_id: Option<u64>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            scroll: Scroll::new(),
            buttons: BTreeMap::new(),
            actions: BTreeMap::new(),
            points: BTreeMap::new(),
            open: BTreeSet::new(),
            drawer_open: false,
            slide_from: 0.,
            slide_started: f32::NEG_INFINITY,
            toggle: RectButton::default(),
            toggle_rect: Rect::default(),
            outside_id: None,
        }
    }
}
impl Panel {
    fn button(&mut self, ui: &mut Ui, t: f32, key: &str, text: impl Into<String>, r: Rect, chosen: bool, action: Action) {
        self.buttons
            .entry(key.into())
            .or_default()
            .render_text(ui, r, t, text.into(), 0.46, chosen);
        self.actions.insert(key.into(), action);
        self.points
            .insert(key.into(), vec2(ui.to_global(r.center().into()).0, ui.to_global(r.center().into()).1));
    }
    fn switch(&mut self, ui: &mut Ui, t: f32, id: &str, title: impl AsRef<str>, value: bool, y: &mut f32) {
        ui.text(title.as_ref())
            .pos(0.02, *y + 0.042)
            .anchor(0., 0.5)
            .size(0.46)
            .max_width(1.1)
            .draw();
        self.button(
            ui,
            t,
            id,
            if value { tl!("practice-on") } else { tl!("practice-off") }.into_owned(),
            Rect::new(1.25, *y, 0.39, 0.084),
            value,
            Action::Toggle(id.into()),
        );
        *y += 0.105;
    }
    fn number(
        &mut self,
        ui: &mut Ui,
        t: f32,
        id: &str,
        title: impl AsRef<str>,
        value: f32,
        step: f32,
        low: f32,
        high: f32,
        label: String,
        y: &mut f32,
    ) {
        ui.text(title.as_ref())
            .pos(0.02, *y + 0.042)
            .anchor(0., 0.5)
            .size(0.46)
            .max_width(0.93)
            .draw();
        self.button(
            ui,
            t,
            &format!("{id}-minus"),
            "−",
            Rect::new(1., *y, 0.10, 0.084),
            false,
            Action::Value(id.into(), (value - step).clamp(low, high)),
        );
        let input = if label.ends_with('%') {
            format!("{:.0}", value * 100.)
        } else {
            format!("{value:.2}")
        };
        self.button(ui, t, id, label, Rect::new(1.115, *y, 0.405, 0.084), false, Action::Input(id.into(), input));
        self.button(
            ui,
            t,
            &format!("{id}-plus"),
            "+",
            Rect::new(1.535, *y, 0.105, 0.084),
            false,
            Action::Value(id.into(), (value + step).clamp(low, high)),
        );
        *y += 0.105;
    }
    fn section(&mut self, ui: &mut Ui, t: f32, id: &str, title: impl AsRef<str>, y: &mut f32) -> bool {
        let opened = self.open.contains(id);
        *y += 0.012;
        self.button(
            ui,
            t,
            id,
            format!("{}  {}", if opened { "−" } else { "+" }, title.as_ref()),
            Rect::new(0., *y, 1.64, 0.085),
            false,
            Action::Toggle(format!("section:{id}")),
        );
        *y += 0.108;
        opened
    }
    // The drawer uses a half-second cubic ease-out transition.
    const TRANSIT: f32 = 0.5;
    const WIDTH: f32 = 1.6;
    const SCALE: f32 = (Self::WIDTH - 0.12) / 1.64;
    pub fn fraction(&self, t: f32) -> f32 {
        let p = ((t - self.slide_started) / Self::TRANSIT).clamp(0., 1.);
        let eased = 1. - (1. - p).powi(3);
        self.slide_from + ((if self.drawer_open { 1. } else { 0. }) - self.slide_from) * eased
    }
    pub fn modal(&self, t: f32) -> bool {
        self.drawer_open || self.fraction(t) > 0.0001 || self.outside_id.is_some()
    }
    pub fn set_open(&mut self, open: bool, t: f32) {
        self.slide_from = self.fraction(t);
        self.slide_started = t;
        self.drawer_open = open;
        self.scroll.x_scroller.halt();
        self.scroll.y_scroller.halt();
        for button in self.buttons.values_mut() {
            button.cancel(t);
        }
    }
    pub fn hide(&mut self) {
        self.drawer_open = false;
        self.slide_from = 0.;
        self.slide_started = f32::NEG_INFINITY;
        self.outside_id = None;
        self.scroll.x_scroller.halt();
        self.scroll.y_scroller.halt();
        self.toggle.cancel();
    }
    pub fn suspend(&mut self, t: f32) {
        self.scroll.x_scroller.suspend(t);
        self.scroll.y_scroller.suspend(t);
        self.outside_id = None;
        self.toggle.cancel();
        for button in self.buttons.values_mut() {
            button.cancel(t);
        }
    }
    pub fn touch(&mut self, touch: &Touch, t: f32) -> (bool, Option<Action>) {
        if self.outside_id == Some(touch.id) {
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.outside_id = None;
            }
            return (true, None);
        }
        let on_toggle = self.toggle.contains(touch.position);
        if self.toggle.touch(touch) {
            self.set_open(!self.drawer_open, t);
            return (true, None);
        }
        if on_toggle {
            self.scroll.touch(touch, t);
            return (true, None);
        }
        if !self.modal(t) {
            return (false, None);
        }
        if !self.drawer_open || t < self.slide_started + Self::TRANSIT {
            return (true, None);
        }
        if touch.position.x > -1. + Self::WIDTH {
            if touch.phase == TouchPhase::Started {
                self.outside_id = Some(touch.id);
                self.set_open(false, t);
            } else {
                self.scroll.touch(touch, t);
            }
            return (true, None);
        }
        (true, self.content_touch(touch, t))
    }
    fn content_touch(&mut self, touch: &Touch, t: f32) -> Option<Action> {
        if !self.scroll.contains(touch) && touch.phase == TouchPhase::Started {
            return None;
        }
        if self.scroll.touch(touch, t) {
            for button in self.buttons.values_mut() {
                button.cancel(t);
            }
            return None;
        }
        if !self.scroll.contains(touch) {
            for button in self.buttons.values_mut() {
                button.cancel(t);
            }
            return None;
        }
        for (id, action) in &self.actions {
            if self.buttons.get_mut(id).unwrap().touch(touch, t) {
                if let Action::Toggle(id) = action {
                    if let Some(section) = id.strip_prefix("section:") {
                        if !self.open.remove(section) {
                            self.open.insert(section.into());
                        }
                        return None;
                    }
                }
                return Some(action.clone());
            }
        }
        None
    }
    #[cfg(record)]
    pub fn control_point(&self, key: &str) -> Option<Vec2> {
        self.points.get(key).copied()
    }
    pub fn render(
        &mut self,
        ui: &mut Ui,
        t: f32,
        config: &Config,
        combo: (usize, usize),
        auto: bool,
        bookmarks: &[Bookmark],
        stats: &Statistics,
        back: Texture2D,
    ) {
        self.actions.clear();
        self.points.clear();
        for button in self.buttons.values_mut() {
            button.invalidate();
        }
        let progress = self.fraction(t);
        let top = ui.top;
        if progress > 0.0001 {
            ui.fill_rect(Rect::new(-1., -top, 2., 2. * top), Color::new(0., 0., 0., progress * 0.6));
            let left = -1. - Self::WIDTH + Self::WIDTH * progress;
            let rect = Rect::new(left + 0.02, -top + 0.02, Self::WIDTH - 0.04, top * 2. - 0.04);
            ui.fill_path(&rect.rounded(0.02), ui.background());
            self.scroll.update(t);
            let mut scroll = std::mem::take(&mut self.scroll);
            ui.scope(|ui| {
                ui.dx(left + 0.06);
                ui.dy(-top + 0.06);
                ui.text(tl!("practice-settings")).size(0.5).draw();
                ui.dy(0.12);
                ui.with(crate::core::Matrix::new_scaling(Self::SCALE), |ui| {
                    scroll.size((1.64, ((top * 2. - 0.23) / Self::SCALE).max(0.2)));
                    scroll.render(ui, |ui| {
                        let mut y = 0.015;
                        let p = &config.practice;
                        ui.text(tl!("practice-combo-range")).pos(0.02, y).size(0.46).draw();
                        y += 0.075;
                        self.button(
                            ui,
                            t,
                            "exercise_combo_start",
                            combo.0.to_string(),
                            Rect::new(0., y, 0.72, 0.084),
                            false,
                            Action::Input("exercise_combo_start".into(), combo.0.to_string()),
                        );
                        ui.text(tl!("to")).pos(0.82, y + 0.042).anchor(0.5, 0.5).size(0.4).draw();
                        self.button(
                            ui,
                            t,
                            "exercise_combo_end",
                            combo.1.to_string(),
                            Rect::new(0.92, y, 0.72, 0.084),
                            false,
                            Action::Input("exercise_combo_end".into(), combo.1.to_string()),
                        );
                        y += 0.105;
                        self.switch(ui, t, "practice_loop", tl!("practice-loop"), auto, &mut y);
                        if self.section(ui, t, "analysis", "谱面分析与回放", &mut y) {
                            for (i, label) in ["密度与失误时间轴", "双回放对照", "单音符判定详情"].iter().enumerate() {
                                self.button(ui, t, &format!("practice_tool_{i}"), *label, Rect::new(0., y, 1.64, 0.084), false, Action::External(i));
                                y += 0.105;
                            }
                        }
                        if self.section(ui, t, "adaptive", "达标晋级练习", &mut y) {
                            self.switch(ui, t, "practice_adaptive", "达标后提速", p.adaptive.enabled, &mut y);
                            self.number(
                                ui,
                                t,
                                "practice_target_accuracy",
                                "目标准度",
                                p.adaptive.accuracy,
                                0.01,
                                0.,
                                1.,
                                format!("{:.2}%", p.adaptive.accuracy * 100.),
                                &mut y,
                            );
                            self.number(
                                ui,
                                t,
                                "practice_consecutive",
                                "连续达标轮数",
                                p.adaptive.consecutive as f32,
                                1.,
                                1.,
                                99.,
                                p.adaptive.consecutive.to_string(),
                                &mut y,
                            );
                            self.switch(ui, t, "practice_goal_fc", "同时要求全连", p.adaptive.full_combo, &mut y);
                            self.switch(ui, t, "practice_goal_drop", "未达标时降低一档", p.adaptive.lower_on_failure, &mut y);
                            ui.text("使用变速练习的起始倍速、每档增幅及原版速度滑条设置的目标倍速。")
                                .pos(0.02, y)
                                .size(0.30)
                                .max_width(1.6)
                                .multiline()
                                .draw();
                            y += 0.10;
                        }
                        if self.section(ui, t, "audio", tl!("practice-audio"), &mut y) {
                            self.switch(ui, t, "practice_pitch", tl!("practice-pitch"), p.preserve_pitch, &mut y);
                            self.switch(ui, t, "practice_correct", tl!("practice-correct"), p.correct_sound, &mut y);
                        }
                        if self.section(ui, t, "ramp", tl!("practice-ramp"), &mut y) {
                            self.switch(ui, t, "practice_ramp", tl!("practice-ramp"), config.practice_ramp, &mut y);
                            self.number(
                                ui,
                                t,
                                "practice_ramp_start",
                                tl!("practice-ramp-start"),
                                config.practice_speed_start,
                                0.01,
                                0.1,
                                2.,
                                format!("{:.2}x", config.practice_speed_start),
                                &mut y,
                            );
                            self.number(
                                ui,
                                t,
                                "practice_ramp_step",
                                tl!("practice-ramp-step"),
                                config.practice_speed_step,
                                0.01,
                                0.01,
                                0.5,
                                format!("+{:.2}x", config.practice_speed_step),
                                &mut y,
                            );
                            self.number(
                                ui,
                                t,
                                "practice_retry",
                                tl!("practice-retry"),
                                config.auto_retry,
                                1.,
                                0.,
                                10.,
                                format!("{:.0}", config.auto_retry),
                                &mut y,
                            );
                            self.number(
                                ui,
                                t,
                                "practice_retry_lead",
                                tl!("practice-retry-lead"),
                                config.retry_lead,
                                0.5,
                                0.,
                                10.,
                                format!("{:.1}s", config.retry_lead),
                                &mut y,
                            );
                            ui.text(tl!("practice-retry-sub"))
                                .pos(0.02, y)
                                .size(0.31)
                                .max_width(1.6)
                                .multiline()
                                .draw();
                            y += 0.09;
                        }
                        if self.section(ui, t, "judgement", tl!("practice-judgement"), &mut y) {
                            self.switch(ui, t, "practice_inverse", tl!("practice-inverse"), p.inverse_speed, &mut y);
                            self.number(
                                ui,
                                t,
                                "practice_multiplier",
                                tl!("practice-multiplier"),
                                p.multiplier(config.speed) as f32,
                                0.01,
                                0.1,
                                3.,
                                format!("{:.0}%", p.multiplier(config.speed) * 100.),
                                &mut y,
                            );
                        }
                        if self.section(ui, t, "view", tl!("practice-view"), &mut y) {
                            self.number(
                                ui,
                                t,
                                "practice_scale",
                                tl!("practice-scale"),
                                p.view.scale,
                                0.05,
                                0.1,
                                5.,
                                format!("{:.0}%", p.view.scale * 100.),
                                &mut y,
                            );
                            self.number(ui, t, "practice_x", tl!("practice-x"), p.view.x, 0.05, -4., 4., format!("{:.2}", p.view.x), &mut y);
                            self.number(ui, t, "practice_y", tl!("practice-y"), p.view.y, 0.05, -4., 4., format!("{:.2}", p.view.y), &mut y);
                            self.button(
                                ui,
                                t,
                                "view_reset",
                                tl!("practice-reset").into_owned(),
                                Rect::new(1., y, 0.64, 0.084),
                                false,
                                Action::Toggle("practice_view_reset".into()),
                            );
                            y += 0.105;
                        }
                        if self.section(ui, t, "debug", tl!("practice-ranges"), &mut y) {
                            self.switch(ui, t, "practice_ranges", tl!("practice-ranges"), p.ranges, &mut y);
                            ui.text(tl!("practice-range-anchor"))
                                .pos(0.02, y + 0.042)
                                .anchor(0., 0.5)
                                .size(0.42)
                                .max_width(0.8)
                                .draw();
                            let label = match p.range_anchor {
                                RangeAnchor::JudgeLine => tl!("practice-range-line"),
                                RangeAnchor::Note => tl!("practice-range-note"),
                                RangeAnchor::Both => tl!("practice-range-both"),
                            };
                            self.button(
                                ui,
                                t,
                                "practice_range_anchor",
                                label.into_owned(),
                                Rect::new(0.85, y, 0.79, 0.084),
                                false,
                                Action::Toggle("practice_range_anchor".into()),
                            );
                            y += 0.105;
                            for (i, label) in ["Tap", "Hold", "Drag", "Flick"].iter().enumerate() {
                                self.switch(ui, t, &format!("practice_kind_{i}"), label, p.range_kinds[i], &mut y);
                            }
                            if self.section(ui, t, "range_tap", tl!("practice-range-tap"), &mut y) {
                                for (i, label) in ["PERFECT", "GOOD", "BAD", "MISS"].iter().enumerate() {
                                    self.switch(ui, t, &format!("practice_tap_range_{i}"), label, p.range_tap[i], &mut y);
                                }
                            }
                            if self.section(ui, t, "range_hold", tl!("practice-range-hold"), &mut y) {
                                for (i, label) in ["PERFECT", "GOOD", "MISS"].iter().enumerate() {
                                    self.switch(ui, t, &format!("practice_hold_range_{i}"), label, p.range_hold[i], &mut y);
                                }
                                self.switch(ui, t, "practice_hold_range_3", tl!("practice-range-tail"), p.range_hold[3], &mut y);
                            }
                            self.number(
                                ui,
                                t,
                                "practice_range_alpha",
                                tl!("practice-opacity"),
                                p.range_alpha,
                                0.02,
                                0.02,
                                0.6,
                                format!("{:.0}%", p.range_alpha * 100.),
                                &mut y,
                            );
                            self.number(
                                ui,
                                t,
                                "practice_horizon",
                                tl!("practice-horizon"),
                                p.range_horizon,
                                0.1,
                                0.1,
                                10.,
                                format!("{:.1}s", p.range_horizon),
                                &mut y,
                            );
                            for (id, label, value) in [
                                ("practice_debug", tl!("practice-debug"), config.chart_debug),
                                ("practice_debug_line", tl!("practice-debug-line"), config.chart_debug_line),
                                ("practice_debug_note", tl!("practice-debug-note"), config.chart_debug_note),
                            ] {
                                self.switch(ui, t, id, label, value, &mut y);
                            }
                        }
                        if self.section(ui, t, "behavior", tl!("practice-behavior"), &mut y) {
                            let label = match p.behavior {
                                NoteBehavior::Original => tl!("practice-original").into_owned(),
                                NoteBehavior::Tap => "Tap".into(),
                                NoteBehavior::Drag => "Drag".into(),
                                NoteBehavior::Flick => "Flick".into(),
                            };
                            self.button(
                                ui,
                                t,
                                "practice_behavior",
                                label,
                                Rect::new(1., y, 0.64, 0.084),
                                false,
                                Action::Toggle("practice_behavior".into()),
                            );
                            ui.text(tl!("practice-conversion")).pos(0.02, y + 0.042).anchor(0., 0.5).size(0.42).draw();
                            y += 0.105;
                            ui.text(tl!("practice-conversion-sub")).pos(0.02, y).size(0.31).max_width(1.6).draw();
                            y += 0.07;
                        }
                        if self.section(ui, t, "bookmarks", tl!("practice-bookmarks"), &mut y) {
                            self.button(ui, t, "save", tl!("practice-save").into_owned(), Rect::new(0., y, 1.64, 0.084), false, Action::Save);
                            y += 0.105;
                            if bookmarks.is_empty() {
                                ui.text(tl!("practice-empty")).pos(0.02, y).size(0.35).draw();
                                y += 0.08;
                            }
                            for (i, b) in bookmarks.iter().enumerate() {
                                self.button(
                                    ui,
                                    t,
                                    &format!("load:{i}"),
                                    format!("{} · {}–{} · {:.2}x", b.name, fmt_time(b.start as f32), fmt_time(b.end as f32), b.speed),
                                    Rect::new(0., y, 1.35, 0.084),
                                    false,
                                    Action::Load(i),
                                );
                                self.button(
                                    ui,
                                    t,
                                    &format!("delete:{i}"),
                                    tl!("practice-delete").into_owned(),
                                    Rect::new(1.38, y, 0.26, 0.084),
                                    false,
                                    Action::Delete(i),
                                );
                                y += 0.105;
                            }
                        }
                        if self.section(ui, t, "statistics", tl!("practice-statistics"), &mut y) {
                            self.button(ui, t, "clear", tl!("practice-clear").into_owned(), Rect::new(1.25, y, 0.39, 0.084), false, Action::Clear);
                            ui.text(format!("{}: {}", tl!("practice-rounds"), stats.total))
                                .pos(0.02, y + 0.042)
                                .anchor(0., 0.5)
                                .size(0.4)
                                .draw();
                            y += 0.105;
                            if let Some(best) = &stats.best {
                                ui.text(format!("{}: {:.2}% · {:.2}x · {}", tl!("practice-best"), best.accuracy * 100., best.speed, best.score))
                                    .pos(0.02, y)
                                    .size(0.38)
                                    .draw();
                                y += 0.075;
                            }
                            for (i, round) in stats.rounds.iter().enumerate().rev() {
                                ui.text(format!(
                                    "#{} · {:.2}x · {:.2}% · {}",
                                    stats.total - stats.rounds.len() as u32 + i as u32 + 1,
                                    round.speed,
                                    round.accuracy * 100.,
                                    round.score
                                ))
                                .pos(0.02, y)
                                .size(0.38)
                                .draw();
                                y += 0.06;
                                let c = round.counts;
                                ui.text(format!("PERFECT+ {} / PERFECT {} / GREAT {} / GOOD {}", c[4], c[0], c[5], c[1]))
                                    .pos(0.02, y)
                                    .size(0.32)
                                    .draw();
                                y += 0.055;
                                ui.text(format!("OK {} / MEH {} / BAD {} / MISS {}", c[6], c[7], c[2], c[3]))
                                    .pos(0.02, y)
                                    .size(0.32)
                                    .draw();
                                y += 0.075;
                            }
                        }
                        (1.64, y + 0.03)
                    });
                });
            });
            self.scroll = scroll;
        }
        // Same opaque fill and overlapping flat edge join the tab to the body
        // throughout the slide. Only the two outer corners remain rounded.
        let x = -1.02 + Self::WIDTH * progress;
        self.toggle_rect = Rect::new(x, -0.10, 0.09, 0.20);
        self.toggle.set(ui, self.toggle_rect.feather(0.01));
        ui.fill_path(&self.toggle_rect.rounded(0.04), ui.background());
        ui.fill_rect(Rect::new(x - 0.005, -0.10, 0.045, 0.20), ui.background());
        let visible_left = self.toggle_rect.x.max(-1.);
        let center = vec2((visible_left + self.toggle_rect.right()) * 0.5, 0.);
        // Reuse the stock arrow's alpha edge with trilinear minification instead
        // of a hard, non-antialiased polygon stroke in the UI pass.
        ui.scope(|ui| {
            ui.dx(center.x);
            ui.with(Matrix::new_rotation(if self.drawer_open { 0. } else { std::f32::consts::PI }), |ui| {
                let r = Rect::new(-0.05, -0.05, 0.10, 0.10);
                ui.fill_rect(r, (back, r, ScaleType::Fit, Color::new(1., 1., 1., if self.toggle.touching() { 0.6 } else { 1. })));
            });
        });
        self.points.insert("drawer".into(), ui.rect_to_global(self.toggle_rect).center());
    }
}
