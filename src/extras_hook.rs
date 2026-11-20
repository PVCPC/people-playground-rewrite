//! The extras hook: new content that is not in the real game (extras/src/lib.rs) joins the existing catalog. Nothing
//! here runs for the game's own items, so the 1:1 game is untouched.
//!
//! How it works: the first catalog lookup (`Game::get_spawnable`) calls `register`, which
//! 1. builds the extras' art into a SerializedFile made in memory ("extras": Texture2Ds, Sprites, a Material), so every
//!    reader of sprites, textures and materials (renderer, physics bounds, wounds, catalog icons) sees them as game data;
//! 2. builds each item's Prefab from its template item's prefab (the crab from the Gorse: same scripts and settings,
//!    its own limbs, joints, sprites, poses), caches it under a new root and adds its name to the catalog lookup, so
//!    spawning, saving and loading by name work as for any catalog item.
//! The spawn menu adds `catalog_items` to the categories they name (src/ui/data.rs), and each fixed step a crab's
//! person gets `crab_fixed_update` (its own behaviour on top of the human muscle and pose system).
#![allow(non_snake_case)]

use crate::assets::classes::PPtr;
use crate::assets::components::{ColliderShape, JointKind, Material};
use crate::assets::db::{AssetDb, Component, ObjRef, Prefab, PrefabObject};
use crate::assets::mono::Value;
use crate::assets::serialized::{External, SerializedFile};
use ppg_extras as x;
use std::collections::HashMap;

pub mod portal_gun;
pub mod syringes;

const FILE: &str = "extras";
/// Path ids of the built prefabs' objects, in their template's file (no game object uses these).
const BASE: i64 = 0x7E00_0000_0000;
const SKIN: i64 = 1;
const FLESH: i64 = 2;
const BONE: i64 = 3;
const ICON_TEX: i64 = 4;
const ICON: i64 = 5;
const MATERIAL: i64 = 6;
const SPRITES: i64 = 100;

/// Unity's serialization, little endian, 4-byte aligned.
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn floats(&mut self, v: &[f32]) -> &mut Self {
        v.iter().for_each(|&f| {
            self.f32(f);
        });
        self
    }
    fn align(&mut self) -> &mut Self {
        self.0.resize((self.0.len() + 3) & !3, 0);
        self
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.i32(b.len() as i32);
        self.0.extend(b);
        self.align()
    }
    fn string(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }
    fn pptr(&mut self, p: PPtr) -> &mut Self {
        self.i32(p.file);
        self.0.extend(p.path_id.to_le_bytes());
        self
    }
}

/// A PNG's size and RGBA rows, bottom row first (Unity's order).
fn png(bytes: &[u8]) -> ([u32; 2], Vec<u8>) {
    let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("extras png");
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).expect("extras png");
    assert_eq!(info.color_type, png::ColorType::Rgba, "extras art is RGBA");
    let row = info.width as usize * 4;
    ([info.width, info.height], buf[..row * info.height as usize].chunks(row).rev().flatten().copied().collect())
}

/// Texture2D (Texture2D::read's layout): RGBA32, point filtered, clamped, sRGB.
fn texture(name: &str, size: [u32; 2], rgba: &[u8]) -> Vec<u8> {
    let mut w = W::default();
    w.string(name).i32(0).0.extend([0, 0]);
    w.align().i32(size[0] as i32).i32(size[1] as i32).u32(rgba.len() as u32).i32(0).i32(4).i32(1);
    w.0.extend([1, 0, 0, 0]);
    w.i32(0).i32(1).i32(2).i32(0).i32(1).f32(0.0).i32(1).i32(1).i32(1).i32(0).i32(1).bytes(&[]).bytes(rgba);
    w.0.extend(0u64.to_le_bytes());
    w.u32(0).string("");
    w.0
}

