//! Owned editor window on its own UI thread. The game's event loop is untouched.
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::{run_return::EventLoopExtRunReturn, windows::EventLoopBuilderExtWindows},
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder};

enum Command {
    Visible(bool),
    Close,
}
pub struct EditorWindow {
    commands: mpsc::Sender<Command>,
    errors: mpsc::Receiver<String>,
}
impl EditorWindow {
    pub fn open(url: String, profile: PathBuf) -> Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let (errors_tx, errors) = mpsc::channel();
        std::thread::Builder::new().name("rpen-window".into()).spawn(move || {
            if let Err(error) = run(url, profile, receiver) {
                let _ = errors_tx.send(format!("无法打开 RPEN 编辑器，请检查 Microsoft Edge WebView2 Runtime：{error}"));
            }
        })?;
        Ok(Self { commands, errors })
    }
    pub fn visible(&self, value: bool) {
        let _ = self.commands.send(Command::Visible(value));
    }
    pub fn error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}
impl Drop for EditorWindow {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}
fn run(url: String, profile: PathBuf, receiver: mpsc::Receiver<Command>) -> Result<()> {
    let mut builder = EventLoopBuilder::<()>::new();
    builder.with_any_thread(true);
    let mut event_loop = builder.build();
    let builder = WindowBuilder::new()
        .with_title("Phira Pro 制谱工作台")
        .with_inner_size(LogicalSize::new(1280., 800.))
        .with_min_inner_size(LogicalSize::new(360., 320.));
    let window = builder.build(&event_loop)?;
    let mut context = WebContext::new(Some(profile));
    let allowed = url.split('/').take(3).collect::<Vec<_>>().join("/");
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&url)
        .with_navigation_handler(move |next| next.starts_with(&(allowed.clone() + "/")) || next == "about:blank")
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .build(&window)?;
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
        while let Ok(command) = receiver.try_recv() {
            match command {
                Command::Visible(value) => {
                    window.set_visible(value);
                    if value {
                        window.set_focus();
                    }
                }
                Command::Close => *flow = ControlFlow::Exit,
            }
        }
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            let _ = webview.evaluate_script("globalThis.phiraEditor?.requestClose()");
        }
    });
    Ok(())
}
