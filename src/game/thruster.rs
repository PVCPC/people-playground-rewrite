//! ThrusterbedBehaviour. The effect plays while on, with its start speed and colour; its noise strength and the context
//! menu are presentation; the activation, thrust, burn rays, temperature and Random draws stay.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};
use crate::b2::math::Vec2 as V;
use crate::game_assets::{Curve, CurveCache};

/// ThrusterbedBehaviour.buffer's length (its static setup: new RaycastHit2D[32]): LinecastNonAlloc's cap in BurnOthers.
pub(crate) const THRUSTERBED_BUFFER: usize = 32;

script! {
    Thrusterbed {
        Activated: bool = false,
        ThrustingForce: f32 = 320.0,
        RelativeDirection: [f32; 2] = [0.0, 1.0],
        DoBurnOthers: bool = true,
        TemperatureTarget: f32 = 600.0,
        BurnRayCount: i32 = 3,
        BurnRayWidth: f32 = 1.0,
        BurnRayLength: f32 = 5.0,
        BurnRayOffset: [f32; 2] = [0.0, 0.0],
        BurnRayPerpendicular: [f32; 2] = [0.0, 1.0],
        /// 1.28.45 (absent from 1.28.41's prefabs, so 1)
        ChargeBoostModifier: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>() (its Rigidbody2D is GetComponent<Rigidbody2D>())
        Phys: usize = 0,
        /// LayersToBurn (LayerMask bits)
        LayersToBurn: u32 = 0,
        StartupStrengthCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        time: f32 = 0.0,
    }
}

// ThrusterbedBehaviour.Awake: rb and physicalBehaviour are the `physical` link.

impl Game {
    /// ThrusterbedBehaviour.Boost (1.28.45): Charge * ChargeBoostModifier; 1.28.41 reads physicalBehaviour.Charge where it is used.
    fn thrusterbed_boost(&self, i: usize) -> f32 {
        let charge = self.physicals[self.thrusterbeds[i].Phys].Charge();
        if crate::assets::il::is_1_28_45() {
            (d(charge) * self.thrusterbeds[i].ChargeBoostModifier).f()
        } else {
            charge
        }
    }

    /// ThrusterbedBehaviour.Start
    pub fn thrusterbed_start(&mut self, i: usize) {
        self.thrusterbed_update_activated(i);
    }

    /// ThrusterbedBehaviour.Use
    pub fn thrusterbed_use(&mut self, i: usize) {
        if self.thrusterbeds[i].enabled {
            self.thrusterbeds[i].Activated = !self.thrusterbeds[i].Activated;
            self.thrusterbed_update_activated(i);
        }
    }

    /// ThrusterbedBehaviour.Shocked(zap): an idle thruster bed starts (Use(null)). Nothing in the game sends the message, so no
    /// caller reaches it.
    #[allow(dead_code)]
    pub fn thrusterbed_shocked(&mut self, i: usize) {
        if !self.thrusterbeds[i].Activated {
            self.thrusterbed_use(i);
        }
    }

    /// lowAudio and highAudio (a null one throws).
    pub(crate) fn thrusterbed_sources(&self, i: usize) -> (i32, i32) {
        let id = self.thrusterbeds[i].id;
        let get = |n: &str| self.audio.source_field(id, n).unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        (get("lowAudio"), get("highAudio"))
    }

    /// ThrusterbedBehaviour.UpdateAudio
    pub(crate) fn thrusterbed_update_audio(&mut self, i: usize, progress: f32) {
        let (low, high) = self.thrusterbed_sources(i);
        let charge = self.thrusterbed_boost(i);
        let a = self.audio.get_mut(high);
        a.volume = progress * charge;
        a.min_distance = 10.0 + progress * charge;
        self.audio.get_mut(low).volume = progress;
    }