/// Sprite (Sprite::read's layout): a full-rect quad of `rect` in texture `tex` about its centre.
fn sprite(name: &str, rect: [u32; 4], sheet: [u32; 2], tex: PPtr) -> Vec<u8> {
    let [x, y, wd, ht] = rect.map(|v| v as f32);
    let (hw, hh) = (wd / 2.0 / x::PPU, ht / 2.0 / x::PPU);
    let mut w = W::default();
    w.string(name).floats(&[x, y, wd, ht]).floats(&[0.0, 0.0]).floats(&[0.0; 4]).f32(x::PPU).floats(&[0.5, 0.5]).u32(1);
    w.0.push(0);
    w.align();
    w.0.extend([0; 16]);
    w.0.extend(0i64.to_le_bytes());
    w.i32(0).pptr(PPtr::default()).pptr(tex).pptr(PPtr::default()).i32(0);
    w.i32(1).u32(0).u32(6).i32(0).u32(0).u32(0).u32(4).floats(&[0.0, 0.0, 0.0, hw, hh, 0.0]);
    let index: Vec<u8> = [0u16, 1, 2, 2, 1, 3].iter().flat_map(|i| i.to_le_bytes()).collect();
    w.bytes(&index).u32(4);
    w.i32(14);
    for c in 0..14 {
        w.0.extend(match c {
            0 => [0, 0, 0, 3],
            4 => [1, 0, 0, 2],
            _ => [0; 4],
        });
    }
    let [sw, sh] = sheet.map(|v| v as f32);
    let (u0, v0, u1, v1) = (x / sw, y / sh, (x + wd) / sw, (y + ht) / sh);
    let mut verts = W::default();
    verts.floats(&[-hw, hh, 0.0, hw, hh, 0.0, -hw, -hh, 0.0, hw, -hh, 0.0]).floats(&[u0, v1, u1, v1, u0, v0, u1, v0]);
    w.bytes(&verts.0).i32(0);
    w.floats(&[x, y, wd, ht]).floats(&[0.0, 0.0]).floats(&[-1.0, -1.0]).u32(64).floats(&[x::PPU, x + wd / 2.0, x::PPU, y + ht / 2.0]).f32(1.0).i32(0).i32(0);
    w.0
}

/// Material (Material::read's layout).
fn material(name: &str, shader: PPtr, keywords: &str, m: &Material, textures: &[(String, PPtr, [f32; 4])]) -> Vec<u8> {
    let mut w = W::default();
    w.string(name).pptr(shader).string(keywords).u32(4).0.extend([0, 0]);
    w.align().i32(m.render_queue).i32(0).i32(0).i32(textures.len() as i32);
    for (n, t, st) in textures {
        w.string(n).pptr(*t).floats(st);
    }
    w.i32(m.floats.len() as i32);
    for (n, f) in &m.floats {
        w.string(n).f32(*f);
    }
    w.i32(m.colors.len() as i32);
    for (n, c) in &m.colors {
        // The crab's blood is blue, and so are its bruises (the template's are green).
        let c = match n.as_str() {
            "_BloodColor" => &x::CRAB_BLOOD_COLOUR,
            "_BruiseColor" => &x::CRAB_BRUISES[0],
            "_SecondBruiseColor" => &x::CRAB_BRUISES[1],
            "_ThirdBruiseColor" => &x::CRAB_BRUISES[2],
            _ => c,
        };
        w.string(n).floats(c);
    }
    w.0
}

/// `to - from` in degrees, in -180..180.
fn mathf_delta(from: f32, to: f32) -> f32 {
    (to - from + 540.0).rem_euclid(360.0) - 180.0
}

fn set(v: &mut Value, name: &str, new: Value) {
    let Value::Struct(fields) = v else { panic!("extras: not a struct") };
    let f = fields.iter_mut().find(|(n, _)| n == name).unwrap_or_else(|| panic!("extras: no field {name}"));
    f.1 = new;
}

fn refs(r: impl IntoIterator<Item = ObjRef>) -> Value {
    Value::Array(r.into_iter().map(|r| Value::PPtr(PPtr { file: 0, path_id: r.path_id })).collect())
}

