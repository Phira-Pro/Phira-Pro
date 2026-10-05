package org.flos.phirapro;

import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;

import androidx.appcompat.app.AppCompatActivity;

import quad_native.QuadNative;

/**
 * 处理深链入口：把原始 URL 原样交给 Rust 的 parse_deeplink 统一解析。
 */
public class DeepLinkActivity extends AppCompatActivity {

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
            QuadNative.setDeepLink(data.toString());
            startActivity(new Intent(this, MainActivity.class));
        } catch (Throwable e) {
            Log.e(TAG, "Failed to handle deep link", e);
        }
        finish();
    }
}
