use super::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    ui::Ui,
};
use settings::{Category, Field, Kind};

const GAP: f32 = 0.012;
const BODY: f32 = 0.38;
const SMALL: f32 = 0.29;

fn paragraph(ui: &mut Ui, y: &mut f32, width: f32, value: &str, size: f32, color: Color) {
    let rect = ui
        .text(value)
        .pos(0., *y)
        .size(size)
        .no_baseline()
        .max_width(width)
        .multiline()
        .color(color)
        .draw();
    *y = rect.bottom() + GAP;
}

impl RenderPage {
    pub(super) fn render_view(&mut self, ui: &mut Ui, s: &mut SharedState) {
        let t = s.t;
        s.render_fader(ui, |ui| {
            let layout = layout::Layout::new(ui.content_rect());
            ui.fill_path(&layout.outer.rounded(0.012), semi_black(0.55));
            let r = layout.chart;
            let (name, subtitle) = self
                .selected
                .as_ref()
                .map(|v| (v.name.as_str(), v.subtitle.as_str()))
                .unwrap_or(("选择要渲染的谱面", "从谱面库选择，展示文字可在开场分类中单独设置"));
            self.pick.build(ui, t, r, |ui, path| {
                ui.fill_path(&path, semi_black(0.30));
                ui.text(name)
                    .pos(r.x + 0.016, r.y + 0.012)
                    .size(0.41)
                    .no_baseline()
                    .max_width(r.w - 0.20)
                    .draw();
                ui.text(subtitle)
                    .pos(r.x + 0.016, r.y + 0.054)
                    .size(SMALL)
                    .no_baseline()
                    .max_width(r.w - 0.20)
                    .color(semi_white(0.65))
                    .draw();
                ui.text(if self.selected.is_some() { "更换" } else { "选择" })
                    .pos(r.right() - 0.025, r.center().y)
                    .anchor(1., 0.5)
                    .no_baseline()
                    .size(BODY)
                    .draw();
            });
            if self.busy() {
                self.pick.invalidate();
            }
            let tab_w = (layout.tabs.w - GAP * 5.) / 6.;
            for (i, (tab, category)) in self.tabs.iter_mut().zip(settings::CATEGORIES).enumerate() {
                tab.render_text(
                    ui,
                    Rect::new(layout.tabs.x + i as f32 * (tab_w + GAP), layout.tabs.y, tab_w, layout.tabs.h),
                    t,
                    category.label(),
                    0.34,
                    self.category == category,
                );
            }
            for button in &mut self.fields {
                button.invalidate();
            }
            self.encoder_button.invalidate();
            let mut scroll = std::mem::take(&mut self.scroll);
            scroll.size((layout.settings.w, layout.settings.h));
            ui.scope(|ui| {
                ui.dx(layout.settings.x);
                ui.dy(layout.settings.y);
                scroll.render(ui, |ui| self.render_fields(ui, t, layout.settings.w));
            });
            self.scroll = scroll;
            ui.fill_path(&layout.status.rounded(0.006), semi_black(0.20));
            let mut status_scroll = std::mem::take(&mut self.status_scroll);
            status_scroll.size((layout.status.w - 0.024, layout.status.h - 0.016));
            ui.scope(|ui| {
                ui.dx(layout.status.x + 0.012);
                ui.dy(layout.status.y + 0.008);
                status_scroll.render(ui, |ui| self.render_status(ui, layout.status.w - 0.024));
            });
            self.status_scroll = status_scroll;
            self.start
                .render_text(ui, layout.footer[0], t, if self.busy() { "取消渲染" } else { "开始渲染" }, BODY, true);
            self.open.render_text(
                ui,
                layout.footer[1],
                t,
                if cfg!(target_os = "android") {
                    "保存到下载目录"
                } else {
                    "打开输出目录"
                },
                BODY,
                false,
            );
            if cfg!(target_os = "android") {
                self.open.invalidate();
            }
        });
    }

