//! LayerEditorController (the layer editing popup and its hotkeys) with the SerialisedLayerBehaviour it adds to the
//! objects it arranges and the SortingLayers lookups. A physical object's sorting is its SpriteRenderer's sortingLayerID
//! and sortingOrder (`sprite_layer`, `sprite.2`); a SortingGroup is not modelled, so GetSortingFor never reads one.
use super::Game;
use crate::play::SORTING_LAYERS;
use std::collections::HashMap;

/// OptionalInt..ctor(value): Active true and Value (an OptionalInt is an Option here; default is None).
fn optional_int(value: i32) -> Option<i32> {
    Some(value)
}

/// SerialisedLayerBehaviour's serialised members (OptionalInt as Option) and OriginalSorting.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SerialisedLayer {
    pub SortingLayerID: Option<i32>,
    pub SortingOrder: Option<i32>,
    pub OriginalSorting: (i32, i32),
}

/// LayerEditorController's fields: Container (active and screen position), the two displays' text, Targets.
#[derive(Clone, Debug, Default)]
pub struct LayerEditor {
    pub container_active: bool,
    pub container_position: [f32; 3],
    pub OrderDisplay: String,
    pub LayerDisplay: String,
    pub Targets: Vec<usize>,
    /// The component's instance id (get_Instance; fn-check loads it from the recording).
    pub id: i32,
    /// The SerialisedLayerBehaviours by physical object.
    pub serialised: HashMap<usize, SerialisedLayer>,
    /// The physical object of each SerialisedLayerBehaviour on a clone (by component id), for ObjectState's pack.
    pub serialised_ids: HashMap<i32, usize>,
}

/// SerialisedLayerBehaviour.NearbySearchRadius
pub(crate) const NEARBY_SEARCH_RADIUS: f32 = 10.0;

/// Default's index in TagManager's order: SortingLayer values count from it (fnb1_gen: GetLayerAbove(-1) is Default,
/// GetLayerAbove(0) the layer after it, GetLayerAbove(10) no layer).
const DEFAULT_INDEX: i32 = 2;

/// SortingLayer.GetLayerValueFromName: the named layer's index in TagManager's order, relative to Default's (0: no such layer).
const fn layer_value_from_name(name: &str) -> i32 {
    let names = crate::play::SORTING_LAYER_NAMES;
    let mut i = 0;
    while i < names.len() {
        let (a, b) = (names[i].as_bytes(), name.as_bytes());
        if a.len() == b.len() {
            let mut k = 0;
            while k < a.len() && a[k] == b[k] {
                k += 1;
            }
            if k == a.len() {
                return i as i32 - DEFAULT_INDEX;
            }
        }
        i += 1;
    }
    0
}

/// UnityEngine.SortingLayer: a layer is its TagManager uniqueID (SortingLayers.All holds one per SORTING_LAYERS entry).
#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct SortingLayer {
    pub m_Id: i32,
}

/// SortingLayers.static setup: Bottom, Background, Default, Foreground and Top, each GetLayerValueFromName of its name.
pub(crate) const SORTING_LAYER_BOTTOM: i32 = layer_value_from_name("Bottom");
pub(crate) const SORTING_LAYER_BACKGROUND: i32 = layer_value_from_name("Background");
pub(crate) const SORTING_LAYER_DEFAULT: i32 = layer_value_from_name("Default");
pub(crate) const SORTING_LAYER_FOREGROUND: i32 = layer_value_from_name("Foreground");
pub(crate) const SORTING_LAYER_TOP: i32 = layer_value_from_name("Top");

/// SortingLayer.GetLayerValueFromID: the layer's index in TagManager's order, relative to Default's.
fn layer_value(id: i32) -> i32 {
    crate::play::sorting_layer_index(id) as i32 - DEFAULT_INDEX
}

/// SortingLayers.GetLayerAbove (`above`) / GetLayerUnder over SortedByDepth: (true, the next layer's id); at the end
/// (false, that end layer's id); a value of no layer (false, 0: default(SortingLayer)).
pub(crate) fn layer_neighbour(value: i32, above: bool) -> (bool, i32) {
    let i = value + DEFAULT_INDEX;
    if i < 0 || i >= SORTING_LAYERS.len() as i32 {
        return (false, 0);
    }
    let j = if above { i + 1 } else { i - 1 };
    match SORTING_LAYERS.get(j as usize).filter(|_| j >= 0) {
        Some(&u) => (true, u as i32),
        None => (false, SORTING_LAYERS[i as usize] as i32),
    }
}

