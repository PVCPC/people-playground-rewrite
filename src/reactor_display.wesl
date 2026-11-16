// Unlit/ReactorDisplay (the Reactor A5's diagnostics sprite), from its glcore program: the red channel of _MainTex picks the part
// (seven bands of 1/7, and the base above 0.9) whose colour ReactorA5DisplayBehaviour sets; alpha 1. Blend SrcAlpha
// OneMinusSrcAlpha. See src/custom_material.rs and game/reactor.rs.
struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;

fn band(on: bool, c: vec4<f32>) -> vec3<f32> {
    return select(vec3<f32>(0.0), c.xyz, on);
}

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    // params: _Wire1, _Wire2, _Wire3, _Wire4, _VacuumChamber, _Connector, _Base.
    let x = textureSample(main_tex, main_sampler, in_uv).x;
    var c = band((0.0 < x) && (x < 0.142857149), params[4]) + band((0.142857149 < x) && (x < 0.428571433), params[5]);
    c = c + band((0.899999976 < x) && (x < 2.0), params[6]);
    c = band((0.428571433 < x) && (x < 0.571428597), params[0]) + c;
    c = c + band((0.571428597 < x) && (x < 0.714285731), params[1]);
    c = c + band((0.714285731 < x) && (x < 0.857142866), params[2]);
    c = band((0.857142866 < x) && (x < 1.0), params[3]) + c;
    return vec4<f32>(c, 1.0);
}
