//! ExternalForcesModule (Update 0x180dcd7c0, Cache 0x180dccc40) for WindZones, ApplyDirectionalForce (180dcc220) and
//! ApplyRadialForce (180dcc670), and ParticleSystemForceFields (ApplyForceField<0> 180dc8d40), on one particle; the
//! sphere test against the system's world bounds (CalculateWorldBounds 1801b6a90, TransformAABB 1806433f0,
//! IntersectAABBSphere 180644e20).

use crate::assets::particles::{ExternalForces, ForceField, WindZone};
use crate::game::particle_modules::{draws, rsqrt_newton};
use crate::game::particle_sim::{evaluate, maxps, minps};

/// A WindZone in WindManager order (AddToManager links it at the back, RemoveFromManager unlinks it): its
/// Transform's localToWorld, its GameObject's layer, and the zone with windMain as the scripts left it.
#[derive(Clone, Debug)]
pub struct Zone {
    pub l2w: [f32; 16],
    pub layer: u32,
    pub zone: WindZone,
}

/// A ParticleSystemForceField in ParticleSystemForceFieldManager order (Add pushes back, RemoveFromManager moves the
/// last into its slot): its component id, Transform localToWorld, layer and parameters.
#[derive(Clone, Debug)]
pub struct Field {
    pub id: i32,
    pub l2w: [f32; 16],
    pub layer: u32,
    pub field: std::sync::Arc<ForceField>,
}

/// One of the module's cachedForces: a wind (shape -1) with its strength, or a field (CacheForce).
#[derive(Clone, Debug)]
pub enum Force {
    Wind { l2w: [f32; 16], mode: i32, strength: f32, radius: f32 },
    Field { l2w: [f32; 16], field: std::sync::Arc<ForceField> },
}

/// The cached strength: windMain * (magnitude * (((cos(phase 0.375) + cos(phase)) + cos(phase 0.05)) * 0.333) + 1)
/// with phase = frequency * (timeSinceSceneLoad * pi).
pub fn strength(w: &WindZone, time_since_scene_load: f32) -> f32 {
    let phase = w.pulse_frequency * (time_since_scene_load * f32::from_bits(0x4049_0fdb));
    let mut s = crate::unity::cosf(phase * 0.375) + crate::unity::cosf(phase);
    s += crate::unity::cosf(phase * 0.05);
    w.main * (w.pulse_magnitude * (s * f32::from_bits(0x3eaa_7efa)) + 1.0)
}

/// ExternalForcesModule::Cache: unless the filter is List, and with a mask, the winds whose layer is in it, then the
/// manager's fields whose layer is in it; unless the filter is LayerMask, the influence list's fields (null, inactive
/// or disabled ones skipped, each once; under LayerMaskAndList the ones the mask took are skipped). `list` holds the
/// list's entries as field component ids (None: null); a live field is one the manager holds.
pub fn cache(x: &ExternalForces, winds: &[Zone], fields: &[Field], list: &[Option<i32>], time_since_scene_load: f32) -> Vec<Force> {
    let mut out = Vec::new();
    let field = |f: &Field| Force::Field { l2w: f.l2w, field: f.field.clone() };
    let in_mask = |layer: u32| (x.influence_mask >> (layer & 31)) & 1 != 0;
    if x.influence_filter != 1 && x.influence_mask != 0 {
        for z in winds.iter().filter(|z| in_mask(z.layer)) {
            out.push(Force::Wind { l2w: z.l2w, mode: z.zone.mode, strength: strength(&z.zone, time_since_scene_load), radius: z.zone.radius });
        }
        out.extend(fields.iter().filter(|f| in_mask(f.layer)).map(field));
    }
    if x.influence_filter != 0 {
        let mut seen = Vec::new();
        for id in list.iter().flatten() {
            let Some(f) = fields.iter().find(|f| f.id == *id) else { continue };
            if (x.influence_filter == 2 && in_mask(f.layer)) || seen.contains(id) {
                continue;
            }
            seen.push(*id);
            out.push(field(f));
        }
    }
    out
}

/// ParticleSystemState.minMaxAABB (min x, y, z, max x, y, z) as CalculateWorldBounds makes it a world AABB (center,
/// extent): (min + max) * 0.5 and (max - min) * 0.5, through TransformAABB by localToWorld unless in world space.
pub fn world_bounds(b: &[f32; 6], world: bool, l2w: &[f32; 16]) -> [f32; 6] {
    let c = [(b[0] + b[3]) * 0.5, (b[4] + b[1]) * 0.5, (b[5] + b[2]) * 0.5];
    let e = [(b[3] - b[0]) * 0.5, (b[4] - b[1]) * 0.5, (b[5] - b[2]) * 0.5];
    if world {
        return [c[0], c[1], c[2], e[0], e[1], e[2]];
    }
    let m = l2w;
    let ext = |i: usize| ((m[i] * e[0]).abs() + (m[4 + i] * e[1]).abs()) + (m[8 + i] * e[2]).abs();
    let ctr = |i: usize| ((c[0] * m[i] + c[1] * m[4 + i]) + c[2] * m[8 + i]) + m[12 + i];
    [ctr(0), ctr(1), ctr(2), ext(0), ext(1), ext(2)]
}

