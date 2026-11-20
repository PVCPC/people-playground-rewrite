//! DestroyableBehaviour, and Object.Destroy of a spawned GameObject.
#![allow(non_snake_case)]

use super::{d, mathf, script, BodyOwner, Game};
use crate::assets::db::{Component, ObjRef};
use crate::b2::contacts2d::ScriptCollision;

script! {
    Destroyable {
        OverallChance: f32 = 1.0,
        ShotBreakChance: f32 = 1.0,
        MinimumImpactForce: f32 = 5.0,
        MinimumShotDamage: f32 = 15.0,
        MinimumExplosionFragmentDamage: f32 = 2.0,
        ReactToCollision: bool = true,
        ReactToBullets: bool = true,
        ReactToExplosions: bool = true,
        RestoreIncomingVelocityWhenDestroyedRatio: f32 = 0.5,
        TemperatureLimit: f32 = f32::INFINITY,
        DestroyWhenBroken: bool = true,
        DeactivateWhenBroken: bool = false,
        TransferUndoRegistry: bool = true,
        IndestructibilityCanBeToggled: bool = true,
        Indestructible: bool = false,
        broken: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GetComponent<PhysicalBehaviour>()
        PhysicalBehaviour: usize = 0,
        DebrisPrefab: Option<ObjRef> = None,
        /// The physical whose Rigidbody2D VelocityAncestor is.
        VelocityAncestor: Option<usize> = None,
        /// OnBreak's persistent calls that are not audio or particles.
        OnBreak: Vec<super::link::EventCall> = Vec::new(),
        /// The component comes after the PhysicalBehaviour on its GameObject (SendMessage order).
        after_physical: bool = true,
        /// The component's index on its GameObject.
        index: usize = 0,
        /// A VaseBehaviour: Awake's spriteRenderer = GetComponent<SpriteRenderer>() (None: no SpriteRenderer, null).
        vase: Option<Option<super::visual::SpriteAt>> = None,
    }
}

/// Vector2.ClampMagnitude: sqrMagnitude is stored as a float, the comparison and the division are Mono doubles.
pub(super) fn clamp_magnitude(v: [f32; 2], max: f32) -> [f32; 2] {
    let sqr = (d(v[0]) * v[0] + d(v[1]) * v[1]).f();
    if (sqr as f64) > d(max).0 * max as f64 {
        let n = (sqr as f64).sqrt() as f32;
        let (x, y) = (v[0] / n, v[1] / n);
        return [x * max, y * max];
    }
    v
}

impl Game {
    /// DestroyableBehaviour.Start: a broken one deactivates itself; its context menu button.
    pub fn destroyable_start(&mut self, x: usize) {
        if self.destroyables[x].broken {
            let id = self.destroyables[x].id;
            self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.destroyables[x].PhysicalBehaviour].menu;
        menu.push(B { label: Label::DestroyableIndestructible(x), condition: Cond::DestroyableToggle(x), ..B::new("machineryIndestructible", "", crate::il!(str "DestroyableBehaviour::Start" 1), A::DestroyableToggle(x)) });
    }

    /// The ExplosiveBehaviour on the destroyable's object (TryGetComponent<ExplosiveBehaviour>).
    pub(super) fn destroyable_explosive(&mut self, x: usize) -> Option<usize> {
        let p = self.destroyables[x].PhysicalBehaviour;
        self.explosives_on(p).first().copied()
    }

    /// DestroyableBehaviour.Update
    pub fn destroyable_update(&mut self, x: usize) {
        let explosive = self.destroyable_explosive(x);
        let s = &self.destroyables[x];
        let p = &self.physicals[s.PhysicalBehaviour];
        if p.SimulateTemperature && p.Temperature >= s.TemperatureLimit && !explosive.is_some_and(|e| self.explosives[e].TemperatureLimit >= p.Temperature) {
            self.destroyable_break(x, [0.0, 0.0]);
        }
    }

    /// DestroyableBehaviour.OnFragmentHit
    pub fn destroyable_on_fragment_hit(&mut self, x: usize, force: f32) {
        let s = &self.destroyables[x];
        if s.ReactToExplosions && !(force < s.MinimumExplosionFragmentDamage) {
            match self.destroyable_explosive(x) {
                Some(e) if self.explosives[e].ExplodesOnFragmentHit => self.explosive_force_immediate(e),
                _ => self.destroyable_break(x, [0.0, 0.0]),
            }
        }
    }

    /// DestroyableBehaviour.OnCollisionEnter2D, DestroyableBehaviour.OnCollisionStay2D, DestroyableBehaviour.EvaluateCollision.
    pub fn destroyable_on_collision(&mut self, x: usize, c: &ScriptCollision) {
        let s = &self.destroyables[x];
        if !s.ReactToCollision {
            return;
        }
        // collision.GetContacts(buffer[8]), Utils.GetMinImpulse (NaN skipped, float.MaxValue without contacts).
        let min = c.contacts.iter().take(DESTROYABLE_BUFFER).map(|p| p.normal_impulse).filter(|i| !i.is_nan()).fold(f32::MAX, mathf::min);
        if min < s.MinimumImpactForce {
            return;
        }
        if let Some(e) = self.destroyable_explosive(x).filter(|&e| self.explosives[e].ImpactForceThreshold > 0.0) {
            self.explosive_force_immediate(e);
            return;
        }
        let ratio = self.destroyables[x].RestoreIncomingVelocityWhenDestroyedRatio;
        if let Some(o) = self.physical_of_collider(c.collider).filter(|&o| self.physicals[o].rigidbody.bodyType == 0) {
            let rb = self.physicals[o].rigidbody.clone();
            let (v, prev) = (rb.velocity(&self.world), self.physical_previous_vel(o));
            rb.set_velocity(&mut self.world, [mathf::lerp(v[0], prev[0], ratio), mathf::lerp(v[1], prev[1], ratio)]);
        }
        let own = c.other_rigidbody.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        // collision.otherRigidbody.velocity: a physical's body or a bare Rigidbody2D.
        let rb = self.physicals.iter().find(|p| !p.destroyed && p.rigidbody.component == own).map(|p| p.rigidbody.clone()).or_else(|| self.node_rigidbodies.iter().find(|n| n.rigidbody.component == own).map(|n| n.rigidbody.clone())).unwrap_or_else(|| crate::game::menu::throw("MissingReferenceException: The object of type 'Rigidbody2D' has been destroyed but you are still trying to access it."));
        let v = rb.velocity(&self.world);
        self.destroyable_break(x, [v[0] - c.relative_velocity.x, v[1] - c.relative_velocity.y]);
    }

