//! fnmaped: snapshot state the generated fn-check's stab and SerialisedLayerBehaviour cases need (the recorder's
//! MapConfigProbe.cs makes it: a Knife lodged in the human, the layer editor's components on its limbs).
use std::collections::HashMap;

use super::{f, floats, inner_fields, list, reference, Snapshot};
use crate::game::layer_editor::SerialisedLayer;
use crate::game::physical::SharpAxis;
use crate::game::stab::Penetration;
use crate::game::Game;

impl Game {
    /// The live stabs as the PhysicalBehaviour records hold them: each stabber's `penetrations` (its Joint the adopted
    /// SliderJoint2D), the victims' `victimPenetrations`, `beingStabbedBy` and `stabWoundCount`, and each active
    /// penetration's IgnoreCollision requests (DoStab's, one per relevant collider) as the stack's counts.
    pub(super) fn load_recorded_stabs(&mut self, s: &Snapshot) -> Result<(), String> {
        let phys = |g: &Game, t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
        // A penetration is one object in both lists: by its stabber and joint.
        let mut made: HashMap<(usize, Option<i32>), usize> = HashMap::new();
        let mut parse = |g: &mut Game, item: &str| -> Result<usize, String> {
            let (pen, joint_id) = penetration_of(g, item)?;
            if let Some(&i) = made.get(&(pen.Stabber, joint_id)) {
                return Ok(i);
            }
            let stabber = pen.Stabber;
            g.penetration_arena.push(pen);
            made.insert((stabber, joint_id), g.penetration_arena.len() - 1);
            Ok(g.penetration_arena.len() - 1)
        };
        let mut recorded = Vec::new();
        for r in s.of("PhysicalBehaviour") {
            let Some(p) = self.physicals.iter().position(|x| x.id == r.id) else { continue };
            if let Some(t) = r.get("penetrations") {
                recorded.push(p);
                for item in list(t) {
                    let i = parse(self, item)?;
                    self.physicals[p].penetrations.push(i);
                }
            }
            if let Some(t) = r.get("victimPenetrations") {
                for item in list(t) {
                    let i = parse(self, item)?;
                    self.physicals[p].victimPenetrations.push(i);
                }
            }
            if let Some(t) = r.get("beingStabbedBy") {
                self.physicals[p].beingStabbedBy = list(t).into_iter().filter_map(|x| phys(self, x)).collect();
            }
            if let Some(n) = r.get("stabWoundCount") {
                self.physicals[p].stabWoundCount = n.parse::<i64>().map_err(|_| "stabWoundCount")? as u32;
            }
        }
        // A stabber without a PhysicalBehaviour record (a per-case snapshot's item, linked from its prefab): its penetrations
        // as its victims list them.
        // ponytail: an inactive penetration its victims already dropped is missing; record the item's list if a case needs it.
        for i in 0..self.penetration_arena.len() {
            let st = self.penetration_arena[i].Stabber;
            if !recorded.contains(&st) && !self.physicals[st].penetrations.contains(&i) {
                self.physicals[st].penetrations.push(i);
            }
        }
        let mut requests: HashMap<(i32, i32), i32> = HashMap::new();
        for p in 0..self.physicals.len() {
            for &i in &self.physicals[p].penetrations {
                let pen = &self.penetration_arena[i];
                if pen.Active && pen.Joint.is_some() {
                    let own = self.physicals[p].colliders[0];
                    for &rc in &pen.RelevantColliders {
                        *requests.entry(Game::collider_pair(rc, own)).or_default() += 1;
                    }
                }
            }
        }
        // The b2 world's ignored pairs came in at the human's own count (2); a stab's pair is asked once per penetration.
        for (k, n) in requests {
            if self.ignore_stack.contains_key(&k) {
                self.ignore_stack.insert(k, n);
            }
        }
        Ok(())
    }

    /// A PPG_CAMREC recording's GlobalCamera record: Global.CameraPosition (`lpos=`).
    pub(super) fn load_recorded_camera(&mut self, s: &Snapshot) -> Result<(), String> {
        if let Some(r) = s.of("GlobalCamera").next() {
            self.camera_control.position = camera_lpos(r.req("lpos")?)?;
        }
        Ok(())
    }

