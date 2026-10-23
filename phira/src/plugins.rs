//! Versioned, observer-only Rhai plugins. No native loading, filesystem, network,
//! account credentials, mutable chart, judgement, score or upload API is exposed.
use anyhow::{Context, Result};
use macroquad::prelude::{Color, Rect, Texture2D, Touch, TouchPhase, Vec2};
use prpr::{
    ext::{RectExt, ScaleType},
    ui::{DRectButton, RectButton, Ui},
};
use rhai::{Dynamic, Engine, Scope, AST};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_PACKAGE: u64 = 2 << 20;
const MAX_ASSET_SIZE: u64 = 8 << 20;
const MAX_ASSETS_SIZE: usize = 8 << 20;
const MAX_EXPANDED_PACKAGE: u64 = 10 << 20;
const MAX_INDEX_SIZE: usize = 24 << 20;
pub const DEFAULT_OPERATION_LIMIT: u64 = 5_000_000;
static OPERATION_LIMIT: AtomicU64 = AtomicU64::new(DEFAULT_OPERATION_LIMIT);
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
                TouchPhase::Moved | TouchPhase::Stationary | TouchPhase::Ended => {
                    if let Some(bounds) = self.bounds {
                        let delta = touch.position - start_touch;
                        let x = (start_position.x + delta.x).clamp(-1.0, 1.0 - bounds.w);
                        let y = (start_position.y + delta.y).clamp(-self.ui_top, (self.ui_top - bounds.h).max(-self.ui_top));
                        self.position = Some(Vec2::new(x, y));
                    }
                    // Fast desktop drags may deliver down/up together without a moved frame.
                    if touch.phase == TouchPhase::Ended {
                        self.drag = None;
                    }
                }
                TouchPhase::Cancelled => self.drag = None,
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
    /// Root-level JSON files loaded into `assets` when importing a package.
    #[serde(default, rename = "assetFiles")]
    pub asset_files: BTreeMap<String, String>,
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
    e.set_max_operations(DEFAULT_OPERATION_LIMIT)
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
fn operation_limit_path() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join("plugins/operation_limit.json"))
}
pub fn operation_limit() -> u64 {
    OPERATION_LIMIT.load(Ordering::Relaxed)
}
fn parse_operation_limit(input: &str) -> Option<u64> {
    let input = input.trim();
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    input.parse::<u64>().ok().filter(|limit| *limit > 0 && *limit <= i64::MAX as u64)
}
fn callback_operation_limit(name: &str, configured: u64) -> u64 {
    if name == "on_frame" {
        25_000
    } else {
        configured
    }
}
fn load_operation_limit(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(DEFAULT_OPERATION_LIMIT);
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok(value
        .get("operations")
        .and_then(Value::as_u64)
        .filter(|limit| *limit > 0 && *limit <= i64::MAX as u64)
        .unwrap_or(DEFAULT_OPERATION_LIMIT))
}
fn save_operation_limit(path: &Path, input: &str) -> Result<bool> {
    let parsed = parse_operation_limit(input);
    let limit = parsed.unwrap_or(DEFAULT_OPERATION_LIMIT);
    std::fs::create_dir_all(path.parent().context("plugin settings path has no parent")?)?;
    crate::transfer::write_atomic(&path, &serde_json::to_vec(&json!({"operations":limit}))?)?;
    Ok(parsed.is_some())
}
pub fn set_operation_limit(input: &str) -> Result<bool> {
    let path = operation_limit_path()?;
    let valid = save_operation_limit(&path, input)?;
    let limit = if valid {
        parse_operation_limit(input).unwrap()
    } else {
        DEFAULT_OPERATION_LIMIT
    };
    OPERATION_LIMIT.store(limit, Ordering::Relaxed);
    Ok(valid)
}
fn init_operation_limit() {
    match operation_limit_path().and_then(|path| load_operation_limit(&path)) {
        Ok(limit) => OPERATION_LIMIT.store(limit, Ordering::Relaxed),
        Err(error) => {
            OPERATION_LIMIT.store(DEFAULT_OPERATION_LIMIT, Ordering::Relaxed);
            tracing::warn!(?error, "cannot read plugin operation limit; using default");
        }
    }
}
fn read_index() -> Result<Vec<Installed>> {
    read_index_at(&index_path()?)
}
fn read_index_at(path: &Path) -> Result<Vec<Installed>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    anyhow::ensure!(std::fs::metadata(&path)?.len() <= MAX_INDEX_SIZE as u64, "插件索引过大");
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn write_index(plugins: &[Installed]) -> Result<()> {
    write_index_at(&index_path()?, plugins)
}
fn write_index_at(path: &Path, plugins: &[Installed]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let bytes = serde_json::to_vec_pretty(plugins)?;
    anyhow::ensure!(bytes.len() <= MAX_INDEX_SIZE, "插件库超过 24MiB，请先删除不需要的插件");
    crate::transfer::write_atomic(&path, &bytes)
}
fn verify(m: &Manifest, script: &str) -> Result<()> {
    anyhow::ensure!(matches!(m.api, 1 | API_VERSION), "不支持 {} v{}；当前支持 {}（并兼容 v1）", API_NAME, m.api, API_LABEL);
    anyhow::ensure!(!m.id.is_empty() && m.id.len() <= 80 && m.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)), "插件 ID 无效");
    anyhow::ensure!(!m.name.trim().is_empty() && m.name.len() <= 160 && m.version.len() <= 80 && m.description.len() <= 2048, "插件名称或描述无效");
    anyhow::ensure!(m.settings.is_object() && serde_json::to_vec(&m.settings)?.len() <= 16 << 10, "插件 settings 必须是小于 16KiB 的 JSON 对象");
    anyhow::ensure!(
        m.assets.is_object() && serde_json::to_vec(&m.assets)?.len() <= MAX_ASSETS_SIZE,
        "插件 assets 必须是小于 8MiB 的 JSON 对象"
    );
    anyhow::ensure!(m.api >= 2 || m.asset_files.is_empty(), "只有 API v2 插件可以声明 assetFiles");
    for (key, name) in &m.asset_files {
        anyhow::ensure!(
            !key.is_empty() && key.len() <= 80 && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "插件 assetFiles 键无效"
        );
        anyhow::ensure!(
            !name.is_empty()
                && name.len() <= 128
                && name.ends_with(".json")
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && name != ".json",
            "插件资源文件名无效：{name}"
        );
    }
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
    anyhow::ensure!(zip.len() >= 2, "插件包缺少必需文件");
    let expanded_size = (0..zip.len()).try_fold(0_u64, |total, index| {
        let file = zip.by_index(index)?;
        total.checked_add(file.size()).context("插件内容大小溢出")
    })?;
    anyhow::ensure!(expanded_size <= MAX_EXPANDED_PACKAGE, "插件解压后总大小超过 10MiB");
    fn entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64) -> Result<String> {
        let file = zip.by_name(name).with_context(|| format!("缺少 {name}"))?;
        anyhow::ensure!(!file.is_dir(), "插件条目必须是文件：{name}");
        anyhow::ensure!(file.size() <= limit, "插件内容过大");
        let mut text = String::new();
        file.take(limit + 1).read_to_string(&mut text)?;
        anyhow::ensure!(text.len() as u64 <= limit, "插件内容过大");
        Ok(text)
    }
    let mut manifest: Manifest = serde_json::from_str(&entry(&mut zip, "manifest.json", MAX_PACKAGE)?)?;
    let expected = std::iter::once("manifest.json".to_owned())
        .chain(std::iter::once("main.rhai".to_owned()))
        .chain(manifest.asset_files.values().cloned())
        .collect::<BTreeSet<_>>();
    anyhow::ensure!(expected.len() == 2 + manifest.asset_files.len(), "插件资源文件名重复");
    anyhow::ensure!(
        manifest.assets.is_object() && manifest.asset_files.keys().all(|key| manifest.assets.get(key).is_none()),
        "assetFiles 键与内嵌 assets 重复"
    );
    let mut actual = BTreeSet::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index)?;
        anyhow::ensure!(!file.is_dir(), "插件包不允许目录条目");
        anyhow::ensure!(actual.insert(file.name().to_owned()), "插件包包含重复条目");
    }
    anyhow::ensure!(actual == expected, "插件包只允许 manifest.json、main.rhai 和 Manifest 声明的 JSON 资源文件");
    let script = entry(&mut zip, "main.rhai", MAX_PACKAGE)?;
    for (key, name) in &manifest.asset_files {
        let asset: Value = serde_json::from_str(&entry(&mut zip, name, MAX_ASSET_SIZE)?)
            .with_context(|| format!("插件资源不是有效 JSON：{name}"))?;
        manifest.assets.as_object_mut().unwrap().insert(key.clone(), asset);
    }
    anyhow::ensure!(serde_json::to_vec(&manifest.assets)?.len() <= MAX_ASSETS_SIZE, "插件 assets 超过 8MiB");
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
    init_operation_limit();
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
    r.engine.set_max_operations(callback_operation_limit(name, operation_limit()));
    let call = (|| -> Result<Value> {
        let mut payload = payload.clone();
        if name == "on_chart" || name == "on_chart_selected" {
            if let Some(ctx) = payload.as_object_mut() {
                ctx.insert("api".into(), json!(r.plugin.manifest.api));
                ctx.insert("operation_limit".into(), json!(operation_limit()));
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
pub fn chart_selected_ppsr_features_needed() -> bool {
    RUNTIME.with(|runtime| {
        runtime.borrow().iter().any(|r| {
            r.plugin.manifest.id == PPSR_PLUGIN_ID
                && r.plugin.manifest.api >= 2
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
fn selected_chart_report() -> Option<(String, Value)> {
    RUNTIME.with(|runtime| {
        let runtime = runtime.borrow();
        let eligible = |r: &&Runtime| {
            r.plugin.manifest.api >= 2
                && r.error.is_none()
                && r.plugin.manifest.permissions.iter().any(|permission| permission == "chart.read")
                && r.plugin.manifest.permissions.iter().any(|permission| permission == "overlay.draw")
                && r.ast
                    .iter_functions()
                    .any(|function| function.name == "on_chart_selected" && function.params.len() == 2)
                && r.report.get("difficulty").and_then(Value::as_f64).is_some()
        };
        let plugin = runtime
            .iter()
            .filter(eligible)
            .find(|r| r.plugin.manifest.id == PPSR_PLUGIN_ID)
            .or_else(|| runtime.iter().filter(eligible).next())?;
        let title = plugin.report.get("title").and_then(Value::as_str).unwrap_or(&plugin.plugin.manifest.name).to_owned();
        Some((title, plugin.report.clone()))
    })
}
pub fn render_plugin_difficulty_panel(
    ui: &mut Ui,
    t: f32,
    artwork: Texture2D,
    collapsed: bool,
    toggle: &mut DRectButton,
    panel: &mut PpsrPanelState,
) -> bool {
    let Some((title, report)) = selected_chart_report() else {
        panel.bounds = None;
        panel.drag = None;
        return false;
    };
    let parts = report.get("parts").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let width = 0.72;
    let row_height = 0.056;
    let height = 0.17 + if collapsed { 0. } else { parts.len().min(10) as f32 * row_height + 0.04 };
    let position = panel.position.unwrap_or(Vec2::new(0.98 - width, ui.top - height - 0.08));
    let rect = Rect::new(position.x.clamp(-1.0, 1.0 - width), position.y.clamp(-ui.top, (ui.top - height).max(-ui.top)), width, height);
    panel.position = Some(Vec2::new(rect.x, rect.y));
    panel.bounds = Some(rect);
    panel.ui_top = ui.top;
    let header_height = 0.15_f32.min(height);
    panel.drag_handle.set(ui, Rect::new(rect.x, rect.y, width - 0.18, header_height));
    // The host supplies the selected chart's artwork; plugins do not need texture access.
    ui.fill_path(&rect.rounded(0.02), (artwork, rect, ScaleType::CropCenter, Color::new(1., 1., 1., 0.82)));
    ui.fill_path(&rect.rounded(0.02), Color::new(0.035, 0.045, 0.075, 0.72));
    ui.stroke_path(&rect.rounded(0.02), 0.0015, Color::new(1., 1., 1., 0.14));
    ui.text(&title)
        .pos(rect.x + 0.025, rect.y + 0.019)
        .size(0.28)
        .color(Color::new(0.83, 0.88, 0.98, 1.))
        .draw();
    let difficulty = format!("{:.2}", report["difficulty"].as_f64().unwrap_or_default());
    let badge = Rect::new(rect.x + 0.025, rect.y + 0.057, 0.25, 0.090);
    ui.fill_path(&badge.rounded(0.022), Color::new(0.15, 0.20, 0.33, 0.65));
    ui.stroke_path(&badge.rounded(0.022), 0.0015, Color::new(1., 1., 1., 0.16));
    let bounds = ui.text(&difficulty).size(1.).no_baseline().measure();
    let size = (0.058 / bounds.h.max(0.001)).min((badge.w - 0.028) / bounds.w.max(0.001));
    ui.text(&difficulty)
        .pos(badge.center().x, badge.center().y)
        .anchor(0.5, 0.5)
        .no_baseline()
        .size(size)
        .draw();
    let toggle_rect = Rect::new(rect.right() - 0.16, rect.y + 0.023, 0.14, 0.06);
    toggle.render_text(ui, toggle_rect, t, if collapsed { "展开" } else { "收起" }, 0.25, false);
    if !collapsed {
        ui.text("细分难度")
            .pos(rect.x + 0.025, rect.y + 0.167)
            .size(0.29)
            .color(macroquad::prelude::Color::new(1., 1., 1., 0.72))
            .draw();
        for (index, part) in parts.iter().take(10).enumerate() {
            let label = part["label"].as_str().or_else(|| part["key"].as_str()).unwrap_or("—");
            let score = part["score"].as_f64().unwrap_or(1.0).clamp(1.0, 10.0);
            let y = rect.y + 0.205 + index as f32 * row_height;
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
    fn operation_limit_rejects_invalid_values_and_persists_default() {
        for input in ["", "0", "-1", "1.5", "+12", "12x", "18446744073709551616"] {
            assert_eq!(parse_operation_limit(input), None, "{input}");
        }
        assert_eq!(parse_operation_limit(" 12345 "), Some(12_345));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins/operation_limit.json");
        assert!(save_operation_limit(&path, "12345").unwrap());
        assert_eq!(load_operation_limit(&path).unwrap(), 12_345);
        assert!(!save_operation_limit(&path, "-1").unwrap());
        assert_eq!(load_operation_limit(&path).unwrap(), DEFAULT_OPERATION_LIMIT);
    }
    #[test]
    fn frame_callback_keeps_its_fixed_operation_limit() {
        assert_eq!(callback_operation_limit("on_frame", 12_345), 25_000);
        assert_eq!(callback_operation_limit("on_chart_selected", 12_345), 12_345);
        assert_eq!(callback_operation_limit("on_result", 12_345), 12_345);
    }
    #[test]
    fn chart_callback_receives_the_current_operation_limit() {
        let mut manifest = ppsr_manifest();
        manifest.permissions = vec!["chart.read".into()];
        let script = "fn on_chart_selected(ctx, settings) { #{limit: ctx.operation_limit} }";
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
        let report = invoke(&mut runtime, "on_chart_selected", &json!({}), "chart.read").unwrap();
        assert_eq!(report["limit"], operation_limit());
        assert_eq!(runtime.engine.max_operations(), operation_limit());
    }
    fn ppsr_manifest() -> Manifest {
        let mut manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/manifest.json")).unwrap();
        manifest.assets["model"] = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/model.json")).unwrap();
        manifest.assets["profile"] = serde_json::from_str(include_str!("../../docs/plugins/examples/ppsr/profile.json")).unwrap();
        manifest
    }
    fn pde_r_manifest() -> Manifest {
        let mut manifest: Manifest = serde_json::from_str(include_str!("../../docs/plugins/examples/pde-r/manifest.json")).unwrap();
        manifest.assets["model"] = serde_json::from_str(include_str!("../../docs/plugins/examples/pde-r/model.json")).unwrap();
        manifest
    }
    #[test]
    fn pde_r_plugin_owns_its_feature_extractor_and_accepts_selected_chart_data() {
        let manifest = pde_r_manifest();
        let script = include_str!("../../docs/plugins/examples/pde-r/main.rhai");
        verify(&manifest, script).unwrap();
        assert_eq!(manifest.id, "phira-pro.pde-r");
        assert_ne!(manifest.id, PPSR_PLUGIN_ID);
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
            &json!({"chart_data":null,"raw":{"sentinel":true}}),
            "chart.read",
        )
        .unwrap_or_else(|| panic!("PDE-R selected-chart callback suspended: {:?}", runtime.error));
        assert_eq!(report["failed"], true);
        assert!(report["text"].as_str().unwrap().contains("原始"));
        assert!(runtime.error.is_none());
    }
    #[test]
    fn pde_r_plugin_extracts_and_scores_its_own_106_features_on_chart_selection() {
        let manifest = pde_r_manifest();
        let script = include_str!("../../docs/plugins/examples/pde-r/main.rhai");
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
        let mut chart_data: Value =
            serde_json::from_str(include_str!("../../prpr/tests/fixtures/rpe-next/chart.json")).unwrap();
        let notes = chart_data["judgeLineList"][0]["notes"].as_array_mut().unwrap();
        let template = notes[0].clone();
        for beat in 20..28 {
            let mut note = template.clone();
            note["startTime"] = json!([beat, 0, 1]);
            note["endTime"] = json!([beat, 0, 1]);
            notes.push(note);
        }
        let report = invoke(
            &mut runtime,
            "on_chart_selected",
            &json!({"chart_data":chart_data}),
            "chart.read",
        )
        .unwrap_or_else(|| panic!("PDE-R feature/model callback suspended: {:?}", runtime.error));
        assert_ne!(report["failed"], true, "PDE-R rejected a supported RPE chart: {report:?}");
        assert_eq!(report["features"], 106);
        assert_eq!(report["feature_map"].as_object().unwrap().len(), 106);
        assert!(report["difficulty"].as_f64().unwrap().is_finite());
        assert!(runtime.error.is_none());
    }
    #[test]
    fn pde_r_plugin_zip_imports_with_its_own_model_cache() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("pde-r.zip");
        let index = dir.path().join("plugins/index.json");
        std::fs::write(&package, include_bytes!("../../docs/plugins/Phira-Pro-PDE-R.zip")).unwrap();
        install_package(&package, &index).unwrap();
        let installed = read_index_at(&index).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].manifest.id, "phira-pro.pde-r");
        assert_eq!(installed[0].manifest.assets["model"]["columns"].as_array().unwrap().len(), 106);
        assert_eq!(installed[0].manifest.asset_files["model"], "model.json");
        assert!(installed[0].script.contains("fn on_chart_selected"));
    }
    fn ppsr_names() -> Vec<String> {
        crate::ppsr_features::BASE_FEATURE_NAMES
            .iter()
            .chain(crate::ppsr_features::V2_FEATURE_NAMES.iter())
            .chain(crate::ppsr_features::AUGMENTED_FEATURE_NAMES.iter())
            .map(|name| (*name).to_owned())
            .collect()
    }
    fn ppsr_ctx(names: &[String], features: Vec<Value>) -> Value {
        json!({
            "feature_version": 2,
            "v2_feature_version": 1,
            "feature_names": names,
            "features": features,
            "note_total": 10,
            "mech_total": 8,
            "duration": 10.0,
            "fake_ratio": 0.0,
            "difficulty": 0.0,
            "chart_set": "phira"
        })
    }

    #[test]
    fn ppsr_fast_drag_uses_release_position() {
        let mut panel = PpsrPanelState {
            position: Some(Vec2::new(0.2, -0.3)),
            bounds: Some(Rect::new(0.2, -0.3, 0.72, 0.17)),
            drag: Some((7, Vec2::new(0.3, -0.2), Vec2::new(0.2, -0.3))),
            ui_top: 0.6,
            ..Default::default()
        };
        let mut release = Touch {
            id: 8,
            phase: TouchPhase::Ended,
            position: Vec2::new(0., 0.1),
            time: 0.,
        };
        assert!(!panel.touch(&release), "another finger must not finish the drag");
        release.id = 7;
        assert!(panel.touch(&release));
        let position = panel.position.unwrap();
        assert!((position.x + 0.1).abs() < 1e-6 && position.y.abs() < 1e-6);
        assert!(panel.drag.is_none());
    }
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
                asset_files: BTreeMap::new(),
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
                asset_files: BTreeMap::new(),
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
        let names = ppsr_names();
        let median = names
            .iter()
            .map(|name| ((*name).to_owned(), json!(0.)))
            .collect::<serde_json::Map<_, _>>();
        let feature_distributions = names
            .iter()
            .map(|name| ((*name).to_owned(), json!([0.])))
            .collect::<serde_json::Map<_, _>>();
        let directions = names
            .iter()
            .map(|name| ((*name).to_owned(), json!(1)))
            .collect::<serde_json::Map<_, _>>();
        let mut part_profile = serde_json::Map::new();
        for key in [
            "density", "stream", "jack", "chord", "travel", "reach", "hold", "reading", "burst", "repeat",
        ] {
            part_profile.insert(key.into(), json!([1.0]));
        }
        let assets = json!({
            "model": {
                "format":"phira-ppsr-model",
                "version":2,
                "featureVersion":2,
                "v2FeatureVersion":1,
                "modelTag":"fixture",
                "names": names,
                "bag":{"models":[{"base":10.0,"lr":0.06,"featureCount":187,"trees":[{"feature":0,"threshold":0.0,"left":{"leaf":1.0},"right":{"leaf":-1.0}}]}]},
                "deshrink": {"slope":1.0,"intercept":0.0},
                "tail": {"idx":[],"xMax":[],"coef":[],"gain":1.0,"maxAdd":5.0,"shape":"log","expK":18.0}
            },
            "profile": {"format":"phira-ppsr-profile","version":1,"featureVersion":2,"v2FeatureVersion":1,
                "modelTag":"fixture","names":names,"base":10.0,"median":median,"features":feature_distributions,
                "directions":directions,"parts":part_profile}
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
                    asset_files: BTreeMap::new(),
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
            &ppsr_ctx(&ppsr_names(), vec![json!(0.); 187]),
            "chart.read",
        )
        .unwrap_or_else(|| panic!("PPSR callback suspended: {:?}", runtime.error));
        assert_eq!(report["difficulty"], 10.06);
        let parts = report["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 10);
        assert!(parts.iter().all(|part| {
            let score = part["score"].as_f64().unwrap();
            (1.0..=10.0).contains(&score) && part["percentile"].as_f64().is_some()
        }));
        let mut official_ctx = ppsr_ctx(&ppsr_names(), vec![json!(0.); 187]);
        official_ctx["difficulty"] = json!(16.68);
        official_ctx["chart_set"] = json!("phigros");
        let official_report = invoke(
            &mut runtime,
            "on_chart_selected",
            &official_ctx,
            "chart.read",
        )
        .expect("official PPSR callback");
        assert!((official_report["difficulty"].as_f64().unwrap() - 16.68).abs() < 1e-9);
    }
    #[test]
    fn ppsr_plugin_manifest_and_script_pass_import_validation() {
        let manifest = ppsr_manifest();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        verify(&manifest, script).unwrap();
        assert_eq!(manifest.id, PPSR_PLUGIN_ID);
        assert_eq!(manifest.api, API_VERSION);
        assert!(manifest.permissions.iter().any(|permission| permission == "overlay.draw"));

        let mut invalid = manifest;
        invalid.asset_files.insert("model".into(), "../model.json".into());
        assert!(verify(&invalid, script).is_err(), "asset file paths must not escape the ZIP root");
    }
    #[test]
    fn ppsr_unified_cache_executes_in_rhai_with_a_complete_feature_vector() {
        let manifest = ppsr_manifest();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        verify(&manifest, script).unwrap();
        let expected_names = ppsr_names();
        let model_names = manifest.assets["model"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(model_names, expected_names);
        assert_eq!(manifest.assets["profile"]["names"], manifest.assets["model"]["names"]);
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
                "feature_version": 2,
                "v2_feature_version": 1,
                "feature_names": manifest.assets["model"]["names"],
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
        .unwrap_or_else(|| panic!("PPSR model callback suspended: {:?}", runtime.error));
        let difficulty = report["difficulty"].as_f64().unwrap();
        let expected = 1.6317731664795476;
        assert!(
            difficulty.is_finite() && (difficulty - expected).abs() < 1e-9,
            "unexpected frozen-model parity result: {difficulty}"
        );
        let parts = report["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 10);
        assert!(parts.iter().all(|part| (1.0..=10.0).contains(&part["score"].as_f64().unwrap())));
    }

    #[test]
    fn ppsr_median_profile_produces_non_floor_subscores() {
        let manifest = ppsr_manifest();
        let script = include_str!("../../docs/plugins/examples/ppsr/main.rhai");
        let names = manifest.assets["model"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let median = &manifest.assets["profile"]["median"];
        let features = names.iter().map(|name| median[name].clone()).collect::<Vec<_>>();
        let note_total = median["x_noteTotal"].clone();
        let mech_total = median["x_mechTotal"].clone();
        let duration = median["x_duration"].clone();
        let fake_ratio = median["x_fakeRatio"].clone();
        let expected_difficulty = manifest.assets["profile"]["base"].as_f64().unwrap();
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
                "feature_version": 2,
                "v2_feature_version": 1,
                "feature_names": names,
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
        assert!(
            (report["difficulty"].as_f64().unwrap() - expected_difficulty).abs() < 0.01,
            "median profile should predict its cached baseline: {report:?}"
        );
        assert!(scores.iter().any(|(_, score)| *score > 1.0), "all median subscores fell to the floor: {scores:?}");
        assert!(scores.iter().all(|(_, score)| (1.0..=10.0).contains(score)));
    }

    #[test]
    fn ppsr_zip_import_loads_replaceable_model_and_profile_files() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("ppsr.zip");
        let index = dir.path().join("plugins/index.json");
        std::fs::write(&package, include_bytes!("../../docs/plugins/Phira-Pro-PPSR.zip")).unwrap();
        install_package(&package, &index).unwrap();
        let installed = read_index_at(&index).unwrap();
        assert_eq!(installed.len(), 1);
        assert!(!installed[0].enabled);
        assert_eq!(installed[0].manifest.api, API_VERSION);
        assert_eq!(installed[0].manifest.asset_files["model"], "model.json");
        assert_eq!(installed[0].manifest.asset_files["profile"], "profile.json");
        assert_eq!(installed[0].manifest.assets["model"]["names"].as_array().unwrap().len(), 187);
        assert_eq!(installed[0].manifest.assets["profile"]["version"], 1);
        assert!(installed[0].script.contains("score: 1.0 + rank * 9.0"));
        assert!(!installed[0].script.contains("1.07"));
        assert!(installed[0].script.contains("PPSR 特征版本或维数与模型缓存不匹配"));
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
