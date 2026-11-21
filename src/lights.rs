//! The game's cameras: Main Camera draws the scene, LightsCamera draws the LightSprite layer (half size, additive) into a
//! texture, and LightsEffect multiplies the two (Main.unity's cameras, Hidden/Custom/LightPost).

use bevy::{
    asset::embedded_asset,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    camera::{visibility::RenderLayers, ClearColorConfig, Hdr, ImageRenderTarget, RenderTarget, ScalingMode},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    render::{
        render_resource::{AsBindGroup, ShaderType, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureFormat},

    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin},
    window::PrimaryWindow,
};

/// GameObject layer LightSprite (LightsCamera's cullingMask).
pub(crate) const LIGHT_SPRITE: i32 = 15;
/// GameObject layer ScreenUI (On screen UI camera's cullingMask).
const SCREEN_UI: i32 = 16;
/// The render layer of On screen UI camera (the bloom passes take 5 and up).
const SCREEN_UI_LAYER: usize = 31;
/// The HeatEffect blit onto the window.
const HEAT_LAYER: usize = 30;
/// GrainRenderer's lookup bake.
const GRAIN_LAYER: usize = 29;
/// The display's upscale of a smaller screen (RenderScale).
const PRESENT_LAYER: usize = 32;
/// Main Camera's cullingMask.
const MAIN_MASK: u32 = 12087;
/// LightsEffect's Intensity, MinBrightness and MaxBrightness.
#[derive(Debug, Default)]
pub(crate) struct LightsEffect {
    intensity: f32,
    min_brightness: f32,
    max_brightness: f32,
}

impl LightsEffect {
    /// LightsEffect..ctor: the FloatParameters' initial values (LightTexture is set by the light camera each frame).
    pub(crate) const fn new() -> Self {
        LightsEffect { intensity: 1.0, min_brightness: 0.05, max_brightness: 100.0 }
    }

    /// DEFAULT POST PROCESSING PROFILE's LightsEffect: the constructed settings, then its serialized values.
    const fn profile() -> Self {
        let mut s = Self::new();
        s.intensity = 1.0;
        s.min_brightness = 0.025;
        s.max_brightness = 100.0;
        s
    }
}

/// The camera the game's Main Camera is: the world view scripts and the mouse go through.
#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
struct LightsCamera;

/// On screen UI camera: the ScreenUI layer in world space over the post stack (Main Camera's view).
#[derive(Component)]
struct ScreenUiCamera;

#[derive(Resource)]
struct Targets {
    lit_material: Handle<Lit>,
    scene: Handle<Image>,
    lights: Handle<Image>,
    lit: Handle<Image>,
    exposure: Handle<Image>,
    /// Unity's whiteTexture: _AutoExposureTex while AutoExposure is off (the Menu scene's profile).
    white: Handle<Image>,
    quad: Handle<Mesh>,
    composite: Handle<Composite>,
    /// The stack's result before the AfterStack HeatEffect.
    graded: Handle<Image>,
    heat: Handle<Heat>,
    baker: Handle<GrainBaker>,
    /// The whole screen (post stack's end and UI) while it renders below the window's size.
    screen: Handle<Image>,
}

/// The display scaling a smaller screen up to the window (RenderScale below 1).
#[derive(Component)]
struct Present;

/// A camera or quad of the bloom pyramid, respawned when the window resizes.
#[derive(Component)]
struct BloomPass;

/// The grain lookup's camera: it bakes while Grain runs.
#[derive(Component)]
struct GrainPass;

/// The camera that draws the post stack's end (HeatEffect, AfterStack) onto the window, under the UI.
#[derive(Component)]
struct FinalPass;

/// VideoSettingsInitBehaviour.Sync: PostProcessLayer.antialiasingMode is SubpixelMorphologicalAntialiasing at quality High
/// while Preferences.SMAA, else None. The layer's final pass runs SMAA after the AfterStack effects (HeatEffect), so it
/// goes on the camera that draws them; the screen-space UI draws later, unfiltered.
fn smaa(mut commands: Commands, settings: Option<Res<crate::ui::Settings>>, camera: Single<(Entity, Has<bevy::anti_alias::smaa::Smaa>), With<FinalPass>>) {
    let on = settings.is_some_and(|s| s.flag("SMAA"));
    let (e, has) = *camera;
    if on && !has {
        commands.entity(e).insert(bevy::anti_alias::smaa::Smaa { preset: bevy::anti_alias::smaa::SmaaPreset::High });
    } else if !on && has {
        commands.entity(e).remove::<bevy::anti_alias::smaa::Smaa>();
    }
}

/// Unlit/AdditiveGlowSprite. `color` is everything but the texture.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Glow {
    #[uniform(0)]
    pub color: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material2d for Glow {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/glow.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &bevy::sprite_render::Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Blend One One.
        let add = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(Some(target)) = descriptor.fragment.as_mut().and_then(|f| f.targets.first_mut()) {
            target.blend = Some(BlendState { color: add, alpha: add });
        }
        Ok(())
    }
}

/// Unlit/ThermalVisionReplacementShader: ThermalVisionBehaviour's materialInstance with its _HeatData.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Thermal {
    #[texture(0)]
    #[sampler(1)]
    heat: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    gradient: Handle<Image>,
}

impl Material2d for Thermal {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/thermal.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &bevy::sprite_render::Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Blend OneMinusDstColor One (colour and alpha).
        let screen = BlendComponent { src_factor: BlendFactor::OneMinusDst, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(Some(target)) = descriptor.fragment.as_mut().and_then(|f| f.targets.first_mut()) {
            target.blend = Some(BlendState { color: screen, alpha: screen });
        }
        Ok(())
    }
}

#[derive(Component)]
struct ThermalSprite(Handle<Thermal>, u32);

