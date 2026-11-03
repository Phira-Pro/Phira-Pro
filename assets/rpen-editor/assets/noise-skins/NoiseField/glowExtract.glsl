#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D mainTex;


void main() {
    gl_FragColor = vec4(0.0, texture2D(mainTex, uv).g, 0.0, 1.0);
}
