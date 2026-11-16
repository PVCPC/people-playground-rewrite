//! DroneBehaviour and PropellerBehaviour. Use toggles the drone; while it is on and alive, FixedUpdate hovers it with its
//! propellers' relative forces and steers the propellers' hinge motors, turning toward the nearest living thing it sees
//! on either side. Start hangs each propeller on a HingeJoint2D (motor on, limits off) connected to the drone's body.
//! The looped hum plays on `audioSource` while it is on.
#![allow(non_snake_case)]

use super::{d, mathf, script, BodyOwner, Game};
use crate::b2::math::Vec2 as V;

script! {
    Drone {
        hoverHeight: f32 = 1.8,
        strength: f32 = 1.0,
        activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (Awake's GetComponent)
        physical: usize = 0,
        /// The `rigidbody` field.
        rigidbody: Option<BodyOwner> = None,
        /// The `propellers` entries' component ids (`Game::propellers`).
        propellers: Vec<i32> = Vec::new(),
        /// The private health field (25).
        health: f32 = 25.0,
    }
}

/// PropellerBehaviour: Awake's rigidbody, and the hinge DroneBehaviour.Start gave it (a `Game::fixed_joints` entry).
#[derive(Clone, Debug)]
pub struct Propeller {
    pub id: i32,
    pub enabled: bool,
    pub rigidbody: BodyOwner,
    pub hinge: Option<usize>,
}

impl Game {
    fn propeller_index(&self, id: i32) -> usize {
        self.propellers.iter().position(|p| p.id == id).expect("a PropellerBehaviour in the arena")
    }

    /// DroneBehaviour.rigidbody (null is a NullReferenceException at the link).
    fn drone_rigidbody(&self, i: usize) -> super::Rigidbody {
        let owner = self.drones[i].rigidbody.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.owner_rigidbody(owner).clone()
    }

    /// DroneBehaviour.Start: each propeller gets a HingeJoint2D connected to the drone's body, limits off, motor on, no
    /// connected anchor auto-configuration, and a HingeJointLimitAutofixBehaviour. The hinge is unlimited, so the
    /// autofix's Start and OnFlip find no limited hinge, slider or FixedJoint2D below it and change nothing; only its
    /// instance id is taken.
    pub fn drone_start(&mut self, i: usize) {
        let connected = self.drones[i].rigidbody;
        for id in self.drones[i].propellers.clone() {
            let q = self.propeller_index(id);
            // AddComponent<HingeJoint2D>
            self.allocate_instance_id();
            let f = self.add_hinge_joint_component_on(self.propellers[q].rigidbody);
            self.propellers[q].hinge = Some(f);
            self.fixed_joint_set_connected_body(f, connected);
            // useLimits = false: the hinge's limits are off already, so EnableLimit(false) changes nothing.
            self.fixed_joint_hinge_set_use_motor(f, true);
            self.fixed_joint_set_auto_configure(f, false);
            // AddComponent<HingeJointLimitAutofixBehaviour>
            self.allocate_instance_id();
        }
    }

    /// audioSource (a null one throws).
    fn drone_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.drones[i].id, "audioSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// DroneBehaviour.Awake: audioSource.loop = true (the PhysicalBehaviour is linked).
    pub fn drone_awake(&mut self, i: usize) {
        let src = self.drone_source(i);
        self.audio.get_mut(src).looping = true;
    }

    /// DroneBehaviour.Use
    pub fn drone_use(&mut self, i: usize) {
        self.drones[i].activated = !self.drones[i].activated;
        let src = self.drone_source(i);
        if self.drones[i].activated {
            self.audio.play(src);
        } else {
            self.audio.stop(src);
        }
    }

    /// DroneBehaviour.IsAlive
    pub fn drone_is_alive(&self, i: usize) -> bool {
        self.drones[i].health > 0.0
    }

    /// DroneBehaviour.Shot
    pub fn drone_shot(&mut self, i: usize, damage: f32) {
        self.drones[i].health -= damage;
    }

    /// PropellerBehaviour.SetMotorAngle
    pub(crate) fn propeller_set_motor_angle(&mut self, q: usize, angle: f32, speed: f32) {
        let f = self.propellers[q].hinge.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let rotation = match self.propellers[q].rigidbody {
            BodyOwner::Physical(p) => self.physicals[p].transform.rotation,
            BodyOwner::Node(k) => self.node_pose(self.node_rigidbodies[k].node).1,
        };
        let torque = self.fixed_joints[f].hinge.as_ref().expect("a HingeJoint2D").motor[1];
        let motor_speed = (0.0 - speed) * mathf::delta_angle(super::dump::euler_z(rotation), angle);
        self.fixed_joint_hinge_set_motor(f, motor_speed, torque);
    }

    /// DroneBehaviour.Steer
    pub(crate) fn drone_steer(&mut self, i: usize, angle: f32) {
        for id in self.drones[i].propellers.clone() {
            let q = self.propeller_index(id);
            self.propeller_set_motor_angle(q, angle, 15.0);
        }
    }

    /// DroneBehaviour.SpinPropellers: AddRelativeForce(Vector2.up * intensity) on each propeller.
    pub(crate) fn drone_spin_propellers(&mut self, i: usize, intensity: f32) {
        for id in self.drones[i].propellers.clone() {
            let q = self.propeller_index(id);
            let rigidbody = self.owner_rigidbody(self.propellers[q].rigidbody).clone();
            rigidbody.add_relative_force(&mut self.world, [0.0 * intensity, 1.0 * intensity]);
        }
    }

    /// DroneBehaviour.FixedUpdate
    pub fn drone_fixed_update(&mut self, i: usize) {
        let p = self.drones[i].physical;
        if self.physicals[p].rigidbody.bodyType != 0 {
            return;
        }
        // audioSource.volume = Lerp(volume, (!activated || !IsAlive()) ? 0 : rigidbody.velocity.magnitude * 0.05 + 0.3, 0.5)
        let src = self.drone_source(i);
        let target = if !self.drones[i].activated || !self.drone_is_alive(i) {
            0.0
        } else {
            let v = self.drone_rigidbody(i).velocity(&self.world);
            mathf::magnitude2(v) * 0.05 + 0.3
        };
        let volume = self.audio.get(src).volume;
        self.audio.get_mut(src).volume = mathf::lerp(volume, target, 0.5);
        if !self.drone_is_alive(i) {
            self.physicals[p].charge = 3.0;
            return;
        }
        if !self.drones[i].activated {
            return;
        }
        let position = self.physicals[p].transform.position;
        let rigidbody = self.drone_rigidbody(i);
        let hover = self.drones[i].hoverHeight;
        let Some(hit) = self.raycast_first(V::new(position[0], position[1]), [0.0, -1.0], hover + 3.0) else {
            self.drone_steer(i, 0.0);
            return;
        };
        let num = mathf::max(hover - hit.distance, 0.1);
        let angular = rigidbody.angular_velocity(&self.world);
        rigidbody.set_angular_velocity(&mut self.world, angular * 0.9);
        let charge = self.physicals[p].Charge();
        self.drone_spin_propellers(i, (d(num) * (d(2.0) + d(charge))).f());
        let velocity = rigidbody.velocity(&self.world);
        self.drone_spin_propellers(i, velocity[1] * -1.0);
        if !self.drone_search_and_destroy(i) {
            let velocity = rigidbody.velocity(&self.world);
            self.drone_steer(i, ((d(5.0) + d(charge)) * velocity[0]).f());
        }
    }

    /// DroneBehaviour.SearchAndDestroy
    pub(crate) fn drone_search_and_destroy(&mut self, i: usize) -> bool {
        let p = self.drones[i].physical;
        let position = self.physicals[p].transform.position;
        let origin = V::new(position[0] - 0.0 * 0.4, position[1] - 0.4);
        let mut hit = self.raycast_first(origin, [-1.0, 0.0], 500.0);
        if !self.drone_is_valid_target(i, hit) {
            hit = self.raycast_first(origin, [1.0, 0.0], 500.0);
        }
        if !self.drone_is_valid_target(i, hit) {
            return false;
        }
        let hit = hit.unwrap();
        // hit.collider.transform.position.x: the collider's own GameObject, here its physical's.
        let target = self.physical_of_collider(hit.collider).unwrap_or_else(|| unimplemented!("mod-only: DroneBehaviour.SearchAndDestroy: a target collider without a PhysicalBehaviour"));
        if let Some(&c) = self.physicals[target].all_colliders.first() {
            if self.world.collider_game_object(c) != Some(hit.game_object) {
                unimplemented!("mod-only: DroneBehaviour.SearchAndDestroy: a target collider on a child GameObject");
            }
        }
        self.drone_steer(i, if self.physicals[target].transform.position[0] > position[0] { -15.0 } else { 15.0 });
        true
    }

    /// DroneBehaviour.IsValidTarget: a hit whose transform.root is not this drone and has an AliveBehaviour that is alive
    /// (a person, or another drone).
    fn drone_is_valid_target(&mut self, i: usize, hit: Option<super::explosion::RayHit2D>) -> bool {
        let Some(hit) = hit else { return false };
        let own = self.drones[i].physical;
        let own_bodies = self.drones[i].propellers.iter().map(|&id| self.owner_rigidbody(self.propellers[self.propeller_index(id)].rigidbody).body).chain(self.drones[i].rigidbody.map(|o| self.owner_rigidbody(o).body)).collect::<Vec<_>>();
        if own_bodies.contains(&hit.body) {
            return false;
        }
        let Some(q) = self.physicals.iter().position(|x| !x.destroyed && x.rigidbody.body == hit.body) else {
            if let Some(n) = self.node_of_body(hit.body) {
                // hit.transform is the node's: its root, then the AliveBehaviour on it.
                let root = self.node_root_key(n);
                if root == self.root_of(own) {
                    return false;
                }
                if let Some(person) = self.person_of_root_key(root) {
                    return self.person_is_alive(person);
                }
                return match (0..self.drones.len()).find(|&j| self.root_of(self.drones[j].physical) == root) {
                    Some(j) => self.drone_is_alive(j),
                    None => false,
                };
            }
            // A collider without a body of its own object: no AliveBehaviour on its root.
            return false;
        };
        let root = self.root_of(q);
        if root == self.root_of(own) {
            return false;
        }
        if let Some(person) = self.person_of_root(q) {
            return self.person_is_alive(person);
        }
        match (0..self.drones.len()).find(|&j| self.root_of(self.drones[j].physical) == root) {
            Some(j) => self.drone_is_alive(j),
            None => false,
        }
    }
}
