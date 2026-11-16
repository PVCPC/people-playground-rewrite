//! ActivationPropagation: a reference object every receiver of one message shares (UseWireBehaviour.ValidateSignal
//! changes the Channel of a direct one in place, and later receivers read the new value). Its Path is a
//! HashSet<Object> from the static TraversalSetPool (Pool<T>, MaximumCapacity 10000): RequestObject pops the free
//! stack, makes a fresh set while fewer than 10000 exist, and past that hands out a fallback set that is not counted;
//! CurrentlyActiveSignals is the count of pooled sets in use. Every constructor joins ActiveSignalSet;
//! ContinuousActivationBehaviour.LateUpdate disposes, once a second, each signal whose ActivityIntensity did not move
//! since the previous check (Dispose clears its Path and returns the set, which a later signal may get while an old
//! reference still reads it).
#![allow(non_snake_case)]

use super::Game;

/// A reference to one ActivationPropagation: shared, as the C# object is, and freed with its last reference.
#[derive(Clone, Debug)]
pub struct Act(std::sync::Arc<std::sync::Mutex<Signal>>);

impl PartialEq for Act {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Signal {
    pub Direct: bool,
    pub Channel: u16,
    pub TraversedCount: i32,
    /// Target: the GameObject (by its physical) the tool or a use wire sent it to.
    pub Target: Option<usize>,
    /// Path: the set in `Signals::sets`.
    set: usize,
    PreviousActivityIntensity: i32,
    pub(crate) ActivityIntensity: i32,
}

impl Signal {
    /// ActivationPropagation.get_Traversed (obsolete): TraversedCount.
    pub fn get_Traversed(&self) -> i32 {
        self.TraversedCount
    }
    /// ActivationPropagation.set_Traversed (obsolete): TraversedCount = value.
    pub fn set_Traversed(&mut self, value: i32) {
        self.TraversedCount = value;
    }
}

#[derive(Default)]
pub struct Signals {
    /// The HashSet<Object>s: root Transform instance ids.
    sets: Vec<Vec<i32>>,
    /// Pool.freeToUse (a stack) and currentlyInUse.
    free: Vec<usize>,
    in_use: Vec<usize>,
    created: usize,
    /// ActivationPropagation.ActiveSignalSet. A HashSet of objects hashed by address: its order differs from run to
    /// run in the game; here it is creation order.
    active: Vec<Act>,
    /// ContinuousActivationBehaviour.cleanupTimer
    pub cleanup_timer: f32,
    /// UseWireBehaviour.lastWarningTime (static)
    pub last_warning: Option<f32>,
}

/// ActivationPropagation.MaximumPropagations, and TraversalSetPool's capacity.
pub const MAXIMUM_PROPAGATIONS: usize = 10000;

/// TraversalSetPool.CreateFresh: a new empty set.
pub(crate) fn traversal_set_create_fresh() -> Vec<i32> {
    Vec::new()
}

/// TraversalSetPool.GetOverCapacityFallback: a new empty set.
pub(crate) fn traversal_set_over_capacity_fallback() -> Vec<i32> {
    Vec::new()
}

/// TraversalSetPool.ResetObjectForNextUse: obj.Clear().
pub(crate) fn traversal_set_reset_for_next_use(obj: &mut Vec<i32>) {
    obj.clear();
}

impl Signals {
    /// Pool<T>.RequestObject with TraversalSetPool's CreateFresh (the Prefill), ResetObjectForNextUse and
    /// GetOverCapacityFallback.
    pub(crate) fn request(&mut self) -> usize {
        // Pool`1.RequestObject: free sets first, then Prefill while CreatedAmount is under MaximumCapacity, else the fallback.
        if self.free.is_empty() && self.created < MAXIMUM_PROPAGATIONS {
            // Pool`1.Prefill: CreateFresh, CreatedAmount++, push onto the free stack.
            self.sets.push(traversal_set_create_fresh());
            // Pool`1.CreatedAmount: the count of sets the pool made (the setter is private to Prefill).
            self.created += 1;
            self.free.push(self.sets.len() - 1);
        }
        // Pool`1.GetExistingFromPool: pop, ResetObjectForNextUse, add to currentlyInUse.
        match self.free.pop() {
            Some(s) => {
                traversal_set_reset_for_next_use(&mut self.sets[s]);
                self.in_use.push(s);
                s
            }
            // GetOverCapacityFallback (with a "Pool can't satisfy demand" warning in the log)
            None => {
                self.sets.push(traversal_set_over_capacity_fallback());
                self.sets.len() - 1
            }
        }
    }

    fn new_signal(&mut self, Direct: bool, Channel: u16, TraversedCount: i32, path: &[i32]) -> Act {
        let set = self.request();
        for &x in path {
            if !self.sets[set].contains(&x) {
                self.sets[set].push(x);
            }
        }
        let a = Act(std::sync::Arc::new(std::sync::Mutex::new(Signal { Direct, Channel, TraversedCount, Target: None, set, PreviousActivityIntensity: -1, ActivityIntensity: 0 })));
        self.active.push(a.clone());
        a
    }

    // Pool`1.AmountInUse: currentlyInUse.Count (ActivationPropagation.CurrentlyActiveSignals reads it).
    /// ActivationPropagation.CurrentlyActiveSignals: traversalSetPool.AmountInUse.
    pub fn currently_active(&self) -> usize {
        self.in_use.len()
    }

