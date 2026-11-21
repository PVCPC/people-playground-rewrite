//! The script oracle: loads the game's own state from a tools/recorder PPG_DUMP file into `Game`,
//! runs the rewritten scripts for one segment of the player loop and compares every field with the game.
use super::circulation::{RefFloat, LIQUIDS};
use super::misc::FixedIntervalDistributor;
use super::person::{LimbPose, RagdollPose};
use super::*;
use crate::unity::transform::{Node as TransformNode, Trs};
use crate::b2::{math::Rot, world::BodyDef};
use crate::game_assets::{CurveCache, HumanRig};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::Path;

pub mod physics;
mod clone;
pub(crate) mod fncheck;
mod fnctor;
mod fnbigb;
mod fnitems;
mod fnmaped;
mod fnweap;
mod ref_cache;
pub use fncheck::fn_check;

/// Transform.eulerAngles.z: Internal_ToEulerRad normalizes (divides by the length), then
/// QuaternionToEuler (kOrderUnityDefault, CRT atan2f of the z row), then Internal_MakePositive
/// on the degrees. Matches all 1694 dumped transforms bit for bit.
pub fn euler_z(q: [f32; 4]) -> f32 {
    let length = (((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2]) + q[3] * q[3]).sqrt();
    let [x, y, z, w] = q.map(|v| v / length);
    let (x2, y2, z2, w2) = (x * x, y * y, z * z, w * w);
    let (xy, wz) = (x * y, w * z);
    let a = wz + xy;
    let sin = a + a;
    let cos = ((y2 - z2) - x2) + w2;
    let degrees = crate::unity::atan2f(sin, cos) * mathf::RAD2DEG;
    let negative_flip = -0.0001 * mathf::RAD2DEG;
    if degrees < negative_flip {
        degrees + 360.0
    } else if degrees > 360.0 + negative_flip {
        degrees - 360.0
    } else {
        degrees
    }
}

#[derive(Clone)]
pub struct Record {
    pub ty: String,
    pub name: String,
    pub id: i32,
    pub fields: Vec<(String, String)>,
}

impl Record {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    pub(crate) fn req(&self, key: &str) -> Result<&str, String> {
        self.get(key).ok_or_else(|| format!("{} {}: no field {key}", self.ty, self.name))
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub pt: String,
    pub snap: Vec<(String, String)>,
    pub records: Vec<Record>,
}

impl Snapshot {
    fn snap(&self, key: &str) -> &str {
        self.snap.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap()
    }
    pub fn of<'a>(&'a self, ty: &'a str) -> impl Iterator<Item = &'a Record> + 'a {
        self.records.iter().filter(move |r| r.ty == ty)
    }
}

/// Splits on spaces outside (), [] and {}.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0i32, 0, Vec::new());
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn key_value(item: &str) -> (String, String) {
    let (k, v) = item.split_once('=').unwrap_or((item, ""));
    let k = k.strip_prefix('<').and_then(|k| k.strip_suffix(">k__BackingField")).unwrap_or(k);
    (k.to_string(), v.to_string())
}

pub fn parse(text: &str) -> Vec<Snapshot> {
    Snapshots { lines: text.as_bytes().lines(), pending: None, header: Vec::new() }.collect()
}

/// A recording's snapshots read one at a time (a drag recording is too large to hold parsed); the other lines
/// (the recorder's header) go to `header`.
pub struct Snapshots<R> {
    lines: std::io::Lines<R>,
    pending: Option<Snapshot>,
    pub header: Vec<String>,
}

/// A recording, or its gzip copy `<path>.gz` when the plain file is absent (the stored recordings are gzipped).
pub fn open_recording(path: &Path) -> Result<Box<dyn std::io::Read>, String> {
    match std::fs::File::open(path) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => {
            let mut gz = path.as_os_str().to_owned();
            gz.push(".gz");
            let f = std::fs::File::open(&gz).map_err(|_| format!("{}: {e}", path.display()))?;
            Ok(Box::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::with_capacity(1 << 20, f))))
        }
    }
}

pub fn read_recording(path: &Path) -> Result<String, String> {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut open_recording(path)?, &mut text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text)
}

pub fn snapshots(path: &Path) -> Result<Snapshots<std::io::BufReader<Box<dyn std::io::Read>>>, String> {
    Ok(Snapshots { lines: std::io::BufReader::with_capacity(1 << 20, open_recording(path)?).lines(), pending: None, header: Vec::new() })
}

/// The recording's PhysicalBehaviour Mono object hashes (GetHashCode: a heap address the game does not define): one
/// made during the run (a crossbow's bolt) seeds Random with it in its Start, before any snapshot lists it.
#[derive(Default)]
pub struct RecordedHashes {
    by_id: HashMap<i32, i32>,
    /// The objects the first snapshot does not list, in the order the recording first lists them: a spawn whose
    /// instance id is not the game's (ours are allocated, not recorded) takes the next one.
    spawns: std::collections::VecDeque<i32>,
    taken: std::collections::HashSet<i32>,
}

impl RecordedHashes {
    /// The recorded instance id of the next PhysicalBehaviour a spawn makes, if the recording lists one no spawn took.
    pub fn next_spawn(&mut self) -> Option<i32> {
        while let Some(&id) = self.spawns.front() {
            if !self.taken.contains(&id) {
                return Some(id);
            }
            self.spawns.pop_front();
        }
        None
    }

    /// The hash of the PhysicalBehaviour made with instance id `id`, if the recording has it; else, with `fallback`, the next
    /// spawn's. The recorder lists no PhysicalBehaviour of an inactive object (a Lightbulb's Bulb_top): one takes no fallback, which
    /// would be the next spawn's id.
    pub fn take(&mut self, id: i32, fallback: bool) -> Option<i32> {
        if let Some(&h) = self.by_id.get(&id) {
            self.taken.insert(id);
            return Some(h);
        }
        while let Some(id) = fallback.then(|| self.spawns.pop_front()).flatten() {
            if self.taken.insert(id) {
                return self.by_id.get(&id).copied();
            }
        }
        None
    }
}

fn recorded_hashes(path: &Path) -> Result<RecordedHashes, String> {
    let mut out = HashMap::new();
    let mut solved = std::collections::HashSet::new();
    let (mut seen, mut spawns, mut snaps) = (std::collections::HashSet::new(), std::collections::VecDeque::new(), 0);
    for line in std::io::BufReader::with_capacity(1 << 20, open_recording(path)?).lines().map_while(Result::ok) {
        snaps += line.starts_with("snap ") as usize;
        let mut w = line.split(' ');
        if w.next() != Some("s") || w.nth(1) != Some("PhysicalBehaviour") {
            continue;
        }
        let Some(id) = w.nth(1).and_then(|i| i.parse::<i32>().ok()) else { continue };
        if seen.insert(id) && snaps > 1 {
            spawns.push_back(id);
        }
        if out.contains_key(&id) {
            continue;
        }
        let (mut hash, mut seed, mut int_seed) = (None, None, None);
        for k in w {
            if let Some(h) = k.strip_prefix("hash=") {
                hash = Some(h.parse::<i32>().map_err(|_| "PhysicalBehaviour hash")?);
            } else if let Some(s) = k.strip_prefix("seed=") {
                seed = Some(u32::from_str_radix(s, 16).map_err(|_| "PhysicalBehaviour seed")?);
            } else if let Some(s) = k.strip_prefix("intSeed=") {
                int_seed = Some(s.parse::<i32>().map_err(|_| "PhysicalBehaviour intSeed")?);
            }
        }
        // A recording older than the per-frame hash= has only Start's seed and intSeed for an object spawned during it:
        // its hash is the one 2048-aligned heap address whose InitState gives both (the address is the OS's; the
        // draws are the game's).
        if let Some(h) = hash.or_else(|| solved.insert(id).then(|| hash_of_seeds(seed?, int_seed?)).flatten()) {
            out.insert(id, h);
        }
    }
    Ok(RecordedHashes { by_id: out, spawns, taken: Default::default() })
}

/// The Mono object hash, (address >> 3) * 0x9e3779b1, of the 2048-aligned address whose Random.InitState makes
/// PhysicalBehaviour.Start draw `seed` (Range(-1500f, 1500f), as bits) and `int_seed` (Range(-1500, 1500)); None
/// unless exactly one does.
fn hash_of_seeds(seed: u32, int_seed: i32) -> Option<i32> {
    let mut found = None;
    for k in 0..1u32 << 24 {
        let hash = (k << 8).wrapping_mul(0x9e37_79b1) as i32;
        // Random.Range's arithmetic on the raw draws (Random::range would trace 2^24 draws under PPG_RANDOM_TRACE).
        let mut r = crate::unity::Random::new(hash);
        let t = (r.next() & 0x7f_ffff) as f32 * f32::from_bits(0x3400_0001);
        if ((1.0 - t) * 1500.0 + t * -1500.0).to_bits() == seed && -1500 + (r.next() % 3000) as i32 == int_seed {
            if found.is_some() {
                return None;
            }
            found = Some(hash);
        }
    }
    found
}

/// Each frame's time (Time.timeAsDouble at its US snapshot). TimeManager sets it before the frame's FixedUpdates, so
/// a WaitForSeconds started in one (TimeManager+0x60, the dynamic time) counts from it, not from the fixed time the
/// FixedUpdate snapshots carry.
fn frame_times(path: &Path) -> Result<HashMap<i32, f64>, String> {
    let mut out = HashMap::new();
    let mut frame = None;
    for line in std::io::BufReader::with_capacity(1 << 20, open_recording(path)?).lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("snap ") {
            frame = rest.strip_prefix("US frame=").and_then(|r| r.split(' ').next()?.parse::<i32>().ok());
        } else if let (Some(n), Some(rest)) = (frame, line.strip_prefix("s US Time ")) {
            if let Some(w) = rest.split(' ').find_map(|k| k.strip_prefix("timeAsDouble=")) {
                out.insert(n, f64::from_bits(u64::from_str_radix(w, 16).map_err(|e| e.to_string())?));
            }
            frame = None;
        }
    }
    Ok(out)
}

/// Only the recorder's header lines (collider and body layers come after the first snapshots).
pub fn header(path: &Path) -> Result<Vec<String>, String> {
    let mut s = snapshots(path)?;
    while let Some(Ok(line)) = s.lines.next() {
        if !line.starts_with("snap ") && !line.starts_with("s ") {
            s.header.push(line);
        }
    }
    Ok(s.header)
}

/// The recorder's `random_state=<s0> <s1> <s2> <s3>` header line: UnityEngine.Random.state just before the Person's Instantiate.
pub fn spawn_random_state(header: &[String]) -> Option<[u32; 4]> {
    let w: Vec<u32> = header.iter().find_map(|l| l.strip_prefix("random_state="))?.split(' ').map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    Some([w[0], w[1], w[2], w[3]])
}

/// The recorder's `native world=<b2World*> stack=<low>,<high>` line (the main thread's stack limits): the game process's
/// addresses b2Island::Report's stale impulse slot reads (`b2::World::native_world`). None: the defaults stay.
pub fn native_addresses(world: &mut crate::b2::world::World, header: &[String]) -> Result<(), String> {
    let Some(line) = header.iter().find_map(|l| l.strip_prefix("native world=")) else { return Ok(()) };
    let hex = |s: &str| u64::from_str_radix(s, 16).map_err(|e| format!("native line {line}: {e}"));
    let (address, stack) = line.split_once(" stack=").ok_or(format!("native line {line}"))?;
    let (low, high) = stack.split_once(',').ok_or(format!("native line {line}"))?;
    let (low, high) = (hex(low)?, hex(high)?);
    if low >> 32 != (high - 1) >> 32 {
        return Err(format!("main thread stack {low:x}..{high:x} straddles 4 GB: Solve's frame high half unknown"));
    }
    world.native_world = hex(address)?;
    world.native_stack_high = (low >> 32) as u32;
    Ok(())
}

impl<R: BufRead> Iterator for Snapshots<R> {
    type Item = Snapshot;
    fn next(&mut self) -> Option<Snapshot> {
        while let Some(Ok(line)) = self.lines.next() {
            if let Some(rest) = line.strip_prefix("snap ") {
                let parts = split_top(rest, ' ');
                // A PPG_ACT value may hold spaces (a Liquid ID).
                let mut pairs: Vec<(String, String)> = Vec::new();
                for p in &parts[1..] {
                    match pairs.last_mut() {
                        Some((k, v)) if k == "act" && !p.contains('=') => {
                            v.push(' ');
                            v.push_str(p);
                        }
                        _ => pairs.push(key_value(p)),
                    }
                }
                let snap = Snapshot { pt: parts[0].into(), snap: pairs, records: Vec::new() };
                if let Some(done) = self.pending.replace(snap) {
                    return Some(done);
                }
            } else if let Some(rest) = line.strip_prefix("s ") {
                let parts = split_top(rest, ' ');
                // fnlive: a record before any snapshot (an FM collision the dropped scenario snapshot held) belongs to none.
                let Some(snap) = self.pending.as_mut() else { continue };
                // The recorder tags every OnCollision*2D call "FM"; outside the physics pass only the empty
                // Collision2D SendMessage of PhysicalBehaviour.OnDestroy/OnDisable (physical_send_buffer_exits) gets
                // there, for an object destroyed in Update. Not compared.
                if parts[0] == "FM" && snap.pt != "FM" && parts[1] == "Collision2D" {
                    continue;
                }
                assert_eq!(parts[0], snap.pt);
                snap.records.push(Record {
                    ty: parts[1].into(),
                    name: parts[2].into(),
                    id: parts[3].parse().unwrap(),
                    fields: parts[4..].iter().map(|p| key_value(p)).collect(),
                });
            } else {
                self.header.push(line);
            }
        }
        self.pending.take()
    }
}

/// `@Name#id` -> id; `null` and `destroyed` (a destroyed object, which reads as null) -> None.
fn reference(text: &str) -> Option<i32> {
    if text == "null" || text == "destroyed" {
        return None;
    }
    Some(text.rsplit_once('#').unwrap().1.parse().unwrap())
}

fn list(text: &str) -> Vec<&str> {
    let inner = &text[1..text.len() - 1];
    if inner.is_empty() { Vec::new() } else { split_top(inner, ';') }
}

/// A PhysicsMaterial2D asset's friction from the exported project.
fn material_friction(name: &str) -> Result<f32, String> {
    material_value(name, "friction")
}

/// A PhysicsMaterial2D asset's float field from the exported project.
fn material_value(name: &str, key: &str) -> Result<f32, String> {
    // Records write names with underscores for spaces ("Car_metal" is the "Car metal" asset).
    let path = format!("unity-export/ExportedProject/Assets/PhysicsMaterial2D/{name}.physicsMaterial2D");
    let text = std::fs::read_to_string(&path)
        .or_else(|_| std::fs::read_to_string(format!("unity-export/ExportedProject/Assets/PhysicsMaterial2D/{}.physicsMaterial2D", name.replace('_', " "))))
        .map_err(|e| format!("{path}: {e}"))?;
    let v = text.lines().find_map(|l| l.trim().strip_prefix(&format!("{key}: "))).ok_or(format!("{path}: no {key}"))?;
    v.parse().map_err(|e| format!("{path}: {e}"))
}

fn f(text: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(text, 16).unwrap_or_else(|_| panic!("bad float {text}")))
}

/// The PlayAudioAtRandom records in Update order (`ambient_audio`'s): level1's WORLD emitter (a scene object, positive
/// id) before the map's own, as load_main and load_map list them; the recorder writes FindObjectsOfType's order.
fn ambient_records(s: &Snapshot) -> Vec<&Record> {
    let mut v: Vec<&Record> = s.of("PlayAudioAtRandom").collect();
    v.sort_by_key(|r| r.id < 0);
    v
}

fn words(text: &str) -> Vec<u32> {
    text.split(',').map(|w| u32::from_str_radix(w, 16).unwrap_or_else(|_| panic!("bad word {w} in {}", &text[..text.len().min(120)]))).collect()
}

fn floats<const N: usize>(text: &str) -> [f32; N] {
    let mut v = [0.0; N];
    v.parse(text).unwrap();
    v
}

/// The inner `Key=value` list of a `Type(Key=value ...)` dump.
fn inner_fields(text: &str) -> Vec<(String, String)> {
    let open = text.find('(').unwrap();
    split_top(&text[open + 1..text.len() - 1], ' ').into_iter().map(key_value).collect()
}

fn distributor(text: &str) -> FixedIntervalDistributor {
    let fields = inner_fields(text);
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).unwrap().1.clone();
    FixedIntervalDistributor {
        timeStepAccumulator: f(&get("timeStepAccumulator")),
        MaxCycles: get("MaxCycles").as_str().parse().unwrap(),
        InterpolationTime: f(&get("InterpolationTime")),
        IntervalSeconds: f(&get("IntervalSeconds")),
    }
}

/// Loads every dumped field a script declares; a declared field missing from the dump is an error.
/// A BloodContainer record's LiquidDistribution in dictionary order.
fn liquid_distribution(r: &Record) -> Result<Vec<(usize, RefFloat)>, String> {
    let mut out = Vec::new();
    for entry in list(r.req("LiquidDistribution")?) {
        let (key, value) = entry.split_once("):RefFloat(").ok_or("bad liquid entry")?;
        let class = key.split('(').next().unwrap();
        let id = LIQUIDS.iter().position(|(_, c)| *c == class).ok_or_else(|| format!("liquid class {class}"))?;
        let v = inner_fields(&format!("RefFloat({value}"));
        let get = |k: &str| f(&v.iter().find(|(n, _)| n == k).unwrap().1);
        out.push((id, RefFloat { Previous: get("Previous"), raw: get("raw") }));
    }
    Ok(out)
}

/// A BloodContainer record's SerialisableDistributions (`[SerialisableDistribution(LiquidID="<id, spaces as _>" Amount=<hex>);...]`).
fn serialisable_distributions(r: &Record) -> Result<Vec<(String, f32)>, String> {
    list(r.get("SerialisableDistributions").unwrap_or("[]")).into_iter().map(|e| {
        let v = inner_fields(e);
        let get = |k: &str| v.iter().find(|(n, _)| n == k).map(|x| x.1.clone()).ok_or(format!("SerialisableDistribution {k}: {e}"));
        Ok((get("LiquidID")?.trim_matches('"').replace('_', " "), f(&get("Amount")?)))
    }).collect()
}

/// Script fields that only exist in 1.28.45.
const FIELDS_45: &[&str] = &["IsAttachedToLauncher", "ChargeBoostModifier"];

