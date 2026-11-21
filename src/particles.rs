//! The ParticleSystems' presentation: their own randomness, nothing here reaches the simulation.
use crate::assets::components::{Material, ParticleRenderer};
use crate::assets::db::{AssetDb, Component, ObjRef};
use crate::assets::particles::{Curve, MinMaxCurve, ParticleSystem};
use crate::unity::Random as Rand;
use crate::game::Game;
use crate::play::{load_texture, render_queue, sprite_line_key, Play, Textures};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    camera::visibility::NoFrustumCulling,
    mesh::{MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexFormat},
    prelude::*,
    reflect::TypePath,
    render::storage::{ShaderBuffer, ShaderBufferData},
    render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};
use std::collections::HashMap;

pub(crate) const UV2_BLEND: MeshVertexAttribute = MeshVertexAttribute::new("Uv2Blend", 0x5057_0001, VertexFormat::Float32x3);

#[derive(Clone, Copy, Default, bevy::render::render_resource::ShaderType)]
pub struct Settings {
    color: Vec4,
    st: Vec4,
    color_add: Vec2,
    cutoff: f32,
    kind: u32,
    mode: u32,
    blend_frames: u32,
}

/// Particles/Standard Unlit and Unlit/AlphaCutoff: see particle.wesl.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(ParticleKey)]
pub struct ParticleMaterial {
    #[uniform(0)]
    settings: Settings,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
    subtract: bool,
    /// Every particle copy's vertices this frame (see `particles`); a line's material binds it unread.
    #[storage(3, read_only, visibility(vertex))]
    vertices: Handle<ShaderBuffer>,
    /// A particle copy's draw: its vertices come from `vertices` (vertex_pulled), the mesh only gives their count.
    pulled: bool,
}

/// The one buffer `particles` writes all the copies' vertices to each frame (one upload in place of a mesh each).
pub const PULLED_VERTICES: Handle<ShaderBuffer> = bevy::asset::uuid_handle!("6c1f3f0e-5d0b-4a5e-9a51-3b0c7f6a2d11");

#[repr(C)]
#[derive(PartialEq, Eq, Hash, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParticleKey {
    subtract: u32,
    pulled: u32,
}

impl From<&ParticleMaterial> for ParticleKey {
    fn from(m: &ParticleMaterial) -> Self {
        Self { subtract: m.subtract as u32, pulled: m.pulled as u32 }
    }
}

impl Material2d for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://people_playground_bevy/particle.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/particle.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, key: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if key.bind_group_data.pulled != 0 {
            descriptor.vertex.entry_point = Some("vertex_pulled".into());
            descriptor.vertex.buffers = vec![layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?];
        } else {
            // Two vertex entry points in the module: this one named.
            descriptor.vertex.entry_point = Some("vertex".into());
            descriptor.vertex.buffers = vec![layout.0.get_layout(&[
                Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
                Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
                UV2_BLEND.at_shader_location(3),
            ])?];
        }
        let operation = if key.bind_group_data.subtract != 0 { BlendOperation::ReverseSubtract } else { BlendOperation::Add };
        let component = BlendComponent { src_factor: BlendFactor::One, dst_factor: if key.bind_group_data.subtract != 0 { BlendFactor::One } else { BlendFactor::OneMinusSrcAlpha }, operation };
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: component, alpha: component });
            }
        }
        Ok(())
    }
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "particle.wesl");
    app.add_plugins(Material2dPlugin::<ParticleMaterial>::default());
    let _ = app.world_mut().resource_mut::<Assets<ShaderBuffer>>().insert(&PULLED_VERTICES, ShaderBuffer::new(vec![0u32; 1 << 14], RenderAssetUsages::default()));
}

/// AnimationCurve.Evaluate: Hermite between keys, clamped outside (the particle curves' wrap modes never loop here).
fn curve(c: &Curve, t: f32) -> f32 {
    let Some(first) = c.keys.first() else { return 0.0 };
    let last = c.keys[c.keys.len() - 1];
    if t <= first.time {
        return first.value;
    }
    if t >= last.time {
        return last.value;
    }
    let i = c.keys.partition_point(|k| k.time <= t);
    let (a, b) = (c.keys[i - 1], c.keys[i]);
    let dt = b.time - a.time;
    let s = (t - a.time) / dt;
    let (m0, m1) = (a.out_slope * dt, b.in_slope * dt);
    if !m0.is_finite() || !m1.is_finite() {
        return a.value;
    }
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * a.value + (s3 - 2.0 * s2 + s) * m0 + (-2.0 * s3 + 3.0 * s2) * b.value + (s3 - s2) * m1
}

/// MinMaxCurve.Evaluate(t, lerpFactor).
fn eval(c: &MinMaxCurve, t: f32, random: f32) -> f32 {
    eval_multiplied(c, c.scalar, t, random)
}

/// MinMaxCurve.Evaluate with the module's multiplier set to `scalar`.
fn eval_multiplied(c: &MinMaxCurve, scalar: f32, t: f32, random: f32) -> f32 {
    match c.state {
        0 => scalar,
        1 => scalar * curve(&c.max, t),
        2 => scalar * (curve(&c.min, t) + (curve(&c.max, t) - curve(&c.min, t)) * random),
        _ => c.min_scalar + (scalar - c.min_scalar) * random,
    }
}

/// GammaToLinearSpaceExact: colours pass through it with m_ApplyActiveColorSpace in a linear project.
/// GammaToLinearSpace: its HDR range is a plain power.
fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else if c < 1.0 {
        ((c + 0.055) / 1.055).powf(2.4)
    } else if c == 1.0 {
        1.0
    } else {
        c.powf(2.2)
    }
}

/// Xorshift: the particles' look, not the game's Random.
pub struct Rng(u32);

impl Rng {
    fn value(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16777216.0
    }
}

#[derive(Default, Clone)]
pub(crate) struct Particle {
    pub(crate) position: Vec3,
    pub(crate) velocity: Vec3,
    pub(crate) age: f32,
    pub(crate) lifetime: f32,
    /// startSize x, y, z (unscaled: the renderer applies the render scale).
    pub(crate) size: f32,
    pub(crate) size_y: f32,
    pub(crate) size_z: f32,
    pub(crate) rotation: f32,
    pub(crate) trail: crate::particle_trails::TrailPoints,
    /// ParticleSystemParticles.randomSeed, from InitialModule::Start.
    pub(crate) seed: u32,
    /// ParticleSystemParticles.parentRandomSeed: the parent particle's randomSeed for a sub-emitter's particle
    /// (RecordEmit 1801aa3b0 / 1801aa0d0 through m_InheritedValues), 0 otherwise.
    pub(crate) parent_seed: u32,
    /// The start colour as InitialModule::Start writes it, ColorRGBA32 r | g << 8 | b << 16 | a << 24.
    pub(crate) color32: u32,
    /// aliveTimePercent, 0..100, aged by SimulateParticles' rule from invStartLifetime.
    pub(crate) alive: f32,
    pub(crate) inv_lifetime: f32,
    /// animatedVelocity, rotationalSpeed (radians per second) and currentSize.
    pub(crate) animated: Vec3,
    pub(crate) rotational_speed: f32,
    pub(crate) current_size: Vec2,
    pub(crate) current_z: f32,
    /// The position before the last SimulateParticles, for the collision module.
    pub(crate) from: Vec3,
    /// ParticleSystemParticles.emitAccumulator: the birth sub-emitters' toEmit, for the first two.
    pub(crate) emit_accumulator: [f32; 2],
    /// ParticleSystemParticles.noiseImpulse: the noise job's last value, for NoiseModule::UpdateSize.
    pub(crate) noise_impulse: [f32; 3],
}

/// One playing ParticleSystem, simulated in its system's space (a local one's particles stay local): nothing moves a
/// played effect but a Drive.
#[derive(Default)]
pub struct Run {
    sys: usize,
    position: Vec3,
    rotation: Quat,
    /// The render scale and localToWorld's scale (1 in Shape scaling).
    scale: f32,
    /// ShapeModule's m_Random and timers.
    shape: Option<crate::game::particle_shape::State>,
    /// CollisionModule's m_Random, plane cache and ray budget.
    collision: Option<crate::game::particle_collision::State>,
    /// The native clock: t, accumulatedDt, delayT and stopEmitting.
    clock: crate::game::particle_clock::Clock,
    /// The emission state: its Rand, toEmit and spacing.
    emitted: Option<crate::game::particle_sim::Emission>,
    /// Update1b's emitterVelocity and the oldPosition it is taken from.
    emitter_velocity: Vec3,
    old_position: Option<Vec3>,
    /// ringBufferIndex.
    ring: usize,
    particles: Vec<Particle>,
    /// The particle arrays' slots past the count, from the count on: KillParticle leaves the last slot as it was, and
    /// the 4-wide modules move the last batch's lanes past the count with the rest. None: a slot no particle has held
    /// since the arrays grew (array_resize 1801ca620 leaves new floats uninitialised; the growth reallocs keep the
    /// old ones), whatever the heap held, taken as out of range.
    tail: Vec<Option<Particle>>,
    /// BloodImpactBehaviour.SetColor.
    color: Option<[f32; 4]>,
    root: Option<i32>,
    stopped: bool,
    /// The clone's scale, for its sub-emitters in Hierarchy scaling.
    clone_scale: f32,
    /// The sub-emitter systems, one per distinct system of the sub entries (a sub system is a child ParticleSystem that
    /// MarkAllSubEmitters 1801bccf0 sets stopEmitting on: it only gets particles from ParticleSystem::Emit).
    children: Vec<Run>,
    /// PhysicalBehaviour's writes to its emission and main modules.
    emission: Option<Emission>,
    /// WaterImpactBehaviour.Shot's writes: shape radius, start speed, start size by system.
    water: Option<std::sync::Arc<(f32, f32, HashMap<usize, f32>)>>,
    /// TrailModule's clock (+0xf4): each step's dt, zeroed by Reset.
    trail_time: f32,
    /// A SpriteRenderer-shaped system's renderer (the attached systems' physical).
    renderer: Option<Renderer>,
    /// ResetSeeds at Play: randomSeed, the emission Rand and InitialModule's RandN.
    seeds: Option<crate::game::particle_rand::Seeds>,
    /// NoiseModule's m_ScrollOffset (ResetScroll in AwakeFromLoad: 0).
    noise_scroll: f32,
    /// ExternalForcesModule's cachedForces (Cache in Update1a).
    forces: Vec<crate::game::particle_forces::Force>,
    /// ParticleSystemState.minMaxAABB (min x, y, z, max x, y, z; the ctor's zeros) and maxSize.
    bounds: [f32; 6],
    max_size: f32,
    /// ParticleSystemState.invalidateProcedural (EmitParticlesExternal, MarkAllSubEmitters, BleedingParticleBehaviour's
    /// writes; the other scripts' writes are their fields here).
    invalid: bool,
    /// ParticleSystemState.culled: RendererBecameInvisible took it out of the active list; it does not update until
    /// RendererBecameVisible.
    culled: bool,
    /// ParticleSystemState.cullTime and stopTime (the frame times RendererBecameInvisible and Stop took; the ctor's 0).
    cull_time: f64,
    stop_time: f64,
    /// `stopped` as the last update saw it: a Stop since then takes this frame's time.
    was_stopped: bool,
    /// The drive the last update had: the module values a catch-up Simulate runs with.
    drive: Option<Drive>,
    /// The culling saw its renderer at the last render (RendererScene's visibility bit).
    seen: bool,
    /// Simulate's kSimulateFixedTimeStep: the update steps by Time.fixedDeltaTime (GetTimeStep 1801bbe50).
    fixed: bool,
    /// Its place in ParticleSystemManager's lists, and playState == kStopped (set by EndUpdateAll).
    id: u32,
    halted: bool,
    /// The clone it was played in (0: none) and its prefab object with that object's ancestors (empty: none).
    group: u32,
    chain: Vec<usize>,
    /// The pooled clone it belongs to.
    pooled: Option<crate::game::Pooled>,
    /// A script's module writes (PsWrites), the latest of each.
    writes: PsWrites,
    /// Its prefab object's name, then its ancestors'.
    names: Vec<String>,
    /// The clone's root follows this parent; the system's offset and rotation under the root, and the clone's scale.
    parent: Option<(crate::game::EffectParent, Vec3, Quat, f32)>,
}

/// PhysicalBehaviour.ManagedUpdate's writes to a system under it (None keeps the prefab's).
#[derive(Clone, Copy)]
struct Emission {
    enabled: bool,
    rate: Option<f32>,
    distance: Option<f32>,
    size: Option<f32>,
}

/// BleedingParticleBehaviour.Update's writes to the system.
#[derive(Clone, Copy)]
struct Drive {
    /// The rigidbody's velocity (useRigidbodyForVelocity).
    velocity: Vec3,
    enabled: bool,
    rate: f32,
    speed: f32,
    color: [f32; 4],
    radius: f32,
    arc: f32,
    drag_max: f32,
    /// collision.enabled, set by BleedingParticleBehaviour.Awake.
    collision: bool,
}

/// A ParticleSystem with what its renderer draws and its sub-emitters by index.
#[derive(Clone)]
pub struct Sys {
    /// The GameObject's layer: which camera draws the system.
    game_layer: i32,
    /// ActivateIfFancyEffects sits on its GameObject, and whether that leaves it on.
    fancy_only: bool,
    awake: bool,
    /// Its transform's lossy scale in the prefab and own scale.
    hierarchy: f32,
    local: f32,
    size_range: [f32; 2],
    /// ParticleSystemRenderer renderAlignment (0 view, 1 world, 2 local) and allowRoll.
    alignment: i32,
    allow_roll: bool,
    /// Render mode 1 (Stretched Billboard): ([m_CameraVelocityScale, m_VelocityScale, m_LengthScale],
    /// m_FreeformStretching, m_RotateWithStretchDirection).
    stretch: Option<([f32; 3], bool, bool)>,
    ps: ParticleSystem,
    /// (system, kind, properties, probability)
    subs: Vec<(usize, i32, i32, f32)>,
    /// The sub-emitter's rotation in its prefab.
    rotation: Quat,
    /// Its world pose in its prefab (None for a system no prefab walk reached) and its Transform's localRotation.
    prefab: Option<(Vec3, Quat)>,
    local_rotation: Quat,
    pivot: Vec3,
    flip: Vec2,
    linear: bool,
    /// UVModule's sprite rect data (m_SpriteRectData): per sprite (u0, v0, u1, v1, pivot x, pivot y, width, height).
    frames: Vec<[f32; 8]>,
    /// Its GameObject (the renderer's materials) and the sheet's sprite texture.
    go: Option<ObjRef>,
    sheet_texture: Option<ObjRef>,
    /// Time.maximumParticleDeltaTime.
    max_step: f32,
    /// Its ParticleSystem's original instance id: the clone's AwakeFromLoad (Play, AddToManager) order.
    order: i32,
    /// Whether the first render culls it (RendererBecameInvisible) and makes it visible again.
    first_cull: Cull,
}

/// A renderer's mesh in world space.
pub struct Output {
    material: Handle<ParticleMaterial>,
    key: (i32, i16, i16, f32, usize),
}

/// Where a prefab's systems sit in it.
#[derive(Clone)]
struct Part {
    sys: usize,
    /// Its prefab object, then that object's ancestors.
    chain: Vec<usize>,
    /// Those objects' names.
    names: Vec<String>,
    position: Vec3,
    rotation: Quat,
    /// Lossy and own scale.
    hierarchy: f32,
    local: f32,
}

#[derive(Default)]
struct Buffers {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    uv2: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

/// The SpriteRenderer a SpriteRenderer-shaped system follows: GetActiveTransform (1801bafc0) hands its Transform to
/// UpdateLocalToWorldMatrixAndScales (1801c7300), flips aside. Its localToWorldMatrix, world position and rotation,
/// lossy scale.
#[derive(Clone, Debug)]
struct Renderer {
    l2w: [f32; 16],
    position: [f32; 3],
    rotation: [f32; 4],
    lossy: [f32; 3],
}

fn start(sys: usize, position: Vec3, rotation: Quat, scale: f32, systems: &[Sys], manager: &mut Manager) -> Run {
    start_with(sys, position, rotation, scale, systems, manager, &mut HashMap::new())
}

/// Play's ResetSeeds and AddToManager of the systems `sys` starts (it and its sub-emitter children), in the clone's
/// AwakeFromLoad order: by original instance id.
fn awake_order(sys: &[usize], systems: &[Sys], manager: &mut Manager) -> HashMap<usize, (crate::game::particle_rand::Seeds, u32)> {
    let mut all = Vec::new();
    let mut stack = sys.to_vec();
    while let Some(s) = stack.pop() {
        if !all.contains(&s) {
            all.push(s);
            stack.extend(sub_systems(&systems[s]));
        }
    }
    all.sort_by_key(|&s| systems[s].order);
    let mut out: HashMap<usize, (crate::game::particle_rand::Seeds, u32)> = HashMap::new();
    for s in all {
        out.insert(s, (crate::game::particle_rand::Seeds::reset(&systems[s].ps, &mut manager.rand), manager.add()));
        // ResetSeeds (1801bf2e0) draws for each sub-emitter too (1801bf4fe): an earlier-awoken one takes it, a later
        // one's own Play draws again over it.
        for c in sub_systems(&systems[s]) {
            let seeds = crate::game::particle_rand::Seeds::reset(&systems[c].ps, &mut manager.rand);
            if let Some(e) = out.get_mut(&c) {
                e.0 = seeds;
            }
        }
    }
    out
}

fn start_with(sys: usize, position: Vec3, rotation: Quat, scale: f32, systems: &[Sys], manager: &mut Manager, pre: &mut HashMap<usize, (crate::game::particle_rand::Seeds, u32)>) -> Run {
    let ps = &systems[sys].ps;
    let drawn = pre.remove(&sys);
    let has = drawn.is_some();
    let (seeds, id) = drawn.unwrap_or_else(|| (crate::game::particle_rand::Seeds::reset(ps, &mut manager.rand), 0));
    let clock = crate::game::particle_clock::Clock::play(&crate::game::particle_clock::Spec::of(ps, seeds.start_delay(&ps.start_delay)));
    let emitted = Some(crate::game::particle_sim::Emission::new(seeds.emission.clone()));
    let shape = Some(crate::game::particle_shape::State::new(seeds.random_seed));
    // Play: AddToManager.
    let id = if has { id } else { manager.add() };
    let collision = Some(crate::game::particle_collision::State::new(seeds.random_seed));
    let mut run = Run { sys, position, rotation, scale, shape, collision, clock, emitted, clone_scale: 1.0, seeds: Some(seeds), id, ..Default::default() };
    // Played with their parent (its children in the hierarchy), stopEmitting from MarkAllSubEmitters.
    for c in sub_systems(&systems[sys]) {
        let mut child = start_with(c, position, rotation, 1.0, systems, manager, pre);
        child.clock.stop_emitting = true;
        child.invalid = true;
        run.children.push(child);
    }
    run
}

/// The distinct systems of a system's sub entries, in entry order: its children runs.
fn sub_systems(s: &Sys) -> Vec<usize> {
    let mut out = Vec::new();
    for sub in &s.subs {
        if !matches!(sub.1, 0 | 2) {
            unimplemented!("mod-only: sub-emitter type {} (collision, trigger or manual): no game system uses it", sub.1);
        }
        if !out.contains(&sub.0) {
            out.push(sub.0);
        }
    }
    out
}

/// The child run of a sub entry.
fn child_index(s: &Sys, sub: usize) -> usize {
    sub_systems(s).iter().position(|c| *c == sub).expect("a sub entry's system has its child run")
}

/// A child run's transform from its parent's: the prefab's relative pose (the clone root through the parent's), the
/// scaling mode's scale and the scripts' writes the parent carries.
fn place_children(run: &mut Run, systems: &[Sys]) {
    let p = &systems[run.sys];
    for child in &mut run.children {
        let s = &systems[child.sys];
        let (position, rotation) = match (p.prefab, s.prefab) {
            (Some((pw, pr)), Some((cw, cr))) => {
                let turn = run.rotation * pr.inverse();
                let root = run.position - turn * (pw * run.clone_scale);
                (root + turn * (cw * run.clone_scale), turn * cr)
            }
            _ => (run.position, run.rotation * s.rotation),
        };
        child.position = position;
        child.rotation = rotation;
        child.scale = match s.ps.scaling {
            0 => s.hierarchy * run.clone_scale,
            1 => s.local,
            _ => 1.0,
        };
        child.clone_scale = run.clone_scale;
        child.water = run.water.clone();
    }
}

/// RequiresIncrementalSize (1801beda0): a module reads currentSize between the passes. `drag` is the limit
/// module's drag scalar after the scripts' writes; lights count whatever sizeAffectsRange says (nothing reads them).
fn incremental_size(ps: &ParticleSystem, drag: f32, collision: bool) -> bool {
    ps.clamp.as_ref().is_some_and(|c| drag != 0.0 && c.drag_by_size) || collision || ps.lights || ps.trail.as_ref().is_some_and(|t| t.size_affects_lifetime) || ps.trigger
}

/// The collision module of a system this frame. A bleeding particle's drive carries the collision.enabled its Awake
/// set under FancyEffects (`BleedingParticle::awake`).
fn collision_of<'a>(ps: &'a ParticleSystem, g: &Game, drive: Option<&Drive>) -> Option<&'a crate::assets::particles::Collision> {
    ps.collision.as_ref().filter(|c| g.prefs.FancyEffects && (c.enabled || drive.is_some_and(|d| d.collision)))
}

fn drag_scalar(ps: &ParticleSystem, drive: Option<&Drive>) -> f32 {
    ps.clamp.as_ref().map_or(0.0, |c| drive.map_or(c.drag.scalar, |d| d.drag_max))
}

/// localToWorld, worldToLocal and the render and emitter scales of a run (UpdateLocalToWorldMatrixAndScales
/// 0x1801c7300): one TRS node from the run's position, rotation and scale (1 in Shape scaling, whose emitterScale is
/// the global scale).
struct Pose {
    l2w: [f32; 16],
    w2l: [f32; 16],
    render: [f32; 3],
    emitter: [f32; 3],
}

fn pose(run: &Run, s: &Sys) -> Pose {
    if let Some(r) = run.renderer.as_ref().filter(|_| s.ps.shape_enabled && s.ps.shape_type == 20) {
        // The renderer's Transform: Hierarchy takes its matrix and the render scale is its lossy scale; Local takes
        // its world position and rotation, and the system's own local scale renders.
        let (l2w, render) = match s.ps.scaling {
            0 => (r.l2w, r.lossy),
            1 => {
                let node = crate::unity::transform::Trs { t: [r.position[0], r.position[1], r.position[2], 0.0], q: r.rotation, s: [1.0, 1.0, 1.0, 0.0] };
                (crate::unity::transform::local_to_world(&[node]), [run.scale; 3])
            }
            m => unimplemented!("mod-only: a SpriteRenderer-shaped system with scaling mode {m}: no game system uses it"),
        };
        return Pose { w2l: crate::game::particle_modules::affine_inverse(&l2w), l2w, render, emitter: [1.0; 3] };
    }
    let p = run.position;
    let k = run.scale;
    let node = crate::unity::transform::Trs { t: [p.x, p.y, p.z, 0.0], q: run.rotation.to_array(), s: [k, k, k, 0.0] };
    let l2w = crate::unity::transform::local_to_world(&[node]);
    let emitter = if s.ps.scaling == 2 { [s.hierarchy * run.clone_scale; 3] } else { [1.0; 3] };
    Pose { w2l: crate::game::particle_modules::affine_inverse(&l2w), l2w, render: [k; 3], emitter }
}

/// What UpdateModulesPreSimulationIncremental reads besides the particle.
struct Ctx {
    world: bool,
    l2w: crate::game::particle_modules::M3,
    w2l: crate::game::particle_modules::M3,
    render: [f32; 3],
    /// Physics.gravity (MapConfig sets it to Physics2D.gravity).
    gravity: [f32; 3],
    /// state.t / length.
    t: f32,
    emitter_velocity: [f32; 3],
    /// The limit module's drag scalar after the scripts' writes.
    drag: f32,
    /// NoiseModule's positionOffset * 100 and scrollOffset.
    noise: (crate::game::particle_modules::V3, f32),
    /// ExternalForcesModule::Update's cached forces, CalculateWorldBounds of the state's bounds and worldToLocal.
    external: Option<(Vec<crate::game::particle_forces::Force>, [f32; 6], [f32; 16])>,
    /// The cached forces' fields as ApplyForceField<0> takes them (None: not a field or out of reach).
    fields: Vec<Option<crate::game::particle_forces::FieldPrep>>,
    /// The state's localToWorld and worldToLocal.
    pose: ([f32; 16], [f32; 16]),
}

fn ctx(run: &Run, s: &Sys, drive: Option<&Drive>, g: &Game, state_t: f32) -> Ctx {
    use crate::game::particle_modules::columns;
    let pose = pose(run, s);
    let external = s.ps.external_forces.as_ref().map(|_| (run.forces.clone(), crate::game::particle_forces::world_bounds(&run.bounds, s.ps.space == 1, &pose.l2w), pose.w2l));
    let world = s.ps.space != 0;
    let fields = external.as_ref().map_or(Vec::new(), |(forces, bounds, _)| {
        use crate::game::particle_forces::{prepare, Force};
        forces.iter().map(|f| match f { Force::Field { l2w, field } => prepare(l2w, field, bounds, (!world).then_some((&pose.l2w, &pose.w2l))), _ => None }).collect()
    });
    Ctx {
        world: s.ps.space != 0,
        l2w: columns(&pose.l2w),
        w2l: columns(&pose.w2l),
        render: pose.render,
        gravity: [g.world.gravity.x, g.world.gravity.y, 0.0],
        t: state_t / s.ps.length,
        emitter_velocity: run.emitter_velocity.to_array(),
        drag: drag_scalar(&s.ps, drive),
        noise: (run.seeds.as_ref().map_or([0.0; 3], |sd| crate::game::particle_noise::offset(sd.random_seed)), run.noise_scroll),
        external,
        fields,
        pose: (pose.l2w, pose.w2l),
    }
}

/// uses3DSize: a 3D start size or a size module on separate axes (SetUses3DSize's callers).
fn uses_3d_size(ps: &ParticleSystem) -> bool {
    ps.start_size_y.is_some() || ps.size.as_ref().is_some_and(|c| c.separate) || ps.size_by_speed.as_ref().is_some_and(|c| c.0.separate)
}

/// uses3DRotation (AllocateParticleArrays 1801b4e70): a 3D start rotation, an aligned shape or a rotation module on
/// separate axes.
fn uses_3d_rotation(ps: &ParticleSystem) -> bool {
    ps.start_rotation_xy.is_some() || (ps.shape_enabled && ps.shape_align_to_direction) || ps.rotation_separate
}

/// UpdateModulesPreSimulationIncremental (1801c87d0) of one particle, without the shape update and the noise scroll:
/// InitialModule::Update's gravity and animatedVelocity reset, VelocityModule, NoiseModule, InheritVelocityModule,
/// ExternalForcesModule::Update, ForceModule, ClampVelocityModule, then the rotation modules' rotationalSpeed.
/// `live`: the fields with a particle of this one's batch of four in range (`live_bits`).
fn presim(s: &Sys, c: &Ctx, p: &mut Particle, dt: f32, live: u64) {
    use crate::game::particle_modules as m;
    let ps = &s.ps;
    let mut vel = p.velocity.to_array();
    // GetGravity (0x1801ff0f0): 0 when the modifier's scalar is 0.
    let g = if ps.gravity.scalar == 0.0 { [0.0; 3] } else { c.gravity };
    let w2l = (!c.world).then_some(&c.w2l);
    if ps.gravity.state < 2 {
        if let Some(v) = m::gravity_shared(&ps.gravity, g, c.t, dt, w2l) {
            vel = [0, 1, 2].map(|i| v[i] + vel[i]);
        }
    } else {
        m::gravity_random(&ps.gravity, g, c.t, dt, w2l, p.seed, &mut vel);
    }
    let mut anim = [0.0; 3];
    if let Some(v) = &ps.velocity {
        let mt = m::transformation(c.world, v.world, &c.l2w, &c.w2l, c.render);
        if ps.space == 2 && (v.orbital.iter().any(|c| c.scalar != 0.0) || v.radial.scalar != 0.0) {
            unimplemented!("mod-only: VelocityModule orbital in custom simulation space (localToWorldEmission and its inverse)");
        }
        m::velocity(v, &mt, (c.world, &c.pose.0, &c.pose.1), p.seed, p.alive, p.position.to_array(), dt, &mut anim);
    }
    if let Some(n) = &ps.noise {
        let (offset, scroll) = c.noise;
        crate::game::particle_noise::job(n, offset, scroll, p.seed, p.alive, dt, p.position.to_array(), uses_3d_rotation(ps), &mut anim, &mut p.rotation, &mut p.noise_impulse);
    }
    if let (true, Some((mode, curve))) = (c.world, &ps.inherit) {
        m::inherit(*mode, curve, c.emitter_velocity, p.seed, &mut anim);
    }
    if let (Some(x), Some((forces, bounds, w))) = (&ps.external_forces, &c.external) {
        let lane = crate::game::particle_forces::FieldLane { position: p.position.to_array(), alive: p.alive, seed: p.seed, anim, size: size_stream(ps, p), uses_3d_size: uses_3d_size(ps) };
        crate::game::particle_forces::apply(x, forces, &c.fields, bounds, (!c.world).then_some(w), dt, live, &lane, &mut vel);
    }
    if let Some(f) = &ps.force {
        let mt = m::transformation(c.world, f.world, &c.l2w, &c.w2l, c.render);
        m::force(f, &mt, p.seed, p.alive, dt, &mut vel);
    }
    if let Some(cl) = &ps.clamp {
        let size = size_stream(ps, p);
        m::clamp(cl, c.drag, dt, p.seed, p.alive, size, uses_3d_size(ps), &mut vel, anim);
    }
    p.velocity = Vec3::from_array(vel);
    p.animated = Vec3::from_array(anim);
    // InitialModule::Update zeroes rotationalSpeed under either rotation module; RotationModule and
    // RotationBySpeedModule add theirs (the by-speed one after the limit module).
    if ps.rotation.is_some() || ps.rotation_by_speed.is_some() {
        p.rotational_speed = crate::particle_render::angular_velocity(ps, p.seed, p.alive, (p.velocity + p.animated).to_array());
    }
}

