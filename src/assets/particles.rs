//! ParticleSystem (class 198) as the player's serialized data lays it out (Unity 2020.3, version 7).
use super::classes::PPtr;
use super::serialized::Reader;

type R<'a, 'b> = &'a mut Reader<'b>;
type Res<T> = Result<T, String>;

fn floats<const N: usize>(r: R) -> Res<[f32; N]> {
    let mut v = [0.0; N];
    for x in &mut v {
        *x = r.f32()?;
    }
    Ok(v)
}

fn vec<T>(r: R, mut item: impl FnMut(&mut Reader) -> Res<T>) -> Res<Vec<T>> {
    let n = r.i32()?;
    (0..n).map(|_| item(r)).collect()
}

fn flags<const N: usize>(r: R) -> Res<[bool; N]> {
    let mut v = [false; N];
    for x in &mut v {
        *x = r.bool()?;
    }
    r.align();
    Ok(v)
}

fn flag(r: R) -> Res<bool> {
    Ok(flags::<1>(r)?[0])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub time: f32,
    pub value: f32,
    pub in_slope: f32,
    pub out_slope: f32,
    pub weighted_mode: i32,
    pub in_weight: f32,
    pub out_weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub keys: Vec<Keyframe>,
    pub pre_infinity: i32,
    pub post_infinity: i32,
}

impl Curve {
    fn read(r: R) -> Res<Self> {
        let keys = vec(r, |r| {
            let [time, value, in_slope, out_slope] = floats(r)?;
            let weighted_mode = r.i32()?;
            let [in_weight, out_weight] = floats(r)?;
            Ok(Keyframe { time, value, in_slope, out_slope, weighted_mode, in_weight, out_weight })
        })?;
        let (pre_infinity, post_infinity) = (r.i32()?, r.i32()?);
        r.i32()?; // m_RotationOrder
        Ok(Self { keys, pre_infinity, post_infinity })
    }
}

/// ParticleSystem.MinMaxCurve. state: 0 constant, 1 curve, 2 between two curves, 3 between two constants.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MinMaxCurve {
    pub state: i16,
    pub scalar: f32,
    pub min_scalar: f32,
    pub max: Curve,
    pub min: Curve,
}

impl MinMaxCurve {
    fn read(r: R) -> Res<Self> {
        let state = r.i16()?;
        r.align();
        let [scalar, min_scalar] = floats(r)?;
        Ok(Self { state, scalar, min_scalar, max: Curve::read(r)?, min: Curve::read(r)? })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gradient {
    pub keys: [[f32; 4]; 8],
    pub ctime: [u16; 8],
    pub atime: [u16; 8],
    /// GradientMode: 0 blend, 1 fixed.
    pub mode: i32,
    pub color_keys: u8,
    pub alpha_keys: u8,
}

impl Gradient {
    fn read(r: R) -> Res<Self> {
        let mut g = Self::default();
        for k in &mut g.keys {
            *k = floats(r)?;
        }
        for t in g.ctime.iter_mut().chain(&mut g.atime) {
            *t = r.u16()?;
        }
        g.mode = r.i32()?;
        (g.color_keys, g.alpha_keys) = (r.u8()?, r.u8()?);
        r.align();
        Ok(g)
    }
}

/// ParticleSystem.MinMaxGradient. state: 0 color, 1 gradient, 2 two colors, 3 two gradients, 4 random colour of a gradient.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MinMaxGradient {
    pub state: i16,
    pub min_color: [f32; 4],
    pub max_color: [f32; 4],
    pub max: Gradient,
    pub min: Gradient,
}

impl MinMaxGradient {
    fn read(r: R) -> Res<Self> {
        let state = r.i16()?;
        r.align();
        let (min_color, max_color) = (floats(r)?, floats(r)?);
        Ok(Self { state, min_color, max_color, max: Gradient::read(r)?, min: Gradient::read(r)? })
    }
}

fn c(r: R) -> Res<MinMaxCurve> {
    MinMaxCurve::read(r)
}

/// A MultiModeParameter: value, mode, spread, speed.
/// ShapeModule.MultiModeParameter: value, mode (0 random, 1 loop, 2 ping-pong, 3 burst spread), spread, speed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Multi {
    pub value: f32,
    pub mode: i32,
    pub spread: f32,
    pub speed: MinMaxCurve,
}

fn multi(r: R) -> Res<Multi> {
    Ok(Multi { value: r.f32()?, mode: r.i32()?, spread: r.f32()?, speed: c(r)? })
}

/// VelocityModule: x, y, z, the orbital and offset axes, radial and speedModifier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Velocity {
    pub axes: [MinMaxCurve; 3],
    pub orbital: [MinMaxCurve; 3],
    pub offset: [MinMaxCurve; 3],
    pub radial: MinMaxCurve,
    pub speed_modifier: MinMaxCurve,
    pub world: bool,
}

/// ForceModule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Force {
    pub axes: [MinMaxCurve; 3],
    pub world: bool,
    pub randomize_per_frame: bool,
}

#[derive(Clone, Debug)]
pub struct Burst {
    pub time: f32,
    pub count: MinMaxCurve,
    pub cycles: i32,
    pub interval: f32,
    pub probability: f32,
}

#[derive(Clone, Debug)]
pub struct SubEmitter {
    pub emitter: PPtr,
    /// ParticleSystemSubEmitterType: 0 birth, 1 collision, 2 death, 3 trigger, 4 manual.
    pub kind: i32,
    /// ParticleSystemSubEmitterProperties bits: 1 color, 2 size, 4 rotation, 8 lifetime, 16 duration.
    pub properties: i32,
    pub probability: f32,
}

#[derive(Clone, Debug)]
pub struct SizeOverLifetime {
    pub x: MinMaxCurve,
    pub y: MinMaxCurve,
    pub z: MinMaxCurve,
    pub separate: bool,
}

