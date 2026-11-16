//! The map editor's scripts as game-side state, ready for an editor screen: MapEditorGlobal, MapEditorSelectionManager,
//! MapEditorSelectable, MapEditorObjectBehaviour, MapEditorManipulator and MapEditorContextMenuBehaviour. The screen sets
//! the camera, the pointer and the UI facts in `EditorUi`, then `Game::map_editor_update` runs one frame from `Input`
//! (the same controls tool.rs reads). Their order within a frame is not in the export: Global.Update, the selection
//! manager, the context menu, then the manipulator's OnRenderObject.
use super::mathf;
use super::tool::Input;
use super::{d, Bounds, Game};

#[path = "map_editor_view.rs"]
pub mod view;
#[path = "map_editor_fncheck.rs"]
pub mod fncheck;

/// An editor object's Transform: its stored local pose under a parent that only translates (the scene's object
/// container: identity rotation, scale 1), and the world position the engine reads back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Transform.position (the read-back world position).
    pub position: [f32; 3],
    /// The parent's localPosition (zero for a root).
    pub parent: [f32; 3],
    /// Transform.localPosition.
    pub local: [f32; 3],
    /// Quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

pub const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

impl Default for Transform {
    fn default() -> Self {
        Transform { position: [0.0; 3], parent: [0.0; 3], local: [0.0; 3], rotation: IDENTITY, scale: [1.0; 3] }
    }
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v3(v: [f32; 2]) -> [f32; 3] {
    [v[0], v[1], 0.0]
}

impl Transform {
    /// Transform.TransformPoint
    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        // Native Transform::TransformPoint (1807f6830): one step per node, the object then its parent, f32 in its order.
        let step = |q: [f32; 4], s: [f32; 3], t: [f32; 3], p: [f32; 3]| {
            let [x, y, z, w] = q;
            let (a, b, c) = (s[0] * p[0], s[1] * p[1], s[2] * p[2]);
            [
                (y * -2.0 * y - z * 2.0 * z) * a + a + (z * -2.0 * w - x * -2.0 * y) * b + (x * 2.0 * z - y * -2.0 * w) * c + t[0],
                (y * 2.0 * x - z * -2.0 * w) * a + b + (z * -2.0 * z - x * 2.0 * x) * b + (x * -2.0 * w - y * -2.0 * z) * c + t[1],
                (y * -2.0 * w - z * -2.0 * x) * a + c + (z * 2.0 * y - x * -2.0 * w) * b + (x * -2.0 * x - y * 2.0 * y) * c + t[2],
            ]
        };
        step(IDENTITY, [1.0; 3], self.parent, step(self.rotation, self.scale, self.local, p))
    }

    /// A root (or a parent at the origin) at world position `p`.
    pub fn at(position: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Self {
        Transform { position, parent: [0.0; 3], local: position, rotation, scale }
    }

    /// transform.position = p: SetPosition's world-to-local through the parent, then the read-back world position.
    pub fn set_position(&mut self, p: [f32; 3]) {
        let [x, y, z] = self.parent;
        let n = crate::unity::transform::Trs { t: [x, y, z, 0.0], q: IDENTITY, s: [1.0, 1.0, 1.0, 0.0] };
        let ((l, _), (w, _)) = crate::unity::transform::set_world(&[n], p, self.rotation);
        (self.local, self.position) = (l, w);
    }

    /// Transform.InverseTransformPoint
    pub fn inverse_transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.rotation;
        let v = mathf::rotate([-x, -y, -z, w], sub3(p, self.position));
        [v[0] / self.scale[0], v[1] / self.scale[1], v[2] / self.scale[2]]
    }

    /// Transform.Translate(translation, Space.World)
    pub fn translate_world(&mut self, t: [f32; 3]) {
        self.set_position(add3(self.position, t));
    }

    /// Transform.Translate(translation, Space.Self): the translation turned by TransformDirection.
    pub fn translate_self(&mut self, t: [f32; 3]) {
        self.set_position(add3(self.position, mathf::rotate(self.rotation, t)));
    }

    /// Transform.right
    pub fn right(&self) -> [f32; 3] {
        mathf::rotate(self.rotation, [1.0, 0.0, 0.0])
    }

    /// Transform.up
    pub fn up(&self) -> [f32; 3] {
        mathf::rotate(self.rotation, [0.0, 1.0, 0.0])
    }
}

/// Vector3.ProjectOnPlane
pub fn project_on_plane(v: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let num = (d(n[0]) * n[0] + d(n[1]) * n[1] + d(n[2]) * n[2]).f();
    if num < mathf::EPSILON {
        return v;
    }
    let num2 = (d(v[0]) * n[0] + d(v[1]) * n[1] + d(v[2]) * n[2]).f();
    [(d(v[0]) - d(n[0]) * num2 / num).f(), (d(v[1]) - d(n[1]) * num2 / num).f(), (d(v[2]) - d(n[2]) * num2 / num).f()]
}

/// Vector3.magnitude
fn magnitude3(v: [f32; 3]) -> f32 {
    (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).0.sqrt() as f32
}

fn bmin(b: &Bounds) -> [f32; 2] {
    [b.center[0] - b.extents[0], b.center[1] - b.extents[1]]
}
fn bmax(b: &Bounds) -> [f32; 2] {
    [b.center[0] + b.extents[0], b.center[1] + b.extents[1]]
}
fn bsize(b: &Bounds) -> [f32; 2] {
    [b.extents[0] * 2.0, b.extents[1] * 2.0]
}

/// A Collider2D's shape in the selectable's local space (a child collider's own transform already applied).
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Box { center: [f32; 2], size: [f32; 2] },
    Circle { center: [f32; 2], radius: f32 },
    Polygon(Vec<[f32; 2]>),
}

impl Shape {
    /// Collider2D.bounds at an identity transform: (min, max).
    fn local_min_max(&self) -> ([f32; 2], [f32; 2]) {
        match self {
            Shape::Box { center, size } => ([center[0] - size[0] / 2.0, center[1] - size[1] / 2.0], [center[0] + size[0] / 2.0, center[1] + size[1] / 2.0]),
            Shape::Circle { center, radius } => ([center[0] - radius, center[1] - radius], [center[0] + radius, center[1] + radius]),
            Shape::Polygon(p) => {
                let mut lo = [f32::MAX; 2];
                let mut hi = [f32::MIN; 2];
                for q in p {
                    for i in 0..2 {
                        lo[i] = mathf::min(lo[i], q[i]);
                        hi[i] = mathf::max(hi[i], q[i]);
                    }
                }
                (lo, hi)
            }
        }
    }

