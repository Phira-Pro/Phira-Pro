//! Read editor folder resources as bytes, preserving Unicode names.
use anyhow::{Context, Result};
pub async fn load(name: &str) -> Result<Vec<u8>> {
    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::NSBundle;
        let root = NSBundle::mainBundle().resourcePath().context("应用资源目录不可用")?;
        let path = std::path::PathBuf::from(root.to_string()).join("assets/rpen-editor").join(name);
        std::fs::read(path).with_context(|| format!("编辑器资源读取失败：{name}"))
    }
    #[cfg(not(target_os = "ios"))]
    macroquad::file::load_file(&format!("rpen-editor/{name}")).await
        .with_context(|| format!("编辑器资源读取失败：{name}"))
}
