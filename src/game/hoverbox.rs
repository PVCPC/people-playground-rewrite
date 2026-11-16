//! HoverboxBehaviour: while Activated, cancels its gravity and damps its motion by 0.9^max(1, Charge). The Effects
//! object shows while on.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation};
use super::{d, mathf, script, Game};

script! {
    HoverboxBehaviour {
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
        Effects: Option<rt::Go> = None,
        /// The Rigidbody2D, as the `Game::physicals` entry that owns it.
        Rigidbody: Option<usize> = None,
        PhysicalBehaviour: Option<usize> = None,
    }
}

impl Game {
    /// HoverboxBehaviour.Start
    pub fn hoverbox_behaviour_start(&mut self, h: usize) {
        self.hoverbox_behaviour_set_activation(h);
    }

    /// HoverboxBehaviour.Use (when enabled): toggles.
    pub fn hoverbox_behaviour_use(&mut self, h: usize, _activation: Activation) {
        if self.tr.hoverbox_behaviour[h].enabled {
            self.tr.hoverbox_behaviour[h].Activated = !self.tr.hoverbox_behaviour[h].Activated;
            self.hoverbox_behaviour_set_activation(h);
        }
    }

    /// HoverboxBehaviour.FixedUpdate: AddForce(Physics2D.gravity * (-1 * gravityScale * mass)), then velocity and
    /// angularVelocity times 0.9^max(1, Charge).
    pub fn hoverbox_behaviour_fixed_update(&mut self, h: usize) {
        if !self.tr.hoverbox_behaviour[h].Activated {
            return;
        }
        let rb = self.physicals[rt::nn(self.tr.hoverbox_behaviour[h].Rigidbody)].rigidbody.clone();
        let k = (d(-1.0f32) * rb.gravity_scale(&self.world) * rb.mass(&self.world)).f();
        let force = rt::mul([self.world.gravity.x, self.world.gravity.y], k);
        rb.add_force(&mut self.world, force);
        let damp = mathf::pow(0.9, mathf::max(1.0, self.physicals[rt::nn(self.tr.hoverbox_behaviour[h].PhysicalBehaviour)].Charge()));
        let v = rt::mul(rb.velocity(&self.world), damp);
        rb.set_velocity(&mut self.world, v);
        let w = rb.angular_velocity(&self.world) * damp;
        rb.set_angular_velocity(&mut self.world, w);
    }

    /// HoverboxBehaviour.OnEnable
    pub fn hoverbox_behaviour_on_enable(&mut self, h: usize) {
        self.hoverbox_behaviour_set_activation(h);
    }

    /// HoverboxBehaviour.OnDisable
    pub fn hoverbox_behaviour_on_disable(&mut self, h: usize) {
        self.tr.hoverbox_behaviour[h].Activated = false;
        self.hoverbox_behaviour_set_activation(h);
    }

    /// HoverboxBehaviour.SetActivation: Effects.SetActive(Activated).
    pub fn hoverbox_behaviour_set_activation(&mut self, h: usize) {
        let s = &self.tr.hoverbox_behaviour[h];
        let on = s.Activated;
        self.tr_set_active(rt::nn(s.Effects), on);
    }
}
