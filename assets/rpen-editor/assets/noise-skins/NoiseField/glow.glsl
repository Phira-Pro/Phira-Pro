#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D mainTex;
uniform sampler2D composeRT;
uniform vec2 targetTexelSize;
uniform float passWeight;
uniform float firstPass;


void main() {
    vec2 t = targetTexelSize;
    vec2 cur = texture2D(mainTex, uv).rg;
    float m = cur.r;
    m = max(m, texture2D(mainTex, uv + vec2( t.x, 0.0)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x, 0.0)).r);
    m = max(m, texture2D(mainTex, uv + vec2(0.0,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(0.0, -t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2( t.x,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2( t.x, -t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x, -t.y)).r);
    float ring = clamp(m - cur.r, 0.0, 1.0);
    ring *= 1.0 - texture2D(composeRT, uv).r;
    float weighted = ring * passWeight;
    float accum = (firstPass > 0.5) ? weighted : (cur.g + weighted);
    gl_FragColor = vec4(m, clamp(accum, 0.0, 1.0), 0.0, 1.0);
}
