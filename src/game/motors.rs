//! The motor machines: MotorisedHingeBehaviour (servo), RotorBehaviour, BoatMotorBehaviour and EBikeBehaviour. Their
//! lights, sounds, indicator arms and context menu buttons are presentation; a missing AudioSource or light still
//! throws where the game would touch it.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation, Xf};
use super::{d, mathf, script, Game};

script! {
    MotorisedHingeBehaviour {
        Activated: bool = false,
        Delta: f32 = 90.0,
        Speed: f32 = 1.0,
        Force: f32 = 1.0,
        VolumeMultiplier: f32 = 0.5,
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
        IndicatorArm: Option<Xf> = None,
        AudioSource: bool = false,
        Hinge: Option<usize> = None,
        PhysicalBehaviour: Option<usize> = None,
        ActiveLight: bool = false,
    }
}

script! {
    RotorBehaviour {
        Activated: bool = false,
        Speed: f32 = 1.0,
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
        Hinge: Option<usize> = None,
        AudioSource: bool = false,
        PhysicalBehaviour: Option<usize> = None,
        ActiveLight: bool = false,
    }
}

script! {
    BoatMotorBehaviour {
        Activated: bool = false,
        Force: f32 = 0.0,
        LocalDirection: [f32; 2] = [0.0; 2],
        VisualRotationSpeed: f32 = 7000.0,
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
        MotorSound: bool = false,
        /// The propeller's CosmeticRotationBehaviour.
        CosmeticRotation: Option<usize> = None,
        phys: Option<usize> = None,
    }
}

script! {
    EBikeBehaviour {
        Activated: bool = false,
        Speed: f32 = 100.0,
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
        Hinge: Option<usize> = None,
    }
}

/// Mathf.Sign
fn sign(f: f32) -> f32 {
    if f >= 0.0 { 1.0 } else { -1.0 }
}

impl Game {
    // ---- MotorisedHingeBehaviour ----

    /// MotorisedHingeBehaviour.Start: SetDelta(Delta), the light, then the context menu buttons on PhysicalBehaviour (UI).
    pub fn motorised_hinge_behaviour_start(&mut self, m: usize) {
        self.motorised_hinge_behaviour_set_delta(m, self.tr.motorised_hinge_behaviour[m].Delta);
        rt::pres(self.tr.motorised_hinge_behaviour[m].ActiveLight);
        let s = &self.tr.motorised_hinge_behaviour[m];
        let (on, id) = (s.Activated, s.this_id);
        self.present_sprite_enabled(id, "ActiveLight", on);
        rt::nn(self.tr.motorised_hinge_behaviour[m].PhysicalBehaviour);
    }

    /// MotorisedHingeBehaviour.ToggleActivation
    pub fn motorised_hinge_behaviour_toggle_activation(&mut self, m: usize) {
        let s = &mut self.tr.motorised_hinge_behaviour[m];
        s.Activated = !s.Activated;
        // ActiveLight.enabled = Activated
        rt::pres(s.ActiveLight);
        let (on, id) = (s.Activated, s.this_id);
        self.present_sprite_enabled(id, "ActiveLight", on);
    }

    /// MotorisedHingeBehaviour.Use
    pub fn motorised_hinge_behaviour_use(&mut self, m: usize, _activation: Activation) {
        self.motorised_hinge_behaviour_toggle_activation(m);
    }

