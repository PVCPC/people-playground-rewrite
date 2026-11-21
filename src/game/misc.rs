//! The small per-limb scripts: ConnectedNodeBehaviour, SkinMaterialHandler, GForceMeasureBehaviour,
//! GripBehaviour, BleedingParticleBehaviour and FancyBloodSplatController.
use super::{d, mathf, script, BreakJoint, Field, Game};
use crate::b2::prismatic::SliderJoint2d;
use crate::b2::weld::FixedJoint2d;
use crate::b2::world::JointId;

/// FixedIntervalDistributor
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedIntervalDistributor {
    pub timeStepAccumulator: f32,
    pub MaxCycles: i32,
    pub InterpolationTime: f32,
    pub IntervalSeconds: f32,
}

impl Default for FixedIntervalDistributor {
    fn default() -> Self {
        Self {
            timeStepAccumulator: Default::default(),
            MaxCycles: 256,
            InterpolationTime: Default::default(),
            IntervalSeconds: 0.016666668,
        }
    }
}

/// gapB r12: a component's private FixedIntervalDistributor (BeamRepeater dist, Firearm clock, Metronome/Dangle
/// distributor, ..) as the recorder's Format writes it, so the per-function checks load its accumulator and compare it.
impl super::Field for FixedIntervalDistributor {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        let inner = text.strip_prefix("FixedIntervalDistributor(").and_then(|t| t.strip_suffix(')')).ok_or_else(|| format!("not a FixedIntervalDistributor: {text}"))?;
        for kv in inner.split(' ') {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("bad FixedIntervalDistributor field {kv}"))?;
            match k.strip_prefix('<').and_then(|k| k.strip_suffix(">k__BackingField")).unwrap_or(k) {
                "timeStepAccumulator" => super::Field::parse(&mut self.timeStepAccumulator, v)?,
                "MaxCycles" => super::Field::parse(&mut self.MaxCycles, v)?,
                "InterpolationTime" => super::Field::parse(&mut self.InterpolationTime, v)?,
                "IntervalSeconds" => super::Field::parse(&mut self.IntervalSeconds, v)?,
                k => return Err(format!("FixedIntervalDistributor has no field {k}")),
            }
        }
        Ok(())
    }
    fn show(&self) -> String {
        format!("FixedIntervalDistributor(<MaxCycles>k__BackingField={} <InterpolationTime>k__BackingField={} timeStepAccumulator={} <IntervalSeconds>k__BackingField={})",
            self.MaxCycles, self.InterpolationTime.show(), self.timeStepAccumulator.show(), self.IntervalSeconds.show())
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        Err(format!("FixedIntervalDistributor is not serialized: {v:?}"))
    }
}

impl FixedIntervalDistributor {
    /// FixedIntervalDistributor.get_RateHz
    pub fn get_RateHz(&self) -> f32 {
        1.0 / self.IntervalSeconds
    }
    /// FixedIntervalDistributor.set_RateHz
    pub fn set_RateHz(&mut self, value: f32) {
        self.IntervalSeconds = 1.0 / value;
    }
    /// FixedIntervalDistributor.get_MaxRate (obsolete): MaxCycles.
    pub fn get_MaxRate(&self) -> i32 {
        self.MaxCycles
    }
    /// FixedIntervalDistributor.set_MaxRate (obsolete): MaxCycles = value.
    pub fn set_MaxRate(&mut self, value: i32) {
        self.MaxCycles = value;
    }
    /// FixedIntervalDistributor.ResetAccumulator
    pub fn reset_accumulator(&mut self, t: f32) {
        self.timeStepAccumulator = t;
    }
    pub fn with_rate(hz: f32) -> Self {
        Self { timeStepAccumulator: 0.0, MaxCycles: 256, InterpolationTime: 0.0, IntervalSeconds: 1.0 / hz }
    }
    /// FixedIntervalDistributor.CalculateCycleCount
    pub fn calculate_cycle_count(&mut self, dt: f32) -> i32 {
        self.timeStepAccumulator += dt;
        let mut num = 0;
        let interval = self.IntervalSeconds;
        while self.timeStepAccumulator > interval {
            self.timeStepAccumulator -= interval;
            num += 1;
        }
        self.InterpolationTime = self.timeStepAccumulator / interval;
        num.min(self.MaxCycles)
    }
}

script! {
    Node {
        IsRoot: bool = false,
        Value: i32 = 0,
        ShouldLoadSerialisableConnections: bool = false,
        IsConnectedToRoot: bool = true,
    }
    links {
        /// Connections as limb indices; a destroyed or disconnected slot is None.
        Connections: Vec<Option<usize>> = Vec::new(),
    }
}

/// DamageType
pub mod damage_type {
    pub const BLUNT: i32 = 0;
    pub const BULLET: i32 = 1;
    pub const STAB: i32 = 2;
    pub const DISMEMBERMENT: i32 = 3;
    pub const BURN: i32 = 4;
    pub const NONE: i32 = 5;
}

script! {
    Skin {
        BluntDamageRegen: bool = true,
        GunshotDamageRegen: bool = true,
        BurnWoundRegen: bool = true,
        TrackWoundAge: bool = true,
        ShouldRecolourWhenCold: bool = false,
        ShouldGlowWhenHot: bool = false,
        MaxGlowTemperature: i32 = 1000,
        MinFreezeTemperature: i32 = -30,
        currentDamagePointCount: i32 = 0,
        intensityMultiplier: f32 = 1.0,
        rottenProgress: f32 = 0.0,
        oldRottenProgress: f32 = 0.0,
        acidProgress: f32 = 0.0,
        oldAcidProgress: f32 = 0.0,
        ShouldRot: bool = true,
    }
    links {
        /// Vector4 (x, y, intensity, DamageType)
        damagePoints: Vec<[f32; 4]> = vec![[0.0, 0.0, 0.0, damage_type::NONE as f32]; 128],
        damagePointTimeStamps: Vec<f32> = vec![0.0; 128],
        intervalDistributor: FixedIntervalDistributor = FixedIntervalDistributor::with_rate(16.0),
        adjacentLimbs: Vec<usize> = Vec::new(),
        /// The material's _AcidProgress: only the setter's jumps past oldAcidProgress reach it.
        materialAcid: f32 = 0.0,
        /// The renderer's sprite once PersonBehaviour.SetBodyTextures replaced it (a LimbSpriteCache sprite), and the
        /// _FleshTex and _BoneTex it set.
        sprite: Option<usize> = None,
        flesh_texture: Option<crate::assets::db::ObjRef> = None,
        bone_texture: Option<crate::assets::db::ObjRef> = None,
        /// The material colours PersonBehaviour.SetShaderColour set: _BruiseColor, _SecondBruiseColor, _ThirdBruiseColor, _Zombie,
        /// and _BloodColor (also set by SkinMaterialHandler.OnWillRenderObject before each draw, limb_material.rs sync).
        colours: [Option<[f32; 4]>; 5] = [None; 5],
        /// Counts the changes the renderer has to draw (sprite and textures).
        version: u32 = 0,
    }
}

impl Skin {
    /// SkinMaterialHandler.set_RottenProgress (the material always exists).
    pub fn set_rotten(&mut self, value: f32) {
        let num = mathf::clamp01(value);
        if (self.oldRottenProgress - num).abs() > 0.01 {
            self.oldRottenProgress = num;
        }
        self.rottenProgress = num;
    }
    /// SkinMaterialHandler.set_AcidProgress; the original stores the previous value as oldAcidProgress.
    pub fn set_acid(&mut self, value: f32) {
        let num = mathf::clamp01(value);
        if (self.oldAcidProgress - num).abs() > 0.01 {
            self.oldAcidProgress = self.acidProgress;
            self.materialAcid = num;
        }
        self.acidProgress = num;
    }
}

script! {
    GForce {
        SustainedAccelerationRatio: f32 = 0.3,
        velocity: [f32; 2] = [0.0, 0.0],
        velocityLastFrame: [f32; 2] = [0.0, 0.0],
        acceleration: [f32; 2] = [0.0, 0.0],
        sustainedAcceleration: [f32; 2] = [0.0, 0.0],
    }
    links {}
}

script! {
    Grip {
        GripPosition: [f32; 3] = [0.0; 3],
        isHolding: bool = false,
        NearestHoldingPos: [f32; 2] = [0.0; 2],
        Anchor: [f32; 2] = [0.0; 2],
        ConnectedAnchor: [f32; 2] = [0.0; 2],
    }
    links {
        /// The component's instance id (its coroutine's owner).
        id: i32 = 0,
        PhysicalBehaviour: usize = 0,
        /// The held PhysicalBehaviour.
        CurrentlyHolding: Option<usize> = None,
        /// joint: the `Game::fixed_joints` entry of the FixedJoint2D CreateJoint added (kept after its Destroy).
        joint: Option<usize> = None,
        /// CollidersToIgnore: the root's colliders as Start found them.
        CollidersToIgnore: Vec<i32> = Vec::new(),
        /// originalRotation: the hand's eulerAngles.z minus the held object's, when CreateJoint (or OnChangePause) last set it.
        originalRotation: f32 = 0.0,
    }
}

script! {
    BleedingParticle {
        IsOnlyForInternalBleeding: bool = false,
        SpeedMultiplier: f32 = 1.0,
        RateMultiplier: f32 = 1.0,
        Laminarity: f32 = 0.0,
        DripSoundRateThreshold: f32 = 2.0,
        ShouldBecomeSmokeInWater: bool = false,
        PulseSpeed: f32 = 12.0,
        PulseIntensity: f32 = 2.0,
        fakeHeartClock: f32 = 0.0,
    }
    links {
        /// CirculationBehaviour / PushingTo as limb indices (CirculationBehaviour null under a parent that is not a limb).
        CirculationBehaviour: Option<usize> = None,
        /// The physical whose Transform the particle hangs under.
        Parent: usize = 0,
        PushingTo: Option<usize> = None,
        BloodSplatController: Option<usize> = None,
        /// BleedingParticleBehaviour.Awake's dripSoundClock (`BleedingParticle::awake`).
        dripSoundClock: FixedIntervalDistributor = FixedIntervalDistributor::with_rate(0.5),
        /// DripSounds length; None when DripSounds is null.
        DripSounds: Option<usize> = None,
        /// (bool)WaterPuffSystem: the prefab's puff system, which Start destroys unless ShouldBecomeSmokeInWater.
        WaterPuffSystem: bool = false,
        /// ParticleSystemRenderer.isVisible, owned by the renderer.
        visible: bool = false,
        /// The prefab's GameObject, and the transform under Parent the emitter sits at.
        prefab: Option<crate::assets::db::ObjRef> = None,
        /// The water puff child stayed active (its ActivateIfFancyEffects); BleedingParticleBehaviour.Start destroys it.
        water_puff: bool = false,
        /// Loaded from a recording: its systems were played long before (only the root is still listed).
        loaded: bool = false,
        /// A loaded one's recorded ParticleSystemManager index (-1: not listed); None in recordings before it.
        psm: Option<i32> = None,
        /// A loaded one's systems (the root, then its children): renderer isVisible, culled, cullTime, stopTime.
        pss: Vec<(bool, bool, f64, f64)> = Vec::new(),
        /// A loaded one's root ParticleSystem randoms as recorded (pseed=): randomSeed, the emission Rand's 4 words, then the
        /// Initial, Shape and Collision RandNs' 16 words each in memory order. None in recordings before it.
        pseed: Option<Vec<u32>> = None,
        local_position: [f32; 3] = [0.0; 3],
        local_up: [f32; 3] = [0.0, 1.0, 0.0],
        /// ParticleSystem main/emission values the renderer draws from.
        color: [f32; 4] = [0.0; 4],
        startSpeedMultiplier: f32 = 0.0,
        rateOverTimeMultiplier: f32 = 0.0,
        emission_enabled: bool = false,
        destroyed: bool = false,
        /// BleedingParticleBehaviour.Awake's collision.enabled = true under FancyEffects (read by particles.rs `collision_of`).
        collision: bool = false,
        /// Its Instantiate among the frame's effect clones (`Game::effect_stamp`): its systems join the manager there.
        log_at: (usize, u64) = (0, 0),
        /// Start's Object.Destroy(WaterPuffSystem): its queued time until the delayed destroy runs.
        puff_destroy_at: Option<f64> = None,
    }
}

impl BleedingParticle {
    /// BleedingParticleBehaviour.Awake: the ParticleSystem, its renderer, MainModule and EmissionModule are kept (written by Update and drawn in
    /// particles.rs `bleeds`), dripSoundClock = new FixedIntervalDistributor { RateHz = 0.5f }, and the CollisionModule is enabled under FancyEffects.
    pub fn awake(&mut self, fancy_effects: bool) {
        self.dripSoundClock = FixedIntervalDistributor::with_rate(0.5);
        self.collision = fancy_effects;
    }
}

/// HashSet<T> (System.Core) in slot order: enumeration walks the slots, Remove pushes its slot onto the free list,
/// Add takes the latest freed slot, and removing the last element resets both.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotSet<T> {
    pub slots: Vec<Option<T>>,
    /// The free list; the next Add takes the last.
    pub free: Vec<usize>,
}

impl<T> Default for SlotSet<T> {
    fn default() -> Self {
        Self { slots: Vec::new(), free: Vec::new() }
    }
}

impl<T> SlotSet<T> {
    pub fn add(&mut self, v: T) {
        match self.free.pop() {
            Some(i) => self.slots[i] = Some(v),
            None => self.slots.push(Some(v)),
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().flatten()
    }

    /// HashSet.RemoveWhere: Remove for each match in slot order.
    pub fn remove_where(&mut self, f: impl Fn(&T) -> bool) {
        for i in 0..self.slots.len() {
            if self.slots[i].as_ref().is_some_and(&f) {
                self.slots[i] = None;
                if self.is_empty() {
                    self.clear();
                    return;
                }
                self.free.push(i);
            }
        }
    }
}

/// FancyBloodSplatController.Particle
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplatParticle {
    pub VelocityX: f32,
    pub VelocityY: f32,
    pub PositionX: f32,
    pub PositionY: f32,
    pub Life: f32,
}

impl SplatParticle {
    /// Particle.get_Velocity (FancyBloodSplatController.Particle): new Vector2(VelocityX, VelocityY).
    #[allow(dead_code)]
    pub fn get_Velocity(&self) -> [f32; 2] {
        [self.VelocityX, self.VelocityY]
    }
}

#[cfg(test)]
mod splat_particle_tests {
    #[test]
    fn velocity_is_the_two_components() {
        let p = super::SplatParticle { VelocityX: 1.5, VelocityY: -2.0, PositionX: 9.0, PositionY: 9.0, Life: 1.0 };
        assert_eq!(p.get_Velocity(), [1.5, -2.0]);
    }
}

script! {
    FancyBloodSplat {
        UnparentAtStart: bool = true,
        DirectionMode: i32 = 0,
        Continuous: bool = false,
        Direction: [f32; 2] = [1.0, 0.0],
        SpawningChance: f32 = 0.1,
        AmountRange: [f32; 2] = [1.0, 4.0],
        AngleRandomisation: f32 = 15.0,
        LifetimeInSeconds: [f32; 2] = [1.0, 2.0],
        EjectionForce: [f32; 2] = [1.0, 5.0],
        DecalColourMultiplier: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        Size: f32 = 0.2,
        DestroyWhenNoParticles: bool = true,
        TriggerOnCollide: bool = false,
    }
    links {
        /// The DecalDescriptor's name (every splat prefab uses Sparse blood).
        Decal: String = "Sparse_blood".into(),
        particles: SlotSet<SplatParticle> = SlotSet::default(),
        /// initialParent world position and the controller's own rotation (read live when attached).
        parent_position: [f32; 3] = [0.0, 0.0, 0.0],
        rotation: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
        /// A bleeding particle's splat: the limb's physical object, the bleeding particle's local Transform and
        /// the controller's local rotation under it. None for a BloodExplosion's (unparented at Start).
        attached: Option<(usize, crate::unity::transform::Trs, [f32; 4])> = None,
        /// A splat whose initialParent is a Transform node (a bare Rigidbody2D's): that node and the controller's local
        /// rotation. Its pose is the node's chain (the node's body drives it).
        attached_node: Option<(usize, [f32; 4])> = None,
        /// False between Instantiate and the delayed Start (loaded scenes are already started).
        started: bool = true,
        destroyed: bool = false,
        CollisionLayers: u32 = 0,
        /// initialParent destroyed (a Clear everything removed its person): `(bool)initialParent` fails, so no particle collides.
        parent_gone: bool = false,
        /// The person whose hierarchy initialParent is in; None for a BloodExplosion, a root of its own.
        parent_person: Option<usize> = None,
        /// The component's instance id when instantiated from a prefab (0: modelled by its spawner).
        id: i32 = 0,
        /// The LiquidContainerHoleBehaviour (`Game::liquid_holes`) whose OnCollide listener this splat calls.
        on_hole: Option<usize> = None,
        /// The BloodTankBehaviour or LiquidOutletBehaviour whose OnCollide listener this splat calls; None without one.
        on_collide: Option<super::container::Owner> = None,
    }
}

/// One DecalControllerBehaviour decal: its local position (localDecalPositions), size, sprite and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Decal {
    pub position: [f32; 2],
    pub size: f32,
    pub sprite: usize,
    pub colour: [f32; 4],
    /// localScale: holder.InverseTransformVector(size * Vector3.one), mirrored or squashed by the object's rotation.
    pub scale: [f32; 2],
    pub streaks: Vec<Streak>,
}

