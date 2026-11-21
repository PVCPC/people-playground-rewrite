//! HoverThrusterBehaviour: while Activated and dynamic, each hover point casts down along -transform.up and pushes the
//! body up by how far inside its calibrated height the ground is, damping the velocity and pushing the hit body back.
//! Channels 1 and 2 raise and lower the height. The light, glow and particles' emission follow Activated.
#![allow(non_snake_case)]

use super::physical::dot2;
use super::{d, mathf, script, Game, V};

/// HoverThrusterBehaviour.MinHeight and MaxHeight.
const MIN_HEIGHT: f32 = 0.6068182;
const MAX_HEIGHT: f32 = 12136.364;

script! {
    HoverThruster {
        BaseHoverHeight: f32 = 1.2136364,
        ForceMultiplier: f32 = 1.0,
        FactorPower: f32 = 2.0,
        VelocityRetention: f32 = 0.99,
        Activated: bool = false,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// HoverPoints[i].localPosition (children of the thruster's object, without physics).
        points: Vec<[f32; 3]> = Vec::new(),
        /// Layers.m_Bits
        mask: u32 = 0,
    }
}

impl Game {
    /// HoverThrusterBehaviour.Use (when enabled): channel 1 raises, 2 lowers, any other toggles Activated; then clamped.
    pub fn hover_thruster_use(&mut self, h: usize, channel: i32) {
        let s = &mut self.hover_thrusters[h];
        if !s.enabled {
            return;
        }
        match channel {
            1 => s.BaseHoverHeight += 0.08239701,
            2 => s.BaseHoverHeight -= 0.08239701,
            _ => {
                s.Activated = !s.Activated;
                self.hover_thruster_update_activation(h);
            }
        }
        let s = &mut self.hover_thrusters[h];
        s.BaseHoverHeight = mathf::clamp(s.BaseHoverHeight, MIN_HEIGHT, MAX_HEIGHT);
    }

