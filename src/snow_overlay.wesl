// Unlit/SnowOverlay (MapConfig's snow and rain overlays), from its glcore program: Blend SrcAlpha OneMinusSrcAlpha. The vertex
// stage passes world.xy. Two _MainTex reads at world * _Tiling, moved along _SnowDirection and pushed by the _Noise texture: a
// slow one (above _Density 0.5 only, its alpha times 2 * _Density - 1) and a faster one (alpha times _Density); their sum
// times _Visibility, saturated, times _Tint. With _BlackIsAlpha the alpha also takes min((0.1 * |rgb|)^2, 1).
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
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var noise_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var noise_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let snow_direction = params[0].xy;
    let noise_direction = params[1].xy;
    let intensity = params[2].x;
    let tiling = params[3].x;
    let tiling_n = params[4].x;
    let black_is_alpha = params[5].x;
    let visibility = params[6].x;
    let density = params[7].x;
    let tint = params[8];
    let t = extra.time.x;
    let uv = in.world_position.xy;
    let n = textureSample(noise_tex, noise_sampler, t * noise_direction + uv * tiling_n);
    var slow = textureSample(main_tex, main_sampler, t * snow_direction + uv * tiling - n.xz * intensity);
    if !(0.5 < density) {
        slow = vec4<f32>(0.0);
    }
    slow.w = slow.w * (density * 2.0 - 1.0);
    var fast = textureSample(main_tex, main_sampler, (t * 0.5 + 5.0) * snow_direction + uv * tiling - n.zx * intensity);
    fast.w = fast.w * density;
    let c = clamp((slow + fast) * visibility, vec4<f32>(0.0), vec4<f32>(1.0)) * tint;
    var a = 1.0;
    if 0.5 < black_is_alpha {
        a = length(c.xyz) * 0.100000001;
    }
    return vec4<f32>(c.xyz, c.w * min(a * a, 1.0));
}
