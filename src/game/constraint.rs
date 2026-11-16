//! UnityEngine.Animations.ParentConstraint as the PreLateUpdate ConstraintManager moves it. ConstraintManager::Update
//! (180106e90) takes each constraint in the manager (an active and enabled one) whose GetConstraintActive (vtable
//! +0x118) is true and whose GetSources (+0x138) is not empty. ConstraintComponentData's ParentConstraint constructor
//! (180d618c0) reads translationAtRest and rotationAtRest, the weight clamped to [0, 1], the translation and rotation
//! axes (the stored ones: vtable +0x128 returns true for a ParentConstraint, and the evaluation mode at +0x168 is 0 in
//! the player) and, through InitConstraintSources (180d67160), each source whose Transform still exists with its weight
//! (the offsets stay by index). ConstraintJob::ComputeConstraints (parent branch 180d63a26) then works out every
//! constraint's local position and rotation, which the manager stores with SetLocalTRS (1800d94a0) afterwards.

use super::Game;

/// ConstraintSource.sourceTransform: a PhysicalBehaviour's Transform (kept live by the physics write-back) or a clone
/// object's Transform, by its instance id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstraintSource {
    Physical(usize),
    Object(i32),
}

/// The constrained Transform: a `Game::renderers` entry (PinBehaviour's SpriteChild) or a clone object, by any of
/// its components' instance ids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstrainedTarget {
    Renderer(usize),
    Object(i32),
}

/// One source: the Transform, its weight, and the translation and rotation offsets at its index.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub transform: ConstraintSource,
    pub weight: f32,
    pub translation_offset: [f32; 3],
    pub rotation_offset: [f32; 3],
}

/// A ParentConstraint component.
#[derive(Clone, Debug)]
pub struct ParentConstraint {
    /// The component's instance id.
    pub id: i32,
    pub target: ConstrainedTarget,
    pub weight: f32,
    /// Axis flags (X 1, Y 2, Z 4).
    pub translation_axis: u8,
    pub rotation_axis: u8,
    pub translation_at_rest: [f32; 3],
    /// Euler degrees.
    pub rotation_at_rest: [f32; 3],
    pub active: bool,
    pub sources: Vec<Source>,
}

impl ParentConstraint {
    /// AddComponent<ParentConstraint>(): ParentConstraint::Reset, weight 1, all axes, inactive, no sources.
    pub fn new(id: i32, target: ConstrainedTarget) -> Self {
        ParentConstraint { id, target, weight: 1.0, translation_axis: 7, rotation_axis: 7, translation_at_rest: [0.0; 3], rotation_at_rest: [0.0; 3], active: false, sources: Vec::new() }
    }

    /// A prefab's ParentConstraint: its serialized weight, axes, m_Active and sources (none of the prefabs' sources
    /// point at a Transform the rewrite resolves, so any is unimplemented).
    pub fn from_data(id: i32, target: ConstrainedTarget, d: &crate::assets::components::ParentConstraintData) -> Self {
        if !d.sources.is_empty() || !d.translation_offsets.is_empty() || !d.rotation_offsets.is_empty() {
            unimplemented!("mod-only: a prefab ParentConstraint with serialized sources or offsets");
        }
        if !d.enabled {
            unimplemented!("mod-only: a disabled prefab ParentConstraint (outside the ConstraintManager until enabled)");
        }
        let axis = |a: &[bool]| a.iter().enumerate().fold(0u8, |m, (i, &b)| m | ((b as u8) << i));
        ParentConstraint {
            id,
            target,
            weight: d.weight,
            translation_axis: axis(&d.affect[..3]),
            rotation_axis: axis(&d.affect[3..]),
            translation_at_rest: d.translation_at_rest,
            rotation_at_rest: d.rotation_at_rest,
            active: d.active,
            sources: Vec::new(),
        }
    }

    /// ParentConstraint.AddSource(source): appended with zero offsets (the offset arrays grow with the sources).
    pub fn add_source(&mut self, transform: ConstraintSource, weight: f32) -> usize {
        self.sources.push(Source { transform, weight, translation_offset: [0.0; 3], rotation_offset: [0.0; 3] });
        self.sources.len() - 1
    }
}

