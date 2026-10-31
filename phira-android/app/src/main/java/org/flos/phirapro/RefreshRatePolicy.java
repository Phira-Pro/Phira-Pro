package org.flos.phirapro;

/** Pure selection rules shared by the Activity and host-JVM regression tests. */
final class RefreshRatePolicy {
    // Shared persisted value with the Rust configuration, never sent as Hz.
    static final int UNLIMITED_FPS = 0xffff;
    private static final float EPSILON_HZ = 0.5f;
    static float requestedRate(float requested, float maximum) {
        if (requested != UNLIMITED_FPS) return requested;
        return !Float.isNaN(maximum) && !Float.isInfinite(maximum) && maximum > 0f ? maximum : 120f;
    }
    static int legacyMode(float requested, int width, int height, int[] widths, int[] heights, float[] rates) {
        int selected = -1;
        int highest = -1;
        for (int i = 0; i < rates.length; i++) {
            if (widths[i] != width || heights[i] != height || Float.isNaN(rates[i]) || Float.isInfinite(rates[i]) || rates[i] <= 0f) continue;
            if (highest < 0 || rates[i] > rates[highest]) highest = i;
            // A refresh request is not a render cap. Prefer the lowest mode
            // that can meet it: requesting 120 on a 60/144 panel needs 144.
            if (rates[i] + EPSILON_HZ >= requested && (selected < 0 || rates[i] < rates[selected])) selected = i;
        }
        return selected >= 0 ? selected : highest;
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
