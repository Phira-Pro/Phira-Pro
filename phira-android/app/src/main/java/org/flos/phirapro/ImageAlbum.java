package org.flos.phirapro;

import android.Manifest;
import android.app.Activity;
import android.content.ContentValues;
import android.content.pm.PackageManager;
import android.media.MediaScannerConnection;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.OutputStream;
import java.util.UUID;

/** Native gallery publication. Called by a background worker, never the render loop. */
final class ImageAlbum {
    static String save(Activity activity, String sourcePath, String suggestedName) {
        Uri uri = null;
        try {
            File source = new File(sourcePath);
            if (!source.isFile()) throw new IllegalArgumentException("分析图片不存在");
            String name = new File(suggestedName).getName();
            if (!name.endsWith(".png")) throw new IllegalArgumentException("需要 PNG 图片");
            if (Build.VERSION.SDK_INT >= 29) {
                ContentValues values = new ContentValues();
                values.put(MediaStore.Images.Media.DISPLAY_NAME, name);
                values.put(MediaStore.Images.Media.MIME_TYPE, "image/png");
                values.put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Phira Pro");
                values.put(MediaStore.Images.Media.IS_PENDING, 1);
                uri = activity.getContentResolver().insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values);
                if (uri == null) throw new java.io.IOException("无法创建相册图片");
                try (OutputStream output = activity.getContentResolver().openOutputStream(uri)) {
                    if (output == null) throw new java.io.IOException("无法写入相册");
                    copy(source, output);
                }
                values.clear(); values.put(MediaStore.Images.Media.IS_PENDING, 0);
                if (activity.getContentResolver().update(uri, values, null, null) == 0)
                    throw new java.io.IOException("相册发布失败");
                return uri.toString();
            }
            if (Build.VERSION.SDK_INT >= 23 && activity.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
                activity.runOnUiThread(() -> activity.requestPermissions(new String[]{Manifest.permission.WRITE_EXTERNAL_STORAGE}, 1201));
                return "ERROR:请允许图片写入权限后再次点击保存图片";
            }
            File directory = new File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES), "Phira Pro");
            if (!directory.isDirectory() && !directory.mkdirs()) throw new java.io.IOException("无法创建图片目录");
            File target = new File(directory, name);
            if (target.exists()) target = new File(directory, UUID.randomUUID() + "-" + name);
            try (OutputStream output = new FileOutputStream(target)) { copy(source, output); }
            MediaScannerConnection.scanFile(activity, new String[]{target.getAbsolutePath()}, new String[]{"image/png"}, null);
            return target.getAbsolutePath();
        } catch (Exception error) {
            if (uri != null) try { activity.getContentResolver().delete(uri, null, null); } catch (Exception ignored) {}
            return "ERROR:" + error.getMessage();
        }
    }
    private static void copy(File source, OutputStream output) throws java.io.IOException {
        try (FileInputStream input = new FileInputStream(source)) {
            byte[] buffer = new byte[65536]; int count;
            while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
        }
    }
}
