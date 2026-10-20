# pro.16 本地开发记录

版本：`0.8.2-pro.16`，内部构建号 `82`。本次修复字体与零速音符显示，按用户要求更新现有 pro.16 标签和 Release，并重新构建本地 Windows 包。

## 新增功能

- 设置 → 谱面新增「显示判定详情」。局内在分数、准确率下方逐行右对齐显示当前预设可用档位的数量；顺序为 PERFECT+、PERFECT、GREAT、GOOD、OK、MEH、BAD、MISS。颜色依次为 `ffc600`、`f6ffac`、`87c8ff`、`a7edff`、`9fffee`、`ffccb6`、`ff4b4b`、`afafaf`，不透明度 50%。沿用分数 HUD 的显隐、缩放和 attachUI 变换。
- 设置 → 谱面新增「显示 Perfect Combo」。仅改变局内连击显示：Perfect+/Perfect 连续累加，其他档位清零；沿用原本至少 3 连击才显示的规则。普通连击、最大连击、分数和准确率计算不变；回放重建判定时同步重建这一显示连击。
- 单曲 Mods 新增「音符行为」，循环选择原始、Tap、Drag、Flick。复用练习已有的音符/音效转换与缓存重建，和练习偏好分别存储。转换后可正常游玩、结算；通过既有 UNRATED 路径阻止成绩保存和上传，结算详情标注 ALL TAP/ALL DRAG/ALL FLICK。

两个显示开关默认关闭，属于外观设置，不改变成绩资格。课题模式仍由其已有规则锁定玩法 Mod。

## RPEN

按 RPEN `0.12.0` 提交 `1ee0fc3e3e434696f00cbb51d4107379913a6490` 实现十项扩展，具体范围和边界见[适配记录](pro-16-rpen-compatibility-audit.md)。

导入时检测实际使用的扩展字段，提示早期测试警告，并默认开启 `useRpen`；谱面信息新增「RPEN特性」开关。显式关闭后回退至旧解析和渲染路径。旧版新速度缓动与 attachUI 的用户选择保持独立。

主页游玩按钮保持 HUD 指定的尺寸，曲绘按 CropCenter 裁切；过渡时裁切完整曲绘，不将曲绘重新适配到动画中的窄条。

## 验证记录

- `cargo test -p prpr --lib --release`：222 通过，2 忽略。新增测试覆盖档位筛选、Perfect Combo 全部中断档位、复位/克隆、Mod 互斥循环、成绩资格和配置序列化。
- 真实 GPU 场景回归 `production_replay_transport_import_and_aspects` 通过，覆盖手机、平板、宽屏的实时判定行渲染开关、Perfect Combo，以及普通游玩三种音符转换。测试计数为人工注入的 HUD 压力样本，不是实打成绩。
- Windows 桌面实际点按两个设置、循环切换四种音符行为，运行 Tap 转换谱至结算；观察到右上角判定行更新、UNRATED 和详情中的 ALL TAP。使用隔离的 QA 数据目录，没有改动用户谱面库。
- iOS `aarch64-apple-ios` 核心库编译检查通过。
- `scripts/build.py --platform windows --package` 从当前源码完成本地构建。ZIP 完整性检查通过，包内可执行文件与桌面操作验证的文件 SHA256 一致。

产物：`E:/Phira Pro/dist/windows/PhiraPro-v0.8.2-pro.16-win64.zip`。

构建 80 的验证及产物摘要见下方追加记录；上方最初的 222 项与桌面交互记录来自构建 79。

## 构建 80 收尾验证