/// IntersectAABBSphere: per axis (maxps(|center - aabb center|, extent) - extent)^2, the lanes summed as
/// (z + 0) + (x + y), against radius^2.
pub fn intersects(a: &[f32; 6], center: [f32; 3], radius: f32) -> bool {
    let s = |i: usize| {
        let v = maxps((center[i] - a[i]).abs(), a[3 + i]) - a[3 + i];
        v * v
    };
    (s(2) + 0.0) + (s(0) + s(1)) < radius * radius
}

/// Update on one particle: every cached force in order, a wind's position and direction in the system's space (`w2l`
/// None in world simulation), the spherical ones only where their sphere meets the system's world bounds `bounds`;
/// `dt` the step. A field (its `prepare`d constants in `fields`, None out of reach) acts only when bit i of `live` says
/// a particle of this one's batch of four is in its range (`live_bits`).
#[allow(clippy::too_many_arguments)]
pub fn apply(x: &ExternalForces, forces: &[Force], fields: &[Option<FieldPrep>], bounds: &[f32; 6], w2l: Option<&[f32; 16]>, dt: f32, live: u64, p: &FieldLane, vel: &mut [f32; 3]) {
    let FieldLane { position, alive, seed, .. } = *p;
    if !(0.0 < x.multiplier.scalar) {
        return;
    }
    const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let w = w2l.unwrap_or(&IDENTITY);
    let t = maxps(alive * f32::from_bits(0x3c23_d70a), 0.0);
    let [r] = draws::<1>(seed, 0xe37a_82fb);
    let k0 = evaluate(&x.multiplier, t, r);
    for (i, f) in forces.iter().enumerate() {
        match f {
            Force::Wind { l2w: m, mode, strength, radius } => {
                let at: [f32; 3] = std::array::from_fn(|i| ((m[12] * w[i] + m[13] * w[4 + i]) + m[14] * w[8 + i]) + w[12 + i]);
                let force = strength * dt;
                if *mode == 1 {
                    if !intersects(bounds, [m[12], m[13], m[14]], *radius) {
                        continue;
                    }
                    let d = [position[0] - at[0], position[1] - at[1], position[2] - at[2]];
                    let sq = (d[1] * d[1] + d[2] * d[2]) + d[0] * d[0];
                    let rn = rsqrt_newton(sq);
                    let u = if f32::from_bits(0x0da2_4260) < sq { d.map(|v| rn * v) } else { [0.0; 3] };
                    let s = minps(1.0, maxps(0.0, sq.sqrt() * (1.0 / radius)));
                    let k = (k0 * (1.0 - s * s)) * force;
                    for i in 0..3 {
                        vel[i] += k * u[i];
                    }
                } else if *mode == 0 {
                    let dir: [f32; 3] = std::array::from_fn(|i| (m[8] * w[i] + m[9] * w[4 + i]) + m[10] * w[8 + i]);
                    let k = k0 * force;
                    for i in 0..3 {
                        vel[i] += k * dir[i];
                    }
                }
            }
            Force::Field { .. } => {
                let Some(f) = &fields[i] else { continue };
                if live >> i & 1 != 0 {
                    field_apply(f, &field_place(f, seed, position), k0 * dt, p, vel);
                }
            }
        }
    }
}

/// What ApplyForceField<0> reads of one particle.
#[derive(Clone, Copy, Debug)]
pub struct FieldLane {
    pub position: [f32; 3],
    pub alive: f32,
    pub seed: u32,
    /// animatedVelocity and the size stream (sizes or currentSizes), uses3DSize.
    pub anim: [f32; 3],
    pub size: [f32; 3],
    pub uses_3d_size: bool,
}

/// A ParticleSystemForceField as ExternalForcesModule::Update (180dcd7c0) hands it to ApplyForceField<0>: `a4` from the
/// system's space to the field's (m0, m1, m2, t), `a5` the field's axes in the system's space, and the constants
/// ApplyForceField works out before its loop.
#[derive(Clone, Debug)]
pub struct FieldPrep {
    field: std::sync::Arc<ForceField>,
    a4: [[f32; 3]; 4],
    a5: [[f32; 3]; 4],
    inv_end: f32,
    start_n: f32,
    k: f32,
    rot: [f32; 2],
    focus: f32,
    random_rotation: bool,
}

