// Unlit/FogShader (MapConfig's fog overlay), from its glcore program: Blend SrcAlpha OneMinusSrcAlpha. The vertex stage passes
// world.xy * (0.7, 1). Three fbm sums of a hashed 3D value noise: two moving ones (the second five times finer), pushed by the
// _Noise texture, make the _Tint fog, darkened by a slow third (at least 0.3); toward the zoomed-out view (unity_OrthoParams.x
// * 0.004, to the 0.2) it fades to _FadeColour times that third. All times _Density (MapConfig.OverlayFadeStep).
// See src/custom_material.rs.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
    screen: vec4<f32>,
    ortho: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var noise_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var noise_sampler: sampler;

// Dave Hoskins' hash13.
fn hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.103100002);
    q = q + dot(q, q.zyx + 31.3199997);
    return fract((q.x + q.y) * q.z);
}

// Trilinear value noise with a smoothstep weight.
fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let h = fn_corner(i, u, 0.0);
    let k = fn_corner(i, u, 1.0);
    return mix(h, k, u.z);
}

fn fn_corner(i: vec3<f32>, u: vec3<f32>, z: f32) -> f32 {
    let a = mix(hash(i + vec3<f32>(0.0, 0.0, z)), hash(i + vec3<f32>(1.0, 0.0, z)), u.x);
    let b = mix(hash(i + vec3<f32>(0.0, 1.0, z)), hash(i + vec3<f32>(1.0, 1.0, z)), u.x);
    return mix(a, b, u.y);
}

fn fbm(p: vec3<f32>) -> f32 {
    return noise(p * 2.0) * 0.0250000004 + noise(p * 4.0) * 0.0125000002 + noise(p) * 0.0500000007 + noise(p * 0.5) * 0.100000001 + noise(p * 0.25) * 0.200000003;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let snow_direction = params[0].xy;
    let noise_direction = params[1].xy;
    let intensity = params[2].x;
    let tiling = params[3].x;
    let tiling_n = params[4].x;
    let density = params[6].x;
    let tint = params[7];
    let fade_colour = params[8];
    let t = extra.time.x;
    let uv = in.world_position.xy * vec2<f32>(0.699999988, 1.0);
    let n = textureSample(noise_tex, noise_sampler, t * noise_direction + uv * tiling_n);
    let p1 = vec3<f32>((t * 0.5 + 5.0) * snow_direction + uv * tiling - n.zx * intensity, t);
    let p2 = vec3<f32>(uv * tiling * 5.0 + t * snow_direction - n.xz * intensity, 23.2320004);
    let p3 = vec3<f32>(t * snow_direction * 0.00999999978 + uv * tiling * 0.00999999978, -13.2320004);
    let slow = fbm(p3);
    let fog = (fbm(p1) + fbm(p2)) * tint * max(slow, 0.300000012);
    let far = exp2(log2(clamp(extra.ortho.x * 0.00400000019, 0.0, 1.0)) * 0.200000003);
    return (far * (fade_colour * slow - fog) + fog) * density;
}
