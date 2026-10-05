#[path = "../build_support/version.rs"]
mod version;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = std::path::Path::new(&manifest_dir).parent().unwrap();
    let versions = version::Version::read(root);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        versions.check_xcode(root);
    }

    // 真机构建需要额外链接参数：部署目标是 iOS 12，而 objc2 对 iOS 14 才引入的
    // UniformTypeIdentifiers 发的是强链接，不弱链接的话 app 在 iOS 12 / 13 上一启动就崩。
    // 放在这里而不是 `.cargo/config.toml`，是为了让 CI 和本地构建都自动生效，
    // 也避免把 Xcode 的绝对路径写进仓库。
    // 用 `TARGET` 三元组判断，因为模拟器的 `CARGO_CFG_TARGET_OS` 同样是 `ios`，
    // 而 `-miphoneos-version-min` 在模拟器上并不适用。
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_ios_device = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") && !target.ends_with("-sim");
    if is_ios_device {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,UniformTypeIdentifiers");
        println!("cargo:rustc-link-arg=-miphoneos-version-min=12.0");
    }
}
