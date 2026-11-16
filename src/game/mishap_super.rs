//! Extras (not the game's): the superposition portal mishap (mishap_glue.rs has the hit and the others). A
//! flickering, see-through copy of the person at the other portal (or a spot nearby) moves as one with them, damage to
//! either hurting both. After about five seconds it collapses: usually back into one (dazed, glitching), sometimes into
//! a clone, rarely the copies are pulled together across the gap until they tear apart. Hitting either again while
//! split collapses it at once, with a small explosion.

use super::mishap_glue::*;
use super::Game;
use crate::b2::math::Vec2;
use crate::b2::world::BodyType;
use ppg_extras::mishap::{self, Collapse};

pub struct Split {
    pub person: usize,
    /// The copy, once the paste has made it.
    pub copy: Option<usize>,
    people_before: usize,
    pub colour: usize,
    /// From the person to the copy.
    offset: Vec2,
    pub ends: f32,
    /// Being pulled together until then.
    pub tear: Option<f32>,
    /// Each limb pair's health and broken bone as last seen, and how hard the tear pulls it.
    health: Vec<[f32; 2]>,
    pull: Vec<f32>,
}

impl Game {
    /// Before each physics step: the copies follow the person (offset), or while tearing, pull toward each other.
    pub(super) fn super_pre_step(&mut self) {
        for k in 0..self.mishap.splits.len() {
            let s = &self.mishap.splits[k];
            let (offset, tearing) = (s.offset, s.tear.is_some());
            let pairs = self.mishap_pairs(s);
            for (n, (a, c)) in pairs.into_iter().enumerate() {
                let (Some(a), Some(c)) = (self.mishap_body(a), self.mishap_body(c)) else { continue };
                if tearing {
                    let (pa, pc) = (self.world.body(a).sweep.c, self.world.body(c).sweep.c);
                    let d = pc - pa;
                    let l = d.x.hypot(d.y).max(0.05);
                    let pull = self.mishap.splits[k].pull.get(n).copied().unwrap_or(1.0);
                    for (b, dir, p) in [(a, d * (1.0 / l), pa), (c, d * (-1.0 / l), pc)] {
                        let body = self.world.body_mut(b);
                        let f = dir * (body.mass * 60.0 * pull);
                        body.apply_force(f, p);
                    }
                    continue;
                }
                let body = self.world.body(a);
                let (p, ang, v, w) = (body.xf.p, body.sweep.a, body.linear_velocity, body.angular_velocity);
                self.world.set_transform(c, p + offset, ang);
                let cb = self.world.body_mut(c);
                (cb.linear_velocity, cb.angular_velocity) = (v, w);
                cb.wake();
            }
        }
    }

    /// Once a frame: copies appear, share their damage and collapse.
    pub(super) fn super_update(&mut self, now: f32) {
        let mut k = 0;
        while k < self.mishap.splits.len() {
            if self.people[self.mishap.splits[k].person].destroyed {
                self.mishap_end_split(k, false);
                continue;
            }
            // The paste's copy: the first new person with as many limbs.
            if self.mishap.splits[k].copy.is_none() {
                let (person, before) = (self.mishap.splits[k].person, self.mishap.splits[k].people_before);
                let n = self.people[person].Limbs.len();
                match (before..self.people.len()).find(|&q| self.people[q].Limbs.len() == n && !self.people[q].destroyed) {
                    Some(q) => {
                        let s = &mut self.mishap.splits[k];
                        s.copy = Some(q);
                        let pairs = self.mishap_pairs(&self.mishap.splits[k]);
                        let health = pairs.iter().map(|&(a, c)| [self.limbs[a].Health, self.limbs[c].Health]).collect();
                        let pull = (0..pairs.len()).map(|_| 0.6 + 1.2 * self.mishap_rand()).collect();
                        (self.mishap.splits[k].health, self.mishap.splits[k].pull) = (health, pull);
                    }
                    None => {
                        // Nothing pasted (it could not be made): no copy, nothing to collapse.
                        self.mishap.splits.remove(k);
                        continue;
                    }
                }
            }
            let copy = self.mishap.splits[k].copy.unwrap();
            if self.people[copy].destroyed {
                self.mishap.splits.remove(k);
                continue;
            }
            // Damage to either hurts both: each side's loss of health since the last frame is taken by the other,
            // a broken bone breaks both, a crushed limb crushes both.
            let pairs = self.mishap_pairs(&self.mishap.splits[k]);
            for (n, &(a, c)) in pairs.iter().enumerate() {
                let [ha, hc] = self.mishap.splits[k].health[n];
                let (da, dc) = (ha - self.limbs[a].Health, hc - self.limbs[c].Health);
                if dc > 0.0 {
                    self.limb_damage(a, dc);
                }
                if da > 0.0 {
                    self.limb_damage(c, da);
                }
                if self.limbs[a].Broken != self.limbs[c].Broken {
                    self.break_bone(if self.limbs[a].Broken { c } else { a });
                }
                let gone = |g: &Game, l: usize| g.physicals[g.limbs[l].PhysicalBehaviour].isDisintegrated;
                match (gone(self, a), gone(self, c)) {
                    (true, false) => self.limb_crush_next_frame(c),
                    (false, true) => self.limb_crush_next_frame(a),
                    _ => {}
                }
                self.mishap.splits[k].health[n] = [self.limbs[a].Health, self.limbs[c].Health];
            }
            let s = &self.mishap.splits[k];
            match s.tear {
                Some(until) if now >= until => {
                    self.mishap_tear_apart(k);
                    continue;
                }
                None if now >= s.ends => {
                    let r = self.mishap_rand();
                    match self.mishap.force_collapse.unwrap_or(mishap::collapse(r)) {
                        Collapse::Merge => {
                            self.mishap_merge(k, 2.0);
                            continue;
                        }
                        Collapse::Clone => {
                            self.mishap_end_split(k, true);
                            continue;
                        }
                        Collapse::Tear => {
                            self.mishap.splits[k].tear = Some(now + mishap::TEAR_TIME);
                            self.extras_sound(SOUND + 1, MERGE_SOUND, arr(self.mishap_extent(copy).0));
                        }
                    }
                }
                _ => {}
            }
            k += 1;
        }
    }

