//! ParticleSystemRenderer's side of a ParticleSystem, as UnityPlayer 2020.3 builds it: the per-particle colour,
//! size and sheet frame the modules write, and the billboard and trail geometry the geometry jobs lay out.
//!
//! Inputs are the native per-particle values: aliveTimePercent (0..100), the ColorRGBA32 start colour and the
//! start size. Colours are ColorRGBA32 packed r | g << 8 | b << 16 | a << 24, as Unity keeps them.

use crate::assets::particles::{Curve, Gradient, MinMaxCurve, MinMaxGradient, Sheet, SizeOverLifetime};

/// 0.01 (0x3c23d70a): aliveTimePercent to normalized age.
const PERCENT: f32 = 0.01;

/// rcpps, the hardware reciprocal estimate the engine refines with Newton steps.
pub(crate) fn rcpps(x: f32) -> f32 {
    // The Ryzen's rcpps table, so the Mac (arm64) gives the same numbers.
    crate::unity::sse::rcp(x)
}

/// cvttps2dq: truncation, and 0x80000000 for anything out of range or NaN.
pub(crate) fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 { i32::MIN } else { x as i32 }
}

/// RandN seeded with `seed` (RandN::RandN, 0x1801b32a0), then one Random01: the per-particle draw the modules
/// take from ParticleSystemParticles.randomSeed plus their own offset.
pub(crate) fn random01(seed: u32) -> f32 {
    crate::game::particle_rand::unit(crate::unity::Random::new(seed as i32).next())
}

/// Gradient::EvaluateHDR<0> (UnityPlayer 0x1801d0580, blend) and <1> (0x1801d0310, fixed), one lane.
pub(crate) fn evaluate_hdr(g: &Gradient, t: f32) -> [f32; 4] {
    let blend = g.mode == 0;
    let t = 65535.0 * t;
    // One channel group: the key times, their count and the values for the group.
    let channel = |times: &[u16; 8], n: usize, value: &dyn Fn(usize) -> f32| -> Option<f32> {
        if n <= 1 {
            return None;
        }
        let time = |i: usize| times[i] as f32;
        let tc = t.max(time(0)).min(time(n - 1));
        if blend {
            let i = (1..n).find(|&i| tc <= time(i))?;
            let f = ((tc - time(i - 1)) / (time(i) - time(i - 1)).max(f32::from_bits(0x358637bd))).min(1.0);
            Some((value(i) - value(i - 1)) * f + value(i - 1))
        } else {
            (0..n).find(|&i| tc <= time(i)).map(value)
        }
    };
    let mut out = [1.0; 4];
    for c in 0..3 {
        if let Some(v) = channel(&g.ctime, g.color_keys as usize, &|i| g.keys[i][c]) {
            out[c] = v;
        }
    }
    if let Some(v) = channel(&g.atime, g.alpha_keys as usize, &|i| g.keys[i][3]) {
        out[3] = v;
    }
    out
}

/// OptimizedGradient: the gradient resampled to ColorRGBA32 keys with reciprocal segment lengths.
pub(crate) struct OptimizedGradient {
    colors: [u32; 16],
    times: [f32; 16],
    rcp: [f32; 16],
    count: usize,
}

