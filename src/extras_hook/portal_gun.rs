//! The portal gun's prefab (extras, not the game's): the Pistol's root object with its own sprite and a box collider
//! its size. It keeps the Pistol's Rigidbody2D and PhysicalBehaviour (metal impact and shot sounds, outline, holding,
//! saving), and drops the slide, the attachments, FirearmBehaviour and ActOnCollide; src/game/extras_glue.rs fires it.

use super::*;

const TEX: i64 = 200;
const SPRITE: i64 = 201;
const ICON_TEX: i64 = 202;
pub(super) const ICON: i64 = 203;
/// The gun's GameObject id in its template's file (the crab's ids start at BASE + 1); `extras_spawned` knows the gun by it.
pub const ROOT: i64 = BASE + 0x1_0000;

/// The gun's textures and sprites, for the extras file.
pub(super) fn art(objects: &mut Vec<(i64, i32, Vec<u8>)>) {
    for (tex, spr, name, bytes) in [(TEX, SPRITE, "portal-gun", x::art::GUN), (ICON_TEX, ICON, x::PORTAL_GUN, x::art::GUN_ICON)] {
        let (size, rgba) = png(bytes);
        objects.push((tex, 28, texture(name, size, &rgba)));
        objects.push((spr, 213, sprite(name, [0, 0, size[0], size[1]], size, PPtr { file: 0, path_id: tex })));
    }
}

pub(super) fn register(db: &mut AssetDb, by_name: &mut HashMap<String, ObjRef>) -> Result<(), String> {
    let item = x::ITEMS.iter().find(|i| i.name == x::PORTAL_GUN).unwrap();
    let template = *by_name.get(item.template).ok_or("extras: no template for the portal gun")?;
    let pistol = db.prefab_cached(template)?;
    let file = template.file;
    // The template's file reaches the extras file through an external (the crab's, when they share a file).
    let externals = &mut db.files[file].externals;
    let extras = match externals.iter().position(|e| e.path == FILE) {
        Some(i) => i + 1,
        None => {
            externals.push(External { guid: [0; 16], kind: 0, path: FILE.to_string() });
            externals.len()
        }
    } as i32;
    let t = &pistol.objects[0];
    let mut next = ROOT;
    let go = ObjRef { file, path_id: ROOT };
    let local = PPtr { file: 0, path_id: ROOT };
    let [w, h] = x::art::GUN_SIZE.map(|v| v / x::PPU);
    let [hx, hy] = x::art::GUN_HANDLE.map(|v| v / x::PPU);
    let mut o = PrefabObject { game_object: go, name: x::PORTAL_GUN.into(), parent: None, children: Vec::new(), components: Vec::new(), ..t.clone() };
    let mut p = Prefab::default();
    let mut collider = false;
    for (c, (_, comp)) in t.components.iter().enumerate() {
        let mut comp = comp.clone();
        match &mut comp {
            Component::Transform(tr) => {
                tr.children = Vec::new();
                tr.local_position = [0.0; 3];
                tr.game_object = local;
            }
            Component::SpriteRenderer(r) => {
                r.sprite = PPtr { file: extras, path_id: SPRITE };
                r.game_object = local;
            }
            // The Pistol has two boxes (grip and barrel); the gun has one, its sprite's size.
            Component::Collider2D(_) if collider => continue,
            Component::Collider2D(col) => {
                collider = true;
                col.offset = [0.0; 2];
                col.game_object = local;
                let ColliderShape::Box { size, .. } = &mut col.shape else { return Err("extras: the Pistol's collider".into()) };
                *size = [w, h];
            }
            Component::Rigidbody2D(b) => b.game_object = local,
            Component::Script { class, fields, .. } if class == "PhysicalBehaviour" => {
                set(fields, "LocalColliderGridPoints", Value::Array(Vec::new()));
                set(fields, "Circumference", Value::Float(-1.0));
                let at = Value::Struct(vec![("x".into(), Value::Float(hx)), ("y".into(), Value::Float(hy)), ("z".into(), Value::Float(0.0))]);
                set(fields, "HoldingPositions", Value::Array(vec![at]));
            }
            Component::Script { .. } => continue,
            _ => {}
        }
        if let Some(&m) = pistol.materials.get(&(0, c)) {
            p.materials.insert((0, o.components.len()), m);
        }
        next += 1;
        o.components.push((ObjRef { file, path_id: next }, comp));
    }
    p.index.insert(go, (0, usize::MAX));
    for (c, (r, _)) in o.components.iter().enumerate() {
        p.index.insert(*r, (0, c));
    }
    p.objects.push(o);
    db.cache_prefab(go, p);
    by_name.insert(x::PORTAL_GUN.to_string(), go);
    Ok(())
}