/// CatalogBehaviour.Populate's lookup gets the extras (once): their art in the "extras" file, their prefabs cached.
pub fn register(db: &mut AssetDb, by_name: &mut HashMap<String, ObjRef>) -> Result<(), String> {
    if db.file_index(FILE).is_some() {
        return Ok(());
    }
    let template = *by_name.get("Gorse").ok_or("extras: no Gorse to build the crab from")?;
    let gorse = db.prefab_cached(template)?;
    let gfile = template.file;
    let body = gorse.objects.iter().position(|o| o.name == "body").ok_or("extras: Gorse body")?;
    let leg = gorse.objects.iter().position(|o| o.name == "right upper leg").ok_or("extras: Gorse leg")?;
    // The Gorse's skin material: its shader, floats and damage map.
    let mat = gorse.objects[body].components.iter().find_map(|(_, c)| match c {
        Component::SpriteRenderer(r) => r.materials.first().copied(),
        _ => None,
    });
    let mat = db.resolve(gfile, mat.ok_or("extras: Gorse material")?).ok_or("extras: Gorse material")?;
    let (m, keywords) = Material::read_with_keywords(db.data(mat))?;
    let shader = db.resolve(mat.file, m.shader).ok_or("extras: shader")?;
    let tex = |n: &str| m.textures.iter().find(|t| t.0 == n).and_then(|t| db.resolve(mat.file, t.1));
    let damage = tex("_DamageTex").ok_or("extras: damage map")?;

    // The "extras" file: externals name every game file, so its PPtrs reach the shader and the damage map.
    let externals = db.files.iter().map(|f| External { guid: [0; 16], kind: 0, path: f.path.file_name().unwrap().to_string_lossy().to_string() }).collect();
    let mut objects = Vec::new();
    for (id, name, bytes) in [(SKIN, "crab-skin", x::CRAB_SKIN), (FLESH, "crab-flesh", x::CRAB_FLESH), (BONE, "crab-bone", x::CRAB_BONE), (ICON_TEX, "crab-icon", x::CRAB_ICON)] {
        let (size, rgba) = png(bytes);
        objects.push((id, 28, texture(name, size, &rgba)));
    }
    for (i, (name, rect)) in x::sheet::RECTS.iter().enumerate() {
        objects.push((SPRITES + i as i64, 213, sprite(&format!("crab {name}"), *rect, x::sheet::SHEET, PPtr { file: 0, path_id: SKIN })));
    }
    let [iw, ih] = x::sheet::ICON;
    objects.push((ICON, 213, sprite("Crab", [0, 0, iw, ih], [iw, ih], PPtr { file: 0, path_id: ICON_TEX })));
    // externals[i] is files[i].
    let ext = |_: &AssetDb, r: ObjRef| PPtr { file: r.file as i32 + 1, path_id: r.path_id };
    let textures = vec![
        ("_BoneTex".to_string(), PPtr { file: 0, path_id: BONE }, [1.0, 1.0, 0.0, 0.0]),
        ("_DamageTex".to_string(), ext(db, damage), [1.0, 1.0, 0.0, 0.0]),
        ("_FleshTex".to_string(), PPtr { file: 0, path_id: FLESH }, [1.0, 1.0, 0.0, 0.0]),
        ("_MainTex".to_string(), PPtr { file: 0, path_id: SKIN }, [1.0, 1.0, 0.0, 0.0]),
    ];
    objects.push((MATERIAL, 21, material("Crab", ext(db, shader), &keywords.replace(" _ISZOMBIE_ON", ""), &m, &textures)));
    portal_gun::art(&mut objects);
    db.add_file(FILE, SerializedFile::in_memory(FILE, externals, objects));
    // The template's file reaches the extras file through one more external.
    db.files[gfile].externals.push(External { guid: [0; 16], kind: 0, path: FILE.to_string() });
    let to_extras = |id: i64| PPtr { file: db.files[gfile].externals.len() as i32, path_id: id };

    let crab = build_crab(&gorse, body, leg, gfile, to_extras(MATERIAL), |s| to_extras(SPRITES + x::sheet::RECTS.iter().position(|(n, _)| *n == s).unwrap() as i64))?;
    let root = crab.objects[0].game_object;
    db.cache_prefab(root, crab);
    by_name.insert("Crab".to_string(), root);
    portal_gun::register(db, by_name)?;
    syringes::register(db, by_name)
}

/// The spawn menu's extras: (name, description, category name, icon sprite, order key).
pub fn catalog_items(db: &mut AssetDb) -> Vec<(String, String, String, Option<ObjRef>, String)> {
    let _ = crate::game::Game::get_spawnable(db, "");
    let file = db.file_index(FILE);
    let icon = |name| file.map(|file| ObjRef { file, path_id: if name == x::PORTAL_GUN { portal_gun::ICON } else { ICON } });
    let mut v: Vec<_> = x::ITEMS.iter().map(|i| (i.name.to_string(), i.description.to_string(), i.category.to_string(), icon(i.name), i.name.to_string())).collect();
    // The syringes show the Empty Syringe's picture, right after the game's syringes (keyed "aSyringe2 <n>").
    let view = spawnables(db).into_iter().find(|h| h.name == syringes::TEMPLATE).and_then(|h| h.view);
    for (k, s) in syringes::list().enumerate() {
        v.push((s.name.to_string(), s.description.to_string(), x::SYRINGE_CATEGORY.to_string(), view, format!("{} {k:02}", x::SYRINGE_ORDER)));
    }
    v
}

