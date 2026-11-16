//! CannotShare: an item a save cannot carry (a pasted or loaded copy of it) is swapped for its Substitute asset.
#![allow(non_snake_case)]

use super::Game;
use crate::assets::db::ObjRef;

/// A CannotShare component: Substitute's Prefab and its asset's name.
#[derive(Clone, Debug)]
pub struct CannotShare {
    pub id: i32,
    pub substitute: ObjRef,
    pub name: String,
}

impl Game {
    /// CannotShare.OnAfterDeserialise: the Substitute's Prefab is instantiated at this transform's position and rotation, gets an
    /// AudioSourceTimeScaleBehaviour and a SerialiseInstructions whose OriginalSpawnableAsset is the Substitute (`substitute_name`),
    /// an ObjectCreationAction for it joins the undo history (named like the clone: "<prefab>(Clone)"), and this object is destroyed.
    /// `id` is a component of this object; `substitute_prefab` is Substitute.Prefab.
    pub fn cannot_share_on_after_deserialise(&mut self, id: i32, substitute_prefab: ObjRef, substitute_name: &str) {
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let mut db = self.assets.take().expect("CannotShare.OnAfterDeserialise without the game's data");
        let spawned = db.prefab(substitute_prefab).and_then(|prefab| {
            let (inst, linked) = self.spawn(&mut db, &prefab, position, rotation, None)?;
            // Instantiate names the clone "<prefab name>(Clone)".
            let clone = format!("{}(Clone)", prefab.objects[0].name);
            self.spawnable_asset_components(&prefab, &linked, &clone, 2);
            Ok((inst, clone))
        });
        self.assets = Some(db);
        let (inst, clone) = spawned.unwrap_or_else(|e| panic!("CannotShare.OnAfterDeserialise: Instantiate: {e}"));
        let root = inst.objects[0].components[0];
        self.unsynced.push(inst);
        // SerialiseInstructions.OriginalSpawnableAsset = Substitute: what a save names the object.
        self.object_names.insert(root, substitute_name.to_string());
        self.undo_register_action(super::undo::ObjectCreationAction { displayName: clone, RelevantObjects: vec![root] });
        self.destroy_object(id);
    }
}
