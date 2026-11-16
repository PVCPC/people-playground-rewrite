//! FlamethrowerBehaviour (the flamethrower and the fire extinguisher). The particle's pose and the context menu are
//! presentation; the continuous-use timer, the particle request, the recoil, the overlap queries and their burn or
//! extinguish effects stay, and so does AffectAoe (the DestroyableBehaviour.OnBreak call).
#![allow(non_snake_case)]

use super::explosion::DEFAULT_RAYCAST_LAYERS;
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::unity::transform::{global_position, global_rotation, Trs};

script! {
    Flamethrower {
        /// SprayEffect: Ignite 0, Extinguish 1
        Effect: i32 = 0,
        Point: [f32; 2] = [0.0, 0.0],
        RayCount: i32 = 16,
        Range: f32 = 5.0,
        Angle: f32 = 30.0,
        AoeRange: f32 = 3.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>()
        phys: usize = 0,
        /// particlePrefab
        particlePrefab: Option<ObjRef> = None,
        /// Muzzle: the local TRS of its object and of each parent below the flamethrower's own object.
        Muzzle: Vec<Trs> = Vec::new(),
        /// LayerMask bits
        LayerMask: u32 = 0,
        time: f32 = 0.0,
    }
}

impl Game {
    /// Muzzle's position and rotation: its local TRS up to the flamethrower's object, then the object's chain.
    fn flamethrower_muzzle_pose(&self, i: usize) -> ([f32; 3], [f32; 4]) {
        let s = &self.flamethrowers[i];
        let nodes: Vec<Trs> = s.Muzzle.iter().copied().chain(self.local_chain(s.phys).iter().rev().copied()).collect();
        let (mut v, mut r) = ([nodes[0].t[0], nodes[0].t[1], nodes[0].t[2], 0.0], nodes[0].q);
        for n in &nodes[1..] {
            (v, r) = (global_position(n, v), global_rotation(n, r));
        }
        ([v[0], v[1], v[2]], r)
    }

    /// FlamethrowerBehaviour.AffectAoe of component `id`: every physical in the chunks around the object is lit (by its
    /// flammability) or put out. The chunk iterator is lazy, so each item's draws come before the next cell's.
    pub fn flamethrower_affect_aoe(&mut self, id: i32) {
        let i = self.flamethrowers.iter().position(|x| x.id == id).expect("a FlamethrowerBehaviour");
        let (effect, range, p) = (self.flamethrowers[i].Effect, self.flamethrowers[i].AoeRange, self.flamethrowers[i].phys);
        let at = self.physicals[p].transform.position;
        self.physics_objects_near_position_accurate_each([at[0], at[1]], range, |g, item| {
            if g.physicals[item].destroyed {
                return;
            }
            if effect == 0 {
                if g.properties[g.physicals[item].Properties].Flammability >= g.random.value() {
                    g.physicals[item].burnIntensity = g.random.value();
                    g.ignite(item, false);
                }
            } else {
                g.physicals[item].burnIntensity = 0.0;
                g.extinguish(item);
            }
        });
    }

    /// FlamethrowerBehaviour.Update
    pub fn flamethrower_update(&mut self, i: usize) {
        let p = self.flamethrowers[i].phys;
        let dt = self.time.deltaTime;
        self.flamethrowers[i].time += dt;
        if self.started_used_continuously(p) {
            // IgnitionSource.PlayOneShot(IgnitionSound); Play(); time = Random.value * clip.length
            let (id, src) = (self.flamethrowers[i].id, self.flamethrower_source(i));
            let sound = self.audio.clip(id, "IgnitionSound");
            self.audio.play_one_shot(src, sound.as_ref(), 1.0);
            self.audio.play(src);
            let v = self.random.value();
            match self.audio.get(src).clip.clone() {
                Some(clip) => self.audio.set_time(src, (d(v) * clip.length()).f()),
                // A replay has no game assets (no clips); the game's IgnitionSource always has one.
                None if self.assets.is_some() => crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"),
                None => {}
            }
            self.flamethrowers[i].time = 0.0;
        }
        if self.used_continuously(p) && self.physicals[p].Wetness <= 0.0 {
            self.flamethrowers[i].time += dt;
            if self.flamethrowers[i].time > 0.02 {
                self.flamethrowers[i].time = 0.0;
                let effect = self.flamethrowers[i].particlePrefab.expect("FlamethrowerBehaviour: RequestPrefab(null)");
                let (muzzle, muzzle_rotation) = self.flamethrower_muzzle_pose(i);
                let scale = self.physicals[p].transform.localScale[0];
                let right = mathf::rotate(muzzle_rotation, [1.0, 0.0, 0.0]);
                // transform.right = Muzzle.right * localScale.x; localScale = (|x|, y, 1): the flames leave the muzzle the way it faces.
                let z = mathf::atan2_f64((right[1] * scale) as f64, (right[0] * scale) as f64).to_degrees() as f32;
                self.request_pooled_effect(effect, "FlamethrowerBehaviour.particlePrefab", [muzzle[0], muzzle[1]], z, scale.abs());
                let mine = mathf::rotate(self.physicals[p].transform.rotation, [1.0, 0.0, 0.0]);
                let k = -0.75f32 * scale;
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_force_at_position(&mut self.world, [mine[0] * k, mine[1] * k], [muzzle[0], muzzle[1]]);
                let point = self.flamethrowers[i].Point;
                let at = self.transform_point3(p, [point[0], point[1], 0.0]);
                self.flamethrower_affect_point(i, [at[0], at[1]], [right[0] * scale, right[1] * scale]);
                let (range, mask) = (self.flamethrowers[i].Range, self.flamethrowers[i].LayerMask);
                for (origin, direction) in self.flamethrower_rays(i) {
                    if let Some(hit) = self.circle_cast_first(origin, 0.05, direction, range, mask) {
                        self.flamethrower_affect_point(i, hit, direction);
                    }
                }
            }
        }
        if self.stopped_used_continuously(p) {
            let src = self.flamethrower_source(i);
            if self.audio.is_playing(src) {
                self.audio.stop(src);
            }
        }
    }

