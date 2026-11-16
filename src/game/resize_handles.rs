//! ResizeHandles: the scene's "Resizing handles" object, which the context menu's Resize points at the selected
//! objects. The handles' own Transforms live in the clone `Game.resize_handles`; Targets and the drag state are here.
use super::{d, mathf, Game};

/// ResizeHandles.Axis (X = 0, Y = 1): the enum's int, so a value outside both is kept as the game keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Axis(pub i32);
impl Axis {
    pub(crate) const X: Axis = Axis(0);
    pub(crate) const Y: Axis = Axis(1);
}

/// ResizeHandles.MultiTarget (the get_/set_ accessors are its auto-properties: Flipped, Scale, Position).
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct MultiTarget {
    pub(crate) flipped: bool,
    pub(crate) scale: [f32; 2],
    pub(crate) position: [f32; 2],
}

impl MultiTarget {
    /// MultiTarget..ctor(transform): Flipped false, Scale = (Vector2)localScale, Position = (Vector2)position.
    fn new(local_scale: [f32; 3], position: [f32; 3]) -> Self {
        MultiTarget { flipped: false, scale: [local_scale[0], local_scale[1]], position: [position[0], position[1]] }
    }
}

/// The ResizeHandles script's fields, with the clone objects of its two handles.
#[derive(Clone, Debug)]
pub struct ResizeHandles {
    pub Targets: Vec<usize>,
    pub HandleDistanceMultiplier: [f32; 2],
    pub(crate) initial_targets: Vec<MultiTarget>,
    pub(crate) multiplying_scale: f32,
    pub(crate) is_dragging: bool,
    pub(crate) current_drag_axis: Axis,
    pub IsHovering: bool,
    /// Whether the object is active (Update runs only then).
    pub active: bool,
    /// Reactivated after its first Update hid it: AddToManager put it at the end of the order-0 Update list.
    pub readded: bool,
    /// HorizontalHandle's and VerticalHandle's objects in the clone.
    pub handles: (usize, usize),
    /// HorizontalColour, VerticalColour, UniformColour.
    colours: [[f32; 4]; 3],
    /// HorizontalHandle's and VerticalHandle's `Game::renderers`, HorizontalLine's and VerticalLine's `Game::lines`.
    sprites: (usize, usize),
    lines: (usize, usize),
    /// Every SpriteRenderer and LineRenderer under the object: (clone object, is a line, its index).
    visuals: Vec<(usize, bool, usize)>,
}

impl Default for ResizeHandles {
    fn default() -> Self {
        ResizeHandles { Targets: Vec::new(), HandleDistanceMultiplier: [1.0, 1.0], initial_targets: Vec::new(), readded: false, multiplying_scale: 1.0, is_dragging: false, current_drag_axis: Axis::X, IsHovering: false, active: false, handles: (0, 0), colours: [[0.0; 4]; 3], sprites: (0, 0), lines: (0, 0), visuals: Vec::new() }
    }
}

/// Vector3.Project(vector, onNormal): Mono keeps `onNormal.x * num2 / num` in double, rounded once at the Vector3 ctor.
fn project(v: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let num = dot(n, n);
    if num < mathf::EPSILON {
        return [0.0; 3];
    }
    let k = dot(n, v);
    [(d(n[0]) * k / num).f(), (d(n[1]) * k / num).f(), (d(n[2]) * k / num).f()]
}

/// Vector3.Dot
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (d(a[0]) * b[0] + d(a[1]) * b[1] + d(a[2]) * b[2]).f()
}

impl Game {
    /// ResizeHandles.IsValidTarget (a destroyed behaviour reads as null)
    pub(crate) fn resize_is_valid_target(&self, p: usize) -> bool {
        let ph = &self.physicals[p];
        !ph.destroyed && !ph.isDisintegrated && ph.Resizable
    }

    /// Global.UILock: the pointer over the UI (`WindowInput::ui_lock`), else the scene's ResizeHandles.IsHovering.
    pub fn ui_lock(&self) -> bool {
        if let Some(lock) = self.input.ui_lock {
            return lock;
        }
        self.input.window.is_some_and(|w| w.ui_lock) || self.resize.IsHovering
    }

    /// ResizeHandles.HasTargets
    pub fn resize_has_targets(&self) -> bool {
        self.resize.Targets.iter().filter(|&&p| self.resize_is_valid_target(p)).count() != 0
    }

    /// ResizeHandles.SingleTarget
    pub fn resize_single_target(&self) -> bool {
        self.resize.Targets.iter().filter(|&&p| self.resize_is_valid_target(p)).count() == 1
    }

