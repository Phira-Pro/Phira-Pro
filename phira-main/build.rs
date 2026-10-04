use std::fs;

const XCCONFIG_PATH: &str = "xcode/Shared.xcconfig";
const XCCONFIG_VERSION_KEY: &str = "MARKETING_VERSION";

fn main() {
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let xcconfig_path = std::path::Path::new(&manifest_dir).parent().unwrap().join(XCCONFIG_PATH);
    let content = fs::read_to_string(&xcconfig_path).unwrap_or_else(|_| panic!("`{XCCONFIG_PATH}` not found"));

    let xcode_version = content
        .lines()
        .find(|l| l.trim_start().starts_with(XCCONFIG_VERSION_KEY))
        .and_then(|l| l.split_once('=').map(|x| x.1))
        .map(|v| v.trim())
        .unwrap_or_else(|| panic!("{XCCONFIG_VERSION_KEY} not found in {}", xcconfig_path.display()));
    if cargo_version != xcode_version {
        panic!(
            "Inconsistent Version:\n\
             Cargo.toml={cargo_version}, {XCCONFIG_PATH}={xcode_version}\n"
        );
    }

    println!("cargo:rerun-if-changed={}", xcconfig_path.display());
    println!("cargo:rerun-if-changed=Cargo.toml");

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
