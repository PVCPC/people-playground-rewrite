//! The [Obsolete] weapons and flying parts: ScherbewerferBehaviour (launches its Projectile), NeedlerBehaviour (fires
//! NeedleBehaviour needles), NeedleBehaviour (a homing needle), MachineGunProjectileBehaviour (a bullet stepped by raycast),
//! PlaneBehaviour and WingBehaviour (thrust, lift and the fuel tank rupture) and GlassShardBehaviour (a flying shard that
//! cuts what it passes through). Muzzle flashes, gizmos and the wing's fire particle are presentation.
#![allow(non_snake_case)]

use super::coroutine::{Routine, Yield};
use super::destroyable::AttachParent;
use super::explosion::{normalized3, ExplosionParameters};
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::game_assets::{Curve, CurveCache};

/// LayerMask.GetMask("Objects", "Bounds")
const OBJECTS_AND_BOUNDS: u32 = (1 << 10) | (1 << 11);

script! {
    Scherbewerfer {
        BaseLaunchForce: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// The physical objects of Projectile and PhysicalBehaviour.
        Projectile: usize = 0,
        PhysicalBehaviour: usize = 0,
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
    }
}

script! {
    Needler {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        FireInterval: f32 = 0.04,
        FireVolume: f32 = 0.5,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// The launcher's PhysicalBehaviour (base.transform).
        physical: usize = 0,
        /// FireSounds.Length
        FireSounds: usize = 0,
        needle: Option<ObjRef> = None,
    }
}

script! {
    Needle {
        Speed: f32 = 10.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// target: the collider whose Transform it homes on.
        target: Option<i32> = None,
        hit: bool = false,
        /// The GameObject's Collider2Ds and Rigidbody2D (0 without one): the trigger messages it receives.
        colliders: Vec<i32> = Vec::new(),
        rigidbody: i32 = 0,
        staticNeedle: Option<ObjRef> = None,
    }
}

script! {
    StaticNeedle {
    }
    links {
        /// The component's instance id, and its GameObject's Transform id.
        id: i32 = 0,
        root: i32 = 0,
        /// limb: the LimbBehaviour (`Game::limbs` index) it is stuck in.
        limb: Option<usize> = None,
        /// GetComponent<ExplosiveBehaviour>() (`Game::explosives` index).
        explosive: usize = 0,
        explosion: Option<ObjRef> = None,
    }
}

script! {
    MachineGunProjectile {
        UnitsPerSecond: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        mask: u32 = 0,
        f: f32 = 0.0,
    }
}

script! {
    Plane {
        speed: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        activated: bool = false,
        /// The physical object of its Rigidbody2D.
        rigidbody: usize = 0,
        /// The component's index on its GameObject, and whether it comes after the PhysicalBehaviour (collision `side` order).
        physical: Option<usize> = None,
        index: usize = 0,
        after_physical: bool = false,
    }
}

script! {
    Wing {
        LiftForce: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Its own Rigidbody2D (Awake).
        rigidbody: usize = 0,
        /// liftPosition's Transform (a component id of its object).
        liftPosition: i32 = 0,
        /// plane: the PlaneBehaviour's index in `Game::planes`.
        plane: usize = 0,
        ruptured: bool = false,
        physical: Option<usize> = None,
        index: usize = 0,
        after_physical: bool = false,
    }
}

script! {
    Shockwave {
        Duration: f32 = 0.02,
        SizeMultiplier: f32 = 5.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GrowingCurve (AnimationCurve.Linear(0, 0, 1, 1) unless serialised otherwise).
        GrowingCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        startSize: f32 = 0.0,
        t: f32 = 0.0,
    }
}

script! {
    GlassShard {
        SpeedThreshold: f32 = 5.0,
        RandomThreshold: f32 = 0.5,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Rigidbody: the physical object of its Rigidbody2D.
        Rigidbody: usize = 0,
    }
}

/// Vector3 * float: each component a float product (computed in double, as the Mono JIT does, and rounded).
fn scale3(v: [f32; 3], k: f32) -> [f32; 3] {
    [(d(v[0]) * k).f(), (d(v[1]) * k).f(), (d(v[2]) * k).f()]
}

impl Game {
    // ---- ScherbewerferBehaviour ----------------------------------------------------------------------------------

    /// ScherbewerferBehaviour.Use(activation): Launch.
    pub fn scherbewerfer_use(&mut self, i: usize) {
        self.scherbewerfer_launch(i);
    }

    /// ScherbewerferBehaviour.Launch: an impulse of transform.right * (BaseLaunchForce + charge) on the Projectile and its
    /// negative on the launcher (the Vector3 converts to the Vector2 AddForce takes).
    pub(crate) fn scherbewerfer_launch(&mut self, i: usize) {
        let s = &self.scherbewerfers[i];
        let (projectile, p, base) = (s.Projectile, s.PhysicalBehaviour, s.BaseLaunchForce);
        let vector = scale3(self.xf_right(&s.own), (d(base) + self.physicals[p].charge).f());
        let rb = self.physicals[projectile].rigidbody.clone();
        rb.add_force_impulse(&mut self.world, [vector[0], vector[1]]);
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_force_impulse(&mut self.world, [-vector[0], -vector[1]]);
    }

