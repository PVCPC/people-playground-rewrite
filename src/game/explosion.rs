//! ExplosiveBehaviour, ExplosionCreator and DeleteAfterTime, with the Physics2D ray, line and circle queries they use.
#![allow(non_snake_case)]

use super::{d, mathf, script, Behaviour, Game};
use super::constraint::{ConstrainedTarget, ConstraintSource, ParentConstraint};
use super::destroyable::AttachParent;
use crate::assets::db::{Component, ObjRef};
use crate::b2::math::{Transform, Vec2 as V};
use crate::b2::shape::Circle;
use crate::b2::world::{BodyId, Shape};
use crate::b2::contacts2d::ScriptCollision;

/// Physics2D.DefaultRaycastLayers: every layer but Ignore Raycast (2).
pub(super) const DEFAULT_RAYCAST_LAYERS: u32 = !(1 << 2);

script! {
    Explosive {
        Range: f32 = 27.0,
        ShockwaveStrength: f32 = 1.0,
        Delay: f32 = 2.0,
        /// A uint; the serialized values are small.
        FragmentationRayCount: i32 = 32,
        FragmentForce: f32 = 2.0,
        ShockwaveLiftForce: f32 = 0.0,
        ArmOnAwake: bool = false,
        DestroyOnExplode: bool = true,
        DisintegrateOnExplode: bool = false,
        ParticleEffectAndSound: bool = true,
        BurnPower: f32 = 0.0,
        TemperatureLimit: f32 = 500.0,
        BigExplosion: bool = false,
        ArmOnUse: bool = true,
        BallisticShrapnelCount: i32 = 0,
        AffectVictimTemperature: f32 = 0.0,
        ShotExplodeChance: f32 = 1.0,
        ExplodesOnFragmentHit: bool = false,
        ImpactForceThreshold: f32 = -1.0,
        ExplodeBurnProgressThreshold: f32 = 2.0,
        DismemberChance: f32 = 0.0,
        ShouldCreateKillzone: bool = true,
        MinChargeToExplode: f32 = f32::INFINITY,
        CanExplodeMultipleTimes: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GetComponent<PhysicalBehaviour>()
        physical: Option<usize> = None,
        /// Awake's `physicalBehaviour = GetComponent<PhysicalBehaviour>()` (the C# reference cache: `physical` once Awake ran).
        physicalBehaviour: Option<usize> = None,
        /// BlockingLayers (the Awake default when serialized as 0).
        BlockingLayers: u32 = 0,
        /// OnExplode's CarBehaviour and DetachFromParent calls, in order (the rest are audio and particles).
        OnExplodeCalls: Vec<super::link::EventCall> = Vec::new(),
        VFXOverride: Option<ObjRef> = None,
        /// The Killzone clone's instance id.
        Killzone: Option<i32> = None,
        Armed: bool = false,
        hasExploded: bool = false,
        /// The pending Explode coroutines.
        coroutines: Vec<Coroutine> = Vec::new(),
        /// The component comes after the PhysicalBehaviour on its GameObject (SendMessage order).
        after_physical: bool = true,
        /// The component comes before the object's DestroyableBehaviour.
        before_destroyable: bool = false,
        /// The component's index on its GameObject.
        index: usize = 0,
        /// Added at run time (AddComponent) to its physical's GameObject: in none of its clone's component lists.
        added: bool = false,
        Tag: String = String::new(),
    }
}

/// A started Explode: it waits for `wake` (WaitForSeconds, from frame `from` on), then for the end of the frame.
#[derive(Clone, Debug)]
pub struct Coroutine {
    pub wake: f64,
    /// CallDelayed runs a call from the frame after the one that queued it (a WaitForSeconds(0) too).
    pub from: i64,
    pub seq: u64,
    pub end_of_frame: bool,
}

script! {
    DeleteAfter {
        Life: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        time: f32 = 0.0,
        /// Behaviour.enabled (a UseEventTrigger sets it at run time).
        enabled: bool = true,
        /// AddComponent<DeleteAfterTime>() at run time: the Transform id of its GameObject.
        added_to: Option<i32> = None,
        /// An IonBoltBehaviour's Awake listener on OnEndOfLife (`Game::ion_bolts` index).
        ion_bolt: Option<usize> = None,
        /// A runtime child's (SetupSpawnPrefab's SpawnParticles(Clone)) parent Transform: its object's Destroy disables it.
        parent: Option<i32> = None,
    }
}

impl Game {
    /// DeleteAfterTime.get_Progress: time / Life.
    pub fn delete_after_progress(&self, i: usize) -> f32 {
        let x = &self.delete_afters[i];
        (super::d(x.time) / x.Life).f()
    }
}

/// The "nitro" tag (TagManager's eleventh user tag).
const NITRO_TAG: u16 = 20010;

/// ExplosionCreator.EffectSize
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum EffectSize {
    #[default]
    Medium,
    Small,
    Large,
}

/// ExplosionCreator.ExplosionParameters
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExplosionParameters {
    pub FragmentationRayCount: u32,
    pub Position: [f32; 3],
    pub Range: f32,
    pub FragmentForce: f32,
    pub CreateParticlesAndSound: bool,
    pub EffectSize: EffectSize,
    pub DismemberChance: f32,
    pub BallisticShrapnelCount: i32,
}

impl ExplosionParameters {
    /// ExplosionParameters..ctor(rayCount, pos, range, force, createFx, effectSize, dismember, shrapnelCount)
    #[allow(clippy::too_many_arguments)]
    pub fn new(ray_count: u32, pos: [f32; 3], range: f32, force: f32, create_fx: bool, effect_size: EffectSize, dismember: f32, shrapnel_count: i32) -> Self {
        Self { FragmentationRayCount: ray_count, Position: pos, Range: range, FragmentForce: force, CreateParticlesAndSound: create_fx, EffectSize: effect_size, DismemberChance: dismember, BallisticShrapnelCount: shrapnel_count }
    }

    /// ExplosionParameters..ctor(rayCount, pos, range, force, createFx, big, dismember, shrapnelCount): the [Obsolete] overload.
    #[allow(clippy::too_many_arguments)]
    pub fn new_big(ray_count: u32, pos: [f32; 3], range: f32, force: f32, create_fx: bool, big: bool, dismember: f32, shrapnel_count: i32) -> Self {
        Self::new(ray_count, pos, range, force, create_fx, if big { EffectSize::Large } else { EffectSize::Medium }, dismember, shrapnel_count)
    }

    /// ExplosionParameters.get_LargeExplosionParticles
    pub fn large_explosion_particles(&self) -> bool {
        self.EffectSize == EffectSize::Large
    }

    /// ExplosionParameters.set_LargeExplosionParticles
    pub fn set_large_explosion_particles(&mut self, value: bool) {
        self.EffectSize = if value { EffectSize::Large } else { EffectSize::Medium };
    }
}

impl EffectSize {
    /// ExplosionCreator.GetEffectPrefabForSize
    pub fn prefab_path(self) -> &'static str {
        match self {
            EffectSize::Small => "Prefabs/SmallExplosion",
            EffectSize::Large => "Prefabs/BigExplosion",
            EffectSize::Medium => "Prefabs/Explosion",
        }
    }
}

/// UnderwaterExplosionBehaviour on a clone of Prefabs/UnderwaterExplosion: `root` is its Transform id.
#[derive(Clone, Debug)]
pub struct UnderwaterExplosion {
    pub root: i32,
    /// The UnderwaterExplosionBehaviour's instance id.
    pub id: i32,
    pub position: [f32; 3],
    pub GivenTop: f32,
    pub MaxDistanceToSurface: f32,
    pub DoSplash: bool,
    pub scale: f32,
    pub splash: Option<ObjRef>,
    pub near: usize,
    pub distant: i64,
    /// SplashAudio's component id.
    pub splash_audio: i32,
}

impl Default for UnderwaterExplosion {
    fn default() -> Self {
        Self {
            root: Default::default(),
            id: Default::default(),
            position: Default::default(),
            GivenTop: Default::default(),
            MaxDistanceToSurface: Default::default(),
            DoSplash: true,
            scale: Default::default(),
            splash: Default::default(),
            near: Default::default(),
            distant: Default::default(),
            splash_audio: Default::default(),
        }
    }
}

/// A fixture a ray crossed, as Raycast2DQuery::ReportFixture records it.
#[derive(Clone, Copy, Debug)]
pub struct RayHit2D {
    pub collider: i32,
    pub game_object: i32,
    pub body: BodyId,
    pub point: V,
    pub normal: V,
    pub distance: f32,
    pub fraction: f32,
}

/// Vector3.normalized: the magnitude is (float)Math.Sqrt(x * x + y * y + z * z), in double.
pub(super) fn normalized3(v: [f32; 3]) -> [f32; 3] {
    let m = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).0.sqrt() as f32;
    if m > 1e-5 { [v[0] / m, v[1] / m, v[2] / m] } else { [0.0; 3] }
}

impl Game {
    /// Raycast2DQuery::RunQuery (180bf8250) from `p1` to `p2` (QueriesStartInColliders is off): b2World::RayCast with
    /// ReportFixture's filters. Sensors are skipped (QueriesHitTriggers is off), then the layer mask on the collider's
    /// GameObject and the depth range on its Transform's z; a collider is recorded once, at its nearest fraction; the
    /// ray never clips. The hits go by fraction.
    fn ray_query(&mut self, p1: V, p2: V, mask: u32, min_depth: f32, max_depth: f32) -> Vec<RayHit2D> {
        self.sync_pending();
        let (dx, dy) = (p2.x - p1.x, p2.y - p1.y);
        let mut hits: Vec<RayHit2D> = Vec::new();
        if !(dy * dy + dx * dx > 1.4210855e-14_f32) {
            return hits;
        }
        let depths = (min_depth, max_depth) != (f32::NEG_INFINITY, f32::INFINITY);
        let mut z_of: Vec<(i32, f32)> = Vec::new();
        if depths {
            // The colliders' Transform z, read outside the world's callback.
            let mut ids = Vec::new();
            self.world.ray_cast(p1, p2, |(f, _, _, _, _)| {
                ids.push(self.world.fixture(f).collider.id);
                1.0
            });
            for c in ids {
                if !z_of.iter().any(|&(x, _)| x == c) {
                    let z = match self.physical_of_collider(c) {
                        Some(ph) => self.physicals[ph].transform.position[2],
                        None => self.bare_position(c)[2],
                    };
                    z_of.push((c, z));
                }
            }
        }
        self.world.ray_cast(p1, p2, |(f, _, point, normal, fraction)| {
            let fx = self.world.fixture(f);
            if fx.is_sensor {
                return -1.0;
            }
            let Some(&layer) = self.world.layers.get(&fx.collider.game_object) else {
                panic!("Physics2D.Raycast: no layer for collider {} (object {})", fx.collider.id, fx.collider.game_object);
            };
            if mask & (1 << layer) == 0 {
                return -1.0;
            }
            if depths {
                let z = z_of.iter().find(|&&(x, _)| x == fx.collider.id).unwrap().1;
                if z < min_depth || max_depth < z {
                    return -1.0;
                }
            }
            let distance = if fraction > 0.0 {
                let (px, py) = (point.x - p1.x, point.y - p1.y);
                (py * py + px * px).sqrt()
            } else {
                0.0
            };
            let hit = RayHit2D { collider: fx.collider.id, game_object: fx.collider.game_object, body: fx.body, point, normal, distance, fraction };
            match hits.iter_mut().find(|h| h.collider == hit.collider) {
                Some(old) if old.fraction > fraction => *old = hit,
                Some(_) => {}
                None => hits.push(hit),
            }
            1.0
        });
        // Raycast2DQuery's std::sort with RaycastHitsByFractionComparitor (180be5740): fraction < fraction.
        super::sort::msvc_sort(&mut hits, |a, b| a.fraction < b.fraction);
        hits
    }

