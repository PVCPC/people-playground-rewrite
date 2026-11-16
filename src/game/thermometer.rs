//! ThermometerBehaviour (the Thermometer) and InfraredThermometer: their readouts are world texts (world_text.rs); the
//! lights are rendering. InfraredThermometer's Update reads the temperature of the first thing its laser hits, with the
//! Physics2D raycasts and Debug drawing, and writes its texts and line.
#![allow(non_snake_case)]

use super::{mathf, script, Game};
use crate::unity::transform::{global_position, global_rotation, Trs};

script! {
    Thermometer {
        Activated: bool = true,
        Min: i32 = -999,
        Max: i32 = 9999,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// Phys and TextMesh (world_text.rs).
        Phys: Option<usize> = None,
        TextMesh: Option<usize> = None,
        /// Light (UpdateActivation and OnDisable switch it).
        Light: Option<super::visual::SpriteAt> = None,
        /// lastShownTemp
        lastShownTemp: f32 = f32::MIN,
    }
}

script! {
    InfraredThermometer {
        Activated: bool = true,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>(): base.transform.localScale.x.
        physical: usize = 0,
        /// RayPoint: the local TRS of its object and of each parent below the thermometer's own object.
        RayPoint: Vec<Trs> = Vec::new(),
        /// LayersToHit (LayerMask bits)
        LayersToHit: u32 = 0,
        /// LineRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// ValueText and UnitText (world_text.rs).
        ValueText: Option<usize> = None,
        UnitText: Option<usize> = None,
    }
}

impl Game {
    /// MaintainScaleSignBehaviour.Start, MaintainScaleSignBehaviour.OnFlip (flip_broadcast) and MaintainScaleSignBehaviour.EnsureScaling: a negative lossy scale.x flips the local scale.x back, so the
    /// object's own x is positive in the world. The object's other components are unchanged.
    pub fn maintain_scale_start(&mut self, id: i32) {
        self.with_component_instance(id, |g, inst, o| {
            if inst.lossy_scale(o)[0] < 0.0 {
                let s = inst.objects[o].trs.s;
                inst.set_local_scale(o, [-s[0], s[1], s[2]]);
                g.texts_local_scale_changed(inst, o);
            }
        });
    }

    /// ThermometerBehaviour.Use: Activated flips while the component is enabled.
    pub fn thermometer_use(&mut self, t: usize) {
        if self.thermometers[t].enabled {
            self.thermometers[t].Activated = !self.thermometers[t].Activated;
            self.thermometer_update_activation(t);
        }
    }