    // ---- NeedlerBehaviour ----------------------------------------------------------------------------------------

    /// NeedlerBehaviour.GetBarrelPosition: transform.TransformPoint(barrelPosition) as a Vector2.
    pub(crate) fn needler_barrel_position(&self, i: usize) -> [f32; 2] {
        self.transform_point(self.needlers[i].physical, self.needlers[i].barrelPosition)
    }

    // NeedlerBehaviour.GetBarrelDirection: presentation (only the muzzle flash's `right` reads it)
    // NeedlerBehaviour.OnDrawGizmos: presentation (Gizmos.DrawRay draws in the editor only)
    // NeedleBehaviour.Start: empty
    // NeedlerBehaviour.constructor: FireInterval = 0.04f, FireVolume = 0.5f (the script! defaults)

    /// NeedlerBehaviour.Use: the flash is presentation; Audio.PlayOneShot(FireSounds.PickRandom(), FireVolume); a needle is
    /// instantiated at the barrel with the launcher's rotation and its `right` set to transform.right * localScale.x.
    pub fn needler_use(&mut self, i: usize) {
        let s = self.needlers[i].clone();
        if let Some(t) = self.field_component(s.id, "flash") {
            let at = self.needler_barrel_position(i);
            let d = self.transform_direction(s.physical, [s.barrelDirection[0], s.barrelDirection[1], 0.0]);
            let k = self.physicals[s.physical].transform.localScale[0];
            let q = crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], [d[0] * k, d[1] * k, 0.0]);
            self.with_component_instance(t, |_, inst, o| {
                inst.set_world_position(o, [at[0], at[1], 0.0]);
                inst.set_world_rotation(o, q);
            });
        }
        self.ps_field_play_stop(s.id, "flash", true);
        let clip = self.pick_clip(s.id, "FireSounds", s.FireSounds);
        let src = self.audio_src(s.id, "Audio");
        self.audio.play_one_shot(src, clip.as_ref(), s.FireVolume);
        let at = self.needler_barrel_position(i);
        let rotation = self.transform_rotation(s.physical);
        let needle = s.needle.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let id = self.spawn_prefab_rotated(needle, [at[0], at[1], 0.0], rotation);
        let right = scale3(self.transform_right(s.physical), self.physicals[s.physical].transform.localScale[0]);
        self.with_component_instance(id, |_, inst, o| inst.set_world_rotation(o, crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], right)));
    }

    // ---- NeedleBehaviour -----------------------------------------------------------------------------------------

    /// NeedleBehaviour.Update: transform.position += transform.right * Time.smoothDeltaTime * Speed.
    pub fn needle_update(&mut self, i: usize) {
        let (id, speed) = (self.needles[i].id, self.needles[i].Speed);
        let smooth = self.time.smoothDeltaTime;
        self.with_component_instance(id, |_, inst, o| {
            let right = mathf::rotate(inst.rotation(o), [1.0, 0.0, 0.0]);
            let step = scale3(scale3(right, smooth), speed);
            let p = inst.position(o);
            inst.set_world_position(o, [(d(p[0]) + step[0]).f(), (d(p[1]) + step[1]).f(), (d(p[2]) + step[2]).f()]);
        });
    }

    /// NeedleBehaviour.FixedUpdate: every collider within 15 of a point 7.5 ahead, farthest first; each one whose root is an
    /// AliveBehaviour that is alive and is the first thing a 15 long ray toward it hits becomes the target (the last, nearest
    /// one stays). The needle turns 0.2 of the way toward the target, which it drops within 15.
    pub fn needle_fixed_update(&mut self, i: usize) {
        let id = self.needles[i].id;
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let centre = [(d(position[0]) + (d(right[0]) * 7.5f32)).f(), (d(position[1]) + (d(right[1]) * 7.5f32)).f()];
        let mut hits = self.overlap_circle(V::new(centre[0], centre[1]), 15.0, super::explosion::DEFAULT_RAYCAST_LAYERS);
        let sqr = |g: &Self, c: i32| {
            let at = g.collider_position(c);
            let v = [(d(at[0]) - position[0]).f(), (d(at[1]) - position[1]).f(), (d(at[2]) - position[2]).f()];
            (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).f()
        };
        // orderby ... descending: a stable sort.
        hits.sort_by(|a, b| sqr(self, b.1).partial_cmp(&sqr(self, a.1)).unwrap());
        for (physical, collider) in hits {
            let Some(person) = self.alive_of_collider(collider, physical) else { continue };
            if !self.person_is_alive(person) {
                continue;
            }
            let at = self.collider_position(collider);
            let direction = [(d(at[0]) - position[0]).f(), (d(at[1]) - position[1]).f()];
            if self.raycast_first(V::new(position[0], position[1]), direction, 15.0).is_some_and(|h| h.collider == collider) {
                self.needles[i].target = Some(collider);
            }
        }
        let Some(target) = self.needles[i].target else { return };
        let at = self.collider_position(target);
        let to = normalized3([(d(at[0]) - position[0]).f(), (d(at[1]) - position[1]).f(), (d(at[2]) - position[2]).f()]);
        // Vector2.Lerp(right, to, 0.2f), assigned as a Vector3 (z 0).
        let lerp = |a: f32, b: f32| (d(a) + (d(b) - a) * 0.2f32).f();
        let new_right = [lerp(right[0], to[0]), lerp(right[1], to[1]), 0.0];
        self.with_component_instance(id, |_, inst, o| inst.set_world_rotation(o, crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], new_right)));
        let (dx, dy) = ((d(at[0]) - position[0]).f(), (d(at[1]) - position[1]).f());
        let distance = (d(dx) * dx + d(dy) * dy).0.sqrt() as f32;
        if distance < 15.0 {
            self.needles[i].target = None;
        }
    }

    /// Whether NeedleBehaviour `i` gets the trigger messages of collider `own`.
    pub(super) fn needle_receives(&self, i: usize, own: i32, rigidbody: Option<crate::b2::world::RigidbodyRef>) -> bool {
        let s = &self.needles[i];
        super::decimator::trigger_receives(&s.colliders, s.rigidbody, own, rigidbody)
    }

    /// NeedleBehaviour.OnTriggerEnter2D: the first solid collider pushes its Rigidbody2D with an impulse of right * 0.05 at
    /// the needle, gets a StaticNeedle at its closest point turned along the needle (parented under the collider's
    /// Transform, remembering the limb), and the needle is destroyed.
    pub fn needle_trigger_enter(&mut self, i: usize, collision: i32) {
        if self.needles[i].hit || self.world.collider_is_trigger(collision) != Some(false) {
            return;
        }
        let (id, source) = (self.needles[i].id, self.needles[i].staticNeedle);
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        // GetComponent<LimbBehaviour>() on the collider's own GameObject.
        let owner = self.physical_of_collider(collision).filter(|&p| self.physicals[p].colliders.contains(&collision));
        let limb = owner.and_then(|p| self.limbs.iter().position(|l| l.PhysicalBehaviour == p));
        if let Some(rb) = self.world.collider_body(collision).and_then(|b| self.rigidbody_of_body(b)) {
            let force = scale3(right, 0.05);
            rb.add_impulse_at_position(&mut self.world, [force[0], force[1]], [position[0], position[1]]);
        }
        let z = (d(57.29578f32) * crate::game::mathf::atan2(right[1], right[0])).f();
        self.sync_pending();
        let closest = self.world.collider_closest_point(collision, V::new(position[0], position[1]));
        let source = source.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let at = [closest.x, closest.y, 0.0];
        let rotation = super::physical::z_rotation(z);
        let root = self.spawn_prefab_rotated(source, at, rotation);
        let parent = match owner {
            Some(p) => AttachParent::Physical(p),
            None if self.bare_colliders.contains_key(&collision) => AttachParent::Bare(collision),
            None => unimplemented!("mod-only: NeedleBehaviour.OnTriggerEnter2D: SetParent under a collider that is neither a physical's nor bare"),
        };
        self.attach_clone_world(root, parent, at, rotation);
        let n = self.static_needles.iter().rposition(|x| x.root == root).expect("the staticNeedle prefab's StaticNeedleBehaviour");
        self.static_needles[n].limb = limb;
        self.destroy_object(id);
        self.needles[i].hit = true;
    }

    // ---- StaticNeedleBehaviour -----------------------------------------------------------------------------------

    /// StaticNeedleBehaviour.Start: stuck in a limb it Shot()s it (0.3 along -right); the seventh needle in that limb's
    /// hierarchy makes a NeedlerSuperExplosion and a 32 ray, range 1 explosion of the needle itself, else it explodes
    /// after 3 seconds; in nothing it is deleted after 5.
    pub fn static_needle_start(&mut self, i: usize) {
        let (id, root, limb) = (self.static_needles[i].id, self.static_needles[i].root, self.static_needles[i].limb);
        let Some(l) = limb else {
            self.add_delete_after(root, 5.0);
            return;
        };
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let normal = scale3(right, -1.0);
        let p = self.limbs[l].PhysicalBehaviour;
        self.send_shot(0, Some(p), [normal[0], normal[1]], [position[0], position[1]], 0.3, false, None);
        if self.static_needles_in_children(p) > 6 {
            self.spawn_resource("Prefabs/NeedlerSuperExplosion", position);
            let e = self.static_needles[i].explosive;
            let x = &mut self.explosives[e];
            (x.FragmentForce, x.FragmentationRayCount, x.Range) = (1.0, 32, 1.0);
            self.explosive_activate(e);
        } else {
            self.start_coroutine(id, id, Self::static_needle_routine(i));
        }
    }

    /// limb.GetComponentsInChildren<StaticNeedleBehaviour>().Length: the needles hung under the Transform of physical `p`
    /// or of an active object below it.
    fn static_needles_in_children(&mut self, p: usize) -> usize {
        let objects = self.with_component_instance(self.physicals[p].id, |_, inst, o| {
            std::iter::once(o).chain(inst.descendants(o)).filter(|&x| inst.objects[x].active).map(|x| inst.objects[x].components[0]).collect::<Vec<i32>>()
        });
        let mut count = 0;
        for key in objects {
            let attached: Vec<i32> = self.attached_clones.get(&key).map(|v| v.iter().map(|c| c.id).collect()).unwrap_or_default();
            for id in attached {
                if self.static_needles.iter().any(|n| n.root == id) && !self.destroyed_components.contains(&id) && self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
                    count += 1;
                }
            }
        }
        count
    }

    /// StaticNeedleBehaviour.WaitAndExplode (StaticNeedleBehaviour.MoveNext): WaitForSeconds(3), then the explosion
    /// prefab at the needle and its ExplosiveBehaviour's Activate.
    pub(crate) fn static_needle_routine(i: usize) -> Routine {
        let mut waited = false;
        Box::new(move |g| {
            if !waited {
                waited = true;
                return Yield::WaitForSeconds(3.0);
            }
            let (id, explosion, e) = (g.static_needles[i].id, g.static_needles[i].explosion, g.static_needles[i].explosive);
            let position = g.with_component_instance(id, |_, inst, o| inst.position(o));
            let explosion = explosion.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            g.spawn_prefab(explosion, position);
            g.explosive_activate(e);
            Yield::Done
        })
    }

    // ---- MachineGunProjectileBehaviour ---------------------------------------------------------------------------

    /// MachineGunProjectileBehaviour.Awake: mask = LayerMask.GetMask("Objects", "Bounds").
    pub fn machine_gun_projectile_awake(&mut self, i: usize) {
        self.machine_gun_projectiles[i].mask = OBJECTS_AND_BOUNDS;
    }

    /// MachineGunProjectileBehaviour.Update: the direction is transform.right bent down by f, normalised; a ray of
    /// UnitsPerSecond * deltaTime hitting something destroys the bullet, explodes at the hit pulled back 0.05 and sends
    /// Shot, ExitShot (35) and Break to what it hit; else the bullet moves along the ray. f grows 0.1 per second.
    pub fn machine_gun_projectile_update(&mut self, i: usize) {
        let s = self.machine_gun_projectiles[i].clone();
        let (position, rotation) = self.with_component_instance(s.id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let mut right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        right[1] = (d(right[1]) - s.f).f();
        let right = normalized3(right);
        let dt = self.time.deltaTime;
        let num = (d(s.UnitsPerSecond) * dt).f();
        match self.raycast(V::new(position[0], position[1]), [right[0], right[1]], num, s.mask, f32::NEG_INFINITY, f32::INFINITY) {
            Some(hit) => {
                let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
                let vector = [(d(point[0]) + d(normal[0]) * 0.05f32).f(), (d(point[1]) + d(normal[1]) * 0.05f32).f()];
                self.destroy_object(s.id);
                self.create_fragmentation_explosion(ExplosionParameters::new_big(8, [vector[0], vector[1], 0.0], 2.0, 4.0, true, false, 0.0, 0));
                let physical = self.hit_transform_physical(&hit);
                self.send_shot(hit.collider, physical, normal, point, 35.0, false, None);
                self.send_shot(hit.collider, physical, normal, point, 35.0, true, None);
                self.send_break(hit.collider, physical, [(d(num) * right[0]).f(), (d(num) * right[1]).f()]);
            }
            None => {
                let step = scale3(right, num);
                let to = [(d(position[0]) + step[0]).f(), (d(position[1]) + step[1]).f(), (d(position[2]) + step[2]).f()];
                self.with_component_instance(s.id, |_, inst, o| inst.set_world_position(o, to));
            }
        }
        self.machine_gun_projectiles[i].f = (d(s.f) + d(dt) * 0.1f32).f();
    }

    // ---- PlaneBehaviour and WingBehaviour ------------------------------------------------------------------------

    /// PlaneBehaviour.Use: activated flips.
    pub fn plane_use(&mut self, i: usize) {
        self.planes[i].activated = !self.planes[i].activated;
    }

    /// PlaneBehaviour.FixedUpdate: while activated, a relative force of speed * transform.root.localScale.x along the body's x.
    pub fn plane_fixed_update(&mut self, i: usize) {
        let s = &self.planes[i];
        if !s.enabled || !s.activated {
            return;
        }
        let (id, speed, body) = (s.id, s.speed, s.rigidbody);
        let root_scale = self.with_component_instance(id, |_, inst, mut o| {
            while let Some(up) = inst.objects[o].parent {
                o = up;
            }
            inst.objects[o].trs.s[0]
        });
        let rb = self.physicals[body].rigidbody.clone();
        rb.add_relative_force(&mut self.world, [(d(speed) * root_scale).f(), 0.0]);
    }

    /// PlaneBehaviour.OnCollisionEnter2D: GetContact(0).normalImpulse above 85 broadcasts FuelTankRupture to the plane's
    /// object and the active objects below it, whose WingBehaviours run it (Debug.Log is the log).
    pub fn plane_on_collision_enter(&mut self, i: usize, c: &crate::b2::contacts2d::ScriptCollision) {
        let impulse = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.")).normal_impulse;
        if impulse > 85.0 {
            let ids = self.with_component_instance(self.planes[i].id, |_, inst, o| {
                std::iter::once(o).chain(inst.descendants(o)).filter(|&x| inst.objects[x].active).flat_map(|x| inst.objects[x].components.clone()).collect::<Vec<i32>>()
            });
            for w in 0..self.wings.len() {
                if ids.contains(&self.wings[w].id) {
                    self.wing_fuel_tank_rupture(w);
                }
            }
        }
    }

    /// WingBehaviour.Awake: rigidbody = GetComponent<Rigidbody2D>() (resolved at link).
    pub fn wing_awake(&mut self, _w: usize) {}

    /// WingBehaviour.FixedUpdate: lift of |v . right| * 0.01 * |v|^2 (at most 1500) along transform.up at liftPosition,
    /// and a torque turning the wing toward its velocity (the angle error at most 10 degrees) scaled by that lift.
    pub fn wing_fixed_update(&mut self, w: usize) {
        let s = &self.wings[w];
        let (id, body, lift_id, force) = (s.id, s.rigidbody, s.liftPosition, s.LiftForce);
        let rb = self.physicals[body].rigidbody.clone();
        let velocity = rb.velocity(&self.world);
        let (rotation, scale_unused) = self.with_component_instance(id, |_, inst, o| (inst.rotation(o), ()));
        let _ = scale_unused;
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let up = mathf::rotate(rotation, [0.0, 1.0, 0.0]);
        let dot = (d(velocity[0]) * right[0] + d(velocity[1]) * right[1]).f();
        let value = (d(dot.abs()) * (d(0.01f32) * mathf::sqr_magnitude(velocity))).f();
        let value = mathf::clamp(value, 0.0, 1500.0);
        let lift_at = self.with_component_instance(lift_id, |_, inst, o| inst.position(o));
        // transform.up * LiftForce * value: two Vector3 * float operators, each rounding its components.
        let f = |u: f32| ((d(u) * force).f() * value);
        rb.add_force_at_position(&mut self.world, [f(up[0]), f(up[1])], [lift_at[0], lift_at[1]]);
        let heading = (d(57.29578f32) * crate::game::mathf::atan2(velocity[1], velocity[0])).f();
        let value2 = mathf::clamp(mathf::delta_angle(crate::game::dump::euler_z(rotation), heading), -10.0, 10.0);
        rb.add_torque(&mut self.world, (d(value2) * value * force * 0.1f32).f());
    }

    /// WingBehaviour.OnCollisionEnter2D: GetContact(0).normalImpulse above 30 ruptures the fuel tank.
    pub fn wing_on_collision_enter(&mut self, w: usize, c: &crate::b2::contacts2d::ScriptCollision) {
        let impulse = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.")).normal_impulse;
        if impulse > 30.0 {
            self.wing_fuel_tank_rupture(w);
        }
    }

    /// WingBehaviour.FuelTankRupture: once, the plane switches off, ignites and a 128 ray explosion goes off at the wing
    /// (the fire particle is presentation).
    pub fn wing_fuel_tank_rupture(&mut self, w: usize) {
        if self.wings[w].ruptured {
            return;
        }
        self.wings[w].ruptured = true;
        let (plane, id) = (self.wings[w].plane, self.wings[w].id);
        self.planes[plane].enabled = false;
        if let Some(p) = self.planes[plane].physical {
            self.ignite(p, false);
        }
        let at = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.create_fragmentation_explosion(ExplosionParameters::new_big(128, at, 25.0, 4.0, true, true, 0.0, 0));
    }

    // ---- ShockwaveBehaviour --------------------------------------------------------------------------------------

    /// ShockwaveBehaviour.Start: startSize = transform.localScale.x; renderer.enabled = false (the MeshRenderer, which nothing
    /// enables again, so it is never drawn: the rewrite draws no MeshRenderer of the prefab).
    pub fn shockwave_start(&mut self, i: usize) {
        let id = self.shockwaves[i].id;
        self.shockwaves[i].startSize = self.with_component_instance(id, |_, inst, o| inst.objects[o].trs.s[0]);
    }

    /// ShockwaveBehaviour.Update: t += deltaTime; the scale is GrowingCurve.Evaluate(t / Duration) * SizeMultiplier + startSize
    /// in x and y (z 0); the material's _Intensity is presentation; the object is destroyed once t passes Duration.
    pub fn shockwave_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let s = &mut self.shockwaves[i];
        s.t = (d(s.t) + dt).f();
        let num = (d(s.t) / s.Duration).f();
        let curve = s.GrowingCurve.clone().expect("ShockwaveBehaviour.GrowingCurve");
        let num2 = (d(curve.evaluate(&mut s.cache, num)) * s.SizeMultiplier + s.startSize).f();
        let (id, over) = (s.id, s.t > s.Duration);
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, [num2, num2, 0.0]));
        if over {
            self.destroy_object(id);
        }
    }

    // ---- GlassShardBehaviour -------------------------------------------------------------------------------------

    /// GlassShardBehaviour.FixedUpdate: Random.value draws every step; at or below RandomThreshold and faster than
    /// SpeedThreshold, every collider (at most 4) at the shard's position that belongs to a limb in the world is Cut along
    /// the normalised velocity.
    pub fn glass_shard_fixed_update(&mut self, i: usize) {
        let s = self.glass_shards[i].clone();
        if !(self.random.value() > s.RandomThreshold) {
            return;
        }
        let rb = self.physicals[s.Rigidbody].rigidbody.clone();
        let velocity = rb.velocity(&self.world);
        if !(mathf::sqr_magnitude(velocity) > (d(s.SpeedThreshold) * s.SpeedThreshold).f()) {
            return;
        }
        let position = self.with_component_instance(s.id, |_, inst, o| inst.position(o));
        let mut hits = self.overlap_point_colliders(position, super::explosion::DEFAULT_RAYCAST_LAYERS, f32::NEG_INFINITY, f32::INFINITY);
        hits.truncate(4);
        for (_, collider) in hits {
            let Some(p) = self.physical_of_collider(collider).filter(|&p| self.physicals[p].in_world) else { continue };
            if let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == p) {
                self.cut(l, [position[0], position[1]], mathf::normalized2(velocity));
            }
        }
    }
}

