//! FreezeBehaviour (the Ceiling Turret, Reactor A5 and Substructure). Start keeps the Rigidbody2D's velocity and
//! angularVelocity and requests a freeze (the first request makes the body static, water.rs); OnDestroy releases the
//! request and restores the saved velocities. Start also shows an outline for a second of real time (outline.rs).
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Freeze {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The GameObject's PhysicalBehaviour, whose rigidbody is GetComponent<Rigidbody2D>().
        physical: usize = 0,
        /// rb.velocity and rb.angularVelocity saved in Start (the Unity units: degrees per second for the latter).
        velocity: [f32; 2] = [0.0, 0.0],
        angularVelocity: f32 = 0.0,
        /// CreateOutlineObject's SpriteRenderer is enabled: from Start until Disappear, or the component's destruction.
        outline: bool = false,
    }
}

script! {
    /// FreezeSerialisationHelper (the Ceiling Turret): Start keeps a FreezeBehaviour on its object when IsFrozen, else
    /// destroys it. It has no other messages.
    FreezeSerialisation {
        IsFrozen: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
    }
}

impl Game {
    /// FreezeStackController.Clear: stack.Clear().
    pub fn freeze_stack_clear(&mut self) {
        self.freeze_stack.clear();
    }

    /// FreezeSerialisationHelper.Start, Utils.GetOrAddComponent: `IsFrozen` runs GetOrAddComponent<FreezeBehaviour>() (a present one is kept; a missing
    /// one is added), else TryGetComponent then Object.Destroy of the one there.
    pub fn freeze_serialisation_start(&mut self, h: usize) {
        let (id, is_frozen) = (self.freeze_serialisations[h].id, self.freeze_serialisations[h].IsFrozen);
        let components = self.with_component_instance(id, |_, inst, o| inst.objects[o].components.clone());
        let freeze = self.freezes.iter().position(|f| components.contains(&f.id) && !self.destroyed_components.contains(&f.id));
        if is_frozen {
            // Utils.GetOrAddComponent<FreezeBehaviour>: TryGetComponent, else AddComponent (the object's PhysicalBehaviour
            // is the Rigidbody2D FreezeBehaviour.Start needs).
            if freeze.is_none() {
                let p = self.physicals.iter().position(|x| components.contains(&x.id)).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                self.freeze_add_component(p);
            }
        } else if let Some(f) = freeze {
            self.freeze_queue_destroy(f);
        }
    }

    /// Object.Destroy(FreezeBehaviour): queued like a DestroyObjectDelayed, so OnDestroy runs with the component's destroy
    /// (a second Destroy finds it queued).
    pub(super) fn freeze_queue_destroy(&mut self, f: usize) {
        let id = self.freezes[f].id;
        if !self.destroyed_components.contains(&id) && !self.component_destroys.iter().any(|&(_, i)| i == id) {
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
            self.component_destroys.push((at, id));
        }
    }

    /// The queued Object.Destroy of a FreezeBehaviour (false: `id` is not one): OnDestroy once.
    pub fn freeze_run_destroy(&mut self, id: i32) -> bool {
        let Some(f) = self.freezes.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            self.freeze_destroy(f);
        }
        true
    }

    /// gameObject.AddComponent<FreezeBehaviour>() on a physical object's GameObject: a new component last on the object;
    /// Start runs with the pending Starts.
    /// AddComponent of script `class` (instance id `id`) on the GameObject of component `on`: the kept clone's prefab copy gains it
    /// (file usize::MAX: added at run time) after the object's components, so GetComponents and ObjectState see it in add order.
    /// An object a recording made (no clone kept for it) has no prefab copy to gain it.
    pub(super) fn add_script_to_clone(&mut self, on: i32, class: &str, id: i32) {
        let owns = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&on));
        if !self.unsynced.iter().chain(self.retained.iter()).any(owns) {
            return;
        }
        self.with_component_instance(on, |_, inst, o| {
            let mut prefab = (*inst.prefab).clone();
            let n = prefab.objects[o].components.len() as i64;
            prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, crate::assets::db::Component::Script { class: class.into(), execution_order: 0, enabled: true, fields: crate::assets::mono::Value::Struct(Vec::new()) }));
            inst.prefab = std::sync::Arc::new(prefab);
            inst.objects[o].components.push(id);
        });
    }

    pub(super) fn freeze_add_component(&mut self, p: usize) {
        let id = self.allocate_instance_id();
        self.add_script_to_clone(self.physicals[p].id, "FreezeBehaviour", id);
        self.freezes.push(Freeze { id, enabled: true, physical: p, ..Default::default() });
        self.pending_starts.push(super::misc::PendingStart::Freeze(self.freezes.len() - 1));
    }

    /// FreezeBehaviour.Start: rb = GetComponent<Rigidbody2D>(), its velocities saved, FreezeStackController.RequestFreeze(rb).
    pub fn freeze_start(&mut self, i: usize) {
        let p = self.freezes[i].physical;
        let rb = self.physicals[p].rigidbody.clone();
        self.freezes[i].velocity = rb.velocity(&self.world);
        self.freezes[i].angularVelocity = rb.angular_velocity(&self.world);
        self.freeze_request(p);
        if self.prefs.ShowOutlines && self.physicals[p].HasOutline {
            self.freeze_create_outline_object(i);
        }
    }

    /// FreezeBehaviour.CreateOutlineObject: the outline (outline.rs draws it), then StartCoroutine(Disappear()).
    pub(crate) fn freeze_create_outline_object(&mut self, i: usize) {
        // new GameObject("Outline") (GameObject, Transform), AddComponent<Optout>, AddComponent<SpriteRenderer>: their 4 instance ids.
        for _ in 0..4 {
            self.allocate_instance_id();
        }
        self.freezes[i].outline = true;
        let id = self.freezes[i].id;
        let go = self.physicals[self.freezes[i].physical].id;
        let mut waited = false;
        // FreezeBehaviour.Disappear: WaitForSecondsRealtime(1), then spriteRenderer.enabled = false.
        self.start_coroutine(id, go, Box::new(move |g| {
            if !std::mem::replace(&mut waited, true) {
                return super::coroutine::Yield::WaitForSecondsRealtime(1.0);
            }
            if let Some(f) = g.freezes.iter_mut().find(|f| f.id == id) {
                f.outline = false;
            }
            super::coroutine::Yield::Done
        }));
    }

    /// FreezeBehaviour.OnDestroy: a live rigidbody is released from the freeze stack and gets its saved velocities back.
    pub fn freeze_destroy(&mut self, i: usize) {
        let (p, velocity, angular) = (self.freezes[i].physical, self.freezes[i].velocity, self.freezes[i].angularVelocity);
        // Object.Destroy(selectionOutlineObject).
        self.freezes[i].outline = false;
        // (bool)rb: a destroyed object's Rigidbody2D is gone.
        if self.physicals[p].destroyed {
            return;
        }
        self.freeze_request_unfreeze(p);
        let rb = self.physicals[p].rigidbody.clone();
        rb.set_velocity(&mut self.world, velocity);
        rb.set_angular_velocity(&mut self.world, angular);
    }
}
