//! LiquidOutletBehaviour (a BloodContainer, Pull): while its container holds more than 0.1 it sprays a FancyBloodSplatController
//! (PhysicalParticles) and drains 0.1 a second. Its particles copy 0.1 into the containers they hit (OnCollide). Its Update is
//! BloodContainer's. The particle systems follow the liquid (liquid_spray_particles); the awake/destroy subscriptions
//! are the link.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::{script, Game};

/// LiquidOutletBehaviour.DrainRate
const DRAIN_RATE: f32 = 0.1;

script! {
    LiquidOutlet {}
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// Phys: the outlet's PhysicalBehaviour (its charge sets the spray's speed).
        physical: usize = 0,
        /// PhysicalParticles: the `Game::splats` index of its FancyBloodSplatController.
        splat: usize = 0,
        /// WaterPuffParticles' Transform id: its position decides whether the puff is under water.
        water_puff: i32 = 0,
    }
}

impl Game {
    /// LiquidOutletBehaviour.Awake: PhysicalParticles.OnCollide += OnParticleCollide, the splat's `on_collide` link (misc.rs calls
    /// liquid_outlet_on_particle_collide; the loader links it as this).
    pub fn liquid_outlet_awake(&mut self, l: usize) {
        let s = self.liquid_outlets[l].splat;
        self.splats[s].on_collide = Some(Owner::LiquidOutlet(l));
    }

    /// LiquidOutletBehaviour.OnDestroy: OnCollide -= OnParticleCollide (a destroyed outlet's listener is also skipped by the
    /// destroyed_components check in liquid_outlet_on_particle_collide).
    pub fn liquid_outlet_on_destroy(&mut self, l: usize) {
        let s = self.liquid_outlets[l].splat;
        if self.splats[s].on_collide == Some(Owner::LiquidOutlet(l)) {
            self.splats[s].on_collide = None;
        }
    }

    /// The outlet's listeners on its splat's OnCollide (FancyBloodSplatController.OnCollide's invocation list, of this outlet).
    pub fn liquid_outlet_listeners(&self, l: usize) -> u32 {
        (self.splats[self.liquid_outlets[l].splat].on_collide == Some(Owner::LiquidOutlet(l))) as u32
    }

    /// LiquidOutletBehaviour.FixedUpdate. Out of the water the splat's ejection force and decal colour follow the charge;
    /// with more than 0.1 of liquid it sprays one particle and drains 0.1.
    pub fn liquid_outlet_fixed_update(&mut self, l: usize) {
        let o = Owner::LiquidOutlet(l);
        let num = (super::d(self.physicals[self.liquid_outlets[l].physical].Charge()) * 0.4f32 + 2.0f32).f();
        let s = self.liquid_outlets[l].splat;
        self.splats[s].SpawningChance = 0.0;
        let puff = self.liquid_outlets[l].water_puff;
        // LiquidOutletBehaviour.IsParticleSubmerged: water_puff_submerged.
        let submerged = self.water_puff_submerged(puff);
        let flag = self.bc(o).TotalLiquidAmount > 0.1;
        if !submerged {
            let fallback = self.bc(o).cachedComputedColour;
            let colour = self.get_computed_color(o, fallback);
            let splat = &mut self.splats[s];
            splat.DecalColourMultiplier = colour;
            splat.EjectionForce = [0.25f32 * num, 0.3f32 * num];
        }
        let fallback = self.bc(o).cachedComputedColour;
        let colour = self.get_computed_color(o, fallback);
        let id = self.liquid_outlets[l].id;
        self.liquid_spray_particles(id, "LiquidParticles", submerged, flag, colour, num, 120.0);
        if flag {
            self.create_splat_particle(s, 0);
            self.drain(o, DRAIN_RATE);
        }
    }

    /// The particle systems of an outlet, a leak's hole or a draining blood tank. Under water WaterPuffParticles takes the
    /// container's colour and rates 1 over time and 3 over distance (0 when `full` is false); out of it the `liquid` system
    /// takes the colour, startSpeedMultiplier `speed` and rate `rate` over time (0 when not `full`). Then under water the
    /// liquid system stops and the puff plays, else the other way (each by isPlaying).
    pub(crate) fn liquid_spray_particles(&mut self, id: i32, liquid: &str, submerged: bool, full: bool, colour: [f32; 4], speed: f32, rate: f32) {
        use crate::particles::PsWrites;
        let on = |x: f32| if full { x } else { 0.0 };
        if submerged {
            self.ps_field_writes(id, "WaterPuffParticles", PsWrites { color: Some(colour), rate: Some(on(1.0)), distance: Some(on(3.0)), speed: None, noise: None });
        } else {
            self.ps_field_writes(id, liquid, PsWrites { color: Some(colour), rate: Some(on(rate)), distance: None, speed: Some(speed), noise: None });
        }
        self.ps_field_playing(id, liquid, !submerged);
        self.ps_field_playing(id, "WaterPuffParticles", submerged);
    }

    /// BloodContainer.Update, which LiquidOutletBehaviour does not override.
    pub fn liquid_outlet_update(&mut self, l: usize) {
        self.blood_container_update(Owner::LiquidOutlet(l));
    }

    /// LiquidOutletBehaviour.OnParticleCollide(hit): CopyTo(0.1, the hit object's BloodContainer), proportional. The listener is
    /// gone once the outlet is destroyed.
    pub fn liquid_outlet_on_particle_collide(&mut self, l: usize, collider: i32) {
        if self.destroyed_components.contains(&self.liquid_outlets[l].id) {
            return;
        }
        if let Some(target) = self.particle_hit_container(collider) {
            self.copy_to(Owner::LiquidOutlet(l), DRAIN_RATE, target, true);
        }
    }
}
