# Phira Pro 发行包许可与第三方声明

| 源码或包内路径 | 许可范围与声明 |
| --- | --- |
| 主程序及原生宿主 `rpen-host/` | Phira Pro 的 GPL-3.0-only 代码。随包提供 `LICENSE`，第三方依赖适用各自条款。 |
| `assets/rpen/` | 源自 RPEN 的播放器着色器预设和噪域皮肤。保留 `LICENSE-RPEN.txt` 与 `NOTICE.txt`，资源适用的额外条款见其来源声明。 |
| `assets/rpen-editor/` | 来自独立 RPEN fork 的生成目录。`LICENSE` 是 PolyForm Noncommercial 1.0.0，`NOTICE` 列出目录范围、上游声明及第三方条款。上游提交见 `UPSTREAM.json`。 |
| `assets/rpen-editor/pro/` | Phira Pro 的 GPL-3.0-only 集成贡献。目录内有 `LICENSE` 和 `NOTICE`。引用的 RPEN 模块适用其上游条款。 |
| `scripts/editor/overlay/` | 两个编辑器 overlay 模块中的 Phira Pro 原创贡献按 GPL-3.0-only 标识。生成目录内见 `assets/rpen-editor/src/ui/LICENSE-Phira-Pro` 和 `NOTICE-Phira-Pro`。保留的 `mod-runtime.upstream.mjs` 继续适用上游条款。 |

RPEN 随附的第三方代码、皮肤、字体和模型保留各自的许可证及署名。相关文件随组件分发，入口见编辑器 `NOTICE`。PolyForm 的用途条款适用于由它授权的材料。

发布二进制时应在下载处提供该版本的 GPL 对应源码及构建脚本。Android 随包的 FFmpeg 使用 `--enable-gpl` 和 libx264 构建，其对应源码与构建配置也应随该版本一并提供。
