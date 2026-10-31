# Android 刷新率机制与验证

## 申请链路

配置 `preferred_refresh_rate` 默认 120，启动加载、设置切换和配置导入通过 JNI 传给 Activity。主界面和游玩使用同一条渲染循环，没有分别设置 60 FPS 上限。Rust 主循环中的 Windows 帧率控制不会编译到 Android；Android 由原生循环渲染并通过 `eglSwapBuffers` 提交。

固定档位之外提供「无上限」。配置以 `65535` 保存该选项，传给系统前解析成系统报告的最高刷新率，绝不将该标记当作 65535 Hz 发送，也不通过发送 0 清除高刷请求。「无上限」随显示能力变化重新解析；旧系统选择同分辨率下的最高可用模式。它不绕过硬件或系统限制。

| 系统版本 | 窗口请求 | 游戏 Surface 请求 |
| --- | --- | --- |
| Android 10 及以下 | 同分辨率下满足目标的最低刷新率；不足时选最高可用档 | 没有公开的 `Surface.setFrameRate` API |
| Android 11–13 | 清除模式锁定和窗口刷新率投票，避免与 Surface 请求冲突 | 保留用户目标，使用 `FRAME_RATE_COMPATIBILITY_DEFAULT` |
| Android 14 及以上 | 直接投票用户目标，不锁定物理模式 ID | 保留用户目标，使用 `FRAME_RATE_COMPATIBILITY_DEFAULT` |

旧系统的刷新率申请不是渲染上限。例如只有 60/144 Hz 档位、请求 120 Hz 时，申请 144 Hz；只有 60 Hz 时选择 60 Hz。不跨分辨率选模式，不因为系统暂时报 60 Hz 就改写已保存的高刷目标。

Android 15 默认游戏帧率策略要求显式申请高刷，但“已经调用 API”和“已经生效”是两件事。AOSP 调度优先级为游戏模式干预、显式帧率请求、默认游戏帧率；高优先级策略可以覆盖应用请求。不能仅凭锁 60 FPS 就判定应用漏申请，也不能由屏幕最大 144 Hz 推出应用能呈现 144 FPS。

## 请求恢复

Activity 在创建、恢复、焦点、全屏和窗口配置变化时申请，并保留两次有界延迟重试。原生渲染循环处理恢复消息后再次申请，使恢复不只依赖早于 EGL 恢复的 Java 生命周期回调；快速暂停和恢复合并成一条消息时也会执行。

Surface 缓存以 Java 对象、生命周期代次、目标和切换策略判断是否需要重新提交，异常和销毁时失效。生命周期代次由公开的 SurfaceHolder 创建、尺寸变化和销毁回调计数，不读取隐藏的底层 generation API。Android 12 起，获得焦点的游戏窗口可申请非无缝切换，未获得焦点时只请求无缝切换。

显示回调先合并到一个待执行任务，读取突发通知后的最终状态；暂停或销毁时取消任务。显示 ID、模式 ID、应用刷新率报告、模式刷新率报告或最高刷新率报告变化时重新提交 Surface 请求。相同状态通知不重复投票，不重新启动延迟重试；窗口属性仍按差异更新，避免请求和显示通知形成反馈循环。不通过检测低 FPS 后不断重试来对抗温控或系统游戏限帧。

## 本地诊断

在工具的输入与帧时间诊断中开始采样，分别记录主界面和游玩，再导出本地诊断。现有帧时间样本是应用帧循环间隔，并不等同于硬件实际呈现时间。

`android_display` 是最近一次申请时的快照，不是实时系统查询。`uptime_ms` 是采集时刻，`snapshot_age_ms` 是导出时的快照年龄；温控、省电或游戏模式在此期间可能变化。导出本身不重新投票。字段包含：

- `requested_hz`、`surface_target_hz`：用户固定目标和 Surface 目标。「无上限」时 `unlimited` 为 true、`requested_hz` 为 null，`surface_target_hz` 为解析后的实际申请值。
- `surface_request_status`、`surface_submitted_hz`：请求待提交、已提交、调用失败或仅使用窗口 API。已提交只表示 API 正常返回，不能证明系统接受请求。
- `surface_generation`、`surface_change_strategy`：当前 Surface 生命周期本地计数与最近提交的切换策略。
- `reported_display_hz`、`physical_mode_hz`、`reported_maximum_hz`：系统提供给应用的刷新率报告、当前显示模式报告与最高值；这些值可能经过系统兼容或厂商策略处理，不能当作硬件测量。
- `android_sdk`、`target_sdk`、`display_id`、`window_preferred_hz`、`window_mode_id`、`supported_modes`：申请使用的 API 和显示环境。
- `reported_game_mode`、`low_power`、`thermal_status`：采集时可读取的系统状态。游戏模式枚举不包含实际限帧值，不能据此排除厂商限帧；游戏服务不可用时记录 null。Android 14 起，targetSdk 不高于 33 的应用可能将 CUSTOM 模式读成 STANDARD，因此 STANDARD 也不能证明没有自定义干预。

持续锁帧时，应将以上记录与设备上的 SurfaceFlinger 呈现时间、原生渲染耗时和 `eglSwapBuffers` 等待时间对照。申请错误、系统限帧、交换等待和渲染负载属于不同原因。通用回归不代表已经验证某个厂商系统的实际高刷效果。

## 验证

运行 `python scripts/verify_android_refresh_rate.py`，验证稀疏刷新率档位、同分辨率约束、浮点误差、系统版本策略、Surface 代次与切换策略变化、失败后的重试以及重复显示通知去重。Android 构建工作流也执行此回归。

真机检查冷启动、切换刷新率、主界面和游玩切换、快速切后台返回、窗口尺寸变化及显示模式变化。每次观察超过启动重试的 1 秒窗口，确认后续状态变化仍能提交请求；相同状态的重复通知不应造成持续模式切换。分别检查仅有 60 Hz、具有精确 120 Hz 和仅有 60/144 Hz 的设备，以及省电或温控限制解除后的表现。

## 官方依据

- [Android 游戏刷新率申请](https://developer.android.com/games/optimize/display-refresh-rate-change)
- [Frame Rate API、请求保持与游戏兼容性](https://developer.android.com/media/optimize/performance/frame-rate)
- [窗口刷新率及模式 ID 语义](https://developer.android.com/reference/android/view/WindowManager.LayoutParams#preferredRefreshRate)
- [GameManager 模式报告的兼容性映射](https://developer.android.com/reference/android/app/GameManager#getGameMode())
- [AOSP 游戏模式、显式投票与默认帧率的优先级](https://android.googlesource.com/platform/frameworks/native/+/8c2703dfc85c911d28111f5b1e54ee8acb7fcbb2%5E%21/)