/// UpdateSizeModules (1801c9970): currentSize from SizeModule, SizeBySpeedModule and NoiseModule::UpdateSize.
fn size_update(s: &Sys, p: &mut Particle) {
    let [x, y, z] = size_of(s, p);
    p.current_size = Vec2::new(x, y);
    p.current_z = z;
}

/// KillParticle's death sub-emitters (1801a8560): RecordEmit of each death entry with the times 0, 0, 1, 1 and length 0.
fn death(run: &mut Run, i: usize, systems: &[Sys], g: &Game, acc: f32, rng: &mut Rng) {
    let s = &systems[run.sys];
    let deaths: Vec<_> = s.subs.iter().filter(|sub| sub.1 == 2).copied().collect();
    if deaths.is_empty() {
        return;
    }
    let mut children = std::mem::take(&mut run.children);
    for sub in deaths {
        let mut es = sub_emission(run, i, 0.0);
        let c = child_index(s, sub.0);
        record(run, &mut children[c], systems, g, sub, 2, i, &mut es, [0.0, 0.0, 1.0, 1.0], 0.0, acc, rng);
    }
    run.children = children;
}

/// KillParticle (1801a8560): the death, then element_assign(index, last) and count - 1.
fn kill(run: &mut Run, i: usize, systems: &[Sys], g: &Game, acc: f32, rng: &mut Rng) {
    death(run, i, systems, g, acc, rng);
    let last = run.particles.last().cloned();
    run.particles.swap_remove(i);
    run.tail.insert(0, last);
}

/// The lanes past `count` a 4-wide module loop from `from` runs over (`while i < count`, four at a time).
fn pad_lanes(count: usize, from: usize) -> usize {
    count.saturating_sub(from).wrapping_neg() & 3
}

/// AddNewParticles (1801b4bc0): the slots from the count to round4(count), where the new ones start, and the `m` slots
/// from there taken out of the tail for them.
fn open(run: &mut Run, m: usize) -> Vec<Option<Particle>> {
    let count = run.particles.len();
    let gap = ((count + 3) & !3) - count;
    let mut out: Vec<Option<Particle>> = run.tail.drain(..gap.min(run.tail.len())).collect();
    out.resize_with(gap, || None);
    run.tail.drain(..m.min(run.tail.len()));
    out
}

/// CopyParticlesToUnalignedDst (1801b7f30) after `open`: `region` the slots from round4(count) as the start modules
/// left them, `n` of them counted. The gap's slots take the last of the counted ones (element_assign leaves their
/// slots as they were), then the count is count + n; every other slot keeps its memory.
fn place(run: &mut Run, gap: Vec<Option<Particle>>, region: Vec<Option<Particle>>, n: usize) {
    let g = gap.len().min(n);
    let src = gap.len() - g + n;
    let mut mem = gap;
    mem.extend(region);
    mem.append(&mut run.tail);
    for k in 0..g {
        mem[k] = mem[src + k].clone();
    }
    run.tail = mem.split_off(n);
    run.particles.extend(mem.into_iter().map(|p| p.expect("a counted slot holds a written particle")));
}

/// SimulateParticles (1801c2230) over the particles from `from` on: aging, the trail keep path, integration, the kill
/// and the rotation. Each particle's position before the step goes to `from` for the collision module.
fn simulate(run: &mut Run, systems: &[Sys], g: &Game, from: usize, dt: f32, acc: f32, rng: &mut Rng) {
    let ps = &systems[run.sys].ps;
    // Trails that outlive their particles: a dead particle stands still until its trail is empty.
    let keep = ps.trail.as_ref().is_some_and(|t| t.mode == 0 && !t.die_with_particles);
    let pad = pad_lanes(run.particles.len(), from);
    for p in run.particles[from..].iter_mut().chain(run.tail.iter_mut().take(pad).flatten()) {
        p.alive = crate::game::particle_sim::age(p.alive, p.inv_lifetime, dt, ps.ring_buffer_mode);
        p.age += dt;
        if keep && p.alive > 100.0 {
            p.velocity = Vec3::ZERO;
            p.animated = Vec3::ZERO;
        }
        p.from = p.position;
        // usesSpeedModifier's factor belongs to the velocity module, which is not rewritten (speedModifier 1).
        p.position += (p.velocity + p.animated) * dt;
    }
    // PauseUntilReplaced kills nothing (LoopUntilReplaced stopped in `age`).
    if ps.ring_buffer_mode == 0 {
        let dead = run.particles[from..].iter().map(|p| p.alive > 100.0 && (!keep || p.trail.is_empty())).collect();
        for i in kill_order(dead) {
            kill(run, from + i, systems, g, acc, rng);
        }
    }
    let pad = pad_lanes(run.particles.len(), from);
    for p in run.particles[from..].iter_mut().chain(run.tail.iter_mut().take(pad).flatten()) {
        p.rotation += p.rotational_speed * dt;
    }
}

/// SimulateParticles' kill (1801c2230) as the indices KillParticle takes in turn, each a swap with the last: batches of
/// four from the start, a lane past the count or not `dead` is left, the rest die from lane 3 down and the same batch
/// is looked at again. KillParticles<0,0> (1801ae440, the trail keep path) leaves the lanes whose trail is not empty
/// the same way, so they count as not dead.
fn kill_order(mut dead: Vec<bool>) -> Vec<usize> {
    let (end, mut i, mut order) = (dead.len(), 0, Vec::new());
    while i < end && i < dead.len() {
        let n = dead.len();
        let lanes: [bool; 4] = std::array::from_fn(|l| i + l < n && dead[i + l]);
        if !lanes.contains(&true) {
            i += 4;
            continue;
        }
        for l in (0..4).rev().filter(|l| lanes[*l]) {
            dead.swap_remove(i + l);
            order.push(i + l);
        }
    }
    order
}

/// CollisionModule::Update over particles `from..to` with the four lanes' dt (particle_collision).
fn collide(run: &mut Run, c: &crate::assets::particles::Collision, systems: &[Sys], g: &Game, from: usize, to: usize, dt: [f32; 4]) {
    let s = &systems[run.sys];
    let ps = &s.ps;
    if ps.subs.iter().any(|x| x.kind == 1) {
        unimplemented!("mod-only: CollisionModule: collision sub-emitters");
    }
    if ps.space == 2 {
        unimplemented!("mod-only: CollisionModule: custom simulation space");
    }
    let pose = (ps.space == 0).then(|| pose(run, s));
    let current = ps.size.is_some() || ps.size_by_speed.is_some() || ps.noise.as_ref().is_some_and(crate::game::particle_noise::uses_impulse);
    let local = pose.as_ref().map(|q| (&q.l2w, &q.w2l));
    let st = run.collision.as_mut().expect("collision state from start");
    crate::game::particle_collision::update(st, c, &g.world, &mut run.particles, &run.tail, from, to, dt, local, current, uses_3d_size(ps));
}

/// A run and its sub-emitter children for AssignRayBudgets; `bleed` is a bleeding particle's run (its collision is on
/// under FancyEffects, as `collision_of` with its drive).
fn budget_runs<'a>(run: &'a mut Run, systems: &'a [Sys], g: &Game, bleed: bool, out: &mut Vec<crate::game::particle_collision::Budgeted<'a>>) {
    let ps = &systems[run.sys].ps;
    let collision = ps.collision.as_ref().filter(|c| g.prefs.FancyEffects && (c.enabled || bleed));
    let count = run.particles.len();
    if let Some(state) = run.collision.as_mut() {
        out.push(crate::game::particle_collision::Budgeted { id: run.id, state, collision, active: true, count });
    }
    for c in &mut run.children {
        budget_runs(c, systems, g, false, out);
    }
}

/// A run started after BeginUpdateAll's AssignRayBudgets: native had it in activeEmitters then (Play adds it), with no
/// particles yet; the pass over it alone gives what the whole pass would (its count is 0, and no listed split is
/// rewritten past one system over the budget).
fn budget_new(run: &mut Run, systems: &[Sys], g: &Game, bleed: bool) {
    let mut list = Vec::new();
    budget_runs(run, systems, g, bleed, &mut list);
    crate::game::particle_collision::assign_ray_budgets(&mut list);
}

/// The size stream the modules read: currentSize when the system uses one (SetUsesCurrentSize 1801c1580: the size
/// and size-by-speed modules and a positive noise sizeAmount), else startSize.
fn size_stream(ps: &ParticleSystem, p: &Particle) -> [f32; 3] {
    let current = ps.size.is_some() || ps.size_by_speed.is_some() || ps.noise.as_ref().is_some_and(crate::game::particle_noise::uses_impulse);
    if current { [p.current_size.x, p.current_size.y, p.current_z] } else { [p.size, p.size_y, p.size_z] }
}

/// TrailModule::Update (1802037b0) of one particle in per-particle mode (a ribbon keeps no points), after the
/// simulation and the collision; `time` the module's clock after this pass's dt. `to_world` the pose's localToWorld
/// for a local system.
fn trail(s: &Sys, p: &mut Particle, time: f32, to_world: Option<&[f32; 16]>) {
    let Some(c) = s.ps.trail.as_ref().filter(|c| c.mode == 0) else { return };
    let size = size_stream(&s.ps, p);
    crate::particle_trails::trail_update(c, time, &mut p.trail, p.seed, p.alive, p.inv_lifetime, size, uses_3d_size(&s.ps), p.position.to_array(), to_world);
}

/// UpdateModulesIncremental (1801c7d00) over every particle for one pass of `dt` at the system time `t`.
#[allow(clippy::too_many_arguments)]
fn modules(run: &mut Run, systems: &[Sys], drive: Option<&Drive>, g: &Game, from: usize, t: f32, dt: f32, acc: f32, rng: &mut Rng) {
    let s = &systems[run.sys];
    let ps = &s.ps;
    // ShapeModule::Update (0x180dffe80), first in UpdateModulesPreSimulationIncremental of a pass.
    if ps.shape_enabled {
        if let Some(sh) = run.shape.as_mut() {
            sh.update(ps, t / ps.length, dt);
        }
    }
    // NoiseModule::Update's scroll, on the passes that also update the shape (not StartModules').
    if let Some(n) = &ps.noise {
        run.noise_scroll = crate::game::particle_noise::scroll(n, t / ps.length, dt, run.noise_scroll);
    }
    let c = ctx(run, s, drive, g, t);
    // ApplyForceField<0> skips a batch of four with none in range; the last batch's lanes past the count are the
    // tail's slots (an uninitialised one out of range), moved like the rest.
    let pad = pad_lanes(run.particles.len(), from);
    let bits = |b: u64, p: &Particle| b | crate::game::particle_forces::live_bits(&c.fields, p.seed, p.position.to_array());
    let mut live = 0;
    for batch in run.particles[from..].chunks_mut(4) {
        live = batch.iter().fold(0, bits);
        if batch.len() < 4 {
            live = run.tail.iter().take(pad).flatten().fold(live, bits);
        }
        for p in batch {
            presim(s, &c, p, dt, live);
        }
    }
    for p in run.tail.iter_mut().take(pad).flatten() {
        presim(s, &c, p, dt, live);
    }
    simulate(run, systems, g, from, dt, acc, rng);
    let collision = collision_of(ps, g, drive);
    if let Some(c) = collision {
        let to = run.particles.len();
        collide(run, c, systems, g, from, to, [dt; 4]);
    }
    let incremental = incremental_size(ps, drag_scalar(ps, drive), collision.is_some());
    // TrailModule::Update's clock takes the pass's dt once for the system.
    if ps.trail.is_some() {
        run.trail_time += dt;
    }
    let (time, l2w) = (run.trail_time, (ps.space == 0).then(|| pose(run, s).l2w));
    for p in &mut run.particles[from..] {
        trail(s, p, time, l2w.as_ref());
        if incremental {
            size_update(s, p);
        }
    }
    if incremental {
        let pad = pad_lanes(run.particles.len(), from);
        for p in run.tail.iter_mut().take(pad).flatten() {
            size_update(s, p);
        }
    }
    let all: Vec<(usize, f32)> = (from..run.particles.len()).map(|i| (i, dt)).collect();
    sub_birth(run, systems, g, &all, acc, rng);
}

/// startSize after the scripts' writes: PhysicalBehaviour's constant, WaterImpactBehaviour's multiplier.
fn start_size(run: &Run, ps: &ParticleSystem) -> Option<MinMaxCurve> {
    if let Some(v) = run.emission.and_then(|e| e.size) {
        return Some(MinMaxCurve { state: 0, scalar: v, ..Default::default() });
    }
    run.water.as_ref().and_then(|w| w.2.get(&run.sys).copied()).map(|v| MinMaxCurve { scalar: v, ..ps.start_size.clone() })
}

/// startSpeed after the scripts' startSpeedMultiplier writes (BleedingParticleBehaviour, WaterImpactBehaviour).
fn start_speed(run: &Run, ps: &ParticleSystem, drive: Option<&Drive>) -> MinMaxCurve {
    let v = drive.map(|d| d.speed).or(run.water.as_ref().map(|w| w.1)).or(run.writes.speed);
    MinMaxCurve { scalar: v.unwrap_or(ps.start_speed.scalar), ..ps.start_speed.clone() }
}

/// InitialModule::Start for `n` new particles with the scripts' startSize write and the inherited values.
fn initial(run: &mut Run, ps: &ParticleSystem, end: f32, n: usize, inherited: &crate::game::particle_sim::Inherited) -> Vec<crate::game::particle_sim::Started> {
    let sized = start_size(run, ps).map(|c| ParticleSystem { start_size: c, ..ps.clone() });
    let seeds = run.seeds.as_mut().expect("a played run has its seeds");
    crate::game::particle_sim::initial_start(sized.as_ref().unwrap_or(ps), &mut seeds.initial, inherited, &vec![end; n])
}

/// A new particle from InitialModule::Start's `started`, at the shape's position with its start velocity (both in
/// simulation space).
fn create(run: &Run, drive: Option<&Drive>, started: crate::game::particle_sim::Started, position: Vec3, velocity: Vec3) -> Particle {
    let color32 = match (drive, run.color) {
        (Some(d), _) => crate::game::particle_sim::color32(d.color),
        (None, Some(c)) => crate::game::particle_sim::color32(c),
        (None, None) => started.color,
    };
    let [sx, sy, sz] = started.size;
    Particle {
        position,
        velocity,
        lifetime: 1.0 / started.inv_lifetime,
        size: sx,
        size_y: sy,
        size_z: sz,
        current_size: Vec2::new(sx, sy),
        current_z: sz,
        rotation: started.rotation[2],
        seed: started.seed,
        parent_seed: started.parent_seed,
        color32,
        inv_lifetime: started.inv_lifetime,
        ..Default::default()
    }
}

/// The emission state a sub entry's RecordEmit gets for particle `i` (SubModule::Update 180203000, KillParticle): a
/// Rand from the parent's emission Rand x plus the particle's seed, spacing 0 and `to_emit`.
fn sub_emission(run: &Run, i: usize, to_emit: f32) -> crate::game::particle_sim::Emission {
    let x = run.emitted.as_ref().expect("a played run has its emission state").rand.state[0].wrapping_add(run.particles[i].seed);
    let rand = crate::unity::Random { state: [x, x.wrapping_mul(0x6c07_8965).wrapping_add(1), x.wrapping_mul(0x0543_41d9).wrapping_add(0x6c07_8966), x.wrapping_mul(0x6ab5_1b9d).wrapping_add(0x714a_cb3f)] };
    crate::game::particle_sim::Emission { rand, to_emit, spacing: 0.0 }
}

/// SubModule::Update (180203000) for the particles `at` (index, the step each one had): per birth entry k, each
/// particle whose time since its start delay is inside the sub system's length records its emits over the step.
fn sub_birth(run: &mut Run, systems: &[Sys], g: &Game, at: &[(usize, f32)], acc: f32, rng: &mut Rng) {
    let s = &systems[run.sys];
    let births: Vec<_> = s.subs.iter().filter(|sub| sub.1 == 0).copied().collect();
    if births.is_empty() || at.is_empty() {
        return;
    }
    let mut children = std::mem::take(&mut run.children);
    for (k, sub) in births.into_iter().enumerate() {
        let q = &systems[sub.0].ps;
        let length = if q.looping { f32::MAX } else { q.length };
        for &(i, dt) in at {
            let p = &run.particles[i];
            let r = crate::game::particle_rand::unit(crate::unity::Random::new(p.seed.wrapping_add(0x34bf_a39a) as i32).next());
            let delay = match q.start_delay.state {
                0 => q.start_delay.scalar,
                3 => (q.start_delay.scalar - q.start_delay.min_scalar) * r + q.start_delay.min_scalar,
                m => unimplemented!("mod-only: SubModule::Update: a startDelay curve (mode {m}, 1801adf00 / 1801ba6f0)"),
            };
            let inv = p.inv_lifetime;
            let d4 = delay * inv;
            let nt = p.alive * f32::from_bits(0x3c23d70a) - d4;
            let time = nt / inv;
            if !(0.0 <= nt && !(1.0 <= nt || length <= time)) {
                continue;
            }
            let prev_nt = (p.alive - (dt * 100.0) * inv) * f32::from_bits(0x3c23d70a) - d4;
            let prev_time = prev_nt / inv;
            let to_emit = if k < 2 {
                p.emit_accumulator[k]
            } else {
                let v = p.velocity + p.animated;
                let speed = ((v.y * v.y + v.z * v.z) + v.x * v.x).sqrt();
                let x = (q.rate_over_distance.scalar * speed + q.rate_over_time.scalar) * time;
                let r = x.round_ties_even();
                x - if x < r { r - 1.0 } else { r }
            };
            let mut es = sub_emission(run, i, to_emit);
            let c = child_index(s, sub.0);
            record(run, &mut children[c], systems, g, sub, 0, i, &mut es, [prev_time, time, prev_nt, nt], length, acc, rng);
            if k < 2 {
                run.particles[i].emit_accumulator[k] = es.to_emit;
            }
        }
    }
    run.children = children;
}

