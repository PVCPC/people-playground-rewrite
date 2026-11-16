//! The Reactor A5's scripts: ReactorCoreBehaviour (temperature, power output, the alarm groups and the meltdown),
//! ReactorConduitBehaviour (a BloodContainer tube with a FixedJoint2D to its neighbour and a wall HingeJoint2D),
//! ReactorConduitSetBehaviour (the status of a set of conduits and the pairs that never collide), ReactorMeltdownBehaviour
//! (Explode) and DoAtTime (the meltdown sequence). ReactorA5DisplayBehaviour writes the diagnostics text at 10 Hz, its shader
//! colours and the alert beep (PlayAlertNoise, `reactor_a5_display_update`).
#![allow(non_snake_case)]

// AudioSourceToggleBehaviour.Toggle: presentation (AudioSource.Play or Stop on the reactor prefab's UnityEvent; audio only)
// ReactorA5DisplayBehaviour.PlayAlertNoise: `reactor_a5_display_update`

use super::container::Owner;
use super::explosion::{EffectSize, ExplosionParameters};
use super::link::EventCall;
use super::{d, mathf, script, Behaviour, BreakJoint, Game};

/// ReactorCoreBehaviour.ReactorState
const LOW_TEMP: i32 = 1;
const HIGH_TEMP: i32 = 2;
const MELTDOWN: i32 = 3;
/// ReactorCoreBehaviour.ReactorCriticality
const SUBCRITICAL: i32 = 0;
const CRITICAL: i32 = 1;
const SUPERCRITICAL: i32 = 2;

/// ReactorA5DisplayBehaviour.ReactorPartStatus
const NORMAL: i32 = 0;
const QUESTIONABLE: i32 = 1;
const DAMAGED: i32 = 2;
const CATASTROPHIC: i32 = 3;

/// The public methods of ReactorCoreBehaviour a UnityEvent calls.
#[derive(Clone, Copy, Debug)]
pub enum ReactorCall {
    IncreaseCooling,
    DecreaseCooling,
    IncreasePower,
    DecreasePower,
    DecrementCoolingPower(f32),
    ForceMeltdown,
}

impl ReactorCall {
    /// The call named `method` (its float argument for DecrementCoolingPower).
    pub fn parse(method: &str, number: Option<f32>) -> Option<Self> {
        Some(match method {
            "IncreaseCooling" => Self::IncreaseCooling,
            "DecreaseCooling" => Self::DecreaseCooling,
            "IncreasePower" => Self::IncreasePower,
            "DecreasePower" => Self::DecreasePower,
            "DecrementCoolingPower" => Self::DecrementCoolingPower(number?),
            "ForceMeltdown" => Self::ForceMeltdown,
            _ => return None,
        })
    }
}

script! {
    ReactorA5Display {
    }
    links {
        id: i32 = 0,
        enabled: bool = true,
        /// Core: `Game::reactor_cores` (None: null).
        Core: Option<usize> = None,
        /// TextMesh (world_text.rs).
        TextMesh: Option<usize> = None,
        /// Wire1 to Wire4: `Game::reactor_conduit_sets` (None: null).
        Wires: [Option<usize>; 4] = [None; 4],
        /// updateDisplayClock: RateHz = 10 (Awake).
        updateDisplayClock: super::misc::FixedIntervalDistributor = super::misc::FixedIntervalDistributor::with_rate(10.0),
        /// phys: the PhysicalBehaviour on its object.
        physical: Option<usize> = None,
        /// alertBeepClock (RateHz 2 from Awake).
        alertBeepClock: super::misc::FixedIntervalDistributor = super::misc::FixedIntervalDistributor::with_rate(2.0),
        /// DiagnosticsMaterial (None: null).
        DiagnosticsMaterial: Option<crate::assets::db::ObjRef> = None,
        /// wire1Col to wire4Col, vacuumChamberCol, connectorCol and baseCol (Color.white at first).
        cols: [[f32; 4]; 7] = [[1.0; 4]; 7],
    }
}

impl ReactorA5Display {
    /// ReactorA5DisplayBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>(), alertBeepClock.RateHz = 2 and updateDisplayClock.RateHz = 10.
    pub fn awake(&mut self, physical: Option<usize>) {
        self.physical = physical;
        self.alertBeepClock.set_RateHz(2.0);
        self.updateDisplayClock.set_RateHz(10.0);
    }
}

#[cfg(test)]
mod reactor_a5_display_tests {
    #[test]
    fn awake_sets_the_clock_rates_and_physical() {
        let mut d = super::ReactorA5Display::default();
        d.awake(Some(3));
        assert_eq!(d.physical, Some(3));
        assert_eq!(d.alertBeepClock, super::super::misc::FixedIntervalDistributor::with_rate(2.0));
        assert_eq!(d.updateDisplayClock, super::super::misc::FixedIntervalDistributor::with_rate(10.0));
    }
}

script! {
    ReactorCore {
        MeltdownTemperatureThreshold: f32 = 10000.0,
        HighTemperatureThreshold: f32 = 5000.0,
        LowTemperatureWarningThreshold: f32 = 1000.0,
        MeltdownRequiredTime: f32 = 15.0,
        CoolingPower: f32 = 0.5,
        InternalTemperature: f32 = 2500.0,
        InsulationPower: f32 = 25.0,
        ReactorPowerTargetRatio: f32 = 1.0,
        HeatUpIntensity: f32 = 100.0,
        CoolIntensity: f32 = 100.0,
        TimeVariationSpeed: f32 = 1.0,
        TimeVariationIntensity: f32 = 1.0,
        CoolingPowerRatio: f32 = 1.0,
        ReferenceOutputPower: f32 = 1000000.0,
        OutputPowerSmoothing: f32 = 50.0,
        State: i32 = 0,
        Criticality: i32 = 1,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// phys = GetComponent<PhysicalBehaviour>()
        physical: usize = 0,
        /// LowTempAlarmGameObjects, HighTempAlarmGameObjects and MeltdownAlarmGameObjects: object indices of the clone.
        low_alarm: Vec<usize> = Vec::new(),
        high_alarm: Vec<usize> = Vec::new(),
        meltdown_alarm: Vec<usize> = Vec::new(),
        /// EnergyConduits: `Game::reactor_conduit_sets`.
        conduits: Vec<usize> = Vec::new(),
        /// MeltdownSequence: `Game::do_at_times` (None: a null field).
        sequence: Option<usize> = None,
        /// DisableOnMeltdown: object indices (None: a null array).
        disable_on_meltdown: Option<Vec<usize>> = None,
        /// OnMeltdownStart's persistent calls that are not presentation.
        on_meltdown_start: Vec<EventCall> = Vec::new(),
        /// OutputNodes: `Game::physicals` (None: a null array).
        output_nodes: Option<Vec<usize>> = None,
        lastState: i32 = 0,
        powerOutput: f32 = 0.0,
        lastPoweroutput: f32 = 0.0,
        meltdownTime: f32 = 0.0,
        /// criticalityClock: RateHz = 8 (Awake).
        criticalityClock: super::misc::FixedIntervalDistributor = super::misc::FixedIntervalDistributor::with_rate(8.0),
        meltdownStarted: bool = false,
        hasMeltedDown: bool = false,
    }
}

