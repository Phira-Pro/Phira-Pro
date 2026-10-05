package quad_native;

import android.content.Context;
import android.net.Uri;
import android.view.MotionEvent;
import android.view.Surface;

/**
 * libphira.so 的 JNI 入口集合。
 *
 * <p>类名与包名由 miniquad 的 JNI 符号命名约定固定（Java_quad_1native_QuadNative_*），不可更改。
 */
public abstract class QuadNative {

    static {
        System.loadLibrary("phira");
    }

    private QuadNative() {}

    public static native void initializeContext(Context context);

    public static native void releaseContext();

    public static native void activityOnCreate(Object activity);

    public static native void activityOnResume();

    public static native void activityOnPause();

    public static native void activityOnDestroy();

    public static native void surfaceOnSurfaceCreated(Surface surface);

    public static native void surfaceOnSurfaceDestroyed(Surface surface);

    public static native void surfaceOnSurfaceChanged(Surface surface, int width, int height);

    /** @param phase 0=移动 1=抬起 2=按下 3=取消 */
    public static native void surfaceOnTouch(int id, int phase, float x, float y, long time);

    public static native void initializeEnvironment(Context context);

    public static native void prprActivityOnResume();

    public static native void prprActivityOnPause();

    public static native void prprActivityOnDestroy();

    public static native void setDataPath(String path);

    public static native void setTempDir(String path);

    public static native void setDpi(int dpi);

    /** @param file 可直接读取的本地路径 */
    public static native void setChosenFile(String file);

    public static native void setDeepLink(String url);

    public static native void markImport();

    public static native void markImportRespack();

    public static native void setInputText(String text);

    public static native void preprocessInput(MotionEvent event, float x, float y, boolean z, boolean z2);

    /** @param fd 所有权转移给 Rust，调用方不得再 close */
    public static native void processExportFd(Uri uri, int fd);
}
