// Unlit/WaterShader (WaterBehaviour's body of water), from its glcore program: Blend One Zero, a GrabPass "_BackgroundTexture"
// (src/grab_pass.rs). The depth d = min((1 - v)^_DepthMultiplier, 1). Below 0 degrees (_Temperature, WaterBehaviour's
// OnWillRenderObject) it is ice: two world-scale _IceTexture reads mixed by the camera zoom. Otherwise the background is
// refracted by _Shimmering, tinted from _Tint to _DepthColour by depth, with scrolling _CausticsTex light; at 100 and up two
// Voronoi bubble layers are added. See src/custom_material.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
    screen: vec4<f32>,
    ortho: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var ice_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var ice_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var shimmer_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var shimmer_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var background_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var background_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var caustics_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var caustics_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

// vs_TEXCOORD0 (_MainTex_ST), 1 (_CausticsTex_ST), 3 (ComputeGrabScreenPos without the flip), 5 (_Shimmering_ST) and 6 (the
// world xy).
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) screen: vec4<f32>,
    @location(3) uv5: vec2<f32>,
    @location(4) world: vec2<f32>,
}

@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    let w = get_world_from_local(v.instance) * vec4<f32>(v.position, 1.0);
    let p = view.clip_from_world * w;
    out.clip = p;
    let uv = vec2<f32>(v.uv.x, 1.0 - v.uv.y);
    out.uv0 = uv * params[15].xy + params[15].zw;
    out.uv1 = uv * params[9].xy + params[9].zw;
    out.uv5 = uv * params[10].xy + params[10].zw;
    let h = p.xyw * vec3<f32>(0.5, 0.5, 0.5);
    out.screen = vec4<f32>(h.zz + h.xy, p.zw);
    out.world = w.xy;
    return out;
}

// texture() at a GL (bottom-up) coordinate.
fn read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, vec2<f32>(uv.x, 1.0 - uv.y));
}

// The squared distance to the nearest of the 3 x 3 cells' moving points (the program's unrolled loops).
fn voronoi(p: vec2<f32>) -> f32 {
    let f = floor(p);
    let g = fract(p);
    var m = 8.0;
    for (var i = -1; i <= 1; i++) {
        for (var j = -1; j <= 1; j++) {
            let o = vec2<f32>(f32(j), f32(i));
            let n = f + o;
            let h = fract(sin(vec2<f32>(dot(n, vec2<f32>(127.099998, 311.700012)), dot(n, vec2<f32>(269.5, 183.300003)))) * vec2<f32>(18.5452995, 18.5452995));
            let r = sin(h * vec2<f32>(6.28310013, 6.28310013) + vec2<f32>(0.5, 0.5)) * vec2<f32>(0.5, 0.5) + (o - g) + vec2<f32>(0.5, 0.5);
            let d = dot(r, r);
            m = select(m, d, d < m);
        }
    }
    return m;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let tint = params[0];
    let depth_colour = params[1];
    let caustics_speed = params[2].x;
    let skew = params[3].x;
    let distort = params[4].x;
    let brightness = params[5].x;
    let depth_fade = params[6].x;
    let depth_multiplier = params[7].x;
    let temperature = params[8].x;
    let time = extra.time;
    let ortho = extra.ortho;
    let d = min(exp2(log2(1.0 - in.uv0.y) * depth_multiplier), 1.0);
    let c1 = read(main_tex, main_sampler, in.uv0);
    if temperature < 0.0 {
        let t7 = vec2<f32>(-d, -d) + vec2<f32>(1.0, 0.899999976);
        let col = t7.x * (depth_colour.xyz - tint.xyz) + tint.xyz;
        let lit = c1.xyz * col;
        let w4 = in.world.xyxy * vec4<f32>(0.00200000009, 0.00200000009, 0.0666666701, 0.0666666701);
        var far = read(ice_tex, ice_sampler, w4.xy).xyz;
        far = (vec3<f32>(0.717647076, 0.909803927, 0.890196085) - far) * vec3<f32>(0.699999988) + far;
        let near = read(ice_tex, ice_sampler, w4.zw).xyz;
        let k = max(t7.y, 0.0);
        let z = exp2(log2(clamp(ortho.x * 0.00400000019, 0.0, 1.0)) * 0.100000001);
        let body = (vec3<f32>(1.0) - c1.xyz * col) * vec3<f32>(0.5) + lit;
        var ice = vec3<f32>(z) * (far - near) + near;
        ice = vec3<f32>(k * k * k) * (vec3<f32>(0.100000001) - ice) + ice;
        return vec4<f32>(body * ice, max(z * 0.800000012, 0.200000003));
    }
    let a7 = 1.0 - d;
    let c2 = vec4<f32>(a7) * (depth_colour.wxyz - tint.wxyz) + tint.wxyz;
    let s = 0.100000001 / ortho.y;
    let sh = read(shimmer_tex, shimmer_sampler, vec2<f32>(time.y * 0.00999999978, 0.0) + in.uv5);
    let r7 = vec3<f32>(max(a7, 0.100000001)) * (vec3<f32>(s) * sh.xyw) * vec3<f32>(distort) + in.screen.xyw;
    let bg = read(background_tex, background_sampler, r7.xy / r7.z);
    let cuv = vec2<f32>(in.uv0.y * skew * 0.0500000007, time.y * caustics_speed) + vec2<f32>(in.uv1.x, 0.0);
    let ca = read(caustics_tex, caustics_sampler, cuv);
    let light = ca.x * ca.x * brightness;
    let fade = depth_fade * (d + -1.0) + 1.0;
    let c1w = vec4<f32>(light) * vec4<f32>(fade) + c1.wxyz;
    let under = (vec3<f32>(d) * (vec3<f32>(1.0) - c2.yzw) + c2.yzw) * bg.xyz;
    let a = c2.x * c1w.x;
    let rgb = vec3<f32>(a) * (c1w.yzw * c2.yzw - under) + under;
    // Both bubble layers' _Shimmering reads before the branch: textureSample needs uniform control flow.
    let b1 = read(shimmer_tex, shimmer_sampler, in.world * vec2<f32>(0.00999999978) + vec2<f32>(0.0, time.x * 0.600000024));
    let b2 = read(shimmer_tex, shimmer_sampler, in.world * vec2<f32>(0.0149999997) + vec2<f32>(0.0, time.x));
    if temperature >= 100.0 {
        let q = min(d + 0.5, 1.0);
        let t3 = time.zz * vec2<f32>(3.60000014, 6.0) + vec2<f32>(q);
        var p = b1.xy * vec2<f32>(2.0) + in.world;
        p = vec2<f32>(p.x, p.y - t3.x) * 2.0;
        let l1 = d * (1.0 - sqrt(voronoi(p)));
        let w1 = 1.0 - abs(-l1 * l1 + 0.5);
        var p2 = b2.xy * vec2<f32>(2.0) + in.world * vec2<f32>(1.5);
        p2 = vec2<f32>(p2.x, p2.y - t3.y) * 2.0;
        let l2 = d * (1.0 - sqrt(voronoi(p2)));
        let w2 = 1.0 - abs(-l2 * l2 + 0.5);
        let x = w2 * w2;
        let y = w1 * w1;
        let r = y * y + x * x;
        return vec4<f32>(r) * vec4<f32>(0.00500000035) + vec4<f32>(rgb, 1.0);
    }
    return vec4<f32>(rgb, 1.0);
}
