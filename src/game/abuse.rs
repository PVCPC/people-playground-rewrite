//! PP_ABUSE (play::test): the damage, copy, save and delete paths real play uses, applied to a spawned item. Each action
//! calls the function the real weapon, explosion, tool or menu calls; nothing here is game behaviour of its own.
use super::explosion::{EffectSize, ExplosionParameters};
use super::Game;

impl Game {
    /// The physical's rigidbody still has its body (a disintegrated or moved-away object has none).
    pub fn body_alive(&self, q: usize) -> bool {
        self.world.bodies.get(self.physicals[q].rigidbody.body).is_some_and(Option::is_some)
    }

    /// One abuse action on the physical objects `range` of the item. Destroyed ones are skipped.
    pub fn abuse_action(&mut self, action: &str, range: std::ops::Range<usize>) {
        let live: Vec<usize> = range.filter(|&q| q < self.physicals.len() && !self.physicals[q].destroyed && self.body_alive(q)).collect();
        let centre = |g: &Game, q: usize| {
            let c = g.world.body(g.physicals[q].rigidbody.body).sweep.c;
            [c.x, c.y]
        };
        for &q in &live {
            if self.physicals[q].destroyed || !self.body_alive(q) {
                continue;
            }
            let c = centre(self, q);
            let collider = self.physicals[q].all_colliders.first().copied();
            match action {
                // A bullet (Firearm: Shot of 25 at the hit point, then the same Shot's ExitShot).
                "shot" => if let Some(k) = collider { self.send_shot(k, Some(q), [0.0, 1.0], c, 25.0, false, None) },
                "exit_shot" => if let Some(k) = collider { self.send_shot(k, Some(q), [0.0, 1.0], c, 25.0, true, None) },
                // A fragment of an explosion (ExplosionCreator: SendMessage OnFragmentHit with the fragment force).
                "fragment" => if let Some(k) = collider { self.send_on_fragment_hit(k, 12.0) },
                // Joint/collision breaking (SendMessage Break with the colliding velocity).
                "break" => if let Some(k) = collider { self.send_break(k, Some(q), [0.0, 8.0]) },
                "slice" => self.send_slice(q),
                // Electricity: Shocked from a zap, then PhysicalBehaviour.Charge flows through the item.
                "shock" => self.physical_shocked(q, c),
                "emp" => self.emp_send_on_hit(q),
                "ignite" => self.ignite(q, true),
                "freeze" => self.freeze_add_component(q),
                "explode" => self.create_explosion_with_water(false, ExplosionParameters::new(32, [c[0], c[1], 0.0], 6.0, 12.0, true, EffectSize::Medium, 0.5, 0)),
                "charge" | "flip" | "copy_paste_undo" | "contraption" | "delete_undo" | "clear_map" => {}
                _ => panic!("PP_ABUSE: no action {action}"),
            }
        }
        // Selection actions: the selection holds the item's selectable objects, as a click or a drag box would.
        let centre3 = live.first().map(|&q| centre(self, q)).map_or([0.0; 3], |c| [c[0], c[1], 0.0]);
        let select = |g: &mut Game| {
            g.tool.as_mut().unwrap().rawSelection.clear();
            for &q in &live {
                g.select(q, true);
            }
        };
        match action {
            // A jukebox (started by the run's activateDirect press, play.rs) with the Charge an energy wire would feed it: above 100 it
            // shakes the camera and hurts ears.
            "charge" => if let (Some(&q), false) = (live.first(), self.jukeboxes.is_empty()) { self.physicals[q].charge = 500.0 },
            "flip" => { select(self); self.flip_selection(); }
            // DragTool copy and paste (Ctrl+C, Ctrl+V), then Ctrl+Z.
            "copy_paste_undo" => {
                select(self);
                let sel = self.tool.as_ref().unwrap().rawSelection.clone();
                self.clipboard_copy(&sel, centre3);
                self.clipboard_paste([centre3[0] + 2.0, centre3[1], 0.0]);
                self.undo();
            }
            // Contraption save and load without the file: the same serialise/deserialise and paste the dialogs use.
            "contraption" => {
                select(self);
                let sel = self.tool.as_ref().unwrap().rawSelection.clone();
                let live_sel: Vec<usize> = sel.into_iter().filter(|&p| !self.physicals[p].destroyed).collect();
                let states = self.object_states_of(&live_sel, centre3);
                let bytes = super::contraption::serialise_states(&states).unwrap_or_else(|e| panic!("contraption save: {e}"));
                let back = super::contraption::deserialise_states(&bytes).unwrap_or_else(|e| panic!("contraption load: {e}"));
                self.paste_states(&back, [centre3[0] - 2.0, centre3[1], 0.0], false, "Load");
            }
            // The delete key, then undo.
            "delete_undo" => { select(self); self.user_delete_selection(); self.undo(); }
            // Global.ClearMap with every layer.
            "clear_map" => self.clear_map(u32::MAX),
            _ => {}
        }
    }
}
