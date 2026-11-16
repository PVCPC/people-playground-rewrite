//! ContinuousActivationBehaviour and PhysicalBehaviour's ContinuousActivationTracker: DragTool's UseContinuous
//! broadcast (40 Hz) and the IsBeingUsedContinuously family the scripts read.
#![allow(non_snake_case)]

use super::misc::FixedIntervalDistributor;
use super::{Behaviour, Game};
use crate::assets::db::Component;

/// DeltaInt
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeltaInt {
    pub Value: i32,
    pub PreviousValue: i32,
    pub PreviousValueFixed: i32,
}

impl DeltaInt {
    /// DeltaInt.constructor: Value = PreviousValueFixed = PreviousValue = value.
    pub const fn new(value: i32) -> Self {
        Self { Value: value, PreviousValue: value, PreviousValueFixed: value }
    }
    /// DeltaInt.Increment
    pub fn increment(&mut self, v: i32) {
        self.Value = self.Value.wrapping_add(v);
    }
    /// DeltaInt.Decrement: Value = Mathf.Max(0, Value - 1).
    pub fn decrement(&mut self) {
        self.Value = self.Value.wrapping_sub(1).max(0);
    }
    /// DeltaInt.UpdatePreviousValue
    pub fn update_previous_value(&mut self) {
        self.PreviousValue = self.Value;
    }
}

/// ContinuousActivationBehaviour: activationSignalProcessor (RateHz 40, MaxCycles 4), the oscillation counter and the
/// OnContinuousUpdate listeners in AddListener order (the signal cleanup is signal.rs's).
#[derive(Clone, Debug)]
pub struct ContinuousActivation {
    pub signals: FixedIntervalDistributor,
    pub oscillation: u32,
    pub listeners: Vec<Listener>,
    /// The recording harness's held activate key (Listener::Hold): the calls still to make.
    pub hold_left: u32,
}

/// An OnContinuousUpdate listener. DragTool's comes from the scene's ToolController, before any item's. The
/// ActivatorElectrodeBehaviour's (ProcessEmitContinuous) and LiquidLevelGateBehaviour's (passingThrough never
/// set) do nothing when called. A listener's OnDestroy removes it: a destroyed one is skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listener {
    Drag,
    KeyTrigger(usize),
    Button(usize),
    ActivationTransformer(usize),
    Detector(usize),
    LaserReceiver(usize),
    ActivationToggle(usize),
    UseWire(usize),
    LiquidLevelGate(usize),
    ActivatorElectrode(usize),
    /// The recording harness's held activate key on a physical object (the recorder's `hold` act): DragTool's body
    /// for `hold_left` calls.
    Hold(usize),
}

/// gapA r8: an event the other scripts' Awake/Start/OnDestroy (un)register a component on, beside OnContinuousUpdate
/// (Listener). The model's own subscription (`listen_base`: the component linked, started where the game subscribes in
/// Start, not destroyed) plus what a later Awake/OnDestroy call added or took (`Game::listen_delta`) is the number of
/// times the event calls the component (`listen_count`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ev {
    /// PhysicalBehaviour.OnSingleUse (UnityEvent): ActivationToggleBehaviour.Awake's OnUse.
    OnSingleUse,
    /// PhysicalBehaviour.OnDisintegration (C# event): ActOnDisintegrate.Awake's OnDisintegration.
    OnDisintegration,
    /// ThermalVisionBehaviour.OnThermalVisionToggle (UnityEvent): ExistInThermalVision.Start's ThermalVisionToggled.
    OnThermalVisionToggle,
    /// ProjectileLauncherBehaviour.OnLaunch (C# event): JavelinBehaviour.Start's TransferTarget.
    OnLaunch,
    /// Global.LimbStatusToggled (C# event): InvisibleOptionInContextMenu.Awake's DetailViewToggled.
    LimbStatusToggled,
    /// DeleteAfterTime.OnEndOfLife (UnityEvent): IonBoltBehaviour.Awake's lambda.
    OnEndOfLife,
}

