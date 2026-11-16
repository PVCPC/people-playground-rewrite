//! The native ParticleSystem velocity-family modules (UnityPlayer 2020.3, asm-read), one particle (lane) at a time:
//! InitialModule::Update's gravity, VelocityModule, InheritVelocityModule, ForceModule, ClampVelocityModule,
//! StartVelocity and StartLifetimeByEmitterSpeed. Every lane of the SIMD templates runs the same maths on its own
//! particle, so one lane is exact. Vectors are [x, y, z]; matrices are three columns.
#![cfg_attr(not(test), allow(dead_code))]
use crate::assets::particles::{Clamp, Force, MinMaxCurve, Velocity};
use crate::game::particle_rand::unit;
use crate::game::particle_sim::{evaluate, maxps, minps};
use crate::unity::Random as Rand;

pub type V3 = [f32; 3];
pub type M3 = [V3; 3];

pub const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The first `N` Random01 draws of a per-particle RandN lane: RandN::RandN (0x1801b32a0) seeds lane i with
/// Random::new(seed_i) and no lane offset.
pub fn draws<const N: usize>(seed: u32, k: u32) -> [f32; N] {
    let mut r = Rand::new(seed.wrapping_add(k) as i32);
    [0; N].map(|_| unit(r.next()))
}

/// t = max(aliveTimePercent * 0.01, 0), the modules' curve time.
fn life_t(alive: f32) -> f32 {
    maxps(alive * f32::from_bits(0x3c23d70a), 0.0)
}

/// The per-axis value of a module curve: Constant, TwoConstants (max - min) * r + min, curves at t with r.
fn axis(c: &MinMaxCurve, t: f32, r: f32) -> f32 {
    evaluate(c, t, r)
}

/// m * v as the module templates do it: c = m0.c * x + (y * m1.c + m2.c * z).
pub fn mul(m: &M3, v: V3) -> V3 {
    [0, 1, 2].map(|c| m[0][c] * v[0] + (v[1] * m[1][c] + m[2][c] * v[2]))
}

/// GetTransformationMatrix (0x1801a7f20): from a module's space to the simulation space. `l2w`/`w2l` the 3x3 of
/// localToWorld/worldToLocal, `scale` the renderScale.
pub fn transformation(sim_world: bool, module_world: bool, l2w: &M3, w2l: &M3, scale: V3) -> M3 {
    match (sim_world, module_world) {
        (true, true) => [[scale[0], 0.0, 0.0], [0.0, scale[1], 0.0], [0.0, 0.0, scale[2]]],
        (false, false) => IDENTITY,
        (true, false) => *l2w,
        (false, true) => [0, 1, 2].map(|c| w2l[c].map(|v| v * scale[c])),
    }
}

/// InitialModule::Update's gravity (0x1802026d0) for the whole step: `g` = GetGravity (0 when the modifier's scalar
/// is 0, else Physics.gravity), `t` = state.t / length. States 0 and 1 give one vector for every particle (None when
/// |g k dt|^2 <= 1e-8 skips the add).
pub fn gravity_shared(m: &MinMaxCurve, g: V3, t: f32, dt: f32, w2l: Option<&M3>) -> Option<V3> {
    let sq = (g[2] * g[2] + 0.0) + (g[0] * g[0] + g[1] * g[1]);
    if sq <= f32::from_bits(0x322bcc76) {
        return None;
    }
    let k = evaluate(m, t, 1.0) * dt;
    let v = [k * g[0], g[1] * k, k * g[2]];
    Some(w2l.map_or(v, |w| local_gravity(w, v)))
}

/// worldToLocal * g as InitialModule::Update associates it.
fn local_gravity(w: &M3, v: V3) -> V3 {
    [
        (w[1][0] * v[1] + w[2][0] * v[2]) + w[0][0] * v[0],
        (w[1][1] * v[1] + w[2][1] * v[2]) + v[0] * w[0][1],
        (v[1] * w[1][2] + v[2] * w[2][2]) + v[0] * w[0][2],
    ]
}

