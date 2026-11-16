//! AdhesiveCouplerBehaviour: Use sticks (a FixedJoint2D to each rigidbody of another hierarchy within StickDistance)
//! or unsticks (the joints destroyed). Update drops connections whose object or joint is gone and moves the fake
//! liquid's FillPercentage. The splats and goo lines are spawned as in the game; the lines' poses, widths and
//! colours follow each connection; the audio and the context menu are presentation and UI.
#![allow(non_snake_case)]

use super::destroyable::{AttachParent, AttachedClone};
use super::{d, mathf, script, Game, V};
use crate::assets::db::ObjRef;
use crate::game_assets::{Curve, CurveCache};
use super::line::LineVisual;

/// AdhesiveCouplerBehaviour.buffer: Collider2D[32].
pub(crate) const BUFFER: usize = 32;

/// AdhesiveCouplerBehaviour.Connection.
#[derive(Default, Clone, Debug)]
pub struct Connection {
    /// Line: the `Game::lines` entry of the goo line's LineRenderer.
    pub line: usize,
    /// OtherLocalPoint
    pub other_local_point: [f32; 2],
    pub time: f32,
    pub time_variation: f32,
    /// Other: the collider's GameObject, the object of this PhysicalBehaviour.
    pub other: usize,
    /// Joint: the `Game::fixed_joints` entry.
    pub joint: usize,
}

impl Connection {
    /// Connection..ctor: TimeVariation = Random.Range(0.95, 2) (the draw stays; the field only paces the goo line), then the
    /// object initialiser's Other and Joint.
    pub(crate) fn new(g: &mut Game, other: usize, joint: usize, line: usize, other_local_point: [f32; 2]) -> Self {
        let time_variation = g.random.range(0.95, 2.0);
        Connection { line, other_local_point, time: 0.0, time_variation, other, joint }
    }
}

script! {
    Coupler {
        StickDistance: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// phys: GetComponent<PhysicalBehaviour>().
        phys: usize = 0,
        SlimeSplatPrefab: ObjRef = ObjRef { file: 0, path_id: 0 },
        GooLineRendererPrefab: ObjRef = ObjRef { file: 0, path_id: 0 },
        /// LiquidContainer (`Game::liquid_containers`), or null.
        LiquidContainer: Option<usize> = None,
        FakeLiquidColor: [f32; 4] = [0.0; 4],
        /// KillZone: the Transform id of its object.
        KillZone: i32 = 0,
        /// LayerMask.m_Bits
        mask: u32 = 0,
        stuck: bool = false,
        connections: Vec<Connection> = Vec::new(),
        /// SerialisableState: the connections' identities between OnBeforeSerialise and OnAfterDeserialise (null otherwise).
        SerialisableState: Option<Vec<u128>> = None,
        fakeLiquidAmount: f32 = 1.0,
        /// WidthAnimationCurve and its evaluation cache; WidthAnimationDuration.
        WidthAnimationCurve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
        WidthAnimationDuration: f32 = 0.5,
        /// The goo line prefab's LineRenderer (GetComponent on its root) and its place among the root's components.
        goo_line: Option<LineVisual> = None,
        goo_line_at: usize = 0,
    }
}

