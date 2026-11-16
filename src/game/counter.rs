//! CounterBehaviour and PointerBehaviour.
//! Counter: Use counts up on channel 0, down on channel 1 and clears on channel 2; reaching ResetValue sends a signal
//! (IsolatedActivation on each channel, with the physical's ActivationPropagationDelay held at ActivationDelay) and
//! clears; each change shows on the NixieTubeBehaviour's digits (`_DigitIndex` of Unlit/nixie_digit_shader, custom_material.rs).
//! The dialog box is presentation.
//! Pointer: Use toggles Activated; FixedUpdate turns the hinge motor towards the mouse (or the kept Delta). OnEnable and
//! OnDisable switch the motor (their arms are at the CeilingTurret sites). ActiveLight follows Activated (pointer_light).
#![allow(non_snake_case)]

use super::{mathf, script, BareAnchor, Game};
use crate::unity::transform::Trs;

script! {
    Counter {
        CurrentCounter: i32 = 0,
        MinValue: i32 = 0,
        MaxValue: i32 = 99,
        ResetValue: i32 = 100,
        ActivationDelay: f32 = 0.1,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (GetComponent in Start).
        physical: usize = 0,
        /// NixieTubeBehaviour.DigitRenderers
        digits: Vec<Option<super::visual::SpriteAt>> = Vec::new(),
        /// The NixieTubeBehaviour's instance id.
        nixie: i32 = 0,
    }
}

script! {
    Pointer {
        Activated: bool = false,
        Speed: f32 = 1.0,
        Force: f32 = 1.0,
        Delta: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour.
        physical: usize = 0,
        /// The Hinge's `Game::hinges` index.
        hinge: usize = 0,
        /// VolumeMultiplier
        VolumeMultiplier: f32 = 0.5,
        /// RotorWheel's Transform: what it hangs from and the local TRSs up to that (object first).
        rotor_anchor: BareAnchor = BareAnchor::Root,
        rotor_chain: Vec<Trs> = Vec::new(),
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// ActiveLight (a SpriteRenderer; None only for a pointer rebuilt from a probe dump).
        ActiveLight: Option<super::visual::SpriteAt> = None,
    }
}

/// CounterBehaviour.Use's counting, without the signal: the new value and whether it reached ResetValue.
pub fn counter_step(value: i32, channel: u16, min: i32, max: i32, reset: i32) -> (i32, bool) {
    let mut v = match channel {
        1 => value - 1,
        2 => min,
        _ => value + 1,
    };
    let signal = v == reset;
    if signal {
        v = min;
    }
    if v > max {
        v = min;
    }
    if v < min {
        v = max;
    }
    (v, signal)
}

/// Mono's conv.i4 of a float (cvttss2si): NaN, an infinity or a value outside int is int.MinValue.
fn conv_i4(v: f32) -> i32 {
    if v.is_finite() && v > -2147483904.0 && v < 2147483648.0 { v as i32 } else { i32::MIN }
}

/// NixieTubeBehaviour.SetValue's digit for renderer `i` of `len`: getDigit(number, i - (len - digitCount)), digitCount =
/// (int)Floor(Log10(number)) + 1, all int arithmetic wrapping.
pub fn nixie_digit(number: i32, i: i32, len: i32) -> i32 {
    let digit_count = conv_i4(mathf::floor((number as f32 as f64).log10() as f32)).wrapping_add(1);
    let mut num2 = i.wrapping_sub(len.wrapping_sub(digit_count));
    if digit_count < num2 {
        return 0;
    }
    num2 = num2.wrapping_add(1);
    // Floor((float)x / Pow(10f, digitCount - num2) % 10f)
    let pow = 10f64.powf(digit_count.wrapping_sub(num2) as f32 as f64) as f32;
    conv_i4(mathf::floor(((number as f32 as f64) / pow as f64 % 10.0) as f32))
}