/// InvertMatrix4x4_General3D (18073f140): the 3x3 by cofactors over the determinant, the translation through it;
/// zeros when det^2 (as a double) is below 1e-25.
pub fn invert_general_3d(m: &[f32; 16]) -> [f32; 16] {
    let det = ((((((m[0] * m[5]) * m[10] + 0.0) + (m[1] * m[6]) * m[8]) + (m[2] * m[4]) * m[9]) - (m[2] * m[5]) * m[8]) - (m[4] * m[1]) * m[10]) - (m[6] * m[0]) * m[9];
    if 1e-25 > ((det * det) as f64) {
        return [0.0; 16];
    }
    let inv = 1.0 / det;
    let mut o = [0.0; 16];
    o[0] = (m[10] * m[5] - m[9] * m[6]) * inv;
    o[4] = -((m[10] * m[4] - m[8] * m[6]) * inv);
    o[8] = (m[4] * m[9] - m[8] * m[5]) * inv;
    o[1] = -((m[10] * m[1] - m[2] * m[9]) * inv);
    o[5] = (m[10] * m[0] - m[8] * m[2]) * inv;
    o[9] = -((m[0] * m[9] - m[8] * m[1]) * inv);
    o[2] = (m[1] * m[6] - m[2] * m[5]) * inv;
    o[6] = -((m[0] * m[6] - m[2] * m[4]) * inv);
    o[10] = (m[0] * m[5] - m[4] * m[1]) * inv;
    o[12] = -((m[13] * o[4] + o[0] * m[12]) + m[14] * o[8]);
    o[13] = -((o[1] * m[12] + m[13] * o[5]) + m[14] * o[9]);
    o[14] = -((o[2] * m[12] + m[13] * o[6]) + m[14] * o[10]);
    o[15] = 1.0;
    o
}

/// ExternalForcesModule::Update's field: the reach test (GetLossyScale 18073dde0: each column's sqrt((x^2 + y^2) +
/// z^2), the largest abs times endRange, against the system's world bounds), the matrices (`sys` the system's
/// localToWorld and worldToLocal, None in world simulation: the identity) and ApplyForceField<0>'s setup.
pub fn prepare(l2w: &[f32; 16], field: &std::sync::Arc<ForceField>, bounds: &[f32; 6], sys: Option<(&[f32; 16], &[f32; 16])>) -> Option<FieldPrep> {
    const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let m = l2w;
    let s: [f32; 3] = std::array::from_fn(|c| ((m[4 * c] * m[4 * c] + m[4 * c + 1] * m[4 * c + 1]) + m[4 * c + 2] * m[4 * c + 2]).sqrt().abs());
    let yz = if s[2] <= s[1] { s[1] } else { s[2] };
    let big = if s[0] < yz { yz } else { s[0] };
    if !intersects(bounds, [m[12], m[13], m[14]], field.end_range * big) {
        return None;
    }
    let f = field;
    if f.shape != 0 {
        unimplemented!("mod-only: ExternalForcesModule: a ParticleSystemForceField of shape {} (ApplyForceField<{}>)", f.shape, f.shape);
    }
    if f.direction.iter().any(|c| c.scalar != 0.0) {
        unimplemented!("mod-only: ApplyForceField<0>: directionX/Y/Z (RandN seed + 0x49ca3fc1)");
    }
    if !f.vector_field.is_null() && f.vector_field_speed.scalar > 0.0 {
        unimplemented!("mod-only: ApplyForceField<0>: a vector field");
    }
    let (sl2w, sw2l) = sys.unwrap_or((&IDENTITY, &IDENTITY));
    let w = invert_general_3d(l2w);
    // Column j of inv(F) * S: ((c0 x + c1 y) + c2 z) + c3 w; of S_w2l * F: ((c1 y + c0 x) + c2 z) + c3 w.
    let a4: [[f32; 3]; 4] = std::array::from_fn(|j| {
        let x = &sl2w[4 * j..4 * j + 4];
        std::array::from_fn(|i| ((w[i] * x[0] + w[4 + i] * x[1]) + w[8 + i] * x[2]) + w[12 + i] * x[3])
    });
    let a5: [[f32; 3]; 4] = std::array::from_fn(|j| {
        let e = &l2w[4 * j..4 * j + 4];
        std::array::from_fn(|i| ((sw2l[4 + i] * e[1] + sw2l[i] * e[0]) + sw2l[8 + i] * e[2]) + sw2l[12 + i] * e[3])
    });
    Some(setup(f, a4, a5))
}

/// ApplyForceField<0>'s constants before its loop.
fn setup(f: &std::sync::Arc<ForceField>, a4: [[f32; 3]; 4], a5: [[f32; 3]; 4]) -> FieldPrep {
    // rcpps and two Newton steps (the raw estimate if they give NaN); 2pi rotationRandomness; gravityFocus in range.
    let (r, st) = (f.end_range, f.start_range);
    let rc = crate::unity::sse::rcp(r);
    let a = (f32::from_bits(0x4000_0002) - rc * r) * rc;
    let b = a * (2.0 - a * r);
    let two_pi = f32::from_bits(0x40c9_0fdb);
    let rot = [f.rotation_randomness[0] * two_pi, f.rotation_randomness[1] * two_pi];
    let eps = f32::from_bits(0x3586_37bd);
    FieldPrep {
        field: f.clone(),
        a4,
        a5,
        inv_end: if b.is_nan() { rc } else { b },
        start_n: st / r,
        k: r / (r - st),
        rot,
        focus: f.gravity_focus * (r - st) + st,
        random_rotation: eps < rot[0] || eps < rot[1],
    }
}

/// Where ApplyForceField<0> finds a particle: its position in the field's space (`q`, turned by the random rotation
/// `m` when rotationRandomness is on), the distance, out of [startRange, endRange], and the curves' time.
pub struct Place {
    q: [f32; 3],
    m: Option<[[f32; 3]; 3]>,
    dist: f32,
    pub out: bool,
    t: f32,
}