/// Gravity states 2 and 3: g * dt (in local space through worldToLocal) times the curve at t with the particle's draw
/// from seed + 0xe2b7c3c3.
pub fn gravity_random(m: &MinMaxCurve, g: V3, t: f32, dt: f32, w2l: Option<&M3>, seed: u32, vel: &mut V3) {
    let gd = g.map(|v| v * dt);
    let gd = w2l.map_or(gd, |w| local_gravity(w, gd));
    let [r] = draws::<1>(seed, 0xe2b7c3c3);
    let k = evaluate(m, t, r);
    for c in 0..3 {
        vel[c] += k * gd[c];
    }
}

/// VelocityModule::Update (0x180e73720): the linear part (UpdateTpl<0..4>): animatedVelocity += M * (x, y, z), the
/// axes at t with draws 1, 2, 3 from seed + 0xe0fbd834; then the orbital part (`orbital`). `sys` the system's
/// localToWorld and worldToLocal, `dt` the lane's. The speed modifier's non-constant states are not rewritten.
#[allow(clippy::too_many_arguments)]
pub fn velocity(v: &Velocity, m: &M3, sys: (bool, &[f32; 16], &[f32; 16]), seed: u32, alive: f32, position: V3, dt: f32, anim: &mut V3) {
    let state = v.axes[0].state;
    if v.axes.iter().any(|a| a.state != state) {
        return; // the native logs "Particle Velocity curves must all be in the same mode" and does nothing
    }
    if v.speed_modifier.state != 0 || v.speed_modifier.scalar != 1.0 {
        unimplemented!("mod-only: VelocityModule speedModifier other than the constant 1 (UpdateSpeedTpl / RandN seed + 0xba821f34): all 849 game systems keep it at 1, checked by particle_inventory");
    }
    if state >= 2 || v.axes.iter().any(|a| a.scalar != 0.0) {
        linear(v, m, seed, alive, anim);
    }
    if v.orbital.iter().any(|c| c.scalar != 0.0) || v.radial.scalar != 0.0 || matches!(v.orbital[0].state, 2 | 3) {
        orbital(v, sys, position, dt, anim);
    }
}

/// VelocityModule::Update's orbital part, constant orbital, offset and radial (UpdateOrbitalTpl<0> 0x180e6e780 ->
/// UpdateOrbitalAndRadialTpl<0,0,0> 0x180e6cb80 -> ApplyOrbitalAndRadialVelocity 0x180e72b50). GetTransformationMatrices
/// (0x1801a7c60): in world simulation the position goes to local space by worldToLocal and the result back by
/// localToWorld's 3x3, else both are the identity. The position less the offset turns by orbital * dt (the module's
/// sincos), moves radial * dt along itself, and the change times 1/dt (rcpps and two Newton steps, 0 at dt <= 1e-6)
/// goes to animatedVelocity.
fn orbital(v: &Velocity, (world, l2w, w2l): (bool, &[f32; 16], &[f32; 16]), position: V3, dt: f32, anim: &mut V3) {
    if v.orbital.iter().chain(&v.offset).chain([&v.radial]).any(|c| c.state != 0) {
        unimplemented!("mod-only: VelocityModule orbital/offset/radial other than constants (UpdateOrbitalAndRadialTpl<..> 0x180e62b20..)");
    }
    const ID: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let (rs, af) = if world { (l2w, w2l) } else { (&ID, &ID) };
    let rc = crate::unity::sse::rcp(dt);
    let a = (f32::from_bits(0x4000_0002) - dt * rc) * rc;
    let b = a * (2.0 - dt * a);
    let inv_dt = if f32::from_bits(0x3586_37bd) < dt { b } else { 0.0 };
    use crate::game::particle_shape::sincos;
    let o = v.orbital.each_ref().map(|c| c.scalar * dt);
    let ((sx, cx), (sy, cy), (sz, cz)) = (sincos(o[0]), sincos(o[1]), sincos(o[2]));
    let ma = (sy * sx) * sz + cz * cy;
    let mb = (sx * cz) * sy - sz * cy;
    let mc = sx * (cz * cy) + sy * sz;
    let md = sy * (-cz) + (sx * cy) * sz;
    let [x, y, z] = position;
    let q: V3 = std::array::from_fn(|i| x * af[i] + (y * af[4 + i] + (z * af[8 + i] + af[12 + i])));
    let [dx, dy, dz] = [0, 1, 2].map(|i| q[i] - v.offset[i].scalar);
    let rx = (dy * mb + dz * (sy * cx)) + ma * dx;
    let ry = (dy * (cx * cz) + dz * (-sx)) + (sz * cx) * dx;
    let rz = (dy * mc + dz * (cx * cy)) + md * dx;
    let l2 = (ry * ry + rz * rz) + rx * rx;
    let inv = rsqrt_newton(l2);
    let n = if f32::from_bits(0x0da2_4260) < l2 { [inv * rx, inv * ry, inv * rz] } else { [0.0; 3] };
    let k = dt * v.radial.scalar;
    let d = [(rx + k * n[0]) - dx, (ry + k * n[1]) - dy, (rz + k * n[2]) - dz];
    for i in 0..3 {
        let w = rs[i] * d[0] + (rs[4 + i] * d[1] + rs[8 + i] * d[2]);
        anim[i] += w * inv_dt;
    }
}

