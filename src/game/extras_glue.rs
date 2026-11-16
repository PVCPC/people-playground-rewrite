//! Extras (not the game's): the portal gun, game side. The gun is a Brick made gun-shaped; activating it fires a blue
//! portal (an orange one with Shift held). The two portals are one doorway: each cuts a hole in its surface's contacts
//! for what goes in, and a body in a doorway has a twin at the other portal (a b2 body the queries never see) whose
//! contacts there act on the real body. A joined group (a ragdoll) crosses over whole when its centre of mass does, so
//! its joints are always solved on one side.

use super::Game;
use crate::b2::math::{dot, Rot, Vec2};
use crate::b2::world::{BodyDef, BodyId, BodyType, FixtureDef, PortalHole, Shape};
use ppg_extras::portal::{fit_shifts, point_velocity, Frame, Pair, DEPTH, HALF_LEN};
use std::collections::HashMap;

/// How far in front of a portal (m) bodies are looked at: 12 m covers 720 m/s at 60 steps a second.
const REACH: f32 = 12.0;

fn arr(a: Vec2) -> [f32; 2] {
    [a.x, a.y]
}
fn vec(a: [f32; 2]) -> Vec2 {
    Vec2::new(a[0], a[1])
}
fn rotate(a: Vec2, angle: f32) -> Vec2 {
    vec(ppg_extras::portal::rotate(arr(a), angle))
}

const PPU: f32 = ppg_extras::art::PIXELS_PER_UNIT;

#[derive(Default)]
pub struct Extras {
    pub guns: Vec<Gun>,
    /// Blue, orange.
    pub portals: [Option<Portal>; 2],
    /// The bodies in a doorway, by body.
    pub crossers: HashMap<BodyId, Crosser>,
    /// The bodies at each portal whose contacts with its surface lose the opening.
    pub near: [Vec<BodyId>; 2],
    /// What happened this frame, for the drawing's lights and beam.
    pub fx: Vec<Fx>,
    /// The last shot's hit (point, normal), whether or not a portal fit there.
    last_hit: Option<([f32; 2], [f32; 2])>,
    /// The effects' own dice (never the game's Random) and the drifting sparks' clock.
    rng: u32,
    spark_clock: f32,
}

/// A moment of the portal gun's effects.
#[derive(Clone, Copy, Debug)]
pub enum Fx {
    /// A shot from the barrel tip to where it hit (or ran out).
    Fire { colour: usize, from: [f32; 2], to: [f32; 2] },
    Open { colour: usize, frame: Frame },
    Close { colour: usize, frame: Frame },
    /// A shot hit a surface that cannot hold a portal.
    Fizzle { colour: usize, at: [f32; 2], normal: [f32; 2] },
}

/// The portals' colours as the game's effects are tinted (blue, orange).
pub const TINTS: [[f32; 4]; 2] = [[0.3, 0.65, 1.0, 1.0], [1.0, 0.55, 0.12, 1.0]];
/// The game's effects and sounds the portal gun uses (Resources prefabs and AudioClips, by name; read at runtime).
/// (Picked by eye from the PP_PORTAL=fxlab scene: BrokenElectronicsSpark is a short puff of light, Exotic flash a
/// few fine sparks, RaygunExplosion wisps; BigZap's smoke puff lingered too long for an opening.)
const MUZZLE: &str = "Prefabs/Exotic flash";
const OPEN: &str = "Prefabs/Exotic flash";
const SPARKS: &str = "Prefabs/Exotic flash";
/// The motes an item gives off when spawned (PhysicalBehaviour.SetupSpawnPrefab's): now and then from a portal's tips.
const MOTES: &str = "Prefabs/SpawnParticles";
const CLOSE: &str = "Prefabs/RaygunExplosion";
const FIZZLE: &str = "Prefabs/BrokenElectronicsSpark";
/// (clip, volume, pitch)
const FIRE_SOUND: (&str, f32, f32) = ("ion_fire", 0.55, 1.35);
const OPEN_SOUND: (&str, f32, f32) = ("energy_sword_enable", 0.7, 1.1);
const CLOSE_SOUND: (&str, f32, f32) = ("energy_sword_disable", 0.5, 1.3);
const FIZZLE_SOUND: (&str, f32, f32) = ("blaster_impact_1", 0.6, 0.8);
/// The extras' own AudioSources (instance ids no game object takes): fire, open, close, fizzle.
const SOURCES: i32 = -0x7E00_0000;

