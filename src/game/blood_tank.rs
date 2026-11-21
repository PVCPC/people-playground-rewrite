//! BloodTankBehaviour (a BloodContainer): a Use steps Mode (Push, Pull, Idle, Drain; channel 1 steps back). Its container
//! follows the mode's pressure. In Drain it drains at DrainRate and feeds its FancyBloodSplatController (PhysicalParticles),
//! whose particles copy blood into the containers they hit (OnParticleCollide). The splat's spawning, ejection and decal
//! colour are simulated, and the liquid bar (its LiquidContainerController); the particle systems follow the mode
//! (liquid_spray_particles); the context menu is presentation; the mode display is a world text.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::flask::BLOOD_CONTAINERS;
use super::{mathf, script, Game};

/// TankMode values: Push 0, Pull 1, Idle 2, Drain 3.
const DRAIN: i32 = 3;

script! {
    BloodTank {
        DrainRate: f32 = 1.0,
        /// Mode (TankMode): Push 0, Pull 1, Idle 2, Drain 3.
        Mode: i32 = 2,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>(): the physical that TryGetComponent<BloodContainer> finds this container by.
        physical: usize = 0,
        /// Awake's `phys = GetComponent<PhysicalBehaviour>()` (the C# reference cache, `physical`'s component once Awake ran).
        phys: Option<usize> = None,
        /// LiquidContainer (`Game::liquid_containers`), or null.
        LiquidContainer: Option<usize> = None,
        /// PhysicalParticles: the `Game::splats` index of its FancyBloodSplatController.
        splat: usize = 0,
        /// WaterPuffParticles' Transform id: its position decides whether the particles are under water.
        water_puff: i32 = 0,
        /// ModeDisplay (world_text.rs).
        ModeDisplay: Option<usize> = None,
    }
}

impl Game {
    /// BloodTankBehaviour.Start: the four mode buttons, then UpdateActivation.
    pub fn blood_tank_start(&mut self, t: usize) {
        use super::menu::{Cond, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.blood_tanks[t].physical].menu;
        for (m, id, label, desc) in [(0, "setToPush", crate::il!(str "BloodTankBehaviour::Start" 1), crate::il!(str "BloodTankBehaviour::Start" 2)), (1, "setToPull", crate::il!(str "BloodTankBehaviour::Start" 4), crate::il!(str "BloodTankBehaviour::Start" 5)),
            (2, "setToIdle", crate::il!(str "BloodTankBehaviour::Start" 7), crate::il!(str "BloodTankBehaviour::Start" 8)), (3, "setToDrain", crate::il!(str "BloodTankBehaviour::Start" 10), crate::il!(str "BloodTankBehaviour::Start" 11))] {
            menu.push(B { condition: Cond::BloodTankNot(t, m), ..B::new(id, label, desc, A::BloodTankMode(t, m)) });
        }
        self.blood_tank_update_activation(t);
    }

    /// BloodTankBehaviour.UpdateActivation: in Drain WaterPuffParticles (under water) or BloodDrainParticles plays; outside
    /// it both stop and the splat's spawning chance is 0. ModeDisplay shows Mode.ToString().ToUpper().
    pub(crate) fn blood_tank_update_activation(&mut self, t: usize) {
        let id = self.blood_tanks[t].id;
        if self.blood_tanks[t].Mode == DRAIN {
            let submerged = self.water_puff_submerged(self.blood_tanks[t].water_puff);
            self.ps_field_play_stop(id, if submerged { "WaterPuffParticles" } else { "BloodDrainParticles" }, true);
        } else {
            self.ps_field_play_stop(id, "BloodDrainParticles", false);
            self.ps_field_play_stop(id, "WaterPuffParticles", false);
            let s = self.blood_tanks[t].splat;
            self.splats[s].SpawningChance = 0.0;
        }
        let name = match self.blood_tanks[t].Mode {
            0 => "PUSH".to_string(),
            1 => "PULL".to_string(),
            2 => "IDLE".to_string(),
            3 => "DRAIN".to_string(),
            m => m.to_string(),
        };
        let display = self.blood_tanks[t].ModeDisplay;
        self.text_write(display, &name);
    }

    /// BloodTankBehaviour.Use: channel 1 steps the mode back, any other channel forward (wrapping through the four modes).
    pub fn blood_tank_use(&mut self, t: usize, channel: i32) {
        if channel != 1 {
            self.blood_tank_next_mode(t);
        } else {
            self.blood_tank_previous_mode(t);
        }
        self.blood_tank_update_activation(t);
    }

    /// BloodTankBehaviour.NextMode: Mode + 1, past Drain back to Push, then UpdateActivation.
    pub fn blood_tank_next_mode(&mut self, t: usize) {
        let mode = self.blood_tanks[t].Mode + 1;
        self.blood_tanks[t].Mode = if mode > 3 { 0 } else { mode };
        self.blood_tank_update_activation(t);
    }

    /// BloodTankBehaviour.PreviousMode: Mode - 1, below Push to Drain, then UpdateActivation.
    pub fn blood_tank_previous_mode(&mut self, t: usize) {
        let mode = self.blood_tanks[t].Mode - 1;
        self.blood_tanks[t].Mode = if mode < 0 { 3 } else { mode };
        self.blood_tank_update_activation(t);
    }

