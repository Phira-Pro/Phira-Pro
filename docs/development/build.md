# 版本维护与跨平台构建

## 统一版本定义

根目录 `version.json` 是唯一人工维护的版本源：

```json
{
  "base_version": "0.8.2",
  "pro_revision": 12,
  "flash_revision": 1,
  "build_number": 49
}
```

`base_version` 是与官服交互的兼容版本，同时用于 Cargo 包版本和 iOS 系统展示版本。Pro 应用内版本、Android versionName 和发布包文件名由它与 `pro_revision` 拼成 `0.8.2-pro.12`；Flash 应用内版本为 `flash.1`。`build_number` 同时用于 Android versionCode 与 iOS CFBundleVersion。普通编译不递增版本，发布新包时显式提升构建号。

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

ALSA 开发包用于音频后端，见 [CPAL 的 Linux 构建依赖说明](https://github.com/RustAudio/cpal#linux-build-dependencies)。桌面 ZIP 包含程序、仓库资源与许可证；Windows 同时复制 release 目录中的 DLL。更新日志和用户数据不打包。包名自动读取版本和目标架构，例如 `PhiraPro-v0.8.2-pro.12-win64.zip`。默认输出在仓库父目录的 `dist/<平台>`，可用 `--output` 指定交付目录。解压后在解压目录运行 phira-main（Windows 为 phira-main.exe）。

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

三个平台工作流可以通过 Actions → 选择工作流 → Run workflow 单独手动触发，也供自动发版工作流复用：

- **Build Desktop Packages**：Windows、Linux、macOS 三个独立 runner 并行构建 release ZIP。任一平台失败不会取消其他平台；每项上传自己的 ZIP Artifact。
- **Build Android APK**：构建 arm64 APK、校验组织 Secret 注入的签名并上传 Artifact。
- **Build iOS IPA**：在 macOS runner 上用 Xcode 构建并上传未签名 IPA，安装前需自行签名。

各工作流先执行版本一致性检查；桌面工作流还运行版本同步和打包工具的回归测试。桌面目标为 Windows x64、Linux x86_64、macOS arm64；macOS 和 iOS 使用 `macos-15` runner，Actions 使用 Node 24 运行时。当前 FFmpeg 发布包只提供 macOS arm64 静态库；Intel Mac 需自行准备对应 FFmpeg 库，并通过 `PRPR_AVC_LIBS` 指向含目标子目录的库根目录。Android CI 所需的组织 Secrets 为 `PHIRA_PRO_KEYSTORE_BASE64`、`PHIRA_PRO_KEYSTORE_PASSWORD`、`PHIRA_PRO_KEY_ALIAS`、`PHIRA_PRO_KEY_PASSWORD`。

### 发布 Release 自动构建

**Build Release Assets**（`.github/workflows/release.yml`）监听 `release.published`，正式版和预发布版均触发；创建草稿或日常 push 不触发构建。工作流不代替维护者创建或发布 Release，也不修改 Release 正文。

1. 修改 `version.json` 中的 Pro 版本并提升 `build_number`，执行 `python scripts/version.py sync` 和 `python scripts/version.py check`，提交版本文件与同步结果。
2. 将包含发版工作流和版本更新的提交推送到 GitHub，再为该提交创建 Tag，例如 `v0.8.2-pro.12`。Tag 必须与该提交的 `pro_version` 一致；也接受不带 `v` 的 `0.8.2-pro.12`。
3. 在 Releases 页面选择这个 Tag，填写更新说明并发布 Release；标记为预发布版也会构建。
4. 工作流检查版本、Release 状态和 Tag 提交，随后并行调用桌面、Android 和 iOS 工作流。各平台固定构建同一个提交 SHA。全部成功后才开始向该 Release 上传完整附件集。

以当前版本为例，附件为：

| 平台 | 附件 |
|---|---|
| Windows x64 | `PhiraPro-v0.8.2-pro.12-win64.zip` |
| Linux x86_64 | `PhiraPro-v0.8.2-pro.12-linux-x86_64.zip` |
| macOS arm64 | `PhiraPro-v0.8.2-pro.12-macos-aarch64.zip` |
| Android arm64-v8a | `PhiraPro-v0.8.2-pro.12-android-arm64-v8a.apk`，使用现有密钥签名 |
| iOS arm64 | `PhiraPro-v0.8.2-pro.12-ios-arm64-unsigned.ipa`，安装前自行签名 |
| 校验清单 | `SHA256SUMS`，包含上述五个文件的 SHA-256 |

发布前需允许 GitHub 官方 Actions 和标准托管 runner，四项 Android 签名 Secrets 对本仓库可用。仓库默认 `GITHUB_TOKEN` 权限可以保持只读，只有最终上传 job 声明 `contents: write`，无需额外 PAT。仓库的 **Settings → General → Releases → Enable release immutability** 必须关闭；已发布的不可变 Release 会在构建前被拒绝，因为它无法追加附件。详见 [Release 触发规则](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#release) 和 [不可变 Release 设置](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/establish-provenance-and-integrity/prevent-release-changes)。

若某个平台失败，其余平台仍会完成并保留 Actions Artifact；该次运行不会进入上传 job。修复环境或下载失败后，优先使用 **Re-run failed jobs**，复用本次已成功平台的产物。若修复需要修改源码，提升版本和构建号后发布新的 Tag，避免移动已发布 Tag。

上传不是 GitHub API 的原子操作：网络中断可能留下部分附件。重跑失败的上传 job 会跳过 SHA-256 和大小均一致的已有附件，只补齐缺失附件；已有同名附件内容不同或缺少可验证的摘要时会明确失败，不自动覆盖，其他维护者附件也会保留。检查冲突附件后可手动移除它再重跑。上传前还会检查 Tag 没有移动、Release 没有被删除后重建。

**Build Release Assets → Run workflow** 也接受 `tag` 参数，用于为已有的已发布 Release 补建附件；手动入口需让工作流文件先进入默认分支。已经存在的旧 Tag 若缺少新构建脚本或版本定义，需要单独处理，手动入口不会为旧提交注入新源码。相关实现见 `scripts/release.py`。

工具回归命令：`python -m unittest discover -s scripts/tests -v`。
