//! ContextMenuBehaviour's static buttons and the conditions that make them interactable
//! (ContextMenuInteractableConditionalBehaviour), and Hover.CurrentlyHovering, which turns the menu's Delete onto the
//! wire under the pointer. The dynamic buttons are menu.rs. Save (contraptions),
//! and Edit layer (LayerEditorController) are not rewritten; Resize (ResizeHandles) is.
#![allow(non_snake_case)]

use super::{mathf, Game};
use crate::assets::db::Component;

/// The tag "RecursiveLayerChild" (TagManager's sixth tag).
const RECURSIVE_LAYER_CHILD_TAG: u16 = 20005;

/// A LayerSerialisationBehaviour: its component id and serialised Layer.
#[derive(Clone, Copy, Debug)]
pub struct LayerSerialisation {
    pub id: i32,
    pub Layer: i32,
}

impl Default for LayerSerialisation {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Layer: -1,
        }
    }
}

// Hover.OnMouseEnter: its one subscriber, ToolInvisibilityTracker.OnMouseEnter, is presentation (add_ and remove_ included).
// Hover.OnMouseExit: its one subscriber, ToolInvisibilityTracker.OnMouseExit, is presentation (add_ and remove_ included).
// Hover.OnMouseOverlapEvent: empty (the base); the overrides are LinkDeviceBehaviour's (presentation) and VisualDeletableDetachedToolBehaviour's.

impl Game {
    /// Hover.CheckMouseInput's CurrentlyHovering: cleared when the pointer leaves this one's bounds, set while it is over
    /// the collider and UserPreferences.DeleteWireByContextMenu is on.
    pub(super) fn hover_track(&mut self, id: i32, inside_bounds: bool, inside_collider: bool) {
        if !inside_bounds {
            if self.hovering == Some(id) {
                self.hovering = None;
            }
        } else if inside_collider && self.prefs.DeleteWireByContextMenu {
            self.hovering = Some(id);
        }
    }

    /// `(bool)Hover.CurrentlyHovering`: Hover.OnDestroy clears it, so a destroyed one reads null.
    pub(crate) fn hovering_alive(&self) -> Option<i32> {
        self.hovering.filter(|id| !self.destroyed_components.contains(id))
    }

    /// Hover.CanBeDeleted: a wire's (WireBehaviour.CanBeDeleted), pin's or belt's (SerpentineBeltBehaviour.CanBeDeleted) is InvisibilityTracker.ShouldBeVisible (not Invisible), a link
    /// device's Global.main.ShowLimbStatus.
    pub(crate) fn hover_can_be_deleted(&self, id: i32) -> bool {
        if let Some(w) = self.blood_wires.iter().find(|w| w.id == id) {
            return self.tool_should_be_visible(w.Invisible);
        }
        if self.root_wires.iter().any(|w| w.id == id) {
            // WireBehaviour.CanBeDeleted: InvisibilityTracker is null (Initialise threw before making it).
            super::menu::npe();
        }
        if self.pins.iter().any(|p| p.id == id) {
            return self.pin_can_be_deleted(self.pins.iter().position(|q| q.id == id).unwrap());
        }
        if let Some(b) = self.belts.iter().find(|b| b.id == id) {
            return self.tool_should_be_visible(b.Invisible);
        }
        if self.phase_links.iter().any(|l| l.ends.contains(&id)) {
            // VisualDeletableDetachedToolBehaviour: the base's true.
            return true;
        }
        if self.phase_links.iter().any(|l| l.id == id) {
            // LimbStatusToggle.get_Visible: Global.main.ShowLimbStatus
            return self.ShowLimbStatus;
        }
        unimplemented!("mod-only: Hover.CanBeDeleted of component {id}")
    }

    /// ContextMenuBehaviour.Show's priorSelectedToolBehaviour.
    pub(crate) fn context_menu_show(&mut self) {
        self.context_prior = match self.hovering_alive() {
            Some(h) if self.prefs.DeleteWireByContextMenu && self.hover_can_be_deleted(h) => Some(h),
            _ => None,
        };
    }

    pub(super) fn context_selection(&self) -> Vec<usize> {
        self.tool.as_ref().map_or(Vec::new(), |t| t.rawSelection.clone())
    }