script! {
    ReactorConduitSet {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// NoCollide: the object index of each Rigidbody2D's GameObject.
        no_collide: Vec<usize> = Vec::new(),
        /// Conduits: `Game::reactor_conduits`, then the objects they sit on (GetComponentsInChildren, Awake).
        conduits: Vec<usize> = Vec::new(),
        conduit_objects: Vec<usize> = Vec::new(),
    }
}

script! {
    ReactorConduit {
        HealthyWallJoint: bool = true,
        Broken: bool = false,
    }
    base { base: super::container::BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// phys = GetComponent<PhysicalBehaviour>()
        physical: usize = 0,
        /// OptionalInflow: the liquid (`LIQUIDS` index; None for an empty or unknown id) and the Amount.
        inflow_liquid: Option<usize> = None,
        inflow_id: String = String::new(),
        inflow_amount: f32 = 0.0,
        /// LiquidPushTarget and source: `Game::reactor_conduits`.
        LiquidPushTarget: Option<usize> = None,
        source: Option<usize> = None,
        /// core = Object.FindObjectOfType<ReactorCoreBehaviour>()
        core: Option<usize> = None,
        /// tubeJoint: the FixedJoint2D's component id, its key and its serialised breakForce (initialBreakForce).
        tube: Option<(i32, BreakJoint)> = None,
        initialBreakForce: f32 = 0.0,
        randomResilience: f32 = 0.0,
        /// reactorJoint: the HingeJoint2D's key.
        reactor_joint: Option<BreakJoint> = None,
        /// connectedConduit: the PhysicalBehaviour of tubeJoint.connectedBody.
        connected: Option<usize> = None,
        /// BreakClips.Length
        clips: usize = 0,
        /// The tube's connectedBody object, to compare with LiquidPushTarget's.
        connected_object: Option<usize> = None,
        /// The object this conduit sits on.
        object: usize = 0,
    }
}

script! {
    ReactorMeltdown {
        SetTemperature: f32 = 5000.0,
        SetCharge: f32 = 12.0,
        MeltdownActive: bool = false,
        AmbientHeatupMax: f32 = 400.0,
        AmbientHeatupSpeed: f32 = 0.5,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// SparkHitLayers (LayerMask bits)
        SparkHitLayers: u32 = 0,
    }
}

/// DoAtTime.Event
#[derive(Clone, Debug)]
pub struct DoAtEvent {
    pub Time: f32,
    pub Passed: bool,
    pub calls: Vec<EventCall>,
}

script! {
    DoAtTime {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        time: f32 = 0.0,
        Events: Vec<DoAtEvent> = Vec::new(),
    }
}

script! {
    IndustrialLift {
        Speed: i32 = 0,
        SpeedMultiplier: f32 = 50.0,
        AccelerationSpeed: f32 = 0.1,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// A and B: the SliderJoint2D's object and component index in the prefab (None: null).
        a: Option<(usize, usize)> = None,
        b: Option<(usize, usize)> = None,
        /// The `Game::fixed_joints` entries the sliders became at Awake.
        a_joint: Option<usize> = None,
        b_joint: Option<usize> = None,
        /// MinMaxDistance
        min: f32 = 0.0,
        max: f32 = 100.0,
        dist: f32 = 0.0,
    }
}

script! {
    AcidPool {
        TemperatureIncreaseCelsius: f32 = 15.0,
        AcidProgress: f32 = 0.01,
        MaxTemperatureCelsius: f32 = 300.0,
        OnlyOrganic: bool = true,
        WinceIntensity: f32 = 300.0,
        PainIntensity: f32 = 0.1,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Pool: the `Game::waters` index.
        Pool: usize = 0,
    }
}

script! {
    Vacuum {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Collider's fixtures (the query shape of OverlapCollider).
        fixtures: Vec<crate::b2::world::FixtureId> = Vec::new(),
        /// Filter.layerMask (all layers without useLayerMask).
        mask: u32 = !0,
    }
}

script! {
    PointAt {
        Force: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Rigidbody: the Rigidbody2D's component id.
        rigidbody: i32 = 0,
        /// Target: the PhysicalBehaviour whose Transform it is (`Game::physicals`).
        target: usize = 0,
    }
}

script! {
    RandomSound {
        PlayOnStart: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Clips.Length (0: null or empty)
        clips: usize = 0,
        /// (bool)AudioSource
        has_source: bool = false,
        /// (bool)phys: GetComponent<PhysicalBehaviour>()
        has_physical: bool = false,
        /// phys (`Game::physicals`)
        physical: Option<usize> = None,
    }
}

script! {
    PlayRandomClip {
        PlayOnStart: bool = false,
        UsePhysicalBehaviourAudioSource: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Clips.Length (0: null or empty)
        clips: usize = 0,
        /// (bool)Source and (bool)PhysicalBehaviour
        has_source: bool = false,
        has_physical: bool = false,
        /// PhysicalBehaviour (`Game::physicals`)
        physical: Option<usize> = None,
    }
}

impl Game {
    // ---- PointAtBehaviour -----------------------------------------------------------------------------------------

    /// PointAtBehaviour.FixedUpdate
    pub fn point_at_fixed_update(&mut self, i: usize) {
        let (id, force, target) = (self.point_ats[i].rigidbody, self.point_ats[i].Force, self.point_ats[i].target);
        let owner = self.rigidbody_owner(id).expect("PointAtBehaviour.Rigidbody: a Rigidbody2D outside the arenas");
        let rb = self.owner_rigidbody(owner).clone();
        if rb.bodyType != 0 {
            return;
        }
        // Rigidbody2D.position and .rotation: AutoSyncTransforms, then the b2 body.
        self.sync_pending();
        let at = self.world.body(rb.body).xf.p;
        let t = self.physicals[target].transform.position;
        let normalized = mathf::normalized2([t[0] - at.x, t[1] - at.y]);
        let rotation = rb.rotation(&self.world);
        let angle = (d(crate::game::mathf::atan2(normalized[1], normalized[0])) * 57.29578f32).f();
        let num = mathf::delta_angle(rotation, angle);
        if num.abs() < 25.0 {
            let w = rb.angular_velocity(&self.world) * (num / 25.0).clamp(0.95, 1.0);
            rb.set_angular_velocity(&mut self.world, w);
        }
        let torque = (d(num) * force * mathf::magnitude2(normalized)).f();
        self.rigidbody_add_torque(id, torque);
    }

