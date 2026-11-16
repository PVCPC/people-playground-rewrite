//! Activations.NodeBehaviour (the node a wire can hang its targets on) and NewUseWireBehaviour, the wire that registers with
//! it. In this build nothing invokes a node's OnUseStart or sets its CurrentlyActive (NodeBehaviour.Use and the
//! serialisation hooks are empty), so a NewUseWire only records itself as a listener and target, and its glow is
//! presentation (a MaterialPropertyBlock on its LineRenderer).
#![allow(non_snake_case)]

use super::{script, BodyOwner, Game};

/// NodeBehaviour.MaxSignalsPerFrame (its static setup: 512). Public in 1.28.41, but nothing in the game reads it.
pub(crate) const MAX_SIGNALS_PER_FRAME: i32 = 512;

/// ActivationTarget(to, Channels) a wire added to a node's Targets; `wire` is the `Game::blood_wires` index that owns it
/// (List.Remove compares references, so each entry is that wire's own).
#[derive(Default, Clone, Debug)]
pub struct NodeTarget {
    pub node: usize,
    pub channels: u16,
    pub wire: usize,
}

impl NodeTarget {
    /// ActivationTarget..ctor(node, channelMask): Node and Channels (`wire` is which wire's own reference it is).
    pub(crate) fn new(node: usize, channel_mask: u16, wire: usize) -> Self {
        NodeTarget { node, channels: channel_mask, wire }
    }
}

impl NodeTarget {
    /// ActivationTarget.ToString: $"To {Node} through channels {Channels}" (`node` is Object.ToString of the node: "name (NodeBehaviour)").
    #[allow(dead_code)]
    pub fn to_string(&self, node: &str) -> String {
        format!("To {node} through channels {}", channel_to_string(self.channels as u8))
    }
}

/// Channel.ToString ([Flags] byte enum None 0, Green 1, Red 2, Blue 4, All 255): a defined value by name, a combination of
/// names ascending joined with ", ", and a value with undefined bits as its number.
fn channel_to_string(c: u8) -> String {
    match c {
        0 => return "None".into(),
        255 => return "All".into(),
        _ => {}
    }
    let names: Vec<&str> = [(1u8, "Green"), (2, "Red"), (4, "Blue")].iter().filter(|(b, _)| c & b != 0).map(|&(_, n)| n).collect();
    if c & !7 != 0 { c.to_string() } else { names.join(", ") }
}

/// Activations.ActivationEventType
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ActivationEventType {
    #[default]
    Start = 0,
    Stop = 1,
}

/// Activations.ActivationSource
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ActivationSource {
    #[default]
    Unknown = 0,
    Player = 1,
    Node = 2,
}

/// Activations.ActivationEvent
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct ActivationEvent {
    pub Depth: i32,
    pub Channels: u8,
    pub EventType: ActivationEventType,
    pub Source: ActivationSource,
}

impl ActivationEvent {
    /// ActivationEvent..ctor(depth, channels, eventType, source = ActivationSource.Node)
    #[allow(dead_code)]
    pub fn new(depth: i32, channels: u8, event_type: ActivationEventType, source: ActivationSource) -> Self {
        ActivationEvent { Depth: depth, Channels: channels, EventType: event_type, Source: source }
    }
}

/// Activations.ScheduledActivation
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct ScheduledActivation {
    pub TimeRemaining: f32,
    pub Activation: ActivationEvent,
}