/// UVModule, the Texture Sheet Animation.
#[derive(Clone, Debug)]
pub struct Sheet {
    /// 0 grid, 1 sprites.
    pub mode: i32,
    /// 0 lifetime, 1 speed, 2 fps.
    pub time_mode: i32,
    pub fps: f32,
    pub frame_over_time: MinMaxCurve,
    pub start_frame: MinMaxCurve,
    pub tiles_x: i32,
    pub tiles_y: i32,
    /// 0 whole sheet, 1 single row.
    pub animation_type: i32,
    pub row_index: i32,
    pub cycles: f32,
    /// ParticleSystemAnimationRowMode: 0 custom, 1 random, 2 mesh index.
    pub row_mode: i32,
    pub uv_channel_mask: i32,
    pub speed_range: [f32; 2],
    pub sprites: Vec<PPtr>,
    pub flip_uv: [f32; 2],
}

/// NoiseModule (Transfer 180ddc860; field offsets of the module in the comments).
#[derive(Clone, Debug)]
pub struct Noise {
    /// strength x/y/z (+0x10/+0x30/+0x50).
    pub strength: [MinMaxCurve; 3],
    pub separate: bool,
    pub frequency: f32,
    pub damping: bool,
    pub octaves: i32,
    pub octave_multiplier: f32,
    pub octave_scale: f32,
    /// 0 = 1D, 1 = 2D, 2 = 3D (CalculateNoise's first template argument).
    pub quality: i32,
    pub scroll_speed: MinMaxCurve,
    pub remap: [MinMaxCurve; 3],
    pub remap_enabled: bool,
    /// positionAmount, rotationAmount, sizeAmount.
    pub amounts: [MinMaxCurve; 3],
}

#[derive(Clone, Debug)]
pub struct Clamp {
    pub magnitude: MinMaxCurve,
    pub dampen: f32,
    pub drag: MinMaxCurve,
    pub drag_by_size: bool,
    pub drag_by_velocity: bool,
}

/// ExternalForcesModule (Transfer order): multiplierCurve, influenceFilter (0 layer mask, 1 list, 2 both), influenceMask,
/// influenceList (ParticleSystemForceField PPtrs).
#[derive(Clone, Debug)]
pub struct ExternalForces {
    pub multiplier: MinMaxCurve,
    pub influence_filter: i32,
    pub influence_mask: u32,
    pub influence: Vec<PPtr>,
}

/// ParticleSystemForceField (class 330): m_GameObject, m_Enabled, then ParticleSystemForceFieldParameters in Transfer
/// order (0x1801a1720).
#[derive(Clone, Debug)]
pub struct ForceField {
    pub game_object: PPtr,
    pub enabled: bool,
    /// ParticleSystemForceFieldShape: 0 sphere, 1 hemisphere, 2 cylinder, 3 box.
    pub shape: i32,
    pub start_range: f32,
    pub end_range: f32,
    pub length: f32,
    pub gravity_focus: f32,
    pub rotation_randomness: [f32; 2],
    pub direction: [MinMaxCurve; 3],
    pub gravity: MinMaxCurve,
    pub rotation_speed: MinMaxCurve,
    pub rotation_attraction: MinMaxCurve,
    pub drag: MinMaxCurve,
    pub vector_field: PPtr,
    pub vector_field_speed: MinMaxCurve,
    pub vector_field_attraction: MinMaxCurve,
    pub drag_by_size: bool,
    pub drag_by_velocity: bool,
}

impl ForceField {
    pub fn read(data: &[u8]) -> Res<Self> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let shape = r.i32()?;
        let [start_range, end_range, length, gravity_focus] = floats(r)?;
        let rotation_randomness = floats(r)?;
        let direction = [c(r)?, c(r)?, c(r)?];
        let (gravity, rotation_speed, rotation_attraction, drag) = (c(r)?, c(r)?, c(r)?, c(r)?);
        let vector_field = PPtr::read(r)?;
        let (vector_field_speed, vector_field_attraction) = (c(r)?, c(r)?);
        let [drag_by_size, drag_by_velocity] = flags(r)?;
        Ok(Self {
            game_object, enabled, shape, start_range, end_range, length, gravity_focus, rotation_randomness, direction, gravity, rotation_speed, rotation_attraction, drag, vector_field,
            vector_field_speed, vector_field_attraction, drag_by_size, drag_by_velocity,
        })
    }
}

/// WindZone (class 182): m_GameObject, m_Enabled, m_Mode (0 directional, 1 spherical), m_Radius, m_WindMain,
/// m_WindTurbulence, m_WindPulseMagnitude, m_WindPulseFrequency.
#[derive(Clone, Debug)]
pub struct WindZone {
    pub game_object: PPtr,
    pub enabled: bool,
    pub mode: i32,
    pub radius: f32,
    pub main: f32,
    pub turbulence: f32,
    pub pulse_magnitude: f32,
    pub pulse_frequency: f32,
}

impl WindZone {
    pub fn read(data: &[u8]) -> Res<Self> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let mode = r.i32()?;
        let [radius, main, turbulence, pulse_magnitude, pulse_frequency] = floats(r)?;
        Ok(Self { game_object, enabled, mode, radius, main, turbulence, pulse_magnitude, pulse_frequency })
    }
}

