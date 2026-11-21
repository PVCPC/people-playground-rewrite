//! The human's distance-joint wires in a snapshot (fnweap): the wire records (`BloodWireBehaviour`, and the recorder's
//! FnWeap.DumpWires records named by class) rebuilt as `Game::blood_wires` entries on their DistanceJoint2D, and
//! compared field by field after a fn-check case.
// ponytail: slider wires are left out until a case needs them.

use super::super::blood_wire::{BloodWire, WireKind};
use super::super::container::Owner;
use super::super::{Behaviour, BodyOwner, Game};
use super::{f, floats, list, physics, reference, Record, Snapshot};

/// A wire record's class: the record's type, or an item's script record's `class` (a wire on a pasted item root).
fn class_of(r: &Record) -> &str {
    if r.ty == "ItemScript" { r.get("class").unwrap_or("") } else { &r.ty }
}

/// A wire record's kind, from its class (and a UseWireBehaviour's Channel and CanBreak).
fn kind_of(r: &Record) -> Option<WireKind> {
    Some(match class_of(r) {
        "BloodWireBehaviour" => WireKind::Blood,
        "AppliedBandageBehaviour" => WireKind::Bandage,
        "CableBehaviour" => WireKind::Cable,
        "RopeBehaviour" => WireKind::Rope,
        "ChainBehaviour" => WireKind::Chain,
        "CopperWireBehaviour" => WireKind::Copper,
        "EnergyWireBehaviour" => WireKind::Energy,
        "HeatPipeBehaviour" => WireKind::HeatPipe,
        "RigidWireBehaviour" => WireKind::Rigid,
        "FixedCableBehaviour" => WireKind::FixedCable,
        "SteelBindingBehaviour" => WireKind::SteelBinding,
        "WoodenBindingBehaviour" => WireKind::WoodenBinding,
        "WoodStrutBehaviour" => WireKind::WoodStrut,
        "SliderJointWireBehaviour" => WireKind::Slider,
        "NewUseWireBehaviour" => WireKind::NewUse,
        "UseWireBehaviour" => WireKind::Use(r.get("Channel")?.parse().ok()?, r.get("CanBreak")? == "1"),
        // One class for the three spring tools: told apart by their material.
        "SpringCableBehaviour" => match r.get("WireMaterialName")?.trim_matches('"') {
            "StrongSpringCable" => WireKind::StrongSpring,
            "DestructibleSpringCable" => WireKind::DestructibleSpring,
            _ => WireKind::Spring,
        },
        _ => return None,
    })
}

pub(super) fn is_wire_record(r: &Record) -> bool {
    kind_of(r).is_some()
}

pub(super) fn wire_records(s: &Snapshot) -> Vec<&Record> {
    let mut v: Vec<&Record> = s.records.iter().filter(|r| is_wire_record(r)).collect();
    // In the order they were laid, as the joint comparison takes them (instance ids count down).
    v.sort_by_key(|r| std::cmp::Reverse(r.id));
    v
}

fn name_of(t: &str) -> &str {
    t.trim_start_matches('@').rsplit_once('#').map_or(t, |x| x.0)
}

