//! PortalBehaviour, PortalTraversingBehaviour and TriggerEventBehaviour. A portal's two TriggerEventBehaviours call back
//! into it (set at its Start): an object entering the entry trigger gets a ghost renderer and, through a
//! PortalTraversingBehaviour added to it, the Portal layer until it leaves the exit trigger. The ghost's pose and sprite
//! (OnWillRenderObject) are presentation; only the instance ids it takes and the layer swap are state.
#![allow(non_snake_case)]

use super::Game;

/// LayerMask.NameToLayer("Portal") (TagManager.asset).
const PORTAL_LAYER: i32 = 14;

/// TriggerEventBehaviour: the portal whose StartPassage / StartExit its OnTriggerEnter / OnTriggerExit delegate runs.
#[derive(Clone, Debug, Default)]
pub struct TriggerEvent {
    pub id: i32,
    /// The GameObject's Collider2Ds and Rigidbody2D (0 without one): the trigger messages it receives.
    pub colliders: Vec<i32>,
    pub rigidbody: i32,
    pub on_enter: Option<usize>,
    pub on_exit: Option<usize>,
}

/// PortalBehaviour: the entry and exit TriggerEventBehaviour component ids, and `passingThrough`'s keys (physicals).
#[derive(Clone, Debug, Default)]
pub struct Portal {
    pub id: i32,
    pub entry: i32,
    pub exit: i32,
    pub passing_through: Vec<usize>,
}

/// A PortalTraversingBehaviour added to a physical's GameObject: its Awake's OriginalLayer and its enabled flag.
#[derive(Clone, Debug)]
pub struct PortalTraversing {
    pub id: i32,
    pub physical: usize,
    pub enabled: bool,
    pub original_layer: i32,
}

impl Game {
    /// Whether TriggerEventBehaviour `i` gets the trigger messages of collider `own`.
    pub(super) fn trigger_event_receives(&self, i: usize, own: i32, rigidbody: Option<crate::b2::world::RigidbodyRef>) -> bool {
        let s = &self.trigger_events[i];
        super::decimator::trigger_receives(&s.colliders, s.rigidbody, own, rigidbody)
    }

    /// TriggerEventBehaviour.OnTriggerEnter2D: OnTriggerEnter?.Invoke(collision)
    pub fn trigger_event_enter(&mut self, i: usize, collision: i32) {
        if let Some(portal) = self.trigger_events[i].on_enter {
            self.portal_start_passage(portal, collision);
        }
    }

    /// TriggerEventBehaviour.OnTriggerExit2D: OnTriggerExit?.Invoke(collision)
    pub fn trigger_event_exit(&mut self, i: usize, collision: i32) {
        if let Some(portal) = self.trigger_events[i].on_exit {
            self.portal_start_exit(portal, collision);
        }
    }

    /// PortalBehaviour.Start: the two delegates.
    pub fn portal_start(&mut self, i: usize) {
        let (entry, exit) = (self.portals[i].entry, self.portals[i].exit);
        let find = |g: &Game, id: i32| g.trigger_events.iter().position(|t| t.id == id).expect("a TriggerEventBehaviour in the arena");
        let (entry, exit) = (find(self, entry), find(self, exit));
        self.trigger_events[entry].on_enter = Some(i);
        self.trigger_events[exit].on_exit = Some(i);
    }

    /// The physical `Global.main.PhysicalObjectsInWorldByTransform` holds for the transform of `collider`.
    fn portal_physical_of(&self, collider: i32) -> Option<usize> {
        self.physicals.iter().position(|p| !p.destroyed && p.in_world && p.all_colliders.contains(&collider))
    }

    /// GameObject.layer = layer on a physical's object.
    fn physical_set_layer(&mut self, p: usize, layer: i32) {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| g.game_object_set_layer(inst, o, layer));
    }

    /// PortalBehaviour.StartPassage: the ghost (new GameObject, its Transform, then the SpriteRenderer take instance ids),
    /// then GetOrAddComponent<PortalTraversingBehaviour>().enabled = true.
    pub fn portal_start_passage(&mut self, i: usize, obj: i32) {
        let Some(value) = self.portal_physical_of(obj) else { return };
        if self.portals[i].passing_through.contains(&value) {
            return;
        }
        if self.physicals[value].sprite.is_none() {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        self.allocate_instance_ids(3);
        self.portals[i].passing_through.push(value);
        match self.portal_traversings.iter().position(|t| t.physical == value) {
            Some(t) => self.portal_traversing_set_enabled(t, true),
            None => {
                // AddComponent: the instance id, Awake (OriginalLayer = layer), then OnEnable.
                let id = self.allocate_instance_id();
                let id_of = self.physicals[value].id;
                let original_layer = self.with_component_instance(id_of, |g, inst, o| g.world.layers[&inst.objects[o].game_object]);
                self.portal_traversings.push(PortalTraversing { id, physical: value, enabled: true, original_layer });
                self.physical_set_layer(value, PORTAL_LAYER);
            }
        }
    }

    /// The PortalTraversingBehaviour AddComponent put after the prefab's components of object `d`: when its GameObject
    /// turns active its OnEnable puts the object on the Portal layer, when inactive its OnDisable back on the original one
    /// (nothing while the component itself is disabled).
    pub(super) fn portal_traversing_script_pass(&mut self, inst: &mut super::spawn::Instance, d: usize, active: bool) {
        let Some(t) = self.portal_traversings.iter().position(|t| inst.objects[d].components.contains(&self.physicals[t.physical].id)) else { return };
        if !self.portal_traversings[t].enabled {
            return;
        }
        let layer = if active { PORTAL_LAYER } else { self.portal_traversings[t].original_layer };
        self.game_object_set_layer(inst, d, layer);
    }

    /// PortalBehaviour.StartExit: the ghost is destroyed (presentation) and the object's PortalTraversingBehaviour disabled.
    pub fn portal_start_exit(&mut self, i: usize, collision: i32) {
        let Some(value) = self.portal_physical_of(collision) else { return };
        let Some(at) = self.portals[i].passing_through.iter().position(|&p| p == value) else { return };
        self.portals[i].passing_through.remove(at);
        // The "Attempt to disable a PortalTraversingBehaviour" warning otherwise: StartPassage added one.
        let t = self.portal_traversings.iter().position(|t| t.physical == value).expect("the PortalTraversingBehaviour StartPassage added");
        self.portal_traversing_set_enabled(t, false);
    }

    /// PortalTraversingBehaviour.OnEnable and PortalTraversingBehaviour.OnDisable, what `enabled = on` runs: the Portal layer
    /// (the Debug.Log is presentation), or back on the original one.
    pub(crate) fn portal_traversing_set_enabled(&mut self, t: usize, on: bool) {
        if std::mem::replace(&mut self.portal_traversings[t].enabled, on) == on {
            return;
        }
        let p = self.portal_traversings[t].physical;
        let layer = if on { PORTAL_LAYER } else { self.portal_traversings[t].original_layer };
        self.physical_set_layer(p, layer);
    }
}
