//! Scene management module.
#![allow(unused_macros)]

prpr_l10n::tl_file!("scene" ttl);

mod ending;
pub use ending::{EndingScene, RecordUpdateState};

mod game;
pub use game::{GameMode, GameScene, SimpleRecord, UploadScore};

mod loading;
pub use loading::{BasicPlayer, LoadingScene, SaveFn, UpdateFn, UploadFn};

use crate::{
    ext::{draw_image, screen_aspect, LocalTask, SafeTexture, ScaleType},
    judge::Judge,
    time::TimeManager,
    ui::{BillBoard, Dialog, Message, MessageHandle, MessageKind, TextPainter, Ui},
};
use anyhow::{Error, Result};
use cfg_if::cfg_if;
use inputbox::{
    backend::{default_backend, Backend},
    InputBox,
};
use macroquad::prelude::*;
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    sync::{Arc, Mutex},
};
use tracing::warn;

#[derive(Default)]
pub enum NextScene {
    #[default]
    None,
    Pop,
    PopN(usize),
    PopWithResult(Box<dyn Any>),
    PopNWithResult(usize, Box<dyn Any>),
    Exit,
    Overlay(Box<dyn Scene>),
    Replace(Box<dyn Scene>),
}

thread_local! {
    pub static BILLBOARD: RefCell<(BillBoard, TimeManager)> = RefCell::new((BillBoard::new(), TimeManager::default()));
    pub static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
    pub static FULL_LOADING: RefCell<Option<FullLoadingView>> = const { RefCell::new(None) };
}

pub struct FullLoadingView {
    keep_alive: Arc<()>,
    text: Option<Cow<'static, str>>,
}

impl FullLoadingView {
    pub fn begin() -> Arc<()> {
        Self::begin_inner(None)
    }
    pub fn begin_text(text: Cow<'static, str>) -> Arc<()> {
        Self::begin_inner(Some(text))
    }
    fn begin_inner(text: Option<Cow<'static, str>>) -> Arc<()> {
        let arc = Arc::new(());
        let ret = arc.clone();
        FULL_LOADING.replace(Some(Self { keep_alive: arc, text }));
        ret
    }
}

#[inline]
pub fn show_error(error: Error) {
    warn!("show error: {error:?}");
    Dialog::error(error).show();
}

pub struct MessageBuilder {
    content: String,
    kind: MessageKind,
    duration: f32,
}

impl MessageBuilder {
    pub fn new(content: String) -> Self {
        Self {
            content,
            kind: MessageKind::Info,
            duration: 2.,
        }
    }

    #[inline]
    pub fn kind(mut self, kind: MessageKind) -> Self {
        self.kind = kind;
        self
    }

    #[inline]
    pub fn duration(mut self, t: f32) -> Self {
        self.duration = t;
        self
    }

    #[inline]
    pub fn ok(self) -> Self {
        self.kind(MessageKind::Ok)
    }

    #[inline]
    pub fn warn(self) -> Self {
        self.kind(MessageKind::Warn)
    }

    #[inline]
    pub fn error(self) -> Self {
        self.kind(MessageKind::Error)
    }

    fn show(&mut self) -> MessageHandle {
        BILLBOARD.with(|it| {
            let mut guard = it.borrow_mut();
            let (msg, handle) = Message::new(std::mem::take(&mut self.content), guard.1.now() as _, self.duration, self.kind.clone());
            guard.0.add(msg);
            handle
        })
    }

    #[inline]
    pub fn handle(mut self) -> MessageHandle {
        let handle = self.show();
        std::mem::forget(self);
        handle
    }
}

impl Drop for MessageBuilder {
    fn drop(&mut self) {
        self.show();
    }
}

#[inline]
pub fn show_message(msg: impl Into<String>) -> MessageBuilder {
    MessageBuilder::new(msg.into())
}

pub static INPUT_TEXT: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));
/// Holds the id of the last input request the user cancelled (clicked Cancel or
/// dismissed the dialog). Consumed via [`take_input_cancelled`]; distinct from
/// [`INPUT_TEXT`] so callers can distinguish "cancelled" from "no input yet".
pub static INPUT_CANCELLED: Mutex<Option<String>> = Mutex::new(None);
#[cfg(not(target_arch = "wasm32"))]
pub static CHOSEN_FILE: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));

