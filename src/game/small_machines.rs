//! Small machines: AerialFaithPlateBehaviour (the launch pad), SirenBehaviour, ResistorBehaviour,
//! GuideTemperatureBehaviour, ElectricityTransformerBehaviour and DisableWhenWet. Their sounds are presentation; a
//! missing reference still throws where the game would touch it.
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::translated::rt::{self, Activation, Xf};
use super::{d, mathf, script, Game};

script! {
    AerialFaithPlateBehaviour {
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        launchJoint: Option<usize> = None,
        phys: Option<usize> = None,
        audio: bool = false,
        isBusy: bool = false,
    }
}

script! {
    SirenBehaviour {
        Activated: bool = false,
        RestartDuration: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        AudioSource: bool = false,
        SirenSound: bool = false,
        BrokenSirenSound: bool = false,
        DamagableMachineryBehaviour: Option<usize> = None,
        time: f32 = 0.0,
    }
}

script! {
    ResistorBehaviour {
        ResistorPower: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        HandleJoint: Option<usize> = None,
        PhysicalBehaviour: Option<usize> = None,
        flipped: bool = false,
    }
}

script! {
    GuideTemperatureBehaviour {
        TargetTemperature: f32 = 0.0,
        GuideSpeed: f32 = 0.0010000000474974513,
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        phys: Option<usize> = None,
    }
}

script! {
    ElectricityTransformerBehaviour {
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        PhysicalBehaviour: Option<usize> = None,
    }
}

script! {
    DisableWhenWet {
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        PhysicalBehaviour: Option<usize> = None,
        ToDisable: Option<rt::Go> = None,
    }
}

/// Where AerialFaithPlateBehaviour.Launch is between its yields.
#[derive(Clone, Copy)]
enum Launch {
    Begin,
    /// After the 2 s wait: the motor goes off.
    Swung,
    /// After the 1 s wait: ready again.
    Settled,
    Done,
}

impl Game {
    // ---- AerialFaithPlateBehaviour ----

    /// AerialFaithPlateBehaviour.Start
    pub fn aerial_faith_plate_behaviour_start(&mut self, a: usize) {
        self.tr.aerial_faith_plate_behaviour[a].isBusy = false;
    }

    /// AerialFaithPlateBehaviour.Use (when enabled): StartCoroutine(Launch()).
    pub fn aerial_faith_plate_behaviour_use(&mut self, a: usize, _activation: Activation) {
        if self.tr.aerial_faith_plate_behaviour[a].enabled {
            let routine = self.aerial_faith_plate_behaviour_launch(a);
            let id = self.tr.aerial_faith_plate_behaviour[a].this_id;
            self.start_coroutine(id, id, routine);
        }
    }

    /// AerialFaithPlateBehaviour.Launch: unless busy, the launch joint's motor swings the plate at
    /// (-800 - Charge * 20) * root.localScale.x for 2 s, then is off for 1 s before the plate is free again.
    pub fn aerial_faith_plate_behaviour_launch(&mut self, a: usize) -> Routine {
        let mut at = Launch::Begin;
        Box::new(move |g: &mut Game| {
            let s = &g.tr.aerial_faith_plate_behaviour[a];
            match at {
                Launch::Begin if s.isBusy => Yield::Done,
                Launch::Begin => {
                    g.tr.aerial_faith_plate_behaviour[a].isBusy = true;
                    let h = g.tr_hinge(g.tr.aerial_faith_plate_behaviour[a].launchJoint);
                    g.hinges[h].set_use_motor(&mut g.world, true);
                    // audio.Play()
                    rt::pres(g.tr.aerial_faith_plate_behaviour[a].audio);
                    let src = g.audio_src(g.tr.aerial_faith_plate_behaviour[a].this_id, "audio");
                    g.audio.play(src);
                    let s = &g.tr.aerial_faith_plate_behaviour[a];
                    let charge = g.physicals[rt::nn(s.phys)].Charge();
                    let root = g.tr_root(Xf::this(s.this_physical, s.this_physics));
                    let speed = ((-800.0f32 - d(charge) * 20.0f32) * g.tr_local_scale(root)[0]).f();
                    g.aerial_faith_plate_behaviour_set_motor_speed(a, speed);
                    at = Launch::Swung;
                    Yield::WaitForSeconds(2.0)
                }
                Launch::Swung => {
                    let h = g.tr_hinge(g.tr.aerial_faith_plate_behaviour[a].launchJoint);
                    g.hinges[h].set_use_motor(&mut g.world, false);
                    at = Launch::Settled;
                    Yield::WaitForSeconds(1.0)
                }
                Launch::Settled => {
                    g.tr.aerial_faith_plate_behaviour[a].isBusy = false;
                    at = Launch::Done;
                    Yield::Done
                }
                Launch::Done => Yield::Done,
            }
        })
    }

