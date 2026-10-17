# Issue #2：iOS 锁屏恢复帧率与诊断返回

对应反馈：[issue #2](https://github.com/Phira-Pro/Phira-Pro/issues/2)。应用版本保留 pro.15，内部构建号 76。未发布。

## 修改

- 锁屏/后台恢复时作废旧 CADisplayLink，再创建、配置并注册新链接；手动更改刷新率和引导式访问状态变化同样刷新链接。
- 同时接收 miniquad 生命周期回调与 UIKit 通知；重复的 active 通知不重复创建链接。继续暂停 GLKViewController 的自动循环，避免双循环渲染。
- 持续低回调频率且渲染有余量时，允许每次激活/设置请求最多两次恢复；每次等待两秒，再采样至少一秒。低电量模式、严重或临界温控、渲染负载过高时不作此恢复。
- 诊断 JSON 增加显示链接 timestamp、targetTimestamp、回调间隔、进入间隔、回调工作时间、GLKView.display 时间、链接代数，以及带时间的生命周期与系统状态变化记录。仅开启采样时保留，最多 7200 帧与 256 事件。
- 页面显示实际测得的回调频率、读取回来的请求区间、低电量/温控/引导状态。主循环间隔不再称为“帧耗时”；请求区间也不视为系统保证的实际屏幕刷新率。
- 诊断页不再吞掉属于页面标题区的触摸，使主场景返回按钮可点击。

## 核查依据

Apple 的 [CADisplayLink 文档](https://developer.apple.com/documentation/quartzcore/cadisplaylink) 和 [preferredFrameRateRange 文档](https://developer.apple.com/documentation/quartzcore/cadisplaylink/preferredframeraterange) 明确区分请求与实际回调，系统功耗、温控和无障碍设置可影响帧率。此修改不会绕过系统限制，也不能仅凭代码检查认定 LiveContainer / iPadOS 的问题已消失。

## iPad 复验流程

1. 120 Hz 开启跨页面采样，清空采样；开启引导式访问，记录正常显示回调频率。
2. 锁屏五秒后解锁，等待至少二十秒，导出诊断。检查 pause/resume/active 顺序、链接代数、60/120 Hz、请求区间及温控/低电量状态。
3. 重复三次锁屏唤醒；另测设置内 60 → 120、结束/重开引导式访问。确认不会双循环，也不会无限重建链接。
4. 分别用原反馈的 LiveContainer 环境与直装环境对照；同一采样不要开启录屏、Instruments 或帧性能分析。
5. 点击诊断页返回，确认回到设置；跨页面采样开启时回到游玩再返回，确认采样仍保留。

Windows 的页面操作、恢复策略回归及 iOS 目标 Rust 类型检查可以本地执行；引导式访问锁屏的实际系统行为需要对应 iPad 运行上述流程。

## 本轮已执行

- `cargo check -p prpr --no-default-features --target aarch64-apple-ios`：通过，包括实际 objc2 / UIKit / QuartzCore 绑定。
- `cargo test -p prpr --lib --release display_`：5 项通过，覆盖恢复上限与重新激活、正常/繁忙/系统限频、暂停间隔、采样容量和导出字段区分。
- Windows 原生界面：旧 build 75 点击返回仍停在诊断页；新 build 76 返回成功。开启跨页采样后返回再进入，采样保留；清空、开关、JSON 保存交互通过。实际导出文件位于本地 `target/issue2-native-diagnostics.json`，标明 build 76。
- 完整 `phira` 的 iOS 交叉检查被 Windows 缺少 Xcode 的 `xcrun` / iOS SDK 阻止；未据此宣称完整 IPA 编译或 iPad 系统行为已通过。未 push 或触发 Actions。