    fn render_fields(&mut self, ui: &mut Ui, t: f32, width: f32) -> (f32, f32) {
        let mut y = 0.;
        let overview = self.category == Category::Overview;
        let count = settings::FIELDS.iter().filter(|f| overview || f.info().0 == self.category).count();
        paragraph(ui, &mut y, width, &format!("{}  /  {} 项", self.category.label(), count), 0.40, WHITE);
        if self.busy() {
            paragraph(ui, &mut y, width, "正在按当前设置渲染，完成或取消后可修改。", SMALL, semi_white(0.65));
        } else if overview {
            paragraph(ui, &mut y, width, "当前所有参数。切换上方分类即可编辑。", SMALL, semi_white(0.65));
        } else if self.category == Category::Flow {
            paragraph(ui, &mut y, width, "开场 → 游玩入场 → 谱面 → 游玩退场 → 结算", SMALL, semi_white(0.65));
        } else if self.category == Category::Ending {
            paragraph(ui, &mut y, width, "头像、玩家信息及自定义统计仅用于视频展示。", SMALL, semi_white(0.65));
        }
        let columns = if width >= 1.35 { 2 } else { 1 };
        let column_w = (width - GAP * (columns - 1) as f32) / columns as f32;
        let mut col = 0;
        let mut row_height = 0f32;
        let mut last_category = None;
        for &field in settings::FIELDS {
            let category = field.info().0;
            if !overview && category != self.category {
                continue;
            }
            if overview && last_category != Some(category) {
                if col > 0 {
                    y += row_height + 0.008;
                    col = 0;
                    row_height = 0.;
                }
                y += GAP;
                paragraph(ui, &mut y, width, category.label(), 0.34, semi_white(0.8));
                last_category = Some(category);
            }
            let height = ui.scope(|ui| {
                ui.dx(col as f32 * (column_w + GAP));
                ui.dy(y);
                self.render_field(ui, t, column_w, field, overview)
            });
            row_height = row_height.max(height);
            col += 1;
            if col == columns {
                y += row_height + 0.008;
                col = 0;
                row_height = 0.;
            }
        }
        if col > 0 {
            y += row_height + 0.008;
        }
        if self.category == Category::Output {
            y += 0.015;
            paragraph(ui, &mut y, width, "视频编码器", BODY, WHITE);
            #[cfg(target_os = "windows")]
            {
                paragraph(ui, &mut y, width, &self.ffmpeg.description(), SMALL, semi_white(0.75));
                if let Some(label) = self.ffmpeg.button_label() {
                    self.encoder_button
                        .render_text(ui, Rect::new(0., y, width.min(0.7), 0.065), t, label, 0.33, false);
                    if self.busy() {
                        self.encoder_button.invalidate();
                    }
                    y += 0.08;
                }
            }
            #[cfg(not(target_os = "windows"))]
            paragraph(
                ui,
                &mut y,
                width,
                if cfg!(target_os = "android") {
                    "已内置 FFmpeg"
                } else {
                    "使用系统 FFmpeg"
                },
                SMALL,
                semi_white(0.65),
            );
        }
        (width, y + 0.02)
    }

