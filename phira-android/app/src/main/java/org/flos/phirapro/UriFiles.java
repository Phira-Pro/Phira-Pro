package org.flos.phirapro;

import android.content.Context;
import android.net.Uri;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.UUID;

/** 把外部 Uri 转成 Rust 可读的本地路径：SAF 的 content:// 先复制到缓存目录。 */
final class UriFiles {

    private static final String TAG = "PhiraPro";

    private UriFiles() {}

    static String materialize(Context context, Uri uri) {
        try {
            if ("file".equals(uri.getScheme())) {
                return uri.getPath();
            }
            File out = new File(context.getCacheDir(), "phira-import-" + UUID.randomUUID());
            try (InputStream in = context.getContentResolver().openInputStream(uri);
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
            return out.getAbsolutePath();
        } catch (Exception e) {
            Log.w(TAG, "materialize failed: " + uri, e);
            return null;
        }
    }
}
