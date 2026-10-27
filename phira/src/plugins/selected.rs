//! CPU-only chart analysis and Rhai execution, with publication on the UI thread.
use super::*;
use prpr::{fs::{FileSystem, fs_from_file}, info::ChartInfo, scene::GameScene};
use std::{any::Any, sync::Arc};

static WORKER: once_cell::sync::Lazy<Arc<tokio::sync::Semaphore>> = once_cell::sync::Lazy::new(|| Arc::new(tokio::sync::Semaphore::new(1)));

pub enum SelectedSource {
    Local(PathBuf),
    Embedded { info: ChartInfo, bytes: Vec<u8> },
}
pub struct SelectedJob {
    epoch: u64,
    request: u64,
    plugins: Vec<(Arc<Installed>, AST)>,
}
struct PluginOutput {
    plugin: Arc<Installed>,
    report: Option<Value>,
    error: Option<String>,
}
pub struct SelectedOutput {
    epoch: u64,
    request: u64,
    reports: Vec<PluginOutput>,
}
fn eligible(r: &Runtime) -> bool {
    r.error.is_none() && r.plugin.manifest.api >= 2
        && r.plugin.manifest.permissions.iter().any(|permission| permission == "chart.read")
        && r.ast.iter_functions().any(|f| f.name == "on_chart_selected" && f.params.len() == 2)
}
pub fn begin_chart_selected() -> SelectedJob {
    clear_chart_selected_reports();
    SelectedJob {
        epoch: RUNTIME_EPOCH.load(Ordering::Relaxed),
        request: SELECTED_REQUEST.load(Ordering::Relaxed),
        // Arc and AST clones share the immutable model/bytecode. Multi-megabyte
        // assets are never copied while opening a chart's details page.
        plugins: RUNTIME.with(|runtime| runtime.borrow().iter().filter(|r| eligible(r)).map(|r| (Arc::clone(&r.plugin), r.ast.clone())).collect()),
    }
}
impl SelectedJob {
    fn current(&self) -> bool {
        self.epoch == RUNTIME_EPOCH.load(Ordering::Relaxed) && self.request == SELECTED_REQUEST.load(Ordering::Relaxed)
    }
    fn empty(&self) -> SelectedOutput { SelectedOutput { epoch: self.epoch, request: self.request, reports: Vec::new() } }
    fn execute(self, source: SelectedSource, chart_set: &str) -> Result<SelectedOutput> {
        if !self.current() { return Ok(self.empty()); }
        let ppsr = self.plugins.iter().any(|(plugin, _)| plugin.manifest.id == PPSR_PLUGIN_ID);
        let payload = pollster::block_on(payload(source, chart_set, ppsr))?;
        let mut output = self.empty();
        for (plugin, ast) in &self.plugins {
            if !self.current() { return Ok(self.empty()); }
            let mut runtime = Runtime { plugin: Arc::clone(plugin), engine: engine(), ast: ast.clone(), report: Value::Null, error: None, frame_tick: -1 };
            let report = invoke(&mut runtime, "on_chart_selected", &payload, "chart.read");
            output.reports.push(PluginOutput { plugin: Arc::clone(plugin), report, error: runtime.error });
        }
        Ok(output)
    }
}
pub async fn run_selected(job: SelectedJob, source: SelectedSource, chart_set: &'static str) -> Result<SelectedOutput> {
    let permit = Arc::clone(&WORKER).acquire_owned().await.context("plugin worker stopped")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job.execute(source, chart_set)
    }).await.context("plugin analysis worker failed")?
}
pub fn publish_selected(output: SelectedOutput) -> bool {
    if output.epoch != RUNTIME_EPOCH.load(Ordering::Relaxed) || output.request != SELECTED_REQUEST.load(Ordering::Relaxed) { return false; }
    RUNTIME.with(|runtime| {
        for result in output.reports {
            if let Some(active) = runtime.borrow_mut().iter_mut().find(|active| Arc::ptr_eq(&active.plugin, &result.plugin) && active.error.is_none()) {
                if let Some(report) = result.report { active.report = report; }
                active.error = result.error;
            }
        }
    });
    true
}

