package quad_native;

import android.content.Context;
import android.net.Uri;
import android.view.MotionEvent;
import android.view.Surface;

/**
 * libphira.so 的 JNI 入口集合。
 *
 * <p>类名 `quad_native.QuadNative` 与包名由 miniquad 的 JNI 符号命名约定固定
 * （`Java_quad_1native_QuadNative_*`），不可更改。
 *
 * <p>本类只负责声明 native 方法；实际实现分别来自：
 * <ul>
 *   <li>miniquad：{@code initializeContext} / {@code activityOn*} / {@code surfaceOn*}</li>
 *   <li>Phira：{@code initializeEnvironment} / {@code prprActivityOn*} / {@code set*} 等</li>
 * </ul>
 */
public abstract class QuadNative {

    static {
        System.loadLibrary("phira");
    }

    private QuadNative() {}

    // ---------- miniquad ----------

    /** 建立 ndk_context 与 rustls 平台验证器；必须在其它 native 调用之前。 */
    public static native void initializeContext(Context context);

    /** 释放 ndk_context。 */
    public static native void releaseContext();

    /** 启动 Rust 事件循环（内部会调用 quad_main，阻塞，需在独立线程调用）。 */
    public static native void activityOnCreate(Object activity);

    public static native void activityOnResume();

    public static native void activityOnPause();

    public static native void activityOnDestroy();

    public static native void surfaceOnSurfaceCreated(Surface surface);

    public static native void surfaceOnSurfaceDestroyed(Surface surface);

    public static native void surfaceOnSurfaceChanged(Surface surface, int width, int height);

    /**
     * @param phase 0=移动 1=抬起 2=按下 3=取消
     * @param time  事件时间（毫秒）
     */
    public static native void surfaceOnTouch(int id, int phase, float x, float y, long time);

    public static native void surfaceOnKeyDown(int keycode);

    public static native void surfaceOnKeyUp(int keycode);

    public static native void surfaceOnCharacter(int character);

    // ---------- Phira ----------

    /** 初始化 inputbox 的 Android 后端；需在 {@link #initializeContext} 之后调用。 */
    public static native void initializeEnvironment();

    public static native void prprActivityOnResume();

    public static native void prprActivityOnPause();

    public static native void prprActivityOnDestroy();

    /** 数据目录（持久化）。 */
    public static native void setDataPath(String path);

    /** 缓存目录（可清理）。 */
    public static native void setTempDir(String path);

    public static native void setDpi(int dpi);

    /** 告诉 Rust「用户已经选好了这个文件」，参数应为可直接读取的本地路径。 */
    public static native void setChosenFile(String file);

    public static native void setDeepLink(String url);

    /** 标记本次选择用于导入谱面（配合 {@link #setChosenFile}）。 */
    public static native void markImport();

    /** 标记本次选择用于导入资源包（配合 {@link #setChosenFile}）。 */
    public static native void markImportRespack();

    public static native void setInputText(String text);

    public static native void preprocessInput(MotionEvent event, float x, float y, boolean z, boolean z2);

    /** 导出：把已打开的 SAF 文件描述符交给 Rust 写入。 */
    public static native void processExportFd(Uri uri, int fd);
}