/// A SpawnableAsset as the catalog reads it.
pub struct Spawnable {
    pub name: String,
    pub description: String,
    pub view: Option<ObjRef>,
    /// CatalogBehaviour's sort key: NameToOrderBy when set, else the name.
    pub order: String,
    /// VisibleInCatalog false or Locked: the game's catalog never shows it (bar an unlocked Locked one).
    pub hidden: bool,
}

/// Every SpawnableAsset, in Resources order.
fn spawnables(db: &mut AssetDb) -> Vec<Spawnable> {
    let mut out = Vec::new();
    for r in db.resources_in("SpawnableAssets", 114).unwrap_or_default() {
        let Ok(Some((_, mb, v))) = db.mono(r) else { continue };
        let visible = v.get("VisibleInCatalog").and_then(Value::bool).unwrap_or(true);
        let locked = v.get("Locked").and_then(Value::bool).unwrap_or(false);
        let description = match v.get("Description") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let view = v.get("ViewSprite").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p));
        let order = match v.get("NameToOrderBy") {
            Some(Value::Str(s)) if !s.is_empty() => s.clone(),
            _ => mb.name.clone(),
        };
        out.push(Spawnable { name: mb.name, description, view, order, hidden: !visible || locked });
    }
    out
}

/// The "Hidden items" tab: every SpawnableAsset the game's catalog hides (VisibleInCatalog false or Locked), always
/// available here, in Resources order.
pub fn hidden_items(db: &mut AssetDb) -> Vec<Spawnable> {
    spawnables(db).into_iter().filter(|s| s.hidden).collect()
}

