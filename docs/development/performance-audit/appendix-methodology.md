# 附录：测量方法、证据等级与复现

- 文档版本：1.1
- 基线日期：2026-11-11
- 配套文档：[README.md](README.md)、[positive-optimizations.md](positive-optimizations.md)、[non-positive-optimizations.md](non-positive-optimizations.md)、[library-level-optimizations.md](library-level-optimizations.md)、[appendix-mainline-integrated.md](appendix-mainline-integrated.md)
- 目的：让后续工程师在没有本地参考实现与历史研究材料的情况下，仍能理解数字是怎么来的、能外推到什么范围、如何复测、如何判定通过或失败

## 1. 适用范围

本附录描述 Phira-Pro 渲染与加载优化研究中使用过的测量协议。它不规定唯一的工具实现，而规定必须满足的口径：

- 同一 SO：对照组和处理组必须使用同一个库二进制，不能一边用旧 SO、一边用新 SO。
- 同一输入：同一谱面、同一配置、同一画质、同一采样窗口。
- 同一设备：同一台真机、同一 Android 版本、同一屏幕分辨率与刷新率。
- 先质量、后性能：质量门不过，性能数字不进入正优化结论。
- 先方向、后幅度：先确认处理组是否稳定优于对照组，再讨论百分比。

不满足这些条件的数字，只能作为线索，不能作为验收证据。

### 1.1 状态口径

主线状态只有两个标记：

| 标记 | 含义 |
|---|---|
| `☐ 未实施（main）` | 尚未合入 `main`，或已合入但未完成验收。 |
| `☑ 已实施（main）` | 已合入 `main` 并完成验收。 |

参考实现另记：本地参考实现或历史研究材料中的代码、构建、默认开关、真机数据都只说明本地参考实现状态，不改变主线状态。能编译、默认启用、真机跑过、有测量数据，都不等于 `☑ 已实施（main）`。

**本地参考实现不算已实施。** 主线状态必须由合入提交、关键文件在 `main` 中存在、普通路径实际使用和验收证据共同支持。

历史引文中的“已实施”指本地参考实现落地，不代表主线实施。引用历史记录时必须按当前 `main` 重新判定，并写成上述两个标记之一。

## 2. 测量对象与时间口径

### 2.1 frame interval

`frame interval` 是相邻两次应用帧开始之间的时间，单位毫秒。它回答“用户两次看到新帧之间隔了多久”，但包含上一帧的渲染、提交、交换、驱动等待和调度。

```text
t0: 第 N-1 帧 app begin
t1: 第 N-1 帧 update 结束
t2: 第 N-1 帧 render 结束
t3: 第 N-1 帧 flush 结束
t4: 第 N 帧 app begin

frame interval = t4 - t0
```

注意：

- 第 N 帧的 interval 不是第 N 帧自己的 `app wall`，而是“上一帧工作 + 间隙”。
- 不能把 interval 直接拆成当前帧的 update/render/flush。
- 不能把 interval 的倒数当成当前帧的渲染能力。

### 2.2 app wall

`app wall` 是一帧应用侧墙钟：

```text
app wall = update + render + flush
```

其中：

- `update`：游戏逻辑、谱面求值、判定、动画、输入处理。
- `render`：CPU 侧录制、批次构造、GL 调用提交。
- `flush`：提交结束、必要的同步/交换前工作。

`app wall` 包含 CPU 执行、驱动排队、驱动等待、调度和部分系统开销。它不是纯 GPU 时间，也不是纯 CPU 时间。它可以回答“这一帧应用侧花了多久”，不能回答“GPU 执行了多久”。

### 2.3 chart stage

`chart stage` 是 `render` 内部的一个阶段，通常包含判定线、音符、批次构造和谱面相关的 GL 提交。它不包括背景、粒子、HUD、block overlay、最终交换。

`chart stage` 改善不等于整帧改善。一个常见的误读是“chart 从 3.0ms 降到 1.5ms，所以游戏快了一倍”；正确表述是“chart 阶段墙钟减少约 1.5ms，整帧是否改善要看 app wall、interval 和 FPS”。

### 2.4 flush 完成不等于纯 GPU 时间

`flush` 返回时，GPU 可能仍在执行；`glFinish` 或 fence 等待完成时，也只能说明“到某个提交点为止的工作完成了”，不能说明“GPU 只花了这么多时间”。

原因包括：

- 驱动可能把多个提交合并、重排或延迟。
- 驱动等待可能混入 fence、队列、内存分配和着色器编译。
- 移动 GPU 的 tile 内执行与外存写出不是同一件事。
- 交换链、合成器和显示控制器可能引入额外等待。

因此：

- `completed p50` 是同步完成墙钟，不是纯 GPU 执行时间。
- `render p50` 是 CPU 提交墙钟，不是 GPU 吞吐。
- `swap p50` 包含呈现等待，不是纯 GPU 执行。
- 没有可信 GPU timer 时，不写“GPU 时间”。

### 2.5 SurfaceFlinger 呈现

系统呈现数据来自 SurfaceFlinger 的实际 present 时间戳。它回答“屏幕实际多久收到一帧”，包含应用、合成器和显示控制。

使用规则：

