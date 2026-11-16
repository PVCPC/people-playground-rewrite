//! The UI event components: UiEventEmitter, UiHoverEventEmitter, OnPointerUpEvent, ScreenUIEventEmitter, InvokeOnEscape and
//! ShowDialogBehaviour. A UnityEvent is a list of listeners.
#![allow(non_snake_case)]

use super::state::{DialogView, Ui};
use bevy::{picking::hover::Hovered, prelude::*};
use std::collections::HashSet;

/// UnityEvent: the listeners, invoked in the order they were added.
#[derive(Default)]
pub struct UnityEvent {
    listeners: Vec<Box<dyn FnMut() + Send + Sync>>,
}

/// The listener count (closures): `UnityEvent(n)`, which fn-check compares with the game's persistent plus runtime calls.
impl std::fmt::Debug for UnityEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnityEvent({})", self.listeners.len())
    }
}

impl UnityEvent {
    pub fn add_listener(&mut self, f: impl FnMut() + Send + Sync + 'static) {
        self.listeners.push(Box::new(f));
    }

    pub fn invoke(&mut self) {
        for l in &mut self.listeners {
            l();
        }
    }
}

/// UiEventEmitter: five events for the pointer on a UI element.
#[derive(Debug, Component, Default)]
pub struct UiEventEmitter {
    pub OnMouseEnter: UnityEvent,
    pub OnMouseExit: UnityEvent,
    pub OnMouseDown: UnityEvent,
    pub OnMouseUp: UnityEvent,
    pub OnMouseClick: UnityEvent,
}

impl UiEventEmitter {
    /// UiEventEmitter.OnPointerClick
    pub fn on_pointer_click(&mut self) {
        self.OnMouseClick.invoke();
    }

    /// UiEventEmitter.OnPointerDown
    pub fn on_pointer_down(&mut self) {
        self.OnMouseDown.invoke();
    }

    /// UiEventEmitter.OnPointerEnter
    pub fn on_pointer_enter(&mut self) {
        self.OnMouseEnter.invoke();
    }

    /// UiEventEmitter.OnPointerExit
    pub fn on_pointer_exit(&mut self) {
        self.OnMouseExit.invoke();
    }

    /// UiEventEmitter.OnPointerUp
    pub fn on_pointer_up(&mut self) {
        self.OnMouseUp.invoke();
    }
}

/// UiHoverEventEmitter: the pointer entering and leaving a UI element.
#[derive(Debug, Component, Default)]
pub struct UiHoverEventEmitter {
    pub OnMouseEnter: UnityEvent,
    pub OnMouseExit: UnityEvent,
}

impl UiHoverEventEmitter {
    /// UiHoverEventEmitter.OnPointerEnter
    pub fn on_pointer_enter(&mut self) {
        self.OnMouseEnter.invoke();
    }

    /// UiHoverEventEmitter.OnPointerExit
    pub fn on_pointer_exit(&mut self) {
        self.OnMouseExit.invoke();
    }
}

/// OnPointerUpEvent: the pointer released on a UI element.
#[derive(Debug, Component, Default)]
pub struct OnPointerUpEvent {
    pub onPointerUp: UnityEvent,
}

impl OnPointerUpEvent {
    /// OnPointerUpEvent.OnPointerUp
    pub fn on_pointer_up(&mut self) {
        self.onPointerUp.invoke();
    }
}