#[derive(Clone, Debug)]
pub struct Collision {
    /// CollisionModule.enabled in the prefab (the bleeding particle's Awake switches it on under FancyEffects).
    pub enabled: bool,
    /// ParticleSystemCollisionType: 0 planes, 1 world; ParticleSystemCollisionMode: 0 3D, 1 2D.
    pub kind: i32,
    pub mode: i32,
    pub collider_force: f32,
    /// m_Planes (Transforms) count; plane mode only.
    pub planes: usize,
    pub dampen: MinMaxCurve,
    pub bounce: MinMaxCurve,
    pub lifetime_loss: MinMaxCurve,
    pub min_kill_speed: f32,
    pub max_kill_speed: f32,
    pub radius_scale: f32,
    pub collides_with: u32,
    pub max_shapes: i32,
    /// ParticleSystemCollisionQuality: 0 high, 1 medium, 2 low.
    pub quality: i32,
    /// voxelSize: the plane cache's cell.
    pub voxel_size: f32,
    /// sendCollisionMessages, enableDynamicColliders, enableInteriorCollisions.
    pub messages: bool,
    pub dynamic: bool,
    pub interior: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Trail {
    /// ParticleSystemTrailMode: 0 particles, 1 ribbon.
    pub mode: i32,
    pub ratio: f32,
    pub lifetime: MinMaxCurve,
    pub min_vertex_distance: f32,
    pub world_space: bool,
    pub die_with_particles: bool,
    pub size_affects_width: bool,
    pub size_affects_lifetime: bool,
    pub inherit_particle_color: bool,
    /// ParticleSystemTrailTextureMode: 0 stretch, 1 tile, 2 distribute per segment, 3 repeat per segment.
    pub texture_mode: i32,
    pub ribbon_count: i32,
    /// generateLightingData, splitSubEmitterRibbons, attachRibbonsToTransform.
    pub lighting_data: bool,
    pub split_sub_emitter_ribbons: bool,
    pub attach_ribbons: bool,
    pub color_over_lifetime: MinMaxGradient,
    pub width_over_trail: MinMaxCurve,
    pub color_over_trail: MinMaxGradient,
}

/// ShapeModule's texture settings: m_TextureClipChannel, m_TextureClipThreshold, m_TextureColorAffectsParticles,
/// m_TextureAlphaAffectsParticles, m_TextureBilinearFiltering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShapeTexture {
    pub clip_channel: i32,
    pub clip_threshold: f32,
    pub color_affects: bool,
    pub alpha_affects: bool,
    pub bilinear: bool,
}