    /// IdleLoop (a null one throws).
    fn hover_thruster_idle(&self, h: usize) -> i32 {
        self.audio.source_field(self.hover_thrusters[h].id, "IdleLoop").unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// HoverThrusterBehaviour.UpdateActivation: IdleLoop plays, and the Effects emission, Light and GlowSprite are on,
    /// while Activated. Start and OnEnable run it too.
    pub fn hover_thruster_update_activation(&mut self, h: usize) {
        // gapC r11: Effects.emission.enabled, Light.enabled (a LightSprite) and GlowSprite.enabled = Activated.
        let (id, on) = (self.hover_thrusters[h].id, self.hover_thrusters[h].Activated);
        self.ps_set_emission(id, "Effects", on);
        if let Some(l) = self.field_component(id, "Light") {
            self.set_item_enabled("LightSprite", l, on);
        }
        if let Some(at) = self.field_component(id, "GlowSprite").and_then(|s| self.item_sprite(s)) {
            self.set_sprite_enabled(at, on);
        }
        let src = self.hover_thruster_idle(h);
        if on { self.audio.play(src) } else { self.audio.stop(src) }
    }

    /// HoverThrusterBehaviour.Update: held continuous use on channel 1 or 2 moves the height at 4.11985 per second.
    pub fn hover_thruster_update(&mut self, h: usize) {
        let p = self.hover_thrusters[h].physical;
        let dt = self.time.deltaTime;
        let (up, down) = (self.used_continuously_on(p, 1), self.used_continuously_on(p, 2));
        let s = &mut self.hover_thrusters[h];
        if up {
            s.BaseHoverHeight = (d(s.BaseHoverHeight) + d(4.11985f32) * dt).f();
        }
        if down {
            s.BaseHoverHeight = (d(s.BaseHoverHeight) - d(4.11985f32) * dt).f();
        }
        s.BaseHoverHeight = mathf::clamp(s.BaseHoverHeight, MIN_HEIGHT, MAX_HEIGHT);
    }

    /// HoverThrusterBehaviour.OnDisable: Activated = false, UpdateActivation.
    pub fn hover_thruster_on_disable(&mut self, h: usize) {
        self.hover_thrusters[h].Activated = false;
        self.hover_thruster_update_activation(h);
    }

    /// HoverThrusterBehaviour.FixedUpdate: Hover when the body is dynamic and Activated.
    pub fn hover_thruster_fixed_update(&mut self, h: usize) {
        let p = self.hover_thrusters[h].physical;
        if self.physicals[p].rigidbody.bodyType == 0 && self.hover_thrusters[h].Activated {
            self.hover_thruster_hover(h);
        }
    }

    /// HoverThrusterBehaviour.Hover
    pub(crate) fn hover_thruster_hover(&mut self, h: usize) {
        let s = &self.hover_thrusters[h];
        let (p, mask, factor, keep) = (s.physical, s.mask, s.FactorPower, s.VelocityRetention);
        let points = s.points.clone();
        let num = (d(mathf::pow(s.BaseHoverHeight, 1.07)) * 1.7f32).f();
        let calibrated: Vec<f32> = points.iter().map(|l| (d(num) + l[1]).f()).collect();
        let own = s.own.clone();
        let up = self.xf_up(&own);
        let vector = [-up[0], -up[1]];
        let num2 = (d(s.ForceMultiplier) + d(self.physicals[p].Charge()) * 4.0f32).f();
        for (j, local) in points.iter().enumerate() {
            let num3 = calibrated[j];
            let pos = self.xf_transform_point(&own, *local);
            let Some(hit) = self.raycast(V::new(pos[0], pos[1]), vector, (d(num3) * 2.0f32).f(), mask, f32::NEG_INFINITY, f32::INFINITY) else {
                continue;
            };
            let clamped = mathf::clamp01(((d(num3) - hit.distance) / num3).f());
            let mut num4 = clamped * dot2([hit.normal.x, hit.normal.y], [up[0], up[1]]);
            if num4 <= mathf::EPSILON {
                continue;
            }
            num4 = mathf::pow(num4, factor);
            let k = num2 * num4;
            let vector2 = [k * vector[0], k * vector[1]];
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force_at_position(&mut self.world, [vector2[0] * -1.0, vector2[1] * -1.0], [pos[0], pos[1]]);
            let v = rb.velocity(&self.world);
            rb.set_velocity(&mut self.world, [v[0] * keep, v[1] * keep]);
            let av = rb.angular_velocity(&self.world);
            rb.set_angular_velocity(&mut self.world, av * keep);
            if let Some(o) = self.physical_of_collider(hit.collider) {
                let div = mathf::max(1.0, hit.distance * hit.distance);
                let other = self.physicals[o].rigidbody.clone();
                other.add_force_at_position(&mut self.world, [vector2[0] / div, vector2[1] / div], [hit.point.x, hit.point.y]);
            }
        }
    }
}

script! {
    Hovercar {
        HoverHeight: f32 = 1.189,
        ForceMultiplier: f32 = 1.0,
        FactorPower: f32 = 2.0,
        VelocityRetention: f32 = 0.99,
        ParkForceMultiplier: f32 = 1.0,
        BaseSpeed: f32 = 1.0,
        /// Fan is null: Update, OnEnable and OnDisable write it first.
        fan_null: bool = false,
        Activated: bool = false,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// HoverPoints[i].localPosition (children of the car's object, without physics).
        points: Vec<[f32; 3]> = Vec::new(),
        /// Layers.m_Bits
        mask: u32 = 0,
        /// The private isOnGround.
        isOnGround: bool = false,
        /// Fan: the CosmeticRotationBehaviour's instance id (0 when null).
        fan: i32 = 0,
    }
}

impl Game {
    /// IdleLoop and AccLoop (a null one throws).
    fn hovercar_sources(&self, h: usize) -> (i32, i32) {
        let id = self.hovercars[h].id;
        let get = |n: &str| self.audio.source_field(id, n).unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        (get("IdleLoop"), get("AccLoop"))
    }

