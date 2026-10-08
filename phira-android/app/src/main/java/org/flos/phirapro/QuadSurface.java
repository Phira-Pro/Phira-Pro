package org.flos.phirapro;

import android.content.Context;
import android.os.Build;
import android.os.SystemClock;
import android.util.Log;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewParent;

import quad_native.QuadNative;

/** 承载 Rust 渲染输出的 SurfaceView，并把触摸事件转发给 libphira.so。 */
public class QuadSurface extends SurfaceView
        implements View.OnTouchListener, SurfaceHolder.Callback {

    private final TouchEventRouter touchRouter = new TouchEventRouter(new TouchEventRouter.Sink() {
        @Override public void send(int id, int phase, float x, float y, long time) {
            QuadNative.surfaceOnTouch(id, phase, x, y, time);
        }
        @Override public void diagnostic(String message) {
            // Captured by the existing app-only bounded diagnostic recorder.
            // No per-frame positions, device identifiers or user data are logged.
            Log.i("PhiraProTouch", message);
        }
    });

    private final MotionFrame motionFrame = new MotionFrame();

    private static final class MotionFrame implements TouchEventRouter.Frame {
        MotionEvent event;
        public int action() { return event.getActionMasked(); }
        public int actionIndex() { return event.getActionIndex(); }
        public int flags() { return event.getFlags(); }
        public int count() { return event.getPointerCount(); }
        public int id(int index) { return event.getPointerId(index); }
        public float x(int index) { return event.getX(index); }
        public float y(int index) { return event.getY(index); }
        public long time() { return event.getEventTime(); }
    }

    public QuadSurface(Context context) {
        super(context);
        getHolder().addCallback(this);
        setFocusable(true);
        setFocusableInTouchMode(true);
        requestFocus();
        setOnTouchListener(this);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        QuadNative.surfaceOnSurfaceCreated(holder.getSurface());
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        cancelTouches("surface-destroyed");
        QuadNative.surfaceOnSurfaceDestroyed(holder.getSurface());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        Surface surface = holder.getSurface();
        QuadNative.surfaceOnSurfaceChanged(surface, width, height);
    }

    @Override
    public boolean onTouch(View v, MotionEvent event) {
        ViewParent parent = getParent();
        if (parent != null && (event.getActionMasked() == MotionEvent.ACTION_DOWN
                || event.getActionMasked() == MotionEvent.ACTION_POINTER_DOWN)) {
            // Keep ancestor gesture recognizers from stealing a multi-finger stream.
            // This does not override system-wide screenshot/split-screen gestures.
            parent.requestDisallowInterceptTouchEvent(true);
        }

        // inputbox 依赖这次预处理，必须先于 surfaceOnTouch；第三参数表示「外接设备」。
        InputDevice device = event.getDevice();
        boolean isExternal = device != null
                && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
                && device.isExternal();
        boolean isVirtual = device != null && device.isVirtual();
        QuadNative.preprocessInput(event, event.getX(), event.getY(), isExternal, isVirtual);

        motionFrame.event = event;
        try {
            touchRouter.dispatch(motionFrame);
        } finally {
            motionFrame.event = null;
        }
        if (parent != null && (event.getActionMasked() == MotionEvent.ACTION_UP
                || event.getActionMasked() == MotionEvent.ACTION_CANCEL)) {
            parent.requestDisallowInterceptTouchEvent(false);
        }
        return true;
    }

    void cancelTouches(String reason) {
        touchRouter.cancelAll(reason, SystemClock.uptimeMillis());
        ViewParent parent = getParent();
        if (parent != null) parent.requestDisallowInterceptTouchEvent(false);
    }

    @Override
    public void onWindowFocusChanged(boolean hasWindowFocus) {
        super.onWindowFocusChanged(hasWindowFocus);
        if (!hasWindowFocus) cancelTouches("window-focus-lost");
    }

    public Surface getNativeSurface() {
        return getHolder().getSurface();
    }
}
