//! First launch without any setup: the per-user folder, the installed game's version check (1.28.41 or 1.28.52, "1.28.45" in the code, by content hash),
//! a prompt in the rewrite's window when the game is not found, and the human asset cache built from the installed game (what
//! scripts/prepare-assets.py made from an AssetRipper export, field for field).

use crate::assets::components::{ColliderShape, JointKind, PhysicsMaterial2D, Texture2D};
use crate::assets::db::{AssetDb, Component};
use crate::assets::mono::Value;
use crate::assets::serialized::Reader;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// SHA-256 of every installed file the rewrite reads, as People Playground 1.28.41 ships them (GAME_HASHES_45: the newer build,
/// 1.28.52, which the rewrite calls 1.28.45) (`sha256sum` lines).
const GAME_HASHES: &str = include_str!("game-1.28.41.sha256");
const GAME_HASHES_45: &str = include_str!("game-1.28.52.sha256");

/// %LOCALAPPDATA%/ppg-rewrite (elsewhere $XDG_DATA_HOME or ~/.local/share).
pub fn user_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| ".".into());
    base.join("ppg-rewrite")
}

pub fn default_cache() -> PathBuf {
    user_dir().join("cache")
}

fn remembered_file() -> PathBuf {
    user_dir().join("game-folder.txt")
}

/// The game folder picked or dropped on an earlier launch.
pub fn remembered_game() -> Option<PathBuf> {
    Some(std::fs::read_to_string(remembered_file()).ok()?.trim().into())
}

pub fn remember_game(game: &Path) {
    if remembered_game().as_deref() != Some(game) {
        let _ = std::fs::create_dir_all(user_dir());
        let _ = std::fs::write(remembered_file(), game.to_string_lossy().as_bytes());
    }
}

fn stamp(path: &Path) -> Option<String> {
    let m = path.metadata().ok()?;
    Some(format!("{} {}", m.len(), m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos()))
}

/// Stops unless every file in GAME_HASHES has 1.28.45's or 1.28.41's content. Hashing reads ~330 MB, so the size and time of
/// files that matched are kept in the user folder and only changed files are hashed again.
pub fn check_version(game: &Path) -> Result<(), String> {
    // Either supported version: 1.28.52 (the current Steam one; "1.28.45" in the code) or 1.28.41.
    check_hashes(game, GAME_HASHES_45).or_else(|_| check_hashes(game, GAME_HASHES))
}

fn check_hashes(game: &Path, list: &str) -> Result<(), String> {
    let wrong = || "This rewrite needs People Playground 1.28.52 (the current Steam version) or 1.28.41; your install is a different version.".to_string();
    let verified_file = user_dir().join("verified.txt");
    let verified = std::fs::read_to_string(&verified_file).unwrap_or_default();
    let mut lines = Vec::new();
    let mut changed = false;
    for entry in list.lines() {
        let (hash, relative) = entry.split_once("  ").ok_or("bad hash list")?;
        let path = game.join(relative);
        let key = format!("{}\t{}", path.display(), stamp(&path).ok_or_else(|| format!("{} (missing {relative})", wrong()))?);
        if !verified.lines().any(|l| l == key) {
            let mut hasher = Sha256::new();
            let mut file = std::fs::File::open(&path).map_err(|e| format!("{relative}: {e}"))?;
            std::io::copy(&mut file, &mut hasher).map_err(|e| format!("{relative}: {e}"))?;
            let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if got != hash {
                return Err(format!("{} ({relative} differs)", wrong()));
            }
            changed = true;
        }
        lines.push(key);
    }
    if changed {
        let _ = std::fs::create_dir_all(user_dir());
        let _ = std::fs::write(verified_file, lines.join("\n"));
    }
    Ok(())
}

/// A folder (or the game's exe) given by the player: the installation it names, if complete. The data folder picked by
/// mistake counts as its game folder.
fn game_in(path: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(path.to_string_lossy().trim().trim_matches('"'));
    [Some(path.clone()), path.parent().map(Path::to_path_buf)]
        .into_iter()
        .flatten()
        .find_map(|p| crate::game_assets::find_game(Some(p)).ok())
}

