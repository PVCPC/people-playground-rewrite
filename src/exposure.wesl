// ExposureHistogram and AutoExposure's compute kernels, disassembled. first.x picks KAutoExposureAvgLuminance_fixed.
struct Params { params1: vec4<f32>, params2: vec4<f32>, scale_offset_res: vec4<f32>, first: vec4<f32> }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<storage, read_write> bins: array<atomic<u32>, 128>;
@group(0) @binding(3) var<storage, read_write> history: array<f32, 1>;
@group(0) @binding(4) var output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var<uniform> params: Params;

@compute @workgroup_size(16, 16, 1)
fn histogram(@builtin(global_invocation_id) id: vec3<u32>) {
    let so = params.scale_offset_res;
    let at = vec2<f32>(id.xy) * 2.0;
    if at.x < so.z && at.y < so.w {
        let uv = at / so.zw;
        let d = uv - 0.5;
        let vignette = max(1.0 - dot(d, d), 0.0);
        let weight = u32(vignette * vignette * 64.0);
        let colour = textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
        let luminance = dot(colour, vec3<f32>(0.212673, 0.715152, 0.072175));
        // D3D: log of 0 is -inf and of less is NaN; both saturate to 0.
        let position = select(0.0, saturate(log2(luminance) * so.x + so.y), luminance > 0.0);
        atomicAdd(&bins[u32(position * 127.0)], weight);
    }
}

fn average_luminance() -> f32 {
    var highest = 0u;
    var values: array<f32, 128>;
    for (var i = 0u; i < 128u; i++) {
        let value = atomicLoad(&bins[i]);
        highest = max(highest, value);
        values[i] = f32(value);
    }
    if highest == 0u {
        return params.params1.z;
    }
    let scale = 1.0 / f32(highest);
    var sum = 0.0;
    for (var i = 0u; i < 128u; i++) {
        values[i] *= scale;
        sum += values[i];
    }
    var weighted = 0.0;
    var total = 0.0;
    var dark = sum * params.params1.x;
    var bright = sum * params.params1.y;
    for (var i = 0u; i < 128u; i++) {
        let offset = min(dark, values[i]);
        var value = values[i] - offset;
        dark -= offset;
        bright -= offset;
        value = min(bright, value);
        bright -= value;
        let luminance = pow(2.0, (f32(i) / 128.0 - params.scale_offset_res.y) / params.scale_offset_res.x);
        weighted += luminance * value;
        total += value;
    }
    return clamp(weighted / max(total, 0.0001), params.params1.z, params.params1.w);
}

@compute @workgroup_size(1, 1, 1)
fn average() {
    var exposure = params.params2.z / max(average_luminance(), 0.0001);
    if params.first.x == 0.0 {
        let previous = history[0];
        let delta = exposure - previous;
        let speed = select(params.params2.y, params.params2.x, delta > 0.0);
        exposure = previous + delta * (1.0 - pow(2.0, -speed * params.params2.w));
    }
    history[0] = exposure;
    textureStore(output, vec2<i32>(0, 0), vec4<f32>(exposure));
}
