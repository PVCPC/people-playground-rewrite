//! The shaders' path ids as the code knows them (1.28.41's), by file and name. Other builds renumber them (1.28.52:
//! TwoSidedUnlit 130 -> 134, AdditiveGlowSprite 891 -> 895, ProceduralShatter 81 -> 82; 1.28.54: HeatEffect 37 -> 38):
//! `AssetDb::shader_id` maps a shader back to its 1.28.41 id through this table.

use super::serialized::Reader;

/// A Shader's name, m_ParsedForm.m_Name: the SerializedShader read up to it (Unity 2020.3.1's layout, the version every
/// build here ships). Read for all 115/116 shaders of 1.28.41, 1.28.53 and both 1.28.54 builds.
pub fn shader_name(d: &[u8]) -> Result<String, String> {
    let r = &mut Reader::new(d);
    r.string()?; // NamedObject m_Name (empty)
    // m_PropInfo.m_Props: m_Name, m_Description, m_Attributes, m_Type, m_Flags, m_DefValue[4], m_DefTexture {m_DefaultName, m_TexDim}.
    for _ in 0..count(r)? {
        r.string()?;
        r.string()?;
        strings(r)?;
        r.bytes(4 + 4 + 16)?;
        r.string()?;
        r.i32()?;
    }
    // m_SubShaders: m_Passes, m_Tags, m_LOD.
    for _ in 0..count(r)? {
        for _ in 0..count(r)? {
            pass(r)?;
        }
        tags(r)?;
        r.i32()?;
    }
    r.string()
}

fn count(r: &mut Reader) -> Result<usize, String> {
    usize::try_from(r.i32()?).map_err(|_| "a negative count".into())
}

fn strings(r: &mut Reader) -> Result<(), String> {
    for _ in 0..count(r)? {
        r.string()?;
    }
    Ok(())
}

/// SerializedTagMap: (key, value) strings.
fn tags(r: &mut Reader) -> Result<(), String> {
    for _ in 0..count(r)? {
        r.string()?;
        r.string()?;
    }
    Ok(())
}

/// `n` items of `size` bytes, then the alignment.
fn array(r: &mut Reader, size: usize) -> Result<(), String> {
    let n = count(r)?;
    r.bytes(n * size)?;
    r.align();
    Ok(())
}

/// SerializedShaderFloatValue: val, name.
fn float_value(r: &mut Reader, n: usize) -> Result<(), String> {
    for _ in 0..n {
        r.bytes(4)?;
        r.string()?;
    }
    Ok(())
}

/// SerializedPass (2020.2-2021.1): m_EditorDataHash, m_Platforms, m_LocalKeywordMask, m_GlobalKeywordMask, m_NameIndices,
/// m_Type, m_State, m_ProgramMask, the six programs, the instancing flags, m_UseName, m_Name, m_TextureName, m_Tags.
fn pass(r: &mut Reader) -> Result<(), String> {
    array(r, 16)?;
    array(r, 1)?;
    array(r, 2)?;
    array(r, 2)?;
    for _ in 0..count(r)? {
        r.string()?;
        r.i32()?;
    }
    r.i32()?;
    // SerializedShaderState: m_Name, rtBlend0-7 (7 values each), rtSeparateBlend, zClip, zTest, zWrite, culling, conservative,
    // offsetFactor, offsetUnits, alphaToMask, stencilOp/Front/Back (4 each), stencilReadMask/WriteMask/Ref, fogStart/End/
    // Density, fogColor (a vector value: x, y, z, w and its name), fogMode, gpuProgramID, m_Tags, m_LOD, lighting.
    r.string()?;
    float_value(r, 8 * 7)?;
    r.bool()?;
    r.align();
    float_value(r, 5 + 3 + 12 + 3 + 3 + 4)?;
    r.string()?;
    r.bytes(8)?;
    tags(r)?;
    r.i32()?;
    r.bool()?;
    r.align();
    r.u32()?;
    for _ in 0..6 {
        for _ in 0..count(r)? {
            sub_program(r)?;
        }
    }
    r.bytes(2)?;
    r.align();
    r.string()?;
    r.string()?;
    r.string()?;
    tags(r)
}