fn load_fields(fields: Vec<(&'static str, &mut dyn Field)>, r: &Record) -> Result<(), String> {
    for (name, field) in fields {
        // Fields 1.28.45 added are absent from 1.28.41 recordings: they keep their defaults.
        if FIELDS_45.contains(&name) && r.get(name).is_none() {
            continue;
        }
        field.parse(r.req(name)?).map_err(|e| format!("{} {} {name}: {e}", r.ty, r.name))?;
    }
    Ok(())
}

/// Indices of the records of one type, by instance id.
/// GlobalShrapnelEmitter.Awake with the Main scene's values.
fn main_shrapnel_emitter() -> super::ballistics::ShrapnelEmitter {
    use super::ballistics::{Cartridge, Emitter, ShrapnelEmitter};
    let cartridge = Cartridge { Damage: 15.0, Recoil: 0.5, ImpactForce: 0.2, StartSpeed: 10.0, PenetrationRandomAngleMultiplier: 0.1 };
    let mut emitter = Emitter::new(cartridge, 10752, 16);
    emitter.RenderTracersIfTraversal = false;
    emitter.DoTraversal = true;
    emitter.MaxBallisticsIterations = 16;
    emitter.RicochetChanceMultiplier = 4.0;
    emitter.MaxTotalDistance = 30.0;
    ShrapnelEmitter { emitter, instance: true }
}

/// The FancyBloodSplatController records the loader makes splats of: not an item clone's (an ItemScript record) nor the
/// map clone's (a map Clone record's component: the map link makes those, then load_splat takes the record).
fn loaded_splats(s: &Snapshot) -> Vec<&Record> {
    let item: HashSet<i32> = s.of("ItemScript").map(|r| r.id).collect();
    let map = map_clone_splats(s);
    s.of("FancyBloodSplatController").filter(|r| !item.contains(&r.id) && !map.contains(&r.id)).collect()
}

/// The FancyBloodSplatController ids the map's Clone records list (`comps=[...;FancyBloodSplatController:<id>;...]`).
fn map_clone_splats(s: &Snapshot) -> HashSet<i32> {
    s.of("Clone").filter(|r| r.get("root") == Some("Map")).filter_map(|r| r.get("comps")).flat_map(|c| c.trim_matches(|x| x == '[' || x == ']').split(';'))
        .filter_map(|c| c.strip_prefix("FancyBloodSplatController:")?.parse().ok()).collect()
}

/// A FancyBloodSplatController record over `sp`: its fields, particles, local rotation and collision mask.
fn load_splat(sp: &mut FancyBloodSplat, r: &Record) -> Result<(), String> {
    load_fields(sp.fields(), r)?;
    // From dump16 on: the HashSet's slots, each "VelocityX,VelocityY,PositionX,PositionY,Life" or "-" (free),
    // then "free=" and its free list; before, the particles' type names.
    let text = r.req("particles")?;
    sp.particles = Default::default();
    match text.split_once("free=") {
        Some((slots, free)) => {
            for item in list(slots) {
                sp.particles.slots.push((item != "-").then(|| {
                    let [VelocityX, VelocityY, PositionX, PositionY, Life] = floats::<5>(item);
                    super::misc::SplatParticle { VelocityX, VelocityY, PositionX, PositionY, Life }
                }));
            }
            sp.particles.free = free.split(',').filter(|i| !i.is_empty()).rev().map(|i| i.parse().unwrap()).collect();
        }
        None if list(text).is_empty() => {}
        None => return Err("splat particles without their slots".into()),
    }
    // A bleeding particle's splat (that particle maybe destroyed since: the splat lives on unparented), a pooled
    // BloodPuff's Extra decal emitter (wired to its puff with the pools), or a loose one (an effect's extra decals,
    // such as a Gorse ball's Alien_Blood_Explosion: no parent wiring, it splats where its particles land).
    if !r.req("Decal")?.starts_with("@Sparse_blood#") {
        return Err(format!("splat {}: only Sparse blood splats are loaded", r.id));
    }
    sp.parent_gone = r.get("initialParent") == Some("destroyed");
    // The local rotation under the bleeding particle until the bleeding particle attaches it.
    if let Some(q) = r.get("tlrot") {
        sp.rotation = floats(q);
    }
    let mask = r.req("CollisionLayers")?;
    sp.CollisionLayers = mask.trim_start_matches("LayerMask(m_Mask=").trim_end_matches(')').parse().map_err(|_| format!("bad mask {mask}"))?;
    Ok(())
}

/// gapB r18: a PhysicalProperties this snapshot has no record of (an unstarted person's PhysicalBehaviours, not yet in
/// PhysicalObjectsInWorld, which the recorder dumped them from): the one already loaded under that name, else the
/// game's asset of that name (a reference `@Name#id`, spaces as `_`).
fn properties_by_name(g: &mut Game, reference: &str) -> Result<usize, String> {
    let name = reference.strip_prefix('@').and_then(|r| r.rsplit_once('#')).map(|(n, _)| n).ok_or_else(|| format!("unknown PhysicalProperties {reference}"))?;
    if let Some(i) = g.properties.iter().position(|p| p.name.replace(' ', "_") == name) {
        return Ok(i);
    }
    if g.assets.is_none() {
        let game = crate::game_assets::find_game(None).map_err(|e| format!("unknown PhysicalProperties {reference} (no game data: {e})"))?;
        g.assets = Some(Box::new(crate::assets::db::AssetDb::open(&game)?));
    }
    let db = g.assets.as_mut().unwrap();
    for a in db.find_scripts("PhysicalProperties")? {
        if db.mono(a)?.is_some_and(|(_, mb, _)| mb.name.replace(' ', "_") == name) {
            let p = super::link::read_physical_properties(db, a)?;
            g.properties.push(p);
            return Ok(g.properties.len() - 1);
        }
    }
    Err(format!("unknown PhysicalProperties {reference}"))
}

/// gapB r27: the SlidingLoop clip of properties `i` (a recording's PhysicalProperties hold no clips): the game's asset of
/// that name's, kept on the properties.
fn sliding_loop_clip(g: &mut Game, i: usize) -> Result<Option<crate::game::audio::ClipRef>, String> {
    if g.properties[i].clips.SlidingLoop.is_none() {
        if g.assets.is_none() {
            let game = crate::game_assets::find_game(None).map_err(|e| format!("SlidingLoop (no game data: {e})"))?;
            g.assets = Some(Box::new(crate::assets::db::AssetDb::open(&game)?));
        }
        let name = g.properties[i].name.clone();
        let db = g.assets.as_mut().unwrap();
        for a in db.find_scripts("PhysicalProperties")? {
            if db.mono(a)?.is_some_and(|(_, mb, _)| mb.name == name) {
                g.properties[i].clips.SlidingLoop = super::link::read_physical_properties(db, a)?.clips.SlidingLoop;
                break;
            }
        }
    }
    Ok(g.properties[i].clips.SlidingLoop.clone())
}

fn ids(s: &Snapshot, ty: &str) -> HashMap<i32, usize> {
    s.of(ty).enumerate().map(|(i, r)| (r.id, i)).collect()
}

fn body_type(t: i32) -> BodyType {
    match t {
        0 => BodyType::Dynamic,
        1 => BodyType::Kinematic,
        _ => BodyType::Static,
    }
}

impl Game {
    /// Builds the game from one snapshot. Limb-owned components keep the LimbBehaviour order.
    pub fn from_snapshot(s: &Snapshot, rig: &HumanRig) -> Result<Game, String> {
        Self::from_snapshot_header(s, rig, &[])
    }

    /// from_snapshot with the recording's header: the colliders' layers are in before the stab item's replayed Starts
    /// (a detector's Start raycasts).
    pub fn from_snapshot_header(s: &Snapshot, rig: &HumanRig, header: &[String]) -> Result<Game, String> {
        let mut g = Game::default();
        (g.velocityIterations, g.positionIterations) = (32, 32);
        g.load_time(s);
        // Main's DiskLogger: the Main scene always holds one (not in the snapshot), its Awake ran before any snapshot.
        g.disk_logger = Some(super::disk_logger::DiskLogger::loaded());
        // Main's Global.BulletCrackClips: 9 clips (scene.rs load_main reads them); a crack's PickRandom draws on the count.
        g.bullet_crack_clips = 9;
        g.world = World::new(V::new(-0.0, -9.81));
        // The whole b2 world when the dump has it (the map's colliders, contacts and listener records too).
        let mut restored = match s.of("b2World").next() {
            Some(_) => Some(physics::restore(s, V::new(-0.0, -9.81))?),
            None => None,
        };
        if let Some(r) = restored.as_mut() {
            g.world = std::mem::replace(&mut r.world, World::new(V::ZERO));
            g.objects = r.objects.clone();
            // IgnoreCollisionStackController's stack under those pairs: PersonBehaviour.SetCollisionIgnores asks for each
            // limb pair both ways round, and ColliderPair matches either order.
            g.ignore_stack = g.world.ignored.iter().map(|&k| (k, 2)).collect();
        }

        let props = ids(s, "PhysicalProperties");
        for r in s.of("PhysicalProperties") {
            let mut p = PhysicalProperties { name: r.name.clone(), ..Default::default() };
            p.Softness = f(r.req("Softness")?);
            p.Flammability = f(r.req("Flammability")?);
            p.MagneticAttractionIntensity = f(r.req("MagneticAttractionIntensity")?);
            p.Conducting = r.req("Conducting")? == "1";
            p.Burnrate = f(r.req("Burnrate")?);
            p.BurningTemperatureThreshold = f(r.req("BurningTemperatureThreshold")?);
            p.HeatTransferSpeedMultiplier = f(r.req("HeatTransferSpeedMultiplier")?);
            p.Buoyancy = f(r.req("Buoyancy")?);
            p.ImpactIntensityMutliplier = f(r.req("ImpactIntensityMutliplier")?);
            p.HitVolumeMultiplier = f(r.req("HitVolumeMultiplier")?);
            p.SparksOnSlide = r.req("SparksOnSlide")? == "1";
            p.SoftImpact = list(r.req("SoftImpact")?).len();
            p.HardImpact = list(r.req("HardImpact")?).len();
            p.SizzleSounds = match r.req("SizzleSounds")? {
                "null" => None,
                t => Some(list(t).len()),
            };
            p.Sharp = r.req("Sharp")? == "1";
            p.IgnoreStabResistance = r.req("IgnoreStabResistance")? == "1";
            p.SharpForceThresholdMultiplier = f(r.req("SharpForceThresholdMultiplier")?);
            // SharpAxis(Axis=x,y LowerLimit=f UpperLimit=f LooseUpperLimit=b LooseLowerLimit=b)
            for item in list(r.req("SharpAxes")?) {
                let v = inner_fields(item);
                let get = |k: &str| -> Result<&str, String> { v.iter().find(|(n, _)| n == k).map(|(_, x)| x.as_str()).ok_or(format!("SharpAxis.{k}")) };
                p.SharpAxes.push(super::physical::SharpAxis {
                    Axis: floats(get("Axis")?),
                    LowerLimit: f(get("LowerLimit")?),
                    UpperLimit: f(get("UpperLimit")?),
                    LooseUpperLimit: get("LooseUpperLimit")? == "1",
                    LooseLowerLimit: get("LooseLowerLimit")? == "1",
                });
            }
            p.LodgeStrengthMultiplier = f(r.req("LodgeStrengthMultiplier")?);
            p.StabSound = match r.req("StabSound")? {
                "null" => None,
                t => Some(list(t).len()),
            };
            p.friction = match (r.get("friction"), r.req("PhysicMaterial")?) {
                (Some(bits), _) => Some(f(bits)),
                (None, "null") => None,
                // Older recordings: the material asset's serialized value.
                (None, m) => Some(material_friction(&m[1..m.find('#').unwrap()])?),
            };
            p.bounciness = match r.req("PhysicMaterial")? {
                "null" => 0.0,
                m => material_value(&m[1..m.find('#').unwrap()], "bounciness")?,
            };
            // Older recordings lack these (only the ballistics read them).
            if let Some(bits) = r.get("Brittleness") {
                p.Brittleness = f(bits);
                p.BulletSpeedAbsorptionPower = f(r.req("BulletSpeedAbsorptionPower")?);
                p.ShotImpact = match r.req("ShotImpact")? {
                    "null" => None,
                    m => Some((None, &m[1..m.find('#').unwrap()] == "BloodPuff_poolable")),
                };
            }
            g.properties.push(p);
        }

        let limb_ids = ids(s, "LimbBehaviour");
        let limb_names: Vec<String> = s.of("LimbBehaviour").map(|r| r.name.clone()).collect();
        let limb_by_name = |n: &str| limb_names.iter().position(|l| l == n);
        // A Person snapshotted between its Instantiate and its Starts (fnlive1_car/fnlive1_wPinTool: spawned that frame):
        // LimbBehaviour.Start has not run, so no limb has the GForceMeasureBehaviour its GetOrAddComponent adds, and no
        // record or clone lists one.
        let limbs_unstarted = !limb_names.is_empty()
            && s.of("GForceMeasureBehaviour").next().is_none()
            && !s.of("Clone").filter_map(|c| c.get("comps")).any(|c| c.contains("GForceMeasureBehaviour:"));
        // Every limb-owned component must list in LimbBehaviour order.
        for ty in ["CirculationBehaviour", "ConnectedNodeBehaviour", "SkinMaterialHandler", "GForceMeasureBehaviour"] {
            if ty == "GForceMeasureBehaviour" && limbs_unstarted {
                continue;
            }
            let names: Vec<&String> = s.of(ty).map(|r| &r.name).collect();
            if names.iter().map(|n| n.as_str()).ne(limb_names.iter().map(|n| n.as_str())) {
                return Err(format!("{ty} order differs from LimbBehaviour"));
            }
        }
        // An inactive limb (a disintegrated one) has no b2 body, so no Rigidbody2D/b2Body record: those list the
        // active limbs, in LimbBehaviour order.
        let mut limb_rb: Vec<Option<usize>> = Vec::new();
        {
            let mut rb_names = s.of("Rigidbody2D").map(|r| r.name.clone()).peekable();
            for n in &limb_names {
                let k = limb_rb.iter().flatten().count();
                limb_rb.push(rb_names.next_if(|r| r == n).map(|_| k));
            }
            if rb_names.next().is_some() || s.of("b2Body").map(|r| &r.name).ne(s.of("Rigidbody2D").map(|r| &r.name)) {
                return Err("Rigidbody2D order differs from LimbBehaviour".into());
            }
        }
        let by_limb_component = |ty: &str| -> HashMap<i32, usize> { ids(s, ty) };
        let circ_ids = by_limb_component("CirculationBehaviour");
        let node_ids = by_limb_component("ConnectedNodeBehaviour");
        let skin_ids = by_limb_component("SkinMaterialHandler");

        // Bodies: the human's from b2Body, the map's as static placeholders.
        let rbs: Vec<&Record> = s.of("Rigidbody2D").collect();
        let mut limb_bodies = Vec::new();
        for (rb, raw) in rbs.iter().zip(s.of("b2Body")) {
            let id = match &restored {
                Some(r) => r.bodies[&format!("{}#{}", rb.name, rb.id)],
                None => {
                    let id = g.world.create_body(&BodyDef { kind: body_type(rb.req("bodyType")?.parse().unwrap()), ..Default::default() });
                    load_body(g.world.body_mut(id), raw)?;
                    id
                }
            };
            limb_bodies.push(id);
        }
        // A body-less limb's Rigidbody2D: an inactive body of its own (SetActive(false) removed the b2 body).
        let limb_bodies: Vec<crate::b2::world::BodyId> = limb_rb.iter().map(|k| match k {
            Some(k) => limb_bodies[*k],
            None => {
                let id = g.world.create_body(&BodyDef::default());
                g.world.deactivate_body(id);
                id
            }
        }).collect();

        let chain: Vec<&Record> = s.of("TransformChain").collect();
        for r in &chain {
            let v3 = |k: &str| -> Result<[f32; 4], String> {
                let [x, y, z]: [f32; 3] = floats(r.req(k)?);
                Ok([x, y, z, 0.0])
            };
            let trs = Trs { t: v3("lpos")?, q: floats(r.req("lrot")?), s: v3("lscale")? };
            g.transform_nodes.push(TransformNode { id: r.id, trs, parent: None });
        }
        let node_of = |g: &Game, text: &str| text.contains('#').then(|| reference(text)).flatten().and_then(|id| g.transform_nodes.iter().position(|n| n.id == id));
        for (i, r) in chain.iter().enumerate() {
            g.transform_nodes[i].parent = node_of(&g, r.req("parent")?);
        }
        let phys_ids = ids(s, "PhysicalBehaviour");
        for r in s.of("PhysicalBehaviour") {
            let mut p = Physical { name: r.name.clone(), id: r.id, ..Default::default() };
            load_fields(p.fields(), r)?;
            p.isStatic = r.get("isStatic") == Some("1");
            p.HoldingPositions = r.get("HoldingPositions").map_or(Vec::new(), |t| list(t).iter().map(|x| floats(x)).collect());
            p.hash_code = r.get("hash").map_or(0, |h| h.parse().unwrap());
            // CreateOutlineObject's GameObject (its Transform the id after): Utils.EnumerateChildrenAndSelf walks it.
            p.ids.selectionOutlineObject = r.get("selectionOutlineObject").and_then(reference).unwrap_or(0);
            let pr = r.req("Properties")?;
            p.Properties = match props.get(&reference(pr).unwrap()) {
                Some(&i) => i,
                None => properties_by_name(&mut g, pr)?,
            };
            p.LocalColliderGridPoints = match r.req("LocalColliderGridPoints")? {
                "null" => None,
                t => Some(list(t).into_iter().map(floats::<2>).collect()),
            };
            p.has_fire_map = r.req("LocalFirePropagationMap")? != "null";
            p.has_fire_system = r.get("FireParticleSystem").is_some_and(|v| v != "null");
            p.MaximumStabVictims = r.req("MaximumStabVictims")?.parse().map_err(|e| format!("{} MaximumStabVictims: {e}", r.name))?;
            p.LooseEndSize = f(r.req("LooseEndSize")?);
            p.StabReleaseSpeedModifier = f(r.req("StabReleaseSpeedModifier")?);
            p.StabWoundSizeMultiplier = f(r.req("StabWoundSizeMultiplier")?);
            // The laser flags (links: MaterialAwareRaycast and GetLaserEndPoint read them; a probe may toggle them).
            if let (Some(re), Some(ab)) = (r.get("ReflectsLasers"), r.get("AbsorbsLasers")) {
                (p.ReflectsLasers, p.AbsorbsLasers) = (re == "1", ab == "1");
            }
            // gapA r18: ContinuousActivationTracker `{<channel>:DeltaInt(Value=.. PreviousValue=.. PreviousValueFixed=..);..}` (a held
            // continuous use: ProjectileLauncher's automaticTimer reads it); before it, every tracker stayed at Awake's 0.
            if let Some(t) = r.get("ContinuousActivationTracker") {
                for e in t.trim_matches(|c| c == '{' || c == '}').split(';').filter(|e| !e.is_empty()) {
                    let Some((ch, d)) = e.split_once(':') else { continue };
                    let Some(slot) = ch.parse::<usize>().ok().and_then(|c| p.ContinuousActivationTracker.get_mut(c)) else { continue };
                    let inner = d.trim_start_matches("DeltaInt(").trim_end_matches(')');
                    for kv in inner.split(' ') {
                        let Some((k, v)) = kv.split_once('=') else { continue };
                        let Ok(v) = v.parse::<i32>() else { continue };
                        match k { "Value" => slot.Value = v, "PreviousValue" => slot.PreviousValue = v, "PreviousValueFixed" => slot.PreviousValueFixed = v, _ => {} }
                    }
                }
            }
            if let Some(t) = r.get("InitialBounds") {
                let v = bounds(t);
                p.InitialBounds = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
            }
            p.OverrideImpactSounds = list(r.req("OverrideImpactSounds")?).len();
            p.audioSource = r.req("audioSource")? != "null";
            // From dump17 on: the SpriteRenderer's local AABB (center, extents).
            if let Some(b) = r.get("sbounds") {
                // Simple mode: the renderer's local AABB is the sprite's with the center mirrored per flip (link::renderer_bounds).
                let d: Vec<&str> = r.req("sdraw")?.split(',').collect();
                if d[0] != "0" {
                    return Err(format!("{}: a sliced or tiled sprite ({})", r.name, r.req("sdraw")?));
                }
                let [mut cx, mut cy, cz, ex, ey, ez]: [f32; 6] = floats(b);
                if d[1] == "1" {
                    cx = -cx;
                }
                if d[2] == "1" {
                    cy = -cy;
                }
                p.sprite_bounds = Some(([cx, cy, cz], [ex, ey, ez]));
            }
            // PhysicalBehaviour.colliders, in component order (the recorder dumps the field).
            p.colliders = list(r.req("colliders")?).into_iter().map(|c| c.rsplit_once('#').unwrap().1.parse().unwrap()).collect();
            p.all_colliders = p.colliders.clone();
            if let Some(rs) = &restored {
                // Same-named objects (the Truck's "Head" beside the person's) part by the clone's component list.
                let own = s.of("Clone").filter_map(|c| c.get("comps")).find(|c| c.contains(&format!("PhysicalBehaviour:{};", r.id)) || c.contains(&format!("PhysicalBehaviour:{}]", r.id)));
                let mine = |id: i32| own.map_or(true, |c| c.contains(&format!(":{id};")) || c.contains(&format!(":{id}]")));
                let collider = |k: &String| k.split(':').next()?.rsplit_once('#').filter(|(o, _)| *o == r.name).map(|(_, id)| id.parse::<i32>().unwrap()).filter(|&id| mine(id));
                p.all_colliders = rs.fixtures.keys().filter_map(collider).fold(Vec::new(), |mut v, c| {
                    if !v.contains(&c) {
                        v.push(c);
                    }
                    v
                });
                // Instance ids fall in creation (= component) order.
                p.all_colliders.sort_by(|a, b| b.cmp(a));
            }
            // A limb, or a PPG_STAB item (ItemRigidbody2D/ItemB2Body records: a dynamic body of its own).
            // A limb's own PhysicalBehaviour (an item's same-named part, the Truck's "Head", is not one).
            let limb = s.of("LimbBehaviour").position(|l| l.get("PhysicalBehaviour").and_then(reference) == Some(r.id));
            let owner: Option<(&Record, Option<&Record>, crate::b2::world::BodyId)> = match limb {
                Some(l) => match limb_rb[l] {
                    Some(k) => Some((rbs[k], s.of("b2Body").nth(k), limb_bodies[l])),
                    None => None,
                },
                None => match physics::item_own_body(s, r) {
                    Some(rb) => {
                        let body = restored.as_ref().and_then(|rs| rs.bodies.get(&format!("{}#{}", r.name, rb.id))).ok_or("an item without the b2 world")?;
                        Some((rb, s.of("ItemB2Body").find(|x| x.id == rb.id), *body))
                    }
                    None => None,
                },
            };
            match owner {
                Some((rb, raw, body)) => {
                    p.rigidbody = Rigidbody {
                        body,
                        bodyType: rb.req("bodyType")?.parse().unwrap(),
                        collisionDetectionMode: rb.req("cdm")?.parse().unwrap(),
                        interpolation: rb.req("interpolation")?.parse().unwrap(),
                        movement: raw.and_then(|r| r.get("move")).map_or(MovementState::default(), |w| movement_state(&words(w))),
                        detached: None,
                        component: rb.id,
                        simulated: true,
                        detached_props: [0.0; 3], constraints: 0,
                    };
                    // Older recordings lack the scene's flag: a non-static body out of the list was just (re)added.
                    let changed = raw.and_then(|r| r.get("hierarchyChanged"));
                    g.rigidbody_hierarchy_changed |= changed.map_or(!p.rigidbody.movement.listed && p.rigidbody.bodyType != 2, |v| v == "1");
                    p.parent = rb.get("parent").and_then(|t| node_of(&g, t));
                    p.transform = Pose {
                        position: floats(rb.req("tpos")?),
                        rotation: floats(rb.req("trot")?),
                        localScale: floats(rb.req("scale")?),
                        local: rb.get("tlpos").zip(rb.get("tlrot")).map(|(p, q)| (floats(p), floats(q))),
                    };
                }
                // A body-less (inactive) limb: its pose from its Clone record (local TRS under the parent Clone's Transform),
                // the world pose as CalculateGlobalPosition/Rotation from the parent up.
                None if limb.is_some() => {
                    p.rigidbody = Rigidbody { body: limb_bodies[limb.unwrap()], bodyType: 0, collisionDetectionMode: 0, interpolation: 0, movement: MovementState::default(), detached: None, component: 0, simulated: false, detached_props: [0.0; 3], constraints: 0 };
                    let comp = |c: &&Record| c.get("comps").is_some_and(|x| list(x).iter().any(|k| *k == format!("PhysicalBehaviour:{}", r.id)));
                    if let Some(c) = s.of("Clone").find(comp) {
                        let parent = s.of("Clone").find(|x| x.get("root") == c.get("root") && x.get("index") == c.get("parent"))
                            .and_then(|x| list(x.get("comps")?).into_iter().find_map(|k| k.strip_prefix("Transform:").and_then(|t| t.parse::<i32>().ok())));
                        p.parent = parent.and_then(|id| g.transform_nodes.iter().position(|n| n.id == id));
                        let (lpos, lrot, lscale): ([f32; 3], [f32; 4], [f32; 3]) = (floats(c.req("lpos")?), floats(c.req("lrot")?), floats(c.req("lscale")?));
                        let (mut pos, mut rot) = ([lpos[0], lpos[1], lpos[2], 0.0], lrot);
                        let mut n = p.parent;
                        while let Some(i) = n {
                            let t = g.transform_nodes[i].trs;
                            (pos, rot) = (crate::unity::transform::global_position(&t, pos), crate::unity::transform::global_rotation(&t, rot));
                            n = g.transform_nodes[i].parent;
                        }
                        p.transform = Pose { position: [pos[0], pos[1], pos[2]], rotation: rot, localScale: lscale, local: Some((lpos, lrot)) };
                    }
                }
                None => {
                    // Same-named map objects (the Reactor's two Heaviest objects) part by the clone's Rigidbody2D; else the name's.
                    let own_rb = s.of("Clone").filter_map(|c| c.get("comps")).find(|c| c.contains(&format!("PhysicalBehaviour:{};", r.id)) || c.contains(&format!("PhysicalBehaviour:{}]", r.id)))
                        .and_then(|c| list(c).into_iter().find_map(|x| x.strip_prefix("Rigidbody2D:").map(String::from)));
                    let by_clone = restored.as_ref().zip(own_rb).and_then(|(rs, id)| rs.bodies.get_key_value(&format!("{}#{id}", r.name)));
                    let body = match by_clone.or_else(|| restored.as_ref().and_then(|rs| rs.bodies.iter().find(|(n, _)| n.rsplit_once('#').map(|(o, _)| o) == Some(r.name.as_str())))) {
                        Some((_, &b)) => b,
                        None => g.world.create_body(&BodyDef::default()),
                    };
                    // RigidbodyType2D from the recorded b2 body (a map's hinged panel is Dynamic, its lights Kinematic).
                    let bodyType = match g.world.body(body).kind {
                        BodyType::Dynamic => 0,
                        BodyType::Kinematic => 1,
                        BodyType::Static => 2,
                    };
                    p.rigidbody = Rigidbody { body, bodyType, collisionDetectionMode: 0, interpolation: 0, movement: MovementState::default(), detached: None, component: 0, simulated: true, detached_props: [0.0; 3], constraints: 0 };
                    // A non-Dynamic body's b2 mass is 0, but Rigidbody2D.mass keeps what RecalculateMassBasedOnSize stored
                    // (InitialMass, clamped by SetMass 180c23790 to [0.0001, 1e6]: the map floor's 1.29e6 is 1e6, which
                    // CenterOfGravity.GetPosition weighs). No Rigidbody2D record holds it.
                    if bodyType != 0 && !p.IsWeightless {
                        let v = p.InitialMass;
                        g.world.body_mut(body).rigidbody.mass = if 0.0001 <= v && v <= 1_000_000.0 { v } else if 0.0001 <= v { 1_000_000.0 } else { 0.0001 };
                    }
                    // From dump16 on the record carries the object's Transform.
                    if let Some(pos) = r.get("tpos") {
                        p.parent = r.get("parent").and_then(|t| node_of(&g, t));
                        p.transform = Pose {
                            position: floats(pos),
                            rotation: floats(r.req("trot")?),
                            localScale: floats(r.req("scale")?),
                            local: Some((floats(r.req("tlpos")?), floats(r.req("tlrot")?))),
                        };
                    }
                }
            }
            g.physicals.push(p);
        }
        // An item body whose Transform parents another (an Axe's handle, its head's parent) is in the chain as a node:
        // that node is the body's own Transform, so the head's pose follows the handle's write-back.
        for (i, r) in chain.iter().enumerate() {
            let parent = g.transform_nodes[i].parent;
            if let Some(p) = g.physicals.iter().position(|p| p.name == r.name && p.parent == parent && p.transform.local.is_some() && s.of("ItemRigidbody2D").any(|x| x.name.replace(' ', "_") == r.name)) {
                g.node_bodies.insert(i, p);
            }
        }
        // LocalFirePropagationMap records: the maps' serialized state and Start's Softness and RandomisedIndices, with
        // PhysicalBehaviour.fireEmissionInterval (only the fire map's emission uses it).
        for r in s.of("LocalFirePropagationMap") {
            let phys: i32 = r.req("phys")?.parse().map_err(|e| format!("{} phys: {e}", r.name))?;
            let p = g.physicals.iter().position(|q| q.id == phys).ok_or(format!("{}: no PhysicalBehaviour {phys}", r.name))?;
            let mut m = super::fire::FireMap::new(p);
            m.collider = reference(r.req("Collider")?);
            m.GridStep = f(r.req("GridStep")?);
            m.FireIntensityDampening = f(r.req("FireIntensityDampening")?);
            m.FireSpreadSpeed = f(r.req("FireSpreadSpeed")?);
            m.Randomisation = f(r.req("Randomisation")?);
            if let Some(v) = r.get("MergeDistanceMultiplier") {
                m.MergeDistanceMultiplier = f(v);
            }
            m.CalculationOffset = floats(r.req("CalculationOffset")?);
            for point in r.req("Points")?.split(';').filter(|x| !x.is_empty() && *x != "null") {
                let w: Vec<&str> = point.split(',').collect();
                // A null NeighbourIndices: the points CalculatePoints made.
                let neighbours = match w[4] {
                    "null" => {
                        m.neighbours_null = true;
                        Vec::new()
                    }
                    n => n.split(':').filter(|x| !x.is_empty()).map(|i| i.parse().unwrap()).collect(),
                };
                m.Points.push(super::fire::BurnPoint { LocalPoint: [f(w[0]), f(w[1])], FireIntensity: f(w[2]), Softness: f(w[3]), NeighbourIndices: neighbours });
            }
            m.RandomisedIndices = match r.req("RandomisedIndices")? {
                "null" => Vec::new(),
                t => t.split(',').filter(|x| !x.is_empty()).map(|i| i.parse().unwrap()).collect(),
            };
            let pr = s.of("PhysicalBehaviour").find(|x| x.id == phys).unwrap();
            m.fireEmissionInterval = distributor(pr.req("fireEmissionInterval")?);
            g.physicals[p].fire_map = Some(g.fire_maps.len());
            g.fire_maps.push(m);
        }
        if let Some(p) = g.physicals.iter().find(|p| p.has_fire_map && p.fire_map.is_none()) {
            return Err(format!("{}: a LocalFirePropagationMap without its record (recorded before the recorder wrote them)", p.name));
        }
        // collisionBuffer (1.28.41 on): the first collisionBufferCount ContactPoint2Ds, with their instance ids. A
        // 1.28.32 recording has the old onCollisionStayBuffer of entered colliders instead, which the new buffer does
        // not derive from: it starts empty there.
        for (i, r) in s.of("PhysicalBehaviour").enumerate() {
            let (Some(items), Some(n)) = (r.get("collisionBuffer"), r.get("collisionBufferCount")) else { continue };
            // A count under 0 (only a fuzzed case: Math.Min(GetContacts, 16) never is) reads as none, as the loops over it do.
            let n: usize = n.parse::<i64>().map_err(|_| "collisionBufferCount")?.max(0) as usize;
            for item in list(items).into_iter().take(n) {
                let fields = inner_fields(item);
                let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).ok_or(format!("ContactPoint2D.{k}"));
                let v2 = |k: &str| get(k).map(|v| { let [x, y]: [f32; 2] = floats(&v); crate::b2::math::Vec2::new(x, y) });
                let id = |k: &str| get(k).and_then(|v| v.as_str().parse::<i32>().map_err(|_| format!("ContactPoint2D.{k}")));
                let rb = |k: &str| id(k).map(|v| Some(v).filter(|&v| v != 0));
                g.physicals[i].collisionBuffer.push(crate::b2::contacts2d::ContactPoint2D {
                    point: v2("m_Point")?,
                    normal: v2("m_Normal")?,
                    relative_velocity: v2("m_RelativeVelocity")?,
                    separation: f(&get("m_Separation")?),
                    normal_impulse: f(&get("m_NormalImpulse")?),
                    tangent_impulse: f(&get("m_TangentImpulse")?),
                    collider: id("m_Collider")?,
                    other_collider: id("m_OtherCollider")?,
                    rigidbody: rb("m_Rigidbody")?,
                    other_rigidbody: rb("m_OtherRigidbody")?,
                    enabled: id("m_Enabled")? != 0,
                });
            }
        }

        // Hinges, from the HingeJoint2D, b2RevoluteJoint and the rig's anchors.
        let hinge_names: Vec<String> = s.of("HingeJoint2D").map(|r| r.name.clone()).collect();
        for (r, raw) in s.of("HingeJoint2D").zip(s.of("b2RevoluteJoint")) {
            let l = limb_by_name(&r.name).ok_or("hinge without limb")?;
            // connectedBody: a limb's body, else (a pin: PinTool.CreatePin on one body or onto a map body) null (the scene's
            // ground body) or the map physical whose Rigidbody2D it names. Every HingeJoint2D record keeps its `hinges` slot
            // (JointBreakLimits and LimbBehaviour.Joint index them by record order).
            let connected_rb = reference(r.req("connected")?);
            let connected_body = match connected_rb {
                None => None,
                Some(id) => match rb_owner(s, id) {
                    Ok(owner) => Some(limb_bodies[limb_ids[&owner]]),
                    // A map body (no Rigidbody2D record: the restored b2 body by its `name#id`).
                    Err(_) => Some(*restored.as_ref().and_then(|rs| rs.bodies.get(r.get("connected").unwrap_or("").trim_start_matches('@'))).ok_or(format!("hinge #{}: connected rigidbody #{id} without limb or b2 body", r.id))?),
                },
            };
            let motor: Vec<&str> = r.req("motor")?.split(',').collect();
            let limits: Vec<&str> = r.req("limits")?.split(',').collect();
            let mut h = Hinge {
                joint: None,
                useMotor: motor[0] == "1",
                motor: [f(motor[1]), f(motor[2])],
                useLimits: limits[0] == "1",
                limits: [f(limits[1]), f(limits[2])],
                breakForce: f(r.req("breakForce")?),
                breakTorque: f(r.req("breakTorque")?),
                connected: connected_body,
                // A recording from before the anchors were dumped never reads them.
                anchor: r.get("anchor").map_or([0.0; 2], floats),
                connectedAnchor: r.get("connectedAnchor").map_or([0.0; 2], floats),
                enabled: r.req("enabled")? == "1",
                destroyed: false,
                destroy_at: None,
            };
            let j = raw.req("joint")?;
            if let Some(rs) = &restored {
                h.joint = rs.joints.get(&format!("{}#{}", r.name, r.id)).copied();
            } else if j != "null" {
                let w = words(j);
                let fl = |i: usize| f32::from_bits(w[i]);
                let def = crate::b2::revolute::RevoluteDef {
                    local_anchor_a: V::new(fl(0), fl(1)),
                    local_anchor_b: V::new(fl(2), fl(3)),
                    enable_motor: w[8] & 0xff != 0,
                    max_motor_torque: fl(9),
                    motor_speed: fl(10),
                    enable_limit: w[11] & 0xff != 0,
                    reference_angle: fl(12),
                    lower_angle: fl(13),
                    upper_angle: fl(14),
                };
                let other = match connected_body { Some(b) => b, None => g.world.ground_body() };
                let id = g.world.create_joint(def, limb_bodies[l], other, false);
                let rev = g.world.joints[id].as_mut().unwrap().revolute_mut();
                rev.impulse = [fl(4), fl(5), fl(6)];
                rev.motor_impulse = fl(7);
                h.joint = Some(id);
            }
            let _ = rig;
            if h.breakForce != f32::INFINITY || h.breakTorque != f32::INFINITY {
                g.joint_break_limits.push(super::BreakJoint::Hinge(g.hinges.len()));
            }
            g.hinges.push(h);
        }

        for (i, r) in s.of("LimbBehaviour").enumerate() {
            // A snapshot is after Awake, which set Color white.
            let mut l = Limb { name: r.name.clone(), id: r.id, Color: [1.0; 4], ..Default::default() };
            load_fields(l.fields(), r)?;
            l.PhysicalBehaviour = phys_ids[&reference(r.req("PhysicalBehaviour")?).unwrap()];
            l.Person = 0;
            l.my_status = r.get("myStatus").and_then(reference);
            l.Joint = hinge_names.iter().position(|n| *n == r.name);
            // An inactive limb (Clone active=0): the recorder writes no HingeJoint2D record for it, yet its joint keeps its place in
            // m_JointBreakLimits (Joint2D::SetBreakForce, update_break_limits). Its own `hinges` slot after the records' ones, with
            // no b2 joint (deactivating destroyed it), so the break walk reads zero force from it. Its limits are not recorded.
            if l.Joint.is_none() && r.get("Joint").and_then(reference).is_some() {
                l.Joint = Some(g.hinges.len());
                g.hinges.push(Hinge { joint: None, useMotor: false, motor: [0.0; 2], useLimits: false, limits: [0.0; 2], breakForce: f32::INFINITY, breakTorque: f32::INFINITY, connected: None, anchor: [0.0; 2], connectedAnchor: [0.0; 2], enabled: true, destroyed: false, destroy_at: None });
            }
            l.GripBehaviour = None;
            let gore = reference(r.req("goreString")?);
            l.goreString = gore.is_some();
            // GoreStringBehaviour.enabled is not dumped; nothing in the game disables it.
            l.goreStringEnabled = true;
            l.goreStringJoint = gore.is_some_and(|id| s.of("GoreStringBehaviour").any(|g| g.id == id && g.req("Joint").is_ok_and(|j| j != "null")));
            if let Some(g) = gore.and_then(|id| s.of("GoreStringBehaviour").find(|g| g.id == id)) {
                let line = |k: &str| -> Result<[f32; 4], String> {
                    let fl = inner_fields(g.req(k)?);
                    let part = |n: &str| -> Result<[f32; 2], String> { Ok(floats(&fl.iter().find(|(m, _)| m == n).ok_or("LineSegment point")?.1)) };
                    let (a, b) = (part("A")?, part("B")?);
                    Ok([a[0], a[1], b[0], b[1]])
                };
                l.gore = super::limb::GoreString {
                    Other: reference(g.req("Other")?).map(|id| rb_owner(s, id).map(|lid| limb_ids[&lid])).transpose()?,
                    ColourSource: match reference(g.req("ColourSource")?) {
                        Some(id) => Some(s.of("CirculationBehaviour").find(|c| c.id == id).and_then(|c| limb_by_name(&c.name)).ok_or("ColourSource without a limb")?),
                        None => None,
                    },
                    JointDampening: f(g.req("JointDampening")?),
                    JointFreq: f(g.req("JointFreq")?),
                    JointBreakThreshold: f(g.req("JointBreakThreshold")?),
                    InvincibilityTime: f(g.req("InvincibilityTime")?),
                    OriginLine: line("OriginLine")?,
                    OtherLine: line("OtherLine")?,
                    HadJointWhenSerialised: g.req("HadJointWhenSerialised")? == "1",
                    TissueSnapClips: match g.req("TissueSnapClips")? {
                        "null" => 0,
                        t => list(t).len(),
                    },
                    AudioSource: g.req("AudioSource")? != "null",
                    id: g.id,
                    ..Default::default()
                };
            }
            if let Some(v) = r.get("VitalParts").filter(|v| *v != "null") {
                g.vital_parts.insert(i, list(v).into_iter().map(bounds).collect());
            }
            l.ConnectedLimbs = list(r.req("ConnectedLimbs")?).into_iter().map(|t| limb_ids[&reference(t).unwrap()]).collect();
            l.NearestLimbToBrain = reference(r.req("NearestLimbToBrain")?).map(|id| limb_ids[&id]);
            l.StrengthMultipliers = list(r.req("StrengthMultipliers")?).into_iter().map(f).collect();
            l.SpeciesIdentity = r.req("SpeciesIdentity")?.trim_matches('"').into();
            l.BloodLiquidType = r.req("BloodLiquidType")?.trim_matches('"').into();
            l.BloodDecal = r.req("BloodDecal")?.trim_start_matches('@').split('#').next().unwrap().into();
            l.KillShotParticles = r.get("KillShotParticles").is_some_and(|t| t != "null");
            if circ_ids.len() != 0 && i >= circ_ids.len() {
                return Err("limb without circulation".into());
            }
            g.limbs.push(l);
        }
        let grip_ids = ids(s, "GripBehaviour");
        for r in s.of("GripBehaviour") {
            let mut gr = Grip::default();
            load_fields(gr.fields(), r)?;
            gr.id = r.id;
            gr.PhysicalBehaviour = phys_ids[&reference(r.req("PhysicalBehaviour")?).unwrap()];
            gr.CurrentlyHolding = reference(r.req("CurrentlyHolding")?).map(|id| phys_ids[&id]);
            let l = limb_by_name(&r.name).ok_or("grip without limb")?;
            g.limbs[l].GripBehaviour = Some(g.grips.len());
            g.grips.push(gr);
        }
        let _ = grip_ids;
        // PoisonSpreadBehaviour records (fnbatch r4; class= the subclass): Game::poisons in record order (record_index's
        // position), an enabled one in the behaviour list (Update from here; its Start ran before the snapshot).
        for r in s.of("PoisonSpreadBehaviour") {
            use super::poison::{Poison, PoisonKind as K};
            let class = r.req("class")?;
            let kind = [K::Acid, K::Adrenaline, K::Coagulation, K::Death, K::Freeze, K::Knockout, K::Life, K::Pink, K::SlowZombie, K::UltraStrength, K::Zombie]
                .into_iter().find(|k| k.class() == class).ok_or_else(|| format!("poison class {class}"))?;
            let limb_id = reference(r.req("Limb")?);
            let p = Poison {
                id: r.id,
                kind,
                Spreads: r.req("Spreads")? == "1",
                Limb: g.limbs.iter().position(|l| Some(l.id) == limb_id).ok_or("poison without limb")?,
                LifeTime: f(r.req("LifeTime")?),
                HasSpread: r.req("HasSpread")? == "1",
                Fingerprint: r.get("Fingerprint").filter(|v| v.starts_with('[')).map(|v| list(v).into_iter().map(f).collect()),
                fingerprintReadPos: r.get("fingerprintReadPos").and_then(|v| v.parse().ok()).unwrap_or(0),
                FillBlood: r.get("FillBlood").map_or(true, |v| v == "1"),
            };
            g.poisons.push(p);
            if r.get("enabled") == Some("1") {
                let b = super::Behaviour::Poison(g.poisons.len() - 1);
                g.behaviour_of.insert(r.id, (0, b));
                g.behaviours.add(0, b);
            }
        }
        // The gore strings holding a SpringJoint2D: its b2DistanceJoint, breakForce, the SetBreakForce coroutine's wake and the tissue strings. DestroyJoint
        // cleared the tissue strings of a joint whose delayed Destroy is pending.
        for r in s.of("GoreStringBehaviour") {
            let Some(id) = reference(r.req("Joint")?) else { continue };
            let spring = s.of("SpringJoint2D").find(|j| j.id == id).ok_or("a gore string joint without its SpringJoint2D")?;
            let l = limb_by_name(&r.name).ok_or("gore string without limb")?;
            let other = g.limbs[l].gore.Other.ok_or("a gore string joint without Other")?;
            // A deactivated limb's spring (disintegrated, or either end inactive) has no b2 joint: Rigidbody2D::Cleanup's
            // joint2d_cleanup_any left gore.joint None and kept the rest.
            let joint = match (&restored, spring.req("joint")?) {
                (_, "null") => None,
                (Some(rs), _) => Some(rs.joints[&format!("{}#{}", spring.name, spring.id)]),
                (None, w) => Some(physics::restore_distance(&mut g.world, &words(w), limb_bodies[l], limb_bodies[other])),
            };
            let now = g.time.fixedTimeAsDouble.min(g.time.timeAsDouble);
            let gore = &mut g.limbs[l].gore;
            gore.joint = joint;
            gore.spring_id = id;
            gore.breakForce = f(spring.req("breakForce")?);
            gore.break_at = r.get("wake").map(|w| f64::from_bits(u64::from_str_radix(w, 16).unwrap()));
            gore.lineData = r.get("tissue").map(|t| {
                t.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()).map(|t| {
                    let v: Vec<f32> = t.split(',').map(|x| f(x.split_once(':').unwrap().1)).collect();
                    super::limb::TissueString { Pos1: v[0], Pos2: v[1], Strength: v[2], Thickness: v[3], ThicknessRatio: v[4] }
                }).collect()
            });
            if gore.lineData.is_none() {
                gore.destroy_at = Some(now);
            }
        }
        // SliderJoint2Ds live at the snapshot (an item's own, a Winch's base on its connector): their components, joined to the restored b2PrismaticJoint.
        let mut late_connected = Vec::new();
        // A per-case (NoMap) snapshot's item has no PhysicalBehaviour record: its physicals come with its link below.
        let mut late_attached = Vec::new();
        if let Some(rs) = &restored {
            for r in s.of("SliderJoint2D") {
                let physical = |t: &str| g.physicals.iter().position(|p| p.name == t.trim_start_matches('@').rsplit_once('#').map_or(t, |x| x.0)).ok_or(format!("slider {}: no physical {t}", r.name));
                // By the joint's Rigidbody2D when recorded (two map objects share a name).
                let own = r.get("own").and_then(|o| o.rsplit_once('#')?.1.parse::<i32>().ok());
                let attached = match own.and_then(|id| g.physicals.iter().position(|p| p.rigidbody.component == id)) {
                    Some(p) => p,
                    None if s.of("ItemRigidbody2D").any(|b| Some(b.id) == own) => {
                        late_attached.push((g.fixed_joints.len(), own.unwrap()));
                        0
                    }
                    None => physical(&r.name)?,
                };
                let connected = match r.req("connected")? {
                    "null" => None,
                    // By its Rigidbody2D id: a Tank barrel's slider holds the item's Hinge, a bare Rigidbody2D (no PhysicalBehaviour).
                    t => {
                        let id = t.rsplit_once('#').and_then(|x| x.1.parse::<i32>().ok());
                        match id.and_then(|id| g.physicals.iter().position(|p| p.rigidbody.component == id)) {
                            Some(p) => Some(BodyOwner::Physical(p)),
                            None => match id.and_then(|id| g.node_rigidbodies.iter().position(|n| n.rigidbody.component == id)) {
                                Some(k) => Some(BodyOwner::Node(k)),
                                // The PPG_STAB item's bare Rigidbody2D: its node body comes with the item's link below.
                                None if id.is_some() && g.physicals.iter().all(|p| p.name != t.trim_start_matches('@').rsplit_once('#').map_or(t, |x| x.0)) => {
                                    late_connected.push((g.fixed_joints.len(), id.unwrap()));
                                    None
                                }
                                None => Some(BodyOwner::Physical(physical(t)?)),
                            },
                        }
                    }
                };
                let bits = |k: &str| -> Result<Vec<f32>, String> { Ok(r.req(k)?.split(',').map(f).collect()) };
                let (motor, limits) = (bits("motor")?, bits("limits")?);
                let mut state = crate::b2::weld::FixedJoint2d::reset();
                state.anchor = { let a = bits("anchor")?; V::new(a[0], a[1]) };
                state.connected_anchor = { let a = bits("connectedAnchor")?; V::new(a[0], a[1]) };
                state.auto_configure_connected_anchor = r.req("auto")?.starts_with('1');
                state.anchor_configured = true;
                state.enable_collision = r.req("collide")? == "1";
                state.break_force = f(r.req("breakForce")?);
                state.break_torque = f(r.req("breakTorque")?);
                let slider = crate::b2::prismatic::SliderJoint2d {
                    angle: f(r.req("angle")?),
                    use_motor: r.req("motor")?.starts_with('1'),
                    motor_speed: motor[1],
                    max_motor_torque: motor[2],
                    use_limits: r.req("limits")?.starts_with('1'),
                    lower: limits[1],
                    upper: limits[2],
                    auto_configure_angle: r.req("auto")?.ends_with('1'),
                };
                let joint = rs.joints.get(&format!("{}#{}", r.name, r.id)).copied();
                // Adopted already: a script's SliderJoint reference (adopt_prefab_joint) finds this entry.
                g.adopted_joints.insert(r.id, g.fixed_joints.len());
                g.fixed_joints.push(super::simulate::FixedJoint { hinge: None, state, slider: Some(slider), spring: None, friction: None, distance: None, relative: None, joint, attached: BodyOwner::Physical(attached), connected, active: true, enabled: r.req("enabled")? == "1", destroyed: false });
            }
        }
        // The PPG_STAB item's own hinges, wheels, FixedJoint2Ds (a Car's) and DistanceJoint2Ds (a Light Machine Gun's): prefab joints, read by the break-limit walk. The map's own hinges and FixedJoint2Ds (the Sea map's panels) too.
        if let Some(rs) = &restored {
            let wire_joints = fnweap::wire_joint_ids(s);
            // A FirearmAttachmentPointBehaviour's HingeJoint2D (ConnectAttachment added it) is the point's `joint`.
            let point_joints: Vec<i32> = s.of("ItemScript").filter(|r| r.get("class") == Some("FirearmAttachmentPointBehaviour")).filter_map(|r| r.get("joint").and_then(reference)).collect();
            let sticky_joints: Vec<i32> = s.of("ItemScript").filter(|r| r.get("class") == Some("StickyGrenadeBehaviour")).filter_map(|r| r.get("joint").and_then(reference)).collect();
            for r in s.of("ItemHingeJoint2D").chain(s.of("ItemWheelJoint2D")).chain(s.of("ItemFixedJoint2D")).chain(s.of("ItemDistanceJoint2D")).chain(s.of("MapHingeJoint2D")).chain(s.of("MapFixedJoint2D")).chain(s.of("MapSpringJoint2D")).chain(s.of("MapDistanceJoint2D")) {
                // A wire's joint on a pasted item (its Item and Map records) is the wire's (fnweap::load_wires), not the prefab's.
                if wire_joints.contains(&r.id) || point_joints.contains(&r.id) {
                    continue;
                }
                // gapB r20: a stuck StickyGrenade's FixedJoint2D (CreateJoint's AddComponent, its `joint`): a `fixed_joints` entry
                // adopted by its id, attached to the grenade's body and connected to the recorded one (sticky_joint_links sets `joint`).
                if r.ty == "ItemFixedJoint2D" && sticky_joints.contains(&r.id) {
                    let p = g.physicals.iter().position(|p| p.name == r.name).ok_or(format!("sticky joint #{}: no physical {}", r.id, r.name))?;
                    let fv = |k: &str| -> Result<[f32; 2], String> { Ok(floats(r.req(k)?)) };
                    let mut state = crate::b2::weld::FixedJoint2d::reset();
                    let (an, cn) = (fv("anchor")?, fv("connectedAnchor")?);
                    state.anchor = V::new(an[0], an[1]);
                    state.connected_anchor = V::new(cn[0], cn[1]);
                    state.enable_collision = r.req("collide")? == "1";
                    state.break_force = f(r.req("breakForce")?);
                    state.break_torque = f(r.req("breakTorque")?);
                    state.anchor_configured = true;
                    let connected = reference(r.req("connected")?).and_then(|id| g.physicals.iter().position(|p| p.id == id || p.rigidbody.component == id)).map(BodyOwner::Physical);
                    let joint = rs.joints.get(&format!("{}#{}", r.name, r.id)).copied();
                    g.adopted_joints.insert(r.id, g.fixed_joints.len());
                    g.fixed_joints.push(super::simulate::FixedJoint { hinge: None, state, slider: None, spring: None, friction: None, distance: None, relative: None, joint, attached: BodyOwner::Physical(p), connected, active: true, enabled: r.req("enabled")? == "1", destroyed: false });
                    continue;
                }
                let joint = rs.joints.get(&format!("{}#{}", r.name, r.id)).copied();
                g.anchored_joints.push(super::AnchoredJoint { break_force: f(r.req("breakForce")?), break_torque: f(r.req("breakTorque")?), joint, hinge: None, component: r.id });
            }
            // A prefab joint on an INACTIVE object (a Car's Gastank after an explosion deactivated it) has no joint record,
            // yet stays in m_JointBreakLimits: a slot without a b2 joint (it cannot break while inactive).
            // checker limit: breakForce/Torque infinite, read the prefab's if such an object is re-activated inside a case.
            let listed: Vec<i32> = s.of("JointBreakLimits").next().and_then(|r| r.get("ids")).unwrap_or("").trim_matches(['[', ']']).split(';').filter_map(|x| x.parse().ok()).collect();
            for c in s.of("Clone").filter(|c| c.get("active").is_some_and(|a| a.starts_with('0'))) {
                for id in c.get("comps").unwrap_or("").trim_matches(['[', ']']).split(';').filter_map(|x| ["FixedJoint2D:", "DistanceJoint2D:", "WheelJoint2D:"].iter().find_map(|k| x.strip_prefix(k))).filter_map(|x| x.parse::<i32>().ok()) {
                    if listed.contains(&id) && !g.anchored_joints.iter().any(|a| a.component == id) && !g.adopted_joints.contains_key(&id) {
                        g.anchored_joints.push(super::AnchoredJoint { break_force: f32::INFINITY, break_torque: f32::INFINITY, joint: None, hinge: None, component: id });
                    }
                }
            }
        }
        // fnweap: the human's distance-joint wires (blood, use, cable...), their scripts on their DistanceJoint2Ds.
        if let Some(rs) = &restored {
            fnweap::load_wires(&mut g, s, rs)?;
            load_grip_joints(&mut g, s, rs)?;
        }
        // The clones behind the recorded human and item (recordings with Clone records).
        let mut item_clone = None;
        let mut map_clone = None;
        if let (Some(rs), true) = (&restored, s.of("Clone").next().is_some()) {
            let game = crate::game_assets::find_game(None)?;
            let mut db = match g.assets.take() {
                Some(db) => db,
                None => Box::new(crate::assets::db::AssetDb::open(&game)?),
            };
            // The map (`root=Map ... map=<Map asset>`): MapLoaderBehaviour's CurrentMap and its clone under WORLD. (Before
            // the item's: link_recording_item takes the last clone kept.)
            if let Some(map) = s.of("Clone").find(|r| r.get("root") == Some("Map")).map(|r| r.req("map").map(|m| m.replace('_', " "))).transpose()? {
                let (prefab, asset) = Self::map_prefab(&mut db, &map)?;
                // MapConfig.Start's weather clones (runtime children of the map root) take their recorded ids (its Start
                // runs with the pending ones, after this link).
                g.recorded_weather = s.of("Clone").filter(|r| r.get("root") == Some("Map") && r.get("parent") == Some("0") && r.name.ends_with("(Clone)")).map(|r| (r.name.clone(), r.id)).collect();
                if let Some((prefab, order, runtime)) = g.recording_clone(&mut db, s, rs, "Map", prefab)? {
                    let inst = g.retained.last().unwrap().clone();
                    map_clone = Some((prefab.clone(), order, runtime, g.retained.len() - 1));
                    // The map's Rigidbody2Ds: the clone holds their components (Rigidbody2D.bodyType and the like).
                    for o in 0..inst.objects.len() {
                        let k = prefab.objects[o].components.iter().position(|(_, c)| matches!(c, crate::assets::db::Component::Rigidbody2D(_)));
                        let Some(k) = k else { continue };
                        for p in g.physicals.iter_mut().filter(|p| inst.objects[o].components.contains(&p.id)) {
                            p.rigidbody.component = inst.objects[o].components[k];
                        }
                    }
                    g.map_root = Some(inst.objects[0].components[0]);
                    g.current_map = Some(prefab);
                    g.current_map_asset = Some(asset);
                    // MapConfig.Start's lightning clone, a runtime child of the map root (the root sits at the origin).
                    if let Some(r) = s.of("Clone").find(|r| r.get("root") == Some("Map") && r.name == "LightningWeather(Clone)") {
                        let id = list(r.req("comps")?).iter().find_map(|c| c.strip_prefix("WeatherLightningBehaviour:")?.parse::<i32>().ok()).ok_or("LightningWeather(Clone) without its WeatherLightningBehaviour")?;
                        let t = clone::trs(r)?.t;
                        let map = g.current_map.take().unwrap();
                        g.assets = Some(db);
                        let made = g.recorded_map_lightning(&map, r.id, [t[0], t[1], t[2]], id);
                        (db, g.current_map) = (g.assets.take().unwrap(), Some(map));
                        made?;
                    }
                }
            }
            // The b2 fixtures' objects (named, from the physics records) take their recorded GameObject.layer from the Clone
            // records (a map wall without a body line in the header: a raycast's layer mask needs it), and from the world
            // PhysicalBehaviour records (an object a case spawned: ep.End's gun).
            for r in s.of("Clone").chain(s.of("PhysicalBehaviour")) {
                if let (Some(&id), Some(l)) = (g.objects.get(&r.name), r.get("layer")) {
                    g.world.layers.entry(id).or_insert(l.parse().map_err(|e| format!("{} layer: {e}", r.ty))?);
                }
            }
            for kind in ["Human", "Item"] {
                if let Some(name) = s.of("Clone").find(|r| r.get("root") == Some(kind)).map(|r| r.name.clone()) {
                    // The recorder names a clone with underscores for spaces ("Machine_Gun").
                    let mut prefab = Self::catalog_prefab(&mut db, &name).or_else(|_| Self::catalog_prefab(&mut db, &name.replace('_', " ")));
                    // An fn probe item no catalog item carries: the prefab holding the case's class.
                    if let (Err(_), "Item", Some(class)) = (&prefab, kind, s.of("FnCase").next().and_then(|c| c.name.rsplit_once('.')).map(|c| c.0.to_string())) {
                        let recorded: Vec<String> = s.of("Clone").filter(|r| r.get("root") == Some("Item")).map(|r| r.name.clone()).collect();
                        prefab = Self::class_prefab(&mut db, &class, &name, &recorded);
                        // The case's target is not on the item (a limb's case, with a launched missile as the item): a class the
                        // item's own scripts hold.
                        for r in s.of("ItemScript").filter(|r| r.name == name) {
                            if prefab.is_ok() {
                                break;
                            }
                            if let Some(c) = r.get("class") {
                                prefab = Self::class_prefab(&mut db, c, &name, &recorded);
                            }
                        }
                    }
                    let prefab = prefab?;
                    let clone = g.recording_clone(&mut db, s, rs, kind, prefab)?;
                    if kind == "Item" {
                        item_clone = clone;
                    }
                }
            }
            g.assets = Some(db);
        }
        // The Main scene's GlobalShrapnelEmitter, as its Awake sets it up (an explosive's BallisticShrapnelCount, the bullet act).
        g.shrapnel = Some(main_shrapnel_emitter());
        // PhysicsScene2D.m_JointBreakLimits. A recording from before the list was dumped holds the spawn order, the
        // hinges by descending id, until a limit changes.
        let hinge_ids: Vec<i32> = s.of("HingeJoint2D").map(|r| r.id).collect();
        match recorded_break_limits(&g, s) {
            Some(limits) => g.joint_break_limits = limits,
            None => g.joint_break_limits.sort_by_key(|k| match k {
                super::BreakJoint::Hinge(h) => std::cmp::Reverse(hinge_ids[*h]),
                _ => unreachable!(),
            }),
        }

        for r in s.of("CirculationBehaviour") {
            let mut c = Circulation::default();
            load_fields(c.fields(), r)?;
            c.recorded_id = Some(r.id);
            c.Source = reference(r.req("Source")?).map(|id| circ_ids[&id]);
            c.PushesTo = list(r.req("PushesTo")?).into_iter().map(|t| circ_ids[&reference(t).unwrap()]).collect();
            c.LocalArteryRects = match r.req("LocalArteryRects")? {
                "null" => None,
                t => Some(list(t).into_iter().map(bounds).collect()),
            };
            for (id, value) in liquid_distribution(r)? {
                c.base.LiquidDistribution.insert(id, value);
            }
            c.base.SerialisableDistributions = serialisable_distributions(r)?;
            g.circulations.push(c);
        }
        // A PPG_STAB syringe.
        for r in s.of("SyringeBehaviour") {
            let mut x = super::syringe::Syringe::default();
            load_fields(x.fields(), r)?;
            x.id = r.id;
            x.Phys = g.physicals.iter().position(|p| p.name == r.name).ok_or("a syringe without a PhysicalBehaviour")?;
            let class = r.req("class")?;
            if class == "PinkSyringe" {
                x.pink = true;
                x.Fingerprint = list(r.req("Fingerprint")?).into_iter().map(f).collect();
                x.fingerprintReadPos = r.req("fingerprintReadPos")?.parse().map_err(|e| format!("fingerprintReadPos: {e}"))?;
                for item in list(r.req("liquids")?) {
                    let (class, amount) = (item.split("Item1=").nth(1).ok_or("bad pink liquid")?.split(['(', ' ']).next().unwrap(), item.split("Item2=").nth(1).ok_or("bad pink liquid")?.trim_end_matches(')'));
                    let id = LIQUIDS.iter().position(|(_, c)| *c == class).ok_or_else(|| format!("liquid class {class}"))?;
                    x.liquids.push((id, f(amount)));
                }
            }
            x.liquid = super::syringe::liquid_id(class).map(super::circulation::liquid);
            // A lodged syringe's pushTargets: the victims' circulations by limb name (the stab itself: load_recorded_stabs).
            for t in list(r.req("pushTargets")?) {
                let l = g.limbs.iter().position(|l| g.physicals[l.PhysicalBehaviour].name == t).ok_or_else(|| format!("a lodged syringe's pushTarget {t}: no limb"))?;
                x.pushTargets.push(Some(super::container::Owner::Circulation(l)));
            }
            for (id, value) in liquid_distribution(r)? {
                x.base.LiquidDistribution.insert(id, value);
            }
            g.syringes.push(x);
        }
        // A syringe's explosion before its Start (the syringe is destroyed: its Liquids recorded).
        for r in s.of("SyringeExplosionBehaviour") {
            let mut x = super::syringe::SyringeExplosion::default();
            load_fields(x.fields(), r)?;
            x.id = r.id;
            x.Position = floats(r.req("tpos")?);
            // gapB r11: a recorded null (recordings before it wrote [] for null too).
            x.Liquids_null = r.req("Liquids")? == "null";
            x.Liquids = if x.Liquids_null { Vec::new() } else { list(r.req("Liquids")?) }.into_iter().map(|c| LIQUIDS.iter().position(|(_, k)| *k == c).ok_or_else(|| format!("liquid class {c}"))).collect::<Result<_, _>>()?;
            g.syringe_explosions.push(x);
        }
        for r in s.of("ConnectedNodeBehaviour") {
            let mut n = Node::default();
            load_fields(n.fields(), r)?;
            n.Connections = list(r.req("Connections")?).into_iter().map(|t| reference(t).map(|id| node_ids[&id])).collect();
            g.nodes.push(n);
        }
        for r in s.of("SkinMaterialHandler") {
            let mut k = Skin::default();
            load_fields(k.fields(), r)?;
            // Older recordings cut lists at 64 entries ("..."); the rest keep their Awake values.
            let pts = list(r.req("damagePoints")?);
            if pts.contains(&"...") {
                for (i, t) in pts.into_iter().enumerate().filter(|(_, t)| *t != "...") {
                    k.damagePoints[i] = floats::<4>(t);
                }
            } else {
                // The whole array, at its own length (OnAfterDeserialise remakes one that is not 128 long).
                k.damagePoints = pts.into_iter().map(floats::<4>).collect();
            }
            for (i, t) in list(r.req("damagePointTimeStamps")?).into_iter().enumerate().filter(|(_, t)| *t != "...") {
                k.damagePointTimeStamps[i] = f(t);
            }
            k.intervalDistributor = distributor(r.req("intervalDistributor")?);
            k.adjacentLimbs = list(r.req("adjacentLimbs")?).into_iter().map(|t| skin_ids[&reference(t).unwrap()]).collect();
            g.skins.push(k);
        }
        for r in s.of("GForceMeasureBehaviour") {
            let mut gf = GForce::default();
            load_fields(gf.fields(), r)?;
            g.gforces.push(gf);
        }
        // An item clone's splats (an ItemScript record carries them) came with the clone's link, before these.
        let splat_base = g.splats.len();
        let splat_recs = loaded_splats(s);
        let splat_ids: HashMap<i32, usize> = splat_recs.iter().enumerate().map(|(i, r)| (r.id, splat_base + i)).collect();
        for &r in &splat_recs {
            // Its instance id: the comparison pairs it with its record by id (an unparented bleeding splat lists out of hierarchy order).
            let mut sp = FancyBloodSplat { id: r.id, ..Default::default() };
            load_splat(&mut sp, r)?;
            g.splats.push(sp);
        }
        for r in s.of("BleedingParticleBehaviour") {
            let mut b = BleedingParticle::default();
            load_fields(b.fields(), r)?;
            // A live Awake case (fnlive): Instantiate's Awake runs before the spawner sets CirculationBehaviour, so the limb
            // is the one the particle's Transform is under.
            let circ = match reference(r.req("CirculationBehaviour")?) {
                Some(id) => circ_ids[&id],
                None => {
                    let parent = r.req("tparent")?;
                    // tparent is "<name>#<Transform id>": the limb by name (one person only, else ambiguous).
                    let name = parent.rsplit_once('#').map_or(parent, |(n, _)| n);
                    let mut ls = g.limbs.iter().enumerate().filter(|(_, l)| g.physicals[l.PhysicalBehaviour].name == name).map(|(i, _)| i);
                    match (ls.next(), ls.next()) {
                        (Some(l), None) => l,
                        _ => return Err(format!("bleeding particle {} under {parent}: no single limb", r.id)),
                    }
                }
            };
            b.CirculationBehaviour = Some(circ);
            b.Parent = g.limbs[circ].PhysicalBehaviour;
            b.PushingTo = reference(r.req("PushingTo")?).map(|id| circ_ids[&id]);
            b.BloodSplatController = reference(r.req("BloodSplatController")?).map(|id| splat_ids[&id]);
            if let Some(s) = b.BloodSplatController {
                let limb = &g.limbs[circ];
                g.splats[s].parent_person = Some(limb.Person);
                // From dump16 on: the bleeding particle's Transform under its limb.
                if let Some(t) = r.get("tlpos") {
                    let parent = r.req("tparent")?;
                    if !parent.starts_with(&format!("{}#", g.physicals[limb.PhysicalBehaviour].name)) {
                        return Err(format!("bleeding particle {} under {parent}", r.id));
                    }
                    let [x, y, z]: [f32; 3] = floats(t);
                    let [sx, sy, sz]: [f32; 3] = floats(r.req("tscale")?);
                    let trs = crate::unity::transform::Trs { t: [x, y, z, 0.0], q: floats(r.req("tlrot")?), s: [sx, sy, sz, 0.0] };
                    g.splats[s].attached = Some((limb.PhysicalBehaviour, trs, g.splats[s].rotation));
                }
            }
            b.DripSounds = match r.req("DripSounds")? {
                "null" => None,
                t => Some(list(t).len()),
            };
            b.WaterPuffSystem = r.get("WaterPuffSystem").is_some_and(|t| t != "null");
            // From dump16 on: where the emitter sits under its limb (transform.up from the local rotation).
            if let (Some(t), Some(q)) = (r.get("tlpos"), r.get("tlrot")) {
                b.local_position = floats(t);
                let [x, y, z, w]: [f32; 4] = floats(q);
                b.local_up = [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)];
            }
            b.loaded = true;
            b.psm = r.get("psm").map(|t| t.parse().map_err(|_| format!("bad psm {t}"))).transpose()?;
            b.pss = r.get("pss").map_or(Vec::new(), |t| {
                t.split(';')
                    .map(|e| {
                        let v: Vec<&str> = e.split(',').collect();
                        let d = |w: &str| f64::from_bits(u64::from_str_radix(w, 16).unwrap());
                        (v[0] == "1", v[1] == "1", d(v[2]), d(v[3]))
                    })
                    .collect()
            });
            b.awake(g.prefs.FancyEffects);
            b.dripSoundClock = distributor(r.req("dripSoundClock")?);
            g.bleeding.push(b);
        }
        // FireLoopSoundControllerBehaviour's chunks an earlier scenario of the launch made (the Main scene's; they stay).
        for r in s.of("FireLoopChunk") {
            let bad = || format!("FireLoopChunk {}: bad fields", r.id);
            let hex = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|_| bad());
            let (x, y) = r.req("cell")?.split_once(',').ok_or_else(bad)?;
            let key: (i32, i32) = (x.parse().map_err(|_| bad())?, y.parse().map_err(|_| bad())?);
            let at = super::physical::fire_loop_chunk_at(key);
            let dirty = r.req("dirty")? == "1";
            let source = match r.req("source")? {
                "null" => None,
                v => {
                    let (id, vol) = v.split_once(',').ok_or_else(bad)?;
                    let id: i32 = id.parse().map_err(|_| bad())?;
                    g.fire_loop_add_source(at, id);
                    g.audio.get_mut(id).volume = hex(vol)?;
                    let ts = g.time.timeScale;
                    g.audio.add_time_scaled(id, ts, false);
                    g.audio.set_active(id, r.req("active")? == "1");
                    if dirty {
                        g.audio.play(id);
                    }
                    Some(id)
                }
            };
            g.fire_loop_chunks.insert(key, super::physical::FireLoopChunk { id: r.id, at, random_start: hex(r.req("rs")?)?, dirty, source });
        }
        // PoolGenerator's pools an earlier scenario of the launch made (ClearEverything empties them; they stay).
        for r in s.of("PoolGeneratorPool") {
            let timer = f32::from_bits(u32::from_str_radix(r.req("timer")?, 16).map_err(|e| e.to_string())?);
            // The BloodPuff pool (a Human's ShotImpact): its puffs in createdObjects order, each with the emitter splat
            // whose initialParent is the puff's Transform (the GameObject's id - 2 for a clone).
            // availableObjects: the recorded stack (avail=, top first; older recordings: the inactive puffs in creation
            // order). Assumed: every emitter active (an inactive emitter's splat is not dumped).
            if r.name == "BloodPuff_poolable_pool" && r.req("size")? != "0" {
                let mut pool = super::misc::ObjectPool { inactivityTimer: timer, ..Default::default() };
                let base = g.blood_puffs.len();
                for o in s.of("PoolObject").filter(|o| o.name == r.name) {
                    // A puff Requested this frame: its splat's Start (initialParent = transform.parent) has not run yet, so
                    // initialParent is null and the splat's Transform parent (tparent "<name>#<Transform id>") is the puff's.
                    let tparent = |x: &Record| x.get("tparent").and_then(|t| t.rsplit_once('#')).and_then(|(_, id)| id.parse::<i32>().ok());
                    let (k, pending) = match splat_recs.iter().position(|x| x.get("initialParent").and_then(reference) == Some(o.id - 2)) {
                        Some(k) => (k, false),
                        None => (splat_recs.iter().position(|x| x.get("initialParent") == Some("null") && tparent(x) == Some(o.id - 2)).ok_or(format!("blood puff {} without its emitter splat", o.id))?, true),
                    };
                    let splat = splat_base + k;
                    // Not queued: the snapshot's own pending Starts are not re-run by the load; it starts before its
                    // first FixedUpdate (splat_fixed_update), as MonoBehaviour::CallUpdateMethod does.
                    if pending {
                        g.splats[splat].started = false;
                    }
                    let active = o.req("active")? == "1";
                    g.splats[splat].parent_position = floats(o.req("pos")?);
                    // The puff's rotation (older recordings: none): a reused puff's splat Start (in RequestPrefab, before
                    // CreateImpactEffect's eulerAngles) ejects along it.
                    if let Some(q) = o.get("rot").filter(|q| *q != "-") {
                        g.splats[splat].rotation = floats(q);
                    }
                    let b = g.blood_puffs.len();
                    g.blood_puffs.push(super::misc::BloodPuff { active, splat, emitter_active: true, off: Vec::new() });
                    pool.created.push(b);
                    if !active {
                        pool.available.push(b);
                    }
                }
                if let Some(avail) = r.get("avail") {
                    pool.available = avail.split(',').filter(|i| *i != "-").rev().map(|i| i.parse::<usize>().map(|k| base + k).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
                }
                g.blood_puff_pool = Some(pool);
                continue;
            }
            let mut pool = super::misc::ObjectPool { inactivityTimer: timer, ..Default::default() };
            if r.req("size")? != "0" {
                // Its clones (PoolObject records, createdObjects order), all returned, and availableObjects (top first).
                let clones: Vec<&Record> = s.of("PoolObject").filter(|o| o.name == r.name).collect();
                // A probe case (no step runs) may start with some still out: availableObjects says which are in. A replay can't
                // (their own scripts that hand them back are not loaded).
                if s.of("FnCase").next().is_none() && clones.iter().any(|o| o.get("active") != Some("0")) {
                    unimplemented!("recording harness: pool {} holding an active object at the replay's start", r.name);
                }
                let avail = r.get("avail").ok_or("PoolGeneratorPool without avail= (re-record)")?;
                pool.created = (0..clones.len()).collect();
                pool.available = avail.split(',').filter(|i| *i != "-").rev().map(|i| i.parse::<usize>().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
                let bits = |v: &str| -> Result<[f32; 3], String> {
                    let w: Vec<f32> = v.split(',').map(|x| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
                    w.try_into().map_err(|_| format!("pos {v}"))
                };
                let pos = clones.iter().map(|o| Ok((bits(o.req("pos")?)?, o.get("active") == Some("1")))).collect::<Result<Vec<_>, String>>()?;
                g.recorded_pool_items.push((r.name.clone(), pos, None));
                g.behaviours.add(0, super::Behaviour::ObjectPool(super::pool::PoolRef::Recorded(g.recorded_pool_items.len() - 1)));
            }
            g.recorded_pools.push((r.name.clone(), pool));
        }
        if let Some(r) = s.of("AudioClipsNotLoaded").next() {
            g.audio.not_loaded = list(r.req("clips")?).into_iter().map(|c| c.rsplit_once('#').map_or(c, |(n, _)| n).to_string()).collect();
        }
        // Older recordings lack the map scripts.
        let array_len = |text: &str| if text == "null" { None } else { Some(list(text).len()) };
        for r in ambient_records(s) {
            let mut a = super::PlayAudioAtRandom::default();
            load_fields(a.fields(), r)?;
            a.clips = array_len(r.req("Clips")?);
            a.isPlaying = r.req("playing")? == "1";
            g.ambient_audio.push(a);
        }
        // A linked map clone's lights are its own (below), with these records' state.
        let load_light = |m: &mut super::MapLight, r: &Record| -> Result<(), String> {
            m.id = r.id;
            load_fields(m.fields(), r)?;
            m.switches = array_len(r.req("Switches")?);
            m.hasAudio = r.req("audio")? == "1";
            m.wake = match r.req("wake")? {
                "none" => None,
                w => Some(f64::from_bits(u64::from_str_radix(w, 16).map_err(|e| e.to_string())?)),
            };
            Ok(())
        };
        if map_clone.is_none() {
            for r in s.of("MapLightBehaviour") {
                let mut m = super::MapLight::default();
                load_light(&mut m, r)?;
                g.map_lights.push(m);
            }
        }

        let r = s.of("PersonBehaviour").next().ok_or("no person")?;
        let mut p = Person { name: r.name.clone(), id: r.id, ..Default::default() };
        load_fields(p.fields(), r)?;
        p.Limbs = list(r.req("Limbs")?).into_iter().map(|t| limb_ids[&reference(t).unwrap()]).collect();
        p.Head = reference(r.req("Head")?).map(|id| limb_ids[&id]);
        p.BoneBreakClips = list(r.req("BoneBreakClips")?).len();
        p.DismembermentClips = list(r.req("DismembermentClips")?).len();
        // Older recordings lack it (only shots read it).
        p.PoolableImpactEffect = r.get("PoolableImpactEffect").filter(|t| *t != "null").map(|_| None);
        // The prefab it names: the Human catalog prefab's (the recorder spawns that one).
        if p.PoolableImpactEffect.is_some() {
            let game = crate::game_assets::find_game(None)?;
            let db = g.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let human = Game::catalog_prefab(db, "Human")?;
            let script = human.objects.iter().find_map(|o| o.components.iter().find_map(|(_, c)| match c {
                crate::assets::db::Component::Script { class, fields, .. } if class == "PersonBehaviour" => Some((o.game_object.file, fields.clone())),
                _ => None,
            }));
            if let Some((file, v)) = script {
                let effect = v.get("PoolableImpactEffect").and_then(crate::assets::mono::Value::pptr).filter(|p| !p.is_null());
                p.PoolableImpactEffect = Some(effect.and_then(|e| db.resolve(file, e)));
            }
        }
        // fnweap: the limbs' Collider2Ds (their shapes and isTrigger; BakeColliderGridPoints reads them): the Human catalog
        // prefab's same-named objects', in component order like the recorded `colliders`.
        if g.limbs.iter().any(|l| g.physicals[l.PhysicalBehaviour].collider_defs.len() != g.physicals[l.PhysicalBehaviour].colliders.len()) {
            let game = crate::game_assets::find_game(None)?;
            let db = g.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let human = Game::catalog_prefab(db, "Human")?;
            for l in 0..g.limbs.len() {
                let ph = &mut g.physicals[g.limbs[l].PhysicalBehaviour];
                let Some(o) = human.objects.iter().find(|o| o.name.replace(' ', "_") == ph.name) else { continue };
                let defs: Vec<_> = o.components.iter().filter_map(|(_, x)| match x {
                    crate::assets::db::Component::Collider2D(c) => Some(c.clone()),
                    _ => None,
                }).collect();
                if defs.len() == ph.colliders.len() {
                    ph.collider_defs = defs;
                }
            }
        }
        // LimbBehaviour.Awake's ShatteredObjectSpriteInitialiser generators (a snapshot does not hold them): the Human catalog
        // prefab's same-named objects'.
        if g.limbs.iter().any(|l| l.ShatteredObjectGenerators.is_none()) {
            let game = crate::game_assets::find_game(None)?;
            let mut db = g.assets.take().unwrap_or_else(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let human = Game::catalog_prefab(&mut db, "Human")?;
            let r = g.load_limb_generators_by_name(&mut db, &human);
            // ProceduralShatterMaterialPool.ShatterMasks, the Main scene's (read once per process).
            static MASKS: std::sync::OnceLock<Vec<super::shatter::Texture>> = std::sync::OnceLock::new();
            if g.shatter.shatter_masks.is_empty() {
                if MASKS.get().is_none() {
                    let mut t = Game::default();
                    t.load_main_shatter_masks(&mut db)?;
                    let _ = MASKS.set(t.shatter.shatter_masks);
                }
                g.shatter.shatter_masks = MASKS.get().unwrap().clone();
            }
            g.assets = Some(db);
            r?;
        }
        // The Resources prefab the record names (@BleedingParticle#id): the bleeds' systems and new bleeds' DripSounds.
        if let Some(name) = r.get("BleedingParticlePrefab").filter(|t| *t != "null") {
            let name = name.trim_start_matches('@').split('#').next().unwrap();
            let game = crate::game_assets::find_game(None)?;
            let db = g.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let go = db.resource(&format!("Prefabs/{name}"), 1)?.ok_or(format!("Resources Prefabs/{name}"))?;
            let (drips, puff) = super::link::bleeding_drip_sounds(db, go)?;
            p.BleedingDripSounds = Some(drips);
            p.BleedingWaterPuff = puff;
            p.BleedingParticlePrefab = Some(go);
            for b in &mut g.bleeding {
                b.prefab = Some(go);
            }
        }
        load_person_poses(&g, &mut p, rig_poses(rig, &g.limbs)?, r)?;
        p.localScale = match s.snap.iter().find(|(k, _)| k == "rootScale") {
            Some((_, v)) => floats(v),
            // Older recordings: a unit-scale limb's lossyScale, which can be 1 ulp off the root's localScale.
            None => {
                let lossy: [f32; 3] = floats(rbs[limb_rb[p.Head.unwrap()].ok_or("a body-less head")?].req("lossy")?);
                [lossy[0], lossy[1], 1.0]
            }
        };
        if g.transform_nodes.is_empty() {
            return Err("a recording without its TransformChain (dump11 and later have it)".into());
        }
        // Its Awake ran in the game: AliveByTransform holds its root.
        p.alive_entry = true;
        g.people.push(p);
        // The limbs' context menu buttons (the menu is no recorded state): PhysicalBehaviour.Start's, then LimbBehaviour.Start's
        // CreateContextMenuOptions, as their Starts ran in the game.
        for l in 0..g.limbs.len() {
            let q = g.limbs[l].PhysicalBehaviour;
            g.physical_add_menu_buttons(q);
            g.limb_create_context_menu_options(l);
        }
        // The map's and the PPG_STAB item's fire maps (both instantiated before the Person), in record order.
        for f in 0..g.fire_maps.len() {
            g.behaviours.add(0, super::Behaviour::FireMap(f));
        }
        // The BulletTracerPool's clones an earlier scenario or case made: its counts and inactivity timer. gapA r15: the
        // active ones (size - inactive) are held by the emitters' `tracers` (clone.rs loads those counts); their flight
        // (a BallisticIteration coroutine of an earlier case) is not recorded, so a case it would end in differs honestly.
        if let Some(r) = s.of("TracerPool").next() {
            let (size, inactive): (usize, usize) = (r.req("size")?.parse().map_err(|_| "TracerPool size")?, r.req("inactive")?.parse().map_err(|_| "TracerPool inactive")?);
            if inactive > size {
                return Err("TracerPool inactive > size".into());
            }
            (g.tracer_pool.created, g.tracer_pool.available) = (size, inactive);
            g.tracer_pool.inactivityTimer = f(r.req("timer")?);
        }
        // The Main scene's singletons (their Awake's `Instance = this`), for a snapshot of a recording made for them
        // (an em= on a static Instance/Main accessor's case): level1's ids from the WORLD PlayAudioAtRandom's.
        if s.of("FnCase").any(|r| r.get("em").is_some() && r.name.split_once('.').is_some_and(|(c, m)| super::scene::SINGLETONS.contains(&c) && matches!(m, "get_Instance" | "set_Instance" | "get_Main" | "set_Main"))) {
            let mut w = s.of("PlayAudioAtRandom").filter(|r| r.id > 0);
            match (w.next(), w.next()) {
                (Some(r), None) => g.scene_singletons_awake(("PlayAudioAtRandom", r.id), s.of("FnCase").find_map(|r| r.get("l0")?.parse().ok()))?,
                _ => return Err("not one scene PlayAudioAtRandom record".into()),
            }
        }
        // The map's scripts (instantiated first), their recorded fields over the linked ones.
        if let Some((prefab, order, runtime, at)) = map_clone {
            g.link_recording_item(s, at, &prefab, &order, runtime)?;
            // An item clone's light (a map light prefab spawned as an item) is its ItemScript's.
            let item: HashSet<i32> = s.of("ItemScript").map(|r| r.id).collect();
            for r in s.of("MapLightBehaviour").filter(|r| !item.contains(&r.id)) {
                let m = g.map_lights.iter_mut().find(|m| m.id == r.id).ok_or_else(|| format!("MapLightBehaviour {} not on the map clone", r.id))?;
                load_light(m, r)?;
            }
            let map = map_clone_splats(s);
            for r in s.of("FancyBloodSplatController").filter(|r| map.contains(&r.id)) {
                let sp = g.splats.iter_mut().find(|x| x.id == r.id).ok_or_else(|| format!("FancyBloodSplatController {} not on the map clone", r.id))?;
                load_splat(sp, r)?;
            }
            g.load_item_scripts(s)?;
        }
        // The PPG_STAB item comes first: it is instantiated before the Person.
        if let Some((prefab, order, runtime)) = item_clone {
            // The layers only: the item's scripts are the clone's (loop_test's load_layers sees them linked).
            g.load_layers(&header.iter().filter(|l| !l.starts_with("stab item=")).cloned().collect::<Vec<_>>());
            let at = g.retained.len() - 1;
            g.link_recording_item(s, at, &prefab, &order, runtime)?;
            for (j, id) in late_attached.drain(..) {
                let p = g.physicals.iter().position(|p| p.rigidbody.component == id).ok_or(format!("slider: no item body {id}"))?;
                g.fixed_joints[j].attached = BodyOwner::Physical(p);
            }
            // A body the item's link made; the rest come with a Pasted/Spawned root's link below (fnb2h: the pasted Tank's Hinge).
            late_connected.retain(|&(j, id)| match g.node_rigidbodies.iter().position(|n| n.rigidbody.component == id) {
                Some(k) => { g.fixed_joints[j].connected = Some(BodyOwner::Node(k)); false }
                None => true,
            });
            // The item's map lights: their recorded state over the linked one (as the map clone's above).
            for r in s.of("MapLightBehaviour") {
                if let Some(m) = g.map_lights.iter_mut().find(|m| m.id == r.id) {
                    load_light(m, r)?;
                }
            }
            // Each body's RigidbodyMovementState2D as recorded (the link starts them empty).
            for r in s.of("ItemB2Body") {
                if let (Some(m), Some(p)) = (r.get("move"), g.physicals.iter().position(|p| p.rigidbody.component == r.id)) {
                    g.physicals[p].rigidbody.movement = movement_state(&words(m));
                }
            }
            g.load_item_scripts(s)?;
            // The SliderJoint2D components as recorded: a script's Start set their limits from prefab values.
            for r in s.of("SliderJoint2D") {
                let Some(&j) = g.adopted_joints.get(&r.id) else { continue };
                let Some(sl) = g.fixed_joints[j].slider.as_mut() else { continue };
                let bits = |k: &str| -> Result<Vec<f32>, String> { Ok(r.req(k)?.split(',').map(f).collect()) };
                let (motor, limits) = (bits("motor")?, bits("limits")?);
                (sl.use_motor, sl.motor_speed, sl.max_motor_torque) = (r.req("motor")?.starts_with('1'), motor[1], motor[2]);
                (sl.use_limits, sl.lower, sl.upper) = (r.req("limits")?.starts_with('1'), limits[1], limits[2]);
            }
            // The scripts' Awake and Start re-listed joints they set limits on: the recorded order is after theirs.
            if let Some(limits) = recorded_break_limits(&g, s) {
                g.joint_break_limits = limits;
            }
        }
        // The roots a script instantiated at run time (`root=Spawned:<id>`: ep.End's gun, a DestroyableBehaviour's debris),
        // A pasted root (`root=Pasted:<id>`, FnWeap's spawned wire ends) the same way.
        // after the item: the SpawnableAsset's prefab, else the prefab asset of the recorded name holding a PhysicalBehaviour.
        // One no prefab is found for stays without a clone (a level object, a recorder helper the catalog lacks).
        if let Some(rs) = &restored {
            let mut kinds: Vec<&str> = Vec::new();
            for r in s.of("Clone") {
                if let Some(k) = r.get("root").filter(|k| (k.starts_with("Spawned:") || k.starts_with("Pasted:")) && !kinds.contains(k)) {
                    kinds.push(k);
                }
            }
            for kind in kinds {
                let root = s.of("Clone").find(|r| r.get("root") == Some(kind)).unwrap();
                let mut db = g.assets.take().ok_or("a Spawned clone without the game's data")?;
                let prefab = match (root.get("spawnable"), root.get("prefab")) {
                    // gapB r4: a recorder-pasted prefab no catalog item is (FnTiny's Javelin_missile: the launcher's projectile
                    // prefab): the prefab asset of that name holding a PhysicalBehaviour, as a "(Clone)" root's.
                    (Some(a), _) => Self::catalog_prefab(&mut db, a)
                        .or_else(|_| Self::catalog_prefab(&mut db, &a.replace('_', " ")))
                        .or_else(|e| {
                            if !kind.starts_with("Pasted:") {
                                return Err(e);
                            }
                            let recorded: Vec<String> = s.of("Clone").filter(|r| r.get("root") == Some(kind)).map(|r| r.name.clone()).collect();
                            Self::class_prefab(&mut db, "PhysicalBehaviour", a, &recorded).map_err(|_| e)
                        })
                        .map(Some),
                    // A recorder-spawned root (recorded as root=Pasted too): FnProbe.SpawnPrefab names it by its SpawnableAsset.
                    (_, Some(name)) if s.of("Clone").any(|r| r.id == root.id && r.get("root") == Some("Pasted")) => {
                        Self::catalog_prefab(&mut db, name).or_else(|_| Self::catalog_prefab(&mut db, &name.replace('_', " ")))
                            // gapB r5: no catalog item (FnTiny's Launched_missile, the Javelin's projectile): the prefab asset.
                            .or_else(|e| Self::class_prefab(&mut db, "PhysicalBehaviour", name, &[]).map_err(|_| e))
                            .map(Some)
                    }
                    // (A root without "(Clone)" is a map object SetParent(null) took out: the map clone holds it.)
                    (_, Some(name)) if root.name.ends_with("(Clone)") => {
                        let recorded: Vec<String> = s.of("Clone").filter(|r| r.get("root") == Some(kind)).map(|r| r.name.clone()).collect();
                        Self::class_prefab(&mut db, "PhysicalBehaviour", name, &recorded)
                            // gapB r26: an effect root holding no PhysicalBehaviour (a Defibrillator's BigZap: ParticleSystem and
                            // ExplosionSoundBehviour): the prefab of that name holding one of the root's recorded scripts.
                            .or_else(|e| list(root.req("comps")?).iter().filter_map(|c| c.rsplit_once(':')).find_map(|(class, _)| Self::class_prefab(&mut db, class, name, &recorded).ok()).ok_or(e))
                            .map(Some)
                    }
                    _ => Ok(None),
                };
                let clone = match prefab {
                    Ok(Some(prefab)) => g.recording_clone(&mut db, s, rs, kind, prefab),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                };
                g.assets = Some(db);
                let clone = clone.map_err(|e| format!("{kind} {}: {e}", root.name))?;
                if let Some((prefab, order, runtime)) = clone {
                    let at = g.retained.len() - 1;
                    g.link_recording_item(s, at, &prefab, &order, runtime)?;
                    g.load_item_scripts(s)?;
                }
            }
            // A pasted root SetParent put under another Spawned root (FnTiny's attachment, connected through the pistol's
            // point): its own prefab's clone (`Pasted@<id>`), linked; the link hangs it under its recorded parent.
            let pasted_roots: Vec<i32> = s.of("Clone").filter(|r| r.get("root") == Some("Pasted") && r.get("index") == Some("0")).map(|r| r.id).collect();
            let nested: Vec<&Record> = s
                .of("Clone")
                .filter(|r| r.get("root").is_some_and(|k| k.starts_with("Spawned:")) && r.get("index") != Some("0") && pasted_roots.contains(&r.id))
                .filter(|r| !s.of("Clone").any(|x| x.get("root") == Some(format!("Spawned:{}", r.id).as_str())))
                .collect();
            for r in nested {
                let mut db = g.assets.take().ok_or("a Spawned clone without the game's data")?;
                let clone = Self::catalog_prefab(&mut db, &r.name).or_else(|_| Self::catalog_prefab(&mut db, &r.name.replace('_', " "))).and_then(|prefab| g.recording_clone(&mut db, s, rs, &format!("Pasted@{}", r.id), prefab));
                g.assets = Some(db);
                let Some((prefab, order, runtime)) = clone.map_err(|e| format!("nested pasted root {}: {e}", r.name))? else { continue };
                let at = g.retained.len() - 1;
                g.link_recording_item(s, at, &prefab, &order, runtime)?;
                g.load_item_scripts(s)?;
                let comps = list(r.req("comps")?);
                if !g.physicals.iter().any(|p| p.parent.is_some() && comps.iter().any(|c| c.ends_with(&format!(":{}", p.id)))) {
                    return Err(format!("nested pasted root {}: not under its recorded parent", r.name));
                }
            }
            // Each point's HingeJoint2D (ConnectAttachment's AddComponent): the restored b2 joint between the firearm and the
            // point's FirearmAttachment, its anchor in the firearm's space.
            for r in s.of("ItemScript").filter(|r| r.get("class") == Some("FirearmAttachmentPointBehaviour")) {
                let Some(pt) = g.attachment_points.iter().position(|x| x.id == r.id) else { continue };
                let Some(hinge) = r.get("joint").and_then(reference) else { continue };
                let Some(h) = s.of("ItemHingeJoint2D").find(|h| h.id == hinge) else { continue };
                let Some(&joint) = rs.joints.get(&format!("{}#{}", h.name, h.id)) else { continue };
                let (Some(a), Some(f)) = (g.attachment_points[pt].FirearmAttachment, g.attachment_points[pt].firearm) else { continue };
                let (Some(gun), Some(att)) = (g.firearms[f].physical, g.firearm_attachments[a].physical) else { continue };
                let local = g.world.revolute(joint).def.local_anchor_a;
                let s3 = g.lossy_scale(gun);
                let pt = &mut g.attachment_points[pt];
                (pt.joint, pt.jointed, pt.anchor) = (Some(joint), Some((gun, att)), [local.x / s3[0], local.y / s3[1]]);
            }
        }
        for (j, id) in late_connected.drain(..) {
            let k = g.node_rigidbodies.iter().position(|n| n.rigidbody.component == id).ok_or(format!("slider: no body {id}"))?;
            g.fixed_joints[j].connected = Some(BodyOwner::Node(k));
        }
        // A Pasted/Spawned root's link listed its joints after the item's: the recorded order again (fnb2h: the pasted Tank's).
        if restored.is_some() && s.of("Clone").any(|r| r.get("root").is_some_and(|k| k.starts_with("Pasted:") || k.starts_with("Spawned:"))) {
            if let Some(limits) = recorded_break_limits(&g, s) {
                g.joint_break_limits = limits;
            }
        }
        // fnweap: the energy wires' antenna and eggs (the pasted items' scripts are in the arenas now).
        fnweap::load_item_wires(&mut g, s)?;
        // Each physical's DecalControllerBehaviours with their localDecalPositions (IsNearOtherDecal reads them) and
        // dirty (CreateContainer has run). The decals' looks are not recorded: a restored object is never drawn.
        for r in s.of("DecalControllers") {
            let Some(p) = g.physicals.iter().position(|x| x.id == r.id) else { continue };
            let mut list = Vec::new();
            for c in r.req("list")?.split('|') {
                let mut f = c.split(':');
                let (Some(name), Some(dirty), Some(at)) = (f.next(), f.next(), f.next()) else { return Err(format!("bad decal controller {c}")) };
                let descriptor = if name == "null" { super::physical::NULL_DECAL } else { g.decal_descriptor(name) };
                let decals = at.split('/').filter(|v| !v.is_empty()).map(|v| super::misc::Decal { position: floats(v), size: 0.0, sprite: 0, colour: [1.0; 4], scale: [1.0, 1.0], streaks: Vec::new() }).collect();
                list.push(super::misc::DecalController { descriptor, decals, holder: (dirty == "1").then_some((None, 0, 0)), holder_hidden: false });
            }
            g.physicals[p].decal_controllers = list;
        }
        for i in 0..g.syringes.len() {
            g.behaviours.add(0, super::Behaviour::Syringe(i));
            // SendMessage("Use") on the item finds the syringe script by its component.
            g.behaviour_of.insert(g.syringes[i].id, (0, super::Behaviour::Syringe(i)));
        }
        // The recorded Person's behaviours (all execution order 0): the root's, each limb's components, the
        // snapshot's splats, the GForceMeasureBehaviours LimbBehaviour.Start added, the bleeding particles.
        for p in 0..g.people.len() {
            g.behaviours.add(0, super::Behaviour::Person(p));
        }
        for l in 0..g.limbs.len() {
            g.behaviours.add(0, super::Behaviour::Skin(l));
            g.behaviours.add(0, super::Behaviour::Circulation(l));
            if let Some(gr) = g.limbs[l].GripBehaviour {
                g.behaviours.add(0, super::Behaviour::Grip(gr));
            }
            g.behaviours.add(0, super::Behaviour::GoreString(l));
        }
        // Assumed: the BloodPuff pool's ObjectPoolBehaviour put before the splats (it joins at the first puff).
        if g.blood_puff_pool.is_some() {
            g.behaviours.add(0, super::Behaviour::ObjectPool(super::pool::PoolRef::BloodPuff));
        }
        // The records are in hierarchy order; the managers hold the clones in creation order, descending id.
        let mut splats: Vec<(i32, usize)> = splat_recs.iter().enumerate().map(|(i, r)| (r.id, splat_base + i)).collect();
        splats.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (_, i) in splats {
            g.behaviours.add(0, super::Behaviour::Splat(i));
        }
        for l in 0..g.gforces.len() {
            g.behaviours.add(0, super::Behaviour::GForce(l));
        }
        // The unstarted limbs' LimbBehaviour.Start waits for the next Start pass (it adds the GForceMeasureBehaviours, in limb order).
        // ponytail: only the limbs' Starts are queued; the person's other unstarted components (Circulation, Node, GoreString,
        // Person) load as started. Queue them too when a stepped check needs a fresh person.
        if limbs_unstarted {
            for l in 0..g.limbs.len() {
                g.limbs[l].started = false;
                g.pending_starts.push(super::misc::PendingStart::Limb(l));
            }
        }
        let mut bleeding: Vec<(i32, usize)> = s.of("BleedingParticleBehaviour").enumerate().map(|(i, r)| (r.id, i)).collect();
        bleeding.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (_, i) in bleeding {
            g.behaviours.add(0, super::Behaviour::Bleeding(i));
        }
        if let Some(r) = s.of("ToolControllerBehaviour").next() {
            g.tool = Some(load_tool(&g, s, r, restored.as_ref().ok_or("a tool without the b2 world")?)?);
        }
        // MapConfig.Start's lightning clone, as recorded (above): the one the linked map's Start made goes.
        if let Some(r) = s.of("Clone").find(|r| r.get("root") == Some("Map") && r.name == "LightningWeather(Clone)") {
            let id = list(r.req("comps")?).iter().find_map(|c| c.strip_prefix("WeatherLightningBehaviour:")?.parse::<i32>().ok()).ok_or("LightningWeather(Clone) without its WeatherLightningBehaviour")?;
            g.keep_recorded_map_lightning(id);
        }
        // A PPG_MAPCONFIG recording's MapConfig record over the linked map's Start.
        g.load_map_config_record(s)?;
        // An item clone's map lights (linked with the item): their records too (wake is no ItemScript field).
        let item: HashSet<i32> = s.of("ItemScript").map(|r| r.id).collect();
        for r in s.of("MapLightBehaviour").filter(|r| item.contains(&r.id)) {
            if let Some(m) = g.map_lights.iter_mut().find(|m| m.id == r.id) {
                load_light(m, r)?;
            }
        }
        // fnmaped: the recorded stabs and SerialisedLayerBehaviours (dump/fnmaped.rs).
        g.load_recorded_stabs(s)?;
        g.load_serialised_layers(s)?;
        g.load_recorded_camera(s)?;
        // gapB r20: each StickyGrenade's `joint`: its recorded FixedJoint2D's adopted entry.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("StickyGrenadeBehaviour")) {
            let j = r.get("joint").and_then(reference).and_then(|id| g.adopted_joints.get(&id).copied());
            if let Some(x) = g.sticky_grenades.iter_mut().find(|x| x.id == r.id) { x.joint = j; }
        }
        // fnv52: each CrossbowBehaviour's syringeJoint (its recorded FixedJoint2D's adopted entry) and AttachedSyringe.
        for r in s.of("ItemScript").filter(|r| r.get("class") == Some("CrossbowBehaviour")) {
            let j = r.get("syringeJoint").and_then(reference).and_then(|id| g.adopted_joints.get(&id).copied());
            let a = r.get("AttachedSyringe").and_then(reference).and_then(|id| g.syringes.iter().position(|x| x.id == id));
            if let Some(x) = g.v45.crossbows.iter_mut().find(|x| x.id == r.id) { (x.syringeJoint, x.AttachedSyringe) = (j, a); }
        }
        // gapB r20: IgnoreCollisionStackController's stack as recorded (`IgnoreStack pairs=[a,b:n;..]`) over the count guessed
        // from the pairs (2) and what the load's own requests added: a pair the stack and the recorded ignore set both lack
        // (a re-run NoCollide.Start's, taken off by the recorder's seed) is not ignored either.
        if let Some(r) = s.of("IgnoreStack").next() {
            let mut st = std::collections::HashMap::new();
            for x in r.req("pairs")?.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()) {
                let (p, n) = x.rsplit_once(':').ok_or("IgnoreStack entry")?;
                let (a, b) = p.split_once(',').ok_or("IgnoreStack pair")?;
                let k = (a.parse::<i32>().map_err(|_| "IgnoreStack id")?, b.parse::<i32>().map_err(|_| "IgnoreStack id")?);
                st.insert(k, n.parse::<i32>().map_err(|_| "IgnoreStack count")?);
            }
            let mut rec = std::collections::HashSet::new();
            for r in s.of("IgnoredCollisions") {
                for pair in r.req("pairs")?.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()) {
                    if let Some((a, b)) = pair.split_once(',') && let (Ok(a), Ok(b)) = (a.parse::<i32>(), b.parse::<i32>()) { rec.insert((a, b)); }
                }
            }
            let gone: Vec<(i32, i32)> = g.ignore_stack.keys().copied().filter(|k| !st.contains_key(k) && !rec.contains(k)).collect();
            // The un-ignore is a load fix-up, not a game event: its refilter's wake must not reset the recorded sleep state
            // (b2Body sleepTime, awake) the snapshot already holds.
            let sleep: Vec<_> = g.world.bodies.iter().map(|b| b.as_ref().map(|b| (b.flags, b.sleep_time))).collect();
            for (a, b) in gone { g.world.ignore_collision(a, b, false); }
            for (b, s) in g.world.bodies.iter_mut().zip(sleep) {
                if let (Some(b), Some(s)) = (b.as_mut(), s) {
                    (b.flags, b.sleep_time) = s;
                }
            }
            g.ignore_stack = st;
        }
        // fnmaped r20: the runtime clones under the loaded physicals (their Start's, not run again): BubbleParticlePrefab
        // (SetupBubblePrefab's BubbleVFX clone, its GameObject) and scrapeParticles (the Global.MetalScrapePrefab clone's root
        // ParticleSystem; the clone's GameObject from its Clone row), their Transforms laid out as the prefabs' (tool.rs).
        for r in s.of("PhysicalBehaviour") {
            let Some(p) = g.physicals.iter().position(|p| p.id == r.id) else { continue };
            let t = g.physicals[p].transform_id;
            if let Some(go) = r.get("BubbleParticlePrefab").and_then(reference) {
                g.add_runtime_clone(t, &[(go - 2, None)]);
            }
            if let Some(ps) = r.get("scrapeParticles").and_then(reference) {
                let key = format!("ParticleSystem:{ps}");
                let Some(go) = s.of("Clone").find(|c| c.get("comps").is_some_and(|x| x.trim_matches(['[', ']']).split(';').any(|e| e == key))).map(|c| c.id) else { continue };
                let objects = match &g.metal_scrape { Some(m) => m.2.clone(), None => g.load_metal_scrape().2 };
                let mut id = go;
                let nodes: Vec<(i32, Option<usize>)> = objects.iter().map(|&(k, up)| { let t = id - 2; id -= 2 * k as i32; (t, up) }).collect();
                g.add_runtime_clone(t, &nodes);
            }
        }
        // Each adopted SliderJoint2D's b2PrismaticJoint as recorded, last: a script's Start (WinchBehaviour.Start's
        // connectedBody null-and-back) and a pasted root's (weapons r20: fnw26/fnw28's Pasted Winch, loaded after the item
        // pass) recreated the b2 joint from the poses at load (anchors and reference angle an ulp off, impulses zero).
        for r in s.of("SliderJoint2D") {
            let Some(&j) = g.adopted_joints.get(&r.id) else { continue };
            if let (Some(id), Some(w)) = (g.fixed_joints[j].joint, r.get("joint").filter(|w| *w != "null")) {
                physics::load_prismatic(g.world.joints[id].as_mut().unwrap().prismatic_mut(), &words(w));
            }
        }
        // PhysicalBehaviour.Start's SetProperties ran before the recording: each Rigidbody2D has its Properties' material
        // (batch3 r4: the fn-check's snapshots too, so SetProperties again changes nothing: sleepTime stays).
        // batch3 r5: a record that names the body's sharedMaterial (`material=`, null after a reset) says it.
        let materials: std::collections::HashMap<i32, bool> = s.of("Rigidbody2D").chain(s.of("ItemRigidbody2D")).filter_map(|r| Some((r.id, r.get("material")? != "null"))).collect();
        for p in 0..g.physicals.len() {
            let recorded = materials.get(&g.physicals[p].rigidbody.component).copied();
            g.physicals[p].rigidbody_has_material = recorded.unwrap_or_else(|| g.properties[g.physicals[p].Properties].friction.is_some());
        }
        Ok(g)
    }

    /// GameObject layers from the recorder's header: `collider <name> layer=<n> ...` and
    /// `body <name> ... layer=<n>`.
    pub fn load_layers(&mut self, header: &[String]) {
        // The layer collision matrix with them (Physics2DSettings): a car's doors (layer 10) pass through a human (9).
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let db = self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        self.world.layer_collision_matrix = Game::load_physics2d_settings(db).expect("Physics2DSettings").layer_collision_matrix;
        // The Main scene's BulletTracerPool, which every firearm's tracer comes from.
        self.load_tracer_pool();
        for line in header {
            // `stab item=<name> ... comps=<Type>:<enabled>;...`: the item's components, for the Lodged/Dislodged messages.
            if let Some(rest) = line.strip_prefix("stab item=") {
                let name = rest.split(' ').next().unwrap().replace(' ', "_");
                let comps = rest.split(" comps=").nth(1).unwrap_or("");
                for c in comps.split(';').filter(|c| !c.is_empty()) {
                    let (ty, enabled) = c.rsplit_once(':').unwrap();
                    if ty == "SharpOnAllSidesBehaviour" && enabled == "1" {
                        // Only the Sword has one, disabled, and no game script enables it (a save's Enabled = true is hand-made).
                        unimplemented!("mod-only: SharpOnAllSidesBehaviour.FixedUpdate: enabled on {name}");
                    }
                    // A recording with Clone rows linked the item's firearms with its other scripts.
                    if ty == "FirearmBehaviour" && !self.firearms.iter().any(|f| f.physical.is_some_and(|p| self.physicals[p].name == name)) {
                        self.recorded_firearm(rest.split(' ').next().unwrap());
                    }
                    // Likewise its PointToVelocityBehaviours (else its FixedUpdate ran twice: the Fusion Bomb's torque doubled).
                    if ty == "PointToVelocityBehaviour" && enabled == "1" && !self.point_to_velocities.iter().any(|x| self.physicals[x.physical].name == name) {
                        self.recorded_point_to_velocity(rest.split(' ').next().unwrap());
                    }
                    if ty == "ExcaliburBehaviour" || ty.contains("Syringe") {
                        if let Some(p) = self.physicals.iter().position(|p| p.name == name) {
                            self.physicals[p].lodge_receiver = ty.to_string();
                        }
                    }
                }
                continue;
            }
            let (name, layer) = if let Some(rest) = line.strip_prefix("collider ") {
                let Some((name, tail)) = rest.split_once(" layer=") else { continue };
                (name.to_string(), tail.split(' ').next().unwrap())
            } else if let Some(rest) = line.strip_prefix("body ") {
                // The name runs to the first key=value (a "Winch base" has a space in it).
                let Some(end) = rest.split(' ').position(|t| t.contains('=')) else { continue };
                let name = rest.split(' ').take(end).collect::<Vec<_>>().join(" ");
                let Some(layer) = rest.split(' ').skip(end).find_map(|kv| kv.strip_prefix("layer=")) else { continue };
                (name, layer)
            } else {
                continue;
            };
            // Snapshot records name objects with spaces as underscores.
            if let Some(&id) = self.objects.get(&name.replace(' ', "_")) {
                self.world.layers.insert(id, layer.parse().unwrap());
            }
        }
        // A stab item's collider objects without a Rigidbody2D (a gun's BarrelAttachment) have no header line: the catalog
        // prefab's layer.
        if let Some(item) = header.iter().find_map(|l| l.strip_prefix("stab item=")).map(|r| r.split(' ').next().unwrap().to_string()) {
            let prefab = Self::catalog_prefab(self.assets.as_mut().unwrap(), &item.replace('_', " ")).expect("the PPG_STAB item's prefab");
            for o in &prefab.objects {
                if let Some(&id) = self.objects.get(&o.name.replace(' ', "_")) {
                    self.world.layers.entry(id).or_insert(o.layer as i32);
                }
            }
            // The Person's limbs have no header line either (a detector's Physics2D.Raycast hits them): the Human
            // catalog prefab's layers, as the recorder spawned it.
            let prefab = Self::catalog_prefab(self.assets.as_mut().unwrap(), "Human").expect("the Human prefab");
            for o in &prefab.objects {
                if let Some(&id) = self.objects.get(&o.name.replace(' ', "_")) {
                    self.world.layers.entry(id).or_insert(o.layer as i32);
                }
            }
        }
    }

    /// The FirearmBehaviours of a PPG_STAB catalog item `asset` (a recording has no clones): the catalog prefab's fields and
    /// Cartridge as its Awake read them, linked to the recorded PhysicalBehaviour, then Start (the recording starts after
    /// both ran, so the Awake's instance ids are already in the recorded ones). A component's instance id counts down by
    /// two in component order from the recorded PhysicalBehaviour's.
    fn recorded_firearm(&mut self, asset: &str) {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        let mut db = self.assets.take().unwrap();
        let prefab = Self::catalog_prefab(&mut db, asset).expect("the PPG_STAB item's prefab");
        // Shoot's DistantSoundBehaviour.Play draws from the level's clip lists.
        self.load_distant_sounds(&mut db).expect("the game's data");
        for (o_index, o) in prefab.objects.iter().enumerate() {
            let kind = |class: &str| o.components.iter().position(|(_, c)| matches!(c, crate::assets::db::Component::Script { class: k, .. } if k == class));
            let (Some(kf), Some(kp)) = (kind("FirearmBehaviour"), kind("PhysicalBehaviour")) else { continue };
            // The root is recorded under the catalog's name (the AK-47's and G1's prefab roots have others).
            let by_name = self.physicals.iter().position(|p| p.name == o.name.replace(' ', "_"));
            let Some(p) = by_name.or_else(|| o.parent.is_none().then(|| self.physicals.iter().position(|p| p.name == asset)).flatten()) else { continue };
            let crate::assets::db::Component::Script { enabled, fields, .. } = &o.components[kf].1 else { unreachable!() };
            let mut fa = Self::firearm_from_fields(&mut db, o.game_object.file, fields).expect("the item's FirearmBehaviour");
            fa.physical = Some(p);
            fa.id = self.physicals[p].id - 2 * (kf as i32 - kp as i32);
            fa.enabled = *enabled;
            self.firearms.push(fa);
            let f = self.firearms.len() - 1;
            self.firearm_muzzle_smoke(&mut db, f).expect("the gun's MuzzleSmoke");
            self.firearm_start(f);
            self.behaviours.add(0, super::Behaviour::Firearm(f));
            // A MinigunBehaviour on the same object (its FirearmBehaviour field is this one); its AudioSource fields
            // are the object's own sources, muted here (Awake's property block is presentation).
            if let Some(km) = kind("MinigunBehaviour") {
                let crate::assets::db::Component::Script { enabled, fields, .. } = &o.components[km].1 else { unreachable!() };
                let mut m = super::minigun::Minigun::default();
                super::spawn::load_values(m.fields(), fields).expect("MinigunBehaviour's fields");
                m.id = self.physicals[p].id - 2 * (km as i32 - kp as i32);
                (m.enabled, m.physical, m.firearm) = (*enabled, p, f);
                for name in ["RattleAudioSource", "FireAudioSource"] {
                    let r = fields.get(name).and_then(|v| v.pptr()).expect("an AudioSource");
                    let ka = o.components.iter().position(|(c, _)| c.path_id == r.path_id).expect("the minigun's own AudioSource");
                    let id = self.physicals[p].id - 2 * (ka as i32 - kp as i32);
                    if !self.audio.sources.contains_key(&id) {
                        self.audio.add(id, super::audio::AudioSource::default());
                    }
                    self.audio.source_fields.insert((m.id, name.to_string()), vec![Some(id)]);
                }
                // BarrelStart, BarrelLoop, BarrelStop, FireStop.
                self.audio.register_clip_fields(&db, o.game_object.file, m.id, fields).expect("MinigunBehaviour's clips");
                self.miniguns.push(m);
                self.behaviours.add(0, super::Behaviour::Minigun(self.miniguns.len() - 1));
            }
            // The item's ActOnCollide (OnCollisionEnter2D draws Random.value): the recording holds no prefab clone.
            self.act_on_collide_record(&prefab, o_index, p, f).expect("the item's ActOnCollide");
        }
        self.assets = Some(db);
    }

    /// The PointToVelocityBehaviours of a PPG_STAB catalog item `asset` (a Spear, a Crossbow Bolt): the prefab's fields,
    /// linked to the recorded PhysicalBehaviour of the same object (the recording holds no prefab clone).
    fn recorded_point_to_velocity(&mut self, asset: &str) {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let mut db = self.assets.take().unwrap_or_else(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        let prefab = Self::catalog_prefab(&mut db, asset).or_else(|_| Self::catalog_prefab(&mut db, &asset.replace('_', " "))).expect("the PPG_STAB item's prefab");
        for o in &prefab.objects {
            let Some(v) = o.components.iter().find_map(|(_, c)| match c { crate::assets::db::Component::Script { class, fields, .. } if class == "PointToVelocityBehaviour" => Some(fields), _ => None }) else { continue };
            // Else the recorded item itself, named as the catalog's item (a Crossbow Bolt's prefab object has another name).
            let p = self.physicals.iter().position(|p| p.name == o.name.replace(' ', "_")).or_else(|| self.physicals.iter().position(|p| p.name == asset)).expect("the item's recorded PhysicalBehaviour");
            let mut x = super::acts::PointToVelocity::default();
            super::spawn::load_values(x.fields(), v).expect("PointToVelocityBehaviour's fields");
            x.physical = p;
            self.point_to_velocities.push(x);
            self.behaviours.add(0, super::Behaviour::PointToVelocity(self.point_to_velocities.len() - 1));
        }
        self.assets = Some(db);
    }

    /// State a probe may set that the prefab doesn't give: the pools' timers and the renderers' sorting.
    pub(crate) fn load_recorded_sorting_and_pools(&mut self, s: &Snapshot) {
        // The pools' inactivity timers as recorded (a probe may have set them to anything).
        let bits = |v: &str| f32::from_bits(u32::from_str_radix(v, 16).unwrap_or(0));
        if let Some(t) = s.of("TracerPool").next().and_then(|r| r.get("timer")) {
            self.tracer_pool.inactivityTimer = bits(t);
        }
        for r in s.of("PoolGeneratorPool") {
            if let (Some(t), Some(p)) = (r.get("timer"), self.pool_named(&r.name)) {
                p.inactivityTimer = bits(t);
            }
        }
        // `sort=<sortingLayerID>,<sortingOrder>`: the renderer's sorting as recorded (an Instantiate'd prefab keeps
        // the prefab's layer; the catalog spawn and the layer editor change it).
        for r in s.of("PhysicalBehaviour") {
            let Some((l, o)) = r.get("sort").and_then(|v| v.split_once(',')) else { continue };
            if let (Some(p), Ok(l), Ok(o)) = (self.physicals.iter().position(|x| x.id == r.id), l.parse::<i32>(), o.parse::<i32>()) {
                if self.physicals[p].sprite.is_some() {
                    self.serialised_layer_set_sorting_for(p, l, o);
                } else {
                    // A human limb: its SpriteRenderer is drawn with the skin, not as `sprite`.
                    self.physicals[p].sprite_layer = l;
                    self.physicals[p].bare_order = o as i16;
                }
            }
        }
    }

    /// InputSystem's controls and the cursor from a recording's snapshot header (`mouse=` and `drag=D,H,U`).
    pub fn load_input(&mut self, s: &Snapshot) {
        if let Some((_, m)) = s.snap.iter().find(|(k, _)| k == "mouse") {
            self.input.mouse = floats(m);
        }
        // `screen=<Input.mousePosition x,y>,<camera x,y>,<orthographicSize>,<pixelWidth>,<pixelHeight>`: the main
        // camera's orthographic view, one pixel being 2 * orthographicSize / pixelHeight world units.
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "screen") {
            let w: Vec<&str> = v.split(',').collect();
            let fw = |i: usize| f32::from_bits(u32::from_str_radix(w[i], 16).unwrap());
            let (pw, ph): (f32, f32) = (w[5].parse().unwrap(), w[6].parse().unwrap());
            let scale = 2.0 * fw(4) / ph;
            self.input.screen = Some(super::tool::Screen { mouse: [fw(0), fw(1)], origin: [fw(2) - pw * 0.5 * scale, fw(3) - ph * 0.5 * scale], scale, view: Some([fw(2), fw(3), fw(4), pw, ph]) });
            // Then Input.GetMouseButtonDown/GetMouseButton/GetMouseButtonUp(0) as 3 digits (recordings since PPG_SCREEN_MOUSE).
            if let Some(m) = w.get(7).map(|m| m.as_bytes()) {
                self.input.keys.insert("Mouse0".into(), super::tool::Control { down: m[0] == b'1', held: m[1] == b'1', up: m[2] == b'1' });
            }
        }
        // `view=<camera x,y>,<orthographicSize>,<aspect>`: the main camera's culling rectangle.
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "view") {
            let [x, y, size, aspect]: [f32; 4] = words(v).into_iter().map(f32::from_bits).collect::<Vec<_>>().try_into().unwrap();
            let w = size * aspect;
            self.input.view = Some(([x - w, y - size], [x + w, y + size]));
        }
        // `uilock=0|1`: Global.main.UILock as the game read it (fuzz recordings from b8 on).
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "uilock") {
            self.input.ui_lock = Some(v == "1");
        }
        // `uiblock=<paused menu>,<DialogBox.IsAnyDialogboxOpen>,<Global.ActiveUiBlock>` (with the tool words).
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "uiblock") {
            let b: Vec<bool> = v.split(',').map(|x| x == "1").collect();
            self.input.ui_block = b[1] || b[2];
        }
        if let Some((_, d)) = s.snap.iter().find(|(k, _)| k == "drag") {
            let b: Vec<bool> = d.split(',').map(|x| x == "1").collect();
            self.input.controls.insert("drag".into(), super::tool::Control { down: b[0], held: b[1], up: b[2] });
        }
        // `rot=left+fast=D,H,U`: the recorder's held rotate keys.
        if let Some((_, r)) = s.snap.iter().find(|(k, _)| k == "rot") {
            let (keys, state) = r.split_once('=').expect("rot=<keys>=D,H,U");
            let b: Vec<bool> = state.split(',').map(|x| x == "1").collect();
            for key in keys.split('+') {
                self.input.controls.insert(key.into(), super::tool::Control { down: b[0], held: b[1], up: b[2] });
            }
        }
        // `key=<control>=D,H,U`: the recorder's pressed keys (PPG_KEY, PPG_HOVER), one word each.
        for (_, r) in s.snap.iter().filter(|(k, _)| k == "key") {
            let (key, state) = r.split_once('=').expect("key=<name>=D,H,U");
            let b: Vec<bool> = state.split(',').map(|x| x == "1").collect();
            self.input.controls.insert(key.into(), super::tool::Control { down: b[0], held: b[1], up: b[2] });
        }
        // `pooltimer=<pool name>,<bits>`: the recorder set that PoolGenerator pool's inactivityTimer (PPG_POOL_TIMER).
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "pooltimer") {
            let (name, bits) = v.split_once(',').expect("pooltimer=<pool name>,<bits>");
            let t = f32::from_bits(u32::from_str_radix(bits, 16).unwrap());
            if name == "BulletTracerPool" {
                self.tracer_pool.inactivityTimer = t;
            } else {
                self.pool_named(name).unwrap_or_else(|| panic!("pooltimer: no pool {name}")).inactivityTimer = t;
            }
        }
        // `tracermax=<n>`: the recorder set the BulletTracerPool's MaxPoolSize (PPG_TRACER_MAX).
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "tracermax") {
            self.tracer_pool.MaxPoolSize = str::parse::<usize>(v).expect("tracermax=<n>");
        }
        if let Some((_, tool)) = s.snap.iter().find(|(k, _)| k == "tool") {
            if tool != "DragTool" && self.power.is_none() && self.pin_tool.is_none() && !self.wire_tool.chosen && !self.link_tool.chosen && !self.belt_tool.chosen && !self.entity_tool_chosen() {
                // A replay starts from the snapshot; the tool's Awake loads its effect.
                let game = crate::game_assets::find_game(None).expect("the game's data");
                self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
                // batch3 r7: a chosen PyroPowerTool's pyroEntity is the recorded entity (its Awake ran in the game), so SetTool
                // does not instantiate another.
                if tool == "PyroPowerTool" && self.pyro_tool.is_none()
                    && let Some(id) = s.of(tool).next().and_then(|w| w.get("pyroEntity")).and_then(reference)
                    && let Some(entity) = self.pyro_entities.iter().position(|x| x.id == id)
                {
                    self.pyro_tool = Some(super::pyro_tool::PyroTool { entity });
                    self.behaviours.add(0, super::Behaviour::PyroTool);
                }
                self.set_tool_by_type_name(tool);
                // SetTool cleared ActiveSingleSelected: the recorded one (the tool's own record) back.
                if let Some(id) = s.of(tool).next().and_then(|w| w.get("ActiveSingleSelected")).and_then(reference) {
                    let sel = self.physicals.iter().position(|p| p.id == id);
                    self.tool.as_mut().unwrap().ActiveSingleSelected = sel;
                }
                // The wire tool's own state (its record: the class `tool` names): OnHold/OnDeselect read it.
                if self.wire_tool.chosen && let Some(w) = s.of(tool).next() {
                    if let (Some(p), Some(o), Some(h)) = (w.get("startPos"), w.get("startOffset"), w.get("hasDragged")) {
                        self.wire_tool.startPos = floats(p);
                        self.wire_tool.startOffset = floats(o);
                        self.wire_tool.hasDragged = h == "1";
                    }
                    if let Some(id) = w.get("startBody").and_then(reference) {
                        self.wire_tool.startBody = self.physicals.iter().position(|p| p.id == id);
                    }
                    // The tool's WireWidth (Awake's, or a fuzzed case's): OnJointCreate copies it to the wire.
                    self.wire_tool.WireWidth = w.get("WireWidth").map(f);
                    self.wire_tool.AllowNothingConnection = w.get("AllowNothingConnection").map(|v| v == "1");
                }
                // A link tool's the same (batch2 r2: FnTiny makes the seeded tool CurrentTool).
                if self.link_tool.chosen && let Some(w) = s.of(tool).next() {
                    if let (Some(p), Some(o), Some(h)) = (w.get("startPos"), w.get("startOffset"), w.get("hasDragged")) {
                        self.link_tool.startPos = floats(p);
                        self.link_tool.startOffset = floats(o);
                        self.link_tool.hasDragged = h == "1";
                    }
                }
                // gapB r9: a belt tool's the same (FnTiny.GapB9Tool chooses the self and presses OnSelect first).
                if self.belt_tool.chosen && let Some(w) = s.of(tool).next()
                    && let (Some(p), Some(o), Some(h)) = (w.get("startPos"), w.get("startOffset"), w.get("hasDragged"))
                {
                    self.belt_tool.startPos = floats(p);
                    self.belt_tool.startOffset = floats(o);
                    self.belt_tool.hasDragged = h == "1";
                }
            }
        }
        // gapC r17: the PhaseLinkTool's WireWidth/WireColor (its Awake's, or a fuzzed case's), when the component is recorded.
        // gapC r18: after the tool block: SetTool's link_tool_choose runs PhaseLinkTool.Awake, which overwrote a fuzzed case's values.
        if let Some(w) = s.of("PhaseLinkTool").next() {
            if let Some(v) = w.get("WireWidth") { self.link_tool.WireWidth = f(v) }
            if let Some(v) = w.get("WireColor") { self.link_tool.WireColor = floats(v) }
        }
        // `LightningToolState active=` (fnlive): a LightningTool an earlier scenario added stays on the ToolController and
        // its Update runs every frame.
        if let Some(r) = s.of("LightningToolState").next() && self.lightning_tool.is_none() {
            let game = crate::game_assets::find_game(None).expect("the game's data");
            self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
            let chosen = s.snap.iter().any(|(k, v)| k == "tool" && v == "LightningTool");
            self.lightning_tool_load(r.get("active") == Some("1"), chosen);
        }
    }

    /// The renderer state scripts read: each bleeding particle's ParticleSystemRenderer.isVisible (dump16 on).
    pub fn load_render_state(&mut self, s: &Snapshot) {
        for (b, r) in bleeding_rows(self).into_iter().zip(s.of("BleedingParticleBehaviour")) {
            if let Some(v) = r.get("visible") {
                self.bleeding[b].visible = v == "1";
            }
        }
        // The ambience emitter's AudioSource.isPlaying: the mixer ends a clip on the wall clock (between frames, not on a
        // recorded time), so the replay reads it as an input.
        for (a, r) in self.ambient_audio.iter_mut().zip(ambient_records(s)) {
            a.isPlaying = r.get("playing") == Some("1");
        }
    }

    fn load_time(&mut self, s: &Snapshot) {
        self.time = Time {
            time: f(s.snap("time")),
            timeAsDouble: s.of("Time").next().and_then(|r| r.get("timeAsDouble")).map_or(f(s.snap("time")) as f64, |w| f64::from_bits(u64::from_str_radix(w, 16).unwrap())),
            fixedTime: f(s.snap("fixedTime")),
            fixedTimeAsDouble: s.of("Time").next().and_then(|r| r.get("fixedTimeAsDouble")).map_or(f(s.snap("fixedTime")) as f64, |w| f64::from_bits(u64::from_str_radix(w, 16).unwrap())),
            deltaTime: f(s.snap("dt")),
            fixedDeltaTime: f(s.snap("fixedDt")),
            maximumDeltaTime: f(s.snap("maxDt")),
            timeScale: f(s.snap("timeScale")),
            frameCount: s.snap("frame").parse().unwrap(),
            unscaledDeltaTime: f(s.snap("unscaledDt")),
            // smoothDt= since fnbb12 (older recordings: the frame's dt).
            smoothDeltaTime: f(s.snap.iter().find(|(k, _)| k == "smoothDt").map_or(s.snap("dt"), |(_, v)| v.as_str())),
            smoothCount: 1.0,
            // The recordings lack the frame's time during FixedUpdate: its own.
            dynamicTimeAsDouble: s.of("Time").next().and_then(|r| r.get("timeAsDouble")).map_or(f(s.snap("time")) as f64, |w| f64::from_bits(u64::from_str_radix(w, 16).unwrap())),
            realtimeSinceStartup: 0.0,
            unscaledTimeAsDouble: 0.0,
            fixedUnscaledTimeAsDouble: 0.0,
            fixedUnscaledDeltaTime: 0.0,
        };
        // Older recordings lack it: the Default map prefab sets Ambient_temperature 20.
        self.AmbientTemperature = s.snap.iter().find(|(k, _)| k == "ambient").map_or(20.0, |(_, v)| f(v));
        // Older recordings lack it: the Default map's BoundingBoxBehaviour.
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "bounds") {
            let b = words(v).iter().map(|&w| f32::from_bits(w)).collect::<Vec<_>>();
            self.BoundingBox = super::Bounds { center: [b[0], b[1]], extents: [b[2], b[3]] };
        }
        let w = words(s.snap("random"));
        self.random = Random { state: [w[0], w[1], w[2], w[3]] };
        // ContinuousActivationBehaviour's oscillation, accumulator and cleanupTimer (older recordings lack it: a fresh one).
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "psrand") {
            let w = words(v);
            crate::particles::set_manager_rand(self, [w[0], w[1], w[2], w[3]]);
        }
        if let Some((_, v)) = s.snap.iter().find(|(k, _)| k == "continuous") {
            let w = words(v);
            self.continuous.oscillation = w[0];
            self.continuous.signals.timeStepAccumulator = f32::from_bits(w[1]);
            self.signals.cleanup_timer = f32::from_bits(w[2]);
        }
    }

    /// EarlyUpdate.ScriptRunDelayedStartupFrame (DelayedCallManager::Update 180634dd0, mode 4), once a frame. Start is
    /// a DelayedCallManager call (MonoBehaviour 1808b6335: mode 7, wake -10 s) and nothing else runs it: the fixed steps
    /// have no Start pass before their FixedUpdates (BaseBehaviourManager::CommonUpdate 180630d70 only calls them),
    /// and Update.ScriptRunDelayedDynamicFrameRate comes after ScriptRunBehaviourUpdate. A call added during a pass
    /// waits for the next one (180634eb0).
    fn startup_pass(&mut self) {
        if self.startup_pass_frame != Some(self.time.frameCount) {
            self.startup_pass_frame = Some(self.time.frameCount);
            self.run_pending_starts();
        }
    }

    /// The FixedUpdate scripts between CameraShakeBehaviour (FS) and CameraControlBehaviour (FM).
    pub fn fixed_update_scripts(&mut self) {
        // BaseBehaviourManager::CommonUpdate (180630d70) integrates its lists once, before the first FixedUpdate: a
        // behaviour made during the pass (a bleeding particle's splat in a limb's ManagedFixedUpdate) waits a step.
        let list = self.behaviours.list(super::Phase::Fixed).clone();
        self.camera_shake_fixed_update();
        self.startup_pass();
        crate::particles::shockwaves_message(self, crate::particles::ShockwavePhase::Start);
        self.tool_fixed_update();
        self.powers_fixed_update();
        self.ambient_grid_fixed_update();
        // BehaviourManager`1.FixedUpdate: each item of GetCollection that is not null and ShouldUpdate gets its ManagedFixedUpdate.
        for p in 0..self.physicals.len() {
            if self.physical_should_update(p) && self.physicals[p].in_world {
                self.physical_fixed_update(p);
            }
        }
        for l in 0..self.limbs.len() {
            if self.limbs[l].enabled && self.limbs[l].started {
                self.limb_fixed_update(l);
            }
        }
        self.water_manager_fixed_update();
        self.run_behaviour_list(super::Phase::Fixed, list);
        crate::particles::shockwaves_message(self, crate::particles::ShockwavePhase::Fixed);
        // Execution order 1500, the last FixedUpdate.
        self.camera_control_fixed_update();
    }

    /// One pass of a behaviour manager over the list as it stood at the start (IntegrateLists); a behaviour removed
    /// during the pass is skipped (its SafeIterator).
    pub fn run_behaviours(&mut self, phase: super::Phase) {
        let list = self.behaviours.list(phase).clone();
        self.run_behaviour_list(phase, list);
    }

    /// The Update loop over the behaviours that existed when the frame's scripts began: one added during it (a paste by
    /// ToolControllerBehaviour.Update) is not visited by this iteration, and its Start is still due.
    fn run_behaviour_list(&mut self, phase: super::Phase, list: Vec<(i32, super::Behaviour)>) {
        for entry in list {
            if !self.behaviours.list(phase).contains(&entry) {
                continue;
            }
            // A managed exception ends this behaviour's callback only (Debug.LogException).
            self.unity_callback(|g| g.run_behaviour(phase, entry.1));
        }
    }

    /// One behaviour's callback of the `phase` manager.
    fn run_behaviour(&mut self, phase: super::Phase, b: super::Behaviour) {
        use super::{Behaviour as B, Phase};
        // Unity's own messages skip a deactivated (disintegrated) limb's components; the managed updates only
        // check enabled.
        match (phase, b) {
            (_, B::Circulation(l) | B::Skin(l) | B::GoreString(l) | B::GForce(l)) if !self.limb_active(l) => {}
            (_, B::Grip(g)) if !self.physicals[self.grips[g].PhysicalBehaviour].active => {}
            (_, B::BloodWire(w)) if { let p = &self.physicals[self.blood_wires[w].physical]; !p.active || p.destroyed } => {}
            (_, B::Pin(p)) if !self.pin_host_active(p) => {}
            (Phase::Fixed, B::Pin(p)) => self.pin_fixed_update(p),
            (Phase::Update, B::Pin(p)) => self.pin_update(p),
            (Phase::Fixed, B::BloodWire(w)) => self.blood_wire_fixed_update(w),
            (Phase::Update, B::BloodWire(w)) => self.blood_wire_update(w),
            // WireBehaviour.FixedUpdate: Tick, empty for the cable.
            (Phase::Fixed, B::RootWire(r)) => self.root_wire_fixed_update(r),
            (Phase::Update, B::RootWire(r)) => self.root_wire_update(r),
            (_, B::Machinery(m)) if { let p = &self.physicals[self.machinery[m].PhysicalBehaviour]; !p.active || p.destroyed } => {}
            (Phase::Fixed, B::Machinery(m)) => self.machinery_fixed_update(m),
            (Phase::Fixed, B::Lava(l)) => self.lava_fixed_update(l),
            (Phase::Fixed, B::HeatRect(h)) => self.heat_rect_fixed_update(h),
            (Phase::Update, B::Spawner(s)) => self.spawner_update(s),
            (Phase::Update, B::Machinery(m)) => self.machinery_update(m),
            (Phase::Update, B::Destroyable(x)) => self.destroyable_update(x),
            (Phase::Fixed, B::Explosive(e)) => self.explosive_fixed_update(e),
            (Phase::Fixed, B::PointToVelocity(i)) => self.point_to_velocity_fixed_update(i),
            (Phase::Fixed, B::ObstructionEmitter(i)) if self.obstruction_emitters[i].enabled => self.obstruction_emitter_fixed_update(i),
            (Phase::Fixed, B::Inverter(i)) if self.inverters[i].enabled => self.inverter_fixed_update(i),
            (Phase::Fixed, B::AndroidSpark(i)) => self.android_spark_fixed_update(i),
            (Phase::Update, B::Car(i)) => self.car_update(i),
            (_, B::Tyre(t)) if { let p = &self.physicals[self.tyres[t].Phys]; !p.active || p.destroyed } => {}
            (Phase::Fixed, B::Tyre(t)) => self.tyre_fixed_update(t),
            (Phase::Update, B::Tyre(t)) => self.tyre_update(t),
            // Update is the sag of a deflated tyre's material (rendering).
            (Phase::Update, B::Tyre(_)) => {}
            (Phase::Update, B::Flask(f)) => self.flask_update(f),
            (Phase::Fixed, B::LiquidContainer(c)) => self.liquid_container_fixed_update(c),
            // LiquidContainerController.Update: the sloshing volume and the particle positions the shader reads.
            (Phase::Update, B::LiquidContainer(c)) => self.liquid_container_update(c),
            (Phase::Fixed, B::Glowtube(g)) => self.glowtube_fixed_update(g),
            (Phase::Update, B::Glowtube(g)) => self.glowtube_update(g),
            (Phase::Fixed, B::CathodeLight(i)) => self.cathode_light_fixed_update(i),
            (Phase::Update, B::CathodeLight(i)) => self.cathode_light_update(i),
            (Phase::Update, B::Led(i)) => self.led_update(i),
            (Phase::Fixed, B::Detector(i)) => self.detector_fixed_update(i),
            (Phase::Update, B::Detector(i)) => self.detector_update(i),
            (Phase::Update, B::LaserReceiver(i)) => self.laser_receiver_update(i),
            (Phase::Update, B::KeyTrigger(i)) => self.key_trigger_update(i),
            (Phase::Fixed, B::Fireworks(i)) => self.fireworks_fixed_update(i),
            (Phase::Update, B::Fireworks(i)) => self.fireworks_update(i),
            (Phase::Fixed, B::Generator(i)) => self.generator_fixed_update(i),
            (Phase::Update, B::Generator(i)) => self.generator_update(i),
            (Phase::Update, B::Floodlight(i)) => self.floodlight_update(i),
            (Phase::Fixed, B::HeatingElement(i)) => self.heating_element_fixed_update(i),
            (Phase::Fixed, B::RayGun(i)) => self.ray_gun_fixed_update(i),
            (Phase::Fixed, B::Bulb(i)) => self.bulb_fixed_update(i),
            (Phase::Update, B::Bulb(i)) => self.bulb_update(i),
            (Phase::Update, B::Television(t)) => self.television_update(t),
            (Phase::Fixed, B::Thrusterbed(i)) => self.thrusterbed_fixed_update(i),
            (Phase::Fixed, B::PointLiquid(i)) => self.point_liquid_fixed_update(i),
            (Phase::Fixed, B::Piston(i)) => self.piston_fixed_update(i),
            (Phase::Fixed, B::MapLift(i)) => self.map_lift_fixed_update(i),
            (Phase::Update, B::ReactorA5Display(i)) => self.reactor_a5_display_update(i),
            (Phase::Fixed, B::JointMotorSound(i)) => self.joint_motor_sound_fixed_update(i),
            (Phase::Update, B::Flamethrower(i)) => self.flamethrower_update(i),
            (Phase::Fixed, B::Rocket(i)) => self.rocket_fixed_update(i),
            (Phase::Update, B::Rocket(i)) => self.rocket_update(i),
            (Phase::Update, B::BreakJointOnBurn(i)) => self.break_joint_on_burn_update(i),
            (Phase::Fixed, B::SoundOnShake(i)) => self.sound_on_shake_fixed_update(i),
            (Phase::Fixed, B::EmitOnShake(i)) => self.emit_on_shake_fixed_update(i),
            (Phase::Update, B::SerialisationHelper(i)) => self.serialisation_helper_update(i),
            (Phase::Fixed, B::GorseDrooling(i)) => self.gorse_drooling_fixed_update(i),
            (Phase::Update, B::GorseEye(i)) => self.gorse_eye_update(i),
            (Phase::Fixed, B::Airfoil(i)) => self.airfoil_fixed_update(i),
            (Phase::Update, B::Stunner(i)) => self.stunner_update(i),
            (Phase::Update, B::Blaster(i)) => self.blaster_update(i),
            (Phase::Update, B::BeamRepeater(i)) => self.beam_repeater_update(i),
            (Phase::Update, B::AndroidLaser(i)) => self.android_laser_update(i),
            (Phase::Fixed, B::AndroidLaser(i)) => self.android_laser_fixed_update(i),
            (Phase::Update, B::WeatherLightning(i)) => self.weather_lightning_update(i),
            (Phase::Update, B::Blasterbolt(i)) => self.blasterbolt_update(i),
            (Phase::Fixed, B::Torch(t)) => self.torch_fixed_update(t),
            (Phase::Update, B::Button(b)) => self.button_update(b),
            (Phase::Fixed, B::Button(b)) => self.button_fixed_update(b),
            (Phase::Fixed, B::BatteryAttachment(a)) => self.battery_attachment_fixed_update(a),
            (Phase::Update, B::Beamformer(i)) => self.beamformer_update(i),
            (Phase::Update, B::Railgun(i)) => self.railgun_update(i),
            (Phase::Update, B::Balloon(i)) => self.balloon_update(i),
            (Phase::Update, B::Metronome(i)) => self.metronome_update(i),
            (Phase::Update, B::DeterminedAnomaly(i)) => self.determined_anomaly_update(i),
            (Phase::Update, B::PhysicsGun(i)) => self.physics_gun_update(i),
            (Phase::Update, B::SmoothLine(i)) => self.smooth_line_update(i),
            // Update only fades the glow (presentation).
            (Phase::Update, B::ActivatorElectrode(e)) => self.activator_electrode_update(e),
            (Phase::Update, B::Visual(_, v)) => self.visual_update(v),
            (Phase::Fixed, B::Thermometer(i)) => self.thermometer_fixed_update(i),
            (Phase::Update, B::InfraredThermometer(i)) => self.infrared_update(i),
            (Phase::Fixed, B::Syringe(i)) => self.syringe_fixed_update(i),
            (Phase::Fixed, B::JetEngine(i)) => self.jet_engine_fixed_update(i),
            (Phase::Fixed, B::Chainsaw(i)) => self.chainsaw_fixed_update(i),
            (Phase::Fixed, B::CeilingTurret(i)) => self.ceiling_turret_fixed_update(i),
            (Phase::Fixed, B::TemperatureTransfer(i)) => self.temperature_transfer_fixed_update(i),
            (Phase::Fixed, B::LaserTurret(i)) => self.laser_turret_fixed_update(i),
            (Phase::Fixed, B::SentryTurret(i)) => self.sentry_turret_fixed_update(i),
            (Phase::Update, B::SentryTurret(i)) => self.sentry_turret_update(i),
            (Phase::Fixed, B::SentryTurretGun(i)) => self.sentry_gun_fixed_update(i),
            (Phase::Update, B::SentryTurretGun(i)) => self.sentry_gun_update(i),
            (Phase::Update, B::IonBolt(i)) => self.ion_bolt_update(i),
            (Phase::Update, B::RandomEvent(i)) => self.random_event_update(i),
            (Phase::Fixed, B::Decimator(i)) => self.decimator_fixed_update(i),
            (Phase::Fixed, B::Drone(i)) => self.drone_fixed_update(i),
            (Phase::Fixed, B::BlackHole(i)) => self.black_hole_fixed_update(i),
            (Phase::Fixed, B::ImmobilityField(i)) => self.immobility_field_fixed_update(i),
            (Phase::Update, B::Forcefield(i)) => self.forcefield_update(i),
            (Phase::Fixed, B::LightningGun(i)) => self.lightning_gun_fixed_update(i),
            (Phase::Update, B::LightningGun(i)) => self.lightning_gun_update(i),
            (Phase::Fixed, B::Javelin(i)) => self.javelin_fixed_update(i),
            (Phase::Fixed, B::JavelinMissile(i)) => self.javelin_missile_fixed_update(i),
            // JavelinMissileBehaviour.Update: the fins' scale.
            (Phase::Update, B::JavelinMissile(_)) => {}
            (Phase::Fixed, B::TeslaCoil(i)) => self.tesla_coil_fixed_update(i),
            (Phase::Update, B::TeslaBolt(k)) => self.tesla_bolt_update(k),
            (Phase::Update, B::WormStaff(i)) => self.worm_staff_update(i),
            (Phase::Update, B::AtomExplosion(i)) => self.atom_explosion_update(i),
            (Phase::Update, B::Chainsaw(i)) => self.chainsaw_update(i),
            (Phase::Update, B::CeilingTurret(i)) => self.ceiling_turret_update(i),
            (Phase::Fixed, B::EnergySword(i)) => self.energy_sword_fixed_update(i),
            (Phase::Update, B::EnergySword(i)) => self.energy_sword_update(i),
            (Phase::Update, B::EnergySwordBlur(i)) => self.energy_blur_update(i),
            (Phase::Update, B::Syringe(i)) => self.syringe_update(i),
            (Phase::Update, B::Piston(i)) => self.piston_update(i),
            (Phase::Update, B::RayGun(i)) => self.ray_gun_update(i),
            (Phase::Fixed, B::Sticky(i)) => self.sticky_grenade_fixed_update(i),
            (Phase::Fixed, B::TruckDoor(t)) => self.truck_door_fixed_update(t, self.truck_doors[t].0),
            (Phase::Update, B::DeleteAfter(i)) => self.delete_after_update(i),
            (Phase::Update, B::LineRendererTargets(i)) => self.line_renderer_targets_update(i),
            (Phase::Update, B::Poison(i)) => self.poison_update(i),
            (Phase::Update, B::InfiniteLiquid(i)) => self.infinite_liquid_update(i),
            (Phase::Update, B::ReturnToPool(i)) => self.return_to_pool_update(i),
            (Phase::Update, B::SlightMovement(i)) => self.slight_movement_update(i),
            (Phase::Update, B::Needle(i)) => self.needle_update(i),
            (Phase::Fixed, B::Needle(i)) => self.needle_fixed_update(i),
            (Phase::Fixed, B::Raygun(i)) => self.raygun_fixed_update(i),
            (Phase::Update, B::WhOpening(i)) => self.wh_opening_update(i),
            (Phase::Update, B::Wh(i)) => self.wh_update(i),
            (Phase::Update, B::IgnoreParentSize(i)) => self.ignore_parent_size_update(i),
            (Phase::Fixed, B::WhOpening(i)) => self.wh_opening_fixed_update(i),
            (Phase::Fixed, B::CustomFixedConstrained(i)) => self.custom_fixed_fixed_update(i),
            (Phase::Update, B::MachineGunProjectile(i)) => self.machine_gun_projectile_update(i),
            (Phase::Fixed, B::Plane(i)) => self.plane_fixed_update(i),
            (Phase::Fixed, B::Wing(i)) => self.wing_fixed_update(i),
            (Phase::Update, B::Shockwave(i)) => self.shockwave_update(i),
            (Phase::Fixed, B::GlassShard(i)) => self.glass_shard_fixed_update(i),
            (Phase::Fixed, B::PressureGrid(i)) => self.pressure_grid_fixed_update(i),
            (Phase::Fixed, B::RadiationGrid(i)) => self.radiation_grid_fixed_update(i),
            (Phase::Update, B::DisableTrigger(i)) => self.disable_trigger_update(i),
            (_, B::FireMap(f)) if { let p = &self.physicals[self.fire_maps[f].physical]; !p.active || p.destroyed } => {}
            (Phase::Fixed, B::FireMap(f)) => self.fire_map_fixed_update(f),
            (Phase::Update, B::Firearm(f)) => self.firearm_update(f),
            (Phase::Fixed, B::Launcher(l)) => self.launcher_fixed_update(l),
            (Phase::Update, B::Launcher(l)) => self.launcher_update(l),
            (Phase::Update, B::MachineGun(m)) => self.machine_gun_update(m),
            (Phase::Update, B::Minigun(m)) => self.minigun_update(m),
            // FixedUpdate only decays the barrel's spin (presentation).
            (Phase::Fixed, B::Minigun(m)) => self.minigun_fixed_update(m),
            (Phase::Update, B::Accelerator(a)) => self.accelerator_update(a),
            (Phase::Update, B::AcceleratorBolt(b)) => self.accelerator_bolt_update(b),
            (Phase::Fixed, B::AcceleratorBolt(b)) => self.accelerator_bolt_fixed_update(b),
            (Phase::Update, B::ArchelixBolt(b)) => self.archelix_bolt_update(b),
            (Phase::Update, B::ObjectPool(k)) => self.object_pool_update(k),
            (Phase::Fixed, B::ArchelixBolt(b)) => self.archelix_bolt_fixed_update(b),
            (Phase::Update, B::UniversalBell(b)) => self.universal_bell_update(b),
            // ValveBehaviour.Update: the container update (valve.rs).
            (Phase::Update, B::Valve(v)) => self.valve_update(v),
            (Phase::Update, B::LiquidCanister(c)) => self.liquid_canister_update(c),
            (Phase::Update, B::LiquidFilter(f)) => self.liquid_filter_update(f),
            (Phase::Update, B::Liquidentifier(l)) => self.liquidentifier_update(l),
            (Phase::Fixed, B::Centrifuge(c)) => self.centrifuge_fixed_update(c),
            (Phase::Update, B::Centrifuge(c)) => self.centrifuge_update(c),
            (Phase::Fixed, B::Vibration(v)) => self.vibration_fixed_update(v),
            (Phase::Update, B::OnlyExistWith(i)) => self.only_exist_with_update(i),
            (Phase::Update, B::OffsetGrid(i)) => self.offset_grid_update(i),
            (Phase::Update, B::WindController(i)) => self.wind_controller_update(i),
            (Phase::Update, B::Egg(i)) => self.egg_update(i),
            (Phase::Fixed, B::LiquidPump(l)) => self.liquid_pump_fixed_update(l),
            (Phase::Update, B::LiquidPump(l)) => self.liquid_pump_update(l),
            (Phase::Update, B::LiquidDuplicator(l)) => self.liquid_duplicator_update(l),
            (Phase::Fixed, B::LiquidHole(h)) => self.liquid_hole_fixed_update(h),
            (Phase::Update, B::HeartMonitor(h)) => self.heart_monitor_update(h),
            (Phase::Update, B::PowerGauge(g)) => self.power_gauge_update(g),
            (Phase::Update, B::Excalibur(e)) => self.excalibur_update(e),
            (Phase::Fixed, B::SharpOnAllSides(s)) => self.sharp_on_all_sides_fixed_update(s),
            (Phase::Update, B::BloodTank(t)) => self.blood_tank_update(t),
            (Phase::Fixed, B::LiquidOutlet(l)) => self.liquid_outlet_fixed_update(l),
            (Phase::Update, B::LiquidOutlet(l)) => self.liquid_outlet_update(l),
            // LagboxBehaviour.Update (inherited): Knob.localEulerAngles = (0, 0, Clamp(t * 180, 0, 180)); the knob's pose is saved.
            (Phase::Update, B::TimedGate(g)) => {
                if self.timed_gates[g].knob_null {
                    super::menu::npe();
                }
                if let Some(knob) = self.timed_gates[g].knob {
                    let q = super::menu::euler_z_rotation(super::mathf::clamp(self.timed_gates[g].t * 180.0, 0.0, 180.0));
                    self.with_component_instance(knob, |_, inst, o| inst.set_local_rotation(o, q));
                }
            }
            (Phase::Update, B::Lagbox(g)) => self.lagbox_update(g),
            (Phase::Update, B::Gate(i)) => self.gate_update(i),
            (Phase::Update, B::LiquidLevelGate(i)) => self.liquid_level_gate_update(i),
            // LiquidLevelGateBehaviour.FixedUpdate: throw new NotImplementedException() (logged each step).
            (Phase::Fixed, B::LiquidLevelGate(_)) => super::menu::throw("NotImplementedException: The method or operation is not implemented."),
            (Phase::Update, B::ActivationTransformer(i)) => self.activation_transformer_update(i),
            (Phase::Fixed, B::HoverThruster(i)) => self.hover_thruster_fixed_update(i),
            (Phase::Fixed, B::Hovercar(i)) => self.hovercar_fixed_update(i),
            (Phase::Update, B::Hovercar(i)) => self.hovercar_update(i),
            (Phase::Fixed, B::Winch(i)) => self.winch_fixed_update(i),
            (Phase::Fixed, B::Conveyor(i)) => self.conveyor_fixed_update(i),
            (Phase::Fixed, B::ReactorCore(i)) => self.reactor_core_fixed_update(i),
            (Phase::Fixed, B::ReactorConduit(i)) => self.reactor_conduit_fixed_update(i),
            // BloodContainer.Update, which ReactorConduitBehaviour does not override.
            (Phase::Update, B::ReactorConduit(i)) => self.blood_container_update(super::container::Owner::ReactorConduit(i)),
            (Phase::Fixed, B::ReactorMeltdown(i)) => self.reactor_meltdown_fixed_update(i),
            (Phase::Update, B::DoAtTime(i)) => self.do_at_time_update(i),
            (Phase::Fixed, B::IndustrialLift(i)) => self.industrial_lift_fixed_update(i),
            (Phase::Fixed, B::AcidPool(i)) => self.acid_pool_fixed_update(i),
            (Phase::Fixed, B::Vacuum(i)) => self.vacuum_fixed_update(i),
            (Phase::Fixed, B::PointAt(i)) => self.point_at_fixed_update(i),
            (Phase::Fixed, B::Ep(i)) => self.ep_fixed_update(i),
            (Phase::Update, B::Ep(i)) => self.ep_update(i),
            (Phase::Update, B::Winch(i)) => self.winch_update(i),
            (Phase::Fixed, B::Jukebox(i)) => self.jukebox_fixed_update(i),
            (Phase::Update, B::AntennaReceiver(i)) => self.antenna_receiver_update(i),
            (Phase::Fixed, B::AntennaReceiver(i)) => self.antenna_receiver_fixed_update(i),
            (Phase::Update, B::Coupler(i)) => self.coupler_update(i),
            (Phase::Update, B::LightningTool) => self.lightning_tool_update(),
            (Phase::Update, B::LightningEntity(i)) => self.lightning_entity_update(i),
            (Phase::Update, B::PyroTool) => self.pyro_tool_update(),
            (Phase::Update, B::PhaseLink(i)) => self.phase_link_update(i),
            (Phase::Update, B::LinkEnd(i, end)) => self.link_end_update(i, end),
            (Phase::Update, B::Belt(i)) => self.belt_update(i),
            (Phase::Fixed, B::Belt(i)) => self.belt_fixed_update(i),
            (Phase::Fixed, B::PyroEntity(i)) => self.pyro_entity_fixed_update(i),
            (Phase::Update, B::SeismicExplosion(i)) => self.seismic_explosion_update(i),
            (Phase::Fixed, B::SeismicExplosion(i)) => self.seismic_explosion_fixed_update(i),
            (Phase::Update, B::EnergyBolt(i)) => self.energy_bolt_update(i),
            (Phase::Update, B::HoverThruster(i)) => self.hover_thruster_update(i),
            (Phase::Fixed, B::UniversalBell(b)) => self.universal_bell_fixed_update(b),
            (Phase::Fixed, B::Pointer(i)) => self.pointer_fixed_update(i),
            (Phase::Fixed, B::ToggleableMirror(m)) => self.toggleable_mirror_fixed_update(m),
            (Phase::Fixed, B::Translated(super::translated::TBehaviour::FireSoundEmitter(i))) => self.fire_sound_emitter_fixed(i),
            (phase, B::Translated(t)) => self.tr_phase(phase, t),
            (phase, B::V45(v)) => self.v45_phase(phase, v),
            (Phase::Update, B::ThermalDisplay) => self.thermal_display_update(),
            (Phase::Update, B::ResizeHandles) => self.resize_handles_update(),
            (Phase::Fixed, B::Person(p)) => {
                self.person_fixed_update(p);
                // extras: the crab's behaviour (src/extras_hook.rs); returns at once for every game species.
                crate::extras_hook::crab_fixed_update(self, p);
            }
            (Phase::Fixed, B::Circulation(l)) => self.circulation_fixed_update(l),
            (Phase::Fixed, B::Grip(g)) => self.grip_fixed_update(g),
            (Phase::Fixed, B::Splat(s)) => self.splat_fixed_update(s),
            (Phase::Fixed, B::GForce(l)) => self.gforce_fixed_update(l),
            (Phase::Update, B::Person(p)) => self.person_update(p),
            (Phase::Update, B::Skin(l)) => self.skin_update(l),
            (Phase::Update, B::Circulation(l)) => self.blood_container_update(super::container::Owner::Circulation(l)),
            (Phase::Update, B::Grip(g)) => self.grip_update(g),
            (Phase::Update, B::GoreString(l)) => self.gore_string_update(l),
            (Phase::Update, B::Bleeding(b)) => self.bleeding_update(b),
            (Phase::Late, B::Person(p)) => {
                // A behaviour's Start runs right before its first LateUpdate.
                if let Some(i) = self.pending_starts.iter().position(|s| *s == super::misc::PendingStart::Person(p)) {
                    self.pending_starts.remove(i);
                    self.person_start(p);
                }
                self.person_late_update(p);
            }
            (Phase::Late, B::Skin(l)) => self.skin_late_update(l),
            (Phase::Late, B::Laser(l)) => self.laser_late_update(l),
            (phase, b) => unreachable!("{b:?} in the {phase:?} manager"),
        }
    }

    /// The Update and LateUpdate scripts between the Steamworks initialiser's Update (US) and end of frame (UE),
    /// in the recorded call order.
    pub fn update_scripts(&mut self) {
        // A frame without a fixed step: EarlyUpdate.ScriptRunDelayedStartupFrame.
        self.startup_pass();
        crate::particles::shockwaves_message(self, crate::particles::ShockwavePhase::Start);
        let update_list = self.behaviours.list(super::Phase::Update).clone();
        self.camera_shake_update();
        self.camera_listener_update();
        self.fire_loop_update();
        self.water_sound_update();
        // BulletTracerPool.Update (ObjectPoolBehaviour, order 0): a Main scene behaviour, before the spawned guns that request.
        self.tracer_pool_update();
        // UndoControllerBehaviour.Update, then the map's ambience emitter.
        self.undo_controller_update();
        for a in 0..self.ambient_audio.len() {
            self.play_audio_at_random_update(a);
        }
        // ToolWheelBehaviour.Update (level1's order-0 behaviours: after PlayAudioAtRandom, before ToolControllerBehaviour).
        self.tool_wheel_update();
        self.eye_animation_update();
        self.tool_update();
        self.layer_editor_update();
        self.map_editor_update();
        // level1's order-0 behaviours update in load order (UndoControllerBehaviour, WORLD's PlayAudioAtRandom,
        // ToolControllerBehaviour, Managers' GearManagerBehaviour, then ResizeHandles), before the map's and the
        // spawned ones.
        self.gear_manager_update();
        if !self.resize.readded {
            self.resize_handles_update();
        }
        self.centers_update();
        self.map_config_update();
        // DragTool.Update (added to the ToolController in ToolControllerBehaviour.Start, after the scene's behaviours).
        self.drag_tool_update();
        // BehaviourManager`1.Update: each item of GetCollection that is not null and ShouldUpdate gets its ManagedUpdate.
        for p in 0..self.physicals.len() {
            if self.physical_should_update(p) && self.physicals[p].in_world {
                self.physical_update(p);
            }
        }
        for l in 0..self.limbs.len() {
            if self.limbs[l].enabled && self.limbs[l].started {
                self.limb_update(l);
            }
        }
        self.thermal_vision_update();
        self.run_behaviour_list(super::Phase::Update, update_list);
        crate::particles::shockwaves_message(self, crate::particles::ShockwavePhase::Update);
        self.node_controller_update();
        // Global.Update (global.rs).
        self.global_update();
        // Execution order 1050, above every spawnable script's.
        self.catalog_update();
        // CameraControlBehaviour.Update is the last Update; then Update.ScriptRunDelayedDynamicFrameRate: the delayed
        // Starts (wake -10 s, first in time order), then the coroutines.
        self.camera_control_update();
        self.run_pending_starts();
        self.map_light_coroutines();
        self.explosive_coroutines();
        self.reactor_a5_arc_coroutines();
        self.gore_string_delayed_calls(true);
        // PreLateUpdate.ConstraintManagerUpdate comes before PreLateUpdate.ScriptRunBehaviourLateUpdate.
        self.pins_constraint_update();
        self.constraints_update();
        // PreLateUpdate.ParticleSystemBeginUpdateAll, after the constraints and before ScriptRunBehaviourLateUpdate.
        crate::particles::begin_update_all(self);
        // LimbBehaviour.ManagedLateUpdate: empty
        // LateUpdate: ContinuousActivationBehaviour (execution order -10) first; LimbBehaviour.ManagedLateUpdate is empty.
        self.continuous_activation_late_update();
        // PhysicalBehaviourManager.GetCollection: Global.main.PhysicalObjectsInWorld, the `physicals` arena below
        // Global's ManagedLateUpdate walks PhysicalObjectsInWorld, which a spawned PhysicalBehaviour joins in Start.
        // BehaviourManager`1.LateUpdate: each item of GetCollection that is not null and ShouldUpdate gets its ManagedLateUpdate.
        for p in 0..self.physicals.len() {
            if self.physical_should_update(p) && self.physicals[p].in_world {
                self.physical_late_update(p);
            }
        }
        self.run_behaviours(super::Phase::Late);
        self.linefy_late_update();
        self.camera_control_late_update();
        self.rectangle_audio_late_update();
        // Global.LateUpdate
        self.MouseDelta = [self.MousePosition[0] - self.oldMousePos[0], self.MousePosition[1] - self.oldMousePos[1]];
        if self.MouseDelta[0] * self.MouseDelta[0] + self.MouseDelta[1] * self.MouseDelta[1] < 0.01 {
            self.inactivity += self.time.deltaTime;
        } else {
            self.inactivity = 0.0;
        }
        if self.time.frameCount % 2 == 0 {
            self.chunksUpToDate = false;
        }
        // PostLateUpdate.ParticleSystemEndUpdateAll: the stopped systems' stop actions.
        for p in crate::particles::end_update_all(self) {
            self.pooled_effect_stopped(p);
        }
        // The Starts still due after the LateUpdate phase (a Person spawned this frame), before the render.
        self.run_pending_starts();
        // The main camera's render: OnWillRenderObject.
        self.thermal_display_will_render();
    }

    /// The render after LateUpdate: SkinMaterialHandler.OnWillRenderObject of each limb the camera drew, in call
    /// order (the UE snapshot's `rendered`; before dump17 every limb was on screen). bloodColor is render state.
    pub fn render_callbacks(&mut self, s: &Snapshot) -> Result<(), String> {
        // The leaves the camera drew, in call order (`foliage`, instance ids); before it was recorded, every leaf.
        if let Some((_, ids)) = s.snap.iter().find(|(k, _)| k == "foliage") {
            for id in ids.split(';').filter(|n| !n.is_empty()) {
                let id: i32 = id.parse().map_err(|e| format!("foliage {id}: {e}"))?;
                let f = self.foliages.iter().position(|x| x.id == id).ok_or(format!("foliage: no FoliageBehaviour {id}"))?;
                self.foliage_on_will_render(f);
            }
        } else {
            self.foliage_will_render(None);
        }
        let drawn: Vec<usize> = match s.snap.iter().find(|(k, _)| k == "rendered") {
            Some((_, names)) => names.split(';').filter(|n| !n.is_empty()).map(|n| {
                self.limbs.iter().position(|l| self.physicals[l.PhysicalBehaviour].name == n).ok_or(format!("rendered: no limb {n}"))
            }).collect::<Result<_, _>>()?,
            None => (0..self.limbs.len()).filter(|&l| self.limb_active(l)).collect(),
        };
        for l in drawn {
            self.skin_will_render(l);
        }
        self.syringes_will_render();
        self.tyres_will_render();
        self.waters_will_render();
        self.physicals_will_render();
        Ok(())
    }

    /// PPG_ACT=<kind>:<limb>[:<value>]. A recording has no clones, so a Shot goes to the limb's receivers in component order.
    /// A recording only names each ShotImpact: the asset has the prefab.
    fn recorded_shot_impacts(&mut self) {
        for i in 0..self.properties.len() {
            if let Some((None, false)) = self.properties[i].ShotImpact {
                self.recorded_shot_impact(i);
            }
        }
    }

    /// The prefab of properties `i`'s recorded ShotImpact, from the game's PhysicalProperties asset of that name.
    pub(crate) fn recorded_shot_impact(&mut self, i: usize) {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        let db = self.assets.as_mut().unwrap();
        let Some((None, puff)) = self.properties[i].ShotImpact else { return };
        let own = self.properties[i].name.clone();
        // Not in Resources (an item's own properties, e.g. Dynamite): the one PhysicalProperties asset of that name (the
        // recording writes its spaces as underscores).
        let named = |db: &mut crate::assets::db::AssetDb| -> Option<crate::assets::db::ObjRef> {
            let all = db.find_scripts("PhysicalProperties").unwrap_or_else(|e| panic!("finding PhysicalProperties {own}: {e}"));
            let hits: Vec<_> = all.into_iter().filter(|&a| db.mono(a).ok().flatten().is_some_and(|(_, mb, _)| mb.name.replace(' ', "_") == own)).collect();
            assert!(hits.len() <= 1, "{} PhysicalProperties assets named {own}", hits.len());
            hits.first().copied()
        };
        // Bounds, BigMetal and Rod are no asset: these share their ShotImpact.
        let name = format!("PhysicalProperties/{}", match own.as_str() { "Bounds" => "Ceramic", "Syringe" => "Glass", "BigMetal" | "Rod" => "Metal", n => n });
        let asset = db.resource(&name, 114).ok().flatten().or_else(|| named(db)).unwrap_or_else(|| panic!("no {name}"));
        let (_, _, fields) = db.mono(asset).ok().flatten().unwrap_or_else(|| panic!("{name} is no MonoBehaviour"));
        let prefab = fields.get("ShotImpact").and_then(crate::assets::mono::Value::pptr).and_then(|p| db.resolve(asset.file, p));
        self.properties[i].ShotImpact = Some((prefab, puff));
    }

    /// `item`: the PPG_STAB item's name from the header; the recorder's item acts go to Dump.Item, whatever the act's
    /// fourth part (a name with a space, "Sniper Rifle", is cut at it in the snapshot line).
    /// The scene's ResizeHandles, which a recording has none of: loaded once, then hidden by its first Update (no Targets).
    pub(crate) fn recorded_resize_handles(&mut self) -> Result<(), String> {
        if self.resize_handles.is_some() {
            return Ok(());
        }
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let mut db = self.assets.take().unwrap_or_else(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
        self.load_scene_resize_handles(&mut db)?;
        self.resize_handles_update();
        // The handles' layer collides with nothing: the layer collision matrix keeps their colliders out of contacts.
        if self.world.layer_collision_matrix.is_empty() {
            self.world.layer_collision_matrix = Game::load_physics2d_settings(&db)?.layer_collision_matrix;
        }
        self.assets = Some(db);
        Ok(())
    }

    fn recorded_act(&mut self, act: &str, item: Option<&str>) -> Result<(), String> {
        let parts: Vec<&str> = act.split(':').collect();
        let item_name = |n: &str| item.map_or(n.replace(' ', "_"), str::to_string);
        // Dump.Item, the recorder's item root, when it has no PhysicalBehaviour (the Decoupler): its Transform's id. The
        // recorder's GetComponent<PhysicalBehaviour>() is then null: itemignite throws (the act is lost), hold's listener
        // does nothing, and the Use broadcast starts at the root.
        let bare_root = |g: &Self, name: &str| -> Option<i32> {
            if g.physicals.iter().any(|ph| ph.name == name && !ph.destroyed) {
                return None;
            }
            g.retained.iter().find(|i| i.prefab.objects[0].name.replace(' ', "_") == name).map(|i| i.objects[0].components[0])
        };
        let l = self.limbs.iter().position(|l| self.physicals[l.PhysicalBehaviour].name == parts[1]).ok_or(format!("no limb {}", parts[1]))?;
        let p = self.limbs[l].PhysicalBehaviour;
        let value: f32 = parts.get(2).map_or(Ok(0.0), |v| v.parse()).map_err(|e| format!("{act}: {e}"))?;
        let at = self.physicals[p].transform.position;
        match parts[0] {
            "shot" => {
                let (normal, point) = ([-1.0, 0.0], [at[0], at[1]]);
                self.physical_shot(p, normal, point, value);
                self.limb_shot(l, normal, point, value, true);
                self.circulation_shot(l, normal, point, value);
            }
            "ignite" => self.ignite(p, true),
            // freeze:<limb>: the context menu's Freeze (AddComponent<FreezeBehaviour>).
            "freeze" => self.freeze_add_component(p),
            // savechild:<limb>:0:<item>:<Class>@<child path>@<0|1>,...: a hand-edited save (the recorder's act): an ObjectState
            // of <item> at (0, 0) whose named children (at the root's centre) carry one bare prototype each, loaded at (4, 2, 0).
            "savechild" => {
                use crate::game::save::{MonoBehaviourPrototype, ObjectState, TransformPrototype};
                let item = parts.get(3).ok_or("savechild:<limb>:0:<item>:<specs>")?;
                // The recorder's probe object just before the paste: the game's ids continue after it.
                let probe: i32 = parts.get(5).ok_or("savechild: no probe id (re-record it)")?.parse().map_err(|_| "savechild probe id")?;
                self.next_instance_id = probe - 2;
                let mut state = ObjectState::new(item.to_string(), TransformPrototype::new([0.0; 2], 0.0, [1.0; 3]), vec![]);
                for spec in parts.get(4).ok_or("savechild specs")?.split(',') {
                    let w: Vec<&str> = spec.split('@').collect();
                    if !state.Children.iter().any(|(k, _)| k == w[1]) {
                        state.Children.push((w[1].into(), TransformPrototype::new([0.0; 2], 0.0, [1.0; 3])));
                    }
                    let proto = MonoBehaviourPrototype { Enabled: w[2] == "1", Type: w[0].into(), storedFields: vec![], storedProperties: vec![], storedReferenceFields: vec![] };
                    match state.ChildComponentData.iter_mut().find(|(k, _)| k == w[1]) {
                        Some((_, list)) => list.push(proto),
                        None => state.ChildComponentData.push((w[1].into(), vec![proto])),
                    }
                }
                self.object_states_convert(&[state], [4.0, 2.0, 0.0], false);
            }
            // load:<limb>:<dx>:<contraption>:<x>,<y>:<probe id>:<.json hex>:<.jaap hex> (the recorder's act): the contraption loaded as the
            // catalog places one, ContraptionSerialiser.LoadContraption then CatalogBehaviour.Spawn's PasteLoadAction(Convert(states, (x, y, 0),
            // false), DisplayName); the files ride in the word (the data file is written to a temp path for LoadContraption to read).
            "load" => {
                let hex = |t: &str| -> Result<Vec<u8>, String> {
                    (0..t.len()).step_by(2).map(|i| u8::from_str_radix(&t[i..i + 2], 16).map_err(|e| e.to_string())).collect()
                };
                let [x, y]: [f32; 2] = floats(parts.get(4).ok_or("load: no position")?);
                let probe: i32 = parts.get(5).ok_or("load: no probe id")?.parse().map_err(|_| "load probe id")?;
                let json = String::from_utf8(hex(parts.get(6).ok_or("load: no .json")?)?).map_err(|e| e.to_string())?;
                let mut meta = crate::game::contraption::ContraptionMetaData::from_json(&json)?.ok_or("load: null metadata")?;
                let data = std::env::temp_dir().join(format!("ppg-replay-load-{}.jaap", std::process::id()));
                std::fs::write(&data, hex(parts.get(7).ok_or("load: no .jaap")?)?).map_err(|e| e.to_string())?;
                meta.PathToDataFile = Some(data.clone());
                let mut messages = crate::game::contraption::LoadMessages::default();
                // The catalog's lookups (and the migrations) are built at load.
                let mut db = self.assets.take().ok_or("load without the game's data")?;
                let _ = Self::get_spawnable(&mut db, "");
                self.assets = Some(db);
                let states = crate::game::contraption::load_contraption(&mut meta, &mut messages);
                let _ = std::fs::remove_file(&data);
                for n in messages.notifications {
                    self.notify(n);
                }
                self.next_instance_id = probe - 2;
                let name = meta.DisplayName.clone().unwrap_or_default();
                self.paste_states(&states?, [x, y, 0.0], false, &name);
            }
            // spawn:<limb>:<dx>:<asset>:<flip>:<x>,<y>:<probe id>: CatalogBehaviour.Spawn(asset, flip) at Global.MousePosition (x, y, 0).
            "spawn" => {
                let asset = parts.get(3).ok_or("spawn: no asset")?.replace('_', " ");
                let flip = parts.get(4) == Some(&"1");
                let [x, y]: [f32; 2] = floats(parts.get(5).ok_or("spawn: no position")?);
                self.next_instance_id = parts.get(6).ok_or("spawn: no probe id")?.parse::<i32>().map_err(|_| "spawn probe id")? - 2;
                let mut db = self.assets.take().ok_or("spawn without the game's data")?;
                let spawned = self.catalog_spawn(&mut db, &asset, [x, y, 0.0], flip);
                self.assets = Some(db);
                self.unsynced.push(spawned?.0);
            }
            // itemignite:<limb>:0:<item>: the PPG_STAB item's PhysicalBehaviour.Ignite(true).
            "itemignite" => {
                let name = item_name(parts.get(3).ok_or("itemignite:<limb>:0:<item>")?);
                // Dump.Item.GetComponent<PhysicalBehaviour>() is null on a root without one (the Decoupler, the Tank whose
                // physicals are its children): Ignite throws.
                let Some(q) = self.physicals.iter().position(|ph| ph.name == name && !ph.destroyed) else { super::menu::npe() };
                self.ignite(q, true);
            }
            "charge" => self.physicals[p].charge = value,
            "temp" => self.physicals[p].Temperature = value,
            "use" => self.syringe_use(0, value as i32),
            // fire:<limb>:0:<item>: the PPG_STAB item activated, BroadcastMessage("Use", new ActivationPropagation(true, 0)).
            "fire" => {
                let name = item_name(parts.get(3).ok_or("fire:<limb>:0:<item>")?);
                if let Some(root) = bare_root(self, &name) {
                    self.recorded_shot_impacts();
                    let a = self.signal_direct(true, 0);
                    self.broadcast_use_object(root, &a);
                    return Ok(());
                }
                let q = self.physicals.iter().position(|ph| ph.name == name && !ph.destroyed).ok_or(format!("no item {name}"))?;
                self.recorded_shot_impacts();
                // A recording with Clone rows linked the item's scripts: the broadcast reaches each of them.
                let id = self.physicals[q].id;
                if self.retained.iter().any(|i| i.objects[0].components.contains(&id)) {
                    let a = self.signal_direct(true, 0);
                    self.broadcast_use(q, &a);
                    return Ok(());
                }
                // BroadcastMessage("Use") reaches the item's FirearmBehaviours (PhysicalBehaviour.Use is only a mod event).
                for f in 0..self.firearms.len() {
                    if self.firearms[f].physical == Some(q) {
                        self.firearm_use(f);
                    }
                }
            }
            // hold:<limb>:<count>:<item>: fire, then the activate key held for <count> OnContinuousUpdate calls (the
            // recorder's listener, added after every other).
            "hold" => {
                let name = item_name(parts.get(3).ok_or("hold:<limb>:<count>:<item>")?);
                if let Some(root) = bare_root(self, &name) {
                    self.recorded_shot_impacts();
                    let a = self.signal_direct(true, 0);
                    self.broadcast_use_object(root, &a);
                    return Ok(());
                }
                let q = self.physicals.iter().position(|ph| ph.name == name && !ph.destroyed).ok_or(format!("no item {name}"))?;
                self.recorded_shot_impacts();
                // The recorder's BroadcastMessage("Use") first, as fire: every linked script of the item.
                let id = self.physicals[q].id;
                if self.retained.iter().any(|i| i.objects[0].components.contains(&id)) {
                    let a = self.signal_direct(true, 0);
                    self.broadcast_use(q, &a);
                } else {
                    for f in 0..self.firearms.len() {
                        if self.firearms[f].physical == Some(q) {
                            self.firearm_use(f);
                        }
                    }
                    for m in 0..self.miniguns.len() {
                        if self.miniguns[m].physical == q {
                            self.minigun_use(m);
                        }
                    }
                }
                self.continuous.hold_left = value as u32;
                self.continuous.listeners.push(super::continuous::Listener::Hold(q));
            }
            "itemshot" => {
                self.recorded_shot_impacts();
                let q = self.syringes[0].Phys;
                let at = self.physicals[q].transform.position;
                self.physical_shot(q, [-1.0, 0.0], [at[0], at[1]], value);
                self.syringe_shot(0);
            }
            "grip" => self.grip_use(self.limbs[l].GripBehaviour.ok_or("a limb without a grip")?),
            // ContextMenuBehaviour.ResizeAction with the limb alone selected.
            "resize" => {
                self.recorded_resize_handles()?;
                let e = self.physicals[p].InitialBounds.1;
                self.resize_handles_set_targets(vec![p], [e[0], e[1]]);
            }
            // UserPreferenceManager.Current.ToolSelectionMode (0 Area, 1 Mass, 2 Depth).
            "selmode" => self.prefs.ToolSelectionMode = value as i32,
            // The same with UserPreferenceManager.Current.AmbientTemperatureTransfer on.
            "airheat" => {
                self.prefs.AmbientTemperatureTransfer = true;
                self.physicals[p].Temperature = value;
            }
            "stun" => self.people[self.limbs[l].Person].Consciousness = value,
            "health" => self.limbs[l].Health = value,
            "blood" => self.circulations[l].BloodFlow = value,
            "explode" => {
                let game = crate::game_assets::find_game(None).expect("the game's data");
                self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
                let mut db = self.assets.take().unwrap();
                self.load_distant_sounds(&mut db).expect("the game's data");
                self.assets = Some(db);
                self.explosion_creator_explode_at(at, value)
            }
            "watershot" => {
                let game = crate::game_assets::find_game(None).expect("the game's data");
                self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
                // The recorder's own object with the component: GameObject, Transform, WaterImpactBehaviour.
                self.allocate_instance_ids(3);
                let splash = self.assets.as_ref().and_then(|db| db.resource("Prefabs/Splash", 1).ok().flatten()).expect("Prefabs/Splash");
                self.water_impact_shot(splash, [0.0, 1.0], [at[0], at[1]], value)
            }
            // PPG_ACT=liquid:<limb>:<amount>:<Liquid ID>
            "liquid" => {
                let liquid = LIQUIDS.iter().position(|(n, _)| *n == parts[3]).ok_or(format!("no liquid {}", parts[3]))?;
                let game = crate::game_assets::find_game(None).expect("the game's data");
                self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
                let mut db = self.assets.take().unwrap();
                self.load_distant_sounds(&mut db).expect("the game's data");
                self.assets = Some(db);
                self.add_liquid(super::container::Owner::Circulation(l), liquid, value);
            }
            // N real bullets through the limb from the GlobalShrapnelEmitter, as Awake sets it up with the Main scene's values.
            "bullet" => {
                self.recorded_shot_impacts();
                for i in 0..value as i32 {
                    self.emitter_emit(super::ballistics::Shooter::Shrapnel, [at[0] - 1.5, at[1]], [1.0, (super::d(-0.2f32) + super::d(0.05f32) * (i as f32)).f()]);
                }
            }
            "underwater" => {
                let game = crate::game_assets::find_game(None).expect("the game's data");
                self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data")));
                let mut db = self.assets.take().unwrap();
                self.load_distant_sounds(&mut db).expect("the game's data");
                self.assets = Some(db);
                self.create_underwater_explosion_effect(at, value, true)
            }
            // menu:<limb>:<dx>:<target>#<PhysicalBehaviour id>:<button>:<x>,<y>:<probe id> (the recorder's MenuAct): the target
            // alone selected, then the button: a static ContextMenuBehaviour one ('_' for ' ', the menu open at x, y) or the
            // target's dynamic buttons with that Identity whose condition holds.
            "menu" => {
                let id: i32 = parts.get(3).and_then(|t| t.rsplit_once('#')).and_then(|(_, id)| id.parse().ok()).ok_or("menu: no target id")?;
                let q = self.physicals.iter().position(|ph| ph.id == id && !ph.destroyed).ok_or(format!("menu: no physical #{id}"))?;
                let button = parts.get(4).ok_or("menu: no button")?.replace('_', " ");
                let [x, y]: [f32; 2] = floats(parts.get(5).ok_or("menu: no position")?);
                self.select(q, false);
                if Game::context_static_rewritten(&button) {
                    if button == "Paste" {
                        self.next_instance_id = parts.get(6).ok_or("menu: no probe id")?.parse::<i32>().map_err(|_| "menu probe id")? - 2;
                    }
                    // Not shown (the recorder sets isOpen): no wire was hovered for Delete.
                    self.context_prior = None;
                    self.context_static_click(&button, [x, y]);
                } else {
                    let actions: Vec<_> = self.context_menu_buttons().into_iter().filter(|b| b.Identity == button).flat_map(|b| b.actions).collect();
                    self.context_menu_click(&actions);
                }
            }
            k => return Err(format!("PPG_ACT {k}")),
        }
        Ok(())
    }

    /// Every drawn syringe's SyringeBehaviour.OnWillRenderObject (syringe.rs syringe_will_render; GetColor caches the computed colour) for every live
    /// syringe the camera draws (its sprite inside the recorded view). A recording from before an item's sprite bounds were
    /// written: every live syringe counts as drawn.
    pub fn syringes_will_render(&mut self) {
        for s in 0..self.syringes.len() {
            let p = self.syringes[s].Phys;
            let drawn = match self.physicals[p].sprite_bounds {
                Some(_) => self.sprite_drawn(super::visual::SpriteAt::Physical(p)),
                None => !self.physicals[p].destroyed,
            };
            if drawn {
                self.syringe_will_render(s);
            }
        }
    }

    /// SkinMaterialHandler.OnWillRenderObject
    pub fn skin_will_render(&mut self, l: usize) {
        let blood = super::circulation::liquid_color(super::circulation::liquid(&self.limbs[l].BloodLiquidType));
        self.get_computed_color(super::container::Owner::Circulation(l), blood);
    }
}