    /// DestroyableBehaviour.Break(velocity)
    pub fn destroyable_break(&mut self, x: usize, velocity: [f32; 2]) {
        let s = &self.destroyables[x];
        if s.Indestructible || s.broken || s.OverallChance < self.random.value() {
            return;
        }
        self.destroyables[x].broken = true;
        for call in self.destroyables[x].OnBreak.clone() {
            self.event_call(call);
        }
        let s = &self.destroyables[x];
        let source = s.DebrisPrefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let pose = self.physicals[s.PhysicalBehaviour].transform.clone();
        let mut db = self.assets.take().expect("DestroyableBehaviour.Break without the game's data");
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(&mut db, &prefab, pose.position, pose.rotation, None));
        self.assets = Some(db);
        let (inst, linked) = spawned.unwrap_or_else(|e| panic!("DestroyableBehaviour: Instantiate: {e}"));
        // OnDebrisCreated is virtual: VaseBehaviour overrides it.
        match self.destroyables[x].vase {
            Some(sprite) => self.vase_on_debris_created(x, sprite, source.file, inst, linked, velocity),
            None => self.destroyable_on_debris_created(x, inst, linked, velocity),
        }
    }

    // VaseBehaviour.Awake: base.Awake (the link's Destroyable), then spriteRenderer = GetComponent<SpriteRenderer>() (`Destroyable::vase`)

    /// fn-check: VaseBehaviour.OnDebrisCreated on a recorded debris root (its GameObject id): the Instance and Linked the
    /// snapshot loader made of it, as Break hands over a fresh spawn's.
    pub(crate) fn vase_on_debris_created_loaded(&mut self, x: usize, debris: i32, velocity: [f32; 2]) -> Result<(), String> {
        let linked = self.recorded_linked.get(&debris).cloned().ok_or(format!("debris #{debris} not linked"))?;
        let sprite = self.destroyables[x].vase.ok_or("not a VaseBehaviour")?;
        let file = self.destroyables[x].DebrisPrefab.ok_or("no DebrisPrefab")?.file;
        let root = |i: &super::spawn::Instance| i.objects[0].game_object == debris;
        let inst = match self.retained.iter().position(root) {
            Some(at) => self.retained.remove(at),
            None => self.unsynced.remove(self.unsynced.iter().position(root).ok_or(format!("no debris clone #{debris}"))?),
        };
        self.vase_on_debris_created(x, sprite, file, inst, linked, velocity);
        Ok(())
    }

    /// VaseBehaviour.OnDebrisCreated: createdDebris.GetComponent<VaseDebrisBehaviour>().SetSprite(spriteRenderer.sprite), then base.OnDebrisCreated.
    fn vase_on_debris_created(&mut self, x: usize, sprite: Option<super::visual::SpriteAt>, file: usize, inst: super::spawn::Instance, linked: super::link::Linked, velocity: [f32; 2]) {
        let fields = inst.prefab.objects[0].components.iter().find_map(|(_, c)| match c {
            Component::Script { class, fields, .. } if class == "VaseDebrisBehaviour" => Some(fields.clone()),
            _ => None,
        });
        let Some(fields) = fields else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let Some(sprite) = sprite else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let sprite = self.sprite_ref(sprite);
        self.vase_debris_set_sprite(&fields, file, &inst, &linked, sprite);
        self.destroyable_on_debris_created(x, inst, linked, velocity);
    }

    /// VaseDebrisBehaviour.SetSprite(vaseSprite): SmallDebris' shape.texture = vaseSprite.texture, then Shards[i].sprite = vaseSprite.
    fn vase_debris_set_sprite(&mut self, v: &crate::assets::mono::Value, file: usize, inst: &super::spawn::Instance, linked: &super::link::Linked, sprite: ObjRef) {
        // shape.texture = vaseSprite.texture: the clone's SmallDebris samples it (its first update comes later).
        let p = v.get("SmallDebris").and_then(|p| p.pptr()).expect("VaseDebrisBehaviour.SmallDebris");
        if p.is_null() || p.file != 0 {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        let &(t, k) = inst.prefab.index.get(&ObjRef { file, path_id: p.path_id }).expect("VaseDebrisBehaviour.SmallDebris outside the debris prefab");
        let db = self.assets.as_ref().expect("VaseDebrisBehaviour.SetSprite without the game's data");
        let sp = db.sprite(sprite).expect("the vase's sprite");
        let texture = db.resolve(sprite.file, sp.texture);
        let owner = inst.objects[t].components[k];
        let e = self.embedded_ps.iter_mut().find(|e| e.owner == owner).expect("VaseDebrisBehaviour.SmallDebris: the clone's system");
        e.shape_texture = Some(texture.unwrap_or_else(|| unimplemented!("mod-only: VaseDebrisBehaviour.SetSprite: a sprite without a texture (shape.texture = null)")));
        let shards = v.get("Shards").and_then(|a| a.array()).expect("VaseDebrisBehaviour.Shards");
        for r in shards {
            let r = r.pptr().expect("VaseDebrisBehaviour.Shards entry");
            if r.is_null() || r.file != 0 {
                crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
            }
            let &(t, k) = inst.prefab.index.get(&ObjRef { file, path_id: r.path_id }).expect("VaseDebrisBehaviour.Shards outside the debris prefab");
            let at = match linked.physicals.get(&t) {
                Some(&p) => super::visual::SpriteAt::Physical(p),
                None => {
                    let id = inst.objects[t].components[k];
                    super::visual::SpriteAt::Renderer(self.renderers.iter().position(|x| x.id == id).expect("VaseDebrisBehaviour.Shards: a SpriteRenderer without a sprite"))
                }
            };
            match at {
                super::visual::SpriteAt::Physical(p) => self.physicals[p].sprite.as_mut().expect("a physical sprite").0 = sprite,
                super::visual::SpriteAt::Renderer(r) => self.renderers[r].sprite.0 = sprite,
            }
        }
    }

    /// DestroyableBehaviour.OnDebrisCreated
    fn destroyable_on_debris_created(&mut self, x: usize, mut inst: super::spawn::Instance, linked: super::link::Linked, velocity: [f32; 2]) {
        // GetOrAddComponent<DebrisComponent>: a new one takes an instance id (it has no messages).
        let has_debris = inst.prefab.objects[0].components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class == "DebrisComponent"));
        if !has_debris {
            self.allocate_instance_id();
        }
        let debris = inst.objects[0].components[0];
        let s = &self.destroyables[x];
        let p = s.PhysicalBehaviour;
        // createdDebris.transform.localScale = base.transform.lossyScale
        let scale = if self.physicals[p].parent.is_some() { self.lossy_scale(p) } else { self.physicals[p].transform.localScale };
        inst.set_local_scale(0, scale);
        self.refresh_transforms(&inst, &linked);
        self.unsynced.push(inst);
        if let Some(a) = self.destroyables[x].VelocityAncestor {
            self.destroyable_apply_ancestor_properties(a, p, velocity, &linked);
        }
        let s = &self.destroyables[x];
        let id = s.id;
        if s.DeactivateWhenBroken {
            self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        } else if s.DestroyWhenBroken {
            self.destroy_object(id);
        } else {
            // AddComponent<ActOnDestroy>().DestroyWith = { createdDebris }: a new component replaces the array.
            let key = self.with_component_instance(id, |g, inst, o| {
                g.act_on_destroy_add(inst, o);
                inst.objects[o].components[0]
            });
            self.act_on_destroy.entry(key).or_default().destroy_with.push(debris);
        }
        // TransferUndoRegistry: the undo history is UI.
    }

    /// DestroyableBehaviour.ApplyAncestorProperties over the debris' active Rigidbody2Ds and PhysicalBehaviours
    /// (GetComponentsInChildren: hierarchy order).
    pub(super) fn destroyable_apply_ancestor_properties(&mut self, ancestor: usize, original: usize, velocity: [f32; 2], debris: &super::link::Linked) {
        let mut parts: Vec<(usize, usize)> = debris.physicals.iter().map(|(&o, &p)| (o, p)).filter(|&(_, p)| self.physicals[p].active).collect();
        parts.sort();
        // The active Rigidbody2Ds (an inactive object's has no body), physical or not.
        let mut bodies: Vec<(usize, BodyOwner)> = parts.iter().map(|&(o, p)| (o, BodyOwner::Physical(p))).collect();
        bodies.extend(debris.node_rigidbodies.iter().filter(|&(_, &k)| self.node_rigidbodies[k].rigidbody.detached.is_none()).map(|(&o, &k)| (o, BodyOwner::Node(k))));
        bodies.sort_by_key(|&(o, _)| o);
        let arb = self.physicals[ancestor].rigidbody.clone();
        for &(_, owner) in &bodies {
            // GetPointVelocity starts with the AutoSyncTransforms.
            self.sync_pending();
            let pos = match owner {
                BodyOwner::Physical(p) => self.physicals[p].transform.position,
                BodyOwner::Node(k) => self.node_world(self.node_rigidbodies[k].node).0,
            };
            let pv = arb.point_velocity(&self.world, [pos[0], pos[1]]);
            let max = mathf::magnitude2(arb.velocity(&self.world));
            let v = clamp_magnitude([pv[0] + velocity[0], pv[1] + velocity[1]], max);
            let rb = self.owner_rigidbody(owner).clone();
            rb.set_velocity(&mut self.world, v);
            let w = arb.angular_velocity(&self.world) * 0.2;
            rb.set_angular_velocity(&mut self.world, w);
            // AddComponent<AudioSourceTimeScaleBehaviour>: audio registration only, but it takes an instance id.
            self.allocate_instance_id();
        }
        let n = parts.len() as i32;
        let o = &self.physicals[original];
        let (burn, on_fire, charge, intensity, simulate, temperature) = (o.BurnProgress, o.OnFire, o.charge, o.burnIntensity, o.SimulateTemperature, o.Temperature);
        for &(_, p) in &parts {
            self.physicals[p].BurnProgress = burn;
            if on_fire {
                self.ignite(p, true);
            }
            let ph = &mut self.physicals[p];
            ph.charge = charge / 1.max(n / 4) as f32;
            ph.burnIntensity = intensity / n as f32;
            if ph.SimulateTemperature && simulate {
                ph.Temperature = temperature;
            }
        }
    }

    /// Object.Destroy(gameObject) of the object holding component `id`: DestroyObjectDelayed queues it at
    /// min(fixedTimeAsDouble, the frame's time); a second Destroy finds it queued.
    pub fn destroy_object(&mut self, id: i32) {
        self.destroy_object_after(id, 0.0);
    }

    /// Object.Destroy(gameObject, delay). A destroyed object reads null, and `(bool)gameObject` checks skip it.
    pub fn destroy_object_after(&mut self, id: i32, delay: f32) {
        if !self.destroyed_components.contains(&id) && !self.object_destroys.iter().any(|&(_, i)| i == id) {
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble) + delay as f64;
            self.object_destroys.push((at, id));
            if delay <= 0.0 {
                self.destroy_disable_behaviours(id);
            }
        }
    }

    /// DestroyObjectFromScripting (1808cd940) with no delay: every Behaviour of the GameObject and of its children,
    /// active or not, gets SetEnabled(false) (1808cdc70) on the spot. An active script's OnDisable runs here, an
    /// active Collider2D, Joint2D or Effector2D loses its Physics2D state; the deactivation in the delayed destroy
    /// finds them done. The coroutines run on until then.
    pub(super) fn destroy_disable_behaviours(&mut self, id: i32) {
        let owns = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
        if self.destroyed_components.contains(&id) {
            return;
        }
        if !self.unsynced.iter().chain(&self.retained).any(owns) {
            // On a checked-out clone: run when it is back. A recording's item without a clone has nothing to disable.
            if self.clones_checked_out > 0 || !self.is_recording() {
                self.destroy_disable_pending.push((id, false));
            }
            return;
        }
        // The GameObject and its direct children only: a grandchild's scripts stay enabled (a destroyed Sentry_Turret's
        // TurretFrontLeg NodeBehaviour reads enabled=0, the leg's own Metal_scrape(Clone) Flash still 1).
        let own = self.with_component_instance(id, |g, inst, o| {
            let prefab = inst.prefab.clone();
            let children: Vec<usize> = (0..inst.objects.len()).filter(|&k| inst.objects[k].parent == Some(o)).collect();
            for x in std::iter::once(o).chain(children) {
                // gapC r9: an inactive object's Behaviours get m_Enabled = false too (SetEnabled stores it; no OnDisable, no
                // Physics2D state to drop): a destroyed Firework's inactive Killzone scripts read enabled=0 (fng4_items1).
                if !inst.objects[x].active {
                    for k in 0..prefab.objects[x].components.len() {
                        match prefab.objects[x].components[k].1 {
                            Component::Script { .. } => {
                                g.destroy_disabled.insert(inst.objects[x].components[k]);
                            }
                            // gapC r22: an inactive Collider2D's m_Enabled goes false too (no fixture to drop): a destroyed
                            // unbroken Bulb's inactive OtherPart Bulb_top reads enabled=0 (fngc26_gen BulbBehaviour.OnDestroy).
                            Component::Collider2D(_) => g.collider_set_enabled(inst, (x, k), false),
                            _ => {}
                        }
                    }
                    continue;
                }
                for k in 0..prefab.objects[x].components.len() {
                    match &prefab.objects[x].components[k].1 {
                        Component::Collider2D(_) => g.collider_set_enabled(inst, (x, k), false),
                        Component::Joint2D(_) => g.joint2d_deactivate(inst, &prefab, x, k),
                        Component::PointEffector2D(_) => g.point_effector_deactivate(inst, x, k),
                        // AudioSource is a Behaviour too: SetEnabled(false) stops it (a destroyed Wh's PlayOneShot from
                        // OnLiftoff reads isPlaying 0 in the same frame).
                        Component::AudioSource { .. } => {
                            let id = inst.objects[x].components[k];
                            if g.audio.sources.contains_key(&id) {
                                g.audio.set_enabled(id, false);
                            }
                        }
                        Component::Script { class, .. } => {
                            let id = inst.objects[x].components[k];
                            if g.destroy_disabled.insert(id) {
                                g.script_on_disable(inst, x, k, true);
                                // gapC r21: a TextMeshPro is a Behaviour too (its enabled is world_texts' flag).
                                if class.starts_with("TextMeshPro") && let Some(t) = g.world_texts.iter().position(|t| t.id == id) {
                                    g.text_enabled_set(t, false);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            inst.objects[o].components[0]
        });
        // The runtime children's DeleteAfterTime.
        for i in 0..self.delete_afters.len() {
            if self.delete_afters[i].enabled && self.delete_afters[i].parent == Some(own) && self.destroy_disabled.insert(self.delete_afters[i].id) {
                self.delete_afters[i].enabled = false;
                self.behaviours.remove(crate::game::Behaviour::DeleteAfter(i));
            }
        }
    }

    /// The queued Object.Destroy calls due by now, as (time, component id).
    pub fn object_due_destroys(&self, now: f64) -> Vec<(f64, i32)> {
        self.object_destroys.iter().copied().filter(|&(at, _)| at <= now).collect()
    }

    /// LimbBehaviour.OnDestroy: LimbBehaviourManager.Limbs.Remove, Destroy(myStatus), then Destroy(item) for each DestroyWith.
    pub(crate) fn limb_on_destroy(&mut self, l: usize) {
        self.limbs[l].enabled = false;
        // Destroy(myStatus): a recorded limb's status clone (no instance in ours: only its pending destroy, under its GameObject id).
        if let Some(s) = self.limbs[l].my_status.take() {
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
            self.object_destroys.push((at, s));
        }
        for item in self.limbs[l].destroy_with.clone() {
            self.destroy_object(item);
        }
    }

    /// PersonBehaviour.OnDestroy: RemoveFromDictionary (AliveBehaviour.AliveByTransform.Remove), the person's `destroyed` flag.
    pub(crate) fn person_on_destroy(&mut self, i: usize) {
        self.people[i].destroy_at = None;
        self.people[i].destroyed = true;
    }

    /// ShatteredShardBehaviour.OnDestroy: empty.
    pub(crate) fn shattered_shard_on_destroy(&mut self, _i: usize) {}

    /// ConnectedNodeBehaviour.OnDestroy: DisconnectFromEverything, then NodeController.Nodes.Remove.
    pub(crate) fn connected_node_on_destroy(&mut self, l: usize) {
        self.node_disconnect_from_everything(l);
    }

    /// GoreStringBehaviour.OnDestroy: DestroyJoint.
    pub(crate) fn gore_string_on_destroy(&mut self, l: usize) {
        if self.limbs[l].goreString {
            self.gore_string_destroy_joint(l);
        }
    }

    /// FirearmBehaviour.OnDestroy: BallisticsEmitter.Dispose() (a NullReferenceException before Start made the emitter).
    pub(crate) fn firearm_on_destroy(&mut self, f: usize) {
        if self.firearms[f].emitter.is_none() {
            super::menu::npe();
        }
        self.emitter_dispose(super::ballistics::Shooter::Firearm(f));
    }

    /// MachineGunBehaviour.OnDestroy: its emitter's Dispose().
    pub(crate) fn machine_gun_on_destroy(&mut self, m: usize) {
        if self.machine_guns[m].emitter.is_none() {
            super::menu::npe();
        }
        self.emitter_dispose(super::ballistics::Shooter::MachineGun(m));
    }

    /// ImmobilityFieldBehaviour.OnDestroy: DestroyAllJoints.
    pub(crate) fn immobility_field_on_destroy(&mut self, i: usize) {
        self.immobility_field_destroy_all_joints(i);
    }

    /// WaterBehaviour.OnDestroy: waters.Remove(this).
    pub(crate) fn water_on_destroy(&mut self, w: usize) {
        self.water_list.retain(|&i| i != w);
    }

    /// The limb whose GameObject has the Transform `key`.
    fn limb_of_object(&self, key: i32) -> usize {
        self.limbs.iter().position(|l| self.physicals[l.PhysicalBehaviour].transform_id == key).expect("the object's LimbBehaviour")
    }

    /// DestroyObjectHighLevel of the object holding component `id`: its hierarchy deactivates (OnDisable, the
    /// Physics2D cleanup), then each script gets OnDestroy, the object's components first, then its descendants.
    // DestroyObjectHighLevel_Internal (180758b40): Deactivate, then the WillDestroy walk (18075a230) runs
    // GameObject::WillDestroyGameObject (18058a140, each component in order) on the object before recursing into each
    // child in order: parent-first pre-order, the prefab's depth-first order `descendants` keeps.
    pub fn run_object_destroy(&mut self, id: i32) {
        self.object_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.contains(&id) {
            return;
        }
        // A SetupSpawnPrefab SpawnParticles(Clone) (its Transform `id`, no clone object kept): its GameObject and
        // components go, and it leaves its parent's children; its DeleteAfterTime drops out at its next Update.
        if let Some(parent) = self.delete_afters.iter().find(|d| d.added_to == Some(id)).and_then(|d| d.parent) {
            self.destroyed_components.extend([id + 2, id, id - 2, id - 4, id - 6, id - 8]);
            if let Some(v) = self.runtime_clone_transforms.get_mut(&parent) {
                v.retain(|c| c[0].0 != id);
            }
            return;
        }
        // A recording's item has no clone: its body and stab joints go. (Clones the replay spawned, such as an
        // explosion's effect, take the usual path.)
        if self.is_recording() && let Some(p) = self.physicals.iter().position(|p| p.id == id) {
            self.destroyed_components.insert(id);
            self.stab_destroy(p);
            self.physicals[p].enabled = false;
            self.physicals[p].destroyed = true;
            self.rigidbody_cleanup(p);
            // The coroutines of its clone's components below it stop (an ExplosionSound's DelayCoroutine).
            let owns = |i: &&super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
            if let Some(inst) = self.unsynced.iter().chain(&self.retained).find(owns) {
                let o = inst.objects.iter().position(|x| x.components.contains(&id)).unwrap();
                let gone: Vec<i32> = std::iter::once(o).chain(inst.descendants(o)).flat_map(|x| inst.objects[x].components.iter().copied()).collect();
                self.coroutines.retain(|c| !gone.contains(&c.owner));
                self.end_of_frame.retain(|c| !gone.contains(&c.owner));
                self.fixed_update_waits.retain(|c| !gone.contains(&c.owner));
            }
            return;
        }
        let mut orphans = Vec::new();
        let root = self.with_component_instance(id, |g, inst, o| {
            let prefab = inst.prefab.clone();
            let objects: Vec<usize> = std::iter::once(o).chain(inst.descendants(o)).collect();
            // The tool components (wires, pins, phase links) already gone by their own Destroy.
            let gone_before: Vec<i32> = objects.iter().flat_map(|&x| inst.objects[x].components.iter().copied()).filter(|c| g.destroyed_components.contains(c)).collect();
            for &x in &objects {
                g.destroyed_components.extend(inst.objects[x].components.iter().copied());
            }
            // Clones instantiated under these Transforms go with them (taken first: SetActive(false) below would
            // otherwise find them).
            for &x in &objects {
                let key = inst.objects[x].components[0];
                orphans.extend(g.attached_clones.remove(&key).unwrap_or_default().into_iter().map(|c| c.id));
            }
            // Its coroutines stop.
            let gone = &g.destroyed_components;
            g.coroutines.retain(|c| !gone.contains(&c.owner));
            g.end_of_frame.retain(|c| !gone.contains(&c.owner));
            g.fixed_update_waits.retain(|c| !gone.contains(&c.owner));
            for &x in &objects {
                for &c in &inst.objects[x].components {
                    g.stop_all_coroutines(c);
                }
            }
            g.set_active_scripts(inst, o, false);
            for &x in &objects {
                let key = inst.objects[x].components[0];
                for k in 0..prefab.objects[x].components.len() {
                    let Component::Script { class, .. } = &prefab.objects[x].components[k].1 else { continue };
                    let cid = inst.objects[x].components[k];
                    match class.as_str() {
                        "PhysicalBehaviour" => {
                            let p = g.physicals.iter().position(|p| p.id == cid).expect("a PhysicalBehaviour in the arena");
                            // DestroyStabJoint for each penetration; PhysicalObjectsInWorld(ByTransform) and the camera
                            // list drop it, so it leaves every manager loop.
                            g.stab_destroy(p);
                            g.physicals[p].enabled = false;
                            g.physicals[p].destroyed = true;
                            // An empty Collision2D exit to each buffered contact's collider, InvokeOnItemRemoved; and
                            // CatalogBehaviour.SpawnedGameObjects.Remove is no simulation state.
                            g.physical_send_buffer_exits(p);
                            g.physical_invoke_on_item_removed(p);
                        }
                        // One added at run time runs below, after the object's other components.
                        "ActOnDestroy" if prefab.objects[x].components[k].0.file == usize::MAX => {}
                        "ActOnDestroy" => g.act_on_destroy_run(key),
                        // Its OnSingleUse listener goes with it: activate_direct skips a destroyed toggle.
                        "ActivationToggleBehaviour" => {}
                        // OnDestroy removes an empty ContinuousActivation listener.
                        "ActivatorElectrodeBehaviour" => {}
                        "NoCollide" => g.no_collide_destroy(cid),
                        "NoCollideGm" => g.no_collide_gm_destroy(cid, inst),
                        "TyreDeflationBehaviour" => g.tyre_destroy(cid),
                        "AdhesiveCouplerBehaviour" => {
                            let i = g.couplers.iter().position(|s| s.id == cid).expect("the AdhesiveCouplerBehaviour");
                            g.coupler_on_destroy(i);
                        }
                        "SharpOnAllSidesBehaviour" => {
                            let i = g.sharp_on_all_sides.iter().position(|s| s.id == cid).expect("the SharpOnAllSidesBehaviour");
                            g.sharp_on_all_sides_destroy(i);
                        }
                        // Destroys its trail mesh (rendering).
                        "EnergySwordMotionBlur" => {}
                        "ImmobilityFieldBehaviour" => {
                            let i = g.immobility_fields.iter().position(|s| s.id == cid).expect("the ImmobilityFieldBehaviour");
                            g.immobility_field_on_destroy(i);
                        }
                        // OnDestroy destroys a live instantiatedWhO (a WhOpening root); the bolt children go with the GameObject.
                        "TeslaCoilBehaviour" => {
                            let i = g.tesla_coils.iter().position(|s| s.id == cid).expect("the TeslaCoilBehaviour");
                            g.tesla_coil_on_destroy(i);
                            g.tesla_coil_destroy_bolts(i);
                        }
                        "EnergySwordBehaviour" => {
                            let i = g.energy_swords.iter().position(|s| s.id == cid).expect("the EnergySwordBehaviour");
                            let blur = inst.objects[g.energy_swords[i].MotionBlur].components[0];
                            g.energy_sword_destroy(i, blur);
                        }
                        "FreezeBehaviour" => {
                            let i = g.freezes.iter().position(|s| s.id == cid).expect("the FreezeBehaviour");
                            g.freeze_destroy(i);
                        }
                        "ChainsawBehaviour" => {
                            let i = g.chainsaws.iter().position(|s| s.id == cid).expect("the ChainsawBehaviour");
                            g.chainsaw_destroy(i);
                        }
                        // Global.RemoveAudioSource
                        "ExplosionSoundBehviour" | "AudioSourceTimeScaleBehaviour" | "ExistInDetailView" => {}
                        // DeregisterBehaviour.OnDestroy: FindRelevantAction / DeregisterAction of this object.
                        "DeregisterBehaviour" => g.undo_deregister_related(key),
                        "GearBehaviour" => g.gear_destroy(cid),
                        // DetectorBehaviour.OnDestroy removes its ContinuousActivationBehaviour listener, which sends nothing received.
                        "DetectorBehaviour" => {}
                        // ButtonBehaviour.OnDestroy removes its ContinuousActivationBehaviour listener, which sends nothing received (button.rs).
                        "ButtonBehaviour" => {}
                        "LaserBehaviour" => {
                            let i = g.lasers.iter().position(|x| x.id == cid).expect("the LaserBehaviour");
                            g.laser_on_destroy(i);
                        }
                        // BulbBehaviour.OnDestroy: Object.Destroy(OtherPart), a child (destroyed with the hierarchy already) until Break detaches it.
                        "BulbBehaviour" => {}
                        // FirearmAttachmentPointBehaviour.OnDestroy: Object.Destroy(FirearmAttachment), the component, queued: its object
                        // is destroyed in this pass already (a firearm's child) or it was never connected (null).
                        "FirearmAttachmentPointBehaviour" => {}
                        // NoChildrenCollide.OnDestroy
                        // GetComponentsInChildren on the destroyed (inactive) GameObject finds no collider.
                        "NoChildrenCollide" => {}
                        "PersonBehaviour" => {
                            let i = g.people.iter().position(|p| p.id == cid).expect("the PersonBehaviour");
                            g.person_on_destroy(i);
                        }
                        "ShatteredShardBehaviour" => {}
                        // Its joints and bleeding particles go with the GameObject.
                        "LimbBehaviour" => {
                            let l = g.limb_of_object(key);
                            g.limb_on_destroy(l);
                            g.limb_destroy_joints(l);
                        }
                        "ConnectedNodeBehaviour" => {
                            let l = g.limb_of_object(key);
                            g.connected_node_on_destroy(l);
                        }
                        "GoreStringBehaviour" => {
                            let l = g.limb_of_object(key);
                            g.gore_string_on_destroy(l);
                        }
                        // SkinMaterialHandler.OnDestroy: Destroy(material), rendering. GripBehaviour.OnDestroy: modding listener.
                        "SkinMaterialHandler" | "GripBehaviour" => {}
                        "DisintegrationCounterBehaviour" => g.disintegration_counter_destroy(cid),
                        "FirearmBehaviour" => {
                            let f = g.firearms.iter().position(|x| x.id == cid).expect("the FirearmBehaviour");
                            // A thrown exception ends this OnDestroy only.
                            super::menu::catch_managed(|| g.firearm_on_destroy(f));
                        }
                        "ProjectileLauncherBehaviour" => g.launcher_destroy(cid),
                        "TransmitterBehaviour" => {
                            let i = g.transmitters.iter().position(|x| x.id == cid).expect("the TransmitterBehaviour");
                            g.transmitter_on_destroy(i);
                        }
                        // MachineGunBehaviour.BallisticsEmitter: the emitter its OnDestroy disposes.
                        "MachineGunBehaviour" => {
                            let m = g.machine_guns.iter().position(|x| x.id == cid).expect("the MachineGunBehaviour");
                            super::menu::catch_managed(|| g.machine_gun_on_destroy(m));
                        }
                        name if super::translated::CLASSES.contains(&name) => {
                            // Only an awakened component (in behaviour_of) gets OnDestroy.
                            if let Some(&(_, super::Behaviour::Translated(t))) = g.behaviour_of.get(&cid) {
                                g.tr_on_destroy(t);
                            }
                        }
                        // Each OnDestroy only unsubscribes, which the event's destroyed_components check stands for:
                        // LiquidOutletBehaviour / BloodTankBehaviour: PhysicalParticles.OnCollide -= OnParticleCollide.
                        // ActivationTransformerBehaviour / LaserReceiverBehaviour: OnContinuousUpdate.RemoveListener (continuous.rs).
                        // LiquidLevelGateBehaviour: OnContinuousUpdate.RemoveListener of a listener never kept (laser.rs).
                        // ActOnDisintegrate: PhysicalBehaviour.OnDisintegration -= OnDisintegration (acts.rs).
                        // JavelinBehaviour: ProjectileLauncher.OnLaunch -= TransferTarget (javelin.rs).
                        // InvisibleOptionInContextMenu: Global.main.LimbStatusToggled -= DetailViewToggled (limb.rs).
                        // LiquidPumpBehaviour: Object.Destroy(material), rendering.
                        "LiquidOutletBehaviour" | "BloodTankBehaviour" | "ActivationTransformerBehaviour" | "LaserReceiverBehaviour" | "LiquidLevelGateBehaviour" | "ActOnDisintegrate" | "JavelinBehaviour" | "InvisibleOptionInContextMenu" | "LiquidPumpBehaviour" => {}
                        "AntennaReceiverBehaviour" => {
                            let i = g.antenna_receivers.iter().position(|x| x.id == cid).expect("an AntennaReceiverBehaviour in the arena");
                            g.antenna_receiver_on_destroy(i);
                        }
                        // LavaBehaviour.OnDestroy: lavas.Remove(this) is the object's behaviours leaving the Fixed list (ballistics.rs lava_at_point).
                        // FuseBoxBehaviour.OnDestroy: Object.Destroy(audioProvider): the clip goes with its AudioSource (whose
                        // object is the one being destroyed).
                        // CopperWireBehaviour: EnergyWireBehaviour.OnDestroy runs through blood_wire_object_destroy below.
                        "LavaBehaviour" | "FuseBoxBehaviour" | "CopperWireBehaviour" => {}
                        "WaterBehaviour" => {
                            if let Some(w) = g.waters.iter().position(|x| x.id == cid) {
                                g.water_on_destroy(w);
                            }
                        }
                        // The tool components' OnDestroy runs in the *_object_destroy calls below (still live for them).
                        c if super::save::is_tool_component(c) => {
                            if !gone_before.contains(&cid) {
                                g.destroyed_components.remove(&cid);
                            }
                            continue;
                        }
                        c if !super::declares(c, "OnDestroy") => {}
                        c => unimplemented!("mod-only: {c}.OnDestroy"),
                    }
                    g.behaviour_of.remove(&cid);
                }
                // The BloodWireBehaviours the BloodWireTool added come after the prefab's components.
                g.blood_wire_object_destroy(&inst.objects[x].components);
                g.root_wire_object_destroy(&inst.objects[x].components);
                g.pin_object_destroy(&inst.objects[x].components);
                g.phase_link_object_destroy(&inst.objects[x].components);
                g.belt_object_destroy(&inst.objects[x].components);
                // WindZone and ParticleSystemForceField leave their managers with the object.
                g.forces_object_destroy(&inst.objects[x].components);
                // The CheckOnDestroyAttachment that FirearmAttachmentBehaviour.Awake added.
                g.check_on_destroy_attachment(key);
                // An ActOnDestroy added at run time is the object's last component.
                if g.act_on_destroy.contains_key(&key) {
                    g.act_on_destroy_run(key);
                }
                // The DeregisterBehaviour CatalogBehaviour.Spawn added to its root.
                if g.deregister_behaviours.remove(&key) {
                    g.undo_deregister_related(key);
                }
            }
            inst.objects.iter().all(|x| g.destroyed_components.contains(&x.components[0]))
        });
        for child in orphans {
            self.run_object_destroy(child);
        }
        // The whole clone went: nothing can reach its objects any more.
        if root {
            let owns = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
            self.unsynced.retain(|i| !owns(i));
            self.retained.retain(|i| !owns(i));
        }
    }
}

/// What a clone made by Instantiate(prefab, position, rotation, parent) hangs from.
#[derive(Clone, Copy, Debug)]
pub enum AttachParent {
    /// hit.transform of a PhysicalBehaviour's Rigidbody2D.
    Physical(usize),
    /// hit.transform of a Collider2D without a Rigidbody2D (by the collider's id).
    Bare(i32),
    /// transform of a clone's object with no body (by one of its components' id), e.g. the Killzone under a firework's TextBurst.
    Object(i32),
}

/// A clone parented under an existing GameObject: its root Transform's id and local TRS under the parent.
#[derive(Clone, Copy, Debug)]
pub struct AttachedClone {
    pub id: i32,
    pub parent: AttachParent,
    pub local: crate::unity::transform::Trs,
    /// A live Rigidbody2D object re-parented under it (`physical_set_parent`): its own body moves it, not the parent's
    /// Transform; only SetActive and Destroy go through the parent.
    pub driven: bool,
}

impl Game {
    /// The Transforms of an AttachParent's GameObject, the object first and then each ancestor up to the root.
    pub(super) fn attach_parent_nodes(&self, parent: AttachParent) -> Vec<crate::unity::transform::Trs> {
        match parent {
            AttachParent::Physical(p) => self.local_chain(p).into_iter().rev().collect(),
            AttachParent::Bare(c) => {
                let b = &self.bare_colliders[&c];
                self.anchored_nodes(b.anchor, &b.chain)
            }
            AttachParent::Object(c) => {
                let inst = self.unsynced.iter().chain(&self.retained).find(|i| i.objects.iter().any(|x| x.components.contains(&c))).expect("an Object parent's clone");
                self.transform_chain(inst, inst.objects.iter().position(|x| x.components.contains(&c)).unwrap())
            }
        }
    }

    /// Instantiate(prefab, position, rotation, parent) for the clone with root Transform `id`: position and rotation are
    /// world values, stored as the local TRS under the parent (the prefab root's own local scale stays).
    pub(super) fn attach_clone(&mut self, id: i32, parent: AttachParent, position: [f32; 3]) {
        let nodes = self.attach_parent_nodes(parent);
        let ((t, q), _) = crate::unity::transform::set_world(&nodes, position, [0.0, 0.0, 0.0, 1.0]);
        let s = self.with_component_instance(id, |_, inst, o| inst.objects[o].trs.s);
        let local = crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s };
        let key = match parent {
            AttachParent::Physical(p) => self.physicals[p].transform_id,
            AttachParent::Bare(c) | AttachParent::Object(c) => self.with_component_instance(c, |_, inst, o| inst.objects[o].components[0]),
        };
        self.attached_clones.entry(key).or_default().push(AttachedClone { id, parent, local, driven: false });
    }

    /// Transform.SetParent(parent, worldPositionStays: true) of the fresh clone with root Transform `id`, which was
    /// Instantiated at `position` with `rotation` and has no parent: the stored local TRS is the world pose under the
    /// parent chain. Under a parent whose lossy scale is not 1 the local scale would change (the clone keeps its lossy scale).
    pub(super) fn attach_clone_world(&mut self, id: i32, parent: AttachParent, position: [f32; 3], rotation: [f32; 4]) {
        use crate::unity::transform::{global_rotation, lossy_scale, set_world, Trs};
        let nodes = self.attach_parent_nodes(parent);
        let mut parent_rotation = [0.0, 0.0, 0.0, 1.0];
        for n in &nodes {
            parent_rotation = global_rotation(n, parent_rotation);
        }
        if lossy_scale(&nodes, parent_rotation) != [1.0, 1.0, 1.0] {
            unimplemented!("mod-only: Transform.SetParent (worldPositionStays) under a Transform whose lossy scale is not 1");
        }
        let ((t, q), _) = set_world(&nodes, position, rotation);
        let s = self.with_component_instance(id, |_, inst, o| inst.objects[o].trs.s);
        let key = match parent {
            AttachParent::Physical(p) => self.physicals[p].transform_id,
            AttachParent::Bare(c) | AttachParent::Object(c) => self.with_component_instance(c, |_, inst, o| inst.objects[o].components[0]),
        };
        let local = Trs { t: [t[0], t[1], t[2], 0.0], q, s };
        self.attached_clones.entry(key).or_default().push(AttachedClone { id, parent, local, driven: false });
    }

    /// The attached clones' world poses: the parent's Transform chain applied to the stored local TRS, after the
    /// physics write-back has moved the parents.
    pub(super) fn update_attached_clones(&mut self) {
        use crate::unity::transform::{global_position, global_rotation, lossy_scale};
        if self.attached_clones.is_empty() {
            return;
        }
        let gone = &self.destroyed_components;
        self.attached_clones.values_mut().for_each(|v| v.retain(|c| !gone.contains(&c.id)));
        self.attached_clones.retain(|_, v| !v.is_empty());
        let mut all: Vec<AttachedClone> = self.attached_clones.values().flatten().copied().collect();
        // The map's order is arbitrary: ids give the spawn order.
        all.sort_by_key(|c| std::cmp::Reverse(c.id));
        for c in all.into_iter().filter(|c| !c.driven) {
            let parents = self.attach_parent_nodes(c.parent);
            let (mut v, mut r) = (c.local.t, c.local.q);
            for n in &parents {
                (v, r) = (global_position(n, v), global_rotation(n, r));
            }
            let mut nodes = vec![c.local];
            nodes.extend(parents);
            let s = lossy_scale(&nodes, r);
            self.with_component_instance(c.id, |_, inst, o| {
                inst.set_world_rotation(o, r);
                inst.set_world_position(o, [v[0], v[1], v[2]]);
                inst.set_local_scale(o, s);
            });
        }
    }

    /// Transform.localScale = s on the clone root Transform `root`: an attached clone keeps it in its local TRS.
    #[allow(dead_code)]
    pub(super) fn clone_set_local_scale(&mut self, root: i32, s: [f32; 3]) {
        if let Some((key, _)) = self.attached_parent_of(root) {
            let c = self.attached_clones.get_mut(&key).unwrap().iter_mut().find(|c| c.id == root).unwrap();
            c.local.s = [s[0], s[1], s[2], 0.0];
        }
        self.with_component_instance(root, |_, inst, o| inst.set_local_scale(o, s));
    }

    /// The AttachParent and parent Transform id of the attached clone with root Transform `root`.
    pub(super) fn attached_parent_of(&self, root: i32) -> Option<(i32, AttachParent)> {
        self.attached_clones.iter().find_map(|(&k, v)| v.iter().find(|c| c.id == root).map(|c| (k, c.parent)))
    }

    /// Transform.SetParent(null) of the attached clone with root Transform `root`: it keeps its world position,
    /// rotation and lossy scale (the parent chain applied to its local TRS now) and leaves the parent.
    pub(super) fn attached_clone_set_parent_null(&mut self, root: i32) {
        use crate::unity::transform::{global_position, global_rotation, lossy_scale};
        let (key, parent) = self.attached_parent_of(root).expect("an attached clone");
        let c = self.attached_clones[&key].iter().find(|c| c.id == root).copied().unwrap();
        if c.driven {
            unimplemented!("mod-only: Transform.SetParent(null) of a clone whose own Rigidbody2D drives it");
        }
        let parents = self.attach_parent_nodes(parent);
        let (mut v, mut r) = (c.local.t, c.local.q);
        for n in &parents {
            (v, r) = (global_position(n, v), global_rotation(n, r));
        }
        let mut nodes = vec![c.local];
        nodes.extend(parents);
        let s = lossy_scale(&nodes, r);
        let list = self.attached_clones.get_mut(&key).unwrap();
        list.retain(|c| c.id != root);
        if list.is_empty() {
            self.attached_clones.remove(&key);
        }
        self.with_component_instance(root, |_, inst, o| {
            inst.set_world_rotation(o, r);
            inst.set_world_position(o, [v[0], v[1], v[2]]);
            inst.set_local_scale(o, s);
        });
    }

    /// GameObject.SetActive(`active`) on object `o` of `inst`: the clones parented under its subtree follow in
    /// activeInHierarchy (their own activeSelf stays). A deactivation reaches them all; an activation only those whose
    /// parent ended up active.
    pub(super) fn attached_set_active(&mut self, inst: &super::spawn::Instance, o: usize, active: bool) {
        if self.attached_clones.is_empty() {
            return;
        }
        let mut clones = Vec::new();
        for x in std::iter::once(o).chain(inst.descendants(o)) {
            if let Some(v) = self.attached_clones.get(&inst.objects[x].components[0]).filter(|_| !active || inst.objects[x].active) {
                clones.extend(v.iter().map(|c| (c.id, c.driven)));
            }
        }
        for (id, driven) in clones {
            if self.destroyed_components.contains(&id) {
                continue;
            }
            self.with_component_instance(id, |g, clone, o| {
                if driven {
                    // A live Rigidbody2D object under it: a full SetActive(false) of the clone with its own OnDisable and body
                    // cleanup, though only its activeInHierarchy changes (its activeSelf stays).
                    if !active {
                        g.set_active_scripts(clone, o, false);
                        clone.objects[o].self_active = true;
                    } else if clone.objects[o].self_active && !clone.objects[o].active {
                        unimplemented!("mod-only: GameObject.SetActive(true) of a parent whose child Rigidbody2D object was deactivated with it (Joint2D::Activate of the HingeJoint2D it holds)");
                    }
                    return;
                }
                let prefab = clone.prefab.clone();
                if !active {
                    let mut scripts = Vec::new();
                    g.deactivate_recursively(clone, &prefab, 0, &mut scripts);
                    for (d, c) in scripts {
                        let Component::Script { class, .. } = &prefab.objects[d].components[c].1 else { unreachable!() };
                        if super::declares(class, "OnDisable") {
                            unimplemented!("mod-only: {class}.OnDisable of a clone whose parent (Instantiate(.., parent)) was deactivated");
                        }
                    }
                } else if clone.objects[0].self_active && !clone.objects[0].active {
                    let mut queued = Vec::new();
                    g.activate_recursively(clone, &prefab, 0, &mut queued);
                    for (d, c) in queued {
                        if let Component::Script { class, .. } = &prefab.objects[d].components[c].1
                            && (!clone.objects[d].activated || super::declares(class, "OnEnable"))
                        {
                            unimplemented!("mod-only: {class}: Awake or OnEnable of a clone whose parent (Instantiate(.., parent)) was activated");
                        }
                    }
                }
            });
        }
    }
}

/// ActOnDestroy on a GameObject (by its Transform's instance id): from the prefab, or added by GetOrAddComponent.
#[derive(Clone, Debug, Default)]
pub struct ActOnDestroy {
    /// The prefab's ActOnDestroy component id (0 for one GetOrAddComponent added: its Event has no persistent calls).
    pub id: i32,
    /// Event's runtime listeners, in AddListener order.
    pub listeners: Vec<Listener>,
    /// DestroyWith: a component id on each object.
    pub destroy_with: Vec<i32>,
}

#[derive(Clone, Copy, Debug)]
pub enum Listener {
    /// DestroyWhenAllChildrenDestroyed.Check of this one.
    Check(usize),
    /// DetachFromParentOnStart.DestroyThis of the object holding this component.
    DestroyThis(i32),
}

/// DestroyWhenAllChildrenDestroyed: its own component id and the count Check counts down.
#[derive(Clone, Debug)]
pub struct DestroyWhenAllChildrenDestroyed {
    pub id: i32,
    pub l: i32,
}

/// A component AddComponent put on clone object `o` at run time, with instance id `id`: last among the object's
/// components (GetComponents order), so the pack writes it there.
pub(super) fn push_runtime_script(inst: &mut super::spawn::Instance, o: usize, class: &str, id: i32) {
    let mut prefab = (*inst.prefab).clone();
    let n = prefab.objects[o].components.len() as i64;
    prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, Component::Script { class: class.into(), execution_order: 0, enabled: true, fields: crate::assets::mono::Value::Struct(Vec::new()) }));
    inst.prefab = std::sync::Arc::new(prefab);
    inst.objects[o].components.push(id);
}

impl Game {
    /// AddComponent<ActOnDestroy>() on clone object `o`: an instance id and the component (its Event without persistent
    /// calls, its DestroyWith empty), whose ActOnDestroy state merges into the object's `act_on_destroy` entry.
    pub(super) fn act_on_destroy_add(&mut self, inst: &mut super::spawn::Instance, o: usize) {
        let id = self.allocate_instance_id();
        push_runtime_script(inst, o, "ActOnDestroy", id);
        self.act_on_destroy.entry(inst.objects[o].components[0]).or_default();
    }

    /// GetOrAddComponent<ActOnDestroy>() on the object with Transform `key`: a new one when it has none.
    pub(super) fn act_on_destroy_get_or_add(&mut self, key: i32) {
        if !self.act_on_destroy.contains_key(&key) {
            self.with_component_instance(key, |g, inst, o| g.act_on_destroy_add(inst, o));
        }
    }

    /// ActOnDestroy.OnDestroy: DestroyWith, then Event (its persistent calls were checked to reach only audio and
    /// particles; the runtime listeners follow them).
    pub(crate) fn act_on_destroy_run(&mut self, key: i32) {
        let Some(a) = self.act_on_destroy.remove(&key) else { return };
        for id in a.destroy_with {
            self.destroy_object(id);
        }
        // Event?.Invoke(): the persistent calls, then the runtime listeners.
        self.invoke_event(a.id, "Event");
        for l in a.listeners {
            match l {
                Listener::Check(d) => self.dwacd_check(d),
                // DetachFromParentOnStart.DestroyThis
                Listener::DestroyThis(id) => self.destroy_object(id),
            }
        }
    }

    /// DestroyWhenAllChildrenDestroyed.Awake over `children` (GetComponentsInChildren<Transform>: the active
    /// objects of its hierarchy, itself first), each object of the clone `inst` with whether it is in Exclude.
    pub fn dwacd_awake(&mut self, id: i32, inst: &mut super::spawn::Instance, children: &[(usize, bool)]) {
        let d = self.dwacds.len();
        let mut l = children.len() as i32;
        for &(o, excluded) in children {
            if excluded {
                l -= 1;
                continue;
            }
            // GetOrAddComponent<ActOnDestroy>
            let key = inst.objects[o].components[0];
            if !self.act_on_destroy.contains_key(&key) {
                self.act_on_destroy_add(inst, o);
            }
            self.act_on_destroy.entry(key).or_default().listeners.push(Listener::Check(d));
        }
        self.dwacds.push(DestroyWhenAllChildrenDestroyed { id, l });
    }

    /// DestroyWhenAllChildrenDestroyed.Check
    pub(crate) fn dwacd_check(&mut self, d: usize) {
        self.dwacds[d].l -= 1;
        if self.dwacds[d].l <= 1 {
            self.destroy_object(self.dwacds[d].id);
        }
    }

    /// The position, rotation and localScale of the Transform holding component `id`: a physical's or node's from
    /// the simulation; another's from its clone, which is current only before the bodies move (`fresh`).
    fn object_pose(&mut self, id: i32, fresh: bool) -> ([f32; 3], [f32; 4], [f32; 3]) {
        self.with_component_instance(id, |g, inst, o| {
            let components = &inst.objects[o].components;
            if let Some(p) = g.physicals.iter().position(|p| components.contains(&p.id)) {
                let t = &g.physicals[p].transform;
                return (t.position, t.rotation, t.localScale);
            }
            if let Some(n) = g.transform_nodes.iter().position(|n| n.id == components[0]) {
                let (position, rotation) = g.node_world(n);
                let s = g.transform_nodes[n].trs.s;
                return (position, rotation, [s[0], s[1], s[2]]);
            }
            if !fresh {
                unimplemented!("mod-only: the pose of a bare Transform after its clone was synced");
            }
            let s = inst.objects[o].trs.s;
            (inst.position(o), inst.rotation(o), [s[0], s[1], s[2]])
        })
    }

    /// InstantiateObjectBehaviour.Instantiate of component `id` (`fresh`: from its Start, before anything moved).
    pub fn instantiate_object(&mut self, id: i32, fresh: bool) {
        let (source, f) = self.instantiate_objects[&id].clone();
        let source = source.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let pose = self.object_pose(id, fresh);
        let position = if f.WorldSpace { f.Offset } else { [pose.0[0] + f.Offset[0], pose.0[1] + f.Offset[1], pose.0[2] + f.Offset[2]] };
        let rotation = if f.CopyRotation { pose.1 } else { [0.0, 0.0, 0.0, 1.0] };
        let mut db = self.assets.take().expect("InstantiateObjectBehaviour without the game's data");
        let spawned = db.prefab(source).and_then(|prefab| self.spawn(&mut db, &prefab, position, rotation, None));
        self.assets = Some(db);
        let (mut inst, linked) = spawned.unwrap_or_else(|e| panic!("InstantiateObjectBehaviour: Instantiate: {e}"));
        if f.CopyScale {
            inst.set_local_scale(0, pose.2);
            self.refresh_transforms(&inst, &linked);
        }
        self.unsynced.push(inst);
        if f.DestroySelf {
            self.destroy_object_after(id, f.DestroySelfDelay);
        }
    }

    /// Utils.DelayCoroutine and every StartCoroutine up to a `yield return new WaitForSeconds(seconds)`, by the MonoBehaviour `owner`.
    pub fn start_wait(&mut self, owner: i32, seconds: f32, step: Step) {
        self.coroutine_seq += 1;
        // CallDelayed(mode 0xa): the frame's time + seconds, from the next frame on.
        let (at, from) = (self.time.dynamicTimeAsDouble + seconds as f64, self.time.frameCount as i64 + 1);
        self.coroutines.push(Coroutine { seq: self.coroutine_seq, at, owner, from, step });
    }

    /// A coroutine past its WaitForSeconds.
    pub fn resume_coroutine(&mut self, c: Coroutine) {
        match c.step {
            // DistantSoundBehaviour.Awake: Instance = this, the Main scene's one that every Play below goes to.
            // DistantSoundBehaviour.Play(type): Play<Type>'s clips.PickRandom() and PlayOneShot (distant_sound_play).
            // DistantSoundBehaviour.PlaySmallExplosion: kind 1 (SmallDistantExplosions)
            // DistantSoundBehaviour.PlayLargeExplosion: kind 2
            // DistantSoundBehaviour.PlayLargerExplosion: kind 3
            // DistantSoundBehaviour.PlaySmallFirearm: kind 4
            // DistantSoundBehaviour.PlayMediumFirearm: kind 5
            // DistantSoundBehaviour.PlayLargeFirearm: kind 6
            Step::DistantSound(kind) => {
                self.distant_sound_play(kind, 0.5);
            }
            Step::ExplosiveEndOfFrame(e, seq) => self.explosive_end_of_frame(e, seq),
            Step::DestroyableShotBreak(x, normal) => self.destroyable_shot_break(x, normal),
            Step::SentryTurretExplode(i) => self.sentry_turret_explode(i),
            Step::RandomEventDelayed(i, k) => self.random_event_delayed(i, k),
            Step::EnergyBoltInit(b) => self.energy_bolt_init(b),
            Step::LightningFlash(e, j) => self.lightning_flash_end(e, j),
            Step::LightningGap(e, j) => self.lightning_gap_end(e, j),
            Step::Bullet(b) => self.bullet_resume(b),
            // FirearmAttachmentPointBehaviour.ConnectDelay
            Step::AttachmentConnectDelay(pt) => self.attachment_points[pt].ignoreConnect = false,
            Step::AttachmentReparent(pt) => self.attachment_point_reparent(pt),
            Step::LimbCrushEndOfFrame(l) => {
                self.coroutine_seq += 1;
                let owner = self.limbs[l].id;
                self.fixed_update_waits.push(Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner, from: i64::MIN, step: Step::LimbCrush(l) });
            }
            Step::LimbCrush(l) => self.limb_crush(l),
            Step::LimbStunNumb(l) => self.limb_stun_numb(l),
            Step::LimbDelayedCrush(l) => self.limb_crush(l),
            Step::ExoticFlash(p) => self.exotic_flash(p),
            Step::FirearmShoot(f) => self.firearm_shoot(f),
            Step::LaunchRoutine(l) => self.launcher_launch_routine(l),
            Step::WireFlipReconfigure(w) => self.blood_wire_flip_reconfigure(w),
            Step::SpikeIgnite(i) => self.spike_grenade_resume(i),
            Step::UnderwaterSplash(i) => self.underwater_splash(i),
            Step::GlobalUpdatePitch => self.audio.update_pitch(self.timeScaleTarget.0),
            Step::BulbBreak(i) => self.bulb_break(i),
            Step::SyringeBreak(s) => self.break_syringe(s),
            Step::BalloonPop(b) => self.balloon_wait_a_frame(b),
            Step::ShrapnelEndOfFrame(at, n) => {
                self.coroutine_seq += 1;
                self.fixed_update_waits.push(Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner: c.owner, from: i64::MIN, step: Step::ShrapnelEmit(at, n) });
            }
            Step::ShrapnelEmit(at, n) => {
                for _ in 0..n {
                    let direction = self.random.inside_unit_circle();
                    self.emitter_emit(super::ballistics::Shooter::Shrapnel, at, direction);
                }
            }
            Step::DoAtTimeOnceInvoke(d, k) => {
                // action.Event.Invoke(): a destroyed target's call is skipped.
                self.invoke_event(self.do_at_time_onces[d].id, &format!("Actions[{k}]"));
                let (id, duration) = (self.do_at_time_onces[d].id, self.do_at_time_onces[d].actions[k].Duration);
                self.start_wait(id, duration, Step::DoAtTimeOnceNext(d, k + 1));
            }
            Step::DoAtTimeOnceNext(d, i) => self.do_at_time_once_next(d, i),
            Step::Routine(id, _) => self.co_run(id),
            Step::CrossbowSetLayer(p) => self.crossbow_set_layer_resume(p),
            Step::CrossbowUnparent(c) => self.crossbow_unparent(c, true),
        }
    }

    /// DoAtTimeOnce.ExecuteSequence: StartCoroutine(StartSequence()).
    pub fn do_at_time_once_execute_sequence(&mut self, d: usize) {
        self.do_at_time_once_next(d, 0);
    }

    /// DoAtTimeOnce.StartSequence from action `i`: WaitForSeconds(Delay) of the next enabled one.
    pub fn do_at_time_once_next(&mut self, d: usize, i: usize) {
        let s = &self.do_at_time_onces[d];
        if let Some(k) = (i..s.actions.len()).find(|&k| s.actions[k].Enabled) {
            let (id, delay) = (s.id, s.actions[k].Delay);
            self.start_wait(id, delay, Step::DoAtTimeOnceInvoke(d, k));
        }
    }
}

