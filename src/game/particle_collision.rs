//! CollisionModule::Update (0x1801f0890) in 2D world mode: WorldCollision (1801f1150) traces a batch of rays through
//! PhysicsManager2D::FindParticleIntersections (`b2::World::particle_collisions`), the medium and low qualities keep
//! the planes found in PlaneColliderCache_dense_hashmap and ReadCache (1801ee3c0) them for the particles left
//! untraced, then PerformPlaneCollisions (1801ece60) bounces. ParticleSystem::AssignRayBudgets (1801b53a0) shares the
//! QualitySettings ray budget between the medium and low systems each frame.

use crate::assets::particles::{Collision, MinMaxCurve};
use crate::b2::cast::{ParticleHit, ParticleRay};
use crate::game::particle_modules::rsqrt_newton;
use crate::game::particle_rand::RandN;
use crate::game::particle_sim::{evaluate, maxps};
use crate::particles::Particle;
use std::collections::HashMap;

/// QualitySettings.particleRaycastBudget of People Playground's one quality level ("High").
pub const RAYCAST_BUDGET: i32 = 256;

/// The module's per-system state: m_Random (ResetSeed 1801eea90 seeds it with the system's randomSeed), the cache,
/// ParticleSystemState's nextParticleToTrace and rayBudget, and the ParticleSystem's m_RayBudget with the quality
/// delay AssignRayBudgets keeps.
#[derive(Clone, Debug)]
pub struct State {
    rand: RandN,
    /// PlaneData (normal, intersection) by ComputeHash; never cleared.
    cache: Option<HashMap<u64, ([f32; 3], [f32; 3])>>,
    next: usize,
    ray_budget: usize,
    last_quality: i32,
    delay: i32,
    remaining: i32,
}

impl State {
    pub fn new(random_seed: u32) -> State {
        State { rand: RandN::new([random_seed; 4]), cache: None, next: 0, ray_budget: 0, last_quality: 0, delay: 0, remaining: 0 }
    }
}

/// One system for AssignRayBudgets: its state, the module when enabled, whether the GameObject is active, and its
/// particle count at BeginUpdate.
pub struct Budgeted<'a> {
    /// The run's id in ParticleSystemManager's activeEmitters.
    pub id: u32,
    pub state: &'a mut State,
    pub collision: Option<&'a Collision>,
    pub active: bool,
    pub count: usize,
}

/// AssignRayBudgets over the active systems, then Update1b's rayBudget (m_RayBudget rounded up to 4).
pub fn assign_ray_budgets(systems: &mut [Budgeted]) {
    let mut assigned = vec![0i32; systems.len()];
    let mut listed = Vec::new();
    for (k, s) in systems.iter_mut().enumerate() {
        let quality = s.collision.map_or(0, |c| c.quality);
        if s.state.last_quality != quality {
            s.state.delay = if quality == 2 { 4 } else { 0 };
            s.state.remaining = s.state.delay;
            s.state.last_quality = quality;
        }
        if s.collision.is_some_and(|c| c.kind == 1) && s.state.remaining == 0 && s.active {
            if quality < 1 {
                assigned[k] = s.count as i32;
            } else {
                listed.push(k);
            }
        }
        s.state.remaining = if s.state.remaining == 0 { s.state.delay } else { s.state.remaining - 1 };
    }
    if !listed.is_empty() {
        // The caller gives the systems in activeEmitters' order: the split favours the earlier ones.
        let mut budget = RAYCAST_BUDGET;
        let per = (budget / listed.len() as i32).max(0);
        for &k in &listed {
            let take = (systems[k].count as i32).min(per);
            budget = (budget - take).max(0);
            assigned[k] = take;
        }
        for &k in &listed {
            let take = (systems[k].count as i32 - assigned[k]).min(budget);
            budget -= take;
            assigned[k] += take;
        }
    }
    for (s, a) in systems.iter_mut().zip(assigned) {
        s.state.ray_budget = round4(a.max(0) as usize);
    }
}

fn round4(n: usize) -> usize {
    (n + 3) & !3
}