/// The EventSystem's delivery for the three components above, from the Hovered state and the left button: enter and exit as a UI element becomes
/// hovered or not, down on the hovered element, up on the element that was pressed (wherever the pointer is), click when that is still the hovered one.
pub fn pointer_events(
    mouse: Res<ButtonInput<MouseButton>>,
    mut emitters: Query<(Entity, &mut UiEventEmitter, &Hovered)>,
    mut hovers: Query<(Entity, &mut UiHoverEventEmitter, &Hovered)>,
    mut ups: Query<(Entity, &mut OnPointerUpEvent, &Hovered)>,
    mut inside: Local<HashSet<Entity>>,
    mut pressed: Local<HashSet<Entity>>,
) {
    for (e, mut m, h) in &mut emitters {
        match (h.get(), inside.contains(&e)) {
            (true, false) => m.on_pointer_enter(),
            (false, true) => m.on_pointer_exit(),
            _ => {}
        }
        if h.get() && mouse.just_pressed(MouseButton::Left) {
            pressed.insert(e);
            m.on_pointer_down();
        }
        if mouse.just_released(MouseButton::Left) && pressed.remove(&e) {
            m.on_pointer_up();
            if h.get() {
                m.on_pointer_click();
            }
        }
    }
    for (e, mut m, h) in &mut hovers {
        match (h.get(), inside.contains(&e)) {
            (true, false) => m.on_pointer_enter(),
            (false, true) => m.on_pointer_exit(),
            _ => {}
        }
    }
    for (e, mut m, h) in &mut ups {
        if h.get() && mouse.just_pressed(MouseButton::Left) {
            pressed.insert(e);
        }
        if mouse.just_released(MouseButton::Left) && pressed.remove(&e) {
            m.on_pointer_up();
        }
    }
    inside.clear();
    inside.extend(emitters.iter().filter(|(_, _, h)| h.get()).map(|(e, ..)| e));
    inside.extend(hovers.iter().filter(|(_, _, h)| h.get()).map(|(e, ..)| e));
}

/// A BoxCollider2D with its Transform: the point test of Collider2D.OverlapPoint.
#[derive(Clone, Copy, Debug)]
pub struct BoxCollider2D {
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub position: [f32; 2],
    /// The Transform's rotation about z, in radians.
    pub rotation: f32,
    pub scale: [f32; 2],
}

impl BoxCollider2D {
    /// Collider2D.OverlapPoint: the point in the box's local space (inverse of position, rotation and scale) within the offset box.
    pub fn overlap_point(&self, p: [f32; 2]) -> bool {
        let (s, c) = self.rotation.sin_cos();
        let (dx, dy) = (p[0] - self.position[0], p[1] - self.position[1]);
        let local = [(c * dx + s * dy) / self.scale[0], (-s * dx + c * dy) / self.scale[1]];
        (local[0] - self.offset[0]).abs() <= self.size[0] / 2.0 && (local[1] - self.offset[1]).abs() <= self.size[1] / 2.0
    }
}

/// ScreenUIEventEmitter: a world-space collider that reports the pointer entering, leaving and pressing it (the map editor's gizmo handles).
#[derive(Debug)]
pub struct ScreenUiEventEmitter {
    pub MouseDownControlCode: String,
    pub IncludeChildren: bool,
    pub OnMouseEnter: UnityEvent,
    pub OnMouseExit: UnityEvent,
    pub OnMouseDown: UnityEvent,
    pub IsMouseInside: bool,
    pub IsBeingDragged: bool,
    /// GameObject.activeInHierarchy
    pub active_in_hierarchy: bool,
    colliders: Vec<BoxCollider2D>,
}

impl ScreenUiEventEmitter {
    /// ScreenUIEventEmitter..ctor: MouseDownControlCode "drag", the rest off.
    pub fn new() -> Self {
        ScreenUiEventEmitter {
            MouseDownControlCode: "drag".into(),
            IncludeChildren: false,
            OnMouseEnter: UnityEvent::default(),
            OnMouseExit: UnityEvent::default(),
            OnMouseDown: UnityEvent::default(),
            IsMouseInside: false,
            IsBeingDragged: false,
            active_in_hierarchy: true,
            colliders: Vec::new(),
        }
    }
}

impl Default for ScreenUiEventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

/// ScreenUIEventEmitter's static `screenUIEventEmitters`: the live emitters; a destroyed one leaves its slot empty.
#[derive(Default)]
pub struct ScreenUiEventEmitters {
    emitters: Vec<Option<ScreenUiEventEmitter>>,
}

impl ScreenUiEventEmitters {
    /// ScreenUIEventEmitter.Awake: added to the set, with the colliders of its own GameObject, or of it and its children with IncludeChildren
    /// (`own` and `children`). Its slot.
    pub fn awake(&mut self, mut e: ScreenUiEventEmitter, own: Vec<BoxCollider2D>, children: Vec<BoxCollider2D>) -> usize {
        e.colliders = if e.IncludeChildren { children } else { own };
        self.emitters.push(Some(e));
        self.emitters.len() - 1
    }