    /// MotorisedHingeBehaviour.FixedUpdate: the motor drives the joint angle toward ±Delta (0 when off); the servo sound
    /// follows the joint speed.
    pub fn motorised_hinge_behaviour_fixed_update(&mut self, m: usize) {
        let h = self.tr_hinge(self.tr.motorised_hinge_behaviour[m].Hinge);
        let angle = self.hinges[h].joint_angle(&self.world);
        let target = if self.tr.motorised_hinge_behaviour[m].Activated { self.motorised_hinge_behaviour_get_scaled_delta(m) } else { 0.0 };
        let s = &self.tr.motorised_hinge_behaviour[m];
        let speed = mathf::delta_angle(angle, target) * s.Speed;
        let force = (d(s.Force) * (d(1.0f32) + self.physicals[rt::nn(s.PhysicalBehaviour)].Charge())).f();
        self.hinges[h].set_motor(&mut self.world, speed, force);
        // AudioSource.volume = Mathf.Clamp01(num * num * VolumeMultiplier) and Play() when num > 0.5, else Stop().
        let num = self.hinges[h].joint_speed(&self.world).abs() * 0.1;
        rt::pres(self.tr.motorised_hinge_behaviour[m].AudioSource);
        let s = &self.tr.motorised_hinge_behaviour[m];
        let (id, mul) = (s.this_id, s.VolumeMultiplier);
        if let Some(src) = self.present_audio(id, "AudioSource") {
            if num > 0.5 {
                if !self.audio.is_playing(src) {
                    self.audio.play(src);
                }
                self.audio.get_mut(src).volume = mathf::clamp01(num * num * mul);
            } else if self.audio.is_playing(src) {
                self.audio.stop(src);
            }
        }
    }

    /// MotorisedHingeBehaviour.GetScaledDelta: Delta, mirrored with the object.
    pub fn motorised_hinge_behaviour_get_scaled_delta(&mut self, m: usize) -> f32 {
        let s = &self.tr.motorised_hinge_behaviour[m];
        let flip = if self.tr_local_scale(Xf::this(s.this_physical, s.this_physics))[0] > 0.0 { 1.0 } else { -1.0 };
        self.tr.motorised_hinge_behaviour[m].Delta * flip
    }

    /// MotorisedHingeBehaviour.OnDisable
    pub fn motorised_hinge_behaviour_on_disable(&mut self, m: usize) {
        let h = self.tr_hinge(self.tr.motorised_hinge_behaviour[m].Hinge);
        self.hinges[h].set_use_motor(&mut self.world, false);
        // AudioSource.Stop()
        rt::pres(self.tr.motorised_hinge_behaviour[m].AudioSource);
        self.present_audio_play(self.tr.motorised_hinge_behaviour[m].this_id, "AudioSource", false);
    }

    /// MotorisedHingeBehaviour.OnEnable
    pub fn motorised_hinge_behaviour_on_enable(&mut self, m: usize) {
        let h = self.tr_hinge(self.tr.motorised_hinge_behaviour[m].Hinge);
        self.hinges[h].set_use_motor(&mut self.world, true);
    }

    /// MotorisedHingeBehaviour.SetDelta: IndicatorArm.localEulerAngles = (0, 0, Delta) (ObjectState saves the arm's pose).
    pub fn motorised_hinge_behaviour_set_delta(&mut self, m: usize, v: f32) {
        self.tr.motorised_hinge_behaviour[m].Delta = v;
        if let Some(arm) = self.tr.motorised_hinge_behaviour[m].IndicatorArm {
            rt::presentation_pose(arm);
            if let Xf::Object(t) = arm {
                let q = super::menu::euler_z_rotation(v);
                self.with_component_instance(t, |_, inst, o| inst.set_local_rotation(o, q));
            }
        }
    }

    // ---- RotorBehaviour ----

    /// RotorBehaviour.Start: SetSpeed(Speed), the light, then the context menu buttons on PhysicalBehaviour (UI).
    pub fn rotor_behaviour_start(&mut self, r: usize) {
        self.rotor_behaviour_set_speed(r, self.tr.rotor_behaviour[r].Speed);
        rt::pres(self.tr.rotor_behaviour[r].ActiveLight);
        let s = &self.tr.rotor_behaviour[r];
        let (on, id) = (s.Activated, s.this_id);
        self.present_sprite_enabled(id, "ActiveLight", on);
        rt::nn(self.tr.rotor_behaviour[r].PhysicalBehaviour);
    }

    /// RotorBehaviour.SetSpeed
    pub fn rotor_behaviour_set_speed(&mut self, r: usize, v: f32) {
        self.tr.rotor_behaviour[r].Speed = mathf::clamp(v, -8000.0, 8000.0);
    }

