# Phira Pro Plugin API v3 · 插件开发说明

当前宿主新增 **v3 回放分析能力**，继续兼容 v1/v2。既有谱面接口与示例保持原契约。PRCA 的安装、精度、算法覆盖与限制见 [PRCA 说明](./prca.md)。

v3 插件可以声明 `replay.read` 与 `report.render`，实现 `on_replay_analysis(ctx, settings)`。只有同时声明两项权限、实现此回调并启用的插件才显示在回放分析页。用户明确选择一份回放后运行；插件不能自行遍历文件、访问任意路径或修改游戏状态。

回调 `ctx` 包含 `api=3`、`feature_schema="replay-observations-1"`、`metadata`、`chart`、`quality`、`metrics`、`windows`、`coverage`、`assets` 和 `replay_hash`。原始输入分块调用 `replay_inputs(start,count)`，判定事件分块调用 `replay_judgements(start,count)`；`start` 从零开始，每页 1–256 条，非法页返回空数组。完整 u64 输入 ID 使用十进制字符串，旧输入明确标 `legacy` 和帧精度。回调不能取得用户账户凭据或未选中的文件。

Build 91 新增 `compact.version=2` 概览、`quality.primary_source` 和逐项能力信息。固定 32 操作块通过 `replay_observations(kind,start,count)` 读取，`kind` 为 `timing` 或 `duration`，每页 1–256 块；块数分别为 `compact.timing_blocks` / `duration_blocks`。块包含 `key/start/end/stats/intervals`，时长块另含暂停 `segment`。同帧和弦压缩为一个有效计时组，键盘时长按键分组；完整块留在原生层及 JSON 报告，避免脚本上下文随回放长度膨胀。既有 `windows` 字段为旧版兼容保留。规则可返回 `checks`，PNG 最多显示 6 项，区分检测覆盖与异常证据。`limited` 和 `conflict` 显示中性的数据不足、数据异常；`review` 才显示黄色存疑。

返回对象必须包含 `status`（`clear`/`review`/`limited`/`conflict`）、`summary` 和 `evidence` 数组；证据最多 24 条，每条含 `family/title/detail/start/end`，起止为谱面秒。另支持 `cheat`，但必须同时声明 `calibrated=true`、`confirmed=true`，且至少两个不同 `family` 的证据各自有 `confirmed=true`；否则拒绝报告。返回值仍限制 32KiB；绘图序列由宿主持有，不通过巨大 JSON 返回值搬运。宿主校验结构、执行受控 PNG 渲染并保存 JSON；不执行任意 HTML。重计算运行在专用工作线程，通过通道返回结果与异常；用户可取消，界面和原生/脚本阶段分别检查 120 秒总预算（单次底层读取/解压不保证立即中断）。更改插件后旧任务不能覆盖新结果。

下述 v2 说明描述既有谱面能力；v2 ZIP 仍声明 `api:2`，无需为兼容修改自身特征或模型。

接口正式名称为 **Phira Pro Plugin API**，当前最高兼容版本为 **v3**。Manifest 和 `on_chart` / `on_chart_selected` 参数中的数值 `api` 是机器读取的兼容版本号，并非接口名称；它与应用版本、插件自身 `version` 以及 Rhai 语言版本分别独立。当前宿主兼容 `"api": 1` 和 `"api": 2` 的既有插件；v2 新增固定 `assets` 和 `on_chart_selected`，v3 新增回放分析。需要对应能力的插件应声明相应最低 API，旧宿主会明确拒绝更高版本。

源码：`phira/src/plugins.rs`、`phira/src/page/plugins.rs`、`prpr/src/extensions.rs`。
此版本运行 Rhai 1.26.1 脚本，Windows、Android 和 iOS 使用同一套接口。插件入口为「设置 → 通用 → 插件管理」，支持导入、启停、编辑 JSON 设置、查看结果和删除。首次安装默认关闭；启用后打开谱面才产生谱面分析结果。

