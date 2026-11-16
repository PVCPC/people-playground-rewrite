//! The map editor's view scripts as game-side state for an editor screen: MapProperties and MapBounds,
//! MapEditorCameraBehaviour (pan and zoom of the main camera), MapEditorPanelBehaviour (the map bounds backdrop quad)
//! and MapEditorGizmoDrawer, whose GL immediate-mode calls are recorded as `GlBatch`es for the screen to draw.
#![allow(non_snake_case)]

use super::super::{d, mathf};
use super::{bmax, bmin, v3, MapEditor, Shape};

/// MapBounds
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MapBounds {
    pub BoundsCenterInMeters: [i32; 2],
    pub BoundsSizeInMeters: [i32; 2],
}

impl MapBounds {
    /// MapBounds.GetSizeInUnits
    pub fn GetSizeInUnits(&self) -> [f32; 2] {
        [self.BoundsSizeInMeters[0] as f32 / 0.82397, self.BoundsSizeInMeters[1] as f32 / 0.82397]
    }

    /// MapBounds.GetCenterInUnits
    pub fn GetCenterInUnits(&self) -> [f32; 2] {
        [self.BoundsCenterInMeters[0] as f32 / 0.82397, self.BoundsCenterInMeters[1] as f32 / 0.82397]
    }

    /// MapBounds.GetMinInUnits: center - size / 2 (Vector2 operators, one rounding each).
    pub fn GetMinInUnits(&self) -> [f32; 2] {
        let (c, s) = (self.GetCenterInUnits(), self.GetSizeInUnits());
        [c[0] - s[0] / 2.0, c[1] - s[1] / 2.0]
    }

    /// MapBounds.GetMaxInUnits
    pub fn GetMaxInUnits(&self) -> [f32; 2] {
        let (c, s) = (self.GetCenterInUnits(), self.GetSizeInUnits());
        [c[0] + s[0] / 2.0, c[1] + s[1] / 2.0]
    }
}

/// MapProperties
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapProperties {
    pub Name: String,
    pub Description: String,
    pub MapBounds: MapBounds,
    pub AmbientTemperature: f32,
}

/// MapEditorCameraBehaviour (the serialized fields; the screen loads the scene's values over these initialisers).
#[derive(Clone, Copy, Debug)]
pub struct CameraBehaviour {
    pub ZoomSpeed: f32,
    pub MinZoom: f32,
    pub MaxZoom: f32,
    /// BoundingBox.size (only its size is read).
    pub BoundingBoxSize: [f32; 3],
    pub KeyboardPanSpeedMultiplier: f32,
    pub KeyboardZoomSpeedMultiplier: f32,
    oldWorldMousePos: [f32; 2],
    targetPos: [f32; 3],
    targetZoom: f32,
}

impl Default for CameraBehaviour {
    fn default() -> Self {
        CameraBehaviour {
            ZoomSpeed: 1.0,
            MinZoom: 0.2,
            MaxZoom: 1000.0,
            BoundingBoxSize: [25.0, 25.0, 0.0],
            KeyboardPanSpeedMultiplier: 4.0,
            KeyboardZoomSpeedMultiplier: 10.0,
            oldWorldMousePos: [0.0; 2],
            targetPos: [0.0; 3],
            targetZoom: 5.0,
        }
    }
}

/// GL.Begin's mode: GL.LINES (1) or GL.LINE_STRIP (2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlMode {
    Lines,
    LineStrip,
}

/// The pass a GL batch draws with: Hidden/MapEditorGizmoShader (`fadeInDistance`) or Hidden/Internal-Colored (`simpleColored`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlPass {
    FadeInDistance,
    SimpleColored,
}

