//! FireworksBehaviour: Use lights it (Activated), then FixedUpdate pushes it along Thrust and Update counts Life up to
//! the randomised burst time. A burn above 0.1 or a shot explodes it at once. Explode makes a FireworksNuke when the
//! firework is scaled past 60, else one of the burst prefabs (ExplosiveBehaviour, scaled and with Range * scale), and
//! destroys the firework and its Killzone. The particle system is drawn in particles.rs.
#![allow(non_snake_case)]

use super::{d, script, Game};
use crate::assets::db::{Component, ObjRef};

script! {
    Fireworks {
        Thrust: [f32; 2] = [0.0; 2],
        Range: f32 = 5.0,
        TimeUntilBurst: f32 = 0.0,
        TimeUntilBurstRandomisation: f32 = 0.5,
        RandomisedTimeUntilBurst: f32 = 0.0,
        Activated: bool = false,
        Life: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        Bursts: Vec<ObjRef> = Vec::new(),
        LowChanceBursts: Vec<ObjRef> = Vec::new(),
        FireworksNuke: Option<ObjRef> = None,
        /// Killzone: the instance's Transform id of that object.
        Killzone: i32 = 0,
        /// The private alreadyShot.
        alreadyShot: bool = false,
    }
}

impl Game {
    /// FireworksBehaviour.Awake: RandomisedTimeUntilBurst = TimeUntilBurst + Random.Range(-r, r) * 0.5f.
    pub fn fireworks_awake(&mut self, f: usize) {
        let r = self.fireworks[f].TimeUntilBurstRandomisation;
        let x = self.random.range(0.0 - r, r);
        let s = &mut self.fireworks[f];
        s.RandomisedTimeUntilBurst = (d(s.TimeUntilBurst) + d(x) * 0.5f32).f();
    }

    /// FireworksBehaviour.Use: the first one sets Activated, plays Audio and UpdateActivation.
    pub fn fireworks_use(&mut self, f: usize) {
        if !self.fireworks[f].Activated {
            self.fireworks[f].Activated = true;
            let src = self.audio.source_field(self.fireworks[f].id, "Audio").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            self.audio.play(src);
            self.fireworks_update_activation(f);
        }
    }

    // FireworksBehaviour.Start: UpdateActivation, the fn below (behaviour start dispatch, Start::Fireworks).
    /// FireworksBehaviour.UpdateActivation: ParticleSystem.Play(withChildren: false) while Activated, else
    /// Stop(withChildren: false). FireworksBehaviour.Start calls it.
    pub fn fireworks_update_activation(&mut self, f: usize) {
        let (id, on) = (self.fireworks[f].id, self.fireworks[f].Activated);
        self.ps_call(|e| e.owner == id && matches!(e.drive, super::EmbeddedDrive::Firework(_)), if on { super::PsCall::Play } else { super::PsCall::Stop(self.time.timeAsDouble) });
    }

    /// FireworksBehaviour.GetScale: Mathf.Abs(lossyScale.x * lossyScale.y).
    pub(crate) fn fireworks_scale(&self, f: usize) -> f32 {
        let s = self.xf_lossy_scale(&self.fireworks[f].own);
        (s[0] * s[1]).abs()
    }

    /// FireworksBehaviour.FixedUpdate
    pub fn fireworks_fixed_update(&mut self, f: usize) {
        let p = self.fireworks[f].physical;
        if self.physicals[p].BurnProgress > 0.1 {
            self.fireworks[f].Life = f32::NEG_INFINITY;
            self.fireworks[f].Activated = true;
            self.fireworks_explode(f);
        } else if self.fireworks[f].Activated {
            let (t, s) = (self.fireworks[f].Thrust, self.fireworks_scale(f));
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_relative_force(&mut self.world, [t[0] * s, t[1] * s]);
        }
    }

    /// FireworksBehaviour.Update
    pub fn fireworks_update(&mut self, f: usize) {
        if self.fireworks[f].Activated {
            let s = &mut self.fireworks[f];
            s.Life += self.time.deltaTime;
            if s.Life > s.RandomisedTimeUntilBurst {
                self.fireworks_explode(f);
            }
        } else {
            // Killzone.transform.localScale = 2f * GetScale() * Range * Vector3.one
            let k = (d(2.0) * self.fireworks_scale(f) * self.fireworks[f].Range).f();
            let killzone = self.fireworks[f].Killzone;
            self.with_component_instance(killzone, |_, inst, o| inst.set_local_scale(o, [k; 3]));
        }
    }

    /// FireworksBehaviour.Shot
    pub fn fireworks_shot(&mut self, f: usize) {
        if !self.fireworks[f].alreadyShot {
            let s = &mut self.fireworks[f];
            s.alreadyShot = true;
            s.Life = f32::NEG_INFINITY;
            s.Activated = true;
            self.fireworks_explode(f);
        }
    }

    /// FireworksBehaviour.Explode
    pub(crate) fn fireworks_explode(&mut self, f: usize) {
        let scale = self.fireworks_scale(f);
        let id = self.fireworks[f].id;
        // base.transform.position
        let at = self.xf_position(&self.fireworks[f].own);
        if scale > 60.0 {
            let nuke = self.fireworks[f].FireworksNuke.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            self.spawn_prefab(nuke, at);
            self.destroy_object(id);
            return;
        }
        let low = (self.random.value() as f64) > 0.999;
        let list = if low { self.fireworks[f].LowChanceBursts.clone() } else { self.fireworks[f].Bursts.clone() };
        let k = self.pick_random(list.len()).unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        let root = self.spawn_prefab(list[k], at);
        let range = self.fireworks[f].Range * scale;
        let explosive = self.with_component_instance(root, |_, inst, o| {
            inst.set_local_scale(o, [scale; 3]);
            let k = inst.prefab.objects[o].components.iter().position(|(_, c)| matches!(c, Component::Script { class, .. } if class == "ExplosiveBehaviour"));
            k.map(|k| inst.objects[o].components[k])
        });
        let explosive = explosive.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let e = self.explosives.iter().position(|e| e.id == explosive).expect("the burst's ExplosiveBehaviour");
        self.explosives[e].Range = range;
        // main.simulationSpeed = 1f / scale: presentation.
        self.destroy_object(id);
        let killzone = self.fireworks[f].Killzone;
        self.destroy_object(killzone);
    }
}
