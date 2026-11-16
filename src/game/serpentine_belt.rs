//! SerpentineBeltTool and the SerpentineBeltBehaviour it adds: a belt from the selected object to the one under the mouse
//! that couples their spins by the ratio of their scaled circumferences. The tool's line, the belt's convex-hull line
//! (UpdateBelt only feeds the LineRenderer and the hover test), the ToolInvisibilityTracker's outline material and the
//! statistic are presentation.
#![allow(non_snake_case)]

use super::undo::ObjectCreationAction;
use super::{mathf, Behaviour, Game};

/// SerpentineBeltTool.WireWidth (static).
pub(crate) const WIRE_WIDTH: f32 = 0.03;

/// One SerpentineBeltBehaviour on `Self_`'s GameObject.
#[derive(Clone, Debug)]
pub struct Belt {
    /// The component's instance id.
    pub id: i32,
    pub Self_: usize,
    pub Other: usize,
    pub gearRatio: f32,
    pub lastScale: f32,
    pub started: bool,
    /// Hover.Invisible: ToolVisibility == Invisible when it was laid.
    pub Invisible: bool,
    /// The objects' collider grid points in world space (RecalculatePointArray); ReallocatePointArray sizes it, zeroed.
    pub worldPointArray: Vec<[f32; 2]>,
    /// The line's positions once UpdateBelt has set them (the convex hull).
    pub hull: Option<Vec<[f32; 2]>>,
    /// LineRenderer (lineRendererGm's) is alive: Awake makes it, OnDestroy destroys it.
    pub line_renderer: bool,
}

impl Default for Belt {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Self_: Default::default(),
            Other: Default::default(),
            gearRatio: 1.0,
            lastScale: -1.0,
            started: Default::default(),
            Invisible: Default::default(),
            line_renderer: true,
            worldPointArray: Default::default(),
            hull: Default::default(),
        }
    }
}

/// Utils.ComputeConvexHull: Array.Sort by x descending with a comparer that never returns 0 (mscorlib's
/// IntrospectiveSort over the array, in place: the caller's array is left sorted), then the monotone chain; ccw is a
/// double expression rounded at its return.
pub fn convex_hull(points: &mut [[f32; 2]]) -> Vec<[f32; 2]> {
    let mut keys: Vec<i32> = (0..points.len() as i32).collect();
    let cmp = |a: i32, b: i32| if !(points[a as usize][0] > points[b as usize][0]) { 1 } else { -1 };
    super::map_lift::list_sort(&mut keys, &mut [], &cmp);
    let p: Vec<[f32; 2]> = keys.iter().map(|&k| points[k as usize]).collect();
    points.copy_from_slice(&p);
    let ccw = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        ((b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64) - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64)) as f32
    };
    let n = p.len();
    let mut hull: Vec<[f32; 2]> = Vec::new();
    for &q in &p {
        while hull.len() >= 2 && ccw(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
            hull.pop();
        }
        hull.push(q);
    }
    let lower = hull.len() + 1;
    for i in (0..n.saturating_sub(1)).rev() {
        while hull.len() >= lower && ccw(hull[hull.len() - 2], hull[hull.len() - 1], p[i]) <= 0.0 {
            hull.pop();
        }
        hull.push(p[i]);
    }
    hull
}

/// The SerpentineBeltTool on the ToolController.
#[derive(Clone, Debug, Default)]
pub struct BeltTool {
    pub chosen: bool,
    pub created: bool,
    pub startPos: [f32; 2],
    pub startOffset: [f32; 2],
    pub hasDragged: bool,
}

impl Game {
    // SerpentineBeltTool.OnToolUnchosen: empty.
    // SerpentineBeltTool.OnFixedHold: empty.
    /// SerpentineBeltTool.OnToolChosen
    /// SetTool: AddComponent<SerpentineBeltTool> the first time, then OnToolChosen (the LineRenderer the wire tools share).
    pub(crate) fn belt_tool_choose(&mut self) {
        if !self.belt_tool.created {
            self.allocate_instance_id();
            self.belt_tool.created = true;
        }
        if !self.wire_tool.has_line {
            self.allocate_instance_id();
            self.wire_tool.has_line = true;
        }
        self.belt_tool.chosen = true;
    }

