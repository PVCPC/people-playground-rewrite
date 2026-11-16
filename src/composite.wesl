// Hidden/PostProcessing/Uber (the DISTORT, CHROMATIC_ABERRATION, BLOOM or BLOOM_LOW, GRAIN, COLOR_GRADING_HDR_3D variant, its
// fragment stage disassembled): the lit scene through the lens distortion and chromatic aberration, plus the bloom texture
// (tent-filtered), the grain, then the graded result.
struct Settings { texel: vec2<f32>, sample_scale: f32, intensity: f32 }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var lit: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var lit_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var bloom: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var bloom_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var exposure: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var exposure_sampler: sampler;
// VideoSettingsInitBehaviour.SetPost: postExposure, BLOOM_LOW, Tonemapper.Neutral, the Bloom effect active.
struct Video { post_exposure: f32, fast: f32, neutral: f32, bloom: f32 }
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var<uniform> video: Video;
// The blended ColorGrading's Lut3DBaker inputs and the Bloom colour (lights.rs Grade).
struct Grade {
    balance: vec4<f32>,
    colour_filter: vec4<f32>,
    hue_sat_con: vec4<f32>,
    lift: vec4<f32>,
    inv_gamma: vec4<f32>,
    gain: vec4<f32>,
    bloom_color: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var<uniform> grading: Grade;
// LensDistortion, ChromaticAberration and Grain (lights.rs Fx).
struct Fx {
    distortion_amount: vec4<f32>,
    distortion_center_scale: vec4<f32>,
    flags: vec4<f32>,
    grain1: vec4<f32>,
    grain2: vec4<f32>,
    main_size: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> fx: Fx;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var grain: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var grain_sampler: sampler;

// Unity's texture coordinates count y from the bottom.
fn flip(uv: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(uv.x, 1.0 - uv.y);
}

// Uber's Distort (Unity coordinates).
fn distort(uv: vec2<f32>) -> vec2<f32> {
    if fx.flags.y == 0.0 {
        return uv;
    }
    let amount = fx.distortion_amount;
    let at = (uv - 0.5) * amount.z + 0.5;
    let ruv = fx.distortion_center_scale.zw * (at - 0.5 - fx.distortion_center_scale.xy);
    let r = length(ruv);
    var f: f32;
    if amount.w > 0.0 {
        f = tan(r * amount.x) / (r * amount.y);
    } else {
        f = amount.x / r * atan(r * amount.y);
    }
    return at + ruv * (f - 1.0);
}

// ChromaticAberrationRenderer's internal 3x1 spectral lookup (red, green, blue), bilinear and clamped, at (t, 0).
fn spectral(t: f32) -> vec3<f32> {
    let x = clamp(t * 3.0 - 0.5, 0.0, 2.0);
    if x < 1.0 {
        return mix(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), x);
    }
    return mix(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), x - 1.0);
}

fn tap(uv: vec2<f32>, x: f32, y: f32) -> vec4<f32> {
    return textureSample(bloom, bloom_sampler, uv + settings.texel * vec2<f32>(x, y));
}