- 记录采样窗口、呈现次数、持续时长、p50/p95/p99/max。
- 记录 `>16.67ms`、`>20ms`、`>30ms` 的计数；需要时另记 `>1.5 refresh` 的漏刷新计数。
- 检查 ring overlap，确认采样窗口内 job 一直在运行。
- 呈现窗口通常只有 9 至 10 秒，不能替代整段 25 秒/30 秒应用窗口。
- 呈现 FPS 接近 60 不代表应用渲染有余量；它可能只是 vsync 限制。

### 2.6 指标表

| 指标 | 单位 | 回答的问题 | 不回答的问题 |
| --- | --- | --- | --- |
| frame interval | ms | 两次应用帧开始之间的间隔 | 当前帧自己的 CPU/GPU 分解 |
| app wall | ms | 一帧 update+render+flush 的墙钟 | 纯 GPU 时间 |
| update | ms | 逻辑与求值墙钟 | 判定正确性 |
| render | ms | CPU 录制与提交墙钟 | GPU 执行时间 |
| flush | ms | 提交结束/同步墙钟 | 纯 GPU 完成时间 |
| chart stage | ms | 谱面相关渲染阶段 | 整帧 FPS |
| HUD stage | ms | HUD 相关阶段 | 整帧 FPS |
| glyph upload | ms | 字形上传阶段 | 字形数量或 GPU 时间 |
| upload bytes | B | 传输量 | 帧时间 |
| draw call | 次 | API 提交次数 | GPU job 数 |
| completed p50 | ms | 同步微基准完成时间 | 游戏 FPS |
| presented FPS | fps | 系统实际呈现节奏 | 应用渲染能力 |
| pixel diff | 通道数 | 采样帧质量 | 全帧/全场景质量 |

## 3. 同 SO ABBA 协议

### 3.1 为什么用 ABBA

同一台设备连续运行会受温度、频率、缓存、内存水位和后台任务影响。简单的 A/B 两次运行无法区分“处理组更好”和“后一次更凉”。

ABBA 的顺序是：

```text
A1: 对照组
B1: 处理组
B2: 处理组
A2: 对照组
```

这样对照组出现在首尾，处理组在中间。如果处理组的改善小于对照组的漂移，就不能判正优化。

### 3.2 运行序列

| 轮次 | 变体 | 作用 |
| --- | --- | --- |
| A1 | 关闭优化 | 对照组基线 |
| B1 | 开启优化 | 处理组第一次 |
| B2 | 开启优化 | 处理组第二次 |
| A2 | 关闭优化 | 对照组复测，检查漂移 |

要求：

- 四轮使用同一 SO、同一谱面、同一配置、同一画质、同一采样窗口。
- 只允许目标开关和运行 ID 不同；其他字段必须一致。
- 每轮重新启动应用或重新进入场景，避免跨轮状态污染。
- 记录每轮的 SO SHA、源码提交、配置快照和设备状态。
- 如果任一轮失败，整组不进入汇总；不能只取成功的三轮。

### 3.3 预热与采样

基准协议：

| 项目 | 值 | 说明 |
| --- | --- | --- |
| 预热 | 120 帧 | 让着色器、纹理、缓存、字体和驱动进入稳定状态 |
| 采样 | 1200 帧 | 默认性能窗口 |
| 起始时间 | chart 60 秒 | 避开加载、开场和第一段特效 |
| 时钟 | realtime | 正常音轨与动画 |
| 质量对照 | fixed clock | 像素对照使用固定时钟，避免噪域相位漂移 |

长窗口扩展：

- P-02、P-03、P-04、N-03、N-04 等 cohort 使用 1800 帧采样，仍为 120 帧预热。
- 长窗口用于观察 p99、max 和长帧计数，不能与 1200 帧 cohort 混池。
- 每个条目必须写实际样本数；不能只写“约 30 秒”。

### 3.4 漂移判定

设：

- 对照组两次均值为 `A_mean`，两次差为 `A_drift = |A2 - A1|`。
- 处理组两次均值为 `B_mean`，组差为 `B_delta = |B_mean - A_mean|`。

判定规则：

- 如果 `A_drift >= B_delta`，不能判正优化；方向可能来自漂移。
- 如果处理组两次运行都落在对照组两次运行之间，不能判正优化。
- 如果处理组方向在低画质和普通画质相反，不能用一个画质的结论覆盖另一个。
- 如果只有一轮处理组数据，不能判高置信度。

负优化结论也要检查漂移：如果处理组变差，但对照组同方向漂移更大，不能把变差归因给优化。

### 3.5 禁止混池

以下情况不能合并统计：

- 不同 SO、不同源码提交、不同库 rev。
- 不同谱面、不同难度、不同 block/effect 密度。
- 不同采样帧数（1200 与 1800）。
- 不同画质（low sample1 与 normal sample2）。
- 不同窗口（loading、playing、ending）。
- 失败后被排除的 cohort 与成功 cohort。
- 观察器 cohort 与收益 ABBA cohort。

混池会产生看似精确、实际不可复现的数字。文档中每个 cohort 必须独立列出。

## 4. 统计量

### 4.1 分位数

每个窗口至少报告：

