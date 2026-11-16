//! AutomaticSentryController (CeilingTurretBehaviour; the sentry turret and laser turret use it too). GetTargetMotorSpeed
//! scans for limbs in sight every 0.5 s (a target stack: torsos at the end, the rest at the front), keeps the current
//! target while it lives, is in SightRange and has a line of sight, and returns the motor speed that turns the barrel
//! towards it, clamped to ±120. OnSight (a new person) and OnShoot (the barrel is on target) are returned as the events
//! the call invokes, in order; the owner runs their listeners after the call, which reads none of the state they change.
#![allow(non_snake_case)]

use super::limb::TORSO;
use super::{d, mathf, Game};
use crate::b2::math::Vec2 as V;

/// The scan interval (`interval` in the controller).
const SCAN_INTERVAL: f32 = 0.5;

#[derive(Clone, Debug)]
pub struct AutomaticSentry {
    pub SightRange: f32,
    pub LayerMask: u32,
    pub AimFuzziness: f32,
    pub MaxAngle: f32,
    /// The limbs found in sight, as `Game::limbs` indices.
    pub targetStack: Vec<usize>,
    pub currentTarget: Option<usize>,
    /// The time since the last scan (`t`).
    pub t: f32,
    /// The `Game::people` index of the last OnSight (None: none yet).
    pub previousPerson: Option<usize>,
}

impl Default for AutomaticSentry {
    fn default() -> Self {
        Self { SightRange: 20.0, LayerMask: 0, AimFuzziness: 0.0005, MaxAngle: 180.0, targetStack: Vec::new(), currentTarget: None, t: 0.0, previousPerson: None }
    }
}

/// The UnityEvents the controller invokes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SentryEvent {
    /// OnSight: the target's person is not the one of the last sight.
    Sight,
    /// OnShoot: the barrel points at the target within AimFuzziness.
    Shoot,
}

use super::stab::vector2_angle as angle;

/// Vector2.SignedAngle: the angle's sign is the cross product's (Mathf.Sign(0) is 1).
pub(super) fn signed_angle(from: [f32; 2], to: [f32; 2]) -> f32 {
    let cross = (d(from[0]) * to[1] - d(from[1]) * to[0]).f();
    let sign = if cross >= 0.0 { 1.0 } else { -1.0 };
    angle(from, to) * sign
}

impl Game {
    /// AutomaticSentryController.get_HasTarget: `(bool)currentTarget`.
    pub fn sentry_has_target(&self, s: &AutomaticSentry) -> bool {
        s.currentTarget.is_some_and(|l| !self.sentry_limb_destroyed(l))
    }

    /// `(bool)limb` is false once the limb's GameObject is destroyed.
    fn sentry_limb_destroyed(&self, l: usize) -> bool {
        self.physicals[self.limbs[l].PhysicalBehaviour].destroyed
    }

    /// AutomaticSentryController.skip: RemoveAt(Count - 1) and the current target cleared.
    fn sentry_skip(s: &mut AutomaticSentry) {
        if s.targetStack.pop().is_none() {
            crate::game::menu::throw("ArgumentOutOfRangeException: Index was out of range. Must be non-negative and less than the size of the collection.
Parameter name: index");
        }
        s.currentTarget = None;
    }

    /// GetTargetMotorSpeed(transform, barrelPos, barrelDir, out motorSpeed) with `transform` the firearm's physical object.
    /// Returns the motor speed and the events invoked.
    pub fn sentry_get_target_motor_speed(&mut self, s: &mut AutomaticSentry, transform: usize, barrel_pos: [f32; 2], barrel_dir: [f32; 2], delta_time: f32) -> (f32, Vec<SentryEvent>) {
        let mut events = Vec::new();
        let mut motor = 0.0f32;
        if s.currentTarget.is_some_and(|l| self.sentry_limb_destroyed(l)) {
            s.currentTarget = None;
        }
        if s.t > SCAN_INTERVAL && s.currentTarget.is_none() {
            s.t = 0.0;
            let root = self.root_id(transform);
            let hits = self.overlap_circle(V::new(barrel_pos[0], barrel_pos[1]), s.SightRange, s.LayerMask);
            // OverlapCircleNonAlloc fills the 256-entry buffer at most.
            for (victim, _) in hits.into_iter().take(256) {
                // A collider without a PhysicalBehaviour has no LimbBehaviour (the GameObjects of limbs have one).
                let Some(p) = victim else { continue };
                if self.root_id(p) == root {
                    continue;
                }
                let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == p) else { continue };
                if s.targetStack.contains(&l) || !self.limb_is_considered_alive(l) {
                    continue;
                }
                if !self.sentry_has_line_of_sight(s, l, transform, barrel_pos, barrel_dir) {
                    continue;
                }
                if self.limbs[l].RoughClassification == TORSO {
                    s.targetStack.push(l);
                } else {
                    s.targetStack.insert(0, l);
                }
            }
        } else {
            s.t += delta_time;
        }
        if s.targetStack.is_empty() {
            return (motor, events);
        }
        if s.currentTarget.is_none() {
            let l = *s.targetStack.last().unwrap();
            s.currentTarget = Some(l);
            if self.sentry_limb_destroyed(l) {
                Self::sentry_skip(s);
                return (motor, events);
            }
        }
        let cur = s.currentTarget.unwrap();
        let tp = self.physicals[self.limbs[cur].PhysicalBehaviour].transform.position;
        let sp = self.physicals[transform].transform.position;
        let (dx, dy) = (tp[0] - sp[0], tp[1] - sp[1]);
        // Vector2.Distance.
        let distance = (d(dx) * dx + d(dy) * dy).0.sqrt() as f32;
        if self.limb_is_considered_alive(cur) && distance < s.SightRange {
            if !self.sentry_has_line_of_sight(s, cur, transform, barrel_pos, barrel_dir) {
                Self::sentry_skip(s);
            } else {
                motor = self.sentry_motor_speed(s, cur, transform, barrel_dir, &mut events);
            }
        } else {
            Self::sentry_skip(s);
        }
        (motor, events)
    }

