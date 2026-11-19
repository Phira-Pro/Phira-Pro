use anyhow::Result;
#[cfg(target_os = "ios")]
use anyhow::{anyhow, Context};
use prpr::task::Task;

#[cfg(target_os = "android")]
pub fn select(reset: bool) -> Result<Task<Result<()>>> {
    use jni::{jni_sig, jni_str, objects::JObject, vm::JavaVM};
    let selected = JavaVM::singleton()?.attach_current_thread(|env| -> jni::errors::Result<bool> {
        let activity = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
        env.call_method(activity, jni_str!("selectLauncherIcon"), jni_sig!("(Z)Z"), &[reset.into()])?.z()
    })?;
    anyhow::ensure!(selected, "Android launcher icon could not be changed");
    Ok(Task::new(async { Ok(()) }))
}

#[cfg(target_os = "android")]
pub fn pin_custom_image(image: &str) -> Result<()> {
    use jni::{jni_sig, jni_str, objects::JObject, vm::JavaVM};
    let pinned = JavaVM::singleton()?.attach_current_thread(|env| -> jni::errors::Result<bool> {
        let activity = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
        let image = env.new_string(image)?;
        env.call_method(activity, jni_str!("pinCustomLauncherIcon"), jni_sig!("(Ljava/lang/String;)Z"), &[(&image).into()])?.z()
    })?;
    anyhow::ensure!(pinned, "Launcher declined the pinned shortcut or could not decode the image");
    Ok(())
}

#[cfg(target_os = "ios")]
pub fn select(reset: bool) -> Result<Task<Result<()>>> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSError, NSString};
    use objc2_ui_kit::UIApplication;

    let mtm = MainThreadMarker::new().context("App icon selection needs the main thread")?;
    let app = UIApplication::sharedApplication(mtm);
    anyhow::ensure!(app.supportsAlternateIcons(), "This iOS build does not support alternate app icons");
    let selected = app.alternateIconName();
    let name = (!reset && selected.is_none()).then(|| NSString::from_str("AppIconAlternate"));
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = std::sync::Mutex::new(Some(send));
    let complete = RcBlock::new(move |error: *mut NSError| {
        if let Some(send) = send.lock().unwrap().take() {
            let result = unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            let _ = send.send(result);
        }
    });
    app.setAlternateIconName_completionHandler(name.as_deref(), Some(&complete));
    Ok(Task::new(async move {
        if let Some(error) = receive.await.context("iOS did not report the result of the icon change")? {
            return Err(anyhow!(error));
        }
        Ok(())
    }))
}
