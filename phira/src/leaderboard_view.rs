//! Song leaderboard presentation, kept separate from the event leaderboard.
prpr_l10n::tl_file!("song" board_tl);

use macroquad::prelude::*;
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    ext::{semi_white, RectExt, SafeTexture},
    ui::{DRectButton, RectButton, Ui},
};

#[derive(Clone, Copy)]
pub enum Action {
    Source(usize),
    Metric(u8),
    Refresh,
}

pub struct Controls {
    pub sources: [DRectButton; 3],
    pub metrics: [DRectButton; 3],
    pub refresh: DRectButton,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            sources: std::array::from_fn(|_| DRectButton::new()),
            metrics: std::array::from_fn(|_| DRectButton::new()),
            refresh: DRectButton::new(),
        }
    }
}

impl Controls {
    pub fn touch(&mut self, touch: &Touch, t: f32) -> Option<Action> {
        if self.refresh.touch(touch, t) {
            return Some(Action::Refresh);
        }
        for (i, btn) in self.sources.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                return Some(Action::Source(i));
            }
        }
        for (i, btn) in self.metrics.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                return Some(Action::Metric(i as u8));
            }
        }
        None
    }
}

pub struct Layout {
    pub sources_y: f32,
    pub metric_y: f32,
    pub ranks_y: f32,
    pub note_y: f32,
    pub list_y: f32,
    pub row_height: f32,
}

impl Layout {
    pub fn new(height: f32, local: bool) -> Self {
        let compact = height < 1.;
        let sources_y = if compact { 0.105 } else { 0.13 };
        let metric_y = sources_y + 0.09;
        let ranks_y = metric_y + 0.07;
        let note_y = ranks_y + if local { 0.06 } else { 0.15 };
        Self {
            sources_y,
            metric_y,
            ranks_y,
            note_y,
            list_y: note_y + 0.055,
            row_height: if compact { 0.115 } else { 0.135 },
        }
    }
}

fn label(ui: &mut Ui, text: &str, x: f32, y: f32, size: f32, width: f32, color: Color) {
    ui.text(text)
        .pos(x, y)
        .anchor(0., 0.5)
        .no_baseline()
        .size(size)
        .max_width(width)
        .color(color)
        .draw();
}

fn tab(ui: &mut Ui, btn: &mut DRectButton, r: Rect, t: f32, text: &str, active: bool, primary: bool) {
    let accent = ui.accent();
    btn.build(ui, t, r, |ui, path| {
        ui.fill_path(
            &path,
            if active {
                Color {
                    a: if primary { 0.25 } else { 0.12 },
                    ..accent
                }
            } else {
                semi_white(0.045)
            },
        );
        if active {
            ui.fill_rect(Rect::new(r.x + 0.018, r.bottom() - 0.005, r.w - 0.036, 0.003), accent);
        }
        ui.text(text)
            .pos(r.center().x, r.center().y)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(if primary { 0.46 } else { 0.37 })
            .max_width(r.w - 0.025)
            .color(if active { WHITE } else { semi_white(0.65) })
            .draw();
    });
}

#[allow(clippy::too_many_arguments)]
pub fn header(ui: &mut Ui, width: f32, height: f32, chart: &str, source: usize, metric: u8, t: f32, controls: &mut Controls) -> Layout {
    let local = source == 2;
    let layout = Layout::new(height, local);
    let tint = Color::new(0.035, 0.04, 0.05, 0.5);
    let opacity = |p: f32| tint.a * p * p * (3. - 2. * p);
    // Feather across the panel boundary instead of drawing a separate edge or shadow.
    for i in 0..16 {
        let start = i as f32 / 16.;
        let end = (i + 1) as f32 / 16.;
        let left = -0.16 + start * 0.32;
        let right = -0.16 + end * 0.32;
        ui.fill_rect(
            Rect::new(left, 0., right - left, height),
            (Color { a: opacity(start), ..tint }, (left, 0.), Color { a: opacity(end), ..tint }, (right, 0.)),
        );
    }
    ui.fill_rect(Rect::new(0.16, 0., width - 0.16, height), tint);
    label(ui, &board_tl!("ldb"), 0.035, 0.058, 0.82, width - 0.26, WHITE);
    if height >= 1. {
        label(ui, chart, 0.037, 0.103, 0.32, width - 0.09, semi_white(0.5));
    }
    tab(ui, &mut controls.refresh, Rect::new(width - 0.19, 0.025, 0.15, 0.062), t, &board_tl!("ldb-refresh"), false, false);
    let w = (width - 0.09) / 3.;
    for (i, key) in ["ldb-pro", "ldb-mixed", "ldb-local-title"].iter().enumerate() {
        tab(
            ui,
            &mut controls.sources[i],
            Rect::new(0.035 + i as f32 * (w + 0.01), layout.sources_y, w, 0.075),
            t,
            &board_tl!(*key),
            source == i,
            true,
        );
    }
    if local {
        for btn in &mut controls.metrics {
            btn.invalidate();
        }
    } else {
        for (i, key) in ["ldb-score", "ldb-std", "ldb-acc"].iter().enumerate() {
            tab(
                ui,
                &mut controls.metrics[i],
                Rect::new(0.035 + i as f32 * (w + 0.01), layout.metric_y, w, 0.055),
                t,
                &board_tl!(*key),
                metric == i as u8,
                false,
            );
        }
    }
    layout
}

