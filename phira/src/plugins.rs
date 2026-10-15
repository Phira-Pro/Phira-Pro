//! Versioned, observer-only Rhai plugins. No native loading, filesystem, network,
//! account credentials, mutable chart, judgement, score or upload API is exposed.
use anyhow::{Context, Result};
use macroquad::prelude::{Rect, Touch, TouchPhase, Vec2};
use prpr::{
    ext::RectExt,
    ui::{DRectButton, RectButton, Ui},
};
use rhai::{Dynamic, Engine, Scope, AST};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_PACKAGE: u64 = 2 << 20;
/// Public interface name; the numeric manifest field is its compatibility version.
pub const API_NAME: &str = "Phira Pro Plugin API";
pub const API_VERSION: u32 = 2;
pub const API_LABEL: &str = "Phira Pro Plugin API v2";
pub const PPSR_PLUGIN_ID: &str = "phira-pro.ppsr";

#[derive(Default)]
pub struct PpsrPanelState {
    position: Option<Vec2>,
    bounds: Option<Rect>,
    drag_handle: RectButton,
    drag: Option<(u64, Vec2, Vec2)>,
    ui_top: f32,
}

impl PpsrPanelState {
    pub fn touch(&mut self, touch: &Touch) -> bool {
        if let Some((id, start_touch, start_position)) = self.drag {
            if id != touch.id {
                return false;
            }
            match touch.phase {
                TouchPhase::Moved | TouchPhase::Stationary => {
                    if let Some(bounds) = self.bounds {
                        let delta = touch.position - start_touch;
                        let x = (start_position.x + delta.x).clamp(-1.0, 1.0 - bounds.w);
                        let y = (start_position.y + delta.y).clamp(-self.ui_top, (self.ui_top - bounds.h).max(-self.ui_top));
                        self.position = Some(Vec2::new(x, y));
                    }
                }
                TouchPhase::Ended | TouchPhase::Cancelled => self.drag = None,
                TouchPhase::Started => {}
            }
            return true;
        }

        if touch.phase == TouchPhase::Started && self.drag_handle.contains(touch.position) {
            if let Some(bounds) = self.bounds {
                self.drag = Some((touch.id, touch.position, Vec2::new(bounds.x, bounds.y)));
                return true;
            }
        }
        false
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub api: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "empty_object")]
    pub settings: Value,
    /// Package-fixed data exposed only to API v2 chart callbacks.
    #[serde(default = "empty_object")]
    pub assets: Value,
}
fn empty_object() -> Value {
    json!({})
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub script: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "empty_object")]
    pub settings: Value,
}
struct Runtime {
    plugin: Installed,
    engine: Engine,
    ast: AST,
    report: Value,
    error: Option<String>,
    frame_tick: i64,
}
thread_local! { static RUNTIME: RefCell<Vec<Runtime>> = const { RefCell::new(Vec::new()) }; }