/// SkinMaterialHandler.colliderBuffer's length (its static setup: new Collider2D[16]): Update's OverlapPointNonAlloc cap.
pub(crate) const SKIN_COLLIDER_BUFFER: usize = 16;

/// DecalDripStreakBehaviour.static setup: MinLocalStreakLength 1, MaxLocalStreakLength 3, StreakDripSpeed 0.2.
pub(crate) const MIN_LOCAL_STREAK_LENGTH: f32 = 1.0;
pub(crate) const MAX_LOCAL_STREAK_LENGTH: f32 = 3.0;
pub(crate) const STREAK_DRIP_SPEED: f32 = 0.2;

/// A DecalDripStreakBehaviour: presentation only, its Update takes no draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Streak {
    /// localPosition under the decal.
    pub position: [f32; 2],
    /// localScale.x and .y
    pub scale: [f32; 2],
    /// localEulerAngles.z
    pub z: f32,
    pub radius: f32,
    pub speed: f32,
    pub length: f32,
    pub t: f32,
}

/// A DecalDescriptor asset: the fields DecalControllerBehaviour reads (DripParticle is only the streaks' sprite).
#[derive(Clone, Debug, PartialEq)]
pub struct DecalDescriptor {
    pub name: String,
    pub Color: [f32; 4],
    pub IgnoreRadius: f32,
    /// Sprites.Length
    pub Sprites: usize,
    pub MinDripCount: i32,
    pub MaxDripCount: i32,
}

impl Default for DecalDescriptor {
    fn default() -> Self {
        Self {
            name: Default::default(),
            Color: [1.0; 4],
            IgnoreRadius: 0.2,
            Sprites: Default::default(),
            MinDripCount: Default::default(),
            MaxDripCount: Default::default(),
        }
    }
}

/// DecalDripStreakBehaviour.EaseOut: 1 - (1 - x)^3 as `num * num * num`.
pub fn decal_drip_streak_ease_out(x: f32) -> f32 {
    let num = 1.0 - x;
    (1.0 - super::d(num) * num * num).f() // the product in double (Mono's float temporaries)
}

impl DecalDescriptor {
    /// DecalDescriptor.get_WillDrip: MaxDripCount > 0.
    pub fn will_drip(&self) -> bool {
        self.MaxDripCount > 0
    }

    /// The ScriptableObject `name` with its serialized fields.
    pub fn read(name: &str, v: &crate::assets::mono::Value) -> Result<Self, String> {
        let get = |n: &str| v.get(n).ok_or_else(|| format!("DecalDescriptor {name}: no {n}"));
        let mut d = DecalDescriptor { name: name.to_string(), Color: [0.0; 4], IgnoreRadius: 0.0, Sprites: 0, MinDripCount: 0, MaxDripCount: 0 };
        d.Color.set(get("Color")?)?;
        d.IgnoreRadius.set(get("IgnoreRadius")?)?;
        d.Sprites = get("Sprites")?.array().ok_or("DecalDescriptor.Sprites")?.len();
        d.MinDripCount.set(get("MinDripCount")?)?;
        d.MaxDripCount.set(get("MaxDripCount")?)?;
        Ok(d)
    }

    /// What recordings (which carry no AssetDb) stand for: the two blood descriptors as the game data holds them
    /// (`decal_descriptors_match_data` compares them).
    pub fn shipped(name: &str) -> Option<Self> {
        let (name, radius) = match name.replace(' ', "_").as_str() {
            "Sparse_blood" => ("Sparse blood", 0.2),
            "Dense_blood" => ("Dense blood", 0.3),
            _ => return None,
        };
        Some(DecalDescriptor { name: name.into(), Color: [1.0; 4], IgnoreRadius: radius, Sprites: 32, MinDripCount: 0, MaxDripCount: 0 })
    }
}

/// AddForceBehaviour (no lifecycle): AddRelativeForce(intensity) is called by UnityEvents.
#[derive(Clone, Debug)]
pub struct AddForce {
    pub id: i32,
    /// The PhysicalBehaviour of its GameObject (GetComponent<Rigidbody2D>()).
    pub physical: usize,
    /// ForceMode2D: 0 Force, 1 Impulse
    pub ForceMode: i32,
    pub LocalAxis: [f32; 2],
    pub ScaleWithSize: bool,
}

/// JointBreakSoundBehaviour: the joints on its own GameObject, as the break limit keys they sit under.
#[derive(Clone, Debug)]
pub struct JointBreakSound {
    /// The JointBreakSoundBehaviour's instance id.
    pub id: i32,
    pub joints: Vec<BreakJoint>,
    /// OnBreak's ExplosiveBehaviour.ForceImmediateExplosion targets (`Game::explosives`).
    pub explosives: Vec<usize>,
    /// OnBreak's CarBehaviour calls, by component id.
    pub cars: Vec<(i32, super::car::CarCall)>,
    /// OnBreak's other component calls (AddForceBehaviour.AddRelativeForce), run after the above.
    pub calls: Vec<super::link::EventCall>,
    pub enabled: bool,
    /// Clips.Length
    pub clips: usize,
    /// GetComponent<PhysicalBehaviour>() on its object (PlayClipOnce when AudioSource is null).
    pub physical: Option<usize>,
}

impl Game {
    /// The arena index of the DecalDescriptor named `name` (a recording writes it with underscores).
    fn decal_descriptor_index(&self, name: &str) -> Option<usize> {
        self.decal_descriptors.iter().position(|d| d.name.replace(' ', "_") == name.replace(' ', "_"))
    }

    /// The DecalDescriptor asset's arena index, loading it on first use (assets are shared by name).
    pub fn load_decal_descriptor(&mut self, db: &mut crate::assets::db::AssetDb, asset: crate::assets::db::ObjRef) -> Result<usize, String> {
        let (_, mb, v) = db.mono(asset)?.ok_or("DecalDescriptor is not a MonoBehaviour")?;
        let i = match self.decal_descriptor_index(&mb.name) {
            Some(i) => i,
            None => {
                self.decal_descriptors.push(DecalDescriptor::read(&mb.name, &v)?);
                self.decal_descriptors.len() - 1
            }
        };
        if self.decal_sprites.contains_key(&i) {
            return Ok(i);
        }
        let resolve = |x: &crate::assets::mono::Value| x.pptr().filter(|p| !p.is_null()).and_then(|p| db.resolve(asset.file, p));
        let sprites = v.get("Sprites").and_then(|x| x.array()).ok_or("DecalDescriptor.Sprites")?.iter().map(|x| resolve(x).ok_or("a null decal Sprite")).collect::<Result<Vec<_>, _>>()?;
        let drip = v.get("DripParticle").and_then(resolve);
        self.decal_sprites.insert(i, (sprites, drip));
        Ok(i)
    }

    /// DecalDripStreakBehaviour.Update for the streaks still growing.
    pub fn decal_streaks_update(&mut self) {
        let (dt, gravity) = (self.time.deltaTime, self.world.gravity);
        for i in (0..self.active_streaks.len()).rev() {
            let [p, c, n, s] = self.active_streaks[i];
            let Some(decal) = self.physicals[p].decal_controllers.get(c).and_then(|x| x.decals.get(n)) else {
                self.active_streaks.swap_remove(i);
                continue;
            };
            let scale = decal.scale;
            // transform.parent.worldToLocalMatrix.MultiplyVector(Physics2D.gravity)
            let (a, b) = (self.inverse_transform_point(p, [gravity.x, gravity.y, 0.0]), self.inverse_transform_point(p, [0.0; 3]));
            let v = [(a[0] - b[0]) / scale[0], (a[1] - b[1]) / scale[1]];
            let z = crate::game::mathf::atan2(v[1], v[0]) * 57.29578;
            let num = (v[0] * v[0] + v[1] * v[1]) / 96.23611;
            let streak = &mut self.physicals[p].decal_controllers[c].decals[n].streaks[s];
            streak.t += dt * streak.speed * num;
            let ease = decal_drip_streak_ease_out(mathf::clamp01(streak.t));
            streak.scale = [streak.radius + ease * streak.length, streak.radius];
            streak.z = z;
            if streak.t > 1.0 || num <= mathf::EPSILON {
                self.active_streaks.swap_remove(i);
            }
        }
    }

    /// A DecalDescriptor by name: already linked, else from the game's data when the Game holds it, else (a
    /// recording) the shipped table.
    pub fn decal_descriptor(&mut self, name: &str) -> usize {
        // One the shipped table stood for while the AssetDb was away still needs its sprites.
        if let Some(i) = self.decal_descriptor_index(name).filter(|i| self.assets.is_none() || self.decal_sprites.contains_key(i)) {
            return i;
        }
        if let Some(mut db) = self.assets.take() {
            let found = db.find_scripts("DecalDescriptor").unwrap().into_iter().find(|&r| db.mono(r).unwrap().is_some_and(|(_, mb, _)| mb.name.replace(' ', "_") == name.replace(' ', "_")));
            let i = found.map(|r| self.load_decal_descriptor(&mut db, r).unwrap());
            self.assets = Some(db);
            if let Some(i) = i {
                return i;
            }
        }
        if let Some(i) = self.decal_descriptor_index(name) {
            return i;
        }
        let Some(d) = DecalDescriptor::shipped(name) else { unimplemented!("mod-only: DecalDescriptor {name}") };
        self.decal_descriptors.push(d);
        self.decal_descriptors.len() - 1
    }
}

/// A DecalControllerBehaviour: its descriptor (an index into `Game.decal_descriptors`) and its decals
/// (localDecalPositions with the sprites' data).
#[derive(Default, Clone, Debug)]
pub struct DecalController {
    pub descriptor: usize,
    pub decals: Vec<Decal>,
    /// `dirty`: CreateContainer has made the decalHolder; then the object's SpriteRenderer sprite (the SpriteMask's), its
    /// sortingLayerID and sortingOrder (originalSortingLayer) at that moment. The sprite is None for an object built from a
    /// recording (dump.rs), which keeps no SpriteRenderer sprite; such objects are never drawn.
    pub holder: Option<(Option<crate::assets::db::ObjRef>, i32, i16)>,
    /// !decalHolder.activeSelf as 1.28.45's InvisibleOptionInContextMenu.UpdateVisibility last set it.
    pub holder_hidden: bool,
    /// fc41 r12: the decalHolder's GameObject instance id, then its children's (Clear destroys them): CreateContainer's and
    /// Decal's new GameObjects (physical.rs), or a recording's Clone records (dump.rs).
    pub holder_objects: Vec<i32>,
}

/// A BloodPuff poolable (the Human PhysicalProperties' ShotImpact), from the pool PoolGenerator made for it.
#[derive(Clone, Debug)]
pub struct BloodPuff {
    /// gameObject.activeSelf: false while it waits in the pool.
    pub active: bool,
    /// The Extra decal emitter's FancyBloodSplatController and whether that child is active (ActivateIfFancyEffects).
    pub splat: usize,
    pub emitter_active: bool,
    /// The children that are inactive (ActivateIfFancyEffects, ExistifUnderwater.Start): they stay so in the pool.
    pub off: Vec<&'static str>,
}

impl super::Game {
    /// ParticleMachineBehaviour.UpdateActivation of the behaviour `m` (`particle_machines` index): setPlaying of the
    /// smoke, fire and electricity systems, Play when (enabled && Activated) & Emit, else Stop. From the OnDisable of a
    /// deactivated object (`deactivating`) the systems' GameObjects are inactive and Play (1801bd4e0) does nothing.
    pub(crate) fn particle_machine_update_activation(&mut self, m: usize, deactivating: bool) {
        for i in 0..self.embedded_ps.len() {
            let super::EmbeddedPs { owner: id, drive: super::EmbeddedDrive::ParticleMachine { k, init, enabled }, .. } = self.embedded_ps[i] else { continue };
            if !matches!(self.behaviour_of.get(&id), Some(&(_, super::Behaviour::ParticleMachine(x))) if x == m) {
                continue;
            }
            let emit = match self.script_slots.get(&id) {
                Some(&super::save_slots::Slot::ParticleMachines(j)) => self.particle_machine_emit[j],
                _ => init,
            };
            if self.script_enabled(id, enabled) && self.particle_machines[m] && emit[k] {
                if !deactivating {
                    self.push_ps_call((i, super::PsCall::Play));
                }
            } else {
                self.push_ps_call((i, super::PsCall::Stop(self.time.timeAsDouble)));
            }
        }
    }

    /// ParticleMachineBehaviour.Use: Activated flips; UpdateActivation.
    pub(crate) fn particle_machine_use(&mut self, m: usize) {
        self.particle_machines[m] = !self.particle_machines[m];
        self.particle_machine_update_activation(m, false);
    }

    /// ParticleMachineBehaviour.OnDisable: UpdateActivation() (of an enabled component; enabled = false, so base.enabled is false and every system stops).
    pub(crate) fn particle_machine_on_disable(&mut self, id: i32, enabled: bool, deactivating: bool) {
        if !enabled {
            return;
        }
        let Some(&(_, super::Behaviour::ParticleMachine(m))) = self.behaviour_of.get(&id) else { unreachable!("a ParticleMachineBehaviour in the arena") };
        if deactivating {
            self.particle_machine_update_activation(m, true);
        } else {
            self.ps_call(|e| e.owner == id && matches!(e.drive, super::EmbeddedDrive::ParticleMachine { .. }), super::PsCall::Stop(self.time.timeAsDouble));
        }
    }

    /// The `particle_machines` index of the behaviour whose context-menu slot is `j` (`particle_machine_emit`).
    pub(crate) fn particle_machine_of_slot(&self, j: usize) -> usize {
        self.script_slots.iter().find_map(|(id, s)| match (s, self.behaviour_of.get(id)) {
            (&super::save_slots::Slot::ParticleMachines(x), Some(&(_, super::Behaviour::ParticleMachine(m)))) if x == j => Some(m),
            _ => None,
        }).expect("a ParticleMachineBehaviour's slot")
    }
}

/// ObjectPoolBehaviour: createdObjects (a HashSet in slot order) and availableObjects (a Stack since 1.28.41: Request
/// pops the last one Returned, and Return pushes without a check, so one object returned twice is in it twice).
#[derive(Clone, Debug, Default)]
pub struct ObjectPool {
    pub created: Vec<usize>,
    pub available: Vec<usize>,
    pub inactivityTimer: f32,
}

impl ObjectPool {
    /// ObjectPoolBehaviour.GetPoolSize: createdObjects.Count.
    pub fn get_pool_size(&self) -> usize {
        self.created.len()
    }

    /// ObjectPoolBehaviour.GetInactivePoolSize: availableObjects.Count.
    pub fn get_inactive_pool_size(&self) -> usize {
        self.available.len()
    }
}

