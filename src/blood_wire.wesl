// Unlit/BloodWireShader's fragment stage, disassembled: a pulse runs along the line, bulging it and tinting it with the blood.
struct Settings {
    st: vec4<f32>,
    blood: vec4<f32>,
    flow: f32,
    bulge: f32,
    time: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;

@fragment
fn fragment(@location(2) mesh_uv: vec2<f32>, @location(4) color: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = mesh_uv * settings.st.xy + settings.st.zw;
    let direction = select(-1.0, 1.0, 0.0 < settings.flow);
    let c = sin(direction * settings.time * 8.0 - uv.x * 0.1) * 0.5 + 0.5;
    let amount = min(abs(settings.flow), 1.0);
    var wave = c * c;
    wave = wave * wave;
    wave = amount * wave * wave;
    let bulge = wave * (1.0 - settings.bulge) + settings.bulge;
    let across = uv.y - 0.5;
    if 0.5 - abs(bulge * across) < 0.0 {
        discard;
    }
    let tint = pow(amount * amount * amount * amount * amount * wave, 0.1);
    let colour = textureSample(sprite, sprite_sampler, vec2<f32>(uv.x, across * bulge + 0.5));
    return mix(colour, vec4<f32>(colour.rgb * settings.blood.rgb, colour.a), max(tint, amount * 0.8)) * color;
}
