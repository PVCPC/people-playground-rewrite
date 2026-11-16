//! GorseDroolingBehaviour (the Gorse's spitting head) and GorseProjectileBehaviour (its Gorseball).
#![allow(non_snake_case)]

use super::{d, BareAnchor, Game};
use crate::assets::db::ObjRef;
use crate::b2::contacts2d::{Message, ScriptCollision};
use crate::b2::math::Vec2 as V;
use crate::unity::transform::Trs;

#[derive(Clone, Debug)]
pub struct GorseDrooling {
    /// The component's instance id.
    pub id: i32,
    /// LimbBehaviour
    pub limb: usize,
    /// Where its (bare) Transform hangs: base.transform.position.
    pub anchor: BareAnchor,
    pub chain: Vec<Trs>,
    pub CanSee: u32,
    /// Projectile (None: null).
    pub Projectile: Option<ObjRef>,
    /// FireClip.Length: PickRandom draws over it.
    pub FireClips: usize,
    pub Range: f32,
    pub Force: f32,
    pub step: i32,
    pub isAlive: bool,
    /// `emission.enabled = isAlive` has run (the prefab's own setting holds before).
    pub emission_set: bool,
}

/// GorseEyeBehaviour: an eye that jumps around inside its Container while its Limb is conscious and alive.
#[derive(Clone, Debug)]
pub struct GorseEye {
    /// The component's instance id.
    pub id: i32,
    /// Limb (None: the Update throws a NullReferenceException).
    pub limb: Option<usize>,
    /// Container.position (x, y) and Container.size (width, height).
    pub container_position: [f32; 2],
    pub container_size: [f32; 2],
    pub SmallRandomRadius: f32,
    pub LargeRange: [f32; 2],
    pub SmallRange: [f32; 2],
    pub largeInterval: f32,
    pub smallInterval: f32,
    pub largeTimer: f32,
    pub smallTimer: f32,
    /// isRendering: what Renderer.enabled was last set to.
    pub isRendering: bool,
}

impl Default for GorseEye {
    fn default() -> Self {
        Self {
            id: Default::default(),
            limb: Default::default(),
            container_position: Default::default(),
            container_size: Default::default(),
            SmallRandomRadius: 0.02,
            LargeRange: [0.7, 1.0],
            SmallRange: [0.7, 1.0],
            largeInterval: Default::default(),
            smallInterval: Default::default(),
            largeTimer: Default::default(),
            smallTimer: Default::default(),
            isRendering: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GorseProjectile {
    /// The component's instance id.
    pub id: i32,
    pub Damage: f32,
    pub DisperseEffect: Option<ObjRef>,
    pub AcidChance: f32,
    /// The colliders on its own GameObject (the OnCollisionEnter2D receivers).
    pub colliders: Vec<i32>,
    /// Its own Rigidbody2D's node body (transform.position).
    pub node_body: usize,
}

impl Default for GorseProjectile {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Damage: Default::default(),
            DisperseEffect: Default::default(),
            AcidChance: 0.2,
            colliders: Default::default(),
            node_body: Default::default(),
        }
    }
}

impl Game {
    /// GorseEyeBehaviour.Awake: both intervals are drawn from their ranges.
    pub fn gorse_eye_awake(&mut self, e: usize) {
        let (large, small) = (self.gorse_eyes[e].LargeRange, self.gorse_eyes[e].SmallRange);
        self.gorse_eyes[e].largeInterval = self.random_between(large);
        self.gorse_eyes[e].smallInterval = self.random_between(small);
    }