| 统计量 | 用途 | 注意 |
| --- | --- | --- |
| p50 | 典型帧 | 不能代表持续 FPS |
| p95 | 较好覆盖尾部 | 仍可能掩盖极少数长帧 |
| p99 | 尾部 | 对卡顿敏感，但单次尖峰影响大 |
| max | 最坏帧 | 必须保留，不能只报均值 |
| mean | 总量/预算 | 会被长帧拉高 |

报告要求：

- 写清楚是“每轮分位数的中位数”还是“合并样本的分位数”。
- 不能把 per-run 分位数的中位数当成 pooled 分位数。
- 不能只报 p50 或只报 FPS。
- 不能删除慢帧后再算分位数。

### 4.2 超预算计数

至少报告：

| 阈值 | 含义 | 用途 |
| --- | --- | --- |
| `>16.67ms` | 超过 60Hz 一帧预算 | 观察掉帧 |
| `>20ms` | 明显超过预算 | 观察可感知卡顿 |
| `>30ms` | 长帧 | 观察尖峰和尾部 |
| `>1.5 refresh` | 漏掉至少一个刷新周期 | 系统呈现侧的漏刷新 |
| `>100ms` | 严重卡顿 | 加载/切换/结算 |

计数必须带分母。例如“1200 帧中 3 帧 >30ms”，不能只写“3 帧”。

### 4.3 持续 FPS

持续 FPS 的定义：

```text
sustained FPS = 非 I/O 采样帧数 / 这些帧的累计 interval 秒数
```

要求：

- 使用完整 interval，不删除启动、切换、长帧。
- 第一个跨场景 interval 可以从该场景窗口的分母中排除，但必须写明。
- 不能用 `1000 / p50` 代替持续 FPS。
- 不能把 app FPS 与系统呈现 FPS 混为一个数。
- 不能把 60Hz 上限内的 59.7 与 60.0 当成“快 0.5%”的收益；要看 p99、长帧和 app wall。

### 4.4 分组均值

四轮 ABBA 的推荐汇总：

- 先算每轮均值/分位数。
- 再算组内两轮均值。
- 报告 `A_mean`、`B_mean`、`B-A`、百分比、`A_drift`、`B_drift`。
- 如果样本数不等，不能直接 pool；要么按 run mean 等权，要么明确说明加权方式。
- 不能只挑最快一轮。

### 4.5 样本数

每个结论必须能回答：

- 预热多少帧。
- 采样多少帧。
- 有效 interval 多少。
- 质量对照多少组、多少像素/通道。
- 系统呈现窗口多少秒、多少次呈现。
- 失败/排除多少轮。

没有这些数字的结论，置信度不能给高。

## 5. 像素与质量对照

### 5.1 固定时钟

动画、噪域、block 效果和粒子可能依赖真实时间。性能 cohort 使用 realtime 以保留真实节奏；像素对照使用 fixed clock，确保同一 chart time 的画面可比。

要求：

- 固定时钟只用于质量对照，不用于性能结论。
- 记录 fixed clock 的步长和起始时间。
- 如果效果读取真实运行时间，必须同步该时钟或明确排除。
- 不能把 fixed clock 的帧时间当成游戏帧时间。

### 5.2 逐 RGBA 通道 diff

质量对照的最小流程：

1. 解码两张 PNG 为 RGBA8。
2. 确认宽高一致。
3. 逐像素、逐通道计算绝对差。
4. 统计：
   - `diff_pixels`：至少一个通道不同的像素数。
   - `diff_channels`：所有通道差绝对值大于 0 的通道数。
   - `max_diff`：最大通道差。
   - `alpha0_rgb_diff`：alpha 为 0 的像素中，RGB 是否不同。
5. 严格相等要求 `diff_channels = 0`。
6. 如果不为 0，报告具体像素区域、通道数、最大差和 alpha 情况。

注意：

- alpha 为 0 的 RGB 也必须比较；否则会漏掉预乘/清零差异。
- 不能用全屏 MAE 代替严格比较；背景会稀释差异。
- 不能只比较缩略图或 JPEG。
- 不能只比较一帧就宣称全场景通过。
- 如果使用容差，必须写明容差、理由和覆盖范围；本审计的正优化条目默认不容差。

### 5.3 组数与样本数

| 条目 | 组数 | 样本/输出 | 结果 |
| --- | ---: | ---: | --- |
| P-02 尾工作集 | 6 组 RGBA 对 | 12 个质量 job | 全部相同 |
| P-03 字体池 | 10 组 RGBA 对 | 14 个质量 job | 全部相同 |
| P-04 透明线 DCE | 4 组像素对 | 低/普通各两组 | 全部相同 |
| P-05 block tiles | 9 组末帧 RGBA | 12 项 ABBA | 全部相同 |
| P-07 Chart JSON 分片 | 6 组输出对照 | 96 项输出 | 全部相同 |
| N-10 snapshot 回退 | 2 项 full-copy 诊断 | 254,520 个通道不同 | 失败 |
| N-15 固定纹理 | Case1 帧对照 | 60 pixels/80 channels | 失败 |

“组数”是独立场景/窗口对；“样本数”是像素、通道或输出项。两者不能混写。

### 5.4 失败判据

出现以下任一情况，质量门失败：