pub struct Gun {
    pub p: usize,
    /// The last colour fired.
    pub colour: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct Portal {
    pub body: BodyId,
    /// The physical whose body carries it (none for the map's static ground).
    pub physical: Option<usize>,
    /// On the body, in its frame.
    centre: Vec2,
    normal: Vec2,
    /// In the world, as of the last refresh.
    pub frame: Frame,
    /// Half the opening's length (HALF_LEN times the Portal size setting when it was shot).
    pub half: f32,
}

pub struct Crosser {
    /// The portal it went into; its twin is at the other.
    pub portal: usize,
    pub ghost: BodyId,
    /// The twin as set before the step: velocity, spin, centre and angle.
    v0: Vec2,
    w0: f32,
    c0: Vec2,
    a0: f32,
    /// The twin's mass over its body's: the body's share of its whole group (a ragdoll's limb in a doorway pushes
    /// and is pushed at the far side as the ragdoll it is part of, not as a lone limb).
    k: f32,
}

impl Extras {
    /// Into portal `i`, out of the other.
    pub fn pair(&self, i: usize) -> Option<Pair> {
        Some(Pair::new(self.portals[i]?.frame, self.portals[1 - i]?.frame))
    }
}

impl Game {
    /// Any spawn (catalog, save, paste): a portal gun's prefab (src/extras_hook/portal_gun.rs) joins the guns.
    pub(super) fn extras_spawned(&mut self, prefab: &crate::assets::db::Prefab, linked: &super::link::Linked) {
        if prefab.objects.first().is_none_or(|o| o.game_object.path_id != crate::extras_hook::portal_gun::ROOT) {
            return;
        }
        if let Some(&p) = linked.physicals.get(&0) {
            self.extras.guns.push(Gun { p, colour: None });
        }
    }

    /// PhysicalBehaviour.Use reached physical component `id`: a gun fires.
    pub(super) fn extras_activated(&mut self, id: i32) {
        if let Some(gun) = self.extras.guns.iter().position(|g| self.physicals[g.p].id == id && !self.physicals[g.p].destroyed) {
            let colour = self.input.held("fast") as usize;
            self.extras_fire(gun, colour);
        }
    }

    /// The gun's barrel tip and direction.
    pub fn extras_barrel(&self, gun: usize) -> ([f32; 2], [f32; 2]) {
        let p = self.extras.guns[gun].p;
        let tip = self.transform_point3(p, [ppg_extras::art::GUN_TIP[0] / PPU, 0.0, 0.0]);
        let back = self.transform_point3(p, [0.0, 0.0, 0.0]);
        let d = [tip[0] - back[0], tip[1] - back[1]];
        let l = d[0].hypot(d[1]).max(1e-6);
        ([tip[0], tip[1]], [d[0] / l, d[1] / l])
    }

    pub fn extras_fire(&mut self, gun: usize, colour: usize) {
        self.extras.guns[gun].colour = Some(colour);
        let (tip, dir) = self.extras_barrel(gun);
        let own = self.extras_group(self.physicals[self.extras.guns[gun].p].rigidbody.body);
        self.extras_effect(MUZZLE, tip, dir[1].atan2(dir[0]), 0.8, colour);
        self.extras_sound(0, FIRE_SOUND, tip);
        self.extras.last_hit = None;
        let opened = self.extras_place(colour, tip, dir, &own);
        let to = self.extras.last_hit.map_or([tip[0] + dir[0] * 60.0, tip[1] + dir[1] * 60.0], |h| h.0);
        self.extras.fx.push(Fx::Fire { colour, from: tip, to });
        if let (false, Some((at, normal))) = (opened, self.extras.last_hit) {
            self.extras.fx.push(Fx::Fizzle { colour, at, normal });
            self.extras_effect(FIZZLE, at, normal[1].atan2(normal[0]), 0.5, colour);
            self.extras_effect(SPARKS, at, normal[1].atan2(normal[0]) - std::f32::consts::FRAC_PI_2, 1.0, colour);
            self.extras_sound(3, FIZZLE_SOUND, at);
        }
    }

    /// One of the game's particle effects (a Resources prefab), tinted the portal's colour; presentation only.
    pub(crate) fn extras_effect(&mut self, path: &str, at: [f32; 2], angle: f32, scale: f32, colour: usize) {
        if self.effects_off {
            return;
        }
        let Some(prefab) = self.assets.as_deref().and_then(|db| db.resource(path, 1).ok().flatten()) else { return };
        let (s, c) = (angle * 0.5).sin_cos();
        self.effect_log.push(super::EffectSpawn { prefab, position: [at[0], at[1], 0.0], rotation: [0.0, 0.0, s, c], scale, color: Some(TINTS[colour]), root: None, subtree: None, water: None, pooled: None, parent: None, off: Vec::new(), tint_all: true });
    }

    /// One of the game's clips, by name, at `at` from the extras' AudioSource `k` (pitched; Sound effects group).
    pub(crate) fn extras_sound(&mut self, k: i32, (name, volume, pitch): (&str, f32, f32), at: [f32; 2]) {
        let Some(db) = self.assets.as_deref() else { return };
        let Ok(Some(clip)) = super::audio::clip_ref(db, &mut self.audio.clip_cache, db.clip_by_name(name)) else { return };
        let id = SOURCES + k;
        if !self.audio.sources.contains_key(&id) {
            let mut s = super::audio::AudioSource { rolloff_mode: 1, min_distance: 1.0, max_distance: 60.0, group: Some(super::audio::MixerGroup::SoundEffects), ..Default::default() };
            s.set_spatial_blend(1.0);
            self.audio.add(id, s);
        }
        let s = self.audio.get_mut(id);
        s.pitch = pitch;
        s.chain = vec![crate::unity::transform::Trs { t: [at[0], at[1], 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] }];
        let v = if self.prefs.ClampVolume { volume.clamp(0.0, 1.0) } else { volume };
        self.audio.play_one_shot(id, Some(&clip), v);
    }

