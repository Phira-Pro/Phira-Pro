#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform float Life;
uniform float Flow;
uniform vec4 Tint;
uniform float Opacity;
uniform sampler2D NoiseA;
uniform sampler2D NoiseB;
varying mediump vec2 uv;

float box_distance(vec2 p, vec2 half_size) {
    vec2 d = abs(p) - half_size;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

vec3 screen_blend(vec3 a, vec3 b) {
    return 1.0 - (1.0 - a) * (1.0 - b);
}

void main() {
    // The quad covers the upper tenth of the reference frame. Work in
    // thousandths of the reference canvas so mediump GPUs can square p.
    vec2 p = vec2((uv.x - 0.5) * 1.92, uv.y * 0.108 - 0.288);
    float focal = 0.54 / (tan(radians(98.2 * 0.5)) * 1.6);
    float radius = max(length(p), 0.0001);
    p *= focal * atan(radius / focal) / radius;
    float sd = box_distance(p + vec2(0.0, 0.188), vec2(Life * 0.310, 0.200));
    if (sd > 0.003) discard;

    vec2 noise_uv = vec2(p.x * (25.527427 * 7.42 / 64.0), (p.y + 0.288) / 38.4);
    noise_uv += vec2(0.05, 0.02) * Flow;
    float noise = mix(texture2D(NoiseA, noise_uv).r, texture2D(NoiseB, noise_uv).r, 0.5);
    float pattern = max(0.0, (noise - 0.591) * 15.6);
    vec3 base = Tint.rgb * 0.7411765;
    vec3 body = mix(2.0 * base * pattern, 1.0 - 2.0 * (1.0 - base) * (1.0 - pattern), step(vec3(0.5), base));
    body = sd < 0.0 ? clamp(body, 0.0, 1.0) : vec3(0.0);

    float distance_to_rim = max(abs(sd) - 0.0015, 0.0);
    vec3 glow = Tint.rgb * (0.34 * dot(vec3(0.56, 0.32, 0.12), exp2(vec3(-8.0, -2.4, -0.75) * (distance_to_rim / 0.060))));
    vec3 rim = mix(Tint.rgb, vec3(1.0), 0.4023534);
    vec3 color = mix(screen_blend(screen_blend(body, glow), glow), rim, 1.0 - smoothstep(0.0, 0.001, distance_to_rim));
    float mask = box_distance(p + vec2(-0.070, 0.303), vec2(1.206, 0.123));
    color *= 1.0 - smoothstep(-0.008, 0.123, mask);
    if (max(color.r, max(color.g, color.b)) < 0.06) discard;
    gl_FragColor = vec4(color, (1.0 - uv.y) * Opacity);
}
