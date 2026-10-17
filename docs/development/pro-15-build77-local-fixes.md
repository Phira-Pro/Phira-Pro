# pro.15 build 77：安卓刷新率与练习目标准度

本轮修复整理纳入 main 源码，包含上轮 iOS issue #2 的修复。用户授权仅推送源码，不重新出包、不触发 Release / Actions；PR #3 的补丁已在本地采用，未操作远端 PR 的合并按钮。

## 同步远端

本地 main 快进到 `7f17be2b`，纳入 `f833699d` 和 `7f17be2b`：PGR 重叠事件、最终事件端点和零时长插值修复。

## 练习百分比

审阅并应用 wiorigami 的 [PR #3](https://github.com/Phira-Pro/Phira-Pro/pull/3)，原提交 `59ca1449788b6a8bcede5d9f400c74a88d163826`。步进与文本解析使用同一 `control_scale`；目标准度与其他百分比项统一换算，移除该分支额外的除 100。95% 点加号应成为 96%，不是 0.96%。

## 安卓刷新率

- Android 11+ 用 Surface 请求用户选择的刷新率，不再把物理模式 ID 固定到临时的 60 Hz 模式。Android 11～13 清除冲突的窗口刷新率投票；Android 14+ 同时按系统支持的 API 语义传递用户请求。
- Android 10 及更早保留同分辨率物理模式选择，对 144.000x 等接近标称值的模式使用 0.5 Hz 容差；不为提高刷新率改变分辨率。
- 恢复焦点、全屏/多窗口切换、配置变化、Surface 创建或尺寸变化时重新申请。一次转换只延后补发两次（300 ms / 1000 ms），不使用永久轮询；后台和失焦取消补发。
- Android 12+ 在这些明确的恢复/用户设置时机允许系统执行非无缝切换，系统可能有短暂显示过渡；常规显示变化通知不反复执行此动作。Surface 销毁期间的异常不使应用崩溃。
- 诊断 JSON 增加 `android_display`：用户请求、系统上报刷新率/模式、窗口请求、是否多窗口、省电、温控、采集时间。`reported_display_hz` 不代表应用真实渲染 FPS，也不把请求成功等同于系统已采用。

依据 Android 官方 [WindowManager.LayoutParams](https://developer.android.com/reference/android/view/WindowManager.LayoutParams#preferredRefreshRate) 与 [Surface.setFrameRate](https://developer.android.com/reference/android/view/Surface#setFrameRate(float,int,int))。系统省电、温控或厂商单独限帧仍可能覆盖应用请求。

## 本地验证

- `cargo test -p prpr --lib --release`：216 通过、2 个需要图形环境的测试忽略；包含 PR #3、PGR 新增回归与前轮显示恢复测试。
- `python scripts/verify_android_refresh_rate.py`：23 项断言通过，执行生产 Java 选择策略。
- Android SDK 35 `:app:compileDebugJavaWithJavac -x cargoNdkBuild --offline`：通过。
- Android NDK `cargo ndk -t arm64-v8a --platform 26 check --locked -p phira --no-default-features`：通过，包括 JNI 诊断导出桥接。
- Windows 使用当前源码构建后，实际操作练习侧栏：98% → 97% → 96% → 95%，点加号后显示 96.00%；输入框可弹出，文本换算由 PR 回归覆盖。交付 ZIP 完整性通过，原生操作使用的程序与 ZIP 内程序 SHA256 一致。

小米平板 6 Pro 的系统版本、游戏刷新率设置和系统限帧设置尚待反馈。本机验证不能证明该机在 MIUI/HyperOS 上已稳定恢复 144 FPS；需对照全屏/小窗导出的诊断 JSON 复验。
