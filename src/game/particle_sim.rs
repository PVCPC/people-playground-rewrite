//! The native ParticleSystem simulation (UnityPlayer 2020.3, read in Ghidra), one step at a time: InitialModule::Start
//! and the aging of SimulateParticles. MinMaxCurve states 1 and 2 and the gradients evaluate through
//! `crate::particle_render`.
#![cfg_attr(not(test), allow(dead_code))]
use crate::assets::particles::{Burst, MinMaxCurve, MinMaxGradient, ParticleSystem};
use crate::game::particle_rand::{unit, RandN};
use crate::particle_render::{curve_at, evaluate_hdr, multiply};
use crate::unity::Random as Rand;

/// The generic MinMaxCurve Evaluate (0x1801ba460) of one lane: Constant, TwoConstants as (max - min) * r + min, the
/// curves through `curve_at`.
pub fn evaluate(c: &MinMaxCurve, t: f32, r: f32) -> f32 {
    match c.state {
        0 => c.scalar,
        3 => (c.scalar - c.min_scalar) * r + c.min_scalar,
        _ => curve_at(c, t, || r),
    }
}

/// maxps(a, b) and minps(a, b) of one lane: b when the comparison fails, NaN included.
pub fn maxps(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
pub fn minps(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// cvttps2dq, low byte: the ColorRGBA32 channel of a float colour after the clamp (0x1801cfd10, 0x1801e0050).
fn byte(c: f32) -> u32 {
    let v = minps(1.0, maxps(0.0, c)) * 255.0 + 0.5;
    let i = if v.is_nan() || v >= 2147483648.0 || v < -2147483648.0 { i32::MIN } else { v as i32 };
    i as u32 & 0xff
}

fn pack(c: [f32; 4]) -> u32 {
    byte(c[0]) | byte(c[1]) << 8 | byte(c[2]) << 16 | byte(c[3]) << 24
}

/// The ColorRGBA32 of a Color, as MinMaxGradient Constant mode packs it (BloodImpactBehaviour.SetColor's startColor).
pub fn color32(c: [f32; 4]) -> u32 {
    pack(c)
}

/// MinMaxGradient Evaluate<min mode, max mode> (0x1801cfd10 and its three siblings) of one lane, ColorRGBA32.
pub fn gradient(g: &MinMaxGradient, t: f32, r: f32) -> u32 {
    match g.state {
        0 => pack(g.max_color),
        1 => pack(evaluate_hdr(&g.max, t)),
        2 => pack([0, 1, 2, 3].map(|i| (g.max_color[i] - g.min_color[i]) * r + g.min_color[i])),
        // Gradient::Evaluate (0x1801e0050) at the draw: EvaluateHDR of its mode, then the same clamp and rounding.
        4 => pack(evaluate_hdr(&g.max, r)),
        _ => unimplemented!("mod-only: start colour between two gradients (EvaluateRandomGradient, 0x1801d1470): no game system uses it"),
    }
}

/// InitialModule.m_InheritedValues: what a sub-emitter's parent hands down, or the defaults of a played system.
#[derive(Clone, Copy, Debug)]
pub struct Inherited {
    pub color: u32,
    pub size: [f32; 3],
    pub rotation: [f32; 3],
    pub lifetime_multiplier: f32,
    /// normalizedT; None is +inf, the emitter's own per-particle times.
    pub normalized_t: Option<f32>,
    /// randomSeed: the parent particle's, which InitialModule::Start writes to parentRandomSeed.
    pub random_seed: u32,
}

impl Default for Inherited {
    fn default() -> Self {
        Inherited { color: 0xffff_ffff, size: [1.0; 3], rotation: [0.0; 3], lifetime_multiplier: 1.0, normalized_t: None, random_seed: 0 }
    }
}

/// What InitialModule::Start writes for one particle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Started {
    /// ParticleSystemParticles.randomSeed.
    pub seed: u32,
    /// parentRandomSeed.
    pub parent_seed: u32,
    pub inv_lifetime: f32,
    /// startSize (and currentSize): y and z only mean something with uses3DSize.
    pub size: [f32; 3],
    /// x and y stay 0 without rotation3D.
    pub rotation: [f32; 3],
    /// ColorRGBA32, r | g << 8 | b << 16 | a << 24.
    pub color: u32,
}

/// One lane of each draw of the four-lane InitialModule.m_Random.
fn lanes(rand: &mut RandN) -> [f32; 4] {
    rand.get01()
}

/// InitialModule::Start (0x180200210) for the particles at `count` new indices: one batch of four lanes per started
/// four, whatever the modes. Per batch: the seeds, the per-particle RandN (seed + 0xff2bb1a4, RandN::RandN 0x1801b32a0)
/// whose first draw turns the rotation, the lifetime draw, the size draws (y and z with size3D), the rotation draws
/// (z, then x and y with rotation3D) and RandN::Get for the colour. `t` is each particle's emission time 0..1.
pub fn initial_start(ps: &ParticleSystem, rand: &mut RandN, inherited: &Inherited, t: &[f32]) -> Vec<Started> {
    let mut out = Vec::with_capacity(t.len());
    for batch in t.chunks(4) {
        let t = |i: usize| inherited.normalized_t.unwrap_or(*batch.get(i).unwrap_or(&batch[0]));
        let seeds = rand.get();
        let sign = seeds.map(|s| {
            let r = unit(Rand::new(s.wrapping_add(0xff2b_b1a4) as i32).next());
            if ps.randomize_rotation_direction < r { 1.0 } else { -1.0 }
        });
        let life = lanes(rand);
        let size = lanes(rand);
        let size3d = ps.start_size_y.as_ref().zip(ps.start_size_z.as_ref()).map(|(y, z)| (y, z, lanes(rand), lanes(rand)));
        let rot = lanes(rand);
        let rot3d = ps.start_rotation_xy.as_ref().map(|xy| (xy, lanes(rand), lanes(rand)));
        let col = lanes(rand);
        for i in 0..batch.len() {
            let inv_lifetime = inverse(maxps(f32::from_bits(0x3727c5ac), evaluate(&ps.start_lifetime, t(i), life[i]) * inherited.lifetime_multiplier));
            let sx = maxps(0.0, evaluate(&ps.start_size, t(i), size[i]));
            let (sy, sz) = match &size3d {
                Some((y, z, ry, rz)) => (maxps(0.0, evaluate(y, t(i), ry[i])), maxps(0.0, evaluate(z, t(i), rz[i]))),
                None => (sx, sx),
            };
            let z = sign[i] * (inherited.rotation[2] + evaluate(&ps.start_rotation, t(i), rot[i]));
            let (rx, ry) = match &rot3d {
                Some(([x, y], r0, r1)) => (sign[i] * (inherited.rotation[0] + evaluate(x, t(i), r0[i])), sign[i] * (inherited.rotation[1] + evaluate(y, t(i), r1[i]))),
                None => (0.0, 0.0),
            };
            out.push(Started {
                seed: seeds[i],
                parent_seed: inherited.random_seed,
                inv_lifetime,
                size: [inherited.size[0] * sx, inherited.size[1] * sy, inherited.size[2] * sz],
                rotation: [rx, ry, z],
                color: multiply(gradient(&ps.start_color, t(i), col[i]), inherited.color),
            });
        }
    }
    out
}

/// rcpps, the hardware reciprocal estimate.
fn rcp(x: f32) -> f32 {
    // The Ryzen's rcpps table, so the Mac (arm64) gives the same numbers.
    crate::unity::sse::rcp(x)
}

/// 1 / x as the module code computes it: rcpps and two Newton steps (InitialModule::Start, StartParticles).
pub fn inverse(x: f32) -> f32 {
    let r0 = rcp(x);
    let r1 = r0 * (f32::from_bits(0x40000002) - r0 * x);
    r1 * (2.0 - r1 * x)
}

/// A new particle's aliveTimePercent after its share `dt` of the step (StartModules 0x1801c35f0): capped at
/// 100.00001 whatever the ring buffer mode.
pub fn start_alive(inv_lifetime: f32, dt: f32) -> f32 {
    minps(dt * 100.0 * inv_lifetime, f32::from_bits(0x42c80001))
}

/// The aging of SimulateParticles (0x1801c2230, not LoopUntilReplaced): aliveTimePercent += invStartLifetime *
/// (100 * dt), capped at 100.00001, or 99.99999 under PauseUntilReplaced.
pub fn age(alive: f32, inv_lifetime: f32, dt: f32, ring_buffer_mode: i32) -> f32 {
    if ring_buffer_mode == 2 {
        unimplemented!("mod-only: ringBufferMode LoopUntilReplaced aging (the loop range wrap of SimulateParticles)");
    }
    let cap = f32::from_bits(if ring_buffer_mode == 1 { 0x42c7ffff } else { 0x42c80001 });
    minps(inv_lifetime * (100.0 * dt) + alive, cap)
}

/// ParticleSystemEmissionState: the emission Rand ResetSeeds seeds, the fractional particles carried between
/// steps and the spacing EmitCommon leaves for the new particles' times.
#[derive(Clone, Debug)]
pub struct Emission {
    pub rand: Rand,
    pub to_emit: f32,
    pub spacing: f32,
}

/// cvttss2si.
fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 { i32::MIN } else { x as i32 }
}