    /// RotorBehaviour.OnDisable
    pub fn rotor_behaviour_on_disable(&mut self, r: usize) {
        let h = self.tr_hinge(self.tr.rotor_behaviour[r].Hinge);
        self.hinges[h].set_use_motor(&mut self.world, false);
    }

    /// RotorBehaviour.OnEnable
    pub fn rotor_behaviour_on_enable(&mut self, r: usize) {
        let h = self.tr_hinge(self.tr.rotor_behaviour[r].Hinge);
        self.hinges[h].set_use_motor(&mut self.world, true);
    }

    /// RotorBehaviour.Use: channel 1 reverses, any other toggles.
    pub fn rotor_behaviour_use(&mut self, r: usize, activation: Activation) {
        if activation.Channel == 1 {
            self.rotor_behaviour_set_speed(r, 0.0 - self.tr.rotor_behaviour[r].Speed);
            return;
        }
        let s = &mut self.tr.rotor_behaviour[r];
        s.Activated = !s.Activated;
        rt::pres(s.ActiveLight);
        let (on, id) = (s.Activated, s.this_id);
        self.present_sprite_enabled(id, "ActiveLight", on);
    }

    /// RotorBehaviour.GetScaledSpeed: Speed, mirrored with the object.
    pub fn rotor_behaviour_get_scaled_speed(&mut self, r: usize) -> f32 {
        let s = &self.tr.rotor_behaviour[r];
        let flip = if !(self.tr_local_scale(Xf::this(s.this_physical, s.this_physics))[0] < 0.0) { 1.0 } else { -1.0 };
        self.tr.rotor_behaviour[r].Speed * flip
    }

    /// RotorBehaviour.FixedUpdate: motor speed -ScaledSpeed while on, torque 100 + Charge * 4.
    pub fn rotor_behaviour_fixed_update(&mut self, r: usize) {
        let h = self.tr_hinge(self.tr.rotor_behaviour[r].Hinge);
        let torque = (d(100.0f32) + d(self.physicals[rt::nn(self.tr.rotor_behaviour[r].PhysicalBehaviour)].Charge()) * 4.0f32).f();
        let speed = if self.tr.rotor_behaviour[r].Activated { 0.0 - self.rotor_behaviour_get_scaled_speed(r) } else { 0.0 };
        self.hinges[h].set_motor(&mut self.world, speed, torque);
    }

    // ---- BoatMotorBehaviour ----

    /// BoatMotorBehaviour.Awake
    pub fn boat_motor_behaviour_awake(&mut self, b: usize) {
        self.tr.boat_motor_behaviour[b].phys = self.tr.boat_motor_behaviour[b].this_physical;
    }

    /// BoatMotorBehaviour.Start: MotorSound plays or stops by Activated, then the context menu button on phys (UI).
    pub fn boat_motor_behaviour_start(&mut self, b: usize) {
        rt::pres(self.tr.boat_motor_behaviour[b].MotorSound);
        let s = &self.tr.boat_motor_behaviour[b];
        let (on, id) = (s.Activated, s.this_id);
        self.present_audio_play(id, "MotorSound", on);
        rt::nn(self.tr.boat_motor_behaviour[b].phys);
    }

    /// BoatMotorBehaviour.Update: the propeller spins while on.
    pub fn boat_motor_behaviour_update(&mut self, b: usize) {
        let s = &self.tr.boat_motor_behaviour[b];
        let (on, speed) = (s.Activated, s.VisualRotationSpeed);
        let prop = &mut self.tr.cosmetic_rotation_behaviour[rt::nn(s.CosmeticRotation)];
        prop.ShouldRotate = on;
        prop.RotationSpeedTarget = speed;
    }

    /// BoatMotorBehaviour.OnDisable
    pub fn boat_motor_behaviour_on_disable(&mut self, b: usize) {
        let s = &self.tr.boat_motor_behaviour[b];
        self.tr.cosmetic_rotation_behaviour[rt::nn(s.CosmeticRotation)].ShouldRotate = false;
        // MotorSound.Stop()
        rt::pres(self.tr.boat_motor_behaviour[b].MotorSound);
        self.present_audio_play(self.tr.boat_motor_behaviour[b].this_id, "MotorSound", false);
    }

