//! JetEngineBehaviour (the Jet Engine): Use toggles it; the startup and shutoff coroutines ramp the intake and exhaust
//! PointEffector2Ds, the exhaust heats what it overlaps, and the Active state pushes the body along its right vector.
//! Awake caches components and clears the plasma sprite's colour, and SetVisualJetIntensity sets the plasma and
//! light sprites' colours (StartupIntensityCurve feeds it); the AudioSource calls play sounds (audio).
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::translated::TBehaviour;
use super::visual::SpriteAt;
use super::{d, mathf, script, Game};
use crate::b2::world::FixtureId;
use crate::game_assets::{Curve, CurveCache};

/// JetEngineBehaviour.State
const IDLE: i32 = 0;
const STARTING: i32 = 1;
const ACTIVE: i32 = 2;
const SHUTDOWN: i32 = 3;

script! {
    JetEngine {
        IntakeForceMultiplier: f32 = 1.0,
        ExhaustForceMultiplier: f32 = 1.0,
        ExhaustTemperature: f32 = 800.0,
        StartupDurationSeconds: f32 = 11.0,
        ShutoffDurationSeconds: f32 = 1.0,
        ScreenShakeIntensity: f32 = 0.25,
        WillDrown: bool = true,
        Activated: bool = false,
        CurrentState: i32 = 0,
        Force: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour: its body, Transform and IsUnderWater.
        PhysicalBehaviour: usize = 0,
        /// DamagableMachinery
        DamagableMachinery: usize = 0,
        /// IntakeEffector and ExhaustEffector: the PointEffector2D component ids.
        IntakeEffector: i32 = 0,
        ExhaustEffector: i32 = 0,
        /// exhaustEffectorCollider: the fixtures of the first Collider2D on the ExhaustEffector's GameObject.
        exhaustEffectorCollider: Vec<FixtureId> = Vec::new(),
        /// guideTemperatureBehaviour: GetComponent<GuideTemperatureBehaviour>() (a translated behaviour); None is null.
        guideTemperatureBehaviour: Option<TBehaviour> = None,
        /// PlasmaJet and Lighting (SetVisualJetIntensity dereferences both).
        PlasmaJet: Option<SpriteAt> = None,
        Lighting: Option<SpriteAt> = None,
        /// StartupIntensityCurve and its evaluation cache.
        StartupIntensityCurve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
    }
}

impl Game {
    /// JetEngineBehaviour.Awake: PlasmaJet.color = Color.clear (the components are links).
    pub fn jet_engine_awake(&mut self, j: usize) {
        let plasma = self.jet_engines[j].PlasmaJet.expect("JetEngineBehaviour.PlasmaJet null (NullReferenceException)");
        self.set_sprite_color(plasma, [0.0; 4]);
    }

    /// JetEngineBehaviour.SetVisualJetIntensity: PlasmaJet.color = (0, 0, 0, i); Lighting.color.a = i * i.
    pub fn jet_set_visual_intensity(&mut self, j: usize, i: f32) {
        let plasma = self.jet_engines[j].PlasmaJet.expect("JetEngineBehaviour.PlasmaJet null (NullReferenceException)");
        self.set_sprite_color(plasma, [0.0, 0.0, 0.0, i]);
        let lighting = self.jet_engines[j].Lighting.expect("JetEngineBehaviour.Lighting null (NullReferenceException)");
        let [r, g, b, _] = self.sprite_color(lighting);
        self.set_sprite_color(lighting, [r, g, b, i * i]);
    }

    fn jet_set_exhaust(&mut self, j: usize, v: f32) {
        self.effector_set_force_magnitude(self.jet_engines[j].ExhaustEffector, v);
    }

    fn jet_set_intake(&mut self, j: usize, v: f32) {
        self.effector_set_force_magnitude(self.jet_engines[j].IntakeEffector, v);
    }