/// A SliderJoint2D or FixedJoint2D below a HingeJointLimitAutofixBehaviour's host, as the spawn left it.
#[derive(Clone, Debug, PartialEq)]
pub struct AutofixJoint {
    /// The Joint2D component's instance id.
    pub id: i32,
    /// The component's object and index in its prefab.
    pub object: usize,
    pub component: usize,
    pub state: FixedJoint2d,
    pub slider: Option<SliderJoint2d>,
    pub joint: Option<JointId>,
    /// The slot in `Game.anchored_joints` the break-limit walk reads.
    pub slot: Option<usize>,
    /// The physicals owning the attached and connected Rigidbody2D (None: the scene's ground body).
    pub attached: super::BodyOwner,
    pub connected: Option<super::BodyOwner>,
    pub enabled: bool,
}

/// What `GetComponentsInChildren` of HingeJoint2D, SliderJoint2D and FixedJoint2D finds below the host (the host and
/// its active descendants, depth first): the `Game::hinges` indices and the other two kinds' state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutofixJoints {
    /// The HingeJointLimitAutofixBehaviour's component id.
    pub id: i32,
    pub hinges: Vec<usize>,
    pub sliders: Vec<AutofixJoint>,
    pub fixeds: Vec<AutofixJoint>,
}

/// InvisibleOptionInContextMenu. Awake takes Invisible = !SpriteRenderer.enabled; Start adds the button and
/// UpdateVisibility, which LimbStatusToggled also runs (limb.rs).
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct Invisible {
    /// The component's instance id (OnDestroy unsubscribes).
    pub id: i32,
    /// Awake's GetComponent<PhysicalBehaviour>() (None: Start and UpdateVisibility throw).
    pub physical: Option<usize>,
    pub Invisible: bool,
    pub VisibleInDetailView: bool,
    /// What UpdateVisibility last wrote to phys.spriteRenderer.enabled (the renderer's own flag is physicals[p].sprite_enabled).
    pub sprite_enabled: bool,
}

impl Default for Invisible {
    fn default() -> Self {
        Self {
            id: Default::default(),
            physical: Default::default(),
            Invisible: Default::default(),
            VisibleInDetailView: true,
            sprite_enabled: Default::default(),
        }
    }
}

impl Game {
    /// InvisibleOptionInContextMenu.UpdateVisibility
    pub fn invisible_update(&mut self, i: usize) {
        let show = self.ShowLimbStatus;
        let s = &mut self.invisibles[i];
        // phys.spriteRenderer is read before the value: a null phys throws first.
        let Some(p) = s.physical else { super::menu::npe() };
        s.sprite_enabled = !s.Invisible || (s.VisibleInDetailView && show);
        let on = s.sprite_enabled;
        self.set_sprite_enabled(super::visual::SpriteAt::Physical(p), on);
        // 1.28.45: every phys.DecalControllers' decalHolder.SetActive(the same value); a controller without one (no decal yet) throws.
        if crate::assets::il::is_1_28_45() {
            for c in &mut self.physicals[p].decal_controllers {
                if c.holder.is_none() {
                    super::menu::npe();
                }
                c.holder_hidden = !on;
            }
        }
    }
}

/// The scripts whose Start only adds context menu buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Menu {
    Led(usize),
    Liquidentifier(usize),
    LiquidDuplicator(usize),
    LaserReceiver(usize),
    LiquidLevelGate(usize),
    KeyTrigger(usize),
    Counter(usize),
    Pointer(usize),
    Hovercar(usize),
    HeatingElement(usize),
    Gate(usize),
    Invisible(usize),
    Jukebox(usize),
    HoverThruster(usize),
    /// HeartMonitorBehaviour.Start: UpdateActivation.
    HeartMonitor(usize),
    Display(usize),
    /// The editable_tints index and the physical.
    EditableTint(usize, usize),
    /// particle_machine_emit index and the physical.
    ParticleMachine(usize, usize),
    LiquidLeak(usize),
}