/// Main.unity's "Thermal vision controller" (ScreenUI layer): its SpriteRenderer (Square, enabled with ThermalVisionEnabled) with
/// the heat texture after each Apply, and FitCameraBackgroundBehaviour.OnWillRenderObject: position = the camera's x and y,
/// localScale = orthographicSize * 2 * (aspect, 1) / the sprite's size (Square: 1 unit). isVisible is the enabled flag: the
/// quad covers the view.
fn thermal_vision(
    mut commands: Commands,
    play: Option<ResMut<crate::play::Play>>,
    window: Single<&Window, With<PrimaryWindow>>,
    main: Single<(&Transform, &Projection), (With<MainCamera>, Without<ThermalSprite>)>,
    mut sprite: Query<(&mut Transform, &mut Visibility, &mut ThermalSprite), Without<MainCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut thermals: ResMut<Assets<Thermal>>,
    mut textures: Local<crate::play::Textures>,
) {
    // ThermalVisionBehaviour.OnDestroy (the Main scene unloads): the sprite goes undrawn with it.
    let hide = |sprite: &mut Query<(&mut Transform, &mut Visibility, &mut ThermalSprite), Without<MainCamera>>| sprite.iter_mut().for_each(|mut s| *s.1 = Visibility::Hidden);
    let Some(mut play) = play else { return hide(&mut sprite) };
    let g = &mut play.game;
    let enabled = g.thermal_vision == Some(true);
    let Some(t) = g.thermal_texture.as_mut() else { return hide(&mut sprite) };
    let (camera, projection) = *main;
    let Projection::Orthographic(o) = projection else { return };
    let aspect = window.physical_width().max(1) as f32 / window.physical_height().max(1) as f32;
    t.visible = enabled;
    if enabled {
        t.position = [camera.translation.x, camera.translation.y];
        t.scale = [o.scale * 2.0 * aspect, o.scale * 2.0];
    }
    let heat_rows = |data: &[u8]| data.chunks(128).rev().flatten().copied().collect::<Vec<u8>>();
    let Ok((mut transform, mut visibility, mut s)) = sprite.single_mut() else {
        let Some(db) = g.assets.as_deref() else { return };
        let Some(material) = t.material else { return };
        let m = crate::assets::components::Material::read(db.data(material)).unwrap_or_else(|e| panic!("the thermal vision material: {e}"));
        let gradient = m.textures.iter().find(|x| x.0 == "_Gradient").and_then(|x| db.resolve(material.file, x.1)).expect("the thermal vision material's _Gradient");
        let Some((gradient, _)) = crate::play::load_texture(db, &mut images, &mut textures, gradient) else { return };
        // ThermalVisionBehaviour.SetupTexture: new Texture2D(64, 64, RG16, no mips), wrapMode Clamp (filter Bilinear), the material's _HeatData.
        let mut heat = Image::new(Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, bevy::render::render_resource::TextureDimension::D2, heat_rows(&t.data), TextureFormat::Rg8Unorm, bevy::asset::RenderAssetUsages::default());
        heat.sampler = ImageSampler::linear();
        let material = thermals.add(Thermal { heat: images.add(heat), gradient });
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(material.clone()),
            ThermalSprite(material, t.version),
            Transform::from_xyz(t.position[0], t.position[1], 0.0).with_scale(Vec3::new(t.scale[0], t.scale[1], 1.0)),
            if enabled { Visibility::Visible } else { Visibility::Hidden },
            RenderLayers::layer(SCREEN_UI_LAYER),
        ));
        return;
    };
    *visibility = if enabled { Visibility::Visible } else { Visibility::Hidden };
    (transform.translation.x, transform.translation.y) = (t.position[0], t.position[1]);
    transform.scale = Vec3::new(t.scale[0], t.scale[1], 1.0);
    if s.1 != t.version {
        s.1 = t.version;
        let heat = thermals.get(&s.0).map(|m| m.heat.clone());
        if let Some(mut image) = heat.and_then(|h| images.get_mut(&h)) {
            image.data = Some(heat_rows(&t.data));
        }
    }
}

/// Unlit/Jaap/PanelsShader's properties.
#[derive(ShaderType, Clone)]
pub struct PanelSettings {
    pub offset: Vec2,
    pub scrolling_speed: Vec2,
    pub fade: Vec4,
    pub panels: Vec2,
    pub scale: f32,
    pub colour_variation: f32,
    pub alpha: f32,
    /// _Time.y: the scene's Time.timeSinceLevelLoad (`panels_time`).
    pub time: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Panels {
    #[uniform(0)]
    pub settings: PanelSettings,
    #[texture(1)]
    #[sampler(2)]
    pub main: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub panels: Handle<Image>,
}

impl Material2d for Panels {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/panels.wesl".into()
    }
}

/// Hidden/Custom/LightPost.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Lit {
    #[uniform(0)]
    settings: Vec4,
    #[texture(1)]
    #[sampler(2)]
    scene: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    lights: Handle<Image>,
}

impl Material2d for Lit {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/lit.wesl".into()
    }
}

#[derive(ShaderType, Clone, Copy)]
struct BloomSettings {
    threshold: Vec4,
    texel: Vec2,
    clamp_to: f32,
    sample_scale: f32,
    pass_index: u32,
}

/// Hidden/PostProcessing/Bloom.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Bloom {
    #[uniform(0)]
    settings: BloomSettings,
    #[texture(1)]
    #[sampler(2)]
    source: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    bloom: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    exposure: Handle<Image>,
}

impl Material2d for Bloom {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/bloom.wesl".into()
    }
}

/// The post stack's last step: Hidden/PostProcessing/Uber. `settings` is _BloomTex_TexelSize.xy, then _Bloom_Settings' sample scale
/// and intensity.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Composite {
    #[uniform(0)]
    settings: Vec4,
    #[texture(1)]
    #[sampler(2)]
    lit: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    bloom: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    exposure: Handle<Image>,
    #[uniform(7)]
    video: Vec4,
    #[uniform(8)]
    grade: Grade,
    #[uniform(9)]
    fx: Fx,
    #[texture(10)]
    #[sampler(11)]
    grain: Handle<Image>,
}

/// The blended ColorGrading's Lut3DBaker inputs (ColorGradingRenderer.RenderHDRPipeline3D) and Bloom.color.linear.
#[derive(ShaderType, Clone, Copy, PartialEq, Default)]
struct Grade {
    balance: Vec4,
    filter: Vec4,
    hue_sat_con: Vec4,
    lift: Vec4,
    inv_gamma: Vec4,
    gain: Vec4,
    bloom_color: Vec4,
}

/// Uber's DISTORT, CHROMATIC_ABERRATION and GRAIN inputs (LensDistortionRenderer, ChromaticAberrationRenderer, GrainRenderer).
#[derive(ShaderType, Clone, Copy, PartialEq, Default)]
struct Fx {
    distortion_amount: Vec4,
    distortion_center_scale: Vec4,
    /// _ChromaticAberration_Amount (0 without the effect), 1 with LensDistortion, 1 with Grain.
    flags: Vec4,
    /// _Grain_Params1 (lumContrib, intensity * 20).
    grain1: Vec4,
    grain2: Vec4,
    /// _MainTex_TexelSize.zw.
    main_size: Vec4,
}

/// Hidden/PostProcessing/GrainBaker into GrainRenderer's 128x128 lookup (bilinear, repeat): _Phase, then 1 for the colored pass.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct GrainBaker {
    #[uniform(0)]
    settings: Vec4,
}

impl Material2d for GrainBaker {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/grain_baker.wesl".into()
    }
}