/// The recorded wires, each with its DistanceJoint2D component joined to the restored b2 joint.
pub(super) fn load_wires(g: &mut Game, s: &Snapshot, rs: &physics::Restored) -> Result<(), String> {
    // Joint record id -> `Game::fixed_joints` index: wires sharing one joint (UseWireTool.CreateWire on a joint that has a
    // wire already) share its entry.
    let mut shared = std::collections::HashMap::new();
    for r in wire_records(s) {
        let kind = kind_of(r).unwrap();
        let physical = |t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
        let p = physical(r.req("physicalBehaviour")?).ok_or(format!("wire #{}: no physicalBehaviour", r.id))?;
        let other = physical(r.req("otherPhysicalBehaviour")?);
        // A wire without its typedJoint before its Start (a spring wire's the probe destroyed; a wire a save load added): Start's
        // untyped branch, from its stored members, no joint yet.
        if reference(r.req("typedJoint")?).is_none() && (kind.is_spring() || r.req("vertices")? == "null") {
            let mut w = BloodWire::new(kind);
            (w.id, w.physical, w.other, w.joint) = (r.id, p, other, usize::MAX);
            load_wire_fields(&mut w, r)?;
            let mut saved = w.current.clone();
            if kind.is_spring() {
                saved.targetDistance = f(r.req("targetDistance")?);
                saved.SpringStrength = f(r.req("SpringStrength")?);
            }
            if let (Some(d), Some(m)) = (r.get("currentDistance"), r.get("currentMaxDistanceOnly")) {
                (saved.currentDistance, saved.currentMaxDistanceOnly) = (f(d), m == "1");
            }
            load_new_use_fields(&mut w, r)?;
            w.current = saved.clone();
            w.saved = Some(saved);
            g.blood_wires.push(w);
            let b = Behaviour::BloodWire(g.blood_wires.len() - 1);
            g.behaviour_of.insert(r.id, (0, b));
            g.behaviours.add(0, b);
            continue;
        }
        // typedJoint "destroyed": the joint is gone (another wire's OnDestroy took it); the wire keeps a destroyed entry.
        let jid = reference(r.req("typedJoint")?);
        // A FixedJointWireBehaviour's typedJoint is a FixedJoint2D.
        let joint_type = if kind.is_fixed() { "FixedJoint2D" } else if kind.is_spring() { "SpringJoint2D" } else { "DistanceJoint2D" };
        // A pasted item's wire: its joint is an Item* record.
        // (A SliderJointWireBehaviour's SliderJoint2D record is the same everywhere: the loader adopted it already.)
        let joint_type = if kind.is_slider() { "SliderJoint2D".to_string() } else if r.ty == "ItemScript" { format!("Item{joint_type}") } else { joint_type.to_string() };
        let joint_type = joint_type.as_str();
        let j = jid.map(|jid| s.of(joint_type).find(|x| x.id == jid).ok_or(format!("wire #{}: no {joint_type} #{jid}", r.id))).transpose()?;
        let mut state = crate::b2::weld::FixedJoint2d::reset();
        if let Some(j) = j {
            let (a, c): ([f32; 2], [f32; 2]) = (floats(j.req("anchor")?), floats(j.req("connectedAnchor")?));
            state.anchor = crate::b2::math::Vec2::new(a[0], a[1]);
            state.connected_anchor = crate::b2::math::Vec2::new(c[0], c[1]);
            state.enable_collision = j.req("collide")? == "1";
            state.break_force = f(j.req("breakForce")?);
            state.break_torque = f(j.req("breakTorque")?);
        }
        // FixedJointWireBehaviour.Start turns autoConfigureConnectedAnchor on (only OnPreFlip..the next FixedUpdate turns it off), so
        // OnChangePause's connectedBody reconnect recomputes the anchor; DistanceJointWireBehaviour.Start turns it off.
        state.auto_configure_connected_anchor = j.and_then(|j| j.get("autoConfigureConnectedAnchor")).map_or(kind.is_fixed(), |v| v == "1");
        state.anchor_configured = true;
        // The joint's own distance and maxDistanceOnly when the record has them (a probe may set them after the wire's Start), else the wire's current*. FixedJoint2D kinds have none.
        let jget = |k: &str| j.and_then(|j| j.get(k));
        // Without a distance field (Item* records), a rigid joint's distance is its b2DistanceJoint's m_length (word 9): Create
        // passes DistanceJoint2D.distance as the length, so a probe's typed wire before its Start (currentDistance still 0) has it too.
        let distance = if kind.is_fixed() || kind.is_spring() || kind.is_slider() { None } else {
            let max_distance_only = jget("maxDistanceOnly").map_or_else(|| r.req("currentMaxDistanceOnly"), Ok)? == "1";
            let length = jget("joint").filter(|w| *w != "null" && !max_distance_only).and_then(|w| w.split(',').nth(9)).and_then(|w| u32::from_str_radix(w, 16).ok()).map(f32::from_bits);
            let distance = match jget("distance") { Some(v) => f(v), None => length.map_or_else(|| r.req("currentDistance").map(f), Ok)? };
            Some(super::super::spawn::DistanceState { auto_configure_distance: jget("autoConfigureDistance").is_none_or(|v| v == "1"), distance, max_distance_only })
        };
        // A SpringJoint2D's own fields (recordings from fnba12 on).
        let spring = if let (true, Some(j)) = (kind.is_spring(), j) { Some(super::super::spawn::SpringState { auto_configure_distance: j.req("autoConfigureDistance")? == "1", distance: f(j.req("distance")?), damping_ratio: f(j.req("dampingRatio")?), frequency: f(j.req("frequency")?) }) } else { None };
        let joint = j.and_then(|j| rs.joints.get(&format!("{}#{}", j.name, j.id)).copied());
        // The joint's connectedBody when the record names it (a probe may clear it), else the other end's.
        let connected = if jget("connected") == Some("null") { None } else { other.map(BodyOwner::Physical) };
        let adopted = jid.filter(|_| kind.is_slider()).and_then(|jid| g.adopted_joints.get(&jid).copied());
        let fj = match adopted.as_ref().or_else(|| jid.and_then(|jid| shared.get(&jid))) {
            Some(&fj) => fj,
            None => {
                let enabled = match j { Some(j) => j.req("enabled")? == "1", None => false };
                g.fixed_joints.push(super::super::simulate::FixedJoint { hinge: None, state, slider: None, spring, friction: None, distance, relative: None, joint, attached: BodyOwner::Physical(p), connected, active: true, enabled, destroyed: j.is_none() });
                if let Some(jid) = jid {
                    shared.insert(jid, g.fixed_joints.len() - 1);
                }
                g.fixed_joints.len() - 1
            }
        };
        let mut w = BloodWire::new(kind);
        w.id = r.id;
        w.physical = p;
        w.other = other;
        w.joint = fj;
        // Start ran (Initialise made the vertices array); a wire added this frame (UseWireTool.CreateWire's) has none yet.
        w.started = r.req("vertices")? != "null";
        load_wire_fields(&mut w, r)?;
        if kind.is_spring() {
            w.current.targetDistance = f(r.req("targetDistance")?);
            w.current.SpringStrength = f(r.req("SpringStrength")?);
        } else if distance.is_some() {
            // The wire's own fields: currentDistance follows the joint only in Tick (and Start).
            w.distanceBeforeFlip = f(r.req("distanceBeforeFlip")?);
            w.current.currentDistance = f(r.req("currentDistance")?);
            w.current.currentMaxDistanceOnly = r.req("currentMaxDistanceOnly")? == "1";
        } else if !kind.is_slider() {
            // FixedJointWireBehaviour: localRenderEndpoint, originalRotation, hasMovedInPauseMode, otherEnd (a CableOtherEndBehaviour).
            w.localRenderEndpoint = floats(r.req("localRenderEndpoint")?);
            w.originalRotation = f(r.req("originalRotation")?);
            w.hasMovedInPauseMode = r.req("hasMovedInPauseMode")? == "1";
            w.otherEnd = !matches!(r.req("otherEnd")?, "null" | "destroyed");
        }
        if let WireKind::Use(channel, can_break) = kind {
            (w.Channel, w.CanBreak) = (channel, can_break);
        }
        load_new_use_fields(&mut w, r)?;
        if let Some(v) = r.get("breakForceBuffer") {
            w.breakForceBuffer = f(v);
        }
        // gapB r11: AppliedBandageBehaviour's private limb (Start's TryGetComponent<LimbBehaviour>) and bloodColour.
        if kind == WireKind::Bandage {
            w.limb = r.get("limb").filter(|t| reference(t).is_some()).and_then(|t| g.limbs.iter().position(|l| g.physicals[l.PhysicalBehaviour].name == name_of(t)));
            if let Some(c) = r.get("bloodColour") {
                w.bloodColour = floats(c);
            }
        }
        // gapB r23: EnergyWireBehaviour.hasEggs (GenRun flips it; SetEggs reads it).
        if kind == WireKind::Energy {
            w.hasEggs = r.get("hasEggs") == Some("1");
        }
        if kind == WireKind::Blood {
            let limb = |k: &str| r.get(k).filter(|t| reference(t).is_some()).and_then(|t| g.limbs.iter().position(|l| g.physicals[l.PhysicalBehaviour].name == name_of(t)));
            (w.limbA, w.limbB) = (limb("limbA"), limb("limbB"));
            (w.containerA, w.containerB) = (limb("containerA").map(Owner::Circulation), limb("containerB").map(Owner::Circulation));
            w.heartLimb = limb("heartLimb");
            w.flow = f(r.req("flow")?);
            w.color = floats(r.req("color")?);
            w.AreBothLimbs = r.req("AreBothLimbs")? == "1";
            w.IsHeartRateWire = r.req("IsHeartRateWire")? == "1";
            w.IsCardiopulmonaryBypassWire = r.req("IsCardiopulmonaryBypassWire")? == "1";
            if r.req("heartMonitor")? != "null" || r.req("cardiopulmonaryBypass")? != "null" {
                return Err(format!("wire #{}: a heart monitor or bypass machine wire (not loaded)", r.id));
            }
        }
        g.blood_wires.push(w);
        let b = Behaviour::BloodWire(g.blood_wires.len() - 1);
        g.behaviour_of.insert(r.id, (0, b));
        g.behaviours.add(0, b);
    }
    // batch4 r8: an OnJointCreate case's arg, a bare SpringJoint2D on a limb (the recorder's MakeArg, made as
    // SpringJointWireTool.OnDeselect makes one; no wire holds it yet): its `fixed_joints` entry, adopted by its id.
    let arg = s.of("FnCase").next().filter(|r| r.name.ends_with(".OnJointCreate")).and_then(|r| r.get("args")).and_then(|a| reference(a.split(';').next().unwrap_or(a)));
    // batch1 r7: or a bare DistanceJoint2D (DistanceWireTool.OnDeselect's, maxDistanceOnly on: BloodWireTool.OnJointCreate's arg).
    let arg = arg.filter(|id| !wire_joint_ids(s).contains(id));
    // fnrec r1: or a bare FixedJoint2D (FixedWireTool.OnDeselect's: FixedCableTool/SteelBindingTool/WoodenBindingTool.OnJointCreate's arg).
    let fixed = arg.and_then(|id| s.of("FixedJoint2D").find(|x| x.id == id));
    let found = arg.and_then(|id| s.of("SpringJoint2D").find(|x| x.id == id).map(|j| (j, true)).or_else(|| s.of("DistanceJoint2D").find(|x| x.id == id).map(|j| (j, false)))).or(fixed.map(|j| (j, false)));
    if let Some((j, is_spring)) = found {
        let is_fixed = fixed.is_some_and(|x| std::ptr::eq(x, j));
        let p = g.physicals.iter().position(|p| p.name == j.name).ok_or(format!("SpringJoint2D #{}: no physical {}", j.id, j.name))?;
        let mut state = crate::b2::weld::FixedJoint2d::reset();
        let (a, c): ([f32; 2], [f32; 2]) = (floats(j.req("anchor")?), floats(j.req("connectedAnchor")?));
        state.anchor = crate::b2::math::Vec2::new(a[0], a[1]);
        state.connected_anchor = crate::b2::math::Vec2::new(c[0], c[1]);
        state.enable_collision = j.req("collide")? == "1";
        state.break_force = f(j.req("breakForce")?);
        state.break_torque = f(j.req("breakTorque")?);
        state.auto_configure_connected_anchor = j.req("autoConfigureConnectedAnchor")? == "1";
        state.anchor_configured = true;
        let spring = if is_spring { Some(super::super::spawn::SpringState { auto_configure_distance: j.req("autoConfigureDistance")? == "1", distance: f(j.req("distance")?), damping_ratio: f(j.req("dampingRatio")?), frequency: f(j.req("frequency")?) }) } else { None };
        let distance = if is_spring || is_fixed { None } else { Some(super::super::spawn::DistanceState { auto_configure_distance: j.req("autoConfigureDistance")? == "1", distance: f(j.req("distance")?), max_distance_only: j.req("maxDistanceOnly")? == "1" }) };
        let to = j.req("connected")?;
        let connected = reference(to).and_then(|_| g.physicals.iter().position(|p| p.name == name_of(to))).map(BodyOwner::Physical);
        let joint = rs.joints.get(&format!("{}#{}", j.name, j.id)).copied();
        g.adopted_joints.insert(j.id, g.fixed_joints.len());
        g.fixed_joints.push(super::super::simulate::FixedJoint { hinge: None, state, slider: None, spring, friction: None, distance, relative: None, joint, attached: BodyOwner::Physical(p), connected, active: true, enabled: j.req("enabled")? == "1", destroyed: false });
    }
    load_pins(g, s, rs)?;
    load_belts(g, s)
}

