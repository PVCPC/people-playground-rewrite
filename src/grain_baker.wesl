// Hidden/PostProcessing/GrainBaker's two passes, disassembled: 0 monochrome (Step2 at frac(_Phase)), 1 colored (Step2 at
// frac(_Phase) times 0.07, 0.11 and 0.13 for red, green and blue), on the 128x128 lookup's pixels; _NoiseParameters is
// GrainRenderer's (12.9898, 78.233, 43758.547).
struct Baker { phase: f32, colored: f32, unused: vec2<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> baker: Baker;

const NOISE: vec3<f32> = vec3<f32>(12.9898, 78.233, 43758.547);

fn noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, NOISE.xy)) * NOISE.z);
}

// A 3x3 kernel: 1 at the corners, `edge` at the sides, `centre` in the middle.
fn weight(x: i32, y: i32, edge: f32, centre: f32) -> f32 {
    if x == 0 && y == 0 {
        return centre;
    }
    return select(1.0, edge, x == 0 || y == 0);
}

fn step1(p: vec2<f32>, n: f32) -> f32 {
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += noise(p + vec2<f32>(f32(x), f32(y)) + n) * weight(x, y, 2.0, -12.0);
        }
    }
    return sum / 24.0;
}

fn step2(p: vec2<f32>, n: f32) -> f32 {
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += step1(p + vec2<f32>(f32(x), f32(y)), n) * weight(x, y, 2.0, 4.0);
        }
    }
    return sum / 16.0;
}

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // texcoord * 128, y up as Unity's.
    let p = vec2<f32>(uv.x, 1.0 - uv.y) * 128.0;
    let f = fract(baker.phase);
    if baker.colored != 0.0 {
        return vec4<f32>(step2(p, f * 0.07), step2(p, f * 0.11), step2(p, f * 0.13), 1.0);
    }
    return vec4<f32>(vec3<f32>(step2(p, f)), 1.0);
}
