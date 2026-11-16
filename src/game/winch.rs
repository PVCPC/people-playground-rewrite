//! WinchBehaviour: FixedUpdate drives the SliderJoint2D's motor at Speed * 0.01 (stopped at the limit it runs into)
//! and wakes the connected body; Use reverses it, or on channel 1 toggles the internal battery. Update turns the wheel
//! and SetLights shows the direction.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};

script! {
    Winch {
        BaseSpeed: f32 = 0.0,
        InternalBattery: f32 = 0.0,
        LowerLimit: f32 = 0.0,
        UpperLimit: f32 = 25.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// SliderJoint: the `Game::fixed_joints` entry, adopted from the spawn at Start.
        slider: Option<usize> = None,
        /// The `Game::autofix_joints` entry holding the prefab's SliderJoint2D until it is adopted.
        autofix: usize = 0,
        PositiveSpeedLight: Option<super::visual::SpriteAt> = None,
        NegativeSpeedLight: Option<super::visual::SpriteAt> = None,
        /// Wheel's local Transform and the renderers drawn through it: (renderer, chain index).
        wheel: crate::unity::transform::Trs = crate::unity::transform::Trs { t: [0.0; 4], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] },
        wheel_refs: Vec<(usize, usize)> = Vec::new(),
        /// Wheel's object index in the prefab (TurnWheel writes its Transform too).
        wheel_obj: usize = 0,
        previousSpeed: f32 = 0.0,
    }
}

impl Game {
    /// SliderJoint (a serialised reference): the prefab's SliderJoint2D, adopted into `fixed_joints` at first use
    /// (Start, or an OnDisable that comes first when the object goes before its Start).
    pub(super) fn winch_slider(&mut self, w: usize) -> usize {
        if let Some(f) = self.winches[w].slider {
            return f;
        }
        let a = std::mem::take(&mut self.autofix_joints[self.winches[w].autofix]).sliders.remove(0);
        if a.slot.is_none() {
            unimplemented!("mod-only: WinchBehaviour: a SliderJoint2D that was never created");
        }
        let f = self.adopt_prefab_joint(&a);
        self.winches[w].slider = Some(f);
        f
    }

    /// base.transform.root.localScale.x
    fn winch_root_scale_x(&mut self, w: usize) -> f32 {
        let root = self.root_id(self.winches[w].physical);
        self.with_component_instance(root, |_, inst, o| inst.objects[o].trs.s[0])
    }

    /// WinchBehaviour.Start: `f` is the adopted SliderJoint2D. connectedBody is set to null and back (each recreates
    /// the joint), then UpdateWinchLimits.
    pub fn winch_start(&mut self, w: usize, f: usize) {
        self.winches[w].slider = Some(f);
        let connected = self.fixed_joints[f].connected;
        self.fixed_joint_set_connected_body(f, None);
        self.fixed_joint_set_connected_body(f, connected);
        // AddContextMenuButtons: both limits open a dialog.
        use super::menu::{MenuAction as A, MenuButton as B};
        let p = self.winches[w].physical;
        self.physicals[p].menu.extend([
            B { LabelWhenMultipleAreSelected: crate::il!(str "WinchBehaviour::AddContextMenuButtons" 3), ..B::new("winchSetLower", crate::il!(str "WinchBehaviour::AddContextMenuButtons" 1), crate::il!(str "WinchBehaviour::AddContextMenuButtons" 2), A::Entry(super::menu::Entry::WinchLower(w))) },
            B { LabelWhenMultipleAreSelected: crate::il!(str "WinchBehaviour::AddContextMenuButtons" 7), ..B::new("winchSetUpper", crate::il!(str "WinchBehaviour::AddContextMenuButtons" 5), crate::il!(str "WinchBehaviour::AddContextMenuButtons" 6), A::Entry(super::menu::Entry::WinchUpper(w))) },
        ]);
        self.winch_update_limits(w);
        self.winch_set_lights(w);
    }