    /// ScreenUIEventEmitter.OnDestroy: removed from the set.
    pub fn on_destroy(&mut self, id: usize) {
        self.emitters[id] = None;
    }

    pub fn get(&self, id: usize) -> Option<&ScreenUiEventEmitter> {
        self.emitters.get(id).and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut ScreenUiEventEmitter> {
        self.emitters.get_mut(id).and_then(Option::as_mut)
    }

    /// ScreenUIEventEmitter.IsMouseOverAny: an active emitter the mouse is inside.
    pub fn is_mouse_over_any(&self) -> bool {
        self.emitters.iter().flatten().any(|e| e.active_in_hierarchy && e.IsMouseInside)
    }

    /// ScreenUIEventEmitter.LateUpdate: `mouse` is Camera.ScreenToWorldPoint(Input.mousePosition), `down` and `up` InputSystem.Down/Up of
    /// the emitter's MouseDownControlCode.
    pub fn late_update(&mut self, id: usize, mouse: [f32; 2], down: bool, up: bool) {
        let Some(e) = self.get_mut(id) else { return };
        let was_inside = e.IsMouseInside;
        e.IsMouseInside = false;
        if !self.is_mouse_over_any() {
            let e = self.get_mut(id).unwrap();
            e.IsMouseInside = e.colliders.iter().any(|c| c.overlap_point(mouse));
        }
        let e = self.get_mut(id).unwrap();
        if !was_inside && e.IsMouseInside {
            e.OnMouseEnter.invoke();
        }
        if was_inside && !e.IsMouseInside {
            e.OnMouseExit.invoke();
        }
        if e.IsMouseInside && down {
            e.IsBeingDragged = true;
            e.OnMouseDown.invoke();
        }
        if e.IsBeingDragged && up {
            e.IsBeingDragged = false;
        }
    }

    /// ScreenUIEventEmitter.OnDisable: no longer dragged, and the mouse leaves if it was inside.
    pub fn on_disable(&mut self, id: usize) {
        let Some(e) = self.get_mut(id) else { return };
        e.IsBeingDragged = false;
        if e.IsMouseInside {
            e.OnMouseExit.invoke();
        }
        e.IsMouseInside = false;
    }
}

/// InvokeOnEscape: OnEscapePress when Escape goes down and no trigger is being edited.
#[derive(Default)]
pub struct InvokeOnEscape {
    pub OnEscapePress: UnityEvent,
}

impl InvokeOnEscape {
    /// InvokeOnEscape.Update: `escape_down` is Input.GetKeyDown(Escape), `being_edited` TriggerEditorBehaviour.IsBeingEdited.
    pub fn update(&mut self, escape_down: bool, being_edited: bool) {
        if escape_down && !being_edited {
            self.OnEscapePress.invoke();
        }
    }
}

/// ShowDialogBehaviour: a Notification with a title and message, optionally when it starts.
#[derive(Debug)]
pub struct ShowDialogBehaviour {
    pub ShouldShowOnStart: bool,
    /// OptionalFloat: Active and Value.
    pub Width: Option<f32>,
    pub Height: Option<f32>,
    pub Title: String,
    pub Message: String,
    pub PlayErrorSound: bool,
    pub PlayWarningSound: bool,
}

impl Default for ShowDialogBehaviour {
    /// ShowDialogBehaviour..ctor: Title "Untitled", Message "Allan please add details".
    fn default() -> Self {
        ShowDialogBehaviour {
            ShouldShowOnStart: false,
            Width: None,
            Height: None,
            Title: "Untitled".into(),
            Message: crate::il!(str "ShowDialogBehaviour::.ctor" 1).into(),
            PlayErrorSound: false,
            PlayWarningSound: false,
        }
    }
}

impl ShowDialogBehaviour {
    /// ShowDialogBehaviour.Start
    pub fn start(&self, ui: &mut Ui, active_in_hierarchy: bool) {
        if self.ShouldShowOnStart {
            self.show_dialog(ui, active_in_hierarchy);
        }
    }