/// VelocityModule's linear part.
fn linear(v: &Velocity, m: &M3, seed: u32, alive: f32, anim: &mut V3) {
    let t = life_t(alive);
    let r = draws::<3>(seed, 0xe0fbd834);
    let n = mul(m, [0, 1, 2].map(|i| axis(&v.axes[i], t, r[i])));
    for c in 0..3 {
        anim[c] = n[c] + anim[c];
    }
}

/// ForceModule::Update without randomizePerFrame (0x1801e1460, UpdateTpl<0..4>): velocity += (M * (x, y, z)) * dt,
/// the axes at t with draws 1, 2, 3 from seed + 0x12460f3b.
pub fn force(f: &Force, m: &M3, seed: u32, alive: f32, dt: f32, vel: &mut V3) {
    if f.randomize_per_frame {
        unimplemented!("mod-only: ForceModule randomizePerFrame (UpdateRandomTpl, the module's own RandN)");
    }
    let t = life_t(alive);
    let r = draws::<3>(seed, 0x12460f3b);
    let n = mul(m, [0, 1, 2].map(|i| axis(&f.axes[i], t, r[i])));
    for c in 0..3 {
        vel[c] = n[c] * dt + vel[c];
    }
}

/// InheritVelocityModule::Update (0x180202580): world simulation only, mode Current, or mode Initial with a curve
/// (state 1/2, from initialVelocity). animatedVelocity += emitterVelocity * k, k at t with one draw from
/// seed + 0x33e627.
pub fn inherit(mode: i32, c: &MinMaxCurve, emitter_velocity: V3, seed: u32, anim: &mut V3) {
    if mode == 0 {
        if c.state == 0 || c.state == 3 {
            return; // StartVelocity's
        }
        unimplemented!("mod-only: InheritVelocity Initial with a curve (initialVelocity stream, UpdateTpl<1/2/4> 0x1801fbbe0)");
    }
    if c.state != 3 {
        unimplemented!("mod-only: InheritVelocity Current other than TwoConstants (UpdateTpl<0/1/2/4> 0x1801fd1b0...)");
    }
    let [r] = draws::<1>(seed, 0x33e627);
    let k = (c.scalar - c.min_scalar) * r + c.min_scalar;
    for i in 0..3 {
        anim[i] = emitter_velocity[i] * k + anim[i];
    }
}

/// normalize's scale for a squared length (rsqrtps, one Newton step): (3.0000002 - (sq * r) * r) * (0.5 * r).
pub(crate) fn rsqrt_newton(sq: f32) -> f32 {
    let r = minps(crate::unity::sse::rsqrt(sq) * f32::from_bits(0x3f7ffffd), f32::MAX);
    (f32::from_bits(0x40400001) - (sq * r) * r) * (0.5 * r)
}

/// The dampen factor of ClampVelocityModule::Update (0x1801f03d0): 1 - (1 - dampen)^(30 dt).
pub fn dampen_factor(dampen: f32, dt: f32) -> f32 {
    1.0 - crate::unity::crt::powf(1.0 - dampen, 30.0 * dt)
}

