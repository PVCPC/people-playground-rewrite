//! LiquidFilterBehaviour: a BloodContainer of Limits (0, 28) that does not overflow and allows transfer. It has no code of its
//! own: its Update is BloodContainer.Update.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::{script, Game};

script! {
    LiquidFilter {}
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// The PhysicalBehaviour on its GameObject.
        physical: usize = 0,
    }
}

/// LiquidFilterBehaviour.get_Limits: (0, 28).
pub fn liquid_filter_limits() -> [f32; 2] {
    [0.0, 28.0]
}

// LiquidFilterBehaviour.get_AllowsOverflow: false (`allows_overflow` lists the containers that overflow; this is not one).
// LiquidFilterBehaviour.get_AllowsTransfer: true (`wire_allows` is true for every container but the valves and the pump).
impl Game {
    /// LiquidFilterBehaviour.Update (BloodContainer.Update, inherited).
    pub fn liquid_filter_update(&mut self, f: usize) {
        self.blood_container_update(Owner::LiquidFilter(f));
    }
}
