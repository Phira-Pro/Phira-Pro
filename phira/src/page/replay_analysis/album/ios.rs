use anyhow::{Context, Result};
use objc2::{define_class, msg_send, rc::Retained, sel, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString};
use objc2_ui_kit::UIImage;
use std::{cell::RefCell, path::Path, sync::Mutex};
static RESULT: Mutex<Option<Result<String>>> = Mutex::new(None);
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {static DELEGATE:RefCell<Option<Retained<AlbumDelegate>>>=const{RefCell::new(None)};}
define_class! {
    #[unsafe(super=NSObject)]
    #[thread_kind=MainThreadOnly]
    struct AlbumDelegate;
    unsafe impl NSObjectProtocol for AlbumDelegate {}
    impl AlbumDelegate {
        #[unsafe(method(image:didFinishSavingWithError:contextInfo:))]
        fn saved(&self,_image:&UIImage,error:Option<&NSError>,_context:*mut std::ffi::c_void) {
            PENDING.store(false,std::sync::atomic::Ordering::Relaxed);
            *RESULT.lock().unwrap()=Some(match error{Some(e)=>Err(anyhow::anyhow!("相册保存失败：{}",e.localizedDescription())),None=>Ok("图片已保存到相册".into())});
        }
    }
}
pub fn start(path: &Path) -> Result<()> {
    let mtm = MainThreadMarker::new().context("相册保存需要在主线程发起")?;
    anyhow::ensure!(!PENDING.load(std::sync::atomic::Ordering::Relaxed),"上一张图片仍在保存，请稍后重试");
    let image = UIImage::imageWithContentsOfFile(&NSString::from_str(path.to_string_lossy().as_ref())).context("无法读取分析图片")?;
    let delegate: Retained<AlbumDelegate> = unsafe { msg_send![super(AlbumDelegate::alloc(mtm).set_ivars(())), init] };
    *RESULT.lock().unwrap() = None;
    PENDING.store(true,std::sync::atomic::Ordering::Relaxed);
    unsafe {
        image.write_to_saved_photos_album(Some(&delegate), Some(sel!(image:didFinishSavingWithError:contextInfo:)), std::ptr::null_mut());
    }
    DELEGATE.with(|v| *v.borrow_mut() = Some(delegate));
    Ok(())
}
pub fn take() -> Option<Result<String>> {
    RESULT.lock().unwrap().take()
}
