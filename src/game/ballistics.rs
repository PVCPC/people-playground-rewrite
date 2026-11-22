//! BallisticsEmitter (the bullets of FirearmBehaviour and GlobalShrapnelEmitter), BulletTracerPool and
//! CameraShakeBehaviour.
#![allow(non_snake_case)]

use super::destroyable::{Coroutine, Step};
use super::physical::dot2;
use super::{d, mathf, Behaviour, Game};
use crate::b2::math::Vec2 as V;

/// Cartridge (ScriptableObject): the values the shot reads.
#[derive(Clone, Copy, Debug)]
pub struct Cartridge {
    pub Damage: f32,
    pub Recoil: f32,
    pub ImpactForce: f32,
    pub StartSpeed: f32,
    pub PenetrationRandomAngleMultiplier: f32,
}

impl Default for Cartridge {
    fn default() -> Self {
        Self {
            Damage: 1.0,
            Recoil: 1.0,
            ImpactForce: 1.0,
            StartSpeed: 100.0,
            PenetrationRandomAngleMultiplier: 0.1,
        }
    }
}

const WATER_LAYER: u32 = 1 << 4;

/// ShotMessagePropagator: its component id and Targets (each a component id on the target GameObject).
#[derive(Clone, Debug)]
pub struct ShotMessagePropagator {
    pub id: i32,
    pub targets: Vec<i32>,
}

/// The classes with a Shot(Shot) / ExitShot(Shot) method, and their subclasses.
pub(super) const SHOT_RECEIVERS: &[&str] = &[
    "PhysicalBehaviour", "DestroyableBehaviour", "ExplosiveBehaviour", "DamagableMachineryBehaviour", "TyreDeflationBehaviour", "ActOnShot", "BalloonBehaviour",
    "BreakJointOnShotBehaviour", "BulbBehaviour", "CathodeLightBehaviour", "CirculationBehaviour", "DebugTargetDamageMeter", "DroneBehaviour", "FireworksBehaviour",
    "GlowtubeBehaviour", "JavelinMissileBehaviour", "LimbBehaviour", "LiquidContainerLeakingBehaviour", "OldSyringeBehaviour", "PersonBehaviour",
    "SentryTurretBehaviour", "ShotMessagePropagator", "SyringeBehaviour", "TelevisionBehaviour", "WaterImpactBehaviour", "VaseBehaviour",
    "AcidSyringe", "AdrenalineSyringe", "BoneEatingPoisonSyringe", "CoagulationSyringe", "DeathSyringe", "EmptySyringe", "FreezeSyringe", "KnockoutSyringe", "LifeSyringe",
    "MendingSyringe", "PainKillerSyringe", "PinkSyringe", "UltraStrengthSyringe", "WaterBreathingSyringe", "ZombieSyringe",
];
/// The classes with a Break(Vector2) or Break() method, and their subclasses.
const BREAK_RECEIVERS: &[&str] = &[
    "BalloonBehaviour", "BulbBehaviour", "CathodeLightBehaviour", "DamagableMachineryBehaviour", "DestroyableBehaviour", "VaseBehaviour", "GlowtubeBehaviour",
    "OldSyringeBehaviour", "SyringeBehaviour", "TelevisionBehaviour", "TyreDeflationBehaviour",
    "AcidSyringe", "AdrenalineSyringe", "BoneEatingPoisonSyringe", "CoagulationSyringe", "DeathSyringe", "EmptySyringe", "FreezeSyringe", "KnockoutSyringe", "LifeSyringe",
    "MendingSyringe", "PainKillerSyringe", "PinkSyringe", "UltraStrengthSyringe", "WaterBreathingSyringe", "ZombieSyringe",
];
const EXIT_SHOT_RECEIVERS: &[&str] = &[
    "PhysicalBehaviour", "BalloonBehaviour", "CirculationBehaviour", "LimbBehaviour", "LiquidContainerLeakingBehaviour", "OldSyringeBehaviour", "SyringeBehaviour",
    "AcidSyringe", "AdrenalineSyringe", "BoneEatingPoisonSyringe", "CoagulationSyringe", "DeathSyringe", "EmptySyringe", "FreezeSyringe", "KnockoutSyringe", "LifeSyringe",
    "MendingSyringe", "PainKillerSyringe", "PinkSyringe", "UltraStrengthSyringe", "WaterBreathingSyringe", "ZombieSyringe",
];

impl Cartridge {
    /// Cartridge..ctor (Damage 1, Recoil 1, ImpactForce 1, StartSpeed 100, PenetrationRandomAngleMultiplier 0.1), then the
    /// asset's serialized fields, which hold every one of them.
    pub fn read(v: &crate::assets::mono::Value) -> Result<Self, String> {
        let f = |name: &str| v.get(name).and_then(crate::assets::mono::Value::f32).ok_or_else(|| format!("Cartridge.{name}"));
        Ok(Self { Damage: f("Damage")?, Recoil: f("Recoil")?, ImpactForce: f("ImpactForce")?, StartSpeed: f("StartSpeed")?, PenetrationRandomAngleMultiplier: f("PenetrationRandomAngleMultiplier")? })
    }
}

/// A BallisticsEmitter's ConnectedBehaviour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shooter {
    /// A FirearmBehaviour (`Game::firearms`).
    Firearm(usize),
    /// A MachineGunBehaviour (`Game::machine_guns`).
    MachineGun(usize),
    /// The GlobalShrapnelEmitter.
    Shrapnel,
}

/// A BallisticsEmitter: a FirearmBehaviour's or MachineGunBehaviour's (made in Start) or the GlobalShrapnelEmitter's.
#[derive(Default, Clone, Debug)]
pub struct Emitter {
    pub cartridge: Cartridge,
    pub MaxBallisticsIterations: u32,
    pub MaxTotalDistance: f32,
    pub RicochetChanceMultiplier: f32,
    pub RenderTracersIfTraversal: bool,
    /// BulletRicochetCallback is FirearmBehaviour.OnRicochet.
    pub ricochet_callback: bool,
    pub LayersToHit: u32,
    pub WaterLayer: u32,
    pub MaxRange: f32,
    pub DoTraversal: bool,
    pub Charge: f32,
    pub lastCrackPlayTime: f32,
    /// `tracers`: how many of the pool's tracers it holds (which ones is rendering); None once disposed (`tracers = null`).
    pub tracers: Option<usize>,
    pub ExplosiveRound: bool,
    pub ExplosiveRoundParams: super::explosion::ExplosionParameters,
    pub ExplodeAtHitPoint: bool,
    pub ThicknessStepSize: f32,
    pub MaterialHardnessMultiplier: f32,
    pub ImpactForceMultiplier: f32,
    pub BulletDropMultiplier: f32,
    pub BulletCrackSpeedThreshold: f32,
    pub BulletCrackChance: f32,
    pub BulletCrackCooldownTime: f32,
    /// Rendering only (the tracers' widthMultiplier); kept for its getter and setter.
    pub TracerWidth: f32,
}

impl Emitter {
    pub fn new(cartridge: Cartridge, layers: u32, water: u32) -> Self {
        Self {
            cartridge,
            MaxBallisticsIterations: 128,
            MaxTotalDistance: 1000.0,
            RicochetChanceMultiplier: 1.0,
            RenderTracersIfTraversal: true,
            ricochet_callback: false,
            LayersToHit: layers,
            WaterLayer: water,
            MaxRange: 1000.0,
            DoTraversal: false,
            Charge: 0.0,
            lastCrackPlayTime: f32::MIN,
            tracers: Some(0),
            ExplosiveRound: false,
            ExplosiveRoundParams: Default::default(),
            ExplodeAtHitPoint: false,
            ThicknessStepSize: 0.08,
            MaterialHardnessMultiplier: 0.35,
            ImpactForceMultiplier: 18.0,
            BulletDropMultiplier: 0.25,
            BulletCrackSpeedThreshold: 5.0,
            BulletCrackChance: 0.8,
            BulletCrackCooldownTime: 0.25,
            TracerWidth: 0.02,
        }
    }
}

/// The level1 GlobalShrapnelEmitter (GlobalShrapnelEmitter.Instance), when its Awake ran.
#[derive(Clone, Debug)]
pub struct ShrapnelEmitter {
    pub emitter: Emitter,
    /// GlobalShrapnelEmitter.Instance is this one (OnDestroy clears it; the component and its disposed emitter stay).
    pub instance: bool,
}

/// GlobalShrapnelEmitter.Awake: a BallisticsEmitter of the Cartridge with the shrapnel's settings; the caller sets
/// `Game::shrapnel` (Instance).
pub fn global_shrapnel_awake(cartridge: Cartridge, layers_to_hit: u32, water_layer: u32) -> ShrapnelEmitter {
    let mut e = Emitter::new(cartridge, layers_to_hit, water_layer);
    e.RenderTracersIfTraversal = false;
    e.DoTraversal = true;
    e.MaxBallisticsIterations = 16;
    e.RicochetChanceMultiplier = 4.0;
    e.MaxTotalDistance = 30.0;
    ShrapnelEmitter { emitter: e, instance: true }
}

/// The coroutine owner of the GlobalShrapnelEmitter: a scene object that is never destroyed.
pub const SHRAPNEL_OWNER: i32 = i32::MIN;

/// The arguments of one BallisticIteration.
#[derive(Clone, Copy, Debug)]
pub struct Bullet {
    pub shooter: Shooter,
    pub origin: [f32; 2],
    pub direction: [f32; 2],
    pub speed: f32,
    pub iteration: i32,
    pub total: f32,
    pub previous: Option<i32>,
    pub tracer: bool,
    /// Rendering only: the tracer's line in `Game::lines` (tracer_line_take).
    pub line: Option<usize>,
    pub splash: bool,
    pub can_explode: bool,
    pub charge: f32,
}

/// The locals of a BallisticIteration in its WaitForSeconds(0.01) after a raycast hit.
#[derive(Clone, Copy, Debug)]
pub struct BulletHit {
    pub physical: usize,
    pub hardness: f32,
    pub damage: f32,
    pub collider: i32,
    pub point: [f32; 2],
    pub normal: [f32; 2],
    pub distance: f32,
}

/// The scene's BulletTracerPool (ObjectPoolBehaviour of "Tracer poolable"): its clones are render-only, so the
/// counts are what matters (instance ids, MaxPoolSize).
#[derive(Clone, Debug, Default)]
pub struct TracerPool {
    pub created: usize,
    pub available: usize,
    pub inactivityTimer: f32,
    pub MaxPoolSize: usize,
    pub InactivityThresholdInSeconds: f32,
    /// The ids an Instantiate of the prefab takes.
    pub clone_ids: usize,
    /// Rendering only: the prefab's LineRenderer (its root's scale in its chain), each clone's line in `Game::lines`:
    /// the inactive ones, and the active ones with the emitter holding them.
    pub line: Option<super::line::LineVisual>,
    pub free: Vec<usize>,
    pub held: Vec<(usize, Shooter)>,
}