@fragment
fn fragment(@location(2) screen_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let unity = flip(screen_uv);
    let uv = flip(distort(unity));
    let s = settings.sample_scale;
    let tent = tap(uv, -s, -s) + tap(uv, 0.0, -s) * 2.0 + tap(uv, s, -s)
        + tap(uv, -s, 0.0) * 2.0 + tap(uv, 0.0, 0.0) * 4.0 + tap(uv, s, 0.0) * 2.0
        + tap(uv, -s, s) + tap(uv, 0.0, s) * 2.0 + tap(uv, s, s);
    let auto_exposure = textureSample(exposure, exposure_sampler, screen_uv).r;
    var colour = textureSample(lit, lit_sampler, uv);
    if fx.flags.x != 0.0 {
        // CHROMATIC_ABERRATION: 3 to 16 samples along the fringe, each through Distort, weighted by the spectral lookup.
        let coords = 2.0 * unity - 1.0;
        let diff = -coords * dot(coords, coords) * fx.flags.x;
        let samples = clamp(i32(length(diff * fx.main_size.xy * 0.5)), 3, 16);
        let delta = diff / f32(samples);
        var pos = unity;
        var sum = vec4<f32>(0.0);
        var filter_sum = vec4<f32>(0.0);
        for (var i = 0; i < samples; i++) {
            let t = (f32(i) + 0.5) / f32(samples);
            let c = textureSampleLevel(lit, lit_sampler, flip(clamp(distort(pos), vec2<f32>(0.0), vec2<f32>(1.0))), 0.0);
            let tint = vec4<f32>(spectral(t), 1.0);
            sum += c * tint;
            filter_sum += tint;
            pos += delta;
        }
        colour = sum / filter_sum;
    }
    colour = vec4<f32>(colour.rgb * auto_exposure, colour.a);
    var glow = tent * 0.0625;
    if video.fast != 0.0 {
        let h = s * 0.5;
        glow = (tap(uv, -h, -h) + tap(uv, h, -h) + tap(uv, -h, h) + tap(uv, h, h)) * 0.25;
    }
    colour += glow * settings.intensity * video.bloom * vec4<f32>(grading.bloom_color.rgb, 1.0);
    if fx.flags.z != 0.0 {
        // GRAIN: the lookup tiled over the screen at the Halton offset, less over bright pixels by lumContrib.
        let g = textureSample(grain, grain_sampler, flip(unity * fx.grain2.xy + fx.grain2.zw)).rgb;
        let lum = 1.0 - fx.grain1.x * sqrt(dot(clamp(colour.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), LUMA));
        colour = vec4<f32>(colour.rgb + colour.rgb * g * fx.grain1.y * lum, colour.a);
    }
    return vec4<f32>(grade(colour.rgb), colour.a);
}

// ColorGrading, High Definition Range on the 3D LUT (Lut3DBaker's ColorGrade, the same steps as the game's Lut2DBaker HDR
// programs), evaluated per pixel instead of through the 33^3 LUT. The settings are the post volumes' blend (lights.rs).

// Written row by row: `v * M` is the row matrix times v.
const SRGB_TO_AP0 = mat3x3<f32>(0.4397010, 0.3829780, 0.1773350, 0.0897923, 0.8134230, 0.0967616, 0.0175440, 0.1115440, 0.8707040);
const AP0_TO_AP1 = mat3x3<f32>(1.4514393161, -0.2365107469, -0.2149285693, -0.0765537734, 1.1762296998, -0.0996759264, 0.0083161484, -0.0060324498, 0.9977163014);
const AP1_TO_AP0 = mat3x3<f32>(0.6954522414, 0.1406786965, 0.1638690622, 0.0447945634, 0.8596711185, 0.0955343182, -0.0055258826, 0.0040252103, 1.0015006723);
const AP1_TO_XYZ = mat3x3<f32>(0.6624541811, 0.1340042065, 0.1561876870, 0.2722287168, 0.6740817658, 0.0536895174, -0.0055746495, 0.0040607335, 1.0103391003);
const XYZ_TO_AP1 = mat3x3<f32>(1.6410233797, -0.3248032942, -0.2364246952, -0.6636628587, 1.6153315917, 0.0167563477, 0.0117218943, -0.0082844420, 0.9883948585);
const D60_TO_D65 = mat3x3<f32>(0.98722400, -0.00611327, 0.0159533, -0.00759836, 1.00186000, 0.0053302, 0.00307257, -0.00509595, 1.0816800);
const XYZ_TO_REC709 = mat3x3<f32>(3.2409699419, -1.5373831776, -0.4986107603, -0.9692436363, 1.8759675015, 0.0415550574, 0.0556300797, -0.2039769589, 1.0569715142);
const AP1_LUMA: vec3<f32> = vec3<f32>(0.2722287168, 0.6740817658, 0.0536895174);

