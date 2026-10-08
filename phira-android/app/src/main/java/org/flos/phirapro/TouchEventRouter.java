package org.flos.phirapro;

import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.Map;

/** Tracks Android pointer IDs across a gesture, independently of pointer indices. */
final class TouchEventRouter {
    // Android MotionEvent actions / flags and the existing libphira JNI phases.
    static final int DOWN = 0, UP = 1, MOVE = 2, CANCEL = 3, POINTER_DOWN = 5, POINTER_UP = 6;
    static final int FLAG_CANCELED = 0x20;
    static final int MOVED = 0, ENDED = 1, STARTED = 2, CANCELLED = 3;

    interface Frame {
        int action();
        int actionIndex();
        int flags();
        int count();
        int id(int index);
        float x(int index);
        float y(int index);
        long time();
    }

    interface Sink {
        void send(int id, int phase, float x, float y, long time);
        void diagnostic(String message);
    }

    private static final class Pointer {
        float x, y;
        Pointer(float x, float y) { this.x = x; this.y = y; }
    }

    private final Sink sink;
    private final Map<Integer, Pointer> active = new LinkedHashMap<>();
    private boolean open;
    private int peak;

    TouchEventRouter(Sink sink) { this.sink = sink; }

    int activeCount() { return active.size(); }

    void dispatch(Frame event) {
        int action = event.action();
        if (action != DOWN && action != UP && action != MOVE && action != CANCEL
                && action != POINTER_DOWN && action != POINTER_UP) return;
        if (action == DOWN) {
            cancelAll("new-down-before-release", event.time());
            open = true;
            peak = 0;
        }
        // CANCEL closes the stream. Do not fabricate presses from later MOVE events.
        if (!open) return;
        if (action == CANCEL) {
            updatePositions(event, false, -1);
            cancelAll("android-action-cancel flags=" + event.flags()
                    + " eventPointers=" + event.count(), event.time());
            return;
        }
        if (event.count() == 0 || event.actionIndex() < 0 || event.actionIndex() >= event.count()) {
            cancelAll("invalid-pointer-packet", event.time());
            return;
        }

        // A normal Android packet contains every pointer, including the one going up.
        // If a malformed stream omits one, release that ID instead of keeping a ghost hold.
        Iterator<Map.Entry<Integer, Pointer>> missing = active.entrySet().iterator();
        while (missing.hasNext()) {
            Map.Entry<Integer, Pointer> item = missing.next();
            boolean found = false;
            for (int i = 0; i < event.count(); i++) {
                if (event.id(i) == item.getKey()) { found = true; break; }
            }
            if (!found) {
                Pointer pointer = item.getValue();
                sink.diagnostic("missing-pointer id=" + item.getKey() + " action=" + action);
                sink.send(item.getKey(), CANCELLED, pointer.x, pointer.y, event.time());
                missing.remove();
            }
        }

        int index = event.actionIndex();
        int changed = event.id(index);
        updatePositions(event, true, action == MOVE ? -1 : changed);
        if (action == DOWN || action == POINTER_DOWN) {
            Pointer previous = active.put(changed, new Pointer(event.x(index), event.y(index)));
            if (previous != null) sink.send(changed, CANCELLED, previous.x, previous.y, event.time());
            sink.send(changed, STARTED, event.x(index), event.y(index), event.time());
            peak = Math.max(peak, active.size());
            if (active.size() >= 3) sink.diagnostic("pointer-down active=" + active.size()
                    + " ids=" + active.keySet() + " eventTime=" + event.time());
        } else if (action == UP || action == POINTER_UP) {
            if (active.remove(changed) != null) {
                int phase = (event.flags() & FLAG_CANCELED) != 0 ? CANCELLED : ENDED;
                sink.send(changed, phase, event.x(index), event.y(index), event.time());
                if (phase == CANCELLED) sink.diagnostic("android-pointer-cancel id=" + changed
                        + " remaining=" + active.size() + " peak=" + peak);
            }
            if (action == UP) cancelAll("final-up-with-stale-pointers", event.time());
        }
    }

    private void updatePositions(Frame event, boolean emitMoves, int skipId) {
        for (int i = 0; i < event.count(); i++) {
            int id = event.id(i);
            Pointer pointer = active.get(id);
            if (pointer == null || id == skipId) continue;
            float x = event.x(i), y = event.y(i);
            // Pointer transitions also carry the other fingers' current positions.
            // Preserve real MOVE samples, including stationary ones, for flick timing.
            // Do not add duplicate samples on an unrelated pointer transition.
            if (emitMoves && (event.action() == MOVE || pointer.x != x || pointer.y != y)) {
                sink.send(id, MOVED, x, y, event.time());
            }
            pointer.x = x;
            pointer.y = y;
        }
    }

    void cancelAll(String reason, long time) {
        if (!active.isEmpty()) {
            sink.diagnostic("touch-stream-cancel reason=" + reason + " active=" + active.size()
                    + " peak=" + peak + " ids=" + active.keySet() + " eventTime=" + time);
            for (Map.Entry<Integer, Pointer> item : active.entrySet()) {
                Pointer pointer = item.getValue();
                sink.send(item.getKey(), CANCELLED, pointer.x, pointer.y, time);
            }
            active.clear();
        }
        open = false;
    }
}
