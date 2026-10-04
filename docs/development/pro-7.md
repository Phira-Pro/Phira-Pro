# Phira Pro 0.8.2-pro.7

## 使用

解压 Windows 包，在解压目录运行 phira-main.exe。更新旧安装时保留自己的 data 目录。

设置新增「体验」分页：

- 字体显示大小：最小 / 较小 / 适中 / 较大 / 最大，对应 80% / 90% / 100% / 110% / 120%。导入字体按常用字的可见高度匹配内置字体；字重不等同于字号。字体导入后需重启。无效字体会在导入时被拒绝，已有损坏字体在启动时回退到内置字体。
- 理论值分数：只修改结算数字，每个 Perfect+ 增加 1 分；本地记录、谱面详情、排行榜和上传数据仍用普通分数。
- 响度统一：音乐载入时测量一次、按非静音片段的平均电平匹配，并保留峰值余量。主界面、预览和游玩使用同一个 0–1 音量；拖动步长 0.01，±按钮步长 0.5。这不是 LUFS 测量或动态压缩，极端动态音乐受峰值保护约束。
- 固定背景明暗：开启后用输入值覆盖谱面预设，默认 0.60。值表示黑色遮罩强度：0.00 最亮，1.00 全黑。
- 上隐 / 下隐：实体遮挡，默认黑色；保留暂停、分数、连击、进度等 HUD。原强度设置的 0–1 对应屏幕高度的 10%–90%。「体验」中可选颜色或图片；图片保持比例，可拖动、缩放裁剪。实线框为最大遮挡范围，虚线框为最小范围，框外暗化。

谱面 Mod 新增「正解音」：音效按谱面音符时刻触发，而非实际打击时刻；不上传成绩。声音调度随帧更新，仍受设备音频缓冲和帧周期影响，不保证零延迟。拖拽和滑动沿用自动完成判定语义，不因定时音效改变判定。

## 修复和优化

- 取消前台菜单静置时约 60fps 的人为限速；后台及最小化保留省电行为。
- 裁剪区外的 UI 文字和形状提前跳过；透视主页卡片保持原绘制路径。
- 噪域的重叠填充改为逐行区间累加，减少区域面积重复写入；保留官方量化、反相、区域数量、几何和时序。
- 无触摸材质移除完整触摸 SDF 分支；触摸遮罩只扫描手指影响范围。整屏四边形保持原 UV 插值，避免火花采样相位漂移。没有引入相机或额外 FBO 切换。
- 噪域感染按手指生命周期保持：区域消失后，未抬起手指仍显示 hover、保持低通和禁用该手指判定；Ended/Cancelled 才清除。暂停期间也消费松手事件，避免恢复后残留幽灵触点。
- 自定义判定窗口保持毫秒口径，严格判定 Mod 仍减半，倍速按真实时间换算。非法输入被限制到设置允许范围；晚按补偿统一键盘、触屏和自动 Miss 截止时间。误差统计保留真实偏移。修复全屏判定下无手指也能保持 Hold，以及静止滑动样本产生 NaN 的问题。

## 验证范围

核心回归 55 项、所有语言文件校验、Windows record 静态检查和 Android arm64 检查通过。桌面 GPU 用官方导出的 ActiveBlock GLSL 校验 Active、Ready、反相、彩色背景、触摸及区域消失后的保持；测试材质像素通道差为 0，MSAA 1/4、尺寸变化通过。这个测试共享遮罩输入，不能替代整套官方多 Pass 或移动端实机对照。

同机 release CPU 基准，Desultory Signals 的 65–66、67–68、71.8–72.8 秒段：2560×1600 动态遮罩从约 4.9–5.0ms 降到约 4.0–4.1ms。完整噪域 CPU+GPU 约 4.5–4.6ms。测试是桌面 NVIDIA，未包含整局音符、故事板、HUD、MSAA，不代表小米平板或 iPad 的帧率。需要继续在这两台设备验证连续游玩、背景切换和字体选择器；不能保证所有设备恒定 120fps。已有「噪域简化模式」仍可用于低性能设备。

## 构建和 GitHub Actions

Windows 本地：在仓库运行 scripts/package-windows.ps1；产物统一放在仓库外的 dist/windows。Cargo 中间文件保留在 target。

云端操作：打开仓库 → Actions → Build iOS IPA → Run workflow → 选择分支 → Run workflow。成功后打开该次运行，在 Artifacts 下载 PhiraPro-unsigned-ipa，解压拿到 IPA。现有工作流不签名，需要自行重签安装，不会创建 GitHub Release。工作流必须已在默认分支存在，才能看到手动运行按钮。官方说明：https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow

Android 工作流已由其他开发者加入并合并：Actions → Build Android APK → Run workflow，成功后下载 PhiraPro-android-apk Artifact。工作流读取组织 Secrets：PHIRA_PRO_KEYSTORE_BASE64、PHIRA_PRO_KEYSTORE_PASSWORD、PHIRA_PRO_KEY_ALIAS、PHIRA_PRO_KEY_PASSWORD；必须允许此仓库访问这些 Secrets。日常运行不会自动发布 Release。

Android 本地：配置 JDK 21、ANDROID_HOME、ANDROID_NDK_HOME（NDK 27.2.12479018）及 cargo-ndk，在仓库运行 scripts/package-android.ps1。签名用 PHIRA_PRO_KEYSTORE_PATH 和上述三个密码/别名环境变量，或未入库的 phira-android/keystore.properties。产物放在仓库外 dist/android，版本为 0.8.2-pro.7（versionCode 42，arm64-v8a）。不提交密钥、密码或用户数据。补齐 Android HTTPS 验证的 JVM 组件，修复旧版 Android 全屏 API 调用。

本轮同步了独立服务器的新成绩上传接口；理论分数仍仅供结算显示，上传使用普通分数，正解音仍禁止上传。