fn show_inputbox(config: InputBox, backend: &dyn Backend) {
    let result = config.show_with_async(backend, |result| match result {
        Ok(Some(text)) => {
            INPUT_TEXT.lock().unwrap().1 = Some(text);
        }
        Ok(None) => {
            // User cancelled; report it under the pending request's id so the
            // caller can react (e.g. return to the previous screen).
            let id = INPUT_TEXT.lock().unwrap().0.clone();
            *INPUT_CANCELLED.lock().unwrap() = id;
        }
        Err(err) => {
            warn!(?err, "failed to get input");
        }
    });
    if let Err(err) = result {
        warn!(?err, "failed to show input box");
    }
}

#[inline]
pub fn request_input(id: impl Into<String>, mut config: InputBox) {
    *INPUT_TEXT.lock().unwrap() = (Some(id.into()), None);
    *INPUT_CANCELLED.lock().unwrap() = None;
    if config.title.is_none() {
        config = config.title(ttl!("input"));
    }
    if config.prompt.is_none() {
        config = config.prompt(ttl!("input-msg"));
    }
    if config.cancel_label.is_none() {
        config = config.cancel_label(ttl!("cancel"));
    }
    if config.ok_label.is_none() {
        config = config.ok_label(ttl!("confirm"));
    }
    show_inputbox(config, &*default_backend());
}

pub fn take_input() -> Option<(String, String)> {
    let mut w = INPUT_TEXT.lock().unwrap();
    w.0.clone().zip(std::mem::take(&mut w.1))
}

/// Returns the id of a cancelled input request once, clearing it.
pub fn take_input_cancelled() -> Option<String> {
    INPUT_CANCELLED.lock().unwrap().take()
}

