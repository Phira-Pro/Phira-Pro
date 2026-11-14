# Linux 性能优化改动记录

本文记录本次 PC-Linux 性能优化的落地范围、文件级改动与验证结果。设计和测量细节见：

- [Linux 帧率上限与帧调度](linux-frame-pacing.md)
- [Linux 调度优先级、显存与 TTM 保护](linux-scheduling-vram.md)

## 目标

在不改变判定、谱面内容、分辨率、字体像素和画面布局的前提下：

1. 让 Linux 桌面端拥有与 Windows 一致的帧率上限设置；
2. 尽量提高更新/渲染线程的调度优先级，并在硬件/权限不允许时安全退化；
3. 可观测核显 GEM/TTM 占用，在系统内存真正告急时只回收可重建缓存；
4. 降低高物量谱面的常驻内存，并提供音频缓冲调节入口；
5. 不修改 TTM 的全局页池参数，不引入需要 root 的默认行为。

## 文件改动

| 文件 | 改动 |
| --- | --- |
| `phira/src/lib.rs` | Linux 接入 `desktop_pacing::Pacer`；启动时调用 `desktop_priority::install()` 与 `desktop_memory::install()`；窗口配置根据已保存的 `desktopFpsLimit` 决定初始 GLX 交换间隔（仅 `UNLIMITED_FPS` 为 0，其余为 1）。 |
| `phira/src/page/settings.rs` | 桌面帧率上限设置对 Linux 开放；音频缓冲设置从 OHOS 扩展到所有非 Android 平台，增加 1024 档；相关值与语言刷新逻辑覆盖 Linux。 |
| `prpr/src/lib.rs` | 在 Linux 导出 `desktop_memory`、`desktop_priority` 模块；`desktop_pacing` 从仅 Windows 扩展为桌面模块。 |
| `prpr/src/config.rs` | 将 `desktop_fps_limit` 的注释从 Windows 专用改为桌面通用。 |
| `prpr/src/desktop_pacing.rs` | 按平台拆分实现，保留统一的 `Pacer::wait(limit)` 契约：`0` 跟随屏幕，`UNLIMITED_FPS` 不限，其他值为固定帧率。 |
| `prpr/src/desktop_pacing/linux.rs` | 新增 Linux pacer：固定档位在 update/render 前 `sleep + 150 µs spin`；跟随屏幕依赖合成器垂直同步；无上限通过 `GLX_EXT` / `GLX_MESA` / `GLX_SGI` 运行时设置交换间隔 0。 |
| `prpr/src/desktop_pacing/windows.rs` | 原有 Windows waitable-timer pacer 原样迁入子模块，行为不变。 |
| `prpr/src/desktop_priority.rs` | 新增尽力而为的调度请求：`PR_SET_TIMERSLACK=1 ns`；`RLIMIT_RTPRIO` 允许时尝试 `SCHED_RR` 优先级 1；`RLIMIT_NICE` 允许时尝试 nice -5；i915 上下文优先级探测与设置；失败只记录 debug/warn，不阻止启动。 |
| `prpr/src/desktop_memory.rs` | 新增显存/内存遥测与压力保护：每 2 秒采样 `/proc/self/fdinfo` 和 `/proc/meminfo`，按 `drm-client-id` 去重；启动基线 + 每 60 秒日志；Critical 时产生一次图集回收代次，30 秒冷却。 |
| `prpr/src/ui/text.rs` | Linux 字体图集从 1024² 起始，按需增长，上限 2048²；到顶后清空复用，不再分配 4096²；新增压力代次检查与回收；非 Linux 保持原行为。 |
| `prpr/src/scene/game.rs` | 移除 `GameScene.chart_bytes` 字段；练习密钥计算完成后 `drop(chart_bytes)`，避免 189 MB 原始谱面在整局常驻。 |
| `prpr/examples/frame_pacing_probe.rs` | 探针支持 Linux pacer、`FRAME_PROBE_LIMIT` 和原生 Wayland 对照后端；原有 Windows 用法保留。 |
| `docs/development/linux-frame-pacing.md` | 新增 Linux 帧率上限、GLX swap-control、实测和残余风险说明；修正设置路径为“调试 → 帧率设置”。 |
| `docs/development/linux-scheduling-vram.md` | 新增调度优先级、i915 能力探测、fdinfo 去重、图集尺寸化、压力保护、TTM 分析和实测说明。 |
| `docs/development/linux-performance-changes.md` | 本文件。 |

## 行为细节

### 帧率上限

