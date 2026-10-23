# Phira Pro Plugin API v2 · 插件开发说明

接口正式名称为 **Phira Pro Plugin API**，当前兼容版本为 **v2**。Manifest 和 `on_chart` / `on_chart_selected` 参数中的数值 `api` 是机器读取的兼容版本号，并非接口名称；它与应用版本 pro.14、插件自身 `version` 以及 Rhai 语言版本分别独立。当前宿主兼容 `"api": 1` 的既有插件；v2 新增固定 `assets` 和 `on_chart_selected`，需要这些能力的插件必须声明 `"api": 2`，旧宿主会明确拒绝 v2 插件。

源码：`phira/src/plugins.rs`、`phira/src/page/plugins.rs`、`prpr/src/extensions.rs`。
此版本运行 Rhai 1.26.1 脚本，Windows、Android 和 iOS 使用同一套接口。插件入口为「设置 → 通用 → 插件管理」，支持导入、启停、编辑 JSON 设置、查看结果和删除。首次安装默认关闭；启用后打开谱面才产生谱面分析结果。

仓库代码 agent 的架构地图、数据流、能力边界与改动验证提示见 [Agent 参考](./agent-guide.md)。
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
- `raw`：解析原谱面 JSON；原数据超过 4MiB 或并非 JSON 时为 JSON null（Rhai 中通常为 unit）。它保留编辑器字段，需自行检测谱面格式。标准化 notes 暂不提供逐时刻坐标、判定线动画、噪域求值接口。

`on_chart_selected` 提供 `api`、谱面 `name` / `level` / `difficulty`、`format`（`rpe` / `pec` / `pgr` / `pbc`）、`chart_set`（内置章节谱为 `phigros`，其余本地谱为 `phira`）、`chart_data` 和 `operation_limit`。`chart_data` 是原始谱面文件解析出的 JSON 对象；文件超过 4 MiB、不是 JSON 或不是对象时为 JSON null。`operation_limit` 是当前非帧回调的 Rhai 操作预算；用户可在插件管理页调整。该原始输入允许每个插件在自身 Rhai 脚本中实现不同的音符解析、特征定义与模型适配；宿主不要求不同算法共用特征列。`assets` 是 Manifest 固定缓存，适合随插件携带模型和算法配置。模型插件的推荐模型元数据、特征 schema 和预测报告结构见 [模型插件 I/O 规范](./model-io.md)。

PPSR v2 的现有安装包还依赖兼容字段：`feature_version`、`v2_feature_version`、`feature_names`、`features`（严格排序的 187 个数值）、`note_total`、`mech_total`、`duration` 和 `fake_ratio`。这组特征仍由宿主的 PPSR 提取器生成，且仅当安装并启用了 ID 为 `phira-pro.ppsr` 的插件时才计算；其他插件只收到原始 `chart_data`，不触发 PPSR 特征计算。新插件应自行提取特征，并不要依赖 PPSR 专用列。谱面分析只针对宿主可从本机读取的谱面；线上预览、尚未下载的谱面不能在进入游戏前分析。分析通过异步任务推进，报告保存在内存，返回 SongScene 后仍可供 PPSR 专用面板显示。

示例 [pde-r/main.rhai](examples/pde-r/main.rhai) 是自包含的不同算法：脚本读取 `chart_data`，在 Rhai 中解析谱面并计算自己的 106 维输入，不读取 PPSR 的 `features`。其插件 ZIP 和检查点均与 PPSR 独立。

`on_result` 参数采用 camelCase：`score`、`accuracy`（0～1）、`fullCombo`、`maxCombo`、`numOfNotes`、`counts`（PERFECT 含 Perfect+ / GOOD / BAD / MISS）、`gradeCounts`、`grading`、`hist`、`std`（秒）。可选字段可能为 null；`gradeCounts` 索引为 PERFECT=0、GOOD=1、BAD=2、MISS=3、PERFECT+=4、GREAT=5、OK=6、MEH=7。

`on_chart` / `on_result` 返回对象由管理页「详情」显示。可约定 `{ algorithm: "ppsr", difficulty: 12.34, text: "…" }`：这些字段供作者组织结果，核心不会将其写入官服定数、PB、RKS 或上传内容。插件系统本身不内置 PPSR，也没有自动替换内置难度的注册接口；PPSR 作为独立插件随本目录单独分发。

## 个性化显示

有 `overlay.draw` 权限时，返回对象可包含 `hud` 数组；`on_frame` 则直接返回这个数组。每项 `{ text, x, y, color }`，最多绘制 8 项，每项最多 160 字符。`x` 是横向归一化坐标、`y` 是相对半屏高度坐标，均夹到 −0.9～+0.9；原点居中，向右／向下为正。`color` 为整数 `0xRRGGBB`，默认白色。使用固定字号 0.4；显示位于当前画面上层，不接收触摸。`on_frame` 返回值会覆盖当前结果中的 `hud`；不应假设它能读取上一次 `on_chart` 返回值。

示例 [main.rhai](examples/density/main.rhai) 包含可执行的密度统计、结算分析与 FPS 显示。密度是示例指标，不是 PPSR，也不声称反映真实谱面难度。

[PPSR v2 独立插件 ZIP](./Phira-Pro-PPSR.zip) 内含可单独替换的 187 维 `model.json` 和分项画像 `profile.json`；源码见 [PPSR 插件示例目录](./examples/ppsr/)。替换模型时保持宿主 187 项 `feature_names` 的完全相同顺序与特征版本，并生成匹配的分项画像；完整模型输入／输出约定见 [PPSR 模型 I/O 规范](./ppsr-model-io.md)。插件 ID 沿用 `phira-pro.ppsr`，更新时先删除旧版插件再导入 v2 包，之后手动启用。它仅对本机可读取的谱面在详情页计算，线上谱需先下载；插件缓存不随运行或用户设置改变。总难度使用模型预测值（Phigros 官方谱面使用官方定数）；细分项按特征画像百分位映射到 1–10 分，并以进度条展示。详情浮窗可拖动，也可收起或展开。

[PDE-R 独立插件 ZIP](./Phira-Pro-PDE-R.zip) 展示另一种算法接入方式：它在插件自己的 Rhai 脚本中解析 `chart_data`、提取算法专用的 106 维特征并执行独立模型，不使用 PPSR 的 187 维特征。源码见 [PDE-R 插件示例目录](./examples/pde-r/)。两种插件可同时安装；PDE-R 使用独立 ID `phira-pro.pde-r`，不会覆盖 PPSR。任一选谱插件返回 `difficulty` 后都可以使用共用的可折叠面板；当多种算法同时返回时，当前面板优先显示 PPSR，其他结果仍可在插件管理页查看。

## 执行限制与错误

没有原生动态库、文件系统、网络、系统命令、账户数据、触摸注入或判定／成绩修改接口。禁用文件模块导入；`print` 和 `debug` 不输出。谱面和结算回调默认最多 5,000,000 个操作，可在插件管理页调整；非法值回退到默认值。帧回调仍固定最多 25,000 个操作。调用深度 32，表达式深度 64/32，变量／函数最多 256，字符串数据最多 8MiB，数组和 Map 各最多 1,000,000 项（Rhai 对嵌套容器递归累加计数），JSON 返回值最多 32KiB。操作计数不是墙钟时间保证；这些限制用于控制脚本开销，复杂算法应自行优化。

回调出错、超限或返回非 JSON 数据时，该插件暂停执行，管理页显示原因。修改设置、停用后再启用或刷新可重新加载。插件的文件读写与安装均使用应用的数据目录；插件本身不能访问该目录。开发时请在副本配置中验证耗时与错误行为。