/// The recorded LinkDeviceBehaviours (PhaseLinkTool's, FnWeap.LinkFields: recordings from fnw16 on), in record order (the
/// fn-check's index among them), as `Game::phase_links`: started ones (from= set) get Start's line and end sprites, their
/// VisualDeletableDetachedToolBehaviours (the ends' records) and the recorded hover state, line positions and looks.
fn load_links(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let ends: Vec<&Record> = s.of("VisualDeletableDetachedToolBehaviour").collect();
    // An end record exists only while its object is active: ExistInDetailView keeps it so only while ShowLimbStatus is on.
    if !ends.is_empty() {
        g.ShowLimbStatus = true;
    }
    for r in s.of("LinkDeviceBehaviour") {
        let Some(other) = r.get("Other") else { continue };
        let physical = |g: &Game, t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
        let p = physical(g, r.req("PhysicalBehaviour")?).ok_or(format!("link #{}: no PhysicalBehaviour", r.id))?;
        let ids: [i32; 2] = [r.req("from")?.parse().map_err(|_| "bad from")?, r.req("to")?.parse().map_err(|_| "bad to")?];
        g.phase_links.push(super::super::link_device::PhaseLink {
            id: r.id,
            physical: p,
            Other: physical(g, other),
            Invisible: r.req("Invisible")? == "1",
            LocalOffset: floats(r.req("LocalOffset")?),
            OtherLocalOffset: floats(r.req("OtherLocalOffset")?),
            IsMouseInsideBounds: r.req("inside")? == "1",
            IsMouseOverlapping: r.req("over")? == "1",
            ..Default::default()
        });
        let l = g.phase_links.len() - 1;
        let b = Behaviour::PhaseLink(l);
        g.behaviour_of.insert(r.id, (0, b));
        g.behaviours.add(0, b);
        if ids[0] == 0 {
            continue;
        }
        // Start ran: its line and sprites (Resources.Load: the game's data).
        if g.assets.is_none() {
            g.assets = Some(Box::new(crate::assets::db::AssetDb::open(&crate::game_assets::find_game(None)?)?));
        }
        // The "link target" object stays under the physical Start put it under, also when Other was nulled since:
        // its Clone record's parent object's PhysicalBehaviour.
        let target = match g.phase_links[l].Other {
            Some(o) => o,
            None => {
                let comp = format!("VisualDeletableDetachedToolBehaviour:{}", ids[1]);
                let has = |o: &&Record, c: &str| o.get("comps").is_some_and(|x| x.trim_matches(['[', ']']).split(';').any(|x| x == c));
                let e = s.of("Clone").find(|o| has(o, &comp)).ok_or(format!("link #{}: no Clone record of its target", r.id))?;
                let (root, parent) = (e.req("root")?, e.req("parent")?);
                let po = s.of("Clone").find(|o| o.get("root") == Some(root) && o.get("index") == Some(parent)).ok_or(format!("link #{}: no target parent", r.id))?;
                let pid = po.req("comps")?.trim_matches(['[', ']']).split(';').find_map(|x| x.strip_prefix("PhysicalBehaviour:")).ok_or(format!("link #{}: target parent has no PhysicalBehaviour", r.id))?;
                let pid: i32 = pid.parse().map_err(|_| "bad PhysicalBehaviour id")?;
                g.physicals.iter().position(|p| p.id == pid).ok_or(format!("link #{}: no physical #{pid}", r.id))?
            }
        };
        g.phase_link_load_visuals(l, target);
        g.phase_links[l].started = true;
        g.phase_links[l].ends = ids;
        if let Some((a, b)) = r.req("linePos")?.split_once(';') {
            let line = [floats::<3>(a), floats::<3>(b)];
            g.phase_links[l].last_line = Some(line);
            let i = g.phase_links[l].line.unwrap();
            g.lines[i].positions = line.to_vec();
        }
        // line=enabled,material,layer
        let shown: Vec<&str> = r.req("line")?.split(',').collect();
        if shown.len() == 3 {
            let i = g.phase_links[l].line.unwrap();
            g.lines[i].enabled = shown[0] == "1";
            g.lines[i].game_layer = shown[2].parse().map_err(|_| "bad line layer")?;
            if shown[1] == "DeleteWire" {
                g.lines[i].material = g.assets.as_ref().unwrap().resource("Materials/DeleteWire", 21).ok().flatten();
            }
        }
        for (end, id) in ids.into_iter().enumerate() {
            let k = g.phase_links[l].end_renderers[end].unwrap();
            // No end record: its object is inactive (ExistInDetailView with ShowLimbStatus off), so its Update never ran and
            // it keeps Start's state (not hovered, Start's sprite).
            if let Some(e) = ends.iter().find(|x| x.id == id) {
                g.phase_links[l].end_inside[end] = e.req("inside")? == "1";
                g.phase_links[l].end_overlapping[end] = e.req("over")? == "1";
                // sprite=r,g,b,a,layer
                let v: Vec<&str> = e.req("sprite")?.split(',').collect();
                g.renderers[k].sprite.1 = floats(&v[..4].join(","));
                g.renderers[k].game_layer = v[4].parse().map_err(|_| "bad end layer")?;
            } else if g.ShowLimbStatus {
                return Err(format!("link #{}: no end #{id}", r.id));
            }
            // The end object's local TRS as the game has it (Clone records, recordings from fnw16 on).
            let comp = format!("VisualDeletableDetachedToolBehaviour:{id}");
            if let Some(o) = s.of("Clone").find(|o| o.get("comps").is_some_and(|c| c.trim_matches(['[', ']']).split(';').any(|x| x == comp))) {
                let (t, q, sc): ([f32; 3], [f32; 4], [f32; 3]) = (floats(o.req("lpos")?), floats(o.req("lrot")?), floats(o.req("lscale")?));
                g.renderers[k].chain[0] = crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s: [sc[0], sc[1], sc[2], 0.0] };
            }
            let beh = Behaviour::LinkEnd(l, end);
            g.behaviour_of.insert(id, (0, beh));
            g.behaviours.add(0, beh);
        }
    }
    Ok(())
}