/// SortingLayers.GetLayerAbove: the id of the next layer up, None for the top layer or no layer.
pub(crate) fn layer_above(value: i32) -> Option<i32> {
    let (ok, id) = layer_neighbour(value, true);
    ok.then_some(id)
}

/// SortingLayers.GetLayerUnder
pub(crate) fn layer_under(value: i32) -> Option<i32> {
    let (ok, id) = layer_neighbour(value, false);
    ok.then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SortingLayers.GetLayerAbove / GetLayerUnder stop at the ends of TagManager's order.
    #[test]
    fn layer_neighbours() {
        assert_eq!(layer_under(-2), None);
        assert_eq!(layer_above(5), None);
        assert_eq!(layer_above(10), None);
        assert_eq!(layer_above(-1), Some(SORTING_LAYERS[2] as i32));
        assert_eq!(layer_under(-1), Some(SORTING_LAYERS[0] as i32));
        assert_eq!(layer_neighbour(5, true), (false, SORTING_LAYERS[7] as i32));
    }
}

impl Game {
    /// A live object: Unity's `(bool)` on a PhysicalBehaviour.
    fn layer_alive(&self, p: usize) -> bool {
        !self.physicals[p].destroyed
    }

    /// SerialisedLayerBehaviour.GetSortingFor
    pub fn serialised_layer_get_sorting_for(&self, p: usize) -> (i32, i32) {
        match self.physicals[p].sprite {
            Some((_, _, order)) => (self.physicals[p].sprite_layer, order as i32),
            // A renderer drawn outside `sprite` (a human limb's): its sorting as loaded; (0, 0) without one.
            None => (self.physicals[p].sprite_layer, self.physicals[p].bare_order as i32),
        }
    }

    /// SerialisedLayerBehaviour.SetSortingFor
    pub fn serialised_layer_set_sorting_for(&mut self, p: usize, layer: i32, order: i32) {
        // Renderer::SetSortingLayerID: an id not in TagManager's sortingLayers logs "Invalid layer id" and keeps the layer.
        let valid = SORTING_LAYERS.contains(&(layer as u32));
        // Renderer_Set_Custom_PropSortingOrder passes `movzx edx, di` to Renderer::SetSortingOrder(short): the int wraps to 16 bits, no range check.
        let order = order as i16;
        if let Some((s, c, _)) = self.physicals[p].sprite {
            if valid {
                self.physicals[p].sprite_layer = layer;
            }
            self.physicals[p].sprite = Some((s, c, order));
        } else if self.limbs.iter().any(|x| x.PhysicalBehaviour == p) {
            // A human limb's spriteRenderer, drawn with the skin.
            if valid {
                self.physicals[p].sprite_layer = layer;
            }
            self.physicals[p].bare_order = order;
        }
    }

    /// SerialisedLayerBehaviour.Start
    pub(crate) fn serialised_layer_start(&mut self, p: usize) {
        let Some(&s) = self.layer_editor.serialised.get(&p) else { return };
        if !self.layer_alive(p) {
            return;
        }
        self.serialised_layer_set_sorting_for(p, s.SortingLayerID.unwrap_or(s.OriginalSorting.0), s.SortingOrder.unwrap_or(s.OriginalSorting.1));
    }

    /// GetOrAddComponent<SerialisedLayerBehaviour>: SerialisedLayerBehaviour.Awake stores OriginalSorting; SerialisedLayerBehaviour.Start sets the sorting back to it (both OptionalInts inactive), a no-op.
    pub(crate) fn serialised_layer_get_or_add(&mut self, p: usize) {
        if !self.layer_editor.serialised.contains_key(&p) {
            let id = self.allocate_instance_id();
            self.attach_script(self.physicals[p].id, "SerialisedLayerBehaviour", id, true);
            self.layer_editor.serialised_ids.insert(id, p);
            self.serialised_layer_awake(p);
        }
    }

    /// SerialisedLayerBehaviour.Awake: OriginalSorting = GetSortingFor(phys), on an existing component too.
    pub(crate) fn serialised_layer_awake(&mut self, p: usize) {
        let original = self.serialised_layer_get_sorting_for(p);
        self.layer_editor.serialised.entry(p).or_default().OriginalSorting = original;
    }

