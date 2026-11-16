//! PinTool, WoodPinTool and PinBehaviour.

use super::explosion::DEFAULT_RAYCAST_LAYERS;
use super::constraint::{ConstrainedTarget, ConstraintSource, ParentConstraint};
use super::{d, mathf, BodyOwner, Game};

/// PinBehaviour
#[derive(Clone, Debug)]
pub struct Pin {
    pub id: i32,
    /// Hover.IsMouseOverlapping
    pub IsMouseOverlapping: bool,
    /// PinBehaviour.invisibilityTracker
    pub tracker: super::tool_invisibility::InvisibilityTracker,
    /// PhysicalBehaviour (usize::MAX when `root` holds the pin).
    pub physical: usize,
    /// A pin loaded onto an object without a PhysicalBehaviour: that object's Transform id. PinBehaviour checks
    /// PhysicalBehaviour only in FixedUpdate's burn, so the pin works: its HingeJoint2D's RequireComponent adds the object a
    /// Rigidbody2D.
    pub root: Option<i32>,
    pub Other: Option<usize>,
    pub LocalAnchor: [f32; 2],
    pub BreakingThreshold: f32,
    pub UsedToHaveConnectedBody: bool,
    pub Invisible: bool,
    /// PinSpritePath, set by WoodPinTool.OnPinCreation.
    pub wood: bool,
    pub Joint: Option<usize>,
    pub initialBreakForce: f32,
    pub started: bool,
    /// SpriteRenderer, in `Game::renderers`.
    pub renderer: Option<usize>,
    /// parentConstraint, in `Game::parent_constraints`.
    pub constraint: Option<usize>,
}

impl Default for Pin {
    fn default() -> Self {
        Self {
            id: Default::default(),
            IsMouseOverlapping: Default::default(),
            tracker: Default::default(),
            physical: Default::default(),
            root: Default::default(),
            Other: Default::default(),
            LocalAnchor: Default::default(),
            BreakingThreshold: f32::INFINITY,
            UsedToHaveConnectedBody: Default::default(),
            Invisible: Default::default(),
            wood: Default::default(),
            Joint: Default::default(),
            initialBreakForce: f32::INFINITY,
            started: Default::default(),
            renderer: Default::default(),
            constraint: Default::default(),
        }
    }
}

/// Vector2.SqrMagnitude(...) < 0.005102041f
const MOUSE_RADIUS_SQUARED: f32 = 0.005102041;

impl Game {
    /// PinTool.GetFoundBodies: the first two PhysicalBehaviours at the point, of the buffer's 8 colliders.
    pub(crate) fn pin_found_bodies(&mut self, point: [f32; 3]) -> (Option<usize>, Option<usize>) {
        let (mut first, mut second) = (None, None);
        let mut hits = self.overlap_point_colliders(point, DEFAULT_RAYCAST_LAYERS, f32::NEG_INFINITY, f32::INFINITY);
        hits.truncate(8);
        for (physical, _) in hits {
            // Utils.HasComponent<Optout>: TryGetComponent, the `has_optout` flag read when the object was linked.
            let Some(c) = physical.filter(|&c| !self.physicals[c].has_optout) else { continue };
            if first.is_none() || first == Some(c) {
                first = Some(c);
            } else if second.is_none() || second == Some(c) {
                second = Some(c);
            } else {
                break;
            }
        }
        (first, second)
    }

    /// PinTool.OnSelect
    pub(crate) fn pin_on_select(&mut self, wood: bool) {
        let mouse = self.MousePosition;
        let (first, second) = self.pin_found_bodies(mouse);
        let Some(a) = first.or(second) else { return };
        let [x, y] = self.pin_tool_get_pin_pos(first, second);
        let anchor = self.inverse_transform_point(a, [x, y, 0.0]);
        let b = if first.is_some() { second } else { None };
        self.create_pin([anchor[0], anchor[1]], a, b, Self::pin_tool_breaking_threshold(wood), wood);
    }

    /// PinTool.GetBreakingThreshold: PositiveInfinity. WoodPinTool.GetBreakingThreshold: 6000.
    pub(crate) fn pin_tool_breaking_threshold(wood: bool) -> f32 {
        if wood { 6000.0 } else { f32::INFINITY }
    }