async fn payload(source: SelectedSource, chart_set: &str, ppsr: bool) -> Result<Value> {
    let (mut fs, mut info): (Box<dyn FileSystem>, ChartInfo) = match source {
        SelectedSource::Local(path) => {
            let mut fs = fs_from_file(&path)?;
            let info = prpr::fs::load_info(fs.as_mut()).await?;
            (fs, info)
        }
        SelectedSource::Embedded { info, bytes } => {
            let fs = ChartBytes { path: info.chart.clone(), bytes: Arc::new(bytes) };
            (Box::new(fs), info)
        }
    };
    let bytes = GameScene::load_chart_bytes(fs.as_mut(), &info).await?;
    let format = GameScene::infer_chart_format(&info, &bytes);
    info.format = Some(format.clone());
    let truncated = bytes.len() > MAX_CHART_JSON_SIZE;
    let raw: Value = if truncated { Value::Null } else { serde_json::from_slice::<Value>(&bytes).ok().filter(Value::is_object).unwrap_or(Value::Null) };
    let mut payload = if ppsr {
        let (chart, _, _) = GameScene::load_chart_for_analysis(fs.as_mut(), &info).await?;
        crate::ppsr_features::extract(&chart, &info)?
    } else { json!({}) };
    let object = payload.as_object_mut().context("selected-chart payload must be an object")?;
    object.insert("api".into(), json!(API_VERSION));
    object.insert("name".into(), json!(info.name));
    object.insert("level".into(), json!(info.level));
    object.insert("difficulty".into(), json!(info.difficulty));
    object.insert("format".into(), serde_json::to_value(format)?);
    object.insert("chart_set".into(), json!(chart_set));
    object.insert("chart_data".into(), raw);
    object.insert("raw_truncated".into(), json!(truncated));
    Ok(payload)
}

#[derive(Clone)]
struct ChartBytes { path: String, bytes: Arc<Vec<u8>> }
#[async_trait::async_trait]
impl FileSystem for ChartBytes {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> { anyhow::ensure!(path == self.path, "analysis asset unavailable: {path}"); Ok(self.bytes.as_ref().clone()) }
    async fn exists(&mut self, path: &str) -> Result<bool> { Ok(path == self.path) }
    fn list_root(&self) -> Result<Vec<String>> { Ok(vec![self.path.clone()]) }
    fn clone_box(&self) -> Box<dyn FileSystem> { Box::new(self.clone()) }
    fn as_any(&mut self) -> &mut dyn Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cpu_analysis_ignores_gpu_assets_and_worker_does_not_block_runtime() {
        let mut chart: Value = serde_json::from_str(include_str!("../../../prpr/tests/fixtures/rpe-next/chart.json")).unwrap();
        chart["judgeLineList"][0]["Texture"] = json!("missing-texture.png");
        chart["effects"] = json!([{ "name": "missing-shader" }]);
        let info = ChartInfo { chart: "chart.json".into(), format: Some(prpr::info::ChartFormat::Rpe), ..Default::default() };
        let source = SelectedSource::Embedded { info, bytes: serde_json::to_vec(&chart).unwrap() };
        let output = tokio::task::spawn_blocking(move || pollster::block_on(payload(source, "phira", true)));
        tokio::time::timeout(std::time::Duration::from_secs(1), tokio::task::yield_now()).await.unwrap();
        let result = output.await.unwrap().unwrap();
        assert_eq!(result["features"].as_array().unwrap().len(), 187);
    }
    #[test]
    fn obsolete_worker_results_do_not_publish() {
        let output = SelectedOutput { epoch: RUNTIME_EPOCH.load(Ordering::Relaxed).wrapping_sub(1), request: SELECTED_REQUEST.load(Ordering::Relaxed), reports: vec![] };
        assert!(!publish_selected(output));
    }
}
