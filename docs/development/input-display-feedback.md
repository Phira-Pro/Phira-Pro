# Early/Late、触控与刷新率

## Early/Late 提示

「设置 → 谱面」提供独立开关，默认关闭。位置可选偏移指示器下方和游玩区域底部，水平居中；指示器关闭时仍能使用提示。实际误差为负显示 EARLY，为正显示 LATE，短暂放大、上浮后淡出，持续 0.65 个谱面秒。后续 Perfect+ / Perfect 不产生新提示，其他有效命中刷新提示。重试、跳转和回放沿用判定记录的时间轴。

普通和细致判定均可使用。没有实际击打误差的 Miss、Drag/Flick 合成误差不用于推断方向；配置不参与计分及成绩资格判断。

## 画质与输入

低耗预设只关闭粒子、噪域辉光、折射及抗锯齿，不降低输入采样或判定更新频率。

同帧的按下与移动合并时，保留按下坐标、时间和阶段；原先只保留 Started 阶段，却使用后续移动坐标和时间，可能让快速点击离开原始命中位置。Flick 检测仍逐个处理移动事件。相同帧内的抬起保留一次有效点击，取消则立即删除；指针 ID 重用重新开始独立接触。

判定坐标和本家流程的空间尺度显式使用 Resource 中的屏幕游玩视口，不再从当前渲染状态读取。关闭 MSAA、切换后处理或调整宽高比不会改变这条输入路径。

触摸事件进入判定前会先经过回放录制的输入观察点。该路径原先为每个事件分配两个字符串保存来源与阶段，现在改为借用静态标签；序列化字段和记录内容不变，录制与回放行为一致。此项只减少输入回调内的堆分配，不改变判定时间或坐标；单指点击时的瞬时帧率下降尚未在源码中定位到确定原因，仍需以真机诊断确认。

## 与原版的差异及处理范围

对照 [原版依赖定义](https://github.com/TeamFlos/phira/blob/main/Cargo.toml) 中的 miniquad `138679c` 及其 [iOS 驱动](https://github.com/Mivik/prpr-miniquad/blob/138679c/src/native/ios.rs)：原版由 GLKViewController 设置 preferredFramesPerSecond 并负责绘制。此前 Pro 暂停该控制器，另建 CADisplayLink 调用 GLKView.display。当前恢复原版驱动路径，只在生命周期、用户设置和有界低帧恢复时重申偏好。诊断记录真实游戏帧循环间隔和更新绘制耗时；不再把它称为额外显示链接的回调，也不将它当作硬件呈现测量。旧诊断字段 target_timestamp 和 glk_display_ms 无独立测量时为 0。

Android 原本已经使用 Surface.setFrameRate，问题不能简单归为漏调用 API。窗口和 Surface 现在使用相同的用户目标；Android 12 起始终只允许无缝切换。此前窗口有焦点时使用 CHANGE_FRAME_RATE_ALWAYS，可能使焦点恢复产生黑屏；[Android 文档](https://developer.android.com/media/optimize/performance/frame-rate) 明确说明非无缝模式切换会短暂黑屏。具体申请与诊断见 [Android 刷新率说明](android-refresh-rate.md)。

以上是源码中确认的差异与修复范围，不代表已在反馈设备上确认黑屏或 80 FPS 的唯一原因。系统截获的三指手势可能产生取消、失焦或真实 Activity 暂停；不能通过伪造持续触摸或忽略后台暂停来掩盖，否则会留下虚假 Hold 或后台继续判定。失焦、生命周期和触摸取消保留本地诊断入口。

## 验证入口

- Rust 判定回归覆盖快速按下移动抬起、取消、指针重用和 Early/Late 档位、方向、持续时间及配置兼容。
- `cargo test -p prpr --lib input_capture` 覆盖输入观察点的顺序、取消与丢弃上限，`cargo test -p phira --lib replay::` 覆盖录制与分析的读取路径。
- `python scripts/verify_android_multitouch.py` 覆盖三至十指输入转发。
- `python scripts/verify_android_refresh_rate.py` 覆盖窗口与 Surface 目标、稀疏档位和生命周期去重。
- 真机需要对照主界面及游玩、低耗及高质量、三指及四指上滑、60 Hz 及 120 Hz、快速后台恢复，并区分应用 FPS、系统报告刷新率与硬件呈现。