    /// SerialisedLayerBehaviour.SetOrder(layerId, order)
    pub fn serialised_layer_set_order(&mut self, p: usize, layer: i32, order: i32) {
        let s = self.layer_editor.serialised.get_mut(&p).unwrap();
        s.SortingLayerID = optional_int(layer);
        s.SortingOrder = optional_int(order);
        self.serialised_layer_set_sorting_for(p, layer, order);
    }

    /// SerialisedLayerBehaviour.FindTopmostAround
    pub fn serialised_layer_find_topmost_around(&mut self, point: [f32; 2], layer: i32) -> Option<(usize, i32)> {
        let mut num = i32::MIN;
        let mut result = None;
        for item in self.physics_objects_near_position_accurate(point, NEARBY_SEARCH_RADIUS) {
            if self.layer_alive(item) {
                let (id, order) = self.serialised_layer_get_sorting_for(item);
                if id == layer && order > num {
                    result = Some((item, order));
                    num = order;
                }
            }
        }
        result
    }

    /// SerialisedLayerBehaviour.FindBottommostAround
    pub fn serialised_layer_find_bottommost_around(&mut self, point: [f32; 2], layer: i32) -> Option<(usize, i32)> {
        let mut num = i32::MAX;
        let mut result = None;
        for item in self.physics_objects_near_position_accurate(point, NEARBY_SEARCH_RADIUS) {
            if self.layer_alive(item) {
                let (id, order) = self.serialised_layer_get_sorting_for(item);
                if id == layer && order < num {
                    result = Some((item, order));
                    num = order;
                }
            }
        }
        result
    }

    /// SerialisedLayerBehaviour.MoveLayer
    pub fn serialised_layer_move_layer(&mut self, p: usize, delta: i32) {
        let (layer, order) = self.serialised_layer_get_sorting_for(p);
        let at = self.physicals[p].transform.position;
        let at = [at[0], at[1]];
        if delta > 0 {
            if self.serialised_layer_find_topmost_around(at, layer).is_some_and(|(r, _)| r == p) {
                if let Some(above) = layer_above(layer_value(layer)) {
                    match self.serialised_layer_find_bottommost_around(at, above) {
                        Some((_, o)) => self.serialised_layer_set_order(p, above, o + 1),
                        None => self.serialised_layer_set_order(p, above, 0),
                    }
                    return;
                }
            }
        } else if delta < 0 && self.serialised_layer_find_bottommost_around(at, layer).is_some_and(|(r, _)| r == p) {
            if let Some(under) = layer_under(layer_value(layer)) {
                match self.serialised_layer_find_topmost_around(at, under) {
                    Some((_, o)) => self.serialised_layer_set_order(p, under, o - 1),
                    None => self.serialised_layer_set_order(p, under, 0),
                }
                return;
            }
        }
        self.serialised_layer_set_order(p, layer, order + delta);
    }

    /// SerialisedLayerBehaviour.SendToBack
    pub fn serialised_layer_send_to_back(&mut self, p: usize) {
        self.serialised_layer_front_or_back(p, false);
    }

    /// SerialisedLayerBehaviour.BringToFront
    pub fn serialised_layer_bring_to_front(&mut self, p: usize) {
        self.serialised_layer_front_or_back(p, true);
    }

    /// SendToBack and BringToFront share their shape: the topmost (or bottommost) layer value and order among the
    /// nearby objects of other roots, then one past it.
    fn serialised_layer_front_or_back(&mut self, p: usize, front: bool) {
        let (mut num, order) = self.serialised_layer_get_sorting_for(p);
        let mut num2 = layer_value(num);
        let mut num3 = order;
        let root = self.root_of(p);
        let at = self.physicals[p].transform.position;
        for item in self.physics_objects_near_position_accurate([at[0], at[1]], NEARBY_SEARCH_RADIUS) {
            if self.layer_alive(item) && self.root_of(item) != root {
                let (id, order2) = self.serialised_layer_get_sorting_for(item);
                let value = layer_value(id);
                if if front { value > num2 } else { value < num2 } {
                    num2 = value;
                    num = id;
                }
                if num == id {
                    num3 = if front { num3.max(order2) } else { num3.min(order2) };
                }
            }
        }
        self.serialised_layer_set_order(p, num, if front { num3 + 1 } else { num3 - 1 });
    }

