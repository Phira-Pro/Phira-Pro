#version 100
precision highp float;
varying highp vec2 uv;

// -----------------------------------------------------------------------------
// User-editable skin parameters: this file is the ONLY source of defaults.
// Official plain edge color #FF5454 approx (1.0, 0.33018857, 0.33018857).
// For color-per-block charts the renderer uses an explicit block color when
// present; otherwise this user-editable default is used.
// -----------------------------------------------------------------------------
uniform vec4 outlineColor;                   // %1.0,0.33018857,0.33018857,1.0% 0.0..1.0
uniform float fillOpacity;                  // %0.667% 0.0..1.0
uniform float fillStrength;                 // %0.667% 0.0..1.0
uniform float edgeOpacity;                  // %0.8% 0.0..2.0
uniform float glowIntensity;                // %0.8% 0.0..2.0
uniform float backgroundPixelScale;         // %6.0% 1.0..32.0
uniform float activeDisplaceSpeed;          // %1.5% 0.0..10.0
uniform float activeDisplaceStrength;       // %0.15% 0.0..1.0
uniform float activeSparkMapOpacity;        // %5.69% 0.0..12.0
uniform float disabledFillOpacity;          // %0.4% 0.0..1.0
uniform float shineBrightness;              // %0.12% 0.0..2.0
uniform float shineSpeed;                   // %37.9% 0.0..100.0
uniform float touchHoverStrength;           // %1.0% 0.0..2.0
uniform float touchPosBrightness;           // %2.0% 0.0..4.0
uniform float touchReleaseDuration; // %0.1% 0.0..1.0
uniform float touchRadius;                  // %0.25% 0.02..0.30

// Engine-provided inputs and standard intermediate textures.
uniform sampler2D screenTexture;
uniform sampler2D blockMask;
uniform sampler2D composeRT;
uniform sampler2D effectRT;
uniform sampler2D readyComposeRT;
uniform sampler2D sceneColorRT;
uniform sampler2D touchHoverRT;
uniform sampler2D blockNoise;
// @texture tex/BlockNoise1.png wrap=mirror filter=nearest colorspace=linear alphaMode=straight
uniform sampler2D pointNoise;
// @texture tex/PointNoise.png wrap=repeat filter=nearest colorspace=linear alphaMode=straight
uniform sampler2D fieldNoise;
// @texture tex/FD_Noise.png wrap=mirror filter=nearest colorspace=linear alphaMode=straight
uniform vec2 screenSize;
uniform vec2 effectRTSize;
uniform float time;
uniform int touchCount;
uniform vec4 touchPoints[32];

// Fixed official material parameters that are intentionally NOT editor knobs.
const float maskPixelScale = 8.0;
const float displaceBlendIntensity = 0.41100001335144043;
const vec2 activeDisplaceDirection = vec2(1.0,1.0);
const vec2 activeDisplaceMapST = vec2(0.80,0.30);
const vec2 activeSparkMapST = vec2(3.00,1.20);
const float activeSparkDisplaceIntensity = 2.390000104904175;
const float activeSparkHueShiftAmount = 0.20000000298023224;
const float touchBackgroundPixelScale = 8.0;
const vec2 touchDisplaceDirection = vec2(1.0,1.0);
const vec2 touchDisplaceMapST = vec2(0.55,0.30);
const vec2 fieldNoiseST = vec2(1.50,1.46);
const float touchDisplaceSpeed = 2.9000000953674316;
const float touchDisplaceStrength = 0.07999999821186066;
const float noiseEvoSpeed = 0.029999999329447746;
const float noiseDirChangeSpeed = 60.0;
const float noiseDisplaceStrength = 1.0;
const float noiseRadius = 0.47999998927116394;
const float noiseSmoothness = 1.0;
const float sdfCellSize = 0.10999999940395355;
const float sdfMoveSpeed = 9.300000190734863;
const float sdfFalloff = 0.3400000035762787;
const float sdfSmoothness = 0.6299999952316284;
const float touchPosRadius = 0.5;
const float touchPosLowThreshold = 0.6299999952316284;
const float touchPosShineSpeed = 43.0;
const float touchPosSDFFalloff = 0.4099999964237213;
const float touchPosSDFSmoothness = 0.4699999988079071;

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
vec2 nfPixelCenter(vec2 p, float scale) {
    float s = max(scale, 1.0);
    return (floor(p * screenSize / s) * s + vec2(s * 0.5)) / screenSize;
}
vec2 nfDir(vec2 d) {
    return length(d) > 0.00001 ? normalize(d) : vec2(0.70710678);
}