- `diff_channels > 0` 且没有事先声明的容差。
- alpha 为 0 的 RGB 不同。
- 只有部分 Case 执行，未执行项被省略。
- 质量对照的时钟、分辨率、MSAA、纹理格式与性能 cohort 不一致。
- 对照图来自不同谱面、不同资源包或不同场景。
- 质量通过但真实消费者没有走到优化路径。

失败后：

- 不能把性能数字升级为正优化。
- 不能把“GL 无错误”当成“像素通过”。
- 不能把“源纹理相同”当成“替代采样流程相同”。

## 6. 真机环境

### 6.1 主设备

| 项目 | 值 |
| --- | --- |
| SoC | MTK MT6769V/CU |
| ADB board | mt6768 |
| GPU | Mali-G52 MC2 |
| Android | 10 |
| 分辨率 | 1920×1200，横屏 |
| 刷新率 | 60Hz |
| 典型采样温度 | 约 28 至 32°C |

主设备用于 2026-10-05 至 10-06 的早期渲染、block、snapshot、Chart JSON 分片研究，包括 P-05、P-07、N-01、N-05 至 N-10、N-17 等条目。它也是早期 1920×1200 应用 observer 的设备。

### 6.2 第二设备

| 项目 | 值 |
| --- | --- |
| GPU | Mali-T880 |
| 分辨率 | 2560×1440 |
| Android | 8.0.0 |
| 用途 | low/normal 画质的字体池、粒子、line DCE 等研究 cohort |

第二设备是 2026-10-07 之后 low/normal 研究的主要设备，包括 P-01 至 P-04、P-06 以及 N-02 至 N-04、N-12 至 N-16 的多数 cohort。N-11 的常驻音符研究跨早期与后期，具体设备以条目内记录为准。第二设备不代表所有 Mali-T880 设备；不同厂商的驱动、热设计、内存带宽和系统调度都会改变结果。

### 6.3 实际 sample count

- 配置可能请求 `sampleCount=2`，但实际 RBO 可能是 4 sample。
- 所有性能和质量结论必须写“请求值/实际值”。
- 不能把请求值当成实际值，也不能为了达标把实际 4 sample 改成 2 sample。
- 质量对照必须与性能 cohort 的实际 sample count 一致。

### 6.4 系统呈现采样

- SurfaceFlinger 窗口通常 9 至 10 秒，覆盖 1200/1800 帧应用窗口的一部分。
- 记录 `polls`、`captured_presentations`、`duration_s`、`ring_overlap_verified`。
- 检查同一 job 在所有 poll 中持续运行；如果 job 结束或切换，窗口不能作为整段证据。
- 呈现 FPS 与 app FPS 分开报告；两者接近 60 时，不能互相替代。

## 7. 证据等级

| 等级 | 定义 | 最低要求 | 能支持 |
| --- | --- | --- | --- |
| A | 同 SO 多轮 ABBA + 像素/质量对照 + 真实消费者 | 至少 4 轮；低/普通至少一个方向；检查漂移；质量门通过 | 高置信度的整帧/局部结论 |
| B | 单窗口/单谱面/局部阶段；或只有一种画质 | 同一 SO 的 ABBA 或成对对照；有明确指标 | 中置信度的方向性结论 |
| C | 只有微基准、单元测试、`glFinish` 诊断或单次探针 | 有可复现输入和输出；但没有真实消费者或整帧对照 | 低置信度的机制线索 |
| D | 无数据/仅原型 | 只有设计或代码，没有测量 | 不能支持收益结论 |

纯质量失败项可以用同一 SO 的确定性像素对照替代 ABBA 轮次；N-10 属于此类。它不是性能 A 级证据，但足以支持“该回退路径不可启用”的高置信度结论。

补充：

- A 级证据不要求“所有设备”，但要求在文档写明的设备/谱面/窗口内稳定。
- B 级证据可以升级为 A，前提是增加独立 cohort、质量对照和真实消费者。
- C 级证据不能用于“整帧正优化”，只能用于“值得进一步验证”。
- 负优化结论同样需要等级；没有整帧数据的负结论要注明“工程判定”。

## 8. 如何复现

### 8.1 构建 feature

不同条目需要不同的编译 feature。常用组合：

| 目标 | feature | 说明 |
| --- | --- | --- |
| 研究载体 | `render_research` | 打开研究 job、计时和注入入口 |
| P-01/P-02 | `render_research`、`chart_pre_compile` | 谱面预编译、workset；判定线 GPU IR 走研究请求 |
| CPU 语义参考（可选） | `render_chart_ir` | 仅作对照；源码注释标明其比原路径慢，不作为生产收益 |
| P-03/P-06/N-02/N-14 | `font_atlas_pool`、`render_atlas_pool` | 通用字体池、图集 delta、双通道、区域合并 |
| P-04 | 默认构建；研究审计加 `render_research` | 透明线 DCE 默认开启 |
| P-05 | `render_research`、`render_bounds` | block bbox/tiles |
| P-07 | `render_research` | Chart JSON 分片 |
| N-07 | `render_research`、`render_glyph_uploads` | glyph scoped flush |
| N-08/N-09/N-10 | `render_research`、`render_lifetime` | mask 查表/bbox、mask ring、snapshot 回退 |
| N-12/N-13 | `render_research` | 可见性/取值计划 |
| N-16 | `render_research` | PBO 字库上传 |

