//! FanBehaviour (fans, propellers, helicopter rotors) and CosmeticRotationBehaviour (their blades, and the boat motor's
//! propeller). Audio volumes, Debug.Draw calls and the blades' localEulerAngles are presentation; a missing AudioSource
//! still throws where the game would touch it.
#![allow(non_snake_case)]

use super::translated::rt::{self, Activation, Xf};
use super::{d, mathf, script, Game};

script! {
    FanBehaviour {
        Strength: f32 = 0.0,
        Range: f32 = 5.0,
        AreaWidth: f32 = 0.0,
        LocalStartPos: [f32; 2] = [0.0; 2],
        RelativeDirection: [f32; 2] = [0.0, -1.0],
        DeathSize: [f32; 2] = [0.0; 2],
        DeathDirection: [f32; 2] = [0.0; 2],
        DeathLocalStartPos: [f32; 2] = [0.0; 2],
        HelicopterVolumeScaleRange: [f32; 2] = [1.5, 5.5],
        Activated: bool = false,
        /// FanMode: FAN or PROPELLER.
        Mode: i32 = FAN,
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
        HelicopterAudioSource: bool = false,
        FirstBladeRenderer: bool = false,
        SecondBladeRenderer: bool = false,
        /// A CosmeticRotationBehaviour.
        Blades: Option<usize> = None,
        DamagableMachinery: Option<usize> = None,
        physicalBehaviour: Option<usize> = None,
        Audio: bool = false,
        Switch: bool = false,
        StartupSound: bool = false,
        ShutoffSound: bool = false,
        Mask: u32 = 0,
    }
}

script! {
    CosmeticRotationBehaviour {
        /// Axis: X, Y or Z.
        Axis: i32 = 2,
        RotationSpeedTarget: f32 = 10.0,
        Dampening: f32 = 0.9,
        Acceleration: f32 = 0.1,
        ShouldRotate: bool = true,
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
        rot: f32 = 0.0,
        vel: f32 = 0.0,
    }
}

/// FanBehaviour.FanMode
const FAN: i32 = 0;
const PROPELLER: i32 = 1;

impl Game {
    fn fan_xf(&self, f: usize) -> Xf {
        let fan = &self.tr.fan_behaviour[f];
        Xf::this(fan.this_physical, fan.this_physics)
    }

    fn fan_physical(&self, f: usize) -> usize {
        rt::nn(self.tr.fan_behaviour[f].physicalBehaviour)
    }

    fn fan_running(&self, f: usize) -> bool {
        self.tr.fan_behaviour[f].Activated && !self.machinery[rt::nn(self.tr.fan_behaviour[f].DamagableMachinery)].Destroyed
    }

    /// FanBehaviour.IsFan
    pub fn fan_behaviour_is_fan(&self, f: usize) -> bool {
        self.tr.fan_behaviour[f].Mode == FAN
    }

    /// FanBehaviour.Awake: physicalBehaviour = GetComponent<PhysicalBehaviour>(); DamagableMachinery.OnDamaged and
    /// OnRepaired get UpdateActivated as runtime listeners (`machinery_runtime_listeners`).
    pub fn fan_behaviour_awake(&mut self, f: usize) {
        self.tr.fan_behaviour[f].physicalBehaviour = self.tr.fan_behaviour[f].this_physical;
        let m = rt::nn(self.tr.fan_behaviour[f].DamagableMachinery);
        self.machinery[m].runtime_listeners.push(super::machinery::MachineryListener::FanUpdateActivated(f));
    }

    /// FanBehaviour.Start
    pub fn fan_behaviour_start(&mut self, f: usize) {
        self.fan_behaviour_update_activated(f);
    }

    /// FanBehaviour.Use
    pub fn fan_behaviour_use(&mut self, f: usize, _activation: Activation) {
        if self.tr.fan_behaviour[f].enabled && !self.machinery[rt::nn(self.tr.fan_behaviour[f].DamagableMachinery)].Destroyed {
            self.tr.fan_behaviour[f].Activated = !self.tr.fan_behaviour[f].Activated;
            // Audio.PlayOneShot(Switch)
            rt::pres(self.tr.fan_behaviour[f].Audio);
            self.fan_behaviour_update_activated(f);
        }
    }

    /// FanBehaviour.Update: the audio volumes by scale (presentation), then the blades follow the fan.
    pub fn fan_behaviour_update(&mut self, f: usize) {
        if self.tr.fan_behaviour[f].HelicopterAudioSource {
            // Audio.volume = Utils.MapRange(...): reads localScale, which throws for a bare object.
            self.tr_local_scale(self.fan_xf(f));
            rt::pres(self.tr.fan_behaviour[f].Audio);
        }
        let running = self.fan_running(f);
        let target = (9000.0f32 * (d(self.physicals[self.fan_physical(f)].Charge()) + 1.0f32)).f();
        let blades = &mut self.tr.cosmetic_rotation_behaviour[rt::nn(self.tr.fan_behaviour[f].Blades)];
        blades.ShouldRotate = running;
        blades.RotationSpeedTarget = target;
    }

