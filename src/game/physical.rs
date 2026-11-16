//! PhysicalBehaviour and the Utils energy/temperature helpers.
use super::{d, mathf, script, BareAnchor, Game, Pose, Rigidbody};
use crate::unity::Random;
use crate::b2::contacts2d::{ContactPoint2D, ScriptCollision};
use crate::b2::world::BodyType;

/// SharpAxis
#[derive(Clone, Copy, Debug, Default)]
pub struct SharpAxis {
    pub Axis: [f32; 2],
    pub LowerLimit: f32,
    pub UpperLimit: f32,
    pub LooseUpperLimit: bool,
    pub LooseLowerLimit: bool,
}

impl SharpAxis {
    /// SharpAxis..ctor(axis = default, lowerLimit = 0, higherLimit = 100, looseUpper = true, looseLower = false)
    pub fn new(axis: [f32; 2], lower_limit: f32, higher_limit: f32, loose_upper: bool, loose_lower: bool) -> Self {
        SharpAxis { Axis: axis, LowerLimit: lower_limit, UpperLimit: higher_limit, LooseUpperLimit: loose_upper, LooseLowerLimit: loose_lower }
    }
}

/// PhysicalProperties (ScriptableObject): the material values the scripts read.
#[derive(Clone, Debug)]
pub struct PhysicalProperties {
    pub name: String,
    /// PhysicMaterial friction, None without a material.
    pub friction: Option<f32>,
    /// PhysicMaterial bounciness (0 without a material).
    pub bounciness: f32,
    pub Softness: f32,
    pub Flammability: f32,
    pub Conducting: bool,
    pub MagneticAttractionIntensity: f32,
    pub Burnrate: f32,
    pub BurningTemperatureThreshold: f32,
    pub HeatTransferSpeedMultiplier: f32,
    pub Buoyancy: f32,
    pub ImpactIntensityMutliplier: f32,
    pub HitVolumeMultiplier: f32,
    pub SparksOnSlide: bool,
    pub SoftImpact: usize,
    pub HardImpact: usize,
    /// SizzleSounds length; None when the array is null.
    pub SizzleSounds: Option<usize>,
    // The stabbing values.
    pub Sharp: bool,
    pub IgnoreStabResistance: bool,
    pub SharpForceThresholdMultiplier: f32,
    pub SharpAxes: Vec<SharpAxis>,
    /// LodgeStrengthMultiplier
    pub LodgeStrengthMultiplier: f32,
    /// StabSound length; None when the array is null.
    pub StabSound: Option<usize>,
    pub Brittleness: f32,
    pub BulletSpeedAbsorptionPower: f32,
    /// The ShotImpact prefab (None from a recording, which only names it) and whether it is "BloodPuff poolable"
    /// (`create_blood_puff`); None when null.
    pub ShotImpact: Option<(Option<crate::assets::db::ObjRef>, bool)>,
    /// The clips of SoftImpact, HardImpact, SizzleSounds, StabSound and SlidingLoop (empty from a recording).
    pub clips: PropertyClips,
}

impl Default for PhysicalProperties {
    fn default() -> Self {
        Self {
            name: Default::default(),
            friction: Default::default(),
            bounciness: Default::default(),
            Softness: Default::default(),
            Flammability: Default::default(),
            Conducting: Default::default(),
            MagneticAttractionIntensity: Default::default(),
            Burnrate: Default::default(),
            BurningTemperatureThreshold: 5000.0,
            HeatTransferSpeedMultiplier: 1.0,
            Buoyancy: Default::default(),
            ImpactIntensityMutliplier: 1.0,
            HitVolumeMultiplier: 1.0,
            SparksOnSlide: Default::default(),
            SoftImpact: Default::default(),
            HardImpact: Default::default(),
            SizzleSounds: Default::default(),
            Sharp: Default::default(),
            IgnoreStabResistance: Default::default(),
            SharpForceThresholdMultiplier: 1.0,
            SharpAxes: Default::default(),
            LodgeStrengthMultiplier: 1.0,
            StabSound: Default::default(),
            Brittleness: Default::default(),
            BulletSpeedAbsorptionPower: Default::default(),
            ShotImpact: Default::default(),
            clips: Default::default(),
        }
    }
}

type Clips = Vec<Option<super::audio::ClipRef>>;

/// A FireLoopChunkBehaviour: its position, RandomStart, Dirty and the AudioSource its first FuzzySetVolume adds.
#[derive(Clone, Debug, Default)]
pub struct FireLoopChunk {
    /// The FireLoopChunkBehaviour's instance id.
    pub id: i32,
    pub at: [f32; 2],
    pub random_start: f32,
    pub dirty: bool,
    pub source: Option<i32>,
}

/// PhysicalProperties' AudioClips (not part of the dumped state: Debug shows nothing).
#[derive(Clone, Default)]
pub struct PropertyClips {
    pub SoftImpact: Clips,
    pub HardImpact: Clips,
    pub SizzleSounds: Clips,
    pub StabSound: Clips,
    pub SlidingLoop: Option<super::audio::ClipRef>,
}

impl std::fmt::Debug for PropertyClips {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("_")
    }
}

/// ContextMenuOptionComponent..ctor: Buttons = new List<ContextMenuButton>().
fn context_menu_option_component_new() -> Vec<super::menu::MenuButton> {
    Vec::new()
}

/// Instance ids of the components and objects PhysicalBehaviour.Awake/Start add (0 = none). A NodeBehaviour or
/// ContextMenuOptionComponent the prefab already carries is set by the spawner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartIds {
    pub ActivationNode: i32,
    pub ContextMenuOptions: i32,
    pub audioSource: i32,
    pub slidingAudioSource: i32,
    /// selectionOutlineObject (its Transform, Optout and SpriteRenderer take the next three ids)
    pub selectionOutlineObject: i32,
    pub outlineSpriteRenderer: i32,
}

// PhysicalBehaviour.ColliderBoolPair (and its IsValid): nothing uses it since 1.28.41 (collisionBuffer holds ContactPoint2Ds).

script! {
    Physical {
        SimulateTemperature: bool = true,
        AmbientTemperatureTransfer: bool = true,
        OnFire: bool = false,
        DisplayBloodDecals: bool = true,
        BurnProgress: f32 = 0.0,
        isSliding: bool = false,
        EnergyWireResistance: f32 = 1.0,
        BuoyancyModifier: f32 = 1.0,
        IsWeightless: bool = false,
        charge: f32 = 0.0,
        ConductOverride: bool = true,
        ForceNoCharge: bool = false,
        StabCausesWound: bool = true,
        IsUnderWater: bool = false,
        underwaterMarkings: i32 = 0,
        IsInLava: bool = false,
        CurrentWaterSurfaceLevel: f32 = 0.0,
        seed: f32 = 0.0,
        intSeed: i32 = 0,
        burnIntensity: f32 = 0.0,
        slideIntensity: f32 = 0.0,
        previousVelocity: [f32; 2] = [0.0, 0.0],
        Temperature: f32 = 0.0,
        beingHeldByGripper: bool = false,
        Wetness: f32 = 0.0,
        GridRes: i32 = 4,
        ObjectArea: f32 = 0.0,
        TemperatureWhenBurningLerpMultiplier: f32 = 1.0,
        BurningProgressionMultiplier: f32 = 1.0,
        Circumference: f32 = -1.0,
        outOfBoundsDuration: f32 = 0.0,
        affectSurroundingTimer: f32 = 0.0,
        sizzleAudioHeat: f32 = 0.0,
        TrueInitialMass: f32 = 0.0,
        InitialMass: f32 = 0.0,
        InitialGravityScale: f32 = 0.0,
        isDisintegrated: bool = false,
        ChargeBurns: bool = true,
        ForceNoChargeParticles: bool = false,
        Deletable: bool = true,
        Selectable: bool = true,
        Disintegratable: bool = true,
        dirtyFire: bool = false,
        dirtyCharged: bool = false,
        dirtySmoke: bool = false,
        dirtyBubble: bool = false,
        SpawnSpawnParticles: bool = true,
        PlaySliderSound: bool = true,
        ForceContinuous: bool = false,
    }
    links {
        /// PhysicalBehaviour.SoundVolumeBoost (the prefab's; Start widens the audioSource's distances by it).
        SoundVolumeBoost: f32 = 0.0,
        /// PhysicalBehaviour.HasOutline (ShatteredObjectGenerator clears it on its fragments).
        /// PhysicalBehaviour.Resizable (the prefab's; the dump has no field).
        Resizable: bool = true,
        HasOutline: bool = true,
        /// PhysicalBehaviour.showOutline: the outline object's activeSelf (Awake sets it false).
        showOutline: bool = false,
        /// Object.GetHashCode(): Mono's object hash (UnityEngine.Object does not override it with the instance id),
        /// a heap-address value that differs from run to run; it seeds Start's Random.InitState, so it is an input
        /// (the recorder dumps it as `hash=`).
        hash_code: i32 = 0,
        /// The objects Awake and Start created, by instance id.
        ids: StartIds = StartIds::default(),
        /// ContextMenuOptions.Buttons
        menu: Vec<super::menu::MenuButton> = Vec::new(),
        /// ActivationPropagationDelay: only UseWireBehaviour reads it; HaltNextPropagation sets it to -1.
        ActivationPropagationDelay: f32 = 0.1,
        /// SendUserPropagation: a use wire on the object passes direct activations on.
        SendUserPropagation: bool = true,
        /// ReflectsLasers and AbsorbsLasers (the dump format has neither).
        ReflectsLasers: bool = false,
        AbsorbsLasers: bool = true,
        /// An AudioSourceTimeScaleBehaviour sits on this object (the spawner sets it for prefab-carried ones).
        audio_time_scale: bool = false,
        /// rigidbody.sharedMaterial is not null in the prefab.
        rigidbody_has_material: bool = false,
        /// GetComponents<DecalControllerBehaviour>().Length at Start (prefab-carried ones).
        prefab_decal_controllers: usize = 0,
        /// spriteRenderer.bounds at Awake (center, extents).
        InitialBounds: ([f32; 3], [f32; 3]) = ([0.0; 3], [0.0; 3]),
        name: String = String::new(),
        id: i32 = 0,
        /// The GameObject's Transform instance id (0 for physicals loaded from a recording).
        transform_id: i32 = 0,
        BulletPenetration: bool = true,
        /// C#'s float.NaN (0/0f, the x86 default NaN 0xffc00000): an AddComponent'd PhysicalBehaviour keeps the initializer's bits.
        BulletDeflectChanceOverride: f32 = f32::from_bits(0xffc0_0000),
        /// OverrideShotSounds length.
        OverrideShotSounds: usize = 0,
        /// The Lodged/Dislodged receiver component on the GameObject (ExcaliburBehaviour, a SyringeBehaviour), if any.
        lodge_receiver: String = String::new(),
        /// PhysicalBehaviour.colliders: the GameObject's non-trigger Collider2D instance ids, in component order.
        colliders: Vec<i32> = Vec::new(),
        /// Every Collider2D on the GameObject (triggers too), as GetComponentsInChildren<Collider2D> sees them.
        all_colliders: Vec<i32> = Vec::new(),
        /// The serialized Collider2Ds behind `colliders` (BakeColliderGridPoints reads their geometry); empty for
        /// physicals loaded from a recording.
        collider_defs: Vec<crate::assets::components::Collider2D> = Vec::new(),
        /// gameObject.isStatic
        isStatic: bool = false,
        Properties: usize = 0,
        rigidbody: Rigidbody = Rigidbody { body: 0, bodyType: 0, collisionDetectionMode: 0, interpolation: 0, movement: super::MovementState::default(), detached: None, component: 0, simulated: true, detached_props: [0.0; 3], constraints: 0 },
        transform: Pose = Pose::default(),
        /// The Transform's parent (index into Game.transform_nodes).
        parent: Option<usize> = None,
        /// Rigidbody2D m_DrivenByParentRigidbody (CheckForDrivenByParentRigidbody, 180c1dc80): for a non-static body,
        /// the b2 body of the nearest ancestor with a Rigidbody2D.
        driven_by: Option<usize> = None,
        /// collisionBuffer[0..collisionBufferCount]: Rigidbody2D.GetContacts' points from the last awake
        /// OnCollisionStayWithoutSleep (at most 16).
        collisionBuffer: Vec<ContactPoint2D> = Vec::new(),
        /// spriteRenderer.isVisible: drawn by the last render (`physicals_will_render`).
        isVisible: bool = false,
        /// scrapeParticles: Start instantiated Global.MetalScrapePrefab (SparksOnSlide then).
        scrapeParticles: bool = false,
        LocalColliderGridPoints: Option<Vec<[f32; 2]>> = None,
        /// HoldingPositions (local points a GripBehaviour can hold; a person's dump has none).
        HoldingPositions: Vec<[f32; 3]> = Vec::new(),
        stabWoundCount: u32 = 0,
        /// Indices into Game.penetration_arena.
        penetrations: Vec<usize> = Vec::new(),
        victimPenetrations: Vec<usize> = Vec::new(),
        /// The stabbers (physical indices) of the live penetrations into this object.
        beingStabbedBy: Vec<usize> = Vec::new(),
        StabReleaseSpeedModifier: f32 = 1.0,
        StabWoundSizeMultiplier: f32 = 1.0,
        /// The GameObject is active in the hierarchy (a recording's activeSelf: its physicals have no inactive
        /// parents).
        active: bool = true,
        enabled: bool = true,
        /// The GameObject was destroyed (Object.Destroy ran): removed from PhysicalObjectsInWorld.
        destroyed: bool = false,
        /// In Global.PhysicalObjectsInWorld: Start adds it (a spawned one is out until then).
        in_world: bool = true,
        /// Set when the oracle loads a component it has no data for.
        has_fire_map: bool = false,
        /// FireParticleSystem is set (SetupFirePrefab ran); a recording's FireParticleSystem field.
        has_fire_system: bool = false,
        /// Its LocalFirePropagationMap (fire_maps); None in a recording.
        fire_map: Option<usize> = None,
        /// The DecalControllerBehaviours PhysicalBehaviour.Decal added, one per descriptor, in add order.
        decal_controllers: Vec<super::misc::DecalController> = Vec::new(),
        /// spriteRenderer's local AABB (center, extents); None when no sprite data was loaded.
        sprite_bounds: Option<([f32; 3], [f32; 3])> = None,
        /// spriteRenderer.enabled
        sprite_enabled: bool = true,
        /// spriteRenderer's component instance id (0: none).
        sprite_id: i32 = 0,
        /// For drawing: the spriteRenderer's Sprite, color and sorting order.
        sprite: Option<(crate::assets::db::ObjRef, [f32; 4], i16)> = None,
        /// For drawing: the spriteRenderer's flipX/flipY, drawMode (Simple 0, Sliced 1, Tiled 2) and size.
        sprite_draw: crate::game::SpriteDraw = ([false; 2], 0, [0.0; 2], 0.0, false),
        /// For drawing: the spriteRenderer's sortingLayerID (0, the Default layer, without a renderer).
        sprite_layer: i32 = 0,
        /// The sortingOrder of a renderer drawn outside `sprite` (a human limb's, drawn with the skin).
        bare_order: i16 = 0,
        /// The GameObject has an Optout component.
        has_optout: bool = false,
        /// The SpriteRenderer GameObject's layer and first material.
        sprite_game_layer: i32 = 0,
        sprite_material: Option<crate::assets::db::ObjRef> = None,
        MaximumStabVictims: i32 = 64,
        LooseEndSize: f32 = 0.49,
        /// OverrideImpactSounds length (0 when null).
        OverrideImpactSounds: usize = 0,
        /// The AudioSource AddComponent'ed in Awake (non-static objects).
        audioSource: bool = true,
        /// ContinuousActivationTracker, one DeltaInt per channel (Awake).
        ContinuousActivationTracker: [super::continuous::DeltaInt; 3] = [super::continuous::DeltaInt::new(0); 3],
    }
}

/// The index of a null DecalDescriptor (an Android limb's BloodDecal).
pub(crate) const NULL_DECAL: usize = usize::MAX;

/// Instance ids of a Prefabs/SpawnParticles clone: GameObject, Transform, ParticleSystem, ParticleSystemRenderer,
/// DeleteAfterTime.
const SPAWN_PARTICLES_OBJECTS: usize = 5;

impl Game {
    /// Global.PhysicalObjectsInWorldByTransform looked up by a collider (the collider's transform).
    pub fn physical_of_collider(&self, collider: i32) -> Option<usize> {
        self.physicals.iter().position(|p| !p.destroyed && p.all_colliders.contains(&collider))
    }
}

/// FireLoopSoundControllerBehaviour.CreateChunkAt's position: Origin + ChunkSize * (x, y) (Origin from Main's bounding box).
pub(crate) fn fire_loop_chunk_at(key: (i32, i32)) -> [f32; 2] {
    let origin = [572.5f32 - 578.4, 496.4f32 - 496.6];
    [(d(origin[0]) + d(12.0f32) * key.0 as f32).f(), (d(origin[1]) + d(12.0f32) * key.1 as f32).f()]
}

/// Utils.Mod
pub(super) fn utils_mod(a: f32, b: f32) -> f32 {
    (d(a) - d(mathf::floor(a / b)) * b).f()
}

impl Game {
    /// Rigidbody2D.mass = v (Rigidbody2D::SetMass 180c23790): clamped, then b2Body::ResetMassData.
    /// Rigidbody2D.mass = v on physical `p`'s component (Rigidbody2D::SetMass 180c23790): after Cleanup destroyed the b2 body
    /// only m_Mass changes (clamped; a UseAutoMass component only warns).
    pub(super) fn physical_set_mass(&mut self, p: usize, v: f32) {
        match self.physicals[p].rigidbody.detached.as_mut() {
            Some(m) => {
                if !m.use_auto_mass {
                    m.mass = if 0.0001 <= v && v <= 1_000_000.0 { v } else if 0.0001 <= v { 1_000_000.0 } else { 0.0001 };
                }
            }
            None => {
                let body = self.physicals[p].rigidbody.body;
                self.body_set_mass(body, v)
            }
        }
    }

