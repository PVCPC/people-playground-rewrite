//! wh (the Wh prefab WhOpening.End makes): once it has been drawn for ten seconds in a row it replaces the chamber with the
//! Uncharted map. Its renderer callbacks (OnWillRenderObject, OnBecameInvisible) are called with the renderer's visibility.
#![allow(non_snake_case)]

use super::Game;
use crate::assets::db::ObjRef;

/// A wh component: OnLiftoff is the unity event `invoke_event(id, "OnLiftoff")` runs.
#[derive(Clone, Debug, Default)]
pub struct Wh {
    pub id: i32,
    /// Uncharted: the GameObject the replacing map instantiates.
    pub Uncharted: Option<ObjRef>,
    /// The index of its ParticleSystem in `embedded_ps` (its renderer).
    pub system: usize,
    pub(crate) t: f32,
    pub(crate) liftoff: bool,
    /// The renderer was drawn last frame (OnBecameInvisible fires on the change).
    visible: bool,
}

impl Wh {
    /// wh.OnBecameInvisible: t = 0f.
    pub fn on_became_invisible(&mut self) {
        self.t = 0.0;
    }

    /// wh.OnWillRenderObject: t += Time.deltaTime.
    pub fn on_will_render_object(&mut self, delta_time: f32) {
        self.t += delta_time;
    }
}

impl Game {
    /// The renderer callbacks of every wh for the frame's render: OnWillRenderObject while the ParticleSystemRenderer is drawn,
    /// OnBecameInvisible on the frame it stops being drawn.
    pub fn whs_will_render(&mut self) {
        for i in 0..self.whs.len() {
            let drawn = crate::particles::embedded_drawn(self, self.whs[i].system);
            let dt = self.time.deltaTime;
            let w = &mut self.whs[i];
            if drawn {
                w.on_will_render_object(dt);
            } else if w.visible {
                w.on_became_invisible();
            }
            w.visible = drawn;
        }
    }

    /// wh.Update: past ten seconds of being drawn, once: OnLiftoff, the WDJZM stat, the ambient temperature grid and the undo history
    /// cleared, MapLoaderBehaviour.CurrentMap = a Map whose InstantiateOverride is Object.Instantiate(Uncharted, parent) (the
    /// Uncharted root sits at the origin unscaled, so it is `load_map`'s placement; the new Map has no asset), then
    /// ClearButtonBehaviour.ClearEverything.
    pub fn wh_update(&mut self, i: usize) {
        if self.whs[i].t > 10.0 && !self.whs[i].liftoff {
            self.whs[i].liftoff = true;
            let (id, uncharted) = (self.whs[i].id, self.whs[i].Uncharted);
            self.invoke_event(id, "OnLiftoff");
            super::stats::increment_integer(super::stats::WDJZM, 1);
            // if ((bool)AmbientTemperatureGridBehaviour.Instance): the grid is always awake here.
            self.ambient_grid.World.clear();
            self.undo_clear_history();
            let uncharted = uncharted.unwrap_or_else(|| unimplemented!("mod-only: wh.Update: Uncharted is null (Instantiate throws ArgumentException when the map loads)"));
            let mut db = self.assets.take().expect("wh.Update without the game's data");
            let prefab = db.prefab(uncharted);
            self.assets = Some(db);
            self.current_map = Some(prefab.unwrap_or_else(|e| panic!("wh.Update: Uncharted: {e}")));
            self.current_map_asset = None;
            self.clear_everything();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_time_counts_and_resets() {
        let mut w = Wh::default();
        w.on_will_render_object(0.25);
        w.on_will_render_object(0.25);
        assert_eq!(w.t, 0.5);
        w.on_became_invisible();
        assert_eq!(w.t, 0.0);
    }

    #[test]
    fn update_before_ten_seconds_does_nothing() {
        let mut g = Game::default();
        g.whs.push(Wh::default());
        g.whs[0].on_will_render_object(9.0);
        g.wh_update(0);
        assert!(!g.whs[0].liftoff);
    }
}