pub fn rank_lines(ui: &mut Ui, r: Rect, heading: &str, official: &str, pro: &str) {
    let accent = ui.accent();
    ui.fill_path(&r.rounded(0.014), semi_white(0.055));
    let compact = r.h < 0.12;
    let top = if compact { 0. } else { 0.032 };
    if !compact {
        label(ui, heading, r.x + 0.02, r.y + 0.024, 0.32, r.w - 0.04, semi_white(0.55));
    }
    for (i, (key, value)) in [("ldb-official-server", official), ("ldb-pro-server", pro)].iter().enumerate() {
        let y = r.y + top + (i as f32 + 0.5) * (r.h - top) / 2.;
        label(ui, &board_tl!(*key), r.x + 0.02, y, 0.38, r.w * 0.4, semi_white(0.75));
        ui.text(*value)
            .pos(r.right() - 0.02, y)
            .anchor(1., 0.5)
            .no_baseline()
            .size(0.42)
            .max_width(r.w * 0.55)
            .color(if value.starts_with('#') { accent } else { semi_white(0.55) })
            .draw();
    }
}

pub fn note(ui: &mut Ui, width: f32, y: f32, text: &str) {
    label(ui, text, 0.04, y + 0.016, 0.30, width - 0.08, semi_white(0.55));
}

pub fn empty(ui: &mut Ui, width: f32, height: f32, text: &str, loading: bool, t: f32) {
    if loading {
        ui.loading(width / 2., height / 2. - 0.04, t, ui.accent(), ());
    }
    ui.text(text)
        .pos(width / 2., height / 2. + 0.035)
        .anchor(0.5, 0.5)
        .no_baseline()
        .size(0.42)
        .max_width(width - 0.12)
        .color(semi_white(0.6))
        .draw();
}

pub struct Row<'a> {
    pub rank: Option<u32>,
    pub name: String,
    pub name_color: Color,
    pub avatar: Result<Option<SafeTexture>, SafeTexture>,
    pub metric: String,
    pub detail: String,
    pub source: String,
    pub pro: bool,
    pub me: bool,
    pub btn: &'a mut RectButton,
}

pub fn row(ui: &mut Ui, width: f32, height: f32, t: f32, item: Row<'_>) {
    let r = Rect::new(0.005, 0.004, width - 0.01, height - 0.012);
    let accent = ui.accent();
    ui.fill_path(&r.rounded(0.013), if item.me { Color { a: 0.13, ..accent } } else { semi_white(0.035) });
    let avatar_x = if item.rank.is_some() { 0.125 } else { 0.058 };
    if let Some(rank) = item.rank {
        ui.text(format!("#{rank}"))
            .pos(0.052, height / 2.)
            .anchor(0.5, 0.5)
            .no_baseline()
            .size(0.4)
            .max_width(0.085)
            .color(if rank <= 3 { accent } else { semi_white(0.55) })
            .draw_using(&PGR_FONT);
    }
    ui.avatar(avatar_x, height / 2. - 0.004, 0.033, t, item.avatar);
    item.btn.set(ui, r);
    let x = avatar_x + 0.05;
    let right = width - 0.025;
    let metric = ui
        .text(&item.metric)
        .pos(right, height * 0.34)
        .anchor(1., 0.5)
        .no_baseline()
        .size(0.68)
        .max_width(width * 0.43)
        .draw_using(&PGR_FONT);
    label(ui, &item.name, x, height * 0.32, 0.46, (metric.x - x - 0.025).max(0.02), item.name_color);
    let badge_width = (width * 0.17).min(0.16);
    let badge = Rect::new(x, height * 0.62, badge_width, 0.034);
    let color = if item.pro { accent } else { semi_white(0.55) };
    ui.fill_path(&badge.rounded(0.006), Color { a: 0.11, ..color });
    label(ui, &item.source, badge.x + 0.01, badge.center().y, 0.27, badge.w - 0.02, color);
    ui.text(&item.detail)
        .pos(right, height * 0.75)
        .anchor(1., 0.5)
        .no_baseline()
        .size(0.32)
        .max_width((right - badge.right() - 0.025).max(0.02))
        .color(semi_white(0.55))
        .draw_using(&BOLD_FONT);
}
