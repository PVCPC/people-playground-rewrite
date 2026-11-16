//! IonThrusterBehaviour: a toggled force along LocalDirection, Thrust + Charge * 4, mirrored with the object. The FX
//! particle system and the thruster sound follow Activated.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation, Xf};
use super::{d, script, EmbeddedDrive, EmbeddedPs, Game, PsCall};

script! {
    IonThrusterBehaviour {
        LocalDirection: [f32; 2] = [0.0; 2],
        Thrust: f32 = 10.0,
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
        ActivationSound: bool = false,
        ThrusterAudioSource: bool = false,
        FX: bool = false,
        PhysicalBehaviour: Option<usize> = None,
        vfxMain: bool = false,
        vfxEmission: bool = false,
    }
}

impl Game {
    /// IonThrusterBehaviour.Awake: caches the particle modules (presentation).
    pub fn ion_thruster_behaviour_awake(&mut self, _t: usize) {}

    fn ion_thruster_xf(&self, t: usize) -> Xf {
        let s = &self.tr.ion_thruster_behaviour[t];
        Xf::this(s.this_physical, s.this_physics)
    }

    /// IonThrusterBehaviour.Use (when enabled): toggles; switching on shakes the camera and plays ActivationSound.
    pub fn ion_thruster_behaviour_use(&mut self, t: usize, _activation: Activation) {
        if !self.tr.ion_thruster_behaviour[t].enabled {
            return;
        }
        self.tr.ion_thruster_behaviour[t].Activated = !self.tr.ion_thruster_behaviour[t].Activated;
        self.ion_thruster_behaviour_update_effect_state(t);
        if self.tr.ion_thruster_behaviour[t].Activated {
            let at = rt::v2(self.tr_position(self.ion_thruster_xf(t)));
            self.camera_shake_at(2.0, at, 1.0);
            let id = self.tr.ion_thruster_behaviour[t].this_id;
            self.audio_one_shot(id, "ThrusterAudioSource", "ActivationSound");
        }
    }

    /// IonThrusterBehaviour.Start
    pub fn ion_thruster_behaviour_start(&mut self, t: usize) {
        self.ion_thruster_behaviour_update_effect_state(t);
    }

    /// IonThrusterBehaviour.OnEnable
    pub fn ion_thruster_behaviour_on_enable(&mut self, t: usize) {
        self.ion_thruster_behaviour_update_effect_state(t);
    }

    /// IonThrusterBehaviour.OnDisable: FX.Stop() and ThrusterAudioSource.Stop().
    pub fn ion_thruster_behaviour_on_disable(&mut self, t: usize) {
        let id = self.tr.ion_thruster_behaviour[t].this_id;
        self.ps_call(|e| e.owner == id && matches!(e.drive, EmbeddedDrive::IonThruster), PsCall::Stop(self.time.timeAsDouble));
        let src = self.audio_src(id, "ThrusterAudioSource");
        self.audio.stop(src);
    }

    /// IonThrusterBehaviour.FixedUpdate
    pub fn ion_thruster_behaviour_fixed_update(&mut self, t: usize) {
        self.ion_thruster_behaviour_update_effect_state(t);
        if self.tr.ion_thruster_behaviour[t].Activated {
            let force = self.ion_thruster_behaviour_get_global_thrust_vector(t);
            let p = rt::nn(self.tr.ion_thruster_behaviour[t].PhysicalBehaviour);
            self.physicals[p].rigidbody.add_force(&mut self.world, force);
        }
    }

    /// IonThrusterBehaviour.GetGlobalThrustVector: TransformDirection(LocalDirection) * ((Thrust + Charge * 4) * sign),
    /// the sign -1 when lossyScale.x > 0.
    pub fn ion_thruster_behaviour_get_global_thrust_vector(&mut self, t: usize) -> [f32; 2] {
        let xf = self.ion_thruster_xf(t);
        let sign = if !(self.tr_lossy_scale(xf)[0] > 0.0) { 1.0f32 } else { -1.0 };
        let dir = self.tr_transform_direction(xf, rt::v3(self.tr.ion_thruster_behaviour[t].LocalDirection));
        let charge = self.physicals[rt::nn(self.tr.ion_thruster_behaviour[t].PhysicalBehaviour)].Charge();
        let thrust = ((d(self.tr.ion_thruster_behaviour[t].Thrust) + d(charge) * 4.0f32) * sign).f();
        rt::v2(rt::mul(dir, thrust))
    }

    /// IonThrusterBehaviour.UpdateEffectState: FX plays while Activated (Play unless isEmitting) with
    /// startSpeedMultiplier 5 + Charge * 0.1 and rateOverTimeMultiplier 15 + that, and stops (if isEmitting) otherwise;
    /// the audio source likewise.
    pub fn ion_thruster_behaviour_update_effect_state(&mut self, t: usize) {
        let id = self.tr.ion_thruster_behaviour[t].this_id;
        let fx = |e: &EmbeddedPs| e.owner == id && matches!(e.drive, EmbeddedDrive::IonThruster);
        let src = self.audio_src(id, "ThrusterAudioSource");
        let emitting = crate::particles::is_emitting(self, id);
        if self.tr.ion_thruster_behaviour[t].Activated {
            if !emitting {
                self.ps_call(fx, PsCall::Play);
            }
            if !self.audio.is_playing(src) {
                self.audio.play(src);
            }
            let p = rt::nn(self.tr.ion_thruster_behaviour[t].PhysicalBehaviour);
            let num = (d(self.physicals[p].Charge()) * 0.1f32).f();
            self.ps_call(fx, PsCall::Multipliers((d(15.0f32) + num).f(), (d(5.0f32) + num).f()));
        } else {
            if emitting {
                self.ps_call(fx, PsCall::Stop(self.time.timeAsDouble));
            }
            if self.audio.is_playing(src) {
                self.audio.stop(src);
            }
        }
    }
}
