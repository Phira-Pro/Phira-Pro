# Phira-Pro 渲染库级优化审计

审计对象：macroquad 与 miniquad 的参考实现中已落地、但未进入 Phira-Pro `main` 固定生产库 rev 的 API/机制。

审计日期：2026-11-11。

约束：本文只描述库层实现与已有证据，不修改源码、不切换仓库状态、不提交。文中的结论以提交对象、文件内容和本地测量记录为依据；读者不需要检出任何仓库状态也能理解每项的接口、算法、开关和风险。

## 0. 先看结论

### 0.1 一句话结论

本文只有一个“主线已实施”口径：**Phira-Pro `main` 已合入并完成验收，且库级项已进入 `main` 所固定的生产库 rev。** 仅存在于 macroquad/miniquad 的 `master` 或参考实现，都不算主线已实施。按此口径，L-01..L-10 与 D-01 全部为“否”，即主线未实施（Phira-Pro `main` 的固定 rev 不含这些 API/改动）；X-01 的核心虽已以 squash 形式合入两个库的 `master`，但尚未进入 Phira-Pro `main` 所固定的生产组合，主线状态同样是“否”。

这批工作可以分成四类，不能混成一张“优化收益表”：

1. **潜在正优化，但当前无稳定收益证据**：固定纹理存储（L-01）、有界 PBO 上传（L-03）、流缓冲 discard（L-04）。三者都有明确机制，但 L-01 没有应用级性能数据且质量探针未闭合；L-03 的 ABBA 无稳定收益；L-04 只有冷帧局部改善、整窗平均回归。不能把“潜在正”写成已实现收益。
2. **只降低调用次数、当前无墙钟收益证据**：uniform 按位缓存（L-05）。packed uniform 调用下降约 67%–69%，但完整应用墙钟与 FPS 没有稳定收益。
3. **正确性或基础设施，不是性能优化**：缓冲扩容前的绑定缓存失效（L-02）、资源标识/生命周期（L-06）、图集读取依赖（L-08）、排队纹理依赖查询（L-09）、绘制状态查询（L-10）。它们为安全接入或后续优化提供条件，本身不应报成帧率收益。
4. **负收益或非优化**：常驻网格延迟提交（L-07）在真实普通 gameplay 中没有收益，完整应用墙钟反而略高；原生帧循环、提交诊断、有界采集（D-01）是诊断设施，不是优化。

X-01（共享 arena/连续 pass）的核心已经以 squash 形式合入两个库的 `master`，但尚未进入 Phira-Pro `main` 所固定的生产库 rev；本文把它放在排除项 X-01，用于区分“库 master 已合入”与“主线已实施”。

### 0.2 版本读法：参考实现是叠放链，不是独立补丁

两个仓库的参考实现都是**叠放链**，不是彼此独立的补丁：

- miniquad 侧从 `9466727` 开始，依次经过 `e63917d`、`406954d`、`b7e1d66`、`1dfaa93`、`eb0eda0`、`4b26d71`、`4d85ed3`、`d7dc3f4`、`bc1d9d4`，最后到 `19e97bd`。因此按整条链统计的 diff 会包含前面所有提交；`19e97bd` 本身的隔离改动只有 5 个文件。
- macroquad 侧的低画质链从 `139ac95` 开始，先有 `7068495`（常驻网格），再依次固定 miniquad 的 uniform、诊断、生命周期、图集、帧循环、discard、PBO、采集、immutable 等 rev，最后是 `de34611`。因此很多按整条链统计的 diff 包含 `7068495`、`51147e9` 等前序实现；这些末端提交的 `Cargo.toml` 改动只是**固定依赖**。
- 判断某个功能改了什么，必须看单个提交的隔离 diff，不能看整条链的总 diff。

本文的“实现侧”按**隔离提交**判定，不按仓库状态判定。凡是 macroquad 只改 `Cargo.toml` 的条目，都明确写成“仅 pin”，并给出 miniquad 的实现提交。

### 0.3 已确认的关键事实

| 事实 | 核对结果 |
|---|---|
| macroquad 侧 `4b46e86` | 只改 `Cargo.toml`（+2/-1）；实现是 miniquad `19e97bd` |
| miniquad 侧 `19e97bd` | `texture.rs` +510、`texture_immutable_tests.rs` +335、`native/gl.rs` +7；另有 `pixel_unpack.rs` 141 行重构 |
| macroquad 侧 `de34611` | 唯一 macroquad 代码改动：`src/quad_gl.rs` +5；依赖 miniquad `1dfaa93` 的 `clear_resource_bindings` |
| macroquad 侧 `ed1db13` | 只改 `Cargo.toml`（+2/-1）；实现是 miniquad `4d85ed3` + `d7dc3f4` |
| miniquad 侧 `d7dc3f4` | 隔离改动 `pixel_unpack.rs` +78/-1；前序 `4d85ed3` 是 484 行主体 |
| macroquad 侧 `cd0ab01` | 只改 `Cargo.toml`（+2/-1）；实现是 miniquad `4b26d71`（`graphics.rs` +123） |
| miniquad 侧 `9466727` | `graphics.rs` +79、`uniform_cache.rs` +91；macroquad 侧没有对应实现提交 |
| macroquad 侧 `c862fc8` | 只改 `Cargo.toml`（+1/-1）；miniquad `1dfaa93` 是 `clear_resource_bindings` 的 17 行实现，链上还有 `406954d`（arena/shader 生命周期）与 `b7e1d66`（名称预校验） |
| macroquad 侧 `a8aace0` | 只改 `Cargo.toml`（+1/-1）；真正的 resident geometry 是同一参考实现的前一提交 `7068495`（`quad_gl.rs` +97/-3） |
| macroquad 侧 `51147e9` | 真实现：`texture_reads.rs` +340、`quad_gl.rs` +133、`lib.rs`/`prelude.rs` 各 1 行 |
| macroquad 侧 `27cc446` | 真实现：`quad_gl.rs` +15，只有 `queued_texture_dependency` |
| macroquad 侧 `461ad5f` | 真实现：`quad_gl.rs` +13，两个只读状态查询 |
| miniquad 侧 `eb0eda0`、`e63917d`、`bc1d9d4` | 分别是原生帧循环诊断、提交诊断、有界采集容量；macroquad `50b5dbf`/`cc302a2`/`161fab1` 只固定 rev，不是优化 |
| X-01 参考实现 | 提交不在库 `master` 祖先链上，但核心已 squash 合入 macroquad `139ac95` 与 miniquad `1796f44`；仅在库 `master`，未进入 Phira-Pro `main` 的固定 rev |

### 0.4 总览表

判定列使用“正 / 无 / 负 / 未完成 / 非优化”。“无”表示没有可归因的收益证据；“负”表示同一 cohort 中出现了方向一致的变慢或明确回归；“未完成”表示质量或验收链未闭合。“主线是否已实施”只按 Phira-Pro `main` 的固定生产库 rev 判定；“参考实现默认”描述参考实现自身，不是生产默认。

| ID | 优化项 | macroquad 侧 | miniquad 侧 | 实现侧 | 主线是否已实施（Phira-Pro main 的固定 rev） | 参考实现默认 | 当前判定 |
|---|---|---|---|---|---|---|---|
| L-01 | 固定 RGBA8 纹理存储 | `4b46e86`（仅 pin） | `19e97bd` | miniquad | 否 | 关（应用 flag） | 无优化（当前无收益证据；潜在正） |
| L-02 | 缓冲扩容前清绑定缓存 | `de34611`（+5 行） | `1dfaa93` | macroquad + miniquad API | 否 | 开（库内自动） | 无优化（正确性修复，非性能） |
| L-03 | 有界 PBO 字体上传 | `ed1db13`（仅 pin） | `4d85ed3` + `d7dc3f4` | miniquad | 否 | 关 | 无（潜在正，无稳定收益） |
| L-04 | 流缓冲 discard 更新 | `cd0ab01`（仅 pin） | `4b26d71` | miniquad | 否 | 关 | 无/负（平均回归，冷帧改善） |
| L-05 | uniform 按位缓存 | `a8aace0`（仅 pin） | `9466727` | miniquad | 否 | 关（unsafe 开启） | 无（调用降 67%–69%，墙钟无收益） |
| L-06 | 资源标识、生命周期与绑定清理 | `c862fc8`（仅 pin） | `1dfaa93`（链上含 `406954d`、`b7e1d66`） | miniquad | 否 | 开（arena）；delete/clear 显式 | 无优化（正确性/内存治理，非性能） |
| L-07 | 常驻网格延迟提交 | `7068495`；`a8aace0` 仅 pin | `1796f44` 仅提供 offset/orphan 原语（库 master） | macroquad | 否 | 显式调用 | 负（普通 gameplay 无收益） |
| L-08 | 图集读取依赖跟踪 | `51147e9` | 无 | macroquad | 否 | 关（需注册） | 无（依赖治理，无独立性能数据） |
| L-09 | 保守排队纹理依赖查询 | `27cc446` | 无 | macroquad | 否 | 关 | 无（无可靠收益） |
| L-10 | 绘制状态查询（判定线省略） | `461ad5f` | 无 | macroquad | 否 | 开（getter）；应用 flag 关 | 无优化（API 本身无性能收益） |
| D-01 | 原生帧循环/提交/采集诊断 | `50b5dbf`、`cc302a2`、`161fab1`（均仅 pin） | `eb0eda0`、`e63917d`、`bc1d9d4` | miniquad | 否 | 关 | 非优化（诊断） |
| X-01 | 共享 arena/连续 pass（squash） | `139ac95`（库 master） | `1796f44`（库 master） | 两库 master | 否（仅库 master 已合入） | 库 master 默认（仅基线） | 排除项 |

### 0.5 证据边界

- 本文的“应用级数据”全部来自同一设备的有限窗口，不能外推为全曲、所有 GPU 或纯 GPU 时间。墙钟包含驱动等待、调度和呈现，不是 GPU elapsed。
- `completed` 一类的数据如果带 `glFinish`，它串行化了 CPU/GPU，不能当作真实 in-flight 帧率。
- 质量对照只证明被采集帧的最终 RGBA 相同，不证明所有时刻、所有字体、所有分辨率或所有 GPU 都相同。
- 单元测试数量是库层契约证据，不是性能证据。

## L-01 固定 RGBA8 纹理存储

### 1. 一句话结论与判定

miniquad 新增 `Texture::try_from_rgba8_immutable`：用 `glTexStorage2D` 一次性分配固定 RGBA8 存储，再用 `glTexSubImage2D` 上传初始像素；纹理仍是 `Copy` 句柄，更新仍然合法，但 resize 不再合法。macroquad 的 `4b46e86` 只是把依赖固定到该 miniquad 提交，不是实现。

这是**潜在正优化**：它可能减少驱动对可变纹理存储的重新规格化与分配，但当前没有应用级性能数据；质量探针也没有完整闭合。判定为**无优化（当前无收益证据；潜在正）**，置信度：实现事实高，收益低。

### 2. 问题与瓶颈

原有 `Texture::new` 走 `glTexImage2D`：每次创建/更新都可能让驱动重新决定内部存储、布局和 mipmap 链。字体图集是“固定尺寸的 RGBA8 页面 + 多次矩形子更新”的典型负载，理论上适合不可变存储：

- 一次 `glTexStorage2D` 固定层数、尺寸和内部格式；
- 后续只做 `glTexSubImage2D`，不再改变存储规格；
- 驱动可以减少重新分配、格式转换和同步的机会。

但“理论上适合”不等于“实测更快”。字体上传的瓶颈可能在上传路径、驱动等待、图集布局或 GPU 采样，而不在 `glTexImage2D` 本身。已有 PBO 探针也显示：提交阶段变快，同步完成时间并没有变快。因此必须把 L-01 当作待验证的潜在优化，而不是既得收益。

### 3. 设计思路与不变量

实现刻意把 API 做成“可拒绝的尝试”，而不是静默回退：

- `ImmutableTextureAttempt::Ready(Texture)`：新纹理已经完整创建并上传。
- `ImmutableTextureAttempt::Declined(reason)`：当前平台/API/入口不满足，调用方可以安全地走原来的可变构造。
- `Err(ImmutableTextureError)`：发生了不可安全重试的致命错误（非法数据、超限、外部 context、GL 错误、清理失败等）。调用方不得再用旧路径重试，否则可能掩盖状态损坏。

关键不变量：

1. 只接受非零 `u16` 宽高，且 `bytes.len() == width * height * 4`；不满足直接 `InvalidData`，不触碰 GL。
2. 只支持 RGBA8、单层、Linear/Clamp；不试图把该 API 扩展成通用纹理构造器。
3. 能力门必须同时检查**实际 API 版本**和**函数入口是否可调用**；不能只看 Android API level 或扩展字符串。
4. 桌面 GL 必须是 4.2 以上的 core profile；ES 必须是 3.0 以上。未知版本、WebGL、旧 GL 一律 `Declined(UnsupportedApi)`。
5. 纹理名、像素 unpack 状态、活动纹理单元和 unit 0 绑定都必须按事务恢复；失败时删除候选纹理名。
6. `Declined` 只用于“还没创建成功且状态已恢复”的情况；任何清理失败都升级为 `Err`，不做“看起来没事”的回退。
7. 返回的 `Texture` 仍是普通 `Copy` 句柄；删除仍由调用方负责，不引入引用计数或注册表。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 miniquad，提交 `19e97bd3d9b0ecce86ae50558311175a92d933a3`（2026-10-14，“加入固定纹理存储”）。隔离改动：

- `src/graphics/texture.rs`：+510 行，新增尝试/拒绝/失败/错误类型、版本解析、事务绑定 guard、`immutable::create` 和公开构造器。
- `src/graphics/texture_immutable_tests.rs`：+335 行，覆盖版本解析、输入校验、GL 事务和清理路径。
- `src/native/gl.rs`：+7 行，加载 `glTexStorage2D` 入口。
- `src/graphics/pixel_unpack.rs`：141 行修改，把客户端内存 unpack 状态提取成可复用的 `ClientMemoryUnpack`，供 immutable 创建和普通上传共用。
- `src/graphics.rs`：2 行修改，导出新 API。

核心数据结构：

- `ImmutableTextureAttempt`：`Ready` / `Declined`。
- `ImmutableTextureDecline`：`UnsupportedPlatform`、`MissingEntryPoints`、`UnsupportedApi`、`NameZero`、`Gl { stage, code }`。
- `ImmutableTextureFailure`：`InvalidData`、`InvalidMaximum`、`TextureTooLarge`、`ForeignContext`、`Gl`、`Unpack`、`BindingRestore`、`Creation`。
- `ImmutableTextureError`：主失败 + unpack 清理错误 + binding 清理错误 + 删除错误码；`is_cleanup_failed()` 用于决定是否还能安全 `Declined`。
- `TextureBinding`：保存当前 `GL_ACTIVE_TEXTURE` 与 unit 0 的 `GL_TEXTURE_BINDING_2D`，恢复时同时更新 miniquad 的 `ctx.cache.textures[0]`。
- `ImmutableApi`：`Es` 或 `Desktop`，由 `immutable_api` 解析版本字符串得到。

算法顺序：

1. 校验输入；不合法直接 `Err`。
2. 检查 12 个 GL 入口是否都已加载；缺一个就 `Declined(MissingEntryPoints)`。
3. 清空既有 GL 错误；查询 `GL_VERSION`，解析 API；桌面端再查 `CONTEXT_PROFILE_MASK`，必须是 core profile。
4. 查询 `GL_MAX_TEXTURE_SIZE`，拒绝零/负上限和超限尺寸。
5. 捕获 `TextureBinding`（活动纹理单元 + unit 0 绑定），随后用 `ClientMemoryUnpack::begin` 确保 PBO 绑定为 0、UNPACK 行对齐为 1、row length/skip 归零。
6. `glGenTextures`、`glBindTexture`、`glTexStorage2D(GL_RGBA8, 1, w, h)`、`glTexSubImage2D`、四个采样参数。
7. 恢复 unpack 状态和纹理绑定；任何一步失败都记录清理码。
8. 只有 operation、unpack cleanup、binding cleanup 全部成功才返回 `Ready`；否则删除候选名字并构造 `ImmutableTextureError`。
9. 如果失败原因只是创建阶段的 `Creation` 且没有清理失败，返回 `Declined`；其他情况返回 `Err`。

### 5. 完整核心代码

以下代码来自 miniquad `19e97bd` 的 `src/graphics/texture.rs`。为避免省略，公开构造器和创建事务都完整列出。

```rust
/// A ready owner, or a refusal after which the original mutable constructor is safe.
#[derive(Debug, PartialEq)]
pub enum ImmutableTextureAttempt {
    Ready(Texture),
    Declined(ImmutableTextureDecline),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImmutableTextureDecline {
    UnsupportedPlatform,
    MissingEntryPoints,
    UnsupportedApi,
    NameZero,
    Gl { stage: &'static str, code: u32 },
}
/// Fatal errors must not be silently retried through a legacy GL upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImmutableTextureFailure {
    InvalidData {
        width: u16,
        height: u16,
        bytes: usize,
    },
    InvalidMaximum(i32),
    TextureTooLarge {
        maximum: i32,
    },
    ForeignContext,
    Gl {
        stage: &'static str,
        code: u32,
    },
    Unpack(crate::PixelUnpackError),
    BindingRestore {
        stage: &'static str,
        code: u32,
        active_restore_code: Option<u32>,
    },
    Creation(ImmutableTextureDecline),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableTextureError {
    pub failure: ImmutableTextureFailure,
    pub unpack_cleanup: Option<crate::PixelUnpackError>,
    pub binding_cleanup: Option<ImmutableTextureFailure>,
    pub delete_code: Option<u32>,
}
impl ImmutableTextureError {
    fn new(failure: ImmutableTextureFailure) -> Self {
        Self {
            failure,
            unpack_cleanup: None,
            binding_cleanup: None,
            delete_code: None,
        }
    }
    pub fn is_cleanup_failed(&self) -> bool {
        let inner_unpack_cleanup = match &self.failure {
            ImmutableTextureFailure::Unpack(
                crate::PixelUnpackError::ClientMemoryBindingMismatch { cleanup_code, .. }
                | crate::PixelUnpackError::BufferNameZero { cleanup_code }
                | crate::PixelUnpackError::CapacityMismatch { cleanup_code, .. }
                | crate::PixelUnpackError::Gl { cleanup_code, .. },
            ) => cleanup_code.is_some(),
            _ => false,
        };
        inner_unpack_cleanup
            || self.unpack_cleanup.is_some()
            || self.binding_cleanup.is_some()
            || self.delete_code.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImmutableApi {
    Es,
    Desktop,
}
fn immutable_api(version: &[u8]) -> Option<ImmutableApi> {
    let (version, es) = match version.strip_prefix(b"OpenGL ES ") {
        Some(version) => (version, true),
        None => (version, false),
    };
    let dot = version.iter().position(|&byte| byte == b'.')?;
    fn number(bytes: &[u8]) -> Option<u32> {
        if bytes.is_empty() {
            return None;
        }
        bytes.iter().try_fold(0u32, |value, &digit| {
            if !digit.is_ascii_digit() {
                return None;
            }
            value.checked_mul(10)?.checked_add((digit - b'0') as u32)
        })
    }
    let major = number(&version[..dot])?;
    let remaining = &version[dot + 1..];
    let minor_end = remaining
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(remaining.len());
    let minor = number(&remaining[..minor_end])?;
    if remaining
        .get(minor_end)
        .is_some_and(|byte| *byte != b'.' && *byte != b' ')
    {
        return None;
    }
    if es && major >= 3 {
        Some(ImmutableApi::Es)
    } else if !es && (major > 4 || (major == 4 && minor >= 2)) {
        Some(ImmutableApi::Desktop)
    } else {
        None
    }
}
fn rgba8_input_valid(width: u16, height: u16, bytes: usize) -> bool {
    width != 0
        && height != 0
        && (width as usize)
            .checked_mul(height as usize)
            .and_then(|size| size.checked_mul(4))
            == Some(bytes)
}
```

下面是创建事务的绑定 guard。它只触碰纹理单元 0，不碰 draw queue 或其他 owner；恢复时如果 context 已经变化，返回 `ForeignContext`，不再操作 GL。

```rust
/// This guard touches only unit zero, never the draw queue or another owner.
struct TextureBinding<'a> {
    ctx: &'a mut Context,
    context: usize,
    active: GLint,
    binding: GLuint,
    restored: bool,
}
impl<'a> TextureBinding<'a> {
    unsafe fn capture(ctx: &'a mut Context) -> Result<Self, ImmutableTextureError> {
        let mut active = 0;
        glGetIntegerv(ACTIVE_TEXTURE, &mut active);
        check("active texture capture").map_err(ImmutableTextureError::new)?;
        glActiveTexture(GL_TEXTURE0);
        let mut binding = 0;
        glGetIntegerv(TEXTURE_BINDING_2D, &mut binding);
        if let Err(failure) = check("unit zero binding capture") {
            glActiveTexture(active as GLenum);
            let cleanup = check("capture active texture restore").err();
            let mut error = ImmutableTextureError::new(failure);
            error.binding_cleanup = cleanup;
            return Err(error);
        }
        let context = ctx.context_id();
        Ok(Self {
            ctx,
            context,
            active,
            binding: binding as GLuint,
            restored: false,
        })
    }
    fn restore(&mut self) -> Result<(), ImmutableTextureFailure> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        if self.ctx.context_id() != self.context {
            return Err(ImmutableTextureFailure::ForeignContext);
        }
        unsafe {
            glActiveTexture(GL_TEXTURE0);
            let selection_error = glGetError();
            let (stage, binding_error) = if selection_error != 0 {
                ("unit zero restore selection", selection_error)
            } else {
                glBindTexture(GL_TEXTURE_2D, self.binding);
                let error = glGetError();
                if error == 0 {
                    self.ctx.cache.textures[0] = self.binding;
                }
                ("unit zero binding restore", error)
            };
            glActiveTexture(self.active as GLenum);
            let active_error = glGetError();
            if binding_error != 0 {
                return Err(ImmutableTextureFailure::BindingRestore {
                    stage,
                    code: binding_error,
                    active_restore_code: if active_error == 0 {
                        None
                    } else {
                        Some(active_error)
                    },
                });
            }
            if active_error != 0 {
                return Err(gl_failure("active texture restore", active_error));
            }
        }
        Ok(())
    }
    fn finish(mut self) -> Result<(), ImmutableTextureFailure> {
        self.restore()
    }
}
impl Drop for TextureBinding<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
```

创建主体如下。注意：`glGetString` 返回的版本字符串只用于能力门；真正的 GL 对象创建在版本检查通过后才开始。

