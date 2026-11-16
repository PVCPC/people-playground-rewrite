//! SmoothLineControllerBehaviour: a cubic Bezier from the From node to the To node, written to its LineRenderer as
//! `Steps` points (z = 0); AutoLength sets Length to the nodes' distance, AutoAdjustWidth the width to the smallest local
//! scale. The Physics Gun (physics_gun.rs) initialises it, hides it and calls UpdateLine.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};
use crate::unity::transform::{global_position, global_rotation, rotate_direction, set_world, Trs};

/// SmoothLineControllerBehaviour.Node: Transform, DirectionTarget (the Transforms' component ids, None: null) and
/// LocalDirection.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub transform: Option<i32>,
    pub direction_target: Option<i32>,
    pub local_direction: [f32; 2],
}

script! {
    SmoothLine {
        Length: f32 = 1.0,
        Steps: i32 = 8,
        AutoLength: bool = false,
        AutoAdjustWidth: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// LineRenderer: its `Game::lines` entry.
        line: usize = 0,
        From: Node = Node::default(),
        To: Node = Node::default(),
        /// vertices
        vertices: Vec<[f32; 3]> = Vec::new(),
        /// Start has run.
        started: bool = false,
    }
}

impl Game {
    /// The Transform with component id `id`, leaf first: its local TRS and each parent's; a physical object's own
    /// pose and its parents come from the live body (`local_chain`).
    pub(super) fn transform_chain(&self, inst: &super::spawn::Instance, o: usize) -> Vec<Trs> {
        let mut out = Vec::new();
        let mut at = Some(o);
        while let Some(x) = at {
            if let Some(p) = (0..self.physicals.len()).find(|&p| !self.physicals[p].destroyed && inst.objects[x].components.contains(&self.physicals[p].id)) {
                out.extend(self.local_chain(p).iter().rev().copied());
                return out;
            }
            out.push(inst.objects[x].trs);
            at = inst.objects[x].parent;
        }
        out
    }

    /// Transform.position of the Transform with component id `id`.
    pub(super) fn transform_position_of(&mut self, id: i32) -> [f32; 3] {
        self.with_component_instance(id, |g, inst, o| {
            let chain = g.transform_chain(inst, o);
            let mut p = chain[0].t;
            for n in &chain[1..] {
                p = global_position(n, p);
            }
            [p[0], p[1], p[2]]
        })
    }

    /// Transform.TransformDirection(direction) of the Transform with component id `id`.
    fn transform_direction_of(&mut self, id: i32, direction: [f32; 3]) -> [f32; 3] {
        self.with_component_instance(id, |g, inst, o| {
            let chain = g.transform_chain(inst, o);
            let mut v = [direction[0], direction[1], direction[2], 0.0];
            for (i, node) in chain.iter().enumerate() {
                v = rotate_direction(node.q, (i > 0).then_some(node.s), v);
            }
            [v[0], v[1], v[2]]
        })
    }

    /// Transform.localScale of the Transform with component id `id`.
    fn transform_local_scale_of(&mut self, id: i32) -> [f32; 3] {
        self.with_component_instance(id, |g, inst, o| {
            let chain = g.transform_chain(inst, o);
            [chain[0].s[0], chain[0].s[1], chain[0].s[2]]
        })
    }

    /// Transform.position = p: the local position that puts the Transform there, its world rotation kept.
    pub(super) fn set_transform_position_of(&mut self, id: i32, position: [f32; 3]) {
        self.with_component_instance(id, |g, inst, o| {
            let chain = g.transform_chain(inst, o);
            let mut q = chain[0].q;
            for n in &chain[1..] {
                q = global_rotation(n, q);
            }
            let ((local, _), _) = set_world(&chain[1..], position, q);
            inst.set_local_position(o, local);
        });
    }

    /// `(bool)transform`: false once the Transform's GameObject was destroyed.
    fn transform_alive(&self, id: Option<i32>) -> Option<i32> {
        id.filter(|i| !self.destroyed_components.contains(i))
    }

    /// Node.get_GlobalDirection (SmoothLineControllerBehaviour.Node)
    fn smooth_line_global_direction(&mut self, node: &Node) -> [f32; 3] {
        let transform = node.transform.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let transform = self.transform_alive(Some(transform)).unwrap_or_else(|| super::menu::throw("MissingReferenceException: The object of type 'Transform' has been destroyed"));
        if let Some(target) = self.transform_alive(node.direction_target) {
            let a = self.transform_position_of(target);
            let b = self.transform_position_of(transform);
            return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        }
        self.transform_direction_of(transform, [node.local_direction[0], node.local_direction[1], 0.0])
    }

