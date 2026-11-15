package org.flos.phirapro;

import android.Manifest;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.util.Log;
import android.widget.Toast;

import androidx.annotation.NonNull;
import androidx.appcompat.app.AppCompatActivity;

import java.io.File;

import quad_native.QuadNative;

/** 由系统「打开方式 → 导入到 Phira Pro」拉起（谱面包）。 */
public class ImportActivity extends AppCompatActivity {

    private static final String TAG = "SAPP";
    private static final int REQ_READ_STORAGE = 1101;

    private Uri pending;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Uri data = getIntent() != null ? getIntent().getData() : null;
        if (data == null) {
            finish();
            return;
        }
        // A file:// URI outside the app sandbox is unreadable without the
        // legacy storage permission; ask for it once and retry after the user
        // answers instead of failing with EACCES inside the native importer.
        if (needsStoragePermission(data)) {
            pending = data;
            requestPermissions(new String[]{Manifest.permission.READ_EXTERNAL_STORAGE}, REQ_READ_STORAGE);
            return;
        }
        importUri(data);
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, @NonNull String[] permissions, @NonNull int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode != REQ_READ_STORAGE) {
            return;
        }
        boolean granted = grantResults.length > 0 && grantResults[0] == PackageManager.PERMISSION_GRANTED;
        if (granted && pending != null) {
            importUri(pending);
        } else {
            Toast.makeText(this, R.string.import_read_failed, Toast.LENGTH_LONG).show();
            finish();
        }
        pending = null;
    }

    private boolean needsStoragePermission(Uri uri) {
        if (!"file".equals(uri.getScheme()) || Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return false;
        }
        String path = uri.getPath();
        if (path == null) {
            return false;
        }
        if (new File(path).canRead()) {
            return false;
        }
        return checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED;
    }

    private void importUri(Uri data) {
        String path = UriFiles.materialize(this, data);
        if (path == null) {
            Toast.makeText(this, R.string.import_read_failed, Toast.LENGTH_LONG).show();
            finish();
            return;
        }
        try {
            QuadNative.setChosenFile(path);
            QuadNative.markImport();
            startActivity(new Intent(this, MainActivity.class));
        } catch (Throwable e) {
            Log.e(TAG, "Failed to import", e);
            Toast.makeText(this, R.string.import_read_failed, Toast.LENGTH_LONG).show();
        }
        finish();
    }
}