    /// FanBehaviour.FixedUpdate
    pub fn fan_behaviour_fixed_update(&mut self, f: usize) {
        if !self.fan_running(f) {
            return;
        }
        let world_dir = self.tr_transform_vector(self.fan_xf(f), rt::v3(self.tr.fan_behaviour[f].RelativeDirection));
        match self.tr.fan_behaviour[f].Mode {
            FAN => {
                self.fan_behaviour_hover_and_push(f, world_dir);
                self.fan_behaviour_kill(f);
            }
            PROPELLER => self.fan_behaviour_fly(f, world_dir),
            _ => {}
        }
    }

    /// FanBehaviour.Fly
    pub(crate) fn fan_behaviour_fly(&mut self, f: usize, world_dir: [f32; 3]) {
        let p = self.fan_physical(f);
        let force = ((d(self.physicals[p].Charge()) + 1.0f32) * 50.0f32 * self.tr.fan_behaviour[f].Strength).f();
        self.physicals[p].rigidbody.add_force(&mut self.world, rt::v2(rt::mul(world_dir, force)));
    }

    /// FanBehaviour.HoverAndPush
    pub(crate) fn fan_behaviour_hover_and_push(&mut self, f: usize, world_dir: [f32; 3]) {
        let length = (d(self.tr.fan_behaviour[f].Range) * 2.0f32 + d(self.physicals[self.fan_physical(f)].Charge()) * 0.5f32).f();
        self.fan_behaviour_push(f, world_dir, length);
        self.fan_behaviour_hover(f, world_dir, length);
    }

    /// The colliders OverlapBoxNonAlloc writes into the shared buffer (at most its 128).
    fn fan_overlap(&mut self, point: [f32; 2], size: [f32; 2], angle: f32, mask: u32) -> usize {
        let hits = self.tr_overlap_box(point, size, angle, mask);
        rt::fill(&mut self.tr.fan_behaviour__buffer, hits) as usize
    }

    /// The PhysicalBehaviour of a buffered collider that is not on the fan's own Transform
    /// (PhysicalObjectsInWorldByTransform.TryGetValue(collider2D.transform)).
    fn fan_other_physical(&mut self, f: usize, collider: i32) -> Option<usize> {
        if self.tr_xf_eq(Some(Xf::Collider(collider)), Some(self.fan_xf(f))) {
            return None;
        }
        self.tr_physical_by_transform(Xf::Collider(collider))
    }

    /// FanBehaviour.Kill: whatever is in the blades is shoved along DeathDirection, the fan the other way.
    pub(crate) fn fan_behaviour_kill(&mut self, f: usize) {
        let xf = self.fan_xf(f);
        let fan = &self.tr.fan_behaviour[f];
        let (start, size, direction, mask) = (fan.DeathLocalStartPos, fan.DeathSize, fan.DeathDirection, fan.Mask);
        let point = rt::v2(self.tr_transform_point(xf, rt::v3(start)));
        let angle = self.tr_euler_angles(xf)[2];
        let num = self.fan_overlap(point, size, angle, mask);
        if num == 0 {
            return;
        }
        let blades = rt::nn(self.tr.fan_behaviour[f].Blades);
        let speed = self.cosmetic_rotation_behaviour_get_current_speed(blades);
        let charge = self.physicals[self.fan_physical(f)].Charge();
        let scale = (d(mathf::sin(self.time.time * speed)) * speed * 2.0f32 * (d(mathf::min(charge, 2.0)) + 1.0f32)).f();
        let vector = rt::mul(self.tr_transform_vector(xf, rt::v3(direction)), scale);
        for i in 0..num {
            let collider = self.tr.fan_behaviour__buffer[i];
            if let Some(other) = self.fan_other_physical(f, collider) {
                self.physicals[other].rigidbody.add_force(&mut self.world, rt::v2(vector));
                let p = self.fan_physical(f);
                self.physicals[p].rigidbody.add_force(&mut self.world, rt::v2(rt::neg(vector)));
            }
        }
    }

    /// FanBehaviour.UpdateActivated: the motor sound (presentation; the AudioSource must exist).
    pub fn fan_behaviour_update_activated(&mut self, f: usize) {
        let audio = self.tr.fan_behaviour[f].Audio;
        if self.fan_running(f) {
            // Audio.Play(); Audio.PlayOneShot(StartupSound); the helicopter source the same.
            rt::pres(audio);
            self.present_audio_play(self.tr.fan_behaviour[f].this_id, "Audio", true);
            return;
        }
        // Audio.Stop()
        rt::pres(audio);
        self.present_audio_play(self.tr.fan_behaviour[f].this_id, "Audio", false);
        let blades = rt::nn(self.tr.fan_behaviour[f].Blades);
        if self.cosmetic_rotation_behaviour_get_current_speed(blades) > 30.0 {
            // HelicopterAudioSource.PlayOneShot(ShutoffSound, 1f) if there is one; Audio.PlayOneShot(ShutoffSound, 0.5f).
            rt::pres(audio);
        }
    }

