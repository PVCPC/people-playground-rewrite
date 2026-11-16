//! PowerGaugeBehaviour and TemperatureGaugeBehaviour (GaugeBehaviour's Start, Awake and Update for the power gauge). Start instantiates LabelCount label
//! objects under GaugeCenter, which carry no physics: each one's text is the gauge value there and its Transform hangs
//! from GaugeCenter's, at the point of the dial. Update and Awake move the arm and read the charge.
#![allow(non_snake_case)]

use super::{mathf, script, Game};
use crate::assets::db::ObjRef;

script! {
    PowerGauge {
        /// LabelCount (GaugeBehaviour)
        LabelCount: i32 = 5,
        /// MaxChargeDisplay (PowerGaugeBehaviour)
        MaxChargeDisplay: f32 = 500.0,
        /// MaxTemperatureDisplay (TemperatureGaugeBehaviour)
        MaxTemperatureDisplay: f32 = 2000.0,
        /// MinAngle and MaxAngle: the arm's z angle at 0 and at 1.
        MinAngle: f32 = 162.105,
        MaxAngle: f32 = -162.105,
        /// GaugeRadius and LabelOffset: the labels sit this far from GaugeCenter.
        GaugeRadius: f32 = 0.1571429,
        LabelOffset: f32 = 0.02,
    }
    links {
        /// A TemperatureGaugeBehaviour (else a PowerGaugeBehaviour): GetProgress reads the temperature.
        temperature: bool = false,
        /// The component's instance id.
        id: i32 = 0,
        /// GaugeLabelPrefab (None: Instantiate(null) would throw).
        label: Option<ObjRef> = None,
        /// GaugeCenter's object index in the prefab, and what its Transform hangs from (the object first).
        center: usize = 0,
        center_anchor: super::BareAnchor = super::BareAnchor::Root,
        center_chain: Vec<crate::unity::transform::Trs> = Vec::new(),
        /// PhysicalBehaviour (None: GetProgress would throw a NullReferenceException).
        PhysicalBehaviour: Option<usize> = None,
        /// The DamagableMachineryBehaviour GaugeBehaviour.Awake finds on its object (None: Update throws).
        damagableMachinery: Option<usize> = None,
        /// The private armVel and armPos.
        armVel: f32 = 0.0,
        armPos: f32 = 0.0,
        /// Arm's object index in the prefab.
        arm: usize = 0,
    }
}

impl Game {
    /// PowerGaugeBehaviour.ChargeFunction and PressureGaugeBehaviour.ChargeFunction
    pub(crate) fn power_gauge_charge_function(i: f32) -> f32 {
        super::mathf::sqrt(i)
    }

    /// PowerGaugeBehaviour.GetDisplayValueFor, PressureGaugeBehaviour.GetDisplayValueFor and TemperatureGaugeBehaviour.GetDisplayValueFor
    pub(crate) fn power_gauge_display_value_for(&self, g: usize, progress: f32) -> String {
        let s = &self.power_gauges[g];
        let v = if s.temperature {
            s.MaxTemperatureDisplay * Self::temperature_gauge_temp_function(progress) / 10.0
        } else {
            s.MaxChargeDisplay * Self::power_gauge_charge_function(progress) / 10.0
        };
        (super::temperature::round_to_int(v).wrapping_mul(10)).to_string()
    }

    // PressureGaugeBehaviour.constructor: the field defaults of `PowerGauge`.
    /// TemperatureGaugeBehaviour.TempFunction
    pub(crate) fn temperature_gauge_temp_function(i: f32) -> f32 {
        i
    }

    /// PowerGaugeBehaviour.GetProgress, PressureGaugeBehaviour.GetProgress and TemperatureGaugeBehaviour.GetProgress
    pub(crate) fn power_gauge_get_progress(&self, g: usize, p: usize) -> f32 {
        if self.power_gauges[g].temperature {
            return Self::temperature_gauge_temp_function(self.physicals[p].Temperature / self.power_gauges[g].MaxTemperatureDisplay);
        }
        Self::power_gauge_charge_function(self.physicals[p].Charge() / self.power_gauges[g].MaxChargeDisplay)
    }