    /// Collider2D.OverlapPoint, the point taken to the collider's local space through its owner's transform.
    fn overlap_point(&self, t: &Transform, point: [f32; 2]) -> bool {
        let l = t.inverse_transform_point(v3(point));
        let l = [l[0], l[1]];
        match self {
            Shape::Box { center, size } => (l[0] - center[0]).abs() <= size[0].abs() / 2.0 && (l[1] - center[1]).abs() <= size[1].abs() / 2.0,
            Shape::Circle { center, radius } => {
                // A circle collider scales by the larger axis.
                let r = radius * mathf::max(t.scale[0].abs(), t.scale[1].abs());
                let c = t.transform_point(v3(*center));
                let dx = point[0] - c[0];
                let dy = point[1] - c[1];
                dx * dx + dy * dy <= r * r
            }
            Shape::Polygon(p) => {
                // Even-odd crossing test.
                let mut inside = false;
                let mut j = p.len().wrapping_sub(1);
                for i in 0..p.len() {
                    let (a, b) = (p[i], p[j]);
                    if (a[1] > l[1]) != (b[1] > l[1]) && l[0] < (b[0] - a[0]) * (l[1] - a[1]) / (b[1] - a[1]) + a[0] {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
        }
    }
}

/// MapEditorObjectBehaviour: the Selectable it found in Awake (this object's own id) and its context menu buttons.
#[derive(Clone, Debug, Default)]
pub struct ObjectBehaviour {
    pub ContextMenuButtons: Vec<super::menu::ShownButton>,
}

/// MapEditorSelectable
#[derive(Clone, Debug)]
pub struct Selectable {
    pub Depth: i32,
    pub LocalBounds: Bounds,
    pub IsSelected: bool,
    pub transform: Transform,
    colliders: Vec<Shape>,
    selectionAge: i32,
    /// The MapEditorObjectBehaviour on the same object.
    pub object: Option<ObjectBehaviour>,
    /// Not destroyed yet (Unity's `(bool)` check).
    pub alive: bool,
}

/// The orthographic main camera.
#[derive(Clone, Copy, Debug)]
pub struct EditorCamera {
    pub position: [f32; 3],
    pub orthographicSize: f32,
    pub pixelWidth: f32,
    pub pixelHeight: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        EditorCamera { position: [0.0, 0.0, -10.0], orthographicSize: 5.0, pixelWidth: 1920.0, pixelHeight: 1080.0 }
    }
}

impl EditorCamera {
    /// Camera.ScreenToWorldPoint of a screen position (x and y of the result).
    /// Camera.ScreenToWorldPoint((p, 0)) (Camera::ScreenToWorldPoint 1805ea260, mono eye): the world-to-clip matrix
    /// (projection * world-to-camera) and the world-to-camera matrix each inverted by InvertMatrix4x4_Full, then
    /// CameraUnProject (1805987f0). The editor camera never turns (identity rotation), near 1, far 64 (the scene's).
    pub fn screen_to_world_point(&self, p: [f32; 2]) -> [f32; 2] {
        let (w, h) = (self.pixelWidth, self.pixelHeight);
        let aspect = w / h;
        let size = self.orthographicSize;
        let proj = ortho(-(size * aspect), size * aspect, -size, size, 1.0, 64.0);
        let [cx, cy, cz] = self.position;
        // SetScale(1, 1, -1) * GetWorldToLocalMatrixNoScale() for an identity rotation.
        let wtc = [1.0, 0.0, -0.0, 0.0, 0.0, 1.0, -0.0, 0.0, 0.0, 0.0, -1.0, 0.0, -cx, -cy, cz, 1.0];
        let mut clip = [0.0f32; 16];
        for c in 0..4 {
            for r in 0..4 {
                clip[c * 4 + r] = wtc[c * 4 + 1] * proj[4 + r] + wtc[c * 4] * proj[r] + wtc[c * 4 + 2] * proj[8 + r] + wtc[c * 4 + 3] * proj[12 + r];
            }
        }
        let (Some(m), Some(c2w)) = (invert_full(&clip), invert_full(&wtc)) else { return [0.0; 2] };
        // CameraUnProject, the rect (0, 0, w, h), screen z 0.
        let x = (p[0] - 0.0 + (p[0] - 0.0)) / w - 1.0;
        let y = (p[1] - 0.0 + (p[1] - 0.0)) / h - 1.0;
        let ww = m[7] * y + m[3] * x + m[11] * 0.95 + m[15];
        if ww.abs() <= 1e-7 {
            return [0.0; 2];
        }
        let ww = 1.0 / ww;
        let mut oy = (y * m[5] + x * m[1] + m[9] * 0.95 + m[13]) * ww;
        let mut ox = ww * (y * m[4] + x * m[0] + m[8] * 0.95 + m[12]);
        let oz = (y * m[6] + x * m[2] + m[10] * 0.95 + m[14]) * ww;
        let (dx, dy, dz) = (ox - c2w[12], oy - c2w[13], oz - c2w[14]);
        let dist = dx * -c2w[8] + -c2w[9] * dy + dz * -c2w[10];
        if dist.abs() < 1e-6 {
            return [0.0; 2];
        }
        if m[3] == 0.0 && m[7] == 0.0 && m[11] == 0.0 && m[15] == 1.0 {
            let t = dist - 0.0;
            ox -= -c2w[8] * t;
            oy -= -c2w[9] * t;
        } else {
            let t = 0.0 / dist;
            ox = dx * t + c2w[12];
            oy = c2w[13] + dy * t;
        }
        [ox, oy]
    }
}

/// Matrix4x4f::SetOrtho (180741c20), column-major.
fn ortho(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[15] = 1.0;
    m[0] = 2.0 / (r - l);
    m[12] = -((l + r) / (r - l));
    m[13] = -((b + t) / (t - b));
    m[14] = -((n + f) / (f - n));
    m[10] = -2.0 / (f - n);
    m[5] = 2.0 / (t - b);
    m
}

/// InvertMatrix4x4_Full (18073e800): Gauss-Jordan with partial pivoting on rows [m row | identity], f32, in its own
/// order (pivot swaps on strictly larger |x| in the first and third steps, on larger-or-equal kept in the second).
fn invert_full(m: &[f32; 16]) -> Option<[f32; 16]> {
    let mut rows = [[0.0f32; 8]; 4];
    for (r, row) in rows.iter_mut().enumerate() {
        *row = [m[r], m[r + 4], m[r + 8], m[r + 12], 0.0, 0.0, 0.0, 0.0];
        row[4 + r] = 1.0;
    }
    // Row order after pivoting: p[0] the pivot of column 0, and so on.
    let (r2, r3) = if rows[2][0].abs() < rows[3][0].abs() { (2, 3) } else { (3, 2) }; // r3 = larger (top)
    let (r1a, top) = if rows[1][0].abs() < rows[r3][0].abs() { (1, r3) } else { (r3, 1) };
    let (p1, p0) = if rows[0][0].abs() < rows[top][0].abs() { (0, top) } else { (top, 0) };
    let (p, q, s) = (p1, r1a, r2); // remaining rows (pfVar4, pfVar1, pfVar3)
    let piv = rows[p0][0];
    if piv == 0.0 {
        return None;
    }
    let (m1, m2, m3) = (rows[p][0] / piv, rows[q][0] / piv, rows[s][0] / piv);
    for c in 1..8 {
        let v = rows[p0][c];
        if c < 4 || v != 0.0 {
            rows[p][c] -= v * m1;
            rows[q][c] -= v * m2;
            rows[s][c] -= v * m3;
        }
    }
    // Second column: among q (pfVar1) and s (pfVar3), then against p (pfVar4).
    let (lo, hi) = if rows[s][1].abs() <= rows[q][1].abs() { (s, q) } else { (q, s) };
    let (top, rest) = if rows[hi][1].abs() <= rows[p][1].abs() { (p, hi) } else { (hi, p) };
    let (r1, mut a, mut z) = (top, rest, lo); // a = pfVar4, z = pfVar2
    let piv = rows[r1][1];
    if piv == 0.0 {
        return None;
    }
    let (ma, mz) = (rows[a][1] / piv, rows[z][1] / piv);
    for c in 2..8 {
        let v = rows[r1][c];
        if c < 4 || v != 0.0 {
            rows[a][c] -= ma * v;
            rows[z][c] -= mz * v;
        }
    }
    if rows[a][2].abs() < rows[z][2].abs() {
        std::mem::swap(&mut a, &mut z);
    }
    let (r2, r3) = (a, z);
    if rows[r2][2] == 0.0 {
        return None;
    }
    let m3 = rows[r3][2] / rows[r2][2];
    for c in 3..8 {
        rows[r3][c] -= m3 * rows[r2][c];
    }
    if rows[r3][3] == 0.0 {
        return None;
    }
    let inv = 1.0 / rows[r3][3];
    for c in 4..8 {
        rows[r3][c] *= inv;
    }
    let inv = 1.0 / rows[r2][2];
    let v = rows[r2][3];
    for c in 4..8 {
        rows[r2][c] = (rows[r2][c] - v * rows[r3][c]) * inv;
    }
    for r in [r1, p0] {
        let v = rows[r][3];
        for c in 4..8 {
            rows[r][c] -= v * rows[r3][c];
        }
    }
    let inv = 1.0 / rows[r1][1];
    let v = rows[r1][2];
    for c in 4..8 {
        rows[r1][c] = (rows[r1][c] - v * rows[r2][c]) * inv;
    }
    let v = rows[p0][2];
    for c in 4..8 {
        rows[p0][c] -= v * rows[r2][c];
    }
    let inv = 1.0 / rows[p0][0];
    let v = rows[p0][1];
    for c in 4..8 {
        rows[p0][c] = (rows[p0][c] - v * rows[r1][c]) * inv;
    }
    let mut out = [0.0f32; 16];
    for (r, &i) in [p0, r1, r2, r3].iter().enumerate() {
        for c in 0..4 {
            out[c * 4 + r] = rows[i][4 + c];
        }
    }
    Some(out)
}

#[cfg(test)]
mod camera_tests {
    #[test]
    fn invert_full_inverts() {
        let m = super::ortho(-8.0, 8.0, -5.0, 5.0, 1.0, 64.0);
        let i = super::invert_full(&m).unwrap();
        assert_eq!((i[0], i[5], i[15]), (8.0, 5.0, 1.0));
        let e = super::EditorCamera { position: [3.0, -2.0, -10.0], orthographicSize: 5.0, pixelWidth: 1920.0, pixelHeight: 1080.0 };
        let p = e.screen_to_world_point([960.0, 540.0]);
        assert!((p[0] - 3.0).abs() < 1e-5 && (p[1] + 2.0).abs() < 1e-5, "{p:?}");
    }
}

/// What the screen tells the editor about its UI each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorUi {
    /// EventSystem.IsPointerOverGameObject()
    pub pointer_over_ui: bool,
    /// DialogBox.IsAnyDialogboxOpen
    pub dialog_open: bool,
    /// ScreenUIEventEmitter.IsMouseOverAny()
    pub mouse_over_emitter: bool,
    /// EventSystem.currentSelectedGameObject is a child of the context menu.
    pub selected_in_context_menu: bool,
    /// The catalog's RectTransform: activeInHierarchy and GetWorldSpaceBounds().size.x.
    pub catalog_active: bool,
    pub catalog_width: f32,
    /// Screen.width and Screen.height.
    pub screen: [f32; 2],
    /// Input.mousePosition
    pub mouse: [f32; 2],
    /// Input.mouseScrollDelta, Time.unscaledDeltaTime and UserPreferenceManager.Current.ZoomSensitivity.
    pub scroll: [f32; 2],
    pub unscaled_delta_time: f32,
    pub zoom_sensitivity: f32,
}

/// One ScreenUIEventEmitter gizmo handle: its Transform and whether it IsBeingDragged.
#[derive(Clone, Copy, Debug)]
pub struct Handle {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub dragged: bool,
}

impl Default for Handle {
    fn default() -> Self {
        Handle { position: [0.0; 3], rotation: IDENTITY, scale: [1.0; 3], dragged: false }
    }
}

impl Handle {
    /// Transform.SetPositionAndRotation on a child of the gizmo `parent` (position, rotation; scale 1): the world pose
    /// the engine reads back after its world-to-local round trip.
    fn set_position_and_rotation(&mut self, parent: crate::unity::transform::Trs, p: [f32; 3], r: [f32; 4]) {
        (self.position, self.rotation) = crate::unity::transform::set_world(&[parent, GIZMO_ROOT], p, r).1;
    }
}

/// MapEditorManipulator
#[derive(Clone, Debug, Default)]
pub struct Manipulator {
    pub MoveGizmo: Gizmo,
    pub ResizeGizmo: Gizmo,
    pub AllAxisMoveGizmo: Handle,
    pub VerticalMoveGizmo: Handle,
    pub HorizontalMoveGizmo: Handle,
    pub TopResizeHandle: Handle,
    pub RightResizeHandle: Handle,
    pub LeftResizeHandle: Handle,
    pub BottomResizeHandle: Handle,
    pub CenterOfSelection: [f32; 2],
    previousMousePos: [f32; 2],
}

impl Manipulator {
    /// ScreenUIEventEmitter.OnDisable of the move gizmo's handles (SetActive(false) on their parent): IsBeingDragged = false.
    fn disable_move_handles(&mut self) {
        for h in [&mut self.AllAxisMoveGizmo, &mut self.VerticalMoveGizmo, &mut self.HorizontalMoveGizmo] {
            h.dragged = false;
        }
    }

