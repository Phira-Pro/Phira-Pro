package org.flos.phirapro;

import android.app.ActivityManager;
import android.app.ApplicationExitInfo;
import android.content.ContentUris;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import android.util.Log;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Date;
import java.util.List;
import java.util.Locale;

/** Bounded, continuous app-only logcat capture. No root or storage permission. */
final class CrashLogRecorder {
    private static final String TAG = "PhiraProLogs";
    private static final String DIRECTORY = Environment.DIRECTORY_DOWNLOADS + "/PhiraPro/logs/";
    private static final String PREFIX = "PhiraPro-";
    private static final long MAX_BYTES = 8L * 1024 * 1024;
    private static final int MAX_FILES = 8;
    private static CrashLogRecorder instance;
    private final Context context;
    private final Object lock = new Object();
    private OutputStream output;
    private Uri currentUri;
    private File currentFile;
    private long bytes;
    private String version = "unknown";

    private CrashLogRecorder(Context context) { this.context = context.getApplicationContext(); }

    static synchronized void start(Context context) {
        if (instance != null) return;
        CrashLogRecorder recorder = new CrashLogRecorder(context);
        instance = recorder;
        Thread.UncaughtExceptionHandler previous = Thread.getDefaultUncaughtExceptionHandler();
        Thread.setDefaultUncaughtExceptionHandler((thread, error) -> {
            recorder.record("JAVA UNCAUGHT on " + thread.getName() + "\n" + Log.getStackTraceString(error));
            if (previous != null) previous.uncaughtException(thread, error);
        });
        Thread worker = new Thread(recorder::capture, "PhiraPro-logcat");
        worker.setDaemon(true);
        worker.start();
    }

    static void clear() {
        CrashLogRecorder recorder = instance;
        if (recorder == null) return;
        // Filesystem/MediaStore work must not stall UI or the native render loop.
        new Thread(() -> {
            synchronized (recorder.lock) {
                try {
                    recorder.closeOutput();
                    recorder.currentUri = null;
                    recorder.currentFile = null;
                    recorder.prune(true);
                    recorder.openOutput();
                    recorder.write("Previous diagnostic logs cleared by user.\n");
                } catch (Exception error) {
                    Log.w(TAG, "Cannot clear diagnostics", error);
                }
            }
        }, "PhiraPro-clear-logs").start();
    }

