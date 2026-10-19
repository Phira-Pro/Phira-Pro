#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D mainTex;
// These are ordinary, fixed single-pass texture copies. Vertical flipping
// is intentionally disabled for the render-pipeline copy.


void main() {
    vec2 p = uv;

    gl_FragColor = texture2D(mainTex, p);
}
