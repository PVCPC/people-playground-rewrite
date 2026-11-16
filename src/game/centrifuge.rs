//! CentrifugeBehaviour and VibrationBehaviour: while on it spins up, shakes its body and removes the smallest of the liquids it
//! holds, until one is left, and lights its lights ; the Roer's spin is not drawn (link.rs).
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::{d, mathf, script, Game};

script! {
    Vibration {
        VibrateForce: f32 = 0.0,
        VibrateRotationSpeed: f32 = 0.0,
    }
    links {
        /// The PhysicalBehaviour on its object (rb).
        physical: usize = 0,
    }
}

script! {
    Centrifuge {
        Activated: bool = false,
        VibrationIntensity: f32 = 2.0,
        ParticleForceIntensity: f32 = 1.0,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// The PhysicalBehaviour on its object.
        physical: usize = 0,
        /// LiquidContainer, the LiquidContainerController whose fill Update sets.
        LiquidContainer: Option<usize> = None,
        /// VibrationBehaviour (a `vibrations` index).
        VibrationBehaviour: usize = 0,
        /// DamagableMachinery (a `machinery` index).
        DamagableMachinery: usize = 0,
        currentIntensity: f32 = 0.0,
        EnabledLight: Option<super::visual::SpriteAt> = None,
        /// LiquidCountLights (a null entry is None).
        LiquidCountLights: Vec<Option<super::visual::SpriteAt>> = Vec::new(),
    }
}

impl Game {
    /// CentrifugeBehaviour.Start
    pub fn centrifuge_start(&mut self, c: usize) {
        self.centrifuge_update_activation(c);
    }

    /// CentrifugeBehaviour.UpdateActivation: EnabledLight.enabled = Activated, then the loop AudioSource plays or stops.
    pub(crate) fn centrifuge_update_activation(&mut self, c: usize) {
        let s = &self.centrifuges[c];
        let (at, on, id) = (s.EnabledLight.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), s.Activated, s.id);
        self.set_sprite_enabled(at, on);
        let src = self.audio.source_field(id, "CentrifugeLoopAudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        if on { self.audio.play(src) } else { self.audio.stop(src) }
    }

    /// CentrifugeBehaviour.OnWillRenderObject (its object's sprite drawn): one light per liquid held.
    pub fn centrifuges_will_render(&mut self) {
        for c in 0..self.centrifuges.len() {
            let p = self.centrifuges[c].physical;
            if self.will_render_dispatch(self.centrifuges[c].id, self.sprite_drawn(super::visual::SpriteAt::Physical(p))) {
                self.centrifuge_will_render(c);
            }
        }
    }

    /// CentrifugeBehaviour.OnWillRenderObject: LiquidCountLights[i].enabled = i < LiquidDistribution.Count.
    pub fn centrifuge_will_render(&mut self, c: usize) {
        let n = self.bc(Owner::Centrifuge(c)).LiquidDistribution.len();
        for (i, at) in self.centrifuges[c].LiquidCountLights.clone().into_iter().enumerate() {
            self.set_sprite_enabled(at.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), i < n);
        }
    }

    /// VibrationBehaviour.ForceVector
    pub(crate) fn vibration_force_vector(&self, v: usize) -> [f32; 2] {
        let s = &self.vibrations[v];
        let f = (d(self.time.time) * s.VibrateRotationSpeed).f();
        [(f as f64).cos() as f32 * s.VibrateForce, (f as f64).sin() as f32 * s.VibrateForce]
    }

    /// VibrationBehaviour.FixedUpdate (VibrationBehaviour.Awake is the linked Rigidbody2D)
    pub fn vibration_fixed_update(&mut self, v: usize) {
        if self.vibrations[v].VibrateForce.abs() >= mathf::EPSILON {
            let force = self.vibration_force_vector(v);
            let rb = self.physicals[self.vibrations[v].physical].rigidbody.clone();
            rb.add_relative_force(&mut self.world, force);
        }
    }

    /// LiquidContainerController.AddForce(globalForce, randomisation)
    pub(crate) fn liquid_container_add_force(&mut self, c: usize, force: [f32; 2], randomisation: f32) {
        if !self.liquid_containers[c].fancy {
            return;
        }
        let (anchor, chain) = (self.liquid_containers[c].anchor, self.liquid_containers[c].chain.clone());
        let v = self.anchored_inverse_vector(anchor, &chain, [force[0], force[1], 0.0]);
        for i in 0..16 {
            let k = (d(1.0) - d(self.random.value()) * randomisation).f();
            let p = &mut self.liquid_containers[c].particles[i];
            p.Velocity[0] += v[0] * k;
            p.Velocity[1] += v[1] * k;
        }
    }

    /// CentrifugeBehaviour.FixedUpdate
    pub fn centrifuge_fixed_update(&mut self, c: usize) {
        let o = Owner::Centrifuge(c);
        let s = &self.centrifuges[c];
        let (v, dt) = (s.VibrationBehaviour, self.time.fixedDeltaTime);
        self.vibrations[v].VibrateForce = (d(s.VibrationIntensity) * s.currentIntensity).f();
        if s.Activated && !self.machinery[s.DamagableMachinery].Destroyed {
            self.centrifuges[c].currentIntensity += dt;
            if let Some(container) = self.centrifuges[c].LiquidContainer {
                let f = self.vibration_force_vector(v);
                let k = self.centrifuges[c].ParticleForceIntensity;
                self.liquid_container_add_force(container, [f[0] * k, f[1] * k], 1.0);
            }
            let keys = self.bc(o).LiquidDistribution.keys();
            if keys.len() > 1 {
                let (mut liquid, mut num) = (None, f32::MAX);
                for key in keys {
                    let raw = self.bc(o).LiquidDistribution.get(key).unwrap().raw;
                    if raw < num && num > 0.0 {
                        num = raw;
                        liquid = Some(key);
                    }
                }
                if let Some(liquid) = liquid.filter(|_| num > 0.0) {
                    self.remove_liquid(o, liquid, dt);
                }
            }
        } else {
            self.centrifuges[c].currentIntensity -= dt;
        }
        self.centrifuges[c].currentIntensity = mathf::clamp(self.centrifuges[c].currentIntensity, 0.0, 1.0);
    }

    /// CentrifugeBehaviour.Update
    pub fn centrifuge_update(&mut self, c: usize) {
        let o = Owner::Centrifuge(c);
        self.blood_container_update(o);
        if let Some(container) = self.centrifuges[c].LiquidContainer {
            self.liquid_containers[container].FillPercentage = mathf::clamp(self.scaled_liquid_amount(o), 0.0, 1.0);
            self.liquid_container_color_from(container, o);
        }
    }

    /// CentrifugeBehaviour.Use
    pub fn centrifuge_use(&mut self, c: usize) {
        if self.centrifuges[c].enabled {
            self.centrifuges[c].Activated = !self.centrifuges[c].Activated;
            self.centrifuge_update_activation(c);
        }
    }
}