impl Ev {
    /// A UnityEvent's RemoveListener takes every call of the listener; a C# event's -= takes one (the last).
    fn unity(self) -> bool {
        matches!(self, Ev::OnSingleUse | Ev::OnThermalVisionToggle | Ev::OnEndOfLife)
    }
}

impl Game {
    /// The model's own subscription of component `id` on `ev` (0 or 1).
    fn listen_base(&self, id: i32, ev: Ev) -> i32 {
        let live = !self.destroyed_components.contains(&id);
        let on = match ev {
            Ev::OnSingleUse => self.activation_toggles.iter().any(|t| t.id == id),
            Ev::OnDisintegration => self.act_on_disintegrates.iter().any(|a| a.id == id),
            Ev::OnThermalVisionToggle => self.thermal_views.contains(&id),
            Ev::OnLaunch => self.javelins.iter().any(|j| j.id == id && j.subscribed),
            Ev::LimbStatusToggled => self.invisibles.iter().any(|i| i.id == id),
            // A DeleteAfterTime's OnEndOfLife calls the bolt's lambda even after the bolt went (no OnDestroy removes it).
            Ev::OnEndOfLife => return 1,
        };
        (on && live) as i32
    }

    /// How many times `ev` calls component `id`.
    pub fn listen_count(&self, id: i32, ev: Ev) -> usize {
        (self.listen_base(id, ev) + self.listen_delta.get(&(id, ev)).copied().unwrap_or(0)).max(0) as usize
    }

    /// `ev += ...` / AddListener (a second Awake subscribes again).
    pub fn listen_add(&mut self, id: i32, ev: Ev) {
        *self.listen_delta.entry((id, ev)).or_default() += 1;
    }

    /// `ev -= ...` / RemoveListener.
    pub fn listen_remove(&mut self, id: i32, ev: Ev) {
        let n = self.listen_count(id, ev) as i32;
        if n > 0 {
            *self.listen_delta.entry((id, ev)).or_default() -= if ev.unity() { n } else { 1 };
        }
    }

    /// Every subscription of `id` on `ev` taken out (fn-check: a registering case starts unsubscribed, MapConfigProbe.GenPre).
    pub fn listen_clear(&mut self, id: i32, ev: Ev) {
        let n = self.listen_count(id, ev) as i32;
        *self.listen_delta.entry((id, ev)).or_default() -= n;
    }
}

impl Default for ContinuousActivation {
    fn default() -> Self {
        // DragTool.Awake: OnContinuousUpdate.AddListener (the Listener::Drag below, before any item's).
        // ContinuousActivationBehaviour.Awake: Instance = this (Game::continuous).
        // ContinuousActivationBehaviour.Start: ResetAccumulator(), so the accumulator starts at 0.
        Self { signals: FixedIntervalDistributor { MaxCycles: 4, ..FixedIntervalDistributor::with_rate(40.0) }, oscillation: 0, listeners: vec![Listener::Drag], hold_left: 0 }
    }
}

/// ActivationPropagation.AllChannels
pub(crate) const CHANNELS: usize = 3;

/// The classes with a UseContinuous(ActivationPropagation) method, and their subclasses.
pub(super) const USE_CONTINUOUS_RECEIVERS: &[&str] = &[
    "ActivatorElectrodeBehaviour", "BeamRepeaterBehaviour", "BlasterBehaviour", "ElectricityTransformerBehaviour",
    "PhysicalBehaviour", "SignalConverterBehaviour", "TemperatureRayGunBehaviour", "TransmitterBehaviour", "UseBubbler",
    "UseWireBehaviour",
];

/// A receiver of the broadcast, in BroadcastMessage's order.
enum Receiver {
    Physical(usize),
    Behaviour(Behaviour),
    UseWire(usize),
}

impl Game {
    /// ContinuousActivationBehaviour.DeltaTime: activationSignalProcessor.IntervalSeconds.
    #[allow(dead_code)]
    pub fn continuous_delta_time(&self) -> f32 {
        self.continuous.signals.IntervalSeconds
    }

    /// PhysicalBehaviour.IsBeingUsedContinuously(channel)
    pub fn used_continuously_on(&self, p: usize, channel: usize) -> bool {
        self.physicals[p].ContinuousActivationTracker[channel].Value > 0
    }

