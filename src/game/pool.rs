//! PoolGenerator and ObjectPoolBehaviour for pooled prefabs that carry scripts (full clones that are Instantiated,
//! deactivated on Return and reactivated on Request). One pool per prefab, as PoolGenerator.pools (keyed by the
//! prefab's instance id): GetFor, Ensure (a new GameObject + ObjectPoolBehaviour take 3 instance ids; MaxPoolSize =
//! PoolGenerator.PoolSize 256), RequestPrefab, Return and the 300 s inactivity Clear.
//!
//! Request: a pool with an available object pops it off `availableObjects` (a Stack since 1.28.41; a destroyed one
//! logs an error and the request returns null), sets its position, SetActive(true), then BroadcastMessage OnPoolableInitialised and
//! OnPoolableReinitialised; otherwise a full pool returns null and a pool with room Instantiates the prefab, SetActive(true)
//! and broadcasts OnPoolableInitialised only. The particle-only prefabs of water.rs (SplashPool), the BloodPuff pool
//! (misc.rs) and the BulletTracerPool (ballistics.rs) are models of the same ObjectPoolBehaviour specialised to what
//! their clones hold (no scripts to message, or a particle clock for the OnParticleSystemStopped Return), so they stay
//! as they are; a prefab goes through exactly one of them.
#![allow(non_snake_case)]

use super::misc::ObjectPool;
use super::translated::rt::Go;
use super::Game;
use crate::assets::db::{Component, ObjRef};

// PoolGenerator.Awake: `Instance = this`, the one static pool generator: `Game::pools` is it.
/// PoolGenerator.PoolSize
const POOL_SIZE: usize = 256;

/// One Instantiated clone of a pool: its root Transform's instance id.
#[derive(Clone, Debug)]
pub struct PoolItem {
    pub root: i32,
}

/// ReturnToPoolAfterTime: Life, the clock and the pool OnPoolableInitialised stored.
#[derive(Clone, Debug)]
pub struct ReturnToPoolAfterTime {
    pub id: i32,
    pub Life: f32,
    pub time: f32,
    pub pool: Option<PoolRef>,
}

impl ReturnToPoolAfterTime {
    /// ReturnToPoolAfterTime.get_Progress: time / Life, read by no caller in the game (mods only).
    pub fn progress(&self) -> f32 {
        self.time / self.Life
    }
}

impl Default for ReturnToPoolAfterTime {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Life: 1.0,
            time: Default::default(),
            pool: Default::default(),
        }
    }
}

/// An ObjectPoolBehaviour in the Update list (PoolGenerator.Ensure adds it): a `generic_pools`, a `splash_pools` or the
/// BloodPuff pool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PoolRef {
    Generic(usize),
    Splash(usize),
    BloodPuff,
    /// A `recorded_pool_items` pool.
    Recorded(usize),
    /// The BulletTracerPool (a probe arg; its own Update runs it).
    Tracer,
}

/// An ObjectPoolBehaviour PoolGenerator made.
#[derive(Clone, Debug)]
pub struct PrefabPool {
    pub prefab: ObjRef,
    /// The prefab's name.
    pub name: String,
    pub MaxPoolSize: usize,
    pub pool: ObjectPool,
    pub items: Vec<PoolItem>,
}

impl Game {
    /// PoolGenerator.GetFor
    pub fn pool_get_for(&self, prefab: ObjRef) -> Option<usize> {
        self.generic_pools.iter().position(|p| p.prefab == prefab)
    }

    /// The pool of prefab `name` a replay found already made, if any.
    pub(super) fn take_recorded_pool(&mut self, name: &str) -> Option<ObjectPool> {
        let key = format!("{}_pool", name.replace(' ', "_"));
        let i = self.recorded_pools.iter().position(|(n, _)| *n == key)?;
        Some(self.recorded_pools.remove(i).1)
    }

