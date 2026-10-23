# pro.17 联机入口与界面

## 源码依据

本轮实际阅读了以下源码后重新实现 Rust 界面：

- 官方 Phira [MainScene](https://github.com/TeamFlos/phira/blob/main/phira/src/scene/main.rs) 的联机开关、圆形入口、拖动与保存位置，以及 [设置](https://github.com/TeamFlos/phira/blob/main/phira/src/page/settings.rs) 的联机开关。
- osu!lazer 固定提交 `920019169c2cb7ea3c6956499e529450064619ea`：[MultiplayerMatchSubScreen](https://github.com/ppy/osu/blob/920019169c2cb7ea3c6956499e529450064619ea/osu.Game/Screens/OnlinePlay/Multiplayer/MultiplayerMatchSubScreen.cs)、[ParticipantPanel](https://github.com/ppy/osu/blob/920019169c2cb7ea3c6956499e529450064619ea/osu.Game/Screens/OnlinePlay/Multiplayer/Participants/ParticipantPanel.cs)、[MultiplayerMatchFooter](https://github.com/ppy/osu/blob/920019169c2cb7ea3c6956499e529450064619ea/osu.Game/Screens/OnlinePlay/Multiplayer/Match/MultiplayerMatchFooter.cs)、[MatchStartControl](https://github.com/ppy/osu/blob/920019169c2cb7ea3c6956499e529450064619ea/osu.Game/Screens/OnlinePlay/Multiplayer/Match/MatchStartControl.cs)。

采用其暗色分区、圆角成员行、独立谱面和聊天栏、底部准备操作的视觉结构。现有 phira-mp v1 会话和房间规则继续由原模块负责。圆形入口图标来自官方 Phira `assets/multiplayer.png`。

## 用户入口与生命周期

- 在“设置 → 通用 → 多人联机”开启，默认关闭。设置搜索可以定位到该项；原先谱面库顶部的临时入口已移除。
- 圆形按钮出现在菜单各页，可拖动。点击和拖动分别处理，拖动结束保存位置，取消触摸恢复拖动前位置；其他手指不能结束当前拖动。
- 位置存储为横向坐标和相对屏幕高度的纵向坐标，改变窗口比例后将整个圆形夹在屏幕安全范围内；恢复默认设置会重置入口开关和位置。
- 点击打开联机面板，右上角关闭按钮收起面板。收起只隐藏界面，原 Session、房间和消息仍保留；重新打开不重复鉴权。
- 关闭设置中的联机开关才断开连接并释放面板任务。动画、界面和游戏启动通过显式接口连接，MainScene 持有唯一实例。
- 游玩仍使用现有 NoRetry 启动与 Pro 本地成绩/回放保存，正常返回后发送匹配本局的 Abort。

## 模块职责

| 模块 | 职责 |
| --- | --- |
| `mp/overlay.rs` | 圆形入口手势、位置、显示动画、面板生命周期和 MainScene 接口 |
| `mp/view.rs` | 连接卡片、成员/谱面/聊天三栏、滚动和按钮绘制、不同宽高比布局 |
| `mp/panel.rs` | 按钮动作、资源准备、连接任务、游戏加载和返回 |
| `mp/session.rs` / `mp/recovery.rs` | 协议执行与恢复，与界面显隐无关 |

背景由 MainScene 显式传入当前模糊背景图，用户更换背景后随之变化。背景先覆盖后方菜单，再绘制暗色表面，避免旧页面文字透过面板。

## 验证

布局和手势自动测试覆盖 4:3、16:9、20:9、21:9、32:9、拖动/点击区分、取消触摸和其他手指隔离。它们验证布局计算与手势状态，不等于这些比例的真实设备验收。

构建 86 的应用回归为 90 通过、3 个图形用例忽略。Windows 真实窗口实操覆盖设置开关、拖动不误打开、入口位置保存与重启恢复、点击进入、关闭后重开保留房间、锁房、准备开始、结算返回、离房和主动断开。实操窗口截图尺寸为 1462×960；其他比例本轮使用布局自动测试。

最终程序连接本地回环夹具完成一局。鉴权仅 1 次（隐藏重开没有重新鉴权），RequestStart=1、Abort=1、Played=0，收到 112 个 Touches 包、6 个 Judges 包；本地成绩从 7 增至 8，独立 ppr 文件也为 8 个。报告 `target/mp-desktop-qa/pro17-ui-qa-report.json`。这是桌面客户端与本地夹具验证，正式服务端与多设备验收另见阶段记录。