    fn render_field(&mut self, ui: &mut Ui, t: f32, width: f32, field: Field, overview: bool) -> f32 {
        let (_, kind, name, help) = field.info();
        let enabled = settings::enabled(&self.prefs, field);
        let value = settings::value(&self.prefs, field);
        let control_w = (width * 0.43).min(0.5);
        let label_w = width - control_w - 0.035;
        let label_h = ui.text(name).size(BODY).no_baseline().max_width(label_w).multiline().measure().h;
        let help_text = if overview { "" } else { help };
        let help_h = if help_text.is_empty() {
            0.
        } else {
            ui.text(help_text).size(SMALL).no_baseline().max_width(label_w).multiline().measure().h + 0.007
        };
        let value_h = if overview {
            ui.text(&value)
                .size(0.31)
                .no_baseline()
                .max_width(control_w - 0.02)
                .multiline()
                .measure()
                .h
        } else {
            0.
        };
        let height = (label_h + help_h + 0.026).max(value_h + 0.026).max(0.082);
        let row = Rect::new(0., 0., width, height);
        ui.fill_path(&row.rounded(0.006), semi_black(0.22));
        let alpha = if enabled { 1. } else { 0.45 };
        ui.text(name)
            .pos(0.012, 0.012)
            .size(BODY)
            .no_baseline()
            .max_width(label_w)
            .multiline()
            .color(semi_white(alpha))
            .draw();
        if !help_text.is_empty() {
            ui.text(help_text)
                .pos(0.012, 0.019 + label_h)
                .size(SMALL)
                .no_baseline()
                .max_width(label_w)
                .multiline()
                .color(semi_white(0.57 * alpha))
                .draw();
        }
        let control = Rect::new(width - control_w - 0.01, height / 2. - 0.029, control_w, 0.058);
        if overview {
            ui.text(&value)
                .pos(control.x + 0.01, height / 2.)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.31)
                .max_width(control.w - 0.02)
                .multiline()
                .color(semi_white(0.8 * alpha))
                .draw();
        } else {
            let busy = self.busy();
            let button = &mut self.fields[field as usize];
            let caption = if kind == Kind::Folder { "更改目录" } else { &value };
            let chosen = kind == Kind::Toggle && value == "开启";
            ui.alpha(if busy { 0.5 } else { alpha }, |ui| button.render_text(ui, control, t, caption, 0.34, chosen));
            if !enabled || busy {
                button.invalidate();
            }
        }
        let mut end = height + 0.005;
        if field == Field::Directory && !overview {
            paragraph(ui, &mut end, width - 0.015, &value, SMALL, semi_white(0.60));
        }
        end
    }

    fn render_status(&mut self, ui: &mut Ui, width: f32) -> (f32, f32) {
        let mut y = 0.;
        match &self.job {
            Job::Idle => paragraph(ui, &mut y, width, "选择谱面和画面预设后开始导出。视频按固定帧率渲染，过程不播放声音。", SMALL, semi_white(0.65)),
            Job::Loading(_) => paragraph(ui, &mut y, width, "正在加载谱面并合成视频音轨…", BODY, WHITE),
            Job::Rendering { renderer, started } => {
                let (frame, total) = renderer.progress();
                let p = if total > 0 { frame as f32 / total as f32 } else { 0. }.clamp(0., 1.);
                let elapsed = started.elapsed().as_secs_f64();
                let eta = if frame > 0 {
                    fmt_duration(elapsed / frame as f64 * total.saturating_sub(frame) as f64)
                } else {
                    "计算中".into()
                };
                paragraph(
                    ui,
                    &mut y,
                    width,
                    &format!("{}   {:.1}%   {frame}/{total} 帧   预计剩余 {eta}", renderer.stage().map_or("渲染中", |stage| stage.label()), p * 100.),
                    SMALL,
                    WHITE,
                );
                let bar = Rect::new(0., y, width, 0.008);
                ui.fill_rect(bar, semi_white(0.15));
                if p > 0. {
                    ui.fill_rect(Rect::new(0., y, width * p, bar.h), WHITE);
                }
                y += 0.02;
            }
            Job::Done { path, frames, secs } => {
                paragraph(ui, &mut y, width, &format!("完成 {frames} 帧，用时 {}", fmt_duration(*secs)), SMALL, WHITE);
                paragraph(ui, &mut y, width, path, SMALL, semi_white(0.65));
            }
            Job::Failed(err) => paragraph(ui, &mut y, width, &format!("渲染失败：{err}"), SMALL, Color::new(1., 0.6, 0.55, 1.)),
        }
        (width, y + 0.005)
    }
}
