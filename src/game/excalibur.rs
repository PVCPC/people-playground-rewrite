//! ExcaliburBehaviour: a Use starts a spell charge (performingSpell, chargeTime = 0; the particles and audio are
//! presentation). Update advances the charge at 2 per second and casts at 1, which instantiates the fireball
//! (CastSpell: not rewritten, see excalibur_cast). Lodged: the Excalibur stabbed into a victim breaks it, or for an
//! android limb sets its acid to at least 0.6 and damages it to death. The sounds are presentation.
#![allow(non_snake_case)]

use super::{d, mathf, script, BareAnchor, BodyOwner, Game};
use crate::b2::contacts2d::{Message, ScriptCollision};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::unity::transform::Trs;
use crate::game_assets::{Curve, CurveCache};

/// Unity's "World" tag (TagManager: index 11 after the builtin 20000 base; poolable 20008 and nitro 20010 agree).
pub(super) const WORLD_TAG: u16 = 20011;

script! {
    Excalibur {
        SpellForce: f32 = 0.0,
    }
    links {
        /// FireBallPrefab (None: null).
        FireBallPrefab: Option<ObjRef> = None,
        /// The Excalibur's PhysicalBehaviour (its transform is base.transform).
        physical: usize = 0,
        /// SpawnPoint's Transform chain (object first) up to the physical's object.
        spawn_chain: Vec<Trs> = Vec::new(),
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// ChargeAudioSource: its instance id (Use plays it).
        ChargeAudioSource: i32 = 0,
        /// The private performingSpell and chargeTime.
        performingSpell: bool = false,
        chargeTime: f32 = 0.0,
        /// SpellChargeCurve and its evaluation cache (Update scales SpellChargeGlow by it).
        SpellChargeCurve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
    }
}

// ExcaliburBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>(), the linked `physical`
impl Game {
    /// ExcaliburBehaviour.Use: starts a charge unless one is running.
    pub fn excalibur_use(&mut self, e: usize) {
        if !self.excaliburs[e].performingSpell {
            self.excaliburs[e].performingSpell = true;
            self.excaliburs[e].chargeTime = 0.0;
            let id = self.excaliburs[e].id;
            self.ps_field_emit_n(id, "SpellChargeParticleSystem", 12);
            self.audio.play(self.excaliburs[e].ChargeAudioSource);
        }
    }

    /// ExcaliburBehaviour.Update: while performing, the charge advances by dt * 2, the glow scales by the curve, and it casts at 1.
    pub fn excalibur_update(&mut self, e: usize) {
        if !self.excaliburs[e].performingSpell {
            return;
        }
        self.excaliburs[e].chargeTime += self.time.deltaTime * 2.0;
        // SpellChargeGlow.localScale = 2f * SpellChargeCurve.Evaluate(chargeTime) * Vector2.one (a Vector2: z 0).
        let x = &mut self.excaliburs[e];
        let k = 2.0 * x.SpellChargeCurve.as_ref().expect("ExcaliburBehaviour.SpellChargeCurve").evaluate(&mut x.curve_cache, x.chargeTime);
        let id = x.id;
        if let Some(t) = self.field_component(id, "SpellChargeGlow") {
            self.with_component_instance(t, |_, inst, o| inst.set_local_scale(o, [k, k, 0.0]));
        }
        if self.excaliburs[e].chargeTime >= 1.0 {
            self.excalibur_cast(e);
        }
    }

    /// ExcaliburBehaviour.CastSpell: Instantiate(FireBallPrefab) at SpawnPoint with an impulse up the blade, and
    /// IgnoreCollision with every collider of the Excalibur, then the glow's scale goes to zero.
    pub(crate) fn excalibur_cast(&mut self, e: usize) {
        let s = self.excaliburs[e].clone();
        let position = self.anchored_world(BareAnchor::Physical(s.physical), &s.spawn_chain).0;
        let source = s.FireBallPrefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let mut db = self.assets.take().expect("ExcaliburBehaviour without the game's data");
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(&mut db, &prefab, position, [0.0, 0.0, 0.0, 1.0], None));
        self.assets = Some(db);
        let (inst, linked) = spawned.unwrap_or_else(|e| panic!("ExcaliburBehaviour: Instantiate: {e}"));
        // GetComponent<Rigidbody2D>().AddForce(transform.up * SpellForce, Impulse)
        let k = *linked.node_rigidbodies.get(&0).unwrap_or_else(|| unimplemented!("mod-only: ExcaliburBehaviour.FireBallPrefab without a bare Rigidbody2D on its root"));
        let up = self.physicals[s.physical].transform.up();
        let rb = self.node_rigidbodies[k].rigidbody.clone();
        rb.add_force_impulse(&mut self.world, [up[0] * s.SpellForce, up[1] * s.SpellForce]);
        let own = super::link::colliders_in_children(&inst, 0).into_iter().find(|&(_, o)| o == 0).map(|(c, _)| c);
        self.unsynced.push(inst);
        let id = self.excaliburs[e].id;
        let theirs = self.with_component_instance(id, |_, inst, o| super::link::colliders_in_children(inst, o));
        for (b, _) in theirs {
            // IgnoreCollisionSubstituteMethod(GetComponent<Collider2D>() == null, b): `(bool)a && (bool)b` is false, nothing happens.
            if let Some(own) = own {
                self.ignore_collision_substitute(own, b, true);
            }
        }
        self.excaliburs[e].performingSpell = false;
        // SpellChargeGlow.localScale = Vector2.zero (z 0 too).
        if let Some(t) = self.field_component(id, "SpellChargeGlow") {
            self.with_component_instance(t, |_, inst, o| inst.set_local_scale(o, [0.0; 3]));
        }
    }

    /// ExcaliburBehaviour.Lodged(stabbing) with the victim: `if (victim && !victim.transform.root.CompareTag("World"))`,
    /// then a DestroyableBehaviour breaks, a DamagableMachinery force-breaks, and an android's limb burns (acid 0.6 at
    /// least) and takes Health + 1 damage; anything else gets SendMessage("Break", Vector2.zero).
    pub(crate) fn excalibur_lodged(&mut self, victim: usize) {
        let root = self.root_id(victim);
        // A root on no clone is a level1 (Main.unity) scene object, as the map's WORLD: its tag is the scene's.
        let on_clone = self.unsynced.iter().chain(&self.retained).any(|i| i.objects.iter().any(|x| x.components.contains(&root)));
        let root_tag = if on_clone {
            self.with_component_instance(root, |_, inst, o| inst.prefab.objects[o].tag)
        } else {
            let db = self.assets.as_ref().expect("ExcaliburBehaviour without the game's data");
            super::spawn::level1_transform_tag(db, root).unwrap_or_else(|| panic!("ExcaliburBehaviour.Lodged: root {root} is on no clone and not a level1 object"))
        };
        if root_tag == WORLD_TAG {
            return;
        }
        if let Some(x) = self.destroyables.iter().position(|d| d.PhysicalBehaviour == victim) {
            self.destroyable_break(x, [0.0, 0.0]);
        }
        if let Some(m) = self.machinery.iter().position(|m| m.PhysicalBehaviour == victim) {
            self.machinery_force_break(m);
        }
        match self.limbs.iter().position(|l| l.PhysicalBehaviour == victim) {
            Some(l) if self.limbs[l].IsAndroid => {
                let acid = mathf::max(self.skins[l].acidProgress, 0.6);
                self.skins[l].set_acid(acid);
                let damage = self.limbs[l].Health + 1.0;
                self.limb_damage(l, damage);
            }
            _ => {
                // SendMessage("Break", Vector2.zero) to the victim's GameObject (its collider only matters for a bare one).
                let collider = self.physicals[victim].all_colliders.first().copied().unwrap_or(0);
                self.send_break(collider, Some(victim), [0.0, 0.0]);
            }
        }
    }
}

