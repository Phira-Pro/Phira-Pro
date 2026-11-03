#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D blockMask;
uniform vec2 screenSize;
const float maskPixelScale = 8.0;
float nfBit(float value, float position) { return mod(floor(value / exp2(position)), 2.0); }
float nfMaskWord(vec2 p) {
    vec2 msz = max(floor(screenSize / max(maskPixelScale, 1.0)), vec2(1.0));
    vec2 q = (floor(p * msz) + 0.5) / msz;
    return floor(texture2D(blockMask, q).r * 255.0 + 0.5);
}
void main() {
    float w = nfMaskWord(uv);
    float dn = nfBit(w, 2.0);
    float ds = nfBit(w, 3.0);
    gl_FragColor = vec4(abs(ds - dn), ds, 0.0, 1.0);
}
