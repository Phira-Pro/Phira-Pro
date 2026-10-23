# Phira Pro 插件模型输入／输出规范

本文定义通用模型插件与 Phira Pro Plugin API v2 的适配边界。适用于模型输入特征与 PPSR 不同、需要在插件中自行提取特征的算法。目标是统一**宿主回调输入、模型资产约定和预测报告输出**，而不是强制所有算法使用相同的特征或推理模型。

## 1. 总体结构

一个模型插件由以下部分组成：

| 部分 | 位置 | 职责 |
| --- | --- | --- |
| 插件声明 | `manifest.json` | 插件身份、API 版本、权限及固定模型资源声明 |
| 适配器／特征提取 | `main.rhai` | 检查回调输入，解析谱面，计算并校验模型特征，执行推理，组织报告 |
| 模型与算法参数 | `model.json` 等 `assetFiles` | 权重、树、归一化参数、校准参数及特征 schema 等只读资源 |
| 报告 | `on_chart_selected` 返回值 | 总难度、可选分项、说明和 HUD 文本 |

常规模型插件应自行从 `ctx.chart_data` 提取特征。不同插件可使用各自的音符解释、特征名、特征数量、预处理及模型算法；**PPSR 的 187 维兼容特征不是通用输入规范**。

宿主不提供通用模型运行器，也不解释任意模型文件。模型资产的 `format`、字段和推理公式由每个算法定义，并由其插件脚本读取执行。上述约定只标准化资源管理和回调 I/O。

## 2. 插件声明及资源

选谱前分析需声明 API v2 和 `chart.read`。要在详情页显示可拖动、可折叠的模型难度面板，还需 `overlay.draw`：

```json
{
  "api": 2,
  "id": "example.rhythm-model",
  "name": "Rhythm Model",
  "version": "1.0.0",
  "permissions": ["chart.read", "overlay.draw"],
  "settings": {},
  "assets": {},
  "assetFiles": {
    "model": "model.json"
  }
}
```

资源文件必须是 ZIP 根目录的 JSON 文件，并由 `assetFiles` 声明。宿主导入时解析文件并合并至插件 `assets`；回调可通过 `ctx.assets.model` 读取。资源随插件固定安装，脚本无权在运行时改写或保存。模型更新应与该模型的特征 schema、归一化、分项画像及校准参数一起重新打包。

```json
{
  "format": "example.rhythm-model",
  "version": 1,
  "modelId": "rhythm-model-2026-01",
  "featureSchema": {
    "id": "example.rhythm-model.features",
    "version": 3,
    "count": 2,
    "names": ["tap_nps", "hold_ratio"]
  },
  "predictor": {
    "type": "plugin-defined",
    "weights": [1.2, 0.8]
  }
}
```

上例仅展示资源的元数据组织方式；`predictor` 是算法私有内容，不是宿主实现的通用预测器。模型替换工具应检查 `format`、主版本、算法版本及特征 schema 是否兼容。

### 安装与资源上限

- ZIP 最大 2 MiB；全部解压文件合计最大 10 MiB。
- 单个 JSON 资源最大 8 MiB；合并后的插件 `assets` 最大 8 MiB。
- ZIP 只允许 `manifest.json`、`main.rhai` 和声明的根目录 JSON 资源，不允许子目录或未声明条目。
- `settings` 必须是 JSON 对象，最大 16 KiB；单次回调返回 JSON 最大 32 KiB。
- 同 ID 更新需先删除旧插件再导入；删除也会清除该插件设置。

## 3. 宿主回调输入

### 3.1 `on_chart_selected(ctx, settings)`：选谱时预测

API v2 在用户选择**本机可读取**的谱面后、进入游戏前调用此回调。除宿主字段外，`ctx.assets` 自动附加插件声明的资源。

通用字段：

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `api` | integer | 调用该回调的插件 API 版本（v2 为 `2`） |
| `name` | string | 谱面名称 |
| `level` | string | 谱面等级文本 |
| `difficulty` | number | 谱面来源中的原始定数；不保证每种 chart set 都有有效值 |
| `format` | string 或 null | `rpe`、`pec`、`pgr`、`pbc` 之一 |
| `chart_set` | string | 内置章节谱为 `phigros`，其他本地谱为 `phira` |
| `chart_data` | object 或 null | 不超过 4 MiB 的谱面源文件解析出的 JSON 对象 |
| `assets` | object | Manifest 的固定 assets，包括 `assetFiles` 加载的 JSON |
| `operation_limit` | integer | 当前非帧 Rhai 回调操作预算；可在插件管理页调整。 |

`chart_data` 在源文件超过 4 MiB、不是合法 JSON、或 JSON 顶层不是对象时为 null。算法不支持的格式、空数据和缺字段必须显式处理，不能将缺失数据静默补成看似有效的预测。

脚本自行解析并计算特征。推荐在插件内部建立以下**逻辑输入对象**，以便把源格式解析和模型推理解耦；此对象由插件构造，不是宿主字段：

