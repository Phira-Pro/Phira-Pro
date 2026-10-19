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
    private float targetFrameRate = 120f;
    private Surface requestedSurface;
    private float appliedFrameRate = Float.NaN;

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
        Surface surface = holder.getSurface();
        QuadNative.surfaceOnSurfaceCreated(surface);
        applyFrameRate(surface, true);
        refreshWindow("surface created");
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        requestedSurface = null;
        appliedFrameRate = Float.NaN;
        cancelTouches("surface-destroyed");
        QuadNative.surfaceOnSurfaceDestroyed(holder.getSurface());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        Surface surface = holder.getSurface();
        QuadNative.surfaceOnSurfaceChanged(surface, width, height);
        applyFrameRate(surface, true);
        refreshWindow("surface resized");
    }

    private void refreshWindow(String reason) {
        if (getContext() instanceof MainActivity) ((MainActivity) getContext()).requestRefreshRateUpdate(reason);
    }

    void setTargetFrameRate(float frameRate, boolean force, boolean allowNonSeamless) {
        if (Float.isNaN(frameRate) || Float.isInfinite(frameRate) || frameRate <= 0f) {
            throw new IllegalArgumentException("frame rate must be finite and positive");
        }
        targetFrameRate = frameRate;
        Surface surface = getHolder().getSurface();
        if (surface.isValid()) applyFrameRate(surface, force, allowNonSeamless);
    }

    private void applyFrameRate(Surface surface, boolean force) {
        applyFrameRate(surface, force, hasWindowFocus());
    }
    private void applyFrameRate(Surface surface, boolean force, boolean allowNonSeamless) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && surface.isValid()) {
            if (!force && requestedSurface == surface && Math.abs(appliedFrameRate - targetFrameRate) < 0.01f) return;
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    surface.setFrameRate(targetFrameRate, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                            allowNonSeamless ? Surface.CHANGE_FRAME_RATE_ALWAYS : Surface.CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS);
                } else {
                    surface.setFrameRate(targetFrameRate, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT);
                }
                requestedSurface = surface;
                appliedFrameRate = targetFrameRate;
            } catch (IllegalStateException | IllegalArgumentException error) {
                // A disappearing Surface during resizing must not crash input
                // or startup; surfaceCreated/resume will reapply the request.
                requestedSurface = null;
                Log.w("PhiraPro", "Surface frame-rate request unavailable", error);
            }
        }
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