/// The game was not found: the rewrite's own window asks for its folder, dropped on it or its path typed or pasted.
/// None when the window is closed without one.
pub fn ask_for_game(reason: &str) -> Option<PathBuf> {
    use bevy::input::keyboard::KeyboardInput;
    use bevy::prelude::*;
    use bevy::ui::RelativeCursorPosition;
    use bevy::window::{FileDragAndDrop, WindowResolution};
    use std::sync::{Arc, Mutex};

    #[derive(Resource)]
    struct Prompt {
        text: String,
        message: String,
        chosen: Arc<Mutex<Option<PathBuf>>>,
    }
    #[derive(Component)]
    struct Field;
    #[derive(Component)]
    struct Message;
    #[derive(Component)]
    struct Ok;

    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    let reason = reason.to_string();
    let setup = move |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands
            .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(14), padding: UiRect::all(px(28)), width: percent(100), ..default() })
            .with_children(|c| {
                c.spawn((Text::new("Drop your People Playground folder here, or paste its path"), font(20.0)));
                c.spawn((
                    Text::new(format!(
                        "Steam: right-click People Playground > Manage > Browse local files to find it.\n({reason})"
                    )),
                    font(15.0),
                    TextColor(Color::srgb(0.7, 0.72, 0.76)),
                ));
                c.spawn((Node { padding: UiRect::all(px(8)), border: UiRect::all(px(1)), ..default() }, BorderColor::all(Color::srgb(0.4, 0.42, 0.46)), BackgroundColor(Color::srgb(0.1, 0.11, 0.13))))
                    .with_child((Text::new("|"), font(16.0), Field));
                c.spawn((Node { padding: UiRect::axes(px(22), px(6)), align_self: AlignSelf::FlexStart, ..default() }, BackgroundColor(Color::srgb(0.25, 0.27, 0.31)), RelativeCursorPosition::default(), Ok))
                    .with_child((Text::new("OK"), font(16.0)));
                c.spawn((Text::new(""), font(15.0), TextColor(Color::srgb(1.0, 0.5, 0.45)), Message));
            });
    };
    let input = |mut keys: MessageReader<KeyboardInput>,
                 held: Res<ButtonInput<KeyCode>>,
                 mouse: Res<ButtonInput<MouseButton>>,
                 mut drops: MessageReader<FileDragAndDrop>,
                 ok: Query<&RelativeCursorPosition, With<Ok>>,
                 mut p: ResMut<Prompt>,
                 mut exit: MessageWriter<AppExit>| {
        let mut submit = mouse.just_pressed(MouseButton::Left) && ok.iter().any(|r| r.cursor_over());
        let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        for k in keys.read().filter(|k| k.state.is_pressed()) {
            match k.key_code {
                KeyCode::Enter | KeyCode::NumpadEnter => submit = true,
                KeyCode::Backspace => drop(p.text.pop()),
                KeyCode::KeyV if ctrl => p.text += clipboard_text().trim(),
                _ if !ctrl => p.text.extend(k.text.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control())),
                _ => {}
            }
        }
        for d in drops.read() {
            if let FileDragAndDrop::DroppedFile { path_buf, .. } = d {
                p.text = path_buf.display().to_string();
                submit = true;
            }
        }
        if submit {
            match game_in(Path::new(&p.text)) {
                Some(game) => {
                    *p.chosen.lock().unwrap() = Some(game);
                    exit.write(AppExit::Success);
                }
                None => p.message = format!("Not a People Playground folder: {}", p.text.trim()),
            }
        }
    };
    let show = |p: Res<Prompt>, mut field: Query<&mut Text, (With<Field>, Without<Message>)>, mut message: Query<&mut Text, With<Message>>| {
        if p.is_changed() {
            for mut t in &mut field {
                t.0 = format!("{}|", p.text);
            }
            for mut t in &mut message {
                t.0 = p.message.clone();
            }
        }
    };
    let chosen = Arc::new(Mutex::new(None));
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "People Playground rewrite".into(), resolution: WindowResolution::new(860, 300), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.08)))
        .insert_resource(Prompt { text: String::new(), message: String::new(), chosen: chosen.clone() })
        .add_systems(Startup, setup)
        .add_systems(Update, (input, show).chain())
        .run();
    chosen.lock().unwrap().take()
}

