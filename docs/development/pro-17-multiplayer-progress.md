# pro.17 多人联机阶段记录

开发依据：用户提供的 `E:/Phira Pro/phira-mp-custom-dev-guide/` 全套文档；版本 `0.8.2-pro.17`，内部构建号 86。未授权 push、标签及 Release。

## 备份和清理

重写前备份：`E:/Phira-Pro-core-before-multiplayer-20261023-093746.zip`，261650605 字节。SHA256：`29f8ee455b08e71107e6f15bf80f22bc728900cc6b1f701381b5ec7551541285`。

包含核心源码、资源、构建配置、当前未提交文档、完整 Git bundle、开发指引及交接文档；ZIP 自检和清单逐文件 SHA256 校验通过。解压后可直接取得当时工作树；Git bundle 用于恢复提交历史。用户数据、dist 和可再生成缓存不属于源码备份内容，均未因此删除。

核对绝对路径和重解析点后，仅删除 `target/debug`、`target/pro16-desktop-qa`、`target/pro16-packs-qa`、`target/songpack-qa`、`target/stationary-qa`。保留 dist、data、历史源码、当前参考源码及 release 编译缓存。

## 本阶段已写入的代码

| 模块 | 责任 |
| --- | --- |
| `phira/src/mp/protocol.rs` | v1 编解码、TCP 分帧、长度/集合限制、保留帧尾，基础消息与实时流格式校验 |
| `phira/src/mp/session.rs` | 单会话执行器、入房快照安装、RPC 先注册后发送、并发限制、超时隔离、心跳及有界实时队列 |
| `phira/src/mp/recovery.rs` | 有界重连、可取消鉴权、原会话句柄上的原子快照恢复，不重放旧操作 |
| `phira/src/mp/room.rs` | 顺序更新房间、谱面代次、资源准备证据、开局消费与本局结束状态 |
| `phira/src/mp/resources.rs` | HTTP 谱面元数据与本地资源版本/文件校验，独立于 RPC |
| `phira/src/mp/game_bridge.rs` | 本局兼容配置、实时流与现有回放组合、正常完成标记，不使用全局成绩 ID |
| `phira/src/mp/panel.rs` | 自定义地址、连接、建房/入房/离房、谱面 ID、准备/取消、锁房/轮换、聊天和本地游戏衔接 |
| `phira/src/mp/overlay.rs` | 设置开关控制的圆形入口、拖动位置与面板生命周期 |
| `phira/src/mp/view.rs` | 参照 osu!lazer 源码的暗色三栏界面与布局 |

必要接入集中在 Data 配置、模块声明、设置开关、MainScene 浮动入口、单曲启动上下文和通用 NoRetry 结算支持。普通单曲保持原入口；原回放记录仍是判定事件的唯一清空者。实时发送在其读取之前观察事件，避免回放或联机丢失判定。

基础协议运行时：PERFECT+ 合并为 Perfect；本局使用原 Phira Pro 基准判定、1.00x 和关闭玩法 Mod 的配置副本，不修改用户保存设置。禁止暂停菜单重试、结算重试及自动重试。实时触点映射到 i8 范围，结束用按位取反编码；Pro 的本地回放保留原始档位与触点。

官方兼容结果路径：本地照常记录成绩/回放，符合现有规则时使用 Pro 上传。返回房间时发送匹配请求的 Abort，不发送 Played 或官方成绩上传。正常完成标记与网络放弃方式分别记录，旧回合未决请求不能在新连接盲重试。

## 构建 84 的检查记录

