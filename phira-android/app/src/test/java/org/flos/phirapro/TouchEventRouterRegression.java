package org.flos.phirapro;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** Executable input-stream regressions; no Android VM or third-party test dependency. */
public final class TouchEventRouterRegression {
    private static int assertions;
    private static final int[] IDS = {0, 7, 2, 21, 10, 30, 6, 12, 1, 17};

    static void check(boolean result, String message) {
        assertions++;
        if (!result) throw new AssertionError(message);
    }

    static final class Packet implements TouchEventRouter.Frame {
        final int rawAction, flags;
        final int[] ids;
        final long time;
        final float delta;
        Packet(int action, int index, int flags, long time, float delta, int... ids) {
            rawAction = action | (index << 8);
            this.flags = flags;
            this.time = time;
            this.delta = delta;
            this.ids = ids;
        }
        public int action() { return rawAction & 255; }
        public int actionIndex() { return (rawAction >> 8) & 255; }
        public int flags() { return flags; }
        public int count() { return ids.length; }
        public int id(int index) { return ids[index]; }
        public float x(int index) { return ids[index] * 20f + delta; }
        public float y(int index) { return ids[index] * 15f + delta; }
        public long time() { return time; }
    }

    static final class NativeState implements TouchEventRouter.Sink {
        final Map<Integer, float[]> held = new HashMap<>();
        final List<String> diagnostics = new ArrayList<>();
        int starts, moves, ends, cancels;
        long expectedTime;
        public void send(int id, int phase, float x, float y, long time) {
            check(time == expectedTime, "JNI timestamp must preserve Android uptime milliseconds");
            if (phase == TouchEventRouter.STARTED) {
                check(!held.containsKey(id), "a finger must not restart when another finger is pressed");
                held.put(id, new float[] {x, y});
                starts++;
            } else if (phase == TouchEventRouter.MOVED) {
                check(held.containsKey(id), "move must refer to the same live pointer ID");
                held.put(id, new float[] {x, y});
                moves++;
            } else {
                check(held.remove(id) != null, "only a live pointer may end or cancel");
                if (phase == TouchEventRouter.ENDED) ends++;
                else if (phase == TouchEventRouter.CANCELLED) cancels++;
                else throw new AssertionError("unknown native phase");
            }
        }
        public void diagnostic(String message) { diagnostics.add(message); }
    }

    static final class Fixture {
        final NativeState nativeState = new NativeState();
        final TouchEventRouter router = new TouchEventRouter(nativeState);
        long clock = 1000;
        void send(int action, int index, int flags, float delta, int... ids) {
            nativeState.expectedTime = ++clock;
            router.dispatch(new Packet(action, index, flags, clock, delta, ids));
            check(router.activeCount() == nativeState.held.size(), "Java and native live IDs must agree");
        }
        void start(int count) {
            for (int n = 1; n <= count; n++) {
                int[] reordered = new int[n];
                // Put the new finger at index zero, even when its ID is not zero.
                reordered[0] = IDS[n - 1];
                for (int i = 1; i < n; i++) reordered[i] = IDS[i - 1];
                send(n == 1 ? TouchEventRouter.DOWN : TouchEventRouter.POINTER_DOWN,
                        0, 0, n, reordered);
                check(nativeState.held.size() == n, "pressing finger " + n + " must retain previous fingers");
                check(nativeState.starts == n, "one start per physical press");
                check(nativeState.ends == 0 && nativeState.cancels == 0, "no false release on pointer-down");
            }
        }
    }

    static void multiFingerSequences() {
        for (int fingers = 3; fingers <= 10; fingers++) {
            Fixture f = new Fixture();
            f.start(fingers);
            int[] remaining = Arrays.copyOf(IDS, fingers);
            // Reverse indices during movement; IDs and positions must remain associated.
            for (int i = 0; i < remaining.length / 2; i++) {
                int value = remaining[i];
                remaining[i] = remaining[remaining.length - 1 - i];
                remaining[remaining.length - 1 - i] = value;
            }
            f.send(TouchEventRouter.MOVE, 0, 0, 100, remaining);
            for (int id : remaining) {
                check(f.nativeState.held.get(id)[0] == id * 20f + 100, "pointer index changes must not swap fingers");
            }
            while (remaining.length > 0) {
                int index = remaining.length / 2;
                int removedId = remaining[index];
                f.send(remaining.length == 1 ? TouchEventRouter.UP : TouchEventRouter.POINTER_UP,
                        index, 0, 200, remaining);
                check(!f.nativeState.held.containsKey(removedId), "release must affect actionIndex's ID");
                check(f.nativeState.held.size() == remaining.length - 1, "other holds must remain pressed");
                int[] next = new int[remaining.length - 1];
                for (int i = 0, j = 0; i < remaining.length; i++) if (i != index) next[j++] = remaining[i];
                remaining = next;
            }
            check(f.nativeState.ends == fingers && f.nativeState.cancels == 0, "normal stream ends each finger once");
        }
    }