vec3 nfRGBtoHSV(vec3 c) {
    float hi = max(max(c.r,c.g),c.b);
    float lo = min(min(c.r,c.g),c.b);
    float d = hi-lo;
    float h = 0.0;
    if (d > 1e-10) {
        if (hi==c.r) h=mod((c.g-c.b)/d,6.0);
        else if (hi==c.g) h=(c.b-c.r)/d+2.0;
        else h=(c.r-c.g)/d+4.0;
        h=mod(h/6.0,1.0);
    }
    return vec3(h, hi<=1e-10 ? 0.0 : d/hi, hi);
}
vec3 nfHSVtoRGB(vec3 h) {
    vec3 p=clamp(abs(fract(h.xxx+vec3(1.0,2.0/3.0,1.0/3.0))*6.0-3.0)-1.0,0.0,1.0);
    return h.z*(vec3(1.0)+h.y*(p-vec3(1.0)));
}
vec3 nfVariant(float s, float v) {
    vec3 c=nfRGBtoHSV(outlineColor.rgb);
    c.y=clamp(c.y*s,0.0,1.0);
    c.z=clamp(c.z*v,0.0,1.0);
    return nfHSVtoRGB(c);
}
vec3 nfEdge() { return nfVariant(1.0,1.0); }
vec3 nfFill() { return nfVariant(1.0,0.713207483291626); }
vec3 nfGlow() { return nfVariant(1.2253520800891937,1.0); }
vec3 nfSpark() { return nfVariant(1.0676054460523543,1.0); }
vec3 nfDisabledFill() { return nfVariant(1.0794083394988696,0.4970000088214874); }
vec3 nfDisabledSpark() { return nfVariant(1.1197181312580806,0.31132078170776367); }
vec3 nfTouchColor() {
    vec3 c=nfRGBtoHSV(outlineColor.rgb);
    return nfHSVtoRGB(vec3(c.x,1.0,1.0));
}



// -----------------------------------------------------------------------------
// Touch effect. Original client: BlockNoise1 + FD_Noise (RG) + cell SDF,
// 10 touch positions. The standalone simulator must supply per-group touches.
// No-touch means no added fragments, preserving the previous default route.
// -----------------------------------------------------------------------------
float nfSmooth(float x) { x=clamp(x,0.0,1.0); return x*x*(3.0-2.0*x); }

// Extracted TouchEffect/ActiveBlock hash: the .38147 / .93821 seeds are NOT
// multiplied by .1031. This is not the commonly published hash21 formula.
float nfHashScaled(vec2 coord) {
    vec2 p=fract(coord*0.1031);
    float d=dot(vec3(p.x,p.y,p.x),vec3(p.y,p.x,p.x)+33.3300018);
    vec2 q=p+d;
    return fract(q.x*(q.x+q.y));
}
float nfDirectionHash(float tick,float seed) {
    float a=fract(tick*0.1031);
    float d=dot(vec3(a,seed,a),vec3(seed,a,a)+33.3300018);
    vec2 q=vec2(a,seed)+d;
    return fract(q.x*(q.x+q.y));
}
vec2 nfNoiseFlow(float t) {
    float tick=floor(t), u=nfSmooth(fract(t));
    vec2 before=vec2(nfDirectionHash(tick,0.381470025),
                     nfDirectionHash(tick,0.93821007))*2.0-1.0;
    vec2 after=vec2(nfDirectionHash(tick+1.0,0.381470025),
                    nfDirectionHash(tick+1.0,0.93821007))*2.0-1.0;
    return mix(before,after,u);
}

// The original active shader also has a *separate*, much broader TouchPos
// radial highlight, independent of the small TouchHoverRT noise patch.
float nfTouchShine(vec2 screenUV,vec2 displacement) {
    float d=1.0;
    vec2 p=(screenUV+displacement)*screenSize/screenSize.y;
    bool anyPress=false;
    for(int i=0;i<32;++i) {
        if(i>=touchCount) break;
        if(touchPoints[i].w<0.5) continue;
        anyPress=true;
        vec2 c=touchPoints[i].xy*screenSize/screenSize.y;
        float e=length(p-c);
        float k=max(touchPosSDFSmoothness-abs(d-e),0.0)/max(touchPosSDFSmoothness,1e-5);
        k=k*k*touchPosSDFSmoothness;
        d=min(d,e)-0.25*k;
    }
    if(!anyPress) return 0.0;
    float x=clamp((d-touchPosRadius)/(-max(touchPosRadius,1e-5)),0.0,1.0);
    x=nfSmooth(x);
    x=pow(max(x,1e-6),touchPosSDFFalloff);
    float strength=mix(touchPosLowThreshold,1.0,0.5+0.5*sin(time*touchPosShineSpeed))*touchPosBrightness;
    return x*strength;
}

