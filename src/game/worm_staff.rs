//! WormStaffBehaviour (the Worm Staff). Start lays each worm out on a circle of Range around its LineRenderer; Update
//! drags the worm ends with the staff's point velocity, sends each worm at a random collider in Range (a limb is
//! pulled, sometimes damaged, stabbed and cut; another Rigidbody2D is only pulled) or back to its rest position. Each
//! worm's LineRenderer gets VertexCount vertices from the staff to the worm's end, wriggled by Perlin noise.
#![allow(non_snake_case)]

use super::{mathf, script, Game};
use crate::b2::math::Vec2 as V;
use crate::unity::transform::Trs;

/// Worm: velocity, endPosition, originalEndpos and seed, and the LineRenderer's local TRS steps below the staff (leaf
/// first) for lineRenderer.transform.TransformPoint (the line itself is `WormStaff::lines`).
#[derive(Clone, Debug, Default)]
pub struct Worm {
    pub velocity: [f32; 2],
    pub endPosition: [f32; 2],
    pub originalEndpos: [f32; 2],
    pub seed: f32,
    pub chain: Vec<Trs>,
}

impl Worm {
    /// Worm..ctor(lineRenderer, seed)
    fn new(chain: Vec<Trs>, seed: f32) -> Self {
        Worm { seed, chain, ..Default::default() }
    }
}

script! {
    WormStaff {
        Range: f32 = 0.0,
        Damage: f32 = 0.0,
        CutChance: f32 = 0.0,
        Center: [f32; 3] = [0.0; 3],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The staff's PhysicalBehaviour; Rigidbody is its Rigidbody2D.
        physical: usize = 0,
        /// Layer.m_Bits
        Layer: u32 = 0,
        /// Each LineRenderer's local TRS steps below the staff, leaf first.
        LineRenderers: Vec<Vec<Trs>> = Vec::new(),
        worms: Vec<Worm> = Vec::new(),
        /// Rendering: VertexCount and each LineRenderer's index in `Game::lines`.
        VertexCount: i32 = 4,
        lines: Vec<usize> = Vec::new(),
    }
}

impl Game {
    /// WormStaffBehaviour.Start
    pub fn worm_staff_start(&mut self, i: usize) {
        let chains = self.worm_staffs[i].LineRenderers.clone();
        let mut worms: Vec<Worm> = chains.into_iter().map(|chain| Worm::new(chain, self.random.value() * 1000.0)).collect();
        let (range, n) = (self.worm_staffs[i].Range, worms.len());
        let mut num = 0.0f32;
        for worm in &mut worms {
            let vector = [mathf::cos(num) * range, mathf::sin(num) * range];
            worm.endPosition = vector;
            worm.originalEndpos = vector;
            num += std::f32::consts::PI * 2.0 / n as f32;
        }
        self.worm_staffs[i].worms = worms;
        // positionCount = VertexCount; SetPosition(k, Vector3.Lerp(zero, vector, k / VertexCount)).
        let count = self.worm_staffs[i].VertexCount.max(0) as usize;
        for w in 0..n {
            let end = self.worm_staffs[i].worms[w].endPosition;
            let l = self.worm_staffs[i].lines[w];
            self.lines[l].positions = (0..count).map(|k| {
                let t = k as f32 / count as f32;
                [mathf::lerp(0.0, end[0], t), mathf::lerp(0.0, end[1], t), 0.0]
            }).collect();
        }
    }

    /// The end of WormStaffBehaviour.Update's worm loop: vertices[j] = Lerp(zero, endPosition, j / VertexCount)
    /// + 0.2 * j / VertexCount * GetPerlin2Mapped(Time.time * 1.2 - j * 0.1, seed), then SetPositions.
    fn worm_staff_set_vertices(&mut self, i: usize, w: usize) {
        let num = self.time.time * 1.2;
        let count = self.worm_staffs[i].VertexCount.max(0) as usize;
        let (end, seed) = (self.worm_staffs[i].worms[w].endPosition, self.worm_staffs[i].worms[w].seed);
        let l = self.worm_staffs[i].lines[w];
        self.lines[l].positions = (0..count).map(|j| {
            let t = j as f32 / count as f32;
            let p = mathf::perlin2_mapped(num - j as f32 * 0.1, seed);
            [mathf::lerp(0.0, end[0], t) + 0.2 * t * p[0], mathf::lerp(0.0, end[1], t) + 0.2 * t * p[1], 0.0]
        }).collect();
    }