- 最终应用常规回归：76 通过、3 个图形专用用例忽略（`target/pro17-app-final.log`）；核心回归 235 通过、3 忽略（`target/pro17-core-final.log`）。
- 联机专用 12 项：包含协议向量、TCP 拆包/粘包、畸形长度、帧尾、旧下载失效、同帧阶段、入房响应后立即推送、并发 RPC 拒绝、重复响应安全断连、7 秒超时及连续两局 Abort。使用回环 TCP，不是仅手写状态模拟。
- Windows 真实 UI：启动 pro.17 隔离程序，通过谱面库联机入口连接本机假服务端，恢复测试房间，准备并正常结算两局，再返回房间。隔离服务端日志确认 2 次 RequestStart、2 次 Abort、没有 Played，并收到 Touches/Judges；history.json 与独立 ppr 文件已落盘。
- 最终包额外复验第 3 局：结算仅保留继续按钮，无重试按钮；正常回房后离房，房间按钮立即切回建房/入房。累计 3 次 RequestStart、3 次 Abort、0 次 Played，3 条本地成绩记录和对应回放文件；截图保存在 `target/pro17-multiplayer-{room,ending,return}.jpg`。
- 建房按钮实际能弹出系统输入框。此输入框由 PowerShell 进程承载，computer-use 工具拒绝跨进程输入，未完成输入提交；没有绕过工具约束。其 RPC 及创建快照安装已由真实 TCP 测试覆盖。
- 可重复的隔离脚本：`scripts/qa/multiplayer_fixture.py --setup --serve`，仅监听 127.0.0.1，使用固定假 token，不读取正式账号数据。图形运行需先构建程序；脚本的假服务端不等于正式 phira-mp 互操作验收。

## 2026-10-24：合并远端与连接恢复

- 先保存本地联机工作树，再快进合并远端 `7701a8e5`（PPSR 算法独立与接口适配），恢复本地工作树，无冲突。快照 `28e0d2b6622c566de9acc7ecb7c69889372e9ad9` 仍保留，未覆盖重写前的核心 ZIP 备份。
- 恢复器最多按 1/2/4 秒退避尝试 3 次，每次连接和鉴权合计限时 8 秒。退避和鉴权均可取消。失败后保留手动连接入口；主动断开不会触发恢复。
- 所有面板/游戏持有同一个 Session。换连接先作废资源和游戏代次，鉴权快照完整安装后才公开已连接；鉴权前到达的推送有界缓存并按顺序应用。恢复时拒绝旧 RPC，丢弃旧实时帧；队列里的请求与实时数据也带代次检查。
- 加载完成前再次检查启动凭据。加载过程中断线、换谱或回合失效，不再启动旧局。正常保存回调成功之后才标记本地完成；已运行的本地游戏可以继续结算，但失效上下文不会把 Abort 发到新回合。
- 从游玩/等待返回选曲阶段时，即使没有 GameEnd，也会清除旧局和旧资源准备状态。每个实时帧绑定房间代次、谱面代次和本地回合号，出队时重新检查，阻止同连接上迟到的上一局数据进入下一局；这仍不是服务端 round_id 的替代品。
- v1 的 Waiting/Playing 鉴权快照没有 chart ID/round ID，不能证明仍是同一局。此时不猜测谱面、不恢复旧 prepared、不自动重开；已在 Playing 的房间等待下一局。增强协议的完整恢复仍属于后续工作。
- 远端新增插件 ZIP 导入测试原本 include 未提交的 ZIP，干净检出无法编译测试。本轮仅调整测试夹具：从已跟踪的示例文件在临时目录打包，未修改 PPSR/PDE-R 的运行逻辑。

执行结果：

- `RUSTFLAGS='--cfg record' cargo test --locked --release -p phira --lib`：**87 通过、3 个图形用例忽略**，包括 17 项联机测试及合入的插件模型测试。日志 `target/pro17-app-merged.log`。
- `RUSTFLAGS='--cfg record' cargo test --locked --release -p prpr --lib`：**235 通过、3 忽略**。日志 `target/pro17-core-merged.log`。
- 新增 4 项真实回环 TCP 测试：同句柄恢复且旧回合/请求失效、退避时取消、鉴权快照前推送的顺序、未响应鉴权期间取消并关闭新 socket。旧请求超时和连续两局 Abort 用例仍通过。
- 另有房间状态用例覆盖缺失 GameEnd 后相同谱面继续下一局，断言旧实时凭据和旧资源任务失效、新凭据可用。
- 最终桌面验证使用夹具 `--serve --drop-once` 和与最终 ZIP 完全相同的 EXE：连接被主动断开一次，界面自动恢复房间，再实际连续完成两局准备→结算→继续→回房，最后离房。夹具计数 Authenticate=2、RequestStart=2、Abort=2、Played=0，收到 202 个 Touches 包和 12 个 Judges 包；history 与 ppr 各从 4 增至 6。主动断开的界面在前一次构建也已复验。本次断言摘要 `target/mp-desktop-qa/pro17-merged-qa-report.json`。这验证桌面客户端与本地夹具，未冒充真实部署双客户端验收。