/// HeatEffect (PostProcessEvent.AfterStack, Hidden/Custom/HeatEffect): HeatEffectRenderer.Render's BlitFullscreenTriangle of the
/// stack's result (a bilinear, clamped RenderTexture) to the screen.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Heat {
    /// Intensity, Frequency, Speed, and 1 when the effect runs.
    #[uniform(0)]
    heat: Vec4,
    /// _Time.z, unity_OrthoParams.x.
    #[uniform(1)]
    view: Vec4,
    #[texture(2)]
    #[sampler(3)]
    source: Handle<Image>,
}

impl Material2d for Heat {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/heat.wesl".into()
    }
}

/// The settings the post volumes touch (PostProcessManager.UpdateSettings' result for layer 8), at their constructed defaults:
/// ColorGrading, Bloom, Grain, ChromaticAberration, LensDistortion and HeatEffect.
#[derive(Clone, Copy)]
struct Grading {
    temperature: f32,
    tint: f32,
    color_filter: Vec4,
    hue_shift: f32,
    saturation: f32,
    contrast: f32,
    post_exposure: f32,
    lift: Vec4,
    gamma: Vec4,
    gain: Vec4,
    bloom_enabled: bool,
    bloom_intensity: f32,
    bloom_threshold: f32,
    bloom_soft_knee: f32,
    bloom_diffusion: f32,
    bloom_anamorphic: f32,
    bloom_color: Vec4,
    bloom_fast: bool,
    grain_enabled: bool,
    grain_colored: bool,
    grain_intensity: f32,
    grain_size: f32,
    grain_lum: f32,
    chroma_enabled: bool,
    chroma_intensity: f32,
    lens_enabled: bool,
    lens_intensity: f32,
    heat_intensity: f32,
    heat_frequency: f32,
    heat_speed: f32,
    heat_enabled: bool,
}

/// PostProcessManager.UpdateSettings for the Main Camera's layer (8): every setting at its default, then each enabled global
/// volume with a profile and a weight above 0, sorted by priority with a stable insertion sort over the registration (OnEnable)
/// order: the Main scene's HeatEffect (2239, priority 1), ColdEffect (2242, 0) and post (2403, `base`: DEFAULT POST PROCESSING PROFILE,
/// 0, weight 1), then `volumes` (the map's, registered after them: MapLoaderBehaviour runs at execution order 1200; the fog's
/// and the Reactor's when they are enabled). OverrideSettings(profile, Clamp01(weight)) skips a setting that is not `active`
/// and lerps each overridden parameter (ParameterOverride.Interp; a bool or enum takes the target as t > 0). MapConfig.ApplySettings
/// sets the heat and cold weights; `post_exposure` and `bloom_mode` are VideoSettingsInitBehaviour.SetPost's on the default profile.
/// In the Menu scene `base` is its one volume (Main Camera/post: MAIN MENU POST PROCESSING PROFILE, global, weight 1) and the
/// heat and cold weights are 0 (no such volumes there).
fn blend_volumes(base: &str, heat: f32, cold: f32, post_exposure: f32, bloom_mode: f32, volumes: &[crate::game::map::PostVolume]) -> Grading {
    let mut g = Grading {
        temperature: 0.0,
        tint: 0.0,
        color_filter: Vec4::ONE,
        hue_shift: 0.0,
        saturation: 0.0,
        contrast: 0.0,
        post_exposure: 0.0,
        lift: Vec4::new(1.0, 1.0, 1.0, 0.0),
        gamma: Vec4::new(1.0, 1.0, 1.0, 0.0),
        gain: Vec4::new(1.0, 1.0, 1.0, 0.0),
        bloom_enabled: false,
        bloom_intensity: 0.0,
        bloom_threshold: 1.0,
        bloom_soft_knee: 0.5,
        bloom_diffusion: 7.0,
        bloom_anamorphic: 0.0,
        bloom_color: Vec4::ONE,
        bloom_fast: false,
        grain_enabled: false,
        grain_colored: true,
        grain_intensity: 0.0,
        grain_size: 1.0,
        grain_lum: 0.8,
        chroma_enabled: false,
        chroma_intensity: 0.0,
        lens_enabled: false,
        lens_intensity: 0.0,
        heat_intensity: 0.0,
        heat_frequency: 5.0,
        heat_speed: -1.0,
        heat_enabled: false,
    };
    let mut list = vec![("HEAT EFFECT", 1.0, heat), ("COLD EFFECT", 0.0, cold), (base, 0.0, 1.0)];
    list.extend(volumes.iter().map(|v| (v.profile.as_str(), v.priority, v.weight)));
    list.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (profile, _, weight) in list {
        if weight > 0.0 {
            override_settings(&mut g, profile, weight.min(1.0), post_exposure, bloom_mode);
        }
    }
    g
}