/// Global.main.camera and CameraShakeBehaviour.
#[derive(Clone, Debug)]
pub struct CameraState {
    pub shakeIntensity: f32,
    /// transform.root.position, which CameraShakeBehaviour.Update writes.
    pub root: [f32; 3],
    /// The camera's world rotation: CameraShakeBehaviour.Update's transform.eulerAngles (0, 0, z) under the unrotated root.
    pub rotation: [f32; 4],
    /// The Camera's orthographicSize as level1 sets it (headless).
    pub orthographic_size: f32,
    /// CameraListenerBehaviour.NormalisedDistance (static; Update sets it).
    pub normalised_distance: f32,
    /// The camera's world z (the AudioListener sits there).
    pub z: f32,
    /// The scene's CameraListenerBehaviour.
    pub listener: Option<CameraListener>,
    /// The main camera's AudioReverbFilter parameters (`crate::assets::components::AudioReverbFilter::params`), the
    /// listener's filter `crate::audio_out` runs; CameraListenerBehaviour.Update writes dryLevel and room.
    pub reverb: Option<[f32; 14]>,
    /// Behaviour.enabled of that filter: only an enabled one has its DSP.
    pub reverb_enabled: bool,
    /// Its m_ReverbPreset (27 User): every property setter's Update runs ChangeProperties with it.
    pub reverb_preset: i32,
}

/// CameraListenerBehaviour's fields.
#[allow(non_snake_case)]
#[derive(Clone, Debug)]
pub struct CameraListener {
    pub orthosizeMin: f32,
    pub orthosizeMax: f32,
    pub masterVolumeCurve: crate::game_assets::Curve,
    pub dryVolumeCurve: crate::game_assets::Curve,
    pub muted: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self { shakeIntensity: 0.0, root: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], orthographic_size: 5.0, normalised_distance: 0.0, z: -10.0, listener: None, reverb: None, reverb_enabled: false, reverb_preset: 27 }
    }
}

/// BallisticsEmitter.SpeedMultiplierByDistance: 1f / Mathf.Max(0.1f, Mathf.Sqrt(d * 0.2f)).
pub(crate) fn speed_multiplier_by_distance(distance: f32) -> f32 {
    1.0 / mathf::max(0.1, mathf::sqrt((d(distance) * 0.2f32).f()))
}

/// BallisticsEmitter.DamageMultiplierByDistance: Mathf.Clamp(1.5f / d + 0.7f / Mathf.Lerp(d, 0.8f, 0.992f), 0.5f, 10f).
pub(crate) fn damage_multiplier_by_distance(distance: f32) -> f32 {
    mathf::clamp((d(1.5f32) / distance + d(0.7f32) / mathf::lerp(distance, 0.8, 0.992)).f(), 0.5, 10.0)
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] * s, a[1] * s]
}

impl Game {
    // ---- Camera ---------------------------------------------------------------------------------------------------

    /// Global.main.camera.orthographicSize (CameraControlBehaviour.LateUpdate sets it).
    pub fn camera_orthographic_size(&self) -> f32 {
        self.camera.orthographic_size
    }

    /// Global.main.camera.transform.position (xy): the shaken root plus the camera's place under it.
    pub fn camera_position(&self) -> [f32; 2] {
        let at = self.camera_control.position;
        [self.camera.root[0] + at[0], self.camera.root[1] + at[1]]
    }

    /// The AudioListener's world position: on the main camera, at the camera's world z.
    pub fn audio_listener(&self) -> [f32; 3] {
        let c = self.camera_position();
        [c[0], c[1], self.camera.z]
    }

    /// The main camera's world position (the shaken root plus its place under it) and rotation.
    pub fn camera_world(&self) -> ([f32; 3], [f32; 4]) {
        let (r, at) = (self.camera.root, self.camera_control.position);
        ([r[0] + at[0], r[1] + at[1], r[2] + at[2]], self.camera.rotation)
    }

    /// CameraListenerBehaviour.Update: NormalisedDistance = (Clamp(orthographicSize, orthosizeMin, orthosizeMax) - min) / (max - min),
    /// AudioListener.volume = muted ? 0 : masterVolumeCurve(NormalisedDistance), reverbFilter.dryLevel =
    /// (1 - dryVolumeCurve(NormalisedDistance)) * -5000 and reverbFilter.room = dryVolumeCurve(NormalisedDistance) * -1000 - 1000
    /// (`crate::audio_out` runs the filter). Without the scene's behaviour (5 and 200) the volume stays.
    pub fn camera_listener_update(&mut self) {
        let (min, max) = self.camera.listener.as_ref().map_or((5.0, 200.0), |l| (l.orthosizeMin, l.orthosizeMax));
        self.camera.normalised_distance = (d(mathf::clamp(self.camera_orthographic_size(), min, max)) - min).f() / (max - min);
        if let Some(l) = &self.camera.listener {
            let n = self.camera.normalised_distance;
            self.audio.listener_volume = if l.muted { 0.0 } else { l.masterVolumeCurve.evaluate(&mut Default::default(), n) };
            let dry = l.dryVolumeCurve.evaluate(&mut Default::default(), n);
            let r = self.camera.reverb.as_mut().unwrap_or_else(|| super::menu::throw("NullReferenceException: CameraListenerBehaviour.reverbFilter"));
            r[0] = ((d(1.0) - dry) * -5000.0f32).f();
            r[1] = (d(dry) * -1000.0f32 - 1000.0f32).f();
            crate::sfx_reverb::change_properties(r, self.camera.reverb_preset);
        }
    }

    /// CameraListenerBehaviour.Mute / Unmute.
    pub fn camera_listener_mute(&mut self, muted: bool) {
        if let Some(l) = &mut self.camera.listener {
            l.muted = muted;
        }
    }

    /// CameraShakeBehaviour.FixedUpdate
    pub fn camera_shake_fixed_update(&mut self) {
        self.camera.shakeIntensity *= 0.85;
    }

    /// CameraShakeBehaviour.Update: the root's position and the camera's eulerAngles z (the root's own rotation does not move it).
    /// 1.28.53 writes shakeTarget, the transform.root Awake keeps; the camera is never re-parented, so it is the same root.
    pub fn camera_shake_update(&mut self) {
        let i = self.camera.shakeIntensity;
        let i = mathf::clamp(if i.is_nan() { 0.0 } else { i }, 0.0, 5.0);
        self.camera.shakeIntensity = i;
        let t = self.time.time;
        // CameraShakeBehaviour.Noise
        let noise = |s: f32| (d(mathf::perlin(t * 24.0, s)) * 2.0f32 - 1.0f32).f();
        let num = (d(noise(1000.0)) * i * 0.1f32).f();
        let y = (d(mathf::sin(t * 80.0)) * i * 0.04f32).f();
        let num2 = (d(noise(0.0)) * i * 0.25f32).f();
        let (num, num2) = (num * num, num2 * num2);
        let s = (d(0.5f32) * self.prefs.ShakeIntensity).f();
        self.camera.root = [s * num, s * y, s * 0.0];
        self.camera.rotation = super::menu::euler_z_rotation(s * num2);
    }

    /// CameraShakeBehaviour.Shake(intensity, position)
    pub fn camera_shake(&mut self, intensity: f32, position: [f32; 2]) {
        self.camera_shake_at(intensity, position, 1.0);
    }

    /// CameraShakeBehaviour.Shake(intensity, position, distanceInfluence)
    pub fn camera_shake_at(&mut self, intensity: f32, position: [f32; 2], distance_influence: f32) {
        let nd = self.camera.normalised_distance;
        let num = (1.0 - d(nd) * 0.5f32).f();
        let num2 = mathf::max(1.0, mathf::magnitude2(sub(position, self.camera_position())) * distance_influence);
        self.camera.shakeIntensity = (d(self.camera.shakeIntensity) + d(intensity) * num / num2).f();
    }

    // BulletTracerPool.Awake: Instance = this; `tracer_pool` is that one pool (BulletTracerPool.get_Instance and set_Instance are the field)
    // ---- BulletTracerPool -----------------------------------------------------------------------------------------

    /// ObjectPoolBehaviour.Request: false when the pool is full.
    pub(crate) fn tracer_request(&mut self) -> bool {
        let pool = &mut self.tracer_pool;
        pool.inactivityTimer = 0.0;
        if pool.available > 0 {
            pool.available -= 1;
            return true;
        }
        if pool.created >= pool.MaxPoolSize {
            return false;
        }
        pool.created += 1;
        for _ in 0..self.tracer_pool.clone_ids {
            self.allocate_instance_id();
        }
        true
    }

    /// Rendering: a tracer clone's line for a Request at `origin` (a new clone's rotation is identity, a reused one keeps
    /// its last; PoolableSizeHelper puts back the prefab's scale), with CreateTracer's widthMultiplier and
    /// SetPosition(1, right * MaxRange). Its Renderer.enabled is the clone's last one.
    fn tracer_line_take(&mut self, f: Shooter, origin: [f32; 2], width: f32, range: f32) -> Option<usize> {
        let template = self.tracer_pool.line.clone()?;
        let l = match self.tracer_pool.free.pop() {
            Some(l) => l,
            None => {
                let mut line = template.clone();
                line.id = i32::MIN / 2 + (self.tracer_pool.held.len() + self.tracer_pool.free.len()) as i32;
                self.lines.push(line);
                self.lines.len() - 1
            }
        };
        self.tracer_pool.held.push((l, f));
        let line = &mut self.lines[l];
        line.visible = true;
        line.width = width;
        line.chain[0].t = [origin[0], origin[1], 0.0, 0.0];
        line.chain[0].s = template.chain[0].s;
        if line.positions.len() >= 2 {
            line.positions[1] = [range, 0.0, 0.0];
        }
        Some(l)
    }

    /// Rendering: ObjectPoolBehaviour.Return of a tracer clone (SetActive(false)).
    fn tracer_line_give(&mut self, l: usize) {
        if let Some(i) = self.tracer_pool.held.iter().position(|h| h.0 == l) {
            self.tracer_pool.held.swap_remove(i);
            self.tracer_pool.free.push(l);
            self.lines[l].visible = false;
        }
    }