    /// PhysicalBehaviour.WasBeingUsedContinuously(channel)
    pub fn was_used_continuously_on(&self, p: usize, channel: usize) -> bool {
        self.physicals[p].ContinuousActivationTracker[channel].PreviousValue > 0
    }

    /// PhysicalBehaviour.IsBeingUsedContinuously()
    pub fn used_continuously(&self, p: usize) -> bool {
        (0..CHANNELS).any(|c| self.used_continuously_on(p, c))
    }

    /// PhysicalBehaviour.StartedBeingUsedContinuously()
    pub fn started_used_continuously(&self, p: usize) -> bool {
        self.physicals[p].ContinuousActivationTracker.iter().any(|t| t.Value > 0 && t.PreviousValue == 0)
    }

    /// PhysicalBehaviour.StoppedBeingUsedContinuously()
    pub fn stopped_used_continuously(&self, p: usize) -> bool {
        self.physicals[p].ContinuousActivationTracker.iter().any(|t| t.Value == 0 && t.PreviousValue > 0)
    }

    /// Global.PhysicalObjectsInWorld
    pub(super) fn physicals_in_world(&self) -> Vec<usize> {
        (0..self.physicals.len()).filter(|&p| self.physicals[p].in_world && !self.physicals[p].destroyed).collect()
    }

    /// ContinuousActivationBehaviour.LateUpdate (execution order -10, before every other LateUpdate).
    pub fn continuous_activation_late_update(&mut self) {
        for p in self.physicals_in_world() {
            for t in self.physicals[p].ContinuousActivationTracker.iter_mut() {
                t.update_previous_value();
            }
        }
        // The loop condition calls CalculateCycleCount(Time.deltaTime) on every pass.
        let mut k = 0;
        while k < self.continuous.signals.calculate_cycle_count(self.time.deltaTime) {
            // UnityEvent.Invoke runs the list as it was when the call began.
            let live: Vec<Listener> = self.continuous.listeners.iter().copied().filter(|&l| !self.listener_removed(l)).collect();
            self.continuous.listeners = live.clone();
            for l in live {
                match l {
                    Listener::Drag => self.drag_continuous_update(),
                    Listener::KeyTrigger(k) => self.key_trigger_continuous_update(k),
                    Listener::Button(b) => self.button_continuous_update(b),
                    Listener::Detector(d) => self.detector_continuous_update(d),
                    Listener::LaserReceiver(r) => self.laser_receiver_continuous_update(r),
                    Listener::ActivationToggle(t) => self.activation_toggle_continuous_update(t),
                    Listener::UseWire(w) => self.use_wire_on_continuous_update(w),
                    Listener::ActivationTransformer(i) => self.activation_transformer_continuous_update(i),
                    Listener::Hold(p) => self.hold_continuous_update(p),
                    Listener::LiquidLevelGate(_) | Listener::ActivatorElectrode(_) => {}
                }
            }
            if self.continuous.oscillation % 2 == 0 {
                for p in self.physicals_in_world() {
                    for t in self.physicals[p].ContinuousActivationTracker.iter_mut() {
                        if t.PreviousValueFixed == t.Value {
                            t.Value = 0;
                        }
                        t.PreviousValueFixed = t.Value;
                    }
                }
            }
            self.continuous.oscillation = self.continuous.oscillation.wrapping_add(1);
            k += 1;
        }
        self.signals.cleanup(self.time.deltaTime);
    }