    /// ShowDialogBehaviour.ShowDialog: while active, a Notification of the bold title, a blank line and the message, then SetWidth and SetHeight
    /// when active. (UISoundBehaviour.Error and Warning are audio.)
    pub fn show_dialog(&self, ui: &mut Ui, active_in_hierarchy: bool) {
        if !active_in_hierarchy {
            return;
        }
        let mut text = String::new();
        if !self.Title.trim().is_empty() {
            text = format!("<b>{}</b>\n\n", self.Title);
        }
        if !self.Message.trim().is_empty() {
            text += &self.Message;
        }
        let mut dialog = DialogView::notification(text);
        if let Some(w) = self.Width {
            dialog.set_width(w);
        }
        if let Some(h) = self.Height {
            dialog.set_height(h);
        }
        ui.open_dialog(dialog);
        ui.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn counter(e: &mut UnityEvent) -> Arc<Mutex<u32>> {
        let n = Arc::new(Mutex::new(0));
        let m = n.clone();
        e.add_listener(move || *m.lock().unwrap() += 1);
        n
    }

    fn handle(x: f32) -> BoxCollider2D {
        BoxCollider2D { offset: [0.0; 2], size: [2.0, 1.0], position: [x, 0.0], rotation: 0.0, scale: [1.0; 2] }
    }

    #[test]
    fn emitters_invoke_their_events() {
        let mut u = UiEventEmitter::default();
        let (click, up) = (counter(&mut u.OnMouseClick), counter(&mut u.OnMouseUp));
        u.on_pointer_click();
        u.on_pointer_up();
        u.on_pointer_up();
        assert_eq!((*click.lock().unwrap(), *up.lock().unwrap()), (1, 2));
        let mut esc = InvokeOnEscape::default();
        let n = counter(&mut esc.OnEscapePress);
        esc.update(true, true);
        esc.update(false, false);
        esc.update(true, false);
        assert_eq!(*n.lock().unwrap(), 1);
    }

    #[test]
    fn screen_emitters_track_enter_exit_drag_and_block_each_other() {
        let mut set = ScreenUiEventEmitters::default();
        let (mut a, b) = (ScreenUiEventEmitter::new(), ScreenUiEventEmitter::new());
        let (enter, exit, press) = (counter(&mut a.OnMouseEnter), counter(&mut a.OnMouseExit), counter(&mut a.OnMouseDown));
        let (a, b) = (set.awake(a, vec![handle(0.0)], vec![]), set.awake(b, vec![handle(0.0)], vec![]));
        set.late_update(a, [0.5, 0.0], true, false);
        assert!(set.get(a).unwrap().IsMouseInside && set.get(a).unwrap().IsBeingDragged && set.is_mouse_over_any());
        // The second, on the same spot, finds the mouse already over one.
        set.late_update(b, [0.5, 0.0], false, false);
        assert!(!set.get(b).unwrap().IsMouseInside);
        set.late_update(a, [5.0, 0.0], false, true);
        assert!(!set.get(a).unwrap().IsBeingDragged && !set.is_mouse_over_any());
        set.late_update(a, [0.0, 0.0], false, false);
        set.on_disable(a);
        assert_eq!((*enter.lock().unwrap(), *exit.lock().unwrap(), *press.lock().unwrap()), (2, 2, 1));
        set.on_destroy(a);
        assert!(set.get(a).is_none());
        // A rotated, scaled box.
        let c = BoxCollider2D { offset: [0.0; 2], size: [2.0, 1.0], position: [0.0; 2], rotation: std::f32::consts::FRAC_PI_2, scale: [2.0, 1.0] };
        assert!(c.overlap_point([0.0, 1.9]) && !c.overlap_point([1.9, 0.0]));
    }

    #[test]
    fn show_dialog_builds_the_notification() {
        let mut ui = crate::ui::new_ui(None, crate::ui::Mode::Menu, None, None);
        let mut d = ShowDialogBehaviour { ShouldShowOnStart: true, Height: Some(300.0), ..Default::default() };
        d.start(&mut ui, false);
        assert!(ui.dialog.is_none());
        d.start(&mut ui, true);
        let v = ui.dialog.take().unwrap();
        assert_eq!((v.title.as_str(), v.height, v.width), ("<b>Untitled</b>\n\nAllan please add details", Some(300.0), None));
        d.Title = " ".into();
        d.show_dialog(&mut ui, true);
        assert_eq!(ui.dialog.unwrap().title, crate::il!(str "ShowDialogBehaviour::.ctor" 1));
    }
}