impl Emission {
    pub fn new(rand: Rand) -> Emission {
        Emission { rand, to_emit: 0.0, spacing: 0.0 }
    }

    /// The whole particles of `amount` plus the carried fraction (EmitCommon 0x1801dfac0, inlined in both emitters).
    fn common(&mut self, amount: f32) -> u64 {
        let f = amount + self.to_emit;
        self.to_emit = f;
        let n = if f >= 9.223372e18 { ((f - 9.223372e18) as i64 as u64) ^ 1 << 63 } else { f as i64 as u64 };
        self.to_emit -= n as f32;
        self.spacing = if amount >= f32::from_bits(0x38d1b717) { 1.0 / amount } else { 1.0 };
        n
    }

    /// EmissionModule::EmitOverTime (0x1801dfd00): `rate` (rateOverTime) between the system times `from` and `to`
    /// (wrapped past `length` when to < from), evaluated at each piece's end, plus the bursts. One draw always; the
    /// bursts' own. Returns the rate's particles (its out parameter) and the total.
    pub fn over_time(&mut self, rate: &MinMaxCurve, bursts: &[Burst], from: f32, to: f32, length: f32) -> (u64, u64) {
        let r = unit(self.rand.next());
        let mut amount = 0.0;
        if rate.scalar > 0.0 {
            let a = if from <= 0.0 { 0.0 } else { from };
            let mut b = if to <= 0.0 { 0.0 } else { to };
            if b < a {
                let v = evaluate(rate, b / length, r);
                amount = b * (if v <= 0.0 { 0.0 } else { v }) + 0.0;
                b = length;
            }
            let v = evaluate(rate, b / length, r);
            amount += (b - a) * if v <= 0.0 { 0.0 } else { v };
        }
        let from = if from <= 0.0 { 0.0 } else { from };
        let mut to = if to <= 0.0 { 0.0 } else { to };
        let forward = from <= to;
        let mut wrapped = 0;
        if !forward {
            wrapped = self.bursts(bursts, length, 0.0, to, true);
            to = length + f32::from_bits(0x38d1b717);
        }
        let bursts = self.bursts(bursts, length, from, to, forward);
        let n = self.common(amount);
        (n, n.wrapping_add(bursts as i64 as u64).wrapping_add(wrapped as i64 as u64))
    }