    /// ThrusterbedBehaviour.UpdateActivated
    pub(crate) fn thrusterbed_update_activated(&mut self, i: usize) {
        self.thrusterbed_update_particle_system(i, 0.0);
        self.thrusterbed_update_audio(i, 0.0);
        let (low, high) = self.thrusterbed_sources(i);
        let id = self.thrusterbeds[i].id;
        if self.thrusterbeds[i].Activated {
            if let Some(clip) = self.audio.clip(id, "IgnitionSound") {
                self.audio.play_one_shot(low, Some(&clip), 1.0);
            }
            self.ps_field_call(id, "effect", false, super::PsCall::Play);
            self.audio.play(low);
            self.audio.play(high);
        } else {
            self.ps_field_call(id, "effect", false, super::PsCall::Stop(self.time.timeAsDouble));
            self.audio.stop(low);
            self.audio.stop(high);
            if let Some(clip) = self.audio.clip(id, "ExtinguishSound") {
                self.play_clip_once(self.thrusterbeds[i].Phys, Some(clip), 1.0);
            }
        }
        self.thrusterbeds[i].time = 0.0;
    }

    /// ThrusterbedBehaviour.UpdateParticleSystem: startSpeedMultiplier = progress * Random.Range(5f, 12f) + Charge * 0.5,
    /// startColor 4 * (num, num - Charge * 0.2, num + Charge * 0.6) * Max(1, Charge * 5) with num = Max(0.1, Charge / 8).
    /// (noise.strengthMultiplier = progress * 2 is not drawn.)
    pub(crate) fn thrusterbed_update_particle_system(&mut self, i: usize, progress: f32) {
        let r = self.random.range(5.0, 12.0);
        let (id, charge) = (self.thrusterbeds[i].id, self.thrusterbed_boost(i));
        let num = mathf::max(0.1, charge / 8.0);
        let k = mathf::max(1.0, charge * 5.0);
        let color = [4.0 * num * k, 4.0 * (num - charge * 0.2) * k, 4.0 * (num + charge * 0.6) * k, 4.0 * k];
        let w = crate::particles::PsWrites { speed: Some(progress * r + charge * 0.5), color: Some(color), ..Default::default() };
        self.ps_field_writes(id, "effect", w);
    }

    /// ThrusterbedBehaviour.OnDisable
    pub fn thrusterbed_on_disable(&mut self, i: usize) {
        self.thrusterbeds[i].Activated = false;
        let id = self.thrusterbeds[i].id;
        self.ps_field_call(id, "effect", false, super::PsCall::Stop(self.time.timeAsDouble));
        let (low, high) = self.thrusterbed_sources(i);
        self.audio.stop(low);
        self.audio.stop(high);
    }

    /// ThrusterbedBehaviour.FixedUpdate
    pub fn thrusterbed_fixed_update(&mut self, i: usize) {
        if !self.thrusterbeds[i].Activated {
            return;
        }
        let dt = self.time.deltaTime;
        let s = &mut self.thrusterbeds[i];
        s.time += dt;
        let t = mathf::clamp01(s.time);
        let curve = s.StartupStrengthCurve.as_ref().expect("ThrusterbedBehaviour.StartupStrengthCurve");
        let num = curve.evaluate(&mut s.cache, t);
        let (p, target, burn) = (s.Phys, s.TemperatureTarget, s.DoBurnOthers);
        self.thrusterbed_update_particle_system(i, num);
        if burn {
            self.thrusterbed_burn_others(i, num);
        }
        self.thrusterbed_thrust(i, num);
        self.thrusterbed_update_audio(i, num);
        let charge = self.thrusterbed_boost(i);
        let temperature = self.physicals[p].Temperature;
        let heat = self.heat_capacity(p);
        self.physicals[p].Temperature = mathf::lerp(temperature, target + charge, 0.005 / heat);
        let at = self.physicals[p].transform.position;
        self.camera_shake(((d(num) + d(charge) * 0.1f32) * 0.1f32).f(), [at[0], at[1]]);
    }

    /// ThrusterbedBehaviour.Thrust
    pub(crate) fn thrusterbed_thrust(&mut self, i: usize, progress: f32) {
        let s = &self.thrusterbeds[i];
        let (p, force, direction) = (s.Phys, s.ThrustingForce, s.RelativeDirection);
        let num = mathf::max(1.0, self.thrusterbed_boost(i) * 2.0);
        let scalar = (d(num) * progress * force).f();
        let v = self.transform_vector(p, direction);
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_force(&mut self.world, [scalar * v[0], scalar * v[1]]);
    }

