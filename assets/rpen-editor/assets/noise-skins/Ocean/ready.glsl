#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D screenTexture;
uniform sampler2D blockMask;
uniform sampler2D composeRT;
uniform vec2 screenSize;
uniform vec4 outlineColor;
uniform float time;
uniform float readyPulse;
uniform float readySpeed;

// Keep the Ready mask separate from Disabled even though the two share a
// combined Disabled+Ready compose target. Same packed-mask semantics as
// NoiseField: bit 4 = Ready normal, bit 5 = Ready subtract.
const float maskPixelScale = 8.0;
float bitAt(float w, float bit) {
    return mod(floor(w / exp2(bit)), 2.0);
}
void main() {
    vec4 bg = texture2D(screenTexture, uv);
    vec2 s = max(floor(screenSize / maskPixelScale), vec2(1.0));
    vec2 q = (floor(uv * s) + 0.5) / s;
    float w = floor(texture2D(blockMask, q).r * 255.0 + 0.5);
    float readyOnly = abs(bitAt(w, 4.0) - bitAt(w, 5.0));
    float rd = readyOnly * texture2D(composeRT, uv).r;
    if (rd < 0.0001) {
        gl_FragColor = bg;
        return;
    }

    // NoiseField-style fast *white* flash. A Ready state lasts approximately
    // 0.5 seconds, so a 37.9 rad/s oscillation yields about three bright/dim
    // cycles rather than the almost invisible 3.1 rad/s breathing effect.
    // Equivalent to ReadyBlock additive pulse, computed in the shader because
    // blockArea's v1 skin protocol requires final screen replacement.
    float flash = 1.0 + 0.5 * sin(time * readySpeed);
    float strength = readyPulse * flash * rd * rd * outlineColor.a;
    gl_FragColor = vec4(clamp(bg.rgb + vec3(strength), 0.0, 1.0), bg.a);
}