/// ColorModule::UpdateSingle (1801e22d0) on an inherited colour: the gradient at (startLifetime - lifetime) /
/// startLifetime, bytes (int)(clamp01(c) * 255 + 0.5) (Evaluate<0,0> 1801cfd10), each channel ((g + 1) * c) >> 8.
/// Two gradients (EvaluateRandomGradient<0,0> 1801d1470) lerp their bytes by (int)(255 * Random01(randomSeed +
/// 0x591bc05c)); RecordEmit never writes that randomSeed (its temp particle's +0x64, a stack slot), so `garbage`
/// stands for whatever the slot held.
fn color_single(g: &crate::assets::particles::MinMaxGradient, start_lifetime: f32, lifetime: f32, color: u32, garbage: u32) -> u32 {
    let t = if start_lifetime == 0.0 { 0.0 } else { (start_lifetime - lifetime) / start_lifetime };
    let t = if t <= 0.0 { 0.0 } else { t };
    let bytes = |gr: &crate::assets::particles::Gradient| {
        let c = crate::particle_render::evaluate_hdr(gr, t);
        (0..4).fold(0u32, |out, k| out | ((((c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as i32 as u32) & 0xff) << (8 * k)))
    };
    let g32 = match g.state {
        1 if g.max.mode == 0 => bytes(&g.max),
        3 if g.max.mode == 0 && g.min.mode == 0 => {
            let f = (255.0 * crate::particle_render::random01(garbage.wrapping_add(0x591bc05c))) as i32 as u32;
            let m = f | f << 16;
            crate::particle_render::lerp_bytes(bytes(&g.min), bytes(&g.max), m | m << 8)
        }
        s => unimplemented!("mod-only: ColorModule::UpdateSingle of gradient state {s} mode {}/{}: no game system inherits it", g.min.mode, g.max.mode),
    };
    let mut out = 0;
    for k in 0..4 {
        let b = (g32 >> (8 * k)) & 0xff;
        out |= ((((b + 1) * ((color >> (8 * k)) & 0xff)) >> 8) & 0xff) << (8 * k);
    }
    out
}

/// SizeModule::UpdateSingle (1801f6fd0) with all three axes: each one times max(0, the curve) at (startLifetime -
/// lifetime) / startLifetime.
fn size_single(m: &crate::assets::particles::SizeOverLifetime, start_lifetime: f32, lifetime: f32, size: &mut [f32; 3]) {
    for (k, v) in size.iter_mut().enumerate() {
        let c = if m.separate { [&m.x, &m.y, &m.z][k] } else { &m.x };
        let t = if start_lifetime == 0.0 { 0.0 } else { (start_lifetime - lifetime) / start_lifetime };
        let t = if t <= 0.0 { 0.0 } else { t };
        let f = match c.state {
            0 => c.scalar,
            1 => crate::particle_render::curve_at(c, t, || unreachable!("a one-curve size reads no random")),
            s => unimplemented!("mod-only: SizeModule::UpdateSingle of state {s} (its randomSeed is never set by RecordEmit)"),
        };
        *v *= if f <= 0.0 { 0.0 } else { f };
    }
}

/// RecordEmit (1801aa3b0) of parent particle `i` for sub entry `sub` (kind 0 birth, 2 death)
/// into `child`: the probability gate, the world position and velocity, the counts, the inherited values and the two
/// Emits. `times` = prevTime, time, prevNT, NT.
#[allow(clippy::too_many_arguments)]
fn record(run: &Run, child: &mut Run, systems: &[Sys], g: &Game, sub: (usize, i32, i32, f32), kind: i32, i: usize, es: &mut crate::game::particle_sim::Emission, times: [f32; 4], length: f32, acc: f32, rng: &mut Rng) {
    let s = &systems[run.sys];
    let p = &run.particles[i];
    if !systems[sub.0].awake || sub.3 == 0.0 || crate::particle_render::random01(p.seed.wrapping_add(0x5aa4_7f98)) > sub.3 {
        return;
    }
    // CalculatePositionAndVelocity (1801a5ba0).
    let v = p.velocity + p.animated;
    let (position, velocity) = if s.ps.space == 0 {
        let m = pose(run, s).l2w;
        let x = p.position;
        (
            [((x.x * m[0] + x.y * m[4]) + x.z * m[8]) + m[12], ((x.x * m[1] + x.y * m[5]) + x.z * m[9]) + m[13], ((x.x * m[2] + x.y * m[6]) + x.z * m[10]) + m[14]],
            [(v.x * m[0] + v.y * m[4]) + v.z * m[8], (v.x * m[1] + v.y * m[5]) + v.z * m[9], (v.x * m[2] + v.y * m[6]) + v.z * m[10]],
        )
    } else {
        (p.position.to_array(), v.to_array())
    };
    let q = &systems[sub.0].ps;
    let [prev_time, time, prev_nt, nt] = times;
    let (first, n_d, n, cont);
    if kind == 0 {
        n_d = es.over_distance(&q.rate_over_distance, velocity, time, time - prev_time, length);
        first = (es.to_emit, es.spacing);
        (cont, n) = es.over_time(&q.rate_over_time, &q.bursts, prev_time, time, length);
    } else {
        let Some(b) = q.bursts.first() else { return };
        n_d = 0;
        first = (es.to_emit, es.spacing);
        n = es.burst(b, nt) as i64 as u64;
        cont = 0;
    }
    if n == 0 && cont == 0 && n_d == 0 {
        return;
    }
    let mut inherited = crate::game::particle_sim::Inherited::default();
    if sub.2 != 0 {
        // CalculateInheritedProperties (1801a5a00) on the particle RecordEmit copies out.
        let ps = &s.ps;
        let start_lifetime = 1.0 / p.inv_lifetime;
        let lifetime = (1.0 - p.alive * f32::from_bits(0x3c23d70a)) * start_lifetime;
        if sub.2 & 1 != 0 {
            inherited.color = p.color32;
            if let Some(c) = &ps.color {
                // The randomSeed UpdateSingle reads (+0x64, `mov eax, [rdx+0x64]` at 1801e22ff) is never written by
                // RecordEmit (1801aa3b0): its temp particle is the stack slot [rbp+0x48], filled field by field
                // (1801aa4b2..1801aa86c) with +0x64 skipped, and 1801a5ba0 only reads it. Measured in the real game
                // (INT3 at 1801a5a45, a shot human's BleedingParticle Trail, 680 calls): the slot holds what earlier
                // calls left there, 0 in 18%, 5 or 0xa7 in 3%, a float in [0.5, 1) in 78%, 509 distinct values,
                // repeated only within one RecordEmit call. No value is reproducible, and random01(seed + 0x591bc05c)
                // makes any of them a uniform pick between the two gradients, which this look-only draw matches.
                let garbage = (rng.value() * 4294967295.0) as u32;
                inherited.color = color_single(c, start_lifetime, lifetime, inherited.color, garbage);
            }
            if ps.color_by_speed.is_some() {
                unimplemented!("mod-only: ColorBySpeedModule::UpdateSingle (18020afe0) for an inherited colour");
            }
        }
        if sub.2 & 2 != 0 {
            let three = uses_3d_size(ps);
            inherited.size = if three { [p.size, p.size_y, p.size_z] } else { [p.size; 3] };
            if let Some(m) = &ps.size {
                size_single(m, start_lifetime, lifetime, &mut inherited.size);
            }
            if ps.size_by_speed.is_some() {
                unimplemented!("mod-only: SizeBySpeedModule::UpdateSingle (1801e2480) for an inherited size");
            }
            if !three {
                inherited.size = [inherited.size[0]; 3];
            }
        }
        if sub.2 & 4 != 0 {
            if ps.start_rotation_xy.is_some() {
                unimplemented!("mod-only: an inherited 3D rotation and axisOfRotation: the particles keep z only");
            }
            inherited.rotation = [0.0, 0.0, p.rotation];
        }
        if sub.2 & 8 != 0 {
            inherited.lifetime_multiplier = lifetime;
        }
        if sub.2 & 16 != 0 {
            inherited.normalized_t = Some(nt);
        }
    }
    // RecordEmit: inheritValues.randomSeed is the parent particle's, whatever the properties.
    inherited.random_seed = p.seed;
    let cmd = Cmd { time_alive: acc, position, velocity, prev_t: prev_nt, t: nt, dt: time - prev_time, inherited };
    emit(child, systems, g, &cmd, first, n_d, n_d, rng);
    emit(child, systems, g, &cmd, (es.to_emit, es.spacing), n, cont, rng);
}

/// SubEmitterEmitCommand without its counts and emission state.
struct Cmd {
    time_alive: f32,
    position: [f32; 3],
    velocity: [f32; 3],
    prev_t: f32,
    t: f32,
    dt: f32,
    inherited: crate::game::particle_sim::Inherited,
}

/// ParticleSystem::Emit(SubEmitterEmitCommand) (1801b8d70): `n` particles (`continuous` spread by the state's toEmit
/// and spacing) on the frame LookRotation(velocity, up) * localRotation at the command's position, then the catch-up
/// passes of fixedDeltaTime over the new ones while timeAlive lasts.
#[allow(clippy::too_many_arguments)]
fn emit(child: &mut Run, systems: &[Sys], g: &Game, cmd: &Cmd, emission: (f32, f32), n: u64, continuous: u64, rng: &mut Rng) {
    use crate::game::particle_modules as m;
    if n == 0 {
        return;
    }
    let s = &systems[child.sys];
    let ps = &s.ps;
    if !(cmd.time_alive < ps.start_lifetime.scalar) {
        return;
    }
    let pose = pose(child, s);
    let l = &pose.l2w;
    let at = |a: [f32; 3]| [(l[0] * a[0] + l[4] * a[1]) + l[8] * a[2], (l[1] * a[0] + l[5] * a[1]) + l[9] * a[2], (l[2] * a[0] + l[6] * a[1]) + l[10] * a[2]];
    let (z, y) = (at([0.0, 0.0, 1.0]), at([0.0, 1.0, 0.0]));
    let v = cmd.velocity;
    let len = ((v[1] * v[1] + v[0] * v[0]) + v[2] * v[2]).sqrt();
    let dir = if f32::from_bits(0x3727c5ac) < len { v.map(|c| c / len) } else { [0.0; 3] };
    let k = ((dir[1] * z[1] + dir[0] * z[0]) + dir[2] * z[2]).abs();
    let up = [0, 1, 2].map(|c| y[c] * k + z[c] * (1.0 - k));
    let local = m::columns(&crate::unity::transform::quaternion_to_matrix(crate::unity::transform::normalize(s.local_rotation.to_array())));
    let rot = m::mul3(&m::look_rotation(dir, up).unwrap_or(m::IDENTITY), &local);
    let (mut frame, velocity) = if ps.space != 0 {
        (crate::game::particle_shape::Frame { m: rot, t: cmd.position }, v)
    } else {
        // MultiplyMatrices3x4(worldToLocal, M) (18073fb20) and the velocity into local space.
        let w = &pose.w2l;
        let cols = [rot[0], rot[1], rot[2], cmd.position];
        let out = [0, 1, 2, 3].map(|c| [0, 1, 2].map(|r| ((w[r] * cols[c][0] + w[r + 4] * cols[c][1]) + w[r + 8] * cols[c][2]) + if c == 3 { w[r + 12] } else { 0.0 }));
        let vel = [0, 1, 2].map(|r| (w[r] * v[0] + w[r + 4] * v[1]) + w[r + 8] * v[2]);
        (crate::game::particle_shape::Frame { m: [out[0], out[1], out[2]], t: out[3] }, vel)
    };
    // Matrix4x4f::Scale(renderScale) (1807413f0).
    for c in 0..3 {
        frame.m[c] = frame.m[c].map(|x| x * pose.render[c]);
    }
    if ps.unscaled_time {
        unimplemented!("mod-only: ParticleSystem::Emit's catch-up step of an unscaled-time sub system (fixedUnscaledDeltaTime)");
    }
    // The caps apply unless the update's flags have bit 2 (ud+0x1c): a frame's update passes kUpdateFlagNone.
    let mut step = g.time.fixedDeltaTime;
    let alive = cmd.time_alive;
    if alive <= 10.0 {
        if 5.0 < alive && step <= 0.2 {
            step = if 0.2 <= ps.length { 0.2 } else { ps.length };
        }
    } else if step <= 1.0 {
        step = if 1.0 <= ps.length { 1.0 } else { ps.length };
    }
    let batch = Batch { frame, velocity, world: true, emission, begin: cmd.prev_t, end: cmd.t, inherited: cmd.inherited };
    let first = start_particles(child, systems, None, g, child.clock.t(), &batch, cmd.dt, continuous, n, 0.0, 0.0, alive, rng);
    if f32::from_bits(0x38d1b717) < step {
        let mut left = alive;
        while step <= left && first < child.particles.len() {
            left -= step;
            let t = child.clock.t();
            modules(child, systems, None, g, first, t, step, alive, rng);
        }
        if !incremental_size(ps, drag_scalar(ps, None), collision_of(ps, g, None).is_some()) {
            let pad = pad_lanes(child.particles.len(), first);
            for p in child.particles[first..].iter_mut().chain(child.tail.iter_mut().take(pad).flatten()) {
                size_update(s, p);
            }
        }
    }
    ring_copy(child, systems, g, first, alive, rng);
    // UpdateBounds unless kPlaying (nothing is culled in the rewrite).
    if child.halted {
        update_bounds(child, &systems[child.sys], g);
    }
}

/// What StartModules takes from its caller: the emission matrix, the velocity (emitterVelocity, or the sub-emitter
/// command's), whether the velocity counts (world space, or a sub-emitter Emit), the emission state's toEmit and
/// spacing, the normalized times the particles spread between and InitialModule's inherited values.
struct Batch {
    frame: crate::game::particle_shape::Frame,
    velocity: [f32; 3],
    world: bool,
    emission: (f32, f32),
    begin: f32,
    end: f32,
    inherited: crate::game::particle_sim::Inherited,
}

/// StartParticles' batch: localToWorld in world space (identity in local), the run's emitterVelocity and emission
/// state, (t - dt) / length .. t / length.
fn own_batch(run: &Run, s: &Sys, t: f32, dt: f32) -> Batch {
    use crate::game::particle_modules as m;
    let pose = pose(run, s);
    let world = s.ps.space != 0;
    let frame = if world { crate::game::particle_shape::Frame { m: m::columns(&pose.l2w), t: [pose.l2w[12], pose.l2w[13], pose.l2w[14]] } } else { crate::game::particle_shape::Frame { m: m::IDENTITY, t: [0.0; 3] } };
    let inv_length = crate::game::particle_sim::inverse(s.ps.length);
    let state = run.emitted.as_ref().expect("a played run has its emission state");
    Batch { frame, velocity: run.emitter_velocity.to_array(), world, emission: (state.to_emit, state.spacing), begin: (t - dt) * inv_length, end: t * inv_length, inherited: Default::default() }
}

/// StartParticles (1801c3cc0) and StartModules (1801c35f0): `total` new particles at the system time `state_t` for a
/// step of `dt` on the batch's frame, the first `rate` of them spread over the step by its toEmit and spacing; `left`
/// passes to come; `before` the accumulatedDt the distance path starts its particles behind; `acc` the
/// accumulatedDt sub-emitter commands carry. Returns the index of the first new particle.
#[allow(clippy::too_many_arguments)]
fn start_particles(run: &mut Run, systems: &[Sys], drive: Option<&Drive>, g: &Game, state_t: f32, b: &Batch, dt: f32, rate: u64, total: u64, left: f32, before: f32, acc: f32, rng: &mut Rng) -> usize {
    use crate::game::particle_modules as m;
    use crate::game::particle_shape as shape;
    use crate::game::particle_sim::{maxps, minps, start_alive};
    let s = &systems[run.sys];
    let ps = &s.ps;
    let max = ps.max_particles.max(0) as usize;
    let count = run.particles.len();
    // AddNewParticles (1801b4bc0): clamped to maxParticles only without a ring buffer.
    let n = if ps.ring_buffer_mode == 0 { (total as usize).min(max.saturating_sub(count)) } else { total as usize };
    if n == 0 {
        return count;
    }
    let (begin, end) = (b.begin, b.end);
    let (to_emit, spacing) = b.emission;
    // The new ones go from round4(count); the start modules write all four lanes of each batch, so the `m - n` lanes
    // past them are particles too (their own draws, frac 0), kept past the count.
    let m = (n + 3) & !3;
    let started = initial(run, ps, end, m, &b.inherited);
    let pose = pose(run, s);
    let frame = &b.frame;
    let water = run.water.clone();
    let (radius, arc) = drive.map_or((water.as_ref().map_or(ps.shape_radius.value, |w| w.0), ps.shape_arc.value), |d| (d.radius, d.arc));
    // A SpriteRenderer shape without a cached mesh (no renderer or sprite): ShapeModule::Start writes nothing.
    let no_mesh = ps.shape_type == 20 && run.shape.as_ref().is_none_or(|s| s.mesh.is_none());
    let emitted: Vec<shape::Emitted> = if !ps.shape_enabled || no_mesh {
        // InitialModule::Start: the matrix's translation and z axis.
        vec![(frame.t, m::initial_direction(&frame.m)); m]
    } else {
        // A sub system's clock is the only one that is stopEmitting here (step takes the others' out while it runs).
        if ps.shape_type == 20 && run.clock.stop_emitting {
            unimplemented!("mod-only: a sprite-shaped sub-emitter (EmitterStoreMeshData on the Emit frame)");
        }
        shape::start(ps, run.shape.as_mut().expect("a played run has its shape state"), frame, pose.emitter, radius, arc, (to_emit, spacing), m)
    };
    let tint = run.shape.as_mut().map(|s| std::mem::take(&mut s.tint)).unwrap_or_default();
    let ev = b.velocity;
    let masked = if b.world { ev } else { [0.0; 3] };
    let speed = start_speed(run, ps, drive);
    let inherit = ps.inherit.as_ref().map(|(mode, c)| (*mode, c));
    let c = ctx(run, s, drive, g, state_t);
    let collision = collision_of(ps, g, drive);
    let incremental = incremental_size(ps, drag_scalar(ps, drive), collision.is_some());
    let rewind = dt / ps.simulation_speed;
    // StartModules' presim runs on batches of four new particles from the first ([i, min(i + 4, end))), after
    // InitialModule::Start's seeds and ShapeModule::Start's positions for all of them.
    let live: Vec<u64> = started
        .chunks(4)
        .zip(emitted.chunks(4))
        .map(|(st, at)| st.iter().zip(at).fold(0, |b, (st, (pos, _))| b | crate::game::particle_forces::live_bits(&c.fields, st.seed, *pos)))
        .collect();
    let gap = open(run, m);
    let aligned = count + gap.len();
    run.particles.resize_with(aligned, Particle::default);
    let mut lanes = Vec::with_capacity(4);
    for (i, (st, (pos, dir))) in started.into_iter().zip(emitted).enumerate() {
        let frac = if (i as u64) < rate { (i as f32 + to_emit) * spacing } else { 0.0 };
        let frac = minps(maxps(frac, f32::from_bits(0x358637bd)), 1.0);
        let at = minps(1.0, maxps(0.0, (begin - end) * frac + end));
        let d = frac * dt - before;
        let velocity = m::start_velocity(&speed, inherit, b.world, masked, st.seed, at, dir);
        let mut p = create(run, drive, st, Vec3::from_array(pos), Vec3::from_array(velocity));
        if let Some(&t) = tint.get(i) {
            p.color32 = shape::tint(p.color32, t);
        }
        if let Some((curve, range)) = &ps.lifetime_by_speed {
            p.inv_lifetime = m::lifetime_by_speed(curve, *range, ev, p.seed, p.inv_lifetime);
            p.lifetime = 1.0 / p.inv_lifetime;
        }
        presim(s, &c, &mut p, d, live[i / 4]);
        p.alive = start_alive(p.inv_lifetime, d);
        p.age = d;
        // pos = (pos - emitterVelocity * ((frac + left) * dt / simulationSpeed)) + d * (animated + velocity).
        let f = (frac + left) * rewind;
        p.from = Vec3::new(p.position.x - masked[0] * f, p.position.y - masked[1] * f, p.position.z - masked[2] * f);
        p.position = p.from + (p.animated + p.velocity) * d;
        p.rotation += p.rotational_speed * d;
        if incremental {
            size_update(s, &mut p);
        }
        run.particles.push(p);
        if i >= n {
            continue;
        }
        let k = run.particles.len() - 1;
        // CollisionModule::Update and SubModule::Update close each batch of four, up to the new ones' end.
        lanes.push((k, d));
        if lanes.len() == 4 || i + 1 == n {
            if let Some(c) = collision {
                let mut dts = [0.0; 4];
                for (x, &(_, d)) in dts.iter_mut().zip(&lanes) {
                    *x = d;
                }
                collide(run, c, systems, g, lanes[0].0, k + 1, dts);
            }
            sub_birth(run, systems, g, &lanes, acc, rng);
            lanes.clear();
        }
    }
    // One at a time over the whole padded batch: the dead ones (a lifetime under the step) go at once, each taking
    // the last lane and the new count one less.
    let (mut i, mut n) = (aligned, n);
    while i < run.particles.len() {
        if run.particles[i].alive <= 100.0 {
            i += 1;
        } else {
            kill(run, i, systems, g, acc, rng);
            n = n.saturating_sub(1);
        }
    }
    let region = run.particles.drain(aligned..).map(Some).collect();
    run.particles.truncate(count);
    place(run, gap, region, n);
    count
}

/// CopyParticlesToUnalignedDst (1801b7f30) past maxParticles, the new ones from `first` on: PauseUntilReplaced writes
/// the overflow over the oldest from ringBufferIndex on, each replaced particle dying; LoopUntilReplaced swaps each
/// new one with the slot at ringBufferIndex through a scratch slot at the count, keeping them all.
fn ring_copy(run: &mut Run, systems: &[Sys], g: &Game, first: usize, acc: f32, rng: &mut Rng) {
    let ps = &systems[run.sys].ps;
    let max = ps.max_particles.max(0) as usize;
    if ps.ring_buffer_mode == 1 && run.particles.len() > max {
        if max == 0 {
            for i in 0..run.particles.len() {
                death(run, i, systems, g, acc, rng);
            }
            let gone = std::mem::take(&mut run.particles);
            run.tail.splice(0..0, gone.into_iter().map(Some));
            run.ring = 0;
        } else {
            for k in max..run.particles.len() {
                let r = run.ring;
                death(run, r, systems, g, acc, rng);
                run.particles[r] = run.particles[k].clone();
                run.ring = (r + 1) % max;
            }
            let over = run.particles.split_off(max);
            run.tail.splice(0..0, over.into_iter().map(Some));
        }
    } else if ps.ring_buffer_mode == 2 && run.particles.len() > max {
        if max == 0 {
            run.ring = 0;
            return;
        }
        let mut scratch = None;
        for i in first..run.particles.len() {
            let r = run.ring;
            let new = run.particles[i].clone();
            run.particles[i] = run.particles[r].clone();
            run.particles[r] = new.clone();
            scratch = Some(new);
            run.ring = (r + 1) % max;
        }
        match run.tail.first_mut() {
            Some(slot) => *slot = scratch,
            None => run.tail.push(scratch),
        }
    }
}

/// One frame of a played system: Update1b (1801c5800) and Update1Incremental (1801c5210) on the native clock.
fn step(run: &mut Run, systems: &[Sys], drive: Option<&Drive>, g: &Game, frame: f32, rng: &mut Rng) {
    // stopTime: a Stop (1801c4830) the Update scripts made since the last update took this frame's m_CurFrameTime (a
    // scripted PsCall::Stop sets it at its call); Update1Incremental's end of a non-looping run takes it too.
    if run.stopped && !run.was_stopped {
        run.stop_time = g.time.timeAsDouble;
    }
    run.was_stopped = run.stopped;
    if let Some(d) = drive {
        run.drive = Some(*d);
    }
    if run.culled {
        return;
    }
    let ended = run.clock.stop_emitting;
    step_inner(run, systems, drive, g, frame, rng);
    if !ended && run.clock.stop_emitting {
        run.stop_time = g.time.timeAsDouble;
    }
}

fn step_inner(run: &mut Run, systems: &[Sys], drive: Option<&Drive>, g: &Game, frame: f32, rng: &mut Rng) {
    let s = &systems[run.sys];
    let ps = &s.ps;
    // emitterVelocity from the move since the last frame (Update2 keeps oldPosition; the first update starts it
    // here). useRigidbodyForVelocity's formula is the drive's body velocity, without the offset turn.
    // Update1a (CollectSubEmittersRecursive): the children's transforms before the parent's job.
    place_children(run, systems);
    if let Some(x) = &ps.external_forces {
        run.forces = g.external_forces_cache(x, run.root, s.go);
    }
    let position = run.position;
    let old = run.old_position.replace(position).unwrap_or(position);
    if frame > f32::from_bits(0x38d1b717) {
        run.emitter_velocity = drive.map_or((position - old) / frame, |d| d.velocity);
    }
    let spec = crate::game::particle_clock::Spec::of(ps, 0.0);
    let mut clock = std::mem::take(&mut run.clock);
    let began = if run.fixed { clock.begin_step(&spec, frame, |_| g.time.fixedDeltaTime) } else { clock.begin(&spec, frame, s.max_step) };
    if let Some((dt, step)) = began {
        let on = !(drive.is_some_and(|d| !d.enabled) || run.emission.is_some_and(|e| !e.enabled) || run.stopped || !ps.emission_enabled);
        // The scripts' writes: Bleeding's rateOverTimeMultiplier (the scalar, its mode kept); PhysicalBehaviour's
        // rateOverTime and rateOverDistance (constants).
        let constant = |v: f32| MinMaxCurve { state: 0, scalar: v, ..Default::default() };
        let rate = match (drive, run.emission.and_then(|e| e.rate)) {
            (Some(d), _) => MinMaxCurve { scalar: d.rate, ..ps.rate_over_time.clone() },
            (None, Some(v)) => constant(v),
            _ => match run.writes.rate {
                Some(r) => MinMaxCurve { scalar: r, ..ps.rate_over_time.clone() },
                None => ps.rate_over_time.clone(),
            },
        };
        let distance = match (run.emission.and_then(|e| e.distance), run.writes.distance) {
            (Some(v), _) => constant(v),
            (None, Some(r)) => MinMaxCurve { scalar: r, ..ps.rate_over_distance.clone() },
            (None, None) => ps.rate_over_distance.clone(),
        };
        let length = ps.length;
        if !clock.stop_emitting && on && distance.scalar > 0.0 {
            let t = clock.t() + clock.accumulated();
            let to = if ps.looping { t % length } else if t < length { t } else { length };
            let velocity = run.emitter_velocity.to_array();
            let n = run.emitted.as_mut().map_or(0, |e| e.over_distance(&distance, velocity, to, dt, length));
            let b = own_batch(run, s, to, dt);
            let acc = clock.accumulated();
            let first = start_particles(run, systems, drive, g, clock.t(), &b, dt, n, n, 0.0, acc, acc, rng);
            ring_copy(run, systems, g, first, acc, rng);
        }
        clock.substeps(&spec, step, |pass| {
            modules(run, systems, drive, g, 0, pass.to, pass.d, pass.accumulated, rng);
            if let Some(e) = pass.emit.filter(|e| *e > 0.0 && on) {
                if let Some((n, total)) = run.emitted.as_mut().map(|st| st.over_time(&rate, &ps.bursts, pass.from, pass.to, length)) {
                    let b = own_batch(run, s, pass.to, e);
                    let first = start_particles(run, systems, drive, g, pass.to, &b, e, n, total, pass.left, 0.0, pass.accumulated, rng);
                    ring_copy(run, systems, g, first, pass.accumulated, rng);
                }
            }
            // Update1Incremental: with ExternalForcesModule, UpdateBounds between the passes.
            if ps.external_forces.is_some() && pass.d <= pass.accumulated - pass.d {
                update_bounds(run, s, g);
            }
        });
        if !incremental_size(ps, drag_scalar(ps, drive), collision_of(ps, g, drive).is_some()) {
            for p in &mut run.particles {
                size_update(s, p);
            }
        }
        update_bounds(run, s, g);
    }
    run.clock = clock;
    // CollectSubEmittersRecursive (1801b7130): each sub system's update job runs after its parent's.
    for child in &mut run.children {
        step(child, systems, None, g, frame, rng);
    }
}

/// A script's module writes on a system (None: not written): emission.rateOverTimeMultiplier and
/// rateOverDistanceMultiplier, main.startSpeedMultiplier (the curves' scalars, their modes kept) and main.startColor
/// (a constant colour).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PsWrites {
    pub rate: Option<f32>,
    pub distance: Option<f32>,
    pub speed: Option<f32>,
    pub color: Option<[f32; 4]>,
}

/// The ParticleSystem.EmitParams fields the scripts set (FireParticleSystem, NukeShockwave, PyroPowerTool); None is
/// the field's Reset (its *Set flag false).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EmitParams {
    pub position: Option<Vec3>,
    /// EmitParams.rotation in degrees (the setter keeps z = value * (PI / 180f)).
    pub rotation: Option<f32>,
    pub start_size: Option<f32>,
    /// EmitParams.startColor from a Color (the implicit Color32: Round(Clamp01(c) * 255) per channel).
    pub start_color: Option<[f32; 4]>,
}

/// Color to Color32 (UnityEngine.Color32's implicit operator): (byte)Mathf.Round(Mathf.Clamp01(c) * 255f).
fn script_color32(c: [f32; 4]) -> u32 {
    let b = |v: f32| (crate::game::mathf::clamp01(v) * 255.0).round_ties_even() as u32 & 0xff;
    b(c[0]) | b(c[1]) << 8 | b(c[2]) << 16 | b(c[3]) << 24
}

/// ParticleSystem.Emit(EmitParams, 1): EmitParticlesExternal (1801b9870) with InitialModule::StartExternal
/// (180201510). The unset fields draw from InitialModule's RandN (one batch: lane 0) in the order rotation, size,
/// colour, randomSeed, lifetime, all at t = state.t / length; CopyFromArrayAOS (1801b7a40) gives aliveTimePercent 0
/// and invStartLifetime = 1 / startLifetime; then StartVelocity, StartLifetimeByEmitterSpeed, UpdateSizeModules and
/// CopyParticlesToUnalignedDst. No step, no sub-emitter (SubModule::Update meets the particle in the next update).
fn emit_external(run: &mut Run, systems: &[Sys], g: &Game, e: &EmitParams, rng: &mut Rng) {
    use crate::game::particle_modules as m;
    use crate::game::particle_sim::{evaluate, gradient, maxps};
    let s = &systems[run.sys];
    let ps = &s.ps;
    let (rot3d, size3d) = (uses_3d_rotation(ps), uses_3d_size(ps));
    let [size_y, size_z, rot_x, rot_y] = &ps.start_raw_3d;
    // A full particle array without a ring buffer takes nothing.
    if ps.ring_buffer_mode == 0 && run.particles.len() >= ps.max_particles.max(0) as usize {
        return;
    }
    let pose = pose(run, s);
    let world = ps.space != 0;
    let mx = if world { pose.l2w } else { [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] };
    let t = run.clock.t() / ps.length;
    let position = match e.position {
        Some(p) => p,
        None if ps.shape_enabled => unimplemented!("mod-only: Emit(EmitParams) without a position on a shaped system (ShapeModule::Start on a randomSeed Rand, 1801b9e33)"),
        None => Vec3::new(mx[12], mx[13], mx[14]),
    };
    // M * zAxis.
    let dir = [(mx[8] * 1.0 + mx[0] * 0.0) + mx[4] * 0.0, (mx[9] * 1.0 + mx[1] * 0.0) + mx[5] * 0.0, (mx[10] * 1.0 + mx[2] * 0.0) + mx[6] * 0.0];
    let size_curve = start_size(run, ps).unwrap_or_else(|| ps.start_size.clone());
    let script_color = run.color;
    let rand = &mut run.seeds.as_mut().expect("a played run has its seeds").initial;
    // With uses3DRotation (uses3DSize) the curves go z, y, x, one draw each; x and y of the rotation are not kept.
    let rotation = match e.rotation {
        Some(deg) => deg * crate::game::mathf::DEG2RAD,
        None => {
            let z = evaluate(&ps.start_rotation, t, rand.get01()[0]);
            if rot3d {
                evaluate(rot_y, t, rand.get01()[0]);
                evaluate(rot_x, t, rand.get01()[0]);
            }
            z
        }
    };
    let size = match e.start_size {
        Some(v) => [v; 3],
        None if size3d => {
            let z = maxps(0.0, evaluate(size_z, t, rand.get01()[0]));
            let y = maxps(0.0, evaluate(size_y, t, rand.get01()[0]));
            [maxps(0.0, evaluate(&size_curve, t, rand.get01()[0])), y, z]
        }
        None => [maxps(0.0, evaluate(&size_curve, t, rand.get01()[0])); 3],
    };
    let color = match e.start_color {
        Some(c) => script_color32(c),
        None => {
            let r = rand.get01()[0];
            script_color.map_or_else(|| gradient(&ps.start_color, t, r), crate::game::particle_sim::color32)
        }
    };
    let seed = rand.get()[0];
    let start_lifetime = maxps(f32::from_bits(0x3727c5ac), evaluate(&ps.start_lifetime, t, rand.get01()[0]));
    let ev = run.emitter_velocity.to_array();
    let masked = if world { ev } else { [0.0; 3] };
    let inherit = ps.inherit.as_ref().map(|(mode, c)| (*mode, c));
    let velocity = m::start_velocity(&start_speed(run, ps, None), inherit, world, masked, seed, t, dir);
    let started = crate::game::particle_sim::Started { seed, parent_seed: 0, inv_lifetime: 1.0 / start_lifetime, size, rotation: [0.0, 0.0, rotation], color };
    let mut p = create(run, None, started, position, Vec3::from_array(velocity));
    p.color32 = color;
    p.from = position;
    if let Some((curve, range)) = &ps.lifetime_by_speed {
        p.inv_lifetime = m::lifetime_by_speed(curve, *range, ev, p.seed, p.inv_lifetime);
    }
    p.lifetime = 1.0 / p.inv_lifetime;
    size_update(s, &mut p);
    // maxSize takes each axis of the start size.
    for v in [p.size, p.size_y, p.size_z] {
        if run.max_size < v {
            run.max_size = v;
        }
    }
    // At round4(count), then copied down. CopyFromArrayAOS fills the batch's other lanes with the same emitted particle
    // (1801b9d67), and StartVelocity, StartLifetimeByEmitterSpeed and UpdateSizeModules then run on all four alike.
    let first = run.particles.len();
    let gap = open(run, 4);
    place(run, gap, vec![Some(p.clone()), Some(p.clone()), Some(p.clone()), Some(p)], 1);
    if ps.ring_buffer_mode != 0 {
        let acc = run.clock.accumulated();
        ring_copy(run, systems, g, first, acc, rng);
    }
    run.invalid = true;
    // A playing system leaves its bounds to the frame's update (nothing is culled in the rewrite).
    if run.halted {
        update_bounds(run, s, g);
    }
}

/// The lane fold of UpdateBounds' accumulators (pshufd 0x93, minps/maxps, pshufd 0x39 and 0x93, minps/maxps).
fn fold(v: [f32; 4], f: fn(f32, f32) -> f32) -> [f32; 4] {
    let a = [f(v[3], v[0]), f(v[0], v[1]), f(v[1], v[2]), f(v[2], v[3])];
    std::array::from_fn(|l| f(a[(l + 3) % 4], a[(l + 1) % 4]))
}

/// ParticleSystem::UpdateBounds (1801c5ea0) into the run's minMaxAABB: procedural when supportsProcedural and not
/// invalidateProcedural.
fn update_bounds(run: &mut Run, s: &Sys, g: &Game) {
    let start = start_size(run, &s.ps).unwrap_or_else(|| s.ps.start_size.clone());
    let pose = pose(run, s);
    let positions: Vec<[f32; 3]> = run.particles.iter().map(|p| p.position.to_array()).collect();
    let trails: Vec<Option<[f32; 3]>> = run.particles.iter().map(|p| p.trail.back().map(|q| [q[0], q[1], q[2]])).collect();
    let ps = &s.ps;
    // A disabled sprite-mode UVModule's m_SpriteRectData is empty: PrepareForRender (180e2e6d0) builds it only when
    // enabled.
    let sprites = (ps.uv_mode == 1).then_some(&s.frames[..]);
    let t = [pose.l2w[12], pose.l2w[13], pose.l2w[14]];
    // invalidateProcedural: Play clears it; EmitParticlesExternal, MarkAllSubEmitters and every scripting module setter
    // on a system that is not stopped set it.
    // The scripts' module writes set it where they happen (`write`).
    let procedural = (ps.supports_procedural() && !run.invalid).then_some((pose.emitter, [g.world.gravity.x, g.world.gravity.y, 0.0]));
    if ps.lights && !ps.lights_light.is_null() {
        unimplemented!("mod-only: UpdateBounds' LightsModule margin with a light prefab (its Light's m_Range): no game system's LightsModule has one");
    }
    run.bounds = min_max_aabb(ps, s.pivot.to_array(), sprites, None, &start, &positions, &trails, run.max_size, t, &pose.w2l, pose.render, procedural);
}

/// FindMinMax (1801ba930) and FindMinMaxIntegrated (1801bac60) of a constant or two constants: (min, max).
fn find_min_max(c: &MinMaxCurve) -> (f32, f32) {
    match c.state {
        0 if c.scalar > 0.0 => (0.0, c.scalar),
        0 => (c.scalar, 0.0),
        3 if c.scalar > c.min_scalar => (c.min_scalar, c.scalar),
        3 => (c.scalar, c.min_scalar),
        s => unimplemented!("mod-only: FindMinMax of a curve (state {s}) for UpdateBounds' procedural bounds: no procedural game system has one"),
    }
}

/// TransformAABBSlow (180643e10): the eight corners (from the min corner, x and y turning first) through `m`, minss and
/// maxss from infinity.
fn transform_aabb_slow(lo: [f32; 3], hi: [f32; 3], m: &[f32; 16]) -> ([f32; 3], [f32; 3]) {
    use crate::game::particle_sim::{maxps, minps};
    let (mut a, mut b) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for [i, j, k] in [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]] {
        let c = [[lo[0], hi[0]][i], [lo[1], hi[1]][j], [lo[2], hi[2]][k]];
        for r in 0..3 {
            let v = ((c[0] * m[r] + c[1] * m[4 + r]) + c[2] * m[8 + r]) + m[12 + r];
            a[r] = minps(v, a[r]);
            b[r] = maxps(v, b[r]);
        }
    }
    (a, b)
}

/// ShapeModule::CalculateProceduralBounds (180dfafd0) for the game's procedural shapes: the shape's own box (Circle
/// (r, r, 0.1), SingleSidedEdge (r, 0.1, 0.1), Rectangle (0.5, 0.5, 0), min = -max) through SetTRS(m_Position, the
/// normalized Euler quaternion, m_Scale), times emitterScale; then the direction box (randomDirection: +-1 and |speed|)
/// rotated by the quaternion, at the max speed added to and at the min speed merged into the bounds.
fn shape_procedural(ps: &ParticleSystem, b: &mut ([f32; 3], [f32; 3]), emitter: [f32; 3], speed: [f32; 2]) {
    use crate::game::particle_sim::{maxps, minps};
    use crate::unity::transform::{normalize, quaternion_to_matrix};
    let r = ps.shape_radius.value;
    let top = match ps.shape_type {
        10 => [r, r, 0.1],
        12 => [r, 0.1, 0.1],
        18 => [0.5, 0.5, 0.0],
        t => unimplemented!("mod-only: ShapeModule::CalculateProceduralBounds of shape type {t}: no procedural game system has it"),
    };
    let rot = quaternion_to_matrix(normalize(crate::game::particle_shape::euler_quat(ps.shape_rotation)));
    let mut m = rot;
    for c in 0..3 {
        for i in 0..3 {
            m[4 * c + i] *= ps.shape_scale[c];
        }
        m[12 + c] = ps.shape_position[c];
    }
    let (lo, hi) = transform_aabb_slow(top.map(|v| -v), top, &m);
    *b = (std::array::from_fn(|i| lo[i] * emitter[i]), std::array::from_fn(|i| hi[i] * emitter[i]));
    let mut speed = speed;
    let (dlo, dhi) = if ps.shape_random_direction > 0.0 {
        speed = speed.map(f32::abs);
        ([-1.0; 3], [1.0; 3])
    } else {
        match ps.shape_type {
            10 => ([-1.0; 3], [1.0; 3]),
            12 => ([0.0; 3], [0.0, 1.0, 0.0]),
            _ => ([0.0; 3], [0.0, 0.0, 1.0]),
        }
    };
    let (dlo, dhi) = transform_aabb_slow(dlo, dhi, &rot);
    let (u, v) = (speed[0], speed[1]);
    for i in 0..3 {
        b.0[i] = minps(v * dlo[i] + b.0[i], b.0[i]);
        b.1[i] = maxps(v * dhi[i] + b.1[i], b.1[i]);
    }
    for i in 0..3 {
        let (x, y) = (u * dlo[i], u * dhi[i]);
        b.0[i] = minps(minps(y, x), b.0[i]);
        b.1[i] = maxps(maxps(y, x), b.1[i]);
    }
}

