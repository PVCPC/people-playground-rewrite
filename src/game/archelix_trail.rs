//! Rendering only: the Archelix bolt's TrailRendererToDetach drawn as a world-space line of `Game::lines` (src/lines.rs
//! meshes it with the trail's own width curve, gradient and material). The points are the ones archelix.rs models for
//! the despawn wait; the line runs from the bolt's Transform (the curves' start) through them, newest first.
//! Self-contained so that a generic TrailRenderer can take it over.

use super::Game;
use crate::assets::components::LineRendererData;
use crate::assets::serialized::Reader;

/// A TrailRenderer (class 96) as a LineRenderer's data: its m_Parameters are a LineRenderer's. The bytes are laid out as
/// a LineRenderer's: m_Time (4 bytes) becomes an empty m_Positions, and the tail (m_MinVertexDistance, m_Autodestruct,
/// m_Emitting) becomes useWorldSpace true, loop false.
pub(crate) fn trail_line_data(data: &[u8]) -> Result<LineRendererData, String> {
    let r = &mut Reader::new(data);
    // The Renderer head (components.rs renderer_head): m_GameObject, m_Enabled and 8 flag bytes, the rendering layer
    // mask, priority, lightmap index, two lightmap ST vectors, m_Materials, the static batch info, three PPtrs, the
    // sorting layer id, layer and order.
    r.bytes(12 + 1 + 8)?;
    r.align();
    r.bytes(4 + 4 + 4 + 32)?;
    let n = r.i32()? as usize;
    r.bytes(12 * n + 4 + 36 + 4 + 2 + 2)?;
    r.align();
    let head = r.pos;
    if data.len() < head + 4 + 6 {
        return Err("TrailRenderer: too short".into());
    }
    let mut fake = data[..head].to_vec();
    fake.extend_from_slice(&0i32.to_le_bytes());
    fake.extend_from_slice(&data[head + 4..data.len() - 6]);
    fake.extend_from_slice(&[1, 0]);
    LineRendererData::read(&fake)
}

impl Game {
    /// After `archelix_trails_update`: each bolt's line shows its Transform and trail points while the bolt is active
    /// and the trail has points.
    pub fn archelix_trail_lines(&mut self) {
        for b in 0..self.archelix_bolts.len() {
            let Some(l) = self.archelix_bolts[b].trail_line else { continue };
            let id = self.archelix_bolts[b].id;
            let shown = !self.archelix_bolts[b].trail.is_empty() && !self.destroyed_components.contains(&id) && self.tr_active_in_hierarchy(super::translated::rt::Go::Local(id));
            if !shown {
                self.lines[l].visible = false;
                continue;
            }
            let at = self.with_component_instance(id, |_, inst, o| inst.position(o));
            let mut positions = vec![at];
            positions.extend(self.archelix_bolts[b].trail.iter().rev().map(|&(_, p)| [p[0], p[1], 0.0]));
            let line = &mut self.lines[l];
            line.positions = positions;
            line.visible = true;
        }
    }
}