    /// WinchBehaviour.SetLights: the light of BaseSpeed's sign, while enabled.
    pub(crate) fn winch_set_lights(&mut self, w: usize) {
        let s = &self.winches[w];
        let flag = s.BaseSpeed > 0.0;
        let (on, pos, neg) = (s.enabled, s.PositiveSpeedLight, s.NegativeSpeedLight);
        self.set_sprite_enabled(pos.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), flag && on);
        self.set_sprite_enabled(neg.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), !flag && on);
    }

    /// WinchBehaviour.Update: TurnWheel, `Wheel.Rotate(0f, 0f, Time.deltaTime * Speed / 2f)` (localRotation *= Euler).
    pub fn winch_update(&mut self, w: usize) {
        let angle = (d(self.time.deltaTime) * self.winch_speed(w) / 2.0f32).f();
        let [rx, ry, rz, rw] = super::menu::euler_z_rotation(angle);
        let [x, y, z, ww] = self.winches[w].wheel.q;
        // Quaternion.operator*
        let q = [
            (d(ww) * rx + d(x) * rw + d(y) * rz - d(z) * ry).f(),
            (d(ww) * ry + d(y) * rw + d(z) * rx - d(x) * rz).f(),
            (d(ww) * rz + d(z) * rw + d(x) * ry - d(y) * rx).f(),
            (d(ww) * rw - d(x) * rx - d(y) * ry - d(z) * rz).f(),
        ];
        self.winches[w].wheel.q = crate::unity::transform::normalize(q);
        // gapB r13: Wheel's own Transform too (ours wrote only the renderers' copy).
        let (id, o, wq) = (self.winches[w].id, self.winches[w].wheel_obj, q);
        self.with_component_instance(id, |_, inst, _| inst.set_local_rotation(o, wq));
        let wheel = self.winches[w].wheel;
        for &(r, k) in &self.winches[w].wheel_refs {
            self.renderers[r].chain[k] = wheel;
        }
    }

    /// WinchBehaviour.UpdateWinchLimits: the limits in meters / 0.82397, mirrored for a flipped root.
    pub(crate) fn winch_update_limits(&mut self, w: usize) {
        let f = self.winch_slider(w);
        let flipped = self.winch_root_scale_x(w) < 0.0;
        let s = &self.winches[w];
        let (min, max) = if flipped { (0.0 - s.UpperLimit, 0.0 - s.LowerLimit) } else { (s.LowerLimit, s.UpperLimit) };
        let (jmin, jmax) = ((d(min) / 0.82397f32).f(), (d(max) / 0.82397f32).f());
        self.slider_joint_set_limits(f, jmin, jmax);
        // The per-function checks (--ui-screens): LowerLimit, UpperLimit and the joint's limits after (F4).
        let (lo, hi) = (self.winches[w].LowerLimit, self.winches[w].UpperLimit);
        crate::ui::uifn("WinchBehaviour.UpdateWinchLimits", || format!("{lo:.4} {hi:.4} {jmin:.4} {jmax:.4}"));
    }

    /// WinchBehaviour.Speed: BaseSpeed * Mathf.Min(20f, Mathf.Max(InternalBattery, PhysicalBehaviour.Charge)) * root.localScale.x.
    pub fn winch_speed(&mut self, w: usize) -> f32 {
        let s = &self.winches[w];
        let charge = self.physicals[s.physical].Charge();
        let (base, battery) = (s.BaseSpeed, s.InternalBattery);
        let sx = self.winch_root_scale_x(w);
        (d(base) * mathf::min(20.0, mathf::max(battery, charge)) * sx).f()
    }

    /// WinchBehaviour.WinchAudio (a null one throws).
    fn winch_source(&self, w: usize) -> i32 {
        self.audio.source_field(self.winches[w].id, "WinchAudio").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// WinchBehaviour.Awake: SliderJoint.useMotor = true is winch_on_enable; WinchAudio.clip = Loop.
    pub fn winch_awake(&mut self, w: usize) {
        let src = self.winch_source(w);
        let clip = self.audio.clip(self.winches[w].id, "Loop");
        self.audio.set_clip(src, clip);
    }

    /// WinchBehaviour.HandleSound
    pub(crate) fn winch_handle_sound(&mut self, w: usize, speed: f32) {
        let speed = speed.abs();
        let previous = self.winches[w].previousSpeed.abs();
        let src = self.winch_source(w);
        let id = self.winches[w].id;
        if speed < 0.05 && previous > 0.05 {
            self.audio.stop(src);
            let clip = self.audio.clip(id, "EndClip");
            self.audio.play_one_shot(src, clip.as_ref(), 1.0);
        } else if speed > 0.05 && previous < 0.05 {
            let clip = self.audio.clip(id, "StartClip");
            self.audio.play_one_shot(src, clip.as_ref(), 1.0);
            self.audio.play(src);
        }
    }

    /// WinchBehaviour.FixedUpdate (AffectSlider).
    pub fn winch_fixed_update(&mut self, w: usize) {
        let f = self.winch_slider(w);
        let speed = self.winch_speed(w);
        let num = (d(speed) * 0.01f32).f();
        self.winch_handle_sound(w, num);
        self.winches[w].previousSpeed = num;
        let stop = self.slider_joint_limit_state(f) == if num < 0.0 { 1 } else { 2 };
        let max_torque = self.fixed_joints[f].slider.as_ref().unwrap().max_motor_torque;
        self.slider_joint_set_motor(f, if stop { 0.0 } else { num }, max_torque);
        // SliderJoint.connectedBody.WakeUp()
        let Some(owner) = self.fixed_joints[f].connected else {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        };
        let rb = self.owner_rigidbody(owner);
        if rb.detached.is_none() {
            let body = rb.body;
            self.world.body_mut(body).set_sleeping_false();
        }
    }

    /// WinchBehaviour.Use
    pub fn winch_use(&mut self, w: usize, channel: i32) {
        let s = &mut self.winches[w];
        if channel == 1 {
            s.InternalBattery = if mathf::approximately(s.InternalBattery, 5.0) { 0.0 } else { 5.0 };
        } else {
            s.BaseSpeed = 0.0 - s.BaseSpeed;
            let src = self.winch_source(w);
            let clip = self.audio.clip(self.winches[w].id, "EndClip");
            self.audio.play_one_shot(src, clip.as_ref(), 1.0);
            self.winch_set_lights(w);
        }
    }

    /// WinchBehaviour.OnEnable: SliderJoint.useMotor = true. Before Start the joint keeps the prefab's useMotor, which
    /// link requires to be on already.
    pub fn winch_on_enable(&mut self, w: usize) {
        if let Some(f) = self.winches[w].slider {
            self.slider_joint_set_use_motor(f, true);
        }
        self.winch_set_lights(w);
    }

    /// WinchBehaviour.OnDisable: SliderJoint.useMotor = false.
    pub fn winch_on_disable(&mut self, w: usize) {
        let f = self.winch_slider(w);
        self.slider_joint_set_use_motor(f, false);
        let src = self.winch_source(w);
        self.audio.stop(src);
        self.winches[w].previousSpeed = 0.0;
        self.winch_set_lights(w);
    }

    // WinchBehaviour.SetLimitFromContextMenu: the dialog and its Apply are menu.rs (Entry::WinchLower / WinchUpper; Min/Max with the 0.01 gap, then winch_update_limits).
    // WinchBehaviour.AddContextMenuButtons: winch_start pushes both buttons.
    // WinchBehaviour.AffectSlider: winch_fixed_update.
    // WinchBehaviour.HandleSound: winch_handle_sound.
    // WinchBehaviour.TurnWheel: winch_update.
    // WinchBehaviour.Awake: winch_awake (SliderJoint.useMotor = true is winch_on_enable).
    // WinchBehaviour.OnFlip: winch_update_limits, called by FlipSelection (link.rs).
}