/// The clipboard's text (Ctrl+V in the folder prompt).
#[cfg(windows)]
fn clipboard_text() -> String {
    use windows_sys::Win32::System::{DataExchange::*, Memory::*};
    const CF_UNICODETEXT: u32 = 13;
    let mut text = String::new();
    // SAFETY: the clipboard is opened and closed here; the locked buffer is a NUL-terminated UTF-16 string.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return text;
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let n = (0..).take_while(|&i| *p.add(i) != 0).count();
                text = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
    }
    text
}

#[cfg(not(windows))]
fn clipboard_text() -> String {
    String::new()
}

/// The asset cache, built from the installed game the first time.
pub fn ensure_cache(cache: &Path, game: &Path) -> Result<(), String> {
    if cache.join("human.json").is_file() {
        return Ok(());
    }
    eprintln!("Preparing the asset cache from your installed game: {}", cache.display());
    build_cache(game, cache)
}

// --- JSON as Python's json.dumps(indent=2) writes it (the cache prepare-assets.py made), CRLF as its write_text on Windows.

enum J {
    F(f64),
    I(i64),
    B(bool),
    S(String),
    Null,
    A(Vec<J>),
    O(Vec<(&'static str, J)>),
}

/// Python's repr of a float: the shortest round trip, fixed between 1e-4 and 1e16, otherwise 1e-05 / 1.5e+16.
fn py_float(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let s = format!("{v:e}");
    let (mantissa, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let mut out = if v < 0.0 { "-".to_string() } else { String::new() };
    if (-4..16).contains(&exp) {
        if exp < 0 {
            out += &format!("0.{}{digits}", "0".repeat((-exp - 1) as usize));
        } else {
            let point = exp as usize + 1;
            if digits.len() <= point {
                out += &format!("{digits}{}.0", "0".repeat(point - digits.len()));
            } else {
                out += &format!("{}.{}", &digits[..point], &digits[point..]);
            }
        }
    } else {
        out += &digits[..1];
        if digits.len() > 1 {
            out += &format!(".{}", &digits[1..]);
        }
        out += &format!("e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    out
}

fn py_string(s: &str) -> String {
    let mut out = String::new();
    for c in serde_json::to_string(s).unwrap().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for u in c.encode_utf16(&mut [0; 2]) {
                out += &format!("\\u{u:04x}");
            }
        }
    }
    out
}

fn write_json(v: &J, indent: usize, out: &mut String) {
    let pad = |n: usize| "\r\n".to_string() + &" ".repeat(n);
    match v {
        J::F(f) => *out += &py_float(*f),
        J::I(i) => *out += &i.to_string(),
        J::B(b) => *out += if *b { "true" } else { "false" },
        J::S(s) => *out += &py_string(s),
        J::Null => *out += "null",
        J::A(items) if items.is_empty() => *out += "[]",
        J::O(items) if items.is_empty() => *out += "{}",
        J::A(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                *out += if i > 0 { "," } else { "" };
                *out += &pad(indent + 2);
                write_json(item, indent + 2, out);
            }
            *out += &pad(indent);
            out.push(']');
        }
        J::O(items) => {
            out.push('{');
            for (i, (key, item)) in items.iter().enumerate() {
                *out += if i > 0 { "," } else { "" };
                *out += &pad(indent + 2);
                *out += &format!("{}: ", py_string(key));
                write_json(item, indent + 2, out);
            }
            *out += &pad(indent);
            out.push('}');
        }
    }
}

// --- The human cache.

/// A float as AssetRipper's YAML writes it (the shortest decimal) read back by Python: the f64 nearest that decimal.
fn d(x: f32) -> f64 {
    format!("{x}").parse().unwrap()
}

fn xy(v: [f32; 2]) -> J {
    J::A(vec![J::F(d(v[0])), J::F(d(v[1]))])
}

fn radians_f32(degrees: f32) -> f64 {
    (degrees * 0.017453292f32) as f64
}

fn num(v: Option<&Value>, name: &str) -> Result<f64, String> {
    match v {
        Some(Value::Float(f)) => Ok(d(*f)),
        Some(Value::Double(f)) => Ok(*f),
        Some(v) if v.i64().is_some() => Ok(v.i64().unwrap() as f64),
        _ => Err(format!("human prefab: no number {name}")),
    }
}

fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, String> {
    v.get(name).ok_or_else(|| format!("human prefab: no field {name}"))
}

fn flag(v: &Value, name: &str) -> Result<bool, String> {
    let f = field(v, name)?;
    f.bool().or_else(|| f.i64().map(|i| i != 0)).ok_or_else(|| format!("human prefab: {name} is not a bool"))
}

fn int(v: &Value, name: &str) -> Result<i64, String> {
    field(v, name)?.i64().ok_or_else(|| format!("human prefab: {name} is not an int"))
}

fn f(v: &Value, name: &str) -> Result<J, String> {
    Ok(J::F(num(v.get(name), name)?))
}

fn curve(c: &Value) -> Result<J, String> {
    let mut keys = Vec::new();
    for k in field(c, "m_Curve")?.array().unwrap_or_default() {
        if int(k, "weightedMode")? != 0 {
            return Err("Weighted animation curve keys are unsupported".into());
        }
        keys.push(J::A(["time", "value", "inSlope", "outSlope"].iter().map(|n| f(k, n)).collect::<Result<_, _>>()?));
    }
    Ok(J::O(vec![("keys", J::A(keys)), ("pre", J::I(int(c, "m_PreInfinity")?)), ("post", J::I(int(c, "m_PostInfinity")?))]))
}

fn poses(person: &Value) -> Result<J, String> {
    let mut result = Vec::new();
    for pose in field(person, "Poses")?.array().unwrap_or_default() {
        let mut angles = Vec::new();
        for angle in field(pose, "Angles")?.array().unwrap_or_default() {
            let Some(Value::Float(degrees)) = angle.get("Angle") else { return Err("pose angle is not a float".into()) };
            let mut a = vec![
                ("part", J::S(field(angle, "Name")?.str_()?)),
                ("angle", J::F(radians_f32(*degrees))),
                ("angle_degrees", J::F(d(*degrees))),
                ("rigidity_modifier", f(angle, "PoseRigidityModifier")?),
            ];
            if flag(angle, "Animated")? {
                a.push(("animation", J::O(vec![
                    ("start", f(angle, "StartAngle")?),
                    ("end", f(angle, "EndAngle")?),
                    ("duration", f(angle, "AnimationDuration")?),
                    ("random_influence", f(angle, "RandomInfluence")?),
                    ("random_speed", f(angle, "RandomSpeed")?),
                    ("time_offset", f(angle, "TimeOffset")?),
                    ("curve", curve(field(angle, "AnimationCurve")?)?),
                ])));
            }
            angles.push(J::O(a));
        }
        result.push(J::O(vec![
            ("name", J::S(field(pose, "Name")?.str_()?)),
            ("state", J::I(int(pose, "State")?)),
            ("rigidity", f(pose, "Rigidity")?),
            ("force_multiplier", f(pose, "ForceMultiplier")?),
            ("stand_upright", J::B(flag(pose, "ShouldStandUpright")?)),
            ("upright_multiplier", f(pose, "UprightForceMultiplier")?),
            ("drag_influence", f(pose, "DragInfluence")?),
            ("should_stumble", J::B(flag(pose, "ShouldStumble")?)),
            ("animation_speed", f(pose, "AnimationSpeedMultiplier")?),
            ("angles", J::A(angles)),
        ]));
    }
    match result.first() {
        Some(J::O(p)) if matches!(&p[0].1, J::S(n) if n == "Rest") => Ok(J::A(result)),
        _ => Err("Human prefab has no default rest pose".into()),
    }
}

trait StrValue {
    fn str_(&self) -> Result<String, String>;
}

impl StrValue for Value {
    fn str_(&self) -> Result<String, String> {
        match self {
            Value::Str(s) => Ok(s.clone()),
            _ => Err("human prefab: not a string".into()),
        }
    }
}

fn simulation(db: &AssetDb) -> Result<J, String> {
    let file = db.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
    let time = db.files[file].objects.iter().find(|o| o.class_id == 5).ok_or("no TimeManager")?;
    let fixed = Reader::new(db.data(crate::assets::db::ObjRef { file, path_id: time.path_id })).f32()?;
    let p = crate::game::Game::load_physics2d_settings(db)?;
    Ok(J::O(vec![
        ("fixed_delta", J::F(d(fixed))),
        ("gravity", xy([p.gravity.x, p.gravity.y])),
        ("velocity_iterations", J::I(p.velocity_iterations as i64)),
        ("position_iterations", J::I(p.position_iterations as i64)),
        ("max_angular_correction_degrees", J::F(d(p.max_angular_correction))),
        ("max_translation", J::F(d(p.max_translation_speed))),
        ("max_rotation_degrees", J::F(d(p.max_rotation_speed))),
    ]))
}

/// prepare-assets.py's `prepare`, reading the installed game instead of an AssetRipper export. The texture is named
/// after its Texture2D (AssetRipper named it by the GUID it made up for the export).
pub fn build_cache(game: &Path, output: &Path) -> Result<(), String> {
    let mut db = AssetDb::open(game)?;
    let catalog = *db.find_scripts("CatalogData")?.first().ok_or("no CatalogData")?;
    let (_, _, catalog_data) = db.mono(catalog)?.ok_or("no CatalogData")?;
    let mut person = None;
    for item in catalog_data.get("Items").and_then(Value::array).ok_or("no catalog items")?.to_vec() {
        let Some(asset) = item.pptr().and_then(|p| db.resolve(catalog.file, p)) else { continue };
        let Some((_, _, spawnable)) = db.mono(asset)? else { continue };
        let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
        if db.game_object(prefab)?.name == "Person" {
            person = Some(prefab);
            break;
        }
    }
    let prefab = db.prefab(person.ok_or("no Person prefab in the catalog")?)?;
    let transform = |o: usize| {
        prefab.objects[o].components.iter().find_map(|(_, c)| match c {
            Component::Transform(t) => Some(t.clone()),
            _ => None,
        })
    };
    // pose(): the world position, angle and scale through the parents, in f64 as Python computes it.
    fn pose(o: usize, prefab: &crate::assets::db::Prefab, transform: &dyn Fn(usize) -> Option<crate::assets::classes::Transform>) -> ([f64; 2], f64, [f64; 2]) {
        let t = transform(o).expect("a Transform");
        let mut position = [d(t.local_position[0]), d(t.local_position[1])];
        let mut scale = [d(t.local_scale[0]), d(t.local_scale[1])];
        let mut angle = 2.0 * d(t.local_rotation[2]).atan2(d(t.local_rotation[3]));
        if let Some(parent) = prefab.objects[o].parent {
            let (p, a, s) = pose(parent, prefab, transform);
            let (x, y) = (position[0] * s[0], position[1] * s[1]);
            position = [p[0] + a.cos() * x - a.sin() * y, p[1] + a.sin() * x + a.cos() * y];
            angle += a;
            scale = [scale[0] * s[0], scale[1] * s[1]];
        }
        (position, angle, scale)
    }
    let body_of = |r| prefab.objects.iter().position(|o| o.components.iter().any(|(c, k)| *c == r && matches!(k, Component::Rigidbody2D(_))));
    let mut parts = Vec::new();
    let mut joints = Vec::new();
    let mut textures: Vec<(String, crate::assets::db::ObjRef)> = Vec::new();
    let mut person_fields = None;
    for (o, object) in prefab.objects.iter().enumerate() {
        for (_, c) in &object.components {
            if let Component::Script { fields, .. } = c
                && fields.get("Poses").is_some()
                && person_fields.is_none()
            {
                person_fields = Some(fields.clone());
            }
        }
        let Some((body_ref, body)) = object.components.iter().find_map(|(r, c)| match c {
            Component::Rigidbody2D(b) => Some((*r, b.clone())),
            _ => None,
        }) else {
            continue;
        };
        let name = object.name.clone();
        let (position, angle, scale) = pose(o, &prefab, &transform);
        if scale.iter().any(|s| (s - 1.0).abs() > 0.0001) {
            return Err("Scaled rigid bodies need explicit collider conversion".into());
        }
        let renderer = object.components.iter().find_map(|(r, c)| match c {
            Component::SpriteRenderer(s) => Some((*r, s.clone())),
            _ => None,
        }).ok_or_else(|| format!("{name}: no SpriteRenderer"))?;
        let sprite_ref = db.resolve(renderer.0.file, renderer.1.sprite).ok_or_else(|| format!("{name}: no sprite"))?;
        let sprite = db.sprite(sprite_ref)?;
        let texture_ref = db.resolve(sprite_ref.file, sprite.texture).ok_or_else(|| format!("{name}: no texture"))?;
        let texture = Texture2D::read(db.data(texture_ref))?;
        let file_name = format!("{}.png", texture.name);
        if textures.iter().any(|(n, r)| *n == file_name && *r != texture_ref) {
            return Err(format!("Two textures named {}", texture.name));
        }
        if !textures.iter().any(|(n, _)| *n == file_name) {
            textures.push((file_name.clone(), texture_ref));
        }
        let (width, height) = (texture.width as f64, texture.height as f64);
        let [x, y, w, h] = sprite.rect.map(d);
        if !(w > 0.0 && h > 0.0 && x >= 0.0 && y >= 0.0 && x + w <= width && y + h <= height) {
            return Err("Sprite rectangle is outside its texture".into());
        }
        let mut colliders = Vec::new();
        let mut collider_materials = Vec::new();
        for (r, c) in &object.components {
            if let Component::Collider2D(c) = c
                && let ColliderShape::Box { size, .. } = c.shape
                && !c.is_trigger
            {
                colliders.push(J::O(vec![("offset", xy(c.offset)), ("size", xy(size))]));
                collider_materials.extend(db.resolve(r.file, c.material));
            }
        }
        if colliders.is_empty() {
            return Err(format!("No supported collider on {name}"));
        }
        let script = |key: &str| object.components.iter().find_map(|(r, c)| match c {
            Component::Script { fields, .. } if fields.get(key).is_some() => Some((*r, fields.clone())),
            _ => None,
        });
        let (_, limb) = script("BaseStrength").ok_or("Human body has no limb strength settings")?;
        let multipliers = limb.get("StrengthMultipliers").and_then(Value::array).unwrap_or_default();
        let muscle = J::O(vec![
            ("strength", f(&limb, "BaseStrength")?),
            ("multipliers", J::A(multipliers.iter().map(|m| num(Some(m), "StrengthMultipliers").map(J::F)).collect::<Result<_, _>>()?)),
            ("upright_force", f(&limb, "FakeUprightForce")?),
            ("has_brain", J::B(flag(&limb, "HasBrain")?)),
            ("do_balance_jerk", J::B(flag(&limb, "DoBalanceJerk")?)),
            ("balance_muscle_movement", f(&limb, "BalanceMuscleMovement")?),
            ("do_stumble", J::B(flag(&limb, "DoStumble")?)),
            ("blood_strength_ratio", f(&limb, "BloodMuscleStrengthRatio")?),
        ]);
        // The body's material, else its PhysicalProperties' PhysicMaterial.
        let material_ref = match db.resolve(body_ref.file, body.material) {
            Some(m) => m,
            None => {
                let (r, physical) = script("Properties").ok_or("Human physical properties are missing")?;
                let properties = physical.get("Properties").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p)).ok_or("Human physical properties are missing")?;
                let (_, _, props) = db.mono(properties)?.ok_or("Human physical properties are missing")?;
                props.get("PhysicMaterial").and_then(Value::pptr).and_then(|p| db.resolve(properties.file, p))
                    .ok_or("Human physics material is missing; implicit Unity defaults are unsupported")?
            }
        };
        let material = PhysicsMaterial2D::read(db.data(material_ref))?;
        for m in collider_materials {
            let other = PhysicsMaterial2D::read(db.data(m))?;
            if other.friction != material.friction || other.bounciness != material.bounciness {
                return Err("Different collider materials on one body are unsupported".into());
            }
        }
        if !material.friction.is_finite() || material.friction < 0.0 || !(0.0..=1.0).contains(&material.bounciness) {
            return Err("Invalid physics material coefficients".into());
        }
        parts.push(J::O(vec![
            ("name", J::S(name.clone())),
            ("position", J::A(vec![J::F(position[0]), J::F(position[1])])),
            ("angle", J::F(angle)),
            ("mass", J::F(d(body.mass))),
            ("linear_drag", J::F(d(body.linear_drag))),
            ("angular_drag", J::F(d(body.angular_drag))),
            ("colliders", J::A(colliders)),
            ("texture", J::S(file_name)),
            ("rect", J::A(vec![J::F(x), J::F(height - y - h), J::F(w), J::F(h)])),
            ("size", J::A(vec![J::F(w / d(sprite.pixels_to_units)), J::F(h / d(sprite.pixels_to_units))])),
            ("pivot", xy(sprite.pivot)),
            ("layer", J::I(renderer.1.sorting_order as i64)),
            ("muscle", muscle),
            ("physics_material", J::O(vec![("friction", J::F(d(material.friction))), ("restitution", J::F(d(material.bounciness)))])),
        ]));
        for (r, c) in &object.components {
            let Component::Joint2D(j) = c else { continue };
            let JointKind::Hinge { use_limits, lower, upper, .. } = j.kind else { continue };
            let other = db.resolve(r.file, j.connected_body).and_then(body_of).ok_or_else(|| format!("{name}: hinge to no body"))?;
            joints.push(J::O(vec![
                ("a", J::S(name.clone())),
                ("b", J::S(prefab.objects[other].name.clone())),
                ("anchor_a", xy(j.anchor)),
                ("anchor_b", xy(j.connected_anchor)),
                ("limits", if use_limits { J::A(vec![J::F(radians_f32(lower)), J::F(radians_f32(upper))]) } else { J::Null }),
                ("limits_degrees", if use_limits { J::A(vec![J::F(d(lower)), J::F(d(upper))]) } else { J::Null }),
            ]));
        }
    }
    if joints.len() + 1 != parts.len() {
        return Err("Human rig is not a single named body tree".into());
    }
    let mut stamps = Vec::new();
    for relative in crate::game_assets::REQUIRED {
        let m = game.join(relative).metadata().map_err(|e| format!("{relative}: {e}"))?;
        let ns = m.modified().map_err(|e| e.to_string())?.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        stamps.push(J::O(vec![("path", J::S(relative.into())), ("bytes", J::I(m.len() as i64)), ("modified_ns", J::I(ns as i64))]));
    }
    let rig = J::O(vec![
        ("source_files", J::A(stamps)),
        ("parts", J::A(parts)),
        ("joints", J::A(joints)),
        ("poses", poses(&person_fields.ok_or("Human prefab has no poses")?)?),
        ("simulation", simulation(&db)?),
    ]);
    let mut text = String::new();
    write_json(&rig, 0, &mut text);
    std::fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    // Textures first and human.json last, each through a temporary name: a cache is complete once human.json exists.
    let write = |name: &str, bytes: &[u8]| {
        let tmp = output.join(format!("{name}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, output.join(name))).map_err(|e| format!("{name}: {e}"))
    };
    for (name, r) in textures {
        let texture = Texture2D::read(db.data(r))?;
        let rgba = texture.rgba(&db.data_dir)?;
        let row = texture.width as usize * 4;
        // Unity's rows are bottom first, PNG's top first.
        let flipped: Vec<u8> = rgba.chunks_exact(row).rev().flatten().copied().collect();
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, texture.width as u32, texture.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&flipped)).map_err(|e| e.to_string())?;
        write(&name, &png)?;
    }
    write("human.json", text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_python_repr() {
        for (v, s) in [(0.1, "0.1"), (1.0, "1.0"), (-0.0, "-0.0"), (1e-5, "1e-05"), (0.0001, "0.0001"), (1e16, "1e+16"), (123456.5, "123456.5"), (0.8765504999999999, "0.8765504999999999"), (-9.81, "-9.81"), (1.5e-7, "1.5e-07")] {
            assert_eq!(py_float(v), s);
        }
    }

    #[test]
    fn a_dropped_or_pasted_game_path_is_recognised() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let data = game.join("People Playground_Data");
        let exe = game.join("People Playground.exe");
        let quoted = PathBuf::from(format!("  \"{}\" ", game.display()));
        for given in [&game, &data, &exe, &quoted] {
            assert_eq!(game_in(given).as_deref(), Some(game.as_path()), "{}", given.display());
        }
        assert!(game_in(&std::env::temp_dir().join("no-ppg-here-6e1")).is_none());
    }

