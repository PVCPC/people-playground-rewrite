//! MagnetBehaviour: while Activated, pulls conducting objects (pushes when Reversed), its particle system and sound on.
#![allow(non_snake_case)]

use super::translated::rt;
use super::{d, mathf, script, Game};

script! {
    MagnetBehaviour {
        BaseRange: f32 = 15.0,
        BaseIntensity: f32 = 5.0,
        ChargeMultiplierMultiplier: f32 = 1.0,
        JointCreationDistanceThreshold: f32 = 0.10000000149011612,
        Reversed: bool = false,
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        particleSystem: bool = false,
        audioSource: bool = false,
        PhysicalBehaviour: Option<usize> = None,
    }
}

impl Game {
    /// MagnetBehaviour.Use
    pub fn magnet_behaviour_use(&mut self, m: usize, activation: rt::Activation) {
        if self.tr.magnet_behaviour[m].enabled {
            if activation.Channel == 1 {
                self.tr.magnet_behaviour[m].Reversed = !self.tr.magnet_behaviour[m].Reversed;
            } else {
                self.tr.magnet_behaviour[m].Activated = !self.tr.magnet_behaviour[m].Activated;
            }
            self.magnet_behaviour_update_activated(m);
        }
    }

    /// MagnetBehaviour.OnDisable
    pub fn magnet_behaviour_on_disable(&mut self, m: usize) {
        self.tr.magnet_behaviour[m].Activated = false;
        self.magnet_ps(m, false);
        self.magnet_audio(m, false);
    }

    /// MagnetBehaviour.UpdateActivated
    pub fn magnet_behaviour_update_activated(&mut self, m: usize) {
        if self.tr.magnet_behaviour[m].Activated {
            self.magnet_audio(m, true);
            self.magnet_ps(m, true);
        } else {
            self.magnet_ps(m, false);
            self.magnet_audio(m, false);
        }
    }

    /// particleSystem.Play() / Stop() (gapC r8: was presentation only).
    fn magnet_ps(&mut self, m: usize, play: bool) {
        rt::pres(self.tr.magnet_behaviour[m].particleSystem);
        let id = self.tr.magnet_behaviour[m].this_id;
        let call = if play { super::PsCall::Play } else { super::PsCall::Stop(self.time.timeAsDouble) };
        self.ps_call(|e| e.owner == id && matches!(e.drive, super::EmbeddedDrive::Magnet), call);
    }

    /// audioSource.Play() / Stop().
    fn magnet_audio(&mut self, m: usize, play: bool) {
        rt::pres(self.tr.magnet_behaviour[m].audioSource);
        let src = self.audio_src(self.tr.magnet_behaviour[m].this_id, "audioSource");
        if play { self.audio.play(src) } else { self.audio.stop(src) }
    }

    /// MagnetBehaviour.Awake
    pub fn magnet_behaviour_awake(&mut self, m: usize) {
        self.tr.magnet_behaviour[m].PhysicalBehaviour = self.tr.magnet_behaviour[m].this_physical;
    }

    /// MagnetBehaviour.Start
    pub fn magnet_behaviour_start(&mut self, m: usize) {
        rt::nn(self.tr.magnet_behaviour[m].PhysicalBehaviour);
        self.magnet_behaviour_update_activated(m);
    }

    /// MagnetBehaviour.FixedUpdate: every conducting object in the world within range is pulled (or pushed) by
    /// MagneticAttractionIntensity * mass / sqrMagnitude, the magnet the opposite way.
    pub fn magnet_behaviour_fixed_update(&mut self, m: usize) {
        let s = &self.tr.magnet_behaviour[m];
        if !s.Activated {
            return;
        }
        let own = rt::nn(s.PhysicalBehaviour);
        let (base_intensity, base_range, multiplier, reversed) = (s.BaseIntensity, s.BaseRange, s.ChargeMultiplierMultiplier, s.Reversed);
        let k = mathf::max(1.0, (d(self.physicals[own].Charge()) * multiplier).f());
        let num = (d(base_intensity) * k).f();
        let num2 = (d(base_range) * k).f();
        let sq_range = (d(num2) * num2).f();
        for q in 0..self.physicals.len() {
            let ph = &self.physicals[q];
            if !ph.in_world || ph.destroyed || q == own {
                continue;
            }
            let properties = ph.Properties;
            if !self.properties[properties].Conducting {
                continue;
            }
            let (a, b) = (ph.transform.position, self.physicals[own].transform.position);
            let mut vector = [a[0] - b[0], a[1] - b[1]];
            if reversed {
                vector = [vector[0] * -1.0, vector[1] * -1.0];
            }
            let sqr = mathf::sqr_magnitude(vector);
            if sqr > sq_range || sqr <= 0.05 {
                continue;
            }
            let n = mathf::normalized2(vector);
            let rb = self.physicals[q].rigidbody.clone();
            let mass = rb.mass(&self.world);
            let attraction = self.properties[properties].MagneticAttractionIntensity;
            // Vector2's operators each return floats.
            let f = |x: f32| x * (0.0 - num) / sqr * attraction * mass;
            let force = [f(n[0]), f(n[1])];
            rb.add_force(&mut self.world, force);
            self.physicals[own].rigidbody.clone().add_force(&mut self.world, [0.0 - force[0], 0.0 - force[1]]);
        }
    }
}
