use anyhow::Result;
use std::path::PathBuf;

impl Window {
    pub fn close_requested(&self) -> bool {
        #[cfg(target_os = "android")]
        {
            use jni::{jni_sig, jni_str, objects::JObject, vm::JavaVM};
            return JavaVM::singleton().and_then(|vm| vm.attach_current_thread(|env| {
                let activity = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
                env.call_method(activity, jni_str!("takeRpenEditorBootExit"), jni_sig!("()Z"), &[])?.z()
            })).unwrap_or(false);
        }
        #[cfg(not(target_os = "android"))]
        false
    }
}

#[cfg(target_os = "windows")]
pub struct Window(rpen_host::windows::EditorWindow);
#[cfg(target_os = "windows")]
impl Window {
    pub fn open(url: String, profile: PathBuf) -> Result<Self> {
        Ok(Self(rpen_host::windows::EditorWindow::open(url, profile)?))
    }
    pub fn visible(&self, value: bool) {
        self.0.visible(value);
    }
    pub fn error(&self) -> Option<String> {
        self.0.error()
    }
}

#[cfg(target_os = "android")]
pub struct Window;
#[cfg(target_os = "android")]
fn android_call(url: Option<String>, visible: Option<bool>) -> Result<()> {
    use anyhow::Context;
    use jni::{jni_sig, jni_str, objects::JObject, vm::JavaVM};
    JavaVM::singleton()
        .context("Android Java VM is unavailable")?
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
            if let Some(url) = url {
                let url = env.new_string(url)?;
                let software = crate::get_data().config.rpen_software_rendering;
                env.call_method(activity, jni_str!("showRpenEditor"), jni_sig!("(Ljava/lang/String;Z)V"), &[(&url).into(), software.into()])?;
            } else if let Some(visible) = visible {
                env.call_method(activity, jni_str!("setRpenEditorVisible"), jni_sig!("(Z)V"), &[visible.into()])?;
            } else {
                env.call_method(activity, jni_str!("closeRpenEditor"), jni_sig!("()V"), &[])?;
            }
            Ok(())
        })
        .map_err(anyhow::Error::from)
}
#[cfg(target_os = "android")]
impl Window {
    pub fn open(url: String, _: PathBuf) -> Result<Self> {
        android_call(Some(url), None)?;
        Ok(Self)
    }
    pub fn visible(&self, value: bool) {
        let _ = android_call(None, Some(value));
    }
    pub fn error(&self) -> Option<String> {
        None
    }
}
#[cfg(target_os = "android")]
impl Drop for Window {
    fn drop(&mut self) {
        let _ = android_call(None, None);
    }
}

#[cfg(target_os = "ios")]
pub struct Window;
#[cfg(target_os = "ios")]
impl Window {
    pub fn open(url: String, _: PathBuf) -> Result<Self> {
        rpen_host::ios::open(&url)?;
        Ok(Self)
    }
    pub fn visible(&self, value: bool) {
        rpen_host::ios::visible(value);
    }
    pub fn error(&self) -> Option<String> {
        None
    }
}
#[cfg(target_os = "ios")]
impl Drop for Window {
    fn drop(&mut self) {
        rpen_host::ios::close();
    }
}

#[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
pub struct Window;
#[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
impl Window {
    pub fn open(_: String, _: PathBuf) -> Result<Self> {
        anyhow::bail!("当前平台尚未接入编辑器网页容器")
    }
    pub fn visible(&self, _: bool) {}
    pub fn error(&self) -> Option<String> {
        None
    }
}
