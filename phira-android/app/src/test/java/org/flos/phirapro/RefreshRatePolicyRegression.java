package org.flos.phirapro;

public final class RefreshRatePolicyRegression {
    private static int checks;
    private static void equal(float expected, float actual) {
        checks++;
        if (Float.isNaN(actual) || Math.abs(expected - actual) > 0.001f) throw new AssertionError(expected + " != " + actual);
    }
    private static void check(boolean value, String reason) {
        checks++;
        if (!value) throw new AssertionError(reason);
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
        // Sparse panels must not turn a high-refresh request into a 60 Hz vote.
        int[] dimensions = {0, 0, 0, 0};
        equal(1, RefreshRatePolicy.legacyMode(120, 0, 0, dimensions, dimensions, new float[] {60, 144, Float.NaN, -1}));
        equal(1, RefreshRatePolicy.legacyMode(120, 0, 0, dimensions, dimensions, new float[] {144, 119.88f, 165, 60}));
        equal(2, RefreshRatePolicy.legacyMode(240, 0, 0, dimensions, dimensions, new float[] {60, 144, 165, Float.POSITIVE_INFINITY}));
        equal(0, RefreshRatePolicy.legacyMode(120, 0, 0, dimensions, dimensions, new float[] {60, Float.NaN, 0, -1}));
        equal(-1, RefreshRatePolicy.legacyMode(120, 0, 0, new int[0], new int[0], new float[0]));
        equal(-1, RefreshRatePolicy.legacyMode(120, 0, 0, dimensions, dimensions, new float[] {Float.NaN, Float.POSITIVE_INFINITY, 0, -1}));
        equal(144, RefreshRatePolicy.requestedRate(RefreshRatePolicy.UNLIMITED_FPS, 144));
        equal(60, RefreshRatePolicy.requestedRate(RefreshRatePolicy.UNLIMITED_FPS, 60));
        equal(360, RefreshRatePolicy.requestedRate(RefreshRatePolicy.UNLIMITED_FPS, 360));
        equal(120, RefreshRatePolicy.requestedRate(RefreshRatePolicy.UNLIMITED_FPS, Float.NaN));
        equal(120, RefreshRatePolicy.requestedRate(120, 144));
        equal(2, RefreshRatePolicy.legacyMode(RefreshRatePolicy.UNLIMITED_FPS, 2880, 1800, widths, heights, rates));
        for (int sdk : new int[] {30, 31, 33, 34, 35, 36}) {
            equal(0, RefreshRatePolicy.windowModeId(sdk, 7));
            // A temporary 60 Hz full-screen report cannot pin modern Surface
            // requests to a physical 60 Hz mode. On 34+, keep the 144 Hz vote.
            equal(sdk >= 34 ? 144 : 0, RefreshRatePolicy.windowRate(sdk, 144, 60));
            equal(sdk >= 34 ? 60 : 0, RefreshRatePolicy.windowRate(sdk, 60, 144));
        }
        equal(7, RefreshRatePolicy.windowModeId(29, 7));
        equal(120, RefreshRatePolicy.windowRate(29, 144, 120));

        RefreshRateRequest request = new RefreshRateRequest();
        check(request.needsSubmission(1, 120, 0, false), "first surface needs a vote");
        request.submitted(1, 120, 0);
        check(!request.needsSubmission(1, 120, 0, false), "unchanged surface must not repeatedly vote");
        check(request.needsSubmission(2, 120, 0, false), "same Java surface in a new lifecycle epoch needs a vote");
        check(request.needsSubmission(1, 120, 1, false), "a non-seamless transition must not be deduplicated");
        check(request.needsSubmission(1, 144, 0, false), "user target changes need a vote");
        check(request.needsSubmission(1, 120, 0, true), "lifecycle recovery must bypass the submission cache");
        request.invalidate();
        check(Float.isNaN(request.submittedRate()), "failed or destroyed surface has no submitted rate");
        check(request.needsSubmission(1, 120, 0, false), "failed request remains retryable");
        request.submitted(2, 144, 1);
        equal(144, request.submittedRate());
        equal(2, request.submittedGeneration());
        equal(1, request.submittedStrategy());
        check(request.displayChanged("display:120"), "initial display needs submission");
        for (int i = 0; i < 100; i++) {
            check(!request.displayChanged("display:120"), "notifications from the same state must not create a vote loop");
        }
        check(request.displayChanged("display:60"), "a later policy downgrade needs a fresh vote");
        check(!request.displayChanged("display:60"), "downgrade recovery must not rearm itself");
        check(request.displayChanged("other-display:60"), "moving to another display needs a fresh vote");
        System.out.println("Refresh-rate policy and request lifecycle: " + checks + " assertions passed");
    }
}