/// PostProcessManager.OverrideSettings of one profile at `t`: its active settings' overridden parameters (every one of them has
/// `enabled` overridden to true).
fn override_settings(g: &mut Grading, profile: &str, t: f32, post_exposure: f32, bloom_mode: f32) {
    let l = |a: &mut f32, b: f32| *a += (b - *a) * t;
    let l4 = |a: &mut Vec4, b: Vec4| *a += (b - *a) * t;
    match profile {
        // ColorGrading temperature -12.2, saturation -34.4, postExposure 0.5, contrast 7.4; Bloom threshold 0.18, color
        // (0.9575472, 0.99002594, 1, 1).
        "COLD EFFECT" => {
            l(&mut g.temperature, -12.2);
            l(&mut g.saturation, -34.4);
            l(&mut g.post_exposure, 0.5);
            l(&mut g.contrast, 7.4);
            g.bloom_enabled = true;
            l(&mut g.bloom_threshold, 0.18);
            l4(&mut g.bloom_color, Vec4::new(0.9575472, 0.99002594, 1.0, 1.0));
        }
        // Bloom intensity 0.95, softKnee 0; SetPost: BloomMode Off sets the Bloom inactive, Fast and Fancy override fastMode.
        // ColorGrading postExposure (SetPost's), gain (0.84853494, 0.83429074, 1, 0); the tonemapper is SetPost's (the
        // Composite's `neutral`).
        "DEFAULT POST PROCESSING PROFILE" => {
            if bloom_mode != 0.0 {
                g.bloom_enabled = true;
                l(&mut g.bloom_intensity, 0.95);
                l(&mut g.bloom_soft_knee, 0.0);
                g.bloom_fast = bloom_mode == 1.0;
            }
            l(&mut g.post_exposure, post_exposure);
            l4(&mut g.gain, Vec4::new(0.84853494, 0.83429074, 1.0, 0.0));
        }
        // The Menu scene's: Bloom intensity 0.95, softKnee 0 and ColorGrading gain (0.84853494, 0.83429074, 1, 0) as the default
        // profile, saturation 4; SetPost's bloom mode and postExposure (Menu.unity's Init runs VideoSettingsInitBehaviour on every
        // volume). Its AutoExposure is overridden off and its LightsEffect inactive: `follow` binds the white exposure and passes
        // the scene through the Lit pass. MotionBlur, ChromaticAberration, Vignette and DepthOfField are inactive.
        "MAIN MENU POST PROCESSING PROFILE" => {
            if bloom_mode != 0.0 {
                g.bloom_enabled = true;
                l(&mut g.bloom_intensity, 0.95);
                l(&mut g.bloom_soft_knee, 0.0);
                g.bloom_fast = bloom_mode == 1.0;
            }
            l(&mut g.saturation, 4.0);
            l(&mut g.post_exposure, post_exposure);
            l4(&mut g.gain, Vec4::new(0.84853494, 0.83429074, 1.0, 0.0));
        }
        // ColorGrading temperature 30.4, colorFilter (1, 0.76981723, 0.7411765, 1), saturation 46, postExposure 0.34, contrast
        // -37.6, gamma (1, 0.8652071, 0.79544574, 0); HeatEffect Intensity 0.004, Frequency 5, Speed -1.
        "HEAT EFFECT" => {
            l(&mut g.temperature, 30.4);
            l4(&mut g.color_filter, Vec4::new(1.0, 0.76981723, 0.7411765, 1.0));
            l(&mut g.saturation, 46.0);
            l(&mut g.post_exposure, 0.34);
            l(&mut g.contrast, -37.6);
            l4(&mut g.gamma, Vec4::new(1.0, 0.8652071, 0.79544574, 0.0));
            g.heat_enabled = true;
            l(&mut g.heat_intensity, 0.004);
            l(&mut g.heat_frequency, 5.0);
            l(&mut g.heat_speed, -1.0);
        }
        // Bloom intensity 1, threshold 0.6, softKnee 0.5, diffusion 6.43, anamorphicRatio 1; ColorGrading temperature -12.5.
        "SNOW POST PROCESSING PROFILE" => {
            l(&mut g.temperature, -12.5);
            g.bloom_enabled = true;
            l(&mut g.bloom_intensity, 1.0);
            l(&mut g.bloom_threshold, 0.6);
            l(&mut g.bloom_soft_knee, 0.5);
            l(&mut g.bloom_diffusion, 6.43);
            l(&mut g.bloom_anamorphic, 1.0);
        }
        // ColorGrading colorFilter (1.0093603, 1.0592737, 1.0149062, 1), saturation -10.
        "SUBSTRUCTURE POST PROCESSING PROFILE" => {
            l4(&mut g.color_filter, Vec4::new(1.0093603, 1.0592737, 1.0149062, 1.0));
            l(&mut g.saturation, -10.0);
        }
        // ColorGrading colorFilter (0.6919756, 0.5411765, 1, 1), postExposure 0, contrast 12.9; ChromaticAberration intensity
        // 0.183 (no spectralLut); Grain colored, intensity 0.219, size 3, lumContrib 1; LensDistortion intensity -9.1.
        "UNCHARTED POST PROCESSING PROFILE" => {
            l4(&mut g.color_filter, Vec4::new(0.6919756, 0.5411765, 1.0, 1.0));
            l(&mut g.post_exposure, 0.0);
            l(&mut g.contrast, 12.9);
            g.chroma_enabled = true;
            l(&mut g.chroma_intensity, 0.183);
            g.grain_enabled = true;
            g.grain_colored = true;
            l(&mut g.grain_intensity, 0.219);
            l(&mut g.grain_size, 3.0);
            l(&mut g.grain_lum, 1.0);
            g.lens_enabled = true;
            l(&mut g.lens_intensity, -9.1);
        }
        // Bloom intensity 1.5, threshold 0, diffusion 7.95, color white, fastMode; ColorGrading saturation -11.8, postExposure
        // -0.69, contrast -26.4.
        "FOG PROFILE" => {
            g.bloom_enabled = true;
            l(&mut g.bloom_intensity, 1.5);
            l(&mut g.bloom_threshold, 0.0);
            l(&mut g.bloom_diffusion, 7.95);
            l4(&mut g.bloom_color, Vec4::ONE);
            g.bloom_fast = true;
            l(&mut g.saturation, -11.8);
            l(&mut g.post_exposure, -0.69);
            l(&mut g.contrast, -26.4);
        }
        // Bloom intensity 1.5, threshold 0, diffusion 7.53, anamorphicRatio 0.75, color white, fastMode; ColorGrading colorFilter
        // (0.7266863, 1, 0.5235849, 1), saturation -11.8, postExposure 1, contrast -8.8; Grain monochrome, intensity 0.079,
        // size 2.4, lumContrib 1.
        "Meltdown profiel" => {
            g.bloom_enabled = true;
            l(&mut g.bloom_intensity, 1.5);
            l(&mut g.bloom_threshold, 0.0);
            l(&mut g.bloom_diffusion, 7.53);
            l(&mut g.bloom_anamorphic, 0.75);
            l4(&mut g.bloom_color, Vec4::ONE);
            g.bloom_fast = true;
            l4(&mut g.color_filter, Vec4::new(0.7266863, 1.0, 0.5235849, 1.0));
            l(&mut g.saturation, -11.8);
            l(&mut g.post_exposure, 1.0);
            l(&mut g.contrast, -8.8);
            g.grain_enabled = true;
            g.grain_colored = false;
            l(&mut g.grain_intensity, 0.079);
            l(&mut g.grain_size, 2.4);
            l(&mut g.grain_lum, 1.0);
        }
        _ => unreachable!("post profile {profile}"),
    }
}

/// Mathf.GammaToLinearSpace.
fn gamma_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else if v < 1.0 {
        ((v + 0.055) / 1.055).powf(2.4)
    } else {
        v.powf(2.2)
    }
}

/// ColorUtilities.ComputeColorBalance.
fn color_balance(temperature: f32, tint: f32) -> Vec3 {
    let (t1, t2) = (temperature / 60.0, tint / 60.0);
    let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.27509508 + t2 * 0.05;
    // CIExyToLMS
    let (big_x, big_z) = (x / y, (1.0 - x - y) / y);
    let lms = Vec3::new(0.7328 * big_x + 0.4296 - 0.1624 * big_z, -0.7036 * big_x + 1.6975 + 0.0061 * big_z, 0.003 * big_x + 0.0136 + 0.9834 * big_z);
    Vec3::new(0.949237, 1.03542, 1.08728) / lms
}

/// ColorUtilities.ColorToLift, ColorToInverseGamma and ColorToGain: the colour less its luminance, plus w (lift) or 1 + w.
fn less_luma(c: Vec4) -> Vec3 {
    let v = c.truncate();
    v - Vec3::splat(v.x * 0.2126 + v.y * 0.7152 + v.z * 0.0722)
}

