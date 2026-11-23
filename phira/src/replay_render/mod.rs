//! 回放渲染：把 `.ppr` 回放按录制判定与触点导出为视频，并可在应用内预览。
//!
//! 本模块的 `replay` / `scene` / `video` 三个子模块移植自 PPR Render 1.0.0
//! （GPL-3.0-only），只把对独立播放器 `crate::` 的引用改成应用内模块，渲染内核仍是 PPR Render
//! 的同一套：`prpr::scene::GameScene` 负责谱面，`ReplayScene` 负责回放时钟，
//! `video` 负责离线编码与音轨。移植后由 Phira Pro 的「回放渲染」页面驱动，
//! 不再单独分发可执行文件。

pub mod replay;
pub mod scene;
pub mod video;

use crate::{dir, get_data};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    scene::{NextScene, Scene},
    time::TimeManager,
    ui::{TextPainter, Ui},
};
use replay::{ChartRef, Replay};
use scene::ReplayScene;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

// 视频导出用的独立文本绘制器。
//
// 应用主界面的绘制器带用户自定义字体与字号缩放，导出画面需要固定字号的
// Phira Pro 排版；两者共用同一份字体数据，但各自持有 `TextPainter`。
thread_local! {
    static EXPORT_PAINTER: RefCell<Option<TextPainter>> = RefCell::default();
}

pub fn install_painter(painter: TextPainter) {
    EXPORT_PAINTER.with(|it| *it.borrow_mut() = Some(painter));
}

/// 取出导出绘制器。返回值在离开作用域时自动放回，导出被取消、页面退出或
/// 异步任务被丢弃都不会把绘制器弄丢。
pub fn take_painter() -> Option<PainterGuard> {
    EXPORT_PAINTER.with(|it| it.borrow_mut().take()).map(PainterGuard::new)
}

pub struct PainterGuard(Option<TextPainter>);

impl PainterGuard {
    fn new(painter: TextPainter) -> Self {
        Self(Some(painter))
    }

    pub fn get(&mut self) -> &mut TextPainter {
        self.0.as_mut().expect("export painter already released")
    }
}

impl Drop for PainterGuard {
    fn drop(&mut self) {
        if let Some(painter) = self.0.take() {
            install_painter(painter);
        }
    }
}

/// 把 `ReplayScene` 挂到应用场景栈上。回放场景原本是独立程序的根场景，
/// 退出时返回 `NextScene::Exit`；在应用里要改成弹回上一页。
pub struct ReplayOverlay {
    inner: ReplayScene,
}

impl ReplayOverlay {
    pub fn new(inner: ReplayScene) -> Self {
        Self { inner }
    }
}

impl Scene for ReplayOverlay {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.inner.enter(tm, target)
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.inner.pause(tm)
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.inner.resume(tm)
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        self.inner.touch(tm, touch)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.inner.update(tm)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        self.inner.render(tm, ui)
    }

    fn suppresses_overlays(&self) -> bool {
        true
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        match self.inner.next_scene(tm) {
            NextScene::Exit => NextScene::Pop,
            other => other,
        }
    }
}

/// 谱面文件 / 目录判断，与 PPR Render 的规则一致。
pub fn is_chart(path: &Path) -> bool {
    (path.is_file()
        && path
            .extension()
            .and_then(|it| it.to_str())
            .is_some_and(|it| matches!(it.to_ascii_lowercase().as_str(), "zip" | "phira" | "pez")))
        || (path.is_dir()
            && (path.join("info.yml").is_file()
                || path.join("info.txt").is_file()
                || path.join("info.csv").is_file()))
}

/// 在回放所在目录查找与 `.ppr` 同名的谱面压缩包或文件夹。
pub fn same_name_chart(dir: &Path, stem: &str) -> Option<PathBuf> {
    fn archive_stem(name: &str) -> &str {
        let lower = name.to_ascii_lowercase();
        for ext in [".zip", ".pez", ".phira"] {
            if lower.len() > ext.len() && lower.ends_with(ext) {
                return &name[..name.len() - ext.len()];
            }
        }
        name
    }

    let mut archives = Vec::new();
    let mut folder = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !is_chart(&path) {
            continue;
        }
        if path.is_file() {
            if let Some(name) = path.file_stem().and_then(|it| it.to_str()) {
                if archive_stem(name).eq_ignore_ascii_case(stem) {
                    archives.push(path);
                }
            }
        } else if let Some(name) = path.file_name().and_then(|it| it.to_str()) {
            if name.eq_ignore_ascii_case(stem) {
                folder = Some(path);
            }
        }
    }
    archives.sort();
    archives.into_iter().next().or(folder)
}

