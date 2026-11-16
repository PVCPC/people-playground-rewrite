//! Extras (not the game's): the spatial fold, a portal mishap (a child module of mishap_glue.rs, which picks it). Space
//! folds around the hit limb: its hinge (or that of a limb hanging from it) now runs through a pair of tiny portals
//! to another limb, mostly the torso, and everything below comes out of that limb, still joined and alive (the game's
//! limb graph, so blood and nerves, is untouched; only the physics hinge goes through the fold, keeping its bend).
//! A head comes out of the stomach, a forearm out of the back. Hitting a folded limb again folds it somewhere else.
//! The limb's own person never collides with it (PersonBehaviour's collision ignores), and the drawing clips the
//! limb at the exit rim (src/mishap_fold_view.rs); the rims are drawn by src/mishap_view.rs.

use super::{arr, vec, Fold, FOLD_SOUND, SOUND};
use crate::b2::math::Vec2;
use crate::b2::world::{BodyId, JointId, JointKind, Shape};
use crate::game::Game;
use ppg_extras::mishap;
use ppg_extras::portal::Frame;

/// The torso, where a fold mostly comes out (front or back).
const TORSO: [&str; 3] = ["UpperBody", "MiddleBody", "LowerBody"];

impl Game {
    /// The bodies hanging from `start` when joint `cut` is gone, among `allowed`.
    fn mishap_below(&self, start: BodyId, cut: JointId, allowed: &[BodyId]) -> Vec<BodyId> {
        let mut out = vec![start];
        let mut k = 0;
        while k < out.len() {
            for &(o, j) in &self.world.body(out[k]).joints {
                if j != cut && allowed.contains(&o) && !out.contains(&o) {
                    out.push(o);
                }
            }
            k += 1;
        }
        out
    }

    /// The local box of a body's solid shapes.
    fn mishap_local_box(&self, b: BodyId) -> (Vec2, Vec2) {
        let (mut lo, mut hi) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
        for &f in &self.world.body(b).fixtures {
            let fx = self.world.fixture(f);
            let pts: Vec<(Vec2, f32)> = match &fx.shape {
                Shape::Polygon(p) => p.vertices[..p.count].iter().map(|&v| (v, p.radius)).collect(),
                Shape::Circle(c) => vec![(c.p, c.radius)],
                Shape::Capsule(c) => c.vertices.iter().map(|&v| (v, c.radius)).collect(),
                Shape::Chain(_) => Vec::new(),
            };
            for (v, r) in pts {
                lo = Vec2::new(lo.x.min(v.x - r), lo.y.min(v.y - r));
                hi = Vec2::new(hi.x.max(v.x + r), hi.y.max(v.y + r));
            }
        }
        if lo.x > hi.x {
            return (Vec2::new(-0.1, -0.1), Vec2::new(0.1, 0.1));
        }
        (lo, hi)
    }