impl Game {
    /// NixieTubeBehaviour.SetValue(number) on the counter's tube: each digit renderer's block, read again, takes its digit as
    /// `_DigitIndex`.
    pub fn nixie_set_value(&mut self, c: usize, number: i32) {
        let len = self.counters[c].digits.len() as i32;
        for i in 0..len {
            let Some(at) = self.counters[c].digits[i as usize] else { continue };
            let mut block = self.get_property_block(at);
            block.set_float("_DigitIndex", nixie_digit(number, i, len) as f32);
            self.set_property_block(at, &block);
        }
    }

    /// CounterBehaviour.Start: NixieTubeBehaviour.SetValue(CurrentCounter), ActivationPropagationDelay = -1 (the buttons are in
    /// menu.rs).
    pub fn counter_start(&mut self, c: usize) {
        self.nixie_set_value(c, self.counters[c].CurrentCounter);
        let p = self.counters[c].physical;
        self.physicals[p].ActivationPropagationDelay = -1.0;
    }

    /// CounterBehaviour.Use.
    pub fn counter_use(&mut self, c: usize, channel: u16) {
        if !self.counters[c].enabled {
            return;
        }
        let s = &self.counters[c];
        let (v, signal) = counter_step(s.CurrentCounter, channel, s.MinValue, s.MaxValue, s.ResetValue);
        // SendSignal runs between the reset-value check and the wrap, which only read CurrentCounter after it.
        self.counters[c].CurrentCounter = v;
        if signal {
            self.counter_send_signal(c);
        }
        self.nixie_set_value(c, v);
    }

    /// CounterBehaviour.SendSignal: BroadcastMessage("IsolatedActivation") on each channel; only use wires take it (link
    /// refuses one below the counter in the prefab, and a physical in its children).
    pub(crate) fn counter_send_signal(&mut self, c: usize) {
        let p = self.counters[c].physical;
        // phys.PlayClipOnce(SignalClip)
        let clip = self.audio.clip(self.counters[c].id, "SignalClip");
        self.play_clip_once(p, clip, 1.0);
        self.physicals[p].ActivationPropagationDelay = self.counters[c].ActivationDelay;
        for channel in 0..3u16 {
            let root = self.root_id(p);
            let a = self.signal_from(root, channel);
            for w in self.use_wires_on(p) {
                self.use_wire_receive(w, &a, false);
            }
        }
        self.physicals[p].ActivationPropagationDelay = -1.0;
    }

    /// The counter's resetCounter button: CurrentCounter = MinValue; NixieTubeBehaviour.SetValue(CurrentCounter).
    pub fn counter_reset(&mut self, c: usize) {
        self.counters[c].CurrentCounter = self.counters[c].MinValue;
        self.nixie_set_value(c, self.counters[c].CurrentCounter);
    }

    /// PointerBehaviour.OnEnable: Hinge.useMotor = true.
    pub fn pointer_on_enable(&mut self, i: usize) {
        let h = self.pointers[i].hinge;
        if self.hinges[h].destroyed {
            crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
        }
        self.hinges[h].set_use_motor(&mut self.world, true);
    }

    /// PointerBehaviour.OnDisable: Hinge.useMotor = false, AudioSource.Stop().
    pub fn pointer_on_disable(&mut self, i: usize) {
        let h = self.pointers[i].hinge;
        if self.hinges[h].destroyed {
            crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
        }
        self.hinges[h].set_use_motor(&mut self.world, false);
        let src = self.pointer_source(i);
        self.audio.stop(src);
    }

    /// AudioSource (a null one throws).
    fn pointer_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.pointers[i].id, "AudioSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// PointerBehaviour.Use: ToggleActivation.
    pub fn pointer_use(&mut self, i: usize) {
        self.pointer_toggle_activation(i);
    }