const LIN_TO_LMS = mat3x3<f32>(0.390405, 0.549941, 0.00892632, 0.0708416, 0.963172, 0.00135775, 0.0231082, 0.128021, 0.936245);
const LMS_TO_LIN = mat3x3<f32>(2.85847, -1.62879, -0.024891, -0.210182, 1.1582, 0.000324281, -0.041812, -0.118169, 1.06867);
const LUMA: vec3<f32> = vec3<f32>(0.2126729, 0.7151522, 0.072175);
const HALF_MAX: f32 = 65504.0;
const ACESCC_MIDGRAY: f32 = 0.4135884;
// The LUT's range: Uber's saturate(LinearToLogC(colour)), LogC 0 and 1 back in linear.
const LOGC_MIN: f32 = -0.092819 / 5.301883;
const LOGC_MAX: f32 = 58.856633;

fn aces_to_acescc(aces: vec3<f32>) -> vec3<f32> {
    let x = clamp(aces, vec3<f32>(0.0), vec3<f32>(HALF_MAX));
    let low = (log2(x * 0.5 + 1.525878e-05) + 9.72) / 17.52;
    let high = (log2(x) + 9.72) / 17.52;
    return select(high, low, x < vec3<f32>(3.05175708e-05));
}

fn acescc_to_aces(x: vec3<f32>) -> vec3<f32> {
    let e = exp2(x * 17.52 - 9.72);
    let mid = select(vec3<f32>(HALF_MAX), e, x < vec3<f32>(1.46799636));
    return select(mid, (e - 1.52587891e-05) * 2.0, x < vec3<f32>(-0.301369876));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.b, c.g, k.w, k.z), vec4<f32>(c.g, c.b, k.x, k.y), step(c.b, c.g));
    let q = mix(vec4<f32>(p.x, p.y, p.w, c.r), vec4<f32>(c.r, p.y, p.z, p.x), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-4;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(c.xxx + vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return c.z * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

fn grade(linear: vec3<f32>) -> vec3<f32> {
    // Uber: colour * _PostExposure (Exp2 of postExposure), into the LUT's LogC range.
    let colour = clamp(linear * exp2(video.post_exposure), vec3<f32>(LOGC_MIN), vec3<f32>(LOGC_MAX));
    // ACEScc (log): Contrast around ACEScc_MIDGRAY.
    let cc = (aces_to_acescc(colour * SRGB_TO_AP0) - ACESCC_MIDGRAY) * grading.hue_sat_con.z + ACESCC_MIDGRAY;
    // ACEScg (linear): WhiteBalance, ColorFilter, the ChannelMixer (identity: no volume sets it), LiftGammaGainHDR.
    var cg = acescc_to_aces(cc) * AP0_TO_AP1;
    cg = ((cg * LIN_TO_LMS) * grading.balance.xyz) * LMS_TO_LIN;
    cg = cg * grading.colour_filter.xyz;
    cg = cg * grading.gain.xyz + grading.lift.xyz;
    cg = sign(cg) * exp2(log2(abs(cg)) * grading.inv_gamma.xyz);
    cg = max(cg, vec3<f32>(0.0));
    // Hue shift (the curves are flat: no hue offset, the saturation multiplier 1), then Saturation.
    var hsv = rgb_to_hsv(cg);
    var hue = hsv.x + grading.hue_sat_con.x;
    hue = select(select(hue, hue - 1.0, hue > 1.0), hue + 1.0, hue < 0.0);
    cg = hsv_to_rgb(vec3<f32>(hue, hsv.y, hsv.z));
    let luma = dot(cg, LUMA);
    cg = vec3<f32>(luma) + grading.hue_sat_con.y * (cg - vec3<f32>(luma));
    if video.neutral != 0.0 {
        return neutral_tonemap(cg * AP1_TO_XYZ * D60_TO_D65 * XYZ_TO_REC709);
    }
    return aces_tonemap(cg * AP1_TO_AP0);
}

fn neutral_curve(x: vec3<f32>) -> vec3<f32> {
    const a = 0.2; const b = 0.29; const c = 0.24; const d = 0.272; const e = 0.02; const f = 0.3;
    return (x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f) - e / f;
}

fn neutral_tonemap(x: vec3<f32>) -> vec3<f32> {
    let white_scale = 1.0 / neutral_curve(vec3<f32>(5.3)).x;
    return neutral_curve(max(x, vec3<f32>(0.0)) * white_scale) * white_scale;
}

fn rgb_to_saturation(c: vec3<f32>) -> f32 {
    let lo = min(c.r, min(c.g, c.b));
    let hi = max(c.r, max(c.g, c.b));
    return (max(hi, 1e-10) - max(lo, 1e-10)) / max(hi, 1e-2);
}

fn rgb_to_yc(c: vec3<f32>) -> f32 {
    let chroma = sqrt(c.b * (c.b - c.g) + c.g * (c.g - c.r) + c.r * (c.r - c.b));
    return (c.b + c.g + c.r + 1.75 * chroma) / 3.0;
}

fn sigmoid_shaper(x: f32) -> f32 {
    let t = max(1.0 - abs(x / 2.0), 0.0);
    return (1.0 + sign(x) * (1.0 - t * t)) / 2.0;
}

fn glow_fwd(yc: f32, gain: f32, mid: f32) -> f32 {
    if yc <= 2.0 / 3.0 * mid {
        return gain;
    }
    if yc >= 2.0 * mid {
        return 0.0;
    }
    return gain * (mid / yc - 0.5);
}

fn rgb_to_hue(c: vec3<f32>) -> f32 {
    if c.r == c.g && c.g == c.b {
        return 0.0;
    }
    let hue = degrees(atan2(sqrt(3.0) * (c.g - c.b), 2.0 * c.r - c.g - c.b));
    return select(hue, hue + 360.0, hue < 0.0);
}

fn center_hue(hue: f32) -> f32 {
    if hue < -180.0 {
        return hue + 360.0;
    }
    return select(hue, hue - 360.0, hue > 180.0);
}

fn dark_to_dim_surround(cv: vec3<f32>) -> vec3<f32> {
    let xyz = cv * AP1_TO_XYZ;
    let d = max(xyz.x + xyz.y + xyz.z, 1e-10);
    let x = xyz.x / d;
    let y = xyz.y / d;
    let big_y = pow(clamp(xyz.y, 0.0, 65535.0), 0.9811);
    let m = big_y / max(y, 1e-10);
    return vec3<f32>(x * m, big_y, (1.0 - x - y) * m) * XYZ_TO_AP1;
}

// PostProcessing's ACES.hlsl AcesTonemap: RRT + ODT.Academy.RGBmonitor_100nits_dim.
fn aces_tonemap(input: vec3<f32>) -> vec3<f32> {
    var aces = input;
    // Glow.
    let saturation = rgb_to_saturation(aces);
    let s = sigmoid_shaper((saturation - 0.4) / 0.2);
    aces = aces * (1.0 + glow_fwd(rgb_to_yc(aces), 0.05 * s, 0.08));
    // Red modifier.
    let centered = center_hue(rgb_to_hue(aces));
    var weight = smoothstep(0.0, 1.0, 1.0 - abs(2.0 * centered / 135.0));
    weight = weight * weight;
    aces.r = aces.r + weight * saturation * (0.03 - aces.r) * (1.0 - 0.82);
    // Rendering space, global desaturation.
    var cg = max(aces * AP0_TO_AP1, vec3<f32>(0.0));
    cg = mix(vec3<f32>(dot(cg, AP1_LUMA)), cg, 0.96);
    // The luminance fit.
    let fit = (cg * (278.5085 * cg + 10.7772)) / (cg * (293.6045 * cg + 88.7122) + 80.6889);
    var cv = dark_to_dim_surround(fit);
    cv = mix(vec3<f32>(dot(cv, AP1_LUMA)), cv, 0.93);
    return cv * AP1_TO_XYZ * D60_TO_D65 * XYZ_TO_REC709;
}
