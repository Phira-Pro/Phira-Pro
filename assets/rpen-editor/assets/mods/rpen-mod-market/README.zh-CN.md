# Nyamon 模组市场

作者：NyamonZone。基于作者提供的 1.0.2 版本内置，保留模组 ID `rpen-mod-market`；RPEN 补充了取消处理、中英文界面、说明和回归测试。编辑器与模组协议版本不因此改变。

## 打开与使用

1. 点击编辑器右上角“模组”，选择“Nyamon 模组市场”并启用。新安装默认禁用；启用本身不会请求市场数据。
2. 点击管理器中的“模组市场”，或该模组的同名操作，打开独立窗口。关闭窗口恢复编辑，停用模组移除它添加的按钮与补丁。
3. 搜索或选择类别后打开模组详情，可查看版本、依赖与反馈，复制发布页地址，安装当前或历史版本。
4. 下载及校验整个依赖计划后，各包分别使用编辑器的风险确认窗口，勾选确认才可写入。取消不改动原有包。市场不能覆盖任何内置模组，也不能更新自身。

安装默认不执行新代码。替换正在使用的模组或依赖时，窗口会列出受影响的模组；“安装后恢复启用”默认未勾选，勾选会在安装后执行新版本。原有模组参数继续保留。缺失依赖先准备，依赖环会拒绝安装。

## 更新、网络与来源

“更新”页面默认只检查清单标记属于当前市场的包，也可切换“手动选择”并勾选其他包。点击“检查更新”时才提交所选模组的 ID、版本和内容哈希，页面显示实际接收通道；打开市场不自动提交完整本地模组列表。浏览会请求分类、搜索结果、详情和封面，点击“发送”会提交填写的反馈。市场不上传谱面、音频、AI 对话或 API Key。

模组参数中的“市场地址”用于发布页及来源匹配，“API 通道”用于 WebSocket 请求。两项可配置为自建服务；修改后取消旧请求并关闭当前窗口，重新打开时使用新配置。界面提示以实际 API 通道为准。内置默认服务由 NyamonZone 提供，不是 RPEN 的谱面存储或联机服务器。

ZIP 与展开文件均进行哈希校验，并检查 ID、版本、路径和大小。来源字段和内容哈希用于识别包及检查完整性，风险确认仍适用于市场安装的外部代码。

## 更新失败与恢复

覆盖前先保存受影响旧包及参数的恢复副本。副本无法保存时，不停用或覆盖原有包。写入失败时按实际存储核验恢复，完整的旧包不会被先行删除。

如果暂时不能恢复，市场保留备份并显示实际安装版本和错误状态，暂停新的安装。重新打开市场可点击“恢复旧包”重试，或“导出旧包”获得含旧版 `.rpenmod` 和参数记录的 ZIP。手动恢复成功后保持禁用，由用户决定再次启用；恢复确认前保留副本。

恢复副本存放在当前浏览器来源或桌面运行配置的独立 IndexedDB `rpen-market-recovery` 中，不写入谱面。普通模组安装仍使用宿主平台接口，网页端与桌面端各用原有后端。清理网站或桌面运行数据会影响尚待恢复的副本，需保存时可先导出。

关闭加载中的窗口、切换服务和停用属于预期取消，不累计模组故障。真实异常仍显示原因和重试入口，并遵循宿主的错误恢复规则。

## English

Enable **Nyamon Mod Market** in the top-right mod manager, then choose **Mod market**. It is disabled by default. Browsing contacts the configured service; checking updates sends only the selected mods’ IDs, versions and content hashes. The default scope covers mods marked as originating from the current market.

Every downloaded package, including dependencies and replacements, requires the editor’s consent dialog. New code stays disabled unless the user explicitly chooses to re-enable affected mods. Built-in packages and the market itself cannot be replaced through the market.

Updates keep recovery copies before replacing files. If recovery is pending, reopen the market to restore or export the previous packages. Recovery copies are local and do not modify charts. Closing an unfinished request is handled as cancellation rather than a mod failure.