    /// Physics2D.Raycast(origin, direction, distance): PhysicsQuery2D::Raycast (180bf6290) scales the direction by the
    /// reciprocal of its length and casts to origin + distance * direction; the nearest hit.
    pub fn raycast_first(&mut self, origin: V, direction: [f32; 2], distance: f32) -> Option<RayHit2D> {
        self.raycast(origin, direction, distance, DEFAULT_RAYCAST_LAYERS, f32::NEG_INFINITY, f32::INFINITY)
    }

    /// Physics2D.Raycast(origin, direction, distance, layerMask, minDepth, maxDepth).
    pub(crate) fn raycast(&mut self, origin: V, direction: [f32; 2], distance: f32, mask: u32, min_depth: f32, max_depth: f32) -> Option<RayHit2D> {
        let infinite = distance == f32::INFINITY;
        self.ray_hits(origin, direction, distance, mask, min_depth, max_depth).first().copied().map(|mut h| {
            if infinite {
                h.fraction *= 100000.0;
            }
            h
        })
    }

    /// Physics2D.RaycastNonAlloc(origin, direction, buffer[n], distance, layerMask): the hits, nearest first.
    pub(super) fn raycast_all(&mut self, origin: V, direction: [f32; 2], distance: f32, mask: u32, buffer: usize) -> Vec<RayHit2D> {
        let infinite = distance == f32::INFINITY;
        let mut hits = self.ray_hits(origin, direction, distance, mask, f32::NEG_INFINITY, f32::INFINITY);
        hits.truncate(buffer);
        // PhysicsQuery2D::Raycast (180bf6290) scales every copied hit's fraction by the 100000 it cast.
        if infinite {
            hits.iter_mut().for_each(|h| h.fraction *= 100000.0);
        }
        hits
    }

    fn ray_hits(&mut self, origin: V, direction: [f32; 2], distance: f32, mask: u32, min_depth: f32, max_depth: f32) -> Vec<RayHit2D> {
        // PhysicsQuery2D::Raycast (180bf6290): an infinite distance casts 100000 and scales each hit's fraction by it.
        let distance = if distance == f32::INFINITY { 100000.0 } else { distance };
        let len2 = direction[0] * direction[0] + direction[1] * direction[1];
        let dir = if len2 != 0.0 {
            let inv = 1.0 / len2.sqrt();
            [direction[0] * inv, direction[1] * inv]
        } else {
            direction
        };
        let end = V::new(origin.x + distance * dir[0], origin.y + distance * dir[1]);
        let hits = self.ray_query(origin, end, mask, min_depth, max_depth);
        hits
    }

    /// Physics2D.LinecastNonAlloc(start, end, buffer[8], layerMask): the number of hits.
    fn linecast_count(&mut self, start: V, end: V, mask: u32) -> usize {
        self.ray_query(start, end, mask, f32::NEG_INFINITY, f32::INFINITY).len().min(EXPLOSIVE_LOS_BUFFER)
    }

    /// Physics2D.LinecastNonAlloc(start, end, buffer[n], layerMask): the hits, nearest first.
    pub(super) fn linecast_hits(&mut self, start: V, end: V, mask: u32, buffer: usize) -> Vec<RayHit2D> {
        let mut hits = self.ray_query(start, end, mask, f32::NEG_INFINITY, f32::INFINITY);
        // PhysicsQuery2D::Linecast (180bf2c50): every hit sorted, then the first min(count, buffer length) copied.
        hits.truncate(buffer);
        hits
    }

    /// Physics2D.OverlapCircleAll / OverlapCircleNonAlloc (OverlapCircleQuery2D, 180bf11a0): AutoSyncTransforms, a
    /// b2CircleShape at `center` against the fixtures in its AABB (b2TestOverlap), ReportFixture's filters as
    /// `overlap_query`. The hits as (the collider's physical, the Collider2D id).
    pub(super) fn overlap_circle(&mut self, center: V, radius: f32, mask: u32) -> Vec<(Option<usize>, i32)> {
        self.sync_pending();
        if radius < 1e-5 {
            // OverlapCircleQuery2D::InitializeAABB (180bf11a0): a radius under 1e-5 is a point query (the AABB is the
            // point), and IsFixtureValid (180bf1b00) then tests the point against the shape.
            return self.overlap_query(self.world.query_point(center), mask, f32::NEG_INFINITY, f32::INFINITY, |shape, _, xf| super::tool::test_point(shape, xf, center));
        }
        let shape = Shape::Circle(Circle { radius, p: center });
        let identity = Transform::default_identity();
        let aabb = shape.compute_aabb(&identity, 0);
        self.overlap_query(self.world.query_aabb(&aabb), mask, f32::NEG_INFINITY, f32::INFINITY, |other, child, xf| shape.test_overlap(0, &identity, other, child, xf))
    }

    /// The Rigidbody2D a b2 body belongs to (Collider2D.attachedRigidbody); None for a static collider's own body.
    pub(super) fn rigidbody_of_body(&self, body: BodyId) -> Option<super::Rigidbody> {
        self.physicals.iter().find(|p| !p.destroyed && p.rigidbody.body == body).map(|p| p.rigidbody.clone()).or_else(|| self.node_rigidbodies.iter().find(|n| n.rigidbody.body == body).map(|n| n.rigidbody.clone()))
    }

    /// Transform.position of a collider's GameObject.
    pub(super) fn collider_position(&self, collider: i32) -> [f32; 3] {
        match self.physical_of_collider(collider) {
            Some(p) => self.physicals[p].transform.position,
            None => self.bare_position(collider),
        }
    }

    /// Transform.position of the explosive's GameObject.
    fn explosive_position(&mut self, e: usize) -> [f32; 3] {
        match self.explosives[e].physical {
            Some(p) => self.physicals[p].transform.position,
            None => {
                let id = self.explosives[e].id;
                self.with_component_instance(id, |_, inst, o| inst.position(o))
            }
        }
    }

    /// GameObject.activeInHierarchy of the explosive's object.
    fn explosive_active(&mut self, e: usize) -> bool {
        match self.explosives[e].physical {
            Some(p) => self.physicals[p].active,
            None => {
                let id = self.explosives[e].id;
                self.with_component_instance(id, |_, inst, o| inst.objects[o].active)
            }
        }
    }

    /// Whether the explosive's GameObject is active and not destroyed (a coroutine dies with it).
    fn explosive_alive(&mut self, e: usize) -> bool {
        self.explosive_check_added(e);
        !self.destroyed_components.contains(&self.explosives[e].id) && self.explosive_active(e)
    }

    /// An added explosive leaves with its destroyed GameObject (it is in none of the clone's component lists).
    fn explosive_check_added(&mut self, e: usize) {
        let s = &self.explosives[e];
        if s.added && let Some(p) = s.physical && self.physicals[p].destroyed && self.destroyed_components.insert(s.id) {
            let id = s.id;
            if let Some((_, b)) = self.behaviour_of.remove(&id) {
                self.behaviours.remove(b);
            }
        }
    }

