# 插件开发说明（API 1）

源码：`phira/src/plugins.rs`、`phira/src/page/plugins.rs`、`prpr/src/extensions.rs`。
此版本运行 Rhai 1.26.1 脚本，Windows、Android 和 iOS 使用同一套接口。插件入口为「设置 → 谱面 → 插件管理」，支持导入、启停、编辑 JSON 设置、查看结果和删除。首次安装默认关闭；启用后打开谱面才产生谱面分析结果。

## 文件与安装

`.ppp` 是 ZIP，根目录必须恰好包含两个 UTF-8 文件：`manifest.json`、`main.rhai`。禁止额外目录和文件。包不超过 2MiB，每个解压条目不超过 2MiB；安装数最多 64，插件库总大小最多 16MiB。同一 ID 更新时先删除旧版本，再导入新包（删除会清除该插件设置）。脚本和设置保存在用户数据目录的 `plugins/index.json`，不是原 ZIP。

Manifest 示例见 [density/manifest.json](examples/density/manifest.json)。必填 `api`（当前为 1）、`id`、`name`、`version`；可选 `author`、`description`、`permissions`、`settings`。ID 仅允许 ASCII 字母、数字、点、下划线、连字符，最多 80 字节；名称非空、最多 160 字节；版本最多 80 字节，说明最多 2048 字节。`settings` 必须是 JSON 对象；管理页编辑保存的设置上限为 16KiB。未知 API 或权限拒绝安装。声明权限不会获得文件或网络访问能力。

打包示例（在仓库根目录，Python 标准库）：

```python
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
source = Path("docs/plugins/examples/density")
with ZipFile("density.ppp", "w", ZIP_DEFLATED) as archive:
    for name in ("manifest.json", "main.rhai"):
        archive.write(source / name, name)
```

## 回调

函数都可省略。必须有两个参数 `(ctx, settings)`，返回可转 JSON 的数据；`settings` 为管理页保存的对象。函数调用之间不保证脚本全局变量保存状态。刷新或修改插件会重新加载并清除当前分析结果。分析结果只保存在运行内存，后一个谱面／结算回调覆盖前一个结果。

| 函数 | 所需权限 | 调用时机与参数 |
| --- | --- | --- |
| `on_chart(ctx, settings)` | `chart.read` | GameScene 加载谱面后，包括回放及练习；参数如下 |
| `on_result(ctx, settings)` | `result.read` | 正常结算保存本地记录时；参数为 SimpleRecord，练习及被现有计分流程排除的局不调用 |
| `on_frame(ctx, settings)` | `overlay.draw` | 主应用绘制阶段，每秒最多 5 次；`time` 为应用计时秒数、`fps` 为当前采样帧率 |

`on_chart` 参数：

- `api`：1；`name`、`level`、`difficulty`：谱面信息；`speed`、`flow_speed`：本局设置。
- `note_count`：非假音符总数；`notes`：按头部时间升序排列，最多 50,000 项；`truncated` 明确指出是否截断。分析算法必须检查此标志，不能把截断结果当完整谱面。
- 每个音符含 `line`、`index`（原始判定线／音符索引，从 0 开始）、`time`、`end_time`（谱面秒，未加音频偏移，非 Hold 等于 time）、`kind`（`tap` / `drag` / `flick` / `hold`）、`above`、`speed`。
- `raw`：解析原谱面 JSON；原数据超过 4MiB 或并非 JSON 时为 JSON null（Rhai 中通常为 unit）。它保留编辑器字段，需自行检测谱面格式。标准化 notes 暂不提供逐时刻坐标、判定线动画、噪域求值接口。

`on_result` 参数采用 camelCase：`score`、`accuracy`（0～1）、`fullCombo`、`maxCombo`、`numOfNotes`、`counts`（PERFECT 含 Perfect+ / GOOD / BAD / MISS）、`gradeCounts`、`grading`、`hist`、`std`（秒）。可选字段可能为 null；`gradeCounts` 索引为 PERFECT=0、GOOD=1、BAD=2、MISS=3、PERFECT+=4、GREAT=5、OK=6、MEH=7。

`on_chart` / `on_result` 返回对象由管理页「详情」显示。可约定 `{ algorithm: "ppsr", difficulty: 12.34, text: "…" }`：这些字段供作者组织结果，核心不会将其写入官服定数、PB、RKS 或上传内容。PPSR 的实际算法由算法作者实现，本版本不包含 PPSR，也没有自动替换内置难度的注册接口。

## 个性化显示

有 `overlay.draw` 权限时，返回对象可包含 `hud` 数组；`on_frame` 则直接返回这个数组。每项 `{ text, x, y, color }`，最多绘制 8 项，每项最多 160 字符。`x` 是横向归一化坐标、`y` 是相对半屏高度坐标，均夹到 −0.9～+0.9；原点居中，向右／向下为正。`color` 为整数 `0xRRGGBB`，默认白色。使用固定字号 0.4；显示位于当前画面上层，不接收触摸。`on_frame` 返回值会覆盖当前结果中的 `hud`；不应假设它能读取上一次 `on_chart` 返回值。

示例 [main.rhai](examples/density/main.rhai) 包含可执行的密度统计、结算分析与 FPS 显示。密度是示例指标，不是 PPSR，也不声称反映真实谱面难度。

## 执行限制与错误

没有原生动态库、文件系统、网络、系统命令、账户数据、触摸注入或判定／成绩修改接口。禁用文件模块导入；`print` 和 `debug` 不输出。谱面和结算回调最多 5,000,000 个操作，帧回调最多 25,000；调用深度 32，表达式深度 64/32，变量／函数最多 256，字符串数据最多 8MiB，数组和 Map 各最多 1,000,000 项（Rhai 对嵌套容器递归累加计数），JSON 返回值最多 32KiB。操作计数不是墙钟时间保证；这些限制用于控制脚本开销，复杂算法应自行优化。

回调出错、超限或返回非 JSON 数据时，该插件暂停执行，管理页显示原因。修改设置、停用后再启用或刷新可重新加载。插件的文件读写与安装均使用应用的数据目录；插件本身不能访问该目录。开发时请在副本配置中验证耗时与错误行为。