/// A coroutine waiting in WaitForSeconds until `at`; it stops when its MonoBehaviour's GameObject is destroyed or
/// deactivated.
#[derive(Clone, Copy, Debug)]
pub struct Coroutine {
    pub seq: u64,
    pub at: f64,
    pub owner: i32,
    /// The first frame it may run in.
    pub from: i64,
    pub step: Step,
}

#[derive(Clone, Copy, Debug)]
pub enum Step {
    /// Global.Start's DelayCoroutine(0.05, UpdatePitch(timeScaleTarget)).
    GlobalUpdatePitch,
    /// ExplosionSoundBehviour.Play's DelayCoroutine(DistantSoundBehaviour.Play(SoundType)).
    DistantSound(i64),
    /// DoAtTimeOnce.StartSequence before action k's Event.
    DoAtTimeOnceInvoke(usize, usize),
    /// DoAtTimeOnce.StartSequence after an action's Duration, going on from action i.
    DoAtTimeOnceNext(usize, usize),
    /// ExplosiveBehaviour.Explode after WaitForEndOfFrame: (explosive, its coroutine sequence).
    ExplosiveEndOfFrame(usize, u64),
    /// DestroyableBehaviour.Shot's WaitAFrame: (destroyable, shot.normal).
    DestroyableShotBreak(usize, [f32; 2]),
    /// SentryTurretBehaviour.Shot's DelayCoroutine(Random.value, Explode): the `Game::sentry_turrets` index.
    SentryTurretExplode(usize),
    /// RandomEventBehaviour.Update's DelayCoroutine action: (the `Game::random_events` index, the event).
    RandomEventDelayed(usize, usize),
    /// EnergyVesselLightningBehaviour.Init after its WaitForSeconds: the `Game::energy_bolts` index.
    EnergyBoltInit(usize),
    /// LightningToolEntityBehaviour.FireBolt after flash j (the `Game::lightning_entities` index).
    LightningFlash(usize, i32),
    /// FireBolt after the gap that follows flash j.
    LightningGap(usize, i32),
    /// A BallisticIteration coroutine (`Game::bullets`).
    Bullet(usize),
    /// FirearmAttachmentPointBehaviour.ConnectDelay after its WaitForSeconds(0.75): `ignoreConnect = false` (attachment point).
    AttachmentConnectDelay(usize),
    /// FirearmAttachmentPointBehaviour.OnBeforeSerialise's NextFrameCoroutine after WaitForEndOfFrame (attachment point).
    AttachmentReparent(usize),
    /// LimbBehaviour.CrushNextFrame after WaitForEndOfFrame, then after WaitForFixedUpdate.
    LimbCrushEndOfFrame(usize),
    LimbCrush(usize),
    /// FirearmBehaviour.DelayedShot after its WaitForSeconds(FireDelay).
    FirearmShoot(usize),
    /// ProjectileLauncherBehaviour.LaunchRoutine after its WaitForFixedUpdate.
    LaunchRoutine(usize),
    /// GlobalShrapnelEmitter.EmitRoutine(position, amount) after its WaitForEndOfFrame, then after its
    /// WaitForFixedUpdate.
    ShrapnelEndOfFrame([f32; 2], i32),
    ShrapnelEmit([f32; 2], i32),
    /// SpikeGrenadeBehaviour.IgniteRoutine after its WaitForSeconds(Delay): the `Game::spike_grenades` index.
    SpikeIgnite(usize),
    /// UnderwaterExplosionBehaviour.DelayedSplash after its WaitForSeconds: the `Game::underwater_explosions` index.
    UnderwaterSplash(usize),
    /// BulbBehaviour.Shot's WaitAFrame after its WaitForSeconds(0.05): the `Game::bulbs` index.
    BulbBreak(usize),
    /// BalloonBehaviour.Shot's WaitAFrame after its WaitForSeconds(0.05): the `Game::balloons` index.
    BalloonPop(usize),
    /// SyringeBehaviour.Shot's WaitAFrame after its WaitForSeconds(0.05): the `Game::syringes` index.
    SyringeBreak(usize),
    /// LimbBehaviour.StunImpact's Utils.DelayCoroutine action after its WaitForSeconds: the `Game::limbs` index.
    LimbStunNumb(usize),
    /// ExplosionCreator.FragmentationRay's Utils.DelayCoroutine(0.01f, limb.Crush): the `Game::limbs` index.
    LimbDelayedCrush(usize),
    /// ExoticLiquid.OnUpdate's Utils.DelayCoroutine action: the container's physical.
    ExoticFlash(usize),
    /// DistanceJointWireBehaviour.OnFlip / FixedJointWireBehaviour.OnFlip after their WaitForFixedUpdate: the `Game::blood_wires` index.
    WireFlipReconfigure(usize),
    /// A translated routine (`coroutine::Routine`) by id, and for a yield null / WaitForSeconds the frame count from
    /// which DelayedCallManager::Update runs it (kWaitForNextFrame: the frame it was started in + 1; else i64::MIN).
    Routine(u64, i64),
    /// CrossbowBehaviour.DisconnectSyringe's Utils.DelayCoroutine(1f, SetLayer(9)): the syringe's physical (1.28.45).
    CrossbowSetLayer(usize),
    /// CrossbowBehaviour.OnPreFlip's Utils.NextFrameCoroutine after WaitForEndOfFrame: the `V45::crossbows` index.
    CrossbowUnparent(usize),
}