/// ClampVelocityModule::Update for one particle: the magnitude limit (MagnitudeUpdateTpl<0/3> 0x1801e72c0/0x1801e6c60,
/// draw from seed + 0x13371337) when dampen > 0, then the drag (inline state 0, DragUpdateTpl<1/4> 0x1801e2950, state
/// 3 from seed + 0xa6790296) when its scalar is not 0. `drag_scalar` is the drag's scalar after the scripts' writes;
/// `size` the currentSize (or startSize) x, y, z; `uses_3d` uses3DSize.
#[allow(clippy::too_many_arguments)]
pub fn clamp(c: &Clamp, drag_scalar: f32, dt: f32, seed: u32, alive: f32, size: V3, uses_3d: bool, vel: &mut V3, anim: V3) {
    if c.dampen > 0.0 {
        let f = dampen_factor(c.dampen, dt);
        let limit = match c.magnitude.state {
            0 => c.magnitude.scalar,
            3 => {
                let [r] = draws::<1>(seed, 0x13371337);
                r * (c.magnitude.scalar - c.magnitude.min_scalar) + c.magnitude.min_scalar
            }
            _ => unimplemented!("mod-only: ClampVelocity magnitude curves (MagnitudeUpdateTpl<1/2/4>)"),
        };
        let total = [0, 1, 2].map(|i| vel[i] + anim[i]);
        let sq = (total[1] * total[1] + total[2] * total[2]) + total[0] * total[0];
        let n = rsqrt_newton(sq);
        let s = sq.sqrt();
        let len = s.abs();
        let clamped = if limit < len { (limit - len) * f + len } else { len };
        let sign = (s > 0.0) as i32 - (s < 0.0) as i32;
        let k = clamped * sign as f32;
        let ok = f32::from_bits(0x0da24260) < sq;
        for i in 0..3 {
            vel[i] = k * if ok { total[i] * n } else { 0.0 } - anim[i];
        }
    }
    if drag_scalar != 0.0 {
        let d = MinMaxCurve { scalar: drag_scalar, ..c.drag.clone() };
        let drag = match d.state {
            0 => d.scalar,
            3 => {
                let [r] = draws::<1>(seed, 0xa6790296);
                (r * (d.scalar - d.min_scalar)) + d.min_scalar
            }
            1 => evaluate(&d, life_t(alive), 0.0),
            _ => unimplemented!("mod-only: ClampVelocity drag state 2 (two curves)"),
        };
        let total = [0, 1, 2].map(|i| vel[i] + anim[i]);
        let sq = (total[1] * total[1] + total[2] * total[2]) + total[0] * total[0];
        let size = if uses_3d { maxps(size[0], maxps(size[1], size[2])) } else { size[0] };
        let radius = size * 0.5;
        let by_size = if c.drag_by_size { (radius * std::f32::consts::PI) * radius } else { 1.0 };
        let by_velocity = if c.drag_by_velocity { sq } else { 1.0 };
        let speed = sq.sqrt();
        let new = maxps(0.0, speed - ((by_size * drag) * by_velocity) * dt);
        let ok = f32::from_bits(0x26901d7d) < speed;
        for i in 0..3 {
            vel[i] = new * if ok { total[i] / speed } else { 0.0 } - anim[i];
        }
    }
}

/// StartVelocity (0x1801c41e0): velocity = shape direction (already M * dir) * startSpeed(t, draw from
/// seed + 0x96aa4de3); in world space with inherit mode Initial and a constant or two-constant curve, + k *
/// emitterVelocity (k at t with a draw from seed + 0x33e627).
pub fn start_velocity(speed: &MinMaxCurve, inherit: Option<(i32, &MinMaxCurve)>, world: bool, emitter_velocity: V3, seed: u32, t: f32, dir: V3) -> V3 {
    let [r] = draws::<1>(seed, 0x96aa4de3);
    let s = evaluate(speed, t, r);
    let mut v = dir.map(|d| d * s);
    if let (true, Some((0, c))) = (world, inherit) {
        if c.state == 0 || c.state == 3 {
            let [r] = draws::<1>(seed, 0x33e627);
            let k = evaluate(c, t, r);
            for i in 0..3 {
                v[i] += k * emitter_velocity[i];
            }
        } else {
            unimplemented!("mod-only: InheritVelocity Initial curve: initialVelocity = emitterVelocity (InheritVelocityModule::Update)");
        }
    }
    v
}

