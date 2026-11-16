//! PhysicalRopeCreator: CreateRope builds the rope's nodes (a GameObject with a BoxCollider2D and a Rigidbody2D, and a
//! DistanceJoint2D to the one before) at run time and gives the LineRenderer a LineRendererTargetsBehaviour (its Update
//! sets the line's points from the nodes).
#![allow(non_snake_case)]

use super::spawn::{Clone as CloneObject, Instance};
use super::{d, mathf, script, Game};
use crate::assets::classes::{PPtr, Transform};
use crate::assets::components::{Collider2D, ColliderShape, Joint2D, JointKind, Rigidbody2D, SpriteTiling};
use crate::assets::db::{Component, ObjRef, Prefab, PrefabObject};
use crate::unity::transform::{global_position, Trs};
use std::collections::HashMap;

script! {
    PhysicalRopeCreator {
        CreateOnStart: bool = false,
        AutoPointCount: bool = false,
        PointCount: i32 = 32,
        PointsPerUnit: f32 = 4.0,
        SetAsChild: bool = false,
        NodeMass: f32 = 0.05,
        NodeLayer: i32 = 0,
        NodeRadius: f32 = -1.0,
        NodeRandomForce: f32 = 0.5,
    }
    links {
        id: i32 = 0,
        enabled: bool = true,
        /// LineRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// The prefab object the LineRenderer is on.
        line_object: usize = 0,
    }
}

/// A LineRendererTargetsBehaviour added to a rope's LineRenderer: Targets are the nodes' `transform_nodes` entries.
#[derive(Clone, Debug, Default)]
pub struct LineRendererTargets {
    pub id: i32,
    pub line: usize,
    pub targets: Vec<usize>,
}

/// Vector3.Distance
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (d(x) * x + d(y) * y + d(z) * z).0.sqrt() as f32
}

impl Game {
    /// PhysicalRopeCreator.Start
    pub fn rope_start(&mut self, id: i32) {
        let r = self.rope_creators.iter().position(|x| x.id == id).expect("a PhysicalRopeCreator in the arena");
        if self.rope_creators[r].CreateOnStart {
            self.rope_create(id);
        }
    }

    /// PhysicalRopeCreator.CreateRope
    pub fn rope_create(&mut self, id: i32) {
        let r = self.rope_creators.iter().position(|x| x.id == id).expect("a PhysicalRopeCreator in the arena");
        let line = self.rope_creators[r].line;
        let positions = self.lines[line].positions.clone();
        let mut total = 0.0f32;
        for i in 1..positions.len() {
            total += distance(positions[i], positions[i - 1]);
        }
        if self.rope_creators[r].AutoPointCount {
            self.rope_creators[r].PointCount = (self.rope_creators[r].PointsPerUnit * total) as i32;
        }
        let count = self.rope_creators[r].PointCount;
        if count < 2 {
            crate::game::menu::throw("Exception: PointCount must be greater than 1");
        }
        let radius = self.rope_creators[r].NodeRadius;
        let num = if radius < 0.0 { self.lines[line].width / 2.0 } else { radius };
        let (mass, layer, random_force) = (self.rope_creators[r].NodeMass, self.rope_creators[r].NodeLayer, self.rope_creators[r].NodeRandomForce);
        // gameObject.transform.SetParent(base.transform) for each node.
        let parent = self.rope_creators[r].SetAsChild.then(|| self.rope_parent_node(id));
        let mut previous: Option<usize> = None;
        let mut targets = Vec::new();
        for j in 0..count {
            let dist = ((d(j as f32) / (d(count as f32) - 1.0)) * total).f();
            let point = self.rope_point_at(r, dist, total);
            let k = self.rope_node(j, point, layer, num, mass, random_force, previous, parent);
            previous = Some(k);
            targets.push(self.node_rigidbodies[k].node);
        }
        // LineRenderer.gameObject.AddComponent<LineRendererTargetsBehaviour>(): Update from the next frame.
        let lrt = self.allocate_instance_id();
        let line_object = self.rope_creators[r].line_object;
        // (The unit test builds a creator without a clone.)
        let owns = |i: &Instance| i.objects.iter().any(|x| x.components.contains(&id));
        if self.unsynced.iter().chain(&self.retained).any(owns) {
            self.with_component_instance(id, |_, inst, _| inst.objects[line_object].components.push(lrt));
        }
        self.line_renderer_targets.push(LineRendererTargets { id: lrt, line, targets });
        let b = super::Behaviour::LineRendererTargets(self.line_renderer_targets.len() - 1);
        self.behaviour_of.insert(lrt, (0, b));
        self.behaviours.add(0, b);
    }

