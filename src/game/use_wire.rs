//! UseWireBehaviour, the propagation wire the six use wire tools lay (a DistanceJointWireBehaviour, blood_wire.rs).
//! Use and UseContinuous reach it like any receiver on its GameObject, as do IsolatedActivation and
//! IsolatedContinuousActivation, which only it receives. A signal on its channel (a direct one takes its channel)
//! goes on as a branch through its root after the physical's ActivationPropagationDelay: Use is broadcast to the
//! connected body, UseContinuous waits in toSendOut for the next continuous step. The glow (_GlowColour, _Timestamp)
//! is rendering.
#![allow(non_snake_case)]

use super::coroutine::Yield;
use super::signal::{Act, MAXIMUM_PROPAGATIONS};
use super::blood_wire::WireKind;
use super::Game;

/// Resources.LoadAll<AudioClip>("Audio/use wire break")
pub const USE_WIRE_BREAK_CLIPS: usize = 3;

// DestructibleRedUseWireTool.GetWireColor: the red row.
// DestructibleBlueUseWireTool.GetWireColor: the blue row.
/// UseWireTool.GetWireColor, RedUseWireTool.GetWireColor and BlueUseWireTool.GetWireColor (the Destructible tools inherit them).
/// DestructibleRedUseWireTool.GetWireColor and DestructibleBlueUseWireTool.GetWireColor are the same rows.
pub fn wire_color(channel: u16) -> [f32; 4] {
    match channel {
        1 => [11.0, 1.0, 0.0, 1.0],
        2 => [0.0, 1.0, 11.0, 1.0],
        _ => [1.0, 11.0, 0.0, 1.0],
    }
}

impl Game {
    /// UseWireBehaviour.Start after base.Start: breakForceBuffer is the joint's force if CanBreak (else infinite) and
    /// goes back on the joint, the continuous listener, and target: the connected body's PhysicalBehaviour.
    pub(crate) fn use_wire_start(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        let buffer = if self.blood_wires[w].CanBreak { self.fixed_joints[f].state.break_force } else { f32::INFINITY };
        self.blood_wires[w].breakForceBuffer = buffer;
        self.fixed_joint_set_break_force(f, buffer);
        self.continuous.listeners.push(super::continuous::Listener::UseWire(w));
        if self.wire_connected_alive(f) {
            self.blood_wires[w].target = match self.fixed_joints[f].connected {
                Some(super::BodyOwner::Physical(q)) => Some(q),
                // Only PhysicalBehaviour implements IUseEmitter: a bare Rigidbody2D's object has none.
                _ => None,
            };
        }
    }

    /// The live UseWireBehaviours added to physical `p`'s GameObject, in AddComponent order: the last of its components.
    pub(super) fn use_wires_on(&self, p: usize) -> Vec<usize> {
        (0..self.blood_wires.len()).filter(|&w| {
            let x = &self.blood_wires[w];
            matches!(x.kind, WireKind::Use(..)) && x.physical == p && !self.destroyed_components.contains(&x.id)
        }).collect()
    }

    /// Utils.SendAllChannelIsolatedActivation, Utils.SendAllChannelContinuousIsolatedActivation, UseWireBehaviour.IsolatedActivation, UseWireBehaviour.IsolatedContinuousActivation: SendMessage with a new ActivationPropagation(transform.root, channel)
    /// on physical `p`'s GameObject for each channel (Utils.SendAllChannel(Continuous)IsolatedActivation; the pause
    /// test of the continuous one never holds). Only a UseWireBehaviour receives them; IsolatedActivation passes the
    /// signal to Use, since its Path is never null.
    pub(crate) fn send_isolated(&mut self, p: usize, continuous: bool) {
        for channel in 0..3 {
            let root = self.root_id(p);
            let a = self.signal_from(root, channel);
            for w in self.use_wires_on(p) {
                self.use_wire_receive(w, &a, continuous);
            }
        }
    }

    /// UseWireBehaviour.Use, UseWireBehaviour.UseContinuous and UseWireBehaviour.ValidateSignal (ContinuousActivationBehaviour.AssertState only reads).
    pub(super) fn use_wire_receive(&mut self, w: usize, a: &Act, continuous: bool) {
        if self.signals.currently_active() >= MAXIMUM_PROPAGATIONS {
            return self.too_many_propagations_warning();
        }
        let p = self.blood_wires[w].physical;
        if self.physicals[p].ActivationPropagationDelay < 0.0 {
            return;
        }
        // ValidateSignal: a direct signal takes the wire's channel, in place.
        let channel = self.blood_wires[w].Channel;
        if self.sig(a).Direct {
            self.sig_mut(a).Channel = channel;
        } else if self.sig(a).Channel != channel {
            return;
        }
        if !self.physicals[p].SendUserPropagation && self.sig(a).Direct {
            return;
        }
        if self.sig(a).TraversedCount > MAXIMUM_PROPAGATIONS as i32 {
            return self.too_many_propagations_warning();
        }
        let root = self.root_id(p);
        let b = self.signal_branch(a, root);
        self.use_wire_send(w, b, continuous);
    }