    /// The cleanup at the end of ContinuousActivationBehaviour.LateUpdate.
    pub fn cleanup(&mut self, delta_time: f32) {
        self.cleanup_timer += delta_time;
        if !(self.cleanup_timer > 1.0) {
            return;
        }
        self.cleanup_timer = 0.0;
        let mut dead = Vec::new();
        for a in &self.active {
            let mut s = a.0.lock().unwrap();
            let previous = s.PreviousActivityIntensity;
            s.PreviousActivityIntensity = s.ActivityIntensity;
            if s.ActivityIntensity == previous {
                dead.push(a.clone());
            }
        }
        for a in dead {
            // ActivationPropagation.Dispose: out of the set, Path.Clear(), ReturnToPool (a fallback set is not in use: a warning only).
            self.active.retain(|x| *x != a);
            let set = a.0.lock().unwrap().set;
            self.sets[set].clear();
            // Pool`1.ReturnToPool: currentlyInUse.Remove(obj) then freeToUse.Push; a set not in use only logs a warning.
            if let Some(i) = self.in_use.iter().position(|&x| x == set) {
                self.in_use.remove(i);
                self.free.push(set);
            }
        }
    }
}

impl Game {
    /// The signal's fields as they are now.
    pub fn sig(&self, a: &Act) -> Signal {
        *a.0.lock().unwrap()
    }

    pub fn sig_mut<'a>(&mut self, a: &'a Act) -> std::sync::MutexGuard<'a, Signal> {
        a.0.lock().unwrap()
    }

    /// new ActivationPropagation(direct, channel, sender)
    pub fn signal_direct(&mut self, direct: bool, channel: u16) -> Act {
        self.signals.new_signal(direct, channel, 1, &[])
    }

    /// new ActivationPropagation(obj, channel, sender): `root` on the path, not direct.
    pub fn signal_from(&mut self, root: i32, channel: u16) -> Act {
        self.signals.new_signal(false, channel, 1, &[root])
    }

    /// new ActivationPropagation(): an empty path, channel 0, not direct, TraversedCount 0.
    pub fn signal_empty(&mut self) -> Act {
        self.signals.new_signal(false, 0, 0, &[])
    }

    /// ActivationPropagation.Contains
    pub fn signal_contains(&self, a: &Act, root: i32) -> bool {
        self.signals.sets[self.sig(a).set].contains(&root)
    }

    /// ActivationPropagation.Branch: ActivityIntensity++, then a new signal with the path plus `root`. Its
    /// TraversedCount is the parameterless constructor's 0, plus one. Identity is copied: no code of 1.28.41 writes it but
    /// the constructors (0), so it is 0 on every signal and the rewrite keeps no field.
    pub fn signal_branch(&mut self, a: &Act, root: i32) -> Act {
        self.sig_mut(a).ActivityIntensity += 1;
        let set = self.sig(a).set;
        let mut path = self.signals.sets[set].clone();
        path.push(root);
        let channel = self.sig(a).Channel;
        self.signals.new_signal(false, channel, 1, &path)
    }

    /// A signal with these fields and Path (fn-check's ActivationPropagation self: the recorder sets them on a fresh one).
    pub(crate) fn signal_with(&mut self, Direct: bool, Channel: u16, TraversedCount: i32, path: &[i32], ActivityIntensity: i32) -> Act {
        let a = self.signals.new_signal(Direct, Channel, TraversedCount, path);
        self.sig_mut(&a).ActivityIntensity = ActivityIntensity;
        a
    }

    /// The signal's Path (root ids).
    pub(crate) fn signal_path(&self, a: &Act) -> Vec<i32> {
        self.signals.sets[self.sig(a).set].clone()
    }

    /// `activation.ActivityIntensity++`
    pub fn signal_touch(&mut self, a: &Act) {
        self.sig_mut(a).ActivityIntensity += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_signals_are_disposed_on_the_second_check_and_their_sets_reused() {
        let mut s = Signals::default();
        let a = s.new_signal(false, 0, 1, &[7]);
        let b = s.new_signal(false, 0, 1, &[8]);
        assert_eq!(s.currently_active(), 2);
        s.cleanup(1.5);
        assert_eq!(s.currently_active(), 2, "the first check only records the intensity");
        b.0.lock().unwrap().ActivityIntensity += 1;
        s.cleanup(1.5);
        assert_eq!(s.currently_active(), 1, "a is disposed, b moved");
        let set = |x: &Act| x.0.lock().unwrap().set;
        assert!(s.sets[set(&a)].is_empty(), "Dispose clears the path");
        let c = s.new_signal(false, 0, 1, &[9]);
        assert_eq!(set(&c), set(&a), "the freed set is reused");
        assert_eq!(s.sets[set(&a)], vec![9], "and the old reference reads the new path");
        assert!(s.active.iter().all(|x| *x != a), "a left the active set");
        let weak = std::sync::Arc::downgrade(&a.0);
        drop(a);
        assert!(weak.upgrade().is_none(), "a disposed signal goes with its last reference");
    }

    #[test]
    fn past_the_capacity_sets_are_not_counted() {
        let mut s = Signals::default();
        for _ in 0..MAXIMUM_PROPAGATIONS + 5 {
            s.new_signal(true, 0, 1, &[]);
        }
        assert_eq!(s.currently_active(), MAXIMUM_PROPAGATIONS);
    }
}