    /// Folds space around limb `hit`: its own hinge (or that of a limb hanging from it) runs through a pair of tiny
    /// portals to another limb, and everything below it comes out of that limb. A folded hinge folds again.
    pub(super) fn mishap_fold(&mut self, hit: usize, colour: usize) -> bool {
        let person = self.limbs[hit].Person;
        let limbs = self.people[person].Limbs.clone();
        let bodies: Vec<BodyId> = limbs.iter().filter_map(|&l| self.mishap_body(l)).collect();
        let live_joint = |g: &Game, l: usize| -> Option<JointId> {
            let h = g.limbs[l].Joint.filter(|_| g.limbs[l].HasJoint)?;
            let j = g.hinges[h].joint.filter(|_| !g.hinges[h].destroyed)?;
            matches!(g.world.joints.get(j), Some(Some(x)) if matches!(x.kind, JointKind::Revolute(_))).then_some(j)
        };
        let Some(hit_body) = self.mishap_body(hit) else { return false };
        // The hit limb's own hinge, else one of the limbs hanging from it.
        let mut choice = live_joint(self, hit).map(|j| (hit, j));
        if choice.is_none() {
            let below: Vec<(usize, JointId)> = limbs.iter().filter_map(|&o| live_joint(self, o).map(|j| (o, j))).filter(|&(o, j)| o != hit && { let x = self.world.joints[j].as_ref().unwrap(); x.body_a == hit_body || x.body_b == hit_body }).collect();
            if !below.is_empty() {
                let r = self.mishap_rand();
                choice = Some(below[((r * below.len() as f32) as usize).min(below.len() - 1)]);
            }
        }
        let Some((child_limb, jid)) = choice else { return false };
        let Some(child) = self.mishap_body(child_limb) else { return false };
        let j = self.world.joints[jid].as_ref().unwrap();
        let parent_is_b = j.body_a == child;
        let parent = if parent_is_b { j.body_b } else { j.body_a };
        let r = j.revolute().def.clone();
        let parent_anchor = if parent_is_b { r.local_anchor_b } else { r.local_anchor_a };
        let below = self.mishap_below(child, jid, &bodies);
        let hosts: Vec<BodyId> = bodies.iter().copied().filter(|b| !below.contains(b) && *b != parent).collect();
        if hosts.is_empty() {
            return false;
        }
        // Mostly the torso (a head out of the stomach, a hand out of the back); any other limb when no torso is left.
        let torso: Vec<BodyId> = hosts.iter().copied().filter(|&b| self.limb_of_body(b).is_some_and(|l| TORSO.contains(&self.physicals[self.limbs[l].PhysicalBehaviour].name.as_str()))).collect();
        let hosts = if torso.is_empty() || self.mishap_rand() < 0.2 { hosts } else { torso };
        let host = hosts[((self.mishap_rand() * hosts.len() as f32) as usize).min(hosts.len() - 1)];
        // Where it comes out: a side of the host (a limb's long axis is its local y: front or back), at random along it.
        let (lo, hi) = self.mishap_local_box(host);
        let mid = (lo + hi) * 0.5;
        let side = if self.mishap_rand() < 0.5 { -1.0 } else { 1.0 };
        let along = (self.mishap_rand() * 2.0 - 1.0) * 0.55 * (hi.y - lo.y) * 0.5;
        let exit_local = (Vec2::new(mid.x + side * (hi.x - lo.x) * 0.5, mid.y + along), Vec2::new(side, 0.0));
        // Where it goes in now: the hinge's anchor on its parent, facing out of the parent (or out of the last fold).
        let existing = self.mishap.folds.iter().position(|f| f.joint == jid);
        let pb = self.world.body(parent);
        let at = pb.xf.apply(parent_anchor);
        let out = match existing {
            Some(k) => pb.xf.q.apply(self.mishap.folds[k].exit.2),
            None => {
                let d = parent_anchor - pb.sweep.local_center;
                let l = d.x.hypot(d.y);
                pb.xf.q.apply(if l > 1e-4 { d * (1.0 / l) } else { Vec2::new(0.0, -1.0) })
            }
        };
        let hb = self.world.body(host);
        let to = Frame { centre: arr(hb.xf.apply(exit_local.0)), normal: arr(hb.xf.q.apply(exit_local.1)) };
        let fold = mishap::Fold::new(Frame { centre: arr(at), normal: arr(out) }, to);
        // The hinge's bend before.
        let angle = |g: &Game, a: BodyId, b: BodyId, reference: f32| g.world.body(b).sweep.a - g.world.body(a).sweep.a - reference;
        let (ja, jb) = if parent_is_b { (child, parent) } else { (parent, child) };
        let bend = angle(self, ja, jb, r.reference_angle);
        // Everything below comes out of the host, as rigidly as it hung.
        for &b in &below {
            let body = self.world.body(b);
            let (p, a, v, w) = (body.xf.p, body.sweep.a, body.linear_velocity, body.angular_velocity);
            self.world.set_transform(b, vec(fold.map_point(arr(p))), a + fold.rotation);
            let body = self.world.body_mut(b);
            body.linear_velocity = vec(fold.map_dir(arr(v)));
            body.angular_velocity = w;
            body.wake();
        }
        // The hinge, through the fold: its parent side is now the host, at the exit, with the same bend.
        let (ja, jb) = if parent_is_b { (child, host) } else { (host, child) };
        let reference = mishap::keep_reference(bend, self.world.body(ja).sweep.a, self.world.body(jb).sweep.a);
        {
            let j = self.world.joints[jid].as_mut().unwrap();
            if parent_is_b {
                j.body_b = host;
            } else {
                j.body_a = host;
            }
            let rj = j.revolute_mut();
            if parent_is_b {
                rj.def.local_anchor_b = exit_local.0;
            } else {
                rj.def.local_anchor_a = exit_local.0;
            }
            rj.def.reference_angle = reference;
            rj.impulse = [0.0; 3];
            rj.motor_impulse = 0.0;
        }
        self.world.body_mut(parent).joints.retain(|&(_, id)| id != jid);
        for e in self.world.body_mut(child).joints.iter_mut().filter(|e| e.1 == jid) {
            e.0 = host;
        }
        self.world.body_mut(host).joints.push((child, jid));
        for b in [parent, host, child] {
            self.world.body_mut(b).wake();
        }
        let exit = (host, exit_local.0, exit_local.1);
        match existing {
            Some(k) => (self.mishap.folds[k].exit, self.mishap.folds[k].colour) = (exit, colour),
            None => {
                let pb = self.world.body(parent);
                let entry = (parent, parent_anchor, pb.xf.q.apply_t(out));
                self.mishap.folds.push(Fold { colour, joint: jid, child, entry, exit });
            }
        }
        for f in [fold.from, fold.to] {
            self.mishap.flashes.push((f.centre, colour, 1.2));
        }
        self.extras_sound(SOUND + 2, FOLD_SOUND, fold.to.centre);
        // It hurts: shock and pain, as a merge's.
        self.people[person].ShockLevel += 2.0;
        self.add_pain(person, 8.0);
        true
    }

    /// The tiny portals' frames in the world now, with their colour (for the drawing).
    pub fn mishap_rims(&self) -> Vec<(Frame, usize)> {
        let mut out = Vec::new();
        for f in &self.mishap.folds {
            for (b, p, n) in [f.entry, f.exit] {
                let body = self.world.body(b);
                out.push((Frame { centre: arr(body.xf.apply(p)), normal: arr(body.xf.q.apply(n)) }, f.colour));
            }
        }
        out
    }

    /// The limbs that come out of a fold's exit rim, with the rim (the drawing clips them behind it).
    pub fn mishap_fold_clips(&self) -> Vec<(usize, Frame)> {
        let mut out = Vec::new();
        for f in &self.mishap.folds {
            let (Some(l), (b, p, n)) = (self.limb_of_body(f.child), f.exit) else { continue };
            let body = self.world.body(b);
            out.push((l, Frame { centre: arr(body.xf.apply(p)), normal: arr(body.xf.q.apply(n)) }));
        }
        out
    }
}