    /// BloodTankBehaviour.Update: the container's update, the LiquidContainer's fill and colour, then the mode's splat settings.
    pub fn blood_tank_update(&mut self, t: usize) {
        let o = Owner::BloodTank(t);
        self.blood_container_update(o);
        let Some(c) = self.blood_tanks[t].LiquidContainer else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        self.liquid_containers[c].FillPercentage = mathf::clamp(self.scaled_liquid_amount(o), 0.0, 1.0);
        self.liquid_container_color_from(c, o);
        let s = self.blood_tanks[t].splat;
        match self.blood_tanks[t].Mode {
            // Push and Pull: no spawning, both particle systems stopped.
            0 | 1 => {
                self.splats[s].SpawningChance = 0.0;
                let id = self.blood_tanks[t].id;
                self.ps_field_play_stop(id, "BloodDrainParticles", false);
                self.ps_field_play_stop(id, "WaterPuffParticles", false);
            }
            DRAIN => {
                let amount = self.time.deltaTime * self.blood_tanks[t].DrainRate;
                self.drain(o, amount);
                let puff = self.blood_tanks[t].water_puff;
                let submerged = self.water_puff_submerged(puff);
                let fallback = self.bc(o).cachedComputedColour;
                let colour = self.get_computed_color(o, fallback);
                let (id, full, speed) = (self.blood_tanks[t].id, self.bc(o).TotalLiquidAmount > 0.001, self.scaled_liquid_amount(o) * 5.0);
                self.liquid_spray_particles(id, "BloodDrainParticles", submerged, full, colour, speed, 120.0);
                if submerged {
                    self.splats[s].SpawningChance = 0.0;
                } else {
                    // The drain's particles start at ScaledLiquidAmount * 5 and eject with it: the splat's ejection
                    // force is (0.25, 0.35) times that speed; its decals take the computed colour.
                    let speed = self.scaled_liquid_amount(o) * 5.0;
                    let fallback = self.bc(o).cachedComputedColour;
                    let colour = self.get_computed_color(o, fallback);
                    let total = self.bc(o).TotalLiquidAmount;
                    let splat = &mut self.splats[s];
                    splat.DecalColourMultiplier = colour;
                    splat.EjectionForce = [0.25f32 * speed, 0.35f32 * speed];
                    splat.SpawningChance = total;
                }
            }
            // Idle: nothing.
            _ => {}
        }
    }

    /// BloodTankBehaviour.IsParticleSubmerged: WaterBehaviour.IsPointUnderWater(WaterPuffParticles.transform.position), the puff's Transform being `id`.
    pub(crate) fn water_puff_submerged(&mut self, id: i32) -> bool {
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.is_point_under_water([position[0], position[1]])
    }

    /// BloodTankBehaviour.OnParticleCollide(hit), subscribed by BloodTankBehaviour.Awake and removed by BloodTankBehaviour.OnDestroy: the container the hit collider's object holds takes DrainRate * dt
    /// of this tank's blood, proportionally. The object's container is found by TryGetComponent<BloodContainer>.
    pub fn blood_tank_on_particle_collide(&mut self, t: usize, collider: i32) {
        // OnDestroy removed the listener.
        if self.destroyed_components.contains(&self.blood_tanks[t].id) {
            return;
        }
        if let Some(target) = self.particle_hit_container(collider) {
            let amount = self.blood_tanks[t].DrainRate * self.time.deltaTime;
            self.copy_to(Owner::BloodTank(t), amount, target, true);
        }
    }

    /// TryGetComponent<BloodContainer> on a particle's hit collider's object: the container it holds, if any.
    pub(super) fn particle_hit_container(&mut self, collider: i32) -> Option<Owner> {
        match self.physical_of_collider(collider) {
            Some(p) => self.physical_container(p),
            None => {
                if self.bare_colliders[&collider].scripts.iter().any(|s| BLOOD_CONTAINERS.contains(&s.as_str())) {
                    unimplemented!("mod-only: a BloodContainer on a particle's hit collider without a PhysicalBehaviour");
                }
                None
            }
        }
    }
}

impl super::Game {
    /// BloodTankBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>() (PhysicalParticles.OnCollide's subscription is
    /// the spawn's `splats[splat].on_collide`).
    pub fn blood_tank_awake(&mut self, i: usize) {
        self.blood_tanks[i].phys = Some(self.blood_tanks[i].physical);
    }

    /// BloodTankBehaviour.OnDestroy: PhysicalParticles.OnCollide -= OnParticleCollide.
    pub fn blood_tank_on_destroy(&mut self, i: usize) {
        let s = self.blood_tanks[i].splat;
        if self.splats[s].on_collide == Some(Owner::BloodTank(i)) {
            self.splats[s].on_collide = None;
        }
    }

    /// The tank's listeners on its splat's OnCollide.
    pub fn blood_tank_listeners(&self, i: usize) -> u32 {
        (self.splats[self.blood_tanks[i].splat].on_collide == Some(Owner::BloodTank(i))) as u32
    }
}
