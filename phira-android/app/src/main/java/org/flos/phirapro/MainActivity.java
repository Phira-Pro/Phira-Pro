package org.flos.phirapro;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.os.Build;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import android.view.Window;
import android.view.View;
import android.view.WindowManager;
import android.view.Display;
import android.hardware.display.DisplayManager;
import android.view.inputmethod.InputMethodManager;
import android.widget.Toast;

import quad_native.QuadNative;

/**
 * 游戏主 Activity：只做「胶水」，游戏本体在 libphira.so 里。
 *
 * <p>启动顺序（不可颠倒）：
 * <ol>
 *   <li>{@code initializeContext} —— 建立 ndk_context / rustls</li>
 *   <li>{@code initializeEnvironment} —— inputbox Android 后端（依赖上一步的 context）</li>
 *   <li>{@code setDataPath} / {@code setTempDir} / {@code setDpi}</li>
 *   <li>{@code activityOnCreate} —— 在 UI 线程建立事件通道；miniquad 自行创建渲染线程</li>
 * </ol>
 *
 * <p>Rust 侧会回调本类的方法：{@code showExportDialog} / {@code chooseFile} /
 * {@code choosePhoto} / {@code deleteUri}，以及 miniquad 的
 * {@code setFullScreen} / {@code showKeyboard}。这些都可能在非 UI 线程被调用，
 * 因此统一用 {@code runOnUiThread} 包一层。
 */
public class MainActivity extends Activity {

    private static final String TAG = "PhiraPro";

    private static final int REQ_CHOOSE_FILE = 1001;
    private static final int REQ_CHOOSE_PHOTO = 1002;
    private static final int REQ_EXPORT = 1003;

    private QuadSurface view;
    private ParcelFileDescriptor exportDescriptor;
    private DisplayManager displayManager;
    private final DisplayManager.DisplayListener displayListener = new DisplayManager.DisplayListener() {
        public void onDisplayAdded(int id) { }
        public void onDisplayRemoved(int id) { }
        public void onDisplayChanged(int id) { requestHighRefreshRate(); }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);

        // 1 + 2：建立上下文并初始化 inputbox。
        // initializeEnvironment 内部用 find_class 查 moe/mivik/inputbox/InputBox，
        // 必须跑在挂载了应用类加载器的线程上——即 onCreate（UI 线程）里调用；
        // 不能挪到后面启动游戏的那个工作线程。
        Log.i(TAG, "Startup: initializeContext");
        QuadNative.initializeContext(this);
        Log.i(TAG, "Startup: initializeEnvironment");
        QuadNative.initializeEnvironment(this);

        // 3：目录与 DPI
        Log.i(TAG, "Startup: setDataPath");
        QuadNative.setDataPath(getFilesDir().getAbsolutePath());
        Log.i(TAG, "Startup: setTempDir");
        QuadNative.setTempDir(getCacheDir().getAbsolutePath());
        Log.i(TAG, "Startup: setDpi");
        QuadNative.setDpi(getResources().getDisplayMetrics().densityDpi);

        // 处理启动 intent（深链 / 直接打开的谱面包）
        handleIntent(getIntent());

        // miniquad stores its event sender in thread-local storage on the
        // calling thread, then starts its own render thread and returns.
        // Activity / Surface callbacks run on the UI thread, so initialize
        // the sender here before any of those callbacks can fire.
        Log.i(TAG, "Startup: activityOnCreate");
        QuadNative.activityOnCreate(this);
        Log.i(TAG, "Startup: createSurface");

