//! A script's own Transform (`base.transform`) when its GameObject is not its PhysicalBehaviour's: the script reads
//! its own object's pose, which hangs below the nearest physical or Transform node through static local TRSs.
//! On a physical's own object it is that physical, read through the same functions as before.

use super::{mathf, BareAnchor, Game};
use crate::unity::transform::Trs;

/// `base.transform` of a script.
#[derive(Clone, Debug)]
pub enum ObjectXf {
    /// The script sits on this physical's GameObject.
    Physical(usize),
    /// A GameObject without a PhysicalBehaviour: what it hangs from, the local TRSs up to that (object first) and
    /// its clone root's Transform id (`transform.root` when it hangs from no physical or node).
    Chain(BareAnchor, Vec<Trs>, i32),
}

impl Default for ObjectXf {
    fn default() -> Self {
        ObjectXf::Chain(BareAnchor::Root, Vec::new(), 0)
    }
}

/// Whether the script whose `base.transform` is `xf` sits on physical `p`'s GameObject (a SendMessage to it reaches
/// the script).
pub fn on_physical(xf: &ObjectXf, p: usize) -> bool {
    matches!(xf, ObjectXf::Physical(q) if *q == p)
}

impl Game {
    /// The object's Transforms, object first.
    fn xf_nodes(&self, a: BareAnchor, chain: &[Trs]) -> Vec<Trs> {
        self.anchored_nodes(a, chain)
    }

