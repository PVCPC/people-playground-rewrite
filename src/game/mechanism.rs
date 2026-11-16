//! FunLittleGames.Linkage: mechanisms (one linked motion, X, Y or rotation) that Rigidbody2Ds share through their
//! LinkedBody components. Linkage is on no prefab or scene of the game.
//!
//! In a player build a Linkage never links a body: LinkedBody.rigid and .joints are only set by its OnValidate, which
//! is editor-only, so the LinkedBody that CreateLinkages adds has a null `rigid` and UpdateMechanismAxes throws before
//! the body joins `bodies`. `bodies` stays empty; every pass over it (GatherMassesAndForces, RedistributeForces,
//! GatherVelocitiesAndDisplacements, RedistributeVelocitiesAndDisplacements, and CreateLinkages' first and last
//! loops) does nothing, and LinkedBody.SumForcesAndTorques throws on its first contact (rigid.position).
#![allow(non_snake_case)]

use super::menu::LogType;
use super::{d, mathf, BodyOwner, Game};

/// LinkedBody.contactCache's length (its static setup: new ContactPoint2D[20]).
pub(crate) const CONTACT_CACHE: usize = 20;

/// Linkage.Link.Axis bits.
pub const LOCAL_X: i32 = 1;
pub const LOCAL_Y: i32 = 2;
pub const ROTATION: i32 = 4;
pub const WORLD_X: i32 = 8;
pub const WORLD_Y: i32 = 0x10;

/// Linkage.Link: `rigidBody` with its GameObject's name.
#[derive(Clone, Debug)]
pub struct Link {
    pub rigidBody: Option<(BodyOwner, String)>,
    pub axis: i32,
    pub leverage: f32,
}

/// The LinkedBody_NoManualAdd.LinkedBody fields CreateLinkages reads back on a body another Linkage gave one.
#[derive(Clone, Debug, Default)]
pub struct LinkedBody {
    pub linkedAxes: i32,
    pub leverageX: f32,
    pub mechanismX: i32,
    pub leverageY: f32,
    pub mechanismY: i32,
    pub leverageR: f32,
    pub mechanismR: i32,
    pub mechX: [f32; 2],
    pub mechY: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct Mechanism {
    pub name: String,
    pub links: Vec<Link>,
    pub drag: f32,
    pub externalForceModifier: f32,
    pub minimumLimit: f32,
    pub minimumLimitStiffness: f32,
    pub maximumLimit: f32,
    pub maximumLimitStiffness: f32,
    pub nudge: f32,
    pub tension: f32,
    pub drive: f32,
    pub clutch: f32,
    pub contributors: i32,
    pub mass: f32,
    pub force: f32,
    pub speed: f32,
    pub displacement: f32,
}

impl Default for Mechanism {
    /// Mechanism.constructor: the field initialisers (externalForceModifier = 1f, both limit stiffnesses 0.5f).
    fn default() -> Self {
        Mechanism {
            name: String::new(),
            links: Vec::new(),
            drag: 0.0,
            externalForceModifier: 1.0,
            minimumLimit: 0.0,
            minimumLimitStiffness: 0.5,
            maximumLimit: 0.0,
            maximumLimitStiffness: 0.5,
            nudge: 0.0,
            tension: 0.0,
            drive: 0.0,
            clutch: 0.0,
            contributors: 0,
            mass: 0.0,
            force: 0.0,
            speed: 0.0,
            displacement: 0.0,
        }
    }
}

impl Mechanism {
    /// Mechanism.PreUpdate
    pub fn pre_update(&mut self) {
        self.force = 0.0;
        self.speed = 0.0;
        self.displacement = 0.0;
        self.mass = 0.0;
        self.contributors = 0;
    }

    /// Mechanism.ScaleForceApplyNudgeAndTension
    pub fn scale_force_apply_nudge_and_tension(&mut self) {
        self.force = (d(self.force) * self.externalForceModifier).f();
        self.force = (d(self.force) + (d(self.nudge) + self.tension)).f();
        self.nudge = 0.0;
    }