    /// GetComponents<ExplosiveBehaviour>() on physical `p`'s GameObject, in component order (added ones last).
    pub(super) fn explosives_on(&mut self, p: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.explosives.len()).filter(|&e| self.explosives[e].physical == Some(p)).collect();
        for &e in &v {
            self.explosive_check_added(e);
        }
        v.retain(|&e| !self.destroyed_components.contains(&self.explosives[e].id));
        v.sort_by_key(|&e| self.explosives[e].index);
        v
    }

    /// The explosives added at run time to physical `p`'s GameObject, in the order they were added.
    pub(super) fn added_explosives_on(&mut self, p: usize) -> Vec<usize> {
        let mut v = self.explosives_on(p);
        v.retain(|&e| self.explosives[e].added);
        v
    }

    /// ExplosiveBehaviour.Awake, for GameObject.AddComponent<ExplosiveBehaviour>() on physical `p`'s GameObject: a new instance id
    /// (GetComponent<PhysicalBehaviour>, BlockingLayers 0 becomes the default mask), the manager lists, and Start
    /// before its first update.
    pub(crate) fn add_explosive(&mut self, p: usize) -> usize {
        let id = self.allocate_instance_id();
        // AddComponent comes after the GameObject's components (an earlier added one among them); a recording has no clone to count them on.
        let index = if self.is_recording() {
            1000 + self.explosives.iter().filter(|x| x.added && x.physical == Some(p)).count()
        } else {
            let count = self.with_component_instance(self.physicals[p].id, |_, inst, o| inst.objects[o].components.len());
            // The clone carries it from then on (GetComponents for ObjectState's prototypes).
            self.add_script_to_clone(self.physicals[p].id, "ExplosiveBehaviour", id);
            count
        };
        self.explosives.push(Explosive { id, physical: Some(p), BlockingLayers: 10752, index, added: true, ..Default::default() });
        self.explosive_awake(self.explosives.len() - 1);
        let e = self.explosives.len() - 1;
        self.script_slots.insert(id, super::save_slots::Slot::Explosives(e));
        let b = Behaviour::Explosive(e);
        // ExplosiveBehaviour's MonoScript execution order is 0.
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::Explosive(e));
        e
    }

    /// AddComponent<ExplosiveBehaviour> on the clone object with Transform `tid` that has no PhysicalBehaviour: the
    /// component goes last on the object, which carries it from then on (position, active, destroy).
    pub(super) fn add_explosive_on_object(&mut self, tid: i32) -> usize {
        let id = self.allocate_instance_id();
        self.add_script_to_clone(tid, "ExplosiveBehaviour", id);
        self.explosives.push(Explosive { id, physical: None, BlockingLayers: 10752, added: true, ..Default::default() });
        self.explosive_awake(self.explosives.len() - 1);
        let e = self.explosives.len() - 1;
        let b = Behaviour::Explosive(e);
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::Explosive(e));
        e
    }

    /// Nitroglycerine.OnUpdate: a living limb's circulation with more than 0.28 of it takes damage.
    pub(crate) fn nitroglycerine_on_update(&mut self, liquid: usize, o: super::container::Owner) {
        let super::container::Owner::Circulation(l) = o else { return };
        if self.limbs[l].SpeciesIdentity != "Android" && self.get_amount(o, liquid) > 0.28 {
            let damage = 10.0 * self.random.value();
            self.limb_damage(l, damage);
        }
    }

    /// Nitroglycerine.OnEnterContainer: unless the GameObject's
    /// first ExplosiveBehaviour passes CompareTag("nitro") (the GameObject's tag), it gains a new one.
    pub fn nitroglycerine_on_enter_container(&mut self, _this: usize, o: super::container::Owner) {
        let p = self.nitro_physical(o);
        let first = self.explosives_on(p).first().copied();
        let tagged = first.is_some() && self.with_component_instance(self.physicals[p].id, |_, inst, x| inst.prefab.objects[x].tag) == NITRO_TAG;
        if tagged {
            return;
        }
        let e = self.add_explosive(p);
        let limb = self.limbs.iter().any(|l| l.PhysicalBehaviour == p);
        let s = &mut self.explosives[e];
        s.Tag = crate::il!(str "Nitroglycerine::OnEnterContainer" 0).into();
        s.Range = 5.0;
        s.Delay = 0.0;
        s.FragmentationRayCount = 8;
        // container.GetComponent<LimbBehaviour>(): a CirculationBehaviour's GameObject has its LimbBehaviour.
        s.DestroyOnExplode = limb;
        s.ShouldCreateKillzone = false;
        s.ArmOnUse = false;
        s.MinChargeToExplode = 4.0;
        s.ExplodesOnFragmentHit = true;
        s.ImpactForceThreshold = 2.0;
        s.DismemberChance = 0.2;
        s.ExplodeBurnProgressThreshold = 0.5;
    }

    /// Nitroglycerine.OnExitContainer: Object.Destroy of the GameObject's first ExplosiveBehaviour when its Tag is
    /// "nitro".
    pub fn nitroglycerine_on_exit_container(&mut self, _this: usize, o: super::container::Owner) {
        let p = self.nitro_physical(o);
        if let Some(&e) = self.explosives_on(p).first() && self.explosives[e].Tag == crate::il!(str "Nitroglycerine::OnExitContainer" 0) {
            let id = self.explosives[e].id;
            self.tr_destroy_component(id, 0.0);
        }
    }

    fn nitro_physical(&self, o: super::container::Owner) -> usize {
        if let super::container::Owner::Flask(f) = o && self.flasks[f].Phys.is_none() {
            unimplemented!("mod-only: Nitroglycerine: AddComponent<ExplosiveBehaviour> on a flask without a PhysicalBehaviour");
        }
        self.container_physical(o)
    }

    /// StartCoroutine(Explode(delay, forceDoubleCheck)) up to its first yield (WaitForSeconds). On an inactive
    /// GameObject StartCoroutine only logs an error: Explode never runs (no Armed).
    pub(super) fn explosive_start_explode(&mut self, e: usize, delay: f32, force_double_check: bool) {
        if self.explosive_active(e) {
            self.explosive_explode_first(e, delay, force_double_check);
        }
    }

    /// Explode(delay, forceDoubleCheck) up to its first yield (WaitForSeconds).
    pub(crate) fn explosive_explode_first(&mut self, e: usize, delay: f32, force_double_check: bool) {
        let s = &self.explosives[e];
        if (!s.CanExplodeMultipleTimes && s.hasExploded) || (force_double_check && s.hasExploded) {
            return;
        }
        let wake = self.time.timeAsDouble + delay as f64;
        self.coroutine_seq += 1;
        let seq = self.coroutine_seq;
        self.explosives[e].Armed = true;
        // OnArm?.Invoke()
        self.invoke_event(self.explosives[e].id, "OnArm");
        self.explosives[e].coroutines.push(Coroutine { wake, from: self.time.frameCount as i64 + 1, seq, end_of_frame: false });
    }

    /// ExplosiveBehaviour.Activate
    pub fn explosive_activate(&mut self, e: usize) {
        let delay = self.explosives[e].Delay;
        self.explosive_start_explode(e, delay, false);
    }

    /// ExplosiveBehaviour.Use
    pub fn explosive_use(&mut self, e: usize) {
        if !self.explosives[e].Armed && self.explosives[e].ArmOnUse {
            self.explosive_activate(e);
        }
    }

    /// ExplosiveBehaviour.ForceImmediateExplosion (and ExplosiveBehaviour.ForceImmediateExposion, its obsolete spelling, which calls it)
    pub fn explosive_force_immediate(&mut self, e: usize) {
        self.explosives[e].coroutines.clear();
        self.explosive_start_explode(e, 0.0, false);
    }

    /// ExplosiveBehaviour.Disarm: StopAllCoroutines (the pending Explode coroutines), then Armed = false.
    pub fn explosive_disarm(&mut self, e: usize) {
        self.explosives[e].coroutines.clear();
        self.explosives[e].Armed = false;
    }

    /// ExplosiveBehaviour.Start: the Killzone clone, Instantiate(Prefabs/Killzone, position, identity, transform),
    /// and SetRange(Range); then Activate for ArmOnAwake.
    pub fn explosive_start(&mut self, e: usize) {
        if !self.explosive_active(e) {
            return;
        }
        if self.explosives[e].ShouldCreateKillzone {
            let position = self.explosive_position(e);
            // A recording's load: the recorded Killzone(Clone)'s ids (recorded_killzones); the ids after it stay where they were.
            let at = self.recorded_killzones.remove(&self.explosives[e].id).map(|kz| std::mem::replace(&mut self.next_instance_id, kz));
            let id = self.spawn_resource("Prefabs/Killzone", position);
            if let Some(next) = at {
                self.next_instance_id = next;
            }
            // transform is the explosive's own object (a firework burst's TextBurst has no body).
            let parent = match self.explosives[e].physical {
                Some(p) => AttachParent::Physical(p),
                None => AttachParent::Object(self.explosives[e].id),
            };
            self.attach_clone(id, parent, position);
            let owner = self.attached_parent_of(id).expect("the Killzone just attached").0;
            self.runtime_child_add(owner, super::save::RuntimeKind::Clone(id), None);
            self.explosives[e].Killzone = Some(id);
            let range = self.explosives[e].Range;
            self.explosive_set_range(e, range);
        }
        if self.explosives[e].ArmOnAwake {
            self.explosive_activate(e);
        }
    }

    /// ExplosiveBehaviour.SetRange(r): a live Killzone's IgnoreParentSize.DesiredSize = 2 * r * Vector2.one.
    pub fn explosive_set_range(&mut self, e: usize, r: f32) {
        if let Some(k) = self.explosives[e].Killzone.filter(|k| !self.destroyed_components.contains(k)) {
            let i = self.killzone_ignore_parent_size(k);
            self.ignore_parent_sizes[i].DesiredSize = [2.0 * r, 2.0 * r];
        }
    }

    /// ExplosiveBehaviour.FixedUpdate
    pub fn explosive_fixed_update(&mut self, e: usize) {
        if self.explosives[e].added && !self.explosive_alive(e) {
            // Gone with its GameObject, or the GameObject is inactive (the manager skips it).
            return;
        }
        let s = &self.explosives[e];
        let Some(p) = s.physical.map(|p| &self.physicals[p]) else { return };
        if p.Temperature >= s.TemperatureLimit || (s.ExplodeBurnProgressThreshold < 1.0 && p.BurnProgress > s.ExplodeBurnProgressThreshold) || p.Charge() > s.MinChargeToExplode {
            self.explosive_start_explode(e, 0.0, false);
        }
    }

    /// ExplosiveBehaviour.OnCollisionEnter2D
    pub fn explosive_on_collision_enter(&mut self, e: usize, c: &ScriptCollision) {
        let threshold = self.explosives[e].ImpactForceThreshold;
        if !(threshold < 0.0) {
            let contact = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("ArgumentOutOfRangeException: Index was out of range. Must be non-negative and less than the size of the collection.
Parameter name: index"));
            if contact.normal_impulse > threshold {
                self.explosive_start_explode(e, 0.05, false);
            }
        }
    }

    /// ExplosiveBehaviour.OnFragmentHit
    pub fn explosive_on_fragment_hit(&mut self, e: usize) {
        let s = &self.explosives[e];
        if s.ExplodesOnFragmentHit && !s.Armed {
            self.explosive_start_explode(e, 0.05, true);
        }
    }

    /// The pending Explode coroutines whose WaitForSeconds ran out (DelayedCallManager, between Update and
    /// LateUpdate, in wake order): each goes on to wait for the end of the frame.
    pub fn explosive_coroutines(&mut self) {
        let now = self.time.timeAsDouble;
        let frame = self.time.frameCount as i64;
        let mut due: Vec<(f64, u64, usize)> = Vec::new();
        for (e, s) in self.explosives.iter().enumerate() {
            due.extend(s.coroutines.iter().filter(|c| !c.end_of_frame && c.wake <= now && frame >= c.from).map(|c| (c.wake, c.seq, e)));
        }
        due.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        for (_, seq, e) in due {
            if !self.explosive_alive(e) {
                self.explosives[e].coroutines.clear();
                continue;
            }
            self.explosives[e].coroutines.iter_mut().find(|c| c.seq == seq).unwrap().end_of_frame = true;
            let owner = self.explosives[e].id;
            self.end_of_frame.push(super::destroyable::Coroutine { seq, at: now, owner, from: i64::MIN, step: super::destroyable::Step::ExplosiveEndOfFrame(e, seq) });
        }
    }

    /// WaitForEndOfFrame: the coroutines that reached it run on, in the order they got there; one that waits for
    /// the end of a frame again here waits for the next.
    pub fn end_of_frame_calls(&mut self) {
        self.delayed_dynamic_calls();
        // PostLateUpdate.UpdateAudio, after ScriptRunDelayedDynamicFrameRate.
        self.audio_update();
        self.end_of_frame_coroutines();
    }

    /// PostLateUpdate.ScriptRunDelayedDynamicFrameRate, before TriggerEndOfFrameCallbacks.
    /// The delayed Destroys of this frame's Update and coroutines go here, before rendering.
    pub fn delayed_dynamic_calls(&mut self) {
        self.gore_string_delayed_calls(false);
        self.routines_dynamic_pass();
        // PostLateUpdate's renderer update, after ScriptRunDelayedDynamicFrameRate: the bolts' trails (a bolt a
        // coroutine Requested in this pass gets its point in this frame).
        self.archelix_trails_update();
        self.archelix_trail_lines();
        self.blasterbolt_trails_update();
        self.trails_update();
    }

    /// TriggerEndOfFrameCallbacks: the WaitForEndOfFrame coroutines.
    pub fn end_of_frame_coroutines(&mut self) {
        for c in std::mem::take(&mut self.end_of_frame) {
            // A coroutine stopped since is gone from the list.
            self.resume_coroutine(c);
        }
    }

    /// The rest of an Explode coroutine after WaitForEndOfFrame.
    pub(super) fn explosive_end_of_frame(&mut self, e: usize, seq: u64) {
        // StopAllCoroutines removed it.
        let Some(i) = self.explosives[e].coroutines.iter().position(|c| c.seq == seq) else { return };
        self.explosives[e].coroutines.remove(i);
        if self.explosive_alive(e) {
            self.explosive_explode(e);
        }
    }

    /// ExplosiveBehaviour.Explode
    /// The rest of Explode after WaitForEndOfFrame.
    pub(crate) fn explosive_explode(&mut self, e: usize) {
        let s = &self.explosives[e];
        let (id, p) = (s.id, s.physical);
        let disintegrate = s.DisintegrateOnExplode;
        for call in s.OnExplodeCalls.clone() {
            self.event_call(call);
        }
        if self.explosives[e].DestroyOnExplode {
            self.explosive_disable_colliders(e);
        }
        let limb = p.and_then(|p| self.limbs.iter().position(|l| l.PhysicalBehaviour == p));
        if limb.is_some() {
            super::stats::unlock_achievement("INSIDES_HURT");
        }
        let position = self.explosive_position(e);
        let under_water = p.is_some_and(|p| self.physicals[p].IsUnderWater) | self.is_point_under_water([position[0], position[1]]);
        let s = &self.explosives[e];
        let mut ex = ExplosionParameters {
            FragmentationRayCount: s.FragmentationRayCount as u32,
            Position: position,
            Range: s.Range,
            FragmentForce: s.FragmentForce,
            CreateParticlesAndSound: s.ParticleEffectAndSound,
            DismemberChance: s.DismemberChance,
            BallisticShrapnelCount: s.BallisticShrapnelCount,
            ..Default::default()
        };
        ex.set_large_explosion_particles(s.BigExplosion);
        self.create_explosion_with_water(under_water, ex);
        let s = &self.explosives[e];
        if s.BurnPower > 0.001 || s.ShockwaveLiftForce > 0.01 || s.AffectVictimTemperature.abs() > 1.0 {
            self.explosive_affect_victims(e, position, ex.Position);
        }
        // TryGetComponent<BloodContainer>(out c): c.ClearLiquid()
        if let Some(c) = p.and_then(|p| self.physical_container(p)) {
            self.clear_liquid(c);
        }
        let s = &self.explosives[e];
        if !under_water {
            if let Some(vfx) = s.VFXOverride {
                self.spawn_prefab(vfx, position);
            }
        }
        let s = &self.explosives[e];
        if s.DestroyOnExplode {
            match limb {
                Some(l) => self.limb_crush(l),
                None => self.destroy_object(id),
            }
        } else if disintegrate {
            if let Some(l) = limb {
                self.limb_crush(l);
            } else if let Some(p) = p.filter(|&p| self.physicals[p].Disintegratable && !self.physicals[p].isDisintegrated) {
                self.physical_disintegrate(p);
            }
            if let Some(k) = self.explosives[e].Killzone {
                self.destroy_object(k);
            }
        } else if let Some(x) = p.and_then(|p| self.destroyables.iter().position(|x| x.PhysicalBehaviour == p)) {
            self.destroyable_break(x, [0.0, 0.0]);
        }
        self.explosives[e].hasExploded = true;
    }

    /// Collider2D.enabled = false on GetComponentsInChildren<Collider2D>() (the active objects below).
    fn explosive_disable_colliders(&mut self, e: usize) {
        // An added one finds its GameObject through its PhysicalBehaviour.
        let id = match self.explosives[e] {
            Explosive { added: true, physical: Some(p), .. } => self.physicals[p].id,
            ref s => s.id,
        };
        // The active objects below, depth first, each object's components in order.
        let colliders: Vec<i32> = self.with_component_instance(id, |_, inst, o| {
            fn collect(inst: &super::spawn::Instance, x: usize, out: &mut Vec<i32>) {
                if !inst.objects[x].active {
                    return;
                }
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    if matches!(c, Component::Collider2D(_)) {
                        out.push(inst.objects[x].components[k]);
                    }
                }
                for &child in &inst.prefab.objects[x].children {
                    collect(inst, child, out);
                }
            }
            let mut out = Vec::new();
            collect(inst, o, &mut out);
            out
        });
        for collider in colliders {
            // A collider of this object's physical, or of a child's own PhysicalBehaviour.
            let body = match self.physicals.iter().find(|q| !q.destroyed && q.all_colliders.contains(&collider)) {
                Some(q) => q.rigidbody.body,
                None => match self.bare_colliders[&collider].anchor {
                    super::BareAnchor::Physical(q) => self.physicals[q].rigidbody.body,
                    super::BareAnchor::Node(n) => self.node_rigidbodies.iter().find(|r| r.node == n).map(|r| r.rigidbody.body).expect("a Transform node's body"),
                    // No Rigidbody2D: the collider's shapes sit on the static body made for it.
                    super::BareAnchor::Root => self.world.collider_body(collider).expect("a collider's fixtures"),
                },
            };
            self.collider_disable_one(collider, body);
        }
    }

    /// The BurnPower / ShockwaveLiftForce / AffectVictimTemperature pass over the colliders in range that see the
    /// explosion.
    fn explosive_affect_victims(&mut self, e: usize, position: [f32; 3], ex_position: [f32; 3]) {
        let s = &self.explosives[e];
        let (range, mask) = (s.Range, s.BlockingLayers);
        let (lift, burn, temperature) = (s.ShockwaveLiftForce, s.BurnPower, s.AffectVictimTemperature);
        let centre = V::new(position[0], position[1]);
        for (victim, _) in self.overlap_circle(centre, range, mask) {
            // PhysicalObjectsInWorldByTransform: the collider sits on the physical's own Transform.
            let Some(v) = victim.filter(|&v| self.physicals[v].in_world) else { continue };
            let at = self.physicals[v].transform.position;
            if self.linecast_count(V::new(at[0], at[1]), centre, mask) > 0 {
                continue;
            }
            let rb = self.physicals[v].rigidbody.clone();
            let force = rb.mass(&self.world) * lift;
            rb.add_force(&mut self.world, [0.0 * force, 1.0 * force]);
            let num = burn / 8.0 * self.random.value();
            if self.physicals[v].BurnProgress < num {
                self.physicals[v].BurnProgress = num;
            }
            if self.random.value() < burn {
                self.ignite_at(v, true, [ex_position[0], ex_position[1]]);
            }
            let t = self.physicals[v].Temperature;
            if (temperature > 0.0 && t < temperature) || (temperature < 0.0 && t > temperature) {
                let heat = 0.035 / self.heat_capacity(v);
                self.physicals[v].Temperature = mathf::lerp(t, temperature, heat);
            }
        }
    }

    /// ExplosionCreator.Explode(center, force): DismemberChance 0.025, Range force * 2.5, 24 fragment rays
    /// (LargeExplosionParticles = force > 30: EffectSize Large).
    pub fn explosion_creator_explode_at(&mut self, center: [f32; 3], force: f32) {
        self.explosion_creator_explode(ExplosionParameters {
            FragmentationRayCount: 24,
            Position: center,
            Range: force * 2.5,
            FragmentForce: force,
            CreateParticlesAndSound: true,
            EffectSize: if force > 30.0 { EffectSize::Large } else { EffectSize::Medium },
            DismemberChance: 0.025,
            ..Default::default()
        });
    }

    /// ExplosionCreator.Explode(ex)
    pub fn explosion_creator_explode(&mut self, ex: ExplosionParameters) {
        let under_water = self.is_point_under_water([ex.Position[0], ex.Position[1]]);
        self.create_explosion_with_water(under_water, ex);
    }

    /// OnlyExplosionBehaviour.Start (ExplodeOnStart): OnlyExplosionBehaviour.Explode with Position = transform.position.
    pub fn only_explosion_start(&mut self, id: i32, mut ex: ExplosionParameters) {
        ex.Position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.explosion_creator_explode(ex);
    }

    /// ExplosionCreator.CreateExplosionWithWater(isUnderWater, ex)
    pub fn create_explosion_with_water(&mut self, under_water: bool, ex: ExplosionParameters) {
        self.create_explosion_with_water_splash(under_water, ex, true);
    }

    /// ExplosionCreator.CreateExplosionWithWater(isUnderWater, ex, doSplash)
    pub fn create_explosion_with_water_splash(&mut self, under_water: bool, mut ex: ExplosionParameters, do_splash: bool) {
        if under_water {
            self.create_underwater_explosion_effect(ex.Position, ex.Range, do_splash);
            ex.CreateParticlesAndSound = false;
        }
        self.create_fragmentation_explosion(ex);
    }

    /// ExplosionCreator.CreateUnderwaterExplosionEffect
    pub(crate) fn create_underwater_explosion_effect(&mut self, position: [f32; 3], range: f32, do_splash: bool) {
        let water = self.water_at_point([position[0], position[1]]);
        let given_top = water.map_or(position[1], |w| self.water_global_surface_level(w));
        let root = self.spawn_resource("Prefabs/UnderwaterExplosion", position);
        let scale = super::mathf::lerp((super::d(range) / 10.0).f(), 1.0, 0.8);
        self.with_component_instance(root, |_, inst, _| inst.set_local_scale(0, [scale, scale, scale]));
        if !self.effects_off {
            self.effect_log.last_mut().expect("the clone's effect").scale = scale;
        }
        // The clone's behaviour (queue_starts made it with the prefab's fields): the fields set after Instantiate.
        let u = self.underwater_explosions.iter_mut().rfind(|u| u.root == root).expect("UnderwaterExplosionBehaviour on Prefabs/UnderwaterExplosion");
        (u.position, u.GivenTop, u.MaxDistanceToSurface, u.DoSplash, u.scale) = (position, given_top, range.clamp(7.0, 25.0), do_splash, scale);
    }

    /// UnderwaterExplosionBehaviour.Start; the Splash object, and its ExplosionSoundBehviour's Near count and DistantSound.
    pub(super) fn underwater_explosion_start(&mut self, root: i32, splash: ObjRef, near: usize, distant: i64, splash_audio: i32) {
        let i = self.underwater_explosions.iter().rposition(|u| u.root == root).expect("made by queue_starts");
        let u = &mut self.underwater_explosions[i];
        (u.splash, u.near, u.distant, u.splash_audio) = (Some(splash), near, distant, splash_audio);
        self.underwater_explosion_start_at(i);
    }

    /// UnderwaterExplosionBehaviour.Start's body: Splash at GivenTop and StartCoroutine(DelayedSplash(min(0.25, d * 0.01))) when the explosion is within
    /// MaxDistanceToSurface (d) of GivenTop and DoSplash.
    pub(crate) fn underwater_explosion_start_at(&mut self, i: usize) {
        let u = &self.underwater_explosions[i];
        let num = (super::d(u.position[1]) - u.GivenTop).f().abs();
        if num <= u.MaxDistanceToSurface && u.DoSplash {
            let delay = (super::d(num) * 0.01).f().min(0.25);
            let (root, top) = (u.root, u.GivenTop);
            // Splash.transform.position = (Splash.transform.position.x, GivenTop, 0).
            self.with_component_instance(root, |_, inst, _| {
                if let Some(o) = (0..inst.objects.len()).find(|&o| inst.prefab.objects[o].name == "Splash") {
                    let x = inst.position(o)[0];
                    inst.set_world_position(o, [x, top, 0.0]);
                }
            });
            self.start_wait(root, delay, super::destroyable::Step::UnderwaterSplash(i));
        }
    }

    /// UnderwaterExplosionBehaviour.DelayedSplash: Splash.Play(withChildren: true) at the surface, SplashAudio.Play().
    pub fn underwater_splash(&mut self, i: usize) {
        let u = self.underwater_explosions[i].clone();
        // ActivateIfFancyEffects keeps Splash inactive without FancyEffects: Play does nothing, StartCoroutine fails.
        let active = self.prefs.FancyEffects;
        if !self.effects_off && active {
            let prefab = self.assets.as_ref().and_then(|db| db.resource("Prefabs/UnderwaterExplosion", 1).ok().flatten());
            self.effect_log.push(super::EffectSpawn {
                prefab: prefab.expect("Resources.Load(Prefabs/UnderwaterExplosion)"),
                position: [u.position[0], u.GivenTop, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: u.scale,
                color: None,
                root: Some(u.root),
                subtree: u.splash,
                water: None,
                pooled: None,
                parent: None,
                off: Vec::new(),
                tint_all: false,
            });
        }
        // SplashAudio.Play(): audioSource.clip = Near.PickRandom(); audioSource.PlayDelayed(0).
        let k = self.pick_random(u.near);
        self.explosion_sound_play_clip(u.splash_audio, k, 0.0);
        if self.prefs.DistantSoundEffects && u.distant != 0 && active {
            self.start_wait(u.root, 0.0, super::destroyable::Step::DistantSound(u.distant));
        }
    }

    /// ExplosionSoundBehviour.Play(delay): with an audioSource (`near` is Near's length), audioSource.clip = Near.PickRandom(),
    /// PlayDelayed(delay), and with DistantSoundEffects the DelayCoroutine(delay) to DistantSoundBehaviour.Play.
    pub(crate) fn explosion_sound_play(&mut self, id: i32, near: Option<usize>, distant: i64, delay: f32) {
        if let Some(n) = near {
            let k = self.pick_random(n);
            self.explosion_sound_play_clip(id, k, delay);
            if self.prefs.DistantSoundEffects && distant != 0 {
                self.start_wait(id, delay, super::destroyable::Step::DistantSound(distant));
            }
        }
    }

    /// ExplosionSoundBehviour's Behaviour.enabled: a prefab-enabled one unless set_enabled(false) put it in `disabled_ids`; a
    /// prefab-disabled one once set_enabled(true) (`explosion_sounds_enabled`) and not disabled again.
    pub(super) fn explosion_sound_enabled(&self, id: i32, prefab_enabled: bool) -> bool {
        (prefab_enabled || self.explosion_sounds_enabled.contains(&id)) && !self.disabled_ids.contains(&id)
    }

    /// ExplosionSoundBehviour set_enabled(true) (no OnEnable). One that has not started gets its Start (StartWhenAwake:
    /// Play(Delay)) before its first Update, once: now on an active object, else at the object's SetActive(true).
    pub(super) fn explosion_sound_enable(&mut self, id: i32, prefab_enabled: bool, start: Option<(Option<usize>, i64, f32)>) {
        self.disabled_ids.remove(&id);
        if !prefab_enabled {
            self.explosion_sounds_enabled.insert(id);
        }
        if self.explosion_sounds_started.contains(&id) {
            return;
        }
        match self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            true => self.explosion_sound_start(id, start),
            false => {
                self.explosion_sounds_waiting.insert(id, (prefab_enabled, start));
            }
        }
    }

    /// ExplosionSoundBehviour.Start queued: with StartWhenAwake, Play(Delay).
    pub(super) fn explosion_sound_start(&mut self, id: i32, start: Option<(Option<usize>, i64, f32)>) {
        self.explosion_sounds_started.insert(id);
        if let Some((near, distant, delay)) = start {
            self.pending_starts.push(super::misc::PendingStart::ExplosionSoundPlay(id, near, distant, delay));
        }
    }

    /// GameObject.SetActive(true) on `inst`: the waiting ExplosionSoundBehviours now active and enabled start (one queue_starts
    /// already started, on a first activation, stays started). A disabled one waits for its set_enabled(true).
    pub(super) fn explosion_sounds_wake(&mut self, inst: &super::spawn::Instance) {
        let now: Vec<i32> = self.explosion_sounds_waiting.keys().copied().filter(|&id| inst.objects.iter().any(|x| x.active && x.components.contains(&id))).collect();
        for id in now {
            let (prefab_enabled, start) = self.explosion_sounds_waiting.remove(&id).unwrap();
            if !self.explosion_sounds_started.contains(&id) && self.explosion_sound_enabled(id, prefab_enabled) {
                self.explosion_sound_start(id, start);
            }
        }
    }

    /// ExplosionSoundBehviour.Play's audio: audioSource.clip = Near[k] (null when Near is empty), PlayDelayed(delay).
    /// audioSource is the field, or Awake's GetComponent<AudioSource>().
    pub(crate) fn explosion_sound_play_clip(&mut self, id: i32, k: Option<usize>, delay: f32) {
        let Some(source) = self.audio.source_field(id, "audioSource").or_else(|| self.audio.object_source(id)) else { return };
        if !self.audio.sources.contains_key(&source) {
            return;
        }
        // fc41 r11: an all-null Near (PulseExplosion's four nulls) is not registered (register_clip_fields): PickRandom's null.
        let clip = k.and_then(|k| self.audio.clips(id, "Near").get(k).cloned().flatten());
        self.audio.set_clip(source, clip);
        self.audio.play_delayed(source, delay);
    }

    /// ExplosionSoundBehviour.Awake: Global.AddAudioSource(audioSource) (the field, or GetComponent<AudioSource>()).
    pub(crate) fn explosion_sound_awake(&mut self, id: i32) {
        if let Some(source) = self.audio.source_field(id, "audioSource").or_else(|| self.audio.object_source(id)) {
            let t = self.time.timeScale;
            self.audio.add_time_scaled(source, t, false);
        }
    }

    /// ExplosionCreator.CreatePulseExplosion(position, force, range, soundAndEffects: false, breakObjects: false)
    pub(crate) fn create_pulse_explosion(&mut self, position: [f32; 3], force: f32, range: f32) {
        self.create_pulse_explosion_with(position, force, range, false, false);
    }

    /// ExplosionCreator.CreatePulseExplosion(position, force, range, soundAndEffects, breakObjects)
    pub(crate) fn create_pulse_explosion_with(&mut self, position: [f32; 3], force: f32, range: f32, sound_and_effects: bool, break_objects: bool) {
        self.camera_shake((d(force) * range * 2.0f32).f(), [position[0], position[1]]);
        if sound_and_effects {
            self.spawn_resource(EffectSize::Medium.prefab_path(), position);
        }
        // OverlapCircleNonAlloc into the 64-entry hitBuffer: Objects, Debris, CollidingDebris.
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), range, (1 << 9) | (1 << 10) | (1 << 13));
        // OverlapCircleArray_Binding (180bf4620): the query's first min(count, buffer length) colliders.
        hits.truncate(HIT_BUFFER);
        for (_, collider) in hits {
            let Some(body) = self.world.collider_body(collider) else { continue };
            let Some(rb) = self.rigidbody_of_body(body) else { continue };
            let at = self.collider_position(collider);
            let v = [at[0] - position[0], at[1] - position[1], at[2] - position[2]];
            let sqr = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).f();
            if sqr < mathf::EPSILON {
                continue;
            }
            let num2 = mathf::sqrt(sqr);
            let dir = [v[0] / num2, v[1] / num2];
            let num3 = (d(force) / mathf::max(1.0, (d(sqr) / (d(range) * range)).f()) * 3.0f32).f();
            let num4 = mathf::min(rb.mass(&self.world), 1.0);
            // 1.28.45: val3 * (num3 * num4).
            let push = if crate::assets::il::is_1_28_45() {
                let k = (d(num3) * num4).f();
                [dir[0] * k, dir[1] * k]
            } else {
                [num3 * dir[0] * num4, num3 * dir[1] * num4]
            };
            rb.add_force_impulse(&mut self.world, push);
            if break_objects && self.random.range_int(0, 10) as f32 > force {
                // collider2D.BroadcastMessage("Break", (Vector2)(-1f * num3 * vector2), DontRequireReceiver)
                let k = -1.0 * num3;
                self.broadcast_break(collider, [k * dir[0], k * dir[1]]);
            }
        }
    }

    /// ExplosionCreator.CreateFragmentationExplosion
    pub(super) fn create_fragmentation_explosion(&mut self, ex: ExplosionParameters) {
        // extras: crabs hear explosions (src/extras_hook.rs; reads no game state back).
        crate::extras_hook::heard_explosion(self.time.time, ex.Position, ex.Range);
        self.camera_shake(ex.FragmentForce, [ex.Position[0], ex.Position[1]]);
        if ex.CreateParticlesAndSound {
            self.spawn_resource(ex.EffectSize.prefab_path(), ex.Position);
        }
        for i in 0..ex.FragmentationRayCount {
            self.fragmentation_ray(i, ex);
        }
        // The loop condition runs OverlapCircleNonAlloc again before each message; nothing it reads changes.
        let centre = V::new(ex.Position[0], ex.Position[1]);
        let mut hits = self.overlap_circle(centre, mathf::max(ex.Range / 10.0, 1.0), DEFAULT_RAYCAST_LAYERS);
        // OverlapCircleArray_Binding (180bf4620): the query's first min(count, buffer length) colliders.
        hits.truncate(HIT_BUFFER);
        for (physical, collider) in hits {
            // SendMessage("Decal", DecalInstruction(BlastMarkDecal, ClosestPoint(Position), 2)) to the collider's GameObject.
            let point = self.world.collider_closest_point(collider, centre);
            match physical {
                Some(p) => {
                    let ty = self.decal_descriptor("Blast mark");
                    self.physical_decal(p, ty, [point.x, point.y], 2.0, [1.0; 4]);
                }
                None if self.bare_colliders[&collider].receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"]) => {
                    unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                }
                None => {}
            }
        }
        if ex.BallisticShrapnelCount > 0 && self.shrapnel.as_ref().is_some_and(|s| s.instance) {
            self.global_shrapnel_emit_shrapnel([ex.Position[0], ex.Position[1]], ex.BallisticShrapnelCount);
        }
    }

    /// ExplosionCreator.FragmentationRay
    pub(crate) fn fragmentation_ray(&mut self, index: u32, ex: ExplosionParameters) {
        let force = ex.FragmentForce;
        // 360f * ((float)index / (float)rayCount) * ((float)Math.PI / 180f), in double.
        let f = (360.0f64 * (index as f32 as f64 / ex.FragmentationRayCount as f32 as f64) * mathf::DEG2RAD as f64) as f32;
        let direction = [(f as f64).cos() as f32, (f as f64).sin() as f32];
        let origin = V::new(ex.Position[0], ex.Position[1]);
        let Some(hit) = self.raycast_first(origin, direction, ex.Range) else { return };
        self.broadcast_on_fragment_hit(hit.collider, hit.game_object, force, true);
        let position = self.collider_position(hit.collider);
        if let Some(rb) = self.rigidbody_of_body(hit.body) {
            let n = normalized3([position[0] - ex.Position[0], position[1] - ex.Position[1], position[2] - ex.Position[2]]);
            rb.add_impulse_at_position(&mut self.world, [n[0] * force, n[1] * force], [hit.point.x, hit.point.y]);
        }
        // transform.CompareTag("Limb"): the limbs, but not the Human's feet (untagged in its prefab).
        let Some(l) = self.physical_of_collider(hit.collider).and_then(|p| self.limbs.iter().position(|l| l.PhysicalBehaviour == p)) else { return };
        if !self.limb_tagged(l) {
            return;
        }
        let kind = if self.random.value() > 0.5 { super::misc::damage_type::BULLET } else { super::misc::damage_type::BLUNT };
        let intensity = force * self.random.range_int(3, 8) as f32;
        self.skin_add_damage_point(l, kind, [hit.point.x, hit.point.y], intensity);
        self.limb_damage(l, force * 1.5);
        if self.limbs[l].SpeciesIdentity != "Android" && self.random.value() < ex.DismemberChance {
            if self.prefs.ProceduralFragments && hit.distance < 3.0 && (self.random.value() as f64) < 0.25 {
                // limb.StartCoroutine(Utils.DelayCoroutine(0.01f, limb.Crush)).
                let owner = self.limbs[l].id;
                self.start_wait(owner, 0.01, super::destroyable::Step::LimbDelayedCrush(l));
            } else {
                self.limb_slice(l);
            }
        }
    }

    /// BroadcastMessage("OnFragmentHit", force) on the collider's GameObject: it and its active descendants, each
    /// object's components in order (DestroyableBehaviour, DamagableMachineryBehaviour and ExplosiveBehaviour are the
    /// receivers among the rewritten scripts).
    /// SendMessage("OnFragmentHit", force) on a collider's GameObject: its components only.
    pub(super) fn send_on_fragment_hit(&mut self, collider: i32, force: f32) {
        let game_object = self.world.collider_game_object(collider).expect("a live collider");
        self.broadcast_on_fragment_hit(collider, game_object, force, false);
    }

    fn broadcast_on_fragment_hit(&mut self, collider: i32, game_object: i32, force: f32, descend: bool) {
        let Some(p) = self.physical_of_collider(collider) else {
            let bare = &self.bare_colliders[&collider];
            if bare.receives(&["DestroyableBehaviour", "DamagableMachineryBehaviour", "ExplosiveBehaviour", "TyreDeflationBehaviour", "GlowtubeBehaviour", "BulbBehaviour", "CathodeLightBehaviour"])
                || bare.scripts.iter().any(|s| super::syringe::is_syringe(s))
                || bare.scripts.iter().any(|s| super::translated::receives(s, "OnFragmentHit"))
            {
                unimplemented!("mod-only: BroadcastMessage OnFragmentHit to the scripts of a collider without a PhysicalBehaviour");
            }
            return;
        };
        // A recording's limbs and map take no OnFragmentHit (they are on no clone); a clone's objects (an item spawned
        // beside the recorded human, an fn-check's) do.
        let id = self.physicals[p].id;
        if self.is_recording() && !self.unsynced.iter().chain(self.retained.iter()).any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            return;
        }
        let receivers: Vec<Behaviour> = self.with_component_instance(id, |g, inst, _| {
            let start = inst.objects.iter().position(|x| x.game_object == game_object).expect("the collider's GameObject");
            let mut out = Vec::new();
            let below = if descend { inst.descendants(start) } else { Vec::new() };
            for x in std::iter::once(start).chain(below) {
                if !inst.objects[x].active {
                    continue;
                }
                let added = g.explosives.iter().enumerate().filter(|(_, s)| s.added && s.physical.is_some_and(|p| inst.objects[x].components.contains(&g.physicals[p].id))).map(|(e, _)| e);
                let added: Vec<i32> = added.map(|e| g.explosives[e].id).collect();
                for k in 0..inst.objects[x].components.len() + added.len() {
                    let cid = inst.objects[x].components.get(k).copied().unwrap_or_else(|| added[k - inst.objects[x].components.len()]);
                    if let Some(&(_, b)) = g.behaviour_of.get(&cid) {
                        if matches!(b, Behaviour::Destroyable(_) | Behaviour::Machinery(_) | Behaviour::Explosive(_) | Behaviour::Tyre(_) | Behaviour::Glowtube(_) | Behaviour::CathodeLight(_) | Behaviour::Bulb(_) | Behaviour::Syringe(_) | Behaviour::Balloon(_))
                            || matches!(b, Behaviour::Translated(t) if super::translated::receives(t.class(), "OnFragmentHit"))
                        {
                            out.push(b);
                        }
                    }
                }
            }
            out
        });
        for b in receivers {
            match b {
                Behaviour::Destroyable(x) => self.destroyable_on_fragment_hit(x, force),
                Behaviour::Machinery(m) => self.machinery_on_fragment_hit(m, force),
                Behaviour::Explosive(e) => self.explosive_on_fragment_hit(e),
                Behaviour::Tyre(t) => self.tyre_on_fragment_hit(t, force),
                // GlowtubeBehaviour.OnFragmentHit
                Behaviour::Glowtube(g) => self.glowtube_break(g),
                // CathodeLightBehaviour.OnFragmentHit
                Behaviour::CathodeLight(c) => self.cathode_light_break(c),
                Behaviour::Bulb(b) => self.bulb_on_fragment_hit(b),
                // SyringeBehaviour.OnFragmentHit
                Behaviour::Syringe(s) => self.syringe_on_fragment_hit(s),
                // BalloonBehaviour.OnFragmentHit: Pop.
                Behaviour::Balloon(b) => self.balloon_on_fragment_hit(b),
                Behaviour::Translated(t) => self.tr_on_fragment_hit(t, force),
                _ => unreachable!(),
            }
        }
    }

    /// Instantiate(Resources.Load<GameObject>(path), position, identity); the clone's first component id.
    pub(super) fn spawn_resource(&mut self, path: &str, position: [f32; 3]) -> i32 {
        let db = self.assets.take().expect("Instantiate without the game's data");
        let source = db.resource(path, 1).ok().flatten().unwrap_or_else(|| panic!("Resources.Load({path}) found no GameObject"));
        self.assets = Some(db);
        self.spawn_prefab(source, position)
    }

    /// Object.Instantiate(prefab, position, identity). Returns the clone root's Transform id.
    pub(super) fn spawn_prefab(&mut self, source: ObjRef, position: [f32; 3]) -> i32 {
        self.spawn_prefab_rotated(source, position, [0.0, 0.0, 0.0, 1.0])
    }

    /// Object.Instantiate(prefab, position, rotation). Returns the clone root's Transform id.
    pub(super) fn spawn_prefab_rotated(&mut self, source: ObjRef, position: [f32; 3], rotation: [f32; 4]) -> i32 {
        null_instantiate(source);
        let mut db = self.assets.take().expect("Instantiate without the game's data");
        let outer = std::mem::replace(&mut self.effect_spawn, true);
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(&mut db, &prefab, position, rotation, None));
        self.effect_spawn = outer;
        self.assets = Some(db);
        let (inst, _) = spawned.unwrap_or_else(|e| panic!("Instantiate: {e}"));
        let id = inst.objects[0].components[0];
        self.unsynced.push(inst);
        self.log_effect(source, position, rotation, 1.0, None, Some(id));
        id
    }

    /// Object.Instantiate(prefab, position, identity) during a spawn's Awake, with the data the spawn holds (`db`), or
    /// with `assets` when the data is in place.
    pub(super) fn spawn_prefab_with(&mut self, db: Option<&mut crate::assets::db::AssetDb>, source: ObjRef, position: [f32; 3]) -> i32 {
        null_instantiate(source);
        let Some(db) = db else { return self.spawn_prefab(source, position) };
        let rotation = [0.0, 0.0, 0.0, 1.0];
        let outer = std::mem::replace(&mut self.effect_spawn, true);
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(db, &prefab, position, rotation, None));
        self.effect_spawn = outer;
        let (inst, _) = spawned.unwrap_or_else(|e| panic!("Instantiate: {e}"));
        let id = inst.objects[0].components[0];
        self.unsynced.push(inst);
        self.log_effect(source, position, rotation, 1.0, None, Some(id));
        id
    }

    /// For the presentation's particles: `effect_log` fills only while something drains it.
    /// The effect just logged is a pooled clone.
    pub(super) fn tag_pooled(&mut self, p: super::Pooled) {
        if !self.effects_off {
            self.effect_log.last_mut().expect("the clone's effect").pooled = Some(p);
        }
    }

    pub(super) fn log_effect(&mut self, prefab: ObjRef, position: [f32; 3], rotation: [f32; 4], scale: f32, color: Option<[f32; 4]>, root: Option<i32>) {
        if !self.effects_off {
            self.effect_log.push(super::EffectSpawn { prefab, position, rotation, scale, color, root, subtree: None, water: None, pooled: None, parent: None, off: Vec::new(), tint_all: false });
        }
    }

    /// AddComponent<DeleteAfterTime>() on the GameObject with Transform `key`, then Life = `life`: a new instance id, and
    /// Update from the next frame.
    pub(super) fn add_delete_after(&mut self, key: i32, life: f32) {
        let id = self.allocate_instance_id();
        self.delete_afters.push(DeleteAfter { id, Life: life, added_to: Some(key), ..Default::default() });
        let b = Behaviour::DeleteAfter(self.delete_afters.len() - 1);
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
    }

    /// DeleteAfterTime.enabled = true: nothing for a destroyed one or when it already is; else, on an active object,
    /// Behaviour::AddToManager.
    pub(super) fn enable_delete_after(&mut self, id: i32) {
        if self.destroyed_components.contains(&id) {
            return;
        }
        let i = self.delete_afters.iter().position(|d| d.id == id).expect("a DeleteAfterTime in the arena");
        if !std::mem::replace(&mut self.delete_afters[i].enabled, true) && self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            let (order, b) = self.behaviour_of[&id];
            self.behaviours.add(order, b);
        }
    }

    /// DeleteAfterTime.Update
    pub fn delete_after_update(&mut self, i: usize) {
        // An added one is not in its clone's component lists: it leaves with its destroyed GameObject here.
        if let Some(key) = self.delete_afters[i].added_to && self.destroyed_components.contains(&key) {
            self.behaviours.remove(Behaviour::DeleteAfter(i));
            self.behaviour_of.remove(&self.delete_afters[i].id);
            return;
        }
        let dt = self.time.deltaTime;
        let s = &mut self.delete_afters[i];
        s.time += dt;
        if s.time > s.Life {
            let (id, bolt) = (s.added_to.unwrap_or(s.id), s.ion_bolt);
            // OnEndOfLife.Invoke(): the persistent calls, then the runtime listeners.
            let own = self.delete_afters[i].id;
            self.invoke_event(own, "OnEndOfLife");
            if let Some(b) = bolt {
                for _ in 0..self.listen_count(self.ion_bolts[b].id, super::continuous::Ev::OnEndOfLife) {
                    self.ion_bolt_end_of_life(b);
                }
            }
            self.destroy_object(id);
        }
    }

    /// ExistInThermalVision.OnDestroy: OnThermalVisionToggle.RemoveListener(ThermalVisionToggled) on the thermal vision.
    pub fn exist_in_thermal_vision_on_destroy(&mut self, id: i32) {
        self.listen_remove(id, super::continuous::Ev::OnThermalVisionToggle);
    }

    /// ExistInDetailView.Start: SetActive(Global.main.ShowLimbStatus), and LimbStatusToggled's listener.
    pub fn exist_in_detail_view_start(&mut self, id: i32) {
        let on = self.ShowLimbStatus;
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, on));
        self.detail_views.push(id);
    }

    /// ExistInBeta.Start: SetActive(false) (1.28.41) or SetActive(true) (1.28.45) of the "Pre-release build text" label under
    /// Canvas in Main.unity and Map Editor.unity, its only holders; the HUD draws it on 1.28.45 (view.rs pre_release_label).
    pub fn exist_in_beta_start(&mut self, id: i32) {
        let active = crate::assets::il::is_1_28_45();
        if !active {
            self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
    }

    /// ExistInThermalVision.Start, ExistInThermalVision.ThermalVisionToggled (`thermal_vision_toggle`) and ExistInThermalVision.OnDestroy (the destroyed_components check there):
    /// SetActive(ThermalVisionBehaviour.Instance.ThermalVisionEnabled) and the listener.
    pub fn exist_in_thermal_vision_start(&mut self, id: i32) {
        let on = self.thermal_vision.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.thermal_views.push(id);
        self.exist_in_thermal_vision_toggled(id);
    }

    /// ExistInThermalVision.ThermalVisionToggled: gameObject.SetActive(ThermalVisionEnabled), of the scene's thermal vision.
    pub fn exist_in_thermal_vision_toggled(&mut self, id: i32) {
        let on = self.thermal_vision.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        if id == super::thermal_display::CONTAINER {
            return self.thermal_display_set_active(on);
        }
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, on));
    }

    // ThermalVisionBehaviour.OnDestroy: the scene's object lives as long as the scene; play.rs `thermal_vision` drops the image with it.
    /// ThermalVisionBehaviour.Update: the toggle key, then (ThermalVisionEnabled) every 1/ThermalVisionUpdateRate s of unscaled
    /// time the 64x64 heat texture: for each texel its point TransformPoint(j/63 - 0.5, i/63 - 0.5) of the controller (which
    /// FitCameraBackgroundBehaviour keeps on the camera's view): a lava's LavaTemperature with alpha 255, else the
    /// PhysicalBehaviour of the collider in ToRead there (its Temperature, or the ambient without SimulateTemperature), else
    /// the ambient grid's at half alpha (AmbientTemperatureTransfer), else the ambient at alpha 0. play.rs draws it.
    /// FitCameraBackgroundBehaviour.Update (same GameObject, taken to run first): while the sprite went undrawn last frame the
    /// controller's position follows the camera.
    pub fn thermal_vision_update(&mut self) {
        let Some(enabled) = self.thermal_vision else { return };
        if self.thermal_texture.as_ref().is_some_and(|t| !t.visible) {
            let c = self.camera_world().0;
            self.thermal_texture.as_mut().unwrap().position = [c[0], c[1]];
        }
        if !self.input.ui_block && self.input.up("toggleThermalVision") {
            self.thermal_vision_toggle(enabled);
        }
        if self.thermal_vision != Some(true) {
            return;
        }
        let Some(mut tex) = self.thermal_texture.take() else { return };
        tex.t += self.time.unscaledDeltaTime;
        if tex.t > 1.0 / self.prefs.ThermalVisionUpdateRate as f32 {
            let mut num = 0;
            for i in 0..64 {
                for j in 0..64 {
                    let (num2, num3) = (i as f32 / 63.0, j as f32 / 63.0);
                    let vector = [tex.position[0] + tex.scale[0] * (num3 - 0.5), tex.position[1] + tex.scale[1] * (num2 - 0.5)];
                    let (heat, alpha) = if let Some(lv) = self.lava_at_point(vector) {
                        (tex.celsius_to_data(self.lavas[lv].LavaTemperature), 255)
                    } else if let Some(p) = self.overlap_point_first(vector, tex.to_read).and_then(|(p, _)| p).filter(|&p| self.physicals[p].in_world && !self.physicals[p].destroyed) {
                        let celsius = if self.physicals[p].SimulateTemperature { self.physicals[p].Temperature } else { self.AmbientTemperature };
                        (tex.celsius_to_data(celsius), get_alpha_for(self.AmbientTemperature, celsius, 1.0))
                    } else if self.prefs.AmbientTemperatureTransfer {
                        let t = self.GetTemperatureAtPoint(vector);
                        (tex.celsius_to_data(t), get_alpha_for(self.AmbientTemperature, t, 0.5))
                    } else {
                        (tex.celsius_to_data(self.AmbientTemperature), 0)
                    };
                    // ThermalVisionBehaviour.SetHeatAt: data[i * 2] = heat
                    // ThermalVisionBehaviour.SetAlphaAt: data[i * 2 + 1] = alpha
                    (tex.data[num * 2], tex.data[num * 2 + 1]) = (heat, alpha);
                    num += 1;
                }
            }
            // SetPixelData and Apply.
            tex.version += 1;
            tex.t = 0.0;
        }
        self.thermal_texture = Some(tex);
    }

    /// ThermalVisionBehaviour.ToggleThermalVision, ThermalVisionBehaviour.StartThermalVision and ThermalVisionBehaviour.StopThermalVision (SpriteRenderer.enabled is
    /// rendering), then OnThermalVisionToggle: ExistInThermalVision.ThermalVisionToggled on each live listener.
    pub(crate) fn thermal_vision_toggle(&mut self, enabled: bool) {
        let on = !enabled;
        self.thermal_vision = Some(on);
        for id in self.thermal_views.clone() {
            if id == super::thermal_display::CONTAINER {
                self.thermal_display_set_active(on);
            } else {
                for _ in 0..self.listen_count(id, super::continuous::Ev::OnThermalVisionToggle) {
                    self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, on));
                }
            }
        }
    }

    /// IgnoreParentSize.Start: without a parent the object is destroyed. Otherwise oldParent is the parent, the
    /// parent's ActOnDestroy (GetOrAddComponent) gets the object in DestroyWith (a new array, or appended when it is
    /// not there), the object's own ParentConstraint gets the parent as a source of weight 1 and constraintActive,
    /// then SetParent(null) and localScale = DesiredSize. A Start whose GameObject is inactive (ExistInDetailView.Start
    /// deactivates the Killzone with the detail view off) waits until the object is activated.
    pub fn ignore_parent_size_start(&mut self, id: i32) {
        let i = self.ignore_parent_sizes.iter().position(|x| x.id == id).expect("an IgnoreParentSize in the arena");
        if self.ignore_parent_sizes[i].started || self.destroyed_components.contains(&id) {
            return;
        }
        // A recorded clone's that ran (its record a root, parent=-2): SetParent(null) and the scale are in the record;
        // oldParent, the parent's ActOnDestroy and the constraint source are made here.
        if let Some(parent) = self.recorded_detached.remove(&id) {
            let key = self.with_component_instance(id, |_, inst, _| inst.objects[parent].components[0]);
            let source = match self.physicals.iter().position(|p| p.transform_id == key) {
                Some(p) => ConstraintSource::Physical(p),
                None => ConstraintSource::Object(key),
            };
            self.ignore_parent_size_started(i, key, source, None);
            return;
        }
        if !self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            self.ignore_parent_sizes[i].waiting = true;
            return;
        }
        self.ignore_parent_size_start_body(i);
    }

    /// The parent of IgnoreParentSize `id`'s object: its Transform id, the constraint source for it, and whether it
    /// is a parent in the object's own clone (false: the clone the object's clone is attached under).
    pub(crate) fn ignore_parent_size_parent(&mut self, id: i32) -> Option<(i32, ConstraintSource, bool)> {
        let (o, root, parent) = self.with_component_instance(id, |_, inst, o| (o, inst.objects[o].components[0], inst.objects[o].parent.map(|q| inst.objects[q].components[0])));
        let attached = if parent.is_none() && o == 0 { self.attached_parent_of(root) } else { None };
        match (parent, attached) {
            (Some(key), _) => match self.physicals.iter().position(|p| p.transform_id == key) {
                Some(p) => Some((key, ConstraintSource::Physical(p), true)),
                None => Some((key, ConstraintSource::Object(key), true)),
            },
            (None, Some((key, AttachParent::Physical(p)))) => Some((key, ConstraintSource::Physical(p), false)),
            (None, Some((key, AttachParent::Bare(_) | AttachParent::Object(_)))) => Some((key, ConstraintSource::Object(key), false)),
            (None, None) => None,
        }
    }

    /// IgnoreParentSize.Start's body (a direct call runs it on an inactive object too).
    pub(crate) fn ignore_parent_size_start_body(&mut self, i: usize) {
        let id = self.ignore_parent_sizes[i].id;
        self.ignore_parent_sizes[i].waiting = false;
        self.ignore_parent_sizes[i].started = true;
        let Some((key, source, own_parent)) = self.ignore_parent_size_parent(id) else {
            self.destroy_object(id);
            return;
        };
        self.ignore_parent_size_started(i, key, source, Some(own_parent));
    }

    /// Start's body from oldParent on; `own_parent` None: a recorded clone's, already detached and scaled.
    fn ignore_parent_size_started(&mut self, i: usize, key: i32, source: ConstraintSource, own_parent: Option<bool>) {
        let id = self.ignore_parent_sizes[i].id;
        self.ignore_parent_sizes[i].waiting = false;
        self.ignore_parent_sizes[i].started = true;
        let root = self.with_component_instance(id, |_, inst, o| inst.objects[o].components[0]);
        self.ignore_parent_sizes[i].oldParent = Some(source);
        // GetOrAddComponent<ActOnDestroy>
        self.act_on_destroy_get_or_add(key);
        let a = self.act_on_destroy.entry(key).or_default();
        if !a.destroy_with.contains(&root) {
            a.destroy_with.push(root);
        }
        // GetComponent<ParentConstraint>() (null: NullReferenceException at AddSource).
        let (pc, data) = self.with_component_instance(id, |_, inst, o| {
            let c = inst.prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::ParentConstraint(_)));
            let c = c.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            let Component::ParentConstraint(d) = &inst.prefab.objects[o].components[c].1 else { unreachable!() };
            (inst.objects[o].components[c], d.clone())
        });
        let mut constraint = ParentConstraint::from_data(pc, ConstrainedTarget::Object(pc), &data);
        constraint.add_source(source, 1.0);
        constraint.active = true;
        self.parent_constraints.push(constraint);
        let Some(own_parent) = own_parent else { return };
        if own_parent {
            self.set_parent_null_of(id, |_, _, _| {});
        } else {
            self.attached_clone_set_parent_null(root);
        }
        let [x, y] = self.ignore_parent_sizes[i].DesiredSize;
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, [x, y, 0.0]));
    }

    /// IgnoreParentSize.Update: Destroy(gameObject) once oldParent is destroyed.
    pub fn ignore_parent_size_update(&mut self, i: usize) {
        let s = &self.ignore_parent_sizes[i];
        let gone = match s.oldParent {
            None => return,
            Some(ConstraintSource::Physical(p)) => self.physicals[p].destroyed,
            Some(ConstraintSource::Object(key)) => self.destroyed_components.contains(&key),
        };
        if gone {
            self.destroy_object(s.id);
        }
    }

    /// GetComponent<IgnoreParentSize>() on the Killzone clone with root Transform `killzone` (null: NullReferenceException).
    pub(super) fn killzone_ignore_parent_size(&mut self, killzone: i32) -> usize {
        let ids = self.with_component_instance(killzone, |_, inst, o| inst.objects[o].components.clone());
        self.ignore_parent_sizes.iter().position(|x| ids.contains(&x.id)).unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }
}