    /// PoolGenerator.Ensure: new GameObject(prefab.name + " pool").AddComponent<ObjectPoolBehaviour>() takes the
    /// GameObject's, its Transform's and the behaviour's instance ids.
    pub fn pool_ensure(&mut self, prefab: ObjRef) -> usize {
        if let Some(i) = self.pool_get_for(prefab) {
            return i;
        }
        let name = self.assets.as_mut().expect("PoolGenerator.Ensure without the game's data").prefab(prefab).map_or_else(|e| panic!("PoolGenerator.Ensure: {e}"), |p| p.objects[0].name.clone());
        let pool = match self.take_recorded_pool(&name) {
            Some(pool) => pool,
            None => {
                for _ in 0..3 {
                    self.allocate_instance_id();
                }
                ObjectPool::default()
            }
        };
        let key = format!("{}_pool", name.replace(' ', "_"));
        self.generic_pools.push(PrefabPool { prefab, name, MaxPoolSize: POOL_SIZE, pool, items: Vec::new() });
        let s = self.generic_pools.len() - 1;
        let Some(r) = self.recorded_pool_items.iter().position(|(n, _, _)| *n == key) else {
            self.behaviours.add(0, super::Behaviour::ObjectPool(PoolRef::Generic(s)));
            return s;
        };
        // The recorded clones: Instantiated with ids far below the recording's (the counter goes back), then
        // OnPoolableInitialised and SetActive(false) as their Return left them.
        self.recorded_pool_items[r].2 = Some(s);
        let saved = self.next_instance_id;
        self.next_instance_id = saved - 0x2000_0000;
        for (pos, active) in self.recorded_pool_items[r].1.clone() {
            let root = self.spawn_prefab(prefab, pos);
            if !self.tr_active_self(Go::Local(root)) {
                self.tr_set_active(Go::Local(root), true);
            }
            self.pool_broadcast(root, s, false);
            // One still out (a probe case's): left active.
            if !active {
                self.tr_set_active(Go::Local(root), false);
            }
            self.generic_pools[s].items.push(PoolItem { root });
        }
        self.next_instance_id = saved;
        s
    }

    /// PoolGenerator.RequestPrefab(prefab, position): the pool and the item (None: the pool is full, null).
    pub fn pool_request_prefab(&mut self, prefab: ObjRef, position: [f32; 2]) -> Option<(usize, usize)> {
        let s = self.pool_ensure(prefab);
        self.pool_request(s, position).map(|i| (s, i))
    }

    /// ObjectPoolBehaviour.Request
    fn pool_request(&mut self, s: usize, position: [f32; 2]) -> Option<usize> {
        let pool = &mut self.generic_pools[s];
        pool.pool.inactivityTimer = 0.0;
        if let Some(i) = pool.pool.available.pop() {
            let root = pool.items[i].root;
            if self.destroyed_components.contains(&root) {
                // Debug.LogError (the log only), then null.
                return None;
            }
            self.with_component_instance(root, |_, inst, o| inst.set_world_position(o, [position[0], position[1], 0.0]));
            self.log_effect(self.generic_pools[s].prefab, [position[0], position[1], 0.0], [0.0, 0.0, 0.0, 1.0], 1.0, None, Some(root));
            self.tag_particle_helper(s, i);
            self.tr_set_active(Go::Local(root), true);
            self.pool_broadcast(root, s, false);
            self.pool_broadcast(root, s, true);
            return Some(i);
        }
        if pool.pool.created.len() >= pool.MaxPoolSize {
            return None;
        }
        let prefab = pool.prefab;
        let root = self.spawn_prefab(prefab, [position[0], position[1], 0.0]);
        let i = self.generic_pools[s].items.len();
        self.tag_particle_helper(s, i);
        // gameObject.SetActive(true): an inactive prefab root runs its scripts' Awake and OnEnable now.
        if !self.tr_active_self(Go::Local(root)) {
            self.tr_set_active(Go::Local(root), true);
        }
        self.pool_broadcast(root, s, false);
        let pool = &mut self.generic_pools[s];
        pool.items.push(PoolItem { root });
        let i = pool.items.len() - 1;
        pool.pool.created.push(i);
        Some(i)
    }