/// The Composite's Grade for the blended settings: RenderHDRPipeline3D's lift * 0.2, gain and gamma * 0.8.
fn grade_uniform(g: &Grading) -> Grade {
    let lift = less_luma(g.lift * 0.2) + Vec3::splat(g.lift.w * 0.2);
    let gamma = less_luma(g.gamma * 0.8) + Vec3::splat(g.gamma.w * 0.8 + 1.0);
    let gain = less_luma(g.gain * 0.8) + Vec3::splat(g.gain.w * 0.8 + 1.0);
    let c = g.bloom_color;
    Grade {
        balance: color_balance(g.temperature, g.tint).extend(0.0),
        filter: g.color_filter,
        hue_sat_con: Vec4::new(g.hue_shift / 360.0, g.saturation / 100.0 + 1.0, g.contrast / 100.0 + 1.0, 0.0),
        lift: lift.extend(0.0),
        inv_gamma: (Vec3::ONE / gamma.max(Vec3::splat(0.001))).extend(0.0),
        gain: gain.extend(0.0),
        bloom_color: Vec4::new(gamma_to_linear(c.x), gamma_to_linear(c.y), gamma_to_linear(c.z), c.w),
    }
}

/// HeatEffectRenderer.Render's _Intensity, _Frequency, _Speed (and whether the effect runs), then _Time.z and unity_OrthoParams.x.
fn heat_uniform(g: &Grading, time: f32, ortho_width: f32) -> (Vec4, Vec4) {
    (Vec4::new(g.heat_intensity, g.heat_frequency, g.heat_speed, g.heat_enabled as u32 as f32), Vec4::new(time * 2.0, ortho_width, 0.0, 0.0))
}

/// RuntimeUtilities' HaltonSeq.Get.
fn halton(mut index: u32, radix: u32) -> f32 {
    let (mut result, mut fraction) = (0.0, 1.0 / radix as f32);
    while index > 0 {
        result += (index % radix) as f32 * fraction;
        index /= radix;
        fraction /= radix as f32;
    }
    result
}

/// The Uber inputs of LensDistortionRenderer, ChromaticAberrationRenderer and GrainRenderer (sample `index` of its Halton offsets)
/// for those effects that run (their IsEnabledAndSupported).
fn fx_uniform(g: &Grading, screen: (u32, u32), index: u32) -> Fx {
    let lens = g.lens_enabled && g.lens_intensity != 0.0;
    let theta = (1.6 * g.lens_intensity.abs().max(1.0)).min(160.0).to_radians();
    let distortion_amount = Vec4::new(if g.lens_intensity >= 0.0 { theta } else { 1.0 / theta }, 2.0 * (theta * 0.5).tan(), 1.0, g.lens_intensity);
    let chroma = if g.chroma_enabled && g.chroma_intensity > 0.0 { g.chroma_intensity * 0.05 } else { 0.0 };
    let grain = g.grain_enabled && g.grain_intensity > 0.0;
    let (w, h) = (screen.0 as f32, screen.1 as f32);
    Fx {
        distortion_amount,
        // centerX, centerY, intensityX, intensityY (the defaults: no profile sets them).
        distortion_center_scale: Vec4::new(0.0, 0.0, 1.0, 1.0),
        flags: Vec4::new(chroma, lens as u32 as f32, grain as u32 as f32, 0.0),
        grain1: Vec4::new(g.grain_lum, g.grain_intensity * 20.0, 0.0, 0.0),
        grain2: Vec4::new(w / 128.0 / g.grain_size, h / 128.0 / g.grain_size, halton(index & 0x3FF, 2), halton(index & 0x3FF, 3)),
        main_size: Vec4::new(w, h, 0.0, 0.0),
    }
}

impl Material2d for Composite {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/composite.wesl".into()
    }
}

/// Which camera draws a GameObject layer: 0 Main, 1 Lights, 2 the final view (nothing else is lit), 3 none.
pub fn camera_layers(game_object_layer: i32) -> RenderLayers {
    RenderLayers::layer(match game_object_layer {
        LIGHT_SPRITE => 1,
        SCREEN_UI => SCREEN_UI_LAYER,
        l if MAIN_MASK >> l & 1 == 1 => 0,
        _ => 3,
    })
}

pub fn add(app: &mut App) {
    crate::exposure::add(app);
    embedded_asset!(app, "glow.wesl");
    embedded_asset!(app, "composite.wesl");
    embedded_asset!(app, "heat.wesl");
    embedded_asset!(app, "grain_baker.wesl");
    embedded_asset!(app, "lit.wesl");
    embedded_asset!(app, "bloom.wesl");
    embedded_asset!(app, "panels.wesl");
    embedded_asset!(app, "thermal.wesl");
    app.add_plugins((Material2dPlugin::<Glow>::default(), Material2dPlugin::<Lit>::default(), Material2dPlugin::<Bloom>::default(), Material2dPlugin::<Composite>::default(), Material2dPlugin::<Heat>::default(), Material2dPlugin::<GrainBaker>::default(), Material2dPlugin::<Panels>::default(), Material2dPlugin::<Thermal>::default()))
        .add_systems(Startup, setup)
        .add_systems(PostUpdate, (follow, thermal_vision, smaa, present).before(TransformSystems::Propagate))
        .add_systems(PreUpdate, scaled_pointer.after(bevy::picking::PickingSystems::ProcessInput).before(bevy::picking::PickingSystems::Backend))
        .add_systems(PreUpdate, scaled_cursor.after(bevy::ui::UiSystems::Focus));
}

/// Main Camera's orthographic size in the scene: 5 up and down from the middle.
pub fn start_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::FixedVertical { viewport_height: 2.0 }, scale: 5.0, ..OrthographicProjection::default_2d() })
}

fn target(handle: &Handle<Image>, scale_factor: f32) -> RenderTarget {
    RenderTarget::Image(ImageRenderTarget { handle: handle.clone(), scale_factor })
}

/// A fullscreen pass into an image.
fn pass_camera(order: isize, layer: usize, image: &Handle<Image>) -> impl Bundle {
    (
        Camera2d,
        Camera { order, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        target(image, 1.0),
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        RenderLayers::layer(layer),
        fullscreen_projection(),
    )
}

fn fullscreen_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width: 1.0, height: 1.0 }, ..OrthographicProjection::default_2d() })
}

fn level_image(w: u32, h: u32) -> Image {
    let mut image = Image::new_target_texture(w, h, TextureFormat::Rgba16Float, None);
    image.sampler = ImageSampler::linear();
    image
}

/// BloomRenderer's _Threshold: GammaToLinearSpace(threshold), less the knee, 2 knee, 0.25 / knee.
fn bloom_threshold(g: &Grading) -> Vec4 {
    let threshold = gamma_to_linear(g.bloom_threshold);
    let knee = threshold * g.bloom_soft_knee + 1e-5;
    Vec4::new(threshold, threshold - knee, knee * 2.0, 0.25 / knee)
}

