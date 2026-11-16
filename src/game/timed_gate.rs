//! TimedGateBehaviour, a LagboxBehaviour: a Use (while enabled and not busy) starts DoLag, which holds the physical's
//! ActivationPropagationDelay at -1 for DelayModifier seconds of frames and then restores it to 0. Start, OnEnable and
//! OnDisable keep the coroutine running or stop it. Lights, knob, labels and the bell and redlight sounds are presentation.
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::{d, mathf, script, Game};

script! {
    TimedGate {
        /// LagboxBehaviour.DelayModifier (the gate's duration in seconds, from 0.04 to 16).
        DelayModifier: f32 = 1.0,
        /// isBusy: a DoLag is running.
        isBusy: bool = false,
        /// The private t: how far the gate has gone, 0 to 1.
        t: f32 = 0.0,
    }
    links {
        /// GreenLight and RedLight (Start and DoLag switch them; null throws there).
        GreenLight: Option<super::visual::SpriteAt> = None,
        RedLight: Option<super::visual::SpriteAt> = None,
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour (its ActivationPropagationDelay is the gate's state).
        physical: usize = 0,
        /// MuteBehaviour = GetComponent<MuteBehaviour>(): the component's id (None: null).
        mute: Option<i32> = None,
        /// Knob (Update and DoLag write it) is null.
        knob_null: bool = false,
        /// Knob: its Transform id (DoLag writes its eulerAngles).
        knob: Option<i32> = None,
    }
}

script! {
    Gate {
        Passthrough: bool = false,
        Threshold: f32 = 0.0,
        MaxPower: f32 = 100.0,
        ThresholdPercentage: f32 = 50.0,
        DoubleTrigger: bool = false,
        ActivationDelay: f32 = 0.1,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// PowerLevelBar, PassthroughLight and Cursor (Awake and Update dereference them).
        PowerLevelBar: Option<super::visual::SpriteAt> = None,
        PassthroughLight: Option<super::visual::SpriteAt> = None,
        Cursor: Option<super::visual::SpriteAt> = None,
        /// CursorMaxDistance
        CursorMaxDistance: f32 = 0.0,
        /// Awake's cursorStartPos, barStartPos and barSize.
        cursorStartPos: [f32; 3] = [0.0; 3],
        barStartPos: [f32; 3] = [0.0; 3],
        barSize: [f32; 3] = [0.0; 3],
    }
}

script! {
    Lagbox {
        /// DelayModifier (seconds, 0.04 to 60): the physical's ActivationPropagationDelay while enabled.
        DelayModifier: f32 = 1.0,
        /// isBusy: a DoLag is running.
        isBusy: bool = false,
        /// The private t: how far the knob has turned, 0 to 1.
        t: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour.
        physical: usize = 0,
        /// MuteBehaviour = GetComponent<MuteBehaviour>(): the component's id (None: null).
        mute: Option<i32> = None,
        /// Knob (Update and DoLag write it) is null.
        knob_null: bool = false,
        /// Knob: its Transform id (Update writes its localEulerAngles; ObjectState saves its pose).
        knob: Option<i32> = None,
    }
}

impl Game {
    /// GateBehaviour.SetThreshold: ThresholdPercentage = Clamp(v, 0, 100) / 100, then gate_tick.
    pub fn gate_set_threshold(&mut self, g: usize, v: f32) {
        self.gates[g].ThresholdPercentage = mathf::clamp(v, 0.0, 100.0) / 100.0;
        self.gate_tick(g);
    }
    /// GateBehaviour.Awake: ThresholdPercentage = Threshold / 100f; cursorStartPos, barStartPos and barSize are the local
    /// poses of Cursor and PowerLevelBar.
    pub fn gate_awake(&mut self, g: usize) {
        self.gates[g].ThresholdPercentage = self.gates[g].Threshold / 100.0;
        let cursor = self.gates[g].Cursor.unwrap_or_else(|| crate::game::menu::npe());
        self.gates[g].cursorStartPos = self.sprite_local_position(cursor);
        let bar = self.gates[g].PowerLevelBar.unwrap_or_else(|| crate::game::menu::npe());
        self.gates[g].barStartPos = self.sprite_local_position(bar);
        self.gates[g].barSize = self.sprite_local_scale(bar);
    }

    /// GateBehaviour.OnEnable: PowerLevelBar.enabled = true.
    pub fn gate_on_enable(&mut self, g: usize) {
        let bar = self.gates[g].PowerLevelBar.unwrap_or_else(|| crate::game::menu::npe());
        self.set_sprite_enabled(bar, true);
    }

    /// GateBehaviour.SnapToPixels: Mathf.Round (half to even) of v / (1 / 35), times (1 / 35).
    pub(crate) fn gate_snap_to_pixels(v: f32) -> f32 {
        let step = 1.0f32 / 35.0;
        (d((v / step).round_ties_even()) * step).f()
    }

    /// GateBehaviour.OnWillRenderObject for each gate whose sprite the camera draws: the cursor and the power bar.
    pub fn gates_will_render(&mut self) {
        for g in 0..self.gates.len() {
            let s = &self.gates[g];
            if !self.will_render_dispatch(s.id, s.enabled && !self.destroyed_components.contains(&s.id) && self.sprite_drawn(super::visual::SpriteAt::Physical(s.physical))) {
                continue;
            }
            let Some(cursor) = s.Cursor else {
                self.unity_callback(|_| crate::game::menu::npe());
                continue;
            };
            let bar = s.PowerLevelBar;
            let (max_distance, start, bar_start, size, max_power, threshold) = (s.CursorMaxDistance, s.cursorStartPos, s.barStartPos, s.barSize, s.MaxPower, s.ThresholdPercentage);
            let charge = self.physicals[s.physical].Charge();
            // Cursor.localPosition = cursorStartPos + Vector3.right * SnapToPixels(CursorMaxDistance * ThresholdPercentage)
            let moved = Self::gate_snap_to_pixels((d(max_distance) * threshold).f());
            self.set_sprite_local_position(cursor, [(d(start[0]) + moved).f(), start[1], start[2]]);
            let Some(bar) = bar else {
                self.unity_callback(|_| crate::game::menu::npe());
                continue;
            };
            // num = SnapToPixels(Clamp(Charge, 0, MaxPower) / MaxPower * barSize.x) / barSize.x
            let mut num = (d(mathf::clamp(charge, 0.0, max_power)) / max_power).f();
            num = (d(Self::gate_snap_to_pixels((d(num) * size[0]).f())) / size[0]).f();
            let x = (d(num) * size[0]).f();
            // localScale = new Vector3(x, barSize.y) (z 0); localPosition = barStartPos - new Vector3(barSize.x / 2 * (1 - num), 0)
            self.set_sprite_local_scale(bar, [x, size[1], 0.0]);
            let shift = (d(size[0] / 2.0) * (d(1.0) - num)).f();
            self.set_sprite_local_position(bar, [(d(bar_start[0]) - shift).f(), bar_start[1], bar_start[2]]);
        }
    }

    /// GateBehaviour.Update (PassthroughLight.enabled = the flag). On an edge it broadcasts IsolatedActivation on each channel in
    /// continuous use: only the use wires added to its object take it (link refuses a receiver in the prefab).
    pub fn gate_update(&mut self, g: usize) {
        let s = &self.gates[g];
        let p = s.physical;
        // Charge / MaxPower >= ThresholdPercentage, in double.
        let flag = self.physicals[p].Charge() as f64 / s.MaxPower as f64 >= s.ThresholdPercentage as f64;
        let light = s.PassthroughLight.unwrap_or_else(|| crate::game::menu::npe());
        self.set_sprite_enabled(light, flag);
        let s = &self.gates[g];
        if flag {
            self.physicals[p].ActivationPropagationDelay = s.ActivationDelay;
        }
        let rising = flag && !self.gates[g].Passthrough;
        let falling = !flag && self.gates[g].Passthrough;
        let double = self.gates[g].DoubleTrigger;
        if rising || (falling && double) {
            for channel in 0..3u16 {
                let used = self.used_continuously_on(p, channel as usize);
                if (rising && !falling && used) || (double && falling && !rising && used) {
                    // BroadcastMessage("IsolatedActivation", new ActivationPropagation(transform.root, channel)).
                    let root = self.root_id(p);
                    let a = self.signal_from(root, channel);
                    // Only UseWireBehaviour has an IsolatedActivation method, and link refuses one below the gate.
                    for w in self.use_wires_on(p) {
                        self.use_wire_receive(w, &a, false);
                    }
                }
            }
        }
        self.gates[g].Passthrough = flag;
        if !flag {
            self.physicals[p].ActivationPropagationDelay = -1.0;
        }
    }

    /// GateBehaviour.SetThreshold's `if (base.enabled) PhysicalBehaviour.PlayClipOnce(Tick)`.
    pub fn gate_tick(&mut self, g: usize) {
        if self.gates[g].enabled {
            let (id, p) = (self.gates[g].id, self.gates[g].physical);
            let clip = self.audio.clip(id, "Tick");
            self.play_clip_once(p, clip, 1.0);
        }
    }

    /// GateBehaviour.OnDisable: the physical's delay is ActivationDelay, and the bar and the light are disabled.
    pub fn gate_on_disable(&mut self, g: usize) {
        let p = self.gates[g].physical;
        self.physicals[p].ActivationPropagationDelay = self.gates[g].ActivationDelay;
        let bar = self.gates[g].PowerLevelBar.unwrap_or_else(|| crate::game::menu::npe());
        self.set_sprite_enabled(bar, false);
        let light = self.gates[g].PassthroughLight.unwrap_or_else(|| crate::game::menu::npe());
        self.set_sprite_enabled(light, false);
    }

    /// LagboxBehaviour.Start: the physical's delay is DelayModifier (the context button is UI); a busy box resumes DoLag.
    pub fn lagbox_start(&mut self, g: usize) {
        let p = self.lagboxes[g].physical;
        self.physicals[p].ActivationPropagationDelay = self.lagboxes[g].DelayModifier;
        self.physicals[p].menu.push(super::menu::MenuButton::new("setLagboxDelay", crate::il!(str "LagboxBehaviour::Start" 1), crate::il!(str "LagboxBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::Lagbox(g))));
        if self.lagboxes[g].isBusy {
            self.lagbox_start_lag(g);
        }
    }

    /// LagboxBehaviour.Update: Knob.localEulerAngles = (0, 0, Clamp(t * 180, 0, 180)).
    pub fn lagbox_update(&mut self, g: usize) {
        let Some(knob) = self.lagboxes[g].knob else { crate::game::menu::npe() };
        let q = super::menu::euler_z_rotation(mathf::clamp(self.lagboxes[g].t * 180.0, 0.0, 180.0));
        self.with_component_instance(knob, |_, inst, o| inst.set_local_rotation(o, q));
    }

    /// LagboxBehaviour.OnEnable: the physical's delay is DelayModifier.
    pub fn lagbox_on_enable(&mut self, g: usize) {
        let p = self.lagboxes[g].physical;
        self.physicals[p].ActivationPropagationDelay = self.lagboxes[g].DelayModifier;
    }

    /// LagboxBehaviour.OnDisable: StopAllCoroutines, and the physical's delay goes back to 0.1.
    pub fn lagbox_on_disable(&mut self, g: usize) {
        let id = self.lagboxes[g].id;
        self.stop_all_coroutines(id);
        let p = self.lagboxes[g].physical;
        self.physicals[p].ActivationPropagationDelay = 0.1;
    }

    /// LagboxBehaviour.Use: while enabled, t = 0 and another DoLag starts (a running one is not checked: both advance t).
    pub fn lagbox_use(&mut self, g: usize) {
        if self.lagboxes[g].enabled {
            self.lagboxes[g].t = 0.0;
            self.lagbox_start_lag(g);
        }
    }

    fn lagbox_start_lag(&mut self, g: usize) {
        let id = self.lagboxes[g].id;
        self.start_coroutine(id, id, Self::lagbox_lag_routine(g));
    }

    /// LagboxBehaviour.DoLag: isBusy; while t < 1, t += deltaTime / the physical's delay and wait for the end of the
    /// frame; then isBusy is false (the knob is presentation) and the ding plays unless muted.
    pub(crate) fn lagbox_lag_routine(g: usize) -> Routine {
        Box::new(move |game| {
            let p = game.lagboxes[g].physical;
            game.lagboxes[g].isBusy = true;
            if game.lagboxes[g].knob_null {
                crate::game::menu::npe();
            }
            if game.lagboxes[g].t < 1.0 {
                game.lagboxes[g].t += game.time.deltaTime / game.physicals[p].ActivationPropagationDelay;
                return Yield::WaitForEndOfFrame;
            }
            game.lagboxes[g].isBusy = false;
            // if (!MuteBehaviour.Muted) PhysicalBehaviour.PlayClipOnce(Ding)
            if !game.muted(game.lagboxes[g].mute) {
                let clip = game.audio.clip(game.lagboxes[g].id, "Ding");
                game.play_clip_once(p, clip, 1.0);
            }
            Yield::Done
        })
    }

    /// TimedGateBehaviour.Awake (after LagboxBehaviour.Awake's MuteBehaviour lookup): PhysicalBehaviour.ActivationPropagationDelay = 0.
    pub fn timed_gate_awake(&mut self, g: usize) {
        let p = self.timed_gates[g].physical;
        self.physicals[p].ActivationPropagationDelay = 0.0;
    }

    /// TimedGateBehaviour.Start: the green light on and the red one off; a gate that was busy resumes DoLag.
    pub fn timed_gate_start(&mut self, g: usize) {
        self.timed_gate_lights(g, true);
        let p = self.timed_gates[g].physical;
        self.physicals[p].menu.push(super::menu::MenuButton::new("setTimedGateDuration", crate::il!(str "TimedGateBehaviour::Start" 1), crate::il!(str "TimedGateBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::TimedGate(g))));
        if self.timed_gates[g].isBusy {
            self.timed_gate_start_lag(g);
        }
    }

    /// TimedGateBehaviour.OnEnable: a busy gate resumes DoLag.
    pub fn timed_gate_on_enable(&mut self, g: usize) {
        if self.timed_gates[g].isBusy {
            self.timed_gate_start_lag(g);
        }
    }

    /// TimedGateBehaviour.OnDisable: StopAllCoroutines (the physical's delay is left as it is).
    pub fn timed_gate_on_disable(&mut self, g: usize) {
        let id = self.timed_gates[g].id;
        self.stop_all_coroutines(id);
    }

    /// TimedGateBehaviour.Use: while enabled and not busy, t = 0, the redlight sound unless muted, and DoLag starts.
    pub fn timed_gate_use(&mut self, g: usize) {
        if self.timed_gates[g].enabled && !self.timed_gates[g].isBusy {
            self.timed_gates[g].t = 0.0;
            if !self.muted(self.timed_gates[g].mute) {
                let (id, p) = (self.timed_gates[g].id, self.timed_gates[g].physical);
                let clip = self.audio.clip(id, "RedlightSound");
                self.play_clip_once(p, clip, 1.0);
            }
            self.timed_gate_start_lag(g);
        }
    }

    /// GreenLight.enabled = green; RedLight.enabled = !green.
    fn timed_gate_lights(&mut self, g: usize, green: bool) {
        let s = &self.timed_gates[g];
        let (gl, rl) = (s.GreenLight.unwrap_or_else(|| crate::game::menu::npe()), s.RedLight.unwrap_or_else(|| crate::game::menu::npe()));
        self.set_sprite_enabled(gl, green);
        self.set_sprite_enabled(rl, !green);
    }

    /// Knob.eulerAngles = (0, 0, z) (a null Knob threw at DoLag's start).
    fn timed_gate_knob(&mut self, g: usize, z: f32) {
        let Some(knob) = self.timed_gates[g].knob else { return };
        let q = super::menu::euler_z_rotation(z);
        self.with_component_instance(knob, |_, inst, o| inst.set_world_rotation(o, q));
    }

    fn timed_gate_start_lag(&mut self, g: usize) {
        let id = self.timed_gates[g].id;
        self.start_coroutine(id, id, Self::timed_gate_lag_routine(g));
    }

    /// TimedGateBehaviour.DoLag. The first step yields a frame only when the delay is not yet -1; then the delay is -1
    /// while t runs from its value to 1 (each step adds dt / DelayModifier and waits for the end of the frame), and the
    /// gate ends with the delay back at 0 and isBusy false.
    pub(crate) fn timed_gate_lag_routine(g: usize) -> Routine {
        #[derive(Clone, Copy)]
        enum Stage {
            Begin,
            AfterFirstWait,
            Running,
        }
        let mut stage = Stage::Begin;
        Box::new(move |game| {
            let p = game.timed_gates[g].physical;
            match stage {
                Stage::Begin => {
                    if game.timed_gates[g].knob_null {
                        crate::game::menu::npe();
                    }
                    game.timed_gate_knob(g, 0.0);
                    game.timed_gate_lights(g, false);
                    game.timed_gates[g].isBusy = true;
                    if game.physicals[p].ActivationPropagationDelay >= 0.0 {
                        stage = Stage::AfterFirstWait;
                        return Yield::WaitForEndOfFrame;
                    }
                    game.physicals[p].ActivationPropagationDelay = -1.0;
                    stage = Stage::Running;
                }
                Stage::AfterFirstWait => {
                    game.physicals[p].ActivationPropagationDelay = -1.0;
                    stage = Stage::Running;
                }
                Stage::Running => {}
            }
            let s = &mut game.timed_gates[g];
            if s.t < 1.0 {
                s.t += game.time.deltaTime / s.DelayModifier;
                let z = s.t * 180.0;
                game.timed_gate_knob(g, z);
                return Yield::WaitForEndOfFrame;
            }
            // The redlight is off and the greenlight on, the delay is restored, the gate is no longer busy, and the ding plays unless muted.
            game.timed_gate_lights(g, true);
            game.physicals[p].ActivationPropagationDelay = 0.0;
            game.timed_gates[g].isBusy = false;
            game.timed_gate_knob(g, 180.0);
            if !game.muted(game.timed_gates[g].mute) {
                let clip = game.audio.clip(game.timed_gates[g].id, "Ding");
                game.play_clip_once(p, clip, 1.0);
            }
            Yield::Done
        })
    }
}

script! {
    /// ActivationTransformerBehaviour: past 4.8 charge it sends IsolatedActivation on each channel (on the rise) and
    /// IsolatedContinuousActivation (each continuous step) to its own object, where the use wires added to it take them.
    ActivationTransformer {
        shouldSend: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// ActivationTransformerBehaviour.PhysicalBehaviour
        physical: usize = 0,
        /// base.transform (Utils.SendAllChannelIsolatedActivation(this) goes to its GameObject)
        own: super::script_object::ObjectXf = Default::default(),
    }
}

impl Game {
    /// ActivationTransformerBehaviour.Update
    pub fn activation_transformer_update(&mut self, i: usize) {
        let flag = self.activation_transformers[i].shouldSend;
        let p = self.activation_transformers[i].physical;
        let send = self.physicals[p].Charge() > 4.8;
        self.activation_transformers[i].shouldSend = send;
        if send && !flag {
            self.send_isolated_xf(&self.activation_transformers[i].own.clone(), false);
        }
    }

    /// ActivationTransformerBehaviour.OnContinuousUpdate
    pub(crate) fn activation_transformer_continuous_update(&mut self, i: usize) {
        if self.activation_transformers[i].shouldSend {
            self.send_isolated_xf(&self.activation_transformers[i].own.clone(), true);
        }
    }
}
