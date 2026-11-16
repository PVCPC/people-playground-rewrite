//! The Menu and Main scenes' authored UI, read from the player's own install at startup (nothing from the scenes is
//! copied into the repo): every RectTransform and MonoBehaviour of the scene objects, by scene and hierarchy path.

use crate::assets::{
    classes::{RectTransform, Transform},
    db::{AssetDb, ObjRef},
    mono::Value,
};
use bevy::prelude::Color;
use std::{collections::HashMap, sync::OnceLock};

/// The scenes by build index: level0 is Menu, level1 is Main.
const SCENES: [(&str, &str); 2] = [("level0", "Menu"), ("level1", "Main")];

/// One scene object: its RectTransform, its MonoBehaviours' fields by script class, and the assets their PPtr fields name.
#[derive(Default)]
struct Authored {
    rect: Option<RectTransform>,
    mono: HashMap<String, Value>,
    /// (class, field) -> the object the field points at.
    refs: HashMap<(String, String), ObjRef>,
    active: bool,
    /// The children's names in sibling order (layout groups place them so).
    children: Vec<String>,
}

static AUTHORED: OnceLock<HashMap<String, Authored>> = OnceLock::new();

/// A GameObject's hierarchy path ("Canvas/Main/Menu/Play") and its RectTransform.
fn locate(db: &AssetDb, go: ObjRef) -> Option<(String, Option<RectTransform>)> {
    let mut names = Vec::new();
    let mut rect = None;
    let mut at = Some(go);
    while let Some(g) = at {
        let object = db.game_object(g).ok()?;
        names.push(object.name);
        let transform = object.components.iter().filter_map(|&c| db.resolve(g.file, c)).find(|&c| matches!(db.class(c), 4 | 224));
        if g == go {
            rect = transform.and_then(|c| RectTransform::read(db.data(c)).ok());
        }
        at = transform.and_then(|c| {
            let father = db.resolve(g.file, Transform::read(db.data(c)).ok()?.father)?;
            db.resolve(father.file, Transform::read(db.data(father)).ok()?.game_object)
        });
    }
    names.reverse();
    Some((names.join("/"), rect))
}

/// Index the scenes' objects.
pub fn load(db: &mut AssetDb) -> Result<(), String> {
    let mut all = HashMap::new();
    for (file, scene) in SCENES {
        let Some(f) = db.file_index(file) else { return Err(format!("{scene}: no {file} in the game's data")) };
        let ids: Vec<i64> = db.files[f].objects.iter().filter(|o| o.class_id == 114).map(|o| o.path_id).collect();
        for path_id in ids {
            let r = ObjRef { file: f, path_id };
            let Ok(Some((script, mb, v))) = db.mono(r) else { continue };
            let Some(go) = db.resolve(f, mb.game_object) else { continue };
            let Some((p, rect)) = locate(db, go) else { continue };
            let node: &mut Authored = all.entry(format!("{scene}:{p}")).or_default();
            node.rect = rect;
            node.active = db.game_object(go).is_ok_and(|g| g.active);
            if let Value::Struct(fields) = &v {
                for (name, value) in fields {
                    if let Some(target) = value.pptr().and_then(|p| db.resolve(f, p)) {
                        node.refs.insert((script.class_name.clone(), name.clone()), target);
                    }
                }
            }
            node.mono.insert(script.class_name, v);
        }
        // Objects with only a RectTransform (layout slots without scripts).
        let ids: Vec<i64> = db.files[f].objects.iter().filter(|o| o.class_id == 224).map(|o| o.path_id).collect();
        for path_id in ids {
            let Ok(t) = Transform::read(db.data(ObjRef { file: f, path_id })) else { continue };
            let Some(go) = db.resolve(f, t.game_object) else { continue };
            let Some((p, rect)) = locate(db, go) else { continue };
            let children = t.children.iter().filter_map(|&c| {
                let c = db.resolve(f, c)?;
                let g = db.resolve(c.file, Transform::read(db.data(c)).ok()?.game_object)?;
                db.game_object(g).ok().map(|g| g.name)
            }).collect();
            let node: &mut Authored = all.entry(format!("{scene}:{p}")).or_default();
            node.rect = node.rect.or(rect);
            node.active = db.game_object(go).is_ok_and(|g| g.active);
            node.children = children;
        }
    }
    // The prefabs the UI instantiates, by the script on their root.
    for (script, name) in PREFABS {
        for r in db.find_scripts(script)? {
            let Some((_, mb, _)) = db.mono(r)? else { continue };
            let Some(root) = db.resolve(r.file, mb.game_object) else { continue };
            // "": the prefab's own name (EnvironmentControlBehaviour is on both the Checkbox and the Number setting).
            let name = if name.is_empty() { db.game_object(root)?.name } else { name.to_string() };
            add_tree(db, root, &name, None, &mut all)?;
        }
    }
    let _ = AUTHORED.set(all);
    Ok(())
}

