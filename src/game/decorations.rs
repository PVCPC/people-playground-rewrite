//! JackOLaternBehaviour and TargetLineRenderer: the lantern's sprites are presentation; TargetLineRenderer's line is
//! drawn from `Game::lines` (Awake adds it there when no other script has). A missing reference still throws where the
//! game would touch it.
#![allow(non_snake_case)]

use super::translated::rt::{self, Xf};
use super::{script, Game};

script! {
    JackOLaternBehaviour {
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        GlowingSprites: bool = false,
        ObjectSprite: bool = false,
        LightSprite: bool = false,
    }
}

script! {
    TargetLineRenderer {
    }
    links {
        /// The component's instance id.
        this_id: i32 = 0,
        /// Behaviour.enabled.
        enabled: bool = false,
        /// The PhysicalBehaviour on its GameObject (its Transform's pose).
        this_physical: Option<usize> = None,
        /// Its GameObject or a descendant has a Collider2D or Rigidbody2D.
        this_physics: bool = false,
        LineRenderer: bool = false,
        Target: Option<Xf> = None,
    }
}

impl Game {
    /// TargetLineRenderer.Awake/Update: Awake sets useWorldSpace = false, positionCount = 2 and point 0 at the origin;
    /// both set point 1 at transform.InverseTransformPoint(Target.position). The points are read back
    /// (PhysicalRopeCreator.CreateRope's GetPosition on the Reactor A5 wires), so the line in `Game::lines` holds them.
    pub(crate) fn target_line_renderer_hand(&mut self, i: usize, awake: bool) {
        let t = &self.tr.target_line_renderer[i];
        if !awake && !t.enabled {
            return;
        }
        let (id, present, target) = (t.this_id, t.LineRenderer, t.Target);
        // LineRenderer (the one the field names, on whichever object): Awake's useWorldSpace, positionCount and point 0.
        let mut l = self.present_ids.get(&(id, "LineRenderer")).and_then(|lid| self.lines.iter().position(|x| x.id == *lid));
        if awake && l.is_none() {
            l = self.present_ids.get(&(id, "LineRenderer")).copied().and_then(|lid| self.target_line_add(lid));
        }
        if awake {
            rt::pres(present);
            if let Some(l) = l {
                self.lines[l].use_world_space = false;
                self.lines[l].positions = vec![[0.0; 3], [0.0; 3]];
            }
        }
        // Target.position (any Transform: by its id), then base.transform.InverseTransformPoint of it.
        let at = match rt::nn(target) {
            Xf::Physical(p) => self.physicals[p].transform.position,
            _ => {
                let tid = *self.present_ids.get(&(id, "Target")).expect("TargetLineRenderer.Target's Transform id");
                self.with_component_instance(tid, |g, inst, o| {
                    let (a, c) = g.object_anchor(inst, o);
                    g.anchored_world(a, &c).0
                })
            }
        };
        let own = self.with_component_instance(id, |g, inst, o| g.object_anchor(inst, o));
        let mut v = [at[0], at[1], at[2], 0.0];
        for node in self.anchored_nodes(own.0, &own.1).iter().rev() {
            v = crate::unity::transform::inverse(node, v, [0.0, 0.0, 0.0, 1.0]).0;
        }
        rt::pres(present);
        // LineRenderer.SetPosition past positionCount only logs an error.
        if let Some(p) = l.and_then(|l| self.lines[l].positions.get_mut(1)) {
            *p = [v[0], v[1], v[2]];
        }
    }

    /// Rendering: the LineRenderer `lid` of a clone, added to `Game::lines` (None: not a LineRenderer in a clone).
    fn target_line_add(&mut self, lid: i32) -> Option<usize> {
        use crate::assets::db::Component;
        let (inst, o, k) = self.component_at(lid)?;
        if !matches!(inst.prefab.objects[o].components[k].1, Component::Other(120)) {
            return None;
        }
        let (r, layer, active) = (inst.prefab.objects[o].components[k].0, inst.prefab.objects[o].layer as i32, inst.objects[o].active);
        let (anchor, chain) = self.object_anchor(inst, o);
        let db = self.assets.take()?;
        let data = crate::assets::components::LineRendererData::read(db.data(r));
        let l = data.ok().map(|data| self.add_line(&db, r.file, data, lid, anchor, chain, layer, active));
        self.assets = Some(db);
        l
    }

    /// JackOLaternBehaviour.Start
    pub fn jack_o_latern_behaviour_start(&mut self, j: usize) {
        self.jack_o_latern_behaviour_refresh(j);
    }

    /// JackOLaternBehaviour.Refresh: picks the face sprites (ObjectSprite, GlowingSprites, LightSprite).
    pub fn jack_o_latern_behaviour_refresh(&mut self, j: usize) {
        let s = &self.tr.jack_o_latern_behaviour[j];
        rt::pres(s.ObjectSprite);
        rt::pres(s.GlowingSprites);
        rt::pres(s.LightSprite);
    }

    /// TargetLineRenderer.Update
    pub fn target_line_renderer_update(&mut self, t: usize) {
        self.target_line_renderer_hand(t, false);
    }
}