    /// The listener's OnDestroy ran (KeyTriggerBehaviour has none: key_trigger_continuous_update refuses a destroyed one).
    fn listener_removed(&self, l: Listener) -> bool {
        let gone = |id: i32| self.destroyed_components.contains(&id);
        match l {
            Listener::Drag | Listener::KeyTrigger(_) | Listener::Hold(_) => false,
            // ButtonBehaviour.OnDestroy: RemoveListener.
            // ActivationTransformerBehaviour.OnDestroy: RemoveListener.
            Listener::Button(b) => gone(self.buttons[b].id),
            Listener::Detector(d) => gone(self.detectors[d].id),
            Listener::LaserReceiver(r) => gone(self.laser_receivers[r].id),
            Listener::ActivationToggle(t) => gone(self.activation_toggles[t].id),
            Listener::UseWire(w) => gone(self.blood_wires[w].id),
            Listener::ActivationTransformer(i) => gone(self.activation_transformers[i].id),
            // LiquidLevelGateBehaviour.OnDestroy, ActivatorElectrodeBehaviour.OnDestroy: RemoveListener.
            Listener::LiquidLevelGate(i) => gone(self.liquid_level_gates[i].id),
            Listener::ActivatorElectrode(i) => gone(self.activator_electrodes[i].id),
        }
    }

    /// UnityEvent.AddListener on OnContinuousUpdate (the Awake of KeyTriggerBehaviour, LaserReceiverBehaviour,
    /// LiquidLevelGateBehaviour, ActivatorElectrodeBehaviour; link.rs makes the same push at link time).
    pub fn continuous_add_listener(&mut self, l: Listener) {
        self.continuous.listeners.push(l);
    }

    /// UnityEvent.RemoveListener on OnContinuousUpdate: every call of `l` goes (the OnDestroy of LaserReceiverBehaviour,
    /// LiquidLevelGateBehaviour, ActivatorElectrodeBehaviour, DetectorBehaviour).
    pub fn continuous_remove_listener(&mut self, l: Listener) {
        self.continuous.listeners.retain(|&x| x != l);
    }

    /// The OnContinuousUpdate calls of `l` that are live (fn-check `lis=`).
    pub fn continuous_listener_count(&self, l: Listener) -> usize {
        self.continuous.listeners.iter().filter(|&&x| x == l && !self.listener_removed(x)).count()
    }

    /// DragTool.OnDestroy: OnContinuousUpdate.RemoveListener(OnContinuousUpdate).
    pub fn drag_tool_on_destroy(&mut self) {
        self.continuous_remove_listener(Listener::Drag);
    }

    /// DragTool.OnContinuousUpdate: the ActivationPropagation(direct: true, 0) goes to the selected objects.
    pub(crate) fn drag_continuous_update(&mut self) {
        let Some(t) = self.tool.as_ref() else { return };
        if !t.shouldSendContinuousUseMessage {
            return;
        }
        for p in t.rawSelection.clone() {
            if self.physicals[p].destroyed {
                continue;
            }
            // new ActivationPropagation(direct: true, 0)
            let a = self.signal_direct(true, 0);
            self.activations_send_continuous(p);
            self.broadcast_use_continuous(p, &a);
        }
    }

    /// The recorder's hold listener: DragTool.OnContinuousUpdate's body on one object, `hold_left` times. The
    /// replayed item has no component instance: the root PhysicalBehaviour's tracker is the UseContinuous it takes
    /// (a firearm reads it in Update).
    fn hold_continuous_update(&mut self, p: usize) {
        if self.continuous.hold_left == 0 {
            return;
        }
        self.continuous.hold_left -= 1;
        if self.physicals[p].destroyed {
            return;
        }
        let a = self.signal_direct(true, 0);
        self.activations_send_continuous(p);
        // PhysicalBehaviour.UseContinuous: AssertState (true inside the LateUpdate), then the channel's tracker.
        let channel = self.sig(&a).Channel as usize;
        self.physicals[p].ContinuousActivationTracker[channel].increment(1);
    }