script! {
    Raygun {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// The PhysicalBehaviour on its object (base.transform).
        physical: usize = 0,
        /// layersTohit (LayerMask.m_Bits).
        layersTohit: u32 = 0,
        /// targetRoot: the Transform id of the target's root.
        targetRoot: Option<i32> = None,
        targetPosition: [f32; 3] = [0.0; 3],
        t: f32 = 0.0,
        activated: bool = false,
        /// beam: its index in `Game::lines`.
        beam: usize = 0,
        /// beamCurve and its evaluation cache.
        beamCurve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
    }
}

impl Game {
    // RaygunBehaviour.OnDrawGizmos: presentation (Gizmos.DrawRay draws in the editor only)
    /// RaygunBehaviour.StopBlast: beam.enabled = false, then audioSource.Stop.
    pub(crate) fn raygun_stop_blast(&mut self, i: usize) {
        let beam = self.raygun_beams[i].beam;
        self.lines[beam].enabled = false;
        let src = self.audio_src(self.raygun_beams[i].id, "audioSource");
        self.audio.stop(src);
    }

    /// RaygunBehaviour.CalculateCurve(GetBarrelPosition(), vector, 16, ref vertices), then positionCount = 17 and SetPositions.
    pub(crate) fn raygun_calculate_curve(&mut self, i: usize, b: [f32; 2]) {
        // Mono rounds every Vector2 and Vector3 operator result and local to float.
        let (a, dir) = (self.raygun_barrel_position(i), self.raygun_barrel_direction(i));
        let (dx, dy) = ((d(a[0]) - b[0]).f(), (d(a[1]) - b[1]).f());
        let num = (((d(dx) * dx + d(dy) * dy).0).sqrt()) as f32;
        let end = [(d(dir[0]) * num).f(), (d(dir[1]) * num).f()];
        let end = [(d(end[0]) + a[0]).f(), (d(end[1]) + a[1]).f()];
        let mut vertices = [[0.0f32; 3]; 17];
        for k in 0..16 {
            let time = k as f32 / 16.0;
            let s = &mut self.raygun_beams[i];
            let w = s.beamCurve.as_ref().expect("RaygunBehaviour.beamCurve").evaluate(&mut s.curve_cache, time);
            // Vector3.Lerp(pointA, GetBarrelDirection() * num + GetBarrelPosition(), time)
            let p = [mathf::lerp(a[0], end[0], time), mathf::lerp(a[1], end[1], time), 0.0];
            // Vector3.Lerp(a, pointB, curve(time)), then Vector3.Lerp(pointA, that, time)
            let q = [mathf::lerp(p[0], b[0], w), mathf::lerp(p[1], b[1], w), mathf::lerp(p[2], 0.0, w)];
            vertices[k] = [mathf::lerp(a[0], q[0], time), mathf::lerp(a[1], q[1], time), mathf::lerp(0.0, q[2], time)];
        }
        vertices[0] = [a[0], a[1], 0.0];
        vertices[16] = [b[0], b[1], 0.0];
        let beam = self.raygun_beams[i].beam;
        self.line_set_position_count(beam, 17);
        self.line_set_positions(beam, &vertices);
    }

