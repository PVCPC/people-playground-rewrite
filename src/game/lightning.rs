//! SimpleLightningEmitterBehaviour.Emit: a random walk from the origin that bends toward nearby physical objects, and
//! stops at the first one under it. Its onHit is AcceleratorBoltBehaviour.OnLightningHit, the only one the rewritten
//! classes pass. Start only sets the static Instance. The walk's vertices go to the pooled clone's LineRenderer
//! (`lightning_line_place`).
#![allow(non_snake_case)]

use super::{mathf, Game};
use crate::b2::math::Vec2 as V;

/// SimpleLightningEmitterBehaviour.vertices' length (its static setup: new Vector3[64]); only the line's positions use it.
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
        // vertices (rendering only): vertices[0] = origin, then the walk spread over the 64 entries.
        let mut vertices = [origin; LIGHTNING_VERTICES];
        let mut i = 0usize;
        for j in 0..max_iterations {
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
            let num2 = ((j as f32 / (max_iterations as f32 / LIGHTNING_VERTICES as f32)).floor() as usize).min(LIGHTNING_VERTICES - 1);
            vertices[num2] = vector;
            let num3 = num2 as i64 - i as i64;
            if num3 > 1 {
                let from = vertices[i];
                for k in i..num2 {
                    vertices[k] = mathf::v2_lerp(from, vector, (k - i) as f32 / (num3 - 1) as f32);
                }
            }
            i = num2;
        }
        for v in &mut vertices[i..] {
            *v = vector;
        }
        // PoolGenerator.Instance.RequestPrefab(PoolableLightningPrefab, origin), then the clone's LineRenderer.
        let prefab = self.simple_lightning_prefab();
        if let Some((s, k)) = self.pool_request_prefab(prefab, origin) {
            let (width, color) = match hit {
                // AcceleratorBoltBehaviour.Update: width 0.04, Color.yellow.
                LightningHit::Accelerator => (0.04, [1.0, 0.921_568_63, 0.015_686_275, 1.0]),
                // ReactorA5ArcBehaviour.GenerateSmallArc: width 0.5, its Color.
                LightningHit::ReactorArc(a) => (0.5, self.reactor_a5_arcs[a].Color),
                LightningHit::None | LightningHit::Probe => (1.0, [1.0; 4]),
            };
            self.lightning_line_place(s, k, &vertices, width, color);
        }
    }

    /// Rendering: Emit's `gameObject.TryGetComponent<LineRenderer>`: SetPositions(vertices), widthMultiplier = width,
    /// startColor and endColor = color (its colorGradient.colorKeys assignment goes to a copy). The clone's line joins
    /// `Game::lines` the first time; ObjectPoolBehaviour.Return's SetActive(false) hides it (refresh_renderers).
    fn lightning_line_place(&mut self, s: usize, k: usize, vertices: &[[f32; 2]], width: f32, color: [f32; 4]) {
        use crate::assets::db::Component;
        let root = self.generic_pools[s].items[k].root;
        let found = self.with_component_instance(root, |_, inst, _| {
            let c = inst.prefab.objects[0].components.iter().position(|(_, x)| matches!(x, Component::Other(120)))?;
            Some((inst.objects[0].components[c], inst.prefab.objects[0].components[c].0, inst.prefab.objects[0].layer as i32))
        });
        let Some((id, data_ref, layer)) = found else { return };
        let l = match self.lines.iter().position(|x| x.id == id) {
            Some(l) => l,
            None => {
                let db = self.assets.take().expect("the game's data");
                let data = crate::assets::components::LineRendererData::read(db.data(data_ref)).expect("the lightning prefab's LineRenderer");
                let line = Self::line_visual(&db, data_ref.file, data, id, super::BareAnchor::Root, Vec::new(), layer, true);
                self.assets = Some(db);
                self.lines.push(line);
                self.lines.len() - 1
            }
        };
        let line = &mut self.lines[l];
        line.visible = true;
        line.positions = vertices.iter().map(|v| [v[0], v[1], 0.0]).collect();
        line.width = width;
        line.gradient.set_end_color(color);
        line.gradient.set_start_color(color);
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