    static void cancelAndResume() {
        Fixture f = new Fixture();
        f.start(3);
        // OEM/ancestor cancellation can contain only a subset of previously active IDs.
        f.send(TouchEventRouter.CANCEL, 0, TouchEventRouter.FLAG_CANCELED, 0, IDS[1]);
        check(f.nativeState.held.isEmpty() && f.nativeState.cancels == 3, "global CANCEL must release ALL live fingers");
        f.send(TouchEventRouter.MOVE, 0, 0, 10, IDS);
        check(f.nativeState.starts == 3 && f.nativeState.held.isEmpty(), "cancelled stream must not invent new presses");
        check(f.nativeState.diagnostics.stream().anyMatch(value -> value.contains("android-action-cancel")
                && value.contains("peak=3")), "three-finger cancellation must be diagnosable");
        f.send(TouchEventRouter.DOWN, 0, 0, 20, IDS[1]);
        check(f.nativeState.held.size() == 1 && f.nativeState.starts == 4, "new DOWN resumes a fresh gesture");
    }

    static void individualCancellation() {
        Fixture f = new Fixture();
        f.start(4);
        f.send(TouchEventRouter.POINTER_UP, 2, TouchEventRouter.FLAG_CANCELED, 30, Arrays.copyOf(IDS, 4));
        check(f.nativeState.cancels == 1 && f.nativeState.ends == 0, "FLAG_CANCELED is not an intentional release");
        check(f.nativeState.held.size() == 3 && !f.nativeState.held.containsKey(IDS[2]), "individual cancel retains the other fingers");
    }

    static void interruptedLifecycle() {
        for (String reason : Arrays.asList("activity-paused", "surface-destroyed", "window-focus-lost")) {
            Fixture f = new Fixture();
            f.start(10);
            f.nativeState.expectedTime = ++f.clock;
            f.router.cancelAll(reason, f.clock);
            check(f.nativeState.held.isEmpty() && f.nativeState.cancels == 10, "lifecycle loss must not leave held fingers");
            f.router.cancelAll(reason, f.clock);
            check(f.nativeState.cancels == 10, "duplicate lifecycle callbacks must not duplicate cancels");
        }
    }

    static void abnormalStreams() {
        Fixture f = new Fixture();
        f.start(3);
        f.send(TouchEventRouter.MOVE, 0, 0, 10, IDS[0], IDS[2]);
        check(f.nativeState.cancels == 1 && !f.nativeState.held.containsKey(IDS[1]), "missing pointer must not become a ghost hold");
        f.send(TouchEventRouter.DOWN, 0, 0, 30, IDS[1]);
        check(f.nativeState.held.size() == 1 && f.nativeState.cancels == 3, "new gesture clears old IDs before reusing an ID");
        f.send(TouchEventRouter.CANCEL, 0, 0, 0);
        check(f.nativeState.held.isEmpty(), "empty cancellation still releases tracked IDs");
    }

    static void longMovement() {
        Fixture f = new Fixture();
        f.start(10);
        int diagnosticCount = f.nativeState.diagnostics.size();
        int previousMoves = f.nativeState.moves;
        for (int i = 0; i < 10000; i++) f.send(TouchEventRouter.MOVE, 0, 0, 100, IDS);
        check(f.nativeState.moves - previousMoves == 100000, "real stationary MOVE samples must remain available for flick timing");
        check(f.nativeState.held.size() == 10 && f.nativeState.starts == 10, "sustained movement must not reset held touches");
        check(f.nativeState.diagnostics.size() == diagnosticCount, "normal MOVE must not write per-frame logs");
    }

    public static void main(String[] args) {
        multiFingerSequences();
        cancelAndResume();
        individualCancellation();
        interruptedLifecycle();
        abnormalStreams();
        longMovement();
        System.out.println("Android multitouch: 6 regression groups passed; " + assertions + " assertions");
    }
}
