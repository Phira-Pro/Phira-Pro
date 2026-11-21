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
import android.os.ParcelFileDescriptor;
import android.provider.MediaStore;
import android.util.Log;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Date;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.Executor;
import java.util.concurrent.Executors;
import java.util.regex.Pattern;

/** Bounded, continuous app-only logcat capture. No root or storage permission. */
final class CrashLogRecorder {
    private static final String TAG = "PhiraProLogs";
    private static final String DIRECTORY = Environment.DIRECTORY_DOWNLOADS + "/PhiraPro/logs/";
    private static final String PREFIX = "PhiraPro-";
    private static final long MIB = 1024L * 1024;
    private static final long SEGMENT_BYTES = 8 * MIB;
    private static final long DEFAULT_STORAGE_BYTES = 32 * MIB;
    private static final String STORAGE_LIMIT_KEY = "storageLimitBytes";
    private static final Pattern LOG_NAMES = Pattern.compile("PhiraPro-[0-9]{8}-[0-9]{6}-[0-9]{3}-[0-9]+\\.log(?:\\.txt)?");
    private static volatile CrashLogRecorder instance;
    private final Context context;
    private final Object lock = new Object();
    private final Executor maintenance = Executors.newSingleThreadExecutor(task -> {
        Thread thread = new Thread(task, "PhiraPro-log-retention");
        thread.setDaemon(true);
        return thread;
    });
    private OutputStream output;
    private Uri currentUri;
    private File currentFile;
    private long bytes;
    private long retainedBytes;
    private long storageBytes;
    private long lastFileTime;
    private boolean publicLogsTracked;
    private String version = "unknown";