    /// EmissionModule::EmitOverDistance (0x1801dfb60): `rate` (rateOverDistance) at t / length times the distance
    /// the emitter moved this step (|velocity| * dt).
    pub fn over_distance(&mut self, rate: &MinMaxCurve, velocity: [f32; 3], t: f32, dt: f32, length: f32) -> u64 {
        if rate.scalar == 0.0 {
            return 0;
        }
        let r = unit(self.rand.next());
        let v = evaluate(rate, t / length, r);
        let rate = if v <= 0.0 { 0.0 } else { v };
        let [x, y, z] = velocity;
        let d = (x * x + y * y + z * z).sqrt();
        self.common(d * rate * dt)
    }

    /// AccumulateBursts (0x1801df040): the bursts whose time, or one of whose repeats (`repeats`), falls in [from, to).
    fn bursts(&mut self, bursts: &[Burst], length: f32, from: f32, to: f32, repeats: bool) -> i32 {
        let mut total = 0i32;
        for b in bursts {
            let t = b.time;
            let hit = (from <= t && t < to)
                || (repeats && b.cycles != 1 && t < from && {
                    let k = (from - t) / b.interval;
                    (b.cycles == 0 || k < (b.cycles - 1) as f32) && cvtt(k) < cvtt((to - t) / b.interval)
                });
            if hit {
                total = total.wrapping_add(self.burst(b, to / length));
            }
        }
        total
    }