- 核心回归：231 通过，2 忽略。
- 真实 GPU 回归：生产 ZIP 导入器的警告、默认开启和关闭回退通过；三种比例下轨迹、分层噪域、摄像机画面通过。30 个新增 shader 实际编译通过。NoiseField/Ocean 实际彩色区域像素检查通过。
- 自定义最小皮肤验证历史反馈首帧/暂停/下一帧/倒退的像素值、参数、R8 采样和低性能通道；谱内 vec3 参数轨道验证本线 BPM 倍率只应用一次。
- 相机 GPU 近/远平面、背向、二维画布外的音符经平移可见性通过。轨迹、镜头和时间样本与固定上游求值器输出比较。
- 桌面使用构建 80 实际进入谱面信息，关闭/保存/重新启用 RPEN 开关，文件 `useRpen` 随之变为 false/true。进入正常游玩可见投影后的原生噪域、轨迹线和音符，暂停及退出可操作。导入器验证来自真实 GPU 回归中的生产导入调用，不把它写成已点按 Windows 文件选择器。
- 隔离数据目录移除之前的自定义游玩 HUD 尺寸，实际观察 512×512、1600×600、600×1200 的测试曲绘自动切换：按钮外框保持相同位置/宽高，曲绘内部居中裁切，原图宽高比不会反过来改变 HUD。
- iOS `aarch64-apple-ios`、Android `aarch64-linux-android` 核心库编译检查通过。
- 按 `scripts/build.py --platform windows --package` 完成构建，ZIP 完整性和 30 个 shader、皮肤及许可证资源检查通过；包内 EXE、`target/release` EXE、桌面验证 EXE 的 SHA256 一致。

ZIP SHA256：`e8cc84ae15529d6d28df1532e0a6f02226159681982387f9946ba4f0cefb6bf9`。

EXE SHA256：`9136f05e8a0e4d031df2855a7d5b5adc1d137e4fe179ee108eaa58e543e84348`。

## 判定详情显示调整

完整显示 PERFECT+、PERFECT、GREAT、GOOD、OK、MEH、BAD、MISS，不透明度由 30% 调整为 50%。复用现有真实 GPU 回归并查看手机、平板、宽屏渲染图，确认完整名称和右对齐显示。Windows 从修改后源码重新构建打包成功。

本次 ZIP SHA256：`7e8760b6beaaabe0bee87bf7717be73b1c92f05cdd1bd2635e6bb7d6309bbc16`。

## 背景音乐播放修复（构建 81）

静音时显示播放图标和恢复声音提示；用户主动点击播放后恢复有效音量并保存。音量配置变化按实际值同步到音频流，恢复播放使用淡入以取消遗留淡出。详见 [检查与验证记录](pro-16-background-music.md)。

本次 ZIP SHA256：`3ca91f9876272a84d6a7a95419623e1cd4eb69140dc96f25ec319bbe325c1ce4`。

## 已知渲染问题修复（构建 82）

- 内置 `phigros.ttf` 的普通空格 U+0020 包含可见轮廓；Perfect Combo 标签改用同字体中无轮廓、有字宽的 U+00A0。回归直接读取实际内置字体，避免用界面字体代替游玩字体造成漏测。
- 零速音符原先在距离控制取样中计算 `0 / 0`，使控制曲线产生 NaN。零速时改为固定位置取样；正常速度保留原计算。上下侧分组裁剪也显式处理零速，包括负零。
- 核心测试 233 项通过，3 项桌面图形测试按默认规则忽略；独立运行新增真实 OpenGL 回归通过：两套判定 × 三种屏幕比例 × 正负零速度 × 上下侧 × Tap/Hold/Flick/Drag，检查音符实际像素与手动触摸事件产生的判定、连击和分数。Flick 使用按下后两段连续移动，未启用 Auto。
- Windows 新构建在隔离数据目录实际进入测试谱面，观察到贴线音符，鼠标点击后 PERFECT 计数与分数增加。没有修改用户谱面库；该桌面样本是自制零速测试谱，不冒称为用户的 Lapis 原谱。
- 版本定义一致性、发布脚本 21 项测试和 ZIP 完整性检查通过；从当前源码执行 `scripts/build.py --platform windows --package` 完成本地打包，包内 EXE 与实际操作验证的 EXE 摘要一致。

ZIP SHA256：`b0db4cb0059d6feb60c0a0db747826e1355a44f8134516d4fb5c2f708c597cd2`。

EXE SHA256：`121d1d824658c6c7ea570b8e6dcf99fd2434dbc04ac1ac28bef931970cadbc00`。