/// BloomRenderer.Render's pyramid: the first level's size (the screen halved, less along the anamorphic axis), the iterations and
/// the sample scale.
fn bloom_shape(screen: (u32, u32), g: &Grading) -> ((u32, u32), usize, f32) {
    let ratio = g.bloom_anamorphic.clamp(-1.0, 1.0);
    let w = (screen.0 as f32 / (2.0 - (-ratio).max(0.0))).floor() as u32;
    let h = (screen.1 as f32 / (2.0 - ratio.max(0.0))).floor() as u32;
    let s = (w.max(h) as f32).log2() + g.bloom_diffusion.min(10.0) - 10.0;
    (( w, h), (s.floor() as i32).clamp(1, 16) as usize, 0.5 + s - s.floor())
}

/// BloomRenderer.Render's passes for a pyramid of `levels` from `first`; returns the last texture and its texel size.
fn bloom_pyramid(commands: &mut Commands, images: &mut Assets<Image>, passes: &mut Assets<Bloom>, quad: &Handle<Mesh>, lit: &Handle<Image>, exposure: &Handle<Image>, screen: (u32, u32), (first, iterations, sample_scale): ((u32, u32), usize, f32), fast: bool, threshold: Vec4) -> (Handle<Image>, Vec2) {
    let (mut w, mut h) = first;
    let settings = BloomSettings {
        threshold,
        texel: Vec2::ZERO,
        clamp_to: 65472f32.powf(2.2), // GammaToLinearSpace(clamp)
        sample_scale,
        pass_index: 0,
    };
    let black = images.add(level_image(1, 1));
    let mut order = 1;
    let mut pass = |commands: &mut Commands, into: &Handle<Image>, material: Bloom| {
        let layer = 4 + order as usize;
        commands.spawn((BloomPass, pass_camera(order, layer, into)));
        commands.spawn((BloomPass, Mesh2d(quad.clone()), MeshMaterial2d(passes.add(material)), RenderLayers::layer(layer)));
        order += 1;
    };
    let (mut source, mut texel) = (lit.clone(), Vec2::new(1.0 / screen.0 as f32, 1.0 / screen.1 as f32));
    let mut levels = Vec::new();
    for i in 0..iterations {
        let down = images.add(level_image(w.max(1), h.max(1)));
        let up = images.add(level_image(w.max(1), h.max(1)));
        let settings = BloomSettings { texel, pass_index: if i == 0 { 0 } else { 2 } + fast as u32, ..settings };
        pass(commands, &down, Bloom { settings, source: source.clone(), bloom: black.clone(), exposure: exposure.clone() });
        source = down.clone();
        texel = Vec2::new(1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32);
        levels.push((down, up, texel));
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let (mut last, mut last_texel) = (levels[iterations - 1].0.clone(), levels[iterations - 1].2);
    for (down, up, texel) in levels[..iterations - 1].iter().rev() {
        let settings = BloomSettings { texel: last_texel, pass_index: 4 + fast as u32, ..settings };
        pass(commands, up, Bloom { settings, source: last, bloom: down.clone(), exposure: exposure.clone() });
        last = up.clone();
        last_texel = *texel;
    }
    (last, last_texel)
}

/// _Bloom_Settings' sample scale and intensity (RuntimeUtilities.Exp2(intensity / 10) - 1) with the bloom texture's texel size.
fn bloom_settings(texel: Vec2, sample_scale: f32, g: &Grading) -> Vec4 {
    Vec4::new(texel.x, texel.y, sample_scale, (g.bloom_intensity / 10.0 * 0.6931472).exp() - 1.0)
}

/// VideoSettingsInitBehaviour.Sync: Borderless and Fullscreen set the resolution to the display's size times
/// Clamp(RenderScale, 0.25, 1), which the display scales back up; Windowed keeps the window's size. The scene renders at
/// this size (physical pixels) and the window's scale factor times the same fraction.
pub fn render_size(window: &Window, settings: Option<&crate::ui::Settings>) -> (u32, u32, f32) {
    let s = settings.filter(|s| s.get("WindowMode") != 0.0).map_or(1.0, |s| s.get("RenderScale").clamp(0.25, 1.0));
    let (w, h) = (window.physical_width().max(1) as f32, window.physical_height().max(1) as f32);
    (((w * s).round() as u32).max(1), ((h * s).round() as u32).max(1), window.scale_factor() * s)
}

fn setup(
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Option<Res<crate::ui::Settings>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut lits: ResMut<Assets<Lit>>,
    mut blooms: ResMut<Assets<Bloom>>,
    mut composites: ResMut<Assets<Composite>>,
    mut heats: ResMut<Assets<Heat>>,
    mut bakers: ResMut<Assets<GrainBaker>>,
    mut colours: ResMut<Assets<ColorMaterial>>,
) {
    let (w, h, factor) = render_size(&window, settings.as_deref());
    let image = |w, h| Image::new_target_texture(w, h, TextureFormat::Rgba16Float, None);
    let scene = images.add(image(w, h));
    let lights = images.add(image((w / 2).max(1), (h / 2).max(1)));
    let lit = images.add(level_image(w, h));
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let exposure = images.add(crate::exposure::exposure_image());
    let white = images.add(crate::exposure::exposure_image());
    let black = ClearColorConfig::Custom(Color::BLACK);
    commands.spawn((
        MainCamera,
        Camera2d,
        Camera { order: -2, clear_color: black.clone(), ..default() },
        target(&scene, factor),
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        start_projection(),
    ));
    commands.spawn((
        LightsCamera,
        Camera2d,
        Camera { order: -1, clear_color: black.clone(), ..default() },
        target(&lights, factor / 2.0),
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(1),
    ));
    // LightsEffect runs BeforeStack: bloom reads its result.
    commands.spawn((pass_camera(0, 4, &lit), crate::exposure::ExposureView(exposure.clone())));
    let fx = LightsEffect::profile();
    let lit_material = lits.add(Lit { settings: Vec4::new(fx.intensity, fx.min_brightness, fx.max_brightness, 1.0), scene: scene.clone(), lights: lights.clone() });
    commands.spawn((
        Mesh2d(quad.clone()),
        MeshMaterial2d(lit_material.clone()),
        RenderLayers::layer(4),
    ));
    // The post stack's last step (Uber) into an image, then HeatEffect from it onto the window; the UI draws over it.
    let graded = images.add(level_image(w, h));
    commands.spawn(pass_camera(99, 2, &graded));
    commands.spawn((
        FinalPass,
        Camera2d,
        Camera { order: 100, clear_color: black, ..default() },
        Hdr,
        Tonemapping::None,
        RenderLayers::layer(HEAT_LAYER),
        fullscreen_projection(),
    ));
    // The UI canvas draws over everything.
    commands.spawn((
        ScreenUiCamera,
        Camera2d,
        Camera { order: 101, clear_color: ClearColorConfig::None, ..default() },
        Hdr,
        Tonemapping::None,
        RenderLayers::layer(SCREEN_UI_LAYER),
        IsDefaultUiCamera,
        start_projection(),
    ));
    let blended = blend_volumes("DEFAULT POST PROCESSING PROFILE", 0.0, 0.0, post_exposure(100.0), 2.0, &[]);
    let shape = bloom_shape((w, h), &blended);
    let (bloom, texel) = bloom_pyramid(&mut commands, &mut images, &mut blooms, &quad, &lit, &exposure, (w, h), shape, false, bloom_threshold(&blended));
    let grade = grade_uniform(&blended);
    // GrainRenderer's lookup: 128x128, bilinear, repeat.
    let mut grain = level_image(128, 128);
    grain.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, ..ImageSamplerDescriptor::linear() });
    let grain = images.add(grain);
    commands.spawn((GrainPass, pass_camera(98, GRAIN_LAYER, &grain)));
    let baker = bakers.add(GrainBaker { settings: Vec4::ZERO });
    commands.spawn((Mesh2d(quad.clone()), MeshMaterial2d(baker.clone()), RenderLayers::layer(GRAIN_LAYER)));
    let composite = composites.add(Composite {
        settings: bloom_settings(texel, shape.2, &blended),
        lit: lit.clone(),
        bloom,
        exposure: exposure.clone(),
        video: Vec4::new(post_exposure(100.0), 0.0, 0.0, 1.0),
        grade,
        fx: fx_uniform(&blended, (w, h), 0),
        grain,
    });
    commands.spawn((Mesh2d(quad.clone()), MeshMaterial2d(composite.clone()), RenderLayers::layer(2)));
    let heat = heats.add(Heat { heat: Vec4::new(0.0, 5.0, -1.0, 0.0), view: Vec4::new(0.0, 1.0, 0.0, 0.0), source: graded.clone() });
    commands.spawn((Mesh2d(quad.clone()), MeshMaterial2d(heat.clone()), RenderLayers::layer(HEAT_LAYER)));
    let mut screen = level_image(w, h);
    screen.sampler = ImageSampler::linear();
    let screen = images.add(screen);
    commands.spawn((
        Present,
        Camera2d,
        Camera { order: 102, is_active: false, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(PRESENT_LAYER),
        fullscreen_projection(),
    ));
    let present = colours.add(ColorMaterial { texture: Some(screen.clone()), alpha_mode: AlphaMode2d::Opaque, ..default() });
    commands.spawn((Mesh2d(quad.clone()), MeshMaterial2d(present), RenderLayers::layer(PRESENT_LAYER)));
    commands.insert_resource(Targets { lit_material, scene, lights, lit, exposure, white, quad, composite, graded, heat, baker, screen });
}