/// FireballBehaviour (the Excalibur spell): its own colliders, body, Range, ExplosionEffect and DAT.
#[derive(Clone, Debug)]
pub struct Fireball {
    /// The component instance id.
    pub id: i32,
    /// ExplosionEffect (None: null).
    pub ExplosionEffect: Option<ObjRef>,
    pub Range: f32,
    /// The colliders on its own GameObject (the OnCollisionEnter2D receivers).
    pub colliders: Vec<i32>,
    /// Its own Rigidbody2D node body (transform.position, isKinematic, constraints).
    pub node_body: usize,
    /// DAT component instance id (DeleteAfterTime, disabled in the prefab).
    pub DAT: i32,
    pub hitOnce: bool,
}

impl Game {
    /// OnCollisionEnter2D to a FireballBehaviour on the collider GameObject.
    pub fn fireball_collision(&mut self, collider: i32, message: Message, _c: &ScriptCollision) {
        if message != Message::CollisionEnter {
            return;
        }
        if let Some(x) = self.fireballs.iter().position(|x| x.colliders.contains(&collider) && !self.destroyed_components.contains(&x.id)) {
            self.fireball_on_collision_enter(x);
        }
    }

    /// FireballBehaviour.OnCollisionEnter2D (the particle emission rates are presentation).
    pub(crate) fn fireball_on_collision_enter(&mut self, x: usize) {
        if self.fireballs[x].hitOnce {
            return;
        }
        self.fireballs[x].hitOnce = true;
        let s = self.fireballs[x].clone();
        let node = self.node_rigidbodies[s.node_body].node;
        let position = self.node_world(node).0;
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), s.Range, super::explosion::DEFAULT_RAYCAST_LAYERS);
        // FireballBehaviour.Awake: cols = new Collider2D[24], the OverlapCircleNonAlloc buffer
        // OverlapCircleNonAlloc into the 24-entry buffer.
        hits.truncate(24);
        for (physical, collider) in hits {
            // collider.transform.TryGetComponent<PhysicalBehaviour>
            let Some(p) = physical else { continue };
            let at = self.collider_position(collider);
            let distance = super::shatter::v2_distance([position[0], position[1]], [at[0], at[1]]);
            let num2 = ((d(s.Range) - d(distance)) / s.Range).f();
            if self.properties[self.physicals[p].Properties].Flammability >= 0.05 {
                if self.random.value() > num2 {
                    self.ignite(p, false);
                    let r = self.random.value();
                    self.physicals[p].burnIntensity = (d(num2) * 0.75f32 + d(r) / 5.0f32).f();
                }
                self.physicals[p].BurnProgress = (d(self.physicals[p].BurnProgress) + d(num2) * 0.25f32).f();
            }
            let n = super::person::normalized([at[0] - position[0], at[1] - position[1], at[2] - position[2]]);
            let k = (d(15.0f32) * num2).f();
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force(&mut self.world, [n[0] * k, n[1] * k]);
        }
        let effect = s.ExplosionEffect.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        self.spawn_prefab(effect, position);
        // Rigidbody.isKinematic = true (bodyType Kinematic), then constraints = FreezeAll.
        let component = self.node_rigidbodies[s.node_body].rigidbody.component;
        self.with_component_instance(component, |g, inst, o| g.rigidbody2d_set_body_type(inst, BodyOwner::Node(s.node_body), o, 1));
        let w = &mut self.world;
        self.node_rigidbodies[s.node_body].rigidbody.set_constraints(w, 7);
        // DAT.enabled = true
        self.enable_delete_after(s.DAT);
    }
}
