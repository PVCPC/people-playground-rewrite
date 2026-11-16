//! The barrel weapons that spawn a projectile prefab: TankBarrelBehaviour (shell, pulse explosion, recoil, muzzle
//! flash) and RocketLauncherBehaviour (rocket and screen shake), and ProjectileBehaviour (the impulse a spawned
//! projectile starts with). Sounds are presentation; the Random draw for a launching sound stays.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation, Xf};
use super::{d, script, Game};

script! {
    TankBarrelBehaviour {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        foleyVolume: f32 = 1.0,
        RecoilIntensity: f32 = 15.0,
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
        Projectile: Option<rt::Go> = None,
        muzzleFlash: Option<rt::Go> = None,
        myRigidBody: Option<usize> = None,
        foleyAudioSource: bool = false,
    }
}

script! {
    RocketLauncherBehaviour {
        ScreenShakeIntensity: f32 = 0.5,
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
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
        Projectile: Option<rt::Go> = None,
        AudioSource: bool = false,
        /// LaunchingSounds.Length.
        LaunchingSounds: usize = 0,
    }
}

script! {
    ProjectileBehaviour {
        LocalDirection: [f32; 2] = [1.0f32, 0.0f32],
        Force: f32 = 5.0,
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
        Rigidbody: Option<usize> = None,
    }
}

const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

impl Game {
    /// transform.TransformPoint(local barrel position).
    fn barrel_world_point(&self, xf: Xf, local: [f32; 2]) -> [f32; 2] {
        rt::v2(self.tr_transform_point(xf, rt::v3(local)))
    }

    /// transform.TransformDirection(local barrel direction) * transform.localScale.x.
    fn barrel_world_direction(&self, xf: Xf, local: [f32; 2]) -> [f32; 2] {
        let dir = self.tr_transform_direction(xf, rt::v3(local));
        rt::v2(rt::mul(dir, self.tr_local_scale(xf)[0]))
    }

    /// Instantiate(prefab, at, identity).transform.right = right; the clone.
    fn barrel_spawn(&mut self, prefab: Option<rt::Go>, at: [f32; 2], right: [f32; 2]) -> rt::Go {
        let clone = rt::nn(self.tr_instantiate(prefab, rt::v3(at), IDENTITY));
        let xf = self.tr_go_transform(clone);
        self.tr_set_right(xf, rt::v3(right));
        clone
    }

    // ---- TankBarrelBehaviour ----

    fn tank_barrel_xf(&self, t: usize) -> Xf {
        let s = &self.tr.tank_barrel_behaviour[t];
        Xf::this(s.this_physical, s.this_physics)
    }

    /// TankBarrelBehaviour.Use (when enabled): a 300° shell along the barrel, a pulse explosion at the muzzle, the recoil
    /// impulse and the muzzle flash.
    pub fn tank_barrel_behaviour_use(&mut self, t: usize, _activation: Activation) {
        if !self.tr.tank_barrel_behaviour[t].enabled {
            return;
        }
        let shell_prefab = self.tr.tank_barrel_behaviour[t].Projectile;
        let at = self.tank_barrel_behaviour_get_barrel_position(t);
        let shell = rt::nn(self.tr_instantiate(shell_prefab, rt::v3(at), IDENTITY));
        let at = self.tank_barrel_behaviour_get_barrel_position(t);
        self.create_pulse_explosion_with(rt::v3(at), 3.0, 10.0, false, true);
        let xf = self.tr_go_transform(shell);
        let dir = self.tank_barrel_behaviour_get_barrel_direction(t);
        self.tr_set_right(xf, rt::v3(dir));
        // obj.GetComponent<PhysicalBehaviour>().Temperature = 300
        let p = match self.tr_go_transform(shell) {
            Xf::Physical(p) => Some(p),
            _ => None,
        };
        self.physicals[rt::nn(p)].Temperature = 300.0;
        let recoil = (d(self.tr.tank_barrel_behaviour[t].RecoilIntensity) * self.tr_lossy_scale(self.tank_barrel_xf(t))[0]).f();
        let rb = rt::nn(self.tr.tank_barrel_behaviour[t].myRigidBody);
        self.physicals[rb].rigidbody.add_relative_force_impulse(&mut self.world, rt::mul([-1.0, 0.0], recoil));
        let flash = self.tr.tank_barrel_behaviour[t].muzzleFlash;
        let at = self.tank_barrel_behaviour_get_barrel_position(t);
        let dir = self.tank_barrel_behaviour_get_barrel_direction(t);
        self.barrel_spawn(flash, at, dir);
    }