fn engine() -> Engine {
    let mut e = Engine::new();
    // Explicitly prevent Rhai's default file module resolver from importing
    // local files. Host capabilities consist only of argument data and return values.
    e.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    e.set_max_operations(5_000_000)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 32)
        .set_max_variables(256)
        .set_max_functions(256)
        .set_max_modules(1)
        // Rhai counts nested containers recursively. A 50k-note input already
        // contains 400k map entries; per-note-looking limits reject real charts.
        .set_max_string_size(8 << 20)
        .set_max_array_size(1_000_000)
        .set_max_map_size(1_000_000);
    e.on_print(|_| {});
    e.on_debug(|_, _, _| {});
    e
}
fn index_path() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join("plugins/index.json"))
}
fn read_index() -> Result<Vec<Installed>> {
    read_index_at(&index_path()?)
}
fn read_index_at(path: &Path) -> Result<Vec<Installed>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    anyhow::ensure!(std::fs::metadata(&path)?.len() <= 16 << 20, "插件索引过大");
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn write_index(plugins: &[Installed]) -> Result<()> {
    write_index_at(&index_path()?, plugins)
}
fn write_index_at(path: &Path, plugins: &[Installed]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let bytes = serde_json::to_vec_pretty(plugins)?;
    anyhow::ensure!(bytes.len() <= 16 << 20, "插件库超过 16MiB，请先删除不需要的插件");
    crate::transfer::write_atomic(&path, &bytes)
}
fn verify(m: &Manifest, script: &str) -> Result<()> {
    anyhow::ensure!(matches!(m.api, 1 | API_VERSION), "不支持 {} v{}；当前支持 {}（并兼容 v1）", API_NAME, m.api, API_LABEL);
    anyhow::ensure!(!m.id.is_empty() && m.id.len() <= 80 && m.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)), "插件 ID 无效");
    anyhow::ensure!(!m.name.trim().is_empty() && m.name.len() <= 160 && m.version.len() <= 80 && m.description.len() <= 2048, "插件名称或描述无效");
    anyhow::ensure!(m.settings.is_object() && serde_json::to_vec(&m.settings)?.len() <= 16 << 10, "插件 settings 必须是小于 16KiB 的 JSON 对象");
    anyhow::ensure!(m.assets.is_object() && serde_json::to_vec(&m.assets)?.len() <= 1 << 20, "插件 assets 必须是小于 1MiB 的 JSON 对象");
    for p in &m.permissions {
        anyhow::ensure!(["chart.read", "result.read", "overlay.draw"].contains(&p.as_str()), "不支持的权限：{p}");
    }
    anyhow::ensure!(script.len() as u64 <= MAX_PACKAGE, "插件脚本过大");
    engine().compile(script).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