    /// WormStaffBehaviour.Update
    pub fn worm_staff_update(&mut self, i: usize) {
        let num2 = self.time.deltaTime * 60.0;
        let num3 = mathf::pow(0.4, num2);
        let p = self.worm_staffs[i].physical;
        let c = self.worm_staffs[i].Center;
        let point = self.transform_point(p, [c[0], c[1]]);
        // Rigidbody.GetPointVelocity runs AutoSyncTransforms.
        self.sync_pending();
        let point_velocity = self.physicals[p].rigidbody.point_velocity(&self.world, point);
        let (range, layer) = (self.worm_staffs[i].Range, self.worm_staffs[i].Layer);
        // OverlapCircleNonAlloc into collisionBuffer (LineRenderers.Length + 1 entries).
        let mut hits = self.overlap_circle(V::new(point[0], point[1]), range, layer);
        hits.truncate(self.worm_staffs[i].LineRenderers.len() + 1);
        for w in 0..self.worm_staffs[i].worms.len() {
            let worm = &mut self.worm_staffs[i].worms[w];
            worm.velocity = [worm.velocity[0] + point_velocity[0] * -0.5, worm.velocity[1] + point_velocity[1] * -0.5];
            if !self.worm_staff_act_on_target(i, w, &hits) {
                let worm = &mut self.worm_staffs[i].worms[w];
                worm.velocity = [worm.velocity[0] + (worm.originalEndpos[0] - worm.endPosition[0]), worm.velocity[1] + (worm.originalEndpos[1] - worm.endPosition[1])];
            }
            let worm = &mut self.worm_staffs[i].worms[w];
            worm.velocity = [worm.velocity[0] * num3, worm.velocity[1] * num3];
            let v = [0.01 * num2 * worm.velocity[0], 0.01 * num2 * worm.velocity[1], 0.0];
            let local = self.inverse_transform_vector(p, v);
            let worm = &mut self.worm_staffs[i].worms[w];
            worm.endPosition = [worm.endPosition[0] + local[0], worm.endPosition[1] + local[1]];
            worm.endPosition = super::destroyable::clamp_magnitude(worm.endPosition, range * 1.5);
            self.worm_staff_set_vertices(i, w);
        }
    }

