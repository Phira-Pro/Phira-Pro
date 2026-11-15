package org.flos.phirapro;

import android.content.Context;
import android.net.Uri;
import android.util.Log;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.UUID;

/** 把外部 Uri 转成 Rust 可读的本地路径：总是复制进应用缓存目录。 */
final class UriFiles {

    private static final String TAG = "PhiraPro";
    private static final String PREFIX = "phira-import-";
    private static final long STALE_MS = 24L * 60 * 60 * 1000;

    private UriFiles() {}

    /**
     * Copy the referenced file into the app cache and return that path.
     *
     * Rust opens the returned path directly, so it must never point at external
     * storage: on Android 7+ that requires a runtime permission the game does
     * not hold for "open with" intents. Copying also keeps the source stable
     * while the import runs.
     */
    static String materialize(Context context, Uri uri) {
        try {
            File out = new File(context.getCacheDir(), PREFIX + UUID.randomUUID() + suffix(uri));
            try (InputStream in = open(context, uri);
                 OutputStream os = new FileOutputStream(out)) {
                if (in == null) {
                    return null;
                }
                byte[] buffer = new byte[64 * 1024];
                int n;
                while ((n = in.read(buffer)) > 0) {
                    os.write(buffer, 0, n);
                }
            }
            cleanup(context);
            return out.getAbsolutePath();
        } catch (Exception e) {
            Log.w(TAG, "materialize failed: " + uri, e);
            return null;
        }
    }

    private static InputStream open(Context context, Uri uri) throws Exception {
        if ("file".equals(uri.getScheme())) {
            String path = uri.getPath();
            if (path == null) {
                return null;
            }
            return new FileInputStream(path);
        }
        return context.getContentResolver().openInputStream(uri);
    }

    /** Keep the source extension so the importer and logs stay recognizable. */
    private static String suffix(Uri uri) {
        String name = uri.getLastPathSegment();
        if (name == null) {
            return "";
        }
        int dot = name.lastIndexOf('.');
        if (dot < 0 || dot == name.length() - 1) {
            return "";
        }
        String suffix = name.substring(dot);
        return suffix.length() <= 16 ? suffix : "";
    }

    private static void cleanup(Context context) {
        File dir = context.getCacheDir();
        File[] files = dir.listFiles();
        if (files == null) {
            return;
        }
        long now = System.currentTimeMillis();
        for (File file : files) {
            if (file.getName().startsWith(PREFIX) && now - file.lastModified() > STALE_MS) {
                if (!file.delete()) {
                    Log.w(TAG, "failed to remove stale import " + file);
                }
            }
        }
    }
}