#[derive(Clone, Debug)]
pub struct DoAtTimeOnce {
    pub id: i32,
    pub actions: Vec<Action>,
}

/// DoAtTimeOnce.Action; Event's InstantiateObjectBehaviour.Instantiate targets by component id.
#[derive(Clone, Debug)]
pub struct Action {
    pub Enabled: bool,
    pub Delay: f32,
    pub Duration: f32,
}

impl Default for Action {
    fn default() -> Self {
        Self {
            Enabled: true,
            Delay: Default::default(),
            Duration: Default::default(),
        }
    }
}

impl Action {
    /// Action..ctor: the field initialisers, Enabled = true (Name and Event are serialised, and Event's calls are `unity_events`).
    pub fn new() -> Self {
        Action { Enabled: true, Delay: 0.0, Duration: 0.0 }
    }
}

script! {
    #[derive(PartialEq)]
    InstantiateObject {
        Offset: [f32; 3] = [0.0; 3],
        InstantiateOnStart: bool = false,
        WorldSpace: bool = false,
        DestroySelf: bool = false,
        CopyRotation: bool = false,
        CopyScale: bool = false,
        DestroySelfDelay: f32 = 0.0,
    }
    links {}
}

#[cfg(test)]
mod tests {
    use super::clamp_magnitude;

    #[test]
    fn action_starts_enabled() {
        let a = super::Action::new();
        assert!(a.Enabled && a.Delay == 0.0 && a.Duration == 0.0);
    }


    #[test]
    fn clamp_magnitude_scales_only_above_max() {
        assert_eq!(clamp_magnitude([3.0, 4.0], 10.0), [3.0, 4.0]);
        assert_eq!(clamp_magnitude([3.0, 4.0], 2.5), [1.5, 2.0]);
    }
}

/// DestroyableBehaviour.buffer: static ContactPoint2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const DESTROYABLE_BUFFER: usize = 8;