/// A behaviour whose Start is still due.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingStart {
    /// 1.28.45's new classes (v45_new.rs).
    V45(super::v45_new::V),
    /// TrivialAnimationBehaviour.Start and ObjectGradientAnimation.Start (visual.rs).
    Visual(usize),
    /// GlowtubeBehaviour.Start
    Glowtube(usize),
    Centrifuge(usize),
    Valve(usize),
    /// ActivatorElectrodeBehaviour.Start
    ActivatorElectrode(usize),
    /// A Start that throws this managed exception (a missing component its GetComponent returned null for).
    Throw(String),
    Splat(usize),
    /// BloodTankBehaviour.Start (blood_tank.rs).
    BloodTank(usize),
    /// BleedingParticleBehaviour.Start by bleeding index.
    Bleeding(usize),
    /// PhysicalRopeCreator.Start by its component id.
    Rope(i32),
    Glow(usize),
    /// A spawned Person's scripts (`Game::spawn`), by arena index. PersonBehaviour.Start runs before its first
    /// LateUpdate; the rest after the LateUpdate phase.
    Person(usize),
    HingeJointLimitAutofix(usize),
    /// The same behaviour on an item that is no Person: the host's physical (Ok) or Transform node (Err).
    HingeJointLimitAutofixItem(Result<usize, usize>, usize),
    Circulation(usize),
    Physical(usize),
    Limb(usize),
    /// ConnectedNodeBehaviour with its serialized SerialisableConnectedNodes (None when null).
    Node(usize, Option<Vec<bool>>),
    /// GripBehaviour.Start with the root's colliders (CollidersToIgnore).
    Grip(usize),
    /// GoreStringBehaviour with its serialized CreateJointOnStart.
    GoreString(usize, bool),
    MapLight(usize),
    Water(usize),
    Machinery(usize),
    Destroyable(usize),
    /// UniversalBellBehaviour.Start: the note's sound arrays (universal_bell.rs).
    UniversalBell(usize),
    /// TimedGateBehaviour.Start: a busy gate resumes DoLag (timed_gate.rs).
    TimedGate(usize),
    /// LagboxBehaviour.Start (timed_gate.rs).
    Lagbox(usize),
    /// FireworksBehaviour.Start
    Fireworks(usize),
    /// CardiopulmonaryBypassMachineBehaviour.Start: UpdateActivation (resuscitation.rs).
    CardiopulmonaryBypass(usize),
    /// ArchelixCasterBehaviour.Start and ArchelixCasterBoltBehaviour.Start (archelix.rs).
    ArchelixCaster(usize),
    ArchelixBolt(usize),
    FireMap(usize),
    /// IncreaseStatOnStart.Start: StatName and IsFloat.
    IncreaseStat(String, bool),
    /// DetachFromParentOnStart.Start: its component id and DestroyIfParentDestroyed.
    DetachFromParent(i32, bool),
    /// PlayAtRandom.Start: Chance.
    PlayAtRandom(i32, f32),
    /// AudioSourceTimeScaleBehaviour.Start: its object's Transform id and IsAmbience.
    AudioTimeScale(i32, bool),
    /// ResetTransformBehaviour.Start with ResetOnStart: its id and ToReset.
    ResetTransform(i32, i64),
    /// NoChildrenCollide.Start: its component id.
    NoChildrenCollide(i32),
    /// InstantiateObjectBehaviour.Start with InstantiateOnStart: its component id.
    InstantiateObject(i32),
    /// ExplosionSoundBehviour.Start with StartWhenAwake: Play(Delay). The Near count when it has an AudioSource
    /// (else Play returns at once), DistantSound, Delay.
    ExplosionSoundPlay(i32, Option<usize>, i64, f32),
    /// RandomForceAtStart.Start: the Rigidbody2D's component id, PositionalVelocityStrength, RotationalVelocityStrength.
    RandomForce(i32, f32, f32),
    /// DoAtTimeOnce.Start with StartSequenceOnStart.
    DoAtTimeOnce(usize),
    /// UnderwaterExplosionBehaviour.Start: the clone's root Transform, Splash, its sound's Near count and DistantSound.
    UnderwaterExplosion(i32, crate::assets::db::ObjRef, usize, i64, i32),
    /// ExistByChance.Start: the component's instance id and its Chance.
    ExistByChance(i32, f32),
    /// ExistifUnderwater.Start of (component, Offset, Space is Self, Invert).
    ExistifUnderwater(i32, [f32; 2], bool, bool),
    /// OnlyExplosionBehaviour.Start with ExplodeOnStart: the component's instance id and its ExplosionParameters.
    OnlyExplosion(i32, super::explosion::ExplosionParameters),
    Explosive(usize),
    /// DistanceJointWireBehaviour.Start of a BloodWireBehaviour (blood_wire.rs).
    BloodWire(usize),
    RootWire(usize),
    /// LinkDeviceBehaviour.Start of a PhaseLinkBehaviour (link_device.rs).
    PhaseLink(usize),
    /// SerpentineBeltBehaviour.Start (serpentine_belt.rs).
    Belt(usize),
    Pin(usize),
    /// NoCollide.Start: the component's instance id.
    NoCollide(i32),
    /// DisintegrationCounterBehaviour.Start: the component's instance id.
    DisintegrationCounter(i32),
    /// CarBehaviour.Start: the `Game::cars` index.
    Car(usize),
    /// TyreDeflationBehaviour.Start: the `Game::tyres` index.
    Tyre(usize),
    Firearm(usize),
    /// MachineGunBehaviour.Start: the `Game::machine_guns` index.
    MachineGun(usize),
    /// A translated class's Start.
    Translated(super::translated::TBehaviour),
    /// FlaskBehaviour.Start: the `Game::flasks` index.
    Flask(usize),
    /// LiquidContainerController.Start: the `Game::liquid_containers` index.
    LiquidContainer(usize),
    /// DetectorBehaviour.Start: the `Game::detectors` index.
    Detector(usize),
    Button(usize),
    /// A Start that only adds context menu buttons.
    Menu(Menu),
    Television(usize),
    /// MuteBehaviour.Start: its `mutes` entry and physical (the menu button, then SetMute(Muted)).
    Mute(usize, usize),
    SerialisationHelper(usize),
    /// LaserBehaviour.Start: the `Game::lasers` index.
    Laser(usize),
    /// SingleFloodlightBehaviour.Start: the `Game::floodlights` index.
    Floodlight(usize),
    /// GeneratorBehaviour.Start: the `Game::generators` index.
    Generator(usize),
    /// SpikeGrenadeBehaviour.Start: the `Game::spike_grenades` index.
    SpikeGrenade(usize),
    /// BulbBehaviour.Start: the `Game::bulbs` index.
    Bulb(usize),
    /// CathodeLightBehaviour.Start: the `Game::cathode_lights` index.
    CathodeLight(usize),
    /// ChainsawBehaviour.Start: the `Game::chainsaws` index.
    Chainsaw(usize),
    /// FreezeBehaviour.Start: the `Game::freezes` index.
    Freeze(usize),
    /// PoisonSpreadBehaviour.Start (poison.rs).
    Poison(usize),
    /// FreezeSerialisationHelper.Start: the `Game::freeze_serialisations` index.
    FreezeSerialisation(usize),
    /// LayerSerialisationBehaviour.Start: the `Game::layer_serialisations` key.
    LayerSerialisation(usize),
    /// SerialisedLayerBehaviour.Start: the `LayerEditor::serialised` key.
    SerialisedLayer(usize),
    /// CeilingTurretBehaviour.Start (the sentry's settings): the `Game::ceiling_turrets` index.
    CeilingTurret(usize),
    /// LaserTurretBehaviour.Start: the `Game::laser_turrets` index.
    LaserTurret(usize),
    /// SentryTurretBehaviour.Start: the `Game::sentry_turrets` index.
    SentryTurret(usize),
    /// DecimatorBehaviour.Start: the `Game::decimators` index.
    Decimator(usize),
    /// ObstructionEmitterBehaviour.Start: the `Game::obstruction_emitters` index.
    ObstructionEmitter(usize),
    /// HeartMonitorBehaviour.Start: the `Game::heart_monitors` index.
    HeartMonitor(usize),
    /// RayCannonBehaviour.Start: the `Game::ray_cannons` index.
    RayCannon(usize),
    /// BeamformerBehaviour.Start: the `Game::beamformers` index.
    Beamformer(usize),
    /// CustomFixedConstrainedBehaviour.Start: the `Game::custom_fixeds` index.
    CustomFixedConstrained(usize),
    /// DroneBehaviour.Start: the `Game::drones` index.
    Drone(usize),
    /// PortalBehaviour.Start: the `Game::portals` index.
    Portal(usize),
    /// ImmobilityFieldBehaviour.Start: the `Game::immobility_fields` index.
    ImmobilityField(usize),
    /// JavelinBehaviour.Start: the `Game::javelins` index.
    Javelin(usize),
    /// JavelinMissileBehaviour.Start: the `Game::javelin_missiles` index.
    JavelinMissile(usize),
    /// TeslaCoilBehaviour.Start: the `Game::tesla_coils` index.
    TeslaCoil(usize),
    /// WormStaffBehaviour.Start: the `Game::worm_staffs` index.
    WormStaff(usize),
    /// NewAtomBombExplosionBehaviour.Start: the `Game::atom_explosions` index.
    AtomExplosion(usize),
    /// EnergySwordBehaviour.Start: the `Game::energy_swords` index.
    EnergySword(usize),
    /// ThrusterbedBehaviour.Start: the `Game::thrusterbeds` index.
    Thrusterbed(usize),
    /// PistonBehaviour.Start: the `Game::pistons` index and the `autofix_joints` entry holding its SliderJoint2D.
    Piston(usize, usize),
    /// MapLiftBehaviour's pending start: the `Game::map_lifts` index and the `autofix_joints` entry holding its SliderJoint2D.
    MapLift(usize, usize),
    /// WinchBehaviour.Start: the winch and its SliderJoint2D's autofix entry.
    Winch(usize, usize),
    Conveyor(usize),
    /// ReactorCoreBehaviour.Start
    ReactorCore(usize),
    /// ReactorConduitSetBehaviour.Start
    ReactorConduitSet(usize),
    /// RandomSoundBehaviour.Start
    RandomSound(usize),
    /// PlayRandomClipBehaviour.Start
    PlayRandomClip(usize),
    /// AdhesiveCouplerBehaviour.Start
    Coupler(usize),
    /// SeismicChargeExplosionBehaviour.Start
    SeismicExplosion(usize),
    /// TransmitterBehaviour.Start
    Transmitter(usize),
    /// The Start of a script whose only job is its world text: the class and its index in the arena.
    TextStart(&'static str, usize),
    /// IonBoltBehaviour.Start
    IonBolt(usize),
    /// RandomCarTextureBehaviour.Start: the `Game::random_car_textures` index.
    CarTexture(usize),
    LiquidPump(usize),
    /// ExistInDetailView.Start / IgnoreParentSize.Start: the component's instance id.
    ExistInDetailView(i32),
    /// ExistInBeta.Start: the component's instance id.
    ExistInBeta(i32),
    /// ExistInThermalVision.Start: the component's instance id.
    ExistInThermalVision(i32),
    /// ThermalVisionTempDisplayBehaviour.Start
    ThermalDisplay,
    /// FirearmAttachmentPointBehaviour.Start without an attachment: CreateDetailViewBlob's new GameObject(name,
    /// ExistInDetailView, Optout) (GameObject, Transform, both scripts) and its SpriteRenderer. The blob has no physics,
    /// and its ExistInDetailView.Start only deactivates it.
    /// The point's component id.
    FirearmAttachmentPoint(i32),
    IgnoreParentSize(i32),
    RandomSprite(usize),
    /// AirfoilBehaviour.Start: SetWingType(GetWingType(CurrentWingTypeName)).
    Airfoil(usize),
    /// ShockwaveBehaviour.Start: startSize = localScale.x (old_weapons.rs).
    Shockwave(usize),
    /// StaticNeedleBehaviour.Start: the `Game::static_needles` index.
    StaticNeedle(usize),
    /// MetronomeBehaviour.Start: seed = Random.value * 1000 and UpdateActivation (`Game::metronomes`).
    Metronome(usize),
    /// BlasterBehaviour.Start: the muzzle flash of a mirrored blaster (`Game::blasters`).
    Blaster(usize),
    /// SmoothLineControllerBehaviour.Start
    SmoothLine(usize),
    DeterminedAnomaly(usize),
    /// StartAudioSourceAtRandomTimeBehaviour.Start.
    StartAudioSourceAtRandomTime(i32),
    /// ArrayCloner.Start: the component's id, Amount and Delta.
    ArrayCloner(i32, i32, [f32; 2]),
    /// MaintainScaleSignBehaviour.Start: EnsureScaling, by the component's instance id.
    MaintainScale(i32),
    Foliage(usize),
    LiquidSpread(usize),
    /// ActivationToggleBehaviour.Start (UpdateActivation): `Game::activation_toggles`.
    ActivationToggle(usize),
    /// GaugeBehaviour.Start for a PowerGauge: `Game::power_gauges`.
    PowerGauge(usize),
    /// FlareBehaviour.Start: `Game::flares`.
    Flare(usize),
    /// SyringeBehaviour.Start (any subclass): the `Game::syringes` index.
    Syringe(usize),
    /// SyringeExplosionBehaviour.Start: the `Game::syringe_explosions` index.
    SyringeExplosion(usize),
    /// BoundingBoxBehaviour with its serialized box.
    BoundingBox(super::Bounds),
    MapConfig(super::map::MapConfigStart),
    /// ExplosionSoundBehviour on a BloodExplosion (three Near clips, StartWhenAwake, no distant sound).
    BloodExplosionSound,
    /// DecalDripStreakBehaviour.Start of (physical, controller, decal, streak): speed and length draws (the streak's growth is presentation).
    DecalDripStreak(usize, usize, usize, usize),
    /// A new BloodPuff's ExplosionSoundBehviour (eight Near clips).
    BloodPuffSound,
    /// A new pooled clone's ExplosionSoundBehviour.Start: Play draws one of its `Near` clips (the count).
    ExplosionSound(usize),
    /// ExistifUnderwater.Start of a new BloodPuff's Extra decal emitter (Invert: off under water).
    BloodPuffEmitterWater(usize),
    /// ExistifUnderwater.Start of a new BloodPuff's particle child (name, Invert): SetActive(Invert ^ under water).
    BloodPuffChildWater(usize, &'static str, bool),
    /// SerialiseInstructions.Start: the number of SerialisableIdentity components it adds (their Regenerate
    /// takes a Guid, not Random) and the root's Transform id.
    SerialiseInstructions(usize, i32),
}

impl Game {
    /// ConnectedNodeBehaviour.DisconnectFromEverything: each neighbour forgets its first link back (SetToNull).
    pub fn node_disconnect_from_everything(&mut self, n: usize) {
        for i in 0..self.nodes[n].Connections.len() {
            if let Some(other) = self.nodes[n].Connections[i] {
                if let Some(at) = self.nodes[other].Connections.iter().position(|&c| c == Some(n)) {
                    self.nodes[other].Connections[at] = None;
                }
            }
        }
        self.nodes[n].Connections.fill(None);
    }

    /// ConnectedNodeBehaviour.IsConnectedTo
    pub fn node_is_connected_to(&self, a: usize, b: usize) -> bool {
        self.nodes[a].Connections.contains(&Some(b))
    }

    /// ConnectedNodeBehaviour.OnBeforeSerialise: ShouldLoadSerialisableConnections = true; SerialisableConnectedNodes[i] = Connections[i] != null.
    pub fn node_on_before_serialise(&mut self, n: usize) -> Vec<bool> {
        self.nodes[n].ShouldLoadSerialisableConnections = true;
        self.nodes[n].Connections.iter().map(|c| c.is_some()).collect()
    }

    /// ConnectedNodeBehaviour.Start (and ConnectedNodeBehaviour.Awake). `serialisable_connected_nodes` is the prefab's SerialisableConnectedNodes (None when
    /// null). Awake only adds the node to NodeController.Nodes, which `node_controller_update` already walks.
    pub fn node_start(&mut self, n: usize, serialisable_connected_nodes: Option<&[bool]>) {
        if !self.nodes[n].ShouldLoadSerialisableConnections {
            return;
        }
        let Some(keep) = serialisable_connected_nodes else { return };
        for i in 0..self.nodes[n].Connections.len() {
            // SerialisableConnectedNodes[i] past the array's end throws in Start (logged; the rest of the loop is skipped).
            if i >= keep.len() {
                crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
            }
            if !keep[i] {
                self.nodes[n].Connections[i] = None;
            }
        }
    }

    /// SkinMaterialHandler.Awake. `renderer.material` (after assigning Person.ChosenMaterial when it is set) makes the
    /// renderer's own Material instance, which takes one instance id. Start only reads the blood colour and uploads the
    /// material (rendering).
    pub fn skin_awake(&mut self, l: usize) {
        // SkinMaterialHandler.Start: drawn in play.rs `limb_sprites` (BloodColour starts at original_blood_color) and limb_material.rs `sync` (Sync uploads the material arrays)
        self.allocate_instance_id();
        let time = self.time.time;
        self.skins[l].damagePointTimeStamps = vec![time; 128];
        self.skins[l].damagePoints = vec![[0.0, 0.0, 0.0, damage_type::NONE as f32]; 128];
    }

    // NodeController.Awake: Nodes.Clear(); a Game starts with the empty `nodes` and the scene's nodes join it in ConnectedNodeBehaviour.Awake
    /// NodeController.Update over NodeController.Nodes (HashSet, insertion order): ConnectedNodeBehaviour.RootPropagation on each node (the IsRoot check, then propagate_value).
    pub fn node_controller_update(&mut self) {
        for n in 0..self.nodes.len() {
            self.connected_node_reset_value(n);
        }
        for i in 0..self.nodes.len() {
            if self.nodes[i].IsRoot {
                self.propagate_value(i);
            }
        }
    }

    /// ConnectedNodeBehaviour.ResetValue
    pub fn connected_node_reset_value(&mut self, n: usize) {
        let n = &mut self.nodes[n];
        n.IsConnectedToRoot = false;
        n.Value = if n.IsRoot { 1_000_000 } else { 0 };
    }

    fn propagate_value(&mut self, me: usize) {
        let n = &mut self.nodes[me];
        n.IsConnectedToRoot = n.IsRoot || n.Value > 0;
        for i in 0..self.nodes[me].Connections.len() {
            if let Some(c) = self.nodes[me].Connections[i] {
                let v = self.nodes[me].Value;
                if self.nodes[c].Value < v {
                    self.nodes[c].Value = v - 1;
                    self.propagate_value(c);
                }
            }
        }
    }

    /// GForceMeasureBehaviour.FixedUpdate
    pub fn gforce_fixed_update(&mut self, l: usize) {
        let v = self.physicals[self.limbs[l].PhysicalBehaviour].rigidbody.velocity(&self.world);
        let g = &mut self.gforces[l];
        g.velocityLastFrame = g.velocity;
        g.velocity = v;
        g.acceleration = [g.velocity[0] - g.velocityLastFrame[0], g.velocity[1] - g.velocityLastFrame[1]];
        g.sustainedAcceleration = mathf::v2_lerp(g.sustainedAcceleration, g.acceleration, 0.5);
    }

    /// GForceMeasureBehaviour.OnDisable: acceleration, sustainedAcceleration, velocity and velocityLastFrame = Vector2.zero.
    pub fn gforce_on_disable(&mut self, l: usize) {
        let g = &mut self.gforces[l];
        (g.acceleration, g.sustainedAcceleration, g.velocity, g.velocityLastFrame) = ([0.0; 2], [0.0; 2], [0.0; 2], [0.0; 2]);
    }

    /// SkinMaterialHandler.Update
    pub fn skin_update(&mut self, l: usize) {
        let dt = self.time.deltaTime;
        // The loop condition re-runs CalculateCycleCount (adding dt again) every iteration.
        let mut i = 0;
        while i < self.skins[l].intervalDistributor.calculate_cycle_count(dt) {
            i += 1;
            let interval = self.skins[l].intervalDistributor.IntervalSeconds;
            if !self.limbs[l].IsZombie && self.prefs.AutoHealWounds && self.limb_is_considered_alive(l) {
                // RegenerateWounds; Sync only uploads to the material.
                self.regenerate_wounds(l, interval);
            }
            let phys = self.limbs[l].PhysicalBehaviour;
            let mut j = 0;
            while j < self.physicals[phys].victimPenetrations.len() {
                let pen = self.penetration_arena[self.physicals[phys].victimPenetrations[j]].clone();
                j += 1;
                let stabber = pen.Stabber;
                if !pen.Active || self.physicals[stabber].destroyed || !self.physicals[stabber].StabCausesWound || self.physicals[stabber].isDisintegrated {
                    continue;
                }
                let sharp_end = self.get_sharp_end(stabber, &pen.Axis);
                // Physics2D.OverlapPointNonAlloc: DefaultRaycastLayers, unbounded depth; a hit on the victim's object.
                let victim = pen.Victim.expect("victim penetration without a victim");
                let inside = self.overlap_point_buffer([sharp_end[0], sharp_end[1], 0.0], 0xffff_fffb, f32::NEG_INFINITY, f32::INFINITY, SKIN_COLLIDER_BUFFER).contains(&victim);
                if !inside {
                    let (c, e) = self.sprite_world_bounds(phys);
                    let (closest, _) = crate::unity::transform::bounds_closest_point([sharp_end[0], sharp_end[1], 0.0], c, e);
                    let size = (d(8.0) * d(self.physicals[stabber].StabWoundSizeMultiplier)).f();
                    self.skin_add_damage_point(l, damage_type::STAB, [closest[0], closest[1]], size);
                }
            }
        }
    }

    /// SkinMaterialHandler.RegenerateWounds
    pub(crate) fn regenerate_wounds(&mut self, l: usize, dt: f32) -> bool {
        use damage_type::*;
        let num = mathf::pow(0.95, dt);
        let num2 = mathf::pow(0.995, dt);
        let num3 = mathf::pow(0.97, dt);
        let mut result = false;
        let flag = self.skins[l].GunshotDamageRegen
            && self.circulations[l].BleedingRate < 1.0
            && self.limbs[l].Health > self.limbs[l].InitialHealth / 2.0;
        let s = &mut self.skins[l];
        for i in 0..128 {
            let p = s.damagePoints[i];
            let ty = mathf::floor(p[3]) as i32;
            let k = match ty {
                BLUNT if s.BluntDamageRegen && p[2] > 0.001 => num,
                BULLET | STAB if flag && p[2] > 0.001 => num2,
                BURN if s.BurnWoundRegen && p[2] > 0.001 => num3,
                _ => continue,
            };
            s.damagePoints[i][2] *= k;
            result = true;
        }
        result
    }

    /// SkinMaterialHandler.LateUpdate
    pub fn skin_late_update(&mut self, l: usize) {
        let limb = &self.limbs[l];
        if limb.SpeciesIdentity != "Android"
            && self.skins[l].ShouldRot
            && !(self.physicals[limb.PhysicalBehaviour].Temperature < -50.0)
        {
            let dt = self.time.deltaTime;
            let mut num = 0.0f32;
            if limb.Health < 0.05 {
                num = (d(num) + d(dt) * 0.002f32).f();
            }
            if limb.IsZombie {
                num = (d(num) + d(dt) * 0.02f32).f();
            }
            let v = self.skins[l].rottenProgress + num;
            self.skins[l].set_rotten(v);
        }
    }

    /// BleedingParticleBehaviour.Update
    /// Update's WaterPuffSystem calls for particles.rs (`puff_update`, which reads isPlaying): under water (`rate`) the
    /// emission write and Play, else Stop; `color` its startColor write.
    fn bleed_puff_update(&mut self, b: usize, rate: Option<f32>, color: Option<[f32; 4]>) {
        let (now, at) = (self.time.timeAsDouble, self.effect_stamp());
        self.bleed_puff_updates.push((b, rate, color, now, at));
    }

    pub fn bleeding_update(&mut self, b: usize) {
        // A child of its limb: no Update once the limb is deactivated (disintegrated).
        let parent = self.bleeding[b].Parent;
        if self.bleeding[b].destroyed || self.physicals[parent].destroyed || !self.physicals[parent].active {
            return;
        }
        let bp = &self.bleeding[b];
        // CirculationBehaviour.BleedingRate on null: the exception ends the Update before anything changes.
        let Some(l) = bp.CirculationBehaviour else { return };
        let c = &self.circulations[l];
        let num = if bp.IsOnlyForInternalBleeding { c.InternalBleedingIntensity } else { c.BleedingRate };
        let scaled = mathf::map_range(0.0, 1.0, 0.0, 1.0, c.base.TotalLiquidAmount);
        let pushing_ok = bp.PushingTo.map_or(true, |p| self.circulations[p].IsDisconnected);
        if pushing_ok && num > 0.05 && scaled > 0.05 && self.physicals[self.limbs[l].PhysicalBehaviour].Temperature > 0.0 {
            let fallback = self.original_blood_color(l);
            let mut color = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
            if self.prefs.GorelessMode {
                color = super::circulation::change_red_to_orange(color);
            }
            // transform.position: the bleeding particle's world position, which its splat tracks as initialParent.
            let s = self.bleeding[b].BloodSplatController.expect("BleedingParticle.prefab has its FancyBloodSplatController");
            self.update_splat_pose(s);
            let at = self.splats[s].parent_position;
            let flag = self.is_point_under_water([at[0], at[1]]);
            let dt = self.time.deltaTime;
            let c = &self.circulations[l];
            let (flow, mult, total) = (c.BloodFlow, c.BloodLossRateMultiplier, c.base.TotalLiquidAmount);
            let bp = &mut self.bleeding[b];
            bp.fakeHeartClock = (d(bp.fakeHeartClock) + d(dt) * flow * bp.PulseSpeed).f();
            let num2 = (d(bp.SpeedMultiplier) * (d(flow) * flow * flow * 2.0f32) * mult * mathf::lerp(0.7, 1.0, bp.Laminarity)).f();
            let num3 = bp.PulseIntensity * flow;
            bp.startSpeedMultiplier = (d(num2) * (d(1.0) + d(num3) * (d(1.0) - bp.fakeHeartClock % 1.0))).f();
            let num4 = if total > mathf::EPSILON { mathf::min(mult * num, 1.0) } else { 0.0 };
            bp.rateOverTimeMultiplier = if flag { 0.0 } else { (d(mathf::lerp(18.0, 32.0, bp.Laminarity)) * num4 * bp.RateMultiplier).f() };
            bp.emission_enabled = true;
            bp.color = color;
            if let Some(s) = bp.BloodSplatController {
                let speed = bp.startSpeedMultiplier;
                let splat = &mut self.splats[s];
                splat.DecalColourMultiplier = color;
                splat.EjectionForce = [0.25 * speed, 0.35 * speed];
                splat.SpawningChance = if flag { 0.0 } else { total * 0.5 };
            }
            let bp = &mut self.bleeding[b];
            let mut flag2 = bp.DripSounds.is_some() && bp.visible && (d(num4) * bp.RateMultiplier).0 >= bp.DripSoundRateThreshold as f64;
            // WaterPuffSystem: startColor = color; under water rateOverTimeMultiplier = num4 and Play() unless isPlaying
            // (no drip sounds), else Stop() if isPlaying.
            if bp.ShouldBecomeSmokeInWater && bp.WaterPuffSystem {
                if flag {
                    flag2 = false;
                }
                self.bleed_puff_update(b, flag.then_some(num4), Some(color));
            }
            let bp = &mut self.bleeding[b];
            if !flag2 {
                return;
            }
            // The loop condition re-runs CalculateCycleCount (adding dt again) every iteration.
            let mut i = 0;
            while i < self.bleeding[b].dripSoundClock.calculate_cycle_count(dt) {
                if self.random.value() > 0.2 {
                    // PlayClipOnce(DripSounds.PickRandom(), volume)
                    let n = self.bleeding[b].DripSounds.unwrap();
                    self.pick_random(n);
                }
                i += 1;
            }
        } else {
            // if (ShouldBecomeSmokeInWater && (bool)WaterPuffSystem && WaterPuffSystem.isPlaying) WaterPuffSystem.Stop();
            if self.bleeding[b].ShouldBecomeSmokeInWater && self.bleeding[b].WaterPuffSystem {
                self.bleed_puff_update(b, None, None);
            }
            let bp = &mut self.bleeding[b];
            bp.startSpeedMultiplier = 0.0;
            bp.rateOverTimeMultiplier = 0.0;
            bp.emission_enabled = false;
            if let Some(s) = bp.BloodSplatController {
                self.splats[s].SpawningChance = 0.0;
            }
        }
    }

    /// The delayed Start of every object instantiated since the last pass. Unity's player loop runs it
    /// at FixedUpdate.ScriptRunDelayedFixedFrameRate (before each step's FixedUpdates) and
    /// Update.ScriptRunDelayedDynamicFrameRate (before Update), whichever comes first after the Instantiate.
    /// After `inst`'s SyncTransforms recreated its joints: the snapshots of its sliders and FixedJoint2Ds that are waiting
    /// for HingeJointLimitAutofixBehaviour.Start follow.
    pub fn refresh_autofix_joints(&mut self, inst: &super::spawn::Instance) {
        let mut all = std::mem::take(&mut self.autofix_joints);
        for joints in &mut all {
            for a in joints.sliders.iter_mut().chain(joints.fixeds.iter_mut()) {
                let key = (a.object, a.component);
                if inst.bodies.get(&a.object) != Some(&self.owner_body(a.attached)) {
                    continue;
                }
                if let Some(st) = inst.anchored.get(&key) {
                    (a.state, a.slider, a.slot) = (st.base.clone(), st.slider.clone(), st.slot);
                    a.joint = inst.joints.get(&key).copied();
                }
            }
        }
        self.autofix_joints = all;
    }

    /// HingeJointLimitAutofixBehaviour.OnFlip (not on a person): IsFlipped follows the sign of localScale.x (nothing reads it),
    /// then FixHinges, FixSlider and FixFixed over the joints below the host. A host without a limited hinge or slider or a
    /// FixedJoint2D has no entry: neither Start nor OnFlip touches anything.
    pub fn hinge_joint_limit_autofix_item_on_flip(&mut self, id: i32) {
        let Some(joints) = self.autofix_started.get(&id).cloned() else { return };
        let live: Vec<usize> = joints.hinges.iter().copied().filter(|&h| !self.hinges[h].destroyed).collect();
        self.fix_hinges(&live);
        self.fix_sliders(&joints.sliders);
        self.fix_fixed(&joints.fixeds);
    }

    /// FixHinges' loop over the given hinges: each limited hinge's limits mirrored.
    pub fn fix_hinges(&mut self, hinges: &[usize]) {
        for &h in hinges {
            if self.hinges[h].useLimits {
                let [min, max] = self.hinges[h].limits;
                // `0f - x` compiles to a negation: a 0 limit becomes -0 (the game's recorded limits).
                self.hinges[h].set_limits(&mut self.world, -max, -min);
            }
        }
    }

    /// The joint's break sent to every JointBreakSoundBehaviour (OnJointBreak2D reaches each on the joint's GameObject).
    pub fn joint_break_sound_on_break(&mut self, j: BreakJoint) {
        for s in 0..self.joint_break_sounds.len() {
            self.joint_break_sound_on_joint_break(s, j);
        }
    }

    /// JointBreakSoundBehaviour.OnJointBreak2D: AudioSource.PlayOneShot, or without one PlayClipOnce, of Clips.PickRandom(),
    /// then OnBreak (audio and particles only, checked at instantiation).
    pub fn joint_break_sound_on_joint_break(&mut self, s: usize, j: BreakJoint) {
        let sound = &self.joint_break_sounds[s];
        if !sound.enabled || !sound.joints.contains(&j) {
            return;
        }
        let (id, clips, physical, explosives, cars, calls) = (sound.id, sound.clips, sound.physical, sound.explosives.clone(), sound.cars.clone(), sound.calls.clone());
        if clips != 0 {
            let clip = self.pick_clip(id, "Clips", clips);
            match self.audio.source_field(id, "AudioSource") {
                Some(s) => self.audio.play_one_shot(s, clip.as_ref(), 1.0),
                None => {
                    let p = physical.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                    self.play_clip_once(p, clip, 1.0);
                }
            }
        }
        for e in explosives {
            self.explosive_force_immediate(e);
        }
        for (id, call) in cars {
            self.car_invoke(id, call);
        }
        for call in calls {
            self.event_call(call);
        }
    }

    /// The Fixed/Slider arena entry for a prefab joint: from here on its break limit is `BreakJoint::Fixed`, in the same
    /// place of the list, and the component setters apply.
    pub(super) fn adopt_prefab_joint(&mut self, a: &AutofixJoint) -> usize {
        // The component is one: a second script adopting it gets the first's entry.
        if let Some(&f) = self.adopted_joints.get(&a.id) {
            return f;
        }
        // The clone (still to sync, or kept after its SyncTransforms) no longer owns the joint. Its b2 joint is the
        // component's current one: the clone's SyncTransforms may have re-created it since the script linked it, or
        // it broke (none left).
        let mut joint = a.joint;
        if let Some(inst) = self.unsynced.iter_mut().chain(self.retained.iter_mut()).find(|i| i.objects.get(a.object).and_then(|o| o.components.get(a.component)) == Some(&a.id)) {
            // A broken joint's entry stays in the clone with no b2 joint behind it.
            joint = inst.joints.remove(&(a.object, a.component)).filter(|&j| self.world.joints[j].is_some());
            inst.anchored.remove(&(a.object, a.component));
        }
        let f = self.fixed_joints.len();
        self.fixed_joints.push(super::simulate::FixedJoint { hinge: None,
            state: a.state.clone(),
            slider: a.slider.clone(),
            spring: None,
            friction: None,
            distance: None,
            relative: None,
            joint,
            attached: a.attached,
            connected: a.connected,
            // A joint on an object deactivated first (a shard) is inactive with it.
            active: !matches!(a.attached, super::BodyOwner::Physical(p) if !self.physicals[p].active),
            enabled: a.enabled,
            // A joint whose object was destroyed first: `(bool)joint` reads false.
            destroyed: self.destroyed_components.contains(&a.id),
        });
        self.adopted_joints.insert(a.id, f);
        if let Some(slot) = a.slot {
            self.adopted_slots.insert(slot, f);
            self.anchored_joints[slot].joint = None;
            for k in self.joint_break_limits.iter_mut().chain(self.joint_break_sounds.iter_mut().flat_map(|s| &mut s.joints)).chain(self.joint_helpers.iter_mut().flat_map(|h| h.joints.iter_mut().flatten())).chain(self.reactor_conduits.iter_mut().flat_map(|c| c.tube.iter_mut().map(|t| &mut t.1).chain(c.reactor_joint.iter_mut()))).filter(|k| **k == BreakJoint::Anchored(slot)) {
                *k = BreakJoint::Fixed(f);
            }
        }
        f
    }

    /// HingeJointLimitAutofixBehaviour.FixSlider: each limited slider's translation limits mirrored.
    pub fn fix_sliders(&mut self, sliders: &[AutofixJoint]) {
        let adopted: Vec<usize> = sliders.iter().map(|a| self.adopt_prefab_joint(a)).collect();
        for f in adopted {
            if self.fixed_joints[f].destroyed {
                continue;
            }
            let s = self.fixed_joints[f].slider.as_ref().unwrap();
            if s.use_limits {
                let (min, max) = (s.lower, s.upper);
                // `0f - x` compiles to a negation, as FixHinges' (a 0 limit becomes -0).
                self.slider_joint_set_limits(f, -max, -min);
            }
        }
    }

    /// HingeJointLimitAutofixBehaviour.FixFixed: each FixedJoint2D's connectedBody set to null and back, which makes the
    /// native joint twice.
    pub fn fix_fixed(&mut self, fixeds: &[AutofixJoint]) {
        // Adopted before the first setter: its AutoSyncTransforms recreates every joint of the instance.
        let adopted: Vec<usize> = fixeds.iter().map(|a| self.adopt_prefab_joint(a)).collect();
        for f in adopted {
            if self.fixed_joints[f].destroyed {
                continue;
            }
            let connected = self.fixed_joints[f].connected;
            self.fixed_joint_set_connected_body(f, None);
            self.fixed_joint_set_connected_body(f, connected);
        }
    }

    pub fn run_pending_starts(&mut self) {
        for start in std::mem::take(&mut self.pending_starts) {
            // Unity runs Start per component: a managed exception ends only that Start.
            let caught = super::menu::catch_managed(|| match start {
                PendingStart::Throw(what) => super::menu::throw(what),
                // A limb's own PhysicalBehaviour.Start (first on the object) can have disintegrated it (SetActive(false)): the rest of
                // the object's Starts wait for its activation (keyed by the PhysicalBehaviour, a script set_active_scripts walks).
                PendingStart::Limb(l) | PendingStart::Circulation(l) | PendingStart::Node(l, _) | PendingStart::GoreString(l, _)
                    if !self.physicals[self.limbs[l].PhysicalBehaviour].active =>
                {
                    self.deferred_starts.push((self.physicals[self.limbs[l].PhysicalBehaviour].id, start))
                }
                PendingStart::Glow(g) => self.glow_start(g),
                PendingStart::Visual(v) => self.visual_start(v),
                PendingStart::Glowtube(g) => self.glowtube_start(g),
                PendingStart::Centrifuge(c) => self.centrifuge_start(c),
                PendingStart::Valve(v) => self.valve_start(v),
                PendingStart::ActivatorElectrode(e) => self.activator_electrode_start(e),
                PendingStart::BloodTank(t) => self.blood_tank_start(t),
                // BleedingParticleBehaviour.Start: if (!ShouldBecomeSmokeInWater && (bool)WaterPuffSystem) Object.Destroy(WaterPuffSystem),
                // queued like a DestroyObjectDelayed (an inactive puff object's system was never listed).
                PendingStart::Bleeding(b) => {
                    let p = &self.bleeding[b];
                    if !p.ShouldBecomeSmokeInWater && p.WaterPuffSystem && p.water_puff && p.puff_destroy_at.is_none() {
                        self.bleeding[b].puff_destroy_at = Some(self.time.fixedTimeAsDouble.min(self.time.timeAsDouble));
                    }
                }
                PendingStart::Splat(s) => {
                    if !self.splats[s].started && !self.splats[s].destroyed {
                        self.splat_start(s);
                    }
                }
                // Play(Delay): audioSource.clip = Near.PickRandom(); DistantSound is None.
                PendingStart::BloodExplosionSound => {
                    self.pick_random(3);
                }
                PendingStart::DecalDripStreak(p, c, d, s) => {
                    // DecalDripStreakBehaviour.Start: speed = Range(0.5, 1) * StreakDripSpeed; length = Range(Min.., Max..).
                    let speed = (super::d(self.random.range(0.5, 1.0)) * STREAK_DRIP_SPEED).f();
                    let length = self.random.range(MIN_LOCAL_STREAK_LENGTH, MAX_LOCAL_STREAK_LENGTH);
                    let found = self.physicals[p].decal_controllers.get_mut(c).and_then(|x| x.decals.get_mut(d)).and_then(|x| x.streaks.get_mut(s));
                    if let Some(streak) = found {
                        streak.speed = speed;
                        streak.length = length;
                        self.active_streaks.push([p, c, d, s]);
                    }
                }
                PendingStart::BloodPuffSound => {
                    self.pick_random(8);
                }
                PendingStart::ExplosionSound(n) => {
                    self.pick_random(n);
                }
                PendingStart::Person(p) => self.person_start(p),
                PendingStart::HingeJointLimitAutofix(p) => self.hinge_joint_limit_autofix_start(p),
                // Start: IsFlipped = false, and only a mirrored host (localScale.x not above zero) fixes anything.
                PendingStart::HingeJointLimitAutofixItem(host, joints) => {
                    let joints = std::mem::take(&mut self.autofix_joints[joints]);
                    let x = match host {
                        Ok(p) => self.physicals[p].transform.localScale[0],
                        Err(n) => self.transform_nodes[n].trs.s[0],
                    };
                    if !(x > 0.0) {
                        self.fix_hinges(&joints.hinges);
                        self.fix_sliders(&joints.sliders);
                        self.fix_fixed(&joints.fixeds);
                    }
                    self.autofix_started.insert(joints.id, joints);
                }
                PendingStart::Circulation(l) => self.circulation_start(l),
                // Water.Start can have deactivated the object (its Ice) before this Start was due.
                PendingStart::Physical(p) => {
                    if self.physicals[p].active {
                        self.physical_start(p)
                    }
                }
                PendingStart::Water(w) => self.water_start(w),
                PendingStart::Machinery(m) => self.machinery_start(m),
                PendingStart::Destroyable(x) => self.destroyable_start(x),
                PendingStart::UniversalBell(b) => self.universal_bell_start(b),
                PendingStart::TimedGate(g) => self.timed_gate_start(g),
                // A disabled Behaviour gets no Start until it is enabled (a save's Enabled false: InjectIntoMonoBehaviour ran first).
                PendingStart::Lagbox(g) if !self.lagboxes[g].enabled => self.deferred_starts.push((self.lagboxes[g].id, start)),
                PendingStart::Lagbox(g) => self.lagbox_start(g),
                PendingStart::Fireworks(f) => self.fireworks_update_activation(f),
                PendingStart::CardiopulmonaryBypass(c) => self.cardiopulmonary_bypass_start(c),
                PendingStart::BloodWire(w) => self.blood_wire_start(w),
                PendingStart::RootWire(r) => self.root_wire_start(r),
                PendingStart::Rope(id) => self.rope_start(id),
                PendingStart::PhaseLink(l) => self.phase_link_start(l),
                PendingStart::Belt(l) => self.belt_start(l),
                PendingStart::Pin(p) => self.pin_start(p),
                PendingStart::ArchelixCaster(c) => self.archelix_caster_start(c),
                PendingStart::ArchelixBolt(b) => self.archelix_bolt_start(b),
                PendingStart::FireMap(f) => self.fire_map_start(f),
                // (An ExistInDetailView.Start before it on the object switched it off: its Start waits for the object.)
                PendingStart::DetachFromParent(id, _) if !self.with_component_instance(id, |_, inst, o| inst.objects[o].active) => self.deferred_starts.push((id, start)),
                PendingStart::DetachFromParent(id, destroy_with_parent) => self.detach_from_parent_on_start(id, destroy_with_parent),
                // IncreaseStatOnStart.Start
                PendingStart::IncreaseStat(stat, float) => match float {
                    true => super::stats::increment_float(&stat, 1.0),
                    false => super::stats::increment_integer(&stat, 1),
                },
                // PlayAtRandom.Start: `if (!(Random.value > Chance))` plays its ParticleSystems and AudioSources.
                PendingStart::PlayAtRandom(id, chance) => self.play_at_random_start(id, chance),
                PendingStart::AudioTimeScale(t, ambience) => self.audio_time_scale_below(t, ambience),
                // ResetTransformBehaviour.Start
                PendingStart::ResetTransform(id, to_reset) => self.reset_transform(id, to_reset),
                PendingStart::NoChildrenCollide(id) => self.no_children_collide_start(id),
                // InstantiateObjectBehaviour.Start
                PendingStart::InstantiateObject(id) => self.instantiate_object(id, true),
                PendingStart::ExplosionSoundPlay(id, near, distant, delay) => self.explosion_sound_play(id, near, distant, delay),
                // RandomForceAtStart.Start
                PendingStart::RandomForce(rb, positional, rotational) => {
                    let owner = self.rigidbody_owner(rb).expect("RandomForceAtStart: a Rigidbody2D outside the arenas");
                    let r = self.owner_rigidbody(owner).clone();
                    let v = self.random.inside_unit_circle();
                    r.add_force_impulse(&mut self.world, [v[0] * positional, v[1] * positional]);
                    let t = self.random.range(-1.0, 1.0) * rotational;
                    r.add_torque_impulse(&mut self.world, t);
                    // Object.Destroy(this): it has no other message.
                }
                // DoAtTimeOnce.Start
                PendingStart::DoAtTimeOnce(d) => self.do_at_time_once_execute_sequence(d),
                PendingStart::UnderwaterExplosion(root, splash, near, distant, audio) => self.underwater_explosion_start(root, splash, near, distant, audio),
                PendingStart::ExistByChance(id, chance) => self.exist_by_chance_start(id, chance),
                PendingStart::ExistifUnderwater(id, offset, self_space, invert) => self.existif_underwater_start(id, offset, self_space, invert),
                // OnlyExplosionBehaviour.Start
                PendingStart::OnlyExplosion(id, ex) => self.only_explosion_start(id, ex),
                PendingStart::Explosive(e) => self.explosive_start(e),
                PendingStart::NoCollide(id) => self.no_collide_start(id),
                PendingStart::DisintegrationCounter(id) => self.disintegration_counter_start(id),
                PendingStart::Car(c) => self.car_start(c),
                PendingStart::Tyre(t) => self.tyre_start(t),
                PendingStart::Firearm(f) => self.firearm_start(f),
                PendingStart::MachineGun(m) => self.machine_gun_start(m),
                PendingStart::Translated(b) => {
                    self.tr_start(b);
                    self.translated_menu(b);
                }
                PendingStart::Flask(f) => self.flask_start(f),
                PendingStart::LiquidContainer(c) => self.liquid_container_start(c),
                PendingStart::Detector(i) => self.detector_start(i),
                PendingStart::Button(b) => self.button_start(b),
                PendingStart::Menu(m) => self.menu_start(m),
                PendingStart::Television(t) => self.television_start(t),
                PendingStart::Mute(m, p) => {
                    use super::menu::{Label, MenuAction as A, MenuButton as B};
                    self.physicals[p].menu.push(B { label: Label::Mute(m), ..B::new("muteAudioSource", "", crate::il!(str "MuteBehaviour::Start" 1), A::Mute(m)) });
                    self.mute_set(m, self.mutes[m]);
                }
                PendingStart::SerialisationHelper(h) => self.serialisation_helper_start(h),
                PendingStart::Laser(l) => self.laser_start(l),
                PendingStart::Floodlight(i) => self.floodlight_start(i),
                PendingStart::Generator(i) => self.generator_start(i),
                PendingStart::SpikeGrenade(i) => self.spike_grenade_start(i),
                PendingStart::Bulb(i) => self.bulb_start(i),
                PendingStart::CathodeLight(i) => self.cathode_light_start(i),
                PendingStart::Chainsaw(i) => self.chainsaw_start(i),
                PendingStart::Freeze(i) => self.freeze_start(i),
                PendingStart::Poison(i) => self.poison_start(i),
                PendingStart::FreezeSerialisation(i) => self.freeze_serialisation_start(i),
                PendingStart::LayerSerialisation(p) => self.layer_serialisation_start(p),
                PendingStart::SerialisedLayer(p) => self.serialised_layer_start(p),
                PendingStart::CeilingTurret(i) => self.ceiling_turret_start(i),
                PendingStart::LaserTurret(i) => self.laser_turret_start(i),
                PendingStart::SentryTurret(i) => self.sentry_turret_start(i),
                PendingStart::Decimator(i) => self.unity_callback(|g| g.decimator_start(i)),
                PendingStart::ObstructionEmitter(i) => self.obstruction_emitter_start(i),
                PendingStart::HeartMonitor(i) => self.heart_monitor_start(i),
                PendingStart::RayCannon(i) => self.ray_cannon_start(i),
                PendingStart::Beamformer(i) => self.beamformer_start(i),
                PendingStart::Drone(i) => self.drone_start(i),
                PendingStart::CustomFixedConstrained(i) => self.custom_fixed_start(i),
                PendingStart::Portal(i) => self.portal_start(i),
                PendingStart::ImmobilityField(i) => self.immobility_field_start(i),
                // JavelinBehaviour.OnEnable (CastLaser, Laser.enabled = true) ran first, at its spawn.
                PendingStart::Javelin(i) => {
                    self.javelin_on_enable(i);
                    self.javelin_start(i);
                }
                PendingStart::JavelinMissile(i) => self.javelin_missile_start(i),
                PendingStart::TeslaCoil(i) => self.unity_callback(|g| g.tesla_coil_start(i)),
                PendingStart::WormStaff(i) => self.worm_staff_start(i),
                PendingStart::AtomExplosion(i) => self.atom_explosion_start(i),
                PendingStart::EnergySword(i) => self.energy_sword_start(i),
                PendingStart::Thrusterbed(i) => self.thrusterbed_start(i),
                PendingStart::Piston(i, joints) => {
                    let a = std::mem::take(&mut self.autofix_joints[joints]).sliders.remove(0);
                    // (A recording's loader adopted the recorded SliderJoint2Ds already: adopt_prefab_joint finds the entry.)
                    if a.slot.is_none() && !self.adopted_joints.contains_key(&a.id) {
                        unimplemented!("mod-only: PistonBehaviour.Start: a SliderJoint2D that was never created");
                    }
                    let f = self.adopt_prefab_joint(&a);
                    self.piston_start(i, f);
                }
                PendingStart::MapLift(i, joints) => {
                    let a = std::mem::take(&mut self.autofix_joints[joints]).sliders.remove(0);
                    // A recording's slider that broke before the snapshot: the Start that ran then took it, now a destroyed `joint`.
                    if !self.recorded_present.is_empty() && !self.recorded_present.contains(&a.id) {
                        self.map_lifts[i].joint = None;
                    } else {
                        // (A recording's loader adopted the recorded SliderJoint2Ds already: adopt_prefab_joint finds the entry.)
                        if a.slot.is_none() && !self.adopted_joints.contains_key(&a.id) {
                            unimplemented!("mod-only: MapLiftBehaviour.Start: a SliderJoint2D that was never created");
                        }
                        let f = self.adopt_prefab_joint(&a);
                        self.map_lifts[i].joint = Some(f);
                    }
                }
                PendingStart::Coupler(i) => self.coupler_start(i),
                PendingStart::Conveyor(i) => self.conveyor_start(i),
                PendingStart::ReactorCore(i) => self.reactor_core_start(i),
                PendingStart::ReactorConduitSet(i) => self.reactor_conduit_set_start(i),
                PendingStart::RandomSound(i) => self.random_sound_start(i),
                PendingStart::PlayRandomClip(i) => self.play_random_clip_start(i),
                PendingStart::SeismicExplosion(i) => self.seismic_explosion_start(i),
                PendingStart::Transmitter(i) => self.transmitter_start(i),
                PendingStart::TextStart(class, i) => match class {
                    "ThermometerBehaviour" => self.thermometer_update_activation(i),
                    "DefibrillatorBehaviour" => self.defibrillator_start(i),
                    // LiquidCanisterBehaviour.Start: UpdateText.
                    "LiquidCanisterBehaviour" => self.liquid_canister_update_text(i),
                    "TextMeshProControlReplacer" => self.control_replacer_start(i),
                    _ => unreachable!("TextStart of {class}"),
                },
                PendingStart::IonBolt(i) => self.ion_bolt_start(i),
                PendingStart::Winch(i, _) => {
                    let f = self.winch_slider(i);
                    self.winch_start(i, f);
                }
                PendingStart::CarTexture(t) => self.random_car_texture_start(t),
                PendingStart::LiquidPump(l) => self.liquid_pump_start(l),
                // A component destroyed before its first frame (a paste the same frame's undo deletes) never starts.
                PendingStart::ExistInDetailView(id) | PendingStart::ExistInBeta(id) | PendingStart::ExistInThermalVision(id) if self.destroyed_components.contains(&id) => {}
                PendingStart::ExistInDetailView(id) => self.exist_in_detail_view_start(id),
                PendingStart::ExistInBeta(id) => self.exist_in_beta_start(id),
                PendingStart::ExistInThermalVision(id) => self.exist_in_thermal_vision_start(id),
                PendingStart::ThermalDisplay => self.thermal_display_start(),
                PendingStart::FirearmAttachmentPoint(id) => self.firearm_attachment_point_start(id),
                PendingStart::IgnoreParentSize(id) => self.ignore_parent_size_start(id),
                PendingStart::RandomSprite(r) => self.random_sprite_start(r),
                PendingStart::Airfoil(a) => self.airfoil_start(a),
                PendingStart::Shockwave(a) => self.shockwave_start(a),
                PendingStart::StaticNeedle(a) => self.static_needle_start(a),
                PendingStart::Metronome(m) => self.metronome_start(m),
                PendingStart::V45(v) => self.v45_start(v),
                PendingStart::Blaster(b) => self.blaster_start(b),
                PendingStart::SmoothLine(s) => self.smooth_line_start(s),
                PendingStart::DeterminedAnomaly(a) => self.determined_anomaly_start(a),
                PendingStart::StartAudioSourceAtRandomTime(id) => self.start_audio_source_at_random_time_start(id),
                PendingStart::ArrayCloner(id, _, _) if self.destroyed_components.contains(&id) => {}
                PendingStart::ArrayCloner(id, amount, delta) => self.array_cloner_start(id, amount, delta),
                PendingStart::MaintainScale(id) => self.maintain_scale_start(id),
                PendingStart::Foliage(f) => self.foliage_start(f),
                PendingStart::LiquidSpread(s) => self.liquid_spread_start(s),
                PendingStart::ActivationToggle(t) => self.activation_toggle_start(t),
                PendingStart::PowerGauge(g) => self.power_gauge_start(g),
                PendingStart::Flare(f) => self.flare_start(f),
                PendingStart::Syringe(s) => self.syringe_start(s),
                PendingStart::SyringeExplosion(e) => self.syringe_explosion_start(e),
                PendingStart::Limb(l) => self.limb_start(l),
                PendingStart::Node(n, keep) => self.node_start(n, keep.as_deref()),
                PendingStart::Grip(g) => self.grip_start(g),
                PendingStart::GoreString(l, create) => self.gore_string_start(l, create),
                PendingStart::MapLight(m) => self.map_light_start(m),
                // BoundingBoxBehaviour.Start: Global.main.CameraControlBehaviour.BoundingBox (Extend is camera only).
                PendingStart::BoundingBox(b) => self.BoundingBox = b,
                PendingStart::MapConfig(m) => self.map_config_start(&m),
                PendingStart::SerialiseInstructions(n, root) => {
                    // The runtime children's objects are among RelevantTransforms too: each gets one unless Reconstruct added it.
                    let tids: Vec<i32> = self.unsynced.iter().chain(&self.retained).find(|i| i.objects.first().is_some_and(|o| o.components[0] == root)).map(|i| i.objects.iter().map(|o| o.components[0]).collect()).unwrap_or_default();
                    let extra: usize = tids.into_iter().map(|t| self.runtime_tids(t).into_iter().filter(|c| !self.identity_added_at.contains_key(&(*c, false))).count()).sum();
                    for _ in 0..n + extra {
                        self.allocate_instance_id();
                    }
                    self.identity_added_at.insert((root, true), self.next_instance_id);
                }
                // ExistifUnderwater.Start
                // ExistifUnderwater.GetPoint: transform.position + Offset; every prefab user has Space World and a zero Offset.
                PendingStart::BloodPuffChildWater(b, name, invert) => {
                    let p = self.splats[self.blood_puffs[b].splat].parent_position;
                    if !(invert ^ self.is_point_under_water([p[0], p[1]])) {
                        self.blood_puffs[b].off.push(name);
                        let at = self.effect_stamp();
                        self.pooled_child_off.push((super::Pooled::BloodPuff(b), name, at));
                    }
                }
                PendingStart::BloodPuffEmitterWater(b) => {
                    let s = self.blood_puffs[b].splat;
                    let p = self.splats[s].parent_position;
                    if self.is_point_under_water([p[0], p[1]]) {
                        self.blood_puffs[b].emitter_active = false;
                    }
                }
            });
            self.log_exception(caught);
        }
    }

    /// PhysicalBehaviour.CreateImpactEffect of a Human (ShotImpact: BloodPuff poolable, tagged Poolable).
    /// The puff and BloodImpactBehaviour.SetColor's colour (none when a shard has no PseudoBloodImpactHelper).
    pub fn create_blood_puff(&mut self, p: usize, position: [f32; 2], z: f32, _size: f32) -> Option<(usize, Option<[f32; 4]>)> {
        let b = self.pool_request_blood_puff(position)?;
        // localScale *= size (presentation); eulerAngles = (0, 0, z): the emitter's rotation (its local one is identity).
        let half = z * mathf::DEG2RAD * 0.5;
        self.splats[self.blood_puffs[b].splat].rotation = crate::unity::transform::normalize([0.0, 0.0, crate::unity::sinf(half), crate::unity::cosf(half)]);
        // BroadcastMessage("OnImpactCreated") down the object: a limb's CirculationBehaviour (never to an inactive object).
        if !self.physicals[p].active {
            return Some((b, None));
        }
        let color = if let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == p) {
            self.circulation_on_impact_created(l, Some(b))
        } else {
            // A shard's PseudoBloodImpactHelper (the same GameObject).
            let id = self.physicals[p].id;
            let components = self.with_component_instance(id, |_, inst, o| inst.objects[o].components.clone());
            let Some(helper) = self.pseudo_blood_helpers.iter().position(|h| components.contains(&h.0)) else { return Some((b, None)) };
            self.pseudo_blood_impact_on_impact_created(helper, Some(b))
        };
        Some((b, color))
    }

    /// BloodImpactBehaviour.SetColor: the particles' start colour and the emitter's decal colour.
    pub(crate) fn blood_impact_set_color(&mut self, b: usize, mut color: [f32; 4]) -> [f32; 4] {
        if self.prefs.GorelessMode {
            color = super::circulation::change_red_to_orange(color);
        }
        color[3] = 0.4;
        self.splats[self.blood_puffs[b].splat].DecalColourMultiplier = color;
        color
    }

    /// PseudoBloodImpactHelper.OnImpactCreated(gm): gm's BloodImpactBehaviour (`blood_puffs` index; None when gm has
    /// none) gets the helper's Color (SetColor). The SetColor result.
    pub(crate) fn pseudo_blood_impact_on_impact_created(&mut self, helper: usize, gm: Option<usize>) -> Option<[f32; 4]> {
        let b = gm?;
        let color = self.pseudo_blood_helpers[helper].1;
        Some(self.blood_impact_set_color(b, color))
    }

    /// The BloodPuff poolable prefab's GameObjects plus components (the instance ids its Instantiate takes), from the Human
    /// PhysicalProperties' ShotImpact; 0 without the game's data or a puff ShotImpact.
    fn blood_puff_id_count(&mut self) -> usize {
        if let Some(n) = self.blood_puff_ids {
            return n;
        }
        let Some(i) = self.properties.iter().position(|x| matches!(x.ShotImpact, Some((_, true)))) else { return 0 };
        let was = self.properties[i].ShotImpact;
        if matches!(was, Some((None, _))) {
            if crate::game_assets::find_game(None).is_err() {
                return 0;
            }
            self.recorded_shot_impact(i);
        }
        let prefab = self.properties[i].ShotImpact.and_then(|(r, _)| r);
        self.properties[i].ShotImpact = was;
        let Some(r) = prefab else { return 0 };
        let Ok(game) = crate::game_assets::find_game(None) else { return 0 };
        let db = self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        let n = db.prefab(r).map_or(0, |p| p.objects.iter().map(|o| 1 + o.components.len()).sum());
        self.blood_puff_ids = Some(n);
        n
    }

    /// PoolGenerator.RequestPrefab(BloodPuff poolable): ObjectPoolBehaviour.Request.
    pub(crate) fn pool_request_blood_puff(&mut self, position: [f32; 2]) -> Option<usize> {
        if self.blood_puff_pool.is_none() {
            // PoolGenerator.Ensure: the ObjectPoolBehaviour joins the Update list.
            self.behaviours.add(0, super::Behaviour::ObjectPool(super::pool::PoolRef::BloodPuff));
            // A replay's empty pool that an earlier scenario of the launch made (ClearEverything emptied it; it stays).
            self.blood_puff_pool = self.take_recorded_pool("BloodPuff poolable");
            // fc41 r15: a new pool (none recorded): new GameObject(name).AddComponent<ObjectPoolBehaviour>() takes the
            // GameObject's, its Transform's and the behaviour's instance ids, as pool_ensure does.
            if self.blood_puff_pool.is_none() {
                for _ in 0..3 {
                    self.allocate_instance_id();
                }
            }
        }
        let pool = self.blood_puff_pool.get_or_insert_with(Default::default);
        pool.inactivityTimer = 0.0;
        // availableObjects.Pop()
        if let Some(b) = pool.available.pop() {
            // transform.position = position; SetActive(true) replays the particle systems (playOnAwake).
            let puff = &mut self.blood_puffs[b];
            puff.active = true;
            let s = puff.splat;
            let emitter = puff.emitter_active;
            self.splats[s].parent_position = [position[0], position[1], 0.0];
            // ExplosionSoundBehviour.OnPoolableReinitialised: Start, i.e. Play
            // OnPoolableInitialised, then OnPoolableReinitialised in hierarchy order: ExplosionSoundBehviour.Start
            // (Play), PoolableSizeHelper (localScale), the emitter's FancyBloodSplatController.Start if active.
            self.pick_random(8);
            // FancyBloodSplatController.OnPoolableReinitialised: SetParent/localPosition are hierarchy only (parent_position is set above), then Start
            if emitter {
                self.splat_start(s);
            }
            return Some(b);
        }
        // PoolGenerator.PoolSize
        if self.blood_puff_pool.as_ref().unwrap().created.len() >= 256 {
            return None;
        }
        // fc41 r15: Instantiate takes an instance id for each GameObject and component of the prefab (fnbb11_gen case 0: the
        // game's first puff took 37, so the BleedingParticle after it had another id than ours).
        let ids = self.blood_puff_id_count();
        self.allocate_instance_ids(ids);
        // Instantiate(Prefab, position, identity). Awake in hierarchy order: the root's ExplosionSoundBehviour,
        // PoolableParticleSystemHelper and PoolableSizeHelper; the Extra decal emitter's and water puff's
        // ActivateIfFancyEffects each draw `Chance(1) > Random.value`.
        let emitter_active = 1.0 > self.random.value() && self.prefs.FancyEffects;
        let water_puff = 1.0 > self.random.value() && self.prefs.FancyEffects;
        self.splats.push(FancyBloodSplat {
            UnparentAtStart: false,
            DirectionMode: 0,
            Continuous: false,
            Direction: [0.0, 1.0],
            SpawningChance: 0.1,
            AmountRange: [1.0, 5.0],
            AngleRandomisation: 90.0,
            LifetimeInSeconds: [1.0, 2.0],
            EjectionForce: [0.5, 3.0],
            DecalColourMultiplier: [1.0, 1.0, 1.0, 1.0],
            Size: 0.2,
            DestroyWhenNoParticles: false,
            TriggerOnCollide: false,
            on_collide: None,
            on_hole: None,
            Decal: "Sparse_blood".into(),
            particles: Default::default(),
            parent_position: [position[0], position[1], 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            started: false,
            destroyed: false,
            CollisionLayers: 10752,
            parent_gone: false,
            attached: None,
            attached_node: None,
            parent_person: None,
            id: 0,
        });
        self.behaviours.add(0, super::Behaviour::Splat(self.splats.len() - 1));
        let s = self.splats.len() - 1;
        let b = self.blood_puffs.len();
        let off = if water_puff { Vec::new() } else { vec!["water puff"] };
        self.blood_puffs.push(BloodPuff { active: true, splat: s, emitter_active, off });
        // Starts in the same order: ExplosionSoundBehviour (Play), the Debris and Trail ExistifUnderwaters (on
        // above water), then the emitter's FancyBloodSplatController and ExistifUnderwater.
        self.pending_starts.push(PendingStart::BloodPuffSound);
        for name in ["Debris2", "Trail", "Debris2 (1)"] {
            self.pending_starts.push(PendingStart::BloodPuffChildWater(b, name, true));
        }
        if emitter_active {
            self.pending_starts.push(PendingStart::Splat(s));
            self.pending_starts.push(PendingStart::BloodPuffEmitterWater(b));
        }
        if water_puff {
            self.pending_starts.push(PendingStart::BloodPuffChildWater(b, "water puff", false));
        }
        let pool = self.blood_puff_pool.as_mut().unwrap();
        pool.created.push(b);
        Some(b)
    }

    /// FancyBloodSplatController.Start (initialParent is the bleeding particle: parent_position).
    pub fn splat_start(&mut self, s: usize) {
        self.splats[s].started = true;
        // UnparentAtStart only changes the hierarchy.
        let v = self.splats[s].AmountRange;
        let num = (self.splat_vector_to_random(v) as f64).round_ties_even() as i32;
        self.splats[s].particles.clear();
        if !self.splats[s].Continuous {
            for i in 0..num {
                self.create_splat_particle(s, i);
            }
        }
    }

    /// FancyBloodSplatController.FixedUpdate
    pub fn splat_fixed_update(&mut self, s: usize) {
        if self.splats[s].destroyed {
            return;
        }
        let id = self.splats[s].id;
        if id != 0 && self.destroyed_components.contains(&id) {
            self.splats[s].destroyed = true;
            return;
        }
        // A pooled BloodPuff's emitter: no FixedUpdate while it or the puff is inactive.
        if self.blood_puffs.iter().any(|b| b.splat == s && !(b.active && b.emitter_active)) {
            return;
        }
        // A bleeding particle's splat is inactive with its limb.
        if self.splats[s].attached.is_some_and(|(p, _, _)| !self.physicals[p].active) {
            return;
        }
        // MonoBehaviour::CallUpdateMethod (1808b70c0) runs Start (flag +0x125, the one DelayedStartCall 1808b75f0
        // sets) before a behaviour's first Update or FixedUpdate when its delayed call has not come yet: a splat
        // made after the last delayed pass (a crush in a WaitForFixedUpdate coroutine) starts here. One made during
        // this pass is not in it (run_behaviour_list).
        if !self.splats[s].started {
            self.splat_start(s);
        }
        self.splats[s].particles.remove_where(|p| p.Life <= 0.0);
        if self.splats[s].Continuous {
            if self.random.value() <= self.splats[s].SpawningChance {
                self.create_splat_particle(s, 0);
            }
        } else if self.splats[s].particles.is_empty() && self.splats[s].DestroyWhenNoParticles {
            self.splats[s].destroyed = true;
            self.splats_destroying.push(s);
            if id != 0 {
                self.destroy_object(id);
            }
            return;
        }
        let fdt = self.time.fixedDeltaTime;
        let g = self.world.gravity;
        let sp = &self.splats[s];
        let (mask, size, colour, root) = (sp.CollisionLayers, sp.Size, sp.DecalColourMultiplier, sp.parent_person);
        for i in 0..self.splats[s].particles.slots.len() {
            let Some(p) = &mut self.splats[s].particles.slots[i] else { continue };
            p.PositionX += p.VelocityX;
            p.PositionY += p.VelocityY;
            p.VelocityX = (d(p.VelocityX) + d(g.x) * fdt / 10.0f32).f();
            p.VelocityY = (d(p.VelocityY) + d(g.y) * fdt / 10.0f32).f();
            p.Life -= fdt;
            // Particle.get_Position: new Vector2(PositionX, PositionY).
            let position = [p.PositionX, p.PositionY];
            if self.splats[s].parent_gone {
                continue;
            }
            // initialParent outlives the splat: a bleeding particle destroys its splat with it, and a
            // BloodExplosion's DeleteAfterTime (5 s) comes after its drops (2 s at most) are gone.
            let Some((hit, collider)) = self.overlap_point_first(position, mask) else { continue };
            // collider.transform.root: a bare collider's root is its anchor physical's.
            let owner = hit.or_else(|| match self.bare_colliders[&collider].anchor {
                super::BareAnchor::Physical(p) => Some(p),
                _ => None,
            });
            let same_root = root.is_some_and(|person| self.limbs.iter().any(|l| Some(l.PhysicalBehaviour) == owner && l.Person == person));
            // collider.transform.root != initialParent.root: a splat hung under a physical compares with that physical's root.
            let parent_root = self.splats[s].attached.map(|(p, _, _)| self.root_id(p)).or_else(|| self.splats[s].attached_node.map(|(n, _)| self.node_root_id(n)));
            let same_root = same_root || parent_root.is_some_and(|r| owner.is_some_and(|q| self.root_id(q) == r));
            if colour[3] > 0.05 && !same_root {
                // SendMessage("Decal") to the collider's GameObject.
                match hit {
                    Some(hit) => {
                        let ty = self.decal_descriptor(&self.splats[s].Decal.clone());
                        self.physical_decal(hit, ty, position, size, colour);
                    }
                    None if self.bare_colliders[&collider].receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"]) => unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour"),
                    None => {}
                }
                self.splats[s].particles.slots[i].as_mut().unwrap().Life = -1.0;
                if self.splats[s].TriggerOnCollide {
                    // FancyBloodSplatController.add_OnCollide: the listener is linked as on_collide / on_hole
                    // FancyBloodSplatController.remove_OnCollide: nothing unsubscribes at run time
                    // FancyBloodSplatController.IsCircleMode: only the inspector's HideIf attribute reads it
                    // OnCollide?.Invoke(collider): its listener is a BloodTankBehaviour (blood_tank.rs) or a LiquidOutletBehaviour.
                    match self.splats[s].on_collide {
                        Some(super::container::Owner::BloodTank(t)) => self.blood_tank_on_particle_collide(t, collider),
                        Some(super::container::Owner::LiquidOutlet(l)) => self.liquid_outlet_on_particle_collide(l, collider),
                        None if self.splats[s].on_hole.is_some() => self.liquid_hole_on_particle_collide(self.splats[s].on_hole.unwrap(), collider),
                        // OnCollide?.Invoke with no subscriber does nothing.
                        None => {}
                        // Only BloodTankBehaviour, LiquidOutletBehaviour and LiquidContainerHoleBehaviour subscribe in the assembly.
                        Some(owner) => unreachable!("FancyBloodSplatController.OnCollide listener {owner:?}"),
                    }
                }
            }
        }
    }

    /// FancyBloodSplatController.VectorToRandom: UnityEngine.Random.Range(v.x, v.y).
    pub(crate) fn splat_vector_to_random(&mut self, v: [f32; 2]) -> f32 {
        self.random.range(v[0], v[1])
    }

    /// FancyBloodSplatController.CreateParticle
    pub fn create_splat_particle(&mut self, s: usize, index: i32) {
        let life = {
            let v = self.splats[s].LifetimeInSeconds;
            self.splat_vector_to_random(v)
        };
        self.update_splat_pose(s);
        let position = self.splats[s].parent_position;
        let force = self.splat_ejection_force(s, index);
        // Particle.set_Position: PositionX, PositionY = initialParent.position's x, y.
        // Particle.set_Velocity: VelocityX, VelocityY = 0.15 * GetEjectionForce(index).
        self.splats[s].particles.add(SplatParticle {
            VelocityX: 0.15 * force[0],
            VelocityY: 0.15 * force[1],
            PositionX: position[0],
            PositionY: position[1],
            Life: life,
        });
    }

    /// initialParent.position of a splat that moves with its limb.
    /// The Transform id of a node's root: the top of its parent chain.
    pub(super) fn node_root_id(&self, n: usize) -> i32 {
        let mut t = n;
        while let Some(up) = self.transform_nodes[t].parent {
            t = up;
        }
        self.transform_nodes[t].id
    }

    pub(super) fn update_splat_pose(&mut self, s: usize) {
        if let Some((n, _)) = self.splats[s].attached_node {
            let mut v = [0.0f32; 4];
            for node in self.node_chain(n).iter().rev() {
                v = crate::unity::transform::global_position(node, v);
            }
            self.splats[s].parent_position = [v[0], v[1], v[2]];
            return;
        }
        let Some((p, bleeding, _)) = self.splats[s].attached else { return };
        let mut v = bleeding.t;
        for node in self.local_chain(p).iter().rev() {
            v = crate::unity::transform::global_position(node, v);
        }
        self.splats[s].parent_position = [v[0], v[1], v[2]];
    }

    /// The splat's transform.TransformDirection(v).
    fn splat_transform_direction(&self, s: usize, v: [f32; 2]) -> [f32; 3] {
        use crate::unity::transform::rotate_direction;
        let sp = &self.splats[s];
        let mut v = [v[0], v[1], 0.0, 0.0];
        if let Some((n, local_rotation)) = sp.attached_node {
            v = rotate_direction(local_rotation, None, v);
            for node in self.node_chain(n).iter().rev() {
                v = rotate_direction(node.q, Some(node.s), v);
            }
            return [v[0], v[1], v[2]];
        }
        match sp.attached {
            None => v = rotate_direction(sp.rotation, None, v),
            Some((p, bleeding, local_rotation)) => {
                v = rotate_direction(bleeding.q, Some(bleeding.s), rotate_direction(local_rotation, None, v));
                for node in self.local_chain(p).iter().rev() {
                    v = rotate_direction(node.q, Some(node.s), v);
                }
            }
        }
        [v[0], v[1], v[2]]
    }

    /// FancyBloodSplatController.GetEjectionForce: the randomised angle turns the direction (the transform's for
    /// Directed, as is for GlobalDirected, a circle step for Circle), scaled by a random draw from EjectionForce.
    pub(crate) fn splat_ejection_force(&mut self, s: usize, index: i32) -> [f32; 2] {
        let q = self.randomised_angle(s);
        let sp = &self.splats[s];
        let dir = match sp.DirectionMode {
            1 => [sp.Direction[0], sp.Direction[1], 0.0],
            2 => {
                let num = mathf::max(sp.AmountRange[0], sp.AmountRange[1]) as i32;
                let f = (d((360 / num) as f32) * index as f32 * (std::f32::consts::PI / 180.0)).f();
                [(f as f64).cos() as f32, (f as f64).sin() as f32, 0.0]
            }
            _ => self.splat_transform_direction(s, sp.Direction),
        };
        let r = mathf::rotate(q, dir);
        let e = self.splats[s].EjectionForce;
        let k = self.splat_vector_to_random(e);
        [r[0] * k, r[1] * k]
    }

    /// FancyBloodSplatController.GetRandomisedAngle: Quaternion.Lerp(identity, Quaternion.Euler(0, 0, Random.value * 360), AngleRandomisation / 360)
    pub(crate) fn randomised_angle(&mut self, s: usize) -> [f32; 4] {
        let z = self.random.value() * 360.0 * mathf::DEG2RAD * 0.5;
        // EulerToQuaternion (18073d0c0) calls the CRT's single-precision sinf/cosf.
        let target = [0.0, 0.0, crate::unity::sinf(z), crate::unity::cosf(z)];
        let t = mathf::clamp01(self.splats[s].AngleRandomisation / 360.0);
        // QuaternionTpl Lerp: dot(identity, target) = target.w; negative flips the target.
        let sign = if target[3] < 0.0 { -1.0 } else { 1.0 };
        let q = [
            0.0 + t * (sign * target[0] - 0.0),
            0.0 + t * (sign * target[1] - 0.0),
            0.0 + t * (sign * target[2] - 0.0),
            1.0 + t * (sign * target[3] - 1.0),
        ];
        let n = mathf::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    }
}

