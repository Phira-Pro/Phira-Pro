# Phira Pro

Phira Pro 是基于 [Phira](https://github.com/TeamFlos/phira) v0.8.2 的**非官方改版**。

它在尽量不改变原有玩法与手感的前提下，补充了大量**练习、调试、外观自定义与本地成绩管理**相关的功能，适合练习高难谱面、研究判定手感或自建外观。

> **本项目为非官方改版，与 TeamFlos / Phira 官方没有任何关系。**
> 使用本改版产生的一切后果由使用者自行承担，请勿将本改版的问题反馈给官方。

## 下载

在 [Releases](https://github.com/Phira-Pro/Phira-Pro/releases) 页面下载 Windows 压缩包或 Linux 压缩包或 Android arm64 APK。Windows 解压后运行 `phira-main.exe` (Linux为`phira-main`)；Android 安装 APK，更新时保留应用数据。

本轮功能与验证范围见 [pro.9 更新说明](docs/development/pro-9.md)；版本维护、五个平台的构建与 Actions 使用方法见 [构建说明](docs/development/build.md)。

## 相对官方版新增的功能

### 判定与手感

- **判定窗口自定义**：Perfect+ / Perfect / Good / Bad 的判定窗口可分别调整。
- **晚按补偿**：把「按晚了」的误差整体减掉若干毫秒；设为 0 时与早按完全对称。
- **黄键保护**：点击（蓝键）不会被叠在附近的黄键（Drag）吃掉。
- **红键保护**：点击（蓝键）不会被叠在附近的红键（Flick）吃掉。
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

### 谱面调试

- **判定线调试**：在每条判定线旁显示编号 / 线高 / z-index，本该隐藏的线以淡影保留。
- **音符调试**：在音符旁显示线号 / 时间 / 高度 / 类型，并画出横向判定范围。

### 结算与偏移

- **结算判定分布图**：以 0ms 居中画一张判定时间分布图（早 ← → 晚），左蓝右橙。
- **一键应用推荐偏移**：分析本局判定后直接应用推荐的整体偏移。

### 本地成绩

- **成绩历史**：本机保存每一次游玩记录，可查看列表、趋势、PB 对比、判定分布对比，并支持导入 / 导出。

### 其他

- 修正 Windows 构建问题；修正 Linux 构建问题；内置 HarmonyOS Sans 作为回退字体；补齐新增文案的多语言翻译。

## 成绩与隐私

- 成绩会**固定上传到 `api.phira.pro`**（Phira Pro 自建成绩服），同时在本机保存一份游玩记录；不收集设备信息，也不上传其它文件。
- 只有**未改动判定 / 玩法**的对局才会上传（`is_official_play`）：自动游玩、全屏判定、严格判定、判定窗口调整、晚按补偿、黄键 / 红键保护、Hold 尾判、血条倍率、降速、键盘模式、离线模式等任一开启都不会上传，仅存本机。
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