    /// FanBehaviour.Hover: a fan blowing at the ground holds itself at half its reach.
    pub(crate) fn fan_behaviour_hover(&mut self, f: usize, world_dir: [f32; 3], length: f32) {
        let p = self.fan_physical(f);
        if self.physicals[p].Wetness > 0.2 {
            let force = ((d(self.physicals[p].Charge()) + 1.0f32) * 50.0f32).f();
            self.physicals[p].rigidbody.add_force(&mut self.world, rt::v2(rt::mul(world_dir, force)));
        }
        let xf = self.fan_xf(f);
        let vector = self.tr_transform_point(xf, rt::v3(self.tr.fan_behaviour[f].LocalStartPos));
        let Some(hit) = self.tr_raycast(rt::v2(vector), rt::v2(rt::neg(world_dir)), length, self.tr.fan_behaviour[f].Mask) else {
            let force = ((d(self.physicals[p].Charge()) + 1.0f32) * 20.0f32).f();
            self.physicals[p].rigidbody.add_force(&mut self.world, rt::v2(rt::mul(world_dir, force)));
            return;
        };
        let hover_point = rt::v2(rt::sub(vector, rt::div(rt::mul(world_dir, length), 2.0)));
        let mass = self.physicals[p].rigidbody.mass(&self.world);
        let vector3 = rt::mul(rt::sub(hover_point, [hit.point.x, hit.point.y]), -20.0 * mass);
        if !(rt::dot2(rt::v2(rt::normalized3(world_dir)), mathf::normalized2(vector3)) < 0.0) {
            // Debug.DrawLine(transform.position, ...): the position read still needs a pose.
            self.tr_position(xf);
            self.physicals[p].rigidbody.add_force(&mut self.world, vector3);
            let v = rt::mul(self.physicals[p].rigidbody.velocity(&self.world), 0.98);
            self.physicals[p].rigidbody.set_velocity(&mut self.world, v);
        }
    }

    /// FanBehaviour.Push: everything in the fan's area is blown along and cooled toward the ambient temperature.
    pub(crate) fn fan_behaviour_push(&mut self, f: usize, world_dir: [f32; 3], length: f32) {
        let p = self.fan_physical(f);
        let strength = (d(self.tr.fan_behaviour[f].Strength) * (d(1.0f32) + self.physicals[p].Charge())).f();
        let xf = self.fan_xf(f);
        let fan = &self.tr.fan_behaviour[f];
        let (start, width, mask) = (fan.LocalStartPos, fan.AreaWidth, fan.Mask);
        let point = rt::v2(self.tr_transform_point(xf, rt::v3(start)));
        let angle = (d(rt::atan2(world_dir[1], world_dir[0])) * 57.29578f32 - 90.0f32).f();
        let num = self.fan_overlap(point, [width, length], angle, mask);
        let vector = rt::mul(world_dir, (d(2.0f32) * (d(0.0f32) - strength)).f());
        for i in 0..num {
            let collider = self.tr.fan_behaviour__buffer[i];
            if let Some(other) = self.fan_other_physical(f, collider) {
                let mass = mathf::max(self.physicals[other].rigidbody.mass(&self.world), 1.0);
                self.physicals[other].rigidbody.add_force(&mut self.world, rt::v2(rt::mul(vector, mass)));
                self.physicals[other].Temperature = mathf::lerp(self.physicals[other].Temperature, self.AmbientTemperature, 0.005);
            }
        }
    }

    /// FanBehaviour.OnDisable
    pub fn fan_behaviour_on_disable(&mut self, f: usize) {
        self.tr.fan_behaviour[f].Activated = false;
        self.fan_behaviour_update_activated(f);
    }

    /// CosmeticRotationBehaviour.Update: the spin eases toward its target and damps; the angle itself is only drawn.
    pub fn cosmetic_rotation_behaviour_update(&mut self, c: usize) {
        let dt = self.time.deltaTime;
        let r = &mut self.tr.cosmetic_rotation_behaviour[c];
        if r.ShouldRotate {
            r.vel = mathf::lerp(r.vel, r.RotationSpeedTarget, mathf::lerp_factor_dt(r.Acceleration, dt));
        }
        r.vel = mathf::lerp(r.vel, 0.0, mathf::lerp_factor_dt(1.0 - r.Dampening, dt));
        r.rot = (d(r.rot) + d(r.vel) * dt).f();
        // transform.localEulerAngles about Axis = rot: presentation.
        rt::presentation_pose(Xf::this(r.this_physical, r.this_physics));
    }

    /// CosmeticRotationBehaviour.get_CurrentSpeed
    pub fn cosmetic_rotation_behaviour_get_current_speed(&self, c: usize) -> f32 {
        self.tr.cosmetic_rotation_behaviour[c].vel
    }
}
