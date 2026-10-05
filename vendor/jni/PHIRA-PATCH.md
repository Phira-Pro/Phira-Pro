# Android JNI 字节返回值修复

来源：jni-rs 0.22.4（crates.io 原始源码，MIT / Apache-2.0）。

只修改 `src/macros.rs`：Android 上把 JNI 布尔返回函数按 JNI 的
`unsigned char` ABI 读取，再转成 Rust bool。JNI 0.22 使用 jni-sys 0.4
的 bool 签名，未规范化的返回寄存器可能被当成 true，导致
`ExceptionCheck` 报告异常而 `ExceptionOccurred` 返回 null。
原版 arm64 APK 在 Android 17 的 ARM 转译环境可稳定复现启动 abort。

覆盖异常检查、对象类型 / 相等检查、布尔方法和字段读取。
其他 JNI API、参数、非 Android 平台行为保持上游实现。
Cargo 的 patch 统一作用于输入框和 HTTPS 验证器，避免只修启动入口
而在后续输入或网络请求时重复触发。Android Gradle 将此目录纳入构建输入。

JNI ABI：https://docs.oracle.com/en/java/javase/22/docs/specs/jni/types.html
上游源码：https://github.com/jni-rs/jni-rs/tree/v0.22.4