/// CollisionModule::Update over particles `from..to` with the four lanes' dt. `local` is localToWorld and
/// worldToLocal of a local system; `current_size` and `size_3d` are usesCurrentSize and uses3DSize.
#[allow(clippy::too_many_arguments)]
pub fn update(st: &mut State, c: &Collision, world: &crate::b2::world::World, p: &mut [Particle], tail: &[Option<Particle>], from: usize, to: usize, dt: [f32; 4], local: Option<(&[f32; 16], &[f32; 16])>, current_size: bool, size_3d: bool) {
    if c.kind != 1 || c.mode != 1 {
        unimplemented!("mod-only: CollisionModule: only 2D world collision is rewritten (type {} mode {})", c.kind, c.mode);
    }
    if c.messages {
        unimplemented!("mod-only: CollisionModule: sendCollisionMessages (OnParticleCollision)");
    }
    if c.collider_force > 0.0 {
        unimplemented!("mod-only: CollisionModule: colliderForce {} on dynamic bodies", c.collider_force);
    }
    if c.quality > 0 {
        st.cache.get_or_insert_with(HashMap::new);
    }
    let start = if st.next < to { from.max(st.next) } else { from };
    let dynamic = c.quality < 1 && c.dynamic;
    let results = world_collision(st, c, world, p, tail, from, to, start, dt, local, current_size, size_3d, dynamic);
    if !results.is_empty() {
        perform_plane_collisions(st, c, p, &results, dt);
    }
}

#[allow(clippy::too_many_arguments)]
fn world_collision(st: &mut State, c: &Collision, world: &crate::b2::world::World, p: &[Particle], tail: &[Option<Particle>], from: usize, to: usize, start: usize, dt: [f32; 4], local: Option<(&[f32; 16], &[f32; 16])>, current_size: bool, size_3d: bool, dynamic: bool) -> Vec<ParticleHit> {
    let cached = c.quality > 0;
    let (t0, t1) = if cached { (start, (start + st.ray_budget).min(to)) } else { (from, to) };
    let reach = if c.voxel_size * 4.0 <= c.voxel_size { c.voxel_size } else { c.voxel_size * 4.0 };
    st.next = start + st.ray_budget;
    let n = t1 - t0;
    st.ray_budget = round4(if st.ray_budget <= n { 0 } else { st.ray_budget - n });
    let mut out = Vec::new();
    if n != 0 {
        let half = c.radius_scale * 0.5;
        let mut batches: Vec<[Option<ParticleRay>; 4]> = (0..n.div_ceil(4))
            .map(|b| {
                std::array::from_fn(|j| {
                    let i = t0 + 4 * b + j;
                    // Past the count: the slots' memory (`tail`), an uninitialised one left out (None).
                    let q = p.get(i).or_else(|| tail.get(i - p.len())?.as_ref())?;
                    let prev = step_back(q, dt[j]);
                    let pos = q.position.to_array();
                    let to = if cached {
                        let d = [pos[0] - prev[0], pos[1] - prev[1], pos[2] - prev[2]];
                        let sq = (d[2] * d[2] + d[1] * d[1]) + d[0] * d[0];
                        let r = rsqrt_newton(sq);
                        let u = if f32::from_bits(0x0da2_4260) < sq { d.map(|v| v * r) } else { [0.0; 3] };
                        [u[0] * reach + prev[0], u[1] * reach + prev[1], u[2] * reach + prev[2]]
                    } else {
                        pos
                    };
                    let s = if current_size { [q.current_size.x, q.current_size.y, q.current_z] } else { [q.size, q.size_y, q.size_z] };
                    let radius = if size_3d { maxps(maxps(s[0], s[1]), s[2]) } else { s[0] } * half;
                    Some(ParticleRay { index: i, prev, to, radius })
                })
            })
            .collect();
        // The last batch's lanes from t1 & 3 on copy its lane 0, with index t1.
        let last = batches.last_mut().unwrap();
        let pad = if t1 & 3 == 0 { 4 } else { t1 & 3 };
        let first = last[0];
        for lane in &mut last[pad..] {
            *lane = first.map(|l| ParticleRay { index: t1, ..l });
        }
        if let Some((l2w, _)) = local {
            for l in batches.iter_mut().flatten().flatten() {
                l.prev = to_world(l2w, l.prev);
                l.to = to_world(l2w, l.to);
            }
        }
        out = world.particle_collisions(&batches, t1, dt, c.collides_with, dynamic, c.max_shapes).0;
        if let Some((_, w2l)) = local {
            for h in &mut out {
                h.point = point_to_local(w2l, h.point);
                h.normal = normal_to_local(w2l, h.normal);
            }
        }
        if let Some(cache) = st.cache.as_mut() {
            for h in &out {
                cache.insert(hash(h.origin, h.direction, c.voxel_size), (h.normal, h.point));
            }
        }
    }
    if t0 != from {
        read_cache(st, c, p, &mut out, from, t0, dt, local);
    }
    if to != t1 {
        read_cache(st, c, p, &mut out, t1, to, dt, local);
    }
    out
}