    // ---- VacuumComponent ------------------------------------------------------------------------------------------

    /// VacuumComponent.FixedUpdate
    pub fn vacuum_fixed_update(&mut self, i: usize) {
        let (fixtures, mask) = (self.vacuums[i].fixtures.clone(), self.vacuums[i].mask);
        // Filter.useTriggers is on.
        let mut hits = self.overlap_collider_triggers(&fixtures, mask, true);
        // NonAlloc copies the first 32 of the sorted results.
        hits.truncate(32);
        for (physical, _) in hits {
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform)
            let Some(p) = physical else { continue };
            self.extinguish(p);
            // BroadcastMessage("OnInsideVacuum"): the object and its active descendants.
            let receivers: Vec<(String, i32)> = self.with_component_instance(self.physicals[p].id, |_, inst, o| {
                let mut out = Vec::new();
                for x in std::iter::once(o).chain(inst.descendants(o)).filter(|&x| inst.objects[x].active) {
                    for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                        if let crate::assets::db::Component::Script { class, .. } = c {
                            if class == "BalloonBehaviour" || class == "TorchBehaviour" {
                                out.push((class.clone(), inst.objects[x].components[k]));
                            }
                        }
                    }
                }
                out
            });
            for (class, id) in receivers {
                // TorchBehaviour.OnInsideVacuum only lowers torchIntensity (the flame rate and sound).
                if class == "BalloonBehaviour" {
                    let b = self.balloons.iter().position(|x| x.id == id).expect("the object's BalloonBehaviour");
                    self.balloon_on_inside_vacuum(b);
                }
            }
            if let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == p).filter(|&l| !self.limbs[l].IsAndroid) {
                self.limb_wince(l, 25.0);
                if self.skins[l].acidProgress < 0.4 {
                    let v = self.skins[l].acidProgress + self.time.deltaTime * 0.2f32;
                    self.skins[l].set_acid(v);
                }
                if self.limbs[l].RoughClassification == super::limb::HEAD {
                    let person = self.limbs[l].Person;
                    self.add_pain(person, 25.0);
                    self.people[person].OxygenLevel -= self.time.deltaTime * 3.0f32;
                }
            }
        }
    }

    // ---- AcidPoolBehaviour ----------------------------------------------------------------------------------------

    /// AcidPoolBehaviour.FixedUpdate
    pub fn acid_pool_fixed_update(&mut self, i: usize) {
        let w = self.acid_pools[i].Pool;
        if self.water_is_frozen(w) || self.water_is_evaporated(w) {
            return;
        }
        let a = &self.acid_pools[i];
        let (num, max_temp, acid, wince, pain) = (a.TemperatureIncreaseCelsius * self.time.fixedDeltaTime, a.MaxTemperatureCelsius, a.AcidProgress, a.WinceIntensity, a.PainIntensity);
        // GetSubmergedObjects: the live (bool)physicalBehaviour check per entry.
        for p in self.waters[w].currentlyInWater.clone() {
            if self.destroyed_components.contains(&self.physicals[p].id) {
                continue;
            }
            if self.random.value() as f64 > 0.99 {
                self.send_break(-1, Some(p), [0.0, 0.0]);
            }
            let props = &self.properties[self.physicals[p].Properties];
            if !(props.Softness > 0.0) || !(props.Flammability > 0.0) {
                continue;
            }
            if self.physicals[p].SimulateTemperature && self.physicals[p].Temperature < max_temp {
                if self.random.value() as f64 > 0.9995 {
                    self.sizzle(p, true);
                }
                if self.random.value() as f64 > 0.955 {
                    self.ignite(p, true);
                }
                self.physicals[p].Temperature += num;
            }
            // TryGetComponent<SkinMaterialHandler>: every limb has one.
            if let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == p) {
                let current = self.skins[l].acidProgress;
                self.skins[l].set_acid(current + acid);
                self.limb_wince(l, wince);
                if self.nodes[l].IsConnectedToRoot {
                    let person = self.limbs[l].Person;
                    self.add_pain(person, pain);
                }
            }
        }
    }

    // ---- IndustrialLiftBehaviour ----------------------------------------------------------------------------------

    /// IndustrialLiftBehaviour.SetSpeed
    pub fn industrial_lift_set_speed(&mut self, id: i32, v: i32) {
        let i = self.industrial_lifts.iter().position(|x| x.id == id).expect("an IndustrialLiftBehaviour in the arena");
        self.industrial_lifts[i].Speed = v;
    }

    /// IndustrialLiftBehaviour.Awake: dist is the first slider's jointTranslation (A, else B).
    pub fn industrial_lift_awake(&mut self, i: usize) {
        let s = &self.industrial_lifts[i];
        if let Some(f) = s.a_joint.or(s.b_joint) {
            self.industrial_lifts[i].dist = self.slider_joint_translation(f);
        }
    }

    /// IndustrialLiftBehaviour.FixedUpdate
    pub fn industrial_lift_fixed_update(&mut self, i: usize) {
        let s = &mut self.industrial_lifts[i];
        s.dist += s.Speed as f32 * 0.04f32;
        s.dist = s.dist.clamp(s.min, s.max);
        let (a, b) = (s.a_joint, s.b_joint);
        for f in [a, b].into_iter().flatten() {
            self.industrial_lift_process_joint(i, f);
        }
    }

    /// IndustrialLiftBehaviour.ProcessJoint: the motor speed eases toward the distance to dist.
    pub(crate) fn industrial_lift_process_joint(&mut self, i: usize, f: usize) {
        let (speed, max_force) = {
            let s = self.fixed_joints[f].slider.as_ref().expect("IndustrialLiftBehaviour: a SliderJoint2D");
            // The get_motor injection (180c47660) reads the stored speed and force whatever useMotor is; set_motor turns it on.
            (s.motor_speed, s.max_motor_torque)
        };
        let s = &self.industrial_lifts[i];
        let target = s.SpeedMultiplier * (s.dist - self.slider_joint_translation(f)).clamp(-1.0, 1.0);
        let new_speed = mathf::lerp(speed, target, s.AccelerationSpeed);
        self.slider_joint_set_motor(f, new_speed, max_force);
    }

    // ---- RandomSoundBehaviour -------------------------------------------------------------------------------------

    /// RandomSoundBehaviour.PlayClip: the clip pick and its PlayOneShot.
    pub fn random_sound_play_clip(&mut self, id: i32) {
        let i = self.random_sounds.iter().position(|x| x.id == id).expect("a RandomSoundBehaviour in the arena");
        let s = &self.random_sounds[i];
        if s.clips > 0 && s.enabled && (s.has_source || s.has_physical) {
            // AudioSource.PlayOneShot(Clips.PickRandom()), else phys.PlayClipOnce(Clips.PickRandom())
            let (has_source, physical, n) = (s.has_source, s.physical, s.clips);
            let clip = self.pick_clip(id, "Clips", n);
            if has_source {
                let source = self.audio.source_field(id, "AudioSource").expect("RandomSoundBehaviour.AudioSource in its clone");
                self.audio.play_one_shot(source, clip.as_ref(), 1.0);
            } else {
                self.play_clip_once(physical.unwrap(), clip, 1.0);
            }
        }
    }

    /// RandomSoundBehaviour.Start
    pub fn random_sound_start(&mut self, i: usize) {
        if self.random_sounds[i].PlayOnStart {
            let id = self.random_sounds[i].id;
            self.random_sound_play_clip(id);
        }
    }

    // ---- PlayRandomClipBehaviour ----------------------------------------------------------------------------------

    /// PlayRandomClipBehaviour.Play: `Clips.PickRandom()` is drawn whatever the target is, then PlayClipOnce or
    /// PlayOneShot (a null target throws).
    pub fn play_random_clip_play(&mut self, id: i32) {
        let i = self.play_random_clips.iter().position(|x| x.id == id).expect("a PlayRandomClipBehaviour in the arena");
        let clip = self.pick_clip(id, "Clips", self.play_random_clips[i].clips);
        let s = &self.play_random_clips[i];
        if !(if s.UsePhysicalBehaviourAudioSource { s.has_physical } else { s.has_source }) {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        if s.UsePhysicalBehaviourAudioSource {
            // PhysicalBehaviour.PlayClipOnce(Clips.PickRandom())
            let p = s.physical.expect("PlayRandomClipBehaviour.PhysicalBehaviour in the arena");
            self.play_clip_once(p, clip, 1.0);
        } else {
            // Source.PlayOneShot(Clips.PickRandom())
            let source = self.audio.source_field(id, "Source").expect("PlayRandomClipBehaviour.Source in its clone");
            self.audio.play_one_shot(source, clip.as_ref(), 1.0);
        }
    }

    /// PlayRandomClipBehaviour.Start
    pub fn play_random_clip_start(&mut self, i: usize) {
        if self.play_random_clips[i].PlayOnStart {
            let id = self.play_random_clips[i].id;
            self.play_random_clip_play(id);
        }
    }

    // ---- ReactorCoreBehaviour -------------------------------------------------------------------------------------

    /// ReactorCoreBehaviour.Start: InternalTemperature is the middle of the warning band, then the alarm groups.
    pub fn reactor_core_start(&mut self, r: usize) {
        let s = &mut self.reactor_cores[r];
        s.InternalTemperature = (d(s.LowTemperatureWarningThreshold) + s.HighTemperatureThreshold).f() / 2.0;
        self.reactor_manage_groups(r, true);
    }

    /// ReactorCoreBehaviour.FixedUpdate
    pub fn reactor_core_fixed_update(&mut self, r: usize) {
        let b = self.reactor_power_output(r);
        let s = &mut self.reactor_cores[r];
        s.powerOutput = mathf::lerp(s.powerOutput, b, 1.0 / mathf::max(s.OutputPowerSmoothing, 1.0));
        let dt = self.time.fixedDeltaTime;
        // The loop condition calls CalculateCycleCount on every test.
        let mut i = 0;
        while i < self.reactor_cores[r].criticalityClock.calculate_cycle_count(dt) {
            self.reactor_calculate_criticality(r);
            i += 1;
        }
        self.reactor_control_temperature(r);
        self.reactor_diffuse(r);
        self.reactor_manage_groups(r, false);
        if let Some(nodes) = self.reactor_cores[r].output_nodes.clone() {
            for n in nodes {
                let power = self.reactor_cores[r].powerOutput;
                self.physicals[n].charge = mathf::max(self.physicals[n].Charge(), power);
            }
        }
        let s = &self.reactor_cores[r];
        let sequence = s.sequence.filter(|&q| !self.destroyed_components.contains(&self.do_at_times[q].id));
        if s.InternalTemperature > s.MeltdownTemperatureThreshold && sequence.is_some() && !s.hasMeltedDown {
            self.reactor_meltdown_start(r);
            self.reactor_cores[r].meltdownTime += dt;
            let s = &self.reactor_cores[r];
            if s.meltdownTime > s.MeltdownRequiredTime {
                self.reactor_melt_down(r, sequence.unwrap());
            } else {
                let p = self.reactor_cores[r].physical;
                self.physicals[p].Temperature = (d(self.physicals[p].Temperature) + d(dt) * 9.0f32).f();
                let at = self.transform_point3(p, [0.0, 0.0, 0.0]);
                self.camera_shake_at(dt * 15.0, [at[0], at[1]], 0.1);
            }
        } else {
            let s = &mut self.reactor_cores[r];
            s.meltdownStarted = false;
            s.meltdownTime = 0.0;
        }
    }

    /// ReactorCoreBehaviour.CalculateCriticality
    pub(crate) fn reactor_calculate_criticality(&mut self, r: usize) {
        let s = &mut self.reactor_cores[r];
        let num = s.powerOutput / s.lastPoweroutput;
        s.Criticality = if num < 0.99 {
            SUBCRITICAL
        } else if num > 1.01 {
            SUPERCRITICAL
        } else {
            CRITICAL
        };
        s.lastPoweroutput = s.powerOutput;
    }

    /// ReactorCoreBehaviour.CalculatePowerOutput
    pub(crate) fn reactor_power_output(&mut self, r: usize) -> f32 {
        if self.reactor_cores[r].hasMeltedDown {
            return 0.0;
        }
        let mut num = 1.0f32;
        for set in self.reactor_cores[r].conduits.clone() {
            match self.reactor_conduit_set_status(set) {
                QUESTIONABLE => num *= 0.7,
                DAMAGED => num *= 0.2,
                CATASTROPHIC => {
                    num *= 0.0;
                    break;
                }
                _ => {}
            }
        }
        let s = &self.reactor_cores[r];
        let (target, reference) = (s.ReactorPowerTargetRatio, s.ReferenceOutputPower);
        let v = self.random.range(-0.1, 0.1);
        let mut num2 = mathf::sqrt(mathf::clamp01(target + v)) * reference;
        if num2.is_nan() || num2.is_infinite() {
            num2 = reference;
        }
        mathf::max(0.0, num2) * num
    }

    /// ReactorCoreBehaviour.ControlTemperature
    pub(crate) fn reactor_control_temperature(&mut self, r: usize) {
        let dt = self.time.fixedDeltaTime;
        let time = self.time.time;
        let s = &mut self.reactor_cores[r];
        let num = ((d(mathf::clamp01(mathf::perlin(-578.2346, (d(time) * s.TimeVariationSpeed).f()))) * 2.0f32 - 1.0f32) * s.TimeVariationIntensity).f();
        s.InternalTemperature = (d(s.InternalTemperature) + d(num) * dt).f();
        s.InternalTemperature = (d(s.InternalTemperature) + d(s.ReactorPowerTargetRatio) * s.HeatUpIntensity * dt).f();
        let cooling = mathf::clamp01((d(s.CoolingPower) * s.CoolingPowerRatio / 2.0f32).f());
        let heat = mathf::clamp01(((d(s.InternalTemperature) + 273.15f32) / 100.0f32).f());
        let b = (d(s.CoolIntensity) * cooling * dt * heat).f();
        s.InternalTemperature -= mathf::max(0.0, b);
        s.InternalTemperature = mathf::max(s.InternalTemperature, -273.15);
    }

    /// ReactorCoreBehaviour.Diffuse
    pub(crate) fn reactor_diffuse(&mut self, r: usize) {
        let dt = self.time.fixedDeltaTime;
        let s = &self.reactor_cores[r];
        let p = s.physical;
        let internal = s.InternalTemperature;
        let temperature = self.physicals[p].Temperature;
        let t = (d(dt) * 10.0f32 / mathf::clamp(s.InsulationPower, 1.0, 16384.0)).f();
        self.reactor_cores[r].InternalTemperature = mathf::lerp(internal, temperature, t);
        self.physicals[p].Temperature = mathf::lerp(temperature, internal, t);
        if !self.reactor_cores[r].meltdownStarted {
            self.physicals[p].Temperature = mathf::lerp(self.physicals[p].Temperature, self.AmbientTemperature, 0.01);
        }
    }

    // ReactorConduitSetBehaviour.Awake: Conduits is the `conduits` link (GetComponentsInChildren).
    // ReactorCoreBehaviour.SynchroniseAlarmObjects: reactor_manage_groups below (SetActive on each alarm object; the behaviour arrays are empty).
    // ReactorCoreBehaviour.Awake: phys is the `physical` link; criticalityClock.RateHz = 8 is the clock's rate.
    /// ReactorCoreBehaviour.ManageActivationGroups: the state from InternalTemperature, and on a change (or `force`)
    /// SynchroniseAlarmObjects for the three groups (their behaviour arrays are empty: link checks).
    pub(crate) fn reactor_manage_groups(&mut self, r: usize, force: bool) {
        let s = &mut self.reactor_cores[r];
        s.State = 0;
        if s.InternalTemperature < s.LowTemperatureWarningThreshold {
            s.State = LOW_TEMP;
        } else if s.InternalTemperature > s.MeltdownTemperatureThreshold {
            s.State = MELTDOWN;
        } else if s.InternalTemperature > s.HighTemperatureThreshold {
            s.State = HIGH_TEMP;
        }
        if s.lastState != s.State || force {
            s.lastState = s.State;
            let state = s.State;
            let groups = [(s.low_alarm.clone(), state == LOW_TEMP), (s.high_alarm.clone(), state == HIGH_TEMP), (s.meltdown_alarm.clone(), state == MELTDOWN)];
            let id = s.id;
            self.with_component_instance(id, |g, inst, _| {
                for (objects, active) in groups {
                    for o in objects {
                        if inst.objects[o].active != active {
                            g.set_active_scripts(inst, o, active);
                        }
                    }
                }
            });
        }
    }

    /// A UnityEvent call of one of the core's public methods.
    pub fn reactor_core_call(&mut self, id: i32, call: ReactorCall) {
        let r = self.reactor_core_index(id);
        let s = &mut self.reactor_cores[r];
        match call {
            // ReactorCoreBehaviour.IncreaseCooling
            ReactorCall::IncreaseCooling => s.CoolingPower = mathf::clamp01(s.CoolingPower + 0.1),
            // ReactorCoreBehaviour.DecreaseCooling
            ReactorCall::DecreaseCooling => s.CoolingPower = mathf::clamp01(s.CoolingPower - 0.1),
            // ReactorCoreBehaviour.IncreasePower
            ReactorCall::IncreasePower => s.ReactorPowerTargetRatio = mathf::clamp01(s.ReactorPowerTargetRatio + 0.1),
            // ReactorCoreBehaviour.DecreasePower
            ReactorCall::DecreasePower => s.ReactorPowerTargetRatio = mathf::clamp01(s.ReactorPowerTargetRatio - 0.1),
            // ReactorCoreBehaviour.DecrementCoolingPower(ratio)
            ReactorCall::DecrementCoolingPower(ratio) => s.CoolingPowerRatio -= ratio,
            // ReactorCoreBehaviour.ForceMeltdown
            ReactorCall::ForceMeltdown => {
                s.InternalTemperature = s.MeltdownTemperatureThreshold * 2.0;
                s.CoolingPowerRatio = 0.0;
                s.ReactorPowerTargetRatio = 1.0;
            }
        }
    }

    /// ReactorA5DisplayBehaviour.Update: UpdateDisplayText once per cycle of the 10 Hz clock (the loop condition calls
    /// CalculateCycleCount again each time), then the four wires' GetStatus (a null wire throws).
    pub fn reactor_a5_display_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let mut n = 0;
        while n < self.reactor_a5_displays[i].updateDisplayClock.calculate_cycle_count(dt) {
            self.reactor_a5_display_update_text(i);
            n += 1;
        }
        self.reactor_a5_display_wires_and_beep(i);
    }

    /// ReactorA5DisplayBehaviour.UpdateDisplayText (AppendLine ends with Environment.NewLine, "\r\n" on Windows).
    pub(crate) fn reactor_a5_display_update_text(&mut self, i: usize) {
        const NL: &str = "\r\n";
        let Some(core) = self.reactor_a5_displays[i].Core else {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")
        };
        let c = &self.reactor_cores[core];
        let unit = self.prefs.TemperatureUnit;
        let suffix = super::utils::get_temperature_unit_suffix(unit);
        let temperature = super::display::cs_double((self.prefs.celsius_to_preference(c.InternalTemperature) as f64 * 100.0).round_ties_even() / 100.0);
        let mut s = String::new();
        s += crate::il!(str "ReactorA5DisplayBehaviour::UpdateDisplayText" 0);
        s += NL;
        s += NL;
        s += &format!("TEMPERATURE: {temperature}{suffix}\n");
        s += &format!("COOLING PUMPS: {}%\n", super::temperature::round_to_int(c.CoolingPower * 100.0));
        s += &format!("CONTROL ROD DEPTH: {}%\n", super::temperature::round_to_int((1.0 - c.ReactorPowerTargetRatio) * 100.0));
        s += NL;
        let criticality = match c.Criticality {
            SUBCRITICAL => "SUBCRITICAL",
            CRITICAL => "CRITICAL",
            SUPERCRITICAL => "SUPERCRITICAL",
            n => unimplemented!("mod-only: ReactorCoreBehaviour.Criticality {n}"),
        };
        s += &format!("REACTOR IS {criticality}\n");
        s += &format!("POWER OUTPUT: {} E\n", super::temperature::round_to_int(c.powerOutput));
        s += crate::il!(str "ReactorA5DisplayBehaviour::UpdateDisplayText" 6);
        s += NL;
        let t = self.time.time;
        if c.State == LOW_TEMP && t % 2.0 > 1.0 {
            s += crate::il!(str "ReactorA5DisplayBehaviour::UpdateDisplayText" 7);
        }
        if c.State == HIGH_TEMP && t % 2.0 > 1.0 {
            s += crate::il!(str "ReactorA5DisplayBehaviour::UpdateDisplayText" 8);
        }
        if c.State == MELTDOWN && t % 1.0 > 0.5 {
            s += crate::il!(str "ReactorA5DisplayBehaviour::UpdateDisplayText" 9);
        }
        let text = self.reactor_a5_displays[i].TextMesh;
        self.text_write(text, &s);
    }

    fn reactor_core_index(&self, id: i32) -> usize {
        self.reactor_cores.iter().position(|x| x.id == id).expect("a ReactorCoreBehaviour in the arena")
    }

    // ---- ReactorA5DisplayBehaviour --------------------------------------------------------------------------------

    /// The rest of ReactorA5DisplayBehaviour.Update: each wire's GetStatus (a null wire throws), then PlayAlertNoise(worst status): a
    /// Normal one returns, the others set the beep clock's rate (0.5, 1 and 4 Hz) and PlayClipOnce(AlertBeep, 0.1) runs
    /// once per cycle (the loop condition calls CalculateCycleCount(Time.deltaTime) again on every pass).
    fn reactor_a5_display_wires_and_beep(&mut self, i: usize) {
        let mut status = [NORMAL; 4];
        for k in 0..4 {
            let set = self.reactor_a5_displays[i].Wires[k].unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            status[k] = self.reactor_conduit_set_status(set);
        }
        let t = self.time.time;
        let normal = color_for_status(NORMAL, t);
        let s = status.map(|x| color_for_status(x, t));
        self.reactor_a5_displays[i].cols = [s[0], s[1], s[2], s[3], normal, normal, normal];
        let worst = status[0].max(status[1].max(status[2].max(status[3])));
        let rate = match worst {
            QUESTIONABLE => 0.5,
            DAMAGED => 1.0,
            CATASTROPHIC => 4.0,
            _ => return,
        };
        self.reactor_a5_displays[i].alertBeepClock.set_RateHz(rate);
        let dt = self.time.deltaTime;
        let mut n = 0;
        while n < self.reactor_a5_displays[i].alertBeepClock.calculate_cycle_count(dt) {
            let (id, p) = (self.reactor_a5_displays[i].id, self.reactor_a5_displays[i].physical);
            let clip = self.audio.clip(id, "AlertBeep");
            let p = p.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            self.play_clip_once(p, clip, 0.1);
            n += 1;
        }
    }

    // ---- ReactorConduitSetBehaviour -------------------------------------------------------------------------------

    /// ReactorConduitSetBehaviour.GetStatus
    pub fn reactor_conduit_set_status(&self, set: usize) -> i32 {
        let mut num = 0;
        let mut flag = false;
        let conduits = &self.reactor_conduit_sets[set].conduits;
        for &c in conduits {
            num += self.reactor_conduits[c].Broken as i32;
            flag |= !self.reactor_conduits[c].HealthyWallJoint;
        }
        if num >= conduits.len() as i32 / 2 {
            CATASTROPHIC
        } else if num > 2 {
            DAMAGED
        } else if flag || num > 0 {
            QUESTIONABLE
        } else {
            NORMAL
        }
    }

    /// ReactorConduitSetBehaviour.Start: every collider below each NoCollide body ignores every collider below each conduit.
    pub fn reactor_conduit_set_start(&mut self, set: usize) {
        let s = &self.reactor_conduit_sets[set];
        let (id, no_collide, conduit_objects) = (s.id, s.no_collide.clone(), s.conduit_objects.clone());
        let pairs: Vec<(i32, i32)> = self.with_component_instance(id, |_, inst, _| {
            let mut out = Vec::new();
            for &n in &no_collide {
                let a = super::car::colliders_below(inst, n);
                for &c in &conduit_objects {
                    let b = super::car::colliders_below(inst, c);
                    for &(_, ka) in &a {
                        for &(_, lb) in &b {
                            out.push((lb, ka));
                        }
                    }
                }
            }
            out
        });
        for (x, y) in pairs {
            // Physics2D.IgnoreCollision(x, y): a collider cannot ignore itself.
            if x != y && self.collider_exists(x) && self.collider_exists(y) {
                self.world.ignore_collision(x, y, true);
            }
        }
    }

    // ---- ReactorConduitBehaviour ----------------------------------------------------------------------------------

    /// `(bool)joint` for a joint's key: the component still exists.
    fn joint_key_alive(&self, key: BreakJoint) -> bool {
        match key {
            BreakJoint::Hinge(h) => !self.hinges[h].destroyed,
            BreakJoint::Fixed(f) => !self.fixed_joints[f].destroyed,
            BreakJoint::Anchored(slot) => !self.destroyed_components.contains(&self.anchored_joints[slot].component) && self.adopted_slots.get(&slot).is_none_or(|&f| !self.fixed_joints[f].destroyed),
            BreakJoint::Spring(l) => self.limbs[l].gore.joint.is_some(),
        }
    }

    /// ReactorConduitBehaviour.FixedUpdate
    pub fn reactor_conduit_fixed_update(&mut self, c: usize) {
        let o = Owner::ReactorConduit(c);
        let s = &self.reactor_conduits[c];
        if s.inflow_amount > 0.0 {
            let amount = s.inflow_amount;
            match s.inflow_liquid {
                Some(liquid) => {
                    self.add_liquid(o, liquid, amount);
                }
                // Liquid.GetLiquid logs the error and returns null.
                None => {
                    self.add_null_liquid(o, amount);
                }
            }
        }
        let s = &self.reactor_conduits[c];
        let (phys, tube, reactor_joint, target, core) = (s.physical, s.tube, s.reactor_joint, s.LiquidPushTarget, s.core);
        if let Some(other) = s.connected {
            self.average_temperature(phys, other, 0.2);
            self.transfer_energy_fixed_rate(phys, other, 0.99);
        }
        if let Some(t) = target {
            if !self.is_full(Owner::ReactorConduit(t), 0.999) {
                self.transfer_to(o, 0.1, Owner::ReactorConduit(t));
            }
        }
        if let (Some(k), Some(joint)) = (core, reactor_joint) {
            if self.joint_key_alive(joint) {
                self.physicals[phys].Temperature = mathf::lerp(self.physicals[phys].Temperature, self.reactor_cores[k].InternalTemperature, 0.5);
            }
        }
        if let Some((id, _)) = tube.filter(|&(_, key)| self.joint_key_alive(key)) {
            let s = &self.reactor_conduits[c];
            let resilience = s.randomResilience;
            let temperature = self.physicals[phys].Temperature;
            let a = mathf::clamp01(mathf::map_range((d(-100.0f32) - resilience).f(), 20.0, 0.0, 1.0, temperature));
            let b = mathf::clamp01(mathf::map_range(5000.0, (d(10000.0f32) + resilience).f(), 1.0, 0.0, temperature));
            let force = s.initialBreakForce * mathf::min(a, b);
            self.joint_set_break_force(id, force);
            let broken = self.reactor_conduits[c].Broken;
            if !broken && temperature > (d(6000.0f32) + resilience).f() && self.random.value() > 0.9999 {
                let at = self.transform_point3(phys, [0.0, 0.0, 0.0]);
                // ExplosionParameters(16u, position, 4f, 64f, createFx: true, EffectSize.Small, 0f, 4)
                self.explosion_creator_explode(ExplosionParameters {
                    FragmentationRayCount: 16,
                    Position: at,
                    Range: 4.0,
                    FragmentForce: 64.0,
                    CreateParticlesAndSound: true,
                    EffectSize: EffectSize::Small,
                    DismemberChance: 0.0,
                    BallisticShrapnelCount: 4,
                });
            }
        }
    }

    /// ReactorConduitBehaviour.OnJointBreak2D, sent to the broken joint's GameObject: the conduit whose tube or wall joint it is
    /// (the only joints on a conduit's object). The joint is still a component here.
    pub fn reactor_conduits_on_joint_break(&mut self, j: BreakJoint) {
        for c in 0..self.reactor_conduits.len() {
            let s = &self.reactor_conduits[c];
            if self.destroyed_components.contains(&s.id) {
                continue;
            }
            let (tube, wall) = (s.tube.map(|(_, k)| k) == Some(j), s.reactor_joint == Some(j));
            if !(tube || wall) {
                continue;
            }
            // phys.PlayClipOnce(BreakClips.PickRandom())
            let (id, clips, phys) = (s.id, s.clips, s.physical);
            let clip = self.pick_clip(id, "BreakClips", clips);
            self.play_clip_once(phys, clip, 1.0);
            if tube {
                if let Some(t) = self.reactor_conduits[c].LiquidPushTarget {
                    if self.reactor_conduits[c].connected_object == Some(self.reactor_conduits[t].object) {
                        self.reactor_conduits[t].source = None;
                        self.reactor_conduits[c].LiquidPushTarget = None;
                    }
                }
                self.reactor_conduits[c].Broken = true;
            } else {
                self.reactor_conduits[c].HealthyWallJoint = false;
            }
            // (bool)source && joint == source.tubeJoint
            let tube_of = |g: &Game, x: usize| g.reactor_conduits[x].tube.map(|(_, k)| k).filter(|&k| g.joint_key_alive(k));
            if let Some(src) = self.reactor_conduits[c].source {
                if tube_of(self, src) == Some(j) {
                    self.reactor_conduits[src].LiquidPushTarget = None;
                    self.reactor_conduits[c].source = None;
                }
            }
            // (bool)LiquidPushTarget && joint == LiquidPushTarget.tubeJoint
            if let Some(t) = self.reactor_conduits[c].LiquidPushTarget {
                if tube_of(self, t) == Some(j) {
                    self.reactor_conduits[t].source = None;
                    self.reactor_conduits[c].LiquidPushTarget = None;
                }
            }
        }
    }

    /// ReactorBehaviour.Update, once meltdown starts: OnMeltdownStart the first time.
    pub fn reactor_meltdown_start(&mut self, r: usize) {
        if !self.reactor_cores[r].meltdownStarted {
            self.reactor_cores[r].meltdownStarted = true;
            for call in self.reactor_cores[r].on_meltdown_start.clone() {
                self.event_call(call);
            }
        }
    }

    /// ReactorBehaviour.Update past MeltdownRequiredTime: DisableOnMeltdown's objects off, the MeltdownSequence (`sequence`) enabled.
    pub fn reactor_melt_down(&mut self, r: usize, sequence: usize) {
        if let Some(objects) = self.reactor_cores[r].disable_on_meltdown.clone() {
            let id = self.reactor_cores[r].id;
            self.with_component_instance(id, |g, inst, _| {
                for o in objects {
                    g.set_active_scripts(inst, o, false);
                }
            });
        }
        self.do_at_time_enable(sequence);
        self.reactor_cores[r].hasMeltedDown = true;
    }

    // ---- ReactorMeltdownBehaviour ---------------------------------------------------------------------------------

    /// ReactorMeltdownBehaviour.FixedUpdate
    pub fn reactor_meltdown_fixed_update(&mut self, m: usize) {
        let s = &self.reactor_meltdowns[m];
        if s.MeltdownActive && self.AmbientTemperature < s.AmbientHeatupMax {
            self.AmbientTemperature = (d(self.AmbientTemperature) + d(self.time.fixedDeltaTime) * s.AmbientHeatupSpeed).f();
        }
    }

    /// ReactorMeltdownBehaviour.Explode
    pub fn reactor_meltdown_explode(&mut self, id: i32) {
        let m = self.reactor_meltdowns.iter().position(|x| x.id == id).expect("a ReactorMeltdownBehaviour in the arena");
        self.reactor_meltdowns[m].MeltdownActive = true;
        let (temperature, charge, layers) = (self.reactor_meltdowns[m].SetTemperature, self.reactor_meltdowns[m].SetCharge, self.reactor_meltdowns[m].SparkHitLayers);
        // CameraShakeBehaviour.main.Shake(3000f, transform.position, 0.1f); transform.root.GetComponentsInChildren<MapLightBehaviour>():
        // the components of the active objects below the root, depth first.
        let (at, lights) = self.with_component_instance(id, |_, inst, o| {
            let mut root = o;
            while let Some(up) = inst.objects[root].parent {
                root = up;
            }
            let at = inst.position(o);
            let prefab = inst.prefab.clone();
            let mut objects = Vec::new();
            super::car::active_below(inst, root, &mut objects);
            let mut lights = Vec::new();
            for x in objects {
                for (k, (_, comp)) in prefab.objects[x].components.iter().enumerate() {
                    if matches!(comp, crate::assets::db::Component::Script { class, .. } if class == "MapLightBehaviour") {
                        lights.push(inst.objects[x].components[k]);
                    }
                }
            }
            ([at[0], at[1]], lights)
        });
        self.camera_shake_at(3000.0, at, 0.1);
        for light_id in lights {
            let l = self.map_lights.iter().position(|x| x.id == light_id).expect("a MapLightBehaviour in the arena");
            // DeactivateInstantly acts while the object is active in the hierarchy; SetActive(false) follows.
            if self.with_component_instance(light_id, |_, inst, o| {
                let mut x = o;
                loop {
                    if !inst.objects[x].active {
                        break false;
                    }
                    match inst.objects[x].parent {
                        Some(up) => x = up,
                        None => break true,
                    }
                }
            }) {
                self.map_light_deactivate_instantly(l);
            }
            self.with_component_instance(light_id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        for q in 0..self.physicals.len() {
            if !self.physicals[q].in_world || self.physicals[q].destroyed || self.physicals[q].isStatic || !super::physical::has_layer(layers, self.physical_layer(q)) {
                continue;
            }
            if self.physicals[q].SimulateTemperature {
                self.physicals[q].Temperature = mathf::max(temperature, self.physicals[q].Temperature);
            }
            self.physicals[q].charge = mathf::max(charge, self.physicals[q].Charge());
            let v = self.random.inside_unit_circle();
            let impulse = [v[0] * 10.0, v[1] * 10.0];
            let local = self.random_grid_point(q);
            let rb = self.physicals[q].rigidbody.clone();
            // An inactive object's Rigidbody2D has no body: GetRelativePoint is zero, AddForceAtPosition does nothing.
            let point = self.rigidbody_point_relative(&rb, local);
            rb.add_impulse_at_position(&mut self.world, impulse, point);
            let v = self.random.inside_unit_circle();
            // SendMessage skips an inactive GameObject.
            if self.physicals[q].active {
                self.send_break(-1, Some(q), [v[0] * 4.0, v[1] * 4.0]);
            }
        }
    }

    // ---- DoAtTime -------------------------------------------------------------------------------------------------

    /// Behaviour.enabled = true on a DoAtTime: nothing when it already is; else, on an active object, AddToManager.
    pub fn do_at_time_enable(&mut self, i: usize) {
        let id = self.do_at_times[i].id;
        if !std::mem::replace(&mut self.do_at_times[i].enabled, true) && self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            let (order, b) = self.behaviour_of[&id];
            self.behaviours.add(order, b);
        }
    }

    /// DoAtTime.Update
    pub fn do_at_time_update(&mut self, i: usize) {
        if !self.do_at_times[i].enabled {
            return;
        }
        self.do_at_times[i].time += self.time.deltaTime;
        for k in 0..self.do_at_times[i].Events.len() {
            let e = &mut self.do_at_times[i].Events[k];
            if !e.Passed && self.do_at_times[i].time > self.do_at_times[i].Events[k].Time {
                self.do_at_times[i].Events[k].Passed = true;
                for call in self.do_at_times[i].Events[k].calls.clone() {
                    self.event_call(call);
                }
            }
        }
    }
}