    /// HovercarBehaviour.Start's and OnEnable's IdleLoop.Play(); AccLoop.Play().
    pub fn hovercar_play_loops(&mut self, h: usize) {
        let (idle, acc) = self.hovercar_sources(h);
        self.audio.play(idle);
        self.audio.play(acc);
    }

    /// HovercarBehaviour.OnEnable: Fan.ShouldRotate = true, then the loops play.
    pub fn hovercar_on_enable(&mut self, h: usize) {
        if self.hovercars[h].fan_null {
            super::menu::npe();
        }
        self.hovercar_fan_should_rotate(h, true);
        self.hovercar_play_loops(h);
    }

    /// HovercarBehaviour.OnDisable: Fan.ShouldRotate = false, then the loops stop.
    pub fn hovercar_on_disable(&mut self, h: usize) {
        if self.hovercars[h].fan_null {
            super::menu::npe();
        }
        self.hovercar_fan_should_rotate(h, false);
        self.hovercar_stop_loops(h);
    }

    /// Fan.ShouldRotate = v.
    fn hovercar_fan_should_rotate(&mut self, h: usize, v: bool) {
        let fan = self.hovercars[h].fan;
        if let Some(r) = self.tr.cosmetic_rotation_behaviour.iter_mut().find(|r| r.this_id == fan) {
            r.ShouldRotate = v;
        }
    }

    /// HovercarBehaviour.OnDisable: IdleLoop.Stop(); AccLoop.Stop().
    pub fn hovercar_stop_loops(&mut self, h: usize) {
        let (idle, acc) = self.hovercar_sources(h);
        self.audio.stop(idle);
        self.audio.stop(acc);
    }

    /// HovercarBehaviour.Update: Fan.RotationSpeedTarget = -1500 - Charge * 150, then AccLoop.volume follows the drive.
    pub fn hovercar_update(&mut self, h: usize) {
        if self.hovercars[h].fan_null {
            super::menu::npe();
        }
        let (fan, charge) = (self.hovercars[h].fan, self.physicals[self.hovercars[h].physical].Charge());
        if let Some(r) = self.tr.cosmetic_rotation_behaviour.iter_mut().find(|r| r.this_id == fan) {
            r.RotationSpeedTarget = (-1500.0f64 - charge as f64 * 150.0) as f32;
        }
        let (_, acc) = self.hovercar_sources(h);
        let target = if self.hovercars[h].isOnGround && self.hovercars[h].Activated { 1.0 } else { 0.0 };
        let f = mathf::lerp_factor_dt(0.9, self.time.deltaTime);
        let a = self.audio.get_mut(acc);
        a.set_volume(mathf::lerp(a.volume, target, f));
    }

    /// HovercarBehaviour.Use (when enabled): channel 1 reverses BaseSpeed, any other toggles Activated.
    pub fn hovercar_use(&mut self, h: usize, channel: i32) {
        let s = &mut self.hovercars[h];
        if s.enabled {
            if channel == 1 {
                s.BaseSpeed = 0.0 - s.BaseSpeed;
            } else {
                s.Activated = !s.Activated;
            }
        }
    }

    /// HovercarBehaviour.GetTargetForceVector: (lossyScale.x < 0 ? -1 : 1) * (BaseSpeed + Charge) * transform.right.
    pub(crate) fn hovercar_target_force(&self, h: usize) -> [f32; 2] {
        let p = self.hovercars[h].physical;
        let sign: f32 = if self.xf_lossy_scale(&self.hovercars[h].own)[0] < 0.0 { -1.0 } else { 1.0 };
        let k = (d(sign) * (d(self.hovercars[h].BaseSpeed) + self.physicals[p].Charge())).f();
        let r = self.xf_right(&self.hovercars[h].own);
        [k * r[0], k * r[1]]
    }

