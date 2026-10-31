package org.flos.phirapro;

/** Tracks submissions, never treats an API return as a measured display rate. */
final class RefreshRateRequest {
    private int generation;
    private float rate = Float.NaN;
    private int strategy = -1;
    private String displayState;

    boolean displayChanged(String state) {
        if (state.equals(displayState)) return false;
        displayState = state;
        return true;
    }

    boolean needsSubmission(int generation, float rate, int strategy, boolean force) {
        return force || this.generation != generation || Float.isNaN(this.rate)
                || Math.abs(this.rate - rate) >= 0.01f || this.strategy != strategy;
    }

    void submitted(int generation, float rate, int strategy) {
        this.generation = generation;
        this.rate = rate;
        this.strategy = strategy;
    }

    void invalidate() {
        rate = Float.NaN;
        strategy = -1;
    }

    float submittedRate() { return rate; }
    int submittedGeneration() { return generation; }
    int submittedStrategy() { return strategy; }
}
