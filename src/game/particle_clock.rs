//! The native ParticleSystem's clock (UnityPlayer 2020.3, read in Ghidra): when a played system stops emitting, when
//! it stops, and when its stop action runs. Only systems whose stop provably does not hang on the particle random
//! (lifetimes, delays) are accepted; the rest are refused with the reason.
//!
//! The particle sim runs on Clock's passes; the stop rules and pool returns are not wired in yet: `survey::pooled_effects` shows every Poolable effect prefab has a system whose stop hangs on that
//! random (a child's lifetime reaching the duration, trails that outlive particles, sub-emitters, random delays). The
//! random comes from ParticleSystemManager's Rand, which Rand::RandomizeState (18074d2c0) seeds from system entropy,
//! so a recording has to carry its state before any pool return can be exact.
#![cfg_attr(not(test), allow(dead_code))]
use crate::assets::particles::{MinMaxCurve, ParticleSystem};

/// GetTimeStep (1801bbe50) of a system that is not fixed-step: a frame longer than Time.maximumParticleDeltaTime
/// is cut into ceil(dt / max) equal steps.
pub fn time_step(dt: f32, max: f32) -> f32 {
    if dt > max {
        dt / (dt / max).ceil()
    } else {
        dt
    }
}

/// The stop rules of one ParticleSystem.
#[derive(Clone, Debug)]
pub struct Spec {
    length: f32,
    looping: bool,
    speed: f32,
    delay: f32,
    pub play_on_awake: bool,
}

/// The constant of a MinMaxCurve: Constant, or TwoConstants with equal ends (min + (max - min) * r is min for every r).
fn constant(c: &MinMaxCurve) -> Option<f32> {
    match c.state {
        0 => Some(c.scalar),
        3 if c.min_scalar == c.scalar => Some(c.scalar),
        _ => None,
    }
}

impl Spec {
    /// The system's stop rules, or why its stop would depend on what the rewrite does not simulate.
    pub fn new(p: &ParticleSystem) -> Result<Spec, String> {
        let delay = constant(&p.start_delay).ok_or("a random start delay (CalculateStartDelay draws from the system's seed)")?;
        if p.prewarm {
            return Err("prewarm".into());
        }
        if p.unscaled_time {
            return Err("useUnscaledTime".into());
        }
        if p.ring_buffer_mode != 0 {
            return Err("ringBufferMode (particles never die)".into());
        }
        // RendererBecameInvisible: Automatic culls only looping systems; a culled one stops on its own rule.
        if !(p.culling_mode == 3 || (p.culling_mode == 0 && !p.looping)) {
            return Err(format!("cullingMode {} on a {} system (it stops by CulledSystemIsStillPlaying while off screen)", p.culling_mode, if p.looping { "looping" } else { "one-shot" }));
        }
        if !p.looping {
            // EndUpdateAll stops it once it stopped emitting and has no particle: every particle must be dead by the
            // time t reaches lengthInSec, whatever its lifetime draw. The 0.1% covers the aliveTimePercent sums.
            if !p.subs.is_empty() {
                return Err("sub-emitters".into());
            }
            if p.lifetime_by_speed.is_some() {
                return Err("lifetime by emitter speed".into());
            }
            if p.trail.as_ref().is_some_and(|t| !t.die_with_particles) {
                return Err("trails that outlive their particles".into());
            }
            let life = match p.start_lifetime.state {
                0 => p.start_lifetime.scalar,
                3 => p.start_lifetime.min_scalar.max(p.start_lifetime.scalar),
                s => return Err(format!("startLifetime mode {s}")),
            };
            let mut last = None::<f32>;
            if p.emission_enabled {
                if constant(&p.rate_over_time) != Some(0.0) || constant(&p.rate_over_distance) != Some(0.0) {
                    return Err("emission over time or distance (the last particle's lifetime draw decides the stop)".into());
                }
                for b in &p.bursts {
                    if b.cycles <= 0 {
                        return Err("an infinite burst".into());
                    }
                    let t = b.time + (b.cycles - 1) as f32 * b.interval;
                    last = Some(last.map_or(t, |l: f32| l.max(t)));
                }
            }
            if let Some(last) = last {
                if (last + life * 1.001) as f64 >= p.length as f64 {
                    return Err(format!("particles that can outlive the duration ({last} + lifetime {life} vs {})", p.length));
                }
            }
        }
        Ok(Spec::of(p, delay))
    }

    /// The clock inputs of any system, with its start delay.
    pub fn of(p: &ParticleSystem, delay: f32) -> Spec {
        Spec { length: p.length, looping: p.looping, speed: p.simulation_speed, delay, play_on_awake: p.play_on_awake }
    }
}

