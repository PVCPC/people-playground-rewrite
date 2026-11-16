//! The recording clone: the spawn::Instance behind a recorded root (the human, the PPG_STAB item), so the scripts that
//! read or write a clone's Transforms find it. The recorder's `Clone` records give every GameObject depth first with
//! its components' instance ids; the catalog prefab is cloned with those ids in a scratch Game (its native awakes
//! give the components' native state), then its bodies, fixtures and joints are the restored world's.

use super::physics::Restored;
use super::{floats, Record, Snapshot};
use crate::assets::db::{AssetDb, Component, Prefab};
use crate::game::spawn::Instance;
use crate::game::Game;
use crate::unity::transform::Trs;

/// The recorder's map script records (Dump.MapTypes: every object's of these classes, named by the class), read as
/// `ItemScript` records of that class.
const MAP_TYPES: &[&str] = &["RandomPrefabSpawnerBehaviour", "LavaBehaviour", "WaterBehaviour", "TemperatureTargetRectangleBehaviour", "DamagableMachineryBehaviour", "RandomEventBehaviour", "DoAtTime", "MapLiftBehaviour", "IndustrialLiftBehaviour", "ReactorCoreBehaviour", "ReactorMeltdownBehaviour", "PhysicalRopeCreator", "ReactorA5DisplayBehaviour", "OffsetGridBehaviour"];

pub(super) fn script_records(s: &Snapshot) -> impl Iterator<Item = (&str, &Record)> {
    s.of("ItemScript").map(|r| (r.get("class").unwrap_or(""), r)).chain(MAP_TYPES.iter().flat_map(move |&t| s.of(t).map(move |r| (t, r))))
}

