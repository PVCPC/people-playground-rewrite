//! FMOD's SFX reverb (FMOD_DSP_TYPE_SFXREVERB), the DSP Unity's AudioReverbFilter creates (AudioFilter::Init,
//! AudioReverbFilter::AddToManager 180a96b90) and the listener puts at the head of the FX_IgnoreVolume channel group
//! (AudioListener::ApplyFilters 180ab6280): FMOD::DSPSfxReverb (createInternal 1810fcee0, setParameterInternal 1810fda90,
//! updateInternal 1810fdda0, readInternal 1810fd8e0, the setters 1810fc550..1810fce20) over ASfxDsp (init 18114e590,
//! DoDSPProcessing 18114cdc0, BlockProcessInput 18114bfe0, the delay setters and allocators 18114bc10..18114e430).

/// FMOD_DSP_PARAMETERDESC min, max, default of the 14 parameters (FMOD::dspsfxreverb_param 1819a4da0): Dry Level, Room,
/// Room HF, Decay Time, Decay HF Ratio, Reflections, Reflect Delay, Reverb, Reverb Delay, Diffusion, Density, HF
/// Reference, Room LF, LF Reference.
const PARAMS: [[f32; 3]; 14] = [
    [-10000.0, 0.0, 0.0],
    [-10000.0, 0.0, -10000.0],
    [-10000.0, 0.0, 0.0],
    [0.1, 20.0, 1.0],
    [0.1, 2.0, 0.5],
    [-10000.0, 1000.0, -10000.0],
    [0.0, 0.3, 0.02],
    [-10000.0, 2000.0, 0.0],
    [0.0, 0.1, 0.04],
    [0.0, 100.0, 100.0],
    [0.0, 100.0, 100.0],
    [20.0, 20000.0, 5000.0],
    [-10000.0, 0.0, 0.0],
    [20.0, 1000.0, 250.0],
];

const LATE: usize = 8;