/// The crab: the Gorse's root (PersonBehaviour and the rest), the Gorse body's components on the shell, a Gorse leg's
/// on every other limb; joints, colliders, sprites, the limb graph and the poses from `ppg_extras::crab_parts`.
fn build_crab(gorse: &Prefab, body: usize, leg: usize, gfile: usize, material: PPtr, sprite: impl Fn(&str) -> PPtr) -> Result<Prefab, String> {
    let parts = &x::crab_parts();
    let mut next = BASE;
    let mut id = || {
        next += 1;
        ObjRef { file: gfile, path_id: next }
    };
    let local = |r: ObjRef| PPtr { file: 0, path_id: r.path_id };
    // New ids: per object its GameObject and each component, in the template's component order.
    let template = |k: Option<usize>| match k {
        None => 0,
        Some(0) => body,
        Some(_) => leg,
    };
    let mut ids: Vec<(ObjRef, Vec<ObjRef>)> = Vec::new();
    for k in std::iter::once(None).chain((0..parts.len()).map(Some)) {
        let t = &gorse.objects[template(k)];
        ids.push((id(), t.components.iter().map(|_| id()).collect()));
    }
    // A component of part `k` (object k + 1) by its kind.
    let comp = |k: usize, pick: &dyn Fn(&Component) -> bool| -> ObjRef {
        let t = &gorse.objects[template(Some(k))];
        let c = t.components.iter().position(|(_, c)| pick(c)).expect("extras: template component");
        ids[k + 1].1[c]
    };
    let script = |k: usize, class: &str| comp(k, &|c| matches!(c, Component::Script { class: n, .. } if n == class));
    let rb = |k: usize| comp(k, &|c| matches!(c, Component::Rigidbody2D(_)));
    let transform = |k: usize| comp(k, &|c| matches!(c, Component::Transform(_)));
    let children = |k: usize| (0..parts.len()).filter(move |&c| parts[c].parent == Some(k));
    let near = |k: usize| parts[k].parent.into_iter().chain(children(k));

    let mut p = Prefab::default();
    // The root.
    let root_t = &gorse.objects[0];
    let mut root = PrefabObject { game_object: ids[0].0, name: "Crab".into(), children: (1..=parts.len()).collect(), components: Vec::new(), ..root_t.clone() };
    for (c, (_, comp)) in root_t.components.iter().enumerate() {
        let mut comp = comp.clone();
        match &mut comp {
            Component::Transform(t) => {
                t.children = (0..parts.len()).map(|k| local(transform(k))).collect();
                t.local_position = [0.0; 3];
            }
            Component::Script { class, fields, .. } if class == "PersonBehaviour" => {
                set(fields, "Limbs", refs((0..parts.len()).map(|k| script(k, "LimbBehaviour"))));
                // The Gorse's shot chunks are green goo; the crab's wounds show its blue blood instead.
                set(fields, "PoolableImpactEffect", Value::PPtr(PPtr { file: 0, path_id: 0 }));
                let poses = fields.get("Poses").and_then(Value::array).ok_or("extras: Poses")?.to_vec();
                let walking = poses.iter().find(|p| matches!(p.get("Name"), Some(Value::Str(n)) if n == "Walking")).ok_or("extras: Walking pose")?;
                let angle_t = walking.get("Angles").and_then(Value::array).and_then(|a| a.first()).ok_or("extras: a pose angle")?.clone();
                let mut out = Vec::new();
                for mut pose in poses {
                    let Some(Value::Str(name)) = pose.get("Name").cloned() else { return Err("extras: pose name".into()) };
                    let name = name.replace(' ', "");
                    let name = if name == "Braindamage" { "Brain damage".to_string() } else { name };
                    let angles = (1..parts.len()).map(|k| {
                        let a = x::pose_angle(&name, &parts[k]);
                        // MoveIntoPose aims jointAngle (connected - attached - referenceAngle) at referenceAngle + angle;
                        // the reference is the rest stance's (parent - limb), so the rest stance is angle = limb - parent,
                        // and turning the limb counter-clockwise by `a` lowers it.
                        let rest = mathf_delta(parts[parts[k].parent.unwrap()].angle, parts[k].angle);
                        let mut v = angle_t.clone();
                        set(&mut v, "Name", Value::Str(parts[k].name.clone()));
                        set(&mut v, "Limb", Value::PPtr(local(script(k, "LimbBehaviour"))));
                        set(&mut v, "Animated", Value::Bool(a.animated));
                        // A swing needs a stiffer aim than a held angle to keep up with its own animation.
                        set(&mut v, "PoseRigidityModifier", Value::Float(if a.animated { 3.0 } else { 0.0 }));
                        set(&mut v, "Angle", Value::Float(rest - a.angle));
                        set(&mut v, "StartAngle", Value::Float(rest - a.start));
                        set(&mut v, "EndAngle", Value::Float(rest - a.end));
                        set(&mut v, "AnimationDuration", Value::Float(a.duration));
                        set(&mut v, "RandomInfluence", Value::Float(0.0));
                        set(&mut v, "RandomSpeed", Value::Float(1.0));
                        set(&mut v, "TimeOffset", Value::Float(a.offset));
                        v
                    });
                    set(&mut pose, "Angles", Value::Array(angles.collect()));
                    out.push(pose);
                }
                set(fields, "Poses", Value::Array(out));
            }
            _ => {}
        }
        root.components.push((ids[0].1[c], comp));
    }
    p.objects.push(root);
    for (k, part) in parts.iter().enumerate() {
        let t = &gorse.objects[template(Some(k))];
        let (go, cids) = &ids[k + 1];
        let mut o = PrefabObject { game_object: *go, name: part.name.clone(), parent: Some(0), children: Vec::new(), components: Vec::new(), ..t.clone() };
        for (c, (_, comp)) in t.components.iter().enumerate() {
            let mut comp = comp.clone();
            match &mut comp {
                Component::Transform(tr) => {
                    let (s, co) = (part.angle.to_radians() / 2.0).sin_cos();
                    tr.local_position = [part.center[0], part.center[1], 0.0];
                    tr.local_rotation = [0.0, 0.0, s, co];
                    tr.local_scale = [1.0; 3];
                    tr.children = Vec::new();
                    tr.father = local(ids[0].1[0]);
                    tr.game_object = local(*go);
                }
                Component::SpriteRenderer(r) => {
                    r.sprite = sprite(part.sprite);
                    r.materials = vec![material];
                    r.sorting_order = part.order;
                    r.color = [part.shade, part.shade, part.shade, 1.0];
                    r.game_object = local(*go);
                }
                Component::Collider2D(col) => {
                    col.offset = [0.0; 2];
                    col.game_object = local(*go);
                    match &mut col.shape {
                        ColliderShape::Polygon { paths, .. } => *paths = vec![x::shell_outline()],
                        ColliderShape::Box { size, .. } => *size = part.size,
                        _ => return Err("extras: an unexpected collider".into()),
                    }
                }
                Component::Rigidbody2D(b) => {
                    b.mass = part.mass;
                    b.game_object = local(*go);
                }
                Component::Joint2D(j) => {
                    let parent = part.parent.unwrap();
                    j.connected_body = local(rb(parent));
                    j.anchor = part.anchor;
                    j.connected_anchor = part.connected_anchor;
                    j.auto_configure_connected_anchor = false;
                    j.game_object = local(*go);
                    if let JointKind::Hinge { lower, upper, .. } = &mut j.kind {
                        (*lower, *upper) = (part.limits[0], part.limits[1]);
                    }
                }
                Component::Script { class, fields, .. } => match class.as_str() {
                    "PhysicalBehaviour" => {
                        set(fields, "LocalColliderGridPoints", Value::Array(Vec::new()));
                        set(fields, "Circumference", Value::Float(-1.0));
                    }
                    "LimbBehaviour" => {
                        set(fields, "SpeciesIdentity", Value::Str(x::SPECIES.into()));
                        set(fields, "BloodLiquidType", Value::Str(x::CRAB_BLOOD.into()));
                        set(fields, "ConnectedLimbs", refs(near(k).map(|n| script(n, "LimbBehaviour"))));
                        // Crab muscles: strong enough to carry the shell on four thin legs.
                        if let Some(Value::Float(b)) = fields.get("BaseStrength").cloned() {
                            set(fields, "BaseStrength", Value::Float(b * x::STRENGTH));
                        }
                    }
                    "CirculationBehaviour" => set(fields, "PushesTo", refs(children(k).map(|n| script(n, "CirculationBehaviour")))),
                    "SkinMaterialHandler" => set(fields, "adjacentLimbs", refs(near(k).map(|n| script(n, "SkinMaterialHandler")))),
                    "ConnectedNodeBehaviour" => set(fields, "Connections", refs(near(k).map(|n| script(n, "ConnectedNodeBehaviour")))),
                    "GoreStringBehaviour" => {
                        set(fields, "Other", Value::PPtr(local(rb(part.parent.unwrap()))));
                        set(fields, "ColourSource", Value::PPtr(local(script(k, "CirculationBehaviour"))));
                    }
                    _ => {}
                },
                _ => {}
            }
            if let Some(&m) = gorse.materials.get(&(template(Some(k)), c)) {
                p.materials.insert((k + 1, c), m);
            }
            o.components.push((cids[c], comp));
        }
        p.objects.push(o);
    }
    for (o, ob) in p.objects.iter().enumerate() {
        p.index.insert(ob.game_object, (o, usize::MAX));
        for (c, (r, _)) in ob.components.iter().enumerate() {
            p.index.insert(*r, (o, c));
        }
    }
    Ok(p)
}