    /// Rendering: tracer.enabled.
    fn tracer_line_enable(&mut self, b: &Bullet, on: bool) {
        if let Some(l) = b.line.filter(|l| self.tracer_pool.held.iter().any(|h| h.0 == *l)) {
            self.lines[l].enabled = on;
        }
    }

    /// Rendering: tracer.transform.position = origin; tracer.transform.right = direction; SetPosition(1, right * length).
    fn tracer_line_place(&mut self, b: &Bullet, origin: [f32; 2], direction: [f32; 2], length: f32) {
        if let Some(l) = b.line.filter(|l| self.tracer_pool.held.iter().any(|h| h.0 == *l)) {
            let half = direction[1].atan2(direction[0]) * 0.5;
            let line = &mut self.lines[l];
            line.chain[0].t = [origin[0], origin[1], 0.0, 0.0];
            line.chain[0].q = [0.0, 0.0, half.sin(), half.cos()];
            if line.positions.len() >= 2 {
                line.positions[1] = [length, 0.0, 0.0];
            }
        }
    }

    /// ObjectPoolBehaviour.Return(poolable): `is_clone` is createdObjects.Contains(poolable) (and poolable alive); otherwise
    /// Debug.LogError("Invalid object being returned to the pool"), console only. SetActive(false) is the caller's.
    pub(crate) fn tracer_return(&mut self, is_clone: bool) {
        if !is_clone {
            return;
        }
        self.tracer_pool.available += 1;
    }

    /// ObjectPoolBehaviour.Update of the BulletTracerPool
    pub fn tracer_pool_update(&mut self) {
        let dt = self.time.deltaTime;
        let pool = &mut self.tracer_pool;
        if pool.created != 0 {
            pool.inactivityTimer += dt;
            if pool.inactivityTimer > pool.InactivityThresholdInSeconds && pool.available == pool.created {
                // Clear: Destroy each clone.
                pool.created = 0;
                pool.available = 0;
                pool.inactivityTimer = 0.0;
            }
        }
    }