通用第三方模型的回调输入、模型资源、特征 schema、预测报告格式及适配检查清单见 [模型插件 I/O 规范](./model-io.md)；该规范不要求算法使用 PPSR 特征。

## 文件与安装

插件包使用普通 `.zip`，根目录必须包含 UTF-8 文件 `manifest.json`、`main.rhai`，以及 Manifest 的 `assetFiles` 声明的 JSON 资源文件；不允许未声明的文件或子目录。压缩包不超过 2MiB，解压后总内容不超过 10MiB；单个 JSON 资源最多 8MiB，合并后的 `assets` 最多 8MiB。安装数最多 64，序列化后的插件库索引最多 24MiB。同一 ID 更新时先删除旧版本，再导入新包（删除会清除该插件设置）。脚本和设置保存在用户数据目录的 `plugins/index.json`，不是原 ZIP。

Manifest 示例见 [density/manifest.json](examples/density/manifest.json)（v1）和 [ppsr/manifest.json](examples/ppsr/manifest.json)（v2）。必填 `api`（v1 数值 `1` 或当前 v2 数值 `2`）、`id`、`name`、`version`；可选 `author`、`description`、`permissions`、`settings`、`assets`、`assetFiles`。ID 仅允许 ASCII 字母、数字、点、下划线、连字符，最多 80 字节；名称非空、最多 160 字节；版本最多 80 字节，说明最多 2048 字节。`settings` 必须是 JSON 对象；管理页编辑保存的设置上限为 16KiB。v2 的 `assets` 与 `assetFiles` 资源是随插件固定安装、不能在设置页修改的 JSON 数据，合计上限 8MiB；`assetFiles` 将资源键映射到 ZIP 根目录中的 JSON 文件名，导入时解析后与内嵌 `assets` 合并，并在 `on_chart` / `on_chart_selected` 中通过 `ctx.assets` 提供。例如 PPSR Manifest 中 `"assetFiles": {"model": "model.json", "profile": "profile.json"}`，脚本仍使用 `ctx.assets.model` 和 `ctx.assets.profile`。插件作者可替换相应文件再重新打包，无需改脚本；替换模型时应同时使用与模型特征顺序及分布相匹配的画像。脚本对回调副本的改动不会持久化。未知 API 或权限拒绝安装。声明权限不会获得文件或网络访问能力。

打包示例（在仓库根目录，Python 标准库）：

```python
import json
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
source = Path("docs/plugins/examples/ppsr")
manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
with ZipFile("ppsr.zip", "w", ZIP_DEFLATED) as archive:
    for name in ("manifest.json", "main.rhai", *manifest.get("assetFiles", {}).values()):
        archive.write(source / name, name)
```

对于不带 `assetFiles` 的插件，只打包 `manifest.json` 和 `main.rhai` 即可。
## 回调

函数都可省略。必须有两个参数 `(ctx, settings)`，返回可转 JSON 的数据；`settings` 为管理页保存的对象。函数调用之间不保证脚本全局变量保存状态。刷新或修改插件会重新加载并清除当前分析结果。分析结果只保存在运行内存，后一个谱面／结算回调覆盖前一个结果。

| 函数 | 所需权限 | 调用时机与参数 |
| --- | --- | --- |
| `on_chart(ctx, settings)` | `chart.read` | GameScene 加载谱面后，包括回放及练习；参数如下 |
| `on_chart_selected(ctx, settings)` | `chart.read` | v2：选中可在本机读取的谱面后、进入游戏前；提供原始 JSON 谱面数据及元信息，插件可自行提取特征；PPSR v2 旧版兼容路径另提供 187 维宿主特征 |
| `on_result(ctx, settings)` | `result.read` | 正常结算保存本地记录时；参数为 SimpleRecord，练习及被现有计分流程排除的局不调用 |
| `on_frame(ctx, settings)` | `overlay.draw` | 主应用绘制阶段，每秒最多 5 次；`time` 为应用计时秒数、`fps` 为当前采样帧率 |

`on_chart` 参数：

