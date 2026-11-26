// Hidden/Custom/FirePost's fragment stage (1.28.57), disassembled from its Direct3D 11 program, whose uv runs from the top as
// this one does: the fire texture, warped by the warp sequence and faded by a smoothstep of its brightness, added to the
// source, which the same warp (scaled by the fire's brightness) shifts. 1.28.57 dropped 1.28.54's colour ramp.
struct Settings { intensity: f32, offset: f32, multiplier: f32, enabled: f32 }
// aspect: _AspectRatio (camera.aspect); frame: _FrameOffset.
struct View { time: f32, ortho_width: f32, aspect: f32, ortho_height: f32 }
struct Camera { position: vec2<f32>, gravity: vec2<f32> }
struct Frame { offset: f32, pad0: f32, pad1: f32, pad2: f32 }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> view: View;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> camera: Camera;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var fire: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var fire_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var warp_sequence: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var warp_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var<uniform> frame: Frame;

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if settings.enabled != 1.0 {
        return textureSample(source, source_sampler, uv);
    }
    // _ClipToWorld of uv * 2 - 1: the camera's position plus the clip position times the half extents.
    let world = camera.position + (uv * 2.0 - 1.0) * vec2<f32>(view.ortho_width, view.ortho_height);
    // The warp sequence's frame is _FrameOffset (1.28.54 worked it out from _Time.y here).
    let lookup = vec2<f32>(world.x * view.aspect * 0.020833334 + frame.offset + view.time * camera.gravity.x, world.y * 0.5 + view.time * camera.gravity.y);
    // A texture's v counts from its bottom row in the game, from the top here.
    let raw = textureSample(warp_sequence, warp_sampler, vec2<f32>(lookup.x, 1.0 - lookup.y)).xy * 2.0 - 1.0;
    let k = 1.0 / view.ortho_width * 0.02;
    var warp = k * raw;
    // The game's texture has no alpha channel: it samples as 1.
    let flame = vec4<f32>(textureSample(fire, fire_sampler, raw * k + uv).rgb, 1.0);
    // 1 where the fire texture has any colour, else 0.
    let any = min(dot(flame.rgb, flame.rgb) * 3.4028235e38 + 0.5, 1.0) * 2.0 - 1.0;
    warp = warp * any;
    let luma = dot(flame.rgb, vec3<f32>(0.299, 0.587, 0.114)) * settings.multiplier + settings.offset;
    let shift = clamp(luma + 0.5, 0.0, 1.0);
    let s = clamp(luma, 0.0, 1.0);
    warp = shift * warp;
    let base = textureSample(source, source_sampler, warp * 0.5 + uv);
    // smoothstep: s * s * (3 - 2 s).
    let fade = (s * s) * (s * -2.0 + 3.0);
    return flame * fade * settings.intensity + base;
}