- **跟随屏幕（0）**：不额外休眠，保留垂直同步，由合成器决定交换节奏。默认值。
- **固定帧率（60..480）**：主循环在 update/render 前按周期休眠，最后 150 µs 自旋；交换仍保留垂直同步。高于刷新率的档位自然落到刷新率。
- **无上限（65535）**：通过 GLX 扩展把交换间隔设为 0，并跳过 pacer 休眠。若运行时扩展不可用，会记录 warning，设置仍保存，重启后由窗口配置生效。

### 调度优先级

- 定时器精度只影响当前线程的 `nanosleep` 唤醒误差；不修改系统级 timer resolution。
- RT/nice 只在 RLIMIT 允许时请求；参考机器权限为 0，因此实际不执行。
- i915 上下文优先级需要 `I915_SCHEDULER_CAP_PRIORITY` 和 `CAP_SYS_NICE`。参考机器 Haswell 的 `I915_PARAM_HAS_SCHEDULER` 为 `0x0`，`SETPARAM` 返回 `ENODEV`，因此是 no-op；代码路径保留，便于换机验证。

### 显存/内存压力

- 遥测不是每帧 I/O；后台线程每 2 秒采样，GL 线程只读原子代次。
- 压力分级：`MemAvailable < 512 MiB` 为 Low；`< 256 MiB` 或（swap 剩余 < 64 MiB 且可用内存 < 1 GiB）为 Critical。
- 只在进入 Critical 时请求一次图集回收，30 秒冷却，避免反复裁剪/栅格化。
- 回收只针对可重建的字体图集；字体、文本、布局、颜色不变。
- TTM `page_pool_size` 未修改：本机 976437 页（约 3.72 GiB）是系统空闲页缓存参数，不是单应用配额；应用占用约 300 MiB，调整它没有收益且会影响整机。

### 音频缓冲

- 配置项默认仍为 `None`，由平台音频后端选择设备默认值；设置页在未写入时回退显示 256。
- Linux 设置页提供 128/256/512/1024 四档；本次压测现场使用 1024（约 23.2 ms @44.1 kHz），512 约 11.6 ms。
- 设置需要重启音频设备/游戏后生效。
- 压力谱面的少量 underrun 集中在 FPS 低谷，继续增大缓冲只会增加延迟，不改善帧率。

## 验证

```bash
cargo check -p prpr --lib
cargo check -p phira-main
cargo test -p prpr --lib desktop_       # 8 个桌面端测试通过
cargo test -p prpr --lib chart_loading  # 1 通过 / 2 ignored
cargo build --release -p phira-main     # 约 5 分 30 秒
```

### 普通谱面

Desultory Signals（约 2k notes），跟随屏幕，1024 帧：

| 指标 | 结果 |
| --- | --- |
| 帧间隔 p50 / p95 / p99 | 16.67 / 18.19 / 19.19 ms |
| 菜单 GPU | 149 → 103 MiB |
| 菜单 RSS | 约 252.6 MiB |
| 字体图集 | 3 × 1024² |

### SkyFire（5,000,000 notes）

| 指标 | 改动前 | 改动后 |
| --- | --- | --- |
| 解析 | 约 43 s | 约 26 s（冷）/ 12 s（热缓存重开） |
| RSS / VmSwap | 约 2.5 GB / 414–640 MB | 约 3.0 GB / **0** |
| GPU total / resident | 约 300 / 109–155 MiB | 284 / 186 MiB |
| 实际 FPS | 8–30，随后 33–60 | 大多 30–40，部分 60，偶发 8–25 |
| 音频 underrun | 大量 | 仅低谷附近少量（1024 缓冲） |

结论：swap 抖动消除后，5M 物量谱在参考机器上大多能维持 30+ FPS；剩余瓶颈是 Note/Object 数据量（约 2 GB）和每帧 7–20 ms 的 update/render，不是 DRM/TTM/VRAM 配置。要冲 60 或显著降内存，需要另做 Note/Object 紧凑表示（潜在节省约 1 GB），不在本次低风险改动范围。

## 包与清理

本快照不包含构建缓存、可再生成的第三方资源、Git 历史或本机运行数据。若从源码重新构建，需要：

- Linux 上带 GLX 的桌面会话（X11 或 XWayland）；
- 允许构建脚本联网获取 `prpr-avc` 的 Linux 静态 FFmpeg 库与 `assets/rpen-editor/`，或提前准备这两部分；
- 首次运行时由程序创建 `data/`，自行导入谱面。

详细复现命令和已知限制见仓库根目录 `README.md`。
