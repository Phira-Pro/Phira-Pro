# 主线已集成优化附录

> 文档性质：性能审计附录，不是待实施清单。
>
> **口径说明：本附录不计入待实施清单；这里的“已实施”均指主线已实施，即已合入 `main` 并完成验收。** 本地参考实现、历史研究材料、能够编译、默认启用或真机跑过，均不构成“已实施”。
>
> 本文只记录**已经合入 `main` 并完成验收**、因此**不列入“待实施正优化”**的优化，以及若干“已确认没有独有可提取内容”的本地位置。第 1 至第 7 节中的每一项状态都是 `已实施（main）`，并给出合入提交与证据；后续不要把同一机制再次立项、再次实验或再次合入。第 8 节列出经逐项核对后确认没有生产优化可提取的本地归档、本地保留快照、旧版快照、归档探针目录和不可达提交。
>
> 本附录**不计入“待实施正优化”**。待实施清单只应保留尚未合入 `main` 的候选；本文列出的内容重复实施不会带来新的主线收益，只会重复消耗设备测量与审查成本。

## 0. 审计口径与证据边界

- 仓库：Phira-Pro，审计时 `main` 为 `0e33da13`（2026-11-11），工作区 tracked 文件干净；本文只写文档，不修改源码、不切换工作分支、不提交。文中的本地证据路径只用于溯源，不是阅读前提；判定所需的数据已在正文内联。
- 设备与画质：下文真机数据均来自 MTK MT6769V/CU（ADB board mt6768）、Android 10、Mali-G52 MC2、原生 1920×1200。除特别注明外，保持 RGB8、请求 sampleCount 2、实测 4 sample、原纹理过滤、原 shader、原粒子与 block/透明顺序；没有通过降低分辨率、MSAA、粒子或效果数量换取数字。
- 数据口径：`p50`、`p95`、`p99` 是各轮分位数的中位数；持续 FPS 是各轮帧数除以累计时长后的中位数；短固定窗口不是整首、不是系统实际呈现、也不是纯 GPU 时间。render 是 `GameScene::render` 的 CPU 墙钟，包含提交、resolve 与驱动等待。
- 状态判定：一个优化只有同时满足“合入提交可从 `main` 到达”“关键文件在 `main` 中存在”“普通路径实际使用该代码（不是仅研究开关）”“已完成验收”才记为 `已实施（main）`。只存在于本地参考实现、历史研究材料、本地保留快照、旧版快照、归档探针目录或不可达提交的代码不计入。
- 主要证据文件：`docs/development/rendering-research.md`、`docs/development/game-performance-target.md`、`本地证据/production-integration/integration.json`、`本地证据/branch-cleanup-20261006/README.md`，以及本文逐条列出的 `git show`、`git diff`、`git fsck` 输出。
- 术语约定：“block”指谱面 blockArea/噪域及其遮罩合成；“支持域”指最终 mask 中可能非零的保守区域；“隐式 MSAA”指 `EXT_multisampled_render_to_texture` 的 render-to-texture 路径；“配对效果”指每张效果输出纹理对应一张隐式输入纹理的轮换。

---

## 1. 共享 geometry arena + 连续同目标 pass + 离屏保留

**状态：`已实施（main）`。合入提交 `78445c20`（2026-10-06，`合入渲染与加载优化`），父提交 `b7547ec5`。生产依赖固定 macroquad `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1`、miniquad `1796f445440edf6b66d4d3a8491f9ac1c53ba52a`。**

### 1.1 机制

这一项解决的是“每个 draw 都重建并上传自己的最大容量顶点/索引缓冲，并且每遇到一次 flush 就结束并重开 render pass”的结构性成本。原实现为每个历史 draw 保留最大容量的 CPU 顶点/索引数组，并创建对应的 GPU buffer；游戏配置下每 draw 约 864 KiB，64 draw 约 54 MiB CPU 与同量 GPU 容量。小线条、文字、UI 矩形也按最大批容量付费。离屏目标上每次 flush 都切回默认 framebuffer，下一次 draw 再切回离屏目标，在 Mali 上形成明显的 pass 切换与存储解析成本。

合入后的机制分为四层：

1. **CPU 几何 arena。** macroquad 的 `QuadGl` 在每次 flush 时把所有 draw 的顶点写入一个连续的 `self.vertices`，把所有索引写入一个连续的 `self.indices`；每个 draw 只记录 `vertices_start` 与 `indices_start`，不再各自持有 buffer。索引仍以每个 draw 的局部 `u16` 编号，绑定顶点缓冲时用字节偏移选择该 draw 在 arena 中的切片，因此单个 arena 可以超过 65536 顶点，而每个 draw 仍满足 GLES2 的 `u16` 索引限制。
2. **一次上传。** `draw()` 对顶点与索引各调用一次 `update_orphaned`。`update_orphaned` 先用 `glBufferData(target, size, null, GL_STREAM_DRAW)` 孤化旧存储，再用 `glBufferSubData` 写入本帧数据；这样驱动可以让仍在排队的旧 draw 继续持有旧存储，不必为覆盖等待。GPU buffer 名与绑定保持有效，未写入的尾部字节未定义。
3. **连续同目标 pass。** `draw()` 维护 `active_pass: Option<Option<RenderPass>>`：只有真实目标变化、`ScreenTexture` 采样、capture、raw GL 观察点等边界才 `end_render_pass()`；连续且同目标的 draw 保持在同一个 pass 内。这样既保留原命令顺序、透明混合和投影，又消掉“每个 draw 结束再重开”的往返。
4. **离屏保留。** `retain_render_pass_on_flush` 允许调用方在 flush 后继续保留离屏 framebuffer；再次进入同一目标时走 `begin_pass_avoiding_redundant_bind`，避免重复绑定。该选项不延长附件寿命、不改变内容；snapshot、capture、目标切换仍然结束 pass，raw GL 消费者必须自行处理保留的 framebuffer 与 buffer 绑定。

主线在 `prpr/src/scene/game.rs` 中按 chart target 的策略启用/关闭保留：MSAA 开启且 target 声明 `retains_pass()` 时，flush 后保留 chart 输入 target；chart 绘制结束、切到 output 前显式清空。`prpr/src/core/resource.rs` 的 `NoteBuffer::draw_all` 不再在音符批次前 `flush()`，而是继续当前 target，把音符几何追加到同一 arena；后续必要边界（resolve、snapshot、glyph 上传、raw GL）仍保留 flush。

需要区分两个层次：`78445c20` 合入的是库层 arena、连续 pass、离屏保留和“音符前不 flush”；`f41ab7db`（pro.11）在此之上把 `NoteBuffer` 的每纹理 `Vec` 改为跨帧保留的 `NoteMeshes`，每帧只 `clear()` 长度并把 `current` 归零，减少每帧重新分配。两者都在 main，不能把后者的分配复用误记为另一项未实施优化。

### 1.2 关键代码

`prpr/src/core/resource.rs:361-408`（当前 main；`f41ab7db` 后的跨帧复用形态）：

```rust
#[derive(Default)]
struct NoteMeshes {
    meshes: Vec<(Vec<Vertex>, Vec<u16>)>,
    current: usize,
}

type NoteBufferMap = BTreeMap<(i8, GLuint), NoteMeshes>;

#[derive(Default)]
pub struct NoteBuffer(NoteBufferMap);

impl NoteBuffer {
    pub fn push(&mut self, key: (i8, GLuint), vertices: [Vertex; 4]) {
        let batch = self.0.entry(key).or_default();
        if batch.meshes.get(batch.current).is_some_and(|it| it.0.len() + 4 > MAX_SIZE * 4) {
            batch.current += 1;
        }
        if batch.current == batch.meshes.len() {
            batch.meshes.push(Default::default());
        }
        let last = &mut batch.meshes[batch.current];
        let i = last.0.len() as u16;
        last.0.extend_from_slice(&vertices);
        last.1.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }

    pub fn draw_all(&mut self) {
        // Continue the current target; geometry recording snapshots draw state.
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.draw_mode(DrawMode::Triangles);
        // Retain per-texture allocations across frames. GL copies geometry
        // while recording it; clearing these buffers doesn't alter queued draws.
        for ((_, tex_id), batch) in &mut self.0 {
            if batch.meshes.first().is_none_or(|mesh| mesh.0.is_empty()) {
                continue;
            }
            gl.texture(Some(Texture2D::from_miniquad_texture(unsafe { Texture::from_raw_id(*tex_id, miniquad::TextureFormat::RGBA8) })));
            for mesh in &mut batch.meshes {
                if mesh.0.is_empty() {
                    break;
                }
                gl.geometry(&mesh.0, &mesh.1);
                mesh.0.clear();
                mesh.1.clear();
            }
            batch.current = 0;
        }
    }
}
```

`本地参考实现（macroquad）src/quad_gl.rs`（生产 rev `139ac95` 的 `draw()` arena 上传段；归档 clone 可用 `git show 139ac95:src/quad_gl.rs` 核对）：

```rust
        // One contiguous upload per buffer per flush. Per-draw indices are
        // local; vertex binding offsets select the corresponding arena slice.
        let vertex_bytes = std::mem::size_of_val(self.vertices.as_slice());
        let index_bytes = std::mem::size_of_val(self.indices.as_slice());
        let bindings = self.bindings.get_or_insert_with(|| Bindings {
            vertex_buffers: vec![Buffer::stream(ctx, BufferType::VertexBuffer, vertex_bytes.max(256).next_power_of_two())],
            index_buffer: Buffer::stream(ctx, BufferType::IndexBuffer, index_bytes.max(256).next_power_of_two()),
            images: vec![Texture::empty(), Texture::empty()],
        });
        if bindings.vertex_buffers[0].size() < vertex_bytes {
            bindings.vertex_buffers[0].delete();
            bindings.vertex_buffers[0] = Buffer::stream(ctx, BufferType::VertexBuffer, vertex_bytes.next_power_of_two());
        }
        if bindings.index_buffer.size() < index_bytes {
            bindings.index_buffer.delete();
            bindings.index_buffer = Buffer::stream(ctx, BufferType::IndexBuffer, index_bytes.next_power_of_two());
        }
        bindings.vertex_buffers[0].update_orphaned(ctx, &self.vertices);
        bindings.index_buffer.update_orphaned(ctx, &self.indices);
```

`本地参考实现（macroquad）src/quad_gl.rs`（生产 rev `139ac95` 的每个 draw 偏移绑定与绘制段）：

```rust
            ctx.apply_pipeline(&pipeline.pipeline);
            if let Some((x, y, w, h)) = dc.viewport {
                ctx.apply_viewport(x, y, w, h);
            } else {
                ctx.apply_viewport(0, 0, width as i32, height as i32);
            }
            if let Some(clip) = dc.clip {
                ctx.apply_scissor_rect(clip.0, height as i32 - (clip.1 + clip.3), clip.2, clip.3);
            } else {
                ctx.apply_scissor_rect(0, 0, width as i32, height as i32);
            }
            let vertex_offset = dc.vertices_start * std::mem::size_of::<Vertex>();
            ctx.apply_bindings_with_offsets(bindings, &[vertex_offset]);

            if let Some(ref uniforms) = dc.uniforms {
                for i in 0..uniforms.len() {
                    pipeline.uniforms_data[i] = uniforms[i];
                }
            }
            pipeline.set_uniform("Projection", projection);
            pipeline.set_uniform("Model", dc.model);
            pipeline.set_uniform("_Time", time);
            ctx.apply_uniforms_from_bytes(
                pipeline.uniforms_data.as_ptr(),
                pipeline.uniforms_data.len(),
            );
            ctx.draw(dc.indices_start as i32, dc.indices_count as i32, 1);
```

`本地参考实现（miniquad）src/graphics.rs`（生产 rev `1796f44` 的 `apply_bindings_with_offsets`，完整函数；归档 clone 可用 `git show 1796f44:src/graphics.rs` 核对）：

```rust
    pub fn apply_bindings_with_offsets(&mut self, bindings: &Bindings, vertex_offsets: &[usize]) {
        let pip = &self.pipelines[self.cache.cur_pipeline.unwrap().0];
        let shader = &self.shaders[pip.shader.0];

        for (n, shader_image) in shader.images.iter().enumerate() {
            let bindings_image = bindings
                .images
                .get(n)
                .unwrap_or_else(|| panic!("Image count in bindings and shader did not match!"));
            if let Some(gl_loc) = shader_image.gl_loc {
                unsafe {
                    self.cache.bind_texture(n, bindings_image.texture);
                    glUniform1i(gl_loc, n as i32);
                }
            }
        }

        self.cache.bind_buffer(
            GL_ELEMENT_ARRAY_BUFFER,
            bindings.index_buffer.gl_buf,
            bindings.index_buffer.index_type,
        );

        let pip = &self.pipelines[self.cache.cur_pipeline.unwrap().0];

        for attr_index in 0..MAX_VERTEX_ATTRIBUTES {
            let cached_attr = &mut self.cache.attributes[attr_index];

            let pip_attribute = pip.layout.get(attr_index).copied();

            if let Some(Some(mut attribute)) = pip_attribute {
                let vb = bindings.vertex_buffers[attribute.buffer_index];
                let offset = vertex_offsets.get(attribute.buffer_index).copied().unwrap_or(0);
                assert!(offset <= vb.size, "Vertex buffer offset is out of bounds");
                attribute.offset = attribute.offset.checked_add(
                    <i64 as std::convert::TryFrom<usize>>::try_from(offset).expect("Vertex buffer offset is too large")
                ).expect("Vertex attribute offset overflow");

                if cached_attr.map_or(true, |cached_attr| {
                    attribute != cached_attr.attribute || cached_attr.gl_vbuf != vb.gl_buf
                }) {
                    self.cache
                        .bind_buffer(GL_ARRAY_BUFFER, vb.gl_buf, vb.index_type);

                    unsafe {
                        glVertexAttribPointer(
                            attr_index as GLuint,
                            attribute.size,
                            attribute.type_,
                            GL_FALSE as u8,
                            attribute.stride,
                            attribute.offset as *mut _,
                        );
                        if self.features.instancing {
                            glVertexAttribDivisor(attr_index as GLuint, attribute.divisor as u32);
                        }
                        glEnableVertexAttribArray(attr_index as GLuint);
                    };
                }

                let cached_attr = &mut self.cache.attributes[attr_index];
                *cached_attr = Some(CachedAttribute {
                    attribute,
                    gl_vbuf: vb.gl_buf,
                });
            } else {
                if cached_attr.is_some() {
                    unsafe {
                        glDisableVertexAttribArray(attr_index as GLuint);
                    }
                    *cached_attr = None;
                }
            }
        }
    }
```