impl OptimizedGradient {
    /// Gradient::InitializeOptimized, as inlined in MinMaxGradient::InitializeOptimized (0x1801a8400).
    pub(crate) fn new(g: &Gradient) -> Self {
        let mut times = [0.0f32; 20];
        let (nc, na) = (g.color_keys as usize, g.alpha_keys as usize);
        for i in 0..nc {
            times[i] = g.ctime[i] as f32 * f32::from_bits(0x37800080);
        }
        for i in 0..na {
            times[nc + i] = g.atime[i] as f32 / 65535.0;
        }
        let mut n = nc + na;
        // Repeated times go: each is swapped with the last live time and the count drops.
        let mut i = 0;
        while i + 1 < n {
            let mut j = i + 1;
            while j < n {
                if times[i] == times[j] {
                    n -= 1;
                    times.swap(j, n);
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if g.mode != 0 {
            for t in &mut times[..n] {
                *t -= f32::from_bits(0x37800080);
            }
        }
        // The engine's exchange sort; the times are distinct here, so any ascending sort orders them the same.
        times[..n].sort_by(|a, b| a.partial_cmp(b).unwrap());
        if n < 16 {
            times[n] = 1.0;
            n += 1;
        } else {
            times[n - 1] = 1.0;
        }
        let padded = (n + 3) & !3;
        for t in &mut times[n..padded] {
            *t = 1.0;
        }
        let mut colors = [0u32; 16];
        for i in 0..padded.min(16) {
            let c = evaluate_hdr(g, times[i]);
            let byte = |v: f32| cvtt(255.0 * v.max(0.0).min(1.0) + 0.5) as u32;
            colors[i] = byte(c[0]) & 0xff | (byte(c[1]) << 8) & 0xff00 | (byte(c[2]) << 16) & 0xff0000 | byte(c[3]) << 24;
        }
        let mut rcp = [0.0f32; 16];
        for i in 1..n {
            let dt = (times[i] - times[i - 1]).max(f32::from_bits(0x358637bd));
            let r = rcpps(dt);
            let r = (f32::from_bits(0x40000002) - r * dt) * r;
            rcp[i] = r * (2.0 - r * dt);
        }
        Self { colors, times: times[..16].try_into().unwrap(), rcp, count: n }
    }

    /// OptimizedGradient::Evaluate<0> (0x1801d0010), one lane. `t` stays at or below 1, the last key time: past
    /// it the engine's four-lane loop would depend on the other particles of the group.
    pub(crate) fn evaluate(&self, t: f32) -> u32 {
        if self.count <= 1 {
            return u32::MAX;
        }
        let i = (1..self.count).find(|&i| t < self.times[i]).unwrap_or(self.count - 1);
        let f = cvtt((t - self.times[i - 1]).max(0.0).min(1.0) * self.rcp[i] * 255.0) as u32;
        let m = f | f << 16;
        let m = m | m << 8;
        lerp_bytes(self.colors[i - 1], self.colors[i], m)
    }
}

/// a + ((b - a) * m + 0x80) >> 8 per byte in 16-bit lanes (psubw, pmullw, paddw, psraw, paddw, pand).
pub(crate) fn lerp_bytes(a: u32, b: u32, m: u32) -> u32 {
    let mut out = 0u32;
    for k in 0..4 {
        let byte = |v: u32| ((v >> (8 * k)) & 0xff) as i16;
        let d = (byte(b) - byte(a)).wrapping_mul(byte(m));
        let v = (d.wrapping_add(0x80) >> 8).wrapping_add(byte(a)) & 0xff;
        out |= (v as u32) << (8 * k);
    }
    out
}

/// The SSE byte multiply the colour modules use: (x + (x >> 8)) >> 8 with x = a * b + 0x80, per channel.
pub(crate) fn multiply(a: u32, b: u32) -> u32 {
    let mut out = 0;
    for k in 0..4 {
        let x = ((a >> (8 * k)) & 0xff) * ((b >> (8 * k)) & 0xff) + 0x80;
        out |= ((x + (x >> 8)) >> 8) << (8 * k);
    }
    out
}

/// ColorModule::Update (0x1801e1390): the colour over lifetime applied to the particle's colour.
pub(crate) fn color_over_lifetime(g: &MinMaxGradient, alive: f32, seed: u32, color: u32) -> u32 {
    let t = (alive * PERCENT).max(0.0);
    match g.state {
        // UpdateTpl<0,0,0> (0x1801ddd30).
        1 if g.max.mode == 0 => multiply(color, OptimizedGradient::new(&g.max).evaluate(t)),
        // UpdateTpl<1,0,0> (0x1801db1a0): a byte weight from the seed lerps the min and max gradients.
        3 if g.max.mode == 0 && g.min.mode == 0 => {
            let f = cvtt(255.0 * random01(seed.wrapping_add(0x591bc05c))) as u32;
            let m = f | f << 16;
            let m = m | m << 8;
            let max = OptimizedGradient::new(&g.max).evaluate(t);
            let min = OptimizedGradient::new(&g.min).evaluate(t);
            multiply(color, lerp_bytes(min, max, m))
        }
        1 | 3 => unimplemented!("mod-only: fixed-mode colour over lifetime gradients (UpdateTpl<0,0,1>, <1,x,1>): no game system uses them"),
        s => unimplemented!("mod-only: colour over lifetime state {s} (UpdateTpl<2,0,0>): no game system uses it"),
    }
}

/// CalculateInverseLerpOffsetScale (0x1801a5b50): (scale, offset) taking range.x to 0 and range.y to 1; a range
/// narrower than 1e-9 (0x3089705f) scales by range.x.
pub(crate) fn inverse_lerp(range: [f32; 2]) -> (f32, f32) {
    let d = range[1] - range[0];
    let scale = if d.abs() > f32::from_bits(0x3089705f) { 1.0 / d } else { range[0] };
    (scale, -(range[0] * scale))
}

/// The speed modules' time: speed * scale + offset, maxps with 0 then minps with 1.
pub(crate) fn speed_time(speed: f32, range: [f32; 2]) -> f32 {
    let (scale, offset) = inverse_lerp(range);
    let t = speed * scale + offset;
    let t = if 0.0 > t { 0.0 } else { t };
    if 1.0 < t { 1.0 } else { t }
}

/// ColorBySpeedModule::Update (0x18020ad40) for one particle: `v` velocity + animatedVelocity, the speed
/// sqrtps(x^2 + (y^2 + z^2)) as UpdateTpl<0,0,0> (0x180208f40) sums it, the gradient's colour SSE-multiplied in.
pub(crate) fn color_by_speed(g: &MinMaxGradient, range: [f32; 2], v: [f32; 3], color: u32) -> u32 {
    let speed = (v[0] * v[0] + (v[1] * v[1] + v[2] * v[2])).sqrt();
    let t = speed_time(speed, range);
    match g.state {
        1 if g.max.mode == 0 => multiply(color, OptimizedGradient::new(&g.max).evaluate(t)),
        s => unimplemented!("mod-only: colour by speed state {s} or a fixed-mode gradient (UpdateTpl<0,0,1>, <1,x,x>, <2,0,0>): no game system uses it"),
    }
}

/// The gamma-to-linear step of ParticleSystem::UpdateModulesNonIncremental (0x1801c7fe5..0x1801c8722), as
/// compiled: below 1 every channel takes ((c + 0.055) / 1.055)^2.4, so the c / 12.92 branch never wins.
pub(crate) fn to_linear(color: u32) -> u32 {
    let k = f32::from_bits;
    // log2 and exp2 from the engine's polynomial approximations.
    let pow = |y: f32, p: f32| {
        let bits = y.to_bits() as i32;
        let f = f32::from_bits((bits as u32 & 0x807fffff) | 0x3f800000) - 1.0;
        let e = (bits >> 23) as f32 - 127.0;
        let log = f * f * (f * k(0x3e470bd9) + k(0xbf21dda4)) + (e + f * k(0x3fb80d57));
        let z = (p * log).max(-127.0);
        let r = z.round_ties_even();
        let floor = if z < r { r - 1.0 } else { r };
        let fz = z - floor;
        let scale = f32::from_bits(((cvtt(floor) + 127) as u32) << 23);
        (fz * fz * k(0x3ea2ad7f) + (k(0x3f2ea941) * fz + 1.0)) * scale
    };
    let channel = |c: u32| {
        let x = c as f32 / 255.0;
        let small = if x <= k(0x3d25aee6) { x * k(0x3d9e8391) } else { x };
        let base = if x < 1.0 { (x + k(0x3d6147ae)) * k(0x3f72a76f) } else { small };
        let p = if x < 1.0 { k(0x4019999a) } else if x <= k(0x3d25aee6) { 1.0 } else { k(0x400ccccd) };
        pow(base, p)
    };
    let byte = |v: f32| cvtt(255.0 * v.max(0.0).min(1.0) + 0.5) as u32;
    let r = byte(channel(color & 0xff));
    let g = byte(channel((color >> 8) & 0xff));
    let b = byte(channel((color >> 16) & 0xff));
    let a = byte((color >> 24) as f32 / 255.0);
    r & 0xff | (g << 8) & 0xff00 | (b << 16) & 0xff0000 | a << 24
}

fn anim_curve(c: &Curve) -> crate::game_assets::Curve {
    if c.keys.iter().any(|k| k.weighted_mode != 0) {
        unimplemented!("mod-only: weighted particle curve keys: the rewrite's AnimationCurve evaluates unweighted keys only");
    }
    crate::game_assets::Curve { keys: c.keys.iter().map(|k| [k.time, k.value, k.in_slope, k.out_slope]).collect(), pre: c.pre_infinity, post: c.post_infinity }
}

/// OptimizedPolynomialCurve, built by BuildOptimizedCurve (0x18074b410).
struct Polynomial {
    segments: [[f32; 4]; 2],
    time: f32,
}

impl Polynomial {
    /// None where the engine keeps the curve unoptimized: more than three keys, or ends off 0 and 1.
    fn build(c: &crate::game_assets::Curve, scalar: f32) -> Option<Self> {
        let n = c.keys.len();
        let eps = f32::from_bits(0x38d1b717);
        if n > 3 || (n >= 2 && !((c.keys[0][0] - 0.0).abs() <= eps && (c.keys[n - 1][0] - 1.0).abs() <= eps)) {
            return None;
        }
        let mut p = Polynomial { segments: [[0.0; 4]; 2], time: 1.0 };
        match n {
            0 => {}
            1 => p.segments = [[0.0, 0.0, 0.0, scalar * c.keys[0][1]]; 2],
            _ => {
                let mut times = [1.0f32; 2];
                for i in 0..2 {
                    if i + 1 < n {
                        let mut cache = crate::game_assets::CurveCache::default();
                        c.cache_segment(&mut cache, i, i + 1, 0.0);
                        p.segments[i] = cache.coeff;
                        times[i] = c.keys[i][0];
                    } else {
                        p.segments[i] = p.segments[i - 1];
                    }
                }
                p.segments = p.segments.map(|s| s.map(|v| scalar * v));
                p.time = times[1];
            }
        }
        Some(p)
    }

    /// The inlined OptimizedPolynomialCurve evaluate of the module templates (SizeModule UpdateTpl<1>, 0x1801f4280).
    fn evaluate(&self, t: f32) -> f32 {
        let [a, b, c, d] = self.segments[0];
        let first = ((a * t + b) * t + c) * t + d;
        let x = t - self.time;
        let [a, b, c, d] = self.segments[1];
        let second = ((a * x + b) * x + c) * x + d;
        if self.time <= t.min(f32::from_bits(0x3f7fff58)) { second } else { first }
    }
}

/// MinMaxCurve state 1 or 2 at `t`, `r` the particle's draw for state 2: the optimized polynomials when
/// BuildCurves (0x1801b6070) built them all, else AnimationCurve::Evaluate times the scalar (EvaluateSlow,
/// 0x1801ba6f0). State 2 is (max - min) * r + min.
pub(crate) fn curve_at(c: &MinMaxCurve, t: f32, r: impl Fn() -> f32) -> f32 {
    let max = anim_curve(&c.max);
    let slow = |k: &crate::game_assets::Curve| k.evaluate(&mut Default::default(), t) * c.scalar;
    let fast = Polynomial::build(&max, c.scalar);
    if c.state != 2 {
        return match fast {
            Some(p) => p.evaluate(t),
            None => slow(&max),
        };
    }
    let min = anim_curve(&c.min);
    let (hi, lo) = match (fast, Polynomial::build(&min, c.scalar)) {
        (Some(p), Some(q)) => (p.evaluate(t), q.evaluate(t)),
        _ => (slow(&max), slow(&min)),
    };
    (hi - lo) * r() + lo
}

/// The maximum of MinMaxCurve::FindMinMax (0x1801ba930): a constant's range takes in 0; two constants their larger;
/// a curve (or two) scalar * CalculateCurveRangesValue (0x1801b6100) of the keys from (+inf, -inf).
pub(crate) fn find_max(c: &MinMaxCurve) -> f32 {
    match c.state {
        0 => {
            if c.scalar <= 0.0 {
                0.0
            } else {
                c.scalar
            }
        }
        3 => {
            if c.min_scalar < c.scalar {
                c.scalar
            } else {
                c.min_scalar
            }
        }
        s => {
            let r = curve_range(&anim_curve(&c.max), [f32::INFINITY, f32::NEG_INFINITY]);
            let r = if s == 2 { curve_range(&anim_curve(&c.min), r) } else { r };
            c.scalar * r[1]
        }
    }
}

/// CalculateCurveRangesValue (0x1801b6100, asm-read): the range goes through CalculateMinMax (0x18074b660: minps
/// and maxps against the value) with the first key's value, then per segment (CalculateCacheData, offset 0) the
/// cubic at each root of its derivative inside the segment, then at the segment's end.
fn curve_range(k: &crate::game_assets::Curve, mut r: [f32; 2]) -> [f32; 2] {
    use crate::game::particle_sim::{maxps, minps};
    let n = k.keys.len();
    if n == 0 {
        return r;
    }
    let take = |r: &mut [f32; 2], v: f32| *r = [minps(r[0], v), maxps(r[1], v)];
    take(&mut r, k.keys[0][1]);
    for i in 0..n - 1 {
        let mut cache = crate::game_assets::CurveCache::default();
        k.cache_segment(&mut cache, i, i + 1, 0.0);
        let [a, b, c, d] = cache.coeff;
        let (a3, b2) = (a * 3.0, b * 2.0);
        let (t0, t1) = (k.keys[i][0], k.keys[i + 1][0]);
        let eps = f32::from_bits(0x3727_c5ac);
        let mut roots = [0.0f32; 2];
        let mut count = 0;
        if eps > a3.abs() {
            if b2.abs() > eps {
                roots[0] = -(c / b2);
                count = 1;
            }
        } else {
            let disc = b2 * b2 - a3 * 4.0 * c;
            if !(0.0 > disc) {
                let h = 0.5 / a3;
                let s = disc.sqrt();
                roots = [(s - b2) * h, (-s - b2) * h];
                count = 2;
            }
        }
        let cubic = |t: f32| ((a * t + b) * t + c) * t + d;
        for &t in &roots[..count] {
            if t >= 0.0 && t1 > t + t0 {
                take(&mut r, cubic(t));
            }
        }
        take(&mut r, cubic(t1 - t0));
    }
    r
}

/// SizeModule::Update (0x1801f6bd0) for one axis: currentSize = max(0, curve) * startSize.
/// States 2 and 3 draw from seed + 0x8d2c8431 (UpdateTpl<2> 0x1801f4480, <4>, and the inlined two-constant loop).
pub(crate) fn size_over_lifetime(m: &SizeOverLifetime, axis: usize, alive: f32, seed: u32, start: f32) -> f32 {
    let c = if m.separate { [&m.x, &m.y, &m.z][axis] } else { &m.x };
    let t = (alive * PERCENT).max(0.0);
    let r = || random01(seed.wrapping_add(0x8d2c8431));
    let v = match c.state {
        0 => c.scalar,
        1 | 2 => curve_at(c, t, r),
        _ => (c.scalar - c.min_scalar) * r() + c.min_scalar,
    };
    v.max(0.0) * start
}

/// The z rotationalSpeed the rotation modules leave this frame, in radians per second. InitialModule::Update
/// (0x1802026d0) zeroes it whenever either module is on; RotationModule::Update (0x1801f0b80) then adds the
/// over-lifetime curve at aliveTimePercent * 0.01 and RotationBySpeedModule::Update (0x1801cef40) the by-speed
/// curve at the speed mapped by its range. Each term takes the sign +1 when randomizeRotationDirection <
/// Random01(seed + 0xff2bb1a4), else -1; two-curve and two-constant states draw from seed + 0x6aed452e (over
/// lifetime) or seed + 0xdec4aea1 (by speed). `v` is velocity + animatedVelocity per axis.
pub(crate) fn angular_velocity(ps: &crate::assets::particles::ParticleSystem, seed: u32, alive: f32, v: [f32; 3]) -> f32 {
    let sign = if ps.randomize_rotation_direction < random01(seed.wrapping_add(0xff2bb1a4)) { 1.0 } else { -1.0 };
    let term = |c: &MinMaxCurve, t: f32, offset: u32| {
        let r = || random01(seed.wrapping_add(offset));
        sign * match c.state {
            0 => c.scalar,
            1 | 2 => curve_at(c, t, r),
            _ => (c.scalar - c.min_scalar) * r() + c.min_scalar,
        }
    };
    let mut speed = 0.0;
    if let Some(c) = &ps.rotation {
        speed += term(c, (alive * PERCENT).max(0.0), 0x6aed452e);
    }
    if let Some((c, range)) = &ps.rotation_by_speed {
        // The range helper (0x1801a5b50): scale 1 / (y - x) unless |y - x| <= 1e-9, where the scale is x.
        let length = (v[1] * v[1] + v[2] * v[2] + v[0] * v[0]).sqrt();
        speed += term(c, speed_time(length, *range), 0xdec4aea1);
    }
    speed
}

/// UVModule::Update's sheetIndex, 0..1 over the sheet (UpdateSheetTpl<0>, 0x180e03130).
pub(crate) fn sheet_index(s: &Sheet, alive: f32, inv_lifetime: f32, seed: u32) -> f32 {
    let floor = |v: f32| {
        let whole = cvtt(v) as f32;
        whole - if v < whole { 1.0 } else { 0.0 }
    };
    if s.mode == 1 {
        // UVModule::Update (0x180e2eca0) by time mode, then UpdateSheetTpl's sprite branch.
        if s.start_frame.state != 0 {
            unimplemented!("mod-only: sprite-mode sheet start frame state {} (UpdateSprites2Tpl variant not read)", s.start_frame.state);
        }
        let v = match s.time_mode {
            // UpdateSheetTpl<0> (0x180e03130): UpdateSpritesTpl<0,0> (0x180e26550) for a constant frame and
            // UpdateSprites2Tpl<0,1,0> (0x180e21a60, the whole-sheet <0,1,0> lane for lane) for an optimized curve.
            0 => {
                let t = (alive * PERCENT).max(0.0);
                let frame = match s.frame_over_time.state {
                    0 => s.frame_over_time.scalar,
                    1 if curve_optimized(&s.frame_over_time) => curve_at(&s.frame_over_time, t, || unreachable!()),
                    st => unimplemented!("mod-only: sprite-mode frame over time state {st} (UpdateSprites2Tpl variant not read)"),
                };
                frame * s.cycles + s.start_frame.scalar
            }
            // UpdateSheetTpl<2> (0x180e02f00): fps / sprite count, then UpdateSprites2Tpl<2,0,0> (0x180e20560) on
            // the age: max(max(aliveTimePercent * 0.01, 0) / inverseStartLifetime, 0).
            2 => {
                let cycles = s.fps / s.sprites.len() as i32 as f32;
                let age = maxps(maxps(alive * PERCENT, 0.0) / inv_lifetime, 0.0);
                cycles * age + s.start_frame.scalar
            }
            m => unimplemented!("mod-only: sprite-mode sheet time mode {m} (UpdateSheetTpl<1>, not read)"),
        };
        return v - floor(v);
    }
    if s.time_mode != 0 {
        unimplemented!("mod-only: grid texture sheet time mode {} (UpdateSheetTpl<1|2> whole-sheet / single-row variants, not read)", s.time_mode);
    }
    let t = (alive * PERCENT).max(0.0);
    let fot = &s.frame_over_time;
    let frame = match fot.state {
        0 => fot.scalar,
        1 => curve_at(fot, t, || unreachable!()),
        // UpdateSingleRow2Tpl<0,2,0> (0x180e13a60): (max - min) * Random01(seed + 0x13740583) + min.
        2 if s.animation_type == 1 => curve_at(fot, t, || random01(seed.wrapping_add(0x13740583))),
        st => unimplemented!("mod-only: frame over time state {st} on animation type {} (UpdateWholeSheet2Tpl<0,2|3,x>, not read)", s.animation_type),
    };
    let start = match s.start_frame.state {
        0 => s.start_frame.scalar,
        // UpdateWholeSheet2Tpl<0,1,3> (0x180e29140) and UpdateSingleRow2Tpl<0,1,3> (0x180e0ea50): seed + 0x56b3dbb0.
        3 => {
            let (lo, hi) = (s.start_frame.min_scalar, s.start_frame.scalar);
            (hi - lo) * random01(seed.wrapping_add(0x56b3dbb0)) + lo
        }
        st => unimplemented!("mod-only: start frame state {st} on animation type {}", s.animation_type),
    };
    let v = frame * s.cycles + start;
    let index = v - floor(v);
    match s.animation_type {
        // UpdateWholeSheet2Tpl<0,0,0> (0x180e2d470 family), <0,1,0> (0x180e29940), <0,4,0> (0x180e2d170).
        0 => index,
        // UpdateSingleRow2Tpl<0,1|2,0|3> (0x180e10160, 0x180e13a60, 0x180e0ea50): the row's start, then index * ((height +
        // row) - row) + row. A random row is floor(Random01(seed + 0xaf502044) * rows) * height; a custom row is
        // floor((rowIndex clamped to 0..rows - 1) * height * rows) * height.
        // <0,4,x> (0x180e1a970, 0x180e19f20) is the same with EvaluateSlow for a curve the polynomial can't hold.
        1 if matches!(s.row_mode, 0 | 1) && (fot.state == 1 || curve_optimized(fot) && Polynomial::build(&anim_curve(&fot.min), fot.scalar).is_some()) => {
            let (cols, rows) = (s.tiles_x as f32, s.tiles_y as f32);
            let height = 1.0 / (rows * cols) * cols;
            let row = if s.row_mode == 1 {
                floor(random01(seed.wrapping_add(0xaf502044)) * rows) * height
            } else {
                floor(s.row_index.clamp(0, s.tiles_y - 1) as f32 * height * rows) * height
            };
            index * ((height + row) - row) + row
        }
        1 => unimplemented!(
            "mod-only: single-row sheet, row mode {} frame state {} (UpdateSingleRow2Tpl variant not read)",
            s.row_mode,
            fot.state
        ),
        a => unimplemented!("mod-only: texture sheet animation type {a}"),
    }
}

/// The reciprocal the geometry job takes of a tile count: rcpps, then two Newton steps.
fn rcp_tiles(n: f32) -> f32 {
    let r = rcpps(n);
    let r = (f32::from_bits(0x40000002) - r * n) * r;
    r * (2.0 - r * n)
}

/// floor as the geometry jobs take it: cvttps2dq back to float, minus 1 where that rounded up.
pub(crate) fn floor_tt(v: f32) -> f32 {
    let whole = cvtt(v) as f32;
    whole - if v < whole { 1.0 } else { 0.0 }
}

/// ParticleGeomAnimateUVs (0x180dbcf90), grid mode, for the uvChannelMask -1 every game sheet has: the tile of
/// `frame` (m_NumUVFrame * sheetIndex, m_NumUVFrame = tiles x * tiles y from PrepareForRender 0x1801a94d0) as
/// UV0 and the next tile as UV1, each (left, bottom, right, top), and the blend frame - floor(frame). The next
/// tile is not wrapped: past the last tile it lands outside the sheet, as in the engine.
pub(crate) fn sheet_uvs(tiles_x: i32, tiles_y: i32, frame: f32) -> ([f32; 4], [f32; 4], f32) {
    let (x, y) = (tiles_x as f32, tiles_y as f32);
    let (rx, ry) = (rcp_tiles(x), rcp_tiles(y));
    let a = rx * frame;
    let top = 1.0 - ry;
    let whole = floor_tt(frame);
    let tile = |index: f32, row: f32| {
        let v = top - row * ry;
        let u = (index - row * x) * rx;
        [u, v, u + rx, v + ry]
    };
    let next = whole + 1.0;
    (tile(whole, floor_tt(a)), tile(next, floor_tt(a + rx)), frame - whole)
}

fn curve_optimized(c: &MinMaxCurve) -> bool {
    Polynomial::build(&anim_curve(&c.max), c.scalar).is_some()
}

/// m_InvViewMatrix rows m0..m2 (camera right, up, forward) that SetMatrices (0x1801ab1c0) gave for the unrotated main
/// camera, run from the real function on this machine: the check of `camera_matrices`.
#[cfg(test)]
pub(crate) const INV_VIEW: [[f32; 3]; 3] = [[1.0, -0.0, -0.0], [-0.0, 1.0, -0.0], [-0.0, -0.0, -1.0]];

/// CalculateRollCorrection (0x180dbb370) for the unrotated camera: m_CameraRotation (0, 0, 0, 1) and INV_VIEW m2,
/// read from the real function the same way: +0.
#[cfg(test)]
pub(crate) const ROLL_UNROTATED: f32 = 0.0;

/// The main camera as the engine holds it for the particle pass: m_ViewMatrix (columns m0..m3 of the world-to-camera
/// matrix), m_InvViewMatrix (columns, w lanes (0, 0, 0, 1) as the trail job forces them) and the roll correction.
pub(crate) struct CameraMatrices {
    pub w2c: [[f32; 4]; 4],
    pub c2w: [[f32; 4]; 4],
    /// m_InvViewMatrix rows m0..m2 (camera right, up, forward).
    pub axes: [[f32; 3]; 3],
    /// CalculateRollCorrection (0x180dbb370) lane 0.
    pub roll: f32,
}

/// Matrix4x4f::operator*= (0x18073c700): `a *= b`, column-major, in the engine's order of additions.
fn mul_assign(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut o = [0.0f32; 16];
    for c in 0..4 {
        for k in 0..4 {
            o[c * 4 + k] = b[c * 4 + 1] * a[4 + k] + b[c * 4] * a[k] + b[c * 4 + 2] * a[8 + k] + b[c * 4 + 3] * a[12 + k];
        }
    }
    o
}

/// Camera::GetWorldToCameraMatrix (0x1805e1ba0), implicit: SetScale(1, 1, -1) *= Transform::GetWorldToLocalMatrixNoScale
/// (0x1807f1b80) of the camera's world position and rotation. Column-major.
fn world_to_camera(p: [f32; 3], q: [f32; 4]) -> [f32; 16] {
    let [x, y, z, w] = q;
    let m8 = y * -2.0 * y + z * -2.0 * z + 1.0;
    let m9 = y * 2.0 * x + z * 2.0 * w + 0.0;
    let m10 = y * -2.0 * w + z * 2.0 * x + 0.0;
    let m5 = z * -2.0 * w + x * 2.0 * y + 0.0;
    let m6 = z * -2.0 * z + x * -2.0 * x + 1.0;
    let m7 = z * 2.0 * y + x * 2.0 * w + 0.0;
    let (px, py, pz) = (-p[0], -p[1], -p[2]);
    let m2 = x * 2.0 * z + y * 2.0 * w + 0.0;
    let m3 = x * -2.0 * w + y * 2.0 * z + 0.0;
    let m4 = x * -2.0 * x + y * -2.0 * y + 1.0;
    let wl = [m8, m5, m2, 0.0, m9, m6, m3, 0.0, m10, m7, m4, 0.0, m8 * px + m9 * py + pz * m10, m5 * px + m6 * py + pz * m7, m2 * px + m3 * py + pz * m4, 1.0];
    let mut s = [0.0f32; 16];
    (s[0], s[5], s[10], s[15]) = (1.0, 1.0, -1.0, 1.0);
    mul_assign(s, wl)
}

/// ParticleSystemTempData::SetMatrices (0x1801ab1c0) for the orthographic main camera at world position `p` with world
/// rotation `q`, then CalculateRollCorrection (0x180dbb370) on m_CameraRotation and m_InvViewMatrix.rs.m2.
pub(crate) fn camera_matrices(p: [f32; 3], q: [f32; 4]) -> CameraMatrices {
    let v = world_to_camera(p, q);
    let (r0, r1, r2) = ([v[0], v[1], v[2]], [v[4], v[5], v[6]], [v[8], v[9], v[10]]);
    let t = [v[12], v[13], v[14]];
    let sq = |r: [f32; 3]| r.map(|x| x * x);
    let (a, b, c) = (sq(r0), sq(r1), sq(r2));
    let (c01, c12, c2z, c0z) = (c[1] + c[0], c[2] + c[1], c[2] + 0.0, c[0] + 0.0);
    let (b01, b12, b2z, b0z) = (b[1] + b[0], b[2] + b[1], b[2] + 0.0, b[0] + 0.0);
    let (a01, a12, a2z, a0z) = (a[1] + a[0], a[2] + a[1], a[2] + 0.0, a[0] + 0.0);
    let lane0 = ((((a2z + a01) + b2z) + b01) + c2z + c01) * 0.333333;
    let lane1 = ((((a0z + a12) + b0z) + b12) + c0z + c12) * 0.333333;
    let lane2 = ((((a01 + a2z) + b01) + b2z) + c01 + c2z) * 0.333333;
    let (rows, scale): ([[f32; 3]; 3], [f32; 3]) = if lane0 < 1e-30 {
        ([[0.0; 3]; 3], [0.0; 3])
    } else {
        // rsqrte_magicnumber (0x18007a020) = 1 / rsqrtps(1).
        let magic = 1.0 / crate::unity::sse::rsqrt(1.0);
        let s = [lane0, lane1, lane2].map(|l| crate::unity::sse::rsqrt(l) * magic);
        let a = r0.map(|x| x * s[0]);
        let b = r1.map(|x| x * s[1]);
        let c = r2.map(|x| x * s[2]);
        let f10 = c[0] * b[2] - b[0] * c[2];
        let f24 = c[1] * b[0] - b[1] * c[0];
        let f14 = c[2] * b[1] - b[2] * c[1];
        let f12 = b[0] * a[2] - a[0] * b[2];
        let f25 = b[1] * a[0] - a[1] * b[0];
        let f21 = b[2] * a[1] - a[2] * b[1];
        let det = a[2] * f24 + 0.0 + a[1] * f10 + a[0] * f14;
        // `1e-6 < |det|` fails for a NaN det too: the shipped game reaches math::svdInverse (0x180ec87f0) through a hand-edited
        // shakeIntensity preference (NaN or infinite camera angle).
        let [row0, row1, row2] = if 1e-6 < det.abs() {
            let k = 1.0 / det;
            [[k * f14, k * (a[2] * c[1] - c[2] * a[1]), k * f21], [k * f10, k * (a[0] * c[2] - c[0] * a[2]), k * f12], [k * f24, k * (a[1] * c[0] - c[1] * a[0]), k * f25]]
        } else {
            crate::unity::svd::svd_inverse([a, b, c])
        };
        ([row0.map(|x| x * s[0]), row1.map(|x| x * s[1]), row2.map(|x| x * s[2])], s)
    };
    let _ = scale;
    let n = [-t[0], -t[1], -t[2]];
    let it: [f32; 3] = std::array::from_fn(|i| rows[0][i] * n[0] + rows[1][i] * n[1] + rows[2][i] * n[2]);
    let w2c = [[v[0], v[1], v[2], 0.0], [v[4], v[5], v[6], 0.0], [v[8], v[9], v[10], 0.0], [v[12], v[13], v[14], 1.0]];
    let c2w = [[rows[0][0], rows[0][1], rows[0][2], 0.0], [rows[1][0], rows[1][1], rows[1][2], 0.0], [rows[2][0], rows[2][1], rows[2][2], 0.0], [it[0], it[1], it[2], 1.0]];
    CameraMatrices { w2c, c2w, axes: rows, roll: roll_correction(q, rows[2]) }
}

/// CalculateRollCorrection (0x180dbb370), lane 0: the camera quaternion `q` and forward vector `d`.
fn roll_correction(q: [f32; 4], d: [f32; 3]) -> f32 {
    use crate::unity::sse::{rcp, rsqrt};
    let [q0, q1, q2, q3] = q;
    let c0 = d[0] * 0.0 - d[2] * 0.0;
    let c1 = d[1] * 0.0 - d[0] * 1.0;
    let c2 = d[2] * 1.0 - d[1] * 0.0;
    let f7 = d[2] * c2 - c1 * d[0];
    let f10 = d[0] * c0 - c2 * d[1];
    let f12 = d[1] * c1 - c0 * d[2];
    let (l0, l1, l2) = (f7 * f7 + f12 * f12, f10 * f10 + f7 * f7, f10 * f10 + 0.0);
    let l3 = f12 * f12 + 0.0;
    let n = [l2 + l0, l3 + l1, l0 + l2];
    let r = n.map(|x| minps(rsqrt(x) * 0.9999998, f32::from_bits(0x7f7fffff)));
    let nr = |i: usize, f: f32| (3.0000002 - r[i] * n[i] * r[i]) * r[i] * 0.5 * f;
    let (w, v, u) = (nr(0, f12), nr(1, f7), nr(2, f10));
    let (a15_0, a15_1, a15_2) = (q0 * -2.0 * q1, q0 * 2.0 * q0, q0 * -2.0 * q3);
    let f30 = (q1 * -2.0 * q1 - q2 * 2.0 * q2) * 0.0 + 0.0 + (q2 * -2.0 * q3 - a15_0) * 1.0 + (q0 * 2.0 * q2 - q1 * -2.0 * q3) * 0.0;
    let f32_ = (q1 * 2.0 * q0 - q2 * -2.0 * q3) * 0.0 + 1.0 + (q2 * -2.0 * q2 - a15_1) * 1.0 + (q0 * -2.0 * q3 - q1 * -2.0 * q2) * 0.0;
    let f34 = (q1 * -2.0 * q3 - q2 * -2.0 * q0) * 0.0 + 0.0 + (q2 * 2.0 * q1 - a15_2) * 1.0 + (q0 * -2.0 * q0 - q1 * 2.0 * q1) * 0.0;
    let s = (f34 * u + 0.0) + (f30 * w + f32_ * v);
    let t = minps(maxps(s, -1.0), 1.0);
    let ratio = (1.0 - t * t).sqrt() / t;
    let a = ratio.abs();
    let rc = rcp(a);
    let big = 1.0 < a;
    let e = (2.0000005 - a * rc) * rc;
    let x = if big { e * (2.0 - e * a) } else { a };
    let x2 = x * x;
    let x6 = x2 * x2 * x2;
    let x8 = x6 * x2;
    let x10 = x8 * x2;
    let p = x * (x2 * -0.3332646 + 0.99999946 + x2 * x2 * 0.19881524 + x6 * -0.13487266 + x8 * 0.08387136 + x10 * -0.037013143 + x10 * x2 * 0.007862508);
    let half_pi = f32::from_bits(0x3fc90fdb);
    let ang = if big { half_pi - p } else { p };
    let out = if t.is_sign_negative() { 3.1415927 - ang } else { ang };
    let cond = ((((q1 * -2.0 * q3 - q2 * -2.0 * q0) * 0.0 + 1.0) + (q2 * 2.0 * q1 - q0 * -2.0 * q3) * 0.0) + (q0 * -2.0 * q0 - q1 * 2.0 * q1) * 1.0) * (w * f32_ - f30 * v) + 0.0
        + ((((q1 * -2.0 * q1 - q2 * 2.0 * q2) * 0.0 + 0.0) + (q2 * -2.0 * q3 - q0 * -2.0 * q1) * 0.0) + (q0 * 2.0 * q2 - q1 * -2.0 * q3) * 1.0) * (v * f34 - f32_ * u)
        + ((((q1 * 2.0 * q0 - q2 * -2.0 * q3) * 0.0 + 0.0) + (q2 * -2.0 * q2 - q0 * 2.0 * q0) * 0.0) + (q0 * -2.0 * q3 - q1 * -2.0 * q2) * 1.0) * (u * f30 - f34 * w);
    if cond < 0.0 { -out } else { out }
}

/// PrepareForRender (0x1801a94d0), orthographic camera: m_MinMaxOrthoSize from minMaxParticleSize, the far plane's
/// world length and renderScale.x (1e-5 when below). m_MinMaxPlaneScale stays 0, so this is the kernel's min/max.
pub(crate) fn ortho_size_limits(range: [f32; 2], far_length: f32, scale_x: f32) -> [f32; 2] {
    let s = if 1e-5 <= scale_x { scale_x } else { 1e-5 };
    let (x, y) = (far_length * (range[0] / s), far_length * (range[1] / s));
    let special = |v: f32| v.to_bits() & 0x7f80_0000 == 0x7f80_0000;
    [if special(x) { 0.0 } else { x }, if special(y) { -1.0 } else { y }]
}

/// GenerateParticleGeometry<0,0,0,0> (0x180d9dce0), alignment 2 without the hierarchy-local bit: the axes from
/// m_WorldRotation (x, y, z, w), lane for lane with its +-2 constants (0x1815e7ad0..0x1815e7b30).
pub(crate) fn rotation_rows(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    let r0 = [1.0 + ((-2.0 * y) * y + (z * -2.0) * z), 0.0 + ((2.0 * y) * x + (z * 2.0) * w), 0.0 + ((-2.0 * y) * w + (z * 2.0) * x)];
    let r1 = [0.0 + ((-2.0 * z) * w + (2.0 * x) * y), 1.0 + ((-2.0 * z) * z + (-2.0 * x) * x), 0.0 + ((2.0 * z) * y + (2.0 * x) * w)];
    let r2 = [0.0 + ((2.0 * x) * z + (2.0 * y) * w), 0.0 + ((-2.0 * x) * w + (2.0 * y) * z), 1.0 + ((-2.0 * x) * x + (-2.0 * y) * y)];
    [r0, r1, r2]
}

/// The kernel's axes times diag(renderScale): component c of each row times scale c (the extra zero products only
/// pick the sign of a zero).
pub(crate) fn scaled_rows(rows: [[f32; 3]; 3], s: [f32; 3]) -> [[f32; 3]; 3] {
    rows.map(|r| [r[0] * s[0], r[1] * s[1], r[2] * s[2]])
}

/// maxps / minps: the second operand unless the first wins the compare (so NaN gives the second).
fn maxps(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn minps(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// The kernel's half size per axis after the min/max clamp (0x180d9ed51..0x180d9ee53): `alive` the particle's
/// aliveTimePercent lane (100 and over draws nothing), `limits` the ortho min/max.
pub(crate) fn clamped_half(sx: f32, sy: f32, alive: f32, limits: [f32; 2]) -> (f32, f32) {
    let [lo, hi] = limits;
    let m = maxps(maxps(sx, sy), f32::from_bits(0x358637bd));
    let a = if 0.0 <= lo { maxps(m, lo) * 0.5 } else { 0.0 };
    let c = if 0.0 <= hi { minps(a, 0.5 * hi) } else { a };
    let ratio = c / m;
    if alive < 100.0 { (sx * ratio, ratio * sy) } else { (0.0, 0.0) }
}

/// cvtps2dq then cvtdq2ps: round to nearest even, 0x80000000 out of range.
fn round_ps(x: f32) -> f32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 { -2147483648.0 } else { x.round_ties_even() }
}

/// ParticleGeomBillboard<0> (0x180d9fb10)'s sine polynomial: a in [-0.25, 0.25] turns, 2 pi a out.
fn turn_poly(a: f32) -> f32 {
    let k = |b: u32| f32::from_bits(b);
    let x2 = a * a;
    let x4 = x2 * x2;
    ((((k(0xc2992322) * x2 + k(0x42a33422)) * x4) + (k(0xc2255ddc) * x2 + k(0x40c90fda))) + (x4 * x4) * k(0x421ea0cd)) * a
}

/// ParticleGeomBillboard<0> (0x180d9fb10): the four corners of one particle at world position `w`, rotation `rot`
/// (radians, roll included) and half sizes; corners top-left, top-right, bottom-right, bottom-left at rotation 0.
pub(crate) fn billboard(axes: [[f32; 3]; 3], w: [f32; 3], rot: f32, hx: f32, hy: f32) -> [[f32; 3]; 4] {
    let t = rot * f32::from_bits(0x3e22f983);
    let a = turn_poly(0.25 - (t - round_ps(t)).abs()) * hy;
    let t2 = -0.25 + t;
    let b = turn_poly(0.25 - (t2 - round_ps(t2)).abs()) * hx;
    let [r, u, f] = axes;
    let along = |p: f32, q: f32| -> [f32; 3] { std::array::from_fn(|i| r[i] * p + (q * u[i] + 0.0 * f[i])) };
    let (v1, v2) = (along(-a + b, a + b), along(a + b, -b + a));
    let add = |v: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| w[i] + v[i]) };
    let sub = |v: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| w[i] - v[i]) };
    [add(v1), add(v2), sub(v1), sub(v2)]
}

