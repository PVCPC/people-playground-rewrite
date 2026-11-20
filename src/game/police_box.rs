//! PoliceBoxBehaviour: the Hybrid map's "policebox" (ExistByChance 0.01 keeps it). When its SpriteRenderer becomes
//! visible it fades from white to clear over Duration, then its GameObject goes off.
#![allow(non_snake_case)]

use super::coroutine::Yield;
use super::visual::SpriteAt;
use super::Game;

#[derive(Clone, Debug)]
pub struct PoliceBox {
    /// The component's instance id.
    pub id: i32,
    /// SpriteRenderer (serialised).
    pub SpriteRenderer: Option<SpriteAt>,
    pub Duration: f32,
    pub isBusy: bool,
    /// The renderer's isVisible after the last render (OnBecameVisible on false -> true).
    pub visible: bool,
}

impl Default for PoliceBox {
    fn default() -> Self {
        Self { id: 0, SpriteRenderer: None, Duration: 1.0, isBusy: false, visible: false }
    }
}

impl Game {
    /// The render after LateUpdate: each box's renderer.isVisible (drawn by this render), and OnBecameVisible when it
    /// turns visible.
    pub fn police_boxes_will_render(&mut self) {
        for b in 0..self.police_boxes.len() {
            let Some(at) = self.police_boxes[b].SpriteRenderer else { continue };
            let drawn = self.sprite_drawn(at) && !self.destroyed_components.contains(&self.police_boxes[b].id);
            if !std::mem::replace(&mut self.police_boxes[b].visible, drawn) && drawn {
                self.police_box_on_became_visible(b);
            }
        }
    }

    /// PoliceBoxBehaviour.OnBecameVisible: presentation (unless busy, StartCoroutine(FadeRoutine())).
    pub fn police_box_on_became_visible(&mut self, b: usize) {
        if !self.police_boxes[b].isBusy {
            let id = self.police_boxes[b].id;
            self.start_coroutine(id, id, Self::police_box_fade_routine(b));
        }
    }

    /// PoliceBoxBehaviour.FadeRoutine: presentation (isBusy = true; each frame t += deltaTime and SpriteRenderer.color =
    /// Color.Lerp(white, clear, t / Duration), WaitForEndOfFrame until t > Duration, then gameObject.SetActive(false)).
    fn police_box_fade_routine(b: usize) -> super::coroutine::Routine {
        let mut t = 0.0f32;
        Box::new(move |g: &mut Game| {
            let x = &mut g.police_boxes[b];
            x.isBusy = true;
            let (id, at, duration) = (x.id, x.SpriteRenderer, x.Duration);
            t += g.time.deltaTime;
            // Color.Lerp(a, b, t): a + (b - a) * Clamp01(t); white to (1, 1, 1, 0) moves only alpha.
            let k = super::mathf::clamp01(t / duration);
            let at = at.unwrap_or_else(|| super::menu::throw("NullReferenceException: PoliceBoxBehaviour.SpriteRenderer is null"));
            g.set_sprite_color(at, [1.0, 1.0, 1.0, 1.0 + (0.0 - 1.0) * k]);
            if t > duration {
                g.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
                return Yield::Done;
            }
            Yield::WaitForEndOfFrame
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::db::AssetDb;
    use crate::assets::mono::Value;

    #[test]
    fn hybrid_police_box_fades_then_goes_off() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let mut map = None;
        for m in db.find_scripts("Map").unwrap() {
            let (_, mb, v) = db.mono(m).unwrap().unwrap();
            if mb.name == "Hybrid" {
                map = v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p));
            }
        }
        let prefab = db.prefab(map.expect("the Hybrid map")).unwrap();
        let mut g = Game::load_main(&mut db, &game).unwrap();
        g.assets = Some(Box::new(AssetDb::open(&game).unwrap()));
        let (inst, _) = g.load_map(&mut db, &prefab).unwrap();
        g.unsynced.push(inst);
        g.run_pending_starts();
        assert_eq!(g.police_boxes.len(), 1);
        let (id, at) = (g.police_boxes[0].id, g.police_boxes[0].SpriteRenderer.unwrap());
        assert_eq!(g.police_boxes[0].Duration, 1.5);
        // Forced to exist: ExistByChance.Start (Chance 0.01) most likely turned it off.
        g.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, true));
        g.input.view = None;
        g.time.deltaTime = 0.02;
        // The first step runs inside OnBecameVisible (the render); its WaitForEndOfFrame resumes at this same frame's
        // TriggerEndOfFrameCallbacks, then once a frame.
        g.police_boxes_will_render();
        let mut t = 0.02f32;
        for frame in 0.. {
            assert_eq!(g.sprite_color(at), [1.0, 1.0, 1.0, 1.0 - (t / 1.5).min(1.0)], "step {frame}");
            assert!(g.police_boxes[0].isBusy && frame < 100);
            if t > 1.5 {
                assert!(!g.sprite_drawn(at), "off after the fade");
                break;
            }
            assert!(g.sprite_drawn(at));
            g.end_of_frame_calls();
            t += 0.02;
            g.police_boxes_will_render();
        }
        // Off: never visible again, and no coroutine left.
        g.end_of_frame_calls();
        g.police_boxes_will_render();
        assert!(!g.police_boxes[0].visible && g.end_of_frame.is_empty());
    }
}
