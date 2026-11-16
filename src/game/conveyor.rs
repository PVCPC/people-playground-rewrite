//! ConveyorBeltBehaviour: the wheel HingeJoint2Ds run their motors while Activated, at (Speed + Charge *
//! ChargeInfluence) * localScale.x. ConveyorBeltEffector.ActOnContacts has no caller, so the effector only sets a
//! direction nothing reads.
#![allow(non_snake_case)]

use super::{d, script, Game};

script! {
    Conveyor {
        Speed: f32 = 3.0,
        ChargeInfluence: f32 = 1.0,
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// WheelJoints: the `Game::hinges` entries.
        wheels: Vec<usize> = Vec::new(),
    }
}

impl Game {
    /// ConveyorBeltBehaviour.SetActivation: every wheel's useMotor = Activated.
    pub(crate) fn conveyor_set_activation(&mut self, c: usize) {
        let on = self.conveyors[c].Activated;
        for h in self.conveyors[c].wheels.clone() {
            if self.hinges[h].destroyed {
                crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
            }
            self.hinges[h].set_use_motor(&mut self.world, on);
        }
    }

    /// ConveyorBeltBehaviour.Start
    pub fn conveyor_start(&mut self, c: usize) {
        self.conveyor_set_activation(c);
    }

    /// ConveyorBeltBehaviour.Use (when enabled): toggles Activated.
    pub fn conveyor_use(&mut self, c: usize) {
        if self.conveyors[c].enabled {
            self.conveyors[c].Activated = !self.conveyors[c].Activated;
            self.conveyor_set_activation(c);
        }
    }

    // ConveyorBeltEffector.FixedUpdate: only sets worldDir, read by ActOnContacts, which nothing calls
    /// ConveyorBeltBehaviour.FixedUpdate: useMotor = Activated and the motor speed = FinalSpeed (0 when off) on each
    /// wheel, the torque kept.
    pub fn conveyor_fixed_update(&mut self, c: usize) {
        let on = self.conveyors[c].Activated;
        let speed = if on { self.conveyor_final_speed(c) } else { 0.0 };
        for h in self.conveyors[c].wheels.clone() {
            if self.hinges[h].destroyed {
                crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
            }
            self.hinges[h].set_use_motor(&mut self.world, on);
            let torque = self.hinges[h].motor[1];
            self.hinges[h].set_motor(&mut self.world, speed, torque);
        }
    }

    /// ConveyorBeltBehaviour.get_FinalSpeed: (Speed + PhysicalBehaviour.Charge * ChargeInfluence) * transform.localScale.x,
    /// in double.
    pub(crate) fn conveyor_final_speed(&self, c: usize) -> f32 {
        let s = &self.conveyors[c];
        ((d(s.Speed) + d(self.physicals[s.physical].Charge()) * s.ChargeInfluence) * self.xf_local_scale(&s.own)[0]).f()
    }

    /// ConveyorBeltBehaviour.OnDisable: Activated = false, then SetActivation.
    pub fn conveyor_on_disable(&mut self, c: usize) {
        self.conveyors[c].Activated = false;
        self.conveyor_set_activation(c);
    }
}
