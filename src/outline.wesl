// Unlit/SpriteOutlineGlowOverlayShader (SelectionOutline.mat), from its glcore program: the texel's alpha less the
// least alpha of its four neighbours (zero within a texel of the sprite rect's edge), kept from _Threshold up, drawn as
// _GlowColour * 0.5, alpha-blended over the ScreenUI camera's view (layer 16) as the game shows it.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct Settings {
    // _GlowColour
    glow: vec4<f32>,
    // _AtlasTransform: the sprite's lowest uv, then its rect over the texture size.
    atlas: vec4<f32>,
    // _MainTex_TexelSize: 1 / width, 1 / height, width, height.
    texel: vec4<f32>,
    // _ObjectScale (RefreshOutline sets one).
    object_scale: vec2<f32>,
    threshold: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The mesh's v runs down the texture; Unity's up.
    let uv = in.uv;
    let unity = vec2<f32>(uv.x, 1.0 - uv.y);
    let t = settings.texel.xy;
    let left = textureSample(sprite, sprite_sampler, uv + vec2<f32>(-t.x, 0.0)).a;
    let right = textureSample(sprite, sprite_sampler, uv + vec2<f32>(t.x, 0.0)).a;
    let up = textureSample(sprite, sprite_sampler, uv + vec2<f32>(0.0, t.y)).a;
    let down = textureSample(sprite, sprite_sampler, uv + vec2<f32>(0.0, -t.y)).a;
    let centre = textureSample(sprite, sprite_sampler, uv).a;
    var least = min(min(down, up), min(right, left));
    // vs_TEXCOORD1: the position inside the sprite rect, 0 to 1.
    let inner = (unity - settings.atlas.xy) / settings.atlas.zw;
    var margin = t / settings.atlas.zw;
    if 0.01 < settings.object_scale.x * settings.object_scale.y {
        margin = margin / settings.object_scale;
    }
    let edge = inner.x < margin.x || inner.y < margin.y || 1.0 - margin.x < inner.x || 1.0 - margin.y < inner.y;
    least = min(select(1.0, 0.0, edge), least);
    let a = clamp((1.0 - least) * centre, 0.0, 1.0);
    if a - settings.threshold < 0.0 {
        discard;
    }
    return settings.glow * 0.5;
}