impl Game {
    /// A constraint source's world position and rotation.
    fn constraint_source_pose(&mut self, s: ConstraintSource) -> Option<([f32; 3], [f32; 4])> {
        match s {
            ConstraintSource::Physical(p) => {
                let ph = &self.physicals[p];
                (!ph.destroyed).then_some((ph.transform.position, ph.transform.rotation))
            }
            ConstraintSource::Object(id) => {
                if self.destroyed_components.contains(&id) {
                    return None;
                }
                Some(self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o))))
            }
        }
    }

    /// Whether the constrained object is active (ConstraintManager only holds constraints whose GameObject is active
    /// and which are enabled).
    fn constraint_in_manager(&mut self, c: &ParentConstraint) -> bool {
        if self.destroyed_components.contains(&c.id) {
            return false;
        }
        match c.target {
            ConstrainedTarget::Renderer(_) => true,
            ConstrainedTarget::Object(id) => self.with_component_instance(id, |_, inst, o| inst.objects[o].active),
        }
    }

    /// The constrained Transform's local position and rotation, or None when it has a parent.
    fn constrained_local(&mut self, t: ConstrainedTarget) -> Option<([f32; 3], [f32; 4])> {
        match t {
            ConstrainedTarget::Renderer(r) => {
                let chain = &self.renderers[r].chain;
                (chain.len() == 1).then(|| ([chain[0].t[0], chain[0].t[1], chain[0].t[2]], chain[0].q))
            }
            ConstrainedTarget::Object(id) => self.with_component_instance(id, |_, inst, o| {
                let trs = inst.objects[o].trs;
                inst.objects[o].parent.is_none().then_some(([trs.t[0], trs.t[1], trs.t[2]], trs.q))
            }),
        }
    }

    /// PreLateUpdate.ConstraintManagerUpdate: every ParentConstraint in creation order, then each result through
    /// SetLocalTRS (the position, and the rotation normalized; the scale stays).
    pub(super) fn constraints_update(&mut self) {
        let mut results = Vec::new();
        for i in 0..self.parent_constraints.len() {
            let c = self.parent_constraints[i].clone();
            if !c.active || c.sources.is_empty() || !self.constraint_in_manager(&c) {
                continue;
            }
            // InitConstraintSources leaves a destroyed (null) Transform out; the offsets are read by the kept index.
            let mut sources = Vec::new();
            for s in &c.sources {
                if let Some((position, rotation)) = self.constraint_source_pose(s.transform) {
                    let n = sources.len();
                    let (translation_offset, rotation_offset) = (c.sources[n].translation_offset, c.sources[n].rotation_offset);
                    sources.push(SourcePose { position, rotation, weight: s.weight, translation_offset, rotation_offset });
                }
            }
            let Some(local) = self.constrained_local(c.target) else {
                unimplemented!("mod-only: a ParentConstraint on a Transform with a parent (the job's world-to-local steps 180171750 / 1807f26a0)");
            };
            results.push((c.target, compute_parent(&c, &sources, local)));
        }
        for (target, (p, q)) in results {
            let q = crate::unity::transform::normalize(q);
            match target {
                ConstrainedTarget::Renderer(r) => {
                    let n = &mut self.renderers[r].chain[0];
                    n.t = [p[0], p[1], p[2], n.t[3]];
                    n.q = q;
                    self.renderer_object(r, |inst, o| {
                        inst.set_local_position(o, p);
                        inst.set_local_rotation(o, q);
                    });

                }
                ConstrainedTarget::Object(id) => self.with_component_instance(id, |_, inst, o| {
                    inst.set_local_position(o, p);
                    inst.set_local_rotation(o, q);
                }),
            }
        }
    }
}

/// A kept source: its Transform's world position and rotation (GetPositionAndRotation, 1800df190), weight and offsets.
struct SourcePose {
    position: [f32; 3],
    rotation: [f32; 4],
    weight: f32,
    translation_offset: [f32; 3],
    rotation_offset: [f32; 3],
}

