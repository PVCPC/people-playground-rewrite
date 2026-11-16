//! PhaseLinkTool (a LinkDeviceTool) and the PhaseLinkBehaviour (a LinkDeviceBehaviour) it adds. The tool drags from the
//! selected object to the object under the mouse; on release the selected object gets a PhaseLinkBehaviour whose Start
//! makes every collider below the one root ignore every collider below the other's. OnDestroy undoes that while both
//! ends live; the link destroys itself once the other end's object is gone (its target sprite goes with it).
//!
//! The link's line (`Game::lines`, lines.rs) and its two end sprites (`Game::renderers`) are drawn; the tool's LineRenderer,
//! the GUID in the end objects' names and the modding events are not. Each end's VisualDeletableDetachedToolBehaviour is a
//! hover over its sprite: ExistInDetailView keeps the end active only while ShowLimbStatus is on, and deleting an end ends
//! the link.
//! The link's own Hover never sees the mouse while ShowLimbStatus is off (its line is disabled).
#![allow(non_snake_case)]

use super::undo::ObjectCreationAction;
use super::{Behaviour, Game};

/// LayerMask.NameToLayer("ScreenUI")
const SCREEN_UI: i32 = 16;

/// One PhaseLinkBehaviour on `physical`'s GameObject.
#[derive(Clone, Debug, Default)]
pub struct PhaseLink {
    /// The component's instance id.
    pub id: i32,
    /// PhysicalBehaviour: GetComponent in Awake.
    pub physical: usize,
    /// A link loaded onto an object without a PhysicalBehaviour: that object's Transform id (`physical` is usize::MAX). Its
    /// Start throws in PrepareSpriteRendererOrder (`start_threw`), and its first unpaused Update destroys it.
    pub root: Option<i32>,
    pub start_threw: bool,
    /// Other: null only for a loaded link whose stored Other was not found (Start destroys it).
    pub Other: Option<usize>,
    /// LinkDeviceBehaviour.Invisible (the tool never sets it; a load can).
    pub Invisible: bool,
    pub LocalOffset: [f32; 2],
    pub OtherLocalOffset: [f32; 2],
    /// Start ran (Update waits for it).
    pub started: bool,
    /// Hover.IsMouseInsideBounds and Hover.IsMouseOverlapping
    pub IsMouseInsideBounds: bool,
    pub IsMouseOverlapping: bool,
    /// The VisualDeletableDetachedToolBehaviours on "link source" (a child of `physical`) and "link target" (a child of
    /// Other): their instance ids and Hover.IsMouseOverlapping.
    pub ends: [i32; 2],
    pub end_overlapping: [bool; 2],
    pub end_inside: [bool; 2],
    /// The line (`Game::lines`) on "link source" and the end SpriteRenderers (`Game::renderers`), from Start.
    pub line: Option<usize>,
    pub end_renderers: [Option<usize>; 2],
    /// The line's two world positions at its last SetPosition (the enabled updates); lineRenderer.bounds of a
    /// useWorldSpace line holds them while the renderer is disabled.
    pub last_line: Option<[[f32; 3]; 2]>,
}

/// The PhaseLinkTool on the ToolController.
#[derive(Clone, Debug)]
pub struct LinkTool {
    /// It is the current tool.
    pub chosen: bool,
    /// The component was added.
    pub created: bool,
    pub startPos: [f32; 2],
    pub startOffset: [f32; 2],
    pub hasDragged: bool,
    /// LinkDeviceTool.WireWidth (0.1; PhaseLinkTool.Awake 0.08) and WireColor (white): OnSelect's line width and colours.
    pub WireWidth: f32,
    pub WireColor: [f32; 4],
}

/// LinkDeviceTool's field initialisers.
impl Default for LinkTool {
    fn default() -> Self {
        LinkTool { chosen: false, created: false, startPos: [0.0; 2], startOffset: [0.0; 2], hasDragged: false, WireWidth: 0.1, WireColor: [1.0; 4] }
    }
}

