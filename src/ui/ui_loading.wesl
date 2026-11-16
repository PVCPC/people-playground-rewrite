// Unlit/LoadingShader's fragment stage, from its glcore program, on Image.Type Sliced's mesh: texels with blue under 0.5 are
// discarded; red and green (times 2 less 1) pick the side, whose uv runs a sin(_Time.z * 4 + phase) pulse raised to the
// 24th: white where it passes 0.4, else black at 0.7; times the vertex colour. See loading.rs for `params`.
@group(1) @binding(0) var<uniform> params: array<vec4<f32>, 5>;
@group(1) @binding(1) var main_tex: texture_2d<f32>;
@group(1) @binding(2) var main_sampler: sampler;

// Image.GenerateSlicedSprite along one axis: outer to inner uv over the first border, inner to inner over the middle,
// inner to outer over the last.
fn sliced(t: f32, b0: f32, b1: f32, o0: f32, i0: f32, i1: f32, o1: f32) -> f32 {
    if t < b0 {
        return mix(o0, i0, t / b0);
    }
    if t > 1.0 - b1 {
        return mix(i1, o1, (t - (1.0 - b1)) / b1);
    }
    return mix(i0, i1, (t - b0) / (1.0 - b0 - b1));
}

fn pulse(phase: f32) -> vec4<f32> {
    var x = sin(params[0].x * 4.0 + phase);
    x = x * 0.5 + 0.5;
    x = exp2(log2(x) * 24.0);
    x = clamp((x + -0.400000006) * 20.0000076, 0.0, 1.0);
    x = x * x * (x * -2.0 + 3.0);
    return select(vec4<f32>(0.0, 0.0, 0.0, 0.699999988), vec4<f32>(1.0), x != 0.0) * params[1];
}

@fragment
fn fragment(@location(0) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let outer = params[2];
    let inner = params[3];
    let b = params[4];
    let uv = vec2<f32>(sliced(in_uv.x, b.x, b.z, outer.x, inner.x, inner.z, outer.z), sliced(1.0 - in_uv.y, b.y, b.w, outer.y, inner.y, inner.w, outer.w));
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    if c.z + -0.5 < 0.0 {
        discard;
    }
    let d = c.xy * 2.0 + vec2<f32>(-1.0);
    if 0.5 < d.x {
        return pulse((uv.y + 2.0) * 1.57079637);
    }
    if d.x < -0.5 {
        return pulse((-uv.y + 1.0) * 1.57079637);
    }
    if 0.5 < d.y {
        return pulse((-uv.x + 4.0) * 1.57079637);
    }
    if d.y < -0.5 {
        return pulse((uv.x + 1.0) * 1.57079637);
    }
    return vec4<f32>(0.0);
}
