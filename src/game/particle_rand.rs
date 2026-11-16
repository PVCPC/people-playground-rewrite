//! The native ParticleSystem randoms (UnityPlayer 2020.3, read in Ghidra). Rand is UnityEngine.Random's xorshift128
//! (`crate::unity::Random`); RandN runs four of them side by side, one per SIMD lane.
#![cfg_attr(not(test), allow(dead_code))]
use crate::assets::particles::{MinMaxCurve, ParticleSystem};
use crate::unity::Random as Rand;

/// RandN: four Rands, lane i seeded from seed + i * 0x16f (RandN::SetSeed, 18074d3e0).
#[derive(Clone, Debug)]
pub struct RandN(pub [Rand; 4]);

impl RandN {
    pub fn new(seed: [u32; 4]) -> RandN {
        RandN([0, 1, 2, 3].map(|i| Rand::new(seed[i].wrapping_add(0x16f * i as u32) as i32)))
    }

    /// RandN::Get (1801baf60): one step of every lane.
    pub fn get(&mut self) -> [u32; 4] {
        [0, 1, 2, 3].map(|i| self.0[i].next())
    }

    /// Random01 of each lane: (v & 0x7fffff) * 1.1920929e-7 (0x34000001).
    pub fn get01(&mut self) -> [f32; 4] {
        self.get().map(unit)
    }
}

/// Random01 of one draw.
pub fn unit(v: u32) -> f32 {
    (v & 0x7fffff) as f32 * f32::from_bits(0x34000001)
}

/// ParticleSystemManager.rand (one per `particles::World`). Rand::RandomizeState (18074d2c0) fills the 16 state bytes from GetSystemEntropy;
/// PPG_PARTICLE_SEED=<u64> replaces them (low word x, high word y, and the constructor's z and w, 1801b23c0) so runs
/// repeat.
pub fn manager_rand() -> Rand {
    if let Some(seed) = std::env::var("PPG_PARTICLE_SEED").ok().and_then(|s| s.trim().parse::<u64>().ok()) {
        return Rand { state: [seed as u32, (seed >> 32) as u32, 0x6c078966, 0x714acb3f] };
    }
    use std::hash::{BuildHasher, Hasher};
    let word = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.finish()
    };
    let (a, b) = (word(0), word(1));
    Rand { state: [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32] }
}

/// The seeds ResetSeeds (1801bf2e0) gives a system at Play: randomSeed (a manager draw when autoRandomSeed), the
/// emission state's Rand and InitialModule's RandN.
#[derive(Clone, Debug)]
pub struct Seeds {
    pub random_seed: u32,
    pub emission: Rand,
    pub initial: RandN,
}

impl Seeds {
    pub fn reset(p: &ParticleSystem, manager: &mut Rand) -> Seeds {
        let random_seed = if p.auto_random_seed { manager.next() } else { p.random_seed };
        Seeds { random_seed, emission: Rand::new(random_seed as i32), initial: RandN::new([random_seed; 4]) }
    }

    /// CalculateStartDelay (1801b6860): delayT is startDelay at time 0 with the first draw of a Rand seeded from
    /// randomSeed (a one-shot copy: the system's randoms are untouched).
    pub fn start_delay(&self, delay: &MinMaxCurve) -> f32 {
        let r = unit(Rand::new(self.random_seed as i32).next());
        match delay.state {
            0 => delay.scalar,
            3 => r * (delay.scalar - delay.min_scalar) + delay.min_scalar,
            s => unimplemented!("mod-only: CalculateStartDelay of a startDelay curve (mode {s}): the native optimized curve evaluation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_follow_the_native_chains() {
        // ResetSeeds' sub-emitter path writes the chain expanded: z = s * 0x54341d9 + 0x6c078966, w = s * 0x6ab51b9d + 0x714acb3f.
        let s = 0x1234_5678u32;
        let r = Rand::new(s as i32);
        assert_eq!(r.state[2], s.wrapping_mul(0x54341d9).wrapping_add(0x6c078966));
        assert_eq!(r.state[3], s.wrapping_mul(0x6ab51b9d).wrapping_add(0x714acb3f));
        let n = RandN::new([s; 4]);
        assert_eq!(n.0[3].state[0], s + 0x44d);
        // CalculateStartDelay's inline draw: t = s ^ s << 11 with w from the chain.
        let (t, w) = (s << 11 ^ s, s.wrapping_mul(0x6ab51b9d).wrapping_add(0x714acb3f));
        assert_eq!(Rand::new(s as i32).next(), (w >> 11 ^ t) >> 8 ^ w ^ t);
    }
}