    /// PinTool.GetPinPos: the mouse position, or with snapToCenter the first (else second) found body's world centre of
    /// mass (a Vector2).
    pub(crate) fn pin_tool_get_pin_pos(&self, first: Option<usize>, second: Option<usize>) -> [f32; 2] {
        let mut v = self.MousePosition;
        if self.input.held("snapToCenter") {
            if let Some(p) = first.or(second) {
                v = self.physical_world_center_of_mass(p);
            }
        }
        [v[0], v[1]]
    }

    /// PhysicalBehaviour.GetWorldCenterOfMass, at z 0 (a Vector2).
    pub(crate) fn physical_world_center_of_mass(&self, p: usize) -> [f32; 3] {
        let x = &self.physicals[p];
        if x.rigidbody.bodyType == 0 {
            let c = self.world.body(x.rigidbody.body).sweep.c;
            [c.x, c.y, 0.0]
        } else {
            let [px, py, _] = x.transform.position;
            [px, py, 0.0]
        }
    }

    // WoodPinTool.OnToolChosen: empty.
    // WoodPinTool.OnToolUnchosen: empty.
    // WoodPinTool.OnHold: empty.
    // WoodPinTool.OnFixedHold: empty.
    // WoodPinTool.OnDeselect: empty.
    /// PinTool.OnToolChosen: the first AddComponent of the tool, then its empty OnToolChosen.
    pub(crate) fn pin_tool_choose(&mut self, wood: bool) {
        if !self.pin_tool_created[wood as usize] {
            self.allocate_instance_id();
            self.pin_tool_created[wood as usize] = true;
        }
        self.pin_tool = Some(wood);
    }

    /// `if ((bool)SpriteChild) SpriteChild.gameObject.SetActive(active)` of pin component `id` (no SpriteChild before Start).
    pub(super) fn pin_sprite_child_set_active(&mut self, id: i32, active: bool) {
        if let Some(r) = self.pins.iter().find(|x| x.id == id).and_then(|x| x.renderer) {
            self.renderers[r].visible = active;
        }
    }

    /// PinTool.CreatePin: AddComponent<PinBehaviour>(), whose PinBehaviour.Awake finds the PhysicalBehaviour (the `physical` field).
    pub(crate) fn create_pin(&mut self, anchor: [f32; 2], a: usize, b: Option<usize>, threshold: f32, wood: bool) {
        let id = self.allocate_instance_id();
        self.add_script_to_clone(self.physicals[a].id, "PinBehaviour", id);
        self.undo_register_action(super::undo::ObjectCreationAction { displayName: crate::il!(str "PinTool::CreatePin" 0).into(), RelevantObjects: vec![id] });
        self.pins.push(Pin {
            id,
            physical: a,
            root: None,
            Other: b,
            LocalAnchor: anchor,
            BreakingThreshold: threshold,
            UsedToHaveConnectedBody: b.is_some(),
            Invisible: self.tool.as_ref().unwrap().invisible,
            wood,
            Joint: None,
            initialBreakForce: f32::INFINITY,
            started: false,
            renderer: None,
            constraint: None,
            IsMouseOverlapping: false,
            tracker: Default::default(),
        });
        // PinTool.OnPinCreation: empty.
        // WoodPinTool.OnPinCreation: PinSpritePath = Sprites/WoodPin (the `wood` flag above).
        let p = self.pins.len() - 1;
        let behaviour = super::Behaviour::Pin(p);
        self.behaviour_of.insert(id, (0, behaviour));
        self.behaviours.add(0, behaviour);
        self.pending_starts.push(super::misc::PendingStart::Pin(p));
    }

    /// PinBehaviour.Start: PinBehaviour.CreateJoint (the SpriteChild position and PinBehaviour.SyncChildPos via `pin_sync_child_pos`; the ToolInvisibilityTracker and
    /// OnPinCreated event are presentation/modding only).
    pub(crate) fn pin_start(&mut self, p: usize) {
        if self.pins[p].started || self.destroyed_components.contains(&self.pins[p].id) {
            return;
        }
        self.pin_start_run(p);
    }

    /// PinBehaviour.Start's body: CreateJoint, the new ToolInvisibilityTracker, OnPinCreated.
    pub(crate) fn pin_start_run(&mut self, p: usize) {
        self.pin_create_joint(p);
        // invisibilityTracker = new ToolInvisibilityTracker(this, SpriteRenderer)
        self.tracker_new(super::tool_invisibility::Owner::Pin(p));
        self.pins[p].started = true;
        // ModdingApi.Events.OnPinCreated.Invoke(this)
        self.mod_event(&super::mods::mod_api::OnPinCreated, self.pins[p].id);
    }

