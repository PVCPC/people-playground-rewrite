//! Each Shader's path id and its first SubShader's Queue tag, for materials with no custom render queue.

// HeatEffectRenderer.Init: Shader.Find("Hidden/Custom/HeatEffect"), the Composite's heat step (composite.wesl).
// HeatEffectRenderer.Render: _Intensity, _Frequency and _Speed from the blended settings (lights.rs blend_volumes), the frame blitted through the heat step (composite.wesl).
// ShaderProperties.Get: presentation (a cache of Shader.PropertyToID; the ids are renderer handles)
pub(crate) const SHADER_QUEUES: [(i64, i16); 158] = [
    (17, 2000), // Hidden/InternalErrorShader
    (19, 2000), // Hidden/Internal-StencilWrite
    (33, 2000), // Legacy Shaders/VertexLit
    (34, 2000), // Particles/Standard Unlit
    (35, 2000), // Unlit/LightsTextureShader
    (36, 2000), // Hidden/Custom/LightPost
    (37, 2000), // Hidden/Custom/HeatEffect
    // 1.28.54 adds Hidden/Custom/FirePost at 35 and moves the three after it up one (all Queue 2000, so 35-37 still hold).
    (38, 2000), // Hidden/Custom/HeatEffect (1.28.54)
    (62, 2000), // Hidden/Internal-DepthNormalsTexture
    (63, 2000), // Hidden/Internal-PrePassLighting
    (64, 2000), // Hidden/Internal-ScreenSpaceShadows
    (65, 2000), // Hidden/Internal-CombineDepthNormals
    (66, 2000), // Hidden/BlitCopy
    (67, 2000), // Hidden/BlitCopyDepth
    (68, 2000), // Hidden/ConvertTexture
    (69, 2000), // Hidden/Internal-DeferredShading
    (70, 2000), // Hidden/Internal-Loading
    (74, 2000), // Hidden/Internal-DeferredReflections
    (75, 2000), // Hidden/Internal-MotionVectors
    (78, 3001), // Unlit/SpriteOutlineGlowOverlayShader
    (79, 3000), // Unlit/SaturationValueShader
    (80, 2000), // Unlit/ThermalVisionReplacementShader
    (81, 3001), // Unlit/ProceduralShatter
    (102, 2000), // Hidden/Internal-Flare
    (105, 2000), // Hidden/Internal-Halo
    (107, 2000), // Hidden/BlitCopyWithDepth
    (109, 2000), // Hidden/BlitToDepth
    (110, 2000), // Hidden/BlitToDepth_MSAA
    (111, 2000), // Hidden/BlitCopyHDRTonemap
    (124, 2000), // Hidden/Linefy/DotsPixelPerfectBillboard
    (125, 3000), // Hidden/Linefy/DotsTransparentWorldspaceBillboard
    (126, 3000), // Hidden/Linefy/PolylineTransparentWorldspaceBillboard
    (127, 3001), // Unlit/BloodWireShader
    (128, 3000), // Hidden/Linefy/LinesTransparentPixelBillboard
    (129, 2000), // Hidden/Linefy/LinesWorldspaceBillboard
    (130, 2000), // Unlit/TwoSidedUnlit
    (131, 2000), // Hidden/Linefy/LinesWorldspaceXY
    (132, 2000), // Hidden/Linefy/DotsPixelBillboard
    (133, 3000), // Hidden/Linefy/DotsTransparentWorldspaceXY
    (134, 2000), // Hidden/Linefy/PolylinePixelBillboard
    (135, 2000), // Hidden/Linefy/PolylineWorldspaceXY
    (136, 2000), // Hidden/Linefy/LinesPixelBillboard
    (137, 3000), // Hidden/Linefy/PolylineTransparentWorldspaceXY
    (138, 2000), // Hidden/Linefy/PolylineWorldspaceBillboard
    (139, 3000), // Hidden/Linefy/DotsTransparentPixelBillboard
    (140, 3000), // Hidden/Linefy/PolylineTransparentPixelBillboard
    (141, 2000), // Hidden/Linefy/DotsWorldspaceXY
    (142, 3000), // Hidden/Linefy/LinesTransparentWorldspaceXY
    (143, 3001), // Unlit/BrokenElectronics
    (144, 3000), // Hidden/Linefy/LinesTransparentWorldspaceBillboard
    (145, 3000), // TextMeshPro/Sprite
    (146, 3000), // Hidden/Linefy/DefaultPolygonalMeshTransparent
    (147, 2000), // Hidden/Linefy/DotsWorldspaceBillboard
    (148, 3001), // Unlit/nixie_digit_shader
    (149, 3000), // Hidden/Linefy/DotsTransparentPixelPerfectBillboard
    (150, 3001), // Jaap/UseWire
    (151, 2000), // Hidden/Linefy/DefaultPolygonalMesh
    (835, 3000), // Legacy Shaders/Particles/Additive
    (836, 3000), // Legacy Shaders/Particles/Alpha Blended Premultiply
    (837, 2450), // Unlit/Transparent Cutout
    (838, 2000), // Unlit/Texture
    (839, 2000), // Hidden/PostProcessing/CopyStdFromTexArray
    (840, 2000), // Hidden/PostProcessing/GrainBaker
    (841, 2000), // Hidden/PostProcessing/DepthOfField
    (842, 3000), // TextMeshPro/Bitmap
    (843, 3001), // Unlit/SnowOverlay
    (844, 3001), // Unlit/MinigunBarrelShader
    (845, 3001), // Unlit/ReactorDisplay
    (846, 2000), // Unlit/RayCannonShader
    (847, 3000), // TextMeshPro/Mobile/Bitmap
    (848, 3001), // Unlit/WaterShader
    (849, 3001), // Jaap/LegacyBodyShader
    (850, 3001), // Unlit/BathroomMirrorShader
    (851, 3001), // Unlit/VertexColourCutout
    (852, 2000), // Hidden/PostProcessing/MotionBlur
    (853, 2000), // Hidden/PostProcessing/Debug/Waveform
    (854, 3001), // Unlit/FoliageParticleShader
    (855, 2000), // Hidden/PostProcessing/Texture2DLerp
    (856, 2000), // Hidden/PostProcessing/Uber
    (857, 3001), // Unlit/JetEngineShader
    (858, 2000), // Unlit/LaserPointerBeam
    (859, 2000), // Hidden/PostProcessing/DeferredFog
    (860, 3001), // Unlit/BrokenVaseShader
    (861, 3001), // Unlit/LiquidContainerShader
    (862, 2000), // Hidden/PostProcessing/CopyStd
    (863, 3000), // Unlit/UIFrameSheet
    (864, 3001), // Unlit/LavaShader
    (865, 3001), // Unlit/WorldspaceBackground
    (866, 2000), // Hidden/PostProcessing/TemporalAntialiasing
    (867, 3000), // Jaap/Wh
    (868, 3001), // Unlit/AlphaCutoff
    (869, 2000), // Hidden/PostProcessing/DiscardAlpha
    (870, 3001), // Jaap/ForcefieldShader
    (871, 3001), // Unlit/SyringeShader
    (872, 2000), // Hidden/PostProcessing/MultiScaleVO
    (873, 3000), // TextMeshPro/Distance Field
    (874, 3001), // Unlit/DeflatedTyreShader
    (875, 2000), // Hidden/PostProcessing/Lut2DBaker
    (876, 2000), // Hidden/PostProcessing/SubpixelMorphologicalAntialiasing
    (877, 3001), // Unlit/RotaryFanShader
    (878, 3001), // Unlit/TempRayShader
    (879, 3001), // Unlit/FoliageShader
    (880, 3001), // Unlit/Obstruction
    (881, 3001), // Unlit/Vila
    (882, 3001), // Unlit/BurnedShader
    (883, 2000), // Hidden/PostProcessing/Debug/Vectorscope
    (884, 3001), // Unlit/ChainsawShader
    (885, 3001), // Unlit/LED Display Shader
    (886, 2000), // Hidden/PostProcessing/Debug/LightMeter
    (887, 2000), // Hidden/PostProcessing/Debug/Overlays
    (888, 3001), // Unlit/Jaap/PanelsShader
    (889, 4001), // Unlit/GravitationalLensingShader
    (890, 2000), // Hidden/PostProcessing/Bloom
    (891, 3001), // Unlit/AdditiveGlowSprite
    (892, 3001), // Jaap/Jukebox
    (893, 2000), // Hidden/PostProcessing/Copy
    (894, 3000), // Jaap/SeismicChargeImplosion
    (895, 3001), // Unlit/LowFidelityLiquid
    (896, 3001), // Unlit/HallwaySh
    (897, 2000), // Hidden/PostProcessing/ScalableAO
    (898, 3001), // Jaap/BodyShader
    (899, 3001), // Unlit/DissolveMirror
    (900, 3001), // Jaap/GlowingMetal
    (901, 2000), // Hidden/PostProcessing/CopyStdFromDoubleWide
    (902, 3001), // Jaap/GlowingSprite
    (903, 3000), // Unlit/LoadingShader
    (904, 3001), // Unlit/UvSlideSprite
    (905, 3001), // Unlit/EnergySword
    (906, 3001), // Unlit/FogShader
    (907, 2000), // Hidden/PostProcessing/FinalPass
    (908, 2000), // Hidden/PostProcessing/Debug/Histogram
    (909, 2000), // Hidden/PostProcessing/ScreenSpaceReflections
    (910, 3001), // Unlit/ResizableSliceShader
    (911, 3000), // TextMeshPro/Mobile/Distance Field
    (912, 3001), // Unlit/GeneratorShader
    (9000, 2000), // Hidden/Internal-GUITextureClip
    (9001, 2000), // Hidden/Internal-GUITextureClipText
    (9002, 2000), // Hidden/Internal-GUITexture
    (9003, 2000), // Hidden/Internal-GUITextureBlit
    (9004, 2000), // Hidden/Internal-GUIRoundedRect
    (9007, 2000), // Hidden/Internal-GUIRoundedRectWithColorPerBorder
    (9100, 3000), // Hidden/Internal-UIRDefault
    (9101, 2000), // Hidden/Internal-UIRAtlasBlitCopy
    (9102, 3000), // Hidden/Internal-UIRDefaultWorld
    (10101, 3000), // GUI/Text Shader
    (10753, 3000), // Sprites/Default
    (10755, 2000), // Hidden/FrameDebuggerRenderTargetDisplay
    (10757, 3000), // Sprites/Mask
    (10770, 3000), // UI/Default
    (15304, 2000), // Hidden/VR/BlitTexArraySlice
    (15308, 2000), // Hidden/Internal-ODSWorldTexture
    (15309, 2000), // Hidden/Internal-CubemapToEquirect
    (15312, 2000), // Hidden/VR/BlitFromTex2DToTexArraySlice
    (16000, 3000), // Hidden/VideoComposite
    (16001, 2000), // Hidden/VideoDecode
    (17000, 2000), // Hidden/Compositing
    (19011, 2000), // Hidden/TextCore/Distance Field SSD
    (4800000, 2000), // Hidden/Custom/HeatEffect
];

/// A FloatParameter of the post-process settings (ParameterOverride<float>): value and overrideState.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(dead_code)]
pub struct FloatParameter {
    pub value: f32,
    pub override_state: bool,
}

/// HeatEffect's post-process settings, read only by HeatEffectRenderer.Render.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub struct HeatEffect {
    pub Intensity: FloatParameter,
    pub Frequency: FloatParameter,
    pub Speed: FloatParameter,
}

impl HeatEffect {
    /// HeatEffect..ctor: the settings' defaults, Intensity 0, Frequency 5, Speed -1, none overriding (lights.rs blend_volumes starts from them).
    #[allow(dead_code)]
    pub fn new() -> Self {
        let parameter = |value| FloatParameter { value, override_state: false };
        HeatEffect { Intensity: parameter(0.0), Frequency: parameter(5.0), Speed: parameter(-1.0) }
    }
}

#[cfg(test)]
mod heat_effect_tests {
    #[test]
    fn heat_effect_defaults() {
        let h = super::HeatEffect::new();
        assert_eq!((h.Intensity.value, h.Frequency.value, h.Speed.value), (0.0, 5.0, -1.0));
        assert!(!h.Intensity.override_state && !h.Frequency.override_state && !h.Speed.override_state);
    }
}
