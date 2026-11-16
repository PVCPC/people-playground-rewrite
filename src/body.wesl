// Jaap/BodyShader's fragment stage, translated instruction by instruction from the game's D3D11 program. Registers hold raw bits, so
// masks and floats mix as they do in D3D.
// cb0 is the shader's $Globals, laid out as Unity's: see `Limb` in src/limb_material.rs.
// Then cb0[269] is _Time.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> cb0: array<vec4<u32>, 270>;
// The sprite's vertex colour, and how to get the mesh's vertex position back from uv (scale, offset).
struct Extra { tint: vec4<f32>, uv_to_local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var t0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var s0: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var t1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var s1: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var t2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var s2: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var t3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var s3: sampler;

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
    var r9 = vec4<u32>(0u);
    var r10 = vec4<u32>(0u);
    var r11 = vec4<u32>(0u);
    var r12 = vec4<u32>(0u);
    var r13 = vec4<u32>(0u);
    var r14 = vec4<u32>(0u);
    var r15 = vec4<u32>(0u);
    var r16 = vec4<u32>(0u);
    var r17 = vec4<u32>(0u);
    var r18 = vec4<u32>(0u);
    var r19 = vec4<u32>(0u);
    var r20 = vec4<u32>(0u);
    var r21 = vec4<u32>(0u);
    var r22 = vec4<u32>(0u);
    var o0 = vec4<u32>(0u);
    {
        let tx = u(f(v0.z) * 35.000000);
        let ty = u(f(v0.w) * 35.000000);
        r0.x = tx;
        r0.y = ty;
    }
    {
        let tx = u(round(f(r0.x)));
        let ty = u(round(f(r0.y)));
        r0.x = tx;
        r0.y = ty;
    }
    r1 = bitcast<vec4<u32>>(textureSample(t0, s0, vec2<f32>(f(v0.x), f(v0.y))));
    r0.z = u(f(r1.w) + (-0.010000));
    r0.z = b(f(r0.z) < 0.000000);
    if r0.z != 0u { discard; }
    r2 = bitcast<vec4<u32>>(textureSample(t1, s1, vec2<f32>(f(v0.x), f(v0.y))));
    r3 = bitcast<vec4<u32>>(textureSample(t2, s2, vec2<f32>(f(v0.x), f(v0.y))));
    r4 = bitcast<vec4<u32>>(textureSample(t3, s3, vec2<f32>(f(v0.x), f(v0.y))));
    r0.z = u(f(r4.z) * 0.900000);
    {
        let tx = u(f(r4.z) * 0.200000 + 1.000000);
        let ty = u(f(r4.w) * 0.100000 + 0.434000);
        let tz = u(f(r4.w) * 0.100000 + 0.084000);
        let tw = u(f(r4.w) * 0.100000 + 0.091000);
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    r0.w = u(f(r5.x) * 0.500000);
    r4.y = u(-f(r5.x) * 0.500000 + 1.000000);
    r4.y = u(f(r4.y) + f(r4.y));
    r5.x = b(f(r5.x) < 1.000000);
    r5.x = r5.x & 1065353216u;
    {
        let tx = u(-f(r5.y) + f(cb0[3u].x));
        let ty = u(-f(r5.z) + f(cb0[3u].y));
        let tz = u(-f(r5.w) + f(cb0[3u].z));
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
    }
    {
        let ty = u(f(r6.x) * 0.200000 + f(r5.y));
        let tz = u(f(r6.y) * 0.200000 + f(r5.z));
        let tw = u(f(r6.z) * 0.200000 + f(r5.w));
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = u(f(r4.x) * f(r4.z));
        let ty = u(f(r4.z) * f(r4.z));
        r6.x = tx;
        r6.y = ty;
    }
    r6.z = u(f(r4.w) * f(r4.w) + 0.300000);
    {
        let tx = u(-f(cb0[2u].x) + f(cb0[3u].x));
        let ty = u(-f(cb0[2u].y) + f(cb0[3u].y));
        let tz = u(-f(cb0[2u].z) + f(cb0[3u].z));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
    }
    r6.w = u(f(r6.x) * 0.400000);
    {
        let tx = u(-f(cb0[9u].x) + 1.000000);
        let ty = u(-f(cb0[9u].y) + 1.000000);
        let tz = u(-f(cb0[9u].z) + 1.000000);
        let tw = u(-f(cb0[9u].w) + 1.000000);
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
        r8.w = tw;
    }
    {
        let tx = u(-f(cb0[2u].x) + f(cb0[3u].x));
        let ty = u(-f(cb0[2u].y) + f(cb0[3u].y));
        let tz = u(-f(cb0[2u].z) + f(cb0[3u].z));
        let tw = u(-f(cb0[2u].w) + f(cb0[3u].w));
        r9.x = tx;
        r9.y = ty;
        r9.z = tz;
        r9.w = tw;
    }
    {
        let tx = u(f(r4.w) * f(r9.x) + f(cb0[2u].x));
        let ty = u(f(r4.w) * f(r9.y) + f(cb0[2u].y));
        let tz = u(f(r4.w) * f(r9.z) + f(cb0[2u].z));
        let tw = u(f(r4.w) * f(r9.w) + f(cb0[2u].w));
        r9.x = tx;
        r9.y = ty;
        r9.z = tz;
        r9.w = tw;
    }
    {
        let tx = u(f(r4.x) + 0.100000);
        let ty = u(f(r4.w) + 0.500000);
        r10.x = tx;
        r10.y = ty;
    }
    r7.w = u(min(f(r10.x), 0.900000));
    r10.x = u(-f(r7.w) + 1.000000);
    r10.x = u(1.000000 / f(r10.x));
    r0.z = u(min(f(r0.z), 1.000000));
    r10.z = u(-f(r0.z) + 1.000000);
    r10.z = u(1.000000 / f(r10.z));
    r10.w = u(f(r4.w) * 2.600000 + 0.500000);
    {
        let tx = u(-f(cb0[3u].x) + 1.000000);
        let ty = u(-f(cb0[3u].y) + 1.000000);
        let tz = u(-f(cb0[3u].z) + 1.000000);
        let tw = u(-f(cb0[3u].w) + 1.000000);
        r11.x = tx;
        r11.y = ty;
        r11.z = tz;
        r11.w = tw;
    }
    {
        let tx = u(f(r11.x) * 0.200000 + f(cb0[3u].x));
        let ty = u(f(r11.y) * 0.200000 + f(cb0[3u].y));
        let tz = u(f(r11.z) * 0.200000 + f(cb0[3u].z));
        let tw = u(f(r11.w) * 0.200000 + f(cb0[3u].w));
        r11.x = tx;
        r11.y = ty;
        r11.z = tz;
        r11.w = tw;
    }
    {
        let tx = u(f(r3.x) * f(r11.x));
        let ty = u(f(r3.y) * f(r11.y));
        let tz = u(f(r3.z) * f(r11.z));
        let tw = u(f(r3.w) * f(r11.w));
        r11.x = tx;
        r11.y = ty;
        r11.z = tz;
        r11.w = tw;
    }
    r12.w = 1065353216u;
    r13.z = r2.z;
    {
        let tx = r1.x;
        let ty = r1.y;
        let tz = r1.z;
        let tw = r1.w;
        r14.x = tx;
        r14.y = ty;
        r14.z = tz;
        r14.w = tw;
    }
    r13.w = 0u;
    loop {
        r15.x = b(bitcast<i32>(r13.w) >= bitcast<i32>(cb0[265u].y));
        if r15.x != 0u { break; }
        r15.x = 2139095040u & cb0[r13.w + 10u].z;
        r15.x = b(r15.x == 2139095040u);
        r15.y = u(max(0.000000, f(cb0[r13.w + 10u].z)));
        r15.y = u(min(f(r15.y), 100.000000));
        r15.x = select(r15.y, 0u, r15.x != 0u);
        r15.y = u(floor(f(cb0[r13.w + 10u].w)));
        r15.y = bitcast<u32>(i32(f(r15.y)));
        r15.z = u(f(cb1[0u].y) + -f(cb0[r13.w + 138u].x));
        {
            let tx = b(r15.y == 5u);
            let ty = b(r15.y == 4u);
            let tz = b(r15.y == 2u);
            let tw = b(r15.y == 3u);
            r16.x = tx;
            r16.y = ty;
            r16.z = tz;
            r16.w = tw;
        }
        r15.w = b(0.000000 >= f(r15.x));
        r15.w = r15.w | r16.x;
        {
            let tx = u(f(r0.x) * 0.028571 + -f(cb0[r13.w + 10u].x));
            let ty = u(f(r0.y) * 0.028571 + -f(cb0[r13.w + 10u].y));
            r17.x = tx;
            r17.y = ty;
        }
        r16.x = u(f(r17.x) * f(r17.x) + f(r17.y) * f(r17.y));
        r16.x = u(sqrt(f(r16.x)));
        r15.w = select(r16.x, 1203982336u, r15.w != 0u);
        {
            let tx = u(f(r15.x) * 100.000000);
            let ty = u(f(r15.x) * 0.040000);
            r17.x = tx;
            r17.y = ty;
        }
        r16.x = u(min(f(r17.x), 8.000000));
        r17.x = u(-f(r15.w) + 1.000000);
        r17.x = u(-f(r4.y) * f(r17.x) + 1.000000);
        r17.z = u(f(r0.w) * f(r15.w) + -f(r17.x));
        r17.x = u(f(r5.x) * f(r17.z) + f(r17.x));
        r16.x = u(f(r16.x) / f(r17.x));
        {
            let tx = u(f(r16.x) + (-20.000000));
            let tz = u(f(r16.x) + (-40.000000));
            let tw = u(f(r16.x) + (-48.000000));
            r17.x = tx;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(saturate(f(r17.x) * 0.031250));
            let tz = u(saturate(f(r17.z) * 0.083333));
            let tw = u(saturate(f(r17.w) * 0.055556));
            r17.x = tx;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(r17.x) * (-2.000000) + 3.000000);
            let ty = u(f(r17.z) * (-2.000000) + 3.000000);
            let tz = u(f(r17.w) * (-2.000000) + 3.000000);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r17.x) * f(r17.x));
            let tz = u(f(r17.z) * f(r17.z));
            let tw = u(f(r17.w) * f(r17.w));
            r17.x = tx;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(r17.x) * f(r18.x));
            let tz = u(f(r17.z) * f(r18.y));
            let tw = u(f(r17.w) * f(r18.z));
            r17.x = tx;
            r17.z = tz;
            r17.w = tw;
        }
        r16.x = u(f(r4.w) * f(r17.x));
        {
            let tx = u(-f(r14.x) + f(cb0[9u].x));
            let ty = u(-f(r14.y) + f(cb0[9u].y));
            let tz = u(-f(r14.z) + f(cb0[9u].z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r16.x) * f(r18.x) + f(r14.x));
            let ty = u(f(r16.x) * f(r18.y) + f(r14.y));
            let tz = u(f(r16.x) * f(r18.z) + f(r14.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(cb0[3u].x) * 0.500000 + -f(r18.x));
            let ty = u(f(cb0[3u].y) * 0.500000 + -f(r18.y));
            let tz = u(f(cb0[3u].z) * 0.500000 + -f(r18.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        {
            let tx = u(f(r17.z) * f(r19.x) + f(r18.x));
            let ty = u(f(r17.z) * f(r19.y) + f(r18.y));
            let tz = u(f(r17.z) * f(r19.z) + f(r18.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r5.y) + -f(r18.x));
            let ty = u(f(r5.z) + -f(r18.y));
            let tz = u(f(r5.w) + -f(r18.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        {
            let tx = u(f(r17.w) * f(r19.x) + f(r18.x));
            let ty = u(f(r17.w) * f(r19.y) + f(r18.y));
            let tz = u(f(r17.w) * f(r19.z) + f(r18.z));
            r12.x = tx;
            r12.y = ty;
            r12.z = tz;
        }
        {
            let tx = u(min(f(r12.x), f(r14.x)));
            let ty = u(min(f(r12.y), f(r14.y)));
            let tz = u(min(f(r12.z), f(r14.z)));
            let tw = u(min(f(r12.w), f(r14.w)));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
            r18.w = tw;
        }
        {
            let tx = select(r14.x, r18.x, r16.y != 0u);
            let ty = select(r14.y, r18.y, r16.y != 0u);
            let tz = select(r14.z, r18.z, r16.y != 0u);
            let tw = select(r14.w, r18.w, r16.y != 0u);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
            r18.w = tw;
        }
        r12.x = u(f(r17.y) / f(r15.w));
        r12.y = u(min(f(r12.x), 1.000000));
        r12.z = u(f(r12.y) * (-2.000000) + 3.000000);
        r12.y = u(f(r12.y) * f(r12.y));
        r12.y = u(f(r12.y) * f(r12.z));
        r12.y = u(f(r6.z) * f(r12.y));
        {
            let tx = u(f(r12.y) * f(r4.z) + (-0.300000));
            let ty = u(f(r12.y) * f(r4.x) + (-0.100000));
            r16.x = tx;
            r16.y = ty;
        }
        {
            let tx = u(saturate(f(r16.x) * 1.666667));
            let ty = u(saturate(f(r16.y) * (-10.000000)));
            r16.x = tx;
            r16.y = ty;
        }
        {
            let tx = u(f(r16.x) * (-2.000000) + 3.000000);
            let ty = u(f(r16.y) * (-2.000000) + 3.000000);
            r17.x = tx;
            r17.y = ty;
        }
        {
            let tx = u(f(r16.x) * f(r16.x));
            let ty = u(f(r16.y) * f(r16.y));
            r16.x = tx;
            r16.y = ty;
        }
        {
            let tx = u(f(r16.x) * f(r17.x));
            let ty = u(f(r16.y) * f(r17.y));
            r16.x = tx;
            r16.y = ty;
        }
        {
            let tx = u(f(r16.x) * f(r7.x) + f(cb0[2u].x));
            let ty = u(f(r16.x) * f(r7.y) + f(cb0[2u].y));
            let tz = u(f(r16.x) * f(r7.z) + f(cb0[2u].z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        {
            let tx = u(f(cb0[4u].x) * f(r12.y) + -f(r17.x));
            let ty = u(f(cb0[4u].y) * f(r12.y) + -f(r17.y));
            let tz = u(f(cb0[4u].z) * f(r12.y) + -f(r17.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        {
            let tx = u(f(r16.y) * f(r19.x) + f(r17.x));
            let ty = u(f(r16.y) * f(r19.y) + f(r17.y));
            let tz = u(f(r16.y) * f(r19.z) + f(r17.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        {
            let tx = u(-f(r18.x) + f(r17.x));
            let ty = u(-f(r18.y) + f(r17.y));
            let tz = u(-f(r18.z) + f(r17.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        {
            let tx = u(f(r12.y) * f(r19.x) + f(r18.x));
            let ty = u(f(r12.y) * f(r19.y) + f(r18.y));
            let tz = u(f(r12.y) * f(r19.z) + f(r18.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        r12.x = u(sqrt(f(r12.x)));
        r12.x = u(f(r6.w) * f(r12.x));
        r12.x = u(f(r12.x) * 4.000000);
        r12.x = u(min(f(r12.x), 0.500000));
        r19.w = r18.w;
        {
            let tx = u(-f(r19.x) + 1.000000);
            let ty = u(-f(r19.y) + 1.000000);
            let tz = u(-f(r19.z) + 1.000000);
            let tw = u(-f(r19.w) + 1.000000);
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        {
            let tx = u(f(r8.x) * f(r20.x));
            let ty = u(f(r8.y) * f(r20.y));
            let tz = u(f(r8.z) * f(r20.z));
            let tw = u(f(r8.w) * f(r20.w));
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        {
            let tx = u(-f(r20.x) * 2.000000 + 1.000000);
            let ty = u(-f(r20.y) * 2.000000 + 1.000000);
            let tz = u(-f(r20.z) * 2.000000 + 1.000000);
            let tw = u(-f(r20.w) * 2.000000 + 1.000000);
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        r12.z = u(f(r19.y) + f(r19.x));
        r12.z = u(f(r19.z) + f(r12.z));
        r12.z = b(f(r12.z) < 0.500000);
        r12.z = r12.z & 1065353216u;
        {
            let tx = u(f(r19.x) * f(cb0[9u].x) + -f(r20.x));
            let ty = u(f(r19.y) * f(cb0[9u].y) + -f(r20.y));
            let tz = u(f(r19.z) * f(cb0[9u].z) + -f(r20.z));
            let tw = u(f(r19.w) * f(cb0[9u].w) + -f(r20.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r12.z) * f(r21.x) + f(r20.x));
            let ty = u(f(r12.z) * f(r21.y) + f(r20.y));
            let tz = u(f(r12.z) * f(r21.z) + f(r20.z));
            let tw = u(f(r12.z) * f(r21.w) + f(r20.w));
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        {
            let tx = u(f(r12.x) + (-0.150000));
            let tz = u(f(r12.x) + (-0.250000));
            r12.x = tx;
            r12.z = tz;
        }
        {
            let tx = u(f(r12.z) * 19.999996);
            let tz = u(f(r12.x) * 2.857143);
            r12.x = tx;
            r12.z = tz;
        }
        r12.z = u(max(f(r12.z), 0.000000));
        r16.x = u(f(r12.z) * (-2.000000) + 3.000000);
        r12.z = u(f(r12.z) * f(r12.z));
        r12.z = u(f(r12.z) * f(r16.x));
        {
            let tx = u(-f(r19.x) + f(r20.x));
            let ty = u(-f(r19.y) + f(r20.y));
            let tz = u(-f(r19.z) + f(r20.z));
            let tw = u(-f(r19.w) + f(r20.w));
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        {
            let tx = u(f(r12.z) * f(r20.x) + f(r19.x));
            let ty = u(f(r12.z) * f(r20.y) + f(r19.y));
            let tz = u(f(r12.z) * f(r20.z) + f(r19.z));
            let tw = u(f(r12.z) * f(r20.w) + f(r19.w));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
            r19.w = tw;
        }
        r12.x = u(saturate(f(r12.x)));
        r16.x = u(f(r12.x) * (-2.000000) + 3.000000);
        r12.x = u(f(r12.x) * f(r12.x));
        r12.x = u(f(r12.x) * f(r16.x));
        {
            let tx = u(f(r2.x) + -f(r19.x));
            let ty = u(f(r2.y) + -f(r19.y));
            let tz = u(f(r2.z) + -f(r19.z));
            let tw = u(f(r2.w) + -f(r19.w));
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        {
            let tx = u(f(r12.x) * f(r20.x) + f(r19.x));
            let ty = u(f(r12.x) * f(r20.y) + f(r19.y));
            let tz = u(f(r12.x) * f(r20.z) + f(r19.z));
            let tw = u(f(r12.x) * f(r20.w) + f(r19.w));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
            r19.w = tw;
        }
        {
            let tx = select(r19.x, r18.x, r15.y != 0u);
            let ty = select(r19.y, r18.y, r15.y != 0u);
            let tz = select(r19.z, r18.z, r15.y != 0u);
            let tw = select(r19.w, r18.w, r15.y != 0u);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
            r18.w = tw;
        }
        r16.x = u(f(r18.w) + (-0.500000));
        r16.x = b(f(r16.x) < 0.000000);
        if r16.x != 0u { discard; }
        {
            let tx = u(min(f(r15.x), 2.500000));
            let ty = u(min(f(r15.x), 15.000000));
            let tz = u(min(f(r15.x), 10.000000));
            let tw = u(min(f(r15.x), 10.000000));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
            r19.w = tw;
        }
        {
            let tx = u(f(r19.x) * 0.040000);
            let ty = u(f(r19.y) * 0.030000);
            let tz = u(f(r19.z) * 10.000000);
            let tw = u(f(r19.w) * 0.160000);
            r20.x = tx;
            r20.y = ty;
            r20.z = tz;
            r20.w = tw;
        }
        r15.x = u(max(f(r15.w), 0.010000));
        r15.x = u(f(r20.x) / f(r15.x));
        r15.x = u(min(f(r15.x), 1.000000));
        r16.x = u(f(r15.x) * f(r15.x));
        {
            let tx = u(f(r9.x) + -f(r18.x));
            let ty = u(f(r9.y) + -f(r18.y));
            let tz = u(f(r9.z) + -f(r18.z));
            let tw = u(f(r9.w) + -f(r18.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r16.x) * f(r21.x) + f(r18.x));
            let ty = u(f(r16.x) * f(r21.y) + f(r18.y));
            let tz = u(f(r16.x) * f(r21.z) + f(r18.z));
            let tw = u(f(r16.x) * f(r21.w) + f(r18.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        r15.x = u(f(r15.x) + (-0.950000));
        r15.x = u(f(r15.x) * 19.999996);
        r15.x = u(max(f(r15.x), 0.000000));
        r16.x = u(f(r15.x) * (-2.000000) + 3.000000);
        r15.x = u(f(r15.x) * f(r15.x));
        r15.x = u(f(r15.x) * f(r16.x));
        {
            let tx = u(f(r2.x) + -f(r21.x));
            let ty = u(f(r2.y) + -f(r21.y));
            let tz = u(f(r2.z) + -f(r21.z));
            let tw = u(f(r2.w) + -f(r21.w));
            r22.x = tx;
            r22.y = ty;
            r22.z = tz;
            r22.w = tw;
        }
        {
            let tx = u(f(r15.x) * f(r22.x) + f(r21.x));
            let ty = u(f(r15.x) * f(r22.y) + f(r21.y));
            let tz = u(f(r15.x) * f(r22.z) + f(r21.z));
            let tw = u(f(r15.x) * f(r22.w) + f(r21.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = select(r18.x, r21.x, r16.z != 0u);
            let ty = select(r18.y, r21.y, r16.z != 0u);
            let tz = select(r18.z, r21.z, r16.z != 0u);
            let tw = select(r18.w, r21.w, r16.z != 0u);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
            r18.w = tw;
        }
        r15.x = u(f(r18.w) + (-0.500000));
        r15.x = b(f(r15.x) < 0.000000);
        if r15.x != 0u { discard; }
        {
            let tx = u(f(r17.x) + -f(r18.x));
            let ty = u(f(r17.y) + -f(r18.y));
            let tz = u(f(r17.z) + -f(r18.z));
            r16.x = tx;
            r16.y = ty;
            r16.z = tz;
        }
        {
            let tx = u(f(r12.y) * f(r16.x) + f(r18.x));
            let ty = u(f(r12.y) * f(r16.y) + f(r18.y));
            let tz = u(f(r12.y) * f(r16.z) + f(r18.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        r17.w = r18.w;
        {
            let tx = u(-f(r17.x) + 1.000000);
            let ty = u(-f(r17.y) + 1.000000);
            let tz = u(-f(r17.z) + 1.000000);
            let tw = u(-f(r17.w) + 1.000000);
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r8.x) * f(r21.x));
            let ty = u(f(r8.y) * f(r21.y));
            let tz = u(f(r8.z) * f(r21.z));
            let tw = u(f(r8.w) * f(r21.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(-f(r21.x) * 2.000000 + 1.000000);
            let ty = u(-f(r21.y) * 2.000000 + 1.000000);
            let tz = u(-f(r21.z) * 2.000000 + 1.000000);
            let tw = u(-f(r21.w) * 2.000000 + 1.000000);
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        r12.y = u(f(r17.y) + f(r17.x));
        r12.y = u(f(r17.z) + f(r12.y));
        r12.y = b(f(r12.y) < 0.500000);
        r12.y = r12.y & 1065353216u;
        {
            let tx = u(f(r17.x) * f(cb0[9u].x) + -f(r21.x));
            let ty = u(f(r17.y) * f(cb0[9u].y) + -f(r21.y));
            let tz = u(f(r17.z) * f(cb0[9u].z) + -f(r21.z));
            let tw = u(f(r17.w) * f(cb0[9u].w) + -f(r21.w));
            r22.x = tx;
            r22.y = ty;
            r22.z = tz;
            r22.w = tw;
        }
        {
            let tx = u(f(r12.y) * f(r22.x) + f(r21.x));
            let ty = u(f(r12.y) * f(r22.y) + f(r21.y));
            let tz = u(f(r12.y) * f(r22.z) + f(r21.z));
            let tw = u(f(r12.y) * f(r22.w) + f(r21.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(-f(r17.x) + f(r21.x));
            let ty = u(-f(r17.y) + f(r21.y));
            let tz = u(-f(r17.z) + f(r21.z));
            let tw = u(-f(r17.w) + f(r21.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r12.z) * f(r21.x) + f(r17.x));
            let ty = u(f(r12.z) * f(r21.y) + f(r17.y));
            let tz = u(f(r12.z) * f(r21.z) + f(r17.z));
            let tw = u(f(r12.z) * f(r21.w) + f(r17.w));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(r2.x) + -f(r17.x));
            let ty = u(f(r2.y) + -f(r17.y));
            let tz = u(f(r2.z) + -f(r17.z));
            let tw = u(f(r2.w) + -f(r17.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r12.x) * f(r21.x) + f(r17.x));
            let ty = u(f(r12.x) * f(r21.y) + f(r17.y));
            let tz = u(f(r12.x) * f(r21.z) + f(r17.z));
            let tw = u(f(r12.x) * f(r21.w) + f(r17.w));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
            r17.w = tw;
        }
        r12.x = u(f(r20.y) / f(r15.w));
        r12.y = u(-f(r12.x) * f(r4.z) + 1.000000);
        r12.y = b(f(r12.y) < 0.000000);
        r12.z = u(-f(r7.w) + f(r12.x));
        r12.z = u(saturate(f(r10.x) * f(r12.z)));
        r15.x = u(f(r12.z) * (-2.000000) + 3.000000);
        r12.z = u(f(r12.z) * f(r12.z));
        r12.z = u(f(r12.z) * f(r15.x));
        {
            let tx = u(f(cb0[9u].x) * 0.800000 + -f(r17.x));
            let ty = u(f(cb0[9u].y) * 0.800000 + -f(r17.y));
            let tz = u(f(cb0[9u].z) * 0.800000 + -f(r17.z));
            r16.x = tx;
            r16.y = ty;
            r16.z = tz;
        }
        {
            let tx = u(f(r12.z) * f(r16.x) + f(r17.x));
            let ty = u(f(r12.z) * f(r16.y) + f(r17.y));
            let tz = u(f(r12.z) * f(r16.z) + f(r17.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        r12.x = u(f(r12.x) + (-0.850000));
        r12.x = u(saturate(f(r12.x) * 6.666668));
        r12.z = u(f(r12.x) * (-2.000000) + 3.000000);
        r12.x = u(f(r12.x) * f(r12.x));
        r12.x = u(f(r12.x) * f(r12.z));
        {
            let tx = u(f(r2.x) + -f(r17.x));
            let ty = u(f(r2.y) + -f(r17.y));
            let tz = u(f(r2.z) + -f(r17.z));
            let tw = u(f(r2.w) + -f(r17.w));
            r21.x = tx;
            r21.y = ty;
            r21.z = tz;
            r21.w = tw;
        }
        {
            let tx = u(f(r12.x) * f(r21.x) + f(r17.x));
            let ty = u(f(r12.x) * f(r21.y) + f(r17.y));
            let tz = u(f(r12.x) * f(r21.z) + f(r17.z));
            let tw = u(f(r12.x) * f(r21.w) + f(r17.w));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = select(r17.x, r3.x, r12.y != 0u);
            let ty = select(r17.y, r3.y, r12.y != 0u);
            let tz = select(r17.z, r3.z, r12.y != 0u);
            let tw = select(r17.w, r3.w, r12.y != 0u);
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = select(r18.x, r17.x, r16.w != 0u);
            let ty = select(r18.y, r17.y, r16.w != 0u);
            let tz = select(r18.z, r17.z, r16.w != 0u);
            let tw = select(r18.w, r17.w, r16.w != 0u);
            r16.x = tx;
            r16.y = ty;
            r16.z = tz;
            r16.w = tw;
        }
        r12.x = u(f(r16.w) + (-0.500000));
        r12.x = b(f(r12.x) < 0.000000);
        if r12.x != 0u { discard; }
        r12.x = b(r15.y == 1u);
        r12.y = u(min(f(r20.z), 1.000000));
        r12.z = u(f(r12.y) * f(r15.z));
        {
            let tx = u(saturate(f(r12.z) * 0.010000));
            let ty = u(saturate(f(r12.z) * 0.100000));
            r15.x = tx;
            r15.y = ty;
        }
        r12.z = u(f(r15.x) * 0.500000 + f(r20.w));
        r15.x = u(f(r15.y) + 1.000000);
        {
            let tx = u(f(r2.x) / f(r15.x));
            let ty = u(f(r2.y) / f(r15.x));
            r13.x = tx;
            r13.y = ty;
        }
        r12.z = u(f(r12.z) * 0.040000);
        r12.z = u(f(r12.z) / f(r15.w));
        r15.x = u(min(f(r12.z), 1.000000));
        r15.y = u(f(r15.x) * (-2.000000) + 3.000000);
        r15.x = u(f(r15.x) * f(r15.x));
        r15.x = u(f(r15.x) * f(r15.y));
        r15.x = u(f(r6.z) * f(r15.x));
        {
            let tx = u(f(r15.x) * f(r4.z) + (-0.300000));
            let ty = u(f(r15.x) * f(r4.x) + (-0.100000));
            r17.x = tx;
            r17.y = ty;
        }
        {
            let tx = u(saturate(f(r17.x) * 1.666667));
            let ty = u(saturate(f(r17.y) * (-10.000000)));
            r17.x = tx;
            r17.y = ty;
        }
        {
            let tz = u(f(r17.x) * (-2.000000) + 3.000000);
            let tw = u(f(r17.y) * (-2.000000) + 3.000000);
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(r17.x) * f(r17.x));
            let ty = u(f(r17.y) * f(r17.y));
            r17.x = tx;
            r17.y = ty;
        }
        {
            let tx = u(f(r17.x) * f(r17.z));
            let ty = u(f(r17.y) * f(r17.w));
            r17.x = tx;
            r17.y = ty;
        }
        {
            let tx = u(f(r17.x) * f(r7.x) + f(cb0[2u].x));
            let tz = u(f(r17.x) * f(r7.y) + f(cb0[2u].y));
            let tw = u(f(r17.x) * f(r7.z) + f(cb0[2u].z));
            r17.x = tx;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(cb0[4u].x) * f(r15.x) + -f(r17.x));
            let ty = u(f(cb0[4u].y) * f(r15.x) + -f(r17.z));
            let tz = u(f(cb0[4u].z) * f(r15.x) + -f(r17.w));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r17.y) * f(r18.x) + f(r17.x));
            let ty = u(f(r17.y) * f(r18.y) + f(r17.z));
            let tz = u(f(r17.y) * f(r18.z) + f(r17.w));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        {
            let tx = u(-f(r16.x) + f(r17.x));
            let ty = u(-f(r16.y) + f(r17.y));
            let tz = u(-f(r16.z) + f(r17.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        {
            let tx = u(f(r15.x) * f(r17.x) + f(r16.x));
            let ty = u(f(r15.x) * f(r17.y) + f(r16.y));
            let tz = u(f(r15.x) * f(r17.z) + f(r16.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        r12.z = u(sqrt(f(r12.z)));
        r12.z = u(f(r6.w) * f(r12.z));
        r12.z = u(f(r12.z) * 4.000000);
        r12.z = u(min(f(r12.z), 0.500000));
        {
            let tx = u(-f(r17.x) + 1.000000);
            let ty = u(-f(r17.y) + 1.000000);
            let tz = u(-f(r17.z) + 1.000000);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r8.x) * f(r18.x));
            let ty = u(f(r8.y) * f(r18.y));
            let tz = u(f(r8.z) * f(r18.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(-f(r18.x) * 2.000000 + 1.000000);
            let ty = u(-f(r18.y) * 2.000000 + 1.000000);
            let tz = u(-f(r18.z) * 2.000000 + 1.000000);
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        r15.x = u(f(r17.y) + f(r17.x));
        r15.x = u(f(r17.z) + f(r15.x));
        r15.x = b(f(r15.x) < 0.500000);
        r15.x = r15.x & 1065353216u;
        {
            let tx = u(f(r17.x) * f(cb0[9u].x) + -f(r18.x));
            let ty = u(f(r17.y) * f(cb0[9u].y) + -f(r18.y));
            let tz = u(f(r17.z) * f(cb0[9u].z) + -f(r18.z));
            r19.x = tx;
            r19.y = ty;
            r19.z = tz;
        }
        {
            let tx = u(f(r15.x) * f(r19.x) + f(r18.x));
            let ty = u(f(r15.x) * f(r19.y) + f(r18.y));
            let tz = u(f(r15.x) * f(r19.z) + f(r18.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r12.z) + (-0.150000));
            let ty = u(f(r12.z) + (-0.250000));
            r15.x = tx;
            r15.y = ty;
        }
        {
            let tx = u(f(r15.y) * 19.999996);
            let ty = u(f(r15.x) * 2.857143);
            r15.x = tx;
            r15.y = ty;
        }
        r12.z = u(max(f(r15.y), 0.000000));
        r15.y = u(f(r12.z) * (-2.000000) + 3.000000);
        r12.z = u(f(r12.z) * f(r12.z));
        r12.z = u(f(r12.z) * f(r15.y));
        {
            let tx = u(-f(r17.x) + f(r18.x));
            let ty = u(-f(r17.y) + f(r18.y));
            let tz = u(-f(r17.z) + f(r18.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r12.z) * f(r18.x) + f(r17.x));
            let ty = u(f(r12.z) * f(r18.y) + f(r17.y));
            let tz = u(f(r12.z) * f(r18.z) + f(r17.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        r15.x = u(saturate(f(r15.x)));
        r12.z = u(f(r15.x) * (-2.000000) + 3.000000);
        r15.x = u(f(r15.x) * f(r15.x));
        r12.z = u(f(r12.z) * f(r15.x));
        {
            let tx = u(f(r13.x) + -f(r17.x));
            let ty = u(f(r13.y) + -f(r17.y));
            let tz = u(f(r13.z) + -f(r17.z));
            r18.x = tx;
            r18.y = ty;
            r18.z = tz;
        }
        {
            let tx = u(f(r12.z) * f(r18.x) + f(r17.x));
            let ty = u(f(r12.z) * f(r18.y) + f(r17.y));
            let tz = u(f(r12.z) * f(r18.z) + f(r17.z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        r12.y = u(f(r15.z) * f(r12.y) + (-1.000000));
        r12.y = u(saturate(f(r12.y) * 0.100000 + 0.200000));
        {
            let tx = u(-f(r16.x) + f(r17.x));
            let ty = u(-f(r16.y) + f(r17.y));
            let tz = u(-f(r16.z) + f(r17.z));
            r15.x = tx;
            r15.y = ty;
            r15.z = tz;
        }
        {
            let tx = u(f(r12.y) * f(r15.x) + f(r16.x));
            let ty = u(f(r12.y) * f(r15.y) + f(r16.y));
            let tz = u(f(r12.y) * f(r15.z) + f(r16.z));
            r15.x = tx;
            r15.y = ty;
            r15.z = tz;
        }
        r12.y = u(log2(f(r19.w)));
        r12.y = u(f(r12.y) * 1.400000);
        r12.y = u(pow(2.0, f(r12.y)));
        r12.y = u(f(r12.y) * 0.003500);
        r12.y = u(f(r12.y) / f(r15.w));
        r12.z = u(-f(r0.z) + f(r12.y));
        r12.z = u(saturate(f(r10.z) * f(r12.z)));
        r13.x = u(f(r12.z) * (-2.000000) + 3.000000);
        r12.z = u(f(r12.z) * f(r12.z));
        r12.z = u(f(r12.z) * f(r13.x));
        {
            let tx = u(-f(r15.x) + f(cb0[9u].x));
            let ty = u(-f(r15.y) + f(cb0[9u].y));
            let tz = u(-f(r15.z) + f(cb0[9u].z));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
        }
        {
            let tx = u(f(r12.z) * f(r17.x) + f(r15.x));
            let ty = u(f(r12.z) * f(r17.y) + f(r15.y));
            let tz = u(f(r12.z) * f(r17.z) + f(r15.z));
            r15.x = tx;
            r15.y = ty;
            r15.z = tz;
        }
        r12.z = u(f(r12.y) + (-0.500000));
        r12.z = u(saturate(f(r12.z) + f(r12.z)));
        r13.x = u(f(r12.z) * (-2.000000) + 3.000000);
        r12.z = u(f(r12.z) * f(r12.z));
        r12.z = u(f(r12.z) * f(r13.x));
        r15.w = r16.w;
        {
            let tx = u(f(r2.x) + -f(r15.x));
            let ty = u(f(r2.y) + -f(r15.y));
            let tz = u(f(r2.z) + -f(r15.z));
            let tw = u(f(r2.w) + -f(r15.w));
            r17.x = tx;
            r17.y = ty;
            r17.z = tz;
            r17.w = tw;
        }
        {
            let tx = u(f(r12.z) * f(r17.x) + f(r15.x));
            let ty = u(f(r12.z) * f(r17.y) + f(r15.y));
            let tz = u(f(r12.z) * f(r17.z) + f(r15.z));
            let tw = u(f(r12.z) * f(r17.w) + f(r15.w));
            r15.x = tx;
            r15.y = ty;
            r15.z = tz;
            r15.w = tw;
        }
        r12.y = u(f(r10.w) + -f(r12.y));
        r12.y = b(f(r12.y) < 0.000000);
        {
            let tx = select(r15.x, r11.x, r12.y != 0u);
            let ty = select(r15.y, r11.y, r12.y != 0u);
            let tz = select(r15.z, r11.z, r12.y != 0u);
            let tw = select(r15.w, r11.w, r12.y != 0u);
            r15.x = tx;
            r15.y = ty;
            r15.z = tz;
            r15.w = tw;
        }
        {
            let tx = select(r16.x, r15.x, r12.x != 0u);
            let ty = select(r16.y, r15.y, r12.x != 0u);
            let tz = select(r16.z, r15.z, r12.x != 0u);
            let tw = select(r16.w, r15.w, r12.x != 0u);
            r14.x = tx;
            r14.y = ty;
            r14.z = tz;
            r14.w = tw;
        }
        r12.x = u(f(r14.w) + (-0.500000));
        r12.x = b(f(r12.x) < 0.000000);
        if r12.x != 0u { discard; }
        r13.w = r13.w + 1u;
    }
    r0.x = u(f(r14.w) + (-0.500000));
    r0.x = b(f(r0.x) < 0.000000);
    if r0.x != 0u { discard; }
    r0.x = u(f(cb0[265u].z) * f(cb0[266u].z));
    r0.y = u(f(r0.x) * f(r6.y));
    r0.y = u(saturate(f(r0.y) * 7.000000));
    r0.z = u(f(r0.y) * (-2.000000) + 3.000000);
    r0.y = u(f(r0.y) * f(r0.y));
    r0.w = u(f(r4.x) * 0.050000);
    r0.y = u(saturate(f(r0.z) * f(r0.y) + -f(r0.w)));
    r1.x = u(f(r0.x) * 3.333333);
    r0.z = u(-f(r0.x) * 1.200000 + 1.000000);
    r0.z = u(-f(r4.z) * 0.500000 + f(r0.z));
    {
        let tx = u(f(r2.x) * f(r0.z) + -f(r2.x));
        let ty = u(f(r2.y) * f(r0.z) + -f(r2.y));
        let tz = u(f(r2.z) * f(r0.z) + -f(r2.z));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    r5.w = 0u;
    {
        let tx = u(f(r0.y) * f(r5.x) + f(r2.x));
        let ty = u(f(r0.y) * f(r5.y) + f(r2.y));
        let tz = u(f(r0.y) * f(r5.z) + f(r2.z));
        let tw = u(f(r0.y) * f(r5.w) + f(r2.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    r0.x = b(f(r0.x) >= 0.990000);
    r0.z = u(f(cb0[265u].z) * f(cb0[266u].z) + (-0.400000));
    r0.z = u(saturate(f(r0.z) * 1.250000));
    r0.w = u(f(r0.z) * (-2.000000) + 3.000000);
    r0.z = u(f(r0.z) * f(r0.z));
    r0.z = u(f(r0.z) * f(r0.w));
    {
        let ty = u(-f(r4.z) + 1.000000);
        let tz = u(-f(r4.w) + 1.000000);
        r1.y = ty;
        r1.z = tz;
    }
    r0.z = b(f(r1.y) < f(r0.z));
    r0.x = r0.z | r0.x;
    {
        let tx = u(f(r4.x) * f(r8.x) + f(cb0[9u].x));
        let ty = u(f(r4.x) * f(r8.y) + f(cb0[9u].y));
        let tz = u(f(r4.x) * f(r8.z) + f(cb0[9u].z));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    {
        let tx = u(f(r3.x) * f(r5.x));
        let ty = u(f(r3.y) * f(r5.y));
        let tz = u(f(r3.z) * f(r5.z));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    {
        let tx = u(f(r5.x) * 0.100000);
        let ty = u(f(r5.y) * 0.100000);
        let tz = u(f(r5.z) * 0.100000);
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    r0.z = u(-f(r0.y) + 1.000000);
    {
        let tx = u(f(r0.z) * f(r14.x));
        let ty = u(f(r0.z) * f(r14.y));
        let tz = u(f(r0.z) * f(r14.z));
        r14.x = tx;
        r14.y = ty;
        r14.z = tz;
    }
    r1.x = u(saturate(f(r1.x)));
    r0.z = u(f(r1.x) * (-2.000000) + 3.000000);
    r0.w = u(f(r1.x) * f(r1.x));
    r0.z = u(f(r0.w) * f(r0.z));
    r0.z = u(f(r0.z) * 0.800000);
    {
        let tx = u(f(r14.x) * f(cb0[9u].x) + -f(r14.x));
        let ty = u(f(r14.y) * f(cb0[9u].y) + -f(r14.y));
        let tz = u(f(r14.z) * f(cb0[9u].z) + -f(r14.z));
        let tw = u(f(r14.w) * f(cb0[9u].w) + -f(r14.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = u(f(r0.z) * f(r7.x) + f(r14.x));
        let ty = u(f(r0.z) * f(r7.y) + f(r14.y));
        let tz = u(f(r0.z) * f(r7.z) + f(r14.z));
        let tw = u(f(r0.z) * f(r7.w) + f(r14.w));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    r0.y = b(0.800000 < f(r0.y));
    r0.y = r0.y & 1065353216u;
    {
        let tx = u(f(r2.x) + -f(r7.x));
        let ty = u(f(r2.y) + -f(r7.y));
        let tz = u(f(r2.z) + -f(r7.z));
        let tw = u(f(r2.w) + -f(r7.w));
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
        r8.w = tw;
    }
    {
        let tx = u(f(r0.y) * f(r8.w) + f(r7.w));
        let ty = u(f(r0.y) * f(r8.x) + f(r7.x));
        let tz = u(f(r0.y) * f(r8.y) + f(r7.y));
        let tw = u(f(r0.y) * f(r8.z) + f(r7.z));
        r7.x = tx;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = select(r7.y, r5.x, r0.x != 0u);
        let ty = select(r7.z, r5.y, r0.x != 0u);
        let tz = select(r7.w, r5.z, r0.x != 0u);
        r8.x = tx;
        r8.y = ty;
        r8.z = tz;
    }
    r5.w = r3.w;
    {
        let ty = r3.x;
        let tz = r3.y;
        let tw = r3.z;
        r7.y = ty;
        r7.z = tz;
        r7.w = tw;
    }
    {
        let tx = select(r7.x, r5.w, r0.x != 0u);
        let ty = select(r7.y, r5.x, r0.x != 0u);
        let tz = select(r7.z, r5.y, r0.x != 0u);
        let tw = select(r7.w, r5.z, r0.x != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r0.x = u(f(r0.x) + (-0.500000));
    r0.x = b(f(r0.x) < 0.000000);
    if r0.x != 0u { discard; }
    {
        let tx = u(f(cb0[266u].x) * f(cb0[266u].x));
        let ty = u(f(cb0[266u].y) * f(cb0[266u].y));
        r1.x = tx;
        r1.y = ty;
    }
    r0.x = u(f(r1.x) * f(r1.x));
    r1.x = u(f(cb0[266u].x) * 0.200000);
    r0.x = u(saturate(max(f(r0.x), f(r1.x))));
    {
        let tx = u(f(r4.w) * f(cb0[9u].x));
        let ty = u(f(r4.w) * f(cb0[9u].y));
        let tz = u(f(r4.w) * f(cb0[9u].z));
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
    }
    {
        let tx = u(f(r3.x) * 0.500000);
        let ty = u(f(r3.y) * 0.500000);
        let tz = u(f(r3.z) * 0.500000);
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
    }
    {
        let tx = u(min(f(r0.y), f(r3.x)));
        let ty = u(min(f(r0.z), f(r3.y)));
        let tz = u(min(f(r0.w), f(r3.z)));
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
    }
    r1.x = u(f(r0.x) * f(r6.x));
    r1.x = u(saturate(f(r1.x) * 12.000000));
    r1.x = u(f(r1.x) * 0.900000);
    {
        let tx = u(-f(r0.y) + f(r3.x));
        let ty = u(-f(r0.z) + f(r3.y));
        let tz = u(-f(r0.w) + f(r3.z));
        r3.x = tx;
        r3.y = ty;
        r3.z = tz;
    }
    {
        let ty = u(f(r1.x) * f(r3.x) + f(r0.y));
        let tz = u(f(r1.x) * f(r3.y) + f(r0.z));
        let tw = u(f(r1.x) * f(r3.z) + f(r0.w));
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r1.x = u(-f(cb0[265u].z) * f(cb0[266u].z) + 1.000000);
    {
        let tx = u(f(r0.y) * f(r1.x));
        let ty = u(f(r0.z) * f(r1.x));
        let tz = u(f(r0.w) * f(r1.x));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
    }
    {
        let ty = u(1.000000 / f(r4.w));
        let tz = u(1.000000 / f(r4.x));
        r0.y = ty;
        r0.z = tz;
    }
    r0.x = u(saturate(f(r0.y) * f(r0.x)));
    r0.y = u(f(r0.x) * (-2.000000) + 3.000000);
    r0.x = u(f(r0.x) * f(r0.x));
    r0.x = u(f(r0.x) * f(r0.y));
    r0.x = u(min(f(r0.x), 1.000000));
    r8.w = 1065353216u;
    r0.y = u(saturate(f(r0.z) * f(r0.x)));
    r0.z = u(f(r0.y) * (-2.000000) + 3.000000);
    r0.y = u(f(r0.y) * f(r0.y));
    r0.y = u(f(r0.y) * f(r0.z));
    r0.y = u(saturate(f(r0.y) * f(r4.z) + f(r0.y) * f(r4.z)));
    {
        let tx = u(f(r8.x) * f(cb0[3u].x) + -f(r8.x));
        let ty = u(f(r8.y) * f(cb0[3u].y) + -f(r8.y));
        let tz = u(f(r8.z) * f(cb0[3u].z) + -f(r8.z));
        let tw = u(f(r8.w) * f(cb0[3u].w) + -f(r8.w));
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
        r6.w = tw;
    }
    {
        let tx = u(f(r0.y) * f(r6.x) + f(r8.x));
        let ty = u(f(r0.y) * f(r6.y) + f(r8.y));
        let tz = u(f(r0.y) * f(r6.z) + f(r8.z));
        let tw = u(f(r0.y) * f(r6.w) + f(r8.w));
        r6.x = tx;
        r6.y = ty;
        r6.z = tz;
        r6.w = tw;
    }
    r0.y = u(f(r0.x) * f(r0.x) + (-0.900000));
    r0.y = u(f(r0.y) * 9.999998);
    r0.y = u(max(f(r0.y), 0.000000));
    r0.z = u(f(r0.y) * (-2.000000) + 3.000000);
    r0.y = u(f(r0.y) * f(r0.y));
    r0.y = u(f(r0.y) * f(r0.z));
    {
        let tx = u(-f(r2.x) + f(r5.x));
        let ty = u(-f(r2.y) + f(r5.y));
        let tz = u(-f(r2.z) + f(r5.z));
        let tw = u(-f(r2.w) + f(r5.w));
        r5.x = tx;
        r5.y = ty;
        r5.z = tz;
        r5.w = tw;
    }
    {
        let tx = u(f(r0.y) * f(r5.x) + f(r2.x));
        let ty = u(f(r0.y) * f(r5.y) + f(r2.y));
        let tz = u(f(r0.y) * f(r5.z) + f(r2.z));
        let tw = u(f(r0.y) * f(r5.w) + f(r2.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    r0.x = u(f(r0.x) + (-0.300000));
    r0.x = u(saturate(f(r0.x) * 3.333333));
    r0.y = u(f(r0.x) * (-2.000000) + 3.000000);
    r0.x = u(f(r0.x) * f(r0.x));
    r0.x = u(f(r0.x) * f(r0.y));
    {
        let tx = u(-f(r6.x) + f(r2.x));
        let ty = u(-f(r6.y) + f(r2.y));
        let tz = u(-f(r6.z) + f(r2.z));
        let tw = u(-f(r6.w) + f(r2.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
        r2.w = tw;
    }
    {
        let tx = u(f(r0.x) * f(r2.x) + f(r6.x));
        let ty = u(f(r0.x) * f(r2.y) + f(r6.y));
        let tz = u(f(r0.x) * f(r2.z) + f(r6.z));
        let tw = u(f(r0.x) * f(r2.w) + f(r6.w));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
        r0.w = tw;
    }
    r0.w = u(f(r0.w) + (-0.500000));
    r0.w = b(f(r0.w) < 0.000000);
    if r0.w != 0u { discard; }
    r0.w = u(-f(cb0[267u].x) * 0.250000 + f(r1.z));
    r0.w = u(f(r0.w) + -f(cb0[267u].x));
    r1.x = u(1.000000 / -f(cb0[267u].x));
    r0.w = u(saturate(f(r0.w) * f(r1.x)));
    r1.x = u(f(r0.w) * (-2.000000) + 3.000000);
    r0.w = u(f(r0.w) * f(r0.w));
    r0.w = u(saturate(f(r1.x) * f(r0.w) + f(cb0[267u].x)));
    {
        let tx = u(f(r10.y) * f(cb0[5u].x));
        let tz = u(f(r10.y) * f(cb0[5u].y));
        let tw = u(f(r10.y) * f(cb0[5u].z));
        r1.x = tx;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = u(f(r1.x) * 0.500000 + (-1.000000));
        let tz = u(f(r1.z) * 0.500000 + (-1.000000));
        let tw = u(f(r1.w) * 0.500000 + (-1.000000));
        r1.x = tx;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = u(f(r0.w) * f(r1.x) + 1.000000);
        let tz = u(f(r0.w) * f(r1.z) + 1.000000);
        let tw = u(f(r0.w) * f(r1.w) + 1.000000);
        r1.x = tx;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = u(f(r0.x) * f(r1.x));
        let ty = u(f(r0.y) * f(r1.z));
        let tz = u(f(r0.z) * f(r1.w));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
    }
    r0.x = u(f(r2.y) + f(r2.x));
    r0.x = u(f(r0.z) * f(r1.w) + f(r0.x));
    r0.y = u(f(r4.z) * f(r0.w));
    r0.y = u(f(r0.y) * 0.800000 + (-0.400000));
    r0.y = u(saturate(f(r0.y) * 20.000008));
    r0.z = u(f(r0.y) * (-2.000000) + 3.000000);
    r0.y = u(f(r0.y) * f(r0.y));
    r0.y = u(f(r0.y) * f(r0.z));
    r0.y = u(f(r0.y) * 0.500000);
    {
        let tx = u(f(r0.x) * 0.333333 + -f(r2.x));
        let tz = u(f(r0.x) * 0.333333 + -f(r2.y));
        let tw = u(f(r0.x) * 0.333333 + -f(r2.z));
        r0.x = tx;
        r0.z = tz;
        r0.w = tw;
    }
    {
        let tx = u(saturate(f(r0.y) * f(r0.x) + f(r2.x)));
        let ty = u(saturate(f(r0.y) * f(r0.z) + f(r2.y)));
        let tz = u(saturate(f(r0.y) * f(r0.w) + f(r2.z)));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    r0.w = u(f(cb1[0u].z) + f(cb1[0u].z));
    r0.w = b(f(r0.w) >= -f(r0.w));
    {
        let tx = select(3221225472u, 1073741824u, r0.w != 0u);
        let tz = select(3204448256u, 1056964608u, r0.w != 0u);
        r1.x = tx;
        r1.z = tz;
    }
    r0.w = u(f(r1.z) * f(cb1[0u].z));
    r0.w = u(fract(f(r0.w)));
    r0.w = u(f(r0.w) * f(r1.x));
    r0.w = u(f(r0.w) * 0.103100);
    r0.w = u(fract(f(r0.w)));
    r1.x = u(f(r0.w) + 33.330002);
    r0.w = u(f(r0.w) * f(r1.x));
    r1.x = u(f(r0.w) + f(r0.w));
    r0.w = u(f(r0.w) * f(r1.x));
    r0.w = u(fract(f(r0.w)));
    r0.w = u(f(r0.w) * f(cb0[266u].w));
    r1.x = u(f(cb1[0u].x) * 5.000000);
    r1.x = b(f(r1.x) >= -f(r1.x));
    {
        let tx = select(3231711232u, 1084227584u, r1.x != 0u);
        let tz = select(3192704205u, 1045220557u, r1.x != 0u);
        r1.x = tx;
        r1.z = tz;
    }
    r1.z = u(f(r1.z) * f(cb1[0u].x));
    r1.z = u(fract(f(r1.z)));
    r1.x = u(f(r1.z) * f(r1.x));
    r1.x = u(f(r1.x) * 0.103100);
    r1.x = u(fract(f(r1.x)));
    r1.z = u(f(r1.x) + 33.330002);
    r1.x = u(f(r1.z) * f(r1.x));
    r1.z = u(f(r1.x) + f(r1.x));
    r1.x = u(f(r1.z) * f(r1.x));
    r1.x = u(fract(f(r1.x)));
    r0.w = b(f(r1.x) < f(r0.w));
    r0.w = r0.w & 1065353216u;
    r0.w = u(f(r3.w) * f(r0.w));
    {
        let tx = u(saturate(f(r0.w) * f(cb0[8u].x) + f(r0.x)));
        let ty = u(saturate(f(r0.w) * f(cb0[8u].y) + f(r0.y)));
        let tz = u(saturate(f(r0.w) * f(cb0[8u].z) + f(r0.z)));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    r0.w = b(0.000000 < f(cb0[266u].y));
    r1.x = u(min(f(r1.y), 1.000000));
    {
        let ty = u(f(r0.x) * f(cb0[6u].x) + -f(r0.x));
        let tz = u(f(r0.y) * f(cb0[6u].y) + -f(r0.y));
        let tw = u(f(r0.z) * f(cb0[6u].z) + -f(r0.z));
        r1.y = ty;
        r1.z = tz;
        r1.w = tw;
    }
    {
        let tx = u(f(r1.x) * f(r1.y) + f(r0.x));
        let ty = u(f(r1.x) * f(r1.z) + f(r0.y));
        let tz = u(f(r1.x) * f(r1.w) + f(r0.z));
        r1.x = tx;
        r1.y = ty;
        r1.z = tz;
    }
    {
        let tx = u(max(f(r1.x), 0.000000));
        let ty = u(max(f(r1.y), 0.000000));
        let tz = u(max(f(r1.z), 0.000000));
        r1.x = tx;
        r1.y = ty;
        r1.z = tz;
    }
    r1.w = u(saturate(-f(cb0[266u].y)));
    {
        let tx = u(f(r0.x) * f(cb0[7u].x) + -f(r0.x));
        let ty = u(f(r0.y) * f(cb0[7u].y) + -f(r0.y));
        let tz = u(f(r0.z) * f(cb0[7u].z) + -f(r0.z));
        r2.x = tx;
        r2.y = ty;
        r2.z = tz;
    }
    {
        let tx = u(f(r1.w) * f(r2.x) + f(r0.x));
        let ty = u(f(r1.w) * f(r2.y) + f(r0.y));
        let tz = u(f(r1.w) * f(r2.z) + f(r0.z));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    {
        let tx = u(max(f(r0.x), 0.000000));
        let ty = u(max(f(r0.y), 0.000000));
        let tz = u(max(f(r0.z), 0.000000));
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    {
        let tx = select(r0.x, r1.x, r0.w != 0u);
        let ty = select(r0.y, r1.y, r0.w != 0u);
        let tz = select(r0.z, r1.z, r0.w != 0u);
        r0.x = tx;
        r0.y = ty;
        r0.z = tz;
    }
    r0.w = 1065353216u;
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