    /// PinBehaviour.CreateJoint: a new HingeJoint2D (an earlier one stays), then the SpriteChild.
    pub(crate) fn pin_create_joint(&mut self, p: usize) {
        let (physical, threshold, anchor, other) = {
            let x = &self.pins[p];
            (x.physical, x.BreakingThreshold, x.LocalAnchor, x.Other)
        };
        let root = self.pins[p].root;
        let f = match root {
            None => {
                self.allocate_instance_id();
                self.add_hinge_joint_component(physical)
            }
            Some(tid) => {
                let rb_id = self.allocate_instance_id();
                self.allocate_instance_id();
                let k = self.with_component_instance(tid, |g, inst, o| g.add_rigidbody2d_component(inst, o, rb_id));
                self.add_hinge_joint_component_on(BodyOwner::Node(k))
            }
        };
        self.pins[p].Joint = Some(f);
        let force = self.calculate_break_force_for_cable(f, threshold, 5.0, 1.0);
        self.fixed_joint_set_break_force(f, force);
        self.pins[p].initialBreakForce = force;
        self.fixed_joint_set_anchor(f, anchor);
        if let Some(o) = other.filter(|_| self.pin_attached_to_wall(p)) {
            self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(o)));
        }
        self.pin_create_sprite_child(p);
    }

    /// PinBehaviour.CreateJoint's SpriteChild (after the joint): the Pin GameObject, its ParentConstraint and SpriteRenderer.
    pub(crate) fn pin_create_sprite_child(&mut self, p: usize) {
        let (physical, other, wood, root) = {
            let x = &self.pins[p];
            (x.physical, x.Other, x.wood, x.root)
        };
        // The "Pin" GameObject, its Transform and ParentConstraint, the SpriteRenderer, then Optout.
        self.allocate_instance_ids(2);
        let constraint_id = self.allocate_instance_id();
        // AddComponent<ParentConstraint>, AddSource(base.transform, weight 1), SyncChildPos, constraintActive = true.
        let mut constraint = ParentConstraint::new(constraint_id, ConstrainedTarget::Renderer(self.renderers.len()));
        constraint.add_source(root.map_or(ConstraintSource::Physical(physical), ConstraintSource::Object), 1.0);
        self.pins[p].constraint = Some(self.parent_constraints.len());
        self.parent_constraints.push(constraint);
        self.pin_sync_child_pos(p);
        self.parent_constraints[self.pins[p].constraint.unwrap()].active = true;
        let renderer_id = self.allocate_instance_id();
        self.allocate_instance_id();
        // Utils.GetTopMostLayer(the other's SpriteRenderer, the own one): the other's only when its layer is strictly above.
        let renderer = |g: &Game, q: usize| g.physicals[q].sprite.map(|(_, _, order)| (g.physicals[q].sprite_layer, order));
        let own = match root {
            None => renderer(self, physical),
            // GetComponent<SpriteRenderer>() of the object: a drawn one's live sorting, else the prefab's.
            Some(tid) => self.with_component_instance(tid, |g, inst, o| {
                inst.prefab.objects[o].components.iter().enumerate().find_map(|(c, (_, x))| match x {
                    crate::assets::db::Component::SpriteRenderer(r) => {
                        let id = inst.objects[o].components[c];
                        Some(g.renderers.iter().find(|q| q.id == id).map_or((r.sorting_layer_id, r.sorting_order), |q| (q.layer, q.sprite.2)))
                    }
                    _ => None,
                })
            }),
        };
        let other = other.and_then(|o| renderer(self, o));
        let value = |r: Option<(i32, i16)>| r.map_or(0, |(layer, _)| crate::play::sorting_layer_index(layer) + 1);
        let (layer, order) = match (other, own) {
            (None, None) => (crate::play::SORTING_LAYERS[6] as i32, 100),
            (Some(a), _) if value(other) > value(own) => a,
            _ => own.unwrap(),
        };
        let db = self.assets.as_ref().expect("Resources.Load without the game's data");
        let path = if wood { "Sprites/WoodPin" } else { "Sprites/Pin" };
        let sprite = db.resource(path, 213).ok().flatten().unwrap_or_else(|| panic!("Resources.Load({path}) found no Sprite"));
        let bounds = super::link::sprite_bounds(&db.sprite(sprite).unwrap());
        // SpriteChild.position = transform.TransformPoint(LocalAnchor), localScale one; a new GameObject's rotation is
        // identity until the ConstraintManager moves it.
        let position = self.pin_anchor_point(p);
        let trs = crate::unity::transform::Trs { t: [position[0], position[1], position[2], 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] };
        self.pins[p].renderer = Some(self.renderers.len());
        self.renderers.push(super::Renderer {
            anchor: super::BareAnchor::Root,
            chain: vec![trs],
            sprite: (sprite, [1.0; 4], order + 1),
            layer,
            game_layer: 0,
            material: None,
            id: renderer_id,
            draw: ([false, false], 0, [0.0, 0.0], 0.5, false),
            visible: true,
            enabled: true,
            bounds,
        });
        self.renderer_sync(self.renderers.len() - 1);
    }

    /// The pinned object's Transform: position, rotation and lossyScale.
    pub(super) fn pin_host_pose(&mut self, p: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
        match self.pins[p].root {
            None => {
                let x = self.pins[p].physical;
                let t = &self.physicals[x].transform;
                (t.position, t.rotation, self.lossy_scale(x))
            }
            Some(tid) => self.with_component_instance(tid, |_, inst, o| (inst.position(o), inst.rotation(o), inst.lossy_scale(o))),
        }
    }

    /// The pinned object's activeInHierarchy.
    pub(super) fn pin_host_active(&mut self, p: usize) -> bool {
        match self.pins[p].root {
            None => self.physicals[self.pins[p].physical].active,
            Some(tid) => self.with_component_instance(tid, |_, inst, o| inst.objects[o].active),
        }
    }

    /// transform.TransformPoint(LocalAnchor) of the pinned object (rotated about z).
    fn pin_anchor_point(&mut self, p: usize) -> [f32; 3] {
        let (position, [_, _, qz, qw], scale) = self.pin_host_pose(p);
        let a = self.pins[p].LocalAnchor;
        let (vx, vy) = (a[0] * scale[0], a[1] * scale[1]);
        // quaternion * vector about z.
        let (c, s) = (1.0 - 2.0 * qz * qz, 2.0 * qw * qz);
        [position[0] + c * vx - s * vy, position[1] + s * vx + c * vy, position[2]]
    }

    /// PinBehaviour.SyncChildPos: parentConstraint.SetTranslationOffset(0, LocalAnchor * transform.lossyScale) (a
    /// Vector2 product, z 0).
    pub(crate) fn pin_sync_child_pos(&mut self, p: usize) {
        let (_, _, scale) = self.pin_host_pose(p);
        let x = &self.pins[p];
        let offset = [x.LocalAnchor[0] * scale[0], x.LocalAnchor[1] * scale[1], 0.0];
        let c = x.constraint.expect("a pin's ParentConstraint");
        self.parent_constraints[c].sources[0].translation_offset = offset;
    }

    /// The pin sprite's GameObject before the ConstraintManager runs: OnDisable/OnEnable follow the pinned object's
    /// activity (SetActive on SpriteChild), OnDestroy destroys it (its ParentConstraint leaves the manager).
    pub(super) fn pins_constraint_update(&mut self) {
        for p in 0..self.pins.len() {
            let (Some(r), Some(c)) = (self.pins[p].renderer, self.pins[p].constraint) else { continue };
            if self.destroyed_components.contains(&self.pins[p].id) {
                self.parent_constraints[c].active = false;
                continue;
            }
            self.renderers[r].visible = self.pin_host_active(p);
            self.renderer_sync(r);
        }
    }

    /// PinBehaviour.get_AttachedToWall (PinBehaviour.GetVisualBounds is the renderer's world bounds, built from `Renderer.bounds` where pin_hover checks the mouse): `Other` as a bool, a Unity object that is not destroyed.
    pub(crate) fn pin_attached_to_wall(&self, p: usize) -> bool {
        self.pins[p].Other.is_some_and(|o| self.wire_physical_alive(o))
    }

    /// PinBehaviour.IsMouseInsideCollider: Vector2.SqrMagnitude(SpriteRenderer.transform.position - Global.main.MousePosition) < 0.005102041f.
    pub(crate) fn pin_is_mouse_inside_collider(&self, p: usize) -> bool {
        let [x, y, ..] = self.renderers[self.pins[p].renderer.expect("a started pin has a sprite")].chain[0].t;
        let m = self.MousePosition;
        mathf::sqr_magnitude([x - m[0], y - m[1]]) < MOUSE_RADIUS_SQUARED
    }

    /// PinBehaviour.CanBeDeleted: invisibilityTracker.ShouldBeVisible (the owner is alive here).
    pub(crate) fn pin_can_be_deleted(&self, p: usize) -> bool {
        self.tool_should_be_visible(self.pins[p].Invisible)
    }

    /// PinBehaviour.Update
    pub fn pin_update(&mut self, p: usize) {
        if self.paused_menu {
            return;
        }
        if !self.pins[p].started {
            return;
        }
        let (id, f) = (self.pins[p].id, self.pins[p].Joint.expect("a started pin has a joint"));
        let gone = self.pins[p].Other.is_some_and(|o| self.wire_physical_alive(o) && self.physicals[o].isDisintegrated);
        if (!self.wire_connected_alive(f) && self.pins[p].UsedToHaveConnectedBody) || gone {
            self.tr_destroy_component(id, 0.0);
            return;
        }
        self.pin_sync_child_pos(p);
        // Hover.CheckMouseInput (no dialog or menu is open); the hover events are presentation. The sprite is where the
        // ParentConstraint left it last frame.
        let r = &self.renderers[self.pins[p].renderer.expect("a started pin has a sprite")];
        let n = &r.chain[0];
        let ([x, y, ..], [_, _, qz, qw]) = (n.t, n.q);
        let m = self.MousePosition;
        let (centre, extent) = r.bounds;
        let (c, s) = (1.0 - 2.0 * qz * qz, 2.0 * qw * qz);
        let corners = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]].map(|[a, b]: [f32; 2]| {
            let (vx, vy) = (centre[0] + a * extent[0], centre[1] + b * extent[1]);
            [x + c * vx - s * vy, y + s * vx + c * vy]
        });
        let inside = (0..2).all(|i| {
            let (lo, hi) = corners.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v[i]), hi.max(v[i])));
            m[i] > lo && m[i] < hi
        });
        let near = self.pin_is_mouse_inside_collider(p);
        self.hover_track(id, inside, inside && near);
        if inside && near && self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) && self.pin_can_be_deleted(p) {
            // OnUserDelete is empty.
            self.tr_destroy_component(id, 0.0);
        }
        // Hover.CheckMouseInput: OnMouseEnter or OnMouseExit when IsMouseOverlapping changes (OnMouseOverlapEvent is empty).
        let flag = inside && near;
        if self.pins[p].IsMouseOverlapping != flag {
            self.pins[p].IsMouseOverlapping = flag;
            let o = super::tool_invisibility::Owner::Pin(p);
            if flag { self.tracker_on_mouse_enter(o) } else { self.tracker_on_mouse_exit(o) }
        }
    }

    /// PinBehaviour.FixedUpdate
    pub fn pin_fixed_update(&mut self, p: usize) {
        let initial = self.pins[p].initialBreakForce;
        if !self.pins[p].started || initial == f32::INFINITY {
            return;
        }
        let mut num = 0.0;
        // `(bool)PhysicalBehaviour`
        if self.pins[p].root.is_none() && self.wire_physical_alive(self.pins[p].physical) {
            num = self.physicals[self.pins[p].physical].BurnProgress;
        }
        if let Some(o) = self.pins[p].Other.filter(|&o| self.wire_physical_alive(o)) {
            num = mathf::max(num, self.physicals[o].BurnProgress);
        }
        let f = self.pins[p].Joint.expect("a started pin has a joint");
        self.fixed_joint_set_break_force(f, (d(initial) * (d(1.0) - d(num))).f());
    }

    /// PinBehaviour.OnJointBreak2D
    pub(crate) fn pin_on_joint_break(&mut self, f: usize) {
        for p in 0..self.pins.len() {
            self.pin_on_joint_break_one(p, f);
        }
    }

    /// PinBehaviour.OnJointBreak2D of pin `p`: `broken == Joint` queues Object.Destroy(this).
    pub(crate) fn pin_on_joint_break_one(&mut self, p: usize, f: usize) {
        if self.pins[p].Joint == Some(f) && !self.destroyed_components.contains(&self.pins[p].id) {
            self.tr_destroy_component(self.pins[p].id, 0.0);
        }
    }

    /// PinBehaviour.Slice of every pin on the physical.
    pub(crate) fn pin_slice(&mut self, physical: usize) {
        for p in 0..self.pins.len() {
            if self.pins[p].physical == physical && self.pins[p].started && !self.destroyed_components.contains(&self.pins[p].id) {
                self.pin_slice_one(p);
            }
        }
    }

    /// PinBehaviour.Slice: the joint's breakForce and breakTorque 0 (a destroyed Joint throws).
    pub(crate) fn pin_slice_one(&mut self, p: usize) {
        let Some(f) = self.pins[p].Joint.filter(|&f| !self.fixed_joints[f].destroyed) else {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object")
        };
        self.fixed_joint_set_break_force(f, 0.0);
        self.fixed_joint_set_break_torque(f, 0.0);
    }

    /// PinBehaviour.GetVisualBounds: SpriteRenderer.bounds (center, extents).
    pub(crate) fn pin_visual_bounds(&self, p: usize) -> ([f32; 3], [f32; 3]) {
        let r = &self.renderers[self.pins[p].renderer.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))];
        crate::unity::transform::transform_aabb(r.bounds.0, r.bounds.1, &r.chain)
    }

    /// PinBehaviour.OnDestroy: the delayed Destroy of a PinBehaviour (false: `id` is not one). Queues the joint's Destroy.
    pub fn pin_run_destroy(&mut self, id: i32) -> bool {
        let Some(p) = self.pins.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            self.pin_destroy(p);
            if let Some(f) = self.pins[p].Joint.filter(|&f| !self.fixed_joints[f].destroyed && !self.joint_destroys.iter().any(|&(_, j)| j == f)) {
                self.destroy_joint_delayed(f);
            }
        }
        true
    }

    /// A pin on a GameObject that is being destroyed: the pin, then the joint added after it.
    pub(super) fn pin_object_destroy(&mut self, components: &[i32]) {
        for p in 0..self.pins.len() {
            let id = self.pins[p].id;
            let host = self.pins[p].root.unwrap_or_else(|| self.physicals[self.pins[p].physical].id);
            if components.contains(&host) && self.destroyed_components.insert(id) {
                self.component_destroys.retain(|&(_, i)| i != id);
                self.pin_destroy(p);
                if let Some(f) = self.pins[p].Joint {
                    self.run_joint_destroy(f);
                }
            }
        }
    }

    // PinBehaviour.OnEnable: SpriteChild SetActive(true): drawn through `renderers[r].visible` (pins_constraint_update, the physical's active state; the component is never disabled by itself).
    // PinBehaviour.OnDisable: SpriteChild SetActive(false): drawn through `renderers[r].visible` (pins_constraint_update).
    // PinBehaviour.OnUserDelete: empty.
    pub(crate) fn pin_destroy(&mut self, p: usize) {
        let id = self.pins[p].id;
        // OnDestroy: invisibilityTracker.Dispose(), then ModdingApi.Events.OnPinDestroyed.Invoke(this).
        self.tracker_dispose(super::tool_invisibility::Owner::Pin(p));
        self.mod_event(&super::mods::mod_api::OnPinDestroyed, id);
        // Hover.OnDestroy: CurrentlyHovering = null when it is this pin.
        if self.hovering == Some(id) {
            self.hovering = None;
        }
        self.stop_all_coroutines(id);
        if let Some((_, b)) = self.behaviour_of.remove(&id) {
            self.behaviours.remove(b);
        }
        // OnDestroy: Destroy(SpriteChild.gameObject).
        if let Some(r) = self.pins[p].renderer {
            self.renderers[r].visible = false;
            self.renderer_sync(r);
        }
    }
}

#[cfg(test)]
mod small_tests {
    use super::*;

    #[test]
    fn pin_without_other_is_not_attached_to_wall() {
        let mut g = Game::default();
        g.pins.push(Pin { id: 0, physical: 0, root: None, Other: None, LocalAnchor: [0.0; 2], BreakingThreshold: 0.0, UsedToHaveConnectedBody: false, Invisible: false, wood: false, Joint: None, initialBreakForce: 0.0, started: false, renderer: None, constraint: None, IsMouseOverlapping: false, tracker: Default::default() });
        assert!(!g.pin_attached_to_wall(0));
    }
}
