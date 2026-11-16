//! SignalConverterBehaviour (the BG, BR and RG Signal Converters): Use and UseContinuous swap ChannelA and ChannelB of
//! a wired (not direct) activation and SendMessage it to the converter's own GameObject as IsolatedActivation or
//! IsolatedContinuousActivation. Only UseWireBehaviour has those methods: the use wires added to that object (link
//! refuses one in the prefab).
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    SignalConverter {
        ChannelA: u16 = 0,
        ChannelB: u16 = 0,
    }
    links {
        /// A UseWireBehaviour on the converter's GameObject (it would receive the isolated activation).
        wired: bool = false,
        /// The converter's GameObject's PhysicalBehaviour.
        physical: Option<usize> = None,
    }
}

impl SignalConverter {
    /// SignalConverterBehaviour.ConvertChannel: the outgoing channel, when the incoming one is converted.
    pub fn convert_channel(&self, direct: bool, channel: u16) -> Option<u16> {
        if direct {
            None
        } else if channel == self.ChannelA {
            Some(self.ChannelB)
        } else if channel == self.ChannelB {
            Some(self.ChannelA)
        } else {
            None
        }
    }
}

impl Game {
    /// SignalConverterBehaviour.Use
    pub fn signal_converter_use(&mut self, i: usize, direct: bool, channel: u16) {
        self.signal_converter_send(i, direct, channel, false);
    }

    /// SendMessage("IsolatedActivation"/"IsolatedContinuousActivation", new ActivationPropagation(transform.root, outgoing)).
    fn signal_converter_send(&mut self, i: usize, direct: bool, channel: u16, continuous: bool) {
        let Some(outgoing) = self.signal_converters[i].convert_channel(direct, channel) else { return };
        if self.signal_converters[i].wired {
            unimplemented!("mod-only: SignalConverterBehaviour.IsolatedActivation received by a prefab's UseWireBehaviour");
        }
        let p = self.signal_converters[i].physical.unwrap_or_else(|| unimplemented!("mod-only: SignalConverterBehaviour on an object without a PhysicalBehaviour"));
        let root = self.root_id(p);
        let a = self.signal_from(root, outgoing);
        for w in self.use_wires_on(p) {
            self.use_wire_receive(w, &a, continuous);
        }
    }

    /// SignalConverterBehaviour.UseContinuous (ContinuousActivationBehaviour.AssertState holds inside its LateUpdate).
    pub fn signal_converter_use_continuous(&mut self, i: usize, direct: bool, channel: u16) {
        self.signal_converter_send(i, direct, channel, true);
    }
}
