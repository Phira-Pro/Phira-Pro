# 官方噪域复刻：实现与验证（2026-10-03，第三轮）

## 本轮收尾：精度、噪声色调与触摸低通

1. **边界精度。** 官方 BlockCompose 的 `u_xlat16_*` 中间量在实际 GPU 上是 binary16；例如归一化方向读回为 `0.70703125`，此前 CPU 使用 `0.70710677`。本轮按原生操作顺序分别舍入逆长度、方向、纹理 R 与减去 0.5 后的值，保留后续坐标 highp。原先 10 组独立遮罩测试中的 Compose / Edge / Glow 均变为零差异。原始 subtract 源的每次 UNorm 混合仍有驱动量化差异，不能将这些测试外推为全部分辨率、时刻与设备的像素一致。
2. **RGB565 导出误差。** 原始 FD_Noise_00000 是 RGB565，旧导出 PNG 将通道位值左移 `(3,2,3)`，没有补全低位。已逐一检查原 APK 的全部 65536 个 texel，按 `(bits << shift) | (bits >> (2,4,2))` 重新生成应用 PNG。R/G/B 最大补正为 7/3/7。独立 GPU oracle 直接上传 APK 原始 RGB565；新的 hover 与它逐通道零差异。没有修改 Fill/Edge/Glow 的官方颜色常量或额外套 gamma。
3. **音乐低通。** 原生 `JudgeControl::UpdateLowPassFilterState` 仅在「是否有任意被拦截触点」变化时调用 `ProgressControl::SetLowPassFilter`。LevelControl 的序列化值为 **1500 Hz、0.1 秒**，覆盖构造函数默认的 0.5 秒；原始字段在 level12 / pathID 195 的字节 504/508。Start 将滤波器附加到谱面音乐的 AudioSource。释放从当前截止频率线性过渡到 **22000 Hz**，然后关闭；快速移入移出会中断并从当前值继续。首次触摸直接启用保存了 1500 Hz 的滤波器；后续触摸使用 0.1 秒过渡。
4. **音频接入。** `core/block_audio.rs` 接到已有 `blocked_touches`；多指时最后一根离开才释放。sasa 新增独立双声道的二阶共振低通，Q=1（[Unity 默认值](https://docs.unity.cn/2022.1/Documentation/Manual/class-AudioLowPassFilter.html)），在音频线程按真实输出采样率处理，非阻塞命令满时下帧重试。暂停/重试清除滤波；seek 清空历史值，暂停时也发布新的播放位置。滤波只作用于 music stream，命中音效没有经过它。Unity 私有 DSP 内核没有源代码，也没有带声音的官方参考录像，所以不声称音频逐样本一致。

最终验证：`cargo check -p prpr --lib` 通过；prpr **41/41**、sasa **6/6**；生产材质与导出原生 GLSL 共 **11 组**对照为零差异，新增红/绿/蓝等彩色底图，并使用原生 /3、Point 场景色 RT。原有几何文件 SHA256 未变。Windows record Release 已重新构建并交付 `E:\Phira Pro\PhiraPro-block-area-win64-v3.zip`。

边界动画继续使用 Unity 风格的全局运行时钟，各 pass 共用同帧时间；不把谱面 seek 当成运行时钟 seek。独立启动的两份录像仍可能有整体相位差。整帧对照中的背景、UI、粒子与亮度差异仍存在，不能用上述共同输入 GPU 验证宣称整帧 100% 一致。Android/iOS 实机未验证。

`audit_official_assets.py` 只读原 APK，重生成项目内噪声 PNG 和证据；`audit_native_audio.py` / `audit_audio_assets.py` 保留原始反汇编与序列化参数。GPU probe 中的额外 FBO 仅在隐藏测试程序内使用；生产噪域没有新建离屏 pass。

以下是第二轮实现记录；旧测试数量、残余误差与 v2 的相位/颜色假设审计属于历史数据，以本节最新验收记录为准。

## 第二轮实现记录

本轮修正区域合成、溶解边界、渲染顺序，并补齐 Ready 与触摸 hover。`block.rs` 的几何、缓动与输入奇偶规则保持不变；SHA256 仍为 `988A4215EEBEA146DD418287B2CBDD3472566AC166A73517E23EC6579591B29E`。`judge.rs` 仅为被拦截的触点保留 finger ID，供视觉动画使用。

## 修正的差异

1. **画面覆盖不能直接复用判定 XOR。** 原生 BlockSprite 是 SrcAlpha/One，subtract sprite 的 alpha 固定为 0.1。SubtractBlockBlender 将累计 R 落在 `[0.09,0.12)` 的像素映射成 1；两层或三层 subtract 都不会映射成 1。输入判定仍保留已经验证的奇偶语义。
2. **启用和未启用图层独立合成。** Enabled Normal/Subtract 生成活动 Compose；Disabled Normal/Subtract（含 Ready）生成未启用 Compose。旧版先对全部可见块 XOR 再分 active，会让未启用 subtract 错误剪掉活动区域。原生 Disabled pass 使用 `abs(postSubtract.r * postSubtract.g - normal.r)`。
3. **溶解在 Compose 阶段发生。** 活动遮罩先按 BlockCompose 的两路位移重采样，再生成外侧 Edge 和 Glow。此前对规则矩形直接膨胀，只能产生平直像素边缘。位移使用 BlockNoise1 的 R，而不是灰度亮度。
4. **PNG 行方向。** 导出图片是从上到下的行，macroquad 原样上传；Unity GLES 的纹理坐标从左下开始。本轮将三张材质贴图上下翻转后上传，CPU 位移也使用相同方向。参考 [Unity 平台渲染差异](https://docs.unity3d.com/ru/2021.1/Manual/SL-PlatformDifferences.html)。
5. **混合与顺序。** ActiveBlock 使用 One/OneMinusSrcAlpha；DisabledBlock 使用 One/One。Disabled 在 Background sorting layer order 2，判定线 order 3；活动噪域由 AfterForwardAlpha 的后处理执行，放到音符、特效与游戏 HUD 之后，采样完整底图。旧版的透明混合与音符前绘制不符合这条管线。
6. **区域数量。** 删除了旧全屏 shader 的 `bp[16]/bq[16]` 上限，全部可见区域参与相机源合成；只剔除几何已经缩到零的不可见区域。ハテ的密集 note 形开口不再受 16 个区域限制，几何变换仍由原 `BlockArea::transform` 提供。

`official-asset-state.json` 保存从原 APK 中解析的 shader blend state、property 类型/flags、材质全部参数、三张纹理的格式/过滤/wrap/通道哈希、版本、资源哈希与颜色空间。ActiveBlock 的原始序列化参数与本轮采用的 .mat 数值一致。PlayerSettings 的 `m_ActiveColorSpace=0`（Gamma），因此没有凭录像色差擅自进行线性色彩换算。

还发现了导出壳的类型错误：壳将 Edge/Fill/Glow/Spark 显示成 Vector，原 APK 序列化 `m_Type=0` 实际为 Color（flags=0）。[Unity C# 参考源码](https://github.com/Unity-Technologies/UnityCsReference/blob/master/Runtime/Export/Shaders/ShaderProperties.cs) 的枚举注明与 ShaderLab 序列化类型同步。以原始数据为准，后续审计不能依赖壳的类型。当前采用的是原 Android Gamma 配置下的材质数值；[Unity 颜色空间文档](https://docs.unity3d.com/2022.3/Documentation/Manual/LinearRendering-GammaTextures.html) 也区分 Gamma 与 Linear 的采样和输出处理。真实 GLSL 来自 `_official_src/shader_code`，不是 DummyShaderTextExporter 壳。

## Ready 与 hover

- Ready 选择启用前最后 0.5 秒的原生 Ready-only 图层。Active shader 的 `_DisabledNormalBlockRT` / `_DisabledSubtractBlockRT` 命名容易误导：Start 实际绑定的是 Ready-only 相机输出；`_ReadyComposeRT` 则绑定全部 Disabled Compose。
- 初次未启用显示的 0.5 秒动画：普通块渐变 alpha；subtract 固定 alpha 0.1，渐变 G。启用后再 disable 的块不重复初次 Show 动画。
- Ready 的 Shine 参数取 ReadyBlock.mat，原生完整 Active GLSL 中的 Shine 分支保留。
- Hover 使用原始 Round10_Blur4 的 44×44 模糊圆 sprite，100 PPU、size 11.5、相机高度 10。Show 从 0 放大，Hide 从当前尺度缩到 0，均为 0.1 秒。
- 模糊圆写到 1/8 分辨率点采样 R8 的等价 CPU 遮罩；完整 Active GLSL 继续执行 NoiseMap、SDF、闪光和触点局部增亮。没有用简单圆形光晕替代 SDF。
- 最多 10 个触点；以 finger ID 保持生命周期和稳定顺序。换谱、重试时清除 hover，保留已分配的纹理。Disabled 与 Active 共用帧内动画时钟及遮罩缓存。

## 两张谱面的遮挡方式

ハテ不是对所有被噪域盖到的 note 自动挖空。官方 JSON 自己编排了 note 形的普通块，在整屏 subtract 中形成开口。例如 138.8s 的 12627/12628 块中心 X=0.27/0.435，尺寸为 0.15×0.04；对应 line 22 的 note X≈0.2703/0.4344。该阶段 note 在屏幕外，开口消失后真实音符接续。没有发现 JSON 中另一个音符挖空 shader 开关。

Desultory Signals 的 750 combo 附近是整屏 subtract 加普通矩形开口。音符在活动合成的底图中，随 SceneColor 的像素量化与位移发生边缘溶解。两种外观由同一官方管线及不同谱面编排产生；本轮保留这些差异。

## 渲染文件与约束

- `prpr/src/core/block_shader.rs`：两种 blend 的全屏材质、typed f32 参数、帧内缓存、场景拷贝与触点上传。
- `block_shader_full.vert` / `.frag`：完整 ActiveBlock、Ready、Hover、Disabled 原生运算次序与精度；仅适配输入、纹理打包和输出。`build_full_fragment.py` 可重新生成，已核验生成文件哈希不变。
- `block_mask.rs`：6 个相机源的 CPU 等价、两种 subtract 后处理、Compose 位移、9-tap 最大值膨胀、外侧 Edge、五次有效 Glow。二值遮罩用 bit rows；部分透明遮罩用灰度路径。删除了只在旧测试中使用的 parity 渲染替身，回归直接调用生产实现。
- `block_touch.rs` 与 `assets/blockarea/TouchHover.png`：原生 hover 相机源与动画。
- 不创建新的生产离屏 FBO，不在噪域绘制中调用 set_camera。macroquad flush 结束会恢复默认 framebuffer，因此拷贝底图时按逻辑 render pass 显式绑定来源，随后恢复 framebuffer 与纹理状态；直接复制 flush 后的当前 framebuffer 会读错底图。只有测试中直接从 MSAA input 调用时，使用已有 MSRenderTarget resolve 并恢复读写 FBO。普通后处理拷贝不依赖 GLES3 READ/DRAW framebuffer API。
- 场景色用当前 viewport 的普通纹理拷贝，再在 shader 内采样原生屏幕 /3 RT 的 texel 中心。分辨率不能整除 3 时，按低分辨率 blit 的线性中心采样处理。
- 自定义 sampler 为 6 个，加 macroquad 保留的 2 个，共 8 个。

## 验证与误差界限

- `cargo test -p prpr --lib`：38 项通过；保留原有几何回归，新增图层隔离、三层 subtract、Ready 输入、位移与初始 Show、hover 生命周期等回归。
- `cargo check -p prpr --lib`：通过。
- `block_area_gpu` 使用实际生产 renderer。Windows NVIDIA 原生 GL 下，对照完整导出 ActiveBlock GLSL：矩形两个时刻、反相四开口、普通 Ready、subtract Ready、多触点 hover 两个时刻、Hide 完成与换谱 reset，RGBA 最大通道误差均为 **0**。输入贴图和遮罩相同，所以这证明材质运算一致，不能证明整帧一致。
- 额外验证 note 底图受到活动合成、后续绘制恢复默认材质、1x/4x MSAA 手动 pass、letterbox、同帧 resize、Chart Y 翻转及 flip_x。MSAA 回归先画黄色音符，再 resolve、切到已有 output pass、执行噪域，并检查 SceneColor 是否实际读到黄色音符。无 GL 错误或渲染崩溃。
- `block_area_mask_gpu` 是独立遮罩审计：只在隔离的测试进程中使用 FBO，运行原始 BlockSprite、SubtractBlockBlender、BlockCompose、EdgeMask、GlowMask。Disabled Compose 与测试中的普通源栅格化零差异；活动 Compose 在 120×67 遮罩中最多 **4 个**像素不同，Edge 在 240×134 中最多 **22 个**不同，Glow 最多 **78 个**不同。原始 subtract R 的 UNorm8 混合每层约差 1，阈值后的有效区域在这些测试中保持一致。审计明确保留每次误差，允许的回归上界不代表像素一致。CPU/GPU 插值、点采样边界和驱动量化可能造成这些残余误差，尚未全部消除。
- `block_area_frames` 调用真正的 GameScene、正式背景加载器、音符/HUD/MSAA/后处理，用隐藏窗口输出两张实际谱面的完整 960×720 / 1920×1440 帧，读取图像时不污染 sampler 状态。测试不播放音乐、不写用户设置。

日志和原始图在 `target/block-area-*.log`、`target/block-area-gpu/`、`target/block-area-frames*/`。

## 官方录像整帧对照

读取用户提供的 `E:\Phira Pro\Desultory Signals.mp4` 和 `E:\Phira Pro\ハテ.mp4`，FFmpeg 解码到原尺寸 960×720。`reference_video.py` 保存实际帧 PTS；捕获程序以该 PTS 作为谱面时刻。例如请求 67.72277s 的视频帧实际 PTS 是 67.733312s，不能直接声称请求时刻就是视频帧时间。

完整并排图在 `E:\Phira Pro\block-area-comparison\`，包括 Desultory 65/67/67.72277/70/71.8s、ハテ 134.96667/137/138.8/139.8/141.8s。左为录像，右为完整 GameScene 捕获，未裁剪；1920×1440 捕获仅在对照图中缩小。

已经对上整屏反相、开口、note 形开口、溶解边界与音符/HUD 遮挡结构。**整帧仍非逐像素一致，色调差异明显**：录像的游戏版本、源设备渲染分辨率、Unity 启动时钟与背景亮度尚未确认，双方 UI、资源包及粒子也不同。这些因素可能解释部分差异，但不能据此排除剩余实现错误。边界相位和色调仍需进一步定位；当前捕获按真实 GameScene 的启动时钟运行，跳到指定谱面时刻不会跳 Unity 的动画时钟。对照图标明了时钟未对齐，不能用它判断每个噪声像素是否一致。

三张贴图重新从原 APK 校核：BlockNoise1 / PointNoise 的导出 RGBA 字节完全相同；FD_Noise_00000 原格式为 RGB565，不同导出器的 5/6 位到 8 位展开最多差 7（R/G 最多 6/2）。这种输入量化也尚未构成独立的像素级 hover 证明。没有通过随意改材质参数隐藏差异，也没有将这些图当成“100% 官方完成”的证据。Android/iOS 真机验证尚未进行。

额外做了 `block_area_phase_audit` / `compare_texture_color_space.py`：分别取两段录像的三个时刻，尝试共同的 Compose 时钟偏移；也比较原始采样和 sRGB 解码位移贴图的假设。共同相位未能稳定解释各帧；sRGB 假设在 Desultory 上仅将颜色阈值遮罩差异从 9901 降到 9816 个采样格，在ハテ上反而从 7588 增至 10545。它没有形成支持修改生产颜色空间的证据。这里的录像遮罩由 RGB 阈值估计，含 glow 并可能漏掉填充；这些数值不是官方真实 Compose 的误差。原始相机输入、红蓝差异图和 JSON 均保留在 `target/block-area-phase*` / `target/block-area-texture-color-space-audit*`；没有将最佳拟合值写进游戏时钟。

## 复现

在 `E:\Phira Pro\phira` 执行：

```powershell
cargo test -p prpr --lib
cargo check -p prpr --lib
$env:RUSTFLAGS='--cfg record'
cargo build --release -p phira-main
cargo build --release -p prpr --example block_area_gpu --example block_area_mask_gpu --example block_area_frames
& '.\target\release\examples\block_area_gpu.exe'
& '.\target\release\examples\block_area_mask_gpu.exe'
$env:BLOCK_CAPTURE_SCALE='2'
& '.\target\release\examples\block_area_frames.exe'
```

`reference_video.py` 和 `compare_video_frames.py` 使用现有 FFmpeg 与 Python/Pillow；正式应用构建不需要它们。`audit_official_assets.py` 使用可选 UnityPy 只读解析原 APK，非应用依赖。

额外的定位工具：

```powershell
cargo build --release -p prpr --example block_area_phase_audit
& '.\target\release\examples\block_area_phase_audit.exe'
# 使用配置好的 Python/Pillow/NumPy 执行 compare_texture_color_space.py
```

## Windows 包

`E:\Phira Pro\PhiraPro-block-area-win64-v2.zip` 包含最新 record 配置 Release 的 `phira-main.exe`、完整 assets、此说明和对照图。解压后保留 exe 与 assets 相邻。已有测试谱面仍在用户 data/charts 下，没有覆盖它们或用户设置。