// This is the ActiveBlock touch branch, not an approximation using a white
// procedural square. It keeps the extracted channel-wise Overlay and uses
// _SDFCellSize in *screen height* units rather than 1/screenSize units.
vec3 nfTouchEffect(vec2 p) {
    vec2 dir=nfDir(touchDisplaceDirection), perp=vec2(-dir.y,dir.x);
    float t=(time/20.0)*touchDisplaceSpeed;
    vec2 texUV=p*touchDisplaceMapST;
    float a=texture2D(blockNoise,nfPixelCenter(texUV+dir*t,touchBackgroundPixelScale)).r;
    float b=texture2D(blockNoise,nfPixelCenter(texUV+perp*t,touchBackgroundPixelScale)).r;
    vec2 disp=dir*(a-0.5)+perp*(b-0.5);
    float hover=texture2D(touchHoverRT,p+disp*touchDisplaceStrength).r;

    // Smoothstep is applied to the displaced touch mask *before* SDF color.
    float gate=nfSmooth((hover-(noiseRadius-noiseSmoothness))/max(2.0*noiseSmoothness,1e-5));
    float core=nfSmooth(hover);

    vec2 flow=nfNoiseFlow(time*noiseDirChangeSpeed);
    vec2 mapUV=p*fieldNoiseST+flow*noiseEvoSpeed;
    vec2 fd=texture2D(fieldNoise,mapUV).rg*2.0-1.0;

    float cellPx=max(screenSize.y*sdfCellSize,1e-5);
    // Equivalent to (p + fd * noiseDisplaceStrength * cellPx/screenSize)
    // * screenSize / cellPx; keeps ~one CELL of perturbation at every resolution.
    vec2 cell=p*screenSize/cellPx + fd*noiseDisplaceStrength;
    vec2 c=floor(cell), f=fract(cell);
    float animation=time*sdfMoveSpeed;
    float tick=floor(animation), w=nfSmooth(fract(animation));
    vec2 key=c+vec2(tick);
    vec2 seed0=vec2(nfHashScaled(key),nfHashScaled(key+vec2(17.1700001)));
    vec2 seed1=vec2(nfHashScaled(key+vec2(1.0)),nfHashScaled(key+vec2(18.1700001)));
    float dist=length(f-mix(seed0,seed1,w));
    float sdf=nfSmooth((dist-sdfFalloff)/max(sdfSmoothness,1e-5));

    vec3 noiseTint=nfTouchColor();          // official default: red (1,0,0)
    // _TouchGlowColor is copied from touchEffectMaterial._GlowColor, NOT
    // from activeBlockMaterial._GlowColor. Default material value (1,0,0).
    // Derive the same fully saturated hue as _NoiseTint for recolorable skins.
    vec3 glowColor=nfTouchColor();
    vec3 cellColor=clamp(vec3(sdf)*(vec3(1.0)+noiseTint),0.0,1.0);
    vec3 n=gate*cellColor;
    vec3 g=core*glowColor;

    // Reconstructed, channel-wise Overlay from Unlit_ActiveBlock:
    // if n >= .5: 1-2*(1-n)*(1-g), else: 2*n*g.
    vec3 low=2.0*n*g;
    vec3 high=1.0-2.0*(vec3(1.0)-n)*(vec3(1.0)-g);
    vec3 overlay=clamp(mix(low,high,step(vec3(0.5),n)),0.0,1.0);
    return (overlay + 0.5*g)*touchHoverStrength;
}





vec2 pixelCenter(vec2 p, float scale) {
    float s = max(scale, 1.0);
    return (floor(p * screenSize / s) * s + vec2(s * 0.5)) / screenSize;
}

