# rpe-next 噪域兼容说明

核对对象为 [rpe-next 0.10.2，提交 78dfc8b](https://github.com/kclg-YSJ/rpe-next/tree/78dfc8b1748ad01a18d5f85767bd50c7bff50d38)。以下区分上游格式、应用中的适配行为和验证范围，不把编辑器预览当作 Phigros 的官方实现。

## 导入与格式识别

可直接导入 rpe-next 以 Next 兼容模式导出的 PEZ。JSON 可作为本地谱面文件使用，仍需配套音乐、曲绘与谱面信息。无需改成 Line2Area 图片线或另外生成原生 BlockAreaList。

Next 噪域位于 RPE JSON 根节点 `blockAreaList`，`META.RPEVersion` 至少为 220。这个名称与原生百分比格式相同，结构却不同：

| 项目 | rpe-next | 原生 BlockAreaList |
|---|---|---|
| 矩形 | `bottomLeft`、`topRight`，1350×900 画布单位 | `bottomLeftPercentage`、`topRightPercentage`，屏幕百分比 |
| 时间 | 全谱拍数 `[整数, 分子, 分母]`，经 BPMList 换算 | 秒 |
| 激活 | 多段 `activeIntervals` | `enableTime` / `disableTime` |
| 反转 | `isInvert` | `isSubtract` |
| 动画 | X/Y 移动、旋转、X/Y 缩放独立事件轨道 | 原生移动、旋转、缩放关键帧 |

解析按矩形字段识别，不能仅看版本号或数组名称。未知或同时包含两套矩形字段的元素报错，并提供 `blockAreaList[index]` 位置；不会悄悄丢弃噪域。原生嵌入列表、外置列表和 Line2Area 保留原有解析路径；显式嵌入列表优先于外置列表。

依据：[格式定义、校验与运行时](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/core/noise-areas.mjs)、[JSON/PEZ 导出](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/platform/files.mjs)。

## 时间与几何

- `appearTime` / `disappearTime` 决定显示生命周期。显示、激活均为左闭右开；alpha 轨道不参与生命周期。
- 激活区间裁切到生命周期；内部动画事件保留原时间，允许先于出现时间开始。
- `readyFlash` 单位是秒，默认 0.5；`true` 为 0.5，`false` 为 0，数字取非负并四舍五入到毫秒。预警期间不阻挡触点。
- 拍数依据全谱 BPMList 换算，不乘判定线的 `bpmfactor`。谱面音频 offset 仍由游玩场景统一处理，噪域不再重复加减 offset。
- 移动轨道是画布中的绝对中心坐标。第一事件前采用原始中心，事件结束后保留末值，空档不自行回到原点。
- 旋转为顺时针角度。锚点在画布全局坐标中；中心按 X 缩放、Y 缩放、旋转的累计变换计算，矩形四角按当前绝对缩放和旋转计算，最后叠加移动。不能把四角全部套入累计矩阵来代替这两个步骤。
- 保留 `transformStart` / `transformEnd` 参考值与 `transformStep` 瞬时链。相同起点的合法瞬时链使用稳定顺序。支持负缩放；零面积不显示、不阻挡触点。
- 支持 1～29 号缓动、`easingLeft/Right`、Bezier。Bezier 对 X 求逆后取 Y，不使用左右裁切值；未量化控制点以合并曲线。

依据：[噪域采样](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/core/noise-areas.mjs)、[事件采样](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/core/events.mjs)、[缓动](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/core/easing.mjs)、[BPM 换算](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/core/tempo.mjs)。

## 显示与游玩适配

噪域在画布内完成旋转和缩放，然后使用现有 RPE 谱面视口映射：`x = 2X/1350`、`y = 2Y/(900×aspect)`。因此拼接和空隙在不同宽高比中保持相同的画布位置，触点使用同一矩形几何。用户设置的谱面比例和留边仍生效。rpe-next 编辑器预览使用等比裁切的 viewport；这里没有改动所有 RPE 音符的投影，因此非 3:2 视口不承诺与编辑器截图逐像素一致。

分别合成 disabled、ready、active 三组。组内普通矩形取并集，反转矩形默认按奇偶覆盖做 XOR。`noiseAreaOptions.ignoreTripleInversion=true` 时，反转覆盖只在计数恰好为 1 时成立，2 次及以上取消；不能把这个选项解释成仅取消第 3 次。最终组覆盖为「普通并集 XOR 反转覆盖」。高状态覆盖先移除低状态材质，再显示自身，避免半透明填充重复变暗。

显示使用现有官方噪域材质路径：`BlockNoise1.png`、`PointNoise.png` 与 `FD_Noise` 纹理，以及 Compose 位移、Edge/Glow 遮罩、场景折射、辉光和触点 Hover。相邻同状态噪域先合成并集，内部不产生独立边框。Disabled 底色在判定线前绘制，Ready/Active 官方效果在 HUD 后绘制，沿用现有原生噪域的合成顺序。

`readyFlash` 决定预警持续时间，预警亮度由现有官方 Ready 材质计算，不使用 Line2Area 的额外 beta3 亮度补偿。上游预览的 `(1+cos(phase×6π))/2` 混合量只保留为解析采样对照数据，不用于显示。rpe-next 的简易红色矩形、预览配色与滤镜均不作为官方材质参考。即使打开旧版/简易噪域选项，Next 噪域仍使用完整材质；材质编译失败时加载报错，不退回简易矩形。

遮罩沿用原生材质的网格尺寸 `max(1, floor(viewport/8)) × 2`，约为视口的四分之一分辨率。输入判断使用连续几何，独立于遮罩纹理。动画和材质均采样谱面时间，支持暂停和前后跳转。

只有 active 组会阻挡触点。普通/反转覆盖和反转选项同时用于显示及输入。沿用现有触点边界余量、感染直到抬手的规则和阻挡音频处理；两套判定算法在分支前共用拦截流程。暂停、回放和练习跳转按谱面时间重新采样，不依赖系统时间。原生与 Next 格式的反转各自在本组内计算，若手工混用则两组的最终阻挡结果取 OR。

依据：[上游预览合成](https://github.com/kclg-YSJ/rpe-next/blob/78dfc8b1748ad01a18d5f85767bd50c7bff50d38/src/ui/noise-preview.mjs)。上游没有游玩判定器；触点处理属于本应用适配，并非上游定义的官方判定协议。本次只接入噪域，不代表支持 rpe-next 的全部新增轨迹、特效功能。

## 可重复验证

固定上游提交，运行其公开 API 生成参考数据和真实导出包；仓库只保存生成的数据，不复制上游实现代码。

```powershell
git clone https://github.com/kclg-YSJ/rpe-next target/rpe-next-audit
git -C target/rpe-next-audit checkout 78dfc8b1748ad01a18d5f85767bd50c7bff50d38
node scripts/verify_rpe_next.mjs target/rpe-next-audit
$env:RUSTFLAGS='--cfg record'
cargo test -p prpr --lib --no-default-features
cargo test -p prpr --lib --no-default-features next_export_filters_real_touches_in_both_judgement_engines -- --ignored --test-threads=1
$env:BLOCK_GPU_NEXT='1'
cargo run -p prpr --example block_area_gpu --no-default-features
Remove-Item Env:BLOCK_GPU_NEXT
python scripts/verify_rpe_next_frames.py
```

- 61 个上游合法噪域、2,074 个采样点，核对六种视口比例的中心、四角、状态和闪烁混合量；逆序采样验证跳转无历史依赖。
- 包括 BPM 变化、音频 offset、非 1 判定线 bpmfactor、多激活区间、预警关闭/自定义秒数、变锚点、同拍瞬时链、变换参考值、零/负缩放、全部缓动及裁切、Bezier、生命周期外事件。
- 遮罩与连续几何逐像素核对，覆盖反转两种模式、三层优先级、相邻拼接、普通重叠不变暗和暂停时缓存复用。
- Next 单噪域的三种状态与原生材质对照，包含不同宽高比、反向跳时和简易选项开启；16:9 Active 效果另与提取的官方着色器参考比较。反转 Ready 遮罩允许 R/G 编码不同，但核对着色器实际使用的 `abs(R-G)`。
- 真实 Judge 输入回归覆盖两套判定 × 三种比例 × disabled/ready/active，确认被挡触点不会命中下面的 Tap。
- 实际 GameScene 分别加载 JSON 配套目录和上游 PEZ，重复渲染并比较画面哈希、几何和官方亮边。结果和截图留在 `target/rpe-next-verification`。不会用编辑器的红色矩形逐像素验收官方噪声材质。

桌面 OpenGL 验证不能替代 Android/iPad 的真机验收。保留参考生成脚本和固定提交，后续上游格式变化可重复比较。
