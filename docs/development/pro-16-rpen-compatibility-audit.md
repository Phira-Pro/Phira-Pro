# RPEN 适配清单（pro.16 开发轮）

审查日期：2026-10-20。RPEN 对照版本为 `0.12.0`，固定提交 [`1ee0fc3e3e434696f00cbb51d4107379913a6490`](https://github.com/kclg-YSJ/rpe-next/tree/1ee0fc3e3e434696f00cbb51d4107379913a6490)。下表保留初始差异调查；后续经用户确认，构建 80 已实施十项适配，实施范围见下节。

这里的“缺口”包括 RPEN 新扩展和播放器已有但尚未覆盖的旧字段，不把所有项都说成 RPEN 最近新增。

| 编号 | 适配项 | RPEN 数据/行为 | 当前缺口与影响 | 建议顺序 |
|---|---|---|---|---|
| 1 | 整体曲线轨迹 | `moveXEvents[].trajectory`，v1 的 options/segments/split；整体事件同时驱动 X/Y，可选切线旋转；最低 RPEVersion 210 | `RPEEvent` 未读 trajectory，只读数值 start/end，不能还原完整路径和自动旋转 | 第一批 |
| 2 | 分层噪域 | `noiseAreaLayers`、区域 `layerId`，最低 230；每层独立合成，按层顺序叠加 | 解析器只装载一个区域列表，不读层 ID/顺序；不同层可能错误地参与同一个反转集合 | 第一批 |
| 3 | 新噪域重叠语义 | 当前 NoisePreview 按激活/未激活集合分别合成；预备状态同时属于未激活；反转恰好覆盖一次才反转，两次及以上不反转 | 现有 `next_single_inversion` 仅在 `noiseAreaOptions.ignoreTripleInversion=true` 时采用“恰好一次”，缺省仍为奇偶规则。三重覆盖和层间重叠需要新版兼容分支，不能改坏旧谱 | 与第 2 项一起 |
| 4 | 噪域自定义皮肤及参数 | 层的 skin/parameters/channel；`shader/<name>/main.glsl` 和 config.json；多 Pass、命名 Target、历史纹理、外部纹理、低性能通道；内置 NoiseField/Ocean 也走资源包 | 当前只走内建官方噪域材料，不解释这些皮肤资源及配置。应保留无自定义皮肤时的官方材料，在显式指定皮肤时加载对应协议；不能用简易红色矩形替代 | 第二批 |
| 5 | 谱内 shader 事件与叠加顺序 | `effects` / `shaderEvents` / `META.effects` / `extended.paintEvents`；含 vars 参数轨道、line/global/order/clone；两层按顺序叠加 | 当前 `extra.json` 的特效实现是另一种入口，不能等同于支持谱内事件；根字段未读，paintEvents 还被解释为数值绘图事件，部分写法可能直接读取失败。参数时间与绘制层次均需转换 | 第一批 |
| 6 | 扩展 shader 库 | shader.mjs 除原十类及其第二层副本外，还列出 liquid、flowing、snow、glow_effect、rain、underwater、hologram、wave 等 30 类 | 当前内建表仅十类。可读自定义 GLSL 不等于已适配这些名称、参数、纹理及渲染语义；需逐种建立实现/兼容映射 | 在第 5 项后分批 |
| 7 | 全局事件容器 | `globalEvents.version=1`、types、columns、source、settings，最低 240；事件使用全局 BPM，语义由提供者定义 | 当前解析器没有该容器/来源处理。应先可识别并报告未知类型，再为具体已知类型注册求值；不能承诺任意 JS 模组可直接运行 | 三维前置 |
| 8 | 三维摄像机 | `spatial-camera/position` 的 x/y/z、rotation 的 yaw/pitch/roll、lens 的 fov；lens.settings.followChart 控制 UI/背景跟随 | 当前没有该投影管线。需要一起处理线、音符、特效、噪域投影及触点映射，不能仅对整幅截图做变形。属于第 7 项中的具体语义实现 | 独立第三批 |
| 9 | 音符倾斜控制曲线 | 判定线 `skewControl`，RPEN 用距离曲线求值后改变非 Hold 音符的倾斜 | 当前 RPEJudgeLine/CtrlObject 未读/应用 skewControl；已有 alpha/pos/size/yControl 不包含此项 | 第一批 |
| 10 | 判定线 BPM 倍率 | `bpmfactor` 同时用于本线事件、扩展事件和音符的拍数到秒转换 | 当前 RPEJudgeLine 仍标记 TODO，未装载 bpmfactor；非 1 倍率可能产生时刻/运动偏差。噪域与全局事件应继续使用全局 BPM，不应跟着缩放 | 第一批 |

## 核查依据

- [轨迹声明和验证](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/chart.mjs)、[轨迹采样](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/curve-trajectory.mjs)、[运行时（含 bpmfactor/skew）](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/scene.mjs)。
- [噪域层和皮肤协议解析](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/noise-skin.mjs)、[分层与反转合成](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/ui/noise-preview.mjs)、[皮肤渲染器](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/ui/noise-skin-renderer.mjs)。
- [shader 事件读写](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/shader-events.mjs)、[shader 库/参数求值](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/src/core/shader.mjs)。
- [全局事件协议](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/protocols/GLOBAL-EVENTS-PROTOCOL.zh-CN.md)、[三维摄像机坐标与存储说明](https://github.com/kclg-YSJ/rpe-next/blob/1ee0fc3e3e434696f00cbb51d4107379913a6490/assets/mods/spatial-camera/README.zh-CN.md)。
- 本地实现：`prpr/src/parse/rpe.rs`、`prpr/src/core/rpe_next_block.rs`、`prpr/src/core/chart.rs`、`prpr/src/core/block_shader.rs`、`prpr/src/core/effect.rs`、`prpr/src/parse/extra.rs`。

## 已有能力与无需单独适配的编辑工具

现有实现已包含 v220 噪域的拍数/画布坐标、生命周期、readyFlash 秒数、激活区间、移动/旋转/缩放/锚点与原有反转选项；不能把这些全列成未支持。普通 RPE 的父线、贴图锚点、GIF 事件、颜色/文字/缩放/倾斜线事件、Bezier、假音符、音符 tint/tintHitEffects 等也已有代码路径，但不据此宣称与最新 RPEN 的所有边界行为完全一致。

噪域图案/文字或图片拟合、噪域追踪、普通曲线音符填充、文字拼合、批量复制与协作等工具，若最终导出的是普通音符和已支持事件，播放器不需要实现这些编辑操作。整体轨迹导出为原 RPE 并拆成普通事件可暂时规避第 1 项；全局事件和噪域在兼容导出中可能被移除，不能把“文件能打开”视为效果完整。

## 构建 80 实施范围

| 编号 | 实现与验证入口 |
|---|---|
| 1 | `parse/rpen/trajectory.rs` 与有界数学表达式解释器；轨迹、极坐标、trim、缩放/旋转/对齐、形状、随机参数、切线旋转。使用精确时间求值，跳转不依赖前一帧。 |
| 2、3 | 每层独立区域集合、遮罩缓存和渲染器；按声明顺序叠加；激活/未激活/预备状态及恰好一次反转按新版语义处理。关闭 RPEN 时保留原规则。 |
| 4 | `core/rpen_skin.rs`；参数、多 Pass、命名目标、通道写入、历史反馈、外部纹理、低性能通道。NoiseField/Ocean 资源随应用提供。没有显式自定义 skin 时继续使用官方噪域材料。 |
| 5、6 | `parse/rpen/effects.rs`；谱内 shader 入口、参数轨道、line/global/order/clone；30 个新预设保留上游 GLSL，与原有十类并存。`_2` 为叠加副本，不隐式代表 global。 |
| 7 | 识别并保留 globalEvents v1 的类型、来源、设置及轨道；已注册 spatial-camera 提供者。未知提供者会记录未实现类型，不会执行附带的 JavaScript；任意第三方全局事件不能据此视为已支持。 |
| 8 | `core/rpen_scene.rs`；位置/旋转/FOV、followChart；线、音符、粒子、噪域使用平面三维投影，输入做逆映射；近远裁剪与传统二维剔除分别处理。应用菜单不随谱面投影。 |
| 9 | `CtrlObject.skew` 距离曲线用于非 Hold 音符剪切，保留上下方向及缩放关系。 |
| 10 | 每条判定线独立 BPM 倍率，覆盖音符、本线事件与扩展事件；全局事件和独立噪域继续使用全局 BPM。 |

解析主要位于 `prpr/src/parse/rpen/`，场景/皮肤运行时位于 `prpr/src/core/rpen_scene.rs`、`rpen_skin.rs`。开关是谱面元数据 `useRpen`，不是全局偏好。

`scripts/qa/rpen_reference.mjs` 从固定上游源码生成可复验数值样本；Rust 核心回归对比轨迹、摄像机和 BPM 时间。真实 GPU 回归入口为 `production_replay_transport_import_and_aspects` 内的 `rpen_qa`，检查生产导入器、实际 shader 编译/像素、比例、开关回退、暂停/倒退、反馈历史、低性能通道及投影裁剪。

资源授权见 `assets/rpen/NOTICE.txt` 和 `LICENSE-RPEN.txt`。上游资源按其 PolyForm Noncommercial 条款保留，不能将其视为项目其他文件的许可证。
