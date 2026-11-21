//! GripBehaviour (a hand's grip): a Use picks up the nearest holding point of an object within GripPosition.z, moves the
//! object there and welds it to the hand with a FixedJoint2D; the next Use, the hand's death (DropOnDeath) or the
//! object's destruction drops it, and the two ends collide again once the object is 1 unit away or 15 s passed.
//! OnGripped / OnDrop have no receiver; OnChangePause reattaches the joint when the game unpauses (Game::change_pause).
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::explosion::DEFAULT_RAYCAST_LAYERS;
use super::{d, mathf, BodyOwner, Game};
use crate::b2::math::Vec2 as V;
use crate::unity::transform::set_world;

impl Game {
    /// `(bool)CurrentlyHolding`: the held PhysicalBehaviour, unless it was destroyed.
    pub(crate) fn grip_held(&self, g: usize) -> Option<usize> {
        self.grips[g].CurrentlyHolding.filter(|&h| !self.physicals[h].destroyed)
    }

    /// GripBehaviour.Start: CollidersToIgnore = transform.root.GetComponentsInChildren<Collider2D>(); a saved grip
    /// holding something attaches again.
    pub fn grip_start(&mut self, g: usize) {
        let id = self.grips[g].id;
        self.grips[g].CollidersToIgnore = self.with_component_instance(id, |_, inst, mut root| {
            while let Some(up) = inst.objects[root].parent {
                root = up;
            }
            super::link::colliders_in_children(inst, root).into_iter().map(|(c, _)| c).collect()
        });
        if self.grips[g].isHolding {
            if let Some(h) = self.grip_held(g) {
                let at = self.grips[g].NearestHoldingPos;
                self.grip_attach(g, h, at);
            }
        }
    }

    /// GripBehaviour.Use
    pub fn grip_use(&mut self, g: usize) {
        if !self.grips[g].isHolding {
            self.grip_pick_up_nearest(g);
        } else {
            self.grip_drop(g);
        }
    }

    /// GripBehaviour.FixedUpdate
    pub fn grip_fixed_update(&mut self, g: usize) {
        if let Some(held) = self.grip_held(g) {
            let me = self.grips[g].PhysicalBehaviour;
            if self.physicals[me].SimulateTemperature && self.physicals[held].SimulateTemperature {
                self.average_temperature(me, held, 0.02);
            }
        }
    }

