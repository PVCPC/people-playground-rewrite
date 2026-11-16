//! BlackHoleBehaviour (what a charged Decimator turns into): every FixedUpdate it shakes the camera, pulls each rigidbody
//! within EffectRange towards it and destroys the root of each deletable non-trigger physical within DeletionRange.
#![allow(non_snake_case)]

use super::{d, script, Game};
use crate::b2::math::Vec2 as V;

script! {
    BlackHole {
        AttractionForce: f32 = 150.0,
        EffectRange: f32 = 4.0,
        DeletionRange: f32 = 4.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// The GameObject's layer (Awake reads Physics2D.GetLayerCollisionMask of it).
        layer: usize = 0,
        /// The GameObject's own Collider2Ds (`collider2D.transform == base.transform` skips them).
        colliders: Vec<i32> = Vec::new(),
        /// Awake's Physics2D.GetLayerCollisionMask.
        mask: u32 = 0,
    }
}

impl Game {
    /// BlackHoleBehaviour.Awake: mask = Physics2D.GetLayerCollisionMask(layer): the matrix row (all layers when the world has no matrix).
    pub fn black_hole_awake(&mut self, i: usize) {
        let layer = self.black_holes[i].layer;
        self.black_holes[i].mask = self.world.layer_collision_matrix.get(layer).copied().unwrap_or(!0);
    }

    /// BlackHoleBehaviour.FixedUpdate
    pub fn black_hole_fixed_update(&mut self, i: usize) {
        let s = &self.black_holes[i];
        let (id, force, range, deletion) = (s.id, s.AttractionForce, s.EffectRange, s.DeletionRange);
        let own = s.colliders.clone();
        let mask = s.mask;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), range, mask);
        // The 256-entry buffer.
        hits.truncate(BLACK_HOLE_BUFFER);
        self.camera_shake_at(0.5, [position[0], position[1]], 0.5);
        for (_, collider) in hits {
            if own.contains(&collider) {
                continue;
            }
            let Some(body) = self.world.collider_body(collider) else { continue };
            let Some(rb) = self.rigidbody_of_body(body) else { continue };
            let at = self.collider_position(collider);
            let v = [position[0] - at[0], position[1] - at[1], position[2] - at[2]];
            // Vector3.magnitude: (float)Math.Sqrt(x * x + y * y + z * z), in double.
            let magnitude = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).0.sqrt() as f32;
            let dir = [v[0] / magnitude, v[1] / magnitude];
            let k = (d(force) * rb.mass(&self.world)).f();
            rb.add_force(&mut self.world, [k * dir[0], k * dir[1]]);
            if !(magnitude > deletion) && self.world.collider_is_trigger(collider) == Some(false) {
                // PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform), then Deletable.
                let Some(p) = self.physical_of_collider(collider) else { continue };
                if self.physicals[p].Deletable {
                    let root = self.root_id(p);
                    self.destroy_object(root);
                }
            }
        }
    }
}

/// BlackHoleBehaviour.buffer: static Collider2D[256] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const BLACK_HOLE_BUFFER: usize = 256;