命令模板：

```bash
cargo check -p phira --features "render_research,chart_pre_compile,font_atlas_pool"
cargo build --release -p phira-main --features "render_research,render_bounds,render_lifetime,render_glyph_uploads"
```

要求：

- 构建前确认 `Cargo.toml` 的 macroquad/miniquad rev 与目标实现一致。
- 不要只改主项目源码而沿用不配套的库 rev。
- 记录 SO SHA、源码提交、feature 列表和 `Cargo.lock`。
- 研究 feature 不能进入普通发布构建。

### 8.2 注入 job

研究载体通常读取一个 JSON job。字段名可能因本地参考实现版本略有差异，但语义如下：

| 字段 | 类型 | 作用 |
| --- | --- | --- |
| `scene` | string | `gameplay`、`loading`、`application` |
| `chart` | string | 谱面 ZIP 路径或哈希 |
| `clock` | string | `realtime` 或 `fixed` |
| `sample_frames` | number | 采样帧数，常用 1200 或 1800 |
| `warmup_frames` | number | 预热帧数，常用 120 |
| `start_time` | number | 起始 chart time，常用 60 |
| `finish_gpu` | bool | 是否在末尾 `glFinish`；性能 cohort 通常 false |
| `config` | object | 画质、粒子、效果、MSAA 等 |
| `chart_pre_compile` | bool | 谱面预编译/workset |
| `tail_workset` | bool | 尾工作集 |
| `font_production_pool` | bool | 通用字体池 |
| `line_elision` | bool | 透明线 DCE |
| `block_bounds` | bool | block bbox 裁剪 |
| `block_tiles` | bool | block 分区裁剪 |
| `gpu_render_ir` | bool | GPU 常驻/渲染 IR |
| `visibility_plan` | bool | 可见性预编译 |
| `render_read_plan` | bool | 渲染取值计划 |
| `font_pool_channels` | number | 图集双通道模式 |
| `font_pool_policy` | number | 图集策略，3 为区域合并 |
| `particle_stream_discard` | bool | 粒子 stream discard |
| `particle_small_stream` | bool | 粒子小缓冲 |
| `font_upload_pbo` | bool | PBO 字库上传 |
| `fused_background` | bool | 背景融合 |
| `chart_loading_slices` | bool | Chart JSON 分片 |
| `async_chart_json` | bool | 后台 JSON 解析 |

示例 job：

```json
{
  "scene": "gameplay",
  "chart": "charts/f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip",
  "clock": "fixed",
  "sample_frames": 1200,
  "warmup_frames": 120,
  "start_time": 60,
  "finish_gpu": false,
  "config": {
    "sampleCount": 1,
    "particle": true,
    "disableEffect": false,
    "aggressive": false,
    "blockAreaSimple": false,
    "shaderPreRender": false
  },
  "chart_pre_compile": true,
  "tail_workset": true,
  "font_production_pool": true,
  "line_elision": true
}
```

要求：

- 目标实现不认识的字段必须被拒绝，不能静默忽略。
- 性能 cohort 的 `finish_gpu` 保持 false；`glFinish` 会改变调度，不能用于持续 FPS。
- 质量 cohort 使用 fixed clock；性能 cohort 使用 realtime。
- 同一组 ABBA 只改目标字段和运行 ID。

### 8.3 采样窗口

- 预热 120 帧。
- 采样 1200 帧（基准）或 1800 帧（长窗口）。
- 起始 chart time 60 秒，避开加载和开场。部分固定窗口使用 21-27、85-87、105-111 秒等区段；这是场景选择，不是采样协议变化，必须记录。
- 保留所有 interval，不删除慢帧、不裁掉启动/切换。
- 记录实际 chart time 范围；不同 FPS 会导致实际时长不同。
- 质量对照单独跑 fixed clock，不插入性能采样。

### 8.4 判定标准

一次 ABBA 通过的最低条件：

- 四轮完整，没有失败轮。
- 同一 SO、同一谱面、同一配置。
- 处理组均值优于对照组，且方向在两组处理轮中一致。
- 对照组漂移小于组差。
- p99 或长帧计数没有恶化；如果恶化，不能判整帧正优化。
- 质量对照通过；没有未执行的关键 Case。
- 真实消费者走到优化路径；不是只在微基准里生效。

整帧正优化的额外条件：

- FPS、frame interval p99、`>30ms` 计数中至少一项稳定改善。
- 改善不能只来自 p50，而 p99/max 明显变差。
- 普通画质和低画质至少一个方向一致；如果相反，必须分开写。

局部正优化的额外条件：

- 阶段指标明确改善。
- 整帧 FPS 基本不变或在噪声内。
- 不把阶段改善写成 FPS 改善。

### 8.5 命令模板

```bash
cargo check -p phira --features "render_research,chart_pre_compile,font_atlas_pool"
cargo build --release -p phira-main --features "render_research,render_bounds,render_lifetime,render_glyph_uploads"
adb shell dumpsys display
adb shell getprop ro.board.platform
adb shell monkey -p org.flos.phirapro.research 1
```

说明：