    /// UseWireBehaviour.SendGlow: _Timestamp = Time.timeSinceLevelLoad (Time.time) into the line's property block.
    pub fn use_wire_send_glow(&mut self, w: usize) {
        self.blood_wires[w].glow_timestamp = Some(self.time.time);
    }

    /// UseWireBehaviour.SendUseMessage and UseWireBehaviour.SendUseContinuousMessage, as StartCoroutine'd: ActivityIntensity++, WaitForSeconds(delay), then
    /// (AssertProperState) the Use broadcast to the connected body and the target's OnSingleUse, or the queue.
    fn use_wire_send(&mut self, w: usize, b: Act, continuous: bool) {
        let (id, p) = (self.blood_wires[w].id, self.blood_wires[w].physical);
        let routine = self.use_wire_send_routine(w, b, continuous);
        let go = self.physicals[p].id;
        self.start_coroutine(id, go, routine);
    }

    /// The routine of UseWireBehaviour.SendUseMessage / SendUseContinuousMessage (not yet started).
    pub(crate) fn use_wire_send_routine(&self, w: usize, b: Act, continuous: bool) -> super::coroutine::Routine {
        let p = self.blood_wires[w].physical;
        let mut step = 0;
        Box::new(move |g: &mut Game| {
            step += 1;
            match step {
                1 => {
                    g.signal_touch(&b);
                    Yield::WaitForSeconds(g.physicals[p].ActivationPropagationDelay)
                }
                2 if g.use_wire_proper_state(w, &b) => {
                    if continuous {
                        g.blood_wires[w].toSendOut.push_back(b.clone());
                        return Yield::Done;
                    }
                    let q = g.use_wire_connected(w);
                    g.sig_mut(&b).Target = Some(q);
                    g.broadcast_use(q, &b);
                    // target?.OnSingleUse.Invoke: the ActivationToggleBehaviour listeners.
                    if let Some(t) = g.blood_wires[w].target {
                        g.activation_toggle_use_direct(t);
                    }
                    g.use_wire_send_glow(w);
                    Yield::WaitForEndOfFrame
                }
                _ => Yield::Done,
            }
        })
    }

    /// The connected body's physical.
    fn use_wire_connected(&self, w: usize) -> usize {
        match self.fixed_joints[self.blood_wires[w].joint].connected {
            Some(super::BodyOwner::Physical(q)) => q,
            _ => unimplemented!("mod-only: UseWireBehaviour: BroadcastMessage on a node's Rigidbody2D"),
        }
    }

    /// UseWireBehaviour.AssertProperState: the connected body is there and the signal has not been through its root.
    pub(crate) fn use_wire_proper_state(&self, w: usize, a: &Act) -> bool {
        if !self.wire_connected_alive(self.blood_wires[w].joint) {
            return false;
        }
        let q = self.use_wire_connected(w);
        !self.signal_contains(a, self.root_id(q))
    }

    /// UseWireBehaviour.OnContinuousUpdate: the queue goes out (target?.OnContinuousUse has no listener).
    pub(crate) fn use_wire_on_continuous_update(&mut self, w: usize) {
        while let Some(a) = self.blood_wires[w].toSendOut.pop_front() {
            if self.use_wire_proper_state(w, &a) {
                let q = self.use_wire_connected(w);
                self.sig_mut(&a).Target = Some(q);
                self.broadcast_use_continuous(q, &a);
            }
        }
    }

    /// UseWireBehaviour.ShowTooManyPropagationsWarning (static lastWarningTime, LAST_WARNING_TIME until the first warning; Time.timeSinceLevelLoad is Time.time
    /// here, the same differences within one level): UISoundBehaviour.Main.Warning() past 10 s, then the notification.
    pub(crate) fn too_many_propagations_warning(&mut self) {
        let now = self.time.time;
        let last = self.signals.last_warning.unwrap_or(LAST_WARNING_TIME);
        if now - last > 1.0 {
            if now - last > 10.0 {
                self.ui_sound(super::audio::UiSoundKind::Warning);
            }
            self.notify(crate::il!(str "UseWireBehaviour::ShowTooManyPropagationsWarning" 0).into());
            self.signals.last_warning = Some(now);
        }
    }
}

/// UseWireBehaviour.lastWarningTime's initialiser (static).
pub const LAST_WARNING_TIME: f32 = -1000.0;
