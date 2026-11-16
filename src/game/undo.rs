//! UndoControllerBehaviour with the IUndoableAction it keeps, and the text NotificationControllerBehaviour.Show
//! queues for the UI. ObjectCreationAction is the only action a rewritten tool registers (CatalogBehaviour.Spawn);
//! PasteLoadAction needs ObjectStateConverter (copy and paste); FreezeObjectAction is [Obsolete], no tool creates one, but
//! it is an IUndoableAction like the rest and runs from the history when one is registered.
use super::Game;

/// ObjectCreationAction: the objects (by their GameObject's Transform id) its Undo destroys.
#[derive(Default, Clone, Debug)]
pub struct ObjectCreationAction {
    pub displayName: String,
    pub RelevantObjects: Vec<i32>,
}

/// FreezeObjectAction: the object (its Transform's component id) and the name its notifications show.
#[derive(Default, Clone, Debug)]
pub struct FreezeObjectAction {
    pub displayName: String,
    pub RelevantObject: i32,
}

/// IUndoableAction: the two implementations a history holds.
#[derive(Clone, Debug)]
pub enum UndoAction {
    Creation(ObjectCreationAction),
    Freeze(FreezeObjectAction),
    Paste(super::save::PasteLoadAction),
}

impl From<ObjectCreationAction> for UndoAction {
    fn from(a: ObjectCreationAction) -> Self {
        UndoAction::Creation(a)
    }
}

impl From<FreezeObjectAction> for UndoAction {
    fn from(a: FreezeObjectAction) -> Self {
        UndoAction::Freeze(a)
    }
}

impl From<super::save::PasteLoadAction> for UndoAction {
    fn from(a: super::save::PasteLoadAction) -> Self {
        UndoAction::Paste(a)
    }
}

impl ObjectCreationAction {
    /// ObjectCreationAction.IsValid: RelevantObjects.Any().
    pub fn is_valid(&self) -> bool {
        !self.RelevantObjects.is_empty()
    }

    /// ObjectCreationAction.Redo: throw new NotImplementedException().
    pub fn redo(&self) -> ! {
        super::menu::throw("NotImplementedException: The method or operation is not implemented.")
    }
}

impl super::save::PasteLoadAction {
    /// PasteLoadAction.IsValid: RelevantObjects != null.
    pub fn is_valid(&self) -> bool {
        true
    }

    /// PasteLoadAction.Redo: throw new NotImplementedException().
    pub fn redo(&self) -> ! {
        super::menu::throw("NotImplementedException: The method or operation is not implemented.")
    }
}

impl Game {
    /// FreezeObjectAction.IsValid: RelevantObject != null (a destroyed GameObject compares equal to null).
    pub fn freeze_object_action_is_valid(&self, a: &FreezeObjectAction) -> bool {
        !self.destroyed_components.contains(&a.RelevantObject)
    }
}

impl FreezeObjectAction {
    /// FreezeObjectAction.Redo: throw new NotImplementedException().
    pub fn redo(&self) -> ! {
        super::menu::throw("NotImplementedException: The method or operation is not implemented.")
    }
}

impl UndoAction {
    pub fn display_name(&self) -> &str {
        match self {
            UndoAction::Creation(a) => &a.displayName,
            UndoAction::Freeze(a) => &a.displayName,
            UndoAction::Paste(a) => &a.displayName,
        }
    }

    pub fn relevant_objects(&self) -> Vec<i32> {
        match self {
            UndoAction::Creation(a) => a.RelevantObjects.clone(),
            UndoAction::Freeze(a) => vec![a.RelevantObject],
            UndoAction::Paste(a) => a.RelevantObjects.clone(),
        }
    }

    /// ObjectCreationAction.IsRelatedTo / FreezeObjectAction.IsRelatedTo / PasteLoadAction.IsRelatedTo: `o == RelevantObject`.
    pub fn is_related_to(&self, o: i32) -> bool {
        match self {
            UndoAction::Creation(a) => a.RelevantObjects.contains(&o),
            UndoAction::Freeze(a) => a.RelevantObject == o,
            UndoAction::Paste(a) => a.RelevantObjects.contains(&o),
        }
    }
}

/// UndoControllerBehaviour: `history` and the static currentHistoryPosition (Awake sets it to -1).
#[derive(Clone, Debug)]
pub struct UndoController {
    pub history: Vec<UndoAction>,
    pub currentHistoryPosition: i32,
}

impl Default for UndoController {
    fn default() -> Self {
        // UndoControllerBehaviour.Awake: main = this, currentHistoryPosition = -1
        Self { history: Vec::new(), currentHistoryPosition: -1 }
    }
}

impl Game {
    /// NotificationControllerBehaviour.Show: the text goes to the UI's notifications.
    pub fn notify(&mut self, text: String) {
        self.notifications.push(text);
    }

    /// ModdingApi.Notify, ModAPI.Notify: NotificationControllerBehaviour.Show(message.ToString()).
    pub fn modding_api_notify(&mut self, message: &str) {
        self.notify(message.to_string());
    }