/// ParticleGeomBillboardPivotOffset<0> (0x180da4e50), which GenerateParticleGeometry<0,1,0,0> (0x180d95400) calls in
/// place of ParticleGeomBillboard<0>: corners TL, TR, BR, BL at world position `w`, rotation `rot` (roll included),
/// half sizes and `pivot` = (sx * pivot.x, sy * pivot.y, sx * pivot.z) on the unclamped sizes. `axes` the kernel's
/// scaled rows (0x650); its unscaled rows (0x6e0) are the identity constants 0x1815e7aa0..0x1815e7ac0.
pub(crate) fn pivot_billboard(axes: [[f32; 3]; 3], w: [f32; 3], rot: f32, hx: f32, hy: f32, pivot: [f32; 3]) -> [[f32; 3]; 4] {
    let t = rot * f32::from_bits(0x3e22f983);
    let c = turn_poly(0.25 - (t - round_ps(t)).abs());
    let t2 = -0.25 + t;
    let s = turn_poly(0.25 - (t2 - round_ps(t2)).abs());
    let [r, u, f] = axes;
    // Unscaled row 2 = (0, 0, 1).
    let (u6, u7, u8) = (0.0f32, 0.0f32, 1.0f32);
    let q0: [f32; 3] = std::array::from_fn(|i| c * r[i] + (u[i] * -s + 0.0 * f[i]));
    let q1: [f32; 3] = std::array::from_fn(|i| s * r[i] + (c * u[i] + f[i] * 0.0));
    let q2: [f32; 3] = std::array::from_fn(|i| r[i] * u6 + (u[i] * u7 + f[i] * u8));
    let [px, py, pz] = pivot;
    let corner = |ox: f32, oy: f32| -> [f32; 3] { std::array::from_fn(|i| ((q2[i] * pz + oy * q1[i]) + q0[i] * ox) + w[i]) };
    [corner(-hx + px, py + hy), corner(hx + px, py + hy), corner(px + hx, -hy + py), corner(px + -hx, py + -hy)]
}