/// The UI prefabs: (the script class on the root, the name their paths are looked up under).
const PREFABS: [(&str, &str); 5] = [("ItemButtonBehaviour", "ItemButton"), ("CategoryButtonBehaviour", "CategoryButton"), ("EnvironmentControlBehaviour", ""), ("SettingTemplateBehaviour", "SettingTemplate"), ("DialogBox", "")];

/// Index a prefab's GameObject tree under `name`; paths are relative to the root (the root's own is ""). `at` is the parent's path.
fn add_tree(db: &mut AssetDb, go: ObjRef, name: &str, at: Option<&str>, all: &mut HashMap<String, Authored>) -> Result<(), String> {
    let object = db.game_object(go)?;
    let path = match at {
        None => String::new(),
        Some("") => object.name.clone(),
        Some(parent) => format!("{parent}/{}", object.name),
    };
    let mut node = Authored { active: object.active, ..Default::default() };
    let mut children = Vec::new();
    for c in &object.components {
        let Some(r) = db.resolve(go.file, *c) else { continue };
        match db.class(r) {
            4 | 224 => {
                node.rect = RectTransform::read(db.data(r)).ok().filter(|_| db.class(r) == 224);
                children = Transform::read(db.data(r))?.children;
            }
            114 => {
                let Some((script, _, v)) = db.mono(r)? else { continue };
                if let Value::Struct(fields) = &v {
                    for (field, value) in fields {
                        if let Some(target) = value.pptr().and_then(|p| db.resolve(go.file, p)) {
                            node.refs.insert((script.class_name.clone(), field.clone()), target);
                        }
                    }
                }
                node.mono.insert(script.class_name, v);
            }
            _ => {}
        }
    }
    all.insert(format!("{name}:{path}"), node);
    for c in children {
        let Some(t) = db.resolve(go.file, c) else { continue };
        let Some(child) = db.resolve(t.file, Transform::read(db.data(t))?.game_object) else { continue };
        add_tree(db, child, name, Some(&path), all)?;
    }
    Ok(())
}

fn node(scene: &str, path: &str) -> Option<&'static Authored> {
    AUTHORED.get()?.get(&format!("{scene}:{path}"))
}

fn required(scene: &str, path: &str) -> &'static Authored {
    node(scene, path).unwrap_or_else(|| panic!("no object at {scene}:{path} in the game's scene"))
}

/// Every TextMeshProUGUI text containing `needle`, with its font size.
pub fn texts_containing(needle: &str) -> Vec<(String, f32)> {
    let Some(all) = AUTHORED.get() else { return Vec::new() };
    all.values()
        .filter_map(|n| n.mono.get("TextMeshProUGUI"))
        .filter_map(|v| match (v.get("m_text"), v.get("m_fontSize").and_then(Value::f32)) {
            (Some(Value::Str(s)), Some(size)) if s.contains(needle) => Some((s.clone(), size)),
            _ => None,
        })
        .collect()
}

/// The font asset a TextMeshProUGUI at `path` uses.
pub fn font_asset(scene: &str, path: &str) -> Option<ObjRef> {
    asset_ref(scene, path, "TextMeshProUGUI", "m_fontAsset")
}

/// Whether the scene has an object at `path`.
pub fn has(scene: &str, path: &str) -> bool {
    node(scene, path).is_some()
}

/// A MonoBehaviour of the object at `path`, by class.
pub fn mono(scene: &str, path: &str, class: &str) -> Option<&'static Value> {
    node(scene, path)?.mono.get(class)
}