// The job's SIMD maths, lane for lane (rsqrtps / rcpps as src/unity/sse.rs answers them).
type V = [f32; 4];

fn k(bits: u32) -> f32 {
    f32::from_bits(bits)
}
fn lanes(a: V, b: V, f: impl Fn(f32, f32) -> f32) -> V {
    std::array::from_fn(|i| f(a[i], b[i]))
}
fn add(a: V, b: V) -> V {
    lanes(a, b, |x, y| x + y)
}
fn sub(a: V, b: V) -> V {
    lanes(a, b, |x, y| x - y)
}
fn mul(a: V, b: V) -> V {
    lanes(a, b, |x, y| x * y)
}
/// pshufd.
fn sh(v: V, i: u32) -> V {
    std::array::from_fn(|n| v[(i >> (2 * n) & 3) as usize])
}
fn sign(x: f32) -> u32 {
    x.to_bits() & 0x8000_0000
}
fn flip(x: f32, s: u32) -> f32 {
    f32::from_bits(x.to_bits() ^ s)
}
/// Each lane's horizontal sum: (v1 + v0) + (v3 + v2) for lane 0, rotated for the others.
fn hsum(v: V) -> V {
    let t = add(sh(v, 0x39), v);
    add(sh(t, 0x4e), t)
}
/// Per x/y/z lane: `a` where the axis bit is set, else `b` (lane 3 from `b`).
fn select(mask: u8, a: V, b: V) -> V {
    std::array::from_fn(|i| if i < 3 && mask >> i & 1 != 0 { a[i] } else { b[i] })
}

/// rsqrtps of each lane's sum of squares with one Newton step: v * (3.0000002 - r*n*r) * 0.5r.
fn normalize_fast(v: V) -> V {
    let n = hsum(mul(v, v));
    std::array::from_fn(|i| v[i] * inv_sqrt(n[i]))
}

/// (3.0000002 - r*n*r) * 0.5r for r = min(rsqrtps(n) * 0.99999982, FLT_MAX).
fn inv_sqrt(n: f32) -> f32 {
    let r = crate::unity::sse::rsqrt(n) * k(0x3f7ffffd);
    let r = if r < f32::MAX { r } else { f32::MAX };
    (k(0x40400001) - r * n * r) * (0.5 * r)
}

/// The job's polynomial sin(2*pi*u) for u in [-0.25, 0.25].
fn sin_turn(u: f32) -> f32 {
    let s2 = u * u;
    let s4 = s2 * s2;
    ((s2 * k(0xc2992322) + k(0x42a33422)) * s4 + (s2 * k(0xc2255ddc) + k(0x40c90fda)) + s4 * s4 * k(0x421ea0cd)) * u
}

/// |a - cvtps2dq(a)|: the distance to the nearest whole turn.
fn turn_frac(a: f32) -> f32 {
    let r = if a.abs() < 2147483648.0 { a.round_ties_even() as i32 } else { i32::MIN };
    (a - r as f32).abs()
}

/// Euler half angles in turns (radians * 0.5 / 2pi) to the job's quaternion (z, then x, then y), not normalized.
fn euler_to_quat(turns: V) -> V {
    let c: V = turns.map(|a| sin_turn(0.25 - turn_frac(a)));
    let s: V = turns.map(|a| sin_turn(0.25 - turn_frac(a + -0.25)));
    let v = [s[0] * c[2], s[0] * s[2], c[0] * s[2], c[0] * c[2]];
    let x1 = mul(sh(mul([s[1]; 4], [1.0, 1.0, -1.0, 1.0]), 0x4e), v);
    let x0 = mul(mul([c[1]; 4], v), [1.0, -1.0, 1.0, 1.0]);
    sh(add(sh(x0, 0x4e), x1), 0x4e)
}

/// Euler degrees to turns of the half angle.
fn degree_turns(e: [f32; 3]) -> V {
    [e[0], e[1], e[2], 0.0].map(|x| x * k(0x3c8efa35) * 0.5 * k(0x3e22f983))
}

