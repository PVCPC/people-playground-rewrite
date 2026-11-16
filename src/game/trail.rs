//! TrailRenderers the game draws, as world-space lines of `Game::lines` (src/lines.rs meshes them with the trail's own
//! width curve, gradient and material). Rendering only: nothing the simulation reads comes from here (a script that
//! needs a trail's points or its autodestruct models them itself, as archelix.rs and blaster.rs do).
//!
//! The native renderer's update after the frame's scripts (PostLateUpdate): points older than m_Time go, an emitting
//! trail adds its Transform's position once that is m_MinVertexDistance from the last point, and the line runs from the
//! Transform (the curves' start) through the points, newest first. A trail whose component is gone frees its line.

use super::{BareAnchor, Game};
use crate::assets::components::TrailRendererData;
use crate::assets::db::AssetDb;

#[derive(Clone, Debug, Default)]
pub struct Trail {
    /// The TrailRenderer's instance id (its GameObject's Transform is followed); None once it is gone.
    pub owner: Option<i32>,
    /// Its line in `Game::lines`.
    pub line: usize,
    /// m_Time, m_MinVertexDistance and m_Emitting.
    pub time: f32,
    pub min_vertex_distance: f32,
    pub emitting: bool,
    /// The points: (time added, position), oldest first.
    pub points: Vec<(f64, [f32; 3])>,
}

impl Game {
    /// TrailRenderer `data` (component `id` of a clone from `file`) starts drawing; a gone trail's line is reused.
    pub fn trail_add(&mut self, db: &AssetDb, file: usize, data: TrailRendererData, id: i32, game_layer: i32, visible: bool) {
        let free = self.trails.iter().position(|t| t.owner.is_none());
        // Its own line id: not a LineRenderer a script could Destroy (tracer lines are below i32::MIN / 2 + n).
        let line_id = |l: usize| i32::MIN / 4 + l as i32;
        let mut line = Self::line_visual(db, file, data.line, 0, BareAnchor::Root, Vec::new(), game_layer, visible);
        let trail = Trail { owner: Some(id), line: 0, time: data.time, min_vertex_distance: data.min_vertex_distance, emitting: data.emitting, points: Vec::new() };
        match free {
            Some(t) => {
                let l = self.trails[t].line;
                line.id = line_id(l);
                self.lines[l] = line;
                self.trails[t] = Trail { line: l, ..trail };
            }
            None => {
                let l = self.lines.len();
                line.id = line_id(l);
                self.lines.push(line);
                self.trails.push(Trail { line: l, ..trail });
            }
        }
    }

    /// The renderers' update of every trail (after the frame's scripts and delayed Destroys).
    pub fn trails_update(&mut self) {
        let now = self.time.timeAsDouble;
        for t in 0..self.trails.len() {
            let Some(id) = self.trails[t].owner else { continue };
            let l = self.trails[t].line;
            let at = if self.destroyed_components.contains(&id) { None } else { self.component_at(id).map(|(inst, o, _)| (inst.position(o), inst.objects[o].active)) };
            let Some((p, active)) = at else {
                self.trails[t].owner = None;
                self.trails[t].points.clear();
                self.lines[l].positions.clear();
                self.lines[l].visible = false;
                continue;
            };
            let tr = &mut self.trails[t];
            let life = tr.time as f64;
            tr.points.retain(|&(at, _)| now - at <= life);
            let far = |q: &[f32; 3]| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() >= tr.min_vertex_distance;
            if active && tr.emitting && tr.points.last().is_none_or(|(_, q)| far(q)) {
                tr.points.push((now, p));
            }
            let positions = if tr.points.is_empty() { Vec::new() } else { std::iter::once(p).chain(tr.points.iter().rev().map(|x| x.1)).collect() };
            self.lines[l].positions = positions;
            self.lines[l].visible = active;
        }
    }
}

