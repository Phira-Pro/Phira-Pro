package org.flos.phirapro;

import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;

import androidx.appcompat.app.AppCompatActivity;

import quad_native.QuadNative;

/** 由系统「打开方式 → 导入到 Phira Pro（资源包）」拉起。 */
public class ImportRespackActivity extends AppCompatActivity {

    private static final String TAG = "SAPP";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Uri data = getIntent() != null ? getIntent().getData() : null;
        if (data == null) {
            finish();
            return;
        }
        try {
            sendFileToNative(data);
            QuadNative.markImportRespack();
            startActivity(new Intent(this, MainActivity.class));
            finish();
        } catch (Throwable e) {
            Log.e(TAG, "Failed to import", e);
        }
    }

    private void sendFileToNative(Uri uri) {
        String path = UriFiles.materialize(this, uri);
        if (path == null) {
            throw new IllegalStateException("cannot read " + uri);
        }
        QuadNative.setChosenFile(path);
    }
}