- `dumpsys display` 用于确认实际分辨率和刷新率，不用于性能统计。
- `getprop ro.board.platform` 用于记录设备，不用于能力判定。
- `monkey` 只是启动应用的示例；具体研究载体可能用不同入口。
- 采样、导出和统计可以由自写脚本完成；字段和判定标准是充分信息。

### 8.6 不依赖脚本路径

复现不要求使用仓库里的某一个脚本。只要满足以下条件，任何实现都可以：

- 能构建同一 SO。
- 能注入目标开关和同一 job。
- 能记录完整 interval、app wall、阶段 span。
- 能导出质量对照图。
- 能按 ABBA 顺序运行四轮。
- 能输出每轮指标和漂移。

文档中的档案名只用于溯源，不作为唯一复现方式。

## 9. 术语表

| 术语 | 定义 |
| --- | --- |
| frame interval | 相邻两次应用帧开始之间的时间，包含上一帧工作和间隙 |
| app wall | 一帧 `update + render + flush` 的应用侧墙钟 |
| chart stage | `render` 内与谱面相关的阶段，不含背景/粒子/HUD/block |
| stage span | 嵌套在 update/render 内的阶段计时；不能与父级相加 |
| managed native draw | 由 macroquad/miniquad 管理的原生 draw 提交；不等于 GPU job |
| resident command | 预先录制、可在后续帧重放的命令；不重新执行 CPU 录制 |
| workset | 加载期预计算的音符/区段集合，用于安全跳过或重放 |
| tail | 判定线尾段或普通音符尾段；本审计中多指尾工作集 |
| tail workset | 针对谱面尾段的安全裁剪/重放工作集 |
| line DCE | 透明判定线的死代码消除；alpha 为 0 且默认材质时跳过绘制 |
| IR | 中间表示；把渲染计划编译成可复用记录 |
| CSE | 公共子表达式消除；复用已计算的矩阵/取值 |
| visibility certificate | 证明某个音符在视野外的保守证书 |
| atlas pool | 多个完整字体图集版本的池；选择已完成版本 |
| atlas delta | 只上传变化的矩形/版本，不上传整张图集 |
| atlas crop | 把图集物理裁到已使用区域，并重映射 UV |
| atlas coalesce | 合并多个上传矩形，减少调用数 |
| stage/phase | 渲染管线中的一个阶段，如 background、chart、HUD |
| pass | 一次 render pass；同目标连续绘制可共享 pass |
| FBO | framebuffer object；离屏渲染目标 |
| resolve | 多重采样目标解析为单采样纹理 |
| blit | 帧缓冲拷贝；可能涉及格式转换和过滤 |
| snapshot | 把当前场景颜色复制到纹理，供后续效果使用 |
| snapshot fallback | blit/resolve 能力不可用时的替代路径；必须保持像素 |
| MSAA | 多重采样抗锯齿 |
| actual sample count | 实际创建的 sample 数，不是配置请求值 |
| sampleCount | 配置请求的 sample 数 |
| PBO | pixel buffer object；异步纹理上传缓冲 |
| immutable storage | 固定尺寸/格式的纹理存储；创建后不可重新指定 |
| uniform cache | 缓存 uniform 值，跳过重复上传 |
| glyph scoped flush | 只刷新字形上传作用域的依赖，不全局 flush |
| texture ring | 多份纹理按帧轮换，避免读写冲突 |
| viewport cache | 按 viewport 缓存 mask/中间结果 |
| keep_resolved_pass | 保留最终 resolve 后的单采样输出，减少重复 resolve |
| fused background | 把背景与遮暗/低频处理合并，减少 draw |
| DCE | dead code elimination；删除不产生可见结果的绘制 |
| ABBA | A1/B1/B2/A2 的交叉运行顺序 |
| same SO | 对照组和处理组使用同一个库二进制 |
| warmup | 采样前的预热帧 |
| p50/p95/p99 | 第 50/95/99 百分位 |
| max | 最大值 |
| over-budget | 超过 16.67ms/20ms/30ms 等阈值的帧 |
| drift | 对照组首尾两次的差异 |
| hard gate | 必须通过的门槛，不通过则整组失败 |
| feature flag | 编译期开关 |
| runtime flag | 运行期开关 |
| mainline | 主线 `main` |
| low quality | 低画质配置，通常 sampleCount 1 |
| normal quality | 普通画质配置，通常 sampleCount 2，实际可能 4 |
| fixed clock | 固定谱面时钟，用于可重复像素对照 |
| realtime | 正常音轨与动画时钟 |
| strict RGBA | 逐通道完全相等，包括 alpha 为 0 的 RGB |
| quality pair | 两组最终帧的像素对照 |
| SF presentation | SurfaceFlinger 实际呈现时间戳 |
| ring overlap | 呈现采样窗口内 job 持续运行的证据 |
| resident note IR | 常驻 GPU 的音符几何/命令中间表示 |
| GPU timer query | GPU 时间查询；不可信时不能用于结论 |
| disjoint | GPU timer 结果无效的标志 |
| managed default material | 库默认材质；透明线 DCE 的安全条件之一 |

## 10. 已知限制

### 10.1 单设备、单谱面、单窗口