#[cfg(test)]
mod tests {
    /// A NaN or infinite shakeIntensity preference makes the camera angle non-finite: SetMatrices' determinant test fails and the
    /// inverse comes from math::svdInverse (0x180ec87f0) instead of the cofactor path.
    #[test]
    fn non_finite_camera_rotation_takes_svd_inverse() {
        let nan = f32::NAN;
        let m = super::camera_matrices([0.0, 0.0, -10.0], [nan, nan, nan, nan]);
        assert!(m.axes.iter().flatten().all(|x| x.is_nan()));
        let m = super::camera_matrices([0.0, 0.0, -10.0], [f32::INFINITY, 0.0, 0.0, 1.0]);
        assert!(m.axes.iter().flatten().all(|x| x.is_nan()));
    }

    use crate::assets::particles::{Curve, MinMaxCurve, MinMaxGradient, ParticleSystem};

    /// The unrotated camera reproduces what the real SetMatrices and CalculateRollCorrection gave (INV_VIEW,
    /// ROLL_UNROTATED and the trail job's two matrices).
    #[test]
    fn camera_matrices_unrotated_match_the_engine() {
        use super::*;
        let (x, y, z) = (1.37f32, -2.71f32, -10.0f32);
        let c = camera_matrices([x, y, z], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(c.axes.map(|r| r.map(f32::to_bits)), INV_VIEW.map(|r| r.map(f32::to_bits)));
        assert_eq!(c.roll.to_bits(), ROLL_UNROTATED.to_bits());
        let w2c = [[1.0, -0.0, -0.0, 0.0], [-0.0, 1.0, -0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [-x, -y, z, 1.0]];
        let c2w = [[1.0, 0.0, -0.0, 0.0], [0.0, 1.0, -0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [x, y, z, 1.0]];
        // (The old constants' zero signs came from the probe's input, not the engine's matrix: compare values.)
        assert_eq!(c.w2c, w2c, "{:?}", c.w2c);
        assert_eq!(c.c2w, c2w, "{:?}", c.c2w);
    }

    #[test]
    fn billboard_kernel() {
        use super::*;
        // Unrotated, unclamped: half size 0.5 of a size-1 particle, corners TL, TR, BR, BL.
        let axes = scaled_rows(INV_VIEW, [1.0; 3]);
        let (hx, hy) = clamped_half(1.0, 1.0, 50.0, ortho_size_limits([0.0, 0.5], 10.0, 1.0));
        assert_eq!((hx, hy), (0.5, 0.5));
        let c = billboard(axes, [2.0, 3.0, 0.0], 0.0, hx, hy);
        assert_eq!([c[0][0], c[0][1], c[1][0], c[1][1], c[2][0], c[2][1], c[3][0], c[3][1]], [1.5, 3.5, 2.5, 3.5, 2.5, 2.5, 1.5, 2.5]);
        // The max clamp: 0.5 of a 4-wide view caps the size at 2; past 100 percent alive draws nothing.
        assert_eq!(clamped_half(8.0, 8.0, 0.0, ortho_size_limits([0.0, 0.5], 4.0, 1.0)), (1.0, 1.0));
        assert_eq!(clamped_half(1.0, 1.0, 100.0, [0.0, 10.0]), (0.0, 0.0));
        // A quarter turn clockwise: top-left goes to top-right.
        let c = billboard(axes, [0.0; 3], std::f32::consts::FRAC_PI_2, 0.5, 0.5);
        assert!((c[0][0] - 0.5).abs() < 1e-5 && (c[0][1] - 0.5).abs() < 1e-5, "{c:?}");
        // The local axes of a quarter turn about z.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let r = rotation_rows([0.0, 0.0, h, h]);
        assert!(r[0][0].abs() < 1e-6 && (r[0][1] - 1.0).abs() < 1e-6 && (r[1][0] + 1.0).abs() < 1e-6, "{r:?}");
    }

    /// ParticleGeomBillboardPivotOffset<0> (0x180da4e50) run in UnityPlayer.dll on these inputs (scratchpad
    /// po_oracle.py): (axes, world, rotation, half x, half y, pivot, corners) as f32 bits.
    #[test]
    fn pivot_billboard_matches_the_engine() {
        #[allow(clippy::type_complexity)]
        let cases: [([[u32; 3]; 3], [u32; 3], u32, u32, u32, [u32; 3], [u32; 12]); 16] = [
            ([[0x3efbb3e2, 0x3f779702, 0x3f97239c], [0x3fe288d8, 0x3f75a7f9, 0x3fd83afb], [0xbff1263a, 0xbe0ccf42, 0x3fe2ffa7]], [0x416e5bfa, 0x42205c36, 0xc21ab7b6], 0x3fc90fdb, 0x3f923607, 0x3e9ca1f7, [0xbf8057b9, 0x3f6cbb6d, 0xbebc1b4d], [0x419fed12, 0x422d8e6b, 0xc2090477, 0x417f2938, 0x4224c991, 0xc2187460, 0x417a5931, 0x42226b9e, 0xc21b582a, 0x419d850f, 0x422b3079, 0xc20be842]),
            ([[0x3efbb3e2, 0x3f779702, 0x3f97239c], [0x3fe288d8, 0x3f75a7f9, 0x3fd83afb], [0xbff1263a, 0xbe0ccf42, 0x3fe2ffa7]], [0xc245dfef, 0x41dadffe, 0x42380d04], 0x3fc90fdb, 0x3f5e2fb6, 0x40389a91, [0x3e20a8ca, 0x3f361931, 0xbf972724], [0xc230df18, 0x41fd7411, 0x4245762d, 0xc23d28e6, 0x41f020ba, 0x4239bb76, 0xc24880fa, 0x41c37e38, 0x421e7c40, 0xc23c372c, 0x41d0d190, 0x422a36f7]),
            ([[0x3efbb3e2, 0x3f779702, 0x3f97239c], [0x3fe288d8, 0x3f75a7f9, 0x3fd83afb], [0xbff1263a, 0xbe0ccf42, 0x3fe2ffa7]], [0xc19ee9a8, 0x4124f9db, 0xc246a592], 0xc0400000, 0x3e4ad22b, 0x3f887476, [0xbf469717, 0x3f492246, 0xbfa16567], [0xc1a50bb1, 0x41145e65, 0xc259968a, 0xc1a5cbe9, 0x410f2890, 0xc25b1003, 0xc186ba60, 0x413438e0, 0xc24b60e8, 0xc185fa28, 0x41396eb6, 0xc249e76f]),
            ([[0x3efbb3e2, 0x3f779702, 0x3f97239c], [0x3fe288d8, 0x3f75a7f9, 0x3fd83afb], [0xbff1263a, 0xbe0ccf42, 0x3fe2ffa7]], [0xc145d0ba, 0x42087e5f, 0xc096f803], 0x3fc90fdb, 0x3eb6d0ca, 0x3f3cfea2, [0x3fa3949c, 0xbe1ace8d, 0x3d4cbc13], [0xc15cc810, 0x420733d8, 0xc0afbb51, 0xc17100d2, 0x42047621, 0xc0d655e3, 0xc17c9df9, 0x41fd7fb0, 0xc1071010, 0xc1686538, 0x42017d8f, 0xc0e7858e]),
            ([[0x3fc9116e, 0xbf7f15a8, 0xbe182ada], [0x3eb7d5dc, 0x3f8fdd8c, 0xbf07e33d], [0xbf510914, 0xbe894385, 0x3ff4e92d]], [0xc1e9369a, 0x3fa4dd5d, 0xc15bcc9a], 0xc0400000, 0x40317229, 0x3fd155d7, [0xbf400d03, 0xbf3b98c9, 0xbfa521a2], [0xc1ba8df2, 0xc051c0ba, 0xc17f78b4, 0xc1fd4b58, 0x40449f06, 0xc17910b0, 0xc1ee3140, 0x40c80e73, 0xc18ad4ff, 0xc1ab73da, 0xbdc85b10, 0xc18e0901]),
            ([[0x3fc9116e, 0xbf7f15a8, 0xbe182ada], [0x3eb7d5dc, 0x3f8fdd8c, 0xbf07e33d], [0xbf510914, 0xbe894385, 0x3ff4e92d]], [0xc1d22177, 0x4116561b, 0x420284a9], 0xc10ddf26, 0x3edfafa9, 0x403bfa4c, [0x3f0da4e8, 0x3f9a73c3, 0xbf29cb55], [0xc1f48658, 0x40ff3818, 0x42062368, 0xc1fc832a, 0x4113bee9, 0x420598d1, 0xc1c71f21, 0x413bccb4, 0x41f25559, 0xc1bf224e, 0x4127a9d7, 0x41f36a87]),
            ([[0x3fc9116e, 0xbf7f15a8, 0xbe182ada], [0x3eb7d5dc, 0x3f8fdd8c, 0xbf07e33d], [0xbf510914, 0xbe894385, 0x3ff4e92d]], [0xc0fdeef2, 0xc20f84e0, 0x4228cd24], 0x3fc90fdb, 0x403b64b9, 0x3fae28cc, [0xbd4200c2, 0x3f6b035d, 0xbdabaf2b], [0xc04e0846, 0xc20b2267, 0x42207ce3, 0xc0aa4cea, 0xc2257643, 0x422cebf1, 0xc1198b66, 0xc21a9dad, 0x422e8a07, 0xc0efce07, 0xc20049d1, 0x42221af8]),
            ([[0x3fc9116e, 0xbf7f15a8, 0xbe182ada], [0x3eb7d5dc, 0x3f8fdd8c, 0xbf07e33d], [0xbf510914, 0xbe894385, 0x3ff4e92d]], [0xc2245854, 0xc1b626e2, 0x41e19d7c], 0xc0400000, 0x3e41dec6, 0x4018ab4d, [0xbf13c3b1, 0x3eb32afa, 0xbfee6dcd], [0xc21ff686, 0xc1ce736e, 0x41d09e46, 0xc2223ddd, 0xc1cafb75, 0x41d0d644, 0xc2173a4e, 0xc1a5e162, 0x41bbfacf, 0xc214f2f8, 0xc1a9595c, 0x41bbc2d0]),
            ([[0xbf46f6d2, 0x3e72f395, 0x3f2c58d0], [0x3faba398, 0xbf70c8f3, 0x3ff63ff2], [0x3f5d2b40, 0x3fe9ece3, 0xbf23c3cd]], [0xc2008a4d, 0x420cba93, 0xc0247494], 0xc0400000, 0x4000f498, 0x3fb1c4c1, [0x3fa223d6, 0x3f16f9b6, 0x3f98626e], [0xc208eef9, 0x421da2df, 0xc0dfbdfe, 0xc1f2f5ff, 0x4217b5e0, 0xc1095704, 0xc1d7e63f, 0x420dbca1, 0xc041e5ab, 0xc1f6ce31, 0x4213a9a0, 0xbfb80b2d]),
            ([[0xbf46f6d2, 0x3e72f395, 0x3f2c58d0], [0x3faba398, 0xbf70c8f3, 0x3ff63ff2], [0x3f5d2b40, 0x3fe9ece3, 0xbf23c3cd]], [0x42369b4a, 0x3fe82f49, 0x403bc6ca], 0xc0400000, 0x4033fa2f, 0x3fb7427f, [0x3f440512, 0x3f60f57e, 0x3f6be1c5], [0x4226b03c, 0x40ca5fe7, 0xbfbbc7b8, 0x423c4168, 0x4088364d, 0xc06c12a6, 0x424a3448, 0x3fd7d593, 0x40025ea4, 0x4234a31b, 0x40703dfe, 0x408846b7]),
            ([[0xbf46f6d2, 0x3e72f395, 0x3f2c58d0], [0x3faba398, 0xbf70c8f3, 0x3ff63ff2], [0x3f5d2b40, 0x3fe9ece3, 0xbf23c3cd]], [0xc23cf381, 0xc207fda3, 0xc0bc94c1], 0x3fc90fdb, 0x4011ba3a, 0x3f0aa643, [0x3f5fb497, 0x3ff91263, 0x3ff5a319], [0xc23686c7, 0xc1f9c2da, 0xc02fca8a, 0xc24ef3e8, 0xc1d77e9c, 0xc1381ffd, 0xc24b95d6, 0xc1d98cf0, 0xc143caf6, 0xc23328b4, 0xc1fbd12f, 0xc05e7672]),
            ([[0xbf46f6d2, 0x3e72f395, 0x3f2c58d0], [0x3faba398, 0xbf70c8f3, 0x3ff63ff2], [0x3f5d2b40, 0x3fe9ece3, 0xbf23c3cd]], [0xc22b358e, 0xc1ec81cb, 0x41ae6985], 0xc0400000, 0x3f9baa0f, 0x3f55e8d9, [0xbeb83bbc, 0xbfc514f7, 0x3fa9a956], [0xc2293dec, 0xc1d98cc0, 0x41b7df57, 0xc21fea43, 0xc1e0b3f0, 0x41b02fbb, 0xc217c637, 0xc1ecb444, 0x41caeb66, 0xc22119e0, 0xc1e58d14, 0x41d29b02]),
            ([[0xbf0125b1, 0xbfed8944, 0x3ee8944a], [0xbfcf7389, 0x3e393983, 0xbf247b53], [0x3ea5aced, 0x3feaa65d, 0x3fa31551]], [0xc1016fda, 0x41fa6452, 0x4163acaa], 0x3fc90fdb, 0x3ef0ce9d, 0x3f92f980, [0xbdbc28d8, 0xbf9a080b, 0xbfbbe9a6], [0xc1172bda, 0x41e67e68, 0x413f912e, 0xc0fd8ec5, 0x41e521f1, 0x41493cb4, 0xc0d87c16, 0x42039cf2, 0x41388bcc, 0xc104a283, 0x42044b2d, 0x412ee047]),
            ([[0xbf0125b1, 0xbfed8944, 0x3ee8944a], [0xbfcf7389, 0x3e393983, 0xbf247b53], [0x3ea5aced, 0x3feaa65d, 0x3fa31551]], [0x40cdbe02, 0xc0e28ea1, 0x423eac13], 0xbfa8b2e6, 0x403ab617, 0x3f91d5f2, [0x3fec1a6c, 0x3fd3d6af, 0x3ec43806], [0x4109ab07, 0xbf6b8960, 0x423c1841, 0xbfa4d7f0, 0xc026d5ea, 0x42303a68, 0xbfbd0ec0, 0xc0d9bb82, 0x4235b3b0, 0x4106a42d, 0xc0a3c1b9, 0x42419188]),
            ([[0xbf0125b1, 0xbfed8944, 0x3ee8944a], [0xbfcf7389, 0x3e393983, 0xbf247b53], [0x3ea5aced, 0x3feaa65d, 0x3fa31551]], [0xc2173edf, 0x4142b2d7, 0xc0b4e750], 0x3fc90fdb, 0x401ebf22, 0x3d223dfb, [0x3e056283, 0xbf67a438, 0x3fded980], [0xc2227b5c, 0x418b20e6, 0xc0aad16f, 0xc202524f, 0x4183f32a, 0xc009a508, 0xc2022962, 0x4185203e, 0xc00bf2a0, 0xc222526f, 0x418c4dfb, 0xc0abf83c]),
            ([[0xbf0125b1, 0xbfed8944, 0x3ee8944a], [0xbfcf7389, 0x3e393983, 0xbf247b53], [0x3ea5aced, 0x3feaa65d, 0x3fa31551]], [0x41262a7c, 0x41c2c752, 0xc218ba79], 0xc0400000, 0x3f58d7c4, 0x4007f0f5, [0x3ee8982f, 0x3f6cd39c, 0xbff25f69], [0x416c69b3, 0x41a32d11, 0xc21a8dab, 0x4173c033, 0x41bc6bb3, 0xc21e3717, 0x4101da74, 0x41b99b3a, 0xc227ef3d, 0x40f507e8, 0x41a05c98, 0xc22445d1]),
        ];
        let f = f32::from_bits;
        for (axes, w, rot, hx, hy, pivot, want) in cases {
            let c = super::pivot_billboard(axes.map(|r| r.map(f)), w.map(f), f(rot), f(hx), f(hy), pivot.map(f));
            let got: Vec<u32> = c.iter().flatten().map(|v| v.to_bits()).collect();
            assert_eq!(got, want.to_vec());
        }
    }

    #[test]
    fn color_pipeline_ends() {
        use super::*;
        assert_eq!(multiply(0xffffffff, 0x12345678), 0x12345678);
        assert_eq!(multiply(0, 0x12345678), 0);
        let mut g = Gradient { keys: [[1.0; 4]; 8], ctime: [0; 8], atime: [0; 8], mode: 0, color_keys: 2, alpha_keys: 2 };
        g.keys[1] = [0.0, 0.0, 0.0, 0.0];
        (g.ctime[1], g.atime[1]) = (65535, 65535);
        let o = OptimizedGradient::new(&g);
        assert_eq!(o.evaluate(0.0), 0xffffffff);
        assert!(o.evaluate(1.0) & 0xff <= 1);
        let mid = o.evaluate(0.5) & 0xff;
        assert!((126..=129).contains(&mid), "{mid}");
        assert_eq!(to_linear(0xffffffff), 0xffffffff);
        assert_eq!(to_linear(0x00000000), 0);
        // sRGB 0.5 is linear 0.214.
        let half = to_linear(0x80808080);
        assert!((54..=56).contains(&(half & 0xff)), "{half:x}");
        assert_eq!(half >> 24, 0x80);
        // RandN(0): x 0, y 1, z 0x6c078966, w = z * 0x6c078965 + 1; Random01 = (w ^ w >> 19) & 0x7fffff.
        let w = 0x6c078966u32.wrapping_mul(0x6c078965).wrapping_add(1);
        assert_eq!(random01(0), ((w ^ (w >> 19)) & 0x7fffff) as f32 * f32::from_bits(0x34000001));
        assert_eq!(lerp_bytes(0, 0xffffffff, 0), 0);
    }

    #[test]
    fn sheet_uvs_tiles() {
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        let (a, b, f) = super::sheet_uvs(4, 2, 0.0);
        assert!(near(a, [0.0, 0.5, 0.25, 1.0]) && near(b, [0.25, 0.5, 0.5, 1.0]) && f == 0.0, "{a:?} {b:?}");
        let (a, b, f) = super::sheet_uvs(4, 2, 5.5);
        assert!(near(a, [0.25, 0.0, 0.5, 0.5]) && near(b, [0.5, 0.0, 0.75, 0.5]) && f == 0.5, "{a:?} {b:?}");
        // The next tile of the last one is not wrapped.
        let (_, b, _) = super::sheet_uvs(4, 2, 7.25);
        assert!(near(b, [0.0, -0.5, 0.25, 0.0]), "{b:?}");
    }

    #[test]
    fn angular_velocity_sign_and_speed_map() {
        let curve = |state, scalar: f32, min_scalar: f32| MinMaxCurve { state, scalar, min_scalar, ..Default::default() };
        let mut ps = ParticleSystem::default();
        ps.rotation = Some(curve(0, 2.0, 0.0));
        assert_eq!(super::angular_velocity(&ps, 7, 50.0, [0.0; 3]), 2.0);
        // randomizeRotationDirection 1: Random01 < 1 always, so the sign is -1.
        ps.randomize_rotation_direction = 1.0;
        assert_eq!(super::angular_velocity(&ps, 7, 50.0, [0.0; 3]), -2.0);
        ps.randomize_rotation_direction = 0.0;
        let r = super::random01(7u32.wrapping_add(0x6aed452e));
        ps.rotation = Some(curve(3, 3.0, 1.0));
        assert_eq!(super::angular_velocity(&ps, 7, 50.0, [0.0; 3]), 2.0 * r + 1.0);
        ps.rotation = None;
        ps.rotation_by_speed = Some((curve(0, 4.0, 0.0), [0.0, 10.0]));
        assert_eq!(super::angular_velocity(&ps, 7, 0.0, [3.0, 4.0, 0.0]), 4.0);
    }

    /// MinMaxCurve's shape: its state, and per curve the key count, whether a key is weighted and the end times.
    fn curve_shape(c: &MinMaxCurve) -> String {
        let one = |k: &Curve| {
            let w = k.keys.iter().any(|k| k.weighted_mode != 0);
            let ends = k.keys.first().map_or(String::new(), |f| format!("{}..{}", f.time, k.keys[k.keys.len() - 1].time));
            format!("{}k{}{} {}/{}", k.keys.len(), if w { " weighted" } else { "" }, ends, k.pre_infinity, k.post_infinity)
        };
        match c.state {
            0 => "const".into(),
            1 => format!("curve [{}]", one(&c.max)),
            2 => format!("two curves [{}] [{}]", one(&c.min), one(&c.max)),
            _ => "two consts".into(),
        }
    }

    fn gradient_shape(g: &MinMaxGradient) -> String {
        let one = |g: &crate::assets::particles::Gradient| format!("mode {} {}c{}a", g.mode, g.color_keys, g.alpha_keys);
        match g.state {
            0 => "color".into(),
            1 => format!("gradient [{}]", one(&g.max)),
            2 => "two colors".into(),
            3 => format!("two gradients [{}] [{}]", one(&g.min), one(&g.max)),
            _ => format!("random color [{}]", one(&g.max)),
        }
    }

    /// The renderer-side settings the game's systems use (`cargo test render_inventory -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn render_inventory() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut m = std::collections::BTreeMap::<String, usize>::new();
        let mut add = |k: String| *m.entry(k).or_insert(0) += 1;
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 198) {
                let p = ParticleSystem::read(f.data(o)).unwrap();
                add(format!("start color {}", gradient_shape(&p.start_color)));
                add(format!("start size {} 3d {}", p.start_size.state, p.start_size_y.is_some()));
                if let Some(x) = &p.color {
                    add(format!("color over life {}", gradient_shape(x)));
                }
                if let Some((x, r)) = &p.color_by_speed {
                    add(format!("color by speed {} range {r:?}", gradient_shape(x)));
                }
                if let Some(x) = &p.size {
                    add(format!("size over life separate {} x {}", x.separate, curve_shape(&x.x)));
                    if x.separate {
                        add(format!("size over life y {} z {}", curve_shape(&x.y), curve_shape(&x.z)));
                    }
                }
                if let Some((x, r)) = &p.size_by_speed {
                    add(format!("size by speed separate {} x {} range {r:?}", x.separate, curve_shape(&x.x)));
                }
                if let Some(x) = &p.rotation {
                    add(format!("rotation over life {}", curve_shape(x)));
                }
                if let Some((x, r)) = &p.rotation_by_speed {
                    add(format!("rotation by speed {} range {r:?}", curve_shape(x)));
                }
                if let Some(s) = &p.sheet {
                    add(format!(
                        "sheet mode {} time {} anim {} row mode {} row {} tiles {}x{} cycles {} sprites {} uv mask {} flip {:?}",
                        s.mode, s.time_mode, s.animation_type, s.row_mode, s.row_index, s.tiles_x, s.tiles_y, s.cycles, s.sprites.len(), s.uv_channel_mask, s.flip_uv
                    ));
                    add(format!("sheet frame over time {} start frame {} fps {}", curve_shape(&s.frame_over_time), curve_shape(&s.start_frame), s.fps));
                }
                if let Some(t) = &p.trail {
                    add(format!(
                        "trail mode {} ratio {} texture {} ribbons {} world {} die {} size width {} size life {} inherit {} light {} split {} attach {} min dist {}",
                        t.mode, t.ratio, t.texture_mode, t.ribbon_count, t.world_space, t.die_with_particles, t.size_affects_width, t.size_affects_lifetime, t.inherit_particle_color, t.lighting_data, t.split_sub_emitter_ribbons, t.attach_ribbons, t.min_vertex_distance
                    ));
                    add(format!("trail lifetime {} width {}", curve_shape(&t.lifetime), curve_shape(&t.width_over_trail)));
                    add(format!("trail color over life {} over trail {}", gradient_shape(&t.color_over_lifetime), gradient_shape(&t.color_over_trail)));
                }
            }
            for o in f.objects.iter().filter(|o| o.class_id == 199) {
                let r = crate::assets::components::ParticleRenderer::read(f.data(o)).unwrap();
                add(format!(
                    "renderer enabled {} mode {} align {} roll {} sort {} fudge {} size {:?} pivot {:?} flip {:?} linear {} streams {:?} mask {} materials {}",
                    r.enabled, r.render_mode, r.alignment, r.allow_roll, r.sort_mode, r.sorting_fudge, r.size_range, r.pivot, r.flip, r.apply_active_color_space, r.custom_streams.then_some(&r.vertex_streams), r.mask_interaction, r.materials.len()
                ));
            }
        }
        for (k, v) in &m {
            println!("{v:5} {k}");
        }
    }
}