fn chart_roots(replay_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = replay_dir {
        roots.push(dir.to_path_buf());
    }
    if let Ok(root) = dir::charts() {
        roots.push(PathBuf::from(root));
    }
    if let Ok(root) = dir::downloaded_charts() {
        roots.push(PathBuf::from(root));
    }
    if let Ok(root) = dir::custom_charts() {
        roots.push(PathBuf::from(root));
    }
    let mut seen = std::collections::HashSet::new();
    roots.retain(|path| seen.insert(path.clone()));
    roots
}

/// 按回放记录的谱面引用、同名文件和应用谱面库依次查找谱面。
pub fn detect_chart(replay: &Replay, replay_path: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let replay_dir = replay_path.parent();

    if let Some(dir) = replay_dir {
        if is_chart(dir) {
            candidates.push(dir.to_path_buf());
        }
        if let Some(stem) = replay_path.file_stem().and_then(|it| it.to_str()) {
            if let Some(path) = same_name_chart(dir, stem) {
                candidates.push(path);
            }
        }
    }

    if let Some(ChartRef::Local(path)) = &replay.chart {
        let path = PathBuf::from(path);
        let resolved = if path.is_absolute() {
            path.clone()
        } else if let Some(dir) = replay_dir {
            dir.join(&path)
        } else {
            path.clone()
        };
        candidates.push(resolved);
        // 应用谱面库里的 `local_path` 相对于 `<数据目录>/data/charts`，
        // 而不是回放所在的 `data/replays`。
        if let Ok(root) = dir::charts() {
            candidates.push(PathBuf::from(root).join(&path));
        }
        let local = path.to_string_lossy();
        if let Some(chart) = get_data().charts.iter().find(|it| it.local_path.as_str() == local.as_ref()) {
            if let Ok(root) = dir::charts() {
                candidates.push(PathBuf::from(root).join(&chart.local_path));
            }
        }
    }

    if let Some(id) = replay.chart_id() {
        for root in chart_roots(replay_dir) {
            for sub in ["", "charts/download", "data/charts/download"] {
                candidates.push(root.join(sub).join(id.to_string()));
            }
        }
        if let Some(chart) = get_data().charts.iter().find(|it| it.info.id == Some(id)) {
            if let Ok(root) = dir::charts() {
                candidates.push(PathBuf::from(root).join(&chart.local_path));
            }
        }
    }

    candidates.into_iter().find(|path| is_chart(path))
}

/// 从应用谱面库的相对路径解析出实际谱面目录。
pub fn chart_from_library(local_path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(dir::charts()?).join(local_path);
    anyhow::ensure!(is_chart(&path), "谱面文件不存在：{}", path.display());
    Ok(path)
}

/// 把应用谱面库里的选谱结果转成回放渲染可读取的路径。
pub fn normalize_chart_path(path: PathBuf) -> Result<PathBuf> {
    anyhow::ensure!(is_chart(&path), "不是有效谱面：{}", path.display());
    Ok(path)
}

/// 导出目录：`<data>/render`。与谱面渲染页共用同一个目录。
pub fn output_dir() -> Result<PathBuf> {
    let dir = PathBuf::from(dir::root()?).join("render");
    std::fs::create_dir_all(&dir).with_context(|| format!("无法创建输出目录 {}", dir.display()))?;
    Ok(dir)
}

/// 生成不会覆盖文件安全字符的输出文件名。
pub fn output_name(replay: &Replay) -> String {
    let name: String = replay
        .meta
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .take(90)
        .collect();
    let name = name.trim().trim_matches('.');
    let name = if name.is_empty() { "replay" } else { name };
    format!("{name} [{:.1}] replay.mp4", replay.meta.difficulty)
}
