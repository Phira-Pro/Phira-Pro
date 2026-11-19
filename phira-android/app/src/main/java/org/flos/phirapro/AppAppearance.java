package org.flos.phirapro;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ShortcutInfo;
import android.content.pm.ShortcutManager;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.drawable.Icon;
import android.os.Build;
import android.util.Log;

import java.util.Arrays;
import java.util.Collections;

final class AppAppearance {
    private static final String TAG = "PhiraPro";
    private static final String SHORTCUT_ID = "custom-launcher-icon";

    private AppAppearance() {}

    static boolean selectLauncherIcon(Activity activity, boolean reset) {
        PackageManager manager = activity.getPackageManager();
        String appPackage = activity.getPackageName();
        ComponentName standard = new ComponentName(appPackage, "org.flos.phirapro.LauncherDefault");
        ComponentName alternate = new ComponentName(appPackage, "org.flos.phirapro.LauncherAlternate");
        try {
            boolean switchToAlternate = !reset
                    && manager.getComponentEnabledSetting(alternate) != PackageManager.COMPONENT_ENABLED_STATE_ENABLED;
            ComponentName enable = switchToAlternate ? alternate : standard;
            ComponentName disable = switchToAlternate ? standard : alternate;
            int flags = PackageManager.DONT_KILL_APP;
            if (Build.VERSION.SDK_INT >= 33) {
                manager.setComponentEnabledSettings(Arrays.asList(
                        new PackageManager.ComponentEnabledSetting(enable, PackageManager.COMPONENT_ENABLED_STATE_ENABLED, flags),
                        new PackageManager.ComponentEnabledSetting(disable, PackageManager.COMPONENT_ENABLED_STATE_DISABLED, flags)));
            } else {
                // Enable the replacement first so the launcher always has an entry.
                manager.setComponentEnabledSetting(enable, PackageManager.COMPONENT_ENABLED_STATE_ENABLED, flags);
                manager.setComponentEnabledSetting(disable, PackageManager.COMPONENT_ENABLED_STATE_DISABLED, flags);
            }
            if (reset && Build.VERSION.SDK_INT >= 26) {
                ShortcutManager shortcuts = activity.getSystemService(ShortcutManager.class);
                if (shortcuts != null && isPinned(shortcuts)) {
                    shortcuts.updateShortcuts(Collections.singletonList(shortcut(activity,
                            Icon.createWithResource(activity, R.mipmap.ic_launcher))));
                }
            }
            return true;
        } catch (RuntimeException e) {
            Log.w(TAG, "Could not change the launcher icon", e);
            return false;
        }
    }

    static boolean pinCustomIcon(Activity activity, String path) {
        if (Build.VERSION.SDK_INT < 26) return false;
        try {
            ShortcutManager manager = activity.getSystemService(ShortcutManager.class);
            if (manager == null || !manager.isRequestPinShortcutSupported()) return false;
            BitmapFactory.Options options = new BitmapFactory.Options();
            options.inJustDecodeBounds = true;
            BitmapFactory.decodeFile(path, options);
            int max = Math.max(options.outWidth, options.outHeight);
            if (max <= 0) return false;
            options.inSampleSize = 1;
            while ((long) max > 384L * options.inSampleSize) options.inSampleSize *= 2;
            options.inJustDecodeBounds = false;
            Bitmap bitmap = BitmapFactory.decodeFile(path, options);
            if (bitmap == null) return false;
            int width = bitmap.getWidth();
            int height = bitmap.getHeight();
            float scale = Math.min(1f, 192f / Math.max(width, height));
            Bitmap icon = Bitmap.createScaledBitmap(bitmap, Math.max(1, Math.round(width * scale)),
                    Math.max(1, Math.round(height * scale)), true);
            ShortcutInfo info = shortcut(activity, Icon.createWithBitmap(icon));
            return isPinned(manager)
                    ? manager.updateShortcuts(Collections.singletonList(info)) : manager.requestPinShortcut(info, null);
        } catch (RuntimeException e) {
            Log.w(TAG, "Could not create the custom launcher shortcut", e);
            return false;
        }
    }

    private static boolean isPinned(ShortcutManager manager) {
        for (ShortcutInfo shortcut : manager.getPinnedShortcuts()) {
            if (SHORTCUT_ID.equals(shortcut.getId())) return true;
        }
        return false;
    }

    private static ShortcutInfo shortcut(Activity activity, Icon icon) {
        return new ShortcutInfo.Builder(activity, SHORTCUT_ID)
                .setShortLabel(activity.getString(R.string.app_name))
                .setIntent(new Intent(Intent.ACTION_MAIN, null, activity, MainActivity.class))
                .setIcon(icon)
                .build();
    }
}