    /// Camera.WorldToScreenPoint of the main camera's view.
    pub(super) fn world_to_screen_point(&self, p: [f32; 3]) -> [f32; 3] {
        // CameraProject: w = row 3 . (p, 1) is NaN for a NaN or infinite coordinate (0 * inf), and a w failing
        // |w| > 1e-7 gives zero (GetTargetPos with no target: 0/0 and -inf, fnb2a 0,0).
        if !p.iter().all(|v| v.is_finite()) {
            return [0.0; 3];
        }
        let s = self.input.screen();
        let Some([cx, cy, size, pw, ph]) = s.view else {
            return [(p[0] - s.origin[0]) / s.scale, (p[1] - s.origin[1]) / s.scale, 0.0];
        };
        // batch2 r4: Unity's own arithmetic (the linear map was 1 ulp off, fnb2g): worldToClip = Ortho(-size*aspect,
        // size*aspect, -size, size) * worldToCamera (identity rotation, z flipped), clip = M . (p, 1), then
        // viewport.x + (1 + clip.x / w) * width * 0.5 with w = 1.
        let hw = size * (pw / ph);
        let (m00, m11) = (2.0 / (hw - -hw), 2.0 / (size - -size));
        let (m03, m13) = (m00 * -cx, m11 * -cy);
        let x = m00 * p[0] + 0.0 * p[1] + 0.0 * p[2] + m03;
        let y = 0.0 * p[0] + m11 * p[1] + 0.0 * p[2] + m13;
        [(1.0 + x) * pw * 0.5, (1.0 + y) * ph * 0.5, 0.0]
    }

    // LayerEditorController.Awake: Instance = this; Container.SetActive(false) is `container_active` starting false
    /// LayerEditorController.get_IsOpen
    pub fn layer_editor_is_open(&self) -> bool {
        self.layer_editor.container_active
    }

    /// LayerEditorController.GetRelevantTargets
    pub(crate) fn layer_editor_get_relevant_targets(&self) -> Vec<usize> {
        if !self.layer_editor_is_open() {
            self.tool.as_ref().map_or_else(Vec::new, |t| t.rawSelection.clone())
        } else {
            self.layer_editor.Targets.clone()
        }
    }

    /// LayerEditorController.GetTargetPos
    pub(crate) fn layer_editor_get_target_pos(&self) -> [f32; 3] {
        let mut num = 0;
        let mut num2 = 0.0f32;
        let mut num3 = f32::NEG_INFINITY;
        for t in self.layer_editor_get_relevant_targets() {
            if self.layer_alive(t) {
                num += 1;
                let position = self.physicals[t].transform.position;
                num2 += position[0];
                num3 = super::mathf::max(num3, position[1]);
            }
        }
        let s = self.world_to_screen_point([num2 / num as f32, num3, 0.0]);
        [s[0], s[1], 0.0]
    }

    /// LayerEditorController.UpdateDisplays
    pub fn layer_editor_update_displays(&mut self) {
        let mut num: Option<i32> = None;
        let mut num2: Option<i32> = None;
        let mut flag = false;
        let mut flag2 = false;
        for target in self.layer_editor.Targets.clone() {
            if self.layer_alive(target) && self.physicals[target].sprite.is_some() {
                self.serialised_layer_get_or_add(target);
                let (num3, num4) = self.serialised_layer_get_sorting_for(target);
                match num {
                    None => num = Some(num3),
                    Some(n) if n != num3 => flag = true,
                    _ => {}
                }
                match num2 {
                    None => num2 = Some(num4),
                    Some(n) if n != num4 => flag2 = true,
                    _ => {}
                }
            }
        }
        self.layer_editor.LayerDisplay = match num {
            Some(n) if !flag => n.to_string(),
            _ => "--".into(),
        };
        self.layer_editor.OrderDisplay = match num2 {
            Some(n) if !flag2 => n.to_string(),
            _ => "--".into(),
        };
    }

    /// `Container.transform.position = p` then reading it back: Transform.SetPosition stores the local position
    /// (p - parent position) * (1 / parent scale) through the Canvas (overlay: at the screen centre, CanvasScaler's
    /// ScaleWithScreenSize scale, reference 2688x1080, match width) and the zero-offset unit-scale UI parents, and
    /// position is local * scale + parent position again: 1 ulp off p far from the centre (batch2 r5, fnb2g Show).
    fn layer_editor_container_round_trip(&self, p: [f32; 3]) -> [f32; 3] {
        let Some([_, _, _, pw, ph]) = self.input.screen().view else { return p };
        // Mathf.Pow(2, Mathf.Lerp(Mathf.Log(w / 2688, 2), .., 0)), each in double and cast.
        let s = 2f64.powf((((pw / 2688.0) as f64).ln() / 2f64.ln()) as f32 as f64) as f32;
        let c = [pw * 0.5, ph * 0.5, 0.0];
        let inv = 1.0 / s;
        std::array::from_fn(|i| (p[i] - c[i]) * inv * s + c[i])
    }

