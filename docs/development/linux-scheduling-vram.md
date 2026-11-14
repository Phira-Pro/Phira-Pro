# Linux 调度优先级、显存与 TTM 保护

## 行为

Linux 桌面端启动时做一次尽力而为的调度请求，并在整个运行期间每 2 秒在后台采样一次 GPU / 系统内存：

- **定时器精度**：主线程设置 `PR_SET_TIMERSLACK = 1 ns`，让帧调度的 `nanosleep` 唤醒更精确；不改变系统级定时器分辨率。
- **CPU 调度**：`RLIMIT_RTPRIO` 允许时，把更新/渲染线程设为 `SCHED_RR` 的最低优先级 1；`RLIMIT_NICE` 允许时，把进程内已有线程的 nice 调到 -5。权限不足时静默跳过，不影响启动。
- **i915 上下文优先级**：读取 `I915_PARAM_HAS_SCHEDULER` 的 `PRIORITY` 能力，按 `PHIRA_DRM_PRIORITY`（默认 1）尝试提高本进程各 DRM 上下文的优先级；需要 `CAP_SYS_NICE`，硬件不支持或无权限时是无损 no-op。
- **显存遥测**：按 `drm-client-id` 去重汇总 `/proc/self/fdinfo` 里的 `drm-total-*`（兼容旧的 `drm-memory-*`），同时记录 `/proc/meminfo`、进程 RSS 与 Swap。启动完成后输出一次基线，之后每 60 秒输出一次。
- **显存压力保护**：`MemAvailable` 低于 256 MiB（或 swap 接近耗尽且可用内存低于 1 GiB）时，向字体图集发出一次性回收请求；图集回落到 1024²，之后按需重新栅格化。只回收可重建的缓存，不改变字体、布局、颜色或分辨率。
- **字体图集**：仅 Linux 桌面端，初始 1024²，按需增长，上限 2048²；到顶后清空缓存复用，而不是继续翻倍到 4096²（64 MiB）。非 Linux 保持原来的 2048² 起始、无上限行为。

所有请求都只在“内核/硬件允许”时生效；失败路径只写 debug 日志，不影响渲染。

## 背景

参考机器是 Haswell 核显笔记本：i5-4300U / HD Graphics 4400（GT2）/ Mesa 26.0.8 / KWin Wayland 会话（游戏经 XWayland）/ 1366×768@59.98 Hz / 游戏窗口 973×608 / 内核 7.0。

核显的“显存”就是系统内存：i915 的 GEM 对象由 shmem 页支撑，驱动把每个 DRM client 的占用记在 `/proc/<pid>/fdinfo/<fd>`。内核 7.0 里 i915 依赖 `ttm`，但 TTM 的页池、放置策略与回收由内核管理，用户态没有针对单个进程的直接控制接口；应用侧能做的是**降低峰值、避免无谓的重新分配**，并用同一份 fdinfo 账本验证效果。

## 调度优先级

### CPU 定时器与 nice / RT

- `PR_SET_TIMERSLACK` 只作用于调用线程。主循环在更新/渲染线程上，设置 1 ns 后，帧调度器最后一小段自旋之前的睡眠唤醒误差更小。
- nice 是 **每线程** 属性。`lower_nice` 遍历 `/proc/self/task` 给已有线程统一设值；之后创建的线程从创建者继承。
- 目标值把允许的最低 nice（`20 - RLIMIT_NICE`）限制在 `[-5, 0]` 内。本机 `RLIMIT_NICE = 0`（`/proc/self/limits` 的 `Max nice priority`），因此不执行。
- `RLIMIT_RTPRIO > 0` 时设 `SCHED_RR` 优先级 1。这是实时区间的最低档，避免在双核机器上压住合成器或音频线程。本机 `RLIMIT_RTPRIO = 0`，因此不执行。
- 授权方式（任选其一，均需重新登录或重新启动游戏）：
  - `/etc/security/limits.d/` 中给用户加 `nice -5` 与 `rtprio 1`（PAM 登录时生效）；
  - 通过 Gamemode 启动（本机用户不在 `gamemode` 组，实测 `gamemoderun` 后 NI 仍为 0，因此没有接入）；
  - 需要 GPU 优先级时，也可以给二进制约 `setcap cap_sys_nice=ep`：这会让游戏同时获得 RT / 负 nice 的权限，属于显式取舍。

### i915 DRM 上下文优先级

实现走标准 i915 上下文参数：

