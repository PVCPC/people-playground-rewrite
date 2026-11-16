//! LiquidPumpBehaviour (the Liquid Pressuriser): it holds no flow of its own (AllowsTransfer false) but pushes or pulls the
//! pressure of what it is wired to, and shakes while on. Its context buttons are UI; the display shows the mode's texture.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::{d, script, Game};

script! {
    LiquidPump {
        /// PressureDirection: Push 0, Pull 1, None 2.
        Mode: i32 = 2,
        VibrateForce: f32 = 0.0,
        VibrateRotationSpeed: f32 = 0.0,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// DisplayRenderer (Awake's `material` is its own instance).
        display: Option<super::visual::SpriteAt> = None,
        /// The Push, Pull and Idle textures.
        Push: Option<crate::assets::db::ObjRef> = None,
        Pull: Option<crate::assets::db::ObjRef> = None,
        Idle: Option<crate::assets::db::ObjRef> = None,
    }
}

impl Game {
    // LiquidPumpBehaviour.Awake: material = DisplayRenderer.material (set_material_prop writes that instance).
    // LiquidPumpBehaviour.OnEnable: SetMode(Mode) rewrites Mode with itself and sets the texture Start sets, so nothing changes.
    // LiquidPumpBehaviour.OnDestroy: Destroy(material), the renderer's own instance going with it.
    /// LiquidPumpBehaviour.UpdateDisplayTexture: material.SetTexture("_LEDMultiply", GetTargetDisplayTexture()).
    pub(crate) fn liquid_pump_update_display_texture(&mut self, l: usize) {
        let display = self.liquid_pumps[l].display.expect("LiquidPumpBehaviour.DisplayRenderer null (NullReferenceException in Awake)");
        let texture = self.liquid_pump_get_target_display_texture(l).unwrap_or_else(|| unimplemented!("mod-only: LiquidPumpBehaviour: a null display texture (SetTexture(null))"));
        self.set_material_prop(display, "_LEDMultiply", super::props::Prop::Texture(texture));
    }

    /// LiquidPumpBehaviour.GetTargetDisplayTexture: Push, Pull, else Idle.
    fn liquid_pump_get_target_display_texture(&self, l: usize) -> Option<crate::assets::db::ObjRef> {
        let s = &self.liquid_pumps[l];
        match s.Mode {
            0 => s.Push,
            1 => s.Pull,
            _ => s.Idle,
        }
    }

    /// LiquidPumpBehaviour.Start: the three pressure buttons, then SetMode(Mode) (only the display texture changes).
    pub(crate) fn liquid_pump_start(&mut self, l: usize) {
        use super::menu::{Cond, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.liquid_pumps[l].physical].menu;
        for (m, id, label, desc) in [(1, "setPumpToPull", "Depressurise", crate::il!(str "LiquidPumpBehaviour::Start" 2)),
            (0, "setPumpToPush", "Pressurise", crate::il!(str "LiquidPumpBehaviour::Start" 5)), (2, "setPumpToIdle", crate::il!(str "LiquidPumpBehaviour::Start" 7), crate::il!(str "LiquidPumpBehaviour::Start" 8))] {
            menu.push(B { condition: Cond::LiquidPumpNot(l, m), ..B::new(id, label, desc, A::LiquidPumpMode(l, m)) });
        }
        self.liquid_pump_set_mode(l, self.liquid_pumps[l].Mode);
    }

    /// LiquidPumpBehaviour.SetMode: Mode = mode and UpdateDisplayTexture, while enabled.
    pub(crate) fn liquid_pump_set_mode(&mut self, l: usize, mode: i32) {
        if self.liquid_pumps[l].enabled {
            self.liquid_pumps[l].Mode = mode;
            self.liquid_pump_update_display_texture(l);
        }
    }

    /// LiquidPumpBehaviour.NextMode: Mode + 1, wrapping past 2 to 0.
    pub(crate) fn liquid_pump_next_mode(&mut self, l: usize) {
        let mut mode = self.liquid_pumps[l].Mode;
        mode += 1;
        if mode > 2 {
            mode = 0;
        }
        self.liquid_pump_set_mode(l, mode);
    }

    /// LiquidPumpBehaviour.PreviousMode: Mode - 1, wrapping below 0 to 2.
    pub(crate) fn liquid_pump_previous_mode(&mut self, l: usize) {
        let mut mode = self.liquid_pumps[l].Mode;
        mode -= 1;
        if mode < 0 {
            mode = 2;
        }
        self.liquid_pump_set_mode(l, mode);
    }

    /// LiquidPumpBehaviour.Use: channel 1 is PreviousMode, any other NextMode. SetMode does nothing while disabled.
    pub fn liquid_pump_use(&mut self, l: usize, channel: i32) {
        if channel != 1 {
            self.liquid_pump_next_mode(l);
        } else {
            self.liquid_pump_previous_mode(l);
        }
    }

    /// LiquidPumpBehaviour.get_ForceVector: new Vector2(Mathf.Cos(f), Mathf.Sin(f)) * VibrateForce, f = Time.time * VibrateRotationSpeed.
    pub(crate) fn liquid_pump_force_vector(&self, l: usize) -> [f32; 2] {
        let s = &self.liquid_pumps[l];
        let f = (d(self.time.time) * s.VibrateRotationSpeed).f();
        [(f as f64).cos() as f32 * s.VibrateForce, (f as f64).sin() as f32 * s.VibrateForce]
    }

    /// LiquidPumpBehaviour.FixedUpdate: AddRelativeForce(ForceVector), negated for Pull.
    pub fn liquid_pump_fixed_update(&mut self, l: usize) {
        let s = &self.liquid_pumps[l];
        if s.Mode == 2 {
            return;
        }
        let v = self.liquid_pump_force_vector(l);
        let force = if self.liquid_pumps[l].Mode == 0 { v } else { [-v[0], -v[1]] };
        let rb = self.physicals[s.physical].rigidbody.clone();
        rb.add_relative_force(&mut self.world, force);
    }

    /// BloodContainer.Update, which LiquidPumpBehaviour does not override.
    pub fn liquid_pump_update(&mut self, l: usize) {
        self.blood_container_update(Owner::LiquidPump(l));
    }
}