/// UpdateBounds' procedural minMaxAABB (1801c5fc0): z from 0 through speed x lifetime (FindMinMax maxima), the shape's
/// procedural bounds, gravity (GetGravity x L^2 x 0.5 x the modifier, through worldToLocal in local simulation) added
/// one-sided, then the velocity module's integrated box times L (TransformAABBSlow by worldToLocal's rotation in world
/// space) and the force module's (none here) added and merged. `emitter` is emitterScale, `gravity` Physics.gravity.
/// The Stretch renderer's extra margin is not reached: the rewrite draws Billboard and None only.
fn procedural_aabb(ps: &ParticleSystem, emitter: [f32; 3], gravity: [f32; 3], w2l: &[f32; 16]) -> ([f32; 3], [f32; 3]) {
    use crate::game::particle_sim::{maxps, minps};
    let l = find_min_max(&ps.start_lifetime).1;
    let (s0, s1) = find_min_max(&ps.start_speed);
    let speed = [s0 * l, s1 * l];
    let mut b = ([0.0f32; 3], [0.0f32; 3]);
    for v in speed {
        b.0 = [minps(0.0, b.0[0]), minps(0.0, b.0[1]), minps(v, b.0[2])];
        b.1 = [maxps(0.0, b.1[0]), maxps(0.0, b.1[1]), maxps(v, b.1[2])];
    }
    if ps.shape_enabled {
        shape_procedural(ps, &mut b, emitter, speed);
    }
    // GetGravity (1801ff0f0): 0 when the modifier's scalar is 0.
    let k = ps.gravity.scalar;
    let g = if k == 0.0 { [0.0; 3] } else { gravity }.map(|c| ((c * l) * l) * 0.5 * k);
    let g = if ps.space == 1 { g } else { std::array::from_fn(|i| (g[0] * w2l[i] + g[1] * w2l[4 + i]) + g[2] * w2l[8 + i]) };
    for i in 0..3 {
        b.1[i] = maxps(0.0, g[i]) + b.1[i];
        b.0[i] = minps(0.0, g[i]) + b.0[i];
    }
    let mut t = ([0.0; 3], [0.0; 3]);
    if let Some(v) = &ps.velocity {
        let r = v.axes.each_ref().map(find_min_max);
        t = (r.map(|r| r.0 * l), r.map(|r| r.1 * l));
        if v.world {
            let mut m = *w2l;
            m[12..15].copy_from_slice(&[0.0; 3]);
            t = transform_aabb_slow(t.0, t.1, &m);
        }
    }
    if ps.force.is_some() {
        unimplemented!("mod-only: ForceModule::CalculateProceduralBounds (1801df610): no procedural game system enables the force module");
    }
    // The velocity box, then the force module's zeros.
    for t in [t, ([0.0; 3], [0.0; 3])] {
        for i in 0..3 {
            b.0[i] = minps(t.0[i] + b.0[i], b.0[i]);
            b.1[i] = maxps(t.1[i] + b.1[i], b.1[i]);
        }
    }
    b
}

/// UpdateBounds' minMaxAABB: `procedural` (emitterScale, Physics.gravity) takes `procedural_aabb`; otherwise no
/// particle gives the system position (world simulation) or 0, +- Vector3f::epsilon, and particles `particle_aabb`;
/// then the size margin (with LightsModule, times max(`light` range or 1 times m_Range, 1)): per size axis (three with uses3DSize) FindMinMax(InitialModule size).max times
/// the SizeModule and SizeBySpeedModule curves' maxima, the largest (from 0), at least maxSize; that times the renderer
/// pivot's largest |axis| first, then the margin times 0.71, the largest render scale (world simulation) and, with UV
/// sprites, the largest of 1 and the sprite rect data's widths and heights after the first.
#[allow(clippy::too_many_arguments)]
fn min_max_aabb(
    ps: &ParticleSystem, pivot: [f32; 3], sprites: Option<&[[f32; 8]]>, light: Option<f32>, start: &MinMaxCurve, positions: &[[f32; 3]], trails: &[Option<[f32; 3]>], max_size: f32, position: [f32; 3],
    w2l: &[f32; 16], render: [f32; 3], procedural: Option<([f32; 3], [f32; 3])>,
) -> [f32; 6] {
    use crate::game::particle_sim::maxps;
    use crate::particle_render::find_max;
    let world = ps.space == 1;
    let (min, max) = match procedural {
        Some((emitter, gravity)) => procedural_aabb(ps, emitter, gravity, w2l),
        None if positions.is_empty() => {
            let eps = f32::from_bits(0x3727_c5ac);
            let t = if world { position } else { [0.0; 3] };
            return [t[0] - eps, t[1] - eps, t[2] - eps, eps + t[0], eps + t[1], eps + t[2]];
        }
        None => particle_aabb(ps, positions, trails, w2l),
    };
    let [size_y, size_z, ..] = &ps.start_raw_3d;
    let axes = if uses_3d_size(ps) { 3 } else { 1 };
    let mut m = 0.0;
    for (i, initial) in [start, size_y, size_z].into_iter().take(axes).enumerate() {
        let mut v = find_max(initial);
        if let Some(c) = &ps.size {
            v *= find_max([&c.x, &c.y, &c.z][i]);
        }
        if let Some((c, _)) = &ps.size_by_speed {
            v *= find_max([&c.x, &c.y, &c.z][i]);
        }
        m = maxps(v, m);
    }
    let big = if max_size > m { max_size } else { m };
    // Every system has its renderer (Library::system).
    let p = pivot.map(f32::abs);
    let k = maxps(maxps(p[0], p[1]), maxps(p[2], p[0])) * big;
    let (min, max) = (min.map(|v| v - k), max.map(|v| k + v));
    // LightsModule: max(the light's range (1 without one) times m_Range, 1) times the margin.
    let big = if ps.lights { maxps(light.unwrap_or(1.0) * ps.lights_range, 1.0) * big } else { big };
    let mut f = big * f32::from_bits(0x3f35_c28f);
    if world {
        let r = render;
        let yz = if r[2] <= r[1] { r[1] } else { r[2] };
        f *= if r[0] < yz { yz } else { r[0] };
    }
    if let Some(frames) = sprites {
        let mut e = 1.0;
        for r in frames.iter().skip(1) {
            let wv = if r[6] > e { r[6] } else { e };
            e = if r[7] > wv { r[7] } else { wv };
        }
        f *= e;
    }
    [min[0] - f, min[1] - f, min[2] - f, f + max[0], f + max[1], f + max[2]]
}

/// UpdateBounds' particle box: minps/maxps over the positions in blocks of four (PadParticleDataToSIMDBoundary repeats
/// the last particle), then each per-particle trail's newest point (into system space for a world-space trail of a
/// local system).
fn particle_aabb(ps: &ParticleSystem, positions: &[[f32; 3]], trails: &[Option<[f32; 3]>], w2l: &[f32; 16]) -> ([f32; 3], [f32; 3]) {
    use crate::game::particle_sim::{maxps, minps};
    let world = ps.space == 1;
    let n = positions.len();
    let (mut lo, mut hi) = ([[f32::INFINITY; 4]; 3], [[f32::NEG_INFINITY; 4]; 3]);
    for b in (0..n).step_by(4) {
        for l in 0..4 {
            let p = positions[(b + l).min(n - 1)];
            for a in 0..3 {
                lo[a][l] = minps(lo[a][l], p[a]);
                hi[a][l] = maxps(hi[a][l], p[a]);
            }
        }
    }
    let lanes = [2, 3, 0];
    let mut min: [f32; 3] = std::array::from_fn(|a| fold(lo[a], minps)[lanes[a]]);
    let mut max: [f32; 3] = std::array::from_fn(|a| fold(hi[a], maxps)[lanes[a]]);
    // The loop runs to min(particles, m_MaxTrails): AddNewParticles (0x1801b4bc0) calls array_reserve (0x1801ca040,
    // ParticleTrails::Reallocate 0x1801bdab0 to the reserved count) before array_resize, and TrailModule::Update only
    // grows it, so m_MaxTrails is never below the particle count and the min is the count.
    if let Some(t) = ps.trail.as_ref().filter(|t| t.mode == 0) {
        let w = w2l;
        for q in trails.iter().flatten() {
            let v: [f32; 3] = if !t.world_space || world { *q } else { std::array::from_fn(|i| ((w[8 + i] * q[2] + w[4 + i] * q[1]) + w[i] * q[0]) + w[12 + i]) };
            for a in 0..3 {
                min[a] = minps(v[a], min[a]);
                max[a] = maxps(v[a], max[a]);
            }
        }
    }
    (min, max)
}

/// CalculateWorldBounds (1801b6a90) of a run's minMaxAABB: its world centre and extent (the draw-order sorter's z is
/// the centre's).
pub(crate) fn world_bounds(run: &Run, s: &Sys) -> [f32; 6] {
    crate::game::particle_forces::world_bounds(&run.bounds, s.ps.space == 1, &pose(run, s).l2w)
}

/// Whether the ParticleSystemRenderer of `embedded_ps[i]` is drawn by the main camera: its world AABB (CalculateWorldBounds) meets the
/// camera's view rectangle (no view: everything is drawn). A system that has not stepped yet has no bounds and is not drawn.
pub(crate) fn embedded_drawn(g: &Game, i: usize) -> bool {
    let Some(w) = g.particle_world.as_ref() else { return false };
    let Some((run, ..)) = w.embedded.get(&i) else { return false };
    seen(g, run, &w.library.systems[run.sys])
}

/// Whether the main camera's culling sees a run's renderer: its world AABB (UpdateBounds') meets the view rectangle
/// (no view: everything is seen).
fn seen(g: &Game, run: &Run, s: &Sys) -> bool {
    let b = world_bounds(run, s);
    g.input.view.is_none_or(|(min, max)| (0..2).all(|k| b[k] + b[3 + k] >= min[k] && b[k] - b[3 + k] <= max[k]))
}

/// Stopped (or done emitting) with no particle left.
/// ParticleSystem.isEmitting of a script's embedded system: played (`started`, else playOnAwake) and not done
/// emitting (Stop, or a non-looping system past its duration).
pub fn is_emitting(g: &Game, owner: i32) -> bool {
    let Some(i) = g.embedded_ps.iter().position(|e| e.owner == owner) else { return false };
    emitting(g, i)
}

/// ParticleSystem.isEmitting of `embedded_ps[i]`: the last Play or Stop since the update decides (Play clears
/// stopEmitting, Stop sets it); otherwise the system as the update left it (one never played is not emitting).
pub(crate) fn emitting(g: &Game, i: usize) -> bool {
    use crate::game::PsCall;
    // gapC r11: a playing system with its emission module off is not emitting (HoverThrusterBehaviour's Effects: "01").
    if !emission_on(g, i) {
        return false;
    }
    for (k, c) in g.ps_calls.iter().rev() {
        match c {
            PsCall::Play if *k == i => return true,
            PsCall::Stop(_) if *k == i => return false,
            _ => {}
        }
    }
    let w = g.particle_world.as_ref();
    match w.and_then(|w| w.embedded.get(&i)) {
        Some((run, ..)) => !run.stopped && !run.clock.stop_emitting,
        // Not updated yet: playing from its Awake when playOnAwake.
        None => g.assets.as_ref().is_some_and(|db| {
            let go = g.embedded_ps[i].go;
            let ps = db.game_object(go).ok().and_then(|o| o.components.into_iter().filter_map(|c| db.resolve(go.file, c)).find(|c| db.class(*c) == 198));
            ps.is_some_and(|c| ParticleSystem::read(db.data(c)).unwrap().play_on_awake)
        }),
    }
}

/// gapC r11: EmissionModule.enabled of `embedded_ps[i]`: a script's write (ps_set_emission), else the asset's.
pub(crate) fn emission_on(g: &Game, i: usize) -> bool {
    let w = g.particle_world.as_ref();
    g.embedded_ps[i].emission.unwrap_or_else(|| match w.and_then(|w| Some((w, &w.embedded.get(&i)?.0))) {
        Some((w, run)) => w.library.systems[run.sys].ps.emission_enabled,
        None => ps_asset(g, i).is_none_or(|p| p.emission_enabled),
    })
}

/// The ParticleSystem asset of `embedded_ps[i]`'s object.
fn ps_asset(g: &Game, i: usize) -> Option<ParticleSystem> {
    let db = g.assets.as_ref()?;
    let go = g.embedded_ps[i].go;
    let c = db.game_object(go).ok()?.components.into_iter().filter_map(|c| db.resolve(go.file, c)).find(|c| db.class(*c) == 198)?;
    ParticleSystem::read(db.data(c)).ok()
}

/// gapC r11: ParticleSystem.isPlaying of `embedded_ps[i]`: played (a Play since the update, or the update's run; one not updated
/// yet plays from its Awake when playOnAwake) until stopped with no particle left: a Stop since the update ends it at once only
/// when ours' run holds no particle (Stop's Clear), and the update's kStopped (`halted`) ends it.
pub(crate) fn playing(g: &Game, i: usize) -> bool {
    use crate::game::PsCall;
    let run = g.particle_world.as_ref().and_then(|w| w.embedded.get(&i)).map(|x| &x.0);
    for (k, c) in g.ps_calls.iter().rev() {
        match c {
            PsCall::Play if *k == i => return true,
            PsCall::Stop(_) if *k == i => return run.is_some_and(|r| !r.particles.is_empty()),
            _ => {}
        }
    }
    match run {
        Some(r) => !r.halted && !(r.stopped && r.particles.is_empty()),
        None => ps_asset(g, i).is_some_and(|p| p.play_on_awake),
    }
}

impl Game {
    /// gapC r11: `<field>.emission.enabled = on` on script `sid`'s ParticleSystem field (field_ps): the one place a script's
    /// EmissionModule.enabled write lands; isEmitting (emitting) and the update's emission read it.
    pub(crate) fn ps_set_emission(&mut self, sid: i32, field: &str, on: bool) {
        if let Some(i) = self.field_ps(sid, field) {
            self.embedded_ps[i].emission = Some(on);
        }
    }

    /// The world pose and lossy scale of `embedded_ps[i]`'s Transform now.
    pub(crate) fn embedded_pose(&self, i: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let e = &self.embedded_ps[i];
        match e.drive {
            crate::game::EmbeddedDrive::Pyro { e: pe, .. } => self.pyro_ps_world(pe, &e.chain),
            crate::game::EmbeddedDrive::Muzzle(f) => self.muzzle_ps_world(f, e),
            _ => self.anchored_world(e.anchor, &e.chain),
        }
    }

    /// gapC r10: ours' embedded system for script `sid`'s serialized ParticleSystem field `field` (its reference into the
    /// clone): the system on that object, a Free one (owner: its own id) or one the script drives. One place for every
    /// script's `<field>.Play()`/`.Stop()` (ps_field_call) and the fn-check's isEmitting compare of that field.
    pub(crate) fn field_ps(&self, sid: i32, field: &str) -> Option<usize> {
        self.field_ps_tree(sid, field, false).first().copied()
    }

    /// field_ps, then with `children` the systems on the objects below it (Play/Stop's withChildren, true by default).
    pub(crate) fn field_ps_tree(&self, sid: i32, field: &str, children: bool) -> Vec<usize> {
        self.field_ps_at(sid, field, children).unwrap_or_default()
    }

    /// Script `sid`'s serialized field `field` (with its clone and object): `field` may name an array element and a
    /// struct member, as "ParticleSystems[1]" or "FlameParticles[0].System".
    fn field_value(&self, sid: i32, field: &str) -> Option<(&crate::game::spawn::Instance, usize, &crate::assets::mono::Value)> {
        use crate::assets::db::Component;
        let inst = self.unsynced.iter().chain(&self.retained).find(|i| i.objects.iter().any(|x| x.components.contains(&sid)))?;
        let o = inst.objects.iter().position(|x| x.components.contains(&sid))?;
        let c = inst.objects[o].components.iter().position(|&k| k == sid)?;
        let Component::Script { fields, .. } = &inst.prefab.objects[o].components[c].1 else { return None };
        let mut v = None;
        for part in field.split('.') {
            let (name, k) = match part.split_once('[') {
                Some((n, k)) => (n, Some(k.trim_end_matches(']').parse::<usize>().ok()?)),
                None => (part, None),
            };
            let x = match v {
                None => fields.get(name)?,
                Some(v) => crate::assets::mono::Value::get(v, name)?,
            };
            v = Some(match k {
                Some(k) => x.array()?.get(k)?,
                None => x,
            });
        }
        Some((inst, o, v?))
    }

    /// Script `sid`'s float field `field` (field_value's paths).
    pub(crate) fn field_f32(&self, sid: i32, field: &str) -> Option<f32> {
        self.field_value(sid, field).and_then(|x| x.2.f32())
    }

    /// The length of script `sid`'s array field `field` (0 when it has none).
    pub(crate) fn field_len(&self, sid: i32, field: &str) -> usize {
        self.field_value(sid, field).and_then(|x| x.2.array().map(|a| a.len())).unwrap_or(0)
    }

    /// gapC r11: script `sid`'s serialized Component field `field` (a reference into its own clone) as (object index in the
    /// clone, the component's instance id).
    fn field_target(&self, sid: i32, field: &str) -> Option<(&crate::game::spawn::Instance, usize, i32)> {
        use crate::assets::db::ObjRef;
        let (inst, o, v) = self.field_value(sid, field)?;
        let p = v.pptr().filter(|p| p.file == 0 && !p.is_null())?;
        let &(t, k) = inst.prefab.index.get(&ObjRef { file: inst.prefab.objects[o].game_object.file, path_id: p.path_id })?;
        Some((inst, t, *inst.objects[t].components.get(k)?))
    }

    /// gapC r11: the instance id of script `sid`'s Component field `field` (field_target).
    pub(crate) fn field_component(&self, sid: i32, field: &str) -> Option<i32> {
        self.field_target(sid, field).map(|x| x.2)
    }

    fn field_ps_at(&self, sid: i32, field: &str, children: bool) -> Option<Vec<usize>> {
        let (inst, t, psid) = self.field_target(sid, field)?;
        let go = inst.prefab.objects[t].game_object;
        let mut out = vec![self.embedded_ps.iter().position(|e| e.go == go && (e.owner == psid || e.owner == sid))?];
        let below = |mut x: usize| loop {
            match inst.objects[x].parent {
                Some(p) if p == t => break true,
                Some(p) => x = p,
                None => break false,
            }
        };
        for x in (0..inst.objects.len()).filter(|&x| children && below(x)) {
            let go = inst.prefab.objects[x].game_object;
            out.extend(self.embedded_ps.iter().position(|e| e.go == go && inst.objects[x].components.contains(&e.owner)));
        }
        Some(out)
    }

    /// gapC r10: `<field>.Play(withChildren)` / `.Stop(withChildren)` on script `sid`'s ParticleSystem field (field_ps_tree);
    /// nothing when ours has none.
    pub(crate) fn ps_field_call(&mut self, sid: i32, field: &str, children: bool, call: crate::game::PsCall) {
        self.adopt_field_ps(sid, field);
        for i in self.field_ps_tree(sid, field, children) {
            self.ps_calls.push((i, call));
        }
    }

    /// A recorded clone's ParticleSystem field target (link_instance's Free systems are a spawned clone's): it joins
    /// embedded_ps as a Free system under its nearest PhysicalBehaviour, so the script's call reaches it.
    fn adopt_field_ps(&mut self, sid: i32, field: &str) {
        if self.field_ps(sid, field).is_some() {
            return;
        }
        let Some((inst, t, psid)) = self.field_target(sid, field) else { return };
        if !inst.prefab.objects[t].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Other(crate::game::water::PARTICLE_SYSTEM))) {
            return;
        }
        let (mut chain, mut up) = (Vec::new(), Some(t));
        let anchor = loop {
            let Some(x) = up else { break crate::game::BareAnchor::Root };
            if let Some(p) = self.physicals.iter().position(|p| inst.objects[x].components.contains(&p.id)) {
                break crate::game::BareAnchor::Physical(p);
            }
            chain.push(inst.objects[x].trs);
            up = inst.objects[x].parent;
        };
        let go = inst.prefab.objects[t].game_object;
        self.embedded_ps.push(crate::game::EmbeddedPs { go, anchor, chain, owner: psid, drive: crate::game::EmbeddedDrive::Free, shape_texture: None, awake: true, emission: None });
    }

    /// A script's module writes on its ParticleSystem field (the system itself, not its children).
    pub(crate) fn ps_field_writes(&mut self, sid: i32, field: &str, w: PsWrites) {
        self.ps_field_call(sid, field, false, crate::game::PsCall::Writes(w));
    }

    /// gapC r11: `<field>.Play()` (on) / `<field>.Stop()` (off), unconditional.
    pub(crate) fn ps_field_play_stop(&mut self, sid: i32, field: &str, on: bool) {
        self.ps_field_call(sid, field, true, if on { crate::game::PsCall::Play } else { crate::game::PsCall::Stop(self.time.timeAsDouble) });
    }

    /// gapC r10: `if (!<field>.isEmitting) <field>.Play()` (on) / `if (<field>.isEmitting) <field>.Stop()` (off).
    pub(crate) fn ps_field_emit(&mut self, sid: i32, field: &str, on: bool) {
        if self.field_ps(sid, field).is_some_and(|i| emitting(self, i) != on) {
            self.ps_field_call(sid, field, true, if on { crate::game::PsCall::Play } else { crate::game::PsCall::Stop(self.time.timeAsDouble) });
        }
    }

    /// gapC r11: `if (!<field>.isPlaying) <field>.Play()` (on) / `if (<field>.isPlaying) <field>.Stop()` (off), by `playing`.
    pub(crate) fn ps_field_playing(&mut self, sid: i32, field: &str, on: bool) {
        if self.field_ps(sid, field).is_some_and(|i| playing(self, i) != on) {
            self.ps_field_call(sid, field, true, if on { crate::game::PsCall::Play } else { crate::game::PsCall::Stop(self.time.timeAsDouble) });
        }
    }

    /// `<field>.Emit(n)` on script `sid`'s ParticleSystem field, at its pose now.
    pub(crate) fn ps_field_emit_n(&mut self, sid: i32, field: &str, n: i32) {
        if let Some(i) = self.field_ps(sid, field) {
            let (p, q, _) = self.embedded_pose(i);
            self.ps_calls.push((i, crate::game::PsCall::Emit(n, p, q)));
        }
    }

    /// `<field>.transform.localScale`: read, then set to what `f` makes of it.
    pub(crate) fn field_local_scale(&mut self, sid: i32, field: &str, f: impl FnOnce([f32; 3]) -> [f32; 3]) {
        if let Some(t) = self.field_component(sid, field) {
            self.with_component_instance(t, |_, inst, o| {
                let s = inst.objects[o].trs.s;
                inst.set_local_scale(o, f([s[0], s[1], s[2]]));
            });
        }
    }

    /// A script's call on the embedded systems `which` picks (in embedded_ps order).
    pub(crate) fn ps_call(&mut self, which: impl Fn(&crate::game::EmbeddedPs) -> bool, call: crate::game::PsCall) {
        for i in 0..self.embedded_ps.len() {
            if which(&self.embedded_ps[i]) {
                self.ps_calls.push((i, call));
            }
        }
    }

    /// ParticleSystem.Emit(n) on the embedded systems `which` picks, at their pose now.
    pub(crate) fn ps_emit(&mut self, which: impl Fn(&crate::game::EmbeddedPs) -> bool, n: i32) {
        for i in 0..self.embedded_ps.len() {
            if which(&self.embedded_ps[i]) {
                let (p, q, _) = self.embedded_pose(i);
                self.ps_calls.push((i, crate::game::PsCall::Emit(n, p, q)));
            }
        }
    }

    /// ParticleSystem.Emit(EmitParams, 1) on the embedded systems `which` picks, at their pose now.
    pub(crate) fn ps_emit_params(&mut self, which: impl Fn(&crate::game::EmbeddedPs) -> bool, params: EmitParams) {
        for i in 0..self.embedded_ps.len() {
            if which(&self.embedded_ps[i]) {
                let (p, q, _) = self.embedded_pose(i);
                self.ps_calls.push((i, crate::game::PsCall::EmitParams(params, p, q)));
            }
        }
    }
}

/// ParticleSystem::Emit(int) (1801b89f0): StartParticles at the system time with dt 0, rate 0 and `n` particles, then
/// UpdateSizeModules on them when no module sizes them incrementally (StartParticles did it otherwise).
fn emit_count(run: &mut Run, systems: &[Sys], g: &Game, n: i32, rng: &mut Rng) {
    if n <= 0 {
        return;
    }
    let s = &systems[run.sys];
    let t = run.clock.t();
    let b = own_batch(run, s, t, 0.0);
    let first = start_particles(run, systems, None, g, t, &b, 0.0, 0, n as u64, 0.0, 0.0, 0.0, rng);
    if !incremental_size(&s.ps, drag_scalar(&s.ps, None), collision_of(&s.ps, g, None).is_some()) {
        let pad = pad_lanes(run.particles.len(), first);
        for p in run.particles[first..].iter_mut().chain(run.tail.iter_mut().take(pad).flatten()) {
            size_update(s, p);
        }
    }
}

fn done(run: &Run) -> bool {
    run.particles.is_empty() && (run.stopped || run.clock.stop_emitting) && run.children.iter().all(done)
}

/// ColorRGBA32 to and from the f32 colours the buffers carry.
fn bytes(c: [f32; 4]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    b(c[0]) | b(c[1]) << 8 | b(c[2]) << 16 | b(c[3]) << 24
}

fn unbytes(c: u32) -> [f32; 4] {
    [0, 8, 16, 24].map(|k| ((c >> k) & 0xff) as f32 / 255.0)
}

fn linear(s: &Sys, c: [f32; 4]) -> [f32; 4] {
    if s.linear { unbytes(crate::particle_render::to_linear(bytes(c))) } else { c }
}

/// The particle's colour and size this frame, from the native module maths in particle_render.
/// `pose` a local system's: its particles' streams are in its local space natively.
fn looks(s: &Sys, p: &Particle, _t: f32) -> ([f32; 4], Vec2) {
    let ps = &s.ps;
    let alive = p.alive;
    let mut color = p.color32;
    if let Some(g) = &ps.color {
        color = crate::particle_render::color_over_lifetime(g, alive, p.seed, color);
    }
    if let Some((g, range)) = &ps.color_by_speed {
        // UpdateTpl reads velocity + animatedVelocity in simulation space, as the particles keep them.
        color = crate::particle_render::color_by_speed(g, *range, (p.velocity + p.animated).to_array(), color);
    }
    (unbytes(color), p.current_size)
}

/// UpdateSizeModules: SizeModule::Update then SizeBySpeedModule::Update (0x1801e1800), which multiplies currentSize
/// when the size module wrote it, else startSize. Axes 1 and 2 only under uses3DSize.
fn size_of(s: &Sys, p: &Particle) -> [f32; 3] {
    let ps = &s.ps;
    let three = uses_3d_size(ps);
    let start = [p.size, p.size_y, p.size_z];
    let mut size = start;
    if let Some(c) = &ps.size {
        for k in 0..if three { 3 } else { 1 } {
            size[k] = crate::particle_render::size_over_lifetime(c, k, p.alive, p.seed, start[k]);
        }
    }
    if let Some((c, range)) = &ps.size_by_speed {
        let v = p.velocity + p.animated;
        let sq = (v.y * v.y + v.z * v.z) + v.x * v.x;
        let d = range[1] - range[0];
        let scale = if d.abs() > f32::from_bits(0x3089705f) { 1.0 / d } else { range[0] };
        let offset = -(range[0] * scale);
        let t = crate::game::particle_sim::minps(1.0, crate::game::particle_sim::maxps(0.0, sq.sqrt() * scale + offset));
        for k in 0..if three { 3 } else { 1 } {
            size[k] = by_speed(c, k, t, p.seed, size[k]);
        }
    }
    if let Some(n) = ps.noise.as_ref().filter(|n| crate::game::particle_noise::uses_impulse(n)) {
        for k in 0..if three { 3 } else { 1 } {
            size[k] = crate::game::particle_noise::size(n, p.seed, p.alive, p.noise_impulse[k], size[k]);
        }
    }
    if !three {
        size[1] = size[0];
        size[2] = size[0];
    }
    size
}