    /// SmoothLineControllerBehaviour.Initialise: vertices = new Vector3[Steps]; LineRenderer.positionCount = Steps.
    pub fn smooth_line_initialise(&mut self, s: usize) {
        let steps = self.smooth_lines[s].Steps;
        if steps < 0 {
            super::menu::throw("OverflowException: Arithmetic operation resulted in an overflow.");
        }
        self.smooth_lines[s].vertices = vec![[0.0; 3]; steps as usize];
        let line = self.smooth_lines[s].line;
        self.line_set_position_count(line, steps as usize);
    }

    /// SmoothLineControllerBehaviour.Start
    pub fn smooth_line_start(&mut self, s: usize) {
        self.smooth_lines[s].started = true;
        self.smooth_line_initialise(s);
        self.smooth_line_update_line(s);
    }

    /// SmoothLineControllerBehaviour.Update
    pub fn smooth_line_update(&mut self, s: usize) {
        self.smooth_line_update_line(s);
    }

    /// SmoothLineControllerBehaviour.UpdateLine
    pub fn smooth_line_update_line(&mut self, s: usize) {
        let (from, to) = (self.smooth_lines[s].From.clone(), self.smooth_lines[s].To.clone());
        let (from_t, to_t) = (self.transform_alive(from.transform), self.transform_alive(to.transform));
        if self.smooth_lines[s].AutoLength {
            if let (Some(a), Some(b)) = (from_t, to_t) {
                // Vector2.Distance
                let (a, b) = (self.transform_position_of(a), self.transform_position_of(b));
                let (x, y) = (a[0] - b[0], a[1] - b[1]);
                self.smooth_lines[s].Length = (d(x) * x + d(y) * y).0.sqrt() as f32;
            }
        }
        self.smooth_line_calculate_vertices(s, &from, &to);
        let line = self.smooth_lines[s].line;
        let vertices = std::mem::take(&mut self.smooth_lines[s].vertices);
        self.line_set_positions(line, &vertices);
        self.smooth_lines[s].vertices = vertices;
        if self.smooth_lines[s].AutoAdjustWidth {
            if let (Some(a), Some(b)) = (from_t, to_t) {
                let (a, b) = (self.transform_local_scale_of(a), self.transform_local_scale_of(b));
                self.lines[line].width = mathf::min(mathf::min(a[0], a[1]), mathf::min(b[0], b[1]));
            }
        }
    }

    /// SmoothLineControllerBehaviour.CalculateVertices
    pub(crate) fn smooth_line_calculate_vertices(&mut self, s: usize, from: &Node, to: &Node) {
        let num = self.smooth_lines[s].Length / 2.0;
        let null = || super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        let from_t = from.transform.unwrap_or_else(null);
        let to_t = to.transform.unwrap_or_else(null);
        let b = self.transform_position_of(from_t);
        let fd = self.smooth_line_global_direction(from);
        let b2 = [b[0] + fd[0] * num, b[1] + fd[1] * num];
        let to_p = self.transform_position_of(to_t);
        let td = self.smooth_line_global_direction(to);
        let b3 = [to_p[0] + td[0] * num, to_p[1] + td[1] * num];
        let b4 = to_p;
        let steps = self.smooth_lines[s].Steps;
        if self.smooth_lines[s].vertices.len() < steps.max(0) as usize {
            // vertices[i] beyond the array: IndexOutOfRangeException
            super::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
        }
        for i in 0..steps.max(0) as usize {
            let t = i as f32 / (steps - 1) as f32;
            // SmoothLineControllerBehaviour.GetBezierPoint: inlined (float operands in double, each local stored as float).
            let num = 1.0f32 - t;
            let num2 = (d(num) * num * num).f();
            let num3 = (d(t) * num * num).f();
            let num4 = (d(t) * t * (d(1.0) - t)).f();
            let num5 = (d(t) * t * t).f();
            let x = (d(b[0]) * num2 + d(3.0) * b2[0] * num3 + d(3.0) * b3[0] * num4 + d(b4[0]) * num5).f();
            let y = (d(b[1]) * num2 + d(3.0) * b2[1] * num3 + d(3.0) * b3[1] * num4 + d(b4[1]) * num5).f();
            self.smooth_lines[s].vertices[i] = [x, y, 0.0];
        }
    }
}
