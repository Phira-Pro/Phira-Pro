#version 100
attribute vec3 position;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
uniform vec3 uView;
varying lowp vec4 color;
varying highp vec2 fieldUV;
varying highp vec2 sceneUV;
varying highp vec2 displaceUV;
varying highp vec2 sparkUV;
varying highp vec2 disabledDisplaceUV;
varying highp vec2 touchDisplaceUV;
varying highp vec2 noiseUV;
varying highp vec4 screenPos;
varying highp float clipHalfWidth;

void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    color = color0 / 255.0;
    fieldUV = position.xy * vec2(0.5, uView.z * 0.5) + 0.5;
    sceneUV = gl_Position.xy / gl_Position.w * 0.5 + 0.5;
    screenPos = vec4(sceneUV, 0.0, 1.0);
    // Retain native ST arithmetic in the vertex stage. Point-sampled textures
    // can select different texels if these multiplies move to the fragment.
    displaceUV = sceneUV * vec2(0.8, 0.3);
    sparkUV = sceneUV * vec2(3.0, 1.2);
    disabledDisplaceUV = sceneUV * vec2(0.5, 0.2);
    touchDisplaceUV = sceneUV * vec2(0.55, 0.3);
    noiseUV = sceneUV * vec2(1.5, 1.46);
    clipHalfWidth = uView.y * 0.888888896 / uView.x;
}