    /// ResizeHandles.Target
    pub(crate) fn resize_target(&self) -> usize {
        // C# `Targets[0]` on an empty array: IndexOutOfRangeException.
        let p = *self.resize.Targets.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        if self.physicals[p].destroyed {
            crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
        }
        p
    }

    /// ResizeHandles.HandleRadiusMultiplier
    pub(crate) fn resize_handle_radius_multiplier(&self) -> f32 {
        self.camera_orthographic_size() / 5.0
    }

    /// ResizeHandles.ForceUniformScaling
    pub(crate) fn resize_force_uniform_scaling(&self) -> bool {
        if !self.input.held("fast") {
            if self.resize_has_targets() {
                return !self.resize_single_target();
            }
            return false;
        }
        true
    }

    /// ResizeHandles.Awake: camera = Camera.main (the camera is Game's).
    /// ContextMenuBehaviour.ResizeAction's tail, once it chose the targets (`hdm` is its HandleDistanceMultiplier):
    /// Targets, SetActive(true), ResetHandles.
    pub fn resize_handles_set_targets(&mut self, targets: Vec<usize>, hdm: [f32; 2]) {
        self.resize.HandleDistanceMultiplier = hdm;
        self.resize.Targets = targets;
        // SetActive(true) of the inactive handles: MonoBehaviour::AddToManager appends ResizeHandles to the order-0
        // Update list, after the behaviours already there (its scene place is gone once it left the manager).
        if !self.resize.active {
            self.resize.readded = true;
            self.behaviours.add(0, super::Behaviour::ResizeHandles);
        }
        if let Some((mut inst, o)) = self.resize_handles.take() {
            for (d, k) in self.set_active(&mut inst, o, true) {
                let crate::assets::db::Component::Script { class, .. } = &inst.prefab.objects[d].components[k].1 else { unreachable!() };
                if class != "ResizeHandles" {
                    unimplemented!("mod-only: {class}.OnEnable under the resize handles");
                }
            }
            self.resize_sync_visuals(&inst);
            self.resize_handles = Some((inst, o));
        }
        self.resize.active = true;
        self.resize_reset_handles();
    }

    /// ContextMenuBehaviour.ResizeAction's stop branch: Targets = empty.
    pub fn resize_handles_clear_targets(&mut self) {
        self.resize.Targets.clear();
    }

    /// ResizeHandles.ResetHandles
    pub fn resize_reset_handles(&mut self) {
        if self.resize_has_targets() && !self.resize_single_target() {
            self.resize.multiplying_scale = 1.0;
            let targets = self.resize.Targets.clone();
            self.resize.initial_targets = targets
                .iter()
                .map(|&t| {
                    if self.physicals[t].destroyed {
                        crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
                    }
                    let (s, p) = (self.physicals[t].transform.localScale, self.physicals[t].transform.position);
                    MultiTarget::new(s, p)
                })
                .collect();
        }
    }

    /// ResizeHandles.Update
    pub fn resize_handles_update(&mut self) {
        if !self.resize.active {
            return;
        }
        if !self.resize_has_targets() {
            self.resize.IsHovering = false;
            self.resize.active = false;
            // SetActive(false): RemoveFromManager.
            self.behaviours.remove(super::Behaviour::ResizeHandles);
            let Some((mut inst, o)) = self.resize_handles.take() else { return };
            for (d, k) in self.set_active(&mut inst, o, false) {
                let crate::assets::db::Component::Script { class, .. } = &inst.prefab.objects[d].components[k].1 else { unreachable!() };
                if class != "ResizeHandles" {
                    unimplemented!("mod-only: {class}.OnDisable under the resize handles");
                }
            }
            self.resize_sync_visuals(&inst);
            self.resize_handles = Some((inst, o));
            return;
        }
        self.resize_handle_display();
        if !self.resize.is_dragging {
            self.resize_check_for_drag_start();
        } else if self.resize_single_target() {
            self.resize_handle_single_drag();
        } else {
            self.resize_handle_multi_drag();
        }
        if self.input.key(27).down {
            self.resize.Targets.clear();
        }
    }

    /// ResizeHandles.CheckForDragStart
    pub(crate) fn resize_check_for_drag_start(&mut self) {
        self.resize.IsHovering = false;
        let (h, v) = self.resize.handles;
        if self.resize_is_mouse_held_near(h, 0.2) {
            self.resize_start_drag(Axis::X);
        } else if self.resize_is_mouse_held_near(v, 0.2) {
            self.resize_start_drag(Axis::Y);
        }
    }

