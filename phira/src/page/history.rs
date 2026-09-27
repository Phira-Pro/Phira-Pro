prpr_l10n::tl_file!("history");

// 成绩历史页：列表 / 趋势图 / PB 对比 / 判定分布对比 / 筛选 / 导入导出。
// 移植自上游改版 Phirc Mod++ 的 `phira/src/page/history.rs`（未引入每日挑战）。
// （注：模块文档注释不能写成 //! —— tl_file! 展开后必须是文件开头）

use super::{Page, SharedState};
use crate::history::{self, Record};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    scene::{request_file, show_error, show_message, take_file},
    ui::{DRectButton, RectButton, Scroll, Ui},
};
use std::{borrow::Cow, io::Write};

const ROW_H: f32 = 0.082;
const GAP: f32 = 0.014;
const CARD_BG: f32 = 0.34;

/// 分数对应的段位色（列表左侧色条 / 分数本身 / 趋势折线）
fn score_color(score: u32) -> Color {
    match score {
        s if s >= 1_000_000 => Color::new(1.00, 0.86, 0.42, 1.),
        s if s >= 960_000 => Color::new(0.96, 0.62, 1.00, 1.),
        s if s >= 920_000 => Color::new(0.58, 0.78, 1.00, 1.),
        s if s >= 880_000 => Color::new(0.58, 0.96, 0.72, 1.),
        s if s >= 820_000 => Color::new(0.98, 0.92, 0.58, 1.),
        _ => Color::new(0.72, 0.72, 0.76, 1.),
    }
}

/// 画一张圆角卡片，返回内容区
fn card(ui: &mut Ui, r: Rect) -> Rect {
    ui.fill_path(&r.rounded(0.012), semi_black(CARD_BG));
    ui.stroke_path(&r.rounded(0.012), 0.0015, semi_white(0.10));
    r.feather(-0.018)
}

pub struct HistoryPage {
    records: Vec<Record>,
    /// `None` = 全部谱面；`Some(key)` = 只看这一首
    selected: Option<String>,
    only_fc: bool,
    scroll: Scroll,
    btn_all: DRectButton,
    btn_fc: DRectButton,
    btn_export: DRectButton,
    btn_import: DRectButton,
    import_waiting: bool,
    /// 每行一个按钮，用来点选「只看这首」（渲染时 set，touch 时用）
    row_btns: Vec<RectButton>,
}

impl HistoryPage {
    pub fn new() -> Self {
        history::ensure_loaded();
        Self {
            records: history::all(),
            selected: None,
            only_fc: false,
            scroll: Scroll::new(),
            btn_all: DRectButton::new().with_radius(0.008),
            btn_fc: DRectButton::new().with_radius(0.008),
            btn_export: DRectButton::new().with_radius(0.008),
            btn_import: DRectButton::new().with_radius(0.008),
            import_waiting: false,
            row_btns: Vec::new(),
        }
    }

    fn reload(&mut self) {
        self.records = history::all();
    }

    /// 当前筛选后的记录（按时间从旧到新，方便画趋势）。
    fn filtered(&self) -> Vec<Record> {
        let mut v: Vec<Record> = match &self.selected {
            Some(key) => self.records.iter().filter(|it| &it.key == key).cloned().collect(),
            None => self.records.clone(),
        };
        if self.only_fc {
            v.retain(|it| it.is_full_combo());
        }
        v.sort_by_key(|it| it.time);
        v
    }

    fn start_export(&mut self) {
        match history::export_json() {
            Ok(text) => {
                super::library::request_export("phira-history.json".to_owned());
                // 系统弹窗是异步的：先把文本放内存，等 take_export 拿到文件再写
                PENDING_EXPORT.with(|it| *it.borrow_mut() = Some(text));
            }
            Err(err) => show_error(err.context(tl!("export-failed"))),
        }
    }

    fn poll_import(&mut self) {
        if !self.import_waiting {
            return;
        }
        let Some(file) = take_file() else {
            return;
        };
        self.import_waiting = false;
        let (_, path) = file;
        if path.is_empty() {
            return;
        }
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| history::import_json(&text))
        {
            Ok(n) => {
                self.reload();
                show_message(tl!("import-done", "count" => n.to_string()));
            }
            Err(err) => show_error(err.context(tl!("import-failed"))),
        }
    }

    fn poll_export_write() {
        PENDING_EXPORT.with(|slot| {
            if slot.borrow().is_none() {
                return;
            }
            // 系统「保存到哪」对话框是异步的：拿到文件之前不能把文本丢掉
            if let Some(Ok(mut config)) = super::library::take_export() {
                let text = slot.borrow_mut().take().unwrap_or_default();
                let res = config.file.write_all(text.as_bytes()).map_err(anyhow::Error::from);
                super::library::resolve_export();
                if let Err(err) = res {
                    show_error(err.context(tl!("export-failed")));
                } else {
                    show_message(tl!("export-done"));
                }
            }
        });
    }
}