```rust
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

公开入口只做输入校验和平台分派。非支持平台不执行任何 GL 调用，直接 `Declined(UnsupportedPlatform)`；这意味着调用方可以无成本地回退。

```rust
impl Texture {
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
}
```

### 6. 开关、默认值、能力探测与回退

- 库层没有 `enable_immutable_texture` 这样的全局开关；是否使用由调用方选择 `try_from_rgba8_immutable` 还是原来的 `from_rgba8` 决定。
- 应用研究钩子使用独立 flag `font_immutable_storage`，默认 `false`；另有 `force_missing` 研究开关用于验证 `Declined` 回退。
- 能力探测是“版本 + 入口”双门：ES 3.0+，或桌面 4.2+ core；同时 12 个入口必须已加载。Android API level、GPU 型号、扩展字符串都不能替代这个检查。
- 回退契约：
  - `Declined(UnsupportedPlatform | MissingEntryPoints | UnsupportedApi | NameZero | Gl { stage, code })` 且没有清理失败：调用方可以安全走原可变构造。
  - `InvalidData`、`InvalidMaximum`、`TextureTooLarge`、`ForeignContext`、`Unpack`、`BindingRestore`、清理失败：必须当作致命错误处理，不得自动重试旧路径。
  - 非支持平台在编译期直接返回 `Declined`，不执行 GL。
- 该 API 不改变普通 `Texture::new`、`from_rgba8`、`update`、`update_texture_part`、`resize` 的语义；只有调用方显式选择新构造器时才生效。

### 7. 测试与测量（完整数据）

**库层测试**

- `src/graphics/texture.rs` 内有 2 个 guard 测试：版本解析、输入校验。
- `src/graphics/texture_immutable_tests.rs` 有 1 个事务测试模块，335 行断言，覆盖成功、拒绝、错误清理、context 变化、绑定恢复等路径。
- 本地研究记录（`Phira-Pro/本地证据/low-quality-normal/AGENTS.md` 第 27 行）记载：参考实现记录中 37 项库测试通过，Windows/Android 构建检查通过。这里的 37 项是库测试总数，不是 37 个 immutable 专属测试。
- `texture_immutable_tests.rs` 的测试重点是“失败后状态是否恢复”，而不是 FPS。

**应用级质量**

- 5 个 canonical case：`pgr_empty_new_repeat_change`、`harmony_geometry_multiframe`、`normalized_assets_drop_rebuild`、`bounded_real_brush_resize`、`rebuild_after_drop`。
- v3 质量运行没有完整通过：Case 0 的 strict RGBA、rect bits、native draw shape 均 exact；Case 1（Harmony 居中 Unicode、alpha 0.65）出现 ON 与 OFF 之间 **60 pixels / 80 RGB channels / max 1 / alpha 0** 的差异，探针以 `Strict immutable quality comparison failed in harmony_geometry_multiframe` 结束。
- 因此不能写“固定纹理存储画质已通过”。它只证明了部分 case 的像素与顺序，且失败 case 尚未解决。

**应用级性能**

- **无应用级性能数据**。没有同一 SO、同一窗口、OFF/ON 的 FPS 或墙钟 A/B；不能把“使用 immutable storage”本身说成加速。
- 已有的字体探针是初始化 fixture + `glFinish` 诊断，不能代表 gameplay。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无优化（当前无收益证据；潜在正）**。

理由：

- 实现完整，能力门、事务恢复和 typed error 都做得比较严格；这部分置信度高。
- 质量探针没有完整通过，尤其是 Case 1 的 60 像素差异；不能把“部分 strict RGBA 相同”当成全量画质通过。
- 没有应用级性能数据；无法判断固定存储是否减少驱动分配或改善字体上传尾延迟。
- 该 API 只覆盖 RGBA8、单层、Linear/Clamp，适用面窄；即使有效，也可能只对字体图集初始化有意义。

置信度：实现事实高；画质结论中等（有明确失败样本）；性能结论低（无数据）。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `19e97bd` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有该 API。
- **参考实现：已落地。** miniquad 侧含完整 API、事务 guard 与测试；macroquad 侧只有 `4b46e86` 的 Cargo pin，没有实现代码。
- **参考实现默认：关。** 库层无全局开关，需显式调用 `try_from_rgba8_immutable`；应用 flag `font_immutable_storage=false`。
- **是否有调用方：有（研究/诊断路径）。** 应用侧研究钩子存在于 `font_atlas_pool` 相关路径；没有生产默认调用方在普通路径上无条件使用。
- **收益标注：潜在正优化；当前无应用级收益证据。** 质量探针也未闭合。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在字体图集的“初始整页分配”接点调用 `try_from_rgba8_immutable`；不要在每次字形更新时调用。
2. 调用前确认宽高能用 `u16` 表示，字节数严格等于 `w*h*4`；图集池的尺寸策略要先收敛。
3. `Ready`：保存返回的 `Texture`，后续仍用 `update_texture_part` 更新；禁止调用 `resize`。
4. `Declined`：记录 typed reason，回退到原有 `from_rgba8`；回退必须只发生一次，不能循环尝试。
5. `Err`：视为致命错误，停止该池的后续分配/渲染，按现有 fatal 路径上报；不得静默改用可变路径。
6. 在同一 GL context/线程内创建；跨线程或 context 丢失后不要复用句柄。
7. 增加 OFF/ON 同 SO 的初始化与 gameplay 墙钟、严格 RGBA、驱动错误计数；至少覆盖字体选择器、结算、Unicode、resize/rebuild。

风险：

- **画质风险**：当前已有 Case 1 的 60 像素差异；在差异根因未解释前，不能默认启用。
- **生命周期风险**：`Texture` 仍是 `Copy`，删除后旧句柄仍可能被使用；接入方必须保证所有绘制命令消费完再删除。
- **尺寸风险**：`u16` 限制和不可 resize 会改变图集池的扩缩容策略；溢出或重建路径必须显式处理。
- **能力风险**：桌面旧驱动、WebGL、OHOS 等平台会 `Declined`；回退路径必须保留原构造器。
- **状态风险**：虽然实现恢复 unit 0 绑定和 unpack 状态，但它不拥有其他纹理单元或外部 raw GL 状态；接入方不要把该 API 当作完整 GL 状态隔离。

### 11. 来源与可追溯性

- miniquad 实现：`19e97bd3d9b0ecce86ae50558311175a92d933a3`。
- 隔离 diffstat：`src/graphics.rs` 2 行、`src/graphics/pixel_unpack.rs` 141 行、`src/graphics/texture.rs` 510 行、`src/graphics/texture_immutable_tests.rs` 335 行、`src/native/gl.rs` 7 行。
- macroquad pin：`4b46e869cdfcc29028a94eadb7cffcc149e4dce4`，只改 `Cargo.toml`。
- 代码位置：`src/graphics/texture.rs:149`（attempt）、`:231`（版本解析）、`:269`（输入校验）、`:319`（binding guard）、`:400`（create）、`:653`（公开构造器）。
- 测试/质量记录：`Phira-Pro/本地证据/low-quality-normal/AGENTS.md` 第 27 行；`immutable-text-quality-v3-execution/results.json` 的失败字段；v3 原始 case 文件。

## L-02 缓冲扩容前的绑定缓存失效

### 1. 一句话结论与判定

macroquad 在 `QuadGl::draw` 的 arena 扩容路径中，先调用 miniquad 的 `clear_resource_bindings()`，再删除并重建 VBO/IBO。`de34611` 本身只有 5 行，但它是这条链上**唯一真正改 macroquad 代码**的提交；miniquad 侧 `1dfaa93` 提供 17 行的清理 API。

这不是性能优化，而是**正确性修复**：避免 GL 名字被立即复用时，miniquad 缓存的 vertex attribute/VAO 绑定仍指向旧对象。判定为**无优化（正确性修复，非性能）**，置信度高。

### 2. 问题与瓶颈

宏quad 的 arena 在容量不足时会：

1. `Buffer::delete()` 删除旧 GL buffer；
2. 立即 `Buffer::stream()` 创建新 buffer；
3. 新旧 buffer 可能拿到同一个 GL 名字。

miniquad 的 `GlCache` 会缓存：

- 每个 vertex attribute 的 `glVertexAttribPointer` 参数与 `gl_vbuf`；
- `GL_ARRAY_BUFFER` / `GL_ELEMENT_ARRAY_BUFFER` 的当前绑定；
- 纹理单元绑定；
- `stored_vertex_buffer` / `stored_index_buffer` / `stored_texture` 等逻辑缓存。

如果删除后名字被复用，而缓存仍认为“这个 attribute 已经指向同名 buffer”，下一次绘制可能复用错误的指针/stride，轻则画面空白，重则驱动崩溃。这个问题在字体图集扩容时最容易触发：第一帧 VBO 较小，第二帧字形增多，arena 扩容并复用名字。

### 3. 设计思路与不变量

修复把“删除/重建 GL 名字”与“清理缓存”绑定成一个顺序契约：

- 在**任一** arena buffer 被替换之前，先调用 `ctx.clear_resource_bindings()`；
- 清理必须覆盖 vertex attribute 数组、array/element buffer 绑定、纹理绑定和 stored 逻辑值；
- 清理不引入 render pass 边界，也不插入 `glFinish` 或 GPU 等待；
- 清理后由后续 `apply_bindings` / `apply_pipeline` 重新建立状态；
- 只有发生扩容时才清理，正常帧不承担这个成本。

不变量：**任何被删除的 GL buffer 名字，在删除前都不能再被 miniquad 缓存引用。**

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧有两部分：

- macroquad `de34611bca60abda9c4a2d8ebea568970fd7a850`，`src/quad_gl.rs` +5；
- miniquad `1dfaa933c642bc1b0ef3a27701a941b20ee084c1`，`src/graphics.rs` +17，新增 `GraphicsContext::clear_resource_bindings`。

macroquad 的 5 行修复位于 `QuadGl::draw` 的 buffer 扩容段：先计算 `vertex_bytes` / `index_bytes`，在 `bindings` 初始化之后、两个 `if size < bytes` 分支之前插入一次清理。条件是“任一 buffer 需要扩容”。

miniquad 的清理函数做五件事：

1. 遍历 `cache.attributes`，对已缓存的 attribute 调用 `glDisableVertexAttribArray(index)`，并清空缓存槽；
2. `cache.clear_buffer_bindings()`；
3. `cache.clear_texture_bindings()`；
4. 把 `stored_vertex_buffer`、`stored_index_buffer`、`stored_index_type`、`stored_texture` 归零/置空；
5. 不调用 `glBindBuffer` 之外的绘制/同步操作。

### 5. 完整核心代码

macroquad 的隔离改动（`de34611`，`src/quad_gl.rs`）：

```rust
        // Deleting a buffer may release its name for immediate reuse. Invalidate
        // cached attribute bindings before either arena buffer is replaced.
        if bindings.vertex_buffers[0].size() < vertex_bytes || bindings.index_buffer.size() < index_bytes {
            ctx.clear_resource_bindings();
        }
        if bindings.vertex_buffers[0].size() < vertex_bytes {
            bindings.vertex_buffers[0].delete();
            bindings.vertex_buffers[0] = Buffer::stream(ctx, BufferType::VertexBuffer, vertex_bytes.next_power_of_two());
        }
        if bindings.index_buffer.size() < index_bytes {
            bindings.index_buffer.delete();
            bindings.index_buffer = Buffer::stream(ctx, BufferType::IndexBuffer, index_bytes.next_power_of_two());
        }
```

miniquad 的清理 API（`1dfaa93`，`src/graphics.rs`）：

```rust
    /// Call after consuming deferred commands and before deleting/reusing raw
    /// buffers or textures. GL name reuse must not match obsolete cached VAO or
    /// sampler bindings. Does not introduce a render-pass boundary or GPU wait.
    pub fn clear_resource_bindings(&mut self) {
        for (index, attribute) in self.cache.attributes.iter_mut().enumerate() {
            if attribute.take().is_some() {
                unsafe { glDisableVertexAttribArray(index as GLuint); }
            }
        }
        self.cache.clear_buffer_bindings();
        self.cache.clear_texture_bindings();
        self.cache.stored_vertex_buffer = 0;
        self.cache.stored_index_buffer = 0;
        self.cache.stored_index_type = None;
        self.cache.stored_texture = 0;
    }
```

### 6. 开关、默认值、能力探测与回退

- 没有开关：只要使用 macroquad 侧的参考实现，扩容时自动清理。
- 没有能力探测：这是状态缓存正确性问题，不依赖 GL 扩展。
- 没有回退：不清理就是不安全；该修复不是可选项。
- 不改变正常帧路径；只有 `size < required` 时多一次缓存遍历和 `glDisableVertexAttribArray` 调用。
- 对 raw GL 调用方的影响：清理会禁用 miniquad 缓存的 attribute 数组；raw GL 调用方如果自己启用过 attribute，需要重新建立状态，或不要在清理后依赖旧 VAO。

### 7. 测试与测量（完整数据）

- **无应用级性能数据**；这是正确性修复，不应报帧率收益。
- 本地记录（`本地证据/low-quality-normal/AGENTS.md` 第 19 行）描述了触发条件：第一帧 VBO 96B 初始 256，第二帧首行 1248B 需要 2048，GLuint 复用；修复前第二帧出现全 blank，修复后在 immutable v3 的 Case 0 中 blank 消失，strict RGBA/rect bits/native shape 均 exact。
- macroquad 编译检查通过（记录中的 `e16fff0`、diff `67d87f`）；miniquad 的 `clear_resource_bindings` 源码检查通过。
- 该修复没有独立单测；它的验证依赖真实 Android 字体扩容回归和固定帧对照。
- 不能把 Case 0 的通过扩大成“immutable 全量画质通过”：v3 的 Case 1 仍有 60 像素差异。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无优化（正确性修复，非性能）**。

理由：

- 问题机制明确：GL 名字复用 + 旧缓存引用会导致错误绑定；修复顺序正确。
- 真实设备上已有 blank 消失的回归证据。
- 它不声称性能收益，因此不需要用 FPS 证明价值。
- 影响范围仅限扩容帧，风险可控。

置信度：高。唯一未闭合的是“所有 GL 名字复用场景都被覆盖”，例如纹理/其他 buffer 的 raw 删除路径仍需调用方遵守同样的清理契约。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** macroquad `de34611` 与 miniquad `1dfaa93` 都不在 Phira-Pro `main` 所固定的生产库 rev 中。
- **参考实现：已落地。** `de34611` 的隔离改动只有 5 行；`1dfaa93` 提供 17 行 `clear_resource_bindings` API。
- **参考实现默认：开。** 只要参考实现的库内扩容路径被触发就自动清理，不需要应用显式开启。
- **是否有调用方：有（库内）。** `QuadGl::draw` 的 arena 扩容路径；应用侧无需单独调用。
- **收益标注：正确性修复，不计性能收益。**

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 同时固定 macroquad `de34611` 与 miniquad `1dfaa93`（或包含该 API 的后续 rev）。
2. 确认 `QuadGl::draw` 的扩容分支在删除前调用 `clear_resource_bindings`；不要在 `delete()` 之后才调用。
3. 在真实字体扩容场景（首帧小 VBO、次帧大 VBO）做固定帧 RGBA 与 draw 顺序回归。
4. 检查 raw GL 粒子/Paint/自定义材质是否在扩容后依赖旧 attribute 状态；如有，显式重建。

风险：

- `clear_resource_bindings` 会禁用所有缓存的 vertex attribute 数组；如果某些外部代码假设 attribute 一直启用，需要重新启用。
- 它不删除/重建 VAO，只清缓存和禁用数组；如果外部持有自己的 VAO，仍要自行管理。
- 清理不等待 GPU；如果旧 buffer 仍被 queued draw 引用，删除仍可能触发驱动同步或未定义行为。调用方必须先消费完延迟命令。
- 该修复不能替代资源生命周期管理；它只解决“删除后名字复用与缓存不匹配”。

### 11. 来源与可追溯性

- macroquad 实现：`de34611bca60abda9c4a2d8ebea568970fd7a850`，`src/quad_gl.rs` +5。
- miniquad API：`1dfaa933c642bc1b0ef3a27701a941b20ee084c1`，`src/graphics.rs` +17。
- 代码位置：macroquad `src/quad_gl.rs:819`；miniquad `src/graphics.rs:785`。
- 隔离 diffstat：macroquad 1 文件 +5；miniquad 1 文件 +17。
- 设备/回归记录：`Phira-Pro/本地证据/low-quality-normal/AGENTS.md` 第 19 行；`immutable-text-quality-v3-execution` 的 Case 0 原始结果。

## L-03 有界 PBO 字体上传

### 1. 一句话结论与判定

miniquad 新增 `PixelUnpackBuffer` / `PixelUnpackSession`：在加载期创建固定容量的 PBO，用 `glBufferSubData` 写入完成字形像素，再用 `glTexSubImage2D` 的 offset 形式上传到纹理；会话结束恢复 unpack 绑定、行对齐、row length/skip 和纹理绑定。macroquad 的 `ed1db13` 只是固定依赖，不是实现。

这是**潜在正优化**：它可以把“客户端内存上传”变成“PBO 上传”，减少驱动在 `glTexSubImage2D` 调用内拷贝/同步的机会。但已有应用 ABBA 没有稳定收益：OFF 组 11.3666ms，ON 组 12.5539ms，ON 高 1.1873ms（+10.45%），且 OFF 自身漂移 1.9458ms 大于组差。判定为**无（潜在正，无稳定收益）**。

### 2. 问题与瓶颈

字体图集的更新模式是“很多小矩形、每帧少量、总量分散”。直接用客户端指针调用 `glTexSubImage2D` 时，驱动可能：

- 在调用内同步等待前一帧 GPU 读取；
- 为每次上传做临时拷贝；
- 因行对齐/row length 不同而走慢路径；
- 在驱动内部重新分配 staging 存储。

PBO 的思路是把像素先写入一个 GPU buffer，再让 `glTexSubImage2D` 从 buffer offset 读取。它可以把“驱动拷贝”从纹理上传调用中拆出来，理论上更适合批量/异步上传。

但 PBO 不是免费的：

- 调用方必须证明 GPU 已经读完这个 PBO，否则会覆盖 in-flight 数据；
- PBO 绑定、unpack 行状态、纹理绑定都必须恢复；
- 小批量上传可能不足以抵消额外绑定/守卫成本；
- 如果驱动仍然同步，submit 变快但 completed 不会变快。

这正是实测看到的情况：提交阶段明显变快，同步完成没有变快。

### 3. 设计思路与不变量

实现把 PBO 做成“有界、显式生命周期、无 Drop GL”的 owner：

- `PixelUnpackBuffer` 不是 `Copy`/`Clone`，Drop 不做 GL 删除；必须显式 `delete(ctx)`。
- 创建时校验容量、ES3 版本、入口、分配大小和 GL 错误；加载期用零初始化 `Vec` 分配，避免未初始化内存上传。
- `begin(ctx, texture)` 捕获完整的 unpack 状态，绑定 PBO 和纹理，重置 alignment/row length/skip；返回一个借用 ctx 和 buffer 的会话。
- `write_slice(offset, bytes)` 只允许非空、非重叠、按 cursor 前进的写入；写入后 cursor 记录范围。
- `upload_rgba8_rect(...)` 只允许上传“紧接前一次写入”的完整范围；它不额外查询 clock/error，错误留给 `finish`。
- `finish()` 恢复状态并检查 GL 错误；如果还有未上传的写入，返回 `IncompleteTransfer`。
- `delete()` 只允许创建线程和同一 context；外部 context/线程消费 owner 但不删除外部名字，避免误删复用名字。
- `with_client_memory_unpack` 是普通客户端指针路径的守卫：临时把 PBO 绑定设为 0、行状态重置，操作结束后精确恢复；用于“PBO 失败后的回退”或旧路径。

不变量：**每个 byte range 只写一次、只上传一次；PBO 的 GPU 读取完成证明由调用方提供，库本身不建立也不轮询 fence。**

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 miniquad：

- `4d85ed3fcca28de27c84a20bb14400da8c6a5435`（2026-10-11，“试验有界字体 PBO 上传”）：`graphics.rs` +2、`pixel_unpack.rs` +484、`pixel_unpack_guards.rs` +212、`native/gl.rs` +1。
- `d7dc3f484e0f3d6c0642517ef001917fee4cd5dc`（2026-10-11，“保护客户端纹理上传状态”）：`pixel_unpack.rs` +78/-1，`graphics.rs` 1 行修改。

核心类型：

- `PixelUnpackBuffer { name, capacity, context, render_thread }`。
- `PixelUnpackSession<'a> { ctx, buffer, texture, saved, restored, cursor, _render_thread }`。
- `SavedUnpack { binding, alignment, row_length, skip_rows, skip_pixels }`。
- `Cursor`（在 `pixel_unpack_guards.rs`）记录已写范围、已上传范围、上传 rect，拒绝重叠/越界。
- `PixelUnpackError`：`Unsupported`、`InvalidCapacity`、`LoadingAllocation`、`ForeignContext`、`ForeignThread`、`InvalidTexture`、`IncompleteTransfer`、`ClientMemoryBindingMismatch`、`BufferNameZero`、`CapacityMismatch`、`Gl`。

算法：

1. `new` 校验 `capacity_valid`，检查入口和 ES3 版本；`try_reserve_exact` + `resize(0)` 做加载期分配；`glGenBuffers`、`glBufferData(GL_STREAM_DRAW)`，查询 `GL_BUFFER_SIZE` 必须等于请求容量；恢复到之前的 `PIXEL_UNPACK_BUFFER_BINDING`。
2. `begin` 校验线程/context/纹理（非零、宽高非零、RGBA8）；捕获 `SavedUnpack`；保存 texture binding 到 miniquad cache；绑定 PBO；设置 alignment=1、row length=0、skip=0；任何 setup 错误都恢复并返回。
3. `write_slice` 用 `glBufferSubData` 写 PBO；cursor 检查 offset/len 非空、不重叠、不越界、未在上传后继续写。
4. `upload_rgba8_rect` 用 `glTexSubImage2D` 的最后一个参数传 `offset as *const GLvoid`；cursor 检查 rect 在纹理内、len 与 rect 匹配、offset/len 与上一次写入一致。
5. `finish` 恢复 saved unpack 和 texture binding；检查 `glGetError`；如果 cursor 还有 pending，返回 `IncompleteTransfer`。
6. `delete` 校验线程/context 后 `glDeleteBuffers`；如果删除前的 GL 错误非零，返回错误并带上删除错误码。

### 5. 完整核心代码

以下是 `pixel_unpack.rs` 的公开 API 主体。为保持可读性，按创建、会话、客户端回退三部分列出；每段都是完整函数，没有省略。

```rust
/// Loading-created PBO. No GL destructor: call delete explicitly.
pub struct PixelUnpackBuffer {
    name: GLuint,
    capacity: usize,
    context: usize,
    render_thread: ThreadId,
}
impl PixelUnpackBuffer {
    /// Allocates and initializes a fixed data store during loading. The actual
    /// ES version, callable entries, allocation size, and GL errors are checked.
    /// Invalid capacity and unavailable entries perform no GL calls.
    pub fn new(ctx: &mut Context, capacity: usize) -> Result<Self, PixelUnpackError> {
        if !guards::capacity_valid(capacity) {
            return Err(PixelUnpackError::InvalidCapacity);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Missing ES3 pixel-unpack entries",
            ));
        }
        unsafe {
            let prior = glGetError();
            if prior != 0 {
                return Err(gl_error("preexisting loading error", prior));
            }
            let version = glGetString(GL_VERSION);
            let version_error = glGetError();
            if version_error != 0 {
                return Err(gl_error("version query", version_error));
            }
            if version.is_null() || !guards::es3_version(CStr::from_ptr(version as _).to_bytes()) {
                return Err(PixelUnpackError::Unsupported(
                    "Active API is not OpenGL ES 3+",
                ));
            }
        }
        let mut initialized = Vec::new();
        initialized
            .try_reserve_exact(capacity)
            .map_err(|_| PixelUnpackError::LoadingAllocation)?;
        initialized.resize(capacity, 0u8);
        unsafe {
            let mut previous = 0i32;
            glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut previous);
            let error = glGetError();
            if error != 0 {
                return Err(gl_error("loading binding query", error));
            }
            let previous = previous as GLuint;
            let mut name = 0;
            glGenBuffers(1, &mut name);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "buffer name creation",
                    code: error,
                    cleanup_code,
                });
            }
            if name == 0 {
                return Err(PixelUnpackError::BufferNameZero {
                    cleanup_code: failed_creation(name, previous),
                });
            }
            glBindBuffer(PIXEL_UNPACK_BUFFER, name);
            glBufferData(
                PIXEL_UNPACK_BUFFER,
                capacity as _,
                initialized.as_ptr().cast(),
                GL_STREAM_DRAW,
            );
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "initialized buffer allocation",
                    code: error,
                    cleanup_code,
                });
            }
            let mut actual = 0i32;
            glGetBufferParameteriv(PIXEL_UNPACK_BUFFER, BUFFER_SIZE, &mut actual);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "actual buffer size query",
                    code: error,
                    cleanup_code,
                });
            }
            if actual < 0 || actual as usize != capacity {
                return Err(PixelUnpackError::CapacityMismatch {
                    expected: capacity,
                    actual,
                    cleanup_code: failed_creation(name, previous),
                });
            }
            glBindBuffer(PIXEL_UNPACK_BUFFER, previous);
            let error = glGetError();
            if error != 0 {
                let cleanup_code = failed_creation(name, previous);
                return Err(PixelUnpackError::Gl {
                    stage: "loading binding restore",
                    code: error,
                    cleanup_code,
                });
            }
            Ok(Self {
                name,
                capacity,
                context: ctx.context_id(),
                render_thread: std::thread::current().id(),
            })
        }
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Caller must supply completed GPU-read proof for this exact buffer.
    /// Restores the captured unpack binding and row/skip state exactly, which
    /// may be nondefault. Legacy client-pointer fallback separately requires
    /// its usual unpack-buffer-zero, tightly-packed input contract.
    /// Captures unpack state once. No other GL operations may be interleaved
    /// with this session; finish/drop it before any client-pointer fallback.
    pub fn begin<'a>(
        &'a mut self,
        ctx: &'a mut Context,
        texture: Texture,
    ) -> Result<PixelUnpackSession<'a>, PixelUnpackError> {
        if self.render_thread != std::thread::current().id() {
            return Err(PixelUnpackError::ForeignThread);
        }
        if self.context != ctx.context_id() {
            return Err(PixelUnpackError::ForeignContext);
        }
        if !guards::texture_valid(
            texture.gl_internal_id(),
            texture.width,
            texture.height,
            texture.format == TextureFormat::RGBA8,
        ) {
            return Err(PixelUnpackError::InvalidTexture);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Pixel-unpack entries unavailable",
            ));
        }
        let saved = unsafe {
            let prior = glGetError();
            if prior != 0 {
                return Err(gl_error("preexisting session error", prior));
            }
            let saved = SavedUnpack::capture();
            let error = glGetError();
            if error != 0 {
                return Err(gl_error("session state capture", error));
            }
            saved
        };
        let mut session = PixelUnpackSession {
            ctx,
            buffer: self,
            texture,
            saved,
            restored: false,
            cursor: guards::Cursor::default(),
            _render_thread: PhantomData,
        };
        session.ctx.cache.store_texture_binding(0);
        session.ctx.cache.bind_texture(0, texture.gl_internal_id());
        unsafe {
            // PBO is a distinct target, never use GlCache::bind_buffer here.
            glBindBuffer(PIXEL_UNPACK_BUFFER, session.buffer.name);
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
            glPixelStorei(UNPACK_ROW_LENGTH, 0);
            glPixelStorei(UNPACK_SKIP_ROWS, 0);
            glPixelStorei(UNPACK_SKIP_PIXELS, 0);
            let error = glGetError();
            if error != 0 {
                session.restore();
                let cleanup_code = match glGetError() {
                    0 => None,
                    code => Some(code),
                };
                return Err(PixelUnpackError::Gl {
                    stage: "session state setup",
                    code: error,
                    cleanup_code,
                });
            }
        }
        Ok(session)
    }

    /// Explicit render-thread retirement. On foreign context the owner is
    /// consumed, but its foreign native name is never deleted. The same applies
    /// to a different thread. No GL Drop runs.
    /// Any preexisting or deletion error prevents an Ok result.
    pub fn delete(self, ctx: &mut Context) -> Result<(), PixelUnpackError> {
        if self.render_thread != std::thread::current().id() {
            return Err(PixelUnpackError::ForeignThread);
        }
        if self.context != ctx.context_id() {
            return Err(PixelUnpackError::ForeignContext);
        }
        if !entries_loaded() {
            return Err(PixelUnpackError::Unsupported(
                "Pixel-unpack entries unavailable",
            ));
        }
        unsafe {
            let prior = glGetError();
            glDeleteBuffers(1, &self.name);
            let error = glGetError();
            if prior != 0 {
                return Err(PixelUnpackError::Gl {
                    stage: "preexisting retirement error",
                    code: prior,
                    cleanup_code: if error == 0 { None } else { Some(error) },
                });
            }
            if error != 0 {
                return Err(gl_error("buffer retirement", error));
            }
        }
        Ok(())
    }
}
```

