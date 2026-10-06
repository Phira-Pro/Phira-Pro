package org.flos.phirapro;

import android.app.Application;
import android.util.Log;

/** Start diagnostics before Activity construction and native library loading. */
public final class PhiraApplication extends Application {
    @Override public void onCreate() {
        super.onCreate();
        CrashLogRecorder.start(this);
    }

    @Override public void onTrimMemory(int level) {
        Log.w("PhiraPro", "Memory pressure: " + level);
        super.onTrimMemory(level);
    }
}
