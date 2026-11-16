//! DefibrillatorBehaviour and CardiopulmonaryBypassMachineBehaviour.
//!
//! The defibrillator's Use runs DefibRoutine: busy for the two seconds of STAND BY plus the indicator clip's length, and
//! at the CLEAR both pads gain 0.2 charge and restart every circulation under them (IsPump back to what it started as,
//! full blood flow, 20 adrenaline, full consciousness). The stand-clear clip and the indicator play through the
//! PhysicalBehaviour's PlayClipOnce.
//!
//! The bypass machine's Use toggles Activated; UpdateActivation shows or hides its Cosmetics object (and so wakes or
//! stops what hangs under it). Its pumping is BloodWireBehaviour.ProcessCardiopulmonaryBypassWire (blood_wire.rs), the
//! only caller of GetFinalIntensity, PlayValidSound and PlayInvalidSound (which draw no Random).
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::spawn::Instance;
use super::{script, Game};
use crate::b2::math::Vec2 as V;

script! {
    Defibrillator {
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// TextScreen (world_text.rs).
        TextScreen: Option<usize> = None,
        /// PhysicalBehaviour (its PlayClipOnce plays StandClearClip and AudioIndicator).
        physical: usize = 0,
        /// Pad1 and Pad2 (PhysicalBehaviour).
        Pad1: usize = 0,
        Pad2: usize = 0,
        /// PadLayer (LayerMask bits).
        PadLayer: u32 = 0,
        /// AudioIndicator.length (None: a null clip, whose length throws).
        AudioIndicator: Option<f32> = None,
        /// busy: a DefibRoutine is running.
        busy: bool = false,
        /// bufferSize: the colliders of the last pad's overlap.
        bufferSize: i32 = 0,
    }
}

script! {
    CardiopulmonaryBypass {
        Activated: bool = false,
        PressureIntensity: f32 = 1.0,
        ValidConnectionCount: i32 = 0,
        TargetHeartrate: f32 = 70.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// The PhysicalBehaviour on its GameObject (a BloodWireBehaviour finds the machine through it).
        physical: usize = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// Cosmetics: the index of its object in the prefab.
        Cosmetics: usize = 0,
        /// StatusText (world_text.rs).
        StatusText: Option<usize> = None,
    }
}

impl Game {
    /// DefibrillatorBehaviour.Use: while enabled and not busy, StartCoroutine(DefibRoutine()).
    pub fn defibrillator_use(&mut self, d: usize) {
        if self.defibrillators[d].enabled && !self.defibrillators[d].busy {
            let id = self.defibrillators[d].id;
            self.start_coroutine(id, id, Self::defibrillator_routine(d));
        }
    }

    /// DefibrillatorBehaviour.OnDisable: StopAllCoroutines, the TextScreen is disabled and cleared, and busy is false.
    pub fn defibrillator_on_disable(&mut self, d: usize) {
        let id = self.defibrillators[d].id;
        self.stop_all_coroutines(id);
        let text = Self::text_ref(self.defibrillators[d].TextScreen);
        self.text_enabled_set(text, false);
        self.text_set(text, "");
        self.defibrillators[d].busy = false;
    }

    /// DefibrillatorBehaviour.Start: TextScreen.text = string.Empty.
    pub fn defibrillator_start(&mut self, d: usize) {
        let text = self.defibrillators[d].TextScreen;
        self.text_write(text, "");
    }

    /// DefibrillatorBehaviour.OnEnable: TextScreen.enabled = true.
    pub fn defibrillator_on_enable(&mut self, d: usize) {
        let text = Self::text_ref(self.defibrillators[d].TextScreen);
        self.text_enabled_set(text, true);
    }

    /// DefibrillatorBehaviour.DefibRoutine
    pub(crate) fn defibrillator_routine(d: usize) -> Routine {
        let mut stage = 0;
        Box::new(move |game| {
            stage += 1;
            match stage {
                // busy, "STAND BY" and the stand-clear clip; then two seconds.
                1 => {
                    game.defibrillators[d].busy = true;
                    let text = game.defibrillators[d].TextScreen;
                    game.text_write(text, crate::il!(str "DefibrillatorBehaviour/<DefibRoutine>d__12::MoveNext" 0));
                    let (p, id) = (game.defibrillators[d].physical, game.defibrillators[d].id);
                    let clip = game.audio.clip(id, "StandClearClip");
                    game.play_clip_once(p, clip, 1.0);
                    Yield::WaitForSeconds(2.0)
                }
                // "CLEAR" and the indicator clip; both pads; then the clip's length.
                2 => {
                    let text = game.defibrillators[d].TextScreen;
                    game.text_write(text, "CLEAR");
                    let (p, id) = (game.defibrillators[d].physical, game.defibrillators[d].id);
                    let clip = game.audio.clip(id, "AudioIndicator");
                    game.play_clip_once(p, clip, 1.0);
                    let (pad1, pad2, layer) = (game.defibrillators[d].Pad1, game.defibrillators[d].Pad2, game.defibrillators[d].PadLayer);
                    game.defibrillate(d, pad1, layer);
                    game.defibrillate(d, pad2, layer);
                    let length = game.defibrillators[d].AudioIndicator.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                    Yield::WaitForSeconds(length)
                }
                _ => {
                    let text = game.defibrillators[d].TextScreen;
                    game.text_write(text, "");
                    game.defibrillators[d].busy = false;
                    Yield::Done
                }
            }
        })
    }