/// A FixedJoint2D or SpringJoint2D (no hinge, slider, friction, distance or relative part) on a physical: the ones the
/// earlier JointBreakLimits arms (adopted, wires, pins) leave are the run-time ones (ShatteredObjectGenerator.Generate's).
fn runtime_joint(g: &Game, f: usize) -> bool {
    let j = &g.fixed_joints[f];
    j.hinge.is_none() && j.slider.is_none() && j.friction.is_none() && j.distance.is_none() && j.relative.is_none()
        && matches!(j.attached, super::BodyOwner::Physical(_))
}

/// The Rigidbody2D record of a referenced Rigidbody2D id -> the owning LimbBehaviour id.
/// PhysicsScene2D.m_JointBreakLimits from its record, by joint id: a limb's hinge, an adopted prefab joint, a linked
/// prefab hinge, a prefab joint's slot, a limb's SpringJoint2D. None for a recording without the record.
fn recorded_break_limits(g: &Game, s: &Snapshot) -> Option<Vec<super::BreakJoint>> {
    let r = s.of("JointBreakLimits").next()?;
    let ids = r.get("ids").unwrap_or("");
    Some(ids.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()).map(|id| {
        let id: i32 = id.parse().unwrap();
        match s.of("HingeJoint2D").position(|h| h.id == id) {
            Some(h) => super::BreakJoint::Hinge(h),
            None if g.adopted_joints.contains_key(&id) => super::BreakJoint::Fixed(g.adopted_joints[&id]),
            // An inactive limb's hinge (no HingeJoint2D record): the slot the limb loader gave it.
            None if s.of("LimbBehaviour").any(|l| l.get("Joint").and_then(reference) == Some(id)) => {
                let rid = s.of("LimbBehaviour").find(|l| l.get("Joint").and_then(reference) == Some(id)).unwrap().id;
                super::BreakJoint::Hinge(g.limbs.iter().find(|l| l.id == rid).and_then(|l| l.Joint).unwrap())
            }
            // fnweap: a wire's DistanceJoint2D (fnweap::load_wires).
            None if fnweap::wire_joint(g, s, id).is_some() => super::BreakJoint::Fixed(fnweap::wire_joint(g, s, id).unwrap()),
            None => match g.anchored_joints.iter().position(|a| a.component == id) {
                Some(a) => match g.anchored_joints[a].hinge {
                    Some(h) => super::BreakJoint::Hinge(h),
                    None => super::BreakJoint::Anchored(a),
                },
                None => super::BreakJoint::Spring(s.of("SpringJoint2D").find(|j| j.id == id).and_then(|j| s.of("LimbBehaviour").position(|l| l.name == j.name)).unwrap_or_else(|| panic!("a break limit's joint #{id}: no HingeJoint2D record, adopted, anchored ({} entries: {:?}) or limb spring joint", g.anchored_joints.len(), g.anchored_joints.iter().map(|a| a.component).collect::<Vec<_>>()))),
            },
        }
    }).collect())
}