pub fn field_place(f: &FieldPrep, seed: u32, position: [f32; 3]) -> Place {
    let a = &f.a4;
    let [x, y, z] = position;
    let mut q: [f32; 3] = std::array::from_fn(|i| ((z * a[2][i] + a[3][i]) + y * a[1][i]) + x * a[0][i]);
    let mut m = None;
    if f.random_rotation {
        use crate::game::particle_shape::sincos;
        let [ra] = draws::<1>(seed, 0x8937_5938);
        let [rb] = draws::<1>(seed, 0xeef6_27ab);
        let (sa, ca) = sincos((ra - 0.5) * f.rot[1]);
        let (sb, cb) = sincos((rb - 0.5) * f.rot[0]);
        let (sc, cc) = sincos(0.0);
        // Rows: the rotated x, y and z as coefficients of x, y, z.
        let r = [
            [(sb * sc) * sa + ca * cc, (sb * ca) * sc - sa * cc, sc * cb],
            [sa * cb, cb * ca, -sb],
            [sc * -ca + (sb * cc) * sa, sb * (ca * cc) + sc * sa, cb * cc],
        ];
        q = std::array::from_fn(|i| (r[i][2] * q[2] + r[i][1] * q[1]) + q[0] * r[i][0]);
        m = Some(r);
    }
    let dist = ((q[2] * q[2] + q[1] * q[1]) + q[0] * q[0]).sqrt();
    let (r, s) = (f.field.end_range, f.field.start_range);
    let out = r < dist || dist < s;
    let t = maxps((minps(dist, r) * f.inv_end - f.start_n) * f.k, 0.0);
    Place { q, m, dist, out, t }
}

/// Bit i: the particle is in field i's range (forces that are not fields, or out of reach, 0).
pub fn live_bits(fields: &[Option<FieldPrep>], seed: u32, position: [f32; 3]) -> u64 {
    assert!(fields.len() <= 64, "ExternalForcesModule: more than 64 cached forces");
    let mut bits = 0;
    for (i, f) in fields.iter().enumerate() {
        if let Some(f) = f {
            if !field_place(f, seed, position).out {
                bits |= 1 << i;
            }
        }
    }
    bits
}

/// ApplyForceField<0>'s gravity, rotation and drag on one lane of a batch that has one in range; `mult` the
/// multiplier curve times dt. Out of range a lane still takes vel + 0, (target - vel) * 0 and the drag's rewrite.
fn field_apply(f: &FieldPrep, pl: &Place, mult: f32, p: &FieldLane, vel: &mut [f32; 3]) {
    let ff = &f.field;
    let (q, out, t) = (pl.q, pl.out, pl.t);
    let a5 = &f.a5;
    let tiny = f32::from_bits(0x0da2_4260);
    // Through the field's axes, normalized (rsqrtps, one Newton step; 0 at |v|^2 <= 1e-30).
    let unit = |u: [f32; 3]| -> [f32; 3] {
        let o: [f32; 3] = std::array::from_fn(|i| (a5[1][i] * u[1] + a5[2][i] * u[2]) + a5[0][i] * u[0]);
        let l2 = (o[2] * o[2] + o[1] * o[1]) + o[0] * o[0];
        let n = rsqrt_newton(l2);
        if tiny < l2 { o.map(|c| c * n) } else { [0.0; 3] }
    };
    if ff.gravity.scalar != 0.0 {
        // With the random rotation the position goes back through its transpose.
        let p3 = match &pl.m {
            None => q,
            Some(m) => std::array::from_fn(|i| (m[2][i] * q[2] + m[1][i] * q[1]) + q[0] * m[0][i]),
        };
        let mut n = unit(p3);
        let [r] = draws::<1>(p.seed, 0xf04d_bbab);
        let g = evaluate(&ff.gravity, t, r);
        if pl.dist < f.focus {
            n = n.map(|c| -c);
        }
        let k = if out { 0.0 } else { ((-g) * mult) * 30.0 };
        for i in 0..3 {
            vel[i] += n[i] * k;
        }
    }
    if ff.rotation_speed.scalar != 0.0 && ff.rotation_attraction.scalar != 0.0 {
        let [r] = draws::<1>(p.seed, 0xbfb0_9356);
        let speed = (mult * evaluate(&ff.rotation_speed, t, r)) * 30.0;
        let [r] = draws::<1>(p.seed, 0xa12a_3aba);
        let att = if out { 0.0 } else { (mult * evaluate(&ff.rotation_attraction, t, r)) * 30.0 };
        // The tangent (z, 0, -x) about the field's y axis.
        let u = match &pl.m {
            None => [q[2], 0.0, -q[0]],
            Some(m) => {
                let (x, z) = (-q[0], q[2]);
                [(x * m[2][0] + 0.0 * m[1][0]) + z * m[0][0], (0.0 * m[1][1] + x * m[2][1]) + z * m[0][1], (0.0 * m[1][2] + x * m[2][2]) + z * m[0][2]]
            }
        };
        let n = unit(u);
        for i in 0..3 {
            vel[i] += (n[i] * speed - vel[i]) * att;
        }
    }
    if ff.drag.scalar != 0.0 {
        let [r] = draws::<1>(p.seed, 0x653a_ecf2);
        let drag = evaluate(&ff.drag, t, r);
        let an = p.anim;
        let v: [f32; 3] = std::array::from_fn(|i| vel[i] + an[i]);
        let sq = (v[1] * v[1] + v[2] * v[2]) + v[0] * v[0];
        let [sx, sy, sz] = p.size;
        let size = if p.uses_3d_size { maxps(sx, maxps(sy, sz)) } else { sx };
        let h = size * 0.5;
        let speed = sq.sqrt();
        let d = v.map(|c| if f32::from_bits(0x2690_1d7d) < speed { c / speed } else { 0.0 });
        let area = if ff.drag_by_size { (f32::from_bits(0x4049_0fdb) * h) * h } else { 1.0 };
        let by = if ff.drag_by_velocity { sq } else { 1.0 };
        let dec = if out { 0.0 } else { (area * drag) * by };
        let s = maxps(0.0, speed - dec * mult);
        for i in 0..3 {
            vel[i] = d[i] * s - an[i];
        }
    }
}