    private void capture() {
        java.lang.Process logcat = null;
        try {
            version = context.getPackageManager().getPackageInfo(context.getPackageName(), 0).versionName;
            synchronized (lock) { openOutput(); }
            // Include the process's existing startup records, then follow live.
            // --pid prevents capturing other apps' logs, tokens or personal data.
            logcat = new ProcessBuilder("logcat", "-b", "main", "-b", "system", "-b", "crash",
                    "-v", "threadtime", "--pid=" + android.os.Process.myPid())
                    .redirectErrorStream(true).start();
            recordPreviousExit();
            try (BufferedReader reader = new BufferedReader(new InputStreamReader(logcat.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) record(line);
            }
        } catch (Exception error) {
            record("Log capture failed: " + Log.getStackTraceString(error));
            Log.w(TAG, "Log capture failed", error);
        } finally {
            if (logcat != null) logcat.destroy();
            synchronized (lock) { closeOutput(); }
        }
    }

    private void record(String text) {
        synchronized (lock) {
            try {
                if (output == null || bytes >= MAX_BYTES) openOutput();
                write(text + "\n");
            } catch (Exception ignored) {
                // A full/unavailable disk must never crash the application.
                closeOutput();
            }
        }
    }

    private void write(String text) throws java.io.IOException {
        byte[] data = text.getBytes(StandardCharsets.UTF_8);
        output.write(data);
        output.flush(); // Persist preceding records even on a native abort.
        bytes += data.length;
    }

    private void closeOutput() {
        if (output != null) {
            try { output.close(); } catch (Exception ignored) { }
            output = null;
        }
    }

    private void openOutput() throws Exception {
        closeOutput();
        currentUri = null;
        currentFile = null;
        String name = PREFIX + new SimpleDateFormat("yyyyMMdd-HHmmss-SSS", Locale.ROOT).format(new Date())
                + "-" + android.os.Process.myPid() + ".log.txt";
        if (Build.VERSION.SDK_INT >= 29) {
            try {
                ContentValues values = new ContentValues();
                values.put(MediaStore.Downloads.DISPLAY_NAME, name);
                values.put(MediaStore.Downloads.MIME_TYPE, "text/plain");
                values.put(MediaStore.Downloads.RELATIVE_PATH, DIRECTORY);
                // Make a live log visible immediately; a killed process cannot
                // finalize IS_PENDING during shutdown.
                values.put(MediaStore.Downloads.IS_PENDING, 0);
                currentUri = context.getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
                if (currentUri != null) output = context.getContentResolver().openOutputStream(currentUri, "w");
            } catch (Exception error) { Log.w(TAG, "Public log unavailable; using app files", error); }
        }
        if (output == null) {
            File directory = context.getExternalFilesDir("logs");
            if (directory == null) directory = new File(context.getFilesDir(), "logs");
            if (!directory.isDirectory() && !directory.mkdirs()) throw new java.io.IOException("Cannot create log directory");
            currentFile = new File(directory, name);
            output = new FileOutputStream(currentFile);
        }
        bytes = 0;
        write("Phira Pro " + version + "; pid=" + android.os.Process.myPid()
                + "; Android=" + Build.VERSION.RELEASE + " (" + Build.VERSION.SDK_INT + ")"
                + "; device=" + Build.MANUFACTURER + " " + Build.MODEL
                + "; ABI=" + Arrays.toString(Build.SUPPORTED_ABIS)
                + "\nLog directory: " + (currentFile != null ? currentFile.getParent() : DIRECTORY) + "\n");
        prune(false);
    }

    private boolean ownLog(String name) {
        // MediaStore may append .txt to text/plain files. Accept both names so
        // rotation and cache cleanup also cover existing provider-renamed logs.
        return name != null && name.matches("PhiraPro-[0-9]{8}-[0-9]{6}-[0-9]{3}-[0-9]+\\.log(?:\\.txt)?");
    }

    private void prune(boolean all) {
        if (Build.VERSION.SDK_INT >= 29) {
            // Query/delete only our rows in our exact directory. Never delete
            // unrelated Downloads files, including documents selected via SAF.
            List<Uri> logs = new ArrayList<>();
            try (Cursor cursor = context.getContentResolver().query(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    new String[] { MediaStore.Downloads._ID, MediaStore.Downloads.DISPLAY_NAME },
                    MediaStore.Downloads.RELATIVE_PATH + "=? AND " + MediaStore.MediaColumns.OWNER_PACKAGE_NAME + "=?",
                    new String[] { DIRECTORY, context.getPackageName() }, MediaStore.Downloads._ID + " DESC")) {
                if (cursor != null) while (cursor.moveToNext()) {
                    if (ownLog(cursor.getString(1))) logs.add(ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cursor.getLong(0)));
                }
                for (int i = 0; i < logs.size(); i++) {
                    Uri uri = logs.get(i);
                    if ((all || i >= MAX_FILES) && !uri.equals(currentUri)) context.getContentResolver().delete(uri, null, null);
                }
            } catch (Exception error) { Log.w(TAG, "Cannot prune public logs", error); }
        }
        for (File directory : new File[] { context.getExternalFilesDir("logs"), new File(context.getFilesDir(), "logs") }) {
            if (directory == null) continue;
            File[] logs = directory.listFiles((dir, name) -> ownLog(name));
            if (logs == null) continue;
            Arrays.sort(logs, Comparator.comparing(File::getName).reversed());
            for (int i = 0; i < logs.length; i++) if ((all || i >= MAX_FILES) && !logs[i].equals(currentFile)) logs[i].delete();
        }
    }

    private void recordPreviousExit() {
        if (Build.VERSION.SDK_INT < 30) return;
        try {
            ActivityManager manager = (ActivityManager) context.getSystemService(Context.ACTIVITY_SERVICE);
            long seen = context.getSharedPreferences("diagnostics", Context.MODE_PRIVATE).getLong("lastExit", 0);
            long newest = seen;
            for (ApplicationExitInfo exit : manager.getHistoricalProcessExitReasons(context.getPackageName(), 0, 3)) {
                if (exit.getTimestamp() <= seen) continue;
                newest = Math.max(newest, exit.getTimestamp());
                record("PREVIOUS EXIT: time=" + exit.getTimestamp() + "; pid=" + exit.getPid()
                        + "; reason=" + exit.getReason() + "; status=" + exit.getStatus()
                        + "; importance=" + exit.getImportance() + "; PSS_KiB=" + exit.getPss()
                        + "; RSS_KiB=" + exit.getRss() + "; description=" + exit.getDescription());
                try (InputStream trace = exit.getTraceInputStream()) {
                    if (trace == null) continue;
                    byte[] buffer = new byte[8192];
                    int total = 0, count;
                    while (total < 2 * 1024 * 1024 && (count = trace.read(buffer)) > 0) {
                        // Native tombstones may be protobuf; preserve their
                        // bytes instead of corrupting them with UTF-8 decoding.
                        record("EXIT TRACE BASE64: " + android.util.Base64.encodeToString(buffer, 0, count, android.util.Base64.NO_WRAP));
                        total += count;
                    }
                    if (total >= 2 * 1024 * 1024) record("Exit trace truncated at 2 MiB.");
                }
            }
            context.getSharedPreferences("diagnostics", Context.MODE_PRIVATE).edit().putLong("lastExit", newest).apply();
        } catch (Exception error) { record("Previous exit report unavailable: " + error); }
    }
}