/// SizeBySpeedModule::Update for one axis at the speed's `t`: max(0, curve) * `from`; states 2 and 3 draw from
/// seed + 0xf3857f6f.
fn by_speed(m: &crate::assets::particles::SizeOverLifetime, axis: usize, t: f32, seed: u32, from: f32) -> f32 {
    let c = if m.separate { [&m.x, &m.y, &m.z][axis] } else { &m.x };
    let r = || crate::particle_render::random01(seed.wrapping_add(0xf3857f6f));
    let v = match c.state {
        0 => c.scalar,
        1 | 2 => crate::particle_render::curve_at(c, t, r),
        _ => (c.scalar - c.min_scalar) * r() + c.min_scalar,
    };
    v.max(0.0) * from
}

/// UVModule::PrepareForRender (0x180e2e6d0): per sprite its texture rect times downscaleMultiplier over the
/// texture's data size, 0.5 - its pivot, and its size over the first sprite's width. A sprite on another texture
/// than the first makes every entry the first one's. The texture is the first sprite's.
fn sprite_table(db: &AssetDb, file: usize, list: &[crate::assets::classes::PPtr]) -> (Vec<[f32; 8]>, Option<ObjRef>) {
    let mut table = Vec::new();
    let (mut first, mut same, mut reference) = (-1.0f32, true, None);
    for p in list {
        let sp = db.resolve(file, *p).and_then(|p| Some((p, db.sprite(p).ok()?)));
        let Some((at, sp)) = sp else { unimplemented!("mod-only: a null or unreadable sprite in a texture sheet's list (PrepareForRender skips it)") };
        let Some(texture) = db.resolve(at.file, sp.texture) else { unimplemented!("mod-only: a sprite without a texture in a texture sheet") };
        let t = crate::assets::components::Texture2D::read(db.data(texture)).expect("a sprite's texture");
        let (w, h) = (t.width as f32, t.height as f32);
        let m = sp.downscale_multiplier;
        let [x, y, rw, rh] = sp.texture_rect.map(|v| v * m);
        if first < 0.0 {
            (first, reference) = (rw, Some(texture));
        } else if reference != Some(texture) {
            same = false;
        }
        table.push([x / w, y / h, (x + rw) / w, (rh + y) / h, 0.5 - sp.pivot[0], 0.5 - sp.pivot[1], rw / first, rh / first]);
    }
    if table.is_empty() {
        unimplemented!("mod-only: a sprite-mode texture sheet without sprites (PrepareForRender's default entry)");
    }
    if !same && table.len() > 1 {
        let e = table[0];
        table.fill(e);
    }
    (table, reference)
}

/// The sheet's two frames as (u0, v0, u1, v1), and the blend between them: the native sheetIndex through
/// ParticleGeomAnimateUVs. PrepareForRender (0x1801a94d0) leaves UVs unanimated when the channel mask is 0 or
/// both tile counts are below 2 outside sprite mode.
fn sheet_frames(s: &Sys, p: &Particle, _t: f32) -> ([f32; 4], [f32; 4], f32, Option<usize>) {
    const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    let Some(sheet) = &s.ps.sheet else { return (WHOLE, WHOLE, 0.0, None) };
    if sheet.uv_channel_mask == 0 || (sheet.tiles_x < 2 && sheet.tiles_y < 2 && sheet.mode == 0) {
        return (WHOLE, WHOLE, 0.0, None);
    }
    if sheet.uv_channel_mask & 3 != 3 {
        unimplemented!("mod-only: texture sheet uvChannelMask {}: only UV0 and UV1 both animated (-1) is read", sheet.uv_channel_mask);
    }
    let index = crate::particle_render::sheet_index(sheet, p.alive, p.inv_lifetime, p.seed);
    if sheet.mode == 1 {
        // ParticleGeomAnimateUVs (0x180dbcf90) sprite mode: the entry of floor(frame) and the next one, wrapped to
        // the first at m_NumUVFrame (the sprite count, PrepareForRender 0x1801a94d0).
        let n = s.frames.len() as f32;
        let frame = n * index;
        let whole = crate::particle_render::floor_tt(frame);
        let next = whole + 1.0;
        let next = if n <= next { 0.0 } else { next };
        let uv = |i: f32| {
            let e = s.frames[i as i32 as usize];
            [e[0], e[1], e[2], e[3]]
        };
        return (uv(whole), uv(next), frame - whole, Some(whole as i32 as usize));
    }
    let (a, c, blend) = crate::particle_render::sheet_uvs(sheet.tiles_x, sheet.tiles_y, (sheet.tiles_x * sheet.tiles_y) as f32 * index);
    (a, c, blend, None)
}

/// The billboards, as RenderJobCommon (0x180dbe5c0) lays them out without a mesh: DrawParticlesWithPivot<0> into
/// GenerateParticleGeometry<0,0,0,0> (0x180d9dce0). `width`: Camera::CalculateFarPlaneWorldSpaceLength.
/// Each centre goes through m_WorldMatrix (localToWorld for a local system, the identity for a world one) on the CPU.
fn quads(b: &mut Buffers, s: &Sys, run: &Run, width: f32, cam: &crate::particle_render::CameraMatrices) {
    use crate::particle_render as pr;
    if run.particles.is_empty() {
        return;
    }
    // Vector2f::epsilon (0x1816a1440) = 1e-5; uses3DSize as in looks(); UV module in sprite mode.
    let uses_3d_size = uses_3d_size(&s.ps);
    let with_pivot = 1e-5 < s.pivot.x * s.pivot.x + s.pivot.y * s.pivot.y + s.pivot.z * s.pivot.z || uses_3d_size || s.ps.sheet.as_ref().is_some_and(|sh| sh.mode == 1);
    if let Some(stretch) = s.stretch {
        return stretched(b, s, run, width, cam, stretch, with_pivot);
    }
    // uses3DRotation (AllocateParticleArrays 0x1801b4e70): a 3D start rotation, shape alignToDirection, or the
    // rotation / rotation by speed modules' separate axes.
    if uses_3d_rotation(&s.ps) {
        unimplemented!("mod-only: uses3DRotation: GenerateParticleGeometry<0,0,1,0> (0x180d99890) is not wired yet");
    }
    let unscaled = [run.scale; 3];
    // m_LocalHierarchyScalingMode (0x20000000): scaling Hierarchy in Local simulation space.
    let hierarchy_local = s.ps.scaling == 0 && s.ps.space == 0;
    let (axes, roll) = match s.alignment {
        // View: m_InvViewMatrix rows; roll correction only without allowRoll.
        0 => (pr::scaled_rows(cam.axes, unscaled), if s.allow_roll { 0.0 } else { cam.roll }),
        1 => (pr::scaled_rows([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], unscaled), 0.0),
        // m_WorldMatrix columns as the rows, renderScale replaced by (1, 1, 1) (0x1815ead80). The rewrite keeps the
        // system's world pose, not its Unity parent chain, so the matrix is CalculateGlobalMatrix of that one pose.
        2 if hierarchy_local => {
            let (q, t, k) = (run.rotation, run.position, run.scale);
            let m = crate::unity::transform::local_to_world(&[crate::unity::transform::Trs { t: [t.x, t.y, t.z, 0.0], q: q.to_array(), s: [k, k, k, 0.0] }]);
            ([[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]], 0.0)
        }
        2 => {
            let q = run.rotation;
            (pr::scaled_rows(pr::rotation_rows([q.x, q.y, q.z, q.w]), unscaled), 0.0)
        }
        a => unimplemented!("mod-only: render alignment {a}: GenerateParticleGeometry<0,0,0,3|4> is not rewritten yet"),
    };
    let limits = pr::ortho_size_limits(s.size_range, width, run.scale);
    // PrepareForRender (0x1801a94d0): m_WorldMatrix is localToWorld outside World simulation space, else the identity.
    let world = if s.ps.space == 0 { pose(run, s).l2w } else { [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] };
    for p in &run.particles {
        let t = (p.age / p.lifetime).min(1.0);
        let (color, size) = looks(s, p, t);
        let color = linear(s, color);
        let (mut a, mut c, blend, sprite) = sheet_frames(s, p, t);
        let spin = p.rotation;
        // GenerateParticleGeometry (0x180d9e9a9): ((m2 * z + t) + m1 * y) + m0 * x, lane for lane; the vertices are
        // world space and the batch draws with the identity (RenderMultipleBillboards 0x18019c0e0).
        let v = p.position;
        let at: [f32; 3] = std::array::from_fn(|i| ((world[8 + i] * v.z + world[12 + i]) + world[4 + i] * v.y) + world[i] * v.x);
        // ParticleGeomFlipUVs (0x180dbd5e0): Random01(seed + 0x294fb5c7) < flip.x, Random01(seed + 0xd4cb82) < flip.y.
        if crate::particle_render::random01(p.seed.wrapping_add(0x294fb5c7)) < s.flip.x {
            (a[0], a[2], c[0], c[2]) = (a[2], a[0], c[2], c[0]);
        }
        if crate::particle_render::random01(p.seed.wrapping_add(0x00d4cb82)) < s.flip.y {
            (a[1], a[3], c[1], c[3]) = (a[3], a[1], c[3], c[1]);
        }
        // Without uses3DSize the size stream is read for both axes.
        let (sx, sy) = (size.x, if uses_3d_size { size.y } else { size.x });
        let corners = if with_pivot {
            // GenerateParticleGeometry<0,1,0,0> (0x180d95400): the pivot times the unclamped sizes; in sprite mode
            // (render flag 0x2000000) the sizes scale by the sprite's and its pivot adds on.
            let pv = s.pivot;
            let (mut sx2, mut sy2, mut offset) = (sx, sy, [sx * pv.x, sy * pv.y, sx * pv.z]);
            if let Some(i) = sprite {
                let e = s.frames[i];
                (sx2, sy2) = (sx * e[6], sy * e[7]);
                offset[0] += sx2 * e[4];
                offset[1] += e[5] * sy2;
            }
            let (hx, hy) = pr::clamped_half(sx2, sy2, p.alive, limits);
            pr::pivot_billboard(axes, at, spin + roll, hx, hy, offset)
        } else {
            let (hx, hy) = pr::clamped_half(sx, sy, p.alive, limits);
            pr::billboard(axes, at, spin + roll, hx, hy)
        };
        let first = b.positions.len() as u32;
        // The corner UV table: top-left (0, 1), top-right (1, 1), bottom-right (1, 0), bottom-left (0, 0).
        for (corner, (u, v)) in corners.iter().zip([(0, 3), (2, 3), (2, 1), (0, 1)]) {
            // The corner as GenerateParticleGeometry writes it, z included (the renderer's sorting orders the draws).
            b.positions.push([corner[0], corner[1], corner[2]]);
            b.uvs.push([a[u], a[v]]);
            b.colors.push(color);
            b.uv2.push([c[u], c[v], blend]);
        }
        b.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// Render mode 1, GenerateParticleGeometry<1,0,0,0> (0x180d88a40): the same colours, sheet frames, flips, clamped
/// half sizes and corner UV table as the billboards, the corners from ParticleGeomStretch3D<0> or, with
/// freeformStretching, ParticleGeomStretchFreeform<1>.
fn stretched(b: &mut Buffers, s: &Sys, run: &Run, width: f32, cam: &crate::particle_render::CameraMatrices, stretch: ([f32; 3], bool, bool), with_pivot: bool) {
    use crate::particle_render as pr;
    let ([camera_velocity_scale, velocity_scale, length_scale], freeform, rotate) = stretch;
    if with_pivot {
        unimplemented!("mod-only: stretched billboard with a pivot, 3D size or sprite sheet: GenerateParticleGeometry<1,1,0,0> (0x180d87010)");
    }
    if s.ps.space != 1 {
        unimplemented!("mod-only: stretched billboard outside World simulation space: View * World is not rewritten");
    }
    if camera_velocity_scale != 0.0 {
        unimplemented!("mod-only: stretched billboard cameraVelocityScale: m_CameraVelocityVS/WS are not rewritten");
    }
    let k = run.scale;
    let axes = pr::scaled_rows(cam.axes, [k; 3]);
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let limits = pr::ortho_size_limits(s.size_range, width, k);
    let uses_3d_size = uses_3d_size(&s.ps);
    // cameraVelocityScale * renderScale.x times the camera velocity: zero above.
    let cam_vel = [0.0f32; 3];
    for p in &run.particles {
        let t = (p.age / p.lifetime).min(1.0);
        let (color, size) = looks(s, p, t);
        let color = linear(s, color);
        let (mut a, mut c, blend, _) = sheet_frames(s, p, t);
        if pr::random01(p.seed.wrapping_add(0x294fb5c7)) < s.flip.x {
            (a[0], a[2], c[0], c[2]) = (a[2], a[0], c[2], c[0]);
        }
        if pr::random01(p.seed.wrapping_add(0x00d4cb82)) < s.flip.y {
            (a[1], a[3], c[1], c[3]) = (a[3], a[1], c[3], c[1]);
        }
        let (sx, sy) = (size.x, if uses_3d_size { size.y } else { size.x });
        let q = pr::Stretched { p: p.position.to_array(), vel: (p.animated + p.velocity).to_array(), size_y: sy, half: pr::clamped_half(sx, sy, p.alive, limits), rot: p.rotation };
        let corners = if freeform {
            pr::stretch_freeform(cam, identity, [k; 3], &q, cam_vel, velocity_scale, length_scale * k, rotate)
        } else {
            pr::stretch_3d(cam, axes, &q, cam_vel, velocity_scale, length_scale * k)
        };
        let first = b.positions.len() as u32;
        for (corner, (u, v)) in corners.iter().zip([(0, 3), (2, 3), (2, 1), (0, 1)]) {
            b.positions.push(*corner);
            b.uvs.push([a[u], a[v]]);
            b.colors.push(color);
            b.uv2.push([c[u], c[v], blend]);
        }
        b.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// The particles' trails, ParticleSystemTrailGeometryJob::RenderJobCommon (0x180dbea10) per particle: one strip per
/// system run, its triangles from Destripify as BakeTrailsMesh (0x1801a5060) sets them. Trail colours skip the
/// gamma-to-linear step: the job reads the start colour, not the converted colour stream.
fn trails(b: &mut Buffers, s: &Sys, run: &Run, view: &crate::particle_trails::View) {
    use crate::particle_trails as pt;
    let Some(c) = &s.ps.trail else { return };
    if c.lighting_data {
        unimplemented!("mod-only: trail lighting data: BuildParticleLine<1> (0x180d7cf30)");
    }
    // m_WorldSpace: a world-space trail on a system that does not simulate in world space.
    let job_world = c.world_space && s.ps.space != 1;
    let uses_3d_size = uses_3d_size(&s.ps);
    // TrailInit (0x180dbc8e0): a job that is not m_WorldSpace keeps the simulation-space points and takes
    // m_WorldToCameraMatrix = view * m_WorldMatrix; m_CameraToWorldMatrix stays the view's inverse, so WriteLineVertex
    // writes world vertices and the batch draws with the identity (RenderMultipleBillboards 0x18019c0e0).
    let l2w = (s.ps.space == 0).then(|| pose(run, s).l2w);
    let local_view = l2w.as_ref().filter(|_| !(c.mode == 0 && c.world_space)).map(|m| view.local(m));
    let view = local_view.as_ref().unwrap_or(view);
    // pow(|renderScale.y * x * z|, 1/3) (0x3eaaaaab), renderScale as the billboards take it.
    let k = run.scale;
    let width_scale = crate::unity::crt::powf(((k * k) * k).abs(), f32::from_bits(0x3eaaaaab));
    let mut strip = Vec::new();
    // ConfigurePerParticleTrailParams with the size stream and the colour after the colour modules' UpdateSingle.
    let params = |p: &Particle| {
        let size = looks(s, p, (p.age / p.lifetime).min(1.0)).1;
        let color = c.inherit_particle_color.then(|| {
            let mut color = p.color32;
            if let Some(g) = &s.ps.color {
                color = pt::color_module_single(g, p.seed, p.alive, p.inv_lifetime, color);
            }
            if let Some((g, range)) = &s.ps.color_by_speed {
                color = pt::color_by_speed_single(g, *range, p.seed, (p.velocity + p.animated).to_array(), color);
            }
            color
        });
        (pt::params(c, p.seed, p.alive, p.inv_lifetime, [size.x, size.y], uses_3d_size, color, width_scale), size)
    };
    if c.mode == 1 {
        if c.attach_ribbons {
            unimplemented!("mod-only: attachRibbonsToTransform: the emitter's position as the last ribbon point");
        }
        let positions: Vec<[f32; 3]> = run.particles.iter().map(|p| p.position.to_array()).collect();
        let ages: Vec<f32> = run.particles.iter().map(|p| p.alive / p.inv_lifetime).collect();
        let width = (width_scale * pt::max_width(c, &s.ps.start_size, uses_3d_size)) * c.width_over_trail.scalar;
        // One run holds every parent's sub-emitter particles, as the engine's one system does.
        let seeds: Option<Vec<u32>> = c.split_sub_emitter_ribbons.then(|| run.particles.iter().map(|p| p.parent_seed).collect());
        pt::ribbons(&mut strip, c, &positions, &ages, seeds.as_deref(), width, view, |k| params(&run.particles[k]).0);
    }
    for p in run.particles.iter().filter(|_| c.mode == 0) {
        if p.trail.is_empty() {
            continue;
        }
        let (params, _) = params(p);
        let lifetime = pt::lifetime(c, p.seed, p.alive, p.inv_lifetime, size_stream(&s.ps, p), uses_3d_size);
        // The head: a world-space job's goes through m_LocalToWorldMatrix (0x180dbf10f), when there is one.
        let head = (!job_world || p.alive <= 100.0).then(|| match l2w.as_ref().filter(|_| job_world) {
            Some(m) => pt::to_world(m, p.position.to_array()),
            None => p.position.to_array(),
        });
        // The shadow bias is 0 outside shadow passes (RendererCullingCallbackProperties.shadowPass).
        pt::strip(&mut strip, c, &p.trail, head, run.trail_time, lifetime, &params, view, 0.0);
    }
    let first = b.positions.len() as u32;
    for (w, color, uv) in &strip {
        // WriteLineVertex's position, z included.
        b.positions.push([w[0], w[1], w[2]]);
        b.uvs.push(*uv);
        b.colors.push(unbytes(*color));
        b.uv2.push([0.0; 3]);
    }
    pt::destripify(&mut b.indices, first, strip.len() as u32);
}

#[derive(Default)]
pub struct Library {
    /// The sprites' cached shape meshes (ShapeModule::UpdateCachedMesh).
    sprite_meshes: HashMap<ObjRef, Option<std::sync::Arc<crate::game::particle_shape::Mesh>>>,
    systems: Vec<Sys>,
    /// By the system's GameObject.
    index: HashMap<ObjRef, usize>,
    /// A system with a script's shape.texture, by the system and the texture.
    textured: HashMap<(usize, ObjRef), usize>,
    /// By prefab root: empty for a prefab that moves, whose systems are not followed yet.
    prefabs: HashMap<ObjRef, Vec<Part>>,
    /// By prefab root: its NukeShockwaveBehaviours.
    shockwaves: HashMap<ObjRef, Vec<crate::game::particle_shockwave::Template>>,
}

struct Gpu<'a> {
    materials: &'a mut Assets<ParticleMaterial>,
    meshes: &'a mut Assets<Mesh>,
    images: &'a mut Assets<Image>,
    textures: &'a mut Textures,
}

/// A Particles/Standard Unlit or Unlit/AlphaCutoff material's settings: `st` is _MainTex_ST.
pub(crate) fn particle_material(m: &Material, st: [f32; 4], texture: Handle<Image>, blend_frames: bool) -> ParticleMaterial {
    let color = m.color("_Color").unwrap_or([1.0; 4]);
    let kind = if m.float("_AlphaCutoff").is_some() {
        3
    } else {
        match m.float("_ColorMode").unwrap_or(0.0) as u32 {
            0 => 0,
            4 => 2,
            _ => 1,
        }
    };
    let add = m.color("_ColorAddSubDiff").unwrap_or([1.0, 0.0, 0.0, 0.0]);
    let mode = m.float("_Mode").unwrap_or(2.0) as u32;
    let settings = Settings {
        color: Vec4::new(to_linear(color[0]), to_linear(color[1]), to_linear(color[2]), color[3]),
        st: Vec4::from(st),
        color_add: Vec2::new(add[0], add[1]),
        cutoff: m.float("_AlphaCutoff").or(m.float("_Cutoff")).unwrap_or(0.5),
        kind,
        mode,
        blend_frames: blend_frames as u32,
    };
    ParticleMaterial { settings, texture, subtract: mode == 5, vertices: PULLED_VERTICES, pulled: false }
}

#[allow(clippy::too_many_arguments)]
fn output(db: &AssetDb, file: usize, list: &[crate::assets::classes::PPtr], at: usize, key: (i32, i16, f32, usize), gpu: &mut Gpu, blend_streams: bool, sheet_texture: Option<ObjRef>) -> Option<Output> {
    let r = db.resolve(file, *list.get(at)?)?;
    let key = (key.0, key.1, render_queue(db, Some(r)), key.2, key.3);
    let m = Material::read(db.data(r)).unwrap();
    let (_, t, st) = m.textures.iter().find(|(n, ..)| n == "_MainTex")?;
    // A sprite sheet draws from its sprites' texture, whatever _MainTex holds.
    let source = sheet_texture.or_else(|| db.resolve(r.file, *t))?;
    let texture = load_texture(db, gpu.images, gpu.textures, source)?.0;
    let blend_frames = blend_streams && m.float("_FlipbookMode") == Some(1.0);
    let material = gpu.materials.add(ParticleMaterial { pulled: true, ..particle_material(&m, *st, texture, blend_frames) });
    Some(Output { material, key })
}

/// A system's ParticleSystemRenderer meshes: the billboards (none when it draws no quads) and the trails.
fn renderer_outputs(db: &AssetDb, go: ObjRef, s: &Sys, gpu: &mut Gpu) -> [Option<Output>; 2] {
    let of = |class: i32| db.game_object(go).ok()?.components.into_iter().filter_map(|c| db.resolve(go.file, c)).find(|c| db.class(*c) == class);
    let Some(r) = of(199).map(|c| ParticleRenderer::read(db.data(c)).unwrap()) else { return [None, None] };
    // ParticleSystemVertexStream.AnimBlend: the second frame's UV2 comes with it.
    let streams = r.custom_streams && r.vertex_streams.contains(&8);
    let quads = if !r.enabled || r.render_mode == 5 || r.mask_interaction == 1 { None } else { output(db, go.file, &r.materials, 0, (r.layer, r.order, r.sorting_fudge, 0), gpu, streams, s.sheet_texture) };
    let trails = s.ps.trail.as_ref().filter(|_| r.enabled).and_then(|_| output(db, go.file, &r.materials, r.materials.len() - 1, (r.layer, r.order, r.sorting_fudge, 1), gpu, false, None));
    [quads, trails]
}

/// ShapeModule::AcquireData's m_TextureColorData (0x180df9ed0): GetPixels32 of mip 0 (rows bottom first).
fn shape_pixels(db: &AssetDb, file: usize, ps: &ParticleSystem) -> (Vec<u32>, i32, i32) {
    texture_pixels(db, db.resolve(file, ps.shape_texture_ref).expect("the shape's texture"))
}

fn texture_pixels(db: &AssetDb, t: ObjRef) -> (Vec<u32>, i32, i32) {
    let t = crate::assets::components::Texture2D::read(db.data(t)).expect("the shape's texture");
    if !t.readable {
        unimplemented!("mod-only: a shape texture without Read/Write ({}): GetPixels32 fails and the native samples an empty array", t.name);
    }
    let px = t.rgba(&db.data_dir).expect("the shape's texture pixels");
    (px.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(), t.width, t.height)
}

impl Library {
    /// System `sys` with shape.texture = `texture` (the setter's AcquireData: the new texture's pixels).
    fn textured(&mut self, db: &AssetDb, sys: usize, texture: ObjRef) -> usize {
        if let Some(&i) = self.textured.get(&(sys, texture)) {
            return i;
        }
        let mut s = self.systems[sys].clone();
        if s.ps.shape_type != 18 {
            unimplemented!("mod-only: shape.texture set on a shape of type {} (only a Rectangle samples it here)", s.ps.shape_type);
        }
        s.ps.shape_texture = true;
        s.ps.shape_pixels = Some(std::sync::Arc::new(texture_pixels(db, texture)));
        self.systems.push(s);
        self.textured.insert((sys, texture), self.systems.len() - 1);
        self.systems.len() - 1
    }

    /// The system on `go`; its sub-emitters with it.
    fn system(&mut self, db: &AssetDb, go: ObjRef, rotation: Quat) -> Option<usize> {
        if let Some(&i) = self.index.get(&go) {
            return Some(i);
        }
        let of = |class: i32| db.game_object(go).ok()?.components.into_iter().filter_map(|c| db.resolve(go.file, c)).find(|c| db.class(*c) == class);
        let order = db.load_order().ok().and_then(|o| o.get(&of(198)?).copied()).unwrap_or(i32::MAX);
        let mut ps = ParticleSystem::read(db.data(of(198)?)).unwrap();
        if ps.shape_texture {
            ps.shape_pixels = Some(std::sync::Arc::new(shape_pixels(db, go.file, &ps)));
        }
        let r = ParticleRenderer::read(db.data(of(199)?)).unwrap();
        let first_cull = match ps.culling_mode {
            _ if !r.enabled => Cull::Never,
            1 | 2 => Cull::Always,
            0 if ps.looping && ps.supports_procedural() => Cull::Procedural,
            _ => Cull::Never,
        };
        if !matches!(r.render_mode, 0 | 1 | 5) {
            unimplemented!("mod-only: ParticleSystemRenderer: render mode {}", r.render_mode);
        }
        let (frames, sheet_texture) = ps.sheet.as_ref().filter(|s| s.mode == 1).map_or((Vec::new(), None), |sheet| sprite_table(db, go.file, &sheet.sprites));
        let i = self.systems.len();
        self.index.insert(go, i);
        let subs = ps.subs.clone();
        let (pivot, flip) = (Vec3::from(r.pivot), Vec2::new(r.flip[0], r.flip[1]));
        let game_layer = db.game_object(go).ok()?.layer as i32;
        self.systems.push(Sys { ps, subs: Vec::new(), rotation, prefab: None, local_rotation: Quat::IDENTITY, game_layer, fancy_only: false, awake: true, hierarchy: 1.0, local: 1.0, size_range: r.size_range, alignment: r.alignment, allow_roll: r.allow_roll, stretch: (r.render_mode == 1).then_some((r.stretch, r.freeform, r.rotate_with_stretch)), pivot, flip, linear: r.apply_active_color_space, frames, go: Some(go), sheet_texture, max_step: crate::game::particle_clock::maximum_particle_delta_time(db).unwrap(), order, first_cull });
        let subs: Vec<_> = subs.iter().filter_map(|sub| {
            let c = db.resolve(go.file, sub.emitter)?;
            let child = db.resolve(c.file, crate::assets::components::game_object_of(db.data(c)).ok()?)?;
            Some((self.system(db, child, Quat::IDENTITY)?, sub.kind, sub.properties, sub.probability))
        }).collect();
        self.systems[i].subs = subs;
        Some(i)
    }

    fn parts(&mut self, db: &mut AssetDb, source: ObjRef, subtree: Option<ObjRef>) -> Vec<Part> {
        if let Some(found) = self.prefabs.get(&subtree.unwrap_or(source)) {
            return found.clone();
        }
        let prefab = db.prefab(source).unwrap_or_default();
        let moves = prefab.objects.iter().any(|o| o.components.iter().any(|(_, c)| matches!(c, Component::Rigidbody2D(_))));
        let mut poses: Vec<(Vec3, Quat, f32, bool)> = Vec::new();
        let mut parts = Vec::new();
        let mut sub = std::collections::HashSet::new();
        let mut inside = Vec::new();
        for (o, ob) in prefab.objects.iter().enumerate() {
            inside.push(subtree.is_none_or(|s| s == ob.game_object) || ob.parent.is_some_and(|p| inside[p]));
            let (mut position, mut rotation, mut scale) = (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            if let Some(Component::Transform(t)) = ob.components.first().map(|c| &c.1) {
                position = Vec3::from(t.local_position);
                rotation = Quat::from_array(t.local_rotation);
                scale = Vec3::from(t.local_scale);
            }
            // The clone's root is at the spawn's pose, wherever the prefab was saved.
            if ob.parent.is_none() {
                (position, rotation) = (Vec3::ZERO, Quat::IDENTITY);
            }
            let parent = ob.parent.map_or((Vec3::ZERO, Quat::IDENTITY, 1.0, true), |p| poses[p]);
            let (world, turned, hierarchy, active) = (parent.0 + parent.1 * (position * parent.2), parent.1 * rotation, parent.2 * scale.x, parent.3 && ob.active);
            poses.push((world, turned, hierarchy, active));
            if moves || !active || !inside[inside.len() - 1] || !ob.components.iter().any(|(_, c)| matches!(c, Component::Other(198))) {
                continue;
            }
            if let Some(i) = self.system(db, ob.game_object, turned) {
                sub.extend(self.systems[i].subs.iter().map(|s| s.0));
                (self.systems[i].hierarchy, self.systems[i].local) = (hierarchy, scale.x);
                (self.systems[i].prefab, self.systems[i].local_rotation) = (Some((world, turned)), rotation);
                self.systems[i].fancy_only = ob.components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class == "ActivateIfFancyEffects"));
                let chain: Vec<usize> = std::iter::successors(Some(o), |&x| prefab.objects[x].parent).collect();
                let names = chain.iter().map(|&x| prefab.objects[x].name.clone()).collect();
                parts.push(Part { sys: i, chain, names, position: world, rotation: turned, hierarchy, local: scale.x });
            }
        }
        parts.retain(|p| !sub.contains(&p.sys) && (subtree.is_some() || self.systems[p.sys].ps.play_on_awake));
        self.prefabs.insert(subtree.unwrap_or(source), parts.clone());
        parts
    }
}

/// BloodImpactBehaviour.ParticleSystemsToAdjust: the GameObjects of the systems SetColor reaches.
fn adjusted(db: &mut AssetDb, source: ObjRef) -> Vec<ObjRef> {
    let Ok(prefab) = db.prefab(source) else { return Vec::new() };
    let mut out = Vec::new();
    for ob in &prefab.objects {
        for (_, c) in &ob.components {
            if let Component::Script { class, fields, .. } = c {
                if class == "BloodImpactBehaviour" {
                    for v in fields.get("ParticleSystemsToAdjust").and_then(|v| v.array()).into_iter().flatten() {
                        let Some(r) = v.pptr().and_then(|p| db.resolve(source.file, p)) else { continue };
                        if let Ok(go) = crate::assets::components::game_object_of(db.data(r)) {
                            out.extend(db.resolve(r.file, go));
                        }
                    }
                }
            }
        }
    }
    out
}

/// The NukeShockwaveBehaviours of the clones `effect_log[from..]` made, as they are instantiated (the clone's group
/// comes with the particles' update, `log` until then).
fn spawn_shockwaves(library: &mut Library, shockwaves: &mut Vec<crate::game::particle_shockwave::Shockwave>, db: &mut AssetDb, log: &[crate::game::EffectSpawn], from: usize) {
    for (j, e) in log.iter().enumerate().skip(from) {
        if e.subtree.is_some() || library.parts(db, e.prefab, e.subtree).is_empty() {
            continue;
        }
        let t = match library.shockwaves.get(&e.prefab) {
            Some(t) => t.clone(),
            None => {
                let prefab = db.prefab(e.prefab).unwrap_or_default();
                let t = crate::game::particle_shockwave::templates(db, e.prefab, &prefab);
                library.shockwaves.insert(e.prefab, t.clone());
                t
            }
        };
        for t in t {
            let mut sw = crate::game::particle_shockwave::Shockwave::new(t, 0, Vec3::from(e.position), Quat::from_array(e.rotation), e.scale);
            sw.log = Some(j);
            shockwaves.push(sw);
        }
    }
}

/// The NukeShockwave clones' MonoBehaviour messages: `Start` (with the pending Starts: the clones made since), each
/// fixed step's FixedUpdate and the frame's Update.
#[derive(Clone, Copy, PartialEq)]
pub enum ShockwavePhase {
    Start,
    Fixed,
    Update,
}

pub fn shockwaves_message(g: &mut Game, phase: ShockwavePhase) {
    let Some(mut db) = g.assets.take() else { return };
    let mut w = g.particle_world.take().unwrap_or_default();
    let World { library, shockwaves, shockwave_scan, .. } = &mut *w;
    if phase == ShockwavePhase::Start {
        spawn_shockwaves(library, shockwaves, &mut db, &g.effect_log, *shockwave_scan);
        *shockwave_scan = g.effect_log.len();
    }
    for sw in shockwaves.iter_mut() {
        match phase {
            ShockwavePhase::Start => sw.start(g),
            ShockwavePhase::Fixed => sw.fixed_update(g),
            ShockwavePhase::Update => sw.update(g.time.deltaTime),
        }
    }
    g.particle_world = Some(w);
    g.assets = Some(db);
}

/// SetActive(false) of a pooled clone's child (its script's Start): ParticleSystem::Deactivate (1801b8610) clears it
/// and RemoveFromManager; it plays no more (children before parents, as ActivateAwakeRecursivelyInternal).
fn child_off(runs: &mut Vec<Run>, manager: &mut Manager, p: crate::game::Pooled, name: &str) {
    let mut i = runs.len();
    while i > 0 {
        i -= 1;
        if runs[i].pooled == Some(p) && runs[i].names.iter().any(|n| n == name) {
            manager.forget(&runs.remove(i));
        }
    }
}

fn play_effect(library: &mut Library, runs: &mut Vec<Run>, db: &mut AssetDb, e: &crate::game::EffectSpawn, manager: &mut Manager) -> Option<u32> {
    let parts = library.parts(db, e.prefab, e.subtree);
    if parts.is_empty() {
        return None;
    }
    manager.groups += 1;
    let group = manager.groups;
    let tinted = if e.color.is_some() { adjusted(db, e.prefab) } else { Vec::new() };
    let (root, turn) = (Vec3::from(e.position), Quat::from_array(e.rotation));
    let water = e.water.as_ref().map(|w| {
        let sizes = w.sizes.iter().filter_map(|(go, size)| library.index.get(go).map(|&sys| (sys, *size))).collect();
        std::sync::Arc::new((w.radius, w.speed, sizes))
    });
    let playing: Vec<_> = parts.iter().filter(|part| !part.names.iter().any(|n| e.off.contains(&n.as_str()))).map(|p| p.sys).collect();
    // Instantiate awakes the clone's systems by original instance id; a pool's SetActive(true) of a made clone
    // activates them through the hierarchy (start's order).
    // ponytail: a pool Clear (300 s idle) remakes clones under the same index; they would count as reactivated.
    let made = e.pooled.is_some_and(|p| !manager.pooled.insert(p));
    // A made clone's reactivation: each played root's Play (needRestart, no particles) runs ResetSeeds, its own draw then
    // one per sub-emitter (1801bf4fe); a sub's Play only joins the list, its isSubEmitter byte (+0x20) still set from
    // its last update (BeginUpdate 1801b5906 clears it only for listed systems). So one draw per system, root first:
    // start_with's own order. A sub-emitter's sub-emitters would keep their last seeds.
    if made && playing.iter().any(|&s| sub_systems(&library.systems[s]).iter().any(|&c| !library.systems[c].subs.is_empty())) {
        unimplemented!("mod-only: a pooled clone's sub-emitter with sub-emitters of its own (ResetSeeds does not reseed them)");
    }
    let mut pre = if made { HashMap::new() } else { awake_order(&playing, &library.systems, manager) };
    for part in parts {
        // A child the clone's scripts deactivated (activeSelf false) does not play.
        if part.names.iter().any(|n| e.off.contains(&n.as_str())) {
            continue;
        }
        let ps = &library.systems[part.sys].ps;
        // ParticleSystemScalingMode: Hierarchy, Local, Shape.
        let scale = match ps.scaling {
            0 => part.hierarchy * e.scale,
            1 => if part.hierarchy == part.local { part.local * e.scale } else { part.local },
            _ => 1.0,
        };
        let mut run = start_with(part.sys, root + turn * (part.position * e.scale), turn * part.rotation, scale, &library.systems, manager, &mut pre);
        run.root = e.root;
        run.parent = e.parent.map(|p| (p, part.position, part.rotation, e.scale));
        (run.group, run.chain, run.pooled) = (group, part.chain.clone(), e.pooled);
        run.names = part.names.clone();
        run.clone_scale = e.scale;
        run.water = water.clone();
        if water.is_some() {
            written(&mut run);
        }
        if e.tint_all || tinted.iter().any(|go| library.index.get(go) == Some(&part.sys)) {
            run.color = e.color;
            if e.color.is_some() {
                written(&mut run);
            }
        }
        runs.push(run);
    }
    Some(group)
}

/// PhysicalBehaviour's Setup*Prefab resources, in the order of its dirty flags.
const ATTACHED: [&str; 4] = ["Prefabs/FireParticle", "Prefabs/BubbleVFX", "Prefabs/ChargedParticles", "Prefabs/SmokeParticle"];

fn attached_flags(p: &crate::game::physical::Physical) -> [bool; 4] {
    [p.dirtyFire, p.dirtyBubble, p.dirtyCharged, p.dirtySmoke]
}

/// One prefab clone under a Physical: its runs with their local offset and turn, and the Physical's last pose.
#[derive(Default)]
pub struct Attached {
    runs: Vec<(Run, Vec3, Quat)>,
    pose: Option<(Vec3, Quat)>,
}

/// ShapeModule.spriteRenderer = a Physical's SpriteRenderer: its Transform and its sprite's mesh.
fn sprite_renderer(g: &crate::game::Game, db: &AssetDb, library: &mut Library, p: usize) -> (Renderer, Option<std::sync::Arc<crate::game::particle_shape::Mesh>>) {
    let ph = &g.physicals[p];
    let mesh = ph.sprite.and_then(|(sprite, ..)| {
        library.sprite_meshes.entry(sprite).or_insert_with(|| {
            let s = db.sprite(sprite).ok()?;
            assert!(s.sub_meshes.len() == 1 && s.sub_meshes[0].topology == 0 && s.sub_meshes[0].first_byte == 0 && s.sub_meshes[0].base_vertex == 0, "a sprite mesh other than one triangle list: {:?}", s.sub_meshes);
            let (positions, _, indices) = s.mesh();
            crate::game::particle_shape::Mesh::new(positions, &indices).map(std::sync::Arc::new)
        }).clone()
    });
    (Renderer { l2w: g.local_to_world(p), position: ph.transform.position, rotation: ph.transform.rotation, lossy: g.lossy_scale(p) }, mesh)
}

/// A Physical's Setup*Prefab systems: the transform follows it, ManagedUpdate writes their modules.
#[allow(clippy::too_many_arguments)]
fn attached_systems(
    g: &mut crate::game::Game,
    db: &mut AssetDb,
    library: &mut Library,
    attached: &mut HashMap<(usize, usize), Attached>,
    dt: f32,
    rng: &mut Rng,
    manager: &mut Manager,
) {
    let usable = |p: &crate::game::physical::Physical| !p.destroyed && p.active;
    attached.retain(|&(p, k), a| {
        let keep = g.physicals.get(p).is_some_and(|ph| usable(ph) && attached_flags(ph)[k]);
        if !keep {
            a.runs.iter().for_each(|r| manager.forget(&r.0));
        }
        keep
    });
    for p in 0..g.physicals.len() {
        let ph = &g.physicals[p];
        let flags = attached_flags(ph);
        if !usable(ph) || !flags.contains(&true) {
            continue;
        }
        let position = Vec3::from(ph.transform.position);
        let turn = Quat::from_array(ph.transform.rotation);
        let lossy = Vec3::from(g.transform_point3(p, [1.0, 0.0, 0.0])).distance(position);
        let (renderer, mesh) = sprite_renderer(g, db, library, p);
        let ph = &g.physicals[p];
        let flammability = g.properties[ph.Properties].Flammability;
        let area = ph.sprite_bounds.map_or(0.0, |_| {
            let e = g.sprite_world_bounds(p).1;
            ((e[0] * e[0] + e[1] * e[1] + e[2] * e[2]) * 4.0).sqrt()
        });
        let burn = (1.0 - ph.BurnProgress) * ph.burnIntensity * 2.0 * area;
        let grey = burn.clamp(0.0, 1.0);
        let charge = ph.Charge();
        let charge_on = !ph.ForceNoCharge && !ph.ForceNoChargeParticles;
        let rate = |v: f32| Emission { enabled: true, rate: charge_on.then_some(v), distance: None, size: None };
        let wet = ph.Wetness > 0.1;
        let writes = [
            Emission { enabled: !ph.has_fire_map && ph.burnIntensity >= 0.05, rate: Some(12.0 * area), distance: Some(area), size: Some((burn * 2.0).min(2.5)) },
            Emission { enabled: true, rate: None, distance: Some(ph.Wetness * 2.0), size: None },
            rate((charge * 2.0).min(30.0)),
            rate((charge * flammability).min(10.0)),
        ];
        for k in 0..ATTACHED.len() {
            if !flags[k] {
                continue;
            }
            let clone = match attached.entry((p, k)) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let Some(source) = db.resource(ATTACHED[k], 1).ok().flatten() else { continue };
                    let mut runs = Vec::new();
                    for part in library.parts(db, source, None) {
                        let ps = &library.systems[part.sys].ps;
                        let scale = match ps.scaling {
                            0 => part.hierarchy * lossy,
                            1 => part.local,
                            _ => 1.0,
                        };
                        let mut run = start(part.sys, position, turn, scale, &library.systems, manager);
                        run.clone_scale = lossy;
                        budget_new(&mut run, &library.systems, g, false);
                        runs.push((run, part.position, part.rotation));
                    }
                    e.insert(Attached { runs, pose: None })
                }
            };
            clone.pose = Some((position, turn));
            for (run, offset, rotation) in &mut clone.runs {
                let ps = &library.systems[run.sys].ps;
                run.position = position + turn * (*offset * lossy);
                run.rotation = turn * *rotation;
                if ps.shape_type == 20 {
                    run.emission = Some(writes[k]);
                    written(run);
                    run.renderer = Some(renderer.clone());
                    if let Some(st) = run.shape.as_mut() {
                        st.mesh = mesh.clone();
                    }
                    if k == 0 {
                        run.color = Some([grey, grey, grey, 1.0]);
                    }
                    if k == 1 {
                        run.stopped = !wet;
                    }
                }
                if k == 0 && ps.shape_type == 20 {
                    for (_, at, size, color) in g.fire_emits.iter().filter(|e| e.0 == p) {
                        let e = EmitParams { position: Some(Vec3::from(*at)), start_size: Some(*size), start_color: Some(*color), ..Default::default() };
                        emit_external(run, &library.systems, g, &e, rng);
                    }
                }
                if k == 3 && ps.shape_type == 20 {
                    for &(_, n) in g.smoke_emits.iter().filter(|e| e.0 == p) {
                        emit_count(run, &library.systems, g, n, rng);
                    }
                }
                step(run, &library.systems, None, g, dt, rng);
            }
        }
    }
    g.fire_emits.clear();
    g.smoke_emits.clear();
}

