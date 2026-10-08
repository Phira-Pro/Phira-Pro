# Phira Pro

Phira Pro 是基于 [Phira](https://github.com/TeamFlos/phira) v0.8.2 的**非官方改版**。

它在尽量不改变原有玩法与手感的前提下，补充了大量**练习、调试、外观自定义与本地成绩管理**相关的功能，适合练习高难谱面、研究判定手感或自建外观。

> **本项目为非官方改版，与 TeamFlos / Phira 官方没有任何关系。**
> 使用本改版产生的一切后果由使用者自行承担，请勿将本改版的问题反馈给官方。

## 下载

在 [Releases](https://github.com/Phira-Pro/Phira-Pro/releases) 页面下载 Windows 压缩包或 Linux 压缩包或 Android arm64 APK。Windows 解压后运行 `phira-main.exe` (Linux为`phira-main`)；Android 安装 APK，更新时保留应用数据。

当前源码版本为 **0.8.2-pro.12**（build 48）。新增功能与验证范围见 [pro.12 更新说明](docs/development/pro-12.md)；版本维护、五个平台的构建与 Actions 使用方法见 [构建说明](docs/development/build.md)。

## 相对官方版新增的功能

### 判定与手感

- **判定设置与预设**：从「设置 → 谱面 → 判定设置」进入独立页面，直接调整当前设置，也可添加、命名、编辑和保存预设。内置「Phigros本家判定」「Phira Pro判定」「细致判定」。
- **提前／延后独立区间**：各档分别设置两侧阈值，例如 Perfect+ −16ms / +20ms；同时提供匹配、保护、Hold 和严判选项。
- **Perfect+ 开关**：关闭时合并到 PERFECT，同时禁用理论值分数。
- **细致判定**：PERFECT+ → PERFECT → GREAT → GOOD → OK → MEH → BAD → MISS；两列结算及历史详情保留各档计数。
- **osu!mania OD 快捷区间**：选择整数 OD −15～15 自动应用判定区间，随后可继续手动调整。扩展档位和窗口映射见 [更新说明](docs/development/pro-12.md)；计分与 Hold 规则仍按当前玩法处理。
- **晚按补偿**：把「按晚了」的误差整体减掉若干毫秒；设为 0 时不附加晚按补偿；提前和延后窗口仍可独立设置。
- **黄键／红键保护**：可配置邻近 Drag / Flick 与 Tap 的点击匹配规则；本家流程包含提前侧的时间差比较、黄键单次保护和红键可重复保护。
- **连击文字**：自定义连击数下方那行文字（最长 16 个字符）。

### 练习与失败处理

- **自动重试**：失败后自动重开本局，可设置次数（0 为关闭）。
- **续练提前量**：重开时从失败前若干秒处开始，而不是从头开始。
- **变速练习**：练习时每完成一圈自动提速，可设置起始速度与每圈增幅。

### 血条

- 血条模式（扣到 0 本局失败）、扣血倍率、血条长度、厚度、整体倍率、颜色。

### 外观与界面

- **自定义外观**：图标、背景、立绘可自行导入替换。
- **自定义背景音乐**：导入 mp3 / ogg / wav / flac / m4a / aac 作为主界面背景音乐，可一键恢复默认。
- **界面主题**：自定义软件界面的强调色与表面色。
- **自定义字体**：导入 ttf/otf 作为界面字体，可一键恢复默认（重启后生效）。
- **显示帧率**：在左下角显示当前帧率。

### 谱面协议与调试

- **彩色噪域**：支持 Line2Area / BlockAreaList 的噪域读取与颜色显示，修正不同比例视口的覆盖范围及白色边缘残红问题。

- **判定线调试**：在每条判定线旁显示编号 / 线高 / z-index，本该隐藏的线以淡影保留。
- **音符调试**：在音符旁显示线号 / 时间 / 高度 / 类型，并画出横向判定范围。

### 结算与偏移

- **结算判定分布图**：以 0ms 居中画一张判定时间分布图（早 ← → 晚），左蓝右橙。
- **一键应用推荐偏移**：分析本局判定后直接应用推荐的整体偏移。

### 本地成绩

- **成绩历史**：本机保存游玩记录，可查看列表、趋势、PB 对比、判定分布对比，并支持导入 / 导出。细致判定单独保留完整档位，不覆盖原计分体系的最佳成绩。
- **本地回放**：新格式保存 Perfect+／扩展档位状态及新增等级，播放不依赖当前判定设置，兼容旧格式。

### 其他

- 修正 Android 多指按下、移动、抬起及取消的触摸转发；修正 Windows / Linux 构建问题；内置 HarmonyOS Sans 作为回退字体。

## 成绩与隐私

- 符合登录、谱面及默认判定／玩法条件的对局上传到 **`api.phira.pro`**（Phira Pro 自建成绩服）；非计成绩对局保留本地记录。
- 只有**未改动判定 / 玩法**的对局才会上传（`is_official_play`）：自动游玩、全屏判定、严格判定、判定窗口调整、晚按补偿、黄键 / 红键保护、Hold 尾判、血条倍率、降速、键盘模式、离线模式等任一偏离默认值都不会上传，仅存本机；本家算法、独立两侧窗口、细致判定同样属于非默认判定。
- 单谱排行榜分为 **Pro榜 / 混合榜 / 本地记录**。混合榜按当前指标为每位玩家保留较优成绩，标明官服或 Pro服来源，不显示混合名次；个人排名分别显示官服与 Pro服名次。准度仅按已返回成绩排序，不显示准度名次。

## 从源码构建

需要 Rust（版本锁定见 `rust-toolchain.toml`，nightly）和 Python 3.8+，以及对应平台的编译环境：

```bash
python scripts/version.py check
python scripts/build.py
```

Windows、Linux、macOS 在各自系统上使用 `python scripts/build.py --package` 构建 ZIP，版本和平台名称自动派生；包内包含程序、资源与许可证。更新日志保留在 `docs`。Android 使用 `python scripts/build.py --platform android`，iOS 在 macOS 上使用 `python scripts/build.py --platform ios`。GitHub Actions 提供手动运行的 Desktop Packages / Android APK / iOS IPA 工作流；发布与版本定义一致的 Release 后，Build Release Assets 自动构建五个平台并上传附件及 SHA-256 清单。iOS 产物未签名，安装前需自行签名。发版步骤、环境与签名配置见 [构建说明](docs/development/build.md)。

## 授权与致谢

本项目以 **GNU GPL-3.0** 授权（与上游一致），见 [LICENSE](LICENSE)。

- 基于 [TeamFlos/Phira](https://github.com/TeamFlos/phira) 开发，感谢 Phira 及其贡献者；
- 内置字体 HarmonyOS Sans。