#[cfg(test)]
mod decal_tests {
    use super::*;

    #[test]
    fn drip_streak_ease_out_and_will_drip() {
        assert_eq!(decal_drip_streak_ease_out(0.0), 0.0);
        assert_eq!(decal_drip_streak_ease_out(1.0), 1.0);
        assert_eq!(decal_drip_streak_ease_out(0.5), 1.0 - 0.5f32 * 0.5 * 0.5);
        let mut d = DecalDescriptor { name: String::new(), Color: [0.0; 4], IgnoreRadius: 0.0, Sprites: 0, MinDripCount: 0, MaxDripCount: 0 };
        assert!(!d.will_drip());
        d.MaxDripCount = 1;
        assert!(d.will_drip());
    }

    #[test]
    fn vector_to_random_draws_one_range_and_limb_color_sets() {
        let mut g = Game::default();
        let mut h = Game::default();
        assert_eq!(g.splat_vector_to_random([2.0, 5.0]), h.random.range(2.0, 5.0));
        g.limbs.push(Default::default());
        g.physicals.push(Default::default());
        g.physicals[0].sprite = Some((Default::default(), [1.0; 4], 0));
        g.limb_set_color(0, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(g.limbs[0].Color, [0.1, 0.2, 0.3, 1.0]);
        // set_Color also sets the limb's SpriteRenderer colour (fc41).
        assert_eq!(g.physicals[0].sprite.unwrap().1, [0.1, 0.2, 0.3, 1.0]);
    }
    use crate::assets::db::AssetDb;

    /// Blood is packed in a SpriteAtlas: the sprites find their texture through it.
    #[test]
    fn decal_sprites_resolve_through_their_atlas() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let mut g = Game::default();
        for r in db.find_scripts("DecalDescriptor").unwrap() {
            let i = g.load_decal_descriptor(&mut db, r).unwrap();
            let (sprites, _) = &g.decal_sprites[&i];
            assert!(!sprites.is_empty());
            for s in sprites {
                let packed = db.sprite(*s).unwrap();
                assert!(db.resolve(s.file, packed.texture).is_some());
                assert!(packed.texture_rect[2] > 0.0);
            }
        }
    }

