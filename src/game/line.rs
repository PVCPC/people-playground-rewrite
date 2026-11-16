//! LineRenderers the game draws (src/lines.rs): the Renderer's state a script reads or writes (enabled, positionCount,
//! SetPositions). A script's link adds its line with `add_line`; the renderer side meshes it every frame.
//! Rows closed through it must read its gradient and width curve as the line draws them: `add_line` stops
//! (`unimplemented!`) at a weighted or non-clamping width curve.
#![allow(non_snake_case)]

use super::visual::Gradient;
use super::{BareAnchor, Game};
use crate::assets::components::LineRendererData;
use crate::assets::db::{AssetDb, ObjRef};
use crate::unity::transform::Trs;

#[derive(Clone, Debug)]
pub struct LineVisual {
    /// The LineRenderer component's instance id.
    pub id: i32,
    pub anchor: BareAnchor,
    pub chain: Vec<Trs>,
    /// LineRenderer.positionCount positions (m_Positions).
    pub positions: Vec<[f32; 3]>,
    /// m_Parameters.widthMultiplier
    pub width: f32,
    /// m_Parameters.widthCurve
    pub curve: crate::wires::WidthCurve,
    /// m_Parameters.colorGradient
    pub gradient: Gradient,
    pub material: Option<ObjRef>,
    pub layer: i32,
    pub order: i16,
    pub game_layer: i32,
    pub cap_vertices: usize,
    /// numCornerVertices, alignment (0 View, 1 Local) and loop: Build3DLine's BuildSmoothCorner and loop geometry (`wires::line_mesh_full`).
    pub corner_vertices: i32,
    pub alignment: i32,
    pub looped: bool,
    /// LineTextureMode
    pub texture_mode: i32,
    pub use_world_space: bool,
    /// Renderer.enabled
    pub enabled: bool,
    /// GameObject.activeInHierarchy
    pub visible: bool,
}

impl Game {
    /// Adds the LineRenderer `data` of component `id`; returns its index in `lines`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_line(&mut self, db: &AssetDb, file: usize, data: LineRendererData, id: i32, anchor: BareAnchor, chain: Vec<Trs>, game_layer: i32, visible: bool) -> usize {
        self.lines.push(Self::line_visual(db, file, data, id, anchor, chain, game_layer, visible));
        self.lines.len() - 1
    }

    /// The LineVisual of the LineRenderer `data` (a prefab's line that a script instantiates later is kept as one).
    #[allow(clippy::too_many_arguments)]
    pub fn line_visual(db: &AssetDb, file: usize, data: LineRendererData, id: i32, anchor: BareAnchor, chain: Vec<Trs>, game_layer: i32, visible: bool) -> LineVisual {
        if data.width_weighted.iter().any(|&w| w != 0) || data.width_infinity != [2, 2] {
            unimplemented!("mod-only: LineRenderer with a weighted width curve or one that does not clamp beyond its ends");
        }
        let (keys, ctime, atime, mode, nc, na) = data.gradient;
        let gradient = Gradient::from_raw(keys, ctime, atime, mode, nc, na);
        let material = data.materials.first().and_then(|&m| db.resolve(file, m));
        LineVisual {
            id,
            anchor,
            chain,
            positions: data.positions,
            width: data.width_multiplier,
            curve: crate::wires::WidthCurve(data.width_keys),
            gradient,
            material,
            layer: data.layer,
            order: data.order,
            game_layer,
            cap_vertices: data.cap_vertices as usize,
            corner_vertices: data.corner_vertices as i32,
            alignment: data.alignment as i32,
            looped: data.looped,
            texture_mode: data.texture_mode,
            use_world_space: data.use_world_space,
            enabled: data.enabled,
            visible,
        }
    }

    /// `(bool)LineRenderer`: false once its component was destroyed.
    pub fn line_destroyed(&self, l: usize) -> bool {
        self.destroyed_components.contains(&self.lines[l].id)
    }

    /// Object.Destroy(LineRenderer): queued like a DestroyObjectDelayed (a second Destroy finds it queued).
    pub fn line_queue_destroy(&mut self, l: usize) {
        let id = self.lines[l].id;
        if !self.destroyed_components.contains(&id) && !self.component_destroys.iter().any(|&(_, i)| i == id) {
            let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
            self.component_destroys.push((at, id));
        }
    }

    /// The queued Object.Destroy of a LineRenderer (false: `id` is not one).
    pub fn line_run_destroy(&mut self, id: i32) -> bool {
        if !self.lines.iter().any(|x| x.id == id) {
            return false;
        }
        self.component_destroys.retain(|&(_, i)| i != id);
        self.destroyed_components.insert(id);
        true
    }

    /// LineRenderer.positionCount = n: the positions are kept, new ones are the zero vector.
    pub fn line_set_position_count(&mut self, l: usize, n: usize) {
        self.lines[l].positions.resize(n, [0.0; 3]);
    }

    /// LineRenderer.SetPosition(i, position): out of range is a logged error in Unity (not reached by any caller here).
    pub fn line_set_position(&mut self, l: usize, i: usize, position: [f32; 3]) {
        let line = &mut self.lines[l];
        assert!(i < line.positions.len(), "LineRenderer.SetPosition({i}) beyond positionCount {}", line.positions.len());
        line.positions[i] = position;
    }

    /// LineRenderer.transform.TransformPoint(point): the object's TRS, then each ancestor's.
    pub fn line_transform_point(&self, l: usize, point: [f32; 3]) -> [f32; 3] {
        let line = &self.lines[l];
        let mut v = [point[0], point[1], point[2], 0.0];
        for node in &self.anchored_nodes(line.anchor, &line.chain) {
            v = crate::unity::transform::global_position(node, v);
        }
        [v[0], v[1], v[2]]
    }

    /// LineRenderer.transform.InverseTransformPoint(point): each ancestor's inverse from the root down, then the object's.
    pub fn line_inverse_transform_point(&self, l: usize, point: [f32; 3]) -> [f32; 3] {
        let line = &self.lines[l];
        let mut v = [point[0], point[1], point[2], 0.0];
        for node in self.anchored_nodes(line.anchor, &line.chain).iter().rev() {
            v = crate::unity::transform::inverse(node, v, [0.0, 0.0, 0.0, 1.0]).0;
        }
        [v[0], v[1], v[2]]
    }

    /// LineRenderer.SetPositions(array): the first min(positionCount, array.Length) positions.
    pub fn line_set_positions(&mut self, l: usize, array: &[[f32; 3]]) {
        let line = &mut self.lines[l];
        let n = line.positions.len().min(array.len());
        line.positions[..n].copy_from_slice(&array[..n]);
    }
}
