//! ep (the explosion egg a hoverbox-driven LaserBehaviour instantiates): Update counts t up over `duration`, FixedUpdate
//! shakes the camera (presentation) and, with a chance that grows with t, toggles every glowtube and bulb in the world,
//! and End explodes, sends OnEMPHit to everything, deactivates itself and instantiates its ToSpawn asset. Save is the item
//! persistence file and a Steam stat.
#![allow(non_snake_case)]

use super::explosion::{EffectSize, ExplosionParameters};
use super::{d, script, Game};
use crate::assets::db::ObjRef;

script! {
    Ep {
        duration: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The private `t`.
        t: f32 = 0.0,
        /// ToSpawn: the SpawnableAsset's Prefab and name.
        to_spawn: Option<(ObjRef, String)> = None,
    }
}

impl Game {
    /// ep.Update
    pub fn ep_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let x = &mut self.eps[i];
        x.t += dt / x.duration;
        if x.t >= 1.0 {
            self.ep_end(i);
        }
    }

    /// ep.FixedUpdate: CameraShakeBehaviour.Shake is the camera's; then, past the roll, each world object that rolls
    /// under 0.5 has its GlowtubeBehaviour and BulbBehaviour Used.
    pub fn ep_fixed_update(&mut self, i: usize) {
        let t = self.eps[i].t;
        if !(d(self.random.value()) > 0.9900000095367432 - d(t) * 0.15) {
            return;
        }
        for p in self.physicals_in_world() {
            if !(self.random.value() as f64 > 0.5) {
                if let Some(g) = self.glowtubes.iter().position(|x| x.Phys == p) {
                    self.glowtube_use(g);
                }
                if let Some(b) = self.bulbs.iter().position(|x| x.Phys == p) {
                    self.bulb_use(b);
                }
            }
        }
    }

    /// ep.End: Explode, OnEMPHit to every physical object in the world, SetActive(false), then Instantiate(ToSpawn.Prefab)
    /// with AudioSourceTimeScaleBehaviour and SerialiseInstructions added, and Save.
    pub(crate) fn ep_end(&mut self, i: usize) {
        let id = self.eps[i].id;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        self.explosion_creator_explode(ExplosionParameters {
            FragmentationRayCount: 32,
            Position: position,
            Range: 30.0,
            FragmentForce: 3500.0,
            CreateParticlesAndSound: true,
            EffectSize: EffectSize::Large,
            DismemberChance: 0.0,
            BallisticShrapnelCount: 0,
        });
        for p in self.physicals_in_world() {
            self.emp_send_on_hit(p);
        }
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        let (source, _) = self.eps[i].to_spawn.clone().unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let mut db = self.assets.take().expect("ep without the game's data");
        let spawned = db.prefab(source).and_then(|prefab| {
            let (inst, linked) = self.spawn(&mut db, &prefab, position, [0.0, 0.0, 0.0, 1.0], None)?;
            // Instantiate names the clone "<prefab name>(Clone)"; nothing renames it.
            let clone = format!("{}(Clone)", prefab.objects[0].name);
            self.spawnable_asset_components(&prefab, &linked, &clone, 2);
            Ok(inst)
        });
        self.assets = Some(db);
        let inst = spawned.unwrap_or_else(|e| panic!("ep.End: Instantiate: {e}"));
        self.unsynced.push(inst);
        // ep.Save: ItemPersistence.Add(ToSpawn.name), then the KEYPAD stat.
        let name = self.eps[i].to_spawn.as_ref().map(|(_, n)| n.clone()).unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        crate::ui::data::persistence_add(&name);
        super::stats::increment_integer(super::stats::KEYPAD, 1);
    }
}
