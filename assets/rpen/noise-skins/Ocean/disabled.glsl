#version 100
precision highp float;
varying highp vec2 uv;

uniform sampler2D screenTexture;
uniform sampler2D composeRT;
uniform sampler2D waterFlow;
// @texture tex/Flow.png wrap=repeat filter=linear colorspace=linear alphaMode=straight
uniform sampler2D waterCaustics;
// @texture tex/Caustics.png wrap=repeat filter=linear colorspace=linear alphaMode=straight
uniform vec2 screenSize;
uniform vec4 outlineColor;
uniform float time;
uniform float disabledOpacity;
uniform float causticScale;
uniform float causticSpeed;
uniform float causticIntensity;

void main() {
    vec4 bg = texture2D(screenTexture, uv);
    float coverage = texture2D(composeRT, uv).r;
    if (coverage < 0.00001) {
        gl_FragColor = bg;
        return;
    }

    // Use screen-height-scaled coordinates, as in main.glsl. This makes
    // the water-cell size independent of the viewport aspect ratio.
    vec2 p = uv * screenSize / max(screenSize.y, 1.0);
    float t = time * causticSpeed;
    vec2 f1 = texture2D(waterFlow,
                        p * 0.78 + vec2(0.055, -0.042) * t * 0.55).rg * 2.0 - 1.0;
    vec2 f2 = texture2D(waterFlow,
                        p * 1.81 + vec2(-0.084, 0.064) * t * 0.55).rg * 2.0 - 1.0;
    vec2 f = f1 * 0.65 + f2 * 0.35;

    // Keep the scene transparent, but make Disabled look like actual water
    // instead of a nearly uniform dark-blue overlay.
    vec2 refrOffset = f * (2.7 / 1080.0)
                      * vec2(screenSize.y / max(screenSize.x, 1.0), 1.0);
    vec3 refracted = texture2D(screenTexture,
                              clamp(uv + refrOffset, vec2(0.0), vec2(1.0))).rgb;
    vec3 water = mix(refracted, outlineColor.rgb, disabledOpacity * 0.29);
    float amount = clamp(coverage * outlineColor.a, 0.0, 1.0);
    vec3 result = mix(bg.rgb, water, amount);

    // Bring the drifting, independently phased caustics into Disabled.
    // Uses the SAME two periodic texture fields as Active but advected at a
    // calmer speed. No dark base color or sudden opaque foam overlay.
    vec2 aUV = p * (causticScale * 0.85) + f * 0.14
               + vec2(0.085, -0.058) * t * 0.62;
    vec2 bUV = p * (causticScale * 1.27) - f * 0.11
               + vec2(-0.071, 0.102) * t * 0.62;
    float a = texture2D(waterCaustics, aUV).r;
    float b = texture2D(waterCaustics, bUV).r;
    float shimmer = pow(clamp(a * 0.82 + b * 0.49, 0.0, 1.0), 1.07);
    vec3 glintColor = mix(outlineColor.rgb, vec3(1.0), 0.83);
    // Deliberately stronger than v4's 0.045*c. Caustics show up in ordinary
    // Disabled as moving luminous lines, not only in Active/Ready.
    result += glintColor * shimmer * causticIntensity * 0.31
              * amount;

    gl_FragColor = vec4(clamp(result, 0.0, 1.0), bg.a);
}
