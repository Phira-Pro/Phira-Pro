# 版本维护与跨平台构建

## 统一版本定义

根目录 `version.json` 是唯一人工维护的版本源：

```json
{
  "base_version": "0.8.2",
  "pro_revision": 9,
  "flash_revision": 1,
  "build_number": 44
}
```

`base_version` 是与官服交互的兼容版本，同时用于 Cargo 包版本和 iOS 系统展示版本。Pro 应用内版本、Android versionName 和发布包文件名由它与 `pro_revision` 拼成 `0.8.2-pro.9`；Flash 应用内版本为 `flash.1`。`build_number` 同时用于 Android versionCode 与 iOS CFBundleVersion。普通编译不递增版本，发布新包时显式提升构建号。

修改 JSON 后执行：

```sh
python scripts/version.py sync
python scripts/version.py check
```

同步工具仅更新 Cargo 的 workspace.package.version 和生成的 `xcode/Version.xcconfig`，不会修改依赖版本。基础版本变化后运行一次 Cargo 更新本地工作区包的 `Cargo.lock` 并一同提交。Rust 和 Gradle 直接读取 JSON；iOS 配置通过 Shared.xcconfig 引用生成文件。生成文件入库，以支持直接打开 Xcode；版本不一致时构建和 CI 会明确失败。官服请求继续使用基础版本。

## 本地构建

公共脚本使用 Python 3.8+ 的标准库，不需要安装 Python 包。Windows 可用 `python`，Linux / macOS 可按环境替换为 `python3`。各平台使用自身的编译环境：

| 目标 | 构建主机与依赖 | 命令 |
|---|---|---|
| Windows | Windows、Rust、Visual Studio C++ Build Tools / Windows SDK | `python scripts/build.py --platform windows --package` |
| Linux | Linux、Rust、C/C++ 工具链、pkg-config、ALSA 与 X11 / Wayland / OpenGL 开发包 | `python3 scripts/build.py --platform linux --package` |
| macOS | Apple Silicon Mac、Rust、Xcode Command Line Tools | `python3 scripts/build.py --platform macos --package` |
| Android | Windows / Linux / macOS、Rust、JDK 21、Android SDK 35、NDK 27.2.12479018、cargo-ndk | `python scripts/build.py --platform android` |
| iOS | macOS、Xcode、Rust 的 aarch64-apple-ios 目标 | `python3 scripts/build.py --platform ios` |

不带 `--platform` 时选择当前桌面系统。桌面不带 `--package` 时仅执行 release 构建，使用 Cargo 默认的本机目标；已有 release 程序可用 `--package --skip-build` 重新打包。设置 `CARGO_BUILD_TARGET` 时，文件名中的架构和程序查找路径随之调整，需要自行准备对应的链接工具链。

Rust 工具链由 `rust-toolchain.toml` 锁定。Linux 的 Ubuntu 构建依赖安装示例：

```sh
sudo apt-get install --no-install-recommends build-essential cmake pkg-config libasound2-dev libx11-dev libxi-dev libwayland-dev libgl1-mesa-dev libdbus-1-dev zlib1g-dev
```

ALSA 开发包用于音频后端，见 [CPAL 的 Linux 构建依赖说明](https://github.com/RustAudio/cpal#linux-build-dependencies)。桌面 ZIP 包含程序、仓库资源与许可证；Windows 同时复制 release 目录中的 DLL。更新日志和用户数据不打包。包名自动读取版本和目标架构，例如 `PhiraPro-v0.8.2-pro.9-win64.zip`。默认输出在仓库父目录的 `dist/<平台>`，可用 `--output` 指定交付目录。解压后在解压目录运行 phira-main（Windows 为 phira-main.exe）。

原有 Windows PowerShell 入口 `scripts/package-windows.ps1` 仍可使用，内部调用公共脚本；Android 的 `scripts/package-android.ps1` 负责本机 APK 签名校验与交付，也从 JSON 读取版本。两者不再接受手工指定版本的参数。

### WSL

把源码放在 WSL 的 Linux 文件系统中，例如 `~/projects/Phira-Pro`，再运行 Linux 构建命令；不要将 Windows 的 `target`、Gradle 构建输出与签名文件一起复制。这样可避免 `/mnt` 下大量小文件读写的开销，也能保持 Windows 和 Linux 的编译缓存独立。

WSL 未安装 Cargo 时，可通过 [rsproxy](https://rsproxy.cn/) 安装 Rust；以下环境变量仅对当前终端生效：

```sh
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
curl --fail --location https://rsproxy.cn/rustup-init.sh -o /tmp/rustup-init.sh
sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain none
. "$HOME/.cargo/env"
cd ~/projects/Phira-Pro
rustup toolchain install
python3 scripts/build.py --platform linux --package
```

Rust 工具链镜像不替代 Cargo 的 crate 和 Git 依赖下载。使用代理时，先在 WSL 内检查 `https_proxy` 指向的地址是否可达，再执行 `cargo fetch --locked`；保留现有可用代理即可。Ubuntu 系统依赖使用上面的 APT 命令，软件源发行版应与 `/etc/os-release` 一致。

### Android

设置 `JAVA_HOME`、`ANDROID_HOME`、`ANDROID_NDK_HOME`，安装 `aarch64-linux-android` Rust 目标与 cargo-ndk。Gradle 负责 Rust 原生库和 APK 的完整构建，直接运行 `phira-android/gradlew assembleRelease` 也会读取统一版本。当前 APK 为 arm64-v8a。

本地签名从未入库的 `phira-android/keystore.properties` 读取，或使用 `PHIRA_PRO_KEYSTORE_PATH`、`PHIRA_PRO_KEYSTORE_PASSWORD`、`PHIRA_PRO_KEY_ALIAS`、`PHIRA_PRO_KEY_PASSWORD` 环境变量。未配置签名时 Gradle 生成未签名 release APK。签名密钥不提交。

### iOS

公共构建脚本生成未签名 iOS Release 应用，产物位于 `build/dd/Build/Products/Release-iphoneos`。IPA 打包由 iOS 工作流完成，文件名使用完整 Pro 版本。直接打开 Xcode 前先同步版本；本地签名仍由不入库的 `xcode/LocalSigning.xcconfig` 配置。

## GitHub Actions

三个工作流均通过 Actions → 选择工作流 → Run workflow 手动触发：

- **Build Desktop Packages**：Windows、Linux、macOS 三个独立 runner 并行构建 release ZIP。任一平台失败不会取消其他平台；每项上传自己的 ZIP Artifact。
- **Build Android APK**：构建 arm64 APK、校验组织 Secret 注入的签名并上传 Artifact。
- **Build iOS IPA**：在 macOS runner 上用 Xcode 构建并上传未签名 IPA，安装前需自行签名。

各工作流先执行版本一致性检查；桌面工作流还运行版本同步和打包工具的回归测试。桌面目标为 Windows x64、Linux x86_64、macOS arm64；macOS 和 iOS 使用 `macos-15` runner，Actions 使用 Node 24 运行时。当前 FFmpeg 发布包只提供 macOS arm64 静态库；Intel Mac 需自行准备对应 FFmpeg 库，并通过 `PRPR_AVC_LIBS` 指向含目标子目录的库根目录。工作流不自动创建 GitHub Release。Android CI 所需的组织 Secrets 为 `PHIRA_PRO_KEYSTORE_BASE64`、`PHIRA_PRO_KEYSTORE_PASSWORD`、`PHIRA_PRO_KEY_ALIAS`、`PHIRA_PRO_KEY_PASSWORD`。

工具回归命令：`python -m unittest discover -s scripts/tests -v`。
