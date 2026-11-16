//! AtomBombBehaviour and SeismicChargeBehaviour: Use, while enabled, instantiates ExplosionPrefab at the object's
//! position and destroys the GameObject; AtomBombBehaviour first counts NUCLEAR_EXPLOSIONS.

use super::Game;
use crate::assets::db::ObjRef;

#[derive(Clone, Debug, Default)]
pub struct PrefabBomb {
    /// The component's instance id.
    pub id: i32,
    pub enabled: bool,
    /// The PhysicalBehaviour on the same GameObject (its Transform).
    pub physical: usize,
    pub ExplosionPrefab: Option<ObjRef>,
    /// AtomBombBehaviour (not SeismicChargeBehaviour).
    pub atom: bool,
}

impl Game {
    /// AtomBombBehaviour.Use / SeismicChargeBehaviour.Use
    pub fn prefab_bomb_use(&mut self, i: usize) {
        let b = self.prefab_bombs[i].clone();
        if b.enabled {
            if b.atom {
                super::stats::with_stats(|s| s.increment("NUCLEAR_EXPLOSIONS", 1.0));
            }
            let source = b.ExplosionPrefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            let position = self.physicals[b.physical].transform.position;
            self.spawn_prefab(source, position);
            self.destroy_object(b.id);
        }
    }
}