    private CrashLogRecorder(Context context) {
        this.context = context.getApplicationContext();
        long saved = context.getSharedPreferences("diagnostics", Context.MODE_PRIVATE).getLong(STORAGE_LIMIT_KEY, DEFAULT_STORAGE_BYTES);
        storageBytes = saved >= SEGMENT_BYTES && saved <= 256 * MIB ? saved : DEFAULT_STORAGE_BYTES;
    }

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
        recorder.maintenance.execute(() -> {
            synchronized (recorder.lock) {
                try {
                    recorder.closeOutput();
                    recorder.currentUri = null;
                    recorder.currentFile = null;
                    recorder.prune(true, 0);
                    recorder.openOutput();
                    recorder.write("Previous diagnostic logs cleared by user.\n".getBytes(StandardCharsets.UTF_8));
                } catch (Exception error) {
                    Log.w(TAG, "Cannot clear diagnostics", error);
                }
            }
        });
    }

    static void setStorageLimit(int mib) {
        CrashLogRecorder recorder = instance;
        if (recorder == null) return;
        long limit = Math.max(8, Math.min(256, mib)) * MIB;
        recorder.maintenance.execute(() -> {
            synchronized (recorder.lock) {
                if (recorder.storageBytes == limit) return;
                recorder.storageBytes = limit;
                recorder.context.getSharedPreferences("diagnostics", Context.MODE_PRIVATE)
                        .edit().putLong(STORAGE_LIMIT_KEY, limit).apply();
                recorder.prune(false, 0);
            }
        });
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
                byte[] data = (text + "\n").getBytes(StandardCharsets.UTF_8);
                if (output == null || bytes + data.length > SEGMENT_BYTES || (currentUri != null && !publicLogsTracked)) openOutput();
                int remaining = (int) (SEGMENT_BYTES - bytes);
                if (data.length > remaining) {
                    int end = remaining - 1;
                    while ((data[end] & 0xc0) == 0x80) end--;
                    data = Arrays.copyOf(data, end + 1);
                    data[end] = '\n';
                }
                if (retainedBytes + bytes + data.length > storageBytes) prune(false, data.length);
                write(data);
            } catch (Exception ignored) {
                // A full/unavailable disk must never crash the application.
                closeOutput();
            }
        }
    }

    private void write(byte[] data) throws IOException {
        if (retainedBytes + bytes + data.length > storageBytes) return;
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
        bytes = 0;
        prune(false, 0);
        lastFileTime = Math.max(System.currentTimeMillis(), lastFileTime + 1);
        String name = PREFIX + new SimpleDateFormat("yyyyMMdd-HHmmss-SSS", Locale.ROOT).format(new Date(lastFileTime))
                + "-" + android.os.Process.myPid() + ".log.txt";
        if (Build.VERSION.SDK_INT >= 29 && publicLogsTracked) {
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
        byte[] header = ("Phira Pro " + version + "; pid=" + android.os.Process.myPid()
                + "; Android=" + Build.VERSION.RELEASE + " (" + Build.VERSION.SDK_INT + ")"
                + "; device=" + Build.MANUFACTURER + " " + Build.MODEL
                + "; ABI=" + Arrays.toString(Build.SUPPORTED_ABIS)
                + "\nLog directory: " + (currentFile != null ? currentFile.getParent() : DIRECTORY) + "\n").getBytes(StandardCharsets.UTF_8);
        if (retainedBytes + header.length > storageBytes) prune(false, header.length);
        write(header);
    }

    private static final class StoredLog {
        final String name;
        final long size;
        final Uri uri;
        final File file;

        StoredLog(String name, long size, Uri uri, File file) {
            this.name = name;
            this.size = size;
            this.uri = uri;
            this.file = file;
        }
    }

    private boolean collectPublicLogs(List<StoredLog> logs) {
        if (Build.VERSION.SDK_INT >= 29) {
            try (Cursor cursor = context.getContentResolver().query(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    new String[] { MediaStore.Downloads._ID, MediaStore.Downloads.DISPLAY_NAME, MediaStore.Downloads.SIZE },
                    MediaStore.Downloads.RELATIVE_PATH + "=? AND " + MediaStore.MediaColumns.OWNER_PACKAGE_NAME + "=?",
                    new String[] { DIRECTORY, context.getPackageName() }, null)) {
                if (cursor != null) while (cursor.moveToNext()) {
                    String name = cursor.getString(1);
                    if (name == null || !LOG_NAMES.matcher(name).matches()) continue;
                    Uri uri = ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cursor.getLong(0));
                    if (uri.equals(currentUri)) continue;
                    long size = Math.max(0, cursor.getLong(2));
                    // The provider may not have updated SIZE for a recently closed stream.
                    try (ParcelFileDescriptor fd = context.getContentResolver().openFileDescriptor(uri, "r")) {
                        if (fd != null && fd.getStatSize() >= 0) size = fd.getStatSize();
                    } catch (IOException | SecurityException ignored) { }
                    logs.add(new StoredLog(name, size, uri, null));
                }
                return cursor != null;
            } catch (Exception ignored) {
                // Keep private diagnostics working if shared storage is unavailable.
            }
        }
        return false;
    }

    private void prune(boolean all, long incoming) {
        List<StoredLog> logs = new ArrayList<>();
        publicLogsTracked = collectPublicLogs(logs);
        for (File directory : new File[] { context.getExternalFilesDir("logs"), new File(context.getFilesDir(), "logs") }) {
            if (directory == null) continue;
            File[] files = directory.listFiles((dir, name) -> LOG_NAMES.matcher(name).matches());
            if (files == null) continue;
            for (File file : files) {
                if (file.isFile() && !file.equals(currentFile)) logs.add(new StoredLog(file.getName(), file.length(), null, file));
            }
        }
        Collections.sort(logs, (left, right) -> left.name.compareTo(right.name));
        retainedBytes = 0;
        for (StoredLog log : logs) retainedBytes += log.size;
        for (StoredLog log : logs) {
            if (!all && retainedBytes + bytes + incoming <= storageBytes) break;
            try {
                boolean deleted = log.file != null ? log.file.delete() : context.getContentResolver().delete(log.uri, null, null) > 0;
                if (deleted) retainedBytes -= log.size;
            } catch (SecurityException ignored) {
                // Leave rows the app can no longer delete and account for their size.
            }
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