    /// BroadcastMessage("UseContinuous", activation) on a physical object's GameObject: it and its active
    /// descendants, each object's components in order.
    pub fn broadcast_use_continuous(&mut self, p: usize, act: &super::signal::Act) {
        let id = self.physicals[p].id;
        // A limb of the recording's Human (no clone kept): PhysicalBehaviour, then the use wires added to it.
        let receivers: Vec<Receiver> = if self.harness_limb(p).is_some() {
            std::iter::once(Receiver::Physical(p)).chain(self.use_wires_on(p).into_iter().map(Receiver::UseWire)).collect()
        } else { self.with_component_instance(id, |g, inst, start| {
            let mut out = Vec::new();
            for x in std::iter::once(start).chain(inst.descendants(start)) {
                if !inst.objects[x].active {
                    continue;
                }
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    let Component::Script { class, .. } = c else { continue };
                    // UseBubbler.UseContinuous: ContinuousActivationBehaviour.AssertState, then SendMessage to transform.parent (below).
                    // UseBubbler sends it to transform.parent: on the Truck, its only user, nothing there takes it.
                    if !USE_CONTINUOUS_RECEIVERS.contains(&class.as_str()) || class == "UseBubbler" {
                        continue;
                    }
                    let cid = inst.objects[x].components[k];
                    if class == "PhysicalBehaviour" {
                        let q = g.physicals.iter().position(|ph| ph.id == cid).expect("a PhysicalBehaviour in the arena");
                        out.push(Receiver::Physical(q));
                        continue;
                    }
                    if class == "UseWireBehaviour" {
                        // A tool's use wire (added at run time) is one of the `use_wires_on` below.
                        if inst.prefab.objects[x].components[k].0.file == usize::MAX {
                            continue;
                        }
                        unimplemented!("mod-only: a prefab's own UseWireBehaviour.UseContinuous");
                    }
                    match g.behaviour_of.get(&cid) {
                        Some(&(_, b @ (Behaviour::Transmitter(_) | Behaviour::RayGun(_) | Behaviour::SignalConverter(_) | Behaviour::Blaster(_) | Behaviour::BeamRepeater(_) | Behaviour::ActivatorElectrode(_)))) => out.push(Receiver::Behaviour(b)),
                        Some(&(_, b @ Behaviour::Translated(t))) if t.has_use_continuous() => out.push(Receiver::Behaviour(b)),
                        _ => unimplemented!("mod-only: {class}.UseContinuous"),
                    }
                }
                // The use wires added to the object come after its own components.
                let go = inst.objects[x].components[0];
                if let Some(q) = g.physicals.iter().position(|ph| ph.transform_id == go && !ph.destroyed) {
                    out.extend(g.use_wires_on(q).into_iter().map(Receiver::UseWire));
                }
            }
            out
        }) };
        for r in receivers {
            // Each receiver reads the shared signal as it is now (a use wire may have set a direct one's channel).
            let (direct, channel) = (self.sig(act).Direct, self.sig(act).Channel as usize);
            match r {
                Receiver::UseWire(w) => self.use_wire_receive(w, act, true),
                // PhysicalBehaviour.UseContinuous: AssertState (true inside the LateUpdate), then the tracker.
                Receiver::Physical(q) => self.physicals[q].ContinuousActivationTracker[channel].increment(1),
                Receiver::Behaviour(Behaviour::RayGun(i)) => self.ray_gun_use_continuous(i),
                Receiver::Behaviour(Behaviour::Blaster(i)) => self.blaster_use_continuous(i),
                Receiver::Behaviour(Behaviour::BeamRepeater(i)) => self.beam_repeater_use_continuous(i),
                Receiver::Behaviour(Behaviour::Transmitter(i)) => self.transmitter_use_continuous(i, act),
                // The drag tool's ActivationPropagation(direct: true, 0, ..).
                Receiver::Behaviour(Behaviour::SignalConverter(i)) => self.signal_converter_use_continuous(i, direct, channel as u16),
                Receiver::Behaviour(Behaviour::ActivatorElectrode(i)) => self.activator_electrode_use_continuous(i, act),
                Receiver::Behaviour(Behaviour::Translated(t)) => self.tr_use_continuous(t, super::translated::rt::Activation { Channel: channel as i32 }),
                Receiver::Behaviour(_) => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod delta_int_tests {
    use super::DeltaInt;

    #[test]
    fn decrement_stops_at_zero() {
        let mut d = DeltaInt::new(1);
        d.decrement();
        assert_eq!(d.Value, 0);
        d.decrement();
        assert_eq!(d.Value, 0);
        d.Value = -5;
        d.decrement();
        assert_eq!(d.Value, 0);
    }
}