impl Game {
    /// The clone of `prefab` (the catalog prefab of the root, or the map's) behind the `root=<kind>` records, kept in
    /// `retained`, with its queue 0x14 MonoBehaviours. None when the recording has no such records.
    pub(super) fn recording_clone(&mut self, db: &mut AssetDb, s: &Snapshot, rs: &Restored, kind: &str, prefab: Prefab) -> Result<Option<(Prefab, Vec<(usize, usize)>, Option<i32>)>, String> {
        // gapA r19: a restored fixture of a root not cloned yet takes its Clone record's layer before any root's Starts (a pasted Detector's
        // Start raycast into a pasted Laser_Receiver's collider found none); a later clone moves it with the id.
        for (k, &f) in &rs.fixtures {
            let Some((name, id)) = k.rsplit_once(':').and_then(|(x, _)| x.rsplit_once('#')) else { continue };
            let go = self.world.fixtures[f].as_ref().map(|x| x.collider.game_object);
            let Some(go) = go.filter(|g| !self.world.layers.contains_key(g)) else { continue };
            let comp = format!(":{id}");
            let layer = s.of("Clone").find(|r| r.name == name && r.get("comps").is_some_and(|c| c.trim_matches(['[', ']']).split(';').any(|y| y.ends_with(&comp)))).and_then(|r| r.get("layer")?.parse().ok());
            if let Some(l) = layer {
                self.world.layers.insert(go, l);
            }
        }
        // A map object SetParent(null) took out at run time (a lift_railing's DetachFromParent on its joint's break) is
        // recorded as its own root, `root=Spawned:<its id>` without "(Clone)": the map clone holds it, detached.
        let map_root = |r: &Record| kind == "Map" && !r.name.ends_with("(Clone)") && r.get("root") == Some(format!("Spawned:{}", r.id).as_str());
        let taken_out: std::collections::HashSet<i32> = s.of("Clone").filter(|r| map_root(r)).map(|r| r.id).collect();
        let recs: Vec<&Record> = s.of("Clone").filter(|r| (r.get("root") == Some(kind) && !taken_out.contains(&r.id)) || map_root(r)).collect();
        // (After the map's own records: the first is the root, and a detached record leaves its parent's cursor.)
        let mut recs: Vec<&Record> = recs.iter().copied().filter(|r| !map_root(r)).chain(recs.iter().copied().filter(|r| map_root(r))).collect();
        // `Pasted@<id>`: a pasted root SetParent put under another root at run time (an attachment ConnectAttachment
        // parented under its firearm): its own `root=Pasted` records, from its index 0 to the next root's.
        if let Some(id) = kind.strip_prefix("Pasted@") {
            let id: i32 = id.parse().map_err(|e| format!("{kind}: {e}"))?;
            let pasted: Vec<&Record> = s.of("Clone").filter(|r| r.get("root") == Some("Pasted")).collect();
            let Some(at) = pasted.iter().position(|r| r.id == id && r.get("index") == Some("0")) else { return Ok(None) };
            recs = pasted[at..].iter().enumerate().take_while(|&(k, r)| k == 0 || r.get("index") != Some("0")).map(|(_, &r)| r).collect();
        }
        // Such a nested root is listed after the root's own records, its subtree last: it is not this root's.
        let nested: std::collections::HashSet<i32> = s.of("Clone").filter(|r| r.get("root") == Some("Pasted") && r.get("index") == Some("0")).map(|r| r.id).collect();
        if let Some(j) = (1..recs.len()).find(|&j| kind.starts_with("Spawned:") && nested.contains(&recs[j].id)) {
            for k in j + 1..recs.len() {
                let p: i64 = recs[k].req("parent")?.parse().map_err(|e| format!("Clone parent: {e}"))?;
                if p < j as i64 {
                    return Err(format!("{kind}: a record after the nested root {} is not below it ({})", recs[j].name, recs[k].name));
                }
            }
            recs.truncate(j);
        }
        if recs.is_empty() {
            return Ok(None);
        }
        let parent = |r: &Record| -> Result<i64, String> { if map_root(r) { return Ok(-2) } r.req("parent")?.parse().map_err(|e| format!("Clone parent: {e}")) };
        // Each prefab object's record: among its parent's record's children, the next of its name (a script's
        // runtime children sit between them).
        let mut map = vec![0usize; prefab.objects.len()];
        let mut cursor = vec![0usize; recs.len()];
        // Objects a DetachFromParentOnStart made roots at the item's Start: the recorder lists them after the item, parent=-2.
        let mut detached = Vec::new();
        // Instantiate gives the clone's GameObjects and their prefab components consecutive instance ids, 2 apart, in
        // prefab order from the root's: a prefab object's record is the one with its id. A prefab object without one was
        // destroyed (a shattered map Window): its clone object stays, inactive, without bodies. (A runtime reorder of
        // siblings or a destroyed object made the name walk below take a later object of the name.)
        let by_id: std::collections::HashMap<i32, usize> = recs.iter().enumerate().map(|(j, r)| (r.id, j)).collect();
        let mut next_id = recs[0].id;
        let expected: Vec<i32> = prefab.objects.iter().map(|ob| { let id = next_id; next_id -= 2 * (1 + ob.components.len() as i32); id }).collect();
        let named = |o: usize| by_id.get(&expected[o]).copied().filter(|&j| recs[j].name == prefab.objects[o].name.replace(' ', "_"));
        let by_ids = prefab.objects.len() > 1 && named(1).is_some();
        let mut gone = vec![false; prefab.objects.len()];
        // The root is the record's root (the spawn names it after its spawnable).
        for o in 1..prefab.objects.len() {
            if by_ids {
                let p = prefab.objects[o].parent.unwrap();
                match named(o) {
                    Some(j) if !gone[p] => {
                        if parent(recs[j])? == -2 {
                            detached.push(o);
                        }
                        map[o] = j;
                    }
                    _ => gone[o] = true,
                }
                continue;
            }
            let rp = map[prefab.objects[o].parent.unwrap()];
            let name = prefab.objects[o].name.replace(' ', "_");
            let mut found = None;
            for j in cursor[rp].max(rp + 1)..recs.len() {
                if parent(recs[j])? == rp as i64 && recs[j].name == name {
                    found = Some(j);
                    break;
                }
            }
            if found.is_none() {
                found = (1..recs.len()).find(|&j| !map.contains(&j) && recs[j].name == name && parent(recs[j]).ok() == Some(-2));
                if found.is_some() {
                    detached.push(o);
                }
            }
            let j = found.ok_or_else(|| format!("Clone: no record of {kind}'s {name} (prefab parent {} = record {} {}, cursor {} after {:?}, prefab siblings before {:?}; records of the name: {:?})", prefab.objects[o].parent.unwrap(), rp, recs[rp].name, cursor[rp], recs.get(cursor[rp].wrapping_sub(1)).map(|r| (&r.name, r.get("parent"))), (1..o).filter(|&q| prefab.objects[q].parent == prefab.objects[o].parent).map(|q| (prefab.objects[q].name.as_str(), map[q])).rev().take(6).collect::<Vec<_>>(), (0..recs.len()).filter(|&j| recs[j].name == name).map(|j| (j, recs[j].get("parent"), map.iter().position(|&m| m == j))).collect::<Vec<_>>()))?;
            // (A detached record, after the item's, leaves the parent's cursor where it was.)
            if detached.last() != Some(&o) {
                cursor[rp] = j + 1;
            }
            map[o] = j;
        }
        // A map object (its record the full-map snapshot's) whose Rigidbody2D the restored world lacks was destroyed since (a
        // shattered Window), and its children with it.
        let rb_key = |o: usize, j: usize| prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).map(|c| format!("{}#{}", recs[j].name, recs[j].id - 2 * (c as i32 + 1)));
        // (Only when the restored world holds the map's bodies at all.)
        if kind == "Map" && (1..prefab.objects.len()).any(|o| !gone[o] && rb_key(o, map[o]).is_some_and(|k| rs.bodies.contains_key(&k))) {
            for o in 1..prefab.objects.len() {
                let p = prefab.objects[o].parent.unwrap();
                let Some(c) = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))) else {
                    gone[o] |= gone[p];
                    continue;
                };
                let j = map[o];
                if gone[p] || (!gone[o] && recs[j].req("active")?.starts_with('1') && !rs.bodies.contains_key(&format!("{}#{}", recs[j].name, recs[j].id - 2 * (c as i32 + 1)))) {
                    gone[o] = true;
                }
            }
        }
        let mut ids = Vec::new();
        for (o, &j) in map.iter().enumerate() {
            if gone[o] {
                ids.push((expected[o], (1..=prefab.objects[o].components.len() as i32).map(|k| expected[o] - 2 * k).collect()));
                continue;
            }
            let comps: Vec<i32> = super::list(recs[j].req("comps")?).into_iter().map(|c| c.rsplit_once(':').unwrap().1.parse().unwrap()).collect();
            let n = prefab.objects[o].components.len();
            // Instantiate numbers an object's components down from its id: one missing from the list was destroyed before
            // this snapshot (a lift's slider shot off), and taking the list by position would hand its id to the next one.
            let want: Vec<i32> = (1..=n as i32).map(|k| recs[j].id - 2 * k).collect();
            let missing: Vec<i32> = want.iter().filter(|w| !comps.contains(w)).copied().collect();
            // (link_recording_item destroys them again.)
            if !missing.is_empty() && missing.len() < n && want.iter().filter(|w| comps.contains(w)).eq(comps.iter().take(n - missing.len())) {
                ids.push((recs[j].id, want));
                continue;
            }
            if comps.len() < n {
                return Err(format!("Clone {}: {} components, the prefab has {n}", recs[j].name, comps.len()));
            }
            ids.push((recs[j].id, comps[..n].to_vec()));
        }
        // The native awakes in a scratch Game: the clone's colliders', hinges' and anchored joints' native state.
        let original_ids = db.load_order()?;
        let mut scratch = Game::default();
        scratch.world = crate::b2::world::World::new(crate::b2::math::Vec2::new(-0.0, -9.81));
        let root_trs = trs(recs[0])?;
        let mut next = ids.into_iter();
        let mut inst = scratch.clone_prefab(&prefab, [root_trs.t[0], root_trs.t[1], root_trs.t[2]], root_trs.q, |_, _| next.next().unwrap());
        let order = scratch.awake_instances(std::slice::from_mut(&mut inst), &original_ids).pop().unwrap();
        let mut kept: Vec<usize> = Vec::new();
        for (o, &j) in map.iter().enumerate() {
            if gone[o] {
                (inst.objects[o].active, inst.objects[o].self_active) = (false, false);
                self.destroyed_components.extend(inst.objects[o].components.iter().copied());
                continue;
            }
            let r = recs[j];
            inst.objects[o].trs = trs(r)?;
            let (active, self_active) = r.req("active")?.split_once(',').ok_or("Clone active")?;
            // An object ExistInDetailView.Start switched off (the detail view off) was active at Instantiate: its scripts
            // awoke. It keeps the prefab's activity here; the queued Start below switches it off again.
            let detail_view_off = self_active == "0" && !self.ShowLimbStatus && inst.objects[o].active
                && prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Script { class, .. } if class == "ExistInDetailView"));
            if detail_view_off || (self_active == "1" && prefab.objects[o].parent.is_some_and(|q| kept.contains(&q))) {
                kept.push(o);
                continue;
            }
            (inst.objects[o].active, inst.objects[o].self_active) = (active == "1", self_active == "1");
            inst.objects[o].activated |= active == "1";
        }
        // A detached object's record is its world pose (a root); its DetachFromParentOnStart.Start ran (the queued Start
        // only adds the ActOnDestroy listener, `recorded_detached`).
        for &o in &detached {
            let t = trs(recs[map[o]])?;
            let parent = inst.objects[o].parent.ok_or("a detached record of a prefab root")?;
            inst.set_parent_null_at(o, ([t.t[0], t.t[1], t.t[2]], t.q, [t.s[0], t.s[1], t.s[2]]));
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                if matches!(comp, Component::Script { class, .. } if class == "DetachFromParentOnStart" || class == "IgnoreParentSize") {
                    self.recorded_detached.insert(inst.objects[o].components[c], parent);
                }
            }
        }
        // An item spawned after the launch (PPG_FNPROBE) has no header `collider` lines: its Clone records' layers.
        if kind == "Item" {
            for (o, &j) in map.iter().enumerate().filter(|&(o, _)| !gone[o]) {
                if let Some(l) = recs[j].get("layer") {
                    self.world.layers.insert(inst.objects[o].game_object, l.parse().map_err(|_| "Clone layer")?);
                }
            }
        }
        // The restored world's bodies, fixtures and joints, by the recorded names and ids.
        let key = |inst: &Instance, o: usize, c: usize| format!("{}#{}", if gone[o] { "" } else { recs[map[o]].name.as_str() }, inst.objects[o].components[c]);
        let body_component = |o: usize| prefab.objects[o].components.iter().position(|(_, c)| matches!(c, Component::Rigidbody2D(_)));
        // (Every object's: one inactive in the prefab, so without a body in the scratch, that the recording has active, as a
        // Lightbulb's Bulb top its Start activated and detached, has its restored body and fixtures too.)
        inst.bodies = (0..prefab.objects.len()).filter_map(|o| Some((o, *rs.bodies.get(&key(&inst, o, body_component(o)?))?))).collect();
        let mut fixtures = std::mem::take(&mut inst.fixtures);
        for o in (0..prefab.objects.len()).filter(|&o| !gone[o] && inst.objects[o].active) {
            for (c, _) in prefab.objects[o].components.iter().enumerate().filter(|(_, (_, x))| matches!(x, Component::Collider2D(_))) {
                let n = (0..).take_while(|k| rs.fixtures.contains_key(&format!("{}:{k}", key(&inst, o, c)))).count();
                if n > 0 {
                    fixtures.entry((o, c)).or_insert_with(|| vec![usize::MAX; n]);
                }
            }
        }
        inst.fixtures = fixtures
            .iter()
            .filter_map(|(&(o, c), v)| (0..v.len()).map(|k| rs.fixtures.get(&format!("{}:{k}", key(&inst, o, c))).copied()).collect::<Option<Vec<_>>>().map(|v| ((o, c), v)))
            .collect();
        inst.joints = inst.joints.keys().filter_map(|&(o, c)| Some(((o, c), *rs.joints.get(&key(&inst, o, c))?))).collect();
        // The restore names GameObjects by its own ids: the clone's bodies and fixtures take the recorded ones (a
        // collision on a body's own GameObject is sent to it once).
        // (The names' ids and their layers go with them.)
        let mut moved = std::collections::HashMap::new();
        for (&o, &b) in &inst.bodies {
            if let Some(u) = self.world.body_mut(b).user.as_mut() {
                moved.insert(std::mem::replace(&mut u.game_object, inst.objects[o].game_object), inst.objects[o].game_object);
            }
        }
        for (&(o, _), fs) in &inst.fixtures {
            for &f in fs {
                moved.insert(std::mem::replace(&mut self.world.fixtures[f].as_mut().unwrap().collider.game_object, inst.objects[o].game_object), inst.objects[o].game_object);
            }
        }
        for (from, to) in moved {
            if let Some(l) = self.world.layers.remove(&from) {
                self.world.layers.insert(to, l);
            }
            self.objects.values_mut().filter(|v| **v == from).for_each(|v| *v = to);
        }
        // An object without a restored collider (inactive, as an Energy Sword's energy_collider) keeps the layer the
        // prefab gave it at Instantiate (the one script that sets a layer, FirearmAttachmentPointBehaviour, does it on
        // an object whose collider the restore holds).
        for (o, c) in inst.objects.iter().enumerate() {
            self.world.layers.entry(c.game_object).or_insert(prefab.objects[o].layer as i32);
        }
        let objects = inst.objects.clone();
        for (&(o, c), a) in inst.anchored.iter_mut() {
            let id = objects[o].components[c];
            a.slot = self.anchored_joints.iter().position(|x| x.component == id);
            // A recorded SliderJoint2D is a `fixed_joints` entry already: its slot is an adopted one.
            if let (None, Some(&f)) = (a.slot, self.adopted_joints.get(&id)) {
                let st = &self.fixed_joints[f].state;
                self.anchored_joints.push(crate::game::AnchoredJoint { break_force: st.break_force, break_torque: st.break_torque, joint: None, hinge: None, component: id });
                a.slot = Some(self.anchored_joints.len() - 1);
                self.adopted_slots.insert(self.anchored_joints.len() - 1, f);
            }
            // gapB r26: a joint missing from its object's recorded comps was destroyed before this snapshot (a wrecked Car's
            // WheelJoint2Ds, broken off): it was there for the Awakes and Starts (CarBehaviour.Awake's SetBreakForce), so it
            // takes a slot without a b2 joint, with the prefab's limits; the destroy after the Starts takes it out again.
            if a.slot.is_none() && !gone[o] && !super::list(recs[map[o]].req("comps")?).iter().any(|x| x.rsplit_once(':').is_some_and(|x| x.1 == id.to_string())) {
                self.anchored_joints.push(crate::game::AnchoredJoint { break_force: a.base.break_force, break_torque: a.base.break_torque, joint: None, hinge: None, component: id });
                a.slot = Some(self.anchored_joints.len() - 1);
            }
        }
        // The hinges' slots: an item hinge's AnchoredJoint, a limb's taken over by its `Game::hinges` entry.
        for (&(o, c), h) in inst.hinges.iter_mut() {
            let id = objects[o].components[c];
            h.slot = match self.anchored_joints.iter().position(|x| x.component == id) {
                Some(slot) => Some(slot),
                None => match s.of("HingeJoint2D").position(|r| r.id == id) {
                    Some(k) => {
                        let r = s.of("HingeJoint2D").nth(k).unwrap();
                        let (break_force, break_torque) = (super::f(r.req("breakForce")?), super::f(r.req("breakTorque")?));
                        self.anchored_joints.push(crate::game::AnchoredJoint { break_force, break_torque, joint: None, hinge: Some(k), component: id });
                        Some(self.anchored_joints.len() - 1)
                    }
                    None => None,
                },
            };
        }
        // The physicals' Transforms are the clone's (the scripts refresh the clone's Transforms from them).
        for o in 0..objects.len() {
            for &id in &objects[o].components {
                if let Some(p) = self.physicals.iter_mut().find(|p| p.id == id) {
                    p.transform_id = objects[o].components[0];
                    // and their activeInHierarchy (an inactive part of a broken car).
                    p.active = objects[o].active;
                }
            }
        }
        // The physicals' SpriteRenderers are the prefab's (the records keep their bounds): the sprite, colour, sorting
        // layer and order that LayerEditorController and the drawing read.
        let file = prefab.objects[0].game_object.file;
        for o in 0..objects.len() {
            let Some((k, r)) = prefab.objects[o].components.iter().enumerate().find_map(|(k, (_, c))| match c {
                Component::SpriteRenderer(r) => Some((k, r)),
                _ => None,
            }) else {
                continue;
            };
            let Some(p) = self.physicals.iter_mut().find(|p| p.sprite.is_none() && objects[o].components.contains(&p.id)) else { continue };
            p.sprite = Some((db.resolve(file, r.sprite).ok_or("a SpriteRenderer without a sprite")?, r.color, r.sorting_order));
            p.sprite_enabled = r.enabled;
            p.sprite_id = objects[o].components[k];
            p.sprite_layer = r.sorting_layer_id;
            p.sprite_game_layer = prefab.objects[o].layer as i32;
            p.sprite_material = r.materials.first().and_then(|&m| db.resolve(file, m));
            p.sprite_draw = ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1);
        }
        // The clone's PointEffector2Ds (the recording has no effector rows): the scratch awake's, their contact arrays
        // from the restored touching contacts. Their scripts' Awake sets them up again.
        let base = self.point_effectors.len();
        self.point_effectors.extend(scratch.point_effectors);
        self.effector_updates.extend(scratch.effector_updates.iter().map(|&e| e + base));
        self.world.effector_colliders.extend(scratch.world.effector_colliders);
        for (id, mut e) in scratch.world.effectors {
            e.contacts.clear();
            self.world.effectors.insert(id, e);
        }
        for c in self.world.contact_list.clone() {
            if self.world.contact(c).flags & crate::b2::world::CONTACT_TOUCHING != 0 {
                self.world.effector_begin_contact(c);
            }
        }
        self.retained.push(inst);
        // The first id the scripts' Awake and Start took: the highest of the records no prefab object maps to.
        // (The components AddComponent put on prefab objects, after the prefab's own, count too; not what a PhysicalBehaviour's
        // Awake/Start made, its record's ActivationNode, ContextMenuOptions, audioSource, slidingAudioSource and
        // selectionOutlineObject: the loaded PhysicalBehaviour keeps those ids and the link takes none again.)
        let kept: std::collections::HashSet<i32> = s.of("PhysicalBehaviour").flat_map(|r| ["ActivationNode", "ContextMenuOptions", "audioSource", "slidingAudioSource", "selectionOutlineObject"].into_iter().filter_map(|k| r.get(k).and_then(super::reference))).collect();
        // An item's PhysicalBehaviour has no such record: its Awake's GetOrAddComponent<NodeBehaviour> and
        // AddComponent<ContextMenuOptionComponent> are the added components of those classes on its object.
        // (So is FirearmAttachmentBehaviour.Awake's GetOrAddComponent<CheckOnDestroyAttachment>: the loaded one is found.)
        let physical_made = |o: usize, c: &str| c.starts_with("CheckOnDestroyAttachment:") || (c.starts_with("NodeBehaviour:") || c.starts_with("ContextMenuOptionComponent:"))
            && prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Script { class, .. } if class == "PhysicalBehaviour"));
        let added = map.iter().enumerate().filter(|&(o, _)| !gone[o]).flat_map(|(o, &j)| super::list(recs[j].req("comps").unwrap()).into_iter().skip(prefab.objects[o].components.len()).filter(|c| !physical_made(o, c)).map(|c| c.rsplit_once(':').unwrap().1.parse::<i32>().unwrap()).collect::<Vec<_>>()).filter(|id| !kept.contains(id));
        let runtime = (0..recs.len()).filter(|&j| !map.contains(&j) && !kept.contains(&recs[j].id)).map(|j| recs[j].id).chain(added).max();
        Ok(Some((prefab, order, runtime)))
    }

    /// The recorded item's scripts, deserialized over its restored PhysicalBehaviours and hinges (link_instance), then
    /// their Awake and Start as the spawn ran them before the recording began: their Random draws are in the recorded
    /// state already, and the objects they made take their recorded ids from `runtime` on. The PhysicalBehaviours keep
    /// their records. The clone is the last one `recording_clone` kept.
    pub(super) fn link_recording_item(&mut self, s: &Snapshot, at: usize, prefab: &Prefab, order: &[(usize, usize)], runtime: Option<i32>) -> Result<(), String> {
        // batch2 r14: a limb-holding item (a Gorse, a second Person) links like a spawn: its PersonBehaviour, LimbBehaviours and
        // CirculationBehaviours are linked from the prefab, their recorded ItemScript fields go over them (load_item_scripts).
        if at >= self.retained.len() {
            return Err("no item clone".into());
        }
        let mut inst = self.retained.remove(at);
        let mut reuse = crate::game::link::Reuse::default();
        for (o, po) in prefab.objects.iter().enumerate() {
            for (c, (_, comp)) in po.components.iter().enumerate() {
                let id = inst.objects[o].components[c];
                let Component::Script { class, .. } = comp else { continue };
                if class == "PhysicalBehaviour" {
                    if let Some(p) = self.physicals.iter().position(|p| p.id == id) {
                        reuse.physical.insert(o, p);
                    }
                } else if crate::game::syringe::CLASSES.contains(&class.as_str()) {
                    reuse.syringe.insert(o, self.syringes.iter().position(|x| x.id == id).ok_or("an item syringe without its record")?);
                }
            }
        }
        let (random, next_id) = (self.random.clone(), self.next_instance_id);
        // What the scripts' collider refilters (IgnoreCollision) did to the world then is in the restored one: the bodies'
        // motion and sleep state and the broad-phase move buffer go back to it after the Starts.
        let bodies: Vec<_> = self.world.bodies.iter().map(|b| b.as_ref().map(|b| (b.flags, b.sleep_time, b.linear_velocity, b.angular_velocity, b.force, b.torque))).collect();
        // And their types: FreezeBehaviour.Start's RequestFreeze makes a body Static that the recording has unfrozen (an
        // earlier case's OnDestroy took it off FreezeStackController's stack).
        let kinds: Vec<_> = self.world.bodies.iter().map(|b| b.as_ref().map(|b| b.kind)).collect();
        let moves = self.world.broad_phase.move_buffer.clone();
        // The restored prismatic joints too: a script's Start sets their limits (WinchBehaviour.UpdateWinchLimits from the
        // prefab's limits), which the recorded world already holds as the item's own were then.
        use crate::b2::world::JointKind;
        let prismatics: Vec<(usize, crate::b2::prismatic::PrismaticJoint)> = self.world.joints.iter().enumerate()
            .filter_map(|(i, j)| match j.as_ref().map(|j| &j.kind) { Some(JointKind::Prismatic(p)) => Some((i, p.clone())), _ => None }).collect();
        // The restored hinges' b2 joints too: a limb's Start (LimbBehaviour's SetMotorStrengthToMuscleStrength from the prefab's
        // motor) rewrites the motor a Gorse's muscles had set by the snapshot.
        let revolutes: Vec<(usize, crate::b2::revolute::RevoluteJoint)> = self.world.joints.iter().enumerate()
            .filter_map(|(i, j)| match j.as_ref().map(|j| &j.kind) { Some(JointKind::Revolute(r)) => Some((i, r.clone())), _ => None }).collect();
        // And the recorded joint components' settings (IndustrialLiftBehaviour.Start re-creates its sliders: their auto
        // connectedAnchor from the bodies' poses now, not then).
        let joint_settings: Vec<_> = self.fixed_joints.iter().map(|j| (j.state.clone(), j.slider.clone())).collect();
        // So are the Transforms the Starts move (PistonBehaviour.Start's Translate of its head along z).
        let transforms: Vec<_> = self.physicals.iter().map(|p| p.transform.clone()).collect();
        self.next_instance_id = runtime.unwrap_or(next_id);
        let mut db = self.assets.take().ok_or("the item clone without the game's data")?;
        // The objects' recorded activity: an Awake that SetActives an object (EnergySwordBehaviour.Awake's SetActivation of
        // its MotionBlur from the prefab's Activated) must not undo a later recorded SetActive(true) (the sword switched on).
        // (An object recording_clone left active for its Awake, ExistInDetailView's, takes its record's activity here.)
        let record_active: std::collections::HashMap<i32, (bool, bool)> = s.of("Clone").filter_map(|r| {
            let t = r.get("comps")?.split(['[', ';']).find_map(|c| c.strip_prefix("Transform:"))?.parse().ok()?;
            let (a, sa) = r.get("active")?.split_once(',')?;
            Some((t, (a == "1", sa == "1")))
        }).collect();
        let recorded_active: Vec<(bool, bool, bool)> = inst.objects.iter().map(|x| {
            let (a, sa) = record_active.get(&x.components[0]).copied().filter(|_| x.active).unwrap_or((x.active, x.self_active));
            (a, sa, x.activated)
        }).collect();
        let linked = (|| {
            // Shoot's DistantSoundBehaviour.Play draws from the level's clip lists.
            self.load_distant_sounds(&mut db)?;
            let linked = self.link_instance_reusing(&mut db, prefab, &inst, reuse)?;
            // The recorded scripts ran their Awake and Start already.
            let recorded = |class: &str| class == "PhysicalBehaviour" || crate::game::syringe::CLASSES.contains(&class);
            let order: Vec<(usize, usize)> = order.iter().copied().filter(|&(o, c)| !matches!(&prefab.objects[o].components[c].1, Component::Script { class, .. } if recorded(class))).collect();
            let class = |&(o, c): &(usize, usize)| match &prefab.objects[o].components[c].1 {
                Component::Script { class, .. } => class.as_str(),
                _ => "",
            };
            // A random outcome is the recorded one, not a new draw: RandomSpawnedSize.Awake's scale is the Clone
            // records' (it registers with no manager); FoliageBehaviour.Start's z jitter too, its burn progress below.
            // FirearmAttachmentBehaviour.Awake's GetOrAddComponent<CheckOnDestroyAttachment> finds the recorded one: no id.
            let attachment = |c: &str| matches!(c, "FirearmAttachmentBehaviour" | "ScopeAttachmentBehaviour" | "BatteryAttachmentBehaviour" | "ExplosiveAttachmentBehaviour" | "FlashlightAttachmentBehaviour" | "IncendiaryAttachmentBehaviour");
            let awake: Vec<(usize, usize)> = order.iter().copied().filter(|x| class(x) != "RandomSpawnedSize" && !attachment(class(x))).collect();
            // A map light's Start (Activated, its OnGroup and the DelayedActivation wake) is the recorded state too.
            let starts: Vec<(usize, usize)> = order.iter().copied().filter(|x| !matches!(class(x), "FoliageBehaviour" | "MapLightBehaviour")).collect();
            let root_scale = { let s = inst.objects[0].trs.s; [s[0], s[1], s[2]] };
            self.awake_behaviours(Some(&mut *db), prefab, &mut inst, &linked, &awake);
            // PersonBehaviour.Awake's RandomisedSize drew the scale at the spawn: the Clone record's is that draw.
            if let Some(p) = linked.person {
                inst.set_local_scale(0, root_scale);
                self.people[p].localScale = root_scale;
                self.refresh_transforms(&inst, &linked);
            }
            // RandomSpriteBehaviour.Awake's chosenIndex and BreakJointOnBurnBehaviour.Awake's randomOffset, as recorded.
            for (&o, &r) in &linked.random_sprites {
                let id = self.random_sprites[r].id;
                if let Some(rec) = s.of("ItemRandomSpriteBehaviour").find(|x| x.id == id) {
                    self.random_sprites[r].chosenIndex = rec.req("chosenIndex")?.parse().map_err(|_| "chosenIndex")?;
                    self.random_sprite_set_to_index(r);
                }
                let _ = o;
            }
            for b in self.break_joint_on_burns.iter_mut() {
                if let Some(rec) = s.of("ItemBreakJointOnBurnBehaviour").find(|x| x.id == b.id) {
                    b.randomOffset = super::f(rec.req("randomOffset")?);
                }
            }
            // The recorded scripts' Start-made context menu buttons (the menu is no recorded state): PhysicalBehaviour.Start's,
            // then SyringeBehaviour.AddButtons, before the other scripts' Starts as their components come later.
            for &q in linked.physicals.values() {
                self.physical_add_menu_buttons(q);
            }
            for s in 0..self.syringes.len() {
                if linked.physicals.values().any(|&q| q == self.syringes[s].Phys) {
                    self.syringe_add_buttons(s);
                }
            }
            self.queue_starts(Some(&mut *db), prefab, &inst, &linked, &starts)?;
            for f in 0..self.foliages.len() {
                if let Some(rec) = s.of("ItemFoliageBehaviour").find(|x| x.id == self.foliages[f].id) {
                    // The recorded field and its property block only: the object's active state is the record's own (a
                    // fuzzed BurnProgress >= 0.95 on an active leaf stays active until its Start), and the clone is not in
                    // `retained` yet for SetBurnProgressInternal's SetActive.
                    self.foliage_burn_progress_block(f, super::f(rec.req("BurnProgress")?));
                }
            }
            // The item's PhysicalBehaviours' Rigidbody2D: its movement state (interpolation poses) as recorded.
            for ph in self.physicals.iter_mut() {
                if let Some(w) = s.of("ItemB2Body").find(|r| r.id == ph.rigidbody.component).and_then(|r| r.get("move")) {
                    ph.rigidbody.movement = super::movement_state(&super::words(w));
                }
            }
            // A Rigidbody2D without a PhysicalBehaviour (a tree's leaves): its movement state as recorded.
            for n in self.node_rigidbodies.iter_mut() {
                if let Some(w) = s.of("ItemB2Body").find(|r| r.id == n.rigidbody.component).and_then(|r| r.get("move")) {
                    n.rigidbody.movement = super::movement_state(&super::words(w));
                }
            }
            Ok::<_, String>(linked)
        })();
        self.assets = Some(db);
        let linked = linked?;
        self.recorded_linked.insert(inst.objects[0].game_object, linked.clone());
        for (x, &(active, self_active, activated)) in inst.objects.iter_mut().zip(&recorded_active) {
            if active && !x.active {
                (x.active, x.self_active, x.activated) = (true, self_active, x.activated | activated);
            }
        }
        if inst.objects.iter().any(|x| !x.activated) {
            self.first_activation.insert(inst.objects[0].components[0], linked);
        }
        self.retained.insert(at, inst);
        // RandomForceAtStart.Start's impulse is in the restored bodies, which go back below (a debris root's Rigidbody2D
        // has no PhysicalBehaviour to push), and NewAtomBombExplosionBehaviour.Start's heat and vaporising in the recorded others.
        self.pending_starts.retain(|p| !matches!(p, crate::game::misc::PendingStart::RandomForce(..) | crate::game::misc::PendingStart::AtomExplosion(..)));
        // The components the Clone records list: the rest are gone by the snapshot, destroyed below after these Starts (a MapLiftBehaviour.Start took its slider, broken since).
        self.recorded_present = s.of("Clone").filter_map(|r| r.get("comps")).flat_map(super::list).filter_map(|c| c.rsplit_once(':').and_then(|x| x.1.parse().ok())).collect();
        let root_go = self.retained[at].objects[0].game_object;
        let recorded_on: Vec<i32> = self.retained[at].objects.iter().zip(&recorded_active).filter(|x| x.1.0).map(|x| x.0.components[0]).collect();
        // gapB r27: an ExplosiveBehaviour's Killzone link (Option<i32>, not a Field): its Start makes the recorded Killzone(Clone).
        let clones: std::collections::HashSet<i32> = s.of("Clone").map(|r| r.id).collect();
        self.recorded_killzones = s.of("ItemScript").filter(|r| r.get("class") == Some("ExplosiveBehaviour"))
            .filter_map(|r| Some((r.id, r.get("Killzone").and_then(super::reference).filter(|k| clones.contains(k))?))).collect();
        self.run_pending_starts();
        self.recorded_killzones.clear();
        // gapA r2: those Starts ran with the prefab's fields (load_item_scripts comes after): a child's local rotation
        // goes back to its Clone record's, the snapshot's (BlasterBehaviour.Start flipped the Muzzleflash of a recorded
        // FlipMuzzleFlash=0 blaster).
        if let Some(i) = self.retained.iter().position(|x| x.objects[0].game_object == root_go) {
            let poses: Vec<(usize, Trs)> = self.retained[i].objects.iter().enumerate().filter(|(_, x)| x.parent.is_some()).filter_map(|(o, x)| {
                let t = format!("Transform:{}", x.components[0]);
                let r = s.of("Clone").find(|r| r.get("parent") != Some("-1") && r.get("comps").is_some_and(|c| c.trim_matches(['[', ']']).split(';').any(|y| y == t)))?;
                Some((o, trs(r).ok()?))
            }).collect();
            // gapA r3: its local position and scale too (a Start's Translate or localScale set from prefab fields).
            for (o, t) in poses {
                self.retained[i].set_local_position(o, [t.t[0], t.t[1], t.t[2]]);
                self.retained[i].store_local_rotation(o, t.q);
                self.retained[i].set_local_scale(o, [t.s[0], t.s[1], t.s[2]]);
            }
        }
        self.recorded_present.clear();
        // A Start that SetActives an object off from the prefab's state (EnergySwordBehaviour.Start's SetActivation with
        // the prefab's Activated=0) ran in the game before the recorded switch-on: the recorded active objects go back on,
        // parents first, with their OnEnable and AddToManager (the blur's object read active=false after this load).
        for id in recorded_on {
            let held = |i: &crate::game::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
            if self.destroyed_components.contains(&id) || !self.retained.iter().chain(&self.unsynced).any(held) {
                continue;
            }
            self.with_component_instance(id, |g, inst, o| {
                if !inst.objects[o].active {
                    g.set_active_scripts(inst, o, true);
                }
            });
        }
        // The holes a Shot made after the spawn (LiquidContainerLeakingBehaviour.holes): CreateHole again, in order, each
        // from its recorded hole object's id; load_item_scripts then gives them their recorded fields.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("LiquidContainerLeakingBehaviour")) {
            let Some(l) = self.liquid_leaks.iter().position(|x| x.id == r.id) else { continue };
            let roots: Vec<i32> = s.of("Clone").filter(|c| c.get("root") == Some("Item") && c.name == "Liquid_Container_Hole(Clone)").map(|c| c.id).collect();
            let holes: Vec<&str> = r.req("holes")?.split("Hole(").skip(1).collect();
            if !self.liquid_leaks[l].holes.is_empty() || holes.len() > roots.len() {
                continue;
            }
            for (h, &root) in holes.iter().zip(&roots) {
                let v: Vec<f32> = h.split(['=', ',', ' ', ')']).filter(|w| w.len() == 8).filter_map(|w| u32::from_str_radix(w, 16).ok()).map(f32::from_bits).collect();
                let [px, py, dx, dy, intensity] = v[..] else { return Err(format!("LiquidContainerLeakingBehaviour.holes: {h}")) };
                self.next_instance_id = root;
                self.liquid_leak_create_hole(l, [px, py], [dx, dy], intensity);
            }
            self.run_pending_starts();
        }
        (self.random, self.next_instance_id) = (random, next_id);
        for p in 0..self.physicals.len() {
            let rb = &self.physicals[p].rigidbody;
            let Some(Some(kind)) = (rb.detached.is_none()).then(|| kinds.get(rb.body).copied().flatten()) else { continue };
            if self.world.body(rb.body).kind != kind {
                self.world.set_body_type(rb.body, kind);
                self.physicals[p].rigidbody.bodyType = match kind {
                    crate::b2::world::BodyType::Dynamic => 0,
                    crate::b2::world::BodyType::Kinematic => 1,
                    crate::b2::world::BodyType::Static => 2,
                };
                if kind != crate::b2::world::BodyType::Static {
                    self.freeze_stack.remove(&p);
                }
            }
        }
        // A Rigidbody2D without a PhysicalBehaviour (a Fireball's): the recorded body type, on the body and the component.
        for n in 0..self.node_rigidbodies.len() {
            let rb = &self.node_rigidbodies[n].rigidbody;
            let Some(Some(kind)) = (rb.detached.is_none()).then(|| kinds.get(rb.body).copied().flatten()) else { continue };
            if self.world.body(rb.body).kind != kind {
                self.world.set_body_type(rb.body, kind);
            }
            self.node_rigidbodies[n].rigidbody.bodyType = match kind {
                crate::b2::world::BodyType::Dynamic => 0,
                crate::b2::world::BodyType::Kinematic => 1,
                crate::b2::world::BodyType::Static => 2,
            };
        }
        for (b, saved) in self.world.bodies.iter_mut().zip(bodies) {
            if let (Some(b), Some(s)) = (b.as_mut(), saved) {
                (b.flags, b.sleep_time, b.linear_velocity, b.angular_velocity, b.force, b.torque) = s;
            }
        }
        for (p, t) in self.physicals.iter_mut().zip(transforms) {
            p.transform = t;
        }
        for (j, (state, slider)) in self.fixed_joints.iter_mut().zip(joint_settings) {
            (j.state, j.slider) = (state, slider);
        }
        self.world.broad_phase.move_buffer = moves;
        for (i, p) in prismatics {
            if let Some(JointKind::Prismatic(q)) = self.world.joints[i].as_mut().map(|j| &mut j.kind) {
                *q = p;
            }
        }
        for (i, r) in revolutes {
            if let Some(JointKind::Revolute(q)) = self.world.joints[i].as_mut().map(|j| &mut j.kind) {
                *q = r;
            }
            // HingeJoint2D's managed motor as the b2 joint holds it (the ItemHingeJoint2D record has no managed motor): the
            // speed back in degrees.
            if let Some(h) = self.hinges.iter_mut().find(|h| h.joint == Some(i)) {
                let d = &self.world.joints[i].as_ref().unwrap().revolute().def;
                if d.motor_speed != h.motor[0] * crate::game::mathf::DEG2RAD || d.max_motor_torque != h.motor[1] {
                    h.motor = [d.motor_speed * crate::game::mathf::RAD2DEG, d.max_motor_torque];
                }
                h.useMotor = d.enable_motor;
            }
        }
        // A component missing from its object's recorded list was destroyed before this snapshot (a broken joint):
        // Object.Destroy it, after the Awakes and Starts that ran while it was there.
        let present: std::collections::HashSet<i32> = s.of("Clone").filter_map(|r| r.get("comps")).flat_map(super::list).filter_map(|c| c.rsplit_once(':').and_then(|x| x.1.parse().ok())).collect();
        let recorded: std::collections::HashSet<i32> = s.of("Clone").map(|r| r.id).collect();
        // (A Start can move the clone to `unsynced`: fnb4_13's vase debris.)
        let inst = self.retained.iter().chain(&self.unsynced).find(|i| i.objects[0].game_object == root_go).ok_or("the item clone went in its Starts")?;
        let gone: Vec<_> = inst.objects.iter().enumerate().filter(|(_, x)| recorded.contains(&x.game_object))
            .flat_map(|(o, x)| x.components.iter().enumerate().map(move |(c, &id)| (o, c, id)))
            .filter(|&(_, _, id)| !present.contains(&id))
            .map(|(o, c, id)| (id, format!("{} {:.60}", inst.prefab.objects[o].name, format!("{:?}", inst.prefab.objects[o].components[c].1)), matches!(inst.prefab.objects[o].components[c].1, Component::Joint2D(_)), inst.joints.get(&(o, c)).copied(),
                matches!(&inst.prefab.objects[o].components[c].1, Component::Script { class, .. } if class == "BreakJointOnShotBehaviour" || class == "BreakJointOnBurnBehaviour" || class == "RandomForceAtStart" || class == "AntennaBehaviour"))).collect();
        // The joints first: a wire's OnDestroy then finds its joint gone (destroyed in the game before the wire, by its break).
        let mut gone = gone;
        gone.sort_by_key(|g| !g.2);
        for (id, what, is_joint, b2, break_script) in gone {
            use crate::game::BreakJoint as J;
            let joint = self.joint_break_limits.iter().copied().find(|k| match *k {
                J::Anchored(a) => self.anchored_joints[a].component == id,
                J::Hinge(h) => self.anchored_joints.iter().any(|a| a.hinge == Some(h) && a.component == id),
                J::Fixed(f) => self.adopted_joints.get(&id) == Some(&f),
                _ => false,
            });
            match joint {
                Some(j) => self.destroy_break_joint(j),
                // A joint outside the break-limit list (an IndustrialLift's slider the link left null): its b2 joint, if any.
                // An adopted joint outside the break-limit list (a map wire's unbreakable-by-limit DistanceJoint2D): its own Destroy.
                None if is_joint && self.adopted_joints.contains_key(&id) => {
                    let f = self.adopted_joints[&id];
                    self.fixed_joint_destroy(f);
                    self.destroyed_components.insert(id);
                }
                None if is_joint => {
                    if let Some(b2) = b2 {
                        self.world.destroy_joint(b2);
                    }
                    self.destroyed_components.insert(id);
                }
                // A BreakJointOn{Shot,Burn}Behaviour's OnJointBreak2D destroyed it with its joint; RandomForceAtStart.Start, itself.
                // An AntennaBehaviour the fnweap probe DestroyImmediate'd (EnergyWireBehaviour.TransferAntennaSignal's destroyed case).
                // A wire (CopperWireBehaviour) destroyed itself when its joint broke (OnJointBreak2D).
                None if self.blood_wires.iter().any(|x| x.id == id) => {
                    self.blood_wire_run_destroy(id);
                }
                None if break_script => {
                    self.tr_destroy_component(id, 0.0);
                    self.tr_run_component_destroy(id);
                }
                // OnlyExistWith.Update's Object.Destroy(this) (its Other gone).
                None if self.only_exist_withs.iter().any(|x| x.id == id) => {
                    let i = self.only_exist_withs.iter().position(|x| x.id == id).unwrap();
                    self.destroyed_components.insert(id);
                    self.behaviours.remove(crate::game::Behaviour::OnlyExistWith(i));
                }
                None => return Err(format!("component #{id} ({what}) destroyed before this snapshot: only a joint's and a joint-break script's Destroy are rewritten here")),
            }
        }
        // A gone object's PhysicalBehaviour (a shattered Window): out of PhysicalObjectsInWorld, a destroyed `!physicalBehaviour`.
        for p in 0..self.physicals.len() {
            if !self.physicals[p].destroyed && self.destroyed_components.contains(&self.physicals[p].id) {
                self.physicals[p].enabled = false;
                self.physicals[p].destroyed = true;
            }
        }
        Ok(())
    }

    /// The recorder's `ItemScript` records (every MonoBehaviour of the item but its PhysicalBehaviours and syringes, all fields
    /// and Behaviour.enabled as `enabled`) over the linked item's `script!` structs: each dumped field by name, and each
    /// Field-typed link a record has under its name when the value reads as that type (a LayerMask link does not).
    /// A recorded ObjectPoolBehaviour reference `@name#id`: the BulletTracerPool by its record, the BloodPuff pool, a
    /// PoolGenerator pool by its GameObject name (`<prefab name> pool`).
    pub(super) fn pool_ref_of(&self, s: &Snapshot, v: &str) -> Result<crate::game::pool::PoolRef, String> {
        use crate::game::pool::PoolRef;
        let (_, id) = ref_name_id(v).ok_or_else(|| format!("pool {v} is no reference"))?;
        let name = v[1..].rsplit_once('#').unwrap().0;
        if s.of("TracerPool").any(|r| r.id == id) {
            return Ok(PoolRef::Tracer);
        }
        if name == "BloodPuff_poolable_pool" {
            return Ok(PoolRef::BloodPuff);
        }
        let key = |n: &str| format!("{}_pool", n.replace(' ', "_"));
        if let Some(i) = self.generic_pools.iter().position(|p| key(&p.name) == name) {
            return Ok(PoolRef::Generic(i));
        }
        if let Some(i) = self.recorded_pool_items.iter().position(|(n, _, _)| n == name) {
            return Ok(PoolRef::Recorded(i));
        }
        if let Some(i) = self.splash_pools.iter().position(|p| key(&p.prefab.objects[0].name) == name) {
            return Ok(PoolRef::Splash(i));
        }
        Err(format!("pool {v}: no such pool in the rewrite"))
    }

    pub(super) fn load_item_scripts(&mut self, s: &Snapshot) -> Result<(), String> {
        // A spawned person's (an item's: the first record is the scene's person, loaded with the snapshot) Poses from its
        // record, its prefab's poses as the rig (GenerateBasePose's name reads Poses.Count).
        let first = s.of("PersonBehaviour").next().map(|r| r.id);
        for r in s.of("PersonBehaviour").filter(|r| Some(r.id) != first && r.get("Poses").is_some() && r.get("LinkedPoses").is_some()) {
            let Some(i) = self.people.iter().position(|p| p.id == r.id) else { continue };
            let mut p = std::mem::take(&mut self.people[i]);
            let rig: Vec<_> = p.Poses[p.PoseStart..].to_vec();
            let res = super::load_person_poses(self, &mut p, rig, r);
            self.people[i] = p;
            res?;
        }
        // PhysicalBehaviour.SetupSpawnPrefab's SpawnParticles(Clone) (physical.rs physical_setup_spawn_prefab): the
        // DeleteAfterTime on it, keyed by the clone's Transform, and the clone's Transform under its parent's.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("DeleteAfterTime") && r.name == "SpawnParticles(Clone)") {
            if self.delete_afters.iter().any(|x| x.id == r.id) {
                continue;
            }
            let own = format!("DeleteAfterTime:{}", r.id);
            let Some(clone) = s.of("Clone").find(|c| c.get("comps").is_some_and(|x| x.split(['[', ';', ']']).any(|t| t == own))) else { continue };
            let key = clone.req("comps")?.trim_start_matches('[').split(';').find_map(|t| t.strip_prefix("Transform:")).and_then(|t| t.parse().ok()).ok_or("SpawnParticles: no Transform")?;
            // The parent's Transform: the record of the same root at index `parent`.
            let (root, at) = (clone.req("root")?, clone.req("parent")?);
            let parent = s.of("Clone").find(|c| c.get("root") == Some(root) && c.get("index") == Some(at)).and_then(|c| c.get("comps")?.trim_start_matches('[').split(';').find_map(|t| t.strip_prefix("Transform:"))?.parse().ok());
            if let Some(parent) = parent {
                self.add_runtime_clone(parent, &[(key, None)]);
            }
            self.delete_afters.push(crate::game::explosion::DeleteAfter { id: r.id, added_to: Some(key), parent, ..Default::default() });
            let b = crate::game::Behaviour::DeleteAfter(self.delete_afters.len() - 1);
            self.behaviour_of.insert(r.id, (0, b));
            self.behaviours.add(0, b);
        }
        // batch2 r14: a GorseDroolingBehaviour's private step and isAlive as recorded (the link starts them at 0).
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("GorseDroolingBehaviour")) {
            if let Some(g) = self.gorse_droolings.iter_mut().find(|x| x.id == r.id) {
                g.step = r.req("step")?.parse().map_err(|_| "GorseDroolingBehaviour step")?;
                g.isAlive = r.req("isAlive")? == "1";
            }
        }
        // gapB r24: a TyreDeflationBehaviour's SharedDeflatedMaterial (a link, not a Field) as recorded: `@<name>#<id>` by the
        // Material's name (Update and OnWillRenderObject return while it is null), `null` none.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("TyreDeflationBehaviour")) {
            let Some(v) = r.get("SharedDeflatedMaterial") else { continue };
            let Some(t) = self.tyres.iter().position(|x| x.id == r.id) else { continue };
            self.tyres[t].SharedDeflatedMaterial = match v.strip_prefix('@').and_then(|v| v.rsplit_once('#')) {
                None => None,
                Some((n, _)) => {
                    let a = self.assets.as_ref().ok_or("TyreDeflationBehaviour: no game data")?;
                    Some(a.material_by_name(n).or_else(|| a.material_by_name(&n.replace('_', " "))).ok_or_else(|| format!("TyreDeflationBehaviour #{}: no Material {n}", r.id))?)
                }
            };
        }
        // batch2 r13: a LayerSerialisationBehaviour (ContextMenuBehaviour.NoCollideAction's AddComponent, or a pasted
        // contraption's ObjectState): the `layer_serialisations` entry of the PhysicalBehaviour beside it; item_fields gives Layer.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("LayerSerialisationBehaviour")) {
            if self.layer_serialisations.values().any(|x| x.id == r.id) {
                continue;
            }
            let own = format!("LayerSerialisationBehaviour:{}", r.id);
            let clone = s.of("Clone").find(|c| c.get("comps").is_some_and(|x| x.split(['[', ';', ']']).any(|t| t == own))).ok_or_else(|| format!("LayerSerialisationBehaviour #{}: no Clone", r.id))?;
            let pid = clone.req("comps")?.split(['[', ';', ']']).find_map(|t| t.strip_prefix("PhysicalBehaviour:")).and_then(|t| t.parse::<i32>().ok());
            let p = pid.and_then(|id| self.physicals.iter().position(|x| x.id == id)).ok_or_else(|| format!("LayerSerialisationBehaviour #{}: no PhysicalBehaviour beside it", r.id))?;
            self.layer_serialisations.insert(p, crate::game::context_menu::LayerSerialisation { id: r.id, Layer: -1 });
        }
        // batch2 r14: a limb item's CirculationBehaviours (linked by limb, no id): each limb record names its own; then the
        // recorded LiquidDistribution and SerialisableDistributions (item_fields gives the plain fields).
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("LimbBehaviour")) {
            let (Some(l), Some(c)) = (self.limbs.iter().position(|x| x.id == r.id), r.get("CirculationBehaviour").and_then(super::reference)) else { continue };
            if let Some(x) = self.circulations.get_mut(l) {
                x.recorded_id = Some(c);
            }
        }
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("CirculationBehaviour")) {
            let Some(c) = self.circulations.iter_mut().find(|x| x.recorded_id == Some(r.id)) else { continue };
            c.base.LiquidDistribution = Default::default();
            for (id, value) in super::liquid_distribution(r)? {
                c.base.LiquidDistribution.insert(id, value);
            }
            c.base.SerialisableDistributions = super::serialisable_distributions(r)?;
        }
        // A Tritium glow (GlowInTheDarkBehaviour, added at runtime) on a flask: Start again with the recorded ids (its glow
        // object's first), which builds the ModLightPrefab light and its LightSprite; the loop below gives the fields.
        // ponytail: flasks only; a syringe's or a limb's glow is skipped (no recording has one yet).
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("GlowInTheDarkBehaviour")) {
            if self.glows.iter().any(|x| x.id == r.id) {
                continue;
            }
            let own = format!("GlowInTheDarkBehaviour:{}", r.id);
            let Some(clone) = s.of("Clone").find(|c| c.get("comps").is_some_and(|x| x.split(['[', ';', ']']).any(|t| t == own))) else { continue };
            let flask = clone.req("comps")?.split(['[', ';', ']']).find_map(|t| t.strip_prefix("FlaskBehaviour:")).and_then(|t| t.parse::<i32>().ok());
            let Some(f) = flask.and_then(|id| self.flasks.iter().position(|x| x.id == id)) else { continue };
            let Some(glow_go) = r.get("glow").and_then(super::reference) else { continue };
            self.glows.push(crate::game::glow::GlowInTheDark { id: r.id, owner: crate::game::container::Owner::Flask(f), light: None });
            let next = self.next_instance_id;
            self.next_instance_id = glow_go;
            self.glow_start(self.glows.len() - 1);
            self.next_instance_id = next;
        }
        // UnderwaterExplosionBehaviour: its recorded GivenTop, MaxDistanceToSurface and DoSplash.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("UnderwaterExplosionBehaviour")) {
            let Some(u) = self.underwater_explosions.iter_mut().find(|x| x.id == r.id) else { continue };
            let f = |n: &str| r.req(n).and_then(|v| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|_| format!("UnderwaterExplosionBehaviour.{n}: {v}")));
            (u.GivenTop, u.MaxDistanceToSurface, u.DoSplash) = (f("GivenTop")?, f("MaxDistanceToSurface")?, r.req("DoSplash")? == "1");
        }
        // A LightSprite (a Tritium glow's ModLightPrefab): its SpriteRenderer link; the loop below gives the fields.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("LightSprite")) {
            if !self.light_sprites.iter().any(|x| x.id == r.id) {
                let sr = r.get("SpriteRenderer").and_then(super::reference).unwrap_or(0);
                self.light_sprites.push(crate::game::glow::LightSprite { id: r.id, renderer_id: sr, ..Default::default() });
            }
        }
        let mut pairs = std::mem::take(&mut self.ref_pairs);
        // The loaded splats' (FancyBloodSplatController records outside the ItemScripts) Decal ids, the same way.
        for r in s.of("FancyBloodSplatController") {
            if let (Some((n, id)), Some(sp)) = (r.get("Decal").and_then(ref_name_id), self.splats.iter().find(|x| x.id == r.id)) && n == ref_name(&sp.Decal) {
                pairs.insert(id, n);
            }
        }
        for (class, r) in script_records(s) {
            let Some((fields, links)) = super::fnitems::item_fields(self, class, r.id) else { continue };
            for (name, field) in fields {
                if let Some(v) = r.get(name) {
                    field.parse(&script_text(v)).map_err(|e| format!("ItemScript {class} {name}: {e}"))?;
                }
            }
            for (name, field) in links {
                if let Some(v) = r.get(name) {
                    let ours = field.show();
                    // An object reference the rewrite keeps by value (FancyBloodSplatController.Decal: the DecalDescriptor's
                    // name): the value stays; the recorded id pairs with it when it is the recorded object's name.
                    if let Some((n, id)) = ref_name_id(v) {
                        if n == ref_name(&ours) {
                            pairs.insert(id, n);
                        }
                        continue;
                    }
                    if field.parse(&script_text(v)).is_err() {
                        field.parse(&ours).unwrap();
                    }
                }
            }
        }
        self.ref_pairs = pairs;
        // ReturnToPoolAfterTime.pool: null until OnPoolableInitialised (a recorded null stays null), else the pool it names.
        for (class, r) in script_records(s) {
            let Some(v) = r.get("pool").filter(|_| class == "ReturnToPoolAfterTime") else { continue };
            let pool = if v == "null" { None } else { Some(self.pool_ref_of(s, v)?) };
            if let Some(x) = self.return_to_pools.iter_mut().find(|x| x.id == r.id) {
                x.pool = pool;
            }
        }
        // batch2 r10: GuideTemperatureBehaviour.phys: a recorded null stays null (Awake not run, or nulled by a probe).
        for (class, r) in script_records(s) {
            if class == "GuideTemperatureBehaviour" && r.get("phys") == Some("null") && let Some(x) = self.tr.guide_temperature_behaviour.iter_mut().find(|x| x.this_id == r.id) {
                x.phys = None;
            }
        }
        // gapB r11: JukeboxBehaviour.tracks: a recorded null stays null.
        for (class, r) in script_records(s) {
            if class == "JukeboxBehaviour" && let Some(x) = self.jukeboxes.iter_mut().find(|x| x.id == r.id) {
                x.tracks_null = r.get("tracks") == Some("null");
            }
            // AntennaReceiverBehaviour.audioSource destroyed (an OnDestroy case destroyed the "antenna" child) or null: no source.
            if class == "AntennaReceiverBehaviour" && matches!(r.get("audioSource"), Some("destroyed" | "null")) && let Some(x) = self.antenna_receivers.iter_mut().find(|x| x.id == r.id) {
                x.audioSource = 0;
            }
        }
        // A BallisticsEmitter's `tracers`: null once disposed (OnDestroy ran), else the pool tracers it holds (gapA r15:
        // `[@Tracer_poolable(Clone)#id;...]`, an earlier case's bullets still in flight; the pool's active count).
        for (class, r) in script_records(s) {
            let Some(v) = r.get("BallisticsEmitter") else { continue };
            let Some(t) = v.split_once("tracers=").map(|x| x.1) else { continue };
            let n = if t.starts_with("null") { None } else { Some(t.split(']').next().unwrap_or("").matches('@').count()) };
            let e = match class {
                "FirearmBehaviour" => self.firearms.iter_mut().find(|x| x.id == r.id).and_then(|x| x.emitter.as_mut()),
                "MachineGunBehaviour" => self.machine_guns.iter_mut().find(|x| x.id == r.id).and_then(|x| x.emitter.as_mut()),
                _ => None,
            };
            if let Some(e) = e {
                e.tracers = n;
            }
        }
        // A TeslaCoilLightningBoltBehaviour's target (a PhysicalBehaviour: a `physicals` index) and colliderArray (collider ids).
        for (class, r) in script_records(s) {
            if class != "TeslaCoilLightningBoltBehaviour" {
                continue;
            }
            let Some(k) = self.tesla_bolts.iter().position(|x| x.id == r.id) else { continue };
            let id = |t: &str| t.rsplit_once('#').and_then(|x| x.1.parse::<i32>().ok());
            if let Some(t) = r.get("target") {
                self.tesla_bolts[k].target = match id(t) {
                    Some(pid) => Some(self.physicals.iter().position(|p| p.id == pid).ok_or_else(|| format!("tesla bolt target: no physical #{pid}"))?),
                    None => None,
                };
            }
            if let Some(a) = r.get("colliderArray") {
                self.tesla_bolts[k].colliderArray = if a == "null" { Vec::new() } else { super::list(a).into_iter().filter_map(id).collect() };
            }
        }
        // A FirearmAttachmentBehaviour's FirearmBehaviour and AttachmentPoint (`firearms`/`attachment_points` indices), a
        // point's FirearmAttachment, and the firearm's OnFire/BulletEntryEvent listeners: ConnectAttachment adds them with
        // the links, DisconnectAttachment removes them with the links.
        for (class, r) in script_records(s) {
            if !crate::game::attachment::CLASSES.contains(&class) {
                continue;
            }
            let Some(a) = self.firearm_attachments.iter().position(|x| x.id == r.id) else { continue };
            let firearm = r.get("FirearmBehaviour").and_then(super::reference).and_then(|id| self.firearms.iter().position(|f| f.id == id));
            let point = r.get("AttachmentPoint").and_then(super::reference).and_then(|id| self.attachment_points.iter().position(|p| p.id == id));
            (self.firearm_attachments[a].firearm, self.firearm_attachments[a].point) = (firearm, point);
            if let (Some(f), Some(_)) = (firearm, point)
                && !self.firearms[f].attachments.contains(&a)
            {
                self.firearms[f].attachments.push(a);
            }
        }
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("FirearmAttachmentPointBehaviour")) {
            let Some(pt) = self.attachment_points.iter().position(|x| x.id == r.id) else { continue };
            if let Some(v) = r.get("FirearmAttachment") {
                self.attachment_points[pt].FirearmAttachment = super::reference(v).and_then(|id| self.firearm_attachments.iter().position(|x| x.id == id));
            }
        }
        // A LavaBehaviour's lastImpactSplashTime (a link: DoSplash's gate), from each snapshot, not only the map's first.
        for (class, r) in script_records(s) {
            if class != "LavaBehaviour" {
                continue;
            }
            if let (Some(&(_, crate::game::Behaviour::Lava(lv))), Some(t)) = (self.behaviour_of.get(&r.id), r.get("lastImpactSplashTime")) {
                self.lavas[lv].lastImpactSplashTime = super::f(t);
            }
        }
        // gapA r2: the compound ItemScript fields a Field link cannot hold (compound_fields shows them for the compare).
        for (class, r) in script_records(s) {
            match class {
                "LiquidContainerController" => {
                    let Some(&(_, crate::game::Behaviour::LiquidContainer(i))) = self.behaviour_of.get(&r.id) else { continue };
                    let x = &mut self.liquid_containers[i];
                    if let Some(v) = r.get("particles") {
                        x.particles = v.trim_matches(['[', ']']).split(';').filter(|p| !p.is_empty()).map(|p| {
                            let w: Vec<f32> = p.split(['=', ',', ' ', ')']).filter_map(|t| u32::from_str_radix(t, 16).ok().filter(|_| t.len() == 8)).map(f32::from_bits).collect();
                            (w.len() == 4).then(|| crate::game::flask::Particle { Position: [w[0], w[1]], Velocity: [w[2], w[3]] }).ok_or(format!("LiquidContainerController particles: {p}"))
                        }).collect::<Result<_, _>>()?;
                    }
                    if let Some(v) = r.get("particlePositions") {
                        x.particlePositions = (v != "null").then(|| v.trim_matches(['[', ']']).split(';').filter(|p| !p.is_empty()).map(super::floats::<4>).collect());
                    }
                }
                // gapA r14: a FirearmBehaviour's/MachineGunBehaviour's BallisticsEmitter (null before Start) and its value members.
                "FirearmBehaviour" | "MachineGunBehaviour" => {
                    let Some(v) = r.get("BallisticsEmitter") else { continue };
                    let e = if class == "FirearmBehaviour" {
                        let Some(x) = self.firearms.iter_mut().find(|x| x.id == r.id) else { continue };
                        if v == "null" { x.emitter = None; continue; }
                        let (c, l) = (x.Cartridge, x.LayersToHit);
                        x.emitter.get_or_insert_with(|| crate::game::ballistics::Emitter::new(c, l, (1 << 4) | l))
                    } else {
                        let Some(x) = self.machine_guns.iter_mut().find(|x| x.id == r.id) else { continue };
                        if v == "null" { x.emitter = None; continue; }
                        let Some(e) = x.emitter.as_mut() else { continue };
                        e
                    };
                    for (k, t) in emitter_members(v) {
                        if let Some(f) = emitter_field(e, k) {
                            f.parse(t).map_err(|err| format!("{class} BallisticsEmitter {k}: {err}"))?;
                        }
                    }
                }
                // gapA r14: LaserBehaviour.finalHit (whether its hit has a value, its physicalBehaviour, point and rayDepth).
                "LaserBehaviour" => {
                    let Some(v) = r.get("finalHit") else { continue };
                    let (hit, pid, point, depth) = laser_hit(v);
                    let p = self.physicals.iter().position(|x| x.id == pid);
                    let Some(x) = self.lasers.iter_mut().find(|x| x.id == r.id) else { continue };
                    x.final_hit = (hit || pid != 0 || point != [0.0; 2] || depth != 0).then_some((hit, depth, p, point));
                }
                // gapC r16: LaserTurretBehaviour.sentryController's scan timer t (FixedUpdate's GetTargetMotorSpeed adds deltaTime).
                "LaserTurretBehaviour" => {
                    let (Some(x), Some(t)) = (self.laser_turrets.iter_mut().find(|x| x.id == r.id), r.get("sentryController").and_then(sentry_t)) else { continue };
                    x.sentry.t = super::f(t);
                }
                "AntennaReceiverBehaviour" => {
                    let (Some(x), Some(v)) = (self.antenna_receivers.iter_mut().find(|x| x.id == r.id), r.get("audioInformation")) else { continue };
                    let w: Vec<&str> = v.trim_start_matches("AudioInformation(").trim_end_matches(')').split(' ').filter_map(|t| t.split_once('=').map(|t| t.1)).collect();
                    let [old, cur] = w[..] else { return Err(format!("AntennaReceiverBehaviour audioInformation: {v}")) };
                    (x.OldFactor, x.CurrentFactor) = (super::f(old), super::f(cur));
                }
                _ => {}
            }
        }
        // A ReactorCoreBehaviour's criticalityClock.
        for r in s.of("ReactorCoreBehaviour") {
            if let Some(c) = self.reactor_cores.iter_mut().find(|x| x.id == r.id) {
                c.criticalityClock = super::distributor(r.req("criticalityClock")?);
            }
        }
        // A PhysicalRopeCreator's LineRenderer points (TargetLineRenderer.Update's point 1 of the frame).
        for r in s.of("PhysicalRopeCreator") {
            let (Some(text), Some(x)) = (r.get("linePositions"), self.rope_creators.iter().find(|x| x.id == r.id)) else { continue };
            let points: Vec<[f32; 3]> = text.trim_matches(['[', ']']).split(';').filter(|p| !p.is_empty()).map(super::floats::<3>).collect();
            self.lines[x.line].positions = points;
        }
        // A ReactorA5DisplayBehaviour's updateDisplayClock and alertBeepClock.
        for r in s.of("ReactorA5DisplayBehaviour") {
            if let Some(d) = self.reactor_a5_displays.iter_mut().find(|x| x.id == r.id) {
                d.updateDisplayClock = super::distributor(r.req("updateDisplayClock")?);
                d.alertBeepClock = super::distributor(r.req("alertBeepClock")?);
            }
        }
        // A WaterBehaviour's currentlyInWater[0..currentlyInWaterCount) (PhysicalBehaviours, a link of `physicals` indices).
        for r in s.of("WaterBehaviour") {
            let (Some(list_text), Some(count)) = (r.get("currentlyInWater"), r.get("currentlyInWaterCount")) else { continue };
            let Some(w) = self.waters.iter().position(|x| x.id == r.id) else { continue };
            // gapA r5: the game's count goes below 0 (an OnTriggerExit2D with no matching enter: gapa5_reactor -1); its loops
            // over 0..count then read nothing.
            let count = count.parse::<i32>().map_err(|e| format!("currentlyInWaterCount: {e}"))?.max(0) as usize;
            let mut inside = Vec::new();
            for t in super::list(list_text).into_iter().take(count).filter(|&t| t != "null") {
                let id: i32 = t.rsplit_once('#').and_then(|x| x.1.parse().ok()).ok_or_else(|| format!("currentlyInWater entry {t}"))?;
                inside.push(self.physicals.iter().position(|p| p.id == id).ok_or_else(|| format!("currentlyInWater: no physical #{id}"))?);
            }
            self.waters[w].currentlyInWater = inside;
        }
        // The item's SpriteRenderers (`ItemSprite`): colour and enabled.
        for r in s.of("ItemSprite") {
            let Some(at) = self.item_sprite(r.id) else { continue };
            self.set_sprite_color(at, floats(r.req("color")?));
            self.set_sprite_enabled(at, r.req("enabled")? == "1");
            // gapC r11: the MaterialPropertyBlock (`mpb=`, PI/PO): its floats and vectors as recorded; ours' textures stay.
            if let Some(m) = r.get("mpb").filter(|m| *m != "?") {
                use crate::game::props::Prop;
                let mut block = self.get_property_block(at);
                block.0.retain(|_, p| matches!(p, Prop::Texture(_)));
                for (n, x) in m.split(';').filter_map(|x| x.split_once(':')) {
                    let v: Vec<f32> = x[1..].split(',').filter_map(|h| u32::from_str_radix(h, 16).ok().map(f32::from_bits)).collect();
                    match (x.as_bytes()[0], v.as_slice()) {
                        (b'f', [f]) if f.to_bits() != 0 => block.set_float(n, *f),
                        (b'v', &[a, b, c, d]) if [a, b, c, d].iter().any(|x| x.to_bits() != 0) => block.set_vector(n, [a, b, c, d]),
                        _ => {}
                    }
                }
                self.set_property_block(at, &block);
            }
            // gapC r11: sortingLayerID,sortingOrder (`order=`).
            if let Some((l, o)) = r.get("order").and_then(|v| v.split_once(',')) && let (Ok(l), Ok(o)) = (l.parse::<i32>(), o.parse::<i16>()) {
                match at {
                    crate::game::visual::SpriteAt::Physical(p) => {
                        self.physicals[p].sprite_layer = l;
                        if let Some(s) = self.physicals[p].sprite.as_mut() { s.2 = o }
                    }
                    crate::game::visual::SpriteAt::Renderer(i) => {
                        self.renderers[i].layer = l;
                        self.renderers[i].sprite.2 = o;
                    }
                }
            }
        }
        // gapC r9: Collider2D.enabled (`ItemCollider`) and every item script's Behaviour.enabled (`ItemScript enabled=`) that
        // item_fields leaves out, as recorded (the field only, as a recorded clone holds it: no OnEnable/OnDisable).
        for r in s.of("ItemCollider") {
            let on = r.req("enabled")? == "1";
            if self.on_clone(r.id) && self.collider_enabled(r.id) != on {
                self.with_component_instance(r.id, |g, inst, o| {
                    let c = inst.objects[o].components.iter().position(|&k| k == r.id).unwrap();
                    g.collider_set_enabled(inst, (o, c), on)
                });
            }
        }
        for r in s.of("ItemScript") {
            let (Some(class), Some(on)) = (r.get("class"), r.get("enabled").map(|v| v == "1")) else { continue };
            if self.item_enabled(class, r.id).is_some_and(|ours| ours != on) {
                self.set_item_enabled(class, r.id, on);
            }
        }
        // Each ParticleMachineBehaviour's systems (`ItemParticleMachine`): isEmitting as recorded, the last Play or Stop.
        for r in s.of("ItemParticleMachine") {
            for (k, e) in r.req("emitting")?.split(',').enumerate() {
                if let Some(i) = self.particle_machine_ps(r.id, k) {
                    self.ps_calls.push((i, if e == "1" { crate::game::PsCall::Play } else { crate::game::PsCall::Stop(self.time.timeAsDouble) }));
                }
            }
        }
        // The item's LineRenderers (`ItemLine`): enabled (EnergyVesselLightningBehaviour.Init turns it off for its wait).
        for r in s.of("ItemLine") {
            if let Some(l) = self.lines.iter().position(|x| x.id == r.id) {
                self.lines[l].enabled = r.req("enabled")? == "1";
            }
        }
        // gapA r5: an AudioSource the game destroyed (AntennaReceiverBehaviour.OnDestroy's Destroy of its child): its object's
        // parent is in the Clone records but no record lists the source, so ours goes too.
        let comps: std::collections::HashSet<i32> = s.of("Clone").filter_map(|r| r.get("comps")).flat_map(super::list)
            .filter_map(|c| c.rsplit_once(':').and_then(|x| x.1.parse().ok())).collect();
        let gone: Vec<i32> = self.audio.sources.iter().filter(|(id, a)| !comps.contains(id) && a.ancestors.get(1).is_some_and(|t| comps.contains(t))).map(|(id, _)| *id).collect();
        for id in gone {
            self.audio.remove(id);
        }
        // gapC r12: a loaded PhysicalBehaviour's Start-made audioSource (`audioSource=@x#id`) that ours lacks: made with the
        // recorded id, so PlayClipOnce plays it (CounterBehaviour.SendSignal, CardiopulmonaryBypass's sounds).
        let recorded_sources: std::collections::HashSet<i32> = s.of("ItemAudioSource").map(|r| r.id).collect();
        for r in s.of("PhysicalBehaviour") {
            if let (Some(p), Some(id)) = (self.physicals.iter().position(|p| p.id == r.id), r.get("audioSource").and_then(super::reference))
                && !self.audio.sources.contains_key(&id) {
                // gapC r13: ours' own Start-made one under another id (no record holds it) goes: PlayClipOnce plays the
                // recorded one (Valve/ArchelixCaster/Defibrillator: ours played a source the compare never looks at).
                let old = self.physicals[p].ids.audioSource;
                if self.audio.sources.contains_key(&old) {
                    if recorded_sources.contains(&old) {
                        continue;
                    }
                    self.audio.remove(old);
                }
                self.physical_add_audio_source(p, id);
            }
        }
        // gapB r27: the same for its slidingAudioSource (`slidingAudioSource=@x#id`, PlaySliderSound): a limb's came with
        // ours' own ids or none, so HandleSlidingSounds found no source and did nothing.
        // Two passes: one limb's recorded id can be another limb's id in ours, so every old one goes before any is made.
        let moves: Vec<(usize, i32)> = s.of("PhysicalBehaviour")
            .filter_map(|r| Some((self.physicals.iter().position(|p| p.id == r.id)?, r.get("slidingAudioSource").and_then(super::reference)?)))
            .filter(|&(p, id)| self.physicals[p].ids.slidingAudioSource != id)
            .collect();
        for &(p, _) in &moves {
            let old = self.physicals[p].ids.slidingAudioSource;
            if self.audio.sources.contains_key(&old) && !recorded_sources.contains(&old) {
                self.audio.remove(old);
            }
        }
        for (p, id) in moves {
            if self.audio.sources.contains_key(&id) {
                self.audio.remove(id);
            }
            let ancestors = self.physical_ancestors(p);
            let clip = super::sliding_loop_clip(self, self.physicals[p].Properties)?;
            self.physical_add_sliding_audio_source(p, id, ancestors);
            self.audio.set_clip(id, clip);
        }

        // The item's AudioSources (`ItemAudioSource`): playing or not, at the recorded time.
        for r in s.of("ItemAudioSource") {
            if !self.audio.sources.contains_key(&r.id) {
                continue;
            }
            // gapC r7: a recorded `clip=null` is the source's clip (AudioSource.clip = null stops it): the game's Play() then plays
            // nothing (fngc8: the recorder's gcpre clear stays on for the later cases).
            if r.get("clip") == Some("null") {
                self.audio.set_clip(r.id, None);
            }
            // gapC r19: `dry=` (recordings since r19): the source's m_dryGroup made (a Play / PlayOneShot since it was created).
            match r.get("dry") {
                Some("1") => { self.audio.dry_groups.insert(r.id); }
                Some(_) => { self.audio.dry_groups.remove(&r.id); }
                None => {}
            }
            // gapB r25: `pitch=` (recordings since r25): the source's pitch (ExplosionSoundBehviour.Awake's " esp=" compare).
            if let Some(p) = r.get("pitch") {
                self.audio.get_mut(r.id).pitch = super::f(p);
            }
            let playing = r.req("playing")? == "1";
            if playing != self.audio.is_playing(r.id) {
                if playing {
                    // gapC r12: a playing AudioSource is enabled (Unity stops a disabled one; the record has no enabled): a
                    // PhysicalBehaviour's audioSource playing an impact one-shot from the step before the case (Start leaves it off).
                    self.audio.set_enabled(r.id, true);
                    // The playing clip (`clip=@name#id`) when the source holds none or another one: a script's AudioClip
                    // field of that name (a sentry turret's PlaySound set its clip in an Update before the snapshot).
                    let name = r.get("clip").and_then(|c| c.strip_prefix('@')).and_then(|c| c.rsplit_once('#')).map(|(n, _)| n.to_string());
                    if let Some(n) = name && self.audio.get(r.id).clip.as_ref().is_none_or(|c| c.clip.name.replace(' ', "_") != n) {
                        let found = self.audio.clip_fields.values().flatten().flatten().find(|c| c.clip.name.replace(' ', "_") == n).cloned();
                        if found.is_some() {
                            self.audio.set_clip(r.id, found);
                        }
                    }
                    // `clip=null` while playing: only one-shots play (a sentry's TurretGun Shoot's PlayOneShot), so a
                    // one-shot stands in for them. ponytail: any script clip, the record names none; only isPlaying is compared.
                    if r.get("clip") == Some("null") && self.audio.get(r.id).clip.is_none() {
                        // `shot=@name#id,<age>`: the recorder's last one-shot of this source (fnlive), by name at its age.
                        if let Some((n, age)) = r.get("shot").and_then(|v| v.strip_prefix('@')?.rsplit_once('#')).and_then(|(n, rest)| Some((n.to_string(), super::f(rest.split_once(',')?.1)))) {
                            let found = self.audio.clip_cache.values().chain(self.audio.clip_fields.values().flatten().flatten()).find(|c| c.clip.name.replace(' ', "_") == n).cloned();
                            if found.is_some() {
                                self.audio.play_one_shot_at(r.id, found.as_ref(), 1.0, age as f64);
                                continue;
                            }
                        }
                        // gapC r12: else any loaded clip (a physical's impact one-shot: its clips are not script fields).
                        let any = self.audio.clip_fields.values().flatten().flatten().chain(self.audio.clip_cache.values()).find(|c| !self.audio.not_loaded.contains(&c.clip.name.replace(' ', "_"))).cloned();
                        self.audio.play_one_shot(r.id, any.as_ref(), 1.0);
                        continue;
                    }
                    self.audio.play(r.id);
                    self.audio.set_time(r.id, super::f(r.req("time")?));
                } else {
                    self.audio.stop(r.id);
                }
            }
        }
        // The item scripts' Start wrote to their PhysicalBehaviours (LagboxBehaviour.Start: ActivationPropagationDelay, a link,
        // = DelayModifier as it was then; a limb item's LimbBehaviour.Start: Temperature): the recorded fields again, and each
        // link the record holds as its type.
        for r in s.of("PhysicalBehaviour") {
            if let Some(p) = self.physicals.iter().position(|p| p.id == r.id) {
                let (fields, links) = self.physicals[p].fields_and_links();
                super::load_fields(fields, r)?;
                for (name, field) in links {
                    if let Some(v) = r.get(name) {
                        let ours = field.show();
                        if field.parse(v).is_err() {
                            field.parse(&ours).unwrap();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The SpriteRenderer of instance id `id`: a physical's own, or a `renderers` entry.
    pub(crate) fn item_sprite(&self, id: i32) -> Option<crate::game::visual::SpriteAt> {
        use crate::game::visual::SpriteAt;
        self.physicals.iter().position(|p| p.sprite_id == id && p.sprite.is_some()).map(SpriteAt::Physical)
            .or_else(|| self.renderers.iter().position(|r| r.id == id).map(SpriteAt::Renderer))
    }

    /// The fields and readable links of the `ItemScript` records that differ from the rewrite's structs.
    /// The `embedded_ps` index of ParticleMachineBehaviour `id`'s system `k` (Smoke, Fire, Electricity).
    fn particle_machine_ps(&self, id: i32, k: usize) -> Option<usize> {
        self.embedded_ps.iter().position(|e| e.owner == id && matches!(e.drive, crate::game::EmbeddedDrive::ParticleMachine { k: x, .. } if x == k))
    }
    /// gapA r2: LiquidContainerController.particles and particlePositions, AntennaReceiverBehaviour.audioInformation, in the
    /// recorder's text.
    fn compound_fields(&self, class: &str, id: i32) -> Vec<(&'static str, String)> {
        let h = |v: f32| format!("{:08x}", v.to_bits());
        match class {
            "LiquidContainerController" => {
                let Some(&(_, crate::game::Behaviour::LiquidContainer(i))) = self.behaviour_of.get(&id) else { return Vec::new() };
                let x = &self.liquid_containers[i];
                let particles = x.particles.iter().map(|p| format!("Particle(Position={},{} Velocity={},{})", h(p.Position[0]), h(p.Position[1]), h(p.Velocity[0]), h(p.Velocity[1]))).collect::<Vec<_>>().join(";");
                let positions = x.particlePositions.as_ref().map_or("null".into(), |v| format!("[{}]", v.iter().map(|p| p.map(h).join(",")).collect::<Vec<_>>().join(";")));
                vec![("particles", format!("[{particles}]")), ("particlePositions", positions)]
            }
            // gapC r16: the sentry's t alone (the controller's other members are compared nowhere: the whole value would differ).
            "LaserTurretBehaviour" => self.laser_turrets.iter().find(|x| x.id == id).map(|x| vec![("sentryController.t", h(x.sentry.t))]).unwrap_or_default(),
            "AntennaReceiverBehaviour" => self.antenna_receivers.iter().find(|x| x.id == id)
                .map(|x| vec![("audioInformation", format!("AudioInformation(OldFactor={} CurrentFactor={})", h(x.OldFactor), h(x.CurrentFactor)))]).unwrap_or_default(),
            _ => Vec::new(),
        }
    }
    /// gapC r9: component `id` is on a clone (a recording's physicals without one have no Collider2D state).
    fn on_clone(&self, id: i32) -> bool {
        self.unsynced.iter().chain(&self.retained).any(|i| i.objects.iter().any(|x| x.components.contains(&id)))
    }

    /// gapC r9: ours' Behaviour.enabled of item script `id` of `class`, for the ones item_fields does not already load and
    /// compare as a field: a TextMeshPro's, a translated behaviour's flag, else !script_disabled over the prefab's m_Enabled.
    /// None: ours keeps none for it.
    // checker limit: a prefab-disabled script without a translated flag is None (its own flag, if any, is not read); Light2D has no
    // rewrite model. Add a class's own flag here when a function that enables one needs it.
    pub(super) fn item_enabled(&mut self, class: &str, id: i32) -> Option<bool> {
        if class == "Light2D" || super::fnitems::item_fields(self, class, id).is_some_and(|(f, l)| f.iter().chain(&l).any(|(n, _)| *n == "enabled")) {
            return None;
        }
        if class.starts_with("TextMeshPro") {
            return self.world_texts.iter().find(|t| t.id == id).map(|t| t.enabled);
        }
        // gapB r14: a no-delay Destroy of a script on an instance outside the synced lists (a fn-check clone) waits in
        // destroy_disable_pending for its SetEnabled(false); Unity reads enabled = false at once.
        if self.destroy_disabled.contains(&id) || self.destroy_disable_pending.iter().any(|&(i, c)| c && i == id) {
            return Some(false);
        }
        if let Some(&(_, crate::game::Behaviour::Translated(t))) = self.behaviour_of.get(&id) {
            return Some(self.tr_enabled(t));
        }
        let prefab = self.unsynced.iter().chain(&self.retained).find_map(|inst| {
            inst.objects.iter().enumerate().find_map(|(o, x)| match &inst.prefab.objects[o].components[x.components.iter().position(|&k| k == id)?].1 {
                Component::Script { enabled, .. } => Some(*enabled),
                _ => None,
            })
        })?;
        if !prefab {
            return None;
        }
        let on = !self.script_disabled(id);
        // A class keeping its own flag whose arena lacks this script (set_own_enabled's unreachable!): ours keeps none. The probe
        // writes the value it reads, so it changes nothing.
        let probe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { self.set_own_enabled(class, id, on); }));
        probe.is_ok().then_some(on)
    }

    /// gapC r9: Behaviour.enabled = on for item_enabled's scripts (the flag only).
    pub(crate) fn set_item_enabled(&mut self, class: &str, id: i32, on: bool) {
        if class.starts_with("TextMeshPro") {
            if let Some(i) = self.world_texts.iter().position(|t| t.id == id) {
                self.text_enabled_set(i, on);
            }
        } else if let Some(&(_, crate::game::Behaviour::Translated(t))) = self.behaviour_of.get(&id) {
            self.tr_set_enabled(t, on);
        } else {
            self.set_own_enabled(class, id, on);
        }
    }

    pub(super) fn compare_item_scripts(&mut self, s: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        let pairs = std::mem::take(&mut self.ref_pairs);
        for (class, r) in script_records(s) {
            let disabled = self.destroy_disabled.contains(&r.id) || self.destroy_disable_pending.iter().any(|&(i, c)| c && i == r.id);
            let Some((fields, links)) = super::fnitems::item_fields(self, class, r.id) else { continue };
            for (link, (name, field)) in fields.into_iter().map(|f| (false, f)).chain(links.into_iter().map(|f| (true, f))) {
                let Some(theirs) = r.get(name) else { continue };
                // Object.Destroy's SetEnabled(false) on the spot (destroy_disable_behaviours) reads as enabled = false.
                let shown = field.show();
                let ours = if name == "enabled" && disabled { "0".to_string() } else { shown.clone() };
                // An object reference (`@name#id`) a link keeps by value: the recorded id must be the one the load paired with
                // that value, and no other recorded id may pair with it (two objects of one name are not the same object).
                // An id the load never paired compares as before (a link of another type skips it below).
                if link && let Some((_, id)) = ref_name_id(theirs) && let Some(p) = pairs.get(&id) {
                    let ours = ref_name(&ours);
                    if *p == ours && !pairs.iter().any(|(&k, v)| k != id && *v == ours) {
                        continue;
                    }
                    out.push(format!("{class} #{} {name}: game {theirs} ours {ours} (load paired #{id} with {p})", r.id));
                    continue;
                }
                let text = script_text(theirs);
                if text == ours {
                    continue;
                }
                // A link whose recorded value is not of its type is not the same thing.
                if link && field.parse(&text).is_err() {
                    continue;
                }
                field.parse(&shown).unwrap();
                out.push(format!("{class} #{} {name}: game {theirs} ours {ours}", r.id));
            }
        }
        self.ref_pairs = pairs;
        // gapA r2: the compound fields (as loaded above).
        for (class, r) in script_records(s) {
            for (name, ours) in self.compound_fields(class, r.id) {
                let theirs = match name.split_once('.') { Some((f, "t")) => r.get(f).and_then(sentry_t), _ => r.get(name) };
                if let Some(theirs) = theirs && theirs != ours {
                    out.push(format!("{class} #{} {name}: game {theirs} ours {ours}", r.id));
                }
            }
        }
        // gapA r14: a FirearmBehaviour's/MachineGunBehaviour's BallisticsEmitter value members (null or not), LaserBehaviour.finalHit.
        for (class, r) in script_records(s) {
            match class {
                "FirearmBehaviour" | "MachineGunBehaviour" => {
                    let Some(v) = r.get("BallisticsEmitter") else { continue };
                    let e = if class == "FirearmBehaviour" { self.firearms.iter_mut().find(|x| x.id == r.id).map(|x| &mut x.emitter) } else { self.machine_guns.iter_mut().find(|x| x.id == r.id).map(|x| &mut x.emitter) };
                    let Some(e) = e else { continue };
                    match (v == "null", e.as_mut()) {
                        (true, None) => {}
                        (true, Some(_)) | (false, None) => out.push(format!("{class} #{} BallisticsEmitter: game {} ours {}", r.id, if v == "null" { "null" } else { "set" }, if e.is_some() { "set" } else { "null" })),
                        (false, Some(e)) => {
                            for (k, t) in emitter_members(v) {
                                if let Some(f) = emitter_field(e, k) && f.show() != t {
                                    out.push(format!("{class} #{} BallisticsEmitter.{k}: game {t} ours {}", r.id, f.show()));
                                }
                            }
                        }
                    }
                }
                "LaserBehaviour" => {
                    let Some(v) = r.get("finalHit") else { continue };
                    let Some(x) = self.lasers.iter().find(|x| x.id == r.id) else { continue };
                    let ours = x.final_hit.map_or((false, 0, [0.0; 2], 0), |(h, d, p, pt)| (h, p.map_or(0, |p| self.physicals[p].id), pt, d));
                    let theirs = laser_hit(v);
                    if theirs.0 != ours.0 || theirs.1 != ours.1 || theirs.2.map(f32::to_bits) != ours.2.map(f32::to_bits) || theirs.3 != ours.3 {
                        out.push(format!("LaserBehaviour #{} finalHit: game {theirs:?} ours {ours:?}", r.id));
                    }
                }
                _ => {}
            }
        }
        // batch2 r10: references compare_item_scripts' item_fields leave out: GuideTemperatureBehaviour.phys (its PhysicalBehaviour)
        // and InfiniteLiquidSourceBehaviour.c (its BloodContainer: the rewrite's `infinite_liquids` owner, in record order).
        for (class, r) in script_records(s) {
            let ours = match class {
                "GuideTemperatureBehaviour" => self.tr.guide_temperature_behaviour.iter().find(|x| x.this_id == r.id).map(|x| x.phys.map_or(0, |p| self.physicals[p].id)),
                "InfiniteLiquidSourceBehaviour" => s.of("ItemScript").filter(|x| x.get("class") == Some(class)).position(|x| x.id == r.id)
                    .and_then(|k| self.infinite_liquids.get(k)).map(|x| self.container_id(x.1)),
                _ => continue,
            };
            let field = if class == "InfiniteLiquidSourceBehaviour" { "c" } else { "phys" };
            let (Some(ours), Some(v)) = (ours, r.get(field)) else { continue };
            let theirs = ref_name_id(v).map_or(0, |x| x.1);
            if theirs != ours {
                out.push(format!("{class} #{} {field}: game {v} ours #{ours}", r.id));
            }
        }
        // ReturnToPoolAfterTime.pool: the pool the recorded reference names.
        for (class, r) in script_records(s) {
            let Some(v) = r.get("pool").filter(|_| class == "ReturnToPoolAfterTime") else { continue };
            let Some(x) = self.return_to_pools.iter().find(|x| x.id == r.id) else { continue };
            let theirs = if v == "null" { Ok(None) } else { self.pool_ref_of(s, v).map(Some) };
            if theirs.as_ref() != Ok(&x.pool) {
                out.push(format!("ReturnToPoolAfterTime #{} pool: game {v} ({theirs:?}) ours {:?}", r.id, x.pool));
            }
        }
        for r in s.of("ReactorA5DisplayBehaviour") {
            let Some(d) = self.reactor_a5_displays.iter().find(|x| x.id == r.id) else { continue };
            for (name, ours) in [("updateDisplayClock", &d.updateDisplayClock), ("alertBeepClock", &d.alertBeepClock)] {
                if let Some(t) = r.get(name) && super::distributor(t) != *ours {
                    out.push(format!("ReactorA5DisplayBehaviour #{} {name}: game {t} ours {ours:?}", r.id));
                }
            }
        }
        for r in s.of("ItemSprite") {
            let Some(at) = self.item_sprite(r.id) else { continue };
            let ours = crate::game::Field::show(&self.sprite_color(at));
            if r.get("color") != Some(ours.as_str()) {
                out.push(format!("SpriteRenderer {} #{} color: game {} ours {ours}", r.name, r.id, r.get("color").unwrap_or("")));
            }
            if (r.get("enabled") == Some("1")) != self.sprite_enabled(at) {
                out.push(format!("SpriteRenderer {} #{} enabled: game {}", r.name, r.id, r.get("enabled").unwrap_or("")));
            }
        }
        // gapC r9: Collider2D.enabled and the item scripts' Behaviour.enabled item_fields leaves out (loaded above).
        for r in s.of("ItemCollider") {
            if self.on_clone(r.id) && (r.get("enabled") == Some("1")) != self.collider_enabled(r.id) {
                out.push(format!("Collider2D {} #{} enabled: game {}", r.name, r.id, r.get("enabled").unwrap_or("")));
            }
        }
        for r in s.of("ItemScript") {
            let (Some(class), Some(theirs)) = (r.get("class"), r.get("enabled")) else { continue };
            if let Some(ours) = self.item_enabled(class, r.id) && (theirs == "1") != ours {
                out.push(format!("{class} {} #{} enabled: game {theirs} ours {}", r.name, r.id, ours as u8));
            }
        }
        for r in s.of("ItemParticleMachine") {
            let ours: Vec<&str> = (0..3).map(|k| if self.particle_machine_ps(r.id, k).is_some_and(|i| crate::particles::emitting(self, i)) { "1" } else { "0" }).collect();
            if r.get("emitting") != Some(ours.join(",").as_str()) {
                out.push(format!("ParticleMachineBehaviour {} #{} emitting: game {} ours {}", r.name, r.id, r.get("emitting").unwrap_or(""), ours.join(",")));
            }
        }
        for r in s.of("ItemLine") {
            if let Some(l) = self.lines.iter().find(|x| x.id == r.id) && (r.get("enabled") == Some("1")) != l.enabled {
                out.push(format!("LineRenderer {} #{} enabled: game {}", r.name, r.id, r.get("enabled").unwrap_or("")));
            }
        }
        for r in s.of("ItemAudioSource") {
            if self.audio.sources.contains_key(&r.id) && (r.get("playing") == Some("1")) != self.audio.is_playing(r.id) {
                let a = self.audio.get(r.id);
                out.push(format!("AudioSource {} #{} playing: game {} (ours: active {} enabled {} clip {:?} main {} one-shots {} paused {})", r.name, r.id, r.get("playing").unwrap_or(""), a.active, a.enabled, a.clip.as_ref().map(|c| c.clip.name.clone()), a.main.is_some(), a.one_shots.len(), a.paused));
            }
        }
        out
    }
}

/// gapC r16: the top-level `t=` member of a recorded AutomaticSentryController(...) (its buffer and targetStack hold no `t=`).
fn sentry_t(v: &str) -> Option<&str> {
    let inner = v.strip_prefix("AutomaticSentryController(")?.strip_suffix(')')?;
    let (mut depth, mut start) = (0i32, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ' ' if depth == 0 => {
                if let Some(t) = inner[start..i].strip_prefix("t=") {
                    return Some(t);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    inner[start..].strip_prefix("t=")
}

/// A recorded object reference `@name#id`: its name (underscores as spaces) and instance id.
/// gapA r14: the value members of a recorded BallisticsEmitter (`<Name>k__BackingField=v`, `lastCrackPlayTime=v`); a LayerMask
/// as its bits. emitter_field says which the rewrite keeps.
fn emitter_members(v: &str) -> Vec<(&str, &str)> {
    v.split(' ').filter_map(|t| {
        let (k, v) = t.split_once('=')?;
        let k = k.rsplit('(').next()?;
        let k = k.strip_prefix('<').and_then(|k| k.strip_suffix(">k__BackingField")).or((k == "lastCrackPlayTime").then_some(k))?;
        Some((k, v.strip_prefix("LayerMask(m_Mask=").unwrap_or(v).trim_end_matches(')')))
    }).collect()
}

/// gapA r14: ours' BallisticsEmitter member `k`.
fn emitter_field<'a>(e: &'a mut crate::game::ballistics::Emitter, k: &str) -> Option<&'a mut dyn crate::game::Field> {
    Some(match k {
        "MaxBallisticsIterations" => &mut e.MaxBallisticsIterations,
        "MaxTotalDistance" => &mut e.MaxTotalDistance,
        "ThicknessStepSize" => &mut e.ThicknessStepSize,
        "RicochetChanceMultiplier" => &mut e.RicochetChanceMultiplier,
        "MaxRange" => &mut e.MaxRange,
        "MaterialHardnessMultiplier" => &mut e.MaterialHardnessMultiplier,
        "ImpactForceMultiplier" => &mut e.ImpactForceMultiplier,
        "LayersToHit" => &mut e.LayersToHit,
        "WaterLayer" => &mut e.WaterLayer,
        "DoTraversal" => &mut e.DoTraversal,
        "TracerWidth" => &mut e.TracerWidth,
        "RenderTracersIfTraversal" => &mut e.RenderTracersIfTraversal,
        "BulletDropMultiplier" => &mut e.BulletDropMultiplier,
        "Charge" => &mut e.Charge,
        "BulletCrackSpeedThreshold" => &mut e.BulletCrackSpeedThreshold,
        "BulletCrackChance" => &mut e.BulletCrackChance,
        "BulletCrackCooldownTime" => &mut e.BulletCrackCooldownTime,
        "ExplosiveRound" => &mut e.ExplosiveRound,
        "ExplodeAtHitPoint" => &mut e.ExplodeAtHitPoint,
        "lastCrackPlayTime" => &mut e.lastCrackPlayTime,
        _ => return None,
    })
}

/// gapA r14: a recorded LaserHit: (hit has a value, physicalBehaviour's id or 0, point, rayDepth).
fn laser_hit(v: &str) -> (bool, i32, [f32; 2], usize) {
    let get = |k: &str| v.split(' ').find_map(|t| t.split_once('=').filter(|x| x.0 == k).map(|x| x.1.trim_end_matches(')')));
    let hit = !v.split(' ').next().unwrap_or("").ends_with("=null");
    let pid = get("physicalBehaviour").and_then(|p| p.rsplit_once('#')).and_then(|x| x.1.parse().ok()).unwrap_or(0);
    let point = get("point").map_or([0.0; 2], floats::<2>);
    (hit, pid, point, get("rayDepth").and_then(|d| d.parse().ok()).unwrap_or(0))
}

fn ref_name_id(v: &str) -> Option<(String, i32)> {
    let (n, id) = v.strip_prefix('@')?.rsplit_once('#')?;
    Some((ref_name(n), id.parse().ok()?))
}

/// An object name as the recorder and the rewrite both may write it (spaces as underscores).
fn ref_name(n: &str) -> String {
    n.replace('_', " ")
}

/// A recorded value as the rewrite's Field shows it: a string without its quotes (the recorder writes spaces as underscores).
fn script_text(v: &str) -> String {
    match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(t) => t.replace('_', " "),
        None => v.to_string(),
    }
}

/// A Clone record's local Transform.
pub(super) fn trs(r: &Record) -> Result<Trs, String> {
    let v3 = |k: &str| -> Result<[f32; 4], String> {
        let [x, y, z]: [f32; 3] = floats(r.req(k)?);
        Ok([x, y, z, 0.0])
    };
    Ok(Trs { t: v3("lpos")?, q: floats(r.req("lrot")?), s: v3("lscale")? })
}
