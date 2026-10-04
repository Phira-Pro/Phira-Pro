package org.flos.phirapro;

import quad_native.QuadNative;

/** 由系统「打开方式 → 导入到 Phira Pro（资源包）」拉起。 */
public class ImportRespackActivity extends ImportActivity {

    @Override
    protected void markKind() {
        QuadNative.markImportRespack();
    }
}