    /// IgnitionSource (a null one throws).
    fn flamethrower_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.flamethrowers[i].id, "IgnitionSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// FlamethrowerBehaviour.GetAllRays: Ray2D normalizes the direction.
    fn flamethrower_rays(&self, i: usize) -> Vec<([f32; 2], [f32; 2])> {
        let s = &self.flamethrowers[i];
        let p = s.phys;
        let c = self.transform_point3(p, [s.Point[0], s.Point[1], 0.0]);
        let t = &self.physicals[p].transform;
        let base_angle = (d(super::dump::euler_z(t.rotation)) + if t.localScale[0] > 0.0 { 0.0f32 } else { 180.0f32 }).f();
        let to_radians = std::f64::consts::PI as f32 / 180.0f32;
        (0..s.RayCount)
            .map(|k| {
                let f = (d(to_radians) * (d(base_angle) + d(s.Angle) * (d(k as f32) / s.RayCount as f32) - d(s.Angle) / 2.0f32)).f();
                ([c[0], c[1]], mathf::normalized2([mathf::cos(f), mathf::sin(f)]))
            })
            .collect()
    }

    /// Physics2D.CircleCast(origin, radius, direction, distance, layerMask): the point of the nearest hit.
    /// PhysicsQuery2D::CircleCast (180be9100) normalizes the direction (a zero one stays zero), casts to
    /// origin + distance * direction and keeps the first hit of Cast2DQueryBase::RunQuery's sorted contacts.
    fn circle_cast_first(&mut self, origin: [f32; 2], radius: f32, direction: [f32; 2], distance: f32, mask: u32) -> Option<[f32; 2]> {
        self.circle_cast(origin, radius, direction, distance, mask).map(|h| [h.point.x, h.point.y])
    }

    /// Physics2D.CircleCast(origin, radius, direction, distance, layerMask): the nearest hit (as `circle_cast_first`).
    pub(super) fn circle_cast(&mut self, origin: [f32; 2], radius: f32, direction: [f32; 2], distance: f32, mask: u32) -> Option<crate::b2::cast::CastHit> {
        if 0.0001f32 >= radius {
            return None;
        }
        // PhysicsQuery2D::CircleCast (180be9100): an infinite distance casts 100000 and scales each hit's fraction (+0x1c) by it.
        let infinite = distance == f32::INFINITY;
        let distance = if infinite { 100000.0 } else { distance };
        self.sync_pending();
        let sqr = direction[0] * direction[0] + direction[1] * direction[1];
        let dir = if sqr != 0.0 {
            let inv = 1.0f32 / sqr.sqrt();
            [direction[0] * inv, direction[1] * inv]
        } else {
            [0.0, 0.0]
        };
        let end = V::new(origin[0] + dir[0] * distance, origin[1] + dir[1] * distance);
        let mut candidates = self.world.circle_cast_candidates(V::new(origin[0], origin[1]), end, radius, mask);
        super::sort::msvc_sort(&mut candidates, |a, b| a.fraction < b.fraction);
        let mut r = radius;
        // RaycastHit2D.transform is the collider's (or its Rigidbody2D's) Transform: never null here.
        let hit = candidates.iter().find_map(|c| self.world.circle_cast_hit(c, &mut r));
        hit.map(|mut h| {
            if infinite {
                h.fraction *= 100000.0;
            }
            h
        })
    }

    /// FlamethrowerBehaviour.AffectPoint: Physics2D.OverlapCircleNonAlloc(worldPoint, 0.1f, buffer[32]).
    pub(crate) fn flamethrower_affect_point(&mut self, i: usize, world_point: [f32; 2], dir: [f32; 2]) {
        let mut hits = self.overlap_circle(V::new(world_point[0], world_point[1]), 0.1, DEFAULT_RAYCAST_LAYERS);
        // OverlapCircleArray_Binding (180bf4620): the query's first min(count, buffer length) colliders.
        hits.truncate(FLAMETHROWER_BUFFER);
        for (physical, collider) in hits {
            self.flamethrower_affect_collider(i, physical, collider, dir, world_point);
        }
    }

    /// FlamethrowerBehaviour.AffectCollider
    pub(crate) fn flamethrower_affect_collider(&mut self, i: usize, physical: Option<usize>, collider: i32, dir: [f32; 2], point: [f32; 2]) {
        let Some(v) = physical else { return };
        let own = self.flamethrowers[i].phys;
        // collider.transform.root == base.transform.root; PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform)
        if self.root_of(v) == self.root_of(own) || !self.physicals[v].colliders.contains(&collider) || !self.physicals[v].in_world {
            return;
        }
        if self.flamethrowers[i].Effect == 0 {
            let dt = self.time.deltaTime;
            let heat = self.heat_capacity(v);
            let temperature = self.physicals[v].Temperature;
            self.physicals[v].Temperature = (d(temperature) + d(dt) / heat * 2.0f32).f();
            self.ignite_at(v, false, point);
            if self.physicals[v].OnFire {
                self.physicals[v].burnIntensity = 1.0;
            }
        } else {
            self.extinguish(v);
            self.physicals[v].burnIntensity = 0.0;
        }
        let r = self.random.range(0.0, 0.2);
        let rb = self.physicals[v].rigidbody.clone();
        rb.add_force(&mut self.world, [dir[0] * r, dir[1] * r]);
    }
}

/// FlamethrowerBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const FLAMETHROWER_BUFFER: usize = 32;