- 大多数 A/B 只在一台 Mali-G52 MC2 设备、一个谱面、一个 25 秒/30 秒窗口上完成。
- 不能外推到所有 Mali、Adreno、PowerVR、Apple GPU。
- 不能外推到所有谱面、所有 block 密度、所有 effect 组合。
- 不能外推到完整歌曲、完整 UI、暂停/恢复/seek、结算动画。
- 系统呈现窗口通常只有 9 至 10 秒，不能替代整段应用窗口。

### 10.2 驱动等待混入

- `app wall`、`render`、`swap` 都包含驱动等待和调度。
- `glFinish` 会串行化 CPU/GPU，改变真实并发。
- GPU timer query 在真实 Petrichor 整帧出现 420 次 disjoint，360 个样本全部无效；`glReadPixels` 报 `0x505`（GL_OUT_OF_MEMORY）。
- 因此没有可信的纯 GPU 分解；不要写“GPU 执行时间”。
- tile 内执行、外存写出、合成器等待不是同一件事。

### 10.3 文档与代码默认值冲突

| 项目 | 文档/注释可能这样写 | 代码事实 | 处理 |
| --- | --- | --- | --- |
| `chart_pre_compile` | “实验性，默认关闭” | low-quality 本地参考实现的 `phira` 默认 feature 包含 `chart_pre_compile`，但 `Config` 运行值默认 false | 写“编译默认包含，运行默认关闭” |
| `font_atlas_pool` | “候选，默认关闭” | low-quality 本地参考实现的 `phira`/`prpr` 默认 feature 包含 `font_atlas_pool`；子项 `font_pool_channels`、`font_pool_deferred`、`font_pool_policy=3` 默认关闭 | 主池写“默认启用”，子项写“默认关闭” |
| `async_chart_json` | “研究默认 false” | android 本地参考实现的普通构建 `chart_json_enabled()` 返回 true；研究请求默认 false，需要显式开启 | 写“普通默认开，研究请求默认关” |
| `line_elision` | “研究开关” | low-quality 本地参考实现源码 `Options::default().enabled = true`；研究 run 可显式选 control | 写“普通默认开，研究可对照关闭” |
| `block_tiles` | “研究候选” | 研究请求默认 false | 写 `☐ 未实施（main）`（本地参考实现默认关闭） |

P-06 的 32×32 tile/4 版本增量候选属于 `font_atlas_pool` 的研究子路径，默认关闭；不要因为它依赖同一个 feature 就写成默认启用。P-03 的通用池默认开启与 P-06 的增量候选默认关闭是两个不同状态。

以上默认值只描述本地参考实现的构建/运行默认，不改变主线状态；主线条目仍按 §1.1 标记，默认启用不等于 `☑ 已实施（main）`。

遇到冲突时：

- 以代码的普通构建/普通配置实际行为为准。
- 在条目里同时写“编译包含”和“运行默认值”，不要只写一个。
- 如果文档与代码冲突，先修文档，不要用文档去覆盖代码。

### 10.4 本地参考实现落后 main

- 三个本地参考实现工作区分别落后 `main` 131、136、133 个提交。
- 这意味着：
  - 接口、配置字段、场景栈、资源路径可能已变化。
  - 本地参考实现的 SO 不能直接拿到当前 `main` 上运行。
  - 本地参考实现的改动需要重新编译、重新验收。
  - 旧证据不能直接当当前 `main` 的证据。
  - 本地参考实现已落地不等于 `☑ 已实施（main）`。

### 10.5 库 rev 不一致