    /// A drip streak grows along gravity at full gravity, and stops once its t passes 1.
    #[test]
    fn decal_streak_runs_down_to_its_length() {
        let mut g = Game::default();
        g.world.gravity.y = -9.81;
        g.time.deltaTime = 0.25;
        let mut ph = crate::game::Physical::default();
        ph.transform.rotation = [0.0, 0.0, 0.0, 1.0];
        ph.transform.localScale = [1.0, 1.0, 1.0];
        let streak = Streak { radius: 0.03, speed: 1.0, length: 2.0, ..Default::default() };
        ph.decal_controllers.push(DecalController { decals: vec![Decal { position: [0.0; 2], size: 1.0, sprite: 0, colour: [1.0; 4], scale: [1.0, 1.0], streaks: vec![streak] }], descriptor: 0, holder: None, holder_hidden: false, holder_objects: Vec::new() });
        g.physicals.push(ph);
        g.active_streaks.push([0, 0, 0, 0]);
        g.decal_streaks_update();
        assert!(g.physicals[0].decal_controllers[0].decals[0].streaks[0].z.abs() - 90.0 < 0.001);
        for _ in 0..5 {
            g.decal_streaks_update();
        }
        assert!(g.active_streaks.is_empty());
        let s = &g.physicals[0].decal_controllers[0].decals[0].streaks[0];
        assert!((s.scale[0] - 2.03).abs() < 0.001 && s.scale[1] == 0.03);
    }