    /// Rigidbody2D.gravityScale = v / .mass = v on the Rigidbody2D of a physical or a node.
    fn owner_set_gravity_scale(&mut self, o: super::BodyOwner, v: f32) {
        match o {
            super::BodyOwner::Physical(p) => self.physical_set_gravity_scale(p, v),
            super::BodyOwner::Node(k) => self.node_rigidbodies[k].rigidbody.set_gravity_scale(&mut self.world, v),
        }
    }

    fn owner_set_mass(&mut self, o: super::BodyOwner, v: f32) {
        match o {
            super::BodyOwner::Physical(p) => self.physical_set_mass(p, v),
            super::BodyOwner::Node(k) => {
                let body = self.node_rigidbodies[k].rigidbody.body;
                self.body_set_mass(body, v)
            }
        }
    }

    /// Rigidbody2D.gravityScale = v on physical `p`'s component: without a b2 body (Cleanup) only m_GravityScale (clamped) changes.
    pub(super) fn physical_set_gravity_scale(&mut self, p: usize, v: f32) {
        self.physicals[p].rigidbody.set_gravity_scale(&mut self.world, v);
    }

    pub(super) fn body_set_mass(&mut self, body: crate::b2::world::BodyId, v: f32) {
        let v = if 0.0001 <= v && v <= 1_000_000.0 { v } else if 0.0001 <= v { 1_000_000.0 } else { 0.0001 };
        self.world.body_mut(body).rigidbody.mass = v;
        self.world.reset_mass_data(body);
    }

    /// PhysicalBehaviour.Awake. The body, fixtures, colliders and Transform exist and the fields hold their prefab
    /// values. Not modelled: SortingGroup, ContinuousActivationTracker (activation propagation).
    pub fn physical_awake(&mut self, p: usize) {
        // ActivationNode = GetOrAddComponent<NodeBehaviour>(): the prefab's node, else a new one (the next instance id).
        if self.physicals[p].ids.ActivationNode == 0 {
            let own = Some(super::BodyOwner::Physical(p));
            self.physicals[p].ids.ActivationNode = match self.activation_nodes.iter().find(|n| n.owner == own && !self.destroyed_components.contains(&n.id)) {
                Some(n) => n.id,
                None => {
                    let id = self.allocate_instance_id();
                    self.activation_nodes.push(super::activations::ActivationNode { id, owner: own, ..Default::default() });
                    id
                }
            };
        }
        if self.physicals[p].isStatic {
            self.physicals[p].PlaySliderSound = false;
        }
        if !self.physicals[p].enabled || !self.physicals[p].active {
            self.physicals[p].isDisintegrated = true;
        }
        self.physicals[p].Temperature = self.AmbientTemperature;
        self.physicals[p].InitialBounds = self.sprite_world_bounds(p);
        let body = self.physicals[p].rigidbody.body;
        self.physicals[p].TrueInitialMass = self.world.body(body).rigidbody.mass;
        // rigidbody.useAutoMass = false: a flag change resets the mass data.
        if self.world.body(body).rigidbody.use_auto_mass {
            self.world.body_mut(body).rigidbody.use_auto_mass = false;
            self.world.reset_mass_data(body);
        }
        if self.physicals[p].ids.ContextMenuOptions == 0 {
            self.physicals[p].ids.ContextMenuOptions = self.allocate_instance_id();
            self.physicals[p].menu = context_menu_option_component_new();
        }
    }

    /// The topmost Transform node above a physical object (None for a root).
    fn transform_root(&self, p: usize) -> Option<usize> {
        let (mut root, mut n) = (self.body_node(p), self.physicals[p].parent);
        while let Some(i) = n {
            root = Some(i);
            n = self.transform_nodes[i].parent;
        }
        root
    }

    /// GetComponentInParent<AudioSourceTimeScaleBehaviour>(): on the object or any ancestor (the root carries it).
    fn has_audio_time_scale_above(&self, p: usize) -> bool {
        let root = self.transform_root(p);
        self.physicals[p].audio_time_scale
            || root.is_some_and(|r| self.audio_time_scale_roots.contains(&r))
            || root.is_some() && (0..self.physicals.len()).any(|q| self.physicals[q].audio_time_scale && self.transform_root(q) == root)
    }

    /// PhysicalBehaviour.Start, run in Start order (`physicals` is Global.PhysicalObjectsInWorld, which Start
    /// appends to). Random.InitState(GetHashCode()) seeds with `hash_code`.
    pub fn physical_start(&mut self, p: usize) {
        self.physicals[p].in_world = true;
        if !self.has_audio_time_scale_above(p) {
            // transform.root.gameObject.AddComponent<AudioSourceTimeScaleBehaviour>(); its Start comes in the next
            // Start pass and takes the AudioSources below the root (this one's among them).
            self.allocate_instance_id();
            self.physicals[p].audio_time_scale = true;
            let root = self.transform_root(p).map_or(self.physicals[p].transform_id, |n| self.transform_nodes[n].id);
            self.pending_starts.push(super::misc::PendingStart::AudioTimeScale(root, false));
        }
        let body = self.physicals[p].rigidbody.body;
        if self.physicals[p].rigidbody.bodyType == 0 && self.physicals[p].LocalColliderGridPoints.as_ref().map_or(true, |g| g.is_empty()) {
            // The AutoSyncTransforms the first non-trigger collider's bounds starts with.
            if self.physicals[p].collider_defs.iter().any(|c| !c.is_trigger) {
                self.sync_pending();
            }
            let baked = self.physical_bake_collider_grid_points(p);
            self.physicals[p].LocalColliderGridPoints = Some(baked);
        }
        if self.physicals[p].prefab_decal_controllers != 0 {
            unimplemented!("mod-only: PhysicalBehaviour.Start: decalControllers.AddRange(prefab DecalControllerBehaviours)");
        }
        self.random = Random::new(self.physicals[p].hash_code);
        let seed = self.random.range(-1500.0, 1500.0);
        let int_seed = self.random.range_int(-1500, 1500);
        self.physicals[p].seed = seed;
        self.physicals[p].intSeed = int_seed;
        self.physicals[p].affectSurroundingTimer = utils_mod(seed, 2.0);
        self.physicals[p].InitialGravityScale = self.world.body(body).gravity_scale;
        // Mathf.Sqrt(spriteRenderer.bounds.extents.sqrMagnitude * 4f)
        let e = self.sprite_world_bounds(p).1;
        let sqr = (d(e[0]) * e[0] + d(e[1]) * e[1] + d(e[2]) * e[2]).f();
        self.physicals[p].ObjectArea = mathf::sqrt(sqr * 4.0);
        // rigidbody.interpolation = Global.CurrentInterpolationMode
        let mode = self.current_interpolation_mode();
        self.rigidbody_set_interpolation(super::BodyOwner::Physical(p), mode);
        self.physicals[p].InitialMass = self.physicals[p].rigidbody.mass(&self.world);
        if !self.physicals[p].isStatic {
            let id = self.allocate_instance_id();
            let ancestors = self.physical_add_audio_source(p, id);
            if self.physicals[p].PlaySliderSound {
                let id = self.allocate_instance_id();
                self.physical_add_sliding_audio_source(p, id, ancestors);
            }
            self.physical_set_properties(p);
            // PhysicalBehaviour.CreateOutlineObject: new GameObject("Outline") (GameObject, Transform), Optout, SpriteRenderer.
            self.physicals[p].ids.selectionOutlineObject = self.allocate_instance_id();
            self.allocate_instance_id();
            self.allocate_instance_id();
            self.physicals[p].ids.outlineSpriteRenderer = self.allocate_instance_id();
        }
        self.set_show_outline(p, false);
        // CollisionQuality.Continuous is 1.
        let mode = if self.prefs.CollisionQuality == 1 || self.physicals[p].ForceContinuous { 1 } else { 0 };
        let mut rb = self.physicals[p].rigidbody.clone();
        rb.set_collision_detection_mode(&mut self.world, mode);
        self.physicals[p].rigidbody = rb;
        self.physical_recalculate_mass_based_on_size(p);
        if self.physicals[p].IsWeightless {
            self.physical_make_weightless(p);
        }
        if self.physicals[p].isDisintegrated {
            self.physical_disintegrate(p);
        }
        self.physical_setup_spawn_prefab(p);
        self.physical_add_menu_buttons(p);
        for i in 0..self.local_grid_point_length(p) {
            let c = self.random.inside_unit_circle();
            let g = &mut self.physicals[p].LocalColliderGridPoints.as_mut().unwrap()[i];
            *g = [g[0] + c[0] * 0.02, g[1] + c[1] * 0.02];
        }
        if self.physicals[p].Circumference < 0.0 {
            self.physical_calculate_circumference(p);
        }
        // Instantiate(Global.main.MetalScrapePrefab, transform): a particle effect for OnCollisionStay to play.
        // Only its instance ids and its ActivateIfFancyEffects Awakes reach the simulation (load_main checks):
        // SetActive(Chance > Random.value && FancyEffects) on childless flash objects.
        if self.properties[self.physicals[p].Properties].SparksOnSlide {
            let (n, draws, objects) = match &self.metal_scrape {
                Some(m) => m.clone(),
                None => self.load_metal_scrape(),
            };
            // Its Transforms (each object's id, then its components': the Transform first) under the physical's.
            let mut id = self.next_instance_id;
            let nodes: Vec<(i32, Option<usize>)> = objects.iter().map(|&(k, up)| { let t = id - 2; id -= 2 * k as i32; (t, up) }).collect();
            self.add_runtime_clone(self.physicals[p].transform_id, &nodes);
            for _ in 0..n {
                self.allocate_instance_id();
            }
            for _ in 0..draws {
                self.random.value();
            }
            self.physicals[p].scrapeParticles = true;
        }
    }

    /// PhysicalBehaviour.CalculateCircumference: the perimeter of LocalColliderGridPoints' convex hull
    /// (Utils.ComputeConvexHull sorts the points in place).
    pub(crate) fn physical_calculate_circumference(&mut self, p: usize) {
        let ph = &mut self.physicals[p];
        if ph.LocalColliderGridPoints.as_ref().unwrap().is_empty() {
            // The AutoSyncTransforms the first non-trigger collider's bounds starts with.
            if ph.collider_defs.iter().any(|c| !c.is_trigger) {
                self.sync_pending();
            }
            let baked = self.physical_bake_collider_grid_points(p);
            self.physicals[p].LocalColliderGridPoints = Some(baked);
        }
        let ph = &mut self.physicals[p];
        let points = ph.LocalColliderGridPoints.as_mut().unwrap();
        if points.is_empty() {
            ph.Circumference = 0.0;
            return;
        }
        let mut hull = Vec::new();
        super::shatter::compute_convex_hull(points, &mut hull);
        let mut num = 0.0f32;
        for i in 0..hull.len() {
            num += super::shatter::v2_distance(hull[i], hull[(i + 1) % hull.len()]);
        }
        ph.Circumference = num;
    }

    /// Collider2D.bounds (Collider2D::GetBounds, 180c11150): the union of the shapes' AABBs (ComputeAABB without the
    /// radius) as center and extents, z from the Transform. The caller runs the AutoSyncTransforms it starts with.
    pub(super) fn collider_bounds(&self, p: usize, collider: i32) -> ([f32; 3], [f32; 3]) {
        self.collider_bounds_on(self.physicals[p].rigidbody.body, collider, self.physicals[p].transform.position)
    }

    /// `collider_bounds` of a collider on `body`, whose GameObject's Transform.position is `position`.
    pub(super) fn collider_bounds_on(&self, body: crate::b2::world::BodyId, collider: i32, position: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let body = self.world.body(body);
        let mut aabb: Option<crate::b2::broadphase::Aabb> = None;
        for &f in &body.fixtures {
            let fx = self.world.fixture(f);
            if fx.collider.id != collider {
                continue;
            }
            for child in 0..fx.shape.child_count() {
                let a = fx.shape.compute_aabb_no_radius(&body.xf, child);
                aabb = Some(match aabb {
                    None => a,
                    Some(b) => crate::b2::broadphase::Aabb {
                        lower: crate::b2::math::Vec2::new(b.lower.x.min(a.lower.x), b.lower.y.min(a.lower.y)),
                        upper: crate::b2::math::Vec2::new(b.upper.x.max(a.upper.x), b.upper.y.max(a.upper.y)),
                    },
                });
            }
        }
        let z = position[2];
        // Without shapes (GetBounds, 180c11150): the Transform's position with zero extents (Vector3f zero).
        let Some(a) = aabb else { return (position, [0.0; 3]) };
        ([(a.upper.x + a.lower.x) * 0.5, (a.upper.y + a.lower.y) * 0.5, z], [(a.upper.x - a.lower.x) * 0.5, (a.upper.y - a.lower.y) * 0.5, 0.0])
    }

    /// Collider2D.OverlapPoint (180c12440): any of the collider's shapes contains the point.
    pub(super) fn collider_overlap_point(&self, p: usize, collider: i32, point: [f32; 2]) -> bool {
        let body = self.world.body(self.physicals[p].rigidbody.body);
        body.fixtures.iter().map(|&f| self.world.fixture(f)).filter(|fx| fx.collider.id == collider).any(|fx| super::tool::test_point(&fx.shape, &body.xf, crate::b2::math::Vec2::new(point[0], point[1])))
    }

    /// PhysicalBehaviour.IsInsideOf: any of the colliders' OverlapPoint contains the global point.
    pub(super) fn is_inside_of(&self, p: usize, colls: &[(i32, &crate::assets::components::Collider2D)], point: [f32; 2]) -> bool {
        colls.iter().any(|&(c, _)| self.collider_overlap_point(p, c, point))
    }

    /// PhysicalBehaviour.BakeColliderGridPoints: a (GridRes + 1)^2 grid over the non-trigger colliders' bounds
    /// (shrunk by 0.999), keeping the points inside a collider and at least 0.025 from every earlier grid point,
    /// in local space; then each collider's outline points.
    pub(crate) fn physical_bake_collider_grid_points(&self, p: usize) -> Vec<[f32; 2]> {
        use crate::assets::components::ColliderShape;
        let ph = &self.physicals[p];
        if ph.collider_defs.len() != ph.colliders.len() {
            unimplemented!("mod-only: {}: BakeColliderGridPoints without the serialized colliders (not a catalog object)", ph.name);
        }
        // Bounds(position, zero), Encapsulate(collider.bounds) each: Encapsulate(center - extents) and
        // Encapsulate(center + extents), each SetMinMax(Min(min, point), Max(max, point)).
        let (mut center, mut extents) = (ph.transform.position, [0.0f32; 3]);
        let mut encapsulate = |point: [f32; 3]| {
            let min: [f32; 3] = std::array::from_fn(|i| super::mathf::min(center[i] - extents[i], point[i]));
            let max: [f32; 3] = std::array::from_fn(|i| super::mathf::max(center[i] + extents[i], point[i]));
            extents = std::array::from_fn(|i| (max[i] - min[i]) * 0.5);
            center = std::array::from_fn(|i| min[i] + extents[i]);
        };
        // GetComponents<Collider2D>() where !isTrigger.
        let colls: Vec<(i32, &crate::assets::components::Collider2D)> = ph.colliders.iter().copied().zip(&ph.collider_defs).filter(|(_, c)| !c.is_trigger).collect();
        for &(c, _) in &colls {
            let (bc, be) = self.collider_bounds(p, c);
            encapsulate(std::array::from_fn(|i| bc[i] - be[i]));
            encapsulate(std::array::from_fn(|i| bc[i] + be[i]));
        }
        // bounds.size *= 0.999f
        extents = extents.map(|e| e * 2.0 * 0.999 * 0.5);
        let size = extents.map(|e| e * 2.0);
        let num2 = ph.GridRes.max(1);
        let step = [size[0] / num2 as f32, size[1] / num2 as f32];
        let min = [center[0] - extents[0], center[1] - extents[1]];
        let mut list = Vec::new();
        let mut seen: Vec<[f32; 2]> = Vec::new();
        for num3 in 0..num2 + 1 {
            for num4 in 0..num2 + 1 {
                let pos = [min[0] + num4 as f32 * step[0], min[1] + num3 as f32 * step[1]];
                if seen.iter().any(|&q| super::shatter::v2_distance(q, pos) < 0.025) {
                    continue;
                }
                seen.push(pos);
                if self.is_inside_of(p, &colls, pos) {
                    let l = self.inverse_transform_point(p, [pos[0], pos[1], 0.0]);
                    list.push([l[0], l[1]]);
                }
            }
        }
        for &(_, c) in &colls {
            let o = c.offset;
            match &c.shape {
                ColliderShape::Polygon { paths, .. } => list.extend(paths.first().into_iter().flatten().map(|q| [q[0] + o[0], q[1] + o[1]])),
                ColliderShape::Box { size, .. } => {
                    let h = [size[0] / 2.0, size[1] / 2.0];
                    list.push([h[0] + o[0], h[1] + o[1]]);
                    list.push([(0.0 - h[0]) + o[0], h[1] + o[1]]);
                    list.push([(0.0 - h[0]) + o[0], (0.0 - h[1]) + o[1]]);
                    list.push([h[0] + o[0], (0.0 - h[1]) + o[1]]);
                }
                ColliderShape::Circle { radius } => {
                    let pi = std::f32::consts::PI;
                    let n = ((d(pi) * 2.0f32 * *radius / 0.25f32).f() as f64).ceil() as i32;
                    let n = n.clamp(6, 32);
                    for i in 0..n {
                        let f = (d(pi) * 2.0f32 * ((d(i as f32) / n as f32).f() - 1.0)).f();
                        let v = [((f as f64).cos() as f32) * radius, ((f as f64).sin() as f32) * radius];
                        list.push([v[0] + o[0], v[1] + o[1]]);
                    }
                }
                _ => {}
            }
        }
        list
    }

