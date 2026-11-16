// Unlit/DissolveMirror's fragment stage, from its glcore program: the sprite, discarded where _DissolveMap (read at the vertex
// position * 0.5) is under _Progress for the pixels all of whose rgb are above _MapThreshold.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var tex0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var samp0: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var tex1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var samp1: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let threshold = params[0];
    let progress = params[1].x;
    let c = textureSample(tex0, samp0, in_uv);
    let pos = (in_uv * extra.local.xy + extra.local.zw) * 0.5;
    // A Unity uv: the images run v down.
    let map = textureSample(tex1, samp1, vec2<f32>(pos.x, 1.0 - pos.y));
    let hit = all(threshold.xyz < c.xyz);
    let d = select(0.0, progress, hit);
    if (map.x - d < 0.0) { discard; }
    return c;
}