fn rb_owner(s: &Snapshot, rb_id: i32) -> Result<i32, String> {
    // An inactive limb's Rigidbody2D has no record: its Clone record lists it.
    let listed = |c: &&Record| c.get("comps").is_some_and(|t| t.contains(&format!("Rigidbody2D:{rb_id};")) || t.contains(&format!("Rigidbody2D:{rb_id}]")));
    let name = &s.of("Rigidbody2D").find(|r| r.id == rb_id).or_else(|| s.of("Clone").find(listed)).ok_or("unknown rigidbody")?.name;
    Ok(s.of("LimbBehaviour").find(|r| &r.name == name).ok_or("rigidbody without limb")?.id)
}

fn bounds(text: &str) -> [f32; 6] {
    let fields = inner_fields(text);
    let c: [f32; 3] = floats(&fields[0].1);
    let e: [f32; 3] = floats(&fields[1].1);
    [c[0], c[1], c[2], e[0], e[1], e[2]]
}

/// b2Body words 0x00-0x58 and 0x90-0xb8 (UnityPlayer PDB layout).
fn load_body(b: &mut crate::b2::world::Body, r: &Record) -> Result<(), String> {
    let w = words(r.req("body")?);
    let m = words(r.req("mass")?);
    let fl = |v: u32| f32::from_bits(v);
    b.kind = match w[0] {
        0 => BodyType::Static,
        1 => BodyType::Kinematic,
        _ => BodyType::Dynamic,
    };
    b.flags = w[1] as u16;
    b.xf.p = V::new(fl(w[3]), fl(w[4]));
    b.xf.q = Rot { s: fl(w[5]), c: fl(w[6]) };
    b.sweep.local_center = V::new(fl(w[7]), fl(w[8]));
    b.sweep.c0 = V::new(fl(w[9]), fl(w[10]));
    b.sweep.c = V::new(fl(w[11]), fl(w[12]));
    b.sweep.a0 = fl(w[13]);
    b.sweep.a = fl(w[14]);
    b.sweep.alpha0 = fl(w[15]);
    b.linear_velocity = V::new(fl(w[16]), fl(w[17]));
    b.angular_velocity = fl(w[18]);
    b.force = V::new(fl(w[19]), fl(w[20]));
    b.torque = fl(w[21]);
    b.mass = fl(m[0]);
    b.inv_mass = fl(m[1]);
    b.axis = V::new(fl(m[2]), fl(m[3]));
    b.i = fl(m[4]);
    b.inv_i = fl(m[5]);
    b.linear_damping = fl(m[6]);
    b.angular_damping = fl(m[7]);
    b.gravity_scale = fl(m[8]);
    b.sleep_time = fl(m[9]);
    b.rigidbody.mass = fl(m[0]);
    Ok(())
}

