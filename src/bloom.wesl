// Hidden/PostProcessing/Bloom's fragment stages, disassembled: Prefilter13, Downsample13 and UpsampleTent.
struct Settings { threshold: vec4<f32>, texel: vec2<f32>, clamp_to: f32, sample_scale: f32, pass_index: u32 }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var bloom: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var bloom_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var exposure: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var exposure_sampler: sampler;

fn tap(uv: vec2<f32>, x: f32, y: f32) -> vec4<f32> {
    return textureSample(source, source_sampler, saturate(uv + settings.texel * vec2<f32>(x, y)));
}

fn downsample(uv: vec2<f32>) -> vec4<f32> {
    let a = tap(uv, -1.0, -1.0);
    let b = tap(uv, 0.0, -1.0);
    let c = tap(uv, 1.0, -1.0);
    let d = tap(uv, -0.5, -0.5);
    let e = tap(uv, 0.5, -0.5);
    let f = tap(uv, -1.0, 0.0);
    let g = tap(uv, 0.0, 0.0);
    let h = tap(uv, 1.0, 0.0);
    let i = tap(uv, -0.5, 0.5);
    let j = tap(uv, 0.5, 0.5);
    let k = tap(uv, -1.0, 1.0);
    let l = tap(uv, 0.0, 1.0);
    let m = tap(uv, 1.0, 1.0);
    return (d + e + i + j) * 0.125 + (a + b + g + f + b + c + h + g + f + g + l + k + g + h + m + l) * 0.03125;
}

// DownsampleBox4Tap, the fast mode's.
fn downsample4(uv: vec2<f32>) -> vec4<f32> {
    return (tap(uv, -1.0, -1.0) + tap(uv, 1.0, -1.0) + tap(uv, -1.0, 1.0) + tap(uv, 1.0, 1.0)) * 0.25;
}

fn prefilter(uv: vec2<f32>, down: vec4<f32>) -> vec4<f32> {
    let exposed = min(down, vec4<f32>(65504.0)) * textureSample(exposure, exposure_sampler, uv).r;
    let colour = min(exposed, vec4<f32>(settings.clamp_to));
    let brightest = max(colour.r, max(colour.g, colour.b));
    let curve = settings.threshold;
    let soft = clamp(brightest - curve.y, 0.0, curve.z);
    let weight = max(soft * soft * curve.w, brightest - curve.x);
    return colour * weight / max(brightest, 0.0001);
}

fn upsample(uv: vec2<f32>) -> vec4<f32> {
    let s = settings.sample_scale;
    let tent = tap(uv, -s, -s) + tap(uv, 0.0, -s) * 2.0 + tap(uv, s, -s)
        + tap(uv, -s, 0.0) * 2.0 + tap(uv, 0.0, 0.0) * 4.0 + tap(uv, s, 0.0) * 2.0
        + tap(uv, -s, s) + tap(uv, 0.0, s) * 2.0 + tap(uv, s, s);
    return tent * 0.0625 + textureSample(bloom, bloom_sampler, uv);
}

// UpsampleBox, the fast mode's.
fn upsample4(uv: vec2<f32>) -> vec4<f32> {
    let s = settings.sample_scale * 0.5;
    let box = (tap(uv, -s, -s) + tap(uv, s, -s) + tap(uv, -s, s) + tap(uv, s, s)) * 0.25;
    return box + textureSample(bloom, bloom_sampler, uv);
}

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    switch settings.pass_index {
        case 0u: { return prefilter(uv, downsample(uv)); }
        case 1u: { return prefilter(uv, downsample4(uv)); }
        case 2u: { return downsample(uv); }
        case 3u: { return downsample4(uv); }
        case 4u: { return upsample(uv); }
        default: { return upsample4(uv); }
    }
}