    /// The WeightlessHelpers among the object's children or itself (the BroadcastMessage receivers of OnMakeWeightless
    /// and OnMakeWeightful), in hierarchy order.
    fn weightless_helpers_of(&mut self, p: usize) -> Vec<i32> {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |_, inst, o| {
            let mut ids = Vec::new();
            for x in std::iter::once(o).chain(inst.descendants(o)) {
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    if matches!(c, crate::assets::db::Component::Script { class, .. } if class == "WeightlessHelper") {
                        ids.push(inst.objects[x].components[k]);
                    }
                }
            }
            ids
        })
    }

    /// WeightlessHelper.Awake: originalMass[i] = Bodies[i].mass.
    pub fn weightless_helper_awake(&mut self, id: i32) {
        let w = self.weightless_helpers.iter().position(|w| w.id == id).unwrap();
        let masses = self.weightless_helpers[w].bodies.iter().map(|&b| self.rigidbody_owner(b).map(|o| self.owner_rigidbody(o).clone()).unwrap_or_else(|| unimplemented!("mod-only: WeightlessHelper.Bodies: a Rigidbody2D outside the physicals and bare bodies")).mass(&self.world)).collect();
        self.weightless_helpers[w].original_mass = masses;
    }

    /// WeightlessHelper.OnMakeWeightless: gravityScale = 0, mass = Max(Min(mass, 0.02), mass * 0.02) for each body.
    pub(crate) fn weightless_helper_make_weightless(&mut self, id: i32) {
        let w = self.weightless_helpers.iter().position(|w| w.id == id).unwrap();
        for b in self.weightless_helpers[w].bodies.clone() {
            let o = self.rigidbody_owner(b).unwrap();
            self.owner_set_gravity_scale(o, 0.0);
            let m = self.owner_rigidbody(o).mass(&self.world);
            self.owner_set_mass(o, mathf::max(mathf::min(m, 0.02), m * 0.02));
        }
    }

    /// WeightlessHelper.OnMakeWeightful: gravityScale = 1, mass = originalMass[i] for each body.
    pub(crate) fn weightless_helper_make_weightful(&mut self, id: i32) {
        let w = self.weightless_helpers.iter().position(|w| w.id == id).unwrap();
        for (i, b) in self.weightless_helpers[w].bodies.clone().into_iter().enumerate() {
            let o = self.rigidbody_owner(b).unwrap();
            self.owner_set_gravity_scale(o, 1.0);
            self.owner_set_mass(o, self.weightless_helpers[w].original_mass[i]);
        }
    }

    /// PhysicalBehaviour.MakeWeightless. BroadcastMessage("OnMakeWeightless") reaches only WeightlessHelper.
    pub fn physical_make_weightless(&mut self, p: usize) {
        self.physical_set_gravity_scale(p, 0.0);
        let m = self.physicals[p].rigidbody.mass(&self.world);
        self.physicals[p].InitialMass = m;
        self.physical_set_mass(p, mathf::max(mathf::min(m, 0.02), m * 0.02));
        self.physicals[p].IsWeightless = true;
        for h in self.weightless_helpers_of(p) {
            self.weightless_helper_make_weightless(h);
        }
    }

    /// PhysicalBehaviour.MakeWeightful: RecalculateMassBasedOnSize, then the body's mass = InitialMass and
    /// gravityScale = InitialGravityScale, IsWeightless = false. BroadcastMessage("OnMakeWeightful") reaches only WeightlessHelper.
    pub fn physical_make_weightful(&mut self, p: usize) {
        self.physical_recalculate_mass_based_on_size(p);
        let (mass, gravity) = (self.physicals[p].InitialMass, self.physicals[p].InitialGravityScale);
        self.physical_set_mass(p, mass);
        self.physical_set_gravity_scale(p, gravity);
        self.physicals[p].IsWeightless = false;
        for h in self.weightless_helpers_of(p) {
            self.weightless_helper_make_weightful(h);
        }
    }

    /// PhysicalBehaviour.RecalculateMassBasedOnSize
    pub fn physical_recalculate_mass_based_on_size(&mut self, p: usize) {
        let s = self.lossy_scale(p);
        let ph = &mut self.physicals[p];
        ph.InitialMass = (d(ph.TrueInitialMass) * s[0].abs() * s[1].abs()).f();
        if !ph.IsWeightless {
            let (body, mass) = (ph.rigidbody.body, ph.InitialMass);
            self.body_set_mass(body, mass);
        }
    }

    /// PhysicalBehaviour.SetProperties: rigidbody.sharedMaterial = Properties.PhysicMaterial when it has none.
    /// Rigidbody2D::SetMaterial (180c23840) stores the material and runs Collider2D::UpdateColliderMaterialState
    /// (180c16380) on each attached collider: GetColliderMaterialState (180c11360) gives the collider's own material,
    /// else the body's (the prefab colliders have none, see spawn), the fixtures take its friction and bounciness and
    /// the collider refilters. The colliders are the body's fixtures' user data, sorted and made unique; the sort is
    /// by native address, modelled as fixture creation order (the allocation-order model of the heap order).
    pub(crate) fn physical_set_properties(&mut self, p: usize) {
        let props = &self.properties[self.physicals[p].Properties];
        let (Some(friction), false) = (props.friction, self.physicals[p].rigidbody_has_material) else { return };
        let bounciness = props.bounciness;
        self.physicals[p].rigidbody_has_material = true;
        let body = self.physicals[p].rigidbody.body;
        let fixtures = self.world.body(body).fixtures.clone();
        let mut colliders: Vec<i32> = Vec::new();
        for &f in &fixtures {
            let id = self.world.fixtures[f].as_ref().unwrap().collider.id;
            if !colliders.contains(&id) {
                colliders.push(id);
            }
        }
        for collider in colliders {
            for &f in &fixtures {
                let fixture = self.world.fixtures[f].as_mut().unwrap();
                if fixture.collider.id == collider {
                    (fixture.friction, fixture.restitution) = (friction, bounciness);
                }
            }
            self.world.refilter_collider(collider);
        }
    }
}

impl Physical {
    /// Charge getter
    pub fn Charge(&self) -> f32 {
        if !self.ForceNoCharge { self.charge } else { 0.0 }
    }
    /// PhysicalBehaviour.GetChargeWithWireResistance
    pub fn GetChargeWithWireResistance(&self) -> f32 {
        self.Charge() * self.EnergyWireResistance
    }
}

impl Game {
    /// Utils.RandomBetween(Vector2): Random.Range(range.x, range.y). Its caller is GorseEyeBehaviour (gorse.rs).
    pub fn random_between(&mut self, range: [f32; 2]) -> f32 {
        self.random.range(range[0], range[1])
    }

    /// PhysicalBehaviour.HaltPropagation: ActivationPropagationDelay = -1f.
    pub fn physical_halt_propagation(&mut self, p: usize) {
        self.physicals[p].ActivationPropagationDelay = -1.0;
    }

    /// PhysicalBehaviour.GetPreviousVel: the velocity recorded by the last ManagedLateUpdate.
    pub fn physical_previous_vel(&self, p: usize) -> [f32; 2] {
        self.physicals[p].previousVelocity
    }

    /// PhysicalBehaviour.ShouldUpdate: enabled and not disintegrated (the managers skip the rest).
    pub fn physical_should_update(&self, p: usize) -> bool {
        self.physicals[p].enabled && !self.physicals[p].isDisintegrated
    }

    /// PhysicalBehaviour.GetHeatCapacity
    pub fn heat_capacity(&self, p: usize) -> f32 {
        let ph = &self.physicals[p];
        if !ph.SimulateTemperature {
            return 1.0;
        }
        ph.ObjectArea * mathf::max(0.1, 0.025 * ph.rigidbody.mass(&self.world))
    }

    /// PhysicalBehaviour.IsBeingStabbed => stabWoundCount != 0
    pub fn is_being_stabbed(&self, p: usize) -> bool {
        self.physicals[p].stabWoundCount != 0
    }

    /// PhysicalBehaviour.LocalGridPointLength: 0 before the grid is baked.
    pub fn local_grid_point_length(&self, p: usize) -> usize {
        self.physicals[p].LocalColliderGridPoints.as_ref().map_or(0, |g| g.len())
    }

    /// Collider2D != null: the object behind the instance id is not destroyed (a bare collider goes with its anchor
    /// physical's object).
    pub(super) fn collider_exists(&self, c: i32) -> bool {
        // A collider of an object destroyed before a recording's snapshot (a shattered map Window) has neither.
        if self.destroyed_components.contains(&c) {
            return false;
        }
        match self.physicals.iter().position(|p| p.all_colliders.contains(&c)) {
            Some(p) => !self.physicals[p].destroyed,
            None => !matches!(self.bare_colliders.get(&c).unwrap_or_else(|| panic!("collider #{c}: no physical or bare collider")).anchor, BareAnchor::Physical(p) if self.physicals[p].destroyed),
        }
    }

    /// `(bool)collider && collider.gameObject.activeSelf`.
    pub(super) fn collider_alive(&self, c: i32) -> bool {
        self.collider_exists(c)
            && match self.physical_of_collider(c) {
                Some(p) => self.physicals[p].active,
                None => self.bare_colliders[&c].active_self,
            }
    }

    /// SendMessage("OnCollisionExit2D", new Collision2D()) to a collider's GameObject.
    pub(super) fn send_empty_collision_exit(&mut self, c: i32) {
        match self.physical_of_collider(c) {
            Some(v) => self.physical_on_collision_exit(v, &ScriptCollision::default()),
            None if self.bare_colliders[&c].receives(&["TorchBehaviour"]) => self.torch_collision_on_collider(c, crate::b2::contacts2d::Message::CollisionExit, &ScriptCollision::default()),
            None => {}
        }
    }

    /// PhysicalBehaviour.IsTouchingSomething: `(bool)rigidbody && collisionBufferCount > 0`.
    pub fn is_touching_something(&self, p: usize) -> bool {
        !self.physicals[p].destroyed && !self.physicals[p].collisionBuffer.is_empty()
    }

    /// PhysicalBehaviour.OnDestroy / OnDisable: SendMessage("OnCollisionExit2D", new Collision2D()) to each buffered
    /// contact's collider that is not null (SendMessage skips an inactive object).
    pub(crate) fn physical_send_buffer_exits(&mut self, p: usize) {
        for c in self.physicals[p].collisionBuffer.clone() {
            if self.collider_alive(c.collider) {
                self.send_empty_collision_exit(c.collider);
            }
        }
    }

    /// Rigidbody2D.GetContacts(ContactPoint2D[16]) (GetRigidbodyContactsArray_Binding 180bf0860 over
    /// PhysicsQuery2D::GetContacts 180bef0b0, no filter): PhysicsContacts2D.m_Collisions in array order, the
    /// non-trigger records not Invalid whose rigidbody A or B is this body, each manifold point seen from this body's
    /// side (B's view swaps the sides and negates the normal and relative velocity); the first 16 are kept.
    pub(super) fn rigidbody_get_contacts(&self, p: usize) -> Vec<ContactPoint2D> {
        let body = self.physicals[p].rigidbody.body;
        let c2d = &self.world.contacts2d;
        let mut out = Vec::new();
        for &rid in &c2d.collisions {
            let Some(r) = c2d.records[rid].as_ref() else { continue };
            if r.is_trigger || r.state == crate::b2::contacts2d::ContactState::Invalid {
                continue;
            }
            let receiving = if r.rigidbody_a == Some(body) {
                r.collider_a.id
            } else if r.rigidbody_b == Some(body) {
                r.collider_b.id
            } else {
                continue;
            };
            out.extend(self.world.script_collision(r, receiving).contacts);
        }
        out.truncate(16);
        out
    }

    /// Global.PhysicalObjectsInWorldByTransform.TryGetValue(rigidbody.transform) for the Rigidbody2D with this instance id.
    fn physical_of_rigidbody(&self, id: i32) -> Option<usize> {
        self.physicals.iter().position(|x| {
            !x.destroyed && x.in_world && x.rigidbody.detached.is_none() && self.world.bodies.get(x.rigidbody.body).and_then(|b| b.as_ref()).and_then(|b| b.user).map(|u| u.id) == Some(id)
        })
    }

    /// The render after LateUpdate: each physical's spriteRenderer.isVisible (drawn by this render).
    pub fn physicals_will_render(&mut self) {
        for p in 0..self.physicals.len() {
            let drawn = self.physicals[p].sprite_id != 0 && self.sprite_drawn(super::visual::SpriteAt::Physical(p));
            self.physicals[p].isVisible = drawn;
        }
    }

    /// Utils.TransferEnergyFixedRate
    pub fn transfer_energy_fixed_rate(&mut self, a: usize, b: usize, rate: f32) {
        let ca = self.physicals[a].GetChargeWithWireResistance();
        let cb = self.physicals[b].GetChargeWithWireResistance();
        if ca > cb {
            let v = mathf::lerp(self.physicals[b].Charge(), ca, rate);
            self.physicals[b].charge = v;
        } else {
            let v = mathf::lerp(self.physicals[a].Charge(), cb, rate);
            self.physicals[a].charge = v;
        }
    }

    /// Utils.GetMinHeatTransferSpeed: the lower HeatTransferSpeedMultiplier of the two objects' properties.
    pub fn min_heat_transfer_speed(&self, a: usize, b: usize) -> f32 {
        mathf::min(self.properties[self.physicals[a].Properties].HeatTransferSpeedMultiplier, self.properties[self.physicals[b].Properties].HeatTransferSpeedMultiplier)
    }

    /// Utils.AverageTemperature
    pub fn average_temperature(&mut self, a: usize, b: usize, transfer_factor: f32) {
        if !(self.physicals[a].SimulateTemperature && self.physicals[b].SimulateTemperature) {
            return;
        }
        let num = self.min_heat_transfer_speed(a, b) * transfer_factor;
        let (ta, tb) = (self.physicals[a].Temperature, self.physicals[b].Temperature);
        let ha = self.heat_capacity(a);
        self.physicals[a].Temperature = mathf::lerp(ta, tb, num / ha);
        let hb = self.heat_capacity(b);
        self.physicals[b].Temperature = mathf::lerp(tb, ta, num / hb);
    }

    /// PhysicalBehaviour.IgniteInternal
    pub(crate) fn ignite_internal(&mut self, p: usize, ignore_flammability: bool) {
        let ph = &self.physicals[p];
        let flammability = self.properties[ph.Properties].Flammability;
        if !ph.isStatic
            && (ignore_flammability || !(mathf::perlin(self.time.time * 100.0, ph.seed + 235.42) > flammability))
            && !(flammability <= mathf::EPSILON)
        {
            self.physicals[p].OnFire = true;
        }
    }

    /// PhysicalBehaviour.Shocked(zap): the charge rises by a second's 25, and one in 10000 draws ignites at the zap.
    pub fn physical_shocked(&mut self, p: usize, zap_position: [f32; 2]) {
        let dt = self.time.deltaTime;
        // Charge += : through the getter (zero when ForceNoCharge).
        // Mono keeps Time.deltaTime * 25f unrounded: the sum is done in double, then stored as float.
        self.physicals[p].charge = (self.physicals[p].Charge() as f64 + dt as f64 * 25.0) as f32;
        if self.random.value() > 0.9999 {
            self.ignite_at(p, false, zap_position);
        }
    }

    /// PhysicalBehaviour.GetLowestGlobalGridPoint: the grid point with the lowest y (the TransformPoint result is
    /// unused, so it is the local point that is returned and compared).
    pub fn physical_lowest_grid_point(&self, p: usize) -> [f32; 2] {
        // Unbaked grid points: Debug.LogWarning, then Vector2.zero.
        self.physicals[p].LocalColliderGridPoints.as_deref().map_or([0.0, 0.0], lowest_point)
    }

    /// PhysicalBehaviour.Ignite(bool, Vector2)
    pub fn ignite_at(&mut self, p: usize, ignore_flammability: bool, position: [f32; 2]) {
        self.ignite_internal(p, ignore_flammability);
        if let Some(f) = self.physicals[p].fire_map.filter(|_| self.physicals[p].OnFire) {
            self.fire_map_ignite_at(f, position);
        }
    }

    /// PhysicalBehaviour.Extinguish
    pub fn extinguish(&mut self, p: usize) {
        let threshold = self.properties[self.physicals[p].Properties].BurningTemperatureThreshold;
        let ph = &mut self.physicals[p];
        ph.Temperature = mathf::min(ph.Temperature, threshold);
        ph.OnFire = false;
    }

    /// PhysicalBehaviour.GetRandomGridPoint
    pub(crate) fn random_grid_point(&mut self, p: usize) -> [f32; 2] {
        match &self.physicals[p].LocalColliderGridPoints {
            None => [0.0, 0.0],
            Some(points) => {
                let n = points.len() as i32;
                // Utils.PickRandom
                if n == 0 {
                    return [0.0, 0.0];
                }
                let i = self.random.range_int(0, n) as usize;
                self.physicals[p].LocalColliderGridPoints.as_ref().unwrap()[i]
            }
        }
    }