    /// AerialFaithPlateBehaviour.SetMotorSpeed: the launch joint's motor speed, its torque kept.
    pub fn aerial_faith_plate_behaviour_set_motor_speed(&mut self, a: usize, speed: f32) {
        let h = self.tr_hinge(self.tr.aerial_faith_plate_behaviour[a].launchJoint);
        let torque = self.hinges[h].motor[1];
        self.hinges[h].set_motor(&mut self.world, speed, torque);
    }

    // ---- SirenBehaviour ----

    /// SirenBehaviour.Use
    pub fn siren_behaviour_use(&mut self, s: usize, _activation: Activation) {
        self.tr.siren_behaviour[s].Activated = !self.tr.siren_behaviour[s].Activated;
    }

    /// SirenBehaviour.Update: while on, every RestartDuration the siren (or the broken siren, once the machine is
    /// destroyed) plays again.
    pub fn siren_behaviour_update(&mut self, s: usize) {
        if !self.tr.siren_behaviour[s].Activated {
            return;
        }
        let siren = &mut self.tr.siren_behaviour[s];
        siren.time += self.time.deltaTime;
        if siren.time > siren.RestartDuration {
            siren.time = 0.0;
            // AudioSource.PlayOneShot(machine.Destroyed ? BrokenSirenSound : SirenSound)
            let _broken = self.machinery[rt::nn(siren.DamagableMachineryBehaviour)].Destroyed;
            rt::pres(self.tr.siren_behaviour[s].AudioSource);
        }
    }

    // ---- ResistorBehaviour ----

    /// ResistorBehaviour.Start
    pub fn resistor_behaviour_start(&mut self, r: usize) {
        let s = &self.tr.resistor_behaviour[r];
        let flipped = self.tr_lossy_scale(Xf::this(s.this_physical, s.this_physics))[0] < 0.0;
        self.tr.resistor_behaviour[r].flipped = flipped;
    }

    /// ResistorBehaviour.FixedUpdate: the handle's place between its limits sets the wire resistance (reversed when not
    /// flipped).
    pub fn resistor_behaviour_fixed_update(&mut self, r: usize) {
        let h = self.tr_hinge(self.tr.resistor_behaviour[r].HandleJoint);
        let [min, max] = self.hinges[h].limits;
        let along = self.hinges[h].joint_angle(&self.world) - min;
        let range = min.abs() + max;
        let t = mathf::clamp01(along / range);
        let s = &mut self.tr.resistor_behaviour[r];
        s.ResistorPower = if s.flipped { t } else { 1.0 - t };
        let power = s.ResistorPower;
        self.physicals[rt::nn(s.PhysicalBehaviour)].EnergyWireResistance = power;
    }

    /// ResistorBehaviour.OnDisable
    pub fn resistor_behaviour_on_disable(&mut self, r: usize) {
        self.physicals[rt::nn(self.tr.resistor_behaviour[r].PhysicalBehaviour)].EnergyWireResistance = 1.0;
    }

    // ---- GuideTemperatureBehaviour ----

    /// GuideTemperatureBehaviour.Awake
    pub fn guide_temperature_behaviour_awake(&mut self, g: usize) {
        self.tr.guide_temperature_behaviour[g].phys = self.tr.guide_temperature_behaviour[g].this_physical;
    }

    /// GuideTemperatureBehaviour.FixedUpdate: Temperature lerps toward TargetTemperature by GuideSpeed.
    pub fn guide_temperature_behaviour_fixed_update(&mut self, g: usize) {
        let s = &self.tr.guide_temperature_behaviour[g];
        let p = rt::nn(s.phys);
        self.physicals[p].Temperature = mathf::lerp(self.physicals[p].Temperature, s.TargetTemperature, s.GuideSpeed);
    }

    // ---- ElectricityTransformerBehaviour ----

    /// ElectricityTransformerBehaviour.UseContinuous (when enabled): charge += 5.
    pub fn electricity_transformer_behaviour_use_continuous(&mut self, e: usize, _activation: Activation) {
        if self.tr.electricity_transformer_behaviour[e].enabled {
            self.physicals[rt::nn(self.tr.electricity_transformer_behaviour[e].PhysicalBehaviour)].charge += 5.0;
        }
    }

    /// ElectricityTransformerBehaviour.Use (when enabled): charge += 50.
    pub fn electricity_transformer_behaviour_use(&mut self, e: usize, _activation: Activation) {
        if self.tr.electricity_transformer_behaviour[e].enabled {
            self.physicals[rt::nn(self.tr.electricity_transformer_behaviour[e].PhysicalBehaviour)].charge += 50.0;
        }
    }

    // ---- DisableWhenWet ----

    /// DisableWhenWet.Update: ToDisable goes inactive while the object is wet and comes back once it is dry.
    pub fn disable_when_wet_update(&mut self, w: usize) {
        let s = &self.tr.disable_when_wet[w];
        let target = rt::nn(s.ToDisable);
        if self.physicals[rt::nn(s.PhysicalBehaviour)].Wetness > 0.0 && self.tr_active_self(target) {
            self.tr_set_active(target, false);
        } else if !self.tr_active_self(target) {
            self.tr_set_active(target, true);
        }
    }
}