/// SerializedSubProgram (2019.1-2020.3.1, parameters inline): m_BlobIndex, m_Channels {m_Channels, m_SourceMap},
/// m_GlobalKeywordIndices, m_LocalKeywordIndices, m_ShaderHardwareTier, m_GpuProgramType, the vector, matrix, texture
/// and buffer parameters, m_ConstantBuffers, m_ConstantBufferBindings, m_UAVParams, m_Samplers, m_ShaderRequirements.
fn sub_program(r: &mut Reader) -> Result<(), String> {
    r.u32()?;
    array(r, 2)?;
    r.u32()?;
    array(r, 2)?;
    array(r, 2)?;
    r.bytes(2)?;
    r.align();
    array(r, 16)?; // VectorParameter
    array(r, 16)?; // MatrixParameter
    array(r, 16)?; // TextureParameter
    array(r, 12)?; // BufferBinding
    // ConstantBuffer: m_NameIndex, m_MatrixParams, m_VectorParams, m_StructParams, m_Size.
    for _ in 0..count(r)? {
        r.i32()?;
        array(r, 16)?;
        array(r, 16)?;
        // StructParameter: m_NameIndex, m_Index, m_ArraySize, m_StructSize, m_VectorParams, m_MatrixParams.
        for _ in 0..count(r)? {
            r.bytes(16)?;
            array(r, 16)?;
            array(r, 16)?;
        }
        r.i32()?;
    }
    array(r, 12)?; // m_ConstantBufferBindings
    array(r, 12)?; // UAVParameter
    array(r, 8)?; // SamplerParameter
    r.i32()?;
    Ok(())
}