/// IgnoreParentSize (the Killzone range circles).
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct IgnoreParentSize {
    /// The component's instance id.
    pub id: i32,
    pub DesiredSize: [f32; 2],
    /// oldParent: the parent's Transform (a PhysicalBehaviour's, or a clone object's by its Transform id).
    pub oldParent: Option<ConstraintSource>,
    pub started: bool,
    /// Start came while the GameObject was inactive: it runs at the next activation.
    pub waiting: bool,
}

impl Default for IgnoreParentSize {
    fn default() -> Self {
        Self {
            id: Default::default(),
            DesiredSize: [1.0; 2],
            oldParent: Default::default(),
            started: Default::default(),
            waiting: Default::default(),
        }
    }
}

/// Object.Instantiate(null): a null reference (path id 0) throws an ArgumentException.
pub(super) fn null_instantiate(source: ObjRef) {
    if source.path_id == 0 {
        crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null.");
    }
}

/// ThermalVisionBehaviour's heat texture (SetupTexture: 64x64 RG16, clamped) and the controller's Transform as
/// FitCameraBackgroundBehaviour sets it.
#[derive(Clone, Debug)]
pub struct ThermalTexture {
    pub min_temp: f32,
    pub max_temp: f32,
    pub to_read: u32,
    t: f32,
    /// data: heat and alpha per texel, rows from the bottom.
    pub data: Vec<u8>,
    /// Bumped at each Apply.
    pub version: u32,
    pub position: [f32; 2],
    pub scale: [f32; 2],
    /// SpriteRenderer.isVisible: drawn in the last frame.
    pub visible: bool,
    /// The SpriteRenderer's sharedMaterial (Unlit/ThermalVisionReplacementShader).
    pub material: Option<crate::assets::db::ObjRef>,
}