/// WindControllerBehaviour: its component, Awake's GetComponent<WindZone>() and BaseStrength.
#[derive(Clone, Debug)]
pub struct WindController {
    pub id: i32,
    pub zone: Option<i32>,
    pub base_strength: f32,
}

impl Default for WindController {
    fn default() -> Self {
        Self {
            id: Default::default(),
            zone: Default::default(),
            base_strength: 3.0,
        }
    }
}

impl crate::game::Game {
    /// WindControllerBehaviour.Update: num = Time.time * 0.05f, windZone.windMain = BaseStrength *
    /// PerlinNoise(num, 5253.294f), then transform.Rotate(0, 0.5f * (PerlinNoise(-57.1532f, 0f - num) * 2f - 1f), 0):
    /// localRotation * Quaternion.Euler (Vector3 * Deg2Rad, then EulerToQuaternion 18073d0c0 in ZXY order).
    pub(crate) fn wind_controller_update(&mut self, i: usize) {
        use crate::game::d;
        use crate::unity::{cosf, perlin_noise, sinf};
        let w = self.wind_controllers[i].clone();
        let num = self.time.time * 0.05;
        let num2 = (d(perlin_noise(-57.1532, 0.0 - num)) * 2.0 - 1.0).f();
        let Some(zone) = w.zone else { crate::game::menu::throw("NullReferenceException: WindControllerBehaviour.windZone") };
        self.wind_main.insert(zone, w.base_strength * perlin_noise(num, 5253.294));
        let rad = [0.0, 0.5 * num2, 0.0].map(|v: f32| v * crate::game::mathf::DEG2RAD * 0.5);
        let q = |axis: usize| {
            let mut q = [0.0, 0.0, 0.0, cosf(rad[axis])];
            q[axis] = sinf(rad[axis]);
            q
        };
        let euler = crate::game::simulate::qmul(crate::game::simulate::qmul(q(1), q(0)), q(2));
        self.with_component_instance(w.id, |_, inst, o| {
            let r = crate::game::launcher::qmul(inst.objects[o].trs.q, euler);
            inst.set_local_rotation(o, r);
        });
    }

    /// WindZone::RemoveFromManager and ParticleSystemForceFieldManager::Remove (the last field into the slot) for a
    /// destroyed object's components.
    pub(super) fn forces_object_destroy(&mut self, components: &[i32]) {
        self.wind_zones.retain(|w| !components.contains(w));
        for id in components {
            if let Some(i) = self.force_fields.iter().position(|f| f == id) {
                self.force_fields.swap_remove(i);
            }
            self.wind_main.remove(id);
        }
    }