`本地参考实现（miniquad）src/graphics.rs`（生产 rev `1796f44` 的 `update_orphaned`，完整函数）：

```rust
    pub fn update_orphaned<T>(&self, ctx: &mut Context, data: &[T]) {
        if self.buffer_type == BufferType::IndexBuffer {
            assert_eq!(self.index_type, Some(IndexType::for_type::<T>()));
        }
        let size = mem::size_of_val(data);
        assert!(size <= self.size);
        let gl_target = gl_buffer_target(&self.buffer_type);
        ctx.cache.store_buffer_binding(gl_target);
        ctx.cache.bind_buffer(gl_target, self.gl_buf, self.index_type);
        unsafe {
            glBufferData(gl_target, self.size as _, std::ptr::null(), GL_STREAM_DRAW);
            if size != 0 {
                glBufferSubData(gl_target, 0, size as _, data.as_ptr() as *const _);
            }
        }
        ctx.cache.restore_buffer_binding(gl_target);
    }
```

`prpr/src/scene/game.rs:2381-2389` 与 `:2448-2450`（离屏保留的启用与清理，完整片段）：

```rust
        unsafe {
            self.gl
                .quad_gl
                .retain_render_pass_on_flush(if msaa && res.chart_target.as_ref().is_some_and(|target| target.retains_pass()) {
                    res.chart_target.as_ref().map(|target| target.input().render_pass)
                } else {
                    None
                });
        }
```

```rust
        self.gl.quad_gl.render_pass(
            res.chart_target
                .as_ref()
                .map(|it| it.output().render_pass)
                .or_else(|| res.camera.render_pass()),
        );

        unsafe {
            self.gl.quad_gl.retain_render_pass_on_flush(None);
        }
```

### 1.3 完整性能数据

微基准（1920×1200，sample_count=1，相同几何与顺序，每 case 20 次预热、120 次采样；submit/completed 均为 p50 ms，`glFinish` 串行化，不是纯 GPU 时间；case4 的 draw call 仍是 64）：

| 场景 | 原始 submit / completed p50 ms | 共享连续 pass | 专用实例 + 共享路径 |
|---|---|---|---|
| 8192 音符，原本已经 1 批 | 0.8315 / 3.5936 | 0.8295 / 3.6748 | 0.2151 / 3.4121 |
| 64 次变换/纹理切换，屏幕目标 | 1.2578 / 3.4674 | 0.4957 / 2.6065 | 0.5055 / 2.8754 |
| 96 次裁剪/纹理切换 | 1.8443 / 4.1304 | 0.6959 / 2.9839 | 0.7052 / 3.0702 |
| 32768 四边形，4 批 | 3.8622 / 10.8562 | 2.9688 / 9.6328 | 0.5732 / 7.3316 |
| 64 次变换/纹理切换，离屏目标 | 15.3440 / 18.2721 | 0.7375 / 2.3542 | 0.7748 / 2.4508 |

即用户要求的离屏微基准：submit/completed p50 从 `15.34 / 18.27 ms` 降到 `0.74 / 2.35 ms`。拆分实验进一步说明优先项是 pass 边界而不是单纯 orphan：“共享缓冲区 + orphan，但保持逐次结束 pass”的 case4 为 `15.5587 / 18.5458 ms`，而共享连续 pass 的早一轮结果为 `1.1698 / 3.3585 ms`。

真实 GameScene（原生分辨率、MSAA2×、粒子、原效果精度、`blockAreaSimple=false`、不插 `glFinish`、固定谱面时钟与 seed；数值为两次运行 p50 的中位数）：

| 场景 | 原始帧间隔 / render ms | arena 帧间隔 / render ms | 像素结论 |
|---|---|---|---|
| Happy ending 21–27 秒 | 53.644 / 51.647 | 34.400 / 2.662 | 两后端、两次重复的末帧 RGBA 一致 |
| 8 线/2048 音符压力谱 | 65.166 / 62.490 | 47.160 / 2.838 | 两后端、两次重复的末帧 RGBA 一致 |
| Petrichor 105–111 秒，噪声实时时钟 | 58.451 / 56.609 | 39.804 / 12.436 | 原始后端重复截图自身不一致，不能据此判断优化破坏画质 |

同步 block 时钟后的独立 cohort（3 场景 × 2 后端 × 2 轮，9 项末帧 RGBA 全一致）：

| 场景 | 原始帧间隔 / render ms | arena 帧间隔 / render ms | CPU update+render+末尾 flush p50 |
|---|---|---|---|
| Petrichor 105–111 秒 | 58.481 / 56.416 | 39.736 / 12.405 | 56.558 → 12.519 ms |
| Petrichor 85–87 秒 | 53.142 / 51.122 | 33.238 / 7.871 | 51.266 → 7.993 ms |
| 确定性 prpr UI 负载 | 16.681 / 1.277 | 16.665 / 1.631 | 6.844 → 3.902 ms |

Happy ending 的 render 调用从 `51.647 ms` 降到 `2.662 ms`，约 `19.4×`；完整帧从 `53.644 ms` 降到 `34.400 ms`。Petrichor 密集 block 段完整帧从 `58.481 ms` 降到 `39.736 ms`。这两个数字分别是“调用耗时倍率”和“完整帧间隔”，不能把 19× 当成游戏整体 FPS 倍率，也不能把剩余 34.4 ms 归因到单一 shader。

原文完整段落（`docs/development/rendering-research.md`）：

> 最先值得改变的是渲染提交和 render pass 生命周期，而非再次给音符做普通纹理合批：项目的 NoteBuffer 已经按类型、纹理合批。离屏目标逐 draw call 结束 pass 会反复切换到默认 framebuffer，在这台 Mali 上形成很明显的成本。共享缓冲区而保留 pass 切分，不能解决这个场景；连续绘制同一目标有显著收益。

> 基础微基准中的glFinish串行化CPU/GPU，submit是构造提交到flush返回的墙钟时间，completed是提交到GPU完成的同步耗时，不能称为纯GPU时间。后续已用完整GameScene、两份真实谱面进行异步AB/BA验证，结果见下文：Happy ending的render调用约19倍加速，完整帧间隔53.64→34.40ms；Petrichor密集block段58.48→39.74ms。不能把调用耗时倍率当FPS倍率。

> 证据支持的正式优化顺序是共享geometry arena与连续同目标pass，其次是block保守支持域裁剪。两者已有真机收益和采集帧严格RGBA证据，仍需覆盖全谱、hover、自定义材质与其他设备后正式合入。播放IR是另一个有潜力的方向，现阶段只证明五百万音符的紧凑token表示可行，不代表游戏已能渲染SkyFire。

> NoteBuffer 按 `(kind, texture GLuint)` 的 BTreeMap 绘制，Hold、Drag、Click、Flick 分组，最多8192四边形一批。提高批容量到32768顶点、49152索引已存在。盲目按纹理重排会改变透明叠加关系；atlas 还必须处理 hold repeat、过滤边缘和现有纹理排序。因此不把全局纹理排序和 atlas 作为第一步。

> 拆分实验：共享缓冲区 + orphan，但保持逐次结束pass，case4 submit15.5587 / completed18.5458ms。对应共享连续pass的早一轮结果1.1698 /3.3585ms。因此有理由优先研究pass边界。计数是库层pass，不是已测量的硬件job数；驱动如何切分需GPU profiler确认。

> 严格像素验证：case0–4覆盖合批、状态切换、裁剪、超65536总顶点、离屏目标；case5覆盖旋转/倾斜角点、透明和彩色纹理、UV子区域、镜像Model。arena和arena-instanced全部6画面的解码RGBA字节与baseline完全一致，包含alpha=0的RGB。可用 `python vendor/render-bench/compare_pixels.py` 重现。只证明这些fixture，尚未证明全谱面、MSAA和第三方shader等价。

> Petrichor末帧分别位于110.98333秒和86.98333秒，覆盖不同block组合。仅这些采集帧验证等价；尚无全谱逐帧截图、触点hover、任意自定义shader和其他设备的证明。UI依然受约60Hz节奏约束，frame interval几乎不变；render录制略增，而提交成本下降，CPU总耗时降约43%，不能宣称UI帧率翻倍或已测得能耗改善。

### 1.4 提交与可达性

| 项目 | 值 |
|---|---|
| 主线合入提交 | `78445c201bbc4f2dfd08a3260f6f6b43cf19b746` |
| 父提交 | `b7547ec5d171271b645e7e6e0d516f16e1bcc410` |
| 提交日期 | 2026-10-06 |
| 提交说明 | `合入渲染与加载优化` |
| 变更规模 | 14 files changed, 890 insertions(+), 98 deletions(-) |
| 生产依赖 | macroquad `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1`；miniquad `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |
| 可达性 | `git merge-base --is-ancestor 78445c20 main` 为真 |
| 当前代码 | `prpr/src/core/resource.rs:361`、`prpr/src/scene/game.rs:2381`、库侧 `quad_gl.rs`/`graphics.rs` 均为同一机制 |

`78445c20` 的 14 个文件为：`prpr/src/core.rs`、`prpr/src/core/block_shader.rs`、`prpr/src/core/block_support.rs`、`prpr/src/core/chart.rs`、`prpr/src/core/render.rs`、`prpr/src/core/render_lifetime.rs`、`prpr/src/core/resource.rs`、`prpr/src/lib.rs`、`prpr/src/loading_cpu.rs`、`prpr/src/loading_work.rs`、`prpr/src/parse.rs`、`prpr/src/parse/pgr.rs`、`prpr/src/scene/game.rs`、`prpr/src/scene/loading.rs`。其中 `render_lifetime.rs` 与 `block_support.rs` 自该提交后没有后续修改，说明这两项的生产形态就是合入时的形态。

### 1.5 为什么不再作为待实施项

- 代码已经在普通路径无条件使用：`NoteBuffer::draw_all` 不再在音符批次前 flush；macroquad `draw()` 无开关地走 arena 与连续 pass；`game.rs` 在 MSAA 目标上直接启用 `retain_render_pass_on_flush`。不存在“研究开关打开才有收益”的状态。
- 真机收益与画质证据已经存在：两份真实谱面、12 项 AB/BA、9 项末帧 RGBA 一致；微基准 6 个 fixture 全字节一致。剩余缺口是“更多谱面时刻、hover、自定义材质、其他 GPU”的覆盖，不是“优化尚未写进主线”。
- 再次实施只会重复同一套结构化改动，并且会与现有 `vertices_start`/`apply_bindings_with_offsets`/`retained_render_pass` 语义冲突。若后续要做的是扩大验证覆盖，应归入“验证/回归”，而不是“待实施正优化”。

---

## 2. 隐式 MSAA + 目标生命周期 + 配对效果

**状态：`已实施（main）`。合入提交 `78445c20`；`prpr/src/core/render_lifetime.rs` 与 `prpr/src/core/block_support.rs` 自该提交后无修改；`render.rs` 之后只有 `04ff1dc1` 的 scissor 状态修复，MSAA 生命周期代码未改。**

### 2.1 机制

原路径每个 chart 目标都创建多采样 RBO，绘制中途或结束时 `glBlitFramebuffer` 显式 resolve；每次 flush 都可能切回默认 framebuffer，下一次 draw 再切回目标。该结构在 Mali 上把大量时间花在“反复存取多采样目标”而不是实际绘制。合入后的机制分为四层：

1. **目标生命周期属于 chart target，不是进程级开关。** `Policy` 只有两个布尔：`retain` 与 `implicit`。`Policy::continuous(eligible, samples)` 在 `samples > 1` 时启用 retain；只有 `eligible` 且 `samples > 1` 时才启用 implicit。`LEGACY` 两者都为 false，保证旧构造路径和回退路径仍存在。
2. **能力探测。** `implicit_api()` 只在当前 Android ES 上下文查询：GL 版本以 `OpenGL ES 3.` 开头、扩展列表包含 `GL_EXT_multisampled_render_to_texture`、`glFramebufferTexture2DMultisampleEXT` 与 `glGetFramebufferAttachmentParameteriv` 入口存在。探测在分配时做，不在每个 draw 做；上下文重建后重新判定，不按 GPU 型号硬编码。
3. **隐式目标创建与完整性验证。** `create_implicit_target` 先验证原 RBO 的 sample 数、RGB8 分量位数、sample 位置；再创建 FBO，用 `glFramebufferTexture2DMultisampleEXT` 把单采样纹理附上，查询实际 sample 数、`GL_RGB8`/`GL_UNSIGNED_NORMALIZED`/`GL_LINEAR`、FBO 完整性与 GL error，并比较新旧 sample 位置。任一不匹配或任一查询失败都返回 `None`，保留显式 MSAA；不会通过改 sample、格式或分辨率“凑兼容”。
4. **准备、最终 resolve、配对轮换。** 每帧在完整 clear 之前调用 `prepare_chart()`，建立 `sample_history_open`；绘制期间同一目标保持一个连续 sample history。`resolve_final()` 在 implicit 路径只绑定 output FBO（由扩展在 tile 内隐式 resolve），不执行显式 blit；显式路径仍走 `copy_fbo`。`swap()` 同时交换 `output` 与 `implicit`，让每张效果输出纹理对应一张隐式输入纹理；第二个输入延迟到下一轮完整 clear 之前创建，第二对失败时删除 implicit 候选并回退显式 RBO。

资格判定在 `prpr/src/scene/game.rs:1761`：含视频的谱面、`chart_debug`、非 `Normal`/`Texture`/`TextureGif` 的判定线都回退显式 MSAA；disabled block 只读 mask/noise，active overlay 在最终 resolve 与 HUD 之后才 `copy_scene`，因此正常 block 路径允许 implicit。效果、视频、Text/Paint 仍排除；意外早期 blit 与 target swap 明确拒绝。

### 2.2 关键代码

`prpr/src/core/render_lifetime.rs:10-36`（策略）：

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub retain: bool,
    implicit: bool,
}

impl Policy {
    pub const LEGACY: Self = Self {
        retain: false,
        implicit: false,
    };

    pub fn continuous(eligible: bool, samples: u32) -> Self {
        Self {
            retain: samples > 1,
            implicit: samples > 1 && eligible,
        }
    }

    pub fn implicit_supported(self) -> bool {
        self.implicit && implicit_api().is_some()
    }

    pub fn source_unverified(self) {
        tracing::warn!("Keeping explicit MSAA: cannot verify original RGB8 renderbuffer");
    }
}
```

`prpr/src/core/render_lifetime.rs:57-81`（能力探测，完整函数）：

