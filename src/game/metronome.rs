//! MetronomeBehaviour: while Activated it ticks at CurrentTempo Hz, and each tick sends IsolatedActivation on every
//! channel. Start draws the seed of the tempo's Perlin wobble; Use toggles it. Update turns the Hand along HandCurve
//! (ObjectState saves its pose); the context menu is presentation.
#![allow(non_snake_case)]

use super::misc::FixedIntervalDistributor;
use super::{mathf, script, Game};
use crate::game_assets::{Curve, CurveCache};

script! {
    Metronome {
        TempoModifier: f32 = 1.0,
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour: the Charge that speeds the tempo up.
        phys: usize = 0,
        animationTimer: f32 = 0.0,
        seed: f32 = 0.0,
        /// muteBehaviour: the MuteBehaviour component's id on its object (None: null).
        mute: Option<i32> = None,
        distributor: FixedIntervalDistributor = FixedIntervalDistributor::with_rate(60.0),
        /// HandCurve and its evaluation cache (AnimationCurve's native cache).
        HandCurve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
        /// Hand: its Transform id (None: null, an NRE in Update).
        hand: Option<i32> = None,
        TickAngleBound: f32 = 11.0,
    }
}

impl Game {
    // MetronomeBehaviour.Awake: muteBehaviour = GetComponent<MuteBehaviour>() (the `mute` link).
    /// MetronomeBehaviour.CurrentTempo
    pub(crate) fn metronome_tempo(&self, m: usize) -> f32 {
        let s = &self.metronomes[m];
        let charge = self.physicals[s.phys].Charge();
        let num = if charge > 0.01 { mathf::lerp(1.0, 25.0 * mathf::perlin(s.seed, self.time.time * 4.0), charge * 0.05) } else { 1.0 };
        mathf::clamp(s.TempoModifier * num, 0.125, 32.0)
    }

    /// MetronomeBehaviour.UpdateActivation: an activated metronome restarts its clock half a tick in and its animation.
    pub(crate) fn metronome_update_activation(&mut self, m: usize) {
        if self.metronomes[m].Activated {
            let tempo = self.metronome_tempo(m);
            self.metronomes[m].distributor.reset_accumulator(0.5f32 / tempo);
            self.metronomes[m].animationTimer = 0.0;
        }
    }

    /// MetronomeBehaviour.Start: seed = Random.value * 1000; the context menu button is UI.
    pub fn metronome_start(&mut self, m: usize) {
        self.metronomes[m].seed = self.random.value() * 1000.0;
        let p = self.metronomes[m].phys;
        self.physicals[p].menu.push(super::menu::MenuButton::new("setMetronomeFrequency", crate::il!(str "MetronomeBehaviour::Start" 1), crate::il!(str "MetronomeBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::Metronome(m))));
        self.metronome_update_activation(m);
    }

    /// MetronomeBehaviour.Update. The tick loop's condition is evaluated every pass, and each evaluation advances the
    /// distributor, exactly as the C# for-loop does.
    pub fn metronome_update(&mut self, m: usize) {
        let dt = self.time.deltaTime;
        let tempo = self.metronome_tempo(m);
        self.metronomes[m].animationTimer += dt * tempo;
        // distributor.RateHz = CurrentTempo: IntervalSeconds = 1f / value.
        let tempo = self.metronome_tempo(m);
        self.metronomes[m].distributor.set_RateHz(tempo);
        if self.metronomes[m].Activated {
            let mut i = 0;
            while i < self.metronomes[m].distributor.calculate_cycle_count(dt) {
                // SendMessage("IsolatedActivation", ..., DontRequireReceiver) for each channel to its own object.
                self.send_isolated(self.metronomes[m].phys, false);
                // if (!muteBehaviour.Muted) PhysicalBehaviour.PlayClipOnce(Tick)
                let mute = self.metronomes[m].mute.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                let k = self.mute_ids.iter().position(|&id| id == mute).unwrap_or_else(|| unimplemented!("mod-only: MetronomeBehaviour: a MuteBehaviour that never started (no PhysicalBehaviour)"));
                if !self.mutes[k] {
                    let clip = self.audio.clip(self.metronomes[m].id, "Tick");
                    self.play_clip_once(self.metronomes[m].phys, clip, 1.0);
                }
                i += 1;
            }
        }
        // float t = HandCurve.Evaluate(animationTimer); target = Activated ? Lerp(-TickAngleBound, TickAngleBound, t) : 0
        let s = &mut self.metronomes[m];
        let t = s.HandCurve.as_ref().unwrap_or_else(|| super::menu::npe()).evaluate(&mut s.curve_cache, s.animationTimer);
        let target = if s.Activated { mathf::lerp(0.0 - s.TickAngleBound, s.TickAngleBound, t) } else { 0.0 };
        // Hand.localEulerAngles: z += DeltaAngle(z, target) * deltaTime * 6 (x and y stay 0 on the hand's pure z rotation).
        let Some(hand) = s.hand else { super::menu::npe() };
        let mut z = 0.0;
        self.with_component_instance(hand, |_, inst, o| z = super::dump::euler_z(inst.objects[o].trs.q));
        let z = z + mathf::delta_angle(z, target) * dt * 6.0;
        self.with_component_instance(hand, |_, inst, o| inst.set_local_rotation(o, super::menu::euler_z_rotation(z)));
    }

    /// MetronomeBehaviour.Use: `base.enabled && !activation.Contains(root)`; a direct activation's path holds no object.
    pub fn metronome_use(&mut self, m: usize) {
        if self.metronomes[m].enabled {
            self.metronomes[m].Activated = !self.metronomes[m].Activated;
            self.metronome_update_activation(m);
        }
    }

    /// MetronomeBehaviour.OnDisable: Activated = false, then StopAllCoroutines (it starts none).
    pub fn metronome_on_disable(&mut self, m: usize) {
        self.metronomes[m].Activated = false;
        let id = self.metronomes[m].id;
        self.stop_all_coroutines(id);
    }
}
