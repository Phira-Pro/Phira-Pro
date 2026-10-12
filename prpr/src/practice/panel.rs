//! Shared Phira rectangular controls, with a single clipped scroll surface.
prpr_l10n::tl_file!("game");
use super::{Bookmark, NoteBehavior, Statistics};
use crate::{
    config::Config,
    ui::{DRectButton, Scroll, Ui},
};
use macroquad::prelude::*;
fn fmt_time(t: f32) -> String {
    super::format_time(t as f64)
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
}
pub struct Panel {
    pub scroll: Scroll,
    buttons: BTreeMap<String, DRectButton>,
    actions: BTreeMap<String, Action>,
    points: BTreeMap<String, Vec2>,
    open: BTreeSet<String>,
    timeline: Rect,
    timeline_id: Option<(u64, i8)>,
    bounds: (f64, f64),
    range: (f64, f64),
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            scroll: Scroll::new(),
            buttons: BTreeMap::new(),
            actions: BTreeMap::new(),
            points: BTreeMap::new(),
            open: BTreeSet::new(),
            timeline: Rect::default(),
            timeline_id: None,
            bounds: (0., 1.),
            range: (0., 1.),
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
    pub fn touch(&mut self, touch: &Touch, t: f32) -> Option<Action> {
        if self.scroll.contains(touch) && touch.phase == TouchPhase::Started && self.timeline.contains(touch.position) {
            let fraction = ((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.) as f64;
            let time = self.bounds.0 + (self.bounds.1 - self.bounds.0) * fraction;
            let threshold = (self.bounds.1 - self.bounds.0) * 0.025;
            let which = if (time - self.range.0).abs() < (time - self.range.1).abs() && (time - self.range.0).abs() < threshold {
                -1
            } else if (time - self.range.1).abs() < threshold {
                1
            } else {
                0
            };
            self.timeline_id = Some((touch.id, which));
        }
        if let Some((_, which)) = self.timeline_id.filter(|(id, _)| *id == touch.id) {
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.timeline_id = None;
            }
            let p = ((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.) as f64;
            let time = self.bounds.0 + (self.bounds.1 - self.bounds.0) * p;
            return Some(if which == 0 {
                Action::Seek(time)
            } else {
                Action::Boundary(which < 0, time)
            });
        }
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
        target_speed: f32,
        start: f64,
        end: f64,
        current: f64,
        earliest: f64,
        length: f64,
        combo: (usize, usize),
        auto: bool,
        bookmarks: &[Bookmark],
        stats: &Statistics,
    ) {
        self.actions.clear();
        self.points.clear();
        for button in self.buttons.values_mut() {
            button.invalidate();
        }
        self.scroll.update(t);
        let top = ui.top;
        ui.fill_rect(Rect::new(-0.88, -top + 0.20, 1.76, (top * 2. - 0.25).max(0.2)), Color::new(0.13, 0.17, 0.22, 0.95));
        ui.text(tl!("practice-title"))
            .pos(-0.82, -top + 0.105)
            .anchor(0., 0.5)
            .size(0.5)
            .max_width(0.5)
            .draw();
        self.scroll.size((1.64, (top * 2. - 0.29).max(0.2)));
        let mut scroll = std::mem::take(&mut self.scroll);
        ui.scope(|ui| {
            ui.dx(-0.82);
            ui.dy(-top + 0.23);
            scroll.render(ui, |ui| {
                let mut y = 0.015;
                ui.text(fmt_time(current as f32)).pos(0.82, y).anchor(0.5, 0.).size(0.45).draw();
                y += 0.065;
                let line = Rect::new(0.02, y, 1.6, 0.015);
                ui.fill_rect(line, Color::new(0.3, 0.3, 0.3, 1.));
                let fraction = |value: f64| ((value - earliest) / (length - earliest).max(0.001)).clamp(0., 1.) as f32;
                ui.fill_rect(
                    Rect::new(line.x + line.w * fraction(start), line.y, line.w * (fraction(end) - fraction(start)), line.h),
                    Color::new(1., 1., 1., 0.5),
                );
                for (time, color) in [(start, BLUE), (end, RED)] {
                    let x = line.x + line.w * fraction(time);
                    ui.fill_rect(Rect::new(x - 0.003, line.y - 0.012, 0.006, 0.039), color);
                }
                ui.fill_circle(line.x + line.w * fraction(current), line.center().y, 0.016, WHITE);
                self.timeline = ui.rect_to_global(Rect::new(line.x, line.y - 0.025, line.w, 0.065));
                self.bounds = (earliest, length);
                self.range = (start, end);
                y += 0.065;
                ui.text(tl!("practice-time-range")).pos(0.02, y).size(0.4).draw();
                y += 0.05;
                self.button(
                    ui,
                    t,
                    "exercise_start",
                    format!("{}  {}", tl!("practice-start"), fmt_time(start as f32)),
                    Rect::new(0., y, 0.805, 0.085),
                    false,
                    Action::Input("exercise_start".into(), fmt_time(start as f32)),
                );
                self.button(
                    ui,
                    t,
                    "exercise_end",
                    format!("{}  {}", tl!("practice-end"), fmt_time(end as f32)),
                    Rect::new(0.835, y, 0.805, 0.085),
                    false,
                    Action::Input("exercise_end".into(), fmt_time(end as f32)),
                );
                y += 0.107;
                ui.text(tl!("practice-combo-range")).pos(0.02, y).size(0.4).draw();
                y += 0.05;
                for (id, label, value, x) in [
                    ("exercise_combo_start", tl!("practice-start"), combo.0, 0.),
                    ("exercise_combo_end", tl!("practice-end"), combo.1, 0.835),
                ] {
                    self.button(
                        ui,
                        t,
                        id,
                        format!("{label}  {value}"),
                        Rect::new(x, y, 0.805, 0.085),
                        false,
                        Action::Input(id.into(), value.to_string()),
                    );
                }
                y += 0.107;
                self.number(ui, t, "practice_speed", tl!("speed"), target_speed, 0.01, 0.1, 2., format!("{target_speed:.2}x"), &mut y);
                self.switch(ui, t, "practice_loop", tl!("practice-loop"), auto, &mut y);
                let p = &config.practice;
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
                    for (i, label) in ["Tap", "Hold", "Drag", "Flick"].iter().enumerate() {
                        self.switch(ui, t, &format!("practice_kind_{i}"), label, p.range_kinds[i], &mut y);
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
                    self.button(ui, t, "practice_behavior", label, Rect::new(1., y, 0.64, 0.084), false, Action::Toggle("practice_behavior".into()));
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
        self.scroll = scroll;
    }
}
