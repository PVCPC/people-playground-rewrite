// Jaap/LegacyBodyShader's fragment stage, translated instruction by instruction from the game's D3D11 program. Registers hold raw bits, so
// masks and floats mix as they do in D3D.
// cb0 is the shader's $Globals, laid out as Unity's: see `Limb` in src/limb_material.rs.
// Then cb0[269] is _Time.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> cb0: array<vec4<u32>, 270>;
// The sprite's vertex colour, and how to get the mesh's vertex position back from uv (scale, offset).
struct Extra { tint: vec4<f32>, uv_to_local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var t0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var s0: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var t1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var s1: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var t2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var s2: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var t3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var s3: sampler;
const icb = array<vec4<u32>, 6>(vec4<u32>(1060320051u, 1065353216u, 0u, 0u), vec4<u32>(1041865114u, 1055286886u, 0u, 0u), vec4<u32>(1008981770u, 1050253722u, 0u, 0u), vec4<u32>(1028443341u, 1055286886u, 0u, 0u), vec4<u32>(1065353216u, 1065353216u, 0u, 0u), vec4<u32>(1065353216u, 1065353216u, 0u, 0u));

fn f(x: u32) -> f32 { return bitcast<f32>(x); }
fn u(x: f32) -> u32 { return bitcast<u32>(x); }
fn b(x: bool) -> u32 { return select(0u, 0xffffffffu, x); }

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let local = uv * extra.uv_to_local.xy + extra.uv_to_local.zw;
    let v0 = vec4<u32>(u(uv.x), u(uv.y), u(local.x), u(local.y));
    let v2 = bitcast<vec4<u32>>(extra.tint);
    let cb1 = array<vec4<u32>, 1>(cb0[269u]);
    var r0 = vec4<u32>(0u);
    var r1 = vec4<u32>(0u);
    var r2 = vec4<u32>(0u);
    var r3 = vec4<u32>(0u);
    var r4 = vec4<u32>(0u);
    var r5 = vec4<u32>(0u);
    var r6 = vec4<u32>(0u);
    var r7 = vec4<u32>(0u);
    var r8 = vec4<u32>(0u);
    var o0 = vec4<u32>(0u);
    r0 = bitcast<vec4<u32>>(textureSample(t0, s0, vec2<f32>(f(v0.x), f(v0.y))));
    r1.x = u(f(r0.w) + (-0.010000));
    r1.x = b(f(r1.x) < 0.000000);
    if r1.x != 0u { discard; }
    r1.x = u(log2(f(cb0[266u].y)));
    r1.x = u(f(r1.x) * 0.300000);
    r1.x = u(pow(2.0, f(r1.x)));
    r1.x = u(f(r1.x) * f(cb0[267u].y));
    r1.x = u(saturate(f(r1.x) * 0.880000));
    r1.y = u(f(r1.x) + f(cb0[266u].w));
    r1.y = b(f(r1.y) < 0.010000);
    {
        let tx = v0.z;
        let ty = v0.w;
        r2.x = tx;
        r2.y = ty;
    }
    r2.z = 0u;
    {
        let tx = u(f(r2.x) * 35.000000);
        let ty = u(f(r2.y) * 35.000000);
        let tz = u(f(r2.z) * 35.000000);
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
    }
    {
        let tx = u(round(f(r2.x)));
        let ty = u(round(f(r2.y)));
        let tz = u(round(f(r2.z)));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
    }
    {
        let tx = u(f(r2.x) / 35.000000);
        let ty = u(f(r2.y) / 35.000000);
        let tz = u(f(r2.z) / 35.000000);
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
    }
    {
        let tz = 0u;
        let tw = 1176256512u;
        r1.z = tz;
        r1.w = tw;
    }
    loop {
        r2.w = b(bitcast<i32>(r1.z) >= bitcast<i32>(cb0[266u].x));
        if r2.w != 0u { break; }
        r2.w = bitcast<u32>(i32(f(cb0[r1.z + 10u].w)));
        r3.x = u(f(icb[r2.w + 0u].x) / f(cb0[r1.z + 10u].z));
        {
            let tx = u(-f(cb0[r1.z + 10u].x));
            let ty = u(-f(cb0[r1.z + 10u].y));
            r4.x = tx;
            r4.y = ty;
        }
        r4.z = u(-f(r3.x));
        {
            let tx = u(f(r2.x) + f(r4.x));
            let ty = u(f(r2.y) + f(r4.y));
            let tz = u(f(r2.z) + f(r4.z));
            r3.x = tx;
            r3.y = ty;
            r3.z = tz;
        }
        r3.x = u(f(r3.x) * f(r3.x) + f(r3.y) * f(r3.y) + f(r3.z) * f(r3.z));
        r3.x = u(sqrt(f(r3.x)));
        r3.x = u(log2(f(r3.x)));
        r2.w = u(f(r3.x) * f(icb[r2.w + 0u].y));
        r2.w = u(pow(2.0, f(r2.w)));
        r1.w = u(min(f(r1.w), f(r2.w)));
        r1.z = r1.z + 1u;
    }
    r1.z = u(max(f(cb0[268u].x), 0.100000));
    r1.z = u(f(r1.w) / f(r1.z));
    r1.w = b(0.700000 < f(r1.z));
    r1.y = r1.y & r1.w;
    r1.w = b(0.010000 >= f(cb0[267u].w));
    r1.y = r1.w & r1.y;
    if r1.y != 0u {
        {
            let tx = u(f(r0.x) * f(v2.x));
            let ty = u(f(r0.y) * f(v2.y));
            let tz = u(f(r0.z) * f(v2.z));
            let tw = u(f(r0.w) * f(v2.w));
            r2.x = tx;
            r2.y = ty;
            r2.z = tz;
            r2.w = tw;
        }
        {
            let tx = u(saturate(f(r2.x)));
            let ty = u(saturate(f(r2.y)));
            let tz = u(saturate(f(r2.z)));
            r2.x = tx;
            r2.y = ty;
            r2.z = tz;
        }
        r1.y = b(0.000000 < f(cb0[267u].x));
        r1.w = u(f(cb0[267u].x) * f(cb0[267u].x));
        r1.w = u(min(f(r1.w), 1.000000));
        {
            let tx = u(f(r2.x) * f(cb0[6u].x) + -f(r2.x));
            let ty = u(f(r2.y) * f(cb0[6u].y) + -f(r2.y));
            let tz = u(f(r2.z) * f(cb0[6u].z) + -f(r2.z));
            r3.x = tx;
            r3.y = ty;
            r3.z = tz;
        }
        {
            let tx = u(f(r1.w) * f(r3.x) + f(r2.x));
            let ty = u(f(r1.w) * f(r3.y) + f(r2.y));
            let tz = u(f(r1.w) * f(r3.z) + f(r2.z));
            r3.x = tx;
            r3.y = ty;
            r3.z = tz;
        }
        {
            let tx = u(max(f(r3.x), 0.000000));
            let ty = u(max(f(r3.y), 0.000000));
            let tz = u(max(f(r3.z), 0.000000));
            r3.x = tx;
            r3.y = ty;
            r3.z = tz;
        }
        r1.w = u(saturate(-f(cb0[267u].x)));
        {
            let tx = u(f(r2.x) * f(cb0[7u].x) + -f(r2.x));
            let ty = u(f(r2.y) * f(cb0[7u].y) + -f(r2.y));
            let tz = u(f(r2.z) * f(cb0[7u].z) + -f(r2.z));
            r4.x = tx;
            r4.y = ty;
            r4.z = tz;
        }
        {
            let tx = u(f(r1.w) * f(r4.x) + f(r2.x));
            let ty = u(f(r1.w) * f(r4.y) + f(r2.y));
            let tz = u(f(r1.w) * f(r4.z) + f(r2.z));
            r2.x = tx;
            r2.y = ty;
            r2.z = tz;
        }
        {
            let tx = u(max(f(r2.x), 0.000000));
            let ty = u(max(f(r2.y), 0.000000));
            let tz = u(max(f(r2.z), 0.000000));
            r2.x = tx;
            r2.y = ty;
            r2.z = tz;
        }
        {
            let tx = select(r2.x, r3.x, r1.y != 0u);
            let ty = select(r2.y, r3.y, r1.y != 0u);
            let tz = select(r2.z, r3.z, r1.y != 0u);
            o0.x = tx;
            o0.y = ty;
            o0.z = tz;
        }
        o0.w = r2.w;
    }
    r2 = bitcast<vec4<u32>>(textureSample(t1, s1, vec2<f32>(f(v0.x), f(v0.y))));
    r1.y = u(f(r2.x) * f(cb0[268u].x));
    r3 = bitcast<vec4<u32>>(textureSample(t2, s2, vec2<f32>(f(v0.x), f(v0.y))));
    r4 = bitcast<vec4<u32>>(textureSample(t3, s3, vec2<f32>(f(v0.x), f(v0.y))));
    r1.w = u(f(r2.x) + f(r2.x));
    r2.w = u(f(r2.x) * 2.000000 + (-1.000000));
    r1.z = u(f(r2.w) * 0.100000 + f(r1.z));
    r2.w = u(-f(r1.x) + 1.000000);
    r1.z = u(min(f(r1.z), f(r2.w)));
    r2.w = u(log2(f(cb0[266u].w)));
    r2.w = u(f(r2.w) * 0.700000);
    r2.w = u(pow(2.0, f(r2.w)));
    r2.w = u(-f(r2.w) + 1.000000);
    r1.z = u(min(f(r1.z), f(r2.w)));
    {
        let tx = u(-f(r1.z) + 0.250000);
        let ty = u(-f(r1.z) + 0.400000);
        let tz = u(-f(r1.z) + 1.000000);
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    r2.w = u(1.000000 / f(r5.x));
    r2.w = u(saturate(f(r2.w) * f(r5.y)));
    r5.x = u(f(r2.w) * (-2.000000) + 3.000000);
    r2.w = u(f(r2.w) * f(r2.w));
    r2.w = u(f(r2.w) * f(r5.x));
    r5.x = u(-f(r2.y) + f(r2.x));
    r2.y = u(f(r5.x) * 0.700000 + f(r2.y));
    {
        let tx = cb0[2u].x;
        let ty = cb0[2u].y;
        let tz = cb0[2u].z;
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
    }
    r6.w = r0.w;
    {
        let tx = u(-f(r6.x) + f(cb0[3u].x));
        let ty = u(-f(r6.y) + f(cb0[3u].y));
        let tz = u(-f(r6.z) + f(cb0[3u].z));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
    }
    r7.w = 0u;
    {
        let tx = u(f(r2.y) * f(r7.x) + f(r6.x));
        let ty = u(f(r2.y) * f(r7.y) + f(r6.y));
        let tz = u(f(r2.y) * f(r7.z) + f(r6.z));
        let tw = u(f(r2.y) * f(r7.w) + f(r6.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = u(-f(r7.x) + f(cb0[4u].x));
        let ty = u(-f(r7.y) + f(cb0[4u].y));
        let tz = u(-f(r7.z) + f(cb0[4u].z));
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
    }
    r8.w = u(f(r0.w) + -f(r7.w));
    {
        let tx = u(f(r2.w) * f(r8.x) + f(r7.x));
        let ty = u(f(r2.w) * f(r8.y) + f(r7.y));
        let tz = u(f(r2.w) * f(r8.z) + f(r7.z));
        let tw = u(f(r2.w) * f(r8.w) + f(r7.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = cb0[9u].x;
        let ty = cb0[9u].y;
        let tz = cb0[9u].z;
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
    }
    {
        let tx = 1036831949u;
        let ty = 0u;
        r5.x = tx;
        r5.y = ty;
    }
    r5.w = r6.w;
    {
        let tx = u(-f(r6.x) + f(r5.x));
        let ty = u(-f(r6.y) + f(r5.y));
        let tz = u(-f(r6.z) + f(r5.y));
        let tw = u(-f(r6.w) + f(r5.w));
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
        r8.w = tw;
    }
    {
        let tx = u(f(cb0[267u].w) * f(r8.x) + f(r6.x));
        let ty = u(f(cb0[267u].w) * f(r8.y) + f(r6.y));
        let tz = u(f(cb0[267u].w) * f(r8.z) + f(r6.z));
        let tw = u(f(cb0[267u].w) * f(r8.w) + f(r6.w));
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
        r6.w = tw;
    }
    {
        let tx = u(f(cb0[267u].w) * (-0.900000) + 1.000000);
        let ty = u(f(cb0[267u].w) * (-1.000000) + 1.000000);
        let tz = u(f(cb0[267u].w) * (-1.000000) + 1.000000);
        let tw = u(f(cb0[267u].w) * 0.000000 + 1.000000);
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
        r8.w = tw;
    }
    {
        let tx = u(f(r3.x) * f(r8.x));
        let ty = u(f(r3.y) * f(r8.y));
        let tz = u(f(r3.z) * f(r8.z));
        let tw = u(f(r3.w) * f(r8.w));
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
        r3.w = tw;
    }
    {
        let tx = 1028443341u;
        let ty = 0u;
        r5.x = tx;
        r5.y = ty;
    }
    r5.w = r7.w;
    {
        let tx = u(-f(r7.x) + f(r5.x));
        let ty = u(-f(r7.y) + f(r5.y));
        let tz = u(-f(r7.z) + f(r5.y));
        let tw = u(-f(r7.w) + f(r5.w));
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
        r8.w = tw;
    }
    {
        let tx = u(f(cb0[267u].w) * f(r8.x) + f(r7.x));
        let ty = u(f(cb0[267u].w) * f(r8.y) + f(r7.y));
        let tz = u(f(cb0[267u].w) * f(r8.z) + f(r7.z));
        let tw = u(f(cb0[267u].w) * f(r8.w) + f(r7.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    r2.y = u(1.000000 / -f(r1.z));
    r2.y = u(saturate(f(r2.y) * f(r5.z)));
    r2.w = u(f(r2.y) * (-2.000000) + 3.000000);
    r2.y = u(f(r2.y) * f(r2.y));
    r2.y = u(f(r2.y) * f(r2.w));
    {
        let tx = u(f(r6.x) + -f(r7.x));
        let ty = u(f(r6.y) + -f(r7.y));
        let tz = u(f(r6.z) + -f(r7.z));
        let tw = u(f(r6.w) + -f(r7.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = u(f(r2.y) * f(r5.x) + f(r7.x));
        let ty = u(f(r2.y) * f(r5.y) + f(r7.y));
        let tz = u(f(r2.y) * f(r5.z) + f(r7.z));
        let tw = u(f(r2.y) * f(r5.w) + f(r7.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let ty = u(f(r1.z) * 0.900000);
        let tw = u(f(r1.z) * 1.500000);
        r2.y = ty;
        r2.w = tw;
    }
    r7.x = u(f(r1.y) * 0.400000 + 0.400000);
    r2.y = b(f(r2.y) < f(r7.x));
    r7.x = b(0.460000 < f(r1.z));
    r1.w = select(r1.y, r1.w, r7.x != 0u);
    r7.x = u(f(r1.z) + 0.550000);
    r1.w = u(f(r1.w) * f(r7.x) + -f(cb0[2u].w));
    r1.w = u(saturate(f(r1.w) + 1.000000));
    {
        let tx = u(f(r0.x) + -f(r5.x));
        let ty = u(f(r0.y) + -f(r5.y));
        let tz = u(f(r0.z) + -f(r5.z));
        let tw = u(f(r0.w) + -f(r5.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = u(f(r1.w) * f(r7.x) + f(r5.x));
        let ty = u(f(r1.w) * f(r7.y) + f(r5.y));
        let tz = u(f(r1.w) * f(r7.z) + f(r5.z));
        let tw = u(f(r1.w) * f(r7.w) + f(r5.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = select(r0.x, r5.x, r2.y != 0u);
        let ty = select(r0.y, r5.y, r2.y != 0u);
        let tz = select(r0.z, r5.z, r2.y != 0u);
        let tw = select(r0.w, r5.w, r2.y != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r1.z = b(f(r1.z) < f(r1.y));
    r1.y = b(f(r2.w) < f(r1.y));
    {
        let tx = u(-f(r6.x) + 1.000000);
        let ty = u(-f(r6.y) + 1.000000);
        let tz = u(-f(r6.z) + 1.000000);
        let tw = u(-f(r6.w) + 1.000000);
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = u(f(r2.x) * f(r5.x) + f(r6.x));
        let ty = u(f(r2.x) * f(r5.y) + f(r6.y));
        let tz = u(f(r2.x) * f(r5.z) + f(r6.z));
        let tw = u(f(r2.x) * f(r5.w) + f(r6.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = u(f(r4.x) * f(r5.x));
        let ty = u(f(r4.y) * f(r5.y));
        let tz = u(f(r4.z) * f(r5.z));
        let tw = u(f(r4.w) * f(r5.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = select(r3.x, r5.x, r1.y != 0u);
        let ty = select(r3.y, r5.y, r1.y != 0u);
        let tz = select(r3.z, r5.z, r1.y != 0u);
        let tw = select(r3.w, r5.w, r1.y != 0u);
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
        r3.w = tw;
    }
    {
        let tx = select(r0.x, r3.x, r1.z != 0u);
        let ty = select(r0.y, r3.y, r1.z != 0u);
        let tz = select(r0.z, r3.z, r1.z != 0u);
        let tw = select(r0.w, r3.w, r1.z != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r1.y = b(f(r2.z) < f(r1.x));
    r1.z = b(0.000000 < f(r0.w));
    r1.y = r1.z & r1.y;
    r1.x = u(f(r1.x) * 0.100000);
    {
        let tx = u(f(r2.z) * 0.420000 + -f(r1.x));
        let tz = u(f(r2.z) * 0.400000 + -f(r1.x));
        let tw = u(f(r2.z) * 0.400000 + -f(r1.x));
        r1.x = tx;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = u(f(r0.x) * f(r1.x));
        let tz = u(f(r0.y) * f(r1.z));
        let tw = u(f(r0.z) * f(r1.w));
        r1.x = tx;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = select(r0.x, r1.x, r1.y != 0u);
        let ty = select(r0.y, r1.z, r1.y != 0u);
        let tz = select(r0.z, r1.w, r1.y != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    r1.x = u(f(r2.z) * f(cb0[267u].w));
    {
        let tx = u(f(r0.x) * f(cb0[5u].x) + -f(r0.x));
        let ty = u(f(r0.y) * f(cb0[5u].y) + -f(r0.y));
        let tz = u(f(r0.z) * f(cb0[5u].z) + -f(r0.z));
        let tw = u(f(r0.w) * f(cb0[5u].w) + -f(r0.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let tx = u(f(r1.x) * f(r2.x) + f(r0.x));
        let ty = u(f(r1.x) * f(r2.y) + f(r0.y));
        let tz = u(f(r1.x) * f(r2.z) + f(r0.z));
        let tw = u(f(r1.x) * f(r2.w) + f(r0.w));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r1.x = u(-f(cb0[267u].w) * 0.500000 + 1.000000);
    {
        let tx = u(f(r0.x) * f(r1.x));
        let ty = u(f(r0.y) * f(r1.x));
        let tz = u(f(r0.z) * f(r1.x));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    r1.x = u(f(r4.w) * f(r0.w));
    r1.x = u(f(r1.x) * f(cb0[267u].z));
    {
        let tx = u(f(cb1[0u].z) * 5.000000);
        let ty = u(f(cb1[0u].z) * 5.000000);
        let tz = u(f(cb1[0u].x) * 5.000000);
        let tw = u(f(cb1[0u].x) * 5.000000);
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let tx = b(f(r2.x) >= -f(r2.y));
        let ty = b(f(r2.y) >= -f(r2.y));
        let tz = b(f(r2.z) >= -f(r2.w));
        let tw = b(f(r2.w) >= -f(r2.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let tx = select(3231711232u, 1084227584u, r2.x != 0u);
        let ty = select(3192704205u, 1045220557u, r2.y != 0u);
        let tz = select(3231711232u, 1084227584u, r2.z != 0u);
        let tw = select(3192704205u, 1045220557u, r2.w != 0u);
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let ty = u(f(r2.y) * f(cb1[0u].z));
        let tz = u(f(r2.w) * f(cb1[0u].x));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let ty = u(fract(f(r1.y)));
        let tz = u(fract(f(r1.z)));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let ty = u(f(r1.y) * f(r2.x));
        let tz = u(f(r1.z) * f(r2.z));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let ty = u(f(r1.y) * 0.103100);
        let tz = u(f(r1.z) * 0.103100);
        r1.y = ty;
        r1.z = tz;
    }
    {
        let ty = u(fract(f(r1.y)));
        let tz = u(fract(f(r1.z)));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let tx = u(f(r1.y) + 33.330002);
        let ty = u(f(r1.z) + 33.330002);
        r2.x = tx;
        r2.y = ty;
    }
    {
        let ty = u(f(r1.y) * f(r2.x));
        let tz = u(f(r1.z) * f(r2.y));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let tx = u(f(r1.y) + f(r1.y));
        let ty = u(f(r1.z) + f(r1.z));
        r2.x = tx;
        r2.y = ty;
    }
    {
        let ty = u(f(r1.y) * f(r2.x));
        let tz = u(f(r1.z) * f(r2.y));
        r1.y = ty;
        r1.z = tz;
    }
    {
        let ty = u(fract(f(r1.y)));
        let tz = u(fract(f(r1.z)));
        r1.y = ty;
        r1.z = tz;
    }
    r1.x = u(f(r1.y) * f(r1.x));
    r1.x = b(f(r1.z) < f(r1.x));
    r1.x = r1.x & 1065353216u;
    {
        let tx = u(-f(r0.x) + f(cb0[8u].x));
        let ty = u(-f(r0.y) + f(cb0[8u].y));
        let tz = u(-f(r0.z) + f(cb0[8u].z));
        let tw = u(-f(r0.w) + f(cb0[8u].w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let tx = u(f(r1.x) * f(r2.x) + f(r0.x));
        let ty = u(f(r1.x) * f(r2.y) + f(r0.y));
        let tz = u(f(r1.x) * f(r2.z) + f(r0.z));
        let tw = u(f(r1.x) * f(r2.w) + f(r0.w));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r1.x = b(0.010000 < abs(f(cb0[267u].x)));
    {
        let ty = u(saturate(f(r0.x)));
        let tz = u(saturate(f(r0.y)));
        let tw = u(saturate(f(r0.z)));
        r1.y = ty;
        r1.z = tz;
        r1.w = tw;
    }
    r2.x = b(0.000000 < f(cb0[267u].x));
    r2.y = u(f(cb0[267u].x) * f(cb0[267u].x));
    r2.y = u(min(f(r2.y), 1.000000));
    {
        let tx = u(f(r1.y) * f(cb0[6u].x) + -f(r1.y));
        let ty = u(f(r1.z) * f(cb0[6u].y) + -f(r1.z));
        let tz = u(f(r1.w) * f(cb0[6u].z) + -f(r1.w));
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
    }
    {
        let ty = u(f(r2.y) * f(r3.x) + f(r1.y));
        let tz = u(f(r2.y) * f(r3.y) + f(r1.z));
        let tw = u(f(r2.y) * f(r3.z) + f(r1.w));
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let ty = u(max(f(r2.y), 0.000000));
        let tz = u(max(f(r2.z), 0.000000));
        let tw = u(max(f(r2.w), 0.000000));
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    r3.x = u(saturate(-f(cb0[267u].x)));
    {
        let ty = u(f(r1.y) * f(cb0[7u].x) + -f(r1.y));
        let tz = u(f(r1.z) * f(cb0[7u].y) + -f(r1.z));
        let tw = u(f(r1.w) * f(cb0[7u].z) + -f(r1.w));
        r3.y = ty;
        r3.z = tz;
        r3.w = tw;
    }
    {
        let ty = u(f(r3.x) * f(r3.y) + f(r1.y));
        let tz = u(f(r3.x) * f(r3.z) + f(r1.z));
        let tw = u(f(r3.x) * f(r3.w) + f(r1.w));
        r1.y = ty;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let ty = u(max(f(r1.y), 0.000000));
        let tz = u(max(f(r1.z), 0.000000));
        let tw = u(max(f(r1.w), 0.000000));
        r1.y = ty;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let ty = select(r1.y, r2.y, r2.x != 0u);
        let tz = select(r1.z, r2.z, r2.x != 0u);
        let tw = select(r1.w, r2.w, r2.x != 0u);
        r1.y = ty;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = select(r0.x, r1.y, r1.x != 0u);
        let ty = select(r0.y, r1.z, r1.x != 0u);
        let tz = select(r0.z, r1.w, r1.x != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    {
        let tx = u(f(r0.x) * f(v2.x));
        let ty = u(f(r0.y) * f(v2.y));
        let tz = u(f(r0.z) * f(v2.z));
        let tw = u(f(r0.w) * f(v2.w));
        o0.x = tx;
        o0.y = ty;
        o0.z = tz;
        o0.w = tw;
    }
    return bitcast<vec4<f32>>(o0);
}