/// (file, shader name, id) for every Shader: 1.28.41's path id, or for a shader 1.28.41 does not have, 10000 + its path id
/// in the first build that has it (Jaap/GlisteningSprite from 1.28.52, Hidden/Custom/FirePost from 1.28.54).
pub const SHADER_IDS: &[(&str, &str, i64)] = &[
    ("globalgamemanagers.assets", "Legacy Shaders/VertexLit", 33),
    ("globalgamemanagers.assets", "Particles/Standard Unlit", 34),
    ("globalgamemanagers.assets", "Unlit/LightsTextureShader", 35),
    ("globalgamemanagers.assets", "Hidden/Custom/LightPost", 36),
    ("globalgamemanagers.assets", "Hidden/Custom/HeatEffect", 37),
    ("resources.assets", "Hidden/Linefy/DotsPixelPerfectBillboard", 124),
    ("resources.assets", "Hidden/Linefy/DotsTransparentWorldspaceBillboard", 125),
    ("resources.assets", "Hidden/Linefy/PolylineTransparentWorldspaceBillboard", 126),
    ("resources.assets", "Unlit/BloodWireShader", 127),
    ("resources.assets", "Hidden/Linefy/LinesTransparentPixelBillboard", 128),
    ("resources.assets", "Hidden/Linefy/LinesWorldspaceBillboard", 129),
    ("resources.assets", "Unlit/TwoSidedUnlit", 130),
    ("resources.assets", "Hidden/Linefy/LinesWorldspaceXY", 131),
    ("resources.assets", "Hidden/Linefy/DotsPixelBillboard", 132),
    ("resources.assets", "Hidden/Linefy/DotsTransparentWorldspaceXY", 133),
    ("resources.assets", "Hidden/Linefy/PolylinePixelBillboard", 134),
    ("resources.assets", "Hidden/Linefy/PolylineWorldspaceXY", 135),
    ("resources.assets", "Hidden/Linefy/LinesPixelBillboard", 136),
    ("resources.assets", "Hidden/Linefy/PolylineTransparentWorldspaceXY", 137),
    ("resources.assets", "Hidden/Linefy/PolylineWorldspaceBillboard", 138),
    ("resources.assets", "Hidden/Linefy/DotsTransparentPixelBillboard", 139),
    ("resources.assets", "Hidden/Linefy/PolylineTransparentPixelBillboard", 140),
    ("resources.assets", "Hidden/Linefy/DotsWorldspaceXY", 141),
    ("resources.assets", "Hidden/Linefy/LinesTransparentWorldspaceXY", 142),
    ("resources.assets", "Unlit/BrokenElectronics", 143),
    ("resources.assets", "Hidden/Linefy/LinesTransparentWorldspaceBillboard", 144),
    ("resources.assets", "TextMeshPro/Sprite", 145),
    ("resources.assets", "Hidden/Linefy/DefaultPolygonalMeshTransparent", 146),
    ("resources.assets", "Hidden/Linefy/DotsWorldspaceBillboard", 147),
    ("resources.assets", "Unlit/nixie_digit_shader", 148),
    ("resources.assets", "Hidden/Linefy/DotsTransparentPixelPerfectBillboard", 149),
    ("resources.assets", "Jaap/UseWire", 150),
    ("resources.assets", "Hidden/Linefy/DefaultPolygonalMesh", 151),
    ("sharedassets0.assets", "Legacy Shaders/Particles/Additive", 835),
    ("sharedassets0.assets", "Legacy Shaders/Particles/Alpha Blended Premultiply", 836),
    ("sharedassets0.assets", "Unlit/Transparent Cutout", 837),
    ("sharedassets0.assets", "Unlit/Texture", 838),
    ("sharedassets0.assets", "Hidden/PostProcessing/CopyStdFromTexArray", 839),
    ("sharedassets0.assets", "Hidden/PostProcessing/GrainBaker", 840),
    ("sharedassets0.assets", "Hidden/PostProcessing/DepthOfField", 841),
    ("sharedassets0.assets", "TextMeshPro/Bitmap", 842),
    ("sharedassets0.assets", "Unlit/SnowOverlay", 843),
    ("sharedassets0.assets", "Unlit/MinigunBarrelShader", 844),
    ("sharedassets0.assets", "Unlit/ReactorDisplay", 845),
    ("sharedassets0.assets", "Unlit/RayCannonShader", 846),
    ("sharedassets0.assets", "TextMeshPro/Mobile/Bitmap", 847),
    ("sharedassets0.assets", "Unlit/WaterShader", 848),
    ("sharedassets0.assets", "Jaap/LegacyBodyShader", 849),
    ("sharedassets0.assets", "Unlit/BathroomMirrorShader", 850),
    ("sharedassets0.assets", "Unlit/VertexColourCutout", 851),
    ("sharedassets0.assets", "Hidden/PostProcessing/MotionBlur", 852),
    ("sharedassets0.assets", "Hidden/PostProcessing/Debug/Waveform", 853),
    ("sharedassets0.assets", "Unlit/FoliageParticleShader", 854),
    ("sharedassets0.assets", "Hidden/PostProcessing/Texture2DLerp", 855),
    ("sharedassets0.assets", "Hidden/PostProcessing/Uber", 856),
    ("sharedassets0.assets", "Unlit/JetEngineShader", 857),
    ("sharedassets0.assets", "Unlit/LaserPointerBeam", 858),
    ("sharedassets0.assets", "Hidden/PostProcessing/DeferredFog", 859),
    ("sharedassets0.assets", "Unlit/BrokenVaseShader", 860),
    ("sharedassets0.assets", "Unlit/LiquidContainerShader", 861),
    ("sharedassets0.assets", "Hidden/PostProcessing/CopyStd", 862),
    ("sharedassets0.assets", "Unlit/UIFrameSheet", 863),
    ("sharedassets0.assets", "Unlit/LavaShader", 864),
    ("sharedassets0.assets", "Unlit/WorldspaceBackground", 865),
    ("sharedassets0.assets", "Hidden/PostProcessing/TemporalAntialiasing", 866),
    ("sharedassets0.assets", "Jaap/Wh", 867),
    ("sharedassets0.assets", "Unlit/AlphaCutoff", 868),
    ("sharedassets0.assets", "Hidden/PostProcessing/DiscardAlpha", 869),
    ("sharedassets0.assets", "Jaap/ForcefieldShader", 870),
    ("sharedassets0.assets", "Unlit/SyringeShader", 871),
    ("sharedassets0.assets", "Hidden/PostProcessing/MultiScaleVO", 872),
    ("sharedassets0.assets", "TextMeshPro/Distance Field", 873),
    ("sharedassets0.assets", "Unlit/DeflatedTyreShader", 874),
    ("sharedassets0.assets", "Hidden/PostProcessing/Lut2DBaker", 875),
    ("sharedassets0.assets", "Hidden/PostProcessing/SubpixelMorphologicalAntialiasing", 876),
    ("sharedassets0.assets", "Unlit/RotaryFanShader", 877),
    ("sharedassets0.assets", "Unlit/TempRayShader", 878),
    ("sharedassets0.assets", "Unlit/FoliageShader", 879),
    ("sharedassets0.assets", "Unlit/Obstruction", 880),
    ("sharedassets0.assets", "Unlit/Vila", 881),
    ("sharedassets0.assets", "Unlit/BurnedShader", 882),
    ("sharedassets0.assets", "Hidden/PostProcessing/Debug/Vectorscope", 883),
    ("sharedassets0.assets", "Unlit/ChainsawShader", 884),
    ("sharedassets0.assets", "Unlit/LED Display Shader", 885),
    ("sharedassets0.assets", "Hidden/PostProcessing/Debug/LightMeter", 886),
    ("sharedassets0.assets", "Hidden/PostProcessing/Debug/Overlays", 887),
    ("sharedassets0.assets", "Unlit/Jaap/PanelsShader", 888),
    ("sharedassets0.assets", "Unlit/GravitationalLensingShader", 889),
    ("sharedassets0.assets", "Hidden/PostProcessing/Bloom", 890),
    ("sharedassets0.assets", "Unlit/AdditiveGlowSprite", 891),
    ("sharedassets0.assets", "Jaap/Jukebox", 892),
    ("sharedassets0.assets", "Hidden/PostProcessing/Copy", 893),
    ("sharedassets0.assets", "Jaap/SeismicChargeImplosion", 894),
    ("sharedassets0.assets", "Unlit/LowFidelityLiquid", 895),
    ("sharedassets0.assets", "Unlit/HallwaySh", 896),
    ("sharedassets0.assets", "Hidden/PostProcessing/ScalableAO", 897),
    ("sharedassets0.assets", "Jaap/BodyShader", 898),
    ("sharedassets0.assets", "Unlit/DissolveMirror", 899),
    ("sharedassets0.assets", "Jaap/GlowingMetal", 900),
    ("sharedassets0.assets", "Hidden/PostProcessing/CopyStdFromDoubleWide", 901),
    ("sharedassets0.assets", "Jaap/GlowingSprite", 902),
    ("sharedassets0.assets", "Unlit/LoadingShader", 903),
    ("sharedassets0.assets", "Unlit/UvSlideSprite", 904),
    ("sharedassets0.assets", "Unlit/EnergySword", 905),
    ("sharedassets0.assets", "Unlit/FogShader", 906),
    ("sharedassets0.assets", "Hidden/PostProcessing/FinalPass", 907),
    ("sharedassets0.assets", "Hidden/PostProcessing/Debug/Histogram", 908),
    ("sharedassets0.assets", "Hidden/PostProcessing/ScreenSpaceReflections", 909),
    ("sharedassets0.assets", "Unlit/ResizableSliceShader", 910),
    ("sharedassets0.assets", "TextMeshPro/Mobile/Distance Field", 911),
    ("sharedassets0.assets", "Unlit/GeneratorShader", 912),
    ("sharedassets1.assets", "Unlit/SpriteOutlineGlowOverlayShader", 78),
    ("sharedassets1.assets", "Unlit/SaturationValueShader", 79),
    ("sharedassets1.assets", "Unlit/ThermalVisionReplacementShader", 80),
    ("sharedassets1.assets", "Unlit/ProceduralShatter", 81),
    ("sharedassets0.assets", "Jaap/GlisteningSprite", 10881),
    ("globalgamemanagers.assets", "Hidden/Custom/FirePost", 10035),
];