    /// Mechanism.AverageSpeedDisplacementApplyDragLimitsAndDrive (`fixed_delta_time` is Time.fixedDeltaTime)
    pub fn average_speed_displacement_apply_drag_limits_and_drive(&mut self, fixed_delta_time: f32) {
        self.speed /= self.contributors as f32;
        self.displacement /= self.contributors as f32;
        if self.minimumLimit != 0.0 || self.maximumLimit != 0.0 {
            if self.displacement < self.minimumLimit {
                self.displacement = mathf::lerp(self.displacement, self.minimumLimit, self.minimumLimitStiffness);
                if self.speed <= 0.0 {
                    self.speed = 0.0;
                }
            }
            if self.displacement > self.maximumLimit {
                self.displacement = mathf::lerp(self.displacement, self.maximumLimit, self.maximumLimitStiffness);
                if self.speed >= 0.0 {
                    self.speed = 0.0;
                }
            }
        }
        self.speed = (d(self.speed) * (d(1.0) - d(self.drag) * fixed_delta_time)).f();
        self.speed = mathf::lerp(self.speed, self.drive, self.clutch);
    }
}

/// A Linkage component: its mechanisms and Awake's name lookup.
#[derive(Clone, Debug, Default)]
pub struct Linkage {
    pub mechanisms: Vec<Mechanism>,
    pub mechDict: std::collections::HashMap<String, usize>,
}

impl Linkage {
    /// Linkage.GetMechanism
    pub fn get_mechanism(&self, name: &str) -> Option<usize> {
        self.mechDict.get(name).copied()
    }

    /// The ProcessForces coroutine (started by OnEnable) after each WaitForFixedUpdate: PrepareMechanisms,
    /// GatherMassesAndForces, RedistributeForces, GatherVelocitiesAndDisplacements and
    /// RedistributeVelocitiesAndDisplacements over no bodies (contributors 0: speed and displacement become 0 / 0).
    pub fn process_forces(&mut self, fixed_delta_time: f32) {
        // Linkage.ProcessForces: the loop body after `yield return new WaitForFixedUpdate()`.
        // Linkage.PrepareMechanisms
        for m in &mut self.mechanisms {
            m.pre_update();
        }
        // Linkage.GatherMassesAndForces: no bodies, then each mechanism's ScaleForceApplyNudgeAndTension.
        for m in &mut self.mechanisms {
            m.scale_force_apply_nudge_and_tension();
        }
        // Linkage.RedistributeForces: no bodies.
        // Linkage.GatherVelocitiesAndDisplacements: no bodies, then each AverageSpeedDisplacementApplyDragLimitsAndDrive.
        for m in &mut self.mechanisms {
            m.average_speed_displacement_apply_drag_limits_and_drive(fixed_delta_time);
        }
        // Linkage.RedistributeVelocitiesAndDisplacements: no bodies.
    }
}

impl Game {
    /// Linkage.Awake: named mechanisms into mechDict (a later name wins), then CreateLinkages. OnValidate (editor
    /// only) and the empty FixedUpdate do nothing in a player.
    pub fn linkage_awake(&mut self, l: &mut Linkage) {
        // Linkage.OnValidate and LinkedBody.OnValidate: editor only. Linkage.FixedUpdate, LinkedBody.FixedUpdate: empty.
        // Linkage.OnEnable: StartCoroutine(ProcessForces()), see Linkage::process_forces.
        for (i, m) in l.mechanisms.iter().enumerate() {
            if !m.name.is_empty() {
                l.mechDict.insert(m.name.clone(), i);
            }
        }
        self.linkage_create_linkages(l);
    }