    /// WormStaffBehaviour.ActOnTarget
    fn worm_staff_act_on_target(&mut self, i: usize, w: usize, hits: &[(Option<usize>, i32)]) -> bool {
        if hits.len() <= 1 {
            return false;
        }
        let (physical, target) = hits[self.pick_random(hits.len()).unwrap()];
        let p = self.worm_staffs[i].physical;
        let go = self.world.collider_game_object(target);
        if go == Some(self.physical_game_object(p)) {
            return false;
        }
        // worm.lineRenderer.transform.TransformPoint(worm.endPosition)
        let end = self.worm_staffs[i].worms[w].endPosition;
        let mut v = [end[0], end[1], 0.0, 0.0];
        for node in &self.worm_staffs[i].worms[w].chain {
            v = crate::unity::transform::global_position(node, v);
        }
        let world = self.transform_point3(p, [v[0], v[1], v[2]]);
        let world_end = [world[0], world[1]];
        // target.TryGetComponent<LimbBehaviour>: the collider's own GameObject.
        let limb = physical.and_then(|q| self.limbs.iter().position(|x| x.PhysicalBehaviour == q));
        if let Some(l) = limb.filter(|&l| go == Some(self.physical_game_object(self.limbs[l].PhysicalBehaviour))) {
            let q = self.limbs[l].PhysicalBehaviour;
            self.worm_staff_move_to_target(i, w, q, target, world_end);
            if crate::unity::perlin_noise(self.worm_staffs[i].worms[w].seed, self.time.time * 15.0) < self.worm_staffs[i].CutChance {
                self.limb_damage(l, self.worm_staffs[i].Damage);
                let c = &mut self.circulations[l];
                c.StabWoundCount = c.StabWoundCount.wrapping_add(1);
                self.skin_add_damage_point(l, super::misc::damage_type::STAB, world_end, 6.0);
                let direction = self.random.inside_unit_circle();
                self.cut(l, world_end, direction);
            }
            return true;
        }
        // target.TryGetComponent<Rigidbody2D>: a Rigidbody2D on the collider's own GameObject.
        if let Some(q) = physical.filter(|&q| go == Some(self.physical_game_object(q))) {
            self.worm_staff_move_to_target(i, w, q, target, world_end);
            return true;
        }
        if physical.is_none() {
            // target.TryGetComponent<Rigidbody2D>: a bare Rigidbody2D on the collider's own GameObject.
            let node = self.with_component_instance(target, |g, inst, o| g.node_rigidbodies.iter().position(|n| inst.objects[o].components.contains(&n.rigidbody.component)));
            if let Some(k) = node {
                self.worm_staff_move_to_bare_target(i, w, k, target, world_end);
                return true;
            }
        }
        false
    }

    /// moveToTargetAndTarget(rb) for a bare Rigidbody2D (`node_rigidbodies[k]`) on the target collider's object.
    fn worm_staff_move_to_bare_target(&mut self, i: usize, w: usize, k: usize, target: i32, world_end: [f32; 2]) {
        self.sync_pending();
        let body = self.world.collider_body(target).expect("a target collider has fixtures");
        let position = self.bare_position(target);
        let (center, ext) = self.collider_bounds_on(body, target, position);
        let (c, _) = crate::unity::transform::bounds_closest_point([world_end[0], world_end[1], 0.0], center, ext);
        let worm = &mut self.worm_staffs[i].worms[w];
        worm.velocity = [worm.velocity[0] + (c[0] - world_end[0]), worm.velocity[1] + (c[1] - world_end[1])];
        let force = self.random.inside_unit_circle();
        let rb = self.node_rigidbodies[k].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, force, [c[0], c[1]]);
    }

    /// moveToTargetAndTarget(rb): the worm heads for the closest point of the target's bounds and rb is pushed there.
    fn worm_staff_move_to_target(&mut self, i: usize, w: usize, q: usize, target: i32, world_end: [f32; 2]) {
        // Collider2D.bounds runs AutoSyncTransforms.
        self.sync_pending();
        let owner = self.physical_of_collider(target).expect("a target collider with a PhysicalBehaviour");
        let (center, ext) = self.collider_bounds(owner, target);
        let (c, _) = crate::unity::transform::bounds_closest_point([world_end[0], world_end[1], 0.0], center, ext);
        let worm = &mut self.worm_staffs[i].worms[w];
        worm.velocity = [worm.velocity[0] + (c[0] - world_end[0]), worm.velocity[1] + (c[1] - world_end[1])];
        let force = self.random.inside_unit_circle();
        let rb = self.physicals[q].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, force, [c[0], c[1]]);
    }

    /// A physical object's GameObject id.
    pub(super) fn physical_game_object(&mut self, p: usize) -> i32 {
        self.with_component_instance(self.physicals[p].id, |_, inst, o| inst.objects[o].game_object)
    }

    /// Transform.InverseTransformVector (1807f27e0) of a physical object, from the root down.
    pub fn inverse_transform_vector(&self, p: usize, v: [f32; 3]) -> [f32; 3] {
        let mut v = [v[0], v[1], v[2], 0.0];
        for node in &self.local_chain(p) {
            v = crate::unity::transform::inverse_vector(node, v);
        }
        [v[0], v[1], v[2]]
    }
}