    /// RaygunBehaviour.GetBarrelPosition: transform.TransformPoint(barrelPosition).
    pub(crate) fn raygun_barrel_position(&self, i: usize) -> [f32; 2] {
        self.transform_point(self.raygun_beams[i].physical, self.raygun_beams[i].barrelPosition)
    }

    /// RaygunBehaviour.GetBarrelDirection: transform.TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn raygun_barrel_direction(&self, i: usize) -> [f32; 2] {
        let s = &self.raygun_beams[i];
        let v = self.transform_direction(s.physical, [s.barrelDirection[0], s.barrelDirection[1], 0.0]);
        let k = self.physicals[s.physical].transform.localScale[0];
        [(d(v[0]) * k).f(), (d(v[1]) * k).f()]
    }

    /// RaygunBehaviour.Use, InitiateBlast (UpdateLineRenderer, beam.enabled = true, audioSource.Play) and StopBlast.
    pub fn raygun_use(&mut self, i: usize) {
        let on = !self.raygun_beams[i].activated;
        self.raygun_beams[i].activated = on;
        let src = self.audio_src(self.raygun_beams[i].id, "audioSource");
        if on {
            self.raygun_beams[i].t = 0.0;
            self.raygun_update_line_renderer(i);
            let beam = self.raygun_beams[i].beam;
            self.lines[beam].enabled = true;
            self.audio.play(src);
        } else {
            self.raygun_stop_blast(i);
        }
    }

    /// RaygunBehaviour.FixedUpdate. The BroadcastMessage("Raygunned") has no receiver in the game.
    pub fn raygun_fixed_update(&mut self, i: usize) {
        if !self.raygun_beams[i].activated {
            return;
        }
        if self.raygun_beams[i].targetRoot.is_none() {
            self.raygun_find_target(i);
        } else {
            let at = self.physicals[self.raygun_beams[i].physical].transform.position;
            let target = self.raygun_beams[i].targetPosition;
            let to = normalized3([(d(target[0]) - at[0]).f(), (d(target[1]) - at[1]).f(), (d(target[2]) - at[2]).f()]);
            let dir = self.raygun_barrel_direction(i);
            // Vector2.Dot of the barrel direction and the (Vector3 converted) normalised offset.
            let dot = (d(dir[0]) * to[0] + d(dir[1]) * to[1]).f();
            if (dot as f64) < 0.7 {
                self.raygun_beams[i].targetRoot = None;
            }
            self.raygun_burn(i);
        }
        self.raygun_update_line_renderer(i);
    }

    /// RaygunBehaviour.Burn
    pub(crate) fn raygun_burn(&mut self, i: usize) {
        let Some(root) = self.raygun_beams[i].targetRoot else { return };
        let t = (d(self.raygun_beams[i].t) + self.time.deltaTime).f();
        self.raygun_beams[i].t = t;
        if !(t > 1.0) {
            return;
        }
        let at = self.raygun_beams[i].targetPosition;
        self.spawn_resource("Prefabs/RaygunExplosion", at);
        let physicals = self.raygun_root_physicals(root);
        for &p in &physicals {
            let position = self.raygun_body_position(p);
            if (self.random.value() as f64) > 0.8 {
                self.spawn_resource("Prefabs/RaygunExplosion", [position[0], position[1], 0.0]);
            }
        }
        for &p in &physicals {
            self.physicals[p].BurnProgress = 1.0;
        }
        for x in 0..self.destroyables.len() {
            if physicals.contains(&self.destroyables[x].PhysicalBehaviour) {
                self.destroyable_break(x, [0.0, 0.0]);
            }
        }
        let s = &mut self.raygun_beams[i];
        (s.targetRoot, s.t, s.activated) = (None, 0.0, false);
        self.raygun_stop_blast(i);
    }

    /// RaygunBehaviour.FindTarget
    pub(crate) fn raygun_find_target(&mut self, i: usize) {
        self.raygun_beams[i].t = 0.0;
        let (position, direction, mask) = (self.raygun_barrel_position(i), self.raygun_barrel_direction(i), self.raygun_beams[i].layersTohit);
        if let Some(hit) = self.raycast(V::new(position[0], position[1]), direction, 250.0, mask, f32::NEG_INFINITY, f32::INFINITY) {
            if let Some(p) = self.hit_transform_physical(&hit) {
                self.raygun_beams[i].targetRoot = Some(self.root_id(p));
                self.raygun_update_line_renderer(i);
            }
        }
    }

    /// RaygunBehaviour.UpdateLineRenderer: targetPosition, then CalculateCurve.
    pub(crate) fn raygun_update_line_renderer(&mut self, i: usize) {
        let vector = match self.raygun_beams[i].targetRoot {
            Some(root) => {
                let physicals = self.raygun_root_physicals(root);
                let mut sum = [0.0f32; 2];
                for &p in &physicals {
                    let q = self.raygun_body_position(p);
                    sum = [(d(sum[0]) + q[0]).f(), (d(sum[1]) + q[1]).f()];
                }
                let n = physicals.len() as f32;
                [(d(sum[0]) / n).f(), (d(sum[1]) / n).f()]
            }
            None => {
                let (position, direction, mask) = (self.raygun_barrel_position(i), self.raygun_barrel_direction(i), self.raygun_beams[i].layersTohit);
                match self.raycast(V::new(position[0], position[1]), direction, 250.0, mask, f32::NEG_INFINITY, f32::INFINITY) {
                    Some(hit) => [hit.point.x, hit.point.y],
                    None => [(d(direction[0]) * 250.0f32 + position[0]).f(), (d(direction[1]) * 250.0f32 + position[1]).f()],
                }
            }
        };
        self.raygun_beams[i].targetPosition = [vector[0], vector[1], 0.0];
        self.raygun_calculate_curve(i, vector);
    }

    /// Rigidbody2D.position of physical `p`.
    fn raygun_body_position(&mut self, p: usize) -> [f32; 2] {
        self.sync_pending();
        let b = self.world.body(self.physicals[p].rigidbody.body).xf.p;
        [b.x, b.y]
    }

    /// targetRoot.GetComponentsInChildren<Rigidbody2D / PhysicalBehaviour / DestroyableBehaviour>(): the physical objects
    /// whose root is `root`, in arena (prefab hierarchy) order.
    fn raygun_root_physicals(&self, root: i32) -> Vec<usize> {
        let found: Vec<usize> = (0..self.physicals.len()).filter(|&p| !self.physicals[p].destroyed && self.root_id(p) == root).collect();
        for n in &self.node_rigidbodies {
            let mut x = n.node;
            while let Some(up) = self.transform_nodes[x].parent {
                x = up;
            }
            if self.transform_nodes[x].id == root {
                unimplemented!("mod-only: RaygunBehaviour: a Rigidbody2D on a transform node under the target root");
            }
        }
        found
    }
}