/// One pass of Update1Incremental's loop, after Tick and the stopEmitting check.
#[derive(Clone, Copy, Debug)]
pub struct Substep {
    /// The pass's dt.
    pub d: f32,
    /// EmitOverTime's times: min(t, length) before Tick (plus 1e-6 when d reaches length) and t after it.
    pub from: f32,
    pub to: f32,
    /// The emission's dt after the start delay; None once stopEmitting.
    pub emit: Option<f32>,
    /// max(0, accumulatedDt / d - 1): StartParticles' rewind for the passes still to come.
    pub left: f32,
    /// accumulatedDt during the pass (before its d comes off): the timeAlive of the sub-emitter commands it records.
    pub accumulated: f32,
}

/// ParticleSystemState's clock.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    t: f32,
    accumulated: f32,
    delay: f32,
    pub stop_emitting: bool,
}

impl Clock {
    /// Play with needRestart (t, numLoops and accumulatedDt cleared, CalculateStartDelay of a constant delay).
    pub fn play(s: &Spec) -> Clock {
        Clock { t: 0.0, accumulated: 0.0, delay: s.delay, stop_emitting: false }
    }

    /// ParticleSystemState::Tick (1801e1330).
    fn tick(&mut self, s: &Spec, dt: f32) {
        self.t += dt;
        if !s.looping {
            if self.t > s.length {
                self.t = s.length;
            }
            return;
        }
        while s.length <= self.t {
            self.t -= s.length;
        }
    }

    /// Update1b (1801c5800) and Update1Incremental's (1801c5210) clock for a frame of `dt` (Time.deltaTime), with
    /// Time.maximumParticleDeltaTime `max`.
    pub fn update(&mut self, s: &Spec, dt: f32, max: f32) {
        if let Some((_, step)) = self.begin(s, dt, max) {
            self.substeps(s, step, |_| ());
        }
    }

    pub fn t(&self) -> f32 {
        self.t
    }

    pub fn accumulated(&self) -> f32 {
        self.accumulated
    }

    /// Update1b's frame: `dt` times the simulation speed, its time step, and accumulatedDt += dt. None when the step
    /// is under 1e-5 and nothing runs.
    pub fn begin(&mut self, s: &Spec, dt: f32, max: f32) -> Option<(f32, f32)> {
        self.begin_step(s, dt, |dt| time_step(dt, max))
    }

    /// `begin` with GetTimeStep's step: `time_step`, or Time.fixedDeltaTime under kSimulateFixedTimeStep.
    pub fn begin_step(&mut self, s: &Spec, dt: f32, step: impl Fn(f32) -> f32) -> Option<(f32, f32)> {
        let dt = dt * if 0.0 <= s.speed { s.speed } else { 0.0 };
        let step = step(dt);
        if !(1e-5 <= step) {
            return None;
        }
        self.accumulated += dt;
        Some((dt, step))
    }

    /// Update1Incremental's loop: `pass` runs UpdateModulesIncremental and the emission of each pass.
    pub fn substeps(&mut self, s: &Spec, step: f32, mut pass: impl FnMut(&Substep)) {
        let entry = self.t;
        let mut last = step;
        while 1e-6 <= self.accumulated {
            let acc = self.accumulated;
            let mut d = if acc < step { acc } else { step };
            if acc <= 10.0 {
                if 5.0 < acc {
                    d = last;
                    if last <= 0.2 {
                        d = if 0.2 <= s.length { 0.2 } else { s.length };
                    }
                }
            } else {
                d = last;
                if last <= 1.0 {
                    d = if 1.0 <= s.length { 1.0 } else { s.length };
                }
            }
            let mut from = if s.length <= self.t { s.length } else { self.t };
            if s.length <= d {
                from += 1e-6;
            }
            if self.delay < d {
                self.tick(s, d - self.delay);
            }
            let left = self.accumulated / d - 1.0;
            let left = if left <= 0.0 { 0.0 } else { left };
            if !s.looping && !self.stop_emitting && s.length <= self.t {
                self.stop_emitting = true;
            }
            let mut emit = None;
            if !self.stop_emitting {
                let mut e = d;
                if entry == 0.0 && 0.0 < self.delay {
                    let rest = self.delay - d;
                    e = if 0.0 <= -rest { -rest } else { 0.0 };
                    self.delay = if rest < 0.0 { 0.0 } else { rest };
                }
                emit = Some(e);
            }
            pass(&Substep { d, from, to: self.t, emit, left, accumulated: self.accumulated });
            self.accumulated -= d;
            last = d;
        }
    }
}

    /// A pooled effect prefab's ParticleSystems by prefab object. Its root's stop action (Callback) runs at the
