//! TeslaCoilBehaviour and its TeslaCoilLightningBoltBehaviour children. Awake instantiates BoltCount bolts under the
//! coil at local (0, 1, 0); Use switches them on and off. An active bolt picks the nearest conducting physical object
//! within Radius each time its life runs out, charges it and nudges it every frame, and now and then leaves a Spark.
//! Two coils facing each other, each held by two charged Immobility Fields, open a Wh. The bolts' LineRenderer,
//! the light is presentation. Each bolt's LineRenderer is a `Game::lines` entry of its own.
#![allow(non_snake_case)]

use super::{d, mathf, script, Behaviour, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::game_assets::{Curve, CurveCache};

script! {
    TeslaCoil {
        /// A uint.
        BoltCount: i32 = 2,
        Activated: bool = false,
        unfit: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// physicalBehaviour
        physical: usize = 0,
        /// The Collider2Ds on its GameObject: the OnImmobilityCapture/Release messages it gets.
        colliders: Vec<i32> = Vec::new(),
        /// sparkClips.Length
        sparkClips: usize = 0,
        /// The `Game::tesla_bolts` entries of `bolts`.
        bolts: Vec<usize> = Vec::new(),
        /// Awake threw on Instantiate(null bolt): the bolts array holds BoltCount nulls.
        bolts_null: bool = false,
        /// audioSource is null (the prefab holds none).
        audio_null: bool = false,
        /// whCapturers (a HashSet): `Game::immobility_fields` indices.
        whCapturers: Vec<usize> = Vec::new(),
        whReady: bool = false,
        /// Wh (null: Instantiate throws).
        Wh: Option<ObjRef> = None,
        /// instantiatedWhO: the WhOpening clone's root Transform id.
        instantiatedWhO: Option<i32> = None,
    }
}

/// A TeslaCoilLightningBoltBehaviour: a clone under a coil, or the bolt prefab spawned on its own (the TelsaCoilLightning
/// catalog item, parent null).
#[derive(Clone, Debug)]
pub struct TeslaBolt {
    /// The `Game::tesla_coils` entry (parent); None for a bolt spawned on its own.
    pub coil: Option<usize>,
    /// LightningMaterial (Awake's lineRenderer.sharedMaterial) of a bolt spawned on its own.
    pub material: Option<ObjRef>,
    /// The script's instance id and execution order.
    pub id: i32,
    pub order: i32,
    /// activeSelf of the clone.
    pub active: bool,
    pub seed: f32,
    pub life: f32,
    pub layer: u32,
    pub Radius: f32,
    pub Spark: Option<ObjRef>,
    pub target: Option<usize>,
    pub t: f32,
    pub randomOffset: [f32; 2],
    pub colliderArray: Vec<i32>,
    /// lineRenderer: its index in `Game::lines`.
    pub line: usize,
    /// curve and its evaluation cache.
    pub curve: Option<Curve>,
    pub curve_cache: CurveCache,
}

impl Default for TeslaBolt {
    fn default() -> Self {
        Self {
            coil: Default::default(),
            material: Default::default(),
            id: Default::default(),
            order: Default::default(),
            active: Default::default(),
            seed: Default::default(),
            life: Default::default(),
            layer: Default::default(),
            Radius: 6.0,
            Spark: Default::default(),
            target: Default::default(),
            t: Default::default(),
            randomOffset: Default::default(),
            colliderArray: Default::default(),
            line: Default::default(),
            curve: Default::default(),
            curve_cache: Default::default(),
        }
    }
}

impl TeslaBolt {
    /// The recorded value fields (ItemScript: seed, life, Radius, the private t and randomOffset) the per-function checker
    /// loads and compares; target and colliderArray are links (fn-check's load_item_scripts maps their ids).
    pub fn fields_and_links(&mut self) -> (Vec<(&'static str, &mut dyn crate::game::Field)>, Vec<(&'static str, &mut dyn crate::game::Field)>) {
        (vec![("seed", &mut self.seed), ("life", &mut self.life), ("Radius", &mut self.Radius), ("t", &mut self.t), ("randomOffset", &mut self.randomOffset)], Vec::new())
    }
}

/// What the link reads from the bolt prefab: its script's values and the ids its Instantiate takes.
#[derive(Clone, Debug, Default)]
pub struct BoltSource {
    pub bolt: TeslaBolt,
    /// Each GameObject and component of the clone, and the bolt script's place among them.
    pub ids: usize,
    pub script_at: usize,
    /// The bolt reference is null: Instantiate throws an ArgumentException.
    pub null: bool,
    /// The bolt's LineRenderer (with LightningMaterial as its material) and its place among the ids.
    pub line: Option<crate::game::line::LineVisual>,
    pub line_at: usize,
    /// The bolt prefab's hierarchy (each clone is a runtime child of the coil, save.rs RuntimeChild).
    pub prefab: Option<std::sync::Arc<crate::assets::db::Prefab>>,
}

impl Game {
    /// TeslaCoilBehaviour.PlaySparkSound: audioSource.PlayOneShot(sparkClips.PickRandom()).
    pub(crate) fn tesla_play_spark_sound(&mut self, coil: usize) {
        let (id, clips) = (self.tesla_coils[coil].id, self.tesla_coils[coil].sparkClips);
        let k = self.pick_random(clips).unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        let clip = self.audio.clips(id, "sparkClips").get(k).cloned().flatten();
        let src = self.tesla_source(coil);
        self.audio.play_one_shot(src, clip.as_ref(), 1.0);
    }

    /// audioSource (a null one throws).
    fn tesla_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.tesla_coils[i].id, "audioSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// TeslaCoilLightningBoltBehaviour.SmoothRandom: (PerlinNoise(frq * Time.time, seed), PerlinNoise(seed, frq * Time.time)),
    /// each * 2 - 1. The line's points use it too (rendering).
    pub(crate) fn tesla_bolt_smooth_random(&self, k: usize, frq: f32) -> [f32; 2] {
        let (seed, num) = (self.tesla_bolts[k].seed, frq * self.time.time);
        [crate::unity::perlin_noise(num, seed) * 2.0 - 1.0, crate::unity::perlin_noise(seed, num) * 2.0 - 1.0]
    }

    /// TeslaCoilBehaviour.Awake: Instantiate(bolt, position, identity, transform) BoltCount times; each clone's Awake
    /// only sets up its LineRenderer, and an active enabled bolt joins the Update list.
    pub fn tesla_coil_awake(&mut self, i: usize, source: &BoltSource) {
        if source.null && self.tesla_coils[i].BoltCount > 0 {
            self.tesla_coils[i].bolts_null = true;
            crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null.");
        }
        for bi in 0..self.tesla_coils[i].BoltCount {
            let ids: Vec<i32> = (0..source.ids).map(|_| self.allocate_instance_id()).collect();
            // Instantiate(bolt, transform.position, identity, transform): the clone's local pose under the coil.
            if let Some(prefab) = source.prefab.clone() {
                let p = self.tesla_coils[i].physical;
                let nodes = self.attach_parent_nodes(crate::game::destroyable::AttachParent::Physical(p));
                let mut at = [0.0f32; 4];
                for n in &nodes {
                    at = crate::unity::transform::global_position(n, at);
                }
                let (_, q) = crate::unity::transform::set_world(&nodes, [at[0], at[1], at[2]], [0.0, 0.0, 0.0, 1.0]).0;
                let mut c = Self::runtime_instance(prefab, &ids, None);
                // gameObject.name += i; transform.localPosition = (0, 1, 0).
                std::sync::Arc::make_mut(&mut c.prefab).objects[0].name += &bi.to_string();
                (c.objects[0].trs.t, c.objects[0].trs.q) = ([0.0, 1.0, 0.0, 0.0], q);
                let owner = self.physicals[p].transform_id;
                self.runtime_child_add(owner, crate::game::save::RuntimeKind::Plain, Some(c));
            }
            let mut b = source.bolt.clone();
            // Awake: lineRenderer = GetComponent<LineRenderer>(), useWorldSpace = true, sharedMaterial = LightningMaterial, positionCount = 64.
            let mut line = source.line.clone().expect("TeslaCoilLightningBoltBehaviour: a LineRenderer on its GameObject");
            line.id = ids[source.line_at];
            line.anchor = crate::game::BareAnchor::Physical(self.tesla_coils[i].physical);
            line.use_world_space = true;
            line.positions.resize(64, [0.0; 3]);
            line.visible = true;
            self.lines.push(line);
            b.line = self.lines.len() - 1;
            b.coil = Some(i);
            b.id = ids[source.script_at];
            b.active = true;
            self.tesla_bolts.push(b);
            let k = self.tesla_bolts.len() - 1;
            self.tesla_coils[i].bolts.push(k);
            self.behaviour_of.insert(self.tesla_bolts[k].id, (source.bolt.order, Behaviour::TeslaBolt(k)));
            self.behaviours.add(source.bolt.order, Behaviour::TeslaBolt(k));
        }
    }

    /// TeslaCoilBehaviour.Start
    pub fn tesla_coil_start(&mut self, i: usize) {
        self.tesla_coil_update_bolts(i);
    }

    /// TeslaCoilBehaviour.Use
    pub fn tesla_coil_use(&mut self, i: usize) {
        if self.tesla_coils[i].enabled {
            self.tesla_coils[i].Activated = !self.tesla_coils[i].Activated;
            self.tesla_coil_update_bolts(i);
        }
    }

    /// GameObject.SetActive on a bolt clone: an inactive one leaves the Update list, an active one rejoins it at the end.
    fn tesla_bolt_set_active(&mut self, k: usize, active: bool) {
        if self.tesla_bolts[k].active == active {
            return;
        }
        self.tesla_bolts[k].active = active;
        let line = self.tesla_bolts[k].line;
        self.lines[line].visible = active;
        let b = Behaviour::TeslaBolt(k);
        if active {
            self.behaviours.add(self.tesla_bolts[k].order, b);
        } else {
            self.behaviours.remove(b);
        }
    }

    /// TeslaCoilBehaviour.UpdateBolts (the audio plays or stops)
    pub(crate) fn tesla_coil_update_bolts(&mut self, i: usize) {
        let on = self.tesla_coils[i].Activated;
        let src = self.tesla_source(i);
        if on {
            self.audio.play(src);
        } else {
            self.audio.stop(src);
        }
        if self.tesla_coils[i].bolts_null {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        for k in self.tesla_coils[i].bolts.clone() {
            self.tesla_bolt_set_active(k, on);
        }
    }

    /// TeslaCoilBehaviour.OnDisable
    pub fn tesla_coil_on_disable(&mut self, i: usize) {
        self.tesla_coils[i].Activated = false;
        let src = self.tesla_source(i);
        self.audio.stop(src);
        if self.tesla_coils[i].bolts_null {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        for k in self.tesla_coils[i].bolts.clone() {
            self.tesla_bolt_set_active(k, false);
        }
        self.tesla_coils[i].whReady = false;
    }

    /// TeslaCoilBehaviour.OnDestroy: `if ((bool)instantiatedWhO) Object.Destroy(instantiatedWhO)`.
    pub fn tesla_coil_on_destroy(&mut self, i: usize) {
        if let Some(w) = self.tesla_wh_alive(i) {
            self.destroy_object(w);
        }
    }

    /// The coil's GameObject is destroyed: its bolt children go with it (the hierarchy's destroy, not OnDestroy's code).
    pub fn tesla_coil_destroy_bolts(&mut self, i: usize) {
        for k in self.tesla_coils[i].bolts.clone() {
            self.behaviours.remove(Behaviour::TeslaBolt(k));
            self.tesla_bolts[k].active = false;
            let line = self.tesla_bolts[k].line;
            self.lines[line].visible = false;
            self.destroyed_components.insert(self.tesla_bolts[k].id);
        }
    }

    /// TeslaCoilBehaviour.FixedUpdate
    pub fn tesla_coil_fixed_update(&mut self, i: usize) {
        self.tesla_coils[i].whReady = self.tesla_coil_calculate_wh_ready(i);
        if self.tesla_coils[i].whReady {
            self.tesla_coil_affect_opposing_node(i);
        }
    }

    /// TeslaCoilBehaviour.CalculateWhReady
    pub(crate) fn tesla_coil_calculate_wh_ready(&self, i: usize) -> bool {
        let s = &self.tesla_coils[i];
        if s.unfit || !s.Activated || !s.enabled || s.whCapturers.len() < 2 || self.physicals[s.physical].Charge() < 25.0 {
            return false;
        }
        let mut num = 0;
        for &f in &s.whCapturers {
            let field = &self.immobility_fields[f];
            if !self.destroyed_components.contains(&field.id) && field.Activated && self.physicals[field.physical].Charge() > 25.0 {
                num += 1;
            }
            if num >= 2 {
                return true;
            }
        }
        false
    }

    /// `(bool)instantiatedWhO`: the clone's root exists.
    fn tesla_wh_alive(&self, i: usize) -> Option<i32> {
        self.tesla_coils[i].instantiatedWhO.filter(|id| !self.destroyed_components.contains(id))
    }

    /// TeslaCoilBehaviour.AffectOpposingNode: Physics2D.OverlapPoint(transform.TransformPoint(0, 6)) on the default
    /// raycast layers; a ready coil there makes the first one Instantiate the Wh (a WhOpening) at the midpoint, then
    /// the forces and the Wh's position follow; anything else destroys the Wh.
    pub(crate) fn tesla_coil_affect_opposing_node(&mut self, i: usize) {
        if self.tesla_coils[i].unfit {
            return;
        }
        let p = self.tesla_coils[i].physical;
        let point = self.transform_point(p, [0.0, 6.0]);
        let hit = self.overlap_point_first(point, !(1 << 2));
        let other = hit.and_then(|(_, collider)| self.tesla_coil_of_collider(collider)).filter(|&o| self.tesla_coils[o].whReady);
        let Some(o) = other else {
            if let Some(w) = self.tesla_wh_alive(i) {
                self.destroy_object(w);
            }
            return;
        };
        let mine = self.transform_point3(p, [0.0, 0.0, 0.0]);
        let theirs = self.transform_point3(self.tesla_coils[o].physical, [0.0, 0.0, 0.0]);
        let mid = [(mine[0] + theirs[0]) / 2.0, (mine[1] + theirs[1]) / 2.0, (mine[2] + theirs[2]) / 2.0];
        if self.tesla_wh_alive(o).is_none() && self.tesla_wh_alive(i).is_none() {
            let wh = self.tesla_coils[i].Wh.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            let id = self.spawn_prefab(wh, mid);
            self.tesla_coils[i].instantiatedWhO = Some(id);
            return;
        }
        if let Some(w) = self.tesla_wh_alive(i) {
            self.with_component_instance(w, |_, inst, o| inst.set_world_position(o, mid));
        }
        let rb = self.physicals[p].rigidbody.clone();
        let torque = (d(self.random.range(-1.0, 1.0)) * 500.0f32).f();
        rb.add_torque(&mut self.world, torque);
        let dir = super::person::normalized([mine[0] - theirs[0], mine[1] - theirs[1], mine[2] - theirs[2]]);
        rb.add_force(&mut self.world, [dir[0] * 320.0, dir[1] * 320.0]);
    }

    /// The coil on the GameObject of `collider`, for SendMessage("OnImmobilityCapture"/"OnImmobilityRelease").
    fn tesla_coil_of_collider(&self, collider: i32) -> Option<usize> {
        self.tesla_coils.iter().position(|c| !self.destroyed_components.contains(&c.id) && c.colliders.contains(&collider))
    }

    /// TeslaCoilBehaviour.OnImmobilityCapture(sender)
    pub(crate) fn tesla_coil_on_immobility_capture(&mut self, collider: i32, field: usize) {
        if let Some(i) = self.tesla_coil_of_collider(collider) {
            if !self.tesla_coils[i].whCapturers.contains(&field) {
                self.tesla_coils[i].whCapturers.push(field);
            }
        }
    }

    /// TeslaCoilBehaviour.OnImmobilityRelease(sender)
    pub(crate) fn tesla_coil_on_immobility_release(&mut self, collider: i32, field: usize) {
        if let Some(i) = self.tesla_coil_of_collider(collider) {
            self.tesla_coils[i].whCapturers.retain(|&f| f != field);
        }
    }

    /// The bolt's transform.position: its local (0, 1, 0) under the coil, or its own Transform's.
    fn tesla_bolt_position(&mut self, k: usize) -> [f32; 3] {
        match self.tesla_bolts[k].coil {
            Some(i) => self.transform_point3(self.tesla_coils[i].physical, [0.0, 1.0, 0.0]),
            None => self.with_component_instance(self.tesla_bolts[k].id, |_, inst, o| inst.position(o)),
        }
    }

    /// TeslaCoilLightningBoltBehaviour.Awake of a bolt spawned on its own (a coil's clones: tesla_coil_awake):
    /// lineRenderer = GetComponent<LineRenderer>(), useWorldSpace = true, sharedMaterial = LightningMaterial,
    /// positionCount = positions.Length (64).
    pub fn tesla_bolt_awake(&mut self, k: usize) {
        let (line, material) = (self.tesla_bolts[k].line, self.tesla_bolts[k].material);
        let l = &mut self.lines[line];
        l.use_world_space = true;
        l.material = material;
        l.positions.resize(64, [0.0; 3]);
    }

    /// TeslaCoilLightningBoltBehaviour.Init
    pub(crate) fn tesla_bolt_init(&mut self, k: usize) {
        self.tesla_bolt_find_new_target(k);
        self.tesla_bolts[k].seed = self.random.range(-9000.0, 9000.0);
        let long = (self.random.value() as f64) > 0.95;
        self.tesla_bolts[k].life = if long { self.random.range(0.1, 0.3) } else { self.random.range(0.02, 0.08) };
        let width = self.random.range(1.0, 2.0);
        let line = self.tesla_bolts[k].line;
        self.lines[line].width = width;
        let c = self.random.inside_unit_circle();
        self.tesla_bolts[k].randomOffset = [c[0] * 4.0, c[1] * 4.0];
    }

    /// TeslaCoilLightningBoltBehaviour.FindNewTarget: OverlapCircleNonAlloc into a 32-collider buffer, then the
    /// conducting in-world physical whose collider is nearest a jittered point (one insideUnitSphere per candidate).
    pub(crate) fn tesla_bolt_find_new_target(&mut self, k: usize) {
        self.tesla_bolts[k].target = None;
        let position = self.tesla_bolt_position(k);
        let (radius, layer) = (self.tesla_bolts[k].Radius, self.tesla_bolts[k].layer);
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), radius, layer);
        hits.truncate(TESLA_OVERLAP_BUFFER);
        if hits.is_empty() {
            return;
        }
        // transform.root: the coil's root; a bolt on its own is its clone's root, which holds no collider.
        let root = self.tesla_bolts[k].coil.map(|i| self.root_id(self.tesla_coils[i].physical));
        let mut best = 10_000_000.0f32;
        for (q, collider) in hits {
            if !self.collider_exists(collider) {
                continue;
            }
            // collider.transform.root != transform.root, PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform)
            let Some(q) = q.filter(|&q| self.physicals[q].in_world && !self.physicals[q].destroyed) else { continue };
            if Some(self.root_id(q)) == root || !self.properties[self.physicals[q].Properties].Conducting {
                continue;
            }
            let at = self.physicals[q].transform.position;
            let s = self.random.inside_unit_sphere();
            let d = [at[0] - (position[0] + s[0]), at[1] - (position[1] + s[1]), at[2] - (position[2] + s[2])];
            let sqr = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if sqr < best {
                self.tesla_bolts[k].target = Some(q);
                best = sqr;
            }
        }
        if let Some(q) = self.tesla_bolts[k].target {
            // target.GetComponents<Collider2D>()
            self.tesla_bolts[k].colliderArray = self.physicals[q].colliders.clone();
        }
    }

    /// TeslaCoilLightningBoltBehaviour.Update
    pub fn tesla_bolt_update(&mut self, k: usize) {
        self.tesla_bolts[k].t += self.time.smoothDeltaTime;
        if self.tesla_bolts[k].t > self.tesla_bolts[k].life {
            self.tesla_bolts[k].t = 0.0;
            self.tesla_bolt_init(k);
        }
        let position = self.tesla_bolt_position(k);
        let vector: [f32; 3] = match self.tesla_bolts[k].target.filter(|&q| !self.physicals[q].destroyed) {
            Some(q) => {
                let mut vector = self.physicals[q].transform.position;
                let mut best = f32::MAX;
                let off = self.tesla_bolts[k].randomOffset;
                let jittered = [position[0] + off[0], position[1] + off[1], position[2]];
                for collider in self.tesla_bolts[k].colliderArray.clone() {
                    if !self.collider_exists(collider) {
                        continue;
                    }
                    // Collider2D.ClosestPoint (AutoSyncTransforms first)
                    self.sync_pending();
                    let c = self.world.collider_closest_point(collider, V::new(position[0], position[1]));
                    let d = [c.x - jittered[0], c.y - jittered[1], 0.0 - jittered[2]];
                    let sqr = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                    if sqr < best {
                        best = sqr;
                        vector = [c.x, c.y, 0.0];
                    }
                }
                self.physicals[q].charge = self.physicals[q].Charge() + self.time.deltaTime * 4.0;
                let c = self.random.inside_unit_circle();
                let rb = self.physicals[q].rigidbody.clone();
                rb.add_force_impulse(&mut self.world, [c[0] * 0.01, c[1] * 0.01]);
                if self.tesla_bolt_smooth_random(k, 10.0)[0] > 0.99 {
                    // if (parent) parent.PlaySparkSound()
                    if let Some(i) = self.tesla_bolts[k].coil {
                        self.tesla_play_spark_sound(i);
                    }
                    let spark = self.tesla_bolts[k].Spark.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
                    self.spawn_prefab(spark, vector);
                }
                vector
            }
            // vector = Random.insideUnitCircle * Radius + (Vector2)position
            None => {
                let c = self.random.inside_unit_circle();
                let r = self.tesla_bolts[k].Radius;
                [c[0] * r + position[0], c[1] * r + position[1], 0.0]
            }
        };
        let (line, seed, num) = (self.tesla_bolts[k].line, self.tesla_bolts[k].seed, self.tesla_bolts[k].t);
        let n = self.lines[line].positions.len();
        let gravity = self.world.gravity;
        let smooth = self.tesla_bolt_smooth_random(k, 5.0);
        let mut positions = Vec::with_capacity(n);
        for j in 0..n {
            let time = j as f32 / (n - 1) as f32;
            let v4 = [mathf::lerp(position[0], vector[0], time), mathf::lerp(position[1], vector[1], time), mathf::lerp(position[2], vector[2], time)];
            let p5 = mathf::perlin2_mapped((d(v4[0]) * 3.354f32 - seed).f(), (d(v4[1]) * 3.354f32 + seed).f());
            let v5 = [p5[0] * 0.5, p5[1] * 0.5];
            let b = &mut self.tesla_bolts[k];
            let w = b.curve.as_ref().expect("TeslaCoilLightningBoltBehaviour.curve").evaluate(&mut b.curve_cache, time);
            let p6 = mathf::perlin2_mapped((d(v4[0]) * 0.3f32 - seed).f(), (d(v4[1]) * 0.3f32 + seed).f());
            // curve.Evaluate(time) * noise * 1.4f + Physics2D.gravity * num * -0.1f: every Vector2 operator rounds.
            let v6 = [(w * p6[0]) * 1.4 + (gravity.x * num) * -0.1, (w * p6[1]) * 1.4 + (gravity.y * num) * -0.1];
            // 5.25f * num * (Vector3)vector5 + (Vector3)vector6 * 1.3f
            let s = 5.25f32 * num;
            let v7 = [s * v5[0] + v6[0] * 1.3, s * v5[1] + v6[1] * 1.3, s * 0.0 + 0.0 * 1.3];
            positions.push([(v4[0] + v7[0]) + smooth[0] * 0.02, (v4[1] + v7[1]) + smooth[1] * 0.02, (v4[2] + v7[2]) + 0.0 * 0.02]);
        }
        self.line_set_positions(line, &positions);
    }
}

/// TeslaCoilLightningBoltBehaviour.overlapBuffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const TESLA_OVERLAP_BUFFER: usize = 32;