/// Explosions a crab can hear: (game time, position, range). Written by ExplosionCreator.CreateFragmentationExplosion,
/// read only by crabs (no game state).
static EXPLOSIONS: std::sync::Mutex<Vec<(f32, [f32; 2], f32)>> = std::sync::Mutex::new(Vec::new());

pub fn heard_explosion(time: f32, position: [f32; 3], range: f32) {
    let mut e = EXPLOSIONS.lock().unwrap();
    e.retain(|x| (time - x.0).abs() < 2.0);
    e.push((time, [position[0], position[1]], range));
}

/// A crab's mind, by its PersonBehaviour's id (not saved: a loaded crab starts afresh).
#[derive(Default)]
struct Mind {
    seed: u32,
    /// Seconds left of the current wander (or rest), and its direction (0 rests).
    timer: f32,
    dir: f32,
    /// Seconds left of running away from fire or an explosion.
    flee: f32,
    /// Seconds spent on its back.
    flipped: f32,
    /// Seconds left of rolling back over, and its way (1 counter-clockwise).
    rolling: f32,
    roll: f32,
    /// Seconds left of settling after a roll: the legs hold it still so it does not tip over again.
    settle: f32,
    /// Per claw (left, right): seconds left of a pinch.
    pinch: [f32; 2],
}

impl Mind {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }
}

static MINDS: std::sync::Mutex<Option<HashMap<i32, Mind>>> = std::sync::Mutex::new(None);