    /// EmissionModule::AccumulateBurst (0x1801dee50): the probability draw below 1, then the count (a curve draws
    /// once; two constants are Random.Range(min, max + 1) as ints).
    pub fn burst(&mut self, b: &Burst, t: f32) -> i32 {
        let p = b.probability;
        if p == 0.0 {
            return 0;
        }
        if p < 1.0 && p <= unit(self.rand.next()) {
            return 0;
        }
        let c = &b.count;
        match c.state {
            0 => cvtt(c.scalar),
            3 => {
                let lo = cvtt(if c.scalar < c.min_scalar { c.scalar } else { c.min_scalar });
                let hi = cvtt(if c.scalar <= c.min_scalar { c.min_scalar } else { c.scalar }).wrapping_add(1);
                if hi <= lo {
                    if lo <= hi {
                        return lo;
                    }
                    let v = self.rand.next();
                    return lo.wrapping_sub((v % lo.wrapping_sub(hi) as u32) as i32);
                }
                let v = self.rand.next();
                lo.wrapping_add((v % hi.wrapping_sub(lo) as u32) as i32)
            }
            _ => {
                let r = unit(self.rand.next());
                cvtt(evaluate(c, t, r))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(lifetime: MinMaxCurve) -> ParticleSystem {
        let mut p = ParticleSystem::default();
        p.start_lifetime = lifetime;
        p
    }

    #[test]
    fn initial_start_draws_in_batches_of_four() {
        let c = |state, scalar, min_scalar| MinMaxCurve { state, scalar, min_scalar, ..Default::default() };
        let mut ps = system(c(3, 2.0, 1.0));
        ps.start_size = c(0, 0.5, 0.0);
        ps.start_rotation = c(3, 1.0, 0.0);
        ps.randomize_rotation_direction = 0.5;
        let seed = 77u32;
        let mut rand = RandN::new([seed; 4]);
        let started = initial_start(&ps, &mut rand, &Inherited::default(), &[0.0; 5]);
        assert_eq!(started.len(), 5);
        // Two batches of seed, lifetime, size, rotation and colour draws: ten of every lane.
        let mut lane0 = Rand::new(seed as i32);
        let draws: Vec<u32> = (0..10).map(|_| lane0.next()).collect();
        assert_eq!(started[0].seed, draws[0]);
        assert_eq!(started[4].seed, draws[5]);
        assert_eq!(rand.0[0].state, lane0.state);
        let r = unit(draws[1]);
        let x = r * 1.0 + 1.0;
        assert!((started[0].inv_lifetime - 1.0 / x).abs() < 1e-6);
        let sign = if 0.5 < unit(Rand::new(draws[0].wrapping_add(0xff2bb1a4) as i32).next()) { 1.0 } else { -1.0 };
        assert_eq!(started[0].rotation[2], sign * unit(draws[3]));
        assert_eq!(started[0].size[0], 0.5);
        assert_eq!(age(0.0, started[0].inv_lifetime, 0.02, 0), started[0].inv_lifetime * 2.0);
    }

    #[test]
    fn emission_carries_fractions_and_draws_bursts() {
        let mut ps = ParticleSystem::default();
        ps.rate_over_time = MinMaxCurve { state: 0, scalar: 10.0, ..Default::default() };
        let count = |state, scalar, min_scalar| MinMaxCurve { state, scalar, min_scalar, ..Default::default() };
        ps.bursts = vec![crate::assets::particles::Burst { time: 0.5, count: count(3, 5.0, 2.0), cycles: 1, interval: 0.01, probability: 1.0 }];
        let mut e = Emission::new(Rand::new(9));
        let mut check = Rand::new(9);
        // 0.25 s at 10/s: 2 whole, 0.5 carried; one draw for the rate.
        assert_eq!(e.over_time(&ps.rate_over_time, &ps.bursts, 0.0, 0.25, 1.0), (2, 2));
        check.next();
        assert_eq!(e.to_emit, 0.5);
        assert_eq!(e.spacing, 1.0 / 2.5);
        // The next 0.25 s holds the burst: Random.Range(2, 6).
        let (rate, n) = e.over_time(&ps.rate_over_time, &ps.bursts, 0.25, 0.5 + 1e-3, 1.0);
        assert_eq!(rate, 3);
        check.next();
        let burst = 2 + check.next() % 4;
        assert_eq!(n, 3 + burst as u64);
        assert_eq!(e.rand.state, check.state);
    }

    #[test]
    fn colour_rounds_like_cvttps() {
        assert_eq!(color32([1.0, 0.5, 0.0, 2.0]), 0xff00_80ff);
        assert_eq!(byte(f32::NAN), 0);
    }
}