/// Person.Poses as the record names them (the recorder writes names only): a prefab pose by name, or a GenerateBasePose
/// one (CreatePose's "Pose N", and InitialiseAllPoses' PoseState-named list with each State) from the joints' angles now.
/// A record without Poses: the prefab's. Returns the store and PoseStart: before the list, the prefab poses that
/// LinkedPoses/ActivePose (`refs`, by name) still hold after InitialiseAllPoses' Clear, or that are not in the list.
/// A person's Poses, PoseStart, ActivePose and LinkedPoses from its PersonBehaviour record, `rig` being its prefab's poses.
pub(super) fn load_person_poses(g: &Game, p: &mut Person, rig: Vec<RagdollPose>, r: &Record) -> Result<(), String> {
    // checker limit: the record names poses only; after InitialiseAllPoses a name both lists share ("Rest") is taken as
    // the prefab's (CreateLinkedPoseDictionary ran at Awake, before).
    let pose_name = |t: &str| t.trim_start_matches("RagdollPose:").trim_matches('"').replace('_', " ");
    let linked = r.req("LinkedPoses")?;
    let linked = format!("[{}]", &linked[1..linked.len() - 1]);
    let linked: Vec<&str> = list(&linked);
    let refs: Vec<String> = linked.iter().filter_map(|e| e.split_once(':')).map(|(_, v)| v).chain(Some(r.req("ActivePose")?).filter(|t| *t != "null")).map(pose_name).collect();
    (p.Poses, p.PoseStart) = recorded_poses(g, &p.Limbs, rig, r.get("Poses"), &refs)?;
    // A reference names a pose that left the list (before PoseStart) first: LinkedPoses and ActivePose outlive Clear.
    let pose_named = |t: &str| -> Result<usize, String> {
        let name = pose_name(t);
        let (gone, listed) = p.Poses.split_at(p.PoseStart);
        gone.iter().position(|q| q.Name == name).or_else(|| listed.iter().position(|q| q.Name == name).map(|i| i + p.PoseStart)).ok_or(format!("pose {name}"))
    };
    p.ActivePose = match r.req("ActivePose")? {
        "null" => None,
        t => Some(pose_named(t)?),
    };
    for entry in linked {
        let (k, v) = entry.split_once(':').unwrap();
        p.LinkedPoses[k.parse::<usize>().unwrap()] = pose_named(v)?;
    }
    Ok(())
}