会话的写入与上传如下。`write_slice` 只写 PBO，不触发纹理上传；`upload_rgba8_rect` 只做 `glTexSubImage2D`，错误由 `finish` 统一检查。

```rust
#[derive(Clone, Copy)]
struct SavedUnpack {
    binding: GLint,
    alignment: GLint,
    row_length: GLint,
    skip_rows: GLint,
    skip_pixels: GLint,
}
impl SavedUnpack {
    unsafe fn capture() -> Self {
        let mut saved = Self {
            binding: 0,
            alignment: 0,
            row_length: 0,
            skip_rows: 0,
            skip_pixels: 0,
        };
        glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut saved.binding);
        glGetIntegerv(GL_UNPACK_ALIGNMENT, &mut saved.alignment);
        glGetIntegerv(UNPACK_ROW_LENGTH, &mut saved.row_length);
        glGetIntegerv(UNPACK_SKIP_ROWS, &mut saved.skip_rows);
        glGetIntegerv(UNPACK_SKIP_PIXELS, &mut saved.skip_pixels);
        saved
    }
    unsafe fn restore(self) {
        glPixelStorei(GL_UNPACK_ALIGNMENT, self.alignment);
        glPixelStorei(UNPACK_ROW_LENGTH, self.row_length);
        glPixelStorei(UNPACK_SKIP_ROWS, self.skip_rows);
        glPixelStorei(UNPACK_SKIP_PIXELS, self.skip_pixels);
        glBindBuffer(PIXEL_UNPACK_BUFFER, self.binding as GLuint);
    }
}

/// One publication; holds exclusive context and PBO borrows until restoration.
pub struct PixelUnpackSession<'a> {
    ctx: &'a mut Context,
    buffer: &'a mut PixelUnpackBuffer,
    texture: Texture,
    saved: SavedUnpack,
    restored: bool,
    cursor: guards::Cursor,
    _render_thread: PhantomData<Rc<()>>,
}
impl PixelUnpackSession<'_> {
    /// Submits nonempty bytes into a fresh range. Invalid/overlapping writes
    /// and writes before the previous range's upload return false without GL.
    pub fn write_slice(&mut self, offset: usize, bytes: &[u8]) -> bool {
        if self.restored
            || self.ctx.context_id() != self.buffer.context
            || !self
                .cursor
                .can_write(self.buffer.capacity, offset, bytes.len())
        {
            return false;
        }
        unsafe {
            glBufferSubData(
                PIXEL_UNPACK_BUFFER,
                offset as _,
                bytes.len() as _,
                bytes.as_ptr().cast(),
            );
        }
        self.cursor.written(offset, bytes.len());
        true
    }
    /// Submits exactly the preceding write as a tightly packed RGBA8 rect.
    /// No clock/error query is added here; finish performs the error check.
    pub fn upload_rgba8_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        offset: usize,
        len: usize,
    ) -> bool {
        if self.restored
            || self.ctx.context_id() != self.buffer.context
            || !self.cursor.can_upload(
                self.buffer.capacity,
                self.texture.width,
                self.texture.height,
                x,
                y,
                width,
                height,
                offset,
                len,
            )
        {
            return false;
        }
        unsafe {
            glTexSubImage2D(
                GL_TEXTURE_2D,
                0,
                x,
                y,
                width,
                height,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                offset as *const GLvoid,
            );
        }
        self.cursor.uploaded(offset, len);
        true
    }
    fn restore(&mut self) -> bool {
        if self.restored {
            return true;
        }
        self.restored = true;
        if self.ctx.context_id() != self.buffer.context {
            return false;
        }
        unsafe {
            self.saved.restore();
        }
        self.ctx.cache.restore_texture_binding(0);
        true
    }
    /// Restores exact captured unpack state and Mini's texture binding, then
    /// checks upload/setup/restore errors. Caller must reject publication on Err.
    pub fn finish(mut self) -> Result<(), PixelUnpackError> {
        if !self.restore() {
            return Err(PixelUnpackError::ForeignContext);
        }
        let error = unsafe { glGetError() };
        if error != 0 {
            return Err(gl_error("session upload/restore", error));
        }
        if self.cursor.has_pending() {
            return Err(PixelUnpackError::IncompleteTransfer);
        }
        Ok(())
    }
}
impl Drop for PixelUnpackSession<'_> {
    fn drop(&mut self) {
        self.restore();
    }
}
```

普通客户端指针的守卫如下。它用于 PBO 失败后的安全回退：临时把 unpack buffer 绑定设为 0，重置 tight RGBA 行状态，执行闭包，然后精确恢复原有状态。注意：它不负责 GPU 完成证明，也不替代 `finish`。

```rust
/// Runs existing client-pointer APIs with unpack-buffer zero and tight RGBA rows.
/// Requires actual core ES3 and callable unpack entries; unsupported APIs decline.
/// The closure must use the supplied context. Captured external unpack state is
/// restored exactly, including a nonzero buffer. Operation/restore errors return
/// Err; callers creating GL owners must retain their handles for error cleanup.
/// Panic restores state through RAII and leaves GL errors for the caller.
#[rustfmt::skip]
pub fn with_client_memory_unpack<T>(
    ctx: &mut Context, operation: impl FnOnce(&mut Context) -> T,
) -> Result<T, PixelUnpackError> {
    #[cfg(any(target_arch = "wasm32", target_os = "macos", target_os = "ios"))]
    let loaded = false;
    #[cfg(not(any(target_arch = "wasm32", target_os = "macos", target_os = "ios")))]
    let loaded = ["glGetString", "glGetError", "glGetIntegerv", "glBindBuffer", "glPixelStorei"]
        .iter().all(|name| crate::native::gl::is_function_loaded(name));
    if !loaded { return Err(PixelUnpackError::Unsupported("Missing ES3 client-unpack entries")); }
    unsafe {
        let prior = glGetError();
        if prior != 0 { return Err(gl_error("preexisting client memory error", prior)); }
        let version = glGetString(GL_VERSION);
        let error = glGetError();
        if error != 0 { return Err(gl_error("client memory version query", error)); }
        if version.is_null() || !guards::es3_version(CStr::from_ptr(version as _).to_bytes()) {
            return Err(PixelUnpackError::Unsupported("Active API is not OpenGL ES 3+"));
        }
    }
    let saved = unsafe {
        let saved = SavedUnpack::capture();
        let error = glGetError();
        if error != 0 { return Err(gl_error("client memory state capture", error)); }
        saved
    };
    struct Restore<'a>(&'a mut Context, SavedUnpack, usize);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            if self.0.context_id() == self.2 { unsafe { self.1.restore(); } }
        }
    }
    let context = ctx.context_id();
    let scope = Restore(ctx, saved, context);
    unsafe {
        glBindBuffer(PIXEL_UNPACK_BUFFER, 0);
        glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
        glPixelStorei(UNPACK_ROW_LENGTH, 0);
        glPixelStorei(UNPACK_SKIP_ROWS, 0);
        glPixelStorei(UNPACK_SKIP_PIXELS, 0);
        let mut actual = -1;
        glGetIntegerv(PIXEL_UNPACK_BUFFER_BINDING, &mut actual);
        let error = glGetError();
        if error != 0 || actual != 0 {
            drop(scope);
            let cleanup_code = match glGetError() { 0 => None, code => Some(code) };
            return Err(if error != 0 {
                PixelUnpackError::Gl { stage: "client memory state setup", code: error, cleanup_code }
            } else {
                PixelUnpackError::ClientMemoryBindingMismatch { actual, cleanup_code }
            });
        }
    }
    let value = operation(&mut *scope.0);
    if scope.0.context_id() != context { return Err(PixelUnpackError::ForeignContext); }
    let operation_error = unsafe { glGetError() };
    drop(scope);
    let restore_error = unsafe { glGetError() };
    if operation_error != 0 {
        return Err(PixelUnpackError::Gl {
            stage: "client memory operation", code: operation_error,
            cleanup_code: if restore_error == 0 { None } else { Some(restore_error) },
        });
    }
    if restore_error != 0 { return Err(gl_error("client memory state restore", restore_error)); }
    Ok(value)
}
```

### 6. 开关、默认值、能力探测与回退

- 库层：没有全局开关；调用方显式创建 `PixelUnpackBuffer` 并使用 session。
- 应用：`font_upload_pbo` 默认 `false`；`font_upload_pbo_force_missing` 默认 `false`，用于强制走缺失能力/回退路径。
- 能力探测：ES3 版本 + 13 个入口（`glGenBuffers`、`glBindBuffer`、`glBufferData`、`glGetBufferParameteriv`、`glBufferSubData`、`glDeleteBuffers`、`glPixelStorei`、`glActiveTexture`、`glBindTexture`、`glTexSubImage2D`、`glGetString`、`glGetError`、`glGetIntegerv`）；不支持时返回 `Unsupported`。
- 容量：`MAX_PIXEL_UNPACK_CAPACITY` 名义上限（由 `pixel_unpack_guards` 定义）；应用侧每 painter 预算 4MiB，实际 buffer 容量 1MiB，8 个加载 slot。
- 回退：
  - 创建失败/不支持：调用方走原客户端指针上传。
  - PBO 上传失败或 `force_missing`：用 `with_client_memory_unpack` 保护原路径，保留 RGBA/current/version/shadow。
  - 会话内失败：`finish` 返回 `Err`，调用方必须拒绝发布该纹理版本，不得把半上传状态当作成功。
- 该 API 不建立 fence、不 `map`、不 orphan、不查 clock、不等待 GPU；GPU 读取完成证明是调用方责任。

### 7. 测试与测量（完整数据）

**库层测试**

- `pixel_unpack_guards.rs` 有 6 个测试函数，覆盖容量、版本解析、纹理有效性、cursor 写入/上传范围、重叠/越界、完成/未完成。
- `pixel_unpack.rs` 本身没有 `#[test]`；PBO 事务通过 `font-upload-pbo-tests` 二进制和参考实现的测试日志验证。
- 本地记录：Windows 默认/研究 feature 检查、Android arm64 API26 构建通过。

**应用级质量**

- 22 个质量作业，12 组有效末帧 RGBA 对照：low/normal × coldEarly/dense/unicode/busy/runtime，以及 missing-on；每组 2560×1440、3,686,400 pixels。
- 12 组全部 `changed_pixels=0`、`max_rgba_difference=0`。
- 但 `full_quality_complete=false`，仍有 8 项 native probe pending：实际 `GL_VERSION`/入口/容量、PBO 策略计数、同 slot 重复发布、多矩形、跨 publication 消费、超限/缺失回退、unpack 状态恢复、部分失败回退、Unicode/custom font、retirement 队列等。
- 因此只能写“有限采集帧像素一致”，不能写“PBO 全量画质通过”。

**应用级性能**

同一 SO、真实 Main、normal sample2、first 25s、ABBA 四轮：

| 轮次 | 候选 | 应用墙钟均值 ms | 备注 |
|---|---|---:|---|
| A1 | OFF | 10.393758 | 首轮 |
| B1 | ON | 12.505982 | PBO 开启 |
| B2 | ON | 12.601824 | PBO 开启 |
| A2 | OFF | 12.339533 | 末轮，已漂移 |

- OFF 组均值：11.3666455 ms；ON 组均值：12.5539029 ms。
- ON − OFF：+1.1872573 ms；描述性变化：+10.4451%。
- OFF 自身漂移（A2 − A1）：1.9457750 ms，**大于**组差；因此不能把 +1.187ms 归因于 PBO。
- `all_four_playing_tail_under30ms=false`；完整目标未完成。
- PBO 会话实际发生：B1 167 次、B2 168 次；A1/A2 0 次。发布事件约 171 次/轮。
- 早期单字体 PBO 探针：原序上传 submit p50 130.252ms → 18.643/27.714ms；但同步 completed 132.279ms → 136.740/135.232ms，3 项严格 RGBA 全同；没有整帧收益，未接入。

结论：**有“提交阶段变快”的迹象，但没有稳定的应用级墙钟收益；该 cohort 甚至方向为变慢，且被 OFF 漂移混淆。**

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无（潜在正，无稳定收益）**。

理由：

- API 设计完整，事务/回退/线程/context 保护都比较严格；实现置信度高。
- 质量只完成有限帧对照，native probe 未闭合；不能算完整正优化。
- 应用 ABBA 的 ON 组均值更高，且 OFF 漂移大于组差；不能证明 PBO 导致变慢，也不能证明变快。
- 早期探针显示 submit 变快、completed 不变，说明瓶颈不在提交调用本身。

置信度：实现高；质量中等；性能结论中等（有一次完整四轮 ABBA，但 cohort 有限）。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `4d85ed3`/`d7dc3f4` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有该 API。
- **参考实现：已落地。** `4d85ed3` 是 PBO 主体，`d7dc3f4` 是 +78 行的客户端状态保护；macroquad `ed1db13` 只改 Cargo pin。
- **参考实现默认：关。** 应用侧 `font_upload_pbo=false`；只有研究构建显式开启。
- **是否有调用方：有（研究构建）。** 没有生产调用方默认使用 PBO；PBO 失败会回退到受保护的客户端上传。
- **收益标注：潜在正优化；当前无稳定收益证据。** ABBA 的 ON 组更高，且被 OFF 漂移混淆。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 在字体图集加载期创建固定容量 `PixelUnpackBuffer`，不要每帧创建。
2. 为每个 texture slot 维护“最后一次 PBO 上传的完成证明”；只有证明 GPU 已读完该 buffer 后才能复写。
3. 每次发布：`begin` → `write_slice` → `upload_rgba8_rect` → `finish`；`finish` 返回 `Err` 时丢弃该版本并走保守回退。
4. 任何 raw GL/自定义材质观察点之前先结束 session；不要让 PBO 绑定跨到旧客户端上传路径。
5. 维护 nominally live bytes 与 painter 预算；超限时拒绝新 PBO，走客户端上传。
6. 在真实字体选择器、结算、Unicode、resize/rebuild、上下文切换上做严格 RGBA 与错误计数。

风险：

- **GPU 完成证明**：库不提供 fence；调用方若提前复写 PBO，会读到旧/新混合数据。
- **状态恢复**：PBO 是独立 target，不能走 `GlCache::bind_buffer`；错误路径必须恢复 `PIXEL_UNPACK_BUFFER_BINDING`、alignment、row length、skip 和纹理绑定。
- **线程/context**：创建线程和 context 必须与 `begin`/`delete` 一致；外部 context 消费 owner 时不会删除外部名字，接入方要自己防泄漏。
- **容量/预算**：PBO 容量是名义值，不含驱动 in-flight 存储；不能把 nominal bytes 当物理显存。
- **回退语义**：`force_missing` 只用于研究；生产回退必须保留原 RGBA/版本/shadow，不得发布半成品。
- **不是 EGL 恢复**：该 API 不处理 context loss；context 丢失后不能复用 owner。

### 11. 来源与可追溯性

- miniquad 主体：`4d85ed3fcca28de27c84a20bb14400da8c6a5435`，`pixel_unpack.rs` +484、`pixel_unpack_guards.rs` +212。
- miniquad 保护提交：`d7dc3f484e0f3d6c0642517ef001917fee4cd5dc`，`pixel_unpack.rs` +78/-1。
- macroquad pin：`ed1db13e96e25c2250c0255a42e97631d12a4f7c`，只改 `Cargo.toml`。
- 代码位置：`pixel_unpack.rs:113`（buffer）、`:123`（new）、`:239`（begin）、`:314`（delete）、`:385`（client fallback）、`:453`（session）、`:465`（write）、`:487`（upload）、`:544`（finish）。
- 隔离 diffstat：`4d85ed3` 为 `pixel_unpack.rs` +484、`pixel_unpack_guards.rs` +212、`graphics.rs` +2、`native/gl.rs` +1；`d7dc3f4` 为 `pixel_unpack.rs` +78/-1、`graphics.rs` 1 行修改。
- 数据：`main-normal-font-pbo-abba-v1-execution/results.json`、`font-pbo-main-summaries/main-normal-font-pbo-abba-independent-v1/summary.json`、`font-pbo-chart-quality-v1-results.json`；早期 PBO 探针见 `rendering-research.md` 第 35 行。

## L-04 流缓冲 discard 更新

### 1. 一句话结论与判定

miniquad 为 `Buffer` 增加 `Usage` 元数据和 `update_discard`：对 `stream`/`index_stream` 创建的 buffer，用 `glBufferData(size, null, GL_STREAM_DRAW)` 丢弃旧存储，再用 `glBufferSubData` 写入完整批次。macroquad 的 `cd0ab01` 只是固定依赖，不是实现。

这是**潜在正优化**，目标是让驱动不必等待仍在读取旧 storage 的 queued draw。已有真实 Main ABBA 显示：冷启动首个非零粒子帧的 update 从 22.433ms 降到 1.073ms，但 25 秒整窗应用墙钟反而从 11.4437ms 升到 12.1625ms（+0.7187ms，+6.28%），且 OFF 漂移 1.5935ms 大于组差。判定为**无/负（平均回归，冷帧改善）**。

### 2. 问题与瓶颈

粒子流每帧完整重写一批顶点/索引。原来的 `Buffer::update` 用 `glBufferSubData` 覆盖旧内容；如果 GPU 还在读旧 storage，驱动可能：

- 同步等待旧绘制完成；
- 内部复制一份旧 storage 再写入；
- 在首次非零粒子的冷帧上出现明显 update 峰值。

`update_discard` 用 orphan 思路：`glBufferData(..., null, STREAM_DRAW)` 让驱动丢弃旧 storage，随后写新批次。对象名和绑定保持不变，旧 storage 可由驱动保留给 queued draw 异步消费。

问题是：

- 这只对完整批次重写有意义；如果还要保留旧内容，不能用；
- 驱动仍可能选择同步或分配；
- 冷帧的局部改善不一定转化为整窗收益；
- 粒子绘制不是每帧都有非零实例，统计窗口会被大量零调用稀释。

### 3. 设计思路与不变量

- 只有 `Usage::Stream` 的 buffer 接受 discard；`Immutable`/`Dynamic` 返回 `false`。
- 输入必须非空、`size <= self.size`、`gl_buf != 0`；索引 buffer 的 `T` 必须与 `index_type` 匹配。
- 不改变固定分配大小和 buffer 名字；上传后 batch 之后的字节是未定义的。
- 失败路径在**任何绑定/GL 调用之前**返回 `false`；`true` 只表示输入守卫通过，不表示 GL 分配成功。
- 不查询 GL 错误、不等待 GPU；驱动可能仍然分配或同步。
- 需要保留旧内容时用 `update`，不要用 `update_discard`。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 miniquad `4b26d719a50165d7cd45cb2dc2590b03e6aa09e9`（2026-10-11，“增加流缓冲丢弃更新”），`src/graphics.rs` +123。内容包括：

- `Buffer` 新增 `usage: Usage`；
- `Buffer::immutable` → `Usage::Immutable`，`stream`/`index_stream` → `Usage::Stream`；
- `discard_sizes` 做纯 CPU 守卫；
- `update_discard` 做绑定、`glBufferData`、`glBufferSubData`、恢复绑定；
- 4 个测试覆盖 stream 元数据、非 stream/空/超限、索引宽度、平台 `GLsizeiptr` 溢出。

### 5. 完整核心代码

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Usage {
    Immutable,
    Dynamic,
    Stream,
}