/// The crab's behaviour, each fixed step after PersonBehaviour.FixedUpdate: gills (no drowning), wandering sideways,
/// running from fire and explosions, pinching what touches its claws, flailing on its back and righting itself.
/// The human pose system does the rest (poses, pain, shock, unconsciousness, death).
pub fn crab_fixed_update(g: &mut crate::game::Game, p: usize) {
    let Some(head) = g.people[p].Head else { return };
    if g.people[p].destroyed || g.limbs[head].SpeciesIdentity != x::SPECIES {
        return;
    }
    let dt = g.time.fixedDeltaTime;
    let shell = g.limbs[head].PhysicalBehaviour;
    let alive = g.person_is_alive(p);
    // Gills: water does not choke a crab.
    if alive && g.physicals[shell].Wetness >= 0.5 && g.circulations[head].BloodFlow > 0.25 {
        g.people[p].OxygenLevel = 1.0;
    }
    let mut minds = MINDS.lock().unwrap();
    let mind = minds.get_or_insert_with(HashMap::new).entry(g.people[p].id).or_insert_with(|| Mind { seed: 0x9E37_79B9 ^ g.people[p].id as u32, ..Default::default() });
    let person = &g.people[p];
    if !alive || person.Consciousness < 0.5 || person.PainLevel > 0.8 || g.limbs[head].IsDismembered {
        return;
    }
    let pos = g.physicals[shell].transform.position;
    let up = g.transform_up(shell);
    let limbs = g.people[p].Limbs.clone();
    let name = |g: &crate::game::Game, l: usize| g.physicals[g.limbs[l].PhysicalBehaviour].name.clone();

    let ph: Vec<usize> = limbs.iter().map(|&l| g.limbs[l].PhysicalBehaviour).collect();
    // Rolling back over: the legs keep turning it until it is nearly upright.
    if mind.rolling > 0.0 {
        mind.rolling -= dt;
        if up[1] > 0.8 {
            // Upright: the legs stop the turn and hold it while it settles.
            mind.rolling = 0.0;
            mind.settle = 0.6;
            heave(g, shell, &ph, 0.0, None);
        } else {
            heave(g, shell, &ph, mind.roll * 4.5, None);
        }
        return;
    }
    if mind.settle > 0.0 {
        mind.settle -= dt;
        heave(g, shell, &ph, 0.0, None);
        return;
    }
    // On its back: flail, and roll back over when not too hurt.
    if up[1] < -0.3 {
        mind.flipped += dt;
        let t = g.time.time;
        for (i, &l) in limbs.iter().enumerate() {
            if name(g, l).contains("leg") {
                g.influence_motor_speed(l, (t * 14.0 + i as f32 * 1.3).sin() * 400.0, 0.6);
            }
        }
        // After a second of flailing, a heave of the legs rolls it back over the nearer way (up = (-sin a, cos a)).
        let touching = g.people[p].IsTouchingFloor || g.is_touching_something(shell);
        if mind.flipped > 1.0 && touching && g.people[p].Consciousness > 0.8 {
            mind.flipped = 0.0;
            mind.roll = if up[0] < 0.0 { 1.0 } else { -1.0 };
            mind.rolling = 1.0;
            heave(g, shell, &ph, mind.roll * 4.5, Some(1.5));
        }
        return;
    }
    mind.flipped = 0.0;
    // Standing: the feet's grip damps the wobble four light legs and the strong pose motors would otherwise build up.
    if g.people[p].IsTouchingFloor {
        for &q in &ph {
            let rb = g.physicals[q].rigidbody.clone();
            let w = rb.angular_velocity(&g.world);
            rb.set_angular_velocity(&mut g.world, w * 0.9);
        }
    }

    // Danger: fire within 3 units, an explosion within twice its range in the last second.
    let own: Vec<usize> = limbs.iter().map(|&l| g.limbs[l].PhysicalBehaviour).collect();
    let mut threat = None;
    for (i, ph) in g.physicals.iter().enumerate() {
        if ph.destroyed || !(ph.OnFire || ph.burnIntensity > 0.1) || own.contains(&i) {
            continue;
        }
        let q = ph.transform.position;
        if (q[0] - pos[0]).hypot(q[1] - pos[1]) < 3.0 {
            threat = Some(q[0]);
        }
    }
    for &(when, at, range) in EXPLOSIONS.lock().unwrap().iter() {
        if g.time.time - when < 1.0 && g.time.time >= when && (at[0] - pos[0]).hypot(at[1] - pos[1]) < range * 2.0 + 2.0 {
            threat = Some(at[0]);
        }
    }
    if let Some(from) = threat {
        mind.flee = 3.0;
        mind.dir = if pos[0] >= from { 1.0 } else { -1.0 };
        mind.timer = 0.0;
    }

    // Wander: scuttle one way for a while, rest, pick again.
    if mind.flee > 0.0 {
        mind.flee -= dt;
    } else {
        mind.timer -= dt;
        if mind.timer <= 0.0 {
            mind.dir = match mind.random() {
                r if r < 0.3 => 0.0,
                r if r < 0.65 => -1.0,
                _ => 1.0,
            };
            mind.timer = 1.5 + mind.random() * 3.0;
        }
    }
    if mind.dir != 0.0 && g.people[p].IsTouchingFloor {
        g.people[p].DesiredWalkingDirection = mind.dir * 2.0;
        // The legs' scuttle, helped along: four short legs push harder than their joint motors show.
        let rb = g.physicals[shell].rigidbody.clone();
        let target = mind.dir * if mind.flee > 0.0 { 1.4 } else { 0.6 };
        if (rb.velocity(&g.world)[0] - target) * mind.dir < 0.0 {
            rb.add_force(&mut g.world, [mind.dir * 0.8, 0.0]);
        }
    }

    // Pinch: something that is not this crab touching a claw or its pincer opens the pincer, then snaps it shut.
    let pincers: Vec<usize> = limbs.iter().copied().filter(|&l| name(g, l).ends_with("pincer")).take(2).collect();
    for (side, l) in pincers.into_iter().enumerate() {
        let claw = g.limbs[l].ConnectedLimbs.first().copied();
        let mut touched = None;
        for ph in [Some(g.limbs[l].PhysicalBehaviour), claw.map(|c| g.limbs[c].PhysicalBehaviour)].into_iter().flatten() {
            for c in g.physicals[ph].collisionBuffer.clone() {
                if let Some(other) = g.physical_of_collider(c.other_collider).filter(|o| !own.contains(o)) {
                    touched = Some(other);
                }
            }
        }
        if mind.pinch[side] <= 0.0 {
            if let Some(other) = touched {
                mind.pinch[side] = 1.0;
                if let Some(victim) = (0..g.limbs.len()).find(|&v| g.limbs[v].PhysicalBehaviour == other && !g.people[g.limbs[v].Person].destroyed) {
                    let who = g.limbs[victim].Person;
                    g.add_pain(who, 0.4);
                }
            }
            continue;
        }
        mind.pinch[side] -= dt;
        let Some(h) = g.limbs[l].Joint else { continue };
        let s = if name(g, l).starts_with("right") { 1.0 } else { -1.0 };
        // Open (counter-clockwise for the right claw) for 0.3 s, then squeeze past shut.
        let target = if mind.pinch[side] > 0.7 { -s * 35.0 } else { s * 6.0 };
        // jointAngle is 0 in the rest stance (shut).
        let j = g.hinges[h].joint_angle(&g.world);
        let value = mathf_delta(j, target) * 12.0;
        g.influence_motor_speed(l, value.clamp(-450.0, 450.0), 1.0);
    }
}