/// A matrix's rotation columns back to a quaternion (180d673e0), normalized with `normalize_fast`.
fn matrix_to_quat(c0: V, c1: V, c2: V) -> V {
    let ([m00, m10, m20, _], [m01, m11, m21, _], [m02, m12, m22, _]) = (c0, c1, c2);
    let v = if 0.0 <= m00 {
        let t = m22 + m11;
        if 0.0 <= t { [m21 - m12, m02 - m20, m10 - m01, (1.0 + m00) + t] } else { [(1.0 + m00) - t, m10 + m01, m02 + m20, m21 - m12] }
    } else {
        let t = m11 - m22;
        if 0.0 <= t { [m10 + m01, (1.0 - m00) + t, m21 + m12, m02 - m20] } else { [m02 + m20, m21 + m12, (1.0 - m00) - t, m10 - m01] }
    };
    normalize_fast(v)
}

/// The job's atan for x >= 0: a polynomial in x, or in 1/x (rcpps, two Newton steps) taken from pi/2 when x > 1.
fn atan_abs(x: f32) -> f32 {
    let r0 = crate::unity::sse::rcp(x);
    let r1 = (k(0x40000002) - r0 * x) * r0;
    let r2 = r1 * (2.0 - r1 * x);
    let big = 1.0 < x;
    let t = if !big { x } else if r2 == r2 { r2 } else { r0 };
    let t2 = t * t;
    let t4 = t2 * t2;
    let t6 = t4 * t2;
    let t8 = t6 * t2;
    let t10 = t8 * t2;
    let t12 = t10 * t2;
    let p = k(0xbeaaa1a8) * t2 + k(0x3f7ffff7) + k(0x3e4b9639) * t4 + k(0xbe0a1c0f) * t6 + k(0x3dabc4bf) * t8 + k(0xbd179b18) * t10 + k(0x3c00d1c0) * t12;
    let a = t * p;
    if big { k(0x3fc90fdb) - a } else { a }
}

/// The job's atan2: pi minus the angle when x has its sign bit set, then y's sign.
fn atan2(y: f32, x: f32) -> f32 {
    let a = atan_abs((y / x).abs());
    flip(if x.is_sign_negative() { k(0x40490fdb) - a } else { a }, sign(y))
}

/// QuaternionToEuler (180ec5680) in the order the job asks for (4: z, then x, then y): radians, lane 3 = x.
fn quat_to_euler(q: V) -> V {
    let two = [2.0; 4];
    let x4 = mul(mul(q, sh(q, 0xff)), two);
    let x1 = mul(mul(q, sh(q, 0xd2)), two);
    let qq = mul(q, q);
    let x6 = sh(x1, 0xc9);
    let s = x1[2] - x4[0];
    if !(s * s < k(0x3f7fffbc)) {
        unimplemented!("mod-only: QuaternionToEuler at an x angle of +-90 degrees (the gimbal branch 180ec63eb; a 2D rotation never reaches it)");
    }
    let a = x6[0] + x4[2];
    let b = x6[2] + x4[1];
    let c = qq[1] + qq[3] - qq[0] - qq[2];
    let d = qq[3] + qq[2] - qq[0] - qq[1];
    let x = -flip(atan_abs((inv_sqrt(1.0 - s * s) * s).abs()), sign(s));
    [x, atan2(b, d), atan2(a, c), x]
}

/// The quaternion lerp from `a` toward `q` (sign-aligned to `a` per lane) by `t`, renormalized.
fn lerp_toward(a: V, q: V, t: f32) -> V {
    let s = hsum(mul(a, q)).map(sign);
    let q: V = std::array::from_fn(|i| flip(q[i], s[i]));
    normalize_fast(add(mul(sub(q, a), [t; 4]), a))
}

/// c0*p.x + (c1*p.y + c2*p.z) + c3.
fn transform_point(c: &[V; 4], p: V) -> V {
    add(add(mul(c[0], [p[0]; 4]), add(mul(c[1], [p[1]; 4]), mul(c[2], [p[2]; 4]))), c[3])
}

