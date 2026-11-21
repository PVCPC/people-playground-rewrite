//! LimbBehaviour
use super::physical::{dot2, get_average_impulse_remove_outliers, get_contacts, get_first_valid_contact, get_min_impulse};
use super::{d, mathf, person::pose_state, script, Game};
use crate::b2::world::BodyId;
use crate::b2::contacts2d::ScriptCollision;

/// LimbBehaviour.BodyPart
pub const HEAD: i32 = 0;
pub const TORSO: i32 = 1;
pub const LEGS: i32 = 2;
pub const ARMS: i32 = 3;

/// Instance ids taken by the Instantiate of the LimbStatus prefab (LimbBehaviour.Awake): GameObject, Transform, Optout,
/// LimbStatusBehaviour, SpriteRenderer, ParentConstraint, then the bar's GameObject, Transform and SpriteRenderer.
pub const LIMB_STATUS_OBJECTS: usize = 9;

script! {
    Limb {
        RoughClassification: i32 = 0,
        GForcePassoutThreshold: f32 = 10.0,
        GForceDamageThreshold: f32 = 10.0,
        KillShotParticlesEmitted: bool = false,
        Vitality: f32 = 0.0,
        InitialHealth: f32 = 0.0,
        HasBrain: bool = false,
        FreezingTemperature: f32 = -25.0,
        DiscomfortingHeatTemperature: f32 = 40.0,
        RegenerationSpeed: f32 = 0.0,
        ImpactPainMultiplier: f32 = 0.0,
        ImpactDamageMultiplier: f32 = 1.0,
        BreakingThreshold: f32 = 0.0,
        BaseStrength: f32 = 5.0,
        FakeUprightForce: f32 = 0.0,
        BodyTemperature: f32 = 37.0,
        ShotDamageMultiplier: f32 = 1.0,
        DoStumble: bool = false,
        IsLethalToBreak: bool = false,
        DoBalanceJerk: bool = false,
        ExplodeLiquidAmount: f32 = 5.357143,
        BloodMuscleStrengthRatio: f32 = 1.0,
        BalanceMuscleMovement: f32 = 1.0,
        Health: f32 = 0.0,
        Numbness: f32 = 0.0,
        IsAndroid: bool = false,
        IsZombie: bool = false,
        HasJoint: bool = false,
        Broken: bool = false,
        Frozen: bool = false,
        IsDismembered: bool = false,
        IsOnFloor: bool = false,
        DistanceToBrain: i32 = 0,
        InternalExternalTemperatureTransferRate: f32 = 0.01,
        InternalToExternalTempTransferRate: f32 = 0.002,
        ExternalToInternalTempTransferRate: f32 = 0.005,
        InternalTemperature: f32 = 0.0,
        IsActiveInCurrentPose: bool = false,
        previousVelocity: [f32; 2] = [0.0, 0.0],
        FrictionBurnWoundMinSpeedSqrd: f32 = 62.0,
        BodyHeatProductionFactor: f32 = 1.0,
        stumbleTime: f32 = 0.0,
        JointStress: f32 = 0.0,
        randomOffset: f32 = 0.0,
        BruiseCount: u16 = 0,
        ShatteredObjectChance: f32 = 0.1,
        CurrentlyShattered: u8 = 0,
        ImmuneToDamage: bool = false,
        previousHealth: f32 = 0.0,
        HasLungs: bool = false,
        LungsPunctured: bool = false,
        shotHeat: f32 = 0.0,
        originalJointLimits: [f32; 2] = [0.0, 0.0],
        RegenerateBurnProgressSpeed: f32 = 1.0,
    }
    links {
        /// The LimbStatus clone's world scale (x, y): SetParent(null) in its Awake keeps the scale it had under the limb then.
        status_scale: [f32; 2] = [1.0, 1.0],
        /// ShatteredObjectGenerators (the ShatteredObjectSpriteInitialiser): its Bone, Flesh and Skin generators as
        /// `Game::shatter_generators` indices; None without the component.
        ShatteredObjectGenerators: Option<[Option<usize>; 3]> = None,
        name: String = String::new(),
        id: i32 = 0,
        PhysicalBehaviour: usize = 0,
        Person: usize = 0,
        /// Index into Game::hinges.
        Joint: Option<usize> = None,
        GripBehaviour: Option<usize> = None,
        ConnectedLimbs: Vec<usize> = Vec::new(),
        /// KillShotParticles is set (limb_shot turns and plays it).
        KillShotParticles: bool = false,
        NearestLimbToBrain: Option<usize> = None,
        StrengthMultipliers: Vec<f32> = Vec::new(),
        SpeciesIdentity: String = "Human".into(),
        BloodLiquidType: String = "BLOOD".into(),
        /// The BloodDecal DecalDescriptor's name.
        BloodDecal: String = "Dense_blood".into(),
        /// LimbBehaviour.Color (the sprite's colour: clear at construction, white from Awake); only the Ultra strength serum reads it.
        Color: [f32; 4] = [0.0; 4],
        // LimbBehaviour.ShouldUpdate (IManagedBehaviour): enabled.
        enabled: bool = true,
        /// In LimbBehaviourManager.Limbs and subscribed to PhysicalBehaviour.OnDisintegration: LimbBehaviour.Start ran (a loaded limb
        /// already had). A limb whose PhysicalBehaviour.Start disintegrated it (an isDisintegrated save) is inactive before its own
        /// Start, so it never joins: no ManagedUpdate, no OnDisintegration handler.
        started: bool = true,
        /// lastSlashProcessTime (1.28.45): float.MinValue.
        lastSlashProcessTime: f32 = f32::MIN,
        /// The `gforces` slot of a limb whose Start is still waiting (a later limb's Start added its GForceMeasureBehaviour first).
        gforce_pending: bool = false,
        /// The GoreStringBehaviour component: present, enabled and its `Joint` (a SpringJoint2D).
        goreString: bool = true,
        goreStringEnabled: bool = true,
        /// LimbBehaviour.DestroyWith: a component id on each GameObject (the gore shards PrepareGoreBits added).
        destroy_with: Vec<i32> = Vec::new(),
        /// gapC r20: myStatus (the LimbStatus UI clone's GameObject id) as a snapshot recorded it; None in play (the clone is
        /// UI, not modelled) and once destroyed.
        my_status: Option<i32> = None,
        goreStringJoint: bool = false,
        /// The GoreStringBehaviour's own values and line state.
        gore: GoreString = GoreString::default(),
    }
}

/// GoreStringBehaviour.TissueString
#[derive(Default, Clone, Copy, Debug)]
pub struct TissueString {
    pub Pos1: f32,
    pub Pos2: f32,
    pub Strength: f32,
    pub Thickness: f32,
    pub ThicknessRatio: f32,
}

impl TissueString {
    /// TissueString..ctor(pos1, pos2, thickness, strength, thicknessRatio)
    pub fn new(pos1: f32, pos2: f32, thickness: f32, strength: f32, thickness_ratio: f32) -> Self {
        TissueString { Pos1: pos1, Pos2: pos2, Strength: strength, Thickness: thickness, ThicknessRatio: thickness_ratio }
    }
}

/// GoreStringBehaviour
#[derive(Clone, Debug)]
pub struct GoreString {
    /// The limb whose rigidbody is `Other`.
    pub Other: Option<usize>,
    pub JointDampening: f32,
    pub JointFreq: f32,
    pub JointBreakThreshold: f32,
    pub InvincibilityTime: f32,
    /// LineSegment A.x, A.y, B.x, B.y
    pub OriginLine: [f32; 4],
    pub OtherLine: [f32; 4],
    pub LineWidthMultiplier: f32,
    pub RenderOrder: i32,
    pub TissueColour: [f32; 4],
    /// The limb whose CirculationBehaviour is ColourSource.
    pub ColourSource: Option<usize>,
    pub smoothedColour: [f32; 4],
    pub HadJointWhenSerialised: bool,
    /// TissueSnapClips length (0 when null).
    pub TissueSnapClips: usize,
    pub AudioSource: bool,
    /// The GoreStringBehaviour's instance id (its TissueSnapClips and AudioSource fields).
    pub id: i32,
    /// `lines != null` with its `lineData`.
    pub lineData: Option<Vec<TissueString>>,
    /// The pending SetBreakForce coroutine: Time.timeAsDouble it resumes at.
    pub break_at: Option<f64>,
    /// Joint.breakForce of the SpringJoint2D.
    pub breakForce: f32,
    /// The SpringJoint2D's b2DistanceJoint.
    pub joint: Option<crate::b2::world::JointId>,
    /// A pending Object.Destroy(Joint): the DelayedCallManager time it runs at.
    pub destroy_at: Option<f64>,
    /// The SpringJoint2D's instance id (the recorded Joint; 0: none or one the rewrite made).
    pub spring_id: i32,
}

impl GoreString {
    /// GoreStringBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>() (the limb's own), smoothedColour = TissueColour.
    pub fn awake(&mut self) {
        self.smoothedColour = self.TissueColour;
    }
}

impl Default for GoreString {
    fn default() -> Self {
        Self {
            Other: None,
            JointDampening: 0.5,
            JointFreq: 4.0,
            JointBreakThreshold: 4.0,
            InvincibilityTime: 0.8,
            OriginLine: [0.0; 4],
            OtherLine: [0.0; 4],
            LineWidthMultiplier: 1.0,
            RenderOrder: 3000,
            TissueColour: [0.0; 4],
            ColourSource: None,
            smoothedColour: [0.0; 4],
            HadJointWhenSerialised: false,
            TissueSnapClips: 0,
            AudioSource: false,
            id: 0,
            lineData: None,
            break_at: None,
            breakForce: f32::INFINITY,
            spring_id: 0,
            joint: None,
            destroy_at: None,
        }
    }
}

/// One SpriteRenderer of the LimbStatus prefab: sprite, colour, sortingOrder, sortingLayerID, draw.
pub type StatusSprite = (crate::assets::db::ObjRef, [f32; 4], i16, i32, super::SpriteDraw);

/// LimbStatusBehaviour's look: the root SpriteRenderer with BarSprite, with DeadSprite, and the Bar child's.
#[derive(Clone, Debug)]
pub struct LimbStatusLook {
    pub frame: StatusSprite,
    pub dead: StatusSprite,
    pub bar: StatusSprite,
    /// BarMaxWidth
    pub BarMaxWidth: f32,
}

impl Default for LimbStatusLook {
    fn default() -> Self {
        Self {
            frame: Default::default(),
            dead: Default::default(),
            bar: Default::default(),
            BarMaxWidth: 9.0,
        }
    }
}

impl Game {
    /// ExistInDetailView.Main_LimbStatusToggled(sender, e): base.gameObject.SetActive(e), on the component `id`'s object.
    pub(crate) fn exist_in_detail_view_toggled(&mut self, id: i32, e: bool) {
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, e));
    }

    /// Global.ToggleLimbStatus (the "toggleLimbStatus" key): the LimbStatusToggled listeners are the limbs' status
    /// objects (drawn from ShowLimbStatus), ExistInDetailView's SetActive and InvisibleOptionInContextMenu's UpdateVisibility.
    pub fn toggle_limb_status(&mut self) {
        // Global.add_LimbStatusToggled: the listeners are subscribed when their objects start (the ones below)
        // Global.remove_LimbStatusToggled: unsubscribed by OnDestroy, which `destroyed_components` stands for
        self.ShowLimbStatus = !self.ShowLimbStatus;
        let on = self.ShowLimbStatus;
        // ExistInDetailView.OnDestroy: removes the listener (a destroyed one is skipped here).
        for id in self.detail_views.clone() {
            if !self.destroyed_components.contains(&id) {
                self.exist_in_detail_view_toggled(id, on);
            }
        }
        // The links' end objects (game/link_device.rs).
        self.phase_link_detail_view(on);
        // WireBehaviour.Main_LimbStatusToggled: with the status view off, the line takes WireColor again (a wire
        // subscribed in Awake and not yet destroyed; no line before Initialise).
        if !on {
            for w in 0..self.blood_wires.len() {
                if self.blood_wires[w].started && !self.destroyed_components.contains(&self.blood_wires[w].id) {
                    let c = self.blood_wires[w].WireColor;
                    (self.blood_wires[w].start_color, self.blood_wires[w].end_color) = (c, c);
                }
            }
            // A root wire's OnDestroy throws before `LimbStatusToggled -= ...`: it listens while its line (the Wire child) lives.
            for w in self.root_wires.iter_mut().filter(|w| w.started && !w.gone) {
                (w.start_color, w.end_color) = (w.WireColor, w.WireColor);
            }
        }
        // InvisibleOptionInContextMenu.DetailViewToggled, once per subscription (none once destroyed)
        for i in 0..self.invisibles.len() {
            for _ in 0..self.listen_count(self.invisibles[i].id, super::continuous::Ev::LimbStatusToggled) {
                self.invisible_update(i);
            }
        }
    }

    /// InvisibleOptionInContextMenu.OnDestroy: when Global.main lives, LimbStatusToggled -= DetailViewToggled.
    pub fn invisible_on_destroy(&mut self, i: usize) {
        self.listen_remove(self.invisibles[i].id, super::continuous::Ev::LimbStatusToggled);
    }
}