fn recorded_poses(g: &Game, limbs: &[usize], rig: Vec<RagdollPose>, names: Option<&str>, refs: &[String]) -> Result<(Vec<RagdollPose>, usize), String> {
    use super::person::pose_state;
    let Some(names) = names else { return Ok((rig, 0)) };
    // batch3 r3 recordings add `/rigidity=<hex>/angles=[<limb id>:<Animated>:<Angle>:<StartAngle>:<EndAngle>,..]` after the
    // name: the pose's state (a GenerateBasePose took the joint angles of its own time, not the snapshot's), loaded below.
    let raw: Vec<&str> = list(names);
    let names: Vec<String> = raw.iter().map(|t| t.trim_start_matches("RagdollPose:").split('/').next().unwrap().trim_matches('"').replace('_', " ")).collect();
    let initialised = names.len() >= pose_state::COUNT && names[..pose_state::COUNT].iter().zip(pose_state::NAMES).all(|(a, b)| a == b);
    let mut out: Vec<RagdollPose> = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let base = initialised && i < pose_state::COUNT;
        let pose = match rig.iter().find(|q| &q.Name == name).filter(|_| !base) {
            Some(q) => q.clone(),
            None if base || name.starts_with("Pose ") => {
                let mut q = g.base_pose(limbs, out.len());
                q.Name = name.clone();
                if base {
                    q.State = i as i32;
                }
                q
            }
            None => return Err(format!("recorded pose {name}: neither the prefab's nor a GenerateBasePose")),
        };
        let mut pose = pose;
        if let Some((_, rest)) = raw[i].split_once("/rigidity=") {
            let (rig, angles) = rest.split_once("/angles=").ok_or(format!("pose {name}: no angles="))?;
            let h = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| format!("pose {name}: {v}: {e}"));
            pose.Rigidity = h(rig)?;
            let entries: Vec<&str> = angles.trim_start_matches('[').trim_end_matches(']').split(',').filter(|e| !e.is_empty()).collect();
            if entries.len() != pose.Angles.len() {
                return Err(format!("pose {name}: {} recorded limb angles, ours {}", entries.len(), pose.Angles.len()));
            }
            for (a, e) in pose.Angles.iter_mut().zip(entries) {
                let f: Vec<&str> = e.split(':').collect();
                let [id, anim, angle, start, end] = f[..] else { return Err(format!("pose {name}: angle {e}")) };
                if id.parse::<i32>().ok() != Some(g.limbs[a.Limb].id) || (anim == "1") != a.Animated {
                    return Err(format!("pose {name}: recorded limb {e}, ours #{} animated {}", g.limbs[a.Limb].id, a.Animated));
                }
                (a.Angle, a.StartAngle, a.EndAngle) = (h(angle)?, h(start)?, h(end)?);
            }
        }
        out.push(pose);
    }
    let gone: Vec<RagdollPose> = rig.into_iter().filter(|q| refs.contains(&q.Name) && (initialised || !names.contains(&q.Name))).collect();
    let start = gone.len();
    Ok((gone.into_iter().chain(out).collect(), start))
}

