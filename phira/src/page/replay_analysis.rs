//! PRCA selection, independent worker state, and a modal result viewer.
use crate::{
    plugins::replay_analysis::{self, Analyzer},
    replay::{
        self,
        analysis::{worker::Work, Control, Job, Output},
        library::{self, Item},
    },
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{RectExt, SafeTexture, ScaleType},
    scene::{request_file, return_file, show_error, show_message, take_file, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, RectButton, Scroll, Ui},
};
use std::path::PathBuf;
mod album;
mod preview;
#[cfg(all(test, target_os = "windows"))]
mod tests;
pub struct ReplayAnalysisScene {
    analyzers: Vec<Analyzer>,
    analyzer: usize,
    selected: Option<PathBuf>,
    items: Vec<Item>,
    buttons: Vec<DRectButton>,
    rows: Vec<RectButton>,
    scroll: Scroll,
    task: Option<Work<Output>>,
    control: Control,
    import_task: Option<Work<Vec<(PathBuf, bool)>>>,
    output: Option<Output>,
    preview: Option<preview::Preview>,
    save: Option<album::Save>,
    waiting: bool,
    status: String,
    next: Option<NextScene>,
    back_icon: Option<SafeTexture>,
}
impl ReplayAnalysisScene {
    pub fn new(selected: Option<PathBuf>) -> Result<Self> {
        let analyzers = replay_analysis::available();
        anyhow::ensure!(!analyzers.is_empty(), "请先在插件管理安装并启用 PRCA");
        let analyzer = analyzers.iter().position(|a| a.id == "phira-pro.prca").unwrap_or(0);
        Ok(Self {
            analyzers,
            analyzer,
            selected,
            items: library::all()?,
            buttons: (0..12).map(|_| DRectButton::new()).collect(),
            rows: vec![],
            scroll: Scroll::new(),
            task: None,
            control: Control::default(),
            import_task: None,
            output: None,
            preview: None,
            save: None,
            waiting: false,
            status: "选择一份本地回放，再点击开始分析；导入文件后会自动分析。".into(),
            next: None,
            back_icon: None,
        })
    }
    fn start(&mut self) -> Result<()> {
        let path = self.selected.clone().context("请先选择回放")?;
        self.control.cancel();
        self.output = None;
        self.preview = None;
        let active = self.analyzers[self.analyzer].clone();
        anyhow::ensure!(active.current(), "插件已变更，请返回后重新打开 PRCA");
        self.control = Control::default();
        let preferred = self
            .items
            .iter()
            .find(|i| i.path == path)
            .and_then(|i| i.binding.as_ref())
            .and_then(|b| b.local_path.clone());
        let job = Job {
            path,
            preferred,
            candidates: crate::get_data().charts.iter().map(|c| c.local_path.clone()).collect(),
            analyzer: active,
            destination: PathBuf::from(crate::dir::root()?).join("replays/analysis"),
            control: self.control.clone(),
        };
        self.status = "正在分析".into();
        match Work::spawn("prca-analysis", move || job.execute()) {
            Ok(work) => self.task = Some(work),
            Err(e) => {
                self.status = e.to_string();
                return Err(e);
            }
        }
        Ok(())
    }
    fn cancel(&mut self) {
        self.control.cancel();
        self.task = None;
        self.status = "分析已取消".into();
    }
}
impl Drop for ReplayAnalysisScene {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl Scene for ReplayAnalysisScene {
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(p) = &mut self.preview {
            p.update();
        } else {
            self.scroll.update(tm.real_time() as f32);
        }
        replay::poll_export();
        if let Some(result) = self.save.as_mut().and_then(album::Save::take) {
            self.save = None;
            match result {
                Ok(message) => {
                    self.status = message.clone();
                    show_message(message).ok();
                }
                Err(e) => show_error(e),
            }
        }
        if self.task.is_some() {
            if !self.analyzers[self.analyzer].current() {
                self.cancel();
                self.status = "插件已变更，旧分析已取消，请返回后重新打开。".into();
            } else if let Err(e) = self.control.check() {
                self.task = None;
                self.control.cancel();
                self.status = format!("分析未完成：{e}");
            }
        }
        if self.waiting {
            if let Some((id, path)) = take_file() {
                if id == "prca_replay_import" {
                    self.waiting = false;
                    if !path.is_empty() {
                        self.status = "正在导入回放".into();
                        self.import_task = Some(Work::spawn("prca-import", move || library::import_package(std::path::Path::new(&path)))?);
                    }
                } else {
                    return_file(id, path);
                }
            }
        }
        if let Some(result) = self.import_task.as_mut().and_then(Work::take) {
            self.import_task = None;
            match result {
                Ok(imported) => {
                    self.items = library::all()?;
                    if imported.len() == 1 {
                        self.selected = Some(imported[0].0.clone());
                        self.start()?;
                    } else {
                        self.status = "已导入多个回放，请选择一份进行分析。".into();
                    }
                }
                Err(e) => {
                    self.status = format!("导入失败：{e:#}");
                    show_error(e);
                }
            }
        }
        if let Some(result) = self.task.as_mut().and_then(Work::take) {
            self.task = None;
            match result {
                Ok(mut output) => {
                    self.status = "分析完成".into();
                    self.preview = Some(preview::Preview::new(output.png.clone(), std::mem::take(&mut output.image)));
                    self.output = Some(output);
                }
                Err(e) => {
                    self.status = format!("分析未完成：{e:#}");
                    show_error(e);
                }
            }
        }
        Ok(())
    }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if let Some(p) = &mut self.preview {
            if p.touch(touch, t, self.save.is_some()) {
                match album::Save::start(p.path.clone()) {
                    Ok(save) => self.save = Some(save),
                    Err(e) => show_error(e),
                }
            }
            if p.closed {
                self.preview = None;
            }
            return Ok(true);
        }
        if self.buttons[0].touch(touch, t) {
            self.cancel();
            self.next = Some(NextScene::Pop);
            return Ok(true);
        }
        if self.buttons[4].touch(touch, t) {
            if self.task.is_some() {
                self.cancel();
            } else if self.import_task.is_none() && !self.waiting {
                if self.selected.is_some() {
                    self.start()?;
                } else {
                    show_message("请先选择一份回放").warn();
                }
            }
            return Ok(true);
        }
        if self.task.is_some() || self.import_task.is_some() || self.waiting {
            return Ok(true);
        }
        if self.buttons[1].touch(touch, t) {
            self.items = library::all()?;
            self.scroll.y_scroller.reset();
            return Ok(true);
        }
        if self.buttons[2].touch(touch, t) {
            self.waiting = true;
            request_file("prca_replay_import");
            return Ok(true);
        }
        if self.buttons[11].touch(touch, t) {
            if let Some(o) = &self.output {
                self.preview = Some(preview::Preview::new(o.png.clone(), image::open(&o.png)?.into_rgba8()));
            }
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            for row in &mut self.rows {
                row.cancel();
            }
            return Ok(true);
        }
        for (i, row) in self.rows.iter_mut().enumerate() {
            if row.touch(touch) {
                self.selected = Some(self.items[i].path.clone());
                self.status = "已选择回放，点击开始分析。".into();
                return Ok(true);
            }
        }
        Ok(true)
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(Color::from_rgba(13, 22, 32, 255));
        let t = tm.real_time() as f32;
        let r = ui.screen_rect().feather(-0.04);
        let bw = (r.w - 0.042) / 4.;
        let bh = 0.078;
        for b in &mut self.buttons {
            b.invalidate();
        }
        let back = ui.back_rect();
        let icon = self
            .back_icon
            .get_or_insert_with(|| Texture2D::from_file_with_format(include_bytes!("../../../assets/back.png"), None).into());
        self.buttons[0].inner.set(ui, back);
        ui.fill_rect(back, (**icon, back, ScaleType::Fit));
        ui.text("PRCA")
            .pos(-0.87, back.center().y)
            .anchor(0., 0.5)
            .no_baseline()
            .size(1.2)
            .draw_using(&prpr::core::BOLD_FONT);
        let start_label = if self.task.is_some() { "取消分析" } else { "开始分析" };
        for (c, (i, label)) in [(1, "本地回放"), (2, "导入回放"), (4, start_label), (11, "查看结果")].iter().enumerate() {
            self.buttons[*i].render_text(ui, Rect::new(r.x + c as f32 * (bw + 0.014), r.y + 0.14, bw, bh), t, *label, 0.35, false);
        }
        ui.text(&self.status).pos(r.x, r.y + 0.24).size(0.32).max_width(r.w).draw();
        if self.task.is_some() || self.import_task.is_some() {
            let bar = Rect::new(r.x + 0.12, r.center().y, r.w - 0.24, 0.024);
            ui.fill_rect(bar, Color::from_rgba(45, 61, 78, 255));
            let progress = if self.import_task.is_some() { 0.08 } else { self.control.fraction() };
            ui.fill_rect(Rect::new(bar.x, bar.y, bar.w * progress.max(0.005), bar.h), Color::from_rgba(88, 178, 235, 255));
            ui.text(if self.import_task.is_some() {
                "正在导入回放".into()
            } else {
                format!("{}  阶段 {}/6  {:.0}%", self.control.stage(), self.control.stage_number(), progress * 100.)
            })
            .pos(bar.x, bar.y - 0.075)
            .size(0.4)
            .max_width(bar.w)
            .draw();
            if self.task.is_some() {
                ui.text(self.control.detail())
                    .pos(bar.x, bar.y + 0.055)
                    .size(0.32)
                    .max_width(bar.w)
                    .draw();
            }
        } else {
            let body = Rect::new(r.x, r.y + 0.30, r.w, (r.h - 0.30).max(0.1));
            self.scroll.size((body.w, body.h));
            self.rows.resize_with(self.items.len(), RectButton::new);
            ui.scope(|ui| {
                ui.dx(body.x);
                ui.dy(body.y);
                self.scroll.render(ui, |ui| {
                    let mut y = 0.;
                    for (i, item) in self.items.iter().enumerate() {
                        let selected = self.selected.as_ref() == Some(&item.path);
                        let rect = Rect::new(0., y, body.w, 0.15);
                        ui.fill_path(
                            &rect.rounded(0.009),
                            if selected {
                                Color::new(0.15, 0.29, 0.38, 1.)
                            } else {
                                Color::new(0.1, 0.16, 0.22, 1.)
                            },
                        );
                        if selected {
                            ui.stroke_path(&rect.rounded(0.009), 0.0025, Color::from_rgba(104, 196, 243, 255));
                        }
                        ui.text(if item.meta.name.is_empty() {
                            "未记录谱面名称"
                        } else {
                            &item.meta.name
                        })
                        .pos(0.025, y + 0.017)
                        .size(0.42)
                        .max_width(body.w - 0.22)
                        .draw();
                        ui.text(format!("{}    {}", item.player(), item.meta.level))
                            .pos(0.025, y + 0.095)
                            .size(0.3)
                            .max_width(body.w - 0.05)
                            .draw();
                        if selected {
                            ui.text("已选择").pos(body.w - 0.025, y + 0.032).anchor(1., 0.5).size(0.3).draw();
                        }
                        self.rows[i].set(ui, rect);
                        y += 0.165;
                    }
                    if self.items.is_empty() {
                        ui.text("暂无本地回放，请点击导入回放。").pos(0.02, 0.03).size(0.4).draw();
                        y = 0.2;
                    }
                    (body.w, y)
                });
            });
        }
        if let Some(p) = &mut self.preview {
            p.render(ui, t, self.save.is_some());
        }
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene {
        self.next.take().unwrap_or_default()
    }
}