    /// A portal opening (`open`) or closing: the game's effects along its line, a sound and the drawing's flash.
    fn extras_portal_fx(&mut self, colour: usize, frame: Frame, half: f32, open: bool) {
        let (t, n) = (frame.tangent(), frame.normal);
        let up = n[1].atan2(n[0]) - std::f32::consts::FRAC_PI_2;
        if open {
            self.extras_effect(OPEN, frame.centre, up, 1.6, colour);
            for k in [-0.8, -0.4, 0.0, 0.4, 0.8] {
                self.extras_effect(SPARKS, [frame.centre[0] + t[0] * k * half, frame.centre[1] + t[1] * k * half], up, 1.0, colour);
            }
            self.extras_sound(1, OPEN_SOUND, frame.centre);
            self.extras.fx.push(Fx::Open { colour, frame });
        } else {
            for k in [-0.5, 0.5] {
                self.extras_effect(CLOSE, [frame.centre[0] + t[0] * k * half, frame.centre[1] + t[1] * k * half], up, 0.5, colour);
            }
            self.extras_sound(2, CLOSE_SOUND, frame.centre);
            self.extras.fx.push(Fx::Close { colour, frame });
        }
    }

    /// The effects' own dice, 0..1.
    fn extras_rand(&mut self) -> f32 {
        self.extras.rng = self.extras.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.extras.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Each frame: now and then (every 0.4 to 1.4 s, at random) a few motes fly off one of a portal's tips.
    fn extras_drift_sparks(&mut self) {
        self.extras.spark_clock -= self.time.deltaTime;
        if self.extras.spark_clock > 0.0 {
            return;
        }
        self.extras.spark_clock = 0.4 + self.extras_rand();
        for i in 0..2 {
            let Some(pt) = self.extras.portals[i] else { continue };
            if self.extras_rand() < 0.4 {
                continue;
            }
            let (t, n) = (pt.frame.tangent(), pt.frame.normal);
            let tip = if self.extras_rand() < 0.5 { -1.0 } else { 1.0 } * pt.half * (0.85 + 0.15 * self.extras_rand());
            let at = [pt.frame.centre[0] + t[0] * tip + n[0] * 0.05, pt.frame.centre[1] + t[1] * tip + n[1] * 0.05];
            self.extras_effect(MOTES, at, n[1].atan2(n[0]) - std::f32::consts::FRAC_PI_2, 0.25, i);
        }
    }

    /// Opens portal `colour` where the ray from `origin` along `dir` meets a surface with room for it (sliding it along
    /// the surface to fit), replacing the old one. `skip`: bodies the ray passes (the gun and who holds it).
    pub fn extras_place(&mut self, colour: usize, origin: [f32; 2], dir: [f32; 2], skip: &[BodyId]) -> bool {
        let first_hit = |g: &mut Game, from: [f32; 2], d: [f32; 2], dist: f32| {
            g.raycast_all(vec(from), d, dist, super::explosion::DEFAULT_RAYCAST_LAYERS, 64).into_iter().find(|h| !skip.contains(&h.body))
        };
        let Some(hit) = first_hit(self, origin, dir, 200.0) else { return false };
        let (point, normal) = (arr(hit.point), arr(hit.normal));
        let l = normal[0].hypot(normal[1]);
        let n = [normal[0] / l, normal[1] / l];
        self.extras.last_hit = Some((point, n));
        // A person hit misfires (mishap_glue.rs): no portal; Some(false) fizzles.
        if let Some(done) = self.mishap_hit(colour, hit.body, point) {
            return done;
        }
        let t = [n[1], -n[0]];
        let other = self.extras.portals[1 - colour];
        // The two portals are always one size (a doorway maps 1:1, nothing is scaled going through): a new size from
        // the setting resizes the other portal in place, or, where it has no room for it there, waits.
        let mut hl = HALF_LEN * self.prefs.PortalSize.clamp(0.25, 4.0);
        let mut resize = None;
        if let Some(o) = other.filter(|o| (o.half - hl).abs() > 1e-4) {
            if self.extras_fits(o.frame.centre, o.frame.normal, hl, o.body, skip) {
                resize = Some(hl);
            } else {
                hl = o.half;
            }
        }
        for shift in fit_shifts().map(|s| s * hl / HALF_LEN) {
            let c = [point[0] + t[0] * shift, point[1] + t[1] * shift];
            // The other portal on the same stretch: no overlap.
            if other.is_some_and(|o| (o.frame.centre[0] - c[0]).hypot(o.frame.centre[1] - c[1]) < 2.0 * hl && o.frame.normal[0] * n[0] + o.frame.normal[1] * n[1] > 0.5) {
                continue;
            }
            if !self.extras_fits(c, n, hl, hit.body, skip) {
                continue;
            }
            if let (Some(h), Some(o)) = (resize, self.extras.portals[1 - colour].as_mut()) {
                o.half = h;
                let (frame, half) = (o.frame, o.half);
                self.extras_portal_fx(1 - colour, frame, half, true);
            }
            let xf = self.world.body(hit.body).xf;
            let physical = self.physicals.iter().position(|p| !p.destroyed && p.rigidbody.body == hit.body);
            if let Some(old) = self.extras.portals[colour] {
                self.extras_portal_fx(colour, old.frame, old.half, false);
            }
            let frame = Frame { centre: c, normal: n };
            let (lo, hi) = ([c[0] - hl - 0.5, c[1] - hl - 0.5], [c[0] + hl + 0.5, c[1] + hl + 0.5]);
            for (fx, _) in self.world.query_aabb(&crate::b2::broadphase::Aabb { lower: vec(lo), upper: vec(hi) }) {
                let b = self.world.fixture(fx).body;
                self.world.body_mut(b).set_sleeping_false();
            }
            self.extras.portals[colour] = Some(Portal { body: hit.body, physical, centre: xf.apply_t(vec(c)), normal: xf.q.apply_t(vec(n)), frame, half: hl });
            self.extras_portal_fx(colour, frame, hl, true);
            return true;
        }
        false
    }

    /// A portal of half length `hl` at `c` (normal `n`) on `body` has room: the surface runs flat all along it, and in
    /// front of it is open (not inside a floor the wall runs down behind; things that move don't count).
    fn extras_fits(&mut self, c: [f32; 2], n: [f32; 2], hl: f32, body: BodyId, skip: &[BodyId]) -> bool {
        let t = [n[1], -n[0]];
        [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|&k| {
            let q = [c[0] + t[0] * k * hl + n[0] * 0.1, c[1] + t[1] * k * hl + n[1] * 0.1];
            let open = self.world.query_point(vec(q)).into_iter().all(|(f, _)| {
                let fx = self.world.fixture(f);
                fx.is_sensor || skip.contains(&fx.body) || self.world.body(fx.body).kind == BodyType::Dynamic || self.world.portal_ghosts.contains(&f) || !super::tool::test_point(&fx.shape, &self.world.body(fx.body).xf, vec(q))
            });
            let back = self.raycast_all(vec(q), [-n[0], -n[1]], 0.3, super::explosion::DEFAULT_RAYCAST_LAYERS, 64).into_iter().find(|h| !skip.contains(&h.body));
            open && back.is_some_and(|h| h.body == body && h.normal.x * n[0] + h.normal.y * n[1] > 0.99 * h.normal.x.hypot(h.normal.y) && (h.distance - 0.1).abs() < 0.02)
        })
    }

    /// The dynamic bodies joined to `body`, it first.
    fn extras_group(&self, body: BodyId) -> Vec<BodyId> {
        let tool = self.tool.as_ref().map(|t| t.body);
        let mut group = vec![body];
        let mut i = 0;
        while i < group.len() {
            for &(other, _) in &self.world.body(group[i]).joints {
                // The drag tool's body is never part of a group: it tethers what it holds (it must not be carried through).
                if self.world.body(other).kind == BodyType::Dynamic && !group.contains(&other) && Some(other) != tool {
                    group.push(other);
                }
            }
            i += 1;
        }
        group
    }

    /// A group with a joint to anything outside it (a static anchor, the drag tool): it cannot cross over.
    fn extras_tethered(&self, group: &[BodyId]) -> bool {
        group.iter().any(|&b| self.world.body(b).joints.iter().any(|(o, _)| !group.contains(o)))
    }

    /// Each portal's world frame from its body; a portal whose body is gone closes.
    fn extras_refresh_portals(&mut self) {
        for i in 0..2 {
            let Some(pt) = self.extras.portals[i] else { continue };
            let alive = self.world.bodies.get(pt.body).is_some_and(|b| b.is_some()) && pt.physical.is_none_or(|p| !self.physicals[p].destroyed && self.physicals[p].rigidbody.body == pt.body);
            if !alive {
                self.extras_portal_fx(i, pt.frame, pt.half, false);
                self.extras.portals[i] = None;
                continue;
            }
            let xf = self.world.body(pt.body).xf;
            self.extras.portals[i].as_mut().unwrap().frame = Frame { centre: arr(xf.apply(pt.centre)), normal: arr(xf.q.apply(pt.normal)) };
        }
    }

    /// The velocity of the surface carrying portal `i` at `p`.
    fn extras_surface_velocity(&self, i: usize, p: Vec2) -> (Vec2, f32) {
        let b = self.world.body(self.extras.portals[i].unwrap().body);
        (vec(point_velocity(arr(b.linear_velocity), b.angular_velocity, arr(b.sweep.c), arr(p))), b.angular_velocity)
    }

    /// Every point a body's solid shapes reach, with the radius around it.
    /// The share of `body`'s area behind portal frame `f` (through the doorway) and that part's centroid.
    fn extras_through_share(&self, body: BodyId, f: Frame) -> Option<(f32, Vec2)> {
        // Shoelace area and centroid of a polygon.
        fn area(poly: &[Vec2]) -> (f32, Vec2) {
            let (mut a, mut c) = (0.0, Vec2::ZERO);
            for k in 0..poly.len() {
                let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
                let cross = p.x * q.y - q.x * p.y;
                a += cross;
                c = c + (p + q) * cross;
            }
            if a.abs() < 1e-9 { (0.0, Vec2::ZERO) } else { (a.abs() * 0.5, c * (1.0 / (3.0 * a))) }
        }
        let b = self.world.body(body);
        let (mut all, mut cut, mut m) = (0.0, 0.0, Vec2::ZERO);
        for &fx in &b.fixtures {
            let fx = self.world.fixture(fx);
            if fx.is_sensor {
                continue;
            }
            let poly: Vec<Vec2> = match &fx.shape {
                Shape::Polygon(p) => p.vertices[..p.count].iter().map(|&v| b.xf.apply(v)).collect(),
                Shape::Circle(c) => (0..16).map(|k| {
                    let t = k as f32 * std::f32::consts::TAU / 16.0;
                    b.xf.apply(c.p + Vec2::new(t.cos(), t.sin()) * c.radius)
                }).collect(),
                Shape::Capsule(c) => {
                    let (p, q) = (c.vertices[0], c.vertices[1]);
                    let d = q - p;
                    let len = (d.x * d.x + d.y * d.y).sqrt().max(1e-6);
                    let n = Vec2::new(-d.y, d.x) * (c.radius / len);
                    [p - n, q - n, q + n, p + n].iter().map(|&v| b.xf.apply(v)).collect()
                }
                _ => continue,
            };
            all += area(&poly).0;
            // The polygon clipped to the far side of the portal line.
            let mut piece = Vec::new();
            for k in 0..poly.len() {
                let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
                let (dp, dq) = (f.local(arr(p))[1], f.local(arr(q))[1]);
                if dp < 0.0 {
                    piece.push(p);
                }
                if (dp < 0.0) != (dq < 0.0) {
                    piece.push(p + (q - p) * (dp / (dp - dq)));
                }
            }
            if piece.len() >= 3 {
                let (a, c) = area(&piece);
                cut += a;
                m = m + c * a;
            }
        }
        (cut > 0.0 && all > 0.0).then(|| ((cut / all).min(1.0), m * (1.0 / cut)))
    }

    fn extras_body_points(&self, body: BodyId) -> Vec<(Vec2, f32)> {
        let b = self.world.body(body);
        let mut out = Vec::new();
        for &f in &b.fixtures {
            let fx = self.world.fixture(f);
            if fx.is_sensor {
                continue;
            }
            match &fx.shape {
                Shape::Polygon(p) => out.extend(p.vertices[..p.count].iter().map(|&v| (b.xf.apply(v), p.radius))),
                Shape::Circle(c) => out.push((b.xf.apply(c.p), c.radius)),
                Shape::Capsule(c) => out.extend(c.vertices.iter().map(|&v| (b.xf.apply(v), c.radius))),
                Shape::Chain(c) => out.extend(c.vertices.iter().map(|&v| (b.xf.apply(v), c.radius))),
            }
        }
        out
    }

    fn extras_drop_crosser(&mut self, body: BodyId) {
        if let Some(c) = self.extras.crossers.remove(&body) {
            for f in self.world.body(c.ghost).fixtures.clone() {
                self.world.portal_ghosts.remove(&f);
            }
            self.world.destroy_body(c.ghost);
        }
    }

    /// A twin for `body`: its solid shapes, mass and colliders, no gravity, no damping, never asleep.
    fn extras_make_ghost(&mut self, body: BodyId) -> BodyId {
        let src = self.world.body(body).clone();
        let ghost = self.world.create_body(&BodyDef { kind: BodyType::Dynamic, position: src.xf.p, angle: src.sweep.a, gravity_scale: 0.0, allow_sleep: false, rigidbody: src.rigidbody, ..Default::default() });
        for &f in &src.fixtures {
            let fx = self.world.fixture(f).clone();
            if fx.is_sensor {
                continue;
            }
            let def = FixtureDef { shape: fx.shape, friction: fx.friction, restitution: fx.restitution, density: fx.density, is_sensor: false, filter: fx.filter, collider: fx.collider };
            let id = self.world.create_fixture(ghost, &def, false);
            self.world.portal_ghosts.insert(id);
        }
        let g = self.world.body_mut(ghost);
        (g.mass, g.inv_mass, g.i, g.inv_i, g.sweep.local_center) = (src.mass, src.inv_mass, src.i, src.inv_i, src.sweep.local_center);
        ghost
    }

    /// Where `body` is through portal `i`: its origin, angle, velocity and spin at the other portal.
    fn extras_through(&self, i: usize, body: BodyId) -> (Vec2, f32, Vec2, f32) {
        let pair = self.extras.pair(i).unwrap();
        let b = self.world.body(body);
        let c = vec(pair.map_point(arr(b.sweep.c)));
        let (se, we) = self.extras_surface_velocity(i, b.sweep.c);
        let (sx, wx) = self.extras_surface_velocity(1 - i, c);
        let v = rotate(b.linear_velocity - se, pair.rotation) + sx;
        (vec(pair.map_point(arr(b.xf.p))), b.sweep.a + pair.rotation, v, b.angular_velocity - we + wx)
    }

    fn extras_clear(&mut self) {
        for b in self.extras.crossers.keys().copied().collect::<Vec<_>>() {
            self.extras_drop_crosser(b);
        }
        self.extras.near = Default::default();
        self.world.portal_holes.clear();
    }

    /// Before b2World::Step: who is in a doorway, groups whose centre of mass went through cross over, the twins are
    /// set from their bodies and the holes from the portals.
    pub(super) fn extras_pre_step(&mut self, dt: f32) {
        if self.extras.portals.iter().all(Option::is_none) && self.extras.crossers.is_empty() {
            return;
        }
        self.extras_refresh_portals();
        if self.extras.portals.iter().any(Option::is_none) {
            self.extras_clear();
            return;
        }
        // Who is at each portal.
        let tool = self.tool.as_ref().map(|t| t.body);
        let mut near: [Vec<BodyId>; 2] = Default::default();
        for i in 0..2 {
            let pt = self.extras.portals[i].unwrap();
            let f = pt.frame;
            let t = f.tangent();
            // Out to REACH in front: a fast body can reach the doorway within a step from far off (filtered below).
            let corners = [(-1.0, REACH), (1.0, REACH), (-1.0, -DEPTH), (1.0, -DEPTH)].map(|(a, o)| [f.centre[0] + t[0] * a * (pt.half + 0.5) + f.normal[0] * o, f.centre[1] + t[1] * a * (pt.half + 0.5) + f.normal[1] * o]);
            let lower = Vec2::new(corners.iter().map(|c| c[0]).fold(f32::MAX, f32::min), corners.iter().map(|c| c[1]).fold(f32::MAX, f32::min));
            let upper = Vec2::new(corners.iter().map(|c| c[0]).fold(f32::MIN, f32::max), corners.iter().map(|c| c[1]).fold(f32::MIN, f32::max));
            let mut bodies: Vec<BodyId> = self.world.query_aabb(&crate::b2::broadphase::Aabb { lower, upper }).into_iter().map(|(fx, _)| self.world.fixture(fx).body).collect();
            bodies.sort_unstable();
            bodies.dedup();
            for b in bodies {
                let body = self.world.body(b);
                if body.kind != BodyType::Dynamic || body.user.is_none() || b == pt.body || Some(b) == tool {
                    continue;
                }
                let (bc, bv) = (body.sweep.c, body.linear_velocity);
                let centre_out = f.local(arr(bc))[1];
                let crossing = self.extras.crossers.get(&b).map(|c| c.portal);
                if crossing == Some(1 - i) {
                    continue;
                }
                // Further than 1 out: only a body that reaches the doorway within two steps counts (so its contacts with
                // the surface lose the opening in time instead of it bouncing off), aimed at the opening.
                if crossing.is_none() {
                    let points = self.extras_body_points(b);
                    let nearest = points.iter().map(|&(p, r)| f.local(arr(p))[1] - r).fold(f32::MAX, f32::min);
                    if nearest > 1.0 {
                        let vn = dot(bv, vec(f.normal));
                        if vn >= 0.0 || nearest > 1.0 - vn * dt * 2.0 {
                            continue;
                        }
                        let along = f.local(arr(bc + bv * (centre_out / -vn).max(0.0)))[0];
                        let size = points.iter().map(|&(p, r)| (p - bc).length() + r).fold(0.0, f32::max);
                        if along.abs() > pt.half + size {
                            continue;
                        }
                    }
                }
                if centre_out > 0.0 || crossing == Some(i) {
                    near[i].push(b);
                }
                let behind = self.extras_body_points(b).iter().any(|&(p, r)| {
                    let [along, out] = f.local(arr(p));
                    out - r < -0.01 && out > -DEPTH && along.abs() < pt.half + r
                });
                match (crossing, behind) {
                    (Some(_), false) => self.extras_drop_crosser(b),
                    (None, true) if centre_out > 0.0 => {
                        let ghost = self.extras_make_ghost(b);
                        self.extras.crossers.insert(b, Crosser { portal: i, ghost, v0: Vec2::ZERO, w0: 0.0, c0: Vec2::ZERO, a0: 0.0, k: 1.0 });
                    }
                    _ => {}
                }
            }
        }
        // A crosser no longer near its portal at all.
        let gone: Vec<BodyId> = self.extras.crossers.iter().filter(|(b, c)| !near[c.portal].contains(b)).map(|(&b, _)| b).collect();
        for b in gone {
            self.extras_drop_crosser(b);
        }
        // A group whose centre of mass reaches the doorway within this step goes through now: at speed it could pass
        // the doorway (and the wall's DEPTH) inside one step and never be seen behind it.
        for i in 0..2 {
            let pt = self.extras.portals[i].unwrap();
            let (f, n) = (pt.frame, vec(pt.frame.normal));
            let mut seen: Vec<BodyId> = Vec::new();
            for b in near[i].clone() {
                if seen.contains(&b) {
                    continue;
                }
                let group = self.extras_group(b);
                seen.extend(&group);
                if self.extras_tethered(&group) || group.iter().any(|g| self.extras.crossers.get(g).is_some_and(|c| c.portal != i)) {
                    continue;
                }
                let (mut m, mut c, mut v) = (0.0, Vec2::ZERO, Vec2::ZERO);
                for &g in &group {
                    let body = self.world.body(g);
                    m += body.mass;
                    c = c + body.sweep.c * body.mass;
                    v = v + body.linear_velocity * body.mass;
                }
                if m <= 0.0 {
                    continue;
                }
                let (c, v) = (c * (1.0 / m), v * (1.0 / m));
                let (out, vn) = (f.local(arr(c))[1], dot(v, n));
                if out <= 0.0 || vn >= 0.0 || out > -vn * dt || f.local(arr(c + v * (out / -vn)))[0].abs() > pt.half {
                    continue;
                }
                // Moved along its path to just past the doorway; the crossing below takes it through.
                let shift = v * ((out + 0.02) / -vn);
                for &g in &group {
                    let body = self.world.body(g);
                    let (p, a) = (body.xf.p + shift, body.sweep.a);
                    self.world.set_transform(g, p, a);
                    if !self.extras.crossers.contains_key(&g) {
                        let ghost = self.extras_make_ghost(g);
                        self.extras.crossers.insert(g, Crosser { portal: i, ghost, v0: Vec2::ZERO, w0: 0.0, c0: Vec2::ZERO, a0: 0.0, k: 1.0 });
                    }
                    if !near[i].contains(&g) {
                        near[i].push(g);
                    }
                }
            }
        }
        // Groups whose centre of mass is through cross over whole.
        let mut done: Vec<BodyId> = Vec::new();
        let starts: Vec<(BodyId, usize)> = self.extras.crossers.iter().map(|(&b, c)| (b, c.portal)).collect();
        for (b, i) in starts {
            if done.contains(&b) {
                continue;
            }
            let group = self.extras_group(b);
            done.extend(&group);
            if self.extras_tethered(&group) || group.iter().any(|g| self.extras.crossers.get(g).is_some_and(|c| c.portal != i)) {
                continue;
            }
            let (mut m, mut c) = (0.0, Vec2::ZERO);
            for &g in &group {
                let body = self.world.body(g);
                m += body.mass;
                c = c + body.sweep.c * body.mass;
            }
            let frame = self.extras.portals[i].unwrap().frame;
            if m <= 0.0 || frame.local(arr(c * (1.0 / m)))[1] >= 0.0 {
                continue;
            }
            let through: Vec<_> = group.iter().map(|&g| (g, self.extras_through(i, g))).collect();
            for (g, (p, a, v, w)) in through {
                self.world.set_transform(g, p, a);
                let body = self.world.body_mut(g);
                (body.linear_velocity, body.angular_velocity) = (v, w);
                body.wake();
                match self.extras.crossers.get_mut(&g) {
                    Some(c) => c.portal = 1 - i,
                    None => {
                        let ghost = self.extras_make_ghost(g);
                        self.extras.crossers.insert(g, Crosser { portal: 1 - i, ghost, v0: Vec2::ZERO, w0: 0.0, c0: Vec2::ZERO, a0: 0.0, k: 1.0 });
                    }
                }
                near[i].retain(|&x| x != g);
                near[1 - i].push(g);
            }
        }
        // The twins, where their bodies are through the doorway, weighing their body's share of its group.
        let crossers: Vec<(BodyId, usize, BodyId)> = self.extras.crossers.iter().map(|(&b, c)| (b, c.portal, c.ghost)).collect();
        for &(b, i, ghost) in &crossers {
            let group = self.extras_group(b);
            let mass = |g: &Game, f: &dyn Fn(&BodyId) -> bool| group.iter().filter(|x| f(x)).map(|&x| g.world.body(x).mass).sum::<f32>();
            let whole = mass(self, &|_| true);
            let crossing = mass(self, &|x| self.extras.crossers.get(x).is_some_and(|c| c.portal == i));
            let k = if crossing > 0.0 { (whole / crossing).clamp(1.0, 8.0) } else { 1.0 };
            let (p, a, v, w) = self.extras_through(i, b);
            self.world.set_transform(ghost, p, a);
            let src = self.world.body_mut(b);
            src.wake();
            let (m, im, ii, iii) = (src.mass, src.inv_mass, src.i, src.inv_i);
            let g = self.world.body_mut(ghost);
            (g.mass, g.inv_mass, g.i, g.inv_i) = (m * k, im / k, ii * k, iii / k);
            (g.linear_velocity, g.angular_velocity) = (v, w);
            g.wake();
            let (c0, a0) = (g.sweep.c, g.sweep.a);
            let c = self.extras.crossers.get_mut(&b).unwrap();
            (c.v0, c.w0, c.c0, c.a0, c.k) = (v, w, c0, a0, k);
        }
        // Gravity at the far side: the part of a body through the doorway falls the way the exit side's gravity goes,
        // seen from the entry side (a force at that part's centroid, on its share of the body's area; area, not corners,
        // so the pull grows smoothly as the body goes in instead of jumping and rocking it).
        let gravity = self.world.gravity;
        for &(b, i, _) in &crossers {
            let (pair, f) = (self.extras.pair(i).unwrap(), self.extras.portals[i].unwrap().frame);
            let Some((share, at)) = self.extras_through_share(b, f) else { continue };
            let body = self.world.body_mut(b);
            let force = (rotate(gravity, -pair.rotation) - gravity) * (body.mass * body.gravity_scale * share);
            let r = at - body.sweep.c;
            body.linear_velocity = body.linear_velocity + force * (dt * body.inv_mass);
            body.angular_velocity += (r.x * force.y - r.y * force.x) * dt * body.inv_i;
        }
        self.world.portal_holes = (0..2)
            .map(|i| {
                let pt = self.extras.portals[i].unwrap();
                let mut allowed = near[i].clone();
                allowed.extend(crossers.iter().filter(|c| c.1 == 1 - i).map(|c| c.2));
                PortalHole { body: pt.body, centre: pt.centre, normal: pt.normal, half_len: pt.half, depth: DEPTH, allowed }
            })
            .collect();
        self.extras.near = near;
    }

    /// After b2World::Step: what the twins met at the far side (their change of velocity and their push out of what
    /// they hit) goes back through the doorway to their bodies.
    pub(super) fn extras_post_step(&mut self, dt: f32) {
        let crossers: Vec<(BodyId, usize, BodyId, Vec2, f32, Vec2, f32, f32)> = self.extras.crossers.iter().map(|(&b, c)| (b, c.portal, c.ghost, c.v0, c.w0, c.c0, c.a0, c.k)).collect();
        for (b, i, ghost, v0, w0, c0, a0, k) in crossers {
            let Some(pair) = self.extras.pair(i) else { continue };
            let g = self.world.body(ghost);
            // The twin's impulse, on the body's own mass.
            let (dv, dw) = ((g.linear_velocity - v0) * k, (g.angular_velocity - w0) * k);
            let dc = g.sweep.c - (c0 + g.linear_velocity * dt);
            let da = g.sweep.a - (a0 + g.angular_velocity * dt);
            let body = self.world.body_mut(b);
            body.linear_velocity = body.linear_velocity + rotate(dv, -pair.rotation);
            body.angular_velocity += dw;
            if dc.x == 0.0 && dc.y == 0.0 && da == 0.0 {
                continue;
            }
            let (c, a) = (body.sweep.c + rotate(dc, -pair.rotation), body.sweep.a + da);
            let origin = c - Rot::new(a).apply(body.sweep.local_center);
            self.world.set_transform(b, origin, a);
        }
    }

    /// Once a frame, after the scripts' Update: a drag lets go when the cursor reaches the line of the portal the
    /// dragged object is going into (the same as letting go of the mouse button).
    pub(crate) fn extras_update(&mut self) {
        if self.extras.portals.iter().all(Option::is_none) {
            return;
        }
        self.extras_refresh_portals();
        self.extras_drift_sparks();
        let Some(t) = self.tool.as_ref() else { return };
        let (Some(j), Some(sel)) = (t.dragJoint.as_ref(), t.selectedRigidbody) else { return };
        if j.destroy_at.is_some() || sel >= self.physicals.len() {
            return;
        }
        let group = self.extras_group(self.physicals[sel].rigidbody.body);
        let mouse = [self.MousePosition[0], self.MousePosition[1]];
        for i in 0..2 {
            let Some(pt) = self.extras.portals[i] else { continue };
            let going_in = group.iter().any(|b| self.extras.near[i].contains(b) || self.extras.crossers.get(b).is_some_and(|c| c.portal == i));
            let [along, out] = pt.frame.local(mouse);
            if going_in && out <= 0.0 && along.abs() <= pt.half {
                let fast = self.input.held("fast");
                self.escape_and_deselect();
                self.stop_selection_drag(fast);
                return;
            }
        }
    }

    /// A shot from `origin` along `dir` that reaches a portal's opening within `dist`: how far, and where it goes on
    /// from just out of the other portal, and which way.
    pub(super) fn extras_ray_portal(&self, origin: [f32; 2], dir: [f32; 2], dist: f32) -> Option<(f32, [f32; 2], [f32; 2])> {
        let mut best: Option<(f32, [f32; 2], [f32; 2])> = None;
        for i in 0..2 {
            let (Some(pt), Some(pair)) = (self.extras.portals[i], self.extras.pair(i)) else { continue };
            let f = pt.frame;
            let out = f.local(origin)[1];
            let dn = dir[0] * f.normal[0] + dir[1] * f.normal[1];
            if out < -1e-4 || dn >= -1e-6 {
                continue;
            }
            let t = out / -dn;
            let p = [origin[0] + dir[0] * t, origin[1] + dir[1] * t];
            if t > dist || best.is_some_and(|b| t >= b.0) || f.local(p)[0].abs() > pt.half {
                continue;
            }
            let (q, d) = (pair.map_point(p), pair.map_dir(dir));
            best = Some((t, [q[0] + d[0] * 1e-3, q[1] + d[1] * 1e-3], d));
        }
        best
    }

    /// The body a physical's sprite moves with, when it is in a doorway: its pair (into, out of).
    pub fn extras_doorway(&self, body: BodyId) -> Option<Pair> {
        self.extras.pair(self.extras.crossers.get(&body)?.portal)
    }
}
