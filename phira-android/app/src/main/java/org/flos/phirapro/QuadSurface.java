package org.flos.phirapro;

import android.content.Context;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;

import quad_native.QuadNative;

/**
 * 承载 Rust 渲染输出的 SurfaceView，并把触摸 / 键盘事件转发给 libphira.so。
 *
 * <p>结构来自 miniquad 的公开 Android 模板；触摸回调为 5 参数版本
 * （多一个事件时间，供 Rust 侧计算输入时间戳）。
 */
public class QuadSurface extends SurfaceView
        implements View.OnTouchListener, View.OnKeyListener, SurfaceHolder.Callback {

    // 与 libphira.so 约定的触摸阶段
    private static final int PHASE_MOVED = 0;
    private static final int PHASE_ENDED = 1;
    private static final int PHASE_STARTED = 2;
    private static final int PHASE_CANCELLED = 3;

    public QuadSurface(Context context) {
        super(context);
        getHolder().addCallback(this);
        setFocusable(true);
        setFocusableInTouchMode(true);
        requestFocus();
        setOnTouchListener(this);
        setOnKeyListener(this);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        QuadNative.surfaceOnSurfaceCreated(holder.getSurface());
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        QuadNative.surfaceOnSurfaceDestroyed(holder.getSurface());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        Surface surface = holder.getSurface();
        QuadNative.surfaceOnSurfaceChanged(surface, width, height);
    }

    @Override
    public boolean onTouch(View v, MotionEvent event) {
        final int pointerCount = event.getPointerCount();
        final long time = event.getEventTime();

        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_MOVE:
                for (int i = 0; i < pointerCount; i++) {
                    QuadNative.surfaceOnTouch(event.getPointerId(i), PHASE_MOVED, event.getX(i), event.getY(i), time);
                }
                break;
            case MotionEvent.ACTION_UP:
                QuadNative.surfaceOnTouch(event.getPointerId(0), PHASE_ENDED, event.getX(0), event.getY(0), time);
                break;
            case MotionEvent.ACTION_DOWN:
                QuadNative.surfaceOnTouch(event.getPointerId(0), PHASE_STARTED, event.getX(0), event.getY(0), time);
                break;
            case MotionEvent.ACTION_POINTER_UP: {
                final int index = event.getActionIndex();
                QuadNative.surfaceOnTouch(event.getPointerId(index), PHASE_ENDED, event.getX(index), event.getY(index), time);
                break;
            }
            case MotionEvent.ACTION_POINTER_DOWN: {
                final int index = event.getActionIndex();
                QuadNative.surfaceOnTouch(event.getPointerId(index), PHASE_STARTED, event.getX(index), event.getY(index), time);
                break;
            }
            case MotionEvent.ACTION_CANCEL:
                for (int i = 0; i < pointerCount; i++) {
                    QuadNative.surfaceOnTouch(event.getPointerId(i), PHASE_CANCELLED, event.getX(i), event.getY(i), time);
                }
                break;
            default:
                break;
        }
        return true;
    }

    // getCharacters 已废弃，但非拉丁输入时只有它有有效数据。
    @SuppressWarnings("deprecation")
    @Override
    public boolean onKey(View v, int keyCode, KeyEvent event) {
        if (event.getAction() == KeyEvent.ACTION_DOWN && keyCode != 0) {
            QuadNative.surfaceOnKeyDown(keyCode);
        }
        if (event.getAction() == KeyEvent.ACTION_UP && keyCode != 0) {
            QuadNative.surfaceOnKeyUp(keyCode);
        }
        if (event.getAction() == KeyEvent.ACTION_UP || event.getAction() == KeyEvent.ACTION_MULTIPLE) {
            int character = event.getUnicodeChar();
            if (character == 0) {
                String characters = event.getCharacters();
                if (characters != null && characters.length() > 0) {
                    character = characters.charAt(0);
                }
            }
            if (character != 0) {
                QuadNative.surfaceOnCharacter(character);
            }
        }
        return true;
    }

    public Surface getNativeSurface() {
        return getHolder().getSurface();
    }
}