impl Game {
    // LinkDeviceTool.OnToolUnchosen: empty.
    // LinkDeviceTool.OnFixedHold: empty.
    /// PhaseLinkTool.Awake, LinkDeviceTool.OnToolChosen
    /// SetTool: AddComponent<PhaseLinkTool> the first time (Awake loads the material), then OnToolChosen
    /// (GetComponent<LineRenderer>, or AddComponent: the one the wire tools share).
    pub(crate) fn link_tool_choose(&mut self) {
        if !self.link_tool.created {
            self.allocate_instance_id();
            self.link_tool.created = true;
            self.phase_link_tool_awake();
        }
        if !self.wire_tool.has_line {
            self.allocate_instance_id();
            self.wire_tool.has_line = true;
        }
        self.link_tool.chosen = true;
    }

    /// PhaseLinkTool.Awake: WireMaterial = Resources.Load("Materials/PhaseLink") (the line's, presentation), WireWidth 0.08,
    /// WireColor white.
    pub(crate) fn phase_link_tool_awake(&mut self) {
        self.link_tool.WireWidth = 0.08;
        self.link_tool.WireColor = [1.0; 4];
    }

    /// LinkDeviceTool.OnSelect (the LineRenderer is presentation).
    pub(crate) fn link_on_select(&mut self) {
        let m = self.MousePosition;
        self.link_tool.startPos = [m[0], m[1]];
        if let Some(sel) = self.wire_selected() {
            let o = self.inverse_transform_point(sel, [m[0], m[1], 0.0]);
            self.link_tool.startOffset = [o[0], o[1]];
        }
        self.tool_line_or_throw(self.link_tool.chosen || self.link_tool.created);
        self.link_tool.hasDragged = true;
    }

    /// LinkDeviceTool.OnHold
    pub(crate) fn link_on_hold(&mut self) {
        if let Some(sel) = self.wire_selected() {
            let o = self.link_tool.startOffset;
            let p = self.transform_point3(sel, [o[0], o[1], 0.0]);
            self.link_tool.startPos = [p[0], p[1]];
        }
        self.tool_line_or_throw(self.link_tool.chosen || self.link_tool.created);
    }

    /// LinkDeviceTool.GetEndObject, SerpentineBeltTool.GetEndObject: endBody is SelectionController.Main.CurrentlyUnderMouse's
    /// body unless that object is on layer 11 (then endBody stays null).
    pub(crate) fn link_end_body(&self) -> Option<usize> {
        let under = self.tool.as_ref().and_then(|t| t.hover).filter(|&p| !self.physicals[p].destroyed);
        under.filter(|&p| self.physical_game_object_layer(p) != 11)
    }

    /// LinkDeviceTool.OnDeselect: GetEndObject is the object under the mouse unless it is on layer 11.
    pub(crate) fn link_on_deselect(&mut self) {
        if !self.link_tool.hasDragged {
            return;
        }
        let start = self.link_tool.startPos;
        let end_body = self.link_end_body();
        if let (Some(a), Some(b)) = (self.wire_selected(), end_body) {
            let from = self.inverse_transform_point(a, [start[0], start[1], 0.0]);
            let m = self.MousePosition;
            let end = self.wire_end_point([m[0], m[1]], start);
            let to = self.inverse_transform_point(b, [end[0], end[1], 0.0]);
            self.phase_link_create([from[0], from[1]], [to[0], to[1]], a, b);
        }
        self.link_tool.hasDragged = false;
    }

    /// PhaseLinkTool.OnLinkCreate: AddComponent (Awake: GetComponent<PhysicalBehaviour>), the undo action, Other and the
    /// offsets (Invisible = ToolVisibility == Invisible is never set).
    pub(crate) fn phase_link_create(&mut self, from: [f32; 2], to: [f32; 2], a: usize, b: usize) {
        // LinkDeviceBehaviour.Awake: PhysicalBehaviour = GetComponent<PhysicalBehaviour>() is `physical`.
        let id = self.allocate_instance_id();
        self.add_script_to_clone(self.physicals[a].id, "PhaseLinkBehaviour", id);
        self.phase_links.push(PhaseLink { id, physical: a, Other: Some(b), LocalOffset: from, OtherLocalOffset: to, ..Default::default() });
        let l = self.phase_links.len() - 1;
        let behaviour = Behaviour::PhaseLink(l);
        // PhaseLinkBehaviour's MonoScript execution order is 0.
        self.behaviour_of.insert(id, (0, behaviour));
        self.behaviours.add(0, behaviour);
        self.pending_starts.push(super::misc::PendingStart::PhaseLink(l));
        self.undo_register_action(ObjectCreationAction { displayName: crate::il!(str "PhaseLinkTool::OnLinkCreate" 0).into(), RelevantObjects: vec![id] });
    }

