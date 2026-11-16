//! The map options' clear buttons: ClearDebrisBehaviour ("Clear Mess"), ClearLivingBehaviour ("Clear Living") and
//! ClearButtonBehaviour ("Clear Everything").
use super::Game;

impl Game {
    /// DecalControllerBehaviour.Clear of every controller FindObjectsOfType finds (those of GameObjects active in the
    /// hierarchy): localDecalPositions is emptied; the holder objects are rendering.
    pub fn decal_controllers_clear(&mut self) {
        for p in 0..self.physicals.len() {
            if self.physicals[p].active && !self.physicals[p].destroyed {
                for c in &mut self.physicals[p].decal_controllers {
                    // DecalControllerBehaviour.Clear: a dirty controller destroys its decalHolder and is clean again.
                    c.decals.clear();
                    c.holder = None;
                }
            }
        }
    }

    /// ClearDebrisBehaviour.Clear: every DecalControllerBehaviour clears (localDecalPositions; the holder objects are
    /// rendering), every DebrisComponent's GameObject is destroyed and its undo action dropped.
    pub fn clear_debris(&mut self) {
        self.decal_controllers_clear();
        let mut debris = Vec::new();
        for inst in self.unsynced.iter().chain(&self.retained) {
            for (x, o) in inst.objects.iter().enumerate() {
                let key = o.components[0];
                let carried = inst.prefab.objects[x].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "DebrisComponent"));
                if o.active && !self.destroyed_components.contains(&key) && (carried || self.debris_added.contains(&key)) {
                    debris.push(key);
                }
            }
        }
        for key in debris {
            self.destroy_object(key);
            self.undo_deregister_related(key);
        }
        self.notify(crate::il!(str "ClearDebrisBehaviour::Clear" 0).into());
    }

    /// ClearLivingBehaviour.Clear: Object.Destroy(transform.root.gameObject) of every AliveBehaviour (people and drones).
    pub fn clear_living(&mut self) {
        for i in 0..self.people.len() {
            // A person whose root is already queued (all limbs disintegrated) has its Destroy pending: a second one does nothing.
            // No script deactivates a person's root, so every other one is found.
            if self.people[i].destroyed || self.people[i].destroy_at.is_some() {
                continue;
            }
            let Some(&l) = self.people[i].Limbs.first() else { continue };
            let root = self.root_id(self.limbs[l].PhysicalBehaviour);
            self.destroy_object(root);
        }
        for i in 0..self.drones.len() {
            let root = self.root_id(self.drones[i].physical);
            if !self.destroyed_components.contains(&self.drones[i].id) {
                self.destroy_object(root);
            }
        }
        self.notify(crate::il!(str "ClearLivingBehaviour::Clear" 0).into());
    }

    /// ClearButtonBehaviour.ClearEverything -> Global.ClearMap(ToRemove, false).
    pub fn clear_everything(&mut self) {
        let mut db = self.assets.take().expect("ClearButtonBehaviour.ClearEverything without the game's data");
        let mask = Self::read_clear_mask(&mut db);
        self.assets = Some(db);
        self.clear_map(mask.expect("ClearButtonBehaviour.ToRemove"));
        self.notify(crate::il!(str "ClearButtonBehaviour::ClearEverything" 0).into());
    }

    /// Global.ClearMap, Utils.NextFrameCoroutine (mask, resetEnvSettings: false): the old MapConfig's Floodlights become
    /// MapLightBehaviour.StartEnabled and its Ambient_temperature PhysicalBehaviour.AmbientTemperature; every object
    /// pool is cleared; every GameObject active in the hierarchy whose layer is in `mask` is destroyed; every decal
    /// controller, the undo history and the ambient temperature grid are cleared; MapLoaderBehaviour.Load puts the map
    /// back. The NextFrameCoroutine that follows runs at WaitForEndOfFrame, after the Destroys: MapConfig.Instance
    /// (set by Reinitialise, from Start) is then the destroyed old config and `(bool)MapConfig.Instance` is false, so
    /// it copies no settings and touches no lights; EnvironmentSettingsController.Start is the settings window.
    pub fn clear_map(&mut self, mask: u32) {
        if self.map_config_present {
            if let Some(settings) = self.map_settings {
                self.map_light_start_enabled = settings.Floodlights;
                self.AmbientTemperature = settings.Ambient_temperature;
            }
        }
        self.object_pools_clear();
        let mut doomed = Vec::new();
        for inst in self.unsynced.iter().chain(&self.retained) {
            for o in &inst.objects {
                let key = o.components[0];
                if o.active && !self.destroyed_components.contains(&key) && mask >> self.world.layers[&o.game_object] & 1 == 1 {
                    doomed.push(key);
                }
            }
        }
        for key in doomed {
            self.destroy_object(key);
        }
        self.decal_controllers_clear();
        self.undo_clear_history();
        self.ambient_grid.World.clear();
        self.map_loader_load();
    }

    /// MapLoaderBehaviour.Load: the map under WORLD is destroyed and, still being WORLD's child, deactivated (its
    /// OnDisables run now); CurrentMap is instantiated again at the origin.
    pub fn map_loader_load(&mut self) {
        let prefab = self.current_map.clone().expect("MapLoaderBehaviour.CurrentMap: a recording made before the recorder wrote the map's Clone records (re-record it)");
        if let Some(root) = self.map_root {
            self.map_overlays_destroy();
            self.destroy_object(root);
            self.with_component_instance(root, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        let mut db = self.assets.take().expect("MapLoaderBehaviour.Load without the game's data");
        let loaded = self.load_map(&mut db, &prefab);
        self.assets = Some(db);
        let (inst, _) = loaded.unwrap_or_else(|e| panic!("MapLoaderBehaviour.InstantiateMap: {e}"));
        self.map_root = Some(inst.objects[0].components[0]);
        self.unsynced.push(inst);
        self.mod_events_clear_listeners();
        // ModLoaderRegistry.Loaders' OnLoadMap: the LuaModLoader's (EmptyModLoader.OnLoadMap) is empty.
    }
}
