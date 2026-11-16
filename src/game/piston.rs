//! PistonBehaviour. Audio is presentation; the activation, the slider's limits and motor, and the head's depth stay.
//! OnFlip is SelectionController's flip broadcast (BroadcastMessage "OnFlip").
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};
use crate::unity::transform;

script! {
    Piston {
        Activated: bool = false,
        MotorSpeedMutliplier: f32 = 5.0,
        CorrectionSpeedMultiplier: f32 = 3.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour (the piston's own)
        Phys: usize = 0,
        /// PistonHead.transform's PhysicalBehaviour
        Head: usize = 0,
        /// DamagableMachinery (`Game::machinery`)
        Machinery: usize = 0,
        /// Slider: the `Game::fixed_joints` entry, adopted from the spawn at Start.
        Slider: Option<usize> = None,
        alreadyReached: bool = false,
        /// originalLimits (min, max)
        originalLimits: [f32; 2] = [0.0, 0.0],
    }
}

impl Game {
    fn piston_slider(&self, i: usize) -> usize {
        self.pistons[i].Slider.unwrap_or_else(|| panic!("PistonBehaviour used before its Start (piston {i} of {}, {} pending starts)", self.pistons.len(), self.pending_starts.len()))
    }

    /// SliderJoint2D.limitState (180c04cc0): the b2 joint's m_limitState, 0 without a joint.
    pub(super) fn slider_joint_limit_state(&self, f: usize) -> u32 {
        match self.fixed_joints[f].joint {
            None => 0,
            Some(id) => self.world.joints[id].as_ref().unwrap().prismatic().limit_state,
        }
    }

    /// PistonBehaviour.Start: `f` is the adopted SliderJoint2D.
    pub fn piston_start(&mut self, i: usize, f: usize) {
        self.pistons[i].Slider = Some(f);
        let s = self.fixed_joints[f].slider.as_ref().unwrap();
        self.pistons[i].originalLimits = [s.lower, s.upper];
        self.piston_update_limits(i);
        self.transform_translate_z(self.pistons[i].Head, 0.05);
        if self.physicals[self.pistons[i].Phys].transform.localScale[0] < 0.0 {
            self.slider_joint_set_angle(f, 180.0);
        }
        self.pistons[i].alreadyReached = false;
    }

    /// Transform.Translate(0, 0, dz) of a physical's Transform (Space.Self: a rotation about z leaves the offset alone):
    /// the world z moves, the local pose follows through the parents, and the nested physicals are refreshed.
    fn transform_translate_z(&mut self, p: usize, dz: f32) {
        let q = self.physicals[p].transform.rotation;
        if q[0] != 0.0 || q[1] != 0.0 {
            unimplemented!("mod-only: Transform.Translate on a Transform tilted about x or y");
        }
        let chain = self.parent_chain(p);
        let t = &mut self.physicals[p].transform;
        let z = t.position[2] + dz;
        let (local, world) = transform::set_world(&chain, [t.position[0], t.position[1], z], t.rotation);
        // SetPosition keeps the local rotation as it was.
        t.local = Some((local.0, t.local.map_or(local.1, |l| l.1)));
        t.position = world.0;
        let (pos, rot) = (t.position, t.rotation);
        self.refresh_nested(p);
        // SyncTransforms of the change (spawn.rs sync_transforms): an interpolating body whose Transform x and y are within
        // 0.0001 of the movement state's current position takes no move, and the current pose becomes the Transform's.
        let rb = &mut self.physicals[p].rigidbody;
        let c = rb.movement.current.0;
        if rb.movement.interpolating && rb.interpolation == 1 && (pos[0] - c[0]).abs() <= 0.0001 && (pos[1] - c[1]).abs() <= 0.0001 {
            rb.movement.current = (pos, rot);
            // Then Rigidbody2D::RecreateJoints, as for any non-animated body's Transform change (it wakes both bodies).
            let body = rb.body;
            for (_, id) in self.world.body(body).joints.clone().into_iter().rev() {
                self.joint2d_recreate_any(id);
            }
        }
    }

    /// PistonBehaviour.Use
    pub fn piston_use(&mut self, i: usize) {
        if !self.machinery[self.pistons[i].Machinery].Destroyed {
            self.pistons[i].Activated = !self.pistons[i].Activated;
            self.pistons[i].alreadyReached = false;
        }
    }

    /// PistonBehaviour.Update
    pub fn piston_update(&mut self, i: usize) {
        self.piston_update_limits(i);
    }

    /// PistonBehaviour.FixedUpdate
    pub fn piston_fixed_update(&mut self, i: usize) {
        self.piston_move(i, if self.pistons[i].Activated { 1.0 } else { -1.0 });
    }

    /// PistonBehaviour.OnFlip(center): UpdateLimits() (1.28.41; originalLimits stays), then `Slider.angle` is 180 for a
    /// mirrored piston and 0 otherwise.
    pub fn piston_on_flip(&mut self, i: usize, _center: [f32; 3]) {
        let f = self.piston_slider(i);
        self.piston_update_limits(i);
        let mirrored = self.physicals[self.pistons[i].Phys].transform.localScale[0] < 0.0;
        self.slider_joint_set_angle(f, if mirrored { 180.0 } else { 0.0 });
    }

    /// PistonBehaviour.MovePiston
    pub(crate) fn piston_move(&mut self, i: usize, direction: f32) {
        let f = self.piston_slider(i);
        let destroyed = self.machinery[self.pistons[i].Machinery].Destroyed;
        self.slider_joint_set_use_motor(f, !destroyed);
        if destroyed {
            return;
        }
        let state = if !(direction > 0.0) { 1 } else { 2 };
        if self.slider_joint_limit_state(f) != state {
            let mut num = 1.0f32;
            if self.pistons[i].alreadyReached {
                let s = self.fixed_joints[f].slider.as_ref().unwrap();
                let num2 = self.slider_joint_translation(f) - if direction > 0.0 { s.upper } else { s.lower };
                num = mathf::clamp01((d(num2) * num2 * self.pistons[i].CorrectionSpeedMultiplier).f());
            }
            let x = self.physicals[self.pistons[i].Phys].transform.localScale[0];
            self.piston_set_motor_speed(i, (d(direction) * x.abs() * num).f());
        } else {
            self.pistons[i].alreadyReached = true;
            self.piston_set_motor_speed(i, 0.0);
        }
    }

    /// PistonBehaviour.UpdateLimits
    pub(crate) fn piston_update_limits(&mut self, i: usize) {
        let f = self.piston_slider(i);
        let num = self.physicals[self.pistons[i].Phys].transform.localScale[0].abs();
        let [min, max] = self.pistons[i].originalLimits;
        self.slider_joint_set_limits(f, min * num, max * num);
    }

    /// PistonBehaviour.SetMotorSpeed: `Slider.motor = motor` with the new speed (SetMotor also turns useMotor on).
    pub(crate) fn piston_set_motor_speed(&mut self, i: usize, v: f32) {
        let f = self.piston_slider(i);
        let p = &self.pistons[i];
        let speed = (d(v) * p.MotorSpeedMutliplier * mathf::max(1.0, self.physicals[p.Phys].Charge())).f();
        let max_torque = self.fixed_joints[f].slider.as_ref().unwrap().max_motor_torque;
        self.slider_joint_set_motor(f, speed, max_torque);
    }
}
