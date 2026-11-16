//! Extras (not the game's): a portal shot that hits a person (a human or an android) misfires instead of opening a
//! portal. One of the mishaps switched on in Settings -> Mods, at random (none on: the shot fizzles):
//! - Superposition: a flickering, see-through copy of the person at the other portal (or a spot nearby) that moves
//!   as one with them, damage to either hurting both. After about five seconds it collapses: usually back into one
//!   (dazed, glitching), sometimes into a clone, rarely the copies are pulled together across the gap until they
//!   tear apart. Hitting either again while split collapses it at once, with a small explosion.
//! - Spatial fold: the hit limb (or one hanging from it) and everything below it come out of another limb through a
//!   pair of tiny portals, still joined: its hinge now runs through the fold to that limb, keeping its bend.
//! - Phase loss: the person drifts and sinks through everything, each limb snapping back into reality at a random
//!   moment; a limb that comes back inside something is crushed (or bursts out of it).
//! The effects and sounds are the game's, tinted to the shot's colour; extras/src/mishap.rs has the pure part.

use super::Game;
use crate::b2::math::Vec2;
use crate::b2::world::{BodyId, JointId};
use ppg_extras::mishap::{self, Collapse, Effect};
pub use super::mishap_super::Split;
pub use super::mishap_phase::Phase;

pub(super) fn arr(a: Vec2) -> [f32; 2] {
    [a.x, a.y]
}
pub(super) fn vec(a: [f32; 2]) -> Vec2 {
    Vec2::new(a[0], a[1])
}

/// The game's effects and sounds the mishaps use (Resources prefabs and AudioClips; read at runtime).
pub(super) const SPLIT_FX: &str = "Prefabs/Exotic flash";
pub(super) const MOTES: &str = "Prefabs/SpawnParticles";
pub(super) const MERGE_FX: &str = "Prefabs/RaygunExplosion";
pub(super) const TEAR_FX: &str = "Prefabs/PulseExplosion";
const FOLD_FX: &str = "Prefabs/BrokenElectronicsSpark";
/// (clip, volume, pitch)
pub(super) const SPLIT_SOUND: (&str, f32, f32) = ("energy_sword_enable", 0.8, 0.6);
pub(super) const MERGE_SOUND: (&str, f32, f32) = ("energy_sword_disable", 0.8, 0.7);
const FOLD_SOUND: (&str, f32, f32) = ("ion_fire", 0.7, 0.55);
/// The mishaps' AudioSources, after the portal gun's four (extras_glue's SOURCES + k).
pub(super) const SOUND: i32 = 8;

#[derive(Default)]
pub struct Mishaps {
    pub splits: Vec<Split>,
    pub folds: Vec<Fold>,
    pub phases: Vec<Phase>,
    /// The glitching after a merge: (person, colour, until).
    pub dazed: Vec<(usize, usize, f32)>,
    /// Flashes of light for the drawing: (where, colour, size).
    pub flashes: Vec<([f32; 2], usize, f32)>,
    /// The test scenes' choice of effect and of how a superposition ends.
    pub force: Option<Effect>,
    pub force_collapse: Option<Collapse>,
    rng: u32,
}


pub struct Fold {
    pub colour: usize,
    joint: JointId,
    child: BodyId,
    /// Where the limb went in (on its own parent) and where it comes out (on the host): body, local point, local normal.
    pub entry: (BodyId, Vec2, Vec2),
    pub exit: (BodyId, Vec2, Vec2),
}