    /// Global.Update's freezeSelection / igniteSelection / resizeSelection: when the menu has something to act on
    /// (HasObject; ShouldShowResizeButton for Resize), ContextMenuBehaviour.Show, the button's action and Hide
    /// (`at`: the menu's world position), then UISoundBehaviour.Main.Blip(). Whether it ran.
    pub fn context_hotkey(&mut self, button: &str, at: [f32; 2]) -> bool {
        let usable = if button == "Resize" { self.context_should_show_resize_button() } else { self.context_has_object() };
        if usable {
            self.context_menu_show();
            self.context_static_click(button, at);
            self.ui_sound(super::audio::UiSoundKind::Blip);
        }
        usable
    }

    /// Global.Update's eyedrop: SelectionController.Main.CurrentlyUnderMouse's SerialiseInstructions.OriginalSpawnableAsset. The
    /// objects with a SerialiseInstructions are the ones `object_names` holds (catalog spawns, saves' CreateNew, launched projectiles,
    /// eggs); None without an object under the mouse or when its root has none (a map's own objects).
    pub fn eyedrop_asset(&self) -> Option<String> {
        let hover = self.tool.as_ref()?.hover?;
        self.object_names.get(&self.root_id(hover)).cloned()
    }

    /// ContextMenuBehaviour.get_HasObject
    pub fn context_has_object(&self) -> bool {
        self.context_selection().iter().any(|&p| !self.physicals[p].destroyed) && self.context_prior.is_none()
    }

    /// ContextMenuBehaviour.get_CanDelete
    pub fn context_can_delete(&self) -> bool {
        self.context_has_object() || self.context_prior.is_some()
    }

    /// A static button's condition property (ContextMenuInteractableConditionalBehaviour): the property by name, as
    /// `GetProperty` finds it; a name the menu has no property for reads false.
    pub fn context_condition(&mut self, property: &str) -> bool {
        self.context_property(property).unwrap_or(false)
    }

    /// The ContextMenuBehaviour bool property called `name` (PropertyInfo.GetValue while the menu is open), None when the
    /// class has none (ConditionalTextBehaviour.Awake warns and the text is never updated).
    pub fn context_property(&mut self, name: &str) -> Option<bool> {
        let v = self.context_property_value(name)?;
        // The getter's result, under the game method's name (the per-function checks, --ui-screens).
        if let Some(&(_, key)) = CONTEXT_GETTERS.iter().find(|(n, _)| *n == name) {
            crate::ui::uifn(key, || if v { "True" } else { "False" }.into());
        }
        Some(v)
    }

    fn context_property_value(&mut self, name: &str) -> Option<bool> {
        Some(match name {
            "HasObject" => self.context_has_object(),
            "HasSingleObject" => self.context_has_single_object(),
            "CanDelete" => self.context_can_delete(),
            // ContextMenuBehaviour.get_HasCopied
            "HasCopied" => self.clipboard.is_some(),
            "isOpen" => true,
            "isFollowingObject" => self.context_is_following_object(),
            "isFrozen" => self.context_is_frozen(),
            "isNoCollide" => self.context_is_no_collide(),
            "isWeightless" => self.context_is_weightless(),
            "isOnFire" => self.context_is_on_fire(),
            "ShouldStopResizing" => self.context_should_stop_resizing(),
            "ShouldShowResizeButton" => self.context_should_show_resize_button(),
            "ShouldHideLayer" => self.context_should_hide_layer(),
            // ContextMenuBehaviour.get_ShouldShowLayerButton: LayerEditorController.Instance exists in the Main scene.
            "ShouldShowLayerButton" => self.context_has_object(),
            "isResizing" => self.context_is_resizing(),
            // ContextMenuBehaviour.get_CanAmalgamate ([Obsolete])
            "CanAmalgamate" => false,
            _ => return None,
        })
    }

    /// ContextMenuBehaviour.get_HasSingleObject
    pub(crate) fn context_has_single_object(&self) -> bool {
        self.context_selection().len() == 1 && self.context_prior.is_none()
    }

    /// ContextMenuBehaviour.get_ShouldHideLayer: LayerEditorController.Instance exists in the Main scene.
    pub(crate) fn context_should_hide_layer(&self) -> bool {
        self.layer_editor_is_open()
    }