    /// PhysicalBehaviour.ManagedFixedUpdate
    pub fn physical_fixed_update(&mut self, p: usize) {
        if self.physicals[p].isStatic {
            self.physicals[p].collisionBuffer.clear();
            return;
        }
        self.physicals[p].charge *= 0.95;
        if self.prefs.CollisionQuality == super::COLLISION_DYNAMIC {
            let rb = &self.physicals[p].rigidbody;
            let fast = mathf::sqr_magnitude(rb.velocity(&self.world)) > self.DynamicSqrdVelocityThreshold.0;
            let mode = rb.collisionDetectionMode;
            if fast {
                if mode != 1 {
                    let mut rb = rb.clone();
                    rb.set_collision_detection_mode(&mut self.world, 1);
                    self.physicals[p].rigidbody = rb;
                }
            } else if mode != 0 {
                let mut rb = rb.clone();
                rb.set_collision_detection_mode(&mut self.world, 0);
                self.physicals[p].rigidbody = rb;
            }
        }
        let props = self.properties[self.physicals[p].Properties].clone();
        if self.physicals[p].SimulateTemperature {
            if self.physicals[p].AmbientTemperatureTransfer {
                if self.prefs.AmbientTemperatureTransfer {
                    self.TransferHeat(p);
                } else {
                    let h = self.heat_capacity(p);
                    let ambient = self.AmbientTemperature;
                    let ph = &mut self.physicals[p];
                    ph.Temperature = mathf::lerp(ph.Temperature, ambient, 0.0001 / h);
                }
            }
            let ph = &mut self.physicals[p];
            if ph.OnFire && ph.Temperature < props.BurningTemperatureThreshold * 3.0 {
                ph.Temperature = mathf::lerp(ph.Temperature, props.BurningTemperatureThreshold * 3.0, 0.01 * ph.TemperatureWhenBurningLerpMultiplier);
            }
            let flag = ph.BurnProgress <= 0.98;
            if flag && ph.Temperature > props.BurningTemperatureThreshold && props.Flammability > mathf::EPSILON {
                if ph.BurnProgress < 0.2 {
                    ph.BurnProgress += 0.001;
                }
                if self.random.value() as f64 > 0.9995 && props.Flammability > mathf::EPSILON {
                    let local = self.random_grid_point(p);
                    let point = self.transform_point(p, local);
                    self.ignite_at(p, true, point);
                }
            }
            let ph = &mut self.physicals[p];
            if ph.OnFire {
                let m = mathf::max(0.0, ph.Temperature - props.BurningTemperatureThreshold);
                ph.BurnProgress = (d(ph.BurnProgress) + d(m) * 6e-5f32 * (d(props.Burnrate) / 2.0f32) * ph.BurningProgressionMultiplier).f();
            }
            if ph.Temperature < props.BurningTemperatureThreshold {
                if flag && ph.OnFire {
                    ph.BurnProgress = (d(ph.BurnProgress) + d(2.5e-5) * (d(props.Burnrate) / 2.0f32) * ph.BurningProgressionMultiplier).f();
                }
                if ph.OnFire && self.random.value() > 0.995 {
                    self.extinguish(p);
                }
            }
            let ph = &mut self.physicals[p];
            ph.Temperature = (d(ph.Temperature) + d(ph.Charge()) * 0.0008f32 / props.HeatTransferSpeedMultiplier).f();
            // The original passes its arguments as Clamp(-273.15, Temperature, 5000000).
            ph.Temperature = mathf::clamp(-273.15, ph.Temperature, 5_000_000.0);
        }
        self.on_collision_stay_without_sleep(p);
        self.physical_scrape_sparks(p);
        if self.physicals[p].IsUnderWater
            && self.random.value() > 0.95
            && !self.physicals[p].decal_controllers.is_empty()
            && mathf::sqr_magnitude(self.physicals[p].rigidbody.velocity(&self.world)) > 2.0
        {
            // Each decal fainter than 0.2 leaves localDecalPositions (its GameObject is destroyed), the others fade
            // by 0.2. Creation keeps decal positions apart, so the removed position is the decal's own.
            for c in &mut self.physicals[p].decal_controllers {
                c.decals.retain_mut(|decal| {
                    if decal.colour[3] < 0.2 {
                        return false;
                    }
                    decal.colour[3] = mathf::max(0.0, decal.colour[3] - 0.2);
                    true
                });
            }
        }
        self.stab_fixed_update(p);
        if self.physicals[p].OnFire {
            if self.random.value() > 0.9993 {
                self.extinguish(p);
            }
            let position = self.physicals[p].transform.position;
            let volume = mathf::clamp01(self.physicals[p].burnIntensity);
            self.fire_loop_fuzzy_set_volume_at(position, volume, 0.5);
        }
    }

    /// FireSoundEmitter.FixedUpdate: while its GameObject is activeInHierarchy, FuzzySetVolumeAt(transform.position,
    /// Clamp01(Intensity)) (the translated runtime cannot read a bare Transform's position).
    pub(crate) fn fire_sound_emitter_fixed(&mut self, i: usize) {
        let id = self.tr.fire_sound_emitter[i].this_id;
        if self.tr_active_in_hierarchy(super::translated::rt::Go::Local(id)) {
            let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
            let volume = mathf::clamp01(self.tr.fire_sound_emitter[i].Intensity);
            self.fire_loop_fuzzy_set_volume_at(position, volume, 0.5);
        }
    }

    /// FireLoopSoundControllerBehaviour.FuzzySetVolumeAt: the volume and the chunk's AudioSource are driven below; a chunk's creation is too. CreateChunkAt
    /// makes a GameObject (GameObject, Transform) with a FireLoopChunkBehaviour whose Awake draws RandomStart, and its first
    /// FuzzySetVolume adds an AudioSource and an AudioSourceTimeScaleBehaviour.
    pub(crate) fn fire_loop_fuzzy_set_volume_at(&mut self, position: [f32; 3], volume: f32, weight: f32) {
        // FireLoopChunkBehaviour.get_Dirty / set_Dirty: `FireLoopChunk::dirty`.
        // FireLoopChunkBehaviour.constructor: QuietDownSpeedMultiplier = 1 (`fire_loop_update`).
        // FireLoopSoundControllerBehaviour.Start: ChunkGridSize and Origin from the scene's ChunkSize and its Start's BoundingBox: Main's, before the map's BoundingBoxBehaviour.Start.
        // FireLoopSoundControllerBehaviour.GenerateGrid: an all-null FireChunks, `fire_loop_chunks` without a cell
        const CHUNK_SIZE: f32 = 12.0;
        let (center, extents) = ([572.5f32, 496.4], [578.4f32, 496.6]);
        let (origin, size) = ([center[0] - extents[0], center[1] - extents[1]], [extents[0] * 2.0, extents[1] * 2.0]);
        let cell = |i: usize| {
            let count = (size[i] / CHUNK_SIZE).ceil() as i32;
            super::temperature::round_to_int((position[i] - origin[i]) / CHUNK_SIZE).clamp(0, count - 1)
        };
        // FireLoopSoundControllerBehaviour.WorldToGrid: `cell` (RoundToInt of the offset over ChunkSize, clamped to the grid)
        // FireLoopSoundControllerBehaviour.GetOrCreateChunkAt: a cell without a chunk gets one, below
        let key = (cell(0), cell(1));
        if !self.fire_loop_chunks.contains_key(&key) {
            // FireLoopSoundControllerBehaviour.CreateChunkAt: GameObject, Transform and the FireLoopChunkBehaviour at
            // Origin + ChunkSize * (x, y), then SetActive(false).
            self.allocate_instance_ids(2);
            let id = self.allocate_instance_id();
            // FireLoopChunkBehaviour.Awake: RandomStart = Random.value.
            let random_start = self.random.value();
            self.fire_loop_chunks.insert(key, FireLoopChunk { id, at: fire_loop_chunk_at(key), random_start, ..Default::default() });
        }
        self.fire_loop_chunk_fuzzy_set_volume(key, volume, weight);
    }

    /// FireLoopChunkBehaviour.FuzzySetVolume's AddComponent<AudioSource> (instance id `id`) on the chunk at `at`: clip
    /// Audio/fire_loop, minDistance 1, maxDistance Range, no playOnAwake, Logarithmic, loop, spatialBlend 1, dopplerLevel 0.
    pub(crate) fn fire_loop_add_source(&mut self, at: [f32; 2], id: i32) {
        let clip = self.resource_clip("Audio/fire_loop");
        let t = crate::unity::transform::Trs { t: [at[0], at[1], 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] };
        let mut s = super::audio::AudioSource { anchor: super::BareAnchor::Root, chain: vec![t], clip, min_distance: 1.0, max_distance: 12.0 /* Range = ChunkSize */, play_on_awake: false, rolloff_mode: 0, looping: true, doppler_level: 0.0, ..Default::default() };
        s.set_spatial_blend(1.0);
        self.audio.add(id, s);
    }

    /// FireLoopChunkBehaviour.FuzzySetVolume(volume, weight)
    pub(crate) fn fire_loop_chunk_fuzzy_set_volume(&mut self, key: (i32, i32), volume: f32, weight: f32) {
        let c = self.fire_loop_chunks[&key].clone();
        let source = if c.dirty {
            c.source.unwrap()
        } else {
            let source = match c.source {
                Some(s) => s,
                None => {
                    // AddComponent<AudioSource>, then AddComponent<AudioSourceTimeScaleBehaviour>.
                    let id = self.allocate_instance_id();
                    self.fire_loop_add_source(c.at, id);
                    self.allocate_instance_id();
                    let ts = self.time.timeScale;
                    self.audio.add_time_scaled(id, ts, false);
                    self.fire_loop_chunks.get_mut(&key).unwrap().source = Some(id);
                    id
                }
            };
            self.fire_loop_chunks.get_mut(&key).unwrap().dirty = true;
            // gameObject.SetActive(true)
            self.audio.set_active(source, true);
            source
        };
        let a = self.audio.get_mut(source);
        // AudioSource.volume's setter clamps to [0, 1].
        if a.volume <= volume {
            a.volume = mathf::clamp01(mathf::lerp(a.volume, volume, weight));
        }
        if !self.audio.is_playing(source) {
            // A replay has no game assets (no clips, muted); the game always has Audio/fire_loop. A recording's act
            // can load the assets after the source was added without its clip.
            if self.audio.get(source).clip.is_none() && self.assets.is_some() {
                self.audio.get_mut(source).clip = self.resource_clip("Audio/fire_loop");
            }
            if let Some(clip) = self.audio.get(source).clip.clone() {
                self.audio.set_time(source, (d(c.random_start) * clip.length()).f());
            } else if self.assets.is_some() {
                crate::game::menu::throw("NullReferenceException: Audio/fire_loop");
            }
            self.audio.play(source);
        }
    }

    /// FireLoopChunkBehaviour.Update of every active (dirty) chunk: the volume falls by smoothDeltaTime until 0.05,
    /// then Stop, Dirty = false and SetActive(false).
    pub fn fire_loop_update(&mut self) {
        let smooth = self.time.smoothDeltaTime;
        for (key, c) in self.fire_loop_chunks.clone() {
            let (true, Some(s)) = (c.dirty, c.source) else { continue };
            let a = self.audio.get_mut(s);
            if a.volume > 0.05 {
                a.volume = mathf::clamp01((d(a.volume) - d(smooth) * 1.0f32).f());
                continue;
            }
            self.audio.stop(s);
            self.fire_loop_chunks.get_mut(&key).unwrap().dirty = false;
            self.audio.set_active(s, false);
        }
    }

    /// Resources.Load<AudioClip>(path) (None without the game's assets or such a clip).
    pub(super) fn resource_clip(&mut self, path: &str) -> Option<super::audio::ClipRef> {
        let db = self.assets.as_ref()?;
        let r = db.resource(path, 83).ok()??;
        super::audio::clip_ref(db, &mut self.audio.clip_cache, Some(r)).ok()?
    }

    /// PhysicalBehaviour.Ignite(bool)
    pub fn ignite(&mut self, p: usize, ignore_flammability: bool) {
        self.ignite_internal(p, ignore_flammability);
        if let Some(f) = self.physicals[p].fire_map.filter(|_| self.physicals[p].OnFire) {
            self.fire_map_ignite_random(f);
        }
    }

    /// PhysicalBehaviour.ClearAllDecals: DecalControllerBehaviour.Clear of each controller (localDecalPositions is emptied; the
    /// holder objects are rendering).
    #[allow(dead_code)]
    pub fn physical_clear_all_decals(&mut self, p: usize) {
        for c in &mut self.physicals[p].decal_controllers {
            // DecalControllerBehaviour.Clear: a dirty controller destroys its decalHolder and is clean again.
            c.decals.clear();
            c.holder = None;
        }
    }

    /// PhysicalBehaviour.ManagedUpdate
    pub fn physical_update(&mut self, p: usize) {
        let dt = self.time.deltaTime;
        let id = self.physicals[p].ids.audioSource;
        if self.physicals[p].audioSource && self.audio.sources.get(&id).is_some_and(|s| s.enabled) && !self.audio.is_playing(id) {
            self.audio.set_enabled(id, false);
        }
        let ph = &mut self.physicals[p];
        if ph.sizzleAudioHeat > 0.0 {
            ph.sizzleAudioHeat -= dt;
        }
        if ph.isStatic {
            return;
        }
        let burnrate = self.properties[ph.Properties].Burnrate;
        if ph.BurnProgress < 0.3 && ph.ChargeBurns {
            ph.BurnProgress = (d(ph.BurnProgress) + d(burnrate) * ph.Charge() * 0.01f32 * self.time.timeScale).f();
        }
        ph.Wetness = mathf::clamp01(ph.Wetness - dt);
        if ph.OnFire {
            ph.burnIntensity = (d(ph.burnIntensity) + d(dt) * 0.1f32).f();
            if ph.BurnProgress >= 0.995 {
                ph.OnFire = false;
            }
            ph.BurnProgress = mathf::clamp01(ph.BurnProgress);
            if ph.Wetness as f64 > 0.1 {
                self.extinguish(p);
                self.physicals[p].burnIntensity = 0.0;
            }
        } else {
            ph.burnIntensity = (d(ph.burnIntensity) - d(dt) * 0.1f32).f();
        }
        let ph = &mut self.physicals[p];
        ph.burnIntensity = mathf::clamp01(ph.burnIntensity);
        self.set_particle_emission(p);
        let ph = &mut self.physicals[p];
        ph.affectSurroundingTimer += dt;
        if ph.affectSurroundingTimer > 1.0 {
            ph.affectSurroundingTimer = 0.0;
            self.affect_surroundings(p);
            let ph = &mut self.physicals[p];
            if ph.Deletable && ph.rigidbody.bodyType == 0 {
                let position = ph.transform.position;
                if self.BoundingBox.contains_expanded([position[0], position[1]], 50.0) {
                    ph.outOfBoundsDuration = 0.0;
                } else {
                    ph.outOfBoundsDuration += 1.0;
                    if ph.outOfBoundsDuration >= 5.0 {
                        if ph.parent.is_some() {
                            if !ph.isDisintegrated && ph.Disintegratable {
                                self.physical_disintegrate(p);
                            }
                        } else {
                            // Object.Destroy(transform.root.gameObject): a physical without a parent is its root.
                            let id = ph.id;
                            self.destroy_object(id);
                        }
                    }
                }
            }
        }
        self.stab_update(p);
        self.handle_sliding_sounds(p);
    }

    /// PhysicalBehaviour.HandleSlidingSounds
    pub(crate) fn handle_sliding_sounds(&mut self, p: usize) {
        let id = self.physicals[p].ids.slidingAudioSource;
        if id == 0 || !self.audio.sources.contains_key(&id) {
            return;
        }
        let ph = &self.physicals[p];
        if ph.isStatic || ph.rigidbody.bodyType != 0 {
            self.audio.stop(id);
            self.audio.set_enabled(id, false);
        } else if ph.isSliding {
            if !self.audio.get(id).enabled {
                self.audio.set_enabled(id, true);
                self.audio.play(id);
            }
            if self.physicals[p].PlaySliderSound && self.audio.is_playing(id) {
                let target = if self.physicals[p].isSliding { self.physicals[p].slideIntensity } else { 0.0 };
                let s = self.audio.get_mut(id);
                s.volume = mathf::lerp(s.volume, target, 0.5);
            }
        } else if self.audio.get(id).enabled {
            self.audio.stop(id);
            self.audio.set_enabled(id, false);
        }
    }

    /// The Transform ids of a physical's object and its ancestors (GetComponentsInChildren from any finds its sources).
    pub(super) fn physical_ancestors(&self, p: usize) -> Vec<i32> {
        let mut out = vec![self.physicals[p].transform_id];
        let mut n = self.physicals[p].parent;
        while let Some(i) = n {
            out.push(self.transform_nodes[i].id);
            n = self.transform_nodes[i].parent;
        }
        out
    }

    /// PhysicalBehaviour.PlayClipOnce(clip, volume): the audioSource is enabled and plays the clip once (volume
    /// clamped to 0..1 with ClampVolume); nothing when disintegrated, without a clip or without an audioSource.
    pub fn play_clip_once(&mut self, p: usize, clip: Option<super::audio::ClipRef>, volume: f32) {
        if self.physicals[p].isDisintegrated || clip.is_none() || !self.physicals[p].audioSource {
            return;
        }
        let id = self.physicals[p].ids.audioSource;
        if !self.audio.sources.contains_key(&id) {
            return;
        }
        self.audio.set_enabled(id, true);
        let v = if self.prefs.ClampVolume { mathf::clamp01(volume) } else { volume };
        self.audio.play_one_shot(id, clip.as_ref(), v);
    }

    /// PhysicalBehaviour.Start's slidingAudioSource (instance id `id`, `ancestors` its object's): dopplerLevel 0, no playOnAwake,
    /// Linear from 0.1 to 20, volume 0, spatialBlend 1, looping, disabled, clip = Properties.SlidingLoop.
    pub(crate) fn physical_add_sliding_audio_source(&mut self, p: usize, id: i32, ancestors: Vec<i32>) {
        self.physicals[p].ids.slidingAudioSource = id;
        let clip = self.properties[self.physicals[p].Properties].clips.SlidingLoop.clone();
        let mut s = super::audio::AudioSource { anchor: super::BareAnchor::Physical(p), ancestors, doppler_level: 0.0, play_on_awake: false, rolloff_mode: 1, min_distance: 0.1, max_distance: 20.0, volume: 0.0, looping: true, enabled: false, clip, ..Default::default() };
        s.set_spatial_blend(1.0);
        self.audio.add(id, s);
    }