script! {
    WhOpening {
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        GrowingCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        Wh: Option<ObjRef> = None,
        /// duration (Awake): AudioSource.clip.length.
        duration: f32 = 0.0,
        /// AudioSource.clip.length, read when the component is linked (what Awake assigns to duration).
        clip_length: f32 = 0.0,
        t: f32 = 0.0,
    }
}

impl Game {
    /// WhOpening.Awake: duration = AudioSource.clip.length (the clip is read when the component is linked).
    pub fn wh_opening_awake(&mut self, i: usize) {
        let s = &mut self.wh_openings[i];
        s.duration = s.clip_length;
    }

    /// WhOpening.Start: empty.
    pub fn wh_opening_start(&mut self, _i: usize) {}

    /// WhOpening.Update: t grows by deltaTime over the clip's length, the scale is Vector3.one * GrowingCurve.Evaluate(t),
    /// and End runs once t reaches 1.
    pub fn wh_opening_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let s = &mut self.wh_openings[i];
        s.t = (d(s.t) + d(dt) / s.duration).f();
        let curve = s.GrowingCurve.clone().expect("WhOpening.GrowingCurve");
        let scale = curve.evaluate(&mut s.cache, s.t);
        let (id, over) = (s.id, s.t >= 1.0);
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, [scale, scale, scale]));
        if over {
            self.wh_opening_end(i);
        }
    }

    /// WhOpening.FixedUpdate: CameraShakeBehaviour.main.Shake(t * 0.5f, position, 0.7f).
    pub fn wh_opening_fixed_update(&mut self, i: usize) {
        let (id, t) = (self.wh_openings[i].id, self.wh_openings[i].t);
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.camera_shake_at((d(t) * 0.5f32).f(), [position[0], position[1]], 0.7);
    }

    /// WhOpening.End: a pulse of 650 over 35 breaking objects and without effects, the 32 ray explosion (35, 25, effects,
    /// big) with the water test, the object deactivated and Wh instantiated at it.
    pub(crate) fn wh_opening_end(&mut self, i: usize) {
        let (id, wh) = (self.wh_openings[i].id, self.wh_openings[i].Wh);
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.create_pulse_explosion_with(position, 650.0, 35.0, false, true);
        let under_water = self.is_point_under_water([position[0], position[1]]);
        self.create_explosion_with_water(under_water, ExplosionParameters::new_big(32, position, 35.0, 25.0, true, true, 0.0, 0));
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        let wh = wh.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        self.spawn_prefab(wh, position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shockwave_defaults() {
        let x = Shockwave::default();
        assert_eq!((x.Duration, x.SizeMultiplier), (0.02, 5.0));
    }

    #[test]
    fn scale3_is_per_component() {
        assert_eq!(scale3([1.0, -2.0, 3.0], 0.5), [0.5, -1.0, 1.5]);
    }

    #[test]
    fn plane_use_flips_activated() {
        let mut g = Game::default();
        g.planes.push(Plane::default());
        g.plane_use(0);
        assert!(g.planes[0].activated);
        g.plane_use(0);
        assert!(!g.planes[0].activated);
    }

    #[test]
    fn mask_is_objects_and_bounds() {
        let mut g = Game::default();
        g.machine_gun_projectiles.push(MachineGunProjectile::default());
        g.machine_gun_projectile_awake(0);
        assert_eq!(g.machine_gun_projectiles[0].mask, (1 << 10) | (1 << 11));
    }
}