fn gl_usage(usage: &Usage) -> GLenum {
    match usage {
        Usage::Immutable => GL_STATIC_DRAW,
        Usage::Dynamic => GL_DYNAMIC_DRAW,
        Usage::Stream => GL_STREAM_DRAW,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Buffer {
    gl_buf: GLuint,
    buffer_type: BufferType,
    size: usize,
    index_type: Option<IndexType>,
    usage: Usage,
}

impl Buffer {
    fn discard_sizes<T>(&self, data: &[T]) -> Option<(GLsizeiptr, GLsizeiptr)> {
        let size = mem::size_of_val(data);
        if self.usage != Usage::Stream || self.gl_buf == 0 || size == 0 || size > self.size {
            return None;
        }
        if self.buffer_type == BufferType::IndexBuffer
            && self.index_type.map(|kind| kind.size() as usize) != Some(mem::size_of::<T>())
        {
            return None;
        }
        Some((
            GLsizeiptr::try_from(self.size).ok()?,
            GLsizeiptr::try_from(size).ok()?,
        ))
    }

    /// Discard a stream buffer's previous storage and upload a complete batch.
    ///
    /// Only buffers created by `stream` or `index_stream` accept this operation.
    /// Empty, oversized or incompatible index data returns `false` before any
    /// binding or GL call. The fixed allocation size and buffer object name stay
    /// unchanged; bytes after the uploaded batch become undefined. Call `update`
    /// instead if any previous contents must be preserved.
    ///
    /// `true` means the input guards accepted the request, not that GL allocation
    /// succeeded. Like `update`, this does not query GL errors or wait for the GPU.
    /// The driver may still allocate or synchronize; device measurements are needed.
    pub fn update_discard<T>(&self, ctx: &mut Context, data: &[T]) -> bool {
        let (capacity, size) = match self.discard_sizes(data) {
            Some(sizes) => sizes,
            None => return false,
        };
        let gl_target = gl_buffer_target(&self.buffer_type);
        ctx.cache.store_buffer_binding(gl_target);
        ctx.cache.bind_buffer(gl_target, self.gl_buf, self.index_type);
        unsafe {
            glBufferData(gl_target, capacity, std::ptr::null(), GL_STREAM_DRAW);
            glBufferSubData(gl_target, 0, size, data.as_ptr() as *const _);
        }
        ctx.cache.restore_buffer_binding(gl_target);
        true
    }
}
```

### 6. 开关、默认值、能力探测与回退

- 库层：无全局开关；调用方显式调用 `update_discard`。
- 应用：`particle_stream_discard` 默认 `false`；只有研究构建开启。
- 没有能力探测：`glBufferData`/`glBufferSubData` 是 GLES2 基础入口；实现不依赖扩展。
- 回退：`discard_sizes` 返回 `None` 时调用方应回退到 `update`；`true` 只代表提交，GL 错误需要调用方另行检查。
- 与 `update_orphaned` 的区别：`update_orphaned` 在库 `master` 中已经存在，保留完整固定容量，并在 size 为 0 时跳过 `glBufferSubData`；`update_discard` 是参考实现新增的“有守卫的 discard + 必定写入完整批次”版本。两者不要混用语义。

### 7. 测试与测量（完整数据）

**库层测试**

miniquad `graphics.rs` 的 discard 模块有 4 项测试：

- stream 元数据/固定容量在 copy/clone 后保持；
- 非 stream、空、零大小、超限被拒绝；
- 索引宽度匹配，不 panic；
- 平台 `GLsizeiptr` 溢出被拒绝。

应用侧 `particle-discard-tests.log` 另有 10 项探针测试，覆盖：

- discard 选择默认关闭、跨 capture 保持；
- 只统计请求的尝试；
- 零实例与旧 token 的行为；
- 两 slot/溢出/旧 span 不能修改新帧；
- 粒子关闭/legacy/disabled 帧。

**应用级质量**

`particle-discard-quality-v2`：8 个质量作业、4 组对照（low/normal × coldEarly/dense），每组 2560×1440、3,686,400 pixels；全部 `changed_pixels=0`、`max_rgba_difference=0`。

**应用级性能**

同一 SO、真实 Main、normal sample2、first 25s、ABBA 四轮：

| 轮次 | 候选 | 应用墙钟均值 ms | 25s FPS | 说明 |
|---|---|---:|---:|---|
| A1 | OFF | 10.646958 | 59.612489 | 首轮 |
| B1 | ON | 12.191961 | 44.450670 | discard 开启 |
| B2 | ON | 12.132952 | 44.593312 | discard 开启 |
| A2 | OFF | 12.240480 | 44.693281 | 末轮，已漂移 |

- OFF 组均值：11.443719ms；ON 组均值：12.162457ms。
- ON − OFF：+0.718738ms；描述性变化：+6.2806%。
- OFF 自身漂移（A2 − A1）：1.593522ms，**大于**组差；不能把 +0.7187ms 归因于 discard。
- 冷启动事件：首个非零粒子帧的下一次 update，OFF 两轮为 22.185416/22.680728ms（均值 22.433072ms），ON 两轮为 1.129166/1.017709ms（均值 1.073438ms）。这是局部改善。
- 但整窗 `whole_playing_update_sum_mean`：OFF 0.081427ms，ON 0.086482ms；update 总量并没有下降。
- `all_on_playing_tails_under30ms=false`；`candidate_default_promotion_supported=false`；结论是 `retain_research_only_pending_broader_evidence`。
- 实际执行：ON 两轮各 945 次 discarded calls（每 slot 945），OFF 0 次；说明候选确实执行，不是“开关没生效”。

结论：**冷帧峰值明显下降，但整窗平均墙钟和 FPS 没有收益，甚至出现回归；该 cohort 还受 OFF 末轮漂移影响。**

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无/负（平均回归，冷帧改善）**。

理由：

- 机制对“首个非零粒子帧”有效：22.43ms → 1.07ms 的局部改善很明确。
- 25 秒整窗没有收益：ON 组均值更高，update 总量也略高；OFF 漂移大于组差，无法把回归稳定归因，但也不能声称正收益。
- 质量对照全同，说明在当前采集帧上没有画质代价。
- 该实现是“有守卫的完整批次 discard”，不适合保留旧内容的更新；接入面窄。

置信度：机制与局部数据高；整窗性能结论中等（四轮有限，漂移大）；不能外推到全曲/所有 GPU。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `4b26d71` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有 `update_discard`。
- **参考实现：已落地。** `4b26d71` 的隔离改动只有 `graphics.rs` +123；macroquad `cd0ab01` 只改 Cargo pin。
- **参考实现默认：关。** 应用 `particle_stream_discard=false`；只有研究构建开启。
- **是否有调用方：有（研究构建）。** 没有生产默认调用方；普通粒子路径仍用 `update`/`update_orphaned`。
- **收益标注：潜在正优化（仅冷帧局部）；当前无稳定收益证据，整窗平均回归。**

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在“完整批次重写、旧内容不需要保留”的粒子流 buffer 上使用；不要用于部分更新或需要读回旧内容的 buffer。
2. 确认 buffer 由 `stream`/`index_stream` 创建，容量足够；索引类型与 `T` 匹配。
3. 在 draw 前调用 `update_discard`；如果返回 `false`，回退到 `update`。
4. 检查 `GL_STREAM_DRAW` 的 orphan 是否在目标驱动上真的避免同步；用同 SO 的 25s/全曲 ABBA，而不是只看冷帧。
5. 在粒子关闭、零实例、低画质、不同 sampleCount 下分别验证；不要把零调用帧混入“改善”统计。

风险：

- **旧 storage 生命周期**：orphan 后旧 storage 由驱动保留；如果驱动选择等待，收益消失。
- **内容未定义**：batch 之后的字节未定义；任何依赖旧尾部的代码都会读到垃圾。
- **索引宽度**：类型不匹配直接返回 `false`；调用方不能忽略返回值。
- **GL 错误**：函数不检查错误；必须在 session/帧末统一检查，否则会把失败当成功。
- **统计偏差**：冷帧局部改善很容易被误报成整窗收益；必须报告全窗口、漂移和 update 总量。

### 11. 来源与可追溯性

- miniquad 实现：`4b26d719a50165d7cd45cb2dc2590b03e6aa09e9`，`src/graphics.rs` +123。
- macroquad pin：`cd0ab01b23998c113312eabffc8a4ba0dc65e740`，只改 `Cargo.toml`。
- 代码位置：`src/graphics.rs:1926`（Usage）、`:1948`（Buffer）、`:2069`（discard_sizes）、`:2096`（update_discard）。
- 隔离 diffstat：`src/graphics.rs` 1 文件 +123。
- 数据：`particle-discard-quality-v2-results.json`；`main-discard-abba-summaries/main-normal-discard-abba-v1/summary.json` 与 `independent-conclusion.json`；`particle-discard-tests.log`。

## L-05 uniform 按位缓存

### 1. 一句话结论与判定

miniquad 为每个 shader program 的每个 uniform 字段保存一份**按位**的字节缓存；重复值不再调用 `glUniform*`，数组按完整 range 校验，可能别名/重复名字的布局回退到原上传。默认关闭，`set_uniform_cache_enabled` 是 unsafe，因为启用后所有 uniform 写入必须经过该 context，raw GL 写入后必须显式 invalidate。

实测：packed uniform 调用减少约 67%–69%，字节减少约 67%；但完整应用墙钟和 FPS 没有稳定收益，部分轮次反而略差。判定为**无（调用降 67%–69%，墙钟无收益）**。macroquad 侧没有对应实现提交；`a8aace0` 只是把依赖 pin 到该 miniquad 提交。

### 2. 问题与瓶颈

macroquad/Phira 的 shader 每帧重复写入大量相同 uniform：Projection、Model、_Time 的部分分量、材质常量等。每次 `glUniform*` 都是一次 API 调用，可能：

- 在驱动里做参数校验/状态更新；
- 触发命令缓冲增长；
- 在移动 GPU 上产生不必要的 CPU 开销。

缓存“每个 program/field 上一次写入的精确字节”，如果本次字节完全相同就跳过调用，理论上能减少 API 调用和命令量。但：

- uniform 调用本身可能不是瓶颈；真正成本在 draw/纹理/驱动等待；
- 缓存查找、字节比较、range 校验也有成本；
- 跳过调用只减少 CPU 提交，不减少 GPU 采样/像素工作；
- 如果 UI 每帧变化的 uniform 很多，命中率会下降。

### 3. 设计思路与不变量

- **按位比较，不做浮点近似**：`+0.0` 与 `-0.0`、不同 NaN payload 都视为不同值；这避免“看起来相等但 shader 结果不同”。
- 每个 `ShaderUniform` 保存 `Value { bytes: Vec<u8>, valid: bool }`；`matches` 要求 `valid && bytes == input`。
- 数组按 `_offset`、`_size`、`array_count` 计算完整 byte range；range 越界/溢出直接 panic，避免部分数组缓存造成状态不一致。
- 缓存预算 4MiB 名义值；字段已有存储时不受预算限制（避免永远无法缓存大字段）；新字段超过预算则本次不缓存，但正常上传。
- 只有 `uniform_cache_eligible` 的 shader 才缓存：uniform 名字与 image 名字必须互不重复、且不含 `[`；否则可能数组别名/sampler 别名，回退原上传。
- 启用后，managed uniform 必须通过该 context 写入；raw GL 写入后调用 `invalidate_uniform_cache`；raw program relink 还必须重建 location 元数据。
- 新 context 默认 `uniform_cache_enabled=false`；`set_uniform_cache_enabled` 先 invalidate 再改开关。
- `Shader::delete` 会从 `cached_bytes` 中扣除该 shader 的 uniform 存储，避免预算泄漏。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 miniquad `9466727421b21cad00e63231d9d7dbf4a68af426`（2026-10-07，“研究uniform重复值消除”）：

- `src/graphics/uniform_cache.rs` +91：`UniformUploadStats`、`Value`、`byte_range`、`names_disjoint`、测试。
- `src/graphics.rs` +79/-14：`ShaderUniform.value`、`ShaderInternal.uniform_cache_eligible`、`GraphicsContext.uniform_cache_enabled` / `uniform_uploads`、setter/invalidate/stats、`apply_uniforms_from_bytes` 改造、shader 加载时 eligibility 计算。

macroquad 侧没有对应实现提交；`a8aace0947640c867898239bba3c4f14b6f5f7c0` 只把 miniquad rev 固定到 `9466727`，不是实现。

### 5. 完整核心代码

`uniform_cache.rs` 全文如下：

```rust
//! Exact uniform-value reuse. No floating-point comparison or approximation.
use std::{collections::HashSet, ops::Range};

#[derive(Clone, Copy, Debug, Default)]
pub struct UniformUploadStats {
    pub enabled: bool,
    pub issued_calls: u64,
    pub issued_bytes: u64,
    pub skipped_calls: u64,
    pub skipped_bytes: u64,
    pub uncached_fields: u64,
    pub cached_bytes: usize,
}

#[derive(Debug, Default)]
pub(super) struct Value {
    bytes: Vec<u8>,
    valid: bool,
}
impl Value {
    pub fn matches(&self, bytes: &[u8]) -> bool { self.valid && self.bytes == bytes }
    pub fn storage_len(&self) -> usize { self.bytes.len() }
    pub fn remember(&mut self, bytes: &[u8]) {
        self.bytes.clear(); self.bytes.extend_from_slice(bytes); self.valid = true;
    }
    pub fn invalidate(&mut self) { self.valid = false; }
}

pub(super) fn byte_range(offset: usize, unit: usize, count: usize, size: usize) -> Option<Range<usize>> {
    let end = offset.checked_add(unit.checked_mul(count)?)?;
    (end <= size).then_some(offset..end)
}

// Indexed names may partially alias another descriptor's array. Sampler
// assignments are made by apply_bindings outside the packed uniform block.
// Preserve ordinary uploads for ambiguous layouts, even when caching is on.
pub(super) fn names_disjoint<'a>(uniforms: impl Iterator<Item=&'a str>, images: impl Iterator<Item=&'a str>) -> bool {
    let mut names = HashSet::new();
    for name in uniforms.chain(images) {
        if name.contains('[') || !names.insert(name) { return false; }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_array_bytes_and_overflow_are_checked() {
        assert_eq!(byte_range(128,16,32,640),Some(128..640));
        assert_eq!(byte_range(128,16,32,639),None);
        assert_eq!(byte_range(usize::MAX,4,1,usize::MAX),None);
        assert_eq!(byte_range(0,64,usize::MAX,usize::MAX),None);
    }
    #[test]
    fn names_that_can_alias_use_original_uploads() {
        assert!(names_disjoint(["Model","Pose"].iter().copied(),["Texture"].iter().copied()));
        for names in [["Model","Model"],["Pose","Pose[1]"],["Pose[0]","Other"],["Texture","Other"]] {
            assert!(!names_disjoint(names.iter().copied(),["Texture"].iter().copied()));
        }
    }
    #[test]
    fn independent_programs_match_an_unconditional_driver_oracle() {
        let mut memo: Vec<Vec<Value>>=(0..3).map(|_|(0..4).map(|_|Value::default()).collect()).collect();
        let mut actual=vec![vec![Vec::<u8>::new();4];3];let mut oracle=actual.clone();
        let mut seed=0x1234abcd_u32;let mut skipped=0;
        for round in 0..2400 {
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let program=(seed as usize)%3;let field=((seed>>8) as usize)%4;
            let len=[4,16,64,512][field];
            let data=if round%4!=0 && !oracle[program][field].is_empty() {
                oracle[program][field].clone()
            } else {(0..len).map(|i|seed.wrapping_add(i as u32*17) as u8).collect()};
            if round%137==0 { for p in &mut memo {for v in p {v.invalidate();}} }
            oracle[program][field]=data.clone();
            if memo[program][field].matches(&data) {skipped+=1;} else {
                actual[program][field]=data.clone();memo[program][field].remember(&data);
            }
            assert_eq!(actual,oracle,"program state at {round}");
        }
        assert!(skipped>1000);
    }
    #[test]
    fn signed_zero_nan_payloads_and_raw_write_invalidation_remain_distinct() {
        let mut v=Value::default();
        for bits in [0_u32,0x80000000,0x7fc00001,0x7fc00002,0x7fc00001] {
            let bytes=bits.to_ne_bytes();assert!(!v.matches(&bytes));v.remember(&bytes);
            assert!(v.matches(&bytes));v.invalidate();assert!(!v.matches(&bytes));v.remember(&bytes);
        }
    }
}
```

`GraphicsContext` 的开关与统计：

```rust
    /// Enable exact per-program uniform reuse. Disabled on a new context.
    /// GLES2 uniform lifetime rules suffice; no extension or format change.
    ///
    /// # Safety
    /// While enabled, managed shader uniforms must be written through this
    /// context. After raw GL writes invalidate before applying uniforms again.
    /// A raw program relink also requires rebuilt Shader location metadata.
    pub unsafe fn set_uniform_cache_enabled(&mut self, enabled: bool) {
        self.invalidate_uniform_cache();
        self.uniform_cache_enabled = enabled;
    }

    /// Forget cached values without clearing actual program state or waiting.
    pub fn invalidate_uniform_cache(&mut self) {
        for shader in &mut self.shaders {
            for uniform in &mut shader.uniforms { uniform.value.invalidate(); }
        }
    }

    pub fn uniform_upload_stats(&self) -> UniformUploadStats {
        let mut stats = self.uniform_uploads;
        stats.enabled = self.uniform_cache_enabled;
        stats
    }
```

`apply_uniforms_from_bytes` 的完整缓存分支：

```rust
    #[doc(hidden)]
    /// Apply uniforms data from array of bytes with very special layout.
    /// Hidden because `apply_uniforms` is the recommended and safer way to work with uniforms.
    pub fn apply_uniforms_from_bytes(&mut self, uniform_ptr: *const u8, size: usize) {
        let pip = &self.pipelines[self.cache.cur_pipeline.unwrap().0];
        let shader = &mut self.shaders[pip.shader.0];

        let eligible = self.uniform_cache_enabled && shader.uniform_cache_eligible;
        for uniform in &mut shader.uniforms {
            use UniformType::*;

            let count = usize::try_from(uniform.array_count).expect("Uniform count exceeds GLsizei");
            let range = uniform_cache::byte_range(uniform._offset, uniform._size, count, size)
                .expect("Uniforms struct does not match full shader array layout");
            unsafe {
                let data = uniform_ptr.add(range.start) as *const f32;
                let data_int = uniform_ptr.add(range.start) as *const i32;
                if let Some(gl_loc) = uniform.gl_loc {
                    let bytes = std::slice::from_raw_parts(uniform_ptr.add(range.start), range.len());
                    let had_storage = uniform.value.storage_len() == bytes.len();
                    let within_budget = had_storage || self.uniform_uploads.cached_bytes
                        .checked_add(bytes.len()).map_or(false, |n| n <= 4 * 1024 * 1024);
                    let cache_this = eligible && within_budget;
                    if cache_this && uniform.value.matches(bytes) {
                        self.uniform_uploads.skipped_calls += 1;
                        self.uniform_uploads.skipped_bytes += bytes.len() as u64;
                        continue;
                    }
                    self.uniform_uploads.issued_calls += 1;
                    self.uniform_uploads.issued_bytes += bytes.len() as u64;
                    if self.uniform_cache_enabled && !cache_this { self.uniform_uploads.uncached_fields += 1; }
                    match uniform.uniform_type {
                        Float1 => {
                            glUniform1fv(gl_loc, uniform.array_count, data);
                        }
                        Float2 => {
                            glUniform2fv(gl_loc, uniform.array_count, data);
                        }
                        Float3 => {
                            glUniform3fv(gl_loc, uniform.array_count, data);
                        }
                        Float4 => {
                            glUniform4fv(gl_loc, uniform.array_count, data);
                        }
                        Int1 => {
                            glUniform1iv(gl_loc, uniform.array_count, data_int);
                        }
                        Int2 => {
                            glUniform2iv(gl_loc, uniform.array_count, data_int);
                        }
                        Int3 => {
                            glUniform3iv(gl_loc, uniform.array_count, data_int);
                        }
                        Int4 => {
                            glUniform4iv(gl_loc, uniform.array_count, data_int);
                        }
                        Mat4 => {
                            glUniformMatrix4fv(gl_loc, uniform.array_count, 0, data);
                        }
                    }
                    if cache_this {
                        if !had_storage { self.uniform_uploads.cached_bytes += bytes.len(); }
                        uniform.value.remember(bytes);
                    }
                }
            }
        }
    }
```

### 6. 开关、默认值、能力探测与回退

- 默认关闭：`GraphicsContext` 初始化 `uniform_cache_enabled: false`；新 context 不会自动开启。
- 开启 API 是 `unsafe fn set_uniform_cache_enabled(&mut self, enabled: bool)`；开启前会 invalidate 全部缓存。
- 应用研究 flag `uniform_cache` 默认 `false`；只有同一 SO 的 A/B 构建开启。
- 能力探测：不需要扩展；GLES2 uniform 生命周期足够。ELIGIBILITY 是 shader 元数据层面的检查，不是硬件能力。
- 回退：
  - shader 名字可能别名（重复、含 `[`、与 image 同名）：`uniform_cache_eligible=false`，该 shader 全部按原路径上传。
  - 字段超出 4MiB 预算且没有已有存储：`cache_this=false`，本次正常上传，不缓存。
  - raw GL 写入：调用方必须 `invalidate_uniform_cache`；否则缓存会与真实 program 状态不一致。
  - raw relink：必须重建 location 元数据；否则缓存的 `gl_loc` 和 range 都可能失效。

### 7. 测试与测量（完整数据）

**库层测试**

`uniform-library-tests.log`：4 项测试通过：

- 完整数组字节与溢出检查；
- 可别名名字回退；
- 3 programs × 4 fields × 2400 次操作对照无条件驱动 oracle；
- `-0.0`、NaN payload、raw 写入 invalidate 的按位区分。

**应用级质量**

`uniform-v1-quality`：10 个作业、6 组有效末帧 RGBA 对照，全部 `changed_pixels=0`；覆盖 CPU 固定/动态、GPU 动态、高画质动态，以及 raw Projection 写坏后 invalidate 的 CPU/GPU 两组。GPU 原参数化坐标仍被分类为“轻量有损待验证”，不能因为 uniform 字节缓存相同就升级整体画质分类。

**应用级性能**

同一 SO、120 预热 + 1200 采样、八轮交错（CPU 外侧 ABBA，GPU 内侧 BAAB）：

| 次序 | 消费者/缓存 | 应用 FPS | 应用间隔 p99 ms | 完整墙钟均值 ms | packed 发出/跳过 calls | packed 发出/跳过 bytes |
|---|---|---|---:|---:|---|---|
| 0 | CPU 关 | 59.752 | 19.159 | 6.149 | 100574/0 | 6141056/0 |
| 1 | CPU 开 | 59.673 | 19.619 | 6.313 | 31001/69557 | 1983840/4156192 |
| 2 | GPU 开 | 59.750 | 19.616 | 6.225 | 34562/76320 | 3316928/6670704 |
| 3 | GPU 关 | 59.656 | 21.355 | 6.140 | 110874/0 | 9984528/0 |
| 4 | GPU 关 | 59.751 | 19.087 | 6.316 | 110886/0 | 9990480/0 |
| 5 | GPU 开 | 59.702 | 19.736 | 6.278 | 34560/76298 | 3315952/6664960 |
| 6 | CPU 开 | 59.702 | 19.469 | 6.118 | 31007/69571 | 1984224/4157088 |
| 7 | CPU 关 | 59.764 | 19.308 | 6.148 | 100582/0 | 6141568/0 |

计算：

- CPU 开：69557 / (31001 + 69557) = **69.17%** skipped calls；4156192 / (1983840 + 4156192) = **67.69%** skipped bytes。
- GPU 开（第一轮）：76320 / (34562 + 76320) = **68.83%**；6670704 / (3316928 + 6670704) = **66.78%**。
- GPU 开（第二轮）：76298 / (34560 + 76298) = **68.82%**；6664960 / (3315952 + 6664960) = **66.77%**。
- CPU 开完整墙钟均值约 6.215ms，CPU 关约 6.149ms；GPU 开约 6.251ms，GPU 关约 6.228ms。没有稳定收益，部分轮次略差。
- FPS 在 59.656–59.764 之间，差异小于漂移；不能声称帧率提升。
- 4/8 轮仍有 33.46–33.50ms 最大间隔；不能声称尖峰消除。
- 电量前后 30–33℃，未记录 CPU 频率；不能把差异归因于缓存或降频。

结论：**调用数确实下降约 69%，但墙钟没有稳定收益；这正是“减少 API 调用 ≠ 减少帧时间”的样本。**

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无（调用降 67%–69%，墙钟无收益）**。

理由：

- 按位缓存和数组 range 校验实现正确，测试覆盖了别名、NaN、-0、raw invalidate；实现置信度高。
- 调用/字节减少是真实的，但完整应用墙钟与 FPS 没有稳定改善。
- 说明瓶颈不在 packed uniform 调用数量，或缓存查找/比较抵消了收益。
- 质量对照全同，说明当前采集帧没有画质代价；但“轻量有损”分类仍待验证。

置信度：实现高；调用统计高；性能结论高（八轮同 SO 交错）；外推到其他 shader/GPU 低。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `9466727` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有该缓存。
- **参考实现：已落地。** macroquad 侧没有对应实现提交；`a8aace0` 只是 pin，只改 `Cargo.toml`。
- **参考实现默认：关。** `GraphicsContext` 初始化 `uniform_cache_enabled=false`；开启走 unsafe 的 `set_uniform_cache_enabled`；应用 `uniform_cache=false`。
- **是否有调用方：有（研究构建）。** 没有生产默认调用方；普通路径仍每次调用 `glUniform*`。
- **收益标注：无（调用降 67%–69%，墙钟无收益）。** `Shader::delete` 已经处理缓存字节扣除；但 context loss/完整 shader 生命周期仍未闭合。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在 managed shader 上开启；所有 uniform 写入统一走 `apply_uniforms`/`apply_uniforms_from_bytes`。
2. 在任何 raw GL uniform 写入后调用 `invalidate_uniform_cache`；raw relink 后不要复用旧 shader 元数据。
3. 为每个 shader 检查 `uniform_cache_eligible`；如果名字含 `[`、重复或与 image 同名，接受“不缓存”的回退。
4. 监控 `UniformUploadStats`：issued/skipped calls、bytes、uncached_fields、cached_bytes；不要只看 skipped。
5. 做同 SO ABBA，报告完整墙钟、p99、最大间隔，而不是只报调用数。
6. 在 UI/结算/字体选择器等 uniform 变化频繁的场景单独验证；命中率低时不要开启。

风险：

- **状态一致性**：raw GL 写入或 relink 会让缓存与真实 program 不一致；这是 unsafe API 的核心契约。
- **别名布局**：数组 uniform 可能部分重叠；`names_disjoint` 只做保守名字检查，不能证明所有驱动布局都安全。
- **预算**：4MiB 是名义值；`cached_bytes` 只统计字段存储，不含 Vec 容量/分配器开销。
- **性能反直觉**：缓存查找/字节比较可能抵消调用节省；必须以墙钟为准。
- **生命周期**：shader 删除必须同步扣除缓存字节；否则预算会泄漏并导致后续字段无法缓存。
- **画质分类**：uniform 字节相同不等于 shader 输出相同；GPU 参数化坐标仍被标为待验证。

### 11. 来源与可追溯性

- miniquad 实现：`9466727421b21cad00e63231d9d7dbf4a68af426`，`graphics.rs` +79/-14、`uniform_cache.rs` +91。
- macroquad pin：`a8aace0947640c867898239bba3c4f14b6f5f7c0`，只改 `Cargo.toml`。
- 代码位置：`src/graphics/uniform_cache.rs` 全文；`src/graphics.rs:1015`（setter）、`:1021`（invalidate）、`:1027`（stats）、`:1040`（apply）。
- 隔离 diffstat：`src/graphics.rs` +79/-14、`src/graphics/uniform_cache.rs` +91。
- 数据：`uniform-v1-performance-results.json`、`uniform-v1-quality-results.json`、`uniform-library-tests.log`；README 第 253–276 行的表格与结论。

## L-06 资源标识、生命周期与绑定缓存清理

### 1. 一句话结论与判定

miniquad 把 shader/pipeline 从 `Vec` 下标改成带 serial 的 arena 句柄，新增 `Shader::delete`、`Pipeline::delete`、`ShaderResourceStats` 和 `clear_resource_bindings`；同时修复 shader 编译/链接失败时的 GL 对象清理，并在分配任何 GL 对象之前校验 shader 元数据名字。

这不是性能优化，而是**正确性与内存治理**：防止 stale/foreign 句柄误指新资源、防止 shader 删除后旧 pipeline 仍引用、防止 GL 名字复用命中旧缓存。判定为**无优化（正确性/内存治理，非性能）**，置信度高。

### 2. 问题与瓶颈

原有实现：

- `Shader(usize)`、`Pipeline(usize)` 是 `Vec` 下标；删除/复用后，旧句柄可能指向新资源。
- shader 创建失败时可能泄漏 program/shader object；link 失败时日志长度处理不够稳健。
- shader/pipeline 没有统一 retire API；删除后 `cur_pipeline` / `cur_program` 缓存可能悬空。
- GL buffer/texture 名字删除后立即可复用，旧 VAO/attribute/sampler 缓存可能命中同名新对象。

这些问题不会直接体现在 FPS，但会造成：

- 难复现的渲染错误/驱动崩溃；
- 资源泄漏（program、shader object、uniform 缓存字节）；
- 删除/重建后的 stale 绑定；
- 接入方无法安全做 buffer 扩容与 PBO/纹理替换。

### 3. 设计思路与不变量

- **非回绕身份**：每个 arena slot 有 `serial`，由全局 `AtomicUsize` 单调递增；句柄是 `{ slot, serial }`。slot 复用但 serial 不同，旧句柄永远不能再命中。
- **foreign context 拒绝**：每个 `GraphicsContext` 有自己的 `context_id` 和独立 arena；另一个 context 的句柄在本 context 查不到。
- **pipeline 引用检查**：`Shader::delete` 拒绝删除仍被任何 managed pipeline 引用的 shader；必须先删 pipeline。
- **删除即失效**：`Shader::delete` 清空 `cur_pipeline`、`glUseProgram(0)`、删除 program 和两个 shader object；`Pipeline::delete` 在删除当前 pipeline 时清空 `cur_pipeline`。
- **缓存字节回收**：shader 删除时从 `uniform_uploads.cached_bytes` 扣除该 shader 的 uniform 存储，避免预算泄漏。
- **绑定清理是调用方契约**：`clear_resource_bindings` 不自动在每次 delete 时调用；调用方必须在消费完 deferred commands、删除/复用 raw buffer/texture 之前调用。
- **无 GPU 等待**：delete/clear 不插入 `glFinish`；GPU 仍可能异步持有已提交工作。
- **shader 名称预校验**：在分配 GL 对象之前把所有 uniform/image 名字转成 `CString`；失败则不创建任何 GL 对象，避免 unwind 泄漏。
- **失败清理**：编译失败删除 shader；link 失败删除 program 和两个 shader；日志长度用 `max(0)` 和 `min(len)` 限界，不再 `assert!(max_length >= 1)`。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 miniquad，三个隔离提交：

- `406954d8189bfeb55f375ee8ab2eafd325405bbf`（2026-10-07，“管理着色器资源生命周期”）：`graphics.rs` +111/-25、`resource_arena.rs` +175。
- `b7e1d66f28269c631b3bb9d847f9ecce7ac8b87b`（2026-10-07，“预先校验着色器名称”）：`graphics.rs` +6。
- `1dfaa933c642bc1b0ef3a27701a941b20ee084c1`（2026-10-07，“清理资源绑定缓存”）：`graphics.rs` +17。

macroquad 侧：

- `9c5d1c1a7e43d4aa1b2c51140d61bc511dfaf9b6` 先 pin 到 `b7e1d66`；
- `c862fc8545593341c644a53ee72647d196c5b6c1` 再 pin 到 `1dfaa93`；
- 两者都只改 `Cargo.toml`，不是实现。

数据结构：

- `Handle { slot: usize, serial: usize }`。
- `Slot<T> { serial, value: Option<T> }`。
- `Arena<T> { slots: Vec<Slot<T>>, free: Vec<usize>, live: usize }`。
- `Shader(Handle)`、`Pipeline(Handle)`。
- `ShaderInternal { program, objects: [GLuint; 2], images, uniforms, uniform_cache_eligible }`。
- `ShaderResourceStats { shaders, pipelines, shader_slots, pipeline_slots }`。

算法：

1. `Arena::insert` 从 free list 取 slot 或 push 新 slot，生成新 serial；`remove` 校验 serial，取走 value，归还 slot；`get` 同时校验 slot 范围和 serial。
2. `GraphicsContext` 创建时生成 `context_id`，初始化两个 arena。
3. `Shader::delete` 先扫描 pipelines 的 `shader.0`，有引用就返回 `false`；否则 remove shader，扣除 uniform bytes，清 `cur_pipeline`，`glUseProgram(0)`，删除 program 和 objects。
4. `Pipeline::delete` remove pipeline；如果它是当前 pipeline，清 `cur_pipeline`。
5. `clear_resource_bindings` 禁用并清空 attribute 缓存，清 buffer/texture 绑定缓存，归零 stored 逻辑值。
6. `load_shader_internal` 在分配 GL 对象前遍历所有 metadata 名字做 `CString::new`；link 失败时删除 program 和两个 shader；编译失败时删除 shader。
7. `load_shader` 先构造 `CString`，再 `glCreateShader`；失败时不再泄漏 GL 对象。

### 5. 完整核心代码

`resource_arena.rs` 全文：

```rust
//! Reusable resource slots with non-wrapping identities. A stale or foreign
//! handle must never silently identify a different shader/pipeline.
use std::ops::{Index, IndexMut};
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(1);
pub(super) fn unique_id() -> usize {
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_add(1)
    })
    .expect("graphics resource identity exhausted")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Handle {
    pub slot: usize,
    serial: usize,
}
struct Slot<T> {
    serial: usize,
    value: Option<T>,
}
pub(super) struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
    live: usize,
}
impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }
}
impl<T> Arena<T> {
    pub fn insert(&mut self, value: T) -> Handle {
        let serial = unique_id();
        let slot = if let Some(slot) = self.free.pop() {
            self.slots[slot] = Slot {
                serial,
                value: Some(value),
            };
            slot
        } else {
            let slot = self.slots.len();
            self.slots.push(Slot {
                serial,
                value: Some(value),
            });
            slot
        };
        self.live += 1;
        Handle { slot, serial }
    }
    pub fn get(&self, handle: Handle) -> Option<&T> {
        let slot = self.slots.get(handle.slot)?;
        (slot.serial == handle.serial)
            .then_some(slot.value.as_ref())
            .flatten()
    }
    fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.slot)?;
        if slot.serial == handle.serial {
            slot.value.as_mut()
        } else {
            None
        }
    }
    pub fn remove(&mut self, handle: Handle) -> Option<T> {
        let slot = self.slots.get_mut(handle.slot)?;
        if slot.serial != handle.serial {
            return None;
        }
        let value = slot.value.take()?;
        self.live -= 1;
        self.free.push(handle.slot);
        Some(value)
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| slot.value.as_ref())
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|slot| slot.value.as_mut())
    }
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn slots_len(&self) -> usize {
        self.slots.len()
    }
}
impl<T> Index<Handle> for Arena<T> {
    type Output = T;
    fn index(&self, handle: Handle) -> &T {
        self.get(handle)
            .expect("stale or foreign graphics resource")
    }
}
impl<T> IndexMut<Handle> for Arena<T> {
    fn index_mut(&mut self, handle: Handle) -> &mut T {
        self.get_mut(handle)
            .expect("stale or foreign graphics resource")
    }
}
```

shader 的删除与资源统计：

```rust
#[derive(Clone, Debug, Copy)]
pub struct Shader(Handle);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaderResourceStats {
    pub shaders: usize,
    pub pipelines: usize,
    pub shader_slots: usize,
    pub pipeline_slots: usize,
}

