//! FirearmBehaviour (the guns).
#![allow(non_snake_case)]

use super::ballistics::{Cartridge, Emitter};
use super::misc::FixedIntervalDistributor;
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;

script! {
    Firearm {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        casingPosition: [f32; 2] = [0.0; 2],
        casingDirection: [f32; 2] = [0.0; 2],
        FireDelay: f32 = 0.0,
        InitialInaccuracy: f32 = 0.001,
        Automatic: bool = false,
        BulletsPerShot: i32 = 1,
        AutomaticFireInterval: f32 = 0.05,
        IgnoreUse: bool = false,
        ApplyCharge: bool = false,
        ShotHeatMultiplier: f32 = 1.0,
        MuzzleShockwave: bool = true,
        EjectShells: bool = true,
        DistantSound: i32 = 0,
    }
    links {
        /// The component's instance id (its coroutines' owner).
        id: i32 = 0,
        enabled: bool = true,
        /// Phys (GetComponent<PhysicalBehaviour>); None without one.
        physical: Option<usize> = None,
        /// Without a PhysicalBehaviour: the Rigidbody2D's NodeBody (`Game::node_rigidbodies`) and its Transform node.
        node_body: Option<usize> = None,
        node: Option<usize> = None,
        /// ShotVolume, and the AudioSource Awake adds (its component id).
        ShotVolume: f32 = 1.0,
        audio_source: i32 = 0,
        /// Instance ids of the Prefabs/BulletCasing clone, and of the Prefabs/MuzzleSmoke one (0 with a
        /// MuzzleFlashEffectOverride).
        casing_ids: usize = 0,
        /// Prefabs/BulletCasing, the clone `bulletCasing` plays.
        casing_prefab: Option<ObjRef> = None,
        /// The Prefabs/BulletCasing and Prefabs/MuzzleSmoke hierarchies (the runtime children Awake registers), and the casing clone's
        /// root Transform id.
        casing_clone: Option<std::sync::Arc<crate::assets::db::Prefab>> = None,
        smoke_clone: Option<std::sync::Arc<crate::assets::db::Prefab>> = None,
        casing: Option<i32> = None,
        smoke_ids: usize = 0,
        /// The Prefabs/MuzzleSmoke clone's root Transform (`muzzleTransform`), local to the gun: Awake instantiates it
        /// under the gun with the prefab's local pose; Shoot sets its scale, right and position. Its ParticleSystems
        /// are embedded systems with an `EmbeddedDrive::Muzzle` drive.
        muzzle: Option<crate::unity::transform::Trs> = None,
        shotIndex: i32 = 0,
        clock: FixedIntervalDistributor = FixedIntervalDistributor::with_rate(60.0),
        /// ShotSounds length.
        ShotSounds: usize = 0,
        InaccuracyMultipliers: Vec<f32> = Vec::new(),
        /// The attachments in BallisticsEmitter.BulletEntryEvent and OnFire (added and removed together), in order.
        attachments: Vec<usize> = Vec::new(),
        MuzzleFlashEffectOverride: Option<ObjRef> = None,
        /// StandardParticleOverride (1.28.52): Awake instantiates it in place of Prefabs/MuzzleSmoke when set.
        StandardParticleOverride: Option<ObjRef> = None,
        Cartridge: Cartridge = Cartridge::default(),
        /// LayersToHit (Awake's WaterLayer is Water | LayersToHit).
        LayersToHit: u32 = 0,
        /// BallisticsEmitter (Start).
        emitter: Option<Emitter> = None,
    }
}

