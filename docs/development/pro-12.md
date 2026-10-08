# pro.12

源码版本 `0.8.2-pro.12`，Android versionCode / iOS CFBundleVersion 为 `48`，统一维护在根目录 `version.json`。本文件说明当前源码的功能；已经发布的下载文件以 Releases 页面为准。

## 判定设置

入口为 **设置 → 谱面 → 判定设置**，不新增独立设置侧栏。当前设置可直接修改并自动保存；添加预设用于保存、命名和复用组合，支持取消草稿、编辑、重命名及删除。内置 Phigros本家判定、Phira Pro判定、细致判定。

每档可分别设置提前／延后阈值，例如 Perfect+ −16ms / +20ms。本家流程的严判基础区间、Drag 区间和 Flick 倍率也支持分侧配置；非法数值拒绝写入，阈值按等级顺序归一化。旧配置继续使用原有对称区间。

Perfect+ 可关闭：普通模式只剩 PERFECT / GOOD / BAD / MISS，原 Perfect+ 计入 PERFECT，理论值分数随之禁用。细致判定提供 PERFECT+ / PERFECT / GREAT / GOOD / OK / MEH / BAD / MISS，关闭 Perfect+ 时保留七档。结算和历史详情保留原来的粗体、灰白文字及提前蓝色／延后橙色，两列各最多四项，集中在 RETRY 左侧；连击栏保留原位置。细致判定时推荐偏移按钮放在 RETRY 上方，避免与档位重叠。

## osu!mania OD 快捷设置

下拉选择整数 OD −15～15 后，自动开启扩展档位并应用以下对称宽判阈值；不覆盖已保存的命名预设。手动改为不对应任何 OD 的区间时显示自定义。

| 档位 | 半窗口（ms） |
| --- | --- |
| PERFECT+ | 8（应用额外分档） |
| PERFECT | 16 |
| GREAT | 64 − 3 × OD |
| GOOD | 97 − 3 × OD |
| OK | 127 − 3 × OD |
| MEH | 151 − 3 × OD |
| BAD | 188 − 3 × OD（使用 mania MISS 点击窗口外边界） |
| MISS | 沿当前算法的未命中／过期规则 |

依据 [osu!mania 官方判定说明](https://osu.ppy.sh/wiki/en/Gameplay/Judgement/osu%21mania) 的普通原生谱面、非 ScoreV2 名义半窗口；[官方通常的 OD 范围为 0～10](https://osu.ppy.sh/wiki/en/Beatmap/Overall_difficulty)，−15～15 是按用户要求外推的范围。

此项只设置区间，不移植 mania 的计分、整数误差舍入、自动 Miss、不可 late MEH、Hold 释放或转谱规则。算法、Perfect+ 开关、严判和保护等其他选择保持原值：Pro 严判仍将宽判减半，本家流程仍使用其独立严判参数。

## 细致判定与兼容

默认半窗口为 16 / 40 / 70 / 100 / 130 / 160 / 220ms；对应准确率权重为 1 / 1 / 0.85 / 0.65 / 0.50 / 0.25 / 0。窗口和权重是项目选择，不宣称使用 osu! 的计分规则。每侧阈值可编辑，未提供权重／颜色编辑器。

PERFECT+ 至 MEH 保持连击，BAD / MISS 断连；GREAT / GOOD / OK / MEH 会失去 AP。细致判定仅保存本地历史，不上传为原计分成绩，不覆盖既有本地最佳。历史新增可选完整等级计数，旧记录兼容。

本地回放 v4 保存判定档位开关和新增等级，兼容旧版本及 JPhiraRec v0/v1；旧事件码含义保持不变。

## 安卓输入与彩色噪域

多指触摸转发按 Android pointer ID 保留每根手指，分别处理按下、移动、抬起、历史采样及取消。Java 回归覆盖 3～10 指序列；系统三指手势拦截和具体设备硬件仍需实机确认。

保留此前的彩色噪域读取，补充视口、遮罩及边缘色的修正，相关回归脚本为 `scripts/verify_block_viewports.py`。隐藏桌面 GPU 结果不能替代 iPad / iOS 或安卓实体设备的验收。

## 验证与构建

核心 Rust 回归 145 项、应用回归 23 项、独立隐藏 OpenGL 面板及结算回归 1 项，共 169 项通过；图形回归覆盖 4:3 / 16:9 / 21:9、真实下拉选择、全部 31 个 OD 值、普通计数／早晚详情、偏移按钮和六位计数。版本和打包工具回归 17 项通过，安卓 Java 路由回归通过。

Windows 与 Android arm64 release 已构建并校验签名、版本、包内程序及校验和。本轮没有完成 iOS IPA、Linux / macOS 构建或手机平板前台人工验收。构建与 Actions 使用方法见 [构建说明](build.md)。