    /// PointerBehaviour.ToggleActivation: Activated = !Activated; ActiveLight.enabled = Activated.
    pub fn pointer_toggle_activation(&mut self, i: usize) {
        self.pointers[i].Activated = !self.pointers[i].Activated;
        self.pointer_light(i);
    }

    /// ActiveLight.enabled = Activated (Start and ToggleActivation).
    pub fn pointer_light(&mut self, i: usize) {
        if let Some(r) = self.pointers[i].ActiveLight {
            self.set_sprite_enabled(r, self.pointers[i].Activated);
        }
    }

    /// PointerBehaviour.GetMouseAngle.
    pub(crate) fn pointer_mouse_angle(&self, i: usize) -> f32 {
        let s = &self.pointers[i];
        let wheel = self.anchored_world(s.rotor_anchor, &s.rotor_chain).0;
        let m = self.MousePosition;
        let n = super::explosion::normalized3([m[0] - wheel[0], m[1] - wheel[1], m[2] - wheel[2]]);
        let n = self.xf_inverse_direction(&s.own, n);
        // Vector2.SignedAngle((-x, y), Vector2.up)
        super::automatic_sentry::signed_angle([0.0 - n[0], n[1]], [0.0, 1.0])
    }

    /// PointerBehaviour.FixedUpdate
    pub fn pointer_fixed_update(&mut self, i: usize) {
        let h = self.pointers[i].hinge;
        if self.hinges[h].destroyed {
            crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
        }
        if self.pointers[i].Activated {
            self.pointers[i].Delta = self.pointer_mouse_angle(i);
        }
        let s = &self.pointers[i];
        let speed = mathf::delta_angle(self.hinges[h].joint_angle(&self.world), s.Delta) * s.Speed;
        let torque = s.Force * (1.0 + self.physicals[s.physical].Charge());
        self.hinges[h].set_motor(&mut self.world, speed, torque);
        let num = self.hinges[h].joint_speed(&self.world).abs() * 0.1;
        let src = self.pointer_source(i);
        if num > 0.5 {
            if !self.audio.is_playing(src) {
                self.audio.play(src);
            }
            self.audio.get_mut(src).volume = mathf::clamp01(num * num * self.pointers[i].VolumeMultiplier);
        } else if self.audio.is_playing(src) {
            self.audio.stop(src);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::counter_step;

    #[test]
    fn counts_and_wraps() {
        assert_eq!(counter_step(5, 0, 0, 99, 100), (6, false));
        assert_eq!(counter_step(5, 1, 0, 99, 100), (4, false));
        assert_eq!(counter_step(5, 2, 0, 99, 100), (0, false));
        assert_eq!(counter_step(0, 1, 0, 99, 100), (99, false));
        assert_eq!(counter_step(99, 0, 0, 99, 50), (0, false));
        assert_eq!(counter_step(99, 0, 0, 99, 100), (0, true));
    }

    #[test]
    fn reset_value_signals_and_clears() {
        assert_eq!(counter_step(9, 0, 0, 99, 10), (0, true));
        // A reset value of 0 on channel 2 signals too.
        assert_eq!(counter_step(7, 2, 0, 99, 0), (0, true));
    }
}

#[cfg(test)]
mod nixie_tests {
    #[test]
    fn nixie_digits() {
        // 42 on three tubes: 0, 4, 2; 7 on two: 0, 7; 0 on two (Log10(0) is -inf: digitCount int.MinValue + 1).
        assert_eq!((0..3).map(|i| super::nixie_digit(42, i, 3)).collect::<Vec<_>>(), [0, 4, 2]);
        assert_eq!((0..2).map(|i| super::nixie_digit(7, i, 2)).collect::<Vec<_>>(), [0, 7]);
        assert_eq!((0..2).map(|i| super::nixie_digit(100, i, 2)).collect::<Vec<_>>(), [0, 0]);
        assert_eq!((0..2).map(|i| super::nixie_digit(0, i, 2)).collect::<Vec<_>>(), [0, 0]);
    }
}