    /// BroadcastMessage("OnPoolableInitialised" or "OnPoolableReinitialised", pool) to the clone's active objects.
    fn pool_broadcast(&mut self, root: i32, s: usize, reinitialised: bool) {
        let receivers: Vec<(String, i32, Option<(bool, Option<usize>, i64, f32)>)> = self.with_component_instance(root, |_, inst, _| {
            let mut out = Vec::new();
            for (x, ob) in inst.prefab.objects.iter().enumerate() {
                if !inst.objects[x].active {
                    continue;
                }
                for (k, (_, c)) in ob.components.iter().enumerate() {
                    if let Component::Script { class, fields, enabled, .. } = c {
                        // ExplosionSoundBehviour.Start: Play(Delay) when StartWhenAwake; Play needs an AudioSource.
                        let sound = (class == "ExplosionSoundBehviour").then(|| {
                            let set = fields.get("audioSource").and_then(|v| v.pptr()).is_some_and(|p| !p.is_null());
                            let has = set || ob.components.iter().any(|(_, y)| matches!(y, Component::AudioSource { .. }));
                            let near = fields.get("Near").and_then(|v| v.array()).map_or(0, |a| a.len());
                            (
                                fields.get("StartWhenAwake").and_then(|v| v.bool()).unwrap_or(true) && *enabled,
                                has.then_some(near),
                                fields.get("DistantSound").and_then(|v| v.i64()).unwrap_or(0),
                                fields.get("Delay").and_then(|v| v.f32()).unwrap_or(0.0),
                            )
                        });
                        out.push((class.clone(), inst.objects[x].components[k], sound));
                    }
                }
            }
            out
        });
        for (class, id, sound) in receivers {
            match (class.as_str(), reinitialised) {
                ("ArchelixCasterBoltBehaviour", false) => {
                    let b = match self.behaviour_of.get(&id) {
                        Some(&(_, super::Behaviour::ArchelixBolt(b))) => b,
                        _ => unreachable!("ArchelixCasterBoltBehaviour in the arena"),
                    };
                    self.archelix_bolt_on_poolable_initialised(b, s);
                }
                // PoolableParticleSystemHelper.OnPoolableInitialised only stores the pool (OnParticleSystemStopped Returns to it).
                ("PoolableParticleSystemHelper", false) => {}
                ("ReturnToPoolAfterTime", false) => self.return_to_pool_on_poolable_initialised(id, PoolRef::Generic(s)),
                ("PoolableSizeHelper", true) => self.poolable_size_helper_on_reinitialised(id),
                // OnPoolableReinitialised: Start(), i.e. Play(Delay) of a script that is enabled.
                ("ExplosionSoundBehviour", true) => {
                    let (start, near, distant, delay) = sound.expect("an ExplosionSoundBehviour's fields");
                    if start && self.script_enabled(id, true) {
                        if let Some(n) = near {
                            let k = self.pick_random(n);
                            self.explosion_sound_play_clip(id, k, delay);
                            if self.prefs.DistantSoundEffects && distant != 0 {
                                self.start_wait(id, delay, super::destroyable::Step::DistantSound(distant));
                            }
                        }
                    }
                }
                ("PoolOriginBehaviour", false) | ("FancyBloodSplatController", true) => {
                    unimplemented!("mod-only: {class}.OnPoolable{}initialised in a pooled script clone", if reinitialised { "Re" } else { "" })
                }
                _ => {}
            }
        }
    }

    /// ReturnToPoolAfterTime.OnPoolableInitialised: time = 0, pool = the pool.
    pub(crate) fn return_to_pool_on_poolable_initialised(&mut self, id: i32, pool: PoolRef) {
        let r = self.return_to_pools.iter_mut().find(|r| r.id == id).unwrap();
        r.time = 0.0;
        r.pool = Some(pool);
    }

