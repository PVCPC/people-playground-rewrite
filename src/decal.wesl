// A decal: Sprites/Default inside a SpriteMask, whose stencil keeps the object sprite's pixels above the alpha cutoff.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct Settings {
    u: vec4<f32>,
    v: vec4<f32>,
    rect: vec4<f32>,
    cutoff: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var mask_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world = vec3<f32>(in.world_position.xy, 1.0);
    let uv = vec2<f32>(dot(settings.u.xyz, world), dot(settings.v.xyz, world));
    let inside = all(uv >= settings.rect.xy) && all(uv <= settings.rect.zw);
    let alpha = textureSample(mask, mask_sampler, uv).a;
    if !inside || alpha <= settings.cutoff {
        discard;
    }
    return textureSample(sprite, sprite_sampler, in.uv) * in.color;
}