    /// guideTemperatureBehaviour.enabled = v (Behaviour::SetEnabled: AddToManager/RemoveFromManager on an active
    /// GameObject; GuideTemperatureBehaviour has no OnEnable or OnDisable).
    fn jet_set_guide_enabled(&mut self, j: usize, v: bool) {
        let Some(TBehaviour::GuideTemperatureBehaviour(g)) = self.jet_engines[j].guideTemperatureBehaviour else {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        };
        if std::mem::replace(&mut self.tr.guide_temperature_behaviour[g].enabled, v) == v {
            return;
        }
        let id = self.tr.guide_temperature_behaviour[g].this_id;
        if self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            let (order, b) = self.behaviour_of[&id];
            if v {
                self.behaviours.add(order, b);
            } else {
                self.behaviours.remove(b);
            }
        }
    }

    /// base.transform.localScale.x and base.transform.position (the jet's PhysicalBehaviour is on its GameObject).
    fn jet_transform(&self, j: usize) -> (f32, [f32; 2]) {
        let t = &self.physicals[self.jet_engines[j].PhysicalBehaviour].transform;
        (t.localScale[0], [t.position[0], t.position[1]])
    }

    /// PhysicalBehaviour.rigidbody.AddRelativeForce(new Vector2(x, 0), ForceMode2D.Force)
    fn jet_add_relative_force(&mut self, j: usize, x: f32) {
        let rb = self.physicals[self.jet_engines[j].PhysicalBehaviour].rigidbody.clone();
        rb.add_relative_force(&mut self.world, [x, 0.0]);
    }

    /// JetEngineBehaviour.FixedUpdate
    pub fn jet_engine_fixed_update(&mut self, j: usize) {
        match self.jet_engines[j].CurrentState {
            IDLE | SHUTDOWN => {
                self.jet_set_exhaust(j, 0.0);
                self.jet_set_intake(j, 0.0);
            }
            STARTING => self.jet_drown(j),
            ACTIVE => {
                let (scale, position) = self.jet_transform(j);
                let num = scale.abs();
                let s = &self.jet_engines[j];
                let (exhaust, intake, force, shake) = (s.ExhaustForceMultiplier * num, s.IntakeForceMultiplier * num, s.Force * scale, s.ScreenShakeIntensity);
                self.jet_set_exhaust(j, exhaust);
                self.jet_set_intake(j, intake);
                self.jet_exhaust_affect_objects(j, 1.0);
                self.jet_add_relative_force(j, force);
                self.camera_shake(shake, position);
                self.jet_drown(j);
            }
            _ => {}
        }
    }

    /// `if (WillDrown && PhysicalBehaviour.IsUnderWater) DamagableMachinery.ForceBreak();`
    pub(crate) fn jet_drown(&mut self, j: usize) {
        let s = &self.jet_engines[j];
        if s.WillDrown && self.physicals[s.PhysicalBehaviour].IsUnderWater {
            self.machinery_force_break(s.DamagableMachinery);
        }
    }

    /// JetEngineBehaviour.ExhaustAffectObjects: Physics2D.OverlapCollider with a ContactFilter2D that sets only the layer
    /// mask (useLayerMask, useTriggers and the depth tests stay off) into the 32-entry buffer.
    pub(crate) fn jet_exhaust_affect_objects(&mut self, j: usize, intensity: f32) {
        let t = 0.05f32 * intensity;
        let fixtures = self.jet_engines[j].exhaustEffectorCollider.clone();
        let mut hits = self.overlap_collider(&fixtures, u32::MAX);
        // NonAlloc copies the first 32 of the sorted results.
        hits.truncate(JET_ENGINE_BUFFER);
        let (own, target) = (self.jet_engines[j].PhysicalBehaviour, self.jet_engines[j].ExhaustTemperature);
        for (physical, _) in hits {
            // collider.gameObject != base.gameObject, and PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform).
            let Some(p) = physical.filter(|&p| p != own) else { continue };
            if self.physicals[p].SimulateTemperature && self.physicals[p].Temperature < target {
                self.physicals[p].Temperature = mathf::lerp(self.physicals[p].Temperature, target, t);
            }
        }
    }

    /// StartCoroutine on the jet engine's own GameObject.
    fn jet_start_coroutine(&mut self, j: usize, r: Routine) {
        let id = self.jet_engines[j].id;
        self.start_coroutine(id, id, r);
    }

    /// JetEngineBehaviour.StartupRoutine
    pub(crate) fn jet_startup_routine(j: usize) -> Routine {
        let mut started = false;
        let mut t = 0.0f32;
        Box::new(move |g| {
            if !started {
                started = true;
                if g.jet_engines[j].CurrentState != IDLE {
                    return Yield::Done;
                }
                g.jet_engines[j].CurrentState = STARTING;
                t = 0.0;
                g.audio_one_shot(g.jet_engines[j].id, "Source", "StartupSound");
                g.jet_set_visual_intensity(j, 0.0);
                g.jet_set_guide_enabled(j, true);
            }
            let s = &g.jet_engines[j];
            if t < s.StartupDurationSeconds {
                let num = t / s.StartupDurationSeconds;
                let num2 = (d(num) * num * 0.25f32).f();
                let (scale, position) = g.jet_transform(j);
                let num3 = num2 * scale.abs();
                let s = &g.jet_engines[j];
                let (exhaust, intake) = (s.ExhaustForceMultiplier * num3, s.IntakeForceMultiplier * num3);
                let (shake, force) = ((d(num) * s.ScreenShakeIntensity * 0.5f32).f(), (d(s.Force) * scale * num * 0.4f32).f());
                g.jet_set_exhaust(j, exhaust);
                g.jet_set_intake(j, intake);
                g.jet_exhaust_affect_objects(j, num2);
                let curve = g.jet_engines[j].StartupIntensityCurve.clone().expect("JetEngineBehaviour.StartupIntensityCurve null (NullReferenceException)");
                let intensity = curve.evaluate(&mut g.jet_engines[j].curve_cache, num);
                g.jet_set_visual_intensity(j, intensity);
                g.camera_shake(shake, position);
                g.jet_add_relative_force(j, force);
                t += g.time.deltaTime;
                return Yield::WaitForFixedUpdate;
            }
            // Source.Play()
            let src = g.audio_src(g.jet_engines[j].id, "Source");
            g.audio.play(src);
            g.jet_set_visual_intensity(j, 1.0);
            let (_, position) = g.jet_transform(j);
            let shake = 40.0f32 * g.jet_engines[j].ScreenShakeIntensity;
            g.camera_shake(shake, position);
            g.jet_engines[j].CurrentState = ACTIVE;
            Yield::Done
        })
    }

    /// JetEngineBehaviour.ShutoffRoutine
    pub(crate) fn jet_shutoff_routine(j: usize) -> Routine {
        let mut started = false;
        let mut waiting = false;
        let mut t = 0.0f32;
        Box::new(move |g| {
            if !started {
                started = true;
                if g.jet_engines[j].CurrentState != ACTIVE {
                    return Yield::Done;
                }
                g.jet_engines[j].CurrentState = SHUTDOWN;
                t = 0.0;
                let src = g.audio_src(g.jet_engines[j].id, "Source");
                g.audio.stop(src);
                g.audio_one_shot(g.jet_engines[j].id, "Source", "ShutoffSound");
                g.jet_set_visual_intensity(j, 1.0);
                g.jet_set_guide_enabled(j, false);
            }
            if waiting {
                g.jet_engines[j].CurrentState = IDLE;
                return Yield::Done;
            }
            if t < g.jet_engines[j].ShutoffDurationSeconds {
                let intensity = (d(1.0) - d(t) / g.jet_engines[j].ShutoffDurationSeconds).f();
                g.jet_set_visual_intensity(j, intensity);
                t += g.time.deltaTime;
                return Yield::WaitForEndOfFrame;
            }
            g.jet_set_visual_intensity(j, 0.0);
            g.jet_set_exhaust(j, 0.0);
            g.jet_set_intake(j, 0.0);
            waiting = true;
            Yield::WaitForSeconds(4.0)
        })
    }

    /// JetEngineBehaviour.OnAfterDeserialise: a saved state resumes.
    pub(crate) fn jet_engine_on_after_deserialise(&mut self, j: usize) {
        match self.jet_engines[j].CurrentState {
            STARTING => {
                self.jet_engines[j].CurrentState = IDLE;
                let src = self.audio_src(self.jet_engines[j].id, "Source");
                self.audio.stop(src);
                self.jet_set_visual_intensity(j, 0.0);
                self.jet_start_coroutine(j, Self::jet_startup_routine(j));
            }
            ACTIVE => {
                self.jet_set_guide_enabled(j, true);
                self.jet_set_visual_intensity(j, 1.0);
                let src = self.audio_src(self.jet_engines[j].id, "Source");
                self.audio.play(src);
            }
            SHUTDOWN => {
                self.jet_engines[j].CurrentState = ACTIVE;
                self.jet_set_visual_intensity(j, 1.0);
                self.jet_start_coroutine(j, Self::jet_shutoff_routine(j));
            }
            _ => {}
        }
    }

    /// JetEngineBehaviour.Use
    pub fn jet_engine_use(&mut self, j: usize) {
        if self.machinery[self.jet_engines[j].DamagableMachinery].Destroyed {
            return;
        }
        let activated = !self.jet_engines[j].Activated;
        self.jet_engines[j].Activated = activated;
        if activated {
            self.jet_engine_try_startup(j);
        } else {
            self.jet_engine_try_shutoff(j);
        }
    }

    /// JetEngineBehaviour.TryStartup: the startup routine, unless the machine is destroyed.
    pub fn jet_engine_try_startup(&mut self, j: usize) {
        if !self.machinery[self.jet_engines[j].DamagableMachinery].Destroyed {
            self.jet_start_coroutine(j, Self::jet_startup_routine(j));
        }
    }

    /// JetEngineBehaviour.TryShutoff
    pub fn jet_engine_try_shutoff(&mut self, j: usize) {
        self.jet_start_coroutine(j, Self::jet_shutoff_routine(j));
    }

    /// JetEngineBehaviour.ForceShutoff (DamagableMachineryBehaviour.OnDamaged's call).
    pub fn jet_engine_force_shutoff(&mut self, id: i32) {
        let j = self.jet_engines.iter().position(|x| x.id == id).expect("a JetEngineBehaviour in the arena");
        self.stop_all_coroutines(id);
        self.jet_engines[j].Activated = false;
        self.jet_set_exhaust(j, 0.0);
        self.jet_set_exhaust(j, 0.0);
        self.jet_set_intake(j, 0.0);
        self.jet_engines[j].CurrentState = IDLE;
        let src = self.audio_src(id, "Source");
        self.audio.stop(src);
        self.jet_set_visual_intensity(j, 0.0);
        self.jet_set_guide_enabled(j, false);
    }

    // JetEngineBehaviour.Awake: jet_engine_awake (guideTemperatureBehaviour and the two effector colliders are links: exhaustEffectorCollider).
    // JetEngineBehaviour.SetVisualJetIntensity: jet_set_visual_intensity (PlasmaJet and Lighting colours).
}

/// JetEngineBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const JET_ENGINE_BUFFER: usize = 32;