/// The arena link and end (0 "link source", 1 "link target") of VisualDeletableDetachedToolBehaviour `id`.
pub(super) fn link_end(g: &Game, id: i32) -> Result<(usize, usize), String> {
    g.phase_links.iter().enumerate().find_map(|(l, x)| x.ends.iter().position(|&e| e == id).map(|e| (l, e))).ok_or(format!("no link end #{id}"))
}

fn compare_links(g: &Game, s: &Snapshot, out: &mut Vec<String>) {
    for r in s.of("LinkDeviceBehaviour").filter(|r| r.get("Other").is_some()) {
        let Some(x) = g.phase_links.iter().find(|x| x.id == r.id) else {
            out.push(format!("LinkDeviceBehaviour #{}: no such link in ours", r.id));
            continue;
        };
        let ends = if x.started { x.ends } else { [0, 0] };
        let mut fields = vec![
            ("inside", bit(x.IsMouseInsideBounds)),
            ("over", bit(x.IsMouseOverlapping)),
            ("LocalOffset", hexes(&x.LocalOffset)),
            ("OtherLocalOffset", hexes(&x.OtherLocalOffset)),
            ("from", ends[0].to_string()),
            ("to", ends[1].to_string()),
        ];
        if let Some(i) = x.line.filter(|_| x.started) {
            let line = &g.lines[i];
            let db = g.assets.as_ref().unwrap();
            let delete = db.resource("Materials/DeleteWire", 21).ok().flatten();
            fields.push(("line", format!("{},{},{}", bit(line.enabled), if line.material == delete { "DeleteWire" } else { "PhaseLink" }, line.game_layer)));
            if let Some(v) = x.last_line {
                fields.push(("linePos", format!("{};{}", hexes(&v[0]), hexes(&v[1]))));
            }
        }
        for (k, ours) in fields {
            let theirs = r.get(k).unwrap_or("");
            if theirs != ours {
                out.push(format!("LinkDeviceBehaviour #{} {k}: game {theirs} ours {ours}", r.id));
            }
        }
        if !x.started {
            continue;
        }
        for end in 0..2 {
            let id = x.ends[end];
            let gone = g.destroyed_components.contains(&id);
            let Some(e) = s.of("VisualDeletableDetachedToolBehaviour").find(|e| e.id == id) else {
                if !gone && g.ShowLimbStatus {
                    out.push(format!("VisualDeletableDetachedToolBehaviour #{id}: in ours, not in the game's"));
                }
                continue;
            };
            let k = x.end_renderers[end].unwrap();
            let sprite = format!("{},{}", hexes(&g.renderers[k].sprite.1), g.renderers[k].game_layer);
            for (key, ours) in [("inside", bit(x.end_inside[end])), ("over", bit(x.end_overlapping[end])), ("sprite", sprite)] {
                let theirs = e.get(key).unwrap_or("");
                if theirs != ours {
                    out.push(format!("VisualDeletableDetachedToolBehaviour #{id} {key}: game {theirs} ours {ours}"));
                }
            }
        }
    }
}

/// NewUseWireBehaviour's Channels, CanBreak and breakForceBuffer (`from`/`to` after the clones: `load_item_wires`).
fn load_new_use_fields(w: &mut BloodWire, r: &Record) -> Result<(), String> {
    if w.kind == WireKind::NewUse {
        w.Channel = r.req("Channels")?.parse().map_err(|_| "bad Channels")?;
        w.CanBreak = r.req("CanBreak")? == "1";
        w.breakForceBuffer = f(r.req("breakForceBuffer")?);
    }
    Ok(())
}

/// A recorded `ValueTuple`2(Item1=.. Item2=..)`'s two references.
fn pair(v: &str) -> (Option<i32>, Option<i32>) {
    let v = v.trim_start_matches("ValueTuple`2(").trim_end_matches(')');
    let (a, b) = v.split_once(" Item2=").unwrap_or((v, "null"));
    (reference(a.trim_start_matches("Item1=")), reference(b))
}