    /// ScreenUIEventEmitter.OnDisable of the resize gizmo's handles.
    fn disable_resize_handles(&mut self) {
        for h in [&mut self.TopResizeHandle, &mut self.RightResizeHandle, &mut self.LeftResizeHandle, &mut self.BottomResizeHandle] {
            h.dragged = false;
        }
    }
}

/// A gizmo GameObject: its active flag and Transform.
#[derive(Clone, Copy, Debug)]
pub struct Gizmo {
    pub active: bool,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// The scene's "Map Editor" root the gizmos hang under (identity at the origin, scale 1).
const GIZMO_ROOT: crate::unity::transform::Trs = crate::unity::transform::Trs { t: [0.0; 4], q: IDENTITY, s: [1.0, 1.0, 1.0, 0.0] };

impl Gizmo {
    /// transform.position = p: the world position the engine reads back through the root.
    fn set_position(&mut self, p: [f32; 3]) {
        self.position = crate::unity::transform::set_world(&[GIZMO_ROOT], p, self.rotation).1 .0;
    }

    /// transform.rotation = r, read back through the root.
    fn set_rotation(&mut self, r: [f32; 4]) {
        self.rotation = crate::unity::transform::set_world(&[GIZMO_ROOT], self.position, r).1 .1;
    }

    /// The gizmo's local TRS under the root.
    fn local(&self) -> crate::unity::transform::Trs {
        let (t, q) = crate::unity::transform::set_world(&[GIZMO_ROOT], self.position, self.rotation).0;
        crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s: [1.0, 1.0, 1.0, 0.0] }
    }
}

impl Default for Gizmo {
    fn default() -> Self {
        Gizmo { active: false, position: [0.0; 3], rotation: IDENTITY }
    }
}

impl ContextMenu {
    /// The overlay canvas the menu sits under: at the screen's centre, scaled by CanvasScaler (ScaleWithScreenSize):
    /// Pow(2, Lerp(Log(w / ref.x, 2), Log(h / ref.y, 2), match)), each Mathf call in double, rounded at its return.
    pub fn canvas(&self, screen: [f32; 2]) -> crate::unity::transform::Trs {
        let log2 = |x: f32| ((x as f64).ln() / 2f64.ln()) as f32;
        let lw = log2(screen[0] / self.reference_resolution[0]);
        let lh = log2(screen[1] / self.reference_resolution[1]);
        let s = 2f64.powf(mathf::lerp(lw, lh, self.match_width_or_height) as f64) as f32;
        crate::unity::transform::Trs { t: [screen[0] / 2.0, screen[1] / 2.0, 0.0, 0.0], q: IDENTITY, s: [s, s, s, 0.0] }
    }
}

/// MapEditorContextMenuBehaviour
#[derive(Clone, Debug, Default)]
pub struct ContextMenu {
    /// The menu's GameObject is active.
    pub shown: bool,
    pub position: [f32; 3],
    /// RectTransform.sizeDelta, and the CanvasScaler's referenceResolution and matchWidthOrHeight.
    pub size_delta: [f32; 2],
    pub reference_resolution: [f32; 2],
    pub match_width_or_height: f32,
    pub DeleteInteractable: bool,
    pub CopyInteractable: bool,
    pub PasteInteractable: bool,
    selectablesWhenShown: Vec<usize>,
    selectedObjects: Vec<usize>,
}

/// ActionUniverse
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Universe {
    #[default]
    None,
    All,
    Game,
    MapEditor,
}

/// The editor's objects: the selectables arena (ids never reused), and MapEditorGlobal, the selection manager, the
/// manipulator and the context menu.
#[derive(Clone, Debug, Default)]
pub struct MapEditor {
    // MapEditorGlobal
    pub PauseMenu: bool,
    pub Catalog: bool,
    pub WorldMousePos: [f32; 2],
    pub Camera: EditorCamera,
    pub CurrentUniverse: Universe,
    pub ui: EditorUi,
    pub(crate) uiLockingRegistrations: i32,
    /// MapEditorSelectionManager.OnSelect and OnDeselect: their AddListener'd listeners (all MapEditorManipulator's).
    pub(crate) listeners: [u32; 2],
    // MapEditorSelectionManager
    selectables: Vec<Selectable>,
    /// MapEditorSelectable.Selectables, a HashSet: its entries in slot order, and the freed slots (the last freed is
    /// reused first).
    slots: Vec<Option<usize>>,
    free: Vec<usize>,
    pub CurrentlySelected: Vec<usize>,
    pub HoveringOver: Vec<usize>,
    hoveringHash: usize,
    // MapEditorManipulator, MapEditorContextMenuBehaviour
    pub manipulator: Manipulator,
    pub context_menu: ContextMenu,
    /// Object.Destroy'd selectables, destroyed at the end of the frame.
    pending_destroy: Vec<usize>,
    /// MapEditorGlobal.MapProperties
    pub MapProperties: view::MapProperties,
    /// MapEditorCameraBehaviour on the main camera.
    pub camera: view::CameraBehaviour,
    /// MapEditorPanelBehaviour's Transform: localScale and position.
    pub panel_scale: [f32; 3],
    pub panel_position: [f32; 3],
    /// MapEditorGizmoDrawer's GL lines of the last frame.
    pub gizmo_lines: Vec<view::GlBatch>,
}

impl MapEditor {
    /// MapEditorGlobal.Awake, MapEditorGlobal.Start, MapEditorSelectionManager.Awake, MapEditorManipulator.Awake, MapEditorManipulator.Start:
    /// InputSystem.CurrentUniverse = MapEditor (InputSystem.Load is the screen's control file load); the selection manager and its
    /// listeners are this struct, Camera.main is `camera` and the move gizmo starts hidden (the catalog rect is `ui`).
    pub fn new(camera: EditorCamera) -> Self {
        let mut e = MapEditor::default();
        e.global_awake();
        e.global_start(camera);
        e.manipulator_awake();
        e.manipulator_start();
        e
    }

