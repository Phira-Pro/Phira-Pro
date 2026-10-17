package org.flos.phirapro;

/** Pure selection rules shared by the Activity and host-JVM regression tests. */
final class RefreshRatePolicy {
    private static final float EPSILON_HZ = 0.5f;
    static int legacyMode(float requested, int width, int height, int[] widths, int[] heights, float[] rates) {
        int selected = -1;
        int lowest = -1;
        for (int i = 0; i < rates.length; i++) {
            if (widths[i] != width || heights[i] != height || Float.isNaN(rates[i]) || Float.isInfinite(rates[i]) || rates[i] <= 0f) continue;
            if (lowest < 0 || rates[i] < rates[lowest]) lowest = i;
            if (rates[i] <= requested + EPSILON_HZ && (selected < 0 || rates[i] > rates[selected])) selected = i;
        }
        return selected >= 0 ? selected : lowest;
    }
    static int windowModeId(int sdk, int legacyId) {
        // On Android 11+, Surface votes for refresh only. Pinning a physical
        // mode can override preferredRefreshRate and interfere with resizing.
        return sdk >= 30 ? 0 : legacyId;
    }
    static float windowRate(int sdk, float requested, float legacyRate) {
        if (sdk >= 34) return requested;
        // Android 11..13: don't leave a conflicting 60 Hz window vote when
        // the Surface requests 144 Hz but supportedModes is system-filtered.
        return sdk >= 30 ? 0f : legacyRate;
    }
    private RefreshRatePolicy() {}
}