- `api`：1；`name`、`level`、`difficulty`：谱面信息；`speed`、`flow_speed`：本局设置；`operation_limit`：当前非帧回调的操作预算。
- `note_count`：非假音符总数；`notes`：按头部时间升序排列，最多 50,000 项；`truncated` 明确指出是否截断。分析算法必须检查此标志，不能把截断结果当完整谱面。
- 每个音符含 `line`、`index`（原始判定线／音符索引，从 0 开始）、`time`、`end_time`（谱面秒，未加音频偏移，非 Hold 等于 time）、`kind`（`tap` / `drag` / `flick` / `hold`）、`above`、`speed`。
- `raw`：解析原谱面 JSON；原数据超过 32 MiB 或并非合法 JSON 时为 JSON null（Rhai 中通常为 unit），超限时另有 `raw_truncated: true`。它保留编辑器字段，需自行检测谱面格式。标准化 notes 暂不提供逐时刻坐标、判定线动画、噪域求值接口。

`on_chart_selected` 提供 `api`、谱面 `name` / `level` / `difficulty`、`format`（`rpe` / `pec` / `pgr` / `pbc`）、`chart_set`（内置章节谱为 `phigros`，其余本地谱为 `phira`）、`chart_data` 和 `operation_limit`。`chart_data` 是原始谱面文件解析出的 JSON 对象；源文件超过 32 MiB 时为 JSON null，并通过 `raw_truncated: true` 明示大小限制；不合法 JSON 或顶层非对象也会返回 null。`raw_truncated` 为 false 不代表文件一定是合法 JSON。`operation_limit` 是当前非帧回调的 Rhai 操作预算，默认 20,000,000；用户可在插件管理页调整。该原始输入允许每个插件在自身 Rhai 脚本中实现不同的音符解析、特征定义与模型适配；宿主不要求不同算法共用特征列。`assets` 是 Manifest 固定缓存，适合随插件携带模型和算法配置。模型插件的推荐模型元数据、特征 schema 和预测报告结构见 [模型插件 I/O 规范](./model-io.md)。

PPSR v2 的现有安装包还依赖兼容字段：`feature_version`、`v2_feature_version`、`feature_names`、`features`（严格排序的 187 个数值）、`note_total`、`mech_total`、`duration` 和 `fake_ratio`。这组特征仍由宿主的 PPSR 提取器生成，且仅当安装并启用了 ID 为 `phira-pro.ppsr` 的插件时才计算；其他插件只收到原始 `chart_data`，不触发 PPSR 特征计算。新插件应自行提取特征，并不要依赖 PPSR 专用列。谱面分析只针对宿主可从本机读取的谱面；线上预览、尚未下载的谱面不能在进入游戏前分析。分析通过异步任务推进，报告保存在内存，返回 SongScene 后仍可供 PPSR 专用面板显示。

示例 [pde-r/main.rhai](examples/pde-r/main.rhai) 是自包含的不同算法：脚本读取 `chart_data`，在 Rhai 中解析谱面并计算自己的 106 维输入，不读取 PPSR 的 `features`。其插件 ZIP 和检查点均与 PPSR 独立。

`on_result` 参数采用 camelCase：`score`、`accuracy`（0～1）、`fullCombo`、`maxCombo`、`numOfNotes`、`counts`（PERFECT 含 Perfect+ / GOOD / BAD / MISS）、`gradeCounts`、`grading`、`hist`、`std`（秒）。可选字段可能为 null；`gradeCounts` 索引为 PERFECT=0、GOOD=1、BAD=2、MISS=3、PERFECT+=4、GREAT=5、OK=6、MEH=7。

`on_chart` / `on_result` 返回对象由管理页「详情」显示。`on_chart_selected` 若返回 `difficulty`，共用面板标题优先读取返回对象的 `title` 字符串，没有该字段时显示 Manifest 插件名称。可约定 `{ algorithm: "ppsr", difficulty: 12.34, text: "…" }`：这些字段供作者组织结果，核心不会将其写入官服定数、PB、RKS 或上传内容。插件系统本身不内置 PPSR，也没有自动替换内置难度的注册接口；PPSR 作为独立插件随本目录单独分发。