    /// GlobalShrapnelEmitter.EmitShrapnel: StartCoroutine(EmitRoutine); GlobalShrapnelEmitter.EmitRoutine itself runs as Step::ShrapnelEndOfFrame then Step::ShrapnelEmit (destroyable.rs).
    pub fn global_shrapnel_emit_shrapnel(&mut self, position: [f32; 2], amount: i32) {
        self.coroutine_seq += 1;
        let step = super::destroyable::Step::ShrapnelEndOfFrame(position, amount);
        self.end_of_frame.push(super::destroyable::Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, from: i64::MIN, owner: SHRAPNEL_OWNER, step });
    }

    /// GlobalShrapnelEmitter.OnDestroy: BallisticsEmitter.Dispose (its tracers go back, then tracers = null: a second
    /// Dispose throws in the foreach), then Instance = null.
    pub fn global_shrapnel_on_destroy(&mut self) {
        if let Some(s) = self.shrapnel.as_mut() {
            let n = s.emitter.tracers.take().unwrap_or_else(|| super::menu::npe());
            s.instance = false;
            for _ in 0..n {
                self.tracer_return(true);
            }
        }
    }

    /// BallisticsEmitter.Dispose (OnDestroy of its ConnectedBehaviour; none before its Start): the tracers it still
    /// holds go back.
    pub fn emitter_dispose(&mut self, f: Shooter) {
        let held: Vec<usize> = self.tracer_pool.held.iter().filter(|h| h.1 == f).map(|h| h.0).collect();
        for l in held {
            self.tracer_line_give(l);
        }
        let e = match f {
            Shooter::Firearm(f) => self.firearms[f].emitter.as_mut(),
            Shooter::MachineGun(m) => self.machine_guns[m].emitter.as_mut(),
            Shooter::Shrapnel => unreachable!(),
        };
        if let Some(e) = e {
            // `foreach (... in tracers)`: a second Dispose finds it null.
            let n = e.tracers.take().unwrap_or_else(|| super::menu::npe());
            for _ in 0..n {
                self.tracer_return(true);
            }
        }
    }

    // ---- BallisticsEmitter ----------------------------------------------------------------------------------------

    pub(crate) fn emitter(&self, f: Shooter) -> &Emitter {
        match f {
            Shooter::Firearm(f) => self.firearms[f].emitter.as_ref().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object")),
            Shooter::MachineGun(m) => self.machine_guns[m].emitter.as_ref().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object")),
            Shooter::Shrapnel => &self.shrapnel.as_ref().expect("the GlobalShrapnelEmitter").emitter,
        }
    }

    fn emitter_mut(&mut self, f: Shooter) -> &mut Emitter {
        match f {
            Shooter::Firearm(f) => self.firearms[f].emitter.as_mut().unwrap(),
            Shooter::MachineGun(m) => self.machine_guns[m].emitter.as_mut().unwrap(),
            Shooter::Shrapnel => &mut self.shrapnel.as_mut().expect("the GlobalShrapnelEmitter").emitter,
        }
    }

    /// The ConnectedBehaviour's instance id (its coroutines' owner).
    fn emitter_owner(&self, f: Shooter) -> i32 {
        match f {
            Shooter::Firearm(f) => self.firearms[f].id,
            Shooter::MachineGun(m) => self.machine_guns[m].id,
            Shooter::Shrapnel => SHRAPNEL_OWNER,
        }
    }

    /// BallisticsEmitter.Emit(origin, direction)
    pub fn emitter_emit(&mut self, f: Shooter, origin: [f32; 2], direction: [f32; 2]) {
        let e = self.emitter(f);
        let (traversal, charge, speed) = (e.DoTraversal && e.RenderTracersIfTraversal, e.Charge, e.cartridge.StartSpeed);
        let (width, range) = (e.TracerWidth, e.MaxRange);
        let (mut tracer, mut line) = (false, None);
        // BallisticsEmitter.CreateTracer: BulletTracerPool.Instance.Request; OnTracerCreation is rendering.
        if traversal && self.tracer_request() {
            *self.emitter_mut(f).tracers.as_mut().unwrap_or_else(|| super::menu::npe()) += 1;
            tracer = true;
            line = self.tracer_line_take(f, origin, width, range);
        }
        let b = Bullet { shooter: f, origin, direction: mathf::normalized2(direction), speed, iteration: 0, total: 0.0, previous: None, tracer, line, splash: true, can_explode: true, charge };
        self.bullet_iterate(b);
    }

    /// The transform.root of a physical's GameObject, as its Transform id.
    pub(crate) fn root_id(&self, p: usize) -> i32 {
        match self.root_of(p) {
            (true, n) => self.transform_nodes[n].id,
            (false, p) => self.physicals[p].transform_id,
        }
    }

    fn remove_tracer(&mut self, b: &Bullet) {
        if let Some(l) = b.line {
            self.tracer_line_give(l);
        }
        if b.tracer {
            *self.emitter_mut(b.shooter).tracers.as_mut().unwrap_or_else(|| super::menu::npe()) -= 1;
            self.tracer_return(true);
        }
    }

    /// BallisticsEmitter.get_RandomisedAngle: Random.insideUnitCircle * Cartridge.PenetrationRandomAngleMultiplier
    pub(crate) fn bullet_random_angle(&mut self, f: Shooter) -> [f32; 2] {
        let m = self.emitter(f).cartridge.PenetrationRandomAngleMultiplier;
        mul(self.random.inside_unit_circle(), m)
    }

    /// BallisticsEmitter.GetDamageForDistance: Cartridge.Damage * DamageMultiplierByDistance(totalDistance)
    pub(crate) fn damage_for_distance(&self, f: Shooter, distance: f32) -> f32 {
        self.emitter(f).cartridge.Damage * damage_multiplier_by_distance(distance)
    }

    /// BallisticsEmitter.CalculateHardness
    pub(crate) fn hardness(&self, f: Shooter, p: usize) -> f32 {
        let m = self.emitter(f).MaterialHardnessMultiplier;
        let ph = &self.physicals[p];
        let props = &self.properties[ph.Properties];
        let num = 1.0 - mathf::clamp01(props.Softness + props.Brittleness);
        let num2 = (d(2.0f32) * m * (d(4.5f32) * num) + props.BulletSpeedAbsorptionPower).f();
        if !ph.BulletDeflectChanceOverride.is_nan() {
            return num2 + mathf::max(ph.BulletDeflectChanceOverride, 0.0);
        }
        num2
    }

    /// PhysicalObjectsInWorldByTransform.TryGetValue(transform) for a transform that is the collider's own (overlap)
    /// or its attached Rigidbody2D's (raycast), with the root check before it. The GlobalShrapnelEmitter's root is a
    /// level1 object, never a physical's root.
    fn bullet_target(&self, f: Shooter, p: Option<usize>) -> Option<usize> {
        let root = match f {
            Shooter::Firearm(f) => Some(self.firearm_root_id(f)),
            Shooter::MachineGun(m) => Some(self.root_id(self.machine_guns[m].physical)),
            Shooter::Shrapnel => None,
        };
        p.filter(|&p| Some(self.root_id(p)) != root && self.physicals[p].in_world && !self.physicals[p].destroyed)
    }

    /// BallisticsEmitter.BallisticIteration up to its next yield; the overlap path's StartCoroutine runs its iteration at once.
    /// True when this coroutine itself yielded (no overlap StartCoroutine before the yield): its continuation is the last
    /// one scheduled.
    pub(crate) fn bullet_iterate(&mut self, mut b: Bullet) -> bool {
        let mut own = true;
        loop {
            let f = b.shooter;
            let e = self.emitter(f);
            if b.iteration as i64 >= e.MaxBallisticsIterations as i64 || b.total >= e.MaxTotalDistance {
                self.remove_tracer(&b);
                return false;
            }
            if self.is_point_in_lava(b.origin) {
                self.randomise_course(1.0, 20.0, &mut b);
            } else if self.is_point_under_water(b.origin) {
                self.randomise_course(0.99999, 10.0, &mut b);
            }
            let e = self.emitter(f);
            let (start_speed, layers, traversal, max_range) = (e.cartridge.StartSpeed, e.LayersToHit, e.DoTraversal, e.MaxRange);
            let (thickness, impact_mul, drop_mul) = (e.ThicknessStepSize, e.ImpactForceMultiplier, e.BulletDropMultiplier);
            let by_distance = speed_multiplier_by_distance(b.total);
            let num = mathf::clamp(b.speed / start_speed, 0.05, 1.0) * by_distance;
            if num <= 0.1 {
                self.remove_tracer(&b);
                return false;
            }
            let hit = self.overlap_point_depth(b.origin, layers, -0.1, 0.1);
            if let Some((p, collider)) = hit.and_then(|(p, c)| self.bullet_target(f, p).map(|p| (p, c))) {
                let num2 = self.hardness(f, p);
                if b.previous != Some(collider) {
                    let damage = self.damage_for_distance(f, thickness + b.total) * mathf::clamp01(num);
                    self.tracer_line_enable(&b, false);
                    if let Some(prev) = b.previous.filter(|&c| self.collider_exists(c)) {
                        let at = sub(b.origin, mul(b.direction, thickness));
                        self.send_shot(prev, None, b.direction, at, damage, true, Some(self.emitter(b.shooter).cartridge));
                        self.bullet_exit_callback(f, at, b.direction);
                    }
                    self.send_shot(collider, Some(p), mul(b.direction, -1.0), b.origin, damage, false, Some(self.emitter(b.shooter).cartridge));
                    self.bullet_entry_callback(f, b.origin, b.direction, collider);
                    let origin = b.origin;
                    self.bullet_explode(&mut b, origin);
                }
                let impact = self.emitter(f).cartridge.ImpactForce;
                let force = mul(mul(mul(mul(mul(b.direction, impact), num2), thickness), impact_mul), 4.0);
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_force_at_position(&mut self.world, force, b.origin);
                let angle = self.bullet_random_angle(f);
                b = Bullet {
                    origin: add(b.origin, mul(b.direction, thickness)),
                    direction: add(b.direction, angle),
                    speed: b.speed - num2,
                    iteration: b.iteration + 1,
                    total: b.total + thickness,
                    previous: Some(collider),
                    splash: true,
                    ..b
                };
                own = false;
                continue;
            }
            if self.prefs.BulletCrackWhiz && self.bullet_crack_clips != 0 && self.can_play_cracks(f, b.origin, b.speed, b.total) {
                // CameraAudioSource.PlayOneShot(BulletCrackClips.PickRandom(), Random.Range(0.05f, 0.3f))
                let k = self.pick_random(self.bullet_crack_clips).unwrap();
                let volume = self.random.range(0.05, 0.3);
                if let Some(id) = self.camera_audio_source {
                    let clip = self.audio.clips(0, "BulletCrackClips")[k].clone();
                    self.audio.play_one_shot(id, clip.as_ref(), volume);
                }
            }
            if let Some(prev) = b.previous.filter(|&c| self.collider_exists(c)) {
                let at = sub(b.origin, mul(b.direction, thickness));
                let damage = self.damage_for_distance(f, thickness + b.total) * mathf::clamp01(num);
                self.send_shot(prev, None, b.direction, at, damage, true, Some(self.emitter(b.shooter).cartridge));
                self.bullet_exit_callback(f, at, b.direction);
                self.tracer_line_enable(&b, true);
            }
            let ray = self.raycast(V::new(b.origin[0], b.origin[1]), b.direction, max_range, layers, -0.1, 0.1);
            // extras: a shot into a portal's opening goes on out of the other (src/game/extras_glue.rs); no portals, no change.
            if let Some((t, origin, direction)) = self.extras_ray_portal(b.origin, b.direction, ray.as_ref().map_or(max_range, |h| h.distance + 0.05)) {
                b = Bullet { origin, direction, total: b.total + t, iteration: b.iteration + 1, previous: None, ..b };
                continue;
            }
            let target = ray.and_then(|h| self.bullet_target(f, self.physical_of_body(h.body)).map(|p| (h, p)));
            let Some((hit, p)) = target else {
                if traversal {
                    // tracer.SetPosition(1, Clamp01(num) * MaxRange * right); position = origin; right = direction.
                    self.tracer_line_place(&b, b.origin, b.direction, mathf::clamp01(num) * max_range);
                    let dt = self.time.deltaTime;
                    let traversal = mathf::min(max_range, (d(dt) * 60.0f32 * max_range * mathf::sqrt(num)).f());
                    let damage = self.damage_for_distance(f, (d(b.total) + d(traversal) * 0.1f32).f()) * num;
                    b.splash = !self.test_for_water_hit(f, b.origin, b.direction, damage, b.splash);
                    let drop = ((1.0 - d(mathf::pow(0.999, dt * 60.0))) * drop_mul * 0.7f32).f();
                    b.direction = mathf::normalized2(mathf::v2_lerp(b.direction, [0.0, -1.0], drop));
                    // WaitForEndOfFrame, then StartCoroutine(BallisticIteration(..)).
                    b = Bullet { origin: add(b.origin, mul(b.direction, traversal)), total: (d(b.total) + d(traversal) * 0.1f32).f(), previous: None, ..b };
                    let slot = self.bullet_slot(b, None);
                    let owner = self.emitter_owner(f);
                    self.coroutine_seq += 1;
                    self.end_of_frame.push(Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner, from: i64::MIN, step: Step::Bullet(slot) });
                }
                return traversal && own;
            };
            let hardness = self.hardness(f, p);
            let impact = self.emitter(f).cartridge.ImpactForce;
            let value = mathf::max(impact * hardness, impact);
            let damage2 = self.damage_for_distance(f, b.total + hit.distance) * mathf::clamp01(num);
            let value = (d(impact_mul) * mathf::clamp(value, 0.0, 1.5) * 1.5f32).f();
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force_at_position(&mut self.world, mul(b.direction, value), b.origin);
            let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
            self.send_shot(hit.collider, Some(p), normal, point, damage2, false, Some(self.emitter(b.shooter).cartridge));
            let charge = self.emitter(f).Charge;
            let ph = &mut self.physicals[p];
            ph.charge = mathf::max(charge * ph.EnergyWireResistance, ph.Charge());
            self.bullet_entry_callback(f, point, b.direction, hit.collider);
            self.bullet_explode(&mut b, point);
            self.tracer_line_place(&b, b.origin, b.direction, hit.distance);
            let h = BulletHit { physical: p, hardness, damage: damage2, collider: hit.collider, point, normal, distance: hit.distance };
            let slot = self.bullet_slot(b, Some(h));
            let owner = self.emitter_owner(f);
            self.start_wait(owner, 0.01, Step::Bullet(slot));
            return own;
        }
    }

    /// BulletEntryCallback: BulletEntryEvent's listeners (a firearm's connected attachments).
    fn bullet_entry_callback(&mut self, f: Shooter, position: [f32; 2], direction: [f32; 2], hit_collider: i32) {
        if let Shooter::MachineGun(m) = f {
            self.machine_gun_on_entry(m, position, direction);
        }
        if let Shooter::Firearm(x) = f {
            self.bullet_entry_listeners(x, hit_collider, position);
        }
    }

    /// BulletExitCallback
    fn bullet_exit_callback(&mut self, f: Shooter, position: [f32; 2], direction: [f32; 2]) {
        if let Shooter::MachineGun(m) = f {
            self.machine_gun_on_exit(m, position, direction);
        }
    }

    /// `if (canExplode && ExplosiveRound) explode(point)`: ExplosionCreator.Explode(ExplosiveRoundParams) at the point,
    /// or half a unit past it, and canExplode = false.
    fn bullet_explode(&mut self, b: &mut Bullet, point: [f32; 2]) {
        let e = self.emitter(b.shooter);
        if !(b.can_explode && e.ExplosiveRound) {
            return;
        }
        let at = if e.ExplodeAtHitPoint { point } else { add(point, mul(b.direction, 0.5)) };
        let mut ex = e.ExplosiveRoundParams;
        ex.Position = [at[0], at[1], 0.0];
        self.emitter_mut(b.shooter).ExplosiveRoundParams.Position = ex.Position;
        self.explosion_creator_explode(ex);
        b.can_explode = false;
    }

    fn bullet_slot(&mut self, b: Bullet, h: Option<BulletHit>) -> usize {
        match self.bullets.iter().position(|s| s.is_none()) {
            Some(i) => {
                self.bullets[i] = Some((b, h));
                i
            }
            None => {
                self.bullets.push(Some((b, h)));
                self.bullets.len() - 1
            }
        }
    }

    /// A BallisticIteration past its WaitForEndOfFrame (the next iteration) or WaitForSeconds(0.01) (after a hit).
    pub(super) fn bullet_resume(&mut self, slot: usize) {
        let (b, h) = self.bullets[slot].take().expect("a resumed bullet");
        let Some(h) = h else {
            self.bullet_iterate(b);
            return;
        };
        let f = b.shooter;
        let mut b = b;
        b.splash = !self.test_for_water_hit(f, b.origin, b.direction, h.damage, b.splash);
        if self.should_ricochet(f, b.direction, &h) {
            self.ricochet_bullet(b, &h);
        } else if !self.physicals[h.physical].BulletPenetration {
            self.remove_tracer(&b);
        } else {
            let thickness = self.emitter(f).ThicknessStepSize;
            let next = Bullet {
                origin: add(h.point, mul(b.direction, thickness)),
                speed: b.speed - h.hardness,
                iteration: b.iteration + 1,
                total: b.total + h.distance,
                previous: Some(h.collider),
                ..b
            };
            self.bullet_iterate(next);
        }
    }

    /// BallisticsEmitter.RicochetBullet(direction, speed, iteration, totalDistance, charge, tracer, hit): BulletRicochetCallback,
    /// then the reflected BallisticIteration (StartCoroutine: up to its first yield).
    pub(crate) fn ricochet_bullet(&mut self, b: Bullet, h: &BulletHit) {
        let f = b.shooter;
        match f {
            // FirearmBehaviour.OnRicochet instantiates Prefabs/Ricochet at the hit point.
            Shooter::Firearm(_) if self.emitter(f).ricochet_callback => {
                self.spawn_resource("Prefabs/Ricochet", [h.point[0], h.point[1], 0.0]);
            }
            Shooter::MachineGun(m) => self.machine_gun_on_ricochet(m, h.point),
            _ => {}
        }
        let dot = dot2(h.normal, b.direction);
        let k = -2.0 * dot;
        let reflect = [(d(k) * h.normal[0] + b.direction[0]).f(), (d(k) * h.normal[1] + b.direction[1]).f()];
        let angle = mul(self.bullet_random_angle(f), 0.1);
        let next = Bullet {
            origin: sub(h.point, mul(h.normal, 0.02)),
            direction: add(reflect, angle),
            iteration: b.iteration + 1,
            total: b.total + h.distance,
            previous: Some(h.collider),
            splash: true,
            can_explode: false,
            ..b
        };
        self.bullet_iterate(next);
    }

    /// BallisticsEmitter.ShouldRicochet
    /// ShouldRicochet(incoming, hit, phys)
    pub(crate) fn should_ricochet(&mut self, f: Shooter, incoming: [f32; 2], h: &BulletHit) -> bool {
        let m = self.emitter(f).RicochetChanceMultiplier;
        let ph = &self.physicals[h.physical];
        // !hit.transform: the physical's GameObject is gone.
        if ph.destroyed {
            return false;
        }
        if !ph.BulletDeflectChanceOverride.is_nan() {
            let chance = m * ph.BulletDeflectChanceOverride;
            return chance > self.random.value();
        }
        let props = &self.properties[ph.Properties];
        let num = dot2(h.normal, incoming).abs();
        let num2 = mathf::clamp01(props.Softness + props.Brittleness);
        let num3 = (1.0 - (d(num2) * 150.0f32 + num)).f();
        if num3 > 0.05 {
            return num3 * m > self.random.value();
        }
        false
    }

    /// BallisticsEmitter.RandomiseCourse
    /// RandomiseCourse(dampening, diversion, ref direction, ref speed)
    fn randomise_course(&mut self, dampening: f32, diversion: f32, b: &mut Bullet) {
        let dt = self.time.deltaTime;
        let r = self.random.inside_unit_circle();
        let s = diversion * dt;
        b.direction = mathf::normalized2(add(b.direction, mul(r, s)));
        b.speed = mathf::lerp(b.speed, 0.0, mathf::lerp_factor_dt(dampening, dt));
    }

    /// BallisticsEmitter.CanPlayCracks
    /// CanPlayCracks(origin, speed, distance); Global.main.Paused is never set.
    pub(crate) fn can_play_cracks(&mut self, f: Shooter, origin: [f32; 2], speed: f32, distance: f32) -> bool {
        if self.camera_orthographic_size() > 8.0 || distance < 5.0 {
            return false;
        }
        let time = self.time.time;
        let e = self.emitter(f);
        if time - e.lastCrackPlayTime < e.BulletCrackCooldownTime || speed < e.BulletCrackSpeedThreshold {
            return false;
        }
        if mathf::sqr_magnitude(sub(origin, self.camera_position())) > 400.0 {
            return false;
        }
        if self.emitter(f).BulletCrackChance < self.random.value() {
            self.emitter_mut(f).lastCrackPlayTime = time;
            return false;
        }
        true
    }

    /// BallisticsEmitter.TestForWaterHit
    /// TestForWaterHit(origin, direction, damage, sendMessage)
    pub(crate) fn test_for_water_hit(&mut self, f: Shooter, origin: [f32; 2], direction: [f32; 2], damage: f32, send: bool) -> bool {
        let e = self.emitter(f);
        let (range, mask) = (e.MaxRange, e.WaterLayer);
        let Some(hit) = self.raycast(V::new(origin[0], origin[1]), direction, range, mask, f32::NEG_INFINITY, f32::INFINITY) else { return false };
        if !(hit.distance > 0.5) {
            return false;
        }
        // hit.transform: the attached Rigidbody2D's GameObject, else the collider's.
        let p = self.physical_of_body(hit.body);
        let layer = match p {
            Some(p) => self.physical_layer(p),
            // A node's Rigidbody2D: hit_transform_physical checks the collider sits on the node's own GameObject.
            None => {
                self.hit_transform_physical(&hit);
                self.world.layers[&hit.game_object]
            }
        };
        if layer != 4 {
            return false;
        }
        if send {
            self.send_shot(hit.collider, p, [hit.normal.x, hit.normal.y], [hit.point.x, hit.point.y], damage, false, Some(self.emitter(f).cartridge));
        }
        true
    }

    /// The physical whose Rigidbody2D owns a b2 body.
    fn physical_of_body(&self, body: crate::b2::world::BodyId) -> Option<usize> {
        self.physicals.iter().position(|p| !p.destroyed && p.rigidbody.body == body)
    }

    /// The layer of a physical's GameObject (that of its own colliders). A disabled collider has no fixture: then the
    /// object is found in its clone.
    pub(super) fn physical_layer(&self, p: usize) -> i32 {
        let c = *self.physicals[p].all_colliders.first().unwrap_or_else(|| unimplemented!("mod-only: the layer of a physical without its own collider"));
        let id = self.physicals[p].id;
        let go = self.world.collider_game_object(c).or_else(|| self.unsynced.iter().chain(self.retained.iter()).find_map(|inst| inst.objects.iter().find(|x| x.components.contains(&id)).map(|x| x.game_object)));
        self.world.layers[&go.expect("a physical in a clone")]
    }

    /// SendMessage("Shot"/"ExitShot", shot) to a GameObject: `physical` when the target is a physical's object, else
    /// the collider's bare object. Every component on it with the method runs (disabled ones too), in order.
    /// `cartridge` is Shot.cartridge: a bullet's emitter Cartridge, None for a Shot built without one.
    pub(super) fn send_shot(&mut self, collider: i32, physical: Option<usize>, normal: [f32; 2], point: [f32; 2], damage: f32, exit: bool, cartridge: Option<Cartridge>) {
        self.send_shot_flags(collider, physical, normal, point, damage, exit, cartridge, true, true);
    }

    /// Transform.position += delta on a running physical (PP_HUMAN moves the item onto the human).
    pub fn move_physical_for_test(&mut self, p: usize, delta: [f32; 3]) {
        let at = self.physicals[p].transform.position;
        self.physical_set_position(p, [at[0] + delta[0], at[1] + delta[1], at[2] + delta[2]]);
    }

    /// StartCoroutine(CrushNextFrame) on a limb (PP_HUMAN=crush).
    pub fn crush_limb_for_test(&mut self, l: usize) {
        self.limb_crush_next_frame(l);
    }

    /// A bullet-like Shot of `damage` at `point` on the physical's collider (PP_STRESS=shoot).
    pub fn send_shot_for_test(&mut self, collider: i32, p: usize, point: [f32; 2], damage: f32) {
        self.send_shot(collider, Some(p), [0.0, 1.0], point, damage, false, None);
    }

    /// Shot..ctor of `send_shot` (CanCrush true; triggersExplosiveOverride from the argument, true by default; the caller clears them):
    /// with Shot.CanCrush and Shot.triggersExplosiveOverride.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn send_shot_flags(&mut self, collider: i32, physical: Option<usize>, normal: [f32; 2], point: [f32; 2], damage: f32, exit: bool, cartridge: Option<Cartridge>, can_crush: bool, triggers_explosive: bool) {
        let receivers = if exit { EXIT_SHOT_RECEIVERS } else { SHOT_RECEIVERS };
        // extras: a crab's shell turns small bullets (src/extras_hook.rs; false for every game object).
        if crate::extras_hook::shell_glances(self, physical.or_else(|| self.physical_of_collider(collider)), damage) {
            return;
        }
        let Some(p) = physical.or_else(|| self.physical_of_collider(collider)) else {
            let bare = &self.bare_colliders[&collider];
            match bare.water_impact {
                Some(splash) if !exit => self.water_impact_shot(splash, normal, point, damage),
                _ if bare.receives(receivers) => self.send_shot_to_object(collider, receivers, exit, damage),
                _ => {}
            }
            return;
        };
        // SendMessage skips an inactive GameObject (a limb Disintegrate deactivated: the previousCollider of a bullet
        // that crushed it).
        if !self.physicals[p].active {
            return;
        }
        // A recording has no clones: a limb's receivers in component order, or a map object's PhysicalBehaviour.
        let classes: Vec<(String, i32)> = if self.is_recording() {
            let limb = self.limbs.iter().any(|x| x.PhysicalBehaviour == p);
            let names: &[&str] = if limb { &["PhysicalBehaviour", "LimbBehaviour", "CirculationBehaviour"] } else { &["PhysicalBehaviour"] };
            // Component order is decreasing instance id (the ids count down along the prefab's components).
            let mut v: Vec<(String, i32)> = names.iter().map(|c| (c.to_string(), match *c {
                "LimbBehaviour" => self.limbs.iter().find(|x| x.PhysicalBehaviour == p).map_or(0, |x| x.id),
                "CirculationBehaviour" => self.limbs.iter().position(|x| x.PhysicalBehaviour == p).and_then(|l| self.circulations[l].recorded_id).unwrap_or(0),
                _ => self.physicals[p].id,
            })).collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1));
            v
        } else {
            self.with_component_instance(self.physicals[p].id, |_, inst, o| {
                inst.prefab.objects[o].components.iter().enumerate().filter_map(|(k, (_, c))| match c {
                    crate::assets::db::Component::Script { class, .. } if receivers.contains(&class.as_str()) => Some((class.clone(), inst.objects[o].components[k])),
                    _ => None,
                }).collect()
            })
        };
        for (class, id) in classes {
            // SendMessage: an exception in one receiver is logged; the next receiver still gets the message.
            self.unity_callback(|g| g.shot_receiver(&class, id, p, exit, normal, point, damage, cartridge, can_crush, triggers_explosive));
        }
        // ExplosiveBehaviours added at run time come last on the GameObject (ExplosiveBehaviour has no ExitShot).
        if !exit {
            for e in self.added_explosives_on(p) {
                self.explosive_shot(e, triggers_explosive);
            }
        }
    }

    /// One receiver of SendMessage("Shot") / ("ExitShot") on the physical object `p`.
    #[allow(clippy::too_many_arguments)]
    fn shot_receiver(&mut self, class: &str, id: i32, p: usize, exit: bool, normal: [f32; 2], point: [f32; 2], damage: f32, cartridge: Option<Cartridge>, can_crush: bool, triggers_explosive: bool) {
        match (class, exit) {
            (c, _) if super::translated::CLASSES.contains(&c) => {
                let Some(&(_, super::Behaviour::Translated(t))) = self.behaviour_of.get(&id) else { unimplemented!("mod-only: {c}.Shot before its Awake") };
                let shot = super::translated::rt::Shot { normal, point, damage };
                if exit { self.tr_exit_shot(t, shot) } else { self.tr_shot(t, shot) }
            }
            ("PhysicalBehaviour", false) => self.physical_shot(p, normal, point, damage),
            ("LiquidContainerLeakingBehaviour", _) => {
                let l = self.liquid_leaks.iter().position(|x| x.id == id).expect("the object's LiquidContainerLeakingBehaviour");
                if exit { self.liquid_leak_exit_shot(l, normal, point, damage) } else { self.liquid_leak_shot(l, normal, point, damage) }
            }
            ("PhysicalBehaviour", true) => self.physical_exit_shot(p, normal, point, damage),
            ("BalloonBehaviour", false) => {
                let b = self.balloons.iter().position(|x| x.physical == Some(p)).expect("the object's BalloonBehaviour");
                self.balloon_shot(b);
            }
            ("BalloonBehaviour", true) => {
                let b = self.balloons.iter().position(|x| x.physical == Some(p)).expect("the object's BalloonBehaviour");
                self.balloon_exit_shot(b);
            }
            // SentryTurretBehaviour.Shot(shot) (not an ExitShot receiver).
            ("SentryTurretBehaviour", false) => {
                let s = self.sentry_turrets.iter().position(|s| s.id == id).expect("the object's SentryTurretBehaviour");
                self.sentry_turret_shot(s, damage);
            }
            ("DestroyableBehaviour" | "VaseBehaviour", _) => {
                let x = self.destroyables.iter().position(|x| x.PhysicalBehaviour == p).expect("the object's DestroyableBehaviour");
                self.destroyable_shot(x, normal, damage);
            }
            // ExplosiveBehaviour has no ExitShot.
            ("ExplosiveBehaviour", true) => {}
            ("ExplosiveBehaviour", false) => {
                let e = self.explosives.iter().position(|e| e.physical == Some(p) && !e.added).expect("the object's ExplosiveBehaviour");
                self.explosive_shot(e, triggers_explosive);
            }
            ("FireworksBehaviour", false) => {
                let f = self.fireworks.iter().position(|f| f.id == id).expect("the object's FireworksBehaviour");
                self.fireworks_shot(f);
            }
            ("DamagableMachineryBehaviour", _) => {
                let m = self.machinery.iter().position(|m| m.PhysicalBehaviour == p).expect("the object's DamagableMachineryBehaviour");
                self.machinery_shot(m, damage);
            }
            ("LimbBehaviour", _) | ("CirculationBehaviour", _) => {
                // A limb's CirculationBehaviour shares its index.
                let l = self.limbs.iter().position(|x| x.PhysicalBehaviour == p).expect("the object's LimbBehaviour");
                match (class, exit) {
                    ("LimbBehaviour", false) => self.limb_shot(l, normal, point, damage, can_crush),
                    ("LimbBehaviour", true) => self.limb_exit_shot(l, normal, point, damage),
                    (_, false) => self.circulation_shot(l, normal, point, damage),
                    (_, true) => self.circulation_exit_shot(l, normal, point),
                }
            }
            ("TyreDeflationBehaviour", _) => {
                let t = self.tyres.iter().position(|t| t.Phys == p).expect("the object's TyreDeflationBehaviour");
                self.tyre_shot(t, damage, point);
            }
            ("GlowtubeBehaviour", _) => {
                let g = self.glowtubes.iter().position(|g| g.Phys == p).expect("the object's GlowtubeBehaviour");
                self.glowtube_shot(g, damage);
            }
            ("CathodeLightBehaviour", false) => {
                let c = self.cathode_lights.iter().position(|c| c.id == id).expect("the object's CathodeLightBehaviour");
                self.cathode_light_shot(c, damage);
            }
            ("JavelinMissileBehaviour", false) => {
                let m = self.javelin_missiles.iter().position(|x| x.id == id).expect("the object's JavelinMissileBehaviour");
                self.javelin_missile_shot(m, damage);
            }
            ("DroneBehaviour", false) => {
                let x = self.drones.iter().position(|x| x.id == id).expect("the object's DroneBehaviour");
                self.drone_shot(x, damage);
            }
            ("DebugTargetDamageMeter", false) => self.debug_target_damage_meter_shot(p, damage),
            ("TelevisionBehaviour", _) => {
                let t = self.televisions.iter().position(|x| x.physical == p).expect("the object's TelevisionBehaviour");
                self.television_shot(t);
            }
            ("BulbBehaviour", _) => {
                let b = self.bulbs.iter().position(|b| b.Phys == p).expect("the object's BulbBehaviour");
                self.bulb_shot(b);
            }
            (c, _) if super::syringe::is_syringe(c) => {
                let s = self.syringes.iter().position(|s| s.Phys == p).expect("the object's SyringeBehaviour");
                if exit { self.syringe_exit_shot(s) } else { self.syringe_shot(s) }
            }
            ("BreakJointOnShotBehaviour", false) => self.break_joint_on_shot_shot(id, damage),
            ("ShotMessagePropagator", false) => {
                let s = self.shot_propagators.iter().position(|s| s.id == id).expect("the object's ShotMessagePropagator");
                self.shot_message_propagator_shot(s, normal, point, damage, cartridge, can_crush, triggers_explosive);
            }
            ("ActOnShot", false) => {
                let a = self.act_on_shots.iter().position(|a| a.id == id).expect("the object's ActOnShot");
                self.act_on_shot_shot(a, damage, cartridge);
            }
            (c, false) => unimplemented!("mod-only: {c}.Shot"),
            (c, true) => unimplemented!("mod-only: {c}.ExitShot"),
        }
    }

    /// ShotMessagePropagator.Shot: gameObject.SendMessage("Shot", shot, DontRequireReceiver) to each live target, in order.
    /// The same Shot object goes to every one.
    #[allow(clippy::too_many_arguments)]
    fn shot_message_propagator_shot(&mut self, s: usize, normal: [f32; 2], point: [f32; 2], damage: f32, cartridge: Option<Cartridge>, can_crush: bool, triggers_explosive: bool) {
        for t in self.shot_propagators[s].targets.clone() {
            // (bool)gameObject
            if self.destroyed_components.contains(&t) {
                continue;
            }
            let components = self.with_component_instance(t, |_, inst, o| inst.objects[o].components.clone());
            let Some(p) = self.physicals.iter().position(|x| components.contains(&x.id) && !x.destroyed) else {
                unimplemented!("mod-only: ShotMessagePropagator.Shot to a GameObject without a PhysicalBehaviour (its other receivers)");
            };
            self.send_shot_flags(0, Some(p), normal, point, damage, false, cartridge, can_crush, triggers_explosive);
        }
    }

    /// SendMessage("Break", velocity) to a GameObject, as `send_shot` finds it.
    pub(super) fn send_break(&mut self, collider: i32, physical: Option<usize>, velocity: [f32; 2]) {
        let Some(p) = physical.or_else(|| self.physical_of_collider(collider)) else {
            if self.bare_colliders[&collider].receives(BREAK_RECEIVERS) {
                self.send_break_to_object(collider, BREAK_RECEIVERS);
            }
            return;
        };
        for class in self.receivers_of(self.physicals[p].id, BREAK_RECEIVERS) {
            // SendMessage: an exception in one receiver is logged; the next receiver still gets the message.
            self.unity_callback(|g| g.break_receiver(&class, p, velocity));
        }
    }

    /// One receiver of SendMessage("Break").
    fn break_receiver(&mut self, class: &str, p: usize, velocity: [f32; 2]) {
        match class {
            "DestroyableBehaviour" | "VaseBehaviour" => {
                let x = self.destroyables.iter().position(|x| x.PhysicalBehaviour == p).expect("the object's DestroyableBehaviour");
                self.destroyable_break(x, velocity);
            }
            "DamagableMachineryBehaviour" => {
                let m = self.machinery.iter().position(|m| m.PhysicalBehaviour == p).expect("the object's DamagableMachineryBehaviour");
                self.machinery_break(m);
            }
            "TyreDeflationBehaviour" => {
                let t = self.tyres.iter().position(|t| t.Phys == p).expect("the object's TyreDeflationBehaviour");
                self.tyre_pop(t, None);
            }
            "GlowtubeBehaviour" => {
                let g = self.glowtubes.iter().position(|g| g.Phys == p).expect("the object's GlowtubeBehaviour");
                self.glowtube_break(g);
            }
            "CathodeLightBehaviour" => {
                let c = self.cathode_lights.iter().position(|c| super::script_object::on_physical(&c.own, p)).expect("the object's CathodeLightBehaviour");
                self.cathode_light_break(c);
            }
            "BalloonBehaviour" => {
                let b = self.balloons.iter().position(|x| x.physical == Some(p)).expect("the object's BalloonBehaviour");
                self.balloon_break(b);
            }
            "TelevisionBehaviour" => {
                let t = self.televisions.iter().position(|x| x.physical == p).expect("the object's TelevisionBehaviour");
                self.television_break(t);
            }
            "BulbBehaviour" => {
                let b = self.bulbs.iter().position(|b| b.Phys == p).expect("the object's BulbBehaviour");
                self.bulb_break(b);
            }
            // SyringeBehaviour.Break(velocity): BreakSyringe().
            c if super::syringe::is_syringe(c) => {
                let s = self.syringes.iter().position(|s| s.Phys == p).expect("the object's SyringeBehaviour");
                self.break_syringe(s);
            }
            c => unimplemented!("mod-only: {c}.Break"),
        }
    }

    /// BroadcastMessage("Break", velocity) to a collider's GameObject: it, then its active descendants in hierarchy order (an
    /// inactive object takes its whole subtree out); each one's receivers in component order.
    pub(super) fn broadcast_break(&mut self, collider: i32, velocity: [f32; 2]) {
        let owner = self.physical_of_collider(collider);
        let below: Vec<Vec<i32>> = self.with_component_instance(owner.map_or(collider, |p| self.physicals[p].id), |_, inst, o| {
            let mut out = Vec::new();
            let mut skipped: Vec<usize> = Vec::new();
            for x in inst.descendants(o) {
                if skipped.iter().any(|&s| inst.is_descendant(x, s)) {
                    continue;
                }
                if !inst.objects[x].active {
                    skipped.push(x);
                    continue;
                }
                let has = inst.prefab.objects[x].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if BREAK_RECEIVERS.contains(&class.as_str())));
                if has {
                    out.push(inst.objects[x].components.clone());
                }
            }
            out
        });
        match owner {
            Some(p) => self.send_break(collider, Some(p), velocity),
            None if self.bare_colliders[&collider].receives(BREAK_RECEIVERS) => self.send_break_to_object(collider, BREAK_RECEIVERS),
            None => {}
        }
        for components in below {
            let Some(p) = self.physicals.iter().position(|x| components.contains(&x.id) && !x.destroyed) else {
                unimplemented!("mod-only: BroadcastMessage Break to a descendant without a PhysicalBehaviour");
            };
            self.send_break(0, Some(p), velocity);
        }
    }

    /// The physical of a RaycastHit2D.transform (the attached Rigidbody2D's GameObject, else the collider's); None for
    /// a collider without a PhysicalBehaviour there.
    pub(super) fn hit_transform_physical(&mut self, hit: &super::explosion::RayHit2D) -> Option<usize> {
        if let Some(p) = self.physical_of_body(hit.body) {
            return Some(p);
        }
        // A Rigidbody2D on a GameObject without a PhysicalBehaviour (a grenade pin): its transform is no key of
        // PhysicalObjectsInWorldByTransform, so there is no physical. The scripts a message reaches are the collider's own
        // GameObject's, which is the node's when the collider sits on it.
        if let Some(n) = self.node_rigidbodies.iter().find(|n| n.rigidbody.body == hit.body) {
            let node_id = self.transform_nodes[n.node].id;
            let own = self.with_component_instance(node_id, |_, inst, x| inst.objects[x].game_object);
            if own != hit.game_object {
                unimplemented!("mod-only: RaycastHit2D.transform of a Transform node's Rigidbody2D whose collider is on another GameObject");
            }
        }
        None
    }

    /// The classes of a script's GameObject that take a message (`receivers`), in component order.
    fn receivers_of(&mut self, id: i32, receivers: &[&str]) -> Vec<String> {
        // A recording has no clones: a person's root takes the Person's messages, limbs and the map take these none.
        if self.is_recording() {
            let person = self.people.iter().any(|p| p.id == id) && receivers.contains(&"PersonBehaviour");
            return if person { vec!["PersonBehaviour".into()] } else { Vec::new() };
        }
        self.with_component_instance(id, |_, inst, o| {
            inst.prefab.objects[o].components.iter().filter_map(|(_, c)| match c {
                crate::assets::db::Component::Script { class, .. } if receivers.contains(&class.as_str()) => Some(class.clone()),
                _ => None,
            }).collect()
        })
    }

    /// LimbBehaviour.Shot
    pub(crate) fn limb_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2], damage: f32, can_crush: bool) {
        use super::misc::damage_type::BULLET;
        if self.limbs[l].ImmuneToDamage {
            return;
        }
        let mut damage = damage * self.limbs[l].ShotDamageMultiplier;
        damage /= self.mass_strength_ratio(l);
        damage *= self.prefs.FragilityMultiplier;
        if self.limbs[l].IsAndroid {
            if damage < 40.0 {
                return;
            }
            damage *= 0.2;
        } else {
            self.limbs[l].shotHeat += 1.0;
        }
        if self.limbs[l].HasLungs && !self.limbs[l].IsAndroid && self.random.value() > 0.9 {
            self.limbs[l].LungsPunctured = true;
        }
        let flag = self.is_world_point_in_vital_part(l, point) && self.random.value() > 0.05;
        let num = if flag { 7.0 } else { 0.1 } * damage;
        let (person, p) = (self.limbs[l].Person, self.limbs[l].PhysicalBehaviour);
        if let Some(effect) = self.people[person].PoolableImpactEffect.filter(|_| !self.prefs.GorelessMode && self.prefs.ChunkyShotParticles)
            && (self.physicals[p].BurnProgress as f64) < 0.6
            && self.skins[l].acidProgress < 0.7
            && num > self.people[person].ImpactEffectShotDamageThreshold
            && self.random.value() > 0.8
        {
            // transform.right = shot.normal: presentation.
            // A recording only names the prefab.
            if let Some(effect) = effect {
                self.request_pooled_effect(effect, "LimbBehaviour.Shot", point, angle_of(normal), 1.0);
            }
        }
        for i in 0..self.limbs[l].ConnectedLimbs.len() {
            let c = self.limbs[l].ConnectedLimbs[i];
            if !self.physicals[self.limbs[c].PhysicalBehaviour].destroyed {
                self.limbs[c].Numbness += 0.5;
            }
        }
        if self.limb_is_considered_alive(l) {
            let m = self.limbs[l].ShotDamageMultiplier;
            let a = self.people[person].AdrenalineLevel;
            self.people[person].AdrenalineLevel = (d(a) + d(self.random.value()) * m).f();
            let c = self.people[person].Consciousness;
            self.people[person].Consciousness = (d(c) - d(self.random.range(0.02, 0.1)) * m).f();
            if !self.limbs[l].IsAndroid && !self.limbs[l].IsZombie {
                let s = self.people[person].ShockLevel;
                self.people[person].ShockLevel = (d(s) + d(num) * self.random.value() * 0.0025f32).f();
                self.person_wince(person, 300.0);
                self.limbs[l].Numbness = 1.0;
                if d(self.random.value()) * self.limbs[l].Vitality > d(0.5) && self.random.value() > 0.6 && !self.limb_is_paralysed(l) {
                    let pain = self.random.value() * 2.0;
                    self.add_pain(person, pain);
                }
                if d(num) > d(2.0) * self.random.value() {
                    let w = self.people[person].DesiredWalkingDirection;
                    let delta = d(self.random.value()) * 3.0f32;
                    let same_side = (normal[0] > 0.0) == (self.people[person].localScale[0] > 0.0);
                    self.people[person].DesiredWalkingDirection = if same_side { d(w) - delta } else { d(w) + delta }.f();
                }
                // Person.SendMessage("Shot", shot)
                for class in self.receivers_of(self.people[person].id, SHOT_RECEIVERS) {
                    match class.as_str() {
                        "PersonBehaviour" => self.person_shot(person),
                        c => unimplemented!("mod-only: {c}.Shot on a person root"),
                    }
                }
            }
            if self.limbs[l].HasBrain && !self.limbs[l].IsAndroid && (!self.limbs[l].IsZombie || !(self.random.value() > 0.2)) {
                let num2 = if flag { 0.1 } else { 0.8 };
                if self.random.value() > num2 {
                    self.limbs[l].Health = 0.0;
                }
                if self.random.value() > num2 {
                    self.people[person].Consciousness = 0.0;
                }
            }
        }
        if can_crush && self.prefs.LimbCrushing && damage > 149.0 && mathf::clamp(((d(damage) - 149.0f32) * 0.005f32).f(), 0.3, 0.8) > self.random.value() {
            if self.prefs.StopAnimationOnDamage {
                self.people[person].OverridePoseIndex = -1;
            }
            self.limb_crush_next_frame(l);
        } else if self.limbs[l].shotHeat > 5.0 && self.random.value() > 0.35 {
            if self.limbs[l].HasJoint && self.random.value() > 0.8 {
                self.limb_slice(l);
            } else if can_crush && self.prefs.LimbCrushing {
                if self.prefs.StopAnimationOnDamage {
                    self.people[person].OverridePoseIndex = -1;
                }
                self.limb_crush_next_frame(l);
            }
        }
        if self.limbs[l].IsZombie || self.random.value() < 0.01 {
            self.limb_damage(l, num * 0.01);
        } else {
            if !self.limbs[l].IsAndroid {
                self.circulations[l].InternalBleedingIntensity += num;
                if flag {
                    self.circulations[l].InternalBleedingIntensity += 5.0;
                }
            }
            let damage = mathf::min(self.limbs[l].InitialHealth / 2.0, num * 2.0);
            self.limb_damage(l, damage);
        }
        let b = damage * 0.1;
        self.skin_add_damage_point(l, BULLET, point, mathf::max(50.0, b));
    }

    /// StartCoroutine(LimbBehaviour.CrushNextFrame()): the dismemberment damage point, then WaitForEndOfFrame.
    pub(crate) fn limb_crush_next_frame(&mut self, l: usize) {
        let p = self.limbs[l].PhysicalBehaviour;
        let at = self.physicals[p].transform.position;
        self.skin_add_damage_point(l, super::misc::damage_type::DISMEMBERMENT, [at[0], at[1]], 15.0);
        self.coroutine_seq += 1;
        let owner = self.limbs[l].id;
        self.end_of_frame.push(Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner, from: i64::MIN, step: Step::LimbCrushEndOfFrame(l) });
    }

    /// LimbBehaviour.Slice
    pub(crate) fn limb_slice(&mut self, l: usize) {
        if self.limbs[l].ImmuneToDamage {
            return;
        }
        let person = self.limbs[l].Person;
        if self.prefs.StopAnimationOnDamage && self.limb_is_considered_alive(l) {
            self.people[person].OverridePoseIndex = -1;
        }
        if self.limb_is_considered_alive(l) {
            self.people[person].AdrenalineLevel += 1.0;
        }
        if self.limbs[l].HasJoint {
            self.limbs[l].RegenerationSpeed = 0.0;
            self.limbs[l].Health = 0.0;
            let h = self.limbs[l].Joint.expect("MissingReferenceException: HasJoint without a Joint");
            let anchor = self.hinges[h].anchor;
            let at = self.transform_point3(self.limbs[l].PhysicalBehaviour, [anchor[0], anchor[1], 0.0]);
            self.act_on_impact(l, 15.0, at);
            self.limbs[l].BreakingThreshold = 0.0;
        }
    }

    /// LimbBehaviour.ExitShot
    pub(crate) fn limb_exit_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        use super::misc::damage_type::BULLET;
        if self.limbs[l].ImmuneToDamage {
            return;
        }
        let mut damage = damage / self.mass_strength_ratio(l);
        damage *= self.prefs.FragilityMultiplier;
        self.skin_add_damage_point(l, BULLET, point, mathf::max(60.0, damage * 0.4));
        let (person, p) = (self.limbs[l].Person, self.limbs[l].PhysicalBehaviour);
        if let Some(effect) = self.people[person].PoolableImpactEffect.filter(|_| !self.prefs.GorelessMode && self.prefs.ChunkyShotParticles)
            && (self.physicals[p].BurnProgress as f64) < 0.6
            && self.skins[l].acidProgress < 0.7
            && damage > self.people[person].ImpactEffectShotDamageThreshold
            && self.random.value() > 0.6
        {
            // A recording names the effect without holding its prefab: nothing of it reaches the simulation.
            if let Some(effect) = effect {
                self.request_pooled_effect(effect, "LimbBehaviour.ExitShot", point, angle_of(normal), 1.0);
            }
        }
        if self.limbs[l].HasLungs && !self.limbs[l].IsAndroid && self.random.value() > 0.9 {
            self.limbs[l].LungsPunctured = true;
        }
        if self.prefs.StopAnimationOnDamage && self.limb_is_considered_alive(l) {
            self.people[person].OverridePoseIndex = -1;
        }
        if !self.prefs.GorelessMode && !self.limbs[l].KillShotParticlesEmitted && self.limbs[l].Health <= mathf::EPSILON && self.limbs[l].KillShotParticles && self.random.value() > 0.7 {
            // KillShotParticles.transform.right = shot.normal; KillShotParticles.Play();
            let id = self.limbs[l].id;
            if let Some(t) = self.field_component(id, "KillShotParticles") {
                let q = crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], [normal[0], normal[1], 0.0]);
                self.with_component_instance(t, |_, inst, o| inst.set_world_rotation(o, q));
            }
            self.ps_field_play_stop(id, "KillShotParticles", true);
            // PhysicalBehaviour.PlayClipOnce(Person.DismembermentClips.PickRandom()).
            let clip = self.pick_clip(self.people[person].id, "DismembermentClips", self.people[person].DismembermentClips);
            self.play_clip_once(self.limbs[l].PhysicalBehaviour, clip, 1.0);
            self.limbs[l].KillShotParticlesEmitted = true;
        }
        if self.limbs[l].IsAndroid || self.amount_of_blood(l) < 0.05 {
            return;
        }
        let fallback = self.original_blood_color(l);
        let colour = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
        // The loop bound Random.Range(1, 4) is drawn again before each pass.
        let mut i = 0;
        while i < self.random.range_int(1, 4) {
            let r = self.random.inside_unit_circle();
            let direction = [normal[0] + r[0] * 0.4, normal[1] + r[1] * 0.4];
            let hit = self.raycast(V::new(point[0], point[1]), direction, 3.0, super::explosion::DEFAULT_RAYCAST_LAYERS, f32::NEG_INFINITY, f32::INFINITY);
            if let Some(hit) = hit {
                // raycastHit2D.transform.gameObject.SendMessage("Decal", new DecalInstruction(BloodDecal, point, colour))
                match self.hit_transform_physical(&hit) {
                    Some(other) => {
                        let ty = self.limb_blood_decal(l);
                        self.physical_decal(other, ty, [hit.point.x, hit.point.y], 1.0, colour);
                    }
                    None if self.bare_colliders[&hit.collider].receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"]) => {
                        unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                    }
                    None => {}
                }
            }
            i += 1;
        }
    }

    /// CirculationBehaviour.Shot
    pub(crate) fn circulation_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        if self.circulations[l].ImmuneToDamage || (self.limbs[l].IsAndroid && damage < 50.0) {
            return;
        }
        let damage = damage * self.limbs[l].ShotDamageMultiplier;
        if damage <= mathf::EPSILON {
            return;
        }
        let softness = self.properties[self.physicals[self.limbs[l].PhysicalBehaviour].Properties].Softness;
        if !self.limbs[l].IsZombie && d(self.random.value()) < d(softness) + 0.001f32 {
            let flag = self.is_world_point_in_artery_rect(l, point);
            let rate = self.circulations[l].BleedingRate;
            self.circulations[l].BleedingRate = (d(rate) + d(mathf::max(0.5, damage / 3.5)) * if flag { 2.0f32 } else { 1.0 }).f();
            self.create_bleeding_particle(l, point, normal, if flag { 1.0 } else { 0.0 });
        }
        if !self.limbs[l].IsAndroid && !self.limbs[l].IsZombie && self.random.value() > 0.2 && self.is_world_point_in_vital_part(l, point) {
            self.circulations[l].IsPump = false;
        }
        self.circulations[l].GunshotWoundCount = self.circulations[l].GunshotWoundCount.wrapping_add(1);
    }

    /// CirculationBehaviour.ExitShot (shot.damage = Limb.ShotDamageMultiplier)
    pub(crate) fn circulation_exit_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2]) {
        if self.circulations[l].ImmuneToDamage || self.limbs[l].IsZombie || self.limbs[l].IsAndroid {
            return;
        }
        let damage = self.limbs[l].ShotDamageMultiplier;
        if damage <= mathf::EPSILON {
            return;
        }
        let flag = self.is_world_point_in_artery_rect(l, point);
        let rate = self.circulations[l].BleedingRate;
        self.circulations[l].BleedingRate = (d(rate) + d(mathf::max(0.5, damage / 10.0)) * if flag { 4.0f32 } else { 1.0 }).f();
        self.create_bleeding_particle(l, point, normal, if flag { 1.0 } else { 0.0 });
        if self.random.value() > 0.2 && self.is_world_point_in_vital_part(l, point) {
            self.circulations[l].IsPump = false;
        }
    }

    /// PhysicalBehaviour.Shot
    pub(crate) fn physical_shot(&mut self, p: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        // PlayClipOnce(OverrideShotSounds.PickRandom(), 1f)
        if self.physicals[p].OverrideShotSounds != 0 {
            let clip = self.pick_clip(self.physicals[p].id, "OverrideShotSounds", self.physicals[p].OverrideShotSounds);
            self.play_clip_once(p, clip, 1.0);
        }
        if self.properties[self.physicals[p].Properties].ShotImpact.is_some() {
            let size = mathf::clamp(mathf::pow(damage, 0.6) / 6.0, 0.1, 3.0) / 2.0;
            self.create_impact_effect(p, point, normal, size);
        }
    }

    /// PhysicalBehaviour.ExitShot
    pub(crate) fn physical_exit_shot(&mut self, p: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        if self.properties[self.physicals[p].Properties].ShotImpact.is_some() {
            let size = mathf::clamp(mathf::pow(damage, 0.6) / 3.0, 0.1, 4.0);
            self.create_impact_effect(p, point, normal, size);
        }
    }

    /// DestroyableBehaviour.Shot
    pub(crate) fn destroyable_shot(&mut self, x: usize, normal: [f32; 2], damage: f32) {
        let s = &self.destroyables[x];
        if s.ReactToBullets && !(damage < s.MinimumShotDamage) && !(s.ShotBreakChance < self.random.value()) {
            match self.destroyable_explosive(x) {
                Some(e) if self.explosives[e].ShotExplodeChance > mathf::EPSILON => self.explosive_force_immediate(e),
                _ => {
                    let owner = self.destroyables[x].id;
                    self.start_wait(owner, 0.05, Step::DestroyableShotBreak(x, normal));
                }
            }
        }
    }

    // DestroyableBehaviour.System.Collections.IEnumerator.get_Current: the WaitAFrame iterator's plumbing.
    // DestroyableBehaviour.System.Collections.Generic.IEnumerator<System.Object>.get_Current: likewise.
    // DestroyableBehaviour.System.Collections.IEnumerator.Reset: likewise (never called).
    // DestroyableBehaviour.System.IDisposable.Dispose: likewise (empty).
    /// DestroyableBehaviour.MoveNext (Shot's WaitAFrame) after its 0.05 s.
    pub(crate) fn destroyable_shot_break(&mut self, x: usize, normal: [f32; 2]) {
        let m = 0.0 - self.destroyables[x].MinimumShotDamage;
        self.destroyable_break(x, [normal[0] * m / 20.0, normal[1] * m / 20.0]);
    }

    /// ExplosiveBehaviour.Shot (triggersExplosiveOverride is always true for bullets).
    /// ExplosiveBehaviour.Shot: `triggers_explosive_override` is shot.triggersExplosiveOverride (it short-circuits the draw).
    pub(crate) fn explosive_shot(&mut self, e: usize, triggers_explosive_override: bool) {
        if triggers_explosive_override && self.random.value() < self.explosives[e].ShotExplodeChance {
            let delay = self.random.value() * 0.2;
            self.explosive_start_explode(e, delay, true);
        }
    }

    /// DamagableMachineryBehaviour.Shot
    pub(crate) fn machinery_shot(&mut self, m: usize, damage: f32) {
        let mc = &mut self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed {
            mc.Health = (d(mc.Health) - d(damage) * mc.FirearmDamageMultiplier).f();
            self.machinery_act_on_damage(m);
        }
    }

    /// LavaBehaviour.IsPointInLava: the lavas whose objects are active (their FixedUpdate is listed).
    pub(crate) fn is_point_in_lava(&mut self, point: [f32; 2]) -> bool {
        self.lava_at_point(point).is_some()
    }

    /// LavaBehaviour.GetLavaAtPoint (and LavaBehaviour.TryGetLavaAtPoint, whose `out lava` is the Option): the first lava in
    /// `lavas` whose object is active in the hierarchy (its FixedUpdate is listed) and whose Trigger contains the point.
    /// `lavas` is LavaBehaviour.lavas: LavaBehaviour.Awake appends at link time, LavaBehaviour.OnDestroy's Remove is the
    /// object's behaviours leaving the list.
    pub(crate) fn lava_at_point(&mut self, point: [f32; 2]) -> Option<usize> {
        for lv in 0..self.lavas.len() {
            let active = self.behaviours.list(super::Phase::Fixed).iter().any(|e| matches!(e.1, Behaviour::Lava(x) if x == lv));
            if active && self.lava_is_point_inside(lv, point) {
                return Some(lv);
            }
        }
        None
    }
}