/// pos - (animatedVelocity + velocity) * dt (speedModifier is 1).
fn step_back(q: &Particle, dt: f32) -> [f32; 3] {
    let v = (q.animated + q.velocity).to_array();
    let pos = q.position.to_array();
    [pos[0] - v[0] * dt, pos[1] - v[1] * dt, pos[2] - v[2] * dt]
}

/// localToWorld on a lane: ((m[2] z + t) + m[1] y) + m[0] x per row.
fn to_world(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| ((m[8 + r] * v[2] + m[12 + r]) + m[4 + r] * v[1]) + m[r] * v[0])
}

fn point_to_local(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| (m[r] * v[0] + (m[4 + r] * v[1] + m[8 + r] * v[2])) + m[12 + r])
}

/// The normal through worldToLocal's 3x3, normalized per lane on that lane's own sum order; (0, 0, 1) lanes when the
/// length is too small.
fn normal_to_local(m: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    let n: [f32; 3] = std::array::from_fn(|r| m[r] * v[0] + (m[4 + r] * v[1] + m[8 + r] * v[2]));
    let s = n.map(|x| x * x);
    let sums = [(s[0] + s[1]) + s[2], (s[1] + s[2]) + s[0], s[2] + (s[0] + s[1])];
    std::array::from_fn(|k| if f32::from_bits(0x0da2_4260) < sums[k] { rsqrt_newton(sums[k]) * n[k] } else { [0.0, 0.0, 1.0][k] })
}

#[allow(clippy::too_many_arguments)]
fn read_cache(st: &State, c: &Collision, p: &[Particle], out: &mut Vec<ParticleHit>, from: usize, to: usize, dt: [f32; 4], local: Option<(&[f32; 16], &[f32; 16])>) {
    let cache = st.cache.as_ref().unwrap();
    let mut b = from;
    while b < to {
        for j in 0..4 {
            let i = b + j;
            if i >= to {
                continue;
            }
            let (mut prev, mut pos) = (step_back(&p[i], dt[j]), p[i].position.to_array());
            if let Some((l2w, _)) = local {
                (prev, pos) = (to_world(l2w, prev), to_world(l2w, pos));
            }
            let direction = [pos[0] - prev[0], pos[1] - prev[1], pos[2] - prev[2]];
            if let Some(&(normal, point)) = cache.get(&hash(prev, direction, c.voxel_size)) {
                out.push(ParticleHit { index: i, origin: prev, direction, normal, point });
            }
        }
        b += 4;
    }
}