    /// TankBarrelBehaviour.GetBarrelPosition
    pub fn tank_barrel_behaviour_get_barrel_position(&mut self, t: usize) -> [f32; 2] {
        self.barrel_world_point(self.tank_barrel_xf(t), self.tr.tank_barrel_behaviour[t].barrelPosition)
    }

    /// TankBarrelBehaviour.GetBarrelDirection
    pub fn tank_barrel_behaviour_get_barrel_direction(&mut self, t: usize) -> [f32; 2] {
        self.barrel_world_direction(self.tank_barrel_xf(t), self.tr.tank_barrel_behaviour[t].barrelDirection)
    }

    /// TankBarrelBehaviour.FixedUpdate: the turning foley sound on a dynamic body (presentation).
    pub fn tank_barrel_behaviour_fixed_update(&mut self, t: usize) {
        let rb = rt::nn(self.tr.tank_barrel_behaviour[t].myRigidBody);
        if self.physicals[rb].rigidbody.bodyType == 0 {
            rt::pres(self.tr.tank_barrel_behaviour[t].foleyAudioSource);
        }
    }

    // ---- RocketLauncherBehaviour ----

    /// RocketLauncherBehaviour.Use (when enabled): a launching sound (LaunchingSounds.PickRandom() draws), the screen
    /// shake and the rocket along the barrel.
    pub fn rocket_launcher_behaviour_use(&mut self, r: usize, _activation: Activation) {
        if !self.tr.rocket_launcher_behaviour[r].enabled {
            return;
        }
        let (id, sounds) = (self.tr.rocket_launcher_behaviour[r].this_id, self.tr.rocket_launcher_behaviour[r].LaunchingSounds);
        if sounds != 0 {
            self.audio_one_shot_pick(id, "AudioSource", "LaunchingSounds", sounds);
        } else {
            self.audio_one_shot_own(id, "AudioSource");
        }
        let shake = self.tr.rocket_launcher_behaviour[r].ScreenShakeIntensity;
        let at = self.rocket_launcher_behaviour_get_barrel_position(r);
        self.camera_shake_at(shake, at, 1.0);
        let rocket = self.tr.rocket_launcher_behaviour[r].Projectile;
        let at = self.rocket_launcher_behaviour_get_barrel_position(r);
        let dir = self.rocket_launcher_behaviour_get_barrel_direction(r);
        self.barrel_spawn(rocket, at, dir);
    }

    fn rocket_launcher_xf(&self, r: usize) -> Xf {
        let s = &self.tr.rocket_launcher_behaviour[r];
        Xf::this(s.this_physical, s.this_physics)
    }

    /// RocketLauncherBehaviour.get_BarrelPosition
    pub fn rocket_launcher_behaviour_get_barrel_position(&mut self, r: usize) -> [f32; 2] {
        self.barrel_world_point(self.rocket_launcher_xf(r), self.tr.rocket_launcher_behaviour[r].barrelPosition)
    }

    /// RocketLauncherBehaviour.get_BarrelDirection
    pub fn rocket_launcher_behaviour_get_barrel_direction(&mut self, r: usize) -> [f32; 2] {
        self.barrel_world_direction(self.rocket_launcher_xf(r), self.tr.rocket_launcher_behaviour[r].barrelDirection)
    }

    // ---- ProjectileBehaviour ----

    /// ProjectileBehaviour.Start: an impulse of TransformDirection(LocalDirection) * Force.
    pub fn projectile_behaviour_start(&mut self, p: usize) {
        let s = &self.tr.projectile_behaviour[p];
        let dir = self.tr_transform_direction(Xf::this(s.this_physical, s.this_physics), rt::v3(s.LocalDirection));
        let impulse = rt::v2(rt::mul(dir, s.Force));
        let rb = rt::nn(self.tr.projectile_behaviour[p].Rigidbody);
        self.physicals[rb].rigidbody.add_force_impulse(&mut self.world, impulse);
    }
}
