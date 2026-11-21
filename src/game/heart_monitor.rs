//! HeartMonitorBehaviour: a Use toggles Activated. While activated and connected (IsConnected) it measures the connected
//! limb's heartbeat each 1/24 s, and draws Random for each history sample. BloodWireBehaviour sets IsConnected and
//! ConnectedLimb (blood_wire.rs). Its LineRenderer (`Game::lines`) shows the history: positionCount in Start, enabled by
//! UpdateActivation and Update, the y of each vertex in SetLineVertices.
#![allow(non_snake_case)]

use super::{mathf, script, Game};

script! {
    HeartMonitor {
        /// Activated: toggled by Use.
        Activated: bool = false,
        /// IsConnected: set by a BloodWireBehaviour (blood_wire.rs).
        IsConnected: bool = false,
        /// LineBounds: the line's x range, from 0 to 0.8 in the prefab.
        LineBounds: [f32; 2] = [0.0, 0.8],
        Amplitude: f32 = 0.2,
    }
    links {
        /// NoWireAttachedText
        NoWireAttachedText: String = crate::il!(str "HeartMonitorBehaviour::.ctor" 0).to_string(),
        /// The component's instance id.
        id: i32 = 0,
        /// HeartRateText (world_text.rs).
        HeartRateText: Option<usize> = None,
        /// The PhysicalBehaviour on its GameObject (a BloodWireBehaviour finds the monitor through it).
        physical: usize = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// ConnectedLimb: the limb whose heartbeat is measured (set by the wire; a null one is an NRE in Measure).
        ConnectedLimb: Option<usize> = None,
        /// The private t and t2: the measurement timer and the beat timer.
        t: f32 = 0.0,
        t2: f32 = 0.0,
        /// The history of 20 samples.
        history: super::FloatArray<20> = super::FloatArray([0.0; 20]),
        /// The 20 line vertices.
        vertices: [[f32; 3]; 20] = [[0.0; 3]; 20],
        /// LineRenderer: its `Game::lines` entry.
        line: usize = 0,
    }
}

impl Game {
    /// HeartMonitorBehaviour.Awake: the 20 line vertices spread from (LineBounds.x, 0) to (LineBounds.y, 0), and the
    /// history cleared.
    pub fn heart_monitor_awake(&mut self, h: usize) {
        let [from, to] = self.heart_monitors[h].LineBounds;
        for i in 0..20 {
            let num = i as f32 / 19.0;
            // Vector3.Lerp(a, b, t) with t clamped to 0..1.
            self.heart_monitors[h].vertices[i] = [mathf::lerp(from, to, num), 0.0, 0.0];
            self.heart_monitors[h].history[i] = 0.0;
        }
    }