## 个性化显示

有 `overlay.draw` 权限时，返回对象可包含 `hud` 数组；`on_frame` 则直接返回这个数组。每项 `{ text, x, y, color }`，最多绘制 8 项，每项最多 160 字符。`x` 是横向归一化坐标、`y` 是相对半屏高度坐标，均夹到 −0.9～+0.9；原点居中，向右／向下为正。`color` 为整数 `0xRRGGBB`，默认白色。使用固定字号 0.4；显示位于当前画面上层，不接收触摸。`on_frame` 返回值会覆盖当前结果中的 `hud`；不应假设它能读取上一次 `on_chart` 返回值。

示例 [main.rhai](examples/density/main.rhai) 包含可执行的密度统计、结算分析与 FPS 显示。密度是示例指标，不是 PPSR，也不声称反映真实谱面难度。

[PPSR v2 独立插件 ZIP](./Phira-Pro-PPSR.zip) 内含可单独替换的 187 维 `model.json` 和分项画像 `profile.json`；源码见 [PPSR 插件示例目录](./examples/ppsr/)。替换模型时保持宿主 187 项 `feature_names` 的完全相同顺序与特征版本，并生成匹配的分项画像；完整模型输入／输出约定见 [PPSR 模型 I/O 规范](./ppsr-model-io.md)。插件 ID 沿用 `phira-pro.ppsr`，更新时先删除旧版插件再导入 v2 包，之后手动启用。它仅对本机可读取的谱面在详情页计算，线上谱需先下载；插件缓存不随运行或用户设置改变。总难度使用模型预测值（Phigros 官方谱面使用官方定数）；细分项按特征画像百分位映射到 1–10 分，并以进度条展示。详情浮窗可拖动，也可收起或展开。

[PDE-R 独立插件 ZIP](./Phira-Pro-PDE-R.zip) 展示另一种算法接入方式：它在插件自己的 Rhai 脚本中解析 `chart_data`、提取算法专用的 106 维特征并执行独立模型，不使用 PPSR 的 187 维特征。源码见 [PDE-R 插件示例目录](./examples/pde-r/)。两种插件可同时安装；PDE-R 使用独立 ID `phira-pro.pde-r`，不会覆盖 PPSR。任一选谱插件返回 `difficulty` 后都可以使用共用的可折叠面板；当多种算法同时返回时，当前面板优先显示 PPSR，其他结果仍可在插件管理页查看。

## 执行限制与错误

pro.17 本轮更新后，选谱分析通过后台工作线程串行运行。本地文件读取、CPU 谱面解析、特征提取和 Rhai 模型计算不会占用界面线程；快速换谱、重新加载或停用插件后，旧任务结果不会覆盖当前面板。结果就绪后面板用 250ms 淡入，纹理上传和界面绘制仍在主线程执行。此调度由宿主提供，单独更换插件 ZIP 无法改变旧宿主的执行线程。

没有原生动态库、文件系统、网络、系统命令、账户数据、触摸注入或判定／成绩修改接口。禁用文件模块导入；`print` 和 `debug` 不输出。谱面和结算回调默认最多 20,000,000 个操作，可在插件管理页调整；非法值回退到默认值。帧回调仍固定最多 25,000 个操作。操作超限或回调报错会暂停插件；选谱页会显示失败提示，插件管理页「详情」保留错误原因，刷新或重新启用可恢复。调用深度 32，表达式深度 64/32，变量／函数最多 256，字符串数据最多 8MiB，数组和 Map 各最多 1,000,000 项（Rhai 对嵌套容器递归累加计数），JSON 返回值最多 32KiB。操作计数不是墙钟时间保证；这些限制用于控制脚本开销，复杂算法应自行优化。

回调出错、超限或返回非 JSON 数据时，该插件暂停执行，管理页显示原因。修改设置、停用后再启用或刷新可重新加载。插件的文件读写与安装均使用应用的数据目录；插件本身不能访问该目录。开发时请在副本配置中验证耗时与错误行为。
