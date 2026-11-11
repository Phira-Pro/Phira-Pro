# Phira-Pro 非正优化详细文档（N-01 至 N-17）

> **主线实施状态总则：本文所有条目在 Phira-Pro `main` 中均未实施，也未达到最低可交付验证。** 只有相关改动已合入 `main` 并达到最低验证，才算进入主线；参考实现、可编译、默认启用或真机跑过都不等于进入主线。
>
> 本文的“已实现”“已落地”“默认启用”只描述参考实现，不表示相关改动已进入 `main` 并达到最低验证。非正优化不建议直接合入；如果未来条件变化需要重开，必须先按本文的复现条件和验收门槛重新测量，再决定是否进入 `main`。

## 非正优化总状态表

| ID | 优化项 | 判定 | 主线是否已实施 | 参考实现状态 | 参考实现默认 | 一句话结论 |
| --- | --- | --- | --- | --- | --- | --- |
| N-01 | 图集裁切 | 无优化 | **否** | 完整 | 关闭 | 上传仍约为原路径 297 倍 |
| N-02 | 图集双通道 RG | 无优化/混合 | **否** | 完整 | 关闭 | 主组 −2.09%，独立对照 +1.87% 变慢 |
| N-03 | 冻结文字图集 | 负优化 | **否** | 完整 | 关闭 | packed Main low +1.19%，phase4 有损 |
| N-04 | 粒子小缓冲/stream discard | 负优化/混合 | **否** | 完整 | 关闭 | 一组 ABBA +6.28% |
| N-05 | UI 图片后台解码 | 负优化 | **否** | 完整 | 关闭 | Loading max 改善，但全窗口 p99/max 变差 |
| N-06 | 加载/模型分帧 | 无优化 | **否** | 完整 | 关闭 | 尖峰无改善 |
| N-07 | glyph scoped flush | 负优化 | **否** | 完整 | 关闭 | Exo 最大间隔 65.1→69.1ms |
| N-08 | mask 有限域查表+逐行 bbox | 无优化（局部微正） | **否** | 完整 | 关闭 | 局部约 0.85ms，整帧无感 |
| N-09 | mask ring/viewport/keep_resolved | 无优化 | **否** | 完整 | 关闭 | 无可靠整帧收益 |
| N-10 | snapshot blit/shader copy 回退 | 负优化（失败） | **否** | 完整 | 关闭 | 254,520 通道不同，最大差 71 |
| N-11 | GPU 常驻音符 IR v1/v2/v3 | 负优化 | **否** | 完整 | 关闭 | v1/v2 变慢，v3 不稳定 |
| N-12 | 可见性预编译 | 负优化 | **否** | 完整 | 关闭 | v1 +21.31%，fast +1.27% |
| N-13 | 渲染取值计划 | 负优化 | **否** | 完整 | 关闭 | v1 +9.96%，v2 +7.45% |
| N-14 | 图集区域合并 | 负优化（观察） | **否** | 完整 | 关闭 | 慢 0.27%，poll 172→444 |
| N-15 | 固定/不可变纹理存储 app 侧 | 负优化（质量失败，性能未测） | **否** | 原型 | 关闭 | 质量门失败，性能未测 |
| N-16 | PBO 字库上传（完整帧） | 无优化 | **否** | 完整 | 关闭 | submit 降，但 completed 无收益 |
| N-17 | 背景融合/低频纹理缓存 | 无优化 | **否** | 原型 | 关闭 | 无稳定 gameplay 收益 |

---
## 第一部分：N-01 至 N-05

本文记录五项都已有参考实现、并做过真机或离线对照的“非正优化”候选：图集裁切、图集双通道 RG、冻结文字图集、粒子小缓冲/stream discard、UI 图片后台解码。五项都不是“没有实现”，而是“实现了、默认关闭、没有进入 main”；本文的重点是逐项说明它们为什么没有成立，以及在什么条件下才可能值得重新考虑。

阅读约定：

- “应用墙钟”指 `update + render + flush` 的 CPU/driver 墙钟，包含驱动执行、调度与等待，不包含 swap，也不等于 GPU 独占耗时。
- “呈现间隔”指 SurfaceFlinger 的真实呈现时间戳间隔，和应用墙钟不是同一个口径。
- “同 SO”指同一份 `.so`；ABBA 指同一轮内按 A-B-B-A 交错，避免把时间漂移当成优化收益。
- 除非特别注明，设备均为 `8DF6R17309002674`：EDI-AL10 / hi3650 / Android 8 / arm64-v8a，Mali-T880 / OpenGL ES 3.2 v1.r20p0，画面 2560×1440（3,686,400 像素）。
- 测试谱面为 `What do you want more than a Happy ending.zip`，AT.json / PGR v3，24 线、1568 notes；使用 autoplay，不代表手动输入验收。
- “参考实现已落地”指参考实现已提交并完成研究验证；“默认关”指请求字段/开关缺省为 false；“未进 main”指生产代码没有 Config/UI 入口、没有默认启用、未完成主线验收。
- 本文只使用本地提交、SO 哈希、原始 JSON/CSV 和日志做溯源；不引用外部网页。

---

## N-01 图集裁切（atlas crop）

### 1. 一句话结论与判定

判定：**无优化**。裁切把“整图重命名”原型每轮约 2.08 GB 的图集上传降到 494,927,872 bytes（约 472 MiB），相对整图版本确实减少约 4.2 倍；但相对原路径每轮约 1.6 MB 的局部 `glTexSubImage2D` 仍是约 297 倍，而且 59.71–59.79 fps 只比整图版本的 59.51–59.55 fps 高 0.2–0.5 个百分点，落在同一批运行的漂移范围内。裁切没有改变“每次发布重建一整张物理图集”的成本结构，只是把整张图的高度从 2048 降到本次已写入区域的 2 次幂高度；它不是一个相对原路径成立的优化。

关键数字：

- 原路径上传：1,664,600 / 1,691,240 bytes（约 1.6 MB）。
- 整图替换上传：2,046,820,352 / 2,113,929,216 bytes，两轮均值 2,080,374,784 bytes（约 1.94 GiB）。
- 裁切上传：494,927,872 bytes × 2 轮。
- 应用 FPS：原路径 57.355 / 57.368；整图 59.545 / 59.513；裁切 59.709 / 59.788。
- 四组固定/动态、MSAA=1/2 末帧 RGBA 逐像素相同（max difference = 0）。

因此本条不能记作“图集上传优化已成立”。它证明的是：在一个本来就会整图重传的极端原型里，按已写高度裁切可以少传约四分之三；但这个原型本身不是生产路径，裁切后的绝对值仍远高于生产路径的脏矩形上传。

### 2. 问题与初衷

游戏内 HUD 字体图集是 2048×2048 RGBA8，单张 16 MiB。原路径每次字形位图变化只对脏矩形调用 `update_texture_part`，一轮 1200 帧约上传 1.6 MB；问题是原路径在某些设备/驱动上会在“刚被采样过的图集上写入”时产生读写依赖等待，HUD 出现 20–50 ms 长帧。

研究原型 `font_atlas_rename` 的做法是：保留原 GlyphBrush、栅格、排布、UV 和文字几何，只额外维护一份 CPU 镜像；每次字形变化后发布一张全新的 GL 图集，避免写入正在被采样的旧图集。这个原型把 HUD >20 ms 帧从 67/80 降到 7/7，但同时每轮上传约 1.94 GiB，代价不可接受。

裁切（atlas crop）的初衷是给这个“整图重命名”原型降上传量：逻辑图集仍是 2048×2048，UV 仍按逻辑高度 2048 计算；但物理纹理只创建到“已写入区域的最大 y 向上取 2 次幂”的高度，并在顶点阶段把 V 坐标乘以 `逻辑高度 / 物理高度`。这样图集上半部分没有字形时，就不必为空白区域付上传和显存带宽。

### 3. 实现思路与具体机制

1. **保留逻辑排布**：GlyphBrush 仍把字形排进 2048×2048 的逻辑空间，`rect`、UV、字形代表（representative）、行高和透明顺序都不变。裁切只改变“物理图像有多高”，不改变“字形在图里的位置”。
2. **CPU 镜像**：`lab_atlas_shadow` 保存逻辑图集的 RGBA 字节。初始按 2048×2048 分配；裁切模式下先给空 `Vec`，等第一次写入时按需扩容。
3. **每次脏矩形写入**：把 `data_buffer` 的矩形逐行复制进 shadow；同时记录本次写入的 `rect.max[1]`。
4. **计算物理高度**：`required_height = rect.max[1].max(1).next_power_of_two().min(atlas_height)`。只有逻辑高度本身是 2 的幂时才启用；否则回退整图高度。
5. **发布**：`physical_height = shadow.len() / width / 4`；创建 `width × physical_height` 的新纹理；把 `lab_atlas_uv_y` 设为 `逻辑高度 / physical_height`；删除旧纹理，替换 `cache_texture`。
6. **顶点重映射**：绘制每个顶点时使用 `v = vertex.uv.1 * lab_atlas_uv_y`。因为线性过滤在物理纹理的 0..1 范围内采样，乘上比例后，逻辑 V 仍映射到原来的字形行。
7. **池化**：`Pool` 保留 4 个逻辑槽位，按 fence 判断旧图是否还被 GPU 读取；没有安全槽位时发布新图，绝不等待或覆盖。裁切把 `physical_height` 传给池，池用 `(width, physical_height)` 作为 `shape`。
8. **形状变化回退**：如果物理高度增长（例如后续字形写到更靠下的位置），`publish` 发现 `shape` 不匹配，会 `clear()` 整个池并重新 `warm()` 四个新尺寸的槽位；这会产生一次新的整池分配，是裁切路径的隐藏成本。
9. **非 2 次幂回退**：逻辑图集高度不是 2 的幂时，`required_height` 直接等于 `atlas_height`，等价于整图，不做裁切。

需要特别说明：裁切的数学不在 `atlas_pool.rs` 内部，而是在 `TextPainter` 的发布路径里；`atlas_pool.rs` 的职责是接受一个“物理高度”并管理 4 个版本。把裁切说成池的功能会误导读者，因此下面的代码同时给出两处：`text.rs` 的裁切发布逻辑，以及 `atlas_pool.rs` 的 `warm` / `publish` 入口。两者合起来才是完整机制。

### 4. 文件、符号与完整核心代码

裁切相关符号：

- `prpr/src/ui/text.rs`：`TextPainter::lab_atlas_shadow`、`lab_atlas_compact`、`lab_atlas_uv_y`、`TextPainter::research_atlas_compact`、`TextPainter::research_publish_atlas`、`submit` 内的 bitmap 回调、`resize_texture` 分支、`redraw` 内的 V 重映射。
- `prpr/src/ui/atlas_pool.rs`：`Pool::warm`、`Pool::publish`、`Pool::clear`、`Pool::report`。
- 研究请求字段：`font_atlas_rename`、`font_atlas_compact`（两者必须同时为 true 才进入裁切路径）。

#### 4.1 `TextPainter` 的裁切状态与开关

```rust
pub struct TextPainter {
    primary_scale: f32,
    ink_cache: InkCache,
    brush: GlyphBrush<[MyVertex; 4]>,
    cache_texture: Texture2D,
    data_buffer: Vec<u8>,
    #[cfg(feature = "render_research")]
    lab_atlas_shadow: Option<Vec<u8>>,
    #[cfg(feature = "render_research")]
    lab_atlas_compact: bool,
    #[cfg(feature = "render_research")]
    lab_atlas_uv_y: f32,
    #[cfg(feature = "render_research")]
    lab_prepare_only: bool,
    #[cfg(feature = "render_research")]
    lab_defer_publication: bool,
    #[cfg(feature = "render_research")]
    lab_shadow_dirty: bool,
    #[cfg(feature = "render_research")]
    lab_atlas_pool: atlas_pool::Pool,
    #[cfg(feature = "render_research")]
    lab_atlas_writes: Vec<[u32; 4]>,
    vertices_buffer: Vec<MyVertex>,
    draw_vertices: Vec<Vertex>,
    draw_indices: Vec<u16>,
}

impl TextPainter {
    #[cfg(feature = "render_research")]
    pub fn research_atlas_rename(enabled: bool) {
        LAB_ATLAS_RENAME.with(|slot| slot.set(enabled));
    }
    #[cfg(feature = "render_research")]
    pub fn research_atlas_compact(enabled: bool) {
        LAB_ATLAS_COMPACT.with(|slot| slot.set(enabled));
    }
}
```

#### 4.2 `research_publish_atlas`：按物理高度发布

```rust
#[cfg(feature = "render_research")]
fn research_publish_atlas(&mut self) {
    if self.lab_shadow_dirty {
        let upload_begin = std::time::Instant::now();
        let (width, height) = self.brush.texture_dimensions();
        assert!(width <= u16::MAX as u32 && height <= u16::MAX as u32);
        let shadow = self.lab_atlas_shadow.as_ref().unwrap();
        let physical_height = (shadow.len() / width as usize / 4) as u32;
        assert!(physical_height > 0 && physical_height <= height);
        self.lab_atlas_uv_y = height as f32 / physical_height as f32;
        let result = self
            .lab_atlas_pool
            .publish(self.cache_texture, width, physical_height, shadow, &self.lab_atlas_writes);
        if !result.previous_owned {
            self.cache_texture.delete();
        }
        self.cache_texture = result.texture;
        self.lab_atlas_writes.clear();
        lab_text_add(1, result.bytes as f64);
        lab_text_add(2, upload_begin.elapsed().as_secs_f64() * 1000.);
        self.lab_shadow_dirty = false;
    }
}
```

这个函数是裁切路径的核心：`physical_height` 不是逻辑高度，而是 shadow 实际长度换算出的高度；`lab_atlas_uv_y` 就是逻辑高度与物理高度的比值。池只看到物理高度，不关心裁切策略。

#### 4.3 `submit` 内的 bitmap 回调：计算 2 次幂高度并写 shadow

```rust
let (atlas_width, atlas_height) = atlas_dimensions;
let required_height = if self.lab_atlas_compact && !self.lab_atlas_pool.supported() && atlas_height.is_power_of_two() {
    rect.max[1].max(1).next_power_of_two().min(atlas_height)
} else {
    atlas_height
};
let atlas_width = atlas_width as usize;
let needed = atlas_width * required_height as usize * 4;
if shadow.len() < needed {
    shadow.resize(needed, 0);
}
let row_bytes = rect.width() as usize * 4;
for row in 0..rect.height() as usize {
    let to = ((rect.min[1] as usize + row) * atlas_width + rect.min[0] as usize) * 4;
    shadow[to..to + row_bytes].copy_from_slice(&self.data_buffer[row * row_bytes..(row + 1) * row_bytes]);
}
shadow_dirty = true;
self.lab_atlas_writes.push([rect.min[0], rect.min[1], rect.max[0], rect.max[1]]);
lab_text_add(0, 1.);
return;
```

这段代码只执行到“阴影写入 + 记录”为止，真正的 GL 发布由循环末尾的 `research_publish_atlas()` 完成。`required_height` 只在“池不支持”时启用；一旦池可用（`atlas_pool` 路径），高度由 shadow 长度决定，池会按 `physical_height` 创建。这个条件容易误读，实际两套路径最终都使用物理高度：裁切直发布走 `next_power_of_two`，池发布走 shadow 长度。

#### 4.4 图集尺寸变化时重置 shadow 与 UV

```rust
self.brush.resize_texture(suggested.0, suggested.1);
#[cfg(feature = "render_research")]
if self.lab_atlas_shadow.is_some() {
    self.lab_atlas_shadow = Some(if self.lab_atlas_compact {
        Vec::new()
    } else {
        vec![0; suggested.0 as usize * suggested.1 as usize * 4]
    });
    self.lab_atlas_uv_y = 1.;
    self.lab_shadow_dirty = false;
}
```

图集扩容/缩容会丢弃旧 shadow 并重置 UV 比例。裁切模式下新 shadow 为空，等待第一次写入时按需扩容；这避免为未使用图集预分配整张 16 MiB。

#### 4.5 `redraw` 内的 V 重映射

```rust
#[cfg(feature = "render_research")]
let v = vertex.uv.1 * self.lab_atlas_uv_y;
#[cfg(not(feature = "render_research"))]
let v = vertex.uv.1;
self.draw_vertices.push(Vertex::new(pos.x, pos.y, 0., vertex.uv.0, v, color));
```

这是裁切能够保持画面不变的关键：逻辑 UV 不变，只在提交顶点时压缩到物理高度。若 `lab_atlas_uv_y = 1.0`，等价于整图。

#### 4.6 `atlas_pool.rs` 的图像构造与裁切入口

```rust
fn image(width: u32, height: u32, bytes: &[u8]) -> Texture2D {
    assert!(width <= u16::MAX as u32 && height <= u16::MAX as u32);
    let texture =
        Texture2D::from_miniquad_texture(miniquad::Texture::from_rgba8(unsafe { get_internal_gl() }.quad_context, width as _, height as _, bytes));
    texture.set_filter(FilterMode::Linear);
    texture
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Clamp);
    texture
}
```

```rust
pub fn warm(&mut self, width: u32, height: u32, shadow: &[u8]) -> Option<Texture2D> {
    let api = self.api?;
    let bytes = width as usize * height as usize * 4;
    assert_eq!(shadow.len(), bytes);
    if bytes.checked_mul(SLOTS).is_none_or(|v| v > BUDGET) {
        self.failure("Atlas pool exceeds 64MiB budget".into());
        return None;
    }
    if !self.slots.is_empty() {
        return self.current;
    }
    self.shape = (width, height);
    self.history = Some(Delta::new(width, height));
    self.scratch = vec![0; 1024 * 1024];
    self.scratch.clear();
    for _ in 0..SLOTS {
        let texture = image(width, height, shadow);
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
            texture,
            version: 0,
            fence: Some(fence),
        });
        if unsafe { miniquad::gl::glGetError() } != 0 {
            self.failure("Atlas pool loading allocation/probe GL error".into());
            return None;
        }
    }
    self.current = Some(self.slots[0].texture);
    self.current
}
```

```rust
pub fn publish(&mut self, previous: Texture2D, width: u32, height: u32, shadow: &[u8], writes: &[[u32; 4]]) -> Publication {
    let previous_owned = self.slots.iter().any(|slot| slot.texture == previous);
    if self.shape != (width, height) && !self.slots.is_empty() {
        self.clear();
    }
    if self.api.is_some() && self.history.is_none() {
        if let Some(texture) = self.warm(width, height, shadow) {
            self.fresh += 1;
            return Publication {
                texture,
                bytes: shadow.len() * SLOTS,
                previous_owned,
            };
        }
    }
    if let Some(api) = self.api {
        let history = self.history.as_mut().unwrap();
        history.advance(writes);
        if let Some(index) = self.slots.iter().position(|slot| Some(slot.texture) == self.current) {
            if let Some(fence) = self.slots[index].fence.take() {
                api.delete(fence);
            }
            match api.insert() {
                Ok(fence) => self.slots[index].fence = Some(fence),
                Err(reason) => self.failure(reason),
            }
        }
        if self.api.is_some() {
            let mut available = None;
            if self.mode != 3 {
                for (index, slot) in self.slots.iter_mut().enumerate() {
                    if Some(slot.texture) == self.current {
                        continue;
                    }
                    let ready = if let Some(fence) = slot.fence {
                        self.polls += 1;
                        match api.ready(fence) {
                            Ok(ready) => ready,
                            Err(reason) => {
                                self.reason = Some(reason);
                                break;
                            }
                        }
                    } else {
                        true
                    };
                    if ready {
                        available = Some(index);
                        break;
                    }
                }
            }
            if let Some(reason) = self.reason.clone() {
                self.failure(reason);
            } else if let Some(index) = available {
                let slot = &mut self.slots[index];
                if let Some(fence) = slot.fence.take() {
                    api.delete(fence);
                }
                let history = self.history.as_ref().unwrap();
                let mut transferred = 0;
                for rect in history.since(slot.version) {
                    self.scratch.clear();
                    self.scratch.reserve((rect.w * rect.h * 4) as usize);
                    for y in rect.y..rect.y + rect.h {
                        let start = ((y * width + rect.x) * 4) as usize;
                        self.scratch.extend_from_slice(&shadow[start..start + (rect.w * 4) as usize]);
                    }
                    slot.texture.raw_miniquad_texture_handle().update_texture_part(
                        unsafe { get_internal_gl() }.quad_context,
                        rect.x as _,
                        rect.y as _,
                        rect.w as _,
                        rect.h as _,
                        &self.scratch,
                    );
                    transferred += self.scratch.len();
                }
                slot.version = history.generation();
                let texture = slot.texture;
                if unsafe { miniquad::gl::glGetError() } == 0 {
                    self.current = Some(texture);
                    self.reused += 1;
                    self.delta_bytes += transferred;
                    return Publication {
                        texture,
                        bytes: transferred,
                        previous_owned,
                    };
                }
                self.failure("Completed-slot delta upload GL error".into());
            } else {
                self.busy += 1;
                let removed = self.slots.remove(0);
                if let Some(fence) = removed.fence {
                    api.delete(fence);
                }
                removed.texture.delete();
                let texture = image(width, height, shadow);
                let version = self.history.as_ref().unwrap().generation();
                self.slots.push(Slot {
                    texture,
                    version,
                    fence: None,
                });
                self.current = Some(texture);
                self.fresh += 1;
                self.full_bytes += shadow.len();
                return Publication {
                    texture,
                    bytes: shadow.len(),
                    previous_owned,
                };
            }
        }
    }
    let texture = image(width, height, shadow);
    self.fresh += 1;
    self.full_bytes += shadow.len();
    Publication {
        texture,
        bytes: shadow.len(),
        previous_owned,
    }
}
```

注意 `publish` 的 `bytes` 语义：走“复用旧槽 + 增量矩形”时是 `transferred`，走“新槽/新图”时是 `shadow.len()`（整张物理图）。裁切只降低 `shadow.len()` 的高度，不改变“有新槽就整张传”的事实。

#### 4.7 池的清理与预算

```rust
fn clear(&mut self) {
    for slot in self.slots.drain(..) {
        if let Some(fence) = slot.fence {
            if let Some(api) = self.api {
                api.delete(fence);
            }
        }
        slot.texture.delete();
    }
    self.current = None;
    self.history = None;
    self.shape = (0, 0);
}
```

```rust
const SLOTS: usize = 4;
const BUDGET: usize = 64 * 1024 * 1024;
```

`BUDGET` 是 64 MiB，按 4 个槽位算，单槽最多 16 MiB；完整 2048×2048 RGBA 单槽正好 16 MiB。裁切后单槽更小，但池仍按 4 槽管理。

### 5. 开关、默认值、构建特性

| 项目 | 值 | 说明 |
|---|---|---|
| 研究请求字段 | `font_atlas_rename` | 打开 CPU 镜像 + 重命名发布路径 |
| 研究请求字段 | `font_atlas_compact` | 打开按已写高度裁切；只有 rename=true 时才有效 |
| 默认值 | 两个字段都 false | 请求缺省即关闭 |
| 构建特性 | `render_research` | 研究后端总开关 |
| 构建特性 | `render_atlas_pool` | 图集池；与 `font_atlas_pool` 不同，前者只给研究后端 |
| 生产默认 | 不启用 | 主项目没有把 `font_atlas_compact` 接入 Config/UI |
| 逻辑图集 | 2048×2048 RGBA8 | 由 `TEXTURE_DIM` 取 `GL_MAX_TEXTURE_SIZE` 与 2048 的较小值 |
| 池预算 | 64 MiB | 4 槽 × 单槽 ≤16 MiB |
| 回退条件 | 逻辑高度非 2 次幂 / 池不支持 / 形状变化 | 回退整图或重建池 |

这里的“默认关”不是“没实现”：裁切代码已经在研究后端里完整实现并通过真机画面对照；只是没有生产入口、没有默认开启、没有进 main。

### 6. 完整测量数据

设备：`8DF6R17309002674`，EDI-AL10 / hi3650 / Android 8 / arm64-v8a，Mali-T880 / OpenGL ES 3.2 v1.r20p0。画面 2560×1440，3,686,400 像素。谱面为 `What do you want more than a Happy ending`，AT.json / PGR v3，24 线、1568 notes；autoplay；低画质、sampleCount=1、无背景融合、无 GPU timer。

窗口：真实时钟，120 预热 + 1200 采样帧；顺序 control / rename / compact / compact / rename / control；同一后端 `f6f95cde2dc4300ca0773cceb3d77e2bbe1b62c52f3298f389af59c082caad16`。独立呈现约 9.2–9.5 s，不是全曲。

| 次序 | 策略 | 应用 FPS | 应用间隔 p99 ms | 应用间隔 max ms | 呈现 p99 ms | 呈现 max ms | HUD >20 ms 帧 | HUD max ms | 上传 bytes |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 原路径 | 57.355 | 46.380 | 56.402 | 50.201 | 50.213 | 67 | 50.496 | 1,664,600 |
| 1 | 整图替换 | 59.545 | 24.619 | 34.432 | 16.795 | 33.476 | 8 | 26.428 | 2,046,820,352 |
| 2 | 裁切 | 59.709 | 18.983 | 32.738 | 16.748 | 16.771 | 1 | 26.947 | 494,927,872 |
| 3 | 裁切 | 59.788 | 18.953 | 29.861 | 16.768 | 19.239 | 1 | 24.063 | 494,927,872 |
| 4 | 整图替换 | 59.513 | 26.525 | 37.090 | 16.780 | 33.470 | 12 | 25.472 | 2,113,929,216 |
| 5 | 原路径 | 57.368 | 44.660 | 58.619 | 44.680 | 50.203 | 80 | 52.945 | 1,691,240 |

补充：

- 两轮整图上传的均值是 `(2,046,820,352 + 2,113,929,216) / 2 = 2,080,374,784` bytes，约 1.94 GiB，约 2.08 GB（十进制）。
- 裁切上传 494,927,872 bytes，是整图均值的 23.8%，约 1/4.2；是原路径约 1.67 MB 的 296 倍。
- 裁切 FPS 59.709/59.788，整图 59.545/59.513；裁切只高 0.164/0.275 fps，即 0.28%/0.46%。
- 裁切相对原路径高 2.35/2.42 fps（约 4.1%），但整图相对原路径已经高 2.19/2.15 fps；裁切相对整图的增量只有约 0.2–0.5%。
- 裁切两轮剩余 HUD 长帧都在 chart≈76.715 s，上传 8 MiB，HUD≈24/27 ms；上传调用≈14.6/17.5 ms，字形处理≈9 ms。说明裁切没有消除“增长/较大资源发布”造成的长帧。

### 7. 质量与回归结果

质量对照为四组 2560×1440 末帧 RGBA：MSAA=1 固定时刻、MSAA=1 动态 240 帧、MSAA=2 固定时刻、MSAA=2 动态 240 帧。每组都是原路径 vs 裁切。

| 对照 | 分辨率 | max RGBA 差异 | 各通道平均绝对差 | 结果 |
|---|---|---:|---|---|
| MSAA=1 固定 | 2560×1440 | 0 | [0,0,0,0] | 逐像素相同 |
| MSAA=1 动态 | 2560×1440 | 0 | [0,0,0,0] | 逐像素相同 |
| MSAA=2 固定 | 2560×1440 | 0 | [0,0,0,0] | 逐像素相同 |
| MSAA=2 动态 | 2560×1440 | 0 | [0,0,0,0] | 逐像素相同 |

这四组只覆盖固定谱面、固定字体、固定设备和有限时刻；不能据此宣称所有字体、所有 DPI、所有 GPU、所有图集尺寸都无损。尤其：

- 裁切依赖 `next_power_of_two`，逻辑高度不是 2 次幂时直接回退，因此“回退也是正确”的证明只在代码路径上成立，没有单独设备画面对照。
- V 重映射依赖线性过滤和 clamp；如果字形行正好落在物理图集边界，半像素采样可能不同。该实验的理论边界是“矩形已写入高度 ≥ 实际使用行”，没有覆盖所有 glyph 排布。
- 池在形状变化时会清空重建；重建期间如果旧图仍被采样，池用 4 槽和 fence 规避，但没有证明所有驱动都正确。

### 8. 为什么判定为无/负（逐项对比）

| 比较项 | 原路径 | 整图替换 | 裁切 | 裁切是否成立 |
|---|---:|---:|---:|---|
| 每轮上传 | 1.66–1.69 MB | 2.05–2.11 GB | 494.93 MB | 比整图少 4.2×，比原路径多 296× |
| 应用 FPS | 57.355–57.368 | 59.513–59.545 | 59.709–59.788 | 相对整图 +0.28%–0.46%，落在漂移内 |
| HUD >20 ms | 67–80 | 8–12 | 1–1 | 相对整图改善，但原路径本身不是同一上传结构 |
| 呈现 p99 | 44.68–50.20 ms | 16.78–16.80 ms | 16.75–16.77 ms | 相对整图基本持平 |
| 剩余长帧 | 50–53 ms | 25–26 ms | 24–27 ms | 没有消除 76.715 s 的 8 MiB 发布尖峰 |
| 质量 | 基准 | RGBA 相同 | RGBA 相同 | 质量成立 |
| 生产可替代性 | 生产路径 | 极端原型 | 仍是整图发布 | 不能替代脏矩形上传 |

逐项说明：

1. **上传量**：裁切确实把整图版本的 2.08 GB 降到 494.9 MB，这是实打实的局部改进；但原路径只传 1.66 MB，裁切仍是它的 296 倍。以“减少上传”为目标时，裁切没有把方案带到生产路径的量级。
2. **帧时间**：裁切相对整图只快 0.16–0.28 fps；而同一批 control 前后差 0.013 fps、整图前后差 0.032 fps，裁切增量与这些漂移同量级。不能把 59.71–59.79 当成稳定收益。
3. **尖峰**：裁切把 HUD >20 ms 从 67/80 降到 1/1，这是相对“整图重命名”原型的改善；但原路径的 67/80 本身是“写入被采样图集”的驱动等待，不是上传量本身。裁切没有改变“发布新整图”的同步结构，所以 76.715 s 仍出现 24–27 ms HUD 帧。
4. **成本**：裁切需要 CPU shadow、每次发布新建纹理、池的 4 槽管理；形状变化时还会清空重建。它把“整图上传”换成“较小的整图上传”，没有消除分配/发布/驱动同步的成本。
5. **结论**：如果目标是“比整图原型少传”，裁切成立；如果目标是“比生产原路径更优”，裁切不成立。因此本条判定为无优化，而不是“负优化”——它没有让原路径变慢，但也没有给出可替代原路径的方案。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：研究后端 `f6f95cde2dc4300ca0773cceb3d77e2bbe1b62c52f3298f389af59c082caad16` 中完整实现；真机 quality 4 组、performance 6 轮均执行 |
| 默认 | 关闭；`font_atlas_compact` 缺省 false；只有 `font_atlas_rename=true` 时才进入裁切路径 |
| 普通路径 | 未接入；生产代码没有 `font_atlas_compact` 字段、没有 Config/UI 开关、没有默认开启；`atlas_pool.rs` 本身是研究后端文件，生产构建即使带 `font_atlas_pool` 也不会走这条裁切发布路径 |
| 主线是否已实施 | 否（main） |
| 研究后端 | `f6f95cde2dc4300ca0773cceb3d77e2bbe1b62c52f3298f389af59c082caad16`；第 11 节给出完整 SO 与冻结源信息 |
| 质量/契约 | 真机 quality 4 组；质量对照与性能对照均完成，见第 7 节 |
| 是否“没实现” | 不是。代码、构建、质量对照、性能对照都在；缺的是生产接入理由 |
| 后续 | 字形方向后来转向增量版本（`atlas_delta`）和不可变页；裁切作为“整图原型的降上传补丁”保留，不再单独推广 |

### 10. 可挽救的部分与后续条件

可挽救的部分：

1. **按已写高度创建物理图集**本身是正确方向。如果未来仍需要“整图快照”语义（例如不可变页、跨 context 发布），按实际使用高度裁切可以减少首次上传和显存占用。
2. **V 重映射**是低成本的；只要逻辑排布不变，顶点阶段乘一个比例即可。该部分可以复用到“逻辑图集 2048、物理图集分页”的方案。
3. **四组 RGBA 相同**说明在有限条件下裁切不改变画面；这为后续“物理分页 + UV 重映射”提供了质量信心。

后续条件：

- 必须与增量上传或不可变页结合，不能每次发布整张物理图集；否则 494.9 MB 仍远高于生产路径。
- 必须处理物理高度增长的边界：当前实现遇到 shape 变化会清空整个池并重建 4 槽；如果高频增长，成本会超过收益。
- 必须验证非 2 次幂逻辑高度、边缘过滤、glyph 行重排、透明顺序和跨 GPU；当前只有四组有限对照。
- 必须给出相对生产原路径的收益，而不是相对整图原型的收益；否则不能作为优化项进入生产。

结论：裁切可以作为“整图发布原型的补丁”保留，但不能作为 N-01 的成立优化。它的正确判定是“局部有效、整体无优化”。

### 11. 来源与可追溯性

- 研究后端 SHA-256：`f6f95cde2dc4300ca0773cceb3d77e2bbe1b62c52f3298f389af59c082caad16`。
- 源码注入基线：本地提交 `2cecb104`（加入渲染研究后端），裁切代码位于 `prpr/src/ui/text.rs` 与 `prpr/src/ui/atlas_pool.rs`。
- 质量计划/结果：`atlas-crop-quality-plan.json`、`atlas-crop-quality-results.json`、`atlas-crop-quality-runs/`。
- 性能计划/结果：`atlas-crop-performance-plan.json`、`atlas-crop-performance-results.json`、`atlas-crop-performance-runs/`。
- 运行日志：`atlas-crop-performance-run.log`、`atlas-crop-quality-run.log`、`atlas-crop-build-run.log`、`atlas-crop-build-failure.log`。
- 设备：`8DF6R17309002674`；分辨率 2560×1440；Mali-T880 / ES 3.2。
- 研究资料原文（见下方引用块）；原始文件只用于溯源，本文已把关键数字完整列出。

以下为 `本地证据/low-quality-normal/README.md` 中“物理图集裁切：降低替换上传量”一节的完整原文（含标题与后续两段）：

```text
## 物理图集裁切：降低替换上传量

后端 `f6f95cde2dc4300ca0773cceb3d77e2bbe1b62c52f3298f389af59c082caad16` 保留2048²逻辑排布，只上传已写区域的2次幂高度，并用2次幂倍率重映射V；非2次幂逻辑高度回退整图。CPU镜像延迟分配，未使用Painter不分配镜像。高/低画质各固定和动态四组画面 RGBA 完全相同，范围不代表其他字体/设备证明。

真实时钟对照顺序原路径/整图替换/裁切/裁切/整图替换/原路径。原路径约57.35–57.37fps，整图约59.51–59.54fps，裁切约59.71–59.79fps。裁切独立呈现p99约16.75/16.77ms，HUD >20ms各1帧，原路径67/80帧。裁切每轮上传494,927,872 bytes（472MiB），约为整图的1/4，仍远大于原路径约1.6MB。

裁切两轮剩余HUD长帧都在chart≈76.715s，上传8MiB，HUD≈24/27ms（上传调用≈14.6/17.5ms，字形处理≈9ms）。增长/较大资源发布是下一步调查方向，不据此分辨CPU分配与GPU实际执行的精确贡献。文件：atlas-crop-quality/performance-*。
```

引用说明：原文中的“atlas-crop-quality/performance-*”是本地证据目录的简写；本文第 6、7 节已把具体数值、窗口和设备完整展开，不依赖该路径阅读。

---

## N-02 图集双通道 RG（atlas channels）

### 1. 一句话结论与判定

判定：**无优化（混合）**。两通道表示把每个字形图集像素从 RGBA8 的 4 bytes 压到 RG8 的 2 bytes，四个池槽位的名义 GPU 容量从 128 MiB 降到 64 MiB，这是真实的资源占用优化；但整帧时间没有稳定收益：主 ABBA 组均值 7.3963→7.2419 ms（−2.09%），独立对照却 6.4600→6.5805 ms（+1.87% 变慢），且独立对照前后漂移 1.72 ms（约 23.5%），大于组差。格式本身没有建立起稳定的因果加速，不能作为生产优化。

关键数字：

- 主组均值：RGBA 7.396285 ms → 两通道 7.241882 ms，观察值 −2.0876%。
- 独立对照：6.459962 ms → 6.580485 ms，观察值 +1.8657%。
- 独立对照的前后控制漂移：−1.720057 ms，约 23.5%。
- 名义 GPU 存储：每个池 4×2048²×2 = 32 MiB；两个池（default + PGR）64 MiB，原先 128 MiB。
- 质量：11 项作业形成 8 组 RGBA 对照，全部相同；每 context 探针 8 draw / 8 readback、4 个亚像素相位、4096 channels、737 meaningful pixels，RGBA 差异 0。

这里的“无优化”不是“格式无效”：两通道的字节数减半成立，探针也证明在有限条件下采样等价。问题是它没有让完整帧变快，独立对照甚至变慢，因此不能把“存储减半”写成“性能优化”。

### 2. 问题与初衷

字体图集是 HUD 更新的核心资源。每个字形在 atlas 里存成 `[R,G,B,A]`，其中 R=G=B=coverage（白色乘数），A=alpha。原路径每像素 4 bytes；2048×2048 单张 16 MiB，4 个槽位 64 MiB，两个 Painter 池合计 128 MiB。

双通道的初衷是：

1. 覆盖率和 alpha 各占 1 byte，像素从 4 bytes 降到 2 bytes；
2. 在相同槽位数下把名义 GPU 容量减半；
3. 上传字节也减半，减少图集发布时的带宽和驱动复制；
4. 不改变字形栅格、排布、UV 和绘制顺序，只改变纹理的内部表示；
5. 如果驱动对 RG8 的内部表示更紧凑，还可能减少采样带宽。

这里的“双通道”不是把 alpha 丢掉，而是把 `[m,m,m,a]` 存成 `[m,a]`，再通过纹理 swizzle 把采样结果映射回 `[m,m,m,a]`。只存 alpha 会丢失“未写/已写交界”的覆盖插值，不等价；这是本条必须保留 RG 两个通道的原因。

### 3. 实现思路与具体机制

1. **编码**：`pack_part` 逐像素检查 `R==G==B`；满足时写入 `[R,A]`，否则报错并回退。`[0,0,0,0]` 的未写像素保持 `[0,0]`；`[255,255,255,a]` 写成 `[255,a]`。
2. **解码**：创建纹理时优先用驱动的 `alpha_texture` 能力走 `GL_LUMINANCE_ALPHA`（内部格式和上传格式都是 LA）；否则走 sized `GL_RG8`（0x822B）与 `GL_RG`（0x8227），并用 `GL_TEXTURE_SWIZZLE_R/G/B/A` 把 R 映射到 RGB、G 映射到 alpha。
3. **探针**：加载阶段创建一个 4×4 的 RGBA 参考纹理和一个 4×4 的 RG8 紧凑纹理，再用 16×16 的 RGBA8 FBO 做 4 个亚像素相位的绘制和 `glReadPixels`；比较 4 个相位的全部 RGBA 通道，任何差异都判定不支持。探针只在 Android + `chart_pre_compile` 构建里执行，普通 RGBA 路径不调用它。
4. **池**：`Pool::warm` 用 `channels::create` 创建 4 个槽位；`Pool::publish` 在增量上传时用 `pack_part` 把 shadow 的矩形压成 RG，再 `update_texture_part`。普通 RGBA 路径仍走原始字节。
5. **预算**：`bytes_per_pixel(compact)` 返回 2；`warm` 用 `rgba_bytes / 4 * bytes_per_pixel` 计算单槽字节，4 槽总和不得超过 64 MiB。两通道下 2048² 单槽 8 MiB，4 槽 32 MiB。
6. **回退**：`channels_requested` 为 0 关闭；2 强制缺能力；3 只做采样探针但保留 RGBA（用于隔离“探针本身”的影响）；1 才真正启用。任何 probe 失败、格式不支持、创建失败都会回到 RGBA 路径。
7. **质量边界**：CPU shadow 仍是完整 RGBA；只有 GPU 纹理是 RG。也就是说，内存减半只发生在 GPU 名义容量，CPU 侧没有减半；物理 GPU 内存是否真的减半没有被测量。

### 4. 文件、符号与完整核心代码

相关符号：

- `prpr/src/ui/atlas_channels.rs`：`bytes_per_pixel`、`pack_part`、`probe`、`create`、`native::image`、`native::probe`、`native::run`、测试 `codec_preserves_coverage_alpha_and_half_open_rows`、`independent_bilinear_oracle_matches_all_rgba_channels`。
- `prpr/src/ui/atlas_pool.rs`：`Pool::warm` 的 `channels_requested` / `channels_enabled`、`Pool::publish` 的 `compact` 分支。
- `prpr/src/ui/text.rs`：`research_pool_channels`、`LAB_ATLAS_POOL_CHANNELS`、`Pool::new(mode, policy, channels_requested, deferred_requested)` 的第三个参数。
- 研究请求字段：`font_pool_channels`（0–3）。

#### 4.1 每像素字节数与打包

```rust
pub fn bytes_per_pixel(compact: bool) -> usize {
    if compact {
        2
    } else {
        4
    }
}

pub fn pack_part(rgba: &[u8], atlas_width: u32, rect: [u32; 4], compact: bool, scratch: &mut Vec<u8>) -> Result<usize, String> {
    let stride = (atlas_width as usize).checked_mul(4).ok_or("Atlas stride overflow")?;
    if stride == 0 || rgba.len() % stride != 0 {
        return Err("Invalid RGBA atlas dimensions".into());
    }
    let [x0, y0, x1, y1] = rect;
    if x0 >= x1 || y0 >= y1 || x1 > atlas_width || y1 as usize > rgba.len() / stride {
        return Err("Atlas rectangle out of bounds".into());
    }
    let needed = ((x1 - x0) as usize)
        .checked_mul((y1 - y0) as usize)
        .and_then(|n| n.checked_mul(bytes_per_pixel(compact)))
        .ok_or("Atlas upload size overflow")?;
    scratch.clear();
    scratch.reserve(needed);
    for y in y0..y1 {
        let start = y as usize * stride + x0 as usize * 4;
        let row = &rgba[start..start + (x1 - x0) as usize * 4];
        if compact {
            for pixel in row.chunks_exact(4) {
                if pixel[0] != pixel[1] || pixel[0] != pixel[2] {
                    scratch.clear();
                    return Err("Atlas RGB coverage channels differ".into());
                }
                scratch.extend_from_slice(&[pixel[0], pixel[3]]);
            }
        } else {
            scratch.extend_from_slice(row);
        }
    }
    Ok(scratch.len())
}
```

这段代码有三个硬约束：半开区间 `[x0,x1) × [y0,y1)`；越界、非 4 字节对齐、`R!=G!=B` 都拒绝；失败时清空 scratch，绝不把半包数据交给 GL。

#### 4.2 加载期探针与能力判定

```rust
pub fn probe(requested: bool, force_missing: bool) -> Value {
    let report = if !requested || force_missing {
        json!({"requested":requested,"supported":false,"reason":if force_missing {"Forced missing capability"} else {"Disabled"},"compared_channels":0,"compared_pixels":0,"phases":0})
    } else {
        #[cfg(all(target_os = "android", feature = "chart_pre_compile"))]
        {
            let context = unsafe { get_internal_gl() }.quad_context.context_id();
            CACHED.with(|slot| {
                let mut slot = slot.borrow_mut();
                if let Some(cached) = slot.as_ref().filter(|cached| cached.context == context) {
                    let mut value = cached.report.clone();
                    value["reused"] = json!(true);
                    return value;
                }
                let value = native::probe(context);
                *slot = Some(Cached {
                    context,
                    report: value.clone(),
                });
                value
            })
        }
        #[cfg(not(all(target_os = "android", feature = "chart_pre_compile")))]
        {
            json!({"requested":true,"supported":false,"reason":"Android loading-state support not built","compared_channels":0,"compared_pixels":0,"phases":0})
        }
    };
    LAST.with(|slot| *slot.borrow_mut() = report.clone());
    report
}
```

探针按 context 缓存；同一 context 的第二个 Painter 复用结果，不重复做 GL 工作。非 Android 或未编 `chart_pre_compile` 的构建直接返回“不支持”，不会误用格式。

#### 4.3 创建 RGBA 或 RG8 纹理

```rust
pub fn create(width: u32, height: u32, rgba: &[u8], compact: bool) -> Result<Texture2D, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("Atlas image size overflow")?;
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 || rgba.len() != expected {
        return Err("Invalid atlas image size".into());
    }
    if compact {
        #[cfg(all(target_os = "android", feature = "chart_pre_compile"))]
        {
            let context = unsafe { get_internal_gl() }.quad_context.context_id();
            let supported = CACHED.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .is_some_and(|cached| cached.context == context && cached.report["supported"] == true)
            });
            if !supported {
                return Err("Compact atlas loading probe has not succeeded".into());
            }
            let mut packed = Vec::new();
            pack_part(rgba, width, [0, 0, width, height], true, &mut packed)?;
            return native::image(width, height, &packed, true).map(Texture2D::from_miniquad_texture);
        }
        #[cfg(not(all(target_os = "android", feature = "chart_pre_compile")))]
        {
            return Err("Compact atlas unavailable on this build".into());
        }
    }
    let context = unsafe { get_internal_gl() }.quad_context;
    if unsafe { miniquad::gl::glGetError() } != 0 {
        return Err("GL error before atlas allocation".into());
    }
    let texture = miniquad::Texture::from_data_and_format(
        context,
        rgba,
        miniquad::TextureParams {
            width,
            height,
            format: miniquad::TextureFormat::RGBA8,
            wrap: miniquad::TextureWrap::Clamp,
            filter: miniquad::FilterMode::Linear,
        },
    );
    let error = unsafe { miniquad::gl::glGetError() };
    if error != 0 || texture.gl_internal_id() == 0 {
        context.clear_resource_bindings();
        texture.delete();
        return Err(format!("RGBA atlas allocation failed: {error:x}"));
    }
    Ok(Texture2D::from_miniquad_texture(texture))
}
```

紧凑路径要求“本 context 的探针已成功”；没有成功就不允许分配，避免先创建后采样出问题。RGBA 路径要求创建前无 GL 错误、创建后无错误且内部 id 非 0。

#### 4.4 native 纹理：LA 或 sized RG8 + swizzle

```rust
pub(super) fn image(width: u32, height: u32, bytes: &[u8], compact: bool) -> Result<Texture, String> {
    entries(&[
        "glGenTextures",
        "glDeleteTextures",
        "glBindTexture",
        "glActiveTexture",
        "glTexImage2D",
        "glTexParameteri",
        "glGetIntegerv",
        "glPixelStorei",
        "glGetError",
    ])?;
    let saved = crate::render_warm::NativeState::capture().map_err(|error| format!("{error:#}"))?;
    saved.client_pixels();
    let mut active = 0;
    let mut binding = 0;
    unsafe {
        gl::glGetIntegerv(0x84e0, &mut active);
        gl::glGetIntegerv(0x8069, &mut binding);
    }
    check("Atlas texture binding capture")?;
    let alpha_texture = unsafe { get_internal_gl() }.quad_context.features().alpha_texture;
    let mut id = 0;
    let result = (|| {
        unsafe {
            gl::glGenTextures(1, &mut id);
            gl::glBindTexture(gl::GL_TEXTURE_2D, id);
            gl::glPixelStorei(gl::GL_UNPACK_ALIGNMENT, 1);
            let (internal, format) = if !compact {
                (gl::GL_RGBA, gl::GL_RGBA)
            } else if alpha_texture {
                (gl::GL_LUMINANCE_ALPHA, gl::GL_LUMINANCE_ALPHA)
            } else {
                (0x822b, 0x8227)
            };
            gl::glTexImage2D(
                gl::GL_TEXTURE_2D,
                0,
                internal as i32,
                width as i32,
                height as i32,
                0,
                format,
                gl::GL_UNSIGNED_BYTE,
                bytes.as_ptr().cast(),
            );
            gl::glTexParameteri(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR as i32);
            gl::glTexParameteri(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR as i32);
            gl::glTexParameteri(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_WRAP_S, gl::GL_CLAMP_TO_EDGE as i32);
            gl::glTexParameteri(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_WRAP_T, gl::GL_CLAMP_TO_EDGE as i32);
            if compact && !alpha_texture {
                for channel in [0x8e42, 0x8e43, 0x8e44] {
                    gl::glTexParameteri(gl::GL_TEXTURE_2D, channel, gl::GL_RED as i32);
                }
                gl::glTexParameteri(gl::GL_TEXTURE_2D, 0x8e45, gl::GL_GREEN as i32);
            }
        }
        check("Atlas texture allocation/format/swizzle")?;
        if id == 0 {
            return Err("Atlas texture allocation returned zero".into());
        }
        let mut texture = unsafe {
            Texture::from_raw_id(
                id,
                if compact {
                    miniquad::TextureFormat::LuminanceAlpha
                } else {
                    miniquad::TextureFormat::RGBA8
                },
            )
        };
        texture.width = width;
        texture.height = height;
        Ok(texture)
    })();
    unsafe {
        gl::glActiveTexture(active as u32);
        gl::glBindTexture(gl::GL_TEXTURE_2D, binding as u32);
    }
    let restore = saved.finish().map_err(|error| format!("{error:#}"));
    match (result, restore) {
        (Ok(texture), Ok(())) => Ok(texture),
        (result, restore) => {
            if id != 0 {
                unsafe {
                    gl::glDeleteTextures(1, &id);
                }
            }
            let reason = result.err().or_else(|| restore.err()).unwrap();
            Err(reason)
        }
    }
}
```

注意：`alpha_texture` 为真时用 `GL_LUMINANCE_ALPHA`（这是旧式 LA，不是 sized RG8）；否则用 0x822B/0x8227（RG8/RG），再把 R 通道 swizzle 到 RGB、G 通道 swizzle 到 alpha。这样采样器返回的仍是四通道，现有 shader 和混合不需要改。

#### 4.5 池的启用与回退

```rust
let channels_probe = if channels_requested == 2 {
    Some(channels::probe(true, true))
} else if matches!(channels_requested, 1 | 3) && api.is_some() {
    Some(channels::probe(true, false))
} else {
    None
};
let channels_enabled = channels_requested == 1 && api.is_some() && channels_probe.as_ref().is_some_and(|probe| probe["supported"] == true);
let channels_reason = if channels_requested == 0 || channels_enabled {
    None
} else if channels_requested == 3 && channels_probe.as_ref().is_some_and(|probe| probe["supported"] == true) {
    Some("Sampling probe only".to_owned())
} else {
    Some(
        channels_probe
            .as_ref()
            .and_then(|probe| probe["reason"].as_str())
            .unwrap_or("Atlas pool sync capability unavailable")
            .to_owned(),
    )
};
```

模式 3 是关键对照：它执行完全相同的采样探针，但保留 RGBA。这样主 ABBA 的差异可以归因于“表示 + 分配”，而不是“探针是否执行”。独立对照 `glyph-probe-control` 正是用这个模式隔离探针影响。

#### 4.6 打包测试与独立双线性 oracle

```rust
#[test]
fn codec_preserves_coverage_alpha_and_half_open_rows() {
    let image = [
        0, 0, 0, 0, 255, 255, 255, 0, 19, 19, 19, 213, 255, 255, 255, 127, 1, 1, 1, 4, 88, 88, 88, 12,
    ];
    let mut packed = Vec::new();
    assert_eq!(pack_part(&image, 2, [1, 0, 2, 3], true, &mut packed).unwrap(), 6);
    assert_eq!(packed, [255, 0, 255, 127, 88, 12]);
    pack_part(&image, 2, [0, 1, 2, 3], false, &mut packed).unwrap();
    assert_eq!(packed, &image[8..]);
    let mut invalid = image;
    invalid[10] = 9;
    assert!(pack_part(&invalid, 2, [0, 0, 2, 3], true, &mut packed).is_err());
    assert!(packed.is_empty());
    assert!(pack_part(&image, 2, [0, 0, 3, 1], true, &mut packed).is_err());
}
```

```rust
#[test]
fn independent_bilinear_oracle_matches_all_rgba_channels() {
    let mut random = 31u32;
    let mut next = || {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        random >> 24
    };
    for _ in 0..1000 {
        let mut rgba = Vec::new();
        for _ in 0..4 {
            let coverage = next() as u8;
            let alpha = next() as u8;
            rgba.extend([coverage, coverage, coverage, alpha]);
        }
        let mut packed = Vec::new();
        pack_part(&rgba, 2, [0, 0, 2, 2], true, &mut packed).unwrap();
        let x = next() as f64 / 255.;
        let y = next() as f64 / 255.;
        let weights = [(1. - x) * (1. - y), x * (1. - y), (1. - x) * y, x * y];
        for channel in 0..4 {
            let old: f64 = (0..4).map(|i| weights[i] * rgba[i * 4 + channel] as f64 / 255.).sum();
            let encoded_channel = if channel == 3 { 1 } else { 0 };
            let new: f64 = (0..4).map(|i| weights[i] * packed[i * 2 + encoded_channel] as f64 / 255.).sum();
            assert_eq!(old.to_bits(), new.to_bits());
        }
    }
}
```

这个 oracle 用 1000 组随机 `[m,m,m,a]` 和随机双线性权重，逐位比较 RGBA 通道；它只能证明“数学表示在 f64 计算下等价”，不能证明真实驱动的过滤精度、格式支持或 sRGB 行为。

### 5. 开关、默认值、构建特性

| 项目 | 值 | 说明 |
|---|---|---|
| 研究请求字段 | `font_pool_channels` | 0=关闭；1=自动启用；2=强制缺能力；3=只探针、保留 RGBA |
| 默认值 | 0 | 请求缺省即关闭 |
| 前置条件 | `font_pool_mode` 必须为 1（自动池） | 没有池就不创建紧凑槽位 |
| 构建特性 | `render_research` | 研究后端总开关 |
| 构建特性 | `font_atlas_pool` | 池编译；裁剪/增量/通道都在此特性下 |
| 平台条件 | Android + `chart_pre_compile` | 只有该组合执行真实 LA/RG8 探针 |
| 非 Android | 回退 RGBA | 只报“不支持”，不做 GL 工作 |
| 预算 | 64 MiB / 池 | 两通道单槽 8 MiB，四槽 32 MiB |
| 生产默认 | 不启用 | 没有 Config/UI，未进 main |

“默认关”是明确的设计：两通道需要真实格式探针，不能在未知驱动上静默启用；探针失败必须回到 RGBA。

### 6. 完整测量数据

设备与谱面同 N-01：Mali-T880 / ES 3.2、2560×1440、EDI-AL10、Android 8；PGR 谱面，autoplay，低画质。窗口：120 预热 + 1200 采样帧；应用墙钟为 update+render+flush，不含 swap/compositor；独立呈现约 9.1 s。

#### 6.1 主 ABBA（同一 SO，四轮，顺序 channels0 / channels1 / channels1 / channels0）

| 次序 | 策略 | 应用均值 ms | p99 ms | max ms | >16.67 ms | >20 ms | 采样帧 |
|---:|---|---:|---:|---:|---:|---:|---:|
| 0 | RGBA 对照 | 7.4191 | 15.041 | 32.823 | 6 | 1 | 1200 |
| 1 | 两通道 | 7.3570 | 14.202 | 16.819 | 2 | 0 | 1200 |
| 2 | 两通道 | 7.1268 | 13.648 | 18.867 | 4 | 0 | 1200 |
| 3 | RGBA 对照 | 7.3735 | 13.855 | 19.856 | 5 | 0 | 1200 |

组均值：`channels0 = 7.396285 ms`，`channels1 = 7.241882 ms`，观察值 `−2.0876%`。两轮处理都低于两轮对照；对照自身首末变化 −0.04565 ms。

#### 6.2 独立对照（隔离“探针存在”本身，顺序 channels3 / channels1 / channels1 / channels3）

这一组的两臂都执行加载期探针；`channels3` 保留 RGBA，`channels1` 使用两通道。因此它比较的是“格式 + 分配”，不是“有没有跑探针”。

| 次序 | 策略 | 应用均值 ms | p99 ms | max ms | >16.67 ms | >20 ms | 采样帧 |
|---:|---|---:|---:|---:|---:|---:|---:|
| 0 | 探针 + RGBA 对照 | 7.3200 | 13.370 | 17.942 | 2 | 0 | 1200 |
| 1 | 两通道 | 6.8041 | 15.094 | 22.651 | 6 | 1 | 1200 |
| 2 | 两通道 | 6.3569 | 16.528 | 18.574 | 9 | 0 | 1200 |
| 3 | 探针 + RGBA 对照 | 5.5999 | 12.571 | 24.974 | 4 | 1 | 1200 |

组均值：`channels3 = 6.459962 ms`，`channels1 = 6.580485 ms`，观察值 `+1.8657%`（两通道更慢）。对照组首末变化 −1.720057 ms，相对首轮 7.32 ms 约 23.5%；这个漂移大于组差，不能把 +1.87% 当成稳定回归，但同样不能把主组的 −2.09% 当成稳定收益。

#### 6.3 存储与探针

| 项目 | RGBA | 两通道 |
|---|---:|---:|
| 单像素 | 4 bytes | 2 bytes |
| 单槽 2048² | 16 MiB | 8 MiB |
| 单池 4 槽 | 64 MiB | 32 MiB |
| 两池合计 | 128 MiB | 64 MiB |
| CPU shadow | 2048²×4 | 2048²×4（未减） |
| 探针 draw/readback | 不执行 | 8 / 8 |
| 探针相位 | 不执行 | 4 |
| 比较通道 | 不执行 | 4096 |
| meaningful pixels | 不执行 | 737 |
| RGBA 差异 | 不执行 | 0 |

“名义 GPU 容量”只是 `slots × width × height × bytes_per_pixel`；它不含 CPU shadow、驱动内部对齐、在途旧版本和 fallback 纹理，不能当作物理 RSS 或驱动显存。

### 7. 质量与回归结果

| 项目 | 结果 |
|---|---|
| 质量作业 | 11 项 |
| 有效 RGBA 对照 | 8 组 |
| 低画质固定/动态 | 相同 |
| 高画质 | 相同 |
| 强制缺能力 | 回退 RGBA，相同 |
| 全 busy | 快照回退，相同 |
| 动态中英/Greek | 相同 |
| 每 context 探针 | 实际 8 draw / 8 readback |
| 探针 RGBA 差异 | 0 |
| 第二 Painter | 复用缓存，不重复 probe |
| 关闭/缺能力 | 不 probe |
| 关闭后 GL 错误 | 0 |

质量边界：

- 只覆盖一种字体组合、一个谱面、一台 Mali-T880、有限帧。
- 探针是 4×4 纹理 + 16×16 FBO 的独立比较，不是完整图集的逐像素比较；它证明“格式/过滤在采样点上的等价”，不证明所有字形边缘、所有 mipmap、所有 sRGB/颜色空间。
- 没有测 EGL context loss、驱动格式内部表示、真实物理内存。
- 没有覆盖自定义字体、Bold、其他 DPI、其他 GPU。

### 8. 为什么判定为无/负（逐项对比）

| 比较项 | RGBA | 两通道 | 结论 |
|---|---:|---:|---|
| 单像素 | 4 B | 2 B | 资源减半成立 |
| 单池名义容量 | 64 MiB | 32 MiB | 资源减半成立 |
| 主组均值 | 7.3963 ms | 7.2419 ms | 观察值 −2.09%，但独立对照反向 |
| 独立对照均值 | 6.4600 ms | 6.5805 ms | 观察值 +1.87% 变慢 |
| 独立对照漂移 | — | 1.72 ms / 23.5% | 大于组差，不能建立因果 |
| 主组 p99 | 15.041 / 13.855 | 14.202 / 13.648 | 处理略低，但对照自身也在降 |
| 主组 max | 32.823 / 19.856 | 16.819 / 18.867 | 处理没有稳定更低 |
| 独立组 p99 | 13.370 / 12.571 | 15.094 / 16.528 | 处理更差 |
| 独立组 >16.67 | 2 / 4 | 6 / 9 | 处理更差 |
| 质量 | 基准 | 8 组相同 | 有限条件下成立 |

逐项说明：

1. **存储**：2 bytes/px 是确定的；单池 4 槽从 64 MiB 降到 32 MiB，两池从 128 MiB 降到 64 MiB。这是本条唯一没有争议的收益。
2. **主组帧时间**：−2.09% 看起来是收益，但两轮处理都低于两轮对照并不能排除“处理恰好跑在设备更凉/更空的时间段”；对照自身首末只差 −0.046 ms，说明主组的前后控制很稳，但这只是一个四轮有限窗口，不是因果证明。
3. **独立对照**：当两臂都执行探针、只改变格式时，处理反而 +1.87% 慢；对照组从 7.32 掉到 5.60，漂移 1.72 ms（23.5%），远大于组差 0.12 ms。这个反向结果直接否定了“格式本身带来稳定加速”。
4. **尾部**：主组处理 p99 略低，但独立组处理 p99 更高（15.09/16.53 vs 13.37/12.57），>16.67 ms 帧也更多（6/9 vs 2/4）。两条 cohort 方向不一致，不能宣称尾延迟改善。
5. **机制成本**：两通道需要逐像素检查 R=G=B、打包、可能的 LA/RG8 格式转换、swizzle 状态设置；这些 CPU/驱动成本可能抵消上传减半的收益。独立对照的 +1.87% 与这个解释一致，但没有单独测每个成本。
6. **结论**：资源优化成立，帧时间优化不成立。按“非正优化”分类，应记为“无优化/混合”，而不是“负优化”——它没有稳定拖慢主路径，但也没有稳定加速；独立对照的变慢是重要反证。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：研究后端 `bc3f78a3`（首版两通道）与 `d523ee4`（隔离探针）中完整实现；SO 分别为 `45ff974bdfa9c500627ab9617feab588dca50234b6ae1b1b8d638bc29766e8af` 与 `11f0a618998b2ea653fc86300e743496fefe4f0368577122513d8026ab9fdc57`；13 个 atlas oracle、Android API26 构建通过 |
| 默认 | 关闭；`font_pool_channels` 缺省 0；只有显式请求 1 且探针成功才启用 |
| 普通路径 | 未接入；生产没有 Config/UI 开关；`font_atlas_pool` 只在研究构建里用，主项目默认构建即使带该 feature 也不会自动开两通道 |
| 主线是否已实施 | 否（main） |
| 研究后端 | `bc3f78a3`、`d523ee4`；SO 见上；第 11 节给出冻结源与证据路径 |
| 质量/契约 | 13 个 atlas oracle、Android API26 构建通过；质量对照见第 7 节 |
| 是否“没实现” | 不是。编码、格式创建、探针、池接入、回退、质量对照都已完成；缺的是稳定帧时间收益 |
| 后续 | 字形方向后来转向增量版本/不可变页；两通道保留为“资源候选”，没有继续推广 |

### 10. 可挽救的部分与后续条件

可挽救：

1. **名义 GPU 容量减半**是真实的；如果未来遇到显存压力或图集槽位不足，两通道可以作为“容量优先”候选。
2. **LA/RG8 + swizzle** 的实现和探针可以复用到其他覆盖率/alpha 分离的资源。
3. **独立探针隔离**的方法本身值得保留：它把“探针执行成本”和“格式成本”分开，是后续任何格式实验的模板。

后续条件：

- 必须在更多 GPU/驱动上验证 LA/RG8 的过滤精度、sRGB、mipmap、clamp 边界；当前只有一台 Mali-T880。
- 必须测真实物理内存，而不是名义 `slots × bytes`；CPU shadow 仍是 RGBA，整体内存没有减半。
- 必须把“格式转换成本”和“上传减半收益”分开计时；当前只有整帧墙钟，不能解释 +1.87% 来自哪里。
- 必须证明在目标设备上帧时间不退化；独立对照已经给出反例，不能只拿主组的 −2.09% 推广。
- 如果只是显存优化，应明确标注“资源优化”，不要写成“性能优化”。

### 11. 来源与可追溯性

- 源码提交：`bc3f78a37deb3b1eed73c4671536e90dfb7a6bc3`（两通道首版）、`d523ee4d3c71e32f8f8244f187bae26a3cd2a7db`（隔离探针）。
- 后端 SHA-256：`45ff974bdfa9c500627ab9617feab588dca50234b6ae1b1b8d638bc29766e8af`、`11f0a618998b2ea653fc86300e743496fefe4f0368577122513d8026ab9fdc57`。
- 主组证据：`glyph-channels-v1-summary.json`、`glyph-channels-v1-performance-results.json`、`glyph-channels-v1-quality-results.json`、`glyph-channels-v1-performance-runs/`、`glyph-channels-v1-quality-runs/`。
- 独立对照：`glyph-probe-control-v1-summary.json`、`glyph-probe-control-v1-performance-results.json`、`glyph-probe-control-v1-quality-results.json`、`glyph-probe-control-v1-performance-runs/`。
- 探针/测试日志：`glyph-channels-tests.log`、`glyph-channels-check.log`、`glyph-channels-default-check.log`、`glyph-channels-lab-build.log`。
- 设备：`8DF6R17309002674`；Mali-T880 / ES 3.2；2560×1440。

以下为 `本地证据/low-quality-normal/README.md` 中两通道相关段落的完整原文（原文中的外部规范链接已去链接化，只保留文字）：

```text
两通道推导（候选，尚未真机验收）：原shadow每个像素是[m,m,m,a]，未写为全0，写入为[255,255,255,a]。在相同clamp/线性过滤下，RGBA采样为[Σw·m,Σw·m,Σw·m,Σw·a]；存[m,a]并映射RGB为首通道、alpha为次通道得到同表达式，现有材质与混合无需改变。只存alpha则丢失未写/已写交界的覆盖插值，不等价。GL规范是理论依据（Khronos ES3规范，纹理基色和swizzle），实际驱动过滤精度/格式支持须加载期抽样＋有限gameplay像素对照。先保留RGBA权威shadow，隔离native传输/逻辑存储改变；CPU pack成本和驱动格式内部表示仍可能抵消收益，不能从2bytes/px声称物理GPU内存减半。font_pool_channels默认0，支持失败与强制缺能力返回原RGBA，独立cohort policy0，不叠加负收益精确damage/coalesce。

两通道上述候选现已完成首个真机cohort：本地源码bc3f78a3（研究字形双通道图集，未push），SO45ff974b冻结35源；默认/研究检查、13个atlas oracle、Android API26通过。ES3 factory sizedRG8映射RGB=RED/alpha=GREEN，避免库原swizzle与unsizedRG，library rev未动。probe每context实际8draw/8read、4subtexel phases/4096channels/737meaningful pixels，RGBA差异0；secondPainter cached复用，关闭/强制缺能力不probe。11项quality/8组RGBA全同，高画质/缺能力/全busy/动态中英Greek均覆盖；仅有限画面证据，不能外推所有字体/GPU/EGL loss。每poolGPUslot名义64→32MiB、CPUshadow仍RGBA；未测物理GPU/RSS。完整ABBA已结束，组均值RGBA7.3963/两通道7.2419ms（观察值减少2.09%），详尽尖峰/呈现与原始输入审计正在离线汇总，暂不称稳定尾优化。默认关闭、保留研究；原chart编译目标继续。

补充同探测对照已完成：本地d523ee4（隔离字形探测影响），SO11f0a618冻结35源。仅增加mode3，和mode1同样加载probe但保留RGBA；quality0/3/1三项两组RGBA全同。独立ABBA3/1/1/3应用均值7.3200/6.8041/6.3569/5.5999ms，对照组6.4600、LA6.5805（观察值慢1.87%），对照前后漂移23.5%，没有建立稳定加速/格式独立因果。p99 13.370/15.094/16.528/12.571，>16.67为2/6/9/4，>20为0/1/0/1；呈现max32.648/33.470/17.087/33.476ms（约10s有限独立呈现采样，非全曲/全部1200帧）。详尽审计glyph-probe-control-v1-summary.json。两通道保留默认关闭的资源优化候选，payload/名义容量减半成立，物理内存和稳定尖峰收益未证。至此结束字形路径这一轮实验，返回谱面render-only求值/父链计划编译，不能把字体实验替换初始chart预编译目标。
```

---

## N-03 冻结文字图集（frozen text）

### 1. 一句话结论与判定

判定：**负优化**。冻结图集把运行时字形栅格/上传移到加载阶段，确实把 HUD 更新从约 1.31–1.36 ms 降到约 1.03–1.06 ms、glyph upload 从 0.282 ms 降到 0.039 ms，也把 Playing 25 s 的 loop p99 从 22.34 ms 降到 18.60 ms；但完整应用墙钟从 9.1199 ms 升到 9.2285 ms（+1.19%），FPS 只从 59.4876 升到 59.6740（+0.31%）。这是“局部 HUD 指标改善、整帧变慢”的典型负优化，而且 phase4 量化本身是有损的，不能进入普通画质。

关键数字：

- packed Main low，4 轮 ABBA：off 9.119857 ms，on 9.228474 ms，`+1.19099%`。
- FPS：off 59.487576，on 59.673998，`+0.31338%`。
- loop p99：off 22.342404 ms，on 18.600550 ms。
- loop max：off 57.959896 ms，on 42.847656 ms；但 off 的 A2−A1 为 −19.44 ms，漂移大于组差，不能把 max 改善当稳定收益。
- >30 ms 帧：3 / 1 / 1 / 3（off 6，on 2）。
- 质量：6 组对照；3 组覆盖组有差异（16562 px / max 71、8567 px / max 49、6600 px / max 71），3 组回退组精确相同。
- 图集：2 页 × 2048² × 4 = 32 MiB；phase4；3216 entries（Main general corpus 的 201 bases × 16）。

### 2. 问题与初衷

HUD 的长帧主要来自字形图集的运行时更新：字形第一次出现时要栅格化、排进 atlas、上传；上传发生在绘制前，会和正在被采样的图集产生读写依赖。原路径在 1200 帧里出现 67/80 个 HUD >20 ms 帧，最大 50 ms 左右；这被怀疑是“运行时字形更新 + 驱动同步”的成本。

冻结图集的初衷是：

1. 在加载阶段把有限字符集、有限字号、有限相位的字形全部栅格化并上传；
2. gameplay 阶段只做布局、查表、生成四边形，不再触发字形栅格和纹理上传；
3. 用 4×4 的亚像素相位分箱，把“同一字形在不同小数位置”的栅格代表限制在 16 个；
4. 用 2048×2048 页、32 MiB 预算，保证加载期可分配；
5. 未知字形、未知字号、未知相位、viewport/font scale 变化时整段回退原路径，绝不部分绘制。

这不是“把字体烤成静态纹理就无损”：phase4 会把字形位置量化到 1/4 像素，栅格代表变化，边缘覆盖不同。代码注释明确写了“phase quantization changes raster representatives, so the caller must restrict this to low quality”。因此它从设计上就是低画质有损候选。

### 3. 实现思路与具体机制

1. **字符集/字号收集**：`collect_bases` 遍历 corpus 的每个字符和 `scales` 的每个字号，按 `layout.rs` 的字体选择规则（空白用第 0 个字体，其他字符找第一个有 glyph 的字体）生成 `Base { font, id, sx, sy }`。最多 4096 个 base。
2. **16 相位**：每个 base 生成 `qx,qy ∈ 0..4` 共 16 个 `Key`；`representative` 把相位转成 `position = (qx/4, qy/4)` 的 Glyph，交给 `outline_glyph` 得到像素边界。
3. **预检**：`Plan::build` 先对每个 key 取 `outline_glyph`，检查边界有限、整数、非空；宽高加 2 像素 padding；超过 2048 或总字节超预算就拒绝。所有分配前完成，失败不产生半张图集。
4. **排布**：按 padded height 降序、padded width 降序、Key 升序排序，再用 shelf 排进 2048 页；这是“集装箱”优化，目的是把同一字形的小字号变体和大字号变体分开，减少行高浪费。
5. **像素生成**：`page_pixels` 先填 `[255,255,255,0]`（白色 RGB、透明 alpha），再对每个 entry 调 `outline_glyph().draw`，只写 alpha；字形边缘的 coverage 直接进 alpha，RGB 保持 255。
6. **上传**：`prepare_loading` 在加载线程/渲染线程上创建 `Texture2D`，`RGBA8`、Linear、Clamp；每页一张；用 `with_client_memory_unpack` 保护 client memory 状态；检查 `GL_MAX_TEXTURE_SIZE ≥ 2048`、context 未变化、纹理 id 非 0。
7. **布局**：`FrozenText::layout` 用独立的 `GlyphCalculator`，不碰原 GlyphBrush 的 cache；`layout_text` 仍走原布局函数，保证 metrics/advance 不变。
8. **查表**：`Plan::lookup` 用 `phase()` 把 glyph 的小数位置转成 `(anchor, q)`；`anchor` 是整数像素，`q` 是 1/4 相位；再用 `Base + qx + qy` 查 entry；查不到就整段回退。
9. **绘制**：`prepare_section_inner` 逐 glyph 生成 quad；UV 用 entry 在页内的 `[x,y,x+w,y+h] / 2048`；按页分组，连续同页最多 512 个 quad 一次 `geometry`；页切换必须保持原顺序，不能按纹理排序。
10. **回退**：`valid=false`、environment 变化、context 变化、section 非法、glyph 未覆盖、scale 不精确、page 越界、transform 非有限，任一条件都让整段返回 false，由原路径绘制。不会出现“部分冻结 + 部分原路径”的重复绘制。
11. **生命周期**：`OwnedTexture` 的 Drop 只把纹理放进全局队列，不在 Drop 里碰 GL；`drain_retired` 在渲染线程 flush、清 binding、删除；foreign context 只丢弃不删除，避免误删新 context 复用的名字。
12. **质量分类**：phase4 是低画质有损；`research_prepare_frozen_loading` 要求 `low_quality`（`sample_count==1 && chart_pre_compile`），普通画质直接拒绝安装。

### 4. 文件、符号与完整核心代码

相关符号：

- `prpr/src/ui/frozen_text.rs`：`PHASES`、`PAGE_SIZE`、`MAX_BUDGET`、`MAX_BASES`、`Base`、`Key`、`Entry`、`Plan`、`phase`、`representative`、`collect_bases`、`Plan::build`、`Plan::page_pixels`、`Plan::lookup`、`FrozenText::prepare_loading`、`FrozenText::prepare_section_inner`、`FrozenText::report`、`drain_retired`。
- `prpr/src/ui/text.rs`：`research_prepare_frozen_loading`、`research_disable_frozen_text`、`research_draw_frozen_section`、`research_frozen_report`、`frozen_environment`。
- `phira/src/render_research.rs`：`FROZEN_TEXT_REQUESTED`、`main_ui_prepare_frozen_text`、`guard_frozen_quality_permission`、`disable_frozen_text_resources`。
- 研究请求字段：`font_frozen_text`、`font_frozen_text_probe`。

#### 4.1 常量、Key 与相位量化

```rust
const PHASES: u8 = 4;
const PAGE_SIZE: u32 = 2048;
const MAX_BUDGET: usize = 32 * 1024 * 1024;
const MAX_BASES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Base { font: usize, id: u16, sx: u32, sy: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Key { base: Base, qx: u8, qy: u8 }
#[derive(Clone, Copy, Debug)]
struct Entry { bounds: [i32; 4], page: usize, rect: [u32; 4] }

fn phase(value: f32) -> Option<(i32, u8)> {
    if !value.is_finite() { return None; }
    let value = f64::from(value);
    let anchor = value.floor();
    let q = ((value - anchor) * f64::from(PHASES)).round() as u8;
    let anchor = anchor + if q == PHASES { 1. } else { 0. };
    if anchor < f64::from(i32::MIN) || anchor > f64::from(i32::MAX) { return None; }
    Some((anchor as i32, if q == PHASES { 0 } else { q }))
}

fn representative(key: Key) -> Glyph {
    Glyph {
        id: GlyphId(key.base.id),
        scale: PxScale { x: f32::from_bits(key.base.sx), y: f32::from_bits(key.base.sy) },
        position: point(f32::from(key.qx) / f32::from(PHASES), f32::from(key.qy) / f32::from(PHASES)),
    }
}
```

`phase()` 的关键是“负坐标也正确”：`floor()` 后取小数，四舍五入到 4 个相位；`q==4` 时进位到下一个整数并归零。它不改变 advance/metrics，只改变栅格代表。

#### 4.2 `Plan::build`：分配前预检与 shelf 排布

```rust
fn build(fonts: &[FontArc], bases: &BTreeSet<Base>, page_size: u32, budget: usize) -> Result<Self, Error> {
    let page_bytes = (page_size as usize).checked_mul(page_size as usize).and_then(|n| n.checked_mul(4))
        .ok_or(Error::Unavailable("atlas dimensions overflow"))?;
    if page_size == 0 || budget > MAX_BUDGET { return Err(Error::Unavailable("invalid atlas budget")); }
    if bases.len() > MAX_BASES { return Err(Error::Unavailable("finite glyph metadata limit exceeded")); }
    let mut plan = Self { entries: HashMap::new(), empty: HashSet::new(), pages: 0, page_size, atlas_bytes: 0 };
    let count = bases.len().checked_mul(usize::from(PHASES).pow(2))
        .ok_or(Error::Unavailable("glyph rectangle count overflow"))?;
    let mut rectangles = Vec::new();
    rectangles.try_reserve_exact(count)
        .map_err(|_| Error::Unavailable("glyph packing scratch allocation failed"))?;
    for &base in bases {
        let font = fonts.get(base.font).ok_or(Error::Unavailable("unknown font id"))?;
        let first = Key { base, qx: 0, qy: 0 };
        if font.outline_glyph(representative(first)).is_none() {
            plan.empty.insert(base);
            continue;
        }
        for qy in 0..PHASES {
            for qx in 0..PHASES {
                let key = Key { base, qx, qy };
                let outlined = font.outline_glyph(representative(key)).ok_or(Error::Unavailable("inconsistent glyph outline"))?;
                let bounds = integer_bounds(outlined.px_bounds())?;
                let w = bounds[2].checked_sub(bounds[0]).and_then(|v| u32::try_from(v).ok())
                    .ok_or(Error::Unavailable("raster width overflow"))?;
                let h = bounds[3].checked_sub(bounds[1]).and_then(|v| u32::try_from(v).ok())
                    .ok_or(Error::Unavailable("raster height overflow"))?;
                let pw = w.checked_add(2).ok_or(Error::Unavailable("padded width overflow"))?;
                let ph = h.checked_add(2).ok_or(Error::Unavailable("padded height overflow"))?;
                if pw > page_size || ph > page_size { return Err(Error::Unavailable("glyph exceeds texture maximum")); }
                rectangles.push((key, bounds, w, h, pw, ph));
            }
        }
    }
    rectangles.sort_unstable_by(|a, b| b.5.cmp(&a.5).then_with(|| b.4.cmp(&a.4)).then_with(|| a.0.cmp(&b.0)));
    let (mut x, mut y, mut row_height) = (0u32, 0u32, 0u32);
    for (key, bounds, w, h, pw, ph) in rectangles {
        if plan.pages == 0 { plan.pages = 1; }
        if x.checked_add(pw).ok_or(Error::Unavailable("packing overflow"))? > page_size {
            x = 0;
            y = y.checked_add(row_height).ok_or(Error::Unavailable("packing overflow"))?;
            row_height = 0;
        }
        if y.checked_add(ph).ok_or(Error::Unavailable("packing overflow"))? > page_size {
            plan.pages = plan.pages.checked_add(1).ok_or(Error::Unavailable("page count overflow"))?;
            x = 0; y = 0; row_height = 0;
        }
        plan.atlas_bytes = plan.pages.checked_mul(page_bytes).ok_or(Error::Unavailable("atlas byte count overflow"))?;
        if plan.atlas_bytes > budget { return Err(Error::Unavailable("atlas RGBA budget exceeded")); }
        plan.entries.insert(key, Entry { bounds, page: plan.pages - 1, rect: [x + 1, y + 1, w, h] });
        x += pw;
        row_height = row_height.max(ph);
    }
    Ok(plan)
}
```

`rect` 的 `x+1,y+1` 是 1 像素 padding；`page_pixels` 会把 padding 留成 `[255,255,255,0]`，供线性过滤采样。排序是“padded height 降序 → padded width 降序 → Key 升序”，Key 升序保证确定性，不依赖 HashMap 顺序。

#### 4.3 `page_pixels`：白色 RGB + coverage alpha

```rust
fn page_pixels(&self, fonts: &[FontArc], page: usize) -> Result<Vec<u8>, Error> {
    let len = (self.page_size as usize).checked_mul(self.page_size as usize).and_then(|n| n.checked_mul(4))
        .ok_or(Error::Unavailable("scratch dimensions overflow"))?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(len).map_err(|_| Error::Unavailable("atlas scratch allocation failed"))?;
    pixels.resize(len, 0);
    for rgba in pixels.chunks_exact_mut(4) { rgba[..3].fill(255); }
    for (&key, entry) in &self.entries {
        if entry.page != page { continue; }
        let outlined = fonts[key.base.font].outline_glyph(representative(key))
            .ok_or(Error::Unavailable("prepared glyph outline disappeared"))?;
        if integer_bounds(outlined.px_bounds())? != entry.bounds { return Err(Error::Unavailable("prepared raster bounds changed")); }
        outlined.draw(|x, y, coverage| {
            let at = ((entry.rect[1] + y) as usize * self.page_size as usize + (entry.rect[0] + x) as usize) * 4;
            pixels[at + 3] = (coverage * 255.) as u8;
        });
    }
    Ok(pixels)
}
```

这段代码必须与 GlyphBrush 的原始 RGBA 回调一致：RGB 全 255，alpha 是 coverage；padding 也是 `[255,255,255,0]`。如果改成 `[0,0,0,a]`，线性过滤在边缘会变暗，画面就会不同。

#### 4.4 `lookup`：相位、边界平移与整段回退

```rust
fn lookup(&self, glyph: &SectionGlyph) -> Result<Option<(Entry, [f32; 4])>, Error> {
    let g = &glyph.glyph;
    if g.id.0 == 0 || !g.scale.x.is_finite() || !g.scale.y.is_finite() || g.scale.x <= 0. || g.scale.y <= 0. {
        return Err(Error::Unavailable("missing glyph or invalid scale"));
    }
    let (ax, qx) = phase(g.position.x).ok_or(Error::Unavailable("invalid glyph position"))?;
    let (ay, qy) = phase(g.position.y).ok_or(Error::Unavailable("invalid glyph position"))?;
    let base = Base { font: glyph.font_id.0, id: g.id.0, sx: g.scale.x.to_bits(), sy: g.scale.y.to_bits() };
    if self.empty.contains(&base) { return Ok(None); }
    let entry = *self.entries.get(&Key { base, qx, qy }).ok_or(Error::Unavailable("glyph or exact scale not covered"))?;
    let px = translated_bounds(entry.bounds, ax, ay).ok_or(Error::Unavailable("translated bounds overflow"))?;
    Ok(Some((entry, px)))
}
```

`Base` 用 `scale.to_bits()`，所以 30.0 与 30.0000001 是不同的 base；任何不是加载期收集到的精确字号都会回退。`empty` 记录“字体有该字符但 outline 为空”的 base（例如空格），查到时返回 `Ok(None)`，表示该 glyph 不画，而不是回退整段。

#### 4.5 加载：预检、预算、上传与 context 检查

```rust
pub(super) fn prepare_loading(fonts: &[FontArc], primary_scale: f32, scales: &[f32], corpus: &str, options: LoadingOptions) -> Result<Self, Error> {
    let env = options.environment;
    let font_scale = f32::from_bits(env.font_display_scale_bits);
    if env.viewport.2 <= 0 || env.viewport.3 <= 0 || !font_scale.is_finite() || font_scale <= 0. {
        return Err(Error::Unavailable("invalid loading environment"));
    }
    if options.budget_bytes == 0 || options.budget_bytes > MAX_BUDGET { return Err(Error::Unavailable("invalid atlas budget")); }
    let bases = collect_bases(fonts, primary_scale, scales, corpus)?;
    let plan = Plan::build(fonts, &bases, PAGE_SIZE, options.budget_bytes)?;
    miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |_| ())
        .map_err(|e| Error::unpack("client-unpack probe", e))?;
    let mut max_texture = 0;
    unsafe { miniquad::gl::glGetIntegerv(miniquad::gl::GL_MAX_TEXTURE_SIZE, &mut max_texture); }
    let error = unsafe { miniquad::gl::glGetError() };
    if error != 0 { return Err(Error::Gl(format!("frozen texture maximum query: GL {error:x}"))); }
    if max_texture < PAGE_SIZE as i32 { return Err(Error::Unavailable("actual texture maximum below 2048")); }
    let context = unsafe { get_internal_gl() }.quad_context.context_id();
    let mut textures = Vec::new();
    for page in 0..plan.pages {
        let pixels = plan.page_pixels(fonts, page)?;
        let mut created = None;
        let result = miniquad::with_client_memory_unpack(unsafe { get_internal_gl() }.quad_context, |ctx| {
            created = Some(Texture::from_data_and_format(ctx, &pixels, TextureParams {
                width: PAGE_SIZE, height: PAGE_SIZE, format: miniquad::TextureFormat::RGBA8,
                filter: FilterMode::Linear, wrap: miniquad::TextureWrap::Clamp,
            }));
        });
        if let Some(raw) = created {
            textures.push(OwnedTexture::new(Texture2D::from_miniquad_texture(raw), context));
        }
        result.map_err(|e| Error::unpack(&format!("page {page} upload"), e))?;
        if unsafe { get_internal_gl() }.quad_context.context_id() != context {
            return Err(Error::Unavailable("managed context changed during preparation"));
        }
        if textures.last().is_none_or(|t| t.texture.raw_miniquad_texture_handle().gl_internal_id() == 0) {
            return Err(Error::Unavailable("frozen texture allocation returned zero"));
        }
    }
    Ok(Self {
        calculator: GlyphCalculatorBuilder::using_fonts(fonts.to_vec()).build::<Extra>(),
        ink: InkCache::default(), plan, textures, environment: env, context, primary_scale,
        valid: true, counters: Counters::default(), _render_thread: PhantomData,
    })
}
```

`try_reserve_exact` 和 `try_reserve` 把“分配失败”变成有界错误，而不是 panic；`with_client_memory_unpack` 把 client memory 状态恢复纳入错误路径；`context_id` 检查防止在错误的 context 上创建。

#### 4.6 `prepare_section_inner`：整段预检、四边拒绝与按页分组

```rust
fn prepare_section_inner(&mut self, section: &Section<'_>, environment: Environment) -> Result<(Vec<PreparedQuad>, usize), Error> {
    if !self.valid { return Err(Error::Unavailable("frozen atlas invalidated")); }
    if environment != self.environment { return Err(Error::Unavailable("viewport, font scale, or generation changed")); }
    if self.context != unsafe { get_internal_gl() }.quad_context.context_id() { return Err(Error::Unavailable("managed context changed")); }
    if !section.screen_position.0.is_finite() || !section.screen_position.1.is_finite()
        || section.bounds.0.is_nan() || section.bounds.1.is_nan() || section.bounds.0 < 0. || section.bounds.1 < 0.
        || section.text.iter().any(|t| t.font_id.0 >= self.calculator.fonts().len()
            || !t.scale.x.is_finite() || !t.scale.y.is_finite() || t.scale.x <= 0. || t.scale.y <= 0.
            || t.extra.color.iter().any(|v| !v.is_finite())) {
        return Err(Error::Unavailable("invalid section"));
    }
    let bounds = section.layout.bounds_rect(&SectionGeometry::from(section));
    if [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y].iter().any(|v| v.is_nan()) {
        return Err(Error::Unavailable("invalid section bounds"));
    }
    let mut scope = self.calculator.cache_scope();
    let glyphs = scope.glyphs(section);
    let count = glyphs.len();
    self.counters.requested_glyphs = self.counters.requested_glyphs.saturating_add(count as u64);
    let mut quads = Vec::new();
    quads.try_reserve_exact(count).map_err(|_| Error::Unavailable("section geometry allocation failed"))?;
    for glyph in glyphs {
        let Some((entry, px)) = self.plan.lookup(glyph)? else { continue; };
        if px[0] > bounds.max.x || px[1] > bounds.max.y || bounds.min.x > px[2] || bounds.min.y > px[3] { continue; }
        let extra = section.text.get(glyph.section_index).ok_or(Error::Unavailable("invalid glyph section index"))?.extra;
        let color: Color = extra.color.into();
        let d = self.plan.page_size as f32;
        let uv = [entry.rect[0] as f32 / d, entry.rect[1] as f32 / d,
            (entry.rect[0] + entry.rect[2]) as f32 / d, (entry.rect[1] + entry.rect[3]) as f32 / d];
        quads.push(PreparedQuad { page: entry.page, vertices: [
            MyVertex::new(px[0], px[1], uv[0], uv[1], color),
            MyVertex::new(px[2], px[1], uv[2], uv[1], color),
            MyVertex::new(px[0], px[3], uv[0], uv[3], color),
            MyVertex::new(px[2], px[3], uv[2], uv[3], color),
        ] });
    }
    Ok((quads, count))
}
```

关键点：

- 先 `lookup` 整段，再返回 quads；任何 glyph 失败都会让整个 section 回退，不会出现半段。
- 四边拒绝用“整字形边界”，与 glyph_brush 的 inclusive 逻辑一致；不引入局部裁剪。
- `page` 只是分组键，排序仍按 glyph 原顺序；同页连续 run 才能合批。
- 没有 `try_reserve` 之外的堆分配；quads 容量按 glyph 数预留。

#### 4.7 报告与生命周期

```rust
pub(super) fn report(&self) -> serde_json::Value {
    let c = &self.counters;
    serde_json::json!({
        "valid":self.valid,"context":self.context,"phase_bins":PHASES,"page_size":PAGE_SIZE,
        "pages":self.plan.pages,"atlas_rgba_bytes":self.plan.atlas_bytes,"budget_max":MAX_BUDGET,
        "entries":self.plan.entries.len(),"empty_bases":self.plan.empty.len(),
        "atlas_metadata_payload_bytes":self.plan.entries.len()*(std::mem::size_of::<Key>()+std::mem::size_of::<Entry>())
            +self.plan.empty.len()*std::mem::size_of::<Base>(),
        "metadata_payload_scope":"Key/value payload only; map capacity, independent layout cache and font data are outside atlas RGBA budget.",
        "scratch_peak_bytes":if self.plan.pages==0 {0}else{PAGE_SIZE as usize*PAGE_SIZE as usize*4},
        "primary_scale_bits":self.primary_scale.to_bits(),"environment":{"viewport":self.environment.viewport,
            "font_display_scale_bits":self.environment.font_display_scale_bits,"generation":self.environment.generation},
        "requested_sections":c.requested_sections,"covered_sections":c.covered_sections,"fallback_sections":c.fallback_sections,
        "requested_glyphs":c.requested_glyphs,"covered_glyphs":c.covered_glyphs,"quads":c.quads,
        "retirement":retirement_report(),
        "scope":"Research-only low-quality finite corpus; phase4 raster representatives are lossy. Exact glyph/FontId/scale keys, layout metrics and glyph order are retained. Coverage and deletion API counters are not FPS, GPU timing, physical resource reclamation, or EGL-loss proof."
    })
}
```

```rust
impl Drop for OwnedTexture {
    fn drop(&mut self) {
        let mut retired = RETIRED.lock().unwrap();
        retired.pending.push(RetiredTexture { texture: self.texture, context: self.context, thread: self.thread });
        retired.queued += 1;
        PENDING.fetch_add(1, Ordering::Release);
    }
}
```

这段 Drop 不调用 GL、不访问 context、不加锁以外的副作用；删除统一在 `drain_retired` 做。foreign context 的名字只丢弃不删除，避免在新 context 里误删同名资源。

### 5. 开关、默认值、构建特性

| 项目 | 值 | 说明 |
|---|---|---|
| 研究请求字段 | `font_frozen_text` | true 才准备冻结图集 |
| 默认值 | false | 请求缺省即关闭 |
| 画质门槛 | `sample_count == 1 && chart_pre_compile` | 仅低画质 + 谱面预编译开启 |
| 构建特性 | `render_research` | 研究后端总开关 |
| 生产默认 | 不启用 | 没有 Config/UI 开关，未进 main |
| 图集页 | 2048×2048 RGBA8 | 页数由 corpus 决定 |
| 预算 | 32 MiB | 2 页；超过就拒绝 |
| 相位 | 4×4 = 16 | 有损；低画质限定 |
| 字体 | 加载期 `FontArc` 列表 | 运行期字体变化/自定义字体回退 |
| 回退 | 未知 glyph/scale/phase、viewport/font scale 变化、context 变化 | 整段原路径 |

“默认关”是硬性的：`main_ui_prepare_frozen_text` 先检查 `FROZEN_TEXT_REQUESTED`，false 直接返回；`guard_frozen_quality_permission` 在设置变化后如果画质不再是低画质，会立即禁用并清 flag，防止把有损图集带进普通画质。

### 6. 完整测量数据

设备：`8DF6R17309002674`，Mali-T880 / ES 3.2，2560×1440，Android 8。谱面：PGR，24 线、1568 notes；autoplay；低画质、sampleCount=1、`chart_pre_compile=true`、`font_frozen_text=true`。窗口：真实 Main，25 s Playing；ABBA 顺序 A1 off / B1 on / B2 on / A2 off；每轮独立 collect；应用墙钟为 update+render+flush。

| 次序 | 策略 | 应用均值 ms | FPS | loop p99 ms | loop max ms | >30 ms 帧 |
|---:|---|---:|---:|---:|---:|---:|
| 0 | 关闭（A1） | 9.126277 | 59.475951 | 22.275760 | 67.681250 | 3 |
| 1 | 冻结（B1） | 9.164145 | 59.671457 | 18.667870 | 36.111458 | 1 |
| 2 | 冻结（B2） | 9.292803 | 59.676539 | 18.533230 | 49.583854 | 1 |
| 3 | 关闭（A2） | 9.113438 | 59.499201 | 22.409047 | 48.238542 | 3 |

组统计：

| 指标 | 关闭组均值 | 冻结组均值 | 变化 |
|---|---:|---:|---:|
| 应用墙钟 | 9.119857 ms | 9.228474 ms | +0.108617 ms / +1.19099% |
| FPS | 59.487576 | 59.673998 | +0.186422 / +0.31338% |
| loop p99 | 22.342404 ms | 18.600550 ms | −3.741854 ms / −16.75% |
| loop max | 57.959896 ms | 42.847656 ms | −15.112240 ms / −26.07% |
| >30 ms 累计 | 6 | 2 | −4 |
| glyph upload 均值 | 0.282127 ms | 0.039138 ms | −0.242989 ms / −86.1% |
| HUD 均值（汇总口径） | 约 1.31–1.36 ms | 约 1.03–1.06 ms | 明显下降 |

漂移检查：

- 关闭组 A2−A1 = −0.012839 ms；冻结组 B2−B1 = +0.128658 ms。
- 组差 +0.108617 ms；冻结组自身漂移 +0.128658 ms 大于组差，说明 +1.19% 不是稳定因果，但也没有任何一轮冻结组低于关闭组均值的方向性证据。
- loop max 的关闭组 A2−A1 = −19.442708 ms，远大于组差 −15.112240 ms；因此 max 的“改善”不能单独当作稳定收益。
- loop p99 的关闭组漂移 +0.133287 ms、冻结组 −0.134640 ms，组差 −3.74 ms 远大于漂移，p99 的改善相对可信；但它没有转化为整帧收益。

25 s 窗口内的分布：

- 关闭组两轮分别有 3 个 >30 ms 帧；冻结组各 1 个。
- 冻结组的 FPS 提高 0.31%，但应用墙钟反而更慢；这是“FPS 取整/窗口口径”与“每帧成本”不一致的典型现象，不能只拿 FPS 说加速。
- HUD 和 glyph upload 的改善是真实的局部收益；它们被冻结图集的查表、分页、绘制调用和加载期成本抵消。

### 7. 质量与回归结果

| 对照 | 覆盖 | changed pixels | max diff | 结果 |
|---|---|---:|---:|---|
| case 0 PGR covered | COMBO/AUTOPLAY、数字、百分比 | 16562 | 71 | 有损，保留 |
| case 1 Harmony covered | 中文/Unicode、变换、宽度 | 8567 | 49 | 有损，保留 |
| case 2 normalized asset covered | 归一化字体资产 | 6600 | 71 | 有损，保留 |
| case 3 unknown whole section | 未覆盖整段 | 0 | 0 | 精确回退 |
| case 4 viewport mismatch | viewport 不匹配 | 0 | 0 | 精确回退 |
| case 5 normal quality rejected | 普通画质拒绝 | 0 | 0 | 精确回退 |

补充：

- 6 组对照、3 组覆盖组有差异、3 组回退组精确相同；这是“低画质有损 + 回退安全”的分类，不是无损。
- 差异集中在字形边缘，max diff 49–71/255；这是 phase4 量化改变栅格代表造成的，不能靠“肉眼不明显”升级为无损。
- 图集为 2 页 × 2048² × 4 = 32 MiB；Main general corpus 为 201 bases / 3216 entries；容器排序把旧 catalog 的 3 页 48 MiB 降到 2 页 32 MiB，但仍是研究默认关。
- 质量作业只覆盖固定字体、固定 DPI、固定设备、有限帧；没有覆盖自定义字体、Bold、所有 Unicode、所有字号、EGL loss。
- 回退组精确相同说明“未知/不匹配时不画错”；它不证明“覆盖组也无损”。

### 8. 为什么判定为负（逐项对比）

| 比较项 | 关闭 | 冻结 | 判定 |
|---|---:|---:|---|
| 应用墙钟 | 9.119857 ms | 9.228474 ms | 冻结慢 1.19% |
| FPS | 59.4876 | 59.6740 | 只高 0.31%，与墙钟方向矛盾 |
| loop p99 | 22.3424 ms | 18.6006 ms | 局部改善 |
| loop max | 57.9599 ms | 42.8477 ms | 漂移大于组差，不能算稳定 |
| >30 ms | 6 | 2 | 局部改善 |
| HUD 均值（汇总口径） | 约 1.31–1.36 ms | 约 1.03–1.06 ms | 局部改善 |
| glyph upload | 0.2821 ms | 0.0391 ms | 局部改善明显 |
| 质量 | 基准 | 3 组有损 | 不能进普通画质 |
| 加载/内存 | 无 | 32 MiB + 预计算 | 额外成本 |

逐项说明：

1. **整帧是最终标准**：HUD 和 glyph upload 是阶段指标，阶段下降不等于整帧下降。冻结图集把工作从 gameplay 移到加载期，并增加了查表、分页、quad 生成和额外的 draw 分组；这些成本落在 render 阶段，抵消了 HUD 的收益。
2. **+1.19% 的方向是慢**：虽然冻结组自身漂移 0.129 ms 大于组差 0.109 ms，不能把它称为“稳定回归”，但也没有任何正向证据。按“非正优化”判定，应当记为负优化，而不是“无优化”。
3. **p99 改善不等于尾延迟达标**：p99 从 22.34 降到 18.60 ms，但 18.60 ms 仍高于 16.67 ms 的刷新预算；>30 ms 仍有 2 帧。不能拿 p99 下降宣称“尖峰解决”。
4. **质量是硬门槛**：phase4 在 3 组覆盖画面产生 6600–16562 个 changed pixels，max 71；这不是“低画质可接受”就能自动进生产的理由。普通画质直接拒绝，说明设计上就不准备覆盖普通用户。
5. **加载期成本**：32 MiB 图集 + 预计算 + 2 页上传，增加加载时间和内存；本条只测了 25 s Playing，没有把加载成本摊到整局。
6. **结论**：冻结图集是一个“有效的 HUD 局部优化”，但不是“完整帧优化”。在 packed Main low 这组数据里，整帧是慢的，因此判定为负优化。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：本地提交 `74ec68d5d5bb023ba202807d2371d2bc23f7c035`（优化字形图集装箱），59 源冻结；SO `4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0` |
| 默认 | 关闭；`font_frozen_text` 缺省 false；只有 `sample_count==1 && chart_pre_compile` 才允许安装 |
| 普通路径 | 未接入；主项目没有 Config/UI 开关，没有默认开启；研究代码只在 `render_research` 构建里 |
| 主线是否已实施 | 否（main） |
| 研究后端 | SO `4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0`；第 11 节给出提交与冻结源 |
| 质量/契约 | 加载预检、16 相位、2 页图集、查表、整段回退、生命周期、质量/性能 cohort 完成；见第 7 节 |
| 是否“没实现” | 不是。加载预检、16 相位、2 页图集、查表、整段回退、生命周期、质量/性能 cohort 都完成 |
| 后续 | 字形方向后来转向不可变纹理存储/增量发布；冻结图集保留为“低画质 HUD 局部优化”的研究证据，不再单独推广 |

### 10. 可挽救的部分与后续条件

可挽救：

1. **glyph upload 从 0.282 ms 降到 0.039 ms**，说明“把栅格/上传移到加载期”这个方向是有效的；如果只针对 HUD 长帧，而不是整帧，它可以作为低画质候选。
2. **2 页 32 MiB** 的集装箱排布比旧 3 页 48 MiB 更紧，说明“按 padded height 排序”能减少页数；这个排布算法可以复用到其他图集。
3. **整段回退** 的设计是安全的：未知字形/字号/相位不会半画；这为后续“加载期精确预热 + 运行期只读”提供了模板。
4. **p99 改善** 说明冻结图集确实减少了部分 HUD 同步；如果能把 render 阶段的查表/分页成本降下来，可能转正。

后续条件：

- 必须让整帧墙钟不退化；当前 +1.19% 是负的，不能只拿 HUD 指标推广。
- 必须把 phase4 换成更高相位或精确缓存，或者只对“确实固定”的 HUD 文本使用；否则普通画质不能用。
- 必须覆盖自定义字体、Bold、所有字号、Unicode、DPI、其他 GPU；当前只有一种字体组合。
- 必须把加载期成本（32 MiB、预计算、上传）算进整局，而不是只看 Playing 25 s。
- 必须证明 >30 ms 帧和 p99 在更多窗口稳定下降；当前 2 个 >30 ms 仍存在，且 max 漂移大。
- 如果保留低画质限定，应在 UI 上明确“低画质可能有字形边缘差异”，不能默认开启。

### 11. 来源与可追溯性

- 源码提交：`74ec68d5d5bb023ba202807d2371d2bc23f7c035`；59 源冻结；SO SHA-256 `4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0`。
- ABBA 证据：`frozen-main-packed-abba-independent-v1/final-audit.json`、`a1-off/`、`b1-on/`、`b2-on/`、`a2-off/`。
- 质量证据：`frozen-text-packed-gpu-quality-v1-execution/results.json`、`runs/frozen-text-packed-gpu-20261011T180201-9bc2032b/frozen-text-probe.json`。
- 构建/检查：`frozen-text-gpu-android-check-v1.log`、`v2.log`、`v3.log`、`frozen-text-gpu-lab-build.log`、`frozen-text-gpu-windows-check-v1.log`。
- 设备：`8DF6R17309002674`；Mali-T880 / ES 3.2；2560×1440。
- 关键源码位置：`prpr/src/ui/frozen_text.rs`、`prpr/src/ui/text.rs`、`phira/src/render_research.rs`。

以下为 `本地证据/AGENTS.md` 中冻结文字相关段落的完整原文：

```text
据new质量Root正式授权Small Main，现actualrequire_escalated41549 live/id main-low-packed-small-abba-v1（唯一owner frozen_gpu_host_prepare），Mainpins绑定same4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0/74ec68d5d5bb023ba202807d2371d2bc23f7c035/59，复用CPU99 proof43f8c490aedc64cf854a7953d7ade9c51c3565155c5b7207daaa7c0efd9d0f2a不新guard。所有4轮font_frozen_text TRUE，只particle_small_stream OFF/ON/ON/OFF+id；sample1/aggressivefalse/compiletrue/commonRGBAon/PBO+fullstreamdiscard+forceMissingfalse，原音轨/判定不变。Root禁其它ADB/build/59sourceedit，续轮owner poll同41549不duplicate/timeout重启。actualprepare34660 terminal0/fixture20261011T185812-73c5312b，A1OFFlaunch33376 terminal0/run packed-small-main-a1-off-20261011T185816-b7c920ff，controls15736当时live原导航40squiet/natural，尚无sealed/actualCSV/新性能。旧Font四轮不混本Smallcohort。审计frozen_main_abba_audit已followup仅适配薄newFlags：OFF也须Frozeninstalled/positiveDraw/PGRpub0，bytes由sidecarinstances*64要DERIVED标签；0/257/missing合法fullfallback；共同source59/finally/old保护/commonChartdrift终态一次。collectedRootformalfollowup再审，不能读live。主trackedclean/HEAD74ec/notpush/font+small研究defaultoff/完整goalactive。

Main下一候选的payload bytes可按真实17-column sidecar discarded且full_discardfalse/SmallTRUE/实例数*64推导，必须标derived payload，不能称原生APIbytecounter。nativeprobe现有真实counter与shadergeometrytrace另核，不新加Mainshaderhook或新SO。qualityaccepted后才能formal许可固定FrozenTRUE、仅SmallOFF/ON/ON/OFF的Main4轮；此前Font4轮整体app+1.19%且各ON仍1>30，不默认推广/完整goalactive。

Playing loopP99依次22.275760/18.667870/18.533230/22.409047，max67.681250/36.111458/49.583854/48.238542，>30=3/1/1/3。两ON realinstalled32Mi/positiveDraw且PGRfirst25+startbefore pub/upload0；full自然lifetime covered38615/38612、draw57086/57100、quads174822/174811、fallback/oldqpc0；其它Painter仍28pub。Glyph局部均.282127→.039138ms/HUD1.306514→1.025630ms，chartlabel4.715181→4.912839只是观察值，不能归因给Frozen/GPU。两ON剩余Playing峰都t3.716/实际1+4粒子，p0/p1 updatewall6.204166+16.881250（B1）、8.612500+22.771875（B2），glyph0；不是所有峰由字体，wall不是GPUelapsed。字体机制成功且尾计数改善，但整体app不加速、loop尾仍未<30，defaultoff不推广生产，完整goalactive。

packed新后端已actual构建38127 terminal0/179.079s，frozen-text-packed-lab.so SHA4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0/HEAD74ec68d5d5bb023ba202807d2371d2bc23f7c035。freeze70764 terminal0/59working=frozen=normalizedGit，trackedclean，正常和研究Windows48306与CPUfinal5tests全通过；主未push/两库rev不改。新GPU host64 CPU9c2718 actual0/e96cda6ef962c3e3f53b93673c05bfde1bb4e0bc1bef2d674dcc5bfd97cdab2c，含实际旧PNGdirectnativeSHA正/再flip负；Main31CPU f0e103 actual0/809ef0e2a04eb48fbbce63930578ae91d7ed827684ae36abf4f73b2a29800a79，含27已有门+4boundedResultswaitmock。原工具/失败/两旧诊断源均保持，保护15选择证据非全部masshash。

图集装箱修正已actual本地提交74ec68d5d5bb023ba202807d2371d2bc23f7c035（优化字形图集装箱，main不push/clean）。仅frozen_text.rs：加载CPU预flight完整finite矩形后按paddedheight/width降序+完整Key确定tie排序，保留phase4、完整通用corpus/3exactsizes/原32MiB预算/runtime文字顺序及RGBA，新增boundedtryreserve/MAXBASES/checkedcount。actualCPU66235 terminal0/5tests：Main PGR无fallback/primary1/fontScale1/2560宽/原f32乘法次序，201bases3216entries，旧catalog shelf3pages48MiB→新2pages32MiB；每Key bounds/确定placement/整padding无重叠/whiteRGBalpha0边框/独立glyph-localRGBA crop逐字节等通过。第一35387 actual0却0tests（错filter）不称通过，45073五项actual0是较早版本，final66235才完整oracle。Windows default与research48306 actual0，独立只读源码review无阻断。GPU UV/page-run与draw-call分组可变，CPU相等不代表GPU精确同/FPS；仍需新GPUquality和Main实际消费者，仍defaultoff/low only，无实际新收益。
```

引用说明：上述原文中的哈希缩写已按原始证据补全；本文第 6 节的数值已完整展开，不依赖缩写。GL 规范/外部资料未在本文给出链接。

---

## N-04 粒子小缓冲 / stream discard

### 1. 一句话结论与判定

判定：**负优化（混合）**。这里有两个独立候选：

- **粒子小缓冲（16 KiB small stream）**：质量通过（12 组 RGBA 相同），但整帧组均值只从 9.1709 ms 降到 9.1111 ms（−0.65%），而且关闭组自身漂移 −0.1395 ms 大于组差 −0.0598 ms，不能建立稳定收益；B1 单轮的 59.6765 fps / max 27.5927 ms / 0 个 >30 ms 只是单轮现象。
- **全量 stream discard（768,000 bytes/frame）**：一组 ABBA 从 11.4437 ms 升到 12.1625 ms（+6.28%），FPS 53.12→46.80，>30 ms 帧 23→47；这是明确的负优化。

因此本条不能记作“粒子优化成立”。小缓冲最多是“在极少数 ≤256 实例的批次上无害”，全量 discard 则是把驱动存储压力/同步成本带进来，反而更慢。

关键数字：

- 小缓冲质量：18 native cases / 69 raw / 12 RGBA pairs，changed=0、max=0。
- 小缓冲性能：A1 9.2407 / B1 9.0975 / B2 9.1247 / A2 9.1011 ms；off 9.1709，on 9.1111，−0.65%；off 漂移 −0.1395 ms > 组差。
- 小缓冲 B1：59.6765 fps，max 27.5927 ms（约 27.6 ms），0 个 >30 ms。
- 全量 discard：A1 10.6470 / B1 12.1920 / B2 12.1330 / A2 12.2405 ms；off 11.4437，on 12.1625，+6.2806%；FPS 53.1233→46.8009；>30 ms 23→47。
- 全量 discard 的 off 自身漂移 +1.5935 ms，大于组差 +0.7187 ms；所以 +6.28% 也不能称为“稳定因果回归”，但它同样没有任何正向证据，且每一轮 ON 都比第一轮 OFF 慢。

### 2. 问题与初衷

粒子发射器每帧把所有活跃粒子实例上传到一条 `Buffer::stream`。满容量是 12000 个 `GpuParticle`，每个 64 bytes，总 768,000 bytes。`Buffer::stream` 的经典用法是每帧 `update`：驱动可能 orphan 旧存储、分配新存储、再拷贝；如果旧 buffer 正被 GPU 读取，驱动可能等待或做隐式同步。

两个候选的初衷不同：

1. **小缓冲**：对“实例数 ≤256、字节 ≤16 KiB”的小批次，改用一条固定的 16 KiB `Buffer::stream`，并调用 `update_discard`（orphan + subdata）。这样小批次不必占用 768 KB 的大 buffer，可能减少驱动分配/同步成本。
2. **全量 discard**：对满容量批次也调用 `update_discard`，用 `glBufferData(NULL, STREAM_DRAW)` + `glBufferSubData` 取代普通 `update`，试图避免“旧存储仍在被读”的等待。

这两个候选都受同一个约束：`update_discard` 只是“允许驱动 orphan”，不保证 GL 分配成功，也不保证驱动真的不等待。API 接受不等于 GPU 完成，更不等于物理内存回收。

### 3. 实现思路与具体机制

1. **决策在触碰 GL 之前**：`upload_decision(requested, instances, stride, present)` 先做纯整数判断，返回 `Empty / Off / Missing / Oversize / Attempt`。
2. **空批次**：`instances == 0` 直接 `Empty`，不绑定、不上传。
3. **关闭**：`requested == false` 返回 `Off`，走原 `full_stream.update`；如果同时请求了全量 discard，则先尝试 `full_stream.update_discard`，接受就返回，否则回原 update。
4. **溢出**：`instances * stride` 溢出，或 `instances > 256`，或 `bytes > 16384`，返回 `Oversize`；不进入小缓冲，回退 full update。
5. **缺失**：小缓冲不存在（force_missing、创建失败、context 不匹配）返回 `Missing`；回退 full update。
6. **尝试**：`Attempt` 时调用 `buffer.update_discard(ctx, &gpu_particles[..])`；返回 true 才绑定小缓冲并记录 accepted；返回 false 记录 declined，回退 full update。
7. **小缓冲所有权**：`OwnedSmallStream` 是唯一删除所有者；`Drop` 只把 `Retirement<Buffer>` 放进全局队列；`flush_pending_deletions` 在渲染线程 flush、清 binding、删除；foreign context 只丢弃不删除。
8. **计数器**：`accepted_uploads/accepted_bytes/missing_uploads/oversize_uploads/declined_uploads` 只在请求开启时累加；`record_stream_discard` 单独记录 full discard 的 accepted/fallback。
9. **绑定恢复**：每帧 `update` 开头把 `bindings.vertex_buffers[1]` 重置为 `full_stream`；只有小缓冲接受后才改绑 small，避免上一帧的 small 绑定泄漏到下一帧。
10. **不改变粒子语义**：粒子数量、RNG、寿命、顺序、颜色、mesh_dirty 都不变；只改变实例数据上传的 buffer 和 API。

### 4. 文件、符号与完整核心代码

相关符号：

- `prpr/src/particle_stream.rs`：`SMALL_STREAM_INSTANCES`、`SMALL_STREAM_CAPACITY`、`UploadDecision`、`upload_decision`、`Retirement`、`OwnedSmallStream`、`flush_pending_deletions`、`retirement_report`。
- `prpr/src/particle.rs`：`Emitter::full_stream`、`small_stream`、`last_stream_discarded`、`last_small_stream`、`last_stream_capacity`、`Emitter::new`、`Emitter::update`、`Emitter::update_research_stream`、`research_stream_strategy`、`research_small_stream_retirement_report`。
- `prpr/src/main_render_timing.rs`：`SmallStreamReport`、`SmallStreamOutcome`、`research_small_stream`、`research_stream_discard`、`record_small_stream_attempt`、`record_stream_discard`、`research_small_stream_report`、`research_stream_discard_report`。
- 研究请求字段：`particle_small_stream`、`particle_small_stream_force_missing`、`particle_stream_discard`、`particle_small_stream_probe`。

#### 4.1 常量与纯决策函数

```rust
pub(crate) const SMALL_STREAM_INSTANCES: usize = 256;
pub(crate) const SMALL_STREAM_CAPACITY: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadDecision {
    Empty,
    Off,
    Missing { bytes: usize },
    Oversize { bytes: usize },
    Attempt { bytes: usize },
}

pub(crate) fn upload_decision(requested: bool, instances: usize, stride: usize, present: bool) -> UploadDecision {
    if instances == 0 {
        return UploadDecision::Empty;
    }
    if !requested {
        return UploadDecision::Off;
    }
    let bytes = match instances.checked_mul(stride) {
        Some(bytes) if bytes != 0 => bytes,
        _ => return UploadDecision::Oversize { bytes: 0 },
    };
    if instances > SMALL_STREAM_INSTANCES || bytes > SMALL_STREAM_CAPACITY {
        UploadDecision::Oversize { bytes }
    } else if !present {
        UploadDecision::Missing { bytes }
    } else {
        UploadDecision::Attempt { bytes }
    }
}
```

顺序很重要：先判空、再判 off、再算字节、再判超限、最后判 missing。这样“关闭”不会要求小缓冲存在，“空批次”不会产生 missing 计数，“溢出”不会被误判为 missing。

#### 4.2 退休记录：同一资源只交出一次

```rust
struct Retirement<T> {
    context: usize,
    resource: Option<T>,
}

impl<T> Retirement<T> {
    fn new(context: usize, resource: T) -> Self {
        Self { context, resource: Some(resource) }
    }

    fn take_for_context(&mut self, context: usize) -> Option<T> {
        let resource = self.resource.take()?;
        (self.context == context).then_some(resource)
    }
}
```

`take_for_context` 先 `take()` 再比较 context；即使 context 不匹配，资源也已经从记录里移除，不会在后续 context 里被二次删除。这是防止 GLuint 复用误删的关键。

#### 4.3 小缓冲的创建、取用与 Drop

```rust
pub(crate) struct OwnedSmallStream {
    retirement: Option<Retirement<Buffer>>,
}

impl OwnedSmallStream {
    pub(crate) fn new(context: &mut Context, force_missing: bool) -> Option<Self> {
        if force_missing || !gl::is_function_loaded("glGetError") {
            return None;
        }
        let prior_error = unsafe { gl::glGetError() };
        if prior_error != 0 {
            tracing::warn!("Particle small stream not created: pre-existing GL error 0x{prior_error:x}");
            return None;
        }
        let buffer = Buffer::stream(context, BufferType::VertexBuffer, SMALL_STREAM_CAPACITY);
        let owner = Self {
            retirement: Some(Retirement::new(context.context_id(), buffer)),
        };
        {
            let mut state = RETIRED.lock().unwrap();
            state.created_owners = state.created_owners.saturating_add(1);
        }
        let creation_error = unsafe { gl::glGetError() };
        if creation_error != 0 {
            tracing::warn!("Particle small stream allocation failed: GL error 0x{creation_error:x}");
            return None;
        }
        Some(owner)
    }

    pub(crate) fn buffer_for_context(&self, context: usize) -> Option<Buffer> {
        let retirement = self.retirement.as_ref()?;
        if retirement.context != context {
            return None;
        }
        retirement.resource
    }
}

impl Drop for OwnedSmallStream {
    fn drop(&mut self) {
        if let Some(retirement) = self.retirement.take() {
            let mut state = RETIRED.lock().unwrap();
            state.queued = state.queued.saturating_add(1);
            state.pending.push(retirement);
        }
    }
}
```

创建前检查“已有 GL 错误”很重要：如果前面某个操作留下错误，`Buffer::stream` 创建后的 `glGetError` 可能把旧错误误算到小缓冲头上。这里先读一次，非 0 就不创建，并保留日志。

#### 4.4 删除钩子与计数

```rust
pub(crate) fn flush_pending_deletions() {
    let retired = std::mem::take(&mut RETIRED.lock().unwrap().pending);
    if retired.is_empty() {
        return;
    }
    let mut internal = unsafe { get_internal_gl() };
    internal.flush();
    let context = internal.quad_context;
    let mut deleted = 0_u64;
    let mut foreign = 0_u64;
    for mut retirement in retired {
        if let Some(buffer) = retirement.take_for_context(context.context_id()) {
            context.clear_resource_bindings();
            buffer.delete();
            deleted = deleted.saturating_add(1);
        } else {
            foreign = foreign.saturating_add(1);
        }
    }
    let mut state = RETIRED.lock().unwrap();
    state.deleted = state.deleted.saturating_add(deleted);
    state.foreign = state.foreign.saturating_add(foreign);
}

pub(crate) fn retirement_report() -> (u64, u64, u64, u64, usize) {
    let state = RETIRED.lock().unwrap();
    (state.created_owners, state.queued, state.deleted, state.foreign, state.pending.len())
}
```

`flush_pending_deletions` 由已有的主渲染线程删除钩子调用；它先 flush，确保延迟队列里的 managed draw 已经消费绑定，再清 binding、删除。`deleted` 只表示“提交了删除 API”，不表示驱动已物理回收。

#### 4.5 `Emitter` 的字段与加载期创建

```rust
pub struct Emitter {
    pipeline: Pipeline,
    bindings: Bindings,
    #[cfg(feature = "render_research")]
    full_stream: Buffer,
    #[cfg(feature = "render_research")]
    small_stream: Option<crate::particle_stream::OwnedSmallStream>,
    post_processing_pass: RenderPass,
    post_processing_pipeline: Pipeline,
    post_processing_bindings: Bindings,

    gpu_particles: Vec<GpuParticle>,
    cpu_counterpart: Vec<CpuParticle>,

    last_emit_time: f32,
    time_passed: f32,

    particles_spawned: u64,
    position: Vec2,

    batched_size_curve: Option<BatchedCurve>,

    blend_mode: BlendMode,
    mesh_dirty: bool,
    #[cfg(feature = "render_research")]
    last_stream_discarded: bool,
    #[cfg(feature = "render_research")]
    last_small_stream: bool,
    #[cfg(feature = "render_research")]
    last_stream_capacity: usize,

    pub config: EmitterConfig,
}
```

```rust
let positions_vertex_buffer = Buffer::stream(ctx, BufferType::VertexBuffer, Self::MAX_PARTICLES * std::mem::size_of::<GpuParticle>());

#[cfg(feature = "render_research")]
let small_stream = crate::particle_stream::OwnedSmallStream::new(
    ctx,
    crate::main_render_timing::research_small_stream_force_missing(),
);
```

小缓冲在加载期创建，大小固定 16 KiB；`force_missing` 控制是否故意不创建，用于验证 missing 回退。

#### 4.6 每帧重置绑定

```rust
fn update(&mut self, ctx: &mut Context, dt: f32) {
    #[cfg(feature = "render_research")]
    {
        self.last_stream_discarded = false;
        self.last_small_stream = false;
        self.last_stream_capacity = self.full_stream.size();
        self.bindings.vertex_buffers[1] = self.full_stream;
    }
    if self.mesh_dirty {
        #[cfg(feature = "render_research")]
        let stream = self.full_stream;
        #[cfg(not(feature = "render_research"))]
        let stream = self.bindings.vertex_buffers[1];
        self.bindings = self
            .config
            .shape
            .build_bindings(ctx, stream, self.config.texture);
        self.mesh_dirty = false;
    }
    if self.config.emitting {
        self.time_passed += dt;

        let gap = (self.config.lifetime / self.config.amount as f32) * (1.0 - self.config.explosiveness);

        let spawn_amount = if gap < 0.001 {
            self.config.amount as usize
        } else {
            ((self.time_passed - self.last_emit_time) / gap) as usize
        };

        for _ in 0..spawn_amount {
            self.last_emit_time = self.time_passed;

            if self.particles_spawned < self.config.amount as u64 {
                self.emit_particle(vec2(0.0, 0.0));
            }

            if self.gpu_particles.len() >= self.config.amount as usize {
                break;
            }
        }
    }

    if self.config.one_shot && self.time_passed > self.config.lifetime {
        self.time_passed = 0.0;
        self.last_emit_time = 0.0;
        self.config.emitting = false;
    }

    for (gpu, cpu) in self.gpu_particles.iter_mut().zip(&mut self.cpu_counterpart) {
        cpu.velocity += cpu.velocity * self.config.linear_accel * dt;
        cpu.angular_velocity += cpu.angular_velocity * self.config.angular_accel * dt;
        cpu.angular_velocity *= 1.0 - self.config.angular_damping;

        gpu.color = {
            let t = cpu.lived / cpu.lifetime;
            if t < 0.5 {
                let t = t * 2.;
                self.config.colors_curve.start.to_vec() * (1.0 - t) + self.config.colors_curve.mid.to_vec() * t
            } else {
                let t = (t - 0.5) * 2.;
                self.config.colors_curve.mid.to_vec() * (1.0 - t) + self.config.colors_curve.end.to_vec() * t
            }
        };
        gpu.color *= cpu.color.to_vec();
        gpu.pos += vec4(cpu.velocity.x, cpu.velocity.y, cpu.angular_velocity, 0.0) * dt;

        gpu.pos.w = cpu.initial_size * self.batched_size_curve.as_ref().map_or(1.0, |curve| curve.get(cpu.lived / cpu.lifetime));

        if cpu.lifetime != 0.0 {
            gpu.data.y = cpu.lived / cpu.lifetime;
        }

        cpu.lived += dt;
        cpu.velocity += self.config.gravity * dt;

        if let Some(atlas) = &self.config.atlas {
            if cpu.lifetime != 0.0 {
                cpu.frame = (cpu.lived / cpu.lifetime * (atlas.end_index - atlas.start_index) as f32) as u16 + atlas.start_index;
            }

            let x = cpu.frame % atlas.n;
            let y = cpu.frame / atlas.n;

            gpu.uv = vec4(x as f32 / atlas.n as f32, y as f32 / atlas.m as f32, 1.0 / atlas.n as f32, 1.0 / atlas.m as f32);
        } else {
            gpu.uv = vec4(0.0, 0.0, 1.0, 1.0);
        }
    }

    for i in (0..self.gpu_particles.len()).rev() {
        if self.cpu_counterpart[i].lived >= self.cpu_counterpart[i].lifetime || self.cpu_counterpart[i].lived > self.config.lifetime {
            if self.cpu_counterpart[i].lived != self.cpu_counterpart[i].lifetime {
                self.particles_spawned -= 1;
            }
            self.gpu_particles.swap_remove(i);
            self.cpu_counterpart.swap_remove(i);
        }
    }

    if !self.gpu_particles.is_empty() {
        #[cfg(feature = "render_research")]
        {
            self.update_research_stream(ctx);
        }
        #[cfg(not(feature = "render_research"))]
        self.bindings.vertex_buffers[1].update(ctx, &self.gpu_particles[..]);
    }
}
```

这里只改了“末尾上传”和开头的绑定重置；粒子模拟本身没有改。

#### 4.7 `update_research_stream`：小缓冲优先、full discard 独立

```rust
#[cfg(feature = "render_research")]
fn update_research_stream(&mut self, ctx: &mut Context) {
    use crate::{
        main_render_timing::{self, SmallStreamOutcome},
        particle_stream::{upload_decision, UploadDecision},
    };

    let small = self.small_stream.as_ref().and_then(|owner| owner.buffer_for_context(ctx.context_id()));
    let decision = upload_decision(
        main_render_timing::research_small_stream_requested(),
        self.gpu_particles.len(),
        std::mem::size_of::<GpuParticle>(),
        small.is_some(),
    );
    match decision {
        UploadDecision::Attempt { bytes } => {
            let buffer = small.unwrap();
            if buffer.update_discard(ctx, &self.gpu_particles[..]) {
                self.bindings.vertex_buffers[1] = buffer;
                self.last_stream_discarded = true;
                self.last_small_stream = true;
                self.last_stream_capacity = buffer.size();
                main_render_timing::record_small_stream_attempt(SmallStreamOutcome::Accepted, bytes);
                return;
            }
            main_render_timing::record_small_stream_attempt(SmallStreamOutcome::Declined, bytes);
        }
        UploadDecision::Missing { bytes } => main_render_timing::record_small_stream_attempt(SmallStreamOutcome::Missing, bytes),
        UploadDecision::Oversize { bytes } => main_render_timing::record_small_stream_attempt(SmallStreamOutcome::Oversize, bytes),
        UploadDecision::Off => {
            let requested = main_render_timing::stream_discard_requested();
            self.last_stream_discarded = requested && self.full_stream.update_discard(ctx, &self.gpu_particles[..]);
            if requested {
                main_render_timing::record_stream_discard(
                    self.last_stream_discarded,
                    self.gpu_particles.len() * std::mem::size_of::<GpuParticle>(),
                );
            }
            if self.last_stream_discarded {
                return;
            }
        }
        UploadDecision::Empty => return,
    }
    self.full_stream.update(ctx, &self.gpu_particles[..]);
}
```

注意两个“不会发生”的交叉：

- 小缓冲请求和 full discard 请求被上层明确禁止同时为 true（`render_research.rs` 的 `ensure!`）。因此 `UploadDecision::Off` 只会在 small 关闭时出现。
- 小缓冲 `Attempt` 失败后走 `full_stream.update`，不是 `update_discard`；所以 small 路径不会偷偷变成 full discard。
- `Missing` / `Oversize` 也走 full update，不记录 full discard。

#### 4.8 策略回报

```rust
#[cfg(feature = "render_research")]
pub fn research_stream_strategy(&self) -> (bool, bool, usize) {
    (self.last_stream_discarded, self.last_small_stream, self.last_stream_capacity)
}

#[cfg(feature = "render_research")]
pub fn research_small_stream_retirement_report() -> (u64, u64, u64, u64, usize) {
    crate::particle_stream::retirement_report()
}
```

这两个函数只读计数/策略，不参与热路径。

#### 4.9 开关函数

```rust
pub fn research_small_stream(enabled: bool, force_missing: bool) {
    SMALL_STREAM.with(|slot| slot.set(SmallStreamReport {
        requested: enabled, force_missing, ..SmallStreamReport::default()
    }));
}

pub(crate) fn research_small_stream_requested() -> bool {
    SMALL_STREAM.with(|slot| slot.get().requested)
}

pub(crate) fn research_small_stream_force_missing() -> bool {
    SMALL_STREAM.with(|slot| slot.get().force_missing)
}

pub fn research_stream_discard(enabled: bool) {
    STREAM_DISCARD.with(|slot| slot.set(StreamDiscardReport {
        requested: enabled,
        accepted_uploads: 0,
        accepted_bytes: 0,
        fallback_uploads: 0,
    }));
}

pub(crate) fn stream_discard_requested() -> bool {
    STREAM_DISCARD.with(|slot| slot.get().requested)
}
```

每次选择都会重置计数，避免把上一轮的选择累计到下一轮。

### 5. 开关、默认值、构建特性

| 项目 | 值 | 说明 |
|---|---|---|
| 研究请求字段 | `particle_small_stream` | 请求 16 KiB 小缓冲 |
| 研究请求字段 | `particle_small_stream_force_missing` | 强制不创建小缓冲，验证 missing 回退 |
| 研究请求字段 | `particle_stream_discard` | 请求全量 `update_discard` |
| 研究请求字段 | `particle_small_stream_probe` | 单独的 GPU probe；不允许和 Main 混跑 |
| 默认值 | 全部 false/null | 请求缺省即关闭 |
| 互斥 | small 与 full discard 不能同时 true | 上层 `ensure!` 强制 |
| 构建特性 | `render_research` | 研究后端总开关 |
| 库依赖 | miniquad 参考实现的 `Buffer::update_discard` | 生产库没有这个扩展 |
| 生产默认 | 不启用 | 没有 Config/UI，未进 main |
| 小缓冲容量 | 16 KiB / 256 instances | 每实例 64 bytes |
| 满缓冲容量 | 12000 × 64 = 768,000 bytes | 每帧全量 |

“默认关”是必要的：小缓冲只覆盖 ≤256 实例的批次，而粒子密集场景往往超过；全量 discard 在 Mali 上可能引入额外存储压力。两者都没有生产默认。

### 6. 完整测量数据

设备：`8DF6R17309002674`，Mali-T880 / ES 3.2，2560×1440，Android 8。谱面：PGR；autoplay；Main 真实 UI；25 s Playing 窗口；ABBA 顺序 off/on/on/off。

#### 6.1 小缓冲（particle_small_stream）质量

| 项目 | 结果 |
|---|---|
| native cases | 18 |
| raw 文件 | 69 |
| RGBA pairs | 12 |
| changed pixels | 0 |
| max RGBA difference | 0 |
| OFF 策略计数 | 0 |
| ON accepted | 4 次 / 16,576 bytes |
| forceMissing accepted | 0 |
| forceMissing missing | 4 |
| oversize | 1 |
| 第二帧 flush 增量 | 0 |
| GL error / pending / foreign | 0 |

质量覆盖空批次、1 个、256 个、257 个、transition、rebuild、forceMissing 和 oversize。12 组 RGBA 全部相同说明“小缓冲接受时画面不变”；它不证明所有粒子数/所有 GPU 都一样。

#### 6.2 小缓冲 Main ABBA（25 s Playing）

| 次序 | 策略 | 应用均值 ms | FPS | loop p99 ms | loop max ms | >30 ms 帧 |
|---:|---|---:|---:|---:|---:|---:|
| 0 | 关闭（A1） | 9.240672 | 59.714265 | 18.798437 | 33.756771 | 1 |
| 1 | 小缓冲（B1） | 9.097520 | 59.676523 | 19.325484 | 27.592708 | 0 |
| 2 | 小缓冲（B2） | 9.124658 | 59.665727 | 19.505755 | 24.526563 | 0 |
| 3 | 关闭（A2） | 9.101124 | 59.715153 | 18.762604 | 36.760417 | 1 |

组均值：off 9.170898 ms，on 9.111089 ms，`−0.65216%`。FPS off 59.714709，on 59.671125，`−0.0730%`。loop p99 off 18.780521，on 19.415620（`+3.38%`，更差）。loop max off 35.258594，on 26.059636（`−26.09%`）。

漂移：

- off A2−A1 = −0.139547 ms；on B2−B1 = +0.027137 ms。
- 组差 −0.059809 ms；off 自身漂移 0.139547 ms 大于组差，`absolute_off_drift_exceeds_absolute_group_delta = true`。
- 因此 −0.65% 不能算稳定收益；B1 的 max 27.59 / 0>30 是单轮，B2 的 max 24.53 / 0>30 也是单轮，两轮 ON 都低于两轮 OFF 的 max，但 OFF 的 A2 自己升到 36.76，说明 max 受 run 内事件影响很大。

#### 6.3 全量 discard ABBA（25 s Playing）

| 次序 | 策略 | 应用均值 ms | FPS | loop p99 ms | loop max ms | >30 ms 帧 |
|---:|---|---:|---:|---:|---:|---:|
| 0 | 关闭（A1） | 10.646958 | 59.368756 | 22.170250 | 64.635938 | 4 |
| 1 | 全量 discard（B1） | 12.191961 | 46.763425 | 32.574656 | 72.006770 | 25 |
| 2 | 全量 discard（B2） | 12.132952 | 46.838327 | 30.812078 | 76.636979 | 22 |
| 3 | 关闭（A2） | 12.240480 | 46.877795 | 31.060855 | 76.550521 | 19 |

组均值：off 11.443719 ms，on 12.162457 ms，`+6.28063%`。FPS off 53.123276，on 46.800876（`−11.90%`）。loop p99 off 26.615552，on 31.693367（`+19.08%`）。loop max off 70.593230，on 74.321875（`+5.28%`）。>30 ms 累计 off 23，on 47。

漂移：

- off A2−A1 = +1.593522 ms；on B2−B1 = −0.059009 ms。
- 组差 +0.718738 ms；off 自身漂移 1.593522 ms 大于组差。因此“+6.28%”不是稳定因果回归；但 A1 之后设备明显变慢，A2 也慢，ON 两轮都在 12.13–12.19 ms，无法证明 ON 有收益。
- 更关键的是：B1/B2 的 FPS 只有 46.76/46.84，>30 ms 帧 25/22；即使把 A2 的漂移考虑进去，ON 也没有任何正向指标。

### 7. 质量与回归结果

| 项目 | 小缓冲 | 全量 discard |
|---|---|---|
| 画面 RGBA | 12 组全部相同 | 质量 cohort 中实际 ON 每 nonzero 都 discard，画面相同 |
| 空/零批次 | 不绑定、不上传 | 不调用 discard |
| oversize 257 | 回退 full update | 仍走 full update |
| forceMissing | 4 次 missing，画面相同 | 不适用 |
| 第二帧 flush | 不增加 | 不重复删除 |
| GL error | 0 | 0 |
| 物理内存回收 | 未测 | 未测 |
| EGL loss | 未测 | 未测 |

小缓冲的 12 组 RGBA 相同是“有限像素证据”；它覆盖了 0/1/256/257/transition/rebuild，但没有覆盖所有粒子材质、所有 atlas、所有混合模式。全量 discard 的质量对照只覆盖有限场景，不能外推全曲/所有 GPU。

### 8. 为什么判定为负/混合（逐项对比）

| 比较项 | 小缓冲 off | 小缓冲 on | 全量 discard off | 全量 discard on |
|---|---:|---:|---:|---:|
| 应用均值 | 9.1709 ms | 9.1111 ms | 11.4437 ms | 12.1625 ms |
| 相对变化 | — | −0.65% | — | +6.28% |
| FPS | 59.7147 | 59.6711 | 53.1233 | 46.8009 |
| loop p99 | 18.7805 | 19.4156 | 26.6156 | 31.6934 |
| loop max | 35.2586 | 26.0596 | 70.5932 | 74.3219 |
| >30 ms | 2 | 0 | 23 | 47 |
| off 漂移 | — | — | — | 1.5935 ms |
| 组差 | — | 0.0598 ms | — | 0.7187 ms |
| 漂移 > 组差 | — | 是 | — | 是 |

逐项说明：

1. **小缓冲覆盖率太低**：只有当实例数 ≤256 且字节 ≤16 KiB 时才尝试；PGR 的密集粒子段很容易超过 256，实际 accepted 只有 4 次/16,576 bytes。大多数帧仍走 full buffer，所以整帧不可能有大收益。
2. **小缓冲的方向不一致**：应用均值 −0.65%，但 FPS −0.07%，loop p99 +3.38%（更差）。三个指标没有同向，不能称优化。
3. **小缓冲漂移大于组差**：off 自身 −0.1395 ms，组差 −0.0598 ms；B1/B2 的“改善”可能是 run 顺序/设备状态造成，不是 small buffer 造成。
4. **全量 discard 明确更慢**：+6.28% 应用均值、−11.9% FPS、+19.1% p99、>30 ms 从 23 增到 47。即使 off 的 A2 也慢，ON 两轮仍然没有一轮回到 A1 的水平。
5. **机制解释**：Mali 的 `glBufferData(NULL)` + `glBufferSubData` 可能引入新的分配/存储压力；每帧 768 KB 的 discard 不等于“零等待”，驱动可能仍然同步或做额外拷贝。API 返回 true 只代表调用被接受，不代表 GPU 完成。
6. **结论**：小缓冲是“覆盖不足的微优化”，全量 discard 是“负优化”。合并判定为“负优化/混合”，不能进生产。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：小缓冲在 `93c4b90529a6330e6a7ba566908bc30510608f95` 接入；统一入口在 `b4c8fdd0961bec7d24205160f7db9229b0d957b8`；后续容器提交 `74ec68d5d5bb023ba202807d2371d2bc23f7c035` 保留该路径 |
| 默认 | 关闭；`particle_small_stream` / `particle_stream_discard` 缺省 false；研究请求必须显式开启 |
| 普通路径 | 未接入；主项目没有 Config/UI；`Buffer::update_discard` 来自 miniquad 参考实现，生产库没有该扩展 |
| 主线是否已实施 | 否（main） |
| 研究后端 | 小缓冲 SO `5a5d74977b293be06ab4b705fba559ba319d74111b0bd698e732d7f53b56bbf5`（首版）、`4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0`（容器后）；全量 discard SO `96d727934a541419e621af739f784a83a2b0d04cd72fd2acc9cef0275de7bd35` |
| 质量/契约 | 决策、创建、绑定、回退、删除所有权、质量/性能 cohort 完成；见第 7 节 |
| 是否“没实现” | 不是。决策、创建、绑定、回退、删除所有权、质量/性能 cohort 都完成 |
| 后续 | 小缓冲保留为“低实例数候选”；全量 discard 不再推广 |

### 10. 可挽救的部分与后续条件

可挽救：

1. **小缓冲的 12 组 RGBA 相同**，说明对 ≤256 实例的批次，16 KiB buffer 的语义是正确的；如果未来有大量小发射器，这个路径可以按“只在 small 时绑定”保留。
2. **决策函数是纯整数、无 GL**，可以独立测试/复用；它把 empty/off/missing/oversize 分得很清楚，不会把错误状态带进 GL。
3. **删除所有权**的设计（唯一 owner、Drop 只入队、渲染线程删除、foreign 不删）可以复用到其他流式 buffer。
4. **B1/B2 的 max 24.53–27.59 ms、0>30** 说明在某些窗口里尾部确实更好；但漂移大，不能作为稳定收益。

后续条件：

- 小缓冲必须提高覆盖率：要么增大容量/实例上限，要么按发射器实际最大批次选择；当前 256/16 KiB 太低。
- 必须把“small accepted 帧”和“full fallback 帧”分开统计；当前整帧均值把大多数 full 帧混在一起，掩盖了 small 的真实效果。
- 全量 discard 需要单独测驱动存储压力、GPU elapsed、温度/频率；当前只有墙钟，不能解释 +6.28% 的根因。
- 必须验证所有粒子材质/atlas/混合模式，以及 EGL loss；当前只有有限 RGBA。
- 如果保留 small，必须在 UI/Config 明确“只对低实例数批次生效”，不能宣称“粒子全面优化”。

### 11. 来源与可追溯性

- 小缓冲源码提交：`93c4b90529a6330e6a7ba566908bc30510608f95`；统一入口：`b4c8fdd0961bec7d24205160f7db9229b0d957b8`；容器：`74ec68d5d5bb023ba202807d2371d2bc23f7c035`。
- 小缓冲 SO：`5a5d74977b293be06ab4b705fba559ba319d74111b0bd698e732d7f53b56bbf5`（首版）、`4d5e003d4341717b2d3f6dd4b1d75f8c2f4113c686948e1225ae469a762843b0`（容器后）。
- 全量 discard SO：`96d727934a541419e621af739f784a83a2b0d04cd72fd2acc9cef0275de7bd35`；源码 `b4c8fdd0961bec7d24205160f7db9229b0d957b8`；50 源。
- 小缓冲质量：`packed-particle-small-quality-v1-execution/results.json`；Main：`packed-small-main-independent-v1/final-audit.json`。
- 全量 discard：`main-discard-abba-summaries/main-normal-discard-abba-v1/summary.json`。
- 设备：`8DF6R17309002674`；Mali-T880 / ES 3.2；2560×1440。
- 库依赖：miniquad 参考实现 `4b26d719a50165d7cd45cb2dc2590b03e6aa09e9`（`update_discard`），macroquad 配套 `cd0ab01b23998c113312eabffc8a4ba0dc65e740`。

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。以下为 `本地证据/AGENTS.md` 中粒子小缓冲/discard 相关段落的完整原文；历史引文中的分支标识已归一化为“参考实现”，其余原文未改：

```text
SmallStream候选本轮已实际实现/构建，local commit93c4b90529a6330e6a7ba566908bc30510608f95（试验预分配粒子小缓冲，main未push），particle-small-lab SO5a5d74977b293be06ab4b705fba559ba319d74111b0bd698e732d7f53b56bbf5/52冻结源逐Git核，Mini4b26/Macrocd0依赖不变。默认false且仅research；所有研究Emitter加载期固定full768000和small16384/256实例（off/on同资源），非empty且fits/available才small discard，接受后绑定small；缺失/超限/拒绝回full legacy update、不full discard、不热allocation/改Vec上限/RNG/粒子寿命/顺序。mesh_dirty使用固定full字段，保留small owner。new particle_stream.rs owner Drop仅globalMutex入队，existing ext主线程hook有退休才flush/context核/clear_resource_bindings/delete；foreign不删、duplicateflush不重复，加载GL error检查无热查询。GL allocation accepted不等于GL成功或物理memory，新owner仅small，旧full/geometry/pipeline等无Drop问题仍未解决/managedUID不是EGLloss检测。std-only 9decision/retirement+11stats/capture tests实际通过，Windowsdefault26805/research62991、Android54839（3m40s）、录核63924全部actualterminal0，无源注入。

真实Main粒子discard ABBA已实际完整执行并核验，不支持默认推广：main-normal-discard-abba-v1，same readySO96d72793/b4c8fdd/50源，A1 discard-main-a1-off-20261011T084529-1094f448、B1 discard-main-b1-on-20261011T085934-aa821ac6、B2 discard-main-b2-on-20261011T091723-b8cdeadc、A2 discard-main-a2-off-20261011T092605-d4f5cddd。所有launch/collect/ns各自通过，request除id+candidate一致、startupConfig四轮全同/sample2/aggressivefalse/precompiletrue/particletrue/actual全部autoplay；6raw files/全source/部署/逐ns forward+boundary/schema2真实策略复核。on每nonzero实际discardtrue，off/zero false，圈定真实Main首25s含冷段，不是observer或全曲。4Playing app均10.646958/12.191961/12.132952/12.240480ms，FPS59.368756/46.763425/46.838327/46.877795，>30计4/25/22/19。组均off11.443719→on12.162457ms（+6.28063%；commonchartTime同方向+6.27577%），但A2off也慢、off漂移+1.593522ms大于组差+.718738，因此不能把该负向均值当稳定因果回归，也绝不能凭局部改善称完整性能达成。

粒子Stream discard候选最新已构建与有限质量执行通过，尚未真实Main性能对照/不得宣称速度收益或默认推广。主项目local aa67e3f2090c8dd5dc7a1d3931dd364e5cf84152接候选，另b4c8fdd0961bec7d24205160f7db9229b0d957b8修统一入口；main未push/trackedclean。Mini4b26d71/Macrocd0ab01配套研究库已push各自参考实现，master不改；Root Cargo.lock仅3个source变化。10 std-only CPUtests、Windowsdefault/research3932、候选Android89391均actualterminal0；初SO8e7ec576257bc9d6ba269c91083f4e9f3ac741df0e33ed780a2fb11fae361ee6/50源/a67，quality-v1实际82161terminal1：第2on请求true而reportrequestedfalse，接受0，候选未执行，因Root把setter插到main_timing_session而非run入口，严格gate拒，原failed JSON/log/2raw/script均保留。不能把其图片或base FPS当候选证据。

SmallMain新B1ON已auditce8d83 actual0（summary4d56493fc3071d0c7e9fd7cb7950ce1d198e6f7077d339d3c7584b989f3d52f9），run packed-small-main-b1-on-20261011T190210-f987fc19/control19396actual0/quiet40.0001474s0call，1535Game+boundary/1492Playing25.001457s。Playing59.676523fps/app9.097520ms/loopP9919.325484/max27.592708/0>30，本有限Playing窗口达到用户30ms尾门；Starting另35.996875长帧，不混音轨。实际Small2536calls/38385instances/DERIVED2456640B非API计数，zero534Game/448Playing/over2560，固定Frozen full lifetime rawexitdraw57090/quads174789/covered38609/fallbackoldqpc0，GamePGRpubbytes0，其它PlayingPainter28pub1515520B。firstnonempty及后4帧loop17.036979/16.623959/16.895312/16.701042/16.551041，update各约.017–.033，jointupdate全窗max.418229ms。只单ON/不完整收益，不推广/全曲未知。
```

引用说明：上述原文中的哈希缩写已按原始证据补全；本文第 6 节已给出完整数值。原始记录里的“SO96d72793”是长哈希的前缀，完整值见第 11 节。

---

## N-05 UI 图片后台解码（loading_cpu::ui_image）

### 1. 一句话结论与判定

判定：**负优化**。把 21 张 UI 图片的 CPU 解码放到后台 worker 后，Loading 场景的“最大 update”确实下降了：Exo 47.573→36.964 ms、Petrichor 51.818→41.441 ms；但全窗口最大帧间隔没有改善，Exo 69.558→71.095 ms、Petrichor 71.985→72.035 ms；Petrichor 的 p99 从 18.202 升到 18.936 ms、p95 从 17.254 升到 17.500 ms；持续 FPS 只从 59.6726 升到 59.7040（+0.03）。目标指标改善、尾延迟和整体帧时间变差，因此判定为负优化。

关键数字：

- Loading 最大 update 中位数：Exo 47.573→36.964 ms（−10.609 ms）；P 51.818→41.441 ms（−10.377 ms）。
- 全窗口 max：Exo 69.558→71.095 ms（+1.537 ms）；P 71.985→72.035 ms（+0.050 ms）。
- P p99：18.202→18.936 ms（+0.734 ms）；P p95：17.254→17.500 ms（+0.246 ms）。
- 持续 FPS：Exo 59.7080→59.7162（+0.008）；P 59.6726→59.7040（+0.031）。
- 质量：8 个作业、6 组严格 RGBA、222 项 CPU 输出全部相同；inline/worker 两条路径都实际执行。
- `ordinary=false`：候选没有进入普通路径，也没有进 main。

这条与 N-03 的结构类似：一个被点名的阶段指标（Loading update）变好，但用户真正感知的完整帧/尾延迟没有变好，甚至更差。不同的是，N-05 的全局 max 和 P p99 明确变差，所以判定更硬。

### 2. 问题与初衷

Loading 场景的 `update` 里包含 21 张 UI 图片的同步加载：

- 文件/JNI 读取约 5.15/5.39 ms；
- `macroquad::texture::Image::from_file_with_format` 解码约 18.66/17.23 ms；
- 每张 `Texture2D` 上传 + mipmap 约 13.18/17.24 ms；
- 其余还有粒子、SFX/music/Judge、音频设备等。

这些工作全在主线程，Loading 的 update 最大到 47–52 ms。候选的初衷是：**只把 CPU 解码移到后台 worker**，文件读取、GL 上传、mipmap、绘制顺序都不变，从而把解码从主线程 critical path 拿掉，降低 Loading 的最大 update。

设计约束：

1. 必须使用 macroquad 原有解码器和版本，不能换库、不能改像素格式；
2. 只能把 `Send` 的 owned CPU `Image` 移到 worker，不能把 `Texture2D`、GL 状态、JNI 文件句柄移到 worker；
3. 每张图的上传仍必须在原顺序、原主线程上；
4. worker 不可用（wasm、无 tokio runtime）时回退 inline，结果必须相同；
5. 质量模式必须对 control/worker 都记录 21 张图的完整 RGBA hash；
6. 默认 false，只有研究请求显式开启。

### 3. 实现思路与具体机制

1. **读取**：`load_ui_texture` 先 `load_file(path)`，仍在原 JNI/asset 线程；`render_research` 构建下用 `research_spikes::span("ui_asset_read_poll")` 包住 poll，span 不跨 await。
2. **选择 worker**：`let worker = gameplay && loading_work::ui_images_enabled();`。只有 gameplay 场景、且 `async_ui_images=true` 时才允许 worker。
3. **调用**：`loading_cpu::ui_image(bytes, path, worker)`。`ui_image` 用 `run_selected_with_proof("ui_asset:{path}", allow_worker, work)`。
4. **worker 执行**：`run_selected_with_proof` 在 `allow_worker && asynchronous()` 且当前有 tokio runtime 时，`runtime.spawn_blocking(measured)`；返回的 future 用 `Waker::noop()` 每帧 poll。这样 macroquad 的外层 executor 不需要被后台线程唤醒，加载 future 每帧检查一次；worker 完成就返回。
5. **解码**：闭包里调用 `macroquad::texture::Image::from_file_with_format(&bytes, None)`，与原路径完全相同的解码器和默认格式。
6. **proof**：质量模式下计算 `width/height/rgba_sha256`，写入 `loading-work.json`；真实性能任务不 hash，避免把 hash 成本混进性能。
7. **上传**：`ui_image` 返回 owned `Image`；`load_ui_texture` 随后 `Texture2D::from_image(&image)`，仍在主线程、原顺序；mipmap 仍由原来的 `ui_mipmapped` 处理。
8. **回退**：没有 tokio runtime、wasm、worker=false、`asynchronous=false` 时，`run_selected_with_proof` 直接在当前线程执行 `measured()`，结果相同；worker panic 被转成 `CPU loading worker failed` 的有限错误，不会静默替换。
9. **顺序**：每张图仍然“读→解码→上传”串行；worker 只改变解码线程，不改变跨图顺序，也不并行上传。
10. **计数**：`loading-work.json` 记录每项的 `compute_ms`、`await_wall_ms`、`off_thread`、`ok`、`proof`；性能 cohort 用它确认 worker 真的 off-thread。

这个设计的关键限制是：**解码只是 Loading update 的一部分**。文件读取和纹理上传仍在主线程；worker 完成后，主线程仍要等它、拿结果、上传。因此“解码移走”不等于“Loading update 变短”，更不等于“全窗口帧时间变短”。

### 4. 文件、符号与完整核心代码

相关符号：

- `prpr/src/loading_cpu.rs`：`ui_image`。
- `prpr/src/loading_work.rs`：`research_ui_images_configure`、`ui_images_enabled`、`run_selected_with_proof`、`research_records`、`validate_outputs`。
- `prpr/src/core/resource.rs`：`load_ui_texture`、`load_image` 宏、`ui_mipmapped`。
- `phira/src/research.rs`：`Request::async_ui_images`、`Config`、`run` 中的 `research_ui_images_configure`。
- 研究请求字段：`async_ui_images`、`async_loading`、`loading_output_hashes`。

#### 4.1 `loading_cpu::ui_image` 完整函数

```rust
/// Use macroquad's original decoder/version, yielding only owned CPU pixels.
/// No texture, JNI file access, GL state or audio device enters this worker.
pub async fn ui_image(bytes: Vec<u8>, path: &'static str, allow_worker: bool) -> Result<macroquad::texture::Image> {
    let validate = crate::loading_work::validate_outputs();
    crate::loading_work::run_selected_with_proof(&format!("ui_asset:{path}"), allow_worker, move || {
        let image = macroquad::texture::Image::from_file_with_format(&bytes, None);
        let proof = validate.then(|| serde_json::json!({
            "width":image.width,"height":image.height,
            "rgba_sha256":format!("{:x}",Sha256::digest(&image.bytes))
        }));
        Ok((image, proof))
    }).await
}
```

这段就是全部：`bytes` 是已经读好的 owned `Vec<u8>`；闭包只做解码和可选 hash；不创建纹理、不访问 JNI、不碰 GL/audio。`allow_worker` 由调用方决定，`run_selected_with_proof` 再叠加 runtime/asynchronous 条件。

#### 4.2 `run_selected_with_proof` 完整函数

```rust
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

`Waker::noop()` 是核心：macroquad 的外层 executor 不能被后台线程唤醒，所以这里不依赖 wake，而是让加载 future 在每帧被 poll 时检查 worker 是否完成。`spawn_blocking` 返回 `JoinHandle`，poll 它；完成就拿到 `Result`。`off_thread` 用线程 id 判断，用于质量/性能审计。

#### 4.3 `load_ui_texture` 完整函数

```rust
async fn load_ui_texture(path: &'static str, gameplay: bool) -> Result<SafeTexture> {
    #[cfg(feature = "render_research")]
    let bytes = {
        use std::future::Future;
        let mut read = Box::pin(load_file(path));
        std::future::poll_fn(move |cx| {
            let _span = crate::research_spikes::span("ui_asset_read_poll");
            let _asset = crate::research_spikes::span(path);
            read.as_mut().poll(cx)
        }).await?
    };
    #[cfg(not(feature = "render_research"))]
    let bytes = load_file(path).await?;
    let worker = gameplay && crate::loading_work::ui_images_enabled();
    let image = if worker || (gameplay && crate::loading_work::validate_outputs()) {
        crate::loading_cpu::ui_image(bytes, path, worker).await?
    } else {
        #[cfg(feature = "render_research")]
        let _span = crate::research_spikes::span("ui_asset_decode");
        #[cfg(feature = "render_research")]
        let _asset = crate::research_spikes::span(path);
        Image::from_file_with_format(&bytes, None)
    };
    #[cfg(feature = "render_research")]
    let _span = crate::research_spikes::span("ui_asset_texture_upload");
    #[cfg(feature = "render_research")]
    let _asset = crate::research_spikes::span(path);
    Ok(Texture2D::from_image(&image).into())
}
```

关键点：

- 读取永远发生在调用 `ui_image` 之前，仍在原 JNI/asset 线程；worker 只拿 owned bytes。
- `worker || (gameplay && validate_outputs())`：质量模式即使 worker=false 也走 `ui_image`，以保证 control 和 worker 都记录同样的 21 项 hash。
- `Texture2D::from_image` 在 await 之后，仍在主线程；`ui_mipmapped` 由调用方继续处理。
- 每个 span 都在 await 的一侧，不跨 await。

#### 4.4 开关与配置

```rust
#[cfg(feature = "render_research")]
thread_local! {
    static UI_IMAGES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "render_research")]
pub fn research_ui_images_configure(enabled: bool) { UI_IMAGES.with(|it| it.set(enabled)); }

pub fn ui_images_enabled() -> bool {
    #[cfg(feature = "render_research")]
    { UI_IMAGES.with(|it| it.get()) }
    #[cfg(not(feature = "render_research"))]
    { false }
}
```

```rust
fn asynchronous() -> bool {
    #[cfg(feature = "render_research")]
    {
        SETTINGS.with(|it| it.get().0)
    }
    #[cfg(not(feature = "render_research"))]
    {
        true
    }
}
```

```rust
ensure!(!request.async_ui_images || request.async_loading,
    "async_ui_images requires the CPU loading worker policy");
```

```rust
prpr::loading_work::research_ui_images_configure(request.async_ui_images);
```

`async_ui_images` 不是独立生效：它要求 `async_loading=true`，后者设置 `SETTINGS.0`，`asynchronous()` 才返回 true。否则即使请求了 worker，`run_selected_with_proof` 也会回退 inline。这是为了防止“只开子开关、没开总开关”的误测。

### 5. 开关、默认值、构建特性

| 项目 | 值 | 说明 |
|---|---|---|
| 研究请求字段 | `async_ui_images` | 打开 UI 图片后台解码 |
| 前置字段 | `async_loading` | 必须同时 true；否则 `ensure!` 拒绝 |
| 质量字段 | `loading_output_hashes` | 记录 21 项 RGBA hash；性能任务关闭 |
| 默认值 | `async_ui_images=false` | 请求缺省即关闭 |
| 构建特性 | `render_research` | 研究后端总开关 |
| 平台 | 非 wasm + 有 tokio runtime | wasm/无 runtime 回退 inline |
| 场景 | 仅 gameplay | 菜单独立图标路径保持同步 |
| 生产默认 | 不启用 | `ordinary=false`，未进 main |
| 上传线程 | 主线程 | 每张 `Texture2D::from_image` 仍主线程 |
| 读取线程 | 原 JNI/asset 线程 | 不随 worker 改变 |

“默认关”是硬性的：`ui_images_enabled()` 在非 research 构建直接返回 false；生产整理明确把 UI 图片 worker 排除在主线之外。

### 6. 完整测量数据

设备：`8DF6R17309002674`，Mali-T880 / ES 3.2，2560×1440，Android 8。场景：真实 UI 应用加载，Exoplanetary 与 Petrichor 两个谱面；每轮统一 60 秒请求上限；顺序 control / worker / worker / control，两个谱面各两轮，共 8 个作业。窗口包含 Loading 与进入 Playing 的过渡；不是全曲。

#### 6.1 逐轮数据

| 谱面 | 路径 | FPS | max ms | p95 ms | p99 ms | Loading update max ms | Loading render max ms | >16.67 update | song→Playing ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Exo | control | 59.684 | 74.738 | 17.220 | 18.287 | 48.530 | 42.821 | 4 | 8841.49 |
| Exo | control | 59.732 | 64.378 | 17.258 | 18.132 | 46.616 | 40.161 | 4 | 9491.73 |
| Exo | worker | 59.695 | 69.729 | 17.259 | 18.281 | 40.545 | 46.790 | 3 | 9073.50 |
| Exo | worker | 59.737 | 72.460 | 17.345 | 18.067 | 33.383 | 45.255 | 3 | 9391.14 |
| P | control | 59.700 | 71.417 | 17.265 | 18.126 | 52.416 | 55.022 | 4 | 8805.93 |
| P | control | 59.645 | 72.554 | 17.242 | 18.279 | 51.219 | 57.699 | 4 | 8907.75 |
| P | worker | 59.696 | 68.896 | 17.806 | 19.549 | 41.986 | 54.583 | 3 | 9075.06 |
| P | worker | 59.712 | 75.174 | 17.194 | 18.324 | 40.896 | 50.393 | 3 | 9307.63 |

#### 6.2 两轮中位数

| 谱面 | 路径 | FPS | max ms | p95 ms | p99 ms | Loading update max ms | Loading render max ms | >16.67 update |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| Exo | control | 59.7080 | 69.558 | 17.239 | 18.209 | 47.573 | 41.491 | 4 |
| Exo | worker | 59.7162 | 71.095 | 17.302 | 18.174 | 36.964 | 46.022 | 3 |
| P | control | 59.6726 | 71.985 | 17.254 | 18.202 | 51.818 | 56.360 | 4 |
| P | worker | 59.7040 | 72.035 | 17.500 | 18.936 | 41.441 | 52.488 | 3 |

#### 6.3 变化

| 指标 | Exo | P |
|---|---:|---:|
| Loading update max | 47.573→36.964（−10.609 ms，−22.30%） | 51.818→41.441（−10.377 ms，−20.03%） |
| 全窗口 max | 69.558→71.095（+1.537 ms，+2.21%） | 71.985→72.035（+0.050 ms，+0.07%） |
| p95 | 17.239→17.302（+0.063 ms） | 17.254→17.500（+0.246 ms） |
| p99 | 18.209→18.174（−0.035 ms） | 18.202→18.936（+0.734 ms） |
| FPS | 59.7080→59.7162（+0.008） | 59.6726→59.7040（+0.031） |
| Loading render max | 41.491→46.022（+4.531 ms） | 56.360→52.488（−3.872 ms） |
| >16.67 update | 4→3 | 4→3 |
| song→Playing | 9166.61→9232.32（+65.71 ms） | 8856.84→9191.34（+334.50 ms） |

解读：

- Loading update max 的改善是真实的：两轮 worker 都低于两轮 control，Exo 和 P 各约 −10.5 ms。
- 但 Loading render max 在 Exo 反而 +4.53 ms；worker 把解码移走，却让渲染/上传阶段更长。P 的 render max 略降，但全窗口 max 仍略升。
- 全窗口 max 在 Exo 明显变差（+1.54 ms），P 基本持平；P 的 p95/p99 变差，说明不只是单个尖峰，尾部整体更差。
- song→Playing 延后 66–335 ms，说明 worker 的调度/等待把加载完成时间推后；这不是“免费”的。
- 持续 FPS 只变化 +0.008/+0.031，低于 0.05；不能称帧率提升。

### 7. 质量与回归结果

| 项目 | 结果 |
|---|---|
| 质量作业 | 8 |
| UI 图片 | 21 张 |
| 严格 RGBA 对照 | 6 组 |
| CPU 输出比较 | 222 项 |
| 全部相同 | 是 |
| inline 路径 | 实际执行 |
| worker 路径 | 实际执行 |
| 最终 gameplay RGBA | 相同 |
| 中间 UI/Ending 帧 | 未覆盖 |
| worker 错误 | 有限错误，不静默替换 |
| 无 runtime | inline 回退，结果相同 |
| wasm | inline 回退 |

质量覆盖：

- 21 张 gameplay UI 图片的完整 RGBA hash；
- 16 项旧的 chart/image/PCM/typed-model 输出；
- 每场景 3 次比较，共 222 项；
- 6 组严格最终 RGBA；8 个作业都实际走 inline/worker 选择。

边界：

- 只覆盖两个谱面、一个设备、有限 UI 场景；不覆盖所有菜单/结算/Ending 帧。
- 不覆盖自定义 UI 资源、不同 DPI、不同 Android 版本。
- 不覆盖 worker 在低内存/被杀/线程池饱和时的行为；只有 panic/错误路径。
- 质量相同不等于“所有中间帧相同”；只保证最终 gameplay RGBA 和列出的 CPU 输出。

### 8. 为什么判定为负（逐项对比）

| 比较项 | control | worker | 判定 |
|---|---:|---:|---|
| Loading update max（Exo） | 47.573 ms | 36.964 ms | 改善 |
| Loading update max（P） | 51.818 ms | 41.441 ms | 改善 |
| 全窗口 max（Exo） | 69.558 ms | 71.095 ms | 变差 |
| 全窗口 max（P） | 71.985 ms | 72.035 ms | 略差 |
| p95（Exo/P） | 17.239 / 17.254 | 17.302 / 17.500 | 变差 |
| p99（Exo/P） | 18.209 / 18.202 | 18.174 / 18.936 | 一项略好、一项变差 |
| FPS（Exo/P） | 59.708 / 59.673 | 59.716 / 59.704 | 基本不变 |
| song→Playing | 9166.6 / 8856.8 | 9232.3 / 9191.3 | 延后 |
| 质量 | 基准 | 相同 | 通过 |
| ordinary | false | false | 未接入 |

逐项说明：

1. **目标指标**：Loading update max 下降 20–22%，这是候选唯一明确的收益。它说明“解码确实从 update 主线程移走了”。
2. **全窗口 max**：Exo 从 69.56 升到 71.10 ms，P 从 71.99 升到 72.04 ms。全窗口 max 是用户感知的卡顿边界；它没有改善，Exo 还更差。
3. **p95/p99**：P 的 p95 +0.246 ms、p99 +0.734 ms，说明尾部整体变差；Exo 的 p99 略好 0.035 ms，但 p95 +0.063 ms。两项谱面方向不一致，不能称尾延迟改善。
4. **FPS**：+0.008/+0.031，低于测量噪声；不能称帧率提升。
5. **加载完成时间**：song→Playing 延后 66–335 ms。worker 让 update 的“最大单帧”变短，但总加载时间变长；这说明解码被移走后在等待/调度/上传阶段又花回来了。
6. **机制**：解码只是三段里的一段。文件读取仍主线程，纹理上传仍主线程；worker 完成后主线程要等 join、拿 Image、上传、mipmap。Loading update 的 max 下降，可能是因为“解码不再卡在 update 里”，但 render/上传阶段的尖峰变大（Exo render max +4.53 ms），全窗口没有收益。
7. **结论**：这是一个“阶段指标改善、端到端变差”的负优化。它不能进生产，也不应该用 Loading update 的下降来宣称加载优化成立。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：研究提交 `3670c2a`；`loading_cpu::ui_image`、`run_selected_with_proof`、`load_ui_texture` 完整接入 |
| 默认 | 关闭；`async_ui_images=false`；生产 `ui_images_enabled()` 返回 false |
| 普通路径 | 未接入；生产整理明确只保留几何流批处理/连续目标/MSAA/保守 bbox/CPU+JSON 后台加载；UI 图片 worker 不在主线。`ordinary=false` |
| 主线是否已实施 | 否（main） |
| 研究后端 | UI 图片研究批次见第 11 节；`production-clean.so` 只覆盖生产文件，不含该候选 |
| 质量/契约 | worker、runtime 选择、inline 回退、质量 proof、真实 UI 8 作业 ABBA 完成；见第 7 节 |
| 是否“没实现” | 不是。worker、runtime 选择、inline 回退、质量 proof、真实 UI 8 作业 ABBA 都完成 |
| 后续 | 没有继续推广；保留为“加载阶段后台加载/解码”的研究证据 |

### 10. 可挽救的部分与后续条件

可挽救：

1. **解码 worker 的语义正确**：222 项输出全同，inline/worker 两条路径都实际执行；这说明“owned CPU Image 可以安全跨线程”成立。
2. **Loading update max 下降 10 ms**：如果目标是“减少 Loading 单帧最长 update”，这个方向有效；可以继续拆分为“读取/解码/上传”三段，分别决定哪些能后台化。
3. **`run_selected_with_proof` 的 noop-waker 轮询**可以复用到其他“后台计算 + 主线程消费”的加载任务；它不依赖 macroquad executor 的唤醒。
4. **质量 proof 的 222 项比较**是一个可复用的加载等价性基线。

后续条件：

- 必须把纹理上传/mipmap 也移出主线程，或至少与解码重叠；当前上传仍主线程，worker 的收益被上传/等待抵消。
- 必须减少 worker 的调度/等待延迟：song→Playing 延后 66–335 ms，说明“后台解码”不是免费的；可以用更早启动、预取、批量提交来抵消。
- 必须证明全窗口 max/p95/p99 不变差；当前 Exo max +1.54 ms、P p99 +0.734 ms，是明确反证。
- 必须覆盖所有 UI/Ending/菜单帧、不同 DPI、不同 Android 版本；当前只有两个谱面。
- 如果保留，应作为“Loading update 尖峰”候选，而不是“加载总时长”或“帧率”优化；并且必须默认关闭，直到端到端数据转正。

### 11. 来源与可追溯性

- 参考实现：研究提交 `3670c2a`（主项目未 push）。
- 源码：`prpr/src/loading_cpu.rs`、`prpr/src/loading_work.rs`、`prpr/src/core/resource.rs`、`phira/src/research.rs`。
- 性能证据：`ui-images-loading-live60-confirmed/latency-summary.json`、`matrix.json`、`control-*/`、`worker-*/`；汇总日志 `ui-images-live60-latency-summary.log`。
- 质量证据：`ui-images-quality-confirmed/matrix.json`、`loading-output-equivalence.json`、`control-*/`、`worker-*/`。
- 早期失败批次：`ui-images-loading-confirmed`（第 7 项日志丢失）与 30 秒批次；整批排除，不与 60 秒批次混池。
- 设备：`8DF6R17309002674`；Mali-T880 / ES 3.2；2560×1440。
- 生产整理结论：UI 图片 worker 未进主线；`production-integration/integration.json` 与 `production-clean.so` 只覆盖生产文件，不含该候选。

以下为 `本地证据/production-integration/local-AGENTS.md` 中 UI 图片候选相关段落的完整原文：

```text
UI图片候选最终8项统一60秒真实UI ABBA完成，另独立8项质量/6项严格RGBA/222项全CPU输出全同。Loading最大update中位数Exo47.573→36.964ms、P51.818→41.441ms，但全窗口max Exo69.558→71.095ms、P71.985→72.035ms无可靠收益；P p99 18.202→18.936ms、p95 17.254→17.500ms变差，持续FPS变化很小，ordinary仍false，不接入。全局转移保留、本批新增1项ADB预送达重试保留；60秒批次不混两个被排除的30秒批次。字库宽边界8/16/32探针12项RGBA全同却仍18次flush/4次跳过，completed140.135→138.554/140.887/136.474ms无可靠收益；按查询精度与坐标计算的下一独立边界探针正在运行，带覆盖旧字形的反例，尚无生产字库优化。完整目标/结算尖峰/互动/严格snapshot回退/其他GPU仍未完成。

UI图片独立质量已完成8项/6项严格最终RGBA/222项完整CPU输出全同（21 UI图片RGBA+16旧图像/PCM/typed-model输出，每场景3次比较），且inline/worker路径实际验证。性能两批分别在第7项日志丢失及第4项daemon连接延迟失败，均整批排除，ordinary仍false。连续PID日志保留完整scene证据，但ADB服务原PID和5037监听仍在，不能称daemon死亡/根因修复；下一完整ABBA使用统一60秒请求与基于scene接收时间的等待，记录transport事件和导航延迟，不混旧30秒批次。

UI图片候选已实现且ordinary默认false：仅Resource::new的21张UI图使用原macroquad Image解码器及版本，owned CPU Image可自动Send；文件/JNI读取和每张Texture2D上传/mipmap仍主线程原序，菜单独立图标路径保持同步。worker可用性探测沿用运行时/wasm同算法inline，无新增unsafe Send或硬件扩展。质量开关对control inline/worker都记录21项完整RGBA hash，真实性能任务不hash；新的选择契约、Android构建与8项真实UI加载ABBA/独立8项质量尚待完成，不提前接入普通路径。

资源阶段后端loading-resource-stages已release（SHA ffad8d71fc2d8be2b8996625de30a2d4c7c7efdef4e87d46b292f3f515a0c2f4）、普通Android check通过，2项真实UI observer全部scope/native契约有效。剩余资源update40.764/47.059ms中21张UI图读取5.154/5.394ms、原macroquad解码18.664/17.230ms、纹理上传13.179/17.235ms；粒子2.548/4.679ms，SFX/music/Judge均很短，另帧音频设备26.406/20.220ms。CPU/GL墙钟分开且不冒充GPU时间。下一候选为原UI解码器后台处理、原GL线程上传，默认false，需独立性能和输出/像素验收；字库/结算、snapshot回退和全目标仍未完成。
```

引用说明：`local-AGENTS.md` 是 `本地证据/production-integration` 目录下的项目记录文件；它不是任务说明，而是历史状态的原始记录。本文第 6 节把它的中位数拆成 8 轮逐轮数据，并补充了 song→Playing 的变化，避免只引用结论。
## 第二部分：N-06 至 N-11

本文是 Phira-Pro「非正优化」审计的第二部分，覆盖 N-06 至 N-11 六个条目。六个条目都曾经以“减少主线程工作”“减少上传”“避免重新分配”“提供能力回退”“把音符搬到 GPU”为目标实现过研究版本，并且都完成了真机测量、像素对照或两者兼有。本文只回答一个问题：这些实现是否值得进入普通路径。

## 判定口径与阅读方式

- **无优化**：实现机制成立、质量通过，但没有可重复的整帧收益。局部阶段变快不计为整帧优化；FPS 变化落在对照自身漂移以内也不计。
- **负优化**：整帧或关键阶段可重复变慢，或者严格像素验收失败，导致该路径不能作为生产回退。
- 本文的“通过”只表示对应实验的验收项通过，不表示完整游戏、全曲、全部 GPU、全部字体或全部分辨率通过。
- 所有时间都是应用线程 `update + render + flush` 的墙钟，或其中某个阶段的墙钟。它们包含驱动提交、等待、调度与抢占，不等于 GPU 独占执行时间。ABBA 表中的数值是“每轮分位数的中位数”，不是把全部帧合并后的分位数；重复轮次的范围不是置信区间。
- 像素结论只覆盖被截图的那个时刻、那个分辨率、那套字体与材质。末帧 RGBA 相同不自动外推为全曲无损。
- 本文不引用外部链接。所有溯源都指向本地 commit、参考实现快照、源码路径、后端 SHA 与 `本地证据/` 目录。

## 设备与数据边界

| 条目 | 设备 | GPU / 上下文 | 分辨率 | 数据来源 |
|---|---|---|---|---|
| N-06 | MT6769V/CU，Android 10 | Mali-G52 MC2 | 1920×1200 | `docs/development/game-performance-target.md`、`本地证据/branch-cleanup-20261006/previous-AGENTS.md` |
| N-07 | MT6769V/CU，Android 10 | Mali-G52 MC2 | 1920×1200 | `本地证据/glyph-scoped-loading-matrix/latency-summary.json` |
| N-08 | EDI-AL10，Android 8 | Mali-T880，OpenGL ES 3.2 | 2560×1440 | `本地证据/mask-*`、`docs/development/rendering-research.md` |
| N-09 | EDI-AL10，Android 8 | Mali-T880，OpenGL ES 3.2 | 2560×1440 | `本地证据/mask-texture-ring-matrix`、`mask-viewport-cache-matrix`、`blit-fallback-screen` |
| N-10 | EDI-AL10，Android 8 | Mali-T880，OpenGL ES 3.2 | 1920×1200（该组诊断） | `本地证据/snapshot-full-copy-diagnostic` |
| N-11 | EDI-AL10，Android 8 | Mali-T880，OpenGL ES 3.2 | 2560×1440 | `本地证据/low-quality-normal/README.md`、`本地证据/AGENTS.md` |

不同条目的设备、分辨率、画质与对照 cohort 不同，跨条目数值不能相加，也不能互相换算成“总收益”。

| 条目 | 机制 | 判定 | 一句话依据 |
|---|---|---|---|
| N-06 | 加载/模型分帧 | 无 | 最大间隔 64.963→67.352 / 67.389→67.063ms，无尖峰改善 |
| N-07 | glyph scoped flush | 负 | Exo 最大间隔 65.111→69.103ms，两首持续 FPS 略降 |
| N-08 | mask 查表 + 逐行 bbox | 无 | 局部 mask −0.85ms，整帧 p95 仅 −0.01ms |
| N-09 | mask ring / viewport / resolved pass | 无 | ABBA/像素通过，但无整帧收益，全部默认关 |
| N-10 | snapshot blit / shader 回退 | 负 | 严格像素 254,520 通道不同、最大差 71 |
| N-11 | GPU 常驻音符 IR v1/v2/v3 | 负 | v1 3.32→3.50–4.14ms；v3 6.25 vs 6.37 不稳定 |

## N-06 加载/模型分帧（loading_schedule / frame budget）

### 1. 一句话结论与判定（无 + 理由）

**判定：无优化。** `FrameBudget` 只在 PGR 判定线转换的少量边界上让本地 loading future 挂起一次，让加载场景获得一次渲染机会；它没有拆分真正的加载大头（资源读取/解码、音频设备创建、block 准备、字形纹理更新），也没有把单次挂起对齐到 16.67ms 的帧预算。最终 8 项真实 UI 加载 ABBA 中，Exoplanetary 的最大帧间隔中位数从 64.963ms 变为 67.352ms（变差 2.389ms），Petrichor 从 67.389ms 变为 67.063ms（仅好 0.326ms），两者都不构成尖峰改善；持续 FPS 还分别从 59.545 降到 59.523、从 59.379 升到 59.507，方向不一致。

### 2. 问题与初衷

加载场景的 `update` 曾经出现 150ms 级长帧。已细分的构成包括：PGR JSON 解码与模型转换约 95ms、block 效果准备约 22.9ms、音频设备创建 22–39ms、UI 资源读取/解码/上传合计约 40ms、字形纹理更新数十毫秒。加载场景本身是一个被逐帧轮询的本地 future；如果一次 poll 内连续完成所有判定线转换，主循环就没有机会在中间渲染，表现为一个长间隔。

初衷不是“把加载变快”，而是“把同一段加载工作切开，让主循环在切片之间渲染”。因此它属于尖峰整形，而不是吞吐优化。这个目标要求两件事同时成立：

1. 切片要落在真正耗时的段落之间；
2. 切片要让出足够长的时间给渲染，而不是只让出一次立即被唤醒的 poll。

`loading_schedule.rs` 只实现了第 2 点的最小版本：一次挂起、立即 `wake_by_ref`，下一帧再继续。它没有根据剩余帧预算调整切片大小，也没有把资源工作纳入调度。

### 3. 实现思路与具体机制

`FrameBudget` 只有两个字段：`enabled` 与 `began`。`checkpoint(boundary)` 的行为是：

- `enabled == false`：立即返回，完全同步；
- `boundary == false` 且距上次重置不足 2ms：立即返回，不挂起；
- 否则进入 `poll_fn`：第一次 poll 把 `suspended` 置真、调用 `waker.wake_by_ref()`、返回 `Poll::Pending`；第二次 poll 返回 `Poll::Ready`，随后把 `began` 重置为当前时刻。

也就是说，每个 checkpoint 最多让出一次，而且唤醒是立即的；真正让加载场景渲染的是外层加载 future 在两次 poll 之间返回控制权，而不是这个 waker 制造了等待。`boundary=true` 的调用点被放在跨线处理与 `finish_phigros` 前后，用来保证这些不可分割的段落不会被 2ms 阈值跳过。

接入点只有 `parse/pgr.rs` 的 `convert_phigros_loading`：

- 先算 `phigros_max_time`，然后 `checkpoint(false)`；
- 逐条判定线调用 `parse_judge_line`，每处理完一条调用 `checkpoint(false)`；
- 跨线 hint 处理与 block/shader 准备之前调用 `checkpoint(true)`；
- `finish_phigros` 之后调用 `checkpoint(true)`。

这意味着实际可挂起的粒度是“一条判定线”。该谱面只有 24 条判定线，所以一次加载最多只有约 26 个 checkpoint；而 `finish_phigros` 内部的 `process_lines`、`Chart::new`、`parse_block_areas` 仍然是一整块，`parse_judge_line` 内部的单条线解析也没有再切。对于 95ms 的 PGR 解析，真正的长段是 JSON 解码与模型构造，不是 24 次循环本身；分帧只能把 24 条线之间的空隙让出来，无法把单条线内部或 `finish_phigros` 切开。

还有一个容易误读的点：源码里写的是 `FrameBudget::new(true)`，看起来“默认开启”。但 `convert_phigros_loading` 只有在 `loading_work::chart_slices_enabled()` 为真时才会被调用。`CHART_SLICES` 是 `thread_local Cell<bool>`，初值 `false`，只能通过 `research_chart_slices_configure(true)` 打开；非 `render_research` 构建下 `chart_slices_enabled()` 直接返回 `false`。所以普通构建的默认行为仍是原来的 `convert_phigros`，`FrameBudget::new(true)` 只是“研究请求打开后的内部常量”，不是生产默认值。

### 4. 文件、符号与完整核心代码

源码快照：参考实现，commit `4f046544`（research: 保存渲染优化与真机验证，同步上游更新）。文件与符号：

- `prpr/src/loading_schedule.rs`：`FrameBudget`、`FrameBudget::new`、`FrameBudget::checkpoint`，以及两个契约测试；
- `prpr/src/parse/pgr.rs`：`convert_phigros_loading`、`parse_phigros_loading`、`chart_slices_enabled` 调用；
- `prpr/src/loading_work.rs`：`CHART_SLICES`、`research_chart_slices_configure`、`chart_slices_enabled`；
- `prpr/src/lib.rs`：`mod loading_schedule;`。

下面是 `loading_schedule.rs` 的完整内容（含测试，未省略）：

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

调用点与开关（`prpr/src/parse/pgr.rs` 第 391–492 行，按该 commit 的原文）：

```rust
    convert_phigros(pgr, extra)
}

/// Only the owned serde model crosses threads. Chart/GL/font/extra resources
/// stay on the render thread; there is no unsafe Send wrapper for this path.
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

`prpr/src/loading_work.rs` 中的开关与默认值：

```rust
    static STARTED: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    static CHART_JSON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CHART_SLICES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static UI_IMAGES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "render_research")]
pub fn research_configure(asynchronous: bool, validate: bool) {
    SETTINGS.with(|it| it.set((asynchronous, validate)));
    RECORDS.with(|it| it.borrow_mut().clear());
    STARTED.with(|it| it.set(Some(Instant::now())));
}

#[cfg(feature = "render_research")]
pub fn research_records() -> serde_json::Value {
    RECORDS.with(|it| serde_json::json!(*it.borrow()))
}

#[cfg(feature = "render_research")]
pub fn research_chart_json_configure(enabled: bool) { CHART_JSON.with(|it| it.set(enabled)); }

#[cfg(feature = "render_research")]
pub fn research_chart_slices_configure(enabled: bool) { CHART_SLICES.with(|it| it.set(enabled)); }

#[cfg(feature = "render_research")]
pub fn research_ui_images_configure(enabled: bool) { UI_IMAGES.with(|it| it.set(enabled)); }

pub fn ui_images_enabled() -> bool {
    #[cfg(feature = "render_research")]
    { UI_IMAGES.with(|it| it.get()) }
    #[cfg(not(feature = "render_research"))]
    { false }
}

pub fn chart_slices_enabled() -> bool {
    #[cfg(feature = "render_research")]
    { CHART_SLICES.with(|it| it.get()) }
    #[cfg(not(feature = "render_research"))]
    { false }
}

pub fn chart_json_enabled() -> bool {
    #[cfg(feature = "render_research")]
    { CHART_JSON.with(|it| it.get()) }
```

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| 运行时开关 | `research_chart_slices_configure(bool)` | 只写 `CHART_SLICES`；无 Config/UI 入口 |
| 默认值 | `false` | `CHART_SLICES` 的 `Cell::new(false)` |
| 非研究构建 | `chart_slices_enabled()` 返回 `false` | 普通构建走原 `convert_phigros` |
| 构建特性 | `render_research` | 只有该 feature 下才编译研究请求与开关 |
| 调度阈值 | 2ms（非 boundary） | 与 16.67ms 帧预算无关 |
| 最大挂起 | 每个 checkpoint 一次 | `suspended` 只允许一次 `Pending` |
| 覆盖范围 | PGR 判定线转换 | 不覆盖资源、音频、block、字形 |
| 生产默认 | 未启用 | `ordinary` 仍为 false |

### 6. 完整测量数据（表格）

数据来自 8 项真实 UI 加载 ABBA（两首歌各 4 轮）与 8 项质量对照。下表是每轮分位数的中位数，两轮一组，不是把 30 秒内所有帧合并后的分位数：

| 场景 / 指标 | 对照（原模型转换） | 分帧（FrameBudget） | 差值 | 判定 |
|---|---:|---:|---:|---|
| Exoplanetary 全 30 秒最大帧间隔中位数 | 64.963ms | 67.352ms | +2.389ms | 变差 |
| Petrichor 全 30 秒最大帧间隔中位数 | 67.389ms | 67.063ms | −0.326ms | 噪声级 |
| Exoplanetary p99 | 19.133ms | 18.366ms | −0.767ms | 略好，但不是目标 |
| Petrichor p99 | 19.046ms | 18.647ms | −0.399ms | 略好，但不是目标 |
| Exoplanetary 持续 FPS | 59.545 | 59.523 | −0.022 | 略降 |
| Petrichor 持续 FPS | 59.379 | 59.507 | +0.128 | 略升，方向不一致 |
| Exoplanetary Loading 最大 update | 46.755ms | 46.925ms | +0.170ms | 略差 |
| Petrichor Loading 最大 update | 54.627ms | 52.042ms | −2.585ms | 局部好，但最大间隔未改善 |

质量与契约：

| 项目 | 数量 | 结果 |
|---|---:|---|
| 严格末帧 RGBA 对照 | 6 项 | 全部相同 |
| 图像 / 全 PCM / typed-model 输出 proof | 96 项 | 全部相同 |
| 挂起 / 恢复 / 取消契约 | 2 项 | 通过 |
| host 契约 | 44 项 | 通过 |
| Android 研究构建 / 普通 check | 各 1 次 | 通过 |
| 研究后端 SHA-256 | `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5` | `chart-slices-ready.so` |

ABBA 的轮次结构是 Exoplanetary 控制 / 分帧 / 分帧 / 控制，Petrichor 同样；表中的“对照”与“分帧”各取两轮中位数。对照自身在两个 30 秒窗口的最大间隔也有数毫秒级漂移，因此 0.326ms 的 Petrichor 改善不能单独作为收益证据。

### 7. 质量与回归结果

- 6 项严格末帧 RGBA 全部相同；96 项图像、全 PCM、typed-model 输出 proof 全部相同。说明分帧没有改变解析结果、Chart 语义、音频数据或模型序列化结果。
- 2 项挂起/恢复/取消契约通过：`enabled_boundary_preserves_work_across_a_render_opportunity` 验证两次 boundary checkpoint 后工作仍能完成；`disabled_schedule_is_synchronous_and_cancellation_does_not_finish_work` 验证禁用时同步返回、取消时未完成工作不会被执行。
- 44 项 host 契约、普通 Android check 与研究 release 构建通过。研究后端 SHA 已记录，源文件 hash 与 commit 对应。
- 质量通过只能证明“没有破坏”，不能证明“值得启用”。本条的失败点在性能与覆盖面，不在画质。

### 8. 为什么判定为无（逐项对比）

| 判据 | 期望 | 实际 | 结论 |
|---|---|---|---|
| 最大帧间隔下降 | 明显低于 64.963 / 67.389ms | 67.352 / 67.063ms | 未达成；Exo 变差 |
| p99 改善 | 稳定下降 | 各降 0.4–0.8ms | 有方向，但最大间隔是主目标 |
| 持续 FPS | 不降或略升 | 一降一升 | 无稳定收益 |
| Loading 最大 update | 明显下降 | 一升一降 | 无稳定收益 |
| 切片覆盖 | 覆盖主要长段 | 只覆盖 24 条线之间 | 覆盖不足 |
| 帧预算对齐 | 按剩余帧时间切片 | 固定 2ms 阈值 | 未对齐 |
| 质量 | 严格相同 | 相同 | 通过，但不构成收益 |

进一步解释：

1. **24 条线的循环不是主成本。** 一条判定线的 `parse_judge_line` 是同步的，`FrameBudget` 只能在两条线之间让出；对 95ms 的 PGR 解析，真正的长段在 JSON 解码（已由 `loading_work` 放到 worker）和 `finish_phigros` 的跨线处理/Chart 构造。后者被 `checkpoint(true)` 包住，但仍然是一整块，不能切。
2. **2ms 阈值太短。** 非 boundary checkpoint 在 2ms 内直接返回；即使挂起，唤醒是立即的，加载 future 下一次 poll 可能马上继续。它没有“让出到下一帧”的语义，只是让出一次 poll。对于 16.67ms 的帧预算，应该按剩余时间切片，而不是固定 2ms。
3. **挂起一次不等于渲染一帧。** 外层加载场景是否真正渲染，取决于它对 local future 的 poll 节奏与场景状态；`wake_by_ref` 只保证 future 被再次调度，不保证中间插入了一次完整的 `render + flush`。
4. **最大间隔由非模型段支配。** 同一轮加载里还有资源读取/解码/上传、音频设备创建、block 准备、字形更新。分帧只改模型转换，其他段不变，所以最大间隔仍由它们决定。Exo 的 +2.389ms 可以解释为一次 30 秒窗口内的调度抖动，但它足以说明该改动没有把最大间隔压下去。
5. **额外 poll 有成本。** 每个 checkpoint 都构造 `poll_fn`、读写 `Instant`、可能触发一次 wake；24 条线加上边界，成本很小，但方向是增加而不是减少。它需要有明确的整帧收益才能抵消。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：commit `4f046544`（研究快照） |
| 默认 | 关闭；`CHART_SLICES=false` |
| 普通路径 | 未接入；`ordinary=false` |
| 主线是否已实施 | 否（main）；未进入；参考实现未推送主项目 |
| 研究后端 | `chart-slices-ready.so`，SHA `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5` |
| 质量/契约 | 6 RGBA、96 输出 proof、2 调度契约、44 host、Android check/release 通过 |
| 后续 | 没有生产计划；仅作为“切片位置选错”的反例保留 |

### 10. 可挽救的部分与后续条件

可挽救的不是当前实现，而是“分帧”这个方向。若未来重新做，需要满足以下条件：

1. **先证明切片对象是主成本。** 用同步嵌套计时分别测 `phigros_json_decode`、`parse_judge_line` 总和、`finish_phigros`、`block_effects_prepare`、`audio device`、`glyph_texture_update`。只有某一项单独超过 16.67ms 且内部可切，才值得切。
2. **把 `finish_phigros` 内部切开。** `process_lines`、跨线 hint、`Chart::new`、`parse_block_areas` 应分别成为可挂起边界；否则 24 条线之间的空隙不足以覆盖长段。
3. **按帧预算切片，而不是 2ms。** checkpoint 应读取当前帧已用时间与目标帧时间，决定是否让出到下一帧；让出后应保证加载场景完成一次 `update + render + flush`，而不是立即被唤醒。
4. **把资源与音频纳入同一调度器。** 资源读取/解码/上传、音频设备创建、block 准备都在同一加载 future 内；只切模型转换不会改变最大间隔。
5. **保留取消语义。** 当前测试已经覆盖“取消不完成工作”；扩展后仍需保证取消时不会留下半构造的 Chart/GL 资源。
6. **以最大间隔和 p99 共同验收。** 只报 p99 或只报均值都不够；本条的主要失败就是最大间隔没有下降。

在这些条件满足之前，`FrameBudget` 只应作为研究代码保留；它的存在价值是证明“在 24 条判定线之间让出一次”不足以解决加载尖峰。

### 11. 来源与可追溯性（仅溯源）

- 源码：`prpr/src/loading_schedule.rs`，commit `4f046544`。
- 调用点：`prpr/src/parse/pgr.rs` 的 `parse_phigros_loading`、`convert_phigros_loading`。
- 开关：`prpr/src/loading_work.rs` 的 `CHART_SLICES`、`research_chart_slices_configure`、`chart_slices_enabled`。
- 模块声明：`prpr/src/lib.rs` 的 `mod loading_schedule;`。
- 后端：`本地证据/backends/chart-slices-ready.so.json`，SHA `9f8319c8df03db99b9f4582e974323f753bd35f20634215fc46a8519631fcdc5`。
- 数据目录：`本地证据/chart-slices-loading-matrix`、`本地证据/chart-slices-loading-confirmed`、`本地证据/chart-slices-quality-confirmed`。
- 文档：`docs/development/game-performance-target.md` 第 33 行；`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 33 行。
- 原始段落（`本地证据/AGENTS.md` 历史快照，归档于 `本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 33 行）：

> 模型分帧最终8项加载ABBA/8项质量完成：6项严格RGBA及96项图像/全PCM/typed-model输出全部相同。Exo控制/分帧全30秒最大间隔中位数64.963/67.352ms、p99 19.133/18.366ms、持续59.545/59.523fps；Petrichor67.389/67.063ms、p99 19.046/18.647ms、59.379/59.507fps。Loading最大update中位数Exo46.755/46.925ms、P54.627/52.042ms，无可靠最大尖峰改善，ordinary仍false，不将较好p99或共享算法当完整目标完成。下一步用同ready so的独立30秒stage观察分解剩余Loading同步调用；observer数据不混入性能ABBA。

## N-07 glyph scoped flush

### 1. 一句话结论与判定（负 + 理由）

**判定：负优化。** `glyph_scoped_flush` 用 `queued_texture_dependency` 判断“队列里是否已有绘制读取当前字形图集”，在没有读取时跳过 `TextPainter::submit` 更新图集前的 `flush`。想法是减少一次全队列提交，但实际效果是把等待从 flush 移到了 `update_texture_part`：驱动仍要在写入前处理已在途的读取，甚至可能因为读写冲突而更慢。8 项真实 UI 加载 ABBA 中，Exoplanetary 最大帧间隔中位数从 65.111ms 变为 69.103ms（变差 3.992ms），Petrichor 从 66.581ms 变为 65.421ms（仅好 1.160ms）；两者持续 FPS 都略降（59.5109→59.4618、59.4991→59.4593）。12 项质量/9 组严格 RGBA 全同只证明像素没坏，不能抵消延迟回归。

### 2. 问题与初衷

`TextPainter::submit` 在 `GlyphBrush::process_queued` 的回调里更新字形图集纹理。更新前必须保证此前录制的绘制命令已经提交，否则会出现“正在读这张纹理的 draw call 还没发，CPU 已经开始写同一张纹理”的读写冲突。原实现的做法是：只要本次 submit 还没 flush 过，就先 `get_internal_gl().flush()`，把整个 macroquad 队列提交掉，再更新纹理。

这个 flush 的代价在 Ending 段被量化过：114 次字形纹理更新累计 217.206ms，22 次更新前 flush 累计 3.846ms；单次 flush 不是最大头，但每个文字段的第一次更新会出现约 10–14ms 的等待，剩余小更新较便宜。于是产生一个假设：如果队列里没有任何命令读取当前图集，这次 flush 就是多余的；只要用一个保守查询证明“没有依赖”，就可以跳过 flush，让更新更早发生。

问题在于：**没有队列依赖，不等于 GPU 没有在途读取。** `flush` 提交的是 CPU 侧命令队列；提交之后 GPU 可能仍在执行上一帧的采样。跳过 flush 只是让 CPU 更早调用 `update_texture_part`，驱动仍必须保证写入不会与在途采样竞争。这个等待不会消失，只会换一个位置出现。

### 3. 实现思路与具体机制

实现分两层：

1. **macroquad 侧只读查询。** 独立库参考实现，commit `27cc446777d70c624ec8d25d0f0757fc7c8b19aa`，在 `quad_gl.rs` 增加 `queued_texture_dependency(texture)`。它扫描当前已录制的 `draw_calls[..draw_calls_count]`，满足任一条件就返回 `true`：
   - draw 的主纹理就是目标纹理；
   - draw 的 pipeline 句柄 `pipeline.0 >= 4`，即不是内置 pipeline，视为可能有额外图像依赖；
   - draw 是 capture（屏幕快照类命令）。
   注释明确说它“不等待已经提交的 GPU 工作”，纹理所有者仍必须考虑队列之外的 raw GL 读取，并在删除/缩放纹理前 flush；不做 UV、过滤、精度假设。
2. **prpr 侧开关。** `text.rs` 顶部在 `render_glyph_uploads` feature 下定义 `thread_local GLYPH_SCOPED_FLUSH: Cell<bool>`，初值 `false`；`research_glyph_scoped_flush(bool)` 只在 `render_research` 下暴露，内部写到这个 Cell。`submit` 的回调里：

```text
needs_flush = !GLYPH_SCOPED_FLUSH.get()
           || quad_gl.queued_texture_dependency(cache_texture)
```

   只有开关打开且查询明确返回“无依赖”时才跳过 flush；否则保持原 flush。非 `render_glyph_uploads` 构建下 `needs_flush` 恒为 `true`。未知材质、capture、任何已排队图集读取都会保守回退，因此它不是“全局删除 flush”。

这套逻辑的边界很窄：它只覆盖 macroquad 自己录制的 draw call，不覆盖 raw GL、不覆盖已经提交但未完成的 GPU 采样、不覆盖其他线程/其他上下文。它也没有插入 fence 或 `glClientWaitSync`；跳过的是一次 CPU 提交，而不是一次同步。

### 4. 文件、符号与完整核心代码

源码快照：参考实现，commit `4f046544`；macroquad 参考实现，commit `27cc446`。文件与符号：

- `prpr/src/ui/text.rs`：`GLYPH_SCOPED_FLUSH`、`research_glyph_scoped_flush`、`TextPainter::submit` 的 `process_queued` 回调；
- macroquad `src/quad_gl.rs`：`QuadGl::queued_texture_dependency`；
- 构建特性：`render_glyph_uploads`（仅研究编译）、`render_research`；
- 根依赖：`Cargo.toml` 未改默认 macroquad rev，研究 profile 才固定 `27cc446`。

`text.rs` 顶部的开关定义：

```rust
use tracing::debug;

#[cfg(feature = "render_glyph_uploads")]
thread_local! {
    static GLYPH_SCOPED_FLUSH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "render_research")]
pub fn research_glyph_scoped_flush(enabled: bool) {
    #[cfg(feature = "render_glyph_uploads")]
    GLYPH_SCOPED_FLUSH.with(|it| it.set(enabled));
    let _ = enabled;
}

mod layout;
```

`TextPainter::submit` 中与 flush 有关的核心段（按该 commit 原文，完整保留回调前后的控制流）：

```rust
    }

    pub fn line_gap(&self, scale: f32) -> f32 {
        self.brush.fonts()[0].as_scaled(scale).line_gap()
    }

    fn submit(&mut self, tr: Matrix, alpha: f32) {
        #[cfg(feature = "render_research")]
        let _span = crate::research_spikes::span("text_submit");
        let mut flushed = false;
        loop {
            let action = {
                #[cfg(feature = "render_research")]
                let _span = crate::research_spikes::span("glyph_process_queued");
                self.brush.process_queued(
                |rect, tex_data| unsafe {
                    if !flushed {
                        // The cache is private to this painter. Only macroquad
                        // glyph commands can queue reads of it. Unknown queued
                        // materials/captures conservatively keep the old flush.
                        #[cfg(feature = "render_glyph_uploads")]
                        let needs_flush = !GLYPH_SCOPED_FLUSH.with(|it| it.get())
                            || get_internal_gl().quad_gl.queued_texture_dependency(self.cache_texture);
                        #[cfg(not(feature = "render_glyph_uploads"))]
                        let needs_flush = true;
                        if needs_flush {
                        #[cfg(feature = "render_research")]
                        let _span = crate::research_spikes::span("glyph_update_flush");
                        get_internal_gl().flush();
                        } else {
                            #[cfg(feature = "render_research")]
                            let _span = crate::research_spikes::span("glyph_update_flush_skipped");
                        }
                        flushed = true;
                    }
                    {
                        #[cfg(feature = "render_research")]
                        let _span = crate::research_spikes::span("glyph_expand_rgba");
                        self.data_buffer.clear();
                        self.data_buffer.reserve(tex_data.len() * 4);
                        for alpha in tex_data {
                            self.data_buffer.extend_from_slice(&[255, 255, 255, *alpha]);
                        }
                    }
                    #[cfg(feature = "render_research")]
                    let _span = crate::research_spikes::span("glyph_texture_update");
                    self.cache_texture.raw_miniquad_texture_handle().update_texture_part(
                        get_internal_gl().quad_context,
                        rect.min[0] as _,
                        rect.min[1] as _,
                        rect.width() as _,
                        rect.height() as _,
                        &self.data_buffer,
                    );
                },
```

以上是该 `process_queued` 回调中与 flush、RGBA 展开、`update_texture_part` 有关的完整核心段；`|vertex|` 顶点上传回调与后续 brush 处理不属于 flush 决策，未纳入本段。macroquad `QuadGl::queued_texture_dependency` 的完整实现：

```rust
    pub fn draw_stats(&self) -> DrawStats {
        self.draw_stats
    }

    /// Conservatively detect queued reads before an owner updates its texture.
    /// Built-in pipelines only read the primary image. Any custom pipeline is
    /// treated as a dependency: its additional images can change after recording
    /// and its screen snapshot may observe earlier commands.
    ///
    /// This does not wait for already submitted GPU work. A texture owner must
    /// still account for raw GL readers outside this queue and flush before
    /// deleting/resizing the texture. No UV/filter/precision assumptions are made.
    pub fn queued_texture_dependency(&self, texture: Texture2D) -> bool {
        let image = texture.raw_miniquad_texture_handle();
        self.draw_calls[..self.draw_calls_count]
            .iter()
            .any(|draw| draw.texture == image || draw.pipeline.0 >= 4 || draw.capture)
    }

```

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| 运行时开关 | `research_glyph_scoped_flush(bool)` | 只写 `GLYPH_SCOPED_FLUSH` |
| 默认值 | `false` | `Cell::new(false)`；普通构建无此 Cell |
| 构建特性 | `render_glyph_uploads` | 只有该 feature 下才编译查询与跳过分支 |
| 外层特性 | `render_research` | `research_glyph_scoped_flush` 的可见性依赖 |
| 依赖 | macroquad 参考实现 `27cc446` | 只读查询；根 `Cargo.toml` 默认依赖未改 |
| 保守回退 | 任意已排队图集读取、未知材质、capture | `needs_flush=true` |
| 未覆盖 | raw GL、已提交 GPU 读取、其他上下文 | 仍需外部同步 |
| 生产默认 | 未启用 | `ordinary` 保持原 flush |

### 6. 完整测量数据（表格）

8 项真实 UI 加载 ABBA，30 秒窗口，Exoplanetary 与 Petrichor 各 4 轮。逐轮数据：

| 轮次 | 场景 | 策略 | 持续 FPS | 最大间隔 ms | p95 ms | p99 ms |
|---:|---|---|---:|---:|---:|---:|
| 1 | Exoplanetary | 对照 | 59.5470 | 66.8773 | 17.3598 | 18.1843 |
| 2 | Exoplanetary | 对照 | 59.4748 | 63.3441 | 17.3050 | 17.9561 |
| 3 | Exoplanetary | scoped | 59.3728 | 69.8068 | 17.2341 | 18.2773 |
| 4 | Exoplanetary | scoped | 59.5507 | 68.3990 | 17.3346 | 18.2972 |
| 5 | Petrichor | 对照 | 59.5445 | 61.7190 | 17.2207 | 18.1551 |
| 6 | Petrichor | 对照 | 59.4538 | 71.4422 | 17.3981 | 18.7218 |
| 7 | Petrichor | scoped | 59.4732 | 64.9065 | 17.2625 | 18.1396 |
| 8 | Petrichor | scoped | 59.4453 | 65.9348 | 17.3344 | 18.1453 |

两轮中位数汇总：

| 场景 / 指标 | 对照 | scoped | 差值 | 判定 |
|---|---:|---:|---:|---|
| Exoplanetary 持续 FPS | 59.5109 | 59.4618 | −0.0491 | 略降 |
| Exoplanetary 最大间隔 | 65.1107ms | 69.1029ms | +3.9922ms | 明显变差 |
| Exoplanetary p95 | 17.3324ms | 17.2844ms | −0.0480ms | 噪声 |
| Exoplanetary p99 | 18.0702ms | 18.2873ms | +0.2171ms | 略差 |
| Petrichor 持续 FPS | 59.4991 | 59.4593 | −0.0398 | 略降 |
| Petrichor 最大间隔 | 66.5806ms | 65.4206ms | −1.1600ms | 略好，但不足 |
| Petrichor p95 | 17.3094ms | 17.2985ms | −0.0109ms | 噪声 |
| Petrichor p99 | 18.4384ms | 18.1425ms | −0.2959ms | 略好 |

场景分解说明了为什么“最大间隔”不可能被这个改动改善：两首歌的最大间隔都出现在 `LibraryPage`，不是 `LoadingScene`。字形更新发生在 Loading/Ending 的字形上传路径；它不在最大间隔所在的场景。

| 场景 | 对照组最大间隔（两轮） | scoped 最大间隔（两轮） |
|---|---:|---:|
| Exo LibraryPage | 66.8773 / 63.3441ms | 69.8068 / 68.3990ms |
| Exo LoadingScene | 59.9607 / 50.8551ms | 48.1228 / 50.0409ms |
| Pet LibraryPage | 61.7190 / 71.4422ms | 64.9065 / 65.9348ms |
| Pet LoadingScene | 51.2155 / 53.8004ms | 55.7760 / 49.2692ms |

质量与契约：

| 项目 | 数量 | 结果 |
|---|---:|---|
| 质量作业 | 12 项 | 完成 |
| 严格 RGBA 分组 | 9 组 | 全部相同 |
| 加载 ABBA | 8 项 | 完成 |
| Ending 覆盖 | 未完成 | 只能证明加载窗口 |
| 完整 gameplay 覆盖 | 未完成 | 不能外推 |
| 研究后端 SHA-256 | `204955169b3d61a562715c458c26e5d24f0ac13d4a2d3fae28306cf2fc2d6c2e` | `glyph-scope-candidate.so` |
| 矩阵 SHA-256 | `c9fe2042fe18281c70e705e13dcda875c9cda0d03fa40341fc3216a57b4a3460` | `glyph-scoped-loading-matrix/latency-summary.json` |

### 7. 质量与回归结果

- 12 项质量作业、9 组严格 RGBA 全部相同。覆盖 UI、Happy ending、Petrichor 等固定画面；没有出现像素差。
- 但这只是“跳过 flush 不改变最终图像”的证据，不是“跳过 flush 不会引入读写竞态”的证据。像素相同可能是因为驱动在 `update_texture_part` 内部完成了必要的同步，而不是因为跳过 flush 没有代价。
- Ending 段没有完成同等级别的覆盖；而 Ending 正是字形更新最集中的场景（114 次更新、217ms 累计）。因此本条的负面结论不能外推到 Ending 的尾延迟，但也不能用当前 9 组 RGBA 证明 Ending 安全。
- 8 项加载 ABBA 的 native 计数严格匹配，无截图/hash 混入性能 cohort；这是延迟数据可信的前提。
- 没有完整 gameplay、没有全曲、没有多 GPU、没有不同字体。质量结论的边界必须与性能结论一起读。

### 8. 为什么判定为负（逐项对比）

| 判据 | 期望 | 实际 | 结论 |
|---|---|---|---|
| 加载最大间隔下降 | 低于 65.111 / 66.581ms | 69.103 / 65.421ms | Exo 明显变差 |
| 持续 FPS 不降 | ≥ 对照 | 两首均略降 | 负 |
| p95/p99 | 稳定改善 | 微幅、方向不一致 | 噪声 |
| 读写冲突减少 | 更新前等待消失 | 等待移到更新内部 | 未减少总等待 |
| 覆盖最大间隔场景 | LibraryPage 也受益 | LibraryPage 不受影响 | 机制不匹配 |
| 质量 | 严格相同 | 相同 | 通过，但不能抵消延迟 |
| Ending 覆盖 | 应有 | 未完成 | 不能证明完整安全 |

机制层面的负优化原因是：

1. **flush 不是等待的全部。** `flush` 只把命令提交给驱动；真正危险的是“GPU 还在采样旧图集时 CPU 写入新像素”。跳过 flush 后，`update_texture_part` 仍然要面对这个危险；驱动可能在 `glTexSubImage2D` 内部做隐式同步，表现为更新调用变慢。总时间没有消失，只是从 `flush` 计时项移到 `glyph_texture_update` 计时项。
2. **查询本身有成本。** 每次回调都要扫描 `draw_calls[..draw_calls_count]`。字形更新是高频路径，单次扫描不大，但 12 项质量/8 项 ABBA 中没有任何一项显示“跳过 flush 后总时间下降”，说明扫描与调度成本至少抵消了少量收益。
3. **最大间隔不在字形路径。** 表 6 的场景分解显示最大间隔在 LibraryPage；即使 LoadingScene 的字形更新变快，全窗口最大间隔仍由 Library 的转场/资源工作决定。用“加载最大间隔”验收这个改动，本身就是错配。
4. **Exo 的 +3.99ms 不是随机噪声可以忽略的量级。** 两轮对照 66.877/63.344，两轮 scoped 69.807/68.399；scoped 的两轮都高于对照的较高值。Petrichor 的 −1.16ms 则落在对照两轮 61.719–71.442 的范围内。方向不一致、量级不足，不能称为收益。
5. **持续 FPS 两首均降。** 59.5109→59.4618、59.4991→59.4593，降幅约 0.05–0.04fps；数值很小，但方向与“减少提交”相反，说明跳过 flush 没有释放出可用的帧时间。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：prpr 侧 commit `4f046544` |
| 参考实现（macroquad 库） | 已落地（未合入 main）：独立库 commit `27cc446`，已推送参考实现远端 |
| 根依赖 | 默认 macroquad rev 未改；只有研究 profile 固定 `27cc446` |
| 默认 | `GLYPH_SCOPED_FLUSH=false` |
| 普通路径 | 未接入；`ordinary` 保持原 flush |
| 主线是否已实施 | 否（main）；未进入 |
| 研究后端 | `glyph-scope-candidate.so`，SHA `204955169b3d61a562715c458c26e5d24f0ac13d4a2d3fae28306cf2fc2d6c2e` |
| 质量/契约 | 12 项/9 组 RGBA 相同 |
| Ending | 未完成 |
| 后续 | 不推进；保留为“跳过 flush 不等于消除同步”的反例 |

### 10. 可挽救的部分与后续条件

可挽救的是“减少字形更新与已提交读取的冲突”这个方向，而不是当前这个开关。后续若继续，需要：

1. **用显式 fence 证明完成，而不是用队列查询推测。** 在更新图集前插入 `glClientWaitSync` 或等价机制，只有 fence 已 signal 才跳过 flush；否则保持原 flush。查询只能证明“队列里没有读”，不能证明“GPU 没有读”。
2. **把更新与绘制解耦到不可变页。** 使用多版本图集页/immutable texture，写入新页而不是覆盖正在采样的页；这样不需要在每次更新前同步，且不会改变 UV/过滤/字形代表。
3. **按更新时间而非 flush 时间验收。** 统计 `glyph_texture_update` 的墙钟、`flush` 的墙钟、以及二者之和；只有总和下降才算收益。当前数据只显示 flush 被跳过，没有显示总和下降。
4. **选对场景。** 如果目标是 Ending 尖峰，应在 Ending 的 114 次更新/217ms 上做 ABBA；如果目标是 Loading，应先把 LibraryPage 的最大间隔纳入同一窗口，否则最大间隔不会动。
5. **保留未知材质/capture 的保守回退。** 这一点当前实现做对了；后续版本不能为了覆盖率删除它。
6. **完整质量回归。** 至少覆盖 Ending、不同字体、不同 atlas 尺寸、动态 UI；9 组固定画面不足以外推。

### 11. 来源与可追溯性（仅溯源）

- prpr 源码：`prpr/src/ui/text.rs`，commit `4f046544`。
- macroquad 源码：参考实现，commit `27cc446777d70c624ec8d25d0f0757fc7c8b19aa`，文件 `src/quad_gl.rs`。
- 开关符号：`GLYPH_SCOPED_FLUSH`、`research_glyph_scoped_flush`、`queued_texture_dependency`。
- 后端：`本地证据/backends/glyph-scope-candidate.so.json`，SHA `204955169b3d61a562715c458c26e5d24f0ac13d4a2d3fae28306cf2fc2d6c2e`。
- 数据：`本地证据/glyph-scoped-loading-matrix/latency-summary.json`（矩阵 SHA `c9fe2042fe18281c70e705e13dcda875c9cda0d03fa40341fc3216a57b4a3460`）、`本地证据/glyph-scoped-quality-matrix`、`本地证据/glyph-scoped-quality-pixels.log`。
- 文档：`docs/development/game-performance-target.md` 第 42 行；`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 38、42、47 行。
- 原始段落一（`本地证据/AGENTS.md` 历史快照，第 42 行）：

> 字库候选已完成验收但未接入：glyph_scoped_flush的8项加载ABBA无可靠收益（Exo最大间隔中位数65.111→69.103ms，Petrichor66.581→65.421ms；两者持续FPS略降）；独立12项质量/9项严格RGBA全同，仍默认false。独立7/8模式glyph微基准也完成并保留输入：完整GPU复制completed p50约490/492ms、部分GPU复制262/265ms均排除；部分CPU版本有界重分配37.340/36.216ms，80MiB纹理，复用池49.483ms更慢，不推进该池。各轮6/7项严格RGBA全同，仅同步glFinish诊断、单字形fixture、非游戏FPS/普遍语义证明。下一步PBO原序上传探针尚未验证；不能将这些实验称生产优化或完整尖峰问题已解决。平均帧率达标后继续分析p95/p99/max和实际长帧，保留加载/结算尖峰，零尖峰不强求。

- 原始段落二（同文件第 47 行）：

> 下一候选glyph_scoped_flush默认false且ordinary保持原flush：只读queued_texture_dependency API在独立 macroquad 参考实现已 push 27cc446777d70c624ec8d25d0f0757fc7c8b19aa，Android check通过；无硬件扩展/UV假设，任何已排队atlas读取、未知材质或capture都保留原flush，已提交GPU读取仍由原GL更新同步处理。独立profile glyph-upload-order.json固定它与mini000ea930；新render_glyph_uploads feature仅研究编译，glyph-scope-candidate已release，根默认依赖未改。8项真实UI加载ABBA正在测量，质量和Ending覆盖尚未完成，不宣称消除glyph读写冲突或启用生产。

## N-08 mask 有限域查表 + 逐行 bbox

### 1. 一句话结论与判定（无 + 理由）

**判定：无优化（局部约 0.85ms 微正，整帧无感）。** 两项 CPU 候选都保持了原 float32 运算次序与 R8 量化，像素严格通过，局部阶段也确实变快：Petrichor 效果窗口 `mask` 均值 7.610→6.757ms、`compose` 均值 3.083→2.202ms。但整帧没有可重复收益：同窗口持续 FPS 58.237→58.338（+0.101，约 0.17%），frame p95 20.995→20.985ms（−0.01ms），正常窗口 59.602→59.727（+0.125）。这些变化落在对照自身漂移内，且 Happy ending 没有 mask 工作，无法从两首谱面证明普遍收益。候选还常驻约 512KiB 查表，因此没有进入普通路径。

### 2. 问题与初衷

Petrichor 的 block 效果在真实密集窗口里，mask CPU 求值墙钟均值约 7.6ms，其中 `compose` 约 3.08ms。原实现逐像素执行：

- subtract 摄像机的 `enabled` 判断：`(red/255) ∈ [0.09, 0.12)`；
- disabled 通道：`green` 先做 smoothstep，再乘 `red`，再与 normal 相减、取绝对值、量化；
- ready 通道：对 `green` 做 R8 量化；
- compose：对每个非零像素做四次 `min/max` 来维护 bbox。

这些输入已经被上游量化为 R8，因此取值域是有限的：`red` 256 种、`green` 256 种、`normal` 256 种。两项候选分别利用这两个事实：

1. **有限域查表**：预计算 `red × green` 的 65,536 组 `(product, green)`，把逐像素的 smoothstep/乘法换成一次数组索引；保留最终与 normal 相减、取绝对值、量化的原顺序，不预先量化乘积。
2. **逐行 bbox**：不再对每个非零像素做四次 min/max，而是按行找首末非零像素，每行只更新一次 bbox；非零像素的并集首末与逐像素 min/max 完全等价。

初衷是“用 512KiB 的只读表换掉每帧数毫秒的算术”，属于典型的空间换时间。它不改变 mask 的几何、噪声、采样或最终纹理，只是把 CPU 侧的等价计算换一种写法。

### 3. 实现思路与具体机制

`block_mask_values.rs` 的核心是一个 `OnceLock<Box<[Subtract]>>` 表：

- `Subtract { product: f32, green: u8 }`，按 `red * 256 + green` 索引；
- `reference_subtract(red, green)` 完整复刻原 shader 等价算术：
  - `g = green as f32 / 255.`；
  - `t = ((g - 0.2) * -10.).clamp(0., 1.)`；
  - `r = enabled(red) as f32 + t*t*(3. - 2.*t)`；
  - 返回 `(unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)`；
- 表中保存 `product = r * g`（未量化）与 `green = unorm(g)`；
- `disabled(normal, red, green)`：`green == 0` 直接返回 `normal`；否则 `unorm((product - normal/255).abs())`；
- `ready(red, green)`：`green == 0` 返回 0，否则返回表中的 `green`；
- `enabled(red)`：`(23..=30).contains(&red)`，这是 `[0.09, 0.12)` 在 R8 上的精确整数等价域；
- `prepare()` 强制初始化表；`set_lookup_values(true)` 时调用。

`block_mask.rs` 的 `compose_masks<const LOOKUP: bool, const ROW_BOUNDS: bool>` 用 const generic 在编译期选择四条路径：

- `LOOKUP=false`：走原 `subtract_enabled` / `subtract_disabled`；
- `LOOKUP=true`：走 `values::enabled` / `values::disabled` / `values::ready`；
- `ROW_BOUNDS=false`：每个非零 `enabled_compose` 像素做四次 bbox min/max；
- `ROW_BOUNDS=true`：逐行 `position` / `rposition` 找首末非零，再更新 bbox。

外层 `render_displaced` 在 `render_lifetime` 下按 `(lookup_values, row_bounds)` 四组合分派到具体单态，避免在每像素循环里做运行期分支。`source_bounds` 随后被用于限制 displacement 的遍历范围：`pad_x = ceil(bw * 0.072) + 1`、`pad_y` 同理，保证噪声位移最多 0.070704 的保守包络；区域外像素保持 `base_rgba` 的零值，`active` 位图也不置位。这不是硬件 scissor，而是 CPU 侧跳过必然为空的像素。

一个容易忽略的细节：`compose_masks` 里的 `disabled` 在 `LOOKUP=false` 时对 `(layers[2], layers[3], raw_disabled_green)` 全零有早退；`LOOKUP=true` 时由 `values::disabled` 的 `green == 0` 早退承担同样语义。`ready` 也一样。这些早退保证全零区域不会因为查表而多算。

### 4. 文件、符号与完整核心代码

源码快照：参考实现，commit `4f046544`。文件与符号：

- `prpr/src/core/block_mask_values.rs`：`Subtract`、`table`、`prepare`、`enabled`、`disabled`、`ready`、`reference_subtract`、`exact_for_all_r8_camera_inputs`；
- `prpr/src/core/block_mask.rs`：`Masks`、`set_lookup_values`、`matches_viewport`、`render_displaced`、`compose_masks`、`source_bounds`；
- `prpr/src/core/block_shader.rs`：`research_mask_lookup(enabled, rows)` 写 `masks.lookup_values` / `masks.row_bounds` 与 spare viewport；
- 构建特性：`render_lifetime`（候选开关与分派）、`render_research`（计时）。

`block_mask_values.rs` 全文（含穷举测试，未省略）：

```rust
//! Exact finite-domain evaluation of the R8 subtract cameras.
//! No SIMD, GL extension, approximation, or extra image quantization.
use std::sync::OnceLock;

#[derive(Clone, Copy, Default)]
struct Subtract {
    product: f32,
    green: u8,
}

fn unorm(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn reference_subtract(red: u8, green: u8) -> (f32, f32) {
    let g = green as f32 / 255.;
    let t = ((g - 0.2) * -10.).clamp(0., 1.);
    let r = u8::from(enabled(red)) as f32 + t * t * (3. - 2. * t);
    (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
}

fn table() -> &'static [Subtract] {
    static TABLE: OnceLock<Box<[Subtract]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries = vec![Subtract::default(); 256 * 256];
        for red in 0..=255_u8 {
            for green in 0..=255_u8 {
                let (r, g) = reference_subtract(red, green);
                entries[(red as usize) * 256 + green as usize] = Subtract {
                    // The product is not quantized. Keep the original f32
                    // multiplication before the final normal-camera subtract.
                    product: r * g,
                    green: unorm(g),
                };
            }
        }
        entries.into_boxed_slice()
    })
}

pub(super) fn prepare() { let _ = table(); }

pub(super) fn enabled(red: u8) -> bool {
    // Exhaustively checked against the original f32 [0.09, 0.12) test.
    (23..=30).contains(&red)
}

pub(super) fn disabled(normal: u8, red: u8, green: u8) -> u8 {
    if green == 0 {
        // The subtract product is zero for every red input in this case.
        return normal;
    }
    let product = table()[(red as usize) * 256 + green as usize].product;
    unorm((product - normal as f32 / 255.).abs())
}

pub(super) fn ready(red: u8, green: u8) -> u8 {
    if green == 0 { 0 } else { table()[(red as usize) * 256 + green as usize].green }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent copy of the original shader-equivalent arithmetic, including
    // both intermediate R8 roundings and the final output quantization.
    fn original(red: u8, green: u8) -> (f32, f32) {
        let g = green as f32 / 255.;
        let t = ((g - 0.2) * -10.).clamp(0., 1.);
        let r = if (0.09..0.12).contains(&(red as f32 / 255.)) { 1. } else { 0. }
            + t * t * (3. - 2. * t);
        (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
    }

    #[test]
    fn exact_for_all_r8_camera_inputs() {
        for red in 0..=255_u8 {
            assert_eq!(enabled(red), (0.09..0.12).contains(&(red as f32 / 255.)));
            for green in 0..=255_u8 {
                let (r, g) = original(red, green);
                assert_eq!(ready(red, green), if green == 0 { 0 } else { unorm(g) });
                for normal in 0..=255_u8 {
                    let expected = if normal == 0 && red == 0 && green == 0 { 0 }
                        else { unorm((r * g - normal as f32 / 255.).abs()) };
                    assert_eq!(disabled(normal, red, green), expected,
                        "normal={normal}, red={red}, green={green}");
                }
            }
        }
    }
}
```

`block_mask.rs` 的结构体与开关（原文第 1–90 行）：

```rust
//! CPU equivalent of the native mask, EdgeMask and GlowMask passes.
//! Bit rows keep the nine-tap dilation inexpensive without any offscreen FBOs.
use super::Zone;
#[path = "block_mask_values.rs"]
mod values;
use once_cell::sync::Lazy;
use std::collections::HashMap;

static DISPLACE: Lazy<image::RgbImage> = Lazy::new(|| {
    let source = image::load_from_memory(include_bytes!("../../../assets/blockarea/BlockNoise1.png"))
        .unwrap()
        .to_rgb8();
    image::imageops::flip_vertical(&source)
});

#[derive(Default)]
pub(super) struct Masks {
    pub rgba: Vec<u8>,
    /// Ready-only normal R, ready-only postprocessed subtract G, disabled compose, hover.
    pub aux_rgba: Vec<u8>,
    /// Point-sampled raw camera outputs, packed enabled N/S and disabled N/S.
    pub sources_rgba: Vec<u8>,
    /// Native camera capture is needed only by the independent mask oracle.
    pub capture_sources: bool,
    pub raw_disabled_green: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub revision: u64,
    pub aux_revision: u64,
    #[cfg(feature = "render_lifetime")]
    pub evaluation_counts: [u64; 4],
    #[cfg(feature = "render_lifetime")]
    pub lookup_values: bool,
    #[cfg(feature = "render_lifetime")]
    pub row_bounds: bool,
    #[cfg(feature = "render_research")]
    pub timing_enabled: bool,
    #[cfg(feature = "render_research")]
    pub timings: [f64; 6],
    active: Vec<u64>,
    ping: Vec<u64>,
    pong: Vec<u64>,
    last_dim: (usize, usize, f32),
    last_zones: Vec<Zone>,
    last_time: Option<f32>,
    layers: [Vec<u8>; 6],
    ready_green: Vec<u8>,
    gray_ping: Vec<u8>,
    gray_pong: Vec<u8>,
    warp_x: Vec<(f32, usize, usize)>,
    warp_y: Vec<(f32, usize)>,
    enabled_compose: Vec<u8>,
    uniform_compose: Option<u8>,
    base_rgba: Vec<u8>,
    coverage_diff: [Vec<i32>; 8],
    blend_steps: HashMap<u32, Option<u8>>,
    prepared_rows: HashMap<[u32; 5], Box<[(u16, u16, u16)]>>,
    prepared_dim: (usize, usize, f32),
    prepared_bytes: usize,
    source_bounds: Option<(usize, usize, usize, usize)>,
    group_index: HashMap<[u32; 5], usize>,
    groups: Vec<(Zone, [i32; 8])>,
}

impl Masks {
    #[cfg(feature = "render_lifetime")]
    pub fn set_lookup_values(&mut self, enabled: bool) {
        if enabled { values::prepare(); }
        self.lookup_values = enabled;
    }

    #[cfg(feature = "render_research")]
    fn research_begin(&self) -> Option<std::time::Instant> {
        self.timing_enabled.then(std::time::Instant::now)
    }

    #[cfg(feature = "render_research")]
    fn research_end(&mut self, start: Option<std::time::Instant>, stage: usize) {
        if let Some(start) = start { self.timings[stage] += start.elapsed().as_secs_f64() * 1000.; }
    }

    #[cfg(feature = "render_lifetime")]
    pub fn matches_viewport(&self, width: usize, height: usize, aspect: f32) -> bool {
        self.last_dim == ((width / 8).max(1) * 2, (height / 8).max(1) * 2, aspect)
    }

    /// Pre-rasterize exact keyframe poses, never quantizing animated geometry
    /// or chart time. Holds/static intervals reuse these poses; intermediates
    /// fall back to live rasterization. Hard 16 MiB budget, retained on retry.
    pub fn prepare_geometry(&mut self, width: usize, height: usize, aspect: f32, areas: &[super::BlockArea]) {
```

`block_mask.rs` 的 `render_displaced` 与 `compose_masks`（原文第 126–410 行，含查表/逐行 bbox 分派、source_bounds 推导与位移遍历）：

```rust
    pub fn render_displaced(&mut self, width: usize, height: usize, aspect: f32, zones: &[Zone], time: f32) {
        // Refine Compose to the existing EffectRT grid: half-sized boundary
        // cells without larger effect textures, more shader work or narrower
        // edge/glow rings. Native geometry and displacement math stay intact.
        let (bw, bh) = ((width / 8).max(1) * 2, (height / 8).max(1) * 2);
        let (ew, eh) = (bw, bh);
        if self.last_dim == (ew, eh, aspect) && self.last_zones == zones && (self.last_time == Some(time) || self.uniform_compose.is_some()) {
            #[cfg(feature = "render_lifetime")]
            { self.evaluation_counts[0] += 1; }
            return;
        }
        #[cfg(feature = "render_research")]
        let geometry_start = self.research_begin();
        if self.last_dim != (ew, eh, aspect) || self.last_zones != zones {
            #[cfg(feature = "render_lifetime")]
            { self.evaluation_counts[1] += 1; }
            self.last_dim = (ew, eh, aspect);
            self.last_zones.clear();
            self.last_zones.extend_from_slice(zones);
            for layer in &mut self.layers {
                layer.resize(bw * bh, 0);
                layer.fill(0);
            }
            self.raw_disabled_green.resize(bw * bh, 0);
            self.raw_disabled_green.fill(0);
            self.ready_green.resize(bw * bh, 0);
            self.ready_green.fill(0);
            if self.prepared_dim != (bw, bh, aspect) {
                self.clear_prepared_geometry();
            }
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
            let mut curves = [[0_u8; 256]; 8];
            let mut curve_end = [0; 8];
            for c in 0..8 {
                if !used[c] || !same_opacity[c] {
                    continue;
                }
                let opacity = f32::from_bits(first_opacity[c].unwrap());
                for n in 1..256 {
                    let value = unorm(curves[c][n - 1] as f32 / 255. + opacity);
                    if value == curves[c][n - 1] {
                        break;
                    }
                    curves[c][n] = value;
                    curve_end[c] = n;
                    if value == 255 {
                        break;
                    }
                }
            }
            for channel in 0..8 {
                if fast[channel] {
                    self.coverage_diff[channel].resize((bw + 1) * bh, 0);
                    self.coverage_diff[channel].fill(0);
                }
            }
            self.group_index.clear();
            self.groups.clear();
            // The six official cameras capture enabled N/S, disabled N/S
            // (including ready), and ready-only N/S. BlockSprite blends
            // SrcAlpha, One; each subtract sprite has alpha .1.
            for z in zones {
                let mut weights = [0_i32; 8];
                let mut slow = false;
                for (channel, opacity) in contributions(z).into_iter().flatten() {
                    if fast[channel] {
                        weights[channel] += if same_opacity[channel] {
                            1
                        } else {
                            self.blend_steps[&opacity.to_bits()].unwrap() as i32
                        };
                    } else {
                        slow = true;
                    }
                }
                if weights != [0; 8] {
                    let key = geometry_key(z);
                    let group = *self.group_index.entry(key).or_insert_with(|| {
                        self.groups.push((z.clone(), [0; 8]));
                        self.groups.len() - 1
                    });
                    for c in 0..8 {
                        self.groups[group].1[c] += weights[c];
                    }
                }
                if !slow {
                    continue;
                }
                let mut write = |y: usize, first: usize, last: usize| {
                    for (channel, opacity) in contributions(z).into_iter().flatten() {
                        if !fast[channel] {
                            // Half-integer blends can round differently depending
                            // on the previous R8 value. Preserve source ordering.
                            let plane = match channel {
                                6 => &mut self.raw_disabled_green,
                                7 => &mut self.ready_green,
                                _ => &mut self.layers[channel],
                            };
                            for pixel in &mut plane[y * bw + first..y * bw + last] {
                                *pixel = unorm(*pixel as f32 / 255. + opacity);
                            }
                        }
                    }
                };
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
                    for &(y, first, last) in rows.iter() {
                        write(y as usize, first as usize, last as usize);
                    }
                } else {
                    raster_rows(bw, bh, aspect, z, write);
                }
            }
            // Merge equal poses before visiting their rows. Source order is
            // irrelevant only for the proven integer/count channels above.
            for (z, weights) in &self.groups {
                let mut write = |y: usize, first: usize, last: usize| {
                    for c in 0..8 {
                        if weights[c] == 0 {
                            continue;
                        }
                        self.coverage_diff[c][y * (bw + 1) + first] += weights[c];
                        self.coverage_diff[c][y * (bw + 1) + last] -= weights[c];
                    }
                };
                if let Some(rows) = self.prepared_rows.get(&geometry_key(z)) {
                    for &(y, first, last) in rows.iter() {
                        write(y as usize, first as usize, last as usize);
                    }
                } else {
                    raster_rows(bw, bh, aspect, z, write);
                }
            }
            // Prefix sums make heavily overlapping active rectangles cost
            // O(rectangles * rows + pixels), rather than O(sum of their areas).
            // Retain the exact R8 quantization of every native additive blend.
            for channel in 0..8 {
                if !fast[channel] {
                    continue;
                }
                let plane = match channel {
                    6 => &mut self.raw_disabled_green,
                    7 => &mut self.ready_green,
                    _ => &mut self.layers[channel],
                };
                for y in 0..bh {
                    let mut sum = 0_i32;
                    for x in 0..bw {
                        sum += self.coverage_diff[channel][y * (bw + 1) + x];
                        plane[y * bw + x] = if same_opacity[channel] {
                            curves[channel][sum.clamp(0, curve_end[channel] as i32) as usize]
                        } else {
                            sum.clamp(0, 255) as u8
                        };
                    }
                }
            }
            #[cfg(feature = "render_research")]
            self.research_end(geometry_start, 4);
            #[cfg(feature = "render_research")]
            let compose_start = self.research_begin();
            #[cfg(feature = "render_lifetime")]
            match (self.lookup_values, self.row_bounds) {
                (false, false) => self.compose_masks::<false, false>(bw, bh),
                (true, false) => self.compose_masks::<true, false>(bw, bh),
                (false, true) => self.compose_masks::<false, true>(bw, bh),
                (true, true) => self.compose_masks::<true, true>(bw, bh),
            }
            #[cfg(not(feature = "render_lifetime"))]
            self.compose_masks::<false, false>(bw, bh);
            if cfg!(test) || self.capture_sources {
                self.sources_rgba.resize(bw * bh * 4, 0);
                for i in 0..bw * bh {
                    for c in 0..4 {
                        self.sources_rgba[i * 4 + c] = self.layers[c][i];
                    }
                }
            }
            self.uniform_compose = self
                .enabled_compose
                .first()
                .copied()
                .filter(|v| self.enabled_compose.iter().all(|p| p == v));
            self.aux_revision = self.aux_revision.wrapping_add(1);
            #[cfg(feature = "render_research")]
            self.research_end(compose_start, 5);
        }
        #[cfg(feature = "render_research")]
        self.research_end(geometry_start, 0);
        #[cfg(feature = "render_research")]
        let displacement_start = self.research_begin();
        self.width = ew;
        self.height = eh;
        // Only Compose/Edge/Glow evolve with noise time. Ready/Disabled camera
        // channels stay unchanged until geometry/phase/opacity changes.
        self.rgba.clone_from(&self.base_rgba);
        let stride = ew.div_ceil(64);
        self.active.resize(stride * eh, 0);
        self.active.fill(0);
        // Both native displacement samples share their Y coordinate. Compute
        // mirrored texture indices per row/column, rather than running four
        // floating-point remainders and image lookups for every mask pixel.
        let texture = &*DISPLACE;
        let pixels = texture.as_raw();
        let texture_stride = texture.width() as usize * 3;
        let d = 0.70703125_f32;
        let dt = d * (time / 20. * 2.59);
        self.warp_x.clear();
        self.warp_y.clear();
        if self.uniform_compose.is_none() {
            self.warp_x.extend((0..bw).map(|x| {
                let u = (x as f32 + 0.5) / bw as f32;
                (u, noise_index(dt + u * 2.13, texture.width()) as usize * 3, noise_index(-dt + u * 2.13, texture.width()) as usize * 3)
            }));
            self.warp_y.extend((0..bh).map(|y| {
                let v = (y as f32 + 0.5) / bh as f32;
                (v, noise_index(dt + v * 1.02, texture.height()) as usize * texture_stride)
            }));
        }
        static CENTERED_NOISE: Lazy<[f32; 256]> = Lazy::new(|| std::array::from_fn(|value| medium(medium(value as f32 / 255.) - 0.5)));
        let centered = &*CENTERED_NOISE;
        // Both centered noise channels lie in [-.5,.5]. The native normalized
        // direction therefore displaces UV by at most .070704 per axis. A
        // conservative .072 envelope skips empty pixels with identical samples.
        let pad_x = (bw as f32 * 0.072).ceil() as usize + 1;
        let pad_y = (bh as f32 * 0.072).ceil() as usize + 1;
        let (x0, y0, x1, y1) = self
            .source_bounds
            .map(|(x0, y0, x1, y1)| (x0.saturating_sub(pad_x), y0.saturating_sub(pad_y), (x1 + pad_x).min(bw), (y1 + pad_y).min(bh)))
            .unwrap_or((0, 0, 0, 0));
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * bw + x;
                let mask = if let Some(value) = self.uniform_compose {
                    value
                } else {
                    let (u, xa, xb) = self.warp_x[x];
                    let (v, row) = self.warp_y[y];
                    let a = centered[pixels[row + xa] as usize];
                    let b = centered[pixels[row + xb] as usize];
                    let duv = [(d * a + b * -d) * 0.1 + u, (d * a + b * d) * 0.1 + v];
                    let sx = (duv[0] * bw as f32).floor().clamp(0., (bw - 1) as f32) as usize;
                    let sy = (duv[1] * bh as f32).floor().clamp(0., (bh - 1) as f32) as usize;
                    self.enabled_compose[sy * bw + sx]
                };
                let dst = i * 4;
                self.rgba[dst] = mask;
                if mask != 0 {
                    self.active[y * stride + x / 64] |= 1 << (x % 64);
                }
            }
        }
        #[cfg(feature = "render_research")]
        self.research_end(displacement_start, 1);
        self.render_rings();
        self.last_time = Some(time);
        self.revision = self.revision.wrapping_add(1);
    }

    fn compose_masks<const LOOKUP: bool, const ROW_BOUNDS: bool>(&mut self, bw: usize, bh: usize) {
        self.enabled_compose.resize(bw * bh, 0);
        self.base_rgba.resize(bw * bh * 4, 0);
        self.base_rgba.fill(0);
        self.aux_rgba.resize(bw * bh * 4, 0);
```

`block_mask.rs` 的量化与 subtract 辅助函数（原文第 540–630 行）：

```rust
fn glow_weights() -> [f32; 6] {
    static WEIGHTS: Lazy<[f32; 6]> = Lazy::new(|| {
        let sum: f32 = (1..=6).map(|k| (k as f32).powf(2.65)).sum();
        std::array::from_fn(|pass| ((6 - pass) as f32).powf(2.65) / sum)
    });
    *WEIGHTS
}

fn unorm(v: f32) -> u8 {
    (v.clamp(0., 1.) * 255.).round() as u8
}

fn subtract_enabled(red: u8) -> f32 {
    // Native SubtractBlockBlender pass 0: a threshold window, not parity.
    let r = red as f32 / 255.;
    if (0.09..0.12).contains(&r) {
        1.
    } else {
        0.
    }
}

fn subtract_disabled(red: u8, green: u8) -> (f32, f32) {
    let g = green as f32 / 255.;
    let t = ((g - 0.2) * -10.).clamp(0., 1.);
    let r = subtract_enabled(red) + t * t * (3. - 2. * t);
    // The intermediate is RG16, clamped and rounded after shader output.
    (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
}

pub(super) fn compose_uv(uv: [f32; 2], time: f32) -> [f32; 2] {
    // BlockCompose pass 0, preserving the native normalized direction and ST.
    // Native u_xlat16_* are mediump, not full-precision CPU floats. In the
    // exported GLES shader the inverse length, direction, sampled noise and
    // centered noise are separately rounded to binary16. Keeping the warp
    // coordinates highp after those steps is essential at point boundaries.
    let d = medium(0.5 * medium((0.5_f32).sqrt().recip()));
    let t = time / 20. * 2.59;
    let st = [uv[0] * 2.13, uv[1] * 1.02];
    let a = medium(medium(noise([d * t + st[0], d * t + st[1]])) - 0.5);
    let b = medium(medium(noise([-d * t + st[0], d * t + st[1]])) - 0.5);
    [(d * a + b * -d) * 0.1 + uv[0], (d * a + b * d) * 0.1 + uv[1]]
}

fn medium(value: f32) -> f32 {
    // The normalized noise/direction intermediates are zero or normal f16
    // numbers. Round ten fraction bits with ties-to-even; no heap or new dep.
    let bits = value.to_bits();
    f32::from_bits((bits + 0xfff + ((bits >> 13) & 1)) & !0x1fff)
}

fn noise_index(v: f32, size: u32) -> u32 {
    let v = v.rem_euclid(2.);
    let v = if v > 1. { 2. - v } else { v };
    (v * size as f32).floor().clamp(0., (size - 1) as f32) as u32
}

fn noise(uv: [f32; 2]) -> f32 {
    // Match the vertically flipped rows uploaded by the material renderer.
    DISPLACE.get_pixel(noise_index(uv[0], DISPLACE.width()), noise_index(uv[1], DISPLACE.height()))[0] as f32 / 255.
}

fn geometry_key(zone: &Zone) -> [u32; 5] {
    [
        zone.center.x.to_bits(),
        zone.center.y.to_bits(),
        zone.half.x.to_bits(),
        zone.half.y.to_bits(),
        zone.angle.to_bits(),
    ]
}

fn contributions(z: &Zone) -> [Option<(usize, f32)>; 4] {
    let layer = if z.active { usize::from(z.invert) } else { 2 + usize::from(z.invert) };
    let opacity = if z.invert { 0.1 } else { z.opacity };
    [
        Some((layer, opacity)),
        (z.invert && !z.active).then_some((6, 0.1 * z.opacity)),
        z.ready.then_some((4 + usize::from(z.invert), opacity)),
        (z.ready && z.invert).then_some((7, 0.1 * z.opacity)),
    ]
}

fn blend_step(opacity: f32) -> Option<u8> {
    let step = unorm(opacity);
    (0..=255_u8)
        .all(|previous| unorm(previous as f32 / 255. + opacity) == previous.saturating_add(step))
        .then_some(step)
}

fn raster_rows(width: usize, height: usize, aspect: f32, zone: &Zone, mut write: impl FnMut(usize, usize, usize)) {
    if zone.half.x <= 0. || zone.half.y <= 0. {
        return;
    }
    let (s, c) = zone.angle.sin_cos();
    let y_half = s.abs() * zone.half.x + c.abs() * zone.half.y;
    let start = (((zone.center.y - y_half) * aspect + 1.) * height as f32 * 0.5 - 0.5)
        .ceil()
        .clamp(0., height as f32) as usize;
    let end = ((((zone.center.y + y_half) * aspect + 1.) * height as f32 * 0.5 - 0.5).floor() + 1.).clamp(0., height as f32) as usize;
    for y in start..end {
        let dy = ((y as f32 + 0.5) * 2. / height as f32 - 1.) / aspect - zone.center.y;
        let (mut lo, mut hi) = (-1.0_f32, 1.0_f32);
        for (a, b, half) in [(c, s * dy, zone.half.x), (-s, c * dy, zone.half.y)] {
            if a.abs() < 1e-7 {
                if b.abs() > half {
                    hi = lo - 1.;
                    break;
                }
            } else {
                let a0 = (-half - b) / a + zone.center.x;
                let a1 = (half - b) / a + zone.center.x;
                lo = lo.max(a0.min(a1));
                hi = hi.min(a0.max(a1));
            }
        }
        if lo > hi {
            continue;
        }
        let first = ((lo + 1.) * width as f32 * 0.5 - 0.5).ceil().clamp(0., width as f32) as usize;
        let last = (((hi + 1.) * width as f32 * 0.5 - 0.5).floor() + 1.).clamp(0., width as f32) as usize;
        write(y, first, last);
    }
}
```

以上覆盖候选的查表、`compose_masks`、逐行 bbox、`source_bounds` 与位移遍历，以及它们依赖的量化/subtract/几何辅助函数。`prepare_geometry`、`render_rings`/`render_gray_rings` 与测试模块属于未改动的既有路径，不在本条核心代码内；正文中的机制说明已经覆盖它们与候选的交互。

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| 查表开关 | `mask_lookup_values` | 默认 `false`；`Masks.lookup_values` |
| 逐行 bbox 开关 | `mask_compose_rows` | 默认 `false`；`Masks.row_bounds` |
| 设置入口 | `research_mask_lookup(enabled, rows)` | 同时写当前 masks 与 `spare_viewport.masks` |
| 构建特性 | `render_lifetime` | 两个开关与四路 const 分派都在该 feature 下 |
| 计时特性 | `render_research` | `research_begin/end` 与 `timings` |
| 生产 Config | 无 | 没有 Settings/UI 入口 |
| 表大小 | 65,536 × 8 bytes = 524,288 bytes（512KiB） | `Subtract { f32, u8 }` 按 8 字节对齐 |
| 表初始化 | `OnceLock`，`prepare()` 强制 | 首次查表前构建 |
| 穷举域 | 256 × 256 × 256 = 16,777,216 | red × green × normal |

### 6. 完整测量数据（表格）

`mask-values-lookup-confirmed` 完成 12 项 ABBA / 9 项严格末帧 RGBA；`mask-compose-rows-stepped` 独立完成 18 项 ABC/CBA / 15 项严格 RGBA。后者同 cohort 的每轮统计中位数如下，不能与前者混池：

| 场景 / 指标 | 原求值 | lookup | lookup + rows | 相对原求值 |
|---|---:|---:|---:|---:|
| Petrichor 效果窗口 mask mean | 7.610ms | 6.896ms | 6.757ms | −0.853ms（−11.2%） |
| Petrichor 效果窗口 compose mean | 3.083ms | 2.336ms | 2.202ms | −0.881ms（−28.6%） |
| Petrichor 效果窗口持续 FPS | 58.237 | 58.254 | 58.338 | +0.101（+0.17%） |
| Petrichor 效果窗口 frame p95 | 20.995ms | 21.026ms | 20.985ms | −0.010ms |
| Petrichor 正常窗口 mask mean | 7.609ms | 6.862ms | 6.741ms | −0.868ms |
| Petrichor 正常窗口持续 FPS | 59.602 | 59.714 | 59.727 | +0.125（+0.21%） |
| Happy ending 持续 FPS | 60.041 | 60.025 | 60.018 | −0.023（无 mask 工作） |

质量、契约与后端：

| 项目 | 数量/值 | 结果 |
|---|---:|---|
| lookup ABBA | 12 项 | 完成 |
| lookup 严格 RGBA | 9 项 | 全部相同 |
| lookup+rows ABC/CBA | 18 项 | 完成 |
| lookup+rows 严格 RGBA | 15 项 | 全部相同 |
| `Masks` 单元测试 | 16 项 | 通过（淡入、ready、动态变换、多尺寸、缓存命中、noise-only） |
| R8 穷举 | 16,777,216 组 | host 与真机 aarch64 均通过 |
| 512KiB 表 | 524,288 bytes | 常驻只读 |
| lookup 后端 SHA-256 | `3056c7382152e314c43c982f220f6793f733a68c0f86affbe028733fd2762769` | `mask-values-lookup.so` |
| lookup+rows 后端 SHA-256 | `1168c1a6af3ad37aa8560c73b058c468f3c82209f807284f8be5245a211dc380` | `mask-compose-rows.so` |

表中的 `compose` 包含在 `mask` 内，不能与 `mask` 相加；`mask` 又包含在 `frame` 内，不能把 0.853ms 当作帧时间下降。`mask` 均值是“无 GL 的 CPU 墙钟”，仍包含 OS 抢占，不是线程 CPU 周期。

### 7. 质量与回归结果

- 所有严格 RGBA 对照相同：lookup 9 项、lookup+rows 15 项。说明查表与逐行 bbox 没有改变 mask 的最终 R8 值、噪声位移、ready/disabled 通道或纹理上传内容。
- 16,777,216 组 R8 输入在 host 与真机 aarch64 都穷举通过。这个穷举覆盖了 `red`、`green`、`normal` 的整个输入域，证明查表算术与独立 oracle 完全一致；它不覆盖多尺寸/多 zone 的几何组合，后者由 `Masks` 的 16 项测试覆盖。
- `Masks` 的 16 项测试包含混合淡入、ready、动态变换、多尺寸、缓存命中与 noise-only 更新；这些是几何/缓存语义的回归，不是性能。
- 质量通过是“可以保留为候选”的前提；本条没有质量失败，失败点在整帧收益与内存代价。
- 没有跨 GPU、没有不同分辨率、没有全曲。Happy ending 没有 mask 工作，不能作为收益样本；它只能证明“无 mask 时不会变慢”。

### 8. 为什么判定为无（逐项对比）

| 判据 | 期望 | 实际 | 结论 |
|---|---|---|---|
| mask 局部时间 | 明显下降 | −0.853ms（−11.2%） | 达成 |
| compose 局部时间 | 明显下降 | −0.881ms（−28.6%） | 达成 |
| 整帧 p95 | 明显下降 | 20.995→20.985ms | 未达成 |
| 持续 FPS | 明显上升 | +0.101（0.17%） | 噪声 |
| 正常窗口 FPS | 明显上升 | +0.125（0.21%） | 噪声 |
| Happy ending | 不应变差 | 60.041→60.018 | 噪声级下降 |
| 内存代价 | 可忽略 | +512KiB 常驻 | 有代价 |
| 适用范围 | 多谱面/多 GPU | 只有 Petrichor/Mali-T880 | 未证明 |

进一步解释：

1. **0.85ms 只是帧预算的 5.1%。** 16.67ms 的帧预算里，mask 求值占 7.6ms；把它降到 6.76ms 是明显的局部改善，但整帧还包含 chart、粒子、HUD、字形、提交/驱动等待。局部下降 0.85ms 最多只能把帧时间下降 0.85ms；实际 p95 只降 0.01ms，说明这点 CPU 时间被其他阶段的重叠/等待吸收了。
2. **CPU mask 与 GPU/驱动可能重叠。** mask 求值在 CPU 上，纹理上传和 block overlay 在 GPU/驱动上；如果帧的尾延迟由上传/采样等待决定，减少 CPU 算术不会改变尾延迟。这正是“局部微正、整帧无感”的典型形态。
3. **+0.101 FPS 在漂移以内。** 58.237→58.338 的相对变化 0.17%；同 cohort 的对照自身在两轮之间也有类似量级的波动。不能把单次方向当作收益。
4. **512KiB 表不是免费。** 该表常驻只读，会占用 L2/L3 与缓存带宽；mask 本身还有多层 `Vec<u8>`/`Vec<u64>` 工作集。对 mask 很重的谱面可能值得，但对没有 mask 的谱面（Happy）是纯负担。当前没有按谱面启用/释放表的机制。
5. **只覆盖 Petrichor。** 需要更多 mask 密集谱面、不同分辨率、不同 GPU 才能证明普遍性；当前证据只支持“这张谱面在这个设备上局部变快”。
6. **逐行 bbox 的收益更稳，但仍不足以单独推广。** 它不增加大表，只改变 bbox 维护方式；如果未来只保留一项，row bounds 比 lookup 更值得保留。但当前整帧仍无感，不能单独作为生产优化。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：commit `4f046544` |
| 默认 | `mask_lookup_values=false`、`mask_compose_rows=false` |
| 普通路径 | 未接入 |
| 主线是否已实施 | 否（main）；未进入 |
| 研究后端 | `mask-values-lookup.so` SHA `3056c7382152e314c43c982f220f6793f733a68c0f86affbe028733fd2762769`；`mask-compose-rows.so` SHA `1168c1a6af3ad37aa8560c73b058c468f3c82209f807284f8be5245a211dc380` |
| 质量/契约 | 9 + 15 组严格 RGBA 相同；16 项 Masks 测试；16,777,216 组穷举 |
| 性能 | 局部 mask −0.853ms、compose −0.881ms；整帧 p95 −0.01ms、FPS +0.17% |
| 后续 | 不进入普通路径；保留为“局部 CPU 收益不等于整帧收益”的样本 |

### 10. 可挽救的部分与后续条件

可挽救的部分按优先级排列：

1. **逐行 bbox（row bounds）优先保留。** 它没有 512KiB 表，逻辑简单，严格等价；如果未来某个谱面的 compose 占比更高，它可以作为低风险候选。但仍需整帧收益证明。
2. **查表按需启用/释放。** 只有检测到当前谱面有大量 block mask 工作时才构建表；离开该谱面/低内存时释放。当前 `OnceLock` 一旦构建就常驻，无法按谱面回收。
3. **缩小表。** 如果只需要 `product` 与 `green`，可以尝试更紧凑的表示（例如 `f32` 产品表 + 单独 64KiB green 表），把 512KiB 降到 320KiB；但必须保持逐位一致，不能用近似。
4. **与其他 mask 优化组合验收。** 单独的 0.85ms 不足以改变整帧；如果与纹理上传/快照/几何裁剪组合后能把 p95 拉低，才值得保留。组合验收必须同 cohort，不能把不同批次的收益相加。
5. **跨谱面/跨 GPU。** 至少覆盖一张 mask 更密、分辨率不同、GPU 不同的谱面；Happy 这种无 mask 谱面应作为“无收益/无回归”的对照。
6. **以 p95/p99 与 FPS 共同验收。** 只报 mask 均值会把“局部微正”误读成“整帧优化”；本条的核心教训就是阶段计时必须与整帧计时同时报告。

### 11. 来源与可追溯性（仅溯源）

- 源码：`prpr/src/core/block_mask_values.rs`、`prpr/src/core/block_mask.rs`、`prpr/src/core/block_shader.rs`，commit `4f046544`。
- 开关符号：`Masks.lookup_values`、`Masks.row_bounds`、`set_lookup_values`、`research_mask_lookup`、`compose_masks<LOOKUP, ROW_BOUNDS>`。
- 后端：`本地证据/backends/mask-values-lookup.so.json`（SHA `3056c7382152e314c43c982f220f6793f733a68c0f86affbe028733fd2762769`）、`本地证据/backends/mask-compose-rows.so.json`（SHA `1168c1a6af3ad37aa8560c73b058c468f3c82209f807284f8be5245a211dc380`）。
- 数据：`本地证据/mask-values-lookup-confirmed`、`本地证据/mask-compose-rows-stepped`、`本地证据/mask-values-lookup-matrix`、`本地证据/mask-compose-rows-matrix`。
- 文档：`docs/development/rendering-research.md` 第 427–445 行；`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 75、79 行。
- 原始段落（`本地证据/AGENTS.md` 历史快照，第 79 行）：

> 本轮CPU候选最终证据：`mask-values-lookup-confirmed`12项ABBA/9项严格RGBA全同；`mask-compose-rows-stepped`18项ABC/CBA/15项严格RGBA全同。有限R8域预计算保持原float32乘积及量化，16,777,216组输入在host和真机ARM均穷举通过；实际Masks的16项淡入/ready/动画/尺寸/缓存契约通过。最终同cohort的Petrichor效果窗口mask求值mean7.610→6.757ms、compose3.083→2.202ms，持续FPS58.237→58.338，没有可靠整帧收益；正常窗口mask7.609→6.741ms，FPS59.602→59.727，Happy约60无mask工作。`mask_lookup_values`/`mask_compose_rows`仍默认false，不接入普通路径，不把CPU余量当目标完成。细分数据表明gray rings实际0次；二值/几何/位移为此谱面执行路径。

- `本地证据/low-quality-normal/README.md` 没有 N-08 的对应条目；该实验属于参考实现，README 只覆盖低画质 gameplay 的字形/GPU IR 研究。此处不把 README 的其他表格混入本条目。

## N-09 mask texture ring / viewport 缓存 / keep_resolved_pass

### 1. 一句话结论与判定（无 + 理由）

**判定：无优化。** 三个候选都完成了 ABBA 或像素对照，且没有画质失败，但都没有整帧收益：

- **mask 纹理 ring**：8 项 ABBA / 6 项严格 RGBA 相同；密集效果窗口 one/three 持续 FPS 中位 58.1686/58.0742，p95 21.0333/21.1961ms。三纹理版本没有更快，反而略低。
- **viewport 缓存**：8 项 ABBA / 6 项严格 RGBA 相同；正常窗口 59.718→59.722，RAINBOW 58.234→58.264，p95 18.770→18.694ms。实际 disabled/active 两层都是 1920×1200，只用了 1 对缓存；尺寸交替的猜想被排除。
- **keep_resolved_pass**：初筛无明显整帧收益；保留 resolve 后单采样 pass 只改变 FBO 绑定，不减少渲染工作，普通路径仍为 false。

三者都属于“减少资源创建/绑定”的机制，不是“减少像素/绘制”的机制。当帧时间由 mask 求值、纹理上传、快照提交或驱动等待支配时，减少一次分配或一次 FBO 重绑不足以改变整帧。

### 2. 问题与初衷

block 效果每帧需要两张 mask 纹理（`effect` 与 `aux`），并在 disabled/active 两层之间复用。原实现只有一组纹理：尺寸变化时 `from_rgba8` 新建、旧纹理 `delete`；同一帧内 disabled 与 active 共用同一组。这样做有三个潜在问题：

1. 尺寸变化/重建时，旧纹理可能仍被已排队命令采样，删除/新建会引入绑定缓存与在途引用的风险；
2. 每帧 `update` 整张 mask 纹理，若上一帧的采样还没完成，驱动可能在写入时同步；
3. viewport 在 disabled/active 或 resize 之间交替时，mask 的几何缓存、动画缓存与上传版本会失效。

对应的三个想法是：

- **纹理 ring**：保留 3 个同尺寸/同格式的普通 mask 纹理版本，每帧轮换一个，给在途读取留出时间；disabled/active 共用同一版本，不改变画质。
- **viewport 缓存**：保留 2 个完整 viewport 状态（masks、effect/aux、uploaded revision、support），当 viewport 在两者之间交替时直接交换，不重算。
- **keep_resolved_pass**：MSAA 最终 resolve 后，把单采样输出 pass 保持在绑定状态，跨 flush 不切回默认 framebuffer，减少 pass 重绑；snapshot/capture/target 变化仍关闭 pass。

三者的共同假设是“资源生命周期/绑定是当前帧的主要成本”。测量结果否定了这个假设。

### 3. 实现思路与具体机制

**纹理 ring。** `FrameTextures` 除当前 `effect/aux/uploaded_masks/uploaded_aux` 外，还有 `spare_masks: [MaskTextures; 2]`。`draw_disabled_zones` 每个 chart 帧执行一次：

```text
index = spare_mask_index
spare_mask_index = (index + 1) % 2
swap(current.effect, spare_masks[index].effect)
swap(current.aux, spare_masks[index].aux)
swap(current.uploaded_masks, spare_masks[index].uploaded_masks)
swap(current.uploaded_aux, spare_masks[index].uploaded_aux)
```

当前 + 两个 spare 共 3 个版本。轮换发生在 disabled 层绘制之前，active 层随后使用同一组，因此同一帧的两层不会看到不同版本。上传逻辑仍在 `draw_layer_at` 里：如果尺寸变了就 `from_rgba8` 新建，否则按 `uploaded_masks != revision` 调 `update`。ring 只改变“当前使用哪一组已分配的纹理”，不改变上传内容、上传频率或 shader。它没有插入 fence；能否避开在途读取取决于驱动/队列的自然延迟，而不是显式保证。

**viewport 缓存。** `FrameTextures` 有 `spare_viewport: MaskViewport`，内部包含 `masks`、`textures` 与（`render_bounds` 下）`support`。`draw_layer_at` 计算当前 viewport 的 `(width, height)` 后：

- `matches_viewport(width, height, aspect)` 比较 `last_dim` 与 `((width/8).max(1)*2, (height/8).max(1)*2, aspect)`；
- 若 `cache_mask_viewports` 为真且不匹配，则把当前 `masks/effect/aux/uploaded_*/support` 与 `spare_viewport` 整体交换；
- 交换后当前状态就是另一套 viewport 的完整缓存，可直接命中 `last_dim/last_zones/last_time` 的早退，或复用已上传纹理。

它最多保留 2 套 viewport；如果 viewport 在更多尺寸之间轮换，仍会失效。测试谱面的 disabled/active 两层都是 1920×1200，所以只有 1 对缓存，交换从未发生。

**keep_resolved_pass。** `Resource.keep_resolved_pass` 默认 false。chart 渲染在 sample_count>1 时：

1. `flush`；
2. `chart_target.resolve_final()`；
3. 若 `keep_resolved_pass`，调用 `retain_render_pass_on_flush(Some(target.output().render_pass))`。

effect 渲染在 swap 输出后也按同样条件保留 `target.output().render_pass`。macroquad 侧的 `retain_render_pass_on_flush` 只是把 `retained_render_pass` 设为该 pass；后续 `flush` 不再自动切回默认 framebuffer，snapshot/capture/target 变化仍会关闭 pass。它不改变 sample 数、resolve 内容、颜色格式或绘制顺序，只改变“flush 后默认绑定哪个 FBO”。

### 4. 文件、符号与完整核心代码

源码快照：参考实现，commit `4f046544`；macroquad 侧 `retain_render_pass_on_flush` 来自 参考实现的 commit `dd5fb4a`（Add opt-in offscreen framebuffer retention across flushes）。文件与符号：

- `prpr/src/core/block_shader.rs`：`FrameTextures`、`MaskTextures`、`MaskViewport`、`draw_disabled_zones` 的 ring 轮换、`draw_layer_at` 的 viewport 交换、`research_snapshot_info`；
- `prpr/src/core/block_mask.rs`：`Masks::matches_viewport`、`Masks::render_displaced` 的 `last_dim/last_zones` 早退；
- `prpr/src/core/chart.rs`：`keep_resolved_pass` 与 `retain_render_pass_on_flush`；
- `prpr/src/core/effect.rs`：effect 输出 pass 的保留；
- `prpr/src/core/resource.rs`：三个默认 false 的字段；
- macroquad `src/quad_gl.rs`：`retain_render_pass_on_flush`、`retained_render_pass`。

ring 与 viewport 的结构体定义（原文第 144–185 行）：

```rust
struct FrameTextures {
    #[cfg(any(feature = "render_bounds", not(feature = "render_research")))]
    support: support::Cache,
    uploaded_masks: Option<u64>,
    uploaded_aux: Option<u64>,
    masks: mask::Masks,
    effect: Option<Texture2D>,
    aux: Option<Texture2D>,
    scene: Option<Texture2D>,
    scene_pass: Option<miniquad::RenderPass>,
    scene_blit_rejected_for: Option<(u32, u32)>,
    scene_blit: bool,
    scene_shader_rejected: bool,
    #[cfg(feature = "render_lifetime")]
    source_image: Option<Image>,
    #[cfg(feature = "render_lifetime")]
    spare_masks: [MaskTextures; 2],
    #[cfg(feature = "render_lifetime")]
    spare_mask_index: usize,
    #[cfg(feature = "render_lifetime")]
    spare_viewport: MaskViewport,
    #[cfg(feature = "render_lifetime")]
    viewport_sizes: [Vec<[usize; 2]>; 2],
    #[cfg(feature = "render_lifetime")]
    scene_copy: Option<SceneCopy>,
    touch: touch::TouchMask,
    clock: Option<f32>,
}

#[cfg(feature = "render_lifetime")]
#[derive(Default)]
struct MaskTextures {
    effect: Option<Texture2D>,
    aux: Option<Texture2D>,
    uploaded_masks: Option<u64>,
    uploaded_aux: Option<u64>,
}

#[cfg(feature = "render_lifetime")]
#[derive(Default)]
struct MaskViewport {
    masks: mask::Masks,
    textures: MaskTextures,
    #[cfg(feature = "render_bounds")]
    support: support::Cache,
}
```

ring 的每帧轮换与 disabled 层入口（原文第 459–480 行）：

```rust
pub fn draw_disabled_zones(res: &mut Resource, aspect: f32, zones: &[Zone]) {
    // Unity's _Time is constant for all passes in a frame. Chart invokes this
    // before notes even when the disabled layer happens to be empty.
    let time = shader_time();
    FRAME.with(|frame| {
        let mut frame = frame.borrow_mut();
        frame.clock = Some(time);
        #[cfg(feature = "render_lifetime")]
        if res.buffer_mask_textures {
            // Three ordinary texture versions; no animation/quality changes.
            // Rotate once per chart frame, so disabled/active see the same pair.
            let index = frame.spare_mask_index;
            frame.spare_mask_index = (index + 1) % 2;
            let FrameTextures { effect, aux, uploaded_masks, uploaded_aux, spare_masks, .. } = &mut *frame;
            let spare = &mut spare_masks[index];
            std::mem::swap(effect, &mut spare.effect);
            std::mem::swap(aux, &mut spare.aux);
            std::mem::swap(uploaded_masks, &mut spare.uploaded_masks);
            std::mem::swap(uploaded_aux, &mut spare.uploaded_aux);
        }
    });
    draw_layer_at(res, aspect, zones, time, true, &[]);
```

viewport 缓存交换（原文第 560–580 行）：

```rust
            let sizes = &mut frame.viewport_sizes[usize::from(!disabled)];
            if !sizes.contains(&[width, height]) { sizes.push([width, height]); }
            if res.cache_mask_viewports && !frame.masks.matches_viewport(width, height, aspect) {
                // Retain the two alternating viewport states, including their
                // geometry, animation caches and matching uploaded texture data.
                // Identical viewports share the same cache as the original path.
                let FrameTextures { masks, effect, aux, uploaded_masks, uploaded_aux, spare_viewport, .. } = &mut *frame;
                std::mem::swap(masks, &mut spare_viewport.masks);
                std::mem::swap(effect, &mut spare_viewport.textures.effect);
                std::mem::swap(aux, &mut spare_viewport.textures.aux);
                std::mem::swap(uploaded_masks, &mut spare_viewport.textures.uploaded_masks);
                std::mem::swap(uploaded_aux, &mut spare_viewport.textures.uploaded_aux);
                #[cfg(feature = "render_bounds")]
                {
                    let FrameTextures { support, spare_viewport, .. } = &mut *frame;
                    std::mem::swap(support, &mut spare_viewport.support);
                }
            }
        }
        #[cfg(feature = "render_research")]
        let mask_start = research_begin();
```

chart 侧的 keep_resolved_pass（原文第 215–222 行）：

```rust
            if res.config.sample_count > 1 {
                unsafe { get_internal_gl() }.flush();
                if let Some(target) = &res.chart_target {
                    target.resolve_final();
                }
                #[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
                unsafe { get_internal_gl().quad_gl.retain_render_pass_on_flush(
                    res.chart_target.as_ref().filter(|_| res.keep_resolved_pass).map(|target| target.output().render_pass)
                ); }
            }
            if !res.no_effect {
                let render = |res: &mut Resource| {
                    for effect in &self.extra.effects {
                        effect.render(res);
                    }
                };
```

effect 侧的 keep_resolved_pass（原文第 195–201 行）：

```rust
        #[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
        unsafe { gl.quad_gl.retain_render_pass_on_flush(
            res.keep_resolved_pass.then(|| target.output().render_pass)
        ); }
        let tex = target.old().texture;
        self.material.set_texture("screenTexture", tex);
        let screen_dim = vec2(tex.width(), tex.height());
```

macroquad 侧的 pass 保留实现（`dd5fb4a`，`src/quad_gl.rs`）：

```rust
    /// Keep this offscreen framebuffer bound after a clear or flush. None
    /// restores the ordinary flush contract. Snapshot, capture and target
    /// changes still close their passes; commands and projections are unchanged.
    ///
    /// # Safety
    /// Raw GL consumers must account for the retained framebuffer and buffer
    /// bindings. The owner must end/replace the framebuffer before observing
    /// its texture, and clear this option before releasing the render pass.
    /// This does not extend attachment lifetimes or invalidate any contents.
    pub unsafe fn retain_render_pass_on_flush(&mut self, pass: Option<RenderPass>) {
        self.retained_render_pass = pass;
    }
```

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| 纹理 ring | `buffer_mask_textures=false` | `FrameTextures.spare_masks` 只在 `render_lifetime` 下存在 |
| viewport 缓存 | `cache_mask_viewports=false` | 与 ring 互斥；`spare_viewport` |
| keep_resolved_pass | `keep_resolved_pass=false` | `Resource` 字段；chart/effect 两处使用 |
| 构建特性 | `render_lifetime` | 三个机制的主开关/字段 |
| 计时特性 | `render_research` | `research_snapshot_info`、mask 计时 |
| 生产 Config | 无 | 没有 Settings/UI 入口 |
| 纹理版本数 | 3（当前 + 2 spare） | 同尺寸/同格式/同过滤 |
| viewport 槽数 | 2（当前 + 1 spare） | 只保留两套完整状态 |
| 普通默认 | 全部 false | `ordinary` 不启用 |

### 6. 完整测量数据（表格）

**纹理 ring**（密集效果窗口，2 轮/策略，表中为每轮分位数中位数）：

| 策略 | 持续 FPS | frame p95 | frame p99 | 实际纹理 pairs | 严格 RGBA |
|---|---:|---:|---:|---:|---|
| one-texture（对照） | 58.1686 | 21.0333ms | 21.5103ms | 1 | 6/6 相同 |
| three-textures（ring） | 58.0742 | 21.1961ms | 21.7066ms | 3 | 6/6 相同 |

ring 的 8 项 ABBA 覆盖效果窗口与早段；早段没有 mask 纹理，不能用来证明 mask 回归。三纹理版本在效果窗口的 FPS、p95、p99 都略差，差异虽小，但方向与“减少分配会更快”相反。

**viewport 缓存**（2 轮/策略）：

| 场景 / 策略 | 持续 FPS | frame p95 | frame p99 | 实际缓存 pairs |
|---|---:|---:|---:|---:|
| 正常窗口 one-cache | 59.718 | 18.7700ms | 19.4490ms | 1 |
| 正常窗口 two-viewports | 59.722 | 18.6938ms | 19.2316ms | 1 |
| RAINBOW one-cache | 58.234 | 21.0908ms | 21.6200ms | 1 |
| RAINBOW two-viewports | 58.264 | 21.0054ms | 21.4584ms | 1 |

8 项 ABBA / 6 项严格 RGBA 相同。关键事实不是 FPS 的 ±0.03，而是 `mask_viewports_disabled_active` 显示 disabled/active 两层都是 1920×1200，`mask_viewport_pairs=1`。也就是说，缓存从未发生交换；这个候选在测试谱面上等价于“没有缓存”。

**keep_resolved_pass**（`blit-fallback-screen` 单轮诊断，不是性能 ABBA）：

| 策略 | frame p50 | frame p95 | frame p99 | render mean |
|---|---:|---:|---:|---:|
| legacy | 16.7908ms | 20.9965ms | 21.4510ms | 11.1382ms |
| checked | 16.7409ms | 21.0684ms | 21.4354ms | 11.6670ms |
| retained（keep_resolved_pass） | 16.7778ms | 20.9671ms | 21.5102ms | 11.3925ms |
| blit-denied | 16.7652ms | 21.0936ms | 21.4992ms | 11.7174ms |

四组都在同一量级，retained 没有比 checked/denied 更好；该组只有单轮，不能当作稳定收益，也不能当作稳定回归。它只能支持“初筛无明显整帧收益”的结论。

质量与契约：

| 项目 | 数量 | 结果 |
|---|---:|---|
| ring ABBA | 8 项 | 完成 |
| ring 严格 RGBA | 6 项 | 全部相同 |
| viewport ABBA | 8 项 | 完成 |
| viewport 严格 RGBA | 6 项 | 全部相同 |
| keep_resolved 像素 | 初筛 | 无明显整帧收益；未进入普通路径 |
| ring 后端 SHA-256 | `36d33252e351c9f5b753294d6a2a29f37f758ff9b79898def57f25de771d0f47` | `mask-texture-ring.so` |
| viewport 后端 SHA-256 | `af0c37ed989075dad57a8e3fcca3971ef447e45585186c8694bb25e0002019ee` | `mask-viewport-cache.so` |

### 7. 质量与回归结果

- ring 与 viewport 各有 6 项严格 RGBA 相同；两者都没有改变 mask 纹理内容、上传矩形、shader 采样或绘制顺序。ring 只是换用另一组已分配纹理；viewport 只是换用另一套缓存状态。
- ring 的“每帧轮换一次、disabled/active 共用同一版本”是正确的一致性设计；如果 disabled 与 active 各自轮换，同一帧两层会看到不同 revision，画质会变。当前实现避免了这个问题。
- viewport 的“整体交换 masks/textures/support”也是正确的：只换纹理不换 masks 会读到错误尺寸的 CPU 缓冲。当前实现把三者一起换，质量通过。
- keep_resolved_pass 只有初筛，没有同等级别的严格 RGBA 全量对照；它没有进入普通路径，也没有被当作已验收的画质候选。
- 三者都没有跨 GPU、全曲、不同分辨率或不同 aspect 的覆盖。ring 的早段没有 mask 纹理，不能证明 mask 回归；viewport 只覆盖一个 1920×1200 的窗口；keep_resolved 只覆盖一个 Petrichor 效果窗口。

### 8. 为什么判定为无（逐项对比）

| 判据 | ring | viewport | keep_resolved |
|---|---|---|---|
| 目标机制 | 减少纹理创建/删除 | 复用 viewport 缓存 | 减少 FBO 重绑 |
| 机制是否触发 | 是（1→3 pairs） | 否（始终 1 pair） | 是（保留 pass） |
| 局部时间是否下降 | 未测到 | 未测到 | 未测到 |
| 整帧 FPS | 58.1686→58.0742 | 59.718→59.722 / 58.234→58.264 | 同量级 |
| p95 | 21.0333→21.1961（差） | 18.770→18.694（微好） | 20.997→20.967（微好） |
| 质量 | 6/6 RGBA | 6/6 RGBA | 初筛 |
| 结论 | 无收益，略负 | 无收益（未触发） | 无收益 |

进一步解释：

1. **ring 解决的不是热点。** mask 纹理的创建/删除只在尺寸变化时发生；稳定游玩时每帧是 `update` 整张纹理，不是 `from_rgba8`/`delete`。ring 把“分配”变成“轮换”，但没有减少 `update` 的字节数、没有减少 mask 求值、没有减少 draw。它增加了 2 组常驻纹理（每组 effect+aux），内存代价明确，收益不明确。
2. **ring 没有同步保证。** 轮换 3 个版本只能给驱动“更多时间”，没有 fence 证明旧版本已经不被采样。若帧率下降或驱动延迟增大，仍可能在第 3 帧复用仍在读的纹理；届时驱动会在写入时同步，反而把等待放到 update 内。质量通过不代表没有这种风险。
3. **viewport 缓存没有触发。** 两个层的 viewport 都是 1920×1200，`matches_viewport` 始终为真，`spare_viewport` 从未交换。它测到的 ±0.03 FPS 只是运行噪声；这个候选在当前谱面上根本没有执行到“缓存命中”的路径。要证明它有价值，必须构造 viewport 交替（例如 disabled 与 active 使用不同尺寸，或 resize/PiP），并记录 `mask_viewport_pairs=2` 与交换次数。
4. **keep_resolved_pass 只省一次绑定。** 它保留的是 resolve 后的单采样 pass，避免 flush 后切回默认 FBO 再重新进入；但 chart/effect 的绘制、采样、快照、resolve 内容都没有变。若帧时间由 mask 求值/上传/驱动等待支配，省一次 FBO 绑定不会改变 p95。表中 retained 与 checked/denied 的差异都在单轮噪声内。
5. **三者都增加了状态复杂度。** ring 需要维护 3 组上传 revision；viewport 需要维护 2 套 masks/textures/support；keep_resolved 需要保证 snapshot/capture/target 变化时正确关闭 pass。没有整帧收益时，这些复杂度是纯负担。

### 9. 主线实施状态与参考实现状态

| 项 | ring | viewport | keep_resolved |
|---|---|---|---|
| 参考实现 | 已落地（未合入 main）：`4f046544` | 已落地（未合入 main）：`4f046544` | 已落地（未合入 main）：`4f046544` + macroquad `dd5fb4a` |
| 默认 | `buffer_mask_textures=false` | `cache_mask_viewports=false` | `keep_resolved_pass=false` |
| 普通路径 | 未接入 | 未接入 | 未接入 |
| 主线是否已实施 | 否（main） | 否（main） | 否（main） |
| 研究后端 | `36d33252e351c9f5b753294d6a2a29f37f758ff9b79898def57f25de771d0f47` | `af0c37ed989075dad57a8e3fcca3971ef447e45585186c8694bb25e0002019ee` | `blit-fallback-screen` cohort |
| 质量/契约 | 6/6 RGBA | 6/6 RGBA | 初筛 |
| 后续 | 不推进 | 不推进 | 不推进 |

### 10. 可挽救的部分与后续条件

- **ring**：只有在“纹理创建/删除”被实测为帧内显著成本时才值得保留；并且需要 fence 或明确的“版本已不再被采样”证明。若保留，应按实际 mask 内存预算限制版本数（2 而不是 3），并记录每次轮换的 `reused/created/deleted` 计数。当前没有这些证据。
- **viewport 缓存**：只有在 viewport 真正交替（disabled/active 不同尺寸、resize、画中画、分屏）时才有意义。后续应先记录 `viewport_sizes` 与交换次数；如果 `mask_viewport_pairs` 始终为 1，直接删除该候选。若保留，需要覆盖 2 套以上的状态，或在超过 2 套时明确回退。
- **keep_resolved_pass**：只有在“flush 后切回默认 FBO / 重新进入 pass”被实测为显著成本时才值得保留；并且必须覆盖 snapshot、capture、target 变化、MSAA resolve、context 重建。当前没有这种证据；它更适合作为 framegraph 的一部分，而不是独立开关。
- **共同条件**：任何一项要进入普通路径，必须给出整帧 p95/p99 与持续 FPS 的可重复改善，而不是只给“分配次数减少”“缓存命中”或“pass 保留”。本条的三项都没有做到。

### 11. 来源与可追溯性（仅溯源）

- 源码：`prpr/src/core/block_shader.rs`、`prpr/src/core/block_mask.rs`、`prpr/src/core/chart.rs`、`prpr/src/core/effect.rs`、`prpr/src/core/resource.rs`，commit `4f046544`。
- macroquad：参考实现 commit `dd5fb4a`（`retain_render_pass_on_flush`），归档仓库参考实现。
- 开关符号：`buffer_mask_textures`、`cache_mask_viewports`、`keep_resolved_pass`、`spare_masks`、`spare_viewport`、`retain_render_pass_on_flush`。
- 后端：`本地证据/backends/mask-texture-ring.so.json`（SHA `36d33252e351c9f5b753294d6a2a29f37f758ff9b79898def57f25de771d0f47`）、`本地证据/backends/mask-viewport-cache.so.json`（SHA `af0c37ed989075dad57a8e3fcca3971ef447e45585186c8694bb25e0002019ee`）。
- 数据：`本地证据/mask-texture-ring-matrix`、`本地证据/mask-viewport-cache-matrix`、`本地证据/blit-fallback-screen`、`本地证据/mask-substage-timing`。
- 文档：`docs/development/rendering-research.md` 第 421、425 行；`docs/development/render-research-harness.md` 第 158、160 行；`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 85、91 行。
- 原始段落一（`本地证据/AGENTS.md` 历史快照，第 91 行）：

> 当前轮执行：mini000ea930/macro ce7949ed pin已接入，普通Androidcheck通过；shader回退像素不等价，仍仅研究（highp恢复），普通legacy源格式匹配fallback未严格验收。mask三纹理8项ABBA/6RGBA全同但密集FPS58.1686→58.0742、p9521.0333→21.1961无可靠收益，默认false。细分block-stage-timing两组正常/RAINBOW实际样本360/native358有效，无GL mask求值墙钟mean7.62/7.68ms，upload仅0.34/0.40，snapshot0.12/0.13；effects截图与旧control严格相同，纯GPU仍未取得。viewport候选现已完成8项ABBA/6项严格RGBA全同，但disabled/active实际尺寸均1920×1200、仅1对纹理，无可靠收益；尺寸交替猜想已排除，cache_mask_viewports仍false。证据本地证据/mask-viewport-cache-matrix，so SHAaf0c37ed989075dad57a8e3fcca3971ef447e45585186c8694bb25e0002019ee。进一步细分已完成两项真实360帧/native358运行，末帧均与旧control严格RGBA相同：geometry mean4.686ms，displacement2.35–2.41ms，binary rings0.56ms，gray0ms/0次。累计hit/geometry/binary/gray为92/382/410/0；同frame两层缓存并未普遍失效。记录本地证据/mask-substage-timing。mask_lookup_values及mask_compose_rows两项CPU候选现已完成独立12/9及18/15严格像素对照，完整结果与收益限制见本节最上方最新继续点；均默认false。初轮及第二批ADB中断数据保留并排除，不混池。完整目标/真实UI/尾段/结算仍active。

- 原始段落二（同文件第 85 行）：

> 长窗口新证据：Exoplanetary同原版ZIP/MSAA2实际4/RGB8/1920×1200，正常音轨/autoplay，chart2.0469–162.1657秒9600帧，应用59.9492fps；系统7191呈现/119.9568秒59.9383fps、p95/p99/max16.7311/16.7622ms、1个>1.5refresh，117ring重叠。9600 native严格匹配、9598非I/O可用。`本地证据/exoplanetary-realtime-long`及索引固定，后端原production-effect-pairs.so。未覆盖尾段/结果/真实UI、非原版完整ABBA，目标仍未完成。mask_texture_ring后续8项/6RGBA通过但无可靠收益，默认false，未接入普通路径。

- `本地证据/low-quality-normal/README.md` 没有 N-09 的对应条目；该实验属于参考实现，不把 README 的低画质表格混入本条目。

## N-10 snapshot blit / shader copy 回退

### 1. 一句话结论与判定（负 + 理由）

**判定：负优化（严格像素失败）。** 当 framebuffer blit 被能力否定或显式 `deny_snapshot_blit` 关闭时，fallback 试图用“全尺寸 CopyTexSubImage”或“shader 全屏复制”替代原来的 320×200 线性 blit。两者都没有 GL 错误，源图也逐字节相同，但最终画面严格不等价：

- 全尺寸 full-copy 最终 **254,520 个通道不同，最大差 71**；
- shader copy 的 320×200 中间结果有 **11,309 个通道 ±1**，最终 **98,728 个通道不同**。

这不是“实现还不够好”的性能问题，而是“替代路径改变了中间过滤/量化语义”的画质问题。严格验收失败，fallback 不能作为生产路径；只保留为显式像素诊断。

### 2. 问题与初衷

block overlay 需要一张场景快照。原路径不是直接采样全分辨率场景，而是先把场景 viewport **线性降采样到 1/6**（1920×1200 → 320×200），再让 block shader 用这张小图做 `snapshotSample`。这个 6× 降采样本身是效果语义的一部分：它决定了采样前的预过滤、边缘颜色与量化。

当 `glBlitFramebuffer` 不可用、源不是自有单采样 FBO、或显式 `deny_snapshot_blit=true` 时，原实现不能走 320×200 blit。为了“能力回退”，研究版本提供两条替代路径：

1. **full-copy**：分配与源同尺寸的纹理，用 `glCopyTexSubImage2D` 把整个 viewport 复制过去；shader 仍按原 UV 采样这张全尺寸纹理。
2. **shader copy**：用一个小 shader 把源 viewport 画到 320×200 目标，试图复刻线性 blit。

初衷是“即使没有 blit，也能保持 block 效果可用”。要求是严格像素等价：不能因为 GL 合法、无错误、源图相同，就认为回退成功。

### 3. 实现思路与具体机制

**默认 blit 路径。** `snapshot_source_blit(res, pass)` 依次检查：

- `res.deny_snapshot_blit` 为真则拒绝；
- `supports_framebuffer_blit()` 为假则拒绝；
- `pass` 为空（系统/外来 backbuffer）则拒绝；
- 若 `pass` 是 chart target 的 input pass，则换成 output pass；
- 用 `normalized_single_sample_fbo(pass, None)` 检查实际 FBO：sample≤1、颜色通道 8 位、filter 为 `0x2601`（LINEAR）、FBO complete、无 GL error；
- 结果按 `(pass, allowed)` 缓存到 `snapshot_blit_sources`，避免每帧查询。

允许时 `small_snapshot=true`，`dim=(width/6, height/6)`，`copy_format=RGBA8`，分配 320×200 纹理并创建 `RenderPass`；`copy_scene` 用 `glBlitFramebuffer(viewport → 0,0,320,200, GL_LINEAR)`。这就是原来的语义。

**full-copy 回退。** `can_blit=false` 且没有 shader source 时：

- `small_snapshot=false`；
- `dim=(width, height)`；
- `copy_format` 取源 pass 的实际格式（RGB8 或 RGBA8），避免原 RGB8→RGBA8 的非法复制；
- 分配全尺寸纹理，`scene_pass=None`；
- `copy_scene` 走 `glCopyTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, viewport.x, viewport.y, viewport.w, viewport.h)`。

它复制的是全分辨率像素，之后 block shader 仍按原来的 UV 采样。问题在于：原路径先做 6× 线性降采样，再采样 320×200；full-copy 直接采样 1920×1200，采样点、线性插值核与量化边界都不同。即使复制纹理与源逐字节相同，最终 overlay 也会不同。

**shader copy。** 当 `snapshot_shader_copy=true` 且 `!can_blit` 时，`shader_source` 取 chart target 的 resolved texture，`SceneCopy::new()` 编译一个 GLSL100 shader：

- 顶点：全屏 quad；
- 片元：`precision highp float; uniform highp sampler2D source; uniform vec4 sourceRect; vec2 uv=gl_FragCoord.xy*sourceRect.zw+sourceRect.xy; gl_FragColor=texture2D(source,uv);`
- `sourceRect = [viewport.x/source.width, viewport.y/source.height, viewport.w/(source.width*dim.w), viewport.h/(source.height*dim.h)]`；
- 目标 320×200，viewport/scissor 设为全目标；
- 关闭 dither（`glDisable(0x0BD0)`），因为 blit 不 dither；
- 若支持 separate read/draw bindings（blit 或 ES3），分别恢复 `GL_READ_FRAMEBUFFER` / `GL_DRAW_FRAMEBUFFER`；否则恢复 `GL_FRAMEBUFFER`。

这个 shader 每个目标像素只做一次双线性采样，而 6× 降采样 blit 通常使用盒式/多 tap 过滤。两者在纹理内部区域可能接近，但在边缘、半透明、颜色突变处会出现 ±1 的差异；这些 ±1 再经过 block shader 的 mask/混合/量化，放大成最终 98,728 个通道不同。

### 4. 文件、符号与完整核心代码

源码快照：参考实现，commit `4f046544`。文件与符号：

- `prpr/src/core/block_shader.rs`：`snapshot_source_blit`、`normalized_single_sample_fbo`、`draw_layer_at` 的 `can_blit/shader_source/small_snapshot/copy_format/dim` 分支、`SceneCopy`、`copy_scene`；
- `prpr/src/core/resource.rs`：`deny_snapshot_blit`、`snapshot_blit_sources`、`capture_snapshot_source`、`snapshot_shader_copy`；
- `prpr/src/core/block_shader.rs`：`research_snapshot_info` 输出 `strategy`、`source_accepted`、`destination_rejected`、`shader_rejected`、`format`；
- 构建特性：`render_lifetime`（fallback 与 shader）、`render_research`（诊断）。

`draw_layer_at` 中快照分支的完整核心段（原文第 695–860 行）：

```rust
        #[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
        let can_blit = !disabled && snapshot_source_blit(res, pass)
            && frame.scene_blit_rejected_for != Some((width as u32, height as u32));
        // Preserve historical research profiles, which predate the capability API.
        #[cfg(all(not(feature = "render_lifetime"), feature = "render_research"))]
        let can_blit = unsafe { get_internal_gl() }.quad_context.features().instancing
            && (pass.is_some() || {
                // A multisampled system backbuffer cannot be resized in a blit.
                // Game windows use one sample; keep probe/foreign-camera support.
                let mut samples = 0;
                unsafe {
                    miniquad::gl::glGetIntegerv(0x80A9, &mut samples);
                }
                samples <= 1
            });
        #[cfg(feature = "render_lifetime")]
        let mut shader_source = (res.snapshot_shader_copy && !disabled && !can_blit).then(|| res.chart_target.as_ref().and_then(|target| target.resolved_texture(pass))).flatten();
        #[cfg(feature = "render_lifetime")]
        if frame.scene_shader_rejected { shader_source = None; }
        #[cfg(feature = "render_lifetime")]
        if shader_source.is_some() && frame.scene_copy.is_none() {
            match SceneCopy::new() {
                Ok(copy) => frame.scene_copy = Some(copy),
                Err(error) => {
                    tracing::warn!("Using full scene snapshot after copy shader rejection: {error}");
                    frame.scene_shader_rejected = true;
                    shader_source = None;
                }
            }
        }
        #[cfg(not(feature = "render_lifetime"))]
        let shader_source: Option<miniquad::Texture> = None;
        let small_snapshot = can_blit || shader_source.is_some();
        let copy_format = if small_snapshot { miniquad::TextureFormat::RGBA8 } else {
            pass.map(|pass| pass.texture(unsafe { get_internal_gl() }.quad_context).format).unwrap_or(miniquad::TextureFormat::RGBA8)
        };
        let mut dim = if small_snapshot {
            ((width / 6).max(1) as u32, (height / 6).max(1) as u32)
        } else {
            (width as u32, height as u32)
        };
        if !disabled && frame.scene.is_none_or(|texture| (texture.width() as u32, texture.height() as u32) != dim || texture.raw_miniquad_texture_handle().format != copy_format) {
            let allocate = |dim: (u32, u32), format| miniquad::Texture::new(
                unsafe { get_internal_gl() }.quad_context,
                miniquad::TextureAccess::Static,
                None,
                miniquad::TextureParams {
                    width: dim.0,
                    height: dim.1,
                    format,
                    // Native SceneColor uses a linear /6 camera blit, followed
                    // by point-sampled texel centers in snapshotSample.
                    filter: miniquad::FilterMode::Linear,
                    ..Default::default()
                },
            );
            let mut texture = allocate(dim, copy_format);
            let mut next_pass = small_snapshot.then(|| miniquad::RenderPass::new(unsafe { get_internal_gl() }.quad_context, texture, None));
            #[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
            if can_blit && next_pass.is_some_and(|pass| !normalized_single_sample_fbo(pass, Some(8))) {
                // A failed optional destination must not disable the block effect.
                // Keep the existing full-size CopyTexSubImage compatibility path.
                next_pass.take().unwrap().delete(unsafe { get_internal_gl() }.quad_context);
                frame.scene_blit_rejected_for = Some((width as u32, height as u32));
                dim = (width as u32, height as u32);
                let format = pass.map(|pass| pass.texture(unsafe { get_internal_gl() }.quad_context).format).unwrap_or(miniquad::TextureFormat::RGBA8);
                texture = allocate(dim, format);
            }
            let old = frame.scene.replace(Texture2D::from_miniquad_texture(texture));
            if let Some(pass) = std::mem::replace(&mut frame.scene_pass, next_pass) {
                // RenderPass::delete also owns/deletes its color texture.
                pass.delete(unsafe { get_internal_gl() }.quad_context);
            } else if let Some(old) = old {
                old.delete();
            }
        }
        let scene = frame.scene.unwrap_or(*EMPTY_TEX);
        if !disabled {
            #[cfg(feature = "render_lifetime")]
            if res.capture_snapshot_source {
                if let Some(source) = res.chart_target.as_ref().and_then(|target| target.resolved_texture(pass)) {
                    frame.source_image = Some(snapshot_image(Texture2D::from_miniquad_texture(source)));
                }
            }
            frame.scene_blit = can_blit && frame.scene_pass.is_some();
            #[cfg(feature = "render_lifetime")]
            if let Some(source) = shader_source.filter(|_| !can_blit && frame.scene_pass.is_some()) {
                let destination = frame.scene_pass.unwrap();
                frame.scene_copy.as_mut().unwrap().draw(source, destination, viewport, dim);
            } else { copy_scene(res, pass, viewport, scene, frame.scene_pass); }
            #[cfg(not(feature = "render_lifetime"))]
            copy_scene(res, pass, viewport, scene, frame.scene_pass);
        }
        #[cfg(feature = "render_research")]
        research_end(snapshot_start, 3);
        #[cfg(feature = "render_research")]
        let record_start = research_begin();
        m.set_texture("uDisplaceTex", *DISPLACE_TEX);
        m.set_texture("uSparkTex", *SPARK_TEX);
        m.set_texture("uMasks", frame.effect.unwrap());
        m.set_texture("uScene", scene);
        m.set_texture("uAuxMasks", frame.aux.unwrap());
        m.set_texture("uNoiseTex", *NOISE_TEX);
        m.set_uniform("uUnityTime", vec4(time / 20., time, time * 2., time * 3.));
        m.set_uniform("uView", vec3(width as f32, height as f32, aspect));
        m.set_uniform("_ScreenParams", vec4(width as f32, height as f32, 1. + 1. / width as f32, 1. + 1. / height as f32));
        m.set_uniform("_EffectRT_TexelSize", vec4(1. / effect_dim.0 as f32, 1. / effect_dim.1 as f32, effect_dim.0 as f32, effect_dim.1 as f32));
        m.set_uniform("_TouchPosCount", touches.len().min(10) as i32);
        for (i, (_, uv)) in touches.iter().take(10).enumerate() {
            m.set_uniform(&format!("_TouchPos[{i}]"), *uv * vec2(width as f32 / height as f32, 1.));
        }
        m.set_uniform("_TouchPosShine", (0.63 + 0.37 * ((time * 43.).sin() * 0.5 + 0.5)) * 2.);
        gl_use_material(m);
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
        #[cfg(feature = "render_research")]
        research_end(record_start, 4);
    });
}

#[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
fn snapshot_source_blit(res: &mut Resource, pass: Option<miniquad::RenderPass>) -> bool {
    if res.deny_snapshot_blit || !unsafe { get_internal_gl() }.quad_context.supports_framebuffer_blit() {
        return false;
    }
    // A system/foreign backbuffer has no owned attachment contract. Its original
    // copy path is retained instead of inferring format from a GPU/API name.
    let Some(pass) = pass else { return false; };
    let pass = res.chart_target.as_ref().filter(|target| target.input().render_pass == pass)
        .map_or(pass, |target| target.output().render_pass);
    if let Some((_, allowed)) = res.snapshot_blit_sources.iter().find(|(source, _)| *source == pass) {
        return *allowed;
    }
    let allowed = normalized_single_sample_fbo(pass, None);
    res.snapshot_blit_sources.push((pass, allowed));
    allowed
}

#[cfg(any(feature = "render_lifetime", not(feature = "render_research")))]
fn normalized_single_sample_fbo(pass: miniquad::RenderPass, alpha_bits: Option<i32>) -> bool {
    unsafe {
        use miniquad::gl::*;
        // The capability gate verified every entry used here. These queries run
        // once per owned attachment, outside steady-state submission.
        assert_eq!(glGetError(), GL_NO_ERROR, "GL error before snapshot attachment probe");
        let mut read = 0;
        let mut draw = 0;
        glGetIntegerv(0x8CAA, &mut read);
        glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw);
        let source = pass.gl_internal_id(get_internal_gl().quad_context);
        if source != draw as u32 { glBindFramebuffer(GL_DRAW_FRAMEBUFFER, source); }
        let mut samples = 0;
        glGetIntegerv(0x80A9, &mut samples);
        let mut color = [0; 6];
        for (value, name) in color.iter_mut().zip([0x8212, 0x8213, 0x8214, 0x8215, 0x8211, 0x8210]) {
            glGetFramebufferAttachmentParameteriv(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, name, value);
        }
        let complete = glCheckFramebufferStatus(GL_DRAW_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
        let error = glGetError();
        if source != draw as u32 { glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw as u32); }
        glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
        let allowed = complete && samples <= 1 && color[..3] == [8, 8, 8]
            && alpha_bits.map_or(color[3] == 0 || color[3] == 8, |bits| color[3] == bits)
            && color[4..] == [0x8C17, 0x2601] && error == GL_NO_ERROR;
        if !allowed { tracing::warn!("Using compatible scene snapshot: samples={samples}, color={color:?}, complete={complete}, error={error:#x}"); }
        allowed
    }
}
```

`SceneCopy` 的 shader、构造与绘制（原文第 955–1025 行）：

```rust
struct SceneCopy {
    pipeline: miniquad::Pipeline,
    bindings: miniquad::Bindings,
    separate_bindings: bool,
}

#[cfg(feature = "render_lifetime")]
impl SceneCopy {
    fn new() -> Result<Self, miniquad::ShaderError> {
        let ctx = unsafe { get_internal_gl() }.quad_context;
        let shader = miniquad::Shader::new(ctx,
            "#version 100\nattribute vec2 position; void main(){ gl_Position=vec4(position,0.0,1.0); }",
            "#version 100\nprecision highp float; uniform highp sampler2D source; uniform vec4 sourceRect; void main(){ vec2 uv=gl_FragCoord.xy*sourceRect.zw+sourceRect.xy; gl_FragColor=texture2D(source,uv); }",
            miniquad::ShaderMeta { images:vec!["source".into()], uniforms:miniquad::UniformBlockLayout { uniforms:vec![miniquad::UniformDesc::new("sourceRect", miniquad::UniformType::Float4)] } })?;
        let pipeline = miniquad::Pipeline::with_params(ctx, &[miniquad::BufferLayout::default()],
            &[miniquad::VertexAttribute::new("position", miniquad::VertexFormat::Float2)], shader, miniquad::PipelineParams::default());
        let vertices = [[-1_f32,-1_f32],[1.,-1.],[1.,1.],[-1.,1.]];
        let indices = [0_u16,1,2,0,2,3];
        let bindings = miniquad::Bindings {
            vertex_buffers:vec![miniquad::Buffer::immutable(ctx,miniquad::BufferType::VertexBuffer,&vertices)],
            index_buffer:miniquad::Buffer::immutable(ctx,miniquad::BufferType::IndexBuffer,&indices),
            images:vec![miniquad::Texture::empty()],
        };
        let version = unsafe { miniquad::gl::glGetString(miniquad::gl::GL_VERSION) };
        let version = if version.is_null() { std::borrow::Cow::Borrowed("") } else { unsafe { std::ffi::CStr::from_ptr(version as _) }.to_string_lossy() };
        let version = version.strip_prefix("OpenGL ES ").unwrap_or(&version);
        let separate_bindings = ctx.supports_framebuffer_blit() || version.split('.').next().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= 3);
        Ok(Self { pipeline, bindings, separate_bindings })
    }

    fn draw(&mut self, source: miniquad::Texture, destination: miniquad::RenderPass,
        viewport: (i32,i32,i32,i32), dim: (u32,u32)) {
        unsafe {
            use miniquad::gl::*;
            let mut read = 0;
            let mut draw = 0;
            let mut previous_viewport = [0;4];
            let mut scissor = 0;
            let mut scissor_box = [0;4];
            let mut dither = 0;
            glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut draw);
            if self.separate_bindings { glGetIntegerv(0x8CAA, &mut read); }
            glGetIntegerv(0x0BA2, previous_viewport.as_mut_ptr());
            glGetIntegerv(GL_SCISSOR_TEST, &mut scissor);
            glGetIntegerv(0x0C10, scissor_box.as_mut_ptr());
            glGetIntegerv(0x0BD0, &mut dither);
            // Blits do not dither. Keep the same RGBA8 quantization boundary.
            glDisable(0x0BD0);
            let ctx = get_internal_gl().quad_context;
            ctx.begin_pass(destination, miniquad::PassAction::Nothing);
            ctx.apply_viewport(0,0,dim.0 as i32,dim.1 as i32);
            ctx.apply_scissor_rect(0,0,dim.0 as i32,dim.1 as i32);
            ctx.apply_pipeline(&self.pipeline);
            self.bindings.images[0] = source;
            ctx.apply_bindings(&self.bindings);
            let rect = [viewport.0 as f32/source.width as f32,viewport.1 as f32/source.height as f32,
                viewport.2 as f32/(source.width as f32*dim.0 as f32),viewport.3 as f32/(source.height as f32*dim.1 as f32)];
            ctx.apply_uniforms(&rect);
            ctx.draw(0,6,1);
            ctx.end_render_pass_preserving_framebuffer();
            if self.separate_bindings {
                glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
                glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw as u32);
            } else { glBindFramebuffer(GL_FRAMEBUFFER, draw as u32); }
            glViewport(previous_viewport[0],previous_viewport[1],previous_viewport[2],previous_viewport[3]);
            glScissor(scissor_box[0],scissor_box[1],scissor_box[2],scissor_box[3]);
            if scissor == 0 { glDisable(GL_SCISSOR_TEST); }
            if dither != 0 { glEnable(0x0BD0); }
        }
    }
}
```

`copy_scene` 的 blit 与 CopyTexSubImage 回退（原文第 1027–1120 行）：

```rust
fn copy_scene(
    res: &Resource,
    pass: Option<miniquad::RenderPass>,
    viewport: (i32, i32, i32, i32),
    texture: Texture2D,
    snapshot_pass: Option<miniquad::RenderPass>,
) {
    use miniquad::gl::*;
    // These GLES3 constants are absent from this miniquad fork's desktop table.
    const READ_FRAMEBUFFER_BINDING: u32 = 0x8CAA;
    const ACTIVE_TEXTURE: u32 = 0x84E0;
    const TEXTURE_BINDING_2D: u32 = 0x8069;
    unsafe {
        if let Some(destination) = snapshot_pass {
            let mut read = 0;
            let mut draw = 0;
            glGetIntegerv(READ_FRAMEBUFFER_BINDING, &mut read);
            glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw);
            let mut scissor = 0;
            glGetIntegerv(GL_SCISSOR_TEST, &mut scissor);
            glDisable(GL_SCISSOR_TEST);
            let source = if let Some(target) = res.chart_target.as_ref().filter(|target| Some(target.input().render_pass) == pass) {
                // Multisample resolve must keep the same size and use NEAREST;
                // then downsample the resolved output with LINEAR.
                target.blit();
                target.output().render_pass.gl_internal_id(get_internal_gl().quad_context)
            } else {
                pass.map(|p| p.gl_internal_id(get_internal_gl().quad_context)).unwrap_or(read as u32)
            };
            glBindFramebuffer(GL_READ_FRAMEBUFFER, source);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, destination.gl_internal_id(get_internal_gl().quad_context));
            glBlitFramebuffer(
                viewport.0,
                viewport.1,
                viewport.0 + viewport.2,
                viewport.1 + viewport.3,
                0,
                0,
                texture.width() as i32,
                texture.height() as i32,
                GL_COLOR_BUFFER_BIT,
                GL_LINEAR,
            );
            glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw as u32);
            if scissor != 0 {
                glEnable(GL_SCISSOR_TEST);
            }
            return;
        }
        let restore = if let Some(target) = res.chart_target.as_ref().filter(|target| Some(target.input().render_pass) == pass) {
            let mut read = 0;
            let mut draw = 0;
            glGetIntegerv(READ_FRAMEBUFFER_BINDING, &mut read);
            glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw);
            target.blit();
            glBindFramebuffer(GL_READ_FRAMEBUFFER, target.output().render_pass.gl_internal_id(get_internal_gl().quad_context));
            Some((read, draw))
        } else {
            // Miniquad end_render_pass restores its default framebuffer after
            // flush. Select the logical pass explicitly for this copy, using
            // GLES2-compatible FRAMEBUFFER APIs, then restore the GL binding.
            let mut binding = 0;
            glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut binding);
            if let Some(pass) = pass {
                glBindFramebuffer(GL_FRAMEBUFFER, pass.gl_internal_id(get_internal_gl().quad_context));
            }
            Some((binding, -1))
        };
        let mut active = 0;
        let mut bound = 0;
        glGetIntegerv(ACTIVE_TEXTURE, &mut active);
        glActiveTexture(GL_TEXTURE0);
        glGetIntegerv(TEXTURE_BINDING_2D, &mut bound);
        glBindTexture(GL_TEXTURE_2D, texture.raw_miniquad_texture_handle().gl_internal_id());
        // Storage is allocated on viewport changes above; do not reallocate
        // a full-resolution texture on every frame.
        glCopyTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, viewport.0, viewport.1, viewport.2, viewport.3);
        glBindTexture(GL_TEXTURE_2D, bound as u32);
        glActiveTexture(active as u32);
        if let Some((read, draw)) = restore {
            if draw < 0 {
                glBindFramebuffer(GL_FRAMEBUFFER, read as u32);
            } else {
                glBindFramebuffer(GL_READ_FRAMEBUFFER, read as u32);
                glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw as u32);
            }
        }
    }
}

const VERTEX: &str = include_str!("block_shader_full.vert");
const FRAGMENT: &str = include_str!("block_shader_full.frag");
```

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| `deny_snapshot_blit` | `false` | 研究用能力否定；强制走 fallback |
| `snapshot_shader_copy` | `false` | 只在 `render_lifetime` 下存在；显式像素诊断 |
| `capture_snapshot_source` | `false` | 读回源图，污染末帧计时的诊断项 |
| `snapshot_blit_sources` | 空 | 按 pass 缓存 `allowed` 结果 |
| 构建特性 | `render_lifetime` | fallback/shader 的主体 |
| 诊断特性 | `render_research` | `research_snapshot_info` |
| 自动能力否定 | 不会自动选 shader | 只有显式 `snapshot_shader_copy=true` 才尝试 |
| 普通默认 | 全部 false | 生产保持原 320×200 blit 或原 full-copy 兼容路径 |
| 生产画质门 | 严格 RGBA 0 差 | 当前未通过 |

### 6. 完整测量数据（表格）

`snapshot-full-copy-diagnostic` 包含两项能力否定对照；`blit-fallback-screen` / `blit-sampler-diagnostic` 包含 shader 候选。严格像素结果：

| 路径 | 源图对照 | 中间结果 | 最终通道不同 | 最大差 | host/GL 错误 | 判定 |
|---|---|---|---:|---:|---|---|
| 原 320×200 线性 blit（参照） | 源相同 | 320×200 RGBA8 | 0（定义） | 0 | 无 | 通过 |
| full-copy（`deny_snapshot_blit`） | `source-vs-source` 相同 | 全尺寸纹理与源逐字节相同 | **254,520** | **71** | 无 | 严格失败 |
| shader copy（`snapshot_shader_copy`） | 源 RGB8 逐字节相同 | 320×200 结果 **11,309 通道 ±1** | **98,728** | 未作为通过项 | 无 | 严格失败 |

补充事实：

| 项目 | 值 |
|---|---|
| 源分辨率 | 1920×1200 |
| 目标分辨率 | 320×200（1/6） |
| 源格式 | RGB8（诊断组） |
| full-copy 目标格式 | 与源匹配，避免 RGB8→RGBA8 非法复制 |
| full-copy 源/目标纹理 | 逐字节相同（`source-equivalence.json`） |
| full-copy 失败原因 | 改变了 320×200 RGBA8 线性 blit 的中间过滤/量化 |
| shader 失败原因 | 单次双线性采样不能等价 6× 降采样；±1 再经 overlay 放大 |
| 能力否定次数 | 2 项 |
| 研究后端 SHA-256 | `7e175d07e57cd5bd2212f282a800b9f92a6abd2d50734d1f2e34c53961d30229` |
| 后端源码 hash | 152 份输入一致 |

严格验收的结论不是“接近”，而是“不等价”：254,520 个通道不同、最大差 71，已经远超任何“±1 量化”可接受范围；shader 的 98,728 个通道不同也不能用“只差一点”描述。

### 7. 质量与回归结果

- `source-vs-source`：两张源图 RGBA SHA 相同，说明测量窗口、谱面时钟、相机与源场景一致；不是输入漂移导致的差异。
- `full-copy-vs-source`：full-copy 纹理与源图逐字节相同，说明复制路径本身没有读错、没有格式错、没有 GL 错误；差异完全来自后续“如何采样这张纹理”。
- full-copy 最终 254,520 通道不同、最大差 71；这是严格 RGBA 比较，不是感知指标。
- shader copy 的 320×200 中间结果 11,309 通道 ±1；最终 98,728 通道不同。中间的小误差被 block overlay 的 mask、混合与量化放大。
- 两项能力否定对照均无 host/GL 错误；`glGetError` 干净不等于画质等价。GL 合法性只证明调用没报错，不证明过滤/量化语义一致。
- 该组只有 Petrichor normal 窗口，没有全曲、没有多 GPU、没有不同分辨率；但严格失败已经足以否定“可作为自动回退”的结论。

### 8. 为什么判定为负（逐项对比）

| 判据 | 期望 | 实际 | 结论 |
|---|---|---|---|
| GL 合法性 | 无错误 | 无错误 | 通过 |
| 源图一致 | 逐字节相同 | 相同 | 通过 |
| 中间语义 | 等价 320×200 线性降采样 | full-copy 直接全尺寸；shader 单 tap | 不等价 |
| 最终严格 RGBA | 0 通道不同 | 254,520 / 98,728 | 失败 |
| 最大差 | 0 | 71 | 失败 |
| 自动回退 | 能力否定时可用 | 不可用 | 负 |
| 性能 | 不是本条目标 | 未作为收益验收 | 不适用 |

机制层面：

1. **原路径的关键不是“复制”，而是“预过滤”。** 1920×1200 → 320×200 的 6× 线性 blit 在降采样时做了预过滤；block shader 随后采样的是已经平均过的小图。full-copy 把全分辨率纹理交给 shader，shader 仍按 320×200 的 UV 间隔采样，等价于“稀疏点采样 + 全分辨率双线性”，没有 6× 预过滤。两者在平坦区域接近，在边缘/高频区域差异大。
2. **shader 的单次双线性采样也不等价。** 6× 降采样通常需要覆盖 6×6 像素的盒式/多 tap 过滤；一个 fragment 只做一次 `texture2D`，最多读到 2×2 源像素。它无法平均整个 6×6 区域，因此出现 ±1；这些 ±1 再经过 mask 乘加、alpha 混合与 R8 量化，放大为最终的大面积差异。
3. **dither 关闭只是其中一项。** `SceneCopy::draw` 关闭 dither 以匹配 blit，但线性过滤核、采样位置、RGB8/RGBA8 格式、量化边界仍未完全匹配。只修 dither 不够。
4. **“无 GL 错误”是最容易误导的证据。** 复制纹理与源逐字节相同、GL 调用全部合法，仍然可以画质失败；这说明问题在采样语义，不在 API 使用。
5. **自动能力否定不能选 shader。** 当前实现明确要求 `snapshot_shader_copy=true` 才尝试 shader；自动能力否定不会选择它。这是正确的保守设计，但也意味着 fallback 仍不可用。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：commit `4f046544` |
| 默认 | `deny_snapshot_blit=false`、`snapshot_shader_copy=false`、`capture_snapshot_source=false` |
| 普通路径 | 未接入；原 320×200 blit 保留 |
| 主线是否已实施 | 否（main）；未进入 |
| 研究后端 | `snapshot-full-copy-policy.so` SHA `7e175d07e57cd5bd2212f282a800b9f92a6abd2d50734d1f2e34c53961d30229` |
| 质量/契约 | 严格 RGBA 失败；source 相同、GL 无错误 |
| 用途 | 只作为显式像素诊断，不作为自动回退 |
| 后续 | 不推进；保留为“GL 合法不等于画质等价”的反例 |

### 10. 可挽救的部分与后续条件

可挽救的是“在无 blit 能力时仍保持 block 效果”这个目标，而不是当前两条 fallback。后续若继续，需要：

1. **精确复刻 6× 线性降采样。** 在 CPU 上按原 blit 的过滤核与量化边界做 320×200 降采样，再上传；或在 shader 里做覆盖 6×6 的多 tap 加权，权重与驱动 blit 的实际行为对齐。只用一次 `texture2D` 不够。
2. **保持格式与量化。** 源 RGB8、目标 RGBA8、R8 量化、alpha 处理、dither 开关都要逐项对齐；不能只匹配其中一个。
3. **严格 RGBA 0 差验收。** 至少覆盖平坦/边缘/半透明/高对比/不同 viewport；任何通道差 >0 都不能自动启用。
4. **跨 GPU/驱动验证。** 线性降采样的实现相关；不同 GPU 的 blit 过滤核可能不同。若无法保证，保留“能力不足则不启用 block 效果/保留原路径”的策略，而不是引入不等价回退。
5. **区分诊断与生产。** `deny_snapshot_blit` 是能力否定测试，不应成为用户可触发的生产开关；`snapshot_shader_copy` 只应在显式像素诊断中启用。
6. **不要用性能收益抵消画质失败。** 本条没有把性能作为验收项；即使 fallback 更快，严格像素失败也不能进入生产。

### 11. 来源与可追溯性（仅溯源）

- 源码：`prpr/src/core/block_shader.rs`、`prpr/src/core/resource.rs`，commit `4f046544`。
- 符号：`snapshot_source_blit`、`normalized_single_sample_fbo`、`SceneCopy`、`copy_scene`、`deny_snapshot_blit`、`snapshot_shader_copy`、`capture_snapshot_source`。
- 后端：`本地证据/backends/snapshot-full-copy-policy.so.json`，SHA `7e175d07e57cd5bd2212f282a800b9f92a6abd2d50734d1f2e34c53961d30229`。
- 数据：`本地证据/snapshot-full-copy-diagnostic`（`source-equivalence.json`、`matrix.json`、两张 `screenshot.png`）、`本地证据/snapshot-full-copy-comparison.log`、`本地证据/blit-fallback-screen`、`本地证据/blit-sampler-diagnostic`、`本地证据/blit-resolved-screen-confirmed`。
- 文档：`docs/development/game-performance-target.md` 第 90 行；`docs/development/render-research-harness.md` 第 158 行；`docs/development/rendering-research.md` 第 423 行；`本地证据/branch-cleanup-20261006/previous-AGENTS.md` 第 77、81 行。
- 原始段落（`本地证据/AGENTS.md` 历史快照，第 81 行）：

> snapshot回退最新失败证据：`snapshot-full-copy-diagnostic`两项能力否定对照无host/GL错误，源图相同，full-copy纹理与原1920×1200 RGB8源逐字节相同；但相对于320×200 RGBA8线性blit，中间过滤/量化语义改变，最终254,520通道不同、最大差71，严格验收失败。shader候选也已失败；现仅在显式`snapshot_shader_copy`像素诊断开关启用，不能自动作为能力否定回退。普通full-copy的GL合法性修复不等于画质回退完成，此项仍需解决。最新后端`snapshot-full-copy-policy.so` SHA7e175d07e57cd5bd2212f282a800b9f92a6abd2d50734d1f2e34c53961d30229，152源码hash一致，普通Android check与31项host契约通过；设备研究进程已停止。主项目未commit/push、库默认 master 未动，完整目标保持active。

- `本地证据/low-quality-normal/README.md` 没有 N-10 的对应条目；该实验属于参考实现的 snapshot 回退研究，不把 README 的低画质表格混入本条目。

## N-11 GPU 常驻音符 IR v1/v2/v3

### 1. 一句话结论与判定（负 + 理由）

**判定：负优化。** 三代实现都把“普通音符的静态顶点/材质/记录”搬到加载期，逐帧只提交可见索引与 pose；语义与画质对照都通过，但整帧没有一代变快：

- **v1 直接提交**：完整应用墙钟 3.323/3.374ms → 4.135/3.501ms，chart 阶段 0.646/0.666ms → 1.432/1.387ms；只有 259,008 bytes 静态 VBO，却新增累计约 4.13MB 的 uniform payload 与 16/376 条 GPU draw。
- **v2 延迟队列**：chart 阶段降到 0.759/0.737ms（direct 仍 1.389/1.387ms），但完整应用墙钟 3.498/3.528/3.523/3.496ms，仍高于对照 3.346/3.367ms；阶段迁移没有变成整帧收益。
- **v3 编译记录**：模型校验/pose 写入从 12005/12021 降到 2907/2899，但完整应用 6.252/6.249ms，old 6.591/6.138ms，new 6.250ms vs old 6.365ms 的差小于 old 自身前后漂移；不能称稳定优于 old。

三代都默认关闭，只作为低画质研究候选保留。它们证明“把音符搬到 GPU”本身不是收益来源；收益取决于是否真正减少每帧提交的数据、draw 数与驱动等待。

### 2. 问题与初衷

普通谱面有 1349 个非 hold 音符、219 个 hold。CPU 路径每帧要遍历判定线、计算 note 的矩阵/高度/时间、构造 4 个顶点与 6 个索引，再按 `(kind order, texture)` 合批。原 `NoteBuffer` 已经做了材质/纹理批处理，所以“减少 draw call”的空间不大；真正可省的是每帧重复的静态顶点构造与部分矩阵/pose 计算。

GPU 常驻 IR 的初衷是：

1. 加载时把语义 IR 降低为不可变 VBO：每个普通音符 4 个顶点，共 259,008 bytes；
2. 逐帧只上传可见音符的 u16 索引（约 144KB/轮）与少量 pose uniform；
3. 保留原材质顺序、加入顺序、f64 高度/时间语义与低画质限制；
4. 高画质、能力不足、复杂对象/hold 回退原路径。

这个方向在“音符极多、每帧顶点构造占主导”的谱面上可能成立；在当前 1349 音符、NoteBuffer 已合批的谱面上，静态顶点节省很小，新增的 uniform/draw/记录成本反而更大。

### 3. 实现思路与具体机制

**v1 直接提交。** `gpu::install` 在加载时：

- 校验 `sample_count == 1`（低画质）、`plan.packets` 非空、IR 与 canonical chart 一致、顶点预算不超 64MiB；
- 查询 `GL_MAX_VERTEX_UNIFORM_VECTORS`（0x8DFB）与 `GL_MAX_VERTEX_ATTRIBS`（0x8869），要求 `attributes >= 3`、`vectors >= 24`；
- `capacity = min((vectors - 12) / 3, 32)`，即每个 page 最多 32 个 pose 组；
- 编译 GLSL100 shader：`attribute vec4 params/data/extra`，`uniform mat4 Projection/Model`，`uniform vec4 Frame`，`uniform vec4 PoseA[capacity]/PoseB[capacity]/PoseC[capacity]`；
- 每个 packet 生成 4 个 `Param { position:[f32;4], data:[f32;4], extra:[f32;4] }`，共 48 bytes/顶点；`position` 存角点 x/y 与 height 的 hi/lo，`data` 存 UV、group、time hi，`extra` 存 time lo 与 RGB 字节；
- 顶点按 page 聚合，page 满（顶点 ≥65536 或 group 数到 capacity）就创建 immutable `Buffer`；
- 逐帧 `try_render` 把可见音符的 6 个 u16 索引写入 `index_buffer`，把每组的 3 个 vec4 pose 写入 page uniforms；`draw_all` 直接 `begin_pass/apply_pipeline/apply_bindings/apply_uniforms/draw/end_render_pass`，每个 GPU item 前后 flush。

**v2 延迟队列。** 宏quad 参考实现，commit `706849529ace7927e6507f235400bbdc1f7dafca`，新增 `resident_geometry`：常驻 draw 进入原 `draw_calls` 队列，记录 target/viewport/scissor，与原 CPU geometry 共享 pass 和全局顺序；uniform 快照按 page 共享 `Arc` 且不可变；更新 index 前如果还有未消费的 resident 命令，先 flush；buffer 销毁前先 flush。v2 的 `deferred=true` 走这条队列，`direct=false`。

**v3 编译记录。** `records.rs` 在加载时按 `(kind order, texture id)` 建立稳定 material id，并用 `BTreeMap<Key, usize>` 维护排序；每个 material 有若干 `Record { page, vertices, indices }`。`Records::reserve` 在加载期最多预留/触达 16MiB 名义容量；`gpu()` 对 GPU 记录追加 6 个索引，`cpu()` 对 CPU 记录追加 4 顶点+6 索引；`end_frame` 只把 `used` 归零，保留容量。`draw_all` 遍历 `records.iter()`，按 key 顺序把 GPU/CPU item 交给原纹理批次。v3 还用 `begin_side/end_side` 缓存同一判定线侧循环的 model/config，减少每 note 的模型校验；`pose_epochs` 让同一 group 在同一个 side epoch 内只写一次 pose。

三代共同的限制：hold 仍走 CPU；高画质 `sample_count != 1` 直接 `ensure!` 失败并回退；`appear_before` 非有限、aspect/flow/width/double_hint 变化都回退；shader 编译本身就是驱动对动态索引 uniform 数组的兼容性探测。

### 4. 文件、符号与完整核心代码

源码快照：参考实现。v1 commit `16aca21c`（加入 GPU 音符驻留原型），v2 主项目 commit `277f0362`（接入常驻音符延迟队列）+ macroquad `7068495`，v3 commit `e1b84a2a`（编译材质记录与复用容量）。文件与符号：

- `prpr/src/render_lab/gpu.rs`：`Param`、`Entry`、`Line`、`Item`、`Page`、`PreparedSide`、`Renderer`、`STATE`、`install`、`try_render`、`record_cpu`、`draw_all`、`begin_side/end_side`、`split`、`vertex/fragment`；
- `prpr/src/render_lab/records.rs`：`Key`、`Record`、`Growth`、`Bin`、`Records`、`material`、`reserve`、`next`、`gpu`、`cpu`、`iter`、`end_frame`；
- `phira/src/render_research.rs`：`gpu_render_ir`、`gpu_ir_deferred`、`gpu_ir_compiled_records`、`gpu_ir_force_fallback` 请求解析与 `gpu::install` 调用；
- 构建特性：`render_chart_ir`（= `render_research` + `chart_pre_compile`）、`render_research`；
- 依赖：macroquad v2 固定 `7068495`；v1/v3 无库改动。

下面是 `gpu.rs` 全文（807 行，含 shader、install、try_render、draw_all、Drop 与 stats；未省略）：

```rust
//! Experimental GPU lowering of the validated semantic IR.
//! Vertex data is immutable after loading; each frame streams only visible
//! u16 indices and a bounded line/speed pose palette. Judgment stays on Chart.
//! Floating-point reassociation is a mild-lossy candidate, low quality only.
use super::ir;
use crate::core::{Chart, Note, Point, RenderConfig, Resource, FADEOUT_TIME};
use anyhow::{ensure, Result};
use macroquad::{prelude::*, window::get_internal_gl};
use miniquad::{
    Bindings, Buffer, BufferLayout, BufferType, Pipeline, PipelineParams, Shader, ShaderMeta, UniformBlockLayout, UniformDesc, UniformType,
    VertexAttribute, VertexFormat,
};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
#[path = "records.rs"]
mod records;

type Key = (i8, u32);
#[repr(C)]
#[derive(Clone, Copy)]
struct Param {
    position: [f32; 4],
    data: [f32; 4],
    extra: [f32; 4],
}
#[derive(Clone, Copy)]
struct Entry {
    page: usize,
    group: usize,
    first: u16,
    height: f64,
    speed: f64,
    time: f64,
    key: Key,
    material: usize,
}
struct Line {
    start: usize,
    count: usize,
    entries: Vec<Option<Entry>>,
}
enum Item {
    Cpu { vertices: Vec<Vertex>, indices: Vec<u16> },
    Gpu { page: usize, indices: Vec<u16> },
}
enum ItemView<'a> {
    Cpu { vertices: &'a [Vertex], indices: &'a [u16] },
    Gpu { page: usize, indices: &'a [u16] },
}
impl Item {
    fn view(&self) -> ItemView<'_> {
        match self {
            Self::Cpu { vertices, indices } => ItemView::Cpu { vertices, indices },
            Self::Gpu { page, indices } => ItemView::Gpu { page: *page, indices },
        }
    }
}
struct Page {
    vertices: Buffer,
    uniforms: Vec<f32>,
    pose_epochs: Vec<u64>,
}
#[derive(Clone, Copy)]
struct PreparedSide {
    above: bool,
    model: crate::core::Matrix,
    line_base: f64,
    epoch: u64,
    checked: bool,
    alpha: f32,
    draw_below: bool,
}
struct Renderer {
    lines: Vec<Line>,
    pages: Vec<Page>,
    palette: usize,
    pipeline: Pipeline,
    index_buffer: Buffer,
    current_line: usize,
    batches: BTreeMap<Key, Vec<Item>>,
    records: records::Records<Vertex>,
    compiled_records: bool,
    side: Option<PreparedSide>,
    side_epoch: u64,
    prepared_calls: u64,
    pose_writes: u64,
    model_checks: u64,
    side_audits: u64,
    side_mismatches: u64,
    input_stream: Sha256,
    recorded_frames: u64,
    index_arena: Vec<u16>,
    aspect: u32,
    flow: u32,
    width: u32,
    hint: bool,
    time: f64,
    templates: usize,
    calls: u64,
    drawn: u64,
    draw_calls: u64,
    index_bytes: u64,
    uniform_bytes: u64,
    static_bytes: usize,
    cpu_quads: u64,
    deferred: bool,
    mutation_flushes: u64,
    audit: bool,
    audited: u64,
    estimated_error: f64,
}
thread_local! {
    static STATE:RefCell<Option<Renderer>>=const {RefCell::new(None)};
    static STATUS:RefCell<serde_json::Value>=RefCell::new(serde_json::json!({"requested":false,"installed":false}));
}
fn split(value: f64) -> (f32, f32) {
    let hi = value as f32;
    (hi, (value - hi as f64) as f32)
}
fn fragment() -> &'static str {
    r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {gl_FragColor=color*texture2D(Texture,uv);}
"#
}
fn vertex(capacity: usize) -> String {
    format!(
        r#"#version 100
attribute vec4 params;
attribute vec4 data;
attribute vec4 extra;
varying lowp vec4 color;
varying lowp vec2 uv;
uniform mat4 Projection;
uniform mat4 Model;
uniform vec4 Frame;
uniform vec4 PoseA[{capacity}];
uniform vec4 PoseB[{capacity}];
uniform vec4 PoseC[{capacity}];
void main() {{
    int slot=int(data.z);
    vec4 a=PoseA[slot];vec4 b=PoseB[slot];vec4 c=PoseC[slot];
    float y=((params.z-a.w)+(params.w-b.w))+params.y;
    vec2 world=vec2(a.x*params.x+a.y*y+a.z,b.x*params.x+b.y*y+b.z);
    gl_Position=Projection*Model*vec4(world,0.,1.);
    float delta=(data.w-Frame.x)+(extra.x-Frame.y);
    float alpha=c.x*(c.y>0.5 ? 1. : (min(delta,0.)/Frame.z+1.));
    color=vec4(extra.yzw/255.,clamp(floor(alpha*255.),0.,255.)/255.);
    uv=data.xy;
}}
"#
    )
}
pub fn disable() {
    STATE.with(|s| *s.borrow_mut() = None);
    STATUS.with(|s| *s.borrow_mut() = serde_json::json!({"requested":false,"installed":false}));
}
pub fn unavailable(reason: String) {
    disable();
    STATUS.with(|s| *s.borrow_mut() = serde_json::json!({"requested":true,"installed":false,"fallback":reason}));
}
pub fn set_line(id: usize) {
    STATE.with(|s| {
        if let Some(r) = s.borrow_mut().as_mut() {
            r.current_line = id;
            r.side = None;
        }
    });
}
pub fn stats() -> serde_json::Value {
    STATE.with(|s|match s.borrow().as_ref() {
    None=>STATUS.with(|v|v.borrow().clone()),
    Some(r)=>serde_json::json!({"requested":true,"installed":true,"gpu_resident":true,"templates":r.templates,"pages":r.pages.len(),"palette_capacity":r.palette,
        "calls":r.calls,"drawn":r.drawn,"gpu_draw_calls":r.draw_calls,"dynamic_index_bytes":r.index_bytes,"uniform_payload_bytes":r.uniform_bytes,
        "static_vertex_bytes":r.static_bytes,"gameplay_vertex_bytes":0,"fallback_cpu_quads":r.cpu_quads,"audited_vertices":r.audited,
        "cpu_shader_estimate_pixel_delta":r.estimated_error,"quality_class":"mild-lossy-pending-pixel-validation","gpu_blob_persisted":false,
        "submission":if r.deferred {"macroquad-queue"} else {"direct"},"pending_buffer_mutation_flushes":r.mutation_flushes,
        "native_queued_draws_issued":unsafe {get_internal_gl()}.quad_gl.resident_geometry_draw_count(),
        "compiled_records":r.compiled_records,"prepared_note_calls":r.prepared_calls,"pose_writes":r.pose_writes,"model_checks":r.model_checks,
        "side_audits":r.side_audits,"side_mismatches":r.side_mismatches,
        "recorded_frames":r.recorded_frames,"input_stream_sha256":if r.audit {Some(format!("{:x}",r.input_stream.clone().finalize()))} else {None},
        "record_growth":{"bins":r.records.growth.bins,"spans":r.records.growth.spans,"vertices":r.records.growth.vertices,"indices":r.records.growth.indices},
        "loading_record_reserved_bytes":r.records.reserved_bytes}),
})
}
pub fn install(plan: &ir::Plan, chart: &Chart, res: &Resource, audit: bool, deferred: bool, compiled_records: bool) -> Result<()> {
    ensure!(res.config.sample_count == 1, "GPU IR approximation requires low quality");
    ensure!(!plan.packets.is_empty(), "No supported packets");
    ensure!(ir::compile(chart, plan.source).0 == *plan, "IR does not describe canonical source chart");
    ensure!(plan.packets.len() <= 64 * 1024 * 1024 / (4 * std::mem::size_of::<Param>()), "GPU vertex budget exceeded");
    let (mut vectors, mut attributes) = (0, 0);
    unsafe {
        miniquad::gl::glGetIntegerv(0x8dfb, &mut vectors);
        miniquad::gl::glGetIntegerv(0x8869, &mut attributes);
    }
    ensure!(unsafe { miniquad::gl::glGetError() } == 0 && attributes >= 3 && vectors >= 24, "Vertex capability probe failed");
    let capacity = ((vectors as usize - 12) / 3).min(32);
    ensure!(capacity > 0, "No pose uniform budget");
    let context = unsafe { get_internal_gl() }.quad_context;
    // Compilation is itself the driver compatibility probe for GLSL100
    // dynamically indexed vertex uniform arrays; failure uses original notes.
    let shader = Shader::new(
        context,
        &vertex(capacity),
        fragment(),
        ShaderMeta {
            images: vec!["Texture".into()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("Projection", UniformType::Mat4),
                    UniformDesc::new("Model", UniformType::Mat4),
                    UniformDesc::new("Frame", UniformType::Float4),
                    UniformDesc::new("PoseA", UniformType::Float4).array(capacity),
                    UniformDesc::new("PoseB", UniformType::Float4).array(capacity),
                    UniformDesc::new("PoseC", UniformType::Float4).array(capacity),
                ],
            },
        },
    )?;
    let pipeline = Pipeline::with_params(
        context,
        &[BufferLayout::default()],
        &[
            VertexAttribute::new("params", VertexFormat::Float4),
            VertexAttribute::new("data", VertexFormat::Float4),
            VertexAttribute::new("extra", VertexFormat::Float4),
        ],
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
    let mut lines = chart
        .lines
        .iter()
        .map(|line| Line {
            start: line.notes.as_ptr() as usize,
            count: line.notes.len(),
            entries: vec![None; line.notes.len()],
        })
        .collect::<Vec<_>>();
    let mut pages = Vec::new();
    let mut vertices = Vec::new();
    let mut groups = BTreeMap::new();
    let mut static_bytes = 0;
    let mut records = records::Records::default();
    if compiled_records {
        let mut counts = BTreeMap::<Key, usize>::new();
        for line in &chart.lines {
            for note in &line.notes {
                let style = if res.config.double_hint && note.multiple_hint {
                    &res.res_pack.note_style_mh
                } else {
                    &res.res_pack.note_style
                };
                let (texture, quads) = match note.kind {
                    crate::core::NoteKind::Click => (*style.click, 1),
                    crate::core::NoteKind::Drag => (*style.drag, 1),
                    crate::core::NoteKind::Flick => (*style.flick, 1),
                    crate::core::NoteKind::Hold { .. } => (*style.hold, 3),
                };
                *counts
                    .entry((note.kind.order(), texture.raw_miniquad_texture_handle().gl_internal_id()))
                    .or_default() += quads;
                if matches!(note.kind, crate::core::NoteKind::Hold { .. }) {
                    if let Some(body) = &style.hold_body {
                        *counts.entry((0, body.raw_miniquad_texture_handle().gl_internal_id())).or_default() += 1;
                    }
                }
            }
        }
        for (key, quads) in counts {
            records.reserve(key, quads.saturating_mul(4).min(65536), quads.saturating_mul(6));
        }
    }
    for p in &plan.packets {
        let group_key = (p.line, p.flags & (1 << 8), p.speed.to_bits());
        if vertices.len() >= 65536 || (!groups.contains_key(&group_key) && groups.len() == capacity) {
            static_bytes += std::mem::size_of_val(vertices.as_slice());
            pages.push(Page {
                vertices: Buffer::immutable(context, BufferType::VertexBuffer, &vertices),
                uniforms: vec![0.; 36 + capacity * 12],
                pose_epochs: if compiled_records { vec![0; capacity] } else { Vec::new() },
            });
            vertices.clear();
            groups.clear();
        }
        let group_count = groups.len();
        let group = *groups.entry(group_key).or_insert(group_count);
        let hinted = res.config.double_hint && p.flags & (1 << 9) != 0;
        let style = if hinted { &res.res_pack.note_style_mh } else { &res.res_pack.note_style };
        // Material order is the original NoteKind::order, NOT the enum ordinal.
        let note = &chart.lines[p.line as usize].notes[p.note as usize];
        let texture = match note.kind {
            crate::core::NoteKind::Click => *style.click,
            crate::core::NoteKind::Drag => *style.drag,
            crate::core::NoteKind::Flick => *style.flick,
            _ => unreachable!(),
        };
        let scale = (if hinted {
            res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
        } else {
            1.
        }) * res.note_width;
        let half = vec2(scale, texture.height() * scale / texture.width());
        let x = -half.x;
        let y = -half.y;
        let w = half.x * 2.;
        let h = half.y * 2.;
        let speed = p.speed * res.config.flow_speed as f64 * 1.;
        let height = p.height / res.aspect_ratio as f64 * speed;
        let (height_hi, height_lo) = split(height);
        let (time_hi, time_lo) = split(p.time);
        let first = vertices.len() as u16;
        for (cx, cy, u, v) in [(x, y + h, 0., 0.), (x + w, y + h, 1., 0.), (x + w, y, 1., 1.), (x, y, 0., 1.)] {
            vertices.push(Param {
                position: [p.x + cx, cy, height_hi, height_lo],
                data: [u, v, group as f32, time_hi],
                extra: [
                    time_lo,
                    (p.color[0] * 255.) as u8 as f32,
                    (p.color[1] * 255.) as u8 as f32,
                    (p.color[2] * 255.) as u8 as f32,
                ],
            });
        }
        let key = (p.flags as i8 & 3, texture.raw_miniquad_texture_handle().gl_internal_id());
        lines[p.line as usize].entries[p.note as usize] = Some(Entry {
            page: pages.len(),
            group,
            first,
            height,
            speed,
            time: p.time,
            key,
            material: if compiled_records { records.material(key) } else { 0 },
        });
    }
    if !vertices.is_empty() {
        static_bytes += std::mem::size_of_val(vertices.as_slice());
        pages.push(Page {
            vertices: Buffer::immutable(context, BufferType::VertexBuffer, &vertices),
            uniforms: vec![0.; 36 + capacity * 12],
            pose_epochs: if compiled_records { vec![0; capacity] } else { Vec::new() },
        });
    }
    let index_buffer = Buffer::stream(context, BufferType::IndexBuffer, (plan.packets.len() * 6 * 2).max(256));
    if unsafe { miniquad::gl::glGetError() } != 0 {
        for page in &pages {
            page.vertices.delete();
        }
        index_buffer.delete();
        anyhow::bail!("GPU immutable allocation failed");
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(Renderer {
            lines,
            pages,
            palette: capacity,
            pipeline,
            index_buffer,
            current_line: 0,
            batches: BTreeMap::new(),
            records,
            compiled_records,
            side: None,
            side_epoch: 0,
            prepared_calls: 0,
            pose_writes: 0,
            model_checks: 0,
            side_audits: 0,
            side_mismatches: 0,
            input_stream: Sha256::new(),
            recorded_frames: 0,
            index_arena: Vec::with_capacity(plan.packets.len() * 6),
            aspect: res.aspect_ratio.to_bits(),
            flow: res.config.flow_speed.to_bits(),
            width: res.note_width.to_bits(),
            hint: res.config.double_hint,
            time: 0.,
            templates: plan.packets.len(),
            calls: 0,
            drawn: 0,
            draw_calls: 0,
            index_bytes: 0,
            uniform_bytes: 0,
            static_bytes,
            cpu_quads: 0,
            deferred,
            mutation_flushes: 0,
            audit,
            audited: 0,
            estimated_error: 0.,
        })
    });
    Ok(())
}
fn eligible(r: &Renderer, res: &Resource, config: &RenderConfig) -> bool {
    res.config.sample_count == 1
        && res.aspect_ratio.to_bits() == r.aspect
        && res.config.flow_speed.to_bits() == r.flow
        && res.note_width.to_bits() == r.width
        && res.config.double_hint == r.hint
        && !config.appear_before.is_finite()
}
fn valid_model(model: &crate::core::Matrix) -> bool {
    model[(2, 0)] == 0. && model[(2, 1)] == 0. && model[(2, 2)] == 1. && model.iter().all(|v| v.is_finite())
}
/// Only JudgeLine's side loop enters this scope. Its Resource model/config
/// remain unchanged across individual notes; CPU note scopes restore the model.
/// Outside a side scope try_render performs the original per-note validation.
pub(crate) fn begin_side(res: &Resource, config: &RenderConfig, above: bool) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(r) = state.as_mut() else {
            return;
        };
        r.side = None;
        if !r.compiled_records || !eligible(r, res, config) {
            return;
        }
        let model = *res.model_stack.last().unwrap();
        r.side_epoch = match r.side_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                for p in &mut r.pages {
                    p.pose_epochs.fill(0);
                }
                1
            }
        };
        r.side = Some(PreparedSide {
            above,
            model,
            line_base: config.line_height / res.aspect_ratio as f64,
            epoch: r.side_epoch,
            checked: false,
            alpha: res.alpha,
            draw_below: config.draw_below,
        });
    });
}
pub(crate) fn end_side() {
    STATE.with(|s| {
        if let Some(r) = s.borrow_mut().as_mut() {
            r.side = None;
        }
    });
}
pub fn try_render(note: &Note, res: &mut Resource, config: &RenderConfig) -> bool {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(r) = state.as_mut() else {
            return false;
        };
        let mut side = r.side.filter(|side| side.above == note.above);
        if side.is_none() && !eligible(r, res, config) {
            return false;
        }
        let Some(line) = r.lines.get(r.current_line) else {
            return false;
        };
        let Some(offset) = (note as *const _ as usize).checked_sub(line.start) else {
            return false;
        };
        let size = std::mem::size_of::<Note>();
        if offset % size != 0 || offset / size >= line.count {
            return false;
        }
        let Some(entry) = line.entries[offset / size] else {
            return false;
        };
        if let Some(prepared) = side.filter(|_| r.audit) {
            r.side_audits += 1;
            let actual = res.model_stack.last().unwrap();
            if !eligible(r, res, config)
                || prepared.model.iter().zip(actual.iter()).any(|(a, b)| a.to_bits() != b.to_bits())
                || prepared.line_base.to_bits() != (config.line_height / res.aspect_ratio as f64).to_bits()
                || prepared.alpha.to_bits() != res.alpha.to_bits()
                || prepared.draw_below != config.draw_below
            {
                r.side_mismatches += 1;
                r.side = None;
                side = None;
            }
        }
        let model = match side {
            Some(side) => {
                if !side.checked {
                    r.model_checks += 1;
                    if !valid_model(&side.model) {
                        r.side = None;
                        return false;
                    }
                    r.side.as_mut().unwrap().checked = true;
                }
                r.prepared_calls += 1;
                side.model
            }
            None => {
                let model = *res.model_stack.last().unwrap();
                r.model_checks += 1;
                if !valid_model(&model) {
                    return false;
                }
                model
            }
        };
        let line_height = side.map_or_else(|| config.line_height / res.aspect_ratio as f64, |side| side.line_base) * entry.speed;
        let base = entry.height - line_height;
        r.calls += 1;
        if !config.draw_below && (res.time - FADEOUT_TIME >= entry.time || (entry.time > res.time && base <= -0.001)) {
            return true;
        }
        r.time = res.time;
        let (hi, lo) = split(line_height);
        let n = r.palette;
        let page = &mut r.pages[entry.page];
        if side.is_none_or(|side| page.pose_epochs[entry.group] != side.epoch) {
            let uniforms = &mut page.uniforms;
            uniforms[36 + entry.group * 4..40 + entry.group * 4].copy_from_slice(&[model[(0, 0)], model[(0, 1)], model[(0, 2)], hi]);
            uniforms[36 + n * 4 + entry.group * 4..40 + n * 4 + entry.group * 4].copy_from_slice(&[model[(1, 0)], model[(1, 1)], model[(1, 2)], lo]);
            uniforms[36 + n * 8 + entry.group * 4..40 + n * 8 + entry.group * 4].copy_from_slice(&[
                res.alpha,
                config.draw_below as u8 as f32,
                0.,
                0.,
            ]);
            if r.compiled_records {
                page.pose_epochs[entry.group] = side.map_or(0, |side| side.epoch);
            }
            r.pose_writes += 1;
        }
        if r.audit {
            let style = if res.config.double_hint && note.multiple_hint {
                &res.res_pack.note_style_mh
            } else {
                &res.res_pack.note_style
            };
            let texture = match note.kind {
                crate::core::NoteKind::Click => *style.click,
                crate::core::NoteKind::Drag => *style.drag,
                crate::core::NoteKind::Flick => *style.flick,
                _ => unreachable!(),
            };
            let scale = (if res.config.double_hint && note.multiple_hint {
                res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
            } else {
                1.
            }) * res.note_width;
            let half = vec2(scale, texture.height() * scale / texture.width());
            let x = -half.x;
            let y = -half.y;
            let w = half.x * 2.;
            let h = half.y * 2.;
            let reference = model * note.now_transform(res, config.ctrl_obj, base as f32, config.incline_sin);
            let (hh, hl) = split(entry.height);
            let note_x = note.object.translation.0.now_opt().unwrap_or(0.);
            for (cx, cy) in [(x, y + h), (x + w, y + h), (x + w, y), (x, y)] {
                let yy = ((hh - hi) + (hl - lo)) + cy;
                let xx = note_x + cx;
                let actual =
                    Point::new(model[(0, 0)] * xx + model[(0, 1)] * yy + model[(0, 2)], model[(1, 0)] * xx + model[(1, 1)] * yy + model[(1, 2)]);
                let expected = reference.transform_point(&Point::new(cx, cy));
                r.estimated_error = r
                    .estimated_error
                    .max((actual.x - expected.x).abs().max((actual.y - expected.y).abs()) as f64 * res.last_vp.2 as f64 / 2.);
                r.audited += 1;
            }
        }
        if r.compiled_records {
            r.records.gpu(entry.material, entry.page, entry.first);
            r.drawn += 1;
            return true;
        }
        let items = r.batches.entry(entry.key).or_default();
        if !matches!(items.last(),Some(Item::Gpu {page,..}) if *page==entry.page) {
            items.push(Item::Gpu {
                page: entry.page,
                indices: Vec::new(),
            });
        }
        if let Item::Gpu { indices, .. } = items.last_mut().unwrap() {
            let i = entry.first;
            indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
        }
        r.drawn += 1;
        true
    })
}
pub fn record_cpu(key: Key, quad: [Vertex; 4], max_vertices: usize) -> bool {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(r) = state.as_mut() else {
            return false;
        };
        if r.compiled_records {
            r.records.cpu(key, quad, max_vertices);
            r.cpu_quads += 1;
            return true;
        }
        let items = r.batches.entry(key).or_default();
        if !matches!(items.last(),Some(Item::Cpu {vertices,..}) if vertices.len()+4<=max_vertices) {
            items.push(Item::Cpu {
                vertices: Vec::new(),
                indices: Vec::new(),
            });
        }
        if let Item::Cpu { vertices, indices } = items.last_mut().unwrap() {
            let i = vertices.len() as u16;
            vertices.extend_from_slice(&quad);
            indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
        }
        r.cpu_quads += 1;
        true
    })
}
pub fn draw_all() -> bool {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(r) = state.as_mut() else {
            return false;
        };
        let batches = std::mem::take(&mut r.batches);
        r.index_arena.clear();
        for items in batches.values() {
            for item in items {
                if let Item::Gpu { indices, .. } = item {
                    r.index_arena.extend_from_slice(indices);
                }
            }
        }
        for (_, item) in r.records.iter() {
            if item.page.is_some() {
                r.index_arena.extend_from_slice(&item.indices);
            }
        }
        let mut gl = unsafe { get_internal_gl() };
        gl.quad_gl.draw_mode(DrawMode::Triangles);
        let projection = gl.quad_gl.get_projection_matrix();
        let model = gl.quad_gl.get_model_matrix();
        let pass = gl.quad_gl.get_active_render_pass();
        let viewport = gl.quad_gl.get_viewport();
        let scissor = gl.quad_gl.get_scissor();
        if !r.index_arena.is_empty() {
            // A second Chart render before the queue flush must consume its
            // previous index contents before orphaning. Ordinary frames do not
            // take this path; uniforms below are immutable per-frame snapshots.
            if r.deferred && gl.quad_gl.has_pending_resident_geometry() {
                gl.flush();
                r.mutation_flushes += 1;
            }
            r.index_buffer.update_orphaned(gl.quad_context, &r.index_arena);
            r.index_bytes += r.index_arena.len() as u64 * 2;
        }
        let (time_hi, time_lo) = split(r.time);
        for page in &mut r.pages {
            page.uniforms[..16].copy_from_slice(&projection.to_cols_array());
            page.uniforms[16..32].copy_from_slice(&model.to_cols_array());
            page.uniforms[32..36].copy_from_slice(&[time_hi, time_lo, FADEOUT_TIME as f32, 0.]);
        }
        let snapshots = if r.deferred {
            r.pages.iter().map(|p| Arc::<[f32]>::from(p.uniforms.as_slice())).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut index_start = 0;
        if r.audit {
            r.input_stream.update(r.recorded_frames.to_le_bytes());
            r.input_stream.update((r.index_arena.len() as u64).to_le_bytes());
            for index in &r.index_arena {
                r.input_stream.update(index.to_le_bytes());
            }
            for page in &r.pages {
                for value in &page.uniforms {
                    r.input_stream.update(value.to_bits().to_le_bytes());
                }
            }
        }
        r.recorded_frames += 1;
        let legacy = batches
            .iter()
            .flat_map(|(&(_, texture), items)| items.iter().map(move |item| (texture, item.view())));
        let compiled = r.records.iter().map(|((_, texture), item)| {
            (
                texture,
                match item.page {
                    Some(page) => ItemView::Gpu {
                        page,
                        indices: &item.indices,
                    },
                    None => ItemView::Cpu {
                        vertices: &item.vertices,
                        indices: &item.indices,
                    },
                },
            )
        });
        for (texture, item) in legacy.chain(compiled) {
            if r.audit {
                match &item {
                    ItemView::Gpu { page, indices } => {
                        r.input_stream.update([1]);
                        r.input_stream.update((*page as u64).to_le_bytes());
                        r.input_stream.update((indices.len() as u64).to_le_bytes());
                    }
                    ItemView::Cpu { vertices, indices } => {
                        r.input_stream.update([0]);
                        r.input_stream.update((vertices.len() as u64).to_le_bytes());
                        r.input_stream.update((indices.len() as u64).to_le_bytes());
                        for vertex in *vertices {
                            let (position, uv, color): ([f32; 3], [f32; 2], [f32; 4]) = (*vertex).into();
                            for value in position.into_iter().chain(uv).chain(color) {
                                r.input_stream.update(value.to_bits().to_le_bytes());
                            }
                        }
                        for index in *indices {
                            r.input_stream.update(index.to_le_bytes());
                        }
                    }
                }
            }
            let texture = unsafe { miniquad::Texture::from_raw_id(texture, miniquad::TextureFormat::RGBA8) };
            match item {
                ItemView::Cpu { vertices, indices } => {
                    let gl = unsafe { get_internal_gl() };
                    gl.quad_gl.texture(Some(Texture2D::from_miniquad_texture(texture)));
                    gl.quad_gl.geometry(vertices, indices);
                }
                ItemView::Gpu { page, indices } => {
                    if r.deferred {
                        // Resource owners outlive queued commands. Drop
                        // flushes before deleting buffers; subsequent index
                        // updates use the pending-command guard above.
                        let gl = unsafe { get_internal_gl() };
                        unsafe {
                            gl.quad_gl.resident_geometry(
                                r.pipeline,
                                Bindings {
                                    vertex_buffers: vec![r.pages[page].vertices],
                                    index_buffer: r.index_buffer,
                                    images: vec![texture],
                                },
                                snapshots[page].clone(),
                                index_start,
                                indices.len() as i32,
                            );
                        }
                        index_start += indices.len() as i32;
                        r.draw_calls += 1;
                        r.uniform_bytes += r.pages[page].uniforms.len() as u64 * 4;
                        continue;
                    }
                    let mut gl = unsafe { get_internal_gl() };
                    gl.flush();
                    let context = gl.quad_context;
                    context.begin_pass(pass, miniquad::PassAction::Nothing);
                    let (width, height) = pass
                        .map(|p| {
                            let t = p.texture(context);
                            (t.width as i32, t.height as i32)
                        })
                        .unwrap_or_else(|| {
                            let (w, h) = context.screen_size();
                            (w as i32, h as i32)
                        });
                    let (x, y, w, h) = viewport.unwrap_or((0, 0, width, height));
                    context.apply_viewport(x, y, w, h);
                    let (x, y, w, h) = scissor.map(|(x, y, w, h)| (x, height - (y + h), w, h)).unwrap_or((0, 0, width, height));
                    context.apply_scissor_rect(x, y, w, h);
                    context.apply_pipeline(&r.pipeline);
                    context.apply_bindings(&Bindings {
                        vertex_buffers: vec![r.pages[page].vertices],
                        index_buffer: r.index_buffer,
                        images: vec![texture],
                    });
                    let uniforms = &r.pages[page].uniforms;
                    context.apply_uniforms_from_bytes(uniforms.as_ptr().cast(), uniforms.len() * 4);
                    context.draw(index_start, indices.len() as i32, 1);
                    context.end_render_pass();
                    index_start += indices.len() as i32;
                    r.draw_calls += 1;
                    r.uniform_bytes += uniforms.len() as u64 * 4;
                }
            }
        }
        r.records.end_frame();
        true
    })
}
impl Drop for Renderer {
    fn drop(&mut self) {
        // Raw bindings in the resident queue refer to these buffers.
        unsafe { get_internal_gl() }.flush();
        for page in &self.pages {
            page.vertices.delete();
        }
        self.index_buffer.delete();
    }
}
```

`records.rs` 全文（230 行，含测试；未省略）：

```rust
//! Retained mixed draw records. Stable material IDs are compiled at load;
//! execution still sorts by the original key and keeps insertion order.
use std::collections::BTreeMap;

pub(super) type Key = (i8, u32);

pub(super) struct Record<V> {
    pub page: Option<usize>,
    pub vertices: Vec<V>,
    pub indices: Vec<u16>,
}
impl<V> Default for Record<V> {
    fn default() -> Self {
        Self {
            page: None,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Growth {
    pub bins: u64,
    pub spans: u64,
    pub vertices: u64,
    pub indices: u64,
}
struct Bin<V> {
    key: Key,
    records: Vec<Record<V>>,
    used: usize,
}
pub(super) struct Records<V> {
    ids: BTreeMap<Key, usize>,
    bins: Vec<Bin<V>>,
    order: Vec<usize>,
    pub growth: Growth,
    pub reserved_bytes: usize,
}
impl<V: Copy + Default> Default for Records<V> {
    fn default() -> Self {
        Self {
            ids: BTreeMap::new(),
            bins: Vec::new(),
            order: Vec::new(),
            growth: Growth::default(),
            reserved_bytes: 0,
        }
    }
}
impl<V: Copy + Default> Records<V> {
    pub fn material(&mut self, key: Key) -> usize {
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = self.bins.len();
        let position = self.order.partition_point(|&i| self.bins[i].key < key);
        self.bins.push(Bin {
            key,
            records: Vec::new(),
            used: 0,
        });
        self.order.insert(position, id);
        self.ids.insert(key, id);
        self.growth.bins += 1;
        id
    }
    /// Bounded loading-time reservation. Runtime overflow uses normal Vec
    /// growth, which is counted; this is not a promise of zero allocations.
    pub fn reserve(&mut self, key: Key, vertices: usize, indices: usize) {
        let id = self.material(key);
        let bin = &mut self.bins[id];
        if bin.records.is_empty() {
            bin.records.push(Record::default());
            self.growth.spans += 1;
        }
        let record = &mut bin.records[0];
        let remaining = 16 * 1024 * 1024 - self.reserved_bytes;
        let vertices = vertices.min(remaining / std::mem::size_of::<V>().max(1));
        record.vertices.reserve_exact(vertices);
        record.vertices.resize(vertices, V::default());
        std::hint::black_box(record.vertices.as_slice());
        record.vertices.clear();
        self.reserved_bytes += vertices * std::mem::size_of::<V>();
        let indices = indices.min((16 * 1024 * 1024 - self.reserved_bytes) / 2);
        record.indices.reserve_exact(indices);
        record.indices.resize(indices, 0);
        std::hint::black_box(record.indices.as_slice());
        record.indices.clear();
        self.reserved_bytes += indices * 2;
    }
    fn next(&mut self, id: usize, page: Option<usize>, cpu_limit: usize) -> &mut Record<V> {
        let bin = &mut self.bins[id];
        let merge =
            bin.used > 0 && bin.records[bin.used - 1].page == page && (page.is_some() || bin.records[bin.used - 1].vertices.len() + 4 <= cpu_limit);
        if !merge {
            if bin.used == bin.records.len() {
                bin.records.push(Record::default());
                self.growth.spans += 1;
            }
            let record = &mut bin.records[bin.used];
            record.page = page;
            record.vertices.clear();
            record.indices.clear();
            bin.used += 1;
        }
        &mut bin.records[bin.used - 1]
    }
    pub fn gpu(&mut self, material: usize, page: usize, first: u16) {
        let record = self.next(material, Some(page), 0);
        let growing = record.indices.capacity() < record.indices.len() + 6;
        record
            .indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        self.growth.indices += growing as u64;
    }
    pub fn cpu(&mut self, key: Key, quad: [V; 4], max_vertices: usize) {
        let material = self.material(key);
        let record = self.next(material, None, max_vertices.clamp(4, 65536));
        let vg = record.vertices.capacity() < record.vertices.len() + 4;
        let ig = record.indices.capacity() < record.indices.len() + 6;
        let first = record.vertices.len() as u16;
        record.vertices.extend_from_slice(&quad);
        record
            .indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        self.growth.vertices += vg as u64;
        self.growth.indices += ig as u64;
    }
    pub fn iter(&self) -> impl Iterator<Item = (Key, &Record<V>)> {
        self.order.iter().flat_map(|&id| {
            let bin = &self.bins[id];
            bin.records[..bin.used].iter().map(move |r| (bin.key, r))
        })
    }
    pub fn end_frame(&mut self) {
        for bin in &mut self.bins {
            bin.used = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    enum Triangle {
        Cpu([u32; 3]),
        Gpu(usize, [u16; 3]),
    }
    fn triangles(r: &Records<u32>) -> BTreeMap<Key, Vec<Triangle>> {
        let mut result: BTreeMap<Key, Vec<Triangle>> = BTreeMap::new();
        for (key, record) in r.iter() {
            for t in record.indices.chunks_exact(3) {
                result.entry(key).or_default().push(match record.page {
                    Some(page) => Triangle::Gpu(page, t.try_into().unwrap()),
                    None => Triangle::Cpu([
                        record.vertices[t[0] as usize],
                        record.vertices[t[1] as usize],
                        record.vertices[t[2] as usize],
                    ]),
                });
            }
        }
        result
    }
    #[test]
    fn mixed_records_match_unbatched_sorted_triangle_oracle_across_frames() {
        let mut records = Records::<u32>::default();
        let mut seed = 43_u32;
        for _ in 0..20 {
            let mut expected: BTreeMap<Key, Vec<Triangle>> = BTreeMap::new();
            for n in 0..400 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let key = ((seed % 4) as i8, (seed >> 8) % 7);
                if seed & 32 != 0 {
                    let page = ((seed >> 15) % 3) as usize;
                    let first = n * 4;
                    let id = records.material(key);
                    records.gpu(id, page, first);
                    expected.entry(key).or_default().extend([
                        Triangle::Gpu(page, [first, first + 1, first + 2]),
                        Triangle::Gpu(page, [first, first + 2, first + 3]),
                    ]);
                } else {
                    let b = n as u32 * 4;
                    records.cpu(key, [b, b + 1, b + 2, b + 3], 8);
                    expected
                        .entry(key)
                        .or_default()
                        .extend([Triangle::Cpu([b, b + 1, b + 2]), Triangle::Cpu([b, b + 2, b + 3])]);
                }
            }
            assert_eq!(triangles(&records), expected);
            let keys = records.iter().map(|(k, _)| k).collect::<Vec<_>>();
            assert!(keys.windows(2).all(|v| v[0] <= v[1]));
            records.end_frame();
            assert!(records.iter().next().is_none());
        }
    }
    #[test]
    fn repeated_workload_reuses_spans_and_capacity_without_stale_payloads() {
        let mut records = Records::<u32>::default();
        records.reserve((2, 5), 64, 96);
        let id = records.material((2, 5));
        let mut warm = None;
        for frame in 0..30 {
            records.gpu(id, 2, 0);
            records.cpu((2, 5), [frame, frame + 1, frame + 2, frame + 3], 8);
            records.gpu(id, 1, 4);
            records.cpu((2, 5), [frame + 4, frame + 5, frame + 6, frame + 7], 8);
            assert_eq!(records.iter().count(), 4);
            if frame == 0 {
                warm = Some(records.growth);
            }
            assert_eq!(Some(records.growth), warm);
            assert!(triangles(&records)[&(2, 5)].contains(&Triangle::Cpu([frame, frame + 1, frame + 2])));
            records.end_frame();
        }
    }
    #[test]
    fn late_material_keeps_existing_ids_and_draw_order() {
        let mut records = Records::<u32>::default();
        let high = records.material((3, 100));
        records.gpu(high, 0, 0);
        records.cpu((0, 1), [1, 2, 3, 4], 65536);
        assert_eq!(records.material((3, 100)), high);
        assert_eq!(records.iter().map(|(k, _)| k).collect::<Vec<_>>(), vec![(0, 1), (3, 100)]);
    }
}
```

### 5. 开关、默认值、构建特性

| 项 | 值 | 说明 |
|---|---|---|
| `gpu_render_ir` | `false` | 研究请求总开关；false 时走 CPU IR 或原路径 |
| `gpu_ir_deferred` | `false` | v2 延迟队列；false 时 v1 直接提交 |
| `gpu_ir_compiled_records` | `false` | v3 编译记录；false 时 v1/v2 的 batches |
| `ir_audit` | `false` | 逐帧输入流 SHA、模型/pose 审计 |
| `gpu_ir_force_fallback` | `false` | 强制能力否定，验证回退 |
| 构建特性 | `render_chart_ir` | = `render_research` + `chart_pre_compile` |
| 画质门 | `sample_count == 1` | 高画质直接 `ensure!` 失败并回退 |
| 顶点预算 | 64MiB / 4 顶点 | `plan.packets.len() <= 64MiB / (4*48)` |
| pose 容量 | `min((vectors-12)/3, 32)` | 依赖当前 context 的 uniform 向量上限 |
| 生产 Config | 无 | 没有 Settings/UI 入口 |
| 默认 | 全部 false | 普通构建不编译/不安装 |

### 6. 完整测量数据（表格）

**v1 直接提交**（同一 SO，120 预热 + 1200 采样，4 轮）：

| 次序 | 路径 | 完整应用墙钟均值 | 应用 p99 | chart 均值 | 持续 FPS |
|---:|---|---:|---:|---:|---:|
| 0 | 原音符 | 3.323ms | 18.491ms | 0.646ms | 59.748 |
| 1 | GPU 直接 | 4.135ms | 19.250ms | 1.432ms | 59.751 |
| 2 | GPU 直接 | 3.501ms | 19.088ms | 1.387ms | 59.755 |
| 3 | 原音符 | 3.374ms | 18.950ms | 0.666ms | 59.754 |

v1 资源量：

| 项目 | 值 |
|---|---:|
| 普通音符 | 1349 |
| hold 回退 | 219 |
| 静态 VBO | 259,008 bytes（1349×4×48） |
| 每轮动态索引 | 144,348 / 144,360 bytes |
| 累计 uniform payload | 4,132,800 bytes（含预热） |
| GPU draw | 16 / 376 |
| 质量 | 7 作业 / 4 组 RGBA 相同 |
| CPU shader 估计最大像素误差 | 0.000305px |

**v2 延迟队列**（同一 SO，6 轮）：

| 次序 | 路径 | 完整应用墙钟均值 | 应用 p99 | chart 均值 | 持续 FPS |
|---:|---|---:|---:|---:|---:|
| 0 | 原音符 | 3.346ms | 19.141ms | 0.657ms | 59.752 |
| 1 | GPU 直接 | 3.498ms | 18.410ms | 1.389ms | 59.755 |
| 2 | GPU 延迟 | 3.528ms | 18.188ms | 0.759ms | 59.759 |
| 3 | GPU 延迟 | 3.523ms | 19.400ms | 0.737ms | 59.755 |
| 4 | GPU 直接 | 3.496ms | 19.557ms | 1.387ms | 59.760 |
| 5 | 原音符 | 3.367ms | 19.521ms | 0.654ms | 59.753 |

v2 资源量：

| 项目 | 值 |
|---|---:|
| 每轮 native resident draw | 2460 |
| pending index protection flush | 0 |
| 静态 VBO | 259,008 bytes |
| 累计 uniform payload | 约 4.13MB（含预热） |
| 质量 | 8 作业 / 5 组 RGBA 相同 |

**v3 编译记录**（同一 SO，6 轮，CPU/old/new/new/old/CPU）：

| 次序 | 路径 | 完整应用墙钟 | 应用 p99 | chart 均值 | 持续 FPS | 模型校验/pose 写入 | 呈现最大间隔 |
|---:|---|---:|---:|---:|---:|---:|---:|
| 0 | CPU | 6.113ms | 19.687ms | 1.153ms | 59.599 | — | 18.616ms |
| 1 | GPU 原记录 | 6.591ms | 20.171ms | 1.380ms | 59.619 | 12005/12005 | 33.468ms |
| 2 | GPU 新记录 | 6.252ms | 20.244ms | 1.273ms | 59.699 | 2907/2907 | 17.795ms |
| 3 | GPU 新记录 | 6.249ms | 19.687ms | 1.289ms | 59.758 | 2899/2899 | 17.072ms |
| 4 | GPU 原记录 | 6.138ms | 19.386ms | 1.289ms | 59.750 | 12021/12021 | 18.654ms |
| 5 | CPU | 6.254ms | 19.616ms | 1.202ms | 59.703 | — | 16.869ms |

v3 资源量：

| 项目 | 值 |
|---|---:|
| 加载记录名义预留/触达 | 216,648 bytes |
| 记录 bin / span | 8 / 8 |
| 动态模型校验/pose 写入 | 2159 → 587 |
| 累计 uniform payload | 约 4.13MB（含预热） |
| 质量 | 10 作业 / 9 组 RGBA 相同 |
| 记录测试 | 20 帧 × 400 混合指令，逐项与未批处理 oracle 相同 |

**三代汇总**（用每代同轮对照的中位数比较，不跨代相加）：

| 代 | 对照完整应用 | GPU 完整应用 | 差值 | chart 变化 | 判定 |
|---|---:|---:|---:|---:|---|
| v1 | 3.323 / 3.374（均 3.349） | 4.135 / 3.501（均 3.818） | +0.469（+14.0%） | 0.656→1.410（约 +115%） | 明显变慢 |
| v2 direct | 3.346 / 3.367（均 3.357） | 3.498 / 3.496（均 3.497） | +0.140（+4.2%） | 0.656→1.388（约 +112%） | 变慢 |
| v2 queue | 同上 | 3.528 / 3.523（均 3.526） | +0.169（+5.0%） | 0.656→0.748（约 +14%） | 仍变慢 |
| v3 | old 6.591 / 6.138（均 6.365） | new 6.252 / 6.249（均 6.251） | −0.114（−1.8%） | 1.335→1.281 | 小于 old 漂移 0.453，不稳定 |
| v3 CPU 参照 | CPU 6.113 / 6.254（均 6.184） | new 6.251 | +0.067 | 1.178→1.281 | 仍高于 CPU |

**易混数据辨析：629,760 bytes 不是音符 IR 的 uniform。** 该数字来自普通线 IR 的 `lines-v1-quality`（`uniform_payload_bytes=629760`，包含预热），属于另一条优化线；音符 GPU IR 的对应数字是累计约 4,132,800 bytes。两者不能互相替代，也不能相加。

| 数据 | 值 | 所属 |
|---|---:|---|
| 音符 GPU IR 累计 uniform | 4,132,800 bytes | v1/v2/v3 `gpu-*` cohort |
| 普通线 IR 累计 uniform | 629,760 bytes（约 630KB） | `lines-v1-quality` / `compiler-v1`，不是 N-11 |

### 7. 质量与回归结果

- v1：7 个 quality 作业形成 4 组有效末帧 RGBA 对照，全部相同；低画质两组 actual `gpu_draw_calls>0`，高画质/强制能力回退实际执行；CPU shader 估计最大 0.000305px，不代表硬件浮点误差上界。
- v2：8 个 quality 作业形成 5 组末帧 RGBA 对照，全部相同；普通路径含 CPU hold，actual native resident draw=16/376 与录制数一致，pending index protection flush=0；高画质/forced-capability 两组回退与 direct/queue 对照也相同。
- v3：10 个 quality 作业形成 9 组末帧 RGBA 对照，全部相同；新旧 GPU 固定/动态与新 GPU direct/queue 的逐帧 index/uniform/CPU 顶点流 SHA 相同；动态 2159 次作用域审计无模型/config 差异；2159→587 的模型校验/pose 写入没有改变结果。
- 三代都仍归类为 `mild-lossy-pending-pixel-validation`：GPU 浮点重关联、uniform 数组的 lowp/mediump 精度、动态索引的驱动实现都可能因 GPU 而异。有限窗口 RGBA 相同不能升级为全设备无损。
- 没有全曲、没有上下文丢失/重建、没有 shader/pipeline 完整释放、没有跨 GPU。质量通过只支持“这些固定窗口没有坏”，不支持“可以进生产”。

### 8. 为什么判定为负（逐项对比）

| 判据 | v1 | v2 | v3 |
|---|---|---|---|
| 静态顶点是否减少 | 是（259,008 bytes） | 是 | 是 |
| 每帧顶点构造是否减少 | 部分 | 部分 | 部分 |
| 每帧 uniform 是否减少 | 否（累计 4.13MB） | 否 | 否 |
| draw 数是否减少 | 否（16/376） | 否（2460 native） | 否 |
| chart 阶段 | 0.656→1.410ms | direct 1.388 / queue 0.748ms | 1.335→1.281ms |
| 完整应用 | 3.349→3.818ms | 3.357→3.497/3.526ms | 6.365→6.251ms（漂移内） |
| 质量 | 4 组 RGBA | 5 组 RGBA | 9 组 RGBA |
| 结论 | 负 | 负 | 负/不稳定 |

机制层面的负优化原因：

1. **省的顶点太少。** 1349 个普通音符 × 4 顶点 × 48 bytes = 259KB 静态数据；原路径每帧构造的顶点量也不大，且 `NoteBuffer` 已经按材质/纹理合批。把 259KB 搬到 GPU 只省了 CPU 侧的顶点写入，没有省 draw、没有省纹理绑定、没有省矩阵/可见性判断。
2. **新增的 uniform 更大。** v1/v2/v3 的累计 uniform payload 约 4.13MB（含预热），远大于静态 VBO。每个 page 的 `PoseA/PoseB/PoseC` 数组、Projection/Model/Frame 都要逐 draw 上传；page 越多、group 越多，uniform 越大。省 259KB 顶点却增加 4MB uniform，方向相反。
3. **v1 每个 GPU item 前后 flush / 结束 pass。** 直接路径在每个 GPU item 前 `gl.flush()`、`begin_pass`、`apply_*`、`draw`、`end_render_pass`；这把原本可以合批的提交切成多个 pass，chart 阶段从 0.65ms 涨到 1.4ms。完整应用也变慢，说明阶段迁移没有换来总时间下降。
4. **v2 队列只迁移了阶段。** 延迟队列把 chart 阶段的 1.389ms 降到 0.759/0.737ms，但完整应用仍是 3.50–3.53ms，高于对照 3.35ms。被“省掉”的 chart 时间转移到了队列录制、uniform 快照、`Arc` 分配与后续 flush；总时间没有下降。native 2460 次 resident draw 本身也是成本。
5. **v3 记录减少的是校验，不是提交。** 模型校验/pose 写入从 12005 降到 2907，但每帧仍要遍历 `BTreeMap`/`Records`、构造 `Arc` 快照、上传 uniform、发出 draw。`reserve` 的 216,648 bytes 只是加载期名义容量，不减少逐帧 payload。new 6.251ms vs old 6.365ms 的 −0.114ms 小于 old 自身 6.138–6.591 的 0.453ms 漂移；不能称稳定优于 old，且 new 仍高于 CPU 6.184ms。
6. **v3 的“记录复用”有额外复杂度。** `Records::next` 在每帧重置 `used`，`iter` 按 key 顺序 flat_map，`BTreeMap` 查找/排序、`Vec` 容量检查、`Arc` 快照都在热路径；这些成本抵消了“少写 pose”的收益。测试证明排序/容量复用正确，不证明整帧更快。

### 9. 主线实施状态与参考实现状态

| 项 | v1 | v2 | v3 |
|---|---|---|---|
| 参考实现 | 已落地（未合入 main）：`16aca21c` | 已落地（未合入 main）：`277f0362` + macroquad `7068495` | 已落地（未合入 main）：`e1b84a2a` |
| 库 commit | 无 | macroquad `7068495` | 无 |
| 研究后端 | `gpu-lab.so` SHA `e879ff255d23d6eeb7ce73dca58b94724d356edd61257e77f71cc96f0553bf82` | `queue-lab.so` SHA `7163b5591f7347e18db4da07b464b5f2a602acfe279f14baa565179c75dad607` | `records-lab.so` SHA `9a535605b737c4a6d2916ff5a187dc5a7843a7b81ea3603434f5f1ba17132d61` |
| 默认 | 关 | 关 | 关 |
| 普通路径 | 未接入 | 未接入 | 未接入 |
| 主线是否已实施 | 否（main） | 否（main） | 否（main） |
| 质量/契约 | 4 组 RGBA | 5 组 RGBA | 9 组 RGBA |
| 结论 | 负 | 负 | 负/不稳定 |

### 10. 可挽救的部分与后续条件

可挽救的是“加载期降低静态表示”这个方向，但需要换掉当前的提交模型：

1. **用 instancing/顶点属性，而不是 uniform 数组。** 每个音符的 pose 放进 instance buffer 或 texture buffer，避免每 page 的 `PoseA/B/C` 大数组；只有真正减少每帧 uniform 字节，才可能抵消静态 VBO 的收益。
2. **静态索引/可见性预计算。** 如果可见索引每帧变化，u16 索引上传是必要的；但可以把“可见性判断”与“索引生成”放到更便宜的路径，或用 GPU-driven culling，减少 CPU 遍历与索引上传。
3. **减少 page/group 数。** 当前 page 满 65536 顶点或 32 group 才切；page 越多，uniform 越多。可以把 pose 编码成更紧凑的 per-instance 数据，减少 page 数与 draw 数。
4. **去掉热路径的 `BTreeMap`/`Arc`。** v3 的记录排序在加载期完成，运行时用扁平数组/索引；uniform 快照按 frame 复用，避免每 draw 分配 `Arc`。
5. **选对谱面。** 只有音符极多、CPU 顶点构造/矩阵求值占主导的谱面才可能收益；当前 1349 音符、NoteBuffer 已合批的谱面不适合。需要先用 CPU profile 证明 note 顶点/矩阵成本是主项。
6. **跨代对照要控制漂移。** v3 的 old 两轮相差 0.453ms，大于 new-old 差；后续必须用更长的窗口、更多轮次、同 SO 的 ABBA，并把 CPU 参照一起报。
7. **完整质量/生命周期。** 全曲、上下文丢失/重建、shader/pipeline 释放、跨 GPU 浮点精度；当前只有固定窗口 RGBA。

### 11. 来源与可追溯性（仅溯源）

- 源码：`prpr/src/render_lab/gpu.rs`、`prpr/src/render_lab/records.rs`、`phira/src/render_research.rs`。
- commit：v1 `16aca21c7f788c916343384be2f1482996fcd31f`；v2 `277f0362`；v3 `e1b84a2a21491c5deac022fe190972a7536c7eaa`；macroquad v2 `706849529ace7927e6507f235400bbdc1f7dafca`。
- 后端：`本地证据/backends/gpu-lab.so.json`（SHA `e879ff255d23d6eeb7ce73dca58b94724d356edd61257e77f71cc96f0553bf82`）、`queue-lab.so`（SHA `7163b5591f7347e18db4da07b464b5f2a602acfe279f14baa565179c75dad607`）、`records-lab.so`（SHA `9a535605b737c4a6d2916ff5a187dc5a7843a7b81ea3603434f5f1ba17132d61`）。
- 数据：`本地证据/low-quality-normal/gpu-v1-performance-runs`、`gpu-v1-quality-runs`、`gpu-v2-performance-runs`、`gpu-v2-quality-runs`、`gpu-v3-performance-runs`、`gpu-v3-quality-runs`、`gpu-runs`。
- 文档：`本地证据/low-quality-normal/README.md` 第 213–252 行；`本地证据/AGENTS.md` 第 679–703 行；`docs/development/rendering-research.md`。
- 以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。以下是 `本地证据/low-quality-normal/README.md` 第 213–252 行的对应段落与表格原文（完整粘贴，未改写）；历史引文中的分支标识已归一化为“参考实现”，其余原文未改：

```text
## GPU 常驻 Render IR v1：有效消费，普通谱面没有加速

`gpu-lab.so` SHA256 `e879ff255d23d6eeb7ce73dca58b94724d356edd61257e77f71cc96f0553bf82`，直接编译当前参考实现源文件并冻结hash，无注入/还原。GPU消费者源码在 `prpr/src/render_lab/gpu.rs`；note.rs、resource.rs接入普通音符和混合批次，render_research.rs导出实际激活及数据量。Android observer入口只在Android研究feature中启用；默认及Windows研究feature构建检查、Android arm64 API26 release构建、6个IR/cache测试通过，日志gpu-*-check/tests、gpu-lab-build.log。

加载时把semantic二进制IR降低为1349普通音符的immutable VBO，259,008bytes；219hold及复杂对象保留CPU。静态每顶点48bytes含角点/X、双f32分裂的height/time、group与原RGB字节，逐帧上传可见u16索引和3个vec4/组的pose。全局材质/加入顺序保留；Shader GLSL100、u16普通索引，不要求instancing。当前context查询uniform/attribute预算，shader编译作为驱动索引兼容探测；高画质/失败回退原路径。它没有持久化降低后的GPU blob，仅持久化semantic IR并加载降低。

同一后端7个quality作业形成4组末帧对照：低画质固定、低画质动态、高画质回退、强制缺能力回退，RGBA全同；低画质实际drawn=56/2159、GPU draw=16/376，CPU reference consumer关闭。8636顶点的CPU shader表达式估计误差最大0.000305px，不能据此界定硬件浮点误差；仍是轻量有损待扩展验证。固定时间quality阶段耗时不能用于真实性能结论。

真实时钟ABBA，120预热+1200采样帧、字体pool在两侧同样开启，HUD准备/背景融合/GPU计时关闭：

| 次序 | 策略 | 应用FPS | 应用p99 ms | chart均值ms | 更新+渲染+提交均值ms |
|---|---|---:|---:|---:|---:|
| 0 | 原音符 | 59.748 | 18.491 | 0.646 | 3.323 |
| 1 | GPU参数 | 59.751 | 19.250 | 1.432 | 4.135 |
| 2 | GPU参数 | 59.755 | 19.088 | 1.387 | 3.501 |
| 3 | 原音符 | 59.754 | 18.950 | 0.666 | 3.374 |

没有普通谱面性能收益，不能进入生产。chart范围现在包含原本延迟到后面阶段的部分flush工作，不能把chart变慢等同GPU独占计算变慢。完整应用墙钟也更高，不能仅以60fps刷新上限判定没有代价。处理两轮静态VBO不更新，动态索引144,348/144,360bytes，但uniform payload各4,132,800bytes（均包含预热，与采样窗口口径分开）；小物量场景代替的原顶点数据很少。原始数据gpu-v1-*，不和其他backend/cohort混池。

下一轮验证方向是把resident命令录入现有macroquad队列，同普通geometry保持顺序并延迟提交，避免每个GPU span提前flush和结束pass；同时收紧palette/减少同page重复uniform发布、复用CPU记录容量。该方向是从源码/负收益推断的待验证方案，尚未证明这些因素各自贡献。shader/pipeline生命周期、context丢失、真实首次GPU预热和生产开关/统一缓存清理仍未完成。

## GPU v2 有序队列：画面一致，完整普通 gameplay 仍未加速

macroquad 独立参考实现提交并 push `706849529ace7927e6507f235400bbdc1f7dafca`，master未改。主项目固定此rev，锁文件仅macroquad/macroquad_macro来源变化，miniquad仍旧固定rev。`queue-lab.so` SHA7163b5591f7347e18db4da07b464b5f2a602acfe279f14baa565179c75dad607；日志queue-library/default/windows-check、queue-lab-build、queue-ir-tests。

常驻draw进入原draw_calls队列，共享target/pass、viewport/scissor和原全局顺序。普通geometry不能跨常驻命令合批；f32 uniform快照按page共享Arc且immutable；重用index前如有未消费旧resident命令先flush，buffer销毁前flush。Shader/pipeline完整释放和失去context仍未完成。8个quality作业构成5组末帧像素对照，RGBA全同；普通路径含CPU hold，actual native resident draw=16/376与录制数一致，pending index protection flush=0。高画质/forced-capability两组回退和直接/延迟GPU对照也相同。

同一SO六轮120预热+1200真实时钟采样，所有臂都开启相同font pool、关闭HUD准备/背景融合/GPU计时：

| 次序 | 策略 | FPS | 应用p99 ms | chart均值ms | 完整应用墙钟均值ms |
|---|---|---:|---:|---:|---:|
| 0 | 原音符 | 59.752 | 19.141 | 0.657 | 3.346 |
| 1 | GPU直接 | 59.755 | 18.410 | 1.389 | 3.498 |
| 2 | GPU延迟 | 59.759 | 18.188 | 0.759 | 3.528 |
| 3 | GPU延迟 | 59.755 | 19.400 | 0.737 | 3.523 |
| 4 | GPU直接 | 59.760 | 19.557 | 1.387 | 3.496 |
| 5 | 原音符 | 59.753 | 19.521 | 0.654 | 3.367 |

GPU延迟每轮native发出2460次，无index保护flush；uniform payload仍4,132,800bytes（含预热），静态VBO259,008bytes不更新。阶段耗时迁移并不等于总耗时降低，没有普通谱面收益，不进生产。数据gpu-v2-*与gpu-v1分开，三个策略只在同一SO内比较。

```


`本地证据/low-quality-normal/README.md` 第 344–363 行的 v3 段落与表格原文（完整粘贴）：

```text
最新GPU v3研究完成，以下方案须结合其负收益修正，不能只按记录操作次数继续堆实现。

主项目本地提交 `e1b84a2a21491c5deac022fe190972a7536c7eaa`（编译材质记录与复用容量），未push；两库固定rev不变。`records-lab.so` SHA256 `9a535605b737c4a6d2916ff5a187dc5a7843a7b81ea3603434f5f1ba17132d61`，20个源文件冻结并核对commit。实际代码：gpu.rs/records.rs，line.rs作用域，render_research.rs独立研究请求。加载编译稳定材质编号和有序执行表，预留/触达至多16MiB名义记录容量；混合CPU/GPU span与索引/顶点容量复用，不释放每帧bins。每侧首次真实GPU音符验证模型，按page/group/侧准备pose，原note迭代/判定不变，保持全局材质与加入顺序；shader和VBO布局仍v2。uniform缓存关闭。CPU record溢出仍正常Vec增长并计数，Arc快照/Bindings等仍分配，不称整体零分配。

Windows默认/研究feature检查、最终9个IR/cache/records测试、Android arm64 API26构建通过。records测试用20帧×400混合指令同独立未批处理三角形oracle逐项核对排序/加入顺序，验证CPU跨度上限、迟到材质编号稳定、重复工作容量复用与旧数据不泄漏。10个真机quality作业9组末帧RGBA全同；原/新GPU固定、原/新GPU动态、新GPU direct/queue三组逐帧index/uniform/CPU顶点数据流SHA全同。流摘要未覆盖纹理像素或硬件浮点精度，RGBA是另一项有限窗口证据。动态作用域模型/配置审计2159次、差异0；模型校验/pose写入2159→587。加载记录名义216,648bytes、8bin/8span；所测窗口无新span或索引/顶点容量增长。原GPU浮点近似分类不因本轮一致升级为全设备无损。

同SO真实时钟六轮，120预热+1200采样，font pool/HUD/背景设置相同，audit/原生写坏/GPU timer/uniform cache关闭；应用墙钟非GPU或CPU独占时间：

| 次序 | 路径 | 应用FPS | 应用间隔p99 ms | chart均值ms | 完整墙钟均值ms | 模型校验/pose写入 | 呈现最大间隔ms |
|---|---|---:|---:|---:|---:|---:|---:|
| 0 | CPU | 59.599 | 19.687 | 1.153 | 6.113 | — | 18.616 |
| 1 | GPU原记录 | 59.619 | 20.171 | 1.380 | 6.591 | 12005/12005 | 33.468 |
| 2 | GPU新记录 | 59.699 | 20.244 | 1.273 | 6.252 | 2907/2907 | 17.795 |
| 3 | GPU新记录 | 59.758 | 19.687 | 1.289 | 6.249 | 2899/2899 | 17.072 |
| 4 | GPU原记录 | 59.750 | 19.386 | 1.289 | 6.138 | 12021/12021 | 18.654 |
| 5 | CPU | 59.703 | 19.616 | 1.202 | 6.254 | — | 16.869 |

操作计数含预热；新GPU uniform payload仍约4.13MB。new约6.250ms、old约6.365ms，但old前后差异大，new并不持续优于old；CPU约6.184ms。不能称有稳定普通谱面完整帧收益，不进入生产。new两轮呈现最大低于old首轮，但只有单个old 33.47ms事件，不称尖峰已消除。原始gpu-v3-*与历史GPU/字体cohort分开；fixed quality审计本身较重，不能用其耗时判断真实性能。

下一轮先补可选Android原生线程CPU耗时，并与update/render/flush及chart/HUD/particle墙钟配对。普通谱面同时需要看计算、调度与驱动等待，不能把墙钟都归给CPU算法。Source角点可见边界请求四次同矩阵逆，但LLVM可能已消公共表达式；strip release没有内部函数符号，llvm-nm未提供函数证据。独立A/B验证后再决定采用，不凭源码调用数宣称收益。真实降低GPU blob/独立Config与UI/统一cache清理统计、上下文/重入/资源生命周期、首帧GPU预热与其他设备验证仍未完成。
```

以下为历史证据原文；其中“已实施”指参考实现落地，不代表主线已实施。`本地证据/AGENTS.md` 第 679–703 行的对应段落原文（完整粘贴，未改写）；历史引文中的分支标识已归一化为“参考实现”，其余原文未改：

```text
最新 GPU Render IR v1 已真正实现（2026-10-07）：源码 prpr/src/render_lab/gpu.rs，在加载时将1349普通音符降为259,008 bytes immutable VBO；原Chart仍用于判定/音频，219hold等走CPU。GLES100 vertex uniform palette由当前context向量/属性上限与shader编译探测决定；不支持/高画质回到原路径。逐帧只有visible u16索引和pose uniforms，混合记录保持原材质排序和加入顺序。后端gpu-lab SHA e879ff255d23d6eeb7ce73dca58b94724d356edd61257e77f71cc96f0553bf82。7个真机quality作业形成4组有效末帧对照，RGBA全同；低画质两组actual gpu_draw_calls>0，高画质/forced capability回退真实执行。CPU shader估计最大0.000305px不代表硬件误差上界，仍分类轻量有损待扩展验证。Windows默认/研究feature检查、Android arm64 API26构建与6个IR/cache测试通过。

上述源码已本地提交 `16aca21c7f788c916343384be2f1482996fcd31f`（加入GPU音符驻留原型）；当前工作区承载参考实现，tracked工作区干净，未push。gpu-lab.so.json已逐项比对冻结source_hashes，并记录source_commit。此次无依赖库改动，两库仍master干净；下一步若做macroquad队列API，先在其独立仓库建立独立参考实现，配套固定完整rev，不能改库master。

GPU v1真实时钟ABBA没有普通谱面收益：对照应用更新+渲染+提交墙钟均值3.323/3.374ms，GPU 4.135/3.501ms；chart阶段0.646/0.666→1.432/1.387ms，其中GPU提前flush会改变阶段归属，不能据此称GPU计算本身慢2倍。均约59.75fps，应用p99无稳定改善。处理每轮可见索引约144KB，但uniform payload约4.13MB；不能只称“顶点零上传”而忽略新动态数据。数据gpu-v1-*，不混历史CPU或字体cohort。下一步研究把resident命令接入macroquad延迟队列/保留pass，而不是每个GPU批次自行flush并结束pass，减少palette重复发布与CPU记录分配，然后同后端验证。v1保留为实验失败/语义参考，不进入生产。

GPU v1仍未落盘真实降低后的VBO blob（只持久化semantic IR然后加载降低），生产独立Config/UI/统一清缓存仍未接通；有效生产消费者集成时必须一并完成。pipeline/shader销毁/复用、context变化、首帧GPU真实预热仍待处理；当前研究APK每个作业重启，不能冒充完整重入生命周期验证。

最新 GPU v2 延迟队列研究完成：独立库 prpr-macroquad 参考实现已提交并 push `706849529ace7927e6507f235400bbdc1f7dafca`（支持常驻网格延迟提交），master未改；主项目参考实现固定此 rev，miniquad仍1796f445。新resident命令录制target/viewport/clip，在原全局顺序中执行，与CPU geometry共享pass；CPU不能跨resident合并。uniform快照不可变，更新index前检查未消费旧命令，Drop先flush再删buffer。后端queue-lab SHA7163b5591f7347e18db4da07b464b5f2a602acfe279f14baa565179c75dad607。Windows默认/研究feature、独立库检查、Android arm64 API26构建、6个IR/cache测试通过。8个quality作业5组末帧对照RGBA全同，GPU实际native发出16/376条（与录制数一致），普通游玩保护flush=0。

GPU v2同SO六轮control/direct/queue/queue/direct/control（120预热+1200采样，gpu-v2-*）：完整应用墙钟均值3.346/3.498/3.528/3.523/3.496/3.367ms，均59.75–59.76fps；queue chart阶段0.759/0.737ms，direct1.389/1.387ms，但完整墙钟未改善，不能把阶段迁移称加速。native queue各发出2460次，无额外index保护flush；每轮uniform payload仍4.13MB。目标未完成。下一步研究GL program uniform的按位相同值消除，须保证program间隔离、数组完整bounds、raw GL/重新链接失效及默认关闭对照；该思路是待验证的命令数据流优化，不能归因已有负收益或称已有效。

GPU v2源码已本地提交 `277f0362696f781b8a7f587e353bb8e9778322a0`（接入常驻音符延迟队列），未push。用户再次确认可直接修改当前参考实现源码并 commit，不再注入/恢复。当前uniform缓存研究实际修改独立miniquad源码，参考实现提交并 push `9466727421b21cad00e63231d9d7dbf4a68af426`；macroquad配套研究提交并push `a8aace0947640c867898239bba3c4f14b6f5f7c0`，两库master未动。主项目固定两库完整rev。缓存按program/field保留完整字节、4MiB名义预算、数组bounds检查，别名布局回退；默认关闭，raw写入须显式失效，raw relink还须重建location元数据。计数只涵盖packed uniform API，不含未改变的sampler setter。四个库测试通过，Windows默认/研究feature检查、Android arm64 API26构建通过；uniform-lab SHA e95d71b668ebec0eeff06889c3474e3b9a692ccbe09272e146655962818551f3。真机画面/原生写入失效测试正在运行，尚无有效画质或性能结论。生产开关/清理/真实降低GPU blob等目标仍未完成。

uniform验证源码已本地提交 `e1f097e70037f20b398e8e8e48ec30ec9bf7161f`（加入uniform缓存验证），未push；uniform-lab冻结19个源文件与本地commit逐项核对（Git CRLF正规化），tracked工作区干净。10个真机quality作业完成6组有效末帧RGBA对照，全同；CPU/GPU、高画质与实际raw Projection写坏后失效恢复均执行。动态低画质CPU实际跳过12760/18540次packed uniform调用；GPU跳过13624/20044次，缓存实际288/1968bytes。不能把它称所有原生GL调用消除或所有设备/帧画质验收。

uniform v1八轮同SO真实时钟交错性能测试完成（120预热+1200采样，uniform-v1-*，不混GPU v2历史cohort）：cpu-off/on/gpu-on/off/off/on/cpu-on/off，完整应用墙钟均值6.149/6.313/6.225/6.140/6.316/6.278/6.118/6.148ms；CPU开关均值约6.215/6.149ms，GPU约6.251/6.228ms，没有稳定完整帧收益。CPU/GPU约69% packed调用消除、约67% payload消除均是真实执行计数，包含预热；sampler仍不去重。应用FPS约59.66–59.76，独立呈现p99约16.77–16.80ms，但4/8轮仍有约33.46–33.50ms最大呈现间隔；不能称尖峰消除。当前battery记录30–33℃不能单独证明CPU频率/热降频，更不能把与历史约3.3ms之差归因缓存；只在同SO内得出本轮负收益。该方案默认关闭，研究结果保留，不进入生产。

当前完整目标审计（仍未完成）：源码/独立库参考实现与完整 rev 已完成；真正加载时GPU降低、真实消费、质量/能力回退与多个负收益实验已完成。shader/pipeline生命周期和重入/context变化、加载期真实GPU预热、全曲/多字体/其他设备/普通高物量验证仍未完成。下一步优先减少GPU记录逐音符检查/临时分配、收紧palette表示与常驻工作集，逐项隔离收益，不能只看阶段或GL调用计数。真正降低的GPU blob落盘仍未完成；生产独立Config/UI开关与统一cache大小/清理/禁用失效必须随有效消费者同阶段接通，当前只有研究请求开关与缓存管理器验证。字体池静态字体浪费与真实物理内存仍未解决。目标保留active，不以这轮已commit/六组RGBA一致为完整交付。

GPU v3正在实现并验证（仍在参考实现上直接编辑源码）：新增prpr/src/render_lab/records.rs，在加载时编译稳定材质编号/排序表、最多16MiB名义CPU记录容量预留并用默认值触达；逐帧复用混合CPU/GPU span及顶点/索引容量，记录动态增长，不承诺零分配。JudgeLine以上/下侧作用域准备pose，首个实际GPU note才校验模型，同一page/group/侧只写一次pose；原判定、源note迭代与全局材质/加入顺序保留。GPU shader与静态顶点布局仍v2，uniform缓存关闭，以隔离CPU记录表示的作用。质量作业还核对原/新每帧index、uniform、CPU顶点数据流摘要与模型/config作用域审计；不是硬件浮点误差证明。三个独立triangle-oracle/容量复用/新增材质稳定编号测试加旧6个IR/cache测试已通过初版；摘要代码已修正为库提供的Vertex转换接口，最终源码测试/Android构建/真机质量与性能仍待完成。有效消费者开关、统一缓存和真实降低GPU blob仍不能遗漏。

GPU v3本轮已完成并本地提交 `e1b84a2a21491c5deac022fe190972a7536c7eaa`（编译材质记录与复用容量），主项目未push，两库rev不变。records-lab SHA `9a535605b737c4a6d2916ff5a187dc5a7843a7b81ea3603434f5f1ba17132d61`，20个冻结源文件已逐项核对commit。Windows默认/研究feature编译检查、最终9个IR/cache/records测试、Android arm64 API26 release构建通过。10个真机quality作业9组RGBA全同；新旧GPU固定/动态与新GPU direct/queue的逐帧索引/uniform/CPU顶点流SHA相同；动态2159次作用域审计无模型/config差异。新GPU动态模型校验/pose写入各2159→587；记录加载名义预留并触达216,648bytes，8材质/8span，所测质量/性能窗口无后续span或Vec容量增长。Arc uniform快照与Bindings等仍可能分配，不称整体零分配；仍是低画质GPU浮点候选，未完成全曲/生命周期验证。

GPU v3同SO六轮CPU/old/new/new/old/CPU真实时钟（120预热+1200采样，gpu-v3-*）：完整墙钟均值6.113/6.591/6.252/6.249/6.138/6.254ms，chart均值1.153/1.380/1.273/1.289/1.289/1.202ms。new两轮模型校验/pose写入2907/2899，相比old12005/12021约四分之一；动态uniform payload仍约4.13MB，含预热。old前后差异大，new约6.250ms、old约6.365ms、CPU约6.184ms，不能称有稳定普通谱面完整帧收益。呈现最大new17.79/17.07ms，old33.47/18.65ms，只有单个old长间隔，不称已消除尖峰。源Shader/静态布局不变，GPU v3留作结构/语义参考，不进入生产。
```

## 第三部分：N-12 至 N-17

本文只讨论已有参考实现并做过真机或离线对照的六项渲染候选。所有“慢百分之几”都是同一台设备、同一份 SO、同一段谱面、同一组配置下的 ABBA 或顺序对照观察值；它们不是 GPU 独占时间，也不是全曲、全设备或生产默认路径的结论。每一项都按统一模板给出：结论、初衷、机制、源码、开关、完整数据、质量、逐项归因、主线实施状态与参考实现状态、可挽救部分和来源。

阅读约定：

- 设备除特别说明外均为 `8DF6R17309002674`：EDI-AL10 / hi3650 / Android 8 / arm64-v8a，实际上下文 Mali-T880 / OpenGL ES 3.2 v1.r20p0，画面 2560×1440。
- 谱面除特别说明外均为 `What do you want more than a Happy ending.zip`，PGR v3，24 条判定线、1568 个音符；规范解析得到 1349 个普通非 hold 音符和 219 个 hold。
- `app_wall_ms` 是 update + render + flush 的 CPU/driver 墙钟，包含驱动执行、调度和等待，不是 GPU elapsed；SurfaceFlinger 的呈现统计单独列出。
- ABBA 的“组均值”是两轮同路径的算术平均；“观察值百分比”是处理组均值相对对照组均值的变化，正数表示更慢。
- 代码路径和行号指向参考实现的冻结快照；这些代码不在生产 `main` 上默认启用。
- 文中的“默认关闭”指生产构建没有可用开关或开关默认 false；研究入口只存在于 `render_research` 作业请求中。

条目索引：

| 条目 | 名称 | 判定 | 关键观察 |
|---|---|---|---|
| N-12 | 可见性预编译 | 负优化 / 无收益 | v1 慢 21.31%，fast 观察值慢 1.27%（漂移大于组差） |
| N-13 | 渲染取值计划 | 确定的负优化 | v1 慢 9.96%，v2 慢 7.45% |
| N-14 | 图集区域合并 | 无稳定收益（观察值慢 0.27%） | poll 172→444，两轮处理落在对照之间 |
| N-15 | 固定/不可变纹理存储 | 质量失败，性能未测 | Case1 60 像素/80 通道/max1 失败，Case2/resize 未执行 |
| N-16 | PBO 字库上传（完整帧） | 无优化 | submit 130.252→18.643/27.714，completed 132.279→136.740/135.232 |
| N-17 | 背景融合 / 低频纹理缓存 | 无优化 | 固定时刻 max RGB 1/255，六轮无稳定 FPS 收益 |

---

## N-12 可见性预编译（visibility bounds）

### 1. 一句话结论与判定

CPU 端在矩阵之前用保守区间证明“这个普通音符的四角一定落在 `[-1,1]^2` 之外”，正确性、缓存、回退和逐次见证都成立；但两版都没有建立整帧收益：v1 组均值 `7.587658 → 9.204597 ms`，慢 `21.31%`；fast 版组均值 `7.609178 → 7.706177 ms`，观察值慢 `1.27%`，而对照前后漂移 `0.420794 ms` 大于组差 `0.096999 ms`，不能证明稳定负优化，只能判定为“无稳定收益”。两版默认关闭，不推广。

判定级别：**负优化 / 无收益（v1 确定负；fast 观察值为负但没有统计稳定性）**。正确性等级：**通过有限窗口的严格几何对照**。性能等级：**不通过**。

### 2. 问题与初衷

普通 gameplay 在 60 秒附近每帧约有 1115 次音符矩阵计算。`aggressive=false` 时，大量未来普通音符仍然会先计算控制高度、alpha、速度、base、`now_transform` 和 model 乘法，然后 `draw_tex_pts` 才用 CPU 四角 AABB 把它排除；GPU 根本没有收到这些 quad。也就是说，排除发生得太晚，CPU 已经为不可见音符付了矩阵和角点变换成本。

直接打开 `aggressive` 会改变可见性语义，可能影响读谱、判定线和透明顺序，不能作为无损优化。于是有了一个更窄的目标：在加载阶段把“静态普通音符”的几何证书编译进缓存；运行期在原 `CtrlObject.set_height`、age/base 检查和原矩阵之前，用当前实际 side model 和样式半宽，证明原 CPU 四角 AABB 必定被拒绝。证明必须保守：只要有一个输入是非有限、非仿射、非 normal、接近边界、证书不匹配或函数不确定，就回原路径。

这个方向的价值假设是：每帧约 1115 次矩阵中，未来音符占相当大的比例；如果一次便宜的区间证明能省掉一次完整矩阵和角点变换，累计应该可见。实际测量证明这个假设不成立，至少在当前谱面、当前设备和当前实现上不成立。

### 3. 实现思路与具体机制

第一版的做法是“向外区间 + 逐 note 证明”：

1. 加载时只接受普通 `NoteKind::Click/Drag/Flick`、非 fake、`height/speed` 有限且 speed 为正、alpha/scale/rotation/translationY 都是默认值、X 是常量（没有 next，keyframes 为空或只有一个且 cursor 为 0）的音符。每个音符保存 `kind`、`above`、`multiple_hint`、`x_bits`、`height_bits`、`speed_bits`。这不是保存动画结果，而是保存“源字段的身份”，运行期每次重新 `capture` 并逐位比较。
2. 每条判定线只接受 `JudgeLineKind::Normal`、无 parent、ctrl 的 alpha/size/pos/y 全默认。非普通线、parent 线、带 ctrl 动画的线整条不进计划。
3. 运行期在 `Note::render` 中，先确认 `visibility_execution.enabled`，再检查证书匹配、ctrl 默认、`incline_sin == 0`、aspect 为 normal 正数、flow_speed 有限正数、base 有限、base 转 f32 有限、base*aspect 有限。全部通过才进入证明。
4. `Side::new` 从当前 `model_stack.last()` 捕获一个 2×3 仿射矩阵；要求所有元素有限、底行严格为 `[0,0,1]`。它保存三份表示：`rows`（每个元素是一个区间点）、`columns`（由 rows 与单位基向量做 dot3 得到）、`raw`（原始 f32 系数，给 fast 版用）。没有分配，没有原生资源。
5. `Side::outside` 复刻源路径的运算图：先做 `0 + x`、`0 + base`，再做 translation 的 dot3；局部坐标不用 `±half` 的代数替换，而是保留 `-half`、`-half + half*2` 的源顺序；最后用 columns 与局部区间做 dot3，得到 horizontal/vertical 两个区间。只有 `horizontal.lo > 1 || horizontal.hi < -1 || vertical.lo > 1 || vertical.hi < -1` 才返回 true。false 表示“没有证明”，必须回原路径；None 表示不支持域，也必须回原路径。
6. 每个区间运算都向外扩一个可表示 f32 步长：`down/up` 对 0 和次正规数做特殊处理；`outward` 把 f64 端点转 f32 后再向外一步；`mul` 用四个端点乘积的 f64 精确范围；`dot3` 同时计算 `(p0+p1)+p2`、`(p1+p2)+p0`、`(p0+p2)+p1` 三种结合并取 hull，从而覆盖分离加法、单次 FMA、链式 FMA、FTZ/DAZ 和不同求和顺序。注释里明确说，这个界包含未舍入的实数中间值，所以不需要假设 LLVM 一定不做 contraction。
7. `outside_range` 是同一证明的“整段”版本：输入是闭区间 `x_lo..x_hi`、`base_lo..base_hi` 和最多 6 组样式半宽；只有所有原四角 AABB 在同一个公共轴上都被拒绝，才返回 true。逐点 OR 不同轴不能证明整段，所以它要求“common reject axis”。这个方法主要给后续 workset/尾段消费者复用，本轮性能实验使用的是逐 note 的 `outside` / `outside_fast`。
8. fast 版不跑区间 dot/hull。它要求 x/base/half extents/raw 系数全部是 normal-or-zero，拒绝 overflow 和接近边界；每个坐标用 f64 的 `centre = a*x + b*base + c`、`radius = |a|*extent_x + |b|*extent_y`，再加一个基于原 f32 算术深度的相对误差预算 `magnitude/65536` 和 FTZ 绝对界 `32*MIN_POSITIVE*(1+|a|+|b|)`。只有 `centre - radius - error > 1` 或 `centre + radius + error < -1` 才拒绝。它每 note 只做两个标量 f64 预算，不做 interval dot。
9. 质量审计不信任“提案”，而是对每个 proved-outside 提案重放原纯矩阵四角 AABB：只有原几何也拒绝，才 `witness`；否则 `false_cull`。性能 cohort 关闭 witness/哈希/TLS 计数，避免审计成本污染墙钟。
10. 缓存使用 `PHRVS001`，头部 112 字节，包含 magic、ABI、源 SHA、body 长度、body SHA；上限 16 MiB。加载时先 `compile` 再 `encode`，用 ABI+source+descriptor 生成 key，走现有 `RenderCache` 的 `begin/read/publish/invalidate`；命中后 `decode` 还会逐 line/note/字段与当前 chart 重新比较，防止伪造、源变化或顺序变化。缓存与 `chart_pre_compile` 共用，Settings 的统一清理/禁用 ticket 会让旧计划失效。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `prpr/src/visibility_bounds.rs` | `Interval`、`down`、`up`、`dot3`、`Side::new`、`Side::outside`、`Side::outside_range`、`Side::outside_fast` | 保守区间算术与三套证明 |
| `prpr/src/render_visibility.rs` | `NotePlan`、`Execution`、`compile`、`encode`、`decode`、`prepare`、`record_witness` | 证书、序列化、缓存、审计 |
| `prpr/src/core/note.rs` | `Note::render` 中的 visibility 分支 | 运行期消费与见证 |
| `prpr/src/core/line.rs` | `Line::render` 中的 `visibility_execution`/`visibility_side` | 线级门控与 side 捕获 |
| `prpr/src/render_compiler.rs` | `pub mod visibility`、`pub mod visibility_bounds` | 把两个文件挂到 `render_compiler` 下 |
| `phira/src/render_research.rs` | `visibility_plan`、`visibility_audit`、`visibility_force_fallback`、`visibility_fast_bounds` | 研究作业开关 |

下面完整粘贴 `visibility_bounds.rs` 的核心算法（第 1 至 305 行，包含 `Interval`、`dot3`、`Side::new`、`outside`、`outside_range`、`outside_fast`）。`#[cfg(test)]` 测试模块不在运行期核心路径，未粘贴；核心算法没有省略。

```rust
// prpr/src/visibility_bounds.rs:1-305
//! Conservative CPU-clip proof for canonical static note quads. It never
//! creates output vertices or changes the original renderer's numeric values.
use crate::core::Matrix;

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f32,
    hi: f32,
}

fn down(value: f32) -> f32 {
    if value == 0. {
        return -f32::from_bits(1);
    }
    f32::from_bits(if value > 0. { value.to_bits() - 1 } else { value.to_bits() + 1 })
}
fn up(value: f32) -> f32 {
    if value == 0. {
        return f32::from_bits(1);
    }
    f32::from_bits(if value > 0. { value.to_bits() + 1 } else { value.to_bits() - 1 })
}
impl Interval {
    fn range(lo: f32, hi: f32) -> Option<Self> {
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return None;
        }
        let mut interval = Self { lo, hi };
        interval.include_zero_for_subnormals();
        Some(interval)
    }
    fn point(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let mut interval = Self { lo: value, hi: value };
        interval.include_zero_for_subnormals();
        Some(interval)
    }
    fn include_zero_for_subnormals(&mut self) {
        if self.lo < f32::MIN_POSITIVE && self.hi > -f32::MIN_POSITIVE {
            self.lo = self.lo.min(0.);
            self.hi = self.hi.max(0.);
        }
    }
    /// One representable f32 step outwards covers conversion rounding and the
    /// much smaller f64-addition rounding. Bounds also contain the unrounded
    /// real intermediate, so contractions/FMA need no assumed extra rounding.
    fn outward(lo: f64, hi: f64) -> Option<Self> {
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return None;
        }
        let lower = lo as f32;
        let upper = hi as f32;
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        let mut result = Self {
            lo: down(lower),
            hi: up(upper),
        };
        if !result.lo.is_finite() || !result.hi.is_finite() {
            return None;
        }
        result.include_zero_for_subnormals();
        Some(result)
    }
    fn hull(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }
    fn add(self, other: Self) -> Option<Self> {
        Self::outward(self.lo as f64 + other.lo as f64, self.hi as f64 + other.hi as f64)
    }
    fn mul(self, other: Self) -> Option<Self> {
        // Products of finite f32 endpoints need at most 48 significand bits,
        // fitting exactly in f64, including underflow-scale products.
        let products = [
            self.lo as f64 * other.lo as f64,
            self.lo as f64 * other.hi as f64,
            self.hi as f64 * other.lo as f64,
            self.hi as f64 * other.hi as f64,
        ];
        let lo = products.into_iter().fold(f64::INFINITY, f64::min);
        let hi = products.into_iter().fold(f64::NEG_INFINITY, f64::max);
        Self::outward(lo, hi)
    }
}

fn dot3(a: [Interval; 3], b: [Interval; 3]) -> Option<Interval> {
    let p = [a[0].mul(b[0])?, a[1].mul(b[1])?, a[2].mul(b[2])?];
    // All three pair-first groups cover both associations and all index
    // orders. Each product bound contains its exact real product as well as
    // rounded/FTZ results; each addition similarly contains exact and rounded
    // results. Thus separate, single-FMA and chained-FMA reductions are covered.
    let left = p[0].add(p[1])?.add(p[2])?;
    let right = p[1].add(p[2])?.add(p[0])?;
    let alternate = p[0].add(p[2])?.add(p[1])?;
    Some(left.hull(right).hull(alternate))
}

/// Captured current side model, not a Chart::update or input-time snapshot.
/// Parent transformations may already be present; only a finite affine matrix
/// is admitted. This object has no allocations or native resources.
#[derive(Clone, Copy, Debug)]
pub struct Side {
    rows: [[Interval; 3]; 2],
    columns: [[Interval; 2]; 2],
    raw: [[f32; 3]; 2],
}
impl Side {
    pub fn new(parent: &Matrix) -> Option<Self> {
        if !parent.iter().all(|value| value.is_finite()) || parent[(2, 0)] != 0. || parent[(2, 1)] != 0. || parent[(2, 2)] != 1. {
            return None;
        }
        let rows = [
            [
                Interval::point(parent[(0, 0)])?,
                Interval::point(parent[(0, 1)])?,
                Interval::point(parent[(0, 2)])?,
            ],
            [
                Interval::point(parent[(1, 0)])?,
                Interval::point(parent[(1, 1)])?,
                Interval::point(parent[(1, 2)])?,
            ],
        ];
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let columns = [
            [dot3(rows[0], [one, zero, zero])?, dot3(rows[0], [zero, one, zero])?],
            [dot3(rows[1], [one, zero, zero])?, dot3(rows[1], [zero, one, zero])?],
        ];
        let raw = [
            [parent[(0, 0)], parent[(0, 1)], parent[(0, 2)]],
            [parent[(1, 0)], parent[(1, 1)], parent[(1, 2)]],
        ];
        Some(Self { rows, columns, raw })
    }

    /// True proves that the original four-corner CPU AABB is wholly outside
    /// [-1,1]^2. False means no proof: execute the original renderer. None is an
    /// unsupported/non-finite/overflow domain and also requires that fallback.
    /// Caller certifies rotation0, unit scale and the canonical note behavior.
    pub fn outside(&self, x: f32, base: f32, hf: [f32; 2]) -> Option<bool> {
        if !x.is_finite() || !base.is_finite() || !hf.iter().all(|value| value.is_finite() && *value >= 0.) {
            return None;
        }
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let two = Interval::point(2.)?;
        // The source canonical append_translation performs 0+x/0+base.
        // Preserve the operations as intervals rather than assigning raw x/y.
        let tx = zero.add(Interval::point(x)?)?;
        let ty = zero.add(Interval::point(base)?)?;
        let translation = [dot3(self.rows[0], [tx, ty, one])?, dot3(self.rows[1], [tx, ty, one])?];

        // Source draw_tex corners use -half and (-half + half*2), not an
        // algebraic +/-half replacement. Their rectangle hull encloses all
        // four source corners including f32 rounding and possible FTZ.
        let hx = Interval::point(hf[0])?;
        let hy = Interval::point(hf[1])?;
        let left = hx.neg();
        let top = hy.neg();
        let right = left.add(hx.mul(two)?)?;
        let bottom = top.add(hy.mul(two)?)?;
        let local = [left.hull(right), top.hull(bottom)];
        let horizontal = dot3([self.columns[0][0], self.columns[0][1], translation[0]], [local[0], local[1], one])?;
        let vertical = dot3([self.columns[1][0], self.columns[1][1], translation[1]], [local[0], local[1], one])?;
        // Affine bottom row remains exactly [0,0,1] in the source matrix
        // product: all other products are finite * zero. Homogeneous point
        // division, if used by nalgebra, is by exactly one.
        Some(horizontal.lo > 1. || horizontal.hi < -1. || vertical.lo > 1. || vertical.hi < -1.)
    }

    /// True proves that every original four-corner CPU AABB for the closed
    /// x/base domain and every supplied style is outside [-1,1]^2 on one
    /// common axis. A per-point OR of different reject axes cannot prove a
    /// whole tail. False requires the original renderer; None also falls back
    /// for an unsupported/non-finite/overflow domain. Styles are bounded to
    /// 1..=6 finite nonnegative half extents, matching the canonical textures.
    ///
    /// Caller certifies rotation0, unit scale and canonical note behavior, and
    /// supplies bounds containing the actual source f32 x/base values. In
    /// particular this method does not replace the caller's f64 height/base
    /// arithmetic. Every operation below is inclusion-monotone: shrinking
    /// x/base intervals or removing styles cannot invalidate a true proof.
    pub fn outside_range(&self, x_lo: f32, x_hi: f32, base_lo: f32, base_hi: f32, hfs: &[[f32; 2]]) -> Option<bool> {
        if hfs.is_empty() || hfs.len() > 6 {
            return None;
        }
        let x = Interval::range(x_lo, x_hi)?;
        let base = Interval::range(base_lo, base_hi)?;
        let zero = Interval::point(0.)?;
        let one = Interval::point(1.)?;
        let two = Interval::point(2.)?;
        // Same append_translation and matrix product graph as `outside`.
        // Range operands also contain input DAZ results; outward arithmetic
        // contains rounded, unrounded/FMA and generated FTZ intermediates.
        let tx = zero.add(x)?;
        let ty = zero.add(base)?;
        let translation = [dot3(self.rows[0], [tx, ty, one])?, dot3(self.rows[1], [tx, ty, one])?];

        let mut local: Option<[Interval; 2]> = None;
        for &hf in hfs {
            if !hf.iter().all(|value| value.is_finite() && *value >= 0.) {
                return None;
            }
            let hx = Interval::point(hf[0])?;
            let hy = Interval::point(hf[1])?;
            let left = hx.neg();
            let top = hy.neg();
            // Keep -half + half*2 in its source order, including separate
            // rounding/FTZ or contraction. Never substitute +/-half.
            let right = left.add(hx.mul(two)?)?;
            let bottom = top.add(hy.mul(two)?)?;
            let style = [left.hull(right), top.hull(bottom)];
            local = Some(match local {
                Some(bounds) => [bounds[0].hull(style[0]), bounds[1].hull(style[1])],
                None => style,
            });
        }
        let local = local?;
        let horizontal = dot3([self.columns[0][0], self.columns[0][1], translation[0]], [local[0], local[1], one])?;
        let vertical = dot3([self.columns[1][0], self.columns[1][1], translation[1]], [local[0], local[1], one])?;
        // All intermediate bounds must remain finite, including zero times
        // an otherwise overflowing value. Side::new already excludes a
        // projective row; homogeneous division therefore remains by one.
        Some(horizontal.lo > 1. || horizontal.hi < -1. || vertical.lo > 1. || vertical.hi < -1.)
    }

    /// Faster opt-in proof with a narrower domain than `outside`. Raw parent
    /// coefficients and x/base/half extents must be normal-or-zero. The caller
    /// also certifies the canonical note and a normal positive aspect so its
    /// original zero/aspect operation cannot become 0/0 under DAZ.
    ///
    /// In this domain the parent*unit-note first two columns equal the captured
    /// parent coefficients numerically: multiplication by 1/0 and addition of
    /// signed zero introduce no error in a normal nonzero coefficient. For
    /// each coordinate, translation dot3 followed by point dot3 uses fewer than
    /// 16 f32 rounds; contraction/FMA removes rounds. For u=2^-24,
    /// gamma16=16*u/(1-16*u) is slightly above 2^-20 and below 2^-19.
    /// A 2^-16 relative budget is over 15 times larger, leaving ample allowance
    /// for the f64 centre/radius/bound arithmetic and all summation orders.
    ///
    /// Every generated f32 subnormal/FTZ loss is < MIN_POSITIVE. There are fewer
    /// than 16 such steps, translation error enters the point with coefficient
    /// 1, and the columns cannot generate a nonzero subnormal in this domain.
    /// 32*MIN_POSITIVE*(1+|a|+|b|) additionally covers potential local-coordinate
    /// underflow/DAZ propagation. This is a magnitude-dependent numeric budget,
    /// never a fixed screen/pixel padding. No allocations or interval dots run
    /// per note; only two scalar f64 coordinate budgets are evaluated.
    pub fn outside_fast(&self, x: f32, base: f32, hf: [f32; 2]) -> Option<bool> {
        let normal_or_zero = |value: f32| value == 0. || value.is_normal();
        if !normal_or_zero(x)
            || !normal_or_zero(base)
            || !hf.iter().all(|&value| normal_or_zero(value) && value >= 0.)
            || !self.raw.iter().flatten().all(|&value| normal_or_zero(value))
        {
            return None;
        }
        // Match the source corner operations even though, for normal halves
        // without overflow, these binary doublings/subtractions are exact.
        let left = -hf[0];
        let top = -hf[1];
        let width = hf[0] * 2.;
        let height = hf[1] * 2.;
        let right = left + width;
        let bottom = top + height;
        if ![left, top, width, height, right, bottom].iter().all(|value| value.is_finite()) {
            return None;
        }
        let extent_x = left.abs().max(right.abs()) as f64;
        let extent_y = top.abs().max(bottom.abs()) as f64;
        let x = x as f64;
        let base = base as f64;
        let mut outside = false;
        for &[a, b, c] in &self.raw {
            let (a, b, c) = (a as f64, b as f64, c as f64);
            let centre = a * x + b * base + c;
            let radius = a.abs() * extent_x + b.abs() * extent_y;
            let magnitude = a.abs() * (x.abs() + extent_x) + b.abs() * (base.abs() + extent_y) + c.abs();
            // All source products, translation sums and point sums are then
            // separated from f32 overflow by over a factor of 31. Rejection
            // avoids trying to reason about infinity/NaN cancellation.
            if !magnitude.is_finite() || magnitude >= f32::MAX as f64 / 32. {
                return None;
            }
            let error = magnitude / 65_536. + 32. * f32::MIN_POSITIVE as f64 * (1. + a.abs() + b.abs());
            let lower = centre - radius - error;
            let upper = centre + radius + error;
            if ![centre, radius, error, lower, upper].iter().all(|value| value.is_finite()) {
                return None;
            }
            outside |= lower > 1. || upper < -1.;
        }
        Some(outside)
    }
}
```

下面是 `render_visibility.rs` 中证书捕获、编译、解码和准备的完整核心片段（第 71 至 110 行、341 至 579 行）。序列化细节和测试模块未展开，但所有运行期决策函数都在这里。

```rust
// prpr/src/render_visibility.rs:71-110
impl NotePlan {
    fn capture(note: &Note) -> Option<Self> {
        if note.fake
            || !note.height.is_finite()
            || !note.speed.is_finite()
            || note.speed <= 0.
            || !note.object.alpha.is_default()
            || !note.object.scale.0.is_default()
            || !note.object.scale.1.is_default()
            || !note.object.rotation.is_default()
            || !note.object.translation.1.is_default()
        {
            return None;
        }
        Some(Self {
            kind: kind(note)?,
            above: note.above,
            multiple_hint: note.multiple_hint,
            x_bits: static_x(note)?.to_bits(),
            height_bits: note.height.to_bits(),
            speed_bits: note.speed.to_bits(),
        })
    }
    /// Recheck public mutable source fields on every actual attempt. Fixed X
    /// never invokes its tween; no custom-function identity is trusted.
    #[inline]
    pub fn matches(&self, note: &Note) -> bool {
        Self::capture(note).is_some_and(|current| current == *self)
    }
    pub fn x(&self) -> f32 {
        f32::from_bits(self.x_bits)
    }
    pub fn height(&self) -> f64 {
        f64::from_bits(self.height_bits)
    }
    pub fn speed(&self) -> f64 {
        f64::from_bits(self.speed_bits)
    }
}
#[inline]
pub fn ctrl_default(ctrl: &CtrlObject) -> bool {
    ctrl.alpha.is_default() && ctrl.size.is_default() && ctrl.pos.is_default() && ctrl.y.is_default()
}
```

```rust
// prpr/src/render_visibility.rs:341-579
fn charge(budget: &mut usize, extra: usize) -> Result<()> {
    *budget = budget.checked_add(extra).context("Visibility metadata overflow")?;
    ensure!(*budget <= LIMIT, "Visibility plan exceeds 16MiB nominal metadata budget");
    Ok(())
}
fn line_eligible(line: &crate::core::JudgeLine) -> bool {
    matches!(line.kind, JudgeLineKind::Normal) && line.parent.is_none() && ctrl_default(&line.ctrl_obj.borrow())
}
pub fn compile(source: [u8; 32], chart: &Chart) -> Result<Plan> {
    ensure!(chart.lines.len() <= u32::MAX as usize, "Visibility line count");
    let mut budget = size_of::<Plan>();
    charge(
        &mut budget,
        chart
            .lines
            .len()
            .checked_mul(size_of::<LinePlan>())
            .context("Visibility line storage overflow")?,
    )?;
    let mut lines = Vec::with_capacity(chart.lines.len());
    for line in &chart.lines {
        ensure!(line.notes.len() <= u32::MAX as usize, "Visibility note count");
        charge(
            &mut budget,
            line.notes
                .len()
                .checked_mul(size_of::<Option<NotePlan>>())
                .context("Visibility note storage overflow")?,
        )?;
        let eligible = line_eligible(line);
        let notes = line
            .notes
            .iter()
            .map(|note| if eligible { NotePlan::capture(note) } else { None })
            .collect();
        lines.push(LinePlan { notes });
    }
    Ok(Plan { source, lines })
}
fn write32(body: &mut Vec<u8>, value: usize) -> Result<()> {
    body.extend_from_slice(&u32::try_from(value)?.to_le_bytes());
    ensure!(body.len() <= LIMIT - HEADER, "Visibility serial budget");
    Ok(())
}
pub fn encode(plan: &Plan) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    write32(&mut body, plan.lines.len())?;
    for (line_id, line) in plan.lines.iter().enumerate() {
        write32(&mut body, line_id)?;
        write32(&mut body, line.notes.len())?;
        for (note_id, note) in line.notes.iter().enumerate() {
            write32(&mut body, note_id)?;
            body.push(u8::from(note.is_some()));
            if let Some(note) = note {
                body.push(note.kind);
                body.push(u8::from(note.above));
                body.push(u8::from(note.multiple_hint));
                body.extend_from_slice(&note.x_bits.to_le_bytes());
                body.extend_from_slice(&note.height_bits.to_le_bytes());
                body.extend_from_slice(&note.speed_bits.to_le_bytes());
            }
            ensure!(body.len() <= LIMIT - HEADER, "Visibility serial budget");
        }
    }
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&abi());
    bytes.extend_from_slice(&plan.source);
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(&body));
    bytes.extend_from_slice(&body);
    Ok(bytes)
}
struct Reader<'a> {
    data: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.cursor.checked_add(count).context("Visibility cursor overflow")?;
        ensure!(end <= self.data.len(), "Visibility cache truncated");
        let bytes = &self.data[self.cursor..end];
        self.cursor = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}
pub fn decode(bytes: &[u8], source: [u8; 32], chart: &Chart) -> Result<Plan> {
    ensure!((HEADER..=LIMIT).contains(&bytes.len()), "Visibility serial size limit");
    ensure!(&bytes[..8] == MAGIC && bytes[8..40] == abi() && bytes[40..72] == source, "Visibility source/ABI mismatch");
    ensure!(u64::from_le_bytes(bytes[72..80].try_into().unwrap()) == (bytes.len() - HEADER) as u64, "Visibility serial length");
    ensure!(bytes[80..112] == Sha256::digest(&bytes[HEADER..])[..], "Visibility checksum mismatch");
    let mut reader = Reader {
        data: &bytes[HEADER..],
        cursor: 0,
    };
    ensure!(reader.u32()? == chart.lines.len(), "Visibility source line count changed");
    let mut budget = size_of::<Plan>();
    charge(
        &mut budget,
        chart
            .lines
            .len()
            .checked_mul(size_of::<LinePlan>())
            .context("Visibility line storage overflow")?,
    )?;
    let mut lines = Vec::with_capacity(chart.lines.len());
    for (line_id, line) in chart.lines.iter().enumerate() {
        ensure!(reader.u32()? == line_id && reader.u32()? == line.notes.len(), "Visibility source note shape/order changed");
        charge(
            &mut budget,
            line.notes
                .len()
                .checked_mul(size_of::<Option<NotePlan>>())
                .context("Visibility note storage overflow")?,
        )?;
        let eligible = line_eligible(line);
        let mut notes = Vec::with_capacity(line.notes.len());
        for (note_id, note) in line.notes.iter().enumerate() {
            ensure!(reader.u32()? == note_id, "Visibility note identity changed");
            let tag = reader.byte()?;
            ensure!(tag <= 1, "Visibility option tag");
            let stored = if tag == 1 {
                let kind = reader.byte()?;
                let above = reader.byte()?;
                let multiple = reader.byte()?;
                ensure!((1..=3).contains(&kind) && above <= 1 && multiple <= 1, "Visibility static role flags");
                Some(NotePlan {
                    kind,
                    above: above != 0,
                    multiple_hint: multiple != 0,
                    x_bits: reader.u32()? as u32,
                    height_bits: reader.u64()?,
                    speed_bits: reader.u64()?,
                })
            } else {
                None
            };
            let actual = if eligible { NotePlan::capture(note) } else { None };
            ensure!(stored == actual, "Visibility source certificate changed/forged");
            notes.push(stored);
        }
        lines.push(LinePlan { notes });
    }
    ensure!(reader.cursor == reader.data.len(), "Visibility trailing bytes");
    Ok(Plan { source, lines })
}
pub fn preparation(chart: &Chart) -> Value {
    chart.compiled_visibility.borrow().report.clone()
}
pub fn report(chart: &Chart) -> Value {
    json!({"preparation":preparation(chart),"runtime":runtime_stats()})
}
pub fn prepare(chart: &Chart, res: &Resource, source_bytes: &[u8]) -> Value {
    *chart.compiled_visibility.borrow_mut() = State::default();
    let opts = execution();
    if !res.config.chart_pre_compile || !opts.enabled {
        let info = json!({"requested":opts.enabled,"enabled":false,"installed":false,"cache_read":false,"cache_write":false});
        chart.compiled_visibility.borrow_mut().report = info.clone();
        return info;
    }
    let begun = Instant::now();
    let mut cache_read = false;
    let mut cache_write = false;
    let result = (|| -> Result<(Plan, Value)> {
        super::set_enabled(res.config.chart_pre_compile);
        let manager = super::cache().context("Host has not configured compiler cache")?;
        let source: [u8; 32] = Sha256::digest(source_bytes).into();
        let descriptor = {
            let plan = compile(source, chart)?;
            encode(&plan)?
        };
        let mut hash = Sha256::new();
        hash.update(abi());
        hash.update(source);
        hash.update(&descriptor[HEADER..]);
        let key = format!("{:x}", hash.finalize());
        let ticket = manager.begin(&key)?.context("Compiler cache disabled")?;
        cache_read = true;
        let mut hit = false;
        let mut error = Value::Null;
        let cached = match manager.read(&ticket) {
            Ok(Some(bytes)) => match decode(&bytes, source, chart) {
                Ok(plan) => {
                    hit = true;
                    Some(plan)
                }
                Err(cause) => {
                    error = json!(format!("{cause:#}"));
                    manager.invalidate_blob(&ticket)?;
                    None
                }
            },
            Ok(None) => None,
            Err(cause) => {
                error = json!(format!("{cause:#}"));
                manager.invalidate_blob(&ticket)?;
                None
            }
        };
        let mut published = false;
        let plan = if let Some(plan) = cached {
            plan
        } else {
            cache_write = true;
            match manager.publish(&ticket, &descriptor) {
                Ok(value) => published = value,
                Err(cause) => error = json!(format!("{cause:#}")),
            };
            decode(&descriptor, source, chart)?
        };
        ensure!(manager.is_current(&ticket), "Visibility compilation invalidated by clear/disable");
        let info = json!({"requested":true,"enabled":true,"installed":true,"cache_hit":hit,"published":published,"key":key,"bytes":descriptor.len(),"cache_error":error,
            "notes":plan.lines.iter().map(|line|line.notes.len()).sum::<usize>(),"eligible":plan.lines.iter().flat_map(|line|&line.notes).filter(|note|note.is_some()).count(),
            "metadata_budget":LIMIT,"scope":"Persisted static note visibility certificates; frame geometry/resources remain live. No GPU capability/high-quality gate. No input/audio clocks or animation results cached."});
        Ok((plan, info))
    })();
    let mut info = match result {
        Ok((plan, info)) => {
            chart.compiled_visibility.borrow_mut().plan = Some(plan);
            info
        }
        Err(cause) => json!({"requested":true,"enabled":true,"installed":false,"fallback":format!("{cause:#}")}),
    };
    info["cache_read"] = json!(cache_read);
    info["cache_write"] = json!(cache_write);
    info["prepare_ms"] = json!(begun.elapsed().as_secs_f64() * 1000.);
    chart.compiled_visibility.borrow_mut().report = info.clone();
    info
}
```

运行期消费点在 `prpr/src/core/note.rs:404-458`。它的顺序很重要：先过原 Fadeout/age/base 检查，再取证书，再捕获 `Side`，最后才做证明；证明为 true 时直接 `return`，不创建顶点、不调用原矩阵。

```rust
// prpr/src/core/note.rs:404-458
        crate::render_compiler::visibility::record_attempt("visited", config.visibility_execution.audit);
        #[cfg(feature = "chart_pre_compile")]
        if config.visibility_execution.enabled {
            use crate::render_compiler::visibility;
            let audit = config.visibility_execution.audit;
            let certified = !config.visibility_execution.forced
                && config.visibility_note.is_some_and(|plan| plan.matches(self))
                && visibility::ctrl_default(ctrl_obj)
                && config.incline_sin == 0.
                && res.aspect_ratio.is_normal()
                && res.aspect_ratio > 0.
                && res.config.flow_speed.is_finite()
                && res.config.flow_speed > 0.
                && base.is_finite()
                && (base as f32).is_finite()
                && ((base as f32) * res.aspect_ratio).is_finite();
            if certified {
                visibility::record_attempt("eligible", audit);
                let tex = match self.kind {
                    NoteKind::Click => Some(*style.click),
                    NoteKind::Drag => Some(*style.drag),
                    NoteKind::Flick => Some(*style.flick),
                    _ => None,
                };
                if let (Some(side), Some(tex)) = (config.visibility_side, tex) {
                    let hf = [scale, tex.height() * scale / tex.width()];
                    let x = self.object.translation.0.now();
                    let proof = if config.visibility_execution.fast_bounds {
                        side.outside_fast(x, base as f32, hf)
                    } else {
                        side.outside(x, base as f32, hf)
                    };
                    if proof == Some(true) {
                        // Quality-only witness repeats pure geometry, never a
                        // stateful tween. Production frames perform no witness.
                        let mut witnessed = true;
                        if audit {
                            let combined = res.model_stack.last().unwrap() * self.now_transform(res, ctrl_obj, base as f32, config.incline_sin);
                            let (x, y, w, h) = (-hf[0], -hf[1], hf[0] * 2., hf[1] * 2.);
                            let p = [Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)]
                                .map(|point| combined.transform_point(&point));
                            witnessed = p.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) > 1.
                                || p.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) < -1.
                                || p.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) > 1.
                                || p.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) < -1.;
                            visibility::record_witness(witnessed, audit);
                        }
                        if witnessed {
                            visibility::record_attempt("proved_outside", audit);
                            return;
                        }
                    }
                }
            }
            visibility::record_attempt("fallback", audit);
        }
```

线级门控在 `prpr/src/core/line.rs:354-365` 和 `prpr/src/core/line.rs:583-589`：只有普通线、无 parent、无 incline 才保留 `visibility_execution.enabled`；`Side` 从当前 `model_stack.last()` 捕获，且只在启用时创建。

```rust
// prpr/src/core/line.rs:354-365
        let visibility_state = chart.compiled_visibility.borrow();
        let visibility_line = visibility_state.plan.as_ref().and_then(|plan| plan.lines.get(id));
        let mut visibility_execution = crate::render_compiler::visibility::execution();
        if !matches!(self.kind, JudgeLineKind::Normal) || self.parent.is_some() || !self.incline.is_default() {
            visibility_execution.enabled = false;
        }
        visibility_execution.enabled &= visibility_line.is_some();
```

```rust
// prpr/src/core/line.rs:583-589
            let workset = workset.filter(|_| !agg && !config.visibility_execution.enabled && !config.visibility_execution.audit);
            let workset_side = workset.and_then(|_| crate::render_compiler::visibility_bounds::Side::new(res.model_stack.last().unwrap()));
            config.visibility_side = if config.visibility_execution.enabled {
                crate::render_compiler::visibility_bounds::Side::new(res.model_stack.last().unwrap())
            } else {
                None
            };
```

### 5. 开关、默认值、构建特性

| 开关/特性 | 默认 | 作用 |
|---|---|---|
| `render_research` feature | 生产构建关闭 | 只有研究后端编译 `visibility_plan` 请求 |
| `chart_pre_compile` feature | 生产默认可能启用，但研究消费者仍默认关闭 | 提供共享 `RenderCache` 与 Config `chart_pre_compile` 门 |
| `visibility_plan` 请求键 | false | 打开证书消费者 |
| `visibility_audit` 请求键 | false | 打开逐次 witness、顶点/控制时钟哈希 |
| `visibility_force_fallback` 请求键 | false | 强制所有尝试回原路径，用于回退对照 |
| `visibility_fast_bounds` 请求键 | false | 从 `outside` 切到 `outside_fast` |
| `Config::chart_pre_compile` | 默认 false | 关闭时不读/不写 cache，不创建计划 |
| 生产 Config/UI 开关 | 无 | 本轮没有把 visibility 计划接进普通设置 |

所有 build（Windows 默认、Windows 研究、Android 默认、Android 研究）默认关闭消费者；研究请求可以复现。缓存与 `chart_pre_compile` 共用同一个 `RenderCache`、同一个 16 MiB 名义预算、同一个 Settings 清理/禁用路径；关闭 `chart_pre_compile` 会让 `prepare` 直接返回 `enabled:false`，不读不写。

### 6. 完整测量数据（表格）

环境：Mali-T880 / ES 3.2 / 2560×1440；谱面 `What do you want more than a Happy ending?`；低画质 `sampleCount=1`；`aggressive=false`；120 预热 + 1200 采样帧；ABBA 顺序 `off/on/on/off`；性能 cohort 关闭 audit、witness、哈希和 TLS 计数。v1 使用 `outside`，fast 使用 `outside_fast`。

v1 逐轮数据：

| 次序 | 路径 | 应用均值 ms | p50 ms | p99 ms | max ms | >16.67ms | >20ms | chart 均值 ms | update 均值 ms | render 均值 ms | flush 均值 ms | SF FPS | SF p99 ms | SF max ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 7.468366 | 16.7417 | 14.518174 | 16.985938 | 4 | 0 | 3.353115 | 0.389724 | 6.406467 | 0.672175 | 59.7536 | 16.7978 | 18.1729 |
| 1 | on | 9.092444 | 16.7302 | 16.327104 | 20.831251 | 10 | 3 | 5.238013 | 0.373092 | 8.086166 | 0.633187 | 59.6471 | 16.7939 | 33.5125 |
| 2 | on | 9.316750 | 16.7281 | 16.077036 | 20.385937 | 9 | 2 | 5.410865 | 0.370477 | 8.304224 | 0.642049 | 59.7519 | 16.7832 | 16.8208 |
| 3 | off | 7.706950 | 16.7323 | 15.254610 | 21.148438 | 5 | 1 | 3.491320 | 0.408924 | 6.612262 | 0.685763 | 59.7486 | 16.8130 | 16.9141 |
| 组均值 | off / on | 7.587658 / 9.204597 |  |  |  |  |  | 3.422218 / 5.324439 | 0.399324 / 0.371785 | 6.509365 / 8.195195 | 0.678969 / 0.637618 | 59.7511 / 59.6995 |  |  |

v1 组差值 `+1.616939 ms`，观察值 `+21.3101%`；对照前后漂移 `A2-A1 = +0.238584 ms`，小于组差，因此 v1 的负收益是确定的，不是控制漂移。

fast 版逐轮数据：

| 次序 | 路径 | 应用均值 ms | p50 ms | p99 ms | max ms | >16.67ms | >20ms | chart 均值 ms | update 均值 ms | render 均值 ms | flush 均值 ms | SF FPS | SF p99 ms | SF max ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 7.398781 | 16.7240 | 15.424234 | 18.549480 | 8 | 0 | 3.440490 | 0.376941 | 6.368673 | 0.653167 | 59.7470 | 16.7746 | 18.9464 |
| 1 | on | 7.703952 | 16.7328 | 14.233656 | 20.086459 | 6 | 1 | 3.545398 | 0.404704 | 6.624056 | 0.675191 | 59.7448 | 16.7821 | 16.8401 |
| 2 | on | 7.708401 | 16.7286 | 14.484072 | 18.895312 | 7 | 0 | 3.552762 | 0.403708 | 6.635895 | 0.668799 | 59.7447 | 16.7803 | 16.8729 |
| 3 | off | 7.819575 | 16.7151 | 15.202750 | 20.959896 | 6 | 1 | 3.652622 | 0.400352 | 6.754856 | 0.664366 | 59.7485 | 16.7643 | 16.8208 |
| 组均值 | off / on | 7.609178 / 7.706177 |  |  |  |  |  | 3.546556 / 3.549080 | 0.388647 / 0.404206 | 6.561765 / 6.629976 | 0.658767 / 0.671995 | 59.7478 / 59.7448 |  |  |

fast 组差值 `+0.096999 ms`，观察值 `+1.2748%`；对照前后漂移 `A2-A1 = +0.420794 ms`，大于组差。因此 fast 版只能记为“观察值慢 1.27%，但没有建立稳定收益或稳定负优化”，不能把 1.27% 当作确定结论。

质量 cohort 的 8 个作业：

| 变体 | 计划状态 | 字节 | prepare ms | 证书音符 | proved_outside | fallback | actual_push | false_cull | RGBA 对照 |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| cold-on | 冷编译并发布 | 39175 | 93.163 | 1349 | 212855 | 54697 | 3464 | 0 | 通过 |
| hot-on | 热命中 | 39175 | 79.770 | 1349 | 212855 | 54697 | 3464 | 0 | 通过 |
| off | 未安装 | 0 |  | 0 | 0 | 0 | 3464 | 0 | 参考 |
| forced-fallback | 计划安装但强制回退 | 39175 | 79.063 | 1349 | 0 | 267552 | 3464 | 0 | 通过 |
| high-on | 高画质计划安装 | 39175 | 81.159 | 1349 | 212855 | 54697 | 3464 | 0 | 通过 |
| high-off | 高画质未安装 | 0 |  | 0 | 0 | 0 | 3464 | 0 | 参考 |
| clear-on | 清理后重建 | 39175 | 97.850 | 1349 | 212855 | 54697 | 3464 | 0 | 通过 |
| corrupt-on | 损坏后重建 | 39175 | 83.098 | 1349 | 212855 | 54697 | 3464 | 0 | 通过 |

运行期计数（每个质量作业 240 帧，v1 与 fast 相同）：

| 计数 | 数值 | 说明 |
|---|---:|---|
| `visited` | 267552 | 进入 `Note::render` 且通过前置可见性/age 检查的尝试 |
| `eligible` | 214992 | 证书、ctrl、incline、aspect、base 门都通过的尝试 |
| `proved_outside` | 212855 | 区间证明为 true 且 witness 通过 |
| `fallback` | 54697 | 未认证、证明 false 或域不支持，回原路径 |
| `original_quad_reject` | 77465 | 原 CPU 四角 AABB 拒绝数（off 路径） |
| `actual_push` | 3464 | 实际进入 NoteBuffer 的 quad |
| `witness` | 212855 | 原纯矩阵四角重放数 |
| `false_cull` | 0 | 见证失败数 |
| `proved_outside / visited` | 79.56% | 约占访问量 |
| `proved_outside / 240` | 886.90 | 每帧约 887 次提前拒绝 |

缓存与几何：`PHRVS001`，39,175 字节，key `af63e9b7b9a9824e4cb216a2b0a443fe8a3b89fb4ae9339103c7120edb4df0fa`，blob SHA `bf2593ac0d619cd9aeeb41d065863a0ac78c4798e11445b9744ecf2103eaf3e4`；24 条线、1568 个音符、1349 个证书。质量作业还核对了 3464 个实际入队 quad（共 13,856 个顶点）的 pos/UV/color 位模式、材质 rank 顺序、5760 个逐 Line 结束控制时钟、job 末源 clock 快照；这些哈希与 off 全同。

### 7. 质量与回归结果

通过的部分：

- 8 个真机质量作业形成 6 组有效末帧 RGBA 对照，全部逐像素相同；高画质、关闭、强制回退、清理后重建、损坏后重建都实际执行。
- 每个 proved-outside 提案都有原几何见证：212,855 次提案全部被原四角 AABB 拒绝，`false_cull=0`。这不是“相信区间”，而是每次都用原纯矩阵重放。
- 实际入队顶点/UV/颜色位模式、材质 rank 顺序、逐 Line 结束控制时钟、job 末源 clock 快照与 off 全同；没有改变绘制顺序、判定、音轨或输入时钟。
- 缓存冷发布、热命中、关闭不读写、清理后重建、损坏后重建全部执行；源 ZIP 保留；高画质和强制缺能力都走回退。
- fast 版的 `Runtime.fast_bounds` 实际为 true，且 12 个新 CPU oracle + 1 个旧测试通过；覆盖 16k 原 nalgebra oracle、8k FMA/FTZ 和边界/CRC/mutation。

没有证明的部分：

- 只覆盖有限谱面、有限帧和当前 Mali-T880；不是全曲、所有帧、所有源状态、所有 GPU 的证明。
- 质量审计记录的是有序提交的 CPU 顶点、语义角色和材质 rank，不包含原生 texture identity 或硬件浮点误差上界。
- v1 的区间证明是 CPU 数学；GPU 窗口/裁剪、驱动精度、纹理采样不在证明范围内。
- 性能 cohort 关闭 witness/哈希/TLS 计数，所以性能数据不含审计成本；质量数据不能反过来当性能。
- 高画质作业只证明计划能安装并回退/对照，不证明高画质有收益。

### 8. 为什么判定为无/负（逐项对比）

第一，v1 的每 note 证明比它省下的矩阵更贵。`outside` 对每个 eligible note 要做 translation 的两次 dot3、local 的两个区间、horizontal/vertical 的两次 dot3；每次 dot3 又包含 3 次区间乘、3 次区间加、3 种结合 hull。虽然 `dot3` 只在 `Side::new` 里预算 columns，但运行期每个 note 仍有多次区间运算。结果是 chart 均值从 `3.353/3.491` 升到 `5.238/5.411`，每轮多 `1.88/1.92 ms`；render 均值从 `6.406/6.612` 升到 `8.086/8.304`。这正好说明“证明成本”落在 chart/render 内，且大于省下的矩阵。

第二，v1 的 212,855 次提前拒绝并不等于 212,855 次本来会进入 GPU 的绘制。`actual_push` 仍是 3464，和 off 相同；被拒绝的音符本来也会在 `draw_tex_pts` 的 CPU AABB 被排除，GPU 从未收到它们。优化省掉的只是“原矩阵 + 原角点变换”，不是 draw call、不是 GPU 顶点、不是纹理上传。以每帧 887 次拒绝、每次省一次 3×3 矩阵和四角变换估算，省下的 CPU 工作很小；而每次证明的区间运算和证书检查更贵，净结果为负。

第三，fast 版把证明降到两个标量 f64 预算，chart 只比 off 多 `0.105/0.112 ms`，说明证明成本确实大幅下降；但整帧仍多 `0.305/0.309 ms`，组均值多 `0.097 ms`。这部分增量落在 render 的 guard/证书/TLS 和 side 捕获上：每个 note 仍要 `plan.matches(self)`（重新 capture 并比较 6 个字段）、`ctrl_default`、多个 finite 检查，然后才走两个标量。省下的矩阵不足以抵消这些固定检查。

第四，fast 版的 1.27% 不能当确定负优化。两轮 off 是 `7.398781/7.819575`，自身相差 `0.420794 ms`；两轮 on 是 `7.703952/7.708401`，彼此只差 `0.004449 ms`。组差 `0.096999 ms` 小于 off 漂移，说明当前窗口的设备状态/调度噪声大于处理差。正确表述是“没有建立稳定收益”，而不是“证明了 1.27% 的负优化”。

第五，两个版本都没有改变真正的瓶颈。整帧约 17 ms，chart/render 只占其中一部分；HUD、glyph 上传、粒子、背景、驱动等待都在同一帧内。可见性预编译只影响 chart 阶段的未来音符矩阵，且只在 CPU 侧；它不可能改善 HUD 长帧、glyph 纹理同步或 GPU 提交。即使把 chart 阶段的矩阵全部省掉，整帧的上界收益也远小于控制漂移。

第六，v1 与 fast 的差异本身给出了方向：区间 dot/hull 太贵，标量预算便宜但固定 guard 仍在。若没有把证明批量化、把 guard 从每 note 降到每 line/每段，单 note 证明很难在 17 ms 帧里跑赢原矩阵。这个结论与“正确性成立”并不矛盾：正确性证明可以保留，性能路径不应默认启用。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：v1 本地提交 `9900c6845825809d3bf511f2c0b600e3241579f8`；fast 本地提交 `8a71f2f39215a068572ab12d76aa291726946d8d`；两版各 41 个冻结源文件 |
| 默认 | 关闭；所有 build 默认关闭；研究请求 `visibility_plan` 可复现 |
| 普通路径 | 未接入；没有生产 Config/UI 开关，没有默认推广 |
| 主线是否已实施 | 否（main） |
| 研究后端 | v1 SO `fb4284f2bbf93e0e6698e06bfe98ec9158ed612800775f05c1452591ce6d9cf2`；fast SO `ec6c5c26b2120a2de2d8f2e3bc7e6f1584506c8fec64c10266dddb81cb250149`；均未推送主项目 |
| 质量/契约 | 8 个质量作业、6 组 RGBA、缓存生命周期和回退对照完成；v1/fast 的 performance cohort 都完整结束 |
| 后续 | 设备/构建作业已结束，没有遗留的可见性设备任务；不推进 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `Side`/`Interval`/`outside_fast` 是一套可复用的保守 CPU 裁剪证明，可用于其他“先证明整段不可见、再跳过整段工作”的消费者。
- `PHRVS001` 的证书/缓存/解码校验和 `false_cull` 见证机制可以复用；它们证明“缓存不会伪造源身份”，这部分与性能无关，仍然有价值。
- `outside_range` 已经支持闭区间和最多 6 种样式，适合做“整条尾段/整个未来区段”的批量证明，而不是每 note 一次。

如果还要继续，必须先满足这些条件：

1. 把证明从每 note 提升到每 line/每连续区段：用 `outside_range` 一次证明一整段 x/base 域和样式集合，避免每 note 的 dot3 与 guard。
2. 只在“本帧有大量未来音符且整段可跳过”的窗口启用；用实际 CPU profile 证明矩阵成本占 chart 阶段的比例足够大。
3. 把证书匹配、ctrl 默认、finite 检查移到 line 级；运行期每 note 只做一次常量比较或直接跳过。
4. 保持 witness 和 `false_cull=0`，保持原顺序、原控制时钟、原判定/音轨；未知/非 finite/近边界继续回退。
5. 需要全曲、多 GPU、真实 Main（不是研究 observer）的 ABBA；不能用当前有限窗口推广。
6. 如果批量版仍然没有收益，应停止这条线，不要为了“少算矩阵”继续堆实现。

### 11. 来源与可追溯性

原始记录（本地研究资料，未推送）：

- `本地证据/low-quality-normal/visibility-v1-summary.json`：v1 质量/性能汇总、组均值、`observed_treatment_relative_mean_percent=21.31012596976156`、`each_treatment_below_both_controls_in_window=false`。
- `本地证据/low-quality-normal/visibility-fast-retry-v1-summary.json`：fast 重试汇总、组均值、`observed_treatment_relative_mean_percent=1.2747622600734143`、`each_treatment_below_both_controls_in_window=false`。
- `本地证据/low-quality-normal/visibility-v1-quality-results.json`、`visibility-fast-v1-quality-results.json`：8 个质量作业和 6 组 RGBA。
- `本地证据/low-quality-normal/visibility-v1-performance-results.json`、`visibility-fast-retry-v1-performance-results.json`：1200 帧原始 CSV/汇总。
- `本地证据/low-quality-normal/visibility-v1-performance-runs/`、`visibility-fast-retry-v1-performance-runs/`：逐轮目录与 SurfaceFlinger summary。
- v1 measured runner SHA `27baa4cfc240ace480b1ab28c2a189a9989ea3f6b0da54cf0df83a454ce397fa`；fast retry runner SHA `9016fb23be2e0587c62a67cd895dba8c277af85babbaa34366f0da2997df8966`。
- v1 性能 plan/result SHA `12135948a6e0a9b87bc4e31a61719499f41a87ee504085ae33cf24dd1da73968` / `0bc8e92b1a62deff101e7695cbb4d80c388a65943060489738b21feea2371eb6`；fast 性能 plan/result SHA `9c3ee0793fdaadbf374a9a34b87bdc5c76d76ec8aef27fb92ff4a8830255c506` / `346bc29efa6e7c6a1435b301f6aebaae14cfdd2ebdd6023d1a81395bd695d7da`。
- 谱面 SHA `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c`；设备/上下文 Mali-T880 / OpenGL ES 3.2 v1.r20p0 / 2560×1440。

对应 `本地证据/AGENTS.md` 段落（原文）：

> 可见性v1实测已结束96365 terminal：off/on/on/off完整墙钟7.468366/9.092444/9.316750/7.706950ms，均7.587658→9.204597（慢21.31%），chart均3.353/5.238/5.411/3.491ms。虽剔除212855次都有原几何见证、六组画面/顶点/控制时钟相同，逐note向外区间矩阵证明比原矩阵更贵，不推广。p99 14.518/16.327/16.077/15.255，>16.67=4/10/9/5，>20=0/3/2/1；SF max18.173/33.513/16.821/16.914（有限10s采样），不称尾解决。PGR累计/线draw计数一致，但全部Painter采样upload off10964992 vs on10743808B差异记录，未强行当同变量。实际PHRVS001 blob39175B/keyaf63e9b7b9a9824e4cb216a2b0a443fe8a3b89fb4ae9339103c7120edb4df0fa、line2385ba91fb870a99168e43678e2c2cda2ff481431c7f89c968e009c91d3b907c独立，summary visibility-v1-summary.json；oldrunnerSHA27baa4cf、source9900/SOfb4284/41源保留。

原文中 `key`/`line` 写作缩写；上面为满足“不使用省略号”按正文完整值展开，其余文字保持原文。

> 快速可见界已本地提交8a71f2f39215a068572ab12d76aa291726946d8d（简化可见性误差界，未push），SOec6c5c26b2120a2de2d8f2e3bc7e6f1584506c8fec64c10266dddb81cb250149冻结41源核对；12个新CPU oracle+既有1测试、Windows研究/默认、Android API26通过。原nalgebra/FMA/FTZ/抵消/边界证据仅当前canonical CPU四角域，不是GPU/window保证；gamma16略高于2^-20注释已更正为低于2^-19，误差预算2^-16*A不变。25077/70098均terminal；真机8quality/6组RGBA、3464实际入队顶点/材质rank hash、5760滚动ctrl结束hash和最终源clock全同，每240帧212855提案全有原几何见证，false0，Runtime.fast_bounds确实true，CPUblob39175B/source/key/line缓存仍独立，源ZIP保留。

对应 `本地证据/low-quality-normal/README.md` 段落（原文）：

> 可见性预编译两版已完成本轮实际实验（2026-10-08），没有稳定性能收益，不推广：本地9900c684“预编译保守可见区间”与8a71f2f“简化可见性误差界”，41源/SOfb4284f2及ec6c5c26冻结核对，未push主项目。实际39175B PHRVS001 source静态geometry证书缓存冷/热/关闭保留/clear/corrupt及高画质/force回退均执行；默认所有build off，共用既有独立开关/manager/Settings清理。每版8quality/6组RGBA、3464实际NoteBuffer入队顶点/UV/颜色与材质rank序列、5760逐Line结束控制时钟、job末sourceclock检查全同；不是完整纹理identity/所有帧所有源状态。每240帧212855个提前拒绝都由原四角CPU几何见证，false_cull0，约887/frame；矩阵数下降不是收益证明。

> 初版每note区间dot/hull过贵，ABBA均7.587658→9.204597ms（慢21.31%）；快速版仅normal/zero canonical域，当前系数f64 centre/radius加原f32算术误差+FTZ绝对界，拒overflow/近边界/不支持域。12个新CPU tests+1旧（16k原nalgebra+8k FMA/FTZ和边界/CRC/mutation）通过，注释gamma16略高于2^-20已修正，2^-16幅度预算不变，不延伸GPU/window证明。

> 首次fastperformance因ADB daemon10060与2.936s两个poll gap失去SF ring overlap终止，原native1200行只作debug，失败数据保留、不放宽校验。独立sameSO retry完整ABBA均7.398781/7.703952/7.708401/7.819575ms，off7.609178/on7.706177（慢1.27%观察值），control漂移.420794ms，未建立稳定gain。p99 15.424/14.234/14.484/15.203，>16.67为8/6/7/6，>20为0/1/0/1；SF约9–10s覆盖max18.946/16.840/16.873/16.821，不称尾改善/全曲验证。字体/线变量最终重试全同，旧失败cohort不混。汇总visibility-v1-summary.json/visibility-fast-retry-v1-summary.json及quality/failure audit保存。本轮可见性结束默认off，后续优先原字体强制flush对延迟队列的影响；必须完整queued/native纹理read依赖才跳flush，未知回退，库改独立参考实现+完整 rev。完整普通gameplay编译/尖峰目标仍active。

<!-- N-13 在此之后追加 -->

---


<!-- N-13 在此之后追加 -->

---

## N-13 渲染取值计划（render value plan）

### 1. 一句话结论与判定

把“渲染会重复读取哪些动画轨道、这些轨道的纯函数证书是什么”编译成 `PHRRD001` 计划，运行期仍读取源 `Anim` 的真实 time/cursor/value，只在每个读取点前做形状/函数证书校验；结果两版都更慢：v1 组均值 `7.590549 → 8.346239 ms`（慢 `9.96%`），v2 按 Line 传递取值上下文后仍 `7.434239 → 7.987807 ms`（慢 `7.45%`）。两次负收益都大于对照漂移，判定为**确定的负优化**，默认关闭。

关键点：这不是“缓存动画结果”，而是“给原读取加证书 guard”。`sample_with` 校验通过后仍然调用 `track.now_opt()`，所以原来要做的右结合加法、Option/or、节点 time/cursor 读取一个都没省；新增的校验、TLS/RefCell 访问和每调用点传参才是净成本。

### 2. 问题与初衷

普通渲染在一帧内会多次读取同一批动画角色：`Note::render`/`init_ctrl_obj`/`now_transform` 对 `translationY` 一般读 3 次，`ctrl.size` 在 `note_uniform_scale` 下读 2 次，`Line::render` 的 `scaleY` 先 `now` 判 thin、再 `now_opt` 建 scale。源码静态计数看起来像“重复求值”，于是设想：在加载时把每条轨道的角色、链形状、每个非终端 keyframe 的纯函数身份编译成计划；运行期按计划走一条更窄的路径，避免重复的通用 `Anim` 遍历和 `TweenFunction` 动态分派。

约束是不能给公开可变 `Anim` 加隐式缓存，不能把 `Chart::update` 的结果直接给 render 用（render 会改 aspect、输入 seek 可能改 time、judge 曾临时 set note.time），也不能用 `res.time` 重采样。计划只能保存“源角色 + 纯函数证书”，值、time、cursor 必须每次从源读。这个约束决定了它天然是“认证”而不是“消除”：如果 `sample_with` 最后仍调用 `now_opt`，那么收益只能来自“少一次通用遍历/少一次动态分派”，而不是“少一次求值”。

第一版把执行选项放在全局/线程局部，每次 `sample` 都查 `OPTIONS` 和 `RUNTIME`。第二版把 `Execution` 按 Line 捕获一次，传给同一行内所有 `sample_with`，试图去掉每个 sample 的 TLS/RefCell。两版都保持原浮点、原证书 guard、原 Option/右结合语义；v2 只改变执行上下文的传递方式。

### 3. 实现思路与具体机制

1. `TrackPlan` 是一棵与源 `Anim` 链同形的节点数组。每个 `NodePlan` 保存 `keyframes: usize` 和 `functions: Box<[u8]>`；`functions` 只为“非终端 keyframe”记录 opcode，因为终端 keyframe 不会调用 tween。空节点/终端节点的函数字节为空。
2. `opcode` 不调用 `as_any`，而是把 `TweenRef` 与 66 个永久共享的 builtin `Arc` 做 `Arc::ptr_eq`：33 个 `StaticTween` + 33 个 `IntStaticTween`。匹配到就写 0..65，匹配不到写 `UNKNOWN=255`。自定义函数即使伪造 `as_any` 也不能冒充。
3. `sample_with` 从源轨道开始，逐节点校验：`keyframes.len()` 必须等于计划；`cursor` 必须小于 keyframes；如果 `cursor + 1 < keyframes`（即当前 keyframe 不是终端），计划里的函数 id 必须不是 UNKNOWN，且源 keyframe 的 `tween` 必须与 `PURE[id]` 指针相等；然后 `current = track.next.as_deref()`。链走完后 `current` 必须为 None。任何形状、cursor、函数、链长变化都返回 None，调用方回原路径。
4. 校验全部通过后，返回 `Some(track.now_opt())`。这里仍然执行源 `Anim` 的完整求值：每个节点的 `time`/`cursor` 读取、`next` 右结合加法、`Option::or`、NaN/-0 语义、值 mutation 都保持原样。计划没有保存结果，也没有用 `res.time` 替换。
5. 计划覆盖三个角色：`line.object.scale.1`（`scale_y`）、`line.ctrl_obj.size`（`ctrl_size`）、每个 note 的 `object.translation.1`（`translation_y`）。`compile` 对每条线/每个 note 调 `compile_track`；`compile_track` 先走一遍链统计节点和 keyframes、计费，再走一遍填 `functions`。总预算 16 MiB。
6. 序列化 `PHRRD001`：每个 track 写角色字节、节点数、每节点的 keyframes 数和函数字节；`decode_track` 会逐节点与当前源链比较 keyframes 数、函数证书，并确认源链没有变长/变短。头部包含 magic、ABI、源 SHA、body 长度、body SHA。
7. 运行期接入点：`Note::render` 先 `config.read_note.and_then(|plan| plan.translation_y.sample_with(&self.object.translation.1, &config.read_execution))` 得到 `sampled_y`，再把它传给 `now_transform_with_reads`；`now_transform_with_reads` 在 `y()` 闭包里用 `sampled_y`，在 `note_uniform_scale` 时用 `size_plan.sample_with` 得到 `sampled_size`；`Line::render` 用 `read_line.scale_y.sample_with` 得到 `sampled_scale_y`，再传给 `now_scale_with_y`。
8. 质量审计在相同矩阵调用点记录 `role + 矩阵 bits` 的 SHA 和计数；`actual_note_y_reuses`、`actual_ctrl_size_reuses`、`actual_line_scale_y_reuses` 只统计“显式消费者替换点”，不把首次求值算成“省掉的调用”。质量作业打开 audit，性能 cohort 关闭 audit/hash。
9. `render_lab/ir.rs` 是另一条更早的“语义 packet”路线：把常量普通音符编译成 `PHRIR001`，保存 line/note/flags/x/time/height/speed/color，给 CPU/GPU 消费者用。它不是本轮 9.96%/7.45% 的取值计划；本轮取值计划的源码是 `prpr/src/render_reads.rs`，通过 `render_compiler::reads` 挂载。两者容易混淆，本文在来源部分明确区分。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `prpr/src/render_reads.rs` | `TrackPlan`、`NodePlan`、`Execution`、`sample_with`、`compile_track`、`compile`、`decode_track`、`decode`、`prepare` | 取值计划、证书、缓存、运行期校验 |
| `prpr/src/core/note.rs` | `read_execution`、`sampled_y`、`size_plan`、`now_transform_with_reads` | note 侧消费 |
| `prpr/src/core/line.rs` | `read_execution`、`sampled_scale_y`、`now_scale_with_y` | line 侧消费 |
| `prpr/src/render_lab/ir.rs` | `Packet`、`Plan`、`compile`、`encode` | 相关但不同的语义 packet 路线 |
| `phira/src/render_research.rs` | `render_read_plan`、`render_read_audit`、`render_read_force_fallback` | 研究作业开关 |

下面是 `render_reads.rs` 的运行期核心。`sample_with` 是负收益的中心：它把每个读取点变成“逐节点证书校验 + 原 `now_opt`”。

```rust
// prpr/src/render_reads.rs:178-229
impl TrackPlan {
    pub fn sample(&self, track: &Anim<f32>) -> Option<Option<f32>> {
        self.sample_with(track, &execution())
    }
    /// One immutable execution choice per line/call site; guards still read
    /// live animation at each sample. Audit-off does no TLS counter/options work.
    #[inline]
    pub fn sample_with(&self, track: &Anim<f32>, opts: &Execution) -> Option<Option<f32>> {
        count_with(opts.audit, |s| s.samples += 1);
        if !opts.enabled {
            count_with(opts.audit, |s| s.disabled += 1);
            return None;
        }
        if opts.forced_fallback {
            count_with(opts.audit, |s| s.forced += 1);
            return None;
        }
        let mut current = Some(track);
        for node in &self.nodes {
            let Some(track) = current else {
                count_with(opts.audit, |s| s.shape += 1);
                return None;
            };
            if track.keyframes.len() != node.keyframes {
                count_with(opts.audit, |s| s.shape += 1);
                return None;
            }
            if node.keyframes != 0 {
                if track.cursor >= node.keyframes {
                    count_with(opts.audit, |s| s.cursor += 1);
                    return None;
                }
                if track.cursor + 1 < node.keyframes {
                    let id = node.functions[track.cursor];
                    if id == UNKNOWN || !Arc::ptr_eq(&track.keyframes[track.cursor].tween, &PURE[id as usize]) {
                        count_with(opts.audit, |s| s.function += 1);
                        return None;
                    }
                }
            }
            current = track.next.as_deref();
        }
        if current.is_some() {
            count_with(opts.audit, |s| s.shape += 1);
            return None;
        }
        count_with(opts.audit, |s| s.sampled += 1);
        // Original right-associated addition, Option/or, node-local time and
        // cursor, NaN/-0 and value mutations remain authoritative.
        Some(track.now_opt())
    }
}
```

```rust
// prpr/src/render_reads.rs:230-280
fn charge(budget: &mut usize, bytes: usize) -> Result<()> {
    *budget = budget.checked_add(bytes).context("Read plan size overflow")?;
    ensure!(*budget <= LIMIT, "Read plan exceeds 16MiB nominal metadata budget");
    Ok(())
}
fn compile_track(track: &Anim<f32>, budget: &mut usize) -> Result<TrackPlan> {
    let mut count = 0usize;
    let mut current = Some(track);
    while let Some(track) = current {
        count += 1;
        ensure!(count <= MAX_CHAIN && track.keyframes.len() <= u32::MAX as usize, "Read track chain/count limit");
        charge(budget, size_of::<NodePlan>())?;
        charge(budget, track.keyframes.len().saturating_sub(1))?;
        current = track.next.as_deref();
    }
    let mut nodes = Vec::with_capacity(count);
    let mut current = Some(track);
    while let Some(track) = current {
        nodes.push(NodePlan {
            keyframes: track.keyframes.len(),
            functions: track
                .keyframes
                .iter()
                .take(track.keyframes.len().saturating_sub(1))
                .map(|kf| opcode(&kf.tween))
                .collect(),
        });
        current = track.next.as_deref();
    }
    Ok(TrackPlan { nodes })
}
pub fn compile(source: [u8; 32], chart: &Chart) -> Result<Plan> {
    ensure!(chart.lines.len() <= u32::MAX as usize, "Read plan line count");
    let mut budget = size_of::<Plan>();
    charge(&mut budget, chart.lines.len().checked_mul(size_of::<LinePlan>()).context("Read line size overflow")?)?;
    let mut lines = Vec::with_capacity(chart.lines.len());
    for line in &chart.lines {
        ensure!(line.notes.len() <= u32::MAX as usize, "Read plan note count");
        charge(&mut budget, line.notes.len().checked_mul(size_of::<NotePlan>()).context("Read note size overflow")?)?;
        let scale_y = compile_track(&line.object.scale.1, &mut budget)?;
        let ctrl_size = compile_track(&line.ctrl_obj.borrow().size, &mut budget)?;
        let mut notes = Vec::with_capacity(line.notes.len());
        for note in &line.notes {
            notes.push(NotePlan {
                translation_y: compile_track(&note.object.translation.1, &mut budget)?,
            });
        }
        lines.push(LinePlan { scale_y, ctrl_size, notes });
    }
    Ok(Plan { source, lines })
}
```

note 侧消费（`prpr/src/core/note.rs:240-355` 的核心片段）：

```rust
    fn now_transform_with_reads(
        &self,
        res: &Resource,
        ctrl_obj: &CtrlObject,
        base: f32,
        incline_sin: f32,
        sampled_y: Option<Option<f32>>,
        #[cfg(feature = "chart_pre_compile")] size_plan: Option<&crate::render_compiler::reads::TrackPlan>,
        #[cfg(feature = "chart_pre_compile")] execution: crate::render_compiler::reads::Execution,
    ) -> Matrix {
        let y = || match sampled_y {
            Some(value) => {
                #[cfg(feature = "chart_pre_compile")]
                crate::render_compiler::reads::record_reuse_with("note_y", execution.audit);
                value.unwrap_or_default()
            }
            None => self.object.translation.1.now(),
        };
        let incline_val = 1. - incline_sin * (base * res.aspect_ratio + y()) * RPE_HEIGHT / 2. / 360.;
        let mut tr = match sampled_y {
            Some(_) => {
                let mut tr = super::Vector::new(self.object.translation.0.now(), y());
                tr.y /= res.aspect_ratio;
                tr
            }
            None => self.object.now_translation(res),
        };
        tr.x *= if matches!(self.kind, NoteKind::Hold { .. }) {
            1.
        } else {
            incline_val * ctrl_obj.pos.now_opt().unwrap_or(1.)
        };
        tr.y += base;
        let mut scale = self.object.scale.now_with_def(1.0, 1.0);
        let sampled_size: Option<Option<f32>> = {
            #[cfg(feature = "chart_pre_compile")]
            {
                if res.info.note_uniform_scale {
                    size_plan.and_then(|plan| plan.sample_with(&ctrl_obj.size, &execution))
                } else {
                    None
                }
            }
            #[cfg(not(feature = "chart_pre_compile"))]
            {
                None
            }
        };
        scale.x *= sampled_size.unwrap_or_else(|| ctrl_obj.size.now_opt()).unwrap_or(1.0);
        if res.info.note_uniform_scale {
            scale.y *= match sampled_size {
                Some(value) => {
                    #[cfg(feature = "chart_pre_compile")]
                    crate::render_compiler::reads::record_reuse_with("ctrl_size", execution.audit);
                    value.unwrap_or(1.)
                }
                None => ctrl_obj.size.now_opt().unwrap_or(1.),
            };
        } else {
            scale.y = 1.0;
        };
        let matrix = self.object.now_rotation().append_nonuniform_scaling(&scale).append_translation(&tr);
        #[cfg(feature = "chart_pre_compile")]
        crate::render_compiler::reads::audit_matrix("note", &matrix);
        matrix
    }
```

line 侧消费（`prpr/src/core/line.rs:369-401` 的核心片段）：

```rust
        let sampled_scale_y: Option<Option<f32>> = {
            #[cfg(feature = "chart_pre_compile")]
            {
                read_line.and_then(|plan| plan.scale_y.sample_with(&self.object.scale.1, &read_execution))
            }
            #[cfg(not(feature = "chart_pre_compile"))]
            {
                None
            }
        };
        let line_scaled = (sampled_scale_y.unwrap_or_else(|| self.object.scale.1.now_opt()).unwrap_or_default() - 1.).abs() > 1e-4;
        let line_matrix = self.now_transform(res, lines);
        #[cfg(feature = "chart_pre_compile")]
        crate::render_compiler::reads::audit_matrix("line", &line_matrix);
        res.with_model(line_matrix, |res| {
            let scale_matrix = self.object.now_scale_with_y(Vector::default(), sampled_scale_y, read_audit);
            #[cfg(feature = "chart_pre_compile")]
            crate::render_compiler::reads::audit_matrix("line_scale", &scale_matrix);
```

相关的 `render_lab/ir.rs` 是语义 packet 路线，不是本轮取值计划；它的 `Packet`/`Plan`/`compile` 如下，放在这里是为了区分两条路线：

```rust
// prpr/src/render_lab/ir.rs:19-99
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub line: u32,
    pub note: u32,
    pub flags: u32,
    pub x: f32,
    pub time: f64,
    pub height: f64,
    pub speed: f64,
    pub color: [f32; 4],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub source: [u8; 32],
    pub line_notes: Vec<u32>,
    pub line_order: Vec<u32>,
    pub packets: Vec<Packet>,
}
fn constant(anim: &AnimFloat) -> bool {
    anim.keyframes.len() <= 1 && anim.next.is_none()
}
pub fn compile(chart: &Chart, source: [u8; 32]) -> (Plan, BTreeMap<String, usize>) {
    let mut packets = Vec::new();
    let mut fallback = BTreeMap::new();
    for (line_id, line) in chart.lines.iter().enumerate() {
        let ctrl = line.ctrl_obj.borrow();
        let basic_line = matches!(line.kind, JudgeLineKind::Normal)
            && line.parent.is_none()
            && line.attach_ui.is_none()
            && line.incline.is_default()
            && ctrl.alpha.is_default()
            && ctrl.size.is_default()
            && ctrl.pos.is_default()
            && ctrl.y.is_default()
            && line.object.alpha.next.is_none()
            && line.object.alpha.keyframes.iter().all(|k| k.value >= 0.);
        for (note_id, note) in line.notes.iter().enumerate() {
            let reason = if matches!(note.kind, NoteKind::Hold { .. }) {
                Some("hold")
            } else if !basic_line {
                Some("complex-line")
            } else if note.fake
                || !constant(&note.object.translation.0)
                || !note.object.translation.1.is_default()
                || !note.object.rotation.is_default()
                || !note.object.scale.0.is_default()
                || !note.object.scale.1.is_default()
                || !note.object.alpha.is_default()
            {
                Some("complex-note")
            } else if !note.time.is_finite() || !note.height.is_finite() || !note.speed.is_finite() || note.speed <= 0. {
                Some("invalid-fast-path-domain")
            } else {
                None
            };
            if let Some(reason) = reason {
                *fallback.entry(reason.to_owned()).or_default() += 1;
                continue;
            }
            let x = note.object.translation.0.now_opt().unwrap_or(0.);
            if !x.is_finite() || ![note.color.r, note.color.g, note.color.b, note.color.a].iter().all(|v| v.is_finite()) {
                *fallback.entry("non-finite-color-position".to_owned()).or_default() += 1;
                continue;
            }
            let flags = note.kind.order() as u32 | ((note.above as u32) << 8) | ((note.multiple_hint as u32) << 9);
            packets.push(Packet {
                line: line_id as _,
                note: note_id as _,
                flags,
                x,
                time: note.time,
                height: note.height,
                speed: note.speed,
                color: [note.color.r, note.color.g, note.color.b, note.color.a],
            });
        }
    }
    (
        Plan {
            source,
            line_notes: chart.lines.iter().map(|l| l.notes.len() as u32).collect(),
            line_order: chart.order.iter().map(|v| *v as u32).collect(),
            packets,
        },
        fallback,
    )
}
```

### 5. 开关、默认值、构建特性

| 开关/特性 | 默认 | 作用 |
|---|---|---|
| `render_research` feature | 生产关闭 | 研究后端才编译 `render_read_plan` 请求 |
| `chart_pre_compile` feature | 生产可能启用，但消费者默认关闭 | 共享 `RenderCache`、Config `chart_pre_compile` 门 |
| `render_read_plan` | false | 打开取值计划消费者 |
| `render_read_audit` | false | 打开矩阵哈希、复用计数、回退计数 |
| `render_read_force_fallback` | false | 强制所有 sample 回原路径 |
| `Config::chart_pre_compile` | 默认 false | 关闭时不读不写 cache |
| 生产 Config/UI | 无 | 本轮没有把取值计划接进普通设置 |

研究请求默认 `render_read_plan=false`；质量 cohort 打开 audit，性能 cohort 关闭 audit。计划与 `chart_pre_compile` 共用同一个 `RenderCache`、16 MiB 名义预算和 Settings 清理；关闭 `chart_pre_compile` 时 `prepare` 返回 `enabled:false`，不读不写。

### 6. 完整测量数据（表格）

环境同 N-12：Mali-T880 / 2560×1440 / 低画质 / `aggressive=false` / 120 预热 + 1200 采样 / ABBA `off/on/on/off`。v1 是全局执行选项；v2 是每 Line 捕获一次 `Execution`，取消热 sample 的 `OPTIONS`/`RUNTIME` TLS 查询。

v1 逐轮数据：

| 次序 | 路径 | 应用均值 ms | p50 ms | p99 ms | max ms | >16.67ms | >20ms | chart 均值 ms | update 均值 ms | render 均值 ms | flush 均值 ms | SF FPS | SF p99 ms | SF max ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 7.580033 | 16.7229 | 15.572662 | 19.182291 | 7 | 0 | 3.410886 | 0.397548 | 6.485256 | 0.697230 | 59.7502 | 16.7804 | 16.8016 |
| 1 | on | 8.316844 | 16.7016 | 16.762126 | 19.685415 | 13 | 0 | 4.268558 | 0.391411 | 7.271630 | 0.653803 | 59.7538 | 16.8218 | 18.7167 |
| 2 | on | 8.375634 | 16.7203 | 15.498015 | 20.495312 | 8 | 1 | 4.325333 | 0.385598 | 7.329786 | 0.660250 | 59.7499 | 16.7626 | 16.8406 |
| 3 | off | 7.601064 | 16.7292 | 15.134308 | 20.052084 | 5 | 1 | 3.423738 | 0.404884 | 6.530651 | 0.665529 | 59.7496 | 16.7740 | 16.8000 |
| 组均值 | off / on | 7.590549 / 8.346239 |  |  |  |  |  | 3.417312 / 4.296946 | 0.401216 / 0.388505 | 6.507954 / 7.300708 | 0.681380 / 0.657027 | 59.7499 / 59.7519 |  |  |

v1 组差 `+0.755690 ms`，观察值 `+9.9557%`；off 前后漂移 `+0.021031 ms`，远小于组差，负收益确定。

v2 逐轮数据：

| 次序 | 路径 | 应用均值 ms | p50 ms | p99 ms | max ms | >16.67ms | >20ms | chart 均值 ms | update 均值 ms | render 均值 ms | flush 均值 ms | SF FPS | SF p99 ms | SF max ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | off | 7.389007 | 16.7234 | 13.432047 | 33.256250 | 5 | 1 | 3.246974 | 0.409347 | 6.301376 | 0.678284 | 59.7496 | 16.7954 | 17.1604 |
| 1 | on | 7.950891 | 16.7234 | 15.070202 | 17.691666 | 5 | 0 | 3.921670 | 0.382340 | 6.920046 | 0.648505 | 59.7488 | 16.7717 | 16.8146 |
| 2 | on | 8.024723 | 16.7307 | 15.590651 | 19.002082 | 8 | 0 | 3.953739 | 0.391362 | 6.968332 | 0.665029 | 59.7506 | 16.7729 | 16.8089 |
| 3 | off | 7.479470 | 16.7307 | 14.617477 | 18.952604 | 5 | 0 | 3.407865 | 0.390503 | 6.436463 | 0.652505 | 59.7487 | 16.7700 | 16.7911 |
| 组均值 | off / on | 7.434239 / 7.987807 |  |  |  |  |  | 3.327420 / 3.937705 | 0.399925 / 0.386851 | 6.368920 / 6.944189 | 0.665395 / 0.656767 | 59.7492 / 59.7497 |  |  |

v2 组差 `+0.553568 ms`，观察值 `+7.4462%`；off 前后漂移 `+0.090463 ms`，仍远小于组差，负收益确定。v2 的 chart 增量从 v1 的 `+0.880 ms` 降到 `+0.610 ms`，说明取消热 TLS 有一点效果，但没有改变方向。

质量 cohort 的 8 个作业（v1 与 v2 的证书/矩阵哈希相同）：

| 变体 | 计划状态 | 字节 | prepare ms | 行 | 音符 | note_y 复用 | line_scale_y 复用 | ctrl_size 复用 | 矩阵审计 | matrix SHA |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| cold-on | 冷编译并发布 | 27588 | 101.696 | 24 | 1568 | 535104 | 5760 | 0 | 279086 | `d0cbc15e95dc8897852fbec9712ade3c3a3cb263368d0aa3135b9d50fd389d53` |
| hot-on | 热命中 | 27588 | 62.341 | 24 | 1568 | 535104 | 5760 | 0 | 279086 | 同上 |
| off | 未安装 | 0 |  |  |  | 0 | 0 | 0 | 279086 | 同上 |
| forced-fallback | 计划安装但强制回退 | 27588 | 47.858 | 24 | 1568 | 0 | 0 | 0 | 279086 | 同上 |
| high-on | 高画质计划安装 | 27588 | 76.804 | 24 | 1568 | 535104 | 5760 | 0 | 279086 | 同上 |
| high-off | 高画质未安装 | 0 |  |  |  | 0 | 0 | 0 | 279086 | 同上 |
| clear-on | 清理后重建 | 27588 | 90.073 | 24 | 1568 | 535104 | 5760 | 0 | 279086 | 同上 |
| corrupt-on | 损坏后重建 | 27588 | 50.736 | 24 | 1568 | 535104 | 5760 | 0 | 279086 | 同上 |

`ctrl_size` 为 0 不是失败：本谱面 `note_uniform_scale` 关闭，消费者没有走 `ctrl.size` 的替换点。`note_y=535104`、`line_scale_y=5760` 是“实际替代读取次数”，不是“省掉的调用次数”；审计说明明确写了“Successful samples are first evaluations, not avoided calls”。

缓存：`PHRRD001`，27,588 字节，key `cc7aa8bedb832ce3471e14ebbe99731118dc6ea614c878a70eeba3c98efcf365`，blob SHA `6036a90684b76ccffe58d41ae4efbddaf9efdf54ddd59d2577405a82f3587790`；24 条线、1568 个音符。质量作业还核对了 279,086 个有序矩阵调用的 bits hash，包含 update/judge 的公共 `now_transform`；这不等于完整 CPU 顶点流。

### 7. 质量与回归结果

通过的部分：

- 8 个真机质量作业形成 6 组有效 RGBA 对照，全部逐像素相同；高画质、关闭、强制回退、清理、损坏都实际执行。
- 279,086 个有序矩阵调用的 count/hash 逐位相同；这说明“校验通过后仍走原 `now_opt`”的路径在有限窗口内没有改变矩阵结果。
- `actual_note_y_reuses=535104`、`actual_line_scale_y_reuses=5760`、`actual_ctrl_size_reuses=0` 是实际消费者替换计数，不是估算。
- 缓存冷发布/热命中/关闭不读写/清理重建/损坏重建全部执行；CPU blob 损坏不会破坏独立的 `PHRLN001` 线缓存；源 ZIP 保留。
- `PHRRD001` 的 decode 会逐节点比较 keyframes 数、函数证书和源链长度，伪造/截断/源变化都会回退。

没有证明的部分：

- 矩阵 hash 覆盖的是有序调用，不是完整顶点流；没有证明所有帧、所有纹理、所有 GPU 的输出。
- 质量窗口只覆盖一个谱面、一个设备；不是全曲/多设备。
- 计划没有消除动画求值，只是加了证书；质量通过不能推出性能收益。
- 性能 cohort 关闭 audit/hash，质量数据不能当性能。
- 高画质作业只证明计划可安装/可回退，不证明高画质收益。

### 8. 为什么判定为无/负（逐项对比）

第一，`sample_with` 的末尾是 `Some(track.now_opt())`。这意味着每个“复用”仍然执行源动画的完整求值：右结合加法、`Option::or`、每个节点的 time/cursor、可能的 NaN/-0 和值 mutation。计划只省掉“通用遍历中的一部分形状检查/动态分派”，没有省掉求值本身。所谓 535,104 次 `note_y` 复用，不是 535,104 次求值被消除，而是 535,104 次读取被换成了“校验 + 原求值”。

第二，校验本身不便宜。每个 sample 要逐节点比较 `keyframes.len()`、`cursor`、`functions[cursor]`、`Arc::ptr_eq`，还要走完 `next` 链；每个 note 至少一次，`translation_y` 还有多次调用点。v2 把 `Execution` 按 Line 捕获，去掉了热 sample 的 `OPTIONS`/`RUNTIME` TLS 查询，所以 chart 增量从 `+0.880 ms` 降到 `+0.610 ms`；但每个 sample 的证书校验仍在，净结果仍慢 7.45%。

第三，v1 的 chart 均值 `3.410886/3.423738 → 4.268558/4.325333`，每轮多约 `0.86/0.90 ms`；render 均值 `6.485256/6.530651 → 7.271630/7.329786`，每轮多约 `0.79/0.80 ms`。v2 的 chart 均值 `3.246974/3.407865 → 3.921670/3.953739`，每轮多约 `0.67/0.55 ms`；render 多约 `0.62/0.53 ms`。两版的增量都落在 chart/render 内，和“每读取点加 guard”的预期一致。

第四，负收益大于漂移。v1 off 前后差 `0.021 ms`，组差 `0.756 ms`；v2 off 前后差 `0.090 ms`，组差 `0.554 ms`。这不是“控制漂移大于处理差”的情形，不能把 9.96%/7.45% 归因于噪声。

第五，计划没有改变真正的帧成本。它只覆盖 `note.translationY`、`line.scaleY`、`ctrl.size` 三个角色；HUD、glyph 上传、粒子、背景、驱动等待都不在计划内。即使这三个角色的读取全部免费，整帧上界收益也有限；而当前实现还为它们加了每 sample 的校验，净结果为负。

第六，这个方向与 N-12 的教训相同：在 17 ms 帧里，给每个 note/每个读取点加 guard，通常跑不赢它省下的局部工作。正确做法应该是“在加载期把重复表达式合并成更少的读取点”，而不是“在运行期给每个读取点加证书”。当前计划保存的是“源角色/纯函数身份”，不是“已合并的表达式 DAG”，所以它无法真正消除重复求值。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：v1 本地提交 `0bb7d51c56a19ee557443c5eb5f9b7868e035caa`；v2 本地提交 `08eb70f4f79eea948becb92c7a8b5e3bc6a9dacb`；两版各 39 个冻结源文件 |
| 默认 | 关闭；所有 build 默认关闭；研究请求 `render_read_plan` 可复现 |
| 普通路径 | 未接入；没有生产 Config/UI 开关 |
| 主线是否已实施 | 否（main） |
| 研究后端 | v1 SO `d296113da47bb4bd77044acafa32493195616b9ee996222eb5048daa786081d2`；v2 SO `40fa51aaa529cc4626f505d8a9a30f8a0a3727fb5d7c408c26238e3f8859cfde`；均未推送主项目 |
| 质量/契约 | 8 个质量作业、6 组 RGBA、279,086 矩阵 hash、缓存生命周期和回退完成；性能 cohort 完整结束 |
| 后续 | 设备/构建作业已结束，没有遗留的取值计划设备任务；不推进 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `TrackPlan` 的“链形状 + 纯函数证书”可以作为安全检查复用，尤其是需要确认“这个 `Anim` 没有换函数/换链”的场景。
- `PHRRD001` 的序列化/解码校验、16 MiB 预算、共享 cache/clear/disable 机制可以复用。
- 矩阵审计的 `role + bits` 哈希方法可以复用到其他渲染计划；它证明“消费者没有改变矩阵”，但不证明性能。

如果还要继续，必须先满足这些条件：

1. 目标从“加 guard”改成“减少读取点”：在加载期把同一帧内重复的 `translationY`/`scaleY`/`ctrl.size` 读取合并成一次，并把结果传给所有消费者；这需要真正的表达式/DAG 计划，而不是每读取点校验。
2. 把证书校验移到 line 级或加载期；运行期只保留一次常量比较/直接索引，不能每 sample 走链。
3. 必须证明合并后的求值次数真的下降；`actual_*_reuses` 不能当“省掉的调用”，需要独立的求值计数或 profile。
4. 保持原 Option/右结合/time/cursor/NaN/-0 语义；未知函数、活 shape、active Arc、next 变化继续回退。
5. 需要全曲、多 GPU、真实 Main 的 ABBA；当前 1200 帧有限窗口只能说明这个实现的方向不对。
6. 如果无法把读取点降下来，应停止这条线；不要继续用“更多复用计数”证明性能。

### 11. 来源与可追溯性

原始记录：

- `本地证据/low-quality-normal/render-read-v1-summary.json`：v1 质量/性能汇总、`observed_treatment_relative_mean_percent=9.955674459484086`、组均值 `7.590548817916666/8.346239147916666`。
- `本地证据/low-quality-normal/render-read-context-v1-summary.json`：v2 汇总、`observed_treatment_relative_mean_percent=7.446198460550768`、组均值 `7.434238715/7.987806883749999`、`each_treatment_below_both_controls_in_window=false`。
- `本地证据/low-quality-normal/render-read-v1-performance-results.json`、`render-read-context-v1-performance-results.json`：1200 帧原始 CSV/汇总。
- `本地证据/low-quality-normal/render-read-v1-quality-results.json`、`render-read-context-v1-quality-results.json`：8 个质量作业、6 组 RGBA。
- 缓存 key `cc7aa8bedb832ce3471e14ebbe99731118dc6ea614c878a70eeba3c98efcf365`；blob SHA `6036a90684b76ccffe58d41ae4efbddaf9efdf54ddd59d2577405a82f3587790`；27,588 字节。
- v1 source `0bb7d51c56a19ee557443c5eb5f9b7868e035caa` / SO `d296113da47bb4bd77044acafa32493195616b9ee996222eb5048daa786081d2`；v2 source `08eb70f4f79eea948becb92c7a8b5e3bc6a9dacb` / SO `40fa51aaa529cc4626f505d8a9a30f8a0a3727fb5d7c408c26238e3f8859cfde`。
- 质量/性能 plan/result SHA：v1 `022eb4467ad51067790174762d5ac5a3edb2db8385befa0c3e5bd024f79d65f7` / `d4129b8bbd916151902eae585ff84beb4358e69888802f958ba4f19d89ee505c`；v2 `79023a1a05584d8a06da9f6b4e8ed12396d2b138c10233030a73e911ee3b5f84` / `fcb976d313c1078be2f89d657368e3aab277e84fe259ab29f5c7cb20f106e1b0`。
- 谱面 SHA `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c`。

对应 `本地证据/AGENTS.md` 段落（原文）：

> 取值计划v1已本地提交0bb7d51c56a19ee557443c5eb5f9b7868e035caa（编译渲染取值计划，主项目未push），Android SOd296113da47bb4bd77044acafa32493195616b9ee996222eb5048daa786081d2冻结39源核对。Windows研究/默认检查及4CPU oracle实际通过；8个真机quality(cache cold/hot/off/force/high/clear/corrupt)6组RGBA全同，279086有序矩阵调用hash/count逐位全同（包含update/judge公共now_transform，不冒称完整CPU顶点流）；实际noteY复用535104次、lineScaleY5760次，ctrlSize因本谱面uniform_scale关闭为0。实际PHRRD001 CPU blob cold发布/hot命中/关闭不读写保留/清理重建/损坏重建，CPU损坏不破坏独立PHRLN001线缓存；源ZIP保留，全部同manager/settings路径。42540已terminal。

> 当前已直接实现普通渲染取值计划（尚未提交）：新增prpr/src/render_reads.rs，加载编译/落盘PHRRD001 source-role及纯函数证书，仍读取源Anim真实time/cursor/value；接入Chart拥有状态、Line.scaleY、Note.translationY和uniform ctrl.size，原判定/音轨不用缓存。未知函数不调用as_any，只认证66个永久共享builtin Arc；empty/terminal节点不调用Tween，活shape/activeArc/next变化回原路径。保留Option、原默认值/算式/右结合，没有帧间值cache、没有res.time重采样。消费者共用已有chart_pre_compile开关、同Arc RenderCache和Settings清理/禁用ticket；高画质CPU消费者独立于GPU线能力。研究默认render_read_plan=false，质量可audit原已计算矩阵bits和实际替代read次数，不额外求值；性能关闭计数/hash。

对应 `本地证据/low-quality-normal/README.md` 段落（原文）：

> 2026-10-08普通动画取值计划两版已实测并停止推广：本地0bb7d51（编译渲染取值计划）与08eb70f（按线传递取值上下文），两SO分别d296113d/40fa51aa、39源冻结；未推主项目。PHRRD001实际27,588B source-role/66个永久builtin Arc证书计划，当前值/time/cursor仍原Anim读取，不修改公开trait、不调用可伪装as_any。共用原chart_pre_compile与RenderCache/Settings清理，cold/hot/off/force/high/clear/corrupt消费者执行。两版各8quality/6RGBA与279086有序矩阵调用hash全同，noteY实际复用535104、lineScaleY5760、ctrlSize0（当前谱面uniform_scale关闭）；矩阵审计包含update/judge调用，不等于完整顶点流。

> V1 ABBA off/on/on/off均值7.5800/8.3168/8.3756/7.6011ms，组7.5905→8.3462（慢9.96%）；V2按Line Execution取消热sample/reuse OPTIONS/RUNTIME TLS，仍7.3890/7.9509/8.0247/7.4795ms，组7.4342→7.9878（慢7.45%）。V2 p99 13.432/15.070/15.591/14.617，>16.67为5/5/8/5、>20为1/0/0/0；SF max17.160/16.815/16.809/16.791ms（有限10s采样），不称尾解决。源码足够纯和调用变少不是性能成功，所有build消费者默认disabled，研究请求仍可复现。汇总render-read-v1-summary.json/render-read-context-v1-summary.json，各原始runner SHA保存，cohort独立。

---

## N-14 图集区域合并（atlas coalesce）

### 1. 一句话结论与判定

把一次字形发布中的多个小 dirty 矩形贪心合并成更少的大矩形，确实把 PGR 的传输矩形从 `253` 降到 `206`、采样上传字节从 `10,964,992` 降到 `6,259,940`；但完整 ABBA 组均值是 `7.422197 → 7.442371 ms`，观察值慢 `0.27%`，而对照自身从 `7.557288` 漂到 `7.287107 ms`（`-0.270181 ms`），组差只有 `0.020174 ms`。两轮处理都落在两轮对照之间，因此判定为**无稳定收益（观察值慢 0.27%，按分类归入负优化/无收益）**。默认 policy 0，不推广。

这不是“合并一定坏”，而是“当前实现的合并成本、额外像素和 poll 增长抵消了调用数下降”。在 17 ms 帧里，减少 47 次纹理上传调用没有转化为可测的整帧收益；同时 PGR 的同步 poll 从 `172` 增到 `444`，版本/等待行为变差。

### 2. 问题与初衷

字形图集的发布不是一个整图上传，而是一批来自 `glyph_brush` 的 dirty 矩形。每个矩形都可能触发一次 `glTexSubImage2D`，而小矩形很多时，GL 调用次数、驱动状态切换和纹理读写依赖会成为 CPU/driver 墙钟的一部分。N-14 的初衷是：在不改变字形栅格、图集排布、UV、透明顺序和像素值的前提下，把多个相邻/重叠的 dirty 矩形合并成更少的包围盒，从而减少上传调用数。

约束是“多上传的像素必须是权威 bitmap 中已经正确的像素”。合并后的包围盒会覆盖原 dirty 区域之外的像素；这些像素必须从 CPU 镜像/权威位图复制，不能是未初始化或旧的。这样合并只改变传输几何，不改变最终图集内容。每合并一次允许的额外 RGBA 字节有硬上限，避免为了少一次调用而上传巨大空白区域。

### 3. 实现思路与具体机制

1. 输入是最多 256 个 `Rect`；每个 `Rect` 有 `x/y/w/h`。如果输入超过 256，计划原样返回并标记 `budget_exhausted`，不做部分合并。
2. 每次迭代在所有矩形对中找“合并后新增面积最小”的一对。`bounding(a,b)` 计算并集；`extra = (area(union) - area(a) - area(b)) * 4`。重叠区域的节省算 0 extra；只有 `extra <= extra_bytes_per_merge` 才允许合并。
3. 选择全局最便宜的一对，而不是按输入顺序或相邻关系；这样合并顺序确定、可复现。合并后用并集替换 `i`，删除 `j`，保留其余矩形的相对顺序，平局时顺序稳定。
4. 工作上限：最多 65,536 次 pair 比较、64 次合并、256 个输入矩形。每次循环先算 `pairs = n*(n-1)/2`；如果 `pairs > budget - checks`，停止并标记 budget exhausted，绝不用“不完整的扫描”选一个合并。
5. 字节统计用 u128 计算 `area * 4`，避免 `u32*u32*4` 溢出；超过 u64 时饱和并标记 `bytes_saturated`。`bounding` 的右/下边界用 u64 计算，无法用 `u32` 表示时返回 None，该对不合并。
6. 合并计划只在 `policy == 3` 时启用。`atlas_pool` 从当前 slot 的 damage/version 差集拿到 `rectangles`，转成 `damage::Rect`，调用 `coalesce::coalesce(exact, 8192, 65_536)`，再把结果转回 `delta::Rect`。统计写入 `coalesce_checks/merges/budget_exhausted/input_rects/output_rects/input_bytes/output_bytes`。
7. 实际传输仍然逐个矩形从 CPU shadow 复制到 scratch，再 `update_texture_part`。合并只减少“矩形数量”，不减少“总像素字节”；如果包围盒多覆盖了空白，字节反而增加。每个矩形仍然是原图集坐标、原格式、原过滤。
8. 质量 oracle 的做法：构造任意 stale 图像（只在原 dirty 区域写入脏值），对原矩形计划和合并计划分别应用权威 source，比较最终整图；同时检查每个原 dirty 像素都被合并计划覆盖。这样即使合并多覆盖了像素，只要那些像素来自权威 source，最终图就一致。
9. 预算的语义是“每次合并最多 8 KiB 额外权威像素”，不是 CPU 墙钟上界；65,536 是 pair 比较上限，不是总 CPU 操作上限。这个区别在文档和审计里都明确写了。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `prpr/src/ui/atlas_coalesce.rs` | `MAX_INPUT_RECTS`、`MAX_MERGES`、`MAX_PAIR_EVALUATIONS`、`Stats`、`area`、`byte_count`、`bounding`、`coalesce` | 有界贪心合并 |
| `prpr/src/ui/atlas_pool.rs` | `policy == 3` 分支、`coalesce::coalesce`、统计字段 | 发布路径接入 |
| `prpr/src/ui/text.rs` | `research_pool_policy(policy)` | 研究策略选择 |
| `prpr/src/ui/atlas_damage.rs` | `Rect`、damage 差集 | 输入矩形来源 |

下面是 `atlas_coalesce.rs` 的完整核心实现（第 1 至 116 行）。测试模块在文件后半部分，未粘贴；核心算法和预算逻辑全部在这里。

```rust
// prpr/src/ui/atlas_coalesce.rs:1-116
//! Bounded upload planning for any atlas damage, independent of characters.
//!
//! A merge copies a bounding box from the authoritative bitmap into an image
//! whose read fence has completed. Extra or overlapping pixels are copied
//! exactly; this changes transfer geometry, not rasterization or pixel values.
pub use super::damage::Rect;

pub const MAX_INPUT_RECTS: usize = 256;
pub const MAX_MERGES: usize = 64;
pub const MAX_PAIR_EVALUATIONS: usize = 65_536;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub input_calls: usize,
    pub output_calls: usize,
    pub planned_input_bytes: u64,
    pub planned_output_bytes: u64,
    pub merged: usize,
    pub checks: usize,
    /// An input-size, pair-evaluation or merge-count work limit stopped planning.
    pub budget_exhausted: bool,
    /// Extremely large synthetic plans exceed the u64 reporting range. Byte
    /// counters saturate rather than wrapping; merge comparisons remain exact.
    pub bytes_saturated: bool,
}

fn area(rect: Rect) -> u64 {
    u64::from(rect.w) * u64::from(rect.h)
}

fn byte_count(rectangles: &[Rect]) -> (u64, bool) {
    let bytes = rectangles
        .iter()
        .fold(0u128, |bytes, &rect| bytes.saturating_add(u128::from(area(rect)) * 4));
    (bytes.min(u128::from(u64::MAX)) as u64, bytes > u128::from(u64::MAX))
}

fn bounding(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    // Ends can exceed u32 when a synthetic input's x + w overflows. Compute
    // them without wrapping and reject a union not representable by Rect.
    let right = (u64::from(a.x) + u64::from(a.w)).max(u64::from(b.x) + u64::from(b.w));
    let bottom = (u64::from(a.y) + u64::from(a.h)).max(u64::from(b.y) + u64::from(b.h));
    Some(Rect {
        x,
        y,
        w: u32::try_from(right - u64::from(x)).ok()?,
        h: u32::try_from(bottom - u64::from(y)).ok()?,
    })
}

/// Greedily merge the globally cheapest eligible pair at each iteration.
/// `extra_bytes_per_merge` limits extra RGBA bytes per removed call. Overlap
/// savings count as zero extra bytes. This budget is not a hardware-cost claim.
///
/// Work is bounded even if the caller supplies a larger candidate budget:
/// at most 65,536 pair evaluations, 64 merges and 256 input rectangles. Plans
/// above the input bound pass through unchanged. An incomplete pair scan is
/// never used to choose a merge; it would cease to be a global greedy choice.
pub fn coalesce(mut rectangles: Vec<Rect>, extra_bytes_per_merge: u64, candidate_budget: usize) -> (Vec<Rect>, Stats) {
    let (input_bytes, saturated) = byte_count(&rectangles);
    let mut stats = Stats {
        input_calls: rectangles.len(),
        planned_input_bytes: input_bytes,
        bytes_saturated: saturated,
        ..Stats::default()
    };
    let budget = candidate_budget.min(MAX_PAIR_EVALUATIONS);
    if rectangles.len() > MAX_INPUT_RECTS {
        stats.budget_exhausted = true;
    } else {
        while rectangles.len() > 1 {
            if stats.merged == MAX_MERGES {
                stats.budget_exhausted = true;
                break;
            }
            let count = rectangles.len();
            let pairs = count * (count - 1) / 2;
            if pairs > budget - stats.checks {
                stats.budget_exhausted = true;
                break;
            }

            let mut best: Option<(u128, usize, usize, Rect)> = None;
            for i in 0..count - 1 {
                for j in i + 1..count {
                    stats.checks += 1;
                    let Some(joined) = bounding(rectangles[i], rectangles[j]) else {
                        continue;
                    };
                    // A u32 width*height fits u64. Adding two such areas and
                    // multiplying RGBA bytes uses u128 to avoid extreme-case
                    // overflow or accidentally classifying a costly merge cheap.
                    let old_area = u128::from(area(rectangles[i])) + u128::from(area(rectangles[j]));
                    let extra = u128::from(area(joined)).saturating_sub(old_area) * 4;
                    if extra <= u128::from(extra_bytes_per_merge) && best.as_ref().is_none_or(|candidate| extra < candidate.0) {
                        best = Some((extra, i, j, joined));
                    }
                }
            }
            let Some((_, i, j, joined)) = best else {
                break;
            };
            rectangles[i] = joined;
            // Keep the other rectangles' order, giving deterministic ties.
            rectangles.remove(j);
            stats.merged += 1;
        }
    }
    stats.output_calls = rectangles.len();
    let (output_bytes, saturated) = byte_count(&rectangles);
    stats.planned_output_bytes = output_bytes;
    stats.bytes_saturated |= saturated;
    (rectangles, stats)
}
```

发布路径接入（`prpr/src/ui/atlas_pool.rs:654-680`）：

```rust
                    let rectangles = if self.policy == 3 {
                        let exact = rectangles
                            .into_iter()
                            .map(|r| damage::Rect {
                                x: r.x,
                                y: r.y,
                                w: r.w,
                                h: r.h,
                            })
                            .collect();
                        let (planned, stats) = coalesce::coalesce(exact, 8192, 65_536);
                        self.coalesce_checks += stats.checks as u64;
                        self.coalesce_merges += stats.merged as u64;
                        self.coalesce_budget_exhausted += stats.budget_exhausted as u64;
                        self.coalesce_input_rects += stats.input_calls as u64;
                        self.coalesce_output_rects += stats.output_calls as u64;
                        self.coalesce_input_bytes += stats.planned_input_bytes;
                        self.coalesce_output_bytes += stats.planned_output_bytes;
                        planned
                            .into_iter()
                            .map(|r| delta::Rect {
                                x: r.x,
                                y: r.y,
                                w: r.w,
                                h: r.h,
                            })
                            .collect::<Vec<_>>()
                    } else {
                        rectangles
                    };
```

### 5. 开关、默认值、构建特性

| 开关/特性 | 默认 | 作用 |
|---|---|---|
| `render_research` / `render_atlas_pool` feature | 生产关闭 | 研究后端才编译 atlas pool 实验 |
| atlas `policy` | 0 | 0 = 原路径；2 = 精确 damage；3 = coalesce |
| `research_pool_policy(policy)` | 调用方传入，研究默认 0 | 选择策略，`assert!(policy <= 3)` |
| `extra_bytes_per_merge` | 8192 | 每次合并最多额外 8 KiB 权威像素 |
| `candidate_budget` | 65,536 | pair 比较上限 |
| 生产 Config/UI | 无 | 没有把 policy3 接进普通设置 |

`policy3` 只是研究候选；`policy2` 的近期真实 ABBA 也是负收益（约慢 1.85%），已归档，不默认。生产仍使用 policy0。关闭/缺能力/全 busy 时走原路径；`missing`/`busy` 质量作业不执行 coalesce，计数为 0。

### 6. 完整测量数据（表格）

环境：Mali-T880 / 2560×1440 / 低画质 / `aggressive=false` / 120 预热 + 1200 采样 / 顺序 `policy0/policy3/policy3/policy0`；同一 SO `ed7b6528043a21fbf830678e928a411e707337877124780feec56948e5580c19`；关闭 trace。

性能逐轮：

| 次序 | 策略 | 应用均值 ms | p50 ms | p99 ms | max ms | >16.67ms | >20ms | 采样上传 bytes | 发布数 | HUD 均值 ms | SF max ms | PGR polls | checks | merges | 输入矩形 | 输出矩形 | 计划 extra bytes | PGR delta bytes |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | policy0 | 7.557288 | 16.7146 | 14.209667 | 17.580208 | 2 | 0 | 10,964,992 | 150 | 1.318729 | 16.835417 | 172 | 0 | 0 | 0 | 0 | 0 | 15,298,560 |
| 1 | policy3 | 7.454705 | 16.7219 | 14.381583 | 19.711979 | 7 | 0 | 6,259,940 | 150 | 1.304336 | 16.794271 | 444 | 4450 | 389 | 595 | 206 | 9,744,568 | 9,744,568 |
| 2 | policy3 | 7.430037 | 16.7583 | 14.157343 | 18.578645 | 2 | 0 | 6,259,940 | 150 | 1.298386 | 17.190104 | 444 | 4450 | 389 | 595 | 206 | 9,744,568 | 9,744,568 |
| 3 | policy0 | 7.287107 | 16.7349 | 14.431635 | 19.443230 | 4 | 0 | 10,964,992 | 150 | 1.290788 | 33.477605 | 172 | 0 | 0 | 0 | 0 | 0 | 15,298,560 |
| 组均值 | policy0 / policy3 | 7.422197 / 7.442371 |  |  |  |  |  | 10,964,992 / 6,259,940 | 150 / 150 | 1.304759 / 1.301361 |  | 172 / 444 | 0 / 4450 | 0 / 389 | 0 / 595 | 0 / 206 | 0 / 9,744,568 | 15,298,560 / 9,744,568 |

组差 `+0.020174 ms`，观察值 `+0.2718%`；对照前后 `-0.270181 ms`，组差小于对照漂移，且两轮 policy3 都落在两轮 policy0 之间。因此不能把 0.27% 当作稳定负优化，只能判定为“没有建立收益”。

质量 cohort 的 11 个作业：

| 作业 | 策略 | 采样上传 bytes | 发布数 | HUD 均值 ms | 应用均值 ms | PGR polls | checks | merges | 输入矩形 | 输出矩形 | 计划 extra bytes | RGBA 对照 |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| low | policy0 | 3,932,160 | 62 | 1.407652 | 5.477431 | 73 | 0 | 0 | 0 | 0 | 0 | 参考 |
| low | policy2 | 2,293,852 | 62 | 1.483628 | 5.839466 | 177 | 0 | 0 | 0 | 0 | 0 | 通过 |
| low | policy3 | 2,500,488 | 62 | 1.956775 | 8.173151 | 177 | 2252 | 211 | 292 | 81 | 5,691,192 | 通过 |
| high | policy0 | 3,932,160 | 62 | 2.010621 | 8.772283 | 74 | 0 | 0 | 0 | 0 | 0 | 参考 |
| high | policy3 | 2,500,488 | 62 | 2.023121 | 8.370651 | 177 | 2293 | 206 | 288 | 82 | 6,209,292 | 通过 |
| missing | policy0 | 113,246,208 | 62 | 2.136476 | 8.525551 | 0 | 0 | 0 | 0 | 0 | 0 | 参考 |
| missing | policy3 | 113,246,208 | 62 | 2.229948 | 8.594918 | 0 | 0 | 0 | 0 | 0 | 0 | 通过 |
| busy | policy0 | 654,311,424 | 62 | 4.473741 | 10.247964 | 0 | 0 | 0 | 0 | 0 | 0 | 参考 |
| busy | policy3 | 654,311,424 | 62 | 3.390990 | 6.719340 | 0 | 0 | 0 | 0 | 0 | 0 | 通过 |
| unicode | policy0 | 3,932,160 | 62 | 1.973546 | 8.189236 | 73 | 0 | 0 | 0 | 0 | 0 | 参考 |
| unicode | policy3 | 2,500,488 | 62 | 2.006411 | 8.596333 | 177 | 2177 | 211 | 291 | 80 | 5,669,176 | 通过 |

质量作业是固定时钟/短窗口，应用均值列不能当性能结论；`busy policy3` 看起来更快，是 busy/fallback 路径的固定窗口差异，不是 coalesce 的收益。性能只看上面的 1200 帧 cohort。

### 7. 质量与回归结果

通过的部分：

- 11 个 CPU oracle 测试通过，覆盖 400 轮随机矩形、任意重叠/边界、零候选预算、超大输入、pair/merge 上限、u32 边界、u64 饱和和不可表示并集。
- 11 个真机质量作业形成 8 组有效末帧 RGBA 对照，全部 `changed_pixels=0`、`max_rgba_difference=0`；低/高画质、缺能力、全 busy、Unicode 动态文本都覆盖。
- 每个正常合并作业都记录实际 `coalesce_checks>0`、`merges>0`、`input_rects>output_rects`；缺能力/全 busy 作业计数为 0，没有误用合并。
- 离线核对 694 个 publication 的预算/回退：每次 pair 比较/merge/payload 都符合约束；65536 是 pair 比较上限，不是总 CPU 操作/墙钟上界。
- 关闭 trace 的性能 ABBA 完整结束；没有 GL 错误；源 ZIP 保留；设备作业结束。

没有证明的部分：

- 质量只覆盖有限画面、有限字体/文本、一个设备；不是所有帧、所有字体、所有 GPU。
- 合并只保证“最终图集像素来自权威 source”；不证明驱动内部上传路径、过滤精度或物理显存行为。
- 没有测量纯 GPU 上传时间；`update_texture_part` 的墙钟包含驱动/调度/等待。
- `policy2` 的近期真实 ABBA 也是负收益（约慢 1.85%），说明“精确 damage”本身也没有收益；本轮不能把 policy3 的 0.27% 单独归因于 merge。
- 质量作业的固定时钟数据不能当性能；性能 cohort 的 p99/max 也没有稳定改善。

### 8. 为什么判定为无/负（逐项对比）

第一，调用数确实下降了：PGR 传输矩形 `253 → 206`（约 `-18.6%`），合并计划把 `595` 个输入矩形变成 `206` 个输出（`-65.4%`），pair 检查 `4450`、合并 `389`。采样上传字节 `10,964,992 → 6,259,940`（约 `-42.9%`）。如果只看这些计数，会以为应该更快。

第二，计划字节反而增加了：PGR `coalesce_input_bytes=9,265,092`，`coalesce_output_bytes=9,744,568`，多 `479,476` 字节（约 `+5.2%`）。原因是包围盒把原矩形之间的空白也覆盖了；虽然每次 merge 的额外字节被限制在 8 KiB 内，389 次累计仍然可观。合并减少的是“调用数”，不是“上传像素数”；在带宽受限的设备上，这可能抵消调用节省。

第三，poll 从 `172` 增到 `444`（`+158%`）。这不是一个小变化：PGR 的版本/槽位等待明显增加，说明合并改变了发布节奏或轮询行为。`chosen_version_gap_max` 只是从 19 增到 21，`chosen_version_gap_total` 从 439 变到 437，版本落后程度几乎不变；真正变差的是 poll 次数。新增的同步/轮询成本落在 render 线程上，和“少 47 次上传调用”的收益方向相反。

第四，CPU 规划成本落在发布路径：每个 publication 最多 65,536 次 pair 比较、64 次 merge；本 cohort 实际 `4450 checks + 389 merges` per run。每次 merge 还要从 shadow 复制包围盒到 scratch，包围盒越大复制越多。这些工作都在 render 线程，不在加载期；它们和原上传调用共享同一帧预算。

第五，整帧没有变化：HUD 均值 `1.318729 → 1.304336/1.298386`（略降，但在噪声内），应用均值 `7.557288/7.287107 → 7.454705/7.430037`，处理组均值 `7.442371` 落在对照 `7.422197` 附近。组差 `0.020 ms` 小于对照漂移 `0.270 ms`；这不是“稳定的 0.27% 负优化”，而是“没有可测收益”。按分类可归入负优化/无收益，但不能宣称确定变慢。

第六，quality 的 `low policy3` 固定时钟应用均值 `8.17 ms` 比 policy0 `5.48 ms` 高很多，这不是性能证据，而是固定时钟短窗口下合并规划/额外上传的成本暴露；它至少说明“合并不是免费的”。性能 cohort 的 1200 帧数据才是判据。

第七，和 N-12/N-13 的共性：减少调用数/复用计数不等于减少整帧时间。图集上传的瓶颈可能不是“调用次数”，而是纹理读写依赖、驱动同步、版本等待或 HUD/glyph 处理；coalesce 只改了传输几何，没有改这些。除非能证明 GL 调用开销是主项，否则这类合并很难在 17 ms 帧里跑赢。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：本地提交 `721d9f872d13a1e3097dd2b815e62a07b67ce006`（合并字形上传区域） |
| 默认 | 关闭；生产默认 `policy0`；`policy3` 只是研究候选 |
| 普通路径 | 未接入；没有 Config/UI 开关，没有推广 |
| 主线是否已实施 | 否（main） |
| 研究后端 | SO `ed7b6528043a21fbf830678e928a411e707337877124780feec56948e5580c19`，34 个冻结源文件；Windows 检查/Android API26 构建通过 |
| 质量/契约 | 11 个 CPU oracle、11 个真机 quality、8 组 RGBA、694 个 publication 离线核对完成；性能 ABBA `91378` 完整结束 |
| 后续 | 设备作业结束，没有残留；`policy3` 不推广 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `coalesce` 的有界贪心/预算/溢出处理是一套可复用的矩形规划器；即使不用于上传，也可以用于 damage/统计。
- `pixel_oracle` 的“任意 stale + 权威 source”验证方法可以复用到其他图集传输实验。
- 694 个 publication 的离线预算核对可以作为后续任何合并策略的审计模板。

如果还要继续，必须先满足这些条件：

1. 先测清楚“每次 `glTexSubImage2D` 的 CPU/driver 成本”与“每字节带宽成本”的比例；如果调用成本不是主项，合并没有意义。
2. 把合并从发布路径移到 damage 生成路径，避免每次 publication 重新 pair 扫描；或缓存合并计划，按 damage generation 复用。
3. 解决 poll 增长：合并后版本/槽位等待从 172 增到 444，必须先解释并消除这部分，否则调用节省会被等待吃掉。
4. 限制额外像素总量，不只限制每次 merge 的 8 KiB；例如给每个 publication 一个总 extra 预算。
5. 在低端 GPU/不同驱动上复测；当前 Mali-T880 的结果不能外推。
6. 如果调用成本/带宽比例仍不支持，停止合并方向，转而处理真正的同步点（glyph 发布/flush/纹理读写依赖）。

### 11. 来源与可追溯性

原始记录：

- `本地证据/low-quality-normal/glyph-coalesce-v1-summary.json`：质量/性能汇总、`performance_group_mean_ms={"policy0":7.422197473333333,"policy3":7.442371080416667}`、`observed_treatment_relative_mean_percent=0.27180100173584965`、`control_first_to_last_mean_change_ms=-0.2701814666666671`。
- `本地证据/low-quality-normal/glyph-coalesce-v1-quality-results.json`：11 个质量作业、8 个 comparisons，全部 `changed_pixels=0` / `max_rgba_difference=0`。
- `本地证据/low-quality-normal/glyph-coalesce-v1-performance-results.json`：1200 帧原始数据。
- `本地证据/low-quality-normal/glyph-coalesce-v1-quality-runs/`、`glyph-coalesce-v1-performance-runs/`：逐作业目录。
- measured runner SHA `9e9a0e34279c4f33a8d86d35a3a53e5234cec25ee3fcace812d37f9bd9f3ba12`；offline helper SHA `308d08f211162713070eca72af1259fbef68ebebd510f48b775e50446344f473`。
- 质量 plan/result SHA `877edcddd7795765d714d6eabe20f78648d48a7b051ed9e1a63faec4934e66ac` / `f25afc6c0bec59882dcb767a2ef9d5e39847673c97d64557060c03b3b2684cd0`；性能 plan/result SHA `313550be421013328950e925a9982321c6d3908a28b8d456a8a39f31f4a735b5` / `8fc01feb45d4d3b1f675340d19823e342aad39649213122ec2075e49df482ee0`。
- SO `ed7b6528043a21fbf830678e928a411e707337877124780feec56948e5580c19`；源码 `721d9f872d13a1e3097dd2b815e62a07b67ce006`；34 源。

对应 `本地证据/AGENTS.md` 段落（原文）：

> 最新字形区域合并实验已本地提交721d9f872d13a1e3097dd2b815e62a07b67ce006（合并字形上传区域，主项目未push）；glyph-coalesce-lab SO ed7b6528与34源逐项核对。11个oracle测试/Windows检查/Android构建通过，11个真机quality作业8组RGBA全同；独立离线核对694次publication合并实际执行、每次pair比较/merge/payload预算符合约束，缺能力/全busy无误用。65536是pair比较上限，不是总CPU操作/墙钟上界。关闭trace的完整ABBA policy0/3/3/0墙钟7.5573/7.4547/7.4300/7.2871ms；组均值7.4222/7.4424ms，约慢0.27%，对照前后漂移明显，不称收益。采样上传10964992→6259940B，累计PGR rects253→206，calls/bytes下降仍不能代替整帧收益，方案默认0、不推广。设备91378已真正结束，无该作业残留。

对应 `本地证据/low-quality-normal/README.md` 段落（原文）：

> 2026-10-08通用字形区域合并实验：源码本地提交721d9f8、SO ed7b6528冻结34源；独立oracle11项通过、真机quality11项/8组RGBA全同，任意文字、缺能力与全busy回退覆盖。每次合并允许最多8KiB额外权威像素，pair比较65536/merge64有上限（不是CPU墙钟上界）。policy0/3/3/0完整ABBA墙钟7.5573/7.4547/7.4300/7.2871ms，均7.4222/7.4424ms，观察值慢0.2718%，没有稳定整帧收益；>16.67ms为2/7/2/4，>20ms全0。SF最大16.835/16.794/17.190/33.478ms，不能称尾改善。采样上传10.457→5.970MiB、PGR累计calls253→206，但polls172→444。原测量runner保存glyph-coalesce-v1-measured-runner.py，离线核对694个publication预算及回退，汇总glyph-coalesce-v1-summary.json。保留研究默认关闭，后续首先隔离两通道coverage+alpha表示；不按数字特例，不更改glyph代表或队列布局。

---

## N-15 固定/不可变纹理存储（app 侧）

### 1. 一句话结论与判定

最新真机质量作业 `immutable-text-quality-v3` 在 Case1 `harmony_geometry_multiframe` 失败：ON 与 forced-missing 相对 OFF 在 frame 0/2 各出现 `60` 个像素、`80` 个 RGB 通道、最大差 `1`、alpha 差 `0`，严格 RGBA 门未通过；Case2 和真实 resize 没有执行，`complete=false`，`main_jobs=0`，没有任何性能 cohort。因此本条的判定是：**负优化（质量失败，性能未测）；质量门失败，不能采用；因为没有性能 cohort，不能声称性能回归，也不能称为优化成功**。候选 `font_immutable_storage` 默认 false，不进入生产。

如果把 N-15 强行放进“负优化”分类，只能说它当前是“不可用/未验证”，而不是“测得负收益”。质量门是性能测试的前置条件；在 60 个像素的差异被解释并消除之前，测 FPS 没有意义。

### 2. 问题与初衷

字体图集是 HUD 长帧的主要来源之一：字形更新会修改 2048×2048 RGBA 图集，然后发布给正在被绘制/采样的纹理。原路径使用可变的 `glTexImage2D` 分配（或等价的可变纹理），尺寸/格式/level 在创建后仍可变；驱动可能因此保留更多状态、做更保守的同步，或者在更新时重新分配存储。N-15 的初衷是：在加载期用 `glTexStorage2D` 创建**固定尺寸、固定格式、单 level** 的 RGBA8 纹理，再用完整的 `glTexSubImage2D` 上传同一份像素；纹理内容、布局、UV、过滤、采样和字形代表都不变，只把“存储分配方式”从可变改成不可变。

这个候选的假设是：固定存储可以让驱动更早确定纹理布局，减少更新路径上的隐式重分配/同步；它不一定减少上传字节，但可能降低更新调用的尾延迟。它不是 PBO（N-16），也不改变上传来源；它只改变纹理对象的创建方式。

限制条件很硬：必须保持原图集像素、原 `GlyphBrush` 代表、原布局/UV/透明顺序、原 64 MiB 名义预算；只在加载期创建，晚 warm/busy replacement 仍走原 allocator；不支持 ES3/桌面 4.2 core 或缺少入口时返回 `Declined`，让调用方安全回退到原可变构造；OOM、context 丢失、外部 context、unpack/binding/delete cleanup 失败是 fatal，不能假装“回退成功”；`force_missing` 只用于质量对照，让四个 image 都走 mutable fallback。

### 3. 实现思路与具体机制

1. miniquad 新增 `Texture::try_from_rgba8_immutable(ctx, width, height, bytes) -> Result<ImmutableTextureAttempt, ImmutableTextureError>`。`ImmutableTextureAttempt` 只有两种：`Ready(Texture)` 或 `Declined(reason)`；`Declined` 表示“可以安全走原可变构造”，`Err` 表示“不能安全回退”。
2. 输入校验：`width/height` 非零，`width*height*4 == bytes.len()`；否则 `InvalidData` fatal。平台门：Android/Windows/Linux（非 OHOS）且非 wasm；运行时再检查 `GL_VERSION` 是 `OpenGL ES 3.x` 或桌面 `4.2+ core`；缺入口/不支持 API 返回 `Declined(UnsupportedPlatform/MissingEntryPoints/UnsupportedApi)`。
3. 入口检查：`glGetString`、`glGetError`、`glGetIntegerv`、`glActiveTexture`、`glBindTexture`、`glGenTextures`、`glDeleteTextures`、`glTexStorage2D`、`glTexSubImage2D`、`glTexParameteri`、`glBindBuffer`、`glPixelStorei` 必须全部已加载；否则 `Declined(MissingEntryPoints)`，不做 GL 调用。
4. 尺寸检查：`GL_MAX_TEXTURE_SIZE` 必须为正；`width/height` 超过最大值是 fatal `TextureTooLarge`，不是回退。创建前先清一次 `glGetError`，确保不是“继承的旧错误”。
5. 状态保护：`TextureBinding` 只操作 unit 0，保存 `GL_ACTIVE_TEXTURE` 和 `GL_TEXTURE_BINDING_2D`，结束时恢复；恢复失败是 `BindingRestore` fatal，并带 `active_restore_code`。`ClientMemoryUnpack` 保存/恢复 pixel unpack 状态；unpack cleanup 失败是 fatal 或 `Declined` 取决于错误类型。
6. 创建序列：`glGenTextures`，检查 name 非零；`glBindTexture(GL_TEXTURE_2D, name)`；`glTexStorage2D(GL_TEXTURE_2D, 1, GL_RGBA8, width, height)`；`glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, width, height, GL_RGBA, GL_UNSIGNED_BYTE, bytes)`；设置 `WRAP_S/WRAP_T=CLAMP_TO_EDGE`、`MIN/MAG_FILTER=LINEAR`。每一步都检查 GL error；OOM/context lost（0x0505/0x0507）是 fatal，其他创建错误是 `Declined` 或 fatal，取决于 cleanup 是否可信。
7. 失败清理：如果 operation 成功但 unpack cleanup 或 binding cleanup 失败，不能返回 Ready；如果 name 非零且不是 foreign context，尝试 `glDeleteTextures` 并记录 `delete_code`；cleanup 失败整体是 `Err`。只有“operation 失败 + cleanup 干净 + failure 是 Creation”才转成 `Declined`，让调用方走 mutable。
8. app 侧接点在 `font_atlas_pool.rs` 的 `make_image`：只有 `self.immutable.requested && loading` 才尝试；`force_missing` 直接构造 `Declined(MissingEntryPoints)`，让四个 image 都走 mutable fallback；尺寸/字节不匹配是 fatal 并 block；`Ready` 计 `accepted_images`/`initialized_rgba_bytes`；`Declined` 计 `declined_images`，然后继续原 `image(width, height, shadow, guarded)`；`Err` 计 `fatal_errors`、`blocked=true`、记录 context，返回错误，调用方 `loading_failure` 不再发第二次 GL 恢复。
9. 失败传播：`client_guard_blocked()` 合并 PBO blocked 和 immutable blocked；`immutable_blocked()` 检查本 pool 或同 context 的全局失败；`loading_failure` 在 immutable blocked 时直接 `self.api=None`，让 native context 拥有清理，不再 `retire_slots`/`drain_errors`；`publish`/`update` 在 blocked 时返回 invalid，禁止继续绘制。
10. 报告：`immutable_storage` 字段包含 `requested/force_missing/attempts/accepted_images/initialized_rgba_bytes/declined_images/mutable_images/fatal_errors/blocked/reason`。`initialized_rgba_bytes` 是逻辑 payload，不是物理显存；`scope` 明确说“Loading allocation method only; late replacement keeps the original allocator”。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `prpr/src/ui/font_atlas_pool.rs` | `ImmutableStats`、`configure_immutable`、`make_image`、`warm_internal`、`immutable_blocked` | app 侧加载期分配与失败传播 |
| `prpr/src/ui/text.rs` | `research_immutable_storage`、`research_immutable_blocked` | 研究入口与状态查询 |
| `prpr/src/font_upload_pbo.rs` | 独立的 PBO 候选，不是本条 | 与 immutable 互斥的传输实验 |
| miniquad `src/graphics/texture.rs` | `ImmutableTextureAttempt`、`ImmutableTextureDecline`、`ImmutableTextureFailure`、`immutable_api`、`create`、`try_from_rgba8_immutable` | 固定存储 API |
| miniquad `src/native/gl.rs` | `glTexStorage2D` | 入口声明 |
| `phira/src/render_research_immutable_text_probe.rs` | Case0/Case1/Case2 质量探针 | 真机严格 RGBA 对照 |

`font_atlas_pool.rs` 的 immutable 路径完整核心（第 454 至 508 行）：

```rust
// prpr/src/ui/font_atlas_pool.rs:454-508
    fn make_image(&mut self, width: u32, height: u32, shadow: &[u8], loading: bool) -> Result<Texture2D, String> {
        #[cfg(not(feature = "render_research"))]
        let _ = loading;
        #[cfg(feature = "render_research")]
        if self.immutable.requested && loading {
            if self.immutable.blocked {
                return Err("Immutable font atlas state is blocked".into());
            }
            self.immutable.attempts += 1;
            let result = if self.immutable.force_missing {
                Ok(miniquad::ImmutableTextureAttempt::Declined(miniquad::ImmutableTextureDecline::MissingEntryPoints))
            } else if bitmap_bytes(width, height) != Some(shadow.len()) {
                self.immutable.blocked = true;
                IMMUTABLE_FAILURE_CONTEXT.with(|value| value.set(Some(self.context)));
                self.immutable.fatal_errors += 1;
                self.immutable.reason = Some("Invalid immutable font atlas dimensions".into());
                return Err(self.immutable.reason.clone().unwrap());
            } else {
                miniquad::Texture::try_from_rgba8_immutable(
                    unsafe { get_internal_gl() }.quad_context,
                    width as u16,
                    height as u16,
                    shadow,
                )
            };
            match result {
                Ok(miniquad::ImmutableTextureAttempt::Ready(raw)) => {
                    self.immutable.accepted_images += 1;
                    self.immutable.initialized_rgba_bytes += shadow.len() as u64;
                    return Ok(Texture2D::from_miniquad_texture(raw));
                }
                Ok(miniquad::ImmutableTextureAttempt::Declined(reason)) => {
                    self.immutable.declined_images += 1;
                    self.immutable.reason = Some(format!("{reason:?}"));
                }
                Err(error) => {
                    self.immutable.fatal_errors += 1;
                    self.immutable.blocked = true;
                    IMMUTABLE_FAILURE_CONTEXT.with(|value| value.set(Some(self.context)));
                    self.immutable.reason = Some(error.to_string());
                    return Err(error.to_string());
                }
            }
        }
        #[cfg(feature = "render_research")]
        let guarded = self.client_guard_required();
        #[cfg(not(feature = "render_research"))]
        let guarded = false;
        let result = image(width, height, shadow, guarded);
        #[cfg(feature = "render_research")]
        if self.immutable.requested && result.is_ok() {
            self.immutable.mutable_images += 1;
        }
        #[cfg(feature = "render_research")]
        if guarded {
            if let Err(reason) = &result { self.block_client_guard(reason.clone()); }
        }
        result
    }
```

miniquad 的固定存储构造函数（最终文件 `src/graphics/texture.rs:653-700`）：

```rust
// miniquad 19e97bd:src/graphics/texture.rs:653-700
    /// Creates one initialized, fixed-size RGBA8 level using core immutable storage.
    /// Keeps Linear/Clamp sampling and exact caller bytes. Updates remain legal;
    /// resize is not legal for this texture. The returned owner still requires delete.
    /// A Declined result permits the original mutable constructor; Err does not.
    /// Unsupported platforms perform no GL calls. Requires the creating GL context
    /// to be current on its render thread, as for the existing Texture constructors.
    pub fn try_from_rgba8_immutable(
        ctx: &mut Context,
        width: u16,
        height: u16,
        bytes: &[u8],
    ) -> Result<ImmutableTextureAttempt, ImmutableTextureError> {
        if !rgba8_input_valid(width, height, bytes.len()) {
            return Err(ImmutableTextureError::new(
                ImmutableTextureFailure::InvalidData {
                    width,
                    height,
                    bytes: bytes.len(),
                },
            ));
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            any(
                target_os = "windows",
                target_os = "android",
                all(target_os = "linux", not(target_env = "ohos"))
            )
        ))]
        {
            immutable::create(ctx, width, height, bytes)
        }
        #[cfg(not(all(
            not(target_arch = "wasm32"),
            any(
                target_os = "windows",
                target_os = "android",
                all(target_os = "linux", not(target_env = "ohos"))
            )
        )))]
        {
            let _ = ctx;
            Ok(ImmutableTextureAttempt::Declined(
                ImmutableTextureDecline::UnsupportedPlatform,
            ))
        }
    }
```

miniquad `create` 的创建/上传核心（最终文件 `src/graphics/texture.rs:400-520`，只去掉重复的 error-check 包装说明，保留全部 GL 调用顺序）：

```rust
// miniquad 19e97bd:src/graphics/texture.rs:400-520
    pub(super) fn create(
        ctx: &mut Context,
        width: u16,
        height: u16,
        bytes: &[u8],
    ) -> Result<ImmutableTextureAttempt, ImmutableTextureError> {
        let loaded = [
            "glGetString",
            "glGetError",
            "glGetIntegerv",
            "glActiveTexture",
            "glBindTexture",
            "glGenTextures",
            "glDeleteTextures",
            "glTexStorage2D",
            "glTexSubImage2D",
            "glTexParameteri",
            "glBindBuffer",
            "glPixelStorei",
        ]
        .iter()
        .all(|name| crate::native::gl::is_function_loaded(name));
        if !loaded {
            return Ok(ImmutableTextureAttempt::Declined(
                ImmutableTextureDecline::MissingEntryPoints,
            ));
        }
        unsafe {
            check("preexisting immutable texture error").map_err(ImmutableTextureError::new)?;
            let version = glGetString(GL_VERSION);
            check("immutable version query").map_err(ImmutableTextureError::new)?;
            let api = if version.is_null() {
                None
            } else {
                immutable_api(std::ffi::CStr::from_ptr(version as _).to_bytes())
            };
            let Some(api) = api else {
                return Ok(ImmutableTextureAttempt::Declined(
                    ImmutableTextureDecline::UnsupportedApi,
                ));
            };
            if api == ImmutableApi::Desktop {
                let mut profile = 0;
                glGetIntegerv(CONTEXT_PROFILE_MASK, &mut profile);
                check("immutable desktop profile query").map_err(ImmutableTextureError::new)?;
                if profile != CORE_PROFILE_BIT {
                    return Ok(ImmutableTextureAttempt::Declined(
                        ImmutableTextureDecline::UnsupportedApi,
                    ));
                }
            }
            let mut maximum = 0;
            glGetIntegerv(GL_MAX_TEXTURE_SIZE, &mut maximum);
            check("immutable maximum size query").map_err(ImmutableTextureError::new)?;
            if maximum <= 0 {
                return Err(ImmutableTextureError::new(
                    ImmutableTextureFailure::InvalidMaximum(maximum),
                ));
            }
            if width as i32 > maximum || height as i32 > maximum {
                return Err(ImmutableTextureError::new(
                    ImmutableTextureFailure::TextureTooLarge { maximum },
                ));
            }
        }
        let binding = unsafe { TextureBinding::capture(ctx)? };
        let (operation, unpack_cleanup, name) = {
            match ClientMemoryUnpack::begin(&mut *binding.ctx) {
                Err(failure) => (Err(ImmutableTextureFailure::Unpack(failure)), None, 0),
                Ok(unpack) => {
                    let mut name = 0;
                    let operation = (|| unsafe {
                        glGenTextures(1, &mut name);
                        check_creation("immutable texture name")?;
                        if name == 0 {
                            return Err(ImmutableTextureFailure::Creation(
                                ImmutableTextureDecline::NameZero,
                            ));
                        }
                        glBindTexture(GL_TEXTURE_2D, name);
                        check("immutable texture bind")?;
                        glTexStorage2D(GL_TEXTURE_2D, 1, GL_RGBA8, width as i32, height as i32);
                        check_creation("immutable texture storage")?;
                        glTexSubImage2D(
                            GL_TEXTURE_2D,
                            0,
                            0,
                            0,
                            width as i32,
                            height as i32,
                            GL_RGBA,
                            GL_UNSIGNED_BYTE,
                            bytes.as_ptr().cast(),
                        );
                        check_creation("immutable full RGBA8 upload")?;
                        for (parameter, value) in [
                            (GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE),
                            (GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE),
                            (GL_TEXTURE_MIN_FILTER, GL_LINEAR),
                            (GL_TEXTURE_MAG_FILTER, GL_LINEAR),
                        ] {
                            glTexParameteri(GL_TEXTURE_2D, parameter, value as i32);
                            check_creation("immutable texture parameters")?;
                        }
                        Ok(())
                    })();
                    let unpack_cleanup = unpack.finish().err();
                    (operation, unpack_cleanup, name)
                }
            }
        };
        let foreign = matches!(
            unpack_cleanup,
            Some(crate::PixelUnpackError::ForeignContext)
        );
        let binding_cleanup = binding.finish().err();
        if operation.is_ok() && unpack_cleanup.is_none() && binding_cleanup.is_none() {
            return Ok(ImmutableTextureAttempt::Ready(Texture {
                texture: name,
                width: width as u32,
                height: height as u32,
                format: TextureFormat::RGBA8,
            }));
        }
        let foreign = foreign
            || matches!(
                binding_cleanup,
                Some(ImmutableTextureFailure::ForeignContext)
            );
        let delete_code = if name != 0 && !foreign {
            unsafe {
                glDeleteTextures(1, &name);
                match glGetError() {
                    0 => None,
                    code => Some(code),
                }
            }
        } else {
            None
        };
        let failure = operation.err().unwrap_or_else(|| {
            if foreign {
                ImmutableTextureFailure::ForeignContext
            } else if let Some(error) = unpack_cleanup.clone() {
                ImmutableTextureFailure::Unpack(error)
            } else {
                binding_cleanup.clone().unwrap()
            }
        });
        let error = ImmutableTextureError {
            failure,
            unpack_cleanup,
            binding_cleanup,
            delete_code,
        };
        if !error.is_cleanup_failed() {
            if let ImmutableTextureFailure::Creation(reason) = error.failure {
                return Ok(ImmutableTextureAttempt::Declined(reason));
            }
        }
        Err(error)
    }
```

### 5. 开关、默认值、构建特性

| 开关/特性 | 默认 | 作用 |
|---|---|---|
| `font_atlas_pool` / `render_research` feature | 研究构建启用 | 生产默认构建不编译 immutable 路径 |
| `font_immutable_storage` 请求键 | false | 加载期请求固定存储 |
| `font_immutable_storage_force_missing` | false | 强制四个 image 走 `Declined`/mutable |
| `IMMUTABLE_REQUEST` 线程局部 | `(false,false)` | 未配置时完全不尝试 |
| 生产 Config/UI | 无 | 没有把 immutable 接进普通设置 |
| 候选默认 | false | `defaultcandidatefalse`，未推广 |

`configure_immutable(requested, force_missing)` 只在研究入口调用；`research_immutable_storage` 把它转到 `font_atlas_pool`。生产构建没有这些字段，`make_image` 直接走原 `image(width, height, shadow, guarded)`。不支持平台/API 返回 `Declined`，不会做 GL 调用；fatal 会 block，不继续绘制。

### 6. 完整测量数据（表格）

构建/运行身份：

| 项 | 值 |
|---|---|
| 质量作业 | `immutable-text-quality-v3` |
| run id | `immutable-text-20261013T185339-1a9a8b14` |
| SO | `cf0ea8c3459a65a81db4dfd3f638d83d7251a3920e7bd0f279251d5d9b789d57` |
| 源码 pin | `33c42a8f8ec56023ab9aa640c1b54b9a4fa778bf` |
| 源文件数 | 65 |
| features | `render_research,render_atlas_pool,chart_pre_compile` |
| collector SHA | `a86b68d36cfe62721b3a9c77fa2773611256ca7d91d394e3e2069fd5e271568a` |
| host proof | `immutable-text-host-input-proof-v4.json`，20 项通过 |
| 构建耗时 | 130.5 s |
| 开始/失败 | 2026-10-13T18:53:30Z / 18:56:02Z |
| `complete` | false |
| `main_jobs` | 0 |
| `performance_proven` | false |

Case0 结果（PGR 数字/COMBO/空/新/重复/变化，6 帧 × 3 模式）：

| 模式 | immutable 计数 | 6 帧 strict RGBA | rect bits | native shape/order |
|---|---|---|---|---|
| OFF（mutable_pool） | 未请求，attempts 0 | 全部相同 | 相同 | 相同 |
| ON（immutable_accepted） | attempts 4 / accepted 4 / declined 0 / mutable 0 | 全部相同 | 相同 | 相同 |
| missing（forced） | attempts 4 / accepted 0 / declined 4 / mutable 4 | 全部相同 | 相同 | 相同 |

Case0 还确认：ON 实际 4 个 immutable image 被接受；missing 的 4 个 image 全部 `MissingEntryPoints` 回退为 mutable；OFF 原先多 row 全 blank 的问题在支持库 growth 修复后消失。这个修复是 Macro 侧 VBO/IBO growth 前清 cached attrs，不是 immutable 本身。

Case1 结果（Harmony 居中 Unicode/alpha 0.65、Latin transform/maxwidth/multiline、HΩ）：

| 帧 | OFF vs ON | OFF vs missing | ON vs missing | 差异 |
|---|---|---|---|---|
| 0 | 不同 | 不同 | 相同 | 60 pixels / 80 RGB channels / max 1 / alpha 0 / bbox bottom-up `1525..1688, 1237..1262` / top-down `177..202` |
| 1 | 相同 | 相同 | 相同 | 0 |
| 2 | 不同 | 不同 | 相同 | 60 pixels / 80 RGB channels / max 1 / alpha 0 / 同一 bbox |

补充事实：

- `OFF0 == OFF2`，`ON0 == ON2`，`missing0 == missing2`；同一模式内 frame 0 与 frame 2 一致。
- `ON == missing` 在三个 frame 都 exact；差异只出现在“OFF vs pooled（ON/missing）”之间。
- Latin transform/maxwidth/multiline 两 row 无差；`HΩ` frame 1 exact。
- 失败字符串：`RuntimeError('Native probe: Strict immutable quality comparison failed in harmony_geometry_multiframe')`。
- Case2 与真实 resize 未执行；`case-2` 文件不存在，`actual_resize_supported=true` 只是能力字段，不是执行结果。

### 7. 质量与回归结果

通过的部分：

- Case0 的 6 帧 × 3 模式全部 strict RGBA 相同，rect bits 和 native draw shape/order 相同；ON 的 4 个 immutable image 实际 accepted，missing 的 4 个实际 declined 并 mutable fallback。
- 支持库 growth 修复后，OFF 原先的多 row 全 blank 消失；这是独立修复，不是 immutable 的画质证明。
- 失败作业的 cleanup 完整：`pid_empty=true`、`stayon_restored=true`、`private_bytes_stat_unchanged=true`、`config_equal=true`、`current_nonconfig_preserved=true`、`font_backup_matches=true`、`restore_pending=false`；原 Config/font/源 ZIP/旧证据保持。
- `source_unchanged=true`、`protected_unchanged=true`；没有修改生产源码或原 0.7.1 包。

失败的部分：

- Case1 的严格门要求“每个 frame 的 RGBA 逐字节相同”，frame 0/2 各有 60 个像素不同；即使最大差只有 1、alpha 不变，也不能通过。质量门没有“容差 1”的降级，也不应该为了通过把门槛从 0 改成 1。
- Case2 和 resize 未执行，覆盖不完整；`complete=false`。
- 没有性能数据：`main_jobs=0`，`performance_proven=false`；没有 FPS、app wall、glyph 上传耗时或整帧 ABBA。
- 差异的根因尚未证明。`ON == missing` 说明“immutable accepted”和“forced mutable fallback”在 pooled 路径上表现一致；`OFF` 是另一个 painter/pool 实例。差异可能来自 pool/painter 的 brush 代表、缓存种子、draw cache 的 lossy-key 代表或上传顺序，而不是 `glTexStorage2D` 本身。这个判断是假设，不是结论；需要专门的 OFF/ON 同条件诊断。

### 8. 为什么判定为无/负（逐项对比）

第一，质量门是性能门的必要条件。Case1 的 60 像素差虽然很小，但严格门要求 0；在未解释/未消除之前，任何 FPS 数字都没有意义。当前没有性能 cohort，所以不能写“慢多少”或“快多少”。

第二，不能把 60 像素差直接归因于 immutable storage。`ON == missing` 是关键：missing 走的是 `Declined` 后的 mutable `image(width, height, shadow, guarded)`，和 ON 的 `TexStorage2D` 不是同一条 GL 分配路径，但两者输出完全相同；OFF 与它们不同。更像是“OFF 的 painter/pool 与 pooled 路径之间的字形代表/缓存种子差异”，而不是“TexStorage 改变了像素”。但这只是模式推断，必须用同条件诊断证明。

第三，Case0 的通过不能抵消 Case1 的失败。Case0 是 PGR 数字/COMBO/空/新/重复/变化，字形集合和布局较窄；Case1 是 Harmony 居中 Unicode/alpha 0.65、Latin transform/maxwidth/multiline、HΩ，覆盖了更多字形/布局/透明度。质量门要求两类都过，当前只过了一类。

第四，不能把“没有性能数据”写成“性能负优化”。N-15 与 N-12/N-13 不同：后者有完整 ABBA 和组均值；N-15 只有质量失败和 `performance_proven=false`。按证据等级，N-15 只能归入“质量失败、性能未测、不可用/未验证”，不能归入“测得负收益”。

第五，候选默认 false 是正确状态。即使后续质量修好，也需要重新做同 SO 的 OFF/ON 性能 ABBA，证明 immutable 分配在真实 Main 的 glyph 更新路径上有收益；当前没有任何这样的证据。默认关闭可以避免把未验证的分配方式带进生产。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：Macro 侧 `de34611bca60abda9c4a2d8ebea568970fd7a850`；miniquad 侧 `19e97bd3d9b0ecce86ae50558311175a92d933a3`；Root hook `09b2370cac399ecf45a6e22302c728a7725f176d`、质量探针 `77d7d7ac31402ce35b34c0c35f04b8da3c06c0c2`；库侧已推送各自远端，Root 侧未 push 主项目 |
| 默认 | 关闭；生产默认 false；研究请求 `font_immutable_storage` 可复现 |
| 普通路径 | 未接入；没有 Config/UI 开关；没有 APK 安装/生产推广 |
| 主线是否已实施 | 否（main） |
| 研究后端 | 质量 SO `cf0ea8c3459a65a81db4dfd3f638d83d7251a3920e7bd0f279251d5d9b789d57`，由 `33c42a8f8ec56023ab9aa640c1b54b9a4fa778bf` 构建，65 源冻结 |
| 质量/契约 | 质量门失败：v3 作业在 Case1 失败，Case2/resize 未执行；性能未测。判定为负优化（质量失败），但不能声称性能回归 |
| 后续 | 设备已清理并交还：PID 空、stayon 2、字体 backup 匹配、Config 原 bytes、原 ZIP/旧证据保持 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `try_from_rgba8_immutable` 的能力门、入口检查、状态保存/恢复、fatal/Declined 区分和 cleanup 诊断是一套完整的资源生命周期实现；即使最终不用于字体，也可以复用到其他固定尺寸纹理。
- `font_atlas_pool` 的 loading-only 接点、4 slot 预算、mutable fallback 和 `blocked` 传播可以保留；它避免了“immutable 失败后继续绘制”的风险。
- Case0/Case1 的严格 RGBA/rect bits/native shape 对照工具可以继续用于诊断。

如果还要继续，必须先满足这些条件：

1. 先定位 Case1 的 60 像素差：在 OFF 与 ON 之间统一 empty-brush 模板、painter/pool 初始化、brush 代表和 draw cache 种子；确认差异是否来自 pool 路径，而不是 TexStorage。
2. 保持严格门为 0，不降门槛；修复后重跑 Case1 的 frame 0/1/2，要求 `ON==OFF` 和 `missing==OFF` 都 exact。
3. 执行 Case2 和真实 resize；resize 是 immutable 的明确限制（固定尺寸不可 resize），必须证明 pool 在 shape 变化时回退/重建正确。
4. 质量全过后，再做同 SO 的真实 Main OFF/ON ABBA；只改 immutable 分配，其他 glyph 代表/布局/上传顺序/预算不变；记录 glyph 上传耗时、HUD 长帧、整帧 app wall 和 >30ms 帧。
5. 需要全曲、多 GPU、EGL loss/context 重建；当前只有一个 Mali-T880 和一个有限窗口。
6. 如果修复成本高于收益，或者性能 ABBA 仍无收益，应停止 immutable 方向；不要为了“固定存储”继续扩大实现。

### 11. 来源与可追溯性

原始记录：

- `本地证据/low-quality-normal/immutable-text-quality-v3-execution/results.json`：`complete=false`、`failure`、`run_id`、`main_jobs=0`、`source_unchanged=true`、`protected_unchanged=true`、cleanup 字段。
- `本地证据/low-quality-normal/immutable-text-quality-v3-execution/failure.json`：失败快照。
- `本地证据/low-quality-normal/immutable-text-quality-v3-execution/runs/immutable-text-20261013T185339-1a9a8b14/case-0.json`、`case-1.json`：逐帧 strict RGBA、rect bits、native draw shape/order、immutable 计数。
- 同目录 `case-1-off-frame-0.rgba`、`case-1-on-frame-0.rgba`、`case-1-missing-frame-0.rgba` 等原始 RGBA；本文的 60/80/max1/alpha0/bbox 由这些原始文件逐字节重算，top-down bbox 由 bottom-up `1237..1262` 换算为 `177..202`。
- `本地证据/low-quality-normal/immutable-text-quality-pins-v2.json`：SO/source/manifest/host proof/保护 hash。
- `本地证据/low-quality-normal/immutable-text-quality-build-proof-v1.json`：另一版 SO `1989866dea3695aafcc361966855acddd7a3619398bd8ec844c0465de0d780ec` 的构建证明，`quality_proven=false`、`performance_proven=false`；本文使用 v2 SO `cf0ea8c3459a65a81db4dfd3f638d83d7251a3920e7bd0f279251d5d9b789d57` 的 v3 作业。
- miniquad `19e97bd3d9b0ecce86ae50558311175a92d933a3`：`src/graphics/texture.rs`、`src/graphics/pixel_unpack.rs`、`src/graphics/texture_immutable_tests.rs`、`src/native/gl.rs`、`src/graphics.rs`；5 文件、953 insert/42 delete；37 tests 通过。
- 源码行号：`prpr/src/ui/font_atlas_pool.rs:454-508`；`prpr/src/ui/text.rs:431-440`；miniquad `src/graphics/texture.rs:400-520`、`653-700`；`src/native/gl.rs:625`。

对应 `本地证据/AGENTS.md` 段落（原文）：

> 2026-10-14 最新修复后GPU真实终态：唯一SAME55400实际terminal1/2c6d18，run immutable-text-20261013T185339-1a9a8b14/SOcf0/source33c65/collector a86。native Case0 PGR数字-COMBO空/新/重复/变化6frames×3模式全部strictRGBA+rectbits+native shape/order exact，ON实际4immutable accepted/missing4mutable，原OFF多row全blank消失（支持库growth修复有效，不伪已记录GLuint复用）。Case1 Harmony居中Unicode/alpha.65严格失败：OFF0==OFF2、ON0==ON2、missing0==missing2且ON==missing三个frame均exact；ON/missing对OFF0/2同60pixels/80RGBchannels/max1/alpha0，topdownbbox1525..1688,177..202，Latintransform/maxwidth/multiline两row无差，HΩframe1exact。Rootactualread985254及独立raw核一致；不算fixedstorage画质通过、不降门槛1、不计性能收益，Case2+resize未执行。

> 2026-10-14 immutable质量入口已localcommit77d7d7ac31402ce35b34c0c35f04b8da3c06c0c2（增加固定纹理存储对照/精确三文件/324insert12delete/mainNoPush），独立最终复核及cleanup副核无blocker。新release SAME38562真正terminal0/d55e2e，201.484s，SO1989866dea3695aafcc361966855acddd7a3619398bd8ec844c0465de0d780ec；manifest3a88ebd5cbd013637154fb2b3f672f79f95b66840849d3e89cf9177de40255bd/65源working=frozen逐hash核，immutable-text-quality-build-proof-v1.json原存。Collector仍HOST准备/未Device，3原ZIP+2fontbackup+5旧主证据实际hash已10项保护map。尚无新GPU质量/性能，不计入已汇报收益，不以编译/静态复核称无损已真机通过；下一只唯一新质量job内部OFF/ON/missing，之后普通Main尖峰单候选对照。source现在77d clean/原Config-font恢复保持/完整goalactive/defaultcandidatefalse。

`本地证据/low-quality-normal/README.md` 的当前版本没有 immutable storage 的质量/性能段落（该实验晚于 README 的最后更新）；对应历史材料在 `本地证据/AGENTS.md` 的上两条。这里不引用不存在的 README 段落。

---

## N-16 PBO 字库上传（完整帧）

### 1. 一句话结论与判定

在 2048×2048 字形图集上用 ES3 pixel unpack buffer 替换客户端指针上传，`submit` p50 从 `130.252 ms` 降到 `18.643/27.714 ms`，但同一试验的 `glFinish` 同步完成时间从 `132.279 ms` 变为 `136.740/135.232 ms`：提交提前了，整帧没有收益，甚至略差。3 项严格 RGBA 对照（mode 2/8/9 对 mode 0）全部相同，说明像素/矩形/绘制顺序没有被改变。判定：**无优化（submit 降、completed 不降，无整帧收益）**，仅保留为诊断，不接入生产。

这条结论的关键是“submit 不是完成”。PBO 只改变了客户端内存的生命周期，没有消除“图集纹理正在被读取，下一批写入必须等待/同步”的纹理读写依赖；`glFinish` 把这一点暴露出来。任何只报 submit 下降的说法都会误导。

### 2. 问题与初衷

字形图集更新时，原路径把 CPU 的 RGBA 字节直接作为 `glTexSubImage2D` 的客户端指针。驱动可能在返回前等待这批客户端内存可用/复制完成，或者在后续绘制读取同一纹理时等待写入完成。N-16 的想法是：用 PBO 把像素先传到 GL buffer，再让 `glTexSubImage2D` 从 buffer offset 读取；这样客户端指针可以立即复用，提交调用可能更早返回。模式 8 用“一个 packed buffer 装所有文本的矩形”，模式 9 用“每个文本一个 buffer”，两者都保持原矩形、原字节、原纹理名和原绘制命令顺序。

这不是 gameplay FPS 优化，而是一个同步诊断：它试图回答“客户端指针生命周期是不是 `TexSubImage2D` 返回慢的原因”。如果是，PBO 的 submit 应该大幅下降；如果 `glFinish` 完成时间不降，说明真正等待在纹理消费/驱动同步，而不是客户端内存。

### 3. 实现思路与具体机制

1. 固定 fixture：2048×2048 图集，22 个文本组（`Petrichor`、`AT Lv.17`、数字、`PERFECT+`、`AUTOPLAY` 等），每组有 1 个或多个矩形写和 4 顶点/6 索引；总计 99 次上传。每组有独立 model 矩阵，绘制 22 条记录，避免“把所有文本合成一个 draw”把结果做漂亮。
2. mode 0 是原路径：按组交错 `upload(atlas, write)` 和 `draw_group(group, atlas)`；每组写之前 `flush`，保持原命令顺序。
3. mode 2 是对照：先把所有组的写全部 `upload` 到同一图集，再统一 `draw_group`。它不改变像素/矩形/绘制顺序，只把“写”和“读”分开；这个模式用来判断“交错读写”本身是不是主因。
4. mode 8：把所有组的矩形字节按顺序拼成一个 `Vec<u8>`，创建一个 PBO，`glBufferData(PIXEL_UNPACK_BUFFER, len, bytes, GL_STATIC_DRAW)`；每个矩形用 `upload_pbo(atlas, write, pbo, global_offset)`，offset 累加；所有写完成后按原顺序 draw。
5. mode 9：每个文本组创建自己的 PBO，组内矩形按 local offset 上传；组间仍按原顺序 draw；PBO 在 trial 结束时统一删除。
6. `upload_pbo` 保存/恢复 `GL_ACTIVE_TEXTURE`、`GL_TEXTURE_BINDING_2D`、`GL_PIXEL_UNPACK_BUFFER_BINDING`；绑定目标纹理、绑定 PBO、`glPixelStorei(GL_UNPACK_ALIGNMENT, 1)`、`glTexSubImage2D(GL_TEXTURE_2D, 0, write.min[0] as _, write.min[1] as _, (write.max[0]-write.min[0]) as _, (write.max[1]-write.min[1]) as _, GL_RGBA, GL_UNSIGNED_BYTE, offset as *const std::ffi::c_void)`，再恢复 PBO/纹理/active texture；每步检查 GL error。
7. 计时：每个 trial 先 `flush` + `glFinish` 清空旧工作；`start=Instant::now()`；执行当前 mode 的全部上传/绘制；`flush`；`submit=elapsed`；再 `glFinish`；`completed=elapsed`。`submit` 是“命令提交返回”，`completed` 是“GPU 完成”。每个 mode 16 个 trial，前 4 个丢弃，取后 12 个的 p50/max。
8. 纹理在 trial 内重新创建（`from_rgba8(2048,2048,initial)`），所以每个 mode 都从同一初始图集开始；`max_texture_bytes` 记录同时拥有的纹理字节，本 fixture 为 16 MiB。
9. 质量：每个 mode 第 16 个 trial 读回默认 framebuffer 的 RGBA，导出 PNG；`pixels.json` 以 mode 0 为 reference，比较 mode 2/8/9 的尺寸、逐通道差异和 SHA。3 项比较全部 `equal=true`、`different_channels=0`。
10. 这是同步 `glFinish` 微基准，不是 FPS 测量；没有 swap/compositor/游戏循环，也没有多帧时序。它只能回答“这一组上传/绘制的完成时间”，不能回答“完整 gameplay 是否更快”。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `本地证据/glyph-stream-diagnostic-pbo/accepted-input-lib.rs` | `make_pbo`、`upload_pbo`、`capture`、`main` | PBO 诊断载体 |
| 同目录 `accepted-input-libglyph_stream_probe.so` | 诊断 SO | 真机执行 |
| 同目录 `run_device.py` | 设备 runner | 部署/启动/收集 |
| 同目录 `pixels.json` | RGBA 对照 | 3 项严格比较 |
| 同目录 `results.txt` / `logcat.txt` | 原始计时 | submit/completed p50/max |

`make_pbo` 与 `upload_pbo` 的完整实现：

```rust
// accepted-input-lib.rs:99-135
// ES3 pixel unpack buffers retain exactly the same upload rectangles, bytes,
// texture name and GL command order. This diagnostic tests whether client
// pointer lifetime is making the driver wait before returning TexSubImage2D.
// No GPU waits or cache reordering are added to either PBO mode.
const PIXEL_UNPACK_BUFFER:u32=0x88EC;
const PIXEL_UNPACK_BUFFER_BINDING:u32=0x88EF;

fn make_pbo(bytes:&[u8]) -> u32 {
    unsafe {
        assert_eq!(miniquad::gl::glGetError(),0);
        let mut previous=0;miniquad::gl::glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING,&mut previous);
        let mut name=0;miniquad::gl::glGenBuffers(1,&mut name);
        miniquad::gl::glBindBuffer(PIXEL_UNPACK_BUFFER,name);
        miniquad::gl::glBufferData(PIXEL_UNPACK_BUFFER,bytes.len() as _,bytes.as_ptr() as _,0x88E0);
        miniquad::gl::glBindBuffer(PIXEL_UNPACK_BUFFER,previous as _);
        assert_eq!(miniquad::gl::glGetError(),0,"PBO allocation/upload failed; reject diagnostic");
        name
    }
}

fn upload_pbo(texture:Texture2D,write:&WriteRect,pbo:u32,offset:usize) {
    unsafe {
        let mut previous_active=0;let mut previous_texture=0;let mut previous_pbo=0;
        miniquad::gl::glGetIntegerv(0x84E0,&mut previous_active);
        miniquad::gl::glActiveTexture(miniquad::gl::GL_TEXTURE0);
        miniquad::gl::glGetIntegerv(0x8069,&mut previous_texture);
        miniquad::gl::glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING,&mut previous_pbo);
        miniquad::gl::glBindTexture(miniquad::gl::GL_TEXTURE_2D,texture.raw_miniquad_texture_handle().gl_internal_id());
        miniquad::gl::glBindBuffer(PIXEL_UNPACK_BUFFER,pbo);
        miniquad::gl::glPixelStorei(miniquad::gl::GL_UNPACK_ALIGNMENT,1);
        miniquad::gl::glTexSubImage2D(miniquad::gl::GL_TEXTURE_2D,0,write.min[0] as _,write.min[1] as _,
            (write.max[0]-write.min[0]) as _,(write.max[1]-write.min[1]) as _,miniquad::gl::GL_RGBA,
            miniquad::gl::GL_UNSIGNED_BYTE,offset as *const std::ffi::c_void);
        miniquad::gl::glBindBuffer(PIXEL_UNPACK_BUFFER,previous_pbo as _);
        miniquad::gl::glBindTexture(miniquad::gl::GL_TEXTURE_2D,previous_texture as _);
        miniquad::gl::glActiveTexture(previous_active as _);
        assert_eq!(miniquad::gl::glGetError(),0,"PBO rect failed; reject diagnostic");
    }
}
```

mode 8/9 的执行与计时（`accepted-input-lib.rs:242-345` 的核心片段）：

```rust
            if mode==8 || mode==9 {
                let packed:Vec<u8>=if mode==8 {groups.iter().flat_map(|g|g.writes.iter().flat_map(|w|w.rgba.iter().copied())).collect()} else {Vec::new()};
                let global=if mode==8 {let pbo=make_pbo(&packed);unpack_buffers.push(pbo);pbo} else {0};
                let mut global_offset=0;
                for group in &groups {
                    if !group.writes.is_empty() {unsafe {get_internal_gl()}.flush();}
                    let local:Vec<u8>=if mode==9 {group.writes.iter().flat_map(|w|w.rgba.iter().copied()).collect()} else {Vec::new()};
                    let pbo=if mode==9 && !local.is_empty() {let pbo=make_pbo(&local);unpack_buffers.push(pbo);pbo} else {global};
                    let mut local_offset=0;
                    for write in &group.writes {
                        upload_pbo(atlas,write,pbo,if mode==8 {global_offset} else {local_offset});
                        global_offset+=write.rgba.len();local_offset+=write.rgba.len();
                    }
                    draw_group(group,atlas);
                }
            } else if mode==2 {
                for group in &groups {for write in &group.writes {upload(atlas,write);}}
                for group in &groups {draw_group(group,atlas);}
            } else {
                // mode 0: interleaved upload + draw, original order
            }
            unsafe {get_internal_gl()}.flush();
            let submit = start.elapsed().as_secs_f64()*1000.;
            max_texture_bytes = max_texture_bytes.max((retired.len()+1)*DIM as usize*DIM as usize*4);
            unsafe {miniquad::gl::glFinish();}
            let completed = start.elapsed().as_secs_f64()*1000.;
            assert_eq!(unsafe {miniquad::gl::glGetError()},0);
            if trial>=4 {samples.push((submit,completed));}
            if trial==15 {capture(mode);}
```

计时统计（`accepted-input-lib.rs:342-345`）：

```rust
        let mut submit:Vec<_> = samples.iter().map(|s|s.0).collect(); let mut completed:Vec<_> = samples.iter().map(|s|s.1).collect();
        submit.sort_by(f64::total_cmp);completed.sort_by(f64::total_cmp);
        report(&format!("mode={mode} samples={} submit_p50_ms={:.4} submit_max_ms={:.4} completed_p50_ms={:.4} completed_max_ms={:.4} simultaneously_owned_texture_bytes={max_texture_bytes}",
            submit.len(),submit[6],submit[11],completed[6],completed[11]));
```

### 5. 开关、默认值、构建特性

| 项 | 状态 |
|---|---|
| 生产开关 | 无；这是独立诊断载体，不是 app feature |
| 默认模式 | `mode=0` 原路径；PBO 8/9 只能手动选择 |
| 构建 | 独立 `glyph_stream_probe` 载体 SO，不是主项目 feature |
| 运行时要求 | `GL_VERSION` 以 `OpenGL ES 3.` 开头；否则拒绝 PBO 诊断 |
| 纹理/格式 | 2048×2048 RGBA8、Linear、Clamp、默认 shader |
| 生产默认 | false；没有把 PBO 接进普通字体路径 |

后续另有一条独立的 `font_upload_pbo` 研究候选（每 painter 4×1 MiB PBO，固定预算，默认 `requested=false`），与本条不是同一 cohort，不能把它的数据混进本条的 130.252/18.643。本文只在“后续证据”里单独提到它。

### 6. 完整测量数据（表格）

环境与 fixture：

| 项 | 值 |
|---|---|
| 设备 | Mali-G52 MC2 |
| 画面 | 1920×1200 |
| 上下文 | Android OpenGL ES 3.x |
| 图集 | 2048×2048 RGBA8，16 MiB |
| 文本组 | 22 |
| 上传矩形 | 99 |
| 每 mode trial | 16（前 4 丢弃，后 12 采样） |
| 同步 | 每 trial `glFinish` |
| SO SHA | `0984a90c115151c670927b8f738054768cbfc8bc1d5f6585b684838508d60021` |
| lib.rs SHA | `11032576bcae16b1edf3ad771d60a48cdd28d549501a0ddae9427ae454da072b` |
| host 耗时 | 13.13 s |

逐 mode 计时：

| mode | 策略 | submit p50 ms | submit max ms | completed p50 ms | completed max ms | 同时拥有纹理 bytes |
|---|---|---:|---:|---:|---:|---:|
| 0 | 原路径，upload/draw 交错 | 130.2520 | 146.9936 | 132.2791 | 148.8902 | 16,777,216 |
| 2 | 先全部 upload，再全部 draw，无 PBO | 7.6788 | 8.6299 | 10.1169 | 11.0957 | 16,777,216 |
| 8 | 一个 packed PBO 装所有文本 | 18.6434 | 45.2313 | 136.7403 | 146.6649 | 16,777,216 |
| 9 | 每个文本一个 PBO | 27.7138 | 45.5918 | 135.2319 | 146.6485 | 16,777,216 |

RGBA 对照（`pixels.json`，reference mode 0）：

| 比较 | 尺寸相同 | 逐通道相同 | 不同通道数 | RGBA SHA |
|---|---|---|---|---|
| mode 2 vs 0 | true | true | 0 | `c9eeb33b9746877026ed234e4f02bebceafffa1ae9c62e3e7c49ca927f47e6a7` |
| mode 8 vs 0 | true | true | 0 | 同上 |
| mode 9 vs 0 | true | true | 0 | 同上 |

`results.txt` 还记录了 shader precision：vertex/fragment low float range `[15,15]`、precision bits `10`、GL error `0x0`；renderer `Mali-G52 MC2`；`native=1920x1200`、`atlas=2048`、`groups=22`、`uploads=99`、`backbuffer_samples=0`。这是单 fixture/单 GPU，不是所有字体/材质。

### 7. 质量与回归结果

通过的部分：

- 3 项严格 RGBA 对照（mode 2/8/9 对 mode 0）全部相同，尺寸相同、不同通道数为 0；PBO 8/9 保留了原矩形、原字节、原纹理名和原绘制顺序。
- 每个 mode 都使用相同的初始图集、相同的 22 组、相同的 99 次上传和 22 次 draw；没有通过合并 draw 或跳过矩形来制造 submit 下降。
- 每个 PBO 路径每步检查 GL error；`glFinish` 后 GL error 为 0；trial 结束时删除 PBO/纹理，没有资源泄漏报告。
- 像素对照只覆盖默认 shader、一个 2048 图集、一个 fixture；但至少证明“PBO 路径没有改变最终画面”。

没有证明的部分：

- 不是完整游戏帧，不是 FPS；没有 swap/compositor、没有多帧时序、没有 gameplay 输入/判定/音轨。
- 只覆盖默认 shader、Linear/Clamp、RGBA8；不覆盖自定义材质、MSAA、其他字体/图集尺寸/驱动。
- `glFinish` 是同步诊断，会改变驱动调度；completed 时间不能当“生产帧时间”，也不能当纯 GPU busy。
- 没有证明 PBO 在所有设备上都无收益；但当前设备/当前 fixture 已经足够否定“PBO 能降低整帧完成时间”的强假设。

### 8. 为什么判定为无/负（逐项对比）

第一，submit 的下降是真实的：mode 0 `130.252 ms`，mode 8 `18.643 ms`（约 `-85.7%`），mode 9 `27.714 ms`（约 `-78.7%`）。如果只看“`glTexSubImage2D` 返回时间”，PBO 确实把客户端提交提前了。这也是最初看起来“有希望”的原因。

第二，completed 没有下降：mode 0 `132.279 ms`，mode 8 `136.740 ms`（约 `+3.4%`），mode 9 `135.232 ms`（约 `+2.2%`）。`glFinish` 等到 GPU 真正完成，PBO 只是把 CPU 侧指针复制/排队提前，纹理上传和后续 draw 的读写依赖仍在；所以整帧完成时间没有改善，8 甚至更差。

第三，mode 2 给出了更强的反证：不做 PBO，只把“所有 upload”放到“所有 draw”之前，completed 就降到 `10.117 ms`，submit `7.679 ms`。这说明当前 fixture 的主要成本不是“客户端指针生命周期”，而是“上传与绘制交错”导致的纹理读写依赖/同步。PBO 8/9 仍然按组交错 upload/draw（每组 draw 前 flush），所以没有解决这个依赖；它只把每次 upload 的源指针换成 PBO offset。

第四，PBO 8/9 的 submit max 仍然很高：`45.231/45.592 ms`，远高于 p50 `18.6/27.7 ms`。这说明 PBO 路径仍有长尾，可能是 PBO 分配/`glBufferData`/驱动同步/首个矩形；mode 2 的 max 只有 `8.630 ms`，更稳定。PBO 并没有把尾延迟压下去。

第五，3 项 RGBA 相同只证明“这个 fixture 的最终画面没变”，不证明“整帧语义没变”。PBO 8/9 保持了矩形/字节/纹理名/绘制顺序，这是质量通过的基础；但它没有改变真正的同步点，所以质量通过不能推出性能收益。

第六，后续独立的 `font_upload_pbo`（Root 1 MiB 固定预算）也没有建立收益：普通 Main ABBA 组均值 `11.366646 → 12.553903 ms`（描述 `+10.4451%`），但 off 前后漂移 `1.945775 ms` 大于组差 `1.187257 ms`；PBO 自身两 ON 的最大 publication 仍有 `50/56 ms`，上传峰未消除，`>30ms` 帧为 `39/40`。这条 cohort 与本条 PBO probe 分开，但方向一致：PBO 不是整帧收益的答案。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：lib.rs `11032576bcae16b1edf3ad771d60a48cdd28d549501a0ddae9427ae454da072b`；host 13.13 s 完成 |
| 默认 | 关闭；没有生产接入；研究 mode 仅用于诊断 |
| 普通路径 | 未接入；没有 app feature/Config 开关；没有默认启用 |
| 主线是否已实施 | 否（main） |
| 研究后端 | 诊断载体 SO `0984a90c115151c670927b8f738054768cbfc8bc1d5f6585b684838508d60021` |
| 实验装置 | 设备 Mali-G52 MC2 / 1920×1200；22 组 / 99 上传 / 2048 图集；4 个 mode 各 16 trial |
| 质量/契约 | 3 项严格 RGBA 相同；submit/completed 原始数据保存在 `results.txt`/`logcat.txt` |
| 后续 | `font_upload_pbo` 是独立研究候选，默认 `requested=false`，也未推广；本条不把它当同一实现 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `make_pbo`/`upload_pbo` 的状态保存/恢复和 GL error 检查可以作为 PBO 上传的参考实现。
- `submit`/`completed` 分开计时的方法必须保留；任何 PBO 实验都不能只报 submit。
- mode 2 的“先全部 upload 再全部 draw”是一个明确的候选方向：它把 completed 从 132 ms 降到 10 ms，说明纹理读写依赖是主因；但这需要证明绘制顺序/透明顺序/材质状态在“先写后画”下仍然等价。

如果还要继续，必须先满足这些条件：

1. 以 completed 时间为准，不以 submit 时间为准；每个候选都要有 `glFinish` 后的整帧时间。
2. 解决上传/绘制交错，而不是只换客户端指针；可以按“同一纹理的写批次”重排，但必须保持原绘制顺序、透明顺序和材质状态。
3. 如果使用 PBO，必须证明 PBO 的分配/`glBufferData`/offset 对齐/首矩形成本没有吃掉收益；mode 8/9 的 max 45 ms 需要解释。
4. 覆盖自定义 shader、MSAA、其他字体/图集尺寸/GPU；当前默认 shader/单 fixture 不够。
5. 需要真实 Main 的整帧 ABBA，而不是同步微基准；如果整帧仍无收益，停止 PBO 方向。
6. 与 N-15 的 immutable storage 分开评估；两者都可能改变驱动同步，但不能把“质量失败”或“submit 下降”混成性能结论。

### 11. 来源与可追溯性

原始记录：

- `本地证据/glyph-stream-diagnostic-pbo/results.txt`：4 个 mode 的 submit/completed p50/max。
- `本地证据/glyph-stream-diagnostic-pbo/logcat.txt`：设备 logcat 原始行，包含 `GLYPH_STREAM` 行（含 mode、submit_p50_ms、completed_p50_ms 等字段）。
- `本地证据/glyph-stream-diagnostic-pbo/pixels.json`：3 项 RGBA 对照、reference SHA `c9eeb33b9746877026ed234e4f02bebceafffa1ae9c62e3e7c49ca927f47e6a7`。
- `本地证据/glyph-stream-diagnostic-pbo/host.json`：SO/lib.rs/runner SHA、host 13.13 s、`scope` 明确“synchronous glFinish diagnostic, not actual FPS”。
- `本地证据/glyph-stream-diagnostic-pbo/accepted-input-lib.rs`：`make_pbo`、`upload_pbo`、mode 8/9 循环、计时和 capture。
- `本地证据/glyph-stream-diagnostic-pbo/accepted-input-libglyph_stream_probe.so`：真机 SO。
- `本地证据/branch-cleanup-20261006/previous-AGENTS.md:39` 与 `previous-game-performance-target.md:37`：历史结论“submit 130.252→18.643/27.714，completed 132.279→136.740/135.232，无整帧收益，不接入”。

对应 `本地证据/branch-cleanup-20261006/previous-AGENTS.md` 段落（原文）：

> PBO探针已完成/3项严格RGBA全同：原序上传submit p50 130.252→18.643/27.714ms，但同步completed 132.279→136.740/135.232ms，无整帧收益，不接入。CPU模型分帧候选已实现并保持默认false；共享原max_time/判定线构造/跨线处理/Chart构造，约2ms后或完成边界让local loading future挂起一次，资源仍主线程，异步边界外才记录span。2项挂起/恢复/取消契约和44项host已通过，Android构建、真实加载ABBA及质量尚待完成；不能称已消除尖峰。主项目参考实现不push、vendor旧证据不覆盖，完整目标继续。

对应 `本地证据/low-quality-normal/README.md` 段落（原文，字形上传诊断背景）：

> 同一字体计时后端 `028b6effcff70fecb5bbda53285c88d7ca5cc84a3055fca8030b85b6e56426aa` 的两次独立原路径运行，各 1200 采样帧、120 预热帧，低画质但无背景近似、无 GPU timer。字体计时后端和源码已经以 `font-telemetry-*` 前缀归档，避免后续 lab.so 覆盖后混淆证据。

> 采样期间总上传分别约 1.63MB、1.66MB。预上传 flush 墙钟均值仅 0.021 / 0.018ms，process_queued 包含上传回调，不能把它当纯 CPU 字形生成。数据证明长帧位于更新调用内，尚不单凭相关性认定每个 GPU/driver 都有相同读写等待。

> 新隔离原型 `font_atlas_rename` 保留原 GlyphBrush、栅格、atlas 排布、UV 和文字几何，CPU 镜像记录上传矩形，每次有变化后一次发布全新的 GL 图集。原路径和处理路径在同一个 lab 后端中切换。旧绘制先提交，之后删除旧 GL 对象，由 GL 管理在途引用；不主动等待 GPU。

> 这是极端空间/带宽换时间验证：2048² RGBA 图集每份 16MiB，每次变更整图上传，加上 CPU 镜像和可能在途的 GPU 版本。三个 TextPainter 各有镜像，本原型仅镜像就约 48MiB，其中未使用的 BOLD 镜像也预分配，是待收紧的成本。不能仅因公式和像素输入不变就称无损。

> 图集替换后端 `18f3585570ae2328d5dde98b8b63d3975d6e44cff7caa54d01bb02c5e0c5b693` 的四组画面对照均为 2560×1440 RGBA **逐像素相同**：sampleCount=1/2 各测试固定 time=70、dt=0，以及 start=60、dt=1/60、总 240 帧后的动态数字画面。冷固定时刻采样只有 4 帧，不能拿它的 FPS 作性能结论；动态固定时钟也只是画面对照。四个画面支持无损候选，不代替完整谱面/其他字体与设备的验证。

> 同一后端真实时钟 ABBA，均无背景融合/降采样、无 timer、sampleCount=1、120 预热 + 1200 采样帧：原路径约57.530/57.518fps，替换约59.470/59.683fps；HUD 均值从约2.415ms降到1.343–1.374ms。代价是每轮替换上传2,080,374,784 bytes（约1.94GiB/约20s），尚不适合直接进入生产；不把原型展示的收益与待实现的低上传方案混为一谈。

---

## N-17 背景融合 / 低频纹理缓存

### 1. 一句话结论与判定

把背景亮度乘法折叠到纹理颜色（fused），并在加载期把模糊背景降到低频表示（compact），在固定 chart-time 70 的画面上相对原路径最大 RGB 差只有 `1/255`；但真实 1200 帧六轮对照没有稳定 gameplay 收益，fused/compact 与 control 的差异都在 0.36 FPS 内且方向不一致，呈现 p99 的 control 自身波动更大。判定：**无优化（无稳定 gameplay 收益）**，保留为轻量有损研究原型，不作为主要优化。

纹理字节从 `8,847,360` 降到 `552,960`（约 `1/16`）是事实，但它没有转化为每帧成本下降；背景阶段本来只占约 `0.35–0.40 ms`，而 HUD 约 `3.12–3.34 ms` 且多次 `20–50 ms`。把背景砍掉也动不了主要长帧。

### 2. 问题与初衷

原背景流程是：背景纹理，叠 `0.3` 黑色混合，再叠 `alpha*backgroundDim` 黑色混合。对于完整 viewport、低画质、无额外离屏效果的场景，这两层黑色混合可以折叠成“把亮度乘法直接乘到纹理颜色”，从而少一次全屏混合/一次 draw。模糊背景则在加载阶段做低频表示：先降采样，再重建到原尺寸检验误差；不满足预算就自动增大分辨率。目标是减少背景的填充/混合带宽和纹理采样成本，同时把误差限制在不影响读谱的范围内。

这是一个“轻量有损”候选：8 位颜色和混合舍入可能不同；它不是无损。定位是低画质、背景等不影响读谱的低频内容，不能在高画质/激进开关下自动启用，也不能影响判定、音轨、输入或透明顺序。

### 3. 实现思路与具体机制

1. fused 只在 `res.config.sample_count == 1`、`res.no_effect` 为真、`res.camera.viewport == Some(ui.viewport)`（完整 viewport）时启用；其他情况保留原路径。它不改变背景纹理、不改变相机、不改变 chart 层。
2. fused 的 tint 计算：`tint = 0.7 * (1 - (res.alpha * res.info.background_dim).clamp(0, 1))`；用 `draw_texture_ex` 把背景纹理以 `Color::new(tint,tint,tint,1.)` 画到 `rect`，`source` 按 `ScaleType::CropCenter` 映射，`dest_size` 为 viewport 大小；随后跳过原来的 `draw_rectangle` 黑色遮罩。
3. compact 在加载期对模糊背景做低频表示：先求 `width/height` 的最大公约数，得到单位宽高 `unit_w/unit_h`；`units = max_side.div_ceil(max(unit_w,unit_h)).max(1).min(common)`；从 `w=unit_w*units, h=unit_h*units` 开始，用 `image::imageops::resize(&source, w, h, image::imageops::FilterType::Triangle)` 降采样，再用 `image::imageops::resize(&reduced, width, height, image::imageops::FilterType::Triangle)` 重建到原尺寸；逐像素比较原图与重建图的 RGB 差，记录 `maximum` 和平方误差。
4. 误差门：`maximum as f64 * 0.7 <= 2.0`，或者已经到 `units == common`（无法再增大）；满足才返回 `reduced` 与报告，否则 `units = (units*2).min(common)` 继续。`0.7` 是最亮背景乘数；`2.0` 是纹理空间预算，不是屏幕空间证明。报告包含 `requested_max_side/original/reduced/original_bytes/reduced_bytes/max_texture_error/rmse_texture/brightest_dimmed_error_bound/aspect_preserved`。
5. 宽高比精确保持：`unit_w/unit_h` 来自 gcd，`units` 同时乘到两边；不会因为四舍五入改变比例。`aspect_preserved=true` 是报告字段，实际由整数倍保证。
6. 这个谱面原始背景 `2048×1080 RGBA8 / 8,847,360 bytes`；预算选择 `512×270 / 552,960 bytes`，体积约 `1/16`；`max_texture_error=2`，`brightest_dimmed_error_bound=1.4`，`rmse_texture=0.388`。
7. 质量对照：固定 `chart-time=70`、`dt=0`、`sampleCount=1`、2560×1440 有效截图，分别跑 control/fused/compact；比较整屏 RGBA，记录最大 RGB 差。只验证这一个时刻，不能外推所有图像/动画/viewport。
8. 性能对照：同一 `lab-v1.so`，顺序 `control/fused/compact/compact/fused/control`，每条 1200 帧 realtime，独立 SurfaceFlinger 约 10 s；所有臂都保持粒子/效果/分辨率/输入逻辑，关闭 GPU timer；应用墙钟包含 update+render+flush，不是纯 GPU。
9. 结果：fused 和 compact 都没有稳定超过 control；control 自身两次的呈现 p99 分别为 `50.20/33.48 ms`，说明呈现统计的方差很大；不能只挑 compact 第一次 `33.48 ms` 当作改善。

### 4. 文件、符号与完整核心代码（禁止省略号）

文件与符号：

| 文件 | 符号 | 作用 |
|---|---|---|
| `本地证据/low-quality-normal/branch-lab-sources/phira/src/render_research.rs` | `compact_background` | 加载期低频表示与误差门 |
| `本地证据/low-quality-normal/branch-lab-sources/prpr/src/scene/game.rs` | `fuse_background`、`LAB_BACKGROUND`、`LAB_FUSED_FRAMES` | 运行期背景融合 |
| `本地证据/low-quality-normal/performance-plan.json` | `fused_background`、`background_max_side` | 研究作业开关 |
| `本地证据/low-quality-normal/quality-plan.json` | 固定时刻质量对照 | control/fused/compact |
| `本地证据/low-quality-normal/performance-runs/` | 六轮原始 CSV/呈现 | 性能数据 |

`compact_background` 的完整实现：

```rust
// branch-lab-sources/phira/src/render_research.rs:77-104
fn compact_background(width: u32, height: u32, bytes: &[u8], max_side: u32) -> Result<(u32, u32, Vec<u8>, Value)> {
    let source = ::image::RgbaImage::from_raw(width, height, bytes.to_vec()).context("background buffer shape")?;
    let common = gcd(width, height);
    let unit_w = width / common;
    let unit_h = height / common;
    let mut units = max_side.div_ceil(unit_w.max(unit_h)).max(1).min(common);
    loop {
        let w = unit_w * units;
        let h = unit_h * units;
        let reduced = ::image::imageops::resize(&source, w, h, ::image::imageops::FilterType::Triangle);
        let decoded = ::image::imageops::resize(&reduced, width, height, ::image::imageops::FilterType::Triangle);
        let mut maximum = 0u8;
        let mut squared = 0f64;
        for (a, b) in source.pixels().zip(decoded.pixels()) {
            for c in 0..3 {
                let e = a[c].abs_diff(b[c]);
                maximum = maximum.max(e);
                squared += (e as f64).powi(2);
            }
        }
        // Bound error at the brightest gameplay background, including dim fade-in.
        // This is a texture-space loading diagnostic, not a screen-space proof.
        if maximum as f64 * 0.7 <= 2. || units == common {
            let report = json!({"requested_max_side":max_side,"original":[width,height],"reduced":[w,h],"original_bytes":bytes.len(),"reduced_bytes":reduced.as_raw().len(),"max_texture_error":maximum,"rmse_texture":(squared/(width as f64*height as f64*3.)).sqrt(),"brightest_dimmed_error_bound":maximum as f64*0.7,"aspect_preserved":true,"scope":"CPU reconstruction diagnostic. Screen capture required separately."});
            return Ok((w, h, reduced.into_raw(), report));
        }
        units = (units * 2).min(common);
    }
}
```

`fuse_background` 的完整运行期块：

```rust
// branch-lab-sources/prpr/src/scene/game.rs:1735-1789
        #[cfg(feature = "render_research")]
        let fuse_background =
            LAB_BACKGROUND.with(|value| value.get()) && res.config.sample_count == 1 && res.no_effect && res.camera.viewport == Some(ui.viewport);
        #[cfg(not(feature = "render_research"))]
        let fuse_background = false;
        #[cfg(feature = "render_research")]
        let stage_begin = std::time::Instant::now();
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
        push_camera_state();
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            viewport: if res.chart_target.is_some() { None } else { Some(ui.viewport) },
            render_target: chart_onto,
            ..Default::default()
        });
        clear_background(BLACK);
        if fuse_background {
            let top = 1. / screen_aspect();
            let rect = Rect::new(-1., -top, 2., top * 2.);
            let source = crate::ext::source_of_image(&res.background, rect, ScaleType::CropCenter);
            let w = res.background.width();
            let h = res.background.height();
            let tint = 0.7 * (1. - (res.alpha * res.info.background_dim).clamp(0., 1.));
            draw_texture_ex(
                *res.background,
                rect.x,
                rect.y,
                Color::new(tint, tint, tint, 1.),
                DrawTextureParams {
                    source: source.map(|it| Rect::new(it.x * w, it.y * h, it.w * w, it.h * h)),
                    dest_size: Some(rect.size()),
                    ..Default::default()
                },
            );
            #[cfg(feature = "render_research")]
            LAB_FUSED_FRAMES.with(|value| value.set(value.get() + 1));
        } else {
            draw_background(*res.background);
        }

        pop_camera_state();
```

跳过黑色遮罩的部分在同一个 `render` 里：

```rust
        let h = 1. / res.aspect_ratio;
        if !fuse_background {
            draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));
        }
```

### 5. 开关、默认值、构建特性

| 开关/特性 | 默认 | 作用 |
|---|---|---|
| `render_research` feature | 生产关闭 | 研究后端才编译 `fused_background` |
| `fused_background` 请求键 | false | 打开亮度折叠 |
| `background_max_side` | 0 | 0 表示不做 compact；非 0 才在加载期降采样 |
| `LAB_BACKGROUND` 线程局部 | false | 未配置时不融合 |
| 生产 Config/UI | 无 | 没有把背景融合/Compact 接进普通设置 |
| 画质限制 | 低画质/`sample_count=1`/无效果/完整 viewport | 不满足即回原路径 |

这是轻量有损候选，不是默认优化；研究作业关闭 GPU timer、保持粒子/效果/分辨率/输入逻辑。生产 `aggressive` 的历史裁剪与这个候选无关，不能因为打开 aggressive 就自动启用背景近似。

### 6. 完整测量数据（表格）

环境：Mali-T880 / 2560×1440 / 低画质 / `sampleCount=1` / 粒子/效果保留 / 同一 `lab-v1.so` SHA `420915963cb0b0b30aded9e7ac9ea0c9d1c3f8dd84d26658f37abcea6ab66f4c`；谱面 SHA `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c`。

质量（固定 `chart-time=70`、`dt=0`，2560×1440）：

| 处理 | 相对原路径最大 RGB 差 | 不同像素 | 说明 |
|---|---:|---:|---|
| fused | 1/255 | 2,471,930 | 亮度折叠的 8 位舍入 |
| compact | 1/255 | 2,473,045 | fused + 低频表示 |
| 原路径 | 0 | 0 | reference |

compact 报告：

| 项 | 值 |
|---|---:|
| 原始 | 2048×1080 RGBA8 / 8,847,360 bytes |
| 降采样 | 512×270 / 552,960 bytes |
| 体积比 | 约 1/16 |
| `max_texture_error` | 2 |
| `brightest_dimmed_error_bound` | 1.4 |
| `rmse_texture` | 0.388 |
| `aspect_preserved` | true |
| `requested_max_side` | 128（实际因误差门加倍到 512×270） |

性能六轮（1200 帧 realtime；呈现约 10 s；顺序 control/fused/compact/compact/fused/control）：

| 次序 | 处理 | 应用 FPS | 应用 p99 ms | 呈现 FPS | 呈现 p99 ms | background 均值 ms | chart 均值 ms | particle 均值 ms | HUD 均值 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | control | 57.1989 | 45.8092 | 55.6916 | 50.1993 | 0.398 | 0.596 | 0.813 | 3.227 |
| 1 | fused | 57.5565 | 46.0005 | 56.7708 | 50.1930 | 0.358 | 0.607 | 0.844 | 3.233 |
| 2 | compact | 57.4592 | 45.4527 | 56.9623 | 33.4806 | 0.355 | 0.585 | 0.861 | 3.164 |
| 3 | compact | 57.3744 | 45.1572 | 56.4477 | 50.2004 | 0.363 | 0.609 | 0.902 | 3.340 |
| 4 | fused | 57.2388 | 48.7090 | 56.9290 | 50.1491 | 0.359 | 0.574 | 0.853 | 3.152 |
| 5 | control | 57.3767 | 45.4021 | 56.9273 | 33.4796 | 0.394 | 0.590 | 0.809 | 3.118 |

阶段均值是 `65–75 s` 窗口的墙钟，不是纯 GPU；HUD 的均值只有 3.1–3.3 ms，但该窗口内单帧 HUD 仍多次到 `46–54 ms`。`background` 只有 0.35–0.40 ms，即使完全消除，理论整帧节省也只有约 2%。

### 7. 质量与回归结果

通过的部分：

- 固定时刻 70 s 的 2560×1440 截图，fused 与 compact 相对原路径最大 RGB 差都是 `1/255`；没有出现结构性错位/颜色跳变/透明顺序错误。
- compact 的宽高比精确保持；报告记录了原始/降采样尺寸、字节、最大误差、RMSE、最亮乘数误差界。
- 六轮性能都使用同一 SO、同一谱面、同一配置；粒子/效果/分辨率/输入逻辑保留；设备温度前后约 31.0→32.0°C（host.json before/after）（电池记录）。
- 纹理字节减少 16 倍是真实的 CPU/纹理表示事实；它不是伪造的“零成本”。

没有证明的部分：

- 只验证一个固定时刻、一个谱面、一个 viewport、一个设备；不能外推所有图像/动画/viewport/GPU。
- 最大 RGB 差 1/255 不等于“无损”；8 位混合舍入可能在不同驱动/不同帧累积，必须逐帧/全曲验证。
- 没有纯 GPU 时间；应用墙钟包含驱动/调度/等待，不能把 background 阶段的差归因于 GPU fill/blend。
- 没有证明 compact 的降采样在所有背景上都满足 `max_texture_error*0.7<=2`；报告只覆盖这个背景。
- 没有证明 fused/compact 对 HUD 长帧、glyph 上传、粒子或 chart 尖峰有帮助；它们只改背景。

### 8. 为什么判定为无/负（逐项对比）

第一，背景阶段太小。65–75 s 窗口里 background 均值 `0.35–0.40 ms`，整帧约 `17 ms`；即使把 background 从 0.40 降到 0，理论上也只省约 `2.3%`。实际 fused/compact 只降到 `0.355–0.363`，节省约 `0.04 ms`，在 control 自身波动（0.394–0.398）面前不可测。

第二，FPS 没有稳定方向。control 两轮 `57.1989/57.3767`，fused `57.5565/57.2388`，compact `57.4592/57.3744`。fused 第一轮比 control 上限高约 `0.18 FPS`，第二轮又落到 control 区间内；compact 第一轮高约 `0.08 FPS`，第二轮几乎等于 control 上限。差异都在 `0.36 FPS` 内，且两轮方向不一致，不能形成稳定分离。不能挑 fused 第一轮 `57.5565` 说“提升”，也不能挑 compact `57.4592` 说“提升”。

第三，呈现 p99 的方差比处理差大。control 两轮 `50.20/33.48 ms`，fused `50.19/50.15`，compact `33.48/50.20`；同一处理两次都能差 16.7 ms。只选 compact 第一次 `33.48` 会把“control 第二次也 33.48”这个事实掩盖掉。呈现 FPS 也都在 `55.69–56.96` 之间，没有稳定分离。

第四，纹理字节减少 16 倍不等于每帧成本减少 16 倍。背景纹理的采样/fill 成本与分辨率、过滤、混合、GPU tile 调度有关；降到 512×270 后仍然要重建/采样到全屏，采样次数和混合次数没有按字节比例下降。compact 的加载期 CPU 重建成本也增加了；它不属于 gameplay 帧，但说明“1/16 字节”不是免费。

第五，HUD 才是这个窗口的主要成本。HUD 均值 `3.12–3.34 ms`，单帧多次 `20–50 ms`；background 只有 `0.35–0.40 ms`。把背景砍掉不会改变 HUD 的字形更新/纹理同步长帧，也不会改变 chart/粒子/驱动等待。这个候选的 ROI 方向就不对。

第六，质量只在一个固定时刻通过。fused 改变了 8 位混合的舍入顺序，compact 又改变了纹理采样；2.47M 个像素有 1/255 差异，说明它不是逐位无损。对读谱影响可能很小，但要做全曲/所有背景/所有 viewport 的逐帧对照，才能升级为“轻量有损可用”。当前证据不够。

### 9. 主线实施状态与参考实现状态

| 项 | 状态 |
|---|---|
| 参考实现 | 已落地（未合入 main）：原型在 `branch-lab-sources` 快照中；`lab-v1.so` SHA `420915963cb0b0b30aded9e7ac9ea0c9d1c3f8dd84d26658f37abcea6ab66f4c`；六轮性能完整结束 |
| 默认 | 关闭；研究请求 `fused_background` 默认 false；`background_max_side=0` 默认不做 compact |
| 普通路径 | 未接入；没有生产 Config/UI 开关 |
| 主线是否已实施 | 否（main） |
| 研究后端 | `lab-v1.so`；见第 11 节冻结源与证据 |
| 质量/契约 | 只做固定时刻 70 s；没有全曲/所有 viewport/所有 GPU 验证 |
| 结论 | 未显示稳定 gameplay 性能收益；不作为主要优化 |
| 后续 | 没有默认推广；设备/构建作业结束；原谱面 ZIP、原 Config/font 保留 |

### 10. 可挽救的部分与后续条件

可保留的部分：

- `compact_background` 的“降采样-重建-误差门-自动增大”流程可以复用到其他低频纹理；它的宽高比保持和误差报告是可复用的。
- fused 的条件门（`sample_count=1`、`no_effect`、完整 viewport）是一个安全边界；不满足就回原路径。
- 固定时刻/整屏 RGBA 对照方法可以继续用于背景候选的质量验证。

如果还要继续，必须先满足这些条件：

1. 先证明目标设备/分辨率下 background 阶段是显著瓶颈；当前 0.35–0.40 ms/17 ms 不支持。
2. 做全曲、所有背景、所有 viewport、MSAA/无 MSAA 的逐帧对照；不能只看固定 70 s。
3. 测量纯 GPU/带宽（如果可用）或至少把 background 阶段与 HUD/glyph 分开；不能把应用墙钟当 GPU。
4. 如果目标是减少纹理内存，compact 的 1/16 是有意义的；如果目标是 gameplay 帧率，当前证据不支持。
5. 需要证明 8 位混合舍入在所有驱动上都不会累积成可见差异；否则只能留在低画质研究。
6. 不要把它当主要优化；优先处理 HUD/glyph 更新和纹理同步长帧。

### 11. 来源与可追溯性

原始记录：

- `本地证据/low-quality-normal/README.md` 的“背景候选：已验证表示和误差，未验证稳定帧率收益”整节。
- `本地证据/low-quality-normal/performance-plan.json`：`order=["control","fused","compact","compact","fused","control"]`、`fused_background`、`background_max_side`。
- `本地证据/low-quality-normal/performance-runs/performance-control-20261006T151425-244368d4/`、`performance-fused-20261006T151457-90eb3502/`、`performance-compact-20261006T151529-515dd23a/`、`performance-compact-20261006T151602-a11fab35/`、`performance-fused-20261006T151637-4eb56cb8/`、`performance-control-20261006T151709-56e07771/`：六轮原始 CSV、`analysis.json`、`presentation/summary.json`。
- `本地证据/low-quality-normal/quality-runs/quality-control-20261006T151226-15a28ee1/`、`quality-fused-20261006T151236-cd077e21/`、`quality-compact-20261006T151245-bfebc31c/`：固定 70 s 截图、`background-source.png`、`metadata.json`。
- 质量 diff 由三张 2560×1440 `screenshot.png` 逐像素重算：fused 2,471,930 个像素、compact 2,473,045 个像素有 RGB 差，最大 1；这是本文对原始 PNG 的复核，不替代原报告。
- SO SHA `420915963cb0b0b30aded9e7ac9ea0c9d1c3f8dd84d26658f37abcea6ab66f4c`；谱面 SHA `f1c246159164cdad74e6cbef73db394bd83907dd82ea087010c23b399298e03c`；compact 报告原始 2048×1080 / 8,847,360 bytes → 512×270 / 552,960 bytes。
- 设备 Mali-T880 / 2560×1440 / Android 8 / arm64-v8a；电池温度前后 31.0→32.0°C（host.json before/after）。

对应 `本地证据/low-quality-normal/README.md` 段落（原文）：

> 原流程为背景纹理 + 0.3 黑色混合 + `alpha*backgroundDim` 黑色混合。完整 viewport 且低画质无额外离屏效果时，可以把亮度乘法折叠到纹理颜色；viewport 不匹配保留原路径。8 位颜色与混合舍入可能不同，故归轻量有损。

> 模糊背景在加载阶段做低频表示：从小尺寸开始，重建到原尺寸检验误差，不满足预算则自动增大；精确保留宽高比。此谱面原始 2048×1080 RGBA8 / 8,847,360 bytes，预算选择 512×270 / 552,960 bytes，体积减为 1/16。最亮背景乘数 0.7 时 CPU 纹理重建最大误差 1.4/255；这不是屏幕证明。

> 固定 chart-time=70 的 2560×1440 有效截图：融合和融合+低频表示相对原路径最大 RGB 差异都为 1/255。只验证这一个时刻，不能外推所有图像/动画/viewport。

> 性能顺序 control / fused / compact / compact / fused / control，同一 `lab-v1.so`（SHA-256 `420915963cb0b0b30aded9e7ac9ea0c9d1c3f8dd84d26658f37abcea6ab66f4c`），不与旧 backend 的 pilot 混池。

> | 次序 | 处理 | 应用 FPS（完整 1200 帧） | 呈现 FPS（独立约 10s） | 呈现 p99 ms |
> |---|---|---:|---:|---:|
> | 0 | control | 57.20 | 55.69 | 50.20 |
> | 1 | fused | 57.56 | 56.77 | 50.19 |
> | 2 | compact | 57.46 | 56.96 | 33.48 |
> | 3 | compact | 57.37 | 56.45 | 50.20 |
> | 4 | fused | 57.24 | 56.93 | 50.15 |
> | 5 | control | 57.38 | 56.93 | 33.48 |

> 没有稳定收益；control 自身也出现较低 p99，不能只选 compact 第一次当作改善。纹理字节量减少 16 倍不代表每帧/GPU 成本减少 16 倍。

> 65–75s 的 chart 墙钟均值约 0.57–0.61ms，background 约 0.35–0.40ms，particle 约 0.81–0.90ms，HUD 约 3.12–3.34ms。HUD 多次 20–50ms；75–82s HUD 均值约 0.9ms。`TextPainter::submit` 在字形更新前 flush 并更新 atlas，需细分计时确认是否造成同步尖峰，不能直接把所有 HUD 长帧叫字体 CPU 热点。

> 可选 EXT_disjoint_timer_query 在实际上下文完成 extension/entry-point/counter-bit 检查，64 位计数，有正结果、无 disjoint/pending/skipped。括号包含 render+flush 的提交范围，**不等于独占 GPU busy 时间**，可能改变 tile 调度；约 26–27ms 结果不能加到 CPU 时长或据此推算 FPS。只作诊断，不作最终性能验收。

对应 `本地证据/AGENTS.md` 的画质分级段落（原文）：

> 用户要求明确画质分类：保持相同输出的资源准备、容量复用、求值缓存等无损优化可以用于所有画质；纹理/效果降采样、动画量化或近似仅用于低画质。理论等价但可能有浮点/混合舍入差异的方案先列待验证，不能直接称为无损。当前背景遮罩融合可能改变 8 位舍入，按低画质候选；GPU 参数化音符也须先验证精度、顺序与兼容性。

> 用户进一步要求有损分级：项目已有激进优化开关。新增策略按无损 / 轻量有损 / 激进有损区分。无损所有模式可用；轻量有损要求低画质；激进有损要求低画质与激进优化同时开启。高画质开启激进优化不能自动开启新增有损策略。轻量近似优先背景等不影响读谱的低频内容，有误差约束；更大空间降采样/时域近似单列激进研究，并验证读谱影响。现有 aggressive 的历史裁剪行为需单独核对，不擅自改成新的画质权限。

对应 `本地证据/low-quality-normal/README.md` 的进度句（原文）：

> 背景融合/低频纹理缓存已原型实测，未显示稳定 gameplay 性能收益；不作为主要优化。
