package org.flos.phirapro;

import android.app.Application;
import android.content.Context;
import android.os.Process;
import android.util.Log;

public final class PhiraApplication extends Application {
    @Override protected void attachBaseContext(Context base) {
        super.attachBaseContext(base);
        if (!BuildConfig.DEBUG && !ApkSignatureVerifier.isOfficial(base)) {
            try {
                Process.killProcess(Process.myPid());
            } finally {
                Runtime.getRuntime().halt(1);
            }
        }
    }

    @Override public void onCreate() {
        super.onCreate();
        CrashLogRecorder.start(this);
    }

    @Override public void onTrimMemory(int level) {
        Log.w("PhiraPro", "Memory pressure: " + level);
        super.onTrimMemory(level);
    }
}