    /// MapEditorGlobal.Awake: Instance is this editor; InputSystem.Load is the screen's control file load.
    pub fn global_awake(&mut self) {
        self.CurrentUniverse = Universe::MapEditor;
    }

    /// MapEditorGlobal.Start: Camera = Camera.main.
    pub fn global_start(&mut self, main: EditorCamera) {
        self.Camera = main;
    }

    /// MapEditorManipulator.Awake: OnSelect/OnDeselect.AddListener (MapEditorSelectionManager.Instance is this editor).
    pub fn manipulator_awake(&mut self) {
        self.listeners[0] += 1;
        self.listeners[1] += 1;
    }

    /// MapEditorManipulator.Start: the move gizmo hidden (catalogRect is `ui`'s catalog).
    pub fn manipulator_start(&mut self) {
        self.manipulator.MoveGizmo.active = false;
    }

    /// MapEditorSelectable.Awake: colliders = GetComponentsInChildren (`colliders`), and Selectables.Add (a no-op when in).
    pub fn selectable_awake(&mut self, id: usize, colliders: Vec<Shape>) {
        self.selectables[id].colliders = colliders;
        if !self.slots.contains(&Some(id)) {
            match self.free.pop() {
                Some(s) => self.slots[s] = Some(id),
                None => self.slots.push(Some(id)),
            }
        }
    }