    /// PhysicalBehaviour.Start's audioSource (instance id `id`): dopplerLevel 0, no playOnAwake, Linear rolloff from
    /// 1 + SoundVolumeBoost to 30 + SoundVolumeBoost, spatialBlend 1, disabled. Returns the physical's ancestors.
    pub(crate) fn physical_add_audio_source(&mut self, p: usize, id: i32) -> Vec<i32> {
        self.physicals[p].ids.audioSource = id;
        self.physicals[p].audioSource = true;
        let ancestors = self.physical_ancestors(p);
        let boost = self.physicals[p].SoundVolumeBoost;
        let mut s = super::audio::AudioSource { anchor: super::BareAnchor::Physical(p), ancestors: ancestors.clone(), doppler_level: 0.0, play_on_awake: false, rolloff_mode: 1, min_distance: 1.0 + boost, max_distance: 30.0 + boost, enabled: false, ..Default::default() };
        s.set_spatial_blend(1.0);
        self.audio.add(id, s);
        ancestors
    }

    /// `(bool)MainAudioSource` then MainAudioSource.Stop().
    pub fn main_audio_stop(&mut self, p: usize) {
        let id = self.physicals[p].ids.audioSource;
        if self.physicals[p].audioSource && self.audio.sources.contains_key(&id) {
            self.audio.stop(id);
        }
    }

    /// `clips.PickRandom()` of a component's AudioClip[] field of length `n` (null when empty or a null element).
    #[track_caller]
    pub(super) fn pick_clip(&mut self, component: i32, field: &str, n: usize) -> Option<super::audio::ClipRef> {
        self.pick_random(n).and_then(|k| self.audio.clips(component, field).get(k).cloned().flatten())
    }

    pub(super) fn allocate_instance_ids(&mut self, count: usize) {
        for _ in 0..count {
            self.allocate_instance_id();
        }
    }

    /// PhysicalBehaviour.SetParticleEmission: only the flags and the fire emission's Random draws matter.
    pub(crate) fn set_particle_emission(&mut self, p: usize) {
        let flammability = self.properties[self.physicals[p].Properties].Flammability;
        let ph = &mut self.physicals[p];
        if ph.isStatic {
            return;
        }
        // `dirtyFire && (bool)FireParticleSystem`: a dirtyFire without its system (the fuzz) sets up the prefab again.
        if ph.dirtyFire && ph.has_fire_system {
            if let Some(f) = ph.fire_map.filter(|_| ph.burnIntensity > 0.01) {
                self.fire_map_emission(p, f);
            }
        } else if ph.burnIntensity > 0.05 {
            ph.dirtyFire = true;
            ph.has_fire_system = true;
            // PhysicalBehaviour.SetupFirePrefab: FireParticle's 19 objects and an Optout; "Falling embers" and "Rising embers" each have an
            // ActivateIfFancyEffects, whose Awake draws `Chance(1) > Random.value`.
            self.allocate_instance_ids(20);
            self.random.value();
            self.random.value();
        }
        let ph = &mut self.physicals[p];
        if !ph.dirtyBubble && ph.Wetness as f64 > 0.01 {
            self.physical_setup_bubble_prefab(p);
        }
        let ph = &mut self.physicals[p];
        if !ph.ForceNoCharge && !ph.ForceNoChargeParticles {
            if !ph.dirtyCharged && ph.Charge() > 0.001 {
                self.physical_setup_charged_prefab(p);
            }
            let ph = &mut self.physicals[p];
            if !ph.dirtySmoke && ph.Charge() * flammability > 0.001 {
                self.physical_setup_smoke_prefab(p);
            }
        }
    }

    /// PhysicalBehaviour.AffectSurroundings
    pub(crate) fn affect_surroundings(&mut self, p: usize) {
        let ph = &self.physicals[p];
        if ph.isStatic || (!ph.OnFire && !(ph.Charge() > 0.001)) {
            return;
        }
        for item in self.physics_objects_near(p) {
            let (me, other) = (&self.physicals[p], &self.physicals[item]);
            if other.IsUnderWater && me.IsUnderWater && me.Charge() > other.Charge() {
                self.physicals[item].charge = me.Charge();
            }
            if self.physicals[p].OnFire && !self.physicals[item].OnFire && self.random.value() > 0.7 {
                self.ignite(item, false);
            }
        }
    }

    /// Global.GetChunkNear
    pub(crate) fn chunk_near(&mut self, position: [f32; 3], radius: f32) -> (i32, i32) {
        let num = radius / 2.0;
        let c = self.random.inside_unit_circle();
        let v = [(position[0] + c[0] * num) * (2.0f32 / 3.0), (position[1] + c[1] * num) * (2.0f32 / 3.0)];
        // Mathf.RoundToInt: Math.Round, ties to even.
        ((v[0] as f64).round_ties_even() as i32, (v[1] as f64).round_ties_even() as i32)
    }

    /// Global.GetChunkOf
    pub(crate) fn chunk_of(&mut self, p: usize) -> (i32, i32) {
        let ph = &self.physicals[p];
        self.chunk_near(ph.transform.position, ph.ObjectArea / 2.0)
    }

    /// Global.UpdatePhysicalBehaviourChunks, when the chunks are stale.
    pub(crate) fn refresh_physical_chunks(&mut self) {
        if !self.chunksUpToDate {
            for chunk in self.physical_chunks.values_mut() {
                // PhysicalBehaviourChunk.Clear: Count = 0 (the slots are nulled; nothing reads past Count).
                chunk.clear();
            }
            for p in 0..self.physicals.len() {
                // PhysicalObjectsInWorld (Start adds, OnDestroy removes): the others draw nothing.
                let ph = &self.physicals[p];
                if ph.in_world && !ph.destroyed && !ph.isDisintegrated {
                    let key = self.chunk_of(p);
                    let chunk = self.physical_chunks.entry(key).or_insert_with(|| Vec::with_capacity(128));
                    // PhysicalBehaviourChunk.Add: only while Count < Array.Length (128).
                    if chunk.len() < 128 {
                        chunk.push(p);
                    }
                }
            }
            self.chunksUpToDate = true;
        }
    }

    /// Global.GetPhysicsObjectsNearPosition(position, radius): the chunk GetChunkNear picks (its draw included), the
    /// objects in it that are not disintegrated.
    pub(crate) fn physics_objects_near_position(&mut self, position: [f32; 2], radius: f32) -> Vec<usize> {
        self.refresh_physical_chunks();
        let key = self.chunk_near([position[0], position[1], 0.0], radius);
        let Some(chunk) = self.physical_chunks.get(&key) else { return Vec::new() };
        chunk.iter().copied().filter(|&o| !self.physicals[o].isDisintegrated).collect()
    }

    /// Global.GetPhysicsObjectsNearPositionAccurate(position, radius) at accuracy 1: for each cell of a CeilToInt(radius)
    /// square around the position, GetChunkNear(cell, 0) (one Random.insideUnitCircle draw each) and the chunk's objects
    /// that are not disintegrated, a chunk met again yielding its objects again. The iterator is lazy: `each` runs on an
    /// object before the next cell's draw, as a foreach body does. Nothing a caller does changes chunk membership, so
    /// a copy of the chunk is the live one.
    pub(super) fn physics_objects_near_position_accurate_each(&mut self, position: [f32; 2], radius: f32, each: impl FnMut(&mut Game, usize)) {
        self.physics_objects_near_position_accurate_at(position, radius, 1.0, each);
    }

    /// GetPhysicsObjectsNearPositionAccurate at any accuracy: below 0.999 a cell is skipped when Random.value > accuracy
    /// (drawn before its GetChunkNear).
    pub(crate) fn physics_objects_near_position_accurate_at(&mut self, position: [f32; 2], radius: f32, accuracy: f32, mut each: impl FnMut(&mut Game, usize)) {
        self.refresh_physical_chunks();
        let res = radius.ceil() as i32;
        let half = res as f32 * 0.5;
        for x in 0..res {
            for y in 0..res {
                if accuracy < 0.999 && self.random.value() > accuracy {
                    continue;
                }
                let v = [(d(position[0]) - half + x as f32).f(), (d(position[1]) - half + y as f32).f()];
                let key = self.chunk_near([v[0], v[1], 0.0], 0.0);
                let Some(chunk) = self.physical_chunks.get(&key).cloned() else { continue };
                for o in chunk {
                    if !self.physicals[o].isDisintegrated {
                        each(self, o);
                    }
                }
            }
        }
    }

    /// `physics_objects_near_position_accurate_each` for callers that draw nothing in the loop body.
    pub(super) fn physics_objects_near_position_accurate(&mut self, position: [f32; 2], radius: f32) -> Vec<usize> {
        let mut out = Vec::new();
        self.physics_objects_near_position_accurate_each(position, radius, |_, o| out.push(o));
        out
    }

    /// Global.GetPhysicsObjectsNear. The game's iterator reads the chunk live; nothing in
    /// AffectSurroundings changes chunk membership, so a copy is the same.
    pub(crate) fn physics_objects_near(&mut self, me: usize) -> Vec<usize> {
        self.refresh_physical_chunks();
        let key = self.chunk_of(me);
        let Some(chunk) = self.physical_chunks.get(&key) else { return Vec::new() };
        chunk.iter().copied().filter(|&o| o != me && !self.physicals[o].isDisintegrated).collect()
    }

    /// PhysicalBehaviour.ManagedLateUpdate
    pub fn physical_late_update(&mut self, p: usize) {
        // PhysicalBehaviour.GetCurrentContacts: [Obsolete] and called by nothing in Assembly-CSharp, so no caller exists to run it
        // PhysicalBehaviour.ResetColliderArray: only Utils.FixColliders calls it, and nothing calls FixColliders (the colliders list is built in Awake)
        if !self.physicals[p].isStatic {
            let v = self.physicals[p].rigidbody.velocity(&self.world);
            self.physicals[p].previousVelocity = v;
        }
    }

    /// PhysicalBehaviour.OnCollisionStayWithoutSleep: a sleeping body keeps the last buffer.
    pub(crate) fn on_collision_stay_without_sleep(&mut self, p: usize) {
        if self.physicals[p].isStatic || self.physicals[p].destroyed {
            self.physicals[p].collisionBuffer.clear();
            return;
        }
        let rb = &self.physicals[p].rigidbody;
        // Rigidbody2D::IsSleeping (180c202e0): a body that is not awake; no b2 body reads false.
        if rb.detached.is_none() && self.world.body(rb.body).flags & crate::b2::world::BODY_AWAKE == 0 {
            return;
        }
        let buffer = if rb.detached.is_none() { self.rigidbody_get_contacts(p) } else { Vec::new() };
        self.physicals[p].collisionBuffer = buffer;
        for i in 0..self.physicals[p].collisionBuffer.len() {
            let c = self.physicals[p].collisionBuffer[i];
            let Some(v) = c.rigidbody.and_then(|id| self.physical_of_rigidbody(id)) else { continue };
            if self.physicals[p].SimulateTemperature && self.physicals[v].SimulateTemperature {
                self.average_temperature(p, v, 0.01);
            }
            let (me, other) = (&self.physicals[p], &self.physicals[v]);
            if !me.ForceNoCharge
                && me.Charge() > other.Charge()
                && me.ConductOverride
                && other.ConductOverride
                && self.properties[other.Properties].Conducting
                && self.properties[me.Properties].Conducting
            {
                let c = mathf::lerp(other.Charge(), me.Charge(), 0.8);
                self.physicals[v].charge = c;
            }
        }
    }

    /// PhysicalBehaviour.ManagedFixedUpdate's scrape sparks: with FancyEffects, out of water, SparksOnSlide, a visible
    /// sprite, a Dynamic body and scrapeParticles, each buffered contact whose collider is a listed physical with
    /// Softness < 0.025 and relativeVelocity.sqrMagnitude > 9 emits (int)Clamp(sqr / 2, 1, 8) particles at its point with
    /// velocity Random.insideUnitCircle - relativeVelocity * 0.15. Only the draw reaches the simulation.
    fn physical_scrape_sparks(&mut self, p: usize) {
        let ph = &self.physicals[p];
        if !(self.prefs.FancyEffects && !ph.IsUnderWater && self.properties[ph.Properties].SparksOnSlide && ph.sprite_id != 0 && ph.isVisible && ph.rigidbody.bodyType == 0 && ph.scrapeParticles) {
            return;
        }
        for i in 0..self.physicals[p].collisionBuffer.len() {
            let c = self.physicals[p].collisionBuffer[i];
            if !self.collider_exists(c.collider) {
                continue;
            }
            let Some(v) = self.physical_of_collider(c.collider).filter(|&v| self.physicals[v].in_world) else { continue };
            if !(self.properties[self.physicals[v].Properties].Softness < 0.025) {
                continue;
            }
            let sq = mathf::sqr_magnitude([c.relative_velocity.x, c.relative_velocity.y]);
            if self.physicals[p].isVisible && sq > 9.0 {
                // scrapeParticles.Emit(emitParams, count): the MetalScrape clone's particles are not presented.
                self.random.inside_unit_circle();
            }
        }
    }

    /// PhysicalBehaviour.ShowOutline's setter: false unless the ShowOutlines setting and HasOutline allow it; then the
    /// outline object's SetActive.
    pub(crate) fn set_show_outline(&mut self, p: usize, value: bool) {
        let ph = &mut self.physicals[p];
        ph.showOutline = value && self.prefs.ShowOutlines && ph.HasOutline;
    }

    /// PhysicalBehaviour.Decal
    /// SendMessage("Decal", new DecalInstruction(type, position, colour, size)) to a physical object:
    /// PhysicalBehaviour.Decal adds a DecalControllerBehaviour for a new descriptor and calls it, then every
    /// DecalControllerBehaviour on the object receives the message too (the new one finds its decal: IsNearOtherDecal).
    pub fn physical_decal(&mut self, p: usize, ty: usize, position: [f32; 2], size: f32, colour: [f32; 4]) {
        let size = mathf::max(0.05, size);
        self.physical_behaviour_decal(p, ty, position, size, colour);
        for c in 0..self.physicals[p].decal_controllers.len() {
            self.decal_controller_decal(p, c, ty, position, size, colour);
        }
    }

    /// PhysicalBehaviour.Decal alone (the message's PhysicalBehaviour receiver): a new descriptor's DecalControllerBehaviour,
    /// added and given the decal.
    pub(crate) fn physical_behaviour_decal(&mut self, p: usize, ty: usize, position: [f32; 2], size: f32, colour: [f32; 4]) {
        let ph = &mut self.physicals[p];
        if ph.DisplayBloodDecals && self.prefs.Decals && ph.decal_controllers.iter().all(|c| c.descriptor != ty) {
            ph.decal_controllers.push(super::misc::DecalController { descriptor: ty, decals: Vec::new(), holder: None });
            // AddComponent<DecalControllerBehaviour>()
            self.allocate_instance_id();
            // DecalControllerBehaviour.Awake: dirty = false, the flag that rebuilds the decal mesh (only the mesh reads it).
            let c = self.physicals[p].decal_controllers.len() - 1;
            self.decal_controller_decal(p, c, ty, position, size, colour);
        }
    }

    /// The `DecalInstruction.type` of a limb's BloodDecal: `NULL_DECAL` for a null one.
    pub(crate) fn limb_blood_decal(&mut self, l: usize) -> usize {
        match self.limbs[l].BloodDecal.clone() {
            n if n.is_empty() => NULL_DECAL,
            n => self.decal_descriptor(&n),
        }
    }

    /// DecalControllerBehaviour.Start: `if (!DecalDescriptor)` Object.Destroy(this) (queued) and Debug.LogError, a log line.
    pub(crate) fn decal_controller_start(&mut self, p: usize, c: usize, id: i32) {
        if self.physicals[p].decal_controllers[c].descriptor == NULL_DECAL {
            self.tr_destroy_component(id, 0.0);
        }
    }