    /// GorseEyeBehaviour.Update: unconscious or dead, the renderer goes off and nothing else happens. Otherwise the timers
    /// run (frozen stops them); the large timer moves the eye's localPosition to a random point of the Container (z becomes
    /// 0, a Vector2 converted to a Vector3), and the small one nudges it by a random offset in a circle.
    pub fn gorse_eye_update(&mut self, e: usize) {
        // A null Limb is a NullReferenceException on the first line.
        let Some(l) = self.gorse_eyes[e].limb else { return };
        if self.people[self.limbs[l].Person].Consciousness < 0.5 || !self.limb_is_considered_alive(l) {
            // Renderer.enabled = false is rendering.
            self.gorse_eyes[e].isRendering = false;
            return;
        }
        self.gorse_eyes[e].isRendering = true;
        if !self.limbs[l].Frozen {
            let dt = self.time.deltaTime;
            self.gorse_eyes[e].largeTimer += dt;
            self.gorse_eyes[e].smallTimer += dt;
        }
        let id = self.gorse_eyes[e].id;
        if self.gorse_eyes[e].largeTimer > self.gorse_eyes[e].largeInterval {
            let (pos, size) = (self.gorse_eyes[e].container_position, self.gorse_eyes[e].container_size);
            let x = self.random.value() * size[0];
            let y = self.random.value() * size[1];
            self.with_component_instance(id, |_, inst, o| inst.set_local_position(o, [pos[0] + x, pos[1] + y, 0.0]));
            let range = self.gorse_eyes[e].LargeRange;
            self.gorse_eyes[e].largeInterval = self.random_between(range);
            self.gorse_eyes[e].largeTimer = 0.0;
        }
        if self.gorse_eyes[e].smallTimer > self.gorse_eyes[e].smallInterval && (d(self.gorse_eyes[e].largeTimer).0 > (d(self.gorse_eyes[e].largeInterval) / 6.0f32).0) {
            let circle = self.random.inside_unit_circle();
            let r = self.gorse_eyes[e].SmallRandomRadius;
            self.with_component_instance(id, |_, inst, o| {
                let t = inst.objects[o].trs.t;
                inst.set_local_position(o, [t[0] + circle[0] * r, t[1] + circle[1] * r, t[2] + 0.0 * r]);
            });
            let range = self.gorse_eyes[e].SmallRange;
            self.gorse_eyes[e].smallInterval = self.random_between(range);
            self.gorse_eyes[e].smallTimer = 0.0;
        }
    }

    fn drooling_position(&self, g: usize) -> [f32; 3] {
        let s = &self.gorse_droolings[g];
        self.anchored_world(s.anchor, &s.chain).0
    }

    /// GorseDroolingBehaviour.FixedUpdate (Start only caches the emission module).
    pub fn gorse_drooling_fixed_update(&mut self, g: usize) {
        // GorseDroolingBehaviour.Start: emission = ParticleSystem.emission, the module handle EmbeddedPs's Gorse drive writes (particles.rs)
        self.gorse_droolings[g].step += 1;
        if self.gorse_droolings[g].step > 30 {
            let l = self.gorse_droolings[g].limb;
            let alive = self.person_is_alive(self.limbs[l].Person);
            // emission.enabled = isAlive (particles.rs reads isAlive and emission_set).
            self.gorse_droolings[g].isAlive = alive;
            self.gorse_droolings[g].emission_set = true;
            if alive && self.limb_is_capable(l) {
                self.gorse_drooling_attack_nearby(g);
            }
        }
    }

    /// The person whose AliveBehaviour.AliveByTransform entry is this collider's transform.root.
    pub(super) fn alive_of_collider(&self, collider: i32, physical: Option<usize>) -> Option<usize> {
        if let Some(p) = physical {
            return self.person_of_root(p);
        }
        match self.bare_colliders.get(&collider).map(|b| b.anchor) {
            Some(BareAnchor::Physical(p)) => self.person_of_root(p),
            Some(BareAnchor::Node(n)) => self.person_of_root_key(self.node_root_key(n)),
            _ => None,
        }
    }