    /// LineRendererTargetsBehaviour.Update
    pub fn line_renderer_targets_update(&mut self, i: usize) {
        let (line, n) = (self.line_renderer_targets[i].line, self.line_renderer_targets[i].targets.len());
        if self.destroyed_components.contains(&self.lines[line].id) || n < 2 {
            return;
        }
        if self.lines[line].positions.len() != n {
            self.line_set_position_count(line, n);
        }
        let mut points = Vec::with_capacity(n);
        for k in 0..n {
            let node = self.line_renderer_targets[i].targets[k];
            if self.destroyed_components.contains(&self.transform_nodes[node].id) {
                crate::game::menu::throw("MissingReferenceException: The object of type 'Transform' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
            }
            let mut vector = self.node_world(node).0;
            if !self.lines[line].use_world_space {
                vector = self.line_inverse_transform_point(line, vector);
            }
            points.push(vector);
        }
        self.line_set_positions(line, &points);
    }

    /// The `transform_nodes` entry of the creator's Transform, which SetParent hangs the nodes under. A Transform that
    /// nothing moves (no Rigidbody2D or PhysicalBehaviour on it or above it) is a root node holding its world pose and
    /// lossy scale.
    fn rope_parent_node(&mut self, id: i32) -> usize {
        self.with_component_instance(id, |g, inst, o| {
            let mut x = Some(o);
            while let Some(i) = x {
                let moves = inst.prefab.objects[i].components.iter().any(|(_, c)| {
                    matches!(c, Component::Rigidbody2D(_)) || matches!(c, Component::Script { class, .. } if class == "PhysicalBehaviour")
                });
                if moves {
                    unimplemented!("mod-only: PhysicalRopeCreator.CreateRope with SetAsChild under a Transform with a Rigidbody2D or PhysicalBehaviour (the nodes follow it)");
                }
                x = inst.objects[i].parent;
            }
            let transform = inst.objects[o].components[0];
            if let Some(n) = g.transform_nodes.iter().position(|n| n.id == transform) {
                return n;
            }
            let (t, q, sc) = (inst.position(o), inst.rotation(o), inst.lossy_scale(o));
            g.transform_nodes.push(crate::unity::transform::Node { id: transform, trs: Trs { t: [t[0], t[1], t[2], 0.0], q, s: [sc[0], sc[1], sc[2], 0.0] }, parent: None });
            g.transform_nodes.len() - 1
        })
    }

    /// PhysicalRopeCreator.CreateRope's getPointAt
    fn rope_point_at(&mut self, r: usize, num2: f32, total: f32) -> [f32; 2] {
        let num2 = mathf::clamp(num2, 0.0, total - f32::from_bits(1));
        let positions = self.lines[self.rope_creators[r].line].positions.clone();
        let mut num3 = 0.0f32;
        for k in 1..positions.len() {
            let lower = num3;
            num3 += distance(positions[k], positions[k - 1]);
            if num3 >= num2 {
                let t = mathf::clamp(mathf::map_range(lower, num3, 0.0, 1.0, num2), 0.0, 1.0);
                let v = mathf::v2_lerp([positions[k - 1][0], positions[k - 1][1]], [positions[k][0], positions[k][1]], t);
                if self.lines[self.rope_creators[r].line].use_world_space {
                    return v;
                }
                let p = self.line_transform_point(self.rope_creators[r].line, [v[0], v[1], 0.0]);
                return [p[0], p[1]];
            }
        }
        crate::game::menu::throw("Exception: Attempt to get point along line at distance that exceeds the line or is less than 0")
    }

    /// One iteration of CreateRope's loop: new GameObject("Rope point j") at `at` on `layer`, a BoxCollider2D of size
    /// num, a Rigidbody2D (mass, Interpolate) pushed by an impulse, and for j > 0 a DistanceJoint2D to the node before
    /// with maxDistanceOnly. Returns the node's `node_rigidbodies` index.
    fn rope_node(&mut self, j: i32, at: [f32; 2], layer: i32, num: f32, mass: f32, random_force: f32, previous: Option<usize>, parent: Option<usize>) -> usize {
        let none = PPtr::default();
        let box_collider = Component::Collider2D(Collider2D {
            game_object: none,
            enabled: true,
            density: 1.0,
            material: none,
            is_trigger: false,
            used_by_effector: false,
            used_by_composite: false,
            offset: [0.0; 2],
            shape: ColliderShape::Box {
                tiling: SpriteTiling { border: [0.0; 4], pivot: [0.0; 2], old_size: [0.0; 2], new_size: [0.0; 2], adaptive_tiling_threshold: 0.0, draw_mode: 0, adaptive_tiling: false },
                auto_tiling: false,
                size: [num, num],
                edge_radius: 0.0,
            },
        });
        // Rigidbody2D::Reset (180c22c10)
        let rigidbody = Component::Rigidbody2D(Rigidbody2D {
            game_object: none,
            body_type: 0,
            simulated: true,
            use_full_kinematic_contacts: false,
            use_auto_mass: false,
            mass: 1.0,
            linear_drag: 0.0,
            angular_drag: 0.05,
            gravity_scale: 1.0,
            material: none,
            interpolate: 0,
            sleeping_mode: 1,
            collision_detection: 0,
            constraints: 0,
        });
        let mut components = vec![
            Component::Transform(Transform { game_object: none, local_rotation: [0.0, 0.0, 0.0, 1.0], local_position: [0.0; 3], local_scale: [1.0; 3], children: Vec::new(), father: none }),
            box_collider,
            rigidbody,
        ];
        if previous.is_some() {
            components.push(Component::Joint2D(Joint2D {
                game_object: none,
                enabled: true,
                enable_collision: false,
                connected_body: none,
                break_force: f32::INFINITY,
                break_torque: f32::INFINITY,
                auto_configure_connected_anchor: false,
                anchor: [0.0; 2],
                connected_anchor: [0.0; 2],
                kind: JointKind::Distance { auto_configure_distance: true, distance: 1.0, max_distance_only: false },
            }));
        }
        let components = components.into_iter().enumerate().map(|(k, c)| (ObjRef { file: usize::MAX, path_id: k as i64 + 1 }, c)).collect();
        let game_object_ref = ObjRef { file: usize::MAX, path_id: 0 };
        let prefab = std::sync::Arc::new(Prefab { objects: vec![PrefabObject { game_object: game_object_ref, name: format!("Rope point {j}"), layer: layer as u32, tag: 0, active: true, parent: None, children: Vec::new(), components }], index: HashMap::new(), materials: HashMap::new() });
        // new GameObject: the GameObject, then its Transform; AddComponent<BoxCollider2D> and <Rigidbody2D> take one each.
        let game_object = self.allocate_instance_id();
        let ids: Vec<i32> = (0..3).map(|_| self.allocate_instance_id()).collect();
        // gameObject.transform.position = vector; gameObject.layer = NodeLayer
        self.world.layers.insert(game_object, layer);
        let trs = Trs { t: [at[0], at[1], 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] };
        // Transform.SetParent(parent) (worldPositionStays): the clone's root keeps the world pose, the node holds the local
        // pose and the scale that keeps the lossy scale at one.
        let mut local = trs;
        if let Some(pn) = parent {
            let mut chain = vec![self.node_trs(pn)];
            let mut up = self.transform_nodes[pn].parent;
            while let Some(i) = up {
                chain.push(self.node_trs(i));
                up = self.transform_nodes[i].parent;
            }
            let ((lp, lq), _) = crate::unity::transform::set_world(&chain, [at[0], at[1], 0.0], [0.0, 0.0, 0.0, 1.0]);
            let (_, pq) = self.node_world(pn);
            let ps = crate::unity::transform::lossy_scale(&chain, pq);
            if ps.contains(&0.0) {
                unimplemented!("mod-only: Transform.SetParent under a Transform with a zero lossy scale (singular matrix)");
            }
            local = Trs { t: [lp[0], lp[1], lp[2], 0.0], q: lq, s: [1.0 / ps[0], 1.0 / ps[1], 1.0 / ps[2], 0.0] };
        }
        let mut inst = Instance {
            prefab: prefab.clone(),
            objects: vec![CloneObject { game_object, components: ids.clone(), trs, parent: None, active: true, self_active: true, activated: true }],
            interest: vec![0],
            changed: vec![0],
            ..Default::default()
        };
        self.collider2d_awake(&mut inst, &prefab, 0, 1);
        // AddComponent<Rigidbody2D>: its AwakeFromLoad makes the body; the collider's shapes stay on the ground body until SyncTransforms.
        let Component::Rigidbody2D(rb_data) = &prefab.objects[0].components[2].1 else { unreachable!() };
        self.rigidbody2d_awake(&mut inst, &prefab, 0, 2, rb_data);
        let body = inst.bodies[&0];
        let rb = super::Rigidbody {
            body,
            bodyType: 0,
            collisionDetectionMode: 0,
            interpolation: 0,
            movement: super::MovementState::default(),
            detached: None,
            component: ids[2],
            simulated: true,
            detached_props: [0.0, 0.05, 1.0],
            constraints: 0,
        };
        // The node's Transform and Rigidbody2D, as link_instance makes them for a Rigidbody2D without a PhysicalBehaviour.
        let node = self.transform_nodes.len();
        self.transform_nodes.push(crate::unity::transform::Node { id: ids[0], trs: local, parent });
        let k = self.node_rigidbodies.len();
        self.node_rigidbodies.push(super::NodeBody { node, rigidbody: rb.clone(), driven_by: None });
        self.bare_colliders.insert(ids[1], super::BareCollider { anchor: super::BareAnchor::Node(node), chain: Vec::new(), scripts: Vec::new(), water_impact: None, active_self: true, root: (game_object, ids[0]) });
        self.rigidbody_hierarchy_changed = true;
        // rigidbody2D.mass = NodeMass; interpolation = Interpolate; AddForce(Random.insideUnitCircle * NodeRandomForce, Impulse)
        self.body_set_mass(body, mass);
        self.rigidbody_set_interpolation(super::BodyOwner::Node(k), 1);
        let v = self.random.inside_unit_circle();
        rb.add_force_impulse(&mut self.world, [v[0] * random_force, v[1] * random_force]);
        if let Some(prev) = previous {
            // AddComponent<DistanceJoint2D>, connectedBody = the node before, maxDistanceOnly = true.
            let joint_id = self.allocate_instance_id();
            inst.objects[0].components.push(joint_id);
            let f = self.add_distance_joint_component_on(super::BodyOwner::Node(k));
            self.fixed_joint_set_connected_body(f, Some(super::BodyOwner::Node(prev)));
            self.distance_joint_set_max_distance_only(f, true);
        }
        self.unsynced.push(inst);
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CreateRope on a 2.95 long line at 2 points per unit: (int)5.9 = 5 nodes, 4 maxDistanceOnly joints between neighbours.
    #[test]
    fn creates_the_nodes_and_joints() {
        let mut g = Game::default();
        let mut x = PhysicalRopeCreator::default();
        x.id = 1;
        x.AutoPointCount = true;
        x.PointsPerUnit = 2.0;
        x.NodeMass = 0.05;
        x.NodeLayer = 10;
        x.NodeRadius = 0.01;
        x.NodeRandomForce = 1.0;
        x.line = 0;
        g.lines.push(crate::game::line::LineVisual {
            id: 2,
            anchor: crate::game::BareAnchor::Root,
            chain: Vec::new(),
            positions: vec![[0.0, 0.0, 0.0], [0.0, -2.95, 0.0]],
            width: 1.0,
            curve: crate::wires::WidthCurve::constant(1.0),
            gradient: crate::game::visual::Gradient::default(),
            material: None,
            layer: 0,
            order: 0,
            game_layer: 0,
            cap_vertices: 0,
            corner_vertices: 0,
            alignment: 0,
            looped: false,
            texture_mode: 0,
            use_world_space: true,
            enabled: true,
            visible: true,
        });
        g.rope_creators.push(x);
        g.rope_create(1);
        assert_eq!(g.rope_creators[0].PointCount, 5);
        assert_eq!(g.node_rigidbodies.len(), 5);
        assert_eq!(g.fixed_joints.len(), 4);
        assert!(g.fixed_joints.iter().all(|f| f.joint.is_some() && f.distance.as_ref().unwrap().max_distance_only));
        let y: Vec<f32> = g.transform_nodes.iter().map(|n| n.trs.t[1]).collect();
        assert_eq!(y[0], 0.0);
        assert!((y[4] + 2.95).abs() < 1e-5);
    }
}
