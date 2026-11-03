#version 100
precision highp float;
varying highp vec2 uv;
uniform vec2 screenSize;
uniform int touchCount;
uniform vec4 touchPoints[32];
uniform float touchRadius;
uniform float touchReleaseDuration;

// Convert touch stage seconds to the hover scale used by this effect.
// The original contact footprint and brightness both use this same value.
float touchHoverFromStage(vec4 touchPoint) {
    if (touchPoint.w > 0.5)
        return clamp(touchPoint.z / 0.10, 0.0, 1.0);
    if (touchReleaseDuration <= 0.0) return 0.0;
    return clamp(1.0 - touchPoint.z / touchReleaseDuration, 0.0, 1.0);
}

void main() {
    // Unity's size=11.5 is a localScale, NOT the world-space sprite width.
    // The prefab mesh bounds are unavailable in the extracted files, so the
    // hover footprint is a resolution-independent *approximation*, not a
    // recovered prefab dimension. The v1 Ocean skin uses the same calibrated 0.25 screen-height
    // footprint as the latest NoiseField touch. This is an approximation.
    float aspect=screenSize.x/max(screenSize.y,1.0);
    float v=0.0;
    for (int i=0;i<32;++i) {
        if(i>=touchCount) break;
        vec4 tp=touchPoints[i];
        tp.z=touchHoverFromStage(tp);
        if(tp.z<=0.0001) continue;
        float d=length((uv-tp.xy)*vec2(aspect,1.0));
        float r=max(touchRadius*tp.z,0.0001);
        // The previous implementation used a large, fully saturated
        // disk (0..0.78*r), which hid most of the extracted SDF structure.
        // Use a continuous radial envelope with no flat central plateau.
        // This envelope approximates the unknown prefab mask alpha, while
        // the original SDF/FD_Noise/Overlay math stays in main.glsl.
        float fade=1.0-smoothstep(0.0,r,d);
        v=max(v,fade*tp.z);
    }
    // Independent touch-camera mask. A contact that began in Active can
    // extend beyond the geometry and persist after the area phase ends.
    gl_FragColor=vec4(v,0.0,0.0,1.0);
}
