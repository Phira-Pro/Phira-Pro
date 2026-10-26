//! Management UI for the observer plugin API.
use super::{
    management::{card, card_header, row_title, PANEL},
    settings::{item_row_h, right_rect},
    Page, SharedState,
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_white, RectExt},
    scene::{request_file, request_input, return_file, return_input, show_error, take_file, take_input},
    ui::{DRectButton, Scroll, Ui},
};
use std::borrow::Cow;

fn plugin_details(manifest: &crate::plugins::Manifest) -> String {
    let description = manifest.description.trim();
    let author = if manifest.author.trim().is_empty() {
        "未填写"
    } else {
        &manifest.author
    };
    let permissions = if manifest.permissions.is_empty() {
        "无".into()
    } else {
        manifest.permissions.join(" / ")
    };
    format!(
        "{}\n\n版本：{}\n作者：{}\n插件 ID：{}\nAPI：v{}\n权限：{}",
        if description.is_empty() {
            "此插件未提供介绍。"
        } else {
            description
        },
        manifest.version,
        author,
        manifest.id,
        manifest.api,
        permissions,
    )
}

pub struct PluginManager {
    items: Vec<crate::plugins::Installed>,
    rows: Vec<[DRectButton; 4]>,
    import: DRectButton,
    refresh: DRectButton,
    operation_limit: DRectButton,
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
            operation_limit: DRectButton::new(),
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
        let a = ui.content_rect().feather(-0.015);
        ui.fill_rect(a, PANEL);
        self.scroll.size((a.w, a.h));
        ui.scope(|ui| {
            ui.dx(a.x);
            ui.dy(a.y);
            self.scroll.render(ui, |ui| {
                let w = a.w;
                let rr = right_rect(w);
                let mut height = 0.;
                macro_rules! row { ($($body:tt)*) => {{ $($body)* ui.dy(item_row_h()); height += item_row_h(); }}; }
                row! {
                    let import_rect = Rect::new(rr.x - 0.28, rr.y, rr.w, rr.h);
                    row_title(ui, "已安装插件", Some(&format!("已安装 {} 个插件。导入 .zip 安装，首次安装默认关闭。", self.items.len())), import_rect.x);
                    self.import.render_text(ui, import_rect, t, "导入", 0.42, false);
                    self.refresh.render_text(ui, rr, t, "刷新", 0.42, false);
                }
                for (p, buttons) in self.items.iter().zip(self.rows.iter_mut()) {
                    ui.dy(0.025);
                    height += 0.025;
                    card(ui, w, item_row_h() * 3., false);
                    row! {
                        let author = if p.manifest.author.trim().is_empty() { "未填写作者" } else { &p.manifest.author };
                        card_header(ui, "插件", &p.manifest.name, &format!("版本 v{}，作者：{}", p.manifest.version, author), rr.x);
                        buttons[0].render_text(ui, rr, t, if p.enabled { "已启用" } else { "已停用" }, 0.42, p.enabled);
                    }
                    row! {
                        let description = p.manifest.description.split_whitespace().collect::<Vec<_>>().join(" ");
                        row_title(ui, "插件介绍", Some(if description.is_empty() { "此插件未提供介绍" } else { &description }), rr.x);
                        buttons[3].render_text(ui, rr, t, "详情", 0.42, false);
                    }
                    row! {
                        let settings_rect = Rect::new(rr.x - 0.28, rr.y, rr.w, rr.h);
                        row_title(ui, "管理插件", Some(&p.manifest.id), settings_rect.x);
                        buttons[1].render_text(ui, settings_rect, t, "设置", 0.42, false);
                        buttons[2].render_text(ui, rr, t, "删除", 0.42, false);
                    }
                }
                if self.items.is_empty() {
                    row! { row_title(ui, "暂无插件", Some("导入插件以添加个性化显示或谱面分析"), w - 0.035); }
                }
                ui.dy(0.04);
                height += 0.04;
                ui.text("通用运行设置")
                    .pos(0.035, 0.025)
                    .anchor(0., 0.5)
                    .size(0.34)
                    .color(semi_white(0.7))
                    .draw();
                ui.fill_rect(Rect::new(0.34, 0.025, w - 0.38, 0.002), semi_white(0.15));
                ui.dy(0.075);
                height += 0.075;
                row! {
                    let limit_rect = Rect::new(rr.x - 0.15, rr.y, rr.w + 0.15, rr.h);
                    row_title(ui, "脚本操作数上限", Some("对所有插件生效；非法输入恢复默认值；帧回调固定 25000"), limit_rect.x);
                    self.operation_limit.render_text(ui, limit_rect, t, crate::plugins::operation_limit().to_string(), 0.36, false);
                }
                (w, height + 0.02)
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
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "plugin_settings" if self.settings_id.is_some() => {
                    let plugin = self.settings_id.take().unwrap();
                    let result = serde_json::from_str(&text)
                        .map_err(anyhow::Error::from)
                        .and_then(|v| crate::plugins::change(&plugin, None, Some(v), false));
                    if let Err(e) = result {
                        show_error(e);
                    }
                    self.reload();
                }
                "plugin_operation_limit" => match crate::plugins::set_operation_limit(&text) {
                    Ok(true) => {}
                    Ok(false) => show_error(anyhow::anyhow!("操作数上限必须是正整数；已恢复默认值 {}", crate::plugins::DEFAULT_OPERATION_LIMIT)),
                    Err(error) => show_error(error),
                },
                _ => return_input(id, text),
            }
        }
        Ok(())
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.scroll.touch(touch, s.t) {
            return Ok(true);
        }
        if !self.scroll.contains(touch) {
            return Ok(false);
        }
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
        if self.operation_limit.touch(touch, s.t) {
            request_input("plugin_operation_limit", inputbox::InputBox::new().default_text(crate::plugins::operation_limit().to_string()));
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
                    prpr::ui::Dialog::plain(p.manifest.name.clone(), plugin_details(&p.manifest))
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
fn render_snapshots(manager: &mut PluginManager, painter: &mut prpr::ui::TextPainter, prefix: &str) {
    for (w, h, suffix) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        for (view, offset) in [("", 0.), ("-scrolled", item_row_h())] {
            manager.scroll.y_scroller.offset = offset;
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
            .export_png(&format!("target/replay-qa/{prefix}-{suffix}{view}.png"));
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) fn render_regression(painter: &mut prpr::ui::TextPainter, state: &mut SharedState) {
    let mut manager = PluginManager::new();
    assert_eq!(manager.items.len(), 1);
    render_snapshots(&mut manager, painter, "plugins");
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
    // The real details button must open the package introduction even with a live report.
    let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
    set_camera(&ui.camera());
    clear_background(Color::new(0.10, 0.13, 0.18, 1.));
    manager.render_inner(&mut ui, 2.);
    let p = center(&manager.rows[0][3]);
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        Page::touch(
            &mut manager,
            &Touch {
                id: 102,
                phase,
                position: p,
                time: f64::INFINITY,
            },
            state,
        )
        .unwrap();
    }
    let mut dialog = prpr::scene::DIALOG
        .with(|dialog| dialog.borrow_mut().take())
        .expect("details opens a dialog");
    dialog.render(&mut ui, 2.);
    unsafe { get_internal_gl() }.flush();
    let mut bytes = vec![0; 1280 * 720 * 4];
    unsafe {
        use miniquad::gl::*;
        glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
        assert_eq!(glGetError(), 0);
    }
    Image {
        width: 1280,
        height: 720,
        bytes,
    }
    .export_png("target/replay-qa/plugin-details.png");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&crate::plugins::reports()["example.density"]).unwrap(), result);
    crate::plugins::change("example.density", None, None, true).unwrap();
    assert!(crate::plugins::installed().unwrap().is_empty());
    println!("Plugin: real GameScene observer, result hook, persisted controls, deletion and three aspect ratios passed");
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use prpr::{
        core::BOLD_FONT,
        ui::{FontArc, TextPainter},
    };
    use std::io::Write;

    #[test]
    #[ignore = "requires desktop OpenGL; run separately with --ignored --test-threads=1"]
    fn plugin_manager_layout_and_package_details() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _runtime = runtime.enter();
        std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        macroquad::Window::from_config(
            Conf {
                window_title: "Plugin UI regression".into(),
                window_width: 1280,
                window_height: 960,
                headless: true,
                ..Default::default()
            },
            async {
                next_frame().await;
                std::fs::create_dir_all("target/replay-qa").unwrap();
                let isolated = tempfile::tempdir_in("target/replay-qa").unwrap();
                let old_root = crate::DATA_PATH.lock().unwrap().replace(isolated.path().to_string_lossy().into_owned());
                crate::set_data(crate::data::Data::default());
                macroquad::file::set_pc_assets_folder("assets");
                prpr::ui::set_multilingual_fallback(FontArc::try_from_vec(std::fs::read("assets/font.ttf").unwrap()).unwrap());
                let font = FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
                let mut painter = TextPainter::new(font.clone(), None);
                BOLD_FONT.with(|v| {
                    *v.borrow_mut() =
                        Some(TextPainter::new(FontArc::try_from_vec(std::fs::read("assets/bold.ttf").unwrap()).unwrap(), Some(font.clone())))
                });
                let package = isolated.path().join("density.zip");
                let mut archive = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
                for (name, body) in [
                    ("manifest.json", include_str!("../../../docs/plugins/examples/density/manifest.json")),
                    ("main.rhai", include_str!("../../../docs/plugins/examples/density/main.rhai")),
                ] {
                    archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                    archive.write_all(body.as_bytes()).unwrap();
                }
                archive.finish().unwrap();
                crate::plugins::import(&package).unwrap();
                let mut state = SharedState::new(font).await.unwrap();
                render_regression(&mut painter, &mut state);
                let package = isolated.path().join("ppsr.zip");
                let mut archive = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
                let source = std::path::Path::new("docs/plugins/examples/ppsr");
                for name in ["manifest.json", "main.rhai", "model.json", "profile.json"] {
                    archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                    archive.write_all(&std::fs::read(source.join(name)).unwrap()).unwrap();
                }
                archive.finish().unwrap();
                crate::plugins::import(&package).unwrap();
                let mut manager = PluginManager::new();
                assert_eq!(manager.items[0].manifest.author, "Rulaifozu");
                render_snapshots(&mut manager, &mut painter, "plugins-ppsr");
                let mut ui = Ui::new(&mut painter, Some((0, 0, 1280, 720)));
                set_camera(&ui.camera());
                clear_background(Color::new(0.10, 0.13, 0.18, 1.));
                manager.render_inner(&mut ui, 2.);
                let mut dialog = prpr::ui::Dialog::plain(manager.items[0].manifest.name.clone(), plugin_details(&manager.items[0].manifest));
                dialog.render(&mut ui, 2.);
                unsafe { get_internal_gl() }.flush();
                let mut bytes = vec![0; 1280 * 720 * 4];
                unsafe {
                    use miniquad::gl::*;
                    glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
                    assert_eq!(glGetError(), 0);
                }
                Image {
                    width: 1280,
                    height: 720,
                    bytes,
                }
                .export_png("target/replay-qa/plugin-ppsr-details.png");
                prpr::ui::cleanup_audio();
                *crate::DATA_PATH.lock().unwrap() = old_root;
            },
        );
    }
}