/// EndUpdateAll where the root and every system still active below it have stopped (AllChildrenAreStopped, 1801b4d80;
/// a root that stopped first waits in inactiveEmittersWithStopActions and is checked later in the same EndUpdateAll).
#[derive(Clone, Debug)]
pub struct Effect {
    pub systems: Vec<(usize, Spec)>,
    pub max_step: f32,
}

impl Effect {
    pub fn new(db: &crate::assets::db::AssetDb, prefab: &crate::assets::db::Prefab) -> Result<Effect, String> {
        let mut systems = Vec::new();
        for (o, ob) in prefab.objects.iter().enumerate() {
            for (r, c) in &ob.components {
                if matches!(c, crate::assets::db::Component::Other(198)) {
                    let p = ParticleSystem::read(db.data(*r)).map_err(|e| format!("ParticleSystem: {e}"))?;
                    if o != 0 && p.stop_action != 0 {
                        return Err(format!("{}: a child system with stopAction {}", ob.name, p.stop_action));
                    }
                    systems.push((o, Spec::new(&p).map_err(|e| format!("{}: {e}", ob.name))?));
                }
            }
        }
        if !systems.first().is_some_and(|(o, s)| *o == 0 && s.play_on_awake) {
            return Err("a root without a playOnAwake ParticleSystem".into());
        }
        Ok(Effect { systems, max_step: maximum_particle_delta_time(db)? })
    }

    /// SetActive(true): each playOnAwake system on an object `active` in the hierarchy plays.
    pub fn play(&self, active: impl Fn(usize) -> bool) -> Vec<(usize, Clock)> {
        self.systems.iter().enumerate().filter(|(_, (o, s))| s.play_on_awake && active(*o)).map(|(i, (_, s))| (i, Clock::play(s))).collect()
    }

    /// Replays the same systems (the clone's objects kept their active state).
    pub fn replay(&self, clocks: &mut [(usize, Clock)]) {
        for (i, c) in clocks {
            *c = Clock::play(&self.systems[*i].1);
        }
    }

    /// BeginUpdateAll's update of the playing systems.
    pub fn update(&self, clocks: &mut [(usize, Clock)], dt: f32) {
        for (i, c) in clocks {
            c.update(&self.systems[*i].1, dt, self.max_step);
        }
    }

    /// At EndUpdateAll: all stopped (no particle is left once a system stopped emitting, by Spec::new).
    pub fn stopped(clocks: &[(usize, Clock)]) -> bool {
        clocks.iter().all(|(_, c)| c.stop_emitting)
    }
}

/// Time.maximumParticleDeltaTime: globalgamemanagers' TimeManager, after Fixed Timestep, Maximum Allowed Timestep and
/// m_TimeScale (StreamedBinaryRead, 1805359f0).
pub fn maximum_particle_delta_time(db: &crate::assets::db::AssetDb) -> Result<f32, String> {
    let file = db.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
    let manager = db.files[file].objects.iter().find(|o| o.class_id == 5).ok_or("no TimeManager")?;
    let r = &mut crate::assets::serialized::Reader::new(db.data(crate::assets::db::ObjRef { file, path_id: manager.path_id }));
    for _ in 0..3 {
        r.f32().map_err(|e| e.to_string())?;
    }
    r.f32().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_step_splits_long_frames() {
        assert_eq!(time_step(0.02, 0.03), 0.02);
        assert_eq!(time_step(0.25, 0.03), 0.25 / 9.0);
        let s = Spec { length: 0.5, looping: false, speed: 1.0, delay: 0.0, play_on_awake: true };
        let mut c = Clock::play(&s);
        for _ in 0..7 {
            c.update(&s, 0.1, 0.03);
        }
        assert!(c.stop_emitting);
        assert_eq!(c.t, 0.5);
    }
}

#[cfg(test)]
mod survey {
    /// Every Poolable prefab with a ParticleSystem: its stop rules, or why they are refused.
    #[test]
    #[ignore]
    fn pooled_effects() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut roots = Vec::new();
        for (file, f) in db.files.iter().enumerate() {
            for o in f.objects.iter().filter(|o| o.class_id == 1) {
                let r = crate::assets::db::ObjRef { file, path_id: o.path_id };
                if db.game_object(r).is_ok_and(|g| g.tag == super::super::water::POOLABLE_TAG) {
                    roots.push(r);
                }
            }
        }
        for r in roots {
            let Ok(prefab) = db.prefab(r) else { continue };
            if !prefab.objects[0].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Other(198))) {
                continue;
            }
            println!("{:40} {:?}", prefab.objects[0].name, super::Effect::new(&db, &prefab).map(|e| e.systems.len()));
        }
    }
}
