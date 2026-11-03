#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D mainTex;
uniform sampler2D composeRT;
uniform vec2 targetTexelSize;


void main() {
    vec2 t = targetTexelSize;
    float m = texture2D(mainTex, uv).r;
    m = max(m, texture2D(mainTex, uv + vec2( t.x, 0.0)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x, 0.0)).r);
    m = max(m, texture2D(mainTex, uv + vec2(0.0,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(0.0, -t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2( t.x,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x,  t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2( t.x, -t.y)).r);
    m = max(m, texture2D(mainTex, uv + vec2(-t.x, -t.y)).r);
    float edge = clamp(m - texture2D(composeRT, uv).r, 0.0, 1.0);
    gl_FragColor = vec4(edge, 0.0, 0.0, 1.0);
}