/// The z angle of transform.right = v, in degrees.
fn angle_of(v: [f32; 2]) -> f32 {
    crate::game::mathf::atan2_f64(v[1] as f64, v[0] as f64).to_degrees() as f32
}

#[cfg(test)]
mod distance_tests {
    use super::*;

    /// A shake turns the camera about z by 0.5 * ShakeIntensity * (Noise(0) * intensity * 0.25)^2 degrees.
    #[test]
    fn a_shake_rolls_the_camera() {
        let mut g = Game::default();
        g.prefs.ShakeIntensity = 1.0;
        g.camera.shakeIntensity = 5.0;
        let mut max = 0.0f32;
        for k in 0..200 {
            g.time.time = k as f32 * 0.02;
            g.camera_shake_update();
            let q = g.camera.rotation;
            assert!(q[0] == 0.0 && q[1] == 0.0 && (q[2] * q[2] + q[3] * q[3] - 1.0).abs() < 1e-6, "{q:?}");
            max = max.max(q[2].abs());
        }
        // At most (1 * 5 * 0.25)^2 * 0.5 = 0.78 degrees: sin(0.39 degrees) = 0.0068.
        assert!(max > 1e-4 && max < 0.0069, "{max}");
        g.camera.shakeIntensity = 0.0;
        g.camera_shake_update();
        assert_eq!(g.camera.rotation, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn multipliers_by_distance() {
        // Max(0.1, Sqrt(0)) = 0.1 -> 10; Sqrt(5 * 0.2) = 1 -> 1.
        assert_eq!(speed_multiplier_by_distance(0.0), 10.0);
        assert_eq!(speed_multiplier_by_distance(5.0), 1.0);
        // 1.5 + 0.7 / Lerp(1, 0.8, 0.992), inside the clamp; far away the clamp floor holds.
        assert!((damage_multiplier_by_distance(1.0) - 2.3732).abs() < 1e-3);
        assert_eq!(damage_multiplier_by_distance(1000.0), 0.5);
        assert_eq!(damage_multiplier_by_distance(0.01), 10.0);
    }
}
