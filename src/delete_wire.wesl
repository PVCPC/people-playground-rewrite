// Unlit/TwoSidedUnlit: the vertex stage's uv * _MainTex_ST.xy + _MainTex_ST.zw, then _MainTex sampled as it is.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> st: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(main_tex, main_sampler, uv * st.xy + st.zw);
}