    /// The SerialisedLayerBehaviour records (`phys=<PhysicalBehaviour id> layer= order= orig=`): the layer editor's
    /// components by object.
    pub(super) fn load_serialised_layers(&mut self, s: &Snapshot) -> Result<(), String> {
        for r in s.of("SerialisedLayer") {
            let id: i32 = r.req("phys")?.parse().map_err(|_| "SerialisedLayer phys")?;
            let p = self.physicals.iter().position(|x| x.id == id).ok_or(format!("SerialisedLayer {}: no PhysicalBehaviour #{id}", r.name))?;
            self.layer_editor.serialised.insert(p, recorded_layer(r.req("layer")?, r.req("order")?, r.req("orig")?)?);
        }
        Ok(())
    }
}

/// OptionalInt as recorded: the value, or "-" when inactive.
fn optional(t: &str) -> Result<Option<i32>, String> {
    if t == "-" { Ok(None) } else { t.parse().map(Some).map_err(|_| format!("bad OptionalInt {t}")) }
}

fn recorded_layer(layer: &str, order: &str, orig: &str) -> Result<SerialisedLayer, String> {
    let (a, b) = orig.split_once(',').ok_or("SerialisedLayer orig")?;
    Ok(SerialisedLayer { SortingLayerID: optional(layer)?, SortingOrder: optional(order)?, OriginalSorting: (a.parse().map_err(|_| "orig id")?, b.parse().map_err(|_| "orig order")?) })
}

fn camera_lpos(t: &str) -> Result<[f32; 3], String> {
    let v: Vec<f32> = t.split(',').map(|x| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|_| format!("GlobalCamera lpos {t}"))).collect::<Result<_, _>>()?;
    v.try_into().map_err(|_| format!("GlobalCamera lpos {t}"))
}

/// The recorded GlobalCamera against the rewrite's Global.CameraPosition.
pub(super) fn compare_camera(g: &Game, s: &Snapshot) -> Vec<String> {
    s.of("GlobalCamera").filter_map(|r| {
        let theirs = r.req("lpos").ok().and_then(|t| camera_lpos(t).ok());
        let ours = g.camera_control.position;
        (theirs.map(|t| t.map(f32::to_bits)) != Some(ours.map(f32::to_bits))).then(|| format!("GlobalCamera: game {theirs:?} ours {ours:?}"))
    }).collect()
}

/// Each recorded SerialisedLayerBehaviour against the rewrite's for its object.
pub(super) fn compare_serialised_layers(g: &Game, s: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    for r in s.of("SerialisedLayer") {
        let theirs = (|| recorded_layer(r.req("layer")?, r.req("order")?, r.req("orig")?))();
        let p = r.get("phys").and_then(|x| x.parse::<i32>().ok()).and_then(|id| g.physicals.iter().position(|x| x.id == id));
        let ours = p.and_then(|p| g.layer_editor.serialised.get(&p).copied());
        match (theirs, ours) {
            (Ok(t), Some(o)) if t == o => {}
            (t, o) => out.push(format!("SerialisedLayer {}: game {t:?} ours {o:?}", r.name)),
        }
    }
    out
}

/// A Penetration argument: the arena entry load_recorded_stabs made for its Stabber and Joint.
pub(super) fn penetration_arg(g: &Game, a: &str) -> Result<usize, String> {
    let v = inner_fields(a);
    let get = |k: &str| v.iter().find(|(n, _)| n == k).map(|(_, x)| x.as_str()).ok_or(format!("Penetration.{k}"));
    let stabber = reference(get("Stabber")?).and_then(|id| g.physicals.iter().position(|p| p.id == id)).ok_or("Penetration: no Stabber")?;
    let joint = reference(get("Joint")?).and_then(|id| g.adopted_joints.get(&id).copied());
    g.penetration_arena.iter().position(|p| p.Stabber == stabber && p.Joint == joint).ok_or_else(|| format!("no penetration for {a}"))
}

/// A Penetration as the recorder formats it (`Penetration(Stabber=@.. Victim=.. Joint=.. ..)`), and its Joint's instance id.
fn penetration_of(g: &Game, item: &str) -> Result<(Penetration, Option<i32>), String> {
    let phys = |t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
    let v = inner_fields(item);
    let get = |k: &str| v.iter().find(|(n, _)| n == k).map(|(_, x)| x.as_str()).ok_or(format!("Penetration.{k}"));
    let joint_id = reference(get("Joint")?);
    Ok((Penetration {
        Stabber: phys(get("Stabber")?).ok_or("Penetration: no Stabber")?,
        Victim: phys(get("Victim")?),
        Joint: joint_id.and_then(|id| g.adopted_joints.get(&id).copied()),
        Duration: f(get("Duration")?),
        Axis: sharp_axis(get("Axis")?)?,
        RelevantColliders: list(get("RelevantColliders")?).into_iter().filter_map(reference).collect(),
        CurrentConnectedAnchor: floats(get("CurrentConnectedAnchor")?),
        Active: get("Active")? == "1",
    }, joint_id))
}