    /// MapEditorObjectBehaviour.Awake: Selectable = GetComponent<MapEditorSelectable>() (the object's own selectable).
    pub fn object_awake(&self, id: usize) -> Option<usize> {
        self.selectables[id].object.as_ref().map(|_| id)
    }

    /// MapEditorGlobal.get_IsPauseMenuOpen
    pub fn IsPauseMenuOpen(&self) -> bool {
        self.PauseMenu
    }

    /// MapEditorGlobal.get_IsCatalogOpen
    pub fn IsCatalogOpen(&self) -> bool {
        self.Catalog
    }

    /// MapEditorGlobal.get_UiLock
    pub fn UiLock(&self) -> bool {
        if self.uiLockingRegistrations == 0 && !self.ui.pointer_over_ui && !self.ui.dialog_open {
            return self.ui.mouse_over_emitter;
        }
        true
    }

    /// MapEditorGlobal.RegisterUILock
    pub fn RegisterUILock(&mut self) {
        self.uiLockingRegistrations += 1;
    }

    /// MapEditorGlobal.DeregisterUILock
    pub fn DeregisterUILock(&mut self) {
        self.uiLockingRegistrations -= 1;
        if self.uiLockingRegistrations < 0 {
            super::menu::throw("Exception: Somehow deregistered an unregistered UI lock");
        }
    }

    /// MapEditorGlobal.Update
    pub fn global_update(&mut self, input: &Input) {
        if !self.IsPauseMenuOpen() {
            self.WorldMousePos = self.Camera.screen_to_world_point(self.ui.mouse);
            if input.down(crate::il!(str "MapEditorGlobal::Update" 0)) {
                self.PauseMenu = true;
            }
            if input.down(crate::il!(str "MapEditorGlobal::Update" 1)) {
                self.Catalog = !self.IsCatalogOpen();
            }
        }
    }

    /// MapEditorSelectable.Awake, MapEditorSelectable.Start and MapEditorObjectBehaviour.Awake (the colliders are GetComponentsInChildren, in
    /// `Shape`s; `object` is the behaviour found on the object): the selectable joins Selectables, then LocalBounds is the colliders' bounds at an identity transform plus the origin, centred on it.
    /// Returns the selectable's id.
    pub fn add_selectable(&mut self, transform: Transform, Depth: i32, colliders: Vec<Shape>, object: Option<ObjectBehaviour>) -> usize {
        let id = self.selectables.len();
        self.selectables.push(Selectable { Depth, LocalBounds: Bounds { center: [0.0; 2], extents: [0.0; 2] }, IsSelected: false, transform, colliders, selectionAge: 0, object, alive: true });
        match self.free.pop() {
            Some(s) => self.slots[s] = Some(id),
            None => self.slots.push(Some(id)),
        }
        // Start: MapEditorSelectionManager.UpdateSortedSelectables is a lazy OrderBy (see determine_hovering).
        let mut lo = [0.0f32; 2];
        let mut hi = [0.0f32; 2];
        for c in &self.selectables[id].colliders {
            let (a, b) = c.local_min_max();
            for i in 0..2 {
                lo[i] = mathf::min(lo[i], a[i]);
                hi[i] = mathf::max(hi[i], b[i]);
            }
        }
        self.selectables[id].LocalBounds = Bounds { center: [0.0; 2], extents: [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5] };
        id
    }

    pub fn selectable(&self, id: usize) -> &Selectable {
        &self.selectables[id]
    }

    pub fn selectable_mut(&mut self, id: usize) -> &mut Selectable {
        &mut self.selectables[id]
    }

    /// MapEditorSelectable.MouseOverlaps
    pub fn mouse_overlaps(&self, id: usize) -> bool {
        let s = &self.selectables[id];
        s.colliders.iter().any(|c| c.overlap_point(&s.transform, self.WorldMousePos))
    }

    /// MapEditorSelectable.GetColliders
    pub fn get_colliders(&self, id: usize) -> &[Shape] {
        &self.selectables[id].colliders
    }

    /// MapEditorSelectionManager.get_TopMostHovering
    pub fn TopMostHovering(&self) -> Option<usize> {
        self.HoveringOver.first().copied()
    }

    /// The members of MapEditorSelectable.Selectables in HashSet order.
    fn selectables_in_order(&self) -> Vec<usize> {
        self.slots.iter().flatten().copied().collect()
    }

    /// MapEditorSelectionManager.UpdateSortedSelectables: sortedSelectables is a lazy `OrderBy(Depth)` over the live
    /// set, so every enumeration sees the current depths and members; this is that enumeration (a stable sort).
    pub fn UpdateSortedSelectables(&self) -> Vec<usize> {
        let mut all = self.selectables_in_order();
        all.sort_by_key(|&i| self.selectables[i].Depth);
        all
    }

    /// MapEditorSelectionManager.DetermineHovering
    pub(crate) fn determine_hovering(&mut self) {
        self.HoveringOver.clear();
        self.hoveringHash = 0;
        for id in self.UpdateSortedSelectables() {
            if self.mouse_overlaps(id) {
                self.HoveringOver.push(id);
                self.hoveringHash = self.hoveringHash.wrapping_add(id);
            }
        }
    }

    /// MapEditorSelectionManager.Update
    pub fn selection_update(&mut self, input: &Input) {
        self.determine_hovering();
        if self.UiLock() {
            return;
        }
        if input.down(crate::il!(str "MapEditorSelectionManager::Update" 0)) {
            for &i in &self.CurrentlySelected {
                self.selectables[i].selectionAge += 1;
            }
        }
        if input.up(crate::il!(str "MapEditorSelectionManager::Update" 0)) && input.held("selectMultiple") && !input.down(crate::il!(str "MapEditorSelectionManager::Update" 3)) {
            for i in self.HoveringOver.clone() {
                if self.selectables[i].selectionAge > 1 && self.CurrentlySelected.contains(&i) {
                    self.Deselect(i);
                }
            }
        }
        if input.down(crate::il!(str "MapEditorSelectionManager::Update" 0)) || input.down(crate::il!(str "MapEditorSelectionManager::Update" 3)) {
            if !input.held("selectMultiple") && (!input.down(crate::il!(str "MapEditorSelectionManager::Update" 3)) || self.CurrentlySelected.len() <= 1) {
                self.ClearSelection();
            }
            if let Some(&current) = self.HoveringOver.first() {
                if !self.CurrentlySelected.contains(&current) {
                    self.Select(current).unwrap();
                    self.selectables[current].selectionAge += 1;
                }
            }
        }
        if input.down(crate::il!(str "MapEditorSelectionManager::Update" 3)) {
            self.context_menu_show(self.ui.mouse);
        }
        if self.context_menu_is_shown() && input.down(crate::il!(str "MapEditorSelectionManager::Update" 0)) && !self.ui.selected_in_context_menu {
            self.context_menu_hide();
        }
    }

