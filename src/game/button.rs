//! ButtonBehaviour (the Button and the Small Button). FixedUpdate reads its SliderJoint2D's jointTranslation and is pressed
//! while |translation| < Threshold * lossyScale.y. A press (or, with TriggerOnExit, a release) sends IsolatedActivation
//! to the button's own GameObject, and the continuous listener sends IsolatedContinuousActivation there too. SendMessage
//! reaches that object's components only: the use wires added to it (link.rs refuses a receiver in the prefab). Awake
//! and OnDestroy add and remove the listener, Start adds the context menu button, and Update and OnDisable drive the light.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Button {
        Threshold: f32 = 0.1,
        TriggerOnExit: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// slider: the SliderJoint2D's object and component index in the prefab (its joint lives on the clone).
        slider_object: usize = 0,
        slider_component: usize = 0,
        /// ButtonBehaviour.isPressed
        isPressed: bool = false,
        /// The PhysicalBehaviour of its GameObject (where the sends go).
        physical: Option<usize> = None,
        /// ButtonBehaviour.phys (None when null).
        phys: Option<usize> = None,
        /// ButtonBehaviour.light (None when null, which Update and OnDisable throw on).
        light: Option<super::visual::SpriteAt> = None,
    }
}

impl Game {
    /// ButtonBehaviour.Start: the context menu button.
    pub fn button_start(&mut self, b: usize) {
        let p = self.buttons[b].phys.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        use super::menu::{Label, MenuAction as A, MenuButton as B};
        self.physicals[p].menu.push(B { label: Label::ButtonTrigger(b), ..B::new("toggleTriggerBehaviour", "", crate::il!(str "ButtonBehaviour::Start" 1), A::ButtonTrigger(b)) });
    }

    /// slider.jointTranslation (Joint2D, then SliderJoint2D's native read): the slider's `fixed_joints` entry when a
    /// script adopted it, otherwise the clone's live b2 prismatic joint (no live joint is unimplemented).
    fn button_slider_translation(&mut self, id: i32, object: usize, component: usize) -> f32 {
        let (slot, joint) = self.with_component_instance(id, |_, inst, _| (inst.anchored.get(&(object, component)).and_then(|s| s.slot), inst.joints.get(&(object, component)).copied()));
        if let Some(f) = slot.and_then(|s| self.adopted_slots.get(&s).copied()) {
            return self.slider_joint_translation(f);
        }
        match joint {
            // SliderJoint2D_Get_Custom_PropJointTranslation (180c47de0) -> 180c04ca0: `mov rcx, [rcx+0x58]; test; xorps xmm0` returns 0 without a live joint.
            None => 0.0,
            Some(j) => {
                let j = self.world.joints[j].as_ref().unwrap();
                j.prismatic().joint_translation(self.world.body(j.body_a), self.world.body(j.body_b))
            }
        }
    }

    /// ButtonBehaviour.FixedUpdate
    pub fn button_fixed_update(&mut self, b: usize) {
        let s = &self.buttons[b];
        let (id, object, component, threshold, flag) = (s.id, s.slider_object, s.slider_component, s.Threshold, s.isPressed);
        let translation = self.button_slider_translation(id, object, component);
        // base.transform.lossyScale.y of the button's own object.
        let scale = self.with_component_instance(id, |_, inst, o| inst.lossy_scale(o)[1]);
        let pressed = translation.abs() < threshold * scale;
        self.buttons[b].isPressed = pressed;
        if pressed && !flag {
            self.button_send(b, false);
        }
        if self.buttons[b].TriggerOnExit && !pressed && flag {
            self.button_send(b, false);
        }
    }

    /// ButtonBehaviour.Update: light.enabled = isPressed.
    pub fn button_update(&mut self, b: usize) {
        let on = self.buttons[b].isPressed;
        self.button_light(b, on);
    }

    /// ButtonBehaviour.OnDisable: light.enabled = false.
    pub fn button_on_disable(&mut self, b: usize) {
        self.button_light(b, false);
    }

    fn button_light(&mut self, b: usize, on: bool) {
        let light = self.buttons[b].light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: ButtonBehaviour.light"));
        self.set_sprite_enabled(light, on);
    }

    /// ButtonBehaviour.OnContinuousUpdate
    pub(crate) fn button_continuous_update(&mut self, b: usize) {
        if self.buttons[b].isPressed {
            self.button_send(b, true);
        }
    }

    /// Utils.SendAllChannel(Continuous)IsolatedActivation(this)
    fn button_send(&mut self, b: usize, continuous: bool) {
        let p = self.buttons[b].physical.unwrap_or_else(|| unimplemented!("mod-only: ButtonBehaviour on an object without a PhysicalBehaviour (its root)"));
        self.send_isolated(p, continuous);
    }
}
