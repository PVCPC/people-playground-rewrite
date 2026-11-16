//! GyrostabiliserBehaviour (damps spin and torques the object upright while on) and
//! IndustrialGyrostabiliserBehaviour (freezes rotation while on). Both show a Light object while Activated.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation, Xf};
use super::{d, mathf, script, Game};

script! {
    GyrostabiliserBehaviour {
        Intensity: f32 = 1.0,
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
        physicalBehaviour: Option<usize> = None,
        Light: Option<rt::Go> = None,
    }
}

script! {
    IndustrialGyrostabiliserBehaviour {
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
        physicalBehaviour: Option<usize> = None,
        Light: Option<rt::Go> = None,
    }
}

/// RigidbodyConstraints2D.FreezeRotation
const FREEZE_ROTATION: i32 = 4;

impl Game {
    // ---- GyrostabiliserBehaviour ----

    /// GyrostabiliserBehaviour.Awake
    pub fn gyrostabiliser_behaviour_awake(&mut self, g: usize) {
        self.tr.gyrostabiliser_behaviour[g].physicalBehaviour = self.tr.gyrostabiliser_behaviour[g].this_physical;
    }

    /// GyrostabiliserBehaviour.Start: Light.SetActive(Activated).
    pub fn gyrostabiliser_behaviour_start(&mut self, g: usize) {
        let s = &self.tr.gyrostabiliser_behaviour[g];
        let on = s.Activated;
        self.tr_set_active(rt::nn(s.Light), on);
    }

    /// GyrostabiliserBehaviour.FixedUpdate: on a dynamic body, the angular velocity is scaled by 0.3 / Intensity /
    /// max(1, Charge) while on (1 when off); on, it then decays by 0.5^k and a torque of DeltaAngle(z, 0) * k turns it
    /// upright, k = Intensity * clamp(Charge, 1, 5) * (mass / 1.5)².
    pub fn gyrostabiliser_behaviour_fixed_update(&mut self, g: usize) {
        let p = rt::nn(self.tr.gyrostabiliser_behaviour[g].physicalBehaviour);
        if self.physicals[p].rigidbody.bodyType != 0 {
            return;
        }
        let (on, intensity) = (self.tr.gyrostabiliser_behaviour[g].Activated, self.tr.gyrostabiliser_behaviour[g].Intensity);
        let scale = if on { (0.3f32 / d(intensity) / mathf::max(1.0, self.physicals[p].Charge())).f() } else { 1.0 };
        let rb = self.physicals[p].rigidbody.clone();
        let w = rb.angular_velocity(&self.world) * scale;
        rb.set_angular_velocity(&mut self.world, w);
        if !on {
            return;
        }
        let m = rb.mass(&self.world) / 1.5;
        let k = (d(intensity) * mathf::clamp(self.physicals[p].Charge(), 1.0, 5.0) * m * m).f();
        let w = rb.angular_velocity(&self.world) * mathf::pow(0.5, k);
        rb.set_angular_velocity(&mut self.world, w);
        let s = &self.tr.gyrostabiliser_behaviour[g];
        let z = self.tr_euler_angles(Xf::this(s.this_physical, s.this_physics))[2];
        rb.add_torque(&mut self.world, mathf::delta_angle(z, 0.0) * k);
    }

    /// GyrostabiliserBehaviour.Use (when enabled): toggles, Light.SetActive(Activated).
    pub fn gyrostabiliser_behaviour_use(&mut self, g: usize, _activation: Activation) {
        if self.tr.gyrostabiliser_behaviour[g].enabled {
            let s = &mut self.tr.gyrostabiliser_behaviour[g];
            s.Activated = !s.Activated;
            let (on, light) = (s.Activated, s.Light);
            self.tr_set_active(rt::nn(light), on);
        }
    }

    /// GyrostabiliserBehaviour.OnDisable
    pub fn gyrostabiliser_behaviour_on_disable(&mut self, g: usize) {
        self.tr_set_active(rt::nn(self.tr.gyrostabiliser_behaviour[g].Light), false);
    }

    // ---- IndustrialGyrostabiliserBehaviour ----

    /// IndustrialGyrostabiliserBehaviour.Awake
    pub fn industrial_gyrostabiliser_behaviour_awake(&mut self, g: usize) {
        self.tr.industrial_gyrostabiliser_behaviour[g].physicalBehaviour = self.tr.industrial_gyrostabiliser_behaviour[g].this_physical;
    }

    /// IndustrialGyrostabiliserBehaviour.Start
    pub fn industrial_gyrostabiliser_behaviour_start(&mut self, g: usize) {
        self.industrial_gyrostabiliser_behaviour_set_activated(g, self.tr.industrial_gyrostabiliser_behaviour[g].Activated);
    }

    /// IndustrialGyrostabiliserBehaviour.FixedUpdate: only reads rigidbody.bodyType (a destroyed one throws).
    pub fn industrial_gyrostabiliser_behaviour_fixed_update(&mut self, g: usize) {
        rt::nn(self.tr.industrial_gyrostabiliser_behaviour[g].physicalBehaviour);
    }

    /// IndustrialGyrostabiliserBehaviour.Use
    pub fn industrial_gyrostabiliser_behaviour_use(&mut self, g: usize, _activation: Activation) {
        if self.tr.industrial_gyrostabiliser_behaviour[g].enabled {
            self.industrial_gyrostabiliser_behaviour_set_activated(g, !self.tr.industrial_gyrostabiliser_behaviour[g].Activated);
        }
    }

    /// IndustrialGyrostabiliserBehaviour.SetActivated: the Light, and rotation frozen while on.
    pub fn industrial_gyrostabiliser_behaviour_set_activated(&mut self, g: usize, activated: bool) {
        self.tr.industrial_gyrostabiliser_behaviour[g].Activated = activated;
        self.tr_set_active(rt::nn(self.tr.industrial_gyrostabiliser_behaviour[g].Light), activated);
        let p = rt::nn(self.tr.industrial_gyrostabiliser_behaviour[g].physicalBehaviour);
        self.physicals[p].rigidbody.set_constraints(&mut self.world, if activated { FREEZE_ROTATION } else { 0 });
    }

    /// IndustrialGyrostabiliserBehaviour.OnDisable
    pub fn industrial_gyrostabiliser_behaviour_on_disable(&mut self, g: usize) {
        self.industrial_gyrostabiliser_behaviour_set_activated(g, false);
    }
}