```rust
fn implicit_api() -> Option<(Attach, AttachmentQuery)> {
    #[cfg(target_os = "android")]
    unsafe {
        // Query the current context when allocating, rather than caching support
        // across activity/context recreation. No capability query occurs per draw.
        if !gl_string(0x1F02)?.starts_with("OpenGL ES 3.") {
            return None;
        }
        if !gl_string(0x1F03)?
            .split_whitespace()
            .any(|value| value == "GL_EXT_multisampled_render_to_texture")
        {
            return None;
        }
        let attach = entry(c"glFramebufferTexture2DMultisampleEXT");
        let query = entry(c"glGetFramebufferAttachmentParameteriv");
        if !attach.is_null() && !query.is_null() {
            return Some((
                std::mem::transmute::<*const std::ffi::c_void, Attach>(attach),
                std::mem::transmute::<*const std::ffi::c_void, AttachmentQuery>(query),
            ));
        }
    }
    None
}
```

`prpr/src/core/render_lifetime.rs:83-125`（创建与验证，完整函数）：

```rust
pub unsafe fn create_implicit_target(texture: u32, samples: i32, source: u32, policy: Policy) -> Option<(u32, i32)> {
    if !policy.implicit {
        return None;
    }
    let (attach, query) = implicit_api()?;
    let Some(source_positions) = (unsafe { sample_positions(source, samples) }) else {
        tracing::warn!("Keeping explicit MSAA: cannot verify original sample positions");
        return None;
    };
    unsafe {
        use miniquad::gl::*;
        let mut previous = 0;
        glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut previous);
        let mut fbo = 0;
        glGenFramebuffers(1, &mut fbo);
        glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        attach(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0, samples);
        let mut actual = 0;
        query(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, 0x8D6C, &mut actual);
        // Do not infer component precision or encoding from an extension name.
        // The original chart source and every effect boundary are linear RGB8.
        let mut color = [0; 6];
        for (value, name) in color.iter_mut().zip([0x8212, 0x8213, 0x8214, 0x8215, 0x8211, 0x8210]) {
            query(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, name, value);
        }
        let format_matches = color == [8, 8, 8, 0, 0x8C17, 0x2601];
        let complete = glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
        let error = glGetError();
        let positions_match = complete
            && actual == samples
            && format_matches
            && error == GL_NO_ERROR
            && sample_positions(fbo, actual).is_some_and(|positions| positions == source_positions);
        if positions_match {
            return Some((fbo, actual));
        }
        glBindFramebuffer(GL_FRAMEBUFFER, previous as _);
        glDeleteFramebuffers(1, &fbo);

        tracing::warn!("Keeping explicit MSAA: implicit samples {actual}/{samples}, color {color:?}, complete {complete}, sample positions match {positions_match}, GL error {error:#x}");
        None
    }
}
```

`prpr/src/core/render.rs:138-209`（准备、最终 resolve、配对轮换，完整函数）：

```rust
    /// Prepare before the complete chart clear, never in the middle of a sample
    /// history. The second pair is allocated only when an effect has introduced
    /// a second output, so charts without effects retain a single implicit pair.
    pub(crate) fn prepare_chart(&mut self) {
        if !self.implicit_enabled {
            return;
        }
        assert!(!self.sample_history_open.get(), "Previous chart was not resolved");
        if self.implicit[0].is_none() {
            unsafe { get_internal_gl() }.flush();
            let texture = self.output().texture.raw_miniquad_texture_handle();
            self.implicit[0] = Self::implicit_target(texture, self.fbo, self.rbo, self.policy);
            if self.implicit[0].is_none() {
                // No frame has started yet: the original RBO can take over with
                // the original format and sample count, without copying samples.
                self.implicit_enabled = false;
                for target in &mut self.implicit {
                    if let Some(target) = target.take() {
                        unsafe {
                            miniquad::gl::glDeleteFramebuffers(1, &target.fbo);
                        }
                    }
                }
                tracing::warn!("Keeping explicit MSAA: second effect output could not be verified");
                return;
            }
        }
        self.sample_history_open.set(true);
    }

    pub fn blit(&self) {
        assert!(!self.implicit_enabled, "Implicit MSAA cannot preserve samples across an early snapshot");
        copy_fbo(self.fbo, internal_id(self.output[0].unwrap()), self.dim);
    }

    /// Final chart resolve. Early snapshots must continue to use `blit` and
    /// retain individual samples for the later chart layers.
    pub fn resolve_final(&self) {
        if self.implicit_enabled {
            assert!(self.sample_history_open.get() && self.implicit[0].is_some(), "Implicit chart input was not prepared");
            // The explicit attachment owner maintained one continuous sample
            // history. Switching to the single-sample output is its only resolve.
            unsafe {
                miniquad::gl::glBindFramebuffer(miniquad::gl::GL_FRAMEBUFFER, internal_id(self.output[0].unwrap()));
                assert_eq!(miniquad::gl::glGetError(), 0, "Implicit MSAA resolve failed");
            }
            self.sample_history_open.set(false);
            return;
        }
        assert!(copy_fbo(self.fbo, internal_id(self.output[0].unwrap()), self.dim), "Final MSAA resolve failed");
    }

    pub fn swap(&mut self) {
        assert!(!self.sample_history_open.get(), "Cannot swap an unresolved MSAA sample history");
        self.output.swap(0, 1);
        self.implicit.swap(0, 1);
        if self.output[0].is_none() {
            let gl = unsafe { get_internal_gl() };
            let texture = miniquad::Texture::new_render_texture(
                gl.quad_context,
                miniquad::TextureParams {
                    width: self.dim.0,
                    height: self.dim.1,
                    format: TextureFormat::RGB8,
                    ..Default::default()
                },
            );
            let render_pass = RenderPass::new(gl.quad_context, texture, None);
            self.output[0] = Some(RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass,
            });
            copy_fbo(internal_id(self.output[1].unwrap()), internal_id(self.output[0].unwrap()), self.dim);
        }
    }
```

`prpr/src/core/chart.rs:321-329`（最终 resolve 与保留清理）：

```rust
            if res.config.sample_count > 1 {
                unsafe { get_internal_gl() }.flush();
                if let Some(target) = &res.chart_target {
                    target.resolve_final();
                }

                unsafe {
                    get_internal_gl().quad_gl.retain_render_pass_on_flush(None);
                }
            }
```

`prpr/src/core/effect.rs:211-217`（效果输出与隐式输入成对轮换）：

```rust
        let target = res.chart_target.as_mut().unwrap();
        target.swap();
        let tex = target.old().texture;
        self.material.set_texture("screenTexture", tex);
        let screen_dim = vec2(tex.width(), tex.height());
        self.material.set_uniform("screenSize", screen_dim);
        gl.quad_gl.render_pass(Some(target.output().render_pass));
```

`prpr/src/scene/game.rs:2368-2389`（每帧准备与保留启用）：

```rust
        let msaa = res.config.sample_count > 1;
        if msaa {
            if let Some(target) = &mut res.chart_target {
                target.prepare_chart();
            }
        }

        let chart_onto = res
            .chart_target
            .as_ref()
            .map(|it| if msaa { it.input() } else { it.output() })
            .or(res.camera.render_target);

        unsafe {
            self.gl
                .quad_gl
                .retain_render_pass_on_flush(if msaa && res.chart_target.as_ref().is_some_and(|target| target.retains_pass()) {
                    res.chart_target.as_ref().map(|target| target.input().render_pass)
                } else {
                    None
                });
        }
```

### 2.3 完整性能数据

同一 so 的三种开关 ABC/CBA 两轮交叉重复，18 项运行、15 项严格末帧 RGBA 比较全部相同（包括 control 自身重复性）：

| 场景 | control p50 ms / 持续 fps | 保持目标 p50 ms / 持续 fps | 隐式 MSAA p50 ms / 持续 fps | 隐式 p95 ms |
|---|---|---|---|---|
| Happy ending 21–27 秒 | 32.464 / 30.75 | 17.825 / 55.30 | 16.684 / 60.04 | 17.117 |
| Exoplanetary Mirage 21–27 秒 | 33.421 / 29.99 | 18.304 / 54.49 | 16.682 / 60.00 | 17.160 |
| Petrichor 105–111 秒 | 35.198 / 27.33 | 20.484 / 47.38 | 16.724 / 59.71 | 18.637 |

即 `p50 32.5 ms → 17.8 ms（仅保持目标）→ 16.7 ms（隐式 MSAA）`。表中控制已包含 arena、note pass 和 bbox，不代表未经优化原版 0.7.1；全部保留 1920×1200、RGB8、原实际 4 sample、纹理过滤、shader、粒子、block 与透明顺序。

配对 MSAA 输入/效果输出（16 项 AB/BA 交叉重复、12 项严格末帧 RGBA 全部相同）：

| 场景 | 显式持续 fps | 配对隐式持续 fps | 配对 p50/p95/p99 ms |
|---|---|---|---|
| Happy ending + RAINBOW | 46.361 | 60.049 | 16.674 / 17.155 / 17.661 |
| Happy ending + RAINBOW + FXAA | 37.056 | 60.019 | 16.736 / 18.168 / 19.102 |
| Petrichor 105–111 + RAINBOW | 41.570 | 58.237 | 16.777 / 21.037 / 21.446 |
| Petrichor 85–87 + RAINBOW | 46.519 | 60.205 | 16.663 / 17.418 / 19.020 |

普通路径能力选择（18 项 ABC/CBA、15 项严格末帧 RGBA 全相同）：

| 场景 | control 持续 fps | 自动策略持续 fps | 否定隐式能力持续 fps |
|---|---|---|---|
| Happy ending | 30.77 | 60.04 | 55.08 |
| Petrichor 105–111 | 27.35 | 59.62 | 47.49 |
| Happy ending + RAINBOW | 27.78 | 46.41 | 46.40 |

自动策略 p95 分别为 17.123 / 18.978 / 23.041 ms。realtime 正常音轨补测：Petrichor 106.055–136.140 秒，1800 帧 / 1798 个非 I/O native 样本，1799 个 app 帧间隔累计 30.086 秒持续 59.796 fps，p50/p95/p99 为 16.670/18.082/22.659 ms；系统 15 秒窗口 854 次呈现 / 14.313 秒持续 59.597 fps，p50/p95/p99 为 16.681/16.762/16.966 ms，最大 33.460 ms，5 个 >1.5 refresh 间隔。

原文完整段落：

> 保留绑定是这轮主要收益，单独discard及组合增量尚无显著证据。Happy swap p50约29.107→14.611ms，Petrichor约23.550→7.886ms；它们是包含等待的墙钟耗时，不是纯GPU执行。背景clear/背景draw/谱面draw之间仍保留原投影与逻辑flush、几何及透明顺序，只不切回默认FBO。最终resolve、早期block snapshot、raw粒子及effect观察点仍保持。请求sampleCount2，但实测RBO实际4sample、RGB8每分量8位、1920×1200；所有候选均保留此实际采样数，不能降成实际2sample作达标证据。

> 逐调用审计纠正了初步一概排除blockArea的限制：正常GameScene的disabled层仅读mask/noise，active overlay在最终resolve与HUD之后才copy_scene，因此正常block路径允许隐式MSAA。效果、视频、Text/Paint仍排除；意外早期blit与target swap明确拒绝。保持RGB8、实际4sample及单采样HUD/粒子阶段，不把HUD移入MSAA改变边缘。

> 表中p50/p95为各轮分位数的中位数，持续fps为各轮帧数/累计时长结果的中位数，不混池；这些短fixed窗口不同于连续整段或系统实际呈现。控制已包含arena、note pass和bbox，不代表未经优化原版0.7.1。全部保留1920×1200、RGB8、原实际4sample、纹理过滤、shader、粒子、block与透明顺序；采集末帧相同不能证明全谱每帧和全部场景。

> 收益符合减少多采样目标反复存取、由扩展执行最终resolve的推导，尚无可信纯GPU时间/外存流量计数，不将swap等待直接称GPU执行。原始RBO在实验中仍保留分配，内存寿命尚可进一步整理。下一步补realtime音轨、系统实际呈现、多时刻像素和观察点回退，再把最终验证实现接入普通构建。当前短窗口接近60，不宣称完整目标完成。

> 连续MSAA只能在完整clear到最终resolve之间保留sample history。每张RGB8输出纹理现在有对应的隐式输入；效果在最终resolve之后交换输入/输出对，第二个输入延迟到下一轮完整clear之前创建。每个候选重新验证能力、实际sample和sample位置；第二对失败时删除隐式候选并使用一直保留的原RGB8 RBO。不可对尚未resolve的sample history执行swap。每个effect仍执行原shader并经过原RGB8边界，没有融合或降低采样。

> 表为每轮持续fps/分位数的中位数，不混池。奇数swap场景实际准备两对输入/输出；双效果偶数swap返回同一个当前输出，实测仅需要一对隐式输入。密集Petrichor仍有明显慢帧，不称完整目标完成。末帧RGBA不能代替所有活动边界与正常音轨呈现。

> 最终自动策略正常音轨补测：`production-effects-realtime/production-effect-realtime-20261006T015751-6e6fbf84`覆盖Petrichor106.074–136.491秒及RAINBOW，1800帧/1798非I/O native样本；1799间隔累计30.417秒，持续59.145fps，p50/p95/p99为16.698/19.147/23.060ms，全部正常慢间隔保留。系统请求15秒窗口获得845次呈现/14.263秒，持续59.175fps，p50/p95/p99为16.682/16.760/33.333ms，最大33.932ms，11个>1.5refresh间隔；15ring有重叠，任务始终running，电池28.4°C不变，host/GL无错误。截图为realtime，不加入固定时钟RGBA对照。仍有漏掉刷新周期，不宣称稳定60、整首或全部UI达标。

### 2.4 提交与可达性

| 项目 | 值 |
|---|---|
| 主线合入提交 | `78445c201bbc4f2dfd08a3260f6f6b43cf19b746` |
| 关键文件 | `prpr/src/core/render_lifetime.rs`、`prpr/src/core/render.rs`、`prpr/src/scene/game.rs` |
| `render_lifetime.rs` 后续修改 | 0 个提交（`git rev-list --count 78445c20..main -- prpr/src/core/render_lifetime.rs` 为 0） |
| `render.rs` 后续修改 | 1 个提交（`04ff1dc1` 只修 `copy_fbo` 的 scissor 状态恢复，未改 MSAA 生命周期） |
| 配对效果关键文件 | `prpr/src/core/effect.rs:211`、`prpr/src/core/render.rs:187` |
| 可达性 | `78445c20` 是 main 祖先；`render_lifetime.rs`、`render.rs`、`game.rs` 均在 main |

### 2.5 为什么不再作为待实施项