impl Default for ThermalTexture {
    fn default() -> Self {
        Self {
            min_temp: -273.0,
            max_temp: 1000.0,
            to_read: Default::default(),
            t: Default::default(),
            data: Default::default(),
            version: Default::default(),
            position: Default::default(),
            scale: Default::default(),
            visible: Default::default(),
            material: Default::default(),
        }
    }
}

impl ThermalTexture {
    pub fn new(min_temp: f32, max_temp: f32, to_read: u32, position: [f32; 2], scale: [f32; 2]) -> Self {
        ThermalTexture { min_temp, max_temp, to_read, t: 0.0, data: vec![0; 8192], version: 0, position, scale, visible: false, material: None }
    }

    /// ThermalVisionBehaviour.CelsiusToData: (byte)RoundToInt(MapRange(MinTemp, MaxTemp, 0, 255, Clamp(celsius, MinTemp, MaxTemp))).
    pub(crate) fn celsius_to_data(&self, celsius: f32) -> u8 {
        let c = if celsius < self.min_temp { self.min_temp } else if celsius > self.max_temp { self.max_temp } else { celsius };
        let v = super::mathf::map_range(self.min_temp, self.max_temp, 0.0, 255.0, c);
        // RoundToInt of NaN is int.MinValue, whose low byte is 0, as `as i32` gives.
        (v as f64).round_ties_even() as i32 as u8
    }
}

