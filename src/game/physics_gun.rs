//! PhysicsGunBehaviour (the Physics Gun). While its physical is used continuously and holds nothing, Update casts a ray
//! from LineStart along transform.right * localScale.x; a hit on a PhysicalBehaviour in LayersToGrab becomes the Target,
//! held by a RelativeJoint2D on the gun's GameObject (maxForce 1200, maxTorque 300, enableCollision) to the target's
//! Rigidbody2D. Stopping the use, a disintegrated target and OnDisable let go (Object.Destroy of the joint). The line
//! (a SmoothLineControllerBehaviour between the gun and LineTarget, which the gun moves) is shown while it is used; the
//! sounds are audio.
#![allow(non_snake_case)]

use super::explosion::normalized3;
use super::{script, BodyOwner, Game};
use crate::b2::math::Vec2 as V;
use crate::unity::transform::{global_position, Trs};

script! {
    PhysicsGun {
        MaxRange: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>() (Awake)
        physical: usize = 0,
        /// LineStart: the local TRS of its object and of each parent below the gun's own object.
        LineStart: Vec<Trs> = Vec::new(),
        /// LayersToHit and LayersToGrab (LayerMask bits)
        LayersToHit: u32 = 0,
        LayersToGrab: u32 = 0,
        /// LineRenderer: the SmoothLineControllerBehaviour's component id and its object in the gun's instance.
        line_id: i32 = 0,
        line_object: usize = 0,
        /// LineTarget: its Transform's component id (None: null).
        LineTarget: Option<i32> = None,
        /// The held PhysicalBehaviour (a `physicals` index).
        Target: Option<usize> = None,
        /// The RelativeJoint2D (`fixed_joints` index).
        joint: Option<usize> = None,
    }
}

impl Game {
    /// `(bool)Target`: Unity's null check, false once the PhysicalBehaviour was destroyed.
    fn physics_gun_has_target(&self, i: usize) -> bool {
        self.physics_guns[i].Target.is_some_and(|t| !self.physicals[t].destroyed)
    }

    /// The gun's SmoothLineControllerBehaviour (`Game::smooth_lines`).
    fn physics_gun_line(&self, i: usize) -> usize {
        let id = self.physics_guns[i].line_id;
        self.smooth_lines.iter().position(|s| s.id == id).expect("the gun's SmoothLineControllerBehaviour")
    }

    /// LineTarget.position = position
    fn physics_gun_set_line_target(&mut self, i: usize, position: [f32; 3]) {
        let target = self.physics_guns[i].LineTarget.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_transform_position_of(target, position);
    }

    /// LineRenderer.gameObject.SetActive(active), from outside the gun's instance.
    fn physics_gun_show_line(&mut self, i: usize, active: bool) {
        let (id, lo) = (self.physics_guns[i].id, self.physics_guns[i].line_object);
        self.with_component_instance(id, |g, inst, _| g.set_active_scripts(inst, lo, active));
    }

    /// PhysicsGunBehaviour.Update
    pub fn physics_gun_update(&mut self, i: usize) {
        let p = self.physics_guns[i].physical;
        let line = self.physics_gun_line(i);
        if self.started_used_continuously(p) {
            if self.physics_gun_has_target(i) {
                let at = self.physicals[self.physics_guns[i].Target.unwrap()].transform.position;
                self.physics_gun_set_line_target(i, at);
            }
            self.smooth_line_update_line(line);
            self.physics_gun_show_line(i, true);
            let id = self.physics_guns[i].id;
            let s = self.audio_src(id, "AudioSource");
            self.audio.play(s);
            self.audio_one_shot(id, "AudioSource", "StartSound");
        }
        if self.used_continuously(p) && !self.physics_gun_has_target(i) {
            self.physics_gun_query_target(i);
            self.smooth_line_update_line(line);
        }
        if self.stopped_used_continuously(p) {
            self.physics_gun_stop_ray(i);
        }
        if self.physics_gun_has_target(i) {
            let t = self.physics_guns[i].Target.unwrap();
            let at = self.physicals[t].transform.position;
            self.physics_gun_set_line_target(i, at);
            if self.physicals[t].isDisintegrated {
                self.physics_gun_set_target(i, None);
            }
        }
    }

    /// PhysicsGunBehaviour.StopRay: SetTarget(null), LineRenderer.gameObject.SetActive(false), AudioSource.Stop(),
    /// AudioSource.PlayOneShot(EndSound).
    pub(crate) fn physics_gun_stop_ray(&mut self, i: usize) {
        self.physics_gun_set_target(i, None);
        self.physics_gun_show_line(i, false);
        let id = self.physics_guns[i].id;
        let s = self.audio_src(id, "AudioSource");
        self.audio.stop(s);
        self.audio_one_shot(id, "AudioSource", "EndSound");
    }