/// The prefab's RagdollPoses with LimbPose entries bound to limbs by name.
fn rig_poses(rig: &HumanRig, limbs: &[Limb]) -> Result<Vec<RagdollPose>, String> {
    rig.poses
        .iter()
        .map(|p| {
            let angles = p
                .angles
                .iter()
                .map(|a| {
                    let limb = limbs.iter().position(|l| l.name == a.part).ok_or(format!("pose limb {}", a.part))?;
                    let anim = a.animation.as_ref();
                    Ok(LimbPose {
                        Limb: limb,
                        Animated: anim.is_some(),
                        PoseRigidityModifier: a.rigidity_modifier,
                        Angle: a.degrees(),
                        StartAngle: anim.map_or(a.degrees(), |x| x.start),
                        EndAngle: anim.map_or(a.degrees(), |x| x.end),
                        AnimationDuration: anim.map_or(1.0, |x| x.duration),
                        RandomInfluence: anim.map_or(0.0, |x| x.random_influence),
                        RandomSpeed: anim.map_or(1.0, |x| x.random_speed),
                        TimeOffset: anim.map_or(0.0, |x| x.time_offset),
                        AnimationCurve: anim.map(|x| x.curve.clone()),
                        cache: CurveCache::default(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(RagdollPose {
                Name: p.name.clone(),
                Rigidity: p.rigidity,
                ForceMultiplier: p.force_multiplier,
                ShouldStandUpright: p.stand_upright,
                DragInfluence: p.drag_influence,
                UprightForceMultiplier: p.upright_multiplier,
                ShouldStumble: p.should_stumble,
                State: p.state,
                AnimationSpeedMultiplier: p.animation_speed,
                Angles: angles,
            })
        })
        .collect()
}

/// Compares every dumped field of the rewritten scripts with a snapshot; returns the mismatches.
/// A started pin whose joint is still there.
fn live_pin(g: &Game, p: &super::pin::Pin) -> bool {
    p.Joint.is_some_and(|f| !g.fixed_joints[f].destroyed)
}

/// The bleeding particles in a snapshot's row order: by limb.
fn bleeding_rows(g: &Game) -> Vec<usize> {
    // The recorder walks Dump.Human (person 0): another person's particles are not its rows (their splats go with the other splats).
    let mut bleeding: Vec<usize> = (0..g.bleeding.len()).filter(|&b| !g.bleeding[b].destroyed && g.bleeding[b].CirculationBehaviour.is_some_and(|l| g.limbs[l].Person == 0)).collect();
    bleeding.sort_by_key(|&b| g.bleeding[b].CirculationBehaviour);
    bleeding
}

pub fn compare(g: &mut Game, s: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    compare_links(g, s, &mut out);
    out.extend(fnmaped::compare_serialised_layers(g, s));
    out.extend(fnmaped::compare_camera(g, s));
    out.extend(super::map::check::compare(g, s));
    // PoolGenerator's pools, by name: GetPoolSize, GetInactivePoolSize and the inactivity timer.
    for r in s.of("PoolGeneratorPool") {
        let theirs = format!("size={} inactive={} timer={}", r.get("size").unwrap_or("?"), r.get("inactive").unwrap_or("?"), r.get("timer").unwrap_or("?"));
        let ours = g.pool_named(&r.name).map(|p| format!("size={} inactive={} timer={:08x}", p.get_pool_size(), p.get_inactive_pool_size(), p.inactivityTimer.to_bits()));
        if ours.as_ref() != Some(&theirs) {
            out.push(format!("PoolGeneratorPool {}: game {theirs} ours {ours:?}", r.name));
        }
    }
    // gapA r2: each LocalFirePropagationMap's Points and RandomisedIndices (the load's record), by its phys.
    for r in s.of("LocalFirePropagationMap") {
        let h = |v: f32| format!("{:08x}", v.to_bits());
        let phys: Option<i32> = r.get("phys").and_then(|v| v.parse().ok());
        let Some(m) = g.fire_maps.iter().find(|m| Some(g.physicals[m.physical].id) == phys) else {
            out.push(format!("LocalFirePropagationMap {} (phys {phys:?}): no fire map", r.id));
            continue;
        };
        let ours: Vec<String> = m.Points.iter().map(|p| {
            let nb = if m.neighbours_null && p.NeighbourIndices.is_empty() { "null".to_string() } else { p.NeighbourIndices.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(":") };
            format!("{},{},{},{},{nb}", h(p.LocalPoint[0]), h(p.LocalPoint[1]), h(p.FireIntensity), h(p.Softness))
        }).collect();
        let theirs: Vec<&str> = r.get("Points").unwrap_or("").split(';').filter(|x| !x.is_empty() && *x != "null").collect();
        if theirs != ours {
            let k = theirs.iter().zip(&ours).position(|(a, b)| a != b).unwrap_or(theirs.len().min(ours.len()));
            out.push(format!("LocalFirePropagationMap {} Points ({} vs {}): first difference #{k}: game {:?} ours {:?}", r.id, theirs.len(), ours.len(), theirs.get(k), ours.get(k)));
        }
        let theirs: Vec<&str> = match r.get("RandomisedIndices") { Some("null") | None => Vec::new(), Some(t) => t.split(',').filter(|x| !x.is_empty()).collect() };
        let ours: Vec<String> = m.RandomisedIndices.iter().map(|i| i.to_string()).collect();
        if theirs != ours {
            out.push(format!("LocalFirePropagationMap {} RandomisedIndices: game {theirs:?} ours {ours:?}", r.id));
        }
    }
    // The scene's BulletTracerPool: GetPoolSize, GetInactivePoolSize and the inactivity timer.
    for r in s.of("TracerPool") {
        let theirs = format!("size={} inactive={} timer={}", r.get("size").unwrap_or("?"), r.get("inactive").unwrap_or("?"), r.get("timer").unwrap_or("?"));
        let t = &g.tracer_pool;
        let ours = format!("size={} inactive={} timer={:08x}", t.created, t.available, t.inactivityTimer.to_bits());
        if ours != theirs {
            out.push(format!("TracerPool: game {theirs} ours {ours}"));
        }
    }
    // A generic pool's clones in createdObjects order: activeSelf, position and a bolt's trail positionCount.
    let mut nth = std::collections::HashMap::new();
    for r in s.of("PoolObject") {
        let Some(p) = g.generic_pools.iter().position(|p| format!("{}_pool", p.name.replace(' ', "_")) == r.name) else { continue };
        let k = nth.entry(r.name.clone()).or_insert(0usize);
        let root = g.generic_pools[p].pool.created.get(*k).map(|&i| g.generic_pools[p].items[i].root);
        *k += 1;
        let theirs = format!("active={} pos={} trail={}", r.get("active").unwrap_or("?"), r.get("pos").unwrap_or("?"), r.get("trail").unwrap_or("?"));
        let ours = root.map(|root| {
            let active = g.tr_active_self(super::translated::rt::Go::Local(root)) as u8;
            let pos = g.with_component_instance(root, |_, inst, o| inst.position(o)).map(|v| format!("{:08x}", v.to_bits())).join(",");
            let trail = g.archelix_bolt_of_clone(root).map_or("-".into(), |b| g.archelix_bolts[b].trail.len().to_string());
            format!("active={active} pos={pos} trail={trail}")
        });
        if ours.as_ref() != Some(&theirs) {
            out.push(format!("PoolObject {} #{}: game {theirs} ours {ours:?}", r.name, *k - 1));
        }
    }
    // m_JointBreakLimits, by joint.
    if let Some(r) = s.of("JointBreakLimits").next() {
        // An adopted joint (a recorded SliderJoint2D, a map wire's DistanceJoint2D): the recorded component's id.
        let adopted: HashMap<usize, i32> = g.adopted_joints.iter().map(|(&id, &f)| (f, id)).collect();
        let ours: Vec<String> = g.joint_break_limits.iter().map(|k| match k {
            super::BreakJoint::Fixed(f) if adopted.contains_key(f) => adopted[f].to_string(),
            super::BreakJoint::Hinge(h) => match g.limbs.iter().find(|l| l.Joint == Some(*h)) {
                // A spawned human's limb: the HingeJoint2D on its GameObject (the Clone row with its Rigidbody2D).
                Some(l) if l.Person != 0 => {
                    let rb = format!("Rigidbody2D:{}", g.physicals[l.PhysicalBehaviour].rigidbody.component);
                    let comps = s.of("Clone").filter_map(|c| c.get("comps")).find(|c| c.trim_matches(['[', ']']).split(';').any(|x| x == rb));
                    comps.and_then(|c| c.trim_matches(['[', ']']).split(';').find_map(|x| x.strip_prefix("HingeJoint2D:")).map(str::to_string)).unwrap_or("?".into())
                }
                Some(l) => s.of("HingeJoint2D").find(|r| r.name == g.physicals[l.PhysicalBehaviour].name).map_or("?".into(), |r| r.id.to_string()),
                // A linked prefab hinge (the clone's item): the slot it took over.
                // Else a recorded HingeJoint2D without a limb of its own (an earlier case's pin on UpperBody): the record in
                // its slot (the loader keeps one `hinges` slot per record, in record order).
                None => g.anchored_joints.iter().find(|a| a.hinge == Some(*h)).map(|a| a.component).or_else(|| s.of("HingeJoint2D").nth(*h).map(|r| r.id)).map_or("?".into(), |id| id.to_string()),
            },
            // A prefab joint (the PPG_STAB item's): its component's id.
            super::BreakJoint::Anchored(a) => g.anchored_joints[*a].component.to_string(),
            // A pin's HingeJoint2D comes after the limb's own on the GameObject.
            super::BreakJoint::Fixed(f) if g.pins.iter().any(|p| p.Joint == Some(*f)) => {
                let pin = g.pins.iter().position(|p| p.Joint == Some(*f)).unwrap();
                // A pin on an object without a PhysicalBehaviour: that object's name.
                let name = match g.pins[pin].root {
                    None => g.physicals[g.pins[pin].physical].name.clone(),
                    Some(tid) => g.unsynced.iter().chain(&g.retained).find_map(|i| i.objects.iter().position(|x| x.components.contains(&tid)).map(|o| i.prefab.objects[o].name.clone())).unwrap(),
                };
                let name = &name;
                let nth = g.pins[..pin].iter().filter(|p| p.physical == g.pins[pin].physical && p.root == g.pins[pin].root && live_pin(g, p)).count();
                s.of("HingeJoint2D").filter(|r| &r.name == name).nth(nth + 1).map_or("?".into(), |r| r.id.to_string())
            }
            super::BreakJoint::Fixed(f) if g.blood_wires.iter().any(|w| w.joint == *f) => {
                // The wire's joint among its object's wire joints (the joints on the Clone row with its Rigidbody2D), oldest first.
                let wire = g.blood_wires.iter().position(|w| w.joint == *f).unwrap();
                let p = g.blood_wires[wire].physical;
                let nth = g.blood_wires[..wire].iter().filter(|w| w.physical == p && !g.fixed_joints[w.joint].destroyed).count();
                let rb = format!("Rigidbody2D:{}", g.physicals[p].rigidbody.component);
                let comps = s.of("Clone").filter_map(|c| c.get("comps")).find(|c| c.trim_matches(['[', ']']).split(';').any(|x| x == rb)).unwrap_or("");
                let mut ids: Vec<i32> = comps.trim_matches(['[', ']']).split(';').filter_map(|x| x.strip_prefix("FixedJoint2D:").or_else(|| x.strip_prefix("DistanceJoint2D:")))
                    .filter_map(|x| x.parse().ok()).filter(|id| !g.anchored_joints.iter().any(|a| a.component == *id)).collect();
                ids.sort_unstable_by_key(|&id| std::cmp::Reverse(id));
                ids.get(nth).map_or("?".into(), |id| id.to_string())
            }
            // A FixedJoint2D or SpringJoint2D added at run time on a physical (ShatteredObjectGenerator.Generate's shards: the soft
            // joints between them, the fixed one to ConnectTo), by its place among that object's joints of its kind, oldest (highest
            // id) first: a SpringJoint2D's from the MapSpringJoint2D records it owns, a FixedJoint2D's from its object's Clone row.
            super::BreakJoint::Fixed(f) if runtime_joint(g, *f) => {
                let (at, spring) = (g.fixed_joints[*f].attached, g.fixed_joints[*f].spring.is_some());
                let super::BodyOwner::Physical(p) = at else { unreachable!() };
                let nth = (0..*f).filter(|&x| runtime_joint(g, x) && !g.fixed_joints[x].destroyed && g.fixed_joints[x].attached == at && g.fixed_joints[x].spring.is_some() == spring).count();
                let rb = g.physicals[p].rigidbody.component;
                let mut ids: Vec<i32> = if spring {
                    let own = format!("#{rb}");
                    s.of("MapSpringJoint2D").filter(|r| r.get("own").is_some_and(|o| o.ends_with(&own))).map(|r| r.id).collect()
                } else {
                    let rb = format!("Rigidbody2D:{rb}");
                    let comps = s.of("Clone").filter_map(|c| c.get("comps")).find(|c| c.trim_matches(['[', ']']).split(';').any(|x| x == rb)).unwrap_or("");
                    comps.trim_matches(['[', ']']).split(';').filter_map(|x| x.strip_prefix("FixedJoint2D:")).filter_map(|x| x.parse().ok()).collect()
                };
                ids.sort_unstable_by_key(|&id| std::cmp::Reverse(id));
                ids.get(nth).map_or(format!("{k:?}"), |id| id.to_string())
            }
            // A stab's SliderJoint2D: the live ones in creation order (the newest has the lowest id).
            super::BreakJoint::Fixed(f) if !g.fixed_joints[*f].destroyed && s.of("SliderJoint2D").next().is_some() => {
                let nth = (0..*f).filter(|&x| !g.fixed_joints[x].destroyed && !g.blood_wires.iter().any(|w| w.joint == x) && !g.pins.iter().any(|p| p.Joint == Some(x))).count();
                let mut records: Vec<&Record> = s.of("SliderJoint2D").collect();
                records.sort_by_key(|r| std::cmp::Reverse(r.id));
                records.get(nth).map_or("?".into(), |r| r.id.to_string())
            }
            super::BreakJoint::Spring(l) => s.of("SpringJoint2D").find(|j| j.name == g.physicals[g.limbs[*l].PhysicalBehaviour].name).map_or("?".into(), |j| j.id.to_string()),
            k => format!("{k:?}"),
        }).collect();
        let theirs = r.get("ids").unwrap_or("");
        // A joint on a disintegrated limb keeps its place in the list after its row leaves the recording.
        let listed: Vec<&str> = theirs.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()).collect();
        let gone = |id: &&str| !s.of("HingeJoint2D").any(|r| r.id.to_string() == **id);
        if listed.len() != ours.len() || !listed.iter().zip(&ours).all(|(t, o)| t == o || (o == "?" && gone(t))) {
            out.push(format!("JointBreakLimits: game {theirs} ours [{}]", ours.join(";")));
        }
    }
    let mut particles = Vec::new();
    let mut check = |what: String, fields: Vec<(&'static str, &mut dyn Field)>, r: &Record| {
        for (name, field) in fields {
            let ours = field.show();
            match r.get(name) {
                Some(theirs) if theirs == ours => {}
                Some(theirs) => out.push(format!("{what}.{name}: game {theirs} ours {ours}")),
                None if FIELDS_45.contains(&name) => {}
                None => out.push(format!("{what}.{name}: not dumped")),
            }
        }
    };
    // By the recorded id (a pasted item's PhysicalBehaviours come before the human's in the game's list), else the
    // next live one in order.
    let live: Vec<usize> = (0..g.physicals.len()).filter(|&p| !g.physicals[p].destroyed && g.physicals[p].in_world).collect();
    let mut used = vec![false; live.len()];
    let mut next = 0;
    for r in s.of("PhysicalBehaviour") {
        let k = match live.iter().position(|&p| g.physicals[p].id == r.id).filter(|&k| !used[k]) {
            Some(k) => k,
            None => {
                while next < live.len() && (used[next] || s.of("PhysicalBehaviour").any(|x| x.id == g.physicals[live[next]].id)) {
                    next += 1;
                }
                if next == live.len() {
                    break;
                }
                next
            }
        };
        used[k] = true;
        check(format!("Physical {}", r.name), g.physicals[live[k]].fields(), r);
    }
    for (l, r) in g.limbs.iter_mut().zip(s.of("LimbBehaviour")) {
        check(format!("Limb {}", r.name), l.fields(), r);
    }
    for (c, r) in g.circulations.iter_mut().zip(s.of("CirculationBehaviour")) {
        check(format!("Circulation {}", r.name), c.fields(), r);
    }
    for (x, r) in g.syringes.iter_mut().zip(s.of("SyringeBehaviour")) {
        check(format!("Syringe {}", r.name), x.fields(), r);
    }
    for (n, r) in g.nodes.iter_mut().zip(s.of("ConnectedNodeBehaviour")) {
        check(format!("Node {}", r.name), n.fields(), r);
    }
    for (k, r) in g.skins.iter_mut().zip(s.of("SkinMaterialHandler")) {
        check(format!("Skin {}", r.name), k.fields(), r);
    }
    for (x, r) in g.gforces.iter_mut().zip(s.of("GForceMeasureBehaviour")) {
        check(format!("GForce {}", r.name), x.fields(), r);
    }
    for (x, r) in g.grips.iter_mut().zip(s.of("GripBehaviour")) {
        check(format!("Grip {}", r.name), x.fields(), r);
    }
    // The containers' liquids, in dictionary order.
    let mut liquid_diffs = Vec::new();
    let liquids = |d: &super::CsDict<usize, RefFloat>| -> Vec<(usize, u32, u32)> { d.iter().map(|(k, v)| (*k, v.Previous.to_bits(), v.raw.to_bits())).collect() };
    let containers = s.of("CirculationBehaviour").zip(g.circulations.iter().map(|c| &c.base)).chain(s.of("SyringeBehaviour").zip(g.syringes.iter().map(|x| &x.base)));
    for (r, base) in containers {
        let theirs: Vec<(usize, u32, u32)> = liquid_distribution(r).unwrap().iter().map(|(k, v)| (*k, v.Previous.to_bits(), v.raw.to_bits())).collect();
        if theirs != liquids(&base.LiquidDistribution) {
            liquid_diffs.push(format!("{} {} liquids: game {theirs:x?} ours {:x?}", r.ty, r.name, liquids(&base.LiquidDistribution)));
        }
    }
    for (x, r) in g.syringes.iter().zip(s.of("SyringeBehaviour")) {
        let ours: Vec<&str> = x.pushTargets.iter().flatten().map(|o| g.physicals[g.container_physical(*o)].name.as_str()).collect();
        if r.get("pushTargets") != Some(format!("[{}]", ours.join(";")).as_str()) {
            liquid_diffs.push(format!("Syringe {} pushTargets: game {:?} ours {ours:?}", r.name, r.get("pushTargets")));
        }
    }
    // The recorder's order: the person's hierarchy (each limb's bleeding particles, their extraDecals splats),
    // then the other splats in creation order.
    let bleeding = bleeding_rows(g);
    let mut splats: Vec<usize> = bleeding.iter().filter_map(|&b| g.bleeding[b].BloodSplatController).collect();
    // A bleeding particle gone this frame: its splat keeps its row with the other bleeding ones until the clone goes.
    splats.extend(g.bleeding.iter().filter(|b| b.CirculationBehaviour.is_some_and(|l| g.limbs[l].Person == 0)).filter_map(|b| b.BloodSplatController).filter(|i| !splats.contains(i)).collect::<Vec<_>>());
    splats.extend((0..g.splats.len()).filter(|i| !splats.contains(i)).collect::<Vec<_>>());
    splats.retain(|&i| !g.splats[i].destroyed || g.splats_destroying.contains(&i));
    let inactive: Vec<usize> = g.blood_puffs.iter().filter(|b| !(b.active && b.emitter_active)).map(|b| b.splat).collect();
    // A splat a prefab instantiated (its component id known) pairs with its record by id; the rest in row order.
    let recs: Vec<&Record> = s.of("FancyBloodSplatController").collect();
    let by_id: Vec<(usize, &Record)> = splats.iter().filter_map(|&i| Some((i, *recs.iter().find(|r| g.splats[i].id != 0 && r.id == g.splats[i].id)?))).collect();
    let rest = splats.iter().copied().filter(|i| !by_id.iter().any(|x| x.0 == *i)).zip(recs.iter().copied().filter(|r| !by_id.iter().any(|x| x.1.id == r.id)));
    let pairs: Vec<(usize, &Record)> = by_id.iter().copied().chain(rest).collect();
    for (i, r) in pairs {
        if let Some(a) = r.get("active") {
            if (a == "1") == inactive.contains(&i) {
                particles.push(format!("Splat {}.active: game {a}", r.id));
            }
        }
        let x = &mut g.splats[i];
        check(format!("Splat {}", r.id), x.fields(), r);
        let theirs = r.get("particles").unwrap_or("");
        if theirs.contains("free=") {
            let slots = x.particles.slots.iter().map(|p| match p {
                Some(p) => [p.VelocityX, p.VelocityY, p.PositionX, p.PositionY, p.Life].map(|f| format!("{:08x}", f.to_bits())).join(","),
                None => "-".into(),
            });
            let free = x.particles.free.iter().rev().map(|i| i.to_string());
            let ours = format!("[{}]free={}", slots.collect::<Vec<_>>().join(";"), free.collect::<Vec<_>>().join(","));
            if theirs != ours {
                particles.push(format!("Splat {}.particles: game {theirs} ours {ours}", r.id));
            }
        }
    }
    for (&b, r) in bleeding.iter().zip(s.of("BleedingParticleBehaviour")) {
        check(format!("Bleeding {}", r.id), g.bleeding[b].fields(), r);
    }
    let mut pose_diffs = Vec::new();
    for (x, r) in g.people.iter_mut().zip(s.of("PersonBehaviour")) {
        check(format!("Person {}", r.name), x.fields(), r);
        // Poses as the recorder lists them (Dump.Format: each RagdollPose by its Name, ' ' as '_'; batch3 r3 recordings
        // also `/rigidity=<hex>/angles=[<limb id>:<Animated>:<Angle>:<StartAngle>:<EndAngle>,..]`).
        if let Some(want) = r.get("Poses") {
            let full = want.contains("/rigidity=");
            let h = |v: f32| format!("{:08x}", v.to_bits());
            let ours = format!("[{}]", x.Poses[x.PoseStart..].iter().map(|q| {
                let name = format!("RagdollPose:\"{}\"", q.Name.replace(' ', "_"));
                if !full {
                    return name;
                }
                let angles = q.Angles.iter().map(|a| format!("{}:{}:{}:{}:{}", g.limbs[a.Limb].id, a.Animated as u8, h(a.Angle), h(a.StartAngle), h(a.EndAngle))).collect::<Vec<_>>().join(",");
                format!("{name}/rigidity={}/angles=[{angles}]", h(q.Rigidity))
            }).collect::<Vec<_>>().join(";"));
            if ours != want {
                pose_diffs.push(format!("Person {} Poses: game {want} ours {ours}", r.name));
            }
        }
    }
    for (x, r) in g.ambient_audio.iter_mut().zip(ambient_records(s)) {
        check(format!("PlayAudioAtRandom {}", r.name), x.fields(), r);
    }
    // A destroyed map's lights (MapLoaderBehaviour.Load's, deactivated on the spot) are not in the game's snapshot.
    // (An inactive object's scripts: the recorder leaves them out.)
    let gone: HashSet<i32> = g.unsynced.iter().chain(&g.retained)
        .flat_map(|i| (0..i.objects.len()).filter(move |&o| std::iter::successors(Some(o), |&x| i.objects[x].parent).any(|x| !i.objects[x].active)).flat_map(move |o| i.objects[o].components.iter().copied()))
        .chain(g.map_lights.iter().map(|x| x.id).filter(|id| g.destroyed_components.contains(id)))
        .collect();
    // In order, both sides without them (a snapshot merged over an earlier one can still hold their records).
    // By id when every light has its record (an item clone's lights come in the item's order, not the map's).
    let ours: Vec<usize> = (0..g.map_lights.len()).filter(|&i| !gone.contains(&g.map_lights[i].id)).collect();
    let by_id: Option<Vec<(usize, &Record)>> = ours.iter().map(|&i| s.of("MapLightBehaviour").find(|r| r.id == g.map_lights[i].id).map(|r| (i, r))).collect();
    let lights: Vec<(usize, &Record)> = by_id.unwrap_or_else(|| ours.iter().copied().zip(s.of("MapLightBehaviour").filter(|r| !gone.contains(&r.id))).collect());
    for &(i, r) in &lights {
        check(format!("MapLight {}", r.name), g.map_lights[i].fields(), r);
    }
    for (x, r) in lights.iter().map(|&(i, r)| (&g.map_lights[i], r)) {
        let theirs = r.get("wake").unwrap_or("");
        let ours = x.wake.map_or("none".to_string(), |w| format!("{:016x}", w.to_bits()));
        if theirs != ours {
            out.push(format!("MapLight {}.wake: game {theirs} ours {ours}", r.name));
        }
    }
    // Hinge motors, limits and break values.
    // Rigidbody2D, b2Body and HingeJoint2D records leave out inactive objects (a crushed limb); the scripts stay.
    let limb_named = |name: &str| g.limbs.iter().position(|l| g.physicals[l.PhysicalBehaviour].name == name);
    // The wires' joints, in the order they were laid.
    // (A map wire's joint is the map's own, a MapDistanceJoint2D record: adopted.)
    let map_joints: HashSet<usize> = s.of("MapDistanceJoint2D").chain(s.of("MapFixedJoint2D")).filter_map(|r| g.adopted_joints.get(&r.id).copied()).collect();
    let wires: Vec<&super::blood_wire::BloodWire> = g.blood_wires.iter().filter(|w| !g.destroyed_components.contains(&w.id) && !g.fixed_joints.get(w.joint).is_some_and(|j| j.destroyed) && !map_joints.contains(&w.joint)).collect();
    for (kind, fixed, impulse) in [("FixedJoint2D", true, 9), ("DistanceJoint2D", false, 8)] {
        // With the hands' joints, oldest first as the records' ids.
        let mut ours: Vec<usize> = wires.iter().filter(|w| w.kind.is_fixed() == fixed && !w.kind.is_spring() && !w.kind.is_slider()).map(|w| w.joint).collect();
        // Wires sharing one joint (UseWireTool.CreateWire on a wired joint): the joint once.
        let mut seen = HashSet::new();
        ours.retain(|j| seen.insert(*j));
        if fixed {
            // (An Adhesive Coupler's StickTo joints too: AddComponent<FixedJoint2D> on the coupler. A Sticky Grenade's
            // CreateJoint the same on the grenade.)
            let coupler = g.couplers.iter().flat_map(|c| c.connections.iter().map(|x| x.joint));
            let sticky = g.sticky_grenades.iter().filter_map(|x| x.joint);
            let sticky = sticky.chain(g.v45.crossbows.iter().filter_map(|x| x.syringeJoint));
            ours.extend(g.grips.iter().filter_map(|x| x.joint).chain(g.loose_fixed_joints.iter().copied()).chain(coupler).chain(sticky).filter(|&f| !g.fixed_joints[f].destroyed));
            ours.sort_unstable();
            ours.dedup();
        }
        // A pasted object's joints are recorded as Item records.
        let item = format!("Item{kind}");
        // A prefab's own joint (an adopted one) is not a wire's.
        let mut records: Vec<&Record> = s.of(kind).chain(s.of(&item)).filter(|r| !g.anchored_joints.iter().any(|a| a.component == r.id)).collect();
        if ours.len() != records.len() {
            out.push(format!("{kind} count: game {} ours {}", records.len(), ours.len()));
            continue;
        }
        records.sort_by_key(|r| std::cmp::Reverse(r.id));
        for (&joint, r) in ours.iter().zip(records) {
            let j = &g.fixed_joints[joint];
            let (a, c): ([f32; 2], [f32; 2]) = (floats(r.req("anchor").unwrap()), floats(r.req("connectedAnchor").unwrap()));
            let mut fields = vec![
                ("anchor.x", a[0].to_bits(), j.state.anchor.x.to_bits()),
                ("anchor.y", a[1].to_bits(), j.state.anchor.y.to_bits()),
                ("connectedAnchor.x", c[0].to_bits(), j.state.connected_anchor.x.to_bits()),
                ("connectedAnchor.y", c[1].to_bits(), j.state.connected_anchor.y.to_bits()),
                ("breakForce", f(r.req("breakForce").unwrap()).to_bits(), j.state.break_force.to_bits()),
                ("breakTorque", f(r.req("breakTorque").unwrap()).to_bits(), j.state.break_torque.to_bits()),
            ];
            // The flags (recordings from fnw18 on): OnPreFlip turns them off, OnFlip's next fixed update on.
            if let Some(v) = r.get("autoConfigureConnectedAnchor") {
                fields.push(("autoConfigureConnectedAnchor", (v == "1") as u32, j.state.auto_configure_connected_anchor as u32));
            }
            if let (Some(v), Some(d)) = (r.get("autoConfigureDistance"), &j.distance) {
                fields.push(("autoConfigureDistance", (v == "1") as u32, d.auto_configure_distance as u32));
            }
            let raw = r.req("joint").unwrap();
            if let (Some(id), true) = (j.joint, raw != "null") {
                let word = words(raw);
                let joint = g.world.joints[id].as_ref().unwrap();
                if fixed {
                    let weld = joint.weld();
                    for (k, name) in [(0, "impulse.x"), (1, "impulse.y"), (2, "impulse.z")] {
                        fields.push((name, word[impulse + k], weld.impulse[k].to_bits()));
                    }
                    fields.push(("referenceAngle", word[7], weld.def.reference_angle.to_bits()));
                } else if let crate::b2::world::JointKind::Rope(rope) = &joint.kind {
                    // maxDistanceOnly: a b2RopeJoint, m_maxLength then m_length, m_impulse, m_u.
                    fields.push(("maxLength", word[4], rope.def.max_length.to_bits()));
                    fields.push(("length", word[5], rope.length.to_bits()));
                    fields.push(("impulse", word[6], rope.impulse.to_bits()));
                    fields.push(("u.x", word[7], rope.u.x.to_bits()));
                    fields.push(("u.y", word[8], rope.u.y.to_bits()));
                } else {
                    fields.push(("impulse", word[impulse], joint.distance().impulse.to_bits()));
                }
            } else if j.joint.is_some() != (raw != "null") {
                out.push(format!("{kind} {}: game {raw} ours {:?}", r.name, j.joint));
            }
            for (name, theirs, ours) in fields {
                if theirs != ours {
                    out.push(format!("{kind} {}.{name}: game {theirs:08x} ours {ours:08x}", r.name));
                }
            }
        }
    }
    // The spring wires' SpringJoint2D (a b2DistanceJoint: frequency, damping, anchors at words 3-6, impulse 8, length 9),
    // the records no GoreStringBehaviour holds, oldest first.
    let gore: Vec<i32> = s.of("GoreStringBehaviour").filter_map(|r| reference(r.get("Joint")?)).collect();
    // A wire started on the item has its SpringJoint2D there (an ItemSpringJoint2D on none of the clone's prefab objects).
    let prefab_comps: String = s.of("Clone").filter_map(|r| r.get("comps")).collect();
    let mut records: Vec<&Record> = s.of("SpringJoint2D").filter(|r| !gore.contains(&r.id))
        .chain(s.of("ItemSpringJoint2D").filter(|r| !prefab_comps.contains(&format!("SpringJoint2D:{};", r.id)) && !prefab_comps.contains(&format!("SpringJoint2D:{}]", r.id)))).collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.id));
    let spring_wires: Vec<_> = wires.iter().filter(|w| w.kind.is_spring()).collect();
    // Each wire's own typedJoint when every wire record names one among them (a joint SpringCableBehaviour.Start remade is
    // younger than the other wires').
    let own: Option<Vec<&Record>> = spring_wires.iter().map(|w| s.records.iter().find(|r| r.id == w.id && fnweap::is_wire_record(r)).and_then(|r| reference(r.get("typedJoint")?)).and_then(|jid| records.iter().find(|r| r.id == jid).copied())).collect();
    if let Some(own) = own.filter(|o| o.len() == records.len()) {
        records = own;
    }
    let springs: Vec<usize> = spring_wires.iter().map(|w| w.joint).collect();
    if springs.len() != records.len() {
        out.push(format!("SpringJoint2D wire count: game {} ours {}", records.len(), springs.len()));
    } else {
        for (&joint, r) in springs.iter().zip(records) {
            let j = &g.fixed_joints[joint];
            let mut fields = vec![("breakForce", f(r.req("breakForce").unwrap()).to_bits(), j.state.break_force.to_bits())];
            // The component's own fields (recordings from fnba12 on).
            if let (Some(d), Some(a), Some(sp)) = (r.get("distance"), r.get("autoConfigureDistance"), j.spring.as_ref()) {
                fields.push(("distance", f(d).to_bits(), sp.distance.to_bits()));
                fields.push(("frequencyField", f(r.req("frequency").unwrap()).to_bits(), sp.frequency.to_bits()));
                fields.push(("autoConfigureDistance", (a == "1") as u32, sp.auto_configure_distance as u32));
            }
            if let Some(id) = j.joint {
                let w = words(r.req("joint").unwrap());
                let d = g.world.joints[id].as_ref().unwrap().distance();
                for (k, name, ours) in [(0, "frequency", d.def.frequency_hz), (1, "damping", d.def.damping_ratio), (3, "anchorA.x", d.def.local_anchor_a.x), (4, "anchorA.y", d.def.local_anchor_a.y), (5, "anchorB.x", d.def.local_anchor_b.x), (6, "anchorB.y", d.def.local_anchor_b.y), (8, "impulse", d.impulse), (9, "length", d.def.length)] {
                    fields.push((name, w[k], ours.to_bits()));
                }
            }
            for (name, theirs, ours) in fields {
                if theirs != ours {
                    out.push(format!("SpringJoint2D {}.{name}: game {theirs:08x} ours {ours:08x}", r.name));
                }
            }
        }
    }
    // The recorded person's (the first) and the pins on its limbs (Human.GetComponentsInChildren); a spawned human's are
    // ItemHingeJoint2D records.
    let live_hinges = g.limbs.iter().filter(|l| l.Person == 0 && g.physicals[l.PhysicalBehaviour].active).filter_map(|l| l.Joint).filter(|&h| !g.hinges[h].destroyed).count();
    // The added HingeJoint2Ds on its limbs (a pin's, one per PinBehaviour.CreateJoint, kept when it runs again), in creation order.
    let on_limb = |o: &BodyOwner| matches!(*o, BodyOwner::Physical(x) if g.physicals[x].active && g.limbs.iter().any(|l| l.Person == 0 && l.PhysicalBehaviour == x));
    let added: Vec<usize> = (0..g.fixed_joints.len()).filter(|&f| g.fixed_joints[f].hinge.is_some() && !g.fixed_joints[f].destroyed && on_limb(&g.fixed_joints[f].attached)).collect();
    let live_hinges = live_hinges + added.len();
    if live_hinges != s.of("HingeJoint2D").count() {
        out.push(format!("HingeJoint2D count: game {} ours {live_hinges}", s.of("HingeJoint2D").count()));
    }
    let mut seen = std::collections::HashMap::new();
    for r in s.of("HingeJoint2D") {
        let nth = seen.entry(r.name.as_str()).or_insert(0usize);
        *nth += 1;
        if *nth > 1 {
            let Some(&fj) = added.iter().filter(|&&f| matches!(g.fixed_joints[f].attached, BodyOwner::Physical(x) if g.physicals[x].name == r.name)).nth(*nth - 2) else {
                out.push(format!("Pin hinge {}: ours missing", r.name));
                continue;
            };
            let state = &g.fixed_joints[fj].state;
            // HingeJoint2D::Reset's motor and limits: 10000 and 0..359.
            let fields = [("motor", "0,00000000,461c4000".to_string()), ("limits", "0,00000000,43b38000".to_string()), ("breakForce", state.break_force.show()), ("breakTorque", state.break_torque.show())];
            for (k, ours) in fields {
                let theirs = r.get(k).unwrap_or("");
                if theirs != ours {
                    out.push(format!("Pin hinge {}.{k}: game {theirs} ours {ours}", r.name));
                }
            }
            continue;
        }
        let Some(h) = limb_named(&r.name).and_then(|l| g.limbs[l].Joint) else {
            out.push(format!("Hinge {}: ours missing", r.name));
            continue;
        };
        let h = &g.hinges[h];
        let motor = format!("{},{},{}", h.useMotor as i32, h.motor[0].show(), h.motor[1].show());
        let limits = format!("{},{},{}", h.useLimits as i32, h.limits[0].show(), h.limits[1].show());
        for (k, ours) in [("motor", motor), ("limits", limits), ("breakForce", h.breakForce.show()), ("breakTorque", h.breakTorque.show())] {
            let theirs = r.get(k).unwrap_or("");
            if theirs != ours {
                out.push(format!("Hinge {}.{k}: game {theirs} ours {ours}", r.name));
            }
        }
    }
    // Body velocities and forces the scripts write.
    for r in s.of("b2Body").chain(s.of("ItemB2Body")) {
        // An item body by its Rigidbody2D (a tree has several "Leaves (21)", its leaves have no PhysicalBehaviour), else by name.
        let item = s.of("ItemB2Body").any(|i| i.id == r.id);
        let by_rb = || g.retained.iter().chain(&g.unsynced).find_map(|inst| inst.objects.iter().position(|o| o.components.contains(&r.id)).and_then(|o| inst.bodies.get(&o).copied()));
        let dead = item && g.physicals.iter().any(|x| x.rigidbody.component == r.id && x.destroyed);
        // An item's physical by its Rigidbody2D first (two same-named Wheels), else by name.
        let p = g.physicals.iter().position(|x| item && x.rigidbody.component == r.id && !x.destroyed)
            .or_else(|| g.limbs.iter().map(|l| l.PhysicalBehaviour).chain(g.physicals.iter().position(|x| x.name.replace(' ', "_") == r.name && item)).find(|&p| g.physicals[p].name.replace(' ', "_") == r.name && !g.physicals[p].destroyed));
        let Some(body) = (if item { by_rb() } else { None }).or(p.map(|p| g.physicals[p].rigidbody.body)).filter(|_| !dead) else {
            out.push(format!("b2Body {}: ours destroyed", r.name));
            continue;
        };
        let Some(b) = g.world.bodies[body].as_ref() else {
            out.push(format!("b2Body {}: ours removed", r.name));
            continue;
        };
        let w = words(r.req("body").unwrap());
        let ours = [b.flags as u32, b.linear_velocity.x.to_bits(), b.linear_velocity.y.to_bits(), b.angular_velocity.to_bits(), b.force.x.to_bits(), b.force.y.to_bits(), b.torque.to_bits()];
        let theirs = [w[1] & 0xffff, w[16], w[17], w[18], w[19], w[20], w[21]];
        if ours != theirs {
            out.push(format!("b2Body {}: game {theirs:08x?} ours {ours:08x?}", r.name));
        }
        if let Some(m) = r.get("move") {
            // An item's state is its body's own (the Truck's "Head" is not the human's).
            let p = if item { g.physicals.iter().position(|x| x.rigidbody.body == body && !x.destroyed) } else { p };
            let w = words(m);
            let theirs = [&[(w[0] != u32::MAX) as u32], &w[4..25], &[w[31] & 0xff]].concat();
            // The physical whose body this is (an item found by its instance's body has no `p`).
            let p = p.or_else(|| g.physicals.iter().position(|x| x.rigidbody.body == body && !x.destroyed));
            let ours = movement_words(&p.map(|p| g.physicals[p].rigidbody.movement.clone()).or_else(|| g.node_rigidbodies.iter().find(|n| n.rigidbody.component == r.id).map(|n| n.rigidbody.movement.clone())).unwrap_or_default());
            if ours != theirs {
                out.push(format!("RigidbodyMovementState2D {}: game {theirs:08x?} ours {ours:08x?}", r.name));
            }
        }
        let sleep = words(r.req("mass").unwrap())[9];
        if sleep != b.sleep_time.to_bits() {
            out.push(format!("b2Body {}.sleepTime: game {sleep:08x} ours {:08x}", r.name, b.sleep_time.to_bits()));
        }
    }
    out.extend(liquid_diffs);
    out.extend(pose_diffs);
    out.extend(compare_stabs(g, s));
    let w = words(s.snap("random"));
    if g.random.state != [w[0], w[1], w[2], w[3]] {
        out.push(format!("random: game {w:08x?} ours {:08x?}", g.random.state));
    }
    // SpriteRenderer.bounds from the sprite's local AABB and our transforms.
    for r in s.of("PhysicalBehaviour") {
        let (Some(theirs), Some(p)) = (r.get("rbounds"), g.physicals.iter().position(|x| x.id == r.id)) else { continue };
        if g.physicals[p].sprite_bounds.is_none() {
            continue;
        }
        let (c, e) = g.sprite_world_bounds(p);
        let ours = [c, e].concat().iter().map(|v| format!("{:08x}", v.to_bits())).collect::<Vec<_>>().join(",");
        if theirs != ours {
            particles.push(format!("{}.bounds: game {theirs} ours {ours}", r.name));
        }
        // `sort=<sortingLayerID>,<sortingOrder>` (recordings since the layer editor check).
        if let Some(theirs) = r.get("sort") {
            let (layer, order) = g.serialised_layer_get_sorting_for(p);
            if theirs != format!("{layer},{order}") {
                out.push(format!("{}.sorting: game {theirs} ours {layer},{order}", r.name));
            }
        }
    }
    out.extend(particles);
    out
}

/// The stab bookkeeping against the PhysicalBehaviour records (stabWoundCount, beingStabbedBy, penetrations and
/// victimPenetrations as the recorder formats them) and the SliderJoint2D records (Joint2D values, translation, speed).
/// The links' hover state (PPG_HOVER): Hover.IsMouseInsideBounds and IsMouseOverlapping of each LinkDeviceBehaviour and end
/// VisualDeletableDetachedToolBehaviour, and what their OnMouseOverlapEvent shows (the line's enabled, material and layer; each
/// end sprite's colour and layer). A link destroyed in the rewrite but not in the game (or the other way) shows as missing.
fn compare_links(g: &Game, s: &Snapshot, out: &mut Vec<String>) {
    let db = g.assets.as_deref();
    let name = |r: Option<crate::assets::db::ObjRef>| match (db, r) {
        (Some(db), Some(r)) => crate::play::object_name(db, r),
        _ => "?".into(),
    };
    let bit = |b: bool| if b { "1" } else { "0" };
    let floats = |c: [f32; 4]| c.map(|v| format!("{:08x}", v.to_bits())).join(",");
    // A loaded recording's instance ids are not the game's, so the live links pair up in creation order. A link before its Start
    // (or whose Start's Destroy(this) is still pending, end of frame) has its record too; only started ones have ends.
    let live: Vec<_> = g.phase_links.iter().filter(|l| !g.destroyed_components.contains(&l.id)).collect();
    let ended: Vec<_> = live.iter().copied().filter(|l| l.started).collect();
    // A link loaded from the record keeps its id: paired by id; the rest (made in the rewrite) in creation order.
    let mut rest = live.iter().copied().filter(|l| !s.of("LinkDeviceBehaviour").any(|r| r.id == l.id));
    for r in s.of("LinkDeviceBehaviour") {
        let Some(l) = live.iter().copied().find(|l| l.id == r.id).or_else(|| rest.next()) else {
            out.push(format!("LinkDeviceBehaviour {}: missing", r.id));
            continue;
        };
        let line = l.line.map_or("null".into(), |i| format!("{},{},{}", bit(g.lines[i].enabled), name(g.lines[i].material), g.lines[i].game_layer));
        let ours = format!("inside={} over={} line={line}", bit(l.IsMouseInsideBounds), bit(l.IsMouseOverlapping));
        let theirs = format!("inside={} over={} line={}", r.get("inside").unwrap_or("?"), r.get("over").unwrap_or("?"), r.get("line").unwrap_or("?"));
        if ours != theirs {
            out.push(format!("LinkDeviceBehaviour {}: game {theirs} ours {ours}", r.id));
        }
    }
    for (k, r) in s.of("VisualDeletableDetachedToolBehaviour").enumerate() {
        let end = usize::from(r.name == "target");
        let Some(&l) = ended.get(k / 2) else {
            out.push(format!("VisualDeletableDetachedToolBehaviour {}: missing", r.id));
            continue;
        };
        let sprite = l.end_renderers[end].map_or("?".into(), |i| format!("{},{}", floats(g.renderers[i].sprite.1), g.renderers[i].game_layer));
        let ours = format!("inside={} over={} sprite={sprite}", bit(l.end_inside[end]), bit(l.end_overlapping[end]));
        let theirs = format!("inside={} over={} sprite={}", r.get("inside").unwrap_or("?"), r.get("over").unwrap_or("?"), r.get("sprite").unwrap_or("?"));
        if ours != theirs {
            out.push(format!("VisualDeletableDetachedToolBehaviour {}: game {theirs} ours {ours}", r.id));
        }
    }
    let live = live.len();
    // The pins' (PPG_HOVER without a link): Hover.IsMouseOverlapping and ToolInvisibilityTracker's look of the sprite (enabled; the
    // DeleteWire material on the ScreenUI layer while `delete_look`, else its own). In creation order, like the links.
    // An unstarted pin (PinTool.CreatePin's AddComponent, Start not run yet) has its record too: sprite=null.
    let pins: Vec<_> = g.pins.iter().filter(|p| !g.destroyed_components.contains(&p.id)).collect();
    for (k, r) in s.of("PinBehaviour").enumerate() {
        // By id (a CreatePin case's new pin sits among the loaded ones), else in creation order (a replay's own ids).
        let Some(&p) = pins.iter().find(|p| p.id == r.id).or_else(|| pins.get(k)) else {
            out.push(format!("PinBehaviour {}: missing", r.id));
            continue;
        };
        let sprite = p.renderer.map_or("null".into(), |i| {
            let (m, layer) = if p.tracker.delete_look { ("DeleteWire".into(), super::tool_invisibility::SCREEN_UI_LAYER) } else { (g.renderers[i].material.map_or("Sprites-Default".into(), |m| name(Some(m))), g.renderers[i].game_layer) };
            format!("{},{m},{layer}", bit(g.renderers[i].enabled))
        });
        let ours = format!("over={} sprite={sprite}", bit(p.IsMouseOverlapping));
        let theirs = format!("over={} sprite={}", r.get("over").unwrap_or("?"), r.get("sprite").unwrap_or("?"));
        if ours != theirs {
            out.push(format!("PinBehaviour {}: game {theirs} ours {ours}", r.id));
        }
    }
    if s.of("Time").next().is_some() && live != s.of("LinkDeviceBehaviour").count() && s.of("LinkDeviceBehaviour").next().is_some() {
        out.push(format!("LinkDeviceBehaviour: game {} ours {live}", s.of("LinkDeviceBehaviour").count()));
    }
}

fn compare_stabs(g: &Game, s: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let hex = |v: f32| format!("{:08x}", v.to_bits());
    // The recorder writes names with underscores for spaces ("Crossbow Bolt", a launched projectile's asset name).
    let named = |p: usize| g.physicals[p].name.replace(' ', "_");
    let at = |p: usize| if g.physicals[p].destroyed { "destroyed".to_string() } else { format!("@{}#{}", named(p), g.physicals[p].id) };
    let collider = |c: i32| format!("@{}#{c}", g.physical_of_collider(c).map_or("?".to_string(), named));
    let pen_text = |i: usize| {
        let p = &g.penetration_arena[i];
        let a = &p.Axis;
        let joint = match p.Joint {
            None => "null",
            Some(j) if g.fixed_joints[j].destroyed => "destroyed",
            Some(_) => "@",
        };
        format!(
            "Penetration(Stabber={} Victim={} Joint={joint} Duration={} Axis=SharpAxis(Axis={},{} LowerLimit={} UpperLimit={} LooseUpperLimit={} LooseLowerLimit={}) RelevantColliders=[{}] CurrentConnectedAnchor={},{} Active={})",
            at(p.Stabber),
            p.Victim.map_or("null".to_string(), at),
            hex(p.Duration),
            hex(a.Axis[0]),
            hex(a.Axis[1]),
            hex(a.LowerLimit),
            hex(a.UpperLimit),
            a.LooseUpperLimit as u8,
            a.LooseLowerLimit as u8,
            p.RelevantColliders.iter().map(|&c| collider(c)).collect::<Vec<_>>().join(";"),
            hex(p.CurrentConnectedAnchor[0]),
            hex(p.CurrentConnectedAnchor[1]),
            p.Active as u8
        )
    };
    // The game's Joint is "@name#id" of a live component; only whether it is set is compared.
    let loose = |t: &str| -> String {
        let mut o = String::new();
        let mut rest = t;
        while let Some(i) = rest.find("Joint=@") {
            o.push_str(&rest[..i + 7]);
            let end = rest[i..].find(' ').map_or(rest.len(), |e| i + e);
            rest = &rest[end..];
        }
        o + rest
    };
    for r in s.of("PhysicalBehaviour") {
        let Some(p) = g.physicals.iter().position(|x| x.id == r.id) else { continue };
        let ph = &g.physicals[p];
        let join = |v: Vec<String>| format!("[{}]", v.join(";"));
        let mut check = |k: &str, ours: String| {
            if let Some(theirs) = r.get(k) {
                if loose(theirs) != ours {
                    out.push(format!("Physical {}.{k}: game {} ours {ours}", r.name, theirs));
                }
            }
        };
        check("stabWoundCount", ph.stabWoundCount.to_string());
        check("beingStabbedBy", join(ph.beingStabbedBy.iter().map(|&q| at(q)).collect()));
        check("penetrations", join(ph.penetrations.iter().map(|&i| pen_text(i)).collect()));
        check("victimPenetrations", join(ph.victimPenetrations.iter().map(|&i| pen_text(i)).collect()));
    }
    // Sliders made after the load (a wire Start's AddComponent) have no adopted entry: the game's k-th such record of a
    // name pair (ids fall as Unity hands them out) is ours k-th unadopted slider of that pair.
    let adopted: HashSet<usize> = g.adopted_joints.values().copied().collect();
    let mut fresh: HashMap<(String, String), Vec<usize>> = HashMap::new();
    let mut taken: HashMap<(String, String), usize> = HashMap::new();
    let mut records: Vec<_> = s.of("SliderJoint2D").collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.id));
    for r in records {
        let name = |t: &str| t.trim_start_matches('@').rsplit_once('#').map_or(t.to_string(), |x| x.0.to_string());
        let connected = name(r.get("connected").unwrap_or("null"));
        let pair_of = |f: usize| {
            let j = &g.fixed_joints[f];
            !j.destroyed && j.slider.is_some() && matches!(j.attached, BodyOwner::Physical(a) if named(a) == r.name) && matches!(j.connected, Some(BodyOwner::Physical(c)) if named(c) == connected)
        };
        let key = (r.name.clone(), connected.clone());
        let unadopted = fresh.entry(key.clone()).or_insert_with(|| (0..g.fixed_joints.len()).filter(|f| !adopted.contains(f) && pair_of(*f)).collect());
        let nth = |k: &mut HashMap<(String, String), usize>| {
            let n = k.entry(key.clone()).or_insert(0);
            *n += 1;
            *n - 1
        };
        // The recorded component's own entry first: two lifts' sliders share their names.
        let Some(f) = g.adopted_joints.get(&r.id).copied().filter(|&f| !g.fixed_joints[f].destroyed && g.fixed_joints[f].slider.is_some()).or_else(|| unadopted.get(nth(&mut taken)).copied()).or_else(|| (0..g.fixed_joints.len()).find(|&f| pair_of(f))) else {
            out.push(format!("SliderJoint2D {} -> {connected}: ours missing", r.name));
            continue;
        };
        let j = &g.fixed_joints[f];
        let (st, sl) = (&j.state, j.slider.as_ref().unwrap());
        let ours = [
            ("enabled", (j.enabled as u8).to_string()),
            ("anchor", format!("{},{}", hex(st.anchor.x), hex(st.anchor.y))),
            ("connectedAnchor", format!("{},{}", hex(st.connected_anchor.x), hex(st.connected_anchor.y))),
            ("auto", format!("{},{}", st.auto_configure_connected_anchor as u8, sl.auto_configure_angle as u8)),
            ("angle", hex(sl.angle)),
            ("motor", format!("{},{},{}", sl.use_motor as u8, hex(sl.motor_speed), hex(sl.max_motor_torque))),
            ("limits", format!("{},{},{}", sl.use_limits as u8, hex(sl.lower), hex(sl.upper))),
            ("breakForce", hex(st.break_force)),
            ("breakTorque", hex(st.break_torque)),
            ("collide", (st.enable_collision as u8).to_string()),
            ("translation", hex(g.slider_joint_translation(f))),
            ("speed", hex(g.slider_joint_speed(f))),
        ];
        for (k, ours) in ours {
            let theirs = r.get(k).unwrap_or("");
            if theirs != ours {
                out.push(format!("SliderJoint2D {} -> {connected}.{k}: game {theirs} ours {ours}", r.name));
            }
        }
    }
    let live = g.fixed_joints.iter().filter(|j| !j.destroyed && j.slider.is_some()).count();
    if live != s.of("SliderJoint2D").count() {
        out.push(format!("SliderJoint2D count: game {} ours {live}", s.of("SliderJoint2D").count()));
    }
    out
}