/// A MonoBehaviour field of the object at `path`.
pub fn field(scene: &str, path: &str, class: &str, name: &str) -> Option<&'static Value> {
    node(scene, path)?.mono.get(class)?.get(name)
}

/// The TextMeshPro text at `path` in `scene` ("Menu" or "Main"); empty before the scenes are loaded (headless tests).
pub fn text(scene: &str, path: &str) -> String {
    if AUTHORED.get().is_none() {
        return String::new();
    }
    match required(scene, path).mono.get("TextMeshProUGUI").and_then(|v| v.get("m_text")) {
        Some(Value::Str(s)) => s.clone(),
        _ => panic!("no text at {scene}:{path} in the game's scene"),
    }
}

/// A button's label: its text without the trailing newline some labels carry.
pub fn label(scene: &str, path: &str) -> String {
    text(scene, path).trim_end_matches('\n').to_string()
}

/// The HasTooltipBehaviour text of the GameObject at `path`; empty when it has none.
pub fn tooltip(scene: &str, path: &str) -> String {
    match field(scene, path, "HasTooltipBehaviour", "Text") {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The font size of the TextMeshPro text at `path`.
pub fn font_size(scene: &str, path: &str) -> f32 {
    field(scene, path, "TextMeshProUGUI", "m_fontSize").and_then(Value::f32).unwrap_or_else(|| panic!("no text at {scene}:{path} in the game's scene"))
}

/// The RectTransform at `path`.
pub fn rect(scene: &str, path: &str) -> RectTransform {
    required(scene, path).rect.unwrap_or_else(|| panic!("no RectTransform at {scene}:{path}"))
}

/// The names of the children of `path`, in sibling order.
pub fn children(scene: &str, path: &str) -> &'static [String] {
    &required(scene, path).children
}

/// Whether the GameObject at `path` is active in the scene.
pub fn active(scene: &str, path: &str) -> bool {
    required(scene, path).active
}

/// A numeric field of a MonoBehaviour at `path`.
pub fn number(scene: &str, path: &str, class: &str, name: &str) -> f32 {
    match field(scene, path, class, name) {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f32,
        Some(Value::UInt(i)) => *i as f32,
        Some(Value::Bool(b)) => f32::from(*b),
        _ => panic!("no {class}.{name} at {scene}:{path}"),
    }
}

/// A layout group's m_Padding (RectOffset, ints) as [left, right, top, bottom].
pub fn padding(scene: &str, path: &str, class: &str) -> [f32; 4] {
    let p = field(scene, path, class, "m_Padding").unwrap_or_else(|| panic!("no {class}.m_Padding at {scene}:{path}"));
    ["m_Left", "m_Right", "m_Top", "m_Bottom"].map(|k| p.get(k).and_then(Value::i64).map_or(0.0, |v| v as f32))
}

/// The object at `path` placed in a parent of size `parent` ([width, height], UI y up): [left, top, width, height] from the
/// parent's top left, as RectTransform anchors, pivot, anchored position and size delta resolve.
pub fn placed(scene: &str, path: &str, parent: [f32; 2]) -> [f32; 4] {
    let r = rect(scene, path);
    let axis = |i: usize| {
        let size = (r.anchor_max[i] - r.anchor_min[i]) * parent[i] + r.size_delta[i];
        let origin = (r.anchor_min[i] + (r.anchor_max[i] - r.anchor_min[i]) * r.pivot[i]) * parent[i] + r.anchored_position[i] - r.pivot[i] * size;
        (origin, size)
    };
    let (x, w) = axis(0);
    let (y, h) = axis(1);
    [x, parent[1] - y - h, w, h]
}

/// A layout group's m_Spacing.
pub fn spacing(scene: &str, path: &str, class: &str) -> f32 {
    number(scene, path, class, "m_Spacing")
}

/// A Vector2 field ({x, y}) of a MonoBehaviour at `path` (a GridLayoutGroup's m_CellSize or m_Spacing).
pub fn vec2(scene: &str, path: &str, class: &str, name: &str) -> [f32; 2] {
    let v = field(scene, path, class, name).unwrap_or_else(|| panic!("no {class}.{name} at {scene}:{path}"));
    ["x", "y"].map(|k| v.get(k).and_then(Value::f32).unwrap_or_else(|| panic!("no {class}.{name}.{k} at {scene}:{path}")))
}

/// A Color field of a MonoBehaviour at `path` (a Button's m_Colors.m_NormalColor is `color_in`).
pub fn color_field(scene: &str, path: &str, class: &str, name: &str) -> Color {
    field(scene, path, class, name).and_then(color_of).unwrap_or_else(|| panic!("no {class}.{name} at {scene}:{path}"))
}

/// A Selectable's (Button, TMP_InputField) transition colour ("m_NormalColor", "m_HighlightedColor", "m_PressedColor") as RGBA.
pub fn transition_color(scene: &str, path: &str, class: &str, name: &str) -> [f32; 4] {
    let c = field(scene, path, class, "m_Colors").and_then(|c| c.get(name)).unwrap_or_else(|| panic!("no {class}.m_Colors.{name} at {scene}:{path}"));
    ["r", "g", "b", "a"].map(|k| c.get(k).and_then(Value::f32).unwrap_or(0.0))
}

/// A Button's transition colour.
pub fn button_color(scene: &str, path: &str, name: &str) -> [f32; 4] {
    transition_color(scene, path, "Button", name)
}

/// A Selectable's m_Colors.m_FadeDuration.
pub fn transition_fade(scene: &str, path: &str, class: &str) -> f32 {
    field(scene, path, class, "m_Colors").and_then(|c| c.get("m_FadeDuration")).and_then(Value::f32).unwrap_or_else(|| panic!("no {class} fade at {scene}:{path}"))
}

/// A Color value.
pub fn color_of(v: &Value) -> Option<Color> {
    Some(Color::srgba(v.get("r")?.f32()?, v.get("g")?.f32()?, v.get("b")?.f32()?, v.get("a")?.f32()?))
}

/// The Image component's colour at `path`.
pub fn image_color(scene: &str, path: &str) -> Color {
    field(scene, path, "Image", "m_Color").and_then(color_of).unwrap_or_else(|| panic!("no Image at {scene}:{path}"))
}

/// The object a MonoBehaviour field of the object at `path` points at (an Image's m_Sprite, for one).
pub fn asset_ref(scene: &str, path: &str, class: &str, name: &str) -> Option<ObjRef> {
    node(scene, path)?.refs.get(&(class.to_string(), name.to_string())).copied()
}

#[cfg(test)]
mod tests {
    #[test]
    fn scene_text_is_read_from_the_install() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = crate::assets::db::AssetDb::open(&game).unwrap();
        super::load(&mut db).unwrap();
        assert!(super::text("Menu", "Canvas/Open-source licenses/info container/viewport/content/info").contains("NativeWebSocket"));
        assert_eq!(super::label("Menu", "Canvas/Main/Menu/Information"), "information");
        assert_eq!(super::tooltip("Main", "Canvas/Pause screen/Main/Menu/Resume"), "Resumes the game");
        let r = super::rect("Main", "Canvas/Catalog");
        assert!(r.size_delta[0] != 0.0 || r.anchor_max[0] != r.anchor_min[0]);
        assert!(super::asset_ref("Main", "Canvas/Toolbar/tab-powers/TextMeshPro Text", "TextMeshProUGUI", "m_fontAsset").is_some());
        // The prefab and the scrollbars the item buttons and scroll areas are built from.
        assert_eq!(super::rect("ItemButton", "button row/View info").size_delta, [27.0, 27.0]);
        assert!(super::asset_ref("ItemButton", "button row/View info", "Image", "m_Sprite").is_some());
        assert!(super::asset_ref("Main", "SpawnableOutline", "SpawnableOutlineBehaviour", "RectPrefab").is_some());
        for bar in ["Canvas/Open-source licenses/info container/Scrollbar", "Canvas/Settings/Settings menu/NEW SCROLL VIEW/Scrollbar Vertical"] {
            super::rect("Menu", &format!("{bar}/Sliding Area/Handle"));
            super::image_color("Menu", bar);
        }
        super::rect("Main", "Canvas/Catalog/ItemPanel/Viewport/Scrollbar/Sliding Area/Handle");
        assert!(!super::texts_containing("<i>").is_empty() && super::font_asset("Menu", "Canvas/Information/Info/version info (1)").is_some());
    }
}
