//! Extras (not the game's): phase loss, one of the portal mishaps (mishap_glue.rs picks it). The person stops being
//! fully real: every limb loses its collision and its weight, the body drifts sideways through walls and things and
//! sinks partway into the floor, see-through and flickering in the shot's colour (mishap_view.rs draws it). Each limb
//! snaps back into reality at its own random moment over a few seconds; one that comes back inside something solid
//! is crushed (mostly inside) or bursts out of it (partly inside), by the game's own crush, slice and damage paths.

use super::mishap_glue::{arr, SOUND, SPLIT_FX, SPLIT_SOUND};
use super::Game;
use crate::b2::math::Vec2;
use crate::b2::world::{BodyId, Filter, FixtureId, Shape};
use ppg_extras::mishap;

const SNAP_SOUND: (&str, f32, f32) = ("blaster_impact_1", 0.6, 1.2);

pub struct Phase {
    pub person: usize,
    pub colour: usize,
    /// Sideways drift (units a second).
    pub drift: f32,
    /// The height the phased limbs' centre sinks to.
    pub sink_to: f32,
    pub limbs: Vec<Phased>,
}

pub struct Phased {
    pub limb: usize,
    pub at: f32,
    filters: Vec<(FixtureId, Filter)>,
    gravity: f32,
}

impl Game {
    fn phase_set(&mut self, b: BodyId, filters: &[(FixtureId, Filter)], off: bool) {
        let mut colliders = Vec::new();
        for &(f, filter) in filters {
            let Some(Some(fx)) = self.world.fixtures.get_mut(f) else { continue };
            fx.filter = if off { Filter { mask_bits: 0, ..filter } } else { filter };
            if !colliders.contains(&fx.collider.id) {
                colliders.push(fx.collider.id);
            }
        }
        for c in colliders {
            self.world.refilter_collider(c);
        }
        self.world.body_mut(b).wake();
    }

    /// The shot hit `person`: every limb goes out of phase (again, if it already was: new snap times).
    pub(super) fn phase_start(&mut self, person: usize, colour: usize) {
        let side = if self.mishap_rand() < 0.5 { -1.0 } else { 1.0 };
        let drift = side * (0.5 + 0.5 * self.mishap_rand());
        let (c, _) = self.mishap_extent(person);
        let sink_to = c.y - (0.45 + 0.35 * self.mishap_rand());
        let k = match self.mishap.phases.iter().position(|p| p.person == person) {
            Some(k) => k,
            None => {
                self.mishap.phases.push(Phase { person, colour, drift, sink_to, limbs: Vec::new() });
                self.mishap.phases.len() - 1
            }
        };
        let p = &mut self.mishap.phases[k];
        (p.colour, p.drift, p.sink_to) = (colour, drift, sink_to);
        for l in self.people[person].Limbs.clone() {
            let Some(b) = self.mishap_body(l) else { continue };
            let at = self.time.time + mishap::snap_time(self.mishap_rand());
            if let Some(p) = self.mishap.phases[k].limbs.iter_mut().find(|p| p.limb == l) {
                p.at = at;
                continue;
            }
            let filters: Vec<(FixtureId, Filter)> = self.world.body(b).fixtures.iter().map(|&f| (f, self.world.fixture(f).filter)).collect();
            self.phase_set(b, &filters, true);
            let body = self.world.body_mut(b);
            let gravity = body.gravity_scale;
            body.gravity_scale = 0.0;
            self.mishap.phases[k].limbs.push(Phased { limb: l, at, filters, gravity });
        }
        self.mishap_fx(SPLIT_FX, c, 1.4, colour);
        self.mishap.flashes.push((arr(c), colour, 2.5));
        self.extras_sound(SOUND, SPLIT_SOUND, arr(c));
    }

    /// How much of body `b` is inside something its fixtures would collide with, not its own person's: (points
    /// inside, points, the way out).
    fn phase_overlap(&self, b: BodyId, own: &[BodyId]) -> (usize, usize, Vec2) {
        let body = self.world.body(b);
        let (mut inside, mut n, mut sum_in, mut sum_out) = (0, 0, Vec2::ZERO, Vec2::ZERO);
        for &f in &body.fixtures {
            let fx = self.world.fixture(f);
            if fx.is_sensor {
                continue;
            }
            let mut samples = Vec::new();
            match &fx.shape {
                Shape::Polygon(p) => {
                    let vs = &p.vertices[..p.count];
                    let c = vs.iter().fold(Vec2::ZERO, |s, &v| s + v) * (1.0 / vs.len().max(1) as f32);
                    samples.push(c);
                    samples.extend(vs.iter().map(|&v| c + (v - c) * 0.7));
                }
                Shape::Circle(c) => {
                    samples.push(c.p);
                    samples.extend([(0.7, 0.0), (-0.7, 0.0), (0.0, 0.7), (0.0, -0.7)].map(|(x, y)| c.p + Vec2::new(x * c.radius, y * c.radius)));
                }
                Shape::Capsule(c) => samples.extend(c.vertices.iter().copied()),
                Shape::Chain(_) => {}
            }
            for s in samples {
                let q = body.xf.apply(s);
                n += 1;
                let hit = self.world.query_point(q).into_iter().any(|(o, _)| {
                    let ox = self.world.fixture(o);
                    !ox.is_sensor && !own.contains(&ox.body) && !self.world.portal_ghosts.contains(&o) && self.world.fixtures_should_collide(f, o) && super::tool::test_point(&ox.shape, &self.world.body(ox.body).xf, q)
                });
                if hit {
                    inside += 1;
                    sum_in = sum_in + q;
                } else {
                    sum_out = sum_out + q;
                }
            }
        }
        let way = if inside == 0 || inside == n {
            Vec2::new(0.0, 1.0)
        } else {
            let d = sum_out * (1.0 / (n - inside) as f32) - sum_in * (1.0 / inside as f32);
            d * (1.0 / d.x.hypot(d.y).max(1e-4))
        };
        (inside, n, way)
    }

