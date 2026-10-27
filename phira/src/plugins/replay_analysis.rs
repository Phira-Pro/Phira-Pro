//! Replay capability: selected tape only; no file paths or mutable game state.
use super::*;
use crate::replay::analysis::{Control, Dataset};

#[derive(Clone)]
pub struct Analyzer {
    pub name: String,
    pub id: String,
    plugin: Arc<Installed>,
    ast: AST,
    epoch: u64,
}

pub fn available() -> Vec<Analyzer> {
    RUNTIME.with(|runtime| {
        runtime
            .borrow()
            .iter()
            .filter(|r| {
                r.error.is_none()
                    && r.plugin.manifest.api >= 3
                    && ["replay.read", "report.render"]
                        .iter()
                        .all(|p| r.plugin.manifest.permissions.iter().any(|v| v == p))
                    && r.ast.iter_functions().any(|f| f.name == "on_replay_analysis" && f.params.len() == 2)
            })
            .map(|r| Analyzer {
                name: r.plugin.manifest.name.clone(),
                id: r.plugin.manifest.id.clone(),
                plugin: Arc::clone(&r.plugin),
                ast: r.ast.clone(),
                epoch: RUNTIME_EPOCH.load(Ordering::Relaxed),
            })
            .collect()
    })
}

impl Analyzer {
    pub fn current(&self) -> bool {
        self.epoch == RUNTIME_EPOCH.load(Ordering::Relaxed)
    }
    pub fn version(&self) -> &str {
        &self.plugin.manifest.version
    }
    pub fn settings(&self) -> &Value {
        &self.plugin.settings
    }
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(serde_json::to_vec(&*self.plugin).unwrap_or_default()))
    }
    pub fn analyze(&self, data: Arc<Dataset>, control: &Control) -> Result<Value> {
        control.check()?;
        anyhow::ensure!(self.current(), "插件已变更，请重新分析");
        let mut engine = engine();
        let stop = control.clone();
        engine.on_progress(move |_| stop.check().err().map(|e| Dynamic::from(e.to_string())));
        // Pages use native JSON records and enforce bounds before allocating.
        let inputs = Arc::clone(&data);
        engine.register_fn("replay_inputs", move |start: i64, count: i64| -> rhai::Array {
            inputs
                .input_page(start, count)
                .into_iter()
                .filter_map(|v| rhai::serde::to_dynamic(&v).ok())
                .collect()
        });
        let judges = Arc::clone(&data);
        engine.register_fn("replay_judgements", move |start: i64, count: i64| -> rhai::Array {
            judges
                .judge_page(start, count)
                .into_iter()
                .filter_map(|v| rhai::serde::to_dynamic(&v).ok())
                .collect()
        });
        let observations = Arc::clone(&data);
        engine.register_fn("replay_observations", move |kind: &str, start: i64, count: i64| -> rhai::Array {
            observations
                .observation_page(kind, start, count)
                .into_iter()
                .filter_map(|v| rhai::serde::to_dynamic(&v).ok())
                .collect()
        });
        let mut runtime = Runtime {
            plugin: Arc::clone(&self.plugin),
            engine,
            ast: self.ast.clone(),
            report: Value::Null,
            error: None,
            frame_tick: -1,
        };
        let value = invoke(&mut runtime, "on_replay_analysis", &data.plugin_payload(), "replay.read")
            .with_context(|| runtime.error.unwrap_or_else(|| "插件未返回分析报告".into()))?;
        control.check()?;
        anyhow::ensure!(self.current(), "插件已变更，已丢弃旧分析结果");
        crate::replay::analysis::validate_report(&value)?;
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        let manifest: Manifest = serde_json::from_str(include_str!("../../../docs/plugins/examples/prca/manifest.json")).unwrap();
        let script = include_str!("../../../docs/plugins/examples/prca/main.rhai").to_owned();
        verify(&manifest, &script).unwrap();
        let ast = engine().compile(&script).unwrap();
        Self {
            name: manifest.name.clone(),
            id: manifest.id.clone(),
            plugin: Arc::new(Installed {
                settings: manifest.settings.clone(),
                manifest,
                script,
                enabled: true,
            }),
            ast,
            epoch: RUNTIME_EPOCH.load(Ordering::Relaxed),
        }
    }
}