    /// SerpentineBeltTool.OnSelect
    pub(crate) fn belt_on_select(&mut self) {
        let m = self.MousePosition;
        self.belt_tool.startPos = [m[0], m[1]];
        if let Some(sel) = self.wire_selected() {
            let o = self.inverse_transform_point(sel, [m[0], m[1], 0.0]);
            self.belt_tool.startOffset = [o[0], o[1]];
        }
        self.belt_line_renderer();
        self.belt_tool.hasDragged = true;
    }

    /// SerpentineBeltTool.OnHold
    pub(crate) fn belt_on_hold(&mut self) {
        if let Some(sel) = self.wire_selected() {
            let o = self.belt_tool.startOffset;
            let p = self.transform_point3(sel, [o[0], o[1], 0.0]);
            self.belt_tool.startPos = [p[0], p[1]];
        }
        self.belt_line_renderer();
    }

    /// lineRenderer.SetPositions in OnSelect/OnHold: the field is null until the first OnToolChosen (`created`), as on a
    /// tool component added without SetTool.
    fn belt_line_renderer(&self) {
        if !self.belt_tool.created {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
    }

    /// SerpentineBeltTool.OnDeselect: GetEndObject is CurrentlyUnderMouse unless it is on layer 11.
    pub(crate) fn belt_on_deselect(&mut self) {
        if !self.belt_tool.hasDragged {
            return;
        }
        // SerpentineBeltTool.GetEndObject (link_end_body).
        let end_body = self.link_end_body();
        if let (Some(a), Some(b)) = (self.wire_selected(), end_body) {
            // SerpentineBeltBehaviour.Awake: AddComponent; it instantiates Prefabs/SerpentineBelt under it (GameObject, Transform,
            // LineRenderer, Optout; its colour and width are presentation). Invisible = ToolVisibility == Invisible.
            let id = self.allocate_instance_id();
            for _ in 0..4 {
                self.allocate_instance_id();
            }
            self.belts.push(Belt { id, Self_: a, Other: b, gearRatio: 1.0, lastScale: -1.0, started: false, Invisible: self.tool.as_ref().unwrap().invisible, worldPointArray: Vec::new(), hull: None, line_renderer: true });
            let l = self.belts.len() - 1;
            let behaviour = Behaviour::Belt(l);
            // SerpentineBeltBehaviour's MonoScript execution order is 0.
            self.behaviour_of.insert(id, (0, behaviour));
            self.behaviours.add(0, behaviour);
            self.pending_starts.push(super::misc::PendingStart::Belt(l));
            super::stats::with_stats(|s| s.increment("WIRES_LAID", 1.0));
            self.undo_register_action(ObjectCreationAction { displayName: crate::il!(str "SerpentineBeltTool::OnDeselect" 1).into(), RelevantObjects: vec![id] });
        }
        self.belt_tool.hasDragged = false;
    }

    /// PhysicalBehaviour.GetScaledCircumference
    pub(crate) fn scaled_circumference(&self, p: usize) -> f32 {
        let s = self.physicals[p].transform.localScale;
        self.physicals[p].Circumference.abs() * mathf::sqrt(s[0].abs() * s[1].abs())
    }

    /// SerpentineBeltBehaviour.RecalculateGearRatio
    pub(crate) fn belt_recalculate_gear_ratio(&mut self, l: usize) {
        let (a, b) = (self.belts[l].Self_, self.belts[l].Other);
        let r = mathf::max(0.01, self.scaled_circumference(a)) / mathf::max(0.01, self.scaled_circumference(b));
        self.belts[l].gearRatio = mathf::clamp(r, 0.01, 1000.0);
    }

    /// SerpentineBeltBehaviour.Start
    pub fn belt_start(&mut self, l: usize) {
        if self.destroyed_components.contains(&self.belts[l].id) {
            return;
        }
        self.belt_reallocate_point_array(l);
        self.belt_recalculate_gear_ratio(l);
        self.belts[l].started = true;
    }

    fn belt_alive(&self, l: usize) -> bool {
        let x = &self.belts[l];
        !self.physicals[x.Self_].destroyed && !self.physicals[x.Other].destroyed
    }

    /// SerpentineBeltBehaviour.Update
    pub fn belt_update(&mut self, l: usize) {
        if !self.belts[l].started {
            return;
        }
        if !self.belt_alive(l) {
            let id = self.belts[l].id;
            self.tr_destroy_component(id, 0.0);
            return;
        }
        let (a, b) = (self.belts[l].Self_, self.belts[l].Other);
        if a != b {
            let sqr = |s: [f32; 3]| (super::d(s[0]) * s[0] + super::d(s[1]) * s[1] + super::d(s[2]) * s[2]).f();
            let scale = sqr(self.physicals[a].transform.localScale) * sqr(self.physicals[b].transform.localScale);
            if !mathf::approximately(scale, self.belts[l].lastScale) {
                self.belt_recalculate_gear_ratio(l);
                self.belts[l].lastScale = scale;
            }
            self.belt_update_belt(l);
        }
        self.belt_check_mouse_input(l);
    }

    fn belt_grid_len(&self, p: usize) -> usize {
        // PhysicalBehaviour.LocalGridPointLength
        self.physicals[p].LocalColliderGridPoints.as_ref().map_or(0, Vec::len)
    }

    /// SerpentineBeltBehaviour.ReallocatePointArray (only worldPointArray's length is read).
    pub(crate) fn belt_reallocate_point_array(&mut self, l: usize) {
        let n = if !self.belt_alive(l) { 0 } else { self.belt_grid_len(self.belts[l].Self_) + self.belt_grid_len(self.belts[l].Other) };
        self.belts[l].worldPointArray = vec![[0.0; 2]; n];
    }

    /// SerpentineBeltBehaviour.RecalculatePointArray: both objects' collider grid points in world space.
    pub(crate) fn belt_recalculate_point_array(&mut self, l: usize) {
        let (a, b) = (self.belts[l].Self_, self.belts[l].Other);
        if self.belts[l].worldPointArray.len() != self.belt_grid_len(a) + self.belt_grid_len(b) {
            self.belt_reallocate_point_array(l);
        }
        let mut points: Vec<[f32; 2]> = Vec::with_capacity(self.belts[l].worldPointArray.len());
        for p in [a, b] {
            for v in self.physicals[p].LocalColliderGridPoints.clone().unwrap_or_default() {
                let w = self.transform_point3(p, [v[0], v[1], 0.0]);
                points.push([w[0], w[1]]);
            }
        }
        self.belts[l].worldPointArray = points;
    }

    /// SerpentineBeltBehaviour.UpdateBelt: RecalculatePointArray, then Utils.ComputeConvexHull into the line's
    /// positions. Only the hover test reads them.
    pub(crate) fn belt_update_belt(&mut self, l: usize) {
        if self.belts[l].worldPointArray.len() > 3 {
            self.belt_recalculate_point_array(l);
            self.belts[l].hull = Some(convex_hull(&mut self.belts[l].worldPointArray));
        }
    }

    /// Hover.CheckMouseInput of the belt (no dialog, UI block or pause menu is ever open): "delete" held over it
    /// destroys it. CurrentlyHovering and the outline events are the context menu's and presentation.
    fn belt_check_mouse_input(&mut self, l: usize) {
        if !self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) || !self.tool_should_be_visible(self.belts[l].Invisible) {
            return;
        }
        // linePositionBuffer is still empty while UpdateBelt has not filled it (Self == Other, or at most 3 points):
        // IsMouseInsideCollider walks no segment and is false, so nothing is deleted.
        if self.belts[l].hull.is_none() {
            return;
        }
        let m = self.MousePosition;
        // IsPointInsideBounds is strict, on x and y.
        let Ok((c, e)) = self.belt_get_visual_bounds(l) else { return };
        let inside = (0..2).all(|i| m[i] > c[i] - e[i] && m[i] < c[i] + e[i]);
        let on = inside && self.belt_is_mouse_inside_collider(l);
        // SerpentineBeltBehaviour.CanBeDeleted: ShouldBeVisible; Object.Destroy(this).
        // SerpentineBeltBehaviour.OnUserDelete: empty.
        if on {
            let id = self.belts[l].id;
            self.tr_destroy_component(id, 0.0);
        }
    }