    /// ThermometerBehaviour.UpdateActivation, ThermometerBehaviour.Start, ThermometerBehaviour.OnEnable: TextMesh.enabled = Activated, then
    /// Light.enabled.
    pub fn thermometer_update_activation(&mut self, t: usize) {
        let (text, on) = (self.thermometers[t].TextMesh, self.thermometers[t].Activated);
        self.text_enabled_set(Self::text_ref(text), on);
        let light = self.thermometers[t].Light.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, on);
    }

    /// ThermometerBehaviour.OnDisable: TextMesh.enabled = false, then Light.enabled.
    pub fn thermometer_on_disable(&mut self, t: usize) {
        let text = self.thermometers[t].TextMesh;
        self.text_enabled_set(Self::text_ref(text), false);
        let light = self.thermometers[t].Light.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, false);
    }

    /// ThermometerBehaviour.FixedUpdate
    pub fn thermometer_fixed_update(&mut self, t: usize) {
        let x = &self.thermometers[t];
        let Some(p) = x.Phys else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let num = mathf::clamp(self.physicals[p].Temperature, x.Min as f32, x.Max as f32);
        if (num - x.lastShownTemp).abs() > 0.45 {
            self.thermometers[t].lastShownTemp = num;
            let text = match self.prefs.TemperatureUnit {
                1 => format!("<mspace=0.057>{} 'F", super::temperature::round_to_int(super::celsius_to_fahrenheit(num)).clamp(-999, 9999)),
                2 => format!("<mspace=0.057>{}  K", super::temperature::round_to_int(super::celsius_to_kelvin(num)).clamp(0, 9999)),
                _ => format!("<mspace=0.057>{} 'C", super::temperature::round_to_int(num)),
            };
            let mesh = self.thermometers[t].TextMesh;
            self.text_write(mesh, &text);
        }
    }

    /// InfraredThermometer.Use
    pub fn infrared_use(&mut self, i: usize) {
        if self.infrared_thermometers[i].enabled {
            self.infrared_thermometers[i].Activated = !self.infrared_thermometers[i].Activated;
            self.infrared_line_enabled(i);
        }
    }

    /// InfraredThermometer.UpdateActivation: ValueText.enabled and LineRenderer.enabled = Activated.
    pub(crate) fn infrared_line_enabled(&mut self, i: usize) {
        let (line, on, text) = (self.infrared_thermometers[i].line, self.infrared_thermometers[i].Activated, self.infrared_thermometers[i].ValueText);
        self.text_enabled_set(Self::text_ref(text), on);
        self.lines[line].enabled = on;
    }

    /// InfraredThermometer.Update and InfraredThermometer.ReadTemperatureFromRay: while Activated, ReadTemperatureFromRay casts Utils.GetLaserEndPoint (whose
    /// Physics2D.Raycast syncs transforms). The temperature it returns only becomes the readout text, and the vertices
    /// and `finalHit` it keeps only feed the line renderer: presentation.
    pub fn infrared_update(&mut self, i: usize) {
        let s = &self.infrared_thermometers[i];
        if !s.Activated {
            return;
        }
        let p = s.physical;
        // RayPoint.position and .right: its local TRS up to the thermometer's object, then the object's chain.
        let nodes: Vec<Trs> = s.RayPoint.iter().copied().chain(self.local_chain(p).iter().rev().copied()).collect();
        let (mut origin, mut rotation) = ([nodes[0].t[0], nodes[0].t[1], nodes[0].t[2], 0.0], nodes[0].q);
        for n in &nodes[1..] {
            (origin, rotation) = (global_position(n, origin), global_rotation(n, rotation));
        }
        // RayPoint.right * (localScale.x < 0 ? -1 : 1), as a Vector2.
        let sign = if self.physicals[p].transform.localScale[0] < 0.0 { -1.0 } else { 1.0 };
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let (origin, direction) = ([origin[0], origin[1]], [right[0] * sign, right[1] * sign]);
        let num = self.infrared_read_temperature_from_ray(i, origin, direction);
        let value = if num.is_nan() { String::new() } else { super::temperature::round_to_int(self.prefs.celsius_to_preference(mathf::clamp(num, -273.0, 9999.0))).to_string() };
        let (value_text, unit_text) = (self.infrared_thermometers[i].ValueText, self.infrared_thermometers[i].UnitText);
        self.text_write(value_text, &value);
        let suffix = super::utils::get_temperature_unit_suffix(self.prefs.TemperatureUnit);
        self.text_write(unit_text, suffix);
    }

    /// InfraredThermometer.ReadTemperatureFromRay (gapA r16: its own fn for the fn-check)
    pub fn infrared_read_temperature_from_ray(&mut self, i: usize, origin: [f32; 2], direction: [f32; 2]) -> f32 {
        let layers = self.infrared_thermometers[i].LayersToHit;
        let mut hits = Vec::new();
        self.get_laser_end_point(origin, direction, &mut hits, layers, 8.495455, 8);
        // ReadTemperatureFromRay's line: positionCount and SetPositions of the origin and each hit, or of the origin and
        // origin + direction * 8.495455 when nothing was hit.
        let line = self.infrared_thermometers[i].line;
        let vertices: Vec<[f32; 3]> = if hits.is_empty() {
            vec![[origin[0], origin[1], 0.0], [origin[0] + direction[0] * 8.495455, origin[1] + direction[1] * 8.495455, 0.0]]
        } else {
            std::iter::once([origin[0], origin[1], 0.0]).chain(hits.iter().map(|h| [h.point[0], h.point[1], 0.0])).collect()
        };
        self.line_set_position_count(line, vertices.len());
        self.line_set_positions(line, &vertices);
        // ReadTemperatureFromRay's return: the last hit's physical (ambient when it does not simulate temperature), else NaN.
        match hits.last().and_then(|h| h.physical) {
            Some(p) if !self.physicals[p].SimulateTemperature => self.AmbientTemperature,
            Some(p) => self.physicals[p].Temperature,
            // float.NaN is 0xffc00000 (the game's return bits on fng2_items1), not Rust's 0x7fc00000.
            None => f32::from_bits(0xffc0_0000),
        }
    }

    /// InfraredThermometer.OnDisable: Activated = false (UpdateActivation hides its text and line).
    pub fn infrared_on_disable(&mut self, i: usize) {
        self.infrared_thermometers[i].Activated = false;
        self.infrared_line_enabled(i);
    }
}
