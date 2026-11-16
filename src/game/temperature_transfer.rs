//! TemperatureTransferBehaviour (the Ceiling Turret and the Reactor A5). Awake caches the object's PhysicalBehaviour;
//! FixedUpdate, while both physical objects simulate temperature, moves each one's temperature towards the other's by
//! GuideSpeed (both new values come from the temperatures read before either write).
#![allow(non_snake_case)]

use super::{mathf, script, Game};

script! {
    TemperatureTransfer {
        GuideSpeed: f32 = 0.001,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// phys: GetComponent<PhysicalBehaviour>() (Awake).
        phys: usize = 0,
        /// Other: the serialized PhysicalBehaviour.
        Other: usize = 0,
        /// phys (no PhysicalBehaviour on the object) or Other is null.
        phys_null: bool = false,
        other_null: bool = false,
    }
}

impl Game {
    /// TemperatureTransferBehaviour.FixedUpdate.
    pub fn temperature_transfer_fixed_update(&mut self, i: usize) {
        let (p, other, guide) = (self.temperature_transfers[i].phys, self.temperature_transfers[i].Other, self.temperature_transfers[i].GuideSpeed);
        if self.temperature_transfers[i].phys_null {
            crate::game::menu::npe();
        }
        if !self.physicals[p].SimulateTemperature {
            return;
        }
        if self.temperature_transfers[i].other_null {
            crate::game::menu::npe();
        }
        // `Other.SimulateTemperature` is read only when the first check passed; a destroyed Other throws there.
        if self.physicals[other].destroyed {
            crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
        }
        if self.physicals[other].SimulateTemperature {
            let (temperature, temperature2) = (self.physicals[p].Temperature, self.physicals[other].Temperature);
            self.physicals[p].Temperature = mathf::lerp(temperature, temperature2, guide);
            self.physicals[other].Temperature = mathf::lerp(temperature2, temperature, guide);
        }
    }
}
