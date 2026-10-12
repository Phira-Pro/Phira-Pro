//! Versioned, observer-only Rhai plugins. No native loading, filesystem, network,
//! account credentials, mutable chart, judgement, score or upload API is exposed.
use anyhow::{Context, Result};
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
pub const API_VERSION: u32 = 1;
pub const API_LABEL: &str = "Phira Pro Plugin API v1";
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
    #[serde(default = "empty_settings")]
    pub settings: Value,
}
fn empty_settings() -> Value {
    json!({})
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub script: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "empty_settings")]
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
    anyhow::ensure!(m.api == API_VERSION, "不支持 {} v{}；当前支持 {}", API_NAME, m.api, API_LABEL);
    anyhow::ensure!(!m.id.is_empty() && m.id.len() <= 80 && m.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)), "插件 ID 无效");
    anyhow::ensure!(!m.name.trim().is_empty() && m.name.len() <= 160 && m.version.len() <= 80 && m.description.len() <= 2048, "插件名称或描述无效");
    anyhow::ensure!(m.settings.is_object() && serde_json::to_vec(&m.settings)?.len() <= 16 << 10, "插件 settings 必须是小于 16KiB 的 JSON 对象");
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
    if !r.ast.iter_functions().any(|f| f.name == name && f.params.len() == 2) {
        return None;
    }
    r.engine.set_max_operations(if name == "on_frame" { 25_000 } else { 5_000_000 });
    let call = (|| -> Result<Value> {
        let ctx = rhai::serde::to_dynamic(payload)?;
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
        r.ast = r.engine.compile("fn on_chart(ctx, settings) { loop {} }").unwrap();
        assert!(invoke(&mut r, "on_chart", &json!({}), "chart.read").is_none());
        assert!(r.error.is_some());
        assert!(engine().eval::<Dynamic>("import \"/etc/passwd\" as p;").is_err());
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
