// Unlit/BrokenElectronics (Materials/BrokenMachine, DamagableMachineryBehaviour.SetAsBroken), from its glcore program: _DamageMap
// and _BrokenElectronics read at the vertex position * 0.5 + _Offset; the damage map's x and z shift _MainTex's uv by 1%, the
// colour is lerped toward colour * _GlowColour by _Progress twice; above 0.1 damage it greys to a fifth of its sum, the damage
// map's z fades it to black, and above 0.2 damage it is _BrokenElectronics times the colour clamped to [0.3, 1]. No vertex
// colour. Blend SrcAlpha OneMinusSrcAlpha. See src/custom_material.rs for `params`.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var damage_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var damage_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var broken_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var broken_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    // params: _Progress, _GlowColour, _Offset.
    let progress = params[0].x;
    let glow = params[1];
    let offset = params[2];
    // vs_TEXCOORD1 = in_POSITION0.xy * 0.5; a Unity uv, the images running v down.
    let pos = (in_uv * extra.local.xy + extra.local.zw) * 0.5 + offset.xy;
    let at = vec2<f32>(pos.x, 1.0 - pos.y);
    let d = textureSample(damage_tex, damage_sampler, at);
    var b = textureSample(broken_tex, broken_sampler, at);
    let uv = d.xz * vec2<f32>(0.00999999978) + vec2<f32>(in_uv.x, 1.0 - in_uv.y);
    var c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let t = c.xyz * glow.xyz * vec3<f32>(progress) - c.xyz;
    c = vec4<f32>(vec3<f32>(progress) * t + c.xyz, c.w);
    let sum = c.z + (c.y + c.x);
    var g = vec4<f32>(vec3<f32>(sum) * vec3<f32>(0.200000003) - c.xyz, 0.0);
    let grey = select(0.0, 1.0, 0.100000001 < d.x);
    g = vec4<f32>(vec3<f32>(grey) * g.xyz + c.xyz, c.w);
    let m = max(c, vec4<f32>(0.300000012));
    b.w = c.w;
    let lit = b * min(m, vec4<f32>(1.0));
    let dark = vec4<f32>(0.0, 0.0, 0.0, g.w);
    let faded = d.zzzz * (dark - g) + g;
    return select(faded, lit, 0.200000003 < d.x);
}
