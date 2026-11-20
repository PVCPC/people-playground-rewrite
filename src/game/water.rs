//! WaterBehaviour and WaterBehaviourManager: the water volumes (their trigger collider, buoyancy and drag on
//! every physical object, the temperature states) and the IsUnderWater flag they keep.
use super::misc::{ObjectPool, PendingStart, SlotSet};
use super::{d, mathf, script, BodyOwner, Game};
use crate::assets::db::{AssetDb, Component, ObjRef, Prefab};
use crate::assets::mono::Value;
use std::collections::HashMap;
use std::sync::Arc;
use crate::b2::{math::Vec2 as V, world::FixtureId};
use crate::unity::transform::{global_position, Trs};

/// RefInt.Increment: Value++ (int arithmetic wraps).
pub(crate) fn ref_int_increment(v: &mut i32) {
    *v = v.wrapping_add(1);
}
/// RefInt.Decrement: Value--.
pub(crate) fn ref_int_decrement(v: &mut i32) {
    *v = v.wrapping_sub(1);
}

script! {
    Water {
        Drag: f32 = 0.0,
        Buoyancy: f32 = 0.0,
        LocalSurfaceLevel: f32 = 0.0,
        FreezingPointCelsius: f32 = 0.0,
        BoilingPointCelsius: f32 = 100.0,
    }
    links {
        /// The component's instance id, and its (object, component) in the clone's prefab.
        id: i32 = 0,
        object: usize = 0,
        component: usize = 0,
        enabled: bool = false,
        /// GameObject.activeInHierarchy.
        active: bool = false,
        /// The object's own Transform, then each parent's, for TransformPoint (water never moves).
        chain: Vec<Trs> = Vec::new(),
        /// The Trigger collider's own Transform, then each parent's (Collider2D.bounds' z is its position).
        trigger_chain: Vec<Trs> = Vec::new(),
        /// The Trigger collider's instance id and fixtures.
        trigger: i32 = 0,
        trigger_fixtures: Vec<FixtureId> = Vec::new(),
        /// The Ice PhysicalBehaviour (its `physicals` index) and its object in the prefab.
        ice: usize = 0,
        ice_object: usize = 0,
        /// GetComponent<SpriteRenderer>(), in `Game::renderers` (None: no sprite drawn).
        renderer: Option<usize> = None,
        /// BoilingWaterSound and ObjectsToBeActiveIfWater, as prefab objects.
        boiling_sound: usize = 0,
        objects_to_be_active: Vec<usize> = Vec::new(),
        /// waterSizzles.Length (Resources.LoadAll<AudioClip>("Audio/water_sizzle")).
        waterSizzles: usize = 0,
        /// The pool of its Splash prefab (`Game::splash_pools`); None when Splash is null.
        splash_pool: Option<usize> = None,
        /// wasFrozen and frozenInIce (a HashSet<PhysicalBehaviour>, in slot order), as `physicals` indices.
        wasFrozen: bool = false,
        frozen_in_ice: SlotSet<usize> = SlotSet::default(),
        /// currentlyInWater[0..currentlyInWaterCount), as `physicals` indices.
        currentlyInWater: Vec<usize> = Vec::new(),
    }
}

/// A FrozenInIceBehaviour a water added to a physical object.
#[derive(Clone, Debug)]
pub struct FrozenInIce {
    pub physical: usize,
    /// The SkinMaterialHandler on the same object (its limb) and the ShouldRot it had.
    pub skin: Option<usize>,
    pub shouldRotOriginal: bool,
    /// A pending Object.Destroy(component): the DelayedCallManager time it runs at.
    pub destroy_at: Option<f64>,
    pub destroyed: bool,
}

/// TagManager's custom tag "Poolable" (tags[8]).
pub(super) const POOLABLE_TAG: u16 = 20008;

/// A Splash clone of an ObjectPoolBehaviour: gameObject.activeSelf. Its root ParticleSystem's stop (EndUpdateAll,
/// stopAction Callback) returns it through PoolableParticleSystemHelper.
#[derive(Clone, Debug)]
pub struct SplashItem {
    pub active: bool,
    /// Destroyed by ObjectPoolBehaviour.Clear.
    pub destroyed: bool,
    /// Each prefab object's activeInHierarchy in this clone (after its ActivateIfFancyEffects).
    pub on: Vec<bool>,
    /// GetComponentsInChildren<ParticleSystem>().Count of the clone (after its ActivateIfFancyEffects): each gets a
    /// Random.Range in DoSplash.
    pub particle_systems: usize,
}

/// The ObjectPoolBehaviour PoolGenerator makes for a Splash prefab (once it is first requested), and what the prefab
/// holds that the simulation reads.
#[derive(Clone, Debug)]
pub struct SplashPool {
    pub source: ObjRef,
    pub prefab: Arc<Prefab>,
    pub original_ids: Arc<HashMap<ObjRef, i32>>,
    /// The prefab root's tag is Poolable.
    pub poolable: bool,
    /// ExplosionSoundBehviour.Play's `Near.PickRandom()` draw: Near.Length, when the behaviour is enabled and has an
    /// AudioSource.
    pub sound: Option<usize>,
    /// The ParticleSystems on each prefab object.
    pub particle_systems: Vec<usize>,
    /// The ActivateIfFancyEffects on active objects, in Awake order, with their Chance: each Awake of a clone whose
    /// ancestors stayed active draws Random.value and deactivates the object unless `Chance > value && FancyEffects`.
    pub fancy: Vec<(usize, f32)>,
    /// PoolGenerator.Ensure ran.
    pub ensured: bool,
    pub pool: ObjectPool,
    pub items: Vec<SplashItem>,
}

/// PoolGenerator.PoolSize
const POOL_SIZE: usize = 256;
/// ParticleSystem class id.
pub(super) const PARTICLE_SYSTEM: i32 = 198;

