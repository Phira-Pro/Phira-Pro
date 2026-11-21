package org.flos.phirapro;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.os.Build;

import java.security.MessageDigest;

final class ApkSignatureVerifier {
    private static final String OFFICIAL_CERT_SHA256 =
            "3a513ba2bf0f73e642043fcabfc5529d60718f66ae60fd31f8b4d37d72d65bd2";

    private ApkSignatureVerifier() {}

    @SuppressWarnings("deprecation") // The legacy signing API supports Android 5.1–8.1.
    static boolean isOfficial(Context context) {
        try {
            PackageManager manager = context.getPackageManager();
            String packageName = context.getPackageName();
            Signature[] signers;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                PackageInfo info = manager.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES);
                if (info.signingInfo == null) return false;
                signers = info.signingInfo.getApkContentsSigners();
            } else {
                signers = manager.getPackageInfo(packageName, PackageManager.GET_SIGNATURES).signatures;
            }
            if (signers == null || signers.length != 1 || signers[0] == null) return false;

            byte[] digest = MessageDigest.getInstance("SHA-256").digest(signers[0].toByteArray());
            StringBuilder fingerprint = new StringBuilder(digest.length * 2);
            for (byte value : digest) {
                fingerprint.append(Character.forDigit((value & 0xff) >>> 4, 16));
                fingerprint.append(Character.forDigit(value & 0xf, 16));
            }
            return OFFICIAL_CERT_SHA256.contentEquals(fingerprint);
        } catch (Exception | LinkageError ignored) {
            return false;
        }
    }
}
