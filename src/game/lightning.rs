//! SimpleLightningEmitterBehaviour.Emit: a random walk from the origin that bends toward nearby physical objects, and
//! stops at the first one under it. Its onHit is AcceleratorBoltBehaviour.OnLightningHit, the only one the rewritten
//! classes pass. Start only sets the static Instance. The LineRenderer vertices and the pooled line are presentation.
#![allow(non_snake_case)]

use super::{mathf, Game};
use crate::b2::math::Vec2 as V;

/// SimpleLightningEmitterBehaviour.vertices' length (its static setup: new Vector2[64]); only the line's positions (presentation) use it.
pub(crate) const LIGHTNING_VERTICES: usize = 64;

/// The onHit delegate of a SimpleLightningEmitterBehaviour.Emit call.
#[derive(Clone, Copy)]
pub(super) enum LightningHit {
    Accelerator,
    ReactorArc(usize),
    /// fn-check (biga:emit): a null onHit.
    None,
    /// fn-check (biga:emit): the probe's hook, which keeps the hit in `Game::fn_lightning_hit`.
    Probe,
}

impl Game {
    /// SimpleLightningEmitterBehaviour.Emit (exclude is null; SimpleLightningEmitterBehaviour.Start only sets the static Instance, which Game stands in for). Each iteration: the point query (a hit ends the walk),
    /// Vector2.Lerp toward a Random.insideUnitCircle, then every object GetPhysicsObjectsNearPosition lists on the
    /// layers pulls the walk (a Random.value roll unless it conducts), and the step is clamped to maxStep.
    pub(super) fn lightning_emit(&mut self, origin: [f32; 2], direction: [f32; 2], max_range: f32, attraction: f32, max_iterations: i32, max_step: f32, randomness: f32, layers: u32, hit: LightningHit) {
        let range2 = max_range * max_range;
        let mut vector = origin;
        let n = mathf::normalized2(direction);
        let mut vector2 = [n[0] * max_step, n[1] * max_step];
        for _ in 0..max_iterations {
            if let Some((Some(p), _)) = self.overlap_point_first(vector, layers) {
                if self.physicals[p].in_world {
                    match hit {
                        LightningHit::Accelerator => self.accelerator_lightning_hit(p, vector),
                        LightningHit::ReactorArc(a) => self.reactor_a5_arc_small_hit(a, p, vector),
                        LightningHit::None => {}
                        LightningHit::Probe => self.fn_lightning_hit = Some((p, vector)),
                    }
                    break;
                }
            }
            let r = self.random.inside_unit_circle();
            vector2 = [mathf::lerp(vector2[0], r[0], randomness), mathf::lerp(vector2[1], r[1], randomness)];
            for item in self.physics_objects_near_position(vector, max_range) {
                if layers & (1 << self.physical_layer(item)) == 0 {
                    continue;
                }
                let closest = self.physical_closest_point(item, V::new(vector[0], vector[1]));
                let v3 = [closest[0] - vector[0], closest[1] - vector[1]];
                let sq = mathf::sqr_magnitude(v3);
                if sq > 0.0 && sq <= range2 && (self.properties[self.physicals[item].Properties].Conducting || self.random.value() > 0.1) {
                    let u = mathf::normalized2(v3);
                    vector2 = [vector2[0] + u[0] / sq * attraction, vector2[1] + u[1] / sq * attraction];
                }
            }
            vector2 = super::destroyable::clamp_magnitude(vector2, max_step);
            vector = [vector[0] + vector2[0], vector[1] + vector2[1]];
        }
        // PoolGenerator.Instance.RequestPrefab(PoolableLightningPrefab, origin); the line's positions, width and colours
        // are presentation.
        let prefab = self.simple_lightning_prefab();
        self.pool_request_prefab(prefab, origin);
    }

    /// AcceleratorBoltBehaviour.OnLightningHit: a finite point gains 20 charge and 100 temperature and explodes with force 2.
    pub(crate) fn accelerator_lightning_hit(&mut self, p: usize, point: [f32; 2]) {
        if point[0].is_nan() || point[1].is_nan() {
            return;
        }
        let ph = &mut self.physicals[p];
        ph.charge = ph.Charge() + 20.0;
        ph.Temperature += 100.0;
        self.explosion_creator_explode_at([point[0], point[1], 0.0], 2.0);
    }

    /// PhysicalBehaviour.GetClosestPointTo: the nearest Collider2D.ClosestPoint over its non-trigger colliders, else the
    /// object's position.
    pub(super) fn physical_closest_point(&mut self, p: usize, point: V) -> [f32; 2] {
        self.sync_pending();
        let pos = self.physicals[p].transform.position;
        let mut best = [pos[0], pos[1]];
        let mut num = f32::MAX;
        for c in self.physicals[p].colliders.clone() {
            if !self.collider_alive(c) {
                continue;
            }
            let v = self.world.collider_closest_point(c, point);
            let sq = mathf::sqr_magnitude([v.x - point.x, v.y - point.y]);
            if sq < num {
                num = sq;
                best = [v.x, v.y];
            }
        }
        best
    }
}