impl ScheduledActivation {
    /// ScheduledActivation..ctor(timeRemaining, activation)
    #[allow(dead_code)]
    pub fn new(time_remaining: f32, activation: ActivationEvent) -> Self {
        ScheduledActivation { TimeRemaining: time_remaining, Activation: activation }
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    #[test]
    fn target_to_string_names_channels() {
        let t = |c| NodeTarget { node: 0, channels: c, wire: 0 }.to_string("N (NodeBehaviour)");
        assert_eq!(t(255), "To N (NodeBehaviour) through channels All");
        assert_eq!(t(3), "To N (NodeBehaviour) through channels Green, Red");
        assert_eq!(t(0), "To N (NodeBehaviour) through channels None");
        assert_eq!(t(9), "To N (NodeBehaviour) through channels 9");
    }

    #[test]
    fn scheduled_activation_holds_its_event() {
        let e = ActivationEvent::new(2, 4, ActivationEventType::Stop, ActivationSource::Node);
        let s = ScheduledActivation::new(0.5, e);
        assert_eq!((s.TimeRemaining, s.Activation.Depth, s.Activation.Channels), (0.5, 2, 4));
    }
}

script! {
    ActivationNode {
        DelaySeconds: f32 = 0.1,
        Started: u8 = 0,
        Ended: u8 = 0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// The Rigidbody2D on the node's GameObject (what `connectedBody.TryGetComponent` finds it through).
        owner: Option<BodyOwner> = None,
        Targets: Vec<NodeTarget> = Vec::new(),
        /// OnUseStart's listeners: NewUseWireBehaviours (`Game::blood_wires` indices).
        OnUseStart: Vec<usize> = Vec::new(),
        CurrentlyActive: u8 = 0,
    }
}

// NodeBehaviour.Use: empty.
// NodeBehaviour.OnBeforeSerialise: empty.
// NodeBehaviour.OnAfterDeserialise: empty.
// NewUseWireBehaviour.OnUseStart: new_use_wire_on_use_start (nothing invokes a node's OnUseStart in this build)
// NewUseWireBehaviour.SendGlow: use_wire.rs use_wire_send_glow (_Timestamp into the line's property block, wires.rs draws it)
impl Game {
    /// NewUseWireBehaviour.Start after DistanceJointWireBehaviour.Start: the break force, then the node on the wire's own
    /// GameObject (`from`) and the one on the connected body's (`to`). A wire with a `from` but no connected body throws
    /// at `typedJoint.connectedBody.TryGetComponent`, ending Start before it listens. The property block, widths and
    /// glow colour are the line's look.
    pub(crate) fn new_use_wire_start(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        let buffer = if self.blood_wires[w].CanBreak { self.fixed_joints[f].state.break_force } else { f32::INFINITY };
        self.blood_wires[w].breakForceBuffer = buffer;
        self.fixed_joint_set_break_force(f, buffer);
        let own = BodyOwner::Physical(self.blood_wires[w].physical);
        let Some(from) = self.activation_node_of(own) else { return };
        self.blood_wires[w].from = Some(from);
        if !self.wire_connected_alive(f) {
            return;
        }
        let connected = self.fixed_joints[f].connected;
        if let Some(to) = connected.and_then(|c| self.activation_node_of(c)) {
            if to == from {
                return;
            }
            self.blood_wires[w].to = Some(to);
            let channels = self.blood_wires[w].Channel;
            self.activation_nodes[from].Targets.push(NodeTarget::new(to, channels, w));
        }
        self.activation_nodes[from].OnUseStart.push(w);
    }

    /// `TryGetComponent<NodeBehaviour>` on `own`'s GameObject: a prefab's node, or the one PhysicalBehaviour.Awake added.
    pub(crate) fn activation_node_of(&self, own: BodyOwner) -> Option<usize> {
        self.activation_nodes.iter().position(|n| n.owner == Some(own) && !self.destroyed_components.contains(&n.id))
    }

    /// NewUseWireBehaviour.Tick: base.Tick() (blood_wire_fixed_update's currentDistance), then the glow while `from.CurrentlyActive`
    /// meets Channels, then the burn while CanBreak. Without a `from` the glow test throws (NRE) and the burn is skipped.
    pub(crate) fn new_use_wire_tick(&mut self, w: usize) {
        let Some(from) = self.blood_wires[w].from else { crate::game::menu::npe() };
        if self.activation_nodes[from].CurrentlyActive as u16 & self.blood_wires[w].Channel != 0 {
            self.use_wire_send_glow(w);
        }
        if self.blood_wires[w].CanBreak {
            self.wire_burn(w);
        }
    }

    /// NewUseWireBehaviour.OnUseStart(c): SendGlow when c meets Channels.
    #[allow(dead_code)] // the listener of NodeBehaviour.OnUseStart, which nothing in the game invokes
    pub(super) fn new_use_wire_on_use_start(&mut self, w: usize, c: u16) {
        if c & self.blood_wires[w].Channel != 0 {
            self.use_wire_send_glow(w);
        }
    }

    /// NewUseWireBehaviour.OnDestroy after WireBehaviour.OnDestroy: a live `from` loses the OnUseStart listener, and a live `to`
    /// the target.
    pub(crate) fn new_use_wire_on_destroy(&mut self, w: usize) {
        let alive = |g: &Game, n: usize| !g.destroyed_components.contains(&g.activation_nodes[n].id);
        let Some(from) = self.blood_wires[w].from.filter(|&n| alive(self, n)) else { return };
        self.activation_nodes[from].OnUseStart.retain(|&x| x != w);
        if let Some(to) = self.blood_wires[w].to.filter(|&n| alive(self, n)) {
            let t = &mut self.activation_nodes[from].Targets;
            if let Some(at) = t.iter().position(|x| x.wire == w && x.node == to) {
                t.remove(at);
            }
        }
    }
}