vec3 rgb2hsvFaithful(vec3 c) {
    // Equivalent to the branchless RGB->HSV sequence emitted by HLSLcc.
    vec4 K = vec4(0.0, -0.3333333333, 0.6666666667, -1.0);
    vec4 p = (c.g >= c.b) ? vec4(c.g, c.b, K.x, K.y)
                          : vec4(c.b, c.g, K.w, K.z);
    vec4 q = (c.r >= p.x) ? vec4(c.r, p.y, p.z, p.x)
                          : vec4(p.x, p.y, p.w, c.r);
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec3 hsv2rgbFaithful(vec3 hsv) {
    vec3 p = abs(fract(hsv.xxx + vec3(1.0, 0.666666667, 0.333333333)) * 6.0 - 3.0) - 1.0;
    p = clamp(p, 0.0, 1.0);
    return hsv.z * (vec3(1.0) + hsv.y * (p - vec3(1.0)));
}

vec3 overlayFaithful(vec3 base, vec3 blend) {
    vec3 low = 2.0 * base * blend;
    vec3 high = vec3(1.0) - 2.0 * (vec3(1.0) - base) * (vec3(1.0) - blend);
    vec3 chooseHigh = step(vec3(0.5), base);
    return clamp(mix(low, high, chooseHigh), 0.0, 1.0);
}

void main() {
    // Original ActiveBlock central 16:9 safety crop on ultrawide screens.
    float limit = (8.0 / 9.0) * (screenSize.y / screenSize.x);
    if (abs(uv.x - 0.5) > limit) { gl_FragColor = texture2D(screenTexture, uv); return; }

    float comp = texture2D(composeRT, uv).r;
    vec2 edgeUV = (floor(uv * effectRTSize) + vec2(0.5)) / effectRTSize;
    float edge = texture2D(effectRT, edgeUV).r;
    float glow = texture2D(effectRT, uv).g;

    float dn = nfMaskBit(uv, 4.0);
    float ds = nfMaskBit(uv, 5.0);
    float ready = texture2D(readyComposeRT, uv).r;
    float rd = ready * abs(ds - dn);

    float sum = glow + edge + comp;
    float total = rd + sum + texture2D(touchHoverRT, uv).r;
    if (total - 0.0001 < 0.0) { gl_FragColor = texture2D(screenTexture, uv); return; }

    vec3 rgb = vec3(0.0);
    float alpha = 0.0;

    if (sum > 0.0001) {
        float edgeTerm = edge * edgeOpacity;
        float glowAdj = glow * (1.0 - (edge + comp));
        float glowTerm = glowAdj * glowIntensity;

        vec2 dir = normalize(activeDisplaceDirection);
        vec2 perp = vec2(-dir.y, dir.x);
        float t = (time / 20.0) * activeDisplaceSpeed; // Unity _Time.x

        vec2 duv = uv * activeDisplaceMapST;
        float a1 = texture2D(blockNoise, pixelCenter(duv + dir  * t, backgroundPixelScale)).r;
        float a2 = texture2D(blockNoise, pixelCenter(duv + perp * t, backgroundPixelScale)).r;
        float dispAvg = (a1 + a2) * 0.5;
        vec2 disp = dir * (a1 - 0.5) + perp * (a2 - 0.5);

        vec2 sceneUV = pixelCenter(uv, backgroundPixelScale) + disp * activeDisplaceStrength;
        vec2 sparkUV = uv * activeSparkMapST + disp * activeSparkDisplaceIntensity;

        float spark = texture2D(pointNoise, sparkUV).r;
        vec3 sparkVec = spark * nfSpark() * dispAvg * activeSparkMapOpacity;

        vec3 scene = texture2D(sceneColorRT, sceneUV).rgb;
        vec3 hsv = rgb2hsvFaithful(scene);
        hsv += sparkVec * activeSparkHueShiftAmount;
        vec3 shiftedScene = hsv2rgbFaithful(hsv);
        vec3 processed = overlayFaithful(shiftedScene, sparkVec);

        vec3 fillBase = nfFill() - vec3(dispAvg * displaceBlendIntensity);
        vec3 fill = fillBase + fillStrength * (processed - fillBase);

        rgb = fill * comp
            + nfEdge() * edgeTerm
            + nfGlow() * glowTerm;

        rgb *= 1.0 + nfTouchShine(uv, disp * activeDisplaceStrength);
        alpha = comp * fillOpacity + edgeTerm + glowTerm;
    }

    // ActiveBlock also reproduces the Ready pulse. Because ActiveBlock uses
    // Blend One / OneMinusSrcAlpha and this Ready branch contributes no alpha,
    // it is additive exactly as in the extracted shader.
    if (rd > 0.0001) {
        float pulse = sin(time * shineSpeed) * 0.5 + 1.0;
        vec3 shine = vec3(1.0) * shineBrightness * pulse;
        rgb += (rd * rd) * shine;
    }

    float hover = texture2D(touchHoverRT, uv).r;
    if (hover > 0.0001) rgb += nfTouchEffect(uv);

    // IMPORTANT: this is the source value from Unlit/ActiveBlock. Do NOT manually
    // mix the destination here. Fixed-function state is One / OneMinusSrcAlpha.
    vec4 behind = texture2D(screenTexture, uv);
    gl_FragColor = vec4(rgb + behind.rgb * (1.0 - alpha),
                         alpha + behind.a * (1.0 - alpha));
}