/// The ParticleSystems scripts play inside their prefab (FireworksBehaviour, GorseDroolingBehaviour): the system follows
/// its Transform and goes with its owner.
#[allow(clippy::too_many_arguments)]
fn embedded_systems(
    g: &crate::game::Game,
    db: &AssetDb,
    library: &mut Library,
    embedded: &mut HashMap<usize, (Run, Option<(Vec3, Quat)>, u32)>,
    calls: Vec<(usize, crate::game::PsCall)>,
    dt: f32,
    rng: &mut Rng,
    manager: &mut Manager,
) {
    use crate::game::{EmbeddedDrive, PsCall};
    // The scripts' Play, Stop and Emit calls since the last update, in call order: each acts on the system at once
    // (manager.rand draws and list order follow the calls), Emit at the pose its caller saw.
    for (i, call) in calls {
        let e = &g.embedded_ps[i];
        if g.destroyed_components.contains(&e.owner) {
            continue;
        }
        let Some(sys) = library.system(db, e.go, Quat::IDENTITY) else { continue };
        let on_awake = library.systems[sys].ps.play_on_awake;
        match call {
            // A system not updated yet: its Awake's Play (playOnAwake) or this one creates it; a Play after Awake's
            // restarts it (ResetSeeds again).
            PsCall::Play => match embedded.get_mut(&i) {
                Some((run, ..)) => play(run, &library.systems, manager),
                None => {
                    let mut run = embedded_start(g, library, i, sys, manager);
                    if on_awake {
                        play(&mut run, &library.systems, manager);
                    }
                    embedded.insert(i, (run, None, 0));
                }
            },
            PsCall::Writes(w) => {
                if !embedded.contains_key(&i) {
                    let mut run = embedded_start(g, library, i, sys, manager);
                    // Never played (playOnAwake off): the writes wait for its Play.
                    run.stopped = !on_awake;
                    embedded.insert(i, (run, None, 0));
                }
                let run = &mut embedded.get_mut(&i).unwrap().0;
                let o = &mut run.writes;
                (o.rate, o.distance, o.speed) = (w.rate.or(o.rate), w.distance.or(o.distance), w.speed.or(o.speed));
                if w.color.is_some() {
                    run.color = w.color;
                }
                written(run);
            }
            // Stop (1801c4830): needRestart, stopEmitting, stopTime; Clear when there is no particle (only its
            // UpdateBounds shows). A playOnAwake system not updated yet was played by its Awake: it exists, now stopped.
            PsCall::Stop(now) => {
                if !embedded.contains_key(&i) && on_awake {
                    let run = embedded_start(g, library, i, sys, manager);
                    embedded.insert(i, (run, None, 0));
                }
                if let Some((run, ..)) = embedded.get_mut(&i) {
                    run.stopped = true;
                    run.was_stopped = true;
                    run.stop_time = now;
                    // Clear: cullTime and stopTime reset; its tail call UpdateBounds.
                    if run.particles.is_empty() {
                        (run.cull_time, run.stop_time) = (0.0, 0.0);
                        update_bounds(run, &library.systems[run.sys], g);
                    }
                }
            }
            PsCall::Emit(..) | PsCall::EmitParams(..) => {
                if !embedded.contains_key(&i) {
                    let mut run = embedded_start(g, library, i, sys, manager);
                    // Never played (playOnAwake off): KeepUpdating's stopEmitting.
                    run.stopped = !on_awake;
                    embedded.insert(i, (run, None, 0));
                }
                let run = &mut embedded.get_mut(&i).unwrap().0;
                // Emit (1801b89f0): KeepUpdating (1801bcc10) wakes a kStopped system, Update0 takes the transform as
                // it is now, StartParticles, then localToWorld, worldToLocal, the rotation and scales are put back.
                if run.halted {
                    keep_updating(run, &library.systems, manager);
                }
                let (position, rotation) = match call {
                    PsCall::Emit(_, p, q) | PsCall::EmitParams(_, p, q) => (Vec3::from(p), Quat::from_array(q)),
                    _ => unreachable!(),
                };
                let kept = (run.position, run.rotation);
                (run.position, run.rotation) = (position, rotation);
                match call {
                    PsCall::Emit(n, ..) => emit_count(run, &library.systems, g, n, rng),
                    PsCall::EmitParams(p, ..) => emit_external(run, &library.systems, g, &p, rng),
                    _ => unreachable!(),
                }
                (run.position, run.rotation) = kept;
            }
        }
    }
    for (i, e) in g.embedded_ps.iter().enumerate() {
        // Destroy(owner's object): the system goes with it.
        if g.destroyed_components.contains(&e.owner) {
            if let Some((run, ..)) = embedded.remove(&i) {
                manager.forget(&run);
            }
            continue;
        }
        if !e.awake {
            continue;
        }
        let Some(sys) = library.system(db, e.go, Quat::IDENTITY) else { continue };
        let sys = e.shape_texture.map_or(sys, |t| library.textured(db, sys, t));
        let on_awake = library.systems[sys].ps.play_on_awake;
        // emission.enabled = isAlive from the 31st FixedUpdate on.
        let emitting = match e.drive {
            EmbeddedDrive::Gorse(k) => g.gorse_droolings[k].emission_set.then_some(g.gorse_droolings[k].isAlive),
            // gapC r11: a script's emission.enabled write (ps_set_emission).
            _ => e.emission,
        };
        // EndUpdateAll took a stopped system out of activeEmitters once it died (kStopped): BeginUpdate (1801b5760)
        // never steps it again, so its clock holds until a Play or an Emit.
        if embedded.get(&i).is_some_and(|(run, ..)| run.halted) {
            continue;
        }
        // A playOnAwake system no call reached starts with its first update.
        if !embedded.contains_key(&i) {
            if !on_awake {
                continue;
            }
            let run = embedded_start(g, library, i, sys, manager);
            embedded.insert(i, (run, None, 0));
        }
        let (position, turn, _) = g.embedded_pose(i);
        let (position, turn) = (Vec3::from(position), Quat::from_array(turn));
        let (run, pose, _) = embedded.get_mut(&i).unwrap();
        *pose = Some((position, turn));
        run.position = position;
        run.rotation = turn;
        run.emission = emitting.map(|enabled| Emission { enabled, rate: None, distance: None, size: None });
        if emitting.is_some() {
            written(run);
        }
        step(run, &library.systems, None, g, dt, rng);
    }
}

/// An embedded system's first Play: its pose and scale now, AddToManager, the ray budget.
fn embedded_start(g: &Game, library: &Library, i: usize, sys: usize, manager: &mut Manager) -> Run {
    let e = &g.embedded_ps[i];
    let (position, turn, lossy) = g.embedded_pose(i);
    let (position, turn) = (Vec3::from(position), Quat::from_array(turn));
    let ps = &library.systems[sys].ps;
    let own = match (e.chain.first(), e.anchor) {
        (Some(n), _) => n.s[0],
        (None, crate::game::BareAnchor::Physical(p)) => g.physicals[p].transform.localScale[0],
        (None, _) => lossy[0],
    };
    let scale = match ps.scaling {
        0 => lossy[0],
        1 => own,
        _ => 1.0,
    };
    let mut run = start(sys, position, turn, scale, &library.systems, manager);
    // The owner's clone (its root Transform): ExternalForcesModule's influence list names objects of that clone (RayCannon's
    // ChargeUp/Impact/Muzzle systems played since gapB r23).
    run.root = g.component_at(e.owner).map(|(inst, ..)| inst.objects[0].components[0]);
    run.clone_scale = lossy[0];
    budget_new(&mut run, &library.systems, g, false);
    run
}

/// ResetSeeds (1801bf2e0): randomSeed (a manager.rand draw when autoRandomSeed) and the module randoms of the system,
/// then the same for each of its sub-emitters (SubModule's list, inline: their own sub-emitters are not reseeded).
fn reset_seeds(run: &mut Run, systems: &[Sys], manager: &mut Manager) {
    fn one(run: &mut Run, systems: &[Sys], manager: &mut Manager) {
        let seeds = crate::game::particle_rand::Seeds::reset(&systems[run.sys].ps, &mut manager.rand);
        run.emitted = Some(crate::game::particle_sim::Emission::new(seeds.emission.clone()));
        run.shape = Some(crate::game::particle_shape::State::new(seeds.random_seed));
        run.collision = Some(crate::game::particle_collision::State::new(seeds.random_seed));
        run.seeds = Some(seeds);
    }
    one(run, systems, manager);
    for child in &mut run.children {
        one(child, systems, manager);
    }
}

/// ParticleSystem::Play (1801bd4e0) on a system that exists. needRestart is never cleared once set (the ctor sets it;
/// only Pause and Simulate clear it), so every Play restarts: kPlaying, ResetSeeds when there is no particle, t,
/// numLoops and accumulatedDt to 0, CalculateStartDelay; then stopEmitting false and AddToManager (appended unless
/// listed; out of inactiveEmittersWithStopActions).
/// A scripting module setter (e.g. EmissionModule set_rateOverTimeMultiplier, 0x180367b70): after the write,
/// `if (!IsStopped()) m_State->invalidateProcedural = true`.
fn written(run: &mut Run) {
    run.invalid |= !run.halted;
}

fn play(run: &mut Run, systems: &[Sys], manager: &mut Manager) {
    let ps = &systems[run.sys].ps;
    if run.particles.is_empty() {
        reset_seeds(run, systems, manager);
    }
    let delay = run.seeds.as_ref().expect("a played system's seeds").start_delay(&ps.start_delay);
    run.clock = crate::game::particle_clock::Clock::play(&crate::game::particle_clock::Spec::of(ps, delay));
    run.stopped = false;
    run.halted = false;
    run.invalid = false;
    if let Some(k) = manager.inactive.iter().position(|&x| x == run.id) {
        manager.inactive.swap_remove(k);
    }
    if !manager.active.contains(&run.id) {
        manager.active.push(run.id);
    }
}

/// ScheduleUpdateJobsHelper (1801bf680, BeginUpdate's per-root step) on an active root: CollectSubEmittersRecursive
/// (1801b7130) gathers its sub systems depth first (CacheSubEmitters 1801df1c0 takes GetSubEmitterPtrs 1801ff200's
/// lists by type: births, then deaths), then KeepUpdating (1801bcc10) sets each kStopped one kPlaying with
/// stopEmitting and no ResetSeeds (1801bccf0 set its isSubEmitter byte, +0x20, which skips it). Its update job runs
/// with the root's; `out` takes the ones that join the manager. An empty one leaves again at the frame's EndUpdateAll.
fn keep_updating_subs(run: &mut Run, systems: &[Sys], out: &mut Vec<u32>) {
    let s = &systems[run.sys];
    let mut order = Vec::new();
    for t in [0, 2] {
        for sub in s.subs.iter().filter(|x| x.1 == t) {
            let c = child_index(s, sub.0);
            if !order.contains(&c) {
                order.push(c);
            }
        }
    }
    for c in order {
        let child = &mut run.children[c];
        if child.halted {
            child.halted = false;
            child.clock.stop_emitting = true;
            out.push(child.id);
        }
        keep_updating_subs(child, systems, out);
    }
}

/// ParticleSystem::KeepUpdating (1801bcc10) on a kStopped system: kPlaying, stopEmitting, ResetSeeds (1801bf2e0;
/// needRestart is still set by the ctor, Stop or a non-looping run's end, and only Pause/Simulate clear it), then
/// AddToManager (appended; out of inactiveEmittersWithStopActions). t, numLoops and accumulatedDt keep their values.
fn keep_updating(run: &mut Run, systems: &[Sys], manager: &mut Manager) {
    reset_seeds(run, systems, manager);
    run.clock.stop_emitting = true;
    run.halted = false;
    if let Some(k) = manager.inactive.iter().position(|&x| x == run.id) {
        manager.inactive.swap_remove(k);
    }
    if !manager.active.contains(&run.id) {
        manager.active.push(run.id);
    }
    // Its next Update2: SetIsRenderable(true) adds the renderer back to the scene.
    if !manager.nodes.contains(&run.id) {
        manager.nodes.push(run.id);
    }
}

/// ParticleSystemManager: its rand, activeEmitters (AddToManager 1801b4cc0 appends, RemoveFromManager 1801bea00
/// swap-removes) and inactiveEmittersWithStopActions.
pub(crate) struct Manager {
    rand: Rand,
    active: Vec<u32>,
    inactive: Vec<u32>,
    next: u32,
    groups: u32,
    /// The listed systems whose renderer the last render no longer saw: the next EarlyUpdate's NotifyInvisible.
    invisible: Vec<u32>,
    /// RendererScene's nodes: AddRenderer (1805d4890) appends, RemoveRendererInternal (1805e5410) swap-removes.
    nodes: Vec<u32>,
    /// The pooled clones made so far: a later Request reactivates one (SetActive(true)).
    pooled: std::collections::HashSet<crate::game::Pooled>,
    /// A loaded recording's listed systems the replay does not run (Listed::Ghost, Listed::Destroyed): they hold their
    /// slots until the EndUpdateAll that counts their walks left down to 0, or their clone's Destroy (its Transform).
    ghosts: HashMap<u32, (Option<i32>, Option<GhostClock>)>,
}

/// One slot of a recording's active list (Game::recorded_active).
#[derive(Clone, Copy, Debug)]
pub enum Listed {
    /// A loaded bleed's root (BleedingParticle.psm).
    Bleed,
    /// BloodPuff `b`'s system at GetComponentsInChildren position `j` (0: the root).
    Puff(usize, usize),
    /// A DeleteAfterTime clone's system (SetupSpawnPrefab's SpawnParticles): it leaves at its Transform's Destroy.
    Destroyed(i32),
    /// ponytail: any other listed system (a loaded bleed's Trail and water_puff, played long ago) is taken to leave at
    /// the first EndUpdateAll, as the 1.28.52 recordings show; one that plays on needs its own run.
    Ghost,
}

/// A loaded SpawnParticles' system clock (it holds no particles): `t` advances as Update0's
/// (unscaled ? unscaledDeltaTime : deltaTime) * simulationSpeed, and the system is empty and stopEmitting once `t`
/// passes `end` (its duration, and its last birth plus its longest start lifetime).
#[derive(Clone, Copy, Debug)]
pub struct GhostClock {
    t: f32,
    speed: f32,
    unscaled: bool,
    end: f32,
}

impl GhostClock {
    fn of(ps: &ParticleSystem) -> Self {
        let longest = |c: &crate::assets::particles::MinMaxCurve| match c.state {
            0 | 3 => c.scalar,
            s => unimplemented!("mod-only: a loaded SpawnParticles with a curve start lifetime (state {s})"),
        };
        let last_birth = if ps.rate_over_time.scalar > 0.0 || ps.rate_over_distance.scalar > 0.0 {
            ps.length
        } else {
            ps.bursts.iter().map(|b| b.time + b.interval * (b.cycles.max(1) - 1) as f32).fold(0.0, f32::max)
        };
        if ps.looping {
            unimplemented!("mod-only: a loaded SpawnParticles that loops");
        }
        GhostClock { t: 0.0, speed: ps.simulation_speed, unscaled: ps.unscaled_time, end: ps.length.max(last_birth + longest(&ps.start_lifetime)) }
    }
}

/// Destroy of a recorded DeleteAfterTime clone (its Transform `key`): RemoveFromManager swap-removes its system.
pub fn destroyed_ghost(g: &mut Game, key: i32) {
    let Some(w) = g.particle_world.as_mut() else { return };
    let m = &mut w.manager;
    if let Some((&id, _)) = m.ghosts.iter().find(|(_, k)| k.0 == Some(key)) {
        m.ghosts.remove(&id);
        if let Some(i) = m.active.iter().position(|&x| x == id) {
            m.active.swap_remove(i);
        }
    }
}

/// RendererBecameInvisible (1801beb10): an enabled renderer's system culls when cullingMode is PauseAndCatchup or Pause
/// (Always), or Automatic with looping and supportsProcedural (Procedural: only while invalidateProcedural is clear).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cull {
    Never,
    Always,
    Procedural,
}