    /// A phased limb comes back: solid again; mostly inside something it is crushed, partly inside it bursts out.
    fn phase_snap(&mut self, k: usize, i: usize) {
        let pl = self.mishap.phases[k].limbs.remove(i);
        let (person, colour) = (self.mishap.phases[k].person, self.mishap.phases[k].colour);
        let Some(b) = self.mishap_body(pl.limb) else { return };
        self.phase_set(b, &pl.filters, false);
        self.world.body_mut(b).gravity_scale = pl.gravity;
        let own: Vec<BodyId> = self.people[person].Limbs.iter().filter_map(|&l| self.mishap_body(l)).collect();
        let (inside, n, way) = self.phase_overlap(b, &own);
        let at = self.world.body(b).sweep.c;
        self.mishap_fx(SPLIT_FX, at, 0.5, colour);
        self.mishap.flashes.push((arr(at), colour, if inside == 0 { 0.9 } else { 2.2 }));
        self.extras_sound(SOUND + 3, SNAP_SOUND, arr(at));
        eprintln!("MISHAP-PHASE snap limb {} inside {inside}/{n} at ({:.2}, {:.2})", pl.limb, at.x, at.y);
        if inside == 0 {
            return;
        }
        let l = pl.limb;
        if inside * 10 >= n * 6 && self.prefs.LimbCrushing {
            self.limb_crush_next_frame(l);
            return;
        }
        // Bursts out: thrown out of what it was in, the bone broken, torn skin, maybe torn off.
        let body = self.world.body_mut(b);
        body.linear_velocity = body.linear_velocity + way * 9.0;
        body.angular_velocity += 6.0;
        self.limb_damage(l, 25.0);
        self.break_bone(l);
        self.skin_impact_damage(l, 3.0, arr(at));
        if self.mishap_rand() < 0.35 {
            self.limb_slice(l);
        }
    }

    /// Before b2World::Step: the phased limbs drift sideways and sink toward their depth.
    pub(super) fn phase_pre_step(&mut self) {
        for k in 0..self.mishap.phases.len() {
            let bodies: Vec<BodyId> = self.mishap.phases[k].limbs.iter().filter_map(|p| self.mishap_body(p.limb)).collect();
            if bodies.is_empty() {
                continue;
            }
            let y = bodies.iter().map(|&b| self.world.body(b).sweep.c.y).sum::<f32>() / bodies.len() as f32;
            let p = &self.mishap.phases[k];
            let target = Vec2::new(p.drift, ((p.sink_to - y) * 1.2).clamp(-0.7, 0.3));
            for b in bodies {
                let body = self.world.body_mut(b);
                body.linear_velocity = body.linear_velocity * 0.9 + target * 0.1;
                body.angular_velocity *= 0.95;
                body.wake();
            }
        }
    }

    /// The phased limbs for the drawing: (limb, its centre, colour, seconds until it snaps back).
    pub fn phase_ghosts(&self) -> Vec<(usize, [f32; 2], usize, f32)> {
        let mut out = Vec::new();
        for p in &self.mishap.phases {
            for pl in &p.limbs {
                if let Some(b) = self.mishap_body(pl.limb) {
                    out.push((pl.limb, arr(self.world.body(b).sweep.c), p.colour, pl.at - self.time.time));
                }
            }
        }
        out
    }

    /// Once a frame: the limbs whose moment has come snap back; a person with none left phased is done.
    pub(super) fn phase_update(&mut self) {
        let now = self.time.time;
        for k in (0..self.mishap.phases.len()).rev() {
            for i in (0..self.mishap.phases[k].limbs.len()).rev() {
                if self.mishap.phases[k].limbs[i].at <= now {
                    self.phase_snap(k, i);
                }
            }
            if self.mishap.phases[k].limbs.is_empty() {
                self.mishap.phases.remove(k);
            }
        }
    }
}