    /// DecalControllerBehaviour.Decal: the decal and its streaks are drawn by decals.rs; the decal's localRotation is
    /// Quaternion.LookRotation(forward, InverseTransformDirection(zero)), a zero up vector, so identity.
    pub(crate) fn decal_controller_decal(&mut self, p: usize, c: usize, ty: usize, position: [f32; 2], size: f32, colour: [f32; 4]) {
        if !self.prefs.Decals || self.physicals[p].decal_controllers[c].descriptor != ty {
            return;
        }
        // DecalControllerBehaviour.Start: a null DecalDescriptor destroys the controller and logs the error; CreateContainer makes nothing, so there is no decalHolder and Decal returns.
        if ty == NULL_DECAL {
            return;
        }
        if self.physicals[p].decal_controllers[c].holder.is_none() {
            // DecalControllerBehaviour.CreateContainer: no child is named "<descriptor> container" (the descriptors are Blast mark, Dense blood
            // and Sparse blood), so new GameObject (GameObject, Transform), then Optout, SortingGroup and SpriteMask.
            for _ in 0..5 {
                self.allocate_instance_id();
            }
            // GetComponent<SpriteRenderer>() (a null one throws); the SortingGroup takes its layer and order + 1, the SpriteMask its
            // sprite and order + 1 (decals.rs draws them).
            // A recording-built object (dump.rs) keeps no sprite, but the game's object had its SpriteRenderer (the
            // recording logs no exception), so only the mask's inputs are missing there.
            let ph = &self.physicals[p];
            let holder = (ph.sprite.map(|s| s.0), ph.sprite_layer, ph.sprite.map_or(0, |s| s.2));
            self.physicals[p].decal_controllers[c].holder = Some(holder);
        }
        let local = self.inverse_transform_point(p, [position[0], position[1], 0.0]);
        let local = [local[0], local[1]];
        let ty = self.decal_descriptors[ty].clone();
        if self.decal_is_near_other_decal(p, c, ty.IgnoreRadius, local) {
            return;
        }
        // new GameObject("decal") (GameObject, Transform) and its SpriteRenderer.
        for _ in 0..3 {
            self.allocate_instance_id();
        }
        let size = size * self.random.range(1.0, 1.2);
        // localScale = decalHolder.transform.InverseTransformVector(num * Vector3.one)
        let (a, b) = (self.inverse_transform_point(p, [size, size, size]), self.inverse_transform_point(p, [0.0; 3]));
        let scale = [a[0] - b[0], a[1] - b[1]];
        let at = self.physicals[p].decal_controllers[c].decals.len();
        let mut streaks = Vec::new();
        // Sprites.PickRandom(); Color times the instruction's multiplier.
        let sprite = self.pick_random(ty.Sprites).unwrap();
        let mut colour = [ty.Color[0] * colour[0], ty.Color[1] * colour[1], ty.Color[2] * colour[2], ty.Color[3] * colour[3]];
        if self.prefs.GorelessMode {
            colour = super::circulation::change_red_to_orange(colour);
        }
        if ty.will_drip() {
            let (lo, hi) = (ty.MaxDripCount.min(ty.MinDripCount), ty.MaxDripCount.max(ty.MinDripCount));
            // The loop condition draws Random.Range(minInclusive, maxExclusive) again on every pass.
            let mut i = 0;
            while i <= self.random.range_int(lo, hi) {
                // new GameObject("streak") (GameObject, Transform), SpriteRenderer, DecalDripStreakBehaviour.
                for _ in 0..4 {
                    self.allocate_instance_id();
                }
                let r = self.random.inside_unit_circle();
                let radius = self.random.range(0.02, 0.05);
                let position = [(d(r[0]) * size / 2.0).f(), (d(r[1]) * size / 2.0).f()];
                self.pending_starts.push(super::misc::PendingStart::DecalDripStreak(p, c, at, streaks.len()));
                streaks.push(super::misc::Streak { position, scale: [radius, radius], radius, ..Default::default() });
                i += 1;
            }
        }
        self.physicals[p].decal_controllers[c].decals.push(super::misc::Decal { position: local, size, sprite, colour, scale, streaks });
    }

    /// DecalControllerBehaviour.IsNearOtherDecal: some localDecalPosition (TransformPoint-ed) lies within DecalDescriptor.IgnoreRadius
    /// of the TransformPoint-ed localPosition.
    pub(crate) fn decal_is_near_other_decal(&self, p: usize, c: usize, ignore_radius: f32, local: [f32; 2]) -> bool {
        let radius = ignore_radius * ignore_radius;
        let at = self.transform_point3(p, [local[0], local[1], 0.0]);
        self.physicals[p].decal_controllers[c].decals.iter().any(|decal| {
            let o = self.transform_point3(p, [decal.position[0], decal.position[1], 0.0]);
            let v = [at[0] - o[0], at[1] - o[1], at[2] - o[2]];
            (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).f() < radius
        })
    }

    /// spriteRenderer.bounds: the sprite's local AABB through the object's transforms (center, extents).
    pub fn sprite_world_bounds(&self, p: usize) -> ([f32; 3], [f32; 3]) {
        let Some((c, e)) = self.physicals[p].sprite_bounds else {
            unimplemented!("mod-only: SpriteRenderer.bounds of {}: the sprite's local bounds are not loaded", self.physicals[p].name)
        };
        let chain: Vec<_> = self.local_chain(p).into_iter().rev().collect();
        crate::unity::transform::transform_aabb(c, e, &chain)
    }

    /// PhysicalBehaviour.CreateImpactEffect
    pub fn create_impact_effect(&mut self, p: usize, position: [f32; 2], direction: [f32; 2], size: f32) {
        if mathf::sqr_magnitude(direction) <= f32::EPSILON || size.is_nan() {
            return;
        }
        let z = (d(crate::game::mathf::atan2(direction[1], direction[0])) * 57.29578f32 - 90.0f32).f();
        let props = &self.properties[self.physicals[p].Properties];
        let Some((source, blood_puff)) = props.ShotImpact else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        if blood_puff {
            let Some((b, color)) = self.create_blood_puff(p, position, z, size) else { return };
            // The puff returns to the pool when its particle systems stop: a recording's ShotImpact needs the prefab.
            let i = self.physicals[p].Properties;
            if source.is_none() {
                self.recorded_shot_impact(i);
            }
            let source = self.properties[i].ShotImpact.and_then(|s| s.0).expect("the BloodPuff poolable prefab");
            self.log_effect(source, [position[0], position[1], 0.0], z_rotation(z), size, color, None);
            self.tag_pooled(super::Pooled::BloodPuff(b));
            if !self.effects_off {
                self.effect_log.last_mut().expect("the clone's effect").off = self.blood_puffs[b].off.clone();
            }
            return;
        }
        // The rotation, the scale and OnImpactCreated (only a BloodImpactBehaviour on the clone has a receiver) are
        // presentation for these.
        // A recording names the ShotImpact only: the prefab from the game's data.
        let i = self.physicals[p].Properties;
        if source.is_none() {
            self.recorded_shot_impact(i);
        }
        let source = self.properties[i].ShotImpact.and_then(|s| s.0).expect("CreateImpactEffect: the ShotImpact prefab");
        let pool = self.effect_pool(source, "PhysicalProperties.ShotImpact");
        if self.splash_pools[pool].poolable {
            self.request_pooled_effect(source, "PhysicalBehaviour.CreateImpactEffect", position, z, size);
        } else {
            // Object.Instantiate(ShotImpact, position, identity)
            let colliding = self.splash_pools[pool].prefab.objects.iter().any(|ob| ob.components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Collider2D(_) | crate::assets::db::Component::Rigidbody2D(_))));
            if colliding {
                unimplemented!("mod-only: PhysicalBehaviour.CreateImpactEffect: a ShotImpact with physics (its scale and rotation)");
            }
            self.spawn_prefab_scaled(source, position, z, size);
        }
    }

    /// WaterImpactBehaviour.Shot
    pub(super) fn water_impact_shot(&mut self, splash: crate::assets::db::ObjRef, normal: [f32; 2], point: [f32; 2], damage: f32) {
        const WHAT: &str = "WaterImpactBehaviour.Shot";
        let pool = self.effect_pool(splash, WHAT);
        // transform.up = Vector2.Reflect((normal.x, -|normal.y|), up): the particles' only.
        let z = (d(crate::game::mathf::atan2(normal[1].abs(), normal[0])) * 57.29578f32 - 90.0f32).f();
        if self.splash_pools[pool].poolable {
            if !self.request_pooled_effect(splash, WHAT, point, z, 1.0) {
                return;
            }
        } else {
            self.spawn_prefab_scaled(splash, point, z, 1.0);
        }
        let speed = (d((damage as f64).powf(0.5) as f32) * 0.9f32).f();
        // GetComponentsInChildren: the active objects, parent first.
        let prefab = self.splash_pools[pool].prefab.clone();
        let mut active = Vec::with_capacity(prefab.objects.len());
        let mut sizes = Vec::new();
        for ob in &prefab.objects {
            active.push(ob.active && ob.parent.is_none_or(|p| active[p]));
            if active[active.len() - 1] && ob.components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Other(198))) {
                sizes.push((ob.game_object, (d(0.15f32) * self.random.range(1.0, 1.4)).f()));
            }
        }
        if !self.effects_off {
            self.effect_log.last_mut().expect("the clone's effect").water = Some(super::WaterSplash { radius: 0.01, speed, sizes });
        }
    }

    /// The splash pool of an effect prefab, read from the data the first time.
    fn effect_pool(&mut self, source: crate::assets::db::ObjRef, what: &str) -> usize {
        let mut db = self.assets.take().unwrap_or_else(|| panic!("{what} without the game's data"));
        let pool = self.splash_pool_for(&mut db, source);
        self.assets = Some(db);
        pool.unwrap_or_else(|e| panic!("{what}: {e}"))
    }

    /// PoolGenerator.RequestPrefab(originalPrefab, position): Ensure(originalPrefab).Request(position), by prefab: a
    /// BloodPuff poolable ShotImpact takes the blood puff pool, any other poolable effect its splash pool. The clone stands at
    /// `position` with its prefab's rotation; false when the full pool hands out nothing.
    pub(crate) fn pool_generator_request_prefab(&mut self, prefab: crate::assets::db::ObjRef, position: [f32; 2]) -> bool {
        let pooled = if self.properties.iter().any(|p| p.ShotImpact == Some((Some(prefab), true))) {
            let Some(b) = self.pool_request_blood_puff(position) else { return false };
            super::Pooled::BloodPuff(b)
        } else {
            let pool = self.effect_pool(prefab, "PoolGenerator.RequestPrefab");
            let Some(item) = self.splash_request(pool) else { return false };
            super::Pooled::Splash(pool, item)
        };
        self.log_effect(prefab, [position[0], position[1], 0.0], [0.0, 0.0, 0.0, 1.0], 1.0, None, None);
        self.tag_pooled(pooled);
        true
    }

    /// A caller's RequestPrefab(effect, position) of a particle effect: the position and rotation only place
    /// particles. `what` names the caller.
    pub(super) fn request_pooled_effect(&mut self, source: crate::assets::db::ObjRef, what: &str, position: [f32; 2], z: f32, scale: f32) -> bool {
        let pool = self.effect_pool(source, what);
        if !self.splash_pools[pool].poolable {
            unimplemented!("mod-only: {what}: a pooled effect not tagged Poolable (its particle lifetime)");
        }
        for ob in &self.splash_pools[pool].prefab.objects {
            for (_, c) in &ob.components {
                if let crate::assets::db::Component::Script { class, .. } = c {
                    if !matches!(class.as_str(), "ExplosionSoundBehviour" | "PoolableParticleSystemHelper" | "PoolableSizeHelper" | "ActivateIfFancyEffects" | "Optout" | "AudioSourceTimeScaleBehaviour") {
                        unimplemented!("mod-only: {what}: a pooled effect with a {class}");
                    }
                }
            }
        }
        // A full pool hands out nothing.
        let Some(item) = self.splash_request(pool) else { return false };
        self.log_effect(source, [position[0], position[1], 0.0], z_rotation(z), scale, None, None);
        self.tag_pooled(super::Pooled::Splash(pool, item));
        true
    }

    /// Instantiate(prefab, position, identity), with the rotation and scale the caller gives it afterwards: both are
    /// presentation here.
    fn spawn_prefab_scaled(&mut self, source: crate::assets::db::ObjRef, position: [f32; 2], z: f32, scale: f32) {
        let off = std::mem::replace(&mut self.effects_off, true);
        let root = self.spawn_prefab(source, [position[0], position[1], 0.0]);
        self.effects_off = off;
        self.log_effect(source, [position[0], position[1], 0.0], z_rotation(z), scale, None, Some(root));
    }

    /// PhysicalBehaviour.SetupSpawnPrefab: Instantiate(Resources.Load("Prefabs/SpawnParticles"), transform.position,
    /// identity, transform), AddComponent<Optout>(), shape.spriteRenderer = GetComponent<SpriteRenderer>(). The clone is a
    /// particle effect over the sprite whose DeleteAfterTime (Life 1) destroys it after a second; it draws no
    /// UnityEngine.Random (the grid jitter after it matches the game without it). Optout keeps it out of saves.
    pub(crate) fn physical_setup_spawn_prefab(&mut self, p: usize) {
        if !self.physicals[p].SpawnSpawnParticles || self.physicals[p].isStatic {
            return;
        }
        // GameObject, Transform, ParticleSystem, ParticleSystemRenderer, DeleteAfterTime, then the Optout.
        let go = self.next_instance_id;
        let t = go - 2;
        self.allocate_instance_ids(SPAWN_PARTICLES_OBJECTS + 1);
        let parent = self.physicals[p].transform_id;
        self.add_runtime_clone(parent, &[(t, None)]);
        // The prefab's DeleteAfterTime (enabled, Life 1): Update from the next frame.
        let dat = go - 8;
        self.delete_afters.push(super::explosion::DeleteAfter { id: dat, added_to: Some(t), parent: Some(parent), ..Default::default() });
        let b = super::Behaviour::DeleteAfter(self.delete_afters.len() - 1);
        self.behaviour_of.insert(dat, (0, b));
        self.behaviours.add(0, b);
        // The particle system (playOnAwake) at the physical's position with world rotation identity, its root a child of
        // the physical's Transform from here on.
        let Some(source) = self.assets.as_ref().and_then(|db| db.resource("Prefabs/SpawnParticles", 1).ok().flatten()) else { return };
        let (position, q, _) = self.anchored_world(super::BareAnchor::Physical(p), &[]);
        self.log_effect(source, position, [0.0, 0.0, 0.0, 1.0], 1.0, None, Some(t));
        if !self.effects_off {
            let rotation = [-q[0], -q[1], -q[2], q[3]];
            self.effect_log.last_mut().expect("the clone's effect").parent = Some(super::EffectParent { physical: p, position: [0.0; 3], rotation, sprite_shape: true });
        }
    }

    /// PhysicalBehaviour.SetupBubblePrefab: dirtyBubble, BubbleVFX's 5 objects and an Optout (particles only).
    pub(crate) fn physical_setup_bubble_prefab(&mut self, p: usize) {
        self.physicals[p].dirtyBubble = true;
        // Instantiate(BubbleParticlePrefab, transform): BubbleVFX's one object (Transform, ParticleSystem, its renderer,
        // SortingGroup, Optout) under the physical's Transform.
        let t = self.next_instance_id - 2;
        self.allocate_instance_ids(6);
        self.add_runtime_clone(self.physicals[p].transform_id, &[(t, None)]);
    }

    /// PhysicalBehaviour.SetupChargedPrefab: dirtyCharged, ChargedParticles's 8 objects and an Optout (particles only).
    pub(crate) fn physical_setup_charged_prefab(&mut self, p: usize) {
        self.physicals[p].dirtyCharged = true;
        self.allocate_instance_ids(9);
    }

    /// PhysicalBehaviour.SetupSmokePrefab: dirtySmoke, SmokeParticle's 4 objects and an Optout (particles only).
    pub(crate) fn physical_setup_smoke_prefab(&mut self, p: usize) {
        self.physicals[p].dirtySmoke = true;
        self.allocate_instance_ids(5);
    }

    /// Transform.TransformPoint of a 2D point (z 0) on a physical object's transform.
    pub fn transform_point(&self, p: usize, local: [f32; 2]) -> [f32; 2] {
        let v = self.transform_point3(p, [local[0], local[1], 0.0]);
        [v[0], v[1]]
    }
}

/// Collision2D.GetContacts(buffer): copies min(count, buffer length) contacts, stale entries stay.
pub fn get_contacts(c: &ScriptCollision, buffer: &mut [ContactPoint2D; 8]) -> usize {
    let n = c.contacts.len().min(buffer.len());
    buffer[..n].copy_from_slice(&c.contacts[..n]);
    n
}

/// Utils.GetFirstValidContact
pub fn get_first_valid_contact(buffer: &[ContactPoint2D; 8], count: usize) -> ContactPoint2D {
    for i in 0..count {
        let result = buffer[i];
        if !result.normal_impulse.is_nan() && !result.normal_impulse.is_infinite() {
            return result;
        }
    }
    buffer[0]
}

/// Utils.GetAverageImpulseRemoveOutliers (outlierThreshold = 1)
pub fn get_average_impulse_remove_outliers(buffer: &[ContactPoint2D; 8], count: usize) -> f32 {
    // No NaN impulse reaches num, so a NaN answer is one this made (0 / 0, inf - inf): x86's default NaN 0xffc00000, which
    // ARM gives as 0x7fc00000.
    let x86 = |r: f32| if r.is_nan() { f32::from_bits(0xffc0_0000) } else { r };
    let outlier_threshold = 1.0f32;
    let mut num = 0.0f32;
    let mut num2 = 0i32;
    for i in 0..count {
        let normal_impulse = buffer[i].normal_impulse;
        if !normal_impulse.is_nan() && !normal_impulse.is_infinite() {
            num += normal_impulse;
            num2 += 1;
        }
    }
    let num3 = num / num2 as f32;
    if count >= 2 && outlier_threshold > mathf::EPSILON {
        for j in 0..count - 1 {
            let normal_impulse2 = buffer[j].normal_impulse;
            if (normal_impulse2 - num3).abs() > outlier_threshold {
                num -= normal_impulse2;
                num2 -= 1;
            }
        }
        return x86(num / num2 as f32);
    }
    x86(num3)
}

/// Utils.GetMinImpulse
pub fn get_min_impulse(buffer: &[ContactPoint2D; 8], count: usize) -> f32 {
    let mut num = f32::MAX;
    for i in 0..count {
        let normal_impulse = buffer[i].normal_impulse;
        if !normal_impulse.is_nan() {
            num = mathf::min(num, normal_impulse);
        }
    }
    num
}

/// Vector2.Dot (rounded at its return).
pub fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (d(a[0]) * b[0] + d(a[1]) * b[1]).f()
}

/// Utils.HasLayer: ((int)mask & (1 << layer)) != 0, with the shift count taken mod 32 as C# does.
pub fn has_layer(mask: u32, layer: i32) -> bool {
    (mask & 1u32.wrapping_shl(layer as u32)) != 0
}

/// Utils.NaNFallback(float): fallback when f is NaN, else f.
pub fn nan_fallback(f: f32, fallback: f32) -> f32 {
    if f.is_nan() { fallback } else { f }
}