- 普通路径已经自动选择：`Resource::chart_render_policy` 由 `GameScene::implicit_msaa_eligible` 计算，`MSRenderTarget::with_policy` 在每帧/重建时消费；不存在研究编译开关才生效的问题。
- 回退是完整语义回退，不是“未实现”：能力缺失、入口缺失、sample/格式/位置不匹配、FBO 不完整、第二对分配失败都保留显式 MSAA；`blit()` 对早期 snapshot 断言回退；`swap()` 对未 resolve 的 sample history 断言拒绝。
- 性能证据覆盖短固定窗口、交叉重复、realtime 音轨、系统呈现和效果配对；剩余缺口是“更多时刻、整首、其他 GPU/桌面”的覆盖，不是主线上缺少代码。
- 重新实施会重复 `Policy`、`prepare_chart`、`resolve_final`、`swap` 与效果配对逻辑，并破坏现有回退断言。若目标是扩大覆盖，应走验证/回归，而不是待实施正优化。

---

## 3. block 支持域 bbox scissor

**状态：`已实施（main）`。合入提交 `78445c20`；`prpr/src/core/block_support.rs` 在 main 中存在且自该提交后未修改（`git rev-list --count 78445c20..main -- prpr/src/core/block_support.rs` 为 0）。**

### 3.1 机制

block shader 的片元成本主要不是最终覆盖的像素，而是“先读多次 mask、再做位移/subtract/edge/glow/ready 运算、最后才发现区域为空并 discard”。支持域裁剪不改变 quad、shader、uniform、采样或插值，只用硬件 scissor 把整块不可见区域排除在 fragment shader 之外。

实现分为“求支持域”和“映射到 framebuffer”两步：

1. **求支持域。** `block_support::Cache::update` 在 CPU 扫描最终 mask 的 `rgba` 与 `aux_rgba`。active 层的条件是 `p[..3] != [0,0,0]` 或 `aux[..2] != [0,0]`；disabled 层的条件是 `p[3] != 0`。对每个非零像素，用 `Bounds::include` 扩展 `x0/y0/x1/y1`。这里取的是**最终** mask 的支持域，位移、subtract、dilation、ready 处理都已经包含在 mask 里；因此不需要在 shader 外重新推导这些运算。hover 使用 `sceneUV` 和位移，触摸特效可能落在 mask 之外，因此 hover 整条回退，不定义支持域。
2. **边界余量。** `Bounds::uv` 在四边各留 `GUARD = 4` texel，覆盖 Linear 采样的半 texel 支持与 `basePixelUV` 向下取整的两 texel cell 采样；再加一个取整保护。这样不会因为支持域边界少一个 texel 而切掉本应可见的噪声/边缘。
3. **映射到 framebuffer。** `support_scissor` 把支持域 UV 的四个角通过当前 projection 与 model 变换到 clip space，再映射到 viewport 像素坐标；只接受仿射、有限、四角 `w` 相同的投影，透视或裁剪导致的非仿射直接回退。像素边界再各留 2 px 取整保护，并与调用方已有的逻辑 scissor 求交集，最后返回 `(x, y, w, h)`。
4. **应用与恢复。** `block_shader.rs:620-626` 只在 `!hover` 时计算 clips；有支持域时逐 clip 设置 `quad_gl.scissor(Some(clip))` 并画原来的整块 quad；没有支持域时按原路径画整块 quad。绘制结束后 `scissor(old_clip)` 恢复调用方状态。整个过程保持一个 quad，不把 quad 细分，因此不会改变 point sample 边界上的插值或 spark/noise texel。

缓存键是 `(revision, aux_revision, width, height)`：mask 内容 revision 或 aux revision 改变、尺寸改变都会重算；同一帧内 active/disabled 两层共用一次扫描。`visible` 早退仍保留：如果 disabled 层没有任何 alpha，或 active 层没有任何 RGB/ready 输入，则直接不画，不进入 shader。

### 3.2 关键代码

`prpr/src/core/block_support.rs`（完整文件，135 行）：

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
            *bounds = Some(Self {
                x0: x,
                y0: y,
                x1: x + 1,
                y1: y + 1,
            });
        }
    }

    pub fn uv(self, width: usize, height: usize) -> [f32; 4] {
        // Linear reads have half-texel support. basePixelUV floors to a two-
        // texel cell. Four texels cover both and include a rounding guard.
        const GUARD: usize = 4;
        [
            self.x0.saturating_sub(GUARD) as f32 / width as f32,
            self.y0.saturating_sub(GUARD) as f32 / height as f32,
            (self.x1 + GUARD).min(width) as f32 / width as f32,
            (self.y1 + GUARD).min(height) as f32 / height as f32,
        ]
    }
}

pub(super) struct Cache {
    key: Option<(u64, u64, usize, usize)>,
    pub active: Option<Bounds>,
    pub disabled: Option<Bounds>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            key: None,
            active: None,
            disabled: None,
        }
    }
}