    /// UndoControllerBehaviour.RegisterAction: inserted after the current position (undone actions stay behind it).
    pub fn undo_register_action(&mut self, action: impl Into<UndoAction>) {
        let u = &mut self.undo;
        u.history.insert((u.currentHistoryPosition + 1) as usize, action.into());
        u.currentHistoryPosition += 1;
    }

    /// UndoControllerBehaviour.FindRelevantAction then DeregisterAction of what it found (the callers' pair).
    pub fn undo_deregister_related(&mut self, object: i32) {
        if let Some(i) = self.undo_find_relevant_action(object) {
            self.undo_deregister_action(Some(i));
        }
    }

    /// UndoControllerBehaviour.FindRelevantAction: the first action whose IsRelatedTo(o) holds (its history index), or none.
    pub fn undo_find_relevant_action(&self, object: i32) -> Option<usize> {
        self.undo.history.iter().position(|a| a.is_related_to(object))
    }

    /// UndoControllerBehaviour.DeregisterAction: an action in the history (its index; None for one not in it) leaves it, and
    /// the position steps back when it was at or before it.
    pub fn undo_deregister_action(&mut self, action: Option<usize>) {
        let u = &mut self.undo;
        let Some(i) = action.filter(|&i| i < u.history.len()) else { return };
        if i as i32 <= u.currentHistoryPosition {
            u.currentHistoryPosition -= 1;
        }
        u.history.remove(i);
    }

    /// UndoControllerBehaviour.Undo: ObjectCreationAction.Undo destroys its objects and shows "<b>NAME</b> removed".
    pub fn undo(&mut self) {
        if self.undo.currentHistoryPosition == -1 {
            return;
        }
        let i = self.undo.currentHistoryPosition as usize;
        match self.undo.history[i].clone() {
            UndoAction::Creation(a) => {
                for o in a.RelevantObjects {
                    // A tool's Behaviour (Object.Destroy(component): a wire, pin, belt or phase link), else a GameObject (its
                    // Transform, never a Behaviour).
                    if self.blood_wires.iter().any(|w| w.id == o) || self.pins.iter().any(|p| p.id == o) || self.behaviour_of.contains_key(&o) {
                        self.tr_destroy_component(o, 0.0);
                    } else {
                        self.destroy_object(o);
                    }
                }
                let name = a.displayName.to_uppercase();
                if let UndoAction::Creation(a) = &mut self.undo.history[i] {
                    a.displayName = name.clone();
                }
                self.notify(format!("<b>{name}</b> removed"));
            }
            UndoAction::Freeze(a) => self.freeze_object_action_undo(&a),
            UndoAction::Paste(a) => self.paste_load_action_undo(&a),
        }
        self.undo.currentHistoryPosition -= 1;
    }

    /// FreezeObjectAction.Undo: a PhysicalBehaviour on the object toggles its FreezeBehaviour ("unfrozen again" destroys
    /// it, "frozen again" adds one). FreezeObjectAction.Redo throws NotImplementedException and nothing calls it.
    fn freeze_object_action_undo(&mut self, a: &FreezeObjectAction) {
        let components = self.with_component_instance(a.RelevantObject, |_, inst, o| inst.objects[o].components.clone());
        let Some(p) = self.physicals.iter().position(|x| components.contains(&x.id) && !self.destroyed_components.contains(&x.id)) else { return };
        match self.freezes.iter().position(|f| f.physical == p && !self.destroyed_components.contains(&f.id)) {
            Some(f) => {
                self.notify(format!("<b>{}</b> unfrozen again", a.displayName));
                self.freeze_queue_destroy(f);
            }
            None => {
                self.notify(format!("<b>{}</b> frozen again", a.displayName));
                self.freeze_add_component(p);
            }
        }
    }

    /// PasteLoadAction.Undo: each pasted GameObject is destroyed, then "<b>NAME</b> removed" (the name upper-cased for good).
    /// PasteLoadAction.Redo throws NotImplementedException and nothing calls it. IsValid (RelevantObjects != null) is always true.
    fn paste_load_action_undo(&mut self, a: &super::save::PasteLoadAction) {
        for &o in &a.RelevantObjects {
            self.destroy_object(o);
        }
        let name = a.displayName.to_uppercase();
        let i = self.undo.currentHistoryPosition as usize;
        if let UndoAction::Paste(a) = &mut self.undo.history[i] {
            a.displayName = name.clone();
        }
        self.notify(format!("<b>{name}</b> removed"));
    }

    /// UndoControllerBehaviour.ClearHistory
    pub fn undo_clear_history(&mut self) {
        self.undo.currentHistoryPosition = -1;
        self.undo.history.clear();
    }

    /// UndoControllerBehaviour.Update (no pause menu, dialog or UI block is ever open).
    pub fn undo_controller_update(&mut self) {
        if self.input.up(crate::il!(str "UndoControllerBehaviour::Update" 0)) {
            self.undo();
        }
    }
}
