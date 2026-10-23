# Phira Pro 插件系统：Agent 参考

本文供需要理解、调试或修改本仓库插件系统的代码 agent 阅读。它概括架构、数据流、能力边界和改动时的验证重点。插件作者使用的完整格式、回调参数和打包步骤见 [Plugin API v2 开发说明](./README.md)、[通用模型插件 I/O 规范](./model-io.md)、[v1 可运行示例](./examples/density/)、[PPSR 示例](./examples/ppsr/) 与 [PDE-R 自有特征算法示例](./examples/pde-r/)。

## 一句话概览

Phira Pro 插件是以 `.zip` 导入的 **Rhai 脚本**，通过版本化的 **Phira Pro Plugin API** 读取谱面／结算数据或绘制只读 HUD。当前宿主为 v2，同时兼容 v1 插件；v2 增加只读固定 assets 与选中谱面回调。插件运行在受限沙箱中，不是动态库，也不能改谱、改判定、操作成绩或访问文件、网络、账户数据。

## 源码地图与数据流

| 位置 | 职责 |
| --- | --- |
| `phira/src/plugins.rs` | Manifest 和插件库读写、ZIP 导入校验、Rhai 引擎与限制、权限校验、回调调度、结果缓存及 HUD 绘制。理解系统行为时从这里开始。 |
| `phira/src/page/plugins.rs` | 插件管理 UI：导入、启用／停用、编辑 JSON 设置、调整 Rhai 操作数上限、查看详情、删除和刷新。 |
| `phira/src/page/settings.rs`、`phira/locales/*/settings.ftl` | 设置页入口及其本地化文字。 |
| `prpr/src/extensions.rs` | 引擎侧的观察者边界，只暴露谱面加载通知，不暴露可修改图表的 API。 |
| `prpr/src/scene/game.rs` | 图表解析后调用观察者；也提供不加载纹理的本地谱面解析路径，供详情页分析使用。 |
| `phira/src/ppsr_features.rs` | PPSR v2 兼容路径：仅在安装并启用 `phira-pro.ppsr` 时，将解析后的 `prpr::core::Chart` 提取为 PPSR unified 特征。 |
| `phira/src/scene/song.rs` | 详情页加载选中谱面的通用原始 JSON；只有 PPSR 兼容插件启用时才额外调用宿主 PPSR 提取器，并在正常本地记录保存时转发结算回调。 |
| `phira/src/lib.rs` | 应用启动时初始化插件，并在主 UI 绘制循环中绘制插件 HUD。 |
| `docs/plugins/README.md`、`docs/plugins/examples/density/` | API 规范及与测试绑定的示例插件。 |
| `docs/plugins/examples/pde-r/` | 自行解析原始谱面 JSON、提取算法私有 106 维特征并加载独立模型的 v2 插件示例。 |

典型流程：

1. 应用启动时 `plugins::init()` 读取用户数据目录下的 `plugins/index.json`，编译已启用脚本，并注册 `prpr::extensions` 的图表观察者。
2. `GameScene` 完成图表解析后，插件收到只读的标准化谱面摘要和可选原始 JSON；回放、练习等使用 `GameScene` 的流程也会经过此加载通知。
3. 对已在本机可读取的谱面，SongScene 将不超过 4 MiB 的 JSON 谱面原文放入 `chart_data` 并调用 `on_chart_selected`。一般插件在自己的 Rhai 脚本中解析谱面、提取自有特征并运行自带模型；插件之间不要求特征名或维度相同。为兼容既有 PPSR v2 包，只有其 ID 插件启用时宿主才额外生成并提供 PPSR 的 187 维特征。返回 `difficulty` 的选谱插件共用可拖动、可折叠难度面板；若同时有多个可显示结果，PPSR 优先，其他算法仍可在插件详情中查看。游玩时详情页暂停，面板自然隐藏。
4. 正常流程产出并保存本地 `SimpleRecord` 时，插件可收到结算回调。是否上传成绩与插件报告无关。
5. 应用绘制阶段最多每秒调用一次帧回调（5 Hz）；返回的 HUD 数据在后续绘制帧重复绘制，直到下次更新。

