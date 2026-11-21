//! SoapBehaviour: a bar of soap that wipes the decals of whatever it touches. A collision wipes at once and records the
//! position; a resting contact wipes again once the soap has moved more than 1 unit. Wiping a decal fades its sprite
//! by CleanPower, and a decal that is already fainter than that is removed.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};
use crate::b2::contacts2d::ScriptCollision;

script! {
    Soap {
        CleanPower: f32 = 0.1,
        RadiusSqrd: f32 = 5.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GetComponent<PhysicalBehaviour>(): the object's collision messages reach it.
        physical: Option<usize> = None,
        /// The component's index on its GameObject, and whether it comes after the PhysicalBehaviour (`side` ordering).
        index: usize = 0,
        after_physical: bool = false,
        /// lastPosition: None until the first collision.
        lastPosition: Option<[f32; 2]> = None,
    }
}

impl Game {
    /// base.transform.position of the soap.
    pub(crate) fn soap_position(&mut self, s: usize) -> [f32; 3] {
        self.with_component_instance(self.soaps[s].id, |_, inst, o| inst.position(o))
    }

    /// SoapBehaviour.OnCollisionEnter2D: lastPosition = transform.position (the Vector2 conversion drops z), then Clean.
    pub fn soap_on_collision_enter(&mut self, s: usize, c: &ScriptCollision) {
        let p = self.soap_position(s);
        self.soaps[s].lastPosition = Some([p[0], p[1]]);
        self.soap_clean_collision(s, c);
    }

    /// SoapBehaviour.OnCollisionStay2D: Clean when no position is recorded yet, or when the soap has moved more than 1
    /// unit since the last clean (then the position is recorded).
    pub fn soap_on_collision_stay(&mut self, s: usize, c: &ScriptCollision) {
        match self.soaps[s].lastPosition {
            Some(last) => {
                let p = self.soap_position(s);
                let here = [p[0], p[1]];
                // Vector2.Distance(a, b) = (a - b).magnitude
                if mathf::magnitude2([here[0] - last[0], here[1] - last[1]]) > 1.0 {
                    self.soap_clean_collision(s, c);
                    self.soaps[s].lastPosition = Some(here);
                }
            }
            None => self.soap_clean_collision(s, c),
        }
    }

    /// Clean(collision.transform): the hit collider's physical (a bare collider's anchor physical) as the transform.
    fn soap_clean_collision(&mut self, s: usize, c: &ScriptCollision) {
        if let Some(hit) = self.soap_hit_transform(c) {
            self.soap_clean(s, hit);
        }
    }

    /// collision.transform as an object key (its Transform component id): the hit collider's physical's object, or a bare
    /// collider's anchor physical's. Controllers sit on physical objects only; a bare collider's chain holds none below
    /// its anchor physical.
    fn soap_hit_transform(&mut self, c: &ScriptCollision) -> Option<i32> {
        // A DecalControllerBehaviour on a non-physical object above the hit collider (a bare collider's chain up to its
        // anchor, or past it): the search over those objects is not rewritten.
        if self.decal_chain_colliders.contains(&c.collider) {
            unimplemented!("mod-only: SoapBehaviour.Clean: a DecalControllerBehaviour on a non-physical object above the hit collider (GetComponentsInParent)");
        }
        let start = match self.physical_of_collider(c.collider) {
            Some(p) => p,
            None => match self.bare_colliders.get(&c.collider).map(|b| b.anchor) {
                Some(super::BareAnchor::Physical(q)) => q,
                _ => return None,
            },
        };
        Some(self.with_component_instance(self.physicals[start].id, |_, inst, o| inst.objects[o].components[0]))
    }

    /// The physical objects that GetComponentsInParent(includeInactive: false) on `hit` (an object key) searches (the
    /// object first, then its parents), of those with DecalControllerBehaviours.
    pub(crate) fn soap_decal_physicals(&mut self, hit: i32) -> Vec<usize> {
        // A pooled effect (BleedingParticle, a VFX) is on no clone and holds no DecalControllerBehaviour.
        let on_clone = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&hit));
        if !self.unsynced.iter().chain(self.retained.iter()).any(on_clone) {
            return Vec::new();
        }
        // The object keys (Transform component ids) of the hit object and its active ancestors, nearest first.
        let chain: Vec<i32> = self.with_component_instance(hit, |_, inst, o| {
            let mut keys = Vec::new();
            let mut at = Some(o);
            while let Some(x) = at {
                if inst.objects[x].active {
                    keys.push(inst.objects[x].components[0]);
                }
                at = inst.objects[x].parent;
            }
            keys
        });
        let mut found = Vec::new();
        for key in chain {
            for q in 0..self.physicals.len() {
                if self.physicals[q].destroyed || self.physicals[q].decal_controllers.is_empty() {
                    continue;
                }
                let qk = self.with_component_instance(self.physicals[q].id, |_, inst, o| inst.objects[o].components[0]);
                if qk == key && !found.contains(&q) {
                    found.push(q);
                }
            }
        }
        found
    }

    /// SoapBehaviour.Clean(hit): every DecalControllerBehaviour on the hit transform and its parents. A decal within
    /// RadiusSqrd of the soap is removed when its sprite is already fainter than CleanPower, else faded by it.
    pub(crate) fn soap_clean(&mut self, s: usize, hit: i32) {
        let position = self.soap_position(s);
        let (power, radius) = (self.soaps[s].CleanPower, self.soaps[s].RadiusSqrd);
        for q in self.soap_decal_physicals(hit) {
            for k in 0..self.physicals[q].decal_controllers.len() {
                // Each child of the holder is visited in order; a removal does not reach the ones after it.
                let visit = self.physicals[q].decal_controllers[k].decals.clone();
                for decal in visit {
                    let world = self.transform_point3(q, [decal.position[0], decal.position[1], 0.0]);
                    let v = [world[0] - position[0], world[1] - position[1], world[2] - position[2]];
                    let sqr = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).f();
                    if sqr >= radius {
                        continue;
                    }
                    let decals = &mut self.physicals[q].decal_controllers[k].decals;
                    let Some(at) = decals.iter().position(|x| x.position == decal.position) else { continue };
                    if decal.colour[3] < power {
                        // localDecalPositions.Remove(item.localPosition): the first equal entry goes.
                        decals.remove(at);
                    } else {
                        decals[at].colour[3] = mathf::max(0.0, decal.colour[3] - power);
                    }
                }
            }
        }
    }
}