    /// ContextMenuInteractableConditionalBehaviour's / ConditionalTextBehaviour's buttons: (button, Property, TrueText,
    /// FalseText) of the scene's seven ConditionalTextBehaviours.
    pub const CONDITIONAL_TEXT: [(&'static str, &'static str, &'static str, &'static str); 7] = [
        ("Freeze", "isFrozen", "Unfreeze", "Freeze"),
        ("Ignite", "isOnFire", "Extinguish", "Ignite"),
        ("Gravity", "isWeightless", "Restore weight", "Make weightless"),
        ("Follow", "isFollowingObject", "Unfollow", "Follow"),
        ("Resize", "ShouldStopResizing", "Stop resizing", "Resize"),
        ("No Collide", "isNoCollide", "Enable collision", "Disable collision"),
        ("Edit layer", "ShouldHideLayer", "Stop editing layer", "Edit layer"),
    ];

    /// ConditionalTextBehaviour.Update for the open menu's seven (`dt` is Time.unscaledDeltaTime): every 0.1 s the
    /// property is read and the text set; an exception is logged and leaves the text.
    pub fn conditional_text_update(&mut self, dt: f32) {
        for i in 0..Self::CONDITIONAL_TEXT.len() {
            self.conditional_text_update_one(i, dt, true);
        }
    }

    /// ConditionalTextBehaviour.Update of the `i`th of the seven; `found`: foundProperty set (its Awake ran).
    pub fn conditional_text_update_one(&mut self, i: usize, dt: f32, found: bool) {
        self.context_text_t[i] += dt;
        if self.context_text_t[i] < 0.1 {
            return;
        }
        self.context_text_t[i] = 0.0;
        // foundProperty == null (Component is the scene's ContextMenuBehaviour, never null).
        if !found {
            return;
        }
        let (was_clicking, was_threw) = (self.menu_clicking, self.menu_threw);
        // try/catch: menu_throw logs (Debug.LogError) and flags.
        (self.menu_clicking, self.menu_threw) = (true, false);
        let value = self.context_property(Self::CONDITIONAL_TEXT[i].1);
        let threw = self.menu_threw;
        (self.menu_clicking, self.menu_threw) = (was_clicking, was_threw);
        if let (Some(v), false) = (value, threw) {
            self.context_text[i] = Some(v);
        }
    }