/// Unity's ReverbPresets (FMOD_REVERB_PROPERTIES[27] at 181991ff0, read from this build), the parts
/// AudioReverbFilter::ChangeProperties copies: Room, RoomHF, DecayTime, DecayHFRatio, Reflections, Reverb, ReverbDelay,
/// Diffusion, Density, HFReference, RoomLF, LFReference, for AudioReverbPreset Off .. Psychotic.
const PRESETS: [[f32; 12]; 27] = [
    [-10000.0, -10000.0, 1.0, 1.0, -2602.0, 200.0, 0.011, 0.0, 0.0, 5000.0, 0.0, 250.0], // Off
    [-1000.0, -100.0, 1.49, 0.83, -2602.0, 200.0, 0.011, 100.0, 100.0, 5000.0, 0.0, 250.0], // Generic
    [-1000.0, -6000.0, 0.17, 0.1, -1204.0, 207.0, 0.002, 100.0, 100.0, 5000.0, 0.0, 250.0], // PaddedCell
    [-1000.0, -454.0, 0.4, 0.83, -1646.0, 53.0, 0.003, 100.0, 100.0, 5000.0, 0.0, 250.0], // Room
    [-1000.0, -1200.0, 1.49, 0.54, -370.0, 1030.0, 0.011, 100.0, 60.0, 5000.0, 0.0, 250.0], // Bathroom
    [-1000.0, -6000.0, 0.5, 0.1, -1376.0, -1104.0, 0.004, 100.0, 100.0, 5000.0, 0.0, 250.0], // Livingroom
    [-1000.0, -300.0, 2.31, 0.64, -711.0, 83.0, 0.017, 100.0, 100.0, 5000.0, 0.0, 250.0], // Stoneroom
    [-1000.0, -476.0, 4.32, 0.59, -789.0, -289.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Auditorium
    [-1000.0, -500.0, 3.92, 0.7, -1230.0, -2.0, 0.029, 100.0, 100.0, 5000.0, 0.0, 250.0], // Concerthall
    [-1000.0, 0.0, 2.91, 1.3, -602.0, -302.0, 0.022, 100.0, 100.0, 5000.0, 0.0, 250.0], // Cave
    [-1000.0, -698.0, 7.24, 0.33, -1166.0, 16.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Arena
    [-1000.0, -1000.0, 10.05, 0.23, -602.0, 198.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Hangar
    [-1000.0, -4000.0, 0.3, 0.1, -1831.0, -1630.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // CarpetedHallway
    [-1000.0, -300.0, 1.49, 0.59, -1219.0, 441.0, 0.011, 100.0, 100.0, 5000.0, 0.0, 250.0], // Hallway
    [-1000.0, -237.0, 2.7, 0.79, -1214.0, 395.0, 0.02, 100.0, 100.0, 5000.0, 0.0, 250.0], // StoneCorridor
    [-1000.0, -270.0, 1.49, 0.86, -1204.0, -4.0, 0.011, 100.0, 100.0, 5000.0, 0.0, 250.0], // Alley
    [-1000.0, -3300.0, 1.49, 0.54, -2560.0, -229.0, 0.088, 79.0, 100.0, 5000.0, 0.0, 250.0], // Forest
    [-1000.0, -800.0, 1.49, 0.67, -2273.0, -1691.0, 0.011, 50.0, 100.0, 5000.0, 0.0, 250.0], // City
    [-1000.0, -2500.0, 1.49, 0.21, -2780.0, -1434.0, 0.1, 27.0, 100.0, 5000.0, 0.0, 250.0], // Mountains
    [-1000.0, -1000.0, 1.49, 0.83, -10000.0, 500.0, 0.025, 100.0, 100.0, 5000.0, 0.0, 250.0], // Quarry
    [-1000.0, -2000.0, 1.49, 0.5, -2466.0, -1926.0, 0.1, 21.0, 100.0, 5000.0, 0.0, 250.0], // Plain
    [-1000.0, 0.0, 1.65, 1.5, -1363.0, -1153.0, 0.012, 100.0, 100.0, 5000.0, 0.0, 250.0], // ParkingLot
    [-1000.0, -1000.0, 2.81, 0.14, 429.0, 1023.0, 0.021, 80.0, 60.0, 5000.0, 0.0, 250.0], // SewerPipe
    [-1000.0, -4000.0, 1.49, 0.1, -449.0, 1700.0, 0.011, 100.0, 100.0, 5000.0, 0.0, 250.0], // Underwater
    [-1000.0, 0.0, 8.39, 1.39, -115.0, 985.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Drugged
    [-1000.0, -400.0, 17.23, 0.56, -1713.0, -613.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Dizzy
    [-1000.0, -151.0, 7.56, 0.91, -626.0, 774.0, 0.03, 100.0, 100.0, 5000.0, 0.0, 250.0], // Psychotic
];

/// AudioReverbFilter::ChangeProperties (180a96cd0): a preset below User (27) overwrites every parameter but the dry
/// level and the reflections delay. `p` is in `SfxReverb::set_parameter`'s order.
pub fn change_properties(p: &mut [f32; 14], preset: i32) {
    if preset < 27 {
        let v = &PRESETS[preset as usize];
        for (k, &i) in [1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13].iter().enumerate() {
            p[i] = v[k];
        }
    }
}

/// I3DL2_LISTENERPROPERTIES and SFX_REVERB_LFPROPS.
#[derive(Clone, Copy, PartialEq)]
struct Props {
    room: i32,
    room_hf: i32,
    decay_time: f32,
    decay_hf_ratio: f32,
    reflections: i32,
    reflections_delay: f32,
    reverb: i32,
    reverb_delay: f32,
    diffusion: f32,
    density: f32,
    hf_reference: f32,
    room_lf: i32,
    lf_reference: f32,
}

/// A value crossfaded over a block when it changed (ASfxDsp's x, mLatchX, mPrevX, mOldX, mXChanged).
#[derive(Clone, Copy, Default)]
struct Ramp {
    v: f32,
    latch: f32,
    prev: f32,
    old: f32,
    changed: bool,
}

impl Ramp {
    fn new(v: f32) -> Self {
        Self { v, latch: v, prev: v, old: v, changed: false }
    }
    /// The block start (DoDSPProcessing).
    fn latch(&mut self) {
        self.latch = self.v;
        if self.v == self.old {
            self.changed = false;
        } else {
            self.prev = self.old;
            self.old = self.v;
            self.changed = true;
        }
    }
    /// The value at frame `n` of the block (`a` = len - n, `b` = n, `inv` = 1 / len).
    fn at(&self, a: f32, b: f32, inv: f32) -> f32 {
        if self.changed { (a * self.prev + b * self.latch) * inv } else { self.latch }
    }
}

fn wrap(p: i32, mask: i32) -> i32 {
    p.wrapping_sub(1) & mask
}

/// ASfxDsp's buffer sizes: the power of two over `n + 1` samples as `1 << (1 - (int)(logf(n + 1) * -1.442695))`.
fn alloc(n: f32) -> Vec<f32> {
    let n = (n as i32 + 1) as f32;
    let shift = (1 - (n.ln() * -1.442695f32) as i32) & 31;
    vec![0.0; 1usize << shift]
}

/// DSPSfxReverb with its ASfxDsp.
pub struct SfxReverb {
    rate: i32,
    dry_lin: f32,
    props: Props,
    update: Props,
    in_buff: Vec<f32>,
    room_hf: Ramp,
    z_room_hf: [f32; 2],
    /// a0, a1, a2, b1, b2
    room_lf_coeffs: [f32; 5],
    room_lf: f32,
    z_room_lf: [f32; 2],
    decay_hf: [f32; LATE],
    decay_hf_changed: [bool; LATE],
    latch_decay_hf: [f32; LATE],
    latch_one_minus_decay_hf: [f32; LATE],
    prev_decay_hf: [f32; LATE],
    old_decay_hf: [f32; LATE],
    z_decay_hf: [f32; LATE],
    matrix: [f32; 24],
    feedback: [f32; LATE],
    latch_feedback: [f32; LATE],
    old_feedback: [f32; LATE],
    prev_feedback: [f32; LATE],
    hadamard: Ramp,
    er_gain: Ramp,
    lr_gain: Ramp,
    diffusion_scale: Ramp,
    allpass_gain: f32,
    late: [Vec<f32>; LATE],
    late_sec: [f32; LATE],
    late_len: [i32; LATE],
    old_late_len: [i32; LATE],
    late_len_b: [i32; LATE],
    late_changed: [bool; LATE],
    late_mask: [i32; LATE],
    late_wp: [i32; LATE],
    late_rp: [i32; LATE],
    old_late_rp: [i32; LATE],
    late_rp_b: [i32; LATE],
    el: Vec<f32>,
    el_sec: [f32; 9],
    el_samples: [i32; 9],
    old_el_samples: [i32; 9],
    el_changed: [bool; 9],
    el_mask: i32,
    el_wp: i32,
    el_rp: [i32; 9],
    old_el_rp: [i32; 9],
    early: Vec<f32>,
    early_sec: [f32; 7],
    early_mask: i32,
    early_wp: i32,
    early_rp: [i32; 7],
    allpass: [Vec<f32>; 2],
    allpass_mask: [i32; 2],
    allpass_wp: [i32; 2],
    allpass_rp: [i32; 2],
    matrix_stages: i32,
}

impl SfxReverb {
    /// DSPSfxReverb::createInternal at the software mixing rate: the defaults, ASfxDsp::init, each parameter's default
    /// set, then every setter.
    pub fn new(rate: i32) -> Self {
        let p = Props { room: -10000, room_hf: -10000, decay_time: 1.0, decay_hf_ratio: 1.0, reflections: -2602, reflections_delay: 0.007, reverb: 200, reverb_delay: 0.011, diffusion: 0.0, density: 0.0, hf_reference: 5000.0, room_lf: 0, lf_reference: 250.0 };
        let mut r = Self {
            rate,
            dry_lin: 0.0,
            props: p,
            update: p,
            in_buff: Vec::new(),
            room_hf: Ramp::new(0.98),
            z_room_hf: [0.0; 2],
            room_lf_coeffs: [1.0, 0.0, 0.0, 0.0, 0.0],
            room_lf: 0.0,
            z_room_lf: [0.0; 2],
            decay_hf: [0.4; LATE],
            decay_hf_changed: [false; LATE],
            latch_decay_hf: [0.4; LATE],
            latch_one_minus_decay_hf: [0.6; LATE],
            prev_decay_hf: [0.4; LATE],
            old_decay_hf: [0.4; LATE],
            z_decay_hf: [0.0; LATE],
            matrix: [0.0; 24],
            feedback: [0.32; LATE],
            latch_feedback: [0.32; LATE],
            old_feedback: [0.32; LATE],
            prev_feedback: [0.32; LATE],
            hadamard: Ramp::new(1.0),
            er_gain: Ramp::new(0.0),
            lr_gain: Ramp::new(0.0),
            diffusion_scale: Ramp { v: 0.25, latch: 1.0, prev: 1.0, old: 1.0, changed: false },
            allpass_gain: 0.63,
            late: Default::default(),
            late_sec: [0.0; LATE],
            late_len: [0; LATE],
            old_late_len: [0; LATE],
            late_len_b: [0; LATE],
            late_changed: [false; LATE],
            late_mask: [0; LATE],
            late_wp: [0; LATE],
            late_rp: [0; LATE],
            old_late_rp: [0; LATE],
            late_rp_b: [0; LATE],
            el: Vec::new(),
            el_sec: [0.0; 9],
            el_samples: [0; 9],
            old_el_samples: [0; 9],
            el_changed: [false; 9],
            el_mask: 0,
            el_wp: 0,
            el_rp: [0; 9],
            old_el_rp: [0; 9],
            early: Vec::new(),
            early_sec: [0.0; 7],
            early_mask: 0,
            early_wp: 0,
            early_rp: [0; 7],
            allpass: Default::default(),
            allpass_mask: [0; 2],
            allpass_wp: [0; 2],
            allpass_rp: [0; 2],
            matrix_stages: 3,
        };
        // ASfxDsp::init
        let rf = rate as f32;
        r.el_sec[0] = 0.04;
        r.el_samples[0] = (rf * 0.04) as i32;
        r.set_late_early_late_taps(0.06, 0.0187, 1.29);
        r.old_el_samples = r.el_samples;
        r.set_early_delay(0.005, 1.59);
        let ap = [0.0059f32, 0.0077];
        for i in 0..2 {
            r.allpass_rp[i] = (rf * ap[i]) as i32 - 1;
        }
        r.set_late_delays(0.061, 1.32, 0.0015, 1.47);
        r.old_late_len = r.late_len;
        for i in 0..LATE {
            r.late[i] = alloc(rf * r.late_sec[i]);
            r.late_mask[i] = r.late[i].len() as i32 - 1;
        }
        r.el = alloc(((r.el_sec[8] + 0.4) - r.el_sec[1]) * rf);
        r.el_mask = r.el.len() as i32 - 1;
        for i in 0..2 {
            r.allpass[i] = alloc(rf * ap[i]);
            r.allpass_mask[i] = r.allpass[i].len() as i32 - 1;
        }
        r.early = alloc(r.early_sec[6] * rf);
        r.early_mask = r.early.len() as i32 - 1;
        // createInternal: each parameter's default, then the props from them and every setter.
        for (i, d) in PARAMS.iter().enumerate() {
            r.set_parameter(i, d[2]);
        }
        r.props = r.update;
        let p = &mut r.props;
        p.room = p.room.clamp(-10000, 0);
        r.set_reflections_level();
        r.set_reverb_level();
        r.set_room_hf();
        r.set_decay_time();
        r.props.decay_hf_ratio = clamp(r.props.decay_hf_ratio, 0.1, 2.0);
        r.set_decay_time();
        r.set_reflections_level();
        r.set_reflections_delay();
        r.set_reverb_level();
        r.set_reverb_delay();
        r.set_diffusion();
        r.props.density = clamp(r.props.density, 0.0, 100.0);
        r.set_delay_line_lengths();
        r.props.hf_reference = clamp(r.props.hf_reference, 20.0, 20000.0);
        r.set_room_hf();
        r.set_decay_time();
        r.set_room_lf();
        r.props.lf_reference = clamp(r.props.lf_reference, 20.0, 1000.0);
        r.set_room_lf();
        r
    }

    /// DSPI::setParameter (1810bed10: clamped to the parameter's range) and DSPSfxReverb::setParameterInternal: the dry
    /// level at once, the rest into the update props `update` applies before the next block.
    pub fn set_parameter(&mut self, i: usize, v: f32) {
        let [min, max, _] = PARAMS[i];
        let v = if min <= v { v } else { min };
        let v = if max <= v { max } else { v };
        let round = |v: f32| if 0.0 <= v { (v + 0.5) as i32 } else { (v - 0.5) as i32 };
        let u = &mut self.update;
        match i {
            0 => self.dry_lin = 10f64.powf((v * 0.0005) as f64) as f32,
            1 => u.room = round(v),
            2 => u.room_hf = round(v),
            3 => u.decay_time = v,
            4 => u.decay_hf_ratio = v,
            5 => u.reflections = round(v),
            6 => u.reflections_delay = v,
            7 => u.reverb = round(v),
            8 => u.reverb_delay = v,
            9 => u.diffusion = v,
            10 => u.density = v,
            11 => u.hf_reference = v,
            12 => u.room_lf = round(v),
            _ => u.lf_reference = v,
        }
    }

    /// DSPSfxReverb::updateInternal: every changed property's setters.
    pub fn update(&mut self) {
        let u = self.update;
        if self.props.room != u.room {
            self.props.room = u.room.clamp(-10000, 0);
            self.set_reflections_level();
            self.set_reverb_level();
        }
        if self.props.room_hf != u.room_hf {
            self.props.room_hf = u.room_hf;
            self.set_room_hf();
        }
        if u.decay_time != self.props.decay_time {
            self.props.decay_time = u.decay_time;
            self.set_decay_time();
        }
        if u.decay_hf_ratio != self.props.decay_hf_ratio {
            self.props.decay_hf_ratio = clamp(u.decay_hf_ratio, 0.1, 2.0);
            self.set_decay_time();
        }
        if self.props.reflections != u.reflections {
            self.props.reflections = u.reflections;
            self.set_reflections_level();
        }
        if u.reflections_delay != self.props.reflections_delay {
            self.props.reflections_delay = u.reflections_delay;
            self.set_reflections_delay();
        }
        if self.props.reverb != u.reverb {
            self.props.reverb = u.reverb;
            self.set_reverb_level();
        }
        if u.reverb_delay != self.props.reverb_delay {
            self.props.reverb_delay = u.reverb_delay;
            self.set_reverb_delay();
        }
        if u.diffusion != self.props.diffusion {
            self.props.diffusion = u.diffusion;
            self.set_diffusion();
        }
        if u.density != self.props.density {
            self.props.density = clamp(u.density, 0.0, 100.0);
            self.set_delay_line_lengths();
        }
        if u.hf_reference != self.props.hf_reference {
            self.props.hf_reference = clamp(u.hf_reference, 20.0, 20000.0);
            self.set_room_hf();
            self.set_decay_time();
        }
        if self.props.room_lf != u.room_lf {
            self.props.room_lf = u.room_lf;
            self.set_room_lf();
        }
        if u.lf_reference != self.props.lf_reference {
            self.props.lf_reference = clamp(u.lf_reference, 20.0, 1000.0);
            self.set_room_lf();
        }
    }

    // ---- DSPSfxReverb's setters --------------------------------------------------------------------------------

    fn set_reflections_level(&mut self) {
        let p = &mut self.props;
        p.reflections = p.reflections.clamp(-10000, 1000);
        let x = (p.room + p.reflections) as f32 * 0.01 * 0.05;
        self.er_gain.v = pow10(x) * 0.35355338;
    }

    fn set_reverb_level(&mut self) {
        let p = &mut self.props;
        p.reverb = p.reverb.clamp(-10000, 2000);
        let mut g = pow10((p.room + p.reverb) as f32 * 0.01 * 0.05);
        let mut s = 0.0f32;
        for f in self.feedback {
            s += f * f;
        }
        s /= LATE as f32;
        if s == 1.0 {
            g /= 1.0f32.sqrt();
        } else {
            s = s / (1.0 - s) + 1.0;
            if s > 0.0 {
                g /= s.sqrt();
            }
        }
        self.lr_gain.v = g * 0.35355338;
    }

    fn set_room_hf(&mut self) {
        let p = &mut self.props;
        p.room_hf = p.room_hf.clamp(-10000, 0);
        let g = pow10(p.room_hf as f32 * 0.005 * 0.05);
        self.room_hf.v = 1.0 - lowpass_coeff(g, p.hf_reference, self.rate as f32);
    }

    fn set_room_lf(&mut self) {
        let p = &mut self.props;
        p.room_lf = p.room_lf.clamp(-10000, 0);
        self.room_lf = p.room_lf as f32 * 0.01;
        self.room_lf_coeffs = shelf_coeffs(self.room_lf, p.lf_reference, self.rate as f32);
    }

    fn set_decay_time(&mut self) {
        let p = &mut self.props;
        p.decay_time = if 0.1 <= p.decay_time { if 10000.0 < p.decay_time { 10000.0 } else { p.decay_time } } else { 0.1 };
        for i in 0..LATE {
            if p.decay_time != 0.0 {
                let l = self.late_sec[i];
                let a = (-60.0 / p.decay_time) * l;
                self.feedback[i] = pow10(a * 0.05);
                let b = (-60.0 / (p.decay_hf_ratio * p.decay_time)) * l - a;
                self.decay_hf[i] = 1.0 - lowpass_coeff(pow10(b * 0.05), p.hf_reference, self.rate as f32);
            }
        }
        self.set_reverb_level();
    }

    fn set_delay_line_lengths(&mut self) {
        let d = self.props.density * 0.01;
        let v = d * d * d * 0.9 + 0.1;
        let v = if v >= 0.1 { if v < 1.0 { v } else { 1.0 } } else { 0.1 };
        self.set_late_delays(v * 0.061, 1.32, 0.0015, 1.47);
        self.set_decay_time();
    }

    fn set_diffusion(&mut self) {
        let p = &mut self.props;
        p.diffusion = clamp(p.diffusion, 0.0, 100.0);
        let x = p.diffusion * 0.01 * 0.7853982;
        self.hadamard.v = x.tan();
        self.diffusion_scale.v = (x.cos() as f64).powf(self.matrix_stages as f64) as f32;
    }

    fn set_reflections_delay(&mut self) {
        let p = &mut self.props;
        p.reflections_delay = clamp(p.reflections_delay, 0.0, 0.3);
        self.el_sec[0] = p.reflections_delay;
        let n = (self.rate as f32 * p.reflections_delay) as i32;
        self.el_samples[0] = if n != 0 { n } else { 1 };
        self.set_reverb_delay();
    }

    fn set_reverb_delay(&mut self) {
        let p = &mut self.props;
        p.reverb_delay = clamp(p.reverb_delay, 0.0, 0.1);
        let first = p.reverb_delay + p.reflections_delay;
        self.set_late_early_late_taps(first, 0.0187, 1.29);
    }

    // ---- ASfxDsp's delay setters -------------------------------------------------------------------------------

    /// ASfxDsp::SetLate_EarlyLateDelayTaps: tap k (1..9) at `first + step * ratio^(k - 2)` seconds, tap 1 at `first`.
    fn set_late_early_late_taps(&mut self, first: f32, step: f32, ratio: f32) {
        let rate = self.rate as f32;
        self.el_sec[1] = first;
        let n = (first * rate) as i32;
        self.el_samples[1] = if n != 0 { n } else { 1 };
        let mut q = step;
        for k in 2..9 {
            let s = first + q;
            self.el_sec[k] = s;
            self.el_samples[k] = (s * rate) as i32;
            q *= ratio;
        }
    }

    /// ASfxDsp::SetEarlyDelay
    fn set_early_delay(&mut self, first: f32, ratio: f32) {
        let rate = self.rate as f32;
        let mut s = first;
        for k in 0..7 {
            self.early_sec[k] = s;
            self.early_rp[k] = (s * rate) as i32 - 1;
            s *= ratio;
        }
    }

    /// ASfxDsp::SetLateDelays
    fn set_late_delays(&mut self, first: f32, ratio: f32, first_b: f32, ratio_b: f32) {
        let rate = self.rate as f32;
        let (mut s, mut b) = (first, first_b * rate);
        for k in 0..LATE {
            self.late_sec[k] = s;
            self.late_len[k] = (s * rate + 0.5) as i32;
            self.late_rp[k] = (self.late_len[k] - 1 + self.late_wp[k]) & self.late_mask[k];
            self.late_len_b[k] = (b + 0.5) as i32;
            self.late_rp_b[k] = (self.late_len_b[k] + self.late_wp[k] - 1) & self.late_mask[k];
            s *= ratio;
            b *= ratio_b;
        }
    }

    /// DSPSfxReverb::readInternal with every speaker active: ASfxDsp::DoDSPProcessing of one block of `channels`
    /// interleaved channels into `out`.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], channels: usize) {
        let ch = channels;
        let len = input.len() / ch;
        let inv = 1.0 / len as f32;
        for c in 0..ch {
            for f in 0..len {
                let i = f * ch + c;
                out[i] = if f % 2 == 0 { input[i] + 1e-15 } else { input[i] - 1e-15 };
            }
        }
        // BlockProcessInput: the channels summed.
        self.in_buff.resize(len, 0.0);
        for f in 0..len {
            let x = &out[f * ch..(f + 1) * ch];
            self.in_buff[f] = match ch {
                1 => x[0],
                6 | 8 => x[2..].iter().fold(x[1] + x[0], |s, v| s + v),
                _ => x.iter().fold(0.0, |s, v| s + v),
            };
        }
        for i in 0..9 {
            self.el_rp[i] = (self.el_wp - 1 + self.el_samples[i]) & self.el_mask;
            if self.el_samples[i] == self.old_el_samples[i] {
                self.el_changed[i] = false;
            } else {
                self.el_changed[i] = true;
                self.old_el_rp[i] = (self.el_wp - 1 + self.old_el_samples[i]) & self.el_mask;
                self.old_el_samples[i] = self.el_samples[i];
            }
        }
        for i in 0..LATE {
            let (wp, m) = (self.late_wp[i], self.late_mask[i]);
            self.late_rp[i] = (self.late_len[i] - 1 + wp) & m;
            self.late_rp_b[i] = (wp - 1 + self.late_len_b[i]) & m;
            if self.late_len[i] == self.old_late_len[i] {
                self.late_changed[i] = false;
            } else {
                self.late_changed[i] = true;
                self.old_late_rp[i] = (self.old_late_len[i] - 1 + wp) & m;
                self.old_late_len[i] = self.late_len[i];
            }
            self.latch_feedback[i] = self.feedback[i];
            if self.feedback[i] == self.old_feedback[i] {
                self.prev_feedback[i] = self.feedback[i];
            } else {
                if !self.late_changed[i] {
                    self.late_changed[i] = true;
                    self.old_late_rp[i] = (self.old_late_len[i] - 1 + wp) & m;
                }
                self.prev_feedback[i] = self.old_feedback[i];
                self.old_feedback[i] = self.latch_feedback[i];
            }
            self.latch_decay_hf[i] = self.decay_hf[i];
            self.latch_one_minus_decay_hf[i] = 1.0 - self.decay_hf[i];
            if self.decay_hf[i] == self.old_decay_hf[i] {
                self.decay_hf_changed[i] = false;
            } else {
                self.decay_hf_changed[i] = true;
                self.prev_decay_hf[i] = self.old_decay_hf[i];
                self.old_decay_hf[i] = self.latch_decay_hf[i];
            }
        }
        self.lr_gain.latch();
        self.er_gain.latch();
        self.room_hf.latch();
        self.hadamard.latch();
        self.diffusion_scale.latch();
        let g = self.allpass_gain;
        for n in 0..len {
            let (a, b) = ((len - n) as f32, n as f32);
            let r = self.room_hf.at(a, b, inv);
            let h = self.hadamard.at(a, b, inv);
            let ds = self.diffusion_scale.at(a, b, inv);
            self.z_room_hf[0] = (1.0 - r) * self.z_room_hf[0] + r * self.in_buff[n];
            self.z_room_hf[1] = self.z_room_hf[0] * r + (1.0 - r) * self.z_room_hf[1];
            let mut x = self.z_room_hf[1];
            if self.room_lf != 0.0 {
                let [a0, a1, a2, b1, b2] = self.room_lf_coeffs;
                let y = x * a0 + self.z_room_lf[0];
                self.z_room_lf[0] = x * a1 + self.z_room_lf[1] + y * b1;
                self.z_room_lf[1] = y * b2 + x * a2;
                x = y;
            }
            self.el_wp = wrap(self.el_wp, self.el_mask);
            self.el[self.el_wp as usize] = x;
            self.el_rp[0] = wrap(self.el_rp[0], self.el_mask);
            let er = self.er_gain.at(a, b, inv);
            let mut e = er * self.el[self.el_rp[0] as usize];
            if self.el_changed[0] {
                self.old_el_rp[0] = wrap(self.old_el_rp[0], self.el_mask);
                e = (er * self.el[self.old_el_rp[0] as usize] * a + b * e) * inv;
            }
            let mut v = e;
            for k in 0..2 {
                let m = self.allpass_mask[k];
                self.allpass_rp[k] = wrap(self.allpass_rp[k], m);
                self.allpass_wp[k] = wrap(self.allpass_wp[k], m);
                let t = self.allpass[k][self.allpass_rp[k] as usize] - g * v;
                self.allpass[k][self.allpass_wp[k] as usize] = g * t + v;
                v = t;
            }
            self.early_wp = wrap(self.early_wp, self.early_mask);
            self.early[self.early_wp as usize] = v;
            let mut taps = [v; LATE];
            for k in 0..7 {
                self.early_rp[k] = wrap(self.early_rp[k], self.early_mask);
                taps[k + 1] = self.early[self.early_rp[k] as usize];
            }
            for i in 0..LATE {
                self.el_rp[i + 1] = wrap(self.el_rp[i + 1], self.el_mask);
                let mut t = self.el[self.el_rp[i + 1] as usize];
                if self.el_changed[i + 1] {
                    self.old_el_rp[i + 1] = wrap(self.old_el_rp[i + 1], self.el_mask);
                    t = (a * self.el[self.old_el_rp[i + 1] as usize] + b * t) * inv;
                }
                let lr = self.lr_gain.at(a, b, inv);
                let m = self.late_mask[i];
                self.late_rp[i] = wrap(self.late_rp[i], m);
                let line = &self.late[i];
                let d = if !self.late_changed[i] {
                    line[self.late_rp[i] as usize] * self.latch_feedback[i]
                } else {
                    self.old_late_rp[i] = wrap(self.old_late_rp[i], m);
                    (line[self.late_rp[i] as usize] * self.latch_feedback[i] * b + line[self.old_late_rp[i] as usize] * self.prev_feedback[i] * a) * inv
                };
                let v = (d + lr * t) * ds;
                let z = &mut self.z_decay_hf[i];
                if !self.decay_hf_changed[i] {
                    *z = v * self.latch_decay_hf[i] + self.latch_one_minus_decay_hf[i] * *z;
                } else {
                    let k = (a * self.prev_decay_hf[i] + b * self.latch_decay_hf[i]) * inv;
                    *z = (1.0 - k) * *z + k * v;
                }
                self.late_wp[i] = wrap(self.late_wp[i], m);
            }
            // The 8x8 Hadamard-style rotation in three stages.
            let (z, mx) = (self.z_decay_hf, &mut self.matrix);
            for p in 0..4 {
                mx[2 * p] = h * z[2 * p + 1] + z[2 * p];
                mx[2 * p + 1] = z[2 * p + 1] - h * z[2 * p];
            }
            for q in 0..2 {
                let o = 4 * q;
                for j in 0..2 {
                    mx[8 + o + j] = h * mx[o + 2 + j] + mx[o + j];
                    mx[8 + o + 2 + j] = mx[o + 2 + j] - h * mx[o + j];
                }
            }
            for j in 0..4 {
                mx[16 + j] = h * mx[12 + j] + mx[8 + j];
                mx[20 + j] = mx[12 + j] - h * mx[8 + j];
            }
            for i in 0..LATE {
                let v = if i < 7 { mx[17 + i] } else { mx[16] };
                self.late[i][self.late_wp[i] as usize] = v;
            }
            let terms = 8 / ch;
            for c in 0..ch {
                let (mut es, mut ls) = (0.0f32, 0.0f32);
                for j in 0..terms {
                    let i = c + j * ch;
                    es += taps[i];
                    self.late_rp_b[i] = wrap(self.late_rp_b[i], self.late_mask[i]);
                    ls += self.late[i][self.late_rp_b[i] as usize];
                }
                let k = n * ch + c;
                out[k] = if self.dry_lin == 0.0 { es + ls } else { self.dry_lin * input[k] + es + ls };
            }
        }
    }
}

/// `if (lo <= v) { if (hi < v) hi else v } else lo`
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if lo <= v { if hi < v { hi } else { v } } else { lo }
}

/// (float)pow(10.0, (double)x)
fn pow10(x: f32) -> f32 {
    10f64.powf(x as f64) as f32
}

/// DSPSfxReverb::Calculate1stOrderLowpassCoeff (1810fc550): the one-pole coefficient whose gain at `freq` is `g`.
fn lowpass_coeff(g: f32, freq: f32, rate: f32) -> f32 {
    let g = if g < 1.0 { g } else { 1.0 };
    if g == 1.0 {
        return 0.0;
    }
    let w = freq * 6.2831855 / rate;
    let g2 = g * g;
    let a = g2 - 1.0;
    let b = 1.0 - w.cos() * g2;
    let b = b + b;
    let disc = b * b - a * 4.0 * a;
    let disc = if disc > 0.0 { disc } else { 0.0 };
    let c = (disc.sqrt() - b) / (a + a);
    if !(c >= 0.0) {
        0.0
    } else if c > 0.99 {
        0.99
    } else {
        c
    }
}

/// DSPSfxReverb::CalculateShelfCoeffs (1810fc640): the low shelf's a0, a1, a2, b1, b2 for `gain` (mB / 100) at `freq`.
fn shelf_coeffs(gain: f32, freq: f32, rate: f32) -> [f32; 5] {
    const S: f32 = 1.4142135;
    let t = (freq * 3.1415927 / rate).tan();
    let u = t / (gain * 0.057564627).exp();
    let norm = 1.0 / ((u + S) * u + 1.0);
    let a1 = (t * t - 1.0) * norm;
    [((t + S) * t + 1.0) * norm, a1 + a1, ((t - S) * t + 1.0) * norm, (u * u - 1.0) * -2.0 * norm, -(((u - S) * u + 1.0) * norm)]
}

#[cfg(test)]
mod tests {
    #[test]
    fn reverb_presets() {
        // SewerPipe is the Main Camera's User values; Off keeps the dry level and the reflections delay.
        let mut p = [0.0, -1000.0, -1000.0, 2.81, 0.14, 429.0, 0.0, 1023.0, 0.021, 80.0, 60.0, 5000.0, 0.0, 250.0];
        let user = p;
        super::change_properties(&mut p, 27);
        assert_eq!(p, user);
        super::change_properties(&mut p, 22);
        assert_eq!(p, user);
        p[0] = -123.0;
        p[6] = 0.2;
        super::change_properties(&mut p, 0);
        assert_eq!(p, [-123.0, -10000.0, -10000.0, 1.0, 1.0, -2602.0, 0.2, 200.0, 0.011, 0.0, 0.0, 5000.0, 0.0, 250.0]);
    }

    #[test]
    fn sfx_reverb_impulse() {
        let mut r = super::SfxReverb::new(48000);
        // The Main Camera's filter as CameraListenerBehaviour leaves it near (dry 0 mB, room -1000).
        for (i, v) in [0.0, -1000.0, -1000.0, 2.81, 0.14, 429.0, 0.0, 1023.0, 0.021, 80.0, 62.3, 5000.0, 0.0, 250.0].into_iter().enumerate() {
            r.set_parameter(i, v);
        }
        r.update();
        assert_eq!((r.props.room, r.props.reflections, r.props.reverb), (-1000, 429, 1023));
        // A block for the gains' ramps from their defaults, then an impulse.
        let zero = vec![0.0f32; 2048];
        let mut out = vec![0.0f32; 2048];
        r.process(&zero, &mut out, 2);
        let mut input = vec![0.0f32; 2048];
        input[0] = 1.0;
        input[1] = 1.0;
        r.process(&input, &mut out, 2);
        // The dry signal at 0 mB, then the reflections and the tail ring.
        assert!(out[0] >= 1.0 && out.iter().all(|v| v.is_finite()), "{:?}", &out[..4]);
        let energy = |o: &[f32]| o.iter().map(|v| v * v).sum::<f32>();
        r.process(&zero, &mut out, 2);
        assert!(energy(&out) > 1e-4, "{}", energy(&out));
        let (mut a, mut b) = (vec![0.0f32; 2048], vec![0.0f32; 2048]);
        for _ in 0..20 {
            r.process(&zero, &mut a, 2);
        }
        for _ in 0..40 {
            r.process(&zero, &mut b, 2);
        }
        assert!(energy(&b) < energy(&a) && energy(&b) > 0.0);
    }
}