    /// A recording holds no map clone: the recorded map's (header `map X`) objects with an active, enabled WindZone are
    /// instantiated alone at their pose in the map (at the origin under WORLD), so WindManager and their
    /// WindControllerBehaviour are there as in the game.
    pub(super) fn recorded_map_winds(&mut self, header: &[String]) {
        use crate::assets::db::Component;
        use crate::unity::transform::{global_position, global_rotation, Trs};
        if !header.iter().any(|l| l.starts_with("map ")) {
            return;
        }
        // The data is opened for the lookup only: a recording keeps the assets it had.
        let had = self.assets.is_some();
        let game = crate::game_assets::find_game(None).expect("the game's data");
        self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        let mut db = self.assets.take().unwrap();
        let prefab = recorded_map_prefab(&mut db, header).unwrap();
        let trs = |o: usize| {
            let Some((_, Component::Transform(t))) = prefab.objects[o].components.first() else { unreachable!("a Transform first") };
            let v = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
            Trs { t: v(t.local_position), q: t.local_rotation, s: v(t.local_scale) }
        };
        for o in 0..prefab.objects.len() {
            let mut active = prefab.objects[o].active;
            let mut p = prefab.objects[o].parent;
            while let Some(i) = p {
                active &= prefab.objects[i].active;
                p = prefab.objects[i].parent;
            }
            if !active || !prefab.objects[o].components.iter().any(|(_, c)| matches!(c, Component::WindZone(z) if z.enabled)) {
                continue;
            }
            let own = trs(o);
            let (mut t, mut q) = (own.t, own.q);
            let mut p = prefab.objects[o].parent;
            while let Some(i) = p {
                let n = trs(i);
                (t, q) = (global_position(&n, t), global_rotation(&n, q));
                p = prefab.objects[i].parent;
            }
            if !prefab.objects[o].children.is_empty() {
                unimplemented!("mod-only: a recording's map WindZone object with children (no shipped map's WindZone object has any)");
            }
            let sub = db.prefab(prefab.objects[o].game_object).expect("the WindZone object");
            // The recorded ids already hold the map's: the clone takes ids far below the recording's, and the counter
            // goes back.
            let saved = self.next_instance_id;
            self.next_instance_id = saved - 0x2000_0000;
            let (mut inst, _) = self.spawn(&mut db, &sub, [t[0], t[1], t[2]], q, None).expect("the map's WindZone object");
            // The Default / Ocean / Uncharted maps scale their WindZone object (1, 0.0013, 0.0006): the clone keeps it.
            inst.objects[0].trs.s = own.s;
            self.next_instance_id = saved;
            self.unsynced.push(inst);
        }
        if had {
            self.assets = Some(db);
        }
    }

    /// The clone holding component `id`, its object and component index.
    pub(crate) fn component_at(&self, id: i32) -> Option<(&crate::game::spawn::Instance, usize, usize)> {
        self.unsynced.iter().chain(&self.retained).find_map(|i| i.objects.iter().enumerate().find_map(|(o, x)| x.components.iter().position(|&c| c == id).map(|c| (i, o, c))))
    }