    /// Linkage.CreateLinkages: each link's errors, and the axes and leverages it sets on a body that already has a
    /// LinkedBody; a body without one gets it from AddComponent and throws (see the module notes).
    fn linkage_create_linkages(&mut self, l: &Linkage) {
        let err = |g: &mut Game, s: String| g.log_message_received(&s, LogType::Error);
        for (i, m) in l.mechanisms.iter().enumerate() {
            for link in &m.links {
                let Some((owner, rb_name)) = &link.rigidBody else {
                    err(self, format!("LINKAGE: Rigid body left blank in channel {i}"));
                    continue;
                };
                if link.leverage == 0.0 {
                    err(self, format!("LINKAGE: Leverage of {rb_name} set to 0 in channel {i}"));
                    continue;
                }
                let Some(k) = self.linked_bodies.iter().position(|(o, _)| o == owner) else {
                    self.linked_bodies.push((*owner, LinkedBody::default()));
                    // LinkedBody.UpdateMechanismAxes: Quaternion.Euler(0, 0, rigid.rotation) on the null rigid.
                    crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
                };
                let mut b = std::mem::take(&mut self.linked_bodies[k].1);
                let i = i as i32;
                b.linkedAxes |= link.axis;
                let both = format!("LINKAGE: Cannot link both World and Local axes of {rb_name}");
                'link: {
                    if link.axis & LOCAL_X != 0 {
                        if b.linkedAxes & (WORLD_X | WORLD_Y) != 0 {
                            err(self, both);
                            break 'link;
                        }
                        if b.leverageX != 0.0 {
                            err(self, format!("LINKAGE: Local X-axis of {rb_name} bound to more than one channel."));
                        } else {
                            (b.leverageX, b.mechanismX) = (link.leverage, i);
                        }
                    }
                    if link.axis & WORLD_X != 0 {
                        if b.linkedAxes & (LOCAL_X | LOCAL_Y) != 0 {
                            err(self, both);
                            break 'link;
                        }
                        if b.leverageX != 0.0 {
                            err(self, format!("LINKAGE: Local X-axis of {rb_name} bound to more than one channel."));
                        } else {
                            (b.mechX, b.mechY, b.leverageX, b.mechanismX) = ([1.0, 0.0], [0.0, 1.0], link.leverage, i);
                        }
                    }
                    if link.axis & LOCAL_Y != 0 {
                        if b.linkedAxes & (WORLD_X | WORLD_Y) != 0 {
                            err(self, both);
                            break 'link;
                        }
                        if b.leverageY != 0.0 {
                            err(self, format!("LINKAGE: Local Y-axis of {rb_name} bound to more than one channel."));
                        } else {
                            (b.leverageY, b.mechanismY) = (link.leverage, i);
                        }
                    }
                    if link.axis & WORLD_Y != 0 {
                        if b.linkedAxes & (LOCAL_X | LOCAL_Y) != 0 {
                            err(self, both);
                            break 'link;
                        }
                        if b.leverageY != 0.0 {
                            err(self, format!("LINKAGE: Local Y-axis of {rb_name} bound to more than one channel."));
                        } else {
                            (b.mechX, b.mechY, b.leverageY, b.mechanismY) = ([1.0, 0.0], [0.0, 1.0], link.leverage, i);
                        }
                    }
                    if link.axis & ROTATION != 0 {
                        if b.leverageR != 0.0 {
                            err(self, format!("LINKAGE: Rotation of {rb_name} bound to more than one channel."));
                            break 'link;
                        }
                        // link.leverage * (PI / 180f), both f32.
                        (b.leverageR, b.mechanismR) = (link.leverage * (std::f32::consts::PI / 180.0), i);
                    }
                }
                self.linked_bodies[k].1 = b;
            }
        }
    }

    /// LinkedBody.OnCollisionEnter2D / OnCollisionStay2D -> SumForcesAndTorques with the collision's contact count:
    /// none sums nothing; the first contact reads the null `rigid` (LinkedBody.OnValidate, which sets it, is editor
    /// only). `contacts` is the collision's contact count; GetContacts(contactCache) fills at most CONTACT_CACHE of them.
    pub fn linked_body_sum_forces_and_torques(&mut self, contacts: usize) {
        let contacts = contacts.min(CONTACT_CACHE);
        if contacts > 0 {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanism_step() {
        let mut m = Mechanism { nudge: 2.0, tension: 1.0, force: 4.0, externalForceModifier: 0.5, ..Default::default() };
        m.scale_force_apply_nudge_and_tension();
        assert_eq!((m.force, m.nudge), (5.0, 0.0));
        m.pre_update();
        assert_eq!((m.force, m.contributors), (0.0, 0));
        // Below the minimum limit the displacement is pulled up by the stiffness and a falling speed stops.
        let mut m = Mechanism { minimumLimit: 2.0, maximumLimit: 10.0, contributors: 2, speed: -2.0, displacement: 0.0, ..Default::default() };
        m.average_speed_displacement_apply_drag_limits_and_drive(0.02);
        assert_eq!((m.displacement, m.speed), (1.0, 0.0));
    }

    #[test]
    fn linkage_awake_logs_bad_links_and_reuses_a_linked_body() {
        let mut g = Game::default();
        let link = |rb: Option<(BodyOwner, String)>, axis, leverage| Link { rigidBody: rb, axis, leverage };
        let rb = Some((BodyOwner::Physical(0), "Arm".to_string()));
        g.linked_bodies.push((BodyOwner::Physical(0), LinkedBody::default()));
        let mut l = Linkage {
            mechanisms: vec![
                Mechanism { name: "a".into(), links: vec![link(None, LOCAL_X, 1.0), link(rb.clone(), LOCAL_X, 0.0), link(rb.clone(), WORLD_X, 2.0)], ..Default::default() },
                Mechanism { name: "a".into(), links: vec![link(rb.clone(), LOCAL_Y, 1.0), link(rb, ROTATION, 90.0)], ..Default::default() },
            ],
            ..Default::default()
        };
        g.linkage_awake(&mut l);
        assert_eq!(l.get_mechanism("a"), Some(1));
        let b = &g.linked_bodies[0].1;
        assert_eq!((b.leverageX, b.mechanismX, b.leverageY, b.linkedAxes), (2.0, 0, 0.0, WORLD_X | LOCAL_Y | ROTATION));
        assert_eq!((b.leverageR, b.mechanismR), (std::f32::consts::FRAC_PI_2, 1));
        l.process_forces(0.02);
        assert!(l.mechanisms[0].speed.is_nan());
    }
}