impl Game {
    /// FirearmBehaviour.Awake: Instantiate(Prefabs/BulletCasing, transform) (particles only), AddComponent<AudioSource>,
    /// clock.RateHz, and Instantiate(Prefabs/MuzzleSmoke, transform) without an override; the rest sets up audio and
    /// particles.
    pub fn firearm_awake(&mut self, f: usize) {
        let casing: Vec<i32> = (0..self.firearms[f].casing_ids).map(|_| self.allocate_instance_id()).collect();
        // AddComponent<AudioSource>: spread 35, volume ShotVolume, minDistance 15, spatialBlend 1, dopplerLevel 0. 1.28.45 sets the
        // volume in Start instead (the AudioSource's own 1 until then).
        let id = self.allocate_instance_id();
        self.firearms[f].audio_source = id;
        let (anchor, ancestors) = self.anchor_ancestors(self.firearms[f].physical, self.firearms[f].node);
        let volume = if crate::assets::il::is_1_28_45() { 1.0 } else { self.firearms[f].ShotVolume };
        let mut s = super::audio::AudioSource { anchor, ancestors, volume, min_distance: 15.0, doppler_level: 0.0, ..Default::default() };
        s.set_spatial_blend(1.0);
        s.set_spread(35.0);
        self.audio.add(id, s);
        let interval = self.firearms[f].AutomaticFireInterval;
        // RateHz = 1f / AutomaticFireInterval: IntervalSeconds = 1f / (1f / AutomaticFireInterval).
        self.firearms[f].clock.set_RateHz(1.0 / interval);
        let smoke: Vec<i32> = (0..self.firearms[f].smoke_ids).map(|_| self.allocate_instance_id()).collect();
        // Both clones are real children of the gun (save.rs RuntimeChild), the casing first.
        let owner = match (self.firearms[f].physical, self.firearms[f].node) {
            (Some(p), _) => self.physicals[p].transform_id,
            (None, Some(n)) => self.transform_nodes[n].id,
            (None, None) => return,
        };
        if let Some(prefab) = self.firearms[f].casing_clone.clone() {
            let c = Self::runtime_instance(prefab, &casing, None);
            self.firearms[f].casing = Some(c.objects[0].components[0]);
            self.runtime_child_add(owner, super::save::RuntimeKind::Plain, Some(c));
        }
        if let Some(prefab) = self.firearms[f].smoke_clone.clone().filter(|_| !smoke.is_empty()) {
            let c = Self::runtime_instance(prefab, &smoke, None);
            self.runtime_child_add(owner, super::save::RuntimeKind::Muzzle(f), Some(c));
        }
    }