## 要求落实进度与后续工作

整套指引仍然是开发目标，不能用本阶段替代全部要求：

| 指引范围 | 当前进度 | 尚待实现或验收 |
| --- | --- | --- |
| 02/03/06 客户端时序、协议与故障机制 | 基础协议、执行器、资源代次、RPC 超时隔离、可取消自动恢复已实现 | 真实部署双客户端、更多断线时序及低帧率/后台设备用例 |
| 01/04/05 自定义业务、房间事务、个人投影 | 客户端模块边界和代次已建立 | 自有服务端完整权威房间模型、参与者固定、个人投影及可取消出站计划 |
| 07 完整快照、request_id、round_id、资源确认等 | 基础 decoder 保留帧尾，不擅自发未知扩展 | 双方扩展格式与协商、增强确认路径、自有成绩提供方 |
| 08 分层验收 | 协议/会话测试及桌面基础流程已执行 | 按 T01–T10 对增强路径、真实设备及正式服务端逐项补齐证据 |
| 09 live 故障隔离 | 基础发送、代次与长度检查已接入 | 独立观战消费、索引和谱面归属隔离、跨回合增强实时流 |
| 10 自动轮换 | 尚未实现 | 管理入口、权限、幂等操作、异步查询提交、下一曲计划及端到端用例 |

房间列表、独立观战 UI、私谱分发、自有成绩服务端及管理 API 本阶段尚未完成。自动恢复已经实现基础连接和鉴权层，不能描述为旧协议中途无缝续局。当前不应描述为“多人联机全部重写完成”。

## 构建 85 产物记录

已从合并后的当前源码执行 `scripts/build.py --platform windows --package`，产物 `E:/Phira Pro/dist/windows/PhiraPro-v0.8.2-pro.17-win64.zip`，SHA256 `eb5e60d21e76186f9d9bd362ab5434fe4b978fd3dc766705aeaa484e9227cf76`。日志 `target/pro17-windows-merged-package.log`。

ZIP 完整性校验通过，包内 EXE 与最终真实窗口测试的 EXE 字节一致（EXE SHA256 `82461b6bab6b01068415ec879aa2c9aeec69d385240b4a9d9f275a03912f2643`）。隔离测试数据不在交付包内。

## 构建 86：入口与界面重做

详细的源码依据、界面与生命周期说明见 [联机入口与界面](pro-17-multiplayer-ui.md)。本轮将入口改到设置开关控制的可拖动圆形按钮，并分离面板生命周期、视图和业务控制。关闭面板保留 Session；关闭联机开关释放会话。

已实际操作设置开关开/关、拖动按钮、点击进入和收起，确认开关即时生效、拖动不误打开、位置保存到 Data。实机检查发现后方设置文字透出，已改为显式传入当前模糊背景覆盖后方菜单。

- 最终应用回归：90 通过、3 个图形用例忽略，其中 20 项联机测试通过。日志 `target/pro17-multiplayer-ui-regression.log`。
- 最终 Windows 程序实操：重启恢复圆形按钮位置，连接房间，收起再打开保留同一连接，锁房，准备并完成一局，正常结算后返回原房间，再离房、主动断开。窗口截图尺寸为 1462×960；其他比例本轮验证布局计算，未描述为设备实测。
- 最终夹具计数 Authenticate=1、RequestStart=1、Abort=1、Played=0、Leave=1，收到 112 个 Touches 包与 6 个 Judges 包。history 从 7 增至 8，ppr 文件为 8 个。断言报告 `target/mp-desktop-qa/pro17-ui-qa-report.json`。隔离客户端与夹具已关闭。
- 从当前源码执行 `scripts/build.py --platform windows --package`，日志 `target/pro17-multiplayer-ui-build.log`。交付 ZIP：`E:/Phira Pro/dist/windows/PhiraPro-v0.8.2-pro.17-win64.zip`，SHA256 `351b388759cf95d91a0251406f9ececda60c9ee9218b278c58e84df8c7b8a35a`。
- ZIP 完整性校验通过，已包含圆形入口图标；包内 EXE、release EXE 和最终窗口实操 EXE 字节一致，EXE SHA256 `47a690d57371cdf42c0c6a33e2d2ad87eb27439b223899365e4473f59a79844f`。未推送、未创建标签或 Release。