thread_local! {
    static PENDING_EXPORT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

impl Page for HistoryPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        self.poll_import();
        Self::poll_export_write();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.btn_all.touch(touch, t) {
            self.selected = None;
            return Ok(true);
        }
        if self.btn_fc.touch(touch, t) {
            self.only_fc ^= true;
            return Ok(true);
        }
        if self.btn_export.touch(touch, t) {
            self.start_export();
            return Ok(true);
        }
        if self.btn_import.touch(touch, t) {
            self.import_waiting = true;
            request_file("history_import");
            return Ok(true);
        }
        // 点某一行 = 只看这一首（再点一次取消）；顺序要和渲染时一致
        let keys: Vec<String> = self.filtered().iter().rev().take(300).map(|it| it.key.clone()).collect();
        for (i, btn) in self.row_btns.iter_mut().enumerate() {
            if btn.touch(touch) {
                if let Some(key) = keys.get(i) {
                    if self.selected.as_deref() == Some(key.as_str()) {
                        self.selected = None;
                    } else {
                        self.selected = Some(key.clone());
                    }
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let mut cr = ui.content_rect();
        cr.x += 0.025;
        cr.w -= 0.025;
        let records = self.filtered();
        let selected_name = self
            .selected
            .as_ref()
            .and_then(|key| self.records.iter().find(|it| &it.key == key))
            .map(|it| it.name.clone());

        s.render_fader(ui, |ui| {
            let outer = cr.feather(-0.005);
            self.scroll.size((outer.w, outer.h));
            ui.dx(outer.x);
            ui.dy(outer.y);
            self.scroll.render(ui, |ui| {
                let w = outer.w;
                let mut y = 0.;
                let row = |y: f32, h: f32| Rect::new(0., y, w, h);

                // ---------------- 顶部：筛选 + 导入导出 ----------------
                let bh = 0.085;
                let bw = (w - GAP * 3.) / 4.;
                let mut rb = Rect::new(0., y, bw, bh);
                self.btn_all
                    .render_text(ui, rb, t, tl!("filter-all"), 0.42, self.selected.is_none() && !self.only_fc);
                rb.x += bw + GAP;
                self.btn_fc.render_text(ui, rb, t, tl!("filter-fc"), 0.42, self.only_fc);
                rb.x += bw + GAP;
                self.btn_export.render_text(ui, rb, t, tl!("export"), 0.4, false);
                rb.x += bw + GAP;
                self.btn_import
                    .render_text(ui, rb, t, if self.import_waiting { tl!("import-waiting") } else { tl!("import") }, 0.4, false);
                y += bh + GAP;

                // ---------------- 标题 ----------------
                let title = match &selected_name {
                    Some(name) => name.clone(),
                    None => tl!("stat-all-title").to_string(),
                };
                ui.text(&title).pos(0.004, y).anchor(0., 0.).size(0.115).color(WHITE).draw();
                if !records.is_empty() {
                    ui.text(tl!("stat-count", "count" => records.len().to_string()))
                        .pos(w - 0.004, y + 0.025)
                        .anchor(1., 0.)
                        .size(0.06)
                        .color(semi_white(0.55))
                        .draw();
                }
                y += 0.12;

                if records.is_empty() {
                    let r = row(y, 0.34);
                    let inner = card(ui, r);
                    ui.text(tl!("empty"))
                        .pos(inner.center().x, inner.center().y)
                        .anchor(0.5, 0.5)
                        .size(0.085)
                        .color(semi_white(0.6))
                        .draw();
                    return (w, y + 0.34 + GAP);
                }

                let pb = records.iter().max_by_key(|it| it.score).cloned().unwrap_or_default();
                let latest = records.last().cloned().unwrap_or_default();
                let avg_score = records.iter().map(|it| it.score as f64).sum::<f64>() / records.len() as f64;
                let avg_acc = records.iter().map(|it| it.accuracy).sum::<f64>() / records.len() as f64;

                // ---------------- 三张统计卡 ----------------
                let sh = 0.155;
                let sw = (w - GAP * 2.) / 3.;
                let stat = |ui: &mut Ui, x: f32, label: Cow<'static, str>, value: String, value_color: Color, extra: String| {
                    let r = Rect::new(x, y, sw, sh);
                    let inner = card(ui, r);
                    ui.text(label)
                        .pos(inner.x, inner.y)
                        .anchor(0., 0.)
                        .size(0.062)
                        .color(semi_white(0.5))
                        .draw();
                    ui.text(value)
                        .pos(inner.x, inner.y + 0.045)
                        .anchor(0., 0.)
                        .size(0.115)
                        .color(value_color)
                        .draw();
                    ui.text(extra)
                        .pos(inner.x, inner.y + 0.128)
                        .anchor(0., 0.)
                        .size(0.06)
                        .color(semi_white(0.65))
                        .draw();
                };
                stat(
                    ui,
                    0.,
                    tl!("stat-latest"),
                    format!("{:07}", latest.score),
                    score_color(latest.score),
                    format!("{:.2}%{}", latest.accuracy * 100., if latest.is_full_combo() { "  FC" } else { "" }),
                );
                stat(
                    ui,
                    sw + GAP,
                    tl!("stat-pb"),
                    format!("{:07}", pb.score),
                    score_color(pb.score),
                    format!("{:.2}%{}", pb.accuracy * 100., if pb.is_full_combo() { "  FC" } else { "" }),
                );
                stat(ui, (sw + GAP) * 2., tl!("stat-avg"), format!("{avg_score:.0}"), semi_white(0.9), format!("{:.2}%", avg_acc * 100.));
                y += sh + GAP;

                // ---------------- 趋势图 ----------------
                let ch = 0.34;
                let inner = card(ui, row(y, ch));
                ui.text(tl!("trend-title"))
                    .pos(inner.x, inner.y)
                    .anchor(0., 0.)
                    .size(0.062)
                    .color(semi_white(0.5))
                    .draw();
                let plot = Rect::new(inner.x, inner.y + 0.05, inner.w, inner.h - 0.075);
                for i in 0..=4 {
                    let gy = plot.y + plot.h * i as f32 / 4.;
                    ui.fill_rect(Rect::new(plot.x, gy, plot.w, 0.0012), semi_white(0.08));
                }
                let pb_y = plot.bottom() - plot.h * (pb.score as f32 / 1_000_000.).clamp(0., 1.);
                ui.fill_rect(Rect::new(plot.x, pb_y, plot.w, 0.0022), Color::new(1., 0.86, 0.42, 0.5));
                ui.text(format!("PB {}", pb.score))
                    .pos(plot.right(), pb_y - 0.006)
                    .anchor(1., 1.)
                    .size(0.055)
                    .color(Color::new(1., 0.86, 0.42, 0.95))
                    .draw();
                // 折线本体：用细矩形拼（Ui 的 dx/dy 偏移对 raw draw_line 不生效）
                let n = records.len();
                let mut prev: Option<(f32, f32)> = None;
                for (i, rec) in records.iter().enumerate() {
                    let x = if n == 1 {
                        plot.center().x
                    } else {
                        plot.x + plot.w * i as f32 / (n - 1) as f32
                    };
                    let v = (rec.score as f32 / 1_000_000.).clamp(0., 1.);
                    let yv = plot.bottom() - plot.h * v;
                    if let Some((px, py)) = prev {
                        let steps = (((x - px).abs() / 0.0035).ceil() as i32).max(1);
                        for k in 0..=steps {
                            let p = k as f32 / steps as f32;
                            let cx = px + (x - px) * p;
                            let cy = py + (yv - py) * p;
                            ui.fill_rect(Rect::new(cx, cy - 0.0013, 0.0035, 0.0026), score_color(rec.score));
                        }
                    }
                    ui.fill_circle(x, yv, 0.0042, score_color(rec.score));
                    prev = Some((x, yv));
                }
                ui.text(tl!("trend-axis"))
                    .pos(plot.x, plot.bottom() + 0.008)
                    .anchor(0., 0.)
                    .size(0.05)
                    .color(semi_white(0.4))
                    .draw();
                y += ch + GAP;

                // ---------------- 判定分布对比（只看单曲时）----------------
                if self.selected.is_some() && (!pb.hist.is_empty() || !latest.hist.is_empty()) {
                    let dh = 0.26;
                    let inner = card(ui, row(y, dh));
                    ui.text(tl!("dist-title"))
                        .pos(inner.x, inner.y)
                        .anchor(0., 0.)
                        .size(0.062)
                        .color(semi_white(0.5))
                        .draw();
                    ui.text(tl!("dist-legend"))
                        .pos(inner.right(), inner.y)
                        .anchor(1., 0.)
                        .size(0.055)
                        .color(semi_white(0.4))
                        .draw();
                    let plot = Rect::new(inner.x, inner.y + 0.05, inner.w, inner.h - 0.075);
                    let bars = latest.hist.len().max(pb.hist.len()).max(1);
                    let maxv = latest.hist.iter().chain(pb.hist.iter()).copied().max().unwrap_or(0).max(1) as f32;
                    let bw = plot.w / bars as f32;
                    for i in 0..bars {
                        let lv = latest.hist.get(i).copied().unwrap_or(0) as f32;
                        let bv = pb.hist.get(i).copied().unwrap_or(0) as f32;
                        let bh2 = plot.h * (bv / maxv);
                        ui.fill_rect(Rect::new(plot.x + bw * i as f32, plot.bottom() - bh2, bw * 0.78, bh2), semi_white(0.22));
                        let lh = plot.h * (lv / maxv);
                        ui.fill_rect(Rect::new(plot.x + bw * i as f32, plot.bottom() - lh, bw * 0.78, lh), Color::new(0.42, 0.74, 1., 0.9));
                    }
                    ui.fill_rect(Rect::new(plot.center().x, plot.y, 0.0012, plot.h), semi_white(0.28));
                    y += dh + GAP;
                }

                // ---------------- 列表 ----------------
                ui.text(tl!("list-title"))
                    .pos(0.004, y)
                    .anchor(0., 0.)
                    .size(0.062)
                    .color(semi_white(0.5))
                    .draw();
                y += 0.062;
                let visible: Vec<Record> = records.iter().rev().take(300).cloned().collect();
                while self.row_btns.len() < visible.len() {
                    self.row_btns.push(RectButton::new());
                }
                for (i, rec) in visible.iter().enumerate() {
                    let r = row(y, ROW_H);
                    self.row_btns[i].set(ui, r);
                    let is_selected = self.selected.as_deref() == Some(rec.key.as_str());
                    ui.fill_path(&r.rounded(0.010), if is_selected { semi_white(0.10) } else { semi_black(0.30) });
                    // 左侧段位色条
                    ui.fill_path(&Rect::new(r.x + 0.006, r.y + 0.013, 0.007, r.h - 0.026).rounded(0.003), score_color(rec.score));
                    let ix = r.x + 0.026;
                    let cy = r.center().y;
                    ui.text(format!("{:07}", rec.score))
                        .pos(ix, cy)
                        .anchor(0., 0.5)
                        .size(0.085)
                        .color(score_color(rec.score))
                        .no_baseline()
                        .draw();
                    ui.text(format!("{:.2}%", rec.accuracy * 100.))
                        .pos(ix + r.w * 0.17, cy)
                        .anchor(0., 0.5)
                        .size(0.07)
                        .color(semi_white(0.88))
                        .no_baseline()
                        .draw();
                    ui.text(format!("P{} G{} B{} M{}", rec.counts[0], rec.counts[1], rec.counts[2], rec.counts[3]))
                        .pos(ix + r.w * 0.29, cy)
                        .anchor(0., 0.5)
                        .size(0.06)
                        .color(semi_white(0.7))
                        .no_baseline()
                        .draw();
                    ui.text(format!("x{}", rec.max_combo))
                        .pos(ix + r.w * 0.50, cy)
                        .anchor(0., 0.5)
                        .size(0.06)
                        .color(semi_white(0.7))
                        .no_baseline()
                        .draw();
                    if rec.is_full_combo() {
                        let badge = Rect::new(ix + r.w * 0.585, cy - 0.019, 0.055, 0.038);
                        ui.fill_path(&badge.rounded(0.009), Color::new(0.42, 0.74, 1., 0.22));
                        ui.text("FC")
                            .pos(badge.center().x, cy)
                            .anchor(0.5, 0.5)
                            .size(0.055)
                            .color(Color::new(0.6, 0.85, 1., 1.))
                            .no_baseline()
                            .draw();
                    }
                    ui.text(format!("{} {:.1}", rec.level, rec.difficulty))
                        .pos(ix + r.w * 0.68, cy)
                        .anchor(0., 0.5)
                        .size(0.06)
                        .color(semi_white(0.62))
                        .no_baseline()
                        .draw();
                    ui.text(rec.time_text())
                        .pos(r.right() - 0.012, cy)
                        .anchor(1., 0.5)
                        .size(0.055)
                        .color(semi_white(0.5))
                        .no_baseline()
                        .draw();
                    y += ROW_H + 0.008;
                }
                if visible.len() < records.len() {
                    ui.text(tl!("list-more", "count" => (records.len() - visible.len()).to_string()))
                        .pos(0.004, y + 0.012)
                        .anchor(0., 0.)
                        .size(0.055)
                        .color(semi_white(0.4))
                        .draw();
                    y += 0.05;
                }
                (w, y + GAP)
            });
        });
        Ok(())
    }
}