    /// HovercarBehaviour.FixedUpdate: Hover while dynamic, then the drive force when Activated and on the ground.
    pub fn hovercar_fixed_update(&mut self, h: usize) {
        let p = self.hovercars[h].physical;
        if self.physicals[p].rigidbody.bodyType == 0 {
            self.hovercar_hover(h);
            if self.hovercars[h].Activated && self.hovercars[h].isOnGround {
                let f = self.hovercar_target_force(h);
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_force(&mut self.world, f);
            }
        }
    }

    /// HovercarBehaviour.Hover. calibratedHoverHeights is fixed at Awake (HoverHeight + each point's localPosition.y).
    pub(crate) fn hovercar_hover(&mut self, h: usize) {
        let s = &self.hovercars[h];
        let (p, mask, factor, keep, force, park, height) = (s.physical, s.mask, s.FactorPower, s.VelocityRetention, s.ForceMultiplier, s.ParkForceMultiplier, s.HoverHeight);
        let points = s.points.clone();
        let own = s.own.clone();
        let up = self.xf_up(&own);
        let vector = [-up[0], -up[1]];
        self.hovercars[h].isOnGround = false;
        for local in &points {
            let num = (d(height) + local[1]).f();
            let pos = self.xf_transform_point(&own, *local);
            let Some(hit) = self.raycast(V::new(pos[0], pos[1]), vector, (d(num) * 2.0f32).f(), mask, f32::NEG_INFINITY, f32::INFINITY) else {
                continue;
            };
            let mut num2 = mathf::clamp01(((d(num) - hit.distance) / num).f()) * dot2([hit.normal.x, hit.normal.y], [up[0], up[1]]);
            if num2 <= mathf::EPSILON {
                continue;
            }
            self.hovercars[h].isOnGround = true;
            num2 = mathf::pow(num2, factor);
            let k = force * num2;
            let vector2 = [k * vector[0], k * vector[1]];
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force_at_position(&mut self.world, [vector2[0] * -1.0, vector2[1] * -1.0], [pos[0], pos[1]]);
            let v = rb.velocity(&self.world);
            rb.set_velocity(&mut self.world, [v[0] * keep, v[1] * keep]);
            let av = rb.angular_velocity(&self.world);
            rb.set_angular_velocity(&mut self.world, av * keep);
            let Some(o) = self.physical_of_collider(hit.collider) else { continue };
            let point = [hit.point.x, hit.point.y];
            let other = self.physicals[o].rigidbody.clone();
            other.add_force_at_position(&mut self.world, vector2, point);
            if self.hovercars[h].Activated {
                let f = self.hovercar_target_force(h);
                other.add_force_at_position(&mut self.world, [f[0] * -0.25, f[1] * -0.25], point);
                continue;
            }
            let a = if other.bodyType == 0 { other.point_velocity(&self.world, point) } else { [0.0; 2] };
            let b = rb.point_velocity(&self.world, point);
            let v3 = [a[0] - b[0], a[1] - b[1]];
            rb.add_force_at_position(&mut self.world, [v3[0] * park, v3[1] * park], [pos[0], pos[1]]);
        }
    }

    // HoverThrusterBehaviour.Start: the "setHoverThrusterRange" button is menu.rs (Entry::HoverHeight, Clamp(v * 1.2136364, 0.6068182, 12136.364)); Start's UpdateActivation is hover_thruster_update_activation.
    // HoverThrusterBehaviour.UpdateActivation: hover_thruster_update_activation (emission, Light and GlowSprite too).
    // HoverThrusterBehaviour.Awake: calibratedHoverHeights is only a scratch array; Hover computes each height itself.
    // HoverThrusterBehaviour.OnEnable: hover_thruster_update_activation.
    // HovercarBehaviour.Start: the "reverseCar" button is menu.rs (Menu::Hovercar, HovercarReverse); its IdleLoop.Play and AccLoop.Play are hovercar_play_loops.
    // HovercarBehaviour.Awake: calibratedHoverHeights (HoverHeight + each point's localPosition.y) is computed per hover in hovercar_hover.
}
