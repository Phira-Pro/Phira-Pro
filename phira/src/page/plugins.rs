//! Management UI for the observer plugin API.
use super::{Page, SharedState};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_white, RectExt},
    scene::{request_file, request_input, return_file, return_input, show_error, take_file, take_input},
    ui::{DRectButton, Scroll, Ui},
};
use std::borrow::Cow;

pub struct PluginManager {
    items: Vec<crate::plugins::Installed>,
    rows: Vec<[DRectButton; 4]>,
    import: DRectButton,
    refresh: DRectButton,
    scroll: Scroll,
    waiting: bool,
    settings_id: Option<String>,
}
impl PluginManager {
    pub fn new() -> Self {
        let mut s = Self {
            items: vec![],
            rows: vec![],
            import: DRectButton::new(),
            refresh: DRectButton::new(),
            scroll: Scroll::new(),
            waiting: false,
            settings_id: None,
        };
        s.reload();
        s
    }
    fn reload(&mut self) {
        match crate::plugins::installed() {
            Ok(items) => self.items = items,
            Err(e) => show_error(e),
        }
        self.rows = (0..self.items.len()).map(|_| std::array::from_fn(|_| DRectButton::new())).collect();
    }
    fn render_inner(&mut self, ui: &mut Ui, t: f32) {
        let a = ui.content_rect().feather(-0.02);
        ui.text("插件管理").pos(a.x, a.y).size(0.6).draw();
        self.import
            .render_text(ui, Rect::new(a.right() - 0.40, a.y, 0.19, 0.075), t, "导入", 0.42, false);
        self.refresh
            .render_text(ui, Rect::new(a.right() - 0.19, a.y, 0.19, 0.075), t, "刷新", 0.42, false);
        ui.text(format!("{} · 导入 .zip 插件包；首次安装默认关闭", crate::plugins::API_LABEL))
            .pos(a.x, a.y + 0.09)
            .size(0.34)
            .max_width(a.w)
            .color(semi_white(0.7))
            .draw();
        self.scroll.size((a.w, (a.h - 0.15).max(0.1)));
        let reports = crate::plugins::reports();
        ui.scope(|ui| {
            ui.dx(a.x);
            ui.dy(a.y + 0.15);
            self.scroll.render(ui, |ui| {
                let w = a.w;
                let mut y = 0.;
                for (p, buttons) in self.items.iter().zip(self.rows.iter_mut()) {
                    ui.text(format!("{} · {}", p.manifest.name, p.manifest.version))
                        .pos(0., y)
                        .max_width(w - 0.86)
                        .size(0.46)
                        .draw();
                    buttons[0].render_text(ui, Rect::new(w - 0.82, y, 0.19, 0.07), t, if p.enabled { "停用" } else { "启用" }, 0.36, p.enabled);
                    buttons[3].render_text(ui, Rect::new(w - 0.61, y, 0.19, 0.07), t, "详情", 0.36, false);
                    buttons[1].render_text(ui, Rect::new(w - 0.40, y, 0.19, 0.07), t, "设置", 0.36, false);
                    buttons[2].render_text(ui, Rect::new(w - 0.19, y, 0.19, 0.07), t, "删除", 0.36, false);
                    ui.text(format!("{} · {}", p.manifest.id, p.manifest.author))
                        .pos(0., y + 0.075)
                        .max_width(w)
                        .size(0.30)
                        .color(semi_white(0.65))
                        .draw();
                    ui.text(&p.manifest.description).pos(0., y + 0.125).max_width(w).size(0.32).draw();
                    ui.text(format!("权限：{}", p.manifest.permissions.join(" / ")))
                        .pos(0., y + 0.18)
                        .max_width(w)
                        .size(0.30)
                        .draw();
                    if let Some(report) = reports.get(&p.manifest.id) {
                        let summary = match serde_json::from_str::<serde_json::Value>(report) {
                            Ok(value) => value
                                .get("text")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned)
                                .or_else(|| value.get("difficulty").and_then(|v| v.as_f64()).map(|v| format!("难度估计：{v:.2}")))
                                .unwrap_or_else(|| "分析完成，点击详情查看结果".into()),
                            Err(_) => report.clone(),
                        };
                        ui.text(summary).pos(0., y + 0.23).max_width(w).size(0.30).draw();
                    }
                    y += 0.31;
                }
                if self.items.is_empty() {
                    ui.text("暂无插件。导入插件以添加个性化显示或谱面分析。")
                        .pos(0., 0.02)
                        .max_width(w)
                        .size(0.4)
                        .draw();
                }
                (w, y.max(0.1))
            });
        });
    }
}
impl Page for PluginManager {
    fn label(&self) -> Cow<'static, str> {
        "插件管理".into()
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if self.waiting {
            if let Some((id, path)) = take_file() {
                if id == "plugin_import" {
                    self.waiting = false;
                    if !path.is_empty() {
                        if let Err(e) = crate::plugins::import(std::path::Path::new(&path)) {
                            show_error(e);
                        }
                        self.reload();
                    }
                } else {
                    return_file(id, path);
                }
            }
        }
        if self.settings_id.is_some() {
            if let Some((id, text)) = take_input() {
                if id == "plugin_settings" {
                    let plugin = self.settings_id.take().unwrap();
                    let result = serde_json::from_str(&text)
                        .map_err(anyhow::Error::from)
                        .and_then(|v| crate::plugins::change(&plugin, None, Some(v), false));
                    if let Err(e) = result {
                        show_error(e);
                    }
                    self.reload();
                } else {
                    return_input(id, text);
                }
            }
        }
        Ok(())
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.import.touch(touch, s.t) {
            self.waiting = true;
            request_file("plugin_import");
            return Ok(true);
        }
        if self.refresh.touch(touch, s.t) {
            crate::plugins::reload();
            self.reload();
            return Ok(true);
        }
        if self.scroll.touch(touch, s.t) {
            return Ok(true);
        }
        let mut action = None;
        for (i, buttons) in self.rows.iter_mut().enumerate() {
            for (b, button) in buttons.iter_mut().enumerate() {
                if button.touch(touch, s.t) {
                    action = Some((i, b));
                }
            }
        }
        if let Some((i, b)) = action {
            let p = &self.items[i];
            let id = p.manifest.id.clone();
            let r = match b {
                0 => crate::plugins::change(&id, Some(!p.enabled), None, false),
                1 => {
                    self.settings_id = Some(id);
                    request_input("plugin_settings", inputbox::InputBox::new().default_text(serde_json::to_string_pretty(&p.settings)?));
                    Ok(())
                }
                2 => crate::plugins::change(&id, None, None, true),
                _ => {
                    let report = crate::plugins::reports()
                        .remove(&id)
                        .unwrap_or_else(|| "暂无分析结果，请启用插件并打开谱面。".into());
                    let report = serde_json::from_str::<serde_json::Value>(&report)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or(report);
                    prpr::ui::Dialog::plain(p.manifest.name.clone(), report)
                        .buttons(vec!["关闭".into()])
                        .show();
                    Ok(())
                }
            };
            if let Err(e) = r {
                show_error(e);
            }
            self.reload();
            return Ok(true);
        }
        Ok(false)
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| self.render_inner(ui, t));
        Ok(())
    }
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) fn render_regression(painter: &mut prpr::ui::TextPainter, state: &mut SharedState) {
    let mut manager = PluginManager::new();
    assert_eq!(manager.items.len(), 1);
    for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::new(0.10, 0.13, 0.18, 1.));
        manager.render_inner(&mut ui, 2.);
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/replay-qa/plugins-{suffix}.png"));
    }
    let center = |b: &DRectButton| {
        let mut sum = Vec2::ZERO;
        let mut count = 0;
        for y in -100..=100 {
            for x in -100..=100 {
                let p = vec2(x as f32 / 100., y as f32 / 100.);
                if b.inner.contains(p) {
                    sum += p;
                    count += 1;
                }
            }
        }
        assert!(count > 0, "plugin button must be on screen");
        sum / count as f32
    };
    let enabled = manager.items[0].enabled;
    let p = center(&manager.rows[0][0]);
    state.t = 2.;
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Page::touch(
            &mut manager,
            &Touch {
                id: 101,
                phase,
                position: p,
                time: f64::INFINITY,
            },
            state,
        )
        .unwrap();
    }
    assert_eq!(crate::plugins::installed().unwrap()[0].enabled, !enabled);
    crate::plugins::change("example.density", Some(true), Some(serde_json::json!({"factor":2.})), false).unwrap();
    crate::plugins::result(&prpr::scene::SimpleRecord {
        score: 987654,
        accuracy: 0.98,
        ..Default::default()
    });
    let result: serde_json::Value = serde_json::from_str(&crate::plugins::reports()["example.density"]).unwrap();
    assert_eq!(result["score"], 987654);
    crate::plugins::change("example.density", None, None, true).unwrap();
    assert!(crate::plugins::installed().unwrap().is_empty());
    println!("Plugin: real GameScene observer, result hook, persisted controls, deletion and three aspect ratios passed");
}