/// One GL.Begin ... GL.End: its pass, GL.Color and the vertices in world space (GL.MultMatrix applied).
#[derive(Clone, Debug, PartialEq)]
pub struct GlBatch {
    pub pass: GlPass,
    pub mode: GlMode,
    pub color: [f32; 4],
    pub vertices: Vec<[f32; 3]>,
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const YELLOW: [f32; 4] = [1.0, 0.92156863, 0.015686275, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

impl MapEditor {
    // MapEditorCameraBehaviour.Awake: Camera = GetComponent<Camera>(), the editor's main camera (`MapEditor::Camera`).

    /// MapEditorCameraBehaviour.LateUpdate
    pub fn camera_late_update(&mut self, input: &super::Input) {
        self.camera.targetPos = self.Camera.position;
        self.camera.targetZoom = self.Camera.orthographicSize;
        let bounds = self.MapProperties.MapBounds;
        self.camera_process_mouse_input(input);
        self.camera_process_keyboard_input(input);
        let max = bounds.GetMaxInUnits();
        let min = bounds.GetMinInUnits();
        let s = self.camera.BoundingBoxSize;
        let vector = [s[0] * 15.0, s[1] * 15.0, s[2] * 15.0];
        let c = &mut self.camera;
        c.targetPos[0] = mathf::clamp(c.targetPos[0], min[0] - vector[0], max[0] + vector[0]);
        c.targetPos[1] = mathf::clamp(c.targetPos[1], min[1] - vector[1], max[1] + vector[1]);
        self.Camera.position = [c.targetPos[0], c.targetPos[1], -10.0];
        self.Camera.orthographicSize = mathf::clamp(c.targetZoom, c.MinZoom, c.MaxZoom);
    }

    /// MapEditorCameraBehaviour.ProcessMouseInput
    fn camera_process_mouse_input(&mut self, input: &super::Input) {
        if self.ui.scroll[1].abs() > 0.0 {
            self.camera_zoom(self.ui.scroll[1], true);
        }
        if input.down(crate::il!(str "MapEditorCameraBehaviour::ProcessMouseInput" 0)) {
            self.camera.oldWorldMousePos = self.Camera.screen_to_world_point(self.ui.mouse);
        }
        if input.held(crate::il!(str "MapEditorCameraBehaviour::ProcessMouseInput" 0)) {
            // (Vector3)oldWorldMousePos - Camera.ScreenToWorldPoint(Input.mousePosition): z is 0 - camera z.
            let w = self.Camera.screen_to_world_point(self.ui.mouse);
            let o = self.camera.oldWorldMousePos;
            let t = &mut self.camera.targetPos;
            *t = [t[0] + (o[0] - w[0]), t[1] + (o[1] - w[1]), t[2] + (0.0 - self.Camera.position[2])];
        }
    }

    /// MapEditorCameraBehaviour.ProcessKeyboardInput
    fn camera_process_keyboard_input(&mut self, input: &super::Input) {
        let dt = self.ui.unscaled_delta_time;
        let num = (d(dt) * self.camera.KeyboardPanSpeedMultiplier * self.camera.targetZoom).f();
        let t = &mut self.camera.targetPos;
        if input.held("panLeft") {
            t[0] += -1.0 * num;
            t[1] += 0.0 * num;
        }
        if input.held("panRight") {
            t[0] += 1.0 * num;
            t[1] += 0.0 * num;
        }
        if input.held("panUp") {
            t[0] += 0.0 * num;
            t[1] += 1.0 * num;
        }
        if input.held("panDown") {
            t[0] += 0.0 * num;
            t[1] += -1.0 * num;
        }
        if input.held("zoomIn") {
            self.camera_zoom((d(dt) * self.camera.KeyboardZoomSpeedMultiplier).f(), false);
        }
        if input.held("zoomOut") {
            self.camera_zoom((d(0.0 - dt) * self.camera.KeyboardZoomSpeedMultiplier).f(), false);
        }
    }

    /// MapEditorCameraBehaviour.ZoomCamera(i, cursorAsOrigin = true)
    pub(crate) fn camera_zoom(&mut self, i: f32, cursor_as_origin: bool) {
        let num = (d(i) * self.camera.ZoomSpeed * 0.1f32 * self.ui.zoom_sensitivity).f();
        self.camera.targetZoom = (d(self.camera.targetZoom) - d(num) * self.Camera.orthographicSize).f();
        let c = self.camera;
        if (c.targetZoom < c.MaxZoom && c.targetZoom > c.MinZoom) & cursor_as_origin {
            let w = self.Camera.screen_to_world_point(self.ui.mouse);
            let p = self.Camera.position;
            // ScreenToWorldPoint's z is the camera's own.
            let vector = [w[0] - p[0], w[1] - p[1], p[2] - p[2]];
            let t = &mut self.camera.targetPos;
            *t = [t[0] + vector[0] * num, t[1] + vector[1] * num, t[2] + vector[2] * num];
        }
    }

    /// MapEditorPanelBehaviour.Update (MapEditorGlobal.Instance is this editor): the backdrop's localScale is the bounds'
    /// size (z 0, from the Vector2) and its position their centre.
    pub fn panel_update(&mut self) {
        let b = self.MapProperties.MapBounds;
        self.panel_scale = v3(b.GetSizeInUnits());
        self.panel_position = v3(b.GetCenterInUnits());
    }

    // MapEditorGizmoDrawer.Start: the two materials, `GlPass`.

    /// MapEditorGizmoDrawer.OnPostRender: the map bounds and grid with the fade pass, then the transform line and the
    /// selection outlines with the plain coloured pass (both shaders are always found, so both SetPass calls succeed).
    pub fn gizmo_on_post_render(&mut self) {
        let mut out = Vec::new();
        self.gizmo_draw_map_bounds(&mut out);
        self.gizmo_draw_grid(&mut out);
        self.gizmo_draw_transform_gizmo_lines(&mut out);
        self.gizmo_draw_selection_outlines(&mut out);
        self.gizmo_lines = out;
    }

    /// MapEditorGizmoDrawer.DrawTransformGizmoLines
    fn gizmo_draw_transform_gizmo_lines(&self, out: &mut Vec<GlBatch>) {
        if self.manipulator.MoveGizmo.active {
            let p = self.manipulator.MoveGizmo.position;
            gizmo_draw_line(out, [p[0], p[1]], self.manipulator.CenterOfSelection, WHITE);
        }
    }

    /// MapEditorGizmoDrawer.DrawSelectionOutlines: each selected collider in yellow (Color.yellow), each selected bounds
    /// in white, then the hovered one's bounds in green when the UI does not hold the mouse.
    fn gizmo_draw_selection_outlines(&self, out: &mut Vec<GlBatch>) {
        for &i in &self.CurrentlySelected {
            let s = &self.selectables[i];
            for c in &s.colliders {
                self.gizmo_draw_collider(out, i, c, YELLOW);
            }
        }
        for &i in &self.CurrentlySelected {
            self.gizmo_draw_bounds(out, i, WHITE);
        }
        if !self.UiLock() {
            if let Some(h) = self.TopMostHovering().filter(|&h| self.selectables[h].alive) {
                self.gizmo_draw_bounds(out, h, GREEN);
            }
        }
    }

    /// DrawSelectionOutlines' local drawCollider: a box as four GL.LINES segments, a polygon as DrawLine per edge (closed),
    /// a circle as a 65-vertex line strip of radius × the larger |lossyScale| axis, turned in 5.625° steps (not by the
    /// object's rotation) about TransformPoint(offset).
    fn gizmo_draw_collider(&self, out: &mut Vec<GlBatch>, i: usize, c: &Shape, color: [f32; 4]) {
        let s = &self.selectables[i];
        if !s.alive {
            return;
        }
        let t = &s.transform;
        match c {
            Shape::Box { center, size } => {
                let h = [size[0] / 2.0, size[1] / 2.0];
                let corner = |x: f32, y: f32| t.transform_point(v3([center[0] + x, center[1] + y]));
                let (a, b, cc, dd) = (corner(0.0 - h[0], h[1]), corner(h[0], h[1]), corner(h[0], 0.0 - h[1]), corner(0.0 - h[0], 0.0 - h[1]));
                out.push(GlBatch { pass: GlPass::SimpleColored, mode: GlMode::Lines, color, vertices: vec![a, b, b, cc, cc, dd, dd, a] });
            }
            Shape::Polygon(path) => {
                // The selectable's shapes carry one path, offset and child transforms already applied.
                for l in 0..path.len().saturating_sub(1) {
                    let (a, b) = (t.transform_point(v3(path[l])), t.transform_point(v3(path[l + 1])));
                    gizmo_draw_line(out, [a[0], a[1]], [b[0], b[1]], color);
                }
                if let (Some(&last), Some(&first)) = (path.last(), path.first()) {
                    let (a, b) = (t.transform_point(v3(last)), t.transform_point(v3(first)));
                    gizmo_draw_line(out, [a[0], a[1]], [b[0], b[1]], color);
                }
            }
            Shape::Circle { center, radius } => {
                let x = radius * mathf::max(t.scale[0].abs(), t.scale[1].abs());
                let o = t.transform_point(v3(*center));
                let mut vertices = vec![[x + o[0], 0.0 + o[1], 0.0 + o[2]]];
                for k in 1..64 {
                    let r = super::super::value_types::utils_rotate([x, 0.0], 5.625 * k as f32, [0.0; 2]);
                    vertices.push([r[0] + o[0], r[1] + o[1], r[2] + o[2]]);
                }
                vertices.push([x + o[0], 0.0 + o[1], 0.0 + o[2]]);
                out.push(GlBatch { pass: GlPass::SimpleColored, mode: GlMode::LineStrip, color, vertices });
            }
        }
    }

    /// MapEditorGizmoDrawer.DrawBounds: the LocalBounds rectangle under the item's localToWorldMatrix.
    fn gizmo_draw_bounds(&self, out: &mut Vec<GlBatch>, i: usize, color: [f32; 4]) {
        let s = &self.selectables[i];
        let (lo, hi) = (bmin(&s.LocalBounds), bmax(&s.LocalBounds));
        let p = |x: f32, y: f32| s.transform.transform_point(v3([x, y]));
        let (a, b, c, dd) = (p(lo[0], lo[1]), p(hi[0], lo[1]), p(lo[0], hi[1]), p(hi[0], hi[1]));
        out.push(GlBatch { pass: GlPass::SimpleColored, mode: GlMode::Lines, color, vertices: vec![a, b, b, dd, dd, c, c, a] });
    }

    /// MapEditorGizmoDrawer.DrawGrid: a line per metre across the bounds, at 5% white.
    fn gizmo_draw_grid(&self, out: &mut Vec<GlBatch>) {
        let b = self.MapProperties.MapBounds;
        let center = b.GetCenterInUnits();
        let size = b.GetSizeInUnits();
        let lo = [center[0] - size[0] / 2.0, center[1] - size[1] / 2.0];
        let hi = [center[0] + size[0] / 2.0, center[1] + size[1] / 2.0];
        let mut vertices = Vec::new();
        for i in 0..b.BoundsSizeInMeters[0] {
            let x2 = (d(i as f32) / 0.82397f32 + center[0] - d(size[0]) / 2.0f32).f();
            vertices.push([x2, hi[1], 0.0]);
            vertices.push([x2, lo[1], 0.0]);
        }
        for j in 0..b.BoundsSizeInMeters[1] {
            let y2 = (d(j as f32) / 0.82397f32 + center[1] - d(size[1]) / 2.0f32).f();
            vertices.push([hi[0], y2, 0.0]);
            vertices.push([lo[0], y2, 0.0]);
        }
        out.push(GlBatch { pass: GlPass::FadeInDistance, mode: GlMode::Lines, color: [1.0, 1.0, 1.0, 0.05], vertices });
    }

    /// MapEditorGizmoDrawer.DrawMapBounds: a closed white line strip round the bounds.
    fn gizmo_draw_map_bounds(&self, out: &mut Vec<GlBatch>) {
        let b = self.MapProperties.MapBounds;
        let (max, min) = (b.GetMaxInUnits(), b.GetMinInUnits());
        let vertices = vec![[min[0], min[1], 0.0], [max[0], min[1], 0.0], [max[0], max[1], 0.0], [min[0], max[1], 0.0], [min[0], min[1], 0.0]];
        out.push(GlBatch { pass: GlPass::FadeInDistance, mode: GlMode::LineStrip, color: WHITE, vertices });
    }
}

/// MapEditorGizmoDrawer.DrawLine: one GL.LINES segment (Vector2 to Vector3: z 0).
fn gizmo_draw_line(out: &mut Vec<GlBatch>, a: [f32; 2], b: [f32; 2], color: [f32; 4]) {
    // The pass of the caller: DrawLine is only called after simpleColored.SetPass.
    out.push(GlBatch { pass: GlPass::SimpleColored, mode: GlMode::Lines, color, vertices: vec![v3(a), v3(b)] });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 m × 10 m map: units of 1 / 0.82397 m; the camera clamps to 15 bounding boxes past the bounds.
    #[test]
    fn bounds_and_camera_clamp() {
        let b = MapBounds { BoundsCenterInMeters: [0, 0], BoundsSizeInMeters: [10, 10] };
        assert_eq!(b.GetSizeInUnits()[0], 10.0f32 / 0.82397);
        assert_eq!(b.GetMinInUnits()[0], 0.0 - (10.0f32 / 0.82397) / 2.0);
        let mut e = MapEditor::new(super::super::EditorCamera::default());
        e.MapProperties.MapBounds = b;
        e.Camera.position = [10000.0, 0.0, -10.0];
        e.camera_late_update(&super::super::Input::default());
        assert_eq!(e.Camera.position[0], b.GetMaxInUnits()[0] + 375.0);
        e.gizmo_on_post_render();
        assert_eq!(e.gizmo_lines.len(), 2);
        assert_eq!(e.gizmo_lines[1].vertices.len(), 40);
    }
}