impl Manager {
    fn add(&mut self) -> u32 {
        self.next += 1;
        self.active.push(self.next);
        self.nodes.push(self.next);
        self.next
    }

    /// Update2 (1801c5b90) of each updated system, the active list walked from the last: one whose renderer is in the
    /// scene but not visible (isVisible, Renderer +0x128 bit 0x1000, off: a new one, or one the last render did not
    /// see) and that is not culled is culled by its mode (Automatic: looping, procedural, not invalidated; Pause and
    /// PauseAndCatchup always): ParticleSystem::Cull (1801b8500) swap-removes it from activeEmitters and parks it in
    /// inactiveEmittersWithStopActions by `push`. Returns the culled ones.
    fn update2_cull(&mut self, culls: impl Fn(u32) -> bool, push: impl Fn(u32) -> bool) -> Vec<u32> {
        let mut culled = Vec::new();
        for i in (0..self.active.len()).rev() {
            let id = self.active[i];
            if culls(id) {
                culled.push(self.active.swap_remove(i));
                if push(id) {
                    self.inactive.push(id);
                }
            }
        }
        culled
    }

    /// GameObject::ActivateAwakeRecursivelyInternal (1805865f0) deactivates the children first, then the object's own
    /// components. ParticleSystem::Deactivate (1801b8610): RemoveFromManager swap-removes it from the active list (a
    /// push to the inactive list it may make is undone at once), then the first inactive entry that is it is
    /// swap-removed (AddToManager keeps a system out of both lists at once).
    fn forget(&mut self, run: &Run) {
        for c in &run.children {
            self.forget(c);
        }
        for list in [&mut self.active, &mut self.inactive] {
            if let Some(i) = list.iter().position(|&x| x == run.id) {
                list.swap_remove(i);
            }
        }
        self.remove_node(run.id);
    }

    /// Renderer::RemoveFromScene (18069e3a0): RendererScene::RemoveRenderer (1805e5260) swap-removes its node at once
    /// (RemoveRendererInternal 1805e5410: the last node takes the hole).
    fn remove_node(&mut self, id: u32) {
        if let Some(i) = self.nodes.iter().position(|&x| x == id) {
            self.nodes.swap_remove(i);
        }
    }
}

/// The game's ParticleSystems: the systems read so far, the played runs and ParticleSystemManager.
pub struct World {
    library: Library,
    runs: Vec<Run>,
    bleeds: HashMap<usize, Run>,
    attached: HashMap<(usize, usize), Attached>,
    shockwaves: Vec<crate::game::particle_shockwave::Shockwave>,
    /// How much of `effect_log` the Start passes have made clones of.
    shockwave_scan: usize,
    embedded: HashMap<usize, (Run, Option<(Vec3, Quat)>, u32)>,
    /// The pick for RecordEmit's never-written randomSeed slot (the inherited colour in `record`).
    rng: Rng,
    /// Every played system's ResetSeeds draws from its rand.
    manager: Manager,
}

/// gParticleSystemManager->rand as a recording holds it (the game's is seeded from system entropy).
/// Particles alive now in played effects, bleeds, the physicals' attached systems and embedded systems (tests).
#[cfg(test)]
pub fn live_particles(g: &Game) -> usize {
    g.particle_world.as_ref().map_or(0, |w| {
        let attached = w.attached.values().flat_map(|a| a.runs.iter().map(|r| &r.0));
        w.runs.iter().chain(w.bleeds.values()).chain(attached).chain(w.embedded.values().map(|e| &e.0)).map(|r| r.particles.len()).sum()
    })
}

pub fn set_manager_rand(g: &mut Game, state: [u32; 4]) {
    g.particle_world.get_or_insert_with(Default::default).manager.rand.state = state;
}

impl Default for World {
    fn default() -> Self {
        World { library: Library::default(), runs: Vec::new(), bleeds: HashMap::new(), attached: HashMap::new(), embedded: HashMap::new(), shockwaves: Vec::new(), shockwave_scan: 0, rng: Rng(0x9e3779b9), manager: Manager { rand: crate::game::particle_rand::manager_rand(), active: Vec::new(), inactive: Vec::new(), next: 0, groups: 0, invisible: Vec::new(), nodes: Vec::new(), pooled: Default::default(), ghosts: HashMap::new() } }
    }
}

/// PreLateUpdate.ParticleSystemBeginUpdateAll: the effects played since the last one, then every system steps by
/// the frame's deltaTime (the native jobs finish in EndUpdateAll; nothing in between reads them).
pub fn begin_update_all(g: &mut Game) {
    let log = std::mem::take(&mut g.effect_log);
    let calls = std::mem::take(&mut g.ps_calls);
    let recorded_active = std::mem::take(&mut g.recorded_active);
    let Some(mut db) = g.assets.take() else { return };
    let mut w = g.particle_world.take().unwrap_or_default();
    let World { library, runs, bleeds, attached, embedded, shockwaves, shockwave_scan, rng, manager } = &mut *w;
    deactivate(runs, manager, &std::mem::take(&mut g.pooled_deactivated));
    for s in &mut library.systems {
        s.awake = !s.fancy_only || g.prefs.FancyEffects;
    }
    let dt = g.time.deltaTime;
    // The clones made since the last Start pass (their Start comes at the next one).
    spawn_shockwaves(library, shockwaves, &mut db, &log, *shockwave_scan);
    *shockwave_scan = 0;
    // The frame's Instantiates, Start passes and new bleeding particles in their order (Unity runs the Start of
    // clones a FixedUpdate made before the next FixedUpdate): each clone's systems join the manager at its Play, a
    // pooled clone's child its Start deactivates leaves it there (swap-remove), and a new bleeding particle's
    // systems join at its Instantiate.
    let offs = std::mem::take(&mut g.pooled_child_off);
    let mut fresh: Vec<(usize, usize, Option<usize>)> = Vec::new();
    for (i, b) in g.bleeding.iter().enumerate() {
        if b.loaded || bleeds.contains_key(&i) || g.physicals[b.Parent].destroyed || g.physicals[b.Parent].rigidbody.detached.is_some() {
            continue;
        }
        let Some(prefab) = b.prefab else { continue };
        let Some(sys) = library.system(&db, prefab, Quat::IDENTITY) else { continue };
        let go = db.prefab_cached(prefab).ok().and_then(|p| p.objects.iter().find(|o| o.name == "water puff").map(|o| o.game_object));
        fresh.push((i, sys, go.and_then(|go| library.system(&db, go, Quat::IDENTITY))));
    }
    fresh.sort_by_key(|f| g.bleeding[f.0].log_at);
    let mut reserved = HashMap::new();
    let (mut o, mut f) = (0, 0);
    for j in 0..=log.len() {
        loop {
            let next_off = offs.get(o).filter(|x| x.2 .0 <= j).map(|x| x.2);
            let next_bleed = fresh.get(f).map(|x| g.bleeding[x.0].log_at).filter(|at| at.0 <= j);
            match (next_off, next_bleed) {
                (Some(a), b) if b.is_none_or(|b| a < b) => {
                    child_off(runs, manager, offs[o].0, offs[o].1);
                    o += 1;
                }
                (_, Some(_)) => {
                    let (i, sys, puff) = fresh[f];
                    reserved.insert(i, awake_order(&[sys].into_iter().chain(puff).collect::<Vec<_>>(), &library.systems, manager));
                    f += 1;
                }
                _ => break,
            }
        }
        let Some(e) = log.get(j) else { break };
        if let Some(group) = play_effect(library, runs, &mut db, e, manager) {
            for sw in shockwaves.iter_mut().filter(|s| s.log == Some(j)) {
                sw.group = group;
            }
        }
    }
    for &(p, name, _) in &offs[o..] {
        child_off(runs, manager, p, name);
    }
    for sw in shockwaves.iter_mut() {
        sw.log = None;
    }
    // The FixedUpdates' Emits: nothing touched the run between them and this update.
    for sw in shockwaves.iter_mut() {
        for at in std::mem::take(&mut sw.pending) {
            let Some(run) = runs.iter_mut().find(|r| r.group == sw.group && r.chain.first() == Some(&sw.t.ps)) else { continue };
            emit_external(run, &library.systems, g, &EmitParams { position: Some(Vec3::new(at[0], at[1], 0.0)), ..Default::default() }, rng);
        }
    }
    shockwaves.retain(|s| s.alive());
    // BeginUpdateAll's AssignRayBudgets over the systems playing at the frame's start.
    {
        let systems = &library.systems;
        let mut list = Vec::new();
        for r in bleeds.values_mut() {
            budget_runs(r, systems, g, true, &mut list);
        }
        for a in attached.values_mut() {
            for (r, ..) in &mut a.runs {
                budget_runs(r, systems, g, false, &mut list);
            }
        }
        for (r, ..) in embedded.values_mut() {
            budget_runs(r, systems, g, false, &mut list);
        }
        for r in runs.iter_mut() {
            budget_runs(r, systems, g, false, &mut list);
        }
        // AssignRayBudgets walks activeEmitters (AddToManager appends, RemoveFromManager swap-removes): only the
        // systems in it, in its order.
        list.retain(|b| manager.active.contains(&b.id));
        list.sort_by_key(|b| manager.active.iter().position(|&x| x == b.id));
        crate::game::particle_collision::assign_ray_budgets(&mut list);
    }
    let mut restored = Vec::new();
    // BeginUpdate's roots' KeepUpdating subs (see keep_updating_subs), joined after the frame's Plays.
    let mut kept: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut loaded_rand: Option<crate::unity::Random> = None;
    for (i, b) in g.bleeding.iter().enumerate() {
        // A child of the limb: destroyed with it, its particles too.
        if g.physicals[b.Parent].destroyed || g.physicals[b.Parent].rigidbody.detached.is_some() {
            if let Some(run) = bleeds.remove(&i) {
                manager.forget(&run);
            }
            continue;
        }
        let Some(prefab) = b.prefab else { continue };
        let Some(sys) = library.system(&db, prefab, Quat::IDENTITY) else { continue };
        let position = g.transform_point3(b.Parent, b.local_position);
        let up = g.transform_direction(b.Parent, b.local_up);
        let velocity = g.world.body(g.physicals[b.Parent].rigidbody.body).linear_velocity;
        let drag = library.systems[sys].ps.clamp.as_ref().map_or(0.0, |c| c.drag.min_scalar);
        let lam = b.Laminarity;
        let drive = Drive {
            velocity: Vec3::new(velocity.x, velocity.y, 0.0),
            enabled: b.emission_enabled && !b.destroyed,
            rate: b.rateOverTimeMultiplier,
            speed: b.startSpeedMultiplier,
            color: b.color,
            radius: 0.1 + (0.02 - 0.1) * lam,
            arc: 10.0 + (5.0 - 10.0) * lam,
            drag_max: 1.0 + (drag - 1.0) * lam,
            collision: b.collision,
        };
        let puff = if b.loaded || bleeds.contains_key(&i) {
            None
        } else {
            let go = db.prefab_cached(prefab).ok().and_then(|p| p.objects.iter().find(|o| o.name == "water puff").map(|o| o.game_object));
            go.and_then(|go| library.system(&db, go, Quat::IDENTITY))
        };
        let run = bleeds.entry(i).or_insert_with(|| {
            // The clone's AwakeFromLoad plays the root, Trail and water puff by original id.
            let mut pre = if b.loaded { HashMap::new() } else { reserved.remove(&i).unwrap_or_else(|| awake_order(&[sys].into_iter().chain(puff).collect::<Vec<_>>(), &library.systems, manager)) };
            // A loaded bleed's ResetSeeds drew before the recording's psrand: the manager's Rand stays as recorded. Its root
            // continues from the recorded randoms (pseed=); the children (halted below) never draw.
            let rand = manager.rand;
            let lanes = |w: &[u32], o: usize| crate::game::particle_rand::RandN([0, 1, 2, 3].map(|i| crate::unity::Random { state: [w[o + i], w[o + 4 + i], w[o + 8 + i], w[o + 12 + i]] }));
            if b.loaded {
                if let Some(w) = &b.pseed {
                    let seeds = crate::game::particle_rand::Seeds { random_seed: w[0], emission: crate::unity::Random { state: [w[1], w[2], w[3], w[4]] }, initial: lanes(w, 5) };
                    pre.insert(sys, (seeds, manager.add()));
                } else {
                    // ponytail: recordings before pseed= (the 1.28.41 ones; on 1.28.52 the four loops whose pseed= re-recording
                    // fails elsewhere keep theirs): the seeds are unknown; the loaded ones take successive draws of a copy of
                    // the manager's Rand. No loop's compared state shows them (a fixed unrelated Rand passes all 54 on both
                    // versions); a pseed= re-recording gives the real ones.
                    manager.rand = loaded_rand.unwrap_or(rand);
                }
            }
            let mut run = start_with(sys, Vec3::ZERO, Quat::IDENTITY, 1.0, &library.systems, manager, &mut pre);
            if b.loaded {
                if let Some(w) = &b.pseed {
                    run.shape.as_mut().unwrap().rand = lanes(w, 21);
                    run.collision.as_mut().unwrap().rand = lanes(w, 37);
                } else {
                    loaded_rand = Some(manager.rand);
                }
                manager.rand = rand;
                // Played long before the recording: the sub-emitters left the list (kStopped) at the first EndUpdateAll.
                for c in &mut run.children {
                    manager.forget(c);
                    c.halted = true;
                }
                restored.push((b.psm, run.id));
                // The root's recorded culling state: renderer isVisible (the last render saw it), culled, cullTime, stopTime.
                if let Some(&(visible, culled, cull_time, stop_time)) = b.pss.first() {
                    (run.seen, run.culled, run.cull_time, run.stop_time) = (visible, culled, cull_time, stop_time);
                }
            } else if let Some(p) = puff {
                if b.water_puff {
                    // BleedingParticleBehaviour.Start destroys it before the clone's first EndUpdateAll, which removes it
                    // with the stopped Trail (one walk from the last).
                    let mut puff = start_with(p, Vec3::ZERO, Quat::IDENTITY, 1.0, &library.systems, manager, &mut pre);
                    puff.clock.stop_emitting = true;
                    puff.invalid = true;
                    run.children.push(puff);
                } else {
                    // ActivateIfFancyEffects.Awake: SetActive(false) right after its AwakeFromLoad.
                    let id = pre[&p].1;
                    let k = manager.active.iter().position(|&x| x == id).unwrap();
                    manager.active.swap_remove(k);
                }
            }
            budget_new(&mut run, &library.systems, g, b.collision);
            run
        });
        run.position = Vec3::from(position);
        // transform.up = local_up: its z axis stays.
        run.rotation = Quat::from_rotation_z(crate::unity::atan2f(up[1], up[0]) - std::f32::consts::FRAC_PI_2);
        run.stopped = b.destroyed;
        run.invalid |= !run.halted;
        if manager.active.contains(&run.id) {
            let mut subs = Vec::new();
            keep_updating_subs(run, &library.systems, &mut subs);
            kept.push((run.id, subs));
        }
        step(run, &library.systems, Some(&drive), g, dt, rng);
    }
    // A recording with every listed system's slot: the active list as recorded, our systems in their slots and
    // placeholders for the others; any of ours it does not list keeps its order after them.
    if !recorded_active.is_empty() {
        let bleed_of = |psm: i32| restored.iter().find(|r| r.0 == Some(psm)).map(|r| r.1);
        let puff_of = |b: usize, j: usize| {
            let chain: Vec<usize> = if j == 0 { vec![0] } else { vec![j, 0] };
            let mut stack: Vec<&Run> = runs.iter().filter(|r| r.pooled == Some(crate::game::Pooled::BloodPuff(b))).collect();
            while let Some(r) = stack.pop() {
                if r.chain == chain {
                    return Some(r.id);
                }
                stack.extend(r.children.iter());
            }
            None
        };
        let mut list = Vec::new();
        for (i, l) in recorded_active {
            let id = match l {
                Listed::Bleed => bleed_of(i),
                Listed::Puff(b, j) => puff_of(b, j),
                Listed::Destroyed(_) | Listed::Ghost => None,
            };
            let id = id.unwrap_or_else(|| {
                manager.next += 1;
                // A loaded SpawnParticles (Listed::Destroyed: the recordings' DeleteAfterTime clones are SetupSpawnPrefab's)
                // runs on its system's clock from its Play (the recorder loads right after the spawn: DeleteAfterTime
                // time 0); the 1.28.52 prefab's simulationSpeed 4 on unscaled time ends it in the recorder's first
                // slow frames (unscaledDeltaTime ~1 s).
                let clock = if let Listed::Destroyed(_) = l {
                    let go = db.resource("Prefabs/SpawnParticles", 1).ok().flatten().expect("SpawnParticles prefab");
                    let sys = library.system(&db, go, Quat::IDENTITY).expect("SpawnParticles system");
                    Some(GhostClock::of(&library.systems[sys].ps))
                } else {
                    None
                };
                manager.ghosts.insert(manager.next, (if let Listed::Destroyed(k) = l { Some(k) } else { None }, clock));
                manager.next
            });
            list.push(id);
        }
        let rest: Vec<u32> = manager.active.iter().copied().filter(|x| !list.contains(x) && !restored.iter().any(|r| r.1 == *x)).collect();
        list.extend(rest);
        manager.active = list;
    } else if restored.iter().any(|r| r.0.is_some()) {
        // The recorded active list: the loaded bleeds take their slots in its order, an unlisted one leaves (recordings
        // before the index keep record order).
        let mut slots: Vec<usize> = restored.iter().map(|r| manager.active.iter().position(|&x| x == r.1).unwrap()).collect();
        slots.sort();
        restored.sort_by_key(|r| r.0);
        let listed: Vec<u32> = restored.iter().filter(|r| r.0 >= Some(0)).map(|r| r.1).collect();
        for (&slot, &id) in slots.iter().zip(&listed) {
            manager.active[slot] = id;
        }
        for &slot in slots[listed.len()..].iter().rev() {
            manager.active.remove(slot);
        }
    }
    attached_systems(g, &mut db, library, attached, dt, rng, manager);
    embedded_systems(g, &db, library, embedded, calls, dt, rng, manager);
    let mut r = std::mem::take(runs);
    for run in &mut r {
        if run.root.is_some_and(|root| g.destroyed_components.contains(&root)) {
            run.stopped = true;
        }
        // A clone parented to a physical's Transform follows it.
        if let Some((p, offset, rotation, scale)) = run.parent.filter(|p| !g.physicals[p.0.physical].destroyed) {
            let (pp, pq, ls) = g.anchored_world(crate::game::BareAnchor::Physical(p.physical), &[]);
            let pq = Quat::from_array(pq);
            let root = Vec3::from(pp) + pq * (Vec3::from(p.position) * Vec3::from(ls));
            let turn = pq * Quat::from_array(p.rotation);
            run.position = root + turn * (offset * scale);
            run.rotation = turn * rotation;
            // PhysicalBehaviour.SetupSpawnPrefab's shape.spriteRenderer: the physical's SpriteRenderer.
            if p.sprite_shape && library.systems[run.sys].ps.shape_type == 20 {
                let (renderer, mesh) = sprite_renderer(g, &db, library, p.physical);
                run.renderer = Some(renderer);
                if let Some(st) = run.shape.as_mut() {
                    st.mesh = mesh;
                }
            }
        }
        // A child clone goes with its destroyed parent object (as a destroyed root's, its emission stops).
        if run.parent.is_some_and(|p| p.0.sprite_shape && g.physicals[p.0.physical].destroyed) {
            run.stopped = true;
        }
        // AOEPowerTool.Update: the effect's transform follows the mouse.
        if let Some(p) = g.powers.iter().find(|p| run.root == Some(p.root)) {
            run.position = Vec3::from(g.MousePosition);
            run.stopped |= !p.playing;
        }
        // ParticleSystem.useUnscaledTime (SpawnParticles).
        let dt = if library.systems[run.sys].ps.unscaled_time { g.time.unscaledDeltaTime } else { dt };
        if manager.active.contains(&run.id) {
            let mut subs = Vec::new();
            keep_updating_subs(run, &library.systems, &mut subs);
            kept.push((run.id, subs));
        }
        step(run, &library.systems, None, g, dt, rng);
    }
    // AddToManager (1801b4cc0) of the kept subs: BeginUpdate walks its roots in activeEmitters order, after the
    // Update scripts' Plays and Instantiates (appended; out of inactiveEmittersWithStopActions).
    kept.sort_by_key(|(root, _)| manager.active.iter().position(|x| x == root));
    for id in kept.into_iter().flat_map(|k| k.1) {
        if let Some(k) = manager.inactive.iter().position(|&x| x == id) {
            manager.inactive.swap_remove(k);
        }
        if !manager.active.contains(&id) {
            manager.active.push(id);
        }
        if !manager.nodes.contains(&id) {
            manager.nodes.push(id);
        }
    }
    *runs = r;
    g.particle_world = Some(w);
    g.assets = Some(db);
}

/// SetActive(false) of pooled clones (a return that is not their own stop): their systems stop and clear.
fn deactivate(runs: &mut Vec<Run>, manager: &mut Manager, clones: &[crate::game::Pooled]) {
    runs.retain(|r| {
        let gone = r.pooled.is_some_and(|p| clones.contains(&p));
        if gone {
            manager.forget(r);
        }
        !gone
    });
}

/// What EndUpdateAll reads of one system.
struct Emitter {
    empty: bool,
    stop_emitting: bool,
    halted: bool,
    sub: bool,
    stop_action: i32,
    group: u32,
    chain: Vec<usize>,
    children: Vec<u32>,
    pooled: Option<crate::game::Pooled>,
}

fn emitters(run: &Run, systems: &[Sys], sub: bool, out: &mut HashMap<u32, Emitter>) {
    // PoolableParticleSystemHelper.Awake sets the clone root's stopAction to Callback.
    let stop_action = if run.pooled.is_some() && run.chain.len() == 1 { 3 } else { systems[run.sys].ps.stop_action };
    let e = Emitter {
        empty: run.particles.is_empty(),
        stop_emitting: run.stopped || run.clock.stop_emitting,
        halted: run.halted,
        sub,
        stop_action,
        group: run.group,
        chain: run.chain.clone(),
        children: run.children.iter().map(|c| c.id).collect(),
        pooled: run.pooled,
    };
    out.insert(run.id, e);
    for c in &run.children {
        emitters(c, systems, true, out);
    }
}

/// AllChildrenAreStopped (1801b4d80): every system below `id` in the hierarchy (its sub-emitters, the clone's systems
/// on objects under its own) has no particle and is stopped. Nothing is culled in the rewrite.
fn all_children_stopped(all: &HashMap<u32, Emitter>, id: u32) -> bool {
    let e = &all[&id];
    let below = |x: &Emitter| e.group != 0 && x.group == e.group && !e.chain.is_empty() && x.chain.len() > e.chain.len() && x.chain[x.chain.len() - e.chain.len()..] == e.chain[..];
    let ok = |x: &Emitter| x.empty && x.halted;
    e.children.iter().all(|c| ok(&all[c]) && all_children_stopped(all, *c)) && all.values().filter(|x| below(x)).all(|x| ok(x) && x.children.iter().all(|c| ok(&all[c]) && all_children_stopped(all, *c)))
}

fn for_runs(w: &mut World, f: &mut impl FnMut(&mut Run)) {
    fn walk(r: &mut Run, f: &mut impl FnMut(&mut Run)) {
        f(r);
        for c in &mut r.children {
            walk(c, f);
        }
    }
    let World { runs, bleeds, attached, embedded, .. } = w;
    for r in runs.iter_mut().chain(bleeds.values_mut()).chain(attached.values_mut().flat_map(|a| a.runs.iter_mut().map(|r| &mut r.0))).chain(embedded.values_mut().map(|e| &mut e.0)) {
        walk(r, f);
    }
}

/// PostLateUpdate.ParticleSystemEndUpdateAll (1801ba1c0): activeEmitters from the last, each with no particle that
/// stopped emitting is stopped and removed (RemoveFromManager parks one whose children still play in
/// inactiveEmittersWithStopActions), its stop action running once AllChildrenAreStopped; then the parked ones whose
/// children have stopped. Returns the pooled clones whose root ran its Callback (OnParticleSystemStopped), in order.
pub fn end_update_all(g: &mut Game) -> Vec<crate::game::Pooled> {
    let Some(mut w) = g.particle_world.take() else { return Vec::new() };
    deactivate(&mut w.runs, &mut w.manager, &std::mem::take(&mut g.pooled_deactivated));
    let mut all = HashMap::new();
    {
        let World { runs, bleeds, attached, embedded, library, .. } = &*w;
        for r in runs.iter().chain(bleeds.values()).chain(attached.values().flat_map(|a| a.runs.iter().map(|r| &r.0))).chain(embedded.values().map(|e| &e.0)) {
            emitters(r, &library.systems, false, &mut all);
        }
    }
    let mut stopped = Vec::new();
    let mut destroyed = Vec::new();
    let mut objects = Vec::new();
    // ExecuteStopAction (1801ba7c0): a sub-emitter's does nothing; Disable deactivates, Destroy destroys the object.
    let execute = |all: &HashMap<u32, Emitter>, id: u32, stopped: &mut Vec<crate::game::Pooled>, destroyed: &mut Vec<u32>, objects: &mut Vec<u32>| {
        let e = &all[&id];
        if e.sub {
            return;
        }
        match (e.stop_action, e.pooled) {
            (3, Some(p)) => stopped.push(p),
            (3, None) => {}
            (1 | 2, _) if e.group != 0 => destroyed.push(e.group),
            (2, _) => objects.push(id),
            (a, _) => unimplemented!("mod-only: ParticleSystemStopAction {a} on a system outside a clone"),
        }
    };
    let m = &mut w.manager;
    let mut i = m.active.len();
    while i > 0 {
        i -= 1;
        let id = m.active[i];
        // A recording's placeholder: a loaded SpawnParticles stops when its clock passes its end (unless its Destroy
        // comes first); the others at the first walk.
        if let Some(k) = m.ghosts.get_mut(&id) {
            let gone = match &mut k.1 {
                None => true,
                Some(c) => {
                    c.t += if c.unscaled { g.time.unscaledDeltaTime } else { g.time.deltaTime } * c.speed;
                    c.t > c.end
                }
            };
            if gone {
                m.ghosts.remove(&id);
                m.active.swap_remove(i);
            }
            continue;
        }
        let e = &all[&id];
        if e.empty && e.stop_emitting {
            all.get_mut(&id).unwrap().halted = true;
            // RemoveFromManager; SetIsRenderable(false) takes its renderer out of the scene.
            m.active.swap_remove(i);
            m.remove_node(id);
            let action = all[&id].stop_action != 0;
            let children = all_children_stopped(&all, id);
            if action && !children {
                m.inactive.push(id);
            }
            if action && children {
                execute(&all, id, &mut stopped, &mut destroyed, &mut objects);
            }
        }
    }
    let mut j = 0;
    while j < m.inactive.len() {
        let id = m.inactive[j];
        let e = &all[&id];
        if e.stop_action == 0 {
            m.inactive.swap_remove(j);
        } else if !e.halted || !all_children_stopped(&all, id) {
            j += 1;
        } else {
            m.inactive.swap_remove(j);
            execute(&all, id, &mut stopped, &mut destroyed, &mut objects);
        }
    }
    for_runs(&mut w, &mut |r| r.halted = all[&r.id].halted);
    // The stop actions, then the render; RendererBecameVisible's stop actions take effect after it.
    let mut rendered = false;
    loop {
        // Destroy on a script's or a clone's own system (an embedded one): DelayedDestroyCallback queued at
        // min(fixed, frame time) - 100, without DestroyObjectFromScripting's SetEnabled(false).
        for id in objects.drain(..) {
            let Some(&i) = w.embedded.iter().find(|(_, (r, ..))| r.id == id).map(|(i, _)| i) else { unimplemented!("mod-only: ParticleSystemStopAction Destroy on a system that is no clone's and no embedded one") };
            let owner = g.embedded_ps[i].owner;
            if !g.destroyed_components.contains(&owner) && !g.object_destroys.iter().any(|&(_, x)| x == owner) {
                g.object_destroys.push((g.time.fixedTimeAsDouble.min(g.time.timeAsDouble) - 100.0, owner));
            }
        }
        // Disable / Destroy: the clone's systems go.
        let World { runs, manager, bleeds, .. } = &mut *w;
        runs.retain(|r| {
            let gone = destroyed.contains(&r.group);
            if gone {
                manager.forget(r);
            }
            !gone
        });
        // The rewrite keeps a stopped system only while something may play it again, or while the manager still lists it
        // (one parked in inactiveEmittersWithStopActions waits for the clone's other systems).
        fn listed(m: &Manager, r: &Run) -> bool {
            m.active.contains(&r.id) || m.inactive.contains(&r.id) || r.children.iter().any(|c| listed(m, c))
        }
        runs.retain(|r| !(r.halted && done(r)) || listed(manager, r));
        bleeds.retain(|_, r| !(r.halted && done(r) && r.stopped) || listed(manager, r));
        if rendered {
            break;
        }
        rendered = true;
        destroyed.clear();
        for id in render(g, &mut w, &all) {
            execute(&all, id, &mut stopped, &mut destroyed, &mut objects);
        }
    }
    g.particle_world = Some(w);
    stopped
}

