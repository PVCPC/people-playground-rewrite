//! TorchBehaviour: a flame that ignites the physical objects it touches and heats each one toward TouchTemperature every
//! fixed step while it is touched. OnEnable registers the contacts its colliders already have; OnCollisionEnter2D and
//! OnCollisionExit2D add and remove objects while enabled. The flame's intensity (OnInsideVacuum lowers it) sets its
//! particles' rates; it and the sound are presentation: nothing in the simulation reads them.
#![allow(non_snake_case)]

use super::{mathf, script, Game};
use crate::assets::db::Component;
use crate::b2::contacts2d::ScriptCollision;

script! {
    Torch {
        TouchTemperature: f32 = 400.0,
        TransferSpeed: f32 = 0.01,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>() of the object, when it has one (its collision messages then go through `side`).
        physical: Option<usize> = None,
        /// The Collider2D instance ids on the torch's own GameObject: their collision messages reach the torch directly
        /// when it has no PhysicalBehaviour (the physical's messages go to the rigidbody's object instead).
        colliders: Vec<i32> = Vec::new(),
        /// The component's index on its GameObject, and whether it comes after the PhysicalBehaviour (`side` ordering).
        index: usize = 0,
        after_physical: bool = false,
        /// currentlyColliding: the physical objects touched, in the order they were added (a set in C#).
        currentlyColliding: Vec<usize> = Vec::new(),
        /// torchIntensity: the flames' rate (presentation).
        torchIntensity: f32 = 1.0,
    }
}

impl Game {
    /// TorchBehaviour.Register(collider, point): an object on the collider's transform is added and ignited.
    pub(crate) fn torch_register(&mut self, t: usize, collider: i32, point: [f32; 2]) {
        if let Some(p) = self.physical_of_collider(collider) {
            if !self.torches[t].currentlyColliding.contains(&p) {
                self.torches[t].currentlyColliding.push(p);
            }
            self.ignite_at(p, true, point);
        }
    }

    /// GetComponentsInChildren<Collider2D>() below object `o`, including its own: the active objects' colliders, in
    /// hierarchy order.
    pub fn torch_child_colliders(inst: &super::spawn::Instance, o: usize) -> Vec<i32> {
        let mut out = Vec::new();
        for x in std::iter::once(o).chain(inst.descendants(o)) {
            if !inst.objects[x].active {
                continue;
            }
            for (k, (_, comp)) in inst.prefab.objects[x].components.iter().enumerate() {
                if matches!(comp, Component::Collider2D(_)) {
                    out.push(inst.objects[x].components[k]);
                }
            }
        }
        out
    }

    /// TorchBehaviour.OnEnable on a running torch.
    pub fn torch_on_enable(&mut self, t: usize) {
        let id = self.torches[t].id;
        let colliders = self.with_component_instance(id, |_, inst, o| Self::torch_child_colliders(inst, o));
        self.torch_enable_contacts(t, colliders);
    }

    /// TorchBehaviour.OnEnable's contact registration over `colliders` (GetComponentsInChildren order). Each collider's
    /// GetContacts (PhysicsQuery2D::GetContacts, 180bee5c0) walks PhysicsContacts2D.m_Collisions in array order, takes the
    /// non-trigger records of the collider whose state is not Invalid, and appends their manifold points; the array holds four.
    pub fn torch_enable_contacts(&mut self, t: usize, colliders: Vec<i32>) {
        for collider in colliders {
            let records: Vec<ScriptCollision> = {
                let c2d = &self.world.contacts2d;
                let mut found = Vec::new();
                for &rid in &c2d.collisions {
                    if let Some(r) = c2d.records[rid].as_ref() {
                        if !r.is_trigger && r.state != crate::b2::contacts2d::ContactState::Invalid && (r.collider_a.id == collider || r.collider_b.id == collider) {
                            found.push(self.world.script_collision(r, collider));
                        }
                    }
                }
                found
            };
            for contact in records.iter().flat_map(|r| r.contacts.iter()).take(4) {
                self.torch_register(t, contact.collider, [contact.point.x, contact.point.y]);
            }
        }
    }

