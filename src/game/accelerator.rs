//! AcceleratorGunBehaviour (the Accelerator) and AcceleratorBoltBehaviour (its bolt). Holding charges
//! CurrentChargeProgress over MaxChargeTime seconds; releasing fires a low-power shot (below 0.9) or a full-power one.
//! A shot instantiates the bolt, sets its energy, and recoils the gun. The bolt is a bare Transform: bolt.rs steps it,
//! and its hit, AoE pushes, charge and temperature, the Immobility field and the lightning are here (lightning.rs).
//! The particles, the charge-up particle scale and the muzzle flash are presentation.
#![allow(non_snake_case)]

use super::bolt::ENERGY_LAYERS;
use super::explosion::RayHit2D;
use super::translated::rt::Xf;
use super::{mathf, script, Behaviour, Game};
use crate::assets::db::{Component, ObjRef};
use crate::b2::math::Vec2 as V;

script! {
    Accelerator {
        MaxChargeTime: f32 = 2.357,
        CurrentChargeProgress: f32 = 0.0,
        LocalBarrelPosition: [f32; 2] = [0.0; 2],
        LocalBarrelDirection: [f32; 2] = [0.0; 2],
        RecoilIntensity: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour (the charge's Started, Is and Stopped queries, and the Rigidbody that recoils).
        physical: usize = 0,
        /// SingleShotSound length (PickRandom for a low-power shot).
        SingleShotSound: usize = 0,
        /// ProjectilePrefab: the bolt a shot instantiates.
        ProjectilePrefab: Option<ObjRef> = None,
    }
}

script! {
    AcceleratorBolt {
        Speed: f32 = 0.0,
        EnergyLevel: f32 = 0.0,
        CameraShakeAmount: f32 = 0.0,
        ImpactStrength: f32 = 0.0,
        AoERadius: f32 = 1.0,
        ActivationDelay: f32 = 0.5,
        ImmobilityFieldSlowdown: f32 = 0.8,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// enabled (the bolt's Update and FixedUpdate run only while it is).
        enabled: bool = true,
        /// BaseBoltBehaviour.Layers (LayerMask bits).
        Layers: u32 = 0,
        /// ImmobiltyFieldLayer's bits.
        ImmobiltyFieldLayer: u32 = 0,
        /// ImpactEffect and LargeImpactEffect; their poses are presentation (link checks they have no physics).
        ImpactEffect: Option<ObjRef> = None,
        LargeImpactEffect: Option<ObjRef> = None,
        /// The private speedMultiplier (1 until the Immobility field slows it).
        speedMultiplier: f32 = 1.0,
        /// The private timeAlive: Update's seconds since the bolt was made.
        timeAlive: f32 = 0.0,
    }
}

/// AcceleratorBoltBehaviour.ShouldReflect (BaseBoltBehaviour.ShouldReflect for the accelerator bolt): EnergyLayers.HasLayer(hit.collider.gameObject.layer).
fn accelerator_reflects(g: &Game, hit: &RayHit2D) -> bool {
    super::physical::has_layer(ENERGY_LAYERS, g.world.layers[&hit.game_object])
}

impl Game {
    /// AcceleratorGunBehaviour.Shoot (the CanShoot override): ShootLowPower. Nothing in the game calls it on this gun.
    #[allow(dead_code)]
    pub fn accelerator_shoot(&mut self, a: usize) {
        self.accelerator_shoot_low_power(a);
    }

    /// AcceleratorGunBehaviour.Update
    pub fn accelerator_update(&mut self, a: usize) {
        let p = self.accelerators[a].physical;
        let id = self.accelerators[a].id;
        if self.started_used_continuously(p) {
            let loop_source = self.audio_src(id, "LoopAudioSource");
            self.audio.get_mut(loop_source).looping = false;
            let clip = self.audio.clip(id, "ChargeUpSound");
            self.audio.play_one_shot(loop_source, clip.as_ref(), 1.0);
        }
        if self.used_continuously(p) {
            let g = &mut self.accelerators[a];
            g.CurrentChargeProgress += self.time.deltaTime / g.MaxChargeTime;
            if g.CurrentChargeProgress >= 0.9999 {
                let loop_source = self.audio_src(id, "LoopAudioSource");
                let waiting = self.audio.clip(id, "FullPowerWaitingLoopSound");
                if self.audio.get(loop_source).clip.as_ref().map(|c| c.asset) != waiting.as_ref().map(|c| c.asset) {
                    self.audio.set_clip(loop_source, waiting);
                    self.audio.get_mut(loop_source).looping = true;
                    self.audio.play(loop_source);
                }
            }
        }
        if self.stopped_used_continuously(p) {
            let loop_source = self.audio_src(id, "LoopAudioSource");
            self.audio.stop(loop_source);
            self.audio.set_clip(loop_source, None);
            let progress = self.accelerators[a].CurrentChargeProgress;
            if progress >= 0.9 {
                self.accelerator_shoot_full_power(a);
            } else if progress >= 0.0 {
                self.accelerator_shoot_low_power(a);
            }
            self.accelerators[a].CurrentChargeProgress = 0.0;
        }
        self.accelerators[a].CurrentChargeProgress = mathf::clamp(self.accelerators[a].CurrentChargeProgress, 0.0, 1.0);
    }

    /// AcceleratorGunBehaviour.BarrelPosition: Rigidbody.GetRelativePoint(LocalBarrelPosition * lossyScale).
    pub(crate) fn accelerator_barrel_position(&mut self, a: usize) -> [f32; 2] {
        let p = self.accelerators[a].physical;
        self.rigidbody_relative_point(p, self.accelerators[a].LocalBarrelPosition)
    }

    /// AcceleratorGunBehaviour.BarrelDirection: transform.TransformVector(LocalBarrelDirection).
    pub(crate) fn accelerator_barrel_direction(&self, a: usize) -> [f32; 2] {
        let p = self.accelerators[a].physical;
        self.transform_vector_chain(p, self.accelerators[a].LocalBarrelDirection)
    }

    /// AcceleratorGunBehaviour.ShootLowPower: PlayOneShot(SingleShotSound.PickRandom()), SpawnProjectile(15 + 10c, 2,
    /// 80 + 10c, 0.6 + 0.4c), then the recoil AddForceAtPosition(0.1 * -RecoilIntensity * BarrelDirection).
    pub(crate) fn accelerator_shoot_low_power(&mut self, a: usize) {
        let id = self.accelerators[a].id;
        self.audio_one_shot_pick(id, "OneShotAudioSource", "SingleShotSound", self.accelerators[a].SingleShotSound);
        let c = self.accelerators[a].CurrentChargeProgress;
        self.accelerator_spawn_projectile(a, 15.0 + c * 10.0, 2.0, 80.0 + c * 10.0, 0.6 + c * 0.4);
        let k = 0.1f32 * (0.0 - self.accelerators[a].RecoilIntensity);
        let dir = self.accelerator_barrel_direction(a);
        let at = self.accelerator_barrel_position(a);
        let rb = self.physicals[self.accelerators[a].physical].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, [k * dir[0], k * dir[1]], at);
    }

    /// AcceleratorGunBehaviour.ShootFullPower: SpawnProjectile(250, 6, 25, 1.5), then the recoil
    /// AddForceAtPosition(BarrelDirection * -RecoilIntensity).
    pub(crate) fn accelerator_shoot_full_power(&mut self, a: usize) {
        let id = self.accelerators[a].id;
        self.audio_one_shot(id, "OneShotAudioSource", "FullPowerShotSound");
        self.accelerator_spawn_projectile(a, 250.0, 6.0, 25.0, 1.5);
        let k = 0.0 - self.accelerators[a].RecoilIntensity;
        let dir = self.accelerator_barrel_direction(a);
        let at = self.accelerator_barrel_position(a);
        let rb = self.physicals[self.accelerators[a].physical].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, [dir[0] * k, dir[1] * k], at);
    }

    /// AcceleratorGunBehaviour.SpawnProjectile: Instantiate(ProjectilePrefab, BarrelPosition, identity), its right along
    /// BarrelDirection and scale `size`; its AcceleratorBoltBehaviour (when it has one) takes the energy, radius, speed,
    /// activation delay (radius / speed * 2) and camera shake (energy * 0.05).
    pub(crate) fn accelerator_spawn_projectile(&mut self, a: usize, energy: f32, radius: f32, speed: f32, size: f32) {
        let prefab = self.accelerators[a].ProjectilePrefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let at = self.accelerator_barrel_position(a);
        let id = self.spawn_prefab(prefab, [at[0], at[1], 0.0]);
        let dir = self.accelerator_barrel_direction(a);
        self.tr_set_right(Xf::Object(id), [dir[0], dir[1], 0.0]);
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, [size, size, size]));
        if let Some(b) = self.accelerator_bolt_of_clone(id) {
            let s = &mut self.accelerator_bolts[b];
            s.EnergyLevel = energy;
            s.AoERadius = radius;
            s.Speed = speed;
            s.ActivationDelay = radius / speed * 2.0;
            s.CameraShakeAmount = energy * 0.05;
            // TryGetComponent<AudioSource>: the loop clip by energy, then Play.
            let bolt_id = s.id;
            if let Some(src) = self.audio.object_source(bolt_id) {
                let gun = self.accelerators[a].id;
                let clip = self.audio.clip(gun, if energy > 100.0 { "HighEnergyBoltLoopSound" } else { "LowEnergyBoltLoopSound" });
                self.audio.set_clip(src, clip);
                self.audio.play(src);
            }
        }
    }

    /// The AcceleratorBoltBehaviour arena entry of the clone whose root Transform is `id` (GetComponent after Instantiate).
    fn accelerator_bolt_of_clone(&mut self, id: i32) -> Option<usize> {
        self.with_component_instance(id, |g, inst, _| {
            for x in 0..inst.prefab.objects.len() {
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    if matches!(c, Component::Script { class, .. } if class == "AcceleratorBoltBehaviour") {
                        return match g.behaviour_of.get(&inst.objects[x].components[k]) {
                            Some(&(_, Behaviour::AcceleratorBolt(i))) => Some(i),
                            _ => None,
                        };
                    }
                }
            }
            None
        })
    }

    /// AcceleratorBoltBehaviour.Update: BaseBoltBehaviour.Update (the flight, reflection and OnHit), then timeAlive.
    pub fn accelerator_bolt_update(&mut self, b: usize) {
        let id = self.accelerator_bolts[b].id;
        // AcceleratorBoltBehaviour.GetSpeedMultiplier: speedMultiplier.
        let (speed, layers, multiplier) = (self.accelerator_bolts[b].Speed, self.accelerator_bolts[b].Layers, self.accelerator_bolts[b].speedMultiplier);
        if let Some(hit) = self.base_bolt_move(id, speed, layers, multiplier, accelerator_reflects) {
            self.accelerator_bolt_on_hit(b, &hit);
            // transform.position = hit.point, then Object.Destroy.
            let point = [hit.point.x, hit.point.y, 0.0];
            self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, point));
            self.destroy_object(id);
        }
        let dt = self.time.deltaTime;
        self.accelerator_bolts[b].timeAlive += dt;
    }

    /// AcceleratorBoltBehaviour.FixedUpdate: the camera shake, the Immobility field's slowdown (checked at the bolt's
    /// point), and once the activation delay has passed, the AoE pushes and a lightning roll.
    pub fn accelerator_bolt_fixed_update(&mut self, b: usize) {
        let id = self.accelerator_bolts[b].id;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let s = &self.accelerator_bolts[b];
        let (shake, immobility, slowdown) = (s.CameraShakeAmount, s.ImmobiltyFieldLayer, s.ImmobilityFieldSlowdown);
        self.camera_shake(shake, [position[0], position[1]]);
        let inside = self.overlap_point_first([position[0], position[1]], immobility).is_some();
        let s = &mut self.accelerator_bolts[b];
        s.speedMultiplier = if inside { s.speedMultiplier * slowdown } else { 1.0 };
        if !(s.timeAlive > s.ActivationDelay) {
            return;
        }
        let (energy, radius, layers) = (s.EnergyLevel, s.AoERadius, s.Layers);
        // Physics2D.OverlapCircleNonAlloc into a 32-entry buffer; the order of a full buffer is not verified.
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), radius, layers);
        // NonAlloc copies the first 32 of the sorted results.
        hits.truncate(ACCELERATOR_BOLT_BUFFER);
        for (phys, _) in hits {
            let Some(p) = phys.filter(|&p| self.physicals[p].in_world) else { continue };
            // vector = collider.transform.position - transform.position; vector /= magnitude; vector /= magnitude * magnitude.
            let target = self.physicals[p].transform.position;
            let v = [target[0] - position[0], target[1] - position[1], target[2] - position[2]];
            let m = (super::d(v[0]) * v[0] + super::d(v[1]) * v[1] + super::d(v[2]) * v[2]).0.sqrt() as f32;
            let u = [v[0] / m, v[1] / m, v[2] / m];
            let u = [u[0] / (m * m), u[1] / (m * m)];
            let k = 0.15f32 * energy;
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force(&mut self.world, [k * u[0], k * u[1]]);
            let ph = &mut self.physicals[p];
            ph.charge = mathf::max(energy, ph.Charge());
            ph.Temperature += energy;
        }
        // Random.value > 0.8 rolls a lightning from the bolt, toward Random.insideUnitCircle, of the AoE's size.
        if self.random.value() > 0.8 {
            let dir = self.random.inside_unit_circle();
            self.lightning_emit([position[0], position[1]], dir, radius / 2.0, 3.0, (energy as i32) + 25, 0.07, 0.03, layers, super::lightning::LightningHit::Accelerator);
        }
    }

    /// AcceleratorBoltBehaviour.OnHit: the impact effect (up along the normal), CreatePulseExplosion(hit, ImpactStrength,
    /// AoERadius / 2, no sound or effects), then the AoE: a roll under energy / 20 breaks each object, which gains charge
    /// and temperature and is pushed by 1.2 * energy along the bolt's right.
    pub(crate) fn accelerator_bolt_on_hit(&mut self, b: usize, hit: &RayHit2D) {
        let s = &self.accelerator_bolts[b];
        let (energy, impact, radius, layers) = (s.EnergyLevel, s.ImpactStrength, s.AoERadius, s.Layers);
        let effect = if energy > 100.0 { s.LargeImpactEffect } else { s.ImpactEffect };
        let effect = effect.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
        let fx = self.spawn_prefab(effect, [point[0], point[1], 0.0]);
        self.tr_set_up(Xf::Object(fx), [normal[0], normal[1], 0.0]);
        self.create_pulse_explosion_with([point[0], point[1], 0.0], impact, radius / 2.0, false, true);
        let id = self.accelerator_bolts[b].id;
        let right = self.with_component_instance(id, |_, inst, o| mathf::rotate(inst.rotation(o), [1.0, 0.0, 0.0]));
        let mut hits = self.overlap_circle(V::new(point[0], point[1]), radius, layers);
        // NonAlloc copies the first 32 of the sorted results.
        hits.truncate(ACCELERATOR_BOLT_BUFFER);
        for (phys, collider) in hits {
            let Some(p) = phys.filter(|&p| self.physicals[p].in_world) else { continue };
            if self.random.value() < energy / 20.0 {
                self.send_break(collider, Some(p), [-normal[0] * impact, -normal[1] * impact]);
            }
            let ph = &mut self.physicals[p];
            ph.charge = mathf::max(energy, ph.Charge());
            ph.Temperature += energy;
            let k = 1.2f32 * energy;
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force_impulse(&mut self.world, [k * right[0], k * right[1]]);
        }
    }
}

/// AcceleratorBoltBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const ACCELERATOR_BOLT_BUFFER: usize = 32;