/// StartLifetimeByEmitterSpeed (0x1801c3220): invStartLifetime / max(curve(t), 1e-4), t =
/// clamp01(scale * |emitterVelocity| + offset) from CalculateInverseLerpOffsetScale (0x1801a5b50), the curve's draw
/// from seed + 0x33e628.
pub fn lifetime_by_speed(c: &MinMaxCurve, range: [f32; 2], emitter_velocity: V3, seed: u32, inv_lifetime: f32) -> f32 {
    let [x, y, z] = emitter_velocity.map(|v| v * v);
    let speed = (z + 0.0 + (y + x)).sqrt();
    let d = range[1] - range[0];
    // CalculateInverseLerpOffsetScale: scale = 1 / d, or range.x itself when |d| <= 1e-9.
    let scale = if f32::from_bits(0x3089705f) < d.abs() { 1.0 / d } else { range[0] };
    let offset = -(range[0] * scale);
    let t = minps(1.0, maxps(0.0, scale * speed + offset));
    let [r] = draws::<1>(seed, 0x33e628);
    let k = maxps(evaluate(c, t, r), f32::from_bits(0x38d1b717));
    inv_lifetime / k
}

/// InitialModule::Start's velocity (0x180200210): the emission matrix's z column normalized, (z^2 + 0) + (x^2 + y^2)
/// its squared length; 0 at or under 1e-30. The shape module, when on, sets the velocity over it.
pub fn initial_direction(m: &M3) -> V3 {
    let c = m[2];
    let sq = (c[2] * c[2] + 0.0) + (c[0] * c[0] + c[1] * c[1]);
    let r = minps(crate::unity::sse::rsqrt(sq) * f32::from_bits(0x3f7ffffd), f32::MAX);
    let n = (r * 0.5) * (f32::from_bits(0x40400001) - r * (r * sq));
    if f32::from_bits(0x0da24260) < sq { c.map(|v| n * v) } else { [0.0; 3] }
}

/// worldToLocal from localToWorld (UpdateLocalToWorldMatrixAndScales 0x1801c7300, the affine inverse), column-major.
pub fn affine_inverse(m: &[f32; 16]) -> [f32; 16] {
    let det = ((((((m[0] * m[5]) * m[10] + 0.0) + (m[1] * m[6]) * m[8]) + (m[2] * m[4]) * m[9]) - (m[2] * m[5]) * m[8]) - (m[4] * m[1]) * m[10]) - (m[6] * m[0]) * m[9];
    let mut w = [0.0f32; 16];
    if ((det * det) as f64) < f64::from_bits(0x3abef2d0f5da7dd9) {
        return w;
    }
    let d = 1.0 / det;
    w[0] = (m[10] * m[5] - m[9] * m[6]) * d;
    w[4] = -((m[10] * m[4] - m[8] * m[6]) * d);
    w[8] = (m[4] * m[9] - m[8] * m[5]) * d;
    w[1] = -((m[10] * m[1] - m[2] * m[9]) * d);
    w[5] = (m[10] * m[0] - m[8] * m[2]) * d;
    w[9] = -((m[0] * m[9] - m[8] * m[1]) * d);
    w[2] = (m[1] * m[6] - m[2] * m[5]) * d;
    w[6] = -((m[0] * m[6] - m[2] * m[4]) * d);
    w[10] = (m[0] * m[5] - m[4] * m[1]) * d;
    w[12] = -((m[13] * w[4] + w[0] * m[12]) + m[14] * w[8]);
    w[13] = -((w[1] * m[12] + m[13] * w[5]) + m[14] * w[9]);
    w[14] = -((w[2] * m[12] + m[13] * w[6]) + m[14] * w[10]);
    w[15] = 1.0;
    w
}

/// LookRotationToMatrix (0x18074c880, read from the asm: the decompile loses its returns): columns right = up x fwd,
/// up' = fwd x right, fwd, each normalized by sqrtf((x^2 + y^2) + z^2). None (the caller's identity) when fwd or right
/// is under Vector3f::epsilon, or |up'|^2 is more than 1e-6 off 1.
pub fn look_rotation(f: V3, u: V3) -> Option<M3> {
    let eps = f32::from_bits(0x3727c5ac);
    let len = ((f[0] * f[0] + f[1] * f[1]) + f[2] * f[2]).sqrt();
    if eps > len {
        return None;
    }
    let f = f.map(|v| v / len);
    let r = [u[1] * f[2] - u[2] * f[1], u[2] * f[0] - u[0] * f[2], u[0] * f[1] - u[1] * f[0]];
    let len = ((r[0] * r[0] + r[1] * r[1]) + r[2] * r[2]).sqrt();
    if eps > len {
        return None;
    }
    let r = r.map(|v| v / len);
    let up = [f[1] * r[2] - f[2] * r[1], f[2] * r[0] - f[0] * r[2], f[0] * r[1] - f[1] * r[0]];
    let sq = ((up[0] * up[0] + up[1] * up[1]) + up[2] * up[2]) - 1.0;
    (f32::from_bits(0x358637bd) >= sq.abs()).then_some([r, up, f])
}