1. 从 `/proc/self/fd` 找到 `/dev/dri/renderD*`，按 `drm-client-id` 去重（Mesa 会 dup 同一个 render node）。
2. 在 render fd 上 `DRM_IOCTL_I915_GETPARAM` 查询 `I915_PARAM_HAS_SCHEDULER`；没有 `I915_SCHEDULER_CAP_PRIORITY` 就直接结束。
3. 有调度器能力时，扫描该 DRM file 的上下文 id，用 `I915_GEM_CONTEXT_GETPARAM` 读当前优先级，再 `SETPARAM` 把低于目标值的上下文提到目标值。
4. `EPERM` → 记录一次 warning（缺少 `CAP_SYS_NICE`，上下文保持不变）；`ENODEV` / `EINVAL` → debug 日志后结束；其它错误只 debug，不重试。

`PHIRA_DRM_PRIORITY` 可覆盖请求值：`0` 关闭，`1..=1023` 是 i915 的用户优先级范围，超过 1023 截断。默认值 1 是保守的“一档优先”，只有系统已经授予能力时才会真正改变调度。

本机实测（DRM GETPARAM / 上下文探针与游戏日志）：

| 项目 | 本机结果 |
| --- | --- |
| GPU | Haswell GT2，i915 revision 11 |
| `I915_PARAM_HAS_SCHEDULER` | `0x0`（无 `ENABLED` / `PRIORITY` / `PREEMPTION`） |
| `I915_GEM_CONTEXT_SETPARAM(PRIORITY)` | 所有上下文返回 `ENODEV` |
| 游戏日志 | `i915 reports no context-priority support; DRM priority is a no-op` |
| `CAP_SYS_NICE` | 当前用户没有，`sudo -n` 也无法非交互测试 |

因此在这台机器上 GPU 优先级是**纯 no-op**；代码路径存在、失败可观测，但本机无法演示成功路径。这是硬件/内核能力限制，不是配置遗漏。

## 显存与 TTM

### 遥测

`prpr/src/desktop_memory.rs` 在启动完成后读取一次 `/proc/self/fdinfo`，启动名为 `linux-mem-monitor` 的后台线程，每 2 秒采样一次。主线程只在绘制文字时读一个 `AtomicU64` 代次，不在 GL 线程做文件 I/O。

去重规则：同一个 `drm-client-id` 的多个 fd 只计一次，避免 Mesa 的 dup 放大 4 倍。`drm-total-*` 包含该 client 的共享对象；不同 client 之间共享的 buffer 会被各自计入，因此多 client 场景下总和是上界。当前游戏是一个 DRM client。

日志示例（本机最终构建）：

```text
INFO prpr::desktop_memory {gpu_mib=85.5 ... rss_mib=144.8 ... pressure=Normal}: linux memory baseline
# 之后每 60 秒输出一次，字段名相同：
DEBUG prpr::desktop_memory {gpu_mib=103.0 ...}: linux memory
```

采样线程可通过 `/proc/<pid>/task/*/comm` 看到，内核显示为 `linux-mem-monit`（15 字符截断）。

### 字体图集右尺寸化

桌面 UI 有三个长期存在的 `TextPainter`：主 UI、粗体、PGR 字体。改动前三者都在创建时分配 2048² RGBA8，单个 16 MiB，合计 48 MiB；日志中可直接看到三行 `creating cache texture: 2048x2048`。

Linux 现在从 1024² 开始，只有 glyph 缓存真的放不下时才增长；增长到 2048² 后，如果 glyph_brush 再建议 4096²，就清空缓存复用现有的 2048² 纹理，并记录一次 `font atlas reached 2048x2048; recycled glyph cache`。清空的是纯栅格化缓存，字形像素、排版、UV、过滤方式都不变，下一帧按需重建。极端情况下同一帧仍放不下，会跳过该帧文字并记录 warning，而不是继续分配 4096²。

非 Linux 平台完全保留原来的初始 2048² 与无上限增长，因为这次改动只针对 PC-Linux 的显存/内存压力。

### 压力保护

- 采样线程只在 **进入 Critical 的那一刻** 发出一次回收请求（30 秒冷却用于防止来回抖动）；不会每 2 秒反复裁剪、反复栅格化。
- 请求用代次而不是布尔标志，三个 painter 都能各自看到并各裁剪一次。
- 裁剪只把大于初始尺寸的图集缩回 1024²；字体、文本内容、布局与颜色不变。
- dGPU 的独立 VRAM 不在这个路径的判定范围内；本实现面向核显 / 共享内存。以后要覆盖 dGPU，应改用 `drm-total-vram*` 的容量压力而不是 `MemAvailable`。

### TTM