    /// ResizeHandles.StartResizeDrag
    pub(crate) fn resize_start_drag(&mut self, axis: Axis) {
        self.resize.current_drag_axis = axis;
        self.resize.is_dragging = true;
    }

    /// ResizeHandles.IsMouseHeldNear: `o` is the handle's object in the clone.
    pub(crate) fn resize_is_mouse_held_near(&mut self, o: usize, radius: f32) -> bool {
        let radius = radius * self.resize_handle_radius_multiplier();
        let p = self.resize_handles.as_ref().unwrap().0.position(o);
        let m = self.MousePosition;
        let v = [m[0] - p[0], m[1] - p[1], m[2] - p[2]];
        if dot(v, v) < radius * radius {
            self.resize.IsHovering = true;
            return self.input.key(323).down;
        }
        false
    }

    fn resize_base_position(&self) -> [f32; 3] {
        let (inst, o) = self.resize_handles.as_ref().unwrap();
        inst.position(*o)
    }

    /// ResizeHandles.HandleMultiDrag
    pub(crate) fn resize_handle_multi_drag(&mut self) {
        self.resize_reset_handles();
        if self.input.key(323).up {
            self.resize.IsHovering = false;
            self.resize.is_dragging = false;
            return;
        }
        self.resize.IsHovering = true;
        let num = self.resize.multiplying_scale;
        let hdm = self.resize.HandleDistanceMultiplier;
        let vector = if self.resize.current_drag_axis == Axis::X { [num / hdm[0], 0.0 * num / hdm[0], 0.0] } else { [0.0 * num / hdm[1], num / hdm[1], 0.0] };
        let base = self.resize_base_position();
        let m = self.MousePosition;
        let f = mathf::sqrt(dot(project([m[0] - base[0], m[1] - base[1], m[2] - base[2]], vector), vector));
        if f.is_nan() {
            return;
        }
        self.resize.multiplying_scale = f.abs();
        let s = self.resize.multiplying_scale;
        for i in 0..self.resize.Targets.len() {
            let p = self.resize.Targets[i];
            if self.physicals[p].destroyed {
                continue;
            }
            let t = self.resize.initial_targets[i];
            self.physical_set_local_scale(p, [s * t.scale[0] * if t.flipped { -1.0 } else { 1.0 }, s * t.scale[1], 1.0]);
            let z = (0.0 - base[2]) * s + base[2];
            self.physical_set_position(p, [(t.position[0] - base[0]) * s + base[0], (t.position[1] - base[1]) * s + base[1], z]);
            self.physical_recalculate_mass_based_on_size(p);
        }
    }

    /// ResizeHandles.HandleSingleDrag
    pub(crate) fn resize_handle_single_drag(&mut self) {
        if self.input.key(323).up {
            self.resize.IsHovering = false;
            self.resize.is_dragging = false;
            return;
        }
        self.resize.IsHovering = true;
        let target = self.resize_target();
        let local_scale = self.physicals[target].transform.localScale;
        let hdm = self.resize.HandleDistanceMultiplier;
        let axis = self.resize.current_drag_axis;
        let vector = if axis == Axis::X {
            let r = self.transform_right(target);
            [r[0] * local_scale[0] / hdm[0], r[1] * local_scale[0] / hdm[0], r[2] * local_scale[0] / hdm[0]]
        } else {
            let u = self.transform_up(target);
            [u[0] * local_scale[1] / hdm[1], u[1] * local_scale[1] / hdm[1], u[2] * local_scale[1] / hdm[1]]
        };
        let base = self.resize_base_position();
        let m = self.MousePosition;
        let lhs = project([m[0] - base[0], m[1] - base[1], m[2] - base[2]], vector);
        let force_uniform = self.resize_force_uniform_scaling();
        let mut num = mathf::sqrt(dot(lhs, vector));
        let flag = self.physicals[target].transform.localScale[0] < 0.0;
        if num.is_nan() {
            return;
        }
        num = mathf::max(0.1, num);
        if flag && (force_uniform || axis == Axis::X) {
            num = 0.0 - num.abs();
        }
        if force_uniform {
            self.physical_set_local_scale(target, [num, num.abs(), 1.0]);
        } else {
            match axis {
                Axis::X => self.physical_set_local_scale(target, [num, local_scale[1], 1.0]),
                Axis::Y => self.physical_set_local_scale(target, [local_scale[0], num.abs(), 1.0]),
                _ => {}
            }
        }
self.physical_recalculate_mass_based_on_size(target);
    }