    /// BoatMotorBehaviour.Reverse
    pub fn boat_motor_behaviour_reverse(&mut self, b: usize) {
        let s = &mut self.tr.boat_motor_behaviour[b];
        s.VisualRotationSpeed *= -1.0;
        s.Force *= -1.0;
    }

    /// BoatMotorBehaviour.OnEnable
    pub fn boat_motor_behaviour_on_enable(&mut self, b: usize) {
        if self.tr.boat_motor_behaviour[b].Activated {
            // MotorSound.Play()
            rt::pres(self.tr.boat_motor_behaviour[b].MotorSound);
            self.present_audio_play(self.tr.boat_motor_behaviour[b].this_id, "MotorSound", true);
        }
    }

    /// BoatMotorBehaviour.FixedUpdate: under water and on, a relative force along LocalDirection; the propeller damps
    /// less in water.
    pub fn boat_motor_behaviour_fixed_update(&mut self, b: usize) {
        let p = rt::nn(self.tr.boat_motor_behaviour[b].phys);
        let s = &self.tr.boat_motor_behaviour[b];
        if !self.physicals[p].IsUnderWater {
            self.tr.cosmetic_rotation_behaviour[rt::nn(s.CosmeticRotation)].Dampening = 0.6;
            return;
        }
        if s.Activated {
            let mut force = rt::mul(s.LocalDirection, s.Force.abs() + self.physicals[p].Charge());
            let flip = sign(self.tr_local_scale(Xf::this(s.this_physical, s.this_physics))[0]) * sign(s.Force);
            force[0] *= flip;
            self.physicals[p].rigidbody.add_relative_force(&mut self.world, force);
        }
        let prop = rt::nn(self.tr.boat_motor_behaviour[b].CosmeticRotation);
        self.tr.cosmetic_rotation_behaviour[prop].Dampening = 0.3;
    }

    /// BoatMotorBehaviour.Use: channel 1 reverses, any other toggles (when enabled).
    pub fn boat_motor_behaviour_use(&mut self, b: usize, activation: Activation) {
        if !self.tr.boat_motor_behaviour[b].enabled {
            return;
        }
        if activation.Channel == 1 {
            self.boat_motor_behaviour_reverse(b);
            return;
        }
        let s = &mut self.tr.boat_motor_behaviour[b];
        s.Activated = !s.Activated;
        // MotorSound.Play() or Stop()
        rt::pres(s.MotorSound);
        let (on, id) = (s.Activated, s.this_id);
        self.present_audio_play(id, "MotorSound", on);
    }

    // ---- EBikeBehaviour ----

    /// EBikeBehaviour.Start: the wheel motor's speed, reversed when the bike is not mirrored.
    pub fn e_bike_behaviour_start(&mut self, e: usize) {
        let s = &self.tr.e_bike_behaviour[e];
        let mut speed = s.Speed;
        if self.tr_lossy_scale(Xf::this(s.this_physical, s.this_physics))[0] > 0.0 {
            speed *= -1.0;
        }
        let h = self.tr_hinge(self.tr.e_bike_behaviour[e].Hinge);
        let torque = self.hinges[h].motor[1];
        self.hinges[h].set_motor(&mut self.world, speed, torque);
        self.e_bike_behaviour_set_activation(e);
    }

    /// EBikeBehaviour.Use
    pub fn e_bike_behaviour_use(&mut self, e: usize, _activation: Activation) {
        self.tr.e_bike_behaviour[e].Activated = !self.tr.e_bike_behaviour[e].Activated;
        self.e_bike_behaviour_set_activation(e);
    }

    /// EBikeBehaviour.SetActivation: `if (Hinge)` the motor follows Activated.
    pub fn e_bike_behaviour_set_activation(&mut self, e: usize) {
        if let Some(h) = self.tr.e_bike_behaviour[e].Hinge.filter(|&h| !self.hinges[h].destroyed) {
            self.hinges[h].set_use_motor(&mut self.world, self.tr.e_bike_behaviour[e].Activated);
        }
    }
}