/// VideoSettingsInitBehaviour.Sync's Screen.SetResolution below the display's size: the whole screen, the post stack's
/// end and the UI with it, renders at render_size and the display scales it up (linear); at full size both draw onto
/// the window.
fn present(
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Option<Res<crate::ui::Settings>>,
    targets: Res<Targets>,
    mut images: ResMut<Assets<Image>>,
    mut screens: Query<&mut RenderTarget, Or<(With<FinalPass>, With<ScreenUiCamera>)>>,
    mut present: Single<&mut Camera, With<Present>>,
) {
    let (w, h, factor) = render_size(&window, settings.as_deref());
    let scaled = factor < window.scale_factor();
    if scaled && images.get(&targets.screen).is_some_and(|i| i.size() != UVec2::new(w, h)) {
        images.get_mut(&targets.screen).unwrap().resize(Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    }
    for mut t in &mut screens {
        match (&*t, scaled) {
            (RenderTarget::Image(i), true) if i.handle == targets.screen && i.scale_factor == factor => {}
            (RenderTarget::Window(_), false) => {}
            _ => *t = if scaled { target(&targets.screen, factor) } else { RenderTarget::default() },
        }
    }
    if present.is_active != scaled {
        present.is_active = scaled;
    }
}

/// The UI camera's target while the screen renders smaller (present): the mouse's window position is on it.
fn scaled_screen(camera: &RenderTarget) -> Option<&ImageRenderTarget> {
    if let RenderTarget::Image(i) = camera { Some(i) } else { None }
}

/// UI picking matches a pointer to the camera drawing on its target: the mouse, on the window, moves to the smaller
/// screen (its logical position is the same there).
fn scaled_pointer(camera: Single<&RenderTarget, With<ScreenUiCamera>>, mut pointers: Query<(&bevy::picking::pointer::PointerId, &mut bevy::picking::pointer::PointerLocation)>) {
    let Some(screen) = scaled_screen(&camera) else { return };
    for (id, mut at) in &mut pointers {
        if let (bevy::picking::pointer::PointerId::Mouse, Some(l)) = (id, at.location.as_mut()) {
            if matches!(l.target, bevy::camera::NormalizedRenderTarget::Window(_)) {
                l.target = bevy::camera::NormalizedRenderTarget::Image(screen.clone());
            }
        }
    }
}

/// RelativeCursorPosition (ui_focus_system takes the cursor from window targets only) on the smaller screen.
fn scaled_cursor(
    camera: Single<&RenderTarget, With<ScreenUiCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut nodes: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, Option<&bevy::ui::CalculatedClip>, &mut bevy::ui::RelativeCursorPosition)>,
) {
    let Some(screen) = scaled_screen(&camera) else { return };
    let cursor = window.cursor_position().map(|c| c * screen.scale_factor);
    for (node, t, clip, mut at) in &mut nodes {
        let over = cursor.is_some_and(|p| node.contains_point(*t, p) && clip.is_none_or(|c| c.contains_point(p)));
        at.set_if_neq(bevy::ui::RelativeCursorPosition { cursor_over: over, normalized: cursor.and_then(|p| node.normalize_point(*t, p)) });
    }
}

/// VideoSettingsInitBehaviour.SetPost: ColorGrading.postExposure from Preferences.Brightness.
pub(crate) fn post_exposure(brightness: f32) -> f32 {
    1.385869 + (-3.0 + (brightness - 10.0) * 7.0 / 390.0)
}

/// The bloom pyramid follow() built: its shape and fast mode, with its last texture's texel size.
#[derive(Default)]
struct Built {
    pyramid: Option<((u32, u32), usize, bool, (u32, u32), bool)>,
    texel: Vec2,
    /// GrainRenderer.m_SampleIndex.
    grain_index: u32,
}