impl Shader {
    /// Returns None for a retired handle or a handle from another context.
    pub fn gl_internal_id(&self, ctx: &Context) -> Option<GLuint> {
        ctx.shaders.get(self.0).map(|shader| shader.program)
    }

    /// Retire only after all caller-owned deferred commands have been consumed.
    /// Refuses retirement while a managed pipeline references this shader.
    /// Invalidates the current program/pipeline binding; apply a live pipeline
    /// before subsequent direct draws. No GPU finish/wait is inserted.
    pub fn delete(&self, ctx: &mut Context) -> bool {
        if ctx.pipelines.iter().any(|pipeline| pipeline.shader.0 == self.0) { return false; }
        let Some(shader) = ctx.shaders.remove(self.0) else { return false; };
        let bytes: usize = shader.uniforms.iter().map(|uniform| uniform.value.storage_len()).sum();
        ctx.uniform_uploads.cached_bytes = ctx.uniform_uploads.cached_bytes.saturating_sub(bytes);
        ctx.cache.cur_pipeline = None;
        unsafe {
            glUseProgram(0);
            glDeleteProgram(shader.program);
            for object in shader.objects { glDeleteShader(object); }
        }
        true
    }
}

struct ShaderInternal {
    program: GLuint,
    objects: [GLuint; 2],
    images: Vec<ShaderImage>,
    uniforms: Vec<ShaderUniform>,
    uniform_cache_eligible: bool,
}
```

pipeline 删除：

```rust
    /// Consume caller-owned deferred commands before retiring their pipeline.
    /// Old handles cannot alias a reused slot. Returns false if already retired
    /// or if this handle belongs to another managed context.
    pub fn delete(&self, ctx: &mut Context) -> bool {
        if ctx.pipelines.remove(self.0).is_none() { return false; }
        if ctx.cache.cur_pipeline.map(|pipeline| pipeline.0) == Some(self.0) {
            ctx.cache.cur_pipeline = None;
        }
        true
    }
```

context 身份、资源统计与绑定清理：

```rust
    /// Identity of this managed context, not an EGL context-loss detector.
    pub fn context_id(&self) -> usize { self.context_id }

    /// Managed resource occupancy. GL may retire submitted work asynchronously;
    /// these counts do not claim physical driver memory has been reclaimed.
    pub fn shader_resource_stats(&self) -> ShaderResourceStats {
        ShaderResourceStats { shaders: self.shaders.len(), pipelines: self.pipelines.len(),
            shader_slots: self.shaders.slots_len(), pipeline_slots: self.pipelines.slots_len() }
    }

    /// Call after consuming deferred commands and before deleting/reusing raw
    /// buffers or textures. GL name reuse must not match obsolete cached VAO or
    /// sampler bindings. Does not introduce a render-pass boundary or GPU wait.
    pub fn clear_resource_bindings(&mut self) {
        for (index, attribute) in self.cache.attributes.iter_mut().enumerate() {
            if attribute.take().is_some() {
                unsafe { glDisableVertexAttribArray(index as GLuint); }
            }
        }
        self.cache.clear_buffer_bindings();
        self.cache.clear_texture_bindings();
        self.cache.stored_vertex_buffer = 0;
        self.cache.stored_index_buffer = 0;
        self.cache.stored_index_type = None;
        self.cache.stored_texture = 0;
    }
```

shader 名称预校验与失败清理（`b7e1d66` + `406954d`）：

```rust
fn load_shader_internal(
    vertex_shader: &str,
    fragment_shader: &str,
    meta: ShaderMeta,
) -> Result<ShaderInternal, ShaderError> {
    // Metadata names must be checked before any GL object is allocated.
    // get_uniform_location can then construct its CString without an unwind leak.
    for name in meta.images.iter().map(String::as_str)
        .chain(meta.uniforms.uniforms.iter().map(|uniform| uniform.name.as_str())) {
        CString::new(name)?;
    }
    unsafe {
        let vertex_shader = load_shader(GL_VERTEX_SHADER, vertex_shader)?;
        let fragment_shader = match load_shader(GL_FRAGMENT_SHADER, fragment_shader) {
            Ok(shader) => shader,
            Err(error) => { glDeleteShader(vertex_shader); return Err(error); }
        };

        let program = glCreateProgram();
        glAttachShader(program, vertex_shader);
        glAttachShader(program, fragment_shader);
        glLinkProgram(program);

        let mut link_status = 0;
        glGetProgramiv(program, GL_LINK_STATUS, &mut link_status as *mut _);
        if link_status == 0 {
            let mut max_length: i32 = 0;
            glGetProgramiv(program, GL_INFO_LOG_LENGTH, &mut max_length as *mut _);

            let mut error_message = vec![0u8; max_length.max(0) as usize + 1];
            glGetProgramInfoLog(
                program,
                max_length,
                &mut max_length as *mut _,
                error_message.as_mut_ptr() as *mut _,
            );
            let used = (max_length.max(0) as usize).min(error_message.len());
            let error_message = std::string::String::from_utf8_lossy(&error_message[..used])
                .trim_end_matches('\0').to_owned();
            glDeleteProgram(program); glDeleteShader(vertex_shader); glDeleteShader(fragment_shader);
            return Err(ShaderError::LinkError(error_message));
        }

        glUseProgram(program);

        #[rustfmt::skip]
        let images = meta.images.iter().map(|name| ShaderImage {
            gl_loc: get_uniform_location(program, name),
        }).collect();

        #[rustfmt::skip]
        let uniforms = meta.uniforms.uniforms.iter().scan(0, |offset, uniform| {
            let res = ShaderUniform {
                gl_loc: get_uniform_location(program, &uniform.name),
                _offset: *offset,
                _size: uniform.uniform_type.size(),
                uniform_type: uniform.uniform_type,
                array_count: uniform.array_count as _,
                value: uniform_cache::Value::default(),
            };
            *offset += uniform.uniform_type.size() * uniform.array_count;
            Some(res)
        }).collect();
        let uniform_cache_eligible = uniform_cache::names_disjoint(
            meta.uniforms.uniforms.iter().map(|u| u.name.as_str()),
            meta.images.iter().map(String::as_str),
        );
        Ok(ShaderInternal {
            program,
            objects: [vertex_shader, fragment_shader],
            images,
            uniforms,
            uniform_cache_eligible,
        })
    }
}
```

### 6. 开关、默认值、能力探测与回退

- arena 和 serial 身份默认启用；没有开关。
- `Shader::delete` / `Pipeline::delete` 是显式 API；不会自动在 Drop 中删除 GL 对象。
- `clear_resource_bindings` 是显式调用；没有自动调用点，除了 L-02 的 macroquad 扩容修复。
- 没有 GPU 能力探测；这是 CPU/GL 状态管理，不依赖扩展。
- 回退/失败语义：
  - stale/foreign 句柄：`get`/`remove` 返回 `None`，索引访问 panic；不会静默命中新资源。
  - shader 仍被 pipeline 引用：delete 返回 `false`，资源保留。
  - 删除后当前 pipeline：清空 `cur_pipeline`；调用方必须重新 apply 一个 live pipeline。
  - `clear_resource_bindings` 后：调用方必须重新 `apply_bindings`/`apply_pipeline`；不能依赖旧 attribute 缓存。

### 7. 测试与测量（完整数据）

- `lifecycle-library-tests.log`：14 项库测试通过。
- `lifecycle-source-tests.log`：16 项源码测试通过。
- `resource_arena.rs` 自带 3 个测试：
  - 2000 轮 slot 复用不产生别名，stale/foreign 句柄拒绝；
  - 数千次重入保持 slot 容量有界，owner 只释放一次；
  - shader retire 等待所有 live pipeline 引用。
- `graphics.rs` 的 shader 生命周期测试覆盖：link/compile 失败清理、名字预校验、uniform 缓存字节扣除、当前 pipeline 清理。
- **无应用级性能数据**：这是正确性/内存治理，不应报 FPS 收益。`lifecycle-v1-performance-results.json` 里的 11.8% 墙钟下降是主项目“chart/blob 生命周期”实验，不是 miniquad arena 的证据，本文不把它归到 L-06。
- 没有隔离的物理显存测量；`ShaderResourceStats` 只是 managed occupancy，不声称驱动已回收。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无优化（正确性/内存治理，非性能）**。

理由：

- stale/foreign 句柄、shader/pipeline 引用、GL 名字复用、编译失败泄漏都得到明确处理。
- 单元测试覆盖了 slot 复用、引用检查、容量有界；实现置信度高。
- 它不追求帧率，因此没有性能数据不是缺陷；但也不能在性能文档里报成加速。
- 仍未覆盖 context loss、完整 shader/pipeline 依赖图、driver 异步回收；这些是后续风险。

置信度：高（库层契约）；中（生产接入后的完整生命周期）。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `406954d`/`b7e1d66`/`1dfaa93` 与 macroquad pin 都不在 Phira-Pro `main` 所固定的生产库 rev 中。
- **参考实现：已落地。** 隔离生命周期提交是 `406954d` + `b7e1d66` + `1dfaa93`；macroquad `9c5d1c1`/`c862fc8` 只改 Cargo pin。
- **参考实现默认：开（arena）；显式（delete/clear）。** 库内 `Shader::new`、`Pipeline::new` 自动使用 arena；delete/clear 需要调用方显式调用。
- **是否有调用方：有（库内 + 调用方显式）。** arena 由库内构造路径使用；shader/pipeline 销毁和绑定清理需要接入方调用。
- **收益标注：正确性/内存治理，不计性能收益。**

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 固定 miniquad `1dfaa93`（或包含 arena/clear 的后续 rev）与对应 macroquad pin。
2. 在所有 shader/pipeline 销毁路径调用 `delete`，并先消费 deferred commands。
3. 删除 shader 前先删除引用它的 pipeline；不要依赖 `Shader::delete` 自动级联。
4. 删除/复用 raw buffer/texture 前调用 `clear_resource_bindings`；之后重新 apply 状态。
5. 在 context 切换/重建时创建新的 `GraphicsContext`，不要复用旧句柄；`context_id` 不是 EGL loss 检测器。
6. 做字体扩容、粒子 buffer 重建、shader 热重载、pause/resume 的回归。

风险：

- `Shader`/`Pipeline` 仍是 `Copy`；delete 后旧句柄不会自动变成 `None`，索引会 panic，调用方要避免继续使用。
- `clear_resource_bindings` 会禁用 attribute 数组；raw GL 代码如果假设状态不变，会出错。
- delete 不等待 GPU；如果 queued draw 仍引用 shader/pipeline，删除可能导致未定义行为。必须先消费命令。
- arena 只保证句柄身份，不保证 GL 对象物理回收；`shader_resource_stats` 不是显存统计。
- shader 名称预校验只覆盖 metadata 名字；其他 `CString` 构造点仍需检查。
- `glUseProgram(0)` 会改变当前 program；删除后必须重新 apply pipeline。

### 11. 来源与可追溯性

- miniquad arena/生命周期：`406954d8189bfeb55f375ee8ab2eafd325405bbf`，`graphics.rs` +111/-25、`resource_arena.rs` +175。
- 名称预校验：`b7e1d66f28269c631b3bb9d847f9ecce7ac8b87b`，`graphics.rs` +6。
- 绑定清理：`1dfaa933c642bc1b0ef3a27701a941b20ee084c1`，`graphics.rs` +17。
- macroquad pin：`9c5d1c1a7e43d4aa1b2c51140d61bc511dfaf9b6`（到 `b7e1d66`）、`c862fc8545593341c644a53ee72647d196c5b6c1`（到 `1dfaa93`），都只改 `Cargo.toml`。
- 代码位置：`resource_arena.rs` 全文；`graphics.rs:305`（Shader）、`:320`（stats）、`:337`（delete）、`:773`（context_id）、`:777`（stats）、`:785`（clear）、`:1869`（Pipeline delete）。
- 隔离 diffstat：`406954d` 为 `graphics.rs` +111/-25、`resource_arena.rs` +175；`b7e1d66` 为 `graphics.rs` +6；`1dfaa93` 为 `graphics.rs` +17。
- 测试：`lifecycle-library-tests.log`、`lifecycle-source-tests.log`。

## L-07 常驻网格延迟提交

### 1. 一句话结论与判定

macroquad `7068495` 在 `QuadGl` 的 draw call 队列里增加 `resident: Option<ResidentDraw>`：调用方把已经存在的 pipeline/bindings/uniforms 和 index range 排入队列，由 `draw` 在原有 target/viewport/scissor/顺序中延迟提交，不上传 CPU geometry arena。

这是**负收益/无收益**：同一 SO 的六轮真实普通 gameplay 中，GPU 延迟提交的完整应用墙钟均值 3.528/3.523ms，高于原音符 3.346ms；FPS 几乎相同，chart 均值也没有下降。质量对照 5 组末帧 RGBA 全同，但质量通过不等于性能收益。判定为**负（普通 gameplay 无收益）**。

注意：`a8aace0` 只是把 miniquad 依赖 pin 到 uniform cache，**不是** resident geometry 的实现。真正的实现提交是 `7068495`。

### 2. 问题与瓶颈

原宏quad 的 `DrawCall` 持有每个 draw 的 CPU 顶点/索引切片，`draw()` 时逐 call 上传。对于“几何已经常驻 GPU”的场景（例如谱面预编译的静态 VBO），再走 CPU arena 上传是浪费；而且每个 GPU span 可能提前 flush 和结束 pass，导致 pass 碎片化。

resident geometry 的目标：

- 复用调用方已有的 GPU buffer，不再每帧上传；
- 把 resident 命令插入原 draw call 队列，保持全局顺序；
- 共享 target/pass、viewport/scissor；
- 延迟到 `draw()` 统一提交，避免每个 span 提前 flush。

但真实普通 gameplay 中：

- 普通音符已经全局材质合批，能省的 CPU 上传很少；
- resident 命令不能跨普通 geometry 合批，反而增加队列分支；
- uniform 快照、Arc 共享、pass 管理都有成本；
- 普通路径含 CPU hold，实际 resident draw 只占 16/376，节省的提交量不足以抵消额外开销。

### 3. 设计思路与不变量

- `ResidentDraw { pipeline, bindings, uniforms: Arc<[f32]> }` 由调用方拥有；`QuadGl` 只保存引用/快照。
- `DrawCall.resident` 与普通 `vertices_start/indices_start` 互斥：resident 分支不上传 arena，也不结束当前 pass（除非 capture）。
- `resident_geometry` 是 unsafe：调用方必须保证所有 buffer/texture/pipeline/shader 在队列被 draw/clear 之前存活且不变；index buffer orphan 不能早于更早的 queued use。
- 快照 `texture`、`model`、`draw_mode`、`pipeline`、`viewport`、`clip`、`render_pass`、`capture`；uniform 是 `Arc<[f32]>`，按 page 共享且不可变。
- 普通 `geometry()` 遇到前一个 resident call 时必须新建 draw call，不能跨 resident 合批。
- `clear_draw_calls` 清空 resident，防止队列复用后旧引用残留。
- `has_pending_resident_geometry` 让调用方在修改/删除资源前 flush。
- `resident_geometry_draw_count` 只统计实际发出的 resident draw。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 macroquad `706849529ace7927e6507f235400bbdc1f7dafca`（2026-10-07，“支持常驻网格延迟提交”），`src/quad_gl.rs` +97/-3。隔离改动：

- 新增 `ResidentDraw`；
- `DrawCall` 新增 `resident: Option<ResidentDraw>`；
- `QuadGl` 新增 `resident_draws_issued: u64`；
- `clear_draw_calls` 清 resident；
- `draw` 新增 resident 分支；
- `geometry` 在前一个 call 是 resident 时强制新 call；
- 新增 `resident_geometry`、`resident_geometry_draw_count`、`has_pending_resident_geometry`。

miniquad 侧没有 resident 实现；它提供库 `master` `1796f44` 的 `apply_bindings_with_offsets` 和 `update_orphaned`，以及 `Pipeline`/`Bindings` 类型。resident 命令本身完全在 macroquad。

### 5. 完整核心代码

数据结构与队列清空：

```rust
// Buffers remain owned by the caller; the unsafe recording API makes the
// lifetime and mutation contract explicit. Uniform contents are snapshotted.
struct ResidentDraw {
    pipeline: Pipeline,
    bindings: Bindings,
    uniforms: std::sync::Arc<[f32]>,
}

struct DrawCall {
    vertices_start: usize,
    indices_start: usize,
    vertices_count: usize,
    indices_count: usize,

    clip: Option<(i32, i32, i32, i32)>,
    viewport: Option<(i32, i32, i32, i32)>,
    texture: Texture,

    model: glam::Mat4,

    draw_mode: DrawMode,
    pipeline: GlPipeline,
    uniforms: Option<Vec<u8>>,
    render_pass: Option<RenderPass>,
    capture: bool,
    resident: Option<ResidentDraw>,
}

    /// Reset only draw calls state
    pub fn clear_draw_calls(&mut self) {
        for dc in &mut self.draw_calls[..self.draw_calls_count] {
            dc.resident = None;
        }
        self.draw_calls_count = 0;
        self.vertices.clear();
        self.indices.clear();
    }
```

`draw` 里的 resident 分支与普通 geometry 分开：

```rust
            // Resident draws use the same ordered target/viewport/clip stream.
            // They neither upload the CPU geometry arena nor end this pass.
            if let Some(resident) = dc.resident.as_ref() {
                ctx.apply_pipeline(&resident.pipeline);
                let (x, y, w, h) = dc.viewport.unwrap_or((0, 0, width as i32, height as i32));
                ctx.apply_viewport(x, y, w, h);
                let (x, y, w, h) = dc.clip.map(|(x, y, w, h)| (x, height as i32 - (y + h), w, h))
                    .unwrap_or((0, 0, width as i32, height as i32));
                ctx.apply_scissor_rect(x, y, w, h);
                ctx.apply_bindings(&resident.bindings);
                ctx.apply_uniforms_from_bytes(resident.uniforms.as_ptr().cast(), resident.uniforms.len() * 4);
                ctx.draw(dc.indices_start as i32, dc.indices_count as i32, 1);
                self.resident_draws_issued += 1;
                if dc.capture {
                    if active_pass.is_some() { ctx.end_render_pass(); }
                    active_pass = None;
                    telemetry::track_drawcall(&resident.pipeline, &resident.bindings, 0, dc.indices_start, dc.indices_count);
                }
                dc.resident = None;
                dc.vertices_count = 0;
                dc.indices_count = 0;
                continue;
            }
```

记录与查询 API：

```rust
    /// Resident draws actually issued to the graphics backend since creation.
    pub fn resident_geometry_draw_count(&self) -> u64 { self.resident_draws_issued }

    /// Whether a caller-owned buffer is still referenced by queued commands.
    /// Flush before mutating/deleting resident resources if this returns true.
    pub fn has_pending_resident_geometry(&self) -> bool {
        self.draw_calls[..self.draw_calls_count].iter().any(|dc| dc.resident.is_some())
    }

    /// Record a custom resident mesh between ordinary geometry commands.
    /// Snapshot target, viewport and clip; shader uniforms include the caller's
    /// projection/model. No immediate draw, upload, sort, or implicit flush.
    ///
    /// # Safety
    /// The caller must keep every buffer, texture, pipeline and shader alive
    /// and unchanged until this queue is drawn or cleared. In particular, an
    /// index-buffer orphan must not precede an earlier queued use. The bindings,
    /// index range and f32 uniform layout must match the supplied pipeline.
    pub unsafe fn resident_geometry(
        &mut self,
        pipeline: Pipeline,
        bindings: Bindings,
        uniforms: std::sync::Arc<[f32]>,
        index_start: i32,
        index_count: i32,
    ) {
        if index_start < 0 || index_count <= 0 { return; }
        let pip = self.pipelines.get(self.state.draw_mode, self.state.depth_test_enable);
        if self.draw_calls_count >= self.draw_calls.len() {
            self.draw_calls.push(DrawCall::new(self.state.texture, self.state.model(),
                self.state.draw_mode, pip, None, self.state.render_pass));
        }
        let dc = &mut self.draw_calls[self.draw_calls_count];
        dc.texture = self.state.texture;
        dc.model = self.state.model();
        dc.pipeline = pip;
        dc.draw_mode = self.state.draw_mode;
        dc.uniforms = None;
        dc.vertices_start = 0;
        dc.vertices_count = 0;
        dc.indices_start = index_start as usize;
        dc.indices_count = index_count as usize;
        dc.viewport = self.state.viewport;
        dc.clip = self.state.clip;
        dc.render_pass = self.state.render_pass;
        dc.capture = self.state.capture;
        dc.resident = Some(ResidentDraw { pipeline, bindings, uniforms });
        self.draw_calls_count += 1;
        self.state.break_batching = false;
    }