    /// The cache built from the installed game equals the AssetRipper one in local-cache (when both are here), field for
    /// field, apart from the texture's file name; the texture's pixels are the same.
    #[test]
    fn built_cache_matches_prepare_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Ok(old) = std::fs::read_to_string(root.join("local-cache/human.json")) else { return };
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        // Only a cache made from this same install compares (local-cache is whichever install built it).
        let made_from: serde_json::Value = serde_json::from_str(&old).unwrap();
        let same_install = made_from["source_files"].as_array().into_iter().flatten().all(|f| {
            let ns = |m: &std::fs::Metadata| m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_nanos() as u64);
            std::fs::metadata(game.join(f["path"].as_str().unwrap_or(""))).is_ok_and(|m| Some(m.len()) == f["bytes"].as_u64() && ns(&m) == f["modified_ns"].as_u64())
        });
        if !same_install {
            return;
        }
        let out = std::env::temp_dir().join(format!("ppg-cache-test-{}", std::process::id()));
        build_cache(&game, &out).unwrap();
        let new = std::fs::read_to_string(out.join("human.json")).unwrap();
        let (mut a, b): (serde_json::Value, serde_json::Value) = (serde_json::from_str(&old).unwrap(), serde_json::from_str(&new).unwrap());
        let old_texture = a["parts"][0]["texture"].as_str().unwrap().to_string();
        let new_texture = b["parts"][0]["texture"].as_str().unwrap().to_string();
        for p in a["parts"].as_array_mut().unwrap() {
            p["texture"] = new_texture.clone().into();
        }
        assert!(a == b, "the built human.json differs");
        assert_eq!(old.replace(&old_texture, &new_texture), new, "not byte for byte");
        let decode = |p: PathBuf| {
            let mut r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap())).read_info().unwrap();
            let mut buf = vec![0; r.output_buffer_size().unwrap()];
            let info = r.next_frame(&mut buf).unwrap();
            (info.width, info.height, info.color_type, buf)
        };
        assert!(decode(root.join("local-cache").join(&old_texture)) == decode(out.join(&new_texture)), "texture pixels differ");
        eprintln!("built cache matches local-cache ({} bytes of human.json)", new.len());
        std::fs::remove_dir_all(out).unwrap();
    }

    /// Both hash lists name the same files in sha256sum's format, and 1.28.52's Assembly-CSharp hash is the one il.rs knows.
    #[test]
    fn hash_lists_cover_the_same_files() {
        let files = |l: &str| l.lines().map(|e| { let (h, f) = e.split_once("  ").unwrap(); assert_eq!(h.len(), 64); f.to_string() }).collect::<Vec<_>>();
        assert_eq!(files(GAME_HASHES), files(GAME_HASHES_45));
        assert!(GAME_HASHES_45.contains("d9b0e6fbd8cb09b2b14877aaf43878c76f775eb78d29880438126bbe32711572  People Playground_Data/Managed/Assembly-CSharp.dll"));
    }
}
