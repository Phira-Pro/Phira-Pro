# pro.8

## 性能与高刷新率

- 修复底层窗口忽略单采样设置、无条件启用 4 倍 MSAA 的问题。主界面使用单采样；游玩抗锯齿仍由原有设置控制。
- Android 请求同分辨率下最高可用刷新率，最高 120 Hz，并在前后台恢复、屏幕模式变化与 Surface 重建后重新申请。
- iOS 使用公开 CADisplayLink 配置 ProMotion 帧率范围；暂停原有 GLK 自动循环，避免重复渲染，并处理前后台暂停恢复。
- 噪域按时间事件维护可见索引，每帧只计算可见区域；判定与两层绘制共用可见索引，复用变换缓冲区。
- 遮罩对可证明等价的混合使用扫描线差分、重复几何合并与原有 R8 量化查表。混合次序影响结果的情况保留原顺序。
- 缓存静态/关键帧几何；遮罩噪声仅在可能受影响的范围计算。辅助纹理仅在内容变化时传输，没有可见噪域和触摸特效时跳过合成。
- GLES3 场景快照采用官方 1/6 分辨率，减少复制带宽；不适用的多采样系统帧缓冲及 GLES2 保留兼容路径。
- 不改变几何、缓动、覆盖规则、噪声相位、音符层级或触摸判定，不限制实际显示的噪域数量。

设置新增「Shader 预渲染」，默认关闭。开启后在加载阶段预热启用的故事板着色器并准备噪域静态/关键帧几何，加载会更慢。动态效果、音符背景反馈与触摸仍实时渲染；这不是整个谱面的视频缓存，收益随谱面和设备而变化。

## iOS 安装与数据

工程 Debug/Release 的 Bundle ID 均改为 `org.flos.phirapro`，与官版 `org.flos.phira` 区分。Actions 继续生成未签名 IPA，并在打包前检查 Bundle ID；本机 LocalSigning 配置不变。签名工具仍需选择原来的账号/Team，不能将标识改回官版。

侧载新版通常可直接覆盖旧版、保留数据，需要同一有效 Bundle ID、签名 Team 和安装工具配置。免费 Personal Team 的描述文件 7 天到期，需要续签。若以前重签工具设置了另一个独立 ID，应沿用它；改变 ID 会形成另一个安装，需要备份迁移。参考 [Apple](https://developer.apple.com/help/account/basics/about-your-developer-account) 与 [Sideloadly](https://sideloadly.io/)。

移动端现在可使用系统文件界面备份和还原 ZIP。备份包含当前内存设置的 data.json、谱面、皮肤、字体、外观等文件，不包含可重建缓存与根目录已有的 phira-backup ZIP。备份含账号状态，请自己保管。单个 data.json 仅保存索引、设置、记录等，不能替代谱面/皮肤/字体文件；迁移应保留整个备份。

还原先验证 data.json，再完整解压验证文件，之后发布；同步主线程数据，避免下次保存被旧状态覆盖。设置等 JSON 使用临时文件同步后替换。字体、皮肤等还原后请重启。桌面从官版数据目录导入仍需要 data.json 旁的资源目录；修复减少动态效果字段的命名兼容。

Android 导出使用重复 fd，写完后关闭文件提供方保留的描述符，修复文件界面显示 0 B；取消导出后可以重试，打不开保存位置时不再静默等待。

## 验证与限制

核心 63 项、应用 11 项回归通过；含遮罩混合、时间倒退、静态几何缓存、字体、备份往返及损坏备份不覆盖旧数据。真实桌面 GPU 检查覆盖单采样/4 倍窗口和现有局内 MSAA 管线，对官方 ActiveBlock 合成 GLSL 的比较最大通道差为 0；这项比较不代表整个 CPU 遮罩已逐像素等同官方全部多 Pass。

真实谱面数百帧的 CPU 遮罩与优化前逐字节相同。桌面 2560×1600 的ハテ若干片段 CPU 遮罩约降低 11%～31%；合成的 8000 个重复 Disabled/Ready 区域压力用例由约 1740 ms 降至 2.5 ms。这是合成压力测试，不是实机游玩帧率。Desultory Signals 整个噪域 CPU/GPU 路径的桌面收益相对较小，约 7%～9%，仍需移动端测量。

高刷新率申请不保证设备系统在省电、热降频或 GPU 压力下始终提供 120 FPS。iOS 驱动模块完成 aarch64 类型检查；Windows 无 Apple SDK，IPA 完整构建交由 GitHub Actions。实体 Android/iPad 的最终帧率、耗电、温度及系统生命周期仍需实测。

最终 Android arm64 签名 APK 在只读 API 37 模拟器中通过冷启动、覆盖安装、前后台恢复、系统文件备份/还原及取消重试检查。实际导出 ZIP 的 data.json 可读；还原为开启预渲染后，冷启动、覆盖安装和再次备份仍保留设置。最终导出的 6.19 MB 文件在系统列表显示正确大小。实际 GameScene 开启预渲染后完成 Desultory Signals、ハテ的加载与 28 个时间点绘制，没有 GL 错误；截图为功能检查，未使用视频对齐元数据时按谱面秒数取帧，不作为新一轮官方逐帧一致性证明。

回归命令：`cargo test -p prpr --lib`、`cargo test -p phira --lib`、`cargo check -p phira --lib`。GPU 检查：`cargo run -p prpr --example block_area_gpu`，设置 `BLOCK_GPU_SAMPLES=1` 或 `4`；官方资源路径可通过 `PHIRA_OFFICIAL_SRC` 指定。完整噪域桌面测量：`cargo run --release -p prpr --example block_area_render_benchmark`。

本轮按要求不 push、不发布 Release。Windows ZIP 和 Android APK 仅放在工作区外侧的 dist 对应平台目录；成功打包后只清理符合既定名称的 pro.6 及更早包，保留 pro.7 和用户数据目录。