/// ThermalVisionBehaviour.GetAlphaFor: (byte)(255 * multiplier * Clamp01(|AmbientTemperature - Celsius| / 5)).
pub(crate) fn get_alpha_for(ambient: f32, celsius: f32, multiplier: f32) -> u8 {
    // The product stays in double (Mono rounds only at stores); (byte) is conv.u1: cvttsd2si, then the low byte (a negative alpha wraps).
    let t = ((ambient - celsius).abs() / 5.0).clamp(0.0, 1.0);
    super::translated::rt::to_int(255.0 * multiplier as f64 * t as f64) as u8
}

#[cfg(test)]
mod thermal_texture_tests {
    use super::*;

    #[test]
    fn heat_and_alpha_bytes() {
        // Main.unity's MinTemp -100, MaxTemp 1000.
        let t = ThermalTexture::new(-100.0, 1000.0, 0, [0.0; 2], [1.0; 2]);
        assert_eq!(t.celsius_to_data(-500.0), 0);
        assert_eq!(t.celsius_to_data(2000.0), 255);
        // 120 / 1100 * 255 = 27.818 -> 28.
        assert_eq!(t.celsius_to_data(20.0), 28);
        assert_eq!(t.celsius_to_data(f32::NAN), 0);
        assert_eq!(get_alpha_for(20.0, 22.0, 1.0), 102);
        assert_eq!(get_alpha_for(20.0, 37.0, 0.5), 127);
    }
}

/// ExplosionCreator.hitBuffer: static Collider2D[64] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const HIT_BUFFER: usize = 64;

/// ExplosiveBehaviour.losBuffer: static RaycastHit2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const EXPLOSIVE_LOS_BUFFER: usize = 8;

impl Game {
    /// ExplosiveBehaviour.Awake: physicalBehaviour = GetComponent<PhysicalBehaviour>(); BlockingLayers 0 takes
    /// LayerMask.GetMask("Objects", "CollidingDebris", "Bounds").
    pub fn explosive_awake(&mut self, i: usize) {
        self.explosives[i].physicalBehaviour = self.explosives[i].physical;
        if self.explosives[i].BlockingLayers == 0 {
            self.explosives[i].BlockingLayers = 10752;
        }
    }
}