impl ParticleSystem {
    /// DetermineSupportsProcedural (1801b8730): local simulation, stopAction None, no rateOverDistance, none of
    /// ExternalForces, ClampVelocity, RotationBySpeed, Collision, Trigger, SubEmitters or Noise, no PerParticle trail,
    /// a constant gravity, a finite lifetime, the arc (Circle, Cone, ConeVolume, Donut) or radius (SingleSidedEdge) in
    /// Random mode, no orbital or radial velocity and no per-frame force randomness; then CurvesSupportProcedural on the
    /// rotation, velocity and force curves (constants pass).
    pub fn supports_procedural(&self) -> bool {
        let p = self;
        let shape_ok = !p.shape_enabled || match p.shape_type { 4 | 8 | 10 | 17 => p.shape_arc.mode == 0, 12 => p.shape_radius.mode == 0, _ => true };
        let ok = p.space == 0 && p.stop_action == 0 && p.rate_over_distance.scalar == 0.0 && p.external_forces.is_none() && p.clamp.is_none() && p.rotation_by_speed.is_none()
            && !p.collision.as_ref().is_some_and(|c| c.enabled) && !p.trigger && p.subs.is_empty() && p.noise.is_none() && !p.trail.as_ref().is_some_and(|t| t.mode == 0)
            && p.gravity.state == 0 && p.start_lifetime.scalar != f32::INFINITY && shape_ok
            && !p.velocity.as_ref().is_some_and(|v| v.orbital.iter().chain([&v.radial]).any(|c| c.scalar != 0.0)) && !p.force.as_ref().is_some_and(|f| f.randomize_per_frame);
        if !ok {
            return false;
        }
        if p.rotation.is_some() && p.rotation_separate {
            unimplemented!("mod-only: CurvesSupportProcedural of separate-axis rotation curves: no game system has them");
        }
        let curves = p.rotation.iter().chain(p.velocity.iter().flat_map(|v| &v.axes)).chain(p.force.iter().flat_map(|f| &f.axes));
        for c in curves {
            if matches!(c.state, 1 | 2) {
                unimplemented!("mod-only: CurvesSupportProcedural's IsValidPolynomialCurve (18074c7a0): no procedural game system has a curve");
            }
        }
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct ParticleSystem {
    /// ParticleSystemStopAction: 0 none, 1 disable, 2 destroy, 3 callback.
    pub stop_action: i32,
    /// ParticleSystemCullingMode: 0 automatic, 1 pause and catch up, 2 pause, 3 always simulate.
    pub culling_mode: i32,
    /// ParticleSystemRingBufferMode: 0 disabled.
    pub ring_buffer_mode: i32,
    pub prewarm: bool,
    pub unscaled_time: bool,
    pub auto_random_seed: bool,
    pub rigidbody_velocity: bool,
    pub random_seed: u32,
    pub external_forces: Option<ExternalForces>,
    pub trigger: bool,
    pub lights: bool,
    /// LightsModule's light prefab (a Light) and its range curve's scalar (m_Range).
    pub lights_light: PPtr,
    pub lights_range: f32,
    pub custom_data: bool,
    pub length: f32,
    pub simulation_speed: f32,
    pub looping: bool,
    pub play_on_awake: bool,
    pub start_delay: MinMaxCurve,
    /// ParticleSystemSimulationSpace: 0 local, 1 world, 2 custom.
    pub space: i32,
    /// ParticleSystemScalingMode: 0 hierarchy, 1 local, 2 shape.
    pub scaling: i32,
    pub start_lifetime: MinMaxCurve,
    pub start_speed: MinMaxCurve,
    pub start_color: MinMaxGradient,
    pub start_size: MinMaxCurve,
    /// startSizeY, with size3D on.
    pub start_size_y: Option<MinMaxCurve>,
    /// startSizeZ, with size3D on.
    pub start_size_z: Option<MinMaxCurve>,
    pub start_rotation: MinMaxCurve,
    /// startRotationX and Y, with rotation3D on.
    pub start_rotation_xy: Option<[MinMaxCurve; 2]>,
    /// startSizeY, startSizeZ, startRotationX and Y whatever the 3D flags (InitialModule::StartExternal reads them
    /// whenever the particles use 3D size or rotation).
    pub start_raw_3d: [MinMaxCurve; 4],
    pub randomize_rotation_direction: f32,
    pub max_particles: i32,
    pub gravity: MinMaxCurve,
    pub shape_enabled: bool,
    pub shape_type: i32,
    /// MeshShapeType: Vertex, Edge, Triangle.
    pub shape_placement: i32,
    pub shape_angle: f32,
    pub shape_length: f32,
    pub shape_radius_thickness: f32,
    pub shape_position: [f32; 3],
    pub shape_rotation: [f32; 3],
    pub shape_scale: [f32; 3],
    pub shape_random_direction: f32,
    pub shape_spherical_direction: f32,
    pub shape_random_position: f32,
    pub shape_radius: Multi,
    pub shape_arc: Multi,
    pub shape_box_thickness: [f32; 3],
    pub shape_align_to_direction: bool,
    /// m_Texture is set (the texture clip and colour paths).
    pub shape_texture: bool,
    pub shape_texture_ref: PPtr,
    /// m_MeshNormalOffset, m_MeshSpawn (mode, spread, speed), m_UseMeshMaterialIndex, m_UseMeshColors.
    pub shape_normal_offset: f32,
    pub shape_mesh_spawn: (i32, f32, MinMaxCurve),
    pub shape_use_material_index: bool,
    pub shape_use_mesh_colors: bool,
    pub shape_texture_params: ShapeTexture,
    /// m_TextureColorData (ShapeModule::AcquireData 0x180df9ed0: GetPixels32 of mip 0), set by the loader: the
    /// pixels (ColorRGBA32, rows bottom first), width and height.
    pub shape_pixels: Option<std::sync::Arc<(Vec<u32>, i32, i32)>>,
    pub emission_enabled: bool,
    pub rate_over_time: MinMaxCurve,
    pub rate_over_distance: MinMaxCurve,
    pub bursts: Vec<Burst>,
    pub size: Option<SizeOverLifetime>,
    pub rotation: Option<MinMaxCurve>,
    pub color: Option<MinMaxGradient>,
    pub sheet: Option<Sheet>,
    /// UVModule.m_Mode whatever its enabled flag (UpdateBounds reads it): 0 grid, 1 sprites.
    pub uv_mode: i32,
    pub velocity: Option<Velocity>,
    pub force: Option<Force>,
    /// mode (0 initial, 1 current), curve.
    pub inherit: Option<(i32, MinMaxCurve)>,
    pub lifetime_by_speed: Option<(MinMaxCurve, [f32; 2])>,
    pub clamp: Option<Clamp>,
    pub noise: Option<Noise>,
    /// RotationBySpeedModule's z curve and speed range.
    pub rotation_by_speed: Option<(MinMaxCurve, [f32; 2])>,
    /// An enabled RotationModule or RotationBySpeedModule on separate axes (with a 3D start rotation: uses3DRotation).
    pub rotation_separate: bool,
    pub size_by_speed: Option<(SizeOverLifetime, [f32; 2])>,
    pub color_by_speed: Option<(MinMaxGradient, [f32; 2])>,
    pub collision: Option<Collision>,
    pub subs: Vec<SubEmitter>,
    pub trail: Option<Trail>,
}

impl ParticleSystem {
    pub fn read(data: &[u8]) -> Res<Self> {
        let r = &mut Reader::new(data);
        PPtr::read(r)?;
        let [length, simulation_speed] = floats(r)?;
        let stop_action = r.i32()?;
        let (culling_mode, ring_buffer_mode) = (r.i32()?, r.i32()?);
        floats::<2>(r)?;
        let [looping, prewarm, play_on_awake, unscaled_time, auto_random_seed, rigidbody_velocity] = flags(r)?;
        let start_delay = c(r)?;
        let space = r.i32()?;
        PPtr::read(r)?;
        let scaling = r.i32()?;
        let random_seed = r.i32()? as u32;
        // InitialModule
        flag(r)?;
        let (start_lifetime, start_speed) = (c(r)?, c(r)?);
        let start_color = MinMaxGradient::read(r)?;
        let start_size = c(r)?;
        let start_size_y = c(r)?;
        let (start_size_z, start_rotation_x, start_rotation_y) = (c(r)?, c(r)?, c(r)?);
        let start_rotation = c(r)?;
        let randomize_rotation_direction = r.f32()?;
        let max_particles = r.i32()?;
        let [size_3d, rotation_3d] = flags(r)?;
        let start_raw_3d = [start_size_y.clone(), start_size_z.clone(), start_rotation_x.clone(), start_rotation_y.clone()];
        let start_size_y = size_3d.then_some(start_size_y);
        let start_size_z = size_3d.then_some(start_size_z);
        let start_rotation_xy = rotation_3d.then_some([start_rotation_x, start_rotation_y]);
        let gravity = c(r)?;
        // ShapeModule
        let shape_enabled = flag(r)?;
        let shape_type = r.i32()?;
        let [shape_angle, shape_length] = floats(r)?;
        let shape_box_thickness = floats(r)?;
        let [shape_radius_thickness, _donut] = floats(r)?;
        let (shape_position, shape_rotation, shape_scale) = (floats(r)?, floats(r)?, floats(r)?);
        let shape_placement = r.i32()?;
        let (_material_index, shape_normal_offset) = (r.i32()?, r.f32()?); // m_MeshMaterialIndex, m_MeshNormalOffset
        let shape_mesh_spawn = (r.i32()?, r.f32()?, c(r)?); // m_MeshSpawn: mode, spread, speed
        for _ in 0..5 {
            PPtr::read(r)?; // m_Mesh, m_MeshRenderer, m_SkinnedMeshRenderer, m_Sprite, m_SpriteRenderer
        }
        let [shape_use_material_index, shape_use_mesh_colors, shape_align_to_direction] = flags::<3>(r)?;
        let shape_texture_ref = PPtr::read(r)?;
        let shape_texture = shape_texture_ref.path_id != 0;
        let (clip_channel, clip_threshold, _uv_channel) = (r.i32()?, r.f32()?, r.i32()?);
        let [color_affects, alpha_affects, bilinear] = flags::<3>(r)?;
        let shape_texture_params = ShapeTexture { clip_channel, clip_threshold, color_affects, alpha_affects, bilinear };
        let [shape_random_direction, shape_spherical_direction, shape_random_position] = floats(r)?;
        let (shape_radius, shape_arc) = (multi(r)?, multi(r)?);
        // EmissionModule
        let emission_enabled = flag(r)?;
        let (rate_over_time, rate_over_distance) = (c(r)?, c(r)?);
        r.i32()?; // m_BurstCount
        let bursts = vec(r, |r| {
            let time = r.f32()?;
            let count = c(r)?;
            let cycles = r.i32()?;
            let [interval, probability] = floats(r)?;
            Ok(Burst { time, count, cycles, interval, probability })
        })?;
        // SizeModule: curve, y, z
        let on = flag(r)?;
        let (x, y, z) = (c(r)?, c(r)?, c(r)?);
        let separate = flag(r)?;
        let size = on.then_some(SizeOverLifetime { x, y, z, separate });
        // RotationModule: x, y, curve
        let on = flag(r)?;
        (c(r)?, c(r)?);
        let curve = c(r)?;
        let mut rotation_separate = flag(r)? && on;
        let rotation = on.then_some(curve);
        // ColorModule
        let on = flag(r)?;
        let gradient = MinMaxGradient::read(r)?;
        let color = on.then_some(gradient);
        // UVModule
        let on = flag(r)?;
        let (mode, time_mode, fps) = (r.i32()?, r.i32()?, r.f32()?);
        let (frame_over_time, start_frame) = (c(r)?, c(r)?);
        let speed_range = floats(r)?;
        let (tiles_x, tiles_y, animation_type, row_index) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
        let cycles = r.f32()?;
        let (uv_channel_mask, row_mode) = (r.i32()?, r.i32()?);
        let sprites = vec(r, PPtr::read)?;
        let flip_uv = floats(r)?;
        let uv_mode = mode;
        let sheet = on.then_some(Sheet { mode, time_mode, fps, frame_over_time, start_frame, tiles_x, tiles_y, animation_type, row_index, cycles, row_mode, uv_channel_mask, speed_range, sprites, flip_uv });
        // VelocityModule: x, y, z, orbitals, offsets, radial, speedModifier
        let on = flag(r)?;
        let axes = [c(r)?, c(r)?, c(r)?];
        let orbital = [c(r)?, c(r)?, c(r)?];
        let offset = [c(r)?, c(r)?, c(r)?];
        let (radial, speed_modifier) = (c(r)?, c(r)?);
        let world = flag(r)?;
        let velocity = on.then_some(Velocity { axes, orbital, offset, radial, speed_modifier, world });
        // InheritVelocityModule
        let on = flag(r)?;
        let mode = r.i32()?;
        let curve = c(r)?;
        let inherit = on.then_some((mode, curve));
        // LifetimeByEmitterSpeedModule
        let on = flag(r)?;
        let curve = c(r)?;
        let range = floats(r)?;
        let lifetime_by_speed = on.then_some((curve, range));
        // ForceModule
        let on = flag(r)?;
        let axes = [c(r)?, c(r)?, c(r)?];
        let [world, randomize_per_frame] = flags(r)?;
        let force = on.then_some(Force { axes, world, randomize_per_frame });
        // ExternalForcesModule
        let on = flag(r)?;
        let multiplier = c(r)?;
        let (influence_filter, influence_mask) = (r.i32()?, r.u32()?);
        let influence = vec(r, PPtr::read)?;
        let external_forces = on.then_some(ExternalForces { multiplier, influence_filter, influence_mask, influence });
        // ClampVelocityModule
        let on = flag(r)?;
        (c(r)?, c(r)?, c(r)?);
        let magnitude = c(r)?;
        let [separate, _world, drag_by_size, drag_by_velocity] = flags(r)?;
        let dampen = r.f32()?;
        let drag = c(r)?;
        if on && separate {
            return Err("separate velocity limits".into());
        }
        let clamp = on.then_some(Clamp { magnitude, dampen, drag, drag_by_size, drag_by_velocity });
        // NoiseModule
        let on = flag(r)?;
        let strength = [c(r)?, c(r)?, c(r)?];
        let separate = flag(r)?;
        let frequency = r.f32()?;
        let damping = flag(r)?;
        let (octaves, octave_multiplier, octave_scale, quality) = (r.i32()?, r.f32()?, r.f32()?, r.i32()?);
        let (scroll_speed, remap) = (c(r)?, [c(r)?, c(r)?, c(r)?]);
        let remap_enabled = flag(r)?;
        let amounts = [c(r)?, c(r)?, c(r)?];
        let noise = on.then_some(Noise { strength, separate, frequency, damping, octaves, octave_multiplier, octave_scale, quality, scroll_speed, remap, remap_enabled, amounts });
        // SizeBySpeedModule, RotationBySpeedModule, ColorBySpeedModule
        let on = flag(r)?;
        let (x, y, z) = (c(r)?, c(r)?, c(r)?);
        let range = floats(r)?;
        let separate = flag(r)?;
        let size_by_speed = on.then_some((SizeOverLifetime { x, y, z, separate }, range));
        let on = flag(r)?;
        (c(r)?, c(r)?);
        let curve = c(r)?;
        rotation_separate |= flag(r)? && on;
        let range = floats(r)?;
        let rotation_by_speed = on.then_some((curve, range));
        let on = flag(r)?;
        let gradient = MinMaxGradient::read(r)?;
        let range = floats(r)?;
        let color_by_speed = on.then_some((gradient, range));
        // CollisionModule
        let on = flag(r)?;
        let (kind, mode, collider_force) = (r.i32()?, r.i32()?, r.f32()?);
        flags::<3>(r)?;
        let planes = vec(r, PPtr::read)?.len();
        let (dampen, bounce, lifetime_loss) = (c(r)?, c(r)?, c(r)?);
        let [min_kill_speed, max_kill_speed, radius_scale] = floats(r)?;
        let collides_with = r.u32()?;
        let (max_shapes, quality, voxel_size) = (r.i32()?, r.i32()?, r.f32()?);
        let [messages, dynamic, interior] = flags::<3>(r)?;
        let collision = Some(Collision { enabled: on, kind, mode, collider_force, planes, dampen, bounce, lifetime_loss, min_kill_speed, max_kill_speed, radius_scale, collides_with, max_shapes, quality, voxel_size, messages, dynamic, interior });
        // TriggerModule
        let trigger = flag(r)?;
        for _ in 0..5 {
            r.i32()?;
        }
        r.f32()?;
        vec(r, PPtr::read)?;
        // SubModule
        let on = flag(r)?;
        let subs = vec(r, |r| {
            let emitter = PPtr::read(r)?;
            let (kind, properties) = (r.i32()?, r.i32()?);
            Ok(SubEmitter { emitter, kind, properties, probability: r.f32()? })
        })?;
        let subs = if on { subs } else { Vec::new() };
        // LightsModule
        let lights = flag(r)?;
        r.f32()?;
        let lights_light = PPtr::read(r)?;
        flags::<4>(r)?;
        let lights_range = c(r)?.scalar;
        c(r)?;
        r.i32()?;
        // TrailModule
        let on = flag(r)?;
        let mode = r.i32()?;
        let ratio = r.f32()?;
        let lifetime = c(r)?;
        let min_vertex_distance = r.f32()?;
        let (texture_mode, ribbon_count) = (r.i32()?, r.i32()?);
        r.f32()?; // shadowBias: the trail job takes it in shadow passes only (Initialize 0x180dbc8e0)
        let [world_space, die_with_particles, size_affects_width, size_affects_lifetime, inherit_particle_color, lighting_data, split_sub_emitter_ribbons, attach_ribbons] = flags::<8>(r)?;
        let color_over_lifetime = MinMaxGradient::read(r)?;
        let width_over_trail = c(r)?;
        let color_over_trail = MinMaxGradient::read(r)?;
        let trail = on.then_some(Trail { mode, ratio, lifetime, min_vertex_distance, world_space, die_with_particles, size_affects_width, size_affects_lifetime, inherit_particle_color, texture_mode, ribbon_count, lighting_data, split_sub_emitter_ribbons, attach_ribbons, color_over_lifetime, width_over_trail, color_over_trail });
        // CustomDataModule
        let custom_data = flag(r)?;
        for _ in 0..2 {
            (r.i32()?, r.i32()?);
            MinMaxGradient::read(r)?;
            (c(r)?, c(r)?, c(r)?, c(r)?);
        }
        if r.pos != data.len() {
            return Err(format!("ParticleSystem: read {} of {} bytes", r.pos, data.len()));
        }
        Ok(Self {
            stop_action, culling_mode, ring_buffer_mode, prewarm, unscaled_time, auto_random_seed, rigidbody_velocity, random_seed, external_forces, trigger, lights, lights_light, lights_range, custom_data,
            length, simulation_speed, looping, play_on_awake, start_delay, space, scaling, start_lifetime, start_speed, start_color, start_size, start_size_y, start_size_z, start_rotation, start_rotation_xy, start_raw_3d, randomize_rotation_direction, max_particles, gravity,
            shape_enabled, shape_type, shape_placement, shape_angle, shape_length, shape_radius_thickness, shape_position, shape_rotation, shape_scale, shape_random_direction, shape_spherical_direction, shape_random_position, shape_radius, shape_arc, shape_box_thickness, shape_align_to_direction, shape_texture, shape_texture_ref, shape_texture_params, shape_pixels: None, shape_normal_offset, shape_mesh_spawn, shape_use_material_index, shape_use_mesh_colors,
            emission_enabled, rate_over_time, rate_over_distance, bursts, size, rotation, color, sheet, uv_mode, velocity, force, inherit, lifetime_by_speed, clamp, noise, rotation_by_speed, rotation_separate, size_by_speed, color_by_speed, collision, subs, trail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every particle system of the game reads to its last byte.
    #[test]
    fn all_particle_systems_read() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let (mut n, mut bad) = (0, std::collections::BTreeMap::new());
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                n += 1;
                if let Err(e) = ParticleSystem::read(f.data(o)) {
                    *bad.entry(e).or_insert(0) += 1;
                }
            }
        }
        assert!(n > 100, "{n}");
        assert!(bad.is_empty(), "{n} systems: {bad:?}");
    }

    /// The modules and modes the game's systems use (`cargo test particle_inventory -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn particle_inventory() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut m = std::collections::BTreeMap::<String, usize>::new();
        let mut add = |k: String| *m.entry(k).or_insert(0) += 1;
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let p = ParticleSystem::read(f.data(o)).unwrap();
                add("systems".into());
                add(format!("stop_action {}", p.stop_action));
                add(format!("ring_buffer {} rotation3d {} size3d {}", p.ring_buffer_mode, p.start_rotation_xy.is_some(), p.start_size_z.is_some()));
                add(format!("looping {} prewarm {} auto_seed {} delay state {}", p.looping, p.prewarm, p.auto_random_seed, p.start_delay.state));
                add(format!("space {} scaling {} unscaled {} rb_velocity {}", p.space, p.scaling, p.unscaled_time, p.rigidbody_velocity));
                add(format!("lifetime state {}", p.start_lifetime.state));
                add(format!("speed state {}", p.start_speed.state));
                add(format!("size state {} 3d {}", p.start_size.state, p.start_size_y.is_some()));
                add(format!("rotation state {}", p.start_rotation.state));
                add(format!("color state {}", p.start_color.state));
                add(format!("gravity state {}", p.gravity.state));
                add(format!("shape {} type {}", p.shape_enabled, p.shape_type));
                if p.shape_enabled {
                    let st = |c: &MinMaxCurve| c.state;
                    add(format!(
                        "shape type {} radius mode {} spread {} speed {}/{} arc mode {} spread {} speed {}/{} thick {} rnddir {} sph {} rndpos {} align {} texture {} space {} scaling {} rot {:?} scale {:?}",
                        p.shape_type, p.shape_radius.mode, p.shape_radius.spread, st(&p.shape_radius.speed), p.shape_radius.speed.scalar, p.shape_arc.mode, p.shape_arc.spread, st(&p.shape_arc.speed), p.shape_arc.speed.scalar,
                        p.shape_radius_thickness, p.shape_random_direction, p.shape_spherical_direction, p.shape_random_position, p.shape_align_to_direction, p.shape_texture, p.space, p.scaling,
                        p.shape_rotation != [0.0; 3], p.shape_scale != [1.0; 3]
                    ));
                }
                add(format!("emission {} rate state {} distance state {} bursts {}", p.emission_enabled, p.rate_over_time.state, p.rate_over_distance.state, p.bursts.len()));
                for b in &p.bursts {
                    add(format!("burst count state {} cycles {} probability<1 {}", b.count.state, b.cycles, b.probability < 1.0));
                }
                if let Some(x) = &p.size { add(format!("size_over_life state {} separate {}", x.x.state, x.separate)); }
                if let Some(x) = &p.rotation { add(format!("rotation_over_life state {}", x.state)); }
                if let Some(x) = &p.color { add(format!("color_over_life state {}", x.state)); }
                if let Some(x) = &p.sheet { add(format!("sheet mode {} time {} anim {}", x.mode, x.time_mode, x.animation_type)); }
                if let Some(v) = &p.velocity {
                    let st = |c: &[MinMaxCurve; 3]| c.iter().map(|c| c.state).collect::<Vec<_>>();
                    let nz = |c: &MinMaxCurve| c.state != 0 || c.scalar != 0.0;
                    add(format!(
                        "velocity_over_life {:?} world {} orbital {} offset {} radial {} speed_modifier {}/{}",
                        st(&v.axes), v.world, v.orbital.iter().any(nz), v.offset.iter().any(nz), nz(&v.radial), v.speed_modifier.state, v.speed_modifier.scalar
                    ));
                }
                if let Some(f) = &p.force { add(format!("force_over_life {:?} world {} per_frame {}", f.axes.iter().map(|c| c.state).collect::<Vec<_>>(), f.world, f.randomize_per_frame)); }
                add(format!("gravity {} {} space {}", p.gravity.state, p.gravity.scalar != 0.0, p.space));
                if let Some((mo, x)) = &p.inherit { add(format!("inherit mode {mo} state {}", x.state)); }
                if p.lifetime_by_speed.is_some() { add("lifetime_by_speed".into()); }
                if let Some(x) = &p.clamp { add(format!("limit_velocity mag state {} dampen {} drag state {} by_size {} by_vel {}", x.magnitude.state, x.dampen, x.drag.state, x.drag_by_size, x.drag_by_velocity)); }
                if let Some((c, r)) = &p.lifetime_by_speed { add(format!("lifetime_by_speed state {} range {r:?}", c.state)); }
                if p.noise.is_some() { add("noise".into()); }
                if p.size_by_speed.is_some() { add("size_by_speed".into()); }
                if p.rotation_by_speed.is_some() { add("rotation_by_speed".into()); }
                if p.color_by_speed.is_some() { add("color_by_speed".into()); }
                if let Some(x) = &p.external_forces {
                    let m = &x.multiplier;
                    add(format!("external_forces multiplier {:?} filter {} mask {:x} list {}", (m.state, m.min_scalar, m.scalar), x.influence_filter, x.influence_mask, x.influence.len()));
                }
                if let Some(c) = p.collision.as_ref().filter(|c| c.enabled) {
                    add(format!("collision kind {} mode {} force {} messages {} quality {} dynamic {}", c.kind, c.mode, c.collider_force, c.messages, c.quality, c.dynamic));
                    let st = |c: &MinMaxCurve| (c.state, c.min_scalar, c.scalar);
                    add(format!(
                        "collision detail kind {} mode {} planes {} dampen {:?} bounce {:?} loss {:?} kill {}..{} radius {} mask {:x} shapes {} quality {} interior {} space {}",
                        c.kind, c.mode, c.planes, st(&c.dampen), st(&c.bounce), st(&c.lifetime_loss), c.min_kill_speed, c.max_kill_speed, c.radius_scale, c.collides_with, c.max_shapes, c.quality, c.interior, p.space
                    ));
                }
                if let Some(c) = p.collision.as_ref().filter(|c| !c.enabled) {
                    add(format!("collision off kind {} mode {} quality {} dynamic {} force {}", c.kind, c.mode, c.quality, c.dynamic, c.collider_force));
                }
                for s in &p.subs { add(format!("sub kind {} props {}", s.kind, s.properties)); }
                if let Some(t) = &p.trail { add(format!("trail mode {} ratio {}", t.mode, t.ratio)); }
                // UpdateBounds' branches.
                let sep = |s: &Option<SizeOverLifetime>| s.as_ref().is_some_and(|c| c.separate);
                add(format!("bounds size3d {} size_by_speed {} lights {} uv_sprites {}", p.start_size_y.is_some() || sep(&p.size) || p.size_by_speed.as_ref().is_some_and(|c| c.0.separate), p.size_by_speed.is_some(), p.lights, p.uv_mode == 1));
                // DetermineSupportsProcedural (1801b8730) before its curve checks.
                let procedural = p.supports_procedural();
                add(format!("procedural candidate {procedural}"));
        if procedural {
            let st = |c: &[MinMaxCurve]| c.iter().map(|c| c.state.to_string()).collect::<Vec<_>>().join("");
            add(format!(
                "procedural shape {} {} velocity {:?} force {:?} rotation {:?} separate {} lifetime {} speed {} size {}",
                p.shape_enabled, p.shape_type, p.velocity.as_ref().map(|v| (st(&v.axes), v.world)), p.force.as_ref().map(|f| (st(&f.axes), f.world)),
                p.rotation.as_ref().map(|r| r.state), p.rotation_separate, p.start_lifetime.state, p.start_speed.state, p.start_size.state
            ));
        }
                for (on, n) in [(p.trigger, "trigger"), (p.lights, "lights"), (p.custom_data, "custom_data")] {
                    if on { add(n.into()); }
                }
            }
        }
        for (k, v) in &m {
            println!("{v:5} {k}");
        }
    }

    /// The systems with a textured Rectangle, a sprite shape or orbital/radial velocity, by GameObject name
    /// (`cargo test particle_shape_velocity_scan -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn particle_shape_velocity_scan() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        for (fi, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let p = ParticleSystem::read(f.data(o)).unwrap();
                let name = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|g| db.resolve(fi, g)).and_then(|g| db.game_object(g).ok()).map_or("?".into(), |g| g.name);
                let file = f.path.file_name().unwrap();
                if p.shape_enabled && (p.shape_type == 20 || (p.shape_type == 18 && p.shape_texture)) {
                    println!("shape {file:?} {name} type {} space {} scale {:?} rot {:?} pos {:?} rnddir {} sph {} rndpos {} align {}", p.shape_type, p.space, p.shape_scale, p.shape_rotation, p.shape_position, p.shape_random_direction, p.shape_spherical_direction, p.shape_random_position, p.shape_align_to_direction);
                    println!("  placement {} spawn {:?} normal {} matidx {} colors {} scaling {}", p.shape_placement, (p.shape_mesh_spawn.0, p.shape_mesh_spawn.1, p.shape_mesh_spawn.2.state, p.shape_mesh_spawn.2.scalar), p.shape_normal_offset, p.shape_use_material_index, p.shape_use_mesh_colors, p.scaling);
                    if let Some(t) = db.resolve(fi, p.shape_texture_ref).filter(|_| p.shape_texture) {
                        let t = crate::assets::components::Texture2D::read(db.data(t)).unwrap();
                        println!("  texture {} {}x{} format {} readable {} {:?}", t.name, t.width, t.height, t.format, t.readable, p.shape_texture_params);
                    }
                }
                if let Some(v) = &p.velocity {
                    let nz = |c: &MinMaxCurve| c.state != 0 || c.scalar != 0.0;
                    if v.orbital.iter().chain(&v.offset).any(nz) || nz(&v.radial) {
                        let st = |c: &MinMaxCurve| (c.state, c.min_scalar, c.scalar, c.max.keys.len());
                        println!("orbital {file:?} {name} space {} world {} axes {:?} orbital {:?} offset {:?} radial {:?} speed {:?}", p.space, v.world, v.axes.each_ref().map(st), v.orbital.each_ref().map(st), v.offset.each_ref().map(st), st(&v.radial), st(&v.speed_modifier));
                    }
                }
            }
        }
    }

    /// Every Sprite's vertex channels and submeshes (the SpriteRenderer shape reads Color, channel 3)
    /// (`cargo test sprite_channel_scan -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn sprite_channel_scan() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut seen = std::collections::BTreeMap::new();
        for f in db.files.iter() {
            for o in f.objects.iter().filter(|o| o.class_id == 213) {
                let Ok(s) = crate::assets::components::Sprite::read(f.data(o)) else { continue };
                let k = format!("{:?} subs {:?} index {}", s.channels, s.sub_meshes.iter().map(|m| (m.topology, m.base_vertex, m.first_byte)).collect::<Vec<_>>(), s.index_buffer.len() / 2 % 3);
                *seen.entry(k).or_insert(0) += 1;
            }
        }
        for (k, n) in seen.iter().take(40) {
            println!("sprite {n} {k}");
        }
        println!("sprite kinds {}", seen.len());
    }

    /// The systems whose stop action reaches a script or a GameObject: their timing inputs.
    #[test]
    #[ignore]
    fn particle_stop_actions() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        for (fi, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let p = ParticleSystem::read(f.data(o)).unwrap();
                if p.stop_action == 0 {
                    continue;
                }
                let me = crate::assets::db::ObjRef { file: fi, path_id: o.path_id };
                let name = crate::assets::components::game_object_of(db.data(me)).ok().and_then(|g| db.resolve(fi, g)).and_then(|g| db.game_object(g).ok()).map_or(String::new(), |g| g.name);
                let bursts: Vec<_> = p.bursts.iter().map(|b| (b.time, b.cycles, b.interval)).collect();
                println!(
                    "{name:30} stop {} loop {} len {} delay {}/{}..{} life {}/{}..{} rate {}/{} dist {} bursts {bursts:?} subs {} speed {} trail {}",
                    p.stop_action, p.looping, p.length, p.start_delay.state, p.start_delay.min_scalar, p.start_delay.scalar, p.start_lifetime.state, p.start_lifetime.min_scalar, p.start_lifetime.scalar,
                    p.rate_over_time.state, p.rate_over_time.scalar, p.rate_over_distance.scalar, p.subs.len(), p.simulation_speed, p.trail.is_some()
                );
            }
        }
    }

    /// And every renderer, with only the render modes the rewrite draws.
    #[test]
    fn all_particle_renderers_read() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut modes = std::collections::BTreeMap::new();
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 199) {
                let r = crate::assets::components::ParticleRenderer::read(f.data(o)).unwrap();
                *modes.entry((r.render_mode, r.mask_interaction, r.apply_active_color_space, r.custom_streams)).or_insert(0) += 1;
            }
        }
        println!("{modes:?}");
        assert!(modes.keys().all(|m| matches!(m.0, 0 | 5) && (0..=2).contains(&m.1)), "{modes:?}");
    }
}