impl Behaviour {
    /// The reactor's behaviours by arena index (used by link.rs).
    pub fn is_reactor(&self) -> bool {
        matches!(self, Behaviour::ReactorCore(_) | Behaviour::ReactorConduit(_) | Behaviour::DoAtTime(_) | Behaviour::ReactorMeltdown(_))
    }
}

/// ReactorA5DisplayBehaviour.Square: Mathf.Sin((float)Math.PI * x) > 0 ? 1 : 0.
pub(crate) fn square(x: f32) -> f32 {
    if super::mathf::sin(std::f32::consts::PI * x) > 0.0 { 1.0 } else { 0.0 }
}

/// ReactorA5DisplayBehaviour.GetColorForStatus (Color.Lerp at t 0 or 1 gives an end exactly).
pub(crate) fn color_for_status(status: i32, time: f32) -> [f32; 4] {
    const WHITE: [f32; 4] = [1.0; 4];
    const YELLOW: [f32; 4] = [1.0, 0.921_568_63, 0.015_686_275, 1.0];
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const CLEAR: [f32; 4] = [0.0; 4];
    let pick = |a: [f32; 4], b: [f32; 4], t: f32| if t > 0.0 { b } else { a };
    match status {
        QUESTIONABLE => pick(WHITE, YELLOW, square(time)),
        DAMAGED => pick(YELLOW, RED, square(time * 4.0)),
        CATASTROPHIC => pick(CLEAR, RED, square(time * 9.0)),
        _ => WHITE,
    }
}