    /// GaugeBehaviour.Update: the arm eases toward the progress (0 for a destroyed machine) as a damped spring.
    pub fn power_gauge_update(&mut self, g: usize) {
        // A null DamagableMachinery or PhysicalBehaviour is a NullReferenceException: the frame stops there.
        let Some(m) = self.power_gauges[g].damagableMachinery else { return };
        let num = if self.machinery[m].Destroyed {
            0.0
        } else {
            let Some(p) = self.power_gauges[g].PhysicalBehaviour else { return };
            self.power_gauge_get_progress(g, p)
        };
        let dt = self.time.deltaTime;
        let s = &mut self.power_gauges[g];
        s.armVel += (num - s.armPos) * dt;
        s.armPos += s.armVel * dt * 150.0;
        s.armVel *= 0.1f32.powf(dt);
        s.armPos = mathf::clamp01(s.armPos);
        self.power_gauge_set_arm(g, self.power_gauges[g].armPos);
    }

    /// GaugeBehaviour.SetArm: Arm.localEulerAngles = (0, 0, Lerp(MinAngle, MaxAngle, Clamp01(i))).
    pub(crate) fn power_gauge_set_arm(&mut self, g: usize, i: f32) {
        let i = mathf::clamp01(i);
        let s = &self.power_gauges[g];
        let (id, arm, z) = (s.id, s.arm, mathf::lerp(s.MinAngle, s.MaxAngle, i));
        self.with_component_instance(id, |_, inst, _| inst.set_local_rotation(arm, crate::game::menu::euler_z_rotation(z)));
    }

    // GaugeBehaviour.Awake: damagableMachinery = GetComponent<DamagableMachineryBehaviour>() (the `damagableMachinery` link).
    /// GaugeBehaviour.Start and GaugeBehaviour.CreateLabels: CreateLabels, LabelCount instances of GaugeLabelPrefab under GaugeCenter.
    pub fn power_gauge_start(&mut self, g: usize) {
        let (id, center, count) = (self.power_gauges[g].id, self.power_gauges[g].center, self.power_gauges[g].LabelCount);
        let label = self.power_gauges[g].label.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let (at, center_active) = self.with_component_instance(id, |_, inst, _| (inst.position(center), inst.objects[center].active));
        // Instantiate(prefab, parent) keeps the prefab root's local rotation and scale.
        let rotation = {
            let db = self.assets.as_mut().expect("Instantiate without the game's data");
            let prefab = db.prefab(label).unwrap_or_else(|e| panic!("Instantiate: {e}"));
            match &prefab.objects[0].components[0].1 {
                crate::assets::db::Component::Transform(t) => t.local_rotation,
                _ => unreachable!("a prefab root without a Transform"),
            }
        };
        let s = &self.power_gauges[g];
        let (min, max, radius, offset) = (s.MinAngle, s.MaxAngle, s.GaugeRadius, s.LabelOffset);
        let (anchor, parents) = (s.center_anchor, s.center_chain.clone());
        for i in 0..count {
            let num = i as f32 / (count as f32 - 1.0);
            let f = (mathf::lerp(min, max, num) + 90.0) * (std::f32::consts::PI / 180.0);
            let scale = radius - offset;
            let vector = [scale * (f as f64).cos() as f32, scale * (f as f64).sin() as f32];
            let root = self.spawn_prefab(label, at);
            let text = self.world_texts.iter().position(|t| t.root == root && t.object == 0);
            let text = text.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            let value = self.power_gauge_display_value_for(g, num);
            self.text_set(text, &value);
            let t = &mut self.world_texts[text];
            t.chain[0].t = [vector[0], vector[1], 0.0, 0.0];
            t.chain[0].q = rotation;
            t.chain.extend(parents.iter().copied());
            t.anchor = anchor;
            t.visible = center_active;
        }
    }
}
