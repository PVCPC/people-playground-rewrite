//! NewAtomBombExplosionBehaviour (the Atom Bomb and Fusion Bomb effect). Start vaporises or heats every physical
//! object in the world: within VaporisationDistance it reaches 10000 degrees and, by chance, its root is destroyed;
//! with EmpChance it gets OnEMPHit; with a clear line it is heated by distance. Update grows the shockwave trigger
//! and weakens its PointEffector2D along ShockwaveGrowth; the ShockwaveSprite grows with the collider and fades. The audio is an AUDIO call site.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};
use crate::b2::math::Vec2 as V;
use crate::game_assets::{Curve, CurveCache};

script! {
    AtomExplosion {
        MaxForceMagnitude: f32 = 25000.0,
        MinForceMagnitude: f32 = 0.0,
        MinColliderSize: f32 = 10.0,
        MaxColliderSize: f32 = 910.2273,
        SpeedMultiplier: f32 = 1.0,
        ShakeIntensity: f32 = 15.0,
        VaporisationDistance: f32 = 0.0,
        TemperatureMultiplier: f32 = 1.0,
        EmpChance: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// transform.position (a root without a PhysicalBehaviour; nothing moves it).
        position: [f32; 3] = [0.0; 3],
        /// ShockwaveCollider: (object, component) in its clone.
        ShockwaveCollider: (usize, usize) = (0, 0),
        /// PointEffector: the PointEffector2D component id.
        PointEffector: i32 = 0,
        ShockwaveGrowth: Option<Curve> = None,
        /// ShockwaveSprite (null: ExpandShockwave throws).
        ShockwaveSprite: Option<super::visual::SpriteAt> = None,
        cache: CurveCache = CurveCache::default(),
        time: f32 = 0.0,
    }
}

// NewAtomBombExplosionBehaviour.Awake: mainCam = Camera.main (read as `Game::camera_position`)
impl Game {
    /// NewAtomBombExplosionBehaviour.Start
    pub fn atom_explosion_start(&mut self, i: usize) {
        let s = &self.atom_explosions[i];
        let (position, vaporisation, emp_chance, multiplier) = (s.position, s.VaporisationDistance, s.EmpChance, s.TemperatureMultiplier);
        let num = vaporisation * vaporisation;
        let items: Vec<usize> = (0..self.physicals.len()).filter(|&q| self.physicals[q].in_world).collect();
        for q in items {
            if self.physicals[q].destroyed {
                continue;
            }
            let at = self.physicals[q].transform.position;
            let d = [at[0] - position[0], at[1] - position[1], at[2] - position[2]];
            let sqr = (super::d(d[0]) * d[0] + super::d(d[1]) * d[1] + super::d(d[2]) * d[2]).f();
            if sqr < num {
                self.physicals[q].Temperature = 10000.0;
                let softness = self.properties[self.physicals[q].Properties].Softness;
                let ph = &self.physicals[q];
                if self.random.value() < softness && ph.Deletable && ph.Disintegratable && !ph.isDisintegrated {
                    // Object.Destroy(transform.root.gameObject)
                    let root = self.with_component_instance(self.physicals[q].id, |_, inst, mut o| {
                        while let Some(up) = inst.objects[o].parent {
                            o = up;
                        }
                        inst.objects[o].components[0]
                    });
                    self.destroy_object(root);
                    continue;
                }
            }
            // Physics2D.LinecastNonAlloc(position, physicalBehaviour.transform.position, buffer[8])
            let hits = self.linecast_hits(V::new(position[0], position[1]), V::new(at[0], at[1]), super::explosion::DEFAULT_RAYCAST_LAYERS, ATOM_BOMB_BUFFER);
            let own = self.physical_game_object(q);
            let mut flag = true;
            for h in hits {
                // buffer[j].transform: the hit collider's GameObject; base.transform has no PhysicalBehaviour.
                if h.game_object != own {
                    let by_transform = (0..self.physicals.len()).find(|&r| self.physicals[r].in_world && !self.physicals[r].destroyed && self.physical_game_object(r) == h.game_object);
                    if by_transform.is_some_and(|r| self.physicals[r].AbsorbsLasers) {
                        flag = false;
                        break;
                    }
                }
            }
            if self.random.value() < emp_chance {
                self.emp_send_on_hit(q);
            }
            if flag {
                let num3 = f32::max(f32::from_bits(1), sqr) / 281250.0;
                let t = self.physicals[q].Temperature;
                self.physicals[q].Temperature = f32::max(500.0, f32::max(t, f32::min(10000.0 * multiplier, (super::d(100.0) / num3 * multiplier).f())));
            }
        }
    }

    /// NewAtomBombExplosionBehaviour.Update: ExpandShockwave(ShockwaveGrowth.Evaluate(SpeedMultiplier * time)).
    pub fn atom_explosion_update(&mut self, i: usize) {
        // NewAtomBombExplosionBehaviour.ExpandShockwave(t): the collider radius, the effector's force, the sprite's scale (2 * radius) and alpha (1 - t)
        let s = &mut self.atom_explosions[i];
        let curve = s.ShockwaveGrowth.clone().expect("ShockwaveGrowth");
        let t = curve.evaluate(&mut s.cache, s.SpeedMultiplier * s.time);
        let radius = mathf::lerp(s.MinColliderSize, s.MaxColliderSize, t);
        let force = mathf::lerp(s.MaxForceMagnitude, s.MinForceMagnitude, t);
        let (id, key, effector) = (s.id, s.ShockwaveCollider, s.PointEffector);
        self.with_component_instance(id, |g, inst, _| g.circle_set_radius(inst, key, radius));
        self.effector_set_force_magnitude(effector, force);
        let sprite = self.atom_explosions[i].ShockwaveSprite.expect("NewAtomBombExplosionBehaviour: ShockwaveSprite null (NullReferenceException)");
        let scale = (d(2.0) * radius).f();
        self.set_sprite_local_scale(sprite, [scale * 1.0, scale * 1.0, scale * 1.0]);
        let [r, g, b, _] = self.sprite_color(sprite);
        self.set_sprite_color(sprite, [r, g, b, 1.0 - t]);
        // NewAtomBombExplosionBehaviour.ShakeScreen(t): mainCam is Camera.main, which `camera_position` reads
        let (position, intensity) = (self.atom_explosions[i].position, self.atom_explosions[i].ShakeIntensity);
        let num = (d(radius) * radius).f();
        let num2 = mathf::sqr_magnitude([self.camera_position()[0] - position[0], self.camera_position()[1] - position[1]]);
        let num3 = (num - num2).abs();
        let num4 = (d(intensity) * (1.0f32 - t)).f();
        self.camera_shake_at(mathf::max(0.0, 40000.0 - num3) * num4, [position[0], position[1]], 0.0);
        self.camera_shake(num4, [position[0], position[1]]);
        self.atom_explosions[i].time += self.time.deltaTime;
    }
}

/// NewAtomBombExplosionBehaviour.buffer: static RaycastHit2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const ATOM_BOMB_BUFFER: usize = 8;