impl Game {
    /// ReactorA5DisplayBehaviour.OnWillRenderObject of each display whose sprite is drawn: SetColor of _Wire1 to _Wire4,
    /// _VacuumChamber, _Connector and _Base on DiagnosticsMaterial, the shared material, so every renderer drawing with it
    /// shows them (a null material throws).
    pub fn reactor_a5_displays_will_render(&mut self) {
        const NAMES: [&str; 7] = ["_Wire1", "_Wire2", "_Wire3", "_Wire4", "_VacuumChamber", "_Connector", "_Base"];
        for i in 0..self.reactor_a5_displays.len() {
            let s = &self.reactor_a5_displays[i];
            let Some(p) = s.physical else { continue };
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(super::visual::SpriteAt::Physical(p))) {
                continue;
            }
            let material = s.DiagnosticsMaterial.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            let cols = s.cols;
            let users: Vec<usize> = (0..self.renderers.len()).filter(|&r| self.renderers[r].material == Some(material)).collect();
            for r in users {
                for k in 0..7 {
                    self.set_material_prop(super::visual::SpriteAt::Renderer(r), NAMES[k], super::props::Prop::Color(cols[k]));
                }
            }
        }
    }
}

#[cfg(test)]
mod display_colour_tests {
    #[test]
    fn color_for_status() {
        assert_eq!(super::color_for_status(super::NORMAL, 0.7), [1.0; 4]);
        assert_eq!(super::color_for_status(super::QUESTIONABLE, 0.5), [1.0, 0.921_568_63, 0.015_686_275, 1.0]);
        assert_eq!(super::color_for_status(super::QUESTIONABLE, 1.5), [1.0; 4]);
        assert_eq!(super::color_for_status(super::DAMAGED, 0.125), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(super::color_for_status(super::CATASTROPHIC, 0.0), [0.0; 4]);
    }
}