    pub(super) fn mishap_split(&mut self, person: usize, colour: usize) {
        let (c, half) = self.mishap_extent(person);
        let target = match self.extras.portals[1 - colour] {
            // In front of the other portal, clear of its surface.
            Some(pt) => {
                let n = vec(pt.frame.normal);
                let reach = half.x * n.x.abs() + half.y * n.y.abs();
                vec(pt.frame.centre) + n * (reach + 0.15)
            }
            None => self.mishap_spot(c, half),
        };
        let offset = target - c;
        let Some(p0) = self.people[person].Limbs.iter().find(|&&l| self.mishap_body(l).is_some()).map(|&l| self.limbs[l].PhysicalBehaviour) else { return };
        let states = self.object_states_of(&[p0], [c.x, c.y, 0.0]);
        let before = self.people.len();
        self.object_states_create(&states, [target.x, target.y, 0.0], false, true);
        let r = self.mishap_rand();
        let ends = self.time.time + mishap::split_time(r);
        self.mishap.splits.push(Split { person, copy: None, people_before: before, colour, offset, ends, tear: None, health: Vec::new(), pull: Vec::new() });
        for at in [c, target] {
            self.mishap_fx(SPLIT_FX, at, 1.6, colour);
            self.mishap_fx(MOTES, at, 0.8, colour);
            self.mishap.flashes.push((arr(at), colour, 3.0));
        }
        self.extras_sound(SOUND, SPLIT_SOUND, arr(c));
    }

    /// A clear spot beside the person, 2.5 to 5 units to either side.
    pub(super) fn mishap_spot(&mut self, c: Vec2, half: Vec2) -> Vec2 {
        let mut last = c + Vec2::new(3.0, 0.0);
        for _ in 0..12 {
            let r = self.mishap_rand();
            let side = if self.mishap_rand() < 0.5 { -1.0 } else { 1.0 };
            last = c + Vec2::new(side * (2.5 + 2.5 * r), 0.0);
            let shrink = Vec2::new(0.15, 0.15);
            let aabb = crate::b2::broadphase::Aabb { lower: last - half + shrink, upper: last + half - shrink };
            let blocked = self.world.query_aabb(&aabb).into_iter().any(|(f, _)| {
                let fx = self.world.fixture(f);
                !fx.is_sensor && self.world.body(fx.body).kind == BodyType::Static
            });
            if !blocked {
                break;
            }
        }
        last
    }

    /// The limb pairs (person's, copy's) of a resolved split.
    pub(super) fn mishap_pairs(&self, s: &Split) -> Vec<(usize, usize)> {
        let Some(copy) = s.copy else { return Vec::new() };
        self.people[s.person].Limbs.iter().copied().zip(self.people[copy].Limbs.iter().copied()).collect()
    }