```

普通 `geometry()` 不能跨 resident 合批：

```rust
        if previous_dc.map_or(true, |draw_call| {
            draw_call.resident.is_some()
                || draw_call.texture != self.state.texture
                || draw_call.clip != self.state.clip
                || draw_call.viewport != self.state.viewport
                || draw_call.model != self.state.model()
                || draw_call.pipeline != pip
                || draw_call.render_pass != self.state.render_pass
                || draw_call.draw_mode != self.state.draw_mode
                || draw_call.vertices_count > self.max_vertices - vertices.len()
                || draw_call.indices_count > self.max_indices - indices.len()
                || draw_call.capture != self.state.capture
                || self.state.break_batching
        }) {
```

### 6. 开关、默认值、能力探测与回退

- 库层没有全局开关；只有调用方显式 `unsafe resident_geometry` 才会进入 resident 分支。
- `DrawCall.resident` 默认 `None`；普通 geometry 路径不受影响。
- 应用研究侧用 `gpu_ir_deferred`/`gpu_render_ir` 之类开关控制是否录制 resident；这些是应用 flag，不是宏quad 的默认行为。
- 没有 GPU 能力探测；resident 依赖调用方提供有效的 pipeline/bindings/index range。
- 回退：调用方在不确定资源生命周期时不要使用 resident；`has_pending_resident_geometry` 为 true 时先 flush；capture 时 resident 分支会结束 pass，行为和普通路径一致。
- `clear_draw_calls` 会丢弃 resident 引用；调用方不能假设未 draw 的命令还会保留。

### 7. 测试与测量（完整数据）

**质量**

- 8 个 quality 作业，构成 5 组末帧像素对照；RGBA 全部相同。
- 普通路径含 CPU hold；actual native resident draw = 16/376，与录制数一致；pending index protection flush = 0。
- 高画质/forced-capability 两组回退和直接/延迟 GPU 对照也相同。
- 这些证明“采集帧输出一致”，不证明所有时刻/所有 GPU 一致。

**应用级性能**

同一 SO、六轮 120 预热 + 1200 真实时钟采样；所有臂相同 font pool，关闭 HUD 准备/背景融合/GPU 计时：

| 次序 | 策略 | FPS | 应用 p99 ms | chart 均值 ms | 完整应用墙钟均值 ms |
|---|---|---:|---:|---:|---:|
| 0 | 原音符 | 59.752 | 19.141 | 0.657 | 3.346 |
| 1 | GPU 直接 | 59.755 | 18.410 | 1.389 | 3.498 |
| 2 | GPU 延迟 | 59.759 | 18.188 | 0.759 | 3.528 |
| 3 | GPU 延迟 | 59.755 | 19.400 | 0.737 | 3.523 |
| 4 | GPU 直接 | 59.760 | 19.557 | 1.387 | 3.496 |
| 5 | 原音符 | 59.753 | 19.521 | 0.654 | 3.367 |

- FPS 几乎相同（59.752–59.760）；不能声称帧率提升。
- 完整应用墙钟：原音符 3.346/3.367ms；GPU 延迟 3.528/3.523ms，反而更高。
- chart 均值：原音符 0.657/0.654ms；GPU 延迟 0.759/0.737ms，也更高。
- 阶段耗时迁移不等于总耗时降低；结论是“没有普通谱面收益，不进生产”。

**微基准的边界**

`RENDER_RESEARCH.md` 的 64-draw 离屏微基准：原始 submit/completed p50 15.3440/18.2721ms；共享 arena + 连续 pass 0.7375/2.3542ms；共享 arena + 拆分 pass 15.5587/18.5458ms。这个数据属于 **库 master 的共享 arena/连续 pass**，不是 `7068495` 的 resident geometry；不能拿它给 resident geometry 背书。resident geometry 的同 SO 六轮见上表，结论是负收益。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**负（普通 gameplay 无收益）**。

理由：

- 质量输出一致，说明实现没有明显画质错误；但性能目标是完整帧，不是“像素相同”。
- 六轮同 SO 数据中，GPU 延迟的完整应用墙钟和 chart 均值都高于原音符；FPS 没有提升。
- resident 只覆盖 16/376 个 native draw，节省的 CPU 上传不足以抵消队列/分支/uniform/pass 成本。
- 微基准的连续 pass 收益属于库 master arena，不应归因给 resident。

置信度：高（同 SO 六轮、方向一致）；外推到其他谱面/GPU 中等。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** `7068495` 不在 Phira-Pro `main` 所固定的生产库 rev 中；`a8aace0` 只是 pin，不包含新的 macroquad 代码。
- **参考实现：已落地（应用实验）。** macroquad 侧 `7068495` 实现 resident 队列；miniquad 侧 `1796f44` 只提供 offset/orphan 原语，没有 resident API。
- **参考实现默认：显式调用。** 只有调用方显式 `unsafe resident_geometry` 才进入 resident 路径；应用研究 flag 控制录制。
- **是否有调用方：有（研究后端）。** 没有调用方在普通路径无条件使用 resident。
- **收益标注：负（普通 gameplay 无收益）。** 库 `master` 的共享 arena/连续 pass 不等于 resident geometry；库 `master` 没有 `resident_geometry`/`resident_draws_issued`。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤（如果仍要试验）：

1. 只为真正静态、已常驻 GPU 的 mesh 录制 resident；不要为普通音符/每帧变化的几何使用。
2. 保证 pipeline/bindings/uniforms/index range 在 draw/clear 之前不变；index buffer orphan 不能早于更早的 queued use。
3. 在修改/删除任何 resident 资源前检查 `has_pending_resident_geometry` 并 flush。
4. 用 `resident_geometry_draw_count` 核对录制数与 native draw 数；不一致时不要报性能。
5. 做同 SO ABBA，报告完整应用墙钟、chart 均值、p99、最大间隔；不要只看 FPS 或阶段耗时。

风险：

- **生命周期**：unsafe API；调用方 owns buffer/texture/pipeline/shader，任何提前删除都是 UB/驱动错误。
- **顺序**：resident 不能跨普通 geometry 合批；插入位置会改变批次结构，必须保持透明顺序。
- **uniform 快照**：`Arc<[f32]>` 是共享不可变；调用方不能在录制后修改同一内存。
- **index range**：base/count 必须匹配 bindings；越界会画错或触发驱动错误。
- **capture**：resident capture 会结束 pass，可能破坏调用方的 pass 假设；必须与普通 capture 语义一致。
- **收益错觉**：微基准/阶段耗时容易显示“提交变快”，但完整墙钟可能更差；必须以完整帧为准。

### 11. 来源与可追溯性

- macroquad 实现：`706849529ace7927e6507f235400bbdc1f7dafca`，`src/quad_gl.rs` +97/-3。
- macroquad pin：`a8aace0947640c867898239bba3c4f14b6f5f7c0`，只改 `Cargo.toml`。
- 代码位置：`src/quad_gl.rs:22`（ResidentDraw）、`:913`（draw count）、`:917`（pending）、`:930`（resident_geometry）。
- 隔离 diffstat：`src/quad_gl.rs` +97/-3；`Cargo.toml` +1/-1（仅 pin）。
- 数据：`Phira-Pro/本地证据/low-quality-normal/README.md` 第 234–251 行；`gpu-v2-*` 原始结果；`RENDER_RESEARCH.md` 的微基准仅作边界说明。

## L-08 图集读取依赖跟踪

### 1. 一句话结论与判定

macroquad `51147e9` 新增 `texture_reads` 模块和 `QuadGl` 的 token API：调用方在纹理首次绘制前注册，之后查询“当前排队命令是否可能读取该纹理”，再决定是否 flush/等待。它用 context id、owner、generation、serial 和 BTreeMap 维护保守的“可能读取”状态；最多 256 个纹理；custom material、ScreenTexture、capture、缺失 pipeline 都返回 unknown，要求走原 flush/fence。

这是**正确性/依赖治理 API**，不是直接优化：它本身不减少 GL 调用、不改变绘制，只让调用方在“更新纹理前是否需要 flush”上做出保守判断。判定为**无（依赖治理，无独立性能数据）**。

### 2. 问题与瓶颈

字体图集/纹理池在更新一张纹理前，必须知道：

- 当前 queue 里是否还有 draw 会读这张纹理？
- 如果读取，是否已经提交给 GPU？
- 如果已提交，是否必须 flush/fence 才能安全更新？

如果一律 flush，会产生不必要的同步；如果完全不查，可能覆盖 queued draw 正在采样的内容。需要一个**保守**的依赖查询：

- 宁可返回 `Some(true)` 或 `None`（未知），不要错误返回 `Some(false)`；
- 能识别普通 draw 的 primary texture、resident draw 的 images、named texture、ScreenTexture、capture；
- context 变化/序列溢出/容量满时 fail closed；
- 不把 serial 当 GPU 完成证明。

### 3. 设计思路与不变量

- `TextureReadToken { context, owner, generation, texture }`：私有 generation 拒绝 stale 句柄和复用的 native name；owner 是全局唯一 id，防止两个 tracker 的 token 互认。
- `Tracker { context, owner, generation, serial, entries: BTreeMap<u32, Entry>, known, failed }`；最多 256 个 texture。
- `register(texture, queue_empty)`：
  - texture 非零、未重复、未超容量；
  - 首次注册要求 queue 为空（否则无法建立 baseline）；
  - generation 递增；失败则永久 fail closed。
- `submitted(indices, images)`：只在 `indices > 0` 时递增 serial，并给所有传入 image 记录 `last = serial`；零索引不推进序列。
- `status(token, queued)`：校验 token；返回 `{ queued, last_submitted }`；`queued` 是当前 queue 观察，不是 GPU 完成。
- `unregister(token, pending)`：只有 `pending == Some(false)` 才允许移除；unknown epoch 下必须先 flush。
- `check_context(current)`：context 改变则永久 fail；相同 id 也只代表 managed context，不检测 native EGL loss。
- `pending_normal(...)`：
  - capture、无 pipeline、custom（`pipeline.0 >= 4`）、ScreenTexture、named texture 任意一个成立 → `None`（unknown）；
  - 否则返回 `Some(indices > 0 && primary == target)`。
- `pending_resident(...)`：capture → `None`；否则 `Some(indices > 0 && images 包含 target)`。
- 序列/generation 溢出永久 fail closed；零索引不推进；容量满拒绝新注册。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 macroquad `51147e901c45876802628468799ee9352827fb6f`（2026-10-08，“追踪图集读取依赖”），隔离改动：

- `src/texture_reads.rs` +340：token/status/tracker/pending 函数 + 7 个测试。
- `src/quad_gl.rs` +133：`QuadGl.texture_reads` 字段、`register_texture_reads`、`unregister_texture_reads`、`texture_read_status`、`invalidate_texture_reads`、`pending_texture_reads`，以及 `draw` 中的 `submitted`/`invalidate` 调用。
- `src/lib.rs` +1、`src/prelude.rs` 2 行：模块与导出。

参考实现没有 miniquad 侧实现；它依赖 macroquad 的 `QuadGl` queue 和 `Texture` 句柄。`51147e9` 是隔离实现提交；更后面的 `50b5dbf`、`461ad5f`、`cd0ab01`、`ed1db13`、`161fab1`、`4b46e86`、`de34611` 都叠在它上面，但各自只做 pin/其他功能。

### 5. 完整核心代码

`texture_reads.rs` 的核心（不含测试）如下：

```rust
//! Bounded conservative managed texture-read markers. No GL queries or waits.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

const LIMIT: usize = 256;
static OWNERS: AtomicU64 = AtomicU64::new(1);

/// Identity of one explicitly registered texture in one QuadGl/context.
/// Private generations reject stale handles and recycled native names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureReadToken {
    context: usize,
    owner: u64,
    generation: u64,
    texture: u32,
}

/// Managed possible-read state, not GPU completion or elapsed time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureReadStatus {
    pub queued: bool,
    /// Conservative serial recorded after a positive-index managed draw
    /// attempt. Context.draw returns void and may reject an invalid request.
    pub last_submitted: u64,
}

struct Entry {
    generation: u64,
    last: u64,
}
pub(crate) struct Tracker {
    context: usize,
    owner: u64,
    generation: u64,
    serial: u64,
    entries: BTreeMap<u32, Entry>,
    known: bool,
    failed: bool,
}
impl Tracker {
    pub(crate) fn new(context: usize) -> Self {
        let owner = OWNERS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                old.checked_add(1)
            })
            .ok();
        Self {
            context,
            owner: owner.unwrap_or(0),
            generation: 0,
            serial: 0,
            entries: BTreeMap::new(),
            known: owner.is_some(),
            failed: owner.is_none(),
        }
    }
    /// A changed managed context permanently invalidates this owner. Equal
    /// IDs only describe managed Context identity, never native EGL lifetime.
    pub(crate) fn check_context(&mut self, current: usize) -> bool {
        if current != self.context {
            self.fail();
            return false;
        }
        true
    }
    pub(crate) fn active(&self) -> bool {
        !self.entries.is_empty()
    }
    pub(crate) fn invalidate(&mut self) {
        self.known = false;
    }
    fn fail(&mut self) {
        self.failed = true;
        self.known = false;
    }
    pub(crate) fn register(&mut self, texture: u32, queue_empty: bool) -> Option<TextureReadToken> {
        if self.failed
            || texture == 0
            || self.entries.len() >= LIMIT
            || self.entries.contains_key(&texture)
        {
            return None;
        }
        if !self.known {
            if !self.entries.is_empty() || !queue_empty {
                return None;
            }
            self.known = true;
        }
        let generation = match self.generation.checked_add(1) {
            Some(next) => next,
            None => {
                self.fail();
                return None;
            }
        };
        self.generation = generation;
        self.entries.insert(
            texture,
            Entry {
                generation,
                last: 0,
            },
        );
        Some(TextureReadToken {
            context: self.context,
            owner: self.owner,
            generation,
            texture,
        })
    }
    pub(crate) fn texture(&self, token: TextureReadToken) -> Option<u32> {
        let entry = self.entries.get(&token.texture)?;
        (token.context == self.context
            && token.owner == self.owner
            && token.generation == entry.generation)
            .then_some(token.texture)
    }
    pub(crate) fn status(
        &self,
        token: TextureReadToken,
        queued: bool,
    ) -> Option<TextureReadStatus> {
        if self.failed || !self.known {
            return None;
        }
        self.texture(token)?;
        Some(TextureReadStatus {
            queued,
            last_submitted: self.entries.get(&token.texture)?.last,
        })
    }
    /// Cleanup remains possible in an unknown epoch, but only after the owner
    /// has consumed every possibly referring pending command.
    pub(crate) fn unregister(&mut self, token: TextureReadToken, pending: Option<bool>) -> bool {
        if self.texture(token).is_none() || pending != Some(false) {
            return false;
        }
        self.entries.remove(&token.texture);
        true
    }
    pub(crate) fn submitted(&mut self, indices: usize, images: impl IntoIterator<Item = u32>) {
        if !self.active() || self.failed || indices == 0 {
            return;
        }
        self.serial = match self.serial.checked_add(1) {
            Some(next) => next,
            None => {
                self.fail();
                return;
            }
        };
        for image in images {
            if let Some(entry) = self.entries.get_mut(&image) {
                entry.last = self.serial;
            }
        }
    }
}

/// Conservative semantics gate for normal queued commands. Named textures are
/// late-bound, and custom shaders may consume flush-time Projection/_Time.
/// Their queued state is deliberately unknown, even if all names are assigned.
pub(crate) fn pending_normal(
    target: u32,
    primary: u32,
    indices: usize,
    capture: bool,
    pipeline_present: bool,
    custom: bool,
    screen: bool,
    named: impl IntoIterator<Item = Option<u32>>,
) -> Option<bool> {
    if capture || !pipeline_present || custom || screen {
        return None;
    }
    if named.into_iter().next().is_some() {
        return None;
    }
    Some(indices > 0 && primary == target)
}
pub(crate) fn pending_resident(
    target: u32,
    indices: usize,
    capture: bool,
    images: impl IntoIterator<Item = u32>,
) -> Option<bool> {
    if capture {
        return None;
    }
    Some(indices > 0 && images.into_iter().any(|image| image == target))
}
```

`QuadGl` 的公开 API 与 queue 扫描：

```rust
    /// Register a private fresh texture before its first draw, or a texture
    /// whose owner already holds a baseline GPU fence covering every prior read.
    /// Registration does not observe reads made earlier and is not a GPU fence.
    /// At most 256 textures can be tracked; duplicate/native-zero IDs fail.
    /// Pass the actual current managed Context ID to every token operation.
    /// A context mismatch permanently disables tracking in this QuadGl.
    /// Unregister before deleting/recycling its native name. Never infer GPU
    /// completion from serial zero. A failed/unknown query needs the original
    /// flush/fence path. Managed Context identity does not detect native EGL loss.
    pub fn register_texture_reads(
        &mut self,
        texture: Texture,
        current_context_id: usize,
    ) -> Option<TextureReadToken> {
        if !self.texture_reads.check_context(current_context_id) {
            return None;
        }
        self.texture_reads
            .register(texture.gl_internal_id(), self.draw_calls_count == 0)
    }

    /// Remove a registration only when no pending command may refer to it.
    /// In an unknown epoch, flushing first permits safe owner cleanup; this
    /// method itself never submits commands, waits, deletes or fences a texture.
    pub fn unregister_texture_reads(
        &mut self,
        token: TextureReadToken,
        current_context_id: usize,
    ) -> bool {
        if !self.texture_reads.check_context(current_context_id) {
            return false;
        }
        let Some(texture) = self.texture_reads.texture(token) else {
            return false;
        };
        let pending = self.pending_texture_reads(texture);
        self.texture_reads.unregister(token, pending)
    }

    /// Conservative managed possible reads only. This is a current queue
    /// observation, not permission to mutate resources across later operations.
    /// Custom/late-bound named materials, ScreenTexture/capture and missing
    /// pipelines are unknown. Raw GL readers/writers must explicitly invalidate.
    /// Resident resource lifetimes remain the unsafe recording caller's contract.
    pub fn texture_read_status(
        &mut self,
        token: TextureReadToken,
        current_context_id: usize,
    ) -> Option<TextureReadStatus> {
        if !self.texture_reads.check_context(current_context_id) {
            return None;
        }
        self.texture_reads.status(token, false)?;
        let texture = self.texture_reads.texture(token)?;
        let queued = self.pending_texture_reads(texture)?;
        self.texture_reads.status(token, queued)
    }

    /// Fail closed before any untracked raw read/write or unsupported helper.
    /// Existing tokens remain unusable until all are unregistered after a flush.
    /// An empty queue plus fresh registrations starts a new known epoch.
    /// Serial/generation exhaustion cannot recover inside this QuadGl.
    pub fn invalidate_texture_reads(&mut self) {
        self.texture_reads.invalidate();
    }

    fn pending_texture_reads(&self, texture: u32) -> Option<bool> {
        let mut queued = false;
        for dc in &self.draw_calls[..self.draw_calls_count] {
            let pending = if let Some(resident) = dc.resident.as_ref() {
                texture_reads::pending_resident(
                    texture,
                    dc.indices_count,
                    dc.capture,
                    resident
                        .bindings
                        .images
                        .iter()
                        .map(|image| image.gl_internal_id()),
                )
            } else {
                let pipeline = self
                    .pipelines
                    .pipelines
                    .get(dc.pipeline.0)
                    .and_then(|pipeline| pipeline.as_ref())?;
                texture_reads::pending_normal(
                    texture,
                    dc.texture.gl_internal_id(),
                    dc.indices_count,
                    dc.capture,
                    true,
                    dc.pipeline.0 >= 4,
                    pipeline.wants_screen_texture,
                    pipeline.textures.iter().map(|name| {
                        pipeline
                            .textures_data
                            .get(name)
                            .map(|image| image.gl_internal_id())
                    }),
                )
            };
            queued |= pending?;
        }
        Some(queued)
    }
```

`draw` 中的提交记录与失效点（节选完整代码路径，不是省略号）：

```rust
                if self.texture_reads.active() {
                    self.texture_reads.submitted(
                        dc.indices_count,
                        resident.bindings.images.iter().map(|image| image.gl_internal_id()),
                    );
                }
                self.resident_draws_issued += 1;
                if dc.capture {
                    if active_pass.is_some() { ctx.end_render_pass(); }
                    active_pass = None;
                    if self.texture_reads.active() {
                        self.texture_reads.invalidate();
                    }
                    telemetry::track_drawcall(&resident.pipeline, &resident.bindings, 0, dc.indices_start, dc.indices_count);
                }
```

```rust
            if self.texture_reads.active() {
                self.texture_reads.submitted(
                    dc.indices_count,
                    bindings.images.iter().map(|image| image.gl_internal_id()),
                );
            }

            if dc.capture {
                if active_pass.is_some() { ctx.end_render_pass(); }
                active_pass = None;
                if self.texture_reads.active() {
                    self.texture_reads.invalidate();
                }
                telemetry::track_drawcall(&pipeline.pipeline, bindings, vertex_offset, dc.indices_start, dc.indices_count);
            }