    /// MapEditorSelectionManager.Select (BroadcastMessage("OnSelect") reaches MapEditorSelectable.OnSelect, which is empty,
    /// then the OnSelect event: MapEditorManipulator.OnSelect).
    pub fn Select(&mut self, id: usize) -> Result<(), String> {
        if self.CurrentlySelected.contains(&id) {
            return Err(crate::il!(str "MapEditorSelectionManager::Select" 0).into());
        }
        self.CurrentlySelected.push(id);
        for _ in 0..self.listeners[0] {
            self.manipulator_on_select();
        }
        Ok(())
    }

    /// MapEditorSelectionManager.Deselect: BroadcastMessage("OnDeselect") reaches the selectable's own OnDeselect.
    pub fn Deselect(&mut self, id: usize) {
        if let Some(at) = self.CurrentlySelected.iter().position(|&x| x == id) {
            self.CurrentlySelected.remove(at);
            self.selectable_on_deselect(id);
        }
        for _ in 0..self.listeners[1] {
            self.manipulator_on_deselect();
        }
    }

    /// MapEditorSelectable.OnDeselect
    pub fn selectable_on_deselect(&mut self, id: usize) {
        self.selectables[id].selectionAge = 0;
    }

    /// MapEditorSelectionManager.ClearSelection
    pub fn ClearSelection(&mut self) {
        for id in self.selectables_in_order() {
            self.Deselect(id);
        }
    }

    /// MapEditorObjectBehaviour.Delete: Object.Destroy(gameObject), which runs at the end of the frame.
    pub fn object_delete(&mut self, id: usize) {
        self.pending_destroy.push(id);
    }

    /// MapEditorSelectable.OnDestroy for each Destroy'd selectable.
    pub(crate) fn flush_destroyed(&mut self) {
        for id in std::mem::take(&mut self.pending_destroy) {
            if !self.selectables[id].alive {
                continue;
            }
            self.selectables[id].alive = false;
            if let Some(s) = self.slots.iter().position(|&x| x == Some(id)) {
                self.slots[s] = None;
                self.free.push(s);
            }
            self.CurrentlySelected.retain(|&x| x != id);
            for _ in 0..self.listeners[1] {
                self.manipulator_on_deselect();
            }
        }
    }

    /// MapEditorContextMenuBehaviour.Show: position zero, ForceRebuildLayoutImmediate (the screen's layout keeps `size_delta` at the
    /// rebuilt size), clampToScreen, then the selection and its MapEditorObjectBehaviours, and SetActive(true).
    pub fn context_menu_show(&mut self, screen_position: [f32; 2]) {
        let ui = self.ui;
        let m = &mut self.context_menu;
        m.position = [0.0; 3];
        // clampToScreen
        let mut vector = screen_position;
        m.position = v3(vector);
        // The divisor stays in double into the inlined Vector2 operator/ (Mono): one rounding per component.
        let scale = d(m.reference_resolution[0]) / mathf::lerp(ui.screen[0], ui.screen[1], m.match_width_or_height);
        let vector2 = [(d(m.size_delta[0] * 2.0) / scale).f(), (d(m.size_delta[1] * 2.0) / scale).f()];
        vector[0] = mathf::clamp(vector[0], 0.0, ui.screen[0] - vector2[0]);
        vector[1] = mathf::clamp(vector[1], vector2[1], ui.screen[1]);
        // transform.position = vector under the overlay canvas: world-to-local through it. The RectTransform keeps it as
        // anchoredPosition = local - anchor reference (anchors (0, 1) in the canvas rect, sizeDelta = screen / scale, pivot
        // 0.5) and rebuilds local = anchoredPosition + reference, then the read-back.
        let canvas = m.canvas(ui.screen);
        let ((local, _), _) = crate::unity::transform::set_world(&[canvas], v3(vector), IDENTITY);
        let a = [-(ui.screen[0] / canvas.s[0]) * 0.5, (ui.screen[1] / canvas.s[1]) * 0.5];
        let local = [(local[0] - a[0]) + a[0], (local[1] - a[1]) + a[1], local[2], 0.0];
        let p = crate::unity::transform::global_position(&canvas, local);
        m.position = [p[0], p[1], p[2]];
        m.selectablesWhenShown = self.CurrentlySelected.clone();
        m.selectedObjects = m.selectablesWhenShown.iter().copied().filter(|&i| self.selectables[i].alive && self.selectables[i].object.is_some()).collect();
        m.shown = true;
    }

    /// MapEditorContextMenuBehaviour.get_IsShown: the menu's GameObject is active.
    pub fn context_menu_is_shown(&self) -> bool {
        self.context_menu.shown
    }

    /// MapEditorContextMenuBehaviour.Update
    pub fn context_menu_update(&mut self) {
        if self.context_menu_is_shown() {
            let interactable = !self.context_menu.selectedObjects.is_empty();
            self.context_menu.DeleteInteractable = interactable;
            self.context_menu.CopyInteractable = interactable;
            self.context_menu.PasteInteractable = false;
        }
    }

    /// MapEditorContextMenuBehaviour.DeleteAction
    pub fn context_menu_delete_action(&mut self) {
        for id in self.context_menu.selectedObjects.clone() {
            self.object_delete(id);
        }
        self.context_menu_hide();
    }

    /// MapEditorContextMenuBehaviour.Hide and MapEditorContextMenuBehaviour.Start (its Hide; the DeleteButton listener is `context_menu_delete_action`).
    pub fn context_menu_hide(&mut self) {
        self.context_menu.shown = false;
    }

    /// MapEditorManipulator.OnSelect
    pub(crate) fn manipulator_on_select(&mut self) {
        let m = &mut self.manipulator;
        if !m.MoveGizmo.active {
            m.MoveGizmo.active = true;
        }
        if !m.ResizeGizmo.active {
            m.ResizeGizmo.active = true;
        }
    }

    /// MapEditorManipulator.OnDeselect
    pub(crate) fn manipulator_on_deselect(&mut self) {
        if self.CurrentlySelected.is_empty() {
            let m = &mut self.manipulator;
            if m.MoveGizmo.active {
                m.MoveGizmo.active = false;
                m.disable_move_handles();
            }
            if m.ResizeGizmo.active {
                m.ResizeGizmo.active = false;
                m.disable_resize_handles();
            }
        }
    }

    /// MapEditorManipulator.OnRenderObject
    pub fn manipulator_on_render_object(&mut self, input: &Input) {
        self.calculate_selection_transform();
        if input.down(crate::il!(str "MapEditorManipulator::OnRenderObject" 0)) {
            self.manipulator.previousMousePos = self.WorldMousePos;
        }
        self.handle_translation(input);
        self.handle_resizing();
        self.manipulator.previousMousePos = self.WorldMousePos;
    }

