# 谱面渲染扩展约定

入口为 `prpr::chart_video::extensions`，接口版本 `RENDER_API_VERSION = 1`。这是由本体直接装配的 Rust 原生接口，尚未接入现有 Rhai Plugin API；安装普通插件不会自动获得这些回调。

## 装配与所有权

`ChartVideoRenderer::new_with_extensions(source, options, output, extensions)` 接受每次导出独有的 `Vec<Box<dyn RenderExtension>>`。扩展按传入顺序执行，状态随会话销毁，不注册进程级全局对象。

`PresentationOptions.effects` 保存带命名空间的 `EffectSpec { id, version, enabled, settings }`。例如 `example.watermark` 可以存放专用参数；核心保留未识别的配置，不会自行执行它们。未来的本体工厂或插件适配器负责检查版本、筛选启用项并构造实例。

## 生命周期

| 回调 | 时机与用途 |
| --- | --- |
| `id` | 稳定的扩展标识，用于错误上下文 |
| `configure` | 载入前修改独立 `VideoOptions`；之后统一校验并建立时间轴 |
| `prepare` | 时间轴与只读曲绘、背景、头像、评级图标及可选演出字体就绪后准备资源 |
| `audio` | 渲染前处理混合音轨，按视频时间顺序提供 44100 Hz 双声道浮点样本块及首样本编号 |
| `stage_enter` | 即将首次绘制一个阶段时调用，包括开场、入场、游玩、退场、结算 |
| `update` | 每帧返回行为参数，当前提供透明度和游玩画面色调；参数每帧复位 |
| `draw` | 每帧按 Background、BeforeScene、AfterScene、AfterUi 顺序调用 |
| `finish` | 完成、取消或失败时通知一次，供扩展释放外部资源 |

加载失败或载入 future 被销毁时以 Failed 结束；已经建好的会话被主动取消或销毁时以 Cancelled 结束；成功编码并校验输出后以 Completed 结束。重复清理不会重复通知。扩展应在 `finish` 中区分三种结果，避免把失败当成成功发布。

当前 Background 与 BeforeScene 位于背景之后、主画面之前，AfterScene 与 AfterUi 位于包含 HUD 的主画面之后。保留四个语义层以便未来分别拆出场景和 HUD；当前不要依赖同一侧两个层之间存在额外本体绘制。

`FrameContext` 同时提供视频总时间、阶段内时间、归一化进度、谱面音乐时间、帧号、固定步长、输出尺寸与预览标记。开场和结算没有谱面音乐时间。修改时长必须在 `configure` 完成，逐帧回调不回写时间轴。

## 错误边界与限制

回调返回错误会终止该次导出，携带扩展标识显示在渲染页，并按失败清理半成品。每个绘制回调位于独立 UI scope，返回后恢复本体相机。扩展须自行释放自建纹理、材质、临时音频和其他资源；不得在回调中阻塞网络或调用账号、成绩和真实游玩状态。

该接口运行于渲染线程，不是原生代码沙箱。未来接入脚本或第三方插件时，还需要权限、参数 schema、资源加载、执行预算和适配层；当前没有提供动态原生库加载或任意脚本入口。未来 shader、额外图层、视觉行为与音频效果可通过上述边界接入，而无需修改导出页面或编码器。