impl Game {
    /// AdhesiveCouplerBehaviour.Start: LiquidContainer.Color = FakeLiquidColor, SetStickDistance(StickDistance) and the stick
    /// distance button.
    pub fn coupler_start(&mut self, c: usize) {
        if let Some(lc) = self.couplers[c].LiquidContainer {
            // LiquidContainer.Color = FakeLiquidColor
            let color = self.couplers[c].FakeLiquidColor;
            self.liquid_container_set_color(lc, color);
        }
        let v = self.couplers[c].StickDistance;
        self.coupler_set_stick_distance(c, v);
        let p = self.couplers[c].phys;
        self.physicals[p].menu.push(super::menu::MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "AdhesiveCouplerBehaviour::Start" 1),
            ..super::menu::MenuButton::new("couplerStickDistance", crate::il!(str "AdhesiveCouplerBehaviour::Start" 1), crate::il!(str "AdhesiveCouplerBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::CouplerStick(c))) });
    }

    /// AdhesiveCouplerBehaviour.SetStickDistance: KillZone.transform.localScale = 2f * v * Vector2.one (z = 0), then the
    /// KillZone's IgnoreParentSize.DesiredSize the same (a missing one throws).
    pub(crate) fn coupler_set_stick_distance(&mut self, c: usize, v: f32) {
        self.couplers[c].StickDistance = v;
        let k = (d(2.0f32) * v).f();
        let killzone = self.couplers[c].KillZone;
        self.with_component_instance(killzone, |_, inst, o| inst.set_local_scale(o, [k, k, 0.0]));
        let i = self.killzone_ignore_parent_size(killzone);
        self.ignore_parent_sizes[i].DesiredSize = [k, k];
    }

    /// AdhesiveCouplerBehaviour.Use
    pub fn coupler_use(&mut self, c: usize) {
        if self.couplers[c].stuck {
            self.coupler_unstick(c);
        } else {
            self.coupler_stick(c);
        }
    }

    /// AdhesiveCouplerBehaviour.Stick
    pub(crate) fn coupler_stick(&mut self, c: usize) {
        if self.couplers[c].stuck {
            return;
        }
        let me = self.couplers[c].phys;
        let at = self.physicals[me].transform.position;
        let mut hits = self.overlap_circle(V::new(at[0], at[1]), self.couplers[c].StickDistance, self.couplers[c].mask);
        hits.truncate(BUFFER);
        if hits.len() <= 1 {
            return;
        }
        let clip = self.audio.clip(self.couplers[c].id, "OnStick");
        self.play_clip_once(me, clip, 0.75);
        self.spawn_prefab(self.couplers[c].SlimeSplatPrefab, at);
        let root = self.root_id(me);
        for (p, collider) in hits {
            let Some(p) = p else {
                // StickTo returns at a null attachedRigidbody, whatever the root.
                let body = self.world.collider_body(collider).expect("an overlapped collider has fixtures");
                if self.rigidbody_of_body(body).is_some() {
                    unimplemented!("mod-only: AdhesiveCouplerBehaviour.Stick: a Rigidbody2D collider without a PhysicalBehaviour (its transform.root)");
                }
                continue;
            };
            if self.root_id(p) != root {
                self.coupler_stick_to(c, p, collider);
            }
        }
        self.couplers[c].stuck = true;
    }

    /// AdhesiveCouplerBehaviour.StickTo(collider) for a collider of physical `p`.
    pub(crate) fn coupler_stick_to(&mut self, c: usize, p: usize, collider: i32) {
        let me = self.couplers[c].phys;
        // collider.attachedRigidbody
        let body = self.world.collider_body(collider).expect("an overlapped collider has fixtures");
        let Some(rb) = self.rigidbody_of_body(body) else { return };
        let mine = self.physicals[me].colliders[0];
        if self.world.ignored.contains(&(collider.min(mine), collider.max(mine))) {
            return;
        }
        if !self.collider_on_physical_object(collider, p) || rb.body != self.physicals[p].rigidbody.body {
            unimplemented!("mod-only: AdhesiveCouplerBehaviour.StickTo: a collider off its PhysicalBehaviour's object or body (collider.transform, attachedRigidbody)");
        }
        let at = self.physicals[me].transform.position;
        // collider.ClosestPoint(transform.position), after AutoSyncTransforms.
        self.sync_pending();
        let point = self.world.collider_closest_point(collider, crate::b2::math::Vec2::new(at[0], at[1]));
        self.spawn_prefab(self.couplers[c].SlimeSplatPrefab, [point.x, point.y, 0.0]);
        let local = Game::inverse_transform_point(self, p, [point.x, point.y, 0.0]);
        // AddComponent<FixedJoint2D> takes the component's instance id.
        self.allocate_instance_id();
        let f = self.add_fixed_joint_component(me);
        self.fixed_joint_set_auto_configure(f, true);
        self.fixed_joint_set_connected_body(f, Some(super::BodyOwner::Physical(p)));
        self.fixed_joint_set_connected_anchor(f, [local[0], local[1]]);
        // Instantiate(GooLineRendererPrefab, collider.transform, worldPositionStays: false): the Goo root's local pose
        // is the prefab's (identity, checked at link), under the collider's Transform.
        let goo = self.spawn_prefab(self.couplers[c].GooLineRendererPrefab, at);
        let s = self.with_component_instance(goo, |_, inst, o| inst.objects[o].trs.s);
        let trs = crate::unity::transform::Trs { t: [0.0; 4], q: [0.0, 0.0, 0.0, 1.0], s };
        let key = self.physicals[p].transform_id;
        self.attached_clones.entry(key).or_default().push(AttachedClone { id: goo, parent: AttachParent::Physical(p), local: trs, driven: false });
        // Line = GetComponent<LineRenderer>(); positionCount = 2; useWorldSpace = true
        let (mut line, at) = (self.couplers[c].goo_line.clone().expect("AdhesiveCouplerBehaviour: a LineRenderer on the GooLineRendererPrefab's root"), self.couplers[c].goo_line_at);
        let (id, active) = self.with_component_instance(goo, |_, inst, o| (inst.objects[o].components[at], inst.objects[o].active));
        line.id = id;
        line.anchor = super::BareAnchor::Physical(p);
        line.visible = active;
        line.positions.resize(2, [0.0; 3]);
        line.use_world_space = true;
        self.lines.push(line);
        let connection = Connection::new(self, p, f, self.lines.len() - 1, [local[0], local[1]]);
        self.couplers[c].connections.push(connection);
    }

    /// AdhesiveCouplerBehaviour.Unstick: each Line and then each Joint destroyed.
    pub(crate) fn coupler_unstick(&mut self, c: usize) {
        if self.couplers[c].stuck {
            for k in std::mem::take(&mut self.couplers[c].connections) {
                self.line_queue_destroy(k.line);
                self.destroy_joint_delayed(k.joint);
            }
            let clip = self.audio.clip(self.couplers[c].id, "OnUnstick");
            self.play_clip_once(self.couplers[c].phys, clip, 0.75);
            self.couplers[c].stuck = false;
        }
    }

    /// AdhesiveCouplerBehaviour.Update. A connection goes when its Other, Line or Joint is gone (each dropped one plays
    /// OnUnstick); the others' line follows from the coupler to the Other's point.
    pub fn coupler_update(&mut self, c: usize) {
        let position = self.physicals[self.couplers[c].phys].transform.position;
        let mut i = 0;
        while i < self.couplers[c].connections.len() {
            let k = self.couplers[c].connections[i].clone();
            if self.physicals[k.other].destroyed || self.line_destroyed(k.line) || self.fixed_joints[k.joint].destroyed {
                self.couplers[c].connections.remove(i);
                let clip = self.audio.clip(self.couplers[c].id, "OnUnstick");
                self.play_clip_once(self.couplers[c].phys, clip, 0.75);
                continue;
            }
            let vector = self.transform_point3(k.other, [k.other_local_point[0], k.other_local_point[1], 0.0]);
            let n = super::explosion::normalized3([vector[0] - position[0], vector[1] - position[1], vector[2] - position[2]]);
            let vector2 = [position[0] + n[0] * 0.2, position[1] + n[1] * 0.2, position[2] + n[2] * 0.2];
            self.line_set_position(k.line, 0, vector2);
            let t = mathf::clamp01(k.time / 0.1f32);
            self.line_set_position(k.line, 1, [mathf::lerp(vector2[0], vector[0], t), mathf::lerp(vector2[1], vector[1], t), mathf::lerp(vector2[2], vector[2], t)]);
            let s = &mut self.couplers[c];
            let w = s.WidthAnimationCurve.as_ref().expect("AdhesiveCouplerBehaviour.WidthAnimationCurve").evaluate(&mut s.curve_cache, mathf::clamp01(k.time / s.WidthAnimationDuration));
            self.lines[k.line].width = w;
            let dt = self.time.deltaTime;
            let x = &mut self.couplers[c].connections[i];
            x.time = (d(x.time) + d(dt) * x.time_variation).f();
            i += 1;
        }
        let dt = self.time.deltaTime;
        let s = &mut self.couplers[c];
        if s.connections.is_empty() {
            s.stuck = false;
        }
        if let Some(lc) = s.LiquidContainer {
            let sign: f32 = if s.stuck { -1.0 } else { 1.0 };
            s.fakeLiquidAmount = (d(s.fakeLiquidAmount) + d(dt) * sign * 4.0f32).f();
            s.fakeLiquidAmount = mathf::clamp01(s.fakeLiquidAmount);
            let fill = (d(0.5f32) * s.fakeLiquidAmount).f();
            self.liquid_containers[lc].FillPercentage = fill;
        }
    }

    // AdhesiveCouplerBehaviour.Awake: phys is the `phys` link (GetComponent<PhysicalBehaviour>).
    /// AdhesiveCouplerBehaviour.OnDestroy: Object.Destroy(Line) for each connection (the joints go with their objects).
    pub fn coupler_on_destroy(&mut self, c: usize) {
        for k in self.couplers[c].connections.clone() {
            self.line_queue_destroy(k.line);
        }
    }
}