    /// GripBehaviour.OnChangePause, GripBehaviour.Awake's and OnDestroy's ModdingApi.Events.OnChangePause listener (Game::change_pause calls every live grip): once the game
    /// runs again, a hold whose hand has turned against the held object since the joint was made reattaches the joint's body.
    pub(crate) fn grip_on_change_pause(&mut self, g: usize, stopped: bool) {
        let Some(held) = self.grip_held(g).filter(|_| !stopped && self.grips[g].isHolding) else { return };
        let me = self.grips[g].PhysicalBehaviour;
        let now = (d(super::dump::euler_z(self.physicals[me].transform.rotation)) - super::dump::euler_z(self.physicals[held].transform.rotation)).f();
        if mathf::approximately(now, self.grips[g].originalRotation) {
            return;
        }
        self.grips[g].originalRotation = now;
        let Some(f) = self.grips[g].joint.filter(|&f| !self.fixed_joints[f].destroyed) else { crate::game::menu::throw("MissingReferenceException: The object of type 'FixedJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.") };
        self.fixed_joint_set_connected_body(f, None);
        self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(held)));
    }

    /// Global.set_Paused: paused = value, ModdingApi.Events.OnChangePause.Invoke(this, value), then every PhysicalBehaviour of
    /// PhysicalObjectsInWorld gets rigidbody.interpolation = CurrentInterpolationMode.
    pub fn set_paused(&mut self, value: bool) {
        self.paused = value;
        self.change_pause(value);
        let mode = self.current_interpolation_mode();
        for p in 0..self.physicals.len() {
            if self.physicals[p].in_world && !self.physicals[p].destroyed {
                self.rigidbody_set_interpolation(BodyOwner::Physical(p), mode);
            }
        }
    }

    /// Global.get_CurrentInterpolationMode: Interpolate (1) unless paused, then None (0).
    pub fn current_interpolation_mode(&self) -> i32 {
        if !self.paused { 1 } else { 0 }
    }

    /// Rigidbody2D::SetInterpolation (180c23700): a changed mode moves the Rigidbody2D in the scene's update lists
    /// (RemoveRigidbodyUpdates, AddRigidbodyUpdates: the movement list is rebuilt after the next step); then, changed or
    /// not, the movement state's previous and body poses become its current one (+0xc4 into +0xa8 and +0xe0) and it stops
    /// interpolating.
    pub fn rigidbody_set_interpolation(&mut self, owner: BodyOwner, mode: i32) {
        let rb = self.owner_rigidbody_mut(owner);
        let changed = rb.interpolation != mode && rb.detached.is_none();
        rb.interpolation = mode;
        rb.movement.previous = rb.movement.current;
        rb.movement.body = rb.movement.current;
        rb.movement.interpolating = false;
        if changed {
            rb.movement.listed = false;
            self.rigidbody_hierarchy_changed = true;
        }
    }

    // ModEvent`1.AddListener: a non-null listener into the listeners HashSet (GripBehaviour.Awake, FixedJointWireBehaviour.Awake).
    // ModEvent`1.RemoveListener: a non-null listener out of it (their OnDestroy: a destroyed component is skipped below).
    // ModEvent`1.AddPersistentListener: into persistentListeners, which ClearListeners keeps; nothing in the game calls it.
    // ModEvent`1.LegacyEvent: ModEvents..ctor sets it to the ModAPI.Invoke* of every event but OnChangePause.
    // ModEvent`1..ctor: empty listener sets.
    /// ModEvent`1.Invoke of ModdingApi.Events.OnChangePause (ModdingApi.Events, ModEvents..ctor): every listener, then
    /// every persistent one (none), then LegacyEvent (OnChangePause has none). The listeners are the grips and the
    /// FixedJointWireBehaviours: ModEvent`1.AddListener at Awake, ModEvent`1.RemoveListener at OnDestroy, all dropped by
    /// ModEvents.ClearListeners at a map load. The handlers touch only their own joint, so the HashSet's order is not observable.
    pub fn change_pause(&mut self, stopped: bool) {
        for g in 0..self.grips.len() {
            let id = self.grips[g].id;
            if !self.destroyed_components.contains(&id) && !self.pause_listeners_cleared.contains(&id) {
                self.grip_on_change_pause(g, stopped);
            }
        }
        for w in 0..self.blood_wires.len() {
            let id = self.blood_wires[w].id;
            if self.blood_wires[w].kind.is_fixed() && !self.destroyed_components.contains(&id) && !self.pause_listeners_cleared.contains(&id) {
                self.fixed_wire_on_change_pause(w, stopped);
            }
        }
    }

    /// ModEvents.ClearListeners (MapLoaderBehaviour.Load, after InstantiateMap): ModEvent`1.ClearListeners on every event;
    /// only OnChangePause has listeners in the game (the persistent listeners and LegacyEvent stay).
    pub fn mod_events_clear_listeners(&mut self) {
        let ids: Vec<i32> = self.grips.iter().map(|g| g.id).chain(self.blood_wires.iter().filter(|w| w.kind.is_fixed()).map(|w| w.id)).collect();
        self.pause_listeners_cleared.extend(ids);
    }

    /// GripBehaviour.Update
    pub fn grip_update(&mut self, g: usize) {
        if self.grips[g].isHolding && self.grip_held(g).is_none() {
            self.grip_drop(g);
        }
    }

    /// transform.root of physical `p`'s Transform, as its component id.
    pub(super) fn transform_root_id(&self, p: usize) -> i32 {
        match self.physicals[p].parent {
            None => self.physicals[p].transform_id,
            Some(mut n) => {
                while let Some(up) = self.transform_nodes[n].parent {
                    n = up;
                }
                self.transform_nodes[n].id
            }
        }
    }

    /// PhysicalBehaviour.GetNearestLocalHoldingPoint: the holding position (local, z dropped) nearest the world point and
    /// its squared distance; float.MaxValue and zero without any.
    pub(super) fn nearest_local_holding_point(&self, v: usize, world: [f32; 2]) -> ([f32; 2], f32) {
        let local = self.inverse_transform_point(v, [world[0], world[1], 0.0]);
        let mut result = [0.0; 2];
        let mut distance = f32::MAX;
        for h in &self.physicals[v].HoldingPositions {
            let sq = mathf::sqr_magnitude([local[0] - h[0], local[1] - h[1]]);
            if !(distance < sq) {
                distance = sq;
                result = [h[0], h[1]];
            }
        }
        (result, distance)
    }

    /// GripBehaviour.PickUpNearestObject
    pub(crate) fn grip_pick_up_nearest(&mut self, g: usize) {
        let me = self.grips[g].PhysicalBehaviour;
        let grip = self.grips[g].GripPosition;
        let point = self.transform_point3(me, grip);
        let world = [point[0], point[1]];
        // OverlapCircleNonAlloc into a 32-element buffer.
        let mut hits = self.overlap_circle(V::new(world[0], world[1]), grip[2], DEFAULT_RAYCAST_LAYERS);
        // OverlapCircleArray_Binding (180bf4620): the query's first min(count, buffer length) colliders.
        hits.truncate(GRIP_BUFFER);
        let root = self.transform_root_id(me);
        let mut nearest = f32::MAX;
        let mut found = None;
        for (physical, _) in hits {
            // PhysicalObjectsInWorldByTransform[collider.transform]: a collider on a PhysicalBehaviour's own Transform.
            let Some(v) = physical.filter(|&v| self.physicals[v].in_world) else { continue };
            if self.transform_root_id(v) == root {
                continue;
            }
            // 1.28.45: an object with PreventGripping is skipped.
            if crate::assets::il::is_1_28_45() && self.physicals[v].PreventGripping {
                continue;
            }
            let (at, distance) = self.nearest_local_holding_point(v, world);
            if distance < nearest {
                nearest = distance;
                self.grips[g].NearestHoldingPos = at;
                found = Some(v);
            }
        }
        if let Some(v) = found {
            let at = self.grips[g].NearestHoldingPos;
            self.grip_attach(g, v, at);
        }
    }

    /// GripBehaviour.Attach
    pub(crate) fn grip_attach(&mut self, g: usize, phys: usize, other_local: [f32; 2]) {
        self.grips[g].isHolding = true;
        self.grips[g].CurrentlyHolding = Some(phys);
        let me = self.grips[g].PhysicalBehaviour;
        let a = self.transform_point3(me, self.grips[g].GripPosition);
        let b = self.transform_point3(phys, [other_local[0], other_local[1], 0.0]);
        let at = self.physicals[phys].transform.position;
        self.physical_set_position(phys, [at[0] + (a[0] - b[0]), at[1] + (a[1] - b[1]), at[2] + (a[2] - b[2])]);
        self.grip_create_joint(g, phys, other_local);
        // SendMessage("OnGripped"): no component has one.
    }

    /// GripBehaviour.CreateJoint
    pub(crate) fn grip_create_joint(&mut self, g: usize, phys: usize, other_local: [f32; 2]) {
        let me = self.grips[g].PhysicalBehaviour;
        // AddComponent<FixedJoint2D> takes the component's instance id.
        self.allocate_instance_id();
        let f = self.add_fixed_joint_component(me);
        // `joint = AddComponent`: the old joint is not destroyed, it stays on the hand unreferenced.
        if let Some(old) = self.grips[g].joint.replace(f) {
            self.loose_fixed_joints.push(old);
        }
        self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(phys)));
        let grip = self.grips[g].GripPosition;
        self.fixed_joint_set_anchor(f, [grip[0], grip[1]]);
        self.fixed_joint_set_connected_anchor(f, other_local);
        self.fixed_joint_set_enable_collision(f, false);
        self.grips[g].originalRotation = (d(super::dump::euler_z(self.physicals[me].transform.rotation)) - super::dump::euler_z(self.physicals[phys].transform.rotation)).f();
        self.grip_refresh_no_collide(g, true);
    }

    /// held.GetComponentsInChildren<Collider2D>(). A recording has no clone: its item's colliders are on its own object.
    fn colliders_of_held(&mut self, held: usize) -> Vec<(i32, ())> {
        if self.is_recording() {
            return self.physicals[held].colliders.iter().map(|&c| (c, ())).collect();
        }
        self.with_component_instance(self.physicals[held].id, |_, inst, o| super::link::colliders_in_children(inst, o)).into_iter().map(|(c, _)| (c, ())).collect()
    }

    /// GripBehaviour.RefreshNoCollide
    pub(crate) fn grip_refresh_no_collide(&mut self, g: usize, ignore: bool) {
        let Some(held) = self.grip_held(g) else { return };
        let held_colliders = self.colliders_of_held(held);
        for (c, _) in held_colliders {
            for item in self.grips[g].CollidersToIgnore.clone() {
                if item != c {
                    self.ignore_collision_substitute(c, item, ignore);
                }
            }
        }
    }

    /// GripBehaviour.DropObject
    pub fn grip_drop(&mut self, g: usize) {
        if !self.grips[g].isHolding {
            return;
        }
        self.grips[g].isHolding = false;
        // Object.Destroy(joint)
        if let Some(f) = self.grips[g].joint.filter(|&f| !self.fixed_joints[f].destroyed) {
            self.destroy_joint_delayed(f);
        }
        if let Some(held) = self.grip_held(g) {
            let (id, me) = (self.grips[g].id, self.grips[g].PhysicalBehaviour);
            let go = self.physicals[me].id;
            let routine = self.grip_reactivate_routine(g);
            self.start_coroutine(id, go, routine);
            self.physicals[held].beingHeldByGripper = false;
            // SendMessage("OnDrop"): no component has one.
            self.grips[g].CurrentlyHolding = None;
        }
    }

    /// GripBehaviour.WaitUntilCollisionReactivate: a second at a time, at most 15, until `other` is further than 1 from
    /// the grip; then the pairs are no longer ignored. A destroyed `other` is a MissingReferenceException, which ends it.
    /// `other` is CurrentlyHolding when the routine is made (null throws after the first second).
    pub(crate) fn grip_reactivate_routine(&self, g: usize) -> Routine {
        let other = self.grips[g].CurrentlyHolding;
        let mut seconds = 15;
        let mut started = false;
        Box::new(move |game| {
            if started {
                let other = other.unwrap_or_else(|| crate::game::menu::npe());
                seconds -= 1;
                if game.physicals[other].destroyed {
                    return Yield::Done;
                }
                if seconds > 0 {
                    let (me, grip) = (game.grips[g].PhysicalBehaviour, game.grips[g].GripPosition);
                    let (a, b) = (game.physicals[other].transform.position, game.transform_point3(me, grip));
                    let (x, y) = (a[0] - b[0], a[1] - b[1]);
                    let distance = (d(x) * x + d(y) * y).0.sqrt() as f32;
                    if !(distance > 1.0) {
                        return Yield::WaitForSeconds(1.0);
                    }
                }
                let colliders = game.colliders_of_held(other);
                for (c, _) in colliders {
                    for item in game.grips[g].CollidersToIgnore.clone() {
                        if item != c {
                            game.ignore_collision_substitute(c, item, false);
                        }
                    }
                }
                return Yield::Done;
            }
            started = true;
            Yield::WaitForSeconds(1.0)
        })
    }

    /// Transform.position = pos on a running physical: its Transform and the nested physicals' follow, the clone's
    /// Transforms (stale once a body moved) are refreshed from the live poses first, and the bodies and joints follow
    /// at the next SyncTransforms.
    pub(super) fn physical_set_position(&mut self, p: usize, pos: [f32; 3]) {
        let id = self.physicals[p].id;
        if self.is_recording() {
            // No clone: the next sync moves the body.
            self.recorded_moves.push((p, false));
            // SetPosition stores the local position (world to local through the parents); the world position
            // SyncTransforms reads is that local one back through the parents.
            let chain = self.parent_chain(p);
            let t = &mut self.physicals[p].transform;
            match t.local.as_mut().filter(|_| !chain.is_empty()) {
                Some(local) => {
                    let ((lpos, _), (world, _)) = crate::unity::transform::set_world(&chain, pos, t.rotation);
                    (local.0, t.position) = (lpos, world);
                }
                // fnv52 r2: a kept local pose (no parent) is the world one: local_chain (lossy_scale) reads it.
                None => {
                    t.position = pos;
                    if let Some(local) = t.local.as_mut() {
                        local.0 = pos;
                    }
                }
            }
            return;
        }
        self.physical_set_world(p, Some(pos), None);
    }

    /// Transform.rotation = q on a running physical, as physical_set_position does it.
    pub(super) fn physical_set_rotation(&mut self, p: usize, q: [f32; 4]) {
        if self.is_recording() {
            // No clone: the next sync (SyncTransforms) takes the body's angle from the new rotation, with the position of any move queued before.
            // fnv52 r2: and a kept local rotation (CrossbowBehaviour.ConnectSyringe's transform.rotation: the syringe's lossy scale
            // for the joint's connected anchor reads it through local_chain).
            if self.physicals[p].transform.local.is_some() {
                let chain = self.parent_chain(p);
                let lq = if chain.is_empty() { q } else { crate::unity::transform::set_world(&chain, self.physicals[p].transform.position, q).0 .1 };
                self.physicals[p].transform.local.as_mut().unwrap().1 = lq;
            }
            self.physicals[p].transform.rotation = q;
            match self.recorded_moves.iter_mut().find(|m| m.0 == p) {
                Some(m) => m.1 = true,
                None => self.recorded_moves.push((p, true)),
            }
            return;
        }
        self.physical_set_world(p, None, Some(q));
    }

    fn physical_set_world(&mut self, p: usize, pos: Option<[f32; 3]>, rot: Option<[f32; 4]>) {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| {
            for x in std::iter::once(o).chain(inst.descendants(o)) {
                let key = inst.objects[x].components[0];
                let Some(q) = g.physicals.iter().position(|ph| ph.transform_id == key && !ph.destroyed) else { continue };
                if g.physicals[q].parent.is_some() != inst.objects[x].parent.is_some() {
                    unimplemented!("mod-only: Transform.position = on a body re-parented under another clone's Transform");
                }
                let t = &g.physicals[q].transform;
                let (position, rotation) = t.local.unwrap_or_else(|| set_world(&g.parent_chain(q), t.position, t.rotation).0);
                let trs = &mut inst.objects[x].trs;
                trs.t = [position[0], position[1], position[2], trs.t[3]];
                trs.q = rotation;
                trs.s = [t.localScale[0], t.localScale[1], t.localScale[2], trs.s[3]];
            }
            match (pos, rot) {
                (Some(pos), _) => inst.set_world_position(o, pos),
                (_, Some(q)) => inst.set_world_rotation(o, q),
                _ => unreachable!(),
            }
        });
        let chain = self.parent_chain(p);
        let t = &self.physicals[p].transform;
        let (local, world) = set_world(&chain, pos.unwrap_or(t.position), rot.unwrap_or(t.rotation));
        let t = &mut self.physicals[p].transform;
        (t.position, t.rotation, t.local) = (world.0, world.1, Some(local));
        self.refresh_nested(p);
        self.unsync_instance_of(id);
    }

    /// The clone's Transforms take its physicals' live poses (stale once a body moved). False (nothing changed) when a
    /// body was re-parented under another clone's Transform.
    pub(super) fn refresh_clone_poses(&self, inst: &mut super::spawn::Instance) -> bool {
        let live: Vec<(usize, usize)> = (0..inst.objects.len())
            .filter_map(|x| self.physicals.iter().position(|ph| ph.transform_id == inst.objects[x].components[0] && !ph.destroyed).map(|q| (x, q)))
            .collect();
        if live.iter().any(|&(x, q)| self.physicals[q].parent.is_some() != inst.objects[x].parent.is_some()) {
            return false;
        }
        for (x, q) in live {
            let t = &self.physicals[q].transform;
            let (position, rotation) = t.local.unwrap_or_else(|| set_world(&self.parent_chain(q), t.position, t.rotation).0);
            let trs = &mut inst.objects[x].trs;
            trs.t = [position[0], position[1], position[2], trs.t[3]];
            trs.q = rotation;
            trs.s = [t.localScale[0], t.localScale[1], t.localScale[2], trs.s[3]];
        }
        true
    }

    /// Transform.localScale = s on a live physical: the clone's Transforms take its physicals' live poses (stale once a
    /// body moved; a joint partner's lossy scale reads its rotation), then SetLocalScale flags the body's joints and the
    /// colliders for the next SyncTransforms. The physicals hung below its Transform follow through the new scale.
    pub(super) fn physical_set_local_scale(&mut self, p: usize, s: [f32; 3]) {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| {
            if !g.refresh_clone_poses(inst) {
                unimplemented!("mod-only: Transform.localScale = on a clone with a body re-parented under another clone's Transform");
            }
            inst.set_local_scale(o, s);
        });
        self.physicals[p].transform.localScale = s;
        if let Some(n) = self.body_node(p) {
            self.transform_nodes[n].trs.s = [s[0], s[1], s[2], 0.0];
        }
        self.refresh_nested(p);
        self.unsync_instance_of(id);
    }
}

/// GripBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const GRIP_BUFFER: usize = 32;