    /// The text ConditionalTextBehaviour last set on a static button, if any (else the scene's own label shows).
    pub fn context_label(&self, button: &str) -> Option<&'static str> {
        let i = Self::CONDITIONAL_TEXT.iter().position(|c| c.0 == button)?;
        self.context_text[i].map(|v| if v { Self::CONDITIONAL_TEXT[i].2 } else { Self::CONDITIONAL_TEXT[i].3 })
    }

    /// ContextMenuBehaviour.ClampToScreen / Update: the menu's position (Unity screen pixels, y up) kept on screen.
    /// The menu is a ScrollRect of fixed sizeDelta (160.78, 314.42) under the CanvasScaler (reference 2688 x 1080,
    /// matchWidthOrHeight 0).
    pub fn context_clamp_to_screen(position: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
        let scale = 2688.0 / mathf::lerp(screen[0], screen[1], 0.0);
        let vector = [160.78_f32 * 2.0 / scale, 314.42_f32 * 2.0 / scale];
        [mathf::clamp(position[0], 0.0, screen[0] - vector[0]), mathf::clamp(position[1], vector[1], screen[1])]
    }

    /// ContextMenuBehaviour.get_isFollowingObject (isOpen: only read from a click)
    pub(crate) fn context_is_following_object(&self) -> bool {
        let following = &self.camera_control.CurrentlyFollowing;
        if !self.context_has_object() || following.is_empty() {
            return false;
        }
        // Intersect is distinct.
        let selection = self.context_selection();
        let mut both: Vec<usize> = Vec::new();
        for &p in following {
            if selection.contains(&p) && !both.contains(&p) {
                both.push(p);
            }
        }
        both.len() == selection.len()
    }

    /// The live FreezeBehaviour on a physical object's GameObject.
    fn context_freeze_of(&self, p: usize) -> Option<usize> {
        self.freezes.iter().position(|f| f.physical == p && !self.destroyed_components.contains(&f.id))
    }

    /// ContextMenuBehaviour.get_isFrozen
    pub(crate) fn context_is_frozen(&self) -> bool {
        self.context_has_object() && self.context_selection().iter().any(|&p| self.context_freeze_of(p).is_some())
    }

    /// ContextMenuBehaviour.get_isWeightless
    pub(crate) fn context_is_weightless(&self) -> bool {
        self.context_has_object() && self.context_selection().iter().all(|&p| self.physicals[p].IsWeightless)
    }

    /// ContextMenuBehaviour.get_isOnFire
    pub(crate) fn context_is_on_fire(&self) -> bool {
        self.context_has_object() && self.context_selection().iter().all(|&p| self.physicals[p].destroyed || self.physicals[p].OnFire)
    }

    /// A static button's onClick (`at`: the menu's world position, Camera.main.ScreenToWorldPoint(transform.position)).
    /// Returns whether the action hid the menu.
    pub fn context_static_click(&mut self, name: &str, at: [f32; 2]) -> bool {
        let hid = self.context_static_action(name, at);
        // The per-function checks (--ui-screens): each action's own result, read after it ran.
        let cs = |b: bool| if b { "True" } else { "False" }.to_string();
        let state = match name {
            "Delete" => Some(("DeleteAction", self.tool.as_ref().map_or(0, |t| t.rawSelection.len()).to_string())),
            "Copy" => Some(("CopyAction", cs(self.clipboard.is_some()))),
            "Follow" => Some(("FollowAction", cs(self.context_property_value("isFollowingObject") == Some(true)))),
            "Activate" => Some(("ActivateAction", self.context_selection().len().to_string())),
            "Freeze" => Some(("FreezeAction", cs(self.context_property_value("isFrozen") == Some(true)))),
            "Gravity" => Some(("WeightlessAction", cs(self.context_property_value("isWeightless") == Some(true)))),
            "Ignite" => Some(("IgniteAction", cs(self.context_property_value("isOnFire") == Some(true)))),
            "No Collide" => Some(("NoCollideAction", cs(self.context_property_value("isNoCollide") == Some(true)))),
            "Resize" => Some(("ResizeAction", self.resize.Targets.len().to_string())),
            "Edit layer" => Some(("EditLayerAction", cs(self.layer_editor.container_active))),
            _ => None,
        };
        if let Some((m, s)) = state {
            crate::ui::uifn(match m {
                "DeleteAction" => "ContextMenuBehaviour.DeleteAction",
                "CopyAction" => "ContextMenuBehaviour.CopyAction",
                "FollowAction" => "ContextMenuBehaviour.FollowAction",
                "ActivateAction" => "ContextMenuBehaviour.ActivateAction",
                "FreezeAction" => "ContextMenuBehaviour.FreezeAction",
                "WeightlessAction" => "ContextMenuBehaviour.WeightlessAction",
                "IgniteAction" => "ContextMenuBehaviour.IgniteAction",
                "NoCollideAction" => "ContextMenuBehaviour.NoCollideAction",
                "ResizeAction" => "ContextMenuBehaviour.ResizeAction",
                _ => "ContextMenuBehaviour.EditLayerAction",
            }, || s);
        }
        hid
    }

    fn context_static_action(&mut self, name: &str, at: [f32; 2]) -> bool {
        match name {
            "Delete" => self.context_delete_action(),
            "Copy" if !self.context_has_object() => false,
            "Copy" => {
                let (selected, center) = (self.context_selection(), [at[0], at[1], self.MousePosition[2]]);
                self.context_copy_action(&selected, center)
            }
            "Paste" => self.context_paste_action([at[0], at[1], self.MousePosition[2]]),
            "Follow" => self.context_follow_action(),
            "Activate" => self.context_activate_action(),
            "Freeze" => self.context_freeze_action(),
            "Gravity" => self.context_weightless_action(),
            "Resize" => self.context_resize_action(),
            "Ignite" => self.context_ignite_action(at),
            "No Collide" => self.context_no_collide_action(),
            "Edit layer" => self.context_edit_layer_action(),
            "Save" => self.context_save_action(at),
            _ => unimplemented!("mod-only: ContextMenuBehaviour {name} button"),
        }
    }

    /// Whether a static button's action is rewritten.
    pub fn context_static_rewritten(name: &str) -> bool {
        matches!(name, "Delete" | "Copy" | "Paste" | "Follow" | "Activate" | "Freeze" | "Gravity" | "Ignite" | "No Collide" | "Edit layer" | "Resize" | "Save")
    }

    /// Global.main.ResizeHandles && ResizeHandles.HasTargets
    pub(crate) fn context_resize_has_targets(&self) -> bool {
        self.resize_handles.is_some() && self.resize_has_targets()
    }

    /// ContextMenuBehaviour.get_isResizing
    pub(crate) fn context_is_resizing(&self) -> bool {
        let v = self.context_has_object() && self.context_resize_has_targets() && self.context_selection().iter().all(|p| self.resize.Targets.contains(p));
        crate::ui::uifn("ContextMenuBehaviour.get_isResizing", || if v { "True" } else { "False" }.into());
        v
    }

    /// ContextMenuBehaviour.get_ShouldStopResizing
    pub(crate) fn context_should_stop_resizing(&self) -> bool {
        if self.context_is_resizing() {
            return true;
        }
        self.context_resize_has_targets() && !self.context_has_object()
    }

    /// ContextMenuBehaviour.get_ShouldShowResizeButton
    pub(crate) fn context_should_show_resize_button(&self) -> bool {
        if self.context_resize_has_targets() {
            return true;
        }
        self.context_has_object()
    }

    /// ContextMenuBehaviour.DeleteAction: the hovered wire (its OnUserDelete is empty, the menu stays open), else the
    /// selection.
    pub(crate) fn context_delete_action(&mut self) -> bool {
        if let Some(id) = self.context_prior {
            if !self.destroyed_components.contains(&id) {
                self.tr_destroy_component(id, 0.0);
            }
            return false;
        }
        self.user_delete_selection();
        true
    }

    /// ContextMenuBehaviour.FollowAction
    pub(crate) fn context_follow_action(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let flag = self.context_is_following_object();
        let selection = self.context_selection();
        for p in &selection {
            // List.Remove: the first one.
            if let Some(i) = self.camera_control.CurrentlyFollowing.iter().position(|q| q == p) {
                self.camera_control.CurrentlyFollowing.remove(i);
            }
        }
        if !flag {
            self.camera_control.CurrentlyFollowing.extend(selection);
        }
        true
    }

    /// ContextMenuBehaviour.ActivateAction: BroadcastMessage("Use", new ActivationPropagation(direct: true, 0, the object)).
    pub(crate) fn context_activate_action(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        for p in self.context_selection() {
            if !self.physicals[p].destroyed {
                let a = self.signal_direct(true, 0);
                self.sig_mut(&a).Target = Some(p);
                self.broadcast_use(p, &a);
            }
        }
        true
    }

    /// The FreezeSerialisationHelpers on the object and its active children (BroadcastMessage OnUserFreeze/Unfreeze:
    /// IsFrozen).
    fn context_broadcast_frozen(&mut self, p: usize, frozen: bool) {
        let id = self.physicals[p].id;
        let ids: Vec<i32> = self.with_component_instance(id, |_, inst, o| {
            let mut objects = Vec::new();
            super::car::active_below(inst, o, &mut objects);
            objects.iter().flat_map(|&o| inst.objects[o].components.clone()).collect()
        });
        // FreezeSerialisationHelper.OnUserFreeze: IsFrozen = true
        // FreezeSerialisationHelper.OnUserUnfreeze: IsFrozen = false
        for h in self.freeze_serialisations.iter_mut().filter(|h| ids.contains(&h.id)) {
            h.IsFrozen = frozen;
        }
    }

    /// ContextMenuBehaviour.FreezeAction.
    pub(crate) fn context_freeze_action(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let flag = !self.context_is_frozen();
        for p in self.context_selection() {
            if self.physicals[p].destroyed {
                continue;
            }
            if !flag {
                if let Some(f) = self.context_freeze_of(p) {
                    self.freeze_queue_destroy(f);
                    self.context_broadcast_frozen(p, false);
                }
            } else {
                // `selectedObject.PlaySliderSound && (bool)selectedObject.slidingAudioSource`: slidingAudioSource.Stop().
                let sliding = self.physicals[p].ids.slidingAudioSource;
                if self.physicals[p].PlaySliderSound && sliding != 0 {
                    self.audio.stop(sliding);
                }
                self.freeze_add_component(p);
                self.context_broadcast_frozen(p, true);
            }
        }
        true
    }

    /// ContextMenuBehaviour.WeightlessAction
    pub(crate) fn context_weightless_action(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let flag = self.context_is_weightless();
        for p in self.context_selection() {
            if flag {
                self.physical_make_weightful(p);
            } else {
                self.physical_make_weightless(p);
            }
        }
        true
    }

    /// ContextMenuBehaviour.IgniteAction (float.Epsilon is the smallest denormal)
    pub(crate) fn context_ignite_action(&mut self, at: [f32; 2]) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let flag = !self.context_is_on_fire();
        for p in self.context_selection() {
            let flammability = self.properties[self.physicals[p].Properties].Flammability;
            if !(flammability <= f32::from_bits(1)) && !self.physicals[p].destroyed {
                if flag {
                    self.ignite_at(p, true, at);
                    self.physicals[p].burnIntensity = 1.0;
                } else {
                    self.extinguish(p);
                    self.physicals[p].burnIntensity = 0.0;
                }
            }
        }
        true
    }

    /// GameObject.layer = layer on object `o` of a clone (GameObject::SetLayer, 180589640): kLayerChanged reaches its
    /// components in order, and only Collider2D (RecalculateContacts, 180c14640) and Rigidbody2D (180c1cdc0, a wake)
    /// act on it among the physics ones (a Renderer's is rendering).
    pub(super) fn game_object_set_layer(&mut self, inst: &mut super::spawn::Instance, o: usize, layer: i32) {
        self.world.layers.insert(inst.objects[o].game_object, layer);
        let prefab = inst.prefab.clone();
        for (k, (_, component)) in prefab.objects[o].components.iter().enumerate() {
            match component {
                Component::Collider2D(_) => self.world.refilter_collider(inst.objects[o].components[k]),
                Component::Rigidbody2D(_) => {
                    if let Some(&body) = inst.bodies.get(&o) {
                        self.world.wake_body(body);
                    }
                }
                _ => {}
            }
        }
    }

    /// Utils.SetLayer(layer, includeChildren: true): the object, then each direct child tagged RecursiveLayerChild
    /// (which does the same below it).
    fn utils_set_layer(&mut self, inst: &mut super::spawn::Instance, o: usize, layer: i32) {
        self.game_object_set_layer(inst, o, layer);
        let children = inst.prefab.objects[o].children.clone();
        for c in children {
            if inst.prefab.objects[c].tag == RECURSIVE_LAYER_CHILD_TAG {
                self.utils_set_layer(inst, c, layer);
            }
        }
    }

    /// `selectedObject.gameObject.layer`
    fn physical_object_layer(&mut self, p: usize) -> i32 {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| g.world.layers[&inst.objects[o].game_object])
    }

    /// ContextMenuBehaviour.get_isNoCollide
    pub(crate) fn context_is_no_collide(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        for p in self.context_selection() {
            if self.physicals[p].destroyed {
                // .gameObject of a destroyed object throws.
                self.menu_throw("MissingReferenceException: ContextMenuBehaviour.get_isNoCollide");
                return false;
            }
            if self.physical_object_layer(p) != 10 {
                return false;
            }
        }
        true
    }

    /// ContextMenuBehaviour.NoCollideAction
    pub(crate) fn context_no_collide_action(&mut self) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let flag = self.context_is_no_collide();
        if self.menu_threw {
            return false;
        }
        for p in self.context_selection() {
            if self.physicals[p].destroyed {
                continue;
            }
            let id = self.physicals[p].id;
            self.with_component_instance(id, |g, inst, o| g.utils_set_layer(inst, o, if flag { 9 } else { 10 }));
            // GetComponent<LayerSerialisationBehaviour>(), else AddComponent: a new component last on the object; Start runs
            // with the pending Starts.
            if !self.layer_serialisations.contains_key(&p) {
                let cid = self.allocate_instance_id();
                self.with_component_instance(id, |_, inst, o| {
                    let mut prefab = (*inst.prefab).clone();
                    let n = prefab.objects[o].components.len() as i64;
                    prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, Component::Script { class: "LayerSerialisationBehaviour".into(), execution_order: 0, enabled: true, fields: crate::assets::mono::Value::Struct(Vec::new()) }));
                    inst.prefab = std::sync::Arc::new(prefab);
                    inst.objects[o].components.push(cid);
                });
                self.layer_serialisations.insert(p, LayerSerialisation { id: cid, Layer: -1 });
                self.pending_starts.push(super::misc::PendingStart::LayerSerialisation(p));
            }
            let layer = self.physical_object_layer(p);
            self.layer_serialisations.get_mut(&p).unwrap().Layer = layer;
        }
        true
    }

    /// LayerSerialisationBehaviour.Start: a fresh component keeps the object's layer, else the saved Layer is applied.
    pub(crate) fn layer_serialisation_start(&mut self, p: usize) {
        let layer = self.layer_serialisations[&p].Layer;
        if layer == -1 {
            let l = self.physical_object_layer(p);
            self.layer_serialisations.get_mut(&p).unwrap().Layer = l;
        } else {
            let id = self.physicals[p].id;
            self.with_component_instance(id, |g, inst, o| g.utils_set_layer(inst, o, layer));
        }
    }

    /// DragTool.IsAnyParentSelected: `phys` is below another selected object's Transform.
    pub(crate) fn drag_is_any_parent_selected(&mut self, phys: usize) -> bool {
        let id = self.physicals[phys].id;
        let others: Vec<i32> = self.context_selection().iter().filter(|&&q| q != phys).map(|&q| self.physicals[q].id).collect();
        self.with_component_instance(id, |_, inst, o| {
            let mut up = inst.objects[o].parent;
            while let Some(a) = up {
                if others.iter().any(|c| inst.objects[a].components.contains(c)) {
                    return true;
                }
                up = inst.objects[a].parent;
            }
            false
        })
    }

    /// ContextMenuBehaviour.ResizeAction
    pub(crate) fn context_resize_action(&mut self) -> bool {
        if self.context_should_stop_resizing() {
            self.resize_handles_clear_targets();
        } else {
            let selection = self.context_selection();
            let hdm = if selection.iter().filter(|&&p| self.physicals[p].Resizable).count() <= 1 {
                let Some(&first) = selection.first() else {
                    self.menu_throw("ArgumentOutOfRangeException: ContextMenuBehaviour.ResizeAction");
                    return false;
                };
                if !self.physicals[first].Resizable {
                    return false;
                }
                let e = self.physicals[first].InitialBounds.1;
                [e[0], e[1]]
            } else {
                [1.0, 1.0]
            };
            let mut targets = Vec::new();
            for p in selection {
                if self.physicals[p].Resizable && !self.drag_is_any_parent_selected(p) {
                    targets.push(p);
                }
            }
            self.resize_handles_set_targets(targets, hdm);
            return true;
        }
        self.resize_reset_handles();
        true
    }

    /// ContextMenuBehaviour.EditLayerAction
    pub(crate) fn context_edit_layer_action(&mut self) -> bool {
        if self.layer_editor_is_open() {
            self.layer_editor_hide();
        } else {
            let selection = self.context_selection();
            self.layer_editor.Targets.clear();
            self.layer_editor.Targets.extend(selection);
            self.layer_editor_show();
        }
        true
    }
}

