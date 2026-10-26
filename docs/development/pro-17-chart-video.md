# pro.17 铺面渲染（视频导出）

## 源码依据

实现基于仓库内既有的离屏渲染能力，没有引入新的图形栈：

- `prpr/examples/block_area_frames.rs` 已经演示了「`Conf { headless: true }` + `render_target` 离屏纹理 + `TimeManager::manual` 固定时钟 + `GameMode::View` + `AUTOPLAY` + `glReadPixels` 读帧」，本功能沿用同一套做法，只是把读出的帧直接送进编码器。
- `prpr/src/scene/game.rs` 的 `GameScene::render` 自己按 `ui.viewport` 更新相机尺寸，因此可以给离屏渲染单独传一个 `(0, 0, W, H)` 的视口。
- 判定线动画由 `GameScene::tick_replay()` 刷新（重置 `res.time` 后更新谱面与特效），自动判定走 `Judge::update` 里已有的 `autoplay` 分支，因此逐帧渲染是确定性的，与真实游玩互不影响。

## 模块职责

| 模块 | 职责 |
| --- | --- |
| `prpr/src/chart_video.rs` | 渲染内核：`GameScene` 装配、逐帧渲染、打击音离线混音、ffmpeg 管道编码、临时文件清理 |
| `prpr/examples/render_chart_video.rs` | 命令行示例，支持 `CHART` / `OUT` / `PNG_DIR` / `FRAMES` 等环境变量，便于批量渲染与回归 |
| `phira/src/page/render.rs` | 「设置 → 调试 → 铺面渲染」页面：选谱、输出设置、进度与预览 |
| `prpr/src/scene/game.rs` | 新增 `GameScene::hide_ui`，渲染时跳过分数/连击/暂停与操作条，只留谱面本体 |

## 渲染流程

1. 打开谱面：`fs_from_file` 同时接受谱面目录和 `.pez` / `.zip`（zip 直接当文件系统读），`load_info` 读取 `info.yml`，`LoadingScene::load` 取曲绘和背景；曲绘缺失时退化为纯色背景。页面传给内核的是谱面库里的 `data/charts/<local_path>`，`.pez` 支持留在内核侧供命令行示例与后续复用。
2. 装配场景：`Config::default()` 后强制 `Mods::AUTOPLAY`、音乐与音效音量归零、`offset = 0`、`speed = 1`、关闭场外交互，再用 `GameScene::new(GameMode::View, …)`。View 模式不会自己推进判定，正好由渲染器完全控制时间。
3. 建离屏目标：`render_target(w, h)`，线性过滤，挂到 `res.camera.render_target` 上；`sample_count` 取当前画质设置。
4. 逐帧推进：第 `i` 帧取 `music_time = start + i / fps`，令 `res.time = music_time - offset`，`alpha = 1`，`tm.seek_to` 后调用 `judge.update`、`tick_replay`、`set_replay_frame_delta(1 / fps)`，再用视口为 `(0, 0, W, H)` 的 `Ui` 渲染。
5. 读帧与编码：`flush` 后把离屏纹理绑到 `GL_READ_FRAMEBUFFER`，`glReadPixels` 读 RGBA（自下而上），原样写入 ffmpeg 的 stdin，由 `-vf vflip` 翻转，`libx264` + `yuv420p` 编码；谱面音乐总是写到临时文件，作为第二个输入按 `-ss start` 对齐并转 AAC；`VideoOptions::hitsound` 打开时再把谱面所有音符的打击音按 `note.time + offset` 离线混成一条 wav（`write_hitsound`），作为第三个输入与音乐 `amix`。

同一 macroquad 帧内可以连续渲染多帧：实测逐帧 `next_frame` 与「一帧内连渲 4 帧」的输出逐像素完全一致，所以页面按每帧 45ms 的时间片推进，界面保持响应。

## 页面行为

- 「选择谱面」直接打开 `LibraryPage::new_render_picker`，和多人游戏共用同一套列表与交互，选中后把 `local_path` 交回本页；`data/render.yml` 记住输出目录、分辨率、帧率、打击音与 HUD 选择。
- 输出文件名取「标题 + 难度」，非法字符替换成 `_`，默认落在 `data/render/`。
- 谱面来源就是谱面库本身，未下载的线上谱面会提示「请先下载这首谱面」，不做第二套目录扫描。
- 渲染中显示进度条、帧数、已用时间、预计剩余、当前帧预览；「取消渲染」会结束 ffmpeg 并删掉半成品，离开页面同样会走清理（`ChartVideoRenderer` 的 `Drop`）。

## 已知限制

- 依赖系统 `ffmpeg`（需带 libx264 与 AAC），没有内置编码器；缺失时开始渲染会直接报错。
- 渲染期间界面仍在实时绘制离屏帧，软件渲染（如 llvmpipe）下 1080p/60 只有 3～4 fps，有独立显卡时快得多；整曲渲染耗时与分辨率、帧率成正比。
- 视频按「音乐时间」线性推进，不包含开局前的准备动画，也不包含结算界面。

## Android 端

渲染器同样编进了 Android 包（arm64-v8a），并内置了一份静态 ffmpeg，手机上没有系统 ffmpeg 也能出片。

- `scripts/build-android-ffmpeg.sh`：下载 x264 stable 与 ffmpeg 7.1.1 源码，用 NDK 的 `aarch64-linux-android26-clang` 交叉编译，产出静态可执行文件（带 libx264，约 21 MB）。
- `phira-android/app/build.gradle` 的 `ffmpegNdkBuild` 任务调用上面的脚本，把结果放进 `app/build/ffmpegJniLibs/arm64-v8a/libffmpeg.so`，并通过 `jniLibs.srcDirs` 打进 APK；`preBuild` 依赖它，所以 `./gradlew assembleRelease` 一条命令即可。
- 运行期由 `prpr::chart_video::ffmpeg_candidates()` 给出候选列表并逐个尝试：环境变量 `PHIRA_FFMPEG` → nativeLibraryDir 里的 `libffmpeg.so` → 它的可写副本（拷到缓存目录后 `chmod 755`）→ PATH 里的 `ffmpeg`。
- 为什么要副本：安装时解压出来的 native lib 权限一般是 `0644`（只够 `dlopen`），直接 `execve` 会 `EACCES`。而 Android 10 起对 `targetSdk >= 29` 的应用禁止执行自己数据目录里的文件（W^X），所以本包的 `targetSdk` 定在 **28**，才允许从缓存目录启动这个子进程（Termux 同款做法）。
- 渲染器包的 `applicationId` 与上游一致（`org.flos.phirapro`）、版本名 `0.8.2-pro.17-render`（versionCode = build_number × 10 + 4），安装会覆盖同名的官方包。
- 移动端没有可用的系统文件对话框，`phira/src/page/render.rs` 里改输出目录在 Android 上是空实现并给出提示，谱面仍可从选谱界面挑。
- 手机上渲染先写到应用私有目录，结束后由 `MainActivity.publishVideo()` 通过 MediaStore 复制到系统「下载/PhiraPro」（API < 29 则直接写公共目录并通知媒体库扫描），随后删掉临时文件；页面上的「打开输出目录」在 Android 换成了「视频会保存到 下载/PhiraPro」的提示。