/// RigidbodyMovementState2D from the recorder's `move` words (Rigidbody2D+0x98 on).
/// The ToolController's script state and drag joint against a snapshot's records.
fn compare_tool(g: &Game, s: &Snapshot) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    // gapC r17: the PhaseLinkTool's WireWidth/WireColor (PhaseLinkTool.Awake writes them), when the component is recorded.
    if let Some(w) = s.of("PhaseLinkTool").next() {
        let hex = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
        for (k, ours) in [("WireWidth", hex(&[g.link_tool.WireWidth])), ("WireColor", hex(&g.link_tool.WireColor))] {
            if let Some(v) = w.get(k).filter(|v| **v != ours) { out.push(format!("PhaseLinkTool {k}: game {v} ours {ours}")) }
        }
    }
    let (Some(t), Some(r)) = (g.tool.as_ref(), s.of("ToolControllerBehaviour").next()) else { return Ok(out) };
    // Only CurrentTool is recorded.
    let Some(d) = s.of("DragTool").next() else { return Ok(out) };
    let sel = s.of("Selection").next().ok_or("no Selection")?;
    // A reference to a destroyed object is recorded as "destroyed": Unity's fake null, compared as null.
    let name = |p: Option<usize>| p.map_or("null".to_string(), |p| if g.physicals[p].destroyed { "null".into() } else { g.physicals[p].name.clone() });
    let reffed = |v: &str| if v == "null" || v == "destroyed" { "null".to_string() } else { v.trim_start_matches('@').rsplit_once('#').unwrap().0.to_string() };
    let mut check = |what: &str, game: String, ours: String| {
        if game != ours {
            out.push(format!("{what}: game {game} ours {ours}"));
        }
    };
    let hex = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
    if let Some((_, m)) = s.snap.iter().find(|(k, _)| k == "mouse") {
        check("Global.MousePosition", m.clone(), hex(&g.MousePosition));
    }
    check("isCurrentlyHolding", r.req("isCurrentlyHolding")?.into(), (t.isCurrentlyHolding as u8).to_string());
    check("currentlyHovering", reffed(r.req("currentlyHovering")?), name(t.currentlyHovering));
    check("isSelecting", r.req("isSelecting")?.into(), (t.isSelecting as u8).to_string());
    check("hover", reffed(sel.req("hovering")?), name(t.hover));
    let selected: Vec<String> = list(sel.req("selected")?).into_iter().filter(|t| *t != "destroyed").map(reffed).collect();
    check("rawSelection", selected.join(";"), t.rawSelection.iter().filter(|&&p| !g.physicals[p].destroyed).map(|&p| name(Some(p))).collect::<Vec<_>>().join(";"));
    check("ActiveSingleSelected", reffed(current_tool(s, d).req("ActiveSingleSelected")?), name(t.ActiveSingleSelected));
    // A wire or link tool's drag state (batch2 r2), when its record has it.
    let ct = current_tool(s, d);
    if let (Some(p), Some(o), Some(h)) = (ct.get("startPos"), ct.get("startOffset"), ct.get("hasDragged")) {
        let (sp, so, hd) = if g.link_tool.chosen { (g.link_tool.startPos, g.link_tool.startOffset, g.link_tool.hasDragged) }
            else if g.belt_tool.chosen { (g.belt_tool.startPos, g.belt_tool.startOffset, g.belt_tool.hasDragged) }
            else { (g.wire_tool.startPos, g.wire_tool.startOffset, g.wire_tool.hasDragged) };
        check("tool startPos", p.into(), hex(&sp));
        check("tool startOffset", o.into(), hex(&so));
        check("tool hasDragged", h.into(), (hd as u8).to_string());
    }
    check("selectedRigidbody", reffed(d.req("selectedRigidbody")?), name(t.selectedRigidbody));
    check("offset", d.req("offset")?.into(), hex(&t.offset));
    check("prevMousePos", d.req("prevMousePos")?.into(), hex(&t.prevMousePos));
    check("snappedMouseDelta", d.req("snappedMouseDelta")?.into(), hex(&t.snappedMouseDelta));
    check("currentRotation", d.req("currentRotation")?.into(), hex(&[t.currentRotation]));
    check("controlRotation", d.req("controlRotation")?.into(), (t.controlRotation as u8).to_string());
    check("initialSnappingFinished", d.req("initialSnappingFinished")?.into(), (t.initialSnappingFinished as u8).to_string());
    if let Some(rb) = s.of("ToolBody").next() {
        check("ToolController position", rb.req("tpos")?.into(), hex(&t.position));
        // gapA r12: the drag body's b2Body velocity, force and torque, as compare() does for the POs' bodies: what
        // DragTool.MoveToMouse (AddForce) and OnFixedHold (velocity *= GetDragFunction) write.
        if let (Some(raw), Some(b)) = (rb.get("body"), g.world.bodies.get(t.body).and_then(|b| b.as_ref())) {
            let w = words(raw);
            let ours = [b.flags as u32, b.linear_velocity.x.to_bits(), b.linear_velocity.y.to_bits(), b.angular_velocity.to_bits(), b.force.x.to_bits(), b.force.y.to_bits(), b.torque.to_bits()];
            check("ToolController b2Body", format!("{:08x?}", [w[1] & 0xffff, w[16], w[17], w[18], w[19], w[20], w[21]]), format!("{ours:08x?}"));
        }
    }
    let game_joint = reference(d.req("dragJoint")?);
    check("dragJoint", game_joint.map_or("null", |_| "joint").into(), t.dragJoint.as_ref().map_or("null", |_| "joint").into());
    if let (Some(id), Some(j)) = (game_joint, t.dragJoint.as_ref()) {
        let h = s.of("ToolJoint").find(|h| h.id == id).ok_or(format!("no ToolJoint #{id}"))?;
        let m = h.req("motor")?;
        check("drag motor", m.into(), format!("{},{}", j.useMotor as u8, hex(&[j.motorSpeed, j.maxMotorTorque])));
        // joint=null: the HingeJoint2D without its b2 joint (its connected Rigidbody2D destroyed).
        let raw = h.req("joint")?;
        let w = if raw == "null" { Vec::new() } else { words(raw) };
        match j.joint {
            None if raw == "null" => {}
            None => check("drag b2 joint", "live".into(), "none".into()),
            Some(_) if raw == "null" => check("drag b2 joint", "none".into(), "live".into()),
            Some(jid) => {
                let def = g.world.revolute(jid).def;
                let ours = [def.local_anchor_a.x, def.local_anchor_a.y, def.local_anchor_b.x, def.local_anchor_b.y];
                check("drag joint anchors", hex(&w[0..4].iter().map(|&x| f32::from_bits(x)).collect::<Vec<_>>()), hex(&ours));
                check("drag joint motor", format!("{},{:08x},{:08x}", w[8] & 0xff, w[9], w[10]), format!("{},{}", def.enable_motor as u8, hex(&[def.max_motor_torque, def.motor_speed])));
                check("drag joint angles", format!("{:08x},{:08x},{:08x}", w[12], w[13], w[14]), hex(&[def.reference_angle, def.lower_angle, def.upper_angle]));
            }
        }
    }
    Ok(out)
}

/// CurrentTool's record (the snapshot's `tool=` class) when it has an ActiveSingleSelected, else the DragTool's `d`.
fn current_tool<'a>(s: &'a Snapshot, d: &'a Record) -> &'a Record {
    s.snap.iter().find(|(k, _)| k == "tool").and_then(|(_, t)| s.of(t).next()).filter(|c| c.get("ActiveSingleSelected").is_some()).unwrap_or(d)
}

/// The ToolController's components from a recording.
fn load_tool(g: &Game, s: &Snapshot, r: &Record, restored: &physics::Restored) -> Result<super::tool::Tool, String> {
    let rb = s.of("ToolBody").next().ok_or("no ToolBody")?;
    let d = s.of("DragTool").next().ok_or("no DragTool")?;
    let sel = s.of("Selection").next().ok_or("no Selection")?;
    let physical = |t: &str| -> Result<Option<usize>, String> {
        match reference(t) {
            None => Ok(None),
            Some(id) => g.physicals.iter().position(|p| p.id == id).map(Some).ok_or(format!("no physical #{id}")),
        }
    };
    // A Rigidbody2D reference: the physical on its object.
    let rigidbody = |t: &str| -> Result<Option<usize>, String> {
        // "destroyed": a Unity-null reference to a body an earlier scenario destroyed.
        if t == "null" || t == "destroyed" {
            return Ok(None);
        }
        let name = t.trim_start_matches('@').rsplit_once('#').unwrap().0;
        g.physicals.iter().position(|p| p.name == name).map(Some).ok_or(format!("no physical {name}"))
    };
    let b = |k: &str| -> Result<bool, String> { Ok(d.req(k)? == "1") };
    let drag_joint = match reference(d.req("dragJoint")?) {
        None => None,
        Some(id) => {
            let h = s.of("ToolJoint").find(|h| h.id == id).ok_or(format!("no ToolJoint #{id}"))?;
            let m = h.req("motor")?;
            let (on, rest) = m.split_once(',').unwrap();
            let [speed, torque]: [f32; 2] = floats(rest);
            let joint = restored.joints.get(&format!("{}#{}", h.name, h.id)).copied();
            Some(super::tool::DragJoint { id, joint, motorSpeed: speed, maxMotorTorque: torque, useMotor: on == "1", destroy_at: None })
        }
    };
    let mask = r.req("SelectionLayer")?;
    let mask = mask.trim_start_matches("LayerMask(m_Mask=").trim_end_matches(')').parse().map_err(|e| format!("SelectionLayer: {e}"))?;
    Ok(super::tool::Tool {
        body: *restored.bodies.get(&format!("ToolController#{}", rb.id)).ok_or("no ToolController body")?,
        position: floats(rb.req("tpos")?),
        rotation: floats(rb.req("trot")?),
        SelectionLayer: mask,
        isCurrentlyHolding: r.req("isCurrentlyHolding")? == "1",
        currentlyHovering: physical(r.req("currentlyHovering")?)?,
        isSelecting: r.req("isSelecting")? == "1",
        selectionDragStart: floats(r.req("selectionDragStart")?),
        // A destroyed entry (an earlier scenario's object, Unity's fake null) is left out (checker limit: the game keeps it in the list).
        rawSelection: list(sel.req("selected")?).into_iter().filter(|t| *t != "destroyed").map(|t| physical(t).map(Option::unwrap)).collect::<Result<_, _>>()?,
        hover: physical(sel.req("hovering")?)?,
        offset: floats(d.req("offset")?),
        dragJoint: drag_joint,
        destroying: Vec::new(),
        controlRotation: b("controlRotation")?,
        selectedRigidbody: rigidbody(d.req("selectedRigidbody")?)?,
        rotationSpeed: f(d.req("rotationSpeed")?),
        currentRotation: f(d.req("currentRotation")?),
        snappedMouseDelta: floats(d.req("snappedMouseDelta")?),
        objectsToMoveInPauseMode: Vec::new(),
        prevMousePos: floats(d.req("prevMousePos")?),
        initialSnappingFinished: b("initialSnappingFinished")?,
        isActive: b("isActive")?,
        shouldSendContinuousUseMessage: b("shouldSendContinuousUseMessage")?,
        initialTranslationSnapOffset: floats(d.req("initialTranslationSnapOffset")?),
        // CurrentTool's (ToolControllerBehaviour sets it on the current tool; the DragTool record keeps its own stale one).
        ActiveSingleSelected: physical(current_tool(s, d).req("ActiveSingleSelected")?)?,
        gentle: false,
        gentle_added: false,
        parentCycleIndex: 0,
        childCycleIndex: 0,
        childCount: 0,
        tab: 0,
        invisible: false,
        chosen: String::new(),
    })
}

fn movement_state(w: &[u32]) -> MovementState {
    let v = |i: usize| f32::from_bits(w[i]);
    let pose = |i: usize| ([v(i), v(i + 1), v(i + 2)], [v(i + 3), v(i + 4), v(i + 5), v(i + 6)]);
    MovementState { previous: pose(4), current: pose(11), body: pose(18), interpolating: w[31] & 0xff != 0, listed: w[0] != u32::MAX, ..Default::default() }
}

/// The inverse of movement_state: listed, words 4-24 and the flag byte.
fn movement_words(m: &MovementState) -> Vec<u32> {
    let mut out = vec![m.listed as u32];
    for (p, q) in [m.previous, m.current, m.body] {
        out.extend(p.iter().chain(&q).map(|f| f.to_bits()));
    }
    out.push(m.interpolating as u32);
    out
}

/// Each limb's Transform against the recording; needs the group objects between limb and root (TransformChain
/// parents, from dump11 on).
fn compare_transforms(g: &Game, chained: &Snapshot, to: &Snapshot) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let chain = chained.of("TransformChain").any(|r| r.get("parent").is_some());
    for rb in to.of("Rigidbody2D").chain(to.of("ItemRigidbody2D")).filter(|_| chain) {
        let item = to.of("ItemRigidbody2D").any(|i| i.id == rb.id);
        // An item's Rigidbody2D by its id (a tree's leaves have no PhysicalBehaviour: their node), else by name.
        let ours = match g.physicals.iter().position(|x| item && x.rigidbody.component == rb.id) {
            Some(p) => (g.physicals[p].transform.position, g.physicals[p].transform.rotation),
            None => match g.node_rigidbodies.iter().find(|n| item && n.rigidbody.component == rb.id) {
                Some(n) => g.node_world(n.node),
                None => {
                    let p = g.limbs.iter().map(|l| l.PhysicalBehaviour).chain(g.physicals.iter().position(|x| x.name == rb.name && item)).find(|&p| g.physicals[p].name == rb.name).ok_or(format!("no limb {}", rb.name))?;
                    (g.physicals[p].transform.position, g.physicals[p].transform.rotation)
                }
            },
        };
        let (pos, rot): ([f32; 3], [f32; 4]) = (floats(rb.req("tpos")?), floats(rb.req("trot")?));
        let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
        if bits(&pos) != bits(&ours.0) || bits(&rot) != bits(&ours.1) {
            out.push(format!("Transform {}: game {:08x?} {:08x?} ours {:08x?} {:08x?}", rb.name, bits(&pos), bits(&rot), bits(&ours.0), bits(&ours.1)));
        }
    }
    Ok(out)
}

/// The physics pass's results at FE: the b2 world, the Collision2D records and each awake body's Transform.
fn compare_physics(g: &mut Game, from: &Snapshot, to: &Snapshot) -> Result<Vec<String>, String> {
    // A second restore of the snapshot names its bodies and fixtures; ours are found by the same Rigidbody2D
    // (ground: none), their fixtures in list order (ids drift once something was destroyed).
    let mut r = physics::restore(from, V::new(-0.0, -9.81))?;
    let ours: HashMap<Option<i32>, crate::b2::world::BodyId> = g.world.body_list.iter().map(|&b| (g.world.body(b).user.map(|u| u.id), b)).collect();
    let fixture_name: HashMap<_, _> = r.fixtures.iter().map(|(n, &f)| (f, n.clone())).collect();
    let (mut bodies, mut fixtures) = (HashMap::new(), HashMap::new());
    for (name, &b) in &r.bodies {
        let Some(&o) = ours.get(&r.world.body(b).user.map(|u| u.id)) else { continue };
        bodies.insert(name.clone(), o);
        for (f, of) in r.world.body(b).fixtures.iter().zip(&g.world.body(o).fixtures) {
            fixtures.insert(fixture_name[f].clone(), *of);
        }
    }
    // Bodies made during the step (a pasted item) are named by the snapshot after it.
    if g.world.body_list.iter().any(|&b| g.world.body(b).user.is_some() && !bodies.values().any(|&o| o == b)) {
        let after = physics::restore(to, V::new(-0.0, -9.81))?;
        let fixture_name: HashMap<_, _> = after.fixtures.iter().map(|(n, &f)| (f, n.clone())).collect();
        for (name, &b) in &after.bodies {
            if bodies.contains_key(name) {
                continue;
            }
            let Some(&o) = ours.get(&after.world.body(b).user.map(|u| u.id)) else { continue };
            bodies.insert(name.clone(), o);
            for (f, of) in after.world.body(b).fixtures.iter().zip(&g.world.body(o).fixtures) {
                fixtures.insert(fixture_name[f].clone(), *of);
            }
        }
    }
    (r.bodies, r.fixtures) = (bodies, fixtures);
    r.world = std::mem::replace(&mut g.world, World::new(V::ZERO));
    let mut out = physics::compare_world(&r, to)?;
    out.extend(physics::compare_contacts2d(&r, to)?);
    g.world = std::mem::replace(&mut r.world, World::new(V::ZERO));
    out.extend(compare_transforms(g, from, to)?);
    Ok(out)
}

/// --loop-test: load the first snapshot with the b2 world once, then run the game loop through the rest of the
/// recording (FixedUpdate scripts, physics pass, Update) taking only Time from it, comparing at every point.
pub fn loop_test(dump: &Path, rig: &HumanRig) -> Result<(), String> {
    let mut snaps = snapshots(dump)?.skip_while(|s| !(s.pt == "FS" && s.of("b2World").next().is_some()));
    let first = snaps.next().ok_or("no FS snapshot")?;
    let frame_times = frame_times(dump)?;
    if first.of("Contacts2D").next().is_none() {
        return Err("--loop-test needs a recording with Contacts2D (dump12 on)".into());
    }
    let header = header(dump)?;
    let mut g = Game::from_snapshot_header(&first, rig, &header)?;
    // The frame before the snapshot rendered: spriteRenderer.isVisible as that render left it.
    g.physicals_will_render();
    g.recorded_hashes = recorded_hashes(dump)?;
    g.load_layers(&header);
    g.recorded_map_winds(&header);
    native_addresses(&mut g.world, &header)?;
    g.recorded_map_scripts(&header, &first);
    let item = header.iter().find_map(|l| l.strip_prefix("stab item=")).map(|r| r.split(' ').next().unwrap().replace(' ', "_"));
    g.load_input(&first);
    g.MousePosition = g.input.mouse;
    let mut prev = first.clone();
    let mut count = 1;
    // The recorder's FE is a WaitForFixedUpdate coroutine too: the snapshot's waits come before it.
    g.recorder_wait_seq = Some(g.coroutine_seq);
    // The recorder's UE is its own WaitForEndOfFrame coroutine, registered before every other: the game's end-of-frame
    // coroutines (a bullet's next iteration) run after the UE snapshot.
    let mut end_of_frame_due = false;
    // The snapshots read ahead to the frame's US (its time, for EarlyUpdate).
    let mut ahead = std::collections::VecDeque::new();
    let mut n = 0;
    while let Some(cur) = ahead.pop_front().or_else(|| snaps.next()) {
        // An fn-check recording (FnProbe.Run): its "PM" snapshot is taken in the probe's coroutine after the harness set
        // the human up (PPG_LAYER_ADD's layers and the like), and the per-case snapshots after it leave the map out. The
        // loop frames end there; the cases are --fn-check's.
        if cur.pt == "PM" {
            println!("loop points {count}, all match (to the fn-check PM snapshot at frame {})", cur.snap("frame"));
            return Ok(());
        }
        n += 1;
        count += 1;
        // Only Time comes from the recording: the scripts' Random state carries over.
        let random = g.random.clone();
        g.load_time(&prev);
        g.random = random;
        if prev.pt.starts_with('F') {
            if let Some(&t) = frame_times.get(&g.time.frameCount) {
                g.time.dynamicTimeAsDouble = t;
            }
        }
        if end_of_frame_due {
            g.audio_update();
            g.end_of_frame_coroutines();
            // EarlyUpdate's RendererScene::NotifyInvisible at the new frame's time (m_CurFrameTime: the US snapshot's).
            while cur.pt != "US" && !ahead.iter().any(|s: &Snapshot| s.pt == "US") {
                let Some(s) = snaps.next() else { break };
                ahead.push_back(s);
            }
            if let Some(us) = std::iter::once(&cur).chain(ahead.iter()).find(|s| s.pt == "US") {
                let random = g.random.clone();
                g.load_time(us);
                let now = g.time.timeAsDouble;
                g.load_time(&prev);
                g.random = random;
                crate::particles::notify_invisible(&mut g, now);
            }
            // Start of what the coroutines created, before the next FixedUpdate: in the next frame (a WaitForSeconds
            // started there resumes from the frame after it).
            let frame = g.time.frameCount;
            g.time.frameCount = cur.snap("frame").parse().unwrap_or(frame);
            g.run_pending_starts();
            g.time.frameCount = frame;
            end_of_frame_due = false;
        }
        // PPG_RANDOM_TRACE: each point between the draws.
        if std::env::var_os("PPG_RANDOM_TRACE").is_some() {
            eprintln!("point {}->{} frame {}", prev.pt, cur.pt, cur.snap("frame"));
        }
        g.load_input(&cur);
        g.load_render_state(&prev);
        let mut diffs = Vec::new();
        match (prev.pt.as_str(), cur.pt.as_str()) {
            ("FS", "FM") => g.fixed_update_scripts(),
            ("FM", "FE") => {
                // The recorder's scripted PPG_ACT, then its crush and slice (Recorder's FixedUpdate postfix order). A managed
                // exception in the act ends the postfix: that step's crush and slice don't run (the recorder logged them).
                let mut postfix = Ok(());
                super::menu::catch_managed(|| postfix = (|| -> Result<(), String> {
                if let Some((_, act)) = prev.snap.iter().find(|(k, _)| k == "act") {
                    let first = g.physicals.len();
                    g.recorded_act(act, item.as_deref())?;
                    // A PhysicalBehaviour the act made (a paste) seeds Random in Start with its Mono object hash, a heap
                    // address: the recording carries the game's.
                    for p in first..g.physicals.len() {
                        if let Some(h) = cur.of("PhysicalBehaviour").find(|r| r.id == g.physicals[p].id).and_then(|r| r.get("hash")) {
                            g.physicals[p].hash_code = h.parse().map_err(|_| "PhysicalBehaviour hash")?;
                        }
                    }
                }
                // The recorder's scripted LimbBehaviour.Crush (PPG_CRUSH) before the physics step.
                if let Some((_, limb)) = prev.snap.iter().find(|(k, _)| k == "crush") {
                    for limb in limb.split(',') {
                        let l = g.limbs.iter().position(|l| &g.physicals[l.PhysicalBehaviour].name == limb).ok_or(format!("no limb {limb}"))?;
                        g.limb_crush(l);
                    }
                }
                // The recorder's scripted LimbBehaviour.Slice (PPG_SLICE).
                if let Some((_, limbs)) = prev.snap.iter().find(|(k, _)| k == "slice") {
                    for limb in limbs.split(',') {
                        let l = g.limbs.iter().position(|l| &g.physicals[l.PhysicalBehaviour].name == limb).ok_or(format!("no limb {limb}"))?;
                        g.limb_slice(l);
                    }
                }
                Ok(())
                })());
                postfix?;
                // The recorder's scripted pull on the PPG_STAB item (PPG_STAB_PULL): its velocity set before the step.
                if let Some((_, v)) = prev.snap.iter().find(|(k, _)| k == "pull") {
                    let p = g.physicals.iter().position(|p| Some(&p.name) == item.as_ref()).ok_or("pull without a stab item")?;
                    let [vx, vy]: [f32; 2] = floats(v);
                    let rb = g.physicals[p].rigidbody.clone();
                    rb.set_velocity(&mut g.world, [vx, vy]);
                }
                g.simulate_to_fixed_waits();
                diffs = compare_physics(&mut g, &prev, &cur)?;
            }
            ("US", "UE") => {
                g.update_scripts();
                g.render_callbacks(&cur)?;
                // The delayed Destroys run before the recorder's end-of-frame UE snapshot; the WaitForEndOfFrame coroutines after it.
                g.delayed_dynamic_calls();
                end_of_frame_due = true;
            }
            // PreUpdate's Physics2DUpdate at the frame's time.
            ("FE", "US") => {
                let random = g.random.clone();
                g.load_time(&cur);
                g.random = random;
                g.interpolate_bodies();
                diffs = compare_transforms(&g, &first, &cur)?;
            }
            _ => {}
        }
        // The recorder's Rigidbody2D.position reads: Rigidbody2D::GetPosition starts with AutoSyncTransforms.
        g.sync_pending();
        diffs.extend(compare(&mut g, &cur));
        diffs.extend(compare_tool(&g, &cur)?);
        if !diffs.is_empty() {
            eprintln!("diverged at {} frame {} (after {n} matching points, {}→{}):", cur.pt, cur.snap("frame"), prev.pt, cur.pt);
            for d in diffs.iter().take(std::env::var("PPG_DIFFS").ok().and_then(|v| v.as_str().parse().ok()).unwrap_or(25)) {
                eprintln!("  {d}");
            }
            return Err(format!("{} diffs", diffs.len()));
        }
        if cur.pt == "FE" {
            g.resume_after_recorder();
            g.fixed_delayed_calls();
        }
        prev = cur;
    }
    println!("loop points {count}, all match");
    Ok(())
}

/// --script-test: for every FS snapshot, load the game's state, run the FixedUpdate scripts and compare at FM;
/// for every FM snapshot with the b2 world, run the physics pass with its callbacks and compare at FE.
pub fn script_test(dump: &Path, rig: &HumanRig) -> Result<(), String> {
    let text = read_recording(dump)?;
    let snaps = parse(&text);
    let (mut segments, mut failed) = (0, 0);
    let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
    // The physics pass needs the contacts' Collision2D records and the scene's simulation id even on contactless
    // frames (recorded from dump12 on).
    let records = snaps.iter().any(|s| s.of("Contacts2D").next().is_some());
    for pair in snaps.windows(2) {
        let update = pair[0].pt == "US" && pair[1].pt == "UE";
        let physics = records && pair[0].pt == "FM" && pair[1].pt == "FE" && pair[0].of("b2World").next().is_some();
        if !update && !physics && (pair[0].pt != "FS" || pair[1].pt != "FM") {
            continue;
        }
        segments += 1;
        let mut g = Game::from_snapshot(&pair[0], rig)?;
        // Transform.eulerAngles as the scripts read it.
        for r in pair[0].of("Rigidbody2D") {
            let q: [f32; 4] = floats(r.req("trot")?);
            let theirs = f(r.req("euler")?);
            if euler_z(q).to_bits() != theirs.to_bits() {
                return Err(format!("euler {}: game {:08x} ours {:08x}", r.name, theirs.to_bits(), euler_z(q).to_bits()));
            }
        }
        let mut diffs = Vec::new();
        if update {
            g.update_scripts();
            g.render_callbacks(&pair[1])?;
            g.end_of_frame_calls();
        } else if physics {
            g.simulate_to_fixed_waits();
            diffs = compare_physics(&mut g, &pair[0], &pair[1])?;
        } else {
            g.fixed_update_scripts();
        }
        diffs.extend(compare(&mut g, &pair[1]));
        if let Ok(path) = std::env::var("PPG_DIFFS") {
            use std::io::Write;
            let mut out = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
            for d in &diffs {
                writeln!(out, "{segments} {d}").unwrap();
            }
        }
        for d in &diffs {
            // "Type Name.field: ..." -> "Type.field"
            let head = d.split(':').next().unwrap();
            let ty = head.split(' ').next().unwrap();
            let field = head.rsplit('.').next().unwrap();
            *kinds.entry(format!("{} {ty}.{field}", pair[0].pt)).or_default() += 1;
        }
        if !diffs.is_empty() {
            failed += 1;
            if failed <= 1 {
                eprintln!("segment {} {} ({} diffs):", pair[0].pt, pair[0].snap("frame"), diffs.len());
                for d in diffs.iter().take(std::env::var("PPG_DIFFS").ok().and_then(|v| v.as_str().parse().ok()).unwrap_or(25)) {
                    eprintln!("  {d}");
                }
            }
        }
    }
    for (k, n) in &kinds {
        println!("  {n:5} {k}");
    }
    println!("script segments {segments}, failed {failed}");
    if failed == 0 && segments > 0 { Ok(()) } else { Err(format!("{failed} of {segments} segments differ")) }
}

/// The hands' FixedJoint2Ds (GripBehaviour.CreateJoint's AddComponent, one per call): each a `fixed_joints` entry on its
/// restored b2WeldJoint; the one a grip's `joint` names is that grip's, the rest stay as loose_fixed_joints.
fn load_grip_joints(g: &mut Game, s: &Snapshot, rs: &physics::Restored) -> Result<(), String> {
    // The connected Transform (name#transform id): the PhysicalBehaviour on the Clone record holding that Transform.
    let physical_of = |g: &Game, t: &str| -> Option<usize> {
        let (name, t) = t.trim_start_matches('@').split_once('#')?;
        let key = format!("Transform:{t}");
        let pid = s.of("Clone").filter_map(|c| c.get("comps")).find(|comps| comps.trim_matches(['[', ']']).split(';').any(|e| e == key))
            .and_then(|comps| comps.trim_matches(['[', ']']).split(';').find_map(|e| e.strip_prefix("PhysicalBehaviour:")?.parse::<i32>().ok()));
        match pid {
            Some(id) => g.physicals.iter().position(|p| p.id == id),
            None => g.physicals.iter().position(|p| p.name == name),
        }
    };
    for (gi, r) in s.of("GripBehaviour").enumerate() {
        if let Some(v) = r.get("originalRotation") {
            g.grips[gi].originalRotation = f(v);
        }
    }
    // Oldest first (instance ids count down), the order the comparison zips them in.
    let mut records: Vec<&Record> = s.of("FixedJoint2D").collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.id));
    // A wire's joint on a hand limb (a FixedCableBehaviour's typedJoint on LowerArmFront) is the wire's, loaded with it: not
    // a loose grip joint (batch2 r5, fnb2g FixedWireTool.OnDeselect: it was counted twice).
    let wired: HashSet<i32> = s.records.iter().filter_map(|r| r.get("typedJoint")).filter_map(reference).collect();
    for j in records {
        if wired.contains(&j.id) {
            continue;
        }
        let Some(gi) = g.grips.iter().position(|x| g.physicals[x.PhysicalBehaviour].name == j.name) else { continue };
        let mut state = crate::b2::weld::FixedJoint2d::reset();
        let (a, c): ([f32; 2], [f32; 2]) = (floats(j.req("anchor")?), floats(j.req("connectedAnchor")?));
        state.anchor = V::new(a[0], a[1]);
        state.connected_anchor = V::new(c[0], c[1]);
        // AddComponent<FixedJoint2D> leaves autoConfigureConnectedAnchor on (GripBehaviour never turns it off): each
        // recreate after a live joint (a held body's SyncTransforms move) derives the connected anchor again. Newer
        // recordings write the flag; older ones (no field) keep the AddComponent default.
        state.auto_configure_connected_anchor = j.get("autoConfigureConnectedAnchor").map_or(true, |v| v == "1");
        state.anchor_configured = true;
        state.enable_collision = j.req("collide")? == "1";
        state.break_force = f(j.req("breakForce")?);
        state.break_torque = f(j.req("breakTorque")?);
        let joint = rs.joints.get(&format!("{}#{}", j.name, j.id)).copied();
        let connected = physical_of(g, j.req("connected")?).map(BodyOwner::Physical);
        let fj = g.fixed_joints.len();
        let attached = BodyOwner::Physical(g.grips[gi].PhysicalBehaviour);
        g.fixed_joints.push(super::simulate::FixedJoint { hinge: None, state, slider: None, spring: None, friction: None, distance: None, relative: None, joint, attached, connected, active: true, enabled: j.req("enabled")? == "1", destroyed: false });
        let owner = s.of("GripBehaviour").position(|x| x.get("joint").and_then(reference) == Some(j.id));
        match owner {
            Some(k) => g.grips[k].joint = Some(fj),
            None => g.loose_fixed_joints.push(fj),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_nested() {
        assert_eq!(split_top("a=1 b=X(c=1 d=2) e=[f;g h]", ' '), ["a=1", "b=X(c=1 d=2)", "e=[f;g h]"]);
        assert_eq!(key_value("<IsOnFloor>k__BackingField=0"), ("IsOnFloor".into(), "0".into()));
        assert_eq!(super::super::circulation::liquid("BLOOD"), 0);
    }
}
