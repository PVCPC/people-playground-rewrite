// Hidden/Custom/FirePost's fragment stage (1.28.54), disassembled from its Direct3D 11 program, whose uv runs from the top as
// this one does: the fire texture, warped by the warp sequence and tinted through the colour ramp, added to the source,
// which the same warp (scaled by the fire's brightness) shifts.
struct Settings { intensity: f32, offset: f32, multiplier: f32, enabled: f32 }
struct View { time: f32, ortho_width: f32, aspect: f32, ortho_height: f32 }
struct Camera { position: vec2<f32>, gravity: vec2<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> view: View;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> camera: Camera;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var fire: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var fire_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var ramp: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var ramp_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var warp_sequence: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var warp_sampler: sampler;

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if settings.enabled != 1.0 {
        return textureSample(source, source_sampler, uv);
    }
    // The warp sequence's frame: floor(_Time.y * 64) / 24, its fraction keeping the sign.
    let frame = floor(view.time * 64.0) * 0.041666668;
    var phase = fract(abs(frame));
    if !(frame >= -frame) {
        phase = -phase;
    }
    // _ClipToWorld of uv * 2 - 1: the camera's position plus the clip position times the half extents.
    let world = camera.position + (uv * 2.0 - 1.0) * vec2<f32>(view.ortho_width, view.ortho_height);
    let lookup = vec2<f32>(world.x * view.aspect * 0.020833334 + phase + view.time * camera.gravity.x, world.y * 0.5 + view.time * camera.gravity.y);
    // A texture's v counts from its bottom row in the game, from the top here.
    var warp = (textureSample(warp_sequence, warp_sampler, vec2<f32>(lookup.x, 1.0 - lookup.y)).xy * 2.0 - 1.0) * (1.0 / view.ortho_width * 1.5);
    // The game's texture has no alpha channel: it samples as 1.
    let flame = vec4<f32>(textureSample(fire, fire_sampler, warp * 0.01 + uv).rgb, 1.0);
    // 1 where the fire texture has any colour, else 0.
    let any = min(dot(flame.rgb, flame.rgb) * 3.4028235e38 + 0.5, 1.0) * 2.0 - 1.0;
    let luma = dot(flame.rgb + vec3<f32>(settings.offset), vec3<f32>(0.299, 0.587, 0.114));
    warp = warp * any * clamp(luma * settings.multiplier + 0.5, 0.0, 1.0);
    let base = textureSample(source, source_sampler, warp * 0.005 + uv);
    let tint = vec4<f32>(textureSample(ramp, ramp_sampler, vec2<f32>(luma * settings.multiplier, 1.0)).rgb, 1.0);
    return flame * tint * settings.intensity + base;
}
