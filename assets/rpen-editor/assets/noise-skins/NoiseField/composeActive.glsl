#version 100
precision highp float;
varying highp vec2 uv;

// Input blockMask is full resolution GL_R8 packed bits 0..5.
uniform sampler2D blockMask;
uniform sampler2D blockNoise;
// @texture tex/BlockNoise1.png wrap=mirror filter=nearest colorspace=linear alphaMode=straight
uniform vec2 screenSize;
uniform float time;
const float maskPixelScale = 8.0;
const float composeDisplaceSpeed = 2.5899999141693115;
const float composeDisplaceStrength = 0.10000000149011612;
const vec2 composeDisplaceDirection = vec2(0.5,0.5);
const vec2 composeDisplaceMapST = vec2(2.13,1.02);

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
    vec2 dir = normalize(composeDisplaceDirection);
    vec2 perp = vec2(-dir.y, dir.x);
    float t = (time / 20.0) * composeDisplaceSpeed;
    vec2 duv = uv * composeDisplaceMapST;
    float d1 = texture2D(blockNoise, duv + dir * t).r - 0.5;
    float d2 = texture2D(blockNoise, duv + perp * t).r - 0.5;
    vec2 disp = dir * d1 + perp * d2;
    vec2 maskUV = uv + disp * composeDisplaceStrength;
    float n = nfMaskBit(maskUV, 0.0);
    float s = nfMaskBit(maskUV, 1.0);
    gl_FragColor = vec4(abs(s - n), 0.0, 0.0, 1.0);
}