/// A crab limb's BloodDecal: the template's stain shapes in crab-blood blue, as a descriptor of its own.
pub fn blood_decal(g: &mut crate::game::Game, species: &str, decal: &mut String) {
    if species != x::SPECIES || decal.is_empty() {
        return;
    }
    let name = format!("{} ({})", x::CRAB_BLOOD, decal);
    if g.decal_descriptors.iter().all(|d| d.name != name) {
        let i = g.decal_descriptor(decal);
        let mut d = g.decal_descriptors[i].clone();
        d.name = name.clone();
        let c = x::CRAB_BLOOD_COLOUR;
        d.Color = [c[0], c[1], c[2], d.Color[3]];
        g.decal_descriptors.push(d);
        if let Some(s) = g.decal_sprites.get(&i).cloned() {
            g.decal_sprites.insert(g.decal_descriptors.len() - 1, s);
        }
    }
    *decal = name;
}

/// Sets every part's velocity to one rigid motion: the shell's (or lifted at `up` m/s) and turning at `w` rad/s about it.
fn heave(g: &mut crate::game::Game, shell: usize, parts: &[usize], w: f32, up: Option<f32>) {
    let c = g.physicals[shell].transform.position;
    let v = g.physicals[shell].rigidbody.velocity(&g.world);
    let (vx, up) = (if up.is_some() { 0.0 } else { v[0] }, up.unwrap_or(v[1]));
    for &q in parts {
        let rb = g.physicals[q].rigidbody.clone();
        let r = g.physicals[q].transform.position;
        rb.set_velocity(&mut g.world, [vx - (r[1] - c[1]) * w, up + (r[0] - c[0]) * w]);
        rb.set_angular_velocity(&mut g.world, w.to_degrees());
    }
}

/// A Shot on a crab's shell below `x::SHELL_GLANCE` damage glances off: no wound, no pain (big hits go through to the
/// human logic and crack it).
pub fn shell_glances(g: &crate::game::Game, physical: Option<usize>, damage: f32) -> bool {
    let Some(p) = physical else { return false };
    damage < x::SHELL_GLANCE
        && g.limbs.iter().any(|l| l.PhysicalBehaviour == p && l.SpeciesIdentity == x::SPECIES && l.HasBrain)
}