- 本机 `/sys/module/ttm/parameters/page_pool_size = 976437` 页（约 3.72 GiB）。它是 TTM 的**空闲页缓存**参数，不是 per-app 限额；只有 root 能改，而且改的是全系统行为。调低可能减少缓存页、但增加分配延迟；调高则相反。本次没有改它，因为应用侧没有可靠依据去决定一个对整机更好的值。
- 应用侧能控制的只有 TTM 的上游需求：减少纹理峰值、避免同一纹理反复销毁/重建、在压力时删除可重建对象。图集从 48 MiB 降到 12 MiB 就是直接减少 GEM/TTM 对象；删除纹理时先 `flush`，GEM 对象随后释放，TTM 页池的空闲页由内核 shrinker 在压力下回收。
- 内核 7.0 的 cgroup v2 暴露了 `dmem` 控制器（`dmem.current/capacity` 等），可以用于系统级显存保护，但它属于整机/用户 slice 配置，本机没有验证写权限，也没有接入；游戏进程内的遥测/裁剪不依赖它。
- 没有接入 Gamemode：本机用户不在 `gamemode` 组，实测无效，且它不能授予 i915 上下文优先级所需的 `CAP_SYS_NICE`。

## 实测

环境同上；数据来自游戏日志与 `/proc/<pid>/fdinfo`，按 client id 去重。

| 指标 | 改动前 | 改动后 |
| --- | --- | --- |
| 菜单稳定 `drm-total-system0` | 152608 KiB（≈149.0 MiB） | 103.0 MiB（≈105472 KiB） |
| `drm-shared-system0` | 7680 KiB | 7680 KiB |
| 三个字体图集 | 3 × 2048² RGBA8 = 48 MiB | 3 × 1024² RGBA8 = 12 MiB |
| 菜单 RSS | ≈262 MB（粗测） | 252.6 MiB |
| 帧间隔（菜单，1024 帧） | p50 16.6 ms；p95 17.7–18.3 ms | p50 16.67 ms；p95 18.19 ms；p99 19.19 ms |

- 显存稳定值下降 **约 46 MiB**。可逐项解释的是三个图集的 48 → 12 MiB（36 MiB）；其余约 10 MiB 差异来自两次采样的场景/驱动缓存状态，未逐项归因。
- 本地日志确认三次 `creating cache texture: 1024x1024`，菜单运行期间没有出现 `font atlas reached 2048x2048`，说明 1024² 足够当前菜单。
- 帧间隔与改动前的基线处于同一区间；新增的后台采样线程不参与渲染，帧调度路径没有改动。
- 启动基线（install 时）为 85.5 MiB，约 3 秒后到 102 MiB，随后稳定在 103 MiB；25 秒内没有持续增长或分配抖动。

## 验证入口

```bash
# 单元测试（解析、压力分级、优先级环境变量）
cargo test -p prpr --lib desktop_

# 构建并运行
cargo build --release -p phira-main
./target/release/phira-main

# 游戏日志里直接看按 client 去重后的显存基线/周期采样
grep -E "linux memory|creating cache texture|font atlas|i915|timer slack" \
  data/logs/phira.$(date +%F).log

# 关闭 DRM 优先级尝试
PHIRA_DRM_PRIORITY=0 ./target/release/phira-main

# 可选：观察帧间隔
PHIRA_FRAME_PROFILE=/tmp/frame.csv ./target/release/phira-main
```

`PHIRA_DRM_PRIORITY`、`PHIRA_FRAME_PROFILE` 都是本地诊断开关，不影响默认行为。`cargo test` 里的优先级测试只解析环境变量，不实际请求内核权限。

## 残余风险

- **成功路径未在本机验证**：Haswell 没有 `I915_SCHEDULER_CAP_PRIORITY`，且当前用户没有 `CAP_SYS_NICE`；GPU 优先级、RT、负 nice 三条成功路径都只能静态审查。ioctl 编码、caps 位和 `ENODEV`/`EPERM` 分支已用 C 探针与实际日志核对。
- **图集上限**：1024² 在大量 CJK/多语言字形或超大字号下仍可能增长到 2048²；到了 2048² 后极端单帧可能触发一次清空回收，仍放不下时该帧文字会被跳过。日志会记录，正常菜单/游玩未触发。
- **压力阈值**：`MemAvailable < 256 MiB` 是保守阈值，适合核显共享内存；没有在本机人为制造内存压力去触发裁剪（会危及桌面会话）。分级逻辑有单元测试，裁剪的 GL 路径与既有增长路径共用 `resize_texture` + 纹理替换模式。
- **多 DRM client**：显存总和按 client 去重，但不同 client 共享的 buffer 会被重复计入；当前游戏只有一个 client。
- **权限耦合**：授予 `CAP_SYS_NICE` 后，代码会同时尝试 RT 与负 nice；如果只想启用 GPU 优先级，需要后续把这三类请求拆成独立开关。
- **cgroup dmem / TTM 页池**：系统级参数未修改；不同内核/驱动版本的 fdinfo 键可能不同，遥测会退回 `drm-memory-*`，两者都没有时只记录 0。