/// Matrix3x3f::operator*= (0x18074b050): a * b, each entry (a.r0 * b.c0 + a.r1 * b.c1) + a.r2 * b.c2.
pub fn mul3(a: &M3, b: &M3) -> M3 {
    [0, 1, 2].map(|c| [0, 1, 2].map(|r| (a[0][r] * b[c][0] + a[1][r] * b[c][1]) + a[2][r] * b[c][2]))
}

/// The 3x3 columns of a column-major 4x4.
pub fn columns(m: &[f32; 16]) -> M3 {
    [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_maths_follow_the_templates() {
        let m = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
        assert_eq!(mul(&m, [1.0, 1.0, 1.0]), [2.0, 3.0, 4.0]);
        assert_eq!(transformation(false, true, &IDENTITY, &m, [1.0, 2.0, 3.0]), [[2.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 12.0]]);
        // A constant drag of 1 at speed 2: 2 - 1 * dt.
        let c = Clamp { magnitude: Default::default(), dampen: 0.0, drag: MinMaxCurve { state: 0, scalar: 1.0, ..Default::default() }, drag_by_size: false, drag_by_velocity: false };
        let mut v = [2.0, 0.0, 0.0];
        clamp(&c, 1.0, 0.5, 7, 0.0, [1.0; 3], false, &mut v, [0.0; 3]);
        assert_eq!(v, [1.5, 0.0, 0.0]);
        // The lerp range degenerates to range.x as the scale.
        let one = MinMaxCurve { state: 0, scalar: 2.0, ..Default::default() };
        assert_eq!(lifetime_by_speed(&one, [1.0, 1.0], [0.0; 3], 0, 1.0), 0.5);
        assert_eq!(draws::<1>(5, 0)[0], unit(Rand::new(5).next()));
        let l2w = [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 1.0, 2.0, 3.0, 1.0];
        let w = affine_inverse(&l2w);
        assert_eq!((w[0], w[5], w[10], w[12], w[13], w[14]), (0.5, 0.5, 0.5, -0.5, -1.0, -1.5));
        let d = initial_direction(&IDENTITY);
        assert!((d[2] - 1.0).abs() < 1e-6 && d[0] == 0.0);
    }

    /// VelocityModule's orbital part against UnityPlayer.dll's on the same batches (particle_orbital_dll.rs); the
    /// identity matrices are a local system's.
    #[test]
    fn orbital_dll() {
        struct O {
            pos: [u32; 3],
            anim: [u32; 3],
            dt: u32,
            out: [u32; 3],
        }
        struct K {
            orbital: [u32; 3],
            offset: [u32; 3],
            radial: u32,
            l2w: [u32; 16],
            w2l: [u32; 16],
            lanes: [O; 4],
        }
        let f = f32::from_bits;
        let c = |b: u32| crate::assets::particles::MinMaxCurve { scalar: f(b), ..Default::default() };
        let cases: &[K] = &include!("particle_orbital_dll.rs");
        for (n, k) in cases.iter().enumerate() {
            let v = crate::assets::particles::Velocity { orbital: k.orbital.map(c), offset: k.offset.map(c), radial: c(k.radial), ..Default::default() };
            let (l2w, w2l) = (k.l2w.map(f), k.w2l.map(f));
            for (i, l) in k.lanes.iter().enumerate() {
                let mut anim = l.anim.map(f);
                super::orbital(&v, (true, &l2w, &w2l), l.pos.map(f), f(l.dt), &mut anim);
                assert_eq!(anim.map(f32::to_bits), l.out, "case {n} lane {i}");
            }
        }
    }

}
