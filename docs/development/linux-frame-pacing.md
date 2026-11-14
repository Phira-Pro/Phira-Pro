# Linux 帧率上限与帧调度

## 行为

「设置 → 调试 → 帧率设置 → 帧率上限」现在在 Linux 桌面端可用，档位与 Windows 一致：跟随屏幕（默认）、固定帧率、无上限。选择后立即生效并保存。

- **跟随屏幕**：保持垂直同步，交换节奏由合成器决定，与改动前完全一致。
- **固定帧率**：主循环在 update/render 前按目标周期休眠（最后一个自旋窗口用于精度），呈现仍由垂直同步兜底。低于刷新率的档位得到稳定降帧；高于刷新率的档位自然落到刷新率。
- **无上限**：运行时把交换间隔设为 0，不再等待垂直同步。通过 `GLX_EXT_swap_control` 实现，按 `GLX_EXT`、`GLX_MESA`、`GLX_SGI` 的顺序探测；驱动不提供任何 swap-control 时记录警告，设置仍会保存，但需要重启才能通过窗口配置生效。

实现位于 `prpr/src/desktop_pacing/linux.rs`，与 Windows 的 `Pacer` 共用 `wait(limit)` 契约；主循环、启动配置和设置页只按平台拆开接入口。

## 为什么不是 Wayland 原生后端

本仓库固定的 miniquad 分支带有 `linux_wayland` 后端，但它不调用 `eglSwapInterval`，无法解除垂直同步；本机实测把交换间隔设为 0 后仍锁定 60 FPS。上游同时把该实现标记为不稳定，输入、剪贴板与窗口尺寸等能力也不完整。因此 Linux 继续使用 X11/GLX 路径（Wayland 会话下经 XWayland），帧率能力通过 GLX 扩展补齐。

## 实测

环境：i5-4300U / Intel HD Graphics 4400 / Mesa 26.0.8 / KWin Wayland 会话（游戏经 XWayland）/ 1366×768@59.98 Hz / 游戏窗口 973×608。

| 场景 | 结果 |
| --- | --- |
| 跟随屏幕（默认） | 59.9–60.0 FPS；帧间隔 p50 16.6 ms、p95 17.7–18.3 ms；菜单 update+render 均值约 0.6 ms、游玩约 1.0–2.6 ms |
| 无上限 | 探针 713–715 FPS；菜单约 110–125 FPS；游玩（2026 note，autoplay）约 100–125 FPS |
| 30 FPS 上限（探针） | 30.00 FPS；帧间隔 p50 33.333 ms、p95 33.605 ms |
| 120 FPS 上限（60 Hz 屏幕） | 59.9 FPS（由垂直同步兜底） |

结论：本机在默认垂直同步下 CPU/GPU 均有余量，帧率上限的价值在于低延迟（无上限）与可预期的降帧；固定档位在 60 Hz 屏幕上不会改变画面内容，只改变调度。

## 验证入口

- `cargo test -p prpr --lib desktop_pacing` 覆盖档位映射与错过截止时间后的重新排程。
- `cargo build --release -p phira-main`。
- 帧调度探针：`FRAME_PROBE_SWAP=1 FRAME_PROBE_PACER=1 FRAME_PROBE_LIMIT=<0|30|65535> cargo run --release -p prpr --example frame_pacing_probe`；Linux 下同时支持 `FRAME_PROBE_WAYLAND=1` 对照后端。
- 游戏内：设置 → 调试 → 帧率设置 → 帧率上限；用 `PHIRA_FRAME_PROFILE=<csv>` 观察 `pacing_ms`、`present_ms` 与帧间隔。

## 残余风险

- 多显示器且刷新率不一致时，固定档位按用户选择的周期调度，不随窗口所在显示器自动调整；「跟随屏幕」始终由合成器与垂直同步决定。
- 无上限解除了垂直同步，画面更新节奏由合成器采样决定；在未启用合成器的裸 Xorg 会话中可能出现撕裂，这是该档位的预期取舍。
- 若窗口后端改用 EGL（例如后续启用 Wayland），运行时 swap-control 可能不可用，需要重新实现并复测。

Linux 的调度优先级、显存遥测与 TTM 相关工作见 [linux-scheduling-vram.md](linux-scheduling-vram.md)。