    /// Transform.position
    pub fn xf_position(&self, xf: &ObjectXf) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.physicals[*p].transform.position,
            ObjectXf::Chain(a, c, _) => self.anchored_world(*a, c).0,
        }
    }

    /// Transform.rotation: CalculateGlobalRotation up the chain.
    pub fn xf_rotation(&self, xf: &ObjectXf) -> [f32; 4] {
        match xf {
            ObjectXf::Physical(p) => self.transform_rotation(*p),
            ObjectXf::Chain(a, c, _) => {
                let nodes = self.xf_nodes(*a, c);
                let (leaf, parents) = nodes.split_first().expect("a Transform");
                parents.iter().fold(leaf.q, |q, node| crate::unity::transform::global_rotation(node, q))
            }
        }
    }

    /// Transform.right: rotation * Vector3.right.
    pub fn xf_right(&self, xf: &ObjectXf) -> [f32; 3] {
        mathf::rotate(self.xf_rotation(xf), [1.0, 0.0, 0.0])
    }

    /// Transform.up: rotation * Vector3.up.
    pub fn xf_up(&self, xf: &ObjectXf) -> [f32; 3] {
        mathf::rotate(self.xf_rotation(xf), [0.0, 1.0, 0.0])
    }

    /// Transform.localScale
    pub fn xf_local_scale(&self, xf: &ObjectXf) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.physicals[*p].transform.localScale,
            ObjectXf::Chain(a, c, _) => {
                let s = self.xf_nodes(*a, c)[0].s;
                [s[0], s[1], s[2]]
            }
        }
    }

    /// Transform.lossyScale
    pub fn xf_lossy_scale(&self, xf: &ObjectXf) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.lossy_scale(*p),
            ObjectXf::Chain(a, c, _) => crate::unity::transform::lossy_scale(&self.xf_nodes(*a, c), self.xf_rotation(xf)),
        }
    }

    /// Transform.TransformPoint
    pub fn xf_transform_point(&self, xf: &ObjectXf, point: [f32; 3]) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.transform_point3(*p, point),
            ObjectXf::Chain(a, c, _) => {
                let mut v = [point[0], point[1], point[2], 0.0];
                for node in &self.xf_nodes(*a, c) {
                    v = crate::unity::transform::global_position(node, v);
                }
                [v[0], v[1], v[2]]
            }
        }
    }

    /// Transform.InverseTransformPoint: each inverse from the root down.
    pub fn xf_inverse_transform_point(&self, xf: &ObjectXf, point: [f32; 3]) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.inverse_transform_point(*p, point),
            ObjectXf::Chain(a, c, _) => {
                let mut v = [point[0], point[1], point[2], 0.0];
                for node in self.xf_nodes(*a, c).iter().rev() {
                    v = crate::unity::transform::inverse(node, v, [0.0, 0.0, 0.0, 1.0]).0;
                }
                [v[0], v[1], v[2]]
            }
        }
    }

    /// Transform.TransformDirection: rotation only (the object's own scale is not applied; its parents' are, as
    /// `transform_direction` does).
    pub fn xf_transform_direction(&self, xf: &ObjectXf, dir: [f32; 3]) -> [f32; 3] {
        match xf {
            ObjectXf::Physical(p) => self.transform_direction(*p, dir),
            ObjectXf::Chain(a, c, _) => {
                let mut v = [dir[0], dir[1], dir[2], 0.0];
                for (i, node) in self.xf_nodes(*a, c).iter().enumerate() {
                    v = crate::unity::transform::rotate_direction(node.q, (i > 0).then_some(node.s), v);
                }
                [v[0], v[1], v[2]]
            }
        }
    }

    /// transform.root's Transform id.
    pub fn xf_root_id(&self, xf: &ObjectXf) -> i32 {
        match xf {
            ObjectXf::Physical(p) | ObjectXf::Chain(BareAnchor::Physical(p), _, _) => self.root_id(*p),
            ObjectXf::Chain(BareAnchor::Node(n), _, _) => self.node_root_id(*n),
            ObjectXf::Chain(BareAnchor::Root, _, root) => *root,
        }
    }

    /// Whether the script `id` on `xf`'s GameObject is destroyed: with its physical, else with the physical it hangs
    /// from or on its own.
    pub fn xf_destroyed(&self, xf: &ObjectXf, id: i32, class: &str) -> bool {
        match xf {
            ObjectXf::Physical(p) => self.physicals[*p].destroyed,
            ObjectXf::Chain(BareAnchor::Physical(p), _, _) => self.physicals[*p].destroyed || self.destroyed_components.contains(&id),
            ObjectXf::Chain(..) => unimplemented!("mod-only: {class}: the destruction of a GameObject that hangs from no physical"),
        }
    }

    /// Utils.SendAllChannel(Continuous)IsolatedActivation(this) / SendMessage("IsolatedActivation") on each channel to
    /// the script's own GameObject. Only UseWireBehaviours take it; the run-time wires hang on physicals and a
    /// prefab one on a script's object stops at link, so a GameObject without a PhysicalBehaviour has no receiver:
    /// only the ActivationPropagations are made.
    pub(super) fn send_isolated_xf(&mut self, xf: &ObjectXf, continuous: bool) {
        match xf {
            ObjectXf::Physical(p) => self.send_isolated(*p, continuous),
            ObjectXf::Chain(..) => {
                for channel in 0..3 {
                    let root = self.xf_root_id(xf);
                    self.signal_from(root, channel);
                }
            }
        }
    }

    /// The scripts of `receivers` on collider `collider`'s GameObject, in component order: (class, instance id).
    fn scripts_on_collider_object(&mut self, collider: i32, receivers: &[&str]) -> Vec<(String, i32)> {
        self.with_component_instance(collider, |_, inst, o| {
            inst.prefab.objects[o].components.iter().enumerate().filter_map(|(k, (_, c))| match c {
                crate::assets::db::Component::Script { class, .. } if receivers.contains(&class.as_str()) => Some((class.clone(), inst.objects[o].components[k])),
                _ => None,
            }).collect()
        })
    }

    /// SendMessage("Shot") / ("ExitShot") to a collider's GameObject without a PhysicalBehaviour: every receiver on
    /// it, in component order, each in its own callback.
    pub(super) fn send_shot_to_object(&mut self, collider: i32, receivers: &[&str], exit: bool, damage: f32) {
        for (class, id) in self.scripts_on_collider_object(collider, receivers) {
            self.unity_callback(|g| match (class.as_str(), exit) {
                ("FireworksBehaviour", false) => {
                    let f = g.fireworks.iter().position(|f| f.id == id).expect("the object's FireworksBehaviour");
                    g.fireworks_shot(f);
                }
                ("CathodeLightBehaviour", false) => {
                    let c = g.cathode_lights.iter().position(|c| c.id == id).expect("the object's CathodeLightBehaviour");
                    g.cathode_light_shot(c, damage);
                }
                (c, _) => unimplemented!("mod-only: SendMessage Shot to the {c} of a collider without a PhysicalBehaviour"),
            });
        }
    }

    /// SendMessage("Break") to a collider's GameObject without a PhysicalBehaviour, as `send_shot_to_object`.
    pub(super) fn send_break_to_object(&mut self, collider: i32, receivers: &[&str]) {
        for (class, id) in self.scripts_on_collider_object(collider, receivers) {
            self.unity_callback(|g| match class.as_str() {
                "CathodeLightBehaviour" => {
                    let c = g.cathode_lights.iter().position(|c| c.id == id).expect("the object's CathodeLightBehaviour");
                    g.cathode_light_break(c);
                }
                c => unimplemented!("mod-only: SendMessage Break to the {c} of a collider without a PhysicalBehaviour"),
            });
        }
    }

    /// Transform.InverseTransformDirection: each inverse rotation from the root down, scales ignored.
    pub fn xf_inverse_direction(&self, xf: &ObjectXf, v: [f32; 3]) -> [f32; 3] {
        let nodes = match xf {
            ObjectXf::Physical(p) => self.anchored_nodes(BareAnchor::Physical(*p), &[]),
            ObjectXf::Chain(a, c, _) => self.xf_nodes(*a, c),
        };
        let mut v = [v[0], v[1], v[2], 0.0];
        for node in nodes.iter().rev() {
            v = crate::unity::transform::inverse_vector(&Trs { s: [1.0; 4], ..*node }, v);
        }
        [v[0], v[1], v[2]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chain object's pose is its parent's composed with its local TRS.
    #[test]
    fn chain_pose_composes_local_trs() {
        let g = Game::default();
        let q = [0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2];
        let child = Trs { t: [1.0, 0.0, 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [2.0, 2.0, 1.0, 0.0] };
        let parent = Trs { t: [5.0, 0.0, 0.0, 0.0], q, s: [1.0, 1.0, 1.0, 0.0] };
        let xf = ObjectXf::Chain(BareAnchor::Root, vec![child, parent], 0);
        let p = g.xf_position(&xf);
        assert!((p[0] - 5.0).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5);
        let r = g.xf_right(&xf);
        assert!(r[0].abs() < 1e-5 && (r[1] - 1.0).abs() < 1e-5);
        assert_eq!(g.xf_local_scale(&xf), [2.0, 2.0, 1.0]);
        let back = g.xf_inverse_direction(&xf, r);
        assert!((back[0] - 1.0).abs() < 1e-5 && back[1].abs() < 1e-5);
    }
}
