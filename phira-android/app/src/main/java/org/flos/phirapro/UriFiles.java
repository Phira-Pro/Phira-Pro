package org.flos.phirapro;

import android.content.Context;
import android.net.Uri;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.UUID;

/**
 * 把外部 Uri 变成 Rust 侧可直接读取的本地路径。
 *
 * <p>SAF 给出的 {@code content://} 不能用普通文件 API 打开，这里统一落到缓存目录，
 * 与 iOS 壳「先复制到临时路径再交给 Rust」的做法一致。
 */
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