    /// TorchBehaviour.OnCollisionEnter2D: while enabled, a collision with a rigidbody registers the other collider at
    /// its first contact point.
    pub fn torch_on_collision_enter(&mut self, t: usize, c: &ScriptCollision) {
        if self.torches[t].enabled && c.rigidbody.is_some() {
            let point = c.contacts.first().unwrap_or_else(|| super::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.")).point;
            self.torch_register(t, c.collider, [point.x, point.y]);
        }
    }

    /// SendMessage of OnCollisionEnter2D or OnCollisionExit2D to a collider's own GameObject, for a torch that has no
    /// PhysicalBehaviour (`physical` None: the physical path in `side` does not reach it). Other messages do nothing here.
    pub fn torch_collision_on_collider(&mut self, collider: i32, message: crate::b2::contacts2d::Message, c: &ScriptCollision) {
        use crate::b2::contacts2d::Message;
        for t in 0..self.torches.len() {
            if self.torches[t].physical.is_none() && self.torches[t].colliders.contains(&collider) {
                match message {
                    Message::CollisionEnter => self.torch_on_collision_enter(t, c),
                    Message::CollisionExit => self.torch_on_collision_exit(t, c),
                    _ => {}
                }
            }
        }
    }

    /// TorchBehaviour.OnCollisionExit2D: while enabled, a collision with a rigidbody removes the other object.
    pub fn torch_on_collision_exit(&mut self, t: usize, c: &ScriptCollision) {
        if self.torches[t].enabled && c.rigidbody.is_some() {
            if let Some(p) = self.physical_of_collider(c.collider) {
                self.torches[t].currentlyColliding.retain(|&x| x != p);
            }
        }
    }

    /// TorchBehaviour.OnInsideVacuum: torchIntensity -= fixedDeltaTime * 2 above -1 (the flame particles' rate and the fire sound).
    pub fn torch_on_inside_vacuum(&mut self, t: usize) {
        let dt = self.time.fixedDeltaTime;
        let s = &mut self.torches[t];
        if s.torchIntensity > -1.0 {
            s.torchIntensity -= dt * 2.0;
        }
    }

    /// TorchBehaviour.FixedUpdate: destroyed objects leave the set, and each remaining one's temperature moves toward
    /// TouchTemperature by TransferSpeed (Mathf.Lerp). Then torchIntensity recovers toward 1 and each FlameParticles
    /// system's rateOverTimeMultiplier is torchIntensity * its Multiplier (the fire sound's intensity is not kept).
    pub fn torch_fixed_update(&mut self, t: usize) {
        self.torches[t].currentlyColliding.retain(|&p| !self.physicals[p].destroyed);
        let (touch, speed) = (self.torches[t].TouchTemperature, self.torches[t].TransferSpeed);
        for i in 0..self.torches[t].currentlyColliding.len() {
            let p = self.torches[t].currentlyColliding[i];
            self.physicals[p].Temperature = mathf::lerp(self.physicals[p].Temperature, touch, speed);
        }
        let dt = self.time.fixedDeltaTime;
        let s = &mut self.torches[t];
        s.torchIntensity = if s.torchIntensity < 1.0 { s.torchIntensity + dt } else { 1.0 };
        let (id, k) = (s.id, s.torchIntensity);
        for n in 0..self.field_len(id, "FlameParticles") {
            let m = self.field_f32(id, &format!("FlameParticles[{n}].Multiplier")).unwrap_or(0.0);
            self.ps_field_writes(id, &format!("FlameParticles[{n}].System"), crate::particles::PsWrites { rate: Some(k * m), ..Default::default() });
        }
    }

    /// TorchBehaviour.enabled = true: an enabled component on an active object runs OnEnable, and its FixedUpdate is
    /// scheduled from then on (the behaviour's execution order).
    pub fn torch_set_enabled(&mut self, t: usize) {
        if self.torches[t].enabled {
            return;
        }
        self.torches[t].enabled = true;
        let id = self.torches[t].id;
        let active = self.with_component_instance(id, |_, inst, o| inst.objects[o].active);
        if active {
            if let Some(&(order, b)) = self.behaviour_of.get(&id) {
                self.behaviours.add(order, b);
            }
            self.torch_on_enable(t);
        }
    }
}

/// ParticleSystemRateMutliplier: a flame ParticleSystem (its instance id, None when null) and its rate multiplier.
#[derive(Default, Clone, Copy, Debug)]
pub struct ParticleSystemRateMutliplier {
    pub System: Option<i32>,
    pub Multiplier: f32,
}

impl ParticleSystemRateMutliplier {
    /// ParticleSystemRateMutliplier..ctor
    #[allow(dead_code)]
    pub fn new(item1: Option<i32>, item2: f32) -> Self {
        ParticleSystemRateMutliplier { System: item1, Multiplier: item2 }
    }

    /// ParticleSystemRateMutliplier.Equals: the same System (Unity Object equality) and a == on Multiplier.
    #[allow(dead_code)]
    pub fn equals(&self, obj: &ParticleSystemRateMutliplier) -> bool {
        self.System == obj.System && self.Multiplier == obj.Multiplier
    }

    /// ParticleSystemRateMutliplier.GetHashCode: (unchecked(-1030903623 * -1521134295) + System hash) * -1521134295 + Multiplier hash
    /// (a null System hashes to 0, float.GetHashCode is the bits, -0 as 0). UnityEngine.Object.GetHashCode is
    /// base.GetHashCode(), Mono's per-object header hash, which no rewrite can reproduce: the instance id stands in (nothing calls this).
    #[allow(dead_code)]
    pub fn get_hash_code(&self) -> i32 {
        (-1030903623i32.wrapping_mul(-1521134295).wrapping_add(self.System.unwrap_or(0))).wrapping_mul(-1521134295).wrapping_add(super::decal_renderer::float_hash(self.Multiplier))
    }

    /// ParticleSystemRateMutliplier.op_Implicit: (ParticleSystem, float) <-> the struct.
    #[allow(dead_code)]
    pub fn op_Implicit((system, multiplier): (Option<i32>, f32)) -> Self {
        Self::new(system, multiplier)
    }
}

#[cfg(test)]
mod rate_multiplier_tests {
    use super::ParticleSystemRateMutliplier as R;

    #[test]
    fn equality_and_hash() {
        let a = R::op_Implicit((Some(7), 2.0));
        let b = R::new(Some(7), 2.0);
        assert!(a.equals(&b) && a.get_hash_code() == b.get_hash_code());
        assert!(!a.equals(&R::new(Some(8), 2.0)) && !a.equals(&R::new(Some(7), 3.0)));
        let h = (-1030903623i32.wrapping_mul(-1521134295)).wrapping_mul(-1521134295);
        assert_eq!(R::new(None, 0.0).get_hash_code(), h);
    }
}