/// EnergyWireBehaviour's antenna (receiver.j set: hasAntenna and not cleared by TransferAntennaSignal) and eggs.b (set iff hasEggs).
/// After the clones: the arenas hold the pasted items' scripts (lasers, receivers).
/// NewUseWireBehaviour's from and to (NodeBehaviours: `Game::activation_nodes`).
pub(super) fn load_item_wires(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    // The links (made during the run, so after the map and the pasted items): an end on a map or item physical needs
    // its SpriteRenderer, which the clones bring.
    load_links(g, s)?;
    for r in wire_records(s) {
        let Some(i) = wire_index(g, r.id) else { continue };
        match kind_of(r) {
            Some(WireKind::Energy) => {
                let (antenna, eggs) = energy_wire_state(g, r)?;
                (g.blood_wires[i].antenna, g.blood_wires[i].eggs) = (antenna, eggs);
            }
            Some(WireKind::NewUse) => {
                let node = |g: &mut Game, k: &str| -> Result<Option<usize>, String> {
                    let Some(id) = r.get(k).and_then(reference) else { return Ok(None) };
                    // PhysicalBehaviour.Awake's node: through the PhysicalBehaviour record naming it.
                    let owner = s.of("PhysicalBehaviour").find(|x| x.get("ActivationNode").and_then(reference) == Some(id)).map(|x| x.id);
                    let p = owner.and_then(|o| g.physicals.iter().position(|p| p.id == o));
                    if let Some(n) = g.activation_nodes.iter().position(|n| n.id == id) {
                        return Ok(Some(n));
                    }
                    let p = p.ok_or(format!("wire #{}: no NodeBehaviour #{id}", r.id))?;
                    // Awake's node as recorded: the one ours made takes the recorded id.
                    g.physicals[p].ids.ActivationNode = id;
                    let n = g.activation_node_of(BodyOwner::Physical(p)).unwrap_or_else(|| {
                        g.activation_nodes.push(crate::game::activations::ActivationNode { owner: Some(BodyOwner::Physical(p)), ..Default::default() });
                        g.activation_nodes.len() - 1
                    });
                    g.activation_nodes[n].id = id;
                    Ok(Some(n))
                };
                (g.blood_wires[i].from, g.blood_wires[i].to) = (node(g, "from")?, node(g, "to")?);
            }
            _ => {}
        }
    }
    Ok(())
}

/// A destroyed antenna.a (the probe's DestroyImmediate of the AntennaBehaviour alone) goes in `destroyed_components`.
fn energy_wire_state(g: &mut Game, r: &Record) -> Result<(Option<super::super::blood_wire::Antenna>, Option<usize>), String> {
    let physical = |g: &Game, id: Option<i32>| id.and_then(|id| g.physicals.iter().position(|p| p.id == id));
    let Some(receiver) = r.get("receiver") else { return Ok((None, None)) };
    let mut w = (None, None);
    let ((j, rp), (a, ap)) = (pair(receiver), pair(r.req("antenna")?));
    if j.is_some() {
        let antenna = physical(g, ap).ok_or(format!("wire #{}: antenna without its physical", r.id))?;
        let receiver = physical(g, rp).ok_or(format!("wire #{}: receiver without its physical", r.id))?;
        let (CorrectUp, Fuzziness, id) = g.antenna_behaviour(antenna);
        if a.is_none() {
            g.destroyed_components.insert(id);
        }
        w.0 = Some(super::super::blood_wire::Antenna { a: id, antenna, CorrectUp, Fuzziness, receiver });
    }
    if let Some(b) = pair(r.req("eggs")?).1 {
        w.1 = Some(g.lasers.iter().position(|l| l.id == b).ok_or(format!("wire #{}: no LaserBehaviour #{b}", r.id))?);
    }
    Ok(w)
}

/// The members every wire record has (WireBehaviour's), and a SpringCableBehaviour's initialBreakForce.
fn load_wire_fields(w: &mut BloodWire, r: &Record) -> Result<(), String> {
        w.WireWidth = f(r.req("WireWidth")?);
        w.line_width = r.get("lineWidth").map(f);
        w.WireColor = floats(r.req("WireColor")?);
        (w.start_color, w.end_color) = (w.WireColor, w.WireColor);
        w.Invisible = r.req("Invisible")? == "1";
        w.shouldIgnoreUpdate = r.req("shouldIgnoreUpdate")? == "1";
        w.usedToHaveConnectedBody = r.req("usedToHaveConnectedBody")? == "1";
        w.IsMouseInsideBounds = r.req("IsMouseInsideBounds")? == "1";
        w.IsMouseOverlapping = r.req("IsMouseOverlapping")? == "1";
        w.current.currentAnchor = floats(r.req("currentAnchor")?);
        w.current.currentConnectedAnchor = floats(r.req("currentConnectedAnchor")?);
        w.current.currentBreakingForce = f(r.req("currentBreakingForce")?);
        w.current.WireMaterialName = r.req("WireMaterialName")?.trim_matches('"').replace('_', " ");
        w.vertices = Some(r.req("vertices")?).filter(|v| *v != "null").map_or(Vec::new(), |v| list(v).into_iter().map(floats::<3>).collect());
        // Initialise: GetVertexCount() + 1 vertices.
        w.vertex_count = w.vertices.len().saturating_sub(1);
        // SpringCableBehaviour.vertexCount (Start's Max(2, CeilToInt(distance)), a field of its own) as recorded once Initialise
        // has run: GetVertexCount() is vertexCount - 1 (a seeded 0 wraps, and the dump below wraps it back).
        if let Some(v) = r.get("vertexCount").and_then(|v| v.parse::<isize>().ok()).filter(|_| !w.vertices.is_empty()) {
            w.vertex_count = (v as usize).wrapping_sub(1);
        }
        // SpringCableBehaviour.initialBreakForce (kept in breakForceBuffer, as Start sets it).
        if let Some(v) = r.get("initialBreakForce") {
            w.breakForceBuffer = f(v);
        }
        Ok(())
}