/// The ContextMenuBehaviour property names and their getters' method names.
const CONTEXT_GETTERS: [(&str, &str); 16] = [
    ("HasObject", "ContextMenuBehaviour.get_HasObject"),
    ("HasSingleObject", "ContextMenuBehaviour.get_HasSingleObject"),
    ("CanDelete", "ContextMenuBehaviour.get_CanDelete"),
    ("HasCopied", "ContextMenuBehaviour.get_HasCopied"),
    ("isOpen", "ContextMenuBehaviour.get_isOpen"),
    ("isFollowingObject", "ContextMenuBehaviour.get_isFollowingObject"),
    ("isFrozen", "ContextMenuBehaviour.get_isFrozen"),
    ("isNoCollide", "ContextMenuBehaviour.get_isNoCollide"),
    ("isWeightless", "ContextMenuBehaviour.get_isWeightless"),
    ("isOnFire", "ContextMenuBehaviour.get_isOnFire"),
    ("ShouldStopResizing", "ContextMenuBehaviour.get_ShouldStopResizing"),
    ("ShouldShowResizeButton", "ContextMenuBehaviour.get_ShouldShowResizeButton"),
    ("ShouldHideLayer", "ContextMenuBehaviour.get_ShouldHideLayer"),
    ("ShouldShowLayerButton", "ContextMenuBehaviour.get_ShouldShowLayerButton"),
    ("isResizing", "ContextMenuBehaviour.get_isResizing"),
    ("CanAmalgamate", "ContextMenuBehaviour.get_CanAmalgamate"),
];