pub fn import(path: &Path) -> Result<()> {
    install_package(path, &index_path()?)?;
    reload();
    Ok(())
}
fn install_package(path: &Path, index: &Path) -> Result<()> {
    anyhow::ensure!(std::fs::metadata(path)?.len() <= MAX_PACKAGE, "插件包超过 2MiB");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    anyhow::ensure!(zip.len() == 2, "插件包只允许 manifest.json 和 main.rhai");
    fn entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
        let file = zip.by_name(name).with_context(|| format!("缺少 {name}"))?;
        anyhow::ensure!(file.size() <= MAX_PACKAGE, "插件内容过大");
        let mut text = String::new();
        file.take(MAX_PACKAGE + 1).read_to_string(&mut text)?;
        anyhow::ensure!(text.len() as u64 <= MAX_PACKAGE, "插件内容过大");
        Ok(text)
    }
    let manifest: Manifest = serde_json::from_str(&entry(&mut zip, "manifest.json")?)?;
    let script = entry(&mut zip, "main.rhai")?;
    verify(&manifest, &script)?;
    let mut list = read_index_at(index)?;
    anyhow::ensure!(list.len() < 64, "最多安装 64 个插件");
    anyhow::ensure!(!list.iter().any(|p| p.manifest.id == manifest.id), "此 ID 已安装，请先删除旧插件");
    list.push(Installed {
        settings: manifest.settings.clone(),
        manifest,
        script,
        enabled: false,
    });
    write_index_at(index, &list)
}
pub fn installed() -> Result<Vec<Installed>> {
    read_index()
}
pub fn change(id: &str, enabled: Option<bool>, settings: Option<Value>, remove: bool) -> Result<()> {
    let mut list = read_index()?;
    if remove {
        list.retain(|p| p.manifest.id != id);
    } else {
        let p = list.iter_mut().find(|p| p.manifest.id == id).context("插件已不存在")?;
        if let Some(v) = enabled {
            p.enabled = v;
        }
        if let Some(v) = settings {
            anyhow::ensure!(v.is_object() && serde_json::to_vec(&v)?.len() <= 16 << 10, "设置必须是小于 16KiB 的 JSON 对象");
            p.settings = v;
        }
    }
    write_index(&list)?;
    reload();
    Ok(())
}
pub fn init() {
    crate::get_data();
    reload();
    prpr::extensions::set_chart_observer(chart_loaded);
}
pub fn reload() {
    let list = match read_index() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(?e, "cannot read plugin index");
            return;
        }
    };
    let mut active = vec![];
    for p in list.into_iter().filter(|p| p.enabled) {
        let engine = engine();
        match verify(&p.manifest, &p.script).and_then(|_| engine.compile(&p.script).map_err(|e| anyhow::anyhow!(e.to_string()))) {
            Ok(ast) => active.push(Runtime {
                plugin: p,
                engine,
                ast,
                report: Value::Null,
                error: None,
                frame_tick: -1,
            }),
            Err(e) => tracing::warn!(?e, id = p.manifest.id, "plugin invalid"),
        }
    }
    RUNTIME.with(|v| *v.borrow_mut() = active);
}
fn invoke(r: &mut Runtime, name: &str, payload: &Value, permission: &str) -> Option<Value> {
    if r.error.is_some() || !r.plugin.manifest.permissions.iter().any(|p| p == permission) {
        return None;
    }
    if name == "on_chart_selected" && r.plugin.manifest.api < 2 {
        return None;
    }
    if !r.ast.iter_functions().any(|f| f.name == name && f.params.len() == 2) {
        return None;
    }
    r.engine.set_max_operations(if name == "on_frame" { 25_000 } else { 5_000_000 });
    let call = (|| -> Result<Value> {
        let mut payload = payload.clone();
        if name == "on_chart" || name == "on_chart_selected" {
            if let Some(ctx) = payload.as_object_mut() {
                ctx.insert("api".into(), json!(r.plugin.manifest.api));
                if r.plugin.manifest.api >= 2 {
                    ctx.insert("assets".into(), r.plugin.manifest.assets.clone());
                }
            }
        }
        let ctx = rhai::serde::to_dynamic(&payload)?;
        let settings = rhai::serde::to_dynamic(&r.plugin.settings)?;
        let output = r
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), &r.ast, name, (ctx, settings))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let v: Value = rhai::serde::from_dynamic(&output)?;
        anyhow::ensure!(serde_json::to_vec(&v)?.len() <= 32 << 10, "插件返回内容过大");
        Ok(v)
    })();
    match call {
        Ok(v) => Some(v),
        Err(e) => {
            r.error = Some(e.to_string());
            tracing::warn!(id = r.plugin.manifest.id, ?e, "plugin suspended");
            None
        }
    }
}
fn chart_loaded(chart: &prpr::core::Chart, info: &prpr::info::ChartInfo, config: &prpr::config::Config, raw: &[u8]) {
    RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        if runtime.is_empty() { return; }
        let mut notes = vec![];
        let mut count = 0usize;
        for (line, l) in chart.lines.iter().enumerate() { for (index, n) in l.notes.iter().enumerate().filter(|(_, n)| !n.fake) {
            count += 1;
            if notes.len() >= 50_000 { continue; }
            use prpr::core::NoteKind;
            let (kind, end) = match n.kind { NoteKind::Click => ("tap", n.time), NoteKind::Drag => ("drag", n.time), NoteKind::Flick => ("flick", n.time), NoteKind::Hold {end_time, ..} => ("hold", end_time) };
            notes.push(json!({"line":line,"index":index,"time":n.time,"end_time":end,"kind":kind,"above":n.above,"speed":n.speed}));
        }}
        notes.sort_by(|a,b| a["time"].as_f64().unwrap().total_cmp(&b["time"].as_f64().unwrap()));
        let raw: Value = if raw.len() <= 4 << 20 { serde_json::from_slice(raw).unwrap_or(Value::Null) } else { Value::Null };
        let ctx = json!({"api":API_VERSION,"name":info.name,"level":info.level,"difficulty":info.difficulty,"note_count":count,"truncated":count>notes.len(),"notes":notes,"raw":raw,"speed":config.speed,"flow_speed":config.flow_speed});
        for r in runtime.iter_mut() { if let Some(v) = invoke(r,"on_chart", &ctx,"chart.read") { r.report=v; } }
    });
}
pub fn result(record: &prpr::scene::SimpleRecord) {
    if let Ok(ctx) = serde_json::to_value(record) {
        RUNTIME.with(|runtime| {
            for r in runtime.borrow_mut().iter_mut() {
                if let Some(v) = invoke(r, "on_result", &ctx, "result.read") {
                    r.report = v;
                }
            }
        });
    }
}
pub fn chart_selected_needed() -> bool {
    RUNTIME.with(|runtime| {
        runtime.borrow().iter().any(|r| {
            r.plugin.manifest.api >= 2
                && r.error.is_none()
                && r.plugin.manifest.permissions.iter().any(|permission| permission == "chart.read")
                && r.ast
                    .iter_functions()
                    .any(|function| function.name == "on_chart_selected" && function.params.len() == 2)
        })
    })
}
pub fn clear_chart_selected_reports() {
    RUNTIME.with(|runtime| {
        for r in runtime.borrow_mut().iter_mut() {
            if r.plugin.manifest.api >= 2
                && r.error.is_none()
                && r.plugin.manifest.permissions.iter().any(|permission| permission == "chart.read")
                && r.ast
                    .iter_functions()
                    .any(|function| function.name == "on_chart_selected" && function.params.len() == 2)
            {
                r.report = Value::Null;
            }
        }
    });
}
pub fn chart_selected(payload: &Value) {
    RUNTIME.with(|runtime| {
        for r in runtime.borrow_mut().iter_mut() {
            let selected_hook = r.plugin.manifest.api >= 2
                && r.error.is_none()
                && r.plugin.manifest.permissions.iter().any(|permission| permission == "chart.read")
                && r.ast
                    .iter_functions()
                    .any(|function| function.name == "on_chart_selected" && function.params.len() == 2);
            if selected_hook {
                r.report = Value::Null;
                if let Some(v) = invoke(r, "on_chart_selected", payload, "chart.read") {
                    r.report = v;
                }
            }
        }
    });
}
fn ppsr_report() -> Option<Value> {
    RUNTIME.with(|runtime| {
        runtime
            .borrow()
            .iter()
            .find(|r| {
                r.plugin.manifest.id == PPSR_PLUGIN_ID
                    && r.plugin.manifest.api >= 2
                    && r.error.is_none()
                    && r.plugin.manifest.permissions.iter().any(|permission| permission == "overlay.draw")
            })
            .map(|r| r.report.clone())
            .filter(|report| report.get("difficulty").and_then(Value::as_f64).is_some())
    })
}
pub fn render_ppsr_panel(ui: &mut Ui, t: f32, collapsed: bool, toggle: &mut DRectButton, panel: &mut PpsrPanelState) -> bool {
    let Some(report) = ppsr_report() else {
        panel.bounds = None;
        panel.drag = None;
        return false;
    };
    let parts = report.get("parts").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let width = 0.72;
    let row_height = 0.056;
    let height = 0.12 + if collapsed { 0. } else { parts.len().min(10) as f32 * row_height + 0.04 };
    let position = panel.position.unwrap_or(Vec2::new(0.98 - width, ui.top - height - 0.08));
    let rect = Rect::new(position.x.clamp(-1.0, 1.0 - width), position.y.clamp(-ui.top, (ui.top - height).max(-ui.top)), width, height);
    panel.position = Some(Vec2::new(rect.x, rect.y));
    panel.bounds = Some(rect);
    panel.ui_top = ui.top;
    let header_height = 0.084_f32.min(height);
    panel.drag_handle.set(ui, Rect::new(rect.x, rect.y, width - 0.18, header_height));
    let panel_color = macroquad::prelude::Color::new(0.04, 0.06, 0.08, 0.9);
    ui.fill_path(&rect.rounded(0.02), panel_color);
    ui.text(format!("PPSR {:.2}", report["difficulty"].as_f64().unwrap_or_default()))
        .pos(rect.x + 0.025, rect.y + 0.019)
        .size(0.38)
        .draw();
    let toggle_rect = Rect::new(rect.right() - 0.16, rect.y + 0.009, 0.14, 0.06);
    toggle.render_text(ui, toggle_rect, t, if collapsed { "展开" } else { "收起" }, 0.25, false);
    if !collapsed {
        ui.text("细分难度")
            .pos(rect.x + 0.025, rect.y + 0.105)
            .size(0.29)
            .color(macroquad::prelude::Color::new(1., 1., 1., 0.72))
            .draw();
        for (index, part) in parts.iter().take(10).enumerate() {
            let label = part["label"].as_str().or_else(|| part["key"].as_str()).unwrap_or("—");
            let score = part["score"].as_f64().unwrap_or(1.0).clamp(1.0, 10.0);
            let y = rect.y + 0.145 + index as f32 * row_height;
            ui.text(label).pos(rect.x + 0.025, y).size(0.27).draw();
            ui.text(format!("{score:.1}/10"))
                .pos(rect.right() - 0.025, y)
                .anchor(1.0, 0.0)
                .size(0.27)
                .draw();
            let track = Rect::new(rect.x + 0.025, y + 0.033, width - 0.05, 0.009);
            ui.fill_rect(track, macroquad::prelude::Color::new(1.0, 1.0, 1.0, 0.14));
            let progress = Rect::new(track.x, track.y, track.w * score as f32 / 10.0, track.h);
            ui.fill_path(&progress.rounded(0.004), macroquad::prelude::Color::new(0.2, 0.78, 0.94, 0.95));
        }
    }
    true
}
pub fn reports() -> BTreeMap<String, String> {
    RUNTIME.with(|runtime| {
        runtime
            .borrow()
            .iter()
            .map(|r| (r.plugin.manifest.id.clone(), r.error.as_ref().map(|s| format!("已暂停：{s}")).unwrap_or_else(|| r.report.to_string())))
            .collect()
    })
}
pub fn render(ui: &mut prpr::ui::Ui, time: f64) {
    RUNTIME.with(|runtime| {
        for r in runtime.borrow_mut().iter_mut() {
            if !r.plugin.manifest.permissions.iter().any(|p| p == "overlay.draw") || r.error.is_some() {
                continue;
            }
            // Frame hooks run at most 5Hz; overlay rendering itself remains smooth.
            let tick = (time * 5.).floor() as i64;
            if tick != r.frame_tick {
                r.frame_tick = tick;
                let ctx = json!({"time":time,"fps":f32::from_bits(prpr::ui::CURRENT_FPS.load(std::sync::atomic::Ordering::Relaxed))});
                if let Some(v) = invoke(r, "on_frame", &ctx, "overlay.draw") {
                    if let Some(map) = r.report.as_object_mut() {
                        map.insert("hud".into(), v);
                    } else {
                        r.report = json!({"hud":v});
                    }
                }
            }
            if let Some(hud) = r.report.get("hud").and_then(Value::as_array) {
                for item in hud.iter().take(8) {
                    let text = item["text"].as_str().unwrap_or("").chars().take(160).collect::<String>();
                    let x = item["x"].as_f64().unwrap_or(0.).clamp(-0.9, 0.9) as f32;
                    let y = item["y"].as_f64().unwrap_or(-0.8).clamp(-0.9, 0.9) as f32 * ui.top;
                    let color = item["color"].as_u64().unwrap_or(0xffffff) as u32 & 0xffffff;
                    ui.text(text)
                        .pos(x, y)
                        .size(0.4)
                        .anchor(0.5, 0.5)
                        .color(macroquad::prelude::Color::from_hex_rgb(color))
                        .draw();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_example_executes_each_documented_hook() {
        let manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/density/manifest.json")).unwrap();
        let script = include_str!("../../docs/plugins/examples/density/main.rhai");
        verify(&manifest, script).unwrap();
        let plugin = Installed {
            settings: manifest.settings.clone(),
            manifest,
            script: script.into(),
            enabled: true,
        };
        let e = engine();
        let ast = e.compile(script).unwrap();
        let mut r = Runtime {
            plugin,
            engine: e,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let v = invoke(&mut r, "on_chart", &json!({"notes":[{"time":1.0},{"time":1.2},{"time":1.4},{"time":3.0}],"truncated":false}), "chart.read")
            .expect("example chart hook");
        assert_eq!(v["difficulty"], 3.0);
        assert!(invoke(&mut r, "on_result", &json!({"score":987654,"accuracy":0.98}), "result.read").unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("98"));
        assert_eq!(invoke(&mut r, "on_frame", &json!({"time":1.,"fps":120.}), "overlay.draw").unwrap()[0]["color"], 0x88ccff);
    }
    #[test]
    fn maximum_normalized_chart_fits_recursive_container_budget() {
        let note = json!({"line":0,"index":0,"time":1.,"end_time":1.,"kind":"hold","above":true,"speed":1.});
        let payload = json!({"notes":vec![note;50_000],"raw":null});
        let data = rhai::serde::to_dynamic(&payload).unwrap();
        engine().ensure_data_size_within_limits(&data).unwrap();
    }
    #[test]
    fn observer_functions_return_data_and_runaway_scripts_are_stopped() {
        let p = Installed {
            manifest: Manifest {
                api: 1,
                id: "test".into(),
                name: "test".into(),
                version: "1".into(),
                author: String::new(),
                description: String::new(),
                permissions: vec!["chart.read".into()],
                settings: json!({}),
                assets: json!({}),
            },
            script: String::new(),
            enabled: true,
            settings: json!({"factor":2}),
        };
        let e = engine();
        let ast = e
            .compile("fn on_chart(ctx, settings) { #{ difficulty: ctx.notes.len() * settings.factor } }")
            .unwrap();
        let mut r = Runtime {
            plugin: p,
            engine: e,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        assert_eq!(invoke(&mut r, "on_chart", &json!({"notes":[1,2,3]}), "chart.read").unwrap()["difficulty"], 6);
        assert!(invoke(&mut r, "on_chart", &json!({}), "result.read").is_none());
        r.ast = r.engine.compile("fn on_chart_selected(ctx, settings) { #{ difficulty: 1 } }").unwrap();
        assert!(invoke(&mut r, "on_chart_selected", &json!({}), "chart.read").is_none());
        r.ast = r.engine.compile("fn on_chart(ctx, settings) { loop {} }").unwrap();
        assert!(invoke(&mut r, "on_chart", &json!({}), "chart.read").is_none());
        assert!(r.error.is_some());
        assert!(engine().eval::<Dynamic>("import \"/etc/passwd\" as p;").is_err());
    }
    #[test]
    fn selected_chart_hook_receives_packaged_assets_without_persisting_mutations() {
        let mut plugin = Installed {
            manifest: Manifest {
                api: 2,
                id: "test".into(),
                name: "test".into(),
                version: "1".into(),
                author: String::new(),
                description: String::new(),
                permissions: vec!["chart.read".into()],
                settings: json!({}),
                assets: json!({"model":{"bias":7}}),
            },
            script: String::new(),
            enabled: true,
            settings: json!({}),
        };
        let engine = engine();
        let script =
            "fn on_chart_selected(ctx, settings) { let b = ctx.assets.model.bias; ctx.assets.model.bias = 0; #{ difficulty: ctx.features[0] + b } }";
        plugin.script = script.into();
        let ast = engine.compile(script).unwrap();
        let mut runtime = Runtime {
            plugin,
            engine,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let report = invoke(&mut runtime, "on_chart_selected", &json!({"features":[5.]}), "chart.read").unwrap();
        assert_eq!(report["difficulty"], 12.0);
        assert_eq!(runtime.plugin.manifest.assets["model"]["bias"], 7);
    }
    #[test]
    fn packaged_ppsr_script_predicts_with_a_small_fixture_model() {
        let names = vec![
            "g1_npsMech1sMax",
            "g2_streakMax",
            "g3_jackRunMax",
            "g4_chordMax",
            "g5_speedP95",
            "g9_handTravel2sMax",
            "g6_holdOverlapDensityMax",
            "g7_lineCount",
            "g8_burstMax",
            "g10_bigramRepeatRatio",
        ];
        let mut aug_names = names.clone();
        aug_names.extend([
            "x_noteTotal",
            "x_mechTotal",
            "x_duration",
            "x_fakeRatio",
            "x_logNoteTotal",
            "x_logDuration",
            "x_meanNps",
        ]);
        let mut median = aug_names
            .iter()
            .map(|name| ((*name).to_owned(), json!(0.)))
            .collect::<serde_json::Map<_, _>>();
        median.extend([
            ("meta_noteTotal".into(), json!(0.)),
            ("meta_mechTotal".into(), json!(0.)),
            ("meta_duration".into(), json!(0.)),
            ("meta_fakeRatio".into(), json!(0.)),
        ]);
        let mut part_profile = serde_json::Map::new();
        for key in [
            "density", "stream", "jack", "chord", "travel", "reach", "hold", "reading", "burst", "repeat",
        ] {
            part_profile.insert(key.into(), json!([10.06]));
        }
        let assets = json!({
            "model": {
                "names": names,
                "augNames": aug_names,
                "learner": {"base":10.0,"lr":0.06,"trees":[{"feature":0,"threshold":0.0,"left":{"leaf":1.0},"right":{"leaf":-1.0}}]},
                "deshrink": {"slope":1.0,"intercept":0.0},
                "tail": {"idx":[],"xMax":[],"coef":[],"gain":1.0,"maxAdd":5.0,"shape":"log","expK":18.0}
            },
            "profile": {"base":10.0,"median":median,"parts":part_profile}
        });
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        let engine = engine();
        let ast = engine.compile(script).unwrap();
        let mut runtime = Runtime {
            plugin: Installed {
                manifest: Manifest {
                    api: 2,
                    id: "phira-pro.ppsr".into(),
                    name: "PPSR".into(),
                    version: "1".into(),
                    author: String::new(),
                    description: String::new(),
                    permissions: vec!["chart.read".into()],
                    settings: json!({}),
                    assets,
                },
                script: script.into(),
                enabled: true,
                settings: json!({}),
            },
            engine,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let report = invoke(
            &mut runtime,
            "on_chart_selected",
            &json!({"features":vec![0.;10],"note_total":10,"mech_total":10,"duration":10.,"fake_ratio":0.,"difficulty":0.,"chart_set":"phira"}),
            "chart.read",
        )
        .expect("PPSR callback");
        assert_eq!(report["difficulty"], 10.06);
        let parts = report["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 10);
        assert!(parts.iter().all(|part| {
            let score = part["score"].as_f64().unwrap();
            (1.0..=10.0).contains(&score) && part["percentile"].as_f64().is_some()
        }));
        let official_report = invoke(
            &mut runtime,
            "on_chart_selected",
            &json!({
                "features":vec![0.;10],
                "note_total":10,
                "mech_total":10,
                "duration":10.,
                "fake_ratio":0.,
                "difficulty":16.68,
                "chart_set":"phigros"
            }),
            "chart.read",
        )
        .expect("official PPSR callback");
        assert!((official_report["difficulty"].as_f64().unwrap() - 16.68).abs() < 1e-9);
    }
    #[test]
    fn ppsr_plugin_manifest_and_script_pass_import_validation() {
        let manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/manifest.json")).unwrap();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        verify(&manifest, script).unwrap();
        assert_eq!(manifest.id, PPSR_PLUGIN_ID);
        assert_eq!(manifest.api, API_VERSION);
        assert!(manifest.permissions.iter().any(|permission| permission == "overlay.draw"));
    }
    #[test]
    fn ppsr_unified_cache_executes_in_rhai_with_a_complete_feature_vector() {
        let manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/manifest.json")).unwrap();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        verify(&manifest, script).unwrap();
        let engine = engine();
        let ast = engine.compile(script).unwrap();
        let mut runtime = Runtime {
            plugin: Installed {
                settings: manifest.settings.clone(),
                manifest: manifest.clone(),
                script: script.into(),
                enabled: true,
            },
            engine,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let report = invoke(
            &mut runtime,
            "on_chart_selected",
            &json!({
                "features": vec![0.; manifest.assets["model"]["names"].as_array().unwrap().len()],
                "note_total": 0,
                "mech_total": 0,
                "duration": 0.,
                "fake_ratio": 0.,
                "difficulty": 0.,
                "chart_set": "phira"
            }),
            "chart.read",
        )
        .expect("PPSR model callback");
        let difficulty = report["difficulty"].as_f64().unwrap();
        let expected = 4.896684037134319;
        assert!((difficulty - expected).abs() < 1e-9, "unexpected frozen-model result: {difficulty}");
        let parts = report["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 10);
        assert!(parts.iter().all(|part| (1.0..=10.0).contains(&part["score"].as_f64().unwrap())));
    }

    #[test]
    fn ppsr_median_profile_produces_non_floor_subscores() {
        let manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/manifest.json")).unwrap();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        let names = manifest.assets["model"]["names"].as_array().unwrap();
        let median = &manifest.assets["profile"]["median"];
        let features = names.iter().map(|name| median[name.as_str().unwrap()].clone()).collect::<Vec<_>>();
        let note_total = median["meta_noteTotal"].clone();
        let mech_total = median["meta_mechTotal"].clone();
        let duration = median["meta_duration"].clone();
        let fake_ratio = median["meta_fakeRatio"].clone();
        let engine = engine();
        let ast = engine.compile(script).unwrap();
        let mut runtime = Runtime {
            plugin: Installed {
                settings: manifest.settings.clone(),
                manifest,
                script: script.into(),
                enabled: true,
            },
            engine,
            ast,
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let report = invoke(
            &mut runtime,
            "on_chart_selected",
            &json!({
                "features":features,
                "note_total":note_total,
                "mech_total":mech_total,
                "duration":duration,
                "fake_ratio":fake_ratio,
                "difficulty":0.0,
                "chart_set":"phira"
            }),
            "chart.read",
        )
        .expect("median PPSR callback");
        let scores = report["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| (part["key"].as_str().unwrap().to_owned(), part["score"].as_f64().unwrap()))
            .collect::<Vec<_>>();
        let expected_difficulty = 18.053963110847416;
        assert!(
            (report["difficulty"].as_f64().unwrap() - expected_difficulty).abs() < 0.01,
            "median profile should predict its cached baseline: {report:?}"
        );
        let density_score = scores.iter().find(|(key, _)| key == "density").unwrap().1;
        assert!(scores.iter().any(|(_, score)| *score > 1.0), "all median subscores fell to the floor: {scores:?}");
        assert!((5.0..=6.5).contains(&density_score), "unexpected median density percentile: {density_score}");
    }

    #[test]
    fn ppsr_zip_import_keeps_embedded_model_and_profile_disabled_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("ppsr.zip");
        let index = dir.path().join("plugins/index.json");
        // Build the fixture from tracked sources; the distributable ZIP is not in Git.
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (name, contents) in [
                ("manifest.json", include_str!("../../docs/plugins/examples/ppsr/manifest.json")),
                ("main.rhai", include_str!("../../docs/plugins/examples/ppsr/main.rhai")),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(contents.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        install_package(&package, &index).unwrap();
        let installed = read_index_at(&index).unwrap();
        assert_eq!(installed.len(), 1);
        assert!(!installed[0].enabled);
        assert_eq!(installed[0].manifest.api, API_VERSION);
        assert_eq!(installed[0].manifest.assets["model"]["names"].as_array().unwrap().len(), 114);
        assert_eq!(installed[0].manifest.assets["profile"]["version"], 3);
        assert!(installed[0].script.contains("score: 1.0 + rank * 9.0"));
        assert!(!installed[0].script.contains("1.07"));
        assert!(installed[0].script.contains("ln(1.0 + ctx.note_total)"));
    }
    #[test]
    fn plugin_package_install_is_disabled_persistent_and_rejects_bad_updates() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("plugins/index.json");
        let package = dir.path().join("density.zip");
        let pack = |extra: bool| {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(include_bytes!("../../docs/plugins/examples/density/manifest.json"))
                .unwrap();
            zip.start_file("main.rhai", options).unwrap();
            zip.write_all(include_bytes!("../../docs/plugins/examples/density/main.rhai")).unwrap();
            if extra {
                zip.start_file("../extra", options).unwrap();
            }
            zip.finish().unwrap();
        };
        pack(false);
        install_package(&package, &index).unwrap();
        let list = read_index_at(&index).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);
        assert_eq!(list[0].settings["factor"], 1.0);
        let previous = std::fs::read(&index).unwrap();
        assert!(install_package(&package, &index).is_err(), "duplicate ID must preserve installed plugin");
        pack(true);
        assert!(install_package(&package, &index).is_err());
        assert_eq!(std::fs::read(&index).unwrap(), previous);
        let mut list = list;
        list[0].enabled = true;
        list[0].settings = json!({"factor":2.});
        write_index_at(&index, &list).unwrap();
        let list = read_index_at(&index).unwrap();
        assert!(list[0].enabled);
        assert_eq!(list[0].settings["factor"], 2.0);
    }
}
