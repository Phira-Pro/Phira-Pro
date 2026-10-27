package org.flos.phirapro;

import android.content.ContentValues;
import android.content.Intent;
import android.content.res.Configuration;
import android.content.pm.ActivityInfo;
import android.hardware.display.DisplayManager;
import android.media.MediaScannerConnection;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.os.Bundle;
import android.os.Handler;
import android.os.ParcelFileDescriptor;
import android.os.PowerManager;
import android.provider.MediaStore;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Display;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;

import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.io.OutputStream;

import androidx.appcompat.app.AppCompatActivity;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import quad_native.QuadNative;
import org.json.JSONArray;
import org.json.JSONObject;

/** 游戏主 Activity：只做胶水，游戏本体在 libphira.so。 */
public class MainActivity extends AppCompatActivity {

    private static final String TAG = "PhiraPro";
    private static final int[] SUPPORTED_GAME_REFRESH_RATES = {60, 120, 144, 165, 185, 200, 240};

    private static final int REQ_CHOOSE_FILE = 1001;
    private static final int REQ_CHOOSE_PHOTO = 1002;
    private static final int REQ_EXPORT = 1003;

    private QuadSurface view;

    /**
     * 把渲染好的视频发布到系统的「下载」目录。
     *
     * <p>Android 10（API 29）起公共目录只能通过 MediaStore 写，写进去之后文件管理器里就能
     * 直接在「下载/PhiraPro」看到；更老的系统直接往公共目录拷贝并通知媒体库扫描。
     *
     * @return 给玩家看的保存位置，失败返回 null。
     */
    public String publishVideo(String sourcePath, String displayName) {
        File source = new File(sourcePath);
        if (!source.isFile()) {
            Log.w(TAG, "publishVideo: source missing " + sourcePath);
            return null;
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ContentValues values = new ContentValues();
                values.put(MediaStore.MediaColumns.DISPLAY_NAME, displayName);
                values.put(MediaStore.MediaColumns.MIME_TYPE, "video/mp4");
                values.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/PhiraPro");
                Uri uri = getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
                if (uri == null) {
                    Log.w(TAG, "publishVideo: MediaStore insert failed");
                    return null;
                }
                try (OutputStream out = getContentResolver().openOutputStream(uri);
                        InputStream in = new FileInputStream(source)) {
                    if (out == null) {
                        return null;
                    }
                    byte[] buffer = new byte[1 << 16];
                    int read;
                    while ((read = in.read(buffer)) > 0) {
                        out.write(buffer, 0, read);
                    }
                }
                return "下载/PhiraPro/" + displayName;
            }
            File dir = new File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "PhiraPro");
            if (!dir.isDirectory() && !dir.mkdirs()) {
                Log.w(TAG, "publishVideo: cannot create " + dir);
                return null;
            }
            File target = new File(dir, displayName);
            try (OutputStream out = new java.io.FileOutputStream(target);
                    InputStream in = new FileInputStream(source)) {
                byte[] buffer = new byte[1 << 16];
                int read;
                while ((read = in.read(buffer)) > 0) {
                    out.write(buffer, 0, read);
                }
            }
            MediaScannerConnection.scanFile(this, new String[] { target.getAbsolutePath() }, null, null);
            return target.getAbsolutePath();
        } catch (Exception e) {
            Log.w(TAG, "publishVideo failed", e);
            return null;
        }
    }
    private DisplayManager displayManager;
    private float requestedFrameRate = 120f;
    private float reportedMaxRefreshRate;
    private boolean activityResumed;
    private final Handler refreshHandler = new Handler(android.os.Looper.getMainLooper());
    private final Runnable refreshRetry = () -> {
        if (view != null && activityResumed) applyRequestedFrameRate(true, "window settled");
    };
    private volatile String displayDiagnostics = "{}";
    private String lastDisplaySummary = "";
    private final DisplayManager.DisplayListener displayListener = new DisplayManager.DisplayListener() {
        @Override public void onDisplayAdded(int displayId) {}
        @Override public void onDisplayRemoved(int displayId) {}
        @Override public void onDisplayChanged(int displayId) {
            if (displayId == getWindowManager().getDefaultDisplay().getDisplayId()) {
                applyRequestedFrameRate(false, "display changed");
            }
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        supportRequestWindowFeature(Window.FEATURE_NO_TITLE);
        super.onCreate(savedInstanceState);
        enforceLandscape();
        Log.i(TAG, "Activity onCreate: native initialization begins");
        Log.i("PhiraProTouch", "device=" + Build.MANUFACTURER + "/" + Build.MODEL
                + " android=" + Build.VERSION.RELEASE + " sdk=" + Build.VERSION.SDK_INT);

        view = new QuadSurface(this);
        setContentView(view);
        displayManager = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        displayManager.registerDisplayListener(displayListener, new Handler(getMainLooper()));
        requestRefreshRateUpdate("create");

        WindowInsetsControllerCompat insets = WindowCompat.getInsetsController(getWindow(), view);
        insets.hide(WindowInsetsCompat.Type.statusBars());
        insets.hide(WindowInsetsCompat.Type.navigationBars());

        QuadNative.setDataPath(getFilesDir().getAbsolutePath());
        QuadNative.setTempDir(getCacheDir().getAbsolutePath());
        DisplayMetrics dm = getResources().getDisplayMetrics();
        // 必须传物理 DPI，不能传 densityDpi：本机两者相差近 1.4 倍，用错会让缩放与触控命中偏位。
        QuadNative.setDpi((int) Math.min(dm.xdpi, dm.ydpi));

        // 必须在 UI 线程调用：miniquad 把消息通道建在调用线程的 thread-local 上，而所有回调都来自 UI 线程。
        QuadNative.initializeContext(this);
        QuadNative.initializeEnvironment(this);
        QuadNative.activityOnCreate(this);
        Log.i(TAG, "Activity onCreate: native initialization returned");
    }

    @Override
    protected void onResume() {
        super.onResume();
        activityResumed = true;
        enforceLandscape();
        requestRefreshRateUpdate("resume");
        Log.i(TAG, "Activity onResume");
        QuadNative.activityOnResume();
        QuadNative.prprActivityOnResume();
    }

    private void enforceLandscape() {
        // SENSOR_LANDSCAPE ignores the system rotation lock while preserving
        // both landscape directions. Reapply after returning from a picker.
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE);
    }

    @Override
    protected void onPause() {
        activityResumed = false;
        refreshHandler.removeCallbacks(refreshRetry);
        Log.i(TAG, "Activity onPause");
        if (view != null) view.cancelTouches("activity-paused");
        super.onPause();
        QuadNative.activityOnPause();
        QuadNative.prprActivityOnPause();
    }

    @Override
    protected void onDestroy() {
        refreshHandler.removeCallbacks(refreshRetry);
        Log.i(TAG, "Activity onDestroy");
        if (displayManager != null) {
            displayManager.unregisterDisplayListener(displayListener);
        }
        super.onDestroy();
        QuadNative.releaseContext();
        QuadNative.activityOnDestroy();
        QuadNative.prprActivityOnDestroy();
    }

    public void setRequestedFrameRate(int frameRate) {
        boolean supported = false;
        for (int rate : SUPPORTED_GAME_REFRESH_RATES) {
            supported |= frameRate == rate;
        }
        if (!supported) {
            throw new IllegalArgumentException("Unsupported requested frame rate: " + frameRate);
        }
        runOnUiThread(() -> {
            requestedFrameRate = frameRate;
            requestRefreshRateUpdate("user setting");
        });
    }

    void requestRefreshRateUpdate(String reason) {
        if (view == null) return;
        refreshHandler.removeCallbacks(refreshRetry);
        applyRequestedFrameRate(true, reason);
        // Window/surface changes may settle after the initial callback. Retry
        // twice, without a perpetual timer or rearming on display notifications.
        refreshHandler.postDelayed(refreshRetry, 300);
        refreshHandler.postDelayed(refreshRetry, 1000);
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (activityResumed) requestRefreshRateUpdate(focused ? "focus restored" : "visible under overlay");
    }

    @Override public void onTopResumedActivityChanged(boolean topResumed) {
        super.onTopResumedActivityChanged(topResumed);
        if (activityResumed) requestRefreshRateUpdate(topResumed ? "top resumed" : "multi-resume under overlay");
    }

    @Override public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        requestRefreshRateUpdate("configuration changed");
    }

    @Override public void onMultiWindowModeChanged(boolean multiWindow) {
        super.onMultiWindowModeChanged(multiWindow);
        requestRefreshRateUpdate(multiWindow ? "multi-window" : "fullscreen");
    }

    public String getDisplayDiagnostics() { return displayDiagnostics; }

    private void applyRequestedFrameRate(boolean force, String reason) {
        Display display = getWindowManager().getDefaultDisplay();
        float maxRefreshRate = display.getRefreshRate();
        float selectedRate = -1f;
        Display.Mode selectedMode = null;
        int selectedModeId = 0;

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            Display.Mode currentMode = display.getMode();
            Display.Mode[] modes = display.getSupportedModes();
            int[] widths = new int[modes.length];
            int[] heights = new int[modes.length];
            float[] rates = new float[modes.length];
            for (int i = 0; i < modes.length; i++) {
                Display.Mode mode = modes[i];
                float rate = mode.getRefreshRate();
                widths[i] = mode.getPhysicalWidth();
                heights[i] = mode.getPhysicalHeight();
                rates[i] = rate;
                maxRefreshRate = Math.max(maxRefreshRate, rate);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    for (float alternative : mode.getAlternativeRefreshRates()) maxRefreshRate = Math.max(maxRefreshRate, alternative);
                }
            }
            int selected = RefreshRatePolicy.legacyMode(requestedFrameRate, currentMode.getPhysicalWidth(),
                    currentMode.getPhysicalHeight(), widths, heights, rates);
            if (selected >= 0) selectedMode = modes[selected];
            if (selectedMode == null) {
                selectedMode = currentMode;
            }
            selectedRate = selectedMode.getRefreshRate();
            selectedModeId = selectedMode.getModeId();
        } else {
            for (float rate : display.getSupportedRefreshRates()) {
                maxRefreshRate = Math.max(maxRefreshRate, rate);
                if (rate <= requestedFrameRate + 0.5f && rate > selectedRate) {
                    selectedRate = rate;
                }
            }
            if (selectedRate < 0f) {
                for (float rate : display.getSupportedRefreshRates()) {
                    if (selectedRate < 0f || rate < selectedRate) selectedRate = rate;
                }
            }
            if (selectedRate < 0f) selectedRate = display.getRefreshRate();
        }

        if (Math.abs(reportedMaxRefreshRate - maxRefreshRate) > 0.01f) {
            reportedMaxRefreshRate = maxRefreshRate;
            QuadNative.setMaxRefreshRate(maxRefreshRate);
        }
        Window window = getWindow();
        WindowManager.LayoutParams attributes = window.getAttributes();
        float windowRate = RefreshRatePolicy.windowRate(Build.VERSION.SDK_INT, requestedFrameRate, selectedRate);
        int windowMode = RefreshRatePolicy.windowModeId(Build.VERSION.SDK_INT, selectedModeId);
        boolean changed = Math.abs(attributes.preferredRefreshRate - windowRate) > 0.01f;
        attributes.preferredRefreshRate = windowRate;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            changed |= attributes.preferredDisplayModeId != windowMode;
            attributes.preferredDisplayModeId = windowMode;
        }
        if (changed || force) {
            window.setAttributes(attributes);
        }
        // Keep the user's vote intact on Android 11+: a temporary 60 Hz mode
        // report must not silently replace a persisted 144 Hz request.
        if (view != null) view.setTargetFrameRate(Build.VERSION.SDK_INT >= Build.VERSION_CODES.R ? requestedFrameRate : selectedRate,
                force, force && view.hasWindowFocus());
        recordDisplayState(display, windowRate, windowMode, maxRefreshRate, reason);
    }

    private void recordDisplayState(Display display, float windowRate, int windowMode, float maximum, String reason) {
        try {
            PowerManager power = (PowerManager) getSystemService(POWER_SERVICE);
            boolean multiWindow = Build.VERSION.SDK_INT >= Build.VERSION_CODES.N && isInMultiWindowMode();
            JSONObject state = new JSONObject().put("requested_hz", requestedFrameRate)
                    .put("reported_display_hz", display.getRefreshRate()).put("reported_maximum_hz", maximum)
                    .put("window_preferred_hz", windowRate).put("window_mode_id", windowMode)
                    .put("multi_window", multiWindow).put("low_power", power != null && power.isPowerSaveMode())
                    .put("surface_valid", view != null && view.getHolder().getSurface().isValid());
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && power != null) state.put("thermal_status", power.getCurrentThermalStatus());
            JSONArray modes = new JSONArray();
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                for (Display.Mode mode : display.getSupportedModes()) {
                    modes.put(new JSONObject().put("id", mode.getModeId()).put("hz", mode.getRefreshRate())
                            .put("width", mode.getPhysicalWidth()).put("height", mode.getPhysicalHeight()));
                }
            }
            state.put("supported_modes", modes);
            String summary = state.toString();
            if (!summary.equals(lastDisplaySummary)) {
                Log.i(TAG, "Display state (" + reason + "): " + summary);
                lastDisplaySummary = summary;
            }
            displayDiagnostics = state.put("reason", reason).put("uptime_ms", android.os.SystemClock.uptimeMillis()).toString();
        } catch (org.json.JSONException error) {
            Log.w(TAG, "Cannot capture display diagnostics", error);
        }
    }

    /** miniquad 回调。setDecorFitsSystemWindows 是 API 30 才有的，低版本走旧的 setSystemUiVisibility。 */
    public void setFullScreen(final boolean fullscreen) {
        runOnUiThread(() -> {
            Window window = getWindow();
            View decorView = window.getDecorView();
            if (fullscreen) {
                window.setFlags(WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                        WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    window.getAttributes().layoutInDisplayCutoutMode =
                            WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    window.setDecorFitsSystemWindows(false);
                } else {
                    decorView.setSystemUiVisibility(View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
                }
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                window.setDecorFitsSystemWindows(true);
            }
            requestRefreshRateUpdate("fullscreen preference changed");
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

    public void chooseFile() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            try {
                startActivityForResult(intent, REQ_CHOOSE_FILE);
            } catch (Exception e) {
                Log.w(TAG, "chooseFile failed", e);
            }
        });
    }

    /** Called by the existing cache-clear setting through JNI. */
    public void clearDiagnosticLogs() {
        CrashLogRecorder.clear();
    }

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
            }
        });
    }

    public String saveImageToAlbum(String sourcePath, String suggestedName) {
        return ImageAlbum.save(this, sourcePath, suggestedName);
    }

    public void openUrl(final String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (Exception e) {
                Log.w(TAG, "openUrl failed: " + url, e);
            }
        });
    }

    /** 导出目标是用户通过 SAF 亲自选定的真实文件，删除会毁掉用户内容，所以这里什么都不做。 */
    public void deleteUri(Uri uri) {
        Log.i(TAG, "deleteUri ignored (SAF destination is user-owned): " + uri);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode != RESULT_OK || data == null) return;
        Uri uri = data.getData();
        if (uri == null) return;

        switch (requestCode) {
            case REQ_CHOOSE_FILE:
            case REQ_CHOOSE_PHOTO: {
                String path = UriFiles.materialize(this, uri);
                QuadNative.setChosenFile(path != null ? path : "");
                break;
            }
            case REQ_EXPORT: {
                try {
                    ParcelFileDescriptor pfd = getContentResolver().openFileDescriptor(uri, "w");
                    if (pfd == null) return;
                    // fd 所有权交给 Rust，不能 close
                    QuadNative.processExportFd(uri, pfd.detachFd());
                } catch (Exception e) {
                    Log.w(TAG, "export fd failed: " + uri, e);
                }
                break;
            }
            default:
                break;
        }
    }
}