    /// GorseDroolingBehaviour.AttackNearby
    pub(crate) fn gorse_drooling_attack_nearby(&mut self, g: usize) {
        self.gorse_droolings[g].step = 0;
        let position = self.drooling_position(g);
        let s = self.gorse_droolings[g].clone();
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), s.Range, s.CanSee);
        // OverlapCircleNonAlloc into the static 32-entry buffer.
        hits.truncate(GORSE_DROOLING_BUFFER);
        if hits.is_empty() {
            return;
        }
        let (physical, collider) = hits[self.random.range_int(0, hits.len() as i32) as usize];
        let Some(value) = self.alive_of_collider(collider, physical) else { return };
        let own = self.limbs[s.limb].PhysicalBehaviour;
        let own_root = self.root_id(own);
        let value_limb = self.people[value].Limbs[0];
        let value_phys = self.limbs[value_limb].PhysicalBehaviour;
        let value_root = self.root_id(value_phys);
        if self.object_name(value_phys, value_root) == self.object_name(own, own_root) || !self.person_is_alive(value) {
            return;
        }
        let at = self.collider_position(collider);
        let vector = [(d(position[0]) - at[0]).f(), (d(position[1]) - at[1]).f(), (d(position[2]) - at[2]).f()];
        let hit = self.material_aware_raycast([at[0], at[1]], [vector[0], vector[1]], s.Range, s.CanSee, 8, 0, 0.0);
        if hit.physical.is_some_and(|p| self.root_of(p) == self.root_of(own)) {
            let person = self.limbs[s.limb].Person;
            self.person_wince(person, 15.0);
            let n = super::person::normalized(vector);
            let magnitude = (d(vector[0]) * vector[0] + d(vector[1]) * vector[1] + d(vector[2]) * vector[2]).0.sqrt() as f32;
            self.gorse_drooling_fire(g, [-n[0], -n[1], -n[2]], (d(magnitude) * s.Force).f());
        }
    }

    /// GorseDroolingBehaviour.Fire
    pub(crate) fn gorse_drooling_fire(&mut self, g: usize, direction: [f32; 3], force: f32) {
        // LimbBehaviour.PhysicalBehaviour.PlayClipOnce(FireClip.PickRandom())
        let (id, n, limb) = (self.gorse_droolings[g].id, self.gorse_droolings[g].FireClips, self.gorse_droolings[g].limb);
        let clip = self.pick_clip(id, "FireClip", n);
        self.play_clip_once(self.limbs[limb].PhysicalBehaviour, clip, 1.0);
        let position = self.drooling_position(g);
        let source = self.gorse_droolings[g].Projectile.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let mut db = self.assets.take().expect("GorseDroolingBehaviour without the game's data");
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(&mut db, &prefab, position, [0.0, 0.0, 0.0, 1.0], None));
        self.assets = Some(db);
        let (inst, linked) = spawned.unwrap_or_else(|e| panic!("GorseDroolingBehaviour: Instantiate: {e}"));
        // gameObject.GetComponent<Rigidbody2D>().AddForce(direction * force, Impulse)
        let k = *linked.node_rigidbodies.get(&0).unwrap_or_else(|| unimplemented!("mod-only: GorseDroolingBehaviour.Projectile without a bare Rigidbody2D on its root"));
        let rb = self.node_rigidbodies[k].rigidbody.clone();
        rb.add_force_impulse(&mut self.world, [(d(direction[0]) * force).f(), (d(direction[1]) * force).f()]);
        let own = super::link::colliders_in_children(&inst, 0).into_iter().find(|&(_, o)| o == 0).map(|(c, _)| c);
        self.unsynced.push(inst);
        let id = self.gorse_droolings[g].id;
        let root = self.with_component_instance(id, |_, inst, _| super::link::colliders_in_children(inst, 0));
        for (b, _) in root {
            // IgnoreCollisionSubstituteMethod(GetComponent<Collider2D>() == null, b): `(bool)a && (bool)b` is false, nothing happens.
            if let Some(own) = own {
                self.ignore_collision_substitute(own, b, true);
            }
        }
    }

    /// OnCollisionEnter2D to a GorseProjectileBehaviour on the collider's GameObject.
    pub fn gorse_projectile_collision(&mut self, collider: i32, message: Message, c: &ScriptCollision) {
        if message != Message::CollisionEnter {
            return;
        }
        if let Some(x) = self.gorse_projectiles.iter().position(|x| x.colliders.contains(&collider) && !self.destroyed_components.contains(&x.id)) {
            self.gorse_projectile_on_collision_enter(x, c);
        }
    }

    /// GorseProjectileBehaviour.OnCollisionEnter2D
    pub(crate) fn gorse_projectile_on_collision_enter(&mut self, x: usize, c: &ScriptCollision) {
        let s = self.gorse_projectiles[x].clone();
        // collision.transform.TryGetComponent<LimbBehaviour>()
        let limb = self.collision_physical(c).and_then(|p| self.limbs.iter().position(|l| l.PhysicalBehaviour == p));
        if let Some(l) = limb {
            self.limb_wince(l, s.Damage);
            self.limb_damage(l, s.Damage);
            if !self.limbs[l].IsAndroid && s.AcidChance > self.random.value() {
                self.add_liquid(super::container::Owner::Circulation(l), super::circulation::liquid(crate::il!(str "GorseProjectileBehaviour::OnCollisionEnter2D" 0)), 25.0);
                if self.nodes[l].IsConnectedToRoot {
                    let person = self.limbs[l].Person;
                    self.add_pain(person, 50.0);
                }
            }
        }
        let node = self.node_rigidbodies[s.node_body].node;
        let position = self.node_world(node).0;
        let effect = s.DisperseEffect.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        self.spawn_prefab(effect, position);
        self.destroy_object(s.id);
    }
}

/// GorseDroolingBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const GORSE_DROOLING_BUFFER: usize = 32;
