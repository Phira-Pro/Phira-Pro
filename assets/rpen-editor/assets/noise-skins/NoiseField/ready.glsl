#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D screenTexture;
uniform sampler2D composeRT;
uniform sampler2D blockMask;
uniform vec2 screenSize;
uniform float time;
uniform float shineBrightness;
uniform float shineSpeed;
const float maskPixelScale = 8.0;

float nfBit(float value, float position) {
    return mod(floor(value / exp2(position)), 2.0);
}
// IMPORTANT: blockMask is FULL RESOLUTION R8, sampled with GL_NEAREST.
// Each pixel encodes SIX Boolean masks that have already passed the agreed
// subtraction threshold before packing, bit order 0..5:
// ANormal, ASubtract, D+R Normal, D+R Subtract, R Normal, R Subtract.
float nfMaskBit(vec2 p, float bit) {
    // Sample at the CENTER of the original source RT pixel (Screen/8 by default).
    // Host allocates those RTs at floor(screenSize / maskPixelScale).
    vec2 msz = max(floor(screenSize / max(maskPixelScale, 1.0)), vec2(1.0));
    vec2 q = (floor(p * msz) + 0.5) / msz;
    float maskWord = floor(texture2D(blockMask, q).r * 255.0 + 0.5);
    return nfBit(maskWord, bit);
}

void main() {
    float dn = nfMaskBit(uv, 4.0);
    float ds = nfMaskBit(uv, 5.0);
    float m = ds - dn;
    float c = texture2D(composeRT, uv).r;
    float a = c * abs(m);
    if (a - 0.0001 < 0.0) { gl_FragColor = texture2D(screenTexture, uv); return; }

    float pulse = sin(time * shineSpeed) * 0.5 + 1.0; // Unity _Time.y
    vec3 rgb = vec3(a) * pulse * (vec3(1.0) * shineBrightness);
    vec4 behind = texture2D(screenTexture, uv);
    gl_FragColor = vec4(behind.rgb + rgb * a, clamp(behind.a + a * a, 0.0, 1.0));
}