    /// The shipped table equals the DecalDescriptor assets, and every FancyBloodSplatController in the data
    /// (prefabs and scenes) uses Sparse blood, the descriptor the splat constructors name.
    #[test]
    fn decal_descriptors_match_data() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let mut g = Game::default();
        let assets = db.find_scripts("DecalDescriptor").unwrap();
        assert!(!assets.is_empty());
        for r in assets {
            let (_, mb, v) = db.mono(r).unwrap().unwrap();
            let data = DecalDescriptor::read(&mb.name, &v).unwrap();
            if let Some(shipped) = DecalDescriptor::shipped(&mb.name) {
                assert_eq!(shipped, data);
            }
            let i = g.load_decal_descriptor(&mut db, r).unwrap();
            assert_eq!(g.decal_descriptors[i], data);
        }
        assert!(["Sparse blood", "Dense blood"].iter().all(|n| g.decal_descriptor_index(n).is_some()));
        for r in db.find_scripts("FancyBloodSplatController").unwrap() {
            let (_, _, v) = db.mono(r).unwrap().unwrap();
            let decal = db.resolve(r.file, v.get("Decal").unwrap().pptr().unwrap()).unwrap();
            assert_eq!(db.mono(decal).unwrap().unwrap().1.name, "Sparse blood");
        }
    }
}

