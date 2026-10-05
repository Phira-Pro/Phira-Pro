来源：Phira-Pro/prpr-macroquad，提交 450d7d8dac44127a5261acb7a714168780a78bbe。
保留原有 MIT / Apache-2.0 许可证。仅随项目保存运行库及宏源码，移除了引用未随包保存的示例项目的 dev-dependencies。

pro.8：Window::from_config 将调用者指定的 sample_count 原样传给 miniquad，移除无条件覆盖为 4 的行为。Phira Pro 默认窗口使用单采样，局内抗锯齿由 Config.sample_count / MSRenderTarget 管理，避免叠加两次 MSAA。

此补丁在本仓库构建，未修改 Cargo 下载缓存或远程依赖仓库。