        view = new QuadSurface(this);
        setContentView(view);
        displayManager = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        if (displayManager != null) displayManager.registerDisplayListener(displayListener, null);
        requestHighRefreshRate();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    private void handleIntent(Intent intent) {
        if (intent == null) return;
        // Import*Activity 已经把 CHOSEN_FILE 设置好了，这里不要覆盖
        if (intent.getBooleanExtra(ImportActivity.EXTRA_IMPORT, false)) return;

        Uri data = intent.getData();
        if (data == null) return;
        String scheme = data.getScheme();
        if ("http".equals(scheme) || "https".equals(scheme) || "phira".equals(scheme)) {
            QuadNative.setDeepLink(data.toString());
        } else {
            String path = UriFiles.materialize(this, data);
            if (path != null) {
                QuadNative.setChosenFile(path);
                QuadNative.markImport();
            }
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        Log.i(TAG, "Lifecycle: onResume");
        requestHighRefreshRate();
        QuadNative.prprActivityOnResume();
        QuadNative.activityOnResume();
    }

    @Override
    protected void onPause() {
        QuadNative.prprActivityOnPause();
        QuadNative.activityOnPause();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        finishExport();
        if (displayManager != null) displayManager.unregisterDisplayListener(displayListener);
        QuadNative.prprActivityOnDestroy();
        QuadNative.activityOnDestroy();
        super.onDestroy();
    }

    // ------------------------------------------------------------------
    // miniquad 回调
    // ------------------------------------------------------------------

    public void setFullScreen(final boolean fullscreen) {
        runOnUiThread(() -> {
            Window window = getWindow();
            if (fullscreen) {
                window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    WindowManager.LayoutParams params = window.getAttributes();
                    params.layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
                    window.setAttributes(params);
                }
            } else {
                window.clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                window.setDecorFitsSystemWindows(!fullscreen);
            }
            // Keep immersive mode on older Android versions as well.
            window.getDecorView().setSystemUiVisibility(fullscreen
                    ? View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_FULLSCREEN
                      | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                      | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    : View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
        });
    }

    public void showKeyboard(final boolean show) {
        runOnUiThread(() -> {
            InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
            if (imm == null) return;
            if (show) {
                imm.showSoftInput(view, 0);
            } else if (view != null) {
                imm.hideSoftInputFromWindow(view.getWindowToken(), 0);
            }
        });
    }

    // ------------------------------------------------------------------
    // Rust 回调：文件选择 / 导出 / 删除
    // ------------------------------------------------------------------

    /** Rust 请求选择任意文件（导入谱面等）。 */
    public void chooseFile() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            try {
                startActivityForResult(intent, REQ_CHOOSE_FILE);
            } catch (Exception e) {
                Log.w(TAG, "chooseFile failed", e);
                Toast.makeText(this, "无法打开文件选择器", Toast.LENGTH_LONG).show();
            }
        });
    }

    /** Rust 请求从相册选择图片（自定义图标 / 背景 / 立绘）。 */
    public void choosePhoto() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("image/*");
            try {
                startActivityForResult(intent, REQ_CHOOSE_PHOTO);
            } catch (Exception e) {
                Log.w(TAG, "choosePhoto failed", e);
            }
        });
    }

    /** Rust 请求把内容导出到用户选择的位置（SAF CREATE_DOCUMENT）。 */
    public void showExportDialog(final String suggestedName) {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_TITLE, suggestedName);
            try {
                startActivityForResult(intent, REQ_EXPORT);
            } catch (Exception e) {
                Log.w(TAG, "showExportDialog failed", e);
                QuadNative.processExportFd(null, -2);
            }
        });
    }

    /**
     * Rust 在导出完成后回调，用于「清理」。
     *
     * <p>注意：Android 的导出目标是用户通过 SAF 亲自选定的真实文件，
     * 删除它会直接毁掉用户刚导出的内容，因此这里刻意不做删除。
     */
    public void deleteUri(Uri uri) {
        finishExport();
        Log.i(TAG, "deleteUri ignored (SAF destination is user-owned): " + uri);
    }

    /** Keep the provider's close notification alive while Rust owns a duplicate fd. */
    public void finishExport() {
        runOnUiThread(() -> {
            if (exportDescriptor != null) {
                try { exportDescriptor.close(); }
                catch (java.io.IOException e) { Log.w(TAG, "export close failed", e); }
                exportDescriptor = null;
            }
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        Log.i(TAG, "File result: request=" + requestCode + ", result=" + resultCode);
        if (resultCode != RESULT_OK || data == null) {
            if (requestCode == REQ_EXPORT) QuadNative.processExportFd(null, -1);
            if (requestCode == REQ_CHOOSE_FILE || requestCode == REQ_CHOOSE_PHOTO) QuadNative.setChosenFile("");
            return;
        }
        Uri uri = data.getData();
        if (uri == null) {
            if (requestCode == REQ_EXPORT) QuadNative.processExportFd(null, -2);
            return;
        }

        switch (requestCode) {
            case REQ_CHOOSE_FILE:
            case REQ_CHOOSE_PHOTO: {
                String path = UriFiles.materialize(this, uri);
                if (path != null) {
                    QuadNative.setChosenFile(path);
                } else {
                    QuadNative.setChosenFile("");
                    Toast.makeText(this, "无法读取所选文件，请确认文件已下载到本机", Toast.LENGTH_LONG).show();
                }
                break;
            }
            case REQ_EXPORT: {
                try {
                    ParcelFileDescriptor pfd = getContentResolver().openFileDescriptor(uri, "w");
                    if (pfd == null) {
                        QuadNative.processExportFd(null, -2);
                        return;
                    }
                    exportDescriptor = pfd;
                    // Rust owns the duplicate. Close the original after the
                    // write, so SAF refreshes the displayed size and cloud data.
                    QuadNative.processExportFd(uri, ParcelFileDescriptor.dup(pfd.getFileDescriptor()).detachFd());
                } catch (Exception e) {
                    finishExport();
                    Log.w(TAG, "export fd failed: " + uri, e);
                    QuadNative.processExportFd(null, -2);
                }
                break;
            }
            default:
                break;
        }
    }
    /** Rendering is paced by VSync; advertise the desired rate instead of accepting the 60 Hz game default. */
    @SuppressWarnings("deprecation")
    public void requestHighRefreshRate() {
        if (view == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return;
        Display display = view.getDisplay();
        if (display == null) return;
        Display.Mode current = display.getMode();
        Display.Mode fastest = current;
        for (Display.Mode mode : display.getSupportedModes()) {
            // Changing refresh rate must not switch resolution or affect chart/input coordinates.
            if (mode.getPhysicalWidth() == current.getPhysicalWidth()
                    && mode.getPhysicalHeight() == current.getPhysicalHeight()
                    && mode.getRefreshRate() > fastest.getRefreshRate()) fastest = mode;
        }
        float rate = Math.min(120f, fastest.getRefreshRate());
        WindowManager.LayoutParams params = getWindow().getAttributes();
        int modeId = Build.VERSION.SDK_INT < Build.VERSION_CODES.R ? fastest.getModeId() : 0;
        if (Math.abs(params.preferredRefreshRate - rate) > 0.1f || params.preferredDisplayModeId != modeId) {
            params.preferredRefreshRate = rate;
            params.preferredDisplayModeId = modeId;
            getWindow().setAttributes(params);
            Log.i(TAG, "Refresh request=" + rate + " Hz, display=" + current.getRefreshRate() + " Hz");
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && view.getNativeSurface().isValid()) {
            view.getNativeSurface().setFrameRate(rate, android.view.Surface.FRAME_RATE_COMPATIBILITY_DEFAULT);
        }
    }
}