/// RandomSpriteBehaviour: the sprite pick that its Awake and Start draw. The outline refresh and context menu are UI.
#[derive(Clone, Debug)]
pub struct RandomSprite {
    pub PhysicalBehaviour: usize,
    pub PickRandomOnStart: bool,
    pub chosenIndex: i32,
    /// sprites, each with the renderer's bounds it gives (SpriteRenderer::GetSpriteBounds).
    pub sprites: Vec<(crate::assets::db::ObjRef, ([f32; 3], [f32; 3]))>,
    /// The RandomSpriteChildBehaviours that Awake added SyncSprite to OnAfterChange for (`Game::random_sprite_children`).
    pub listeners: Vec<usize>,
    /// The component's instance id.
    pub id: i32,
}

impl Default for RandomSprite {
    fn default() -> Self {
        Self {
            PhysicalBehaviour: Default::default(),
            PickRandomOnStart: true,
            chosenIndex: i32::MIN,
            sprites: Default::default(),
            listeners: Default::default(),
            id: Default::default(),
        }
    }
}

impl RandomSprite {
    /// RandomSpriteBehaviour's stored member.
    pub fn fields(&mut self) -> Vec<(&'static str, &mut dyn super::Field)> {
        vec![("chosenIndex", &mut self.chosenIndex as &mut dyn super::Field)]
    }
}

/// RandomSpriteChildBehaviour: follows its RandomSpriteBehaviour parent's chosenIndex.
#[derive(Default, Clone, Debug)]
pub struct RandomSpriteChild {
    /// Sprites.Length
    pub SpriteCount: usize,
    /// GetComponentInParent<RandomSpriteBehaviour>() (`Game::random_sprites`).
    pub parent: Option<usize>,
    /// The index of the Sprite that spriteRenderer.sprite holds.
    pub sprite: Option<usize>,
    /// Sprites (a null entry: None).
    pub sprites: Vec<Option<crate::assets::db::ObjRef>>,
    /// GetComponent<SpriteRenderer>()'s instance id (0: none).
    pub renderer: i32,
}

impl Game {
    /// RandomSpriteBehaviour.Awake
    pub fn random_sprite_awake(&mut self, r: usize, enabled: bool) {
        if enabled {
            let s = &self.random_sprites[r];
            if !s.sprites.is_empty() && s.PickRandomOnStart {
                self.random_sprites[r].chosenIndex = self.random.range_int(0, s.sprites.len() as i32);
            }
            self.random_sprite_set_to_index(r);
        }
    }

    /// RandomSpriteBehaviour.Start: SetSpriteToIndex and the two context menu buttons.
    pub(crate) fn random_sprite_start(&mut self, r: usize) {
        self.random_sprite_set_to_index(r);
        use super::menu::{MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.random_sprites[r].PhysicalBehaviour].menu;
        menu.push(B { LabelWhenMultipleAreSelected: crate::il!(str "RandomSpriteBehaviour::Start" 1), ..B::new("nextSkin", crate::il!(str "RandomSpriteBehaviour::Start" 1), crate::il!(str "RandomSpriteBehaviour::Start" 2), A::Skin(r, 1)) });
        menu.push(B { LabelWhenMultipleAreSelected: crate::il!(str "RandomSpriteBehaviour::Start" 5), ..B::new("previousSkin", crate::il!(str "RandomSpriteBehaviour::Start" 5), crate::il!(str "RandomSpriteBehaviour::Start" 6), A::Skin(r, -1)) });
    }

    /// RandomSpriteBehaviour.SetSpriteToIndex
    pub fn random_sprite_set_to_index(&mut self, r: usize) {
        let n = self.random_sprites[r].sprites.len() as i32;
        if n == 0 {
            return;
        }
        let s = &self.random_sprites[r];
        let i = if s.chosenIndex == i32::MIN {
            if s.PickRandomOnStart { self.random.range_int(0, n) } else { 0 }
        } else if s.chosenIndex >= n {
            0
        } else if s.chosenIndex < 0 {
            n - 1
        } else {
            s.chosenIndex
        };
        self.random_sprites[r].chosenIndex = i;
        let s = &self.random_sprites[r];
        let (sprite, bounds) = s.sprites[i as usize];
        let p = &mut self.physicals[s.PhysicalBehaviour];
        p.sprite.as_mut().expect("RandomSpriteBehaviour without a SpriteRenderer").0 = sprite;
        p.sprite_bounds = Some(bounds);
        // OnAfterChange?.Invoke(): the persistent calls, then the runtime listeners, in the order they were added.
        self.invoke_event(self.random_sprites[r].id, "OnAfterChange");
        for child in self.random_sprites[r].listeners.clone() {
            self.random_sprite_child_sync_sprite(child);
        }
    }

    /// RandomSpriteChildBehaviour.Awake: spriteRenderer and parent are found; a parent gets SyncSprite as an OnAfterChange listener.
    pub fn random_sprite_child_awake(&mut self, c: usize) {
        if let Some(parent) = self.random_sprite_children[c].parent {
            self.random_sprites[parent].listeners.push(c);
        }
    }

    /// RandomSpriteChildBehaviour.SyncSprite: spriteRenderer.sprite = Sprites[parent.chosenIndex].
    pub(crate) fn random_sprite_child_sync_sprite(&mut self, c: usize) {
        let parent = self.random_sprite_children[c].parent.expect("a SyncSprite listener has a parent");
        let index = self.random_sprites[parent].chosenIndex;
        if index < 0 || index as usize >= self.random_sprite_children[c].SpriteCount {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
        }
        self.random_sprite_children[c].sprite = Some(index as usize);
        // spriteRenderer.sprite = Sprites[index] (gapC r7: the renderer itself, compared as `ItemSprite sprite=`).
        let x = &self.random_sprite_children[c];
        let Some(at) = self.item_sprite(x.renderer) else { crate::game::menu::throw("NullReferenceException: RandomSpriteChildBehaviour.spriteRenderer") };
        let Some(o) = x.sprites[index as usize] else { unimplemented!("mod-only: RandomSpriteChildBehaviour: a null sprite") };
        match at {
            crate::game::visual::SpriteAt::Physical(p) => { if let Some(s) = self.physicals[p].sprite.as_mut() { s.0 = o } }
            crate::game::visual::SpriteAt::Renderer(r) => self.renderers[r].sprite.0 = o,
        }
    }

    /// StartAudioSourceAtRandomTimeBehaviour.Start: AudioSource.time = clip.length * Random.value; the audio is
    /// presentation, the draw is not.
    pub fn start_audio_source_at_random_time_start(&mut self, id: i32) {
        // AudioSource.time = AudioSource.clip.length * Random.value
        let s = self.audio.source_field(id, "AudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: StartAudioSourceAtRandomTimeBehaviour.AudioSource"));
        let clip = self.audio.get(s).clip.clone().unwrap_or_else(|| super::menu::throw("NullReferenceException: StartAudioSourceAtRandomTimeBehaviour.AudioSource.clip"));
        let t = clip.length() * self.random.value();
        self.audio.set_time(s, t);
    }

    /// Where clone object `o`'s Transform hangs: the nearest physical or transform node on or above it, and the local
    /// TRS of each object below that (`o` first).
    pub(crate) fn object_anchor(&self, inst: &super::spawn::Instance, o: usize) -> (super::BareAnchor, Vec<crate::unity::transform::Trs>) {
        let (mut chain, mut up) = (Vec::new(), Some(o));
        while let Some(x) = up {
            if let Some(p) = (0..self.physicals.len()).find(|&p| !self.physicals[p].destroyed && inst.objects[x].components.contains(&self.physicals[p].id)) {
                return (super::BareAnchor::Physical(p), chain);
            }
            if let Some(n) = self.transform_nodes.iter().position(|n| n.id == inst.objects[x].components[0]) {
                return (super::BareAnchor::Node(n), chain);
            }
            chain.push(inst.objects[x].trs);
            up = inst.objects[x].parent;
        }
        (super::BareAnchor::Root, chain)
    }

    /// ArrayCloner.Start: Amount - 1 clones of the live GameObject (the object and everything below it, Awake of the clone
    /// run, its ArrayCloner destroyed at once), `Delta * i` further each, at the object's rotation and lossy scale.
    pub fn array_cloner_start(&mut self, id: i32, amount: i32, delta: [f32; 2]) {
        if amount <= 1 {
            return;
        }
        let (prefab, o, position, rotation, scale) = self.with_component_instance(id, |_, inst, o| (inst.prefab.clone(), o, inst.position(o), inst.rotation(o), inst.lossy_scale(o)));
        let sub = super::link::subtree_prefab(&prefab, o);
        // obj.transform.SetParent(base.transform.parent, worldPositionStays: true): what the parent hangs from (the
        // nearest physical or transform node above it, the objects between), when it has one.
        let under = self.with_component_instance(id, |g, inst, o| inst.objects[o].parent.map(|p| g.object_anchor(inst, p)));
        let has_body = sub.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Rigidbody2D(_))));
        if under.is_some() && has_body {
            // batch3 r7: the export's only ArrayCloner (grep of every Assets file for its script GUID; nothing AddComponents
            // it) is Uncharted.prefab's Root/lD: parent Root, no Rigidbody2D in its subtree. So this is mod-only.
            unimplemented!("mod-only: ArrayCloner: a clone with a Rigidbody2D SetParent under its base's parent");
        }
        let cloner = sub.objects[0].components.iter().position(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "ArrayCloner")).expect("the ArrayCloner of its object");
        for i in 1..amount {
            // Vector2 vector2 = vector + Delta * i; Instantiate(gameObject, vector2, rotation) (z 0).
            let at = [position[0] + delta[0] * i as f32, position[1] + delta[1] * i as f32, 0.0];
            let mut db = self.assets.take().expect("ArrayCloner without the game's data");
            // (The clone's own TRS from here on is local to the parent, which the link takes as its anchor.)
            let local = under.as_ref().map(|(a, c)| {
                use crate::unity::transform::set_world;
                let nodes = self.anchored_nodes(*a, c);
                let ((t, q), _) = set_world(&nodes, at, rotation);
                (t, q, crate::unity::transform::local_scale_under(&nodes, rotation, scale, q))
            });
            // (The link takes the clone root's TRS as its objects' chains: its localScale is the one set right after.)
            let mut sub = sub.clone();
            for (_, c) in sub.objects[0].components.iter_mut() {
                if let crate::assets::db::Component::Transform(t) = c {
                    t.local_scale = local.map_or(scale, |l| l.2);
                }
            }
            self.link_under = under.clone();
            let spawned = match local {
                Some((t, q, _)) => self.spawn(&mut db, &sub, t, q, None),
                None => self.spawn(&mut db, &sub, at, rotation, None),
            };
            self.link_under = None;
            self.assets = Some(db);
            let (mut inst, linked) = spawned.unwrap_or_else(|e| panic!("ArrayCloner: Instantiate: {e}"));
            // obj.transform.localScale = base.transform.lossyScale; Destroy(obj.GetComponent<ArrayCloner>()).
            inst.set_local_scale(0, local.map_or(scale, |l| l.2));
            self.refresh_transforms(&inst, &linked);
            self.destroyed_components.insert(inst.objects[0].components[cloner]);
            self.unsynced.push(inst);
        }
    }
}

impl Game {
    /// PlayAtRandom.Start: `if (!(Random.value > Chance))` plays its ParticleSystems and AudioSources.
    pub(crate) fn play_at_random_start(&mut self, id: i32, chance: f32) {
        if !(self.random.value() > chance) {
            for k in 0..self.field_len(id, "ParticleSystems") {
                self.ps_field_play_stop(id, &format!("ParticleSystems[{k}]"), true);
            }
            for s in self.audio.source_fields(id, "AudioSources").to_vec() {
                let s = s.unwrap_or_else(|| super::menu::throw("NullReferenceException: PlayAtRandom.AudioSources has a null entry"));
                self.audio.play(s);
            }
        }
    }
}
