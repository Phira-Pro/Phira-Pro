# Phira-Pro 正优化详细文档（P-01 至 P-07）

> **主线实施状态总则：本文所有条目在 Phira-Pro `main` 中均未实施（P-05 的 bbox 是另一项，已在 main `78445c20`；tiles 未实施）。**
>
> “是否已实施”只指 **Phira-Pro `main` 是否已经合入并完成验收**。本地存在参考实现、参考实现能编译、参考实现默认启用、真机跑过对照，都**不等于**主线已实施。只有合入 `main`、完成本文第 11 节的落地步骤和第 7 节的质量验收后，才能把条目顶部的 `☐ 未实施（main）` 改为 `☑ 已实施（main）`。

## 正优化总状态表

| ID | 优化项 | 分类 | 主线是否已实施 | 参考实现状态 | 参考实现默认 | 一句话结论 |
| --- | --- | --- | --- | --- | --- | --- |
| P-01 | 谱面预编译 + 判定线 GPU IR | 正优化（局部） | **否** | 完整，已接真实消费者 | 关闭（`Config.chart_pre_compile=false`） | 阶段墙钟 −11.15%/−12.79%，FPS 不变 |
| P-02 | 谱面尾工作集 | 正优化（局部） | **否** | 完整，随 P-01 开关 | 关闭 | 25 秒窗口 app wall −22.83%/−30.64%，chart stage −49%～−61%，FPS 不变 |
| P-03 | 通用字体图集池 | 正优化（整帧） | **否** | 完整，Android 普通构建默认接；桌面未转发 | 开启（Android；桌面未转发） | 普通 FPS 46.11→57.92，app wall −12.08%；低画质 −10.65% |
| P-04 | 透明判定线 DCE | 正优化（整帧，仅普通画质） | **否** | 完整，参考实现默认开 | 开启 | 普通画质 −7.52%，FPS 57.93→59.76；低画质无收益 |
| P-05 | block 分区裁剪 tiles | 正优化（局部/条件性） | **否** | tiles 完整，仅研究开关（bbox 为另一项，已在 main） | 关闭（tiles；普通路径只有 bbox） | Petrichor 105–111s −10.5%，其他窗口无收益 |
| P-06 | 图集增量版本 | 正优化（局部） | **否** | 完整，研究候选 | 关闭 | 上传 44.34×，HUD 尖峰下降，FPS 持平 |
| P-07 | Chart JSON 分片加载 | 正优化（局部） | **否** | 研究后端 | 关闭 | Loading render max 约 −23%，全局 FPS/p99 无改善 |

## 标记规则

1. 条目顶部只能使用两种状态：`☐ 未实施（main）` 或 `☑ 已实施（main）`。
2. 合入 `main` 后，必须同时填写：实施日期、实施提交、实施人、验收人、验收证据位置。
3. 只改代码、没有完成质量验收，不得勾选；只能写“已合入 main，验收未完成”。
4. 参考实现状态（完整/原型/默认开关）单独记录，不能替代“主线是否已实施”。

---
## 第一部分：P-01 至 P-03

本文档只记录已经在本项目参考实现上实际落地、并做过同 SO 对照的三项正优化。三项都属于“减少运行时工作”而不是“关闭效果”：谱面判定线被预先降低为 GPU 顶点/索引流，普通音符尾部被预先证明为屏幕外并跳过遍历，字体图集被改成多版本增量发布。三项的默认值、文档冲突和未验证边界在后文逐项写明。

阅读约定：

- “应用墙钟”指 `update + render + flush` 的 CPU/driver 墙钟，包含驱动执行、调度与等待，不包含 swap，也不等于 GPU 独占耗时。
- “呈现间隔”指 SurfaceFlinger 的真实呈现时间戳间隔，和“应用墙钟”不是同一个口径。
- “同 SO”指同一份 `.so`；ABBA 指同一轮内按 A-B-B-A 或 A-B-B-A-A-B-B-A 交错，避免把时间漂移当成优化收益。
- 除非特别注明，设备均为 `8DF6R17309002674`：EDI-AL10 / hi3650 / Android 8 / arm64-v8a，Mali-T880 / OpenGL ES 3.2 v1.r20p0，画面 2560×1440（3,686,400 像素）。
- 测试谱面为 `What do you want more than a Happy ending.zip`，AT.json / PGR v3，24 线、1568 notes、1349 普通非 hold、219 holds；使用 autoplay，不代表手动输入验收。
- “参考实现已落地”指参考实现已完成提交与本地验证；三项都没有进入 `main`，也没有推送主项目。

---

## P-01 谱面预编译 + 判定线 GPU IR

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-07（2c31be7f、9913d98b、9498c734）　参考实现提交：2c31be7f、9913d98b、9498c734　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计，不冒充独立验收）

### 1. 一句话结论

判定线 GPU IR 把每帧逐线生成的四边形、索引与颜色，提前编译成不可变 VBO/IBO 和按 slot 组织的 pose/RGBA uniform；在低画质、默认材质、非调试、单采样条件下，同一有限窗口内把 `update+render+flush` 均值从 **6.2263 ms 降到 5.5321 ms（−11.15%）**，研究后端另一组同 SO 八轮从 **6.280 ms 降到 5.477 ms（−12.79%）**，managed native draw 从 **38.333 次/帧降到 17.333 次/帧**。它不是整帧 FPS 提升（两组都贴着约 59.7 FPS 上限），而是局部提交成本下降；画面归类为**低画质轻量有损候选**，不是无损。

### 2. 问题与瓶颈（为什么原路径慢）

原路径每帧对每条普通判定线执行 `draw_line(-len, 0., len, 0., width, color)`。这条路径每帧都要：

1. 根据当前 `len` 和线宽（`0.01` 或 `0.0076`）重新计算四边形的四个角；
2. 把 `Color` 转成 `Vertex` 的 RGBA8；
3. 把四边形写入 macroquad 的动态顶点缓冲，并生成 6 个索引；
4. 在 `quad_gl` 队列里做一次状态提交与绘制；
5. 对每条线重复上述矩阵、颜色和批次判断。

这条谱面只有 24 条线，但判定线是**每帧都画**的，且与音符的全局材质批处理不同：线的颜色、宽度、模型矩阵来自当前 `JudgeLine` 状态，不能像静态音符那样一次合批。实测低画质下 managed native draw 平均 **38.333 次/帧**，其中 21 个 6-index 与 2 个 12-index 的白色 geometry 调用提示“线的状态提交”占有可见份额；白纹理本身不能证明调用来源，但和源码逐线绘制一致。原始路径的瓶颈不是“几何复杂”，而是**每帧重复的 CPU 顶点生成、动态上传与逐线提交**。

另一个关键点是：这条路径的几何其实**不随帧变化**。对固定谱面、固定线长、固定 palette 容量，四边形的 x/y 与索引完全由 `len` 和线宽决定；每帧变化的只有模型矩阵、颜色和投影。把不变部分编译掉，只保留每帧的 pose/颜色，是这项优化的出发点。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

**不变量一：几何只依赖 `len`、线宽和 slot。** 原 `draw_line` 的四个角是 `(-len, ±width)` 与 `(len, ±width)`；源码用 `y(thickness) = dx / ((dx*dx).sqrt() / (thickness*0.5))` 保留原 f32 运算次序，而不是把 `thickness` 直接当半宽。`quad()` 把宽、窄两种 y 都写进同一个顶点，wide 放 `params.y`，thin 放 `params.z`，因此同一份几何可以服务 `line_scaled=false/true` 两种线宽。

**不变量二：颜色量化与原路径相同。** 原路径把 `Color` 写进 `Vertex` 时已经做 `(v * 255.) as u8`。GPU IR 在 `try_render()` 里做同样的 `(v * 255.) as u8 as f32`，然后把整数 alpha 的符号当作宽度选择位：宽线存 `+alpha`，窄线存 `-alpha-1`。0..255 的 alpha 与 `-1..-256` 在 f32 中都能精确表示，shader 用 `thin = c.a < 0.` 判断，再 `c.a = -c.a - 1.` 还原；没有额外的颜色量化。

**不变量三：透明顺序不变。** 编译后的 `ids` 保持 `chart.order` 中普通线的原始顺序；运行时的 `Record.start = i*6` 和 span 只合并**相邻且连续**的 record。遇到页变化、capture 变化、非普通线、CPU 回退、debug、非默认材质、深度测试、capture 开启、非有限矩阵/颜色、`len` 变化，立即 `flush()` 并把当前线交回原 `draw_line`。因此不会为了合批而跨透明对象重排。

**不变量四：判定、音频、pose 仍由原 Chart 拥有。** IR 只包含线的几何、slot、源 line ID 和静态长度；每帧 pose 仍从 `quad_gl.get_model_matrix()` 读取，投影仍从 `get_projection_matrix()` 读取；`JudgeLine::render` 的判定、`CtrlObject`、音符、音频完全没被替换。

**为什么可能不是逐像素相同：** shader 里的 `Projection * model * vec4(params.x, y, 0., 1.)` 和 `c/255.` 是 GPU f32 求值，原路径是 CPU 端生成顶点后由固定管线/原 shader 求值；驱动可能使用不同的精度、FMA 或 FTZ。源码因此把质量类写成 `mild-lossy`，并用有限画面的 RGBA 对照来限定结论，而不是声称无损。画面“看起来相同”来自同一几何、同一颜色整数、同一投影/模型、同一混合顺序；不能从公式相同直接推出所有 GPU 逐像素相同。

### 4. 具体实现

#### 4.1 文件与符号清单（含行数、函数/结构名）

| 文件（参考实现，head `be4e1305`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/render_compiler.rs` | 173 | `CACHE`（33）、`configure_cache`（52）、`configure_cache_with_paths`（55）、`prepare`（90）、`report`（84） | 进程级唯一 cache、加载期编译入口、报告 |
| `prpr/src/render_lab/lines.rs` | 859 | `CacheLease`（21）、`Mesh`（45）、`supported_ids`（53）、`cache_key`（61）、`quad`（74）、`domain`（96）、`compile`（104）、`encode`（123）、`decode`（147）、`capability`（191）、`install`（316）、`Renderer`（256）、`flush`（420）、`queue_loading_sample`（454）、`warm_loading`（484）、`begin_chart`（561）、`before_line`（583）、`end_chart`（600）、`try_render`（607）、`Drop`（710） | 判定线 GPU IR 的编译、序列化、安装、运行、退休 |
| `prpr/src/render_lab/cache.rs` | 350 | `MAX_CACHE_BYTES`（16）、`Ticket`（18）、`RenderCache`（27）、`begin`（98）、`read_bounded`（119）、`publish`（153）、`invalidate_blob`（182）、`clear_all`（196） | 代次/票据、256 MiB 读取预算、原子发布、统一清理 |
| `prpr/src/core/chart.rs` | — | `compiled_lines`（39）、`render_scoped`（263）、`render_inner`（267） | Chart 拥有 renderer 状态；加载作用域与公开 render 分离 |
| `prpr/src/core/line.rs` | — | `render`（298）、`render_inner`（322）、`try_render` 调用（414） | 原线路径保留；命中 IR 时提前返回，否则原 `draw_line` |
| `prpr/src/scene/game.rs` | — | 加载期 `workset::prepare`（718-727）、`render_compiler::prepare`（731-739）、`chart_render_scope`（550-566）、`Chart::render_scoped`（2003-2008） | 真实生产消费者与生命周期 |
| `prpr/src/config.rs` | 829 | `chart_pre_compile`（164）、默认 `false`（295） | 独立运行开关 |
| `prpr/Cargo.toml` | 136 | `chart_pre_compile = []`（13）、`default = ["log", "font_atlas_pool"]`（18） | feature 定义；prpr 自身默认不编入 |
| `phira/Cargo.toml` | 127 | `chart_pre_compile`（14）、`default = ["video", "font_atlas_pool", "chart_pre_compile"]`（18） | 主 crate 默认编入 |
| `phira-main/Cargo.toml` | — | `default = ["video", "chart_pre_compile"]`（8）、`chart_pre_compile = ["phira/chart_pre_compile"]`（10） | 桌面主程序转发 |

#### 4.2 数据结构与算法（逐步说明）

1. **能力探测**：`capability(false, res)` 先要求 `res.config.sample_count == 1` 且 `!res.config.chart_debug`；再读 `GL_MAX_VERTEX_UNIFORM_VECTORS`（`0x8dfb`）和 `GL_MAX_VERTEX_ATTRIBS`（`0x8869`），要求无 GL 错误、vectors≥9、attributes≥1，palette 容量取 `((vectors-4)/5).min(32)`。四个 pose 数组加一个 RGBA 数组，每个 slot 占 5 个 vec4；减 4 是给 Projection 等留余量。
2. **key 计算**：`cache_key(source, length, palette, ids)` 把 ABI 字符串、源 ZIP 的 SHA-256、`length.to_bits()`、palette、按源顺序的普通线 ID 全部哈希。源 ZIP、线长、容量、线集合任一变化都会换 key。
3. **读缓存**：`manager.begin(&key)` 返回带 epoch 的 ticket；`read` 走 `read_bounded(256 MiB)`。命中后 `decode` 逐项校验 magic、ABI、source、palette、length、IDs、总长度、checksum，并**重新计算每个顶点和索引**与期望值逐位比较；不是只信 checksum。损坏/截断/旧源都会 `invalidate_blob` 后回退重编译。
4. **冷编译**：`compile()` 生成 `Mesh { source, length, palette, ids, vertices, indices }`。每条线 4 个 `Param`（`[f32;4]`），索引固定 `[b, b+1, b+2, b+2, b+1, b+3]`；`slot = i % palette`，所以同一页最多 32 条线，超出后换页。
5. **双重校验后发布**：冷编译后先 `encode()` 再 `decode()` 一次，确保刚生成的内存表示和落盘表示一致；然后 `manager.publish(&ticket, &blob)`。发布在锁内检查 `enabled && epoch == ticket.epoch`，先写同目录临时文件、`sync_all`，再原子 rename；已有有效 blob 时删除临时文件，不覆盖。
6. **安装**：`install()` 再检查低画质、`supported_ids(chart) == mesh.ids`、`length.to_bits()` 一致、lease 仍当前；创建 shader、pipeline、每页 immutable VBO、immutable IBO、1×1 白纹理。shader 的 `params` attribute 必须被驱动分配到 location 0，否则删除 shader 并回退；pipeline 使用 `SourceAlpha / OneMinusSourceAlpha` 混合，和原线一致。
7. **每帧开始**：`begin_chart()` 在 Chart 的线循环前调用一次；检查 lease/context，失效则丢弃 renderer 并写 fallback 状态；否则 `flush()` 上一帧残留、`cursor=0`、`frame_valid=true`、`frames+=1`。
8. **逐线记录**：`before_line()` 校验当前线 ID 是否等于 `chart.order[cursor]`；不等则 `frame_valid=false`。`cursor+=1`。任何 debug/非普通线/record 缺失都触发 `flush()`，把待提交 span 先落地，保证后续线回到原路径时顺序不变。
9. **try_render**：读取当前 model/projection/pass/viewport/scissor；检查 record、长度、采样数、debug、默认材质、capture、深度、有限矩阵/颜色。全部通过后，把 `gl.quad_gl.texture(None)`、`draw_mode(Triangles)` 设成与原 `draw_line` 相同的逻辑状态；把 RGBA8 和符号化 alpha 写进当前 page 的 slot uniform；若 pending span 还能连续（同 page、start+count==record.start、capture 相同）就 `count += 6`，否则先 `flush()` 再开新 span。
10. **flush**：把 projection 写进 page uniform 前 16 个 f32；把整个 page uniform 复制成 `Arc<[f32]>` 快照；保存并设置 render pass/viewport/scissor；调用 `quad_gl.resident_geometry()` 提交 immutable buffer + index + 1×1 texture + uniform；恢复旧 pass/viewport/scissor；累计 draws/uniform_bytes。
11. **退休**：`Renderer::drop` 不调用 GL，只把 pages/indices/texture/pipeline/shader 放进全局 `RETIRED` 队列。主线程已有的 texture retirement hook 调 `flush_pending_deletions()`，先 `gl.flush()`，再检查 context_id；同一 managed context 才 `clear_resource_bindings()` 并删除资源；跨 context 直接跳过，避免删除新 context 复用的同名 GL 对象。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

以下代码直接取自参考实现的 `be4e1305`。为了可读性只按主题分段，函数体未删行。

**加载入口 `prpr/src/render_compiler.rs:90-172`：**

```rust
pub fn prepare(chart: &Chart, res: &Resource, source_bytes: &[u8]) -> Value {
    lines::disable(chart);
    set_enabled(res.config.chart_pre_compile);
    reads::prepare(chart, res, source_bytes);
    visibility::prepare(chart, res, source_bytes);
    if !res.config.chart_pre_compile {
        let result = json!({"requested":false,"enabled":false,"installed":false,"cache_read":false,"cache_write":false});
        lines::record_preparation(chart, result.clone());
        return result;
    }
    let begun = Instant::now();
    let mut cache_read = false;
    let mut cache_write = false;
    let prepare = (|| -> Result<Value> {
        let palette = lines::capability(false, res)?;
        let manager = cache().context("Host has not configured the compiler cache")?;
        let source = Sha256::digest(source_bytes).into();
        let ids = lines::supported_ids(chart);
        let length = res.info.line_length;
        let key = lines::cache_key(source, length, palette, &ids);
        let ticket = manager.begin(&key)?.context("Compiler cache disabled")?;
        let mut cache_hit = false;
        let mut cache_error = Value::Null;
        cache_read = true;
        let cached = match manager.read(&ticket) {
            Ok(Some(bytes)) => match lines::decode(&bytes, source, length, palette, &ids) {
                Ok(mesh) => {
                    cache_hit = true;
                    Some((mesh, bytes.len()))
                }
                Err(error) => {
                    cache_error = json!(format!("{error:#}"));
                    manager.invalidate_blob(&ticket)?;
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                cache_error = json!(format!("{error:#}"));
                manager.invalidate_blob(&ticket)?;
                None
            }
        };
        let mut published = false;
        let (mesh, bytes) = match cached {
            Some(value) => value,
            None => {
                let mesh = lines::compile(source, length, palette, ids.clone())?;
                let blob = lines::encode(&mesh);
                let mesh = lines::decode(&blob, source, length, palette, &ids)?;
                cache_write = true;
                match manager.publish(&ticket, &blob) {
                    Ok(value) => published = value,
                    Err(error) => cache_error = json!(format!("{error:#}")),
                }
                (mesh, blob.len())
            }
        };
        ensure!(manager.is_current(&ticket), "Compiler work invalidated by clear/disable");
        lines::install(mesh, chart, res, lines::CacheLease::new(manager.clone(), ticket))?;
        let (warm, force) = warm_options();
        let loading_warm = if warm {
            lines::warm_loading(chart, force)?
        } else {
            json!({"requested":false})
        };
        Ok(json!({"requested":true,"enabled":true,"installed":true,"cache_hit":cache_hit,"published":published,
            "cache_error":cache_error,"key":key,"bytes":bytes,"root":manager.root(),"gpu_blob_persisted":cache_hit || published,"loading_warm":loading_warm,
            "quality_class":"mild-lossy; low quality required independently of compiler switch",
            "scope":"Actual immutable line GPU blob; original source owns judgement, audio and dynamic pose."}))
    })();
    let mut result = match prepare {
        Ok(report) => report,
        Err(error) => {
            lines::unavailable(chart, format!("{error:#}"));
            json!({"requested":true,"enabled":true,"installed":false,"fallback":format!("{error:#}")})
        }
    };
    result["cache_read"] = json!(cache_read);
    result["cache_write"] = json!(cache_write);
    result["prepare_ms"] = json!(begun.elapsed().as_secs_f64() * 1000.);
    lines::record_preparation(chart, result.clone());
    result
}
```

**格式常量、`Mesh` 与 key `prpr/src/render_lab/lines.rs:34-71`：**

```rust
const HEADER: usize = 116;
const MAX_LINES: usize = 65_536;
const MAGIC: &[u8; 8] = b"PHRLN001";
fn abi() -> [u8; 32] {
    Sha256::digest(b"phira-line-gpu-v1/ordered/params-xy-wide-thin-slot/rgba8-signed-width").into()
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Param {
    data: [f32; 4],
}
pub struct Mesh {
    source: [u8; 32],
    length: f32,
    palette: usize,
    ids: Vec<usize>,
    vertices: Vec<Param>,
    indices: Vec<u16>,
}
pub fn supported_ids(chart: &Chart) -> Vec<usize> {
    chart
        .order
        .iter()
        .copied()
        .filter(|&id| matches!(chart.lines[id].kind, JudgeLineKind::Normal))
        .collect()
}
pub fn cache_key(source: [u8; 32], length: f32, palette: usize, ids: &[usize]) -> String {
    let mut hash = Sha256::new();
    hash.update(abi());
    hash.update(source);
    hash.update(length.to_bits().to_le_bytes());
    hash.update((palette as u32).to_le_bytes());
    for &id in ids {
        hash.update((id as u32).to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}
```

**几何、定义域、编译、编码、解码 `prpr/src/render_lab/lines.rs:74-189`：**

```rust
fn quad(length: f32, slot: usize) -> [Param; 4] {
    let x1 = -length;
    let x2 = length;
    let dx = x2 - x1;
    let y = |thickness: f32| dx / ((dx * dx).sqrt() / (thickness * 0.5));
    let wide = y(0.01);
    let thin = y(0.0076);
    [
        Param {
            data: [x1, wide, thin, slot as f32],
        },
        Param {
            data: [x1, -wide, -thin, slot as f32],
        },
        Param {
            data: [x2, wide, thin, slot as f32],
        },
        Param {
            data: [x2, -wide, -thin, slot as f32],
        },
    ]
}
fn domain(length: f32, palette: usize, ids: &[usize]) -> Result<()> {
    ensure!(length.is_finite() && length > 0. && (length * 2.).is_finite() && (length * 2.).powi(2).is_finite(), "Unsupported line geometry");
    ensure!((1..=32).contains(&palette) && !ids.is_empty() && ids.len() <= MAX_LINES, "Line mesh budget/capacity");
    let mut unique = std::collections::HashSet::new();
    ensure!(ids.iter().all(|&id| id <= u32::MAX as usize && unique.insert(id)), "Duplicate/invalid source line");
    ensure!(quad(length, 0).iter().flat_map(|v| v.data).all(|v| v.is_finite()), "Non-finite lowered vertex");
    Ok(())
}
pub fn compile(source: [u8; 32], length: f32, palette: usize, ids: Vec<usize>) -> Result<Mesh> {
    domain(length, palette, &ids)?;
    let mut vertices = Vec::with_capacity(ids.len() * 4);
    let mut indices = Vec::with_capacity(ids.len() * 6);
    for i in 0..ids.len() {
        let slot = i % palette;
        vertices.extend_from_slice(&quad(length, slot));
        let base = (slot * 4) as u16;
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    Ok(Mesh {
        source,
        length,
        palette,
        ids,
        vertices,
        indices,
    })
}
pub fn encode(mesh: &Mesh) -> Vec<u8> {
    let mut body = Vec::with_capacity(mesh.ids.len() * 80);
    for &id in &mesh.ids {
        body.extend_from_slice(&(id as u32).to_le_bytes());
    }
    for v in &mesh.vertices {
        for x in v.data {
            body.extend_from_slice(&x.to_le_bytes());
        }
    }
    for &i in &mesh.indices {
        body.extend_from_slice(&i.to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&abi());
    bytes.extend_from_slice(&mesh.source);
    bytes.extend_from_slice(&(mesh.palette as u32).to_le_bytes());
    bytes.extend_from_slice(&mesh.length.to_le_bytes());
    bytes.extend_from_slice(&(mesh.ids.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(&body));
    bytes.extend_from_slice(&body);
    bytes
}
pub fn decode(bytes: &[u8], source: [u8; 32], length: f32, palette: usize, ids: &[usize]) -> Result<Mesh> {
    domain(length, palette, ids)?;
    ensure!(bytes.len() >= HEADER && &bytes[..8] == MAGIC && bytes[8..40] == abi() && bytes[40..72] == source, "Stale/invalid lowered line cache");
    let u = |o| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    ensure!(u(72) as usize == palette && u(76) == length.to_bits() && u(80) as usize == ids.len(), "Line cache shape/config changed");
    ensure!(bytes.len() == HEADER + ids.len() * 80, "Line cache counts/size mismatch");
    ensure!(bytes[84..116] == Sha256::digest(&bytes[HEADER..])[..], "Line cache checksum mismatch");
    let mut cursor = HEADER;
    for &id in ids {
        ensure!(u(cursor) as usize == id, "Line source order changed");
        cursor += 4;
    }
    let mut vertices = Vec::with_capacity(ids.len() * 4);
    for i in 0..ids.len() {
        for expected in quad(length, i % palette) {
            let data = std::array::from_fn(|_| {
                let v = f32::from_bits(u(cursor));
                cursor += 4;
                v
            });
            ensure!(data.map(f32::to_bits) == expected.data.map(f32::to_bits), "Invalid lowered line geometry/slot");
            vertices.push(Param { data });
        }
    }
    let mut indices = Vec::with_capacity(ids.len() * 6);
    for i in 0..ids.len() {
        let b = ((i % palette) * 4) as u16;
        for expected in [b, b + 1, b + 2, b + 2, b + 1, b + 3] {
            let value = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
            cursor += 2;
            ensure!(value == expected, "Invalid lowered triangle order");
            indices.push(value);
        }
    }
    Ok(Mesh {
        source,
        length,
        palette,
        ids: ids.to_vec(),
        vertices,
        indices,
    })
}
```

**资源所有权与运行态结构 `prpr/src/render_lab/lines.rs:21-32, 227-281`：**

```rust
pub struct CacheLease {
    manager: Arc<super::cache::RenderCache>,
    ticket: super::cache::Ticket,
}
impl CacheLease {
    pub fn new(manager: Arc<super::cache::RenderCache>, ticket: super::cache::Ticket) -> Self {
        Self { manager, ticket }
    }
    pub fn is_current(&self) -> bool {
        self.manager.is_current(&self.ticket)
    }
}
#[derive(Clone, Copy)]
struct Record {
    page: usize,
    slot: usize,
    start: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    page: usize,
    start: usize,
    count: usize,
}
impl Span {
    fn continues(self, record: Record) -> bool {
        self.page == record.page && self.start + self.count == record.start
    }
}
#[derive(Clone, PartialEq)]
struct Capture {
    projection: [f32; 16],
    pass: Option<miniquad::RenderPass>,
    viewport: Option<(i32, i32, i32, i32)>,
    clip: Option<(i32, i32, i32, i32)>,
}
struct Page {
    buffer: Buffer,
    uniforms: Vec<f32>,
    lines: usize,
}
struct Renderer {
    context_id: usize,
    lease: CacheLease,
    shader: Shader,
    _render_thread: PhantomData<Rc<()>>,
    records: Vec<Option<Record>>,
    order: Vec<usize>,
    cursor: usize,
    frame_valid: bool,
    pages: Vec<Page>,
    indices: Buffer,
    texture: miniquad::Texture,
    pipeline: Pipeline,
    length: u32,
    palette: usize,
    pending: Option<(Span, Capture)>,
    frames: u64,
    calls: u64,
    accepted: u64,
    transparent: u64,
    fallback: u64,
    draws: u64,
    loading_draws: u64,
    uniform_bytes: u64,
    static_bytes: usize,
}
```

**安装与资源创建 `prpr/src/render_lab/lines.rs:316-412`：**

```rust
pub fn install(mesh: Mesh, chart: &Chart, res: &Resource, lease: CacheLease) -> Result<()> {
    ensure!(res.config.sample_count == 1 && !res.config.chart_debug, "Low quality only");
    ensure!(supported_ids(chart) == mesh.ids && res.info.line_length.to_bits() == mesh.length.to_bits(), "Line source changed");
    ensure!(lease.is_current(), "Compiler work invalidated by clear/disable");
    let ctx = unsafe { get_internal_gl() }.quad_context;
    let shader = Shader::new(
        ctx,
        &vertex(mesh.palette),
        FRAGMENT,
        ShaderMeta {
            images: vec!["Texture".into()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("Projection", UniformType::Mat4),
                    UniformDesc::new("PoseA", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseB", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseC", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseD", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("RGBA", UniformType::Float4).array(mesh.palette),
                ],
            },
        },
    )?;
    let location = unsafe { miniquad::gl::glGetAttribLocation(shader.gl_internal_id(ctx).unwrap(), b"params\0".as_ptr().cast()) };
    if location != 0 || unsafe { miniquad::gl::glGetError() } != 0 {
        shader.delete(ctx);
        anyhow::bail!("Unsupported line attribute location");
    }
    let pipeline = Pipeline::with_params(
        ctx,
        &[BufferLayout::default()],
        &[VertexAttribute::new("params", VertexFormat::Float4)],
        shader,
        PipelineParams {
            color_blend: Some(miniquad::BlendState::new(
                miniquad::Equation::Add,
                miniquad::BlendFactor::Value(miniquad::BlendValue::SourceAlpha),
                miniquad::BlendFactor::OneMinusValue(miniquad::BlendValue::SourceAlpha),
            )),
            ..Default::default()
        },
    );
    let pages = mesh
        .vertices
        .chunks(mesh.palette * 4)
        .map(|v| Page {
            buffer: Buffer::immutable(ctx, BufferType::VertexBuffer, v),
            uniforms: vec![0.; 16 + mesh.palette * 20],
            lines: v.len() / 4,
        })
        .collect();
    let indices = Buffer::immutable(ctx, BufferType::IndexBuffer, &mesh.indices);
    let texture = miniquad::Texture::from_rgba8(ctx, 1, 1, &[255; 4]);
    let mut records = vec![None; chart.lines.len()];
    for (i, &id) in mesh.ids.iter().enumerate() {
        records[id] = Some(Record {
            page: i / mesh.palette,
            slot: i % mesh.palette,
            start: i * 6,
        });
    }
    let static_bytes = mesh.vertices.len() * std::mem::size_of::<Param>() + mesh.indices.len() * 2;
    let renderer = Renderer {
        context_id: ctx.context_id(),
        lease,
        shader,
        _render_thread: PhantomData,
        records,
        order: chart.order.clone(),
        cursor: 0,
        frame_valid: false,
        pages,
        indices,
        texture,
        pipeline,
        length: mesh.length.to_bits(),
        palette: mesh.palette,
        pending: None,
        frames: 0,
        calls: 0,
        accepted: 0,
        transparent: 0,
        fallback: 0,
        draws: 0,
        loading_draws: 0,
        uniform_bytes: 0,
        static_bytes,
    };
    ensure!(unsafe { miniquad::gl::glGetError() } == 0, "Line resources failed");
    ensure!(renderer.lease.is_current(), "Compiler work invalidated by clear/disable");
    chart.compiled_lines.borrow_mut().renderer = Some(renderer);
    Ok(())
}
```

**span 提交 `prpr/src/render_lab/lines.rs:419-449`：**

```rust
impl Renderer {
    fn flush(&mut self) {
        let Some((span, capture)) = self.pending.take() else { return };
        let page = &mut self.pages[span.page];
        page.uniforms[..16].copy_from_slice(&capture.projection);
        let uniforms = Arc::<[f32]>::from(page.uniforms.as_slice());
        let gl = unsafe { get_internal_gl() };
        let old = (gl.quad_gl.get_active_render_pass(), gl.quad_gl.get_viewport(), gl.quad_gl.get_scissor());
        gl.quad_gl.render_pass(capture.pass);
        gl.quad_gl.viewport(capture.viewport);
        gl.quad_gl.scissor(capture.clip);
        unsafe {
            gl.quad_gl.resident_geometry(
                self.pipeline,
                Bindings {
                    vertex_buffers: vec![page.buffer],
                    index_buffer: self.indices,
                    images: vec![self.texture],
                },
                uniforms,
                span.start as i32,
                span.count as i32,
            )
        };
        gl.quad_gl.render_pass(old.0);
        gl.quad_gl.viewport(old.1);
        gl.quad_gl.scissor(old.2);
        self.draws += 1;
        self.uniform_bytes += page.uniforms.len() as u64 * 4;
    }
}
```

**每帧边界、逐线前置检查与命中路径 `prpr/src/render_lab/lines.rs:561-672`：**

```rust
pub fn begin_chart(chart: &Chart) {
    let mut state = chart.compiled_lines.borrow_mut();
    if state.renderer.as_ref().is_some_and(|r| !r.lease.is_current()) {
        state.renderer = None;
        state.status = serde_json::json!({"requested":true,"installed":false,
            "fallback":"Compiled line cache invalidated by clear/disable; reload the chart to prepare new resources."});
    }
    let current = unsafe { get_internal_gl() }.quad_context.context_id();
    if state.renderer.as_ref().is_some_and(|r| r.context_id != current) {
        state.renderer = None;
        state.status =
            serde_json::json!({"requested":true,"installed":false,"fallback":"Managed context changed; reload the chart to prepare new resources."});
    }
    if let Some(r) = state.renderer.as_mut() {
        r.flush();
        r.cursor = 0;
        r.frame_valid = true;
        r.frames += 1;
    }
}
pub fn before_line(chart: &Chart, id: usize, kind: &JudgeLineKind, res: &Resource) {
    let mut state = chart.compiled_lines.borrow_mut();
    if let Some(r) = state.renderer.as_mut() {
        if r.order.get(r.cursor) != Some(&id) {
            r.frame_valid = false;
        }
        r.cursor += 1;
        if !r.frame_valid
            || res.config.chart_debug
            || res.config.sample_count != 1
            || !matches!(kind, JudgeLineKind::Normal)
            || r.records.get(id).and_then(|v| *v).is_none()
        {
            r.flush();
        }
    }
}
pub fn end_chart(chart: &Chart) {
    let mut state = chart.compiled_lines.borrow_mut();
    if let Some(r) = state.renderer.as_mut() {
        r.flush();
        r.frame_valid = false;
    }
}
pub fn try_render(chart: &Chart, id: usize, length: f32, thin: bool, color: Color, res: &Resource) -> bool {
    let mut state = chart.compiled_lines.borrow_mut();
    let Some(r) = state.renderer.as_mut() else { return false };
    r.calls += 1;
    let gl = unsafe { get_internal_gl() };
    let model = gl.quad_gl.get_model_matrix().to_cols_array();
    let capture = Capture {
        projection: gl.quad_gl.get_projection_matrix().to_cols_array(),
        pass: gl.quad_gl.get_active_render_pass(),
        viewport: gl.quad_gl.get_viewport(),
        clip: gl.quad_gl.get_scissor(),
    };
    let record = r.records.get(id).and_then(|v| *v);
    if !r.frame_valid
        || record.is_none()
        || length.to_bits() != r.length
        || res.config.sample_count != 1
        || res.config.chart_debug
        || !gl.quad_gl.is_using_default_material()
        || gl.quad_gl.is_geometry_capture_enabled()
        || gl.quad_gl.is_depth_test_enabled()
        || !model.iter().chain(capture.projection.iter()).all(|v| v.is_finite())
        || ![color.r, color.g, color.b, color.a].iter().all(|v| v.is_finite())
    {
        r.flush();
        r.fallback += 1;
        return false;
    }
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    let record = record.unwrap();
    if r.pending.as_ref().is_some_and(|(span, key)| !span.continues(record) || *key != capture) {
        r.flush();
    }
    let rgba = [color.r, color.g, color.b, color.a].map(|v| (v * 255.) as u8 as f32);
    if rgba[3] == 0. {
        r.transparent += 1;
    }
    let page = &mut r.pages[record.page];
    for col in 0..4 {
        let offset = 16 + col * r.palette * 4 + record.slot * 4;
        page.uniforms[offset..offset + 4].copy_from_slice(&model[col * 4..col * 4 + 4]);
    }
    let offset = 16 + 4 * r.palette * 4 + record.slot * 4;
    page.uniforms[offset..offset + 4].copy_from_slice(&[rgba[0], rgba[1], rgba[2], if thin { -rgba[3] - 1. } else { rgba[3] }]);
    if let Some((span, _)) = &mut r.pending {
        span.count += 6;
    } else {
        r.pending = Some((
            Span {
                page: record.page,
                start: record.start,
                count: 6,
            },
            capture,
        ));
    }
    r.accepted += 1;
    true
}
```

**延迟退休 `prpr/src/render_lab/lines.rs:673-721`：**

```rust
struct Retired {
    context_id: usize,
    pages: Vec<Page>,
    indices: Buffer,
    texture: miniquad::Texture,
    pipeline: Pipeline,
    shader: Shader,
}
static RETIRED: Mutex<Vec<Retired>> = Mutex::new(Vec::new());
pub fn flush_pending_deletions() {
    let retired = std::mem::take(&mut *RETIRED.lock().unwrap());
    if retired.is_empty() {
        return;
    }
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    let context = gl.quad_context;
    for resource in retired {
        if resource.context_id != context.context_id() {
            continue;
        }
        context.clear_resource_bindings();
        for page in resource.pages {
            page.buffer.delete();
        }
        resource.indices.delete();
        resource.texture.delete();
        let pipeline = resource.pipeline.delete(context);
        let shader = resource.shader.delete(context);
        debug_assert!(pipeline && shader, "Compiled-line resources were not exclusively owned");
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        RETIRED.lock().unwrap().push(Retired {
            context_id: self.context_id,
            pages: std::mem::take(&mut self.pages),
            indices: self.indices,
            texture: self.texture,
            pipeline: self.pipeline,
            shader: self.shader,
        });
    }
}
```

#### 4.4 调用链与生命周期（加载期→游玩期→清理/失效）

**加载期：**

1. `GameScene::new` 读取谱面字节后，先调用 `render_compiler::workset::prepare(&chart, &res, &chart_bytes)`（P-02，见后文），再调用 `render_compiler::prepare(&chart, &res, &chart_bytes)`。
2. `prepare()` 先 `lines::disable(chart)`，把 Chart 上的旧 renderer 清空；再把 `Res.config.chart_pre_compile` 同步到进程 cache 的 enabled/epoch。
3. `reads::prepare`、`visibility::prepare` 是另外的实验消费者；P-01 只依赖 `lines::*`，但共享同一个 `RenderCache`。
4. 开关关闭：直接返回 `requested=false`，不 hash、不读盘、不建目录、不创建 GPU 资源；原 `draw_line` 路径继续。
5. 开关开启：`capability` 检查低画质与 GL 容量 → `cache().begin(key)` → `read` → `decode` 或 `compile/encode/decode` → `publish` → `install` → `warm_loading`。
6. `install()` 把 `Renderer` 放进 `chart.compiled_lines: RefCell<State>`；`State` 由 Chart 拥有。`CacheLease` 只保存在 renderer 内，不写入 blob；blob 里没有任何 GL 句柄。
7. `warm_loading()` 在加载期用 1×1 render target 对每个 page 的每个 slot、宽/窄两种宽度做一次真实绘制和 `glReadPixels`，确认 immutable buffer、shader、sampler、palette 都能被驱动实际消费；读回后恢复 native transfer/FBO/viewport 状态。它只覆盖有限 slot/宽度组合，不等于所有驱动路径或 EGL loss。

**游玩期：**

1. `Chart::render_inner` 在遍历 `chart.order` 前调用 `lines::begin_chart`。
2. 每条线 `JudgeLine::render_inner` 先调用 `lines::before_line`，再在 `JudgeLineKind::Normal` 分支调用 `lines::try_render`。
3. 命中：写当前 slot 的 model/RGBA，追加到 pending span；不调用原 `draw_line`。
4. 未命中/barrier：`try_render` 先 `flush()` 已积累的 span，再返回 `false`，随后代码执行 `render_elision::try_skip_cpu_line` 或原 `draw_line`；顺序与状态不跳。
5. 一条线结束后，后面的线继续；页切换（第 33 条普通线）会自然形成新 span。`flush()` 只在 span 不连续或 capture 变化时发生，不会按材质重排。
6. `end_chart` 在最后一个线之后 `flush()`，把最后一个 span 提交；`note_buffer.draw_all()` 仍按原顺序执行。

**清理/失效：**

1. Settings 的开关调用 `prpr::render_compiler::set_enabled(false)`；`RenderCache::set_enabled` 递增 epoch。所有旧 ticket 立即 `is_current()==false`。
2. Settings 的“清除缓存”调用 `phira::dir::clear_cache()`；它走同一个 `RenderCache::clear_all()`，先递增 epoch，再在锁内按配置的 roots/protected 边界删除。旧 worker 即使编译完成，`publish()` 看到 epoch 不匹配会返回 `false`，不能重建目录。
3. 下一次 `begin_chart` 发现 `lease.is_current()==false`，把 `renderer=None`，写 fallback 状态；`Renderer::drop` 把资源推进 `RETIRED`。
4. 主线程 texture retirement hook 调 `lines::flush_pending_deletions()`；先 `gl.flush()` 消费在途 resident 命令，再按 `context_id` 删除。跨 managed context 的资源不删除，留给原 context 生命周期处理。
5. managed context id 变化也会让 `begin_chart` 丢弃 renderer；但 `context_id` 不是 EGL loss 检测器：同一 managed context 内的 native EGL surface 丢失，源码明确没有证明。
6. `Chart::render` 公开路径始终走原 `draw_line`；只有 `GameScene` 私有的 `ChartRenderScope` 才能把 `Prepared` 和 Chart 配对传给 `render_scoped`。这是 P-02 的所有权设计，也保护 P-01 不被外部调用者绕过开关。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| Cargo feature（prpr） | `chart_pre_compile = []` | 只编译判定线 GPU IR，不依赖 `render_research`；`render_chart_ir = ["render_research","chart_pre_compile"]` 是旧的负收益音符 IR，不是本条 |
| Cargo feature（phira） | `chart_pre_compile = ["prpr/chart_pre_compile"]` | 主 crate 转发 |
| phira 默认 feature | `default = ["video", "font_atlas_pool", "chart_pre_compile"]` | **默认编入** |
| phira-main 默认 feature | `default = ["video", "chart_pre_compile"]` | 桌面主程序也默认编入 |
| prpr 自身默认 feature | `default = ["log", "font_atlas_pool"]` | 单独构建 prpr 时**不**默认编入 `chart_pre_compile` |
| 运行开关 | `Config.chart_pre_compile`，默认 `false` | 即使编入，默认游玩仍走原路径 |
| Settings | `item-chart-pre-compile` 开关，只有 feature 编入时显示 | 修改后立即 `set_enabled`，并写 `data.json` |
| 低画质条件 | `res.config.sample_count == 1 && !res.config.chart_debug` | 高画质/MSAA/调试不安装，自动回退 |
| 研究覆盖 | `WARM_OPTIONS` thread-local `(warm, force_unavailable)`，默认 `(true,false)` | 只在 `render_research` 下存在；生产固定 `(true,false)` |
| 缓存根 | 现有 app cache root 下的 `render-ir/` | 不迁移旧 cache，不创建第二套目录 |
| 读取上限 | `MAX_CACHE_BYTES = 256 * 1024 * 1024` | 打开后文件增长也由 `take(limit+1)` 拦截 |
| 环境变量 | 无 | 开关只走 Cargo feature + Config，不读环境变量 |
| 默认是否实际启用 | **编译期默认编入；运行期默认关闭** | 这是本文档要澄清的核心事实 |

默认值真相必须分三层说：

1. **源码/构建层**：`phira/Cargo.toml` 的默认 feature 已含 `chart_pre_compile`；`phira-main` 也转发；Android Gradle 直接执行 `cargo ndk -t arm64-v8a --platform 26 build --locked --release -p phira`，所以普通 APK 会把这个 feature 编进去。
2. **运行层**：`Config::default()` 的 `chart_pre_compile` 是 `false`；用户不打开 Settings，游戏仍走原 `draw_line`。所以“默认启用”不成立，准确说法是“默认编入、默认关闭”。
3. **能力层**：即使运行开关打开，`capability()` 还要求低画质、单采样、非 debug、GL 容量足够；不满足就回退原路径。`quality_class` 明确写 `mild-lossy; low quality required independently of compiler switch`。

### 6. 测量方法与完整数据（完整表格；注明窗口、样本、设备、同 SO/ABBA）

#### 6.1 测量口径

- 设备：Mali-T880 / EDI-AL10 / Android 8 / arm64-v8a / 2560×1440。
- 谱面：`f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip`，24 线、1349 普通非 hold、219 holds。
- 采样：每轮 120 帧预热 + 1200 帧真实时钟采样；应用墙钟只统计 1200 帧。
- 画质：`sampleCount=1`、`aggressive=false`、`particle=true`、`shaderPreRender=false`、`disableEffect=false`、autoplay。
- 同 SO：compiler-v1-retry 与 lines-v1 各自一个 SO；两组不混池。
- 交错：compiler-v1-retry 为 off/on/on/off；lines-v1 为 A-B-B-A-A-B-B-A（原线/GPU线/GPU线/原线/原线/GPU线/GPU线/原线）。
- 说明：应用墙钟是 `update+render+flush`，包含驱动执行/调度/等待；不是 GPU elapsed。呈现间隔来自 SurfaceFlinger，另有 10 秒左右的独立采样，不是完整 1200 帧。

#### 6.2 compiler-v1-retry（生产入口，同 SO，4 轮）

| 轮次 | 路径 | 应用墙钟均值 ms | 应用 p99 ms | 应用最大 ms | >16.67 ms | >20 ms | FPS | chart 阶段均值 ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 6.275313 | 14.348839 | 18.840104 | 3 | 0 | 59.734853 | 1.238013 |
| 1 | on | 5.446275 | 13.071927 | 18.041667 | 1 | 0 | 59.696001 | 1.233457 |
| 2 | on | 5.617995 | 12.523395 | 15.894792 | 0 | 0 | 59.746561 | 1.271070 |
| 3 | off | 6.177321 | 12.352240 | 22.130210 | 5 | 1 | 59.700508 | 1.211801 |

- 组均值：off `6.226317 ms`（2 轮），on `5.532135 ms`（2 轮）。
- 减少：`0.694182 ms`，`11.149163%`。
- 方向一致性：两轮 on 都低于两轮 off；但 off 的 A2−A1 为 `−0.097992 ms`，on 的 B2−B1 为 `+0.171720 ms`，组差大于前后漂移。
- 阶段拆解：off 的 `render_ms` 为 `5.075183/4.988658`，on 为 `4.285025/4.405638`；`particle_ms` off `1.544873/1.531737`，on `0.776490/0.800677`。chart 阶段没有下降（off `1.238013/1.211801`，on `1.233457/1.271070`），说明收益主要来自线提交所在的 render/队列段，不是“谱面 CPU 遍历”段。注意 `particle_ms` 是 render 内的子段，**不能和 `render_ms` 相加**；阶段均值只用于定位，不是互斥分解。
- 呈现最大值：off `20.513541/18.698438 ms`，on `16.804688/16.807292 ms`；>16.67 应用帧 off `3/5`，on `1/0`；>20 应用帧 off `0/1`，on `0/0`。这些只是有限窗口，不称稳定尾改善。

#### 6.3 lines-v1（研究后端，同 SO，8 轮 ABBA）

| 轮次 | 路径 | 应用墙钟均值 ms | 应用 p99 ms | 应用最大 ms | >16.67 ms | >20 ms | FPS | 呈现最大 ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 0 | 原线 | 6.082873 | 13.333645 | 29.589584 | 7 | 1 | 59.657676 | 17.419792 |
| 1 | GPU线 | 5.528864 | 13.944999 | 23.385939 | 3 | 1 | 59.650069 | 16.790104 |
| 2 | GPU线 | 5.459668 | 12.249010 | 17.024479 | 2 | 0 | 59.743800 | 16.803125 |
| 3 | 原线 | 6.321753 | 12.347812 | 18.245312 | 3 | 0 | 59.717289 | 16.915625 |
| 4 | 原线 | 6.674186 | 13.140448 | 18.245312 | 3 | 0 | 59.721528 | 33.468750 |
| 5 | GPU线 | 5.505540 | 12.189432 | 19.676561 | 2 | 0 | 59.716194 | 16.798437 |
| 6 | GPU线 | 5.415026 | 12.458114 | 17.158335 | 1 | 0 | 59.750950 | 16.800000 |
| 7 | 原线 | 6.042810 | 13.314360 | 19.741667 | 2 | 0 | 59.703087 | 16.898958 |

- 组均值：原线 `6.280406 ms`（4 轮），GPU 线 `5.477274 ms`（4 轮）。
- 减少：`0.803131 ms`，`12.787887%`。
- 四轮 GPU 线都低于四轮原线（原线范围 6.043–6.674，GPU 5.415–5.529）。
- FPS 约 59.65–59.75；不能说“FPS 提高 13%”，因为接近 60 上限。
- 应用 p99：原线 13.334/12.348/13.140/13.314，GPU 13.945/12.249/12.189/12.458；GPU 首轮 p99 反而更高。
- >16.67 应用帧：原线 `[7,3,3,2]`，GPU `[3,2,2,1]`；>20 各只有首轮 1 帧。不能据此称稳定尾延迟改善。
- 呈现：只有原线第 4 轮出现一次 `33.469 ms` 最大间隔，其余 16.79–17.42；不称尖峰消除。
- 最慢应用帧：原线 `29.590 ms`（chart 76.513 s，particle 21.110 ms），GPU `23.386 ms`（chart 76.717 s，particle 15.143 ms）。particle 阶段包含前面的队列提交，不能全归粒子算法。

#### 6.4 渲染调用计数（lines-v1-quality，不是同一性能窗口）

- `lines-v1-quality` 11 个作业、8 组有效末帧 RGBA 全同。
- 动态图 240 帧接受 5760 条线；实际录制 240 条 resident 命令；`0 CPU fallback / 0 gameplay buffer upload`。
- managed native draw 平均 `38.333 → 17.333`；`logical pass/FBO` 绑定调用不变，不能称物理 GPU pass 减少。
- uniform payload `629760 bytes`（包含预热），不能说是逐帧零动态数据。
- 仍有 `4639` 条 `alpha byte=0` 的线在网格内；透明 primitive DCE 是另一条优化，不属于 P-01。

#### 6.5 原始记录（README 与 AGENTS 原文）

历史引文中的分支标识已归一化为“参考实现”，其余原文未改。以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

`本地证据/low-quality-normal/README.md` 第 9、13、36-38、40-53 行原文：

> 最新源码提交 `9498c73427a8e5d88631826267cb562246590ca0`（接入谱面预编译开关，本地未push）。上一提交 `9913d98`完成Chart归属/主线程retirement。两库独立resource-lifecycle参考实现已push，miniquad 1dfaa933/macroquad c862fc85完整rev。有效普通线编译器现在有独立chart_pre_compile feature，不依赖负收益note原型或Android研究入口；正常GameScene::new加载消费者、默认off的Config/UI与同一Arc cache已连接。Settings与observer共用clear/size/toggle函数，清理/关闭代次失效已实际验证。研究SO compiler-lab与正常入口SO compiler-production均31源核对9498c734，Windows三模式、16研究测试与Android两构建通过。
>
> compiler-v1-quality 10作业/8组RGBA全同；实际冷/热/损坏/清理、独立关闭、不读写/保留旧blob、高画质+aggressive回退、shader开关独立与20轮重入。实际Settings size看到IR+tmp 13bytes，clear后0，晚worker拒绝发布、disabled/re-enabled旧ticket拒绝、source ZIP保存。没有宣称实际UI点击/full主应用或EGL loss通过。源码仍轻量有损/低画质候选。
>
> compiler-v1首次性能批次呈现采集超过job运行窗口，失败原始数据保留，不能称完整ABBA。重试compiler-v1-retry 4轮同SO：off/on/on/off 6.2753/5.4463/5.6180/6.1773ms；均值6.2263→5.5321ms，11.15%，FPS59.70–59.75。完整stage/尖峰/呈现见compiler-v1-retry-performance-summary.json；应用>16.67ms off3/5 vs on1/0，仍不称所有尖峰已消除。lifecycle-v1是另一独立cohort，6.0508→5.3358ms，11.82%，不能混池。未来优先真实首次GPU采样预热，EGL loss/音符blob/字体静态浪费/全曲及多设备仍未完成，完整goal保留active。
>
> 源码已本地提交 `2c31be7f01286a84cab1b65adcf5b91571c4f7b1`（编译普通判定线批次），主项目未push。最终`lines-lab.so` SHA `8a9c5e21364cbbb38fb6f72abb338a1a84367b132f33050d105747243ea4f2a3`，23个冻结源码文件核对commit，Android arm64/API26 release通过。普通线的2036bytes blob实际冷生成、命中、关闭停读写、损坏重建及清理后重建；1824bytes immutable GPU buffers，单页32容量容纳24线。它不是GPU音符blob，不将音符未完成项擦掉。
>
> `lines-v1-quality`十一项作业、八组有效末帧RGBA全同。动态图240帧接受5760线，实际录制240条resident命令，0CPU回退/0gameplay buffer上传；仍有629760bytes uniforms（包含预热）。平均managed native draw 38.333→17.333，logical pass/FBO绑定调用不变，不能称物理GPU pass减少。4639条alpha byte=0仍在网格内，透明primitive DCE未实现。高画质、独立开关关闭和强制缺能力实际回退。有限时刻相同不证明全曲/所有GPU相同，仍分类轻量有损待验证。
>
> `lines-v1-performance`同SO，120预热+1200真实时钟采样/轮；font pool相同，shader uniform缓存/HUD批量/背景近似/提交诊断/线程CPU/GPU计时全部关闭。顺序A-B-B-A-A-B-B-A；只变化普通线消费者，原音符/判定/音轨/粒子保持。
>
> | 轮次 | 路径 | 完整应用墙钟均值 ms | 应用 p99 ms | 呈现最大间隔 ms |
> |---|---|---:|---:|---:|
> | 0 | 原线 | 6.083 | 13.334 | 17.420 |
> | 1 | GPU线 | 5.529 | 13.945 | 16.790 |
> | 2 | GPU线 | 5.460 | 12.249 | 16.803 |
> | 3 | 原线 | 6.322 | 12.348 | 16.916 |
> | 4 | 原线 | 6.674 | 13.140 | 33.469 |
> | 5 | GPU线 | 5.506 | 12.189 | 16.798 |
> | 6 | GPU线 | 5.415 | 12.458 | 16.800 |
> | 7 | 原线 | 6.043 | 13.314 | 16.899 |
>
> 四轮均值6.280→5.477ms，减少12.79%；原线各轮6.043–6.674、GPU各轮5.415–5.529，这个窗口中四轮GPU均低于四轮原线。均约59.65–59.75fps，不能说FPS提高13%。墙钟是update+render+flush，包含驱动执行/调度/等待，不是独立GPU elapsed；swap/其他线程不属于这个观察器。原线>16.67ms帧[7,3,3,2]，GPU[3,2,2,1]；>20ms各只有首轮1帧，GPU首轮p99比对照更高，不称稳定尾延迟改善。独立呈现只有一个原线33.47ms事件，不据此称卡顿已消除。汇总/最大帧/实际chart-time范围见`lines-v1-performance-summary.json`。

`本地证据/AGENTS.md` 第 641、643、659 行原文：

> 最新生产消费者接入已直接修改并本地提交 `9498c73427a8e5d88631826267cb562246590ca0`（接入谱面预编译开关，2026-10-07），tracked干净，主项目未push；两库仍1dfaa933/c862fc85参考实现干净，主项目未改。独立chart_pre_compile feature可运行正常游戏入口，不启用research；Config.chart_pre_compile默认false，Settings独立按钮仅在有效consumer已编入时显示。GameScene::new实际加载并消费普通线GPU blob；关闭不hash/read/write/create GPU，低画质权限独立，aggressive或shader_pre_render不能自动启用它。App和所有研究生产者共享同一Arc RenderCache，Settings清理/大小扫描共用dir::clear_cache/cache_size。保留legacy Android cache根目录，没有迁移其它cache。Windows默认/独立/研究三种检查、16研究测试通过；研究Android compiler-lab SHA `49ab0ef7494184577d65ad0b0c97eacefd3e6515d022e5f605bb97133379da13`，正常入口Android compiler-production SHA `54cee055960b6ea47eb9e61cf73bfbcbe179e5530c954b717ffcf7fa804f0cf3`，两者31个冻结文件核对9498c734。后者首次link因C盘TEMP不足失败，日志保留；改本次TEMP/TMP到G盘target/research-tmp后真正重建成功，不是安装target或假通过。
>
> compiler-v1-quality十项真机作业/八组末帧RGBA全同，调用正常GameScene::new入口而非manual IR注入。实际冷写/热读/损坏重建/清理重建、高画质+aggressive回退、关闭保留旧blob且不读取/写入、shader_pre_render独立、20轮重入都执行。production cache probe使用Settings同函数，13bytes IR+旧tmp被统一size扫描计入，clear后size0、晚完成worker不可复建目录、disable/re-enable旧ticket不复活、源ZIP SHA保留。UI按钮与真实消费者代码已连通，但没有用真实UI点击/完整主应用重入验收，不把observer冒充全主应用。首次compiler-v1-performance因独立呈现采集未在20s窗口内结束而失败，保留不完整cohort，不引用为ABBA。主机编译结束后compiler-v1-retry四轮完整ABBA通过（120预热+1200采样）：off/on/on/off墙钟6.2753/5.4463/5.6180/6.1773ms，均值6.2263→5.5321ms，减少11.15%；约59.70–59.75fps。应用p99 off14.349/12.352、on13.072/12.523ms；>16.67ms off3/5、on1/0，>20ms off0/1、on0/0；on呈现max16.805/16.807，但不能凭四轮称稳定尾改善或尖峰消除。均有原始数据/汇总，包含驱动/调度/等待墙钟，不是GPU elapsed。后端均保持原判定/音轨。完整目标仍active：真实首次GPU采样预热、EGL loss重建、音符真正降低blob、字体池静态浪费/物理内存、全曲/其它GPU/复杂字体和gameplay尖峰还需继续。下一阶段优先加载期真实GPU预热，不能因已接cache开关结束。
>
> lines-lab SHA `8a9c5e21364cbbb38fb6f72abb338a1a84367b132f33050d105747243ea4f2a3`，23个冻结文件与2c31be7逐项核对。Windows默认/研究检查、16个研究测试、Android API26构建通过。十一项真机quality/cache作业形成八组有效末帧RGBA全同（固定/动态、冷/热、关闭独立预编译、高画质/强制能力回退、损坏及清理后重建），均实际执行。动态240帧接受5760线、实际录制240条resident命令，0CPU回退；native managed draw平均38.333→17.333，logical pass/FBO数量不变，0gameplay buffer上传；uniform payload629760bytes包含预热，不能说逐帧完全零动态数据。4639条alpha byte=0仍被绘制，尚未做透明线DCE。原曲source保留，缓存复用既有manager同一根/clear代失效原型，但生产单一manager/Config/UI/统一清理尚未接通，不能以研究请求替代生产开关。只真正实现线的GPU blob，音符GPU降低blob仍未落盘。数据lines-v1-quality-*。同SO关闭提交/线程/GPU诊断的八轮真实时钟A-B-B-A-A-B-B-A已完成（lines-v1-performance-*，120预热+1200采样/轮）：应用update+render+flush墙钟均值依次6.083/5.529/5.460/6.322/6.674/5.506/5.415/6.043ms，四轮对照均值6.280、四轮GPU5.477，减少12.79%；本窗口四轮GPU都低于四轮对照，但不外推全谱面/所有硬件。FPS59.65–59.75，呈现p99仍约16.77–16.79ms，不称FPS提高13%。应用>16.67ms帧对照[7,3,3,2]、GPU[3,2,2,1]，>20ms各首轮1帧；GPU首轮p99 13.945高于对照13.334，不能称稳定尾延迟改善。独立呈现只有单个对照33.469ms最大间隔，其余16.79–17.42，不称尖峰消除。对照29.59ms/GPU23.39ms最慢应用帧在76.5/76.7s，粒子阶段含前面的队列提交，不能归给粒子算法；有字形更新伴随但不直接认定因果。其余19.7ms帧有HUD13.6–14.5ms及442–463KB glyph上传，仍需继续真实驱动/资源尖峰定位。summarize-lines.py及完整汇总/时钟范围/最大帧数据保留；USB常亮2，两库参考实现干净，无遗留作业在跑。下一阶段必须处理有效生产消费者独立Config/UI、单一cache manager/统一清理/禁用竞态、Renderer与Chart生命周期、pipeline/shader retirement、context/reentry和真实GPU预热，再扩展混合线/page/全曲/设备验证。音符GPU blob与字体池静态浪费仍未解决，完整目标active，不以本轮正收益结束。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证的画面对照

| 组别 | 作业/对照 | 覆盖 | 结果 |
|---|---:|---|---|
| `compiler-v1-quality` | 10 个作业 / 8 组有效末帧 | 正常 `GameScene::new` 入口、冷写/热读、损坏重建、清理重建、高画质回退、aggressive 回退、`shader_pre_render` 独立、关闭后保留旧 blob、20 轮重入 | 每组 2560×1440 RGBA **逐像素相同**；实际执行 cache probe：13 bytes IR+tmp 被 size 扫描计入，clear 后 0，晚 worker 不能发布，disable/re-enable 旧 ticket 不复活，源 ZIP SHA 不变 |
| `lines-v1-quality` | 11 个作业 / 8 组有效末帧 | 固定/动态、冷/热、关闭独立预编译、高画质、强制缺能力、损坏、清理后重建 | 每组 RGBA **逐像素相同**；动态 240 帧接受 5760 线，240 条 resident 命令，0 CPU 回退，0 gameplay buffer 上传 |
| `lines-v1` cache/lifecycle | 冷/热/关闭/清理/损坏/20 轮重入 | 2036 bytes blob、1824 bytes immutable buffers、单页 32 slot | 冷生成、命中、关闭停读写、损坏重建、清理后重建都实际执行；没有把“文件存在”当作“消费者已用” |
| `lifecycle-v1` | 独立 cohort | Chart 销毁、pipeline/shader retirement、pending resident draw、1px readback | 6.0508→5.3358 ms（−11.82%），但这是另一独立 cohort，不能和 compiler-v1-retry 混池 |

#### 7.2 失败项与限制

- **首次 compiler-v1-performance 失败**：独立呈现采集超过 job 运行窗口，原始数据保留，不能算完整 ABBA；后续 retry 才是有效 cohort。
- **尾延迟未稳定改善**：compiler-v1-retry 的应用 >16.67 ms 帧为 off `3/5`、on `1/0`，但只有 4 轮，且 on 的 B2−B1 有正漂移；lines-v1 的 GPU 首轮 p99 `13.945` 高于对照 `13.334`，不能称稳定尾延迟改善。
- **呈现尖峰**：lines-v1 只有一次原线 `33.469 ms` 最大呈现间隔，不能据此说尖峰消除。
- **透明线仍被绘制**：4639 条 alpha byte=0 的线仍在网格内；P-01 不做透明 DCE，后续 `render_elision` 是另一条优化。
- **不是 GPU 独占计时**：应用墙钟包含驱动/调度/等待，`render_ms` 下降不能直接换算成 GPU 时间下降。
- **没有 EGL loss/跨 context 证明**：`context_id` 只能发现 managed context 变化，不能检测同一 managed context 内的 native EGL surface 丢失；shader/pipeline 完整生命周期仍未验收。
- **没有全曲/多设备/手动输入**：只测了单 Mali-T880、单谱面、autoplay、有限 chart-time 窗口（约 62–82 s）。
- **没有真实 Main UI 点击验收**：Settings 按钮与消费者代码已连通，但没有用真实 UI 点击完成全流程；不能用源码接线代替 UI 验收。
- **音符 GPU blob 未完成**：P-01 只降低普通判定线；1349 普通音符的 GPU 常驻 blob 仍是另一项未完成工作。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：局部。**

理由：

1. **收益真实且方向一致**：compiler-v1-retry 两轮 on 都低于两轮 off，均值 `6.2263→5.5321 ms`（−11.15%）；lines-v1 四轮 GPU 都低于四轮原线，均值 `6.2804→5.4773 ms`（−12.79%）。这不是单轮偶然。
2. **收益有明确的机制对应**：render 阶段下降约 0.7–0.8 ms，chart 阶段基本不变；managed draw 从 38.333 降到 17.333；线几何/索引/颜色被预编译，逐帧只剩 pose/RGBA 写入和 resident 提交。
3. **为什么是“局部”而不是“整帧”**：两组 FPS 都在 59.65–59.75，接近 60 上限；应用墙钟下降没有转化为可见 FPS 提升。收益集中在线的提交成本，不是所有帧、所有阶段、所有 GPU 的整帧加速。p99/尖峰没有稳定改善。
4. **为什么是“正”而不是“无”**：chart 阶段虽然不变，但 render/particle 段和总墙钟有可重复下降；不是仅减少 API 调用计数。对照前后漂移小于组差，方向在共同 chart-time 窗口也保留。
5. **质量代价**：GPU shader 求值属于 `mild-lossy`，只在低画质/单采样/默认材质下启用；有限 RGBA 对照为 0，但不能升级为全设备无损。

**置信度：**

- 对“这个有限窗口、这个 SO、这个设备上的应用墙钟下降”：**高**。
- 对“机制是线几何预编译与 resident 提交减少”：**中高**；阶段拆解和 draw 计数支持，但没有 GPU elapsed 分解。
- 对“全曲/所有 GPU/稳定尾延迟/整帧 FPS 提升”：**低**，不能从本 cohort 外推。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`2c31be7f` 编译普通判定线批次；`9913d98b` 管理谱面渲染资源/主线程 retirement；`9498c734` 接入独立 `chart_pre_compile` feature、Config/UI、统一 cache 清理 |
| 参考实现是否默认启用 | **否（运行期默认关闭；编译期默认编入）**。`phira/Cargo.toml` 默认含 `chart_pre_compile`，`phira-main` 也转发；`Config.chart_pre_compile=false`，Settings 不打开就走原路径 |
| 参考实现是否有消费者 | 有。`GameScene::new` 调用 `render_compiler::prepare`；`Chart::render_inner` 调 `begin_chart`/`end_chart`；`JudgeLine::render_inner` 调 `try_render`；Settings 用同一个 cache 清理/大小入口 |
| 是否进 main | 否。所有提交都在参考实现，主项目未 push |
| 是否默认低画质 | 是。`capability` 要求 `sample_count==1 && !chart_debug`；高画质/MSAA/调试自动回退 |
| 是否有独立开关 | 有。`Config.chart_pre_compile` + Settings 按钮；关闭不读/写/创建缓存，旧 ticket 失效 |
| 是否有统一清理 | 有。`dir::clear_cache`/`cache_size` 与 `RenderCache` 共用；但真实 UI 点击/完整主应用重入仍未验收 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **质量分类仍是轻量有损候选**。源码自己写 `mild-lossy`；GPU f32 求值、驱动精度、FTZ/FMA 与 CPU 路径不可能仅凭公式相同就宣布全设备无损。
2. **只在低画质/单采样/默认材质/非调试下启用**。高画质、MSAA、capture、depth、非默认材质都回退；这不是一个全画质通用优化。
3. **收益是局部提交成本，不是整帧 FPS**。FPS 已接近上限，p99/尖峰没有稳定改善；主项目没有足够理由为一个低画质局部收益承担额外 shader/pipeline/缓存生命周期复杂度。
4. **生命周期仍未完全闭环**。shader/pipeline retirement、EGL loss、context 重建、真实首次 GPU 采样预热、全曲/多设备/手动输入都未验收。
5. **参考实现未 push，主项目未进 main**。这不是“被否决”，而是“保留为已验证候选，等待更大的整帧收益或更完整的生命周期证据”。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| 驱动精度差异 | shader 内 `Projection*model`、`c/255.` 可能和原路径不同 | 低画质限定；有限 RGBA 对照；`mild-lossy` 分类 |
| shader/pipeline 生命周期 | 编译失败、link 失败、context 丢失、资源复用 | `glGetAttribLocation` 失败即回退；`Drop` 延迟退休；`context_id` 检查；但仍未证明 EGL loss |
| cache 损坏/旧源 | 旧 blob、截断、恶意计数、文件增长 | magic/ABI/source/shape/checksum/逐顶点逐索引校验；256 MiB 读取预算；`take(limit+1)` |
| 清理竞态 | clear/disable 与后台编译同时发生 | epoch + ticket；`publish` 锁内检查；`clear_all` 先失效再删除；测试覆盖晚 worker |
| 缓存路径 | Android 旧 cache 根有嵌套路径问题 | 保留 legacy 根，不迁移；绝对路径校验 |
| 透明顺序 | 合批跨透明对象会改变画面 | 只合并相邻连续 record；页/capture/回退 barrier；不排序 |
| 线 ID 顺序 | Chart 线顺序变化会让 record 错位 | `before_line` 校验 `order[cursor]`；`install` 校验 `supported_ids`；不匹配回退 |
| 低画质权限 | 高画质使用不同采样/材质 | `capability` 强制 `sample_count==1`；`install` 再检查 |

#### 10.3 未验证项

- 全曲 162.7 s 的完整线 GPU 路径（本 cohort 只覆盖约 62–82 s）。
- 其他 GPU/驱动（本 cohort 只有 Mali-T880）。
- 手动输入、真实 Main 首次 Playing 前 5 s、暂停/重试/多 UI 生命周期。
- 同一 managed context 内的 native EGL surface 丢失与恢复。
- shader/pipeline 跨 context 的完整所有权与泄漏证明。
- 音符 GPU blob；P-01 不包含。
- 真实 Settings UI 点击、完整主应用重入；目前只有源码接线和有限 probe。
- 透明线 DCE；P-01 仍绘制 4639 条 alpha=0 线。

#### 10.4 README 版本漂移（与本条相关）

`本地证据/low-quality-normal/README.md` 第 9 行把“最新源码提交”写成 `9498c734`，但参考实现 head 是 `be4e1305`；两者之间相差 40 个提交（`git rev-list --count 9498c734..be4e1305` = 40）。README 第 24 行还写“由独立Cargo feature启用，默认构建关闭”，这在 `9498c734` 时是真的（当时 `phira` 的 `default=["video"]`），但在 head 已经过时：`phira/Cargo.toml` 的默认 feature 已含 `chart_pre_compile`。因此：

- README 的测量数据仍然有效（它对应 9498c734/compiler-v1-retry）。
- README 的“默认构建关闭”不能拿来描述 head 的构建事实。
- 本文档把“编译期默认编入”和“运行期默认关闭”分开写，避免继续传播这个冲突。

### 11. 落地步骤（可执行清单）

如果要把 P-01 推进到主线，按以下顺序执行；每一步都必须保留可回退路径。

- [ ] 在参考实现中确认 `chart_pre_compile` 的默认 feature 策略：是继续“默认编入、运行关闭”，还是改为显式 feature；同步 `phira`、`phira-main`、Android Gradle 与文档。
- [ ] 把 `capability` 的低画质条件与 `Config.chart_pre_compile` 的 UI 文案对齐；确认高画质/aggressive/shader_pre_render 不会隐式打开线 GPU 路径。
- [ ] 在真实 Main UI 中用实际点击打开/关闭开关，验证 `data.json`、cache size、clear、disable/re-enable、进程重启。
- [ ] 跑完整 162.7 s 谱面的同 SO ABBA，保留 120 预热 + 全部真实帧；报告应用墙钟、p99、max、>30 ms、呈现间隔、chart 阶段和 render 阶段。
- [ ] 在同一 SO 上验证 `sample_count=2`、`chart_debug`、非默认材质、capture、depth 场景全部回退，且 RGBA 与原路径一致。
- [ ] 验证 EGL surface loss/context 重建：至少证明不会删除新 context 的同名对象；无法证明时保持 `context_id` 回退并记录。
- [ ] 用真实设备做 20 轮以上 Chart 重入，检查 shader/pipeline/arena 计数不增长，pending resident draw 不丢。
- [ ] 用至少两种 GPU（Mali + Adreno 或不同 Mali）重复有限窗口 RGBA 与性能；不能把 Mali-T880 结果外推。
- [ ] 把质量类从 `mild-lossy` 升级为“无损”之前，必须给出全帧/全设备/全材质证据；否则保持低画质候选。
- [ ] 更新 `本地证据/AGENTS.md`/README 的默认值与版本，避免继续出现“默认关闭”与 Cargo 默认 feature 的矛盾。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `be4e1305823bfebdad1853d7b333e0f79c3ead79`（2026-10-24） |
| 线 GPU IR 提交 | `2c31be7f01286a84cab1b65adcf5b91571c4f7b1`（编译普通判定线批次，2026-10-07 17:28:43 +0800） |
| 资源生命周期提交 | `9913d98b`（管理谱面渲染资源，2026-10-07 18:34:16 +0800） |
| 生产开关提交 | `9498c73427a8e5d88631826267cb562246590ca0`（接入谱面预编译开关，2026-10-07 18:59:37 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/render_compiler.rs`（173 行）、`prpr/src/render_lab/lines.rs`（859 行）、`prpr/src/render_lab/cache.rs`（350 行）、`prpr/src/core/chart.rs`、`prpr/src/core/line.rs`、`prpr/src/scene/game.rs`、`prpr/src/config.rs`、`prpr/Cargo.toml`、`phira/Cargo.toml`、`phira-main/Cargo.toml` |
| 测量数据 | `本地证据/low-quality-normal/compiler-v1-retry-performance-summary.json`；`lines-v1-performance-summary.json`；`lines-v1-quality-*`；`compiler-v1-quality-*` |
| 设备 | EDI-AL10 / Mali-T880 / Android 8 / 2560×1440 |
| 谱面 | `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip` |
| 说明 | 本文档正文已包含关键代码、表格与结论；上述路径只用于追溯，不是阅读前提 |

---

## P-02 谱面尾工作集 tail workset

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-12（796e3835、84422d41）；2026-10-14（241f55dd 生产接线）　参考实现提交：796e3835、84422d41、241f55dd　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计，不冒充独立验收）

### 1. 一句话结论

尾工作集把普通音符按“源顺序、above/below、同 speed”切成组，在加载期预计算每组的高度有序序列、静态 x 区间和 x 后缀包围；运行时用整尾公共轴证明直接跳到第一个仍可能可见的音符，只重放末尾 `init_ctrl`，不再逐帧遍历整段必然出屏的未来尾部。真实 Main 25 s 窗口内，低画质应用墙钟 **9.04240682→6.27222914 ms（−30.635%）**、chart 阶段 **4.65696413→1.81280745 ms（−61.073%）**；普通画质 **10.99071212→8.48172562 ms（−22.828%）**、chart **5.43567587→2.75849372 ms（−49.252%）**。但 FPS 没有实质收益，普通画质的 p99/max/>30 ms 反而变差；因此判定是**正优化·局部**，不是整帧胜利。

### 2. 问题与瓶颈（为什么原路径慢）

原 `JudgeLine::render` 在每条线的每个 speed 游标 `index` 上，从 `self.notes[*index..]` 开始逐条 `note.render`，直到 above/below 或 speed run 结束、或 aggressive 的 `limit` 命中；下方 §4.3 给出了完整源码。这段循环有两个层次的问题：

1. **只跳过“已判定过去前缀”**。`JudgeLineCache::above_indices/below_indices` 的推进逻辑只在没有 `show_below`、没有 PE alpha 扩展时，把已经过去的音符游标向前推；它没有“远端终点”的概念。也就是说，当前时间之后、但已经远离屏幕的未来音符仍会被逐条访问。
2. **每个未来音符仍进入 `Note::render`**。即使最终被 CPU AABB 拒绝，`Note::render` 仍要执行 `set_height`、时间/age 判断、`init_ctrl_obj`、矩阵/角点计算；这些工作在“必然不可见”的尾部上是纯浪费。

源码审计给出的具体数字：这张谱面 line0 有 1029 个 future plain 音符，首事件 ≥13.150685 s，部分区间的姿态在 2.191781–12.328767 s 内恒定、height 线性；在 60/62/64 s 时分别有 915/899/876 个 plain 音符仍位于未来。`aggressive=false` 时，原路径会先完整算 matrix，再由 `draw_tex_pts` 的 CPU AABB 排除；GPU 根本没有收到这些 quad。所以优化空间不是“少画”，而是**少算**。

原有 `RenderCache` 只负责“已判定过去前缀”的游标推进，没有对远端尾部做保守证明；`aggressive` 的屏幕外剔除又要求 `aggressive=true`，而普通画质默认 `aggressive=false`。尾工作集补的正是这个缺口：在**不打开 aggressive**、不改变任何判定/音频/pose 的前提下，用加载期证书证明整段尾部不可见。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

**核心不变量：整段尾部在同一个轴向上必然出屏。** 对一组普通音符，源码按原始 `JudgeLineCache::reset` 的边界切分：

- 从第一个 `plain()` 音符开始；
- 先处理 `above=true` 的连续同 speed 段；
- 再处理 `below` 的连续同 speed 段；
- 不把一个不支持的成员从某个 speed run 中拆出来“制造资格”。

对每个候选组，要求：

- 组内至少 4 个音符（`MIN_GROUP_NOTES`）；
- 组内所有音符 `plain()`、非 `fake`、非 hold、alpha/scale/rotation/y translation 都是默认；
- x 是静态的（`AnimFloat` 没有 `next`，keyframes ≤1，cursor=0）；
- height 有限且按源顺序非降；
- speed 是有限正数；
- 组不跨 above/below，不跨 speed；
- 组内音符 source ID 连续、ordered。

**两层证书：**

1. **X 后缀包围**：从组尾向前做 `suffix_x_lo/hi`，`extend_bounds` 用 `f32::total_cmp` 保留 `-0` 低端、`+0` 高端。这样任意 `[offset, last]` 的 x 范围都有确定包围。
2. **高度区间**：`source_base(height, aspect, spd, line_height)` 保留原 `Note` 渲染的运算顺序 `height / aspect * spd - line_height`，不重关联成 `(height - line_height) / aspect * spd`；因为大数相消时两种顺序的 f32 结果不同。组内 height 非降，所以 `[start, last]` 的 base 区间是 `[base(start), base(last)]`。

**整尾证明：** `Side::outside_range(x_lo, x_hi, base_lo, base_hi, hfs)` 在**同一个轴**上证明“所有 x/base 组合、所有 style 的 CPU 四角 AABB 都完全在 `[-1,1]^2` 外”。它返回 `true` 才表示整段尾部可跳过；`false` 表示没有证明；`None` 表示域不支持/非有限/溢出，也要回退。点级 `outside` 的 OR 不能当作二分谓词，因为不同点可能靠不同轴拒绝；`outside_range` 的公共轴包含关系才是单调的。

**为什么可以二分：** 随着 offset 增大，x 后缀区间只会缩小，height 区间只会缩小；`outside_range` 对区间收缩是包含单调的。因此 `proof(mid)` 为真时，`mid` 右侧（更大的 offset）也都为真；`first_proved_tail` 可以二分找到第一个“整尾已出屏”的 offset。

**为什么不会改变画面：**

- 被跳过的 `[end, group_end)` 全部由 `outside_range` 证明不可见；审计模式还会逐音符重放原 `workset_outside_witness`，`false_culls` 必须为 0。
- 末音符 `terminal_note_id` 仍执行原 `init_ctrl_obj`，保持 `CtrlObject` 的最终高度/时钟状态；这对后续线/特效/判定读取是必要的。
- 原 `JudgeLine` 的顺序、`above/below` 分支、speed run 边界、`show_below`、`appear_before`、`incline_sin`、`config.line_height`、`aspect_ratio` 都保留；任何一项不满足就 `None` 回退。
- 跳过发生在 `note.render` 之前，不改变 `Note` 的 judge/audio/time；只少了“本来就被 CPU AABB 拒绝”的矩阵/角点工作。

**为什么是局部：** chart 阶段省的是“未来尾部的逐 note 遍历/矩阵”，不是整帧；FPS 仍受 60 上限、呈现/驱动/粒子/HUD 等共同影响。低画质 25 s 窗口 FPS 只从 59.758 到 59.777（+0.032%），普通画质 ON 轮次甚至更低；所以只能判定为 chart 局部的正优化。

### 4. 具体实现

#### 4.1 文件与符号清单（含行数、函数/结构名）

| 文件（head `be4e1305`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/render_workset.rs` | 798 | `ENABLED/AUDIT`（20-23）、`execution_enabled`（33）、`GroupIndex`（58）、`Counters`（65）、`Prepared`（74）、`Cut`（83）、`cutoff`（138）、`terminal_init_time`（259）、`source_base`（265）、`first_proved_tail`（276）、`note_certificate`（345）、`SourceGroups`（366）、`eligible_group`（401）、`capture`（427）、`index`（503）、`prepare`（530） | 尾工作集的资格、证书、缓存、运行时 cutoff |
| `prpr/src/render_workset_ir.rs` | 535 | `MAGIC`（12）、`ABI`（14）、`MAX_BYTES`（16）、`Group`（26）、`Workset`（67）、`cache_key`（84）、`validate`（109）、`compile`（206）、`encode`（215）、`decode`（270） | PHRWS001 二进制格式、16 MiB 预算、损坏/旧源拒绝 |
| `prpr/src/visibility_bounds.rs` | 888 | `Interval`（5）、`down/up`（11/17）、`outward`（49）、`Side`（111）、`outside`（150）、`outside_range`（193）、`outside_fast`（258） | 保守 f32 区间/仿射证明；P-02 用 `outside_range` |
| `prpr/src/core/line.rs` | — | `above_indices` 循环（604-645）、`below_indices` 循环（669-710）、`workset_verify_cut`（720-735） | 在真实渲染循环中消费 cutoff |
| `prpr/src/core/note.rs` | — | `workset_outside_witness`（196-235） | 审计时独立重放原四角 AABB 拒绝 |
| `prpr/src/scene/game.rs` | — | `ChartOwnerState`（394-422）、`ChartRenderScope`（426-441）、`workset`（458-461）、`chart_mut/chart_parts_mut`（530-540）、`chart_render_scope`（550-566）、加载期 `workset::prepare`（718-729）、`Chart::render_scoped`（2003-2008） | owner/lease、可变访问撤销、真实 Main 消费者 |
| `prpr/src/core/chart.rs` | — | `render_scoped`（263）、`render_inner`（267） | 把 owner 作用域内的 chart+workset 配对 |
| `prpr/src/render_compiler.rs` | 173 | `workset::prepare` 调用（P-02 接线）、`cache()` | 共享 cache 与加载入口 |
| `prpr/src/config.rs` | 829 | `chart_pre_compile`（164）、默认 `false`（295） | 独立运行开关，不新增开关 |
| `prpr/Cargo.toml` / `phira/Cargo.toml` / `phira-main/Cargo.toml` | — | `chart_pre_compile` feature | 默认编入；运行默认关闭 |

#### 4.2 数据结构与算法（逐步说明）

1. **源分组**：`SourceGroups::new(line)` 从第一个 `plain()` 音符开始；`next()` 按 above/below、同 `speed`、同 above 状态切 run。它不排序、不拆组。
2. **资格检查**：`eligible_group` 要求长度 ≥4、`end <= MAX_NOTES`、同 speed bits、所有 `note_certificate` 通过、height 非降。
3. **静态证书**：`note_certificate` 只接受 `plain()`、非 fake、有限 height/speed/time、默认 alpha/scale/rotation/y translation、静态 x、Click/Drag/Flick（含 multiple_hint 的 3/4/5 role）。任何不满足的音符会让整组不合格，而不是把该音符单独剔除。
4. **描述符哈希**：`capture()` 把 `DESCRIPTOR_ABI`、IR ABI、线数、每条线的 line_id/notes.len/ctrl_default、每个音符的 id/plain/above/fake/multiple_hint/height bits/speed bits/time bits/kind、x/role 证书、每个 SourceGroup 的 start/end/above/eligible 全部写进 SHA-256。这个 descriptor 反映**实际加载后的静态形态**，但排除 judge/time cursor/ctrl 等运行态；`loaded_descriptor` 变化会换 key，运行态变化不会让 blob 失效。
5. **逻辑预算**：`charge()` 在分配前累计 `Group`/索引/notes 数组/suffix 数组/序列化 body 的字节数，上限 `MAX_BYTES = 16 MiB`；`reserved()` 用 `try_reserve_exact`，溢出/不足直接回退。
6. **建组**：`groups` 先按 groups_count 预分配，再逐组填 `note_ids/heights_bits/x_bits/style_roles`；`ir::Group::new` 后由 `ir::compile` 做结构校验并重算 suffix。
7. **缓存读写**：`prepare()` 用 `ir::cache_key(source, descriptor)` 取 key；`read_bounded(ticket, MAX_BYTES)` 读前先限制；`decode(bytes, &expected)` 用**当前现场 capture 的 expected shape**逐组、逐音符比对 line_id/above/speed/count/IDs/heights/x/role，再重算 suffix 并与 blob 内证书比对；任何一项不符就 `invalidate_blob` 后重编译。
8. **发布**：冷路径 `ir::encode(&expected)`，`publish` 与 P-01 共用 `RenderCache`；epoch 不匹配返回 false，不重建目录。发布失败只记录 `cache_error`，仍使用内存中的 plan。
9. **索引**：`index()` 把每组压成 `GroupIndex { line, start, end, styles }`，要求按 `(line,start)` 严格递增；运行时 `lookup` 用 `partition_point` 做 O(log N) 定位，再校验 cursor 在组内且 above 匹配。
10. **运行时 cutoff**：`cutoff()` 先检查 `execution_enabled`、reads/visibility 诊断、`chart_pre_compile`、aggressive、极端线、debug、`appear_before`、`incline_sin`、ctrl 默认、有限 time/aspect/flow_speed/line_height；再 `lookup`；再要求末音符未被判定、time 有限且 ≥当前时间、未被 FADEOUT 提前排除、`terminal_init_time` 非零；再算 `spd = speed * flow_speed * 1.0`、`line_height = config.line_height / aspect * spd`；再根据组的 style mask 取 1–6 个 half extents；最后 `first_proved_tail` 二分。
11. **运行时消费**：`line.rs` 在 `above_indices`/`below_indices` 循环里调用 `cutoff`，把 `count = cut.end - index`；循环只跑到 `cut.end`，然后对 `terminal_note_id` 执行 `init_ctrl_obj`，再 `record_cut(terminal_note_id + 1 - cut.end)`。below 分支同理。
12. **审计**：`workset_verify_cut` 只在 `audit_enabled` 时逐音符调用 `workset_outside_witness`，它独立重放原 `Note` 的四角 AABB 拒绝，返回 false 就 `false_culls += 1` 并拒绝这次 cut。性能测量固定 `audit=false`，所以计数器真实为 0，不能用“0 计数”反推没有消费；消费由 quality 的 `accepted/proved/witness` 证明。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

以下代码取自 `be4e1305` 的 `prpr/src/render_workset.rs`、`prpr/src/core/line.rs`、`prpr/src/core/note.rs`。为便于阅读按主题分段，函数体未删行。

**数据结构与 cutoff `prpr/src/render_workset.rs:58-90, 126-253`：**

```rust
#[derive(Clone, Copy)]
struct GroupIndex {
    line: u32,
    start: u32,
    end: u32,
    styles: u8,
}
#[derive(Default)]
struct Counters {
    attempts: Cell<u64>,
    proved: Cell<u64>,
    accepted: Cell<u64>,
    skipped: Cell<u64>,
    witnesses: Cell<u64>,
    false_culls: Cell<u64>,
}
pub(crate) struct Prepared {
    plan: ir::Workset,
    lease: CacheLease,
    index: Vec<GroupIndex>,
    counters: Counters,
    preparation: Value,
}
/// Render [current_cursor, end), then replay the unchanged init helper for
/// terminal_note_id. The proved suffix [end, group_end) produces no vertices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cut {
    pub end: usize,
    pub terminal_note_id: usize,
}
fn increment(value: &Cell<u64>, amount: u64) {
    value.set(value.get().saturating_add(amount));
}
impl Prepared {
    pub(crate) fn is_current(&self) -> bool {
        self.lease.is_current()
    }
    pub(crate) fn audit_enabled(&self) -> bool {
        audit_enabled()
    }
    pub(crate) fn record_cut(&self, skipped: usize) {
        if audit_enabled() {
            increment(&self.counters.accepted, 1);
            increment(&self.counters.skipped, skipped as u64);
        }
    }
    pub(crate) fn record_witness(&self, valid: bool) {
        if audit_enabled() {
            increment(&self.counters.witnesses, 1);
            if !valid {
                increment(&self.counters.false_culls, 1);
            }
        }
    }
    fn lookup(&self, line: usize, cursor: usize, above: bool) -> Option<(&ir::Group, GroupIndex)> {
        let key = (u32::try_from(line).ok()?, u32::try_from(cursor).ok()?);
        let position = self.index.partition_point(|group| (group.line, group.start) <= key).checked_sub(1)?;
        let index = self.index[position];
        if index.line != key.0 || key.1 < index.start || key.1 >= index.end || self.plan.groups[position].above != above {
            return None;
        }
        Some((&self.plan.groups[position], index))
    }
    pub(crate) fn cutoff(
        &self,
        line_id: usize,
        cursor: usize,
        above: bool,
        line: &JudgeLine,
        res: &Resource,
        config: &RenderConfig<'_>,
        side: Side,
    ) -> Option<Cut> {
        if !execution_enabled() {
            return None;
        }
        let reads = super::reads::execution();
        let visibility = super::visibility::execution();
        if reads.enabled || reads.audit || visibility.enabled || visibility.audit {
            return None;
        }
        if audit_enabled() {
            increment(&self.counters.attempts, 1);
        }
        if !res.config.chart_pre_compile
            || res.config.aggressive
            || line.notes.len() > EXTREME_LINE_NOTES
            || res.config.chart_debug
            || res.config.chart_debug_line
            || res.config.chart_debug_note
            || config.read_execution.audit
            || config.read_execution.enabled
            || config.visibility_execution.audit
            || config.visibility_execution.enabled
            || config.appear_before != f64::INFINITY
            || config.incline_sin != 0.
            || !super::visibility::ctrl_default(config.ctrl_obj)
            || !res.time.is_finite()
            || !res.aspect_ratio.is_normal()
            || res.aspect_ratio <= 0.
            || !res.config.flow_speed.is_normal()
            || res.config.flow_speed <= 0.
            || !normal_or_zero(config.line_height)
        {
            return None;
        }
        let (group, index) = self.lookup(line_id, cursor, above)?;
        let terminal_note_id = index.end as usize - 1;
        let terminal = line.notes.get(terminal_note_id)?;
        if matches!(terminal.judge, JudgeStatus::Judged)
            || !terminal.time.is_finite()
            || terminal.time < res.time
            || (!config.draw_below && res.time - FADEOUT_TIME >= terminal.time)
            || terminal_init_time(terminal.height, config.line_height, terminal.speed).is_none()
        {
            return None;
        }
        let speed = f64::from_bits(group.speed_bits);
        let spd = speed * res.config.flow_speed as f64 * 1.0f64;
        if !spd.is_normal() || spd <= 0. {
            return None;
        }
        let aspect = res.aspect_ratio as f64;
        let line_height = config.line_height / aspect * spd;
        if !normal_or_zero(line_height) {
            return None;
        }
        let mut hfs = [[0.; 2]; 6];
        let mut styles = 0;
        for role in 0..6 {
            if index.styles & (1 << role) == 0 {
                continue;
            }
            let hint = res.config.double_hint && role >= 3;
            let style = if hint { &res.res_pack.note_style_mh } else { &res.res_pack.note_style };
            let texture = match role % 3 {
                0 => &style.click,
                1 => &style.drag,
                _ => &style.flick,
            };
            let click_width = res.res_pack.note_style.click.width();
            if !click_width.is_normal() || click_width <= 0. {
                return None;
            }
            let scale = (if hint {
                res.res_pack.note_style_mh.click.width() / click_width
            } else {
                1.0
            }) * res.note_width;
            let width = texture.width();
            let height = texture.height();
            if !width.is_normal() || width <= 0. || !height.is_finite() || height < 0. {
                return None;
            }
            let half = [scale, height * scale / width];
            if !half.iter().all(|value| value.is_finite() && *value >= 0.) {
                return None;
            }
            hfs[styles] = half;
            styles += 1;
        }
        let offset = cursor.checked_sub(index.start as usize)?;
        let relative = first_proved_tail(group, offset, aspect, spd, line_height, res.aspect_ratio, &hfs[..styles], side)?;
        let end = (index.start as usize).checked_add(relative)?;
        if audit_enabled() {
            increment(&self.counters.proved, 1);
        }
        Some(Cut { end, terminal_note_id })
    }
}
```

**磁盘/内存格式 `prpr/src/render_workset_ir.rs:25-72`：**

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub line_id: u32,
    pub above: bool,
    pub speed_bits: u64,
    pub note_ids: Vec<u32>,
    pub heights_bits: Vec<u64>,
    pub x_bits: Vec<u32>,
    pub style_roles: Vec<u8>,
    pub suffix_x_lo_bits: Vec<u32>,
    pub suffix_x_hi_bits: Vec<u32>,
}
impl Group {
    pub fn new(
        line_id: u32,
        above: bool,
        speed_bits: u64,
        note_ids: Vec<u32>,
        heights_bits: Vec<u64>,
        x_bits: Vec<u32>,
        style_roles: Vec<u8>,
    ) -> Self {
        Self {
            line_id,
            above,
            speed_bits,
            note_ids,
            heights_bits,
            x_bits,
            style_roles,
            suffix_x_lo_bits: Vec::new(),
            suffix_x_hi_bits: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workset {
    pub source: [u8; 32],
    pub descriptor: [u8; 32],
    pub groups: Vec<Group>,
}
```

**原运算次序与整尾二分 `prpr/src/render_workset.rs:256-323`：**

```rust
fn normal_or_zero(value: f64) -> bool {
    value == 0. || value.is_normal()
}
fn terminal_init_time(height: f64, line_height: f64, speed: f64) -> Option<f64> {
    let value = (height - line_height + 0.0f32 as f64 / speed) * RPE_HEIGHT as f64 / 2.;
    (value.is_finite() && value != 0.).then_some(value)
}
fn source_base(height: f64, aspect: f64, spd: f64, line_height: f64) -> Option<f32> {
    let scaled_height = height / aspect * spd;
    let value = scaled_height - line_height;
    if !normal_or_zero(scaled_height) || !normal_or_zero(value) {
        return None;
    }
    let value = value as f32;
    value.is_finite().then_some(value)
}
fn first_proved_tail(
    group: &ir::Group,
    start: usize,
    aspect: f64,
    spd: f64,
    line_height: f64,
    aspect_f32: f32,
    hfs: &[[f32; 2]],
    side: Side,
) -> Option<usize> {
    let last = group.note_ids.len().checked_sub(1)?;
    if start > last {
        return None;
    }
    let lower = source_base(f64::from_bits(*group.heights_bits.get(start)?), aspect, spd, line_height)?;
    let upper = source_base(f64::from_bits(*group.heights_bits.get(last)?), aspect, spd, line_height)?;
    if lower > upper || !(lower.abs().max(upper.abs()) * aspect_f32).is_finite() {
        return None;
    }
    let proof = |offset: usize| -> Option<bool> {
        let base = source_base(f64::from_bits(*group.heights_bits.get(offset)?), aspect, spd, line_height)?;
        side.outside_range(
            f32::from_bits(*group.suffix_x_lo_bits.get(offset)?),
            f32::from_bits(*group.suffix_x_hi_bits.get(offset)?),
            base,
            upper,
            hfs,
        )
    };
    if !proof(last)? {
        return None;
    }
    let (mut lo, mut hi) = (start, last);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if proof(mid)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}
```

**资格、静态证书与源分组 `prpr/src/render_workset.rs:325-414`：**

```rust
fn static_x(track: &AnimFloat) -> Option<f32> {
    if track.next.is_some() {
        return None;
    }
    let value = match track.keyframes.len() {
        0 => 0.,
        1 if track.cursor == 0 => track.keyframes[0].value,
        _ => return None,
    };
    value.is_finite().then_some(value)
}
fn role(note: &Note) -> Option<u8> {
    let kind = match note.kind {
        NoteKind::Click => 0,
        NoteKind::Drag => 1,
        NoteKind::Flick => 2,
        _ => return None,
    };
    Some(kind + if note.multiple_hint { 3 } else { 0 })
}
fn note_certificate(note: &Note) -> Option<(f32, u8)> {
    if !note.plain()
        || note.fake
        || !note.height.is_finite()
        || !note.speed.is_normal()
        || note.speed <= 0.
        || !note.time.is_finite()
        || !note.object.alpha.is_default()
        || !note.object.scale.0.is_default()
        || !note.object.scale.1.is_default()
        || !note.object.rotation.is_default()
        || !note.object.translation.1.is_default()
    {
        return None;
    }
    Some((static_x(&note.object.translation.0)?, role(note)?))
}
struct SourceGroups<'a> {
    notes: &'a [Note],
    cursor: usize,
    above: bool,
}
impl<'a> SourceGroups<'a> {
    fn new(line: &'a JudgeLine) -> Self {
        Self {
            notes: &line.notes,
            cursor: line.notes.iter().position(Note::plain).unwrap_or(line.notes.len()),
            above: true,
        }
    }
}
impl Iterator for SourceGroups<'_> {
    type Item = (usize, usize, bool);
    fn next(&mut self) -> Option<Self::Item> {
        let first = self.notes.get(self.cursor)?;
        if self.above && !first.above {
            self.above = false;
        }
        let above = self.above;
        let start = self.cursor;
        let speed = first.speed;
        self.cursor += 1;
        while self
            .notes
            .get(self.cursor)
            .is_some_and(|note| (!above || note.above) && note.speed == speed)
        {
            self.cursor += 1;
        }
        Some((start, self.cursor, above))
    }
}
fn eligible_group(line: &JudgeLine, start: usize, end: usize, above: bool) -> bool {
    if end - start < MIN_GROUP_NOTES || end > ir::MAX_NOTES {
        return false;
    }
    let speed = line.notes[start].speed.to_bits();
    let mut previous = f64::NEG_INFINITY;
    for note in &line.notes[start..end] {
        if note.above != above || note.speed.to_bits() != speed || note_certificate(note).is_none() || note.height < previous {
            return false;
        }
        previous = note.height;
    }
    true
}
```

**加载捕获 `prpr/src/render_workset.rs:427-502`：**

```rust
fn capture(chart: &Chart, source_bytes: &[u8]) -> Result<ir::Workset> {
    ensure!(chart.lines.len() <= ir::MAX_LINES, "Tail workset line count exceeded");
    let mut descriptor = Sha256::new();
    descriptor.update(DESCRIPTOR_ABI);
    descriptor.update(ir::ABI.to_le_bytes());
    descriptor.update(u64::try_from(chart.lines.len())?.to_le_bytes());
    let (mut groups_count, mut notes_count, mut all_notes) = (0usize, 0usize, 0usize);
    let mut payload = size_of::<ir::Workset>();
    for (line_id, line) in chart.lines.iter().enumerate() {
        all_notes = all_notes.checked_add(line.notes.len()).context("Tail source count overflow")?;
        ensure!(all_notes <= ir::MAX_NOTES, "Tail source note count exceeded");
        descriptor.update(u32::try_from(line_id)?.to_le_bytes());
        descriptor.update(u64::try_from(line.notes.len())?.to_le_bytes());
        descriptor.update([super::visibility::ctrl_default(&line.ctrl_obj.borrow()) as u8]);
        for (note_id, note) in line.notes.iter().enumerate() {
            descriptor.update(u32::try_from(note_id)?.to_le_bytes());
            descriptor.update([note.plain() as u8, note.above as u8, note.fake as u8, note.multiple_hint as u8]);
            descriptor.update(note.height.to_bits().to_le_bytes());
            descriptor.update(note.speed.to_bits().to_le_bytes());
            descriptor.update(note.time.to_bits().to_le_bytes());
            descriptor.update([match note.kind {
                NoteKind::Click => 0,
                NoteKind::Drag => 1,
                NoteKind::Flick => 2,
                NoteKind::Hold { .. } => 3,
            }]);
            match note_certificate(note) {
                Some((x, role)) => {
                    descriptor.update([1, role]);
                    descriptor.update(x.to_bits().to_le_bytes());
                }
                None => descriptor.update([0]),
            }
        }
        for (start, end, above) in SourceGroups::new(line) {
            let eligible = eligible_group(line, start, end, above);
            descriptor.update(u32::try_from(start)?.to_le_bytes());
            descriptor.update(u32::try_from(end)?.to_le_bytes());
            descriptor.update([above as u8, eligible as u8]);
            if !eligible {
                continue;
            }
            groups_count = groups_count.checked_add(1).context("Tail group count overflow")?;
            notes_count = notes_count.checked_add(end - start).context("Tail note count overflow")?;
            ensure!(groups_count <= ir::MAX_GROUPS && notes_count <= ir::MAX_NOTES, "Tail workset count exceeded");
            charge(&mut payload, 1, size_of::<ir::Group>() + size_of::<(u32, u32, u32)>() + size_of::<GroupIndex>())?;
            charge(&mut payload, end - start, size_of::<u32>() * 4 + size_of::<u64>() + size_of::<u8>())?;
        }
    }
    let mut groups = reserved(groups_count)?;
    for (line_id, line) in chart.lines.iter().enumerate() {
        for (start, end, above) in SourceGroups::new(line) {
            if !eligible_group(line, start, end, above) {
                continue;
            }
            let count = end - start;
            let mut ids = reserved(count)?;
            let mut heights = reserved(count)?;
            let mut xs = reserved(count)?;
            let mut roles = reserved(count)?;
            for (offset, note) in line.notes[start..end].iter().enumerate() {
                let (x, role) = note_certificate(note).context("Tail source changed while capturing")?;
                ids.push(u32::try_from(start + offset)?);
                heights.push(note.height.to_bits());
                xs.push(x.to_bits());
                roles.push(role);
            }
            groups.push(ir::Group::new(u32::try_from(line_id)?, above, line.notes[start].speed.to_bits(), ids, heights, xs, roles));
        }
    }
    ir::compile(Sha256::digest(source_bytes).into(), descriptor.finalize().into(), groups)
}
```

**加载入口 `prpr/src/render_workset.rs:530-603`：**

```rust
pub(crate) fn prepare(chart: &Chart, res: &Resource, source_bytes: &[u8]) -> (Option<Prepared>, Value) {
    super::set_enabled(res.config.chart_pre_compile);
    if !res.config.chart_pre_compile {
        return (None, json!({"requested":false,"installed":false,"cache_read":false,"cache_write":false}));
    }
    let begun = Instant::now();
    let (mut cache_read, mut cache_write) = (false, false);
    let result = (|| -> Result<(Prepared, Value)> {
        let manager = super::cache().context("Host has not configured the compiler cache")?;
        let expected = capture(chart, source_bytes)?;
        let key = ir::cache_key(expected.source, expected.descriptor);
        let ticket = manager.begin(&key)?.context("Compiler cache disabled")?;
        cache_read = true;
        let (mut cache_hit, mut published, mut cache_error) = (false, false, Value::Null);
        let cached = match manager.read_bounded(&ticket, ir::MAX_BYTES) {
            Ok(Some(bytes)) => match ir::decode(&bytes, &expected) {
                Ok(plan) => {
                    cache_hit = true;
                    Some((plan, bytes.len()))
                }
                Err(error) => {
                    cache_error = json!(format!("{error:#}"));
                    manager.invalidate_blob(&ticket)?;
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                cache_error = json!(format!("{error:#}"));
                manager.invalidate_blob(&ticket)?;
                None
            }
        };
        let (plan, bytes) = match cached {
            Some(value) => value,
            None => {
                let blob = ir::encode(&expected)?;
                cache_write = true;
                match manager.publish(&ticket, &blob) {
                    Ok(value) => published = value,
                    Err(error) => cache_error = json!(format!("{error:#}")),
                }
                (expected, blob.len())
            }
        };
        ensure!(manager.is_current(&ticket), "Tail workset invalidated by cache clear/disable");
        let lookup = index(&plan)?;
        let groups = plan.groups.len();
        let notes: usize = plan.groups.iter().map(|group| group.note_ids.len()).sum();
        let report = json!({"requested":true,"installed":true,"cache_hit":cache_hit,"published":published,
            "cache_read":cache_read,"cache_write":cache_write,"cache_error":cache_error,"key":key,"bytes":bytes,
            "groups":groups,"notes":notes,"root":manager.root(),"cpu_blob_persisted":cache_hit || published,
            "execution_enabled":execution_enabled(),"lossless":true,"scope":"Owner-sealed canonical plain-note tails; original judge/audio/control initialization."});
        let prepared = Prepared {
            plan,
            lease: CacheLease::new(manager.clone(), ticket),
            index: lookup,
            counters: Counters::default(),
            preparation: report.clone(),
        };
        Ok((prepared, report))
    })();
    let (mut plan, mut report) = match result {
        Ok((plan, report)) => (Some(plan), report),
        Err(error) => {
            (None, json!({"requested":true,"installed":false,"cache_read":cache_read,"cache_write":cache_write,"fallback":format!("{error:#}")}))
        }
    };
    report["elapsed_ms"] = json!(begun.elapsed().as_secs_f64() * 1000.);
    if let Some(plan) = plan.as_mut() {
        plan.preparation = report.clone();
    }
    (plan, report)
}
```

**运行时消费（above 循环）`prpr/src/core/line.rs:604-645`：**

```rust
for index in &self.cache.above_indices {
    if *index >= self.notes.len() {
        continue;
    }
    let speed = self.notes[*index].speed;
    let limit = height_above as f64 / speed;
    #[cfg(feature = "chart_pre_compile")]
    let cut = workset
        .and_then(|plan| workset_side.and_then(|side| plan.cutoff(id, *index, true, self, res, &config, side)))
        .and_then(|cut| self.workset_verify_cut(workset.unwrap(), cut, res, &config));
    let count = {
        #[cfg(feature = "chart_pre_compile")]
        {
            cut.as_ref().map_or(usize::MAX, |cut| cut.end - *index)
        }
        #[cfg(not(feature = "chart_pre_compile"))]
        {
            usize::MAX
        }
    };
    for (offset, note) in self.notes[*index..].iter().enumerate().take(count) {
        if !note.above || speed != note.speed {
            break;
        }
        if agg && note.height - config.line_height + note.object.translation.1.now() as f64 > limit {
            break;
        }
        #[cfg(feature = "chart_pre_compile")]
        {
            config.read_note = read_line.and_then(|plan| plan.notes.get(*index + offset));
            config.visibility_note = visibility_line
                .and_then(|plan| plan.notes.get(*index + offset))
                .and_then(|plan| plan.as_ref());
        }
        note.render(res, &mut config, bpm_list);
    }
    #[cfg(feature = "chart_pre_compile")]
    if let Some(cut) = cut {
        self.notes[cut.terminal_note_id].init_ctrl_obj(config.ctrl_obj, config.line_height);
        workset.unwrap().record_cut(cut.terminal_note_id + 1 - cut.end);
    }
}
```

**审计重放 `prpr/src/core/note.rs:192-235`：**

```rust
#[cfg(feature = "chart_pre_compile")]
pub(crate) fn workset_outside_witness(&self, res: &Resource, config: &RenderConfig) -> bool {
    if matches!(self.judge, JudgeStatus::Judged) {
        return true;
    }
    if !config.draw_below && res.time - FADEOUT_TIME >= self.time {
        return true;
    }
    let spd = self.speed * res.config.flow_speed as f64 * config.ctrl_obj.y.now_opt().unwrap_or(1.) as f64;
    let line_height = config.line_height / res.aspect_ratio as f64 * spd;
    let height = self.height / res.aspect_ratio as f64 * spd;
    let base = height - line_height;
    if !config.draw_below && self.time > res.time && base <= -0.001 {
        return true;
    }
    let style = if res.config.double_hint && self.multiple_hint {
        &res.res_pack.note_style_mh
    } else {
        &res.res_pack.note_style
    };
    let tex = match self.kind {
        NoteKind::Click => *style.click,
        NoteKind::Drag => *style.drag,
        NoteKind::Flick => *style.flick,
        _ => return false,
    };
    let scale = (if res.config.double_hint && self.multiple_hint {
        res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
    } else {
        1.
    }) * res.note_width;
    let hf = vec2(scale, tex.height() * scale / tex.width());
    let combined = res.model_stack.last().unwrap() * self.now_transform(res, config.ctrl_obj, base as f32, config.incline_sin);
    let (x, y, w, h) = (-hf.x, -hf.y, hf.x * 2., hf.y * 2.);
    let p =
        [Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)].map(|point| combined.transform_point(&point));
    p.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) > 1.
        || p.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) < -1.
        || p.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) > 1.
        || p.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) < -1.
}
```

#### 4.4 调用链与生命周期（加载期→游玩期→清理/失效）

**加载期：**

1. `GameScene::new` 读入 `chart_bytes` 后，先 `workset::prepare(&chart, &res, &chart_bytes)`，再 `render_compiler::prepare`（P-01）。顺序很重要：workset 只做 CPU 准备，不依赖 GPU 能力；低画质/高画质都可以准备。
2. `workset::prepare` 先 `super::set_enabled(res.config.chart_pre_compile)`，同步 cache epoch；关闭时立即返回 `requested=false`，不读盘、不分配。
3. 开启时 `capture(chart, source_bytes)` 从**实际加载后的 Chart**生成 expected shape 和 descriptor；descriptor 包含静态音符字段、线数、ctrl 默认状态；不包含 judge/time cursor/运行态。
4. `cache().begin(key)` → `read_bounded(16 MiB)` → `ir::decode(bytes, &expected)`；命中后逐组/逐音符比对现场 shape，任何不一致就 invalidate 并重编译。
5. 冷路径 `ir::encode(&expected)` → `publish`；发布失败不阻断游玩，只在报告里写 `cache_error`。
6. `Prepared { plan, lease, index, counters, preparation }` 返回给 `GameScene`；`ChartOwnerState::prepared(workset.is_some())` 记录 `workset_generation=Some(0)`。
7. `GameScene.workset` 是私有字段；`chart` 也是私有字段。只有 `ChartRenderScope` 能把二者配对传出。`chart_mut()`/`chart_parts_mut()` 在返回可变引用前 `revoke_chart_workset()`，把 `workset=None` 并把 generation 递增；之后即使外部改了 Chart，也不会再拿旧 plan 去跳音符。

**游玩期：**

1. `GameScene::render` 调 `Self::chart_render_scope(&self.chart, &mut self.workset, &mut self.chart_owner, res.config.chart_pre_compile)`。
2. `chart_render_scope` 检查 `compiler_enabled`、`owner.permits_workset()`、`workset.is_current()`；任一失败就 `workset=None`、`workset_generation=None`，返回 None，走原 `Chart::render`。
3. 成功时 `Chart::render_scoped(&scope, ui, res)` → `render_inner(ui, res, Some(scope.workset()))`。
4. `Chart::render_inner` 调 `lines::begin_chart`（P-01），然后按 `chart.order` 逐线调用 `render_inner(ui, res, workset)`。
5. `JudgeLine::render_inner` 在计算 `workset_side = Side::new(res.model_stack.last())` 后，于 above/below 循环调用 `plan.cutoff(id, *index, true, self, res, &config, side)`；`Side::new` 只接受有限仿射矩阵（`(2,0)/(2,1)==0`、`(2,2)==1`）。
6. `cutoff` 返回 `Some(Cut { end, terminal_note_id })` 时，循环只跑到 `end`；随后对 `terminal_note_id` 执行原 `init_ctrl_obj(config.ctrl_obj, config.line_height)`，保持 CtrlObject 终态。
7. audit 模式额外逐音符 `workset_outside_witness`；性能模式 `audit=false`，不增加 witness 成本。

**清理/失效：**

1. 关闭 `chart_pre_compile`：`workset::prepare` 直接返回 None；`chart_render_scope` 的 `compiler_enabled=false` 让旧 plan 立即停用；原路径继续。
2. Settings clear：`RenderCache::clear_all()` 递增 epoch；下一次 `chart_render_scope` 发现 `!workset.is_current()`，丢弃 plan 并回原路径；重新加载谱面后重建。
3. 外部可变访问：`chart_mut`/`chart_parts_mut` 永久撤销本 scene 的 workset lease；`permanently_revoked` 或 generation 不匹配后不再复用。
4. Chart 重入/重试：`GameScene` 重新构造时重新 prepare；旧 plan 的 `CacheLease` 随 `Prepared` drop，旧 epoch 的 ticket 不再 current。
5. `context`/GPU 变化：workset 是 CPU-only blob，不含 GPU 句柄；真正 GPU 变化由 P-01 的 line renderer 处理。workset 本身只需要 cache epoch 与 Chart 静态形态。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| Cargo feature | `chart_pre_compile` | 与 P-01 共用；没有单独的 `tail_workset` feature |
| phira 默认 feature | `default = ["video", "font_atlas_pool", "chart_pre_compile"]` | 默认编入 |
| phira-main 默认 feature | `default = ["video", "chart_pre_compile"]` | 桌面主程序默认编入 |
| prpr 默认 feature | `default = ["log", "font_atlas_pool"]` | 单独 prpr 不默认编入 |
| 运行开关 | `Config.chart_pre_compile=false` | 关闭时 `workset::prepare` 直接返回 None |
| 研究原子开关 | `ENABLED=false`、`AUDIT=false` | 仅 `render_research`；`configure(enabled,audit)` 由研究请求设置 |
| 生产执行 | `execution_enabled()` 非 research 返回 `true` | 但 `cutoff` 仍要求 `Config.chart_pre_compile=true` 且无 debug/agg/诊断 |
| 旧 IR 互斥 | `cfg!(feature="render_chart_ir")` 时 false | 旧负收益音符 IR 开启时，workset 不执行 |
| 缓存 | 同一个 `RenderCache`，`read_bounded(16 MiB)` | 不新建 manager；与 P-01/Settings 共用 epoch/clear/size |
| 数据格式 | `PHRWS001` / `ABI=1` / `MAX_BYTES=16 MiB` | CPU-only；不含 GPU id、judge、clocks |
| 环境变量 | 无 | 只走 feature + Config + 研究请求 |
| 默认是否实际启用 | **编译期默认编入；运行期默认关闭** | 打开 `chart_pre_compile` 后才消费 |

`cutoff` 的回退条件必须逐项列出，因为这些条件决定了“为什么普通画质也不会改变画面”：

- `!res.config.chart_pre_compile` → 回退；
- `res.config.aggressive` → 回退（避免和原 aggressive 屏幕外剔除叠加）；
- `line.notes.len() > 20_000` → 回退（极端线保护）；
- `chart_debug`/`chart_debug_line`/`chart_debug_note` → 回退；
- `config.read_execution.audit/enabled`、`config.visibility_execution.audit/enabled` → 回退（诊断会改变读取/可见性）；
- `config.appear_before != f64::INFINITY` → 回退（PE alpha 扩展会画未来音符）；
- `config.incline_sin != 0` → 回退（倾斜会改变 AABB）；
- `!ctrl_default(config.ctrl_obj)` → 回退（非默认 ctrl）；
- `!res.time.is_finite()`、`!aspect_ratio.is_normal()`、`aspect_ratio<=0` → 回退；
- `!flow_speed.is_normal()`、`flow_speed<=0` → 回退；
- `!normal_or_zero(config.line_height)` → 回退；
- 末音符 Judged、time 非有限、time < 当前、FADEOUT 已过、`terminal_init_time` 为 0/非有限 → 回退；
- `spd` 非 normal/≤0、line_height 非 normal/zero → 回退；
- style 宽度非 normal/≤0、纹理高非有限/负、half extents 非有限 → 回退；
- `outside_range` 返回 false/None → 回退。

### 6. 测量方法与完整数据（完整表格；注明窗口、样本、设备、同 SO/ABBA）

#### 6.1 测量口径

- 设备/谱面：Mali-T880 / 2560×1440；谱面 `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip`（24 线、1333 个尾工作集 notes、8 groups）。
- 窗口：真实 Main 首次 Playing 后 25 s（不是全曲 162.7 s）；每轮 25 s 目标窗口，采样帧数随 FPS 变化（low 1494，normal 1488）。
- 交错：每组 OFF/ON/ON/OFF；low sample1，normal sample2；`audit=false`；Frozen/Small/PBO/CPU/GPU/native/trace 全部 false；common font、line DCE、`chart_pre_compile=true`、`aggressive=false` 固定；**唯一变量是 tail workset flag**。
- 同 SO：`b0a293c7bda1850abc267c36499bdee890522f4bc49f2ec16329fe38c4ec49c8`（source commit `4f573dfd`，63 源冻结）。实现提交是 `84422d4`，生产接线是 `241f55dd`；测量 SO 与 head 不是同一个，不能混池。
- 应用墙钟：`update+render+flush`；`chart_ms` 是 chart 阶段；`forward_wall` 是 begin-to-begin 的应用帧间隔，包含未分段的框架/native swap/yield。
- 呈现：独立 SF 10 s 左右，不是 25 s 全程；只能作为局部呈现证据。

#### 6.2 Low：真实 Main 25 s，OFF/ON/ON/OFF

| 轮次 | 路径 | 应用均值 ms | 应用 p99 ms | 应用 max ms | chart 均值 ms | chart p99 ms | chart max ms | FPS | 前向 p99 ms | 前向 max ms | >30 ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A1 | OFF | 9.346402 | 12.541182 | 14.696875 | 4.858176 | 6.071463 | 8.595833 | 59.745308 | 18.411260 | 26.163021 | 0 |
| B1 | ON | 6.333293 | 9.899156 | 14.482292 | 1.824765 | 2.468291 | 3.544271 | 59.774232 | 18.784750 | 22.239063 | 0 |
| B2 | ON | 6.211166 | 10.149969 | 15.725521 | 1.800850 | 2.356479 | 2.845834 | 59.779964 | 18.654813 | 25.454166 | 0 |
| A2 | OFF | 8.738412 | 12.708521 | 18.069270 | 4.455753 | 5.963802 | 7.026562 | 59.770681 | 19.196333 | 26.682292 | 0 |

- 应用组均：OFF `9.0424068203 ms`，ON `6.2722291395 ms`，减少 `2.7701776808 ms`，`30.635402%`。
- chart 组均：OFF `4.6569641330 ms`，ON `1.8128074492 ms`，减少 `2.8441566839 ms`，`61.073193%`。
- 漂移：OFF A2−A1 `−0.6079895000 ms`，ON B2−B1 `−0.1221268890 ms`，都小于组差；方向支持。
- FPS 组均：OFF `59.75799444`，ON `59.77709805`，`+0.01910360`（`+0.031968%`）；漂移 `+0.025374` 大于组差，**没有实质 FPS 收益**。
- 前向 p99 组均：OFF `18.80379677 ms`，ON `18.71978101 ms`，`−0.08401576 ms`（`−0.4468%`）；漂移 `+0.785073` 大于组差，不能称尾改善。
- 前向 max 组均：OFF `26.4226565 ms`，ON `23.8466145 ms`；四轮 max 全 <30，strict both ON<30 PASS；但 Starting 仍有 1 个长帧/轮（见 6.4）。
- first5：OFF `8.521607628→` ON `6.026780395 ms`（`−29.276486%`）；chart `4.497553815→1.793618923`（`−60.120123%`）。
- rest20：OFF `9.173206972→` ON `6.333848071 ms`（`−30.952740%`）；chart `4.697006942→1.817624652`（`−61.302492%`）。

#### 6.3 Normal：真实 Main 25 s，OFF/ON/ON/OFF

| 轮次 | 路径 | 应用均值 ms | 应用 p99 ms | 应用 max ms | chart 均值 ms | chart p99 ms | chart max ms | FPS | 前向 p99 ms | 前向 max ms | >30 ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A1 | OFF | 10.257337 | 18.607042 | 48.056250 | 5.322244 | 6.987902 | 8.703646 | 59.485140 | 21.833161 | 51.087500 | 4 |
| B1 | ON | 8.187126 | 17.967667 | 63.779687 | 2.690482 | 4.174979 | 5.319791 | 56.660343 | 29.477213 | 68.532812 | 8 |
| B2 | ON | 8.776325 | 21.989479 | 79.580729 | 2.826505 | 4.516875 | 9.663021 | 52.790041 | 30.259271 | 82.500521 | 17 |
| A2 | OFF | 11.724087 | 24.427682 | 63.313542 | 5.549108 | 7.451843 | 10.933855 | 46.380549 | 32.129937 | 66.494792 | 28 |

- 应用组均：OFF `10.9907121215 ms`，ON `8.4817256198 ms`，减少 `2.5089865018 ms`，`22.828243%`。
- chart 组均：OFF `5.4356758704 ms`，ON `2.7584937220 ms`，减少 `2.6771821484 ms`，`49.252056%`。
- 漂移：OFF A2−A1 `+1.4667492484 ms`，ON B2−B1 `+0.5891990618 ms`，都小于组差；方向支持。
- FPS 组均：OFF `52.93284482`，ON `54.72519170`，`+1.79234688`（`+3.386077%`）；但 OFF A2−A1 `−13.104591`，远大于组差，**不能称 FPS 收益**。
- 前向 p99 组均：OFF `26.981549475 ms`，ON `29.868242145 ms`，`+2.88669267 ms`（`+10.699%`）；四轮 `21.833/29.477/30.259/32.130`，ON 两轮 ≥30；**尾延迟变差**。
- 前向 max 组均：OFF `58.791146 ms`，ON `75.516667 ms`，`+16.72552 ms`（`+28.449%`）；四轮 `51.088/68.533/82.501/66.495`；**max 变差**。
- >30 帧：`4/8/17/28`；`both_ON_playing_strictly_below30ms = false`。
- first5：OFF `9.820184260→` ON `7.636763713 ms`（`−22.234008%`）；chart `5.220521338→2.734403595`（`−47.622021%`）。
- rest20：OFF `11.353139972→` ON `8.707300750 ms`（`−23.304911%`）；chart `5.509138973→2.765086104`（`−49.809106%`）。

#### 6.4 Starting 长帧与窗口边界

- Low 四轮 Starting 各 1 个 ≥30 ms：max `31.860938/34.227604/34.351563/35.409896 ms`；全部发生在 Playing 前，不能混入“游玩 25 s 30 ms 门”。
- Normal Starting max `65.947396/64.864062/51.210417/88.118229 ms`；同样在 Playing 前保留，不 trim。
- 所有 long 帧的 previous/current/next/native/particle/glyph 上下文都保留；不猜 GPU 因果。
- 25 s 不是全曲；不能用这组数据宣称 162.7 s 全曲收益。

#### 6.5 独立 observer 30 s cohort（另一组，不与真实 Main 混池）

`chart-workset-performance-v1` 是另一组 observer cohort：低/普通各 OFF/ON/ON/OFF，1800 帧，唯一变量仍是 tail workset；它不是真实 Main，也不能和上面的 25 s 结果相加。

| 画质 | OFF 应用均值 ms | ON 应用均值 ms | 应用变化 | OFF chart 均值 ms | ON chart 均值 ms | chart 变化 |
|---|---:|---:|---:|---:|---:|---:|
| Low | 7.018825 | 5.611402 | −20.052% | 3.011051 | 1.541317 | −48.811% |
| Normal | 8.666111 | 7.404205 | −14.561% | 3.619807 | 2.279424 | −37.029% |

这张表只用于说明“同一算法在另一窗口方向一致”；因为 observer 窗口/采样与真实 Main 不同，正文主结论仍以 6.2/6.3 的 25 s 表为准。原始结果在 `chart-workset-performance-v1-execution/results.json`，其中 `cohorts.low.whole_app` 为 `7.0188247872→5.6114019186`（−20.052116%），`cohorts.normal.whole_app` 为 `8.6661109542→7.4042054511`（−14.561382%）；`common_chart_app` 分别为 `7.0179230305→5.6124242173`（−20.027276%）与 `8.6676702078→7.4057199032`（−14.559279%）。chart 均值来自该 cohort 每个 `analysis.json` 的 `metrics_ms.chart_ms.mean` 两轮平均，不是从应用墙钟推算。

#### 6.6 质量 cohort：6 组 RGBA 与消费计数

`chart-workset-quality-v1` 使用 6 个固定 pair（低/普通 × early/dense/later），每组 2560×1440 原始 RGBA；ON/OFF 成对比较：

| 项 | 结果 |
|---|---|
| 6 个原始末帧 | `changed/max = 0/0`，逐像素 RGBA 相同 |
| 成对原 timeline | 逐行一致 |
| 低/普通 ON 的 accepted = proved = witness | early `2400`，dense `1800`，later `1440`（每画质） |
| 低/普通 ON 的 skipped note visits | early `399499`，dense `310903`，later `147124` |
| 合计 | `11280` 次 cut，`1,715,052` 次省遍历，`false_culls=0` |
| OFF | 所有计数真实为 0 |
| blob | `22921 bytes`，`8 groups`，`1333 notes`，key 以 `8abde2cf` 开头、`f3ab3fc` 结尾 |
| cache | 首轮 OFF 真正 cold publish，其余 11 轮 hot read；`cache_root` 仍是 Android 旧嵌套路径 |
| 保护 | 63 源、旧 15 保护、336 quality raw 保持；private data/fonts before-after tuple 通过 |

这组数据的意义是：**消费确实发生**（accepted/proved/witness 为正，skip 为正，false_culls 为 0），而不是“blob 文件存在”。性能 cohort 固定 `audit=false`，因此它的 6 个 exit counter 真实为 0；这不能反推“没有消费”，消费由 quality cohort 证明。

#### 6.7 原始记录（AGENTS 原文）

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

`本地证据/AGENTS.md` 第 163 行（Low 真实 Main 25 s）原文：

> ONLYtailflag OFF/ON/ON/OFF，固定Low sample1/FrozenSmallTRUE/commonFont/aggFALSE/compilerTRUE/auditFALSE：full25 appOFF9.04240682→ON6.27222914ms(-30.635402%,-2.770178)，chart-stage wall4.65696413→1.81280745(-61.073193%,约2.57x该stage速度)，OFFappA2-A1−.6079895/ON−.1221269小于组差；first5 app8.521607628→6.026780395(-29.27648564%)/chart−60.12012314%，rest20 app9.173206972→6.333848071(-30.95273997%)/chart−61.30249167%。sharedbothforwardendpoints [.022085937,25.007235412] app−30.619246%/chart−61.071649%同方向。Playingmax A1/B1/B2/A2 26.163021/22.239063/25.454166/26.682292ms，全4无>=30，strictbothON<30 PASS；FPS59.75799444→59.77709805(+.031968%，漂移大于微差)，没有实质FPS收益。fullloopP99mean OFF18.80379677→ON18.71978101，first5mean19.75815893→18.92859090，rest20mean18.069853935→18.388088840，不把平均app减小说成各段尾都改善。Starting各1长(max31.860938/34.227604/34.351563/35.409896)全部保留，发生Playing前不混游玩门；所有long完整prev/current/next/native/particle/glyph上下文留存，不猜GPU因果。PGRtarget25pub0/Smallpositive且payloadDERIVED/zero/over256与sourceUI旗标一致，exit计数auditfalse真实0。

`本地证据/AGENTS.md` 第 139 行（Normal 真实 Main 25 s）原文：

> NORMAL独立四轮审计真实终态0：SAME82057/1c8f4f/no restart，final-audit03b3706adf8da1573cb463733636536de2368d273e484bb57512d062e7889e2d，Root实际hash与关键comparison核；finalreceipt仍待auditor seal。full25 appOFF10.99071212→ON8.48172562ms(-22.828243%,-2.5089865)，chartwall5.43567587→2.75849372(-49.252056%,-2.6771821)；appOFFdrift+1.466749/ON+.589199，chartOFF+.226864/ON+.136023小于组差，支持有限窗口局部CPU-driver wall减少而非纯GPU。FPSordered59.485140/56.660343/52.790041/46.380549，OFFdrift−13.104591大于组+1.792347，不能称稳定整体或FPS收益。PlayingP9921.833161/29.477213/30.259271/32.129937；max51.087500/68.532812/82.500521/66.494792，>30=4/8/17/28，strictbothON<30 FAIL，不以均值下降当完整目标结束。所有FrozenOFF/PGRpositive/source/restore门已通过。本段未原因归因（thermal/GPU/driver均未知），已要求auditor只读sealed长帧prevapp/render/flush/particle/HUD/glyph/gap有界汇总，下一针对实热点而非blind重ABBA。

`本地证据/AGENTS.md` 第 207 行（质量）原文：

> 新chart尾工作集真机质量已实际完整通过：唯一owner40045 actualterminal0/12jobs/6pairs，results779563d4add4003e9bf3defb7d5651d34140e6f54845bdd94897f25db353c0dd，ownerreceipt ef64358372821abc1e93a79c3488b72d194c617f277cbbd5071e4ff3a46fee8d。独立71693 actualterminal0/final-audit47322c7167849f876f1d13c98d1d7d0aec0cac722e7f6346527f4e39e406aa40/terminal4c1ccf5f4c5b6330ce30552119253806bec78ad1aea760761860040c3a27741e。6原始2560x1440 RGBA逐pixel changed/max均0、成对原timeline逐行一致；每画质ON early/dense/later accepted=proved=witness2400/1800/1440、skip399499/310903/147124，合计11280次cut/1715052省遍历/false0，6OFF全0。22921B blob/8groups/1333notes/key8abde2cf…f3ab3fc，firstOFF真正coldpublish/其余11hotread；实际cache_root仍nested D/C原路径，不能据hot/cold替代真正UI清cache/epoch回退验收。336raw/63source/old15保持；仅privateData/fonts实际before-after tuple通过，不假造未保存beforeSHA，PIDempty/stayon2/设备已交还。Root实际view normal-dense Off+On原截图，AUTOPLAY/score43367/combo68/音符与粒子清晰且同；不宣称全chart所有font/其它GPU质量。sourceHEAD84422d4/SO9a0/candidatedefaultoff、mainnoPush，本轮仍NO新FPS收益。

`本地证据/AGENTS.md` 第 217 行（消费者提交）原文：

> 真正chart尾工作集消费者已本地提交84422d41905094b7ec78ab51be967371ff531beb（接入谱面尾工作集，main未push），12source与所有权/API迁移/CPUcache/blob/实际line prefix-cut/replay/witness落地；consumer defaultfalse，不称生产收益。最终checks95555 actualterminal0：19workset（8IR+8实际capture/math+2原Ctrlhelper位型+1有界cache）、3owner、8cache（含重复有界case）及default/新consumer/oldIR三Windowscheck均0；额外73839 actualterminal0/正式chart_pre_compile无research及迁移example check都0。原checks16402同样完整0保留，之后补currentglobalreads/visibilityfreshflag guard才最终v2。自动agg/末init±0/sidecursor/范围溢出/旧IR及旧诊断均回原。Rootformatter递归无关4文件已真正恢复，随后3个CRLF状态只通过gitadd更新stat，cached diff明确空，最终Gitstatus实际空；未提交无关格式变化。

`本地证据/AGENTS.md` 第 225 行（格式/边界）原文：

> 本轮谱面尾工作集基础已实际实现并本地提交796e3835b7f67db41276d62cb7daf9aeb97b9c6e（加入谱面工作集基础，参考实现/主项目未push/trackedclean）。仅3source：render_workset_ir.rs/visibility_bounds.rs/render_compiler.rs声明；没有实际GameScene/Chart/Note消费者、cache manager存取或新的gameplay收益。PHRWS001/ABI1 CPU-only blob与64lowercaseSHA key(namespace+ABI+sourceSHA+loaded descriptorSHA)，f64 speed/height原u64bits、连续sourceIDs/global同line重叠拒绝、finite ascending heights/6logicaltexture roles、checked/tryreserve单份16MiB logical/serial预算、decode actual expected ordered shape逐项匹配/精确EOF/重新suffix-x界（suffix不存盘）、无GPUhandles/judge/clocks。仅声明未启用，旧公开render行为不变。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

- 6 组 2560×1440 末帧 RGBA `max diff = 0`；成对 timeline 逐行一致。
- ON 的 `accepted/proved/witness` 为正，`skipped_note_visits` 为正，`false_culls=0`；OFF 全 0。
- `22921 bytes` / `8 groups` / `1333 notes` blob 冷发布/热读取/损坏重建/关闭停用/清理后重建都实际执行。
- Chart owner / `chart_mut` 永久撤销 / `ChartRenderScope` 只允许 GameScene 私有调用；旧 `render_chart_ir` 或旧 `visibility` 诊断开启时 workset 回退。
- 动态 ctrl、`appear_before`、`incline_sin`、非有限值、±0 末控制、自动 aggressive、极端线、旧 IR、debug 均回原路径；测试覆盖 19 个 workset 相关用例（8 IR + 8 capture/math + 2 Ctrl helper + 1 有界 cache）。
- 性能 cohort 的 `audit=false` 六个 exit counter 真实为 0；消费由 quality cohort 证明，不用 0 计数冒充“已消费”。

#### 7.2 失败项与限制

- **FPS 没有实质收益**：Low `+0.032%`，漂移大于组差；Normal `+3.386%`，但 OFF A2−A1 `−13.10%`，远大于组差，不能称 FPS 改善。
- **Normal 尾延迟变差**：p99 `26.982→29.868 ms`（+10.7%），max `58.791→75.517 ms`（+28.4%），>30 帧 `4/8/17/28`；`both_ON_playing_strictly_below30ms=false`。
- **Low 的 30 ms 门只在 Playing 通过**：Starting 每轮仍有 1 个 ≥30 ms 长帧（31.86–35.41 ms）；不能把 Starting 的长帧藏掉，也不能把 Playing 的门当成整帧门。
- **25 s 不是全曲**：没有 162.7 s 全曲证据；没有其它 GPU/设备证据。
- **没有真实 UI clear**：cache_root 仍是旧嵌套路径，hot/cold 不能替代真实 Settings clear/epoch 回退验收。
- **缓存失效只覆盖有限场景**：Chart 外部可变访问会撤销 lease；但如果外部直接改 `chart.lines` 的内部字段而绕过 `chart_mut`，源码用 capture/current 检查回退，仍未证明所有 mutation 路径。
- **workset 只覆盖 canonical plain 音符**：hold、fake、复杂 note、动态 ctrl、非默认 alpha/scale/rotation/y、非有限值都保留原路径；覆盖比例不是“全部音符”。
- **审计开销**：quality 的 witness 是逐音符重放原四角 AABB，成本高，只用于质量；性能固定 audit false，不能用 quality 耗时当性能。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：局部。**

理由：

1. **chart 阶段收益大且方向一致**：Low chart `4.657→1.813 ms`（−61.07%），Normal `5.436→2.758 ms`（−49.25%）；first5/rest20 分层也同方向（−60.12%/−61.30%，−47.62%/−49.81%）。这不是单点。
2. **应用均值也随之下降**：Low `9.042→6.272 ms`（−30.64%），Normal `10.991→8.482 ms`（−22.83%）。说明省掉的遍历确实进入了应用墙钟，不只是阶段迁移。
3. **为什么仍是“局部”**：FPS 没有实质收益，Normal 的 p99/max/>30 反而变差；收益集中在 chart CPU 段和有限窗口，不是整帧、不是尾延迟、不是所有画质/GPU。
4. **为什么不是“无”**：chart 阶段减少约 2.68–2.84 ms，应用均值减少 2.51–2.77 ms；`accepted/skipped/witness/false0` 证明跳过的是被原 AABB 拒绝的尾部，而不是少画可见音符。
5. **为什么不是“负”**：画面 RGBA 0，判定/音频/pose 未改；Normal 尾延迟变差是“整帧/尾部没有收益”的证据，不足以否定 chart 阶段的真实节省。

**置信度：**

- 对“chart 阶段在这个有限窗口内大幅下降”：**高**。
- 对“应用均值下降由 workset 造成”：**中高**；OFF/ON 漂移小于组差，first5/rest20 同方向，但没有逐帧因果分解。
- 对“整帧 FPS/尾延迟/全曲/所有 GPU 受益”：**低**；Normal 的 p99/max 变差是明确反证。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`796e3835` 格式/边界基础；`84422d41` 消费者/所有权/cache/实际 prefix-cut；`241f55dd` 生产接线（非 research 执行 true、Cargo 默认 feature、Config 注释/locale） |
| 参考实现是否默认启用 | **否（运行期默认关闭；编译期默认编入）**。`phira`/`phira-main` 默认 feature 含 `chart_pre_compile`，`Config.chart_pre_compile=false` |
| 参考实现是否有消费者 | 有。`GameScene::new` prepare；`ChartRenderScope` + `Chart::render_scoped`；`JudgeLine::render_inner` 的 above/below 循环消费 `Cut` |
| 是否进 main | 否。参考实现未合入 main，主项目未 push |
| 是否默认研究执行 | 否。`render_research` 下 `ENABLED=false`；非 research 下 `execution_enabled=true`，但仍受 `Config.chart_pre_compile` 与所有回退条件约束 |
| 是否有独立开关 | 复用 `chart_pre_compile`，没有新增 `tail_workset` 开关；Settings 文案已改为“普通/低画质均可、加载缓存、关闭回原、清 cache、能力/谱面自动回退” |
| 是否有统一清理 | 有。与 P-01 共用 `RenderCache` 的 size/clear/epoch；但真实 UI clear 尚未验收 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **整帧收益不成立**：FPS 没有实质提升，Normal 的 p99/max/>30 变差；主项目不会为一个“chart 阶段省 2.7 ms 但尾部更差”的优化直接开默认。
2. **覆盖有限**：只覆盖 canonical plain 音符的尾工作集；hold/复杂 note/动态 ctrl/非默认对象都回原路径。它不是“全谱面编译器”。
3. **25 s 窗口**：不是全曲 162.7 s；没有长曲/重入/多 UI 生命周期证据。
4. **缓存生命周期未完全闭环**：真实 Settings clear、epoch 回退、动态退路、其它 GPU 仍未验收；cache_root 还有旧嵌套路径。
5. **生产接线较晚**：`241f55dd` 才把非 research 执行设为 true、Cargo 默认 feature 编入；此前只是研究消费者。默认运行仍关闭，主项目未 push。
6. **与 P-01 共享开关**：`chart_pre_compile` 同时控制线 GPU IR 和 CPU workset；用户关闭开关会同时失去两者，不能单独评估/部署 workset。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| 证明不保守 | `outside_range` 若错误地返回 true，会少画可见音符 | 整尾公共轴证明；`Interval` 向外取整；FTZ/FMA/NaN 域回退；audit 逐音符 witness/false0 |
| 运算次序改变 | `height/aspect*spd-line_height` 重关联会改 f32 结果 | `source_base` 保留原顺序；大数相消测试；不替换 `set_height` |
| ctrl 终态丢失 | 跳过尾部后 `CtrlObject` 高度错误 | 末音符 `init_ctrl_obj` 精确重放；±0/非有限末控制回退 |
| Chart mutation | 外部改 Chart 后旧 plan 仍被使用 | `chart_mut`/`chart_parts_mut` 永久 revoke；capture/descriptor + 运行态检查；`ChartRenderScope` 私有 |
| 缓存旧源/损坏 | 旧 blob 对应旧 parser/旧 descriptor | source SHA + descriptor SHA + ABI + 逐项 expected shape + checksum + 16 MiB 预算 |
| 分组边界 | 把不支持音符从 speed run 拆出会改变顺序/资格 | `SourceGroups` 严格按原 `JudgeLineCache::reset` 边界；整组不合格就整组回退 |
| 极端线 | 20k+ 音符的线可能内存/时间不可控 | `EXTREME_LINE_NOTES=20_000` 回退；`MAX_NOTES=1_000_000` |
| 自动 aggressive | 与 aggressive 屏幕外剔除叠加可能改变行为 | `res.config.aggressive` 直接回退 |
| 诊断/读取计划 | 旧 reads/visibility 会改变每 note 回调 | 任一 enabled/audit 都回退 |
| 缓存竞态 | clear/disable 与后台 prepare 同时 | epoch/ticket；`is_current` 再检查；`publish` 锁内检查 |

#### 10.3 未验证项

- 162.7 s 全曲、完整 loop、真实 Main 首次 Playing 前 5 s 之外的更长时间。
- 其它 GPU/驱动；本 cohort 只有 Mali-T880。
- 真实 Settings UI clear + cold/hot + OFF/reenable + 动态退路。
- 多 UI 生命周期、暂停/重试/退出重进、字体/资源变化。
- 手动输入（本 cohort autoplay）。
- 所有 mutation 路径都能被 owner/lease 捕获的证明。
- workset 与 P-01/line DCE/粒子等组合优化的交互；本 cohort 固定 common font/line DCE，只变 tail flag。

### 11. 落地步骤（可执行清单）

- [ ] 在真实 Main UI 中完成一次完整流程：打开 `chart_pre_compile` → 进入谱面 → Playing 25 s → 暂停/返回 → 再次进入 → 关闭开关 → 清 cache → 重进；记录 `workset` report、cache size、epoch。
- [ ] 用同一 SO 跑完整 162.7 s 全曲 OFF/ON/ON/OFF；保留 120 预热 + 全部真实帧，报告应用均值/p99/max/>30、chart 阶段、forward 间隔、呈现。
- [ ] 在 Normal 画质重复 25 s 四轮，专门检查 p99/max/>30 是否仍变差；如果变差，明确标注“不能默认推广”。
- [ ] 增加真实 UI clear 后的 cold/hot/OFF/reenable 证据；不能只用文件存在或 hot read 代替。
- [ ] 验证 `chart_mut`/`chart_parts_mut` 的所有调用点都会 revoke；用测试覆盖“外部改 note/line 后不再复用旧 plan”。
- [ ] 验证 EGL/context 变化时 workset 只失效、不崩溃；和 P-01 的 line renderer 生命周期分开记录。
- [ ] 在至少两种 GPU 上重复 6 组 RGBA + 25 s 性能；不能把 Mali-T880 外推。
- [ ] 评估是否把 workset 从 `chart_pre_compile` 拆成独立开关；当前共用开关会让用户无法单独关闭 CPU workset。
- [ ] 更新 AGENTS/README 的默认值与“生产/研究”状态，避免继续出现“consumer defaultfalse”与“非 research execution true”的歧义。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `be4e1305823bfebdad1853d7b333e0f79c3ead79` |
| 格式/边界基础 | `796e3835b7f67db41276d62cb7daf9aeb97b9c6e`（加入谱面工作集基础，2026-10-12 04:44:43 +0800） |
| 消费者/所有权/cache | `84422d41905094b7ec78ab51be967371ff531beb`（接入谱面尾工作集，2026-10-12 05:06:32 +0800） |
| 生产接线 | `241f55dd04e98b256753416a48c3482d94c57c09`（接入谱面工作集预编译，2026-10-14 00:23:03 +0800） |
| 游玩消费记录 | `4f573dfd4d25b4853813e24585524a021974465b`（记录工作集游玩消费，2026-10-12 23:40:10 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/render_workset.rs`（798 行）、`prpr/src/render_workset_ir.rs`（535 行）、`prpr/src/visibility_bounds.rs`（888 行）、`prpr/src/core/line.rs`、`prpr/src/core/note.rs`、`prpr/src/scene/game.rs`、`prpr/src/core/chart.rs`、`prpr/src/render_compiler.rs`、`prpr/src/config.rs` |
| 测量数据 | `chart-workset-main-low-abba-independent-v1/final-audit.json`；`chart-workset-main-normal-abba-independent-v1/final-audit.json`；`chart-workset-quality-v1-execution`；`chart-workset-performance-v1-execution/results.json` |
| 质量 SO | `9a0dfff3cb0e37e1dd5c0a2eb02ce7b0eb20f9b1b7b6673bf2973bba53016caa`（63 源，commit 84422d4） |
| 真实 Main SO | `b0a293c7bda1850abc267c36499bdee890522f4bc49f2ec16329fe38c4ec49c8`（commit 4f573dfd，63 源） |
| 说明 | 正文已含关键代码、表格与结论；上述路径只用于追溯 |

---

## P-03 通用字体图集池

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-09（2b57559d；后续 09b2370c/be4e1305 只扩展固定纹理研究）　参考实现提交：2b57559d　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计，不冒充独立验收）

### 1. 一句话结论

通用字体图集池把 `GlyphBrush` 的单一可变 2048² RGBA 图集改成每个 Painter 最多 4 个完整 RGBA 版本、32×32 tile 增量、零超时 fence 轮询的版本池；字形代表、布局、UV、过滤和透明顺序不变。生产通用消费者在同 SO 1800 帧 ABBA 中把低画质应用均值 **8.21744→7.34205 ms（−10.65%）**、FPS **57.1763→59.7380**，普通画质 **11.15815→9.81005 ms（−12.08%）**、FPS **46.1153→57.9166**；另一组整体策略 ABBA 为低 **8.63315→7.64735 ms（−11.42%）**、普通 **11.42990→9.74472 ms（−14.74%）**。这是**整帧**正优化；但普通画质新路径的呈现 max 仍有 33.5 ms、低画质有一帧 30.66 ms，不能宣称所有尖峰消失。

### 2. 问题与瓶颈（为什么原路径慢）

原 `TextPainter` 使用 `GlyphBrush` 自带的一张 2048² RGBA 图集纹理。字形栅格化/更新时，`process_queued` 的回调直接对**当前正在被采样的同一张纹理**做 `update_texture_part`。这会形成隐式的 GPU 读→写依赖：

- 上一帧/更早的 draw 仍可能采样这张纹理；
- 驱动必须等旧读完成，才能安全写入同一区域；
- 等待发生在 render 线程的纹理更新调用里，表现为 HUD 长帧；
- 每次 glyph 更新都可能触发这种等待，尤其是新字形/大块更新。

README 的字形上传诊断给出了直接证据：同一原路径两次 1200 帧采样，字形上传帧 89/90，HUD >20 ms 帧 71/79，且**所有 >20 ms HUD 帧都伴随上传**；HUD 均值 2.400/2.458 ms，上传调用最大 46.428/46.305 ms；采样期总上传约 1.63/1.66 MB。相关性不等于所有 GPU 的因果，但在本设备上“更新调用内的读写等待”是最合理的解释。

整图替换原型验证了另一条极端路径：每次变更发布全新的 2048² RGBA 图集（16 MiB），旧图集延迟删除；四组画面 RGBA 逐像素相同，原路径 57.53/57.52 FPS，替换 59.47/59.68 FPS；HUD 均值 2.415→1.343–1.374 ms，HUD >20 ms 帧 69/78→7/7。代价是每轮上传 2,080,374,784 bytes（约 1.94 GiB/20 s），原路径仅 1.66 MB：**用巨大带宽换掉读写等待**。生产池要保留“写空闲版本”的思想，同时把上传压回增量区域。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

**不变量一：逻辑图集不变。** 池不改变 `GlyphBrush` 的 glyph 代表、栅格化、布局、UV、过滤、透明顺序；它只改变“这些像素写进哪张物理纹理、何时切换”。因此同一个 glyph 的 UV 仍指向同一逻辑位置，文字几何不变。

**不变量二：完整 RGBA 权威镜像。** 每个 Painter 保留一份完整 2048² RGBA CPU shadow。每次 callback 把 `tex_data` 转成 `[255,255,255,alpha]` 写入 shadow 的对应矩形，并记录 write rect；池发布时从 shadow 复制变化区域，而不是从 glyph_brush 的内部缓存猜测。shadow 是权威像素源，池版本只是它的副本。

**不变量三：只写“不再被当前采样”的版本。** 池有 4 个 slot；每个 slot 有 `version` 和 `fence`。发布时：

- 当前 slot 不写；
- 其他 slot 先 `glClientWaitSync(fence, 0, 0)`（零超时，只查询，不阻塞）；
- 找到 `ready=true` 的 slot 才写；
- 写完后 `current` 指向新 slot，旧 current 保留给在途 draw；
- 旧 slot 的 fence 在后续发布中删除/替换。

这样写操作不会命中“当前正在被采样”的纹理；如果 4 个 slot 都忙，池选择一个既不是 caller previous、也不是 current 的版本退休，分配一张新图集并全量上传（busy fallback）；如果连可退休版本都没有，回退到调用方的原可变纹理。

**不变量四：零超时。** `sync_api::ready()` 只接受 `0x911a`（already signaled）、`0x911c`（condition satisfied）、`0x911b`（timeout）三种结果；`timeout=0`，不请求 flush，不阻塞。没有“等 1 ms/等 16 ms”的隐藏等待。`blocking_waits=0` 是报告字段。

**不变量五：增量只覆盖真实变化。** `Delta` 把图集按 32×32 tile 记录最后修改 generation；`since(version)` 按行合并连续 dirty tile 成矩形，再从 shadow 复制。历史存储是 `columns*rows` 个 `u64`，和图集面积绑定，不随曲长增长；不保存无限 dirty journal。落后多个版本的 slot 也能从 shadow 重建，不依赖中间版本。

**为什么不会改变画面：** 逻辑图集/UV/过滤/透明顺序不变；物理版本都是同一份 RGBA shadow 的完整副本；增量矩形覆盖所有自该版本以来变化的 tile；发布前检查 `bitmap_bytes(width,height)==shadow.len()`、write rect 边界、shape 未变、slot fence 已 ready；GL 错误则走 mutable recovery。因此“同一逻辑图集”在任意时刻被采样到的像素应与原路径相同。它不是压缩格式、不是双通道、不是低分辨率，理论上比 `mild-lossy` 更强；但源码仍要求有限 RGBA 对照，不能凭理论宣布所有驱动无损。

**代价：** 每个 Painter 4 个 slot × 2048² × 4 B = **64 MiB 名义 GPU 容量**；PGR 与 default 两个主 Painter 都预热时是 **128 MiB 名义 GPU 容量**；两份完整 CPU mirror 是 **32 MiB**；每 Painter 还有 1 MiB scratch。`nominal_storage_bytes` 明确不是物理 GPU 内存/RSS。静态 default 字体只复用 3 次，完整池是否值得常驻，取决于后续动态字体/重入/长曲测试。

### 4. 具体实现

#### 4.1 文件与符号清单（含行数、函数/结构名）

| 文件（head `be4e1305`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/ui/font_atlas_pool.rs` | 1101 | `SLOTS=4`（7）、`BUDGET=64 MiB`（8）、`Slot`（140）、`Publication`（147）、`Pool`（159）、`retirement_candidate`（221）、`image`（224）、`Pool::new`（366）、`warm_internal`（554）、`mutable_fallback`（658）、`publish`（730）、`report`（986）、`Drop`（1078） | 通用 RGBA 图集版本池 |
| `prpr/src/ui/atlas_delta.rs` | 143 | `TILE=32`（3）、`Rect`（5）、`Delta`（11）、`advance`（39）、`since`（50） | 32×32 tile 版本表与增量矩形合并 |
| `prpr/src/ui/text.rs` | 1609 | `ProductionAtlas`（27-58）、`production_requested`（843-853）、`production_atlas`（866-883）、`prepare_font_pool_loading`（484-511）、`production_recover_mutable`（513-588）、`production_publish_atlas`（590-650）、`submit` callback（1173-1341） | 生产消费者：shadow、发布、回退、resize |
| `phira/src/lib.rs` | — | PGR 字体 warm（456-461）、default 字体 warm（506-513） | 主入口加载 warm |
| `prpr/Cargo.toml` | 136 | `font_atlas_pool=[]`（15）、`default=["log","font_atlas_pool"]`（18） | prpr 默认编入 |
| `phira/Cargo.toml` | 127 | `font_atlas_pool=["prpr/font_atlas_pool"]`（15）、`default=["video","font_atlas_pool","chart_pre_compile"]`（18） | 主 crate 默认编入 |
| `phira-main/Cargo.toml` | — | **没有** `font_atlas_pool` 转发；`default=["video","chart_pre_compile"]` | 桌面主程序不编入（见 §10.3） |
| `prpr/src/config.rs` | 829 | 没有字体池开关 | 字体池不依赖 `Config.chart_pre_compile` 或画质 |

#### 4.2 数据结构与算法（逐步说明）

1. **构造**：`Pool::new()` 取当前 managed context id；`Api::load()` 检查 GL 版本是否 `OpenGL ES 3.x`，通过 `eglGetProcAddress` 取 `glFenceSync/glClientWaitSync/glDeleteSync`，实际创建一个 fence 并 poll 一次，确认无 GL 错误。失败则 `api=None`、`reason=Some("Font pool sync backend unavailable on this target")`，后续 `warm/publish` 走 mutable 回退。
2. **预算**：`bitmap_bytes(width,height)` 检查非零、≤65535、乘法不溢出；`pool_fits_budget` 要求 `width*height*4*4 <= 64 MiB`。2048² 正好 16 MiB/张，4 张 64 MiB；2048×2049 被拒绝。
3. **加载 warm**：`warm_internal(width,height,shadow,loading=true)`：
   - 已有 slots 且 shape 相同 → 直接返回 current（重复 warm 不重新分配）；
   - 校验 shadow 长度；
   - 检查预算；
   - `gl.flush()` 消费旧 loading UI 的队列；
   - 分配 `Delta`、1 MiB scratch；
   - 循环 4 次 `make_image()` 创建完整 RGBA 纹理（研究模式下可走 immutable 实验，生产走 `Texture::from_rgba8`）；
   - 每次创建后插入 fence；
   - 全部成功才把 `current=slots[0]`；任何失败 `loading_failure()` 清空 slots、释放 fence/texture、`api=None`，调用方保留原可变纹理。
4. **首发布**：`publish(previous,width,height,shadow,writes)`：
   - `previous_owned = self.owns(previous)`，先捕获，防止退休后重名 GLuint 被调用方重复删除；
   - context 变化 → `mutable_fallback`；
   - 校验 shadow 长度和 write rect；
   - `api=None` → mutable fallback；
   - shape 变化但 slots 非空 → mutable fallback；
   - slots 为空 → `warm_internal(width, height, shadow, false)`，成功则返回整池 `bytes=shadow.len()*4`；
   - 否则进入增量路径。
5. **增量路径**：
   - `history.advance(writes)` 递增 generation，按 32×32 tile 标记；
   - 找到 current slot index；
   - 为 current 插入新 fence，替换并删除旧 fence；
   - 遍历其他 slot，跳过 current；研究模式 2 直接视为 busy；否则 `api.ready(fence)` 零超时轮询；
   - 第一个 ready 的 slot 作为目标；删除其旧 fence；
   - `rectangles = history.since(target.version)`；
   - 用 PBO（研究可选）或 `update_texture_part` 逐矩形从 shadow 复制；
   - 检查 GL 错误；成功则 `target.version=generation`，`current=target.texture`，`reused+=1`，`delta_bytes+=bytes`；
   - 失败 → `mutable_fallback`。
6. **busy fallback**：没有 ready slot 时，`retirement_candidate()` 选一个既不是 `previous`、也不是 `current` 的 slot；`flush_retirement()` 后删除它（fence+texture），`make_image()` 新分配完整纹理，`version=generation`，`current=新纹理`，`fresh+=1`，`full_bytes+=shadow.len()`。这保证不会在 4 个都在途时卡死，也不会覆盖 caller/current。
7. **mutable fallback**：释放池内除 `previous` 外的 slot；如果 `previous` 在池内，把它从 slot 移除但不删除（所有权交还调用方）；`api=None`；如果 shape 匹配，对 `previous` 做一次完整 `update`，成功则 `valid=true`；失败则 `valid=false` 并保留 CPU shadow，调用方停止字体绘制/尝试恢复。跨 context 时直接返回 `previous_owned=true`、`valid=false`，不删除外来名字。
8. **resize**：`process_queued` 返回 `TextureTooSmall { suggested }` 时，调用方 `gl.flush()`，`pool.reset()`，清 shadow/writes/dirty，重新检查预算；如果新尺寸超预算，`atlas.disabled=true`；分配新 `cache_texture`，更新 context，`brush.resize_texture(suggested)`，清空研究 shadow 并重建。
9. **退休**：`Pool::drop` 如果 context 已变或 immutable blocked，直接返回（不删外来名字）；否则 `flush_retirement()`，逐个删除 fence；不是 current 的 texture 走 `queue_texture_deletion()`，current 由 `TextPainter` 自己排队删除，避免双删。
10. **报告**：`report()` 输出 `supported/reason/slots/shape/warm_bytes/reused/fresh/busy_fallbacks/failed_fallbacks/polls/delta_bytes/full_bytes/mutable_fallbacks/nominal_storage_bytes/blocking_waits`；`blocking_waits` 固定 0，`nominal_storage_bytes` 明确只是 live slot 名义容量，不含 CPU mirror/in-flight/driver。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

以下代码取自 `be4e1305` 的 `prpr/src/ui/font_atlas_pool.rs` 与 `prpr/src/ui/text.rs`。为便于阅读按主题分段，函数体未删行。

**生产消费者包装 `prpr/src/ui/text.rs:27-58`：**

```rust
#[cfg(feature = "font_atlas_pool")]
struct ProductionAtlas {
    pool: font_atlas_pool::Pool,
    shadow: Vec<u8>,
    writes: Vec<[u32; 4]>,
    dirty: bool,
    disabled: bool,
    recovery_attempted: bool,
    error: Option<String>,
}
#[cfg(feature = "font_atlas_pool")]
impl ProductionAtlas {
    fn mirror(&mut self, width: u32, height: u32) -> bool {
        if !self.shadow.is_empty() {
            return true;
        }
        if self.disabled || !self.pool.can_warm(width, height) {
            self.disabled = true;
            return false;
        }
        let Some(bytes) = (width as usize).checked_mul(height as usize).and_then(|value| value.checked_mul(4)) else {
            self.disabled = true;
            self.error = Some("Font atlas mirror size overflow".into());
            return false;
        };
        if self.shadow.try_reserve_exact(bytes).is_err() {
            self.disabled = true;
            self.error = Some("Font atlas mirror allocation failed; using mutable atlas".into());
            return false;
        }
        self.shadow.resize(bytes, 0);
        true
    }
}
```

**池结构 `prpr/src/ui/font_atlas_pool.rs:140-189`：**

```rust
struct Slot {
    texture: Texture2D,
    version: u64,
    fence: Option<Fence>,
    #[cfg(feature = "render_research")]
    pbo: Option<pbo::Owner>,
}
pub struct Publication {
    pub texture: Texture2D,
    pub bytes: usize,
    pub previous_owned: bool,
    pub pooled: bool,
    pub valid: bool,
}
pub struct Pool {
    context: usize,
    api: Option<Api>,
    reason: Option<String>,
    slots: Vec<Slot>,
    current: Option<Texture2D>,
    shape: (u32, u32),
    history: Option<delta::Delta>,
    scratch: Vec<u8>,
    warm_bytes: u64,
    reused: u64,
    fresh: u64,
    busy: u64,
    failed: u64,
    polls: u64,
    delta_bytes: u64,
    full_bytes: u64,
    mutable_fallbacks: u64,
    #[cfg(feature = "render_research")]
    research_mode: u8,
    #[cfg(feature = "render_research")]
    research_runtime_fallback_triggered: bool,
    #[cfg(feature = "render_research")]
    pbo_stats: pbo::Stats,
    #[cfg(feature = "render_research")]
    pbo_client_core: bool,
    #[cfg(feature = "render_research")]
    pbo_client_blocked: bool,
    #[cfg(feature = "render_research")]
    immutable: ImmutableStats,
}
```

**退休候选 `prpr/src/ui/font_atlas_pool.rs:219-223`：**

```rust
fn retirement_candidate<T: Copy + PartialEq>(textures: impl IntoIterator<Item = T>, previous: T, current: Option<T>) -> Option<usize> {
    textures.into_iter().position(|texture| texture != previous && Some(texture) != current)
}
```

**加载 warm `prpr/src/ui/font_atlas_pool.rs:554-657`：**

```rust
fn warm_internal(&mut self, width: u32, height: u32, shadow: &[u8], loading: bool) -> Option<Texture2D> {
    #[cfg(not(feature = "render_research"))]
    let _ = loading;
    let api = self.api?;
    if self.context != context_id() {
        self.reason = Some("Managed context changed; recreate painter".into());
        self.api = None;
        return None;
    }
    if !self.slots.is_empty() {
        return (self.shape == (width, height) && bitmap_bytes(width, height) == Some(shadow.len()))
            .then_some(self.current)
            .flatten();
    }
    let Some(bytes) = bitmap_bytes(width, height).filter(|&bytes| bytes == shadow.len()) else {
        self.loading_failure("Invalid atlas bitmap".into());
        return None;
    };
    if !self.can_warm(width, height) {
        self.loading_failure("Font pool exceeds 64MiB nominal GPU budget".into());
        return None;
    }
    unsafe { get_internal_gl() }.flush();
    #[cfg(feature = "render_research")]
    if loading && self.pbo_stats.requested {
        pbo::flush_pending_deletions();
    }
    self.shape = (width, height);
    self.history = Some(delta::Delta::new(width, height));
    if self.scratch.try_reserve_exact(1024 * 1024).is_err() {
        self.loading_failure("Font upload scratch allocation failed".into());
        return None;
    }
    self.scratch.resize(1024 * 1024, 0);
    self.scratch.clear();
    for _ in 0..SLOTS {
        let texture = match self.make_image(width, height, shadow, loading) {
            Ok(texture) => texture,
            Err(reason) => {
                self.loading_failure(reason);
                return None;
            }
        };
        #[cfg(feature = "render_research")]
        let pbo = if loading && self.pbo_stats.requested {
            self.pbo_count(|stats| stats.loading_slots = stats.loading_slots.saturating_add(1));
            let owner = match pbo::Owner::new(unsafe { get_internal_gl() }.quad_context, self.pbo_stats.force_missing) {
                    Ok(owner) => owner,
                Err(error) => {
                    let unknown_state = match &error {
                        miniquad::PixelUnpackError::Gl { stage, cleanup_code, .. } => *stage == "loading binding restore" || cleanup_code.is_some(),
                        miniquad::PixelUnpackError::CapacityMismatch { cleanup_code, .. }
                        | miniquad::PixelUnpackError::BufferNameZero { cleanup_code } => cleanup_code.is_some(),
                        _ => false,
                    };
                    if unknown_state {
                        let reason = format!("Font PBO loading cleanup failed: {error}");
                        self.block_client_guard(reason.clone());
                        flush_retirement();
                        texture.delete();
                        self.loading_failure(reason);
                        return None;
                    }
                    None
                }
            };
            if owner.is_some() {
                self.pbo_count(|stats| stats.initialized_slots = stats.initialized_slots.saturating_add(1));
            } else {
                self.pbo_count(|stats| stats.loading_missing_slots = stats.loading_missing_slots.saturating_add(1));
            }
            owner
        } else {
            None
        };
        let fence = match api.insert() {
            Ok(fence) => fence,
            Err(reason) => {
                flush_retirement();
                texture.delete();
                self.loading_failure(reason);
                return None;
            }
        };
        self.slots.push(Slot {
            texture,
            version: 0,
            fence: Some(fence),
            #[cfg(feature = "render_research")]
            pbo,
        });
        self.warm_bytes += bytes as u64;
        if unsafe { miniquad::gl::glGetError() } != 0 {
            self.loading_failure("Font loading upload/fence GL error".into());
            return None;
        }
    }
    self.current = Some(self.slots[0].texture);
    self.current
}
```

**可变回退 `prpr/src/ui/font_atlas_pool.rs:658-727`：**

```rust
fn mutable_fallback(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], reason: String, previous_owned: bool) -> Publication {
    #[cfg(feature = "render_research")]
    if let Some(timing) = publish_timing::current() {
        timing.fallback(&reason);
    }
    self.failed += 1;
    self.mutable_fallbacks += 1;
    self.reason = Some(reason);
    #[cfg(feature = "render_research")]
    if self.client_guard_blocked() {
        return Publication { texture: previous, bytes: 0, previous_owned, pooled: false, valid: false };
    }
    if self.context != context_id() {
        self.slots.clear();
        self.current = None;
        self.history = None;
        self.shape = (0, 0);
        self.api = None;
        return Publication {
            texture: previous,
            bytes: 0,
            previous_owned: true,
            pooled: false,
            valid: false,
        };
    }
    #[cfg(feature = "render_research")]
    let guarded = self.client_guard_required();
    flush_retirement();
    if let Some(index) = self.slots.iter().position(|slot| slot.texture == previous) {
        let slot = self.slots.remove(index);
        if let (Some(api), Some(fence)) = (self.api, slot.fence) {
            api.delete(fence);
        }
    }
    self.retire_slots();
    self.api = None;
    drain_errors();
    let shape = previous.raw_miniquad_texture_handle();
    let dimensions_ok = shape.width == width && shape.height == height && bitmap_bytes(width, height) == Some(shadow.len());
    #[cfg(feature = "render_research")]
    if dimensions_ok && guarded {
        let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |context| shape.update(context, shadow));
        if let Err(error) = result {
            self.block_client_guard(format!("Font full client recovery guard failed: {error}"));
            return Publication { texture: previous, bytes: 0, previous_owned, pooled: false, valid: false };
        }
    }
    #[cfg(feature = "render_research")]
    let uploaded_by_guard = dimensions_ok && guarded;
    #[cfg(not(feature = "render_research"))]
    let uploaded_by_guard = false;
    if dimensions_ok && !uploaded_by_guard {
        shape.update(unsafe { get_internal_gl() }.quad_context, shadow);
    }
    let valid = dimensions_ok && unsafe { miniquad::gl::glGetError() } == 0;
    if valid {
        self.full_bytes += shadow.len() as u64;
    } else {
        self.reason = Some("Mutable font atlas recovery upload failed; recreate painter".into());
    }
    Publication {
        texture: previous,
        bytes: if valid { shadow.len() } else { 0 },
        previous_owned,
        pooled: false,
        valid,
    }
}
```

**发布主函数 `prpr/src/ui/font_atlas_pool.rs:730-977`：**

```rust
pub fn publish(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], writes: &[[u32; 4]]) -> Publication {
    #[cfg(feature = "render_research")]
    let timing = publish_timing::current();
    #[cfg(feature = "render_research")]
    let prepare_span = timing.and_then(|timing| timing.start(Phase::Prepare));
    let previous_owned = self.owns(previous);
    if self.context != context_id() {
        return self.mutable_fallback(previous, width, height, shadow, "Managed context changed".into(), previous_owned);
    }
    if bitmap_bytes(width, height) != Some(shadow.len())
        || writes.iter().any(|&[x0, y0, x1, y1]| x0 >= x1 || y0 >= y1 || x1 > width || y1 > height)
    {
        return self.mutable_fallback(previous, width, height, shadow, "Invalid font bitmap/write".into(), previous_owned);
    }
    if self.api.is_none() {
        return self.mutable_fallback(previous, width, height, shadow, "Font pool unavailable".into(), previous_owned);
    }
    if self.shape != (width, height) && !self.slots.is_empty() {
        return self.mutable_fallback(previous, width, height, shadow, "Atlas shape changed without reset".into(), previous_owned);
    }
    #[cfg(feature = "render_research")]
    if self.research_mode == 3 && !self.research_runtime_fallback_triggered {
        self.research_runtime_fallback_triggered = true;
        return self.mutable_fallback(
            previous,
            width,
            height,
            shadow,
            "Research: forced first-publication mutable fallback".into(),
            previous_owned,
        );
    }
    if self.slots.is_empty() {
        if let Some(texture) = self.warm_internal(width, height, shadow, false) {
            self.fresh += 1;
            #[cfg(feature = "render_research")]
            if let Some(timing) = timing {
                timing.outcome(Outcome::Warm);
            }
            return Publication {
                texture,
                bytes: shadow.len() * SLOTS,
                previous_owned,
                pooled: true,
                valid: true,
            };
        }
        return self.mutable_fallback(previous, width, height, shadow, "Font pool loading failed".into(), previous_owned);
    }
    let api = self.api.unwrap();
    self.history.as_mut().unwrap().advance(writes);
    let current = self.slots.iter().position(|slot| Some(slot.texture) == self.current).unwrap();
    let fence = match api.insert() {
        Ok(fence) => fence,
        Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
    };
    if let Some(old) = self.slots[current].fence.replace(fence) {
        api.delete(old);
    }
    let mut available = None;
    for (index, slot) in self.slots.iter().enumerate() {
        if index == current {
            continue;
        }
        #[cfg(feature = "render_research")]
        if self.research_mode == 2 {
            continue;
        }
        let ready = if let Some(fence) = slot.fence {
            self.polls += 1;
            api.ready(fence)
        } else {
            Ok(true)
        };
        match ready {
            Ok(true) => {
                available = Some(index);
                break;
            }
            Ok(false) => {}
            Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
        }
    }
    if let Some(index) = available {
        if let Some(fence) = self.slots[index].fence.take() {
            api.delete(fence);
        }
        let version = self.history.as_ref().unwrap().generation();
        let rectangles = self.history.as_ref().unwrap().since(self.slots[index].version);
        let texture = self.slots[index].texture;
        let mut bytes = 0usize;
        #[cfg(feature = "render_research")]
        let pbo_choice = pbo::decision(self.pbo_stats.requested, self.slots[index].pbo.is_some(),
            self.slots[index].pbo.as_ref().map_or(0, pbo::Owner::capacity), self.scratch.capacity(),
            width, height, rectangles.iter().map(|rect| [rect.x, rect.y, rect.w, rect.h]));
        #[cfg(feature = "render_research")]
        if let Some(timing) = timing {
            timing.reuse(current, index, version, self.slots[index].version, rectangles.is_empty());
            timing.finish(Phase::Prepare, prepare_span);
        }
        #[cfg(feature = "render_research")]
        let used_pbo = {
            match pbo_choice {
                PboDecision::Attempt { bytes: expected } => {
                    let result = upload_pbo(self.slots[index].pbo.as_mut().unwrap(), texture, width, shadow,
                        &rectangles, &mut self.scratch, timing);
                    match result {
                        PboUpload::Applied(uploaded) => { debug_assert_eq!(uploaded, expected); bytes = uploaded; true }
                        PboUpload::Declined => {
                            self.pbo_count(|stats| stats.declined_publications = stats.declined_publications.saturating_add(1));
                            false
                        }
                        PboUpload::Failed => {
                            self.pbo_count(|stats| stats.runtime_errors = stats.runtime_errors.saturating_add(1));
                            return self.mutable_fallback(previous, width, height, shadow, "Font PBO upload failure".into(), previous_owned);
                        }
                    }
                }
                PboDecision::Missing => {
                    self.pbo_count(|stats| stats.missing_publications = stats.missing_publications.saturating_add(1));
                    false
                }
                PboDecision::Oversize => {
                    self.pbo_count(|stats| stats.oversize_publications = stats.oversize_publications.saturating_add(1));
                    false
                }
                PboDecision::Off | PboDecision::Empty => false,
            }
        };
        #[cfg(not(feature = "render_research"))]
        let used_pbo = false;
        #[cfg(feature = "render_research")]
        let used_client_guard = if !used_pbo && !rectangles.is_empty() && self.client_guard_required() {
            let raw = texture.raw_miniquad_texture_handle();
            let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |context| {
                upload_client_delta(raw, context, width, shadow, &rectangles, &mut self.scratch, timing)
            });
            match result {
                Ok(Ok(uploaded)) => { bytes = uploaded; true }
                Ok(Err(())) => return self.mutable_fallback(previous, width, height, shadow, "Font delta scratch allocation failed".into(), previous_owned),
                Err(error) => {
                    let reason = format!("Font client delta guard failed: {error}");
                    self.block_client_guard(reason.clone());
                    return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned);
                }
            }
        } else {
            false
        };
        #[cfg(not(feature = "render_research"))]
        let used_client_guard = false;
        if !used_pbo && !used_client_guard {
            for rect in rectangles {
                #[cfg(feature = "render_research")]
                let copy_span = timing.and_then(|timing| timing.start(Phase::Copy));
                self.scratch.clear();
                let needed = (rect.w as usize) * (rect.h as usize) * 4;
                if self.scratch.try_reserve(needed).is_err() {
                    #[cfg(feature = "render_research")]
                    if let Some(timing) = timing {
                        timing.finish(Phase::Copy, copy_span);
                    }
                    return self.mutable_fallback(previous, width, height, shadow, "Font delta scratch allocation failed".into(), previous_owned);
                }
                for y in rect.y..rect.y + rect.h {
                    let start = ((y * width + rect.x) * 4) as usize;
                    self.scratch.extend_from_slice(&shadow[start..start + (rect.w * 4) as usize]);
                }
                #[cfg(feature = "render_research")]
                if let Some(timing) = timing {
                    timing.finish(Phase::Copy, copy_span);
                }
                #[cfg(feature = "render_research")]
                let update_span = timing.and_then(|timing| timing.start(Phase::Update));
                texture.raw_miniquad_texture_handle().update_texture_part(
                    unsafe { get_internal_gl() }.quad_context,
                    rect.x as i32,
                    rect.y as i32,
                    rect.w as i32,
                    rect.h as i32,
                    &self.scratch,
                );
                #[cfg(feature = "render_research")]
                if let Some(timing) = timing {
                    timing.finish(Phase::Update, update_span);
                    timing.uploaded(self.scratch.len());
                }
                bytes += self.scratch.len();
            }
        }
        #[cfg(feature = "render_research")]
        let error_span = timing.and_then(|timing| timing.start(Phase::GlError));
        let error = unsafe { miniquad::gl::glGetError() };
        #[cfg(feature = "render_research")]
        if let Some(timing) = timing {
            timing.finish(Phase::GlError, error_span);
        }
        if error != 0 {
            #[cfg(feature = "render_research")]
            if used_pbo {
                self.pbo_count(|stats| stats.runtime_errors = stats.runtime_errors.saturating_add(1));
            }
            return self.mutable_fallback(previous, width, height, shadow, "Font delta GL error".into(), previous_owned);
        }
        self.slots[index].version = version;
        self.current = Some(texture);
        self.reused += 1;
        self.delta_bytes += bytes as u64;
        #[cfg(feature = "render_research")]
        if used_pbo {
            self.pbo_count(|stats| {
                stats.accepted_publications = stats.accepted_publications.saturating_add(1);
                stats.accepted_bytes = stats.accepted_bytes.saturating_add(bytes as u64);
            });
        }
        #[cfg(feature = "render_research")]
        if let Some(timing) = timing {
            timing.outcome(Outcome::Reused);
        }
        return Publication {
            texture,
            bytes,
            previous_owned,
            pooled: true,
            valid: true,
        };
    }
    let Some(retire) = retirement_candidate(self.slots.iter().map(|slot| slot.texture), previous, self.current) else {
        return self.mutable_fallback(previous, width, height, shadow, "No retireable atlas version".into(), previous_owned);
    };
    self.busy += 1;
    flush_retirement();
    let old = self.slots.remove(retire);
    if let Some(fence) = old.fence {
        api.delete(fence);
    }
    old.texture.delete();
    let texture = match self.make_image(width, height, shadow, false) {
        Ok(texture) => texture,
        Err(reason) => return self.mutable_fallback(previous, width, height, shadow, reason, previous_owned),
    };
    self.slots.push(Slot {
        texture,
        version: self.history.as_ref().unwrap().generation(),
        fence: None,
        #[cfg(feature = "render_research")]
        pbo: None,
    });
    self.current = Some(texture);
    self.fresh += 1;
    self.full_bytes += shadow.len() as u64;
    #[cfg(feature = "render_research")]
    if let Some(timing) = timing {
        timing.outcome(Outcome::BusyFresh);
    }
    Publication {
        texture,
        bytes: shadow.len(),
        previous_owned,
        pooled: true,
        valid: true,
    }
}
```

**池 Drop `prpr/src/ui/font_atlas_pool.rs:1078-1101`：**

```rust
impl Drop for Pool {
    fn drop(&mut self) {
        #[cfg(feature = "render_research")]
        if self.immutable.blocked || immutable_context_blocked() {
            return;
        }
        if self.context != context_id() {
            return;
        }
        if !self.slots.is_empty() {
            flush_retirement();
        }
        for slot in self.slots.drain(..) {
            if let (Some(api), Some(fence)) = (self.api, slot.fence) {
                api.delete(fence);
            }
            if Some(slot.texture) != self.current {
                crate::ext::queue_texture_deletion(slot.texture);
            }
        }
    }
}
```

**生产消费者 `prpr/src/ui/text.rs:484-511, 590-650`：**

```rust
#[cfg(feature = "font_atlas_pool")]
pub fn prepare_font_pool_loading(&mut self) {
    if !self.production_shadow() {
        return;
    }
    let (width, height) = self.brush.texture_dimensions();
    let atlas = self.production_atlas.as_mut().unwrap();
    if atlas.disabled {
        return;
    }
    if let Some(texture) = atlas.pool.warm(width, height, &atlas.shadow) {
        if texture != self.cache_texture
            && !atlas.pool.owns(self.cache_texture)
            && self.cache_context == unsafe { get_internal_gl() }.quad_context.context_id()
        {
            unsafe { get_internal_gl() }.flush();
            unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
            self.cache_texture.delete();
        }
        self.cache_texture = texture;
        self.cache_context = unsafe { get_internal_gl() }.quad_context.context_id();
        self.cache_valid = true;
    } else {
        atlas.disabled = true;
        #[cfg(feature = "render_research")]
        if atlas.pool.client_guard_blocked() {
            self.cache_valid = false;
        }
    }
}
#[cfg(feature = "font_atlas_pool")]
fn production_publish_atlas(&mut self) {
    let (width, height) = self.brush.texture_dimensions();
    let Some(atlas) = self.production_atlas.as_mut() else {
        return;
    };
    if !atlas.dirty {
        return;
    }
    if !self.cache_valid {
        atlas.writes.clear();
        self.production_recover_mutable();
        return;
    }
    #[cfg(feature = "render_research")]
    let upload_begin = std::time::Instant::now();
    #[cfg(feature = "render_research")]
    let publication = crate::glyph_publish_timing::PublicationGuard::begin(crate::glyph_publish_timing::Context {
        painter_id: self.lab_painter_id,
        process_seq: self.lab_process_seq,
        caller: "TextPainter::production_publish_atlas",
        caller_file: file!(),
        caller_line: line!(),
        width,
        height,
        write_count: atlas.writes.len(),
    });
    let result = atlas.pool.publish(self.cache_texture, width, height, &atlas.shadow, &atlas.writes);
    if result.texture != self.cache_texture
        && !result.previous_owned
        && self.cache_context == unsafe { get_internal_gl() }.quad_context.context_id()
    {
        unsafe { get_internal_gl() }.flush();
        unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
        self.cache_texture.delete();
    }
    self.cache_texture = result.texture;
    #[cfg(feature = "render_research")]
    if let Some(publication) = publication {
        publication.finish();
    }
    #[cfg(feature = "render_research")]
    {
        lab_text_add(1, result.bytes as f64);
        lab_text_add(2, upload_begin.elapsed().as_secs_f64() * 1000.);
    }
    atlas.writes.clear();
    atlas.dirty = false;
    if result.valid {
        self.cache_valid = true;
        self.cache_context = unsafe { get_internal_gl() }.quad_context.context_id();
        if !result.pooled {
            atlas.disabled = true;
            atlas.shadow.clear();
            atlas.shadow.shrink_to_fit();
        }
    } else {
        self.cache_valid = false;
        atlas.disabled = true;
        atlas.error = Some("Font pool publication invalid; recovering mutable atlas".into());
        tracing::error!("font atlas publication invalid; attempting one mutable recovery");
        self.production_recover_mutable();
    }
}
```

**生产请求开关 `prpr/src/ui/text.rs:843-883`：**

```rust
#[cfg(feature = "font_atlas_pool")]
let production_requested = {
    #[cfg(feature = "render_research")]
    {
        LAB_PRODUCTION_POOL.with(|slot| slot.get())
    }
    #[cfg(not(feature = "render_research"))]
    {
        true
    }
};
#[cfg(feature = "font_atlas_pool")]
let production_conflict = {
    #[cfg(feature = "render_research")]
    {
        production_requested && (LAB_ATLAS_RENAME.with(|slot| slot.get()) || LAB_ATLAS_POOL_MODE.with(|slot| slot.get()) != 0)
    }
    #[cfg(not(feature = "render_research"))]
    {
        false
    }
};
#[cfg(feature = "font_atlas_pool")]
let production_atlas = (production_requested && !production_conflict).then(|| ProductionAtlas {
    pool: {
        #[cfg(feature = "render_research")]
        {
            font_atlas_pool::Pool::new_for_research(LAB_PRODUCTION_POOL_MODE.with(|slot| slot.get()))
        }
        #[cfg(not(feature = "render_research"))]
        {
            font_atlas_pool::Pool::new()
        }
    },
    shadow: Vec::new(),
    writes: Vec::new(),
    dirty: false,
    disabled: false,
    recovery_attempted: false,
    error: None,
});
```

#### 4.4 调用链与生命周期（加载期→游玩期→清理/失效）

**加载期：**

1. `TextPainter::new(font, fallback)` 创建 `GlyphBrush` 和原 `cache_texture`；随后根据 `production_requested` 与 `production_conflict` 决定是否创建 `ProductionAtlas`。
2. 非 research 构建：`production_requested = true`，`production_conflict = false`；只要 `font_atlas_pool` feature 编入，就创建 `ProductionAtlas`，`Pool::new()` 探测 ES3 sync。
3. research 构建：`production_requested` 来自 thread-local `LAB_PRODUCTION_POOL`，默认 false；研究请求可显式打开；如果同时打开旧的 rename/lab pool，`production_conflict=true`，生产池不创建，避免两套策略混用。
4. `phira::run` 里 PGR 字体先 `TextPainter::new` 再 `prepare_font_pool_loading()`；default 字体先 `normalize_to`（自定义字体）再 `prepare_font_pool_loading()`。Bold 没有额外 64 MiB 预热（`Bold` 的 `production_requested=false`，报告 `mirror_bytes=0`）。
5. `prepare_font_pool_loading()` 先 `production_shadow()`：按当前 brush 尺寸分配完整 RGBA mirror（2048² = 16 MiB）；再 `pool.warm(width,height,shadow)`：检查预算、flush 旧 loading UI、分配 4 张完整纹理和 fence、设置 `current=slot0`，并把 `cache_texture` 指向池纹理。
6. `warm` 失败：`atlas.disabled=true`，原 `cache_texture` 保持；不删除原可变纹理，不丢已缓存 glyph。这是“加载 warm 失败不改变原路径”的关键。

**游玩期：**

1. `TextPainter::submit` 调 `brush.process_queued`；每次 glyph 更新回调收到 `(rect, tex_data)`。
2. 生产路径把 `tex_data` 的 alpha 转成 `[255,255,255,alpha]` 写入 `data_buffer`，再复制到 `atlas.shadow` 的对应矩形；`atlas.writes.push([x0,y0,x1,y1])`，`dirty=true`。它**不直接写当前 GL 纹理**。
3. `process_queued` 返回后，`production_publish_atlas()`：
   - 如果 `dirty=false`，直接返回；
   - 如果 `cache_valid=false`，清 writes 并 `production_recover_mutable()`；
   - 否则 `pool.publish(cache_texture,width,height,shadow,writes)`；
   - 返回的 `texture` 成为新的 `cache_texture`；
   - 如果旧 texture 不是池拥有且 context 未变，flush + clear bindings + delete 旧 texture；
   - `valid=false` 时 `cache_valid=false`，`atlas.disabled=true`，尝试一次 `production_recover_mutable()`。
4. `pool.publish` 的正常增量路径只写一个 ready 的 slot；旧 current 保留给在途 draw；下一次发布再检查它的 fence。
5. 字形几何/UV/颜色仍由 `glyph_brush` 生成；池只换纹理对象，不重建 vertices/indices。
6. resize：`TextureTooSmall` 时 flush、`pool.reset()`、清 shadow/writes/dirty、重新预算、分配新 `cache_texture`、`brush.resize_texture(suggested)`；如果新尺寸超 64 MiB，`atlas.disabled=true`，后续走 mutable。

**清理/失效：**

1. 没有 `Config` 开关，也没有 Settings 按钮；字体池随 `font_atlas_pool` feature 编入、随 `TextPainter` 生命周期存在。要关闭只能不编入 feature 或走研究 thread-local（仅 research）。
2. `TextPainter` drop → `ProductionAtlas` drop → `Pool::drop`：检查 context，flush retirement，删除 fence；非 current texture 走 `queue_texture_deletion`，current 由 TextPainter 自己排队删除，避免双删。
3. context 变化：`publish`/`mutable_fallback` 检测 `self.context != context_id()`；返回 `valid=false`，不删除外来名字；调用方必须重建 Painter。
4. `cache_valid=false`：发布失败/恢复失败时停止字体 draw，保留 CPU shadow；只尝试一次 mutable recovery，避免无限循环。
5. 没有磁盘 cache；字体池是纯内存/GPU 资源，不参与 `dir::clear_cache()`。这与 P-01/P-02 的 `RenderCache` 生命周期不同。
6. 真实 UI 清 cache 不会清字体池；字体池也不受 `chart_pre_compile` 影响。不要把它和谱面 IR 的清理混为一谈。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| Cargo feature（prpr） | `font_atlas_pool = []`（15） | 独立 feature，不依赖 `chart_pre_compile`/`render_research` |
| prpr 默认 feature | `default = ["log", "font_atlas_pool"]`（18） | 单独 prpr 默认编入 |
| phira feature | `font_atlas_pool = ["prpr/font_atlas_pool"]`（15） | 主 crate 转发 |
| phira 默认 feature | `default = ["video", "font_atlas_pool", "chart_pre_compile"]`（18） | 主 crate 默认编入 |
| phira-main 默认 feature | `default = ["video", "chart_pre_compile"]`（8） | **没有转发 `font_atlas_pool`**；桌面主程序默认不编入 |
| Android Gradle | `cargo ndk -t arm64-v8a --platform 26 build --locked --release -p phira` | 构建 `phira`，其默认 feature 含 `font_atlas_pool`，所以 Android 普通 APK 编入 |
| 运行请求 | 非 research `production_requested=true`（843-853） | 编入即请求，无 Config 开关 |
| research 请求 | `LAB_PRODUCTION_POOL` thread-local，默认 false | 研究对照可开关；与 rename/lab pool 冲突时不创建 |
| 每池 slot | `SLOTS=4` | 4 个完整版本 |
| 每池预算 | `BUDGET=64 MiB` | 4 × 2048² × 4 B |
| 两个主 Painter | PGR + default | 各 64 MiB，合计 **128 MiB 名义 GPU** |
| CPU mirror | 每 Painter 16 MiB | 两份 **32 MiB**；Bold 不 warm，不额外 64 MiB |
| scratch | 每 Painter 1 MiB | `scratch.try_reserve_exact(1024*1024)` |
| 能力 | Android ES3 core sync | 非 Android `sync_api::load()` 返回 Err，自动 mutable fallback |
| 环境变量 | 无 | 只走 feature + 研究 thread-local |
| 默认是否实际启用 | **非 research 编入即请求；实际池化取决于 ES3 sync 与预算** | 不能简单写成“所有构建默认池化” |

默认值真相必须分三层：

1. **构建层**：`phira/Cargo.toml` 默认含 `font_atlas_pool`，Android `-p phira` 会编入；`prpr/Cargo.toml` 默认也含。桌面 `phira-main` 的默认 feature 只含 `video`/`chart_pre_compile`，没有转发 `font_atlas_pool`，所以桌面默认二进制不编入——这是一个真实的构建差异，不能忽略。
2. **运行层**：非 research 构建的 `production_requested=true`，没有 `Config` 开关；只要 `TextPainter` 创建且 feature 在，就创建 `ProductionAtlas`。research 构建默认 false，只有研究请求打开。
3. **能力层**：`Pool::new()` 需要 Android ES3 的 `glFenceSync/glClientWaitSync/glDeleteSync`；非 Android 或探测失败时 `api=None`，`warm` 失败后 `atlas.disabled=true`，走原可变纹理。`Blocking_waits=0` 是设计约束，不是所有平台都已实际池化。

### 6. 测量方法与完整数据（完整表格；注明窗口、样本、设备、同 SO/ABBA）

#### 6.1 production-font-retry-v1（生产通用消费者，1800 帧）

- 设备/谱面：Mali-T880 / 2560×1440；`f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip`。
- 同 SO：`2bc2744c2f052675cc008ae513b047e0f9d1ca73c24fb9fbe2e7f4765147a835`；44 源冻结；source commit `2b57559d`。
- 每轮 120 预热 + 1800 采样（1799 个 begin interval）；low/normal 各自独立 ABBA，顺序 `old/new/new/old`。
- 唯一变量：`font_production_pool` bool；其余 common font、line DCE、`chart_pre_compile=true`、`aggressive=false`、无 native/CPU/GPU 诊断固定。
- 应用墙钟：`update+render+flush`；interval 是 begin-to-begin，包含未分段的框架/native swap/yield；SF 是独立 10 s 左右采样。

**Low：**

| 轮次 | 路径 | 应用均值 ms | 应用 p99 ms | 应用 max ms | >30 应用帧 | FPS | interval p99 ms | interval max ms | >30 interval | SF max ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | old | 8.161298 | 39.861458 | 66.584374 | 57 | 57.067326 | 43.670906 | 69.826042 | 63 | 66.963541 |
| 1 | new | 7.370683 | 13.405141 | 17.998437 | 0 | 59.737722 | 19.305605 | 23.532813 | 0 | 16.880729 |
| 2 | new | 7.313420 | 13.666745 | 19.435417 | 0 | 59.738372 | 19.585177 | 29.583334 | 0 | 16.844270 |
| 3 | old | 8.273587 | 39.324208 | 60.296876 | 55 | 57.285289 | 43.317979 | 64.445312 | 63 | 50.281771 |

- 组均：应用 `8.217443→7.342051 ms`，`−10.65%`；FPS `57.176308→59.738047`；interval p99 `43.494443→19.445391 ms`；max `69.826042→29.583334 ms`；interval >30 `126→0`（应用 >30 为 112）。
- SF：new max `16.880729/16.844270 ms`、>30 `0/0`；old max `66.963541/50.281771 ms`、>30 `40/40`。
- 说明：new 仍有 `29.583334 ms` 的 interval max，不能声称“低画质所有帧 <30 ms”只凭组均；但该轮 >30 为 0。

**Normal：**

| 轮次 | 路径 | 应用均值 ms | 应用 p99 ms | 应用 max ms | >30 应用帧 | FPS | interval p99 ms | interval max ms | >30 interval | SF max ms |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | old | 11.093397 | 43.244729 | 78.832812 | 88 | 46.286078 | 47.959552 | 81.083333 | 96 | 50.332292 |
| 1 | new | 9.878001 | 15.356985 | 20.343750 | 0 | 57.824464 | 21.109823 | 28.897917 | 0 | 33.543750 |
| 2 | new | 9.742094 | 15.966698 | 20.627603 | 0 | 58.008819 | 20.771938 | 23.562500 | 0 | 33.505729 |
| 3 | old | 11.222913 | 43.838625 | 68.308334 | 90 | 45.944555 | 48.536875 | 71.211458 | 96 | 66.963541 |

- 组均：应用 `11.158155→9.810048 ms`，`−12.08%`；FPS `46.115317→57.916641`；interval p99 `48.248214→20.940880 ms`；max `81.083333→28.897917 ms`；interval >30 `192→0`（应用 >30 为 178）。
- SF：new max `33.543750/33.505729 ms`、>30 `7/3`（合计 10）；old max `50.332292/66.963541 ms`、>30 `113/133`。**不能**声称所有呈现 <30 ms 或普通稳定 60。
- 窗口：normal old 实际 chart 时长 `38.867/39.156 s`，new `31.111/31.013 s`；不是每轮固定 30 s。
- 应用 max：new `20.343750/20.627603 ms`，但 interval max 仍 `28.897917/23.562500`；应用墙钟和 begin-to-begin 的区别要分开看。

#### 6.2 default-font-pool-v1（整体策略，1200 帧）

- 同 SO：`7f932503a1246e942935da118fcc7a3e98523953d74b2b0f78cbff7cdad74799`；配套旧 SO 以 `46c972` 为前缀；120 预热 + 1200 采样；low/normal 各 `font0/font1/font1/font0`。
- `font0` = 旧 mutable；`font1` = 新 RGBA pool + rename/compact 整体策略；差异**不是**各细项因果。
- 应用墙钟/interval 口径同上；实际 1200 样本时长随 FPS 变化约 20–25 s，不是固定 20 s。

**Low：**

| 轮次 | 策略 | 应用均值 ms | 应用 p99 ms | 应用 max ms | >30 应用帧 | FPS | interval p99 ms | interval max ms | >30 interval |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | font0 | 8.635875 | 42.325996 | 62.272395 | 55 | 55.805576 | 45.781020 | 65.320833 | 62 |
| 1 | font1 | 7.673458 | 15.827411 | 18.925000 | 0 | 59.690166 | 20.574157 | 30.659375 | 1 |
| 2 | font1 | 7.621245 | 14.802635 | 19.704688 | 0 | 59.682654 | 20.152000 | 27.089063 | 0 |
| 3 | font0 | 8.630431 | 39.479256 | 66.091667 | 55 | 56.265582 | 43.897646 | 70.400000 | 64 |

- 组均：应用 `8.633153→7.647352 ms`，`−11.42%`；FPS `56.035579→59.686410`；interval p99 `44.839333→20.363079 ms`；max `70.4→30.659375 ms`；interval >30 `126→1`（应用 >30 为 110）。
- 两轮 font1 的 FPS 为 `59.690/59.683`；font0 为 `55.806/56.266`；ABBA 四轮 FPS `55.806/59.690/59.683/56.266`。
- 唯一 >30 是 B1 `30.659375 ms`；该帧前 app `5.133 ms`、glyph `0`，不能归因 GPU/driver。

**Normal：**

| 轮次 | 策略 | 应用均值 ms | 应用 p99 ms | 应用 max ms | >30 应用帧 | FPS | interval p99 ms | interval max ms | >30 interval |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | font0 | 11.493375 | 54.568504 | 63.996354 | 76 | 48.346950 | 56.843791 | 67.590105 | 85 |
| 1 | font1 | 9.755336 | 15.908621 | 19.639062 | 0 | 59.252734 | 20.595124 | 24.301042 | 0 |
| 2 | font1 | 9.734109 | 16.674504 | 19.321875 | 0 | 59.202199 | 21.154698 | 22.224480 | 0 |
| 3 | font0 | 11.366431 | 54.035985 | 65.603125 | 77 | 48.519582 | 56.336760 | 68.247917 | 82 |

- 组均：应用 `11.429903→9.744722 ms`，`−14.74%`；FPS `48.433266→59.227467`；interval p99 `56.590276→20.874911 ms`；max `68.247917→24.301042 ms`；interval >30 `167→0`（应用 >30 为 153）。
- ABBA FPS：`48.347/59.253/59.202/48.520`；组均 `48.4333→59.2275`。
- 质量两组的 3,686,400 像素逐 RGBA diff 0；raw/request/meta/host/SF/PNG hash 原样。

#### 6.3 质量、回退与预算

| 项 | 结果 |
|---|---|
| production-font-retry quality | 质量会话 9588 terminal0 / 14 jobs / 10 pairs；每对 3,686,400 像素逐 RGBA diff 0 |
| callback 轨迹 | 148/168 区域、长度、顺序相同；`drop=0` |
| default-font-pool quality | 两组（low/normal）各 3,686,400 像素逐 RGBA diff 0；raw/request/meta/host/SF/PNG hash 原样 |
| PGR/default warm | 各 64 MiB；PGR reuse 59，default reuse 3 |
| missing/forced busy | missing 镜像/warm/slots 0；forced busy fresh 59/3，full 16 MiB/次，polls 0 |
| runtime fallback | 每活跃 Painter 一次全 16 MiB mutable fallback；existing glyph 保留、slots/mirror 0、cache valid |
| Bold | unused，不额外 64 MiB 预热，不要求触发 |
| 预算 | 每池 4×2048²×4 B = 64 MiB；PGR+default = **128 MiB 名义 GPU**；两份 CPU mirror = **32 MiB**；scratch 每 Painter 1 MiB |
| 物理内存 | **未测**；`nominal_storage_bytes` 不是 RSS/物理 GPU；不能把 128 MiB 当实际占用 |
| 失败项 | 首 production performance 9543 terminal1（SF 采集晚启动，仅 4 poll/2.996 s），原数据保留；normal new SF max 33.5 ms / >30 合计 10；low default B1 30.659 ms 一帧 |
| 未验证 | 全曲、其它 GPU、EGL loss、OOM、完整 Main UI 多轮、真正 UI clear |

#### 6.4 原始记录（README 与 AGENTS 原文）

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

`本地证据/low-quality-normal/README.md` 第 187、189 行原文：

> 代价：当前加载PGR与default两个池，每池4×16MiB名义GPU图像，合计128MiB名义GPU容量，加两份CPU镜像32MiB及scratch。不是测得的物理内存；静态default只复用3次，明显不值得常驻完整池。后续收紧到实际动态字体、等待加载上传完成并做真实采样预热，测试从gameplay首帧到长曲/重入/字体变化。原始不重命名路径每轮仅约1.6MB，增量版本上传仍高于它，但避开了原路径当前设备的读写等待尖峰。
>
> 当前是研究后端，不是已经默认启用的生产优化。移入源码后的branch-lab构建/测试记录与旧注入后端记录分别保存；代码路径迁移不冒充额外设备验收。

`本地证据/AGENTS.md` 第 537 行（生产重试）原文：

> 生产字体重试性能已完整执行（40503实际terminal0，low/normal各completeTrue）：sameSO2bc2744/44源/2b57559，仅font_production_pool bool对照，各1800帧/1799 begininterval，无native/cpu/gpu诊断，strictSF8作业10poll/running/jobID/ring/raw重放全部通过，旧失败1200不混。production-font-retry-v1-summary.json独立ownABBA：Low应用均8.21744→7.34205ms减少10.65%，FPS57.1763→59.7380；p99 interval均43.494→19.445，>30累计126→0，max69.826→29.583334。普通应用11.15815→9.81005ms减少12.08%，FPS46.1153→57.9166；p99 interval48.248→20.941，>30累计192→0，max81.0833→28.897917。共同chartTime补充亦有趋势但不外推全曲/GPU；normal旧38.867/39.156s/new31.111/31.013s，不能说每轮都固定30s。Low新SFmax16.8807/16.8443且>30=0；普通新SFmax33.54375/33.505729且>30合计10，不能声称所有呈现<30或普通稳定60。新的common消费者有真实整体收益，不能用旧research实验的11–15%替代此10.65/12.08%。

`本地证据/AGENTS.md` 第 541 行（生产通用池接入）原文：

> 生产通用RGBA字体池已接入并本地提交2b57559dd7cb97479c1f4c8b019ca03f1788c796（接入通用字体图集池，main未push）：新增独立font_atlas_pool feature且normal default启用，不依赖chartPrecompile/画质；PGR/default主入口加载warm，自定义字体normalize后warm，Bold不额外64MiB预热。text保持glyph代表/process/布局/UV/透明顺序，完整RGBA shadow/policy0四slot零超时，缺cap/预算无shadow退原mutable，runtime保全current并全shadow恢复后释放mirror且以后mutable，validfalse仅一次新texture恢复/失败保留CPU数据并停字体draw，foreigncontext不删旧native同号对象。研究TLS only旧路径oracle/0auto1missing2busy3firstpubfallback，negativechannel/defer/coalesce不进入生产，ctx同号EGLloss框架未证。

`本地证据/AGENTS.md` 第 543 行（质量/预算）原文：

> 已实际default/no-default/research Windows检查与4个budget/retire身份/既有Delta oracle通过；首default构建脚本因WindowsOS5neverexecuted失败log保留、重试通过（64591terminal101/19194terminal0），34877Android API26真正terminal0。SO2bc2744c2f052675cc008ae513b047e0f9d1ca73c24fb9fbe2e7f4765147a835冻结44源与commit核对。9588质量terminal0/14jobs/10pairs每3686400px逐RGBA0，callback148/168区域/长度/顺序同且drop0；newcommon无publication trace，不伪称每GLtransfer记录。PGR/default实际warm64MiB各/reuse59/3；missing镜像/warm/slots0；forcedbusy fresh59/3及full16MiB/次polls0；runtime每活跃Painter一次全16MiB mutablefallback，existingGlyph最终保留、slots/mirror0且cachevalid，Boldunused不要求触发。源码main接线≠MainUI完整操作/全曲/OOM/GPUloss验收。

`本地证据/AGENTS.md` 第 547 行（整体策略）原文：

> 新的字体整体策略同SO验证已完成（86395 terminal0，default-font-pool-v1-summary.json）：固定旧SO7f932/46c972/aggressivefalse，普通Chart/AutoPlay/linecompileron，font0旧mutable与font1 RGBA pool的差只有mode/rename/compact整体策略；不是各细项因果。LowABBA FPS55.806/59.690/59.683/56.266，font0/1均56.0356→59.6864；appwall8.63315→7.64735ms减少11.42%，>30两轮累计126→1，max70.4→30.659375（那一帧前app5.133、glyph0，不归因GPU/driver）。普通画质sample2/CPUlinefallbackABBA48.347/59.253/59.202/48.520 FPS，均48.4333→59.2275；app11.42990→9.74472ms减少14.74%，>30累计167→0，max68.2479→24.3010。两组quality各3686400pixels逐RGBA diff0、raw/request/meta/host/SF/PNG hash原样。实际1200样本时长随FPS改变约20–25秒，不应称每轮固定20秒；共同chartTime区间附加分层仍有趋势但78–82s low两者都约59.7，不外推全曲/所有GPU。这次有真实FPS/30ms改善；此前defaultFont新baseline40.42/58.17与当前off48.43/56.04分属不同窗口，不混池。尚只研究入口，没有生产默认接入，正式用户现在仍旧路径。

`本地证据/AGENTS.md` 第 737 行（池代价）原文：

> 当前池代价：加载阶段每个已预热Painter四份2048²RGBA，共64MiB GPU名义容量；研究后端PGR与default均预热，合计128MiB名义GPU容量+32MiB CPU镜像，未测真实物理内存。加载上传完成/真实采样资源预热尚需完善，当前性能样本有120帧预热。非Android回退快照；生产通用能力层仍待库实现。后续优先把浪费的静态字体池删掉、测更长窗口与复杂字体/lifecycle，然后回到真正GPU常驻Render IR；缓存UI/清理接入仍是有效生产消费者的首阶段必要条件。不能用字体成功代替chart编译器目标。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

- `production-font-retry-v1`：质量会话 9588（14 jobs/10 pairs），每对 3,686,400 像素逐 RGBA diff 0；callback 148/168 区域、长度、顺序相同，drop 0。
- `default-font-pool-v1`：low/normal 两组 quality 各 3,686,400 像素逐 RGBA diff 0；raw/request/meta/host/SF/PNG hash 原样。
- PGR/default 主入口实际 warm 64 MiB 各；PGR reuse 59，default reuse 3。
- missing 能力/预算：镜像/warm/slots 0；forced busy：fresh 59/3、full 16 MiB/次、polls 0；runtime：每活跃 Painter 一次全 16 MiB mutable fallback，existing glyph 保留，slots/mirror 0，cache valid。
- Bold 不 warm，不额外 64 MiB；没有把 Bold 的空报告当成池化成功。
- 首 default 构建脚本 Windows OS5 never-executed 失败日志保留；重试 64591 terminal101/19194 terminal0；34877 Android API26 真正 terminal0。失败没有被伪装成通过。

#### 7.2 失败项与限制

- **首 production performance 失败**：9543 terminal1；low-old native 1200 完成，但 SF 采集晚启动，仅 4 poll/2.996 s，捕获末 job 已 done，严格失败；原 `runs[]/False/failure/raw/measuredSHA` 全保留，不恢复完整。
- **Normal 呈现 max 仍 33.5 ms**：new SF max `33.543750/33.505729 ms`，>30 合计 10；不能声称“所有呈现 <30 ms”或“普通稳定 60”。
- **Low 有一帧 30.659 ms**：default-font-pool B1 的 interval max 30.659375 ms；该帧前 app 5.133、glyph 0，不归因 GPU/driver。
- **128 MiB 不是物理内存**：只是 4 slot × 16 MiB × 2 Painter 的名义容量；没有 RSS/物理 GPU 测量；静态 default 只复用 3 次，“常驻完整池是否值得”仍未证明。
- **没有真实 Main UI 全流程**：主入口加载 warm 与源码接线 ≠ 完整 Main UI 导入/点击/全曲/OOM/GPU loss 验收。
- **没有 EGL loss**：`context_id` 能发现 managed context 变化，但同一 managed context 内的 native EGL surface 丢失没有证明；`foreigncontext` 不删旧名字只是保守处理。
- **没有全曲/其它 GPU**：1200/1800 帧窗口不是全曲；只有 Mali-T880。
- **没有真实 UI 清 cache**：字体池不参与 `dir::clear_cache`，但这不代表真实 UI 生命周期/多轮重入已验证。
- **研究路径的 PBO/immutable**：PBO/immutable storage 是研究扩展，生产消费者仍以 RGBA slot + zero-timeout fence 为主；不能用固定纹理研究结果替代本生产池。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：整帧。**

理由：

1. **FPS 和应用均值同时改善**：production-font-retry low `57.1763→59.7380`、app `8.21744→7.34205 ms`（−10.65%）；normal `46.1153→57.9166`、app `11.15815→9.81005 ms`（−12.08%）。这不是只减少 API 调用或只迁移阶段。
2. **方向在两组独立 cohort 一致**：default-font-pool low `56.0356→59.6864`、−11.42%；normal `48.4333→59.2275`、−14.74%。两组 SO/窗口不同，不能相加，但方向一致。
3. **机制对应明确**：原路径的长帧/上传等待被“写空闲版本 + 零超时 fence + 增量矩形”替代；HUD 上传不再直接命中当前采样纹理。
4. **为什么是“整帧”**：FPS 是整帧指标；低/普通都从明显低于 60 提升到接近 60，普通从 46 提升到 58，远超“只省 chart 阶段”的局部效应。
5. **质量代价**：完整 RGBA、同一逻辑图集、同一 UV/过滤/顺序；两组 3,686,400 像素 RGBA diff 0。它不是双通道/压缩/低分辨率，理论上比 P-01 的 `mild-lossy` 更强；但仍只对有限画面/设备/字体成立，不能宣布全设备无损。
6. **为什么不是“无”**：FPS +4.48%/+25.59%（production retry）和 +6.52%/+22.29%（default pool）不是噪声；ABBA 前后对照支持。
7. **为什么不是“负”**：普通新路径 SF max 仍有 33.5 ms，但应用 interval >30 为 0，且整体 FPS/均值大幅改善；尾部的残余不否定整帧正收益。

**置信度：**

- 对“这个 SO/设备/有限窗口的整帧 FPS 与均值改善”：**高**。
- 对“机制是版本池/增量/fence 而不是别的变量”：**中高**；production retry 只变 bool，default pool 是整体策略（不是细项因果），两组不能混为“每个子项都有效”。
- 对“全曲/所有 GPU/所有字体/EGL loss/OOM”：**低**，未验证。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`2b57559d` 接入通用字体图集池；`09b2370c`/`be4e1305` 只扩展固定纹理研究，不改变生产池主路径 |
| 参考实现是否默认启用 | **是（非 research 构建；桌面未转发）**。编入即请求 `production_requested=true`；没有 Config 开关。`phira`/`prpr` 默认 feature 含 `font_atlas_pool`；Android `-p phira` 编入。桌面 `phira-main` 未转发，默认不编入 |
| 参考实现是否有消费者 | 有。`TextPainter::new` 创建 `ProductionAtlas`；PGR/default 主入口 `prepare_font_pool_loading`；`submit` callback 写 shadow；`production_publish_atlas` 发布 |
| 是否进 main | 否。参考实现未合入 main，主项目未 push |
| 是否依赖画质 | 否。低/普通都可；但只有 Android ES3 sync + 预算内才实际池化，否则 mutable |
| 是否依赖 `chart_pre_compile` | 否。独立 feature；谱面开关关闭不影响字体池 |
| 是否参与 cache clear | 否。纯内存/GPU，不写磁盘；不要和 `RenderCache` 清理混为一谈 |
| 是否默认池化所有平台 | 否。非 Android `sync_api` 返回 Err；桌面默认也不编入 feature |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **参考实现未 push**：生产消费者已本地提交，但主项目未进 main；没有完成真实 UI 全流程/全曲/其它 GPU 验收。
2. **128 MiB 名义容量代价高**：两个主 Painter 各 64 MiB；静态 default 只用 3 次，完整池是否值得常驻仍未证明；物理内存/RSS 未测。
3. **尾部残余**：normal 新路径 SF max 33.5 ms、>30 合计 10；low 有一帧 30.66 ms。虽然整帧均值/FPS 改善，但不能宣称尖峰清零。
4. **平台限制**：非 Android 无 sync 后端；桌面 `phira-main` 没转发 feature；实际池化只在 Android ES3 + 预算内。
5. **生命周期未闭环**：EGL loss、OOM、完整 Main UI 多轮、真正 UI clear、动态字体变化都未验证。
6. **研究扩展混杂**：PBO/immutable/双通道/coalesce 等研究路径不进入生产；不能用固定纹理研究的 RGBA/性能替代本池。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| fence 未 ready | 4 slot 都忙时不能写 | 零超时 poll；busy 时退休非 current/non-previous 版本并新建；再失败回 mutable |
| 版本覆盖在途读 | 写当前纹理会造成读写依赖/画面撕裂 | current 不写；旧 current 保留到 fence ready；`retirement_candidate` 排除 previous/current |
| 增量漏写 | 32×32 tile 版本表若漏标会留下旧像素 | `history.advance(writes)` 标记所有覆盖 tile；`since(version)` 合并连续 dirty tile；shadow 为权威源；quality RGBA0 |
| shape/resize | 尺寸变化后旧 slot 不匹配 | shape 变化且 slots 非空 → mutable fallback；resize 时 `pool.reset()` 重建 |
| 预算 | 2048² 池 64 MiB；更大图集超预算 | `pool_fits_budget` 在分配前拒绝；`atlas.disabled` 回 mutable |
| context 变化 | 新 context 复用同名 GL 对象 | `context_id` 检查；foreign context 不删旧名字；`valid=false` 要求重建 Painter |
| 双删 | caller previous 与池 current 重叠 | `previous_owned` 在退休前捕获；`publish` 只在 `!previous_owned` 时删旧；Drop 只删非 current |
| GL 错误 | 分配/更新失败后状态未知 | `drain_errors` 有界；失败回 mutable；`valid=false` 停字体 draw，保留 CPU shadow |
| 预算/内存误读 | `nominal_storage_bytes` 不是物理内存 | 报告 scope 明确；未测 RSS/物理 GPU |
| 研究/生产混用 | rename/lab pool 与 production pool 同时开 | `production_conflict` 阻止创建；研究 mode 只用于对照 |

#### 10.3 文档冲突（必须保留原文）

历史引文中的分支标识已归一化为“参考实现”，其余原文未改。以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

**冲突一：README 说“默认构建关闭”，代码实际默认编入/运行请求。**

`本地证据/low-quality-normal/README.md` 第 24 行原文：

> 当前参考实现已按用户最新授权直接保存研究源码，不再构建后还原。真实实现位于 `phira/src/render_research*.rs`、`prpr/src/ui/text.rs/atlas_pool.rs/atlas_delta.rs`、`prpr/src/render_lab.rs`及子模块；由独立Cargo feature启用，默认构建关闭。

`README.md` 第 189 行原文：

> 当前是研究后端，不是已经默认启用的生产优化。移入源码后的branch-lab构建/测试记录与旧注入后端记录分别保存；代码路径迁移不冒充额外设备验收。

代码事实（head `be4e1305`）：

- `prpr/Cargo.toml`：`default = ["log", "font_atlas_pool"]`；
- `phira/Cargo.toml`：`default = ["video", "font_atlas_pool", "chart_pre_compile"]`；
- `prpr/src/ui/text.rs:843-853`：非 research 构建 `production_requested = true`；
- `TextPainter::new` 在该条件下创建 `ProductionAtlas`，主入口 PGR/default 调 `prepare_font_pool_loading()`。

因此 README 的“默认构建关闭”只适用于更早的快照（`9498c734` 时 `phira` 的 `default=["video"]`，且 `2b57559` 之前没有 font pool feature）。在 head，`font_atlas_pool` 已默认编入，非 research 运行请求为 true；准确说法是“默认编入、默认请求，实际池化受平台/预算约束”。`chart_pre_compile` 则是“默认编入、运行 Config 默认 false”，两者不能混为一谈。

**冲突二：README 版本 `9498c734` 落后 head `be4e1305`。**

`README.md` 第 9 行原文：

> 最新源码提交 `9498c73427a8e5d88631826267cb562246590ca0`（接入谱面预编译开关，本地未push）。

参考实现 head 是 `be4e1305823bfebdad1853d7b333e0f79c3ead79`（2026-10-24）。`git rev-list --count 9498c734..be4e1305` = **40**。README 的数据/结论对应旧快照，不能用来描述 head 的默认 feature、字体池/固定纹理研究或工作集生产接线。

**冲突三：AGENTS.md 第 541 行与第 547 行自相矛盾。**

第 541 行原文（生产通用池已接入）：

> 生产通用RGBA字体池已接入并本地提交2b57559dd7cb97479c1f4c8b019ca03f1788c796（接入通用字体图集池，main未push）：新增独立font_atlas_pool feature且normal default启用，不依赖chartPrecompile/画质；PGR/default主入口加载warm，自定义字体normalize后warm，Bold不额外64MiB预热。text保持glyph代表/process/布局/UV/透明顺序，完整RGBA shadow/policy0四slot零超时，缺cap/预算无shadow退原mutable，runtime保全current并全shadow恢复后释放mirror且以后mutable，validfalse仅一次新texture恢复/失败保留CPU数据并停字体draw，foreigncontext不删旧native同号对象。研究TLS only旧路径oracle/0auto1missing2busy3firstpubfallback，negativechannel/defer/coalesce不进入生产，ctx同号EGLloss框架未证。

第 547 行原文（仍只研究入口）：

> 新的字体整体策略同SO验证已完成（86395 terminal0，default-font-pool-v1-summary.json）：固定旧SO7f932/46c972/aggressivefalse，普通Chart/AutoPlay/linecompileron，font0旧mutable与font1 RGBA pool的差只有mode/rename/compact整体策略；不是各细项因果。LowABBA FPS55.806/59.690/59.683/56.266，font0/1均56.0356→59.6864；appwall8.63315→7.64735ms减少11.42%，>30两轮累计126→1，max70.4→30.659375（那一帧前app5.133、glyph0，不归因GPU/driver）。普通画质sample2/CPUlinefallbackABBA48.347/59.253/59.202/48.520 FPS，均48.4333→59.2275；app11.42990→9.74472ms减少14.74%，>30累计167→0，max68.2479→24.3010。两组quality各3686400pixels逐RGBA diff0、raw/request/meta/host/SF/PNG hash原样。实际1200样本时长随FPS改变约20–25秒，不应称每轮固定20秒；共同chartTime区间附加分层仍有趋势但78–82s low两者都约59.7，不外推全曲/所有GPU。这次有真实FPS/30ms改善；此前defaultFont新baseline40.42/58.17与当前off48.43/56.04分属不同窗口，不混池。尚只研究入口，没有生产默认接入，正式用户现在仍旧路径。

矛盾点：541 说“生产通用RGBA字体池已接入、normal default 启用、PGR/default 主入口 warm”；547 说“尚只研究入口，没有生产默认接入，正式用户现在仍旧路径”。按提交时间，547 描述的是 `2b57559` 之前的 `default-font-pool-v1` 研究策略（旧 SO 7f932/46c972，font0/font1 整体策略），541 描述的是 `2b57559` 的生产通用池（SO 2bc2744，`font_production_pool` bool）。两者不是同一版本/同一消费者；547 是旧快照，541 是后续生产接入。保留原文是为了说明“不能把旧研究策略的结论当成新生产池的细项因果”，不是让两句话同时成立。

**冲突四：`phira-main` 未转发 `font_atlas_pool`。**

`phira-main/Cargo.toml` 原文：

```toml
[features]
default = ["video", "chart_pre_compile"]
video = ["phira/video"]
chart_pre_compile = ["phira/chart_pre_compile"]

[dependencies]
phira = { workspace = true, default-features = false }
```

所以：

- Android Gradle 用 `-p phira`，`phira` 默认 feature 含 `font_atlas_pool`，普通 APK 编入；
- 桌面 `phira-main` 关闭 `phira` 默认 feature，只转发 `video`/`chart_pre_compile`，**默认不编入字体池**；
- 如果桌面也要池化，必须显式加 `font_atlas_pool = ["phira/font_atlas_pool"]` 并纳入 default 或构建参数。本文不能笼统写“所有普通构建都默认启用字体池”。

#### 10.4 未验证项

- 全曲 162.7 s、完整 loop、真实 Main 首次 Playing 前 5 s 之外的长窗口。
- 其它 GPU/驱动；只有 Mali-T880。
- EGL loss/context 重建；同一 managed context 内 native surface 丢失未证。
- OOM/RSS/物理 GPU 内存；128 MiB 只是名义容量。
- 多轮 Main UI、导入/暂停/重试/退出重进、动态字体/自定义字体变化。
- 真实 UI clear；字体池不参与磁盘 cache，但内存生命周期仍需多轮验证。
- 双通道/immutable/PBO/coalesce 等研究扩展的生产化；它们不属于本生产池。
- 桌面默认构建的字体池行为（当前 `phira-main` 未编入）。

### 11. 落地步骤（可执行清单）

- [ ] 明确 `phira-main` 是否要转发 `font_atlas_pool`；如果要，补 feature + default，并重新跑桌面检查；如果不要，文档明确“桌面默认不池化”。
- [ ] 在真实 Main UI 中完成：启动 → 主页 → Library → 导入谱面 → 游玩 → 暂停/返回 → 再次进入 → 切换自定义字体；记录 PGR/default warm、mirror、reuse、fallback。
- [ ] 跑完整 162.7 s 全曲 low/normal 各 OFF/ON/ON/OFF；保留 120 预热 + 全部真实帧，报告 FPS/app/p99/max/>30/SF。
- [ ] 在至少两种 GPU 上重复 9588 质量（3,686,400 像素 RGBA0）与 1800 帧性能；不能把 Mali-T880 外推。
- [ ] 测真实 RSS/物理 GPU 内存，确认 128 MiB 名义容量与 32 MiB mirror 的实际代价；如果静态 default 复用太低，考虑只 warm 动态字体/PGR。
- [ ] 验证 EGL loss/OOM：失败时 `valid=false`、停字体 draw、保留 CPU shadow、只恢复一次；不能无限重试。
- [ ] 验证 4 slot 全 busy、forced missing、预算边界、shape/resize、context 变化、双删保护。
- [ ] 验证真实 UI clear/多轮重入后字体池不泄漏、不误删、不双删。
- [ ] 明确研究 PBO/immutable 与生产池的边界；不要用固定纹理研究结果替代生产 RGBA 池。
- [ ] 更新 README/AGENTS 的默认值、版本、541/547 状态，避免继续传播“默认关闭/仅研究入口”的旧描述。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `be4e1305823bfebdad1853d7b333e0f79c3ead79` |
| 生产池提交 | `2b57559dd7cb97479c1f4c8b019ca03f1788c796`（接入通用字体图集池，2026-10-09 02:08:04 +0800） |
| 固定纹理研究提交 | `09b2370c`（接入固定纹理存储研究，2026-10-14 01:57:57 +0800）、`be4e1305`（保存固定纹理画质对照暂停进度，2026-10-24 11:07:06 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/ui/font_atlas_pool.rs`（1101 行）、`prpr/src/ui/atlas_delta.rs`（143 行）、`prpr/src/ui/text.rs`（1609 行）、`phira/src/lib.rs`、`prpr/Cargo.toml`、`phira/Cargo.toml`、`phira-main/Cargo.toml` |
| 测量数据 | `production-font-retry-v1-summary.json`（SO 2bc2744，44 源，commit 2b57559）；`default-font-pool-v1-summary.json`（旧 SO 7f932/46c972）；`production-font-v1-quality-audit.json` |
| 质量 | 质量会话 9588 / 14 jobs / 10 pairs / 3,686,400 px RGBA0；default-font-pool 两组 3,686,400 px RGBA0 |
| 设备 | EDI-AL10 / Mali-T880 / Android 8 / 2560×1440 |
| 谱面 | `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip` |
| 说明 | 正文已含关键代码、表格、冲突原文与结论；上述路径只用于追溯 |

---

## 第二部分：P-04 至 P-07

本文档只记录四条已经在参考实现上实际落地、并有同 SO 对照或严格输出对照的“正优化”。四条都不是“关掉效果”换帧率：P-04 省掉的是最终颜色 alpha 为 0、对 framebuffer 恒等的普通 CPU 判定线几何；P-05 用保守支持域 scissor 避免不可见 block fragment 进入 shader；P-06 只上传图集真正变化的 32×32 tile；P-07 把 JSON 解码后的谱面模型转换切成可让出一帧的片段。四条的收益边界差异很大，后文逐条写明“整帧 / 局部 / 条件性”。

阅读约定：

- “应用墙钟”指 `update + render + flush` 的 CPU/driver 墙钟，包含驱动执行、调度与等待，不包含 swap，也不等于 GPU 独占耗时。
- “呈现间隔”指 SurfaceFlinger 的真实呈现时间戳间隔，和“应用墙钟”不是同一个口径。
- “同 SO”指同一份 `.so`；ABBA 指同一轮内按 A-B-B-A 交错，避免把时间漂移当成优化收益。
- P-04 / P-06 的设备是 `8DF6R17309002674`：EDI-AL10 / hi3650 / Android 8 / arm64-v8a，Mali-T880 / OpenGL ES 3.2 v1.r20p0，画面 2560×1440（3,686,400 像素）。
- P-05 / P-07 的设备是 MT6769V/CU / Android 10 / Mali-G52 MC2 / OpenGL ES 3.2，画面 1920×1200（2,304,000 像素）。
- 本文只描述参考实现事实；“参考实现已落地”不等于“已进 main”，每条第 9 节单独说明。
- 所有代码、表格、结论都在正文内；第 13 节的路径只用于追溯，不是阅读前提。

---

## P-04 透明判定线 DCE

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-09　参考实现提交：`657e40f1`（研究透明判定线）、`689df63d`（启用透明线优化）　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计，不冒充独立验收）

### 1. 一句话结论

P-04 只消除“普通 CPU 判定线、最终 RGBA8 alpha 为 0、默认材质、无深度、无 capture、有限仿射矩阵”这一种 framebuffer 恒等操作：普通画质（`sampleCount=2`）下应用墙钟从 **9.65279 ms 降到 8.92722 ms（−7.52%）**，FPS 从 **57.92575 升到 59.75533（+3.16%）**；低画质（`sampleCount=1`）没有收益（7.30410→7.37333 ms，DCE 计数为 0），因为低画质的普通线已经走 GPU 常驻线批次，CPU 路径没有透明线可省。它是**普通画质下的整帧正优化**（机制上只影响 CPU 判定线提交段）；低画质无收益。在满足守卫时对 framebuffer 是无损恒等，但不是“所有 alpha=0 图元都可以删”的通用透明裁剪。

### 2. 问题与瓶颈（为什么原路径慢）

原路径对每条普通判定线执行：

```rust
draw_line(-len, 0., len, 0., if line_scaled { 0.0076 } else { 0.01 }, color);
```

`draw_line` 每帧都要：

1. 根据 `len` 与线宽重新计算四边形的四个角；
2. 把 `Color` 转成 `Vertex` 的 RGBA8；
3. 把四边形写入 macroquad 的动态顶点缓冲并生成 6 个索引；
4. 在 `quad_gl` 队列里做一次状态提交与绘制；
5. 对每条线重复矩阵、颜色和批次判断。

这条谱面只有 24 条线，但判定线每帧都画；普通画质下 `chart_pre_compile` 的 GPU 线消费者要求低画质/单采样，普通画质不满足，所以 24 条线仍走 CPU `draw_line`。大量线在淡出阶段 `color.a` 已经量化成 0，仍然会生成几何、写顶点、提交绘制——最终混合结果却与“什么都不画”完全一样。质量作业在 240 帧内记录到 **6805 次候选**，说明这不是理论上的稀有路径。

瓶颈不是“几何复杂”，而是**每帧重复的 CPU 顶点生成、动态上传与逐线提交**。P-04 的目标只是把这部分已经证明无输出的提交省掉，不动判定、pose、音符、音频和绘制顺序。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

#### 3.1 恒等混合的数学条件

原路径使用默认材质，混合为 `SourceAlpha / OneMinusSourceAlpha`（源码和 P-01 的线路径一致）。设源颜色 `(Rs, Gs, Bs, As)`、目标 `(Rd, Gd, Bd, Ad)`，当 `As = 0`：

```text
RGB' = Rs*0 + Rd*(1-0) = Rd
A'   = As*As + Ad*(1-As) = Ad
```

RGB 与 alpha 都是恒等。颜色写入 `Vertex` 时已经做 `(v * 255.) as u8`；因此“最终 alpha 字节为 0”比“浮点 alpha 等于 0”更准确，也覆盖了 `0 < a < 1/255` 被量化成 0 的情况。P-04 用：

```rust
if (color.a * 255.) as u8 != 0 { return false; }
```

这与原 `Vertex::new` 的饱和转换一致，`-0.0`、`0.0`、`-0.5`、`0.5/255.` 都会得到 0；`1/255.` 的相邻 lower float 得到 0，边界 `1/255.` 得到 1。这个边界有独立单元测试。

#### 3.2 守卫条件

只有同时满足以下条件才返回 `true`（跳过原 `draw_line`）：

1. `options.enabled` 为真，或处于 audit 模式；
2. 颜色 alpha 的 RGBA8 字节为 0；
3. 非 `chart_debug` / `chart_debug_line`；
4. `color.r/g/b/a` 全部有限；
5. 几何落在原 `draw_line` 的非退化域：`len` 有限、`0 < len <= 64`、`tlen` 有限且不小于 `f32::EPSILON`、`dx/tlen` 有限；
6. 当前是默认材质（`is_using_default_material()`）；
7. 深度测试关闭（`is_depth_test_enabled()` 为 false）；
8. geometry capture 关闭（`is_geometry_capture_enabled()` 为 false）；
9. Model 与 Projection 都是有限值、绝对值不超过 4096、最后一行为 `[0,0,0,1]` 的仿射矩阵（索引 3/7/11 为 0，索引 15 为 1）。

第 9 条是一个保守域，不是替代原矩阵计算。`|matrix|<=4096`、`|local position|<=64`、`w=1` 时，GLSL 两个乘积远低于 f32 溢出，裁剪/插值不会产生无穷 w；未知或溢出的几何回原路径。

#### 3.3 跳过时仍保留的逻辑状态

`draw_line` 在写几何之前会设置 `texture(None)` 与 `draw_mode(Triangles)`。P-04 在返回 `true` 之前显式执行同样的两次逻辑状态设置：

```rust
gl.quad_gl.texture(None);
gl.quad_gl.draw_mode(DrawMode::Triangles);
```

这样即使后续调用方读取逻辑状态，看到的也和原路径一致。跳过的只有“几何 + 绘制提交”，不是整个 `draw_line` 的语义。

#### 3.4 不变量与画质为何不变

- **混合恒等**：alpha 字节为 0 时，源项对 RGB 和 alpha 的贡献都是 0；默认材质的混合方程是固定的。
- **无深度写入**：深度测试关闭，不会留下深度值。
- **无 capture**：capture 关闭，不会漏掉 telemetry 需要的图元。
- **无自定义 shader**：默认材质，不存在“alpha=0 但 shader 仍写颜色”的自定义路径。
- **顺序不变**：只对当前这一条线提前返回；前后线的相对顺序、模型 scope、pose 读取、判定逻辑都不变。
- **GPU 线批次优先**：调用点先执行 `chart_pre_compile::lines::try_render`，命中 GPU 线批次就直接返回；P-04 只处理 GPU 路径未接管的 CPU 普通线。

因此，在守卫成立时 framebuffer 的 RGBA8 结果与原路径逐字节相同。P-04 不声称“任意 alpha=0 的纹理/音符/自定义材质都可删”；它只覆盖普通 CPU 判定线这一种受控路径。

### 4. 具体实现

#### 4.1 文件与符号清单（行数、函数/结构）

| 文件（参考实现，head `be4e1305`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/render_elision.rs` | 208 | `Options`（8-19）、`Stats`（21-27）、`configure_guard_material`（36-51）、`GuardMaterial`（53-69）、`configure`（72-75）、`options`（76-85）、`report`（87-95）、`audit`（96-113）、`geometry_valid`（117-125）、`matrix_valid`（129-133）、`try_skip_cpu_line`（135-167）、3 个测试（169-208） | 判定与跳过；研究统计/保护材质 |
| `prpr/src/core/line.rs` | 736 | `JudgeLine::render_inner`（约 405-420） | 调用点：GPU 线批次之后、`draw_line` 之前 |
| `prpr/src/lib.rs` | — | `pub mod render_elision;`（12） | 模块无条件编入 |
| `prpr/src/ui/atlas_pool.rs` 等 | — | 无 | P-04 不涉及图集 |
| `prpr-macroquad` `src/quad_gl.rs`（commit `de34611b`） | — | `is_depth_test_enabled`（1003-1005）、`is_using_default_material`（1007-1012）、`is_geometry_capture_enabled`（1014-1018） | 只读逻辑状态 getter，不 flush、不查 GL、不改队列 |
| `phira/src/render_research.rs` | — | `line_elision` / `line_elision_audit` / `line_elision_guard_material`（295、371-373、823） | 研究请求配置、报告、保护材质；不是生产开关 |

`prpr/src/render_elision.rs` 的 208 行是参考实现当前的真实行数；`Options::default()` 在 13-19 行把 `enabled` 设为 `true`，`689df63d` 只改了这个默认值和注释，没有加 Config/UI。

#### 4.2 数据结构与算法（逐步说明）

1. **`Options`**：`{ enabled: bool, audit: bool }`。生产构建没有 `render_research` 时，`options()` 直接返回 `Options::default()`；研究构建则读 thread-local `OPTIONS`。
2. **`Stats`**：仅 `render_research` 下存在，四个计数：`candidates`、`skipped`、`blocked_state`、`blocked_domain`。`audit` 为 false 时计数器不更新，避免生产热路径原子/线程局部写开销。
3. **`try_skip_cpu_line` 入口**：
   - 先读 options；`!enabled && !audit` 直接 false；
   - alpha 字节非 0 直接 false（这是最常见早退）；
   - 记 candidate；
   - debug / 非有限颜色 / 几何域失败 → `blocked_domain`；
   - 默认材质/深度/capture 失败 → `blocked_state`；
   - 矩阵域失败 → `blocked_domain`；
   - `!enabled`（audit-only）→ false；
   - 设置 `texture(None)`、`draw_mode(Triangles)`，记 skipped，返回 true。
4. **`geometry_valid`**：复刻 `draw_line` 的非退化域。`length` 必须在 `(0, 64]`；`dx = length - -length = 2*length`；`thickness` 为 0.0076 或 0.01；`tlen = sqrt(dx*dx)/(thickness*0.5)`；要求 `tlen` 有限且 ≥ `f32::EPSILON`，且 `dx/tlen` 有限。它不重新计算最终矩阵，只判断“原路径不会提前返回/不会产生非有限值”。
5. **`matrix_valid`**：16 个元素全部有限且 `|v| <= 4096`；列主序下索引 3/7/11 为 0（仿射最后一列），索引 15 为 1。透视/裁剪/非仿射一律回退。
6. **保护材质（仅研究）**：`configure_guard_material(true)` 编译一个忽略 vertex alpha 的红色 fragment shader；`guard_material()` 在 quality 作业里临时 `gl_use_material`，`Drop` 恢复默认材质。它用于证明“P-04 真的没有把红线一起跳过”，不是生产路径。
7. **测试**：
   - `identity_blend_preserves_every_rgba8_destination`：256×256 组源/目标字节，验证 `src*0 + dst*(1-0) == dst`；
   - `alpha_byte_boundary_uses_original_vertex_conversion`：验证 `(a*255.) as u8` 的 0/1 边界和 `-0.0`；
   - `domain_rejects_early_return_perspective_and_nonfinite_inputs`：验证 `len=0/-1/MIN_POSITIVE/NaN/INF/65` 拒绝，单位阵接受，透视/非有限/超界拒绝。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

以下代码直接取自参考实现的 `be4e1305`。`render_elision.rs` 全文 208 行，未删行。

**`prpr/src/render_elision.rs` 全文：**

```rust
//! Elide only the ordinary CPU line's framebuffer identity operation.
//! Resident line batches keep their contiguous index ranges unchanged.
use macroquad::{prelude::*, window::get_internal_gl};

#[cfg(feature = "render_research")]
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy)]
struct Options {
    enabled: bool,
    audit: bool,
}
impl Default for Options {
    fn default() -> Self {
        // Lossless managed-default-material path, verified with both CPU
        // qualities. Research run_job explicitly selects its own control flag.
        Self { enabled: true, audit: false }
    }
}
#[cfg(feature = "render_research")]
#[derive(Clone, Copy, Default)]
struct Stats {
    candidates: u64,
    skipped: u64,
    blocked_state: u64,
    blocked_domain: u64,
}
#[cfg(feature = "render_research")]
thread_local! {
    static OPTIONS: Cell<Options> = Cell::new(Options::default());
    static STATS: Cell<Stats> = Cell::new(Stats::default());
    static GUARD_MATERIAL: RefCell<Option<macroquad::material::Material>> = const { RefCell::new(None) };
}

#[cfg(feature = "render_research")]
pub fn configure_guard_material(enabled: bool) -> anyhow::Result<()> {
    use macroquad::material::{load_material, MaterialParams};
    let material = if enabled {
        // The fragment deliberately ignores vertex alpha. This must remain
        // visible even for a zero-alpha ordinary line, in both control/opt modes.
        Some(load_material(
            "#version 100\nattribute vec3 position; uniform mat4 Model; uniform mat4 Projection; void main(){gl_Position=Projection*Model*vec4(position,1.);}",
            "#version 100\nprecision mediump float; void main(){gl_FragColor=vec4(1.,0.,0.,1.);}",
            MaterialParams::default(),
        )?)
    } else {
        None
    };
    GUARD_MATERIAL.with(|slot| *slot.borrow_mut() = material);
    Ok(())
}
#[cfg(feature = "render_research")]
pub struct GuardMaterial;
#[cfg(feature = "render_research")]
impl Drop for GuardMaterial {
    fn drop(&mut self) {
        macroquad::material::gl_use_default_material();
    }
}
#[cfg(feature = "render_research")]
pub fn guard_material() -> Option<GuardMaterial> {
    GUARD_MATERIAL.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref().map(|material| {
            macroquad::material::gl_use_material(*material);
            GuardMaterial
        })
    })
}

#[cfg(feature = "render_research")]
pub fn configure(enabled: bool, audit: bool) {
    OPTIONS.with(|slot| slot.set(Options { enabled, audit }));
    STATS.with(|slot| slot.set(Stats::default()));
}
fn options() -> Options {
    #[cfg(feature = "render_research")]
    {
        OPTIONS.with(Cell::get)
    }
    #[cfg(not(feature = "render_research"))]
    {
        Options::default()
    }
}
#[cfg(feature = "render_research")]
pub fn report() -> serde_json::Value {
    let options = options();
    let stats = STATS.with(Cell::get);
    serde_json::json!({"enabled":options.enabled,"audit":options.audit,
        "guard_material":GUARD_MATERIAL.with(|slot| slot.borrow().is_some()),
        "candidates":stats.candidates,"skipped":stats.skipped,
        "blocked_state":stats.blocked_state,"blocked_domain":stats.blocked_domain,
        "scope":"CPU ordinary line geometry only; final original RGBA8 alpha zero, default material, no depth/capture, bounded affine domain. Resident batches, note/pose evaluation and ordering unchanged. Counters disabled without audit."})
}
#[inline]
fn audit(options: Options, event: u8) {
    #[cfg(feature = "render_research")]
    if options.audit {
        STATS.with(|slot| {
            let mut stats = slot.get();
            match event {
                0 => stats.candidates += 1,
                1 => stats.skipped += 1,
                2 => stats.blocked_state += 1,
                _ => stats.blocked_domain += 1,
            }
            slot.set(stats);
        });
    }
    #[cfg(not(feature = "render_research"))]
    let _ = (options, event);
}

// Keep the original non-degenerate draw_line domain. It returns before setting
// texture/mode when tlen<EPSILON; unknown/overflow geometry uses that old path.
fn geometry_valid(length: f32, thin: bool) -> bool {
    if !(length.is_finite() && length > 0. && length <= 64.) {
        return false;
    }
    let dx = length - -length;
    let thickness = if thin { 0.0076 } else { 0.01 };
    let tlen = (dx * dx).sqrt() / (thickness * 0.5);
    tlen.is_finite() && tlen >= f32::EPSILON && (dx / tlen).is_finite()
}
// This is a conservative domain, not a replacement matrix computation. With
// |matrix|<=4096, |local position|<=64 and affine w=1, both GLSL products remain
// far below f32 overflow; clipping/interpolation cannot acquire an infinite w.
fn matrix_valid(matrix: &[f32; 16]) -> bool {
    matrix.iter().all(|value| value.is_finite() && value.abs() <= 4096.)
        && [3, 7, 11].into_iter().all(|index| matrix[index] == 0.)
        && matrix[15] == 1.
}

pub fn try_skip_cpu_line(length: f32, thin: bool, color: Color, debug: bool) -> bool {
    let options = options();
    if !options.enabled && !options.audit {
        return false;
    }
    // Match Vertex::new, including the saturating float-to-u8 conversion.
    if (color.a * 255.) as u8 != 0 {
        return false;
    }
    audit(options, 0);
    if debug || ![color.r, color.g, color.b, color.a].iter().all(|value| value.is_finite()) || !geometry_valid(length, thin) {
        audit(options, 3);
        return false;
    }
    let gl = unsafe { get_internal_gl() };
    if !gl.quad_gl.is_using_default_material() || gl.quad_gl.is_depth_test_enabled() || gl.quad_gl.is_geometry_capture_enabled() {
        audit(options, 2);
        return false;
    }
    if !matrix_valid(&gl.quad_gl.get_model_matrix().to_cols_array()) || !matrix_valid(&gl.quad_gl.get_projection_matrix().to_cols_array()) {
        audit(options, 3);
        return false;
    }
    if !options.enabled {
        return false;
    }
    // draw_line sets these before recording geometry. Keep its logical state,
    // model scopes and caller-side reads even though the color operation is 0.
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    audit(options, 1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_blend_preserves_every_rgba8_destination() {
        // Independent float blend oracle for every source byte and destination
        // byte; alpha=0 makes both RGB and inherited-alpha blending identity.
        for source in 0..=255 {
            for destination in 0..=255 {
                let source = source as f64 / 255.;
                let destination = destination as f64 / 255.;
                assert_eq!(source * 0. + destination * (1. - 0.), destination);
            }
        }
    }
    #[test]
    fn alpha_byte_boundary_uses_original_vertex_conversion() {
        let edge = 1. / 255.0_f32;
        let below = f32::from_bits(edge.to_bits() - 1);
        assert_eq!((below * 255.) as u8, 0);
        assert_eq!((edge * 255.) as u8, 1);
        for alpha in [-0., 0., -0.5, 0.5 / 255.] {
            assert_eq!((alpha * 255.) as u8, 0);
        }
    }
    #[test]
    fn domain_rejects_early_return_perspective_and_nonfinite_inputs() {
        for length in [0., -1., f32::MIN_POSITIVE, f32::NAN, f32::INFINITY, 65.] {
            assert!(!geometry_valid(length, false));
        }
        assert!(geometry_valid(1., false) && geometry_valid(1., true));
        let identity = Mat4::IDENTITY.to_cols_array();
        assert!(matrix_valid(&identity));
        for (index, value) in [(3, 0.001), (15, 0.), (0, f32::NAN), (0, f32::INFINITY), (0, 4097.)] {
            let mut matrix = identity;
            matrix[index] = value;
            assert!(!matrix_valid(&matrix));
        }
    }
}
```

**调用点 `prpr/src/core/line.rs:405-421`：**

```rust
                res.apply_model(|res| match &self.kind {
                    JudgeLineKind::Normal => {
                        let mut color = color.unwrap_or(res.judge_line_color);
                        // 判定线调试：本该淡出的线抬到至少 0.15 保留淡影。
                        color.a = parse_alpha(color.a, alpha.max(0.0), 0.15, res.config.chart_debug_line);
                        let len = res.info.line_length;
                        #[cfg(feature = "render_research")]
                        let _guard_material = crate::render_elision::guard_material();
                        #[cfg(feature = "chart_pre_compile")]
                        if crate::render_compiler::lines::try_render(chart, id, len, line_scaled, color, res) {
                            return;
                        }
                        if crate::render_elision::try_skip_cpu_line(len, line_scaled, color, res.config.chart_debug || res.config.chart_debug_line) {
                            return;
                        }
                        draw_line(-len, 0., len, 0., if line_scaled { 0.0076 } else { 0.01 }, color);
                    }
```

**只读 getter（`prpr-macroquad` `de34611b:src/quad_gl.rs:1003-1018`）：**

```rust
    pub fn is_depth_test_enabled(&self) -> bool {
        self.state.depth_test_enable
    }

    /// Whether managed geometry uses the built-in material. Depth state remains
    /// independent; this only reads the queued logical pipeline selection.
    /// It does not query GL, flush, or mutate queued commands.
    pub fn is_using_default_material(&self) -> bool {
        self.state.pipeline.is_none()
    }

    /// Whether subsequent geometry is marked for telemetry capture.
    /// Reading this flag does not flush or mutate queued commands.
    pub fn is_geometry_capture_enabled(&self) -> bool {
        self.state.capture
    }
```

#### 4.4 调用链与生命周期

调用链：

```text
GameScene::render
  -> Chart::render / JudgeLine::render
    -> JudgeLine::render_inner
      -> res.apply_model(line_matrix)
        -> res.apply_model(scale_matrix)
          -> JudgeLineKind::Normal
            -> [chart_pre_compile] render_compiler::lines::try_render(chart, id, len, line_scaled, color, res)
                命中 GPU 常驻线 -> return
            -> render_elision::try_skip_cpu_line(len, thin, color, debug)
                命中 -> texture(None) + draw_mode(Triangles) -> return
            -> draw_line(-len, 0., len, 0., width, color)
```

生命周期：

- 无 GPU 资源、无缓存、无后台线程、无 `Drop` 清理；模块只读 thread-local options/stats。
- `Options` 是 `Copy`，每个线程一份；生产构建每次调用返回默认值，没有初始化/关闭流程。
- `Stats`/`OPTIONS`/`GUARD_MATERIAL` 只在 `render_research` 下存在；研究作业通过 `configure`/`configure_guard_material` 设置，作业结束线程退出即回收。
- 跳过发生在 `draw_line` 之前，不影响 macroquad 队列中已有的命令；`texture(None)`/`draw_mode` 只是逻辑状态，不产生 GL 调用。
- 因为不持有资源，P-04 没有 EGL/context 生命周期问题；风险集中在“判断错杀”和默认开启。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| 模块编入 | `prpr/src/lib.rs:12` 无条件 `pub mod render_elision;` | 没有 feature 条件编译；生产构建也编入 |
| 跳过默认 | `Options::default().enabled = true` | **默认启用**，不是默认关闭 |
| 用户开关 | 无 | 没有 `Config` 字段、Settings 项、环境变量；要关只能改源码或研究请求 |
| 研究关闭 | `render_research` 下 `configure(false, audit)` | 只有研究作业能显式选 control；audit 可单独开 |
| 研究审计 | `configure(_, true)` + `report()` | 计数只在 audit 时累加；生产没有计数 |
| 保护材质 | `configure_guard_material(true)`，仅 `render_research` | 固定时钟 quality fixture；不是生产 |
| 画质权限 | 无显式画质开关；靠“低画质 GPU 线批次优先”自然分流 | 普通画质走 CPU 线，低画质走 GPU 线 |
| Cargo feature | 无 P-04 专属 feature | `render_research` 只控制统计/配置/保护材质 |
| 默认是否实际生效 | **是（源码默认，参考实现）** | 该参考实现的非 `render_research` 构建中，只要走到普通 CPU 线且守卫成立，就跳过；main 无此文件 |

这里必须区分“研究请求的 control”与“生产默认”：`phira/src/render_research.rs` 的 `line_elision` 是研究作业字段，默认 `false` 只影响研究作业；生产构建没有这个字段，`Options::default()` 仍是 `true`。因此不能写“默认关闭”。

### 6. 测量方法与完整数据（完整表格）

#### 6.1 测量口径

- 设备：EDI-AL10 / Mali-T880 / Android 8 / arm64-v8a / 2560×1440。
- 谱面：`f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip`，24 线、1349 普通非 hold、219 holds。
- 采样：每轮 120 帧预热 + 1800 帧真实时钟采样；应用墙钟只统计 1800 帧。
- 画质：`normal` = `sampleCount=2`；`low` = `sampleCount=1`。两组独立 cohort，不混池。
- 同 SO：`line-elision-lab.so` SHA-256 `aeebddc0a70f58970e88b15b0fd8e99d6273ac37c4be22759824fe748acef6c1`；源提交 `657e40f13033afd09622321dfb664f41c95935d7`，45 源冻结核对。
- 交错：low-off / low-on / low-on / low-off；normal-off / normal-on / normal-on / normal-off。只切换 `line_elision`。
- 其它：`chart_pre_compile=true`、`particle=true`、`disableEffect=false`、`blockAreaSimple=false`、`shaderPreRender=false`、autoplay、offline。
- 应用墙钟是 `update+render+flush`，包含驱动/调度/等待；不是 GPU elapsed。

#### 6.2 八轮完整数据

| 轮次 | 画质 | 路径 | 应用墙钟均值 ms | FPS | 帧间隔 p50 ms | 帧间隔 p99 ms | 帧间隔 max ms |
|---:|---|---|---:|---:|---:|---:|---:|
| 0 | low | off | 7.312104 | 59.747692 | 16.717709 | 19.890823 | 26.674479 |
| 1 | low | on | 7.311617 | 59.745153 | 16.743750 | 19.393771 | 24.300000 |
| 2 | low | on | 7.435039 | 59.730682 | 16.750000 | 19.059646 | 26.885937 |
| 3 | low | off | 7.296100 | 59.765700 | 16.745833 | 19.089052 | 27.074479 |
| 4 | normal | off | 9.605854 | 57.968627 | 16.842188 | 20.771218 | 27.857812 |
| 5 | normal | on | 8.888230 | 59.752806 | 16.735937 | 19.255032 | 26.523959 |
| 6 | normal | on | 8.966212 | 59.757860 | 16.731771 | 19.090698 | 22.504687 |
| 7 | normal | off | 9.699733 | 57.882869 | 16.879687 | 21.412178 | 29.015625 |

组均值：

| 画质 | off 均值 ms | on 均值 ms | 差值 ms | 比例 | off FPS | on FPS | FPS 变化 |
|---|---:|---:|---:|---:|---:|---:|---:|
| normal | 9.652793 | 8.927221 | −0.725573 | **−7.5167%** | 57.925748 | 59.755333 | **+3.1585%** |
| low | 7.304102 | 7.373328 | +0.069226 | +0.9478% | 59.756696 | 59.737917 | −0.0314% |

解读：

- normal 两轮 on 都低于两轮 off（8.888/8.966 vs 9.606/9.700），组差大于前后漂移；p99 从 20.77/21.41 降到 19.26/19.09，max 从 27.86/29.02 降到 26.52/22.50。
- FPS 从 57.93 升到 59.76，接近 60 上限；这不是“帧率提高 3.16% 的整帧优化”，而是墙钟下降后较少掉出 60Hz 节奏。
- low 的 on 两轮均值反而高 0.069 ms（+0.95%），FPS 低 0.03%；DCE 计数为 0，说明没有实际跳过任何线。这个差异在漂移范围内，不能称“低画质负优化”，但**低画质无收益**是确定事实。

#### 6.3 质量计数（证明 DCE 真的执行）

| 作业 | 画质 | 候选 | 跳过 | blocked_state | blocked_domain | 结果 |
|---|---|---:|---:|---:|---:|---|
| low-off | low | 0 | 0 | 0 | 0 | GPU 线批次接管，无 CPU 透明线 |
| low-on | low | 0 | 0 | 0 | 0 | 同上 |
| normal-off | normal | 6805 | 0 | 0 | 0 | 对照，不跳过 |
| normal-on | normal | 6805 | **6805** | 0 | 0 | 全部候选均跳过 |
| normal-custom-material-off | normal | 6805 | 0 | 6805 | 0 | 红色保护材质，全部 blocked |
| normal-custom-material-on | normal | 6805 | 0 | 6805 | 0 | 红色保护材质，全部 blocked |

`normal-custom-material` 组的两张图各有 **217238** 个红色像素，且最小要求 64；两图 RGBA 逐像素相同。这证明“保护材质画出的红线”没有被 DCE 误杀，也证明 `blocked_state` 分支实际执行。

#### 6.4 质量对照

| 组别 | 作业数 | 有效 RGBA 对照 | 覆盖 | 结果 |
|---|---:|---:|---|---|
| `line-elision-v1-quality` | 6 | 3 组 | low off/on、normal off/on、normal custom-material off/on | 每组 2560×1440、3,686,400 px，`changed_pixels=0`、`max_rgba_difference=0` |
| `line-elision-cpu-v1-quality` | 2 | 1 组 | 关闭 `chartPreCompile` 的低画质 CPU 路径，off/on | 3,686,400 px，`changed_pixels=0`、`max_rgba_difference=0`；off/on 各 6805 candidates，on 6805 skipped |

`line-elision-cpu-v1-quality` 的意义是把“GPU 线批次优先”这个混淆因素去掉：`chartPreCompile=false` 时普通线仍走 CPU，DCE 有真实候选；结果仍逐像素相同。它不是性能 cohort，不能拿它的固定帧计时当收益。

#### 6.5 `本地证据/AGENTS.md` 对应段落

历史引文中的分支标识已归一化为“参考实现”，其余原文未改。以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

第 523 行原文：

> 普通透明线DCE已获得有限普通画质收益并接入默认无损路径。本地候选提交657e40f13033afd09622321dfb664f41c95935d7/SO aeebddc0a70f58970e88b15b0fd8e99d6273ac37c4be22759824fe748acef6c1，45源冻结核对；6个quality作业3RGBA对照、另2个关闭chartPrecompile的低画质CPU作业1RGBA对照，每组3686400px逐RGBA0。普通/低CPU各实际skip6805次（含120warm+240sample），低GPU仍8640 accepted/360residentdraw/6805透明记录且skip0，连续索引未拆。fixed红shader忽略vertexAlpha，217238红pixels两图一致、blocked6805/skip0，保护实际执行。质量11416/CPU4263均terminal0；初版两API编译错误与Windows/NDK neverexecuted失败log保留，修正/重试最终通过。

第 527 行原文：

> 默认接入已另本地提交689df63df8fffa8c6bd2e2509cec350b6beefeee（启用透明线优化，主项目未push），只把Options initial/default设true，研究run_job仍显式选control。默认/研究Windows检查、正常Android独立chart_pre_compile构建与final research构建实际通过并逐45源核commit：正常SO2d125d071e4b0310aa500e8f81e412b01eec19022224340257fc85ccb7820a8c，final research2b27a0cb7ac87f7e0ab37427fd1c9f74b914b6a780f9c2037b341539da457019。后两SO未混进前候选ABBA或冒称已测相同收益；后续全曲必须显式line_elision=true，否则research run_job会关闭。限managed默认材质/noDepth/noCapture/有限颜色/有效非退化len与bounded affine w1，未知与debug回原，全部note/pose/Tween读取保持。macroquad461ad5f独立参考实现已push/主项目不动，miniquadeb0不改。当前tracked clean、旧device/build handles实际terminal，正在准备finalSO普通画质完整曲（新cohort不覆盖full-v1）；真实Main首次Playing前5s/多UI生命周期/清cache及完整音符GPU blob/其它GPU仍未完成，goal active，不扩warm/FBO。

第 529 行原文（候选阶段记录；其中“default所有build关闭”已被第 527 行的 `689df63d` 覆盖，最终以第 527 行为准）：

> 普通透明线DCE候选正在直接实现，尚未真机验证/不得称收益：macroquad独立参考实现已commit/push461ad5f7f8eeff218eb17ec7201c89ff4b73975e（查询绘制状态），只增logical default-material/capture两getter，check通过/库clean/主项目139ac954不动，miniquad仍eb0eda0；主项目已固定新完整rev及仅两Cargo.lock源替换，未改其它依赖。新增render_elision.rs仅CPU普通线在原try_render失败后省几何，最终原RGBA8 alpha0、有限颜色、原有效tlen、len(0,64]与|Model/Projection|<=4096 affine w1守卫，默认material/noDepth/noCapture，保留texture(None)/Triangles、全部getter/pose/模型scope/负alpha控制及notes；GPU常驻连续索引不拆，default所有build关闭。现有GPU消费者也补custom/capture回退。研究fixed-only红shader忽略vertexAlpha，测试必须blocked_state>0/skipped0/红线可见，不是通用material RAII（它恢复default）/任意rawGL缓存失配证明。3个CPU数学/边界/domain tests实际通过；初版Material引用和private DrawMode错误已改，失败log保留。Windowsdefault已Finished通过，研究检查受中断且没有Finished/进程已消失，原partial保留，正在用独立retry完成；Android/quality6jobs3pairs/同SO各画质ABBA尚未执行。主HEAD仍e593b30，tracked源码WIP、主项目不push；无设备作业在跑，整体goal active。

第 519 行原文（SIGABRT 风险，未定性）：

> 后续Main多次启动出现tokio-rt-worker SIGABRT；旧line-elision-final-lab同配置对照也出现，#02pc0x1217eec/offset0x856000均同，未见panicmessage，不据worker或字体相关日志认定原因。offlineMode不能完全隔离首页update/bold update tasks（home.rs182/188仍无条件创建）；字体worker只CPU FontArc，TextPainter创建在Main消费，不支持“worker创建GL”推断。正在仅本地构建main-timing-symbols（实际handle需续轮poll60685），先核ELF allocated sections同码再地址映射，不部署/改变生产profile；不能因观测超时重启。启动失败/错误autoplay不叫gameplay性能通过，当前无有效新autoplay数据，完整goalactive。设备研究包已崩溃/需实际PID核；新Main UI异常和host工具修正不替代普通渲染/音符GPUblob/多GPU/真实首段目标，不扩warm/FBO。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

- **恒等混合单元测试**：256×256 组源/目标字节全部满足 `src*0 + dst = dst`；alpha 字节边界与 `-0.0` 覆盖。
- **域单元测试**：`len=0/-1/MIN_POSITIVE/NaN/INF/65` 全部拒绝；单位阵接受；透视（索引 3 非 0）、索引 15 非 1、NaN/INF/4097 全部拒绝。
- **三组真机 RGBA**：low、normal、normal-custom-material 各一对，3,686,400 px，逐 RGBA 0 差异；normal-custom-material 的 217238 个红像素证明保护材质仍可见。
- **独立低画质 CPU 对照**：关闭 `chartPreCompile` 的 off/on，一组 RGBA 0 差异，且 on 实际 skip 6805。
- **计数一致性**：normal-on 6805 candidates / 6805 skipped；custom-material 6805 blocked_state / 0 skipped；low 0 candidates。计数与代码分支一致，不是“只跑通编译”。
- **45 源冻结/SO 核对**：候选 SO 与源提交 `657e40f1` 的 45 个源文件逐个 hash 核对；质量作业 `11416`、CPU 对照 `4263` 均 terminal 0。

#### 7.2 失败项与限制

- **低画质无收益**：low 组 0 candidates，P-04 在低画质没有实际工作可省；不能把 normal 的 −7.52% 外推到低画质。
- **只覆盖普通画质 chart 60 s 起的约 30 s 窗口**：性能是 start=60、1800 帧（约 30 s）真实时钟；质量是 240 帧固定时钟。没有全曲逐帧、没有手动输入、没有其他 GPU。
- **没有 GPU 计时**：应用墙钟下降不能直接归因于“GPU 少画了 6805 条线”；只能说明 CPU 提交/驱动等待的总和下降。
- **没有真实 UI 开关验收**：P-04 没有用户开关，无法用 Settings 点击验证；研究请求的 control 不等于生产开关。
- **没有 EGL loss 风险**：P-04 不持有 GPU 资源，理论上不受 context 丢失影响；但仍未在真实 surface 重建流程中跑过。
- **初版编译失败日志保留**：初版两个 API 编译错误、Windows/NDK “never executed” 失败日志都保留；后续修正/重试才通过。这些失败不是性能结论，但不能从最终通过反推“一次成功”。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：整帧（仅普通画质；机制上只影响 CPU 判定线提交段）。**

理由：

1. **收益方向一致**：normal 两轮 on 都低于两轮 off，均值 9.652793→8.927221 ms（−7.5167%）；FPS 57.925748→59.755333（+3.1585%）。不是单轮偶然。
2. **机制可解释**：normal-on 实际 skip 6805 条透明线；低画质 0 candidates 且无收益，正好形成“有跳过才有收益”的对照。
3. **画质有严格证据**：三组 3,686,400 px RGBA 0 差异；保护材质红线 217238 px 仍在；独立 CPU 路径 0 差异。
4. **为什么只限普通画质，而不是所有画质**：只影响普通画质的 CPU 判定线；低画质已经走 GPU 常驻线批次，DCE 计数为 0。普通画质 FPS 从 57.9 升到 59.75，接近 60 上限，这是用户可感知的整帧改善；但不能外推到低画质、其他 GPU 或该窗口之外。
5. **为什么是正而不是无**：normal 的墙钟、p99、max 同方向改善，且 skip 计数与收益同时出现；不是只减少计数。
6. **低画质为什么不算负**：low on 的 +0.95% 在漂移范围，且 0 candidates；不能把“没有收益”写成“负优化”，但也不能把 normal 收益外推到 low。

**置信度：**

- 对“这个设备、这个 SO、normal 画质、chart 60 s 起的 1800 帧窗口的应用墙钟下降”：**高**。
- 对“机制是透明线几何提交被省掉”：**高**（计数 + 低画质 0 候选 + CPU 独立对照）。
- 对“全曲/所有 GPU/低画质/该窗口之外的整帧 FPS”：**低**，不能外推。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`657e40f1` 新增 `render_elision.rs` 并接入 `line.rs`；`689df63d` 把 `Options::default().enabled` 设为 `true` |
| 参考实现是否默认启用 | **是（源码默认）**。该参考实现的非 `render_research` 构建中，`options()` 返回 `Options::default()`，`enabled=true`；main 尚无此文件 |
| 是否有用户开关 | **没有**。无 Config/Settings/环境变量；只有研究请求能选 control |
| 参考实现是否有消费者 | 有调用点：`JudgeLine::render_inner` 对 `JudgeLineKind::Normal` 调用；但整条路径未进 main |
| 是否进 main | **否**。`render_elision.rs` 不在 main；main 的 `line.rs` 没有 `try_skip_cpu_line` |
| 是否低画质限定 | 不是显式限定；低画质因 GPU 线批次先接管而自然 0 候选。普通画质是主要受益者 |
| 是否无损 | 在守卫条件（alpha 字节 0、默认材质、无深度、无 capture、有限仿射）下对 framebuffer 无损；不是通用无损 |
| 是否有独立验收 | 没有记录独立验收人；本文档是事后审计 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **默认开启且无用户开关**。这不是“研究候选默认关闭”的形态；一旦合并，所有普通画质的透明 CPU 线都会被跳过。必须先决定是否加 Config/Settings，还是接受默认开启。
2. **tokio SIGABRT 未定性**。后续 Main 多次启动出现 `tokio-rt-worker SIGABRT`；旧 `line-elision-final-lab` 同配置对照也出现，`#02 pc 0x1217eec / offset 0x856000` 相同，未见 panic message。没有证据把它归因于 P-04，但在原因未明前不能把“默认开启”推进主线。
3. **收益只在普通画质**。低画质 0 候选、无收益；普通画质的 FPS 已接近 60，收益表现为“少掉帧”而不是“从 30 到 60”。
4. **没有全曲/多 GPU/手动输入**。只测 Mali-T880、单谱面、autoplay、chart 60 s 起的约 30 s 窗口。
5. **参考实现未 push/未合并**。保留为已验证候选，等待开关设计与崩溃归因。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| 误判 alpha=0 | 若未来混合方程/材质改变，alpha=0 不再恒等 | 守卫要求默认材质；自定义材质 `blocked_state`；保护材质红线测试 |
| 深度/capture | 深度写入或 telemetry 需要图元 | 两者开启即回退；`blocked_state` 计数 |
| 非仿射/超大矩阵 | 透视或溢出可能产生不同裁剪 | `matrix_valid` 限制 4096/仿射；失败回退 |
| 调试线 | `chart_debug_line` 需要保留淡影 | debug 直接回退 |
| 默认开启无开关 | 线上误判无法由用户关闭 | 目前只能改源码；落地前应加开关或更强审计 |
| tokio SIGABRT | 启动/运行崩溃原因未定 | 不作为 P-04 因果；保留日志；未进 main |
| 计数只在研究构建 | 生产无法观测 skip 数 | 若落地，建议加低成本计数器或采样审计 |

#### 10.3 未验证项

- 全曲 162.7 s 的 normal/low 逐帧对照。
- Mali-G52 / Adreno / 其他 Mali 的 alpha=0 混合与驱动差异。
- 手动输入、暂停/重试、真实 Main 首次 Playing 前 5 s。
- `sampleCount=4`、MSAA、非默认材质、capture、depth 的组合。
- EGL surface loss/context 重建（P-04 无资源，但仍需确认调用点不会在重建中误判）。
- tokio SIGABRT 的根因与 P-04 的因果关系。
- 生产构建无 `render_research` 时的代码体积/分支预测影响（可忽略但未测）。

### 11. 落地步骤（可执行清单）

- [ ] 先决定开关形态：加 `Config.line_elision` + Settings，还是保持默认开启但加一次性审计；不能继续“默认开启、无用户开关”。
- [ ] 在参考实现中把 `Options::default().enabled` 改为由 Config 控制；研究请求仍可显式 control。
- [ ] 用真实 Main UI 打开/关闭开关，验证 `data.json`、重启、默认值。
- [ ] 跑完整 162.7 s 谱面的 normal/low 同 SO ABBA；报告应用墙钟、p99、max、>16.67/20/30 ms、呈现间隔。
- [ ] 在 `sampleCount=2/4`、`chart_debug`、`chart_debug_line`、capture、depth、非默认材质下逐一验证回退；每组至少一组 RGBA。
- [ ] 在 Mali-G52 和至少一种非 Mali GPU 上重复 normal 窗口；不能把 Mali-T880 外推。
- [ ] 对 `0 < a < 1/255`、`1/255` 边界、`-0.0`、NaN/INF 做设备端像素对照（CPU 单元测试已覆盖，但驱动量化仍需真机）。
- [ ] 定位 tokio SIGABRT：保留 pc/offset、symbolize、检查 home update/bold update 任务；在原因明确前不合并默认开启。
- [ ] 更新 `本地证据/AGENTS.md`/README，明确“默认开启、无用户开关、低画质无收益”，避免被误读为“默认关闭的研究候选”。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `be4e1305823bfebdad1853d7b333e0f79c3ead79`（2026-10-24） |
| 研究提交 | `657e40f13033afd09622321dfb664f41c95935d7`（研究透明判定线，2026-10-09 19:59:05 +0800） |
| 默认启用提交 | `689df63df8fffa8c6bd2e2509cec350b6beefeee`（启用透明线优化，2026-10-09 20:38:23 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/render_elision.rs`（208 行）、`prpr/src/core/line.rs`（调用点 405-421）、`prpr/src/lib.rs`（12）、`prpr-macroquad/src/quad_gl.rs`（1003-1018，`de34611b`） |
| 性能数据 | `本地证据/low-quality-normal/line-elision-v1-performance-results.json`；SO `aeebddc0a70f58970e88b15b0fd8e99d6273ac37c4be22759824fe748acef6c1` |
| 质量数据 | `line-elision-v1-quality-results.json`（6 作业/3 组 RGBA）；`line-elision-cpu-v1-quality-results.json`（2 作业/1 组 RGBA） |
| 设备 | EDI-AL10 / Mali-T880 / Android 8 / 2560×1440 |
| 谱面 | `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip` |
| 说明 | 正文已包含完整源码、八轮数据、质量计数与风险；上述路径只用于追溯 |

---

## P-05 block 分区裁剪 tiles

> 主线实施状态：☐ 未实施（main）（tiles）；bbox 是另一项，已实施（main，78445c20）
> 参考实现状态：已落地，未合入 main（tiles）；bbox 为另一项，已在 main
> 参考实现日期：2026-10-06　参考实现提交：`4f046544`（tiles）；bbox 主线提交：`78445c20`　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计）

### 1. 一句话结论

P-05 用“最终 mask 的保守支持域”限制 block overlay 的硬件 scissor：bbox 版本只画一个保守矩形，tiles 版本把分散的 16×16 mask tile 映射到 128×128 屏幕格并合并成互不重叠的多个矩形，对每个矩形重画同一个 full quad。Petrichor 105–111 s 的 block 密集段，tiles 把帧间隔 p50 从 **39.729 ms 降到 35.559 ms（−10.5%）**，但 render p50 从 **12.259 ms 升到 12.654 ms（略增）**；Petrichor 85–87 s 和 Happy ending 21–27 s 没有收益。它是**局部、条件性正优化**：只对“block 覆盖分散且确实 GPU fragment 受限”的窗口有效；不能外推到普通谱面、整帧 FPS、所有 GPU，也不能把 tiles 和 bbox 的独立 cohort 相减当成“tiles 比 bbox 多赚 1.1%”。

### 2. 问题与瓶颈（为什么原路径慢）

block overlay 是 chart 之后的独立后处理：它把最终 mask、scene、noise、spark 等纹理绑到同一个 full-screen quad，shader 内多次采样 mask/aux/noise，并按 `_EdgeOpacity`、`_FillStrength`、`_GlowIntensity`、`_SparkMapOpacity`、`_DisplaceStrength`、`_SDFCellSize` 等参数合成。对每个像素，即使最终 mask 在该处为 0、shader 最终 discard，fragment 仍然要先跑完这些采样和算术。

Petrichor 105–111 s 的实测：arena 控制组 `frame_interval p50 = 39.729 ms`（约 25 fps），`render p50 = 12.259 ms`，`block_overlay p50 = 4.552 ms`，`block_overlay p95 = 12.525 ms`。这说明该窗口不是 60 fps 上限，而是 GPU/驱动/排队共同造成的 ~40 ms 帧；block overlay 是其中一个显著阶段。85–87 s 的 `frame_interval p50 = 33.215 ms`，Happy ending 21–27 s 的 `frame_interval p50 = 34.304 ms`，但后两者的 block overlay p50 分别只有 1.055 ms 和 0.005 ms——没有 block 负担，自然没有裁剪收益。

原路径没有“只画有 mask 的区域”的概念：它总是画整个 quad。最终 mask 已经包含 displacement、subtract、edge、glow、ready、dilation 的结果；区域外为 0，shader 必定 discard，但 fragment 仍进入 shader。P-05 的目标是让这些 fragment 在进入 shader 前就被 scissor 拒绝，而不是改 shader、改顶点、改采样或降画质。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

#### 3.1 从最终 mask 推导保守支持域

关键不变量：**支持域必须从最终 mask 推导，不能从源矩形推导**。位移、subtract、dilation、ready 处理都已经在 mask 生成阶段完成；如果只看源 block 矩形，会漏掉位移/膨胀后的像素。`block_support.rs` 的注释写得很明确：

```text
//! Conservative support of the final masks, not of the source rectangles.
//! Displacement, subtraction, dilation and ready processing have already run.
```

对 active 层，像素 `p` 的 RGB 非零或 aux 的 R/G 非零就认为可能有输出；对 disabled 层，`p[3] != 0` 就认为可能有输出。这个并集比真实支持域**更宽**（例如 ready 只贡献 `|aux.r-aux.g|*aux.b`，但代码把 R/G 任一非零都算上），所以是保守的：不会漏掉任何可能可见的像素。

`Bounds::uv` 再加 `GUARD = 4` 个 texel：

- Linear 采样有半 texel 支持；
- `basePixelUV` 会 floor 到 2 texel 单元；
- 4 texel 同时覆盖两者并留舍入余量。

#### 3.2 从 UV 域映射到屏幕 scissor

`support_scissor` 取 UV 四角，经当前 `Projection * Model` 变换到裁剪空间，再映射到 viewport 的 framebuffer 坐标：

```text
screen.x = viewport.x + (p.x/p.w * 0.5 + 0.5) * viewport.w
screen.y = target_height - (viewport.y + (p.y/p.w * 0.5 + 0.5) * viewport.h)
```

然后 `floor` 左下、`ceil` 右上，各留 2 px guard，最后与已有逻辑 scissor 求交。只接受仿射、有限、`w > 1e-6` 且四角 `w` 相同的投影；透视/裁剪/非有限直接返回 `None`，回退到不裁剪。

#### 3.3 tiles：128×128 屏幕格 + 16×16 mask tile

`support_scissors` 先算 bbox。`sparse=false` 直接返回 `vec![bounding]`；`sparse=true` 才进入 tiles：

1. 屏幕按 `CELL = 128` 分成 `cols × rows` 的布尔格；
2. 对 16×16 mask tile 中每个 occupied tile，取其 UV 子矩形（左右各加 `5/effect_dim` 的 pad），用同一个 `support_scissor` 映射成屏幕矩形；
3. 把这个屏幕矩形覆盖的 128 格全部标记为 true；
4. 逐行扫描连续 true 段，合并成水平 run；
5. 若上一行有相同 `x0`、相同宽度且 `previous.y + previous.h == y0`，把高度加上去，形成垂直合并；
6. 若矩形数 > 32，或矩形总面积 ≥ bbox 面积的 85%，回退到单个 bbox（避免为了稀疏 mask 提交太多 scissor 状态）。

**为什么不会重复混合**：屏幕格互不重叠；水平 run 在同一行内互不重叠；垂直合并只发生在相同 x 范围且直接相邻的行。因此多个 scissor 矩形之间没有面积重叠。每个矩形都重画**同一个 full quad**，不是把几何切碎；顶点插值、UV、shader、uniform、混合顺序完全不变。代码注释也明确：

```text
// Grid cells do not overlap, including after rotation/rounding/MSAA. We
// redraw the SAME full quad; splitting geometry would change interpolation.
```

#### 3.4 不变量

- **同一 quad**：`draw_rectangle(-1., -1./aspect, 2., 2./aspect, WHITE)`，不按 tile 重建几何。
- **同一 shader/uniform/UV**：材质、纹理绑定、`uView`、`_ScreenParams`、`_EffectRT_TexelSize`、touch uniforms 全部不变。
- **同一顺序**：block overlay 仍在 chart effects 之后、global effects 之前；scissor 只影响当前 draw。
- **同一混合**：每个像素最多被一个 scissor 矩形覆盖一次，不会重复混合。
- **hover 回退**：有触点 hover 时 `clips = None`，走原 full quad；hover 会额外引入 touch mask/位移，支持域不是单纯 field mask。
- **投影回退**：非仿射/非有限/透视裁剪 → `support_scissor` 返回 `None` → 不裁剪。
- **已有 scissor 求交**：不覆盖调用方设置的逻辑 scissor；绘制后恢复 `old_clip`。

因此，在守卫成立时，被 scissor 拒绝的 fragment 在最终 mask 上为 0，原 shader 对它们的输出也是 0/discard；可见像素的 RGBA8 不变。这个结论由“最终 mask 支持域”保证，不由“截图看起来一样”单独保证。

### 4. 具体实现

#### 4.1 文件与符号清单（行数、函数/结构）

| 文件（参考实现，head `3670c2aa`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/core/block_shader.rs` | 1119 | `SUPPORT_MODE`（232-235）、`set_research_bounds`（237-240）、`support_scissor`（242-265）、`support_scissors`（267-317）、clip 选择（605-613）、逐 rect 绘制（808-823） | block overlay、bbox/tiles、scissor 提交 |
| `prpr/src/core/block_support.rs` | 129 | `Bounds`（3-32）、`Cache`（34-49）、`TILE_DIM = 16`（42）、`Cache::update`（51-79）、2 个测试（82-129） | 最终 mask 的保守 bbox + 16×16 tile 占用 |
| `prpr/src/scene/game.rs` | — | `research_block_bounds`（1295-1298）、`research_production_rendering`（1312-1323） | 研究作业设置 0/1/2；生产强制 bbox |
| `prpr/src/core.rs` | — | `pub(crate) use block_shader::set_research_bounds;`（43） | 导出 |
| `prpr/src/core/block_shader.rs`（main） | 1001 | `support_scissor`（272-303）、bbox 调用（620-629） | **生产 bbox 版本**；没有 `support_scissors`/tiles |
| `prpr/src/core/block_support.rs`（main） | 135 | `Bounds`、`Cache`（无 `TILE_DIM`/tile bool） | 生产 bbox 版本 |
| `phira/src/research.rs` | — | `block_bounds`（63）、`block_tiles`（64）、校验（292-297）、`GameScene::research_block_bounds`（441） | 研究请求字段 |
| `prpr/Cargo.toml` | — | `render_bounds = ["render_research"]`（14） | tiles/研究 bbox 的 feature 门 |
| `phira/Cargo.toml` | — | `render_bounds = ["render_research", "prpr/render_bounds"]`（17） | 主 crate 转发 |

行号说明：`block_shader.rs` 1119 行是参考实现的真实行数；`block_support.rs` 129 行是参考实现的真实行数。用户问题里“208 行”是 P-04 的 `render_elision.rs`，不是本条的 `block_shader.rs`。

#### 4.2 数据结构与算法（逐步说明）

1. **`Bounds`**：`x0/y0/x1/y1` 为半开区间；`include` 扩张 min/max；`uv` 加 4 texel guard 并归一化。`x1/y1` 存储的是“最后一个像素 + 1”，所以 UV 右/下界是 `x1 + GUARD`。
2. **`Cache`**：key 为 `(revision, aux_revision, width, height)`；revision 不变直接返回，避免每帧重扫。`active`/`disabled` 为 `Option<Bounds>`；`active_tiles`/`disabled_tiles` 为 `16×16` 的 `Vec<bool>`（参考实现额外字段）。
3. **`Cache::update`**：先清空 bounds/tiles；预计算每一列 x 对应的 tile 列 `x * 16 / width`；逐行扫描，`tile_row = y * 16 / height * 16`；对每个像素：
   - disabled：`p[3] != 0` → include disabled + tile true；
   - active：`p[..3] != [0,0,0] || a[..2] != [0,0]` → include active + tile true；
   - 最后写回 key。
4. **`support_scissor`**：见 3.2；只接受仿射有限投影；与旧 scissor 求交；返回 `(x,y,w,h)`。
5. **`support_scissors`**：先 bbox；`sparse=false` 返回单矩形；`sparse=true` 用 `CELL=128` 建格；对每个 occupied tile 用 `pad=5/effect_dim` 扩边后映射；标记格；逐行合并；垂直合并；若 `rects.len()>32` 或面积 ≥85% bbox，回退 bbox。
6. **clip 选择**：`if !hover && SUPPORT_MODE != 0` 才更新 cache 并调用；`disabled` 选 disabled 层，否则 active；`sparse` 只在 mode==2 时为 true；否则 `clips=None`。
7. **绘制**：`gl.flush()` 之后设置材质/纹理/uniform；若 `clips` 有值，逐个 `scissor(Some(*clip))` + `draw_rectangle`；最后恢复 `old_clip`，`gl_use_default_material()`。
8. **生产 bbox**：main 的 `block_shader.rs` 没有 `SUPPORT_MODE`/`set_research_bounds`/`support_scissors`；它总是 `if !hover { support.update(&masks.rgba, &masks.aux_rgba, masks.width, masks.height, masks.revision, masks.aux_revision); support_scissor(b.uv(masks.width, masks.height), aspect, viewport, target_height).map(|clip| vec![clip]) }`。因此 bbox 是生产默认行为，tiles 是研究增量。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

以下代码取自参考实现的 `3670c2aa`。

**`prpr/src/core/block_shader.rs:232-317`（模式、bbox、tiles）：**

```rust
#[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
thread_local! {
    static SUPPORT_MODE: std::cell::Cell<u8> = const { std::cell::Cell::new(if cfg!(feature = "render_research") { 0 } else { 1 }) };
}

#[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
pub(crate) fn set_research_bounds(enabled: bool, tiles: bool) {
    SUPPORT_MODE.with(|value| value.set(if !enabled { 0 } else if tiles { 2 } else { 1 }));
}

#[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
fn support_scissor(uv: [f32; 4], aspect: f32, viewport: (i32, i32, i32, i32), target_height: f32) -> Option<(i32, i32, i32, i32)> {
    let gl = unsafe { get_internal_gl() };
    let projection = gl.quad_gl.get_projection_matrix();
    let model = gl.quad_gl.get_model_matrix();
    let points = [(uv[0], uv[1]), (uv[2], uv[1]), (uv[2], uv[3]), (uv[0], uv[3])]
        .map(|(u, v)| projection * (model * vec4(u * 2. - 1., (v * 2. - 1.) / aspect, 0., 1.)));
    // Only affine, finite projections: perspective/clipped W falls back.
    if points.iter().any(|p| !p.is_finite() || p.w <= 1e-6 || p.w != points[0].w) { return None; }
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for p in points {
        let screen = vec2(viewport.0 as f32 + (p.x / p.w * 0.5 + 0.5) * viewport.2 as f32,
            target_height - (viewport.1 as f32 + (p.y / p.w * 0.5 + 0.5) * viewport.3 as f32));
        lo = lo.min(screen); hi = hi.max(screen);
    }
    // Keep full-pixel rounding guard; intersect the existing logical scissor.
    let old = gl.quad_gl.get_scissor().unwrap_or((viewport.0, target_height as i32 - viewport.1 - viewport.3, viewport.2, viewport.3));
    let x0 = (lo.x.floor() as i32).saturating_sub(2).max(old.0);
    let y0 = (lo.y.floor() as i32).saturating_sub(2).max(old.1);
    let x1 = (hi.x.ceil() as i32).saturating_add(2).min(old.0.saturating_add(old.2));
    let y1 = (hi.y.ceil() as i32).saturating_add(2).min(old.1.saturating_add(old.3));
    Some((x0, y0, x1.saturating_sub(x0).max(0), y1.saturating_sub(y0).max(0)))
}

#[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
fn support_scissors(tiles: &[bool], uv: [f32; 4], effect_dim: (usize, usize), aspect: f32,
    viewport: (i32, i32, i32, i32), target_dim: (f32, f32), sparse: bool) -> Option<Vec<(i32, i32, i32, i32)>> {
    let bounding = support_scissor(uv, aspect, viewport, target_dim.1)?;
    if !sparse { return Some(vec![bounding]); }
    // Rasterize conservative UV-cell support into an integer screen grid.
    // Grid cells do not overlap, including after rotation/rounding/MSAA. We
    // redraw the SAME full quad; splitting geometry would change interpolation.
    const CELL: i32 = 128;
    let (w, h) = (target_dim.0 as i32, target_dim.1 as i32);
    let (cols, rows) = (((w + CELL - 1) / CELL) as usize, ((h + CELL - 1) / CELL) as usize);
    let mut grid = vec![false; cols * rows];
    let pad = vec2(5. / effect_dim.0 as f32, 5. / effect_dim.1 as f32);
    for (index, occupied) in tiles.iter().enumerate() {
        if !occupied { continue; }
        let x = (index % support::TILE_DIM) as f32 / support::TILE_DIM as f32;
        let y = (index / support::TILE_DIM) as f32 / support::TILE_DIM as f32;
        let step = 1. / support::TILE_DIM as f32;
        let rect = support_scissor([(x - pad.x).max(0.), (y - pad.y).max(0.),
            (x + step + pad.x).min(1.), (y + step + pad.y).min(1.)], aspect, viewport, target_dim.1)?;
        if rect.2 <= 0 || rect.3 <= 0 { continue; }
        let x0 = rect.0.max(0) as usize / CELL as usize;
        let y0 = rect.1.max(0) as usize / CELL as usize;
        let x1 = ((rect.0.saturating_add(rect.2).min(w).max(0) + CELL - 1) / CELL) as usize;
        let y1 = ((rect.1.saturating_add(rect.3).min(h).max(0) + CELL - 1) / CELL) as usize;
        for y in y0.min(rows)..y1.min(rows) { for x in x0.min(cols)..x1.min(cols) { grid[y * cols + x] = true; } }
    }
    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    for y in 0..rows {
        let mut x = 0;
        while x < cols {
            if !grid[y * cols + x] { x += 1; continue; }
            let start = x;
            while x < cols && grid[y * cols + x] { x += 1; }
            let x0 = (start as i32 * CELL).max(bounding.0);
            let y0 = (y as i32 * CELL).max(bounding.1);
            let x1 = (x as i32 * CELL).min(bounding.0 + bounding.2);
            let y1 = ((y + 1) as i32 * CELL).min(bounding.1 + bounding.3);
            if x1 <= x0 || y1 <= y0 { continue; }
            if let Some(previous) = rects.iter_mut().find(|r| r.0 == x0 && r.2 == x1 - x0 && r.1 + r.3 == y0) {
                previous.3 += y1 - y0;
            } else { rects.push((x0, y0, x1 - x0, y1 - y0)); }
        }
    }
    let area: i64 = rects.iter().map(|r| r.2 as i64 * r.3 as i64).sum();
    // Avoid adding many state changes for a nearly solid mask. This is a
    // research heuristic, not a correctness requirement.
    if rects.len() > 32 || area as f64 >= bounding.2 as f64 * bounding.3 as f64 * 0.85 {
        Some(vec![bounding])
    } else { Some(rects) }
}
```

**`prpr/src/core/block_support.rs` 全文（129 行）：**

```rust
//! Conservative support of the final masks, not of the source rectangles.
//! Displacement, subtraction, dilation and ready processing have already run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bounds {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Bounds {
    fn include(bounds: &mut Option<Self>, x: usize, y: usize) {
        if let Some(b) = bounds {
            b.x0 = b.x0.min(x);
            b.y0 = b.y0.min(y);
            b.x1 = b.x1.max(x + 1);
            b.y1 = b.y1.max(y + 1);
        } else {
            *bounds = Some(Self { x0: x, y0: y, x1: x + 1, y1: y + 1 });
        }
    }

    pub fn uv(self, width: usize, height: usize) -> [f32; 4] {
        // Linear reads have half-texel support. basePixelUV floors to a two-
        // texel cell. Four texels cover both and include a rounding guard.
        const GUARD: usize = 4;
        [self.x0.saturating_sub(GUARD) as f32 / width as f32,
         self.y0.saturating_sub(GUARD) as f32 / height as f32,
         (self.x1 + GUARD).min(width) as f32 / width as f32,
         (self.y1 + GUARD).min(height) as f32 / height as f32]
    }
}

pub(super) struct Cache {
    key: Option<(u64, u64, usize, usize)>,
    pub active: Option<Bounds>,
    pub disabled: Option<Bounds>,
    pub active_tiles: Vec<bool>,
    pub disabled_tiles: Vec<bool>,
}

pub(super) const TILE_DIM: usize = 16;

impl Default for Cache {
    fn default() -> Self {
        Self { key: None, active: None, disabled: None,
            active_tiles: vec![false; TILE_DIM * TILE_DIM], disabled_tiles: vec![false; TILE_DIM * TILE_DIM] }
    }
}

impl Cache {
    pub fn update(&mut self, rgba: &[u8], aux: &[u8], width: usize, height: usize, revision: u64, aux_revision: u64) {
        let key = (revision, aux_revision, width, height);
        if self.key == Some(key) { return; }
        assert_eq!(rgba.len(), width * height * 4);
        assert_eq!(aux.len(), rgba.len());
        self.active = None;
        self.disabled = None;
        self.active_tiles.fill(false);
        self.disabled_tiles.fill(false);
        let x_tiles: Vec<_> = (0..width).map(|x| x * TILE_DIM / width).collect();
        for (y, (row, aux_row)) in rgba.chunks_exact(width * 4).zip(aux.chunks_exact(width * 4)).enumerate() {
          let tile_row = y * TILE_DIM / height * TILE_DIM;
          for (x, (p, a)) in row.chunks_exact(4).zip(aux_row.chunks_exact(4)).enumerate() {
            let tile = tile_row + x_tiles[x];
            if p[3] != 0 {
                Bounds::include(&mut self.disabled, x, y);
                self.disabled_tiles[tile] = true;
            }
            // Ready contributes |aux.r-aux.g|*aux.b. Unioning R/G is wider
            // than its true support, which is safe. Hover is handled elsewhere.
            if p[..3] != [0, 0, 0] || a[..2] != [0, 0] {
                Bounds::include(&mut self.active, x, y);
                self.active_tiles[tile] = true;
            }
          }
        }
        self.key = Some(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_contains_linear_and_two_texel_point_cells_at_every_edge() {
        for (w, h) in [(2, 2), (6, 10), (480, 300)] {
            for (x, y) in [(0, 0), (1, 1), (w / 2, h / 2), (w - 1, h - 1)] {
                let mut rgba = vec![0; w * h * 4];
                let mut aux = rgba.clone();
                let i = (y * w + x) * 4;
                // Isolate all coverage inputs and the disabled layer.
                for channel in 0..6 {
                    rgba.fill(0); aux.fill(0);
                    if channel < 4 { rgba[i + channel] = 1; } else { aux[i + channel - 4] = 1; }
                    let mut cache = Cache::default();
                    cache.update(&rgba, &aux, w, h, 1, 1);
                    let b = if channel == 3 { cache.disabled } else { cache.active }.unwrap().uv(w, h);
                    for (s, dimension, lo, hi) in [(x, w, b[0], b[2]), (y, h, b[1], b[3])] {
                        let cell = s / 2 * 2;
                        let expected_lo = ((s as f32 - 0.5).min(cell as f32) / dimension as f32).max(0.);
                        let expected_hi = ((s as f32 + 1.5).max((cell + 2) as f32) / dimension as f32).min(1.);
                        assert!(lo <= expected_lo && hi >= expected_hi);
                    }
                }
            }
        }
    }

    #[test]
    fn revision_invalidates_bounds_and_hover_does_not_define_field_support() {
        let mut rgba = vec![0; 8 * 8 * 4];
        let mut aux = rgba.clone();
        aux[3] = 255;
        let mut cache = Cache::default();
        cache.update(&rgba, &aux, 8, 8, 1, 1);
        assert!(cache.active.is_none() && cache.disabled.is_none());
        rgba[(7 * 8 + 7) * 4] = 255;
        cache.update(&rgba, &aux, 8, 8, 2, 1);
        assert!(cache.active.is_some());
        rgba.fill(0);
        cache.update(&rgba, &aux, 8, 8, 3, 1);
        assert!(cache.active.is_none());
        aux[1] = 255;
        cache.update(&rgba, &aux, 8, 8, 3, 2);
        assert!(cache.active.is_some());
    }
}
```

**clip 选择 `block_shader.rs:605-613`：**

```rust
        #[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
        let clips = if !hover && SUPPORT_MODE.with(|value| value.get()) != 0 {
            let FrameTextures { masks, support, .. } = &mut *frame;
            support.update(&masks.rgba, &masks.aux_rgba, masks.width, masks.height, masks.revision, masks.aux_revision);
            let bounds = if disabled { support.disabled } else { support.active };
            bounds.and_then(|b| support_scissors(if disabled { &support.disabled_tiles } else { &support.active_tiles },
                b.uv(masks.width, masks.height), (masks.width, masks.height), aspect, viewport, (target_width, target_height),
                SUPPORT_MODE.with(|value| value.get()) == 2))
        } else { None };
```

**逐 rect 绘制 `block_shader.rs:808-823`：**

```rust
        #[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
        let old_clip = unsafe { get_internal_gl() }.quad_gl.get_scissor();
        // Keep one quad: subdividing it changes vertex interpolation at point
        // sample boundaries, moving a few spark/noise texels between frames.
        // Specialize the expensive hover path instead of changing native UVs.
        #[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
        if let Some(clips) = &clips {
            for clip in clips {
                unsafe { get_internal_gl() }.quad_gl.scissor(Some(*clip));
                draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE);
            }
        } else { draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE); }
        #[cfg(all(not(feature = "render_bounds"), feature = "render_research"))]
        draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE);
        #[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
        if clips.is_some() { unsafe { get_internal_gl() }.quad_gl.scissor(old_clip); }
        gl_use_default_material();
```

#### 4.4 调用链与生命周期

调用链：

```text
GameScene::render
  -> BlockShader::render / block_shader::render
    -> frame.masks.render_displaced(width, height, aspect, zones, time)
    -> visible 检查（disabled 看 alpha，active 看 RGB / aux.RG）
    -> support.update(final mask RGBA, aux, revision, aux_revision)
    -> support_scissor / support_scissors
    -> gl.flush()
    -> 设置材质/纹理/uniform
    -> for clip in clips: scissor(clip); draw_rectangle(full quad)
    -> 恢复 old_clip；gl_use_default_material()
```

生命周期：

- `FrameTextures.support` 是 thread-local，随 block overlay 的 frame 缓存存在；`render_lifetime` 开启时，`MaskViewport.support` 会随两张交替 viewport 一起 swap，避免尺寸切换时丢缓存。
- `Cache::update` 只在 `(revision, aux_revision, w, h)` 变化时重扫；否则直接复用 bbox/tiles。
- 没有 GPU 资源；scissor 是 GL 状态，绘制后恢复旧值。若 `clips=None`，不设置 scissor，也不恢复（因为没有改）。
- `SUPPORT_MODE` 是 thread-local `Cell<u8>`，默认值在编译期决定：研究构建为 0，非研究构建为 1。研究作业通过 `GameScene::research_block_bounds(enabled, tiles)` 设置；生产没有设置点。
- `research_production_rendering(true, false)` 会把 mode 设为 `(true,false)`，即 bbox；它用于把研究生产渲染固定到已验证的 bbox 路径。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| 生产 bbox | main `78445c20`；`block_shader.rs:620-629` | **已在 main**；默认对所有非 hover block overlay 生效 |
| 研究 tiles | 参考实现 `4f046544` | 仅参考实现；未进 main |
| `SUPPORT_MODE` 默认 | 研究构建 0；非研究构建 1 | `cfg!(feature = "render_research") ? 0 : 1` |
| `set_research_bounds` | `(enabled, tiles)` → 0/1/2 | 只在 `render_bounds` 或非研究构建存在 |
| 研究请求 | `block_bounds`、`block_tiles`；`block_tiles` 要求 `block_bounds` | `research.rs:292-297` |
| feature | `prpr/render_bounds = ["render_research"]`；`phira/render_bounds = ["render_research","prpr/render_bounds"]` | tiles 需要研究构建 |
| 生产 Config/Settings | 无 | bbox 无用户开关；tiles 无生产入口 |
| 环境变量 | 无 | 只走 feature + 研究 job |
| 默认是否实际启用 | **bbox 是；tiles 否** | 生产只跑 bbox；tiles 只在研究 job 显式 `block_tiles=true` |
| hover | 回退 | `!hover` 才计算 clips；hover 走原 full quad |
| 空 mask | 已有 early return | `visible` 检查在裁剪之前 |

这里最容易误读的是“研究默认关闭裁剪”：`rendering-research.md` 写“研究默认关闭裁剪”，指的是研究构建 `SUPPORT_MODE=0`，不是生产 main。main 的 bbox 是生产默认；参考实现的 tiles 才需要 job 打开。两者不能混写。

### 6. 测量方法与完整数据（完整表格）

#### 6.1 测量口径

- 设备：MT6769V/CU / Android 10 / Mali-G52 MC2 / OpenGL ES 3.2 / 1920×1200 / 60 Hz。
- 谱面：
  - Petrichor：24 线、1436 音符、36 个 blockArea，新特性主要在 84–122 s；取 105–111 s（密集 block）和 85–87 s（不同 block 组合）。
  - Happy ending：24 线、1568 音符、无 blockArea；取 21–27 s 密集段。
- 采样：每轮 360 帧固定时钟；`sampleCount=2`、粒子/效果原精度、`blockAreaSimple=false`、不插 `glFinish`。
- 同 SO：`arena-tiles.so` SHA-256 `e5ec08cc1ce2c6b2caab3233f4753a9da46c714093b58ff7750b1fc63c0ae913`；12 项 AB/BA 交叉重复（3 场景 × 2 后端 × 2 轮）。
- 后端：`arena-control` 关 `block_bounds`/`block_tiles`；`arena-tiles` 同时开 `block_bounds`/`block_tiles`。
- 统计：每格 2 轮，表中为“每轮分位数的中位数”，不是混池分位数；重复范围不是置信区间。
- 质量：每个场景 3 组末帧对照（2 轮 control + 2 轮 tiles 的组合），共 9 组，1920×1200 RGBA 逐像素比较。

#### 6.2 bbox 独立 cohort（不是 tiles 的对照）

bbox 与 tiles 是两个独立 cohort，不能相减。bbox 的 12 项 AB/BA、9 项末帧 RGBA 全同：

| 场景 | arena 帧间隔 p50 ms | bbox 帧间隔 p50 ms | render p50 ms | 结论 |
|---|---:|---:|---|---|
| Petrichor 105–111 s | 39.803 | 36.044 | 12.327→12.883 | 约 −9.4%；render 略增 |
| Petrichor 85–87 s | 33.202 | 33.194 | 7.846→7.926 | 无显著收益 |
| Happy ending 21–27 s | 34.389 | 34.386 | 2.681→2.662 | 无显著收益 |

bbox 已在 main，是生产路径；它的 9.4% 只说明“该窗口的 bbox 有效”，不说明 tiles 的增量。

#### 6.3 tiles 完整分位数

下表是 `tiles-matrix-confirmed/matrix-comparison.json` 的完整关键列；每格为 2 轮中位数，括号内为两轮范围。

| 场景 | 后端 | 帧间隔 p50 ms | p95 ms | p99 ms | render p50 ms | render p95 ms | render p99 ms | chart p50 ms | block overlay p50 ms | block overlay p95 ms | cpu total p50 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Petrichor 105–111 s | control | 39.729 (39.728–39.730) | 46.341 (46.281–46.401) | 48.061 (48.015–48.107) | 12.259 (12.238–12.280) | 16.462 (16.448–16.475) | 19.323 (19.321–19.324) | 1.551 (1.539–1.564) | 4.552 (4.488–4.616) | 12.525 (12.500–12.550) | 12.368 (12.339–12.397) |
| Petrichor 105–111 s | tiles | **35.559** (35.517–35.602) | **43.919** (43.878–43.959) | **44.623** (44.613–44.633) | **12.654** (12.510–12.798) | 17.012 (16.735–17.289) | 19.750 (19.050–20.451) | 1.510 (1.477–1.542) | **5.311** (5.217–5.406) | 13.236 (13.188–13.285) | **12.757** (12.617–12.897) |
| Petrichor 85–87 s | control | 33.215 (33.175–33.255) | 34.905 (34.829–34.981) | 36.046 (35.364–36.728) | 8.049 (8.038–8.059) | 23.852 (23.300–24.404) | 31.400 (29.089–33.711) | 0.990 (0.987–0.992) | 1.055 (0.943–1.166) | 6.326 (6.227–6.425) | 8.175 (8.173–8.177) |
| Petrichor 85–87 s | tiles | 33.200 (33.150–33.249) | 34.851 (34.727–34.974) | 35.182 (35.125–35.240) | 8.107 (8.038–8.175) | 25.363 (24.519–26.206) | 29.054 (28.945–29.163) | 1.009 (0.984–1.033) | 1.123 (1.007–1.240) | 6.482 (6.428–6.535) | 8.240 (8.178–8.302) |
| Happy ending 21–27 s | control | 34.304 (34.301–34.308) | 35.689 (35.677–35.701) | 36.386 (36.339–36.434) | 2.589 (2.561–2.616) | 21.749 (21.544–21.953) | 24.252 (24.066–24.438) | 1.003 (1.002–1.005) | 0.00525 (0.00523–0.00527) | 0.00998 (0.00923–0.01073) | 2.737 (2.717–2.756) |
| Happy ending 21–27 s | tiles | 34.361 (34.335–34.387) | 35.665 (35.593–35.737) | 36.351 (36.226–36.475) | 2.663 (2.655–2.671) | 22.550 (22.407–22.692) | 24.495 (24.043–24.947) | 1.021 (1.021–1.022) | 0.00565 (0.00554–0.00577) | 0.01032 (0.00985–0.01079) | 2.794 (2.785–2.803) |

汇总：

| 场景 | 帧间隔 p50 变化 | 百分比 | render p50 变化 | block overlay p50 变化 | 结论 |
|---|---:|---:|---:|---:|---|
| Petrichor 105–111 s | 39.729→35.559 ms | **−4.170 ms / −10.50%** | +0.395 ms | +0.760 ms | 唯一明确收益；仍约 28 fps |
| Petrichor 85–87 s | 33.215→33.200 ms | −0.015 ms / −0.05% | +0.058 ms | +0.069 ms | 无收益 |
| Happy ending 21–27 s | 34.304→34.361 ms | +0.056 ms / +0.16% | +0.074 ms | +0.0004 ms | 无收益（略差在漂移内） |

关键解读：

- **帧间隔下降，但 render 和 CPU 都略增**。Petrichor 105–111 s 的 render p50 从 12.259 升到 12.654（+0.395 ms），block overlay p50 从 4.552 升到 5.311（+0.760 ms），cpu total 从 12.368 升到 12.757（+0.389 ms）。如果只看 CPU 阶段，会得出“更慢”；帧间隔却下降 4.17 ms。这与“减少不可见 fragment 的 GPU 工作、但增加 CPU 扫描/scissor 提交”的机制一致，但**没有 GPU elapsed 分解**，不能把 4.17 ms 精确归给 fragment 节省。
- **85–87 s 和 Happy ending 没有收益**。前者的 block overlay p50 只有 1.055 ms，后者只有 0.00525 ms；没有足够的不可见 fragment 可省，反而多付了扫描/提交成本。
- **35.559 ms 仍是约 28 fps**，不是 60 fps。P-05 是局部帧时间下降，不是整帧达标。
- **不能把 tiles 与 bbox 相减**。bbox 的 36.044 和 tiles 的 35.559 来自不同 cohort（不同运行批次/开关组合），差值 0.485 ms 不能解释为“tiles 比 bbox 多赚 1.2%”。`rendering-research.md` 明确写“该 cohort 与 bbox cohort 独立”。

#### 6.4 质量对照

| 组别 | 运行数 | 对照数 | 覆盖 | 结果 |
|---|---:|---:|---|---|
| `tiles-matrix-confirmed` | 12 | 9 | Petrichor 105–111 s / 85–87 s、Happy ending 21–27 s；每场景 control 重复 + tiles 重复 | 1920×1200 RGBA **逐像素相同**，`different_channels=0` |
| `bounds-matrix`（bbox 独立） | 12 | 9 | 同上三场景 | 1920×1200 RGBA **逐像素相同** |
| 单元测试 | — | 2 | 线性/2 texel point cell 覆盖；revision 失效与 hover 不定义 field support | 通过 |

末帧位置：Petrichor 110.98333 s 和 86.98333 s，覆盖不同 block 组合；Happy ending 21–27 s。仅这些采集帧验证等价，不是全谱逐帧。

#### 6.5 原始记录（README/AGENTS/rendering-research 对应段落）

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

当前 `本地证据/AGENTS.md` 已滚动到 2026-10 之后的记录，不含这段 bbox/tiles 原文；以下引自同目录历史快照 `本地证据/branch-cleanup-20261006/previous-AGENTS.md`，不伪造当前段落。`docs/development/rendering-research.md` 是参考实现内的正式说明，表格与本节一致。

`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 222 行原文：

> 2026-10-06追加实验：最终mask的保守支持域裁剪保持原quad/shader、过滤与采样不变，hover及不适合的投影回退。单bbox裁剪12项AB/BA（Petrichor两段及Happy ending）9项末帧RGBA比较全部一致。Petrichor105–111帧间隔p50 39.803→36.044ms（约9.4%），render 12.327→12.883ms略增；不要将其称为CPU加速。证据`本地证据/bounds-matrix`。非重叠分区最终12项交叉重复、9项末帧RGBA全部一致：Petrichor105–111帧间隔39.729→35.559ms（约10.5%），另外Petrichor85–87及Happy ending没有明显收益；证据`本地证据/tiles-matrix-confirmed`。不同cohort不能直接比较bbox和tiles的增量收益。

同文件第 228 行原文：

> 最终组合note pass+bbox裁剪完整12项AB/BA、9项末帧RGBA全部一致：相比同so arena控制，Petrichor105–111帧间隔39.756→35.539ms，85–87秒33.174→32.286，Happy ending34.353→32.537。证据`本地证据/combined-matrix-confirmed`；首次组合cohort在5项后ADB daemon连接故障，中断数据保留且不混入最终结果。可重跑suite为`research/suites/gameplay-combined.json`。本轮建议：共享arena/连续同目标pass优先，其次已验证note边界，再保守bbox裁剪；tiles比bbox更复杂，尚无可靠额外收益证据。GPU分解、能耗、全谱/hover/custom材质与跨设备仍属于明确未验证项。原始→arena、arena→组合两组独立cohort不可直接合成一次A/B。

同文件第 230 行原文：

> 本轮最终审计：同步clock、bbox、tiles、note pass、组合五组均完成12项运行/9项严格末帧RGBA比较，合计60项运行及45项像素比较；`research/evidence-index.json`固定索引hash，不混池性能。最新so SHA、149个构建输入文件hash和实验lock已核对，默认Cargo.toml/Cargo.lock恢复；普通Android check、研究release与8项host契约通过。主项目仍未commit/push，正式优化留待用户评审提案；研究载体与vendor实验已经交付。不得把这个阶段的完成解释成全谱/全设备、真实GPU时间或SkyFire完整播放已验证。

`docs/development/rendering-research.md`（参考实现内）第 239-263 行原文：

> 进一步从block效果的依赖推导：final mask已经包含位移、subtract、edge、glow与ready结果。区域外全零时原shader必定discard，但仍先读取多次mask。可以用保守支持域限制硬件scissor，让这部分fragment不进入shader；保留原quad、shader、uniform和采样运算，不通过改顶点或划分几何改变插值。
>
> 实验缓存active RGB与ready R/G的支持域、disabled A的支持域；留出覆盖Linear和两texel point-cell采样的边界余量，用实际Projection/Model/viewport映射到framebuffer，交集已有scissor并恢复。hover使用sceneUV和位移，先完整回退；不适合的投影也回退。GL scissor本身可用于旧GL基线，算法不依赖Vulkan。
>
> 单bbox路径完成12项AB/BA，三种场景的9项末帧RGBA比较全部一致。与arena同一个so的开关对照：
>
> | 场景 | arena帧间隔p50 ms | bbox裁剪 ms | render p50 ms |
> |---|---|---|---|
> | Petrichor105–111秒 | 39.803 | 36.044 | 12.327→12.883 |
> | Petrichor85–87秒 | 33.202 | 33.194 | 7.846→7.926 |
> | Happy ending21–27秒 | 34.389 | 34.386 | 2.681→2.662 |
>
> Petrichor密集block段完整帧约减少9.4%，另外两段无显著收益。扫描支持域增加CPU工作，render没有变快；这与减少不可见fragment的推导相符，但还不是纯GPU时间的测量证明。数据在`本地证据/bounds-matrix`。
>
> 再向前推导：单bbox覆盖中间空洞，分散mask可映射为固定整数屏幕格并合并成互不重叠矩形。对每个矩形重画同一个原quad，像素不会重复混合，插值不变；同target保持一个pass，避免每次scissor结束pass。区域过多或覆盖面积接近bbox时自动回到bbox，避免额外状态提交。
>
> 最终12项AB/BA交叉重复、9项末帧RGBA比较全部一致，数据在`本地证据/tiles-matrix-confirmed`：
>
> | 场景 | 同so关闭裁剪帧间隔p50 ms | 分区裁剪 ms | render p50 ms |
> |---|---|---|---|
> | Petrichor105–111秒 | 39.729 | 35.559 | 12.259→12.654 |
> | Petrichor85–87秒 | 33.215 | 33.200 | 8.049→8.107 |
> | Happy ending21–27秒 | 34.304 | 34.361 | 2.589→2.663 |
>
> 密集block段完整帧约减少10.5%；另外两段没有明显收益。该cohort与bbox cohort独立，不能把35.559与另一组36.044之间的差直接解释成可靠的增量收益。研究默认关闭裁剪；当前证据支持先选较简单的bbox方案，只有mask分散且额外draw成本合算时才考虑分区方案。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

- **bbox**：12 项 AB/BA、9 项末帧 RGBA 全同；已进 main。
- **tiles**：12 项 AB/BA、9 项末帧 RGBA 全同；`different_channels=0`，包括 control 自身重复性。
- **完整组合**（note pass + bbox）：12 项、9 项 RGBA 全同；首次组合 cohort 在 5 项后 ADB daemon 故障，失败数据保留且不混入最终结果。
- **单元测试**：线性/2 texel point cell 在每个边界都包含；revision 变化使 bounds 失效；hover 不定义 field support。
- **像素位置**：Petrichor 110.98333 s / 86.98333 s 两个不同 block 组合的末帧；不是只看一个时刻。
- **计数/后端**：149 个构建输入 hash、实验 lock、SO SHA 核对；普通 Android check、研究 release、8 项 host 契约通过（历史阶段记录）。

#### 7.2 失败项与限制

- **只有末帧**：9 组对照是末帧，不是全谱逐帧；不能证明所有 block 组合/所有时刻。
- **没有 hover**：hover 路径完整回退，未验证 touch mask 的支持域。
- **没有 custom shader**：block shader 的 ScreenTexture/自定义材质未覆盖。
- **没有其他 GPU**：只有 Mali-G52 MC2；Mali-T880 的 P-04 数据不能拿来替代。
- **没有 GPU 计时**：帧间隔下降、render/CPU 上升，只能用“减少不可见 fragment”的机制解释，不能精确分解。
- **没有 60 fps**：35.559 ms 仍是约 28 fps；这不是整帧达标。
- **tiles 的 CPU 成本**：`support_scissors` 每帧分配 `grid`、`rects`，并做逐 tile 映射；在 block overlay 很轻的窗口（Happy ending 0.005 ms）纯属额外开销。
- **bbox 已进 main，tiles 没有**：不要把“P-05 参考实现已落地”理解成 tiles 已生产；main 只有 bbox（另一项），tiles 仍在参考实现。
- **首次组合 cohort 失败**：ADB daemon 连接故障，原始数据保留但排除；不能用它补足样本。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化，但为条件性局部正优化。**

更精确地说：

- **bbox**：正优化，局部；已在 main。
- **tiles**：条件性正优化，局部；仅 Petrichor 105–111 s 这一 block 密集、mask 分散的窗口有明确收益；85–87 s 和 Happy ending 无收益。

理由：

1. **唯一收益窗口方向一致**：39.729→35.559 ms（−10.50%），p95 46.341→43.919，p99 48.061→44.623；两轮范围不重叠（control 39.728–39.730，tiles 35.517–35.602）。
2. **机制与阶段数据一致**：render/CPU 略增、帧间隔下降，符合“CPU 多扫一点、GPU 少跑不可见 fragment”；如果收益来自 CPU，render 应该下降，事实相反。
3. **无收益窗口也一致**：85–87 s 和 Happy ending 的 block overlay 很轻，tiles 只增加成本；这正好说明它是条件性优化，不是通用加速。
4. **画质证据强**：9 组 1920×1200 RGBA 逐像素相同；bbox 另有 9 组。
5. **为什么不是整帧**：只影响 block overlay；Petrichor 105–111 s 仍 35.6 ms；音符/线/粒子/HUD/其他 pass 不变。
6. **为什么不能外推**：单设备、单谱面、两个窗口、末帧对照、无 hover/custom shader/其他 GPU。

**置信度：**

- 对“Petrichor 105–111 s、Mali-G52、这个 SO 的 tiles 帧间隔下降”：**高**。
- 对“机制是减少不可见 fragment 的 GPU 工作”：**中高**；阶段数据支持，但没有 GPU elapsed。
- 对“tiles 比 bbox 更好”：**低**；独立 cohort，不能相减。
- 对“其他谱面/其他 GPU/整帧 FPS”：**低**。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。tiles 未实施；bbox 是另一项，已在 main（`78445c20`） |
| 参考实现是否默认启用 | **否**（tiles 默认关闭，研究 job 显式 `block_tiles=true`）；bbox 是另一项，main 默认启用 |
| 参考实现是否有消费者 | **无**（tiles 无生产消费者）；bbox 是另一项，main 有消费者（`block_shader.rs:620-629`） |
| 是否进 main | **否**（tiles 未进 main；bbox 是另一项，已在 main `78445c20`） |
| 是否有用户开关 | 无；bbox/tiles 都没有 Config/Settings |
| 是否低画质限定 | 否；bbox 对所有画质生效；tiles 只在研究 job |
| 是否依赖 GPU 扩展 | 否；GL scissor 是旧 GL 基线能力 |
| 是否有独立验收 | 没有记录独立验收人；本文档是事后审计 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 tiles 未进主线的原因

1. **收益只在单一窗口**：Petrichor 105–111 s 有 10.5%，另外两个窗口无收益；不足以证明通用性。
2. **没有证明比 bbox 更好**：bbox 已在 main，且更简单；tiles 的 35.559 与 bbox 的 36.044 是独立 cohort，不能相减。
3. **CPU 成本可观测**：render p50 +0.395 ms、block overlay +0.760 ms、cpu total +0.389 ms；在轻 block 窗口是纯负担。
4. **复杂度和状态提交**：最多 32 个 scissor 矩形；虽然超过 32 或面积 ≥85% 回退 bbox，但仍比单 bbox 多分支/分配。
5. **验证面不足**：无 hover、无 custom shader、无其他 GPU、只有末帧。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| scissor 状态泄漏 | 设置多个 scissor 后必须恢复 `old_clip` | 绘制后 `if clips.is_some() { scissor(old_clip) }`；`clips=None` 不改 |
| 支持域漏像素 | 位移/膨胀/subtract/ready 已在最终 mask；若未来新增后处理在 mask 之后，支持域会失效 | 从最终 mask 推导；新后处理必须重新审计 |
| 过滤边界 | Linear/2 texel point cell 需要 guard | `GUARD=4` texel + tile pad 5 texel |
| 旋转/非仿射 | 非仿射投影回退 | `w` 检查、`w != points[0].w` 回退 |
| 多矩形重复混合 | 矩形重叠会重复混合 | 屏幕格不重叠；垂直合并只并相邻同 x 范围；理论保证 |
| 矩形过多 | 状态提交反而变慢 | `>32` 或面积 ≥85% 回退 bbox |
| hover | touch mask/位移可能扩展支持域 | hover 完整回退 |
| 视口/aspect | viewport 与 target 尺寸不同、letterbox | 用实际 viewport 映射，target 尺寸裁剪；但未覆盖所有 aspect |
| MSAA | scissor 在 MSAA target 上的行为 | scissor 是 fragment 级，理论不变；未单独做 MSAA 像素对照 |
| 生产 bbox 无开关 | 若 bbox 在某些设备误裁，用户无法关闭 | 9 组 RGBA + main 生产；仍需更多设备验证 |

#### 10.3 未验证项

- 全谱逐帧、所有 block 组合、所有 chart-time。
- hover/触点、custom shader、Paint/glyph 上传、video/effect 组合。
- `sampleCount=1/2/4`、不同 MSAA、不同 aspect/viewport/letterbox。
- Mali-T880、Adreno、其他 Mali GPU。
- GPU elapsed/带宽/能耗；当前只有墙钟。
- 128 与 64/256 CELL 的对比；32 矩形上限是否最优。
- `render_lifetime` 下 spare viewport 与 support cache 的长期重入。
- bbox 在 main 的全曲/多设备回归（已有 9 组末帧，但不是全谱）。

### 11. 落地步骤（可执行清单）

- [ ] 保留 main 的 bbox 作为生产默认；不要因为 tiles 的 10.5% 就把 tiles 设为默认。
- [ ] 若要推进 tiles，先在更多 block 密集谱面/窗口重复 12 项 AB/BA，确认收益不是单一窗口。
- [ ] 用同 SO 做 bbox vs tiles 的直接 A/B（不是两个独立 cohort 相减），确认 tiles 的增量收益/成本。
- [ ] 加 hover、custom shader、不同 aspect/viewport、MSAA、sampleCount 1/2/4 的像素对照。
- [ ] 在至少两种非 Mali-G52 GPU 上重复 Petrichor 105–111 s；不能外推。
- [ ] 用 GPU timer 或可比的 GPU 阶段分解验证“fragment 节省”假设；没有就不要在文档里写“GPU 快 10.5%”。
- [ ] 评估 128/64/256 CELL、32 矩形上限、85% 面积阈值；用同一 SO 扫描，不要只调一个值。
- [ ] 检查 scissor 恢复在 panic/unwind/早退路径的行为；必要时用 RAII guard。
- [ ] 更新 `rendering-research.md`/AGENTS，明确“bbox 已进 main、tiles 仅研究、两 cohort 不可相减”。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 tiles 的 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

当前主线状态：tiles 为 `☐ 未实施（main）`；参考实现已落地，tiles 未合入 main；bbox 是另一项，已在 main（`78445c20`）。

真正把 tiles 合入 main 后填写：
- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `3670c2aa390b6b788d45c7eeb262d94f936b8b82`（2026-10-06 19:28:34 +0800） |
| 研究提交 | `4f046544c2a3d15b709a020e119c65bd29d75128`（2026-10-06 18:52:02 +0800） |
| 生产 bbox 提交 | `78445c201bbc4f2dfd08a3260f6f6b43cf19b746`（合入渲染与加载优化，2026-10-06 20:03:54 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/core/block_shader.rs`（1119 行研究/1001 行 main）、`prpr/src/core/block_support.rs`（129 行研究/135 行 main）、`prpr/src/scene/game.rs`、`prpr/src/core.rs`、`phira/src/research.rs`、`prpr/Cargo.toml`、`phira/Cargo.toml` |
| 性能数据 | `本地证据/tiles-matrix-confirmed/matrix.json`、`matrix-comparison.json`；`本地证据/bounds-matrix/*` |
| 质量数据 | `本地证据/tiles-confirmed.log`（9 组 RGBA 全同）、`本地证据/tiles-comparison.log` |
| 后端 | `arena-tiles.so` SHA-256 `e5ec08cc1ce2c6b2caab3233f4753a9da46c714093b58ff7750b1fc63c0ae913` |
| 设备 | MT6769V/CU / Mali-G52 MC2 / Android 10 / 1920×1200 |
| 谱面 | Petrichor（36 blockArea；105–111 s、85–87 s）；Happy ending（无 blockArea；21–27 s） |
| 说明 | 正文已包含完整代码、六组分位数、9 组 RGBA 与风险；上述路径只用于追溯 |

---

## P-06 图集增量版本

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-07 至 2026-10-09　参考实现提交：`2cecb104`（首个 `atlas_delta`/`atlas_pool`）、`39d82139`（字形更新对照）、`721d9f87`（合并上传区域）、`c3b1b976`（延迟发布）、`46c97226`（读取围栏）、`2b57559d`（通用字体图集池）　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计）

### 1. 一句话结论

P-06 把“每次字形变化就整图替换 2048² 图集”改成“32×32 tile 记录最后修改 generation，4 个 GPU 版本各带 fence，只上传该版本之后变化的矩形”：gameplay 上传量从 **494,927,872 B 降到 11,161,600 B（约 44.34×）**，HUD 最大耗时从 **20.489/26.074 ms 降到 6.449/7.277 ms**，`>20 ms` HUD 帧从 **1 降到 0**，但应用 FPS 仍约 **59.75**，没有提升。它是**图集上传/HUD 尖峰的局部正优化**，不是整帧 FPS 优化；代价是每池 4×16 MiB 名义 GPU 图像、两池合计 128 MiB 名义 GPU，加两份 16 MiB CPU 镜像（32 MiB）和 scratch，默认关闭。

### 2. 问题与瓶颈（为什么原路径慢）

原路径的图集替换是“每次字形变化后发布一整张 2048² RGBA 图集”：

- 单张 2048×2048 RGBA8 = 16 MiB；
- 一次 gameplay 内字形变化很多，整图上传会把 16 MiB 反复写进驱动；
- 即使只改一个 32×32 tile，也要传整张图；
- 原路径在当前设备上不仅是带宽，还会和已提交的读取形成读写依赖，表现为 HUD 20–46 ms 的长帧。

`本地证据/low-quality-normal/README.md` 的记录：

README 记录整图替换原型的四组画面 RGBA 相同，HUD 均值约 2.415→1.343–1.374 ms，呈现最大间隔仍有 33–50 ms；但每轮上传 2,080,374,784 B（约 1.94 GiB / 20 s），原路径仅 1,664,600 B。因此整图替换能消掉 HUD 尖峰，却不能直接生产。
>
> 代价：每轮替换上传 2,080,374,784 bytes（约 1.94GiB / 约20s），原路径仅 1,664,600 bytes。此实现尚不适合直接进入生产；不把原型展示的收益与待实现的低上传方案混为一谈。

也就是说，整图替换能消掉 HUD 尖峰，但上传量从 1.6 MB 涨到 1.94 GiB，不能直接生产。后续“裁切快照”把上传降到 494,927,872 B（472 MiB/20 s），仍是原路径的约 297×；P-06 的增量版本进一步降到 11,161,600 B。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

#### 3.1 权威 CPU 镜像 + 有界版本历史

图集的权威像素在 CPU 的 `shadow`（完整 RGBA 镜像）。GPU 侧保留 **4 个完整版本**（4 张 2048² 纹理）。每次发布：

1. 调用方给出本帧的 `writes`（每个矩形 `[x0,y0,x1,y1]`）；
2. `Delta::advance(writes)` 把 generation 加 1，并把每个写矩形覆盖的 32×32 tile 标记为当前 generation；
3. 选一个“安全”的旧版本 slot：它的 fence 已 signal，且没有仍在 CPU 队列里的读取；
4. `Delta::since(slot.version)` 返回该 slot 之后变化的矩形（按行合并水平 run，再合并相邻同宽垂直 run）；
5. 只把这些矩形从 `shadow` 拷到 scratch，逐矩形 `update_texture_part`；
6. 把 slot.version 设为当前 generation，返回这张纹理；
7. 旧纹理由调用方在“已提交读取之后”删除；GL 保留在途存储。

#### 3.2 为什么是 4 个版本

一个版本可能正在被 GPU 读取；另一个版本可能还有 CPU 队列中的 draw 未提交；第三个版本可能刚刚发布。4 个版本给“当前 + 在途 + 上一次 + 备用”留出余量。**4 不是正确性证明**：如果 4 个都不安全，`publish` 不会等待/覆盖，而是删除一个逻辑 slot、上传整张新快照（busy fallback）；如果连这个也失败，返回普通 mutable image。因此“有界版本 + 安全回退”比“假设 4 个一定够”更准确。

#### 3.3 fence 与 CPU 读取

Android 上加载 ES3 核心入口：

- `glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0)`；
- `glClientWaitSync(fence, 0, 0)`：**零 timeout、无 flush flag、无重试循环**；
- `glDeleteSync`。

fence 在旧 macroquad 队列 flush 之后插入，覆盖已提交的 GPU 读取。研究版另有**可选 deferred 模式**：`prepare_deferred_update`/`after_forced_flush` 用 `TextureReadToken`/`TextureReadStatus` 记录 managed draw 的 queued/last_submitted，必要时刷新 fence；unknown 队列会 flush 后再继续。P-06 的 8 个性能作业没有启用 `font_pool_deferred`，主要走“fence ready + 发布顺序 + 必要 flush”，不是每条都查 read token。

#### 3.4 不变量与画质

- **字形栅格、排布、UV、绘制队列不变**：`Delta` 只决定“上传哪些矩形”，不改变 glyph 的生成/布局/采样。
- **CPU 镜像是权威**：每个矩形都从 `shadow` 逐字节拷贝；没有重采样、没有格式转换（RGBA 路径）。
- **版本内容完整**：每个新版本从旧版本继承，再加上 `since` 的矩形；`since` 覆盖所有 generation 变化，不会漏 tile。
- **busy 回退是整图快照**：如果找不到安全 slot，上传整张 `shadow`，画面仍完整。
- **不覆盖在途图像**：忙时删除逻辑 slot / 新建快照，不写正在被读取的纹理。
- **1000 轮 oracle**：任意重叠/边界写、4 个不同滞后版本，重建位图与独立 byte-array oracle 逐字节相同。

因此，RGBA 路径在“矩形覆盖正确 + 整图回退正确”时逐像素不变。P-06 不声称“所有 GPU/所有字体/所有首帧”都一样；它只证明这些有限画面和 CPU oracle。

### 4. 具体实现

#### 4.1 文件与符号清单（行数、函数/结构）

| 文件（参考实现，head `be4e1305`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/ui/atlas_delta.rs` | 143 | `TILE = 32`（3）、`Rect`（4-10）、`Delta`（11-18）、`new`（20-32）、`generation`（33-35）、`nominal_bytes`（36-38）、`advance`（39-49）、`since`（50-82）、2 个测试（85-143） | 32×32 tile generation / 矩形合并 |
| `prpr/src/ui/atlas_pool.rs` | 880 | `sync_api`（23-122）、`SLOTS = 4`（124）、`BUDGET = 64 MiB`（125）、`Slot`（134-141）、`Publication`（142-146）、`Pool`（147-203）、`warm`（477-537）、`publish`（538-814）、`report`（826-880） | 研究版 4 槽池、fence、delta 上传、回退 |
| `prpr/src/ui/font_atlas_pool.rs` | 1101 | `SLOTS = 4`（7）、`Slot`（140-146）、`Publication`（147-158）、`Pool`（159-189）、`history = Delta::new`（586）、`since`（817） | 生产候选字体池，复用同一 `Delta` |
| `prpr/src/ui/text.rs` | — | `lab_atlas_pool`（332）、`lab_atlas_writes`（334）、`research_pool_mode` 等 | 研究版 TextPainter 接线 |
| `prpr/Cargo.toml` | — | `font_atlas_pool = []`（15）、`render_atlas_pool = ["render_research"]`（16）、`default = ["log","font_atlas_pool"]`（18） | 默认编入生产池；研究池需 feature |
| `phira/Cargo.toml` | — | `font_atlas_pool`（15）、`render_atlas_pool`（16）、默认含 `font_atlas_pool`（18） | 主 crate 转发 |
| `phira/src/render_research.rs` | — | `font_pool_mode`（471）、`font_production_pool`（452-462）、`font_pool_policy/channels/deferred`（467-469） | 研究请求；生产池默认 false |

#### 4.2 数据结构与算法（逐步说明）

1. **`Rect`**：`x/y/w/h`，半开矩形；`since` 返回的矩形已经裁剪到图集边界。
2. **`Delta`**：`width/height/columns/rows/generation/versions`。`versions` 长度 = `columns*rows`，每个 tile 存最后修改 generation。内存只随图集面积增长（2048² / 32² = 64×64 = 4096 个 u64 = 32 KiB），不随曲长增长。
3. **`advance`**：generation 用 `checked_add(1).expect("atlas generation overflow")`；每个写矩形断言 `x0<x1`、`y0<y1`、在界内；对覆盖的 tile 行/列写当前 generation。
4. **`since(version)`**：断言 `version <= generation`；逐行扫描，跳过 `versions <= version` 的 tile；连续 `> version` 的 tile 形成水平 run；生成 `Rect { x: start*TILE, y: row*TILE, w: min(x*TILE,width)-start*TILE, h: min(TILE,height-row*TILE) }`；如果上一矩形同 x、同 w、且 `previous.y+previous.h == rect.y`，合并高度。返回的矩形可能跨多行，但不会重叠。
5. **`Pool::warm`**：检查 `width*height*4` 与 shadow 长度一致；按 `channels::bytes_per_pixel` 算每槽字节；`bytes*SLOTS <= 64 MiB`；如果已有 slot 返回 current；flush 旧队列；创建 `Delta`；按需创建 `Damage`；分配 1 MiB scratch；循环 `SLOTS` 次创建纹理、插入 fence、push `Slot { damage_id, texture, version:0, fence:Some, reads:None, fenced_read_serial:0 }`，并尝试注册读 token（非 deferred 模式为 None）；检查 GL error；current = slot[0]。
6. **`Pool::publish`**：先验证 `reserved`（延迟模式）仍然有效；更新 `publications/raw_write_*`；shape 变化则 clear；没有 history 则 warm；`history.advance(writes)`；如果当前槽的 fence 存在，删除并插入新 fence；选择一个可用 slot：
   - 优先 `reserved`；
   - 否则遍历非 current slot，`api.ready(fence)` 零 timeout；policy 0 取第一个 ready，其他 policy 取最新 version；
   - 如果没有任何 ready，进入 busy fallback；
   - 计算 `gap = generation - slot.version`，记录 `chosen_version_gap_*`；
   - `rectangles = history.since(slot.version)`（或 damage/coalesce 变体）；
   - 逐矩形从 shadow 拷贝到 scratch（compact 模式走 `channels::pack_part`），`update_texture_part`；
   - 成功后 `slot.version = generation`、current = slot.texture、`reused += 1`、`delta_bytes += transferred`，返回 `Publication`；
   - 失败则 `failure()` 回退。
7. **busy fallback**：`busy += 1`；如果 deferred 模式先 flush；`slots.remove(0)`；删除 fence/unregister token/`clear_resource_bindings`/删除纹理；用整张 shadow 新建纹理，push 新 Slot（version = 当前 generation，fence=None，reads=None），注册 token，deferred 模式补 fence；成功则 `fresh += 1`、`full_bytes += bytes`，返回整图。
8. **最终回退**：如果没有 api/history/slot，直接 `image(width,height,shadow)`，`fresh += 1`，`full_bytes += shadow.len()`。
9. **`report`**：输出 `slots/shape/policy/publications/raw_write_bytes/raw_write_rects/transferred_rects/chosen_version_gap_*/history_metadata_bytes/coalesce_*/warm_bytes/reused/fresh/busy_fallbacks/failed_fallbacks/polls/delta_bytes/full_bytes/blocking_waits=0`；`nominal_storage_bytes` 只统计 live slots，不含 fallback/CPU shadow/driver/in-flight retired storage，明确不是物理 GPU 内存。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

**`prpr/src/ui/atlas_delta.rs` 全文（143 行）：**

```rust
//! Bounded change history: each 32x32 tile stores its last modified generation.
//! A lagging image can replay changes directly from the authoritative bitmap.
pub const TILE: u32 = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
pub struct Delta {
    width: u32,
    height: u32,
    columns: u32,
    rows: u32,
    generation: u64,
    versions: Vec<u64>,
}
impl Delta {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0);
        let columns = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        Self {
            width,
            height,
            columns,
            rows,
            generation: 0,
            versions: vec![0; (columns * rows) as usize],
        }
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn nominal_bytes(&self) -> usize {
        self.versions.capacity() * std::mem::size_of::<u64>()
    }
    pub fn advance(&mut self, writes: &[[u32; 4]]) {
        self.generation = self.generation.checked_add(1).expect("atlas generation overflow");
        for &[x0, y0, x1, y1] in writes {
            assert!(x0 < x1 && y0 < y1 && x1 <= self.width && y1 <= self.height);
            for y in y0 / TILE..y1.div_ceil(TILE) {
                for x in x0 / TILE..x1.div_ceil(TILE) {
                    self.versions[(y * self.columns + x) as usize] = self.generation;
                }
            }
        }
    }
    pub fn since(&self, version: u64) -> Vec<Rect> {
        assert!(version <= self.generation);
        let mut rectangles: Vec<Rect> = Vec::new();
        for row in 0..self.rows {
            let mut x = 0;
            while x < self.columns {
                if self.versions[(row * self.columns + x) as usize] <= version {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.columns && self.versions[(row * self.columns + x) as usize] > version {
                    x += 1;
                }
                let rect = Rect {
                    x: start * TILE,
                    y: row * TILE,
                    w: (x * TILE).min(self.width) - start * TILE,
                    h: TILE.min(self.height - row * TILE),
                };
                if let Some(previous) = rectangles
                    .iter_mut()
                    .rev()
                    .find(|r| r.x == rect.x && r.w == rect.w && r.y + r.h == rect.y)
                {
                    previous.h += rect.h;
                } else {
                    rectangles.push(rect);
                }
            }
        }
        rectangles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn apply(image: &mut [u8], source: &[u8], width: u32, rect: Rect) {
        for y in rect.y..rect.y + rect.h {
            let from = ((y * width + rect.x) * 4) as usize;
            let end = from + (rect.w * 4) as usize;
            image[from..end].copy_from_slice(&source[from..end]);
        }
    }
    #[test]
    fn staggered_images_reconstruct_the_exact_bitmap_after_arbitrary_changes() {
        // Independent byte-array oracle; includes overlapping/edge writes and
        // slots that skip several publications. No finite journal to overflow.
        let (w, h) = (103, 77);
        let mut delta = Delta::new(w, h);
        let mut source = vec![0; (w * h * 4) as usize];
        let mut slots = vec![(0, source.clone()); 4];
        let mut random = 7u32;
        for frame in 0..1000 {
            let mut writes = Vec::new();
            for _ in 0..1 + frame % 5 {
                let mut next = || {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    random
                };
                let x0 = next() % w;
                let y0 = next() % h;
                let x1 = (x0 + 1 + next() % 39).min(w);
                let y1 = (y0 + 1 + next() % 35).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        for c in 0..4 {
                            source[((y * w + x) * 4 + c) as usize] = next() as u8;
                        }
                    }
                }
                writes.push([x0, y0, x1, y1]);
            }
            delta.advance(&writes);
            let index = (frame / 3 % 4) as usize;
            let (version, image) = &mut slots[index];
            for rect in delta.since(*version) {
                apply(image, &source, w, rect);
            }
            assert_eq!(*image, source, "publication {frame}");
            *version = delta.generation();
        }
    }
    #[test]
    fn empty_changes_transfer_nothing_and_a_full_change_merges_to_one_rectangle() {
        let mut delta = Delta::new(103, 77);
        delta.advance(&[]);
        assert!(delta.since(0).is_empty());
        delta.advance(&[[0, 0, 103, 77]]);
        assert_eq!(delta.since(0), vec![Rect { x: 0, y: 0, w: 103, h: 77 }]);
        assert!(delta.since(delta.generation()).is_empty());
    }
}
```

**`prpr/src/ui/atlas_pool.rs:23-122`（ES3 sync，完整）：**

```rust
#[cfg(target_os = "android")]
mod sync_api {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    }
    type Insert = unsafe extern "C" fn(u32, u32) -> *mut c_void;
    type Poll = unsafe extern "C" fn(*mut c_void, u32, u64) -> u32;
    type Delete = unsafe extern "C" fn(*mut c_void);
    #[derive(Clone, Copy)]
    pub struct Api {
        insert: Insert,
        poll: Poll,
        delete: Delete,
    }
    pub type Fence = *mut c_void;
    impl Api {
        pub fn load() -> Result<Self, String> {
            unsafe {
                let pointer = miniquad::gl::glGetString(0x1f02);
                if pointer.is_null() {
                    return Err("No current GL context".into());
                }
                let version = CStr::from_ptr(pointer.cast()).to_string_lossy();
                let major = version
                    .strip_prefix("OpenGL ES ")
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<u32>().ok());
                if major.unwrap_or(0) < 3 {
                    return Err(format!("Core ES3 sync unavailable: {version}"));
                }
                macro_rules! load {
                    ($name:literal,$ty:ty) => {{
                        let p = eglGetProcAddress(concat!($name, "\0").as_ptr().cast());
                        if p.is_null() {
                            return Err(concat!("Missing ", $name).into());
                        }
                        std::mem::transmute::<*const c_void, $ty>(p)
                    }};
                }
                let api = Self {
                    insert: load!("glFenceSync", Insert),
                    poll: load!("glClientWaitSync", Poll),
                    delete: load!("glDeleteSync", Delete),
                };
                let fence = api.insert()?;
                let checked = api.ready(fence);
                api.delete(fence);
                checked?;
                if miniquad::gl::glGetError() != 0 {
                    return Err("Core sync probe GL error".into());
                }
                Ok(api)
            }
        }
        pub fn insert(&self) -> Result<Fence, String> {
            unsafe {
                let fence = (self.insert)(0x9117, 0);
                if fence.is_null() {
                    return Err("glFenceSync returned null".into());
                }
                Ok(fence)
            }
        }
        pub fn ready(&self, fence: Fence) -> Result<bool, String> {
            unsafe {
                match (self.poll)(fence, 0, 0) {
                    // no flush flag, no wait, no retry loop
                    0x911a | 0x911c => Ok(true),
                    0x911b => Ok(false),
                    other => Err(format!("Sync poll failed {other:#x}")),
                }
            }
        }
        pub fn delete(&self, fence: Fence) {
            unsafe {
                (self.delete)(fence);
            }
        }
    }
}
#[cfg(not(target_os = "android"))]
mod sync_api {
    pub type Fence = usize;
    #[derive(Clone, Copy)]
    pub struct Api;
    impl Api {
        pub fn load() -> Result<Self, String> {
            Err("Non-Android research backend uses snapshot fallback".into())
        }
        pub fn insert(&self) -> Result<Fence, String> {
            unreachable!()
        }
        pub fn ready(&self, _: Fence) -> Result<bool, String> {
            unreachable!()
        }
        pub fn delete(&self, _: Fence) {}
    }
}
```

**`Slot` / `Publication` / 关键常量 `atlas_pool.rs:124-146`：**

```rust
use sync_api::{Api, Fence};
const SLOTS: usize = 4;
const BUDGET: usize = 64 * 1024 * 1024;
fn read_status(token: TextureReadToken) -> Option<TextureReadStatus> {
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.texture_read_status(token, gl.quad_context.context_id())
}
fn unregister(token: TextureReadToken) -> bool {
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.unregister_texture_reads(token, gl.quad_context.context_id())
}
struct Slot {
    damage_id: usize,
    texture: Texture2D,
    version: u64,
    fence: Option<Fence>,
    reads: Option<TextureReadToken>,
    fenced_read_serial: u64,
}
pub struct Publication {
    pub texture: Texture2D,
    pub bytes: usize,
    pub previous_owned: bool,
}
```

**`Pool::warm` 全文 `atlas_pool.rs:477-537`：**

```rust
    pub fn warm(&mut self, width: u32, height: u32, shadow: &[u8]) -> Option<Texture2D> {
        let api = self.api?;
        let rgba_bytes = width as usize * height as usize * 4;
        assert_eq!(shadow.len(), rgba_bytes);
        let bytes = rgba_bytes / 4 * channels::bytes_per_pixel(self.channels_enabled);
        if bytes.checked_mul(SLOTS).is_none_or(|v| v > BUDGET) {
            self.failure("Atlas pool exceeds 64MiB budget".into());
            return None;
        }
        if !self.slots.is_empty() {
            return self.current;
        }
        // Loading may have queued text using the owner's original atlas. Its
        // replacement must not make that old image disappear before submission.
        // Both controls consume this loading queue before creating fresh slots.
        unsafe { get_internal_gl() }.flush();
        self.shape = (width, height);
        self.history = Some(Delta::new(width, height));
        self.damage = (self.policy >= 2).then(|| Damage::new(width, height, SLOTS));
        // Touch reusable CPU upload capacity during loading, not gameplay.
        self.scratch = vec![0; 1024 * 1024];
        self.scratch.clear();
        for damage_id in 0..SLOTS {
            let texture = if self.channels_enabled {
                match channels::create(width, height, shadow, true) {
                    Ok(texture) => texture,
                    Err(reason) => {
                        self.failure(format!("Compact atlas loading allocation failed: {reason}"));
                        return None;
                    }
                }
            } else {
                image(width, height, shadow)
            };
            self.warmed += bytes;
            let fence = match api.insert() {
                Ok(fence) => fence,
                Err(reason) => {
                    texture.delete();
                    self.failure(reason);
                    return None;
                }
            };
            self.slots.push(Slot {
                damage_id,
                texture,
                version: 0,
                fence: Some(fence),
                reads: None,
                fenced_read_serial: 0,
            });
            let token = self.register(texture);
            self.slots.last_mut().unwrap().reads = token;
            if unsafe { miniquad::gl::glGetError() } != 0 {
                self.failure("Atlas pool loading allocation/probe GL error".into());
                return None;
            }
        }
        self.current = Some(self.slots[0].texture);
        self.current
    }
```

#### 4.4 调用链与生命周期

调用链：

```text
TextPainter::prepare / draw
  -> 字形栅格写入 CPU shadow（lab_atlas_shadow / production_atlas.shadow）
  -> 记录 writes: Vec<[u32;4]>（lab_atlas_writes）
  -> Pool::prepare_deferred_update()（可选，写前预留）
  -> Pool::publish(previous, width, height, shadow, writes)
       -> Delta::advance(writes)
       -> 选 ready slot（fence + no queued read）
       -> Delta::since(slot.version)
       -> copy rects from shadow -> update_texture_part
       -> slot.version = generation
  -> 返回 Publication { texture, bytes, previous_owned }
  -> 调用方在已提交读取之后删除旧纹理 / 由 GL 保留在途
```

生命周期：

- `warm` 在加载阶段创建 4 张完整纹理和 fence，随后 `current = slots[0]`；这会把加载期的整图上传成本前移，但 P-06 的测量仍统计了 gameplay 窗口的上传，加载成本另计。
- `publish` 在 gameplay 每次字形变化时调用；如果没有安全 slot，忙回退上传整图，不阻塞。
- `Drop`：`deferred_requested` 时先 `flush()`；遍历 slots，unregister token、delete fence、删除非 current 纹理（current 由 TextPainter 队列拥有）；避免删除正在被引用的 current。
- `clear`：先 flush CPU 队列，`clear_resource_bindings()`，再删除所有 slot/fence/history/damage；用于 resize/disable/context 生命周期。
- `report` 的 `blocking_waits=0` 是设计值：`ready` 只用零 timeout；没有 `glFinish`/重试循环。
- 非 Android 研究后端 `Api::load()` 直接返回错误，`Pool` 走快照回退；因此 P-06 的 fence 路径只在 Android ES3+ 真机生效。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| `font_atlas_pool` feature | `prpr/Cargo.toml:15`，`phira/Cargo.toml:15`；prpr 默认 `["log","font_atlas_pool"]`，phira 默认含它 | **编译期默认编入**生产候选池 |
| 生产池运行开关 | `TextPainter::research_production_pool(enabled)`，`LAB_PRODUCTION_POOL` 默认 `false` | 默认关闭；研究请求 `font_production_pool=true` 才开 |
| 生产池模式 | `font_production_pool_mode` 0..3 | 0=disabled，1=automatic，2=forced unsupported，3=forced busy |
| 研究池 feature | `render_atlas_pool = ["render_research"]`；phira 转发 | lab `atlas_pool` 只在研究构建 |
| 研究池模式 | `font_pool_mode` 0..3；`font_pool_policy` 0..3；`font_pool_channels` 0..3；`font_pool_deferred` | 0=off；2=强制缺能力；3=强制 busy；policy 0=first-ready，1=latest，2=damage，3=coalesce |
| 默认 | **默认关闭** | 生产/研究都需要请求显式打开；`font_pool_mode=0` |
| Config/Settings | 无 | 没有用户 UI 开关 |
| 环境变量 | 无 | 只走 feature + 研究 job |
| 预算 | `BUDGET = 64 MiB`/池；`SLOTS=4` | 2048² RGBA 时 4×16 MiB=64 MiB/池 |
| 非 Android | 快照回退 | `Api::load()` 返回错误；不伪造 fence |
| ES3 门 | `major >= 3` 且三个入口非空 + 试调用 | 缺能力回退整图快照 |

这里必须区分两层默认：

1. **编译期**：`font_atlas_pool` 默认编入，所以代码存在；
2. **运行期**：`LAB_PRODUCTION_POOL` 默认 `false`，`font_production_pool_mode=0`，所以不创建池、不分配 4 张图、不走 delta。

“默认编入、默认关闭”是准确说法；“默认开启”不成立。

### 6. 测量方法与完整数据（完整表格）

#### 6.1 测量口径

- 设备：EDI-AL10 / Mali-T880 / Android 8 / arm64-v8a / 2560×1440。
- 谱面：`f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip`，24 线、1349 普通非 hold、219 holds。
- 采样：每轮 120 帧预热 + 1200 帧真实时钟；`sampleCount=1`、`particle=true`、`disableEffect=false`、`blockAreaSimple=false`、`offlineMode=true`；`start_time=60`。
- 同 SO：注入后端 `pool-lab.so` SHA-256 `8a9eaff6fe65100fcf44f188d8c9d1a272adad6d696bdd47bd9382cd6adf1c2d`；源码后端 `branch-lab.so` SHA-256 `51dd5d53cf6640fc47683663425ef7733ad43030a79e0258f4ee27eacce8d3ec`（artifact 记录 17 个源文件 hash、`source_commit=2cecb104`，features `render_research,render_atlas_pool,render_chart_ir`）。
- 交错：snapshot / delta / delta / snapshot（ABBA）。
- 上传量：`uploaded_bytes` 为 gameplay 窗口计数；`pool.pgr.delta_bytes` 是池内部计数（含 warm-up/其他矩形），两者不要混用。
- 应用墙钟/帧间隔是 CPU/driver 墙钟，不是 GPU elapsed；呈现是独立约 10 s 窗口。

#### 6.2 注入后端（`pool-lab.so`，ABBA 四轮）

| 次序 | 策略 | 应用 FPS | 应用 p99 ms | HUD 最大 ms | HUD >20 ms 帧 | gameplay 上传 bytes |
|---:|---|---:|---:|---:|---:|---:|
| 0 | 裁切快照 | 59.753 | 20.916 | 20.489 | 1 | 494,927,872 |
| 1 | 增量版本 | 59.752 | 19.916 | 6.449 | 0 | 11,161,600 |
| 2 | 增量版本 | 59.752 | 18.807 | 7.277 | 0 | 11,161,600 |
| 3 | 裁切快照 | 59.659 | 20.205 | 26.074 | 1 | 494,927,872 |

- 上传：494,927,872 / 11,161,600 = **44.342×**；减少 **97.745%**。
- HUD 最大：20.489/26.074 → 6.449/7.277 ms；`>20 ms` 1→0。
- HUD 均值：约 0.996 → 0.816 ms（off 两轮 0.991/1.000，on 两轮 0.817/0.815）。
- FPS：59.753/59.659 vs 59.752/59.752，**持平**；p99 19.916/18.807 vs 20.916/20.205，方向混合，不能称稳定尾延迟改善。
- 独立呈现 p99 约 16.745/16.747 vs 16.782/16.779 ms；仍有约 18.2 ms 最大间隔。

#### 6.3 源码后端（`branch-lab.so`，ABBA 四轮）

| 次序 | 策略 | 应用 FPS | 应用 p99 ms | HUD 最大 ms | HUD >20 ms 帧 | gameplay 上传 bytes |
|---:|---|---:|---:|---:|---:|---:|
| 0 | 裁切快照 | 59.762 | 19.273 | 21.059 | 1 | 494,927,872 |
| 1 | 增量版本 | 59.747 | 19.686 | 6.990 | 0 | 11,161,600 |
| 2 | 增量版本 | 59.748 | 18.983 | 6.500 | 0 | 11,161,600 |
| 3 | 裁切快照 | 59.751 | 19.467 | 21.899 | 1 | 494,927,872 |

- 源码后端处理 PGR 均实际复用 **148 次**，无 busy/error 回退；chart 阶段均值 0.623–0.634 ms，两种策略相近。
- 源码版上传仍是 11,161,600 B；HUD 最大 6.99/6.50 vs 21.06/21.90；`>20 ms` 1→0。
- FPS 仍约 59.75；p99 19.686/18.983 vs 19.273/19.467，混合；不能把 HUD 尖峰消除等同所有 gameplay 尖峰消除。

#### 6.4 内部计数与上传口径

| 项 | 值 | 说明 |
|---|---:|---|
| `uploaded_bytes`（PGR，每轮） | 11,161,600 | 性能表口径；约 10.65 MiB |
| `pool.pgr.delta_bytes` | 15,323,136 | 池内部累计，含 warm-up/其他；不能当作 gameplay 上传 |
| 复用次数 | 148 / 轮 | 两轮均 148，无 busy/error |
| `busy_fallbacks` | 0 | 本窗口 4 槽足够 |
| `failed_fallbacks` | 0 | 无 GL 错误 |
| `blocking_waits` | 0 | 零 timeout，不阻塞 |
| 历史元数据 | 4096×8 B = 32 KiB/池 | 2048² / 32² = 4096 tile；不随曲长增长 |
| scratch | 1 MiB/池 | 加载期分配，gameplay 复用 |

`delta_bytes` 比 `uploaded_bytes` 大，是因为它包含 warm-up/其他上传；这不是矛盾。README 原文写“采样上传约10.65MiB，比裁切快照少44.3倍”，用的是 `uploaded_bytes`。

#### 6.5 代价

| 项 | 值 | 说明 |
|---|---:|---|
| 单池 GPU | 4×16 MiB = 64 MiB 名义 | 2048² RGBA8 |
| 两池 GPU | 128 MiB 名义 | PGR + default |
| CPU 镜像 | 2×16 MiB = 32 MiB | 两份完整 shadow |
| scratch | 2×1 MiB = 2 MiB | 可复用 |
| 物理内存 | **未测** | 名义容量不是 RSS/驱动实际 |
| 静态 default 池 | 只复用 3 次 | 明显不值得常驻完整池；后续应收紧 |

#### 6.6 原始记录（README 与 AGENTS 对应段落/表格）

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

`本地证据/low-quality-normal/README.md` “图集增量版本：少上传，并保留安全回退”原文：

> 注入后端 `8a9eaff6fe65100fcf44f188d8c9d1a272adad6d696bdd47bd9382cd6adf1c2d`。原字形栅格、glyph排布与逐段队列不变，HUD准备关闭；统一CPU镜像，4份完整2048²图集，每份有最后更新版本与旧read的fence。完成的图像补上传自该版本以来发生变化的区域；所有版本忙时新建快照，绝不覆盖仍在被读取的图像。
>
> 每32×32 tile记录最后变化的generation，历史存储与图集面积绑定，不随曲长增长。更新区域合并再从CPU镜像复制，多个落后的版本各自重建，不保存无限dirty journal。两个CPU测试包含1000轮任意重叠/边界更新、四份不同滞后版本，重建位图都与独立byte-array oracle相同。
>
> Android检查实际context为ES3+，三个sync函数入口非空，并试调用；缺失能力回退快照。poll为`glClientWaitSync(fence,0,0)`，不请求flush或阻塞；fence在旧macroquad队列flush之后插入。非Android当前研究原型回退快照，未宣称已为所有平台提供pool。
>
> 四个固定/动态、高/低画质末帧对照，以及强制缺能力、强制全部slot繁忙两种回退对照，共六组RGBA相同。每个正常处理都记录实际reused>0、delta_bytes>0；繁忙处理reused=0且busy_fallbacks>0；没有GL错误。不能外推完整字体/谱面、首次帧或跨设备。
>
> 真实时钟ABBA，120预热+1200采样帧，原型两种模式在同一后端中切换：
>
> | 次序 | 策略 | 应用FPS | 应用间隔p99 ms | HUD最大ms | HUD>20ms帧 | gameplay上传bytes |
> |---|---|---:|---:|---:|---:|---:|
> | 0 | 裁切快照 | 59.753 | 20.916 | 20.489 | 1 | 494,927,872 |
> | 1 | 增量版本 | 59.752 | 19.916 | 6.449 | 0 | 11,161,600 |
> | 2 | 增量版本 | 59.752 | 18.807 | 7.277 | 0 | 11,161,600 |
> | 3 | 裁切快照 | 59.659 | 20.205 | 26.074 | 1 | 494,927,872 |
>
> 处理PGR均复用148次，无busy/error回退，采样上传约10.65MiB，比裁切快照少44.3倍；这不是FPS快44倍。HUD均值约0.995→0.816ms。独立呈现p99约16.745/16.747ms，对照16.782/16.779ms；全程仍有约18.2ms呈现最大间隔。整体帧时间是应用/driver墙钟，未用CPU时间当GPU独占时间。
>
> 代价：当前加载PGR与default两个池，每池4×16MiB名义GPU图像，合计128MiB名义GPU容量，加两份CPU镜像32MiB及scratch。不是测得的物理内存；静态default只复用3次，明显不值得常驻完整池。后续收紧到实际动态字体、等待加载上传完成并做真实采样预热，测试从gameplay首帧到长曲/重入/字体变化。原始不重命名路径每轮仅约1.6MB，增量版本上传仍高于它，但避开了原路径当前设备的读写等待尖峰。
>
> 当前是研究后端，不是已经默认启用的生产优化。移入源码后的branch-lab构建/测试记录与旧注入后端记录分别保存；代码路径迁移不冒充额外设备验收。

同 README “当前源码参考实现验收记录”表格原文：

> 源码后端真实时钟ABBA，均关闭IR消费/HUD准备，不同配置/队列条件在同一后端中比较：
>
> | 次序 | 策略 | 应用FPS | 应用间隔p99 ms | HUD最大ms | HUD>20ms帧 | gameplay上传bytes |
> |---|---|---:|---:|---:|---:|---:|
> | 0 | 裁切快照 | 59.762 | 19.273 | 21.059 | 1 | 494,927,872 |
> | 1 | 增量版本 | 59.747 | 19.686 | 6.990 | 0 | 11,161,600 |
> | 2 | 增量版本 | 59.748 | 18.983 | 6.500 | 0 | 11,161,600 |
> | 3 | 裁切快照 | 59.751 | 19.467 | 21.899 | 1 | 494,927,872 |
>
> 源版处理PGR均实际复用148次，无busy/error回退。chart阶段均值0.623–0.634ms，在本后端两种图集策略之间相近；不拿它与另一个后端的不同cohort直接判定迁移是否引入某一函数的开销。整体应用p99未显示稳定改善，平均FPS相近；不能把HUD尖峰消除等同所有gameplay尖峰都消除。

`本地证据/AGENTS.md` 第 677 行原文（源码后端验收）：

> 当前已本地提交 `2cecb1042fc9d41d743f763050a5fb90195afb60`（加入渲染研究后端），15个源码/manifest文件；未push。源码版后端branch-lab SHA256 `51dd5d53cf6640fc47683663425ef7733ad43030a79e0258f4ee27eacce8d3ec`，正式源码hash与commit一致，包含render_research/render_atlas_pool/render_chart_ir特性。默认Windows cargo check通过，源码中的6个IR/cache和2个delta测试通过，Android arm64 API26研究构建通过。源码版10个quality作业组成6组有效对照，全部RGBA相同，actual reuse/forced fallback计数确认，结果pool-branch-quality-*。源码版ABBA同样每轮472→10.65MiB上传，HUD最大21.06/21.90→6.99/6.50ms、>20ms各1→0；均约59.75fps，整体p99未显示稳定改善，不夸大为所有尖峰消除。结果pool-branch-performance-*与旧pool-*不混池。

第 735 行原文（注入后端验收）：

> 增量图集池已实际验证（旧注入后端8a9eaff6，pool-*数据）：ES3当前context版本+三个函数入口+试探调用确认sync支持；glClientWaitSync flags=0/timeout=0只查询，四个slot仅在旧read fence完成后做精确区域增量。无能力/没有安全slot时发布新快照，无主动GPU等待。32px tile版本表有固定上界，1000轮重叠更新的4个滞后CPU图像重建相同；高/低画面四组及强制缺能力/全部busy回退两组RGBA相同。真实时钟ABBA每轮上传494,927,872→11,161,600 bytes（约44倍减少），PGR每轮复用148次，无busy/error回退，HUD最大20.49/26.07→6.45/7.28ms，HUD>20ms各1→0，平均FPS仍约59.75并非倍增。还不能称所有设备/字体/全曲无损或完全无尖峰。

#### 6.7 提交序列（git log 核对）

`2b57559d` 是“通用字体图集池”进入生产候选的基线；图集增量算法本身更早就在 `atlas_pool.rs`/`atlas_delta.rs` 中。按 `git log -- prpr/src/ui/atlas_pool.rs prpr/src/ui/atlas_delta.rs` 的时间顺序：

| 提交 | 日期 | 说明 | 与 P-06 的关系 |
|---|---|---|---|
| `2cecb104` | 2026-10-07 | 加入渲染研究后端 | 首次加入 `atlas_delta.rs`（140 行）与 `atlas_pool.rs`（405 行） |
| `39d82139` | 2026-10-07 | 加入字形更新对照 | `atlas_delta.rs` +3 行到 143 行；`atlas_pool.rs` +119 行，接入版本对照 |
| `721d9f87` | 2026-10-08 | 合并字形上传区域 | 新增 `atlas_coalesce.rs`；policy 3 的矩形合并 |
| `bc3f78a3` | 2026-10-08 | 研究字形双通道图集 | `atlas_channels.rs`；P-06 的 RGBA 路径不受影响 |
| `d523ee4d` | 2026-10-08 | 隔离字形探测影响 | 研究探针，不是 delta 主体 |
| `c3b1b976` | 2026-10-08 | 延迟发布字形图集 | `atlas_pool.rs` +297 行；deferred reserve/fence |
| `46c97226` | 2026-10-08 | 提前记录字形读取围栏 | `atlas_pool.rs` +30 行；read token/fence 刷新 |
| `2b57559d` | 2026-10-09 | 接入通用字体图集池 | 新增 `font_atlas_pool.rs` 653 行；同一 `Delta` 进入生产候选池 |
| `47f90b40` | 2026-10-11 | 细分字体图集发布耗时 | 诊断，不改 delta 语义 |
| `74ec68d5` | 2026-10-12 | 优化字形图集装箱 | FrozenText 集装箱，不是 P-06 主体 |

因此，P-06 的准确定位是：**`2cecb104` 引入、`39d82139` 完善、`2b57559d` 推广到通用字体池**；“2b57559d 之后的 atlas 系列”这个说法只对“生产候选池”成立，不对“delta 算法首次出现”成立。

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

| 组别 | 作业/对照 | 覆盖 | 结果 |
|---|---:|---|---|
| `pool-quality` | 10 个作业 / 6 组有效末帧 | 4 个固定/动态、高/低画质；强制缺能力；强制全部 slot 忙 | 六组 RGBA 相同；正常 `reused>0`/`delta_bytes>0`；忙 `reused=0`/`busy_fallbacks>0`；GL error 0 |
| 强制缺能力 | 其中 2 组 | `font_pool_mode=2` | 回退快照，RGBA 相同 |
| 强制全部 slot 忙 | 其中 2 组 | `font_pool_mode=3` | `reused=0`、`busy_fallbacks>0`，RGBA 相同 |
| CPU oracle | 2 个测试 | 1000 轮任意重叠/边界写；4 个不同滞后版本；空写/全图写 | 重建位图与独立 byte-array oracle 逐字节相同 |
| 计数 | 正常处理 | `reused>0`、`delta_bytes>0`；忙处理 `reused=0` | 实际执行，不是只编译 |
| GL 错误 | 全部 | `glGetError` | 0 |

#### 7.2 失败项与限制

- **只有有限画面**：四个固定/动态 + 高/低画质末帧，不是全曲逐帧、不是所有字体、不是首次帧。
- **FPS 没提升**：59.75 持平；收益是上传量/HUD 尖峰，不是平均帧率。
- **p99 混合**：注入后端 p99 19.916/18.807 vs 20.916/20.205，源码后端 19.686/18.983 vs 19.273/19.467；不能称稳定尾延迟改善。
- **呈现仍有 18.2 ms 最大间隔**：HUD 尖峰消除不等于所有呈现尖峰消除。
- **128 MiB 名义 GPU + 32 MiB CPU**：不是物理内存；static default 池只复用 3 次，成本明显不划算。
- **非 Android 回退**：研究原型在非 Android 走快照；不能称所有平台都有 pool。
- **没有跨设备/EGL loss/字体变化**：只有 Mali-T880；没有真实字体切换/重入/context 丢失。
- **默认关闭**：不是生产默认优化；不能把研究请求的收益写成“用户已获得”。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：局部（图集上传/HUD 尖峰），不是整帧。**

理由：

1. **上传量数量级下降**：494,927,872→11,161,600 B（44.34×，−97.745%），且源码后端也复现。
2. **HUD 尖峰明确下降**：20.489/26.074→6.449/7.277 ms；`>20 ms` 1→0；源码后端 21.059/21.899→6.990/6.500。
3. **FPS 持平**：59.75 左右，说明收益不是“帧率翻倍”，而是“少掉进 HUD 长帧”。
4. **质量证据**：6 组 RGBA 相同 + 1000 轮 CPU oracle；但只覆盖有限画面。
5. **代价真实**：128 MiB 名义 GPU + 32 MiB CPU；在静态字体池上不划算。
6. **为什么是正而不是无**：上传量/HUD 最大/`>20ms` 都同方向改善，且计数显示 `reused=148`、无 busy；不是只减少计数。
7. **为什么不是整帧**：FPS/p99 没有稳定改善；其他阶段不变。

**置信度：**

- 对“这个设备、这个 SO、这个窗口的上传量/HUD 尖峰下降”：**高**。
- 对“机制是 tile 增量上传 + 版本 fence 避开读写等待”：**中高**；上传计数和 HUD 同时改善，但没有 GPU 独占计时。
- 对“FPS/整帧/其他 GPU/所有字体”：**低**。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`2cecb104`/`39d82139` 加入 delta/pool；`2b57559d` 加入通用字体池 |
| 参考实现是否默认启用 | **否**。`font_atlas_pool` 默认编入，但 `LAB_PRODUCTION_POOL=false`、`font_production_pool_mode=0`；研究池 `font_pool_mode=0` |
| 参考实现是否有消费者 | 生产候选 `font_atlas_pool.rs` 有接线，但默认关闭；研究 `atlas_pool.rs` 只在研究构建 |
| 是否进 main | **否**。main 没有 `atlas_delta.rs`/`atlas_pool.rs`/`font_atlas_pool.rs` |
| 是否有用户开关 | 无 Config/Settings；只有研究请求 |
| 是否低画质限定 | 否；图集池与画质无关，但测量用 `sampleCount=1` |
| 是否有独立验收 | 没有记录独立验收人；本文档是事后审计 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **成本高**：128 MiB 名义 GPU + 32 MiB CPU；static default 池只复用 3 次，不值得常驻。
2. **收益是 HUD 尖峰，不是 FPS**：平均 FPS 持平，p99 混合；主项目没有足够理由为局部尖峰承担两池常驻。
3. **只验证有限画面**：没有全曲/首次帧/字体变化/重入/跨设备。
4. **非 Android 回退**：没有跨平台 pool。
5. **参考实现未 push/未合并**：保留为已验证候选，等待“只保留实际动态字体池 + 加载期预热 + 真实内存测量”的收紧版本。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| 覆盖仍在读取的图像 | 会改变画面/触发驱动同步 | fence；deferred 模式另用 read token；忙时整图快照；不覆盖 |
| fence 语义 | `glClientWaitSync` 只表示提交完成，不是“所有读取完成” | fence 在旧队列 flush 后插入；deferred 模式另有 read token；未证明所有驱动 |
| generation 溢出 | `u64` 溢出会 panic | `checked_add`；实际曲长不可能达到 |
| 写矩形错误 | 漏 tile 会导致旧内容 | `advance` 断言边界；`since` 全覆盖；1000 轮 oracle |
| 矩形合并错误 | 重叠会重复混合/漏 | 水平 run + 同 x/同 w 垂直合并；不重叠 |
| 版本不足 | 4 槽全忙 | busy fallback 整图；不等待 |
| 内存误报 | 名义容量不是物理 | `report` 明确 scope；未测 RSS |
| 静态池浪费 | default 只复用 3 次 | 后续只保留动态字体池 |
| 非 Android | 快照回退 | 明确不声称 pool |
| 默认关闭 | 用户拿不到收益 | 若落地需产品决策，不是技术默认 |

#### 10.3 未验证项

- 全曲逐帧、首次帧、字体切换/重入、context 丢失/恢复。
- 其他 GPU（Adreno/其他 Mali）、其他分辨率/图集尺寸。
- 物理 GPU 内存/RSS/驱动内部副本。
- fence 在所有驱动上的完成语义；`glClientWaitSync` 零 timeout 的假阴性/假阳性。
- `Damage`/`coalesce` policy 2/3 的独立收益（P-06 主线是 policy 0 + Delta）。
- 双通道 RGBA8/RG8 路径与 delta 的组合（P-06 只评价 RGBA 路径）。
- 生产 UI 真实点击/开关（没有开关）。

### 11. 落地步骤（可执行清单）

- [ ] 只保留实际动态字体池，去掉 static default 的完整 4 槽常驻；先测真实复用次数。
- [ ] 在加载阶段完成 4 张纹理上传 + fence，并等待首次采样真正完成；不要只“创建了纹理”。
- [ ] 用真实 RSS/驱动查询测物理内存，不要用 128 MiB 名义值当实际占用。
- [ ] 跑完整曲目 + 字体切换 + 重入 + EGL loss；确认 fence/read token/clear 生命周期。
- [ ] 在至少两种 GPU 上重复 ABBA；确认 HUD 尖峰收益不是 Mali-T880 特有。
- [ ] 评估 32×32 tile 大小、4 槽数量、矩形合并阈值；用同一 SO 扫描。
- [ ] 把 `blocking_waits=0`、`busy_fallbacks`、`reused`、`delta_bytes` 纳入生产诊断；没有诊断就不要默认开启。
- [ ] 若要进 main，先决定产品开关/默认值；当前“默认编入、默认关闭”应保持到成本收紧完成。
- [ ] 更新 README/AGENTS，明确 44.3× 是上传量不是 FPS，128 MiB 是名义不是物理。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `be4e1305823bfebdad1853d7b333e0f79c3ead79`（2026-10-24） |
| 首次 delta/pool | `2cecb1042fc9d41d743f763050a5fb90195afb60`（2026-10-07 01:57:11 +0800） |
| 更新对照 | `39d8213959ae7e75ab80cd24f2b9793cbf784119`（2026-10-07 23:40:48 +0800） |
| 合并上传区域 | `721d9f872d13a1e3097dd2b815e62a07b67ce006`（2026-10-08 00:18:00 +0800） |
| 延迟发布 | `c3b1b976`（2026-10-08） |
| 读取围栏 | `46c97226b14343be0de6b33579809d6bd7dc2a94`（2026-10-08 23:43:24 +0800） |
| 通用字体池 | `2b57559dd7cb97479c1f4c8b019ca03f1788c796`（2026-10-09 02:08:04 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/ui/atlas_delta.rs`（143 行）、`prpr/src/ui/atlas_pool.rs`（880 行）、`prpr/src/ui/font_atlas_pool.rs`（1101 行）、`prpr/src/ui/text.rs`、`prpr/Cargo.toml`、`phira/Cargo.toml`、`phira/src/render_research.rs` |
| 注入后端 | `pool-lab.so` SHA-256 `8a9eaff6fe65100fcf44f188d8c9d1a272adad6d696bdd47bd9382cd6adf1c2d` |
| 源码后端 | `branch-lab.so` SHA-256 `51dd5d53cf6640fc47683663425ef7733ad43030a79e0258f4ee27eacce8d3ec` |
| 性能数据 | `本地证据/low-quality-normal/pool-performance-results.json`、`pool-branch-performance-results.json` |
| 质量数据 | `本地证据/low-quality-normal/pool-quality-results.json`、`pool-branch-quality-results.json` |
| 设备 | EDI-AL10 / Mali-T880 / Android 8 / 2560×1440 |
| 谱面 | `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c.zip` |
| 说明 | 正文已包含完整 `Delta`/sync/warm 代码、两套 ABBA 表、成本与风险；上述路径只用于追溯 |

---

## P-07 Chart JSON 分片加载

> 主线实施状态：☐ 未实施（main）
> 参考实现状态：已落地，未合入 main
> 参考实现日期：2026-10-06　参考实现提交：`4f046544`（研究保存分片加载与真机验证）　参考实现作者：RENaa_FD　参考实现验收：未记录独立验收人（本文档为事后审计）

### 1. 一句话结论

P-07 不是把 JSON 解析拆到多个线程，也不是把 JSON 提交给 GPU；它把**已经由后台 worker 解码完成的 `PgrChart` 模型 → `Chart` 的转换**按“每条判定线”切片，并在 2 ms/边界处让本地 loading future 挂起一次，让 LoadingScene 有机会渲染/提交下一帧。研究后端 `chart-slices-ready.so` 的 8 项 30 s 真实 UI ABBA 中，Loading 阶段 `render` 最大耗时从 **51.772 ms 降到 39.850 ms**、从 **44.594 ms 降到 39.845 ms**（另一首为 58.981/57.871→50.122/49.837 ms），但全局 FPS 没有改善（59.545→59.523、59.379→59.507），p99 仅有小幅且不稳定的变化（19.133→18.366、19.046→18.647），max 没有一致改善（64.963→67.352、67.389→67.063）。它是**局部 loading 改善**，不能称为整帧正优化；总 CPU 工作没有减少，只是把一次长同步调用摊到多帧。

### 2. 问题与瓶颈（为什么原路径慢）

既有生产路径（main）已经把 PGR JSON 的 `serde_json::from_str` 放到 CPU worker：

```rust
pub async fn parse_phigros_loading(bytes: Vec<u8>, extra: ChartExtra) -> Result<(Chart, Vec<u8>)> {
    let (pgr, bytes) = crate::loading_work::run(move || {
        let pgr: PgrChart = serde_json::from_slice(super::json_bytes(&bytes))?;
        Ok((pgr, bytes))
    })
    .await
    .with_context(|| ptl!("json-parse-failed"))?;
    Ok((convert_phigros(pgr, extra)?, bytes))
}
```

JSON 解码已经不在渲染线程，但 `convert_phigros(pgr, extra)` 仍在 loading future 的同一 poll 里同步执行：

1. `phigros_max_time(&pgr)` 遍历所有线的全部 note，求最大时间；
2. 对每条 `judge_line_list` 调 `parse_judge_line`，构造 `JudgeLine`、`Note`、`Anim`、`CtrlObject` 等；
3. `finish_phigros` 调 `process_lines` 做跨线 hint/parent 处理，再 `Chart::new`、`parse_block_areas`。

这一步在 LoadingScene 的某一帧内一次跑完，造成 `LoadingScene` 的 `render` 长帧。30 s 真实 UI 观测的 Loading 段：控制组 `render max` 中位数 48.183 ms（Exoplanetary）/58.426 ms（Petrichor），单轮最高 51.772/58.981 ms；`update max` 46.755/54.627 ms。用户看到的是 Loading 画面卡一下，而不是 gameplay 帧率问题。

P-07 的目标不是减少总转换工作，而是**把一次长同步调用切成多个可渲染的片段**，让 LoadingScene 在片段之间有机会提交画面。它不改变解析顺序、数值计算、资源归属或最终 Chart。

### 3. 优化思路（原理、不变量、为什么不会改变画面）

#### 3.1 先澄清“JSON 分片”的实际含义

条目名沿用任务名“Chart JSON 分片加载”，但实际实现分成两段：

1. **JSON 解码**：`serde_json::from_str::<PgrChart>` 在 `run_with_proof` 的 `tokio::spawn_blocking` worker 中一次完成；只有 owned `PgrChart`（Send）跨线程，`Chart`/GL/font/extra 不上 worker。
2. **模型转换**：`convert_phigros_loading` 在渲染线程执行，按判定线切片；每个 `checkpoint` 最多挂起一次，让本地 future 返回 `Pending`，LoadingScene 继续 poll/渲染；下一次 poll 从上次位置恢复。

所以它不是“JSON parser 分片”，也不是“多线程并行解析多条线”；它是**单线程、顺序不变、协作式让出**。把它写成“JSON 并行加载”是错的。

#### 3.2 `FrameBudget` 的协作式让出

`FrameBudget` 只有 `enabled` 与 `began`：

- `checkpoint(false)`：如果距上次让出不足 2 ms，直接返回；否则挂起一次。
- `checkpoint(true)`：边界处无条件挂起一次。
- 挂起实现是 `poll_fn`：第一次 poll 返回 `Pending` 并 `wake_by_ref()`；第二次 poll 返回 `Ready`。因此每个 checkpoint 最多让出一次，不会忙等、不会丢工作。
- `began` 在恢复后重置为 `Instant::now()`。

这个设计的关键是：**不让出在 GL 命令或 profiler scope 内部**；只在“一条线解析完”“max_time 算完”“finish 前后”这些安全边界让出。让出期间没有资源跨线程，`Chart`/`Extra`/GL 仍留在渲染线程。

#### 3.3 不变量

- **顺序不变**：仍然按 `judge_line_list` 的原始顺序逐条 `parse_judge_line`；`finish_phigros` 仍在所有线之后调用。
- **数值不变**：`phigros_max_time`、`parse_judge_line`、`process_lines`、`parse_block_areas` 都是原函数；切片只改变“何时返回 Pending”，不改变参数/算术/迭代顺序。
- **资源归属不变**：`Chart`、`ChartExtra`、GL、font 不上 worker；worker 只拿到 owned `PgrChart`。
- **取消安全**：如果 loading future 被取消，已解析的线随 future 丢弃；不会留下半成品 Chart 或后台线程继续写共享状态。
- **最终 Chart 相同**：质量对照的 6 组末帧 RGBA 和 96 项 CPU 输出全部相同。

#### 3.4 为什么画面不变

Loading 画面在切片之间正常渲染；切片恢复后继续构造同一个 Chart。最终 `Chart` 的线顺序、note 数据、block areas、offset、BPM 列表都与原路径相同。切片不改变任何可见状态，只改变“这一帧做了多少工作”。因此末帧 RGBA 相同是预期结果；质量对照用于排除“切片顺序/边界改变了某个数值”的实现错误。

### 4. 具体实现

#### 4.1 文件与符号清单（行数、函数/结构）

| 文件（参考实现，head `3670c2aa`） | 行数 | 关键符号 | 职责 |
|---|---:|---|---|
| `prpr/src/parse/pgr.rs` | 432 | `parse_phigros_loading`（396-414）、`convert_phigros`（416-428）、`phigros_max_time`（430-448）、`finish_phigros`（450-457）、`convert_phigros_loading`（459-487） | JSON worker + 模型转换切片 |
| `prpr/src/loading_schedule.rs` | 66 | `FrameBudget`（6-26）、2 个测试（28-66） | 2 ms/边界 checkpoint |
| `prpr/src/loading_work.rs` | 213 | `CHART_SLICES`（17）、`research_chart_slices_configure`（31）、`chart_slices_enabled`（43-48）、`run_with_proof`（90-95）、`run_selected_with_proof`（99-145） | 研究开关、worker 选择、CPU 输出 proof |
| `prpr/src/scene/game.rs` | — | `GameScene::load_chart`（516-535），PGR 分支调 `parse_phigros_loading`（529） | 真实 loading 入口 |
| `phira/src/research.rs` | — | `chart_loading_slices`（95）、默认 false（158）、校验（341-342）、配置（369） | 研究请求；要求 `async_chart_json` |
| `research/suites/chart-slices-loading.json` | — | `control`/`slices` 两个 backend，`chart_loading_slices` false/true | 8 项 30 s ABBA |

#### 4.2 数据结构与算法（逐步说明）

1. **请求校验**：`chart_loading_slices` 必须与 `async_chart_json` 同时为 true；后者保证 `PgrChart` 已经在 worker 中解码，`convert_phigros_loading` 只处理 owned 模型。`chart_loading_slices` 默认 false。
2. **`parse_phigros_loading`**：
   - `validate = validate_outputs()`（研究可选 CPU 输出 proof）；
   - `run_with_proof` 的 `phigros_json_decode` 闭包内执行 `serde_json::from_str::<PgrChart>`，并按需生成 typed_model proof，然后返回 `((pgr, bytes), proof)`；
   - 返回 `(pgr, bytes)`；
   - 若 `chart_slices_enabled()`，走 `convert_phigros_loading(pgr, extra).await`；否则走原 `convert_phigros(pgr, extra)`。
3. **`convert_phigros_loading`**：
   - `FrameBudget::new(true)`；
   - 计算 `max_time`（原 `phigros_max_time`），`checkpoint(false)`；
   - `Vec::with_capacity(lines)`；逐条 `parse_judge_line(line, max_time, format_version)`，push，`checkpoint(false)`；
   - 所有线完成后 `checkpoint(true)`，把跨线 hint pass 与 block/shader 准备分到不同的 loading poll；
   - `finish_phigros(offset, block_area_list, lines, extra)`；
   - 再 `checkpoint(true)`，返回 Chart。
4. **`FrameBudget::checkpoint`**：
   - `!enabled` 或（非边界且 `<2ms`）直接返回；
   - `poll_fn` 第一次 `Pending` + `wake_by_ref()`，第二次 `Ready`；
   - `began = Instant::now()`。
5. **worker 选择**：`run_selected_with_proof` 在非 wasm 且 `allow_worker && asynchronous()` 时，若 `tokio::runtime::Handle::try_current()` 成功，用 `spawn_blocking`；否则 inline。运行时能力决定路径，不按 CPU 型号。
6. **取消**：loading future 被 drop 时，`spawn_blocking` 的 JSON 解析可能继续到完成，但其结果不会被消费；模型转换在渲染线程，drop 即停止。不会出现后台线程持有 `Chart`/GL 的跨线程问题。

#### 4.3 核心代码（完整函数/结构，禁止省略号）

**`prpr/src/parse/pgr.rs:396-487`（worker + 切片，完整）：**

```rust
pub async fn parse_phigros_loading(bytes: Vec<u8>, extra: ChartExtra) -> Result<(Chart, Vec<u8>)> {
    use sha2::{Digest, Sha256};
    let validate = crate::loading_work::validate_outputs();
    let (pgr, bytes) = crate::loading_work::run_with_proof("phigros_json_decode", move || {
        let pgr: PgrChart = serde_json::from_str(&String::from_utf8_lossy(&bytes))?;
        let proof = if validate {
            let canonical = serde_json::to_vec(&pgr)?;
            Some(serde_json::json!({"typed_model_sha256":format!("{:x}",Sha256::digest(&canonical)),
                "typed_model_bytes":canonical.len(),"lines":pgr.judge_line_list.len(),
                "notes":pgr.judge_line_list.iter().map(|line|line.notes_above.len()+line.notes_below.len()).sum::<usize>(),
                "block_areas":pgr.block_area_list.len()}))
        } else { None };
        Ok(((pgr, bytes), proof))
    }).await.with_context(|| ptl!("json-parse-failed"))?;
    let chart = if crate::loading_work::chart_slices_enabled() {
        convert_phigros_loading(pgr, extra).await?
    } else { convert_phigros(pgr, extra)? };
    Ok((chart, bytes))
}

fn convert_phigros(pgr: PgrChart, extra: ChartExtra) -> Result<Chart> {
    #[cfg(feature = "render_research")]
    let _span = crate::research_spikes::span("phigros_model_convert");
    let format_version = pgr.format_version;
    let max_time = phigros_max_time(&pgr);
    let lines = pgr
        .judge_line_list
        .into_iter()
        .enumerate()
        .map(|(id, pgr)| parse_judge_line(pgr, max_time, format_version).with_context(|| ptl!("judge-line-location", "jlid" => id)))
        .collect::<Result<Vec<_>>>()?;
    Ok(finish_phigros(pgr.offset, pgr.block_area_list, lines, extra))
}

fn phigros_max_time(pgr: &PgrChart) -> f64 {
    *pgr
        .judge_line_list
        .iter()
        .map(|line| {
            (line
                .notes_above
                .iter()
                .chain(line.notes_below.iter())
                .map(|note| note.time.not_nan())
                .max()
                .unwrap_or_default()
                * (60. / line.bpm / 32.))
                .not_nan()
        })
        .max()
        .unwrap_or_default()
        + 1.
}

fn finish_phigros(offset: f32, blocks: Vec<PgrBlockArea>, mut lines: Vec<JudgeLine>, extra: ChartExtra) -> Chart {
    process_lines(&mut lines);
    let mut chart = Chart::new(offset, lines, BpmList::default(), ChartSettings::default(), extra, HashMap::new());
    // Block-area times are authored in seconds (the official runtime compares
    // them directly against the song time), so no beat conversion is applied.
    chart.block_areas = parse_block_areas(blocks);
    chart
}

async fn convert_phigros_loading(pgr: PgrChart, extra: ChartExtra) -> Result<Chart> {
    let mut budget = crate::loading_schedule::FrameBudget::new(true);
    let max_time = {
        #[cfg(feature = "render_research")]
        let _span = crate::research_spikes::span("phigros_model_bounds_slice");
        phigros_max_time(&pgr)
    };
    budget.checkpoint(false).await;
    let mut lines = Vec::with_capacity(pgr.judge_line_list.len());
    for (id, line) in pgr.judge_line_list.into_iter().enumerate() {
        let line = {
            #[cfg(feature = "render_research")]
            let _span = crate::research_spikes::span("phigros_model_line_slice");
            parse_judge_line(line, max_time, pgr.format_version).with_context(|| ptl!("judge-line-location", "jlid" => id))?
        };
        lines.push(line);
        budget.checkpoint(false).await;
    }
    // Keep the cross-line hint pass and block/shader preparation on separate
    // loading polls, without changing the order or any numeric calculations.
    budget.checkpoint(true).await;
    let chart = {
        #[cfg(feature = "render_research")]
        let _span = crate::research_spikes::span("phigros_model_finish_slice");
        finish_phigros(pgr.offset, pgr.block_area_list, lines, extra)
    };
    budget.checkpoint(true).await;
    Ok(chart)
}
```

**`prpr/src/loading_schedule.rs` 全文（66 行）：**

```rust
//! Cooperative checkpoints for a local future polled by the loading scene.
//! No resource crosses threads; never yield inside a GL command or a profiler
//! scope. A checkpoint suspends once, so the normal scene gets to render.
use std::{future::poll_fn, task::Poll, time::{Duration, Instant}};

pub struct FrameBudget {
    enabled: bool,
    began: Instant,
}

impl FrameBudget {
    pub fn new(enabled: bool) -> Self { Self { enabled, began: Instant::now() } }

    pub async fn checkpoint(&mut self, boundary: bool) {
        if !self.enabled || (!boundary && self.began.elapsed() < Duration::from_millis(2)) { return; }
        let mut suspended = false;
        poll_fn(move |cx| {
            if suspended { Poll::Ready(()) } else {
                suspended = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }).await;
        self.began = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, task::{Context, Waker}};

    #[test]
    fn enabled_boundary_preserves_work_across_a_render_opportunity() {
        let mut state = 0;
        let mut future = Box::pin(async {
            let mut budget = FrameBudget::new(true);
            state = 1;
            budget.checkpoint(true).await;
            state = 2;
            budget.checkpoint(true).await;
            state = 3;
        });
        let mut cx = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert!(future.as_mut().poll(&mut cx).is_ready());
        drop(future);
        assert_eq!(state, 3);
    }

    #[test]
    fn disabled_schedule_is_synchronous_and_cancellation_does_not_finish_work() {
        let mut budget = FrameBudget::new(false);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Box::pin(budget.checkpoint(true)).as_mut().poll(&mut cx).is_ready());
        let mut completed = false;
        let mut future = Box::pin(async {
            FrameBudget::new(true).checkpoint(true).await;
            completed = true;
        });
        assert!(future.as_mut().poll(&mut cx).is_pending());
        drop(future);
        assert!(!completed);
    }
}
```

**worker 选择 `prpr/src/loading_work.rs:90-145`（完整）：**

```rust
pub async fn run_with_proof<T: Send + 'static>(
    label: &str,
    work: impl FnOnce() -> Result<(T, Option<serde_json::Value>)> + Send + 'static,
) -> Result<T> {
    run_selected_with_proof(label, true, work).await
}

/// An isolated candidate can retain inline execution while recording exactly
/// the same CPU-output proof. Runtime absence/wasm still select inline work.
pub async fn run_selected_with_proof<T: Send + 'static>(
    label: &str,
    allow_worker: bool,
    work: impl FnOnce() -> Result<(T, Option<serde_json::Value>)> + Send + 'static,
) -> Result<T> {
    let began = Instant::now();
    let origin = std::thread::current().id();
    let measured = move || {
        let compute = Instant::now();
        let result = work();
        (result, compute.elapsed().as_secs_f64() * 1000., std::thread::current().id() != origin)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (result, compute_ms, off_thread) = if allow_worker && asynchronous() {
        // Runtime availability, not a model/CPU string, selects the path.
        // wasm and callers without a runtime retain the same inline work.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let mut worker = Box::pin(runtime.spawn_blocking(measured));
            std::future::poll_fn(move |_| {
                let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                worker.as_mut().poll(&mut context)
            })
            .await
            .context("CPU loading worker failed")?
        } else {
            measured()
        }
    } else {
        measured()
    };
    #[cfg(target_arch = "wasm32")]
    let (result, compute_ms, off_thread) = { let _ = allow_worker; measured() };
    #[cfg(feature = "render_research")]
    {
        let proof = result.as_ref().ok().and_then(|(_, proof)| proof.clone());
        RECORDS.with(|records| {
            records.borrow_mut().push(serde_json::json!({
            "label":label,"began_seconds":STARTED.with(|it|it.get().map(|start|began.duration_since(start).as_secs_f64())),
            "compute_ms":compute_ms,"await_wall_ms":began.elapsed().as_secs_f64()*1000.,
            "off_thread":off_thread,"ok":result.is_ok(),"proof":proof,
            "timing":"CPU work wall time; optional output hashing included; wait spans rendered frames"}))
        });
    }
    #[cfg(not(feature = "render_research"))]
    let _ = (label, began, compute_ms, off_thread);
    result.map(|(value, _)| value)
}
```

#### 4.4 调用链与生命周期

调用链：

```text
GameScene::load_chart
  -> ChartFormat::Pgr
    -> loading_work::chart_json_enabled()（生产默认 true）
      -> parse::parse_phigros_loading(bytes, extra)
        -> run_with_proof("phigros_json_decode")
             -> tokio::spawn_blocking serde_json::from_str::<PgrChart>
             -> 可选 typed_model proof
        -> chart_slices_enabled()?
             true  -> convert_phigros_loading(pgr, extra).await
                        -> FrameBudget::checkpoint(false/true)  // 让出一帧
                        -> parse_judge_line per line
                        -> finish_phigros
             false -> convert_phigros(pgr, extra)             // 原同步路径
```

生命周期：

- `FrameBudget` 是 stack 上的局部变量，随 future 存活；没有全局状态。
- 每个 `checkpoint` 只挂起一次；`began` 在恢复后重置。没有 busy loop，没有定时器线程。
- JSON worker 由 `tokio` runtime 管理；`spawn_blocking` 的结果通过 `poll_fn` 驱动，不要求宏 quad 外层 executor 被唤醒。
- 转换过程中的 `JudgeLine`/`Note`/`Anim` 都是普通 owned 值；future 被取消时随栈回收。
- 没有新的 GPU 资源；Chart/GL 仍在渲染线程。
- 研究关闭时 `chart_slices_enabled()` 返回 false（非研究构建直接 false），走原 `convert_phigros`；生产 main 没有这个代码路径。

### 5. 开关、默认值与构建特性（feature、Config、环境变量、默认值）

| 项 | 值 | 说明 |
|---|---|---|
| 研究请求 | `chart_loading_slices`，默认 `false` | `research.rs:95/158` |
| 依赖 | 必须 `async_chart_json=true` | `research.rs:341-342`；否则拒绝 |
| 研究配置 | `research_chart_slices_configure`（`loading_work.rs:31`） | 写 thread-local `CHART_SLICES` |
| 生产默认 | 无 | main 没有 `chart_loading_slices`/`convert_phigros_loading` |
| 既有 JSON worker | `chart_json_enabled()` 生产默认 true | 这是 P-07 的前置，不是 P-07 本身 |
| 用户开关 | 无 | 没有 Config/Settings |
| 环境变量 | 无 | 只走研究 job |
| Feature | 无 P-07 专属 feature | `render_research` 下才有请求字段/配置 |
| 默认是否实际启用 | **否** | 研究 job 显式 `chart_loading_slices=true` 才走切片 |
| 时间片 | 2 ms / 边界 | `FrameBudget::checkpoint` |
| 线程 | JSON worker；转换在渲染线程 | 没有 Chart/GL 跨线程 |

这里必须避免两个误读：

- **“JSON 解码已经生产默认异步”** 是真的，但那是既有 `async_chart_json`，不是 P-07 的切片；P-07 切片只在研究构建。
- **“切片默认开启”** 是假的；`chart_loading_slices` 默认 false，main 没有这个字段。

### 6. 测量方法与完整数据（完整表格）

#### 6.1 测量口径

- 设备：MT6769V/CU / Android 10 / Mali-G52 MC2 / OpenGL ES 3.2 / 1920×1200 / 60 Hz。
- 谱面：ExoplanetaryMirage（`fdf8408211260bf59eebf68cd6e484370cbdc89520a6556b80255f24de49def1`）和 Petrichor；均从 Home → Library → Song → Loading → Gameplay 的真实 UI 流程。
- 采样：每轮 30 s、`sample_frames=3000`、`warmup_frames=4`、`sampleCount=2`、particle/effects 原设置、`production_rendering=true`、`native_frame_timing=true`。
- 同 SO：`chart-slices-ready.so` SHA-256 `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5`；`source_revision` 字段为 `b25272310fa41122c203b9db7c037886bc24f4bf`。该 SO 同时用于 control 与 slices，只切换 `chart_loading_slices`。
- 交错：8 项 ABBA（2 场景 × 2 后端 × 2 轮），不是 8 个独立条件。
- 统计：全局指标保留场景转移帧；每轮分位数不混池；下表“中位数”是 2 轮指标的中位数，不是 pooled-frame 分位数。
- Loading 段：`LoadingScene` 的 `update`/`render` 最大耗时；全局 `frame_interval` 包含 Home/Library/Song/Loading/Gameplay/Ending 转移。
- 质量：6 组末帧 RGBA（`chart-slices-quality-confirmed`）+ 96 项 CPU 输出（16 labels × 2 场景 × 3 次比较）。

#### 6.2 八轮完整数据

| 轮次 | 场景 | 后端 | 全局 FPS | 全局 p99 ms | 全局 max ms | Loading update max ms | Loading render max ms |
|---:|---|---|---:|---:|---:|---:|---:|
| 0 | Exoplanetary | control | 59.5127 | 20.5445 | 67.8277 | 51.0887 | 51.7719 |
| 1 | Exoplanetary | control | 59.5764 | 17.7211 | 62.0984 | 42.4205 | 44.5941 |
| 2 | Petrichor | control | 59.3752 | 19.3132 | 68.5248 | 53.8487 | 58.9805 |
| 3 | Petrichor | control | 59.3834 | 18.7781 | 66.2538 | 55.4050 | 57.8712 |
| 4 | Exoplanetary | slices | 59.4737 | 18.6556 | 68.0936 | 51.8119 | **39.8505** |
| 5 | Exoplanetary | slices | 59.5724 | 18.0773 | 66.6095 | 42.0378 | **39.8449** |
| 6 | Petrichor | slices | 59.4402 | 18.6514 | 69.3867 | 58.6815 | **50.1222** |
| 7 | Petrichor | slices | 59.5735 | 18.6416 | 64.7385 | 45.4028 | **49.8366** |

中位数：

| 场景 | 后端 | FPS | p95 ms | p99 ms | max ms | Loading update max ms | Loading render max ms |
|---|---|---:|---:|---:|---:|---:|---:|
| Exoplanetary | control | 59.5445 | 17.2429 | 19.1328 | 64.9630 | 46.7546 | 48.1830 |
| Exoplanetary | slices | 59.5230 | 17.3124 | 18.3665 | 67.3515 | 46.9248 | **39.8477** |
| Petrichor | control | 59.3793 | 17.4375 | 19.0457 | 67.3893 | 54.6268 | 58.4258 |
| Petrichor | slices | 59.5068 | 17.2264 | 18.6465 | 67.0626 | 52.0422 | **49.9794** |

解读：

- **Loading render max 明确下降**：Exoplanetary 51.772→39.850（−23.0%）、44.594→39.845（−10.7%）；Petrichor 58.981→50.122（−15.0%）、57.871→49.837（−13.9%）。中位数 48.183→39.848（−17.3%）、58.426→49.979（−14.5%）。
- **全局 FPS 没有改善**：Exoplanetary 59.545→59.523（略低），Petrichor 59.379→59.507（略高）；都在 60 上限附近。
- **全局 p99 方向混合**：Exoplanetary 19.133→18.366（略好），Petrichor 19.046→18.647（略好）；但这是 2 轮中位数，不能称稳定尾延迟改善。
- **全局 max 没有改善**：Exoplanetary 64.963→67.352（更差），Petrichor 67.389→67.063（略好）；场景转移长帧仍在。
- **Loading update max 没有改善**：Exoplanetary 46.755→46.925（略差），Petrichor 54.627→52.042（略好）；切片只影响 render，不减少 update 的其他工作。
- **Loading render 仍有 40–50 ms**：不是“消除尖峰”，只是把最坏 51.8/59.0 降到 39.8/50.0；Loading 仍可能卡一帧。

#### 6.3 30 s stage observation（独立单轮，不是 ABBA）

`本地证据/chart-slices-stage-observation` 使用同一 ready SO，`spike_stage_timing=true`，单轮 Petrichor：

| 指标 | control | slices | 变化 |
|---|---:|---:|---:|
| 全局 FPS | 59.451 | 59.621 | +0.17 |
| 全局 p99 ms | 18.830 | 18.469 | −0.361 |
| 全局 max ms | 65.707 | 62.897 | −2.810 |
| Loading 段 p99 ms | 42.908 | 36.930 | −5.978 |
| Loading 段 max ms | 54.085 | 50.550 | −3.535 |
| `render` max ms | 57.717 | 58.881 | +1.164 |
| `update` max ms | 50.835 | 43.576 | −7.259 |

这个 observer 只有单轮，不能当 ABBA 收益；它说明 Loading 段 p99/max 下降，但全局 `render` max 反而略增，且 `update` 的改善可能来自其他同步调用拆分（model/block/audio），不能全归 P-07。

#### 6.4 质量与输出对照

| 组别 | 作业/对照 | 覆盖 | 结果 |
|---|---:|---|---|
| `chart-slices-quality-confirmed` | 8 个作业 / 6 组末帧 | Happy ending、Petrichor，control/slices 各两轮 | 1920×1200 RGBA **逐像素相同**，`different_channels=0`，`all_pixels_equal=true` |
| `loading-output-equivalence.json` | 6 组比较 / 96 项输出 | 16 labels × 2 场景 × 3 次；图像、全 PCM、typed-model | `all_outputs_equal=true`，`different_labels=[]` |
| 单元测试 | 2 个 | 边界跨 render opportunity 保留工作；disabled 同步/取消不完成 | 通过 |
| 研究校验 | — | `chart_loading_slices` 必须依赖 `async_chart_json` | 请求校验通过 |

`scope` 原文：

> Both paths use owned JSON worker; control/sliced engine conversion runs on original thread. Full PCM/image/typed-model proofs and final-frame pixels are distinct, not whole-chart/whole-UI coverage.

#### 6.5 原始记录（`previous-AGENTS.md` 对应段落）

历史引文中的分支标识已归一化为“参考实现”，其余原文未改。以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。

当前 `本地证据/AGENTS.md` 没有模型分帧/`chart-slices` 的独立段落；以下引自同目录历史快照 `本地证据/branch-cleanup-20261006/previous-AGENTS.md`，不伪造当前段落。

`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 33 行原文：

> 模型分帧最终8项加载ABBA/8项质量完成：6项严格RGBA及96项图像/全PCM/typed-model输出全部相同。Exo控制/分帧全30秒最大间隔中位数64.963/67.352ms、p99 19.133/18.366ms、持续59.545/59.523fps；Petrichor67.389/67.063ms、p99 19.046/18.647ms、59.379/59.507fps。Loading最大update中位数Exo46.755/46.925ms、P54.627/52.042ms，无可靠最大尖峰改善，ordinary仍false，不将较好p99或共享算法当完整目标完成。下一步用同ready so的独立30秒stage观察分解剩余Loading同步调用；observer数据不混入性能ABBA。

同文件第 36 行原文：

> 字库裁剪版本独立探针完成：原始/集中上传/裁剪不可变版本/同名裁剪重定义/同尺寸orphan同步completed p50为133.750/11.194/28.178/36.222/165.212ms，4项严格RGBA全同。裁剪版本同时62914560B（60MiB）纹理；GL名拥有量不代表driver所有in-flight存储。该fixture初始化整个atlas，crop改变UV尺度，真实缓存淘汰/未初始化texel/custom材质/跨设备精度未证明，尚未接入root。初次crop错误启动旧so已人工force-stop研究包、保留排除；不是driver崩溃证据。模型分帧ready后端SHA9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5，普通Android check/release通过，真实加载ABBA正在运行；旧candidate缺render_bounds被配置拒绝，无性能样本，保留排除。

同文件第 39 行原文：

> PBO探针已完成/3项严格RGBA全同：原序上传submit p50 130.252→18.643/27.714ms，但同步completed 132.279→136.740/135.232ms，无整帧收益，不接入。CPU模型分帧候选已实现并保持默认false；共享原max_time/判定线构造/跨线处理/Chart构造，约2ms后或完成边界让local loading future挂起一次，资源仍主线程，异步边界外才记录span。2项挂起/恢复/取消契约和44项host已通过，Android构建、真实加载ABBA及质量尚待完成；不能称已消除尖峰。主项目未push、vendor旧证据不覆盖，完整目标继续。

同文件第 5 行明确“模型分帧不进主线”：

> 只保留几何流批处理/连续目标/自动MSAA配对与能力回退/保守bbox/CPU和JSON后台加载；字体、mask、UI图片worker、模型分帧、shader snapshot诊断和observer均不进主线。

#### 6.6 研究后端构建

| 项 | 值 |
|---|---|
| 后端 | `chart-slices-ready.so` |
| SHA-256 | `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5` |
| `source_revision` | `b25272310fa41122c203b9db7c037886bc24f4bf` |
| 旧 candidate | `chart-slices-candidate.so` SHA `157f56352709793915c99c555adfb14f98fbbe346681a33a5d4f7b8942714c87` |
| 构建 | Android arm64/API26 release；普通 Android check；44 项 host 通过（历史记录） |
| 说明 | `source_revision` 是 artifact 记录字段；切片代码在参考实现 `4f046544`，两者不要混为“b2527231 已含切片” |

### 7. 质量与回归验证（RGBA/输出对照、失败项）

#### 7.1 已验证

- **6 组末帧 RGBA**：Happy ending/Petrichor，control/slices 各两轮，1920×1200，`different_channels=0`，`all_pixels_equal=true`。
- **96 项 CPU 输出**：16 labels × 2 场景 × 3 次，图像/全 PCM/typed-model 全部相同，`different_labels=[]`。
- **2 个调度测试**：边界 checkpoint 跨 render opportunity 保留工作；disabled 同步、取消不完成。
- **请求校验**：`chart_loading_slices` 必须依赖 `async_chart_json`，防止在没有 owned 模型时误用。
- **8 项 ABBA**：同一 SO，只切换 `chart_loading_slices`；全局与 Loading 指标都有原始 `frames.csv`/`analysis.json`。
- **44 项 host 契约**：历史记录通过；不是 P-07 的独立验收，但覆盖了 loading/输出 schema。

#### 7.2 失败项与限制

- **全局 FPS/p99/max 没有稳定改善**：FPS 59.545→59.523、59.379→59.507；max 64.963→67.352、67.389→67.063；p99 略好但不能称稳定。
- **Loading render 仍有 40–50 ms**：没有消除尖峰，只是降低最坏值；用户仍可能看到 Loading 卡顿。
- **总 CPU 工作没有减少**：切片把一次长调用摊到多帧，总计算量不变；如果 Loading 总时长变长，是可能的代价。
- **只有 2 个场景/30 s**：Exoplanetary + Petrichor；不是全谱/所有 chart/所有设备。
- **单设备**：Mali-G52 MC2；没有 Adreno/其他 Mali。
- **没有手动输入/暂停/取消的真实 UI 验收**：调度测试覆盖取消语义，但没有真实用户取消加载。
- **没有整帧收益**：P-07 不能称为整帧正优化；它只影响 Loading 的 render 分布。
- **observer 单轮**：stage observation 只用于定位，不混入 ABBA。
- **source_revision 与 feature commit 不同**：artifact 记录 `b2527231`，切片代码在 `4f046544`；不要把两者混写。

### 8. 判定：正/无/负 + 子类（整帧/局部）+ 理由 + 置信度

**判定：正优化。**

**子类：局部（Loading 阶段 render 分布），不是整帧。**

理由：

1. **Loading render max 明确下降**：两场景四轮都下降（51.772→39.850、44.594→39.845、58.981→50.122、57.871→49.837），中位数 −17.3%/−14.5%；不是单轮偶然。
2. **机制对应明确**：每 2 ms/边界让出一帧，LoadingScene 可以在切片间提交；`FrameBudget` 的单元测试证明挂起一次后恢复。
3. **全局没有改善**：FPS 持平、max 没有稳定下降、p99 只略好；说明收益局限在 Loading 的 render 分布。
4. **质量证据强**：6 组 RGBA + 96 项 CPU 输出全部相同；顺序/数值不变的实现假设得到支持。
5. **为什么是正而不是无**：Loading render max 的四轮方向一致，且中位数下降 14–17%；这不是只减少计数。
6. **为什么不是整帧**：gameplay FPS/p99/max 不变；总 CPU 不变；不能把 Loading 的改善写成“游戏整体更快”。

**置信度：**

- 对“这个设备、这个 SO、这两个 30 s 场景的 Loading render max 下降”：**高**。
- 对“机制是 2 ms 切片让出”：**高**（代码 + 调度测试 + Loading 段指标）。
- 对“全局 FPS/整帧/其他设备/全谱”：**低**。

### 9. 主线实施状态与参考实现状态

| 问题 | 回答 |
|---|---|
| 主线是否已实施 | 否（main）。参考实现已落地：`4f046544` 加入 `loading_schedule.rs`、`convert_phigros_loading`、`chart_loading_slices` 请求/配置 |
| 参考实现是否默认启用 | **否**。`chart_loading_slices` 默认 false；main 没有该字段/路径 |
| 参考实现是否有消费者 | 无；`GameScene::load_chart` 的 PGR 分支在 main 仍走 `parse_phigros_loading` → `convert_phigros`，没有 `convert_phigros_loading` |
| 是否进 main | **否**。previous-AGENTS 明确“模型分帧不进主线” |
| 是否有用户开关 | 无；只有研究 job |
| 是否依赖 `async_chart_json` | 是；请求校验强制 |
| 是否减少总 CPU | 否；只是切片 |
| 是否有独立验收 | 没有记录独立验收人；本文档是事后审计 |

### 10. 未进主线原因、风险与未验证项

#### 10.1 未进主线的原因

1. **收益只在 Loading render 分布**：全局 FPS/p99/max 没有稳定改善；主项目没有足够理由为 Loading 单阶段承担调度复杂度。
2. **总 CPU 不变，可能拉长 Loading 总时长**：切片让出会增加 poll 次数/帧间开销；如果用户感知的总加载时间变长，收益是“画面更流畅但更久”，需要产品权衡。
3. **Loading render 仍有 40–50 ms**：没有消除卡顿，只是降低最坏值。
4. **验证面不足**：2 场景、单设备、30 s；没有全谱/取消/重入/其他 GPU。
5. **参考实现未进 main**：保留为已验证候选，等待更大的 loading 尖峰收益或与资源阶段拆分组合。

#### 10.2 风险

| 风险 | 说明 | 当前缓解 |
|---|---|---|
| 让出点不安全 | 在 GL 命令/profiler scope 内让出会破坏状态 | 只在 max_time/每条线/finish 边界让出；注释明确 |
| 总加载变长 | 多帧 poll/调度开销 | 2 ms 阈值；边界只让一次；需测总时长 |
| 取消泄漏 | future 被 drop 后后台 worker 仍跑 | JSON worker 结果不被消费；转换在渲染线程，drop 即停 |
| 顺序/数值变化 | 切片改变遍历顺序会改 Chart | 顺序不变；6 组 RGBA + 96 输出相同 |
| 线程安全 | Chart/GL 跨线程 | 只有 owned `PgrChart` 跨 worker；Chart/GL 留在渲染线程 |
| 请求误配 | 没有 `async_chart_json` 时切片 | 校验强制 `chart_loading_slices` 依赖 `async_chart_json` |
| Loading 其他尖峰 | update/audio/block 仍长 | 切片只影响 render；stage observation 显示 update 仍有 43–58 ms |
| 设备差异 | 2 ms 阈值不一定适合所有设备 | 研究可调；生产需设备矩阵 |
| 全局 max 更差 | Exoplanetary max 64.963→67.352 | 不称尾延迟改善；需更多轮/场景 |

#### 10.3 未验证项

- 全谱/所有 chart 格式（PGR 以外不走此路径）。
- 真实用户取消/返回/重试 loading。
- 其他 GPU/CPU、低端设备上的 2 ms 阈值。
- Loading 总时长/功耗/内存。
- 与 UI 图片 worker、资源阶段拆分、glyph 上传的组合。
- 真实 Main 首次冷启动/热启动。
- `source_revision b2527231` 与 `4f046544` 的构建对应关系（artifact 记录 vs 参考实现提交），需要构建脚本复核。

### 11. 落地步骤（可执行清单）

- [ ] 先量化 Loading 总时长：切片是否让总加载变长；如果变长，评估用户可接受度。
- [ ] 在更多谱面/设备上重复 8 项 ABBA；确认 Loading render max 收益不是这两个场景特有。
- [ ] 测真实取消/返回/重试：future drop 后 worker 是否继续、是否浪费 CPU、是否影响下一次加载。
- [ ] 把 2 ms 阈值做成研究可调，扫描 1/2/4/8 ms；不要只调一个值。
- [ ] 与 UI 图片 worker、资源阶段拆分组合，确认不会互相抵消或重复让出。
- [ ] 用真实 Main 冷启动/热启动验收；不是研究壳的 Home→Library→Song 流程。
- [ ] 确认 `chart_loading_slices` 与 `async_chart_json` 的请求校验在所有入口一致。
- [ ] 若进 main，决定是否需要产品开关；当前研究默认 false，不要默认开启。
- [ ] 更新 AGENTS/README，明确“JSON 解码已在 worker，切片的是模型转换；总 CPU 不变；不是整帧优化”。

### 12. 实施完成标记

**合入 main 并完成第 7 节质量验收后**，把顶部 `☐ 未实施（main）` 改为 `☑ 已实施（main）`，填写日期/提交/验收人。

- 实施日期：以真正合入 main 的日期为准；
- 实施提交：填写 main 上的实际 commit（不是本文档引用的参考实现 commit）；
- 实施人：填写实际提交者；
- 验收人：填写独立验收人；如果没有独立验收，明确写“无独立验收”，不要把本文档的事后审计写成验收。

当前主线状态：`☐ 未实施（main）`；参考实现已落地。

### 13. 来源与可追溯性（commit/文件；仅溯源，正文已自包含）

| 项 | 值 |
|---|---|
| 来源 | 参考实现 |
| head | `3670c2aa390b6b788d45c7eeb262d94f936b8b82`（2026-10-06 19:28:34 +0800） |
| 切片提交 | `4f046544c2a3d15b709a020e119c65bd29d75128`（research: 保存渲染优化与真机验证，2026-10-06 18:52:02 +0800） |
| 作者 | `RENaa_FD` |
| 关键文件 | `prpr/src/parse/pgr.rs`（432 行；396-487）、`prpr/src/loading_schedule.rs`（66 行）、`prpr/src/loading_work.rs`（213 行；31/43/90-145）、`prpr/src/scene/game.rs`（516-535）、`phira/src/research.rs`（95/158/341-342/369） |
| 研究后端 | `chart-slices-ready.so` SHA-256 `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5`；`source_revision=b2527231` |
| 旧 candidate | `chart-slices-candidate.so` SHA `157f56352709793915c99c555adfb14f98fbbe346681a33a5d4f7b8942714c87` |
| 性能数据 | `本地证据/chart-slices-loading-confirmed/matrix.json`、`latency-summary.json`、`matrix-comparison.json` |
| 质量数据 | `本地证据/chart-slices-quality-confirmed/matrix.json`、`matrix-comparison.json`、`loading-output-equivalence.json` |
| Stage observer | `本地证据/chart-slices-stage-observation/matrix.json` |
| 设备 | MT6769V/CU / Mali-G52 MC2 / Android 10 / 1920×1200 |
| 谱面 | ExoplanetaryMirage（`fdf8408211260bf59eebf68cd6e484370cbdc89520a6556b80255f24de49def1`）、Petrichor |
| 说明 | 正文已包含完整代码、八轮数据、中位数、observer 与质量对照；上述路径只用于追溯 |
