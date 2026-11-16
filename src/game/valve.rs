//! ValveBehaviour: a BloodContainer whose ValvePosition (0 closed, 1 open) is driven by TurnRoutine along ValveTurnCurve.
//! Use (a direct activation, channel 0) turns it toward !LastTarget. Its container is limited to 0..0.5 and does not
//! overflow; below 0.5 the container keeps no pressure (AllowPressureTransfer). The valve wheel follows ValvePosition.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::coroutine::{Routine, Yield};
use super::{mathf, script, Game};
use crate::game_assets::{Curve, CurveCache};

script! {
    Valve {
        TurnDuration: f32 = 0.25,
        MinAngle: f32 = 0.0,
        MaxAngle: f32 = 90.0,
        ValvePosition: f32 = 0.0,
        LastTarget: bool = false,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// LiquidPressureValveBehaviour: AllowsTransfer is false, only pressure passes when open.
        pressure_only: bool = false,
        /// PhysicalBehaviour (GetComponent in Awake): only the turn sound reads it.
        physical: usize = 0,
        /// ValveTurnCurve and its native evaluation cache.
        ValveTurnCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        /// ValveTransform's renderers: (renderer, chain index); its local Transform is `wheel`.
        wheel_refs: Vec<(usize, usize)> = Vec::new(),
        /// ValveTransform's object index in the prefab (gapB r12: SetValveRotationToValue writes its Transform too).
        wheel_obj: usize = 0,
        /// DisplayRenderer, and the displayProperties block ValveBehaviour.Awake read from it.
        display: Option<super::visual::SpriteAt> = None,
        displayProperties: super::props::PropBlock = Default::default(),
        /// displayOffset: ValvePosition as of the last Update.
        displayOffset: f32 = 0.0,
        wheel: crate::unity::transform::Trs = crate::unity::transform::Trs { t: [0.0; 4], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] },
    }
}


impl Game {
    // LiquidPressureValveBehaviour.get_AllowsTransfer: false, `pressure_only` in wire_allows (blood_wire.rs).
    // LiquidPressureValveBehaviour.get_AllowPressureTransfer: ValvePosition > 0.5 (container_allows_pressure in container.rs).
    /// ValveBehaviour.AllowsTransfer: ValvePosition > 0.5.
    pub fn valve_allows_transfer(&self, v: usize) -> bool {
        self.valves[v].ValvePosition > 0.5
    }

    /// ValveBehaviour.Limits: (0, 0.5).
    pub fn valve_limits(&self) -> [f32; 2] {
        [0.0, 0.5]
    }

    /// ValveBehaviour.Use: channel 1 opens, channel 2 closes, and any other channel toggles toward !LastTarget.
    pub fn valve_use(&mut self, v: usize, channel: i32) {
        let open = match channel {
            1 => true,
            2 => false,
            _ => !self.valves[v].LastTarget,
        };
        let id = self.valves[v].id;
        self.start_coroutine(id, id, Self::valve_turn_routine(v, open));
    }

    /// ValveBehaviour.OpenValve: StopCoroutine("TurnRoutine") (no coroutine is started by name, so nothing stops), then
    /// StartCoroutine(TurnRoutine(open: true)).
    pub fn valve_open(&mut self, v: usize) {
        let id = self.valves[v].id;
        self.start_coroutine(id, id, Self::valve_turn_routine(v, true));
    }

    /// ValveBehaviour.CloseValve: the same as OpenValve, with TurnRoutine(open: false).
    pub fn valve_close(&mut self, v: usize) {
        let id = self.valves[v].id;
        self.start_coroutine(id, id, Self::valve_turn_routine(v, false));
    }

    /// ValveBehaviour.TurnRoutine(open): each frame sets ValvePosition to the curve at t (1 - t when closing), waits for
    /// the end of the frame, then advances t by dt / TurnDuration. It ends once ValvePosition is approximately the target.
    pub(crate) fn valve_turn_routine(v: usize, open: bool) -> Routine {
        let target = if open { 1.0f32 } else { 0.0 };
        let mut t = 0.0f32;
        let mut started = false;
        Box::new(move |g| {
            if !started {
                started = true;
                g.valves[v].LastTarget = open;
                // phys.PlayClipOnce(TurnSound)
                let (p, id) = (g.valves[v].physical, g.valves[v].id);
                let clip = g.audio.clip(id, "TurnSound");
                g.play_clip_once(p, clip, 1.0);
                let p = g.valves[v].ValvePosition;
                t = if open { p } else { 1.0 - p };
            } else {
                t += g.time.deltaTime / g.valves[v].TurnDuration;
            }
            if mathf::approximately(g.valves[v].ValvePosition, target) {
                g.valves[v].ValvePosition = target;
                g.valve_set_rotation(v);
                return Yield::Done;
            }
            let s = &mut g.valves[v];
            let x = if open { t } else { 1.0 - t };
            let next = s.ValveTurnCurve.as_ref().expect("ValveBehaviour.ValveTurnCurve").evaluate(&mut s.cache, x);
            s.ValvePosition = next;
            g.valve_set_rotation(v);
            Yield::WaitForEndOfFrame
        })
    }

    // ValveBehaviour.Awake: phys is `physical`; displayProperties is read from DisplayRenderer when linked (link.rs).
    /// ValveBehaviour.Start
    pub fn valve_start(&mut self, v: usize) {
        self.valve_set_rotation(v);
    }

    /// ValveBehaviour.SetValveRotationToValue: ValveTransform.localEulerAngles = (0, 0, LerpAngle(MinAngle, MaxAngle, ValvePosition)).
    pub fn valve_set_rotation(&mut self, v: usize) {
        let s = &self.valves[v];
        let num = mathf::delta_angle(s.MinAngle, s.MaxAngle);
        let z = (super::d(s.MinAngle) + super::d(num) * mathf::clamp01(s.ValvePosition)).f();
        self.valves[v].wheel.q = super::menu::local_euler_z_rotation(z);
        let (id, o) = (self.valves[v].id, self.valves[v].wheel_obj);
        self.with_component_instance(id, |_, inst, _| inst.set_local_rotation(o, super::menu::euler_z_rotation(z)));
        let wheel = self.valves[v].wheel;
        for &(r, k) in &self.valves[v].wheel_refs {
            self.renderers[r].chain[k] = wheel;
        }
    }

    /// ValveBehaviour.Update: the BloodContainer's update, then displayOffset = ValvePosition.
    pub fn valve_update(&mut self, v: usize) {
        self.blood_container_update(Owner::Valve(v));
        self.valves[v].displayOffset = self.valves[v].ValvePosition;
    }

    /// ValveBehaviour.OnWillRenderObject for each valve whose own sprite the camera draws: displayProperties `_Offset` = displayOffset,
    /// then DisplayRenderer.SetPropertyBlock.
    pub fn valves_will_render(&mut self) {
        for v in 0..self.valves.len() {
            let s = &self.valves[v];
            let Some(display) = s.display else { continue };
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(super::visual::SpriteAt::Physical(s.physical))) {
                continue;
            }
            let mut block = std::mem::take(&mut self.valves[v].displayProperties);
            block.set_float("_Offset", self.valves[v].displayOffset);
            self.set_property_block(display, &block);
            self.valves[v].displayProperties = block;
        }
    }
}