impl Game {
    /// The pool of the Splash prefab `source`, read from the data the first time a water names it.
    pub fn splash_pool_for(&mut self, db: &mut AssetDb, source: ObjRef) -> Result<usize, String> {
        if let Some(i) = self.splash_pools.iter().position(|p| p.source == source) {
            return Ok(i);
        }
        let prefab = db.prefab(source)?;
        let root = &prefab.objects[0];
        let sound = root.components.iter().find_map(|(_, c)| match c {
            Component::Script { class, enabled, fields, .. } if class == "ExplosionSoundBehviour" => {
                let source_set = fields.get("audioSource").and_then(Value::pptr).is_some_and(|p| !p.is_null());
                let has_source = source_set || root.components.iter().any(|(_, c)| matches!(c, Component::AudioSource { .. }));
                // DistantSound (SoundType.None = 0) is checked in Play too.
                assert!(fields.get("DistantSound").and_then(Value::i64) == Some(0), "ExplosionSoundBehviour.DistantSound");
                (*enabled && has_source).then(|| fields.get("Near").and_then(Value::array).expect("Near").len())
            }
            _ => None,
        });
        let particle_systems = prefab.objects.iter().map(|ob| ob.components.iter().filter(|(_, c)| matches!(c, Component::Other(PARTICLE_SYSTEM))).count()).collect();
        let mut fancy = Vec::new();
        for (o, ob) in prefab.objects.iter().enumerate() {
            if ob.parent.is_some_and(|p| p > o) {
                unimplemented!("mod-only: {}: a prefab object before its parent (Awake order)", ob.name);
            }
            let aife = ob.components.iter().find_map(|(_, c)| match c {
                Component::Script { class, fields, .. } if class == "ActivateIfFancyEffects" => Some(fields.get("Chance").and_then(Value::f32).unwrap_or(1.0)),
                _ => None,
            });
            if let Some(chance) = aife.filter(|_| self.prefab_object_active(&prefab, o)) {
                fancy.push((o, chance));
            }
        }
        let poolable = root.tag == POOLABLE_TAG;
        // PoolableParticleSystemHelper.Awake sets the root ParticleSystem's stop action to Callback.
        let helper = root.components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class == "PoolableParticleSystemHelper"));
        if poolable && !helper && root.components.iter().any(|(_, c)| matches!(c, Component::Other(PARTICLE_SYSTEM))) {
            unimplemented!("mod-only: {}: a poolable effect without PoolableParticleSystemHelper (who returns it?)", root.name);
        }
        self.splash_pools.push(SplashPool {
            source,
            poolable,
            sound,
            particle_systems,
            fancy,
            original_ids: Arc::new(db.resource_load_order()?),
            prefab: Arc::new(prefab),
            ensured: false,
            pool: ObjectPool::default(),
            items: Vec::new(),
        });
        Ok(self.splash_pools.len() - 1)
    }

    /// activeInHierarchy of a prefab object.
    pub(super) fn prefab_object_active(&self, prefab: &Prefab, mut o: usize) -> bool {
        loop {
            if !prefab.objects[o].active {
                return false;
            }
            match prefab.objects[o].parent {
                Some(p) => o = p,
                None => return true,
            }
        }
    }

    /// PoolGenerator.RequestPrefab(Splash, position): ObjectPoolBehaviour.Request, returning the item.
    pub(crate) fn splash_request(&mut self, s: usize) -> Option<usize> {
        if !self.splash_pools[s].ensured {
            // Ensure: new GameObject(name + " pool").AddComponent<ObjectPoolBehaviour>() takes the GameObject's,
            // its Transform's and the behaviour's instance ids.
            let name = self.splash_pools[s].prefab.objects[0].name.clone();
            match self.take_recorded_pool(&name) {
                Some(pool) => self.splash_pools[s].pool = pool,
                None => {
                    for _ in 0..3 {
                        self.allocate_instance_id();
                    }
                }
            }
            self.splash_pools[s].ensured = true;
            self.behaviours.add(0, super::Behaviour::ObjectPool(super::pool::PoolRef::Splash(s)));
        }
        let pool = &mut self.splash_pools[s];
        pool.pool.inactivityTimer = 0.0;
        // availableObjects.Pop()
        if let Some(i) = pool.pool.available.pop() {
            // SetActive(true) replays the particle systems (playOnAwake); OnPoolableReinitialised is
            // ExplosionSoundBehviour.Start (Play).
            pool.items[i].active = true;
            let sound = pool.sound;
            if let Some(n) = sound {
                self.pick_random(n);
            }
            return Some(i);
        }
        if pool.pool.created.len() >= POOL_SIZE {
            return None;
        }
        // Instantiate(Prefab, position, identity); the position and rotation only place the particles and the sound.
        let (prefab, ids, sound) = (pool.prefab.clone(), pool.original_ids.clone(), pool.sound);
        self.instantiate_native(&prefab, [0.0; 3], [0.0, 0.0, 0.0, 1.0], &ids);
        // Awake: each ActivateIfFancyEffects still active in the hierarchy draws `Chance > Random.value` before it
        // reads FancyEffects; a parent comes before its children.
        let mut off = vec![false; prefab.objects.len()];
        let mut particle_systems = 0;
        for o in 0..prefab.objects.len() {
            off[o] = prefab.objects[o].parent.is_some_and(|p| off[p]);
            if let Some(&(_, chance)) = self.splash_pools[s].fancy.iter().find(|f| f.0 == o).filter(|_| !off[o]) {
                off[o] = !(chance > self.random.value() && self.prefs.FancyEffects);
            }
            if !off[o] && self.prefab_object_active(&prefab, o) {
                particle_systems += self.splash_pools[s].particle_systems[o];
            }
        }
        let on = (0..prefab.objects.len()).map(|o| !off[o] && self.prefab_object_active(&prefab, o)).collect();
        let pool = &mut self.splash_pools[s];
        pool.items.push(SplashItem { active: true, destroyed: false, particle_systems, on });
        let i = pool.items.len() - 1;
        pool.pool.created.push(i);
        // Start of the clone's ExplosionSoundBehviour (Play).
        if let Some(n) = sound {
            self.pending_starts.push(PendingStart::ExplosionSound(n));
        }
        Some(i)
    }

    /// WaterBehaviour.DoSplash after its gates: the pooled Splash at Trigger.bounds.ClosestPoint(collision's position),
    /// transform.up = Reflect((v.x, -|v.y|), up), and each of its ParticleSystems' shape.radius, startSizeMultiplier
    /// (with a Random.Range(1f, 1.4f)) and startSpeedMultiplier. The AudioSource volume is audio.
    pub(crate) fn water_splash(&mut self, w: usize, p: usize, collider: i32) {
        let Some(s) = self.waters[w].splash_pool else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        if !self.splash_pools[s].poolable {
            unimplemented!("mod-only: WaterBehaviour.DoSplash: Object.Instantiate of a Splash prefab that is not tagged Poolable");
        }
        // Trigger.bounds starts with the AutoSyncTransforms.
        self.sync_pending();
        let at = self.physicals[p].transform.position;
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        let mut shapes = 0;
        for fx in self.waters[w].trigger_fixtures.iter().filter_map(|&f| self.world.fixtures[f].as_ref()) {
            shapes += 1;
            for child in 0..fx.shape.child_count() {
                let a = fx.shape.compute_aabb_no_radius(&self.world.body(fx.body).xf, child);
                lo = [lo[0].min(a.lower.x), lo[1].min(a.lower.y)];
                hi = [hi[0].max(a.upper.x), hi[1].max(a.upper.y)];
            }
        }
        // Collider2D.bounds (GetBounds, 180c11150): the union's centre and extents, z from the collider's Transform (extent 0);
        // without shapes the Transform's position with zero extents. Bounds.ClosestPoint: the exact clamp.
        let mut trigger_at = [0.0, 0.0, 0.0, 0.0];
        for node in &self.waters[w].trigger_chain {
            trigger_at = global_position(node, trigger_at);
        }
        let (center, extents) = if shapes == 0 {
            ([trigger_at[0], trigger_at[1], trigger_at[2]], [0.0; 3])
        } else {
            ([(hi[0] + lo[0]) * 0.5, (hi[1] + lo[1]) * 0.5, trigger_at[2]], [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, 0.0])
        };
        let (closest, _) = crate::unity::transform::bounds_closest_point(at, center, extents);
        let Some(i) = self.splash_request(s) else { return };
        let v = self.physicals[p].rigidbody.velocity(&self.world);
        let z = (super::d(crate::game::mathf::atan2(v[1].abs(), v[0])) * 57.29578f32 - 90.0f32).f();
        let num = self.collider_bounds(p, collider).1[0].min(4.0);
        let mass = self.physicals[p].rigidbody.mass(&self.world);
        let speed = ((v[0] * v[0] + v[1] * v[1]).sqrt() * 0.5 * mass).clamp(0.2, 25.0);
        let prefab = self.splash_pools[s].prefab.clone();
        let mut sizes = Vec::new();
        for (o, ob) in prefab.objects.iter().enumerate() {
            if !self.splash_pools[s].items[i].on[o] {
                continue;
            }
            for _ in 0..self.splash_pools[s].particle_systems[o] {
                sizes.push((ob.game_object, (super::d(num) * 2.5f32 * self.random.range(1.0, 1.4)).f()));
            }
        }
        let source = self.splash_pools[s].source;
        self.log_effect(source, [closest[0], closest[1], closest[2]], super::physical::z_rotation(z), 1.0, None, None);
        if !self.effects_off {
            self.effect_log.last_mut().expect("the clone's effect").water = Some(super::WaterSplash { radius: num, speed, sizes });
        }
        self.tag_pooled(super::Pooled::Splash(s, i));
    }

    /// PoolableParticleSystemHelper.OnParticleSystemStopped: pool.Return(gameObject) (SetActive(false),
    /// availableObjects.Push) for a pooled effect whose root system ran its stopAction Callback (the particle systems'
    /// update after LateUpdate: a Splash's 5 s stop). The pool is always set (OnPoolableInitialised ran at Request).
    pub fn pooled_effect_stopped(&mut self, p: super::Pooled) {
        match p {
            super::Pooled::Splash(s, i) => {
                let pool = &mut self.splash_pools[s];
                if pool.items[i].destroyed {
                    return;
                }
                pool.items[i].active = false;
                pool.pool.available.push(i);
            }
            super::Pooled::Generic(s, i) => {
                let root = self.generic_pools[s].items[i].root;
                self.pool_return(s, root);
            }
            super::Pooled::BloodPuff(b) => {
                self.blood_puffs[b].active = false;
                let pool = self.blood_puff_pool.as_mut().expect("a requested blood puff's pool");
                pool.available.push(b);
            }
        }
    }

    /// WaterBehaviour.IsEvaporated
    pub(crate) fn water_is_evaporated(&self, w: usize) -> bool {
        self.AmbientTemperature > (d(self.waters[w].BoilingPointCelsius) + 100.0).f()
    }

    /// WaterBehaviour.IsFrozen
    pub(crate) fn water_is_frozen(&self, w: usize) -> bool {
        self.AmbientTemperature < self.waters[w].FreezingPointCelsius
    }

    /// WaterBehaviour.GetGlobalSurfaceLevel: transform.TransformPoint(0, LocalSurfaceLevel).y
    pub(crate) fn water_global_surface_level(&self, w: usize) -> f32 {
        let mut v = [0.0, self.waters[w].LocalSurfaceLevel, 0.0, 0.0];
        for node in &self.waters[w].chain {
            v = global_position(node, v);
        }
        v[1]
    }

    /// WaterBehaviour.IsPointInsideWater: Trigger.OverlapPoint (Collider2D::OverlapPoint, 180c12440, starts with
    /// the AutoSyncTransforms and asks each of the collider's shapes).
    pub(crate) fn water_is_point_inside(&mut self, w: usize, point: [f32; 2]) -> bool {
        self.sync_pending();
        let p = V::new(point[0], point[1]);
        // A disabled trigger (its map deactivated by Clear everything, destroyed at the frame's end) has no shapes: Unity's
        // OverlapPoint is false there.
        self.waters[w].trigger_fixtures.iter().filter_map(|&f| self.world.fixtures[f].as_ref()).any(|fx| super::tool::test_point(&fx.shape, &self.world.body(fx.body).xf, p))
    }

    /// WaterBehaviour.GetWaterAtPoint
    pub(crate) fn water_at_point(&mut self, point: [f32; 2]) -> Option<usize> {
        for i in 0..self.water_list.len() {
            let w = self.water_list[i];
            if self.waters[w].active && self.water_is_point_inside(w, point) {
                return Some(w);
            }
        }
        None
    }

    /// WaterBehaviour.IsPointUnderWater
    pub fn is_point_under_water(&mut self, point: [f32; 2]) -> bool {
        for i in 0..self.water_list.len() {
            let w = self.water_list[i];
            if self.waters[w].active && !self.water_is_evaporated(w) && !self.water_is_frozen(w) && self.water_is_point_inside(w, point) {
                return true;
            }
        }
        false
    }

    // WaterBehaviour.OnDestroy: waters.Remove(this) (destroyable.rs: `water_list`).
    // WaterSoundBehaviour.Update: presentation (AudioSource.volume from the camera distance; the link skips the component, nothing reads the volume).
    // WaterSoundBehaviour.constructor: field initialisers (SoftRange 20) of that presentation-only component.
    // WaterBehaviour.IgnoreObject: Ignore.Add and Physics2D.IgnoreCollision; it has no caller anywhere, so Ignore stays empty.
    // WaterBehaviour.StopIgnoringObject: the same, Ignore.Remove of an empty set.
    // WaterBehaviour.GetSubmergedObjects: water_submerged_objects (the acid pool reads currentlyInWater lazily, as the game's iterator).
    // WaterBehaviour.OnTriggerExit2D: the same DoSplash gate as OnTriggerEnter2D (water_on_trigger).
    /// WaterBehaviour.ShouldUpdate: active in the hierarchy and enabled, the test in water_manager_fixed_update.
    pub fn water_should_update(&self, w: usize) -> bool {
        self.waters[w].active && self.waters[w].enabled
    }
    // WaterBehaviour.ManagedUpdate: empty.
    // WaterBehaviour.ManagedLateUpdate: empty.
    /// WaterBehaviour.OnWillRenderObject for every drawn water: `mat` (SpriteRenderer.material, the renderer's own copy) takes
    /// _Temperature = MapRange(FreezingPointCelsius, BoilingPointCelsius, 0, 100, AmbientTemperature).
    pub fn waters_will_render(&mut self) {
        for w in 0..self.waters.len() {
            let Some(r) = self.waters[w].renderer else { continue };
            if !self.will_render_dispatch(self.waters[w].id, self.sprite_drawn(super::visual::SpriteAt::Renderer(r))) {
                continue;
            }
            let s = &self.waters[w];
            let t = mathf::map_range(s.FreezingPointCelsius, s.BoilingPointCelsius, 0.0, 100.0, self.AmbientTemperature);
            self.set_material_prop(super::visual::SpriteAt::Renderer(r), "_Temperature", super::props::Prop::Float(t));
        }
    }
    // WaterBehaviourManager.GetCollection: WaterBehaviour.waters, `water_list`.
    /// WaterBehaviour.Awake: waters.Add(this). (OnDestroy's Remove: destroyable.rs.)
    pub fn water_awake(&mut self, w: usize) {
        self.water_list.push(w);
    }

    /// WaterBehaviour.Start: SpriteRenderer = GetComponent (an engine reference), waterSizzles (loaded at link) and
    /// UpdateTemperatureState.
    pub fn water_start(&mut self, w: usize) {
        self.water_update_temperature_state(w);
    }

    /// A recording holds no map clone: each WaterBehaviour of the recorded map (header `map X`) that is active in the
    /// map is set up as its Awake (waters.Add) and Start left it, on the recorded Trigger collider (found by its object's
    /// name). Start's UpdateTemperatureState ran before the recording: its Ice object is off and unrecorded, so a
    /// recording that freezes the water (or any SetActive through the water) has no clone to work on.
    pub(super) fn recorded_map_scripts(&mut self, header: &[String], first: &super::dump::Snapshot) {
        // A linked map clone (Clone records) made these scripts already: twice, a water marks its bodies twice.
        if self.map_root.is_some() || !header.iter().any(|l| l.starts_with("map ")) {
            return;
        }
        let had = self.assets.is_some();
        let game = crate::game_assets::find_game(None).expect("the game's data");
        self.assets.get_or_insert_with(|| Box::new(AssetDb::open(&game).expect("the game's data")));
        let mut db = self.assets.take().unwrap();
        let prefab = super::particle_forces::recorded_map_prefab(&mut db, header).unwrap();
        let trs = |o: usize| {
            let Some((_, Component::Transform(t))) = prefab.objects[o].components.first() else { unreachable!("a Transform first") };
            let v = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
            Trs { t: v(t.local_position), q: t.local_rotation, s: v(t.local_scale) }
        };
        // The object's own Transform, then each parent's (the map is instantiated at the origin, unrotated).
        let chain = |mut o: usize| {
            let mut c = vec![trs(o)];
            while let Some(p) = prefab.objects[o].parent {
                c.push(trs(p));
                o = p;
            }
            c
        };
        let active = |mut o: usize| {
            let mut a = prefab.objects[o].active;
            while let Some(p) = prefab.objects[o].parent {
                a &= prefab.objects[p].active;
                o = p;
            }
            a
        };
        // The map clone's instance ids (Instantiate's numbering, as play gives them): `rel[o]` is object o's GameObject id
        // less the root's; component j's is 2 * (1 + j) below its GameObject's.
        let rel = map_id_offsets(&prefab);
        let root_id = map_root_id(self, &prefab, &rel);
        // Without one (no map object alone with its name has its collider recorded) the map's ids can't be known.
        let comp_id = |o: usize, j: usize| root_id.map(|r| r + rel[o] - 2 * (1 + j as i32)).unwrap_or_else(|| unimplemented!("recording harness: no recorded map collider to number the map's instance ids from"));
        let is_collider = |o: usize, j: usize| matches!(prefab.objects[o].components[j].1, Component::Collider2D(_));
        // A map object's Trigger collider (component tc of object to): its fixtures and collider id. By its object's name
        // when that object has one collider and the recording one id; else (not in the recording: off in the b2 world;
        // or several colliders on it) by the map's numbering.
        let recorded_trigger = |g: &Game, to: usize, tc: usize| {
            let name = prefab.objects[to].name.replace(' ', "_");
            let one = (0..prefab.objects[to].components.len()).filter(|&j| is_collider(to, j)).count() == 1;
            let by_name = g.objects.get(&name).map(|&go| g.world.fixtures.iter().flatten().filter(|x| x.collider.game_object == go).map(|x| x.collider.id).collect::<std::collections::BTreeSet<i32>>());
            let id = match by_name.map(|s| s.into_iter().collect::<Vec<_>>()).as_deref() {
                Some(&[id]) if one => id,
                _ => comp_id(to, tc),
            };
            let fixtures: Vec<FixtureId> = (0..g.world.fixtures.len()).filter(|&f| g.world.fixtures[f].as_ref().is_some_and(|x| x.collider.id == id)).collect();
            (fixtures, id)
        };
        // The script's record in the first snapshot (a recorder without map scripts has none: its unrecorded state stays
        // as Start left it).
        let record = |class: &'static str, o: usize| first.of(class).find(|r| r.name == prefab.objects[o].name.replace(' ', "_"));
        let hex = |t: &str| f32::from_bits(u32::from_str_radix(t, 16).unwrap_or_else(|_| panic!("bad float {t}")));
        // Behaviour::AddToManager in the map's spawn order, ahead of everything spawned after the map.
        let mut managed = Vec::new();
        for o in 0..prefab.objects.len() {
            for (c, (_, x)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Script { class, enabled, fields: v, execution_order, .. } = x else { continue };
                let (class, enabled, order) = (class.as_str(), *enabled, *execution_order);
                if !matches!(class, "WaterBehaviour" | "LavaBehaviour" | "RandomPrefabSpawnerBehaviour" | "TemperatureTargetRectangleBehaviour") {
                    continue;
                }
                // On an inactive object no Awake ran (WaterBehaviour.Awake's waters.Add, Behaviour.AddToManager): the
                // script is not in play. No shipped map has one, and no recording activates map objects on this path.
                if !active(o) {
                    continue;
                }
                let file = prefab.objects[o].game_object.file;
                let at = |name: &str| {
                    let pp = v.get(name).and_then(Value::pptr).filter(|pp| !pp.is_null() && pp.file == 0).unwrap_or_else(|| panic!("{class}.{name} outside the map or null"));
                    *prefab.index.get(&ObjRef { file, path_id: pp.path_id }).unwrap_or_else(|| panic!("{class}.{name} outside the map"))
                };
                match class {
                    "WaterBehaviour" => {
                        let mut w = Water::default();
                        super::spawn::load_values(w.fields(), v).expect("WaterBehaviour's fields");
                        let (to, tc) = at("Trigger");
                        (w.trigger_fixtures, w.trigger) = recorded_trigger(self, to, tc);
                        // The script's id from its Trigger's, by the map's numbering (on one object: two apart per component).
                        w.id = w.trigger + (rel[o] - 2 * c as i32) - (rel[to] - 2 * tc as i32);
                        (w.object, w.component, w.enabled, w.active) = (o, c, enabled, true);
                        (w.chain, w.trigger_chain) = (chain(o), chain(to));
                        // Off since Start and not recorded: only a frozen water reads it.
                        w.ice_object = at("Ice").0;
                        w.ice = usize::MAX;
                        w.boiling_sound = at("BoilingWaterSound").0;
                        for x in v.get("ObjectsToBeActiveIfWater").and_then(Value::array).expect("WaterBehaviour.ObjectsToBeActiveIfWater") {
                            let pp = x.pptr().expect("a GameObject reference");
                            w.objects_to_be_active.push(prefab.index[&ObjRef { file, path_id: pp.path_id }].0);
                        }
                        if let Some(r) = v.get("Splash").and_then(Value::pptr).and_then(|pp| db.resolve(file, pp)) {
                            w.splash_pool = Some(self.splash_pool_for(&mut db, r).expect("the water's Splash prefab"));
                        }
                        w.waterSizzles = db.resources_in("Audio/water_sizzle", 83).expect("Audio/water_sizzle").len();
                        self.waters.push(w);
                        self.water_awake(self.waters.len() - 1);
                    }
                    "LavaBehaviour" => {
                        let mut lava = super::scenery::Lava::default();
                        super::spawn::load_values(lava.fields(), v).expect("LavaBehaviour's fields");
                        let (to, tc) = at("Trigger");
                        lava.trigger_fixtures = recorded_trigger(self, to, tc).0;
                        let filter = v.get("ContactFilter").expect("LavaBehaviour.ContactFilter");
                        let flag = |name: &str| filter.get(name).and_then(Value::bool).unwrap_or_else(|| panic!("ContactFilter.{name}"));
                        if flag("useDepth") || flag("useOutsideDepth") || flag("useNormalAngle") || flag("useOutsideNormalAngle") {
                            unimplemented!("mod-only: LavaBehaviour.ContactFilter depth or normal angle filtering");
                        }
                        if flag("useLayerMask") {
                            lava.layer_mask = filter.get("layerMask").and_then(|m| m.get("m_Bits")).and_then(Value::i64).expect("layerMask.m_Bits") as u32;
                        }
                        if let Some(r) = v.get("ImpactPrefab").and_then(Value::pptr).and_then(|pp| db.resolve(file, pp)) {
                            lava.impact = Some(self.splash_pool_for(&mut db, r).expect("the lava's ImpactPrefab"));
                        }
                        if let Some(r) = record("LavaBehaviour", o) {
                            lava.lastImpactSplashTime = hex(r.get("lastImpactSplashTime").expect("lastImpactSplashTime"));
                            let victims = r.get("previousVictims").expect("previousVictims");
                            for e in victims[1..victims.len() - 1].split(';').filter(|e| !e.is_empty()) {
                                let (key, marked) = e.split_once(':').expect("a victim");
                                let id: i32 = key.rsplit_once('#').expect("a reference").1.parse().expect("an instance id");
                                let p = self.physicals.iter().position(|x| x.id == id).expect("a recorded victim");
                                lava.previousVictims.push(p, marked == "RefBool(Marked=1)");
                            }
                        }
                        managed.push((order, super::Behaviour::Lava(self.lavas.len())));
                        self.lavas.push(lava);
                    }
                    "RandomPrefabSpawnerBehaviour" => {
                        let mut s = super::scenery::Spawner::default();
                        super::spawn::load_values(s.fields(), v).expect("RandomPrefabSpawnerBehaviour's fields");
                        for p in v.get("Prefabs").and_then(Value::array).expect("RandomPrefabSpawnerBehaviour.Prefabs") {
                            s.prefabs.push(db.resolve(file, p.pptr().expect("a prefab reference")));
                        }
                        s.chain = chain(o);
                        if let Some(r) = record("RandomPrefabSpawnerBehaviour", o) {
                            s.t = hex(r.get("t").expect("t"));
                        }
                        managed.push((order, super::Behaviour::Spawner(self.spawners.len())));
                        self.spawners.push(s);
                    }
                    _ => {
                        let mut h = super::scenery::HeatRect::default();
                        super::spawn::load_values(h.fields(), v).expect("TemperatureTargetRectangleBehaviour's fields");
                        let ints = |name: &str| {
                            let x = v.get(name).unwrap_or_else(|| panic!("{name}"));
                            let c = |k: &str| x.get(k).and_then(Value::i64).unwrap_or_else(|| panic!("{name}.{k}")) as i32;
                            [c("x"), c("y"), c("z")]
                        };
                        (h.Size, h.Offset) = (ints("Size"), ints("Offset"));
                        let mut p = [0.0; 4];
                        for node in &chain(o) {
                            p = global_position(node, p);
                        }
                        h.position = [p[0], p[1], p[2]];
                        managed.push((order, super::Behaviour::HeatRect(self.heat_rects.len())));
                        self.heat_rects.push(h);
                    }
                }
            }
        }
        // The map's colliders without a PhysicalBehaviour are bare (link.rs registers them at the map's spawn): an
        // explosion or overlap reads their Transform. The recording names one object per name, so same-named objects'
        // colliders go by instance id, which counts down in the map's instantiation (prefab) order.
        let is_physical = |o: usize| prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Script { class, .. } if class == "PhysicalBehaviour"));
        // link.rs anchor_chain: the Transforms up to the nearest physical (the recorded one of that name), else the root.
        let anchor_chain = |g: &Game, o: usize| {
            let (mut chain, mut up) = (Vec::new(), Some(o));
            let anchor = loop {
                let Some(t) = up else { break super::BareAnchor::Root };
                if is_physical(t) {
                    let name = prefab.objects[t].name.replace(' ', "_");
                    let mut found = (0..g.physicals.len()).filter(|&p| g.physicals[p].name == name);
                    if let (Some(p), None) = (found.next(), found.next()) {
                        break super::BareAnchor::Physical(p);
                    }
                    // Same-named physicals: the one whose PhysicalBehaviour is a component of this object, by the map's numbering.
                    let own: Vec<i32> = (0..prefab.objects[t].components.len()).map(|j| comp_id(t, j)).collect();
                    let mut found = (0..g.physicals.len()).filter(|&p| own.contains(&g.physicals[p].id));
                    let (Some(p), None) = (found.next(), found.next()) else { unimplemented!("recording harness: the map physical {name} is not in the recording (re-record it)") };
                    break super::BareAnchor::Physical(p);
                }
                chain.push(trs(t));
                up = prefab.objects[t].parent;
            };
            (anchor, chain)
        };
        let colliders = |o: usize| prefab.objects[o].components.iter().filter(|(_, x)| matches!(x, Component::Collider2D(_))).count();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for o in (0..prefab.objects.len()).filter(|&o| colliders(o) > 0 && !is_physical(o)) {
            by_name.entry(prefab.objects[o].name.replace(' ', "_")).or_default().push(o);
        }
        let root = self.objects.get(&prefab.objects[0].name.replace(' ', "_")).map_or((0, 0), |&go| (go, 0));
        for (name, candidates) in by_name {
            let Some(&go) = self.objects.get(&name) else { continue };
            // Same-named objects (Hybrid's two Hitboxes): the fixtures carry each one's own GameObject id, from the map's
            // Clone rows; the name keeps only one.
            let gos: Vec<i32> = std::iter::once(go).chain(first.of("Clone").filter(|r| r.get("root") == Some("Map") && r.name == name).map(|r| r.id)).collect();
            let mut ids: Vec<i32> = self.world.fixtures.iter().flatten().filter(|x| gos.contains(&x.collider.game_object)).map(|x| x.collider.id).collect();
            ids.sort_unstable_by(|a, b| b.cmp(a));
            ids.dedup();
            ids.retain(|&c| self.physical_of_collider(c).is_none() && !self.bare_colliders.contains_key(&c));
            if ids.is_empty() {
                continue;
            }
            // Each candidate's colliders in component order: the recorded ids counting down, one each, when all are recorded;
            // else (an inactive one, off in the b2 world) each by the map's numbering, the unrecorded left out.
            let own: Vec<Vec<i32>> = if ids.len() == candidates.iter().map(|&o| colliders(o)).sum::<usize>() {
                let mut ids = ids.into_iter();
                candidates.iter().map(|&o| ids.by_ref().take(colliders(o)).collect()).collect()
            } else {
                candidates.iter().map(|&o| (0..prefab.objects[o].components.len()).filter(|&j| is_collider(o, j)).map(|j| comp_id(o, j)).filter(|c| ids.contains(c)).collect()).collect()
            };
            for (o, own) in candidates.into_iter().zip(own) {
                let (anchor, chain) = anchor_chain(self, o);
                let scripts: Vec<String> = prefab.objects[o].components.iter().filter_map(|(_, x)| if let Component::Script { class, .. } = x { Some(class.clone()) } else { None }).collect();
                let file = prefab.objects[o].game_object.file;
                let water_impact = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                    Component::Script { class, fields, .. } if class == "WaterImpactBehaviour" => fields.get("Splash").and_then(Value::pptr).and_then(|pp| db.resolve(file, pp)),
                    _ => None,
                });
                for c in own {
                    let bare = super::BareCollider { anchor, chain: chain.clone(), scripts: scripts.clone(), water_impact, active_self: prefab.objects[o].active, root };
                    self.bare_colliders.insert(c, bare);
                }
            }
        }
        // In reverse, each at the front of its execution order: the map's behaviours keep their order, ahead of the rest.
        for (order, b) in managed.into_iter().rev() {
            for (list, m) in [(&mut self.behaviours.fixed, "FixedUpdate"), (&mut self.behaviours.update, "Update"), (&mut self.behaviours.late, "LateUpdate")] {
                if super::declares(b.class(), m) {
                    let at = list.partition_point(|&(o, _)| o < order);
                    list.insert(at, (order, b));
                }
            }
        }
        if had {
            self.assets = Some(db);
        }
    }

    /// Runs `f` on the clone the water belongs to (still unsynced, or retained by the sync).
    fn with_water_instance<R>(&mut self, w: usize, f: impl FnOnce(&mut Game, &mut super::spawn::Instance) -> R) -> R {
        let (o, c, id) = (self.waters[w].object, self.waters[w].component, self.waters[w].id);
        let owns = |i: &super::spawn::Instance| i.objects.get(o).and_then(|x| x.components.get(c)) == Some(&id);
        if let Some(i) = self.unsynced.iter().position(owns) {
            let mut inst = self.unsynced.remove(i);
            self.clones_checked_out += 1;
            let r = f(self, &mut inst);
            self.clones_checked_out -= 1;
            self.unsynced.insert(i, inst);
            self.checked_in_pending();
            return r;
        }
        let i = self.retained.iter().position(owns).expect("the clone of a WaterBehaviour");
        let mut inst = self.retained.remove(i);
        self.clones_checked_out += 1;
        let r = f(self, &mut inst);
        self.clones_checked_out -= 1;
        self.retained.insert(i, inst);
        self.checked_in_pending();
        r
    }

    /// Runs `f` on the clone that holds the component `id` (still unsynced, or retained by the sync), with the
    /// index of its object.
    pub(crate) fn with_component_instance<R>(&mut self, id: i32, f: impl FnOnce(&mut Game, &mut super::spawn::Instance, usize) -> R) -> R {
        let owns = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
        let (list, i) = match self.unsynced.iter().position(owns) {
            Some(i) => (true, i),
            None => (false, self.retained.iter().position(owns).unwrap_or_else(|| panic!("component {id}: on no clone (a recording made before the recorder wrote its root's Clone records: re-record it)"))),
        };
        let mut inst = if list { self.unsynced.remove(i) } else { self.retained.remove(i) };
        let o = inst.objects.iter().position(|x| x.components.contains(&id)).unwrap();
        self.clones_checked_out += 1;
        let r = f(self, &mut inst, o);
        self.clones_checked_out -= 1;
        if list { self.unsynced.insert(i, inst) } else { self.retained.insert(i, inst) }
        self.checked_in_pending();
        r
    }

    /// What was asked while a clone was checked out, run now it is back: Object.Destroy calls, and the OnDisable of translated scripts.
    pub(super) fn checked_in_pending(&mut self) {
        // Taken once: an id whose clone is still checked out (a nested call) goes back on the list for the outer call.
        for (id, component) in std::mem::take(&mut self.destroy_disable_pending).into_iter().rev() {
            if component { self.destroy_disable_component(id) } else { self.destroy_disable_behaviours(id) }
        }
        while let Some(t) = self.tr_on_disable_pending.pop() {
            self.tr_on_disable(t);
        }
    }

    /// GameObject.SetActive on one of the water's prefab objects. SetActive(true) of an active object does nothing.
    fn water_set_active(&mut self, w: usize, o: usize, active: bool) {
        self.with_water_instance(w, |g, inst| g.set_active_scripts(inst, o, active));
    }

    /// GameObject.SetActive on object `o` of a clone, then the OnEnable or OnDisable of the scripts it passes.
    /// The awoken scripts a SetActive(true) passes, in queue order: a held-back Start, Behaviour::AddToManager and OnEnable.
    pub(super) fn script_enable_passes(&mut self, inst: &mut super::spawn::Instance, prefab: &Prefab, passes: &[(usize, usize)]) {
        for (n, &(d, k)) in passes.iter().enumerate() {
            let Component::Script { class, enabled, .. } = &prefab.objects[d].components[k].1 else { unreachable!() };
            let id = inst.objects[d].components[k];
            // A Start the object's deactivation held back is due now.
            while let Some(i) = self.deferred_starts.iter().position(|s| s.0 == id) {
                let start = self.deferred_starts.remove(i).1;
                self.pending_starts.push(start);
            }
            // Behaviour::AddToManager with the OnEnable.
            if let (Some(&(order, b)), true) = (self.behaviour_of.get(&id), self.script_enabled(id, *enabled)) {
                self.behaviours.add(order, b);
            }
            self.script_on_enable_arm(inst, d, class, enabled, id);
            // A PortalTraversingBehaviour was added to the object after its prefab's components: its OnEnable comes last.
            if passes.get(n + 1).is_none_or(|x| x.0 != d) {
                self.portal_traversing_script_pass(inst, d, true);
            }
        }
    }

    pub(super) fn set_active_scripts(&mut self, inst: &mut super::spawn::Instance, o: usize, active: bool) {
        let prefab = inst.prefab.clone();
        if active {
            // The colliders it creates read the live Transforms (Collider2D::CreateFixtures' relative matrix).
            self.refresh_clone_poses(inst);
            let passes = self.set_active(inst, o, true);
            self.script_enable_passes(inst, &prefab, &passes);
            return;
        }
        let passes = self.set_active(inst, o, false);
        for (n, &(d, k)) in passes.iter().enumerate() {
            let Component::Script { class, .. } = &prefab.objects[d].components[k].1 else { unreachable!() };
            let id = inst.objects[d].components[k];
            // Deactivation stops its coroutines.
            self.coroutines.retain(|c| c.owner != id);
            self.end_of_frame.retain(|c| c.owner != id);
            self.fixed_update_waits.retain(|c| c.owner != id);
            self.stop_all_coroutines(id);
            if class == "PhysicalBehaviour" {
                let p = self.physicals.iter().position(|p| p.id == id).expect("a PhysicalBehaviour in the arena");
                self.physicals[p].active = false;
            }
            // Object.Destroy disabled it already.
            if !self.destroy_disabled.remove(&id) {
                self.script_on_disable(inst, d, k, true);
                if passes.get(n + 1).is_none_or(|x| x.0 != d) {
                    self.portal_traversing_script_pass(inst, d, false);
                }
            }
        }
    }

    /// The OnEnable of script `id` (class `class`, prefab m_Enabled `enabled`) on object `d`, once its Behaviour is enabled.
    pub(super) fn script_on_enable_arm(&mut self, inst: &mut super::spawn::Instance, d: usize, class: &str, enabled: &bool, id: i32) {
        match class {
            "PhysicalBehaviour" => {
                let p = self.physicals.iter().position(|p| p.id == id).expect("a PhysicalBehaviour in the arena");
                self.physicals[p].active = true;
                // The wires AddComponent put on this GameObject come back into the manager with it.
                self.blood_wires_requeue_start(p);
            }
            "BeamformerBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.beamformers.iter().position(|c| c.id == id).expect("a BeamformerBehaviour in the arena");
                    self.beamformer_on_enable(i);
                }
            }
            "HeartMonitorBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.heart_monitors.iter().position(|c| c.id == id).expect("a HeartMonitorBehaviour in the arena");
                    self.heart_monitor_update_activation(i);
                }
            }
            "JavelinBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.javelins.iter().position(|c| c.id == id).expect("a JavelinBehaviour in the arena");
                    self.javelin_on_enable(i);
                }
            }
            // SpeedometerBehaviour.OnEnable: UpdateActivation.
            "SpeedometerBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.v45.speedometers.iter().position(|c| c.id == id).expect("a SpeedometerBehaviour in the arena");
                    self.speedometer_update_activation(i);
                }
            }
            // TorchBehaviour.OnEnable: a torch that is enabled registers the contacts its colliders have.
            "TorchBehaviour" => {
                if let Some(t) = self.torches.iter().position(|x| x.id == id) {
                    if self.torches[t].enabled {
                        let colliders = Game::torch_child_colliders(inst, d);
                        self.torch_enable_contacts(t, colliders);
                    }
                }
            }
            "DefibrillatorBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.defibrillators.iter().position(|c| c.id == id).expect("a DefibrillatorBehaviour in the arena");
                    self.defibrillator_on_enable(i);
                }
            }
            "ThermometerBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.thermometers.iter().position(|c| c.id == id).expect("a ThermometerBehaviour in the arena");
                    self.thermometer_update_activation(i);
                }
            }
            "HeartMonitorBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.heart_monitors.iter().position(|c| c.id == id).expect("a HeartMonitorBehaviour in the arena");
                    self.heart_monitor_update_activation(i);
                }
            }
            "KeyTriggerBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let Some(&(_, super::Behaviour::KeyTrigger(k))) = self.behaviour_of.get(&id) else { unreachable!("KeyTrigger in the arena") };
                    self.key_trigger_on_enable(k);
                }
            }
            "EnergySwordMotionBlur" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.energy_blurs.iter().position(|c| c.id == id).expect("an EnergySwordMotionBlur in the arena");
                    // Its clone is checked out here (a SetActive from EnergySwordBehaviour.SetActivation): read from it.
                    self.energy_blur_on_enable_awake(i, inst.objects[d].activated);
                }
            }
            "JukeboxBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.jukeboxes.iter().position(|c| c.id == id).expect("a JukeboxBehaviour in the arena");
                    self.jukebox_on_enable(i);
                }
            }
            // OnEnable: UpdateActivation (IdleLoop).
            "HoverThrusterBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.hover_thrusters.iter().position(|c| c.id == id).expect("a HoverThrusterBehaviour in the arena");
                    self.hover_thruster_update_activation(i);
                }
            }
            // OnEnable: IdleLoop.Play(); AccLoop.Play() (the fan is presentation).
            "HovercarBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.hovercars.iter().position(|c| c.id == id).expect("a HovercarBehaviour in the arena");
                    self.unity_callback(|g| g.hovercar_on_enable(i));
                }
            }
            // SmoothLineControllerBehaviour.Start, not run yet (the gun hid it in Awake).
            "SmoothLineControllerBehaviour" => {
                let i = self.smooth_lines.iter().position(|c| c.id == id).expect("a SmoothLineControllerBehaviour in the arena");
                if self.script_enabled(id, *enabled) && !self.smooth_lines[i].started {
                    self.pending_starts.push(super::misc::PendingStart::SmoothLine(i));
                }
            }
            // No OnEnable.
            "AdhesiveCouplerBehaviour" => {}
            "LightningToolEntityBehaviour" | "PyroPowerToolEntity" => {}
            "EnergyVesselLightningBehaviour" | "SeismicChargeExplosionBehaviour" => {}
            "TransmitterBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.transmitters.iter().position(|c| c.id == id).expect("a TransmitterBehaviour in the arena");
                    self.transmitter_on_enable(i);
                }
            }
            // OnEnable: SliderJoint.useMotor = true (winch.rs).
            "WinchBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.winches.iter().position(|c| c.id == id).expect("a WinchBehaviour in the arena");
                    self.winch_on_enable(i);
                }
            }
            // OnEnable: Joint.useMotor = true (ceiling_turret.rs).
            "CeilingTurretBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.ceiling_turrets.iter().position(|c| c.id == id).expect("a CeilingTurretBehaviour in the arena");
                    self.ceiling_turret_on_enable(i);
                }
            }
            // OnEnable: Hinge.useMotor = true (counter.rs).
            "PointerBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.pointers.iter().position(|c| c.id == id).expect("a PointerBehaviour in the arena");
                    self.pointer_on_enable(i);
                }
            }
            // ImmobilityFieldBehaviour.OnEnable: UpdateActivation.
            "ImmobilityFieldBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.immobility_fields.iter().position(|c| c.id == id).expect("an ImmobilityFieldBehaviour in the arena");
                    self.immobility_field_update_activation_in(i, inst);
                }
            }
            // OnEnable: UpdateActivation.
            "CardiopulmonaryBypassMachineBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.cardiopulmonary_bypasses.iter().position(|c| c.id == id).expect("a CardiopulmonaryBypassMachineBehaviour in the arena");
                    self.cardiopulmonary_bypass_update_activation(i, inst);
                }
            }
            // GateBehaviour.OnEnable: PowerLevelBar.enabled = true.
            "GateBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let g = self.gates.iter().position(|c| c.id == id).expect("a GateBehaviour in the arena");
                    self.gate_on_enable(g);
                }
            }
            "LagboxBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let g = self.lagboxes.iter().position(|c| c.id == id).expect("a LagboxBehaviour in the arena");
                    self.lagbox_on_enable(g);
                }
            }
            "TimedGateBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let g = self.timed_gates.iter().position(|c| c.id == id).expect("a TimedGateBehaviour in the arena");
                    self.timed_gate_on_enable(g);
                }
            }
            "ObstructionEmitterBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.obstruction_emitters.iter().position(|c| c.id == id).expect("an ObstructionEmitterBehaviour in the arena");
                    self.obstruction_emitter_on_enable(i);
                }
            }
            "RailgunBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.railguns.iter().position(|c| c.id == id).expect("a RailgunBehaviour in the arena");
                    self.railgun_on_enable(i);
                }
            }
            // OnEnable: UpdateActivation (the readout, text and sprite), SetMode(Mode) (the display texture), the aim line,
            // the key display and light: presentation.
            "ParticleMachineBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let Some(&(_, super::Behaviour::ParticleMachine(m))) = self.behaviour_of.get(&id) else { unreachable!("a ParticleMachineBehaviour in the arena") };
                    self.particle_machine_update_activation(m, false);
                }
            }
            "LiquidPumpBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let l = self.liquid_pumps.iter().position(|c| c.id == id).expect("a LiquidPumpBehaviour in the arena");
                    let mode = self.liquid_pumps[l].Mode;
                    self.liquid_pump_set_mode(l, mode);
                }
            }
            "JavelinBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let j = self.javelins.iter().position(|c| c.id == id).expect("a JavelinBehaviour in the arena");
                    self.javelin_on_enable(j);
                }
            }
            "AntennaReceiverBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    let i = self.antenna_receivers.iter().position(|c| c.id == id).expect("an AntennaReceiverBehaviour in the arena");
                    self.antenna_receiver_on_enable(i);
                }
            }
            // EggBehaviour.OnEnable renames the object.
            // RectangularReverbZone.OnEnable: Zone = GetComponent<AudioReverbZone>() (audio).
            "EggBehaviour" | "RectangularReverbZone" => {}
            // ObjectGradientAnimation.OnEnable: ReplayOnEnable restarts the gradient.
            "SpriteGradientAnimation" => {
                if self.script_enabled(id, *enabled) {
                    let v = self.visuals.iter().position(|c| c.id == id).expect("a SpriteGradientAnimation in the arena");
                    self.visual_on_enable(v);
                }
            }
            // BreakJointOnShotBehaviour.OnEnable: the GetComponent<Joint2D>() is made at link time.
            "BreakJointOnShotBehaviour" => {}
            "PostProcessVolume" => {
                if self.script_enabled(id, *enabled) {
                    let k = inst.objects[d].components.iter().position(|&x| x == id).expect("the volume's component");
                    let v = super::map::post_volume_of(self.assets.as_deref().expect("the game's data"), &inst.prefab, d, k, id);
                    self.post_volume_on_enable(v);
                }
            }
            "ColorWave" => {
                if *enabled && !self.script_disabled(id) {
                    self.color_wave_on_enable();
                }
            }
            name if super::translated::CLASSES.contains(&name) => match self.behaviour_of.get(&id) {
            Some(&(_, super::Behaviour::Translated(t))) => {
                if self.tr_enabled(t) {
                    self.tr_on_enable(t);
                }
            }
            _ => unreachable!("{class}: OnEnable of a script SetActive did not wake"),
        },
            // IgnoreParentSize.Start, not run yet (it came, or would have come, while the object was inactive).
            // PinBehaviour.OnEnable: SpriteChild.gameObject.SetActive(true).
            "PinBehaviour" => {
                if self.script_enabled(id, *enabled) {
                    self.pin_sprite_child_set_active(id, true);
                }
            }
            "IgnoreParentSize" => {
                if self.script_enabled(id, *enabled) && self.ignore_parent_sizes.iter().any(|x| x.id == id && !x.started) {
                    self.pending_starts.push(super::misc::PendingStart::IgnoreParentSize(id));
                }
            }
        c if !super::declares(c, "OnEnable") => {}
            _ => unimplemented!("mod-only: {class}.OnEnable"),
        }
    }

    /// Behaviour::RemoveFromManager of the script `k` on object `d`, with its OnDisable.
    /// `deactivating`: its GameObject goes inactive (or is destroyed), not `enabled = false`.
    pub(super) fn script_on_disable(&mut self, inst: &mut super::spawn::Instance, d: usize, k: usize, deactivating: bool) {
        let prefab = inst.prefab.clone();
        let Component::Script { class, enabled, .. } = &prefab.objects[d].components[k].1 else { unreachable!() };
        let id = inst.objects[d].components[k];
        {
            if let Some(&(_, b)) = self.behaviour_of.get(&id) {
                self.behaviours.remove(b);
            }
            match class.as_str() {
                "PhysicalBehaviour" => {
                    let p = self.physicals.iter().position(|p| p.id == id).expect("a PhysicalBehaviour in the arena");
                    if *enabled {
                        self.physical_on_disable(p);
                    }
                }
                "ImmobilityFieldBehaviour" => {
                    let i = self.immobility_fields.iter().position(|c| c.id == id).expect("an ImmobilityFieldBehaviour in the arena");
                    if self.immobility_fields[i].enabled {
                        self.immobility_field_on_disable(i, inst);
                    }
                }
                "SpeedometerBehaviour" => {
                    if self.script_enabled(id, *enabled) {
                        let i = self.v45.speedometers.iter().position(|c| c.id == id).expect("a SpeedometerBehaviour in the arena");
                        self.speedometer_on_disable(i);
                    }
                }
                // PinBehaviour.OnDisable: SpriteChild.gameObject.SetActive(false).
                "PinBehaviour" => {
                    if self.script_enabled(id, *enabled) {
                        self.pin_sprite_child_set_active(id, false);
                    }
                }
                "PostProcessVolume" => self.post_volume_on_disable(id),
                // LimbBehaviour has no OnDisable, but the GForceMeasureBehaviour its Awake added (after the prefab's
                // components, so not in this list) has: it zeroes its four vectors (fnb2c_clearmap's destroyed person).
                "LimbBehaviour" => {
                    if let Some(l) = self.limbs.iter().position(|x| x.id == id) {
                        self.gforce_on_disable(l);
                    }
                }
                // No OnDisable.
                "AdhesiveCouplerBehaviour" => {}
                "LightningToolEntityBehaviour" | "PyroPowerToolEntity" => {}
                "EnergyVesselLightningBehaviour" | "SeismicChargeExplosionBehaviour" => {}
                "TransmitterBehaviour" => {
                    let i = self.transmitters.iter().position(|c| c.id == id).expect("a TransmitterBehaviour in the arena");
                    if self.transmitters[i].enabled {
                        self.transmitter_on_disable(i);
                    }
                }
                "JukeboxBehaviour" => {
                    let i = self.jukeboxes.iter().position(|c| c.id == id).expect("a JukeboxBehaviour in the arena");
                    if self.jukeboxes[i].enabled {
                        self.jukebox_on_disable(i);
                    }
                }
                "WinchBehaviour" => {
                    let i = self.winches.iter().position(|c| c.id == id).expect("a WinchBehaviour in the arena");
                    if self.winches[i].enabled {
                        self.winch_on_disable(i);
                    }
                }
                "HoverThrusterBehaviour" => {
                    let i = self.hover_thrusters.iter().position(|c| c.id == id).expect("a HoverThrusterBehaviour in the arena");
                    if self.hover_thrusters[i].enabled {
                        self.hover_thruster_on_disable(i);
                    }
                }
                "HovercarBehaviour" => {
                    let i = self.hovercars.iter().position(|c| c.id == id).expect("a HovercarBehaviour in the arena");
                    if self.hovercars[i].enabled {
                        self.unity_callback(|g| g.hovercar_on_disable(i));
                    }
                }
                "ConveyorBeltBehaviour" => {
                    let i = self.conveyors.iter().position(|c| c.id == id).expect("a ConveyorBeltBehaviour in the arena");
                    if self.conveyors[i].enabled {
                        self.conveyor_on_disable(i);
                    }
                }
                // OnDisable: Joint.useMotor = false (ceiling_turret.rs).
                "CeilingTurretBehaviour" => {
                    let i = self.ceiling_turrets.iter().position(|c| c.id == id).expect("a CeilingTurretBehaviour in the arena");
                    if self.ceiling_turrets[i].enabled {
                        self.ceiling_turret_on_disable(i);
                    }
                }
                // OnDisable: Hinge.useMotor = false (counter.rs).
                "PointerBehaviour" => {
                    let i = self.pointers.iter().position(|c| c.id == id).expect("a PointerBehaviour in the arena");
                    if self.pointers[i].enabled {
                        self.pointer_on_disable(i);
                    }
                }
                "TeslaCoilBehaviour" => {
                    let i = self.tesla_coils.iter().position(|c| c.id == id).expect("a TeslaCoilBehaviour in the arena");
                    if self.tesla_coils[i].enabled {
                        self.unity_callback(|g| g.tesla_coil_on_disable(i));
                    }
                }
                "ObstructionEmitterBehaviour" => {
                    let i = self.obstruction_emitters.iter().position(|c| c.id == id).expect("an ObstructionEmitterBehaviour in the arena");
                    if self.obstruction_emitters[i].enabled {
                        self.unity_callback(|g| g.obstruction_emitter_on_disable(i, inst));
                    }
                }
                "DecimatorBehaviour" => {
                    let i = self.decimators.iter().position(|c| c.id == id).expect("a DecimatorBehaviour in the arena");
                    if self.decimators[i].enabled {
                        self.unity_callback(|g| g.decimator_on_disable(i, inst));
                    }
                }
                "EnergySwordBehaviour" => {
                    let i = self.energy_swords.iter().position(|c| c.id == id).expect("an EnergySwordBehaviour in the arena");
                    if self.energy_swords[i].enabled {
                        self.energy_sword_on_disable(i, inst);
                    }
                }
                // OnDisable -> OnDestroy
                "SharpOnAllSidesBehaviour" => {
                    let i = self.sharp_on_all_sides.iter().position(|c| c.id == id).expect("a SharpOnAllSidesBehaviour in the arena");
                    if self.sharp_on_all_sides[i].enabled {
                        self.sharp_on_all_sides_on_disable(i);
                    }
                }
                "DefibrillatorBehaviour" => {
                    let i = self.defibrillators.iter().position(|c| c.id == id).expect("a DefibrillatorBehaviour in the arena");
                    if self.defibrillators[i].enabled {
                        self.defibrillator_on_disable(i);
                    }
                }
                "CardiopulmonaryBypassMachineBehaviour" => {
                    let i = self.cardiopulmonary_bypasses.iter().position(|c| c.id == id).expect("a CardiopulmonaryBypassMachineBehaviour in the arena");
                    if self.cardiopulmonary_bypasses[i].enabled {
                        self.cardiopulmonary_bypass_on_disable(i);
                    }
                }
                "LiquidentifierBehaviour" => {
                    let i = self.liquidentifiers.iter().position(|c| c.id == id).expect("a LiquidentifierBehaviour in the arena");
                    if self.liquidentifiers[i].enabled {
                        self.liquidentifier_on_disable(i);
                    }
                }
                "CarBehaviour" => {
                    let car = self.cars.iter().position(|c| c.id == id).expect("a CarBehaviour in the arena");
                    self.car_on_disable(car, inst);
                }
                "LEDBulbBehaviour" => {
                    let i = self.leds.iter().position(|c| c.id == id).expect("a LEDBulbBehaviour in the arena");
                    if self.leds[i].enabled {
                        self.led_on_disable(i);
                    }
                }
                "DetectorBehaviour" => {
                    let i = self.detectors.iter().position(|c| c.id == id).expect("a DetectorBehaviour in the arena");
                    if self.detectors[i].enabled {
                        self.detector_on_disable(i);
                    }
                }
                "GeneratorBehaviour" => {
                    let i = self.generators.iter().position(|c| c.id == id).expect("a GeneratorBehaviour in the arena");
                    if self.generators[i].enabled {
                        self.generator_on_disable(i);
                    }
                }
                "SingleFloodlightBehaviour" => {
                    let i = self.floodlights.iter().position(|c| c.id == id).expect("a SingleFloodlightBehaviour in the arena");
                    if self.floodlights[i].enabled {
                        self.floodlight_on_disable(i, inst);
                    }
                }
                "SpikeGrenadeBehaviour" => {
                    let i = self.spike_grenades.iter().position(|c| c.id == id).expect("a SpikeGrenadeBehaviour in the arena");
                    if self.spike_grenades[i].enabled {
                        self.spike_grenade_on_disable(i);
                    }
                }
                "TemperatureRayGunBehaviour" => {
                    let i = self.ray_guns.iter().position(|c| c.id == id).expect("a TemperatureRayGunBehaviour in the arena");
                    if self.ray_guns[i].enabled {
                        self.ray_gun_on_disable(i);
                    }
                }
                "ThrusterbedBehaviour" => {
                    let i = self.thrusterbeds.iter().position(|c| c.id == id).expect("a ThrusterbedBehaviour in the arena");
                    if self.thrusterbeds[i].enabled {
                        self.thrusterbed_on_disable(i);
                    }
                }
                "BeamformerBehaviour" => {
                    let i = self.beamformers.iter().position(|c| c.id == id).expect("a BeamformerBehaviour in the arena");
                    if self.beamformers[i].enabled {
                        self.beamformer_on_disable(i);
                    }
                }
                "DisplayBehaviour" => {
                    let i = self.displays.iter().position(|c| c.id == id).expect("a DisplayBehaviour in the arena");
                    if self.displays[i].enabled {
                        self.display_on_disable(i, inst);
                    }
                }
                "ButtonBehaviour" => {
                    let i = self.buttons.iter().position(|c| c.id == id).expect("a ButtonBehaviour in the arena");
                    self.button_on_disable(i);
                }
                "LaserBehaviour" => {
                    let i = self.lasers.iter().position(|c| c.id == id).expect("a LaserBehaviour in the arena");
                    if self.lasers[i].enabled {
                        self.laser_on_disable(i);
                    }
                }
                "PhysicsGunBehaviour" => {
                    let i = self.physics_guns.iter().position(|c| c.id == id).expect("a PhysicsGunBehaviour in the arena");
                    if self.physics_guns[i].enabled {
                        self.physics_gun_on_disable(i, inst);
                    }
                }
                "LagboxBehaviour" => {
                    let g = self.lagboxes.iter().position(|c| c.id == id).expect("a LagboxBehaviour in the arena");
                    if self.lagboxes[g].enabled {
                        self.lagbox_on_disable(g);
                    }
                }
                "TimedGateBehaviour" => {
                    let g = self.timed_gates.iter().position(|c| c.id == id).expect("a TimedGateBehaviour in the arena");
                    if self.timed_gates[g].enabled {
                        self.timed_gate_on_disable(g);
                    }
                }
                "JavelinBehaviour" => {
                    let i = self.javelins.iter().position(|c| c.id == id).expect("a JavelinBehaviour in the arena");
                    if self.javelins[i].enabled {
                        self.javelin_on_disable(i);
                    }
                }
                "LaserReceiverBehaviour" => {
                    let i = self.laser_receivers.iter().position(|c| c.id == id).expect("a LaserReceiverBehaviour in the arena");
                    if self.laser_receivers[i].enabled {
                        self.laser_receiver_on_disable(i);
                    }
                }
                "AntennaReceiverBehaviour" => {
                    let i = self.antenna_receivers.iter().position(|c| c.id == id).expect("an AntennaReceiverBehaviour in the arena");
                    if self.antenna_receivers[i].enabled {
                        self.antenna_receiver_on_disable(i);
                    }
                }
                "MetronomeBehaviour" => {
                    let m = self.metronomes.iter().position(|c| c.id == id).expect("a MetronomeBehaviour in the arena");
                    if self.metronomes[m].enabled {
                        self.metronome_on_disable(m);
                    }
                }
                "InfraredThermometer" => {
                    let i = self.infrared_thermometers.iter().position(|c| c.id == id).expect("an InfraredThermometer in the arena");
                    if self.infrared_thermometers[i].enabled {
                        self.infrared_on_disable(i);
                    }
                }
                "ActivationToggleBehaviour" => {
                    let t = self.activation_toggles.iter().position(|c| c.id == id).expect("an ActivationToggleBehaviour in the arena");
                    if self.activation_toggles[t].enabled {
                        self.activation_toggle_on_disable_in(inst, t);
                    }
                }
                "GateBehaviour" => {
                    let g = self.gates.iter().position(|c| c.id == id).expect("a GateBehaviour in the arena");
                    if self.gates[g].enabled {
                        self.gate_on_disable(g);
                    }
                }
                "RailgunBehaviour" => {
                    let i = self.railguns.iter().position(|c| c.id == id).expect("a RailgunBehaviour in the arena");
                    if self.railguns[i].enabled {
                        self.railgun_on_disable(i);
                    }
                }
                "RayCannonBehaviour" => {
                    let i = self.ray_cannons.iter().position(|c| c.id == id).expect("a RayCannonBehaviour in the arena");
                    if self.ray_cannons[i].enabled {
                        self.ray_cannon_on_disable(i);
                    }
                }
                "LightningGunBehaviour" => {
                    let i = self.lightning_guns.iter().position(|c| c.id == id).expect("a LightningGunBehaviour in the arena");
                    if self.lightning_guns[i].enabled {
                        self.lightning_gun_on_disable(i);
                    }
                }
                "ParticleMachineBehaviour" => self.particle_machine_on_disable(id, *enabled, deactivating),
                "ThermometerBehaviour" => {
                    let i = self.thermometers.iter().position(|c| c.id == id).expect("a ThermometerBehaviour in the arena");
                    if self.thermometers[i].enabled {
                        self.thermometer_on_disable(i);
                    }
                }
                "HeartMonitorBehaviour" => {
                    let i = self.heart_monitors.iter().position(|c| c.id == id).expect("a HeartMonitorBehaviour in the arena");
                    if self.heart_monitors[i].enabled {
                        self.heart_monitor_on_disable(i);
                    }
                }
                "KeyTriggerBehaviour" => {
                    let Some(&(_, super::Behaviour::KeyTrigger(k))) = self.behaviour_of.get(&id) else { unreachable!("KeyTrigger in the arena") };
                    if self.key_triggers[k].enabled {
                        self.key_trigger_on_disable(k);
                    }
                }
                name if super::translated::CLASSES.contains(&name) => {
                    if let Some(&(_, super::Behaviour::Translated(t))) = self.behaviour_of.get(&id) {
                        if self.tr_enabled(t) {
                            // The script's OnDisable may SetActive an object of this very clone (the Gyrostabiliser's Light), which is checked out here.
                            self.tr_on_disable_pending.push(t);
                        }
                    }
                }
                c if !super::declares(c, "OnDisable") => {}
                _ => unimplemented!("mod-only: {class}.OnDisable"),
            }
        }
    }

    /// WaterBehaviour.UpdateObjectsToBeActiveIfWater: the objects, then SpriteRenderer.enabled and
    /// SpriteMask.enabled (rendering).
    pub(crate) fn water_update_objects_to_be_active(&mut self, w: usize, active: bool) {
        for i in 0..self.waters[w].objects_to_be_active.len() {
            let o = self.waters[w].objects_to_be_active[i];
            self.water_set_active(w, o, active);
        }
    }

    /// FreezeStackController.RequestFreeze(a) (the stack's RefInt..ctor(1) holds Value as the i32): the first request makes the Rigidbody2D static.
    pub(super) fn freeze_request(&mut self, p: usize) {
        if self.physicals[p].destroyed {
            return;
        }
        if let Some(n) = self.freeze_stack.get_mut(&p) {
            ref_int_increment(n);
            return;
        }
        self.freeze_stack.insert(p, 1);
        self.rigidbody_set_body_type(p, 2);
    }

    /// FreezeStackController.RequestUnfreeze(a): the last release makes the Rigidbody2D dynamic again.
    pub(crate) fn freeze_request_unfreeze(&mut self, p: usize) {
        if self.physicals[p].destroyed {
            return;
        }
        let Some(n) = self.freeze_stack.get_mut(&p) else { return };
        ref_int_decrement(n);
        if *n <= 0 {
            self.freeze_stack.remove(&p);
            self.rigidbody_set_body_type(p, 0);
        }
    }

    /// Rigidbody2D.bodyType = t (0 Dynamic, 2 Static).
    fn rigidbody_set_body_type(&mut self, p: usize, body_type: i32) {
        let rb = &mut self.physicals[p].rigidbody;
        if rb.detached.is_some() {
            rb.bodyType = body_type;
            return;
        }
        let component = rb.component;
        if component == 0 {
            panic!("{}: Rigidbody2D.bodyType of a body recorded before the recorder wrote its root's Clone records (re-record it)", self.physicals[p].name);
        }
        // The colliders are remade from the clone's Transforms: its physicals' live poses first (stale once a body moved).
        self.with_component_instance(component, |g, inst, o| {
            g.refresh_clone_poses(inst);
            g.rigidbody2d_set_body_type(inst, BodyOwner::Physical(p), o, body_type)
        });
    }

    /// FrozenInIceBehaviour.Awake, from physical.gameObject.AddComponent<FrozenInIceBehaviour>(): it takes the SkinMaterialHandler's ShouldRot.
    pub(crate) fn frozen_in_ice_add(&mut self, p: usize) {
        self.allocate_instance_id();
        let skin = self.limbs.iter().position(|l| l.PhysicalBehaviour == p);
        let mut f = FrozenInIce { physical: p, skin, shouldRotOriginal: false, destroy_at: None, destroyed: false };
        if let Some(l) = skin {
            f.shouldRotOriginal = self.skins[l].ShouldRot;
            self.skins[l].ShouldRot = false;
        }
        self.frozen_in_ice.push(f);
    }

    /// `item.TryGetComponent<FrozenInIceBehaviour>(out c)` then Object.Destroy(c) and RequestUnfreeze(item.rigidbody).
    fn frozen_in_ice_release(&mut self, p: usize) {
        if let Some(i) = self.frozen_in_ice.iter().position(|f| f.physical == p && !f.destroyed) {
            // DestroyObjectDelayed: queued at min(fixedTimeAsDouble, the frame's time); a second Destroy finds it queued.
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
            self.frozen_in_ice[i].destroy_at.get_or_insert(at);
            self.freeze_request_unfreeze(p);
        }
    }

    /// The FrozenInIceBehaviours' queued destroys due by now, as (time, index).
    pub fn frozen_in_ice_due_destroys(&self, now: f64) -> Vec<(f64, usize)> {
        (0..self.frozen_in_ice.len()).filter_map(|i| self.frozen_in_ice[i].destroy_at.filter(|&at| at <= now).map(|at| (at, i))).collect()
    }

    /// FrozenInIceBehaviour.OnDestroy: the skin gets its ShouldRot back.
    pub fn frozen_in_ice_run_destroy(&mut self, i: usize) {
        let f = &mut self.frozen_in_ice[i];
        f.destroy_at = None;
        f.destroyed = true;
        let (p, skin, rot) = (f.physical, f.skin, f.shouldRotOriginal);
        if let Some(l) = skin.filter(|_| !self.physicals[p].destroyed) {
            self.skins[l].ShouldRot = rot;
        }
    }

    /// WaterBehaviour.SetFrozen
    pub(crate) fn water_set_frozen(&mut self, w: usize) {
        let sound = self.waters[w].boiling_sound;
        self.water_set_active(w, sound, false);
        self.waters[w].wasFrozen = true;
        // LayerMask.GetMask("Bounds") (layer 11); Ignore is empty.
        for p in 0..self.physicals.len() {
            let ph = &self.physicals[p];
            if !ph.in_world || ph.destroyed {
                continue;
            }
            // physicalBehaviour.gameObject.layer: its body's object, or (a map physical whose body is gone or has no user) its clone's object.
            let id = ph.id;
            // (A detached Rigidbody2D, destroyed, has no body left.)
            let go = ph.rigidbody.detached.is_none().then(|| self.world.body(ph.rigidbody.body).user).flatten().map(|u| u.game_object)
                .or_else(|| self.unsynced.iter().chain(self.retained.iter()).find_map(|inst| inst.objects.iter().find(|x| x.components.contains(&id)).map(|x| x.game_object)))
                .unwrap_or_else(|| panic!("physical {} #{id}: no Rigidbody2D user and no clone", ph.name));
            if self.world.layers[&go] == 11 || self.waters[w].frozen_in_ice.iter().any(|&x| x == p) {
                continue;
            }
            let grid = ph.LocalColliderGridPoints.clone().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            let mut num = 0;
            for local in &grid {
                let point = self.transform_point3(p, [local[0], local[1], 0.0]);
                if self.water_is_point_inside(w, [point[0], point[1]]) {
                    num += 1;
                    if num as f32 / grid.len() as f32 >= 0.05 {
                        let rb = self.physicals[p].rigidbody.clone();
                        rb.set_velocity(&mut self.world, [0.0, 0.0]);
                        rb.set_angular_velocity(&mut self.world, 0.0);
                        self.freeze_request(p);
                        self.waters[w].frozen_in_ice.add(p);
                        self.frozen_in_ice_add(p);
                        let ph = &mut self.physicals[p];
                        ph.Temperature = mathf::min(ph.Temperature, self.AmbientTemperature);
                        break;
                    }
                }
            }
        }
        self.water_update_objects_to_be_active(w, false);
        // SpriteRenderer.enabled = true (rendering).
        let ice = self.waters[w].ice_object;
        self.water_set_active(w, ice, true);
    }

    /// WaterBehaviour.UpdateTemperatureState
    pub fn water_update_temperature_state(&mut self, w: usize) {
        let ice = self.waters[w].ice_object;
        self.water_set_active(w, ice, false);
        if self.water_is_frozen(w) {
            self.water_set_frozen(w);
            return;
        }
        if self.waters[w].wasFrozen {
            let items: Vec<usize> = self.waters[w].frozen_in_ice.iter().copied().collect();
            for p in items {
                if !self.physicals[p].destroyed {
                    self.frozen_in_ice_release(p);
                }
            }
        }
        self.waters[w].wasFrozen = false;
        self.waters[w].frozen_in_ice.clear();
        if self.water_is_evaporated(w) {
            self.water_set_evaporated(w);
            return;
        }
        let sound = self.waters[w].boiling_sound;
        let boiling = self.AmbientTemperature >= self.waters[w].BoilingPointCelsius;
        self.water_set_active(w, sound, boiling);
        self.water_update_objects_to_be_active(w, true);
    }

    /// WaterBehaviour.GetSubmergedObjects: currentlyInWater in order, the destroyed ones skipped.
    pub(crate) fn water_submerged_objects(&self, w: usize) -> Vec<usize> {
        self.waters[w].currentlyInWater.iter().copied().filter(|&p| !self.destroyed_components.contains(&self.physicals[p].id)).collect()
    }

    /// WaterBehaviour.SetEvaporated
    pub(crate) fn water_set_evaporated(&mut self, w: usize) {
        let sound = self.waters[w].boiling_sound;
        self.water_set_active(w, sound, false);
        self.water_update_objects_to_be_active(w, false);
    }

    /// WaterBehaviour.OnTriggerEnter2D / OnTriggerExit2D: DoSplash unless evaporated or frozen.
    pub fn water_on_trigger(&mut self, w: usize, collider: i32) {
        if self.water_is_evaporated(w) || self.water_is_frozen(w) {
            return;
        }
        self.water_do_splash(w, collider);
    }

    /// WaterBehaviour.DoSplash. Ignore is empty (IgnoreObject has no caller).
    pub fn water_do_splash(&mut self, w: usize, collider: i32) {
        if !self.prefs.FancyEffects {
            return;
        }
        // collision.isTrigger, then PhysicalObjectsInWorldByTransform.TryGetValue(collision.transform).
        let Some(p) = self.physical_of_collider(collider) else { return };
        let ph = &self.physicals[p];
        let k = ph.colliders.iter().position(|&c| c == collider).expect("a collider of the physical");
        // A limb has no collider_defs (its colliders come from the human's rig): none of them is a trigger.
        if ph.collider_defs.get(k).is_some_and(|d| d.is_trigger) || !ph.in_world {
            return;
        }
        if ph.rigidbody.velocity(&self.world)[1].abs() < 0.75 {
            return;
        }
        self.water_splash(w, p, collider);
    }

    /// WaterBehaviour.GetDragFactor
    pub(crate) fn water_drag_factor(&self, w: usize, velocity: [f32; 2]) -> f32 {
        let num = (d(mathf::sqr_magnitude(velocity)) * 0.2).f();
        mathf::clamp(mathf::pow(num + 1.0, num * self.waters[w].Drag), 0.6, 1.0)
    }

    /// The person whose AliveBehaviour.AliveByTransform entry is this physical's transform.root.
    pub(super) fn person_of_root(&self, p: usize) -> Option<usize> {
        self.person_of_root_key(self.root_of(p))
    }

    /// The person whose AliveBehaviour.AliveByTransform entry is the root `root` (a `root_of` value).
    pub(super) fn person_of_root_key(&self, root: (bool, usize)) -> Option<usize> {
        if !root.0 {
            return None;
        }
        (0..self.people.len()).find(|&i| !self.people[i].destroyed && self.people[i].Limbs.first().is_some_and(|&l| self.root_of(self.limbs[l].PhysicalBehaviour) == root))
    }

    /// WaterBehaviour.ManagedFixedUpdate (not frozen: SetFrozen is unimplemented, so Ice.Temperature and the
    /// frozenInIce loop never run).
    pub(crate) fn water_fixed_update(&mut self, w: usize) {
        self.waters[w].currentlyInWater.clear();
        if self.water_is_frozen(w) {
            let ice = self.waters[w].ice;
            self.physicals[ice].Temperature = self.AmbientTemperature;
            let even = self.time.frameCount % 2 == 0;
            let items: Vec<usize> = self.waters[w].frozen_in_ice.iter().copied().collect();
            for p in items {
                if self.physicals[p].destroyed {
                    continue;
                }
                if self.physicals[p].OnFire {
                    self.extinguish(p);
                }
                self.physicals[p].Wetness = 1.0;
                if !even {
                    continue;
                }
                let grid = self.physicals[p].LocalColliderGridPoints.clone().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                let mut inside = false;
                for local in &grid {
                    let point = self.transform_point3(p, [local[0], local[1], 0.0]);
                    if self.water_is_point_inside(w, [point[0], point[1]]) {
                        inside = true;
                        break;
                    }
                }
                if !inside {
                    self.frozen_in_ice_release(p);
                }
            }
            return;
        }
        if self.water_is_evaporated(w) {
            return;
        }
        let buoyancy = self.waters[w].Buoyancy;
        // Vector3.up * Buoyancy
        let up = [0.0 * buoyancy, 1.0 * buoyancy];
        let level = self.water_global_surface_level(w);
        let num = mathf::min(self.AmbientTemperature, self.waters[w].BoilingPointCelsius);
        let boiling = self.AmbientTemperature >= self.waters[w].BoilingPointCelsius;
        for p in 0..self.physicals.len() {
            if !self.physicals[p].in_world || self.physicals[p].destroyed {
                continue;
            }
            let ph = &self.physicals[p];
            let num2 = mathf::clamp(mathf::pow((ph.transform.position[1] - level).abs(), 0.25), 1.0, 8.0);
            let num3 = mathf::clamp01(((d(num2) - 1.0) / 7.0).f()) + 1.0;
            let rb = ph.rigidbody.clone();
            let mass = rb.mass(&self.world);
            let grid = ph.LocalColliderGridPoints.clone().unwrap_or_default();
            let props = &self.properties[ph.Properties];
            // mass / (float)LocalGridPointLength * num2 * (Buoyancy * BuoyancyModifier) * Vector3
            let a = (d(mass) / grid.len() as f32 * num2 * (d(props.Buoyancy) * ph.BuoyancyModifier)).f();
            let force = [a * up[0], a * up[1]];
            self.physicals[p].CurrentWaterSurfaceLevel = level;
            let mut inside = false;
            for local in &grid {
                let point = self.transform_point3(p, [local[0], local[1], 0.0]);
                let point = [point[0], point[1]];
                if self.water_is_point_inside(w, point) {
                    inside = true;
                    if !boiling {
                        rb.add_force_at_position(&mut self.world, force, point);
                    }
                    // GetPointVelocity starts with the AutoSyncTransforms.
                    self.sync_pending();
                    let v = rb.point_velocity(&self.world, point);
                    let k = (d(0.4f32) * num3 * self.water_drag_factor(w, v) * mass).f();
                    rb.add_force_at_position(&mut self.world, [k * -v[0], k * -v[1]], point);
                }
            }
            let was_dry = !self.physicals[p].IsUnderWater;
            if !inside {
                continue;
            }
            self.physicals[p].underwaterMarkings += 1;
            self.waters[w].currentlyInWater.push(p);
            if boiling {
                if let Some(person) = self.person_of_root(p) {
                    self.physicals[p].BurnProgress = mathf::max(self.physicals[p].BurnProgress, 0.5);
                    // SendMessage("AddPain", 15f)
                    self.add_pain(person, 15.0);
                }
            }
            let ph = &mut self.physicals[p];
            let transfer = ph.Temperature;
            ph.Temperature = mathf::lerp(transfer, num, 0.05 * self.properties[ph.Properties].HeatTransferSpeedMultiplier);
            if was_dry && self.physicals[p].Temperature >= 100.0 && self.physicals[p].Temperature > num * 2.0 {
                // physicalBehaviour.PlayClipOnce(waterSizzles.PickRandom())
                let clip = self.pick_resource_clip("Audio/water_sizzle", self.waters[w].waterSizzles);
                self.play_clip_once(p, clip, 1.0);
                self.sizzle(p, false);
            }
            self.physicals[p].Wetness += 0.1;
            // SendMessage("WaterImpact", rigidbody.velocity.magnitude): LimbBehaviour's, on an active object.
            let speed = mathf::magnitude2(rb.velocity(&self.world));
            if let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == p) {
                if self.limb_active(l) {
                    self.limb_water_impact(l, speed);
                }
            }
        }
    }

    /// WaterBehaviourManager.FixedUpdate: every physical's underwaterMarkings cleared, each water that ShouldUpdate
    /// (active in the hierarchy and enabled) marks the ones in it, then IsUnderWater follows the markings.
    pub fn water_manager_fixed_update(&mut self) {
        let listed = |ph: &super::Physical| ph.in_world && !ph.destroyed;
        for ph in self.physicals.iter_mut().filter(|ph| listed(ph)) {
            ph.underwaterMarkings = 0;
        }
        for i in 0..self.water_list.len() {
            let w = self.water_list[i];
            if self.water_should_update(w) {
                self.water_fixed_update(w);
            }
        }
        for ph in self.physicals.iter_mut().filter(|ph| listed(ph)) {
            ph.IsUnderWater = ph.underwaterMarkings > 0;
        }
    }

    /// LimbBehaviour.WaterImpact
    pub fn limb_water_impact(&mut self, l: usize, magnitude: f32) {
        if self.limbs[l].ImmuneToDamage {
            return;
        }
        let magnitude = magnitude * self.prefs.FragilityMultiplier;
        if magnitude > 30.0 {
            self.limb_damage(l, magnitude * 0.09);
            if !self.limbs[l].IsAndroid {
                let v = mathf::min((d(self.skins[l].acidProgress) + d(magnitude) * 0.05).f(), 0.6);
                self.skins[l].set_acid(v);
            }
        }
    }
}