插件报告保存在内存中，不是谱面难度、PB、RKS 或服务器数据。后续谱面／结算回调可覆盖报告；停用、启用、刷新或修改设置会重新加载运行时并清除当前报告。

## API 能力

| 回调 | 权限 | 能做什么 | 关键边界 |
| --- | --- | --- | --- |
| `on_chart(ctx, settings)` | `chart.read` | 分析谱面元信息、有限数量的标准化音符、可解析的原始 JSON，以及本局 `speed` / `flow_speed`。 | 不提供实时坐标、判定线动画或噪域求值 API；`notes` 最多 50,000 条，必须检查 `truncated`。 |
| `on_chart_selected(ctx, settings)` | `chart.read` | v2：详情页选中可在本机读取的谱面后读取 `chart_data` 和元信息；脚本可以自行解析、计算特征，并读取 `ctx.assets` 中随插件固定的模型。 | 线上预览但未下载的谱面不触发；原文 JSON 仅在 4 MiB 内提供。只有旧版 PPSR 插件使用宿主生成的 187 维兼容向量。 |
| `on_result(ctx, settings)` | `result.read` | 分析本地 `SimpleRecord` 中的分数、判定统计、直方图等数据。 | 只在已有本地记录保存流程调用；练习或该流程未生成记录的对局没有回调。 |
| `on_frame(ctx, settings)` | `overlay.draw` | 读取当前应用时间和 FPS，返回最多 8 条文本 HUD。 | 每秒最多执行 5 次；只画字、不接收触摸，不能读取或控制游戏输入。 |

回调可以省略；存在时必须恰好接收 `(ctx, settings)` 两个参数，返回值必须能转换成 JSON。权限只决定是否调用相应回调，不会授予系统能力。具体字段结构和 HUD 坐标见 API 开发说明，不要根据示例自行推断额外 API。

## 用户如何使用

1. 在「设置 → 通用 → 插件管理」导入插件 `.zip`。
2. 首次导入默认关闭；通过管理页启用。需要时可编辑插件 JSON `settings`。
3. 打开／游玩谱面触发谱面分析；正常结算并保存记录时触发结算分析；HUD 在主界面绘制阶段更新。
4. 在插件管理页查看结果详情、刷新运行时或删除插件。

当前没有从应用内下载插件、原 ZIP 导出或原位更新流程。同 ID 插件更新需先删除旧插件再导入新 ZIP；删除会清除该插件的设置。

## 安装格式与资源限制

- ZIP 根目录必须有 UTF-8 文件 `manifest.json` 和 `main.rhai`，并且只能额外包含 Manifest `assetFiles` 声明的根目录 JSON 文件；不接受子目录和未声明的附加条目。
- ZIP 最大 2 MiB，解压后总内容最大 10 MiB，单个 JSON 资源最大 8 MiB，合并后的 `assets` 最大 8 MiB；最多安装 64 个插件；序列化后的插件索引最大 24 MiB。
- Manifest 声明 `api: 1`（legacy）或 `api: 2`（当前）；有效 `id`、非空 `name` 和 `version` 均必需。v1 仍可用原有回调，v2 才提供 `assets` 与 `on_chart_selected`。ID 仅可用 ASCII 字母、数字、`.`、`_`、`-`。
- `permissions` 只接受 `chart.read`、`result.read`、`overlay.draw`。未识别的 API 版本或权限会拒绝安装。
- `settings` 必须是 JSON 对象，最多 16 KiB；v2 Manifest 的 `assets` 与 `assetFiles` 加载后合计必须是 JSON 对象且最多 8 MiB，随插件持久保存并从 `ctx.assets` 暴露，设置页不提供编辑入口。`assetFiles` 将资产键映射至 ZIP 根目录 JSON 文件名，导入时解析并合并到 `assets`；资源文件名只允许 ASCII 字母、数字、点、下划线和连字符，且必须以 `.json` 结尾。单次回调 JSON 返回值最多 32 KiB。
- 通用 v2 插件从 `chart_data` 获取原始 JSON，可定义完全不同的音符解析和特征算法。只有启用旧版 `phira-pro.ppsr` 插件时，宿主才额外提供 PPSR 特定的 187 项兼容向量（114 个基础列、66 个 PPSR-v2 列和 7 个派生输入）；PPSR 仍需保持 `feature_names` 与配套模型一致。可替换模型和分项画像的格式见 [PPSR 模型 I/O 规范](./ppsr-model-io.md)。
- `PDE-R` 插件是当前自有特征算法的端到端示例：自己的 Rhai 脚本直接从 `chart_data` 建立 106 项特征向量，不依赖 PPSR 特征提取器，模型缓存放在自己的插件资源中。
- 第三方模型推荐遵循 [通用模型插件 I/O 规范](./model-io.md)：算法自定义并版本化特征 schema，模型资产明确标识格式与版本，回调返回有限 `difficulty` 和可解释的可选 `parts`；不要把 PPSR 的 187 维向量误认为 API 通用要求。
- 原始谱面 JSON 仅在源数据不超过 4 MiB 且可解析为 JSON 时提供，否则为 JSON `null`。
- 管理页将脚本和设置保存到应用数据目录 `plugins/index.json`；插件本身没有访问该目录的能力。

