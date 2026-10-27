//! Explicit image saving; desktop Pictures and mobile photo libraries.
use crate::replay::analysis::worker::Work;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
pub struct Save {
    #[cfg(not(target_os = "ios"))]
    work: Work<String>,
}
impl Save {
    pub fn start(path: PathBuf) -> Result<Self> {
        #[cfg(not(target_os = "ios"))]
        {
            Ok(Self {
                work: Work::spawn("prca-save-image", move || save(&path))?,
            })
        }
        #[cfg(target_os = "ios")]
        {
            ios::start(&path)?;
            Ok(Self {})
        }
    }
    pub fn take(&mut self) -> Option<Result<String>> {
        #[cfg(not(target_os = "ios"))]
        {
            self.work.take()
        }
        #[cfg(target_os = "ios")]
        {
            ios::take()
        }
    }
}
#[cfg(target_os = "windows")]
fn pictures() -> Result<PathBuf> {
    #[repr(C)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: *mut std::ffi::c_void, path: *mut *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(p: *mut std::ffi::c_void);
    }
    let id = Guid {
        a: 0x33e28130,
        b: 0x4e1e,
        c: 0x4676,
        d: [0x83, 0x5a, 0x98, 0x39, 0x5c, 0x3b, 0xc3, 0xbb],
    };
    let mut ptr = std::ptr::null_mut();
    let status = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut ptr) };
    anyhow::ensure!(status >= 0 && !ptr.is_null(), "无法取得系统图片目录");
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
    }
    use std::os::windows::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(ptr, len) }));
    unsafe { CoTaskMemFree(ptr as _) };
    Ok(path)
}
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn save(path: &Path) -> Result<String> {
    #[cfg(target_os = "windows")]
    let directory = pictures()?.join("Phira Pro");
    #[cfg(not(target_os = "windows"))]
    let directory = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("无法取得图片目录")?
        .join("Pictures/Phira Pro");
    save_to(path, &directory)
}
pub(super) fn save_to(path: &Path, directory: &Path) -> Result<String> {
    use std::io::Write;
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "需要有效的 PNG 图片");
    std::fs::create_dir_all(directory)?;
    let stem = path.file_stem().context("图片名称无效")?.to_string_lossy();
    for n in 0..1000 {
        let target = directory.join(if n == 0 { format!("{stem}.png") } else { format!("{stem}-{n}.png") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(&bytes).and_then(|_| f.sync_all()) {
                    drop(f);
                    let _ = std::fs::remove_file(&target);
                    return Err(e.into());
                }
                return Ok(format!("图片已保存到 {}", target.display()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("图片名称重复过多")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_image_preserves_existing_photos_and_rejects_non_images() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("PRCA.png");
        let album = root.path().join("相册");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([43, 85, 232, 255])).save(&source).unwrap();
        save_to(&source, &album).unwrap();
        let original = std::fs::read(album.join("PRCA.png")).unwrap();
        save_to(&source, &album).unwrap();
        assert_eq!(std::fs::read(album.join("PRCA.png")).unwrap(), original);
        assert!(album.join("PRCA-1.png").exists());
        std::fs::write(&source, b"invalid").unwrap();
        assert!(save_to(&source, &album).is_err());
        assert!(!album.join("PRCA-2.png").exists());
    }
}
#[cfg(target_os = "android")]
fn save(path: &Path) -> Result<String> {
    use jni::{
        jni_sig, jni_str,
        objects::{JObject, JString},
        vm::JavaVM,
    };
    let result = JavaVM::singleton()
        .context("Android Java VM 不可用")?
        .attach_current_thread(|env| -> jni::errors::Result<String> {
            let activity = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
            let source = env.new_string(path.to_string_lossy().as_ref())?;
            let name = env.new_string(path.file_name().unwrap_or_default().to_string_lossy().as_ref())?;
            let result = env
                .call_method(
                    activity,
                    jni_str!("saveImageToAlbum"),
                    jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                    &[(&source).into(), (&name).into()],
                )?
                .l()?;
            JString::cast_local(env, result)?.try_to_string(env)
        })?;
    if let Some(error) = result.strip_prefix("ERROR:") {
        anyhow::bail!("{error}");
    }
    Ok("图片已保存到相册 Phira Pro".into())
}
#[cfg(target_os = "ios")]
mod ios;