    /// BeepSource (a null one throws).
    fn heart_monitor_source(&self, h: usize) -> i32 {
        self.audio.source_field(self.heart_monitors[h].id, "BeepSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// HeartMonitorBehaviour.Use: `if (base.enabled)` Activated flips, then UpdateActivation.
    pub fn heart_monitor_use(&mut self, h: usize) {
        if self.heart_monitors[h].enabled {
            self.heart_monitors[h].Activated = !self.heart_monitors[h].Activated;
            self.heart_monitor_update_activation(h);
        }
    }

    /// HeartMonitorBehaviour.Start: LineRenderer.positionCount = 20, then UpdateActivation.
    pub fn heart_monitor_start(&mut self, h: usize) {
        self.line_set_position_count(self.heart_monitors[h].line, 20);
        self.heart_monitor_update_activation(h);
    }

    /// HeartMonitorBehaviour.UpdateActivation, HeartMonitorBehaviour.OnEnable: HeartRateText.enabled = Activated; when not,
    /// BeepSource.Stop and LineRenderer.enabled = false.
    pub fn heart_monitor_update_activation(&mut self, h: usize) {
        let (text, on) = (self.heart_monitors[h].HeartRateText, self.heart_monitors[h].Activated);
        if !on {
            let src = self.heart_monitor_source(h);
            self.audio.stop(src);
            self.lines[self.heart_monitors[h].line].enabled = false;
        }
        self.text_enabled_set(Self::text_ref(text), on);
    }

    /// HeartMonitorBehaviour.Update: nothing while not activated. Connected, the timers run and Measure runs; unconnected
    /// it only shows the text.
    pub fn heart_monitor_update(&mut self, h: usize) {
        if !self.heart_monitors[h].Activated {
            return;
        }
        let connected = self.heart_monitors[h].IsConnected;
        self.lines[self.heart_monitors[h].line].enabled = connected;
        if !connected {
            // the text shows NoWireAttachedText; BeepSource.Stop() if it is playing
            let (text, value) = (self.heart_monitors[h].HeartRateText, self.heart_monitors[h].NoWireAttachedText.clone());
            self.text_write(text, &value);
            let src = self.heart_monitor_source(h);
            if self.audio.is_playing(src) {
                self.audio.stop(src);
            }
        } else {
            let dt = self.time.deltaTime;
            self.heart_monitors[h].t += dt;
            self.heart_monitors[h].t2 += dt;
            self.heart_monitor_measure(h);
        }
    }

    /// HeartMonitorBehaviour.Measure: every 1/24 s, the BPM eases toward the connected limb's heartbeat (0 unless it is
    /// connected to the root), and each beat (t2 past 60 / BPM) samples the blood flow, other frames sample noise.
    /// currentBPM is never assigned, so its lerp starts from 0.
    pub(crate) fn heart_monitor_measure(&mut self, h: usize) {
        if self.heart_monitors[h].t < 1.0 / 24.0 {
            return;
        }
        self.heart_monitors[h].t = 0.0;
        let limb = self.heart_monitors[h].ConnectedLimb.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let person = self.limbs[limb].Person;
        let target = if self.nodes[limb].IsConnectedToRoot { self.people[person].Heartbeat } else { 0.0 };
        let dt = self.time.deltaTime;
        // Mathf.Exp is (float)Math.Exp in double.
        let factor = 3.0f32 - ((-0.01f32 * dt) as f64).exp() as f32;
        let num = mathf::lerp(0.0, target, factor);
        let num2 = 60.0f32 / num;
        let text = format!("{} BPM", super::temperature::round_to_int(num));
        let field = self.heart_monitors[h].HeartRateText;
        self.text_write(field, &text);
        let src = self.heart_monitor_source(h);
        let flag = super::temperature::round_to_int(num) <= 0;
        if flag && !self.audio.is_playing(src) {
            self.audio.play(src);
        }
        if self.heart_monitors[h].t2 > num2 {
            self.heart_monitors[h].t2 = 0.0;
            if !self.audio.is_playing(src) && !flag {
                self.audio.play(src);
            }
            let flow = mathf::clamp01(self.circulations[limb].BloodFlow);
            let sample = self.random.range(0.8, 1.0) * flow;
            self.heart_monitor_add_to_history(h, sample);
        } else {
            if self.audio.is_playing(src) && !flag {
                self.audio.stop(src);
            }
            let sample = self.random.value() * 0.02;
            self.heart_monitor_add_to_history(h, sample);
        }
    }

    /// HeartMonitorBehaviour.AddToHistory: shift the samples down and append, then SetLineVertices.
    pub(crate) fn heart_monitor_add_to_history(&mut self, h: usize, value: f32) {
        let hist = &mut self.heart_monitors[h].history;
        hist.copy_within(1..20, 0);
        hist[19] = value;
        self.heart_monitor_set_line_vertices(h);
    }

    /// HeartMonitorBehaviour.SetLineVertices: vertices[i].y = Clamp(history[i], -1, 1) * Amplitude; LineRenderer.SetPositions.
    pub(crate) fn heart_monitor_set_line_vertices(&mut self, h: usize) {
        let s = &mut self.heart_monitors[h];
        for i in 0..20 {
            s.vertices[i][1] = mathf::clamp(s.history[i], -1.0, 1.0) * s.Amplitude;
        }
        let (line, vertices) = (s.line, s.vertices);
        self.line_set_positions(line, &vertices);
    }

    /// HeartMonitorBehaviour.OnDisable: Activated = false, then UpdateActivation.
    pub fn heart_monitor_on_disable(&mut self, h: usize) {
        self.heart_monitors[h].Activated = false;
        self.heart_monitor_update_activation(h);
    }
}
