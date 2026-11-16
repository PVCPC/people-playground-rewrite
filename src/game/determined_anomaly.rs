//! DeterminedAnomaly: an object that remembers where it was placed and, once it has been moved more than a unit
//! away and out of sight (and then its start is off screen), is replaced by a fresh clone of its Prefab. Start keeps
//! the pose; Update needs the camera's aspect and the renderer's isVisible, which a headless run does not have.
#![allow(non_snake_case)]

use super::visual::SpriteAt;
use super::{script, Game};
use crate::assets::db::ObjRef;

script! {
    DeterminedAnomaly {
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// The GameObject's PhysicalBehaviour (`ActiveSingleSelected.gameObject == base.gameObject`).
        physical: Option<usize> = None,
        /// position, rotation and scale (localScale) as Start keeps them.
        position: [f32; 3] = [0.0; 3],
        rotation: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
        scale: [f32; 3] = [1.0; 3],
        sentBack: bool = false,
        /// Prefab: the GameObject (here a child of the Reactor A5 prefab) Instantiate clones.
        Prefab: Option<ObjRef> = None,
        /// spriteRenderer.isVisible: drawn by the last render (`determined_anomalies_will_render`).
        isVisible: bool = false,
    }
}

impl Game {
    /// DeterminedAnomaly.Start: position, rotation and localScale of the transform (the SpriteRenderer is only read in Update).
    pub fn determined_anomaly_start(&mut self, a: usize) {
        let id = self.determined_anomalies[a].id;
        let (position, rotation, scale) = self.with_component_instance(id, |_, inst, o| {
            let s = inst.objects[o].trs.s;
            (inst.position(o), inst.rotation(o), [s[0], s[1], s[2]])
        });
        let x = &mut self.determined_anomalies[a];
        x.position = position;
        x.rotation = rotation;
        x.scale = scale;
    }

    /// DeterminedAnomaly.Update: the selected object is left alone; once sentBack it is replaced when its start leaves the
    /// viewport (-0.1 to 1.1), else it is sent back when it lies over a unit away and its renderer is not visible.
    pub fn determined_anomaly_update(&mut self, a: usize) {
        let x = &self.determined_anomalies[a];
        // currentTool && currentTool.ActiveSingleSelected && its gameObject == base.gameObject
        if let (Some(selected), Some(own)) = (self.wire_selected(), x.physical) {
            if selected == own {
                return;
            }
        }
        let (id, position) = (x.id, x.position);
        if x.sentBack {
            // Camera.WorldToViewportPoint of the orthographic main camera: the position's place in its view.
            let (min, max) = self.input.view.unwrap_or_else(|| unimplemented!("mod-only: DeterminedAnomaly.Update: Camera.WorldToViewportPoint without a camera view"));
            let (vx, vy) = ((position[0] - min[0]) / (max[0] - min[0]), (position[1] - min[1]) / (max[1] - min[1]));
            if vx > 1.1 || vx < -0.1 || vy > 1.1 || vy < -0.1 {
                self.determined_anomaly_replace(a);
            }
            return;
        }
        let now = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let (dx, dy, dz) = (now[0] - position[0], now[1] - position[1], now[2] - position[2]);
        let sqr = (super::d(dx) * dx + super::d(dy) * dy + super::d(dz) * dz).f();
        // spriteRenderer.isVisible: a headless run (no view) draws every renderer.
        let visible = self.input.view.is_none() || self.determined_anomalies[a].isVisible;
        if sqr > 1.0 && !visible {
            self.determined_anomalies[a].sentBack = true;
        }
    }

    /// DeterminedAnomaly.Update's replacement: Instantiate(Prefab) at the kept position, localScale and rotation, then
    /// Object.Destroy(gameObject). The fresh clone's Rigidbody2D already has zero velocities (the setters change nothing).
    fn determined_anomaly_replace(&mut self, a: usize) {
        let x = &self.determined_anomalies[a];
        let (id, position, rotation, scale) = (x.id, x.position, x.rotation, x.scale);
        let prefab = x.Prefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let mut db = self.assets.take().expect("DeterminedAnomaly.Update: Instantiate without the game's data");
        let spawned = db.prefab(prefab).and_then(|p| self.spawn(&mut db, &p, position, rotation, None));
        self.assets = Some(db);
        let (mut inst, linked) = spawned.unwrap_or_else(|e| panic!("DeterminedAnomaly.Update: Instantiate: {e}"));
        inst.set_local_scale(0, scale);
        self.refresh_transforms(&inst, &linked);
        self.unsynced.push(inst);
        self.destroy_object(id);
    }

    /// The render after LateUpdate: spriteRenderer.isVisible of each anomaly (drawn by this render).
    pub fn determined_anomalies_will_render(&mut self) {
        for a in 0..self.determined_anomalies.len() {
            if let Some(p) = self.determined_anomalies[a].physical {
                let drawn = self.sprite_drawn(SpriteAt::Physical(p));
                self.determined_anomalies[a].isVisible = drawn;
            }
        }
    }
}