    /// The scene's renderers under "Resizing handles" (a root no link pass sees) into `renderers` and `lines`, and the
    /// ResizeHandles colours and renderer fields.
    pub(super) fn resize_add_visuals(&mut self, db: &crate::assets::db::AssetDb, file: usize, fields: &crate::assets::mono::Value, inst: &super::spawn::Instance) -> Result<(), String> {
        use crate::assets::db::Component;
        for o in 0..inst.objects.len() {
            for (c, (r, comp)) in inst.prefab.objects[o].components.iter().enumerate() {
                let (id, game_layer, visible) = (inst.objects[o].components[c], inst.prefab.objects[o].layer as i32, inst.objects[o].active);
                match comp {
                    Component::SpriteRenderer(sr) => {
                        let sprite = db.resolve(file, sr.sprite).ok_or("a resize handle without a sprite")?;
                        let bounds = super::link::renderer_bounds(&db.sprite(sprite)?, sr);
                        let draw = ([sr.flip_x, sr.flip_y], sr.draw_mode, sr.size, sr.adaptive_mode_threshold, sr.sprite_tile_mode == 1);
                        let material = sr.materials.first().and_then(|&m| db.resolve(file, m));
                        self.resize.visuals.push((o, false, self.renderers.len()));
                        self.renderers.push(super::Renderer { anchor: super::BareAnchor::Root, chain: inst.chain(o), sprite: (sprite, sr.color, sr.sorting_order), layer: sr.sorting_layer_id, game_layer, material, id, draw, visible, enabled: sr.enabled, bounds });
                        // Scene objects awake at load: in the RendererScene before anything spawned.
                        self.renderer_sync(self.renderers.len() - 1);
                    }
                    Component::Other(120) => {
                        let data = crate::assets::components::LineRendererData::read(db.data(*r))?;
                        let line = self.add_line(db, file, data, id, super::BareAnchor::Root, inst.chain(o), game_layer, visible);
                        self.resize.visuals.push((o, true, line));
                    }
                    _ => {}
                }
            }
        }
        let find = |name: &str, line: bool| -> Result<usize, String> {
            let r = fields.get(name).and_then(crate::assets::mono::Value::pptr).and_then(|p| db.resolve(file, p)).ok_or_else(|| format!("ResizeHandles.{name}"))?;
            let o = inst.prefab.index.get(&r).ok_or_else(|| format!("ResizeHandles.{name} outside its object"))?.0;
            self.resize.visuals.iter().find(|v| v.0 == o && v.1 == line).map(|v| v.2).ok_or_else(|| format!("ResizeHandles.{name}: no renderer"))
        };
        self.resize.sprites = (find("HorizontalHandle", false)?, find("VerticalHandle", false)?);
        self.resize.lines = (find("HorizontalLine", true)?, find("VerticalLine", true)?);
        let colour = |name: &str| -> Result<[f32; 4], String> {
            let v = fields.get(name).ok_or_else(|| format!("ResizeHandles.{name}"))?;
            let f = |k: &str| v.get(k).and_then(crate::assets::mono::Value::f32).ok_or_else(|| format!("ResizeHandles.{name}.{k}"));
            Ok([f("r")?, f("g")?, f("b")?, f("a")?])
        };
        self.resize.colours = [colour("HorizontalColour")?, colour("VerticalColour")?, colour("UniformColour")?];
        Ok(())
    }

    /// The renderers follow the clone's Transforms and activeInHierarchy.
    fn resize_sync_visuals(&mut self, inst: &super::spawn::Instance) {
        for &(o, line, i) in &self.resize.visuals {
            let (chain, visible) = (inst.chain(o), inst.objects[o].active);
            if line {
                (self.lines[i].chain, self.lines[i].visible) = (chain, visible);
            } else {
                (self.renderers[i].chain, self.renderers[i].visible) = (chain, visible);
            }
        }
    }

    /// fn-check (biga:handles): what HandleDisplay writes, in BigProbeA.HState's order: the object's position and rotation,
    /// HandleDistanceMultiplier, each handle's localPosition and localScale, each line's position 1 and width, the handles'
    /// colours and the lines' gradients at 0.
    pub(crate) fn resize_handle_state(&self) -> Vec<f32> {
        let (inst, o) = self.resize_handles.as_ref().unwrap();
        let mut v = inst.position(*o).to_vec();
        v.extend(inst.rotation(*o));
        v.extend(self.resize.HandleDistanceMultiplier);
        for h in [self.resize.handles.0, self.resize.handles.1] {
            let t = &inst.objects[h].trs;
            v.extend(&t.t[..3]);
            v.extend(&t.s[..3]);
        }
        for l in [self.resize.lines.0, self.resize.lines.1] {
            v.extend(self.lines[l].positions[1]);
            v.push(self.lines[l].width);
        }
        v.extend(self.renderers[self.resize.sprites.0].sprite.1);
        v.extend(self.renderers[self.resize.sprites.1].sprite.1);
        v.extend(self.lines[self.resize.lines.0].gradient.evaluate(0.0));
        v.extend(self.lines[self.resize.lines.1].gradient.evaluate(0.0));
        v
    }