pub fn return_input(id: String, text: String) {
    *INPUT_TEXT.lock().unwrap() = (Some(id), Some(text));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn request_file(id: impl Into<String>) {
    let id: String = id.into();
    #[cfg(target_env = "ohos")]
    let is_photo = id == "avatar";
    // Phira Pro：图片类导入（图标 / 背景 / 立绘）在 Android / iOS 上走相册选择器。
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let is_photo = matches!(id.as_str(), "icon_import" | "background_import" | "appearance_import" | "hide_upper_import" | "hide_lower_import");
    *CHOSEN_FILE.lock().unwrap() = (Some(id), None);
    cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                // 图片类优先用相册（GET_CONTENT image/*）；老包里没有 choosePhoto 时回退文件选择器。
                let name = if is_photo { c"choosePhoto" } else { c"chooseFile" };
                let mut method = (**env).GetMethodID.unwrap()(env, class, name.as_ptr() as _, c"()V".as_ptr() as _);
                if method.is_null() {
                    if (**env).ExceptionCheck.unwrap()(env) != 0 { (**env).ExceptionClear.unwrap()(env); }
                    method = (**env).GetMethodID.unwrap()(env, class, c"chooseFile".as_ptr() as _, c"()V".as_ptr() as _);
                }
                if method.is_null() {
                    if (**env).ExceptionCheck.unwrap()(env) != 0 { (**env).ExceptionClear.unwrap()(env); }
                    show_error(anyhow::anyhow!("当前 Android 包缺少文件选择器，请更新完整安装包"));
                } else {
                    (**env).CallVoidMethod.unwrap()(env, ctx, method);
                    if (**env).ExceptionCheck.unwrap()(env) != 0 {
                        (**env).ExceptionClear.unwrap()(env);
                        show_error(anyhow::anyhow!("无法打开 Android 文件选择器"));
                    }
                }
                (**env).DeleteLocalRef.unwrap()(env, class);
            }
        } else if #[cfg(target_os = "ios")] {
            use objc2::runtime::{AnyObject, ProtocolObject};
            use objc2::{available, define_class, msg_send, rc::Retained, MainThreadMarker, MainThreadOnly};
            use objc2_foundation::{NSArray, NSDictionary, NSObject, NSObjectProtocol, NSString, NSTemporaryDirectory, NSURL};
            use objc2_ui_kit::{
                UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController, UIImage, UIImagePickerController,
                UIImagePickerControllerDelegate, UIImagePickerControllerInfoKey, UIImagePickerControllerOriginalImage,
                UIImagePickerControllerSourceType, UINavigationControllerDelegate, UIViewController,
            };

            // 本项目是**非 scene 老式 App**（Info.plist 没有 UIApplicationSceneManifest），而
            // inputbox 的 `get_top_view_controller` 走 scene 查找，容易拿不到 / 拿到别的窗口，
            // 结果选择器被挂到不可交互的窗口上——表现为「弹窗能显示、点不动」。这里优先取 App 自己的 key window。
            fn top_view_controller(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
                let root = {
                    let app = UIApplication::sharedApplication(mtm);
                    #[allow(deprecated)]
                    let window = app.keyWindow();
                    match window {
                        Some(window) => window.rootViewController(),
                        None => inputbox::backend::IOS::get_top_view_controller(mtm),
                    }
                };
                let mut top = root?;
                while let Some(presented) = top.presentedViewController() {
                    top = presented;
                }
                Some(top)
            }

            thread_local! {
                static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
                static PHOTO_DELEGATE: RefCell<Option<Retained<PhotoDelegate>>> = const { RefCell::new(None) };
            }

            define_class! {
                // SAFETY:
                // - The superclass NSObject does not have any subclassing requirements.
                // - `Delegate` does not implement `Drop`.
                #[unsafe(super = NSObject)]
                #[thread_kind = MainThreadOnly]
                struct PickerDelegate;

                // SAFETY: `NSObjectProtocol` has no safety requirements.
                unsafe impl NSObjectProtocol for PickerDelegate {}

                // SAFETY: `UIDocumentPickerDelegate` has no safety requirements.
                unsafe impl UIDocumentPickerDelegate for PickerDelegate {
                    // SAFETY: The signature is correct.
                    #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
                    fn did_pick_documents_at_urls(&self, controller: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
                        use objc2_foundation::{NSData, NSDataReadingOptions, NSTemporaryDirectory};

                        let Some(url) = urls.firstObject() else {
                            controller.dismissViewControllerAnimated_completion(true, None);
                            show_error(Error::msg("No file was selected").context(ttl!("read-file-failed")));
                            return;
                        };
                        let need_close = unsafe { url.startAccessingSecurityScopedResource() };

                        let imported: Result<String> = (|| {
                            let data = NSData::dataWithContentsOfURL_options_error(&url, NSDataReadingOptions::Uncached)
                                .map_err(|err| Error::msg(err.localizedDescription().to_string()).context(ttl!("read-file-failed")))?;
                            let dir = NSTemporaryDirectory();
                            let path = format!("{}{}", dir, uuid::Uuid::new_v4());
                            if !data.writeToFile_atomically(&NSString::from_str(&path), true) {
                                return Err(Error::msg("Unable to copy the selected file to app storage").context(ttl!("read-file-failed")));
                            }
                            Ok(path)
                        })();
                        if need_close {
                            unsafe { url.stopAccessingSecurityScopedResource() };
                        }
                        controller.dismissViewControllerAnimated_completion(true, None);
                        match imported {
                            Ok(path) => CHOSEN_FILE.lock().unwrap().1 = Some(path),
                            Err(err) => show_error(err),
                        }
                    }
                }
            }

            impl PickerDelegate {
                fn new(mtm: MainThreadMarker) -> Retained<Self> {
                    let this = Self::alloc(mtm).set_ivars(());
                    unsafe { msg_send![super(this), init] }
                }
            }

            // ---- 相册选择器委托（自定义图标 / 背景 / 立绘）----
            define_class! {
                // SAFETY:
                // - The superclass NSObject does not have any subclassing requirements.
                // - `PhotoDelegate` does not implement `Drop`.
                #[unsafe(super = NSObject)]
                #[thread_kind = MainThreadOnly]
                struct PhotoDelegate;

                // SAFETY: `NSObjectProtocol` has no safety requirements.
                unsafe impl NSObjectProtocol for PhotoDelegate {}

                // SAFETY: `UINavigationControllerDelegate` has no safety requirements.
                unsafe impl UINavigationControllerDelegate for PhotoDelegate {}

                // SAFETY: `UIImagePickerControllerDelegate` has no safety requirements.
                unsafe impl UIImagePickerControllerDelegate for PhotoDelegate {
                    // SAFETY: The signature is correct.
                    #[unsafe(method(imagePickerController:didFinishPickingMediaWithInfo:))]
                    unsafe fn did_finish_picking_media(
                        &self,
                        picker: &UIImagePickerController,
                        info: &NSDictionary<UIImagePickerControllerInfoKey, AnyObject>,
                    ) {
                        if let Some(image) = info.objectForKey(UIImagePickerControllerOriginalImage) {
                            if let Some(image) = image.downcast_ref::<UIImage>() {
                                if let Some(data) = image.png_representation() {
                                    let dir = NSTemporaryDirectory();
                                    let path = format!("{}{}", dir, uuid::Uuid::new_v4());
                                    data.writeToFile_atomically(&NSString::from_str(&path), true);
                                    CHOSEN_FILE.lock().unwrap().1 = Some(path);
                                }
                            }
                        }
                        picker.dismissViewControllerAnimated_completion(true, None);
                    }

                    // SAFETY: The signature is correct.
                    #[unsafe(method(imagePickerControllerDidCancel:))]
                    fn did_cancel_picking(&self, picker: &UIImagePickerController) {
                        picker.dismissViewControllerAnimated_completion(true, None);
                    }
                }
            }

            impl PhotoDelegate {
                fn new(mtm: MainThreadMarker) -> Retained<Self> {
                    let this = Self::alloc(mtm).set_ivars(());
                    unsafe { msg_send![super(this), init] }
                }
            }

            let mtm = MainThreadMarker::new().unwrap();
            let Some(presenting) = top_view_controller(mtm) else { return };

            if is_photo {
                let picker: Retained<UIImagePickerController> = unsafe { msg_send![UIImagePickerController::alloc(mtm), init] };
                #[allow(deprecated)]
                picker.setSourceType(UIImagePickerControllerSourceType::PhotoLibrary);
                picker.setAllowsEditing(false);
                let dlg = PhotoDelegate::new(mtm);
                let delegate: &AnyObject = unsafe { &*(&*dlg as *const PhotoDelegate as *const AnyObject) };
                unsafe { picker.setDelegate(Some(delegate)) };
                PHOTO_DELEGATE.with(|it| *it.borrow_mut() = Some(dlg));
                presenting.presentViewController_animated_completion(&picker, true, None);
            } else {
                let picker = UIDocumentPickerViewController::alloc(mtm);
                let picker = if available!(ios = 14.0.0) {
                    use objc2_uniform_type_identifiers::UTType;

                    let ext = |e: &str| UTType::typeWithFilenameExtension(&NSString::from_str(e)).unwrap();
                    let types = NSArray::from_retained_slice(&[
                        ext("zip"),
                        ext("pez"),
                        ext("jpg"),
                        ext("png"),
                        ext("jpeg"),
                        ext("json"),
                        ext("mp3"),
                        ext("ogg"),
                    ]);
                    // Copy provider-backed files into the app before the picker dismisses. This
                    // avoids a stale security-scoped URL when import processing starts next frame.
                    UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(picker, &types, true)
                } else {
                    #[allow(deprecated)]
                    {
                        use objc2_ui_kit::UIDocumentPickerMode;

                        let ext = NSString::from_str;
                        let types = NSArray::from_retained_slice(&[ext("public.image"), ext("public.archive")]);
                        UIDocumentPickerViewController::initWithDocumentTypes_inMode(picker, &types, UIDocumentPickerMode::Import)
                    }
                };
                let dlg = PickerDelegate::new(mtm);
                picker.setDelegate(Some(ProtocolObject::from_ref(&*dlg)));
                DELEGATE.with(|it| *it.borrow_mut() = Some(dlg));
                presenting.presentViewController_animated_completion(&picker, true, None);
            }
        } else if #[cfg(target_env = "ohos")] {
            miniquad::native::call_request_callback(format!(r#"{{"action": "chooseFile", "isPhoto": {}}}"#, is_photo));
        } else { // desktop
            CHOSEN_FILE.lock().unwrap().1 = rfd::FileDialog::new().pick_file().map(|it| it.display().to_string());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_file() -> Option<(String, String)> {
    let mut w = CHOSEN_FILE.lock().unwrap();
    w.0.clone().zip(std::mem::take(&mut w.1))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn return_file(id: String, file: String) {
    *CHOSEN_FILE.lock().unwrap() = (Some(id), Some(file));
}

/// 复制文本到系统剪贴板。
pub fn copy_to_clipboard(text: &str) {
    unsafe { get_internal_gl() }.quad_context.clipboard_set(text);
}

pub trait Scene {
    fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        Ok(())
    }
    fn pause(&mut self, _tm: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn resume(&mut self, _tm: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn on_result(&mut self, _tm: &mut TimeManager, _result: Box<dyn Any>) -> Result<()> {
        Ok(())
    }
    /// Called on the outgoing scene if its replacement failed to enter.
    /// A loading scene must retain a route back to the chart details.
    fn on_enter_error(&mut self, _tm: &mut TimeManager, error: Error) -> Result<()> {
        Err(error)
    }
    fn touch(&mut self, _tm: &mut TimeManager, _touch: &Touch) -> Result<bool> {
        Ok(false)
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()>;
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()>;
    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        NextScene::None
    }
}

pub trait RenderTargetChooser {
    fn choose(&mut self) -> Option<RenderTarget>;
}
impl RenderTargetChooser for Option<RenderTarget> {
    fn choose(&mut self) -> Option<RenderTarget> {
        *self
    }
}
impl<F: FnMut() -> Option<RenderTarget>> RenderTargetChooser for F {
    fn choose(&mut self) -> Option<RenderTarget> {
        self()
    }
}

pub struct Main {
    pub scenes: Vec<Box<dyn Scene>>,
    times: Vec<f64>,
    target_chooser: Box<dyn RenderTargetChooser>,
    tm: TimeManager,
    paused: bool,
    last_update_time: f64,
    should_exit: bool,
    pub top_level: bool,
    touches: Option<Vec<Touch>>,
    pub viewport: Option<(i32, i32, i32, i32)>,
}

impl Main {
    pub async fn new(mut scene: Box<dyn Scene>, mut tm: TimeManager, mut target_chooser: impl RenderTargetChooser + 'static) -> Result<Self> {
        simulate_mouse_with_touch(false);
        scene.enter(&mut tm, target_chooser.choose())?;
        let last_update_time = tm.now();
        macro_rules! load_tex {
            ($path:literal) => {
                SafeTexture::from(Texture2D::from_image(&load_image($path).await?))
            };
        }
        let icons = [load_tex!("info.png"), load_tex!("warn.png"), load_tex!("ok.png"), load_tex!("error.png")];
        BILLBOARD.with(|it| it.borrow_mut().0.set_icons(icons));
        Ok(Self {
            scenes: vec![scene],
            times: Vec::new(),
            target_chooser: Box::new(target_chooser),
            tm,
            paused: false,
            last_update_time,
            should_exit: false,
            top_level: true,
            touches: None,
            viewport: None,
        })
    }

    pub fn update(&mut self) -> Result<()> {
        self.update_with_mutate(|_| {})
    }

    fn transition(&mut self) -> Result<()> {
        match self.scenes.last_mut().unwrap().next_scene(&mut self.tm) {
            NextScene::None => {}
            NextScene::Pop => {
                self.scenes.pop();
                self.tm.seek_to(self.times.pop().unwrap());
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopN(num) => {
                for _ in 0..num {
                    self.scenes.pop();
                    self.tm.seek_to(self.times.pop().unwrap());
                }
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopWithResult(result) => {
                self.scenes.pop();
                self.tm.seek_to(self.times.pop().unwrap());
                self.scenes.last_mut().unwrap().on_result(&mut self.tm, result)?;
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopNWithResult(num, result) => {
                for _ in 0..num {
                    self.scenes.pop();
                    self.tm.seek_to(self.times.pop().unwrap());
                }
                self.scenes.last_mut().unwrap().on_result(&mut self.tm, result)?;
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::Exit => {
                self.should_exit = true;
            }
            NextScene::Overlay(mut scene) => {
                let previous_time = self.tm.now();
                scene.enter(&mut self.tm, self.target_chooser.choose())?;
                self.times.push(previous_time);
                self.scenes.push(scene);
            }
            NextScene::Replace(mut scene) => {
                if let Err(error) = scene.enter(&mut self.tm, self.target_chooser.choose()) {
                    return self.scenes.last_mut().unwrap().on_enter_error(&mut self.tm, error);
                }
                *self.scenes.last_mut().unwrap() = scene;
            }
        }
        Ok(())
    }

    pub fn update_with_mutate(&mut self, f: impl Fn(&mut Touch)) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        self.transition()?;
        Judge::on_new_frame();
        let mut touches = Judge::get_touches();
        touches.iter_mut().for_each(f);
        if !(touches.is_empty() || FULL_LOADING.with(|it| it.borrow().is_some())) {
            let now = self.tm.now();
            let delta = (now - self.last_update_time) / touches.len() as f64;
            let start_time = self.tm.start_time;
            let mut last_err = None;
            DIALOG.with(|it| -> Result<()> {
                let mut index = 1;
                touches.retain_mut(|touch| {
                    let t = self.last_update_time + (index + 1) as f64 * delta;
                    index += 1;
                    let mut guard = it.borrow_mut();
                    if let Some(dialog) = guard.as_mut() {
                        if !dialog.touch(touch, t as _) {
                            drop(guard);
                            *it.borrow_mut() = None;
                        }
                        false
                    } else {
                        drop(guard);
                        self.tm.seek_to(t);
                        match self.scenes.last_mut().unwrap().touch(&mut self.tm, touch) {
                            Ok(val) => !val,
                            Err(err) => {
                                warn!(?err, "failed to handle touch");
                                last_err = Some(err);
                                false
                            }
                        }
                    }
                });
                Ok(())
            })?;
            if let Some(err) = last_err {
                return Err(err);
            }
            self.tm.start_time = start_time;
        }
        self.touches = Some(touches);
        self.last_update_time = self.tm.now();
        DIALOG.with(|it| {
            if let Some(dialog) = it.borrow_mut().as_mut() {
                dialog.update(self.last_update_time as _);
            }
        });
        self.scenes.last_mut().unwrap().update(&mut self.tm)?;
        Ok(())
    }

    pub fn render(&mut self, painter: &mut TextPainter) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        let mut ui = Ui::new(painter, self.viewport);
        ui.set_touches(self.touches.take());
        ui.scope(|ui| self.scenes.last_mut().unwrap().render(&mut self.tm, ui))?;
        if self.top_level {
            push_camera_state();
            set_camera(&ui.camera());
            let mut gl = unsafe { get_internal_gl() };
            gl.flush();
            // gl.quad_gl.render_pass(None);
            // gl.quad_gl.viewport(None);
            BILLBOARD.with(|it| {
                let mut guard = it.borrow_mut();
                let t = guard.1.now() as f32;
                guard.0.render(&mut ui, t);
            });
            DIALOG.with(|it| {
                if let Some(dialog) = it.borrow_mut().as_mut() {
                    dialog.render(&mut ui, self.tm.now() as _);
                }
            });
            let remove = FULL_LOADING.with(|it| {
                if let Some(loading) = it.borrow_mut().as_mut() {
                    if Arc::strong_count(&loading.keep_alive) > 1 {
                        if let Some(text) = loading.text.as_ref() {
                            ui.full_loading(text.clone(), self.tm.now() as _);
                        } else {
                            ui.full_loading_simple(self.tm.now() as _);
                        }
                        return false;
                    } else {
                        return true;
                    }
                }
                false
            });
            if remove {
                FULL_LOADING.take();
            }
            // 左下角帧率：只有一个数字，无文字、无背景。
            if crate::ui::SHOW_FPS.load(std::sync::atomic::Ordering::Relaxed) {
                let fps = f32::from_bits(crate::ui::CURRENT_FPS.load(std::sync::atomic::Ordering::Relaxed));
                if fps > 0. {
                    // 由视口直接推算屏幕范围：覆盖层阶段 ui.top 与 screen_rect 都不可靠。
                    // 贴到左下角最边缘并减小字号，避开游玩界面左下角的曲名。
                    // 用 anchor(0,1) 把整行字贴在 pos 上方；否则默认从 pos 往下排版，
                    // 会被顶出屏幕底边而看不见。
                    let vp = crate::ext::get_viewport();
                    let half_h = vp.3 as f32 / vp.2 as f32;
                    ui.text(format!("{fps:.0}")).pos(-0.99, half_h - 0.006).anchor(0., 1.).size(0.25).draw();
                }
            }
            pop_camera_state();
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        self.paused = true;
        self.scenes.last_mut().unwrap().pause(&mut self.tm)
    }

    pub fn resume(&mut self) -> Result<()> {
        self.paused = false;
        self.scenes.last_mut().unwrap().resume(&mut self.tm)
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }
}

fn draw_background(tex: Texture2D) {
    let asp = screen_aspect();
    let top = 1. / asp;
    draw_image(tex, Rect::new(-1., -top, 2., top * 2.), ScaleType::CropCenter);
    draw_rectangle(-1., -top, 2., top * 2., Color::new(0., 0., 0., 0.3));
}

pub type LocalSceneTask = LocalTask<Result<NextScene>>;

#[cfg(test)]
mod transition_tests {
    use super::*;
    use std::rc::Rc;

    struct TestScene {
        next: NextScene,
        fail_enter: bool,
        recover_loading: bool,
        reported: Rc<RefCell<Option<String>>>,
    }

    impl Scene for TestScene {
        fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
            anyhow::ensure!(!self.fail_enter, "audio initialization failed");
            Ok(())
        }
        fn on_enter_error(&mut self, _tm: &mut TimeManager, error: Error) -> Result<()> {
            if self.recover_loading {
                self.next = NextScene::PopWithResult(Box::new(error));
                Ok(())
            } else {
                Err(error)
            }
        }
        fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn Any>) -> Result<()> {
            *self.reported.borrow_mut() = Some(result.downcast::<Error>().unwrap().to_string());
            Ok(())
        }
        fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
            std::mem::take(&mut self.next)
        }
        fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
            Ok(())
        }
        fn render(&mut self, _tm: &mut TimeManager, _ui: &mut Ui) -> Result<()> {
            Ok(())
        }
    }

    fn scene(reported: &Rc<RefCell<Option<String>>>) -> TestScene {
        TestScene {
            next: NextScene::None,
            fail_enter: false,
            recover_loading: false,
            reported: reported.clone(),
        }
    }

    fn main(scenes: Vec<Box<dyn Scene>>, times: Vec<f64>) -> Main {
        Main {
            scenes,
            times,
            target_chooser: Box::new(None::<RenderTarget>),
            tm: TimeManager::default(),
            paused: false,
            last_update_time: 0.,
            should_exit: false,
            top_level: false,
            touches: None,
            viewport: None,
        }
    }

    #[test]
    fn failed_game_entry_can_return_loading_error_to_details() {
        let reported = Rc::new(RefCell::new(None));
        let mut game = scene(&reported);
        game.fail_enter = true;
        let mut loading = scene(&reported);
        loading.recover_loading = true;
        loading.next = NextScene::Replace(Box::new(game));
        let mut app = main(vec![Box::new(scene(&reported)), Box::new(loading)], vec![2.]);
        app.transition().unwrap();
        assert_eq!(app.scenes.len(), 2);
        app.transition().unwrap();
        assert_eq!(app.scenes.len(), 1);
        assert!(app.times.is_empty());
        assert_eq!(reported.borrow().as_deref(), Some("audio initialization failed"));
    }

    #[test]
    fn failed_overlay_does_not_leave_a_phantom_time_frame() {
        let reported = Rc::new(RefCell::new(None));
        let mut overlay = scene(&reported);
        overlay.fail_enter = true;
        let mut parent = scene(&reported);
        parent.next = NextScene::Overlay(Box::new(overlay));
        let mut app = main(vec![Box::new(parent)], vec![]);
        assert!(app.transition().is_err());
        assert_eq!(app.scenes.len(), 1);
        assert!(app.times.is_empty());
    }
}