    /// MapEditorManipulator.HandleTranslation
    fn handle_translation(&mut self, input: &Input) {
        if !self.manipulator.MoveGizmo.active {
            return;
        }
        let selected = self.CurrentlySelected.clone();
        let mouse = self.WorldMousePos;
        let prev = self.manipulator.previousMousePos;
        let mut vector = [mouse[0] - prev[0], mouse[1] - prev[1], 0.0];
        let space_self = input.held("localSpaceTransform") && selected.len() == 1;
        if space_self {
            self.manipulator.MoveGizmo.set_rotation(self.selectables[selected[0]].transform.rotation);
        } else {
            self.manipulator.MoveGizmo.set_rotation(IDENTITY);
        }
        if self.manipulator.AllAxisMoveGizmo.dragged {
            for &i in &selected {
                if self.selectables[i].alive {
                    self.selectables[i].transform.translate_world(vector);
                }
            }
            return;
        }
        if self.manipulator.VerticalMoveGizmo.dragged {
            if space_self {
                vector = project_on_plane(vector, self.selectables[selected[0]].transform.right());
            }
            for &i in &selected {
                if self.selectables[i].alive {
                    let t = [0.0, vector[1], 0.0];
                    if space_self {
                        self.selectables[i].transform.translate_self(t);
                    } else {
                        self.selectables[i].transform.translate_world(t);
                    }
                }
            }
        }
        if !self.manipulator.HorizontalMoveGizmo.dragged {
            return;
        }
        if space_self {
            vector = project_on_plane(vector, self.selectables[selected[0]].transform.up());
        }
        for &i in &selected {
            if self.selectables[i].alive {
                let t = [vector[0], 0.0, 0.0];
                if space_self {
                    self.selectables[i].transform.translate_self(t);
                } else {
                    self.selectables[i].transform.translate_world(t);
                }
            }
        }
    }

    /// The four corners of a selectable's LocalBounds in world space: (min.x,max.y), (max.x,max.y), (min.x,min.y), (max.x,min.y).
    fn bounds_corners(&self, id: usize) -> [[f32; 3]; 4] {
        let s = &self.selectables[id];
        let (lo, hi) = (bmin(&s.LocalBounds), bmax(&s.LocalBounds));
        [
            s.transform.transform_point(v3([lo[0], hi[1]])),
            s.transform.transform_point(v3([hi[0], hi[1]])),
            s.transform.transform_point(v3([lo[0], lo[1]])),
            s.transform.transform_point(v3([hi[0], lo[1]])),
        ]
    }

    /// MapEditorManipulator.HandleResizing
    pub(crate) fn handle_resizing(&mut self) {
        if !self.manipulator.ResizeGizmo.active {
            return;
        }
        let selected = self.CurrentlySelected.clone();
        if selected.len() == 1 {
            self.handle_resize_single_object(selected[0]);
            return;
        }
        if selected.len() > 1 {
            let center = self.manipulator.CenterOfSelection;
            self.manipulator.ResizeGizmo.set_position(v3(center));
            let mut num = f32::MAX;
            let mut num2 = f32::MIN;
            let mut num3 = f32::MAX;
            let mut num4 = f32::MIN;
            for &i in &selected {
                let [a, b, c, e] = self.bounds_corners(i);
                num = mathf::min(num, mathf::min(mathf::min(a[0], b[0]), mathf::min(c[0], e[0])));
                num3 = mathf::min(num3, mathf::min(mathf::min(a[1], b[1]), mathf::min(c[1], e[1])));
                num2 = mathf::max(num2, mathf::max(mathf::max(a[0], b[0]), mathf::max(c[0], e[0])));
                num4 = mathf::max(num4, mathf::max(mathf::max(a[1], b[1]), mathf::max(c[1], e[1])));
            }
            let num5 = (num4 - num3).abs();
            let num6 = (num2 - num).abs();
            if num5.is_nan() || num5.is_infinite() || num6.is_nan() || num6.is_infinite() {
                return;
            }
            let v5 = [num, num4, 0.0];
            let v6 = [num2, num4, 0.0];
            let v7 = [num, num3, 0.0];
            let v8 = [num2, num3, 0.0];
            let half = |a: [f32; 3], b: [f32; 3]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
            let m = &mut self.manipulator;
            let gz = m.ResizeGizmo.local();
            m.TopResizeHandle.set_position_and_rotation(gz, half(v6, v5), IDENTITY);
            m.RightResizeHandle.set_position_and_rotation(gz, half(v6, v8), IDENTITY);
            m.BottomResizeHandle.set_position_and_rotation(gz, half(v7, v8), IDENTITY);
            m.LeftResizeHandle.set_position_and_rotation(gz, half(v7, v5), IDENTITY);
            let num7 = self.Camera.orthographicSize * 0.01;
            let m = &mut self.manipulator;
            m.TopResizeHandle.scale = [num6, num7, 0.0];
            m.BottomResizeHandle.scale = [num6, num7, 0.0];
            m.RightResizeHandle.scale = [num7, num5, 0.0];
            m.LeftResizeHandle.scale = [num7, num5, 0.0];
            let mouse = self.WorldMousePos;
            let prev = m.previousMousePos;
            let vector9 = [mouse[0] - prev[0], mouse[1] - prev[1], 0.0];
            let magnitude = magnitude3(vector9);
            let num8 = (vector9[0] + vector9[1]) / 2.0;
            if !m.RightResizeHandle.dragged && !m.TopResizeHandle.dragged && !m.BottomResizeHandle.dragged && !m.LeftResizeHandle.dragged {
                return;
            }
            for &i in &selected {
                let s = &mut self.selectables[i];
                let size = bsize(&s.LocalBounds);
                let mut scale = s.transform.scale;
                scale[0] = (d(scale[0]) + d(magnitude) / size[0] * 2.0).f();
                scale[1] = (d(scale[1]) + d(magnitude) / size[1] * 2.0).f();
                scale[0] = mathf::clamp(scale[0], 0.1, 1000.0);
                scale[1] = mathf::clamp(scale[1], 0.1, 1000.0);
                s.transform.scale = scale;
                let v11 = [s.transform.position[0] - center[0], s.transform.position[1] - center[1]];
                let v10 = [(d(0.0) + d(num8) * v11[0] / 2.0).f(), (d(0.0) + d(num8) * v11[1] / 2.0).f(), 0.0];
                let p = add3(s.transform.position, v10);
                s.transform.set_position(p);
            }
            return;
        }
        self.manipulator.ResizeGizmo.active = false;
        self.manipulator.disable_resize_handles();
    }