    /// ThrusterbedBehaviour.BurnOthers
    pub(crate) fn thrusterbed_burn_others(&mut self, i: usize, progress: f32) {
        let s = &self.thrusterbeds[i];
        let (p, direction, count, width, length, offset, perpendicular, mask) =
            (s.Phys, s.RelativeDirection, s.BurnRayCount, s.BurnRayWidth, s.BurnRayLength, s.BurnRayOffset, s.BurnRayPerpendicular, s.LayersToBurn);
        let num = (d(self.thrusterbed_boost(i)) * 0.2f32 * progress).f();
        let t = self.transform_vector(p, direction);
        let vector = [t[0], t[1]];
        let vector2 = [vector[0] * num, vector[1] * num];
        let step = width / mathf::max(1.0, (count - 1) as f32);
        let vector3 = [step * perpendicular[0], step * perpendicular[1]];
        let half = width / 2.0;
        for k in 0..count {
            let k = k as f32;
            let local = [(offset[0] + vector3[0] * k) - half * perpendicular[0], (offset[1] + vector3[1] * k) - half * perpendicular[1]];
            let start = self.transform_point3(p, [local[0], local[1], 0.0]);
            let back = 0.0 - length;
            let end = [(start[0] + vector[0] * back) - vector2[0], (start[1] + vector[1] * back) - vector2[1]];
            let hits = self.linecast_hits(V::new(start[0], start[1]), V::new(end[0], end[1]), mask, THRUSTERBED_BUFFER);
            if !hits.is_empty() {
                self.thrusterbed_burn_in_buffer(i, progress, &hits);
            }
        }
    }

    /// ThrusterbedBehaviour.BurnInBuffer
    fn thrusterbed_burn_in_buffer(&mut self, i: usize, progress: f32, hits: &[super::explosion::RayHit2D]) {
        let s = &self.thrusterbeds[i];
        let (p, direction, target, id) = (s.Phys, s.RelativeDirection, s.TemperatureTarget, s.id);
        let vector = self.transform_vector(p, direction);
        let own = self.with_component_instance(id, |_, inst, o| inst.objects[o].game_object);
        for hit in hits {
            // collider.transform == base.transform
            if hit.game_object == own {
                continue;
            }
            let charge = self.thrusterbed_boost(i);
            if let Some(rb) = self.rigidbody_of_body(hit.body) {
                // 1.28.45: val * (progress * (-15f - Boost * 2f)).
                let force = if crate::assets::il::is_1_28_45() {
                    let k = (d(progress) * (d(-15.0f32) - d(charge) * 2.0f32)).f();
                    [vector[0] * k, vector[1] * k]
                } else {
                    let k = (d(-15.0f32) - d(charge) * 2.0f32).f();
                    [vector[0] * progress * k, vector[1] * progress * k]
                };
                rb.add_force(&mut self.world, force);
                let torque = self.random.range(-0.1, 0.1);
                rb.add_torque(&mut self.world, torque);
            }
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform)
            let Some(v) = self.physical_of_collider(hit.collider).filter(|&v| self.physicals[v].in_world) else { continue };
            let vid = self.physicals[v].id;
            if self.with_component_instance(vid, |_, inst, o| inst.objects[o].game_object) != hit.game_object {
                continue;
            }
            let heat = self.heat_capacity(v);
            let temperature = self.physicals[v].Temperature;
            self.physicals[v].Temperature = mathf::lerp(temperature, target + charge, 0.01 / heat);
            let r = self.random.value();
            let burn = self.physicals[v].BurnProgress;
            self.physicals[v].BurnProgress = (d(burn) + (d(0.02f32) * r + d(charge) * 0.01f32)).f();
            if self.random.value() as f64 > 0.95 {
                self.ignite(v, false);
            }
        }
    }
}