impl Game {
    /// Global.PhysicalObjectsInWorldByTransform.TryGetValue(collision.transform): None for a collider
    /// without a PhysicalBehaviour (and for the empty `new Collision2D()`).
    pub fn collision_physical(&self, c: &ScriptCollision) -> Option<usize> {
        if c.collider == 0 { None } else { self.physical_of_collider(c.collider) }
    }

    /// Rigidbody2D.velocity of the rigidbody with this instance id.
    pub fn rigidbody_velocity_by_id(&self, id: i32) -> [f32; 2] {
        for b in self.world.bodies.iter().flatten() {
            if b.user.map(|u| u.id) == Some(id) {
                return if b.kind == BodyType::Static { [0.0, 0.0] } else { [b.linear_velocity.x, b.linear_velocity.y] };
            }
        }
        [0.0, 0.0]
    }

    /// Transform.TransformVector for a 2D vector of a physical object's transform.
    pub(super) fn transform_vector(&self, p: usize, local: [f32; 2]) -> [f32; 3] {
        let t = &self.physicals[p].transform;
        mathf::rotate(t.rotation, [local[0] * t.localScale[0], local[1] * t.localScale[1], 0.0])
    }

    // PhysicalBehaviour.TryGetFreeCollisionBufferIndex: [Obsolete], returns false and nothing calls it.

    /// PhysicalBehaviour.CalculateSliding
    pub(crate) fn calculate_sliding(&mut self, p: usize, c: &ScriptCollision) {
        if self.physicals[p].isStatic {
            return;
        }
        self.physicals[p].isSliding = false;
        let rb = self.physicals[p].rigidbody.clone();
        if rb.bodyType != 0 {
            return;
        }
        let v = rb.velocity(&self.world);
        let num = match c.rigidbody {
            None => mathf::magnitude2(v),
            Some(id) => {
                let o = self.rigidbody_velocity_by_id(id);
                mathf::magnitude2([v[0] - o[0], v[1] - o[1]])
            }
        };
        let num2 = rb.angular_velocity(&self.world).abs() * 0.025;
        if !(mathf::max(num, num2) <= 0.05) {
            let a = if num2 > 0.05 && num < 2.0 { num2 } else { 0.0 };
            let b = if num2 < 2.0 && num > 0.05 { num } else { 0.0 };
            self.physicals[p].slideIntensity = mathf::max(a, b) / 40.0;
            if !((self.physicals[p].slideIntensity as f64) < 0.1) {
                self.physicals[p].isSliding = true;
            }
        }
    }

    /// PhysicalBehaviour.OnCollisionStay2D
    pub fn physical_on_collision_stay(&mut self, p: usize, c: &ScriptCollision) {
        if self.physicals[p].isStatic || self.collision_physical(c) == Some(p) {
            return;
        }
        self.calculate_sliding(p, c);
        if self.physicals[p].SimulateTemperature {
            let num = 0.0001f32 * self.properties[self.physicals[p].Properties].friction.map_or(0.5, |f| f * 2.0);
            let sq = mathf::sqr_magnitude([c.relative_velocity.x, c.relative_velocity.y]);
            let ph = &mut self.physicals[p];
            ph.Temperature = (d(ph.Temperature) + d(sq) * num).f();
        }
    }

    /// PhysicalBehaviour.OnCollisionExit2D
    pub fn physical_on_collision_exit(&mut self, p: usize, _c: &ScriptCollision) {
        if self.physicals[p].isStatic {
            return;
        }
        self.physicals[p].isSliding = false;
        let id = self.physicals[p].ids.slidingAudioSource;
        if self.physicals[p].PlaySliderSound && id != 0 && self.audio.sources.contains_key(&id) {
            self.audio.stop(id);
        }
    }

    /// PhysicalBehaviour.OnCollisionEnter2D
    pub fn physical_on_collision_enter(&mut self, p: usize, c: &ScriptCollision) {
        if self.physicals[p].isStatic {
            return;
        }
        let mut buffer = self.physical_contact_buffer;
        let contacts = get_contacts(c, &mut buffer);
        self.physical_contact_buffer = buffer;
        if contacts == 0 {
            return;
        }
        let first_valid_contact = get_first_valid_contact(&buffer, contacts);
        let average_impulse = get_average_impulse_remove_outliers(&buffer, contacts);
        let at = self.physicals[p].transform.position;
        self.camera_shake((d(average_impulse) * 0.01f32).f(), [at[0], at[1]]);
        self.handle_sounds(p, average_impulse);
        self.handle_stabbing(p, c, &first_valid_contact);
        if let Some(v) = self.collision_physical(c) {
            let props = &self.properties[self.physicals[p].Properties];
            let (me, other) = (&self.physicals[p], &self.physicals[v]);
            if props.SizzleSounds.is_some()
                && other.SimulateTemperature
                && me.SimulateTemperature
                && (other.Temperature - me.Temperature).abs() > 10.0
                && other.Temperature > me.Temperature
                && other.Temperature >= props.BurningTemperatureThreshold
            {
                self.sizzle(p, true);
            }
            // The draw comes first in the original's condition.
            let roll = self.random.value();
            let props = &self.properties[self.physicals[p].Properties];
            let (me, other) = (&self.physicals[p], &self.physicals[v]);
            let conducting = (props.Conducting, self.properties[other.Properties].Conducting);
            if !(roll > 0.5)
                && me.Charge() > other.Charge()
                && me.ConductOverride
                && other.ConductOverride
                && conducting.1
                && conducting.0
                && (me.Charge() - other.Charge()).abs() > 15.0
            {
                let diff = (me.Charge() - other.Charge()).abs();
                let from = me.transform.position;
                let to = other.transform.position;
                let direction = super::person::normalized([to[0] - from[0], to[1] - from[1], to[2] - from[2]]);
                let k = 0.2f32 * diff;
                let impulse = [k * direction[0], k * direction[1]];
                let charge = me.Charge();
                let rb = other.rigidbody.clone();
                rb.add_force_impulse(&mut self.world, impulse);
                self.physicals[v].charge = charge;
                // Instantiate(Prefabs/BigZap): effect prefab only.
            }
        }
    }

    /// PhysicalBehaviour.Sizzle
    pub fn sizzle(&mut self, p: usize, with_sound: bool) {
        if !self.physicals[p].dirtySmoke {
            self.physical_setup_smoke_prefab(p);
        }
        // SmokeParticleSystem.Emit(5)
        if !self.effects_off {
            self.smoke_emits.push((p, 5));
        }
        if with_sound {
            if let Some(n) = self.properties[self.physicals[p].Properties].SizzleSounds {
                if n != 0 && self.physicals[p].sizzleAudioHeat <= mathf::EPSILON {
                    // PlayClipOnce(SizzleSounds.PickRandom(), 0.5f)
                    let k = self.pick_random(n).unwrap();
                    let clip = self.properties[self.physicals[p].Properties].clips.SizzleSounds.get(k).cloned().flatten();
                    self.play_clip_once(p, clip, 0.5);
                    self.physicals[p].sizzleAudioHeat = 1.0;
                }
            }
        }
    }

    /// PhysicalBehaviour.HandleSounds
    pub(crate) fn handle_sounds(&mut self, p: usize, impulse: f32) {
        if !self.physicals[p].audioSource {
            return;
        }
        let props = &self.properties[self.physicals[p].Properties];
        let num = impulse * props.ImpactIntensityMutliplier;
        if num < 0.002 {
            return;
        }
        let num2 = mathf::max(1.0, self.physicals[p].rigidbody.mass(&self.world));
        let flag = num > 0.5 / num2;
        let mut num3 = (d(num) * 0.3f32 / num2 * props.HitVolumeMultiplier).f();
        if self.prefs.ClampVolume {
            num3 = mathf::clamp01(num3);
        }
        let (hard, soft) = (props.HardImpact, props.SoftImpact);
        if self.physicals[p].OverrideImpactSounds != 0 {
            let clip = self.pick_clip(self.physicals[p].id, "OverrideImpactSounds", self.physicals[p].OverrideImpactSounds);
            self.play_clip_once(p, clip, num3);
        } else if flag {
            if hard != 0 {
                let k = self.pick_random(hard).unwrap();
                let clip = self.properties[self.physicals[p].Properties].clips.HardImpact.get(k).cloned().flatten();
                self.play_clip_once(p, clip, num3);
            }
        } else if soft != 0 {
            let k = self.pick_random(soft).unwrap();
            let clip = self.properties[self.physicals[p].Properties].clips.SoftImpact.get(k).cloned().flatten();
            self.play_clip_once(p, clip, num3);
        }
    }

    /// PhysicalBehaviour.IsPointNearLooseSharpEnd
    pub(crate) fn is_point_near_loose_sharp_end(&self, p: usize, point: [f32; 2]) -> bool {
        let position = self.physicals[p].transform.position;
        let loose = self.physicals[p].LooseEndSize;
        let end = |axis: &SharpAxis, t: f32| {
            let v = self.transform_vector(p, axis.Axis);
            let b = [position[0] + v[0], position[1] + v[1]];
            // Vector2.LerpUnclamped
            let l = [(d(position[0]) + (d(b[0]) - position[0]) * t).f(), (d(position[1]) + (d(b[1]) - position[1]) * t).f()];
            mathf::sqr_magnitude([l[0] - point[0], l[1] - point[1]])
        };
        for axis in &self.properties[self.physicals[p].Properties].SharpAxes {
            if axis.LooseLowerLimit && end(axis, axis.LowerLimit) < loose * loose {
                return true;
            }
            if axis.LooseUpperLimit && end(axis, axis.UpperLimit) < loose * loose {
                return true;
            }
        }
        false
    }

    /// PhysicalBehaviour.HandleStabbing
    pub(super) fn handle_stabbing(&mut self, p: usize, c: &ScriptCollision, contact: &ContactPoint2D) {
        let props = &self.properties[self.physicals[p].Properties];
        if !props.Sharp || self.physicals[p].penetrations.len() as i32 >= self.physicals[p].MaximumStabVictims {
            return;
        }
        let Some(v) = self.collision_physical(c) else { return };
        let victim = &self.properties[self.physicals[v].Properties];
        let relative_velocity = [c.relative_velocity.x, c.relative_velocity.y];
        if (victim.Softness <= 0.01 && !props.IgnoreStabResistance)
            || (victim.IgnoreStabResistance && props.IgnoreStabResistance)
            || mathf::magnitude2(relative_velocity) / props.SharpForceThresholdMultiplier
                < mathf::map_range(0.01, 1.0, 20.0, 1.25, if props.IgnoreStabResistance { 1.0 } else { mathf::sqrt(victim.Softness) })
            || !self.is_point_near_loose_sharp_end(p, [contact.point.x, contact.point.y])
        {
            return;
        }
        let normalized = mathf::normalized2(relative_velocity);
        let contact_normal = [contact.normal.x, contact.normal.y];
        if dot2(contact_normal, normalized) < -0.98 {
            return;
        }
        let hit = props.SharpAxes.iter().copied().find(|axis| {
            let vector = self.transform_vector(p, axis.Axis);
            let vector = [vector[0], vector[1]];
            dot2(normalized, vector) < -0.92 && dot2(contact_normal, vector) < -0.3
        });
        if let Some(axis) = hit {
            self.stab_begin(p, v, axis, c, contact);
        }
    }

    /// PhysicalBehaviour.Disintegrate
    pub fn physical_disintegrate(&mut self, p: usize) {
        let ph = &self.physicals[p];
        if ph.isStatic || !ph.Disintegratable || !ph.Deletable {
            return;
        }
        self.stab_disintegrate(p);
        // PhysicalBehaviour.add_OnDisintegration: the event's subscriptions are the handlers below.
        // PhysicalBehaviour.remove_OnDisintegration: unsubscription on destroy, same handlers.
        // OnDisintegration: the limb's handler, then the person's DisintegrationCounterBehaviour (the other
        // subscriber, LimbStatusBehaviour, only hides the status UI).
        if let Some(l) = self.limbs.iter().position(|limb| limb.PhysicalBehaviour == p) {
            if self.limbs[l].started {
                self.limb_on_disintegration(l);
            }
            // DisintegrationCounterBehaviour.Start (on the root, so before the limbs' Starts) subscribed every child.
            self.person_on_child_disintegrated(self.limbs[l].Person);
        }
        // A gore shard registered as a pseudo child of a person's DisintegrationCounterBehaviour.
        for person in (0..self.people.len()).filter(|&i| self.people[i].pseudo_children.contains(&p)).collect::<Vec<_>>() {
            self.person_on_child_disintegrated(person);
        }
        // The subscriptions a later DisintegrationCounterBehaviour.Start added (after every first-Start one).
        for person in 0..self.people.len() {
            for _ in 0..self.people[person].counter_subs.iter().filter(|&&c| c == p).count() {
                self.person_on_child_disintegrated(person);
            }
        }
        self.disintegration_counters_on_disintegration(p);
        self.act_on_disintegrate_run(p);
        // GetComponentsInChildren<Collider2D>().enabled = false: the object's own collider.
        self.collider_disable(p);
        // GetComponentsInChildren<Renderer>().enabled = false: the SpriteRenderers on the active objects below, depth first.
        self.disintegrate_disable_sprites(p);
        // Rigidbody2D.simulated = false (Rigidbody2D::SetSimulated -> b2Body::SetActive(false)). Limbs have no
        // BloodTankBehaviour.
        let body = self.physicals[p].rigidbody.body;
        self.world.deactivate_body(body);
        self.physicals[p].rigidbody.simulated = false;
        // SetActive(false) deactivates the clones parented under it too (their Renderers are presentation).
        if !self.attached_clones.is_empty() {
            let id = self.physicals[p].transform_id;
            self.with_component_instance(id, |g, inst, o| g.attached_set_active(inst, o, false));
        }
        self.deactivate(p);
        // gameObject.SetActive(false): the clone object's activeSelf (gapB r13: limbs too, not only shards; a crushed
        // limb's activeSelf stayed 1). The shard's body, shapes and joints are gone: a later Destroy finds nothing to deactivate.
        let id = self.physicals[p].transform_id;
        if let Some(inst) = self.unsynced.iter_mut().chain(self.retained.iter_mut()).find(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            let o = inst.objects.iter().position(|x| x.components.contains(&id)).unwrap();
            inst.objects[o].self_active = false;
            for x in std::iter::once(o).chain(inst.descendants(o)) {
                inst.objects[x].active = false;
            }
            if inst.prefab.objects[o].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "ShatteredShardBehaviour")) {
                inst.bodies.remove(&o);
                inst.fixtures.retain(|&(x, _), _| x != o);
            }
        }
        self.physicals[p].isDisintegrated = true;
        // InvokeOnItemRemoved
        self.physical_invoke_on_item_removed(p);
    }

    /// PhysicalBehaviour.Disintegrate's `GetComponentsInChildren<Renderer>()` loop, `enabled = false` on each: the
    /// SpriteRenderers of the object's clone, its active objects depth first; a physical on no clone, its own sprite.
    fn disintegrate_disable_sprites(&mut self, p: usize) {
        let id = self.physicals[p].transform_id;
        let on_clone = self.unsynced.iter().chain(self.retained.iter()).any(|i| i.objects.iter().any(|x| x.components.contains(&id)));
        let sprites: Vec<i32> = if on_clone {
            self.with_component_instance(id, |_, inst, o| {
                fn collect(inst: &super::spawn::Instance, x: usize, out: &mut Vec<i32>) {
                    if !inst.objects[x].active {
                        return;
                    }
                    for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                        if matches!(c, crate::assets::db::Component::SpriteRenderer(_)) {
                            out.push(inst.objects[x].components[k]);
                        }
                    }
                    for &child in &inst.prefab.objects[x].children {
                        collect(inst, child, out);
                    }
                }
                let mut out = Vec::new();
                collect(inst, o, &mut out);
                out
            })
        } else {
            vec![self.physicals[p].sprite_id]
        };
        for s in sprites {
            if let Some(at) = self.item_sprite(s) {
                self.set_sprite_enabled(at, false);
            }
        }
    }

    /// Collider2D::SetEnabled(false) (180c15670), for each collider in turn: Collider2D::Cleanup (180c0d940) with
    /// ColliderCleanupOperation 2 destroys the shapes last first, resets the body's mass and, with
    /// Physics2D.callbacksOnDisable off, drops the collider's contact records without callbacks.
    pub(super) fn collider_disable(&mut self, p: usize) {
        let body = self.physicals[p].rigidbody.body;
        for collider in self.physicals[p].all_colliders.clone() {
            self.collider_disable_one(collider, body);
        }
    }

    /// Collider2D::SetEnabled(false) of one collider whose fixtures sit on `body`.
    pub(super) fn collider_disable_one(&mut self, collider: i32, body: crate::b2::world::BodyId) {
        let shapes: Vec<_> = self.world.body(body).fixtures.iter().copied().filter(|&f| self.world.fixture(f).collider.id == collider).collect();
        for &f in shapes.iter().rev() {
            self.world.destroy_fixture(body, f);
        }
        if !shapes.is_empty() {
            self.world.reset_mass_data(body);
        }
        self.world.contacts2d.destroy_contacts(collider);
        self.record_collider_disabled(collider);
    }

    /// GameObject.SetActive(false) on a physical object, component by component.
    fn deactivate(&mut self, p: usize) {
        // Collider2D::Deactivate (180c0fc00): its Physics2D.IgnoreCollision pairs go; the cleanup finds no shapes.
        let w = &mut self.world;
        for &collider in &self.physicals[p].all_colliders {
            w.ignored.retain(|&(a, b)| a != collider && b != collider);
            w.contacts2d.destroy_contacts(collider);
        }
        self.rigidbody_cleanup(p);
        self.physicals[p].active = false;
        // The Joint2Ds a script or tool added to the object (or adopted from its prefab) go inactive with it.
        for f in self.fixed_joints.iter_mut().filter(|f| matches!(f.attached, super::BodyOwner::Physical(x) if x == p)) {
            f.active = false;
        }
        // GForceMeasureBehaviour.OnDisable on a limb.
        if let Some(l) = self.limbs.iter().position(|limb| limb.PhysicalBehaviour == p) {
            // A limb that has not started yet (destroyed in the frame it was created) has no gforce arena entry: its GForceMeasureBehaviour still holds zeros.
            if let Some(g) = self.gforces.get_mut(l) {
                (g.acceleration, g.sustainedAcceleration, g.velocity, g.velocityLastFrame) = ([0.0; 2], [0.0; 2], [0.0; 2], [0.0; 2]);
            }
        }
        self.physical_on_disable(p);
        // The HingeJoint2D/SpringJoint2D Deactivate cleanups find their b2 joints already gone.
    }

    /// PhysicalBehaviour.OnDisable: an empty Collision2D exit to each buffered contact's collider (SendMessage skips
    /// inactive objects; collision messages reach disabled behaviours).
    pub fn physical_on_disable(&mut self, p: usize) {
        self.physical_send_buffer_exits(p);
    }

    /// Rigidbody2D::Cleanup (180c1dd20) with DeactivateOperation 0: no attached colliders are left, every joint on
    /// the body (body list from the head) loses its b2 joint, then the body goes and the scene's movement list is
    /// rebuilt after the next step.
    pub(super) fn rigidbody_cleanup(&mut self, p: usize) {
        let body = self.physicals[p].rigidbody.body;
        for (_, j) in self.world.body(body).joints.clone().into_iter().rev() {
            self.joint2d_cleanup_any(j);
        }
        let b = self.world.body(body);
        let (mass, props) = (b.rigidbody, [b.linear_damping, b.angular_damping, b.gravity_scale]);
        self.world.destroy_body(body);
        // The instance no longer has this body: a later SyncTransforms finds nothing to move. (An object loaded from
        // a recording has no clone kept.)
        let id = self.physicals[p].id;
        if self.unsynced.iter().chain(&self.retained).any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            self.with_component_instance(id, |_, inst, _| inst.bodies.retain(|_, b| *b != body));
        }
        let rb = &mut self.physicals[p].rigidbody;
        rb.detached = Some(mass);
        rb.detached_props = props;
        rb.movement.listed = false;
        self.rigidbody_hierarchy_changed = true;
    }
}

