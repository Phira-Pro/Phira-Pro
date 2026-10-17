package org.flos.phirapro;

public final class RefreshRatePolicyRegression {
    private static int checks;
    private static void equal(float expected, float actual) {
        checks++;
        if (Math.abs(expected - actual) > 0.001f) throw new AssertionError(expected + " != " + actual);
    }
    public static void main(String[] args) {
        int[] widths = {2880, 2880, 2880, 1920};
        int[] heights = {1800, 1800, 1800, 1080};
        float[] rates = {60f, 120.0001f, 144.0002f, 240f};
        equal(1, RefreshRatePolicy.legacyMode(120, 2880, 1800, widths, heights, rates));
        equal(2, RefreshRatePolicy.legacyMode(144, 2880, 1800, widths, heights, rates));
        equal(0, RefreshRatePolicy.legacyMode(60, 2880, 1800, widths, heights, rates));
        equal(2, RefreshRatePolicy.legacyMode(240, 2880, 1800, widths, heights, rates));
        equal(-1, RefreshRatePolicy.legacyMode(144, 800, 600, widths, heights, rates));
        equal(0, RefreshRatePolicy.legacyMode(30, 2880, 1800, widths, heights, rates));
        for (int sdk : new int[] {30, 31, 33, 34, 35}) {
            equal(0, RefreshRatePolicy.windowModeId(sdk, 7));
            // A temporary 60 Hz full-screen report cannot pin modern Surface
            // requests to a physical 60 Hz mode. On 34+, keep the 144 Hz vote.
            equal(sdk >= 34 ? 144 : 0, RefreshRatePolicy.windowRate(sdk, 144, 60));
            equal(sdk >= 34 ? 60 : 0, RefreshRatePolicy.windowRate(sdk, 60, 144));
        }
        equal(7, RefreshRatePolicy.windowModeId(29, 7));
        equal(120, RefreshRatePolicy.windowRate(29, 144, 120));
        System.out.println("Refresh-rate policy: " + checks + " assertions passed (legacy tolerance/resolution, modern mode clearing and user intent)");
    }
}