    /// Ends split `i`: the copy goes (a merge) or stays (a clone).
    pub(super) fn mishap_end_split(&mut self, i: usize, keep_copy: bool) {
        let s = self.mishap.splits.remove(i);
        let Some(copy) = s.copy else { return };
        let (c, _) = self.mishap_extent(copy);
        if !keep_copy {
            if let Some(l) = self.people[copy].Limbs.iter().find(|&&l| !self.physicals[self.limbs[l].PhysicalBehaviour].destroyed).copied() {
                let root = self.root_id(self.limbs[l].PhysicalBehaviour);
                self.destroy_object(root);
            }
        }
        self.mishap_fx(MERGE_FX, c, 1.2, s.colour);
        self.mishap_fx(SPLIT_FX, c, 1.2, s.colour);
        self.mishap.flashes.push((arr(c), s.colour, 2.5));
        self.extras_sound(SOUND + 1, MERGE_SOUND, arr(c));
    }

    /// A merge: the copy snaps back into the person, who is dazed and glitches for a while.
    pub(super) fn mishap_merge(&mut self, i: usize, shock: f32) {
        let (person, colour) = (self.mishap.splits[i].person, self.mishap.splits[i].colour);
        self.mishap_end_split(i, false);
        let (c, _) = self.mishap_extent(person);
        self.mishap_fx(MERGE_FX, c, 1.0, colour);
        self.mishap.flashes.push((arr(c), colour, 2.0));
        self.people[person].ShockLevel += shock;
        self.add_pain(person, shock * 4.0);
        self.mishap.dazed.retain(|d| d.0 != person);
        self.mishap.dazed.push((person, colour, self.time.time + mishap::DAZED_TIME));
    }

    /// A second hit while split: an instant, violent collapse, a flash and a small explosion's shockwave.
    pub(super) fn mishap_violent(&mut self, i: usize) {
        let s = &self.mishap.splits[i];
        let at = s.copy.map(|c| self.mishap_extent(c).0);
        let (person, colour) = (s.person, s.colour);
        let (c, _) = self.mishap_extent(person);
        for p in std::iter::once(c).chain(at) {
            self.explosion_creator_explode(super::explosion::ExplosionParameters { FragmentationRayCount: 8, Position: [p.x, p.y, 0.0], Range: 2.5, FragmentForce: 1.5, CreateParticlesAndSound: true, EffectSize: super::explosion::EffectSize::Small, DismemberChance: 0.0, BallisticShrapnelCount: 0 });
            self.mishap_fx(TEAR_FX, p, 1.2, colour);
            self.mishap.flashes.push((arr(p), colour, 6.0));
        }
        self.mishap_merge(i, 6.0);
    }

    /// The catastrophe's end: both copies are torn apart where they met.
    pub(super) fn mishap_tear_apart(&mut self, k: usize) {
        let pairs = self.mishap_pairs(&self.mishap.splits[k]);
        let s = self.mishap.splits.remove(k);
        for at in [self.mishap_extent(s.person).0].into_iter().chain(s.copy.map(|c| self.mishap_extent(c).0)) {
            self.mishap_fx(TEAR_FX, at, 1.6, s.colour);
            self.mishap_fx(SPLIT_FX, at, 1.6, s.colour);
            self.mishap.flashes.push((arr(at), s.colour, 6.0));
            self.extras_sound(SOUND + 1, MERGE_SOUND, arr(at));
        }
        for (a, c) in pairs {
            for l in [a, c] {
                if self.mishap_body(l).is_none() {
                    continue;
                }
                if self.mishap_rand() < 0.3 && self.prefs.LimbCrushing {
                    self.limb_crush_next_frame(l);
                } else {
                    self.limb_slice(l);
                }
            }
        }
    }

    /// The tearing pairs' ends (for the drawing's stretched glow): (person's limb, copy's limb, colour).
    pub fn mishap_tears(&self) -> Vec<([f32; 2], [f32; 2], usize)> {
        let mut out = Vec::new();
        for s in self.mishap.splits.iter().filter(|s| s.tear.is_some()) {
            for (a, c) in self.mishap_pairs(s) {
                if let (Some(a), Some(c)) = (self.mishap_body(a), self.mishap_body(c)) {
                    out.push((arr(self.world.body(a).sweep.c), arr(self.world.body(c).sweep.c), s.colour));
                }
            }
        }
        out
    }
}