impl Game {
    /// Joint2D::Cleanup(false) (vtable +0x110, DeactivateOperation 0) that Rigidbody2D::Cleanup (180c1dd20) runs on
    /// every joint of its body's list, whoever owns the Joint2D: each kind keeps what its next Create needs, then the
    /// b2 joint is destroyed. The caller handles the joints of the clone it has checked out.
    pub(super) fn joint2d_cleanup_any(&mut self, j: crate::b2::world::JointId) {
        if self.attachment_joint_cleanup(j) {
            // The HingeJoint2D a FirearmAttachmentPointBehaviour added to its firearm.
        } else if let Some(f) = self.fixed_joints.iter().position(|f| f.joint == Some(j)) {
            // A Joint2D a script or tool added (FixedJoint2D, SliderJoint2D, HingeJoint2D, SpringJoint2D, ...), or a
            // prefab joint a script adopted (ahead of the record, which can still name it), as joint2d_recreate_any.
            self.fixed_joint_cleanup(f, false);
        } else if self
            .with_joint_instance(j, |g, inst, o, c| {
                // A prefab-cloned Joint2D: HingeJoint2D / FixedJoint2D / SliderJoint2D::Cleanup of its clone.
                let prefab = inst.prefab.clone();
                g.joint2d_deactivate(inst, &prefab, o, c);
            })
            .is_some()
        {
        } else if let Some(h) = self.hinges.iter().position(|h| h.joint == Some(j)) {
            // A limb's HingeJoint2D: HingeJoint2D::Cleanup (180bfc5e0) keeps the angles for a re-create a limb never gets.
            self.hinges[h].joint = None;
            self.world.destroy_joint(j);
        } else if let Some(l) = self.limbs.iter().position(|l| l.gore.joint == Some(j)) {
            // The gore string's SpringJoint2D.
            self.limbs[l].gore.joint = None;
            self.world.destroy_joint(j);
        } else if let Some(t) = self.tool.as_mut().filter(|t| t.dragJoint.iter().chain(&t.destroying).any(|d| d.joint == Some(j))) {
            // The drag tool's HingeJoint2D.
            for d in t.dragJoint.iter_mut().chain(&mut t.destroying) {
                if d.joint == Some(j) {
                    d.joint = None;
                }
            }
            self.world.destroy_joint(j);
        } else {
            unimplemented!("mod-only: Rigidbody2D::Cleanup: a joint no owner knows");
        }
    }
}

/// The loop of PhysicalBehaviour.GetLowestGlobalGridPoint: the first point with the least y, zero for none.
fn lowest_point(points: &[[f32; 2]]) -> [f32; 2] {
    let (mut num, mut result) = (f32::MAX, [0.0f32; 2]);
    for &v in points {
        if v[1] < num {
            result = v;
            num = v[1];
        }
    }
    result
}

#[cfg(test)]
mod lowest_tests {
    #[test]
    fn lowest_point_is_first_least_y() {
        assert_eq!(super::lowest_point(&[]), [0.0, 0.0]);
        assert_eq!(super::lowest_point(&[[1.0, 2.0], [3.0, -1.0], [5.0, -1.0], [0.0, 4.0]]), [3.0, -1.0]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{dump, Game};
    use crate::assets::db::{AssetDb, Component};
    use crate::assets::mono::Value;
    use crate::b2::world::BodyId;
    use crate::game_assets::HumanRig;
    use std::path::Path;

    fn grid(v: &Value) -> Vec<[f32; 2]> {
        v.array().unwrap().iter().map(|p| [p.get("x").unwrap().f32().unwrap(), p.get("y").unwrap().f32().unwrap()]).collect()
    }

    /// Prefabs/SpawnParticles is the one object physical_setup_spawn_prefab lays out: GameObject, Transform,
    /// ParticleSystem, ParticleSystemRenderer and an enabled DeleteAfterTime with Life 1 (its id the GameObject's - 8).
    #[test]
    fn spawn_particles_objects_match_data() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let prefab = db.prefab(db.resource("Prefabs/SpawnParticles", 1).unwrap().unwrap()).unwrap();
        assert_eq!(prefab.objects.iter().map(|o| 1 + o.components.len()).sum::<usize>(), super::SPAWN_PARTICLES_OBJECTS);
        match &prefab.objects[0].components[3].1 {
            Component::Script { class, fields, enabled, .. } => {
                assert_eq!(class, "DeleteAfterTime");
                assert!(*enabled);
                assert_eq!(fields.get("Life").unwrap().f32().unwrap(), 1.0);
            }
            c => panic!("SpawnParticles' fourth component: {c:?}"),
        }
    }

    /// PhysicalBehaviour.Awake and Start on dump30start's first snapshot (the Person's 14 limbs, after the
    /// game ran them): the fields reset to the Person prefab's values, then Awake and Start must rebuild what the
    /// recording holds, bit for bit. SpawnSpawnParticles is switched off: its clone (physical_setup_spawn_prefab's
    /// ids, DeleteAfterTime and particle effect) is checked by spawn_particles_objects_match_data and the replays.
    #[test]
    fn start_matches_game() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Ok(rig) = HumanRig::load(&root.join("local-cache"), &game) else { return };
        let Ok(mut snaps) = dump::snapshots(&root.join("reference-probe/game/dump30start.txt")) else { return };
        // The first snapshot (frame 428, right after the Starts) holds the fields; the first FS one with the b2 world builds the game.
        let first = snaps.next().unwrap();
        let fs = snaps.find(|s| s.pt == "FS" && s.of("b2World").next().is_some()).unwrap();
        let mut g = Game::from_snapshot(&fs, &rig).unwrap();

        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, cat) = db.mono(catalog).unwrap().unwrap();
        let mut person = None;
        for item in cat.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, sp) = db.mono(asset).unwrap().unwrap();
            if let Some(p) = sp.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) {
                if db.game_object(p).unwrap().name == "Person" {
                    person = Some(p);
                }
            }
        }
        let prefab = db.prefab(person.unwrap()).unwrap();

        // The Start order is the recording's: the Person's limbs as the snapshot lists their PhysicalBehaviours.
        let records: Vec<_> = first.of("PhysicalBehaviour").collect();
        let order: Vec<usize> = records
            .iter()
            .filter(|r| prefab.objects.iter().any(|o| o.name == r.name && o.components.iter().any(|(_, c)| matches!(c, Component::Rigidbody2D(_)))))
            .map(|r| g.physicals.iter().position(|p| p.id == r.id).unwrap())
            .collect();
        assert_eq!(order.len(), 14);
        let mut out = Vec::new();
        for (n, &p) in order.iter().enumerate() {
            let name = g.physicals[p].name.clone();
            let r = records.iter().find(|r| r.id == g.physicals[p].id).unwrap();
            let obj = prefab.objects.iter().find(|o| o.name == name).unwrap();
            let rb = obj.components.iter().find_map(|(_, c)| match c { Component::Rigidbody2D(r) => Some(r.clone()), _ => None }).unwrap();
            let fields = obj.components.iter().find_map(|(_, c)| match c { Component::Script { class, fields, .. } if class == "PhysicalBehaviour" => Some(fields.clone()), _ => None }).unwrap();
            let body: BodyId = g.physicals[p].rigidbody.body;

            // The recording.
            let id_of = |k: &str| -> i32 { r.get(k).unwrap().rsplit_once('#').unwrap().1.parse().unwrap() };
            let b = g.world.body(body);
            let want_mass = [b.mass, b.inv_mass, b.i, b.inv_i].map(f32::to_bits);
            let want_rb_mass = b.rigidbody.mass;
            let ph = &g.physicals[p];
            let f = |k: &str| f32::from_bits(u32::from_str_radix(r.get(k).unwrap(), 16).unwrap());
            let want = (f("seed"), r.get("intSeed").unwrap().parse::<i32>().unwrap(), f("affectSurroundingTimer"), f("InitialGravityScale"), f("ObjectArea"), f("InitialMass"), f("TrueInitialMass"));
            let want_grid: Vec<[f32; 2]> = r.get("LocalColliderGridPoints").unwrap().trim_matches(|c| c == '[' || c == ']').split(';').map(|p| {
                let (x, y) = p.split_once(',').unwrap();
                [f32::from_bits(u32::from_str_radix(x, 16).unwrap()), f32::from_bits(u32::from_str_radix(y, 16).unwrap())]
            }).collect();
            let want_circ = f("Circumference");
            let want_ids = (id_of("ActivationNode"), id_of("ContextMenuOptions"), id_of("audioSource"), id_of("slidingAudioSource"), id_of("selectionOutlineObject"), id_of("outlineSpriteRenderer"));
            let want_fx: Vec<(u32, u32)> = g.world.body(body).fixtures.iter().map(|&f| (g.world.fixture(f).friction.to_bits(), g.world.fixture(f).restitution.to_bits())).collect();
            let want_mode = (ph.rigidbody.collisionDetectionMode, ph.rigidbody.interpolation);
            let want_bounds = r.get("InitialBounds").unwrap().to_string();

            // Back to the prefab.
            for &f in &g.world.body(body).fixtures.clone() {
                let fx = g.world.fixtures[f].as_mut().unwrap();
                (fx.friction, fx.restitution) = (0.4, 0.0);
            }
            {
                let b = g.world.body_mut(body);
                b.rigidbody.mass = rb.mass;
                b.rigidbody.use_auto_mass = rb.use_auto_mass;
            }
            g.world.reset_mass_data(body);
            {
                let ph = &mut g.physicals[p];
                ph.seed = 0.0;
                ph.intSeed = 0;
                ph.affectSurroundingTimer = 0.0;
                ph.InitialGravityScale = 0.0;
                ph.ObjectArea = 0.0;
                ph.InitialMass = 0.0;
                ph.TrueInitialMass = 0.0;
                ph.LocalColliderGridPoints = Some(grid(fields.get("LocalColliderGridPoints").unwrap()));
                ph.Circumference = fields.get("Circumference").unwrap().f32().unwrap();
                ph.ids = Default::default();
                g.activation_nodes.retain(|n| n.owner != Some(crate::game::BodyOwner::Physical(p)));
                ph.audioSource = false;
                ph.audio_time_scale = false;
                ph.rigidbody.collisionDetectionMode = rb.collision_detection;
                ph.rigidbody.interpolation = rb.interpolate;
                ph.SpawnSpawnParticles = false;
                // The prefab's Rigidbody2D has no sharedMaterial (the snapshot's has Start's): SetProperties sets it again.
                ph.rigidbody_has_material = false;
            }
            let prefab_circ = g.physicals[p].Circumference;
            g.next_instance_id = want_ids.0;
            g.physical_awake(p);
            // The first Start also adds the AudioSourceTimeScaleBehaviour to the root.
            g.next_instance_id = want_ids.2 + if n == 0 { 2 } else { 0 };
            g.physical_start(p);

            let b = g.world.body(body);
            let got_mass = [b.mass, b.inv_mass, b.i, b.inv_i].map(f32::to_bits);
            let ph = &g.physicals[p];
            let got = (ph.seed, ph.intSeed, ph.affectSurroundingTimer, ph.InitialGravityScale, ph.ObjectArea, ph.InitialMass, ph.TrueInitialMass);
            let got_ids = (ph.ids.ActivationNode, ph.ids.ContextMenuOptions, ph.ids.audioSource, ph.ids.slidingAudioSource, ph.ids.selectionOutlineObject, ph.ids.outlineSpriteRenderer);
            let got_fx: Vec<(u32, u32)> = g.world.body(body).fixtures.iter().map(|&f| (g.world.fixture(f).friction.to_bits(), g.world.fixture(f).restitution.to_bits())).collect();
            let eq = |a: f32, b: f32| a.to_bits() == b.to_bits();
            let mut bad = Vec::new();
            if got_mass != want_mass {
                bad.push(format!("mass words game {want_mass:08x?} ours {got_mass:08x?}"));
            }
            if !eq(b.rigidbody.mass, want_rb_mass) {
                bad.push(format!("rigidbody.mass game {want_rb_mass} ours {}", b.rigidbody.mass));
            }
            if !(eq(got.0, want.0) && got.1 == want.1 && eq(got.2, want.2) && eq(got.3, want.3) && eq(got.4, want.4) && eq(got.5, want.5) && eq(got.6, want.6)) {
                bad.push(format!("seed/intSeed/timer/gravity/area/mass/true game {want:?} ours {got:?}"));
            }
            if ph.LocalColliderGridPoints.as_ref().unwrap().iter().flatten().map(|v| v.to_bits()).ne(want_grid.iter().flatten().map(|v| v.to_bits())) {
                bad.push(format!("grid game {want_grid:?} ours {:?}", ph.LocalColliderGridPoints));
            }
            if !eq(prefab_circ, want_circ) {
                bad.push(format!("Circumference prefab {prefab_circ} game {want_circ}"));
            }
            if got_ids != want_ids {
                bad.push(format!("ids game {want_ids:?} ours {got_ids:?}"));
            }
            if g.next_instance_id != want_ids.5 - 2 {
                bad.push(format!("next id {} after the outline renderer {}", g.next_instance_id, want_ids.5));
            }
            if got_fx != want_fx {
                bad.push(format!("fixture friction/restitution game {want_fx:08x?} ours {got_fx:08x?}"));
            }
            if (ph.rigidbody.collisionDetectionMode, ph.rigidbody.interpolation) != want_mode {
                bad.push(format!("cdm/interpolation game {want_mode:?} ours {:?}", (ph.rigidbody.collisionDetectionMode, ph.rigidbody.interpolation)));
            }
            let (c, e) = ph.InitialBounds;
            let ours = format!("Bounds(m_Center={:08x},{:08x},{:08x} m_Extents={:08x},{:08x},{:08x})", c[0].to_bits(), c[1].to_bits(), c[2].to_bits(), e[0].to_bits(), e[1].to_bits(), e[2].to_bits());
            if ours != want_bounds {
                bad.push(format!("InitialBounds game {want_bounds} ours {ours}"));
            }
            out.extend(bad.into_iter().map(|m| format!("{name}: {m}")));
        }
        assert!(out.is_empty(), "{} mismatches:\n{}", out.len(), out.join("\n"));
    }
}

/// Quaternion.Euler(0, 0, z), for an effect's pose.
pub(super) fn z_rotation(z: f32) -> [f32; 4] {
    let half = z as f64 * std::f64::consts::PI / 360.0;
    [0.0, 0.0, half.sin() as f32, half.cos() as f32]
}

/// WeightlessHelper: Bodies as Rigidbody2D component ids and the masses Awake kept.
pub struct WeightlessHelper {
    pub id: i32,
    pub bodies: Vec<i32>,
    pub original_mass: Vec<f32>,
}

/// Zap: the message argument of Shocked, the point the zap hit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zap {
    pub position: [f32; 3],
}

impl Zap {
    /// Zap..ctor(position): this.position = position.
    pub fn new(position: [f32; 3]) -> Self {
        Zap { position }
    }
}

#[cfg(test)]
mod zap_tests {
    use super::Zap;

    #[test]
    fn zap_keeps_its_position() {
        assert_eq!(Zap::new([1.0, 2.0, 3.0]).position, [1.0, 2.0, 3.0]);
    }
}

/// PhysicalBehaviour.TryGetFreeCollisionBufferIndex: 1.28.41's body is `index = -1; return false;` (nothing calls it).
pub fn try_get_free_collision_buffer_index() -> (bool, i32) {
    (false, -1)
}
