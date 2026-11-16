//! RandomEventBehaviour (Substructure's "when on" group, under a map light's OnGroup). Update, while enabled and past its
//! cooldown, draws Random.value and dispatches its Events only when that draw is not above Time.deltaTime * ChancePerSecond.
//! ChancePerSecond and Cooldown are not in the export's serialized prefab, so they are the C# defaults, 0: every Update
//! draws once and dispatches only on an exact 0 draw, which is not ported (it stops the run).
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    RandomEvent {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// lastDispatchTime
        lastDispatchTime: f32 = 0.0,
        /// Events: (Delay, Actions' persistent calls).
        events: Vec<(f32, Vec<super::link::EventCall>)> = Vec::new(),
        ChancePerSecond: f32 = 0.0,
        Cooldown: f32 = 0.0,
    }
}

impl Game {
    /// RandomEventBehaviour.Update
    pub fn random_event_update(&mut self, i: usize) {
        let now = self.time.time;
        if now - self.random_events[i].lastDispatchTime <= self.random_events[i].Cooldown {
            return;
        }
        let num = self.time.deltaTime * self.random_events[i].ChancePerSecond;
        if self.random.value() > num {
            return;
        }
        self.random_events[i].lastDispatchTime = now;
        for k in 0..self.random_events[i].events.len() {
            let (delay, calls) = self.random_events[i].events[k].clone();
            if delay < f32::from_bits(1) {
                for call in calls {
                    self.event_call(call);
                }
            } else {
                // StartCoroutine(Utils.DelayCoroutine(item.Delay, ...)) on the component.
                let owner = self.random_events[i].id;
                self.start_wait(owner, delay, super::destroyable::Step::RandomEventDelayed(i, k));
            }
        }
    }

    /// The DelayCoroutine action of RandomEventBehaviour.Update: only while enabled.
    pub fn random_event_delayed(&mut self, i: usize, k: usize) {
        if self.random_events[i].enabled {
            for call in self.random_events[i].events[k].1.clone() {
                self.event_call(call);
            }
        }
    }
}