/// PlaneColliderCache_dense_hashmap::ComputeHash (1807fedf0): CityHash64 of the voxel cell (y cells four times as
/// tall), xor the direction's major axis and sign.
fn hash(o: [f32; 3], d: [f32; 3], voxel: f32) -> u64 {
    let cell = |v: f32| {
        let f = v.floor();
        // cvttss2si: out of range and NaN give 0x80000000.
        if (-2147483648.0..2147483648.0).contains(&f) { f as i32 } else { i32::MIN }
    };
    let key = [cell(o[0] / voxel), cell(o[1] / (voxel * 4.0)), cell(o[2] / voxel)];
    let mut bytes = [0u8; 12];
    for (k, v) in key.iter().enumerate() {
        bytes[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    let a = d.map(f32::abs);
    let bucket = if a[0] >= a[1] && a[0] >= a[2] {
        !(d[0] > 0.0) as u64
    } else if a[1] >= a[2] {
        2 + !(d[1] > 0.0) as u64
    } else {
        4 + !(d[2] > 0.0) as u64
    };
    city64_12(&bytes) ^ bucket
}

/// CityHash64 v1.1 (1805302d0) of 12 bytes: HashLen0to16's 8..16 branch.
fn city64_12(s: &[u8; 12]) -> u64 {
    const K2: u64 = 0x9ae1_6a3b_2f90_404f;
    let mul = K2.wrapping_add(24);
    let a = u64::from_le_bytes(s[0..8].try_into().unwrap()).wrapping_add(K2);
    let b = u64::from_le_bytes(s[4..12].try_into().unwrap());
    let c = b.rotate_right(37).wrapping_mul(mul).wrapping_add(a);
    let d = a.rotate_right(25).wrapping_add(b).wrapping_mul(mul);
    let mut x = (c ^ d).wrapping_mul(mul);
    x ^= x >> 47;
    let mut y = (d ^ x).wrapping_mul(mul);
    y ^= y >> 47;
    y.wrapping_mul(mul)
}

/// The module's curves at time 0 (constant and two-constant only).
fn at0(c: &MinMaxCurve, r: f32, what: &str) -> f32 {
    match c.state {
        0 | 3 => evaluate(c, 0.0, r),
        s => unimplemented!("mod-only: PerformPlaneCollisions: {what} curve state {s}"),
    }
}

/// PerformPlaneCollisions: three draws per four results (bounce, lifetimeLoss, dampen), then the results from the last.
fn perform_plane_collisions(st: &mut State, c: &Collision, p: &mut [Particle], results: &[ParticleHit], dt: [f32; 4]) {
    let groups = results.len().div_ceil(4);
    let mut draws = Vec::with_capacity(groups);
    for _ in 0..groups {
        let b = st.rand.get01().map(|r| 1.0 - at0(&c.bounce, r, "bounce"));
        let l = st.rand.get01().map(|r| at0(&c.lifetime_loss, r, "lifetimeLoss"));
        let d = st.rand.get01().map(|r| 1.0 - at0(&c.dampen, r, "dampen"));
        draws.push((b, l, d));
    }
    let (min2, max2) = (c.min_kill_speed * c.min_kill_speed, c.max_kill_speed * c.max_kill_speed);
    let cached = c.quality > 0;
    for j in (0..results.len()).rev() {
        let h = &results[j];
        let (bb, ll, dd) = &draws[j >> 2];
        let (b, l, d) = (bb[j & 3], ll[j & 3], dd[j & 3]);
        let q = &mut p[h.index];
        let n = h.normal;
        let anim = q.animated.to_array();
        let vel = q.velocity.to_array();
        let v = [vel[0] + anim[0], vel[1] + anim[1], vel[2] + anim[2]];
        let pos = q.position.to_array();
        let mut at = h.point;
        if cached {
            let t = dt[h.index & 3];
            let dv = v.map(|x| t * x);
            let prev = [pos[0] - dv[0], pos[1] - dv[1], pos[2] - dv[2]];
            let denom = n[2] * dv[2] + (n[0] * dv[0] + n[1] * dv[1]);
            if f32::from_bits(0x3400_0000) > denom.abs() || denom >= 0.0 {
                continue;
            }
            let s = -(((n[2] * prev[2]) + (prev[0] * n[0] + n[1] * prev[1]) - ((n[2] * at[2]) + (n[0] * at[0] + n[1] * at[1]))) / denom);
            if 0.0 > s || s > 1.0 {
                continue;
            }
            at = [prev[0] + dv[0] * s, prev[1] + dv[1] * s, prev[2] + dv[2] * s];
        }
        q.alive += l * 100.0;
        let s2 = (v[1] * v[1] + v[0] * v[0]) + v[2] * v[2];
        if q.alive > 100.0 || min2 > s2 || s2 > max2 {
            q.alive = f32::from_bits(0x42c8_0001);
        }
        let e = [pos[0] - at[0], pos[1] - at[1], pos[2] - at[2]];
        let k = ((e[2] * n[2]) + (e[0] * n[0] + e[1] * n[1])) * -2.0;
        let mut r = [(e[0] + k * n[0]) * d, (k * n[1] + e[1]) * d, (k * n[2] + e[2]) * d];
        let kv = ((v[2] * n[2]) + (v[0] * n[0] + v[1] * n[1])) * -2.0;
        let mut w = [(v[0] + kv * n[0]) * d, (v[1] + kv * n[1]) * d, (v[2] + kv * n[2]) * d];
        let er = (n[2] * r[2]) + (r[0] * n[0] + n[1] * r[1]);
        r = [r[0] - (er * n[0]) * b, r[1] - (n[1] * er) * b, r[2] - (n[2] * er) * b];
        let ev = n[2] * w[2] + (n[0] * w[0] + n[1] * w[1]);
        w = [w[0] - (ev * n[0]) * b, w[1] - (n[1] * ev) * b, w[2] - (n[2] * ev) * b];
        q.position = bevy::math::Vec3::new(at[0] + r[0], at[1] + r[1], at[2] + r[2]);
        q.velocity = bevy::math::Vec3::new(w[0] - anim[0], w[1] - anim[1], w[2] - anim[2]);
    }
}

#[cfg(test)]
mod tests {
    /// The hash's direction buckets and cell rounding (one cell, x major negative = bucket 1).
    #[test]
    fn plane_cache_hash() {
        let o = [0.3, 1.9, 0.0];
        let base = super::hash(o, [1.0, 0.0, 0.0], 0.5);
        assert_eq!(super::hash(o, [-1.0, 0.5, 0.0], 0.5), base ^ 1);
        assert_eq!(super::hash(o, [0.1, 2.0, 0.0], 0.5), base ^ 2);
        assert_eq!(super::hash(o, [0.0, 0.0, -3.0], 0.5), base ^ 5);
        assert_eq!(super::hash([0.49, 1.99, 0.0], [1.0, 0.0, 0.0], 0.5), base);
        assert_ne!(super::hash([0.51, 1.9, 0.0], [1.0, 0.0, 0.0], 0.5), base);
    }

    /// Every system's CollisionModule with its GameObject (`cargo test collision_scan -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn collision_scan() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut m = std::collections::BTreeMap::<String, Vec<String>>::new();
        for (fi, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let me = crate::assets::db::ObjRef { file: fi, path_id: o.path_id };
                let name = crate::assets::components::game_object_of(db.data(me)).ok().and_then(|g| db.resolve(fi, g)).and_then(|g| db.game_object(g).ok()).map_or(String::new(), |g| g.name);
                let p = crate::assets::particles::ParticleSystem::read(f.data(o)).unwrap();
                let Some(c) = &p.collision else { continue };
                let st = |c: &crate::assets::particles::MinMaxCurve| (c.state, c.min_scalar, c.scalar);
                let k = format!(
                    "on {} kind {} mode {} planes {} force {} dampen {:?} bounce {:?} loss {:?} kill {}..{} radius {} mask {:x} shapes {} quality {} voxel {} msgs {} dyn {} interior {} space {} subs {:?}",
                    c.enabled, c.kind, c.mode, c.planes, c.collider_force, st(&c.dampen), st(&c.bounce), st(&c.lifetime_loss), c.min_kill_speed, c.max_kill_speed, c.radius_scale, c.collides_with, c.max_shapes, c.quality,
                    c.voxel_size, c.messages, c.dynamic, c.interior, p.space, p.subs.iter().map(|s| s.kind).collect::<Vec<_>>()
                );
                if c.enabled || c.kind != 0 || c.mode != 0 {
                    m.entry(k).or_default().push(name);
                }
            }
        }
        for (k, v) in &m {
            println!("{:4} {k}\n     {:?}", v.len(), v.iter().take(12).collect::<Vec<_>>());
        }
    }
}
