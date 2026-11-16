// Hidden/Custom/HeatEffect's fragment stage, disassembled: the stack's result read at x + sin(y * _Frequency * ortho.x + _Time.z *
// _Speed) * _Intensity / ortho.x, y counted from the bottom. Without the effect (no heat volume) the stack's result shows as is.
struct Heat { intensity: f32, frequency: f32, speed: f32, enabled: f32 }
struct View { time_z: f32, ortho: f32, unused: vec2<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> heat: Heat;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> view: View;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var source_sampler: sampler;

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var at = uv;
    if heat.enabled != 0.0 {
        at.x = at.x + sin((1.0 - uv.y) * heat.frequency * view.ortho + view.time_z * heat.speed) * heat.intensity / view.ortho;
    }
    return textureSample(source, source_sampler, at);
}