    /// transform.root.GetComponentsInChildren<Collider2D>() of a physical's object.
    fn link_root_colliders(&mut self, p: usize) -> Vec<i32> {
        if self.physicals[p].parent.is_some() {
            // Under a person's (or a re-parented) root: the active physicals there, as Penetration does (stab.rs).
            let root = self.root_of(p);
            return (0..self.physicals.len())
                .filter(|&q| !self.physicals[q].destroyed && self.physicals[q].active && self.root_of(q) == root)
                .flat_map(|q| self.physicals[q].all_colliders.clone())
                .collect();
        }
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, mut root| {
            while let Some(up) = inst.objects[root].parent {
                root = up;
            }
            g.colliders_in_children_with_clones(inst, root)
        })
    }

    /// GetComponentsInChildren<Collider2D>() from object `o` of a checked-out clone, depth first over active objects:
    /// its own colliders, its children's subtrees, then the clones Instantiated under it (the last children, in the
    /// order they were attached).
    fn colliders_in_children_with_clones(&mut self, inst: &super::spawn::Instance, o: usize) -> Vec<i32> {
        if !inst.objects[o].active {
            return Vec::new();
        }
        let prefab = &inst.prefab;
        let mut out: Vec<i32> = (0..prefab.objects[o].components.len())
            .filter(|&k| matches!(prefab.objects[o].components[k].1, crate::assets::db::Component::Collider2D(_)))
            .map(|k| inst.objects[o].components[k])
            .collect();
        for c in inst.descendants(o).into_iter().filter(|&d| inst.objects[d].parent == Some(o)) {
            out.extend(self.colliders_in_children_with_clones(inst, c));
        }
        let clones: Vec<i32> = self.attached_clones.get(&inst.objects[o].components[0]).map(|v| v.iter().map(|c| c.id).collect()).unwrap_or_default();
        for id in clones {
            out.extend(self.with_component_instance(id, |g, clone, r| g.colliders_in_children_with_clones(clone, r)));
        }
        out
    }

    /// Every collider of the one root against every other one of the other's: ignore or stop ignoring.
    fn phase_link_ignore(&mut self, l: usize, ignore: bool) {
        let (a, b) = (self.phase_links[l].physical, self.phase_links[l].Other.expect("a started link's Other"));
        for c1 in self.link_root_colliders(a) {
            for c2 in self.link_root_colliders(b) {
                if c1 != c2 {
                    // RequestIgnoreCollision / RequestDontIgnoreCollision
                    self.ignore_collision_substitute(c1, c2, ignore);
                }
            }
        }
    }

    /// LinkDeviceBehaviour.Start (its last call: AfterInitialise).
    pub fn phase_link_start(&mut self, l: usize) {
        let id = self.phase_links[l].id;
        if self.destroyed_components.contains(&id) {
            return;
        }
        if self.phase_links[l].Other.is_none_or(|o| self.physicals[o].destroyed) {
            self.tr_destroy_component(id, 0.0);
            return;
        }
        if self.phase_links[l].root.is_some() {
            // "link source": GameObject, Transform, Optout, LineRenderer; PrepareSpriteRendererOrder(PhysicalBehaviour) reads
            // GetComponent of null. The line keeps a new LineRenderer's (0,0,0), (0,0,1): nothing to draw.
            for _ in 0..4 {
                self.allocate_instance_id();
            }
            self.phase_links[l].start_threw = true;
            crate::game::menu::npe();
        }
        // "link source": GameObject, Transform, Optout, LineRenderer, SpriteRenderer, ExistInDetailView; "link target":
        // GameObject, Transform, Optout, SpriteRenderer, ExistInDetailView; a VisualDeletableDetachedToolBehaviour on each.
        // LinkDeviceBehaviour.PrepareSpriteRendererOrder: the SpriteRenderer, its sorting and ExistInDetailView.
        let ids: Vec<i32> = (0..11).map(|_| self.allocate_instance_id()).collect();
        let other = self.phase_links[l].Other.unwrap();
        self.phase_link_visuals(l, other, ids[3], [ids[4], ids[9]]);
        // VisualDeletableDetachedToolBehaviour.Awake: spriteRenderer = GetComponent<SpriteRenderer>(). Execution order 0.
        // ExistInDetailView.Start runs with the other Starts before any Update: the ends update only while ShowLimbStatus is on.
        for end in 0..2 {
            let id = self.allocate_instance_id();
            self.phase_links[l].ends[end] = id;
            let behaviour = Behaviour::LinkEnd(l, end);
            self.behaviour_of.insert(id, (0, behaviour));
            self.behaviours.add(0, behaviour);
        }
        // ModdingApi.Events.OnLinkCreated.Invoke(this), then AfterInitialise.
        self.mod_event(&super::mods::mod_api::OnLinkCreated, self.phase_links[l].id);
        self.phase_link_after_initialise(l);
        self.phase_links[l].started = true;
    }

    /// PhaseLinkBehaviour.AfterInitialise: every collider of its root against every other one of Other's root, ignored.
    pub fn phase_link_after_initialise(&mut self, l: usize) {
        self.phase_link_ignore(l, true);
    }

    /// LinkDeviceBehaviour.Update
    pub fn phase_link_update(&mut self, l: usize) {
        if self.paused_menu {
            return;
        }
        // PhaseLinkBehaviour.GetDeviceSprite: the wire's end sprite (rendering).
        // PhaseLinkBehaviour.GetWireMaterial: the PhaseLink material (rendering).
        if !self.phase_links[l].started {
            if self.phase_links[l].start_threw {
                // !fromSpriteRenderer
                let id = self.phase_links[l].id;
                self.tr_destroy_component(id, 0.0);
            }
            return;
        }
        // !fromSpriteRenderer || !toSpriteRenderer: the target sprite is a child of the other end's object; a deleted end
        // took its sprite with it.
        if self.physicals[self.phase_links[l].Other.unwrap()].destroyed || self.phase_links[l].ends.iter().any(|e| self.destroyed_components.contains(e)) {
            let id = self.phase_links[l].id;
            self.tr_destroy_component(id, 0.0);
            return;
        }
        // lineRenderer.enabled = ShowLimbStatus: a disabled line is never inside its collider (IsMouseInsideCollider).
        if let Some(i) = self.phase_links[l].line {
            self.lines[i].enabled = self.ShowLimbStatus;
        }
        if self.ShowLimbStatus {
            let (a, b) = (self.phase_links[l].physical, self.phase_links[l].Other.unwrap());
            let (from, to) = (self.phase_links[l].LocalOffset, self.phase_links[l].OtherLocalOffset);
            let line = [self.transform_point3(a, [from[0], from[1], 0.0]), self.transform_point3(b, [to[0], to[1], 0.0])];
            self.phase_links[l].last_line = Some(line);
            // lineRenderer.SetPosition(0, from), SetPosition(1, to)
            if let Some(i) = self.phase_links[l].line {
                self.lines[i].positions = line.to_vec();
            }
            self.link_device_check_mouse_input(l, line);
        } else {
            self.link_device_check_mouse_input_disabled(l);
        }
    }

    /// VisualDeletableDetachedToolBehaviour.Update (its GameObject is active: ShowLimbStatus is on): Hover.CheckMouseInput with
    /// GetVisualBounds and IsMouseInsideCollider both spriteRenderer.bounds (the PhaseLink sprite at the end's offset under its
    /// object; Bounds.Contains is inclusive), CanBeDeleted true, OnUserDelete Object.Destroy(gameObject).
    /// VisualDeletableDetachedToolBehaviour.OnMouseOverlapEvent: the sprite's colour and layer (presentation).
    pub fn link_end_update(&mut self, l: usize, end: usize) {
        if !self.ShowLimbStatus || self.input.ui_block {
            return;
        }
        let id = self.phase_links[l].ends[end];
        let (c, e) = self.link_end_bounds(l, end);
        let m = self.MousePosition;
        // Utils.IsPointInsideBounds (strict) for the bounds, Bounds.Contains for the collider.
        let inside_bounds = super::utils::is_point_inside_bounds([c[0], c[1]], [e[0], e[1]], [m[0], m[1]]);
        let inside = inside_bounds && self.link_end_inside_collider(l, end);
        self.phase_links[l].end_inside[end] = inside_bounds;
        self.hover_track(id, inside_bounds, inside);
        if inside && self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) {
            // VisualDeletableDetachedToolBehaviour.OnUserDelete: Object.Destroy(gameObject).
            self.tr_destroy_component(id, 0.0);
        }
        if self.phase_links[l].end_overlapping[end] != inside {
            self.phase_links[l].end_overlapping[end] = inside;
            self.link_end_overlap_event(l, end, inside);
        }
    }

    /// VisualDeletableDetachedToolBehaviour.GetVisualBounds: spriteRenderer.bounds, the PhaseLink sprite under the end object (its own
    /// local TRS) under its parent (centre, extents).
    pub(crate) fn link_end_bounds(&self, l: usize, end: usize) -> ([f32; 3], [f32; 3]) {
        let p = match end {
            0 => self.phase_links[l].physical,
            _ => self.phase_links[l].Other.unwrap(),
        };
        let db = self.assets.as_ref().expect("Resources.Load without the game's data");
        let sprite = db.resource("Sprites/PhaseLink", 213).ok().flatten().expect("Resources.Load(Sprites/PhaseLink) found no Sprite");
        let (c, e) = super::link::sprite_bounds(&db.sprite(sprite).unwrap());
        let mut chain = self.local_chain(p);
        // The end object's own local TRS (Start's SetParent kept it world-aligned at scale 1).
        chain.push(self.renderers[self.phase_links[l].end_renderers[end].unwrap()].chain[0]);
        chain.reverse();
        crate::unity::transform::transform_aabb(c, e, &chain)
    }

    /// VisualDeletableDetachedToolBehaviour.IsMouseInsideCollider: spriteRenderer.bounds.Contains(MousePosition) (inclusive).
    pub(crate) fn link_end_inside_collider(&self, l: usize, end: usize) -> bool {
        let (c, e) = self.link_end_bounds(l, end);
        let m = self.MousePosition;
        (0..3).all(|i| m[i] >= c[i] - e[i] && m[i] <= c[i] + e[i])
    }

    /// VisualDeletableDetachedToolBehaviour.OnMouseOverlapEvent: base (empty), then the sprite's colour red and its GameObject's
    /// layer ScreenUI while overlapping with ShowOutlines on, else white and Default. "link source" holds the line too.
    pub(crate) fn link_end_overlap_event(&mut self, l: usize, end: usize, overlap: bool) {
        let on = overlap && self.prefs.ShowOutlines;
        if let Some(r) = self.phase_links[l].end_renderers[end] {
            self.renderers[r].sprite.1 = if on { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
        }
        self.link_object_layer(l, end, if on { SCREEN_UI } else { 0 });
    }

    /// The "link source" (end 0: the line and its sprite) or "link target" GameObject's layer.
    fn link_object_layer(&mut self, l: usize, end: usize, layer: i32) {
        if let Some(r) = self.phase_links[l].end_renderers[end] {
            self.renderers[r].game_layer = layer;
        }
        if let (0, Some(i)) = (end, self.phase_links[l].line) {
            self.lines[i].game_layer = layer;
        }
    }

    /// Hover.CheckMouseInput's OnMouseOverlapEvent when IsMouseOverlapping changes. LinkDeviceBehaviour.OnMouseOverlapEvent: the
    /// line's sharedMaterial Resources "Materials/DeleteWire" and its GameObject's layer ScreenUI while overlapping with ShowOutlines
    /// on, else initialMaterial and Default.
    fn phase_link_overlap(&mut self, l: usize, flag: bool) {
        if self.phase_links[l].IsMouseOverlapping == flag {
            return;
        }
        self.phase_links[l].IsMouseOverlapping = flag;
        self.link_device_overlap_event(l, flag);
    }

    /// LinkDeviceBehaviour.OnMouseOverlapEvent: the line's material and its GameObject's layer.
    pub(crate) fn link_device_overlap_event(&mut self, l: usize, flag: bool) {
        let on = flag && self.prefs.ShowOutlines;
        if let Some(i) = self.phase_links[l].line {
            let db = self.assets.as_ref().expect("Resources.Load without the game's data");
            let path = if on { "Materials/DeleteWire" } else { "Materials/PhaseLink" };
            self.lines[i].material = Some(db.resource(path, 21).ok().flatten().unwrap_or_else(|| panic!("Resources.Load({path}) found no Material")));
        }
        self.link_object_layer(l, 0, if on { SCREEN_UI } else { 0 });
    }

    /// LinkDeviceBehaviour.Start's line and PrepareSpriteRendererOrder's sprites: "link source" (a child of the object at
    /// LocalOffset) gets the LineRenderer (useWorldSpace, PhaseLinkBehaviour.GetWireMaterial "Materials/PhaseLink", width 0.08,
    /// white) and a SpriteRenderer; "link target" (a child of Other at OtherLocalOffset) a SpriteRenderer. Each sprite is
    /// GetDeviceSprite ("Sprites/PhaseLink") on its object's SpriteRenderer's sorting layer, one order above it. ExistInDetailView.Start
    /// leaves both objects active only while ShowLimbStatus is on.
    fn phase_link_visuals(&mut self, l: usize, other: usize, line_id: i32, sprite_ids: [i32; 2]) {
        let db = self.assets.as_ref().expect("Resources.Load without the game's data");
        let material = db.resource("Materials/PhaseLink", 21).ok().flatten().expect("Resources.Load(Materials/PhaseLink) found no Material");
        let on = self.ShowLimbStatus;
        let mut gradient = super::visual::Gradient::default();
        gradient.set_start_color(phase_link_wire_color());
        gradient.set_end_color(phase_link_wire_color());
        self.phase_links[l].line = Some(self.lines.len());
        self.lines.push(super::line::LineVisual {
            id: line_id,
            anchor: super::BareAnchor::Root,
            chain: Vec::new(),
            // A new LineRenderer's two positions.
            positions: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            width: phase_link_wire_width(),
            curve: crate::wires::WidthCurve::constant(1.0),
            gradient,
            material: Some(material),
            layer: 0,
            order: 0,
            game_layer: 0,
            cap_vertices: 0,
            corner_vertices: 0,
            alignment: 0,
            looped: false,
            texture_mode: 0,
            use_world_space: true,
            enabled: true,
            visible: on,
        });
        let ends = [(self.phase_links[l].physical, self.phase_links[l].LocalOffset), (other, self.phase_links[l].OtherLocalOffset)];
        for (end, (p, offset)) in ends.into_iter().enumerate() {
            // new GameObject (world origin, identity, scale 1), then transform.SetParent(parent) (worldPositionStays): the
            // local rotation is the parent's world rotation undone (SetPositionAndRotation's world->local), the local scale
            // the parent's lossy scale undone (InverseSafe: a zero axis stays 0); then localPosition = the offset.
            let parents: Vec<crate::unity::transform::Trs> = self.local_chain(p).into_iter().rev().collect();
            let ((_, q), _) = crate::unity::transform::set_world(&parents, [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
            let ls = self.lossy_scale(p).map(|v| if v.abs() < 1e-5 { 0.0 } else { 1.0 / v });
            let local = crate::unity::transform::Trs { t: [offset[0], offset[1], 0.0, 0.0], q, s: [ls[0], ls[1], ls[2], 0.0] };
            self.phase_links[l].end_renderers[end] = Some(self.link_prepare_sprite_renderer_order(p, local, sprite_ids[end]));
        }
    }

    /// LinkDeviceBehaviour.PrepareSpriteRendererOrder(phys, container): the container (`local`, its transform under `p`) gets
    /// a SpriteRenderer (`id`) with GetDeviceSprite (PhaseLinkBehaviour's "Sprites/PhaseLink", the only subclass) on phys's
    /// SpriteRenderer's sorting layer, one order above it, then an ExistInDetailView (its Start comes later). Returns the
    /// renderer's index.
    pub(crate) fn link_prepare_sprite_renderer_order(&mut self, p: usize, local: crate::unity::transform::Trs, id: i32) -> usize {
        let db = self.assets.as_ref().expect("Resources.Load without the game's data");
        let sprite = db.resource("Sprites/PhaseLink", 213).ok().flatten().expect("Resources.Load(Sprites/PhaseLink) found no Sprite");
        let bounds = super::link::sprite_bounds(&db.sprite(sprite).unwrap());
        let order = match self.physicals[p].sprite {
            Some((_, _, order)) => order,
            // A recording's limbs (a --loop-test) carry no sprite data; a replay draws nothing, so the order is unused.
            None if self.limbs.iter().any(|x| x.PhysicalBehaviour == p) => 0,
            None => unimplemented!("mod-only: LinkDeviceBehaviour.PrepareSpriteRendererOrder on an object without a SpriteRenderer"),
        };
        let on = self.ShowLimbStatus;
        self.renderers.push(super::Renderer {
            anchor: super::BareAnchor::Physical(p),
            chain: vec![local],
            sprite: (sprite, [1.0; 4], order + 1),
            layer: self.physicals[p].sprite_layer,
            game_layer: 0,
            material: None,
            id,
            draw: ([false, false], 0, [0.0, 0.0], 0.5, false),
            visible: on,
            enabled: true,
            bounds,
        });
        self.renderers.len() - 1
    }

    /// A loaded, started link's line and end sprites (the snapshot loader's; ids are not compared). `other`: the
    /// physical the "link target" object is under (Other, or the target's parent once Other was nulled).
    pub(crate) fn phase_link_load_visuals(&mut self, l: usize, other: usize) {
        self.phase_link_visuals(l, other, 0, [0, 0]);
    }

    /// A live link's two end objects, each its ExistInDetailView.Main_LimbStatusToggled: SetActive(on).
    pub(super) fn phase_link_detail_view(&mut self, on: bool) {
        for l in 0..self.phase_links.len() {
            if self.phase_links[l].started && !self.destroyed_components.contains(&self.phase_links[l].id) {
                self.phase_link_set_active(l, on);
            }
        }
    }

    fn phase_link_set_active(&mut self, l: usize, on: bool) {
        if let Some(i) = self.phase_links[l].line {
            self.lines[i].visible = on;
        }
        for r in self.phase_links[l].end_renderers.into_iter().flatten() {
            self.renderers[r].visible = on;
        }
    }

    /// Hover.CheckMouseInput of a link whose line is off (no dialog box or UI block is open): LinkDeviceBehaviour.IsMouseInsideCollider
    /// is false, so the pointer is outside the collider; the bounds still decide CurrentlyHovering.
    pub(crate) fn link_device_check_mouse_input_disabled(&mut self, l: usize) {
        // lineRenderer.bounds of a disabled useWorldSpace line: CalculateWorldMatrixAndBoundsJob (180ea4790) writes no
        // bounds for a world-space line, so they stay those of its last SetPosition (the hull of the two points, half the
        // width on each side). Only the bounds can end the hover: the collider test is false.
        let id = self.phase_links[l].id;
        if self.input.ui_block {
            return;
        }
        // A line never positioned was never inside its collider either: nothing can be hovering it, whatever its bounds.
        let Some(line) = self.phase_links[l].last_line else {
            self.phase_link_overlap(l, false);
            return;
        };
        let m = self.MousePosition;
        let inside_bounds = (0..2).all(|i| {
            let (lo, hi) = (line[0][i].min(line[1][i]), line[0][i].max(line[1][i]));
            let (c, e) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + 0.08 * 0.5);
            m[i] > c - e && m[i] < c + e
        });
        self.phase_links[l].IsMouseInsideBounds = inside_bounds;
        self.hover_track(id, inside_bounds, false);
        self.phase_link_overlap(l, false);
    }

    /// LinkDeviceBehaviour.GetVisualBounds, LinkDeviceBehaviour.IsMouseInsideCollider and LinkDeviceBehaviour.CanBeDeleted in Hover.CheckMouseInput.
    /// The bounds are lineRenderer.bounds of the two-point world line (width 0.08, half of it on each side), the collider is a
    /// segment within 0.04 of the mouse, CanBeDeleted is ShowLimbStatus (true here) and LinkDeviceBehaviour.OnUserDelete is empty.
    fn link_device_check_mouse_input(&mut self, l: usize, line: [[f32; 3]; 2]) {
        if self.input.ui_block {
            return;
        }
        let m = self.MousePosition;
        let inside_bounds = (0..2).all(|i| {
            let (lo, hi) = (line[0][i].min(line[1][i]), line[0][i].max(line[1][i]));
            let (c, e) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + 0.08 * 0.5);
            m[i] > c - e && m[i] < c + e
        });
        let id = self.phase_links[l].id;
        self.phase_links[l].IsMouseInsideBounds = inside_bounds;
        let mut flag = false;
        if inside_bounds {
            flag = super::blood_wire::sqrd_distance_to_segment([m[0], m[1]], [line[0][0], line[0][1]], [line[1][0], line[1][1]]) <= 0.0016;
        }
        self.hover_track(id, inside_bounds, flag);
        if flag && self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) {
            // LinkDeviceBehaviour.OnUserDelete: empty.
            self.tr_destroy_component(id, 0.0);
        }
        self.phase_link_overlap(l, flag);
    }

    /// LinkDeviceBehaviour.GetVisualBounds: lineRenderer.bounds, the hull of the line's two positions grown by half its width
    /// (0.08) on every axis (centre, extents).
    pub(crate) fn link_device_bounds(&self, l: usize) -> ([f32; 3], [f32; 3]) {
        let p = &self.lines[self.phase_links[l].line.expect("lineRenderer is null before Start")].positions;
        let (mut c, mut e) = ([0.0; 3], [0.0; 3]);
        for i in 0..3 {
            let (lo, hi) = (p[0][i].min(p[1][i]), p[0][i].max(p[1][i]));
            (c[i], e[i]) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + 0.08 * 0.5);
        }
        (c, e)
    }

    /// LinkDeviceBehaviour.IsMouseInsideCollider: false with the line off, else the mouse within 0.04 of its segment.
    pub(crate) fn link_device_inside_collider(&self, l: usize) -> bool {
        let Some(i) = self.phase_links[l].line.filter(|&i| self.lines[i].enabled) else { return false };
        let (p, m) = (&self.lines[i].positions, self.MousePosition);
        super::blood_wire::sqrd_distance_to_segment([m[0], m[1]], [p[0][0], p[0][1]], [p[1][0], p[1][1]]) <= 0.0016
    }

    /// The delayed Destroy of a PhaseLinkBehaviour (false: `id` is not one).
    pub fn phase_link_run_destroy(&mut self, id: i32) -> bool {
        let Some(l) = self.phase_links.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            if let Some((_, b)) = self.behaviour_of.remove(&id) {
                self.behaviours.remove(b);
            }
            self.phase_link_on_destroy(l);
        }
        true
    }

    /// PhaseLinkBehaviour.OnDestroy: while both ends live, their roots' colliders stop ignoring each other; then base.OnDestroy.
    pub(crate) fn phase_link_on_destroy(&mut self, l: usize) {
        let (a, b) = (self.phase_links[l].physical, self.phase_links[l].Other);
        if self.phase_links[l].root.is_none() && b.is_some_and(|b| !self.physicals[a].destroyed && !self.physicals[b].destroyed) {
            self.phase_link_ignore(l, false);
        }
        self.link_device_on_destroy(l);
    }

    /// LinkDeviceBehaviour.OnDestroy: ModdingApi.Events.OnLinkDestroyed.Invoke(this), Hover.OnDestroy (CurrentlyHovering = null
    /// when it is this one), then Destroy of both sprites' objects ("link source" holds the line).
    pub(crate) fn link_device_on_destroy(&mut self, l: usize) {
        let id = self.phase_links[l].id;
        self.mod_event(&super::mods::mod_api::OnLinkDestroyed, id);
        if self.hovering == Some(id) {
            self.hovering = None;
        }
        self.phase_link_set_active(l, false);
    }

    /// The links on a GameObject that is being destroyed: its PhysicalBehaviour is gone, so OnDestroy does nothing.
    pub(super) fn phase_link_object_destroy(&mut self, components: &[i32]) {
        for l in 0..self.phase_links.len() {
            let id = self.phase_links[l].id;
            let host = self.phase_links[l].root.unwrap_or_else(|| self.physicals[self.phase_links[l].physical].id);
            if components.contains(&host) && self.destroyed_components.insert(id) {
                // LinkDeviceBehaviour.OnDestroy: Destroy of both sprites' objects.
                self.phase_link_set_active(l, false);
                self.component_destroys.retain(|&(_, i)| i != id);
                if let Some((_, b)) = self.behaviour_of.remove(&id) {
                    self.behaviours.remove(b);
                }
            }
        }
    }
}

/// PhaseLinkBehaviour.GetWireColor: Color.white.
pub(crate) fn phase_link_wire_color() -> [f32; 4] {
    [1.0; 4]
}
/// PhaseLinkBehaviour.GetWireWidth: 0.08.
pub(crate) fn phase_link_wire_width() -> f32 {
    0.08
}
