//! LiquidDuplicatorBehaviour: while on it grows each liquid it holds toward Capacity (PeriodicNoise on fixedInterval), and it
//! can flush (FlushSound). Its context button is UI; EnabledLight is presentation.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::coroutine::{Routine, Yield};
use super::{d, mathf, script, Game};

script! {
    LiquidDuplicator {
        Capacity: f32 = 1.7857144,
        RateMultiplier: f32 = 1.0,
        Activated: bool = false,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// DamagableMachinery (a `machinery` index).
        DamagableMachinery: usize = 0,
        /// LiquidContainer, the LiquidContainerController whose fill Update sets.
        LiquidContainer: Option<usize> = None,
        /// FlushSound.length
        flush_length: f32 = 0.0,
        isBusy: bool = false,
        /// fixedInterval: RateHz = 2f / 9f (PeriodicNoise).
        fixedInterval: super::misc::FixedIntervalDistributor = super::misc::FixedIntervalDistributor::with_rate(2.0 / 9.0),
    }
}

impl Game {
    /// LiquidDuplicatorBehaviour.Pressure
    /// Pressure: Push while on and not destroyed, else None.
    pub(crate) fn liquid_duplicator_pressure(&self, l: usize) -> i32 {
        let s = &self.liquid_duplicators[l];
        if s.Activated && !self.machinery[s.DamagableMachinery].Destroyed { 0 } else { 2 }
    }

    /// LiquidDuplicatorBehaviour.Use
    pub fn liquid_duplicator_use(&mut self, l: usize, channel: i32) {
        let s = &self.liquid_duplicators[l];
        if !s.enabled || self.machinery[s.DamagableMachinery].Destroyed {
            return;
        }
        if channel == 1 {
            self.liquid_duplicator_try_flush(l);
        } else {
            self.liquid_duplicators[l].Activated = !self.liquid_duplicators[l].Activated;
        }
    }

    /// LiquidDuplicatorBehaviour.TryFlush: StartCoroutine(FlushRoutine()).
    pub(crate) fn liquid_duplicator_try_flush(&mut self, l: usize) {
        let id = self.liquid_duplicators[l].id;
        self.start_coroutine(id, id, Self::liquid_duplicator_flush(l));
    }

    /// LiquidDuplicatorBehaviour.FlushRoutine
    pub(crate) fn liquid_duplicator_flush(l: usize) -> Routine {
        let o = Owner::LiquidDuplicator(l);
        let (mut started, mut start_amount, mut time_spent) = (false, 0.0f32, 0.0f32);
        Box::new(move |g| {
            if !started {
                started = true;
                if g.liquid_duplicators[l].isBusy {
                    return Yield::Done;
                }
                g.liquid_duplicators[l].isBusy = true;
                let (p, id) = (g.liquid_duplicators[l].physical, g.liquid_duplicators[l].id);
                let clip = g.audio.clip(id, "FlushSound");
                g.play_clip_once(p, clip, 1.0);
                start_amount = g.bc(o).TotalLiquidAmount;
            } else {
                time_spent += g.time.deltaTime;
            }
            if (g.bc(o).TotalLiquidAmount >= mathf::EPSILON || time_spent <= g.liquid_duplicators[l].flush_length) && time_spent < 5.0 {
                let amount = (d(start_amount) * g.time.deltaTime / 1.521f32).f();
                g.drain(o, amount);
                return Yield::WaitForEndOfFrame;
            }
            g.delete_empty_liquid_entries(o);
            g.liquid_duplicators[l].isBusy = false;
            Yield::Done
        })
    }

    /// LiquidDuplicatorBehaviour.Update
    pub fn liquid_duplicator_update(&mut self, l: usize) {
        let o = Owner::LiquidDuplicator(l);
        self.blood_container_update(o);
        let s = &self.liquid_duplicators[l];
        let (p, capacity, rate) = (s.physical, s.Capacity, s.RateMultiplier);
        let num = (d(rate) * (d(1.0) + d(self.physicals[p].Charge()) * 0.1f32) * self.time.deltaTime * 60.0f32).f();
        if s.Activated && !self.machinery[s.DamagableMachinery].Destroyed && self.bc(o).TotalLiquidAmount < capacity {
            // for (i = 0; i < fixedInterval.CalculateCycleCount(Time.deltaTime); i++) PlayClipOnce(PeriodicNoise, 0.1f)
            let mut i = 0;
            while i < self.liquid_duplicators[l].fixedInterval.calculate_cycle_count(self.time.deltaTime) {
                let clip = self.audio.clip(self.liquid_duplicators[l].id, "PeriodicNoise");
                self.play_clip_once(p, clip, 0.1);
                i += 1;
            }
            for liquid in self.bc(o).LiquidDistribution.keys() {
                let raw = self.bc(o).LiquidDistribution.get(liquid).unwrap().raw;
                self.add_liquid(o, liquid, (d(raw) / capacity * num).f());
            }
        }
        if let Some(c) = self.liquid_duplicators[l].LiquidContainer {
            self.liquid_containers[c].FillPercentage = mathf::clamp(self.scaled_liquid_amount(o), 0.0, 1.0);
            self.liquid_container_color_from(c, o);
        }
    }
}