    /// LayerEditorController.Show
    pub fn layer_editor_show(&mut self) {
        self.layer_editor.container_position = self.layer_editor_container_round_trip(self.layer_editor_get_target_pos());
        self.layer_editor.container_active = self.layer_editor.Targets.iter().any(|&t| self.layer_alive(t));
        if self.layer_editor.container_active {
            self.layer_editor_update_displays();
        }
    }

    /// LayerEditorController.Hide
    pub fn layer_editor_hide(&mut self) {
        self.layer_editor.container_active = false;
        self.layer_editor.Targets.clear();
    }

    /// The targets with a sprite renderer, alive: what each arrange method loops over.
    fn layer_editor_arrangeable(&self) -> Vec<usize> {
        self.layer_editor_get_relevant_targets().into_iter().filter(|&t| self.layer_alive(t) && self.physicals[t].sprite.is_some()).collect()
    }

    /// LayerEditorController.RawOffsetLayer
    pub fn layer_editor_raw_offset_layer(&mut self, sign: bool) {
        for t in self.layer_editor_arrangeable() {
            self.serialised_layer_get_or_add(t);
            let (layer, order) = self.serialised_layer_get_sorting_for(t);
            let next = if sign { layer_above(layer_value(layer)) } else { layer_under(layer_value(layer)) };
            if let Some(id) = next {
                self.serialised_layer_set_order(t, id, order);
            }
        }
        self.layer_editor_update_displays();
    }

    /// LayerEditorController.RawOffsetOrder
    pub fn layer_editor_raw_offset_order(&mut self, sign: bool) {
        for t in self.layer_editor_arrangeable() {
            self.serialised_layer_get_or_add(t);
            let (layer, order) = self.serialised_layer_get_sorting_for(t);
            self.serialised_layer_set_order(t, layer, order + if sign { 1 } else { -1 });
        }
        self.layer_editor_update_displays();
    }

    /// LayerEditorController.MoveLayer
    pub fn layer_editor_move_layer(&mut self, delta: i32) {
        if delta != 0 {
            for t in self.layer_editor_arrangeable() {
                self.serialised_layer_get_or_add(t);
                self.serialised_layer_move_layer(t, delta);
            }
        }
        self.layer_editor_update_displays();
    }

    /// LayerEditorController.BringToFront
    pub fn layer_editor_bring_to_front(&mut self) {
        for t in self.layer_editor_arrangeable() {
            self.serialised_layer_get_or_add(t);
            self.serialised_layer_bring_to_front(t);
        }
        self.layer_editor_update_displays();
    }

    /// LayerEditorController.SendToBack
    pub fn layer_editor_send_to_back(&mut self) {
        for t in self.layer_editor_arrangeable() {
            self.serialised_layer_get_or_add(t);
            self.serialised_layer_send_to_back(t);
        }
        self.layer_editor_update_displays();
    }

    /// LayerEditorController.Update
    pub fn layer_editor_update(&mut self) {
        if self.input.ui_block {
            return;
        }
        if self.input.down("arrangeSelection") {
            if self.layer_editor_is_open() {
                self.layer_editor_hide();
            } else {
                self.layer_editor.Targets.clear();
                self.layer_editor.Targets.extend(self.tool.as_ref().map_or_else(Vec::new, |t| t.rawSelection.clone()));
                self.layer_editor_show();
            }
        }
        if self.input.down("arrangeFront") {
            self.layer_editor_bring_to_front();
        }
        if self.input.down("arrangeBack") {
            self.layer_editor_send_to_back();
        }
        if self.input.down("arrangeDown") {
            self.layer_editor_move_layer(-1);
        }
        if self.input.down("arrangeUp") {
            self.layer_editor_move_layer(1);
        }
        if !self.layer_editor_is_open() {
            return;
        }
        if self.input.key(27).down {
            self.layer_editor_hide();
        }
        if !self.layer_editor.Targets.is_empty() {
            if self.layer_editor.Targets.iter().all(|&t| !self.layer_alive(t)) {
                self.layer_editor.container_active = false;
            } else {
                self.layer_editor.container_position = self.layer_editor_container_round_trip(self.layer_editor_get_target_pos());
            }
        } else {
            self.layer_editor_hide();
        }
    }
}
