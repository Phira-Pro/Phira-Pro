# Phira Pro

Phira Pro 是基于 [Phira](https://github.com/TeamFlos/phira) v0.8.2 的**非官方改版**。

它在尽量不改变原有玩法与手感的前提下，补充了大量**练习、调试、外观自定义与本地成绩管理**相关的功能，适合练习高难谱面、研究判定手感或自建外观。

> **本项目为非官方改版，与 TeamFlos / Phira 官方没有任何关系。**
> 使用本改版产生的一切后果由使用者自行承担，请勿将本改版的问题反馈给官方。

## 下载

在 [Releases](https://github.com/Phira-Pro/Phira-Pro/releases) 页面下载 Windows、macOS、Linux 压缩包、Android arm64 APK 或 iOS 未签名 IPA。Windows 解压后运行 `phira-main.exe`，macOS / Linux 运行 `phira-main`；Android 更新时保留应用数据，iOS IPA 需自行签名安装。

当前版本为 **pro.15**（应用内 `0.8.2-pro.15`，build 72），新增设置搜索、备份预检与选择性还原、回放批量管理、画质与问题诊断、练习分析与双回放对照、谱库排序筛选、课题模式与课题成绩历史，以及主页音乐播放列表。课题模式包含三首连续游玩、黑场过渡、Saira 字体结算和五种课题牌子；牌子页取消与确定均返回主页。包含 Rulaifozu 的 PPSR 插件 API 更新及 Terrysquid 的判定与谱面兼容修复，见 [pro.15 更新说明](docs/development/pro-15.md)与 [pro.15 Release](https://github.com/Phira-Pro/Phira-Pro/releases/tag/v0.8.2-pro.15)。

插件接口见 [开发说明](docs/plugins/README.md)，模块职责与审查方法见 [代码审查指南](docs/development/code-review-guide.md)。本版继续包含 [rpe-next 噪域兼容](docs/development/rpe-next-noise.md)、[回放录制修复](docs/development/replays.md)及各平台帧率设置。历史更新见 [pro.14-fix1](docs/development/pro-14-fix1.md)、[pro.14](docs/development/pro-14.md)、[pro.13](docs/development/pro-13.md)和 [pro.12](docs/development/pro-12.md)。版本维护、五个平台构建与 Actions 使用方法见 [构建说明](docs/development/build.md)。

当前版本为 **pro.15 / build 73**，优化课题结算分数、判定详情及 Early/Late 的字号和对齐，按卡片高度适配手机与平板。

## 相对官方版新增的功能

### 判定与手感

- **判定设置与预设**：从「设置 → 谱面 → 判定设置」进入独立页面，直接调整当前设置，也可添加、命名、编辑和保存预设。内置「Phigros本家判定」「Phira Pro判定」「细致判定」。
- **提前／延后独立区间**：各档分别设置两侧阈值，例如 Perfect+ −16ms / +20ms；同时提供匹配、保护、Hold 和严判选项。
- **判定颜色**：各档独立设置 RGB 颜色，随预设保存，应用于打击粒子、偏移条和结算名称；留空恢复资源包颜色。
- **Perfect+ 开关**：关闭时合并到 PERFECT，同时禁用理论值分数。
- **细致判定**：PERFECT+ → PERFECT → GREAT → GOOD → OK → MEH → BAD → MISS；两列结算及历史详情保留各档计数。
- **osu!mania ScoreV2 OD 快捷区间**：选择整数 OD −15～15 自动关闭 Perfect+ 并应用随 OD 变化的 PERFECT 等判定区间，随后可继续手动调整。实际半毫秒边界和所选 ACC 权重见 [build 56 说明](docs/development/pro-14-build-56.md)；计分与 Hold 规则仍按当前玩法处理。
- **晚按补偿**：把「按晚了」的误差整体减掉若干毫秒；设为 0 时不附加晚按补偿；提前和延后窗口仍可独立设置。
- **黄键／红键保护**：可配置邻近 Drag / Flick 与 Tap 的点击匹配规则；本家流程包含提前侧的时间差比较、黄键单次保护和红键可重复保护。
- **连击文字**：自定义连击数下方那行文字（最长 16 个字符）。

### 练习与失败处理

- **自动重试**：失败后自动重开本局，可设置次数（0 为关闭）。
- **续练提前量**：重开时从失败前若干秒处开始，而不是从头开始。
- **变速练习**：练习时每完成一圈自动提速，可设置起始速度与每圈增幅；倍速、流速和提速步长精细到 0.01x。
- **分段练习**：保留原版速度滑条、区间时间轴与白底时间输入框；按时间或起止 Combo 指定区间，保留同刻音符与 Hold 尾部，支持自动循环。新增配置从左侧圆角菜单展开，Combo、自动循环、起始速度、每轮增速、失败重试次数、回退时长及多轮统计均收进菜单。
- **音频辅助与练习判定**：变速不变调、按音频时钟播放正解音、临时判定区间倍率及速度反向联动；不修改保存的判定预设。
- **谱面揭秘与音符行为**：缩放／平移谱面视角并同步触点坐标，缩小时高亮标出原始可见范围；临时转换为 Tap、Drag 或 Flick，随时恢复原谱。判定范围采用 Phira ad 的独立透明色带和描边，可显示在判定线／音符／两处，支持按音符类型、Tap／Hold 判定档位、Hold 尾部及透明度调整。
- **片段书签与多轮统计**：按谱面保存命名片段及练习参数；记录每轮速度、准度、判定数和最佳准度。详见 [练习功能说明](docs/development/pro-14-build-59.md)、[build 60 界面调整](docs/development/pro-14-build-60.md)及 [build 61 侧栏调整](docs/development/pro-14-build-61.md)。

### 血条

- 血条模式（扣到 0 本局失败）、扣血倍率、血条长度、厚度、整体倍率、颜色。

### 外观与界面

- **自定义外观**：图标、背景、立绘可自行导入替换。
- **主页音乐面板**：左下角水平面板提供进度、随机切歌、播放／暂停和歌曲信息弹窗；无自定义音乐时随机播放本地谱面音乐，从预览时间起播，有自定义音乐时循环播放。
- **自定义背景音乐**：导入 mp3 / ogg / wav / flac / m4a / aac 作为主界面背景音乐，可一键恢复默认。
- **界面主题**：自定义软件界面的强调色与表面色。
- **自定义字体**：导入 ttf/otf 作为界面字体，可一键恢复默认（重启后生效）。
- **显示帧率**：默认居中顶部，带背景衬底，可选择顶部／底部居中及四角；iOS 避让安全区域。
- **Windows 帧率上限**：在「设置 → 调试」选择跟随屏幕或 60～480 FPS 的固定档位，立即生效并保存；实际帧率受设备性能与驱动影响。

### 谱面协议与调试

- **rpe-next 噪域**：读取 Next 模式导出的 JSON/PEZ，支持全谱拍数、多段激活、独立动画轨道、锚点与反转选项，使用官方贴图、折射与辉光材质，并接入游玩触点拦截。显示适配与验证范围见 [兼容说明](docs/development/rpe-next-noise.md)。
- **彩色噪域**：支持 Line2Area / BlockAreaList 的噪域读取与颜色显示；Line2Area 在编谱画布内先旋转，再映射到谱面视口，保持相邻噪域的拼接与间距，并修正出现、预警闪烁和失效阶段的显示。

- **判定线调试**：在每条判定线旁显示编号 / 线高 / z-index，本该隐藏的线以淡影保留。
- **音符调试**：在音符旁显示线号 / 时间 / 高度 / 类型，并画出横向判定范围。

### 结算与偏移

- **结算判定分布图**：以 0ms 居中画一张判定时间分布图（早 ← → 晚），左蓝右橙。
- **本局判定标注**：结算状态条简要显示 UNRATED、OD、STRICT 等，点击可查看完整判定体系、实际区间、保护、尾判和其他启用选项。
- **一键应用推荐偏移**：分析本局判定后直接应用推荐的整体偏移。

### 本地成绩

- **成绩历史**：本机保存游玩记录，可查看列表、趋势、PB 对比、判定分布对比，并支持导入 / 导出。细致判定单独保留完整档位，不覆盖原计分体系的最佳成绩。
- **回放文件管理**：从「设置 → 谱面 → 回放文件管理」统一导入、导出、关联和删除回放，单谱本地记录与成绩详情保留入口。新文件使用 `.ppr`，继续读取 `.phirar` / `.phirarec`；回放与谱面、音乐分离，缺少谱面时提供关联或在线下载入口。
- **他人回放**：导入后显示在对应谱面的本地记录中，保留原玩家与来源标记，可按来源和录制规则筛选；不计入本人的 PB、RKS 或统计，也不上传。
- **回放播放器**：连续拖动时间轴、观看倍速、实际录制帧步进、A–B 循环、BAD／MISS 定位、触点编号与显示调节、判定范围和录制信息。新格式保存原判定配置及真实误差，兼容旧格式；旧回放缺失的信息明确标为未记录。

### 插件

- **Phira Pro Plugin API v1**：从「设置 → 通用 → 插件管理」导入 `.zip` Rhai 脚本插件，支持启停、JSON 设置、谱面／结算分析结果与文字覆盖层。提供 PPSR 等算法读取谱面的接口，实际算法由作者实现。见 [接口开发说明与可运行示例](docs/plugins/README.md)。

### 其他

- iOS「设置 → 调试」提供 60／120Hz 选项，并接入 ProMotion 刷新率申请。
- Android「设置 → 调试」可选择 60、120、144、165、185、200 或 240 Hz，并显示当前设备最高刷新率；切换、恢复、显示模式变化及 Surface 重建时会重新申请。实际显示刷新率受设备支持、系统省电和温控策略影响。修正 Android 多指按下、移动、抬起及取消的触摸转发；修正 Windows / Linux 构建问题；内置 HarmonyOS Sans 及补充字体，覆盖语言菜单中的韩语、泰语等字符。

## 成绩与隐私

- 符合登录、谱面及默认判定／玩法条件的对局上传到 **`api.phira.pro`**（Phira Pro 自建成绩服）；非计成绩对局保留本地记录。
- 只有**未改动判定 / 玩法**的对局才会上传（`is_official_play`）：自动游玩、全屏判定、严格判定、判定窗口调整、晚按补偿、Hold 尾判、降速、键盘模式、离线模式等任一偏离默认值都不会上传，仅存本机；本家算法、独立两侧窗口、细致判定同样属于非默认判定。红黄保护、血条参数、流速和判定颜色允许正常计成绩。
- 单谱排行榜分为 **Pro榜 / 混合榜 / 本地记录**。混合榜按当前指标为每位玩家保留较优成绩，标明官服或 Pro服来源，不显示混合名次；个人排名分别显示官服与 Pro服名次。准度仅按已返回成绩排序，不显示准度名次。

## 从源码构建

需要 Rust（版本锁定见 `rust-toolchain.toml`，nightly）和 Python 3.8+，以及对应平台的编译环境：

```bash
python scripts/version.py check
python scripts/build.py
```

Windows、Linux、macOS 在各自系统上使用 `python scripts/build.py --package` 构建 ZIP，版本和平台名称自动派生；包内包含程序、资源与许可证。更新日志保留在 `docs`。Android 使用 `python scripts/build.py --platform android`，iOS 在 macOS 上使用 `python scripts/build.py --platform ios`。GitHub Actions 提供手动运行的 Desktop Packages / Android APK / iOS IPA 工作流；发布与版本定义一致的 Release 后，Build Release Assets 自动构建并上传五个平台附件，上传时校验文件大小与 SHA-256。iOS 产物未签名，安装前需自行签名。发版步骤、环境与签名配置见 [构建说明](docs/development/build.md)。

## 维护人员

Terrysquid、Rulaifozu。应用内人员名单同步更新。

## 授权与致谢

本项目以 **GNU GPL-3.0** 授权（与上游一致），见 [LICENSE](LICENSE)。

- 基于 [TeamFlos/Phira](https://github.com/TeamFlos/phira) 开发，感谢 Phira 及其贡献者；
- 特别感谢 [Phira Mod++](https://github.com/YiyiMo-git/Phira-Mod-New) 与 [Phira ad](https://github.com/Adai-add/PhiraiAd) 的功能探索及公开资料；
- 内置字体 HarmonyOS Sans。