/// Instantiate's numbering of a map clone (dump/clone.rs recording_clone, spawn.rs clone_prefab): each GameObject's id
/// less the root's. GameObjects and their prefab components take consecutive ids 2 apart, in prefab order.
pub(super) fn map_id_offsets(prefab: &Prefab) -> Vec<i32> {
    let mut next = 0;
    prefab.objects.iter().map(|ob| { let id = next; next -= 2 * (1 + ob.components.len() as i32); id }).collect()
}

/// The map root's GameObject id, from the recorded b2 world: each map object, alone with its name in the map, holding one
/// Collider2D whose recorded fixtures carry one collider id, gives one; the most common wins (a same-named object outside
/// the map can only mislead one). None without any.
pub(super) fn map_root_id(g: &Game, prefab: &Prefab, rel: &[i32]) -> Option<i32> {
    let name = |o: usize| prefab.objects[o].name.replace(' ', "_");
    let mut count: HashMap<String, usize> = HashMap::new();
    for o in 0..prefab.objects.len() {
        *count.entry(name(o)).or_default() += 1;
    }
    let mut votes: HashMap<i32, usize> = HashMap::new();
    for o in (0..prefab.objects.len()).filter(|&o| count[&name(o)] == 1) {
        let cols: Vec<usize> = (0..prefab.objects[o].components.len()).filter(|&j| matches!(prefab.objects[o].components[j].1, Component::Collider2D(_))).collect();
        let (&[j], Some(&go)) = (&cols[..], g.objects.get(&name(o))) else { continue };
        let ids: std::collections::BTreeSet<i32> = g.world.fixtures.iter().flatten().filter(|x| x.collider.game_object == go).map(|x| x.collider.id).collect();
        let [id] = ids.into_iter().collect::<Vec<_>>()[..] else { continue };
        *votes.entry(id - rel[o] + 2 * (1 + j as i32)).or_default() += 1;
    }
    votes.into_iter().max_by_key(|&(r, n)| (n, r)).map(|(r, _)| r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_map_numbering() {
        // The numbering against a recording on the legacy path (b2 world, no Clone rows): every map object alone with its
        // name and holding colliders has its recorded collider ids where the numbering puts them.
        let dump = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("reference-probe/game/dump34map.txt");
        let (Ok(game), Ok(header)) = (crate::game_assets::find_game(None), super::super::dump::header(&dump)) else { return };
        if crate::assets::il::load(&game).is_ok() && crate::assets::il::is_1_28_45() {
            return; // recorded on 1.28.41: 1.28.45's map numbers its objects differently
        }
        let s = super::super::dump::snapshots(&dump).unwrap().find(|s| s.pt == "FS" && s.of("b2World").next().is_some()).unwrap();
        let r = super::super::dump::physics::restore(&s, V::new(-0.0, -9.81)).unwrap();
        let mut g = Game::default();
        (g.world, g.objects) = (r.world, r.objects);
        let mut db = AssetDb::open(&game).unwrap();
        let prefab = super::super::particle_forces::recorded_map_prefab(&mut db, &header).unwrap();
        let rel = map_id_offsets(&prefab);
        let root = map_root_id(&g, &prefab, &rel).expect("a numbered map");
        let name = |o: usize| prefab.objects[o].name.replace(' ', "_");
        let mut checked = 0;
        for o in (0..prefab.objects.len()).filter(|&o| (0..prefab.objects.len()).filter(|&p| name(p) == name(o)).count() == 1) {
            let Some(&go) = g.objects.get(&name(o)) else { continue };
            let recorded: std::collections::BTreeSet<i32> = g.world.fixtures.iter().flatten().filter(|x| x.collider.game_object == go).map(|x| x.collider.id).collect();
            let numbered: std::collections::BTreeSet<i32> = (0..prefab.objects[o].components.len()).filter(|&j| matches!(prefab.objects[o].components[j].1, Component::Collider2D(_))).map(|j| root + rel[o] - 2 * (1 + j as i32)).collect();
            if numbered.is_empty() || recorded.is_empty() {
                continue;
            }
            assert_eq!(recorded, numbered, "{}", name(o));
            checked += 1;
        }
        assert!(checked >= 2, "{checked} map objects checked");
    }
}