    /// DefibrillatorBehaviour.Defibrillate
    pub(crate) fn defibrillate(&mut self, d: usize, pad: usize, layer: u32) {
        let charge = self.physicals[pad].Charge();
        self.physicals[pad].charge = charge + 0.2;
        let at = self.transform_point3(pad, [-0.1, 0.0, 0.0]);
        // Physics2D.OverlapCircleNonAlloc into the 16-entry buffer.
        let mut hits = self.overlap_circle(V::new(at[0], at[1]), 0.1, layer);
        hits.truncate(16);
        self.defibrillators[d].bufferSize = hits.len() as i32;
        for (physical, _) in hits {
            // The collider's own GameObject holds the CirculationBehaviour, so the limb of its physical.
            let Some(l) = physical.and_then(|p| self.limbs.iter().position(|x| x.PhysicalBehaviour == p)) else { continue };
            self.circulations[l].IsPump = self.circulations[l].WasInitiallyPumping;
            self.circulations[l].BloodFlow = 1.0;
            let person = self.limbs[l].Person;
            self.people[person].AdrenalineLevel += 20.0;
            self.people[person].Consciousness = 1.0;
        }
    }

    /// CardiopulmonaryBypassMachineBehaviour.Use
    pub fn cardiopulmonary_bypass_use(&mut self, c: usize) {
        if self.cardiopulmonary_bypasses[c].enabled {
            self.cardiopulmonary_bypasses[c].Activated = !self.cardiopulmonary_bypasses[c].Activated;
        }
        let id = self.cardiopulmonary_bypasses[c].id;
        self.with_component_instance(id, |g, inst, _| g.cardiopulmonary_bypass_update_activation(c, inst));
    }

    /// CardiopulmonaryBypassMachineBehaviour.GetFinalIntensity: nothing while disabled or not activated.
    pub fn cardiopulmonary_bypass_final_intensity(&self, c: usize) -> f32 {
        let x = &self.cardiopulmonary_bypasses[c];
        if !x.enabled || !x.Activated {
            return 0.0;
        }
        x.PressureIntensity
    }

    /// CardiopulmonaryBypassMachineBehaviour.UpdateStatusText
    pub fn cardiopulmonary_bypass_update_status_text(&mut self, c: usize) {
        let text = if self.cardiopulmonary_bypasses[c].ValidConnectionCount == 0 { "Inactive" } else { "<color=green>Active</color>" };
        let field = self.cardiopulmonary_bypasses[c].StatusText;
        self.text_write(field, text);
    }

    /// CardiopulmonaryBypassMachineBehaviour.PlayValidSound: PlayClipOnce(ValidSound) while enabled.
    pub(crate) fn cardiopulmonary_bypass_play_valid_sound(&mut self, c: usize) {
        self.cardiopulmonary_bypass_play(c, "ValidSound");
    }

    /// CardiopulmonaryBypassMachineBehaviour.PlayInvalidSound: PlayClipOnce(InvalidSound) while enabled.
    pub(crate) fn cardiopulmonary_bypass_play_invalid_sound(&mut self, c: usize) {
        self.cardiopulmonary_bypass_play(c, "InvalidSound");
    }

    /// PhysicalBehaviour.PlayClipOnce(clip) of the machine's clip field `field`, while enabled.
    fn cardiopulmonary_bypass_play(&mut self, c: usize, field: &str) {
        let (id, enabled, p) = (self.cardiopulmonary_bypasses[c].id, self.cardiopulmonary_bypasses[c].enabled, self.cardiopulmonary_bypasses[c].physical);
        if enabled {
            let clip = self.audio.clip(id, field);
            self.play_clip_once(p, clip, 1.0);
        }
    }

    /// CardiopulmonaryBypassMachineBehaviour.OnDisable: LoopSource.Stop().
    pub fn cardiopulmonary_bypass_on_disable(&mut self, c: usize) {
        let s = self.audio_src(self.cardiopulmonary_bypasses[c].id, "LoopSource");
        if self.audio.sources.contains_key(&s) {
            self.audio.stop(s);
        }
    }

    // CardiopulmonaryBypassMachineBehaviour.OnEnable: UpdateActivation, as cardiopulmonary_bypass_start.
    // CardiopulmonaryBypassMachineBehaviour.get_IsConnected: ValidConnectionCount > 0; nothing in the game reads the property (the blood wire reads the count).
    /// CardiopulmonaryBypassMachineBehaviour.Start
    pub fn cardiopulmonary_bypass_start(&mut self, c: usize) {
        let id = self.cardiopulmonary_bypasses[c].id;
        self.with_component_instance(id, |g, inst, _| g.cardiopulmonary_bypass_update_activation(c, inst));
    }

    /// CardiopulmonaryBypassMachineBehaviour.UpdateActivation: Cosmetics.SetActive(Activated && enabled), LoopSource.Play or
    /// Stop when it is alive, then the status text while active.
    pub fn cardiopulmonary_bypass_update_activation(&mut self, c: usize, inst: &mut Instance) {
        let x = &self.cardiopulmonary_bypasses[c];
        let (cosmetics, flag, id) = (x.Cosmetics, x.Activated && x.enabled, x.id);
        self.set_active_scripts(inst, cosmetics, flag);
        if let Some(s) = self.audio.source_field(id, "LoopSource").filter(|s| self.audio.sources.contains_key(s)) {
            if flag {
                self.audio.play(s);
            } else {
                self.audio.stop(s);
            }
        }
        if flag {
            self.cardiopulmonary_bypass_update_status_text(c);
        }
    }
}
