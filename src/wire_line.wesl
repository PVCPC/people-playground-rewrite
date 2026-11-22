// Unlit/VertexColourCutout (mode 0), Jaap/GlowingMetal (mode 1) and Jaap/UseWire (mode 2), the fragment stages disassembled. The cutout shader
// blends One Zero: alpha 1 here gives the same pixel in the 2D blend pass.
struct Settings {
    st: vec4<f32>,
    glow: vec4<f32>,
    cutoff: f32,
    progress: f32,
    mode: f32,
    time: f32,
    timestamp: f32,
    glow_time: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;

fn srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn fragment(@location(2) mesh_uv: vec2<f32>, @location(4) color: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite, sprite_sampler, mesh_uv * settings.st.xy + settings.st.zw);
    if settings.mode < 0.5 {
        if texel.a * color.a - settings.cutoff < 0.0 {
            discard;
        }
        // Unity hands the LineRenderer's colours to the shader as stored (gamma values, no linear conversion); the mesh's
        // colours are linear, so undo that (checked against the game's SerpentineBeltTool line, ISSUES.md #21).
        return vec4<f32>(texel.rgb * srgb(color.rgb), 1.0);
    }
    if settings.mode > 1.5 {
        // Jaap/UseWire: the glow fades over _GlowTime from _Timestamp, on the texels over 0.9 alpha.
        var t = (settings.time - settings.timestamp) / settings.glow_time;
        t = clamp(t, 0.0, 1.0);
        t = min(t, 0.980000019);
        t = -t + 1.0;
        var u1 = texel;
        let lit = select(0.0, 1.0, 0.899999976 < u1.w);
        t = lit * t;
        u1.w = 1.0;
        let u2 = settings.glow * u1 - u1;
        return (vec4<f32>(t) * u2 + u1) * color;
    }
    if texel.a - 0.5 < 0.0 {
        discard;
    }
    let progress = saturate(settings.progress);
    let hot = texel.rgb * settings.glow.rgb;
    return vec4<f32>(progress * (hot * progress - texel.rgb) + texel.rgb, texel.a);
}