impl Game {
    pub(super) fn mishap_rand(&mut self) -> f32 {
        let m = &mut self.mishap;
        if m.rng == 0 {
            m.rng = 0x9E37_79B9 ^ (self.time.frameCount as u32).wrapping_mul(2654435761);
        }
        m.rng = m.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (m.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    pub(super) fn mishap_body(&self, l: usize) -> Option<BodyId> {
        let p = &self.physicals[self.limbs[l].PhysicalBehaviour];
        (!p.destroyed && !p.isDisintegrated && self.world.bodies.get(p.rigidbody.body).is_some_and(|b| b.is_some())).then_some(p.rigidbody.body)
    }

    /// The live limbs' centre (mean of their centres of mass) and half extents.
    pub(super) fn mishap_extent(&self, person: usize) -> (Vec2, Vec2) {
        let cs: Vec<Vec2> = self.people[person].Limbs.iter().filter_map(|&l| self.mishap_body(l)).map(|b| self.world.body(b).sweep.c).collect();
        if cs.is_empty() {
            return (Vec2::ZERO, Vec2::ZERO);
        }
        let (mut lo, mut hi) = (cs[0], cs[0]);
        for c in &cs {
            lo = Vec2::new(lo.x.min(c.x), lo.y.min(c.y));
            hi = Vec2::new(hi.x.max(c.x), hi.y.max(c.y));
        }
        ((lo + hi) * 0.5, (hi - lo) * 0.5 + Vec2::new(0.3, 0.3))
    }

    pub(super) fn mishap_fx(&mut self, path: &str, at: Vec2, scale: f32, colour: usize) {
        let a = self.mishap_rand() * std::f32::consts::TAU;
        self.extras_effect(path, arr(at), a, scale, colour);
    }

    /// The portal shot (extras_glue's extras_place) hit `body`: a person's limb misfires. None: not a person;
    /// Some(false): no mishap is on, the shot fizzles.
    pub(crate) fn mishap_hit(&mut self, colour: usize, body: BodyId, point: [f32; 2]) -> Option<bool> {
        let l = self.limb_of_body(body)?;
        let person = self.limbs[l].Person;
        if self.people[person].destroyed {
            return None;
        }
        if let Some(i) = self.mishap.splits.iter().position(|s| s.person == person || s.copy == Some(person)) {
            self.mishap_violent(i);
            return Some(true);
        }
        let on = [self.prefs.PortalMishapSuperposition, self.prefs.PortalMishapFold, self.prefs.PortalMishapPhase];
        let r = self.mishap_rand();
        let Some(effect) = self.mishap.force.or(mishap::pick(on, r)) else { return Some(false) };
        match effect {
            Effect::Superposition => self.mishap_split(person, colour),
            Effect::Fold => {
                // Its own flashes at the rims (the hit's smoke would hide them).
                return Some(self.mishap_fold(l, colour));
            }
            Effect::Phase => self.phase_start(person, colour),
        }
        self.extras_effect(FOLD_FX, point, 0.0, 0.6, colour);
        Some(true)
    }

    // --- Superposition ---------------------------------------------------------------------------------------------







    // --- Spatial fold ------------------------------------------------------------------------------------------------

    // In its own file: mishap_fold.rs.
}

#[path = "mishap_fold.rs"]
mod fold;

impl Game {
    // --- Every step and frame --------------------------------------------------------------------------------------

    /// Before b2World::Step: the copies move with their people, the tearing copies are pulled together, the phased drift.
    pub(super) fn mishap_pre_step(&mut self) {
        let m = &self.mishap;
        if m.splits.is_empty() && m.phases.is_empty() {
            return;
        }
        self.super_pre_step();
        self.phase_pre_step();
    }

    /// Once a frame, after the scripts' Update: copies appear, share their damage and collapse; folds whose hinge is
    /// gone end; phased limbs snap back.
    pub(crate) fn mishap_update(&mut self) {
        let m = &self.mishap;
        if m.splits.is_empty() && m.folds.is_empty() && m.phases.is_empty() && m.dazed.is_empty() {
            return;
        }
        let now = self.time.time;
        self.mishap.dazed.retain(|d| d.2 > now);
        self.super_update(now);
        // A fold ends with its hinge (broken, or made again by the game) or the host.
        let alive = |g: &Game, f: &Fold| {
            let host_ok = g.world.bodies.get(f.exit.0).is_some_and(|b| b.is_some()) && g.world.bodies.get(f.entry.0).is_some_and(|b| b.is_some());
            host_ok && matches!(g.world.joints.get(f.joint), Some(Some(j)) if (j.body_a == f.exit.0 || j.body_b == f.exit.0) && (j.body_a == f.child || j.body_b == f.child))
        };
        let folds = std::mem::take(&mut self.mishap.folds);
        self.mishap.folds = folds.into_iter().filter(|f| alive(self, f)).collect();
        self.phase_update();
    }


    /// The limbs drawn see-through (a split's both sides, the phased) and the dazed: limb -> (colour, ghost).
    pub fn mishap_looks(&self) -> std::collections::HashMap<usize, (usize, bool)> {
        let mut out = std::collections::HashMap::new();
        for &(person, colour, _) in &self.mishap.dazed {
            for &l in &self.people[person].Limbs {
                out.insert(l, (colour, false));
            }
        }
        for s in &self.mishap.splits {
            for (a, c) in self.mishap_pairs(s) {
                out.insert(a, (s.colour, true));
                out.insert(c, (s.colour, true));
            }
        }
        for p in &self.mishap.phases {
            for pl in &p.limbs {
                out.insert(pl.limb, (p.colour, true));
            }
        }
        out
    }

}