impl Cache {
    pub fn update(&mut self, rgba: &[u8], aux: &[u8], width: usize, height: usize, revision: u64, aux_revision: u64) {
        let key = (revision, aux_revision, width, height);
        if self.key == Some(key) {
            return;
        }
        assert_eq!(rgba.len(), width * height * 4);
        assert_eq!(aux.len(), rgba.len());
        self.active = None;
        self.disabled = None;
        for (y, (row, aux_row)) in rgba.chunks_exact(width * 4).zip(aux.chunks_exact(width * 4)).enumerate() {
            for (x, (p, a)) in row.chunks_exact(4).zip(aux_row.chunks_exact(4)).enumerate() {
                if p[3] != 0 {
                    Bounds::include(&mut self.disabled, x, y);
                }
                // Ready contributes |aux.r-aux.g|*aux.b. Unioning R/G is wider
                // than its true support, which is safe. Hover is handled elsewhere.
                if p[..3] != [0, 0, 0] || a[..2] != [0, 0] {
                    Bounds::include(&mut self.active, x, y);
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
                    rgba.fill(0);
                    aux.fill(0);
                    if channel < 4 {
                        rgba[i + channel] = 1;
                    } else {
                        aux[i + channel - 4] = 1;
                    }
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

`prpr/src/core/block_shader.rs:272-302`（支持域到 framebuffer 的保守映射，完整函数）：

```rust
fn support_scissor(uv: [f32; 4], aspect: f32, viewport: (i32, i32, i32, i32), target_height: f32) -> Option<(i32, i32, i32, i32)> {
    let gl = unsafe { get_internal_gl() };
    let projection = gl.quad_gl.get_projection_matrix();
    let model = gl.quad_gl.get_model_matrix();
    let points = [(uv[0], uv[1]), (uv[2], uv[1]), (uv[2], uv[3]), (uv[0], uv[3])]
        .map(|(u, v)| projection * (model * vec4(u * 2. - 1., (v * 2. - 1.) / aspect, 0., 1.)));
    // Only affine, finite projections: perspective/clipped W falls back.
    if points.iter().any(|p| !p.is_finite() || p.w <= 1e-6 || p.w != points[0].w) {
        return None;
    }
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for p in points {
        let screen = vec2(
            viewport.0 as f32 + (p.x / p.w * 0.5 + 0.5) * viewport.2 as f32,
            target_height - (viewport.1 as f32 + (p.y / p.w * 0.5 + 0.5) * viewport.3 as f32),
        );
        lo = lo.min(screen);
        hi = hi.max(screen);
    }
    // Keep full-pixel rounding guard; intersect the existing logical scissor.
    let old = gl
        .quad_gl
        .get_scissor()
        .unwrap_or((viewport.0, target_height as i32 - viewport.1 - viewport.3, viewport.2, viewport.3));
    let x0 = (lo.x.floor() as i32).saturating_sub(2).max(old.0);
    let y0 = (lo.y.floor() as i32).saturating_sub(2).max(old.1);
    let x1 = (hi.x.ceil() as i32).saturating_add(2).min(old.0.saturating_add(old.2));
    let y1 = (hi.y.ceil() as i32).saturating_add(2).min(old.1.saturating_add(old.3));
    Some((x0, y0, x1.saturating_sub(x0).max(0), y1.saturating_sub(y0).max(0)))
}
```

`prpr/src/core/block_shader.rs:620-629`（按层选择支持域并计算 clip）：

```rust
        let clips = if !hover {
            let FrameTextures { masks, support, .. } = &mut *frame;
            support.update(&masks.rgba, &masks.aux_rgba, masks.width, masks.height, masks.revision, masks.aux_revision);
            let bounds = if disabled { support.disabled } else { support.active };
            bounds
                .and_then(|b| support_scissor(b.uv(masks.width, masks.height), aspect, viewport, target_height))
                .map(|clip| vec![clip])
        } else {
            None
        };
```

`prpr/src/core/block_shader.rs:806-822`（应用 scissor、保持单 quad、恢复旧 scissor）：

```rust
        let old_clip = unsafe { get_internal_gl() }.quad_gl.get_scissor();
        // Keep one quad: subdividing it changes vertex interpolation at point
        // sample boundaries, moving a few spark/noise texels between frames.
        // Specialize the expensive hover path instead of changing native UVs.

        if let Some(clips) = &clips {
            for clip in clips {
                unsafe { get_internal_gl() }.quad_gl.scissor(Some(*clip));
                draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE);
            }
        } else {
            draw_rectangle(-1., -1. / aspect, 2., 2. / aspect, WHITE);
        }

        if clips.is_some() {
            unsafe { get_internal_gl() }.quad_gl.scissor(old_clip);
        }
```

### 3.3 完整性能数据

单 bbox 路径，12 项 AB/BA，三种场景的 9 项末帧 RGBA 比较全部一致；与 arena 同一个 so 的开关对照：

| 场景 | arena 帧间隔 p50 ms | bbox 裁剪 ms | render p50 ms |
|---|---|---|---|
| Petrichor 105–111 秒 | 39.803 | 36.044 | 12.327 → 12.883 |
| Petrichor 85–87 秒 | 33.202 | 33.194 | 7.846 → 7.926 |
| Happy ending 21–27 秒 | 34.389 | 34.386 | 2.681 → 2.662 |

Petrichor 密集 block 段完整帧约减少 9.4%，另外两段无显著收益；扫描支持域增加 CPU 工作，render 反而略增（12.327→12.883 ms），这与“减少不可见 fragment、但不减少 CPU 扫描”的推导相符。该数据不是纯 GPU 时间测量。

分区裁剪（把分散 mask 映射到固定整数屏幕格并合并成互不重叠矩形；同 target 保持一个 pass）作为研究选项，12 项 AB/BA、9 项末帧 RGBA 全一致：

| 场景 | 同 so 关闭裁剪帧间隔 p50 ms | 分区裁剪 ms | render p50 ms |
|---|---|---|---|
| Petrichor 105–111 秒 | 39.729 | 35.559 | 12.259 → 12.654 |
| Petrichor 85–87 秒 | 33.215 | 33.200 | 8.049 → 8.107 |
| Happy ending 21–27 秒 | 34.304 | 34.361 | 2.589 → 2.663 |

密集 block 段完整帧约减少 10.5%；该 cohort 与 bbox cohort 独立，不能把 35.559 与 36.044 的差直接解释成可靠的增量收益。主线最终采用较简单的 bbox，而不是分区裁剪；分区裁剪保留为研究选项，不是待实施项。

组合方案（共享 arena + note pass + bbox）的完整 12 项 AB/BA、9 项末帧 RGBA 比较全部一致：

| 场景 | arena 帧间隔 p50 ms | 组合 ms | 帧间隔 p95 ms | render p50 ms |
|---|---|---|---|---|
| Petrichor 105–111 秒 | 39.756 | 35.539 | 46.276 → 43.548 | 12.195 → 12.627 |
| Petrichor 85–87 秒 | 33.174 | 32.286 | 34.879 → 33.681 | 7.965 → 7.879 |
| Happy ending 21–27 秒 | 34.353 | 32.537 | 35.721 → 33.754 | 2.619 → 2.502 |

增量帧间隔约减少 10.6% / 2.7% / 5.3%；block 密集段的 CPU/render 成本仍略增。原始→arena 与 arena→组合是独立 cohort，不能把倍率相乘或称为一次原始→组合对照。组合后端 SHA256 为 `3b9f63ac384e1770a30301954162e3326b34d2e9dbb16f2368c522226b48363e`。

原文完整段落：

> 进一步从block效果的依赖推导：final mask已经包含位移、subtract、edge、glow与ready结果。区域外全零时原shader必定discard，但仍先读取多次mask。可以用保守支持域限制硬件scissor，让这部分fragment不进入shader；保留原quad、shader、uniform和采样运算，不通过改顶点或划分几何改变插值。

> 实验缓存active RGB与ready R/G的支持域、disabled A的支持域；留出覆盖Linear和两texel point-cell采样的边界余量，用实际Projection/Model/viewport映射到framebuffer，交集已有scissor并恢复。hover使用sceneUV和位移，先完整回退；不适合的投影也回退。GL scissor本身可用于旧GL基线，算法不依赖Vulkan。

> 单bbox路径完成12项AB/BA，三种场景的9项末帧RGBA比较全部一致。与arena同一个so的开关对照：

> Petrichor密集block段完整帧约减少9.4%，另外两段无显著收益。扫描支持域增加CPU工作，render没有变快；这与减少不可见fragment的推导相符，但还不是纯GPU时间的测量证明。数据在`本地证据/bounds-matrix`。

> 再向前推导：单bbox覆盖中间空洞，分散mask可映射为固定整数屏幕格并合并成互不重叠矩形。对每个矩形重画同一个原quad，像素不会重复混合，插值不变；同target保持一个pass，避免每次scissor结束pass。区域过多或覆盖面积接近bbox时自动回到bbox，避免额外状态提交。

> 密集block段完整帧约减少10.5%；另外两段没有明显收益。该cohort与bbox cohort独立，不能把35.559与另一组36.044之间的差直接解释成可靠的增量收益。研究默认关闭裁剪；当前证据支持先选较简单的bbox方案，只有mask分散且额外draw成本合算时才考虑分区方案。

> 增量帧间隔约减少10.6%、2.7%、5.3%；block密集段的CPU/render成本仍略增。原始→arena与arena→组合是独立cohort，不能把倍率相乘或将二者称为一次原始→组合对照。首次组合尝试在5项完成后遭遇本机ADB daemon连接故障，保留在`本地证据/combined-matrix`，不混入这批完整结果。

> 生产合入前的具体补验是更多谱面时刻、block hover/触点、custom shader的ScreenTexture与uniform纹理、Paint/glyph上传、不同aspect/viewport、MSAA配置、桌面以及另一类移动GPU。现有证据仅是这些采集帧和单一设备，不能将自动化载体成功等同全部场景性能已测。GPU分解、能耗及realtime音频/pacing仍需单独采集；无效timer cohort不用于性能结论。

### 3.4 边界与回退清单

| 条件 | 行为 |
|---|---|
| hover / 触摸可见 | 整条回退，不计算支持域，按原整块 quad 绘制 |
| 透视投影、四角 `w` 不同、非有限值 | `support_scissor` 返回 `None`，按原整块 quad 绘制 |
| disabled 层 alpha 全零 | 早期 `visible` 判定直接返回，不绘制 |
| active 层 RGB 与 ready 输入全零 | 早期 `visible` 判定直接返回，不绘制 |
| mask revision / aux revision / 尺寸变化 | `Cache::update` 重算支持域 |
| 已有逻辑 scissor | 与支持域 clip 求交集，不扩大绘制范围 |
| 支持域为空 | `bounds` 为 `None`，不设置 scissor，按原路径绘制（不会误裁） |
| 支持域接近整屏 | 仍设置 scissor，但不会比原 scissor 更大；不引入额外 draw |

### 3.5 为什么不再作为待实施项

- 代码在普通 block 绘制路径中无条件启用（除 hover 回退），不是研究开关；`block_support.rs` 在 main 中且自 `78445c20` 后未修改。
- 收益方向与机制一致：只减少不可见 fragment，不改变 CPU mask 计算，因此 render 略增、完整帧在密集段下降；这正是已测到的形态。
- 剩余缺口是“更多谱面时刻、hover、自定义材质、其他 GPU”的覆盖，不是“bbox 尚未实现”。分区裁剪是研究选项，未证明稳定优于 bbox，不应作为待实施正优化重新立项。
- 再次实施会重复支持域扫描、4 texel 余量、仿射回退与 scissor 交集逻辑，并可能改变 point sample 边界像素；没有必要。

---

## 4. 加载期 CPU 工作后台化

**状态：`已实施（main）`。合入提交 `78445c20`；`prpr/src/loading_work.rs` 自该提交后无修改；`prpr/src/loading_cpu.rs` 之后只有 1 个提交（扩展 blur amount），后台化机制未改。**

### 4.1 机制

原加载路径在本地 future 第一次 poll 时同步执行：背景图解码、颜色提取、全分辨率 Gaussian blur、资源包图像解码、PCM 解码、音乐解码。它们与 GL 纹理创建、过滤、音频设备创建在同一个主线程帧内完成，形成数百毫秒的 Loading 尖峰。

合入后的边界是“纯 CPU 工作离开主循环，GL 与音频设备留在原线程”：

1. `loading_work::run` 在存在 Tokio runtime 时用 `runtime.spawn_blocking(work)` 把闭包送到阻塞线程池；返回的 JoinHandle 被包成 `poll_fn`，每帧用 `Waker::noop()` poll 一次。silent waker 避免 worker 唤醒 macroquad 外层 executor 的错误线程。
2. runtime 不存在（例如 wasm 或未初始化 runtime）时，`run` 直接执行同一个闭包，保持原解码器与输出算法；这不是“另一套 fallback 实现”，而是同一函数的同步调用。
3. `loading_cpu::decode_illustration` 保留原 `image::load_from_memory`、`color_thief::get_palette`、`fastblur::gaussian_blur` 的 kernel、半径、像素格式与迭代顺序；`illustration_with_blur` 只是把 amount 映射到原 50px 半径（0.5 对应原值），不改变默认输出。
4. 资源包图像、`hit_fx`、SFX、音乐都走 `loading_cpu::image` / `loading_cpu::audio`；`audio` 只做 `AudioClip::decode` + `from_raw`，不触碰音频设备。GL 纹理创建/过滤、`AudioManager` 创建仍在原线程。
5. 错误不被吞掉或替换内容：worker 的 `Result` 原样返回，panic 被 `spawn_blocking` 捕获为 `CPU loading worker failed`，加载失败仍按原路径报错。

### 4.2 关键代码

`prpr/src/loading_work.rs:8-23`（后台化入口，完整函数）：

```rust
/// The local loading future is polled once each frame. A silent waker keeps the
/// worker from waking macroquad's outer executor on the wrong thread.
pub async fn run<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let mut worker = Box::pin(runtime.spawn_blocking(work));
        return std::future::poll_fn(move |_| {
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            worker.as_mut().poll(&mut context)
        })
        .await
        .context("CPU loading worker failed")?;
    }
    // Runtime absence and wasm keep the same decoder and output algorithm.
    work()
}
```

`prpr/src/loading_cpu.rs:12-18` 与 `:21-52`（背景图入口与同步 CPU 实现，完整函数）：

```rust
pub async fn illustration(bytes: Vec<u8>) -> Result<Illustration> {
    illustration_with_blur(bytes, 0.5).await
}

pub async fn illustration_with_blur(bytes: Vec<u8>, amount: f32) -> Result<Illustration> {
    crate::loading_work::run(move || decode_illustration(&bytes, amount)).await
}
```

```rust
/// Synchronous CPU entry for ordinary background tasks (no GL and no frame executor).
pub fn decode_illustration(bytes: &[u8], amount: f32) -> Result<Illustration> {
    let image = image::load_from_memory(bytes).context("Failed to decode image")?;
    let (w, h) = (image.width(), image.height());
    let size = w as usize * h as usize;
    let mut blurred_rgb = image.to_rgb8();
    let color = color_thief::get_palette(&blurred_rgb, color_thief::ColorFormat::Rgb, 10, 2)?[0];
    // Preserve the existing kernel, radius, pixel format and iteration order.
    let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute::<*mut u8, *mut [u8; 3]>(blurred_rgb.as_mut_ptr()), size, size) };
    let amount = if amount.is_finite() { amount.clamp(0., 1.) } else { 0.5 };
    // 0.50 preserves the original 50px blur. Larger values expand its radius.
    let sigma = if amount <= 0.5 {
        amount * 100.
    } else {
        50. + (amount - 0.5) * 2. * (w.min(h) as f32 * 0.5).max(50.)
    };
    if sigma > 0. {
        fastblur::gaussian_blur(&mut vec, w as _, h as _, sigma);
    }
    std::mem::forget(vec);
    let mut blurred = Vec::with_capacity(size * 4);
    for input in blurred_rgb.chunks_exact(3) {
        blurred.extend_from_slice(input);
        blurred.push(255);
    }
    Ok(Illustration {
        width: w,
        height: h,
        rgba: image.into_rgba8().into_raw(),
        blurred,
        color: [color.r, color.g, color.b],
    })
}
```

`prpr/src/loading_cpu.rs:54-65`（图像与音频，完整函数）：

```rust
pub async fn image(bytes: Vec<u8>) -> Result<image::DynamicImage> {
    crate::loading_work::run(move || Ok(image::load_from_memory(&bytes)?)).await
}

pub async fn audio(bytes: Vec<u8>) -> Result<sasa::AudioClip> {
    crate::loading_work::run(move || {
        // AudioClip::new is exactly decode -> from_raw, with no device access.
        let (frames, rate) = sasa::AudioClip::decode(bytes)?;
        Ok(sasa::AudioClip::from_raw(frames, rate))
    })
    .await
}
```

`prpr/src/scene/loading.rs:72`（Loading 场景调用，完整语句）：

```rust
        let image = crate::loading_cpu::illustration(fs.load_file(path).await?).await?;
```

`prpr/src/core/resource.rs:212` 与 `:255`、`:523`（资源图像、hit_fx、音乐走同一后台入口；对应调用为）：

```rust
                SafeTexture::from(crate::loading_cpu::image(fs.load_file($path).await.with_context(|| format!("Missing {}", $path))?).await?)
                    .with_filter(GL_LINEAR)
```

```rust
        let hit_fx = crate::loading_cpu::image(fs.load_file("hit_fx.png").await.context("Missing hit_fx.png")?)
            .await?
            .into();
```

```rust
        let music = crate::loading_cpu::audio(fs.load_file(&info.music).await?).await?;
```

### 4.3 完整性能数据

真正 UI 的 8 项 ABBA，均按 scene 日志自动输入并进入正常 Playing，完整帧/native 严格匹配；hash 与截图关闭，不混入质量诊断。以下为每轮统计的中位数（两轮，不是置信区间）：

| 场景 | 同步→后台最大帧间隔 | 同步→后台 p99 | 全 30 秒窗口 FPS | Song→Playing 时间 |
|---|---|---|---|---|
| Exoplanetary | 798.002→162.152 ms | 18.938→18.383 ms | 56.190→59.224 | 8.631→9.660 秒 |
| Petrichor | 848.297→160.094 ms | 20.608→18.289 ms | 55.962→59.265 | 8.682→9.707 秒 |

Loading 中的主要卡顿减少约 4.9/5.3 倍；后台两轮最大间隔仍约 158–163 ms。Exoplanetary p95 从 17.208 到 17.412 ms 略增，Petrichor 17.328 到 17.308 ms，不能说所有尾部统计改善。该有限 UI 输入计划下进入游玩总时间约增加 1 秒，包含动画、主机输入间隔、调度和各加载步骤，不能全归因某个 decoder。背景原计算和音乐解码分别确实占同步路径约 787–833 及 541–552 ms，已取得内部 CPU 计时。

质量：加载质量新批次 8 项 ABBA / 6 项严格末帧 RGBA 相同，6 组共 90 项输出 proof 相同，覆盖原图、blur、主题色、资源纹理及全部 stereo float32 PCM 位。首次质量批次在第 6 项 daemon 连接失败，5 项记录保留排除，新批次没有实际重试。

原文完整段落：

> 新loading-cpu-candidate.so（SHA256 2b0b2235a728af942d5f3ea100eaeb952afa87076c69ac780670ecfa02ed3e2b，155源码输入）将原图解码/颜色提取/全分辨率blur及资源图像、PCM解码分离，GL纹理创建/过滤和音频设备仍在原线程。native有运行时则spawn_blocking、每帧poll静默waker；运行时缺失/wasm保留相同inline计算。错误不吞掉或换内容，host/真机ARM的4项无运行时回退、挂起/完成、解码错误和panic契约通过，普通/研究Android check及release通过。该阶段ordinary为false；后续独立8项质量cohort通过后已启用普通默认后台加载。

> Loading中的主要卡顿减少约4.9/5.3倍，不是gameplay渲染吞吐加速倍数。后台两轮最大间隔仍约158–163ms，必须继续定位；Library/Song也还有约60ms首帧。Exoplanetary p95从17.208到17.412ms略增，Petrichor17.328到17.308ms，不能说所有尾部统计改善。该有限UI输入计划下进入游玩总时间约增加1秒，包含动画、主机输入间隔、调度和各加载步骤，不能全归因某个decoder。背景原计算和音乐解码分别确实占同步路径约787–833及541–552ms，已取得内部CPU计时。输出hash质量对照独立验证原图/blur/主题色/全部stereo float32 PCM位；这些诊断的hash代价不得作性能结果。主目标、结算232ms、新特性密集窗口与跨设备回退仍未完成。

> production-async-loading.so SHA256 `55406065ca3ca9829227e13a26d4ef4b92436afc13aa6136e4b1f29b10ec13e8`，155源码输入核对；release、ordinary Android check及42项host契约通过。加载质量新批次8项ABBA/6项严格末帧RGBA相同，6组共90项输出proof相同，覆盖原图/blur/主题色、资源纹理及全部stereo float32 PCM位。首次质量批次在第6项daemon连接失败，5项记录保留排除，新批次没有实际重试。

> 两轮真正Home→Library→Song→Loading→Petrichor整首→Ending都保持正常音轨、autoplay、sampleCount2/实际4/RGB8/1920×1200/粒子/36个blockArea。均1436 Perfect+、0 Good/Bad/Miss；Playing约59.928fps。第二轮9935间隔/165.782秒，p95/p99/max17.116/17.473/23.864ms；全应用11387间隔/190.594秒59.745fps，p95/p99/max17.153/17.814/257.025ms。Loading update156.257ms、下一完整间隔159.666ms；Ending render254.477ms、下一间隔257.025ms，第一轮约293ms。尖峰仍保留在全局统计。

### 4.4 为什么不再作为待实施项

- 普通加载路径已经使用 `loading_cpu` / `loading_work`：背景图、资源图像、hit_fx、SFX、音乐全部走 `run`；不是研究 feature。
- 质量证据覆盖 6 项严格 RGBA 与 90 项输出 proof，且最终整首 Petrichor 仍为 1436 Perfect+、0 Good/Bad/Miss；后台化没有改变解码器、blur 半径、像素格式或 PCM 位。
- 剩余 158–163 ms 是“后台两轮最大间隔仍存在”的后续尖峰，不是“CPU 后台化未做”。若继续优化，应针对剩余加载阶段（Library/Song 首帧、音频设备创建、glyph 更新等）另立候选，不能重复搬移已经搬走的 CPU 工作。
- 重复实施会重新引入同步 decode/blur，直接回退已测得的 798→162 / 848→160 ms 改善。

---

## 5. JSON 谱面后台解析

**状态：`已实施（main）`。合入提交 `78445c20`；`prpr/src/parse/pgr.rs` 的 `parse_phigros_loading` 在 main 中存在；后续 7 个提交涉及 PGR/RPE 解析与相关功能扩展，未移除后台化。**

### 5.1 机制

PGR 谱面加载的原路径在 Loading 帧内同步执行 `serde_json::from_str`（或 `from_slice`），把整个 JSON 解析成 `PgrChart`，再在主线程构造 `Chart`、判定线、动画与 block 区域。JSON 解析是纯 CPU、只依赖 owned bytes，适合搬离主线程；`Chart` 及渲染资源包含非 Send 内容，必须留在原线程。

合入后的边界：

1. `GameScene::load_chart_bytes` 对 `ChartFormat::Pgr` 调用 `parse_phigros_loading(std::mem::take(&mut bytes), chart_extra).await`，把 owned `Vec<u8>` 移入 future。
2. `parse_phigros_loading` 在 `loading_work::run` 中只做 `serde_json::from_slice`，返回 `(PgrChart, Vec<u8>)`；`PgrChart` 是 owned serde 模型，可以跨线程。
3. 解码完成后回到原线程执行 `convert_phigros(pgr, extra)`：构造 `Chart`、`JudgeLine`、动画、BPM、block areas；GL/字体/纹理仍按原顺序。
4. 原始 bytes 一并返回，后续逻辑（例如重新解析、缓存、诊断）仍能拿到原文件；不改变文件内容或编码处理（`json_bytes` 仍负责去 BOM）。
5. runtime 缺失或 wasm 时 `loading_work::run` 同步执行同一闭包，行为与旧路径一致；错误仍以 `json-parse-failed` 上下文返回。

### 5.2 关键代码

`prpr/src/parse/pgr.rs:387-402`（同步入口、后台入口，完整函数）：

```rust
pub fn parse_phigros(source: &str, extra: ChartExtra) -> Result<Chart> {
    let pgr: PgrChart = serde_json::from_slice(super::json_bytes(source.as_bytes())).with_context(|| ptl!("json-parse-failed"))?;
    convert_phigros(pgr, extra)
}

/// Only the owned serde model crosses threads. Chart and its render resources
/// are constructed on the original thread after decoding completes.
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

`prpr/src/parse/pgr.rs:404-436`（主线程转换，完整函数）：

```rust
fn convert_phigros(pgr: PgrChart, extra: ChartExtra) -> Result<Chart> {
    let format_version = pgr.format_version;
    let max_time = *pgr
        .judge_line_list
        .iter()
        .map(|line| {
            (line
                .notes_above
                .iter()
                .chain(line.notes_below.iter())
                .map(|note| (note.time + if note.kind == 3 { note.hold_time } else { 0. }).not_nan())
                .max()
                .unwrap_or_default()
                * (60. / line.bpm / 32.))
                .not_nan()
        })
        .max()
        .unwrap_or_default()
        + 1.;
    let mut lines = pgr
        .judge_line_list
        .into_iter()
        .enumerate()
        .map(|(id, pgr)| parse_judge_line(pgr, max_time, format_version).with_context(|| ptl!("judge-line-location", "jlid" => id)))
        .collect::<Result<Vec<_>>>()?;

    process_lines(&mut lines);
    let mut chart = Chart::new(pgr.offset, lines, BpmList::default(), ChartSettings::default(), extra, HashMap::new());
    // Block-area times are authored in seconds (the official runtime compares
    // them directly against the song time), so no beat conversion is applied.
    chart.block_areas = parse_block_areas(pgr.block_area_list);
    Ok(chart)
}
```

`prpr/src/scene/game.rs:819-823`（PGR 分支调用，完整 match 分支）：

```rust
            ChartFormat::Pgr => {
                let (chart, owned) = crate::parse::parse_phigros_loading(std::mem::take(&mut bytes), chart_extra).await?;
                bytes = owned;
                Ok(chart)
            }
```

`prpr/src/parse.rs:16`（导出）：

```rust
pub use pgr::{parse_phigros, parse_phigros_loading};
```

### 5.3 完整性能数据

独立 8 项真实 UI 加载 ABBA（每轮统计中位数）：

| 场景 | 全 30 秒最大间隔 | Loading 最大 update | p99 |
|---|---|---|---|
| Exoplanetary | 161.682→63.247 ms | 61.255 ms | 18.324→17.995 ms |
| Petrichor | 160.316→64.309 ms | 51.412 ms | 18.287→18.445 ms |

每轮 >100 ms 的最大间隔由 1 变 0；Exoplanetary p99 略降，Petrichor p99 略增，不能声称全部尾部改善。质量：独立 8 项质量 / 6 项 RGBA / 96 项加载输出 proof 全部相同；两项最终帧也与旧同步解析后端严格一致。

完整 Petrichor 复测（`application-spike-stages-json-petrichor`）：11326 native 严格匹配 / 11324 非 I/O，真实 UI、正常音轨、完整 Petrichor 1436 Perfect+、0 Good/Bad/Miss；Playing 9935 间隔 / 165.781 秒为 59.9283 fps，p95/p99/max 17.179/17.769/28.255 ms。全应用 189.561 秒 59.7432 fps、最大 295.336 ms；独立系统 120 秒请求实际 119.006 秒、7131 呈现 59.9130 fps，p95/p99/max 16.738/16.789/33.421 ms，4 个 >1.5 refresh，116 ring 重叠，温度 30.9→31.2°C。

原文完整段落：

> JSON尖峰优化已验收并接入ordinary：独立8项真实UI加载ABBA，Exoplanetary全30秒最大间隔中位数161.682→63.247ms、Petrichor160.316→64.309ms；Loading分别61.255/51.412ms。Exo p99 18.324→17.995ms，Petrichor18.287→18.445ms略增；每轮>100ms由1变0，不称全部尾部改善。另独立8项质量/6项RGBA/96项加载输出proof全部相同，两项最终帧也与旧同步解析后端严格一致。仅owned serde模型自动Send到worker，Chart/extra/GL仍主线程，无运行时/wasm同算法inline。研究async_chart_json默认false保留旧对照，ordinary默认true，新完整job显式开启。production-json-loading.so SHA256 ad94680bec7ae23f9c1e36a82775d230209c38c1f00acfcd96117744734ac652，156源码hash核对，ordinary Android check/release通过；完整Petrichor正在验证，目标仍active。

> JSON普通策略完整复测已完成：application-spike-stages-json-petrichor共11326 native严格匹配/11324非I/O，真实UI/正常音轨/完整Petrichor1436 Perfect+，0 G/B/M，Playing9935间隔/165.781s为59.9283fps，p95/p99/max17.179/17.769/28.255ms。全应用189.561s为59.7432fps、最大295.336ms；独立系统120秒请求实际119.006s、7131呈现59.9130fps，p95/p99/max16.738/16.789/33.421ms，4个>1.5refresh，116ring重叠，温度30.9→31.2°C。stage observer有开销，不与旧pilot混池或声称p99全部改善。Loading剩余53.368ms render是glyph更新；model convert17.418ms与block prepare22.532ms仍同update帧40.566ms。Ending render292.627ms未解决。以上不是全目标完成，保留189个slow user-work帧及warmup尖峰。

### 5.4 为什么不再作为待实施项

- 普通 PGR 加载已经调用 `parse_phigros_loading`；`ordinary` 路径默认开启，不存在研究开关。
- 只搬移 owned serde 模型，`Chart`、GL、字体、纹理仍在原线程，因此不改变渲染资源生命周期，也不需要新的能力探测或回退。
- 质量证据覆盖 6 项 RGBA 与 96 项输出 proof，完整 Petrichor 仍为 1436 Perfect+；重复实施会重新把 JSON 解析放回 Loading 帧内，直接回退 161.7→63.2 / 160.3→64.3 ms 的改善。
- 剩余 Loading 尖峰（glyph 更新、model convert、block prepare）是不同阶段的问题，不应混入“JSON 后台解析”再次立项。

---

## 6. block area 性能修复

**状态：`已实施（main）`。合入提交 `cb70d098`（2026-10-06，`合并RPE噪域修复并提升构建号`）；块区域性能代码在 `77298da3`（pro.8）与 `9c85d587`（RPE 噪域状态适配）中。本地参考实现的头 `776b8108` 是 main 的祖先；任务说明中提到的合并提交 `18a6b021` 经核对是“把 main 合入本地参考实现”的同步提交，不是块区域性能代码的新增点；下文给出逐项证据，避免把同步提交误当成待实施项。**

### 6.1 机制

这一项针对 RPE 噪域/blockArea 的 CPU 遮罩与合成成本，不是第 3 节的“支持域 bbox scissor”。原路径每帧遍历全部 blockArea，按当前时间求值每个区域的变换、覆盖与混合，再生成多层 mask；大谱面、重复区域、彩色噪域和动态关键帧会把 CPU 主线程拖长。pro.8 的修复分成六块：

1. **时间可见索引。** `BlockTimeline` 把每个区域的 `appear_time`/`disappear_time` 建成按时间排序的事件表，每帧用 `partition_point` 推进游标，只保留当前可见的 id 集合；时间倒退（seek、重开）时清空并重建。这样不需要每帧扫描全部过去/未来区域，同时保持源顺序（包括 additive R8 混合顺序）。
2. **关键帧几何预光栅化。** `Masks::prepare_geometry` 只在 `appear/enable/disable/disappear`、旋转/位移/缩放事件的关键帧时间求 `Zone`，把精确姿态光栅化成行跨度 `(y, first, last)`，按几何 key 缓存；静态/保持区间直接复用这些行，动态中间帧回退到实时 `raster_rows`。预光栅化不量化动画几何或谱面时间，只在关键帧上取精确姿态。缓存有 16 MiB 硬预算，超预算停止继续准备，后续实时回退；重试时保留已准备内容。
3. **重复几何合并与扫描线差分。** 相同几何的区域合并为一组，只光栅化一次；遮罩合成用逐行差分代替逐像素四邻域 min/max。混合次序影响结果的情况保留原顺序，不做全局重排。
4. **可证明等价的 R8 混合快路径。** 对相同 opacity 的重复叠加，预计算 0..255 的 R8 曲线，用计数查表代替逐像素浮点累加；对混合 opacity，只有在所有可能 R8 输入都被证明可加时才走加权和，否则回退原混合。`blend_steps` 缓存每种 opacity 的判定结果，超过 512 项清空。
5. **噪声只在受影响范围计算。** 位移/噪声纹理的采样范围由当前 mask 的 `source_bounds` 限定；辅助纹理（ready/disabled/hover）只在 `revision`/`aux_revision` 变化时上传；没有可见噪域或触摸特效时跳过合成。
6. **快照与变换复用。** GLES3 场景快照使用官方 1/6 分辨率，减少复制带宽；判定与两层绘制共用可见索引，复用变换缓冲区。没有可见区域时不创建/上传 mask。

这些改动不改变几何、缓动、覆盖规则、噪声相位、音符层级或触摸判定，也不限制实际显示的噪域数量；它们只消除重复求值、重复光栅化和可证明等价的重复混合。

### 6.2 关键代码

`prpr/src/core/block_timeline.rs:1-40`（时间可见索引，完整实现的主体）：

```rust
//! A time sweep keeps large charts from visiting every past/future block twice
//! per frame. IDs remain in source order, including additive R8 blend ordering.
use super::BlockArea;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct BlockTimeline {
    count: Option<usize>,
    events: Vec<(f64, usize, bool)>,
    cursor: usize,
    time: Option<f64>,
    visible: BTreeSet<usize>,
    ids: Vec<usize>,
}

impl BlockTimeline {
    pub fn at(&mut self, areas: &[BlockArea], time: f64) -> &[usize] {
        if self.count != Some(areas.len()) {
            *self = Self::default();
            self.count = Some(areas.len());
            for (id, area) in areas.iter().enumerate() {
                if area.appear_time < area.disappear_time {
                    self.events.push((area.appear_time, id, true));
                    self.events.push((area.disappear_time, id, false));
                }
            }
            self.events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        }
        let end = self.events.partition_point(|event| event.0 <= time);
        let mut changed = false;
        if self.time.is_some_and(|previous| time < previous) {
            self.visible.clear();
            self.cursor = 0;
            changed = true;
        }
        while self.cursor < end {
            let (_, id, appears) = self.events[self.cursor];
            if appears {
                self.visible.insert(id);
            } else {
```

`prpr/src/core/block_timeline.rs:41-53`（续，完整实现的主体）：

```rust
                self.visible.remove(&id);
            }
            self.cursor += 1;
            changed = true;
        }
        if changed {
            self.ids.clear();
            self.ids.extend(self.visible.iter().copied());
        }
        self.time = Some(time);
        &self.ids
    }
}
```

`prpr/src/core/block_mask.rs:170-203`（关键帧几何预光栅化，完整函数）：

```rust
    /// Pre-rasterize exact keyframe poses, never quantizing animated geometry
    /// or chart time. Holds/static intervals reuse these poses; intermediates
    /// fall back to live rasterization. Hard 16 MiB budget, retained on retry.
    pub fn prepare_geometry(&mut self, width: usize, height: usize, aspect: f32, areas: &[super::BlockArea]) {
        self.clear_prepared_geometry();
        let (bw, bh) = ((width / 8).max(1) * 2, (height / 8).max(1) * 2);
        self.prepared_dim = (bw, bh, aspect);
        Lazy::force(&DISPLACE);
        prepare_workers();
        for area in areas {
            let times = [area.appear_time, area.enable_time, area.disable_time, area.disappear_time.next_down()]
                .into_iter()
                .chain(area.rotate_events.iter().map(|e| e.time))
                .chain(area.move_events.iter().map(|e| e.time))
                .chain(area.scale_events.iter().map(|e| e.time));
            for t in times {
                let Some(zone) = super::Zone::from_area(area, t, aspect) else {
                    continue;
                };
                let key = geometry_key(&zone);
                if self.prepared_rows.contains_key(&key) {
                    continue;
                }
                let mut rows = Vec::new();
                raster_rows(bw, bh, aspect, &zone, |y, first, last| rows.push((y as u16, first as u16, last as u16)));
                let bytes = rows.len() * std::mem::size_of::<(u16, u16, u16)>() + 96;
                if self.prepared_bytes + bytes > 16 * 1024 * 1024 {
                    return;
                }
                self.prepared_bytes += bytes;
                self.prepared_rows.insert(key, rows.into_boxed_slice());
            }
        }
    }

    pub fn clear_prepared_geometry(&mut self) {
        self.prepared_rows.clear();
        self.prepared_bytes = 0;
    }
```

`prpr/src/core/block_mask.rs:238-260`（相同 opacity / 可加混合的判定，完整片段）：

```rust
            if self.blend_steps.len() > 512 {
                self.blend_steps.clear();
            }
            let mut used = [false; 8];
            let mut additive = [true; 8];
            let mut same_opacity = [true; 8];
            let mut first_opacity = [None; 8];
            for z in zones {
                for (channel, opacity) in contributions(z).into_iter().flatten() {
                    used[channel] = true;
                    if let Some(first) = first_opacity[channel] {
                        same_opacity[channel] &= first == opacity.to_bits();
                    } else {
                        first_opacity[channel] = Some(opacity.to_bits());
                    }
                    let step = *self.blend_steps.entry(opacity.to_bits()).or_insert_with(|| blend_step(opacity));
                    additive[channel] &= step.is_some();
                }
            }
            // Identical opacities need only a count and their reachable R8
            // sequence, including .1's half-integer rounding. Mixed opacities
            // use weighted sums only when every possible R8 input was proven.
            let fast: [bool; 8] = std::array::from_fn(|c| used[c] && (same_opacity[c] || additive[c]));
```

`prpr/src/core/block_mask.rs:334` 与 `:354`（关键帧行跨度的消费点）：

```rust
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
```

```rust
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
```

### 6.3 完整性能数据

`docs/development/pro-8.md` 的验证数据（桌面 2560×1600，CPU 遮罩，非移动端 FPS）：

| 项目 | 数值 |
|---|---|
| 真实谱面 ハテ 若干片段 CPU 遮罩 | 约降低 11%～31% |
| 合成 8000 个重复 Disabled/Ready 区域压力用例 | 约 1740 ms → 2.5 ms |
| Desultory Signals 整个噪域 CPU/GPU 路径 | 约 7%～9% |
| 真实谱面数百帧 CPU 遮罩 | 与优化前逐字节相同 |
| 官方 ActiveBlock 合成 GLSL 比较 | 最大通道差 0 |
| 回归 | 核心 63 项、应用 11 项通过 |

8000 重复区域用例是合成压力测试，`1740 ms → 2.5 ms` 不能当作实机游玩帧率；ハテ 的 11%～31% 与 Desultory Signals 的 7%～9% 是桌面 CPU 路径数据，仍需移动端测量。pro.8 文档同时明确：真实谱面数百帧的 CPU 遮罩与优化前逐字节相同，说明快路径没有改变像素结果；但这只覆盖这些采集帧，不是全谱所有帧。

原文完整段落（`docs/development/pro-8.md`）：

> 噪域按时间事件维护可见索引，每帧只计算可见区域；判定与两层绘制共用可见索引，复用变换缓冲区。

> 遮罩对可证明等价的混合使用扫描线差分、重复几何合并与原有 R8 量化查表。混合次序影响结果的情况保留原顺序。

> 缓存静态/关键帧几何；遮罩噪声仅在可能受影响的范围计算。辅助纹理仅在内容变化时传输，没有可见噪域和触摸特效时跳过合成。

> GLES3 场景快照采用官方 1/6 分辨率，减少复制带宽；不适用的多采样系统帧缓冲及 GLES2 保留兼容路径。

> 不改变几何、缓动、覆盖规则、噪声相位、音符层级或触摸判定，不限制实际显示的噪域数量。

> 真实谱面数百帧的 CPU 遮罩与优化前逐字节相同。桌面 2560×1600 的ハテ若干片段 CPU 遮罩约降低 11%～31%；合成的 8000 个重复 Disabled/Ready 区域压力用例由约 1740 ms 降至 2.5 ms。这是合成压力测试，不是实机游玩帧率。Desultory Signals 整个噪域 CPU/GPU 路径的桌面收益相对较小，约 7%～9%，仍需移动端测量。

### 6.4 提交核对

| 提交 | 日期 | 父提交 | 作用 | 是否新增块区域性能代码 |
|---|---|---|---|---|
| `77298da3` | 2026-10-05 | `9ba712f6` | `优化移动端帧率并完善 pro.8 噪域渲染` | 是：`block_mask.rs` +325/−63、`block_shader.rs` +96/−11、新增 `block_timeline.rs` 88 行、新增 `frame_pacing.rs` 123 行 |
| `9c85d587` | 2026-10-06 | `56ee3c81` | `适配RPE噪域状态并接入现有渲染`；本地参考实现的创建点 | 是：新增 `rpe_block.rs` 212 行，`chart.rs` +41/−24，`block_area_frames.rs` 23 行变更 |
| `cb70d098` | 2026-10-06 | `78445c20` + `9c85d587` | `合并RPE噪域修复并提升构建号`，把 RPE 噪域修复并入 main | 是：9 files，+474/−85 |
| `18a6b021` | 2026-10-13 | `ac5001ad` + `066a6548` | 把 main 合入本地参考实现（同步提交） | 否：相对第二父提交 `066a6548`，`prpr/src` 只有 `judge.rs` 的 +16/−27 行，`prpr/src/core` 无 diff；不含 block area 代码 |
| `776b8108` | 2026-10-07 | `c754c322` | `统一升级至pro.10并同步安卓和苹果内部版本` | 否：版本/同步；本地参考实现本身不新增块区域代码 |
| `f41ab7db` | 2026-10-07 | main | pro.11 `优化全局帧稳定性并修复RPE噪域锚点` | 是：`block_mask.rs` +276/−50、`block_shader.rs` +44、`rpe_block.rs` +87/−2；进一步主线修复 |
| `0526256b` | 2026-10-07 | `f41ab7db` | `支持彩色噪域并修复白色边框和黄红键点击保护` | 是：新增 `block_color.rs` 184 行、`block_shader.rs` +86/−13；彩色噪域功能，不是性能回退 |

提交可达性核验：

- 本地参考实现头提交为 `776b81087f1b9716e4f219da4fbbac21381bcef1`（在该工作区用 `git rev-parse HEAD` 核对）。
- `git merge-base main 776b8108` 同为 `776b81087f1b9716e4f219da4fbbac21381bcef1`，即该头提交就是 merge base；`git merge-base --is-ancestor 776b8108 main` 为真。
- `git diff --stat main 776b8108` 为 `539 files changed, 3287 insertions(+), 153815 deletions(-)`；`git diff --name-status` 的 327 个 `D` 是 main 后来删除的旧文件，唯一 `A` 是旧 `phira/src/censor`。本地参考实现没有独有的渲染/加载生产文件，整体是 main 的旧快照。
- `git diff --stat 18a6b021^2 18a6b021 -- prpr/src` 只有 `judge.rs` 的 16/−27 行；`git diff --stat 18a6b021^2 18a6b021 -- prpr/src/core` 为空。`18a6b021` 相对第二父提交的差异是文档与 `judge.rs`，不是 block area 性能。
- `776b8108` 头本身已是 main 祖先（其子 `f01877de` 之后沿主线合并）；含 `18a6b021` 的 PR #1 线经 `d756c018`（`Merge PR #1: preserve same-frame clicks and clear reused touch IDs`）进入 main，`18a6b021` 是它之前的 main→本地参考实现同步点。把 `18a6b021` 当作“块区域性能合入提交”会重复实施已经在 `77298da3`/`9c85d587`/`cb70d098` 完成的代码。

### 6.5 为什么不再作为待实施项

- 块区域性能代码已经在 main 的普通路径中：`BlockTimeline`、`prepare_geometry`、扫描线差分、R8 快路径、`source_bounds`、1/6 快照都在 `prpr/src/core/block_mask.rs` 与 `block_shader.rs` 中被实际调用，没有研究开关。
- 本地参考实现（头 `776b8108`）完全被 main 包含，且没有独有文件；`18a6b021` 只是同步提交，不包含新的块区域实现。
- 数据已经给出方向：合成重复区域巨大收益、真实谱面 11%～31%、Desultory 7%～9%、遮罩逐字节相同。剩余工作是移动端测量与其他谱面覆盖，不是“代码尚未实现”。
- 重新实施会重复时间索引、几何缓存、混合快路径与 16 MiB 预算；如果目标是扩大覆盖，应做验证，不应把同一项再次列入待实施正优化。

---

## 7. render-production 整合（本地参考实现 / PR #7）

**状态：`已实施（main）`。生产子集合入提交 `78445c20`（第 1～5 节）；应用层整合合并提交 `b3040dd5`（PR #7，2026-11-10）。本地参考实现的头 `69f31f8e` 是 main 的祖先，相对 `78445c20` 多出 119 个提交，全部已在 main。这一项不是独立的新优化，而是上述渲染/加载生产子集的整合容器。**

### 7.1 机制与边界

本地参考实现的基线是 `78445c20`，因此它天然包含第 1～5 节的全部生产代码。其后续 119 个提交主要是 pro.10～pro.18 的应用层功能：RPEN 编辑器、练习面板、判定预设、结算布局、谱面视频、HUD、多指/触控修复、PPSR、HOLD 渲染、i18n 与发布配置等。它们不改变 arena/pass、MSAA 生命周期、bbox scissor、loading CPU、JSON 后台解析的核心机制；其中对 `prpr/src/core/render.rs` 的后续改动只有 `04ff1dc1` 的 `copy_fbo` scissor 状态恢复，对 `loading_cpu.rs` 的后续改动只有 1 个提交，对 `render_lifetime.rs`/`loading_work.rs`/`block_support.rs` 没有后续修改。

PR #7 的合并提交 `b3040dd5` 的第一父为 `0d43632d`，第二父为 `d8e84306`；第二父线包含 `69f31f8e`。`b3040dd5` 相对第一父的变更统计为 18 files，+464/−8，内容是 HOLD 渲染、设置项、PPSR 与解析字段，不包含 arena/pass、MSAA、bbox、loading CPU 或 JSON 后台解析的新实现。也就是说，PR #7 把本地参考实现的应用层工作合入 main，但渲染/加载优化的“生产子集”在更早的 `78445c20` 就已经进入 main。

本节没有独立的性能数字可列：本地参考实现的渲染/加载代码与第 1～5 节是同一套 so/源码，性能数据就是第 1～5 节的数据；把那些数字再算一次会重复计数。`integration.json` 的 `backend.installed_or_new_device_cohort` 为 `false`，也说明这次整合没有新的设备 cohort。因此第 7 节只记录“整合关系与构建证据”，不新增待实施项，也不产生新的收益声明。

### 7.2 `78445c20` 的生产文件清单

`本地证据/production-integration/integration.json` 记录的生产文件为 16 个：

```text
Cargo.lock
Cargo.toml
prpr/src/core.rs
prpr/src/core/block_shader.rs
prpr/src/core/block_support.rs
prpr/src/core/chart.rs
prpr/src/core/render.rs
prpr/src/core/render_lifetime.rs
prpr/src/core/resource.rs
prpr/src/lib.rs
prpr/src/loading_cpu.rs
prpr/src/loading_work.rs
prpr/src/parse.rs
prpr/src/parse/pgr.rs
prpr/src/scene/game.rs
prpr/src/scene/loading.rs
```

这 16 个文件覆盖第 1～5 节的全部机制：`resource.rs` 的 NoteBuffer、`render_lifetime.rs`/`render.rs`/`game.rs` 的 MSAA 生命周期、`block_support.rs`/`block_shader.rs` 的 bbox、`loading_cpu.rs`/`loading_work.rs` 的后台加载、`parse/pgr.rs` 的后台 JSON。除此之外的文件（编辑器、视频、HUD、联机等）不属于这次渲染/加载生产整合。

### 7.3 集成证据

`integration.json` 的关键字段：

| 字段 | 值 |
|---|---|
| `repositories.phira.commit` | `78445c201bbc4f2dfd08a3260f6f6b43cf19b746` |
| `repositories.phira.branch` | `main` |
| `repositories.miniquad.commit` | `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |
| `repositories.macroquad.commit` | `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1` |
| `base` | `56ee3c8199d52f5128927a250f970fd53e57ddba` |
| `production_files` | 16 个（见上） |
| `backend.sha256` | `251fc01a63cbea7609cf9507647e91c64c91915bee924363cbcafd239bf7ed96` |
| `backend.matching_source_commit` | `78445c201bbc4f2dfd08a3260f6f6b43cf19b746` |
| `backend.installed_or_new_device_cohort` | `false` |
| `git_dependencies.macroquad` | `139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1` |
| `git_dependencies.miniquad` | `1796f445440edf6b66d4d3a8491f9ac1c53ba52a` |
| `git_dependencies.sasa` | `5049b671cff93be1cf2f7a3f693e7ae1f0c179f7` |
| `local_research.branch` | 本地参考实现（该字段只标注本地研究材料，不代表主线状态） |
| `local_research.head` | `3670c2aa390b6b788d45c7eeb262d94f936b8b82` |
| `goal_complete` | `false` |

检查项（日志与 SHA256）：

| 检查 | 日志 | SHA256 |
|---|---|---|
| Android check | `android-check.log` | `3e1d2ef08dc3b34510d3001ef99bfe18586ae0560fb9ecd348a18412d3474d97` |
| Windows `phira` + `phira-main` check | `windows-check.log` | `68e50da90953121eff8e4378a322c01bce7e76f4e91f642167a524261538870d` |
| Android release | `android-release.log` | `1f97189f478e7ef80ad92b613b9ecde9beda616b84affdff6978b0cebfb4032c` |
| 五项生产契约 | `contracts-final.log` | `9c7db052c2022ce9851bedf4c531e4f48fbdc44e90b1567e2e7476757ea8a369` |

`backend.installed_or_new_device_cohort` 为 `false`，说明这次整合没有新的完整真机 cohort；第 1～5 节的性能数字来自整合前的研究/验证 cohort，不能把 `production-clean.so` 当作新设备复测。整合本身是“同一源码、固定依赖、普通构建通过”，不是新的性能实验。

后续主线更新：

| 提交 | 日期 | 规模 | 内容 | 对本文机制的影响 |
|---|---|---|---|---|
| `f41ab7db` | 2026-10-07 | 30 files, +1317/−114 | pro.11 全局帧稳定性、RPE 噪域锚点、`frame_profile`、`desktop_pacing`、`block_mask` 326 行、`block_shader` 44 行 | 在 arena/MSAA/bbox 之上增加帧 pacing 与噪域锚点修复；不替换本文机制 |
| `04ff1dc1` | 2026-10-30 | 31 files, +3376/−1307 | pro.17 谱面视频、加载/结算画面、`render.rs`/`game.rs`/`layout.rs` | `render.rs` 只修 `copy_fbo` 的 scissor 状态；视频路径仍按 `implicit_msaa_eligible` 排除 |

### 7.4 为什么不再作为待实施项

- 本地参考实现的生产子集就是 `78445c20` 的 16 个文件，已在 main；其多出的 119 个提交是应用层功能，不是新的渲染/加载优化。
- PR #7 的 `b3040dd5` 变更集是 HOLD 渲染/设置/PPSR 等，不包含 arena、MSAA、bbox、loading CPU、JSON 后台解析的新代码；把它当作“渲染生产整合”会重复第 1～5 节。
- `integration.json` 记录的是普通 Android/Windows check、release、五项契约与固定依赖，没有新的设备性能 cohort；重复整合不会产生新收益。
- 若后续要继续做应用层功能（编辑器、视频、HUD），应按功能立项；不要把已整合的渲染/加载机制重新列入待实施。

---

## 8. 已确认没有独有内容的本地位置

本节记录经逐项核对后确认“没有生产优化可提取”的位置。它们不是“待实施”，也不是“未验证”，而是**不要从这里重复提取代码**。每个位置给出核对命令与结果，避免后续仅凭旧名称再次立项。

### 8.1 仅含测量载体的本地归档

- 头提交：`987ea164`（`归档中断前研究文件`），父提交 `78445c20`。
- 相对 main 的差异：`git diff --stat main...987ea164` 为 7 files changed, 2163 insertions(+)。7 个文件全部是新增：
  - `phira/src/research_application.rs`（186 行）
  - `phira/src/research_native_timing.rs`（89 行）
  - `prpr/src/research_spikes.rs`（184 行）
  - `scripts/capture_android_frames.py`（140 行）
  - `scripts/render_research.py`（1108 行）
  - `scripts/tests/test_capture_android_frames.py`（39 行）
  - `scripts/tests/test_render_research.py`（417 行）
- 上述 7 个路径在 main 中全部不存在（`git cat-file -e main:<path>` 均失败），也没有删除任何 main 文件。
- 内容性质：研究载体、帧采集、native 计时、spike 记录与 Python 测试；没有 `prpr/src/core` 的渲染实现，没有 `loading_cpu`/`loading_work` 的生产逻辑，也没有库层 arena/pass 代码。
- 结论：仅含测量载体的本地归档只保存测量工具与证据，**没有生产优化可提取**。其中的研究思路（spike 细分、应用观察）属于验证/诊断，不是待实施正优化。

### 8.2 本地保留快照

- 本地保留快照的提交：`2b89e03c`（2026-10-06，旧研究快照）。
- `git diff --stat 2b89e03c 3670c2aa`（本地研究材料）：120 files changed, 66734 insertions(+), 92 deletions(-)。
- `git diff --name-status 2b89e03c 3670c2aa` 的状态计数：`114 A`、`6 M`、`0 D`。**没有 `D`**，即不存在“只在本地保留快照中存在、本地研究材料中不存在”的文件；差异是本地研究材料在旧快照之后新增/修改的内容。
- 核心文件哈希逐项对比（本地保留快照与本地研究材料同路径）：

| 文件 | 快照哈希（前 10 位） | 与本地研究材料相同 | 说明 |
|---|---|---|---|
| `prpr/src/core/resource.rs` | `1b5e14b468` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/core/render.rs` | `598a8b0ce7` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/core/block_shader.rs` | `54ac54aa18` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/parse/pgr.rs` | `83c134ed4b` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/scene/game.rs` | `8cc371846f` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/scene/loading.rs` | `dd758714fd` | 是 | 旧研究快照；main 已有更新 |
| `prpr/src/core/render_lifetime.rs` | 不存在 | 否（快照无此文件） | 本地研究材料后续新增 |
| `prpr/src/core/block_support.rs` | 不存在 | 否（快照无此文件） | 本地研究材料后续新增 |
| `prpr/src/loading_cpu.rs` | 不存在 | 否（快照无此文件） | 本地研究材料后续新增 |
| `prpr/src/loading_work.rs` | 不存在 | 否（快照无此文件） | 本地研究材料后续新增 |

- 解释：本地保留快照是 2026-10-06 的旧快照，早于 `render_lifetime.rs`/`block_support.rs`/`loading_cpu.rs`/`loading_work.rs` 的创建；它已有的 6 个核心文件与本地研究材料逐字节相同。本地研究材料后来新增的文件、以及对 `judge.rs`/`chart.rs`/`line.rs` 的修改，都是本地研究材料方向的内容；本地保留快照没有独有实现。
- 结论：**无独有生产优化**。不要把本地保留快照当作“未合入的渲染实验”再次 apply；其中的旧研究代码已经被本地研究材料/主线更新覆盖。

### 8.3 旧版快照

- 位置：旧版快照工作区，是一个独立 Git 仓库。
- 头提交：`0526256b`（`支持彩色噪域并修复白色边框和黄红键点击保护`，pro.11 时期）；`git merge-base --is-ancestor 0526256b main` 为真，即该提交是 main 的祖先。
- 性能文件哈希对照（旧版快照工作区 `git hash-object` 与 main `git rev-parse`）：

| 文件 | 旧版快照哈希（前 10 位） | main 哈希（前 10 位） | 关系 | `0526256b..main` 该文件提交数 |
|---|---|---|---|---|
| `prpr/src/core/resource.rs` | `955099653f` | `5b48ef9dca` | main 更新 | 6 |
| `prpr/src/core/render.rs` | `23901e2914` | `8bdb94e48f` | main 更新 | 1 |
| `prpr/src/core/render_lifetime.rs` | `de04109bc4` | `de04109bc4` | 相同 | 0 |
| `prpr/src/core/block_support.rs` | `017bb9cf97` | `017bb9cf97` | 相同 | 0 |
| `prpr/src/core/block_shader.rs` | `33306c91aa` | `a7ad749e38` | main 更新 | 5 |
| `prpr/src/loading_cpu.rs` | `c1d546ca25` | `2205473699` | main 更新 | 1 |
| `prpr/src/loading_work.rs` | `16ca4a4614` | `16ca4a4614` | 相同 | 0 |
| `prpr/src/parse/pgr.rs` | `de592b6104` | `ca20c66bc3` | main 更新 | 6 |
| `prpr/src/scene/game.rs` | `3ec61b8791` | `f39eeafcc6` | main 更新 | 31 |
| `prpr/src/scene/loading.rs` | `17ac62abd5` | `e9a8df0e4b` | main 更新 | 1 |
| `prpr/src/core/block_mask.rs` | `4973a742d7` | `e083956e8b` | main 更新 | 4 |
| `prpr/src/frame_pacing.rs` | `f4e83217f7` | `cdd275ba1f` | main 更新 | 5 |

- 对“main 更新”的文件，`git rev-list --count 0526256b..main -- <file>` 均大于 0，说明差异来自 main 在 pro.11 之后的正常演进（Mod 图标、viewport、RPE 解析、练习/结算、彩色噪域等），不是旧版快照独有的优化。
- 旧版快照工作区未提交修改只有两个 tracked 文件：`phira-android/app/src/main/java/org/flos/phirapro/QuadSurface.java`（`switch` 的 `default` 由 `break` 改为 `return false`）与 `phira-android/gradle.properties`（Gradle 9.4+ 并行 tooling）；另有 `.idea/` 未跟踪目录。它们是 Android 壳/IDE/构建配置，不是渲染或加载性能代码。
- 结论：旧版快照是 main 的祖先快照，性能文件“相同或 main 更新”，**没有可提取项**。

### 8.4 `git fsck` 的 16 个不可达提交

`git fsck --unreachable --no-reflogs` 统计到 16 个 unreachable commit（另有 465 blob、2 tag、618 tree）。逐提交查看 `git show --name-only` 后分类如下：

| 类别 | 提交 | 说明 |
|---|---|---|
| Weblate 翻译 | `8a06d5cd`、`cc988781`、`65a33199`、`13091909` | 翻译提交/合并，无代码 |
| 联机协议 | `f5067ad8`、`41ec3211`、`30f922ab` | 原生 multiplayer crate 与联调工具 |
| 安卓/HYKB 修复 | `eff4f2cf`、`08f55bd1`、`b07773cf`、`fba83f07` | Android 闪退/clean-room、HYKB 反沉迷 |
| 成绩上传/旧 main 功能 | `a0114ea5` | 自服成绩上传，无渲染/加载实现 |
| 旧 main/rebase/index 副本 | `675d5b85`、`89645de7`、`2d7ff14b`、`2b5c2370` | 与当前 main 同主题的旧副本、合并或 stash index |

- 逐提交扫描 `prpr/src/core/resource.rs`、`render.rs`、`render_lifetime.rs`、`block_support.rs`、`block_shader.rs`、`block_mask.rs`、`loading_cpu.rs`、`loading_work.rs`、`parse/pgr.rs`、`scene/game.rs`、`scene/loading.rs`，只有 `675d5b85` 命中 `scene/game.rs`/`scene/loading.rs`。
- `675d5b85` 与当前 main 的 `resource.rs`、`render.rs`、`loading_cpu.rs`、`loading.rs` 哈希相同；`game.rs` 的差异仅是缺少当前 main 的 `audio.recover_if_needed()` 与 `invalidate_render_surface()` 生命周期修复，属于旧副本，不是额外优化。其余 15 个提交没有修改上述性能文件。
- 结论：16 个不可达提交是翻译、联机、安卓修复、成绩上传、rebase 旧版，**没有性能代码**；不要从 `git fsck` 结果中恢复并提取“未合入优化”。

### 8.5 归档探针目录

- 位置：归档探针目录，共 6 个归档项目（README 称“六个独立探针”），281 个文件；README 明确“不作为当前生产构建入口”。
- 项目与性质：

| 项目 | 类型 | 内容 | 与 main 的关系 |
|---|---|---|---|
| `render-bench` | 基准/原型 | arena、连续 pass、离屏保留、专用实例、像素比较；含 Android 与 baseline 对照 | 其生产子集（arena/pass/offscreen）经 `78445c20` 与固定库 rev 进入 main |
| `glyph-stream-probe` | 探针 | 字形上传/flush 顺序、纹理读取依赖测量 | 研究-only，未形成生产实现 |
| `loading-work-probe` | 探针 | 加载期 CPU 工作后台化可行性 | 生产等价物在 `loading_cpu.rs`/`loading_work.rs`（main） |
| `mask-values-probe` | 探针 | R8 有限域查表穷举与像素对照 | 研究-only；`block_mask_values.rs` 不在 main |
| `playback-ir-probe` | 探针 | 500 万音符 24B token 表示与流式验证 | 研究-only，未形成播放 IR 生产实现 |
| `spike-timing-probe` | 探针 | Loading/Ending/Gameplay 阶段计时 | 测量工具，不是渲染实现 |

- 生产子集：`render-bench` 的 arena/连续 pass/离屏保留与 `loading-work-probe` 的后台加载思路已经在 main（`78445c20` 的 16 文件与固定库 rev）；其余探针只提供测量或原型证据，没有进入生产，也不构成“本地独有的生产优化”。
- 结论：归档探针目录是**原型/证据**，不是生产实现；不要直接复制其中的 probe 代码作为待实施方案。若要做新的 glyph/IR/查表优化，应作为新候选重新立项、重新验证，而不是“从归档提取已完成优化”。

---

## 9. 总表

| 项目 | 状态 | 证据 | 备注 |
|---|---|---|---|
| 共享 geometry arena + 连续同目标 pass + 离屏保留 | `已实施（main）` | `78445c20`；`prpr/src/core/resource.rs:361`、`game.rs:2381`；macroquad/miniquad rev `139ac95`/`1796f44`；Happy 53.644→34.400 ms、render 51.647→2.662 ms（约 19×）；Petrichor 58.481→39.736 ms；离屏 15.34/18.27→0.74/2.35 ms | 普通路径无开关；`f41ab7db` 追加 NoteMeshes 跨帧复用；不要再实施 |
| 隐式 MSAA + 目标生命周期 + 配对效果 | `已实施（main）` | `78445c20`；`render_lifetime.rs` 自合入后 0 修改；p50 32.5→17.8/16.7 ms；配对 46.361→60.049 fps；15 项 RGBA 相同 | 自动能力选择；失败完整回退显式 MSAA；不要再实施 |
| block 支持域 bbox scissor | `已实施（main）` | `78445c20`；`block_support.rs` 0 后续修改；`block_shader.rs:620-626`；密集段 39.803→36.044 ms（9.4%）；组合 39.756→35.539 ms；9 项 RGBA 相同 | 仅非 hover；透视/非仿射回退；分区裁剪仍是研究选项 |
| 加载期 CPU 工作后台化 | `已实施（main）` | `78445c20`；`loading_work.rs` 0 后续修改；798.002→162.152 ms / 848.297→160.094 ms；6 项 RGBA、90 项输出相同 | 普通路径默认；剩余 158–163 ms 是后续尖峰，不是未实施 |
| JSON 谱面后台解析 | `已实施（main）` | `78445c20`；`parse_phigros_loading`；161.682→63.247 ms / 160.316→64.309 ms；6 项 RGBA、96 项输出相同 | 只搬 owned serde 模型；Chart/GL 仍主线程 |
| block area 性能修复 | `已实施（main）` | `77298da3`、`9c85d587`、`cb70d098`；本地参考实现头 `776b8108` 是 main 祖先；`18a6b021` 是同步提交，无 block area 新增；ハテ 11–31%、8000 重复 1740→2.5 ms、Desultory 7–9% | 不要把 `18a6b021` 当合入点；不要从本地参考实现重复提取 |
| render-production 整合 | `已实施（main）` | 本地参考实现头 `69f31f8e` 是 main 祖先；`78445c20` + 16 生产文件；PR #7 `b3040dd5`；119 个后续提交均为应用层 | 是生产子集整合，不是独立新优化；不要再整合 |
| 仅含测量载体的本地归档 | 无独有 | `987ea164`；7 个测量/harness 文件、2163 行；7 路径在 main 全部不存在 | 只保存研究工具，不是生产优化 |
| 本地保留快照 | 无独有 | `2b89e03c`；diff 120 files/66734+/92−；状态 0 D；6 个核心文件与本地研究材料逐字节相同；4 个文件快照不存在 | 旧快照；不要 apply |
| 旧版快照 | 无独有 | 头 `0526256b` 是 main 祖先；3 个性能文件相同、其余 main 有 1–31 个后续提交；未提交仅 Android/Gradle | 不要从旧版快照提取 |
| `git fsck` 16 个不可达提交 | 无独有 | 翻译/联机/安卓/成绩上传/rebase 旧版；仅 `675d5b85` 命中 game/loading，且为旧副本、无额外优化 | 不要从 fsck 恢复 |
| 归档探针目录 | 无独有 | 6 个归档项目/281 文件；生产子集已在 main；其余为探针/证据 | 不要直接复制 probe 代码 |

---

## 10. 使用约束：本附录不计入“待实施正优化”

1. **第 1～7 节全部是 `已实施（main）`，不进入待实施清单。** 任何后续计划、任务拆分、命名或实验申请都不得把共享 arena、连续 pass、离屏保留、隐式 MSAA、配对效果、bbox scissor、加载 CPU 后台化、JSON 后台解析、block area 性能修复、render-production 整合再次列为“待实施”。
2. **第 8 节全部是“无独有”，不进入待实施清单，也不作为“可提取代码”的来源。** 本地归档、本地保留快照、旧版快照、fsck 提交、归档探针目录都不能提供新的生产优化；从这些位置恢复代码只会得到旧快照或研究原型。
3. **验证缺口 ≠ 实施缺口。** 本文多次注明“更多谱面时刻、hover、自定义材质、其他 GPU、整首/系统呈现未覆盖”。这些是验证/回归覆盖问题；若要继续，应单独立“验证项”，而不是把已经实现的机制重新列入“正优化”。
4. **重复实施的判定标准。** 如果一个候选的核心机制已经出现在本文第 1～7 节的关键文件/代码块中，或已经被第 8 节证明只存在于旧快照/本地参考实现，则判定为“重复”，应直接引用本附录拒绝，不再安排设备测量。
5. **不要改写本文结论来支持新的本地实验。** 若未来确实需要改动机制（例如更换 MSAA 策略、重做 arena、改变 block 支持域算法），那是新的架构变更，必须重新给出独立的能力探测、回退、像素与性能证据；不能以“优化附录说已经做过”为由跳过验证，也不能把旧数据直接当新实现的证据。
6. **维护方式。** 若后续 main 又合入新的性能提交并通过验收，应新增 `已实施（main）` 条目并给出提交、关键文件、完整数据与排除理由；不要删除本附录中的历史条目，否则后续审计会重新把旧项当成待实施。

**结论：本文第 1～7 节是 `已实施（main）` 的主线优化，第 8 节是无独有内容的位置；两者都不计入“待实施正优化”，不要重复实施。**