    /// AutomaticSentryController.HasLineOfSight(t, transform, worldspacePos, barrelDir): the angle is within MaxAngle, and the linecast's first five
    /// hits contain no AbsorbsLasers physical outside the target's and the turret's roots.
    pub(crate) fn sentry_has_line_of_sight(&mut self, s: &AutomaticSentry, l: usize, transform: usize, pos: [f32; 2], dir: [f32; 2]) -> bool {
        let target = self.limbs[l].PhysicalBehaviour;
        let tp = self.physicals[target].transform.position;
        let vector = [tp[0] - pos[0], tp[1] - pos[1]];
        if angle(vector, dir) > s.MaxAngle {
            return false;
        }
        // LinecastNonAlloc into losBuffer[5].
        let hits = self.linecast_hits(V::new(pos[0], pos[1]), V::new(tp[0], tp[1]), s.LayerMask, SENTRY_LOS_BUFFER);
        if hits.is_empty() {
            return false;
        }
        let (target_root, own_root) = (self.root_id(target), self.root_id(transform));
        for hit in &hits {
            // PhysicalObjectsInWorldByTransform[hit.transform] throws for a transform without a physical object; the
            // root tests that would skip it need the root of a collider without one, which is not modelled.
            let Some(hp) = self.physical_of_collider(hit.collider) else {
                crate::game::menu::throw("KeyNotFoundException: The given key was not present in the dictionary.");
            };
            let root = self.root_id(hp);
            if root != target_root && root != own_root && self.physicals[hp].AbsorbsLasers {
                return false;
            }
        }
        true
    }

    /// AutomaticSentryController.GetMotorSpeed(transform, worldspaceDir): OnSight when the person changes, OnShoot when the barrel is on target,
    /// and the clamped signed angle towards the target.
    pub(crate) fn sentry_motor_speed(&mut self, s: &mut AutomaticSentry, cur: usize, transform: usize, dir: [f32; 2], events: &mut Vec<SentryEvent>) -> f32 {
        let person = self.limbs[cur].Person;
        if s.previousPerson != Some(person) {
            events.push(SentryEvent::Sight);
            s.previousPerson = Some(person);
        }
        let tp = self.physicals[self.limbs[cur].PhysicalBehaviour].transform.position;
        let sp = self.physicals[transform].transform.position;
        // Vector3.normalized: the z difference counts in the length.
        let n = super::explosion::normalized3([tp[0] - sp[0], tp[1] - sp[1], tp[2] - sp[2]]);
        let vector = [n[0], n[1]];
        if super::physical::dot2(vector, dir) > 1.0 - s.AimFuzziness {
            events.push(SentryEvent::Shoot);
        }
        // IL `neg` (the decompiler's `0f - x`): -0 when the barrel is on target.
        mathf::clamp(-signed_angle(dir, vector), -120.0, 120.0)
    }
}

/// AutomaticSentryController.losBuffer: static RaycastHit2D[5] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const SENTRY_LOS_BUFFER: usize = 5;