```

### 6. 开关、默认值、能力探测与回退

- 没有全局开关；不注册就不跟踪。`Tracker::new` 在 `QuadGl::new` 中创建，`active()` 只在 entries 非空时为 true。
- 默认不注册任何纹理；应用必须显式 `register_texture_reads`。
- 能力探测：无 GL 扩展；纯 CPU 队列状态。
- 回退/未知：
  - `capture`、无 pipeline、custom pipeline、ScreenTexture、named texture → `None`，调用方走原 flush/fence。
  - context 改变、generation/serial 溢出、容量满、重复/零 id → `None`/`false`，fail closed。
  - raw GL 读写不在跟踪范围；调用方必须 `invalidate_texture_reads`。
- `TextureReadStatus.last_submitted` 不是 GPU 完成时间；serial 0 不代表完成。
- `unregister` 只有在 `pending == Some(false)` 时成功；unknown epoch 下先 flush。

### 7. 测试与测量（完整数据）

- `texture_reads.rs` 有 7 个测试：
  - submitted images 覆盖 primary/ScreenTexture/named/resident bindings；
  - queued primary/resident reads 与 opaque 路径区分；
  - token owner/context/pending cleanup/recycled id 不别名；
  - unknown epoch 需要移除所有旧 token 且 queue 为空；
  - 容量、重复、generation 溢出有界；
  - context 改变永久 fail closed；
  - serial 溢出/零索引不推进。
- 参考实现的测试日志：`branch-atlas-tests.log` 2 项；`render-reads-tests.log` 4 项；`render-read-context-tests-retry.log` 5 项。
- **无独立应用级性能数据**。该 API 不直接减少 GL 调用；它只改变调用方“是否 flush/更新”的决策。
- 应用集成在 `prpr/src/ui/atlas_pool.rs`：注册 token、查询 `texture_read_status`、unregister；`atlas_deferred_probe` 也用它做 sentinel。`cache-lifecycle`/字体池数据属于依赖治理与生命周期验证，不是本 API 的帧率收益。
- 不能把“没有 flush”当作“没有 GPU 读取”；`queued=false` 只表示当前 managed queue 没有已知读取。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无（依赖治理，无独立性能数据）**。

理由：

- 设计目标是保守正确，不是加速；它本身不改变 draw call、纹理上传或 GPU 工作。
- 测试覆盖了 stale/foreign/overflow/unknown 等 fail-closed 行为，实现置信度高。
- 没有同 SO OFF/ON 的帧率或墙钟数据，不能报性能收益。
- 它对后续“减少不必要 flush”是必要条件，但必须和应用侧策略一起测量。

置信度：实现高；正确性中高（保守语义清楚）；性能无数据。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** macroquad `51147e9` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有 `texture_reads`。
- **参考实现：已落地。** `51147e9` 是隔离实现提交；后续 `50b5dbf` 等只做 pin/其他功能。
- **参考实现默认：关（需注册）。** 不注册就不跟踪；应用必须显式 `register_texture_reads`。
- **是否有调用方：有（研究路径）。** 应用侧 atlas_pool、atlas_deferred_probe 使用，但只在研究构建/特定路径；没有默认全局启用。
- **收益标注：基础设施/依赖治理，无独立性能数据。** 普通路径没有额外成本，也没有收益。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 在纹理首次绘制前注册；如果纹理已有 GPU 读取历史，先建立 baseline fence/flush，否则不要注册。
2. 每次更新/删除前查询 `texture_read_status`；`Some(false)` 才能安全更新，`Some(true)`/`None` 走原 flush/fence。
3. 在 raw GL 读写、custom material、ScreenTexture、capture 前后调用 `invalidate_texture_reads`。
4. `unregister` 前确保 pending 为 false；unknown epoch 先 flush。
5. 在 context 切换时丢弃旧 token，重新注册；不要把 managed context id 当 EGL 存活检测。

风险：

- **保守性**：custom/named/ScreenTexture 都返回 unknown；如果调用方误把 unknown 当 false，会覆盖 queued 读取。
- **容量**：最多 256 个；超出后注册失败，调用方必须回退。
- **生命周期**：token 不是 GPU fence；serial 只表示提交顺序，不表示完成。
- **raw GL**：不在跟踪范围；必须显式 invalidate。
- **context loss**：相同 context id 不代表 native EGL 仍存活；context 丢失后旧 token 必须废弃。
- **性能误判**：减少 flush 可能只是把同步推迟到别处；必须看完整帧和 GPU 完成。

### 11. 来源与可追溯性

- macroquad 实现：`51147e901c45876802628468799ee9352827fb6f`。
- 隔离 diffstat：`src/lib.rs` +1、`src/prelude.rs` +1/-1、`src/quad_gl.rs` +133、`src/texture_reads.rs` +340。
- 代码位置：`src/texture_reads.rs:11`（token）、`:20`（status）、`:31`（tracker）、`:164`（pending_normal）、`:182`（pending_resident）；`src/quad_gl.rs:604`（register）、`:619`（unregister）、`:639`（status）、`:657`（invalidate）、`:661`（pending scan）。
- 数据/集成：`branch-atlas-tests.log`、`render-reads-tests.log`、`render-read-context-tests-retry.log`；`prpr/src/ui/atlas_pool.rs`（研究源）。

## L-09 保守的排队纹理依赖查询

### 1. 一句话结论与判定

macroquad `27cc446` 新增一个 15 行的只读查询 `queued_texture_dependency(texture)`：遍历当前 draw call 队列，若某个 draw 的 primary texture 等于目标，或 pipeline 是 custom（`pipeline.0 >= 4`），或 `capture` 为真，就返回 `true`。

它比 L-08 的 token 系统简单得多：没有注册、没有 serial、没有 context 校验；它只回答“当前 queue 中是否有保守的已知/未知依赖”。判定为**无（无可靠收益）**：应用侧 `glyph_scoped_flush` 的 8 项加载 ABBA 没有可靠收益，默认关闭。

### 2. 问题与瓶颈

字体图集更新前，如果 queue 里有 draw 会读该 atlas，直接更新可能覆盖 queued 采样。最简单的保守策略是“只要有任何 custom pipeline 或 capture，就 flush”；`queued_texture_dependency` 把这个判断暴露给调用方：

- built-in pipeline：只看 primary texture；
- custom pipeline：一律视为依赖（它的额外 image 可能在 recording 后变化，screen snapshot 也可能观察早期命令）；
- capture：一律视为依赖。

它不等待 GPU，也不处理 raw GL；调用方仍要自己 flush/fence。

### 3. 设计思路与不变量

- 只读：不修改 queue、不 flush、不查 GL。
- 保守：custom/capture 直接 `true`，不做“名字已绑定所以安全”的假设。
- built-in pipeline 的 primary texture 相等即 `true`；否则 `false`。
- 不处理 named texture、ScreenTexture 的细粒度依赖；这些由 custom/capture 的保守分支覆盖。
- 不提供 token/所有权；纹理删除/复用仍由调用方保证。
- 没有 context 校验；调用方必须在正确的 `QuadGl`/context 上调用。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 macroquad `27cc446777d70c624ec8d25d0f0757fc7c8b19aa`（2026-10-06，“Expose conservative queued texture dependency query”），`src/quad_gl.rs` +15。

L-09 的参考实现基于 `ce7949e`，不是 `7068495` 的 resident 路径；因此它没有 `texture_reads.rs`，也没有 resident token。它只使用当时的 `DrawCall.texture` / `draw.pipeline.0` / `draw.capture`。

### 5. 完整核心代码

```rust
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

### 6. 开关、默认值、能力探测与回退

- 库层没有开关；函数是只读查询。
- 应用侧 `glyph_scoped_flush` 默认 `false`；只有研究构建开启。
- 没有 GPU 能力探测；纯 CPU queue 检查。
- 回退：返回 `true` 时调用方走原 flush；返回 `false` 只表示“当前 queue 没有已知依赖”，不表示 GPU 已完成。raw GL 读取、已提交 GPU 读取仍由原路径处理。
- `pipeline.0 >= 4` 是 built-in pipeline 数量的硬编码假设；如果内置 pipeline 数量变化，这个阈值必须同步，否则会把 built-in 误判为 custom（保守）或把 custom 误判为 built-in（危险）。这是该 API 的主要脆弱点。

### 7. 测试与测量（完整数据）

- 该提交没有新增库级单测；它只是一个 15 行只读函数。
- 应用 `glyph_scoped_flush` 的 8 项真实 UI 加载 ABBA：
  - Exo 最大间隔中位数：65.111ms → 69.103ms；
  - Petrichor：66.581ms → 65.421ms；
  - 两者持续 FPS 略降；
  - 结论：无可靠收益，默认 `false`。
- 12 项质量/9 项严格 RGBA 全同，说明该策略在采集帧上没有画质差异；但性能不支持启用。
- 没有独立的应用级 FPS/墙钟收益数据表明“减少 flush”能转化为完整帧改善。
- 该 cohort 的口径是同一 SO、同一真实 UI 加载窗口、同一字体池设置；它不是 gameplay FPS，也不是全曲/全设备结论。
- 由于收益/损失还包含应用侧 flush 策略，不能把差异单独归因给这 15 行查询；查询只是把“是否必须 flush”从猜测变成保守判断。
- 在 Exo 上变慢、Petrichor 上基本持平，说明收益依赖具体图集更新顺序和窗口；没有稳定方向时保持默认关闭是正确决定。
- 质量 12 项/严格 RGBA 9 项全同只证明采集帧没有画质差异；它不能证明所有字体/所有 atlas 布局都安全。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无（无可靠收益）**。

理由：

- 查询本身只减少“是否 flush”的判断成本，不减少 GPU 读取或上传；
- 应用 ABBA 没有可靠收益，Exo 反而变慢，Petrichor 基本持平；
- 默认关闭，未进入普通路径；
- 硬编码 `pipeline.0 >= 4` 增加维护风险。

置信度：实现事实高；应用性能结论中等（8 项有限窗口）；收益结论高（没有稳定正收益）。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** macroquad `27cc446` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有 `queued_texture_dependency`。
- **参考实现：已落地（应用实验）。** `27cc446` 基于 `ce7949e` 的 arena 基线，不是 `7068495`；因此没有 resident 代码。
- **参考实现默认：关。** 应用侧 `glyph_scoped_flush=false`。
- **是否有调用方：有（研究构建）。** 没有生产默认调用方。
- **收益标注：基础设施/保守查询；当前无可靠收益证据。** 不是 L-08 的替代品：L-08 有 token/serial/context，L-09 只有 queue 遍历。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在字形图集更新前调用；返回 `true` 就 flush，返回 `false` 也只跳过“已知 queue 依赖”，仍要处理 raw GL/已提交 GPU。
2. 同步维护内置 pipeline 数量；`pipeline.0 >= 4` 的阈值必须由生成代码或常量保证。
3. 在 custom material、ScreenTexture、capture、named texture 场景默认走 flush；不要试图细化。
4. 用 8 项以上真实 UI 加载 ABBA、严格 RGBA、持续 FPS、p99/max 验证；不要只看 flush 次数。
5. 如果收益不稳定，保持默认关闭。

风险：

- **误判 built-in/custom**：阈值硬编码；pipeline 顺序变化会导致保守性错误。
- **未知依赖**：named texture/ScreenTexture 没有被显式检查；依赖 custom/capture 分支兜底。
- **GPU 完成**：函数不等待已提交读取；调用方仍要 fence/flush。
- **生命周期**：没有 token；纹理删除/复用仍由调用方负责。
- **性能错觉**：减少 flush 次数不等于减少帧时间；已有数据就是反例。
- **魔数维护**：`pipeline.0 >= 4` 应替换为显式的 built-in pipeline 数量常量，并在 pipeline 创建处集中校验，避免阈值散落。

### 11. 来源与可追溯性

- macroquad 实现：`27cc446777d70c624ec8d25d0f0757fc7c8b19aa`，`src/quad_gl.rs` +15。
- 隔离 diffstat：`src/quad_gl.rs` +15。
- 代码位置：`src/quad_gl.rs:690`。
- 数据：`Phira-Pro/docs/development/rendering-research.md` 第 38 行；8 项加载 ABBA 与 12 项质量/9 项严格 RGBA 记录；默认 `false`。

## L-10 绘制状态查询（判定线省略）

### 1. 一句话结论与判定

macroquad `461ad5f` 新增两个只读 getter：

- `is_using_default_material()`：当前 queued logical pipeline 是否为内置材质；
- `is_geometry_capture_enabled()`：后续 geometry 是否标记 telemetry capture。

它们本身**不做省略、不改绘制、不查 GL、不 flush**，只是把决策所需的状态暴露给调用方。真正的“判定线省略”在应用侧（`line_elision`），根据默认材质、深度/捕获状态和几何域做保守跳过。

判定：**API 自身为无（只读查询）；应用侧 normal CPU 路径在一次 cohort 中有初步正收益。** 不能把应用侧的 -7.52% 直接归给这 13 行 getter。

### 2. 问题与瓶颈

判定线省略需要知道：

- 当前是否使用内置材质？自定义材质可能改变 alpha/深度/捕获语义，不能省略。
- 是否开启 capture？capture 需要保留 draw call 以记录 telemetry，不能省略。
- 深度测试/混合/裁剪域是否满足保守条件？这些由应用侧状态判断。

如果调用方无法读取这些状态，就只能猜测或每帧 flush 后查询，破坏 queued 语义。getter 的价值是**零副作用地读取逻辑状态**，让调用方在录制阶段做保守决策。

### 3. 设计思路与不变量

- 只读 `self.state`，不调用 GL，不 flush，不修改 queued commands。
- `is_using_default_material()` 返回 `self.state.pipeline.is_none()`；它只表示逻辑 pipeline 选择，不表示深度/混合状态。
- `is_geometry_capture_enabled()` 返回 `self.state.capture`；它只表示后续 geometry 的 capture 标记，不表示已经 capture 的 draw。
- 读取不会使 queue 失效；调用方可以在录制任意位置查询。
- 省略决策必须由调用方结合深度、混合、alpha、clip、viewport、material 等条件；getter 不是“安全省略”的充分条件。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧是 macroquad `461ad5f7f8eeff218eb17ec7201c89ff4b73975e`（2026-10-09，“查询绘制状态”），`src/quad_gl.rs` +13。它属于参考实现；该参考实现还叠放了 `51147e9` 的 atlas 依赖，但 `461ad5f` 本身只有两个 getter。

没有 miniquad 侧实现；状态在 macroquad 的 `GlState` 中：

- `pipeline: Option<GlPipeline>`；
- `capture: bool`；
- `depth_test_enable`、`clip`、`viewport` 等由其他 getter/状态字段提供。

### 5. 完整核心代码

```rust
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

### 6. 开关、默认值、能力探测与回退

- getter 常驻，无开关；调用即读。
- 应用侧 `line_elision` 是研究 flag，默认关闭；只有特定 cohort 开启。
- 没有能力探测；纯 CPU 状态。
- 回退：调用方在任一条件不满足（custom material、capture、depth、非有限域、非零 alpha 等）时保留原几何；getter 本身不提供回退。
- 读取不改变 queue，因此不会引入额外 flush；但调用方如果根据读取结果跳过 draw，必须自己保证语义等价。

### 7. 测试与测量（完整数据）

- `line-elision-tests-final.log`：3 项测试通过；覆盖默认材质/capture 查询与省略守卫的基本契约。
- 应用 `line-elision-v1`：
  - normal CPU 路径 app wall change：**-7.5167%**（ON 比 OFF 低）；
  - low GPU 编译路径 app wall change：**+0.9478%**（略差，且没有实际省略）；
  - CPU low skipped：**6805**；
  - 4 组有限质量像素对全部相等；
  - 8 个采样循环窗口 max 均 < 30ms。
- 解释：normal CPU 路径的收益来自应用侧省略逻辑，不是 getter 本身；low GPU 路径没有省 draw，+0.95% 是墙钟波动，不是 DCE 收益。
- 没有独立“getter 调用成本”测量；它只是两个字段读取。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**无优化（API 本身无性能收益）；应用侧效果归入 P-04。**

理由：

- getter 只是读取两个字段，没有独立性能收益或成本。
- normal CPU 路径的 -7.52% 是同 cohort 的应用级证据，但依赖省略策略、场景和有限窗口；不能归因给 13 行 API。
- low GPU 路径 +0.95% 且没有实际省略，说明“有 API”不等于“有收益”。
- 质量 4 对相同，但覆盖有限；不能声称所有判定线/所有材质都安全。

置信度：API 事实高；应用收益中等（单 cohort）；跨场景低。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** macroquad `461ad5f` 不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有这两个 getter。
- **参考实现：已落地。** 参考实现叠在 atlas 依赖链上；`461ad5f` 隔离 +13。
- **参考实现默认：开（getter）；关（应用 flag）。** getter 常驻、调用即读；应用侧 `line_elision` 默认关闭。
- **是否有调用方：有（应用研究路径）。** `line_elision` 研究开关有调用方；没有生产默认启用。
- **收益标注：基础设施（API 自身无收益）；应用侧初步正仅限单一 cohort，不归因给 getter。** 不是通用“省略 API”，只是状态查询。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在录制阶段读取 getter；不要在 flush 后假设它仍描述已提交命令。
2. 省略条件至少包括：默认材质、capture=false、无深度、有限仿射域、最终 alpha=0、非 resident/非特殊批次。
3. 保留原几何的投影、颜色、混合、顺序；省略不能改变透明排序或判定线可见性。
4. 用严格 RGBA + draw 顺序 + 反例（非零 alpha、custom material、capture、裁剪）验证。
5. 把 getter 调用成本与省略收益分开报告；不要用 normal cohort 的 -7.52% 代表所有路径。

风险：

- **状态快照**：getter 读的是当前逻辑状态，不是已提交 GPU 状态；在 queue 中间读取后继续修改状态，可能得到不一致决策。
- **充分条件**：默认材质/capture 只是两个条件；深度、混合、alpha、clip 仍需应用侧判断。
- **收益错觉**：省略 draw 可能减少 CPU/提交，但不一定减少 GPU 像素；low GPU 路径就是反例。
- **画质**：判定线可能在 alpha=0 时仍影响深度/遮挡；省略必须保持原语义。
- **维护**：getter 没有版本/契约标记；应用侧条件变化时要同步。

### 11. 来源与可追溯性

- macroquad 实现：`461ad5f7f8eeff218eb17ec7201c89ff4b73975e`，`src/quad_gl.rs` +13。
- 隔离 diffstat：`src/quad_gl.rs` +13。
- 代码位置：`src/quad_gl.rs:1005`、`:1011`。
- 数据：`line-elision-v1-summary.json` 的 `conclusion`；`line-elision-v1-performance-results.json`；`line-elision-tests-final.log`；`summarize-line-elision.py` 的 `(on / off - 1) * 100` 口径。

## D-01 诊断设施：原生帧循环、提交诊断、有界采集

### 1. 一句话结论与判定

三项都是**诊断设施，不是优化**：

- miniquad `eb0eda0`：在 Android 原生循环中记录 Events/Update/Draw/Swap/Yield 的墙钟与可选线程 CPU 时间；
- miniquad `e63917d`：统计 managed draw/pass API 边界、绑定次数和有界 draw trace；
- miniquad `bc1d9d4`：把原生帧 journal 容量从固定 2048 扩展为 1..=32768 的有界可配置容量，且热路径不分配。

macroquad 的 `50b5dbf`、`cc302a2`、`161fab1` 都只是固定对应 miniquad rev，不包含实现。判定：**非优化（诊断）**；默认全部关闭。

### 2. 问题与瓶颈

没有诊断时，团队只能看到：

- 应用层 `update/render/flush` 墙钟；
- FPS/间隔；
- 偶发长帧，但不知道长在 native Events、Update、Draw、Swap、Yield 还是 queue gap。

原生循环诊断解决“时间花在哪个阶段”的归因问题；提交诊断解决“draw/pass/bind API 边界有多少”的计数问题；有界采集解决“短窗口 journal 溢出/丢帧”的问题。

它们不改变帧操作、不插入 GPU 查询、不等待；因此不应期待帧率提升。诊断本身还有观察开销，必须与无诊断 cohort 分开。

### 3. 设计思路与不变量

**原生帧循环（`native_loop_timing`）**

- 线程本地 `State`，只在渲染线程 configure/snapshot。
- `Phase`：Events=0、Update=1、Draw=2、Swap=3、Yield=4。
- 每帧 `begin_frame` → 各阶段 `begin_phase/end_phase` → `finish_frame`；下一帧 `begin` 时 finalize 上一帧。
- `Journal` 只保留已完成帧；当前 in-flight 帧不计入 snapshot。
- 墙钟用 `Instant`；线程 CPU 用 Android `CLOCK_THREAD_CPUTIME_ID`，可选 mode 0/1/2（关闭/真实/强制缺失）。
- 任何 CPU 读取失败/回退/非单调 → 清空该帧所有 CPU 值，不伪造 0。
- 容量默认 `CAPACITY=2048`；`configure_with_capacity` 接受 1..=`MAX_CAPACITY=32768`；启用时先 `try_reserve_exact`，失败保留旧 capture，不改变 epoch/状态。
- 热路径 `begin/finish` 不再分配；ring 满了丢最旧并计数 `dropped`。
- 不测量 GPU 时间；draw/swap 墙钟包含驱动/呈现等待。

**提交诊断（`submit_diagnostics`）**

- `SubmissionStats` 统计 native draw calls、instanced API calls、multi-instance、zero-work、invalid-shape、rejected、nominal index-instances、pass begin/end、framebuffer bind、pipeline apply、binding apply、trace dropped。
- `DrawSubmission` 记录 pipeline、program、cached texture0、base element、elements、instances、instanced API。
- `Counter` 默认 disabled；`configure(enabled, trace)` 清空并尝试预留 128 条 trace；预留失败则 `trace_enabled=false`，但计数仍可开启。
- 计数用 `saturating_add`，nominal work 饱和到 `u64::MAX`，不回绕。
- trace 上限 128；超出只增加 `trace_dropped_calls`，不扩容。
- 不查询 GL、不等待；统计的是 managed API 边界，不是物理 GPU pass/图元。

**有界采集（`bc1d9d4`）**

- `MAX_CAPACITY=32768`；`Journal.capacity` 可配置；`report.capacity` 返回实际逻辑容量。
- 启用时按请求容量预留；失败/非法容量保留旧 capture。
- ring 按 logical capacity 丢弃最旧帧并计数；不按 `VecDeque` 的物理容量。
- 默认 `configure` 恢复 2048；旧 epoch 的 token 不能提交半帧。

### 4. 具体实现（实现侧、文件、数据结构、算法）

实现侧全部是 miniquad：

- `eb0eda0bb48c229750ba9f154ccb101ca0c4f611`（2026-10-09，“记录原生帧循环”）：`lib.rs` +1、`native/android.rs` +16、`native_loop_timing.rs` +834。
- `e63917d47e206507ccaec438607d0abf0773af55`（2026-10-07，“加入提交诊断”）：`graphics.rs` +34、`submit_diagnostics.rs` +236。
- `bc1d9d425b4ca7ff0d01929bc58ab3552f266ce9`（2026-10-12，“支持有界原生帧采集容量”）：`native_loop_timing.rs` +155/-9。

macroquad：

- `50b5dbf25ddd274e14c5e7d95ab9b09ac8fbe709` → pin `eb0eda0`；
- `cc302a273cde1372a644ccbfe81abac4f2a0b00e` → pin `e63917d`；
- `161fab1f52d1b25be733c9e404f2fa16ec6123b7` → pin `bc1d9d4`。

这些都只改 `Cargo.toml`，没有 macroquad 诊断实现。

### 5. 完整核心代码

提交诊断的统计结构与计数器：

```rust
//! Optional managed-API submission diagnostics. No GL queries or GPU waits.
//! Counts API boundaries, not physical tile passes or executed GPU primitives.
use std::cell::{Cell, RefCell};

pub const TRACE_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubmissionStats {
    pub enabled: bool,
    pub trace_enabled: bool,
    pub native_draw_calls: u64,
    pub instanced_api_calls: u64,
    pub multi_instance_calls: u64,
    pub zero_work_calls: u64,
    pub invalid_shape_calls: u64,
    pub rejected_draw_requests: u64,
    /// Requested positive index count × instances, not processed GPU work.
    pub nominal_index_instances: u64,
    pub pass_begin_api_calls: u64,
    pub pass_end_api_calls: u64,
    pub framebuffer_bind_calls: u64,
    pub pipeline_apply_calls: u64,
    pub binding_apply_calls: u64,
    pub trace_dropped_calls: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawSubmission {
    pub pipeline: usize,
    pub program: u32,
    /// Managed texture cache, not a query of raw external GL state.
    pub cached_texture0: u32,
    pub base_element: i32,
    pub elements: i32,
    pub instances: i32,
    pub instanced_api: bool,
}

#[derive(Default)]
pub(super) struct Counter {
    enabled: Cell<bool>,
    trace_enabled: Cell<bool>,
    stats: Cell<SubmissionStats>,
    trace: RefCell<Vec<DrawSubmission>>,
}
impl Counter {
    pub fn configure(&self, enabled: bool, trace: bool) {
        self.enabled.set(enabled);
        let allocated = enabled && trace && {
            let mut data = self.trace.borrow_mut();
            data.clear();
            data.try_reserve(TRACE_LIMIT).is_ok()
        };
        self.trace_enabled.set(allocated);
        self.reset();
    }
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }
    pub fn reset(&self) {
        self.stats.set(SubmissionStats::default());
        self.trace.borrow_mut().clear();
    }
    pub fn stats(&self) -> SubmissionStats {
        SubmissionStats {
            enabled: self.enabled.get(),
            trace_enabled: self.trace_enabled.get(),
            ..self.stats.get()
        }
    }
    pub fn trace(&self) -> Vec<DrawSubmission> {
        if self.trace_enabled.get() {
            self.trace.borrow().clone()
        } else {
            Vec::new()
        }
    }
    #[inline]
    fn update(&self, f: impl FnOnce(&mut SubmissionStats)) {
        if !self.enabled.get() {
            return;
        }
        let mut s = self.stats.get();
        f(&mut s);
        self.stats.set(s);
    }
    pub fn begin(&self, framebuffer_bound: bool) {
        self.update(|s| {
            s.pass_begin_api_calls = s.pass_begin_api_calls.saturating_add(1);
            s.framebuffer_bind_calls = s
                .framebuffer_bind_calls
                .saturating_add(framebuffer_bound as u64);
        });
    }
    pub fn end(&self) {
        self.update(|s| s.pass_end_api_calls = s.pass_end_api_calls.saturating_add(1));
    }
    pub fn framebuffer_bind(&self) {
        self.update(|s| s.framebuffer_bind_calls = s.framebuffer_bind_calls.saturating_add(1));
    }
    pub fn pipeline(&self) {
        self.update(|s| s.pipeline_apply_calls = s.pipeline_apply_calls.saturating_add(1));
    }
    pub fn bindings(&self) {
        self.update(|s| s.binding_apply_calls = s.binding_apply_calls.saturating_add(1));
    }
    pub fn rejected(&self) {
        self.update(|s| s.rejected_draw_requests = s.rejected_draw_requests.saturating_add(1));
    }
    pub fn draw(&self, draw: DrawSubmission) {
        self.update(|s| {
            s.native_draw_calls = s.native_draw_calls.saturating_add(1);
            s.instanced_api_calls = s
                .instanced_api_calls
                .saturating_add(draw.instanced_api as u64);
            s.multi_instance_calls = s
                .multi_instance_calls
                .saturating_add((draw.instances > 1) as u64);
            s.zero_work_calls = s
                .zero_work_calls
                .saturating_add((draw.instances == 0 || draw.elements == 0) as u64);
            s.invalid_shape_calls = s
                .invalid_shape_calls
                .saturating_add((draw.instances < 0 || draw.elements < 0) as u64);
            if draw.instances > 0 && draw.elements > 0 {
                s.nominal_index_instances = s
                    .nominal_index_instances
                    .saturating_add(draw.instances as u64 * draw.elements as u64);
            }
        });
        if self.trace_enabled.get() {
            let mut data = self.trace.borrow_mut();
            if data.len() < TRACE_LIMIT {
                data.push(draw);
            } else {
                self.update(|s| s.trace_dropped_calls = s.trace_dropped_calls.saturating_add(1));
            }
        }
    }
}
```

原生帧循环的容量与配置：

```rust
pub const CAPACITY: usize = 2048;
pub const MAX_CAPACITY: usize = 32768;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub wall_ns: u64,
    pub thread_cpu_ns: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub id: u64,
    pub epoch: u64,
    pub begin_ns: u64,
    pub end_ns: u64,
    pub next_begin_ns: u64,
    pub begin_to_begin_ns: u64,
    /// Native begin through the finish hook after yield, including yield.
    pub frame_body_ns: u64,
    pub unsegmented_gap_ns: u64,
    /// Finish-hook end to next begin; excludes the small yield-end/finish gap.
    pub gap_after_yield_ns: u64,
    pub event_messages: u64,
    pub events: Option<Sample>,
    pub update: Option<Sample>,
    pub draw: Option<Sample>,
    pub swap: Option<Sample>,
    pub yield_phase: Option<Sample>,
    pub thread_cpu_total_ns: Option<u64>,
    pub unsegmented_thread_cpu_ns: Option<u64>,
    pub gap_after_yield_thread_cpu_ns: Option<u64>,
}