impl Game {
    /// LimbBehaviour.GetMassStrengthRatio
    pub(crate) fn mass_strength_ratio(&self, l: usize) -> f32 {
        let ph = &self.physicals[self.limbs[l].PhysicalBehaviour];
        ph.rigidbody.mass(&self.world) / ph.TrueInitialMass
    }

    /// The limb's GameObject.activeInHierarchy (a disintegrated limb is deactivated).
    pub fn limb_active(&self, l: usize) -> bool {
        self.physicals[self.limbs[l].PhysicalBehaviour].active
    }

    /// LimbBehaviour.IsParalysed
    pub fn limb_is_paralysed(&self, l: usize) -> bool {
        let limb = &self.limbs[l];
        if !limb.IsAndroid && !limb.IsZombie {
            if let Some(n) = limb.NearestLimbToBrain {
                if self.nodes[n].IsConnectedToRoot {
                    if !self.limbs[n].Broken || self.limbs[n].RoughClassification != TORSO {
                        return self.limb_is_paralysed(n);
                    }
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// LimbBehaviour.IsConsideredAlive
    pub fn limb_is_considered_alive(&self, l: usize) -> bool {
        if self.nodes[l].IsConnectedToRoot && self.circulations[l].BloodFlow > 0.25 {
            return self.limbs[l].Health > 0.01;
        }
        false
    }

    /// LimbBehaviour.IsAnyHeadAlive -> Person.IsHeadAlive
    pub(crate) fn is_any_head_alive(&self, l: usize) -> bool {
        self.people[self.limbs[l].Person].Head.is_some_and(|h| self.limb_is_considered_alive(h))
    }

    /// LimbBehaviour.IsCapable
    pub fn limb_is_capable(&mut self, l: usize) -> bool {
        let limb = &self.limbs[l];
        let ph = &self.physicals[limb.PhysicalBehaviour];
        if limb.Frozen || ph.Temperature <= limb.FreezingTemperature {
            return false;
        }
        let person = &self.people[limb.Person];
        let c = &self.circulations[l];
        if self.nodes[l].IsConnectedToRoot
            && limb.Health > 1.0
            && person.Consciousness > 0.8
            && person.ShockLevel < 0.5
            && person.PainLevel < 0.5
            && limb.Numbness < 0.9
            && c.InternalBleedingIntensity < 0.5
            && self.has_circulation(l)
            && !limb.Broken
            && c.BloodFlow > 0.25
        {
            return self.is_any_head_alive(l);
        }
        false
    }

    /// LimbBehaviour.MotorStrength getter (it writes Health and Numbness).
    pub fn motor_strength(&mut self, l: usize) -> f32 {
        let person = self.limbs[l].Person;
        let (consciousness, adrenaline) = (self.people[person].Consciousness, self.people[person].AdrenalineLevel);
        if consciousness < mathf::max(0.3, 0.8 - mathf::clamp01(adrenaline)) {
            return 0.0;
        }
        let ib = self.circulations[l].InternalBleedingIntensity;
        let limb = &mut self.limbs[l];
        limb.Health = mathf::max(0.0, limb.Health);
        limb.Numbness = mathf::clamp01(limb.Numbness + mathf::min(ib * 0.25, 0.1));
        let charge = self.physicals[self.limbs[l].PhysicalBehaviour].Charge();
        if (self.limbs[l].Broken || self.limb_is_paralysed(l) || !self.limb_is_considered_alive(l) || !self.is_any_head_alive(l))
            && charge < 0.05
        {
            return 0.0;
        }
        let limb = &self.limbs[l];
        let ph = &self.physicals[limb.PhysicalBehaviour];
        let c = &self.circulations[l];
        let ratio = 3.0 * limb.BloodMuscleStrengthRatio;
        let a = ((d(limb.Health) / limb.InitialHealth + mathf::clamp01(adrenaline))
            * mathf::pow(self.amount_of_blood(l), ratio)
            * mathf::pow(c.BloodFlow, 3.0 * limb.BloodMuscleStrengthRatio)
            * mathf::clamp01(consciousness + mathf::clamp01(adrenaline))
            * mathf::clamp01(1.0 - ph.BurnProgress)
            * mathf::clamp01(1.0 - self.skins[l].acidProgress)
            * mathf::clamp01((1.0 - (d(limb.Numbness) - mathf::clamp01(adrenaline))).f()))
        .f();
        let num = match self.people[person].ActivePose {
            Some(_) if limb.IsActiveInCurrentPose => self.active_pose(person).ForceMultiplier,
            _ => 1.0,
        };
        (d(limb.BaseStrength) * num * 0.9 * mathf::clamp01(mathf::max(a, ph.Charge() * 0.5)) * self.mass_strength_ratio(l)).f()
    }

    /// LimbBehaviour.Damage
    /// limb.transform.CompareTag("Limb"): TagManager's "Limb" (20001). The Human prefab's Foot and FootFront are untagged.
    pub(crate) fn limb_tagged(&mut self, l: usize) -> bool {
        const LIMB_TAG: u16 = 20001;
        let tag = if self.is_recording() {
            // A recording holds no clone: the recorded person's catalog prefab (the recorder names the person's
            // GameObject after its spawnable), its object of the limb's name.
            let name = self.physicals[self.limbs[l].PhysicalBehaviour].name.clone();
            let spawnable = self.people[self.limbs[l].Person].name.clone();
            let game = crate::game_assets::find_game(None).expect("the game's data");
            let db = self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let prefab = Self::catalog_prefab(db, &spawnable).expect("the recorded person's catalog prefab");
            prefab.objects.iter().find(|o| o.name.replace(' ', "_") == name).expect("the limb in its person's prefab").tag
        } else {
            let id = self.limbs[l].id;
            self.with_component_instance(id, |_, inst, o| inst.prefab.objects[o].tag)
        };
        tag == LIMB_TAG
    }

    pub fn limb_damage(&mut self, l: usize, damage: f32) {
        if self.prefs.StopAnimationOnDamage && !self.limbs[l].IsZombie && damage > 15.5 && self.limb_is_considered_alive(l) {
            self.people[self.limbs[l].Person].OverridePoseIndex = -1;
        }
        self.limbs[l].Health -= damage;
        if self.limbs[l].Health <= 0.0 {
            self.circulations[l].IsPump = false;
        }
    }

    /// LimbBehaviour.Wince
    pub fn limb_wince(&mut self, l: usize, intensity: f32) {
        if self.limbs[l].HasJoint && self.limb_is_considered_alive(l) && !(self.limbs[l].Health < 0.1) {
            let value = (d(60.0f32) * intensity * (d(mathf::perlin(self.time.time * 8.0, self.limbs[l].randomOffset)) * 2.0 - 1.0)).f();
            self.influence_motor_speed(l, mathf::clamp(value, -450.0, 450.0), 0.5);
        }
    }

    /// LimbBehaviour.InfluenceMotorSpeed
    pub fn influence_motor_speed(&mut self, l: usize, mut value: f32, influence: f32) {
        if !self.limbs[l].HasJoint {
            return;
        }
        let h = self.limbs[l].Joint.unwrap();
        let motor = self.hinges[h].motor;
        if self.people[self.limbs[l].Person].BrainDamaged && self.random.value() > 0.95 {
            value *= self.random.range_int(-1, 1) as f32;
        }
        let speed = mathf::lerp(motor[0], value, influence);
        self.hinges[h].set_motor(&mut self.world, speed, motor[1]);
    }

    /// LimbBehaviour.SetMotorStrength
    pub(crate) fn set_motor_strength(&mut self, l: usize, strength: f32) {
        let h = self.limbs[l].Joint.unwrap();
        let motor = self.hinges[h].motor;
        let mut num = 1.0;
        if self.limbs[l].IsActiveInCurrentPose {
            let pose = self.active_pose(self.limbs[l].Person);
            num = mathf::clamp01(1.0 + pose.Angles[pose.angle_index(l).unwrap()].PoseRigidityModifier);
        }
        let torque = (d(strength) * (if self.limbs[l].IsAndroid { 6.0 } else { 1.0 }) * num).f();
        self.hinges[h].set_motor(&mut self.world, motor[0], torque);
    }

    /// LimbBehaviour.SetMotorStrengthToMuscleStrength
    pub(crate) fn set_motor_strength_to_muscle_strength(&mut self, l: usize) {
        let strength = self.motor_strength(l);
        let mut num = mathf::max(strength, self.skins[l].rottenProgress * 0.8);
        for &m in &self.limbs[l].StrengthMultipliers {
            num *= m;
            if num.abs() <= mathf::EPSILON {
                break;
            }
        }
        self.set_motor_strength(l, num);
    }

    /// LimbBehaviour.SetJointFragility
    pub(crate) fn set_joint_fragility(&mut self, l: usize) {
        if !self.limbs[l].HasJoint {
            return;
        }
        let h = self.limbs[l].Joint.unwrap();
        let ph = &self.physicals[self.limbs[l].PhysicalBehaviour];
        if ph.isDisintegrated {
            self.set_hinge_break_force(h, 0.0);
            return;
        }
        let limb = &self.limbs[l];
        // One float expression: Mono evaluates the chain in double.
        let mut num = (d(limb.BreakingThreshold) * 120.0f32 * mathf::clamp(1.0 - self.skins[l].rottenProgress, 0.1, 1.0)
            * self.mass_strength_ratio(l))
        .f();
        num /= self.prefs.FragilityMultiplier;
        if !limb.IsAndroid {
            num *= mathf::clamp(mathf::map_range(-100.0, 0.0, 0.1, 1.0, ph.Temperature), 0.1, 1.0);
        }
        if num.is_infinite() {
            num = f32::MAX;
        }
        let force = num * (if limb.IsAndroid { 80.0 } else { 2.5 });
        self.set_hinge_break_force(h, force);
        self.set_hinge_break_torque(h, num * 5000.0);
    }

    /// LimbBehaviour.CalculateJointStress
    pub(crate) fn calculate_joint_stress(&mut self, l: usize) {
        let Some(h) = self.limbs[l].Joint else { return };
        if self.limbs[l].HasJoint && !self.limbs[l].Broken && self.hinges[h].useLimits {
            let angle = self.hinges[h].joint_angle(&self.world);
            let limits = self.limbs[l].originalJointLimits;
            if angle - 5.0 > limits[1] || angle + 5.0 < limits[0] {
                self.limbs[l].JointStress += if self.limbs[l].IsAndroid { 0.1 } else { 0.25 };
            }
            if self.limbs[l].JointStress > self.limbs[l].BreakingThreshold * self.mass_strength_ratio(l) {
                self.break_bone(l);
            }
            self.limbs[l].JointStress *= 0.982;
        }
    }

    /// LimbBehaviour.BreakBone
    pub fn break_bone(&mut self, l: usize) {
        if !self.limbs[l].Broken {
            self.break_bone_internal(l);
        }
    }

    /// LimbBehaviour.BreakBoneInternal
    pub(crate) fn break_bone_internal(&mut self, l: usize) {
        self.limbs[l].Broken = true;
        if !self.limbs[l].HasJoint {
            return;
        }
        let person = self.limbs[l].Person;
        if self.nodes[l].IsConnectedToRoot {
            self.people[person].ShockLevel = (d(self.people[person].ShockLevel) + d(self.random.value()) * 5.0f32).f();
            let i = self.random.value() * 150.0;
            self.person_wince(person, i);
        }
        if self.limbs[l].IsLethalToBreak {
            let damage = self.limbs[l].Health + 1.0;
            self.limb_damage(l, damage);
        }
        if self.random.value() as f64 > 0.9 {
            self.circulations[l].InternalBleedingIntensity += self.random.value();
        }
        // PhysicalBehaviour.PlayClipOnce(Person.BoneBreakClips.PickRandom())
        let clip = self.pick_clip(self.people[person].id, "BoneBreakClips", self.people[person].BoneBreakClips);
        self.play_clip_once(self.limbs[l].PhysicalBehaviour, clip, 1.0);
        let h = self.limbs[l].Joint.unwrap();
        if self.hinges[h].useLimits {
            let [min, max] = self.hinges[h].limits;
            let max = mathf::lerp(max, 180.0, 0.5);
            let min = mathf::lerp(min, -180.0, 0.5);
            self.hinges[h].set_limits(&mut self.world, min, max);
        }
    }

    /// LimbBehaviour.Awake. The component links (Skin, Physical, Circulation, Node, Grip, GoreString, Collider, Joint,
    /// ShatteredObjectGenerators) are the shared arena index and the fields of this limb. Not modelled: the LimbStatus UI
    /// clone's own state (LimbStatusBehaviour holds no simulation data).
    pub fn limb_awake(&mut self, l: usize) {
        // Color = Color.white (set_Color: the SpriteRenderer's colour too).
        self.limb_set_color(l, [1.0; 4]);
        // myStatus = Instantiate(LimbStatus, transform): the clone's objects take instance ids.
        for _ in 0..LIMB_STATUS_OBJECTS {
            self.allocate_instance_id();
        }
        self.limbs[l].InitialHealth = self.limbs[l].Health;
        // LimbStatusBehaviour.Awake: SetParent(null) (world scale kept: the person's and the limb's local scales now).
        let [px, py, _] = self.people.get(self.limbs[l].Person).map_or([1.0; 3], |p| p.localScale);
        let lp = self.limbs[l].PhysicalBehaviour;
        self.limbs[l].status_scale = [px * self.physicals[lp].transform.localScale[0], py * self.physicals[lp].transform.localScale[1]];
        self.limbs[l].HasJoint = self.limbs[l].Joint.is_some();
        self.limbs[l].randomOffset = self.random.range_int(-10000, 10000) as f32;
        if self.limbs[l].HasJoint {
            self.limb_setup_joint(l);
        }
        self.limbs[l].InternalTemperature = self.limbs[l].BodyTemperature;
        let p = self.limbs[l].PhysicalBehaviour;
        self.physicals[p].Temperature = self.limbs[l].BodyTemperature;
    }

    /// LimbBehaviour.SetupJoint: useMotor = true, then motor = (speed 0, torque MotorStrength). The motor setter forces
    /// useMotor on and enables the b2 motor first (waking both bodies), so it covers the first assignment too.
    pub(crate) fn limb_setup_joint(&mut self, l: usize) {
        let torque = self.motor_strength(l);
        let h = self.limbs[l].Joint.unwrap();
        self.hinges[h].set_motor(&mut self.world, 0.0, torque);
    }

    /// LimbBehaviour.Start. Not modelled: the undo history lookup (UndoControllerBehaviour.FindRelevantAction),
    /// LimbBehaviourManager.Limbs.Add and the OnDisintegration subscription (Disintegrate calls `limb_on_disintegration`
    /// directly).
    pub fn limb_start(&mut self, l: usize) {
        self.limbs[l].started = true;
        // LimbBehaviourManager.static setup: the empty Limbs list.
        // LimbBehaviourManager.Awake: Limbs.Clear(), a new Game's limb arena.
        // LimbBehaviourManager.GetCollection: Limbs, the limb arena in order.
        // GForceMeasureBehaviour.Awake: rb = GetComponent<Rigidbody2D>() is the limb's body (the gforces entry of the same index).
        // gforce = GetOrAddComponent<GForceMeasureBehaviour>(): adds one (and its id) when the prefab has none; the
        // arena keeps the limb's index.
        // A deferred Start (a disintegrated limb's, run on activation) can come after a later limb's: that limb's slot waits empty.
        while self.gforces.len() < l {
            let k = self.gforces.len();
            self.limbs[k].gforce_pending = true;
            self.gforces.push(Default::default());
        }
        if l == self.gforces.len() || self.limbs[l].gforce_pending {
            self.limbs[l].gforce_pending = false;
            self.allocate_instance_id();
            if l == self.gforces.len() {
                self.gforces.push(Default::default());
            } else {
                self.gforces[l] = Default::default();
            }
            self.behaviours.add(0, super::Behaviour::GForce(l));
        }
        self.limbs[l].IsOnFloor = false;
        self.limb_create_context_menu_options(l);
        if self.limbs[l].Broken {
            self.break_bone_internal(l);
        }
        self.synchronise_dismemberment_call(l, None);
        if self.limbs[l].HasJoint {
            let h = self.limbs[l].Joint.unwrap();
            self.limbs[l].originalJointLimits = self.hinges[h].limits;
        }
        for i in 0..self.limbs[l].ConnectedLimbs.len() {
            let other = self.limbs[l].ConnectedLimbs[i];
            let closer = match self.limbs[l].NearestLimbToBrain {
                None => true,
                Some(n) => self.limbs[other].DistanceToBrain < self.limbs[n].DistanceToBrain,
            };
            if closer && self.limbs[other].DistanceToBrain < self.limbs[l].DistanceToBrain {
                self.limbs[l].NearestLimbToBrain = Some(other);
            }
        }
        if self.limbs[l].goreString {
            self.limbs[l].gore.TissueColour = self.original_blood_color(l);
        }
    }

    /// GoreStringBehaviour.Start. `create_joint_on_start` is the prefab's CreateJointOnStart.
    pub fn gore_string_start(&mut self, l: usize, create_joint_on_start: bool) {
        if create_joint_on_start || self.limbs[l].gore.HadJointWhenSerialised {
            self.gore_string_create_joint(l);
        }
    }

    /// Utils.PickRandom over a list of `count` items: the drawn index.
    #[track_caller]
    pub fn pick_random(&mut self, count: usize) -> Option<usize> {
        if count == 0 {
            return None;
        }
        Some(self.random.range_int(0, count as i32) as usize)
    }

    /// LimbBehaviour.FakeStandUpright
    pub(crate) fn fake_stand_upright(&mut self, l: usize) {
        let p = self.limbs[l].PhysicalBehaviour;
        let t = self.physicals[p].transform;
        let up = t.up();
        if self.physicals[p].rigidbody.bodyType != 0 || !self.nodes[l].IsConnectedToRoot || up[0] * 0.0 + up[1] * -1.0 > 0.0 {
            return;
        }
        let euler = super::dump::euler_z(t.rotation);
        let strength = self.motor_strength(l);
        let mut num = (d(mathf::delta_angle(euler, 0.0))
            * self.limbs[l].FakeUprightForce
            * strength
            * mathf::lerp(self.mass_strength_ratio(l), 1.0, 0.6))
        .f();
        if !num.is_nan() {
            if self.limbs[l].IsAndroid {
                num *= 3.2;
            }
            let m = self.active_pose(self.limbs[l].Person).UprightForceMultiplier;
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_torque(&mut self.world, (d(num) * 1.8 * m).f());
        }
    }

    /// LimbBehaviour.MoveIntoPose(Person.ActivePose)
    pub(crate) fn move_into_pose(&mut self, l: usize, pose: usize, mut speed_modifier: f32, rigidity_multiplier: f32) {
        let person = self.limbs[l].Person;
        let ph = &self.physicals[self.limbs[l].PhysicalBehaviour];
        // `FreezingTemperature + 10f` is compared in double, unrounded.
        let mut num = if ph.Temperature as f64 <= self.limbs[l].FreezingTemperature as f64 + 10.0
            || ph.Temperature >= self.limbs[l].DiscomfortingHeatTemperature
        {
            0.5
        } else {
            1.0
        };
        let p = &self.people[person];
        if p.Poses[pose].State == pose_state::WALKING {
            num *= if p.DesiredWalkingDirection >= 0.0 { 1.0 } else { -1.0 };
        }
        speed_modifier /= mathf::lerp(self.mass_strength_ratio(l), 1.0, 0.9);
        let h = self.limbs[l].Joint.unwrap();
        let joint_angle = self.hinges[h].joint_angle(&self.world);
        let reference = self.hinges[h].reference_angle(&self.world);
        let (time, offset) = (self.time.time, self.limbs[l].randomOffset);
        let p = &mut self.people[person];
        let (angle_offset, global) = (p.AngleOffset, p.GlobalRigidityMultiplier);
        let rp = &mut p.Poses[pose];
        let (speed, rigidity) = (rp.AnimationSpeedMultiplier, rp.Rigidity);
        let i = rp.angle_index(l).unwrap();
        let lp = &mut rp.Angles[i];
        let angle = lp.evaluate_angle(time, offset, (d(speed) * speed_modifier * num).f());
        let modifier = lp.PoseRigidityModifier;
        let value = (d(mathf::delta_angle(joint_angle, (reference + d(angle) * angle_offset).f()))
            * rigidity_multiplier
            * global
            * rigidity
            * (1.0 + d(modifier)))
        .f();
        self.influence_motor_speed(l, value, 0.5);
    }

    /// LimbBehaviour.MoveIntoPoseAt
    pub(crate) fn move_into_pose_at(&mut self, l: usize, pose: usize, time_override: f32, rigidity_multiplier: f32) {
        let person = self.limbs[l].Person;
        let h = self.limbs[l].Joint.unwrap();
        let joint_angle = self.hinges[h].joint_angle(&self.world);
        let reference = self.hinges[h].reference_angle(&self.world);
        let p = &mut self.people[person];
        let (angle_offset, global) = (p.AngleOffset, p.GlobalRigidityMultiplier);
        let rp = &mut p.Poses[pose];
        let rigidity = rp.Rigidity;
        let i = rp.angle_index(l).unwrap();
        let lp = &mut rp.Angles[i];
        let angle = lp.evaluate_angle_at(time_override);
        let value = (d(mathf::delta_angle(joint_angle, (reference + d(angle) * angle_offset).f()))
            * rigidity_multiplier
            * global
            * rigidity
            * (1.0 + d(lp.PoseRigidityModifier)))
        .f();
        self.influence_motor_speed(l, value, 0.5);
    }

    /// LimbBehaviour.ApplyPoseOverrides
    pub(crate) fn apply_pose_overrides(&mut self, l: usize) -> bool {
        let person = self.limbs[l].Person;
        if self.limb_is_capable(l) && self.people[person].IsTouchingFloor && self.active_pose(person).ShouldStumble {
            let num = (self.people[person].BalanceOffset * 1.5).abs();
            let strength = self.motor_strength(l);
            let num2 = mathf::clamp(strength, 0.0, num * 2.0);
            if self.limbs[l].DoBalanceJerk {
                let h = self.limbs[l].Joint.unwrap();
                let joint_angle = self.hinges[h].joint_angle(&self.world);
                let euler = super::dump::euler_z(self.physicals[self.limbs[l].PhysicalBehaviour].transform.rotation);
                let v = (d(mathf::delta_angle(joint_angle, 0.0 - mathf::delta_angle(euler, 0.0))) * 1.0 * self.limbs[l].BalanceMuscleMovement).f();
                self.influence_motor_speed(l, v, 0.5 * num2);
            }
            if self.limbs[l].IsActiveInCurrentPose
                && self.people[person].ShockLevel < 0.5
                && self.limbs[l].DoStumble
                && num > 0.3
                && num < 1.0
            {
                self.limbs[l].stumbleTime += mathf::clamp(self.people[person].BalanceOffset * -0.1, -1.7, 1.7);
                let pose = self.people[person].LinkedPoses[pose_state::STUMBLING as usize];
                let t = self.limbs[l].stumbleTime;
                self.move_into_pose_at(l, pose, t, (d(mathf::pow(num, 1.5)) * 1.3 * num2).f());
                return true;
            }
        }
        false
    }

    /// LimbBehaviour.ManagedFixedUpdate
    pub fn limb_fixed_update(&mut self, l: usize) {
        self.limbs[l].shotHeat = 0.0;
        let p = self.limbs[l].PhysicalBehaviour;
        for i in 0..self.limbs[l].ConnectedLimbs.len() {
            let other = self.limbs[l].ConnectedLimbs[i];
            let op = self.limbs[other].PhysicalBehaviour;
            if !self.physicals[op].isDisintegrated && self.nodes[l].Connections.contains(&Some(other)) {
                self.transfer_energy_fixed_rate(op, p, 0.89);
                self.average_temperature(op, p, 0.01);
            }
        }
        let temperature = self.physicals[p].Temperature;
        let internal = self.limbs[l].InternalTemperature;
        self.physicals[p].Temperature = mathf::lerp(temperature, internal, self.limbs[l].InternalToExternalTempTransferRate);
        let rate = self.limbs[l].ExternalToInternalTempTransferRate;
        self.limbs[l].InternalTemperature =
            mathf::lerp(internal, temperature, if temperature > internal { rate * 0.3 } else { rate });
        let touching = self.is_touching_something(p);
        if touching != self.limbs[l].IsOnFloor {
            self.limbs[l].IsOnFloor = touching;
        }
        let person = self.limbs[l].Person;
        if self.limbs[l].HasBrain {
            if self.circulations[l].BloodFlow < 0.25 && !self.limbs[l].IsAndroid {
                self.people[person].OxygenLevel -= self.time.deltaTime;
            }
            if self.limbs[l].Health < self.limbs[l].InitialHealth * 0.5 {
                for j in 0..self.people[person].Limbs.len() {
                    let other = self.people[person].Limbs[j];
                    if self.limb_is_considered_alive(other) {
                        self.influence_motor_speed(other, 0.0, 0.3);
                    }
                }
            }
            if self.limbs[l].IsZombie {
                self.people[person].BrainDamaged = false;
                self.people[person].Braindead = false;
            } else if self.prefs.BrainDamage
                && !self.limbs[l].IsZombie
                && !self.limbs[l].IsAndroid
                && (self.circulations[l].InternalBleedingIntensity > 0.5 || self.people[person].OxygenLevel <= 0.25)
            {
                let v = self.random.value() > 0.999;
                self.people[person].BrainDamaged |= v;
            }
        }
        if !self.limbs[l].IsZombie && !self.limbs[l].IsAndroid && self.circulations[l].InternalBleedingIntensity > 0.2 {
            let ib = self.circulations[l].InternalBleedingIntensity;
            match self.limbs[l].RoughClassification {
                HEAD => {
                    let damage = (if self.random.value() as f64 > 5.0 / ib as f64 { 20.0 } else { 0.0004 }) * ib;
                    self.limb_damage(l, damage);
                    if self.circulations[l].InternalBleedingIntensity > 0.0 {
                        self.people[person].OxygenLevel = (d(self.people[person].OxygenLevel) - d(0.0001) * ib).f();
                        self.people[person].Consciousness = (d(self.people[person].Consciousness) - d(0.0001) * ib).f();
                    }
                }
                TORSO => self.limb_damage(l, 0.0015 * ib),
                LEGS | ARMS => {
                    self.limbs[l].Numbness = (d(self.limbs[l].Numbness) + d(0.2) * ib).f();
                    self.limb_damage(l, 0.001 * ib);
                }
                _ => {}
            }
            if self.limb_is_considered_alive(l) {
                if self.random.value() > 0.9999 {
                    self.people[person].Consciousness = (d(self.people[person].Consciousness) - d(0.001) * ib).f();
                }
                if self.random.value() as f64 > 0.99 {
                    let i = (d(self.random.value()) * self.circulations[l].InternalBleedingIntensity * 60.0f32).f();
                    self.person_wince(person, i);
                }
            }
        }
        self.calculate_joint_stress(l);
        if (self.limbs[l].InternalTemperature - self.limbs[l].BodyTemperature).abs() >= 3.0 {
            self.limbs[l].Numbness = (d(self.limbs[l].Numbness) + d(0.001) * self.random.value()).f();
            let damage = 0.005 * self.random.value();
            self.limb_damage(l, damage);
        }
        let limb = &self.limbs[l];
        if limb.InternalTemperature >= limb.DiscomfortingHeatTemperature || limb.InternalTemperature <= limb.BodyTemperature - 3.0 {
            if limb.InternalTemperature >= 100.0 {
                self.heal_bleeding(l);
            }
            if self.limbs[l].HasBrain && !self.limb_is_paralysed(l) {
                if self.limbs[l].InternalTemperature > self.limbs[l].BodyTemperature {
                    self.people[person].Consciousness *= 0.9995;
                    self.add_pain(person, 20.0);
                } else {
                    self.people[person].Consciousness *= 0.999;
                }
            }
        }
        if self.physicals[p].Charge() > mathf::EPSILON && !self.limbs[l].IsZombie {
            let damage = self.physicals[p].Charge() * 0.01;
            self.limb_damage(l, damage);
        }
        let threshold = self.properties[self.physicals[p].Properties].BurningTemperatureThreshold;
        if self.physicals[p].Temperature > threshold {
            self.limb_wince(l, 0.1);
            let damage = (d(0.0015) + (d(self.physicals[p].Temperature) - threshold) / 1000.0f32).f();
            self.limb_damage(l, damage);
        } else if self.physicals[p].Temperature < self.limbs[l].FreezingTemperature {
            // 0.5f + randomOffset / 90000f is evaluated in double and compared unrounded.
            if (self.skins[l].acidProgress as f64) < 0.5f64 + self.limbs[l].randomOffset as f64 / 90000.0f64 {
                { let v = self.skins[l].acidProgress + 0.0005; self.skins[l].set_acid(v); }
            }
            self.limb_damage(l, 0.05);
        }
        if self.physicals[p].Wetness > 0.25 && self.limbs[l].IsAndroid {
            let c = self.physicals[p].Charge() + 0.5;
            self.physicals[p].charge = c;
        }
        if self.has_circulation(l) {
            let limb = &self.limbs[l];
            let mut num = if limb.InternalTemperature <= limb.BodyTemperature { 0.05 } else { 0.025 };
            if self.physicals[p].Temperature < limb.FreezingTemperature {
                num *= 0.03;
            }
            let v = mathf::lerp(limb.InternalTemperature, limb.BodyTemperature, num * limb.BodyHeatProductionFactor);
            self.limbs[l].InternalTemperature = v;
        }
        if self.limb_is_considered_alive(l) {
            if self.physicals[p].OnFire {
                let fdt = self.time.fixedDeltaTime;
                { let v = (d(self.skins[l].acidProgress) + d(fdt) * 0.01f32).f(); self.skins[l].set_acid(v); }
                self.physicals[p].BurnProgress = (d(self.physicals[p].BurnProgress) + d(fdt) * 0.01f32).f();
                let z: f32 = if self.limbs[l].IsZombie { 0.01 } else { 1.0 };
                self.limbs[l].Health = (d(self.limbs[l].Health) - d(self.time.deltaTime) * 0.5f32 * z).f();
                if !self.limbs[l].IsZombie && !self.limb_is_paralysed(l) {
                    if self.prefs.StopAnimationOnDamage {
                        self.people[person].OverridePoseIndex = -1;
                    }
                    let pain = self.physicals[p].burnIntensity * self.time.deltaTime;
                    self.add_pain(person, pain);
                }
            }
            if self.prefs.AutoHealWounds && self.limbs[l].RegenerateBurnProgressSpeed > mathf::EPSILON && self.physicals[p].BurnProgress > 0.0 {
                let ph = &mut self.physicals[p];
                ph.BurnProgress = (d(ph.BurnProgress) - d(self.time.fixedDeltaTime) * self.limbs[l].RegenerateBurnProgressSpeed * 0.001f32).f();
                ph.BurnProgress = mathf::max(0.0, ph.BurnProgress);
            }
        }
        let adrenaline = self.people[person].AdrenalineLevel;
        if self.circulations[l].BloodFlow > mathf::max(0.25, 0.9 - mathf::clamp01(adrenaline))
            && self.people[person].IsTouchingFloor
            && self.active_pose(person).ShouldStandUpright
            && self.limb_is_capable(l)
            && (!self.limbs[l].HasBrain || !self.limbs[l].IsDismembered)
        {
            if self.limbs[l].FakeUprightForce > 0.001 {
                self.fake_stand_upright(l);
            }
            if self.physicals[p].rigidbody.bodyType == 0 {
                let drag = self.active_pose(person).DragInfluence;
                let rb = self.physicals[p].rigidbody.clone();
                let av = rb.angular_velocity(&self.world) * mathf::lerp(1.0, 0.92, drag);
                rb.set_angular_velocity(&mut self.world, av);
                let v = rb.velocity(&self.world);
                let k = mathf::lerp(1.0, 0.94, drag);
                rb.set_velocity(&mut self.world, [v[0] * k, v[1] * k]);
            }
        }
        if self.limbs[l].HasJoint {
            if self.limbs[l].Frozen || self.physicals[p].Temperature <= self.limbs[l].FreezingTemperature {
                self.set_motor_strength(l, 10.0);
                self.influence_motor_speed(l, 0.0, 1.0);
            } else {
                if self.limb_is_considered_alive(l) && self.physicals[p].Temperature <= self.limbs[l].BodyTemperature - 15.0 {
                    let v = self.random.range_int(-45, 45) as f32;
                    self.influence_motor_speed(l, v, 0.5);
                }
                self.set_motor_strength_to_muscle_strength(l);
                if !self.apply_pose_overrides(l) && self.limbs[l].IsActiveInCurrentPose && self.limb_is_considered_alive(l) {
                    let pose = self.people[person].ActivePose.unwrap();
                    self.move_into_pose(l, pose, 1.0, 1.0);
                }
            }
            if self.limb_is_considered_alive(l) && self.physicals[p].Charge() > mathf::EPSILON {
                let v = -50.0 * self.people[person].localScale[0];
                let i = self.physicals[p].Charge() * 0.5;
                self.influence_motor_speed(l, v, i);
            }
            if !self.limbs[l].IsZombie || !self.limb_is_considered_alive(l) {
                let r = self.skins[l].rottenProgress;
                self.influence_motor_speed(l, 0.0, r);
            }
        }
        let limb = &self.limbs[l];
        if limb.ImmuneToDamage
            || self.physicals[p].rigidbody.bodyType != 0
            || limb.SpeciesIdentity != "Human"
            || !(limb.GForcePassoutThreshold > mathf::EPSILON)
        {
            return;
        }
        let sqr = mathf::sqr_magnitude(self.gforces[l].sustainedAcceleration);
        if sqr > limb.GForcePassoutThreshold * limb.GForcePassoutThreshold {
            if limb.HasBrain {
                self.people[person].Consciousness *= 0.5;
            }
            if sqr > self.limbs[l].GForceDamageThreshold * self.limbs[l].GForceDamageThreshold {
                self.limb_damage(l, sqr / 250.0);
            }
        }
    }

    /// LimbBehaviour.ManagedUpdate
    pub fn limb_update(&mut self, l: usize) {
        self.set_joint_fragility(l);
        let dt = self.time.deltaTime;
        let flag = self.limbs[l].Health > mathf::EPSILON;
        let person = self.limbs[l].Person;
        if self.limbs[l].HasBrain {
            if self.people[person].Braindead {
                let p = &mut self.people[person];
                p.Consciousness = 0.0;
                p.ShockLevel = 0.0;
                p.PainLevel = 0.0;
                self.limbs[l].Health = 0.0;
            } else {
                self.people[person].Braindead = !flag;
            }
        }
        let limb = &mut self.limbs[l];
        if flag && limb.Health < limb.InitialHealth {
            limb.Health = (d(limb.Health) + d(limb.RegenerationSpeed) * dt).f();
        }
        self.handle_drowning(l);
        self.limbs[l].Numbness = (d(self.limbs[l].Numbness) - d(dt) / 20.0f32).f();
        if !self.nodes[l].IsConnectedToRoot && self.limbs[l].Health > 0.0 {
            let limb = &mut self.limbs[l];
            limb.Health = (d(limb.Health) - d(dt) / 20.0f32 * limb.InitialHealth).f();
        }
        if let Some(g) = self.limbs[l].GripBehaviour {
            if self.grips[g].isHolding && self.prefs.DropOnDeath && !self.limb_is_considered_alive(l) {
                self.grip_drop(g);
            }
        }
        let p = self.limbs[l].PhysicalBehaviour;
        if self.physicals[p].isDisintegrated {
            self.limbs[l].RegenerationSpeed = 0.0;
            self.limbs[l].Health = 0.0;
        }
        let limb = &self.limbs[l];
        if limb.HasLungs && limb.LungsPunctured && self.limb_is_considered_alive(l) && !limb.IsZombie && !limb.IsAndroid {
            self.add_pain(person, 2.0);
            self.people[person].OxygenLevel = (d(self.people[person].OxygenLevel) - d(dt) * 0.5f32).f();
        }
        if self.is_being_stabbed(p) {
            let mut i = 0;
            while i < self.physicals[p].beingStabbedBy.len() {
                let stabber = self.physicals[p].beingStabbedBy[i];
                i += 1;
                if !self.physicals[stabber].destroyed && self.physicals[stabber].StabCausesWound && self.get_relative_stab_speed(stabber, p) > 0.05 {
                    let point = self.get_global_stab_point(stabber, p);
                    let fallback = self.original_blood_color(l);
                    let colour = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
                    // SendMessage skips an inactive stabber.
                    if self.physicals[stabber].active {
                        let ty = self.limb_blood_decal(l);
                        self.physical_decal(stabber, ty, point, 1.0, colour);
                    }
                }
            }
        }
        let limb = &mut self.limbs[l];
        if limb.Health > mathf::EPSILON && limb.previousHealth <= mathf::EPSILON {
            limb.KillShotParticlesEmitted = false;
        }
        limb.previousHealth = limb.Health;
    }

    /// LimbBehaviour.HandleDrowning
    pub(crate) fn handle_drowning(&mut self, l: usize) {
        if !self.nodes[l].IsConnectedToRoot || self.limbs[l].IsAndroid {
            return;
        }
        let dt = self.time.deltaTime;
        let person = self.limbs[l].Person;
        if self.people[person].OxygenLevel < 0.05 && !self.limbs[l].IsZombie {
            self.limb_damage(l, dt * 4.0);
        }
        if !self.limbs[l].HasBrain || self.physicals[self.limbs[l].PhysicalBehaviour].Wetness < 0.5 {
            return;
        }
        self.people[person].OxygenLevel = (d(self.people[person].OxygenLevel) - d(dt) / 40.0f32).f();
        if self.people[person].OxygenLevel < 0.9 {
            self.people[person].Consciousness = (d(self.people[person].Consciousness) - d(dt) / 40.0f32 * 0.5f32).f();
            if !self.limbs[l].IsZombie {
                self.add_pain(person, dt * 0.9);
            }
        }
    }
}

impl Game {
    /// SkinMaterialHandler.AddDamagePoint
    pub fn skin_add_damage_point(&mut self, l: usize, ty: i32, global_position: [f32; 2], intensity: f32) {
        if self.skins[l].currentDamagePointCount < 128 && !(intensity <= mathf::EPSILON) {
            let intensity = intensity * self.skins[l].intensityMultiplier;
            self.skin_internal_add_damage_point(l, ty, global_position, intensity);
            for a in self.skins[l].adjacentLimbs.clone() {
                self.skin_internal_add_damage_point(a, ty, global_position, intensity);
            }
        }
    }

    /// SkinMaterialHandler.GetNearEntry: the first damage point of this type (or an almost empty one) within minDistance;
    /// it takes the type.
    pub(crate) fn skin_get_near_entry(&mut self, l: usize, local: [f32; 3], min_distance: f32, ty: i32) -> Option<usize> {
        let skin = &mut self.skins[l];
        let num = min_distance * min_distance;
        for index in 0..skin.currentDamagePointCount as usize {
            let p = skin.damagePoints[index];
            let dz = local[2] - 0.0;
            let sqr = (d(local[0] - p[0]) * (local[0] - p[0]) + d(local[1] - p[1]) * (local[1] - p[1]) + d(dz) * dz).f();
            if (mathf::floor(p[3]) as i32 == ty || p[2] <= 0.01) && sqr <= num {
                skin.damagePoints[index][3] = ty as f32;
                return Some(index);
            }
        }
        None
    }

    /// SkinMaterialHandler.ImpactDamage (obsolete): AddDamagePoint(Blunt, ...).
    #[allow(dead_code)]
    pub fn skin_impact_damage(&mut self, l: usize, intensity: f32, global_position: [f32; 2]) {
        self.skin_add_damage_point(l, super::misc::damage_type::BLUNT, global_position, intensity);
    }

    /// SkinMaterialHandler.ShotDamage (obsolete): AddDamagePoint(Bullet, ...).
    #[allow(dead_code)]
    pub fn skin_shot_damage(&mut self, l: usize, intensity: f32, global_position: [f32; 2]) {
        self.skin_add_damage_point(l, super::misc::damage_type::BULLET, global_position, intensity);
    }

    /// SkinMaterialHandler.ClearAllDamage: both progresses back to 0 and every damage point empty (Sync is rendering).
    #[allow(dead_code)]
    pub fn skin_clear_all_damage(&mut self, l: usize) {
        self.skins[l].set_rotten(0.0);
        self.skins[l].set_acid(0.0);
        for p in self.skins[l].damagePoints.iter_mut() {
            *p = [0.0, 0.0, 0.0, super::misc::damage_type::NONE as f32];
        }
    }

    /// SkinMaterialHandler.Stabbed: a stabber that causes wounds adds a Stab damage point of 8 * StabWoundSizeMultiplier.
    pub fn skin_stabbed(&mut self, l: usize, stabber: usize, point: [f32; 2]) {
        if self.physicals[stabber].StabCausesWound {
            let size = (d(8.0) * self.physicals[stabber].StabWoundSizeMultiplier).f();
            self.skin_add_damage_point(l, super::misc::damage_type::STAB, point, size);
        }
    }

    /// LimbBehaviour.OnFlip: a live joint swaps and negates its original limits.
    pub fn limb_on_flip(&mut self, l: usize) {
        if self.limbs[l].HasJoint && self.limbs[l].Joint.is_some_and(|h| !self.hinges[h].destroyed) {
            let [x, y] = self.limbs[l].originalJointLimits;
            self.limbs[l].originalJointLimits = [-y, -x];
        }
    }

    /// SkinMaterialHandler.InternalAddDamagePoint (Sync only uploads to the material).
    pub(crate) fn skin_internal_add_damage_point(&mut self, l: usize, ty: i32, global_position: [f32; 2], intensity: f32) {
        use super::misc::damage_type::*;
        if !(self.skins[l].currentDamagePointCount < 128 && !(intensity <= mathf::EPSILON)) {
            return;
        }
        let intensity = mathf::clamp(intensity, 0.0, 100.0);
        let local = self.inverse_transform_point(self.limbs[l].PhysicalBehaviour, [global_position[0], global_position[1], 0.0]);
        let point_intensity = intensity * 0.1;
        let min_distance = match ty {
            BLUNT => 0.1 * point_intensity,
            BULLET => 0.01 * mathf::min(point_intensity, 5.0),
            STAB => 0.02 * point_intensity,
            DISMEMBERMENT => 0.05,
            BURN => 0.015,
            _ => 0.0,
        };
        let near = self.skin_get_near_entry(l, local, min_distance, ty);
        let skin = &mut self.skins[l];
        match near {
            Some(index) => skin.damagePoints[index][2] = mathf::max(skin.damagePoints[index][2], point_intensity),
            None => {
                let n = skin.currentDamagePointCount as usize;
                skin.damagePointTimeStamps[n] = self.time.time;
                skin.damagePoints[n] = [local[0], local[1], point_intensity, ty as f32];
                skin.currentDamagePointCount += 1;
            }
        }
    }

    /// LimbBehaviour.OnCollisionEnter2D
    pub fn limb_on_collision_enter(&mut self, l: usize, c: &ScriptCollision) {
        use super::misc::damage_type::*;
        let mut buffer = self.limb_contact_buffer;
        let contacts = get_contacts(c, &mut buffer);
        self.limb_contact_buffer = buffer;
        let mut num = (d(get_average_impulse_remove_outliers(&buffer, contacts)) / self.mass_strength_ratio(l) * self.prefs.FragilityMultiplier * 0.8f32).f();
        let normal = [buffer[0].normal.x, buffer[0].normal.y];
        let point = [buffer[0].point.x, buffer[0].point.y];
        if self.limbs[l].IsAndroid {
            num *= 0.1;
        }
        if let Some(v) = self.collision_physical(c) {
            let value = &self.physicals[v];
            if value.SimulateTemperature && value.Temperature >= 70.0 && !self.limbs[l].IsAndroid {
                let temperature = value.Temperature;
                self.limb_damage(l, temperature / 140.0);
                self.skin_add_damage_point(l, BURN, point, temperature * 0.01);
                if self.limb_is_considered_alive(l) && !self.limb_is_paralysed(l) {
                    self.add_pain(self.limbs[l].Person, 1.0);
                }
                self.limb_wince(l, 150.0);
                if temperature >= 100.0 {
                    self.heal_bleeding(l);
                }
            }
        }
        if self.limbs[l].HasBrain && num > 0.6 && self.random.value() as f64 > 0.8 {
            self.circulations[l].InternalBleedingIntensity += num;
        }
        if num < 2.0 {
            return;
        }
        self.limbs[l].BruiseCount = self.limbs[l].BruiseCount.wrapping_add(1);
        self.propagate_impact_damage(l, num, normal, point, 0, l);
        if !(num < 1.0) && !self.limbs[l].IsAndroid && !(self.amount_of_blood(l) < 0.2) {
            if self.random.value() > 0.8 {
                self.cut(l, point, normal);
            }
            if !(num < 3.0) || !(self.limbs[l].Health > self.limbs[l].InitialHealth * 0.2) {
                let p = self.limbs[l].PhysicalBehaviour;
                self.create_impact_effect(p, point, normal, mathf::clamp(num / 4.0, 1.0, 2.0));
                let fallback = self.original_blood_color(l);
                let colour = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
                if let Some(other) = self.collision_physical(c) {
                    let ty = self.limb_blood_decal(l);
                    self.physical_decal(other, ty, point, 1.0, colour);
                } else if self.bare_colliders.get(&c.collider).is_some_and(|b| b.receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"])) {
                    unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                }
            }
        }
    }

    /// LimbBehaviour.PropagateImpactDamage
    pub(crate) fn propagate_impact_damage(&mut self, l: usize, impulse: f32, direction: [f32; 2], pos: [f32; 2], iteration: i32, origin: usize) {
        self.act_on_impact(l, impulse, [pos[0], pos[1], 0.0]);
        if iteration >= 8 {
            return;
        }
        for i in 0..self.limbs[l].ConnectedLimbs.len() {
            let other = self.limbs[l].ConnectedLimbs[i];
            if other == origin {
                continue;
            }
            let (a, b) = (self.physicals[self.limbs[other].PhysicalBehaviour].transform.position, self.physicals[self.limbs[l].PhysicalBehaviour].transform.position);
            let num = dot2([a[0] - b[0], a[1] - b[1]], direction);
            if num > 0.0 {
                self.propagate_impact_damage(other, (d(num) * impulse * 0.9f32).f(), direction, pos, iteration + 1, l);
            }
        }
    }

    /// LimbBehaviour.OnCollisionStay2D
    pub fn limb_on_collision_stay(&mut self, l: usize, c: &ScriptCollision) {
        use super::misc::damage_type::*;
        if self.limbs[l].IsAndroid {
            return;
        }
        let mut buffer = self.limb_contact_buffer;
        let contacts = get_contacts(c, &mut buffer);
        self.limb_contact_buffer = buffer;
        let first_valid_contact = get_first_valid_contact(&buffer, contacts);
        let mass_strength_ratio = self.mass_strength_ratio(l);
        let sqr = mathf::sqr_magnitude([c.relative_velocity.x, c.relative_velocity.y]);
        if d(sqr) * self.prefs.FragilityMultiplier / mass_strength_ratio > d(self.limbs[l].FrictionBurnWoundMinSpeedSqrd) {
            self.limb_damage(l, 1.0);
            self.skin_add_damage_point(l, BURN, [first_valid_contact.point.x, first_valid_contact.point.y], 2.0);
        }
        let p = self.limbs[l].PhysicalBehaviour;
        if self.prefs.LimbCrushing && self.physicals[p].rigidbody.bodyType != 2 {
            let threshold = d(mathf::max(10.0, self.limbs[l].BreakingThreshold))
                * mathf::lerp(self.positionIterations as f32 / 16.0, 1.0, 0.2);
            let force = d(get_min_impulse(&buffer, contacts)) * self.prefs.CrushForceMultiplier / mass_strength_ratio * self.prefs.FragilityMultiplier;
            if !(force < threshold) {
                let fallback = self.original_blood_color(l);
                let colour = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
                if let Some(other) = self.collision_physical(c) {
                    let ty = self.limb_blood_decal(l);
                    let at = self.physicals[p].transform.position;
                    self.physical_decal(other, ty, [at[0], at[1]], 1.0, colour);
                } else if self.bare_colliders.get(&c.collider).is_some_and(|b| b.receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"])) {
                    unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                }
                self.limb_crush(l);
            }
        }
    }

    /// LimbBehaviour.ActOnImpact
    pub fn act_on_impact(&mut self, l: usize, impulse: f32, global_position: [f32; 3]) {
        use super::misc::damage_type::*;
        if self.limbs[l].ImmuneToDamage {
            return;
        }
        let limb = &self.limbs[l];
        let num = (d(limb.BreakingThreshold) * self.mass_strength_ratio(l) / limb.ImpactDamageMultiplier).f();
        if impulse > num && self.random.value() > 0.2 {
            self.break_bone(l);
        }
        let num2 = mathf::max(1.0, self.limbs[l].Vitality);
        if d(impulse) > d(num) * 0.5f32 / num2 && d(self.random.value()) > d(0.8) / num2 {
            self.circulations[l].InternalBleedingIntensity += mathf::clamp(impulse * num2, 0.0, 1.0);
        }
        let person = self.limbs[l].Person;
        if !self.limbs[l].IsAndroid {
            if self.prefs.BrainDamage && !self.limbs[l].IsZombie && self.limbs[l].HasBrain && self.random.value() > 0.991 && impulse > 4.0 {
                self.people[person].BrainDamaged = true;
            } else {
                let damage = (d(impulse) * impulse * impulse * 2.8f32 * self.limbs[l].ImpactDamageMultiplier).f();
                self.limb_damage(l, damage);
            }
        }
        self.skin_add_damage_point(l, BLUNT, [global_position[0], global_position[1]], impulse * 4.0);
        let num3 = (d(impulse) * (d(self.limbs[l].Vitality) + 1.0f32) * 0.25f32).f();
        if !self.people[person].BrainDamaged && self.limbs[l].HasBrain && num3 > 1.0 && !self.limbs[l].IsAndroid && self.random.value() > 0.5 {
            let r = self.random.value();
            self.people[person].Consciousness = (d(self.people[person].Consciousness) * (d(r) * 0.8f32)).f();
        }
        if self.limb_is_considered_alive(l) && num3 > 0.2 && !self.limbs[l].IsZombie {
            self.people[person].ShockLevel = (d(self.people[person].ShockLevel) + d(num3) * 0.04f32).f();
        }
    }

    /// LimbBehaviour.set_Color: color = value, then GetComponent<SpriteRenderer>().color = value (the limb's PhysicalBehaviour
    /// holds its SpriteRenderer).
    pub fn limb_set_color(&mut self, l: usize, value: [f32; 4]) {
        self.limbs[l].Color = value;
        let p = self.limbs[l].PhysicalBehaviour;
        if let Some(sprite) = self.physicals[p].sprite.as_mut() {
            sprite.1 = value;
        }
    }

    /// SkinMaterialHandler.IsSkinVisible: acidProgress < 0.62 and the limb's PhysicalBehaviour.BurnProgress < 0.45.
    pub(crate) fn skin_is_skin_visible(&self, l: usize) -> bool {
        if self.skins[l].acidProgress < 0.62 {
            return self.physicals[self.limbs[l].PhysicalBehaviour].BurnProgress < 0.45;
        }
        false
    }

    /// SkinMaterialHandler.IsFleshVisible: acidProgress < 0.85 and the limb's PhysicalBehaviour.BurnProgress < 0.78.
    pub(crate) fn skin_is_flesh_visible(&self, l: usize) -> bool {
        if self.skins[l].acidProgress < 0.85 {
            return self.physicals[self.limbs[l].PhysicalBehaviour].BurnProgress < 0.78;
        }
        false
    }

    /// LimbBehaviour.ShatterProcedurally(shatterFlags): Random.value is drawn first.
    pub(crate) fn limb_shatter_procedurally(&mut self, l: usize, shatter_flags: u8) {
        let v = self.random.value();
        let Some(gens) = self.limbs[l].ShatteredObjectGenerators else { return };
        if self.limbs[l].ShatteredObjectChance < v || !self.prefs.ProceduralFragments {
            return;
        }
        let p = self.limbs[l].PhysicalBehaviour;
        let brightness = mathf::min(1.0 - self.physicals[p].BurnProgress, 1.0 - self.skins[l].rottenProgress * 0.5);
        // ShatterFlags.Bone/Flesh/Skin = 1/2/4 (All = 255); shatterFlags & ~CurrentlyShattered.
        let flags = shatter_flags & !self.limbs[l].CurrentlyShattered;
        for (k, flag) in [1u8, 2, 4].into_iter().enumerate() {
            let Some(gi) = gens[k] else { continue };
            let visible = match k {
                1 => self.skin_is_flesh_visible(l),
                2 => self.skin_is_skin_visible(l),
                _ => true,
            };
            if flags & flag == 0 || !visible {
                continue;
            }
            let r = self.random.range(0.0, 0.1);
            self.shatter_generators[gi].g.ConnectToRange += r;
            self.shatter_generators[gi].g.Brightness = brightness;
            self.limb_generate_gore_bits(l, gi);
            self.limbs[l].CurrentlyShattered |= flag;
        }
    }

    /// `PrepareGoreBits(array, generator.Generate(base.transform, PhysicalBehaviour.rigidbody, array))`.
    fn limb_generate_gore_bits(&mut self, l: usize, gi: usize) {
        let (count, buffer) = self.limb_generator_generate(l, gi);
        self.limb_prepare_gore_bits(l, count, buffer);
    }

    /// `generator.Generate(base.transform, PhysicalBehaviour.rigidbody, array)` with array = new GameObject[PartCount]:
    /// the count and the buffer.
    pub(crate) fn limb_generator_generate(&mut self, l: usize, gi: usize) -> (i32, Vec<Option<usize>>) {
        let mut st = std::mem::take(&mut self.shatter);
        let mut g = self.shatter_generators[gi].g.clone();
        // The shards' PhysicalBehaviour.Properties asset joins the arena when first used.
        let props = self.shatter_generators[gi].properties.clone();
        g.PartsPhysicalProperties = match self.properties.iter().position(|q| q.name == props.name) {
            Some(i) => i,
            None => {
                self.properties.push(props);
                self.properties.len() - 1
            }
        };
        let p = self.limbs[l].PhysicalBehaviour;
        let t = self.physicals[p].transform;
        let to_replace = super::shatter::ToReplace { position: t.position, rotation: t.rotation, lossy_scale: self.lossy_scale(p) };
        let rb = &self.physicals[p].rigidbody;
        let source = super::shatter::ShatterSource { name: self.physicals[p].name.clone(), velocity: rb.velocity(&self.world), angular_velocity: rb.angular_velocity(&self.world) };
        let mut buffer = vec![None; g.PartCount.max(0) as usize];
        let count = self.shattered_object_generate(&mut st, &g, Some(to_replace), Some(&source), Some(&mut buffer));
        self.shatter = st;
        (count, buffer)
    }

    /// LimbBehaviour.PrepareGoreBits(array, count).
    fn limb_prepare_gore_bits(&mut self, l: usize, count: i32, buffer: Vec<Option<usize>>) {
        // LimbBehaviour.PrepareGoreBits: GetComponentInParent<DisintegrationCounterBehaviour>() (the person's) and
        // GetOriginalBloodType().Color (the helper's Color), then per shard:
        let blood_color = self.original_blood_color(l);
        let person = self.limbs[l].Person;
        for shard in buffer.iter().take(count.max(0) as usize).map(|s| s.expect("a creation buffer slot Generate filled")) {
            // DisintegrationCounterBehaviour.RegisterPseudoChild: children grows by the shard (its OnDisintegration listener is the pseudo_children walk in physical.rs)
            // DestroyWith.Add(g[i]); RegisterPseudoChild(g[i].GetComponent<PhysicalBehaviour>())
            let id = self.physicals[shard].id;
            self.limbs[l].destroy_with.push(id);
            self.people[person].pseudo_children.push(shard);
            if let Some(c) = &mut self.people[person].counter_children {
                c.push(shard);
            }
            // g[i].AddComponent<PseudoBloodImpactHelper>().Color = color: a new component on the shard's GameObject.
            let helper = self.allocate_instance_id();
            self.with_component_instance(id, |_, inst, o| {
                let mut prefab = (*inst.prefab).clone();
                let n = prefab.objects[o].components.len() as i64;
                prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, crate::assets::db::Component::Script { class: "PseudoBloodImpactHelper".into(), execution_order: 0, enabled: true, fields: crate::assets::mono::Value::Struct(Vec::new()) }));
                inst.prefab = std::sync::Arc::new(prefab);
                inst.objects[o].components.push(helper);
            });
            self.pseudo_blood_helpers.push((helper, blood_color));
        }
    }

    /// LimbBehaviour.Crush
    pub fn limb_crush(&mut self, l: usize) {
        let p = self.limbs[l].PhysicalBehaviour;
        if self.physicals[p].isDisintegrated || self.limbs[l].ImmuneToDamage {
            return;
        }
        let person = self.limbs[l].Person;
        if self.nodes[l].IsConnectedToRoot {
            if self.prefs.StopAnimationOnDamage {
                self.people[person].OverridePoseIndex = -1;
            }
            self.add_pain(person, 25.0);
        }
        if !self.prefs.GorelessMode {
            // Instantiate(BloodExplosionPrefab, transform.position, identity). Awake in hierarchy order: the
            // root's ExplosionSoundBehviour (audio source registration), then the ActivateIfFancyEffects of
            // Debris 2, extraDecals and chunks each draw `Chance(1) > Random.value` before reading FancyEffects.
            // The particle systems draw from their own generators, not UnityEngine.Random.
            let position = self.physicals[p].transform.position;
            let _debris2 = 1.0 > self.random.value() && self.prefs.FancyEffects;
            let extra_decals = 1.0 > self.random.value() && self.prefs.FancyEffects;
            let _chunks = 1.0 > self.random.value() && self.prefs.FancyEffects;
            self.pending_starts.push(super::misc::PendingStart::BloodExplosionSound);
            // extraDecals: a circle of 25..32 drops from the explosion (its parent at Start).
            let splat = extra_decals.then(|| {
                self.splats.push(super::misc::FancyBloodSplat {
                    UnparentAtStart: true,
                    DirectionMode: 2,
                    Continuous: false,
                    Direction: [0.0, 1.0],
                    SpawningChance: 0.1,
                    AmountRange: [25.0, 32.0],
                    AngleRandomisation: 11.25,
                    LifetimeInSeconds: [1.0, 2.0],
                    EjectionForce: [0.0, 5.0],
                    DecalColourMultiplier: [f32::from_bits(0x3f169697), 0.0, 0.0, 1.0],
                    Size: 0.3,
                    DestroyWhenNoParticles: true,
                    TriggerOnCollide: false,
                    on_collide: None,
                    on_hole: None,
                    Decal: "Sparse_blood".into(),
                    particles: Default::default(),
                    parent_position: position,
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
                self.pending_starts.push(super::misc::PendingStart::Splat(s));
                s
            });
            // DeleteAfterTime (5 s) only removes the explosion's own objects.
            if !self.limbs[l].IsAndroid {
                // BloodExplosionBehaviour.SetColor: the particles' start colour and the splat's decal colour.
                let fallback = self.original_blood_color(l);
                let color = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
                if let Some(s) = splat {
                    self.splats[s].DecalColourMultiplier = color;
                }
            }
            self.limb_shatter_procedurally(l, 255);
        }
        if !self.physicals[p].isDisintegrated {
            self.physical_disintegrate(p);
        }
    }
}

impl Game {
    /// SendMessage("OnJointBreak2D", joint) for hinge `h`, to the limb GameObject that owns it.
    /// Receivers in component order (Person.prefab, every limb): PhysicalBehaviour, LimbBehaviour,
    /// GoreStringBehaviour; disabled behaviours receive it too. The caller destroys the joint
    /// afterwards (Hinge.destroyed), so everything here still sees it intact.
    pub fn joint_break_messages(&mut self, h: usize) {
        let Some(l) = self.limbs.iter().position(|limb| limb.Joint == Some(h)) else { return };
        // PhysicalBehaviour.OnJointBreak2D: only matches `penetration.Joint` (a SliderJoint2D), never a hinge.
        // LimbBehaviour.OnJointBreak2D
        self.limb_on_joint_break(l, h);
        // GoreStringBehaviour.OnJointBreak2D: acts only when `joint == Joint` (its own SpringJoint2D), never a hinge.
    }

    /// ConnectedNodeBehaviour.DisconnectFrom(other): other.SetToNull(this), then this.SetToNull(other).
    pub(crate) fn node_disconnect_from(&mut self, node: usize, other: usize) {
        self.node_set_to_null(other, node);
        self.node_set_to_null(node, other);
    }

    /// ConnectedNodeBehaviour.SetToNull
    pub(crate) fn node_set_to_null(&mut self, node: usize, other: usize) {
        if let Some(c) = self.nodes[node].Connections.iter_mut().find(|c| **c == Some(other)) {
            *c = None;
        }
    }

    /// connectedBody.GetComponent<LimbBehaviour>() (a Rigidbody2D's body belongs to the one limb that owns it).
    pub fn limb_of_body(&self, b: BodyId) -> Option<usize> {
        (0..self.limbs.len()).find(|&l| self.physicals[self.limbs[l].PhysicalBehaviour].rigidbody.body == b)
    }

    /// `limb.Joint.connectedBody == other.PhysicalBehaviour.rigidbody`
    fn hinge_connected_to(&self, h: usize, other: usize) -> bool {
        self.hinges[h].connected == Some(self.physicals[self.limbs[other].PhysicalBehaviour].rigidbody.body)
    }

    /// LimbBehaviour.OnJointBreak2D (joint == Joint)
    pub(crate) fn limb_on_joint_break(&mut self, l: usize, h: usize) {
        // connectedBody.GetComponent<LimbBehaviour>(); a limb is also the only owner of a ConnectedNodeBehaviour here.
        let component = self.hinges[h].connected.and_then(|b| self.limb_of_body(b));
        self.circulation_on_joint_break(l, Some(h));
        if self.limbs[l].IsLethalToBreak {
            self.limbs[l].Health = 0.0;
        }
        if self.nodes[l].IsConnectedToRoot && !self.limb_is_paralysed(l) {
            let person = self.limbs[l].Person;
            self.add_pain(person, 15.0);
        }
        if let Some(component) = component {
            self.node_disconnect_from(l, component);
            if let Some(i) = self.limbs[l].ConnectedLimbs.iter().position(|&x| x == component) {
                self.limbs[l].ConnectedLimbs.remove(i);
            }
        }
        let p = self.limbs[l].PhysicalBehaviour;
        if !self.prefs.GorelessMode
            && self.prefs.DismembermentLooseTissue
            && self.limbs[l].goreString
            && self.random.value() > 0.5
            && self.limbs[l].goreStringEnabled
            && !self.limbs[l].goreStringJoint
            && self.skins[l].acidProgress < 0.7
            && self.physicals[p].BurnProgress < 0.8
        {
            self.gore_string_create_joint(l);
        }
        self.limbs[l].IsDismembered = true;
        let vector = self.transform_point(p, self.hinges[h].anchor);
        let up = self.transform_up(p);
        self.create_impact_effect(p, vector, [up[0], up[1]], 2.0);
        self.skin_add_damage_point(l, super::misc::damage_type::DISMEMBERMENT, vector, 25.0);
        let clips = self.people[self.limbs[l].Person].DismembermentClips;
        // PhysicalBehaviour.PlayClipOnce(Person.DismembermentClips.PickRandom())
        let clip = self.pick_clip(self.people[self.limbs[l].Person].id, "DismembermentClips", clips);
        self.play_clip_once(p, clip, 1.0);
        self.synchronise_dismemberment(l, component);
    }

    /// LimbBehaviour.SynchroniseDismemberment(connectedLimb) from its start: nothing unless IsDismembered, then
    /// Destroy(Joint) if (bool)Joint and the shared tail.
    pub(crate) fn synchronise_dismemberment_call(&mut self, l: usize, connected: Option<usize>) {
        if !self.limbs[l].IsDismembered {
            return;
        }
        self.limb_destroy_joint(l);
        self.synchronise_dismemberment(l, connected);
    }

    /// `if ((bool)Joint) Object.Destroy(Joint);` (SynchroniseDismemberment's start).
    pub(crate) fn limb_destroy_joint(&mut self, l: usize) {
        if let Some(h) = self.limbs[l].Joint.filter(|&h| !self.hinges[h].destroyed) {
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
            self.hinges[h].destroy_at.get_or_insert(at);
            // DestroyObjectFromScripting (1808cd940) disables the Behaviour at once.
            if let Some(j) = self.hinges[h].joint.take() {
                self.world.destroy_joint(j);
            }
        }
    }

    /// SynchroniseDismemberment's tail after Destroy(Joint) (IsDismembered is true here;
    /// OnJointBreak2D's joint is destroyed by the break itself)
    pub(crate) fn synchronise_dismemberment(&mut self, l: usize, connected: Option<usize>) {
        // Destroy(Joint): the break already destroys the joint; Start queues it before calling this.
        self.limbs[l].HasJoint = false;
        let remove_first = |v: &mut Vec<usize>, x: usize| {
            if let Some(i) = v.iter().position(|&y| y == x) {
                v.remove(i);
            }
        };
        let mut i = 0;
        while i < self.limbs[l].ConnectedLimbs.len() {
            let o = self.limbs[l].ConnectedLimbs[i];
            let attached = self.limbs[o].HasJoint && {
                let oh = self.limbs[o].Joint.expect("MissingReferenceException: HasJoint without a Joint");
                assert!(!self.hinges[oh].destroyed, "MissingReferenceException: connected limb's Joint was destroyed");
                self.hinge_connected_to(oh, l)
            };
            if !attached {
                remove_first(&mut self.limbs[o].ConnectedLimbs, l);
                // `Source == this` compares a CirculationBehaviour with a LimbBehaviour: never equal.
            }
            i += 1;
        }
        if let Some(connected) = connected {
            remove_first(&mut self.limbs[connected].ConnectedLimbs, l);
        }
        if let Some(src) = self.circulations[l].Source {
            remove_first(&mut self.limbs[l].ConnectedLimbs, src);
            remove_first(&mut self.limbs[src].ConnectedLimbs, l);
        }
    }
}

impl Game {
    /// AddComponent<SpringJoint2D> and the setters GoreStringBehaviour.CreateJoint runs, ending with
    /// connectedBody = other. Each ReCreate (anchor, connectedAnchor, connectedBody) destroys the b2 joint and
    /// builds it again; the ones before connectedBody hang from the scene's fixtureless ground body, so all they
    /// leave behind is limb `l` woken by b2World::DestroyJoint. The last one is SpringJoint2D::Create with
    /// autoconfiguration already complete: anchors scaled by each body's lossy scale, the clamped distance,
    /// frequency and damping ratio, breakForce infinite (so out of the break-limit list).
    pub(crate) fn create_spring_joint(&mut self, l: usize, other: usize, anchor: [f32; 2], connected_anchor: [f32; 2], distance: f32, frequency: f32, damping_ratio: f32) {
        let (pa, pb) = (self.limbs[l].PhysicalBehaviour, self.limbs[other].PhysicalBehaviour);
        let (sa, sb) = (self.lossy_scale(pa), self.lossy_scale(pb));
        let (a, b) = (self.physicals[pa].rigidbody.body, self.physicals[pb].rigidbody.body);
        // SpringJoint2D::SetDistance / SetFrequency / SetDampingRatio.
        let clamp = |v: f32, lo: f32, hi: f32| if lo <= v && v <= hi { v } else if lo <= v { hi } else { lo };
        let def = crate::b2::distance_joint::DistanceDef {
            local_anchor_a: crate::b2::math::Vec2::new(sa[0] * anchor[0], sa[1] * anchor[1]),
            local_anchor_b: crate::b2::math::Vec2::new(sb[0] * connected_anchor[0], sb[1] * connected_anchor[1]),
            length: clamp(distance, f32::from_bits(0x3ba3_d70a), 1_000_000.0),
            frequency_hz: clamp(frequency, 0.0, 1_000_000.0),
            damping_ratio: clamp(damping_ratio, 0.0, 1.0),
        };
        self.world.wake_body(a);
        let id = self.world.create_joint(def, a, b, false);
        self.world.wake_joined(a);
        self.world.wake_joined(b);
        self.limbs[l].gore.joint = Some(id);
    }

    /// Joint2D::ReCreate (180c05570) of limb `l`'s gore-string SpringJoint2D: Cleanup, then SpringJoint2D::Create
    /// (180bfd060), whose auto-configuration ran at the first Create, so the stored anchors, distance, frequency and
    /// damping ratio come back (the anchors scaled by the bodies' current lossy scale).
    pub(super) fn gore_spring_recreate(&mut self, l: usize) {
        if let Some(id) = self.limbs[l].gore.joint.take() {
            self.world.destroy_joint(id);
        }
        let g = &self.limbs[l].gore;
        let other = g.Other.expect("a gore string joint has its other end");
        let anchor = [(g.OriginLine[0] + g.OriginLine[2]) * 0.5, (g.OriginLine[1] + g.OriginLine[3]) * 0.5];
        let connected_anchor = [(g.OtherLine[0] + g.OtherLine[2]) * 0.5, (g.OtherLine[1] + g.OtherLine[3]) * 0.5];
        let (freq, damping) = (g.JointFreq, g.JointDampening);
        self.create_spring_joint(l, other, anchor, connected_anchor, 0.05, freq, damping);
    }

    /// Object.Destroy(Joint): DestroyObjectDelayed queues it on the DelayedCallManager at
    /// min(fixedTimeAsDouble, the frame's time); a second Destroy finds nothing left to do.
    fn destroy_spring_joint(&mut self, l: usize) {
        let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
        self.limbs[l].gore.destroy_at.get_or_insert(at);
        // DestroyObjectFromScripting (1808cd940) disables the Behaviour at once.
        if let Some(id) = self.limbs[l].gore.joint.take() {
            self.world.destroy_joint(id);
        }
    }

    /// SpringJoint2D destruction (Joint2D::Cleanup): the b2 joint and its break-limit entry go; `Joint` reads null.
    pub fn remove_spring_joint(&mut self, l: usize) {
        if let Some(id) = self.limbs[l].gore.joint.take() {
            self.world.destroy_joint(id);
        }
        self.joint_break_limits.retain(|&k| k != super::BreakJoint::Spring(l));
        self.limbs[l].goreStringJoint = false;
        self.limbs[l].gore.destroy_at = None;
    }

    /// Joint.breakForce = v on limb `l`'s SpringJoint2D (Joint2D::SetBreakForce).
    fn set_spring_break_force(&mut self, l: usize, v: f32) {
        let v = if 0.0 <= v && v <= f32::INFINITY { v } else if 0.0 <= v { f32::INFINITY } else { 0.0 };
        self.limbs[l].gore.breakForce = v;
        self.add_break_limits_update(super::BreakJoint::Spring(l), v, f32::INFINITY);
    }

    /// GoreStringBehaviour.CreateJoint
    pub fn gore_string_create_joint(&mut self, l: usize) {
        if self.limbs[l].goreStringJoint {
            // Debug.LogError("Tried to create gore string joint again ...")
            return;
        }
        let Some(other) = self.limbs[l].gore.Other else {
            panic!("{}", crate::il!(str "GoreStringBehaviour::CreateJoint" 1));
        };
        self.limbs[l].gore.HadJointWhenSerialised = true;
        let g = &self.limbs[l].gore;
        // LineSegment.GetMidpoint: (A + B) * 0.5f
        let anchor = [(g.OriginLine[0] + g.OriginLine[2]) * 0.5, (g.OriginLine[1] + g.OriginLine[3]) * 0.5];
        let connected_anchor = [(g.OtherLine[0] + g.OtherLine[2]) * 0.5, (g.OtherLine[1] + g.OtherLine[3]) * 0.5];
        let (freq, damping, invincibility) = (g.JointFreq, g.JointDampening, g.InvincibilityTime);
        self.create_spring_joint(l, other, anchor, connected_anchor, 0.05, freq, damping);
        self.limbs[l].goreStringJoint = true;
        self.limbs[l].gore.breakForce = f32::INFINITY;
        // StartCoroutine(DelayCoroutine(InvincibilityTime, SetBreakForce)): WaitForSeconds.
        self.limbs[l].gore.break_at = Some(self.time.timeAsDouble + invincibility as f64);
        let num = self.random.range_int(5, 14);
        let mut data = Vec::with_capacity(num as usize);
        data.push(TissueString::new(0.5, 0.5, 2.0, f32::INFINITY, 1.0));
        for _ in 1..num {
            // Constructor arguments evaluate in order: pos1, pos2, thickness, strength, thicknessRatio.
            let pos1 = self.random.value();
            let pos2 = self.random.value();
            let thickness = self.random.range(0.5, 2.0);
            let strength = self.random.range(0.1, 1.0);
            let ratio = self.random.range(0.5, 2.0);
            data.push(TissueString::new(pos1, pos2, thickness, strength, ratio));
        }
        // The Lines renderer setup is presentation.
        self.limbs[l].gore.lineData = Some(data);
    }

    /// LimbBehaviour.PhysicalBehaviour_OnDisintegration
    pub fn limb_on_disintegration(&mut self, l: usize) {
        if self.limb_is_considered_alive(l) && !self.limb_is_paralysed(l) {
            self.add_pain(self.limbs[l].Person, 25.0);
        }
        let position = self.physicals[self.limbs[l].PhysicalBehaviour].transform.position;
        self.skin_add_damage_point(l, super::misc::damage_type::DISMEMBERMENT, [position[0], position[1]], 25.0);
        self.circulation_disintegrate(l);
        for i in 0..self.limbs[l].ConnectedLimbs.len() {
            let other = self.limbs[l].ConnectedLimbs[i];
            if self.node_is_connected_to(l, other) && self.limbs[other].HasJoint {
                let h = self.limbs[other].Joint.unwrap();
                if self.hinge_connected_to(h, l) {
                    self.set_hinge_break_force(h, 0.0);
                }
            }
        }
        if self.limbs[l].goreString {
            self.gore_string_destroy_joint(l);
        }
        self.node_disconnect_from_everything(l);
        if self.limbs[l].HasJoint {
            let h = self.limbs[l].Joint.unwrap();
            self.set_hinge_break_force(h, 0.0);
            self.set_hinge_break_torque(h, 0.0);
        }
    }

    /// GoreStringBehaviour.DestroyJoint
    pub fn gore_string_destroy_joint(&mut self, l: usize) {
        self.limbs[l].gore.HadJointWhenSerialised = false;
        self.limbs[l].gore.lineData = None;
        if self.limbs[l].goreStringJoint {
            self.destroy_spring_joint(l);
        }
    }

    /// GoreStringBehaviour.TryPlaySnapSound: AudioSource.PlayOneShot(TissueSnapClips.PickRandom(), 0.5f) when the
    /// AudioSource is set, else phys.PlayClipOnce(the same, 0.5f) (phys: the limb's PhysicalBehaviour).
    pub(crate) fn gore_string_snap_sound(&mut self, l: usize) {
        let (n, id) = (self.limbs[l].gore.TissueSnapClips, self.limbs[l].gore.id);
        if n == 0 {
            return;
        }
        let clip = self.pick_clip(id, "TissueSnapClips", n);
        if self.limbs[l].gore.AudioSource {
            if let Some(s) = self.audio.source_field(id, "AudioSource").filter(|s| self.audio.sources.contains_key(s)) {
                self.audio.play_one_shot(s, clip.as_ref(), 0.5);
            }
        } else {
            self.play_clip_once(self.limbs[l].PhysicalBehaviour, clip, 0.5);
        }
    }

    /// GoreStringBehaviour.OnJointBreak2D for the string's own SpringJoint2D breaking.
    pub fn gore_string_on_joint_break(&mut self, l: usize) {
        self.gore_string_snap_sound(l);
        self.gore_string_destroy_joint(l);
    }

    /// GoreStringBehaviour.Update (the colour smoothing and the line drawing are presentation).
    pub fn gore_string_update(&mut self, l: usize) {
        if !self.limbs[l].goreString || !self.limbs[l].goreStringEnabled || self.limbs[l].gore.lineData.is_none() {
            return;
        }
        let Some(other) = self.limbs[l].gore.Other else { return };
        if !self.limb_active(other) {
            self.gore_string_destroy_joint(l);
            return;
        }
        let tissue = self.limbs[l].gore.TissueColour;
        let target = match self.limbs[l].gore.ColourSource {
            Some(c) => self.get_computed_color(super::container::Owner::Circulation(c), tissue),
            None => tissue,
        };
        let t = 1.0 - mathf::pow(0.5, self.time.deltaTime);
        let smoothed = &mut self.limbs[l].gore.smoothedColour;
        for i in 0..3 {
            smoothed[i] = mathf::clamp01(mathf::lerp(smoothed[i], target[i], t));
        }
        smoothed[3] = 1.0;
        let g = &self.limbs[l].gore;
        let break_force = g.breakForce;
        let flag = !break_force.is_infinite();
        let anchor = [(g.OriginLine[0] + g.OriginLine[2]) * 0.5, (g.OriginLine[1] + g.OriginLine[3]) * 0.5];
        let connected_anchor = [(g.OtherLine[0] + g.OtherLine[2]) * 0.5, (g.OtherLine[1] + g.OtherLine[3]) * 0.5];
        let a = self.transform_point(self.limbs[l].PhysicalBehaviour, anchor);
        let b = self.transform_point(self.limbs[other].PhysicalBehaviour, connected_anchor);
        let num = mathf::magnitude2([a[0] - b[0], a[1] - b[1]]);
        for i in 0..self.limbs[l].gore.lineData.as_ref().unwrap().len() {
            let t = self.limbs[l].gore.lineData.as_ref().unwrap()[i];
            if flag && t.Thickness > f32::from_bits(1) && ((num / (t.Strength * t.Strength)) as f64) > (break_force as f64) * 0.05 {
                self.gore_string_snap_sound(l);
                self.limbs[l].gore.lineData.as_mut().unwrap()[i].Thickness = 0.0;
            }
        }
    }

    /// GoreStringBehaviour.Update's Line values, with their RenderOrder: position A, position B, width A, width B.
    pub fn gore_lines(&self) -> Vec<(i32, [f32; 2], [f32; 2], f32, f32, [f32; 4])> {
        let mut out = Vec::new();
        let along = |line: [f32; 4], t: f32| [mathf::lerp(line[0], line[2], t), mathf::lerp(line[1], line[3], t)];
        for l in 0..self.limbs.len() {
            let g = &self.limbs[l].gore;
            let (true, true, true, Some(data), Some(other)) = (self.limbs[l].goreString, self.limbs[l].goreStringEnabled, self.limb_active(l), &g.lineData, g.Other) else { continue };
            if !self.limb_active(other) {
                continue;
            }
            let (p, q) = (self.limbs[l].PhysicalBehaviour, self.limbs[other].PhysicalBehaviour);
            let a = self.transform_point(p, along(g.OriginLine, 0.5));
            let b = self.transform_point(q, along(g.OtherLine, 0.5));
            let width = g.LineWidthMultiplier / mathf::magnitude2([a[0] - b[0], a[1] - b[1]]).max(0.25);
            for t in data.iter().filter(|t| t.Thickness > 0.01) {
                let w = (width * t.Thickness).abs();
                let (a, b) = (self.transform_point(p, along(g.OriginLine, t.Pos1)), self.transform_point(q, along(g.OtherLine, t.Pos2)));
                out.push((g.RenderOrder, a, b, w / t.ThicknessRatio, w * t.ThicknessRatio, g.smoothedColour));
            }
        }
        out
    }

    /// DelayedCallManager::Update (180634dd0): the callbacks due when the pass starts, in time order. The fixed-rate
    /// pass (after the physics step) runs only the delayed Destroys; the dynamic one (after Update) also the
    /// SetBreakForce coroutines. A callback queued during the pass is not in this snapshot, so it waits for the next
    /// pass ("not added during this very pass"). A callback that another one cancelled or re-armed earlier in the pass
    /// is no longer pending, so it is skipped (the field-backed kinds below). Not rechecked: the tool drags, the object
    /// and component destroys, the attachment and joint destroys and the coroutine resumes (their owners' cancellation
    /// within one pass is not modelled here).
    pub fn gore_string_delayed_calls(&mut self, dynamic: bool) {
        self.delayed_calls_before(dynamic, f64::INFINITY);
    }

    /// The due calls queued for before `before`.
    pub fn delayed_calls_before(&mut self, dynamic: bool, before: f64) {
        let now = self.time.timeAsDouble;
        // (time, kind: 0 break force, 1 gore string destroy, 2 drag joint destroy, 3 person destroy, limb/person)
        let mut due: Vec<(f64, u8, usize)> = Vec::new();
        for l in 0..self.limbs.len() {
            let g = &self.limbs[l].gore;
            // A deactivated limb's coroutines stopped with it.
            if let Some(t) = g.break_at.filter(|&t| dynamic && t <= now && self.limb_active(l)) {
                due.push((t, 0, l));
            }
            if let Some(t) = g.destroy_at.filter(|&t| t <= now) {
                due.push((t, 1, l));
            }
        }
        due.extend(self.tool_due_destroys(now).into_iter().map(|t| (t, 2, 0)));
        due.extend(self.person_due_destroys(now).into_iter().map(|(t, p)| (t, 3, p)));
        due.extend(self.frozen_in_ice_due_destroys(now).into_iter().map(|(t, i)| (t, 4, i)));
        due.extend(self.object_due_destroys(now).into_iter().map(|(t, id)| (t, 5, id as usize)));
        due.extend(self.joint_due_destroys(now).into_iter().map(|(t, f)| (t, 7, f)));
        for h in 0..self.hinges.len() {
            if let Some(t) = self.hinges[h].destroy_at.filter(|&t| t <= now) {
                due.push((t, 10, h));
            }
        }
        due.extend(self.attachment_joint_destroys.iter().filter(|&&(at, _)| at <= now).map(|&(t, j)| (t, 9, j)));
        due.extend(self.component_destroys.iter().filter(|&&(at, _)| at <= now).map(|&(t, id)| (t, 8, id as usize)));
        if dynamic {
            let frame = self.time.frameCount as i64;
            due.extend(self.coroutines.iter().filter(|c| c.at <= now && super::coroutine::frame_reached(c, frame)).map(|c| (c.at, 6, c.seq as usize)));
        }
        due.retain(|d| d.0 < before);
        due.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for (t, kind, l) in due {
            // Still pending: the field that armed it still holds this due time.
            let pending = match kind {
                0 => self.limbs[l].gore.break_at == Some(t),
                1 => self.limbs[l].gore.destroy_at == Some(t),
                3 => self.people[l].destroy_at == Some(t),
                4 => self.frozen_in_ice[l].destroy_at == Some(t),
                10 => self.hinges[l].destroy_at == Some(t),
                _ => true,
            };
            if !pending {
                continue;
            }
            if kind == 6 {
                // Gone if its GameObject was destroyed or deactivated earlier in this pass.
                if let Some(i) = self.coroutines.iter().position(|c| c.seq == l as u64) {
                    let c = self.coroutines.remove(i);
                    self.resume_coroutine(c);
                }
            } else if kind == 10 {
                self.run_hinge_destroy(l);
            } else if kind == 9 {
                self.attachment_run_joint_destroy(l);
            } else if kind == 8 {
                // gapC r23: a FuseBox's audioProvider (an AudioClip, no script): gone, nothing runs.
                if self.fuse_box_clips.values().any(|&c| c == l as i32) {
                    self.component_destroys.retain(|&(_, i)| i != l as i32);
                    self.destroyed_components.insert(l as i32);
                } else if !self.line_run_destroy(l as i32) && !self.blood_wire_run_destroy(l as i32) && !self.root_wire_run_destroy(l as i32) && !self.phase_link_run_destroy(l as i32) && !self.belt_run_destroy(l as i32) && !self.pin_run_destroy(l as i32) && !self.freeze_run_destroy(l as i32) && !self.poison_run_destroy(l as i32) && !self.glow_run_destroy(l as i32) {
                    self.tr_run_component_destroy(l as i32);
                }
            } else if kind == 7 {
                self.run_joint_destroy(l);
            } else if kind == 5 {
                self.run_object_destroy(l as i32);
            } else if kind == 4 {
                self.frozen_in_ice_run_destroy(l);
            } else if kind == 3 {
                self.person_run_destroy(l);
            } else if kind == 2 {
                self.tool_run_destroy(t);
            } else if kind == 1 {
                if self.limbs[l].gore.destroy_at.is_some() {
                    self.remove_spring_joint(l);
                }
            } else {
                self.limbs[l].gore.break_at = None;
                self.gore_string_set_break_force(l);
            }
        }
    }

    /// GoreStringBehaviour.SetBreakForce: if Joint, Joint.breakForce = JointBreakThreshold * 1.5.
    pub(crate) fn gore_string_set_break_force(&mut self, l: usize) {
        if self.limbs[l].goreStringJoint {
            let v = self.limbs[l].gore.JointBreakThreshold * 1.5;
            self.set_spring_break_force(l, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tool::Control;
    use crate::play::{new_game, step};

    fn frames(g: &mut Game, n: usize) {
        for _ in 0..n {
            step(g, 1.0 / 60.0).unwrap();
            for c in g.input.controls.values_mut() {
                c.down = false;
                c.up = false;
            }
        }
    }

    /// ShatterProcedurally on a Human's limbs with the chance forced to pass: each Generate builds PartCount shards
    /// (body, collider, PhysicalBehaviour) whose ids follow the AddComponent order, registers them as the limb's
    /// DestroyWith and the person's pseudo children, and the run goes on. Skipped without the installed game.
    #[test]
    fn shatter_procedurally_spawns_shards() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Default", Some("Human"), None, 0).unwrap();
        g.prefs.ProceduralFragments = true;
        frames(&mut g, 30);
        let c = g.BoundingBox.center;
        let ground = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4).unwrap().point.y;
        g.input.mouse = [c[0], ground + 0.6, 0.0];
        g.input.controls.insert("spawnRight".into(), Control { down: false, held: false, up: true });
        frames(&mut g, 1);
        g.input.controls.insert("spawnRight".into(), Control::default());
        frames(&mut g, 60);
        let l = (0..g.limbs.len()).find(|&l| g.limbs[l].ShatteredObjectGenerators.is_some()).expect("a limb with generators");
        g.limbs[l].ShatteredObjectChance = 2.0;
        let (first_physical, first_id) = (g.physicals.len(), g.next_instance_id);
        g.limb_shatter_procedurally(l, 255);
        let shards = first_physical..g.physicals.len();
        // The collider's shapes sit on the ground body until the next SyncTransforms moves them onto the shard's body.
        g.sync_pending();
        assert!(!shards.is_empty(), "no shard spawned");
        assert_eq!(g.limbs[l].destroy_with.len(), shards.len());
        assert_eq!(g.people[g.limbs[l].Person].pseudo_children, shards.clone().collect::<Vec<_>>());
        for s in shards {
            let ph = &g.physicals[s];
            // A shard whose mask polygon is empty keeps a collider without shapes (SetPath skips an empty path).
            let has_path = matches!(&ph.collider_defs[0].shape, crate::assets::components::ColliderShape::Polygon { paths, .. } if !paths.is_empty());
            assert_eq!(!g.world.body(ph.rigidbody.body).fixtures.is_empty(), has_path, "shard {s}: collider shapes");
            assert!(ph.ids.ActivationNode < ph.id && ph.id <= first_id);
            // Instance ids count down: the components' ids follow the order the C# adds them in.
            let id = ph.id;
            let components = g.with_component_instance(id, |_, inst, _| inst.objects[0].components.clone());
            assert_eq!(components.len(), 11, "the ten of Generate, then the PseudoBloodImpactHelper");
            assert!(components.windows(2).all(|w| w[1] < w[0]), "{components:?}");
        }
        // Generate's SpringJoint2Ds between the shards (GenerateSoftJointsChance > 0), live on the shards' bodies.
        assert!(g.fixed_joints.iter().any(|f| f.spring.is_some() && f.joint.is_some()), "no spring joint between shards");
        frames(&mut g, 120);
        // A shard's disintegration counts toward the person's DisintegrationCounterBehaviour like a limb's.
        let person = g.limbs[l].Person;
        let before = g.people[person].DisintegrationCount;
        g.physical_disintegrate(first_physical);
        assert_eq!(g.people[person].DisintegrationCount, before + 1);
        // Destroying the person's root takes the shards with it (LimbBehaviour.OnDestroy: DestroyWith).
        let root = g.people[person].id;
        g.destroy_object(root);
        frames(&mut g, 5);
        assert!(g.physicals[first_physical + 1..].iter().all(|q| q.destroyed), "a shard survived its limb");
    }
}
