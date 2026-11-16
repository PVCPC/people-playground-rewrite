// Unlit/MinigunBarrelShader's fragment stage, from its glcore program: the uv is the mesh's times _ActualSprite_ST; _ActualSprite is
// read at that uv moved up by _Mask.g * _Rotation (the barrels turning), its alpha capped by _Mask.r, and its colour lerped towards
// itself * _GlowColour by sqrt(_Mask.b) * _Progress, twice (the heat glow). The vertex colour is not used. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var actual_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var actual_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var mask_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var mask_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let rotation = params[0].x;
    let glow_colour = params[1];
    let progress = params[2].x;
    // Unity's uv (v up; the mesh and the images run v down).
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let mask = textureSample(mask_tex, mask_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let at = vec2<f32>(0.0, mask.y * rotation) + uv;
    let c = textureSample(actual_tex, actual_sampler, vec2<f32>(at.x, 1.0 - at.y));
    let g = c.xyz * glow_colour.xyz;
    let k = sqrt(mask.z) * progress;
    let d = g * vec3<f32>(k) + (-c.xyz);
    return vec4<f32>(vec3<f32>(k) * d + c.xyz, min(c.w, mask.x));
}