    /// ExternalForcesModule::Cache (Update1a) for a system of the clone under `root` (its root Transform) on prefab
    /// object `go`: the managers' zones and fields with their Transforms' localToWorld, layers and windMain now.
    pub(crate) fn external_forces_cache(&self, x: &ExternalForces, root: Option<i32>, go: Option<crate::assets::db::ObjRef>) -> Vec<Force> {
        use crate::assets::db::Component;
        let at = |id: i32| self.component_at(id).unwrap_or_else(|| unimplemented!("mod-only: ExternalForcesModule::Cache: component {id} without its clone"));
        let layer = |inst: &crate::game::spawn::Instance, o: usize| self.world.layers.get(&inst.objects[o].game_object).map_or(inst.prefab.objects[o].layer, |&l| l as u32);
        let l2w = |inst: &crate::game::spawn::Instance, o: usize| crate::unity::transform::local_to_world(&inst.chain(o));
        let winds: Vec<Zone> = self
            .wind_zones
            .iter()
            .map(|&id| {
                let (inst, o, c) = at(id);
                let Component::WindZone(z) = &inst.prefab.objects[o].components[c].1 else { unreachable!("a WindZone id") };
                let mut zone = z.clone();
                if let Some(&m) = self.wind_main.get(&id) {
                    zone.main = m;
                }
                Zone { l2w: l2w(inst, o), layer: layer(inst, o), zone }
            })
            .collect();
        let fields: Vec<Field> = self
            .force_fields
            .iter()
            .map(|&id| {
                let (inst, o, c) = at(id);
                let Component::ForceField(f) = &inst.prefab.objects[o].components[c].1 else { unreachable!("a ParticleSystemForceField id") };
                // The field's Transform as it is now: a clone's physical object (the Stormbaan's `target` child, which
                // LightningGunBehaviour moves) poses it, not the clone's spawn pose.
                let (a, chain) = self.object_anchor(inst, o);
                let l2w = crate::unity::transform::local_to_world(&self.anchored_nodes(a, &chain));
                Field { id, l2w, layer: layer(inst, o), field: f.clone() }
            })
            .collect();
        let list: Vec<Option<i32>> = if x.influence_filter == 0 || x.influence.is_empty() {
            Vec::new()
        } else {
            let (Some((inst, ..)), Some(go)) = (root.and_then(|r| self.component_at(r)), go) else {
                unimplemented!("mod-only: ExternalForcesModule::Cache: an influence list on a system without its clone")
            };
            x.influence
                .iter()
                .map(|p| {
                    if p.path_id == 0 {
                        return None;
                    }
                    assert_eq!(p.file, 0, "an influence list field outside the prefab");
                    let r = crate::assets::db::ObjRef { file: go.file, path_id: p.path_id };
                    let &(o, c) = inst.prefab.index.get(&r).unwrap_or_else(|| unimplemented!("mod-only: ExternalForcesModule::Cache: an influence list field outside the clone"));
                    Some(inst.objects[o].components[c])
                })
                .collect()
        };
        cache(x, &winds, &fields, &list, self.time.time)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExternalForces, Force, ForceField};
    /// The sphere test on a box from bounds: a local box (-1..1) moved to x = 10 meets a sphere 2.5 away only with radius > 2.
    #[test]
    fn bounds_sphere() {
        let mut l2w = [0.0f32; 16];
        (l2w[0], l2w[5], l2w[10], l2w[15], l2w[12]) = (1.0, 1.0, 1.0, 1.0, 10.0);
        let b = super::world_bounds(&[-1.0, -1.0, -1.0, 1.0, 1.0, 1.0], false, &l2w);
        assert_eq!(b, [10.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert!(super::intersects(&b, [13.5, 0.0, 0.0], 2.6));
        assert!(!super::intersects(&b, [13.5, 0.0, 0.0], 2.4));
    }

    /// A directional zone along its z axis at time 0 (sum of the three cosines 3): vel += scalar * strength * dt * z.
    #[test]
    fn directional_wind() {
        let mut m = crate::assets::particles::MinMaxCurve::default();
        m.scalar = 2.0;
        let x = crate::assets::particles::ExternalForces { multiplier: m, influence_filter: 0, influence_mask: !0, influence: Vec::new() };
        let mut l2w = [0.0; 16];
        (l2w[0], l2w[5], l2w[10], l2w[15]) = (1.0, 1.0, 1.0, 1.0);
        let zone = crate::assets::particles::WindZone { game_object: Default::default(), enabled: true, mode: 0, radius: 20.0, main: 1.5, turbulence: 1.0, pulse_magnitude: 0.5, pulse_frequency: 0.01 };
        let s = super::strength(&zone, 0.0);
        assert_eq!(s, 1.5 * (0.5 * (3.0 * f32::from_bits(0x3eaa_7efa)) + 1.0));
        let forces = super::cache(&x, &[super::Zone { l2w, layer: 0, zone }], &[], &[], 0.0);
        let mut vel = [0.0, 0.0, 1.0];
        let lane = super::FieldLane { position: [0.0; 3], alive: 10.0, seed: 7, anim: [0.0; 3], size: [0.0; 3], uses_3d_size: false };
        super::apply(&x, &forces, &[None], &[0.0; 6], None, 0.02, 0, &lane, &mut vel);
        assert_eq!(vel, [0.0, 0.0, 1.0 + 2.0 * (s * 0.02)]);
    }

    /// Every system with ExternalForces on, every WindZone (182) and ParticleSystemForceField (330) in the game files
    /// (`cargo test external_forces_scan -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn external_forces_scan() {
        use crate::assets::{classes::Transform, db::ObjRef};
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        // "root/../name (layer, active)" of a component's GameObject.
        let path = |me: ObjRef| -> String {
            let Some(go) = crate::assets::components::game_object_of(db.data(me)).ok().and_then(|g| db.resolve(me.file, g)) else { return "?".into() };
            let g = db.game_object(go).unwrap();
            let mut names = vec![format!("{} (layer {} active {})", g.name, g.layer, g.active)];
            let mut t = g.components.iter().filter_map(|c| db.resolve(go.file, *c)).find(|c| db.class(*c) == 4);
            while let Some(tr) = t {
                let x = Transform::read(db.data(tr)).unwrap();
                t = db.resolve(tr.file, x.father);
                if let Some(f) = t {
                    let fg = crate::assets::components::game_object_of(db.data(f)).ok().and_then(|g| db.resolve(f.file, g)).unwrap();
                    names.push(db.game_object(fg).unwrap().name);
                }
            }
            names.reverse();
            names.join("/")
        };
        for (fi, f) in db.files.iter().enumerate() {
            let file = f.path.file_name().unwrap();
            for o in &f.objects {
                let me = ObjRef { file: fi, path_id: o.path_id };
                match o.class_id {
                    198 => {
                        let p = crate::assets::particles::ParticleSystem::read(f.data(o)).unwrap();
                        if let Some(x) = &p.external_forces {
                            let m = &x.multiplier;
                            println!("system {file:?} {} space {} mult {:?} filter {} mask {:x} list {}", path(me), p.space, (m.state, m.min_scalar, m.scalar), x.influence_filter, x.influence_mask, x.influence.len());
                            // What UpdateBounds reads: the renderer's mode and pivot, trails, lights, UV mode and sprites, the size curves.
                            let go = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|g| db.resolve(me.file, g)).unwrap();
                            let r = db.game_object(go).unwrap().components.iter().filter_map(|c| db.resolve(go.file, *c)).find(|c| db.class(*c) == 199);
                            let r = r.map(|r| crate::assets::components::ParticleRenderer::read(db.data(r)).unwrap());
                            let st = |c: &crate::assets::particles::MinMaxCurve| (c.state, c.min_scalar, c.scalar);
                            println!(
                                "  bounds: renderer {:?} trail {:?} lights {} uv {} sheet {:?} start {:?} {:?} {:?} size {:?} by_speed {:?} scaling {} max {}",
                                r.map(|r| (r.render_mode, r.pivot)), p.trail.as_ref().map(|t| (t.mode, t.world_space)), p.lights, p.uv_mode, p.sheet.as_ref().map(|s| (s.mode, s.sprites.len())),
                                st(&p.start_size), p.start_size_y.as_ref().map(st), p.start_size_z.as_ref().map(st), p.size.as_ref().map(|z| (st(&z.x), z.separate)),
                                p.size_by_speed.as_ref().map(|z| (st(&z.0.x), z.0.separate)), p.scaling, p.max_particles
                            );
                        }
                    }
                    182 => println!("wind {file:?} {} {:?}", path(me), crate::assets::particles::WindZone::read(f.data(o)).unwrap()),
                    330 => {
                        let x = crate::assets::particles::ForceField::read(f.data(o)).unwrap();
                        let st = |c: &crate::assets::particles::MinMaxCurve| (c.state, c.min_scalar, c.scalar, c.max.keys.len());
                        println!(
                            "field {file:?} {} on {} shape {} range {}..{} length {} focus {} rnd {:?} dir {:?} gravity {:?} rot {:?} attract {:?} drag {:?} vf {} vfs {:?} vfa {:?} drag_by {} {}",
                            path(me), x.enabled, x.shape, x.start_range, x.end_range, x.length, x.gravity_focus, x.rotation_randomness, x.direction.each_ref().map(st), st(&x.gravity), st(&x.rotation_speed),
                            st(&x.rotation_attraction), st(&x.drag), !x.vector_field.is_null(), st(&x.vector_field_speed), st(&x.vector_field_attraction), x.drag_by_size, x.drag_by_velocity
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    /// ApplyForceField<0> against UnityPlayer.dll's results on the same batches (particle_forces_dll.rs).
    #[test]
    fn force_field_dll() {
        use crate::assets::particles::MinMaxCurve;
        #[allow(dead_code)]
        enum C {
            S(u32),
            R(u32, u32),
        }
        struct L {
            pos: [u32; 3],
            vel: [u32; 3],
            anim: [u32; 3],
            size: [u32; 3],
            seed: u32,
            alive: u32,
        }
        struct K {
            start: u32,
            end: u32,
            focus: u32,
            rot: [u32; 2],
            gravity: C,
            speed: C,
            attraction: C,
            drag: C,
            by_size: bool,
            by_vel: bool,
            mult: C,
            dt: u32,
            a4: [[u32; 3]; 4],
            a5: [[u32; 3]; 4],
            #[allow(dead_code)]
            cur: bool,
            d3: bool,
            lanes: [L; 4],
            out: [[u32; 3]; 4],
        }
        let f = f32::from_bits;
        let v = |a: [u32; 3]| a.map(f32::from_bits);
        let m4 = |a: [[u32; 3]; 4]| a.map(v);
        let curve = |c: &C| match *c {
            C::S(s) => MinMaxCurve { scalar: f(s), ..Default::default() },
            C::R(lo, hi) => MinMaxCurve { state: 3, min_scalar: f(lo), scalar: f(hi), ..Default::default() },
        };
        let cases: &[K] = &include!("particle_forces_dll.rs");
        for (n, k) in cases.iter().enumerate() {
            let field = std::sync::Arc::new(ForceField {
                game_object: Default::default(),
                enabled: true,
                shape: 0,
                start_range: f(k.start),
                end_range: f(k.end),
                length: 0.0,
                gravity_focus: f(k.focus),
                rotation_randomness: k.rot.map(f32::from_bits),
                direction: Default::default(),
                gravity: curve(&k.gravity),
                rotation_speed: curve(&k.speed),
                rotation_attraction: curve(&k.attraction),
                drag: curve(&k.drag),
                vector_field: Default::default(),
                vector_field_speed: Default::default(),
                vector_field_attraction: Default::default(),
                drag_by_size: k.by_size,
                drag_by_velocity: k.by_vel,
            });
            let prep = super::setup(&field, m4(k.a4), m4(k.a5));
            let x = ExternalForces { multiplier: curve(&k.mult), influence_filter: 0, influence_mask: 0, influence: Vec::new() };
            let forces = [Force::Field { l2w: [0.0; 16], field: field.clone() }];
            let fields = [Some(prep)];
            let live = k.lanes.iter().fold(0, |b, l| b | super::live_bits(&fields, l.seed, v(l.pos)));
            for (i, l) in k.lanes.iter().enumerate() {
                let lane = super::FieldLane { position: v(l.pos), alive: f(l.alive), seed: l.seed, anim: v(l.anim), size: v(l.size), uses_3d_size: k.d3 };
                let mut vel = v(l.vel);
                super::apply(&x, &forces, &fields, &[0.0; 6], None, f(k.dt), live, &lane, &mut vel);
                assert_eq!(vel.map(f32::to_bits), k.out[i], "case {n} lane {i}");
            }
        }
    }

}

/// The map prefab a recording's header names (`map X`, the Map asset's name); None without that line.
pub(super) fn recorded_map_prefab(db: &mut crate::assets::db::AssetDb, header: &[String]) -> Option<crate::assets::db::Prefab> {
    use crate::assets::mono::Value;
    let map = header.iter().find_map(|l| l.strip_prefix("map "))?;
    let mut root = None;
    for m in db.find_scripts("Map").expect("the Map assets") {
        let Some((_, mb, v)) = db.mono(m).expect("a Map asset") else { continue };
        if mb.name == map {
            root = v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p));
        }
    }
    Some(db.prefab(root.unwrap_or_else(|| panic!("no map {map:?}"))).expect("the map prefab"))
}