/// LightRenderCameraBehaviour: follows Main Camera; both textures follow the window (ResetRT).
fn follow(
    mut commands: Commands,
    (window, real): (Single<&Window, With<PrimaryWindow>>, Res<Time<Real>>),
    targets: Res<Targets>,
    mut images: ResMut<Assets<Image>>,
    mut blooms: ResMut<Assets<Bloom>>,
    (mut composites, mut heats, mut bakers): (ResMut<Assets<Composite>>, ResMut<Assets<Heat>>, ResMut<Assets<GrainBaker>>),
    (old_passes, mut grain_pass): (Query<Entity, With<BloomPass>>, Single<&mut Camera, With<GrainPass>>),
    main: Single<(&Transform, &Projection), (With<MainCamera>, Without<LightsCamera>, Without<ScreenUiCamera>)>,
    mut lights: Single<(&mut Transform, &mut Projection), (With<LightsCamera>, Without<ScreenUiCamera>)>,
    mut screen_ui: Single<(&mut Transform, &mut Projection), (With<ScreenUiCamera>, Without<MainCamera>, Without<LightsCamera>)>,
    mut scales: Query<(&mut RenderTarget, Has<LightsCamera>), Or<(With<MainCamera>, With<LightsCamera>)>>,
    settings: Option<Res<crate::ui::Settings>>,
    mut lits: ResMut<Assets<Lit>>,
    mut built: Local<Built>,
    (play, menu): (Option<Res<crate::play::Play>>, Option<Res<crate::ui::MenuAudio>>),
    bloom_passes: Query<&MeshMaterial2d<Bloom>, With<BloomPass>>,
) {
    // VideoSettingsInitBehaviour.SetPost. The enums: BloomMode Off, Fast, Fancy; TonemappingMode Modern, Legacy.
    let (brightness, bloom_mode, tonemapping, lighting) =
        settings.as_ref().map_or((100.0, 2.0, 0.0, 1.0), |s| (s.get("Brightness"), s.get("BloomMode"), s.get("TonemappingMode"), s.get("Lighting")));
    // The post volumes (MapConfig.ApplySettings' heat and cold weights and the registered ones; none without a map).
    let (heat, cold, time, volumes) = play.as_ref().map_or((0.0, 0.0, 0.0, &[][..]), |p| (p.game.post_weights[0], p.game.post_weights[1], p.game.time.time, &p.game.post_volumes[..]));
    // The Menu scene (loaded, no game): its own profile, no AutoExposure (whiteTexture) and no LightsEffect.
    let menu_scene = play.is_none() && menu.is_some();
    let base = if menu_scene { "MAIN MENU POST PROCESSING PROFILE" } else { "DEFAULT POST PROCESSING PROFILE" };
    let lighting = if menu_scene { 0.0 } else { lighting };
    let exposure = if menu_scene { &targets.white } else { &targets.exposure };
    let blended = blend_volumes(base, heat, cold, post_exposure(brightness), bloom_mode, volumes);
    let ortho = if let Projection::Orthographic(o) = main.1 { o.scale } else { 1.0 };
    let (w, h, factor) = render_size(&window, settings.as_deref());
    let grade = grade_uniform(&blended);
    let (heat, view) = heat_uniform(&blended, time, ortho * w as f32 / h as f32);
    if heats.get(&targets.heat).is_some_and(|m| m.heat != heat || m.view != view) {
        let mut m = heats.get_mut(&targets.heat).unwrap();
        (m.heat, m.view) = (heat, view);
    }
    // Bloom runs when enabled with an intensity above 0 (Bloom.IsEnabledAndSupported).
    let bloom_on = blended.bloom_enabled && blended.bloom_intensity > 0.0;
    let fast = blended.bloom_fast;
    let video = Vec4::new(blended.post_exposure, fast as u32 as f32, tonemapping, bloom_on as u32 as f32);
    // GrainRenderer.Render bakes the lookup (_Phase = realtimeSinceStartup % 10) and steps its Halton sample once per frame.
    let grain_on = blended.grain_enabled && blended.grain_intensity > 0.0;
    grain_pass.is_active = grain_on;
    let fx = fx_uniform(&blended, (w, h), built.grain_index);
    if grain_on {
        built.grain_index = (built.grain_index + 1) % 1024;
        let settings = Vec4::new(real.elapsed_secs() % 10.0, blended.grain_colored as u32 as f32, 0.0, 0.0);
        bakers.get_mut(&targets.baker).unwrap().settings = settings;
    }
    let shape = bloom_shape((w, h), &blended);
    let threshold = bloom_threshold(&blended);
    if lits.get(&targets.lit_material).is_some_and(|l| l.settings.w != lighting) {
        lits.get_mut(&targets.lit_material).unwrap().settings.w = lighting;
    }
    *lights.0 = *main.0;
    *lights.1 = main.1.clone();
    *screen_ui.0 = *main.0;
    *screen_ui.1 = main.1.clone();
    let key = (shape.0, shape.1, fast, (w, h), menu_scene);
    if built.pyramid != Some(key) {
        built.pyramid = Some(key);
        let size = |w, h| Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        if images.get(&targets.scene).is_some_and(|i| i.size() != UVec2::new(w, h)) {
            if let Some(mut i) = images.get_mut(&targets.scene) {
                i.resize(size(w, h));
            }
            if let Some(mut i) = images.get_mut(&targets.lights) {
                i.resize(size((w / 2).max(1), (h / 2).max(1)));
            }
            for (mut render_target, light) in &mut scales {
                *render_target = target(if light { &targets.lights } else { &targets.scene }, if light { factor / 2.0 } else { factor });
            }
            if let Some(mut i) = images.get_mut(&targets.lit) {
                i.resize(size(w, h));
            }
            if let Some(mut i) = images.get_mut(&targets.graded) {
                i.resize(size(w, h));
            }
            // Its bind group holds the old textures until the material changes.
            lits.get_mut(&targets.lit_material).unwrap().settings.w = lighting;
        }
        for e in &old_passes {
            commands.entity(e).despawn();
        }
        let (bloom, texel) = bloom_pyramid(&mut commands, &mut images, &mut blooms, &targets.quad, &targets.lit, exposure, (w, h), shape, fast, threshold);
        built.texel = texel;
        let mut composite = composites.get_mut(&targets.composite).unwrap();
        composite.bloom = bloom;
        composite.exposure = exposure.clone();
    } else {
        // The blend moves the threshold, knee and sample scale between frames.
        for m in &bloom_passes {
            if blooms.get(&m.0).is_some_and(|b| b.settings.threshold != threshold || b.settings.sample_scale != shape.2) {
                let mut b = blooms.get_mut(&m.0).unwrap();
                (b.settings.threshold, b.settings.sample_scale) = (threshold, shape.2);
            }
        }
    }
    let settings = bloom_settings(built.texel, shape.2, &blended);
    if composites.get(&targets.composite).is_some_and(|c| c.video != video || c.grade != grade || c.fx != fx || c.settings != settings) {
        let mut c = composites.get_mut(&targets.composite).unwrap();
        (c.video, c.grade, c.fx, c.settings) = (video, grade, fx, settings);
    }
}
