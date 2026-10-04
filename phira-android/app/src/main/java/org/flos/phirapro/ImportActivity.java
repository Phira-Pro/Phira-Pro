package org.flos.phirapro;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

import quad_native.QuadNative;

/**
 * 由系统「打开方式 → 导入到 Phira Pro」拉起：把外部文件登记到 Rust 的
 * {@code CHOSEN_FILE}，再进入主界面完成导入。
 *
 * <p>它本身不启动游戏，这样用户从文件管理器打开谱面包时不会把文件解析逻辑
 * 混进主 Activity 的启动流程。
 */
public class ImportActivity extends Activity {

    /** 传给 {@link MainActivity}，表示选中文件已由本 Activity 处理过。 */
    public static final String EXTRA_IMPORT = "phira.import";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Intent intent = getIntent();
        Uri uri = intent != null ? intent.getData() : null;
        if (uri != null) {
            String path = UriFiles.materialize(this, uri);
            if (path != null) {
                QuadNative.setChosenFile(path);
                markKind();
            }
        }

        Intent next = new Intent(this, MainActivity.class);
        next.putExtra(EXTRA_IMPORT, true);
        startActivity(next);
        finish();
    }

    /** 子类返回 true 时标记为资源包导入。 */
    protected void markKind() {
        QuadNative.markImport();
    }
}