| 使用方 | macroquad | miniquad |
| --- | --- | --- |
| `main` | `8d617ff60dd0c6c1d237e5c26ecde16e2ed0c72e` | `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |
| low-quality 本地参考实现 | `de34611bca60abda9c4a2d8ebea568970fd7a850` | `19e97bd3d9b0ecce86ae50558311175a92d933a3` |
| android 本地参考实现 | `ce7949ed2df6023e0269cc6e43f5eeb8392c2d98` | `000ea930a35feed73c1be398a73f8e08c5a4dd98` |
| 仅含测量载体的本地归档 | `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1` | `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |
| 上游 master | `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1` | `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |

后果：

- 同一份主项目源码在不同 rev 下可能编译失败或行为不同。
- resident command、固定纹理存储、uniform cache、glyph upload 依赖的库 API 不是所有 rev 都有。
- 不能只替换 `Cargo.toml` 中的一个 rev；macroquad 和 miniquad 必须配套。
- 升级库 rev 后必须重新跑质量对照和 ABBA。

### 10.6 其他限制

- 1200 帧与 1800 帧 cohort 不能混池。
- low sample1 与 normal sample2/actual4 不能直接比较。
- 观察器 cohort 有额外计时开销，不能当收益 cohort。
- ADB/transport 失败轮整组排除，不能只取成功轮。
- 温度、频率、内存水位没有完整记录；约 28 至 32°C 只是采样时温度，不是热稳定证明。
- 质量对照只覆盖采样帧，不覆盖全谱每一帧。
- “末帧 RGBA 相同”不等于“所有帧、所有视角、所有自定义材质都相同”。
- p99 不是保证值；单次尖峰可能改变 p99。
- 失败/回滚的条目仍保留在文档中，不能从统计里消失。

## 11. 结论强度表

### 11.1 指标分级

| 数字 | 级别 | 能支持 | 不能支持 |
| --- | --- | --- | --- |
| app FPS / 持续 FPS | 整帧 | 该窗口内的用户可感知帧率 | 所有谱面、所有设备、纯 GPU 能力 |
| frame interval p50/p95/p99/max | 整帧 | 该窗口内的帧间隔与尾部 | 当前帧自己的阶段分解 |
| app wall 均值 | 整帧（CPU/驱动墙钟） | 一帧应用侧总耗时 | 纯 GPU 时间 |
| update/render/flush | 阶段 | 各阶段墙钟 | 相加等于 app wall 之外的结论 |
| chart/HUD/particle/mask/glyph stage | 局部 | 阶段成本变化 | FPS 倍率 |
| render p50 / swap p50 / native draw | 提交/等待 | 提交与等待的变化 | 纯 GPU 执行时间 |
| upload bytes / draw calls / texture capacity | 局部工作量 | 传输/提交/容量变化 | 帧时间或 FPS |
| completed p50 / glFinish | 同步微基准 | 局部同步完成时间 | 游戏 FPS |
| SF presented FPS | 系统呈现 | 屏幕实际呈现节奏 | 应用渲染能力 |
| pixel diff | 质量 | 采样帧的像素等价性 | 全帧/全场景质量 |
| prepare_ms / cache hit | 加载阶段 | 加载准备成本 | 游玩 FPS |
| resident draw / skip count | 提交策略 | 记录/跳过数量 | 整帧收益 |

### 11.2 禁止换算

- 禁止把 `render` 调用倍率当成 FPS 倍率。Happy ending 的 render 调用约 19 倍加速，完整帧 interval 仅 53.64→34.40ms；不能写“游戏快 19 倍”。
- 禁止把阶段改善相加当成 app wall 改善。阶段是嵌套/包含关系，重复相加会重复计算。
- 禁止把 `1000 / p50` 当成持续 FPS。
- 禁止把 `p99` 改善当成平均 FPS 同比例改善。
- 禁止把上传字节减少当成帧时间同比例减少。
- 禁止把请求 `sampleCount` 当成实际 sample count。
- 禁止把单窗口结果写成全曲/全设备结论。
- 禁止把“GL 无错误”当成“像素通过”。
- 禁止把“末帧相同”当成“全帧相同”。
- 禁止把“代码已写”当成“性能已改善”。
- 禁止把“本地参考实现已落地”当成 `☑ 已实施（main）`；本地参考实现状态必须与主线状态分开写。

### 11.3 允许的表述

- “在同一 SO、同一谱面、同一 25 秒窗口、低画质下，app wall 从 9.042ms 降到 6.272ms（-30.64%）；FPS 仍约 59.78，因此判局部正优化。”
- “普通画质 4 轮 ABBA 中，FPS 从 46.115 升到 57.916，p99 从 48.532ms 降到 20.878ms；质量 10 组 RGBA 对相同；判整帧正优化，外推限于该设备和谱面。”
- “该候选只改善了 upload bytes，整帧 FPS 与 p99 在噪声内；判无优化。”
- “该候选在 4 轮 ABBA 中 FPS 从 53.12 降到 46.80，p99 上升 19.1%；判负优化。”

不允许的表述：

- “快了一倍”“提升 20%”而不说是哪个指标。
- “GPU 时间降低”而没有可信 GPU timer。
- “所有设备都能到 60fps”。
- “已经完成”但没有质量对照和真实消费者。

## 12. 常见误读

| 误读 | 事实 | 正确写法 |
| --- | --- | --- |
| render 快 19 倍，游戏快 19 倍 | 完整帧只从 53.64ms 降到 34.40ms | render 调用耗时约 19 倍下降；完整帧 interval 下降约 36% |
| app wall -20%，FPS +25% | 60Hz 窗口内 FPS 仍约 59.75 | app wall 局部改善，FPS 基本不变 |
| p99 -50%，平均 FPS +50% | p99 只描述尾部 | p99 改善；平均 FPS 需单独报告 |
| upload -50%，帧时间 -50% | 上传只是局部工作量 | 上传量下降；整帧未改善 |
| 60fps 封顶，所以没有优化 | 还要看 p99、长帧、app wall | 平均 FPS 封顶；p99/长帧仍有改善空间 |
| 一个谱面通过，所有谱面通过 | 谱面密度、效果、block 差异很大 | 只在该谱面/窗口成立 |
| 末帧 RGBA 相同，全场景相同 | 只覆盖采样帧 | 采样帧质量通过；全场景仍需覆盖 |
| 代码已写，优化已完成 | 代码存在不等于已合入 `main`，也不等于通过验收 | `☑ 已实施（main）` 需要合入提交 + 质量门 + ABBA；否则写 `☐ 未实施（main）` |

## 13. 方法变更记录

| 版本 | 日期 | 变更 |
| --- | --- | --- |
| 1.0 | 2026-11-11 | 首版：ABBA、app wall、分位数、像素对照、真机环境、证据等级、复现、术语、限制、结论强度 |
| 1.1 | 2026-11-11 | 明确状态口径：主线只用 `☐ 未实施（main）`/`☑ 已实施（main）`；参考实现另记；历史引文中的“已实施”只指本地参考实现落地；本地位置改用中性表述 |