pub fn configure(enabled: bool, cpu_mode: u8) -> Report {
    configure_with_capacity(enabled, cpu_mode, CAPACITY)
        .expect("Failed to reserve default native-loop journal capacity")
}

/// Configures a bounded journal with a logical capacity in 1..=MAX_CAPACITY.
/// An enabled capture reserves storage before resetting its epoch or changing
/// any capture state. Invalid capacity or reservation failure leaves the
/// previous capture unchanged. Completed frames never reserve on the hot path.
pub fn configure_with_capacity(
    enabled: bool,
    cpu_mode: u8,
    capacity: usize,
) -> Result<Report, &'static str> {
    if !(1..=MAX_CAPACITY).contains(&capacity) {
        return Err("Native-loop journal capacity must be in 1..=32768");
    }
    STATE.with(|slot| {
        let mut s = slot.borrow_mut();
        if enabled && s.journal.frames.capacity() < capacity {
            let additional = capacity - s.journal.frames.len();
            s.journal
                .frames
                .try_reserve_exact(additional)
                .map_err(|_| "Failed to reserve native-loop journal capacity")?;
        }
        ACTIVE.store(false, Ordering::Relaxed);
        let epoch = match s.journal.epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                s.journal.overflowed = true;
                s.enabled = false;
                return Ok(s.report());
            }
        };
        s.journal.reset(epoch);
        s.journal.capacity = capacity;
        s.enabled = enabled && !s.journal.overflowed;
        s.cpu_mode = cpu_mode;
        s.cpu_requested = enabled && cpu_mode != 0;
        s.cpu_enabled = false;
        s.resolution = None;
        s.cpu_error = None;
        s.calibration = None;
        s.cpu_reads = 0;
        s.cpu_failures = 0;
        s.wall_reads = 0;
        s.last_cpu = None;
        s.origin = None;
        if s.enabled {
            match cpu_mode {
                0 => {}
                2 => s.cpu_error = Some("Forced missing thread CPU capability".into()),
                1 => {
                    match native_cpu(true) {
                        Ok(ns) if ns > 0 => {
                            s.resolution = Some(ns);
                            s.cpu_enabled = true;
                        }
                        value => {
                            s.cpu_error = Some(
                                value
                                    .err()
                                    .unwrap_or_else(|| "Zero thread CPU clock resolution".into()),
                            )
                        }
                    }
                    if s.cpu_enabled {
                        let mut pairs = [0u64; 256];
                        let mut complete = true;
                        for value in &mut pairs {
                            let wall = Instant::now();
                            s.wall_reads += 1;
                            let a = s.cpu();
                            let b = s.cpu();
                            *value = u64::try_from(wall.elapsed().as_nanos()).unwrap_or(u64::MAX);
                            s.wall_reads += 1;
                            if a.zip(b).and_then(|(a, b)| b.checked_sub(a)).is_none() {
                                complete = false;
                                break;
                            }
                        }
                        if complete {
                            pairs.sort_unstable();
                            s.calibration = Some(Calibration {
                                pairs: 256,
                                p50_ns: pairs[128],
                                p95_ns: pairs[243],
                                max_ns: pairs[255],
                                mean_ns: pairs.iter().map(|&v| v as f64).sum::<f64>() / 256.,
                            });
                        }
                    }
                }
                _ => s.cpu_error = Some("Invalid thread CPU mode".into()),
            }
        }
        ACTIVE.store(s.enabled, Ordering::Relaxed);
        Ok(s.report())
    })
}
```

帧边界与快照：

```rust
/// Returns completed records only. The last body-complete record is awaiting
/// its NEXT begin and remains excluded, together with a current in-flight frame.
pub fn snapshot() -> Snapshot {
    STATE.with(|slot| {
        let s = slot.borrow();
        Snapshot {
            report: s.report(),
            frames: s.journal.frames.iter().copied().collect(),
            dropped: s.journal.dropped,
            discarded: s.journal.discarded,
            incomplete: u64::from(s.journal.pending.is_some()),
        }
    })
}

pub(crate) fn begin_frame() -> Option<Token> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    STATE.with(|slot| {
        let mut s = slot.borrow_mut();
        let now = s.start()?;
        let token = s.journal.begin(now);
        if token.is_none() {
            s.enabled = false;
            s.cpu_enabled = false;
            ACTIVE.store(false, Ordering::Relaxed);
        }
        token
    })
}
pub(crate) fn begin_phase(phase: Phase) -> Option<Token> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    STATE.with(|slot| {
        let mut s = slot.borrow_mut();
        let p = s.journal.pending.as_ref().filter(|p| p.end.is_none())?;
        let (epoch, id) = (p.epoch, p.id);
        let start = s.start()?;
        Some(Token {
            epoch,
            id,
            phase: Some(phase),
            start,
        })
    })
}
pub(crate) fn end_phase(token: Option<Token>) {
    let Some(token) = token else { return };
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    STATE.with(|slot| {
        let mut s = slot.borrow_mut();
        if !s.journal.accepts(token) {
            return;
        }
        if let Some(end) = s.end() {
            s.journal.record(token, end);
        }
    });
}
pub(crate) fn finish_frame(token: Option<Token>) {
    let Some(token) = token else { return };
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    STATE.with(|slot| {
        let mut s = slot.borrow_mut();
        if !s.journal.accepts(token) {
            return;
        }
        if let Some(end) = s.end() {
            s.journal.finish(token, end);
        }
    });
}
```

Android 原生循环的挂钩：

```rust
    fn frame(&mut self) {
        let update_timing = crate::native_loop_timing::begin_phase(crate::native_loop_timing::Phase::Update);
        self.event_handler
            .update(self.context.with_display(&mut self.display));
        crate::native_loop_timing::end_phase(update_timing);

        if self.surface.is_null() == false {
            let draw_timing = crate::native_loop_timing::begin_phase(crate::native_loop_timing::Phase::Draw);
            self.event_handler
                .draw(self.context.with_display(&mut self.display));
            crate::native_loop_timing::end_phase(draw_timing);

            let swap_timing = crate::native_loop_timing::begin_phase(crate::native_loop_timing::Phase::Swap);
            unsafe {
                (self.libegl.eglSwapBuffers.unwrap())(self.egl_display, self.surface);
            }
            crate::native_loop_timing::end_phase(swap_timing);
        } else {
            crate::native_loop_timing::skip_phase(crate::native_loop_timing::Phase::Draw);
            crate::native_loop_timing::skip_phase(crate::native_loop_timing::Phase::Swap);
        }
    }
```

```rust
        while !s.quit {
            let frame_timing = crate::native_loop_timing::begin_frame();
            let events_timing = crate::native_loop_timing::begin_phase(crate::native_loop_timing::Phase::Events);
            while let Ok(msg) = rx.try_recv() {
                crate::native_loop_timing::event_message();
                s.process_message(msg);
            }
            crate::native_loop_timing::end_phase(events_timing);

            s.frame();

            let yield_timing = crate::native_loop_timing::begin_phase(crate::native_loop_timing::Phase::Yield);
            thread::yield_now();
            crate::native_loop_timing::end_phase(yield_timing);
            crate::native_loop_timing::finish_frame(frame_timing);
        }
```

### 6. 开关、默认值、能力探测与回退

- 原生帧循环：`configure(false, 0)` 默认关闭；`cpu_mode` 0=关闭、1=真实线程 CPU、2=强制缺失；不支持/失败时清空 CPU 字段并关闭 CPU 读数，不伪造 0。
- 提交诊断：`set_submission_diagnostics(enabled, trace)` 默认 `false,false`；trace 预留失败则只关 trace，不禁用计数。
- 有界采集：默认 2048；`configure_with_capacity` 接受 1..=32768；非法/预留失败保留旧 capture。
- 没有 GPU 能力探测；诊断不依赖 GL 扩展。
- 回退：诊断关闭时所有挂钩 early-return，不改帧操作；容量失败不改变旧状态；CPU 失败只影响 CPU 列，不影响墙钟。
- 观察开销：每次 begin/end 都有两次 clock 读数（墙钟 + 可选 CPU），128/2048/32768 的 journal 预留和 trace 预留发生在 configure，不在热路径。

### 7. 测试与测量（完整数据）

**测试**

- `native-loop-library-tests-final.log`：6 项测试通过，覆盖 forward period/stages/gap/inflight 排除、CPU 失败清空、reset mid-stage 旧 epoch、disabled/forced missing 不读时钟、stamp offset、ring 固定/丢弃/序列不 wrap。
- `submit-library-tests.log`：4 项测试通过；`submit-render-tests.log`：13 项通过（含 render lab / GPU records / cache）。
- `bc1d9d4` 新增 4 个容量测试：非法容量保留 active capture、较大容量不热分配、容量滚动/丢弃计数、默认 configure 恢复 2048 并拒绝旧 epoch。
- `thread-clock-tests.log`：4 项故障/非法 timespec/时代/禁用测试通过。

**诊断数据**

- `native-loop-v1-diagnostic-results.json`：1200 帧，应用间隔 p50 16.721ms，update p50 0.395ms，render p50 6.362ms，flush p50 0.628ms，持续 FPS 59.743。
- 该运行是诊断，不是 OFF/ON 性能对照；不能把“开启诊断后的 FPS”当作诊断收益/成本。
- 提交诊断没有应用级性能数据；它只提供计数/trace，不能证明 GPU pass 数或帧时间。
- 有界采集没有独立性能数据；它只影响 journal 容量和丢弃计数，不改变帧操作。

### 8. 判定：正/无/负/未完成 + 理由 + 置信度

判定：**非优化（诊断）**。

理由：

- 三项都只观察/记录，不减少 draw、纹理上传、pass 或 GPU 工作。
- 原生循环诊断能帮助归因，但自身有 clock 读取/内存预留开销；不能当优化。
- 提交诊断只统计 managed API 边界，不是物理 GPU pass；有界采集只影响诊断记录。
- 默认关闭，未进入生产路径。

置信度：实现高；诊断数据可信；不能用于性能收益结论。

### 9. 主线实施状态与参考实现状态

- **主线是否已实施：否（main）。** miniquad `eb0eda0`、`e63917d`、`bc1d9d4` 都不在 Phira-Pro `main` 所固定的生产库 rev 中；库 `master` 也没有这些诊断设施。
- **参考实现：已落地（诊断）。** macroquad `50b5dbf`、`cc302a2`、`161fab1` 只改 Cargo pin；没有 macroquad 代码。
- **参考实现默认：关。** 原生帧循环、提交诊断、有界采集均默认关闭。
- **是否有调用方：有（研究/诊断构建）。** native loop / submit diagnostics / capacity 由研究构建调用；没有生产默认启用。
- **收益标注：非优化（诊断设施）。** 诊断开关必须与性能 cohort 分开。

### 10. 接入 Phira-Pro 的步骤与风险

接入步骤：

1. 只在研究/诊断构建启用；不要在性能 A/B 的 control 和 candidate 中混用不同诊断。
2. 原生循环：在渲染线程 configure；用 `configure_with_capacity` 预留所需容量；snapshot 只取已完成帧。
3. 提交诊断：先 `reset`，再在 pass/draw/bind 边界调用计数；trace 上限 128，超出看 `trace_dropped_calls`。
4. 有界采集：按窗口长度选择容量；不要超过 32768；预留失败时保留旧 capture。
5. 记录设备、温度、频率、cohort、SO 哈希；不要把诊断数据与历史 cohort 混池。

风险：

- **观察开销**：clock 读取、journal 预留、trace 预留都会扰动；诊断 cohort 不能当无诊断性能。
- **归因错误**：draw/swap 墙钟包含驱动/呈现等待，不是 GPU 执行；submit 计数不是物理 pass。
- **容量**：32768 是逻辑上限；ring 丢弃/`incomplete` 必须一起报告，不能只报“平均”。
- **线程/context**：native loop 只在渲染线程 configure/snapshot；跨线程会读到不完整状态。
- **隐私/日志**：trace 只记录 pipeline/program/texture id，不记录用户内容；但日志量仍需限制。
- **不是优化**：不要把这些设施写进“性能提升”表；它们只用于解释/验证。

### 11. 来源与可追溯性

- miniquad 原生帧循环：`eb0eda0bb48c229750ba9f154ccb101ca0c4f611`，`native_loop_timing.rs` +834、`native/android.rs` +16、`lib.rs` +1。
- miniquad 有界采集：`bc1d9d425b4ca7ff0d01929bc58ab3552f266ce9`，`native_loop_timing.rs` +155/-9。
- miniquad 提交诊断：`e63917d47e206507ccaec438607d0abf0773af55`，`graphics.rs` +34、`submit_diagnostics.rs` +236。
- macroquad pin：`50b5dbf25ddd274e14c5e7d95ab9b09ac8fbe709`、`cc302a273cde1372a644ccbfe81abac4f2a0b00e`、`161fab1f52d1b25be733c9e404f2fa16ec6123b7`，均只改 `Cargo.toml`。
- 代码位置：`native_loop_timing.rs:12`（MAX_CAPACITY）、`:443`（configure）、`:458`（configure default）、`:467`（configure_with_capacity）、`:558`（snapshot）、`:571`（begin_frame）、`:631`（finish_frame）；`submit_diagnostics.rs:5`（TRACE_LIMIT）、`:47`（configure）、`:87`（begin）、`:110`（draw）。
- 数据：`native-loop-v1-diagnostic-results.json`、`native-loop-library-tests-final.log`、`submit-library-tests.log`、`submit-render-tests.log`、`thread-clock-tests.log`。

## X-01 库 master 已合入的排除项：共享 arena 与连续 pass

### X-01.1 结论

X-01 不再属于“未合入优化”。它的核心实现已经以 **squash** 形式合入两个库的 `master`：

- macroquad `master`：`139ac954c5654f4d83fc2bdeaf5a0c011fa9dee1`（“整理批处理与离屏渲染”）。
- miniquad `master`：`1796f445440edf6b66d4d3a8491f9ac1c53ba52a`（“优化渲染提交与能力探测”）。

但这只说明“库 master 已合入”，不等于“主线已实施”。Phira-Pro `main` 当前固定的生产库 rev 不包含这组能力，也没有完成主线验收；因此 X-01 不作为待接入优化，也不计入主线已实施清单，只作为“库 master 已有能力”的基线。

### X-01.2 合入事实

| 仓库 | 参考实现提交 | 与库 `master` 的关系 | 隔离 diffstat | 处理 |
|---|---|---|---|---|
| macroquad | `ce7949e` | 提交不是 `master` 祖先；核心以 squash 合入 `139ac95` | 4 文件 +231/-95 | 库 master 已合入，排除 |
| miniquad | `000ea93` | 提交不是 `master` 祖先；核心以 squash 合入 `1796f44` | 6 文件 +368/-7 | 库 master 已合入，排除 |

L-01..L-10、D-01 的实现提交都不在库 `master` 中；共享 arena/连续 pass 的代码等价物已在库 `master` 中，但未进入 Phira-Pro `main` 的固定 rev，所以仍不能按“主线已实施”统计。

### X-01.3 库 `master` 已验证的符号与位置

miniquad `master`（`1796f44`）：

| 符号 | 文件:行 | 作用 |
|---|---|---|
| `GraphicsContext::apply_bindings_with_offsets` | `src/graphics.rs:924` | 为每个 vertex buffer 增加 byte offset；共享 VBO 可超过 65536 顶点，单 draw 仍用 u16 局部索引 |
| `Buffer::update_orphaned` | `src/graphics.rs:1904` | `glBufferData(size, null, STREAM_DRAW)` + `glBufferSubData`，保留对象名与固定容量 |
| `GraphicsContext::supports_framebuffer_blit` | `src/graphics.rs:725` | 当前 context 的 blit 能力查询 |
| `capabilities::framebuffer_blit` | `src/graphics/capabilities.rs:20` | 版本 + 入口 + 扩展的实际能力探测 |

macroquad `master`（`139ac95`）：

| 符号/调用 | 文件:行 | 作用 |
|---|---|---|
| `QuadGl::retain_render_pass_on_flush` | `src/quad_gl.rs:671` | opt-in 保留离屏 framebuffer，默认 `None` |
| `Buffer::update_orphaned` 调用 | `src/quad_gl.rs:697`、`:698` | 每 flush 一次连续 VBO/IBO 上传 |
| `apply_bindings_with_offsets` 调用 | `src/quad_gl.rs:762` | 用 vertex byte offset 选择 arena slice |
| `telemetry::track_drawcall` 的 offset 参数 | `src/telemetry.rs:370` | capture 时记录实际 offset |

库 `master` 中**没有**以下符号（`git grep master` 无匹配）：

- `resident_geometry` / `resident_draws_issued`；
- `queued_texture_dependency`；
- `texture_reads` / `TextureReadToken`；
- `clear_resource_bindings`；
- `try_from_rgba8_immutable` / `PixelUnpackBuffer`；
- `update_discard`；
- `uniform_cache` / `set_uniform_cache_enabled`；
- `native_loop_timing` / `submit_diagnostics`。

这说明 L-01..L-10、D-01 不是“已经在库 master 里”的功能；只有 X-01 的 arena/连续 pass/offset/orphan 基线在库 master。

### X-01.4 库 master 核心代码：`apply_bindings_with_offsets`

```rust
    pub fn apply_bindings(&mut self, bindings: &Bindings) {
        self.apply_bindings_with_offsets(bindings, &[]);
    }

    /// Apply bindings with a byte offset for each vertex buffer.
    ///
    /// Missing offsets default to zero. Indices remain local to each vertex
    /// slice, so a shared buffer may contain more than 65536 vertices while
    /// individual draws still use u16 indices. This also works on GLES2.
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
                    }
                }
            }
        }
    }
```

### X-01.5 库 master 核心代码：`update_orphaned`

```rust
    /// Replace all buffer contents without preserving the previous data store.
    ///
    /// Use for streaming complete batches. Orphaning permits the driver to keep
    /// the old storage alive for queued draws instead of waiting for them. The
    /// object name and bindings stay valid; untouched bytes are undefined.
    /// Drivers may still synchronize or allocate, so benchmark on the device.
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

### X-01.6 与 L-07 的边界

- 库 `master` 的 arena/连续 pass 是**已合入基线**；L-07 的 `resident_geometry` 是**未合入**的另一个机制。
- 二者都使用 `apply_bindings_with_offsets`/`update_orphaned`，但：
  - 库 master arena 每 flush 上传 CPU 顶点/索引，按 pass 合并；
  - L-07 resident 不上传 CPU arena，只排入外部 bindings/index range。
- 因此不能用库 master arena 的微基准（64 draw 15.344→0.7375ms）来证明 L-07；L-07 的同 SO 数据是负收益。
- 参考实现还包含 `retain_render_pass_on_flush`、`begin_pass_avoiding_redundant_bind` 等实验；这些能力在库 `master` 中已有等价物（macroquad `:671`；miniquad `begin_pass_avoiding_redundant_bind`），不属于未合入清单。

### X-01.7 结论

- 统计“库 master 已有能力”时，可以引用 `139ac95`/`1796f44` 的 arena、offset、orphan、blit 能力探测；这只是基线，不代表 Phira-Pro 主线已实施。
- 统计“主线未实施 API”时，L-01..L-10、D-01 才是对象；其中 L-01/L-03/L-04 是潜在正但当前无稳定收益证据，L-05/L-07 是无收益/负收益，L-02/L-06/L-08/L-09/L-10 是正确性/基础设施，D-01 是诊断。

## 附录 A：判定与接入优先级

| 优先级 | 条目 | 原因 | 建议 |
|---|---|---|---|
| P0（正确性） | L-02、L-06 | 防止 GL 名字复用/裸句柄导致错误绑定或崩溃 | 先接入并做字体扩容、shader 热重载回归；不要求性能收益 |
| P1（治理/可观测） | L-08、L-09、L-10 | 为安全更新/省略提供保守查询 | 作为研究/诊断 API；默认不改变普通路径 |
| P2（潜在正，待验证） | L-01、L-03、L-04 | 有明确机制，但质量/性能未闭合 | 只在独立研究构建做同 SO ABBA；不默认启用 |
| P3（已证明无收益/负） | L-05、L-07 | 调用降但墙钟无收益；resident 普通 gameplay 更慢 | 保留研究记录，不进入生产默认 |
| 非优化 | D-01 | 诊断设施 | 只在诊断构建启用，与性能 cohort 分开 |
| 排除 | X-01 | 库 master 已合入（squash），未进入 Phira-Pro `main` 固定 rev | 作为基线引用，不计主线已实施 |

## 附录 B：复现与核对命令

以下命令只读，不修改仓库状态。提交哈希均为完整哈希或可唯一解析的短哈希；所有核对都按单个提交或库 `master` 快照进行。

```bash
git -C archived/prpr-miniquad show --stat 19e97bd 4d85ed3 d7dc3f4 4b26d71 9466727 406954d b7e1d66 1dfaa93 eb0eda0 e63917d bc1d9d4
git -C archived/prpr-miniquad show 19e97bd -- src/graphics/texture.rs
git -C archived/prpr-miniquad show 19e97bd:src/graphics/texture.rs
git -C archived/prpr-miniquad show 4d85ed3:src/graphics/pixel_unpack.rs
git -C archived/prpr-miniquad show 4b26d71 -- src/graphics.rs
git -C archived/prpr-miniquad show 9466727 -- src/graphics.rs src/graphics/uniform_cache.rs
git -C archived/prpr-miniquad show 406954d -- src/graphics.rs src/graphics/resource_arena.rs
git -C archived/prpr-miniquad show b7e1d66 -- src/graphics.rs
git -C archived/prpr-miniquad show 1dfaa93 -- src/graphics.rs
git -C archived/prpr-miniquad show eb0eda0 -- src/native_loop_timing.rs src/native/android.rs
git -C archived/prpr-miniquad show e63917d -- src/graphics/submit_diagnostics.rs
git -C archived/prpr-miniquad show bc1d9d4 -- src/native_loop_timing.rs
```

```bash
git -C archived/prpr-macroquad show --stat 4b46e86 de34611 7068495 51147e9 27cc446 461ad5f ed1db13 cd0ab01 a8aace0 c862fc8 50b5dbf cc302a2 161fab1
git -C archived/prpr-macroquad show de34611 -- src/quad_gl.rs
git -C archived/prpr-macroquad show 7068495 -- src/quad_gl.rs
git -C archived/prpr-macroquad show 51147e9 -- src/texture_reads.rs src/quad_gl.rs
git -C archived/prpr-macroquad show 27cc446 -- src/quad_gl.rs
git -C archived/prpr-macroquad show 461ad5f -- src/quad_gl.rs
git -C archived/prpr-macroquad show --stat 139ac95
git -C archived/prpr-miniquad show --stat 1796f44
```

核对库 `master` 符号：

```bash
git -C archived/prpr-miniquad grep -n "apply_bindings_with_offsets\|update_orphaned\|supports_framebuffer_blit" master -- src
git -C archived/prpr-macroquad grep -n "retain_render_pass_on_flush\|update_orphaned\|apply_bindings_with_offsets" master -- src
git -C archived/prpr-macroquad grep -n "resident_geometry\|queued_texture_dependency\|texture_reads" master -- src
```

## 附录 C：术语与口径

- **主线已实施**：只指 Phira-Pro `main` 已合入并完成验收；库级项还必须进入 `main` 所固定的生产库 rev。参考实现存在、库 `master` 已合入，都不等于主线已实施。
- **库 master 已合入**：只描述 macroquad/miniquad 的 `master` 状态，用于区分代码来源；不代表 Phira-Pro `main` 已固定该 rev，也不代表完成验收。
- **参考实现**：本地参考代码及其叠放提交链，不是生产 pin；文中用提交哈希、文件路径和隔离 diffstat 定位。
- **墙钟/wall**：包含 CPU 执行、驱动调用、可能的驱动等待；不是纯 GPU 时间。
- **completed**：如果带 `glFinish`，是同步完成时间，串行化 CPU/GPU；不能当 FPS。
- **packed uniform calls**：只统计 `apply_uniforms_from_bytes` 里的字段级 `glUniform*`；sampler setter 不计入。
- **nominal bytes**：按 API 传入的字节数统计，不是驱动的物理分配/带宽。
- **resident draw**：调用方拥有 GPU buffer、通过 `resident_geometry` 排入队列的 draw；不是库 `master` 的 CPU arena draw。
- **managed context id**：miniquad/macroquad 自己生成的 context 身份，不是 EGL/GL context 存活检测。
- **Declined**：安全拒绝，可以回退到旧构造；**Err**：致命错误，不可静默回退。
- **正/无/负/未完成**：本文的判定单位是“该条目是否在已有应用级数据中表现出可归因收益”，不是库层实现是否存在。
- **潜在正 / 当前无收益证据**：前者只表示机制上可能有利；后者表示已有 cohort 没有可归因的稳定收益。两者都不能写成已实现收益。