```json
{
  "schema": "phira-plugin-model-input",
  "version": 1,
  "plugin_id": "example.rhythm-model",
  "model_id": "rhythm-model-2026-01",
  "feature_schema": "example.rhythm-model.features",
  "feature_version": 3,
  "feature_names": ["tap_nps", "hold_ratio"],
  "features": [4.25, 0.18]
}
```

适配器必须保证 `feature_names[i]` 与 `features[i]` 的语义、单位和训练时完全一致。所有值必须是有限数值。维度、顺序或版本不匹配时应明确返回错误，不要排序特征、按名称静默补零或将 NaN／无穷值传进模型。若模型需要标准化、裁剪、缺失值处理或类别编码，这些变换及其顺序必须与训练流程一致，并与模型版本一起固定。

`main.rhai` 回调结构可参考下例。`extract_features` 和 `predict` 是算法自己的实现，不是宿主内置函数：

```rhai
fn on_chart_selected(ctx, settings) {
    if ctx.chart_data == () || type_of(ctx.chart_data) != "map" {
        return #{
            algorithm: "example.rhythm-model",
            title: "Rhythm Model",
            failed: true,
            text: "缺少支持的 JSON 谱面数据"
        };
    }

    let model = ctx.assets.model;
    if model.format != "example.rhythm-model" || model.version != 1 {
        return #{
            algorithm: "example.rhythm-model",
            failed: true,
            text: "模型格式或版本不兼容"
        };
    }

    let names = model.featureSchema.names;
    let features = extract_features(ctx.chart_data);
    if features.len() != names.len() {
        return #{
            algorithm: "example.rhythm-model",
            failed: true,
            text: "提取的特征数量与模型不匹配"
        };
    }
    let difficulty = predict(model, features);
    #{
        algorithm: "example.rhythm-model",
        title: "Rhythm Model",
        model_id: model.modelId,
        difficulty: difficulty,
        parts: [],
        text: "谱面难度分析完成"
    }
}
```

### 3.2 PPSR 兼容输入（非通用字段）

为兼容既有 `phira-pro.ppsr` 插件，当宿主确认这个插件需要分析时，宿主会额外提供 `feature_version`、`v2_feature_version`、`feature_names`、`features`、`note_total`、`mech_total`、`duration` 和 `fake_ratio`。其中 `features` 是特定 PPSR 算法的 187 维输入。其他模型**不应依赖这些字段**；将 PPSR 特征复制到其他模型并不构成通用适配。

### 3.3 `on_chart(ctx, settings)`：进入游戏后

此回调在谱面加载后运行，包括游玩、练习和回放流程。其 `raw` 同样受 4 MiB 原始 JSON 限制；`notes` 是最多 50,000 项的标准化摘要，超过上限时 `truncated: true`。摘要不含源谱面的完整坐标、判定线动画等特征。若算法需要完整源数据，请使用 v2 的选谱回调；不要把截断的 `notes` 当作完整特征输入。

### 3.4 其他回调

- `on_result(ctx, settings)`：正常保存本地结算记录时提供 `SimpleRecord`，需 `result.read` 权限；不是选谱难度模型的输入。
- `on_frame(ctx, settings)`：提供应用 `time` 和 `fps`，需 `overlay.draw` 权限；最多每秒 5 次调用，适合显示轻量 HUD，不应在此反复运行整谱推理。
- 所有回调都必须恰好声明两个参数 `(ctx, settings)`。插件函数间不保证全局变量状态持久。

## 4. 模型推理与可复现性

`main.rhai` 是特征提取与预测适配器的权威实现。要让第三方模型可替换且结果可复现：

1. 为模型声明稳定 `modelId`、模型文件 `format` 和单调递增的 `version`。
2. 定义带版本的 `feature_schema`，保存特征数量、精确名称顺序、单位、取值定义及必要的预处理参数。
3. 将预测公式、树遍历方向、缺失值规则、集成平均方式、偏置／校准顺序及输出单位写入模型规范；不能依赖宿主猜测模型类型。
4. 对任一不匹配项返回明确失败，不要继续以零向量或截断输入进行推理。
5. 增加黄金测试向量：固定完整特征向量、期望中间值及最终预测值，验证不同运行环境的推理一致性。
6. 对超长或超密谱面估算脚本成本；若输入将超出操作预算，应在昂贵步骤前给出可读拒绝结果。

宿主 Rhai 每次非帧回调的默认操作预算为 **5,000,000 次**，用户可在插件管理页点击当前上限修改。仅接受 1 至 `i64::MAX` 的 ASCII 正整数字符串；空值、负数、小数、非数字、零或超出可支持范围的值均回退到默认值 5,000,000。设置保存在插件目录的独立 `operation_limit.json` 中，不改变插件索引或插件包。`on_frame` 始终固定为 **25,000 次**，不受此设置影响。模型推理和整个特征提取共用一次回调预算，不会因为它们分成多个函数就增加额度。操作超限、抛出异常或返回非 JSON 值会暂停该插件，需在插件管理页刷新或重新启用恢复。Rhai 预算不是墙钟超时保证；复杂的每音符全量扫描、重复模型推理和高复杂度循环都可能使长谱面无法分析。PDE-R 示例通过 `ctx.operation_limit` 按当前预算预先估算谱面成本。