    /// MapEditorManipulator.HandleResizeSingleObject
    pub(crate) fn handle_resize_single_object(&mut self, id: usize) {
        let [v, v2, v3_, v4] = self.bounds_corners(id);
        let (position, rotation) = (self.selectables[id].transform.position, self.selectables[id].transform.rotation);
        self.manipulator.ResizeGizmo.set_position(position);
        let magnitude = magnitude3(sub3(v, v3_));
        let magnitude2 = magnitude3(sub3(v3_, v4));
        let half = |a: [f32; 3], b: [f32; 3]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
        let num = self.Camera.orthographicSize * 0.01;
        let m = &mut self.manipulator;
        let gz = m.ResizeGizmo.local();
        m.TopResizeHandle.set_position_and_rotation(gz, half(v2, v), rotation);
        m.RightResizeHandle.set_position_and_rotation(gz, half(v2, v4), rotation);
        m.BottomResizeHandle.set_position_and_rotation(gz, half(v3_, v4), rotation);
        m.LeftResizeHandle.set_position_and_rotation(gz, half(v, v3_), rotation);
        m.TopResizeHandle.scale = [magnitude2, num, 0.0];
        m.BottomResizeHandle.scale = [magnitude2, num, 0.0];
        m.RightResizeHandle.scale = [num, magnitude, 0.0];
        m.LeftResizeHandle.scale = [num, magnitude, 0.0];
        let mouse = self.WorldMousePos;
        let prev = m.previousMousePos;
        let vector5 = [mouse[0] - prev[0], mouse[1] - prev[1], 0.0];
        let (right, top, left, bottom) = (m.RightResizeHandle.dragged, m.TopResizeHandle.dragged, m.LeftResizeHandle.dragged, m.BottomResizeHandle.dragged);
        let s = &mut self.selectables[id];
        let size = bsize(&s.LocalBounds);
        // (axis, sign, direction vector)
        let step = if right {
            Some((0, 1.0, [1.0f32, 0.0, 0.0]))
        } else if top {
            Some((1, 1.0, [0.0, 1.0, 0.0]))
        } else if left {
            Some((0, -1.0, [1.0, 0.0, 0.0]))
        } else if bottom {
            Some((1, -1.0, [0.0, 1.0, 0.0]))
        } else {
            None
        };
        if let Some((axis, sign, dir)) = step {
            let mut scale = s.transform.scale;
            scale[axis] = if sign > 0.0 { (d(scale[axis]) + d(vector5[axis]) / size[axis]).f() } else { (d(scale[axis]) - d(vector5[axis]) / size[axis]).f() };
            scale[axis] = mathf::clamp(scale[axis], 0.1, 1000.0);
            s.transform.scale = scale;
            let delta = [dir[0] * vector5[axis] / 2.0, dir[1] * vector5[axis] / 2.0, dir[2] * vector5[axis] / 2.0];
            let p = add3(s.transform.position, delta);
            s.transform.set_position(p);
        }
    }

    /// MapEditorManipulator.CalculateSelectionTransform
    pub(crate) fn calculate_selection_transform(&mut self) {
        let mut num = f32::MAX;
        let mut num2 = f32::MIN;
        let mut num3 = f32::MAX;
        let mut num4 = f32::MIN;
        for &i in &self.CurrentlySelected {
            let s = &self.selectables[i];
            if s.alive {
                let p = s.transform.position;
                num = mathf::min(p[0], num);
                num2 = mathf::max(p[0], num2);
                num3 = mathf::min(p[1], num3);
                num4 = mathf::max(p[1], num4);
            }
        }
        let center = [(num + num2) * 0.5, (num3 + num4) * 0.5];
        self.manipulator.CenterOfSelection = center;
        let camera = self.Camera;
        // One double-evaluated expression (Mono), rounded at the store.
        let num5 = (d(170.66667) * (d(camera.pixelHeight) / 1080.0)).f();
        let vector = camera.screen_to_world_point([(if self.ui.catalog_active { self.ui.catalog_width } else { 0.0 }) + num5, 0.0 + num5]);
        let vector2 = camera.screen_to_world_point([camera.pixelWidth - num5, camera.pixelHeight - num5]);
        let x = mathf::clamp(center[0], vector[0], vector2[0]);
        let y = mathf::clamp(center[1], vector[1], vector2[1]);
        if self.manipulator.MoveGizmo.active {
            self.manipulator.MoveGizmo.set_position([x, y, 0.0]);
        }
    }

    /// One frame of the editor's behaviours, then the end-of-frame destroys: the Updates, the camera's LateUpdate, the
    /// manipulator's OnRenderObject and the gizmo drawer's OnPostRender.
    pub fn update(&mut self, input: &Input) {
        self.global_update(input);
        self.selection_update(input);
        self.context_menu_update();
        self.panel_update();
        self.camera_late_update(input);
        self.manipulator_on_render_object(input);
        self.gizmo_on_post_render();
        self.flush_destroyed();
    }
}

impl Game {
    /// Runs the map editor's frame when an editor screen created one.
    pub fn map_editor_update(&mut self) {
        if let Some(mut e) = self.map_editor.take() {
            e.update(&self.input);
            self.map_editor = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tool::Control;

    fn press(input: &mut Input, names: &[&str]) {
        input.controls.clear();
        for n in names {
            input.controls.insert(n.to_string(), Control { down: true, held: true, up: false });
        }
    }

    fn editor() -> (MapEditor, usize) {
        let mut e = MapEditor::new(EditorCamera::default());
        let id = e.add_selectable(Transform::default(), 0, vec![Shape::Box { center: [0.0; 2], size: [2.0, 2.0] }], Some(ObjectBehaviour::default()));
        e.ui.screen = [1920.0, 1080.0];
        e.ui.mouse = [960.0, 540.0];
        (e, id)
    }

    /// A click on an object selects it, shows the gizmos, and a click on empty ground clears it.
    #[test]
    fn click_selects_and_clears() {
        let (mut e, id) = editor();
        let mut input = Input::default();
        press(&mut input, &["drag"]);
        e.update(&input);
        assert_eq!(e.CurrentlySelected, vec![id]);
        assert!(e.manipulator.MoveGizmo.active && e.manipulator.ResizeGizmo.active);
        e.ui.mouse = [100.0, 100.0];
        e.update(&input);
        assert!(e.CurrentlySelected.is_empty() && !e.manipulator.MoveGizmo.active);
    }

    /// Dragging the all-axis handle follows the mouse; Delete from the context menu removes the object at the frame's end.
    #[test]
    fn drag_moves_and_delete_destroys() {
        let (mut e, id) = editor();
        let mut input = Input::default();
        press(&mut input, &["drag"]);
        e.update(&input);
        input.controls.clear();
        e.manipulator.AllAxisMoveGizmo.dragged = true;
        e.ui.mouse = [960.0 + 108.0, 540.0];
        e.update(&input);
        assert!((e.selectable(id).transform.position[0] - 1.0).abs() < 1e-4, "{:?}", e.selectable(id).transform.position);
        e.context_menu_show([10.0, 10.0]);
        e.context_menu_update();
        assert!(e.context_menu.DeleteInteractable);
        e.context_menu_delete_action();
        assert!(!e.context_menu.shown);
        e.update(&input);
        assert!(!e.selectable(id).alive && e.CurrentlySelected.is_empty());
    }
}