/// DoStab's `new Penetration()` argument (in no list yet): a new arena entry.
pub(super) fn fresh_penetration(g: &mut Game, a: &str) -> Result<usize, String> {
    let (pen, _) = penetration_of(g, a)?;
    g.penetration_arena.push(pen);
    Ok(g.penetration_arena.len() - 1)
}

/// A ContactPoint2D as the recorder writes a Collision2D's contact (point, normal, separation, normalImpulse,
/// tangentImpulse, relativeVelocity, enabled, collider, other, rigidbody, otherRigidbody; ids 0 for null).
pub(super) fn contact_point(c: &str) -> Result<crate::b2::contacts2d::ContactPoint2D, String> {
    use crate::b2::math::Vec2;
    let w: Vec<&str> = c.split(',').collect();
    if w.len() != 14 { return Err(format!("ContactPoint2D of {} words", w.len())) }
    let id = |s: &str| s.parse::<i32>().map_err(|_| format!("ContactPoint2D id {s}"));
    let opt = |s: &str| id(s).map(|i| (i != 0).then_some(i));
    let v2 = |x: &str, y: &str| Vec2::new(f(x), f(y));
    Ok(crate::b2::contacts2d::ContactPoint2D {
        point: v2(w[0], w[1]), normal: v2(w[2], w[3]), separation: f(w[4]), normal_impulse: f(w[5]), tangent_impulse: f(w[6]),
        relative_velocity: v2(w[7], w[8]), enabled: w[9] == "1", collider: id(w[10])?, other_collider: id(w[11])?,
        rigidbody: opt(w[12])?, other_rigidbody: opt(w[13])?,
    })
}

/// IsInsideOf's Collider2D[] (`[@name#id;..]`): each one of the object's colliders with its serialized definition.
pub(super) fn own_colliders<'g>(g: &'g Game, p: usize, a: &str) -> Result<Vec<(i32, &'g crate::assets::components::Collider2D)>, String> {
    let ph = &g.physicals[p];
    list(a).into_iter().map(|x| {
        let id = reference(x).ok_or_else(|| format!("collider {x}"))?;
        let i = ph.colliders.iter().position(|&c| c == id).ok_or_else(|| format!("collider #{id} not on {}", ph.name))?;
        ph.collider_defs.get(i).map(|d| (id, d)).ok_or_else(|| format!("{}: no serialized collider {i}", ph.name))
    }).collect()
}

/// A SharpAxis as Dump.Format writes it (`SharpAxis(Axis=.. LowerLimit=..)`): a penetration's Axis, GetSharpEnd's argument.
pub(super) fn sharp_axis(t: &str) -> Result<SharpAxis, String> {
    let a = inner_fields(t);
    let ax = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, x)| x.as_str()).ok_or(format!("SharpAxis.{k}"));
    Ok(SharpAxis { Axis: floats(ax("Axis")?), LowerLimit: f(ax("LowerLimit")?), UpperLimit: f(ax("UpperLimit")?), LooseUpperLimit: ax("LooseUpperLimit")? == "1", LooseLowerLimit: ax("LooseLowerLimit")? == "1" })
}

/// A Penetration getter's self (MapConfigProbe.Self): the stabber's first penetration that is Active with its joint and
/// victim still there.
pub(super) fn live_penetration(g: &Game, p: usize) -> Result<usize, String> {
    g.physicals[p].penetrations.iter().copied().find(|&i| {
        let x = &g.penetration_arena[i];
        x.Active && x.Joint.is_some() && x.Victim.is_some()
    }).ok_or_else(|| "no live penetration".into())
}

/// FindTopmostAround/FindBottommostAround's return and out args: `<found> out=<point>;<layer>;<result>;<sortingOrder>`
/// (`none` the order with no result: int.MinValue or int.MaxValue).
pub(super) fn find_around(g: &Game, args: &[&str], r: Option<(usize, i32)>, none: i32) -> String {
    match r {
        Some((p, o)) => format!("1 out={};{};@{}#{};{o}", args[0], args[1], g.physicals[p].name.replace(' ', "_"), g.physicals[p].id),
        None => format!("0 out={};{};null;{none}", args[0], args[1]),
    }
}