## 5. 标准预测输出

`on_chart_selected` 必须返回可序列化 JSON。通用建议对象：

```json
{
  "algorithm": "example.rhythm-model",
  "title": "Rhythm Model",
  "model_id": "rhythm-model-2026-01",
  "difficulty": 12.34,
  "unit": "定数",
  "parts": [
    {
      "key": "density",
      "label": "物量密度",
      "score": 7.2,
      "unit": "/10",
      "value": 5.8,
      "value_unit": "notes/s"
    }
  ],
  "text": "预测定数 12.34",
  "hud": []
}
```

字段规则：

| 字段 | 必需 | 含义与要求 |
| --- | --- | --- |
| `difficulty` | 是（供共用难度面板识别） | 有限 JSON number；插件负责声明定数尺度和意义 |
| `title` | 否 | 算法显示名。共用面板标题优先用此字段，否则使用插件 Manifest 的 `name` |
| `algorithm` | 推荐 | 算法／模型族标识 |
| `model_id` | 推荐 | 与打包模型资产一致的精确版本标识，便于报告追溯 |
| `unit` | 否 | 总难度单位说明，宿主不换算或校准该值 |
| `parts` | 否 | 分项数组；面板最多绘制前 10 项 |
| `text` | 否 | 插件详情中的简短说明 |
| `hud` | 否 | `overlay.draw` HUD 项数组；帧回调也可直接返回数组 |

每个 `parts` 行至少应包含 `label`（可读名称）和有限数值 `score`。推荐还包含稳定的 `key`、`unit`、`value`、`value_unit`，方便报告消费方解释该分项的统计量。共用难度面板将 `score` 限制到 1–10 显示为进度条；不在此范围的模型应先自行归一化，或在说明中告知真实尺度。若算法没有可信的分项模型，不要伪造分项；省略 `parts` 即可。

错误时建议返回非误导性的 JSON 结果，让用户看见原因，而不是伪造成功难度：

```json
{
  "algorithm": "example.rhythm-model",
  "title": "Rhythm Model",
  "failed": true,
  "text": "该算法不支持此谱面格式",
  "hud": []
}
```

这种业务级拒绝是正常返回，插件不会因其暂停。不要用宽泛的成功值（例如零难度、空特征向量或默认模型输出）掩盖格式错误、超大谱面或模型版本不兼容。

### HUD 约定

`hud` 最多 8 项，每项 `{ "text": string, "x": number, "y": number, "color": integer }`；文本最多 160 字符。坐标由宿主限制在 −0.9 到 +0.9；颜色为 `0xRRGGBB`。HUD 仅绘制文本，不接收触摸，也不能代替详情页的难度报告。

## 6. 面板与插件并存

- 任一 API v2 插件同时声明 `chart.read`、`overlay.draw`，实现 `on_chart_selected`，并返回有效数值 `difficulty`，即可进入共用选谱难度面板。
- 当前只显示一个插件的面板结果：若有效 PPSR 插件存在，PPSR 优先；否则选择第一个有效的合格插件。其他插件报告仍可在插件管理页的详情中查看。
- 详情页结果保存在内存；后续插件回调可能覆盖该插件报告。面板仅在有符合条件的选谱报告时出现。
- 插件不能改变游戏内置定数、成绩、PB、RKS 或网络数据；预测结果仅供插件显示。

## 7. 实施检查清单

- [ ] Manifest 声明 `api: 2`、`chart.read`；需要面板时再声明 `overlay.draw`。
- [ ] 检查 `chart_data` 是否为 JSON 对象，并显式拒绝不支持的格式／缺失字段。
- [ ] 特征名、顺序、维度、版本、单位和归一化与模型元数据一致。
- [ ] 确认全部特征值和预测值有限；任何维度不匹配都不静默补零。
- [ ] 长谱面特征提取和模型推理的合计 Rhai 操作数保持在用户配置预算内（默认 5,000,000）。
- [ ] 有限且清晰的错误报告，不返回伪成功结果。
- [ ] 返回有限 `difficulty`；有意义的分项才放入 `parts`。
- [ ] 单元测试覆盖特征黄金样本、模型黄金向量、错误输入、长谱面预算及 ZIP 导入。
- [ ] ZIP、assets 和回调输出均符合宿主大小上限。

## 8. 本仓库参考实现

- [PDE-R 插件](./examples/pde-r/)：脚本自有 106 维特征提取、模型资产及独立模型推理。
- [PPSR 示例](./examples/ppsr/)：既有 187 维宿主特征兼容路径；其输入不是通用模型约定。
- [插件 API 总览](./README.md)：Manifest、回调及插件能力完整说明。
- [PPSR 专用模型 I/O](./ppsr-model-io.md)：PPSR v2 的精确特征顺序、模型树格式与画像约定。
