# pro.17 谱面渲染（视频导出）

从「设置 → 调试 → 谱面渲染」进入。保留纯谱面视频，增加 Phigros 风格开场、游玩入退场和结算。完整参数见 [设置项清单](chart-video-settings.md)，原生扩展接入见 [扩展约定](chart-video-extensions.md)。

## 模块职责

| 模块 | 职责 |
| --- | --- |
| `prpr/src/chart_video.rs` | 公共入口与各平台 FFmpeg 定位 |
| `prpr/src/chart_video/options.rs` | 导出专用配置、默认值和校验 |
| `prpr/src/chart_video/timeline.rs` | 五阶段时间轴与帧量化 |
| `prpr/src/chart_video/presentation.rs` | 独立开场与结算画面 |
| `prpr/src/chart_video/session.rs` | 场景装配、固定时钟、读帧与会话清理 |
| `prpr/src/chart_video/audio.rs` | 音乐、打击音、结算音乐的离线合成 |
| `prpr/src/chart_video/encoder.rs` | RGBA/WAV 输入与 H.264/AAC 编码进程 |
| `prpr/src/chart_video/extensions.rs` | 每会话原生扩展契约与结束通知 |
| `prpr/examples/render_chart_video.rs` | 环境变量驱动的导出与 PNG 序列验证示例 |
| `phira/src/page/render.rs` | 页面数据与子模块入口 |
| `phira/src/page/render/config.rs` | 本地设置读写与旧配置迁移 |
| `phira/src/page/render/settings.rs` | 唯一字段清单、分类、显示值与事务式校验 |
| `phira/src/page/render/layout.rs` | 分类设置和固定操作栏的区域划分 |
| `phira/src/page/render/view.rs` | 对齐字段行、参数总览和进度显示 |
| `phira/src/page/render/controller.rs` | 选谱、编辑、加载、导出与取消控制 |
| `phira/src/page/render/ffmpeg.rs` | Windows 编码器检测、按需下载、校验与应用内安装 |

`GameScene::set_video_frame` 集中接入离线帧时间，`set_video_entrance` 独立控制谱面展开和 HUD 入场，`set_video_progress` 接入视频专用的局内进度，不走交互式场景生命周期。渲染配置由默认值独立装配，强制自动播放、速度 1、音量静音；不传入账号、成绩保存或上传回调。

## 时间轴与音画同步

顺序为开场、游玩入场、谱面播放、游玩退场、结算。每个启用阶段按目标帧率量化成整数帧，累计起始帧构成唯一视频时钟，不借用屏幕刷新率或真实时间。禁用的阶段不占时长；至少保留一个主要阶段。

谱面入场固定在音乐起点，播放阶段以 `start + frame / fps` 推进，退场固定在音乐终点。跳转预览需要时重置并固定步长重算判定。开场和结算由独立演出器绘制，不触发真实游玩回调。默认纯谱面配置不增加任何演出时长；旧版 `render.yml` 缺失的新字段使用该默认值。

局内进度条按音乐区间与可见帧范围计算，不使用扣除谱面偏移后的判定时间。完整播放的最后一帧覆盖到音乐终点，条尾白色标记保持在画面内，退场及结算收尾沿用最终画面；实际音乐采样与判定时间仍按原帧时间推进。内核导出局部片段时保留其在整首音乐中的位置。

音乐与打击音从谱面播放阶段起始帧对齐；结算音乐从结算起始帧加所设延迟对齐，默认延迟 1.085 秒。开场与入退场不额外播放谱面音乐。先在工作线程合成 44100 Hz 双声道音轨，再交给扩展音频回调处理并写入临时 WAV。音量各自独立，音乐和结算支持淡入淡出，峰值过高时统一归一化。

谱面场景先绘入独立目标，再和演出背景、阶段透明度及扩展图层合成。最终 RGBA 通过 `glReadPixels` 送入 FFmpeg；`vflip` 翻转 OpenGL 读帧方向，输出 `libx264`、`yuv420p`、AAC 和 faststart MP4，总时长以视频时间轴为准。

