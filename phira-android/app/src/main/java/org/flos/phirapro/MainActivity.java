package org.flos.phirapro;

import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;

import androidx.appcompat.app.AppCompatActivity;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import quad_native.QuadNative;

/** 游戏主 Activity：只做胶水，游戏本体在 libphira.so。 */
public class MainActivity extends AppCompatActivity {

    private static final String TAG = "PhiraPro";

    private static final int REQ_CHOOSE_FILE = 1001;
    private static final int REQ_CHOOSE_PHOTO = 1002;
    private static final int REQ_EXPORT = 1003;

    private QuadSurface view;

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
        enforceLandscape();
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
        Log.i(TAG, "Activity onPause");
        if (view != null) view.cancelTouches("activity-paused");
        super.onPause();
        QuadNative.activityOnPause();
        QuadNative.prprActivityOnPause();
    }

    @Override
    protected void onDestroy() {
        Log.i(TAG, "Activity onDestroy");
        super.onDestroy();
        QuadNative.releaseContext();
        QuadNative.activityOnDestroy();
        QuadNative.prprActivityOnDestroy();
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