/// ConstraintJob::ComputeConstraints' parent branch for a constrained Transform without a parent: its new local
/// position and (not yet normalized) rotation from the kept sources and its current local pose.
fn compute_parent(c: &ParentConstraint, sources: &[SourcePose], (local_t, local_q): ([f32; 3], V)) -> ([f32; 3], V) {
    use crate::unity::transform::{normalize, quat_mul, rotation_columns};
    let weight = c.weight.clamp(0.0, 1.0);
    // Each source's matrix: its rotation's columns and its position.
    let matrices: Vec<[V; 4]> = sources
        .iter()
        .map(|s| {
            let [c0, c1, c2] = rotation_columns(s.rotation);
            [c0, c1, c2, [s.position[0], s.position[1], s.position[2], 0.0]]
        })
        .collect();
    let offset_rotation = |s: &SourcePose| normalize(euler_to_quat(degree_turns(s.rotation_offset)));
    // Position: the weighted sum of each source matrix times its offset matrix, at the origin.
    let mut position = [0.0f32; 4];
    for (s, m) in sources.iter().zip(&matrices) {
        let col = |oc: V| add(mul(m[0], [oc[0]; 4]), add(mul(m[1], [oc[1]; 4]), mul(m[2], [oc[2]; 4])));
        let [o0, o1, o2] = rotation_columns(offset_rotation(s));
        let t = s.translation_offset;
        let offset = [col(o0), col(o1), col(o2), transform_point(m, [t[0], t[1], t[2], 0.0])];
        let p = mul(transform_point(&offset, [0.0; 4]), [s.weight; 4]);
        for i in 0..3 {
            position[i] = p[i] + position[i];
        }
    }
    // Rotation: from identity, a lerp toward each source's rotation times its offset (the first one at weight 1).
    let mut rotation = [0.0, 0.0, 0.0, 1.0];
    let mut first = true;
    for (s, m) in sources.iter().zip(&matrices) {
        if 1e-6 > s.weight {
            continue;
        }
        let r = quat_mul(matrix_to_quat(m[0], m[1], m[2]), offset_rotation(s));
        rotation = lerp_toward(rotation, r, if first { 1.0 } else { s.weight });
        first = false;
    }
    if !sources.is_empty() {
        rotation = normalize(rotation);
    }
    // Toward the rest pose by the weight, then the unconstrained axes keep the current local pose (its Euler angles
    // through degrees and back).
    let rest = c.translation_at_rest;
    let rest = [rest[0], rest[1], rest[2], 0.0];
    let position = add(mul(sub(position, rest), [weight; 4]), rest);
    let position = select(c.translation_axis, position, [local_t[0], local_t[1], local_t[2], 0.0]);
    let rotation = lerp_toward(euler_to_quat(degree_turns(c.rotation_at_rest)), rotation, weight);
    let current = quat_to_euler(local_q).map(|x| k(0x42652ee1) * x * k(0x3c8efa35));
    let euler = select(c.rotation_axis, quat_to_euler(rotation), current);
    let rotation = euler_to_quat(euler.map(|x| x * 0.5 * k(0x3e22f983)));
    // The manager's output slot starts at position zero and rotation identity.
    ([position[0] + 0.0, position[1] + 0.0, position[2] + 0.0], quat_mul([0.0, 0.0, 0.0, 1.0], rotation))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(b: [u32; 4]) -> V {
        b.map(f32::from_bits)
    }

    /// Answers of the game's own 180ec5680 (order 4) and 180d673e0, run natively on the 9950X3D.
    #[test]
    fn job_maths_match_the_game() {
        for line in include_str!("constraint_vectors.txt").lines() {
            let w: Vec<u32> = line[2..].split(' ').map(|x| u32::from_str_radix(x, 16).unwrap()).collect();
            let got = match &line[..1] {
                "E" => quat_to_euler(v([w[0], w[1], w[2], w[3]])),
                _ => matrix_to_quat(v([w[0], w[1], w[2], w[3]]), v([w[4], w[5], w[6], w[7]]), v([w[8], w[9], w[10], w[11]])),
            };
            let want = &w[w.len() - 4..];
            assert_eq!(got.map(f32::to_bits), [want[0], want[1], want[2], want[3]], "{line}");
        }
    }
}