/// The human's HingeJoint2Ds past each limb's own (the first of its name): pins' joints (PinBehaviour.CreateJoint's
/// AddComponent, one per call), then the PinBehaviour records on them (FnWeap.PinFields), each with its SpriteChild.
fn load_pins(g: &mut Game, s: &Snapshot, rs: &physics::Restored) -> Result<(), String> {
    let physical = |g: &Game, t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
    let mut joints = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for j in s.of("HingeJoint2D") {
        if seen.insert(j.name.as_str()) {
            continue;
        }
        let p = g.physicals.iter().position(|p| p.name == j.name).ok_or(format!("hinge #{}: no body {}", j.id, j.name))?;
        let mut state = crate::b2::weld::FixedJoint2d::reset();
        let (a, c): ([f32; 2], [f32; 2]) = (floats(j.req("anchor")?), floats(j.req("connectedAnchor")?));
        state.anchor = crate::b2::math::Vec2::new(a[0], a[1]);
        state.connected_anchor = crate::b2::math::Vec2::new(c[0], c[1]);
        state.anchor_configured = true;
        state.break_force = f(j.req("breakForce")?);
        state.break_torque = f(j.req("breakTorque")?);
        let joint = rs.joints.get(&format!("{}#{}", j.name, j.id)).copied();
        let connected = physical(g, j.req("connected")?).map(BodyOwner::Physical);
        let fj = g.fixed_joints.len();
        g.adopted_joints.insert(j.id, fj);
        g.fixed_joints.push(super::super::simulate::FixedJoint { hinge: Some(Default::default()), state, slider: None, spring: None, friction: None, distance: None, relative: None, joint, attached: BodyOwner::Physical(p), connected, active: true, enabled: j.req("enabled")? == "1", destroyed: false });
        joints.insert(j.id, fj);
    }
    for r in s.of("PinBehaviour") {
        let Some(joint) = r.get("Joint") else { continue };
        let p = physical(g, r.req("PhysicalBehaviour")?).ok_or(format!("pin #{}: no PhysicalBehaviour", r.id))?;
        let pin = super::super::pin::Pin {
            id: r.id,
            physical: p,
            Other: physical(g, r.req("Other")?),
            LocalAnchor: floats(r.req("LocalAnchor")?),
            BreakingThreshold: f(r.req("BreakingThreshold")?),
            UsedToHaveConnectedBody: r.req("UsedToHaveConnectedBody")? == "1",
            wood: r.req("PinSpritePath")?.trim_matches('"') == "Sprites/WoodPin",
            Joint: reference(joint).and_then(|id| joints.get(&id).copied()),
            initialBreakForce: f(r.req("initialBreakForce")?),
            Invisible: r.get("Invisible") == Some("1"),
            IsMouseOverlapping: r.req("over")? == "1",
            ..Default::default()
        };
        g.pins.push(pin);
        let k = g.pins.len() - 1;
        if r.req("SpriteChild")? != "null" {
            // Resources.Load(PinSpritePath): the game's data.
            if g.assets.is_none() {
                g.assets = Some(Box::new(crate::assets::db::AssetDb::open(&crate::game_assets::find_game(None)?)?));
            }
            g.pin_create_sprite_child(k);
            // The sprite where the last ConstraintManager pass put it (recordings from fnw14 on).
            if let (Some(t), Some(q)) = (r.get("childPos").filter(|v| *v != "null"), r.get("childRot").filter(|v| *v != "null")) {
                let (t, q): ([f32; 3], [f32; 4]) = (floats(t), floats(q));
                let n = &mut g.renderers[g.pins[k].renderer.unwrap()].chain[0];
                (n.t[0], n.t[1], n.t[2], n.q) = (t[0], t[1], t[2], q);
            }
            g.tracker_new(super::super::tool_invisibility::Owner::Pin(k));
            let i = g.pins[k].renderer.unwrap();
            if let Some((layer, order)) = r.req("sorting")?.split_once(',') {
                g.renderers[i].layer = layer.parse().map_err(|_| "bad sorting")?;
                g.renderers[i].sprite.2 = order.parse().map_err(|_| "bad sorting")?;
            }
            let over = g.pins[k].IsMouseOverlapping;
            g.pins[k].tracker.wasMouseOverlapping = over;
            g.pins[k].tracker.delete_look = r.req("sprite")?.split(',').nth(1) == Some("DeleteWire");
            // The sprite's recorded enabled (sprite=enabled,material,layer): the tracker sets it only in its Update, so a pin
            // whose Invisible changed since (a case's seed) still shows as recorded; tracker_new takes it from Invisible.
            g.renderers[i].enabled = r.req("sprite")?.split(',').next() == Some("1");
        }
        g.pins[k].started = true;
        let b = Behaviour::Pin(k);
        g.behaviour_of.insert(r.id, (0, b));
        g.behaviours.add(0, b);
    }
    Ok(())
}

fn belt_records(s: &Snapshot) -> Vec<&Record> {
    let mut v: Vec<&Record> = s.of("SerpentineBeltBehaviour").collect();
    v.sort_by_key(|r| std::cmp::Reverse(r.id));
    v
}

/// The recorded SerpentineBeltBehaviours (laid by the belt tool between two limbs), as `Game::belts` entries.
fn load_belts(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    for r in belt_records(s) {
        let physical = |t: &str| reference(t).and_then(|id| g.physicals.iter().position(|p| p.id == id));
        let a = physical(r.req("Self")?).ok_or(format!("belt #{}: no Self", r.id))?;
        let b = physical(r.req("Other")?).ok_or(format!("belt #{}: no Other", r.id))?;
        let hull: Vec<[f32; 2]> = list(r.req("linePositionBuffer")?).into_iter().map(|v| { let v = floats::<3>(v); [v[0], v[1]] }).collect();
        g.belts.push(super::super::serpentine_belt::Belt {
            id: r.id,
            Self_: a,
            Other: b,
            gearRatio: f(r.req("gearRatio")?),
            lastScale: f(r.req("lastScale")?),
            // Start ran: it made the InvisibilityTracker.
            started: r.req("InvisibilityTracker")? != "null",
            Invisible: r.req("Invisible")? == "1",
            worldPointArray: list(r.req("worldPointArray")?).into_iter().map(floats::<2>).collect(),
            hull: (!hull.is_empty()).then_some(hull),
            line_renderer: !matches!(r.req("LineRenderer")?, "destroyed" | "null"),
        });
        let beh = Behaviour::Belt(g.belts.len() - 1);
        g.behaviour_of.insert(r.id, (0, beh));
        g.behaviours.add(0, beh);
    }
    Ok(())
}

/// The arena index of belt record `id`.
pub(super) fn belt_index(g: &Game, id: i32) -> Option<usize> {
    g.belts.iter().position(|x| x.id == id)
}

fn compare_belts(g: &Game, s: &Snapshot, out: &mut Vec<String>) {
    for r in belt_records(s) {
        let Some(l) = belt_index(g, r.id) else {
            out.push(format!("SerpentineBeltBehaviour #{}: no such belt in ours", r.id));
            continue;
        };
        let x = &g.belts[l];
        let hull = x.hull.as_ref().map_or(String::new(), |h| h.iter().map(|v| hexes(&[v[0], v[1], 0.0])).collect::<Vec<_>>().join(";"));
        for (k, ours) in [
            ("gearRatio", hex(x.gearRatio)),
            ("lastScale", hex(x.lastScale)),
            ("Invisible", bit(x.Invisible)),
            ("worldPointArray", format!("[{}]", x.worldPointArray.iter().map(|v| hexes(v)).collect::<Vec<_>>().join(";"))),
            ("linePositionBuffer", format!("[{hull}]")),
        ] {
            let theirs = r.get(k).unwrap_or("").to_string();
            if theirs != ours {
                out.push(format!("SerpentineBeltBehaviour #{} {k}: game {theirs} ours {ours}", r.id));
            }
        }
    }
}

/// The `Game::fixed_joints` index of the wire whose typedJoint is joint record `id`.
pub(super) fn wire_joint(g: &Game, s: &Snapshot, id: i32) -> Option<usize> {
    let r = wire_records(s).into_iter().find(|r| r.get("typedJoint").and_then(reference) == Some(id))?;
    wire_index(g, r.id).map(|w| g.blood_wires[w].joint)
}

/// The joints the wire records name (typedJoint).
pub(super) fn wire_joint_ids(s: &Snapshot) -> std::collections::HashSet<i32> {
    wire_records(s).into_iter().filter_map(|r| r.get("typedJoint").and_then(reference)).collect()
}

/// The arena index of wire record `id`.
pub(super) fn wire_index(g: &Game, id: i32) -> Option<usize> {
    g.blood_wires.iter().position(|w| w.id == id)
}