/// CulledSystemIsStillPlaying (1801b8550) at `now` with cullTime `cull_time` (the rewrite has no kPaused).
fn still_playing(run: &Run, ps: &ParticleSystem, cull_time: f64, now: f64) -> bool {
    let life = ps.start_lifetime.scalar;
    !((!ps.looping && ((life + ps.length) as f64) < (now - cull_time) + run.clock.t() as f64) || ((run.stopped || run.clock.stop_emitting) && (life as f64) < now - run.stop_time))
}

fn find_run(r: &mut Run, id: u32) -> Option<&mut Run> {
    if r.id == id {
        return Some(r);
    }
    r.children.iter_mut().find_map(|c| find_run(c, id))
}

type Embedded = HashMap<usize, (Run, Option<(Vec3, Quat)>, u32)>;

fn run_by_id<'a>(runs: &'a mut [Run], bleeds: &'a mut HashMap<usize, Run>, attached: &'a mut HashMap<(usize, usize), Attached>, embedded: &'a mut Embedded, id: u32) -> Option<&'a mut Run> {
    runs.iter_mut().chain(bleeds.values_mut()).chain(attached.values_mut().flat_map(|a| a.runs.iter_mut().map(|r| &mut r.0))).chain(embedded.values_mut().map(|e| &mut e.0)).find_map(|r| find_run(r, id))
}

/// The frame's render, after EndUpdateAll. The updated systems' Update2 culling (`Manager::update2_cull`), then
/// RendererScene::UpdateVisibility (1805ef290): RendererBecameVisible on each node the culling sees and did not see at
/// the last render, in node order; one it saw then and not now gets RendererBecameInvisible at the next EarlyUpdate
/// (`notify_invisible`). Returns the systems whose stop action RendererBecameVisible runs.
/// A renderer without particles takes part with its bounds as UpdateBounds left them (the probe of the scene's
/// m_BoundingBoxes: a point of 1e-5 extents at the system, or the procedural bounds).
fn render(g: &Game, w: &mut World, all: &HashMap<u32, Emitter>) -> Vec<u32> {
    struct Seen {
        invalid: bool,
        culled: bool,
        cull: Cull,
        now: bool,
        last: bool,
        sub: bool,
        push: bool,
    }
    let now = g.time.timeAsDouble;
    let mut info = HashMap::new();
    {
        let mut stack: Vec<(&Run, bool)> = w.runs.iter().chain(w.bleeds.values()).chain(w.attached.values().flat_map(|a| a.runs.iter().map(|r| &r.0))).chain(w.embedded.values().map(|e| &e.0)).map(|r| (r, false)).collect();
        while let Some((r, sub)) = stack.pop() {
            let s = &w.library.systems[r.sys];
            // RendererBecameInvisible's push to inactiveEmittersWithStopActions, culled at `now`.
            let push = all[&r.id].stop_action != 0 && ((!r.halted && still_playing(r, &s.ps, now, now)) || !all_children_stopped(all, r.id));
            info.insert(r.id, Seen { invalid: r.invalid, culled: r.culled, cull: s.first_cull, now: seen(g, r, s), last: r.seen, sub, push });
            stack.extend(r.children.iter().map(|c| (c, true)));
        }
    }
    let World { runs, bleeds, attached, embedded, library, manager, rng, .. } = w;
    let nodes = manager.nodes.clone();
    let culled = manager.update2_cull(
        |id| {
            info.get(&id).is_some_and(|i| {
                !i.last && !i.culled && nodes.contains(&id) && match i.cull {
                    Cull::Never => false,
                    Cull::Always => true,
                    Cull::Procedural => !i.invalid,
                }
            })
        },
        |id| info[&id].push,
    );
    for &id in &culled {
        let r = run_by_id(runs, bleeds, attached, embedded, id).unwrap();
        r.culled = true;
        r.cull_time = now;
    }
    let mut acts = Vec::new();
    for id in manager.nodes.clone() {
        let Some(i) = info.get(&id) else { continue };
        if i.now && !i.last {
            let r = run_by_id(runs, bleeds, attached, embedded, id).unwrap();
            if became_visible(r, &library.systems, i.sub, g, manager, rng) && all[&id].stop_action != 0 {
                acts.push(id);
            }
        }
    }
    // NotifyInvisible's bits: visible then, not now.
    for &id in &manager.nodes {
        if info.get(&id).is_some_and(|i| i.last && !i.now) {
            manager.invisible.push(id);
        }
    }
    // The renderer's isVisible flag: the culling's answer for a node; off out of the scene (RemoveFromScene).
    fn mark(r: &mut Run, info: &HashMap<u32, Seen>, nodes: &[u32]) {
        if let Some(i) = info.get(&r.id) {
            r.seen = i.now && nodes.contains(&r.id);
        }
        for c in &mut r.children {
            mark(c, info, nodes);
        }
    }
    for r in runs.iter_mut().chain(bleeds.values_mut()).chain(attached.values_mut().flat_map(|a| a.runs.iter_mut().map(|r| &mut r.0))).chain(embedded.values_mut().map(|e| &mut e.0)) {
        mark(r, &info, &manager.nodes);
    }
    acts
}

/// ParticleSystem::RendererBecameVisible (1801beb80) at the frame's m_CurFrameTime. True when the system is no longer
/// playing: CulledSystemIsStillPlaying failed, so it stopped, Clear ran, and its stop action is due.
fn became_visible(run: &mut Run, systems: &[Sys], sub: bool, g: &Game, manager: &mut Manager, rng: &mut Rng) -> bool {
    if !run.culled {
        return false;
    }
    run.culled = false;
    if run.halted {
        return false;
    }
    let s = &systems[run.sys];
    let ps = &s.ps;
    let now = g.time.timeAsDouble;
    if !still_playing(run, ps, run.cull_time, now) {
        // needRestart, stopEmitting, stopTime; Clear (1801b6cf0): no particle (array_resize 0 leaves the slots as they
        // were), emitReplay, cullTime, stopTime and ringBufferIndex reset, kStopped as it stops emitting, UpdateBounds.
        run.stopped = true;
        run.was_stopped = true;
        let mut tail: Vec<Option<Particle>> = run.particles.drain(..).map(Some).collect();
        tail.append(&mut run.tail);
        run.tail = tail;
        run.cull_time = 0.0;
        run.stop_time = 0.0;
        run.ring = 0;
        run.halted = true;
        update_bounds(run, s, g);
        return true;
    }
    let dt = now - run.cull_time;
    let (stopped, ended, was, stop_time) = (run.stopped, run.clock.stop_emitting, run.was_stopped, run.stop_time);
    // kCullingModePauseAndCatchup: Simulate (1801c1bf0) with kSimulateFixedTimeStep over CalculateRequiredCatchupTime
    // (1801b6830: the time modulo the duration when looping), the time before a Stop still emitting.
    let catchup = |t: f64| if ps.looping { (t as f32 % ps.length) as f64 } else { t };
    let mut simulated = false;
    if ps.culling_mode == 1 {
        if stop_time <= run.cull_time {
            if 0.0 < dt {
                catch_up(run, systems, g, catchup(dt) as f32, rng);
                simulated = true;
            }
        } else {
            run.stopped = false;
            run.clock.stop_emitting = false;
            let d = catchup(stop_time - run.cull_time);
            catch_up(run, systems, g, d as f32, rng);
            run.stopped = true;
            catch_up(run, systems, g, catchup(dt - d) as f32, rng);
            simulated = true;
        }
    }
    // Play(true) with needRestart cleared: kPlaying, firstUpdate (oldPosition from the transform), AddToManager. A
    // sub-emitter's Play only does that from kPaused (Simulate's), else it returns.
    if !sub || simulated {
        if !sub {
            run.old_position = None;
        }
        if let Some(k) = manager.inactive.iter().position(|&x| x == run.id) {
            manager.inactive.swap_remove(k);
        }
        if !manager.active.contains(&run.id) {
            manager.active.push(run.id);
        }
    }
    (run.stopped, run.clock.stop_emitting, run.was_stopped, run.stop_time) = (stopped, ended, was, stop_time);
    false
}

/// Simulate (1801c1bf0) with kSimulateFixedTimeStep alone: one Update of `t` stepped by Time.fixedDeltaTime with the
/// last update's module values, then kPaused and RemoveFromManager (a culled system is not listed); its sub systems'
/// UpdateBounds.
fn catch_up(run: &mut Run, systems: &[Sys], g: &Game, t: f32, rng: &mut Rng) {
    let drive = run.drive;
    run.fixed = true;
    step(run, systems, drive.as_ref(), g, t, rng);
    run.fixed = false;
    for c in &mut run.children {
        update_bounds(c, &systems[c.sys], g);
    }
}

/// EarlyUpdate's RendererScene::NotifyInvisible (1805e36a0): ParticleSystem::RendererBecameInvisible (1801beb10), in
/// node order, on each system the last render stopped seeing. Mode Automatic culls a looping procedural system without
/// invalidateProcedural, Pause and PauseAndCatchup always, AlwaysSimulate never: culled at the frame's time, out of
/// activeEmitters (swap-removed), and parked in inactiveEmittersWithStopActions while it, or a child, still plays.
pub fn notify_invisible(g: &mut Game, now: f64) {
    let Some(w) = g.particle_world.as_deref_mut() else { return };
    let ids = std::mem::take(&mut w.manager.invisible);
    if ids.is_empty() {
        return;
    }
    let mut all = HashMap::new();
    let mut cull = HashMap::new();
    {
        let mut stack: Vec<&Run> = w.runs.iter().chain(w.bleeds.values()).chain(w.attached.values().flat_map(|a| a.runs.iter().map(|r| &r.0))).chain(w.embedded.values().map(|e| &e.0)).collect();
        for r in &stack {
            emitters(r, &w.library.systems, false, &mut all);
        }
        while let Some(r) = stack.pop() {
            let s = &w.library.systems[r.sys];
            let culls = !r.culled
                && match s.first_cull {
                    Cull::Never => false,
                    Cull::Always => true,
                    Cull::Procedural => !r.invalid,
                };
            if culls {
                let push = all[&r.id].stop_action != 0 && ((!r.halted && still_playing(r, &s.ps, now, now)) || !all_children_stopped(&all, r.id));
                cull.insert(r.id, push);
            }
            stack.extend(r.children.iter());
        }
    }
    let World { runs, bleeds, attached, embedded, manager, .. } = w;
    for id in ids {
        let Some(&push) = cull.get(&id) else { continue };
        let r = run_by_id(runs, bleeds, attached, embedded, id).unwrap();
        r.culled = true;
        r.cull_time = now;
        if let Some(k) = manager.active.iter().position(|&x| x == id) {
            manager.active.swap_remove(k);
            if push {
                manager.inactive.push(id);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn particles(
    camera: Single<(&Projection, &Transform), With<crate::lights::MainCamera>>,
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut textures: ResMut<Textures>,
    mut outputs: Local<Vec<Option<[Option<Output>; 2]>>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut copies: Local<HashMap<(u32, u8), Drawn>>,
    mut pulled: Local<Pulled>,
) {
    if play.is_added() {
        (*outputs, *copies, *pulled) = Default::default();
    }
    let p = &mut *pulled;
    if p.slots == 0 {
        // Slot 0 stays empty (the warm-up draw's).
        p.slots = 1;
        // Its pipeline compiles in the background from the first draw: an effect should not wait for that.
        let material = materials.add(ParticleMaterial { settings: Settings::default(), texture: Handle::default(), subtract: false, vertices: PULLED_VERTICES, pulled: true });
        let mesh = p.bucket(&mut meshes, 0);
        commands.spawn((Mesh2d(mesh), MeshMaterial2d(material), MeshTag::new(0), Transform::default(), sprite_line_key(0, 0, 3000, 0), TrailEntity, NoFrustumCulling));
    }
    // This frame's vertices and indices of every copy, each copy's header [vertex start, index start, index count, 0] at its slot.
    let mut headers: Vec<[u32; 4]> = vec![[0; 4]; p.slots as usize];
    let mut data: Vec<u32> = Vec::new();
    let (Some(w), Some(db)) = (play.game.particle_world.as_deref(), play.game.assets.as_deref()) else { return };
    let library = &w.library;
    let (projection, _) = *camera;
    let width = match projection {
        Projection::Orthographic(o) => o.area.width(),
        _ => unreachable!("the main camera is orthographic"),
    };
    let (position, rotation) = play.game.camera_world();
    let cam = crate::particle_render::camera_matrices(position, rotation);
    let trail_view = crate::particle_trails::View::of(&cam);
    let n = library.systems.len();
    outputs.resize_with(n, || None);
    let mut gpu = Gpu { materials: &mut materials, meshes: &mut meshes, images: &mut images, textures: &mut textures };
    for (s, made) in library.systems.iter().zip(outputs.iter_mut()) {
        made.get_or_insert_with(|| s.go.map_or([None, None], |go| renderer_outputs(db, go, s, &mut gpu)));
    }
    // One draw per ParticleSystem copy and subset (its renderer's render node), as the engine's: each with its own sort
    // key, the copies' ties in spawn order (roIndex).
    let mut stack: Vec<&Run> = w.runs.iter().chain(w.bleeds.values()).chain(w.attached.values().flat_map(|a| a.runs.iter().map(|r| &r.0))).chain(w.embedded.values().map(|e| &e.0)).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(run) = stack.pop() {
        stack.extend(run.children.iter());
        let s = &library.systems[run.sys];
        let [qo, to] = outputs[run.sys].as_ref().expect("made above");
        let mut q = Buffers::default();
        let mut tb = Buffers::default();
        if qo.is_some() {
            quads(&mut q, s, run, width, &cam);
        }
        if to.is_some() {
            trails(&mut tb, s, run, &trail_view);
        }
        for (subset, (out, b)) in [(qo, q), (to, tb)].into_iter().enumerate() {
            let Some(out) = out else { continue };
            let k = (run.id, subset as u8);
            if b.indices.is_empty() && !copies.contains_key(&k) {
                continue;
            }
            seen.insert(k);
            // The render node's distance: this frame's world bounds centre.
            let key = crate::play::particle_key(out.key.0, out.key.1, out.key.2, world_bounds(run, s)[2], out.key.3, run.id as usize * 2 + subset);
            let n = b.indices.len();
            let bucket = (0..).find(|&j| 3usize << j >= n).unwrap();
            let c = match copies.get_mut(&k) {
                Some(c) => {
                    if c.key != key {
                        c.key = key;
                        commands.entity(c.entity).insert(key);
                    }
                    if c.bucket != bucket {
                        c.bucket = bucket;
                        commands.entity(c.entity).insert(Mesh2d(p.bucket(&mut meshes, bucket)));
                    }
                    c
                }
                None => {
                    let slot = p.free.pop().unwrap_or_else(|| {
                        p.slots += 1;
                        headers.push([0; 4]);
                        p.slots - 1
                    });
                    let mesh = p.bucket(&mut meshes, bucket);
                    let entity = commands.spawn((Mesh2d(mesh), MeshMaterial2d(out.material.clone()), MeshTag::new(slot), Transform::default(), key, crate::lights::camera_layers(s.game_layer), TrailEntity, NoFrustumCulling)).id();
                    copies.entry(k).or_insert(Drawn { entity, slot, bucket, key })
                }
            };
            if n > 0 {
                let v = data.len() as u32;
                for i in 0..b.positions.len() {
                    // x and y only: the draw's depth is the copy's sort rank (its Transform z), as every sprite's. The world z
                    // (a flamethrower's particles start at -1, in front of the scene in Unity's view) points the other way in
                    // Bevy's and failed them against the depth other sprites write, so the young particles never drew.
                    let [x, y, _] = b.positions[i];
                    data.extend([x, y, 0.0].map(f32::to_bits));
                    data.extend(b.uvs[i].map(f32::to_bits));
                    data.extend(b.colors[i].map(f32::to_bits));
                    data.extend(b.uv2[i].map(f32::to_bits));
                }
                headers[c.slot as usize] = [v, data.len() as u32, n as u32, 0];
                data.extend_from_slice(&b.indices);
            }
        }
    }
    copies.retain(|k, c| {
        let keep = seen.contains(k);
        if !keep {
            commands.entity(c.entity).despawn();
            p.free.push(c.slot);
        }
        keep
    });
    // One upload for all of them: the headers first, the offsets moved past them.
    let h = 4 * headers.len() as u32;
    let mut all: Vec<u32> = Vec::with_capacity(h as usize + data.len());
    for x in &headers {
        all.extend(if x[2] > 0 { [x[0] + h, x[1] + h, x[2], 0] } else { [0; 4] });
    }
    all.extend_from_slice(&data);
    let size = (all.len() as u64 * 4).max(1 << 16).next_power_of_two().max(p.capacity);
    p.capacity = size;
    let mut sb = ShaderBuffer::new(all, RenderAssetUsages::default());
    if let ShaderBufferData::Initialized { buffer_size, .. } = &mut sb.data {
        *buffer_size = size;
    }
    let _ = buffers.insert(&PULLED_VERTICES, sb);
}

/// A drawn particle system copy and subset: its entity, header slot, bucket mesh and sort key.
pub struct Drawn {
    entity: Entity,
    slot: u32,
    bucket: usize,
    key: crate::play::SortKey,
}

/// The pulled particle draws: header slots in use and free, the bucket meshes, the vertex buffer's size.
#[derive(Default)]
pub struct Pulled {
    slots: u32,
    free: Vec<u32>,
    buckets: Vec<Handle<Mesh>>,
    capacity: u64,
}

impl Pulled {
    /// The mesh of 3 << j vertices a copy with up to that many indices draws: its vertex shader reads the copy's own
    /// vertices from PULLED_VERTICES (the triangles past the copy's index count collapse to nothing).
    fn bucket(&mut self, meshes: &mut Assets<Mesh>, j: usize) -> Handle<Mesh> {
        while self.buckets.len() <= j {
            let n = 3usize << self.buckets.len();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            // Each vertex's number in x: a draw's vertex_index starts where the mesh sits in its shared buffer.
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, (0..n).map(|i| [i as f32, 0.0, 0.0]).collect::<Vec<_>>());
            self.buckets.push(meshes.add(mesh));
        }
        self.buckets[j].clone()
    }
}



/// A particle system copy's particles or trails, one mesh in world space.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct TrailEntity;

#[cfg(test)]
mod tests {
    use super::*;

    /// Update2's culling walks the active list from the last, swap-removing.
    #[test]
    fn update2_cull_order() {
        let mut m = Manager { rand: Rand::new(1), active: Vec::new(), inactive: Vec::new(), next: 0, groups: 0, invisible: Vec::new(), nodes: Vec::new(), pooled: Default::default(), ghosts: HashMap::new() };
        for _ in 0..5 {
            m.add();
        }
        assert_eq!(m.update2_cull(|id| [2, 4, 5].contains(&id), |id| id == 4), [5, 4, 2]);
        assert_eq!(m.active, [1, 3]);
        assert_eq!(m.inactive, [4]);
    }

    /// min_max_aabb against ParticleSystem::UpdateBounds (0x1801c5ea0) run in UnityPlayer.dll (Billboard renderer, point
    /// lights): particle cases, then procedural ones (supportsProcedural, local simulation).
    #[test]
    fn update_bounds_dll() {
        struct K {
            world: bool,
            uses3d: bool,
            size_on: bool,
            sbs_on: bool,
            trail: Option<bool>,
            curves: [(i16, u32, u32); 9],
            pivot: [u32; 3],
            max_size: u32,
            t: [u32; 3],
            w2l: [u32; 16],
            render: [u32; 3],
            pos: &'static [[u32; 3]],
            trails: &'static [Option<[u32; 3]>],
            sprites: Option<&'static [[u32; 8]]>,
            /// LightsModule: the light's range (None: no light) and m_Range.
            light: Option<(Option<u32>, u32)>,
            out: [u32; 6],
        }
        /// The procedural inputs: lifetime, speed, shape (type, radius, position, rotation, scale, randomDirection),
        /// emitterScale, Physics.gravity, the gravity modifier and the velocity module (axes, worldSpace).
        struct Pr {
            life: (i16, u32, u32),
            speed: (i16, u32, u32),
            shape: Option<(i32, u32, [u32; 3], [u32; 3], [u32; 3], u32)>,
            emitter: [u32; 3],
            gravity: [u32; 3],
            modifier: u32,
            velocity: Option<([(i16, u32, u32); 3], bool)>,
        }
        use crate::assets::particles::SizeOverLifetime;
        let f = f32::from_bits;
        let v = |b: [u32; 3]| b.map(f);
        let mc = |(state, lo, hi): (i16, u32, u32)| MinMaxCurve { state, scalar: f(hi), min_scalar: f(lo), ..Default::default() };
        let plain: &[K] = &include!("game/particle_bounds_dll.rs");
        let procedural: &[(K, Pr)] = &include!("game/particle_bounds_procedural_dll.rs");
        let cases = plain.iter().map(|k| (k, None)).chain(procedural.iter().map(|(k, p)| (k, Some(p))));
        for (i, (k, pr)) in cases.enumerate() {
            let c: Vec<MinMaxCurve> = k.curves.iter().map(|&t| mc(t)).collect();
            let sol = |o: usize| SizeOverLifetime { x: c[o].clone(), y: c[o + 1].clone(), z: c[o + 2].clone(), separate: false };
            let ps = ParticleSystem {
                space: k.world as i32,
                start_size_y: k.uses3d.then(|| c[1].clone()),
                start_raw_3d: [c[1].clone(), c[2].clone(), Default::default(), Default::default()],
                size: k.size_on.then(|| sol(3)),
                size_by_speed: k.sbs_on.then(|| (sol(6), [0.0; 2])),
                trail: k.trail.map(|world_space| crate::assets::particles::Trail { mode: 0, world_space, ..Default::default() }),
                lights: k.light.is_some(),
                lights_range: k.light.map_or(0.0, |l| f(l.1)),
                ..Default::default()
            };
            let mut ps = ps;
            let procedural = pr.map(|p| {
                ps.start_lifetime = mc(p.life);
                ps.start_speed = mc(p.speed);
                ps.gravity = MinMaxCurve { scalar: f(p.modifier), ..Default::default() };
                if let Some((ty, radius, position, rotation, scale, random)) = p.shape {
                    ps.shape_enabled = true;
                    ps.shape_type = ty;
                    ps.shape_radius.value = f(radius);
                    (ps.shape_position, ps.shape_rotation, ps.shape_scale) = (v(position), v(rotation), v(scale));
                    ps.shape_random_direction = f(random);
                }
                ps.velocity = p.velocity.map(|(axes, world)| crate::assets::particles::Velocity { axes: axes.map(mc), world, ..Default::default() });
                (v(p.emitter), v(p.gravity))
            });
            let sprites: Option<Vec<[f32; 8]>> = k.sprites.map(|s| s.iter().map(|e| e.map(f)).collect());
            let pos: Vec<[f32; 3]> = k.pos.iter().map(|&p| v(p)).collect();
            let trails: Vec<Option<[f32; 3]>> = k.trails.iter().map(|q| q.map(v)).collect();
            let got = min_max_aabb(&ps, v(k.pivot), sprites.as_deref(), k.light.and_then(|l| l.0).map(f), &c[0], &pos, &trails, f(k.max_size), v(k.t), &k.w2l.map(f), v(k.render), procedural);
            assert_eq!(got.map(f32::to_bits), k.out, "case {i}");
        }
    }

    /// Every game system through the rewritten start and module code for a few frames: the ones that reach an
    /// `unimplemented!` are listed (the native pieces still missing).
    #[test]
    #[ignore]
    fn every_system_runs_the_modules() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut g = Game::default();
        g.world.gravity = crate::b2::math::Vec2 { x: 0.0, y: -9.81 };
        let mut missing = std::collections::BTreeMap::<String, usize>::new();
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        g.time.fixedDeltaTime = 0.02;
        // The system and its sub-emitter systems, depth first.
        fn add(db: &crate::assets::db::AssetDb, file: usize, ps: ParticleSystem, systems: &mut Vec<Sys>) -> usize {
            let i = systems.len();
            let entries = ps.subs.clone();
            systems.push(Sys { game_layer: 0, fancy_only: false, awake: true, hierarchy: 1.0, local: 1.0, size_range: [0.0, 1.0], alignment: 0, allow_roll: false, stretch: None, ps, subs: Vec::new(), rotation: Quat::IDENTITY, prefab: None, local_rotation: Quat::IDENTITY, pivot: Vec3::ZERO, flip: Vec2::ZERO, linear: false, frames: Vec::new(), go: None, sheet_texture: None, max_step: 0.1, order: 0, first_cull: Cull::Never });
            let mut subs = Vec::new();
            for e in entries {
                let Some(c) = db.resolve(file, e.emitter) else { continue };
                let Ok(q) = ParticleSystem::read(db.data(c)) else { continue };
                subs.push((add(db, c.file, q, systems), e.kind, e.properties, e.probability));
            }
            systems[i].subs = subs;
            i
        }
        for (fi, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let ps = ParticleSystem::read(f.data(o)).unwrap();
                let mut systems = Vec::new();
                add(&db, fi, ps, &mut systems);
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut rng = Rng(1);
                    let mut run = start(0, Vec3::new(1.0, 2.0, 0.0), Quat::from_rotation_z(0.3), 1.0, &systems, &mut Manager { rand: Rand::new(1), active: Vec::new(), inactive: Vec::new(), next: 0, groups: 0, invisible: Vec::new(), nodes: Vec::new(), pooled: Default::default(), ghosts: HashMap::new() });
                    for k in 0..30 {
                        run.position.x += 0.01 * k as f32;
                        step(&mut run, &systems, None, &g, 1.0 / 60.0, &mut rng);
                        // ParticleSystem.Emit(EmitParams) with a position, as the scripts call it.
                        if k % 10 == 0 {
                            emit_external(&mut run, &systems, &g, &EmitParams { position: Some(Vec3::new(1.0, 2.0, 0.0)), ..Default::default() }, &mut rng);
                        }
                    }
                }));
                if let Err(e) = r {
                    let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    *missing.entry(msg.chars().take(120).collect()).or_insert(0) += 1;
                    let go = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|p| db.resolve(fi, p)).and_then(|r| db.game_object(r).ok()).map(|g| g.name).unwrap_or_default();
                    println!("PANIC {fi} {go}: {}", msg.chars().take(60).collect::<String>());
                }
            }
        }
        std::panic::set_hook(hook);
        for (k, v) in &missing {
            println!("{v:5} {k}");
        }
    }

    /// The sub-emitter entries the game uses and what their systems read.
    #[test]
    #[ignore]
    fn noise_inventory() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut seen = std::collections::BTreeMap::<String, usize>::new();
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let ps = ParticleSystem::read(f.data(o)).unwrap();
                let Some(n) = &ps.noise else { continue };
                let st = |c: &MinMaxCurve| c.state;
                let k = format!(
                    "q{} oct{} om{} os{} sep{} damp{} remap{} str{:?} scroll{}:{} amt{:?} space{} freq{} posamt{}",
                    n.quality, n.octaves, n.octave_multiplier, n.octave_scale, n.separate as u8, n.damping as u8, n.remap_enabled as u8,
                    n.strength.iter().map(st).collect::<Vec<_>>(), n.scroll_speed.state, n.scroll_speed.scalar,
                    n.amounts.iter().map(|c| (c.state, c.scalar)).collect::<Vec<_>>(), ps.space, n.frequency, ps.size.is_some() as u8
                );
                *seen.entry(k).or_default() += 1;
            }
        }
        for (k, n) in seen {
            println!("{n:4} {k}");
        }
    }

    #[test]
    #[ignore]
    fn sub_inventory() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut seen = std::collections::BTreeMap::<String, usize>::new();
        for (fi, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let ps = ParticleSystem::read(f.data(o)).unwrap();
                for sub in ps.subs.iter() {
                    let Some(c) = db.resolve(fi, sub.emitter) else { continue };
                    let Ok(q) = ParticleSystem::read(db.data(c)) else { continue };
                    let k = format!(
                        "kind {} props {} prob {} | parent space {} | sub space {} loop {} delay st {} life st {} rate st {} {} dist {} bursts {} max {} subs {} inherit {}",
                        sub.kind, sub.properties, sub.probability, ps.space, q.space, q.looping, q.start_delay.state, q.start_lifetime.state, q.rate_over_time.state, q.rate_over_time.scalar, q.rate_over_distance.scalar, q.bursts.len(), q.max_particles, q.subs.len(), q.inherit.is_some()
                    ) + &format!(" | parent color {} cbs {} size {} sbs {} 3d {}", ps.color.is_some(), ps.color_by_speed.is_some(), ps.size.is_some(), ps.size_by_speed.is_some(), uses_3d_size(&ps));
                    *seen.entry(k).or_insert(0) += 1;
                }
            }
        }
        for (k, v) in &seen {
            println!("{v:4} {k}");
        }
    }

    #[test]
    fn kills_go_by_batches_of_four() {
        // Lanes 3 and 1 die; 3 takes the dead last particle, so the batch is looked at again and 3 dies once more.
        let dead = vec![false, true, false, true, false, true];
        assert_eq!(kill_order(dead), vec![3, 1, 3]);
        assert_eq!(kill_order(vec![true; 5]), vec![3, 2, 1, 0, 0]);
    }

    #[test]
    fn curves_interpolate() {
        use crate::assets::particles::Keyframe;
        let k = |time, value| Keyframe { time, value, in_slope: 0.0, out_slope: 0.0, weighted_mode: 0, in_weight: 0.0, out_weight: 0.0 };
        let c = Curve { keys: vec![k(0.0, 0.0), k(1.0, 2.0)], ..Default::default() };
        assert_eq!((curve(&c, -1.0), curve(&c, 0.5), curve(&c, 2.0)), (0.0, 1.0, 2.0));
    }
}