## 沙箱及失败语义

Rhai 禁止文件模块导入，且不注册原生动态加载、文件系统、网络、系统命令、账户凭据、触摸注入或游戏状态写入 API。`print` / `debug` 输出被丢弃。

主要执行限制：

- 谱面和结算回调：默认最多 5,000,000 次 Rhai 操作，可在插件管理页调整，配置单独保存在 `plugins/operation_limit.json`；非法值回退默认值。帧回调仍固定最多 25,000 次。
- 调用深度 32；表达式深度限制 64 / 32；变量和函数各最多 256。
- 字符串最多 8 MiB；数组与 Map 各最多 1,000,000 项（Rhai 对嵌套容器递归计数）。
- `on_chart` 的标准化音符上限为 50,000；`on_chart` 原始 JSON 与 `on_chart_selected.chart_data` 均限制在 4 MiB。

选谱和谱面回调的 `ctx.operation_limit` 提供当前非帧预算，插件可用于成本预估；该值不会放宽 Rhai 的实际硬限制。操作数限制不等于墙钟时间保证。回调发生错误、超限或返回非 JSON 值后，该插件会暂停，错误通过管理页报告；刷新或重新启用可重新加载。不要吞掉宿主端错误或把错误结果伪装成成功分析。

## Agent 修改准则

- 新增或变更能力时，先明确它属于只读分析、只读 HUD，还是违反当前安全边界的状态修改。不要通过增加脚本绑定或回调把宿主内部可变对象暴露给插件。
- 改动回调时同步检查权限名、参数形状、触发时机、返回值、错误报告和 API 兼容版本；旧主程序应明确拒绝依赖新能力的更高 API 插件，同时新版可继续兼容旧 API；同步更新 Manifest 校验、开发说明与示例。
- 改动 ZIP／Manifest／持久化时，保持导入默认禁用、`assetFiles` 与实际条目完全匹配、包结构与大小上限、ID 唯一性、设置校验和原子索引写入；资源内容仅作 JSON 数据解析，不要把原始插件内容执行到沙箱之外。
- 改动谱面提取时，保留假音符过滤、按时间排序、`note_count` 与 `truncated` 的语义，并确认解析前后及回放／练习加载均符合预期。
- 改动 HUD 时，保持权限检查、5 Hz 回调节流、数量／文本长度限制、坐标夹取及只读、不接收触摸的行为。
- 设置页入口文字由 `phira/locales/` 管理；插件管理页的主要文案目前直接写在 `phira/src/page/plugins.rs`。新增本地化时，按项目语言资源模式补齐各语言，且检查管理页操作与错误提示。
- 以 `docs/plugins/README.md` 为面向插件作者的规范来源；若本指南和实现不一致，以代码为准，并修正文档。

## 验证入口

插件运行时、示例脚本、ZIP 导入和沙箱行为的单元测试位于 `phira/src/plugins.rs` 的 `plugins::tests`。相关验证可使用：

```powershell
cargo test -p phira plugins::tests
```

谱面加载、结算回调和管理页的端到端回归还涉及 `phira/src/scene/replay/tests.rs`；UI 截图回归仅在 Windows 测试配置下启用。只改本文档不需要运行测试。