fn hex(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

fn hexes(v: &[f32]) -> String {
    v.iter().map(|&x| hex(x)).collect::<Vec<_>>().join(",")
}

fn bit(b: bool) -> String {
    (if b { "1" } else { "0" }).to_string()
}

/// The recorded wires' fields that differ from ours (compare_wires below).
/// weapons r20/r21: each joint record's `joint=` words (the b2 joint behind the component, words from 0x98) against
/// ours' live b2 joint, the words the restore loads (physics.rs restore_*; bias and gamma are the last step's
/// InitVelocityConstraints temporaries, not loaded). compare() has no joint words: WinchBehaviour.Start's connectedBody
/// null-and-back rebuild showed only here. Ours' joint: the adopted entry (SliderJoint2D, pins' HingeJoint2D, a script's
/// prefab joint), a wire's typedJoint, else the anchored prefab joint. A HingeJoint2D's words are its b2RevoluteJoint
/// record's. `sliders`: SliderJoint2D only; `others`: every other joint type (both on unless PPG_FNCHECK_NOJOINTS).
/// Bool words keep only their flag bytes (the padding is the allocator's), as physics::compare_world.
pub(super) fn compare_joints(g: &Game, s: &Snapshot, sliders: bool, others: bool) -> Vec<String> {
    use crate::b2::world::JointKind as K;
    let mut out = Vec::new();
    let wire_joint: std::collections::HashMap<i32, usize> = wire_records(s).iter().filter_map(|r| Some((r.get("typedJoint").and_then(reference)?, g.blood_wires[wire_index(g, r.id)?].joint))).collect();
    let mut seen = std::collections::HashSet::new();
    let kinds: &[&str] = &["SliderJoint2D", "HingeJoint2D", "FixedJoint2D", "SpringJoint2D", "DistanceJoint2D", "ItemHingeJoint2D", "ItemWheelJoint2D", "ItemFixedJoint2D", "ItemDistanceJoint2D", "ItemSpringJoint2D", "MapHingeJoint2D", "MapFixedJoint2D", "MapSpringJoint2D", "MapDistanceJoint2D"];
    for &kind in kinds {
        if !(if kind == "SliderJoint2D" { sliders } else { others }) {
            continue;
        }
        for r in s.of(kind) {
            // A pasted object's joint has its Item and Map record: once.
            if !seen.insert(r.id) {
                continue;
            }
            // A limb's own hinge (the first HingeJoint2D of its name, load_pins) lives in the limb arena: compare() has it.
            if kind == "HingeJoint2D" && s.of("HingeJoint2D").find(|x| x.name == r.name).is_some_and(|x| x.id == r.id) {
                continue;
            }
            let w = if kind == "HingeJoint2D" { s.of("b2RevoluteJoint").find(|x| x.id == r.id).and_then(|x| x.get("joint")) } else { r.get("joint") };
            let Some(w) = w else { continue };
            let entry = g.adopted_joints.get(&r.id).or_else(|| wire_joint.get(&r.id)).and_then(|&f| g.fixed_joints.get(f)).map(|f| (!f.destroyed).then_some(f.joint).flatten());
            let ours = match entry {
                Some(j) => j,
                // An anchored prefab joint: its b2 joint, else its `Game::hinges` entry's (an item hinge the link took over).
                // Neither (a grip's): not compared here.
                None => match g.anchored_joints.iter().find(|a| a.component == r.id).map(|a| a.joint.or_else(|| g.hinges.get(a.hinge?).filter(|h| !h.destroyed)?.joint)) {
                    Some(Some(j)) => Some(j),
                    _ => continue,
                },
            };
            let live = ours.filter(|&id| g.world.joints.get(id).is_some_and(|j| j.is_some()));
            // gapB r23: a joint the game's call destroyed (`joint=null`) must be gone in ours too.
            if w == "null" {
                if live.is_some() { out.push(format!("{kind} #{}: game no b2 joint, ours live", r.id)); }
                continue;
            }
            let Some(id) = live else {
                out.push(format!("{kind} #{}: no b2 joint in ours", r.id));
                continue;
            };
            let f = |v: f32| v.to_bits();
            let v2 = |v: crate::b2::math::Vec2| [v.x.to_bits(), v.y.to_bits()];
            // (word, ours, mask)
            let mut ws: Vec<(usize, u32, u32)> = Vec::new();
            let mut put = |k: usize, vals: &[u32]| for (i, &v) in vals.iter().enumerate() { ws.push((k + i, v, !0)); };
            let name = match &g.world.joints[id].as_ref().unwrap().kind {
                K::Prismatic(p) => {
                    for (k, v) in physics::prismatic_words(p).into_iter().enumerate() {
                        ws.push((k, v, if k == 17 { 0xffff } else { !0 }));
                    }
                    "b2PrismaticJoint"
                }
                K::Revolute(j) => {
                    let d = &j.def;
                    put(0, &[v2(d.local_anchor_a), v2(d.local_anchor_b)].concat());
                    put(4, &[f(j.impulse[0]), f(j.impulse[1]), f(j.impulse[2]), f(j.motor_impulse)]);
                    put(9, &[f(d.max_motor_torque), f(d.motor_speed)]);
                    put(12, &[f(d.reference_angle), f(d.lower_angle), f(d.upper_angle)]);
                    put(37, &[j.limit_state]);
                    ws.push((8, d.enable_motor as u32, 0xff));
                    ws.push((11, d.enable_limit as u32, 0xff));
                    "b2RevoluteJoint"
                }
                K::Distance(j) => {
                    let d = &j.def;
                    put(0, &[f(d.frequency_hz), f(d.damping_ratio)]);
                    put(3, &[v2(d.local_anchor_a), v2(d.local_anchor_b)].concat());
                    put(8, &[f(j.impulse), f(d.length)]);
                    "b2DistanceJoint"
                }
                K::Rope(j) => {
                    let d = &j.def;
                    put(0, &[v2(d.local_anchor_a), v2(d.local_anchor_b)].concat());
                    put(4, &[f(d.max_length), f(j.length), f(j.impulse), f(j.u.x), f(j.u.y)]);
                    "b2RopeJoint"
                }
                K::Weld(j) => {
                    let d = &j.def;
                    put(0, &[f(d.frequency_hz), f(d.damping_ratio)]);
                    put(3, &[v2(d.local_anchor_a), v2(d.local_anchor_b)].concat());
                    put(7, &[f(d.reference_angle)]);
                    put(9, &[f(j.impulse[0]), f(j.impulse[1]), f(j.impulse[2])]);
                    "b2WeldJoint"
                }
                K::Wheel(j) => {
                    let d = &j.def;
                    put(0, &[f(d.frequency_hz), f(d.damping_ratio)]);
                    put(2, &[v2(d.local_anchor_a), v2(d.local_anchor_b), v2(j.local_x_axis_a), v2(j.local_y_axis_a)].concat());
                    put(10, &[f(j.impulse), f(j.motor_impulse), f(j.spring_impulse), f(d.max_motor_torque), f(d.motor_speed)]);
                    ws.push((15, d.enable_motor as u32, 0xff));
                    "b2WheelJoint"
                }
                _ => {
                    out.push(format!("{kind} #{}: ours is no joint of its kind", r.id));
                    continue;
                }
            };
            let game = super::words(w);
            for (k, o, m) in ws {
                let Some(&gw) = game.get(k) else { continue };
                if gw & m != o & m {
                    out.push(format!("{name} {kind} #{}.w{k}: game {:08x} ours {:08x}", r.id, gw & m, o & m));
                }
            }
        }
    }
    out
}

pub(super) fn compare_wires(g: &Game, s: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    for r in wire_records(s) {
        let Some(i) = wire_index(g, r.id) else {
            out.push(format!("{} #{}: no such wire in ours", r.ty, r.id));
            continue;
        };
        let w = &g.blood_wires[i];
        let limb = |l: Option<usize>| l.map(|l| g.physicals[g.limbs[l].PhysicalBehaviour].name.clone());
        let phys = |p: Option<usize>| p.map(|p| g.physicals[p].id);
        let mut fields: Vec<(&str, String)> = vec![
            ("WireWidth", hex(w.WireWidth)),
            ("WireColor", hexes(&w.WireColor)),
            ("Invisible", bit(w.Invisible)),
            ("shouldIgnoreUpdate", bit(w.shouldIgnoreUpdate)),
            ("usedToHaveConnectedBody", bit(w.usedToHaveConnectedBody)),
            ("IsMouseInsideBounds", bit(w.IsMouseInsideBounds)),
            ("IsMouseOverlapping", bit(w.IsMouseOverlapping)),
            ("currentAnchor", hexes(&w.current.currentAnchor)),
            ("currentConnectedAnchor", hexes(&w.current.currentConnectedAnchor)),
            ("currentBreakingForce", hex(w.current.currentBreakingForce)),
            ("vertices", if w.started { format!("[{}]", w.vertices.iter().map(|v| hexes(v)).collect::<Vec<_>>().join(";")) } else { "null".into() }),
        ];
        if w.kind.is_fixed() {
            fields.push(("localRenderEndpoint", hexes(&w.localRenderEndpoint)));
            fields.push(("originalRotation", hex(w.originalRotation)));
            fields.push(("hasMovedInPauseMode", bit(w.hasMovedInPauseMode)));
        } else if w.kind.is_spring() {
            fields.push(("targetDistance", hex(w.current.targetDistance)));
            fields.push(("SpringStrength", hex(w.current.SpringStrength)));
            fields.push(("initialBreakForce", hex(w.breakForceBuffer)));
            // Before Start (a tool's wire, OnDeselect's PO): vertexCount 0 and no WireMaterialName yet (OnJointCreate sets WireMaterial only).
            fields.push(("vertexCount", if w.started { (w.vertex_count.wrapping_add(1) as isize).to_string() } else { "0".into() }));
            fields.push(("WireMaterialName", if w.started || w.saved.is_some() { format!("\"{}\"", w.current.WireMaterialName) } else { "null".into() }));
        } else if !w.kind.is_slider() {
            // (SliderJointWireBehaviour is not a DistanceJointWireBehaviour: no distance members.)
            fields.push(("distanceBeforeFlip", hex(w.distanceBeforeFlip)));
            fields.push(("currentDistance", hex(w.current.currentDistance)));
            fields.push(("currentMaxDistanceOnly", bit(w.current.currentMaxDistanceOnly)));
        }
        if r.get("breakForceBuffer").is_some() {
            fields.push(("breakForceBuffer", hex(w.breakForceBuffer)));
        }
        if w.kind == WireKind::NewUse {
            fields.push(("Channels", w.Channel.to_string()));
            fields.push(("CanBreak", bit(w.CanBreak)));
            let node = |n: Option<usize>| n.map_or("null".to_string(), |n| g.activation_nodes[n].id.to_string());
            for (k, ours) in [("from", node(w.from)), ("to", node(w.to))] {
                let theirs = r.get(k).and_then(reference).map_or("null".to_string(), |id| id.to_string());
                if theirs != ours {
                    out.push(format!("{} #{} {k}: game {theirs} ours {ours}", r.ty, r.id));
                }
            }
        }
        if let WireKind::Use(channel, can_break) = w.kind {
            fields.push(("Channel", channel.to_string()));
            fields.push(("CanBreak", bit(can_break)));
        }
        if w.kind == WireKind::Blood {
            fields.push(("flow", hex(w.flow)));
            fields.push(("color", hexes(&w.color)));
            fields.push(("AreBothLimbs", bit(w.AreBothLimbs)));
            fields.push(("IsHeartRateWire", bit(w.IsHeartRateWire)));
            fields.push(("IsCardiopulmonaryBypassWire", bit(w.IsCardiopulmonaryBypassWire)));
        }
        for (k, ours) in fields {
            let theirs = r.get(k).unwrap_or("");
            if theirs != ours {
                out.push(format!("{} #{} {k}: game {theirs} ours {ours}", r.ty, r.id));
            }
        }
        // The joint: there or gone (a missing one compares as missing).
        // A spring wire loaded before its Start has no joint entry yet.
        let gone = g.fixed_joints.get(w.joint).is_none_or(|j| j.destroyed);
        if r.get("typedJoint").and_then(reference).is_none() != gone {
            out.push(format!("{} #{} typedJoint: game {} ours {}", r.ty, r.id, r.get("typedJoint").unwrap_or(""), if gone { "destroyed" } else { "present" }));
        }
        // Links by object: the physicals by id, the limbs by name.
        for (k, ours) in [("physicalBehaviour", phys(Some(w.physical))), ("otherPhysicalBehaviour", phys(w.other))] {
            if r.get(k).and_then(reference) != ours {
                out.push(format!("{} #{} {k}: game {} ours {ours:?}", r.ty, r.id, r.get(k).unwrap_or("")));
            }
        }
        // EnergyWireBehaviour: receiver.phys and antenna.phys (set together, cleared together), eggs.b.
        if let (WireKind::Energy, Some(_)) = (w.kind, r.get("receiver")) {
            let x = w.antenna;
            for (k, ours) in [("receiver", x.map(|x| g.physicals[x.receiver].id)), ("antenna", x.map(|x| g.physicals[x.antenna].id)), ("eggs", w.eggs.map(|b| g.lasers[b].id))] {
                let theirs = pair(r.get(k).unwrap_or("")).1;
                if theirs != ours {
                    out.push(format!("{} #{} {k}: game {} ours {ours:?}", r.ty, r.id, r.get(k).unwrap_or("")));
                }
            }
        }
        if w.kind == WireKind::Blood {
            for (k, ours) in [("limbA", limb(w.limbA)), ("limbB", limb(w.limbB)), ("heartLimb", limb(w.heartLimb))] {
                let theirs = r.get(k).filter(|t| reference(t).is_some()).map(|t| name_of(t).to_string());
                if theirs != ours {
                    out.push(format!("{} #{} {k}: game {theirs:?} ours {ours:?}", r.ty, r.id));
                }
            }
        }
    }
    compare_belts(g, s, &mut out);
    compare_links(g, s, &mut out);
    out
}