    /// PhysicsGunBehaviour.OnDisable: SetTarget(null), LineRenderer.gameObject.SetActive(false), AudioSource.Stop().
    pub fn physics_gun_on_disable(&mut self, i: usize, inst: &mut super::spawn::Instance) {
        self.physics_gun_set_target(i, None);
        let lo = self.physics_guns[i].line_object;
        self.set_active_scripts(inst, lo, false);
        let s = self.audio_src(self.physics_guns[i].id, "AudioSource");
        self.audio.stop(s);
    }

    /// PhysicsGunBehaviour.QueryTarget: Physics2D.Raycast(LineStart.position, direction, MaxRange, LayersToHit); a hit
    /// transform whose layer is in LayersToGrab and that has a PhysicalBehaviour in the world becomes the Target.
    pub(crate) fn physics_gun_query_target(&mut self, i: usize) {
        let s = &self.physics_guns[i];
        let (p, layers, grab, range) = (s.physical, s.LayersToHit, s.LayersToGrab, s.MaxRange);
        self.sync_pending();
        // LineStart.transform.position: its local TRS, then the gun's chain.
        let nodes: Vec<Trs> = self.physics_guns[i].LineStart.iter().copied().chain(self.local_chain(p).iter().rev().copied()).collect();
        let mut origin = [nodes[0].t[0], nodes[0].t[1], nodes[0].t[2], 0.0];
        for n in &nodes[1..] {
            origin = global_position(n, origin);
        }
        // (base.transform.right * base.transform.localScale.x).normalized
        let right = self.transform_direction(p, [1.0, 0.0, 0.0]);
        let scale = self.physicals[p].transform.localScale[0];
        let dir = normalized3([right[0] * scale, right[1] * scale, right[2] * scale]);
        let Some(hit) = self.raycast(V::new(origin[0], origin[1]), [dir[0], dir[1]], range, layers, f32::NEG_INFINITY, f32::INFINITY) else {
            // LineTarget.position = LineStart.transform.position + normalized * MaxRange
            self.physics_gun_set_line_target(i, [origin[0] + dir[0] * range, origin[1] + dir[1] * range, origin[2] + dir[2] * range]);
            return;
        };
        // raycastHit2D.transform: the attached Rigidbody2D's GameObject, else the collider's.
        let physical = self.hit_transform_physical(&hit);
        let layer = match physical {
            Some(t) => self.physical_layer(t),
            // A Rigidbody2D without a PhysicalBehaviour: hit_transform_physical has checked the collider sits on that
            // GameObject, so its layer is the collider's.
            None => self.world.layers[&hit.game_object],
        };
        if grab & (1 << layer) != 0 {
            // One that is no key of PhysicalObjectsInWorldByTransform is not found.
            if let Some(target) = physical.filter(|&t| self.physicals[t].in_world) {
                self.physics_gun_set_target(i, Some(target));
            }
        } else {
            self.physics_gun_set_line_target(i, [hit.point.x, hit.point.y, 0.0]);
        }
    }

    /// PhysicsGunBehaviour.SetTarget: null destroys the joint; otherwise a RelativeJoint2D on the gun's GameObject
    /// holds the target's Rigidbody2D (LineTarget.position goes to the target first).
    pub(crate) fn physics_gun_set_target(&mut self, i: usize, target: Option<usize>) {
        self.physics_guns[i].Target = target;
        if let Some(t) = target.filter(|&t| !self.physicals[t].destroyed) {
            let at = self.physicals[t].transform.position;
            self.physics_gun_set_line_target(i, at);
        }
        let Some(t) = target.filter(|&t| !self.physicals[t].destroyed) else {
            // Object.Destroy(joint): a null or destroyed component is ignored.
            if let Some(f) = self.physics_guns[i].joint.filter(|&f| !self.fixed_joints[f].destroyed) {
                self.destroy_joint_delayed(f);
            }
            return;
        };
        let gun = self.physics_guns[i].physical;
        // AddComponent<RelativeJoint2D>() takes an instance id.
        self.allocate_instance_id();
        let f = self.add_relative_joint_component(gun);
        self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(t)));
        self.relative_joint_set_max_force(f, 1200.0);
        self.relative_joint_set_max_torque(f, 300.0);
        self.fixed_joint_set_enable_collision(f, true);
        self.physics_guns[i].joint = Some(f);
    }
}