    /// FirearmBehaviour.Awake without a MuzzleFlashEffectOverride: muzzleTransform = Instantiate(Prefabs/MuzzleSmoke,
    /// transform) (1.28.52: Instantiate(StandardParticleOverride ? StandardParticleOverride : Prefabs/MuzzleSmoke, transform)),
    /// flash = its GetComponentInChildren<ParticleSystem>(). The systems Play(withChildren) starts (flash
    /// and the ParticleSystems below it) become embedded systems that follow the clone's root.
    pub(crate) fn firearm_muzzle_smoke(&mut self, db: &mut crate::assets::db::AssetDb, f: usize) -> Result<(), String> {
        use crate::assets::db::Component;
        use crate::unity::transform::Trs;
        if self.firearms[f].MuzzleFlashEffectOverride.is_some() {
            return Ok(());
        }
        let r = match self.firearms[f].StandardParticleOverride {
            Some(r) => r,
            None => db.resource("Prefabs/MuzzleSmoke", 1)?.ok_or("Resources.Load(Prefabs/MuzzleSmoke) found no GameObject")?,
        };
        let prefab = db.prefab(r)?;
        self.firearms[f].smoke_clone = Some(std::sync::Arc::new(prefab.clone()));
        let trs = |o: usize| match &prefab.objects[o].components[0].1 {
            Component::Transform(t) => Trs { t: [t.local_position[0], t.local_position[1], t.local_position[2], 0.0], q: t.local_rotation, s: [t.local_scale[0], t.local_scale[1], t.local_scale[2], 0.0] },
            _ => unreachable!("a GameObject's first component is its Transform"),
        };
        let is_ps = |o: usize| prefab.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Other(198)));
        // GetComponentInChildren (includeInactive false): depth first from the root, active objects only.
        let active = |mut o: usize| loop {
            if !prefab.objects[o].active {
                break false;
            }
            match prefab.objects[o].parent {
                Some(up) => o = up,
                None => break true,
            }
        };
        let flash = (0..prefab.objects.len()).find(|&o| is_ps(o) && active(o)).ok_or("MuzzleSmoke without a ParticleSystem (NullReferenceException in Shoot)")?;
        let below = |mut o: usize| loop {
            if o == flash {
                break true;
            }
            match prefab.objects[o].parent {
                Some(up) => o = up,
                None => break false,
            }
        };
        let fa = &self.firearms[f];
        let anchor = match (fa.physical, fa.node) {
            (Some(p), _) => super::BareAnchor::Physical(p),
            (None, Some(n)) => super::BareAnchor::Node(n),
            (None, None) => unreachable!("a FirearmBehaviour without a physical has its Rigidbody2D's node"),
        };
        let owner = fa.id;
        self.firearms[f].muzzle = Some(trs(0));
        for o in (0..prefab.objects.len()).filter(|&o| is_ps(o) && below(o)) {
            // The object's Transform up to the clone's root (the root's own pose is `muzzle`, put last by the drive).
            let mut chain = Vec::new();
            let mut up = Some(o);
            while let Some(x) = up {
                chain.push(trs(x));
                up = prefab.objects[x].parent;
            }
            self.embedded_ps.push(super::EmbeddedPs { go: prefab.objects[o].game_object, anchor, chain, owner, drive: super::EmbeddedDrive::Muzzle(f), shape_texture: None, awake: true, emission: None });
        }
        Ok(())
    }

    /// The muzzle smoke system's world pose: its chain under the gun with the clone root's current local pose.
    pub(crate) fn muzzle_ps_world(&self, f: usize, e: &super::EmbeddedPs) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let mut chain = e.chain.clone();
        *chain.last_mut().expect("the clone's root") = self.firearms[f].muzzle.expect("Awake's muzzleTransform");
        self.anchored_world(e.anchor, &chain)
    }

    /// FirearmBehaviour.Start: the BallisticsEmitter and SetTraversalSettings.
    pub fn firearm_start(&mut self, f: usize) {
        // 1.28.45: audioSource.volume = gameObject.HasComponent<RotatingSoundBehaviour>() ? 0 : ShotVolume.
        if crate::assets::il::is_1_28_45() {
            let id = self.firearms[f].id;
            let rotating = self.with_component_instance(id, |_, inst, o| {
                inst.prefab.objects[o].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "RotatingSoundBehaviour"))
            });
            let volume = if rotating { 0.0 } else { self.firearms[f].ShotVolume };
            let source = self.firearms[f].audio_source;
            self.audio.get_mut(source).volume = volume;
        }
        let g = &self.firearms[f];
        let layers = g.LayersToHit;
        self.firearms[f].emitter = Some(Emitter::new(g.Cartridge, layers, (1 << 4) | layers));
        self.firearm_set_traversal(f);
        self.firearms[f].emitter.as_mut().unwrap().ricochet_callback = true;
    }

    /// FirearmBehaviour.SetTraversalSettings
    pub(crate) fn firearm_set_traversal(&mut self, f: usize) {
        let tracers = self.prefs.TracerBullets;
        let speed = self.firearms[f].Cartridge.StartSpeed;
        let e = self.firearms[f].emitter.as_mut().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        if tracers {
            e.MaxRange = mathf::clamp(speed / 4.0, 4.0, 8.0);
            e.DoTraversal = true;
        } else {
            e.MaxRange = 1000.0;
            e.DoTraversal = false;
        }
    }

    /// FirearmBehaviour.Update
    pub fn firearm_update(&mut self, f: usize) {
        let g = &mut self.firearms[f];
        if g.Automatic && g.AutomaticFireInterval > 0.0 {
            g.clock.IntervalSeconds = g.AutomaticFireInterval;
        }
        let g = &self.firearms[f];
        if g.Automatic && g.physical.is_some_and(|p| self.used_continuously(p)) && !g.IgnoreUse && g.shotIndex != 0 {
            self.firearm_shoot_continuous(f);
        }
    }

    /// FirearmBehaviour.ShootContinuous: the loop condition calls CalculateCycleCount(Time.deltaTime) on every pass.
    pub fn firearm_shoot_continuous(&mut self, f: usize) {
        if self.firearms[f].Automatic {
            let mut i = 0;
            while i < self.firearms[f].clock.calculate_cycle_count(self.time.deltaTime) {
                self.firearm_shoot(f);
                i += 1;
            }
        }
    }

    /// FirearmBehaviour.ResetIndex
    pub fn firearm_reset_index(&mut self, f: usize) {
        self.firearms[f].shotIndex = 0;
    }

    /// FirearmBehaviour.Use
    pub fn firearm_use(&mut self, f: usize) {
        if self.firearms[f].IgnoreUse {
            return;
        }
        self.firearms[f].shotIndex = 0;
        if self.firearms[f].FireDelay > mathf::EPSILON {
            // FirearmBehaviour.DelayedShot: audioSource.PlayOneShot(TriggerSound, 1f) (shotIndex is 0), then
            // WaitForSeconds(FireDelay) and Shoot (Step::FirearmShoot)
            let (id, delay) = (self.firearms[f].id, self.firearms[f].FireDelay);
            let clip = self.audio.clip(id, "TriggerSound");
            self.audio.play_one_shot(self.firearms[f].audio_source, clip.as_ref(), 1.0);
            self.start_wait(id, delay, super::destroyable::Step::FirearmShoot(f));
            return;
        }
        self.firearms[f].clock.reset_accumulator(0.0);
        self.firearm_shoot(f);
    }

    /// GetComponent<Rigidbody2D>(): the physical object's body, else the bare body's NodeBody.
    pub(super) fn firearm_rigidbody(&self, f: usize) -> Option<super::Rigidbody> {
        let g = &self.firearms[f];
        match (g.physical, g.node_body) {
            (Some(p), _) => Some(self.physicals[p].rigidbody.clone()),
            (None, Some(n)) => Some(self.node_rigidbodies[n].rigidbody.clone()),
            (None, None) => None,
        }
    }

    /// The Transform chain of the firearm's object, root first: a physical object's (local_chain), or a bare node's.
    pub(super) fn firearm_chain(&self, f: usize) -> Vec<crate::unity::transform::Trs> {
        let g = &self.firearms[f];
        match (g.physical, g.node) {
            (Some(p), _) => self.local_chain(p),
            (None, Some(n)) => self.node_chain(n),
            (None, None) => unreachable!("a firearm has its physical or, from link, a node: link stops before pushing one with neither"),
        }
    }

    /// The Transform id of the firearm object's root (root_id for a physical object, the top node's for a bare one).
    pub(super) fn firearm_root_id(&self, f: usize) -> i32 {
        match (self.firearms[f].physical, self.firearms[f].node) {
            (Some(p), _) => self.root_id(p),
            (None, Some(n)) => {
                let mut t = n;
                while let Some(up) = self.transform_nodes[t].parent {
                    t = up;
                }
                self.transform_nodes[t].id
            }
            (None, None) => unreachable!("a firearm has its physical or, from link, a node: link stops before pushing one with neither"),
        }
    }

    /// A transform node's world position, rotation and lossy scale (from its chain).
    pub(super) fn node_pose(&self, n: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let chain = self.node_chain(n);
        let mut v = [0.0f32; 4];
        for node in chain.iter().rev() {
            v = crate::unity::transform::global_position(node, v);
        }
        let mut q = chain.last().unwrap().q;
        for node in chain.iter().rev().skip(1) {
            q = crate::unity::transform::global_rotation(node, q);
        }
        let mut direct_first = chain.clone();
        direct_first.reverse();
        let lossy = crate::unity::transform::lossy_scale(&direct_first, q);
        ([v[0], v[1], v[2]], q, lossy)
    }

    /// A bare GameObject's chain: its parent nodes root first, then its own pose (node_trs).
    pub(super) fn node_chain(&self, n: usize) -> Vec<crate::unity::transform::Trs> {
        let mut chain = Vec::new();
        let mut up = self.transform_nodes[n].parent;
        while let Some(i) = up {
            chain.push(self.node_trs(i));
            up = self.transform_nodes[i].parent;
        }
        chain.reverse();
        chain.push(self.node_trs(n));
        chain
    }

    /// The firearm's world position, world rotation and lossy scale: a physical object's cached transform, else from its chain.
    pub(super) fn firearm_pose(&self, f: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
        match self.firearms[f].physical {
            Some(p) => (self.physicals[p].transform.position, self.physicals[p].transform.rotation, self.lossy_scale(p)),
            None => {
                let chain = self.firearm_chain(f);
                let mut v = [0.0f32; 4];
                for node in chain.iter().rev() {
                    v = crate::unity::transform::global_position(node, v);
                }
                let mut q = chain.last().unwrap().q;
                for node in chain.iter().rev().skip(1) {
                    q = crate::unity::transform::global_rotation(node, q);
                }
                let mut direct_first = chain.clone();
                direct_first.reverse();
                let lossy = crate::unity::transform::lossy_scale(&direct_first, q);
                ([v[0], v[1], v[2]], q, lossy)
            }
        }
    }

    /// FirearmBehaviour.BarrelPosition
    /// BarrelPosition: rb.GetRelativePoint(barrelPosition * lossyScale), or TransformPoint(barrelPosition) without a rigidbody.
    pub(crate) fn barrel_position(&mut self, f: usize) -> [f32; 2] {
        let local = self.firearms[f].barrelPosition;
        match self.firearm_rigidbody(f) {
            Some(rb) => {
                let (_, _, lossy) = self.firearm_pose(f);
                self.rigidbody_point_relative(&rb, [local[0] * lossy[0], local[1] * lossy[1]])
            }
            None => {
                let chain = self.firearm_chain(f);
                let mut v = [local[0], local[1], 0.0, 0.0];
                for node in chain.iter().rev() {
                    v = crate::unity::transform::global_position(node, v);
                }
                [v[0], v[1]]
            }
        }
    }

    /// Rigidbody2D.GetRelativePoint (180c1fba0) of body `rb` for a scaled local point: Vector2f.zero without a b2Body.
    pub(super) fn rigidbody_point_relative(&mut self, rb: &super::Rigidbody, v: [f32; 2]) -> [f32; 2] {
        self.sync_pending();
        if rb.detached.is_some() {
            return [0.0, 0.0];
        }
        let xf = self.world.body(rb.body).xf;
        [xf.q.c * v[0] - xf.q.s * v[1] + xf.p.x, xf.q.s * v[0] + xf.q.c * v[1] + xf.p.y]
    }

    /// Rigidbody2D.GetRelativePoint(local * transform.lossyScale) of physical `p`'s body (shared by the barrels).
    pub(super) fn rigidbody_relative_point(&mut self, p: usize, local: [f32; 2]) -> [f32; 2] {
        let s = self.lossy_scale(p);
        let v = [local[0] * s[0], local[1] * s[1]];
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        [xf.q.c * v[0] - xf.q.s * v[1] + xf.p.x, xf.q.s * v[0] + xf.q.c * v[1] + xf.p.y]
    }

    /// FirearmBehaviour.get_CasingPosition: transform.TransformPoint(casingPosition) over the firearm's chain.
    pub(crate) fn casing_position(&self, f: usize) -> [f32; 2] {
        let local = self.firearms[f].casingPosition;
        let mut v = [local[0], local[1], 0.0, 0.0];
        for node in self.firearm_chain(f).iter().rev() {
            v = crate::unity::transform::global_position(node, v);
        }
        [v[0], v[1]]
    }

    /// FirearmBehaviour.get_CasingDirection: transform.TransformDirection(casingDirection), the world rotation without scale.
    pub(crate) fn casing_direction(&self, f: usize) -> [f32; 2] {
        let [_, _, qz, qw] = self.firearm_pose(f).1;
        let (c, s) = (1.0 - 2.0 * qz * qz, 2.0 * qw * qz);
        let l = self.firearms[f].casingDirection;
        [c * l[0] - s * l[1], s * l[0] + c * l[1]]
    }

    /// FirearmBehaviour.BarrelDirection
    /// BarrelDirection: transform.TransformVector(barrelDirection) (each node's scale and rotation, no translation).
    pub(crate) fn barrel_direction(&self, f: usize) -> [f32; 2] {
        let chain = self.firearm_chain(f);
        Self::transform_vector_of(&chain, self.firearms[f].barrelDirection)
    }

    /// transform.TransformVector(local) of physical `p`'s object: each node's scale and rotation, no translation.
    pub(super) fn transform_vector_chain(&self, p: usize, local: [f32; 2]) -> [f32; 2] {
        Self::transform_vector_of(&self.local_chain(p), local)
    }

    /// TransformVector over a chain (root first).
    pub(super) fn transform_vector_of(chain: &[crate::unity::transform::Trs], local: [f32; 2]) -> [f32; 2] {
        let mut v = [local[0], local[1], 0.0, 0.0];
        for node in chain.iter().rev() {
            let node = crate::unity::transform::Trs { t: [0.0; 4], ..*node };
            v = crate::unity::transform::global_position(&node, v);
        }
        [v[0], v[1]]
    }

    /// FirearmBehaviour.Shoot
    pub fn firearm_shoot(&mut self, f: usize) {
        if !self.firearms[f].enabled {
            return;
        }
        // ModdingApi.Events.OnGunShot.Invoke(this)
        self.mod_event(&super::mods::mod_api::OnGunShot, self.firearms[f].id);
        let g = &self.firearms[f];
        let first = g.shotIndex == 0;
        // audioSource.PlayOneShot(ShotSounds.PickRandom(), Automatic ? (first ? 1 : 0.5) : 1)
        if g.ShotSounds != 0 {
            let volume = if g.Automatic && !first { 0.5 } else { 1.0 };
            let (id, n, source) = (g.id, g.ShotSounds, g.audio_source);
            let clip = self.pick_clip(id, "ShotSounds", n);
            self.audio.play_one_shot(source, clip.as_ref(), volume);
        }
        let kind = self.firearms[f].DistantSound;
        if self.prefs.DistantSoundEffects && kind != 0 {
            self.distant_sound_play(kind as i64, 0.5);
        }
        if first {
            self.firearm_set_traversal(f);
        }
        let c = self.firearms[f].Cartridge;
        // (bool)Phys && (bool)Cartridge: the heat only with a PhysicalBehaviour.
        if let Some(p) = self.firearms[f].physical {
            let hc = self.heat_capacity(p);
            let heat = self.firearms[f].ShotHeatMultiplier;
            let t = self.physicals[p].Temperature;
            self.physicals[p].Temperature = (d(t) + d(c.Recoil) / hc * 0.05f32 * heat).f();
        }
        self.firearms[f].shotIndex += 1;
        if self.firearms[f].MuzzleShockwave {
            let at = self.barrel_position(f);
            self.create_pulse_explosion([at[0], at[1], 0.0], c.Recoil / 25.0, c.Recoil / 3.0);
        }
        let n = self.firearms[f].BulletsPerShot;
        match self.firearms[f].MuzzleFlashEffectOverride {
            None => {
                // transform = muzzleTransform; localScale = one * Clamp(Pow(Damage * BulletsPerShot, 0.6) / 4, 0.5, 2.5),
                // x negated for a mirrored gun (lossyScale.x <= 0).
                let k = mathf::clamp((d(c.Damage) * n as f32).f().powf(0.6) / 4.0, 0.5, 2.5);
                let (at, _, lossy) = self.firearm_pose(f);
                let sx = if lossy[0] > 0.0 { 1.0 } else { -1.0 };
                let mut m = self.firearms[f].muzzle.expect("Awake's muzzleTransform");
                m.s = [sx * k, k, k, 0.0];
                self.firearms[f].muzzle = Some(m);
                let intensity = (d(c.ImpactForce) * 0.8f32 * n as f32 / 0.9f32).f();
                self.camera_shake(intensity, [at[0], at[1]]);
                // flash.Play(withChildren: true), then transform.right and transform.position below (Play only marks
                // the systems; they take the pose at their update).
                self.ps_call(|x| matches!(x.drive, super::EmbeddedDrive::Muzzle(g) if g == f), super::PsCall::Play);
                // transform.right = BarrelDirection (rotation = FromToRotation(right, value)); transform.position = BarrelPosition.
                let dir = self.barrel_direction(f);
                let rot = crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], [dir[0], dir[1], 0.0]);
                let pos = self.barrel_position(f);
                let fa = &self.firearms[f];
                let anchor = match (fa.physical, fa.node) {
                    (Some(p), _) => super::BareAnchor::Physical(p),
                    (_, n) => super::BareAnchor::Node(n.expect("the Rigidbody2D's node")),
                };
                // Both setters store local values under the gun (the position's does not depend on the rotation).
                // anchored_nodes: the gun's own Transform first, the root last, as set_world takes them.
                let parents = self.anchored_nodes(anchor, &[]);
                let ((t, q), _) = crate::unity::transform::set_world(&parents, [pos[0], pos[1], 0.0], rot);
                (m.t, m.q) = ([t[0], t[1], t[2], 0.0], q);
                self.firearms[f].muzzle = Some(m);
            }
            Some(effect) => {
                // Instantiate(MuzzleFlashEffectOverride, BarrelPosition, rotation).transform.right = BarrelDirection (link
                // checks it has no physics); transform.position = BarrelPosition again.
                let at = self.barrel_position(f);
                let id = self.spawn_prefab(effect, [at[0], at[1], 0.0]);
                let dir = self.barrel_direction(f);
                self.tr_set_right(super::translated::rt::Xf::Object(id), [dir[0], dir[1], 0.0]);
            }
        }
        // bulletCasing.transform.position = CasingPosition; .right = CasingDirection; Play(): the casing system is drawn by
        // particles.rs from this effect (a 0.05 s burst of the clone's one ParticleSystem).
        if self.firearms[f].EjectShells {
            // The casing clone's local pose under the gun (a runtime child the saves hold).
            if let Some(tid) = self.firearms[f].casing {
                let (at, dir) = (self.casing_position(f), self.casing_direction(f));
                let rot = crate::unity::transform::from_to_rotation([1.0, 0.0, 0.0], [dir[0], dir[1], 0.0]);
                let fa = &self.firearms[f];
                let anchor = match (fa.physical, fa.node) {
                    (Some(p), _) => super::BareAnchor::Physical(p),
                    (_, n) => super::BareAnchor::Node(n.expect("the Rigidbody2D's node")),
                };
                let parents = self.anchored_nodes(anchor, &[]);
                let ((t, q), _) = crate::unity::transform::set_world(&parents, [at[0], at[1], 0.0], rot);
                for c in self.runtime_children.values_mut().flatten() {
                    if let Some(i) = c.inst.as_mut().filter(|i| i.objects[0].components[0] == tid) {
                        (i.objects[0].trs.t, i.objects[0].trs.q) = ([t[0], t[1], t[2], 0.0], q);
                    }
                }
            }
        }
        if self.firearms[f].EjectShells && !self.effects_off {
            if let Some(prefab) = self.firearms[f].casing_prefab {
                let at = self.casing_position(f);
                let dir = self.casing_direction(f);
                let a = dir[1].atan2(dir[0]) * 0.5;
                self.effect_log.push(super::EffectSpawn { prefab, position: [at[0], at[1], 0.0], rotation: [0.0, 0.0, a.sin(), a.cos()], scale: 1.0, color: None, root: None, subtree: None, water: None, pooled: None, parent: None, off: Vec::new(), tint_all: false });
            }
        }
        for _ in 0..n {
            self.firearm_fire(f);
        }
        // OnFire?.Invoke()
        self.invoke_event(self.firearms[f].id, "OnFire");
    }

    /// FirearmBehaviour.GetInaccuracyMultiplier: the product of InaccuracyMultipliers, starting at 1.
    pub fn firearm_inaccuracy_multiplier(&self, f: usize) -> f32 {
        let mut num = 1.0f32;
        for &x in &self.firearms[f].InaccuracyMultipliers {
            num *= x;
        }
        num
    }

    /// FirearmBehaviour.Fire
    pub(crate) fn firearm_fire(&mut self, f: usize) {
        let recoil = self.firearms[f].Cartridge.Recoil;
        let (_, rot, lossy) = self.firearm_pose(f);
        let s = (d(0.5f32) * (0.0 - recoil) * lossy[0]).f();
        let right = mathf::rotate(rot, [1.0, 0.0, 0.0]);
        let at = self.barrel_position(f);
        // rb.AddForceAtPosition: a null rb throws.
        let rb = self.firearm_rigidbody(f).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        rb.add_impulse_at_position(&mut self.world, [s * right[0], s * right[1]], at);
        let charge = if self.firearms[f].ApplyCharge {
            // BallisticsEmitter.Charge = Phys.Charge: a null Phys throws.
            match self.firearms[f].physical {
                Some(p) => self.physicals[p].Charge(),
                None => crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"),
            }
        } else {
            0.0
        };
        self.firearms[f].emitter.as_mut().unwrap().Charge = charge;
        let origin = self.barrel_position(f);
        let direction = self.barrel_direction(f);
        let spread = mathf::max(0.0025, self.firearms[f].InitialInaccuracy * self.firearm_inaccuracy_multiplier(f));
        let r = self.random.inside_unit_circle();
        self.emitter_emit(super::ballistics::Shooter::Firearm(f), origin, [direction[0] + r[0] * spread, direction[1] + r[1] * spread]);
    }
}