    /// SerpentineBeltBehaviour.GetVisualBounds: LineRenderer.bounds (LineRenderer::CalculateWorldMatrixAndBoundsJob, world
    /// space): centre (max + min) * 0.5, extents (max - min) * 0.5 + widthMultiplier (the prefab's 0.07; SetStartWidth
    /// only rescales the curve) * 0.5 on all three axes (the line's z is 0). A destroyed renderer throws
    /// (Renderer.get_bounds_Injected). A renderer before UpdateBelt set its positions is not reached.
    pub(crate) fn belt_get_visual_bounds(&self, l: usize) -> Result<([f32; 3], [f32; 3]), String> {
        if !self.belts[l].line_renderer {
            return Err("NullReferenceException".into());
        }
        let Some(line) = self.belts[l].hull.as_ref() else { return Err("unreached: a belt line without positions".into()) };
        let mut c = [0.0f32; 3];
        let mut e = [0.0f32; 3];
        for i in 0..2 {
            let (lo, hi) = line.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[i]), hi.max(p[i])));
            (c[i], e[i]) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + 0.07 * 0.5);
        }
        e[2] = 0.07 * 0.5;
        Ok((c, e))
    }

    /// SerpentineBeltBehaviour.OnDestroy: InvisibilityTracker.Dispose (no tracker here), Hover.OnDestroy (CurrentlyHovering
    /// cleared when it is this one), Destroy(lineRendererGm).
    pub(crate) fn belt_on_destroy(&mut self, l: usize) {
        if self.hovering == Some(self.belts[l].id) {
            self.hovering = None;
        }
        self.belts[l].line_renderer = false;
    }

    /// SerpentineBeltBehaviour.get_ShouldEnableRenderer
    pub(crate) fn belt_should_enable_renderer(&self, l: usize) -> bool {
        !self.belts[l].Invisible && self.belts[l].worldPointArray.len() >= 3
    }

    /// SerpentineBeltBehaviour.IsMouseInsideCollider: the mouse within WireWidth + 0.02 of a segment of the line
    /// (linePositionBuffer, closed); an empty buffer has no segment. `num * num` stays a double in the comparison.
    pub(crate) fn belt_is_mouse_inside_collider(&self, l: usize) -> bool {
        let Some(line) = self.belts[l].hull.as_ref() else { return false };
        let m = self.MousePosition;
        let num = 0.03f32 + 0.02f32;
        let n = line.len();
        (0..n).any(|i| {
            let d = super::blood_wire::sqrd_distance_to_segment([m[0], m[1]], line[i], line[(i + 1) % n]);
            d.abs() as f64 <= num as f64 * num as f64
        })
    }

    /// SerpentineBeltBehaviour.FixedUpdate
    pub fn belt_fixed_update(&mut self, l: usize) {
        if !self.belts[l].started || !self.belt_alive(l) {
            return;
        }
        let (a, b, ratio) = (self.belts[l].Self_, self.belts[l].Other, self.belts[l].gearRatio);
        if a == b {
            return;
        }
        let num = self.physicals[a].rigidbody.angular_velocity(&self.world) * 0.9999;
        let num2 = self.physicals[b].rigidbody.angular_velocity(&self.world) * 0.9999;
        let w = self.physicals[a].rigidbody.angular_velocity(&self.world);
        let v = (w as f64 + 0.5 * (num2 as f64 / ratio as f64 - num as f64)) as f32;
        self.physicals[a].rigidbody.set_angular_velocity(&mut self.world, v);
        let w = self.physicals[b].rigidbody.angular_velocity(&self.world);
        let v = (w as f64 + 0.5 * (num as f64 * ratio as f64 - num2 as f64)) as f32;
        self.physicals[b].rigidbody.set_angular_velocity(&mut self.world, v);
    }

    /// The delayed Destroy of a SerpentineBeltBehaviour (false: `id` is not one). SerpentineBeltBehaviour.OnDestroy destroys its line object.
    pub fn belt_run_destroy(&mut self, id: i32) -> bool {
        if !self.belts.iter().any(|x| x.id == id) {
            return false;
        }
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            if let Some((_, b)) = self.behaviour_of.remove(&id) {
                self.behaviours.remove(b);
            }
        }
        true
    }

    /// The belts on a GameObject that is being destroyed.
    pub(super) fn belt_object_destroy(&mut self, components: &[i32]) {
        for l in 0..self.belts.len() {
            let id = self.belts[l].id;
            if components.contains(&self.physicals[self.belts[l].Self_].id) && self.destroyed_components.insert(id) {
                self.component_destroys.retain(|&(_, i)| i != id);
                if let Some((_, b)) = self.behaviour_of.remove(&id) {
                    self.behaviours.remove(b);
                }
            }
        }
    }
}