    /// ReturnToPoolAfterTime.Update: time += deltaTime; past Life, pool.Return(gameObject).
    pub fn return_to_pool_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let r = &mut self.return_to_pools[i];
        r.time += dt;
        if r.time > r.Life {
            let (pool, id) = (r.pool.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), r.id);
            match pool {
                PoolRef::Generic(s) => self.pool_return(s, id),
                PoolRef::Recorded(k) if self.recorded_pool_items[k].2.is_some() => self.pool_return(self.recorded_pool_items[k].2.unwrap(), id),
                // Return's `!createdObjects.Contains(poolable)`: Debug.LogError (the log only). These pools' clones are
                // render-only (no ReturnToPoolAfterTime of their own), and a recorded pool not yet Ensured has none.
                PoolRef::Splash(_) | PoolRef::BloodPuff | PoolRef::Tracer | PoolRef::Recorded(_) => {}
            }
        }
    }

    /// PoolableSizeHelper.Awake: localScaleAtStart = transform.localScale (once).
    pub fn poolable_size_helper_awake(&mut self, id: i32, scale: [f32; 3]) {
        match self.poolable_size_helpers.iter_mut().find(|h| h.0 == id) {
            Some(h) => h.1 = h.1.or(Some(scale)),
            None => self.poolable_size_helpers.push((id, Some(scale))),
        }
    }

    /// PoolableSizeHelper.OnPoolableReinitialised: transform.localScale = localScaleAtStart.
    pub(crate) fn poolable_size_helper_on_reinitialised(&mut self, id: i32) {
        let scale = self.poolable_size_helpers.iter().find(|h| h.0 == id).and_then(|h| h.1).unwrap();
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, scale));
    }

    /// PoolableParticleSystemHelper.Awake set the root ParticleSystem's stopAction to Callback: the clone just logged
    /// returns through OnParticleSystemStopped (the pool is set at Request). Unpooled, the callback would throw;
    /// the game only Requests these prefabs.
    pub(crate) fn tag_particle_helper(&mut self, s: usize, i: usize) {
        let prefab = self.generic_pools[s].prefab;
        let helper = self.assets.as_mut().expect("the game's data").prefab(prefab).is_ok_and(|p| {
            p.objects[0].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "PoolableParticleSystemHelper"))
        });
        if helper {
            self.tag_pooled(super::Pooled::Generic(s, i));
        }
    }

    /// PoolableParticleSystemHelper.OnParticleSystemStopped: pool.Return(gameObject); without the pool
    /// OnPoolableInitialised gave (an object no generic pool made) it throws. `id` is the helper's instance id.
    pub fn poolable_particle_stopped(&mut self, id: i32) {
        let root = self.with_component_instance(id, |_, inst, _| inst.objects[0].components[0]);
        let Some(s) = self.generic_pools.iter().position(|p| p.items.iter().any(|x| x.root == root)) else {
            super::menu::throw("Exception: Poolable object is unpooled!")
        };
        self.pool_return(s, id);
    }

    /// ObjectPoolBehaviour.Return: SetActive(false), then availableObjects.Push.
    /// `go` is the instance id of any component of the pooled object's root GameObject.
    pub fn pool_return(&mut self, s: usize, go: i32) {
        let root = self.with_component_instance(go, |_, inst, _| inst.objects[0].components[0]);
        let pool = &mut self.generic_pools[s];
        // `!poolable || !createdObjects.Contains(poolable)`: Debug.LogError (the log only), nothing returned.
        let Some(i) = pool.items.iter().position(|x| x.root == root).filter(|i| pool.pool.created.contains(i)) else { return };
        self.tr_set_active(Go::Local(root), false);
        self.generic_pools[s].pool.available.push(i);
    }

    /// The pool behind a PoolGenerator pool's GameObject (`<prefab name> pool`, spaces as '_').
    pub fn pool_named(&mut self, name: &str) -> Option<&mut ObjectPool> {
        let key = |n: &str| format!("{}_pool", n.replace(' ', "_"));
        if let Some(i) = self.recorded_pools.iter().position(|(n, _)| n == name) {
            return Some(&mut self.recorded_pools[i].1);
        }
        if name == "BloodPuff_poolable_pool" {
            return self.blood_puff_pool.as_mut();
        }
        if let Some(i) = self.generic_pools.iter().position(|p| key(&p.name) == name) {
            return Some(&mut self.generic_pools[i].pool);
        }
        let i = self.splash_pools.iter().position(|p| key(&p.prefab.objects[0].name) == name)?;
        Some(&mut self.splash_pools[i].pool)
    }

    /// ObjectPoolBehaviour.Clear of every pool (Global.ClearMap's FindObjectsOfType<ObjectPoolBehaviour>): each created
    /// clone is destroyed and both sets are emptied. The inactivity timer keeps counting (Request resets it).
    pub fn object_pools_clear(&mut self) {
        // The PoolGenerator pools: the clones are scripted objects.
        for s in 0..self.generic_pools.len() {
            let pool = &mut self.generic_pools[s];
            let roots: Vec<i32> = pool.pool.created.iter().map(|&i| pool.items[i].root).collect();
            pool.pool.created.clear();
            pool.pool.available.clear();
            for root in roots {
                self.destroy_object(root);
            }
        }
        // The Splash pools, the BloodPuff pool and the BulletTracerPool: render-only clones, only their counts matter.
        for pool in &mut self.splash_pools {
            for &i in &pool.pool.created {
                pool.items[i].destroyed = true;
            }
            pool.pool.created.clear();
            pool.pool.available.clear();
        }
        if let Some(pool) = &mut self.blood_puff_pool {
            for &b in &pool.created {
                self.splats[self.blood_puffs[b].splat].destroyed = true;
            }
            pool.created.clear();
            pool.available.clear();
        }
        self.tracer_pool.created = 0;
        self.tracer_pool.available = 0;
    }

    /// ObjectPoolBehaviour.Update (GetPoolSize() != 0, then availableObjects.Count == createdObjects.Count): Clear (Object.Destroy
    /// of every clone) after InactivityThresholdInSeconds (300, PoolGenerator leaves it) with every object back.
    pub fn object_pool_update(&mut self, k: PoolRef) {
        let dt = self.time.deltaTime;
        let pool = match k {
            PoolRef::Generic(s) => &mut self.generic_pools[s].pool,
            PoolRef::Splash(s) => &mut self.splash_pools[s].pool,
            PoolRef::BloodPuff => self.blood_puff_pool.as_mut().expect("the BloodPuff pool"),
            PoolRef::Recorded(r) => match self.recorded_pool_items[r].2 {
                Some(s) => return self.object_pool_update(PoolRef::Generic(s)),
                None => {
                    let name = self.recorded_pool_items[r].0.clone();
                    &mut self.recorded_pools.iter_mut().find(|(n, _)| *n == name).expect("a recorded pool").1
                }
            },
            PoolRef::Tracer => unreachable!("the BulletTracerPool is no Behaviour::ObjectPool"),
        };
        if pool.get_pool_size() == 0 {
            return;
        }
        pool.inactivityTimer += dt;
        if !(pool.inactivityTimer > 300.0 && pool.available.len() == pool.created.len()) {
            return;
        }
        let created = std::mem::take(&mut pool.created);
        pool.available.clear();
        pool.inactivityTimer = 0.0;
        for i in created {
            match k {
                PoolRef::Generic(s) => {
                    let root = self.generic_pools[s].items[i].root;
                    self.destroy_object(root);
                }
                // Render-only clones: only their counts matter.
                PoolRef::Splash(s) => self.splash_pools[s].items[i].destroyed = true,
                PoolRef::BloodPuff => self.splats[self.blood_puffs[i].splat].destroyed = true,
                PoolRef::Recorded(_) => unimplemented!("recording harness: a recorded pool's 300 s Clear before Ensure"),
                PoolRef::Tracer => unreachable!("the BulletTracerPool is no Behaviour::ObjectPool"),
            }
        }
    }
}
