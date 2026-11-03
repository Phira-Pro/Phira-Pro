//! RPEN integration point. Editor persistence and UI live outside game modules.
mod platform;
mod trial;
mod library;

use crate::page::{NextPage, Page, SharedState};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::poll_future,
    scene::{NextScene, Scene},
    ui::{DRectButton, Ui},
};
use rpen_host::{EditorEvent, EditorHost};
use std::{borrow::Cow, future::Future, path::PathBuf, pin::Pin};

type Opening = Pin<Box<dyn Future<Output = Result<(EditorHost, PathBuf)>>>>;
type Loading = Pin<Box<dyn Future<Output = Result<Box<dyn Scene>>>>>;
pub(crate) struct EditorPage {
    opening: Option<Opening>,
    prepared: Option<(EditorHost, PathBuf)>,
    host: Option<EditorHost>,
    window: Option<platform::Window>,
    loading: Option<Loading>,
    next_scene: Option<NextScene>,
    next_page: NextPage,
    status: String,
    initialized: bool,
    trial_active: bool,
    reopen: DRectButton,
    library: library::LibraryAdapter,
}
impl EditorPage {
    pub fn new() -> Self {
        Self {
            opening: None,
            prepared: None,
            host: None,
            window: None,
            loading: None,
            next_scene: None,
            next_page: NextPage::None,
            status: "正在准备离线制谱器".into(),
            initialized: false,
            trial_active: false,
            reopen: DRectButton::new(),
            library: library::LibraryAdapter::default(),
        }
    }
}
async fn prepare() -> Result<(EditorHost, PathBuf)> {
    let root = PathBuf::from(crate::dir::root()?).join("editor");
    let assets = root.join(format!("runtime-{}", env!("PHIRA_BUILD_NUMBER")));
    let manifest = load_file("rpen-editor/files.json").await?;
    let index: serde_json::Value = serde_json::from_slice(&manifest)?;
    let names: Vec<String> = serde_json::from_value(index["files"].clone())?;
    // Comparing the manifest also detects rebuilt resources with the same build
    // number, so local iteration cannot silently keep an older frontend.
    if std::fs::read(assets.join(".complete")).ok().as_deref() != Some(manifest.as_slice()) {
        for name in names {
            anyhow::ensure!(
                !name.contains(['\\', ':'])
                    && std::path::Path::new(&name)
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_))),
                "无效的编辑器资源路径"
            );
            let bytes = load_file(&format!("rpen-editor/{name}")).await?;
            let path = assets.join(&name);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, bytes)?;
        }
        std::fs::write(assets.join(".complete"), &manifest)?;
    }
    let host = EditorHost::start(assets, root.clone())?;
    Ok((host, root.join("browser")))
}
impl Page for EditorPage {
    fn label(&self) -> Cow<'static, str> {
        "制谱工作台".into()
    }
    fn can_play_bgm(&self) -> bool {
        false
    }
    fn enter(&mut self, _: &mut SharedState) -> Result<()> {
        if !self.initialized {
            self.opening = Some(Box::pin(prepare()));
            self.initialized = true;
        } else if self.trial_active {
            self.resume()?;
        }
        Ok(())
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        if self.window.as_ref().is_some_and(platform::Window::close_requested) {
            self.window = None;
            self.host = None;
            self.next_page = NextPage::Pop;
            return Ok(());
        }
        if let Some(task) = &mut self.opening {
            if let Some(result) = poll_future(task.as_mut()) {
                self.opening = None;
                match result {
                    Ok((host, profile)) => {
                        self.prepared = Some((host, profile));
                    }
                    Err(error) => self.status = format!("无法准备编辑器：{error}"),
                }
            }
        }
        // Let the native page finish its transition before covering it with a WebView.
        if !s.fader.transiting() {
            if let Some((host, profile)) = self.prepared.take() {
                match platform::Window::open(host.url.clone(), profile) {
                    Ok(window) => {
                        self.window = Some(window);
                        self.status = "RPEN 编辑器已打开".into();
                    }
                    Err(error) => self.status = error.to_string(),
                };
                self.host = Some(host);
            }
        }
        if let Some(error) = self.window.as_ref().and_then(platform::Window::error) {
            self.status = error;
        }
        if let Some(task) = &mut self.loading {
            if let Some(result) = poll_future(task.as_mut()) {
                self.loading = None;
                match result {
                    Ok(scene) => {
                        self.trial_active = true;
                        self.next_scene = Some(NextScene::Overlay(scene));
                    }
                    Err(error) => {
                        self.status = format!("试玩加载失败：{error}");
                        if let Some(host) = &self.host {
                            host.resume(Some(self.status.clone()));
                        }
                        if let Some(window) = &self.window {
                            window.visible(true);
                        }
                    }
                }
            }
        }
        if let Some(event) = self.host.as_ref().and_then(EditorHost::poll) {
            match event {
                EditorEvent::Play { request, bytes } => {
                    if self.loading.is_none() && !self.trial_active {
                        self.status = "正在加载试玩快照".into();
                        self.host.as_ref().unwrap().playing();
                        if let Some(window) = &self.window {
                            window.visible(false);
                        }
                        self.loading = Some(Box::pin(trial::load(bytes, request)));
                    }
                }
                EditorEvent::Close => {
                    self.window = None;
                    self.host = None;
                    self.next_page = NextPage::Pop;
                }
                EditorEvent::Export { name, bytes } => crate::replay::request_export_bytes(name, bytes),
                EditorEvent::Library { id, request, bytes } => {
                    self.library.request(self.host.as_ref().unwrap(), id, request, bytes);
                }
            }
        }
        crate::replay::poll_export();
        if let Some(host) = &self.host { self.library.update(host, s); }
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        if self.trial_active {
            self.trial_active = false;
            self.status = "已返回制谱工作台".into();
            if let Some(host) = &self.host {
                host.resume(None);
            }
            if let Some(window) = &self.window {
                window.visible(true);
            }
        }
        Ok(())
    }
    fn exit(&mut self) -> Result<()> {
        self.window = None;
        self.host = None;
        Ok(())
    }
    fn on_back_pressed(&mut self, _: &mut SharedState) -> bool {
        if let Some(host) = &self.host {
            host.request_close();
            if let Some(window) = &self.window {
                window.visible(true);
            }
            true
        } else {
            false
        }
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.reopen.touch(touch, s.t) {
            if let Some(window) = &self.window {
                window.visible(true);
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| {
            let r = ui.content_rect();
            ui.text("RPEN 制谱工作台").pos(r.x + 0.04, r.y + 0.04).size(0.7).draw();
            ui.text(&self.status).pos(r.x + 0.04, r.y + 0.17).size(0.42).max_width(r.w - 0.08).draw();
            ui.text("工程、素材和扩展保存在独立编辑器目录中。试玩结束后返回原编辑位置。")
                .pos(r.x + 0.04, r.y + 0.27)
                .size(0.36)
                .max_width(r.w - 0.08)
                .draw();
            if self.window.is_some() && !self.trial_active {
                self.reopen
                    .render_text(ui, Rect::new(r.x + 0.04, r.y + 0.4, 0.5, 0.1), t, "回到编辑器", 0.42, false);
            }
        });
        Ok(())
    }
    fn next_page(&mut self) -> NextPage {
        std::mem::take(&mut self.next_page)
    }
    fn next_scene(&mut self, _: &mut SharedState) -> NextScene {
        self.next_scene.take().unwrap_or_default()
    }
}