同一界面帧可推进多帧，页面以 45ms 为目标时间片。完成时等待编码器关闭并检查输出；取消、异常或销毁会结束编码器并移除半成品和临时音轨。会话销毁也释放离屏纹理，避免多次预览积累目标。

## 页面行为

- 选择按钮复用谱面库选谱，只接受已下载谱面；导出文件名由谱面标题和难度组成，默认 `data/render/`。
- 五个可编辑分类加参数总览，共 47 项。宽窗口双列，窄窗口单列；设置与底部状态区独立滚动，操作固定，长路径和错误按实际高度布局。
- 设置页不提供画面预览。内核保留指定帧接口供验证和后续复用，不要求页面持有第二个游戏场景。
- 导出中锁定编辑，可查看当前阶段、累计帧数、进度、预计剩余时间，底部可取消。
- 结算默认使用本次自动播放统计，也可设置展示值。只导出结算或指定帧预览时使用满分示意；画面始终标注 AUTOPLAY，不保存或上传成绩。
- Windows 后台检测实际 H.264/AAC 编码能力。缺失时点击“下载并安装 FFmpeg”，从 [gyan.dev Windows builds](https://www.gyan.dev/ffmpeg/builds/) 下载 essentials ZIP，支持进度、取消和重试。使用独立 HTTPS 客户端，不携带游戏认证信息；内部校验下载包，提取编码器和许可说明，在临时目录验证编码成功后再安装。
- Windows 查找顺序为 `PHIRA_FFMPEG`、`data/tools/ffmpeg/ffmpeg.exe`、系统 PATH；按需下载，不修改系统 PATH，主程序包不附带 FFmpeg。

## 演出坐标与动画

开场曲绘使用宽度比例定位：左边界 0.437，垂直中心偏移 -0.167，尺寸 0.50781×0.26779。文字纵坐标用画面高度锚点加 4:3 参考比例补偿，长文本使用单行缩字，避免换行挤占其他元素。

默认开场 5.8 秒，采用 3 秒退场时序：前 2.8 秒保持加载画面，随后曲绘用三次曲线滑出 1 秒，停顿 0.5 秒，再用 0.5 秒完成背景混合、真实谱面判定线展开及顶部和底部 HUD 滑入；入场进度采用二次缓入缓出曲线，帧数减一作为分母，最后保持完整游玩画面 1 秒后开始播放。自定义开场短于 3 秒时等比压缩动画。入场期间冻结谱面音乐时间，不推进判定。判定线从开始即使用谱面和资源包的真实颜色，最终保持画面与播放首帧共用完整场景；开场后另设游玩入场时继续保持该场景，避免重复淡入。未启用开场时，游玩入场参数控制真实谱面与 HUD 入场。

Loading 的白色矩形先固定左边界向右延伸，再固定右边界向右收缩至消失，空白周期边界不闪回整块。提示文字优先使用导出设置，其次沿用谱面提示；均未提供时，每次导出从游玩加载共用的提示条目中抽取一次。

默认结算 6 秒，先保留游玩收尾，再分批呈现曲绘、玩家资料、分数、连击与准度、判定统计和评级图标。不同元素分别使用三次移入、二次透明度过渡和评级缩放；较长结算时长用于保持最终画面，较短时长压缩入场时间。演出字体可单独选择，渲染期间暂时固定字体显示比例，返回设置页时恢复应用比例。

文字排版对空格同样检查字体覆盖；主字体缺少普通空格或其他空白字符时，从已配置的备用字体中选择对应字形，保留间距，避免绘制缺字方框。该行为与应用其他文字共用。

玩家昵称底板按实际字体测量和缩字后的宽度向左伸缩，保留斜边内距，头像、课题牌子和 RKS 位置固定。课题框较窄，下方 RKS 白框较宽，两者完整收在延伸至画面右边缘的黑色底板内，课题素材的透明留白与发光边缘计入整体尺寸。课题背景从 `assets/render/challenge/` 加载绿、蓝、红、金、彩五种纹理；中性牌子或纹理缺失时使用纯色后备。

## 命令行示例

`prpr/examples/render_chart_video.rs` 支持 `CHART`、`OUT`、`WIDTH`、`HEIGHT`、`FPS`、`START`、`END`、`SAMPLES`，以及 `HUD`、`NO_HITSOUND`、`FULL_PRESENTATION` 标志。设置 `FULL_PRESENTATION=1` 启用完整演出，未设置时保持纯谱面。

`OPTIONS` 可读取一份 JSON 格式的完整 `VideoOptions`，便于使用同一组参数进行离线验证。

`PNG_DIR` 保存原始读帧方向的 PNG 序列，`FRAMES` 可限制验证帧数；截断导出不当作完整视频成功。`NEXT_FRAME` 供逐帧调度检查使用。

## 当前范围

本阶段完成独立配置、完整演出流程、分类设置与原生扩展边界。Phigros 风格画面使用项目现有字体与资源，尚不保证与某个 Phigros 版本逐像素一致。

后续阶段继续扩展游玩画面细节、细分音频、硬件编码、预设文件及批处理。当前预留原生接口和命名空间配置，尚未接入 Rhai 渲染插件适配器。

Windows 可按需安装 FFmpeg；macOS、Linux 需系统提供带 libx264/AAC 的 FFmpeg。离线渲染仍消耗 GPU 和 CPU，耗时取决于分辨率、帧率及谱面效果复杂度。当前交互页面固定 16:9 输出。

## Android 端

渲染器同样编进了 Android 包（arm64-v8a），并内置了一份静态 ffmpeg，手机上没有系统 ffmpeg 也能出片。

- `scripts/build-android-ffmpeg.sh`：下载 x264 stable 与 ffmpeg 7.1.1 源码，用 NDK 的 `aarch64-linux-android26-clang` 交叉编译，产出静态可执行文件（带 libx264，约 21 MB）。
- `phira-android/app/build.gradle` 的 `ffmpegNdkBuild` 任务调用上面的脚本，把结果放进 `app/build/ffmpegJniLibs/arm64-v8a/libffmpeg.so`，并通过 `jniLibs.srcDirs` 打进 APK；`preBuild` 依赖它，所以 `./gradlew assembleRelease` 一条命令即可。
- 运行期由 `prpr::chart_video::ffmpeg_candidates()` 给出候选列表并逐个尝试：环境变量 `PHIRA_FFMPEG` → nativeLibraryDir 里的 `libffmpeg.so` → 它的可写副本（拷到缓存目录后 `chmod 755`）→ PATH 里的 `ffmpeg`。
- 为什么要副本：安装时解压出来的 native lib 权限一般是 `0644`（只够 `dlopen`），直接 `execve` 会 `EACCES`。而 Android 10 起对 `targetSdk >= 29` 的应用禁止执行自己数据目录里的文件（W^X），所以本包的 `targetSdk` 定在 **28**，才允许从缓存目录启动这个子进程（Termux 同款做法）。
- 渲染器包的 `applicationId` 与上游一致（`org.flos.phirapro`）、版本名 `0.8.2-pro.17-render`（versionCode = build_number × 10 + 4），安装会覆盖同名的官方包。
- 移动端没有可用的系统文件对话框，Android 页面隐藏更改目录按钮并显示最终保存位置，谱面仍可从选谱界面挑。
- 手机上渲染先写到应用私有目录，结束后由 `MainActivity.publishVideo()` 通过 MediaStore 复制到系统「下载/PhiraPro」（API < 29 则直接写公共目录并通知媒体库扫描），随后删掉临时文件；页面上的「打开输出目录」在 Android 换成了「视频会保存到 下载/PhiraPro」的提示。