    /// ResizeHandles.HandleDisplay
    pub(crate) fn resize_handle_display(&mut self) {
        if !self.resize_has_targets() {
            return;
        }
        let mut rotation = [0.0, 0.0, 0.0, 1.0];
        let (position, scale);
        if self.resize_single_target() {
            let t = self.resize_target();
            let tr = &self.physicals[t].transform;
            position = [tr.position[0], tr.position[1]];
            rotation = tr.rotation;
            scale = [tr.localScale[0], tr.localScale[1]];
        } else {
            let (c, e) = self.resize_selection_rect();
            position = [c[0], c[1]];
            scale = [self.resize.multiplying_scale; 2];
            self.resize.HandleDistanceMultiplier = [e[0], e[1]];
        }
        let hdm = self.resize.HandleDistanceMultiplier;
        let (mut inst, o) = self.resize_handles.take().unwrap();
        inst.set_world_position(o, [position[0], position[1], 0.0]);
        inst.set_world_rotation(o, rotation);
        let (h, v) = self.resize.handles;
        let (hl, vl) = self.resize.lines;
        self.resize_update_display_axis(&mut inst, h, hl, [hdm[0] * scale[0], hdm[0] * scale[0] * 0.0, 0.0]);
        self.resize_update_display_axis(&mut inst, v, vl, [hdm[1] * scale[1] * 0.0, hdm[1] * scale[1], 0.0]);
        let [hc, vc, uc] = self.resize.colours;
        let (color, color2) = if self.resize_force_uniform_scaling() { (uc, uc) } else { (hc, vc) };
        let (hs, vs) = self.resize.sprites;
        self.renderers[hs].sprite.1 = color;
        self.renderers[vs].sprite.1 = color2;
        // new Gradient { alphaKeys = { (1, 0) }, colorKeys = { (color, 0) } }
        let gradient = |c: [f32; 4]| {
            let mut keys = [[0.0; 4]; 8];
            keys[0] = [c[0], c[1], c[2], 1.0];
            super::visual::Gradient::from_raw(keys, [0; 8], [0; 8], 0, 1, 1)
        };
        self.lines[hl].gradient = gradient(color);
        self.lines[vl].gradient = gradient(color2);
        self.resize_sync_visuals(&inst);
        self.resize_handles = Some((inst, o));
    }

    /// ResizeHandles.UpdateDisplayAxis
    fn resize_update_display_axis(&mut self, inst: &mut super::spawn::Instance, handle: usize, line: usize, position: [f32; 3]) {
        let hrm = self.resize_handle_radius_multiplier();
        let s = 0.2 * hrm;
        inst.set_local_scale(handle, [s * 1.0, s * 1.0, 0.0]);
        inst.set_local_position(handle, position);
        let t = inst.objects[handle].trs.t;
        self.lines[line].positions[1] = [t[0], t[1], t[2]];
        self.lines[line].width = hrm * 2.0;
    }

    /// ResizeHandles.GetSelectionRect: the Bounds' (center, extents).
    pub(crate) fn resize_selection_rect(&self) -> ([f32; 3], [f32; 3]) {
        let (mut num, mut num2, mut num3, mut num4) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &t in &self.resize.Targets {
            if self.physicals[t].destroyed {
                crate::game::menu::throw("MissingReferenceException: The object of type 'HingeJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
            }
            let (c, e) = self.sprite_world_bounds(t);
            let (max, min) = ([c[0] + e[0], c[1] + e[1]], [c[0] - e[0], c[1] - e[1]]);
            if max[0] > num3 {
                num3 = max[0];
            }
            if max[1] > num4 {
                num4 = max[1];
            }
            if min[0] < num {
                num = min[0];
            }
            if min[1] < num2 {
                num2 = min[1];
            }
        }
        let size = [(num - num3).abs(), (num2 - num4).abs(), 0.0];
        ([num + size[0] / 2.0, num2 + size[1] / 2.0, 0.0], [size[0] * 0.5, size[1] * 0.5, 0.0])
    }
}
