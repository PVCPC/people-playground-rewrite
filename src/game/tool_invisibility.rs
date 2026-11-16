//! ToolInvisibilityTracker: a wire's LineRenderer or a pin's SpriteRenderer shows only while ShouldBeVisible, and
//! swaps to Global.main.DeleteWireMaterial on the ScreenUI layer while the pointer is over a deletable one.
#![allow(non_snake_case)]

use super::Game;

/// ToolInvisibilityTracker.static setup: ScreenUiLayer = LayerMask.NameToLayer("ScreenUI") (TagManager.asset), the layer
/// `delete_look` draws on.
pub(crate) const SCREEN_UI_LAYER: i32 = 16;

/// One tracker's renderer state. `enabled` is Renderer.enabled; `delete_look` is the material and layer swap
/// (sharedMaterial = DeleteWireMaterial, layer = ScreenUiLayer, until OnMouseExit restores initialMaterial and
/// initialLayer).
#[derive(Clone, Copy, Debug, Default)]
pub struct InvisibilityTracker {
    pub enabled: bool,
    pub wasMouseOverlapping: bool,
    pub delete_look: bool,
}

/// The Hover a tracker belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Owner {
    Wire(usize),
    Pin(usize),
}

impl Game {
    fn tracker(&mut self, o: Owner) -> &mut InvisibilityTracker {
        match o {
            Owner::Wire(w) => &mut self.blood_wires[w].tracker,
            Owner::Pin(p) => &mut self.pins[p].tracker,
        }
    }

    /// `(bool)owner`: a destroyed Hover reads null.
    fn tracker_owner_alive(&self, o: Owner) -> bool {
        let id = match o {
            Owner::Wire(w) => self.blood_wires[w].id,
            Owner::Pin(p) => self.pins[p].id,
        };
        !self.destroyed_components.contains(&id)
    }

    /// owner.IsMouseOverlapping
    fn tracker_owner_overlapping(&self, o: Owner) -> bool {
        match o {
            Owner::Wire(w) => self.blood_wires[w].IsMouseOverlapping,
            Owner::Pin(p) => self.pins[p].IsMouseOverlapping,
        }
    }

    /// ToolInvisibilityTracker.get_ShouldBeVisible
    pub(crate) fn tracker_should_be_visible(&self, o: Owner) -> bool {
        let invisible = match o {
            Owner::Wire(w) => self.blood_wires[w].Invisible,
            Owner::Pin(p) => self.pins[p].Invisible,
        };
        // ToolWheelBehaviour.ToolVisibility starts Visible: a loaded snapshot without the ToolControllerBehaviour record has its default.
        if !self.tracker_owner_alive(o) || invisible { self.tool.as_ref().is_some_and(|t| t.invisible) } else { true }
    }

    /// ToolInvisibilityTracker.constructor: initialLayer and initialMaterial are the renderer's own (`delete_look`
    /// off), the OnToolVisibilityChanged, OnMouseEnter and OnMouseExit subscriptions (`trackers`), then UpdateVisibility.
    pub(super) fn tracker_new(&mut self, o: Owner) {
        *self.tracker(o) = InvisibilityTracker::default();
        self.tool_wheel_add_on_tool_visibility_changed(o);
        self.tracker_update_visibility(o);
    }

    /// ToolInvisibilityTracker.OnMouseEnter (`delete_look` is drawn by delete_wire.rs).
    pub(crate) fn tracker_on_mouse_enter(&mut self, o: Owner) {
        let can_be_deleted = match o {
            Owner::Wire(w) => self.tool_should_be_visible(self.blood_wires[w].Invisible),
            Owner::Pin(p) => self.pin_can_be_deleted(p),
        };
        if self.prefs.ShowOutlines && can_be_deleted {
            let t = self.tracker(o);
            t.wasMouseOverlapping = true;
            t.delete_look = true;
            self.tracker_update_visibility(o);
        }
    }

    /// ToolInvisibilityTracker.OnMouseExit (`delete_look` is drawn by delete_wire.rs).
    pub(crate) fn tracker_on_mouse_exit(&mut self, o: Owner) {
        if self.tracker(o).wasMouseOverlapping {
            self.tracker_update_visibility(o);
            self.tracker(o).delete_look = false;
        }
        self.tracker(o).wasMouseOverlapping = false;
    }

    /// ToolInvisibilityTracker.OnToolVisibilityChanged
    pub(crate) fn tracker_on_tool_visibility_changed(&mut self, o: Owner) {
        let alive = self.tracker_owner_alive(o);
        if alive && self.tracker_should_be_visible(o) && self.tracker_owner_overlapping(o) {
            self.tracker_on_mouse_enter(o);
        } else if alive && !self.tracker_should_be_visible(o) && self.tracker(o).wasMouseOverlapping && !self.tracker_owner_overlapping(o) {
            self.tracker_on_mouse_exit(o);
        } else {
            self.tracker_update_visibility(o);
        }
        self.tracker(o).wasMouseOverlapping = self.tracker_owner_overlapping(o);
    }

    /// ToolInvisibilityTracker.UpdateVisibility: renderer.enabled = ShouldBeVisible when `(bool)renderer` (the wire's
    /// line and the pin's sprite go with their behaviour).
    pub(crate) fn tracker_update_visibility(&mut self, o: Owner) {
        if !self.tracker_owner_alive(o) {
            return;
        }
        let v = self.tracker_should_be_visible(o);
        self.tracker(o).enabled = v;
        if let Owner::Pin(p) = o {
            if let Some(r) = self.pins[p].renderer {
                self.renderers[r].enabled = v;
                self.renderer_sync(r);
            }
        }
    }

    /// ToolInvisibilityTracker.Dispose (WireBehaviour.OnDestroy, PinBehaviour.OnDestroy; SerpentineBeltBehaviour's belts have
    /// no tracker here: their line is not drawn): the OnToolVisibilityChanged handler goes. The OnMouseEnter/OnMouseExit
    /// subscriptions are the owner's own events, which a destroyed owner never raises again.
    pub(crate) fn tracker_dispose(&mut self, o: Owner) {
        self.tool_wheel_remove_on_tool_visibility_changed(o);
    }

    /// ToolWheelBehaviour.add_OnToolVisibilityChanged (Delegate.Combine): the tracker's handler at the end of the list.
    pub(crate) fn tool_wheel_add_on_tool_visibility_changed(&mut self, o: Owner) {
        self.trackers.push(o);
    }

    /// ToolWheelBehaviour.remove_OnToolVisibilityChanged (Delegate.Remove): the last equal handler (same tracker) goes; none, no change.
    pub(crate) fn tool_wheel_remove_on_tool_visibility_changed(&mut self, o: Owner) {
        if let Some(i) = self.trackers.iter().rposition(|&x| x == o) {
            self.trackers.remove(i);
        }
    }

    /// ToolWheelBehaviour.OnToolVisibilityChanged?.Invoke(ToolVisibility): every tracker in subscription order.
    pub(crate) fn trackers_on_tool_visibility_changed(&mut self) {
        for i in 0..self.trackers.len() {
            let o = self.trackers[i];
            self.tracker_on_tool_visibility_changed(o);
        }
    }
}
