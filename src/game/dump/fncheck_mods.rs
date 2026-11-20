// The mod system's and the save data's twins, checked on DllCheck's `mods` rows (tools/dllcheck/ModsCheck.cs): its own
// encoding (a string `null` / `b64:<UTF-8 base64>`, a bool 1/0, a list `[a;b]`, a file tree `tree:<path>:<content>,...`
// made fresh in a temp folder; a path written relative to that folder as `<root>/...`).
use super::super::contraption::{self as ct, ContraptionFolderSource, ContraptionMetaData};
use super::super::mods::{self, LuaModInfo, LuaModLoader, Mod, ModMetaData, ModSourceKind};

fn m_b64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("not base64: {s}")),
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}
fn m_b64e(b: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for ch in b.chunks(3) {
        let n = ch.iter().enumerate().fold(0u32, |a, (i, &x)| a | (x as u32) << (16 - 8 * i));
        for i in 0..4 {
            s.push(if i <= ch.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}
fn m_str(a: &str) -> Result<Option<String>, String> {
    if a == "null" {
        return Ok(None);
    }
    let b = a.strip_prefix("b64:").ok_or_else(|| format!("not a string: {a}"))?;
    String::from_utf8(m_b64(b)?).map(Some).map_err(|e| e.to_string())
}
fn m_s(s: Option<&str>) -> String {
    s.map_or_else(|| "null".into(), |s| format!("b64:{}", m_b64e(s.as_bytes())))
}
fn m_b(b: bool) -> String {
    (if b { "1" } else { "0" }).into()
}
fn m_l(xs: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", xs.into_iter().collect::<Vec<_>>().join(";"))
}
fn m_sl(xs: Option<&[String]>) -> String {
    xs.map_or_else(|| "null".into(), |xs| m_l(xs.iter().map(|x| m_s(Some(x)))))
}
fn m_list(a: &str) -> Result<Option<Vec<String>>, String> {
    if a == "null" {
        return Ok(None);
    }
    let a = a.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or_else(|| format!("not a list: {a}"))?;
    split_args(a).into_iter().map(|x| m_str(x).map(Option::unwrap_or_default)).collect::<Result<Vec<_>, _>>().map(Some)
}
fn m_arg<'a>(c: &Case<'a>, i: usize) -> Result<&'a str, String> {
    c.args.get(i).copied().ok_or_else(|| format!("no arg {i}"))
}

/// A tree row's folder: made fresh, removed when dropped.
struct MTree(std::path::PathBuf);
impl Drop for MTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn m_tree(a: &str) -> Result<MTree, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!("ppg-fnmods-rs-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let t = MTree(root);
    let body = a.strip_prefix("tree:").ok_or_else(|| format!("not a tree: {a}"))?;
    for e in body.split(',').filter(|e| !e.is_empty()) {
        let (p, d) = e.split_once(':').ok_or("bad tree entry")?;
        let full = t.0.join(String::from_utf8(m_b64(p)?).map_err(|e| e.to_string())?);
        if d == "/" {
            std::fs::create_dir_all(&full).map_err(|e| e.to_string())?;
        } else {
            std::fs::create_dir_all(full.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&full, m_b64(d)?).map_err(|e| e.to_string())?;
        }
    }
    Ok(t)
}
/// A path as the game's FullName relative to the tree: `<root>/...`.
fn m_rel(t: &MTree, p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let r = t.0.to_string_lossy().replace('\\', "/");
    m_s(Some(&match s.strip_prefix(&r) { Some(rest) => format!("<root>{rest}"), None => s }))
}
fn m_unrel(t: &MTree, a: &str) -> Result<Option<std::path::PathBuf>, String> {
    Ok(m_str(a)?.map(|s| match s.strip_prefix("<root>") { Some(rest) => t.0.join(rest.trim_start_matches('/')), None => s.into() }))
}
fn m_io<T>(r: std::io::Result<T>, f: impl FnOnce(T) -> String) -> String {
    match r {
        Ok(v) => f(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "exc:FileNotFoundException".into(),
        Err(_) => "exc:IOException".into(),
    }
}

/// `Mod(name,identifier,authors,summary,version,gameVersion,tags)`
fn m_info(a: &str) -> Result<LuaModInfo, String> {
    let body = a.strip_prefix("Mod(").and_then(|a| a.strip_suffix(')')).ok_or_else(|| format!("not a Mod: {a}"))?;
    let p = m_parts(body);
    if p.len() != 7 {
        return Err(format!("Mod has {} parts", p.len()));
    }
    let mut i = LuaModInfo::new();
    i.Name = m_str(p[0])?;
    i.Identifier = m_str(p[1])?;
    i.Authors = m_list(p[2])?;
    i.Summary = m_str(p[3])?;
    i.Version = m_str(p[4])?;
    i.GameVersion = m_str(p[5])?;
    i.Tags = m_list(p[6])?;
    Ok(i)
}
/// Comma parts outside brackets.
fn m_parts(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut at) = (Vec::new(), 0i32, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[at..i]);
                at = i + 1
            }
            _ => {}
        }
    }
    out.push(&s[at..]);
    out
}
fn m_info_text(i: &LuaModInfo) -> String {
    [m_s(i.Name.as_deref()), m_s(i.Identifier.as_deref()), m_sl(i.Authors.as_deref()), m_s(i.Summary.as_deref()), m_s(i.Version.as_deref()), m_s(i.GameVersion.as_deref()), m_sl(i.Tags.as_deref())].join(",")
}
fn m_info_full(i: &LuaModInfo) -> String {
    format!("{},{},{},{}", m_info_text(i), m_sl(i.Dependencies.as_deref()), m_sl(i.LoadBefore.as_deref()), m_sl(i.LoadAfter.as_deref()))
}
fn m_mod(c: &Case) -> Result<Mod, String> {
    Ok(Mod::new(m_info(m_arg(c, 0)?)?, ModSourceKind::Folder, Default::default()))
}
fn m_loader_void(c: &Case, f: fn(&mut LuaModLoader, &Mod)) -> Result<String, String> {
    let m = m_mod(c)?;
    let mut l = LuaModLoader;
    f(&mut l, &m);
    Ok(format!("void {},{}", m_l(l.GetMods().iter().map(|_| "m".to_string())), m_b(l.IsEnabled(&m))))
}
/// A LuaModInfo property's setter: the whole object after.
fn m_info_set(c: &Case, f: fn(&mut LuaModInfo, &str) -> Result<(), String>) -> Result<String, String> {
    let mut i = LuaModInfo::new();
    f(&mut i, m_arg(c, 0)?)?;
    Ok(m_info_full(&i))
}
fn m_modules(a: &str) -> Result<Option<Vec<(Option<String>, Option<String>)>>, String> {
    if a == "null" {
        return Ok(None);
    }
    let a = a.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or("not a list")?;
    split_args(a).into_iter().map(|x| { let p = m_parts(x); Ok((m_str(p[0])?, m_str(p.get(1).ok_or("pair")?)?)) }).collect::<Result<Vec<_>, String>>().map(Some)
}
fn m_modules_text(m: &Option<Vec<(Option<String>, Option<String>)>>) -> String {
    m.as_ref().map_or_else(|| "null".into(), |m| m_l(m.iter().map(|(a, b)| format!("{},{}", m_s(a.as_deref()), m_s(b.as_deref())))))
}

/// `name,author,tags,creator,thumbnail,metaLocation`
fn m_meta(a: &str) -> Result<ModMetaData, String> {
    let p = m_parts(a);
    let mut m = ModMetaData::new();
    m.Name = m_str(p[0])?;
    m.Author = m_str(p[1])?;
    m.Tags = m_strs(p[2])?;
    m.CreatorUGCIdentity = m_str(p[3])?;
    Ok(m)
}

/// A ContraptionMetaData row (MetaRow): fields, RequiredMods, the Tags count, the four paths and the directory.
fn m_cmeta(t: &MTree, a: &str) -> Result<ContraptionMetaData, String> {
    let p = m_parts(a);
    if p.len() != 12 {
        return Err(format!("metadata has {} parts", p.len()));
    }
    let mut m = ContraptionMetaData::empty();
    m.Name = m_str(p[0])?;
    m.DisplayName = m_str(p[1])?;
    m.Version = m_str(p[2])?;
    m.PublishedFileID = p[3].parse().map_err(|_| "PublishedFileID")?;
    m.CreatorUGCIdentity = m_str(p[4])?;
    m.RequiredMods = if p[5] == "null" {
        None
    } else {
        let l = p[5].strip_prefix('[').and_then(|x| x.strip_suffix(']')).ok_or("RequiredMods")?;
        Some(split_args(l).into_iter().map(|r| {
            let f: Vec<&str> = r.split('|').collect();
            Ok(ct::RequiredMod { Name: m_str(f[0])?, Identifier: m_str(f[1])?, WorkshopId: m_str(f[2])? })
        }).collect::<Result<Vec<_>, String>>()?)
    };
    m.PathToMetadata = m_unrel(t, p[7])?;
    m.PathToDataFile = m_unrel(t, p[8])?;
    m.PathToThumbnail = m_unrel(t, p[9])?;
    m.PathToOutlineFile = m_unrel(t, p[10])?;
    m.Directory = m_unrel(t, p[11])?;
    Ok(m)
}
fn m_cmeta_text(t: &MTree, m: &ContraptionMetaData) -> String {
    let rel = |p: &Option<std::path::PathBuf>| p.as_ref().map_or_else(|| "null".into(), |p| m_rel(t, p));
    let req = m.RequiredMods.as_ref().map_or_else(|| "null".into(), |r| m_l(r.iter().map(|x| format!("{}|{}|{}", m_s(x.Name.as_deref()), m_s(x.Identifier.as_deref()), m_s(x.WorkshopId.as_deref())))));
    [m_s(m.Name.as_deref()), m_s(m.DisplayName.as_deref()), m_s(m.Version.as_deref()), m.PublishedFileID.to_string(), m_s(m.CreatorUGCIdentity.as_deref()), req, m.Tags.len().to_string(),
        rel(&m.PathToMetadata), rel(&m.PathToDataFile), rel(&m.PathToThumbnail), rel(&m.PathToOutlineFile), rel(&m.Directory)].join(",")
}
fn m_contraptions(c: &Case, all: bool) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let src = ContraptionFolderSource::new(&t.0.join("Contraptions"));
    let list = if all { ct::get_all_contraptions(&[src]) } else { src.GetContraptions() };
    Ok(m_l(list.iter().map(|m| m_cmeta_text(&t, m))))
}
fn m_cmeta_call(c: &Case, f: fn(&MTree, &ContraptionMetaData) -> String) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let m = m_cmeta(&t, m_arg(c, 1)?)?;
    Ok(f(&t, &m))
}
fn m_get_path(c: &Case, f: fn(&std::path::Path, Option<&str>) -> Result<std::path::PathBuf, &'static str>) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let name = m_str(m_arg(c, 1)?)?;
    Ok(match f(&t.0.join("Contraptions"), name.as_deref()) { Ok(p) => m_rel(&t, &p), Err(e) => format!("exc:{e}") })
}

/// TypeText: `E` + the element's, else s/f/i/b (string, float, int, bool), p (another primitive), v (a value type), c (a class).
fn m_type(s: &str) -> Result<super::super::utils::ManagedType, String> {
    use super::super::utils::ManagedType as T;
    Ok(match s.as_bytes().first() {
        Some(b'E') => T::Element(Box::new(m_type(&s[1..])?)),
        Some(b's') => T::String,
        Some(b'f') => T::Float,
        Some(b'i') => T::Int,
        Some(b'b') => T::Bool,
        Some(b'p') => T::Primitive,
        Some(b'v') => T::ValueType,
        Some(b'c') => T::Class,
        _ => return Err(format!("bad type {s}")),
    })
}

/// ModResourceCache ops (`C,key,type` / `G,key,type` / `R,key,type` / `X`) on a fresh cache: each op's result.
fn m_cache(c: &Case) -> Result<String, String> {
    use mods::mod_resource_cache as rc;
    #[derive(Clone)]
    struct Texture2D;
    #[derive(Clone)]
    struct Material;
    #[derive(Clone)]
    struct Sprite;
    let ops = m_arg(c, 0)?.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or("ops")?;
    rc::Clean();
    let mut out = Vec::new();
    for op in split_args(ops) {
        let p: Vec<&str> = op.split(',').collect();
        let key = || m_str(p[1]).map(Option::unwrap_or_default);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String, String> {
            Ok(match (p[0], p.get(2).copied()) {
                ("C", Some(ty)) => {
                    let k = key()?;
                    let r = match ty { "0" => rc::Cache(&k, Texture2D), "1" => rc::Cache(&k, Material), _ => rc::Cache(&k, Sprite) };
                    match r { Ok(()) => "ok".into(), Err(e) => format!("exc:{}", e.split(':').next().unwrap_or("")) }
                }
                ("G", Some(ty)) => {
                    let k = key()?;
                    let (ok, n) = match ty {
                        "0" => (rc::TryGet::<Texture2D>(&k).is_some(), "Texture2D"),
                        "1" => (rc::TryGet::<Material>(&k).is_some(), "Material"),
                        _ => (rc::TryGet::<Sprite>(&k).is_some(), "Sprite"),
                    };
                    format!("{},{}", m_b(ok), if ok { n } else { "null" })
                }
                ("R", Some(ty)) => {
                    let id = match ty { "0" => std::any::TypeId::of::<Texture2D>(), "1" => std::any::TypeId::of::<Material>(), _ => std::any::TypeId::of::<Sprite>() };
                    rc::Remove(&key()?, id);
                    "ok".into()
                }
                _ => {
                    rc::Clean();
                    "ok".into()
                }
            })
        }));
        out.push(match r {
            Ok(r) => r?,
            Err(e) => {
                let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                format!("exc:{}", msg.split(':').next().unwrap_or(""))
            }
        });
    }
    Ok(m_l(out))
}

const PURE_MODS: &[(&str, PureCall)] = &[
    // DllCheck mods rows (b64 strings); an in-game FnProbe row (the recorder's encoding) goes to the generated twin.
    ("HappyTypeBinder.GetTypeFromBase", |c| {
        let a = m_arg(c, 0)?;
        if a != "null" && !a.starts_with("b64:") {
            return (PURE_GEN.iter().find(|(n, _)| *n == "HappyTypeBinder.GetTypeFromBase").ok_or("no generated twin")?.1)(c);
        }
        Ok(m_s(crate::game::ceras::happy_type_binder_get_type_from_base(m_str(a)?.as_deref()).as_deref()))
    }),
    ("EmptyModLoader.GetMods", |_| Ok(m_l(LuaModLoader.GetMods().iter().map(|m| m_s(m.Info.Identifier.as_deref()))))),
    ("EmptyModLoader.TryLoadAsync", |c| { let d = m_str(m_arg(c, 0)?)?.unwrap_or_default(); Ok(format!("1,{}", m_b(LuaModLoader.TryLoadAsync(std::path::Path::new(&d))))) }),
    ("EmptyModLoader.TryGetModById", |c| { let id = m_str(m_arg(c, 0)?)?.unwrap_or_default(); let m = LuaModLoader.TryGetModById(&id); Ok(format!("{},{}", m_b(m.is_some()), if m.is_some() { "mod" } else { "null" })) }),
    ("EmptyModLoader.GetThumbnail", |c| Ok(if LuaModLoader.GetThumbnail(&m_mod(c)?).is_some() { "sprite" } else { "null" }.into())),
    ("EmptyModLoader.IsEnabled", |c| Ok(m_b(LuaModLoader.IsEnabled(&m_mod(c)?)))),
    ("EmptyModLoader.Reload", |c| m_loader_void(c, |l, m| l.Reload(m))),
    ("EmptyModLoader.Unload", |c| m_loader_void(c, |l, m| l.Unload(m))),
    ("EmptyModLoader.Enable", |c| m_loader_void(c, |l, m| l.Enable(m))),
    ("EmptyModLoader.Disable", |c| m_loader_void(c, |l, m| l.Disable(m))),
    ("EmptyModLoader.OnLoadMap", |c| m_loader_void(c, |l, _| l.OnLoadMap())),
    ("EmptyModLoader.OnUpdate", |c| m_loader_void(c, |l, _| super::super::background_loader::ModLoader::on_update(l))),
    ("EmptyModLoader.Dispose", |c| m_loader_void(c, |l, _| super::super::background_loader::ModLoader::unload(l))),
    ("EmptyModLoader.OnCreateModEntryUi", |c| m_loader_void(c, |l, m| l.OnCreateModEntryUi(m))),
    ("Mod.constructor", |c| { let m = m_mod(c)?; Ok(format!("{},{},{}", m_sl(Some(&m.Errors)), m_s(m.Status.as_deref()), if m.File.as_os_str().is_empty() { "null" } else { "file" })) }),
    ("Mod.get_HasErrors", |c| { let mut m = m_mod(c)?; m.Errors = m_list(m_arg(c, 1)?)?.unwrap_or_default(); Ok(m_b(m.HasErrors())) }),
    ("Mod.get_IsEnabled", |c| Ok(m_b(m_mod(c)?.IsEnabled(&LuaModLoader)))),
    ("ModExtensions.GetWorkshopId", |c| Ok(m_s(m_mod(c)?.GetWorkshopId().as_deref()))),
    ("ModExtensions.IsFromWorkshop", |c| Ok(m_b(m_mod(c)?.IsFromWorkshop()))),
    ("ModLoaderRegistry.TryGetModById", |c| { let id = m_str(m_arg(c, 0)?)?; let m = mods::registry_try_get_mod_by_id(id.as_deref()); Ok(format!("{},{}", m_b(m.is_some()), if m.is_some() { "mod" } else { "null" })) }),
    ("ModLoaderRegistry.get_Mods", |_| Ok(m_l(mods::registry_mods().iter().map(|m| m_s(m.Info.Identifier.as_deref()))))),
    ("LuaModInfo.constructor", |_| { let i = LuaModInfo::new(); Ok(format!("{},{}", m_info_full(&i), m_modules_text(&i.Modules))) }),
    ("LuaModInfo.set_Name", |c| m_info_set(c, |i, a| { i.Name = m_str(a)?; Ok(()) })),
    ("LuaModInfo.set_Identifier", |c| m_info_set(c, |i, a| { i.Identifier = m_str(a)?; Ok(()) })),
    ("LuaModInfo.set_Summary", |c| m_info_set(c, |i, a| { i.Summary = m_str(a)?; Ok(()) })),
    ("LuaModInfo.set_Version", |c| m_info_set(c, |i, a| { i.Version = m_str(a)?; Ok(()) })),
    ("LuaModInfo.set_GameVersion", |c| m_info_set(c, |i, a| { i.GameVersion = m_str(a)?; Ok(()) })),
    ("LuaModInfo.set_Authors", |c| m_info_set(c, |i, a| { i.Authors = m_list(a)?; Ok(()) })),
    ("LuaModInfo.set_Tags", |c| m_info_set(c, |i, a| { i.Tags = m_list(a)?; Ok(()) })),
    ("LuaModInfo.set_Dependencies", |c| m_info_set(c, |i, a| { i.Dependencies = m_list(a)?; Ok(()) })),
    ("LuaModInfo.set_LoadBefore", |c| m_info_set(c, |i, a| { i.LoadBefore = m_list(a)?; Ok(()) })),
    ("LuaModInfo.set_LoadAfter", |c| m_info_set(c, |i, a| { i.LoadAfter = m_list(a)?; Ok(()) })),
    ("LuaModInfo.get_Name", |c| { let mut i = LuaModInfo::new(); i.Name = m_str(m_arg(c, 0)?)?; Ok(m_s(i.Name.as_deref())) }),
    ("LuaModInfo.get_Identifier", |c| { let mut i = LuaModInfo::new(); i.Identifier = m_str(m_arg(c, 0)?)?; Ok(m_s(i.Identifier.as_deref())) }),
    ("LuaModInfo.get_Summary", |c| { let mut i = LuaModInfo::new(); i.Summary = m_str(m_arg(c, 0)?)?; Ok(m_s(i.Summary.as_deref())) }),
    ("LuaModInfo.get_Version", |c| { let mut i = LuaModInfo::new(); i.Version = m_str(m_arg(c, 0)?)?; Ok(m_s(i.Version.as_deref())) }),
    ("LuaModInfo.get_GameVersion", |c| { let mut i = LuaModInfo::new(); i.GameVersion = m_str(m_arg(c, 0)?)?; Ok(m_s(i.GameVersion.as_deref())) }),
    ("LuaModInfo.get_Authors", |c| { let mut i = LuaModInfo::new(); i.Authors = m_list(m_arg(c, 0)?)?; Ok(m_sl(i.Authors.as_deref())) }),
    ("LuaModInfo.get_Tags", |c| { let mut i = LuaModInfo::new(); i.Tags = m_list(m_arg(c, 0)?)?; Ok(m_sl(i.Tags.as_deref())) }),
    ("LuaModInfo.get_Dependencies", |c| { let mut i = LuaModInfo::new(); i.Dependencies = m_list(m_arg(c, 0)?)?; Ok(m_sl(i.Dependencies.as_deref())) }),
    ("LuaModInfo.get_LoadBefore", |c| { let mut i = LuaModInfo::new(); i.LoadBefore = m_list(m_arg(c, 0)?)?; Ok(m_sl(i.LoadBefore.as_deref())) }),
    ("LuaModInfo.get_LoadAfter", |c| { let mut i = LuaModInfo::new(); i.LoadAfter = m_list(m_arg(c, 0)?)?; Ok(m_sl(i.LoadAfter.as_deref())) }),
    ("LuaModInfo.set_Modules", |c| { let mut i = LuaModInfo::new(); i.Modules = m_modules(m_arg(c, 0)?)?; Ok(m_modules_text(&i.Modules)) }),
    ("LuaModInfo.get_Modules", |c| { let mut i = LuaModInfo::new(); i.Modules = m_modules(m_arg(c, 0)?)?; Ok(m_modules_text(&i.Modules)) }),
    ("ModMetaData.constructor", |_| {
        let m = ModMetaData::new();
        Ok([m_s(m.Name.as_deref()), m_s(m.Author.as_deref()), m_s(m.Description.as_deref()), m_s(m.ModVersion.as_deref()), m_s(m.GameVersion.as_deref()), m_s(m.ThumbnailPath.as_deref()), m_s(m.EntryPoint.as_deref()), m_strs_text(&m.Tags), m_strs_text(&m.Scripts), m_b(m.Active), m_b(m.Suspicious), m_b(m.NeedsUpdateApproval)].join(","))
    }),
    ("ModMetaData.GetUniqueName", |c| Ok(m_meta(m_arg(c, 0)?)?.GetUniqueName().map_or_else(|e| format!("exc:{e}"), |n| m_s(Some(&n))))),
    ("ModMetaData.GetTagsForSteam", |c| Ok(m_sl(Some(&m_meta(m_arg(c, 0)?)?.GetTagsForSteam())))),
    ("ModMetaData.set_Suspicious", |c| { let mut m = ModMetaData::new(); m.Suspicious = m_arg(c, 0)? == "1"; m.NeedsUpdateApproval = !m.Suspicious; Ok(format!("{},{}", m_b(m.Suspicious), m_b(m.NeedsUpdateApproval))) }),
    ("ModMetaData.get_Suspicious", |c| { let mut m = ModMetaData::new(); m.Suspicious = m_arg(c, 0)? == "1"; Ok(m_b(m.Suspicious)) }),
    ("ModMetaData.set_NeedsUpdateApproval", |c| { let mut m = ModMetaData::new(); m.NeedsUpdateApproval = m_arg(c, 0)? == "1"; m.Suspicious = !m.NeedsUpdateApproval; Ok(format!("{},{}", m_b(m.Suspicious), m_b(m.NeedsUpdateApproval))) }),
    ("ModMetaData.get_NeedsUpdateApproval", |c| { let mut m = ModMetaData::new(); m.NeedsUpdateApproval = m_arg(c, 0)? == "1"; Ok(m_b(m.NeedsUpdateApproval)) }),
    ("ModMetaData.HasDoNotReuploadSign", |c| {
        let t = m_tree(m_arg(c, 0)?)?;
        let mut m = ModMetaData::new();
        m.ThumbnailPath = m_str(m_arg(c, 1)?)?;
        m.MetaLocation = Some(t.0.to_string_lossy().to_string());
        Ok(m.HasDoNotReuploadSign().map_or_else(|e| format!("exc:{e}"), m_b))
    }),
    ("ModValueAttribute.constructor", |c| { let v = if m_arg(c, 0)? == "default" { mods::ModValueAttribute::default() } else { mods::ModValueAttribute::new(m_str(m_arg(c, 0)?)?.as_deref(), m_arg(c, 1)? == "1", m_arg(c, 2)? == "1") }; Ok(format!("{},{},{}", m_s(v.Name.as_deref()), m_b(v.Read), m_b(v.Write))) }),
    ("ModValueAttribute.get_Name", |c| Ok(m_s(mods::ModValueAttribute::new(m_str(m_arg(c, 0)?)?.as_deref(), m_arg(c, 1)? == "1", m_arg(c, 2)? == "1").Name.as_deref()))),
    ("ModValueAttribute.get_Read", |c| Ok(m_b(mods::ModValueAttribute::new(m_str(m_arg(c, 0)?)?.as_deref(), m_arg(c, 1)? == "1", m_arg(c, 2)? == "1").Read))),
    ("ModValueAttribute.get_Write", |c| Ok(m_b(mods::ModValueAttribute::new(m_str(m_arg(c, 0)?)?.as_deref(), m_arg(c, 1)? == "1", m_arg(c, 2)? == "1").Write))),
    ("ModFunctionAttribute.constructor", |c| Ok(if m_arg(c, 0)? == "default" { m_s(mods::ModFunctionAttribute::default().Name.as_deref()) } else { m_s(mods::ModFunctionAttribute::new(m_str(m_arg(c, 0)?)?.as_deref()).Name.as_deref()) })),
    ("ModFunctionAttribute.get_Name", |c| Ok(m_s(mods::ModFunctionAttribute::new(m_str(m_arg(c, 0)?)?.as_deref()).Name.as_deref()))),
    ("ModInfoUtilities.TryGetModIdFromPath", |c| {
        let t = m_tree(m_arg(c, 0)?)?;
        let p = t.0.join(m_str(m_arg(c, 1)?)?.unwrap_or_default());
        Ok(m_io(mods::TryGetModIdFromPath(&p), |(ok, id)| format!("{},{}", m_b(ok), m_s(id.as_deref()))))
    }),
    ("ModHasher.Compute", |c| { let t = m_tree(m_arg(c, 0)?)?; Ok(m_io(mods::mod_hasher_compute(&t.0), |h| h.to_string())) }),
    ("ModHasher.Hash", |c| {
        let t = m_tree(m_arg(c, 0)?)?;
        let mut h: u32 = m_arg(c, 1)?.parse().map_err(|_| "hash")?;
        let p = t.0.join(m_str(m_arg(c, 2)?)?.unwrap_or_default());
        Ok(m_io(mods::mod_hasher_hash(&mut h, &mut [0u8; 1024], &p), |_| h.to_string()))
    }),
    ("ModFolderSource.GetFolders", |c| {
        let t = m_tree(m_arg(c, 0)?)?;
        let mut s = mods::ModFolderSource::new(t.0.join("Mods")).map_err(|e| e.to_string())?;
        s.Initialise();
        s.OnUpdate();
        let r = format!("{},{},{}", m_l(s.GetFolders().iter().map(|f| m_rel(&t, f))), m_rel(&t, &s.Folder), m_b(t.0.join("Mods").is_dir()));
        s.Dispose();
        Ok(r)
    }),
    ("ContraptionFolderSource.GetContraptions", |c| m_contraptions(c, false)),
    ("ContraptionSerialiser.GetAllContraptions", |c| m_contraptions(c, true)),
    ("ContraptionFolderSource.get_Folder", |c| { let t = m_tree(m_arg(c, 0)?)?; Ok(m_rel(&t, &ContraptionFolderSource::new(&t.0.join("Contraptions")).Folder)) }),
    ("ContraptionFolderSource.constructor", |c| {
        let t = m_tree(m_arg(c, 0)?)?;
        let s = ContraptionFolderSource::new(&t.0.join("New").join("Folder"));
        Ok(format!("{},{}", m_rel(&t, &s.Folder), m_b(t.0.join("New").join("Folder").is_dir())))
    }),
    ("ContraptionMetaData.get_IsCurrentVersion", |c| m_cmeta_call(c, |_, m| m_b(m.IsCurrentVersion()))),
    ("ContraptionMetaData.get_IsWorkshopItem", |c| m_cmeta_call(c, |_, m| m_b(m.IsWorkshopItem()))),
    ("ContraptionMetaData.get_RequiresMods", |c| m_cmeta_call(c, |_, m| m_b(m.RequiresMods()))),
    ("ContraptionMetaData.GetSizeOnDisk", |c| m_cmeta_call(c, |_, m| m.GetSizeOnDisk().to_string())),
    ("ContraptionMetaData.IsLegacyStructure", |c| m_cmeta_call(c, |t, m| m_b(m.IsLegacyStructure(&t.0.join("Contraptions"))))),
    ("ContraptionMetaData.GetPath", |c| m_get_path(c, ContraptionMetaData::GetPath)),
    ("Contraption.GetPath", |c| m_get_path(c, ct::contraption_get_path)),
    ("ObjectState.IsSerialisableComponent", |c| Ok(m_b(super::super::Game::is_component_to_serialise(&m_str(m_arg(c, 0)?)?.unwrap_or_default())))),
    ("ObjectState.IsComponentToSerialise", |c| Ok(m_b(super::super::Game::is_component_to_serialise(&m_str(m_arg(c, 0)?)?.unwrap_or_default())))),
    ("ReflectionCache.IsComponentToSerialise", |c| Ok(m_b(super::super::Game::is_component_to_serialise(&m_str(m_arg(c, 0)?)?.unwrap_or_default())))),
    ("ReflectionCache.IsSerialisableType", |c| Ok(m_b(super::super::utils::is_serialisable_type(&m_type(&m_str(m_arg(c, 0)?)?.unwrap_or_default())?)))),
    ("Utils.IsSerialisableType", |c| Ok(m_b(super::super::utils::is_serialisable_type(&m_type(&m_str(m_arg(c, 0)?)?.unwrap_or_default())?)))),
    ("ModResourceCache.Cache", m_cache),
    ("ModResourceCache.TryGet", m_cache),
    ("ModResourceCache.Remove", m_cache),
    ("ModResourceCache.Clean", m_cache),
];

// ---- the legacy C# mod path (tools/dllcheck/LegacyCheck.cs) ----
use super::super::legacy_mods as lg;

fn m_exc(e: lg::Exc) -> String {
    format!("exc:{}", e.class)
}
/// A list whose items may be null.
fn m_strs(a: &str) -> Result<Option<Vec<Option<String>>>, String> {
    if a == "null" {
        return Ok(None);
    }
    let a = a.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or_else(|| format!("not a list: {a}"))?;
    split_args(a).into_iter().map(m_str).collect::<Result<Vec<_>, _>>().map(Some)
}
fn m_strs_text(xs: &Option<Vec<Option<String>>>) -> String {
    xs.as_ref().map_or_else(|| "null".into(), |xs| m_l(xs.iter().map(|x| m_s(x.as_deref()))))
}
/// PathText: a path under the tree as `<root>/...`.
fn m_path(t: &MTree, p: Option<&str>) -> String {
    p.map_or_else(|| "null".into(), |p| m_rel(t, std::path::Path::new(p)))
}
fn m_root(t: &MTree) -> String {
    t.0.to_string_lossy().into_owned()
}
/// MetaArgs: name,author,creator,assembly,scripts,thumb,hasErrors,workshop,metaLocation,contentHash
fn m_largs(t: &MTree, a: &str) -> Result<ModMetaData, String> {
    let p = m_parts(a);
    if p.len() != 10 {
        return Err(format!("meta args have {} parts", p.len()));
    }
    let mut m = ModMetaData::new();
    m.Name = m_str(p[0])?;
    m.Author = m_str(p[1])?;
    m.CreatorUGCIdentity = m_str(p[2])?;
    m.AssemblyName = m_str(p[3])?;
    m.Scripts = m_strs(p[4])?;
    m.ThumbnailPath = m_str(p[5])?;
    m.HasErrors = p[6] == "1";
    m.FromWorkshop = p[7] == "1";
    m.MetaLocation = m_str(p[8])?.map(|_| m_root(t));
    m.ContentHash = p[9].parse().map_err(|_| "ContentHash")?;
    Ok(m)
}
fn m_unrel_s(t: &MTree, a: &str) -> Result<Option<String>, String> {
    Ok(m_unrel(t, a)?.map(|p| p.to_string_lossy().into_owned()))
}
/// FullMetaText (22 fields).
fn m_full(t: &MTree, a: &str) -> Result<ModMetaData, String> {
    let p = m_parts(a);
    if p.len() != 22 {
        return Err(format!("meta has {} parts", p.len()));
    }
    let mut m = ModMetaData::new();
    m.Name = m_str(p[0])?;
    m.Author = m_str(p[1])?;
    m.Description = m_str(p[2])?;
    m.RichReadme = m_str(p[3])?;
    m.ModVersion = m_str(p[4])?;
    m.GameVersion = m_str(p[5])?;
    m.ThumbnailPath = m_str(p[6])?;
    m.EntryPoint = m_str(p[7])?;
    m.Tags = m_strs(p[8])?;
    m.Scripts = m_strs(p[9])?;
    m.Active = p[10] == "1";
    m.MetaLocation = m_unrel_s(t, p[11])?;
    m.UGCIdentity = m_str(p[12])?;
    m.CreatorUGCIdentity = m_str(p[13])?;
    m.AssemblyName = m_str(p[14])?;
    m.FromWorkshop = p[15] == "1";
    m.MetaPath = m_unrel_s(t, p[16])?;
    m.HasErrors = p[17] == "1";
    m.Errors = m_str(p[18])?;
    m.ContentHash = p[19].parse().map_err(|_| "ContentHash")?;
    m.Suspicious = p[20] == "1";
    m.NeedsUpdateApproval = p[21] == "1";
    Ok(m)
}
fn m_full_text(t: &MTree, m: &ModMetaData) -> String {
    [
        m_s(m.Name.as_deref()), m_s(m.Author.as_deref()), m_s(m.Description.as_deref()), m_s(m.RichReadme.as_deref()), m_s(m.ModVersion.as_deref()), m_s(m.GameVersion.as_deref()),
        m_s(m.ThumbnailPath.as_deref()), m_s(m.EntryPoint.as_deref()), m_strs_text(&m.Tags), m_strs_text(&m.Scripts), m_b(m.Active), m_path(t, m.MetaLocation.as_deref()),
        m_s(m.UGCIdentity.as_deref()), m_s(m.CreatorUGCIdentity.as_deref()), m_s(m.AssemblyName.as_deref()), m_b(m.FromWorkshop), m_path(t, m.MetaPath.as_deref()), m_b(m.HasErrors),
        m_s(m.Errors.as_deref()), m.ContentHash.to_string(), m_b(m.Suspicious), m_b(m.NeedsUpdateApproval),
    ]
    .join(",")
}
/// TreeText: `now:` and every folder (`tree-dir:<path>`) and file (`<path>:<base64>`) under the tree, sorted.
fn m_now(t: &MTree) -> String {
    fn walk(d: &std::path::Path, t: &MTree, items: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                items.push(format!("tree-dir:{}", m_rel(t, &p)));
                walk(&p, t, items);
            } else {
                items.push(format!("{}:{}", m_rel(t, &p), m_b64e(&std::fs::read(&p).unwrap_or_default())));
            }
        }
    }
    let mut items = Vec::new();
    walk(&t.0, t, &mut items);
    items.sort();
    format!("now:{}", items.join("|"))
}
/// A `now:` tree made fresh.
fn m_now_tree(a: &str) -> Result<MTree, String> {
    let body = a.strip_prefix("now:").ok_or_else(|| format!("not a now tree: {a}"))?;
    let t = m_tree("tree:")?;
    for e in body.split('|').filter(|e| !e.is_empty()) {
        if let Some(d) = e.strip_prefix("tree-dir:") {
            std::fs::create_dir_all(m_unrel(&t, d)?.ok_or("dir")?).map_err(|e| e.to_string())?;
        } else {
            let (p, d) = e.rsplit_once(':').ok_or("bad now entry")?;
            let full = m_unrel(&t, p)?.ok_or("file")?;
            std::fs::create_dir_all(full.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&full, m_b64(d)?).map_err(|e| e.to_string())?;
        }
    }
    Ok(t)
}
fn m_bytes8(b: Option<&[u8]>) -> String {
    b.map_or_else(|| "null".into(), |b| m_l(b.iter().map(|x| x.to_string())))
}
/// ExcStr: an exception's text as its class (`excstr:<class>`).
fn m_excstr(s: Option<&str>) -> String {
    match s {
        None => "null".into(),
        Some(s) if s.starts_with("System.") || s.contains("Exception:") => format!("excstr:{}", s.split(':').next().unwrap_or("").rsplit('.').next().unwrap_or("")),
        Some(s) => m_s(Some(s)),
    }
}
fn m_paths(t: &MTree, r: Result<Vec<String>, lg::Exc>) -> String {
    match r {
        Ok(v) => m_l(v.iter().map(|p| m_path(t, Some(p)))),
        Err(e) => m_exc(e),
    }
}
fn m_lpath(c: &Case, f: fn(&std::path::Path, &ModMetaData) -> Result<String, lg::Exc>) -> Result<String, String> {
    let t = m_tree("tree:")?;
    let m = m_largs(&t, m_arg(c, 0)?)?;
    Ok(f(&t.0, &m).map_or_else(m_exc, |p| m_path(&t, Some(&p))))
}
fn m_compile(c: &Case) -> Result<String, String> {
    let t = m_now_tree(m_arg(c, 0)?)?;
    let m = std::sync::Arc::new(std::sync::Mutex::new(m_largs(&t, m_arg(c, 1)?)?));
    Ok(match lg::Compile(&t.0, &m, true) {
        Err(e) => m_exc(e),
        Ok((r, s)) => format!(
            "{},{},{},{},{},{},{};{}",
            m_b(r.Success),
            m_excstr(r.Errors.as_deref()),
            m_b(r.Suspicious),
            m_bytes8(s.AssemblyData.as_deref()),
            s.AbsoluteScriptPaths.as_ref().map_or_else(|| "null".into(), |v| m_l(v.iter().map(|p| m_path(&t, Some(p))))),
            m_path(&t, s.RelativePath.as_deref()),
            m_b(m.lock().unwrap().HasErrors),
            m_now(&t)
        ),
    })
}
fn m_load_mod_at(c: &Case) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let p = t.0.join(m_str(m_arg(c, 1)?)?.unwrap_or_default()).to_string_lossy().into_owned();
    let mut l = lg::ModLoader::default();
    let m = l.LoadModAt(&p);
    Ok(format!("{};{}", m.map_or_else(|| "null".into(), |m| m_full_text(&t, &m.lock().unwrap())), l.LoadedMods.len()))
}
fn m_update_json(c: &Case) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let m = m_full(&t, m_arg(c, 1)?)?;
    Ok(lg::UpdateJSON(&m).map_or_else(m_exc, |_| m_now(&t)))
}
fn m_thumbnail(c: &Case) -> Result<String, String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let m = std::sync::Arc::new(std::sync::Mutex::new(m_largs(&t, m_arg(c, 1)?)?));
    Ok(if lg::ModLoader::default().GetThumbnail(&m).is_some() { "sprite" } else { "null" }.into())
}
/// The static lists' op sequence on three metadata (`L` `S` `R` `A` `M` `+` `-` `I` `a` `O` `U` `P` `V`): each op's result,
/// then LoadedMods and ModScripts' keys.
fn m_lists(c: &Case) -> Result<String, String> {
    let ops = m_arg(c, 0)?.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or("ops")?;
    let pool: Vec<lg::ModRef> = ["a", "b", "c"].iter().map(|n| { let mut m = ModMetaData::new(); m.Name = Some(n.to_string()); std::sync::Arc::new(std::sync::Mutex::new(m)) }).collect();
    let idx = |m: &lg::ModRef| pool.iter().position(|x| std::sync::Arc::ptr_eq(x, m)).unwrap().to_string();
    let mut l = lg::ModLoader::default();
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f2 = fired.clone();
    let h: lg::Handler = std::sync::Arc::new(move || {
        f2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let asm = Some(1u64);
    let mut res = Vec::new();
    for op in split_args(ops) {
        let b = op.as_bytes();
        let i = || (b[1] - b'0') as usize;
        let already = |l: &lg::ModLoader| pool.iter().map(|x| if l.alreadyInitialisedMods.iter().any(|y| std::sync::Arc::ptr_eq(x, y)) { "1" } else { "0" }).collect::<String>();
        res.push(match b[0] {
            b'L' => {
                l.LoadedMods.push(pool[i()].clone());
                "ok".into()
            }
            b'S' => l.ModScripts.add(pool[i()].clone(), lg::ModScript { LoadedAssembly: if b[2] == b'a' { asm } else { None }, ..Default::default() }).map_or_else(m_exc, |_| "ok".into()),
            b'R' => {
                l.RemoveMod(&pool[i()]);
                "ok".into()
            }
            b'A' => if l.GetModAssembly(&pool[i()]).is_some() { "asm" } else { "null" }.into(),
            b'M' => l.GetModMetaFromAssembly(if b[1] == b'a' { asm } else { None }).map_or_else(|| "null".into(), |m| idx(&m)),
            b'+' => {
                l.add_ModListChanged(h.clone());
                "ok".into()
            }
            b'-' => {
                l.remove_ModListChanged(&h);
                "ok".into()
            }
            b'I' => {
                fired.store(0, std::sync::atomic::Ordering::SeqCst);
                for f in l.InvokeModListChange() {
                    f();
                }
                fired.load(std::sync::atomic::Ordering::SeqCst).to_string()
            }
            b'a' => {
                pool[i()].lock().unwrap().Active = b[2] == b'1';
                "ok".into()
            }
            b'O' => {
                lg::ModificationManager::InvokeOnLoad_mod(&mut l, &pool[i()]);
                already(&l)
            }
            b'U' => {
                lg::ModificationManager::InvokeOnUnload(&mut l, &pool[i()]);
                already(&l)
            }
            b'P' => {
                lg::ModificationManager::InvokeOnLoad(&mut l, true);
                already(&l)
            }
            _ => {
                let m = pool[i()].clone();
                if !l.alreadyInitialisedMods.iter().any(|y| std::sync::Arc::ptr_eq(&m, y)) {
                    l.alreadyInitialisedMods.push(m.clone());
                }
                m_b(lg::ModificationManager::Invoke("OnLoad", &m))
            }
        });
    }
    Ok(format!("{};{},{}", m_l(res), m_l(l.LoadedMods.iter().map(idx)), m_l(l.ModScripts.iter().map(|(k, _)| idx(k)))))
}

fn m_config_text(c: &Option<lg::CompilerConfig>) -> String {
    c.as_ref().map_or_else(|| "null".into(), |c| format!("{},{},{}", m_s(c.Hostname.as_deref()), c.Port, m_b(c.ShutdownWhenGameNotFound)))
}
fn m_reply_text(r: &lg::CompilerReply) -> String {
    format!("{},{},{},{}", r.ID, lg::state_name(r.State), m_s(r.Message.as_deref()), m_b(r.Suspicious))
}
/// WriteConfig: the tree cleared, then ppgModCompiler/config.json (none for null).
fn m_write_config(t: &MTree, c: Option<&str>) -> Result<(), String> {
    for e in std::fs::read_dir(&t.0).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    if let Some(c) = c {
        std::fs::create_dir_all(t.0.join("ppgModCompiler")).map_err(|e| e.to_string())?;
        std::fs::write(t.0.join("ppgModCompiler").join("config.json"), c).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn m_instr() -> lg::ModCompileInstructions {
    lg::ModCompileInstructions { Paths: Some(std::sync::Arc::new(vec!["a.cs".to_string()])), ..Default::default() }
}
fn m_ok(r: Result<(), lg::Exc>) -> String {
    r.map_or_else(m_exc, |_| "ok".into())
}
fn m_ops<'a>(c: &Case<'a>) -> Result<Vec<&'a str>, String> {
    Ok(split_args(m_arg(c, 0)?.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or("ops")?))
}
fn m_read_config(c: &Case) -> Result<String, String> {
    let t = m_tree("tree:")?;
    m_write_config(&t, m_str(m_arg(c, 0)?)?.as_deref())?;
    Ok(m_config_text(&lg::CompilerClient::ReadConfig(&t.0)))
}
/// The static CompilerClient's op sequence (`I<config>` `S` `T` `D` `Q`).
fn m_client(c: &Case) -> Result<String, String> {
    let t = m_tree("tree:")?;
    let mut cl = lg::CompilerClient::default();
    let mut res = Vec::new();
    for op in m_ops(c)? {
        res.push(match &op[..1] {
            "I" => {
                m_write_config(&t, m_str(&op[1..])?.as_deref())?;
                cl.InitialiseConfig(&t.0);
                m_config_text(&cl.compiler.Config)
            }
            "S" => m_ok(cl.Start(&t.0)),
            "T" => m_ok(cl.Stop()),
            "D" => m_ok(cl.Dispose()),
            _ => m_reply_text(&cl.RequestCompilationSynchronous(&t.0, m_instr())),
        });
    }
    let _ = cl.compiler.Stop();
    Ok(m_l(res))
}
/// A ModCompilerSockets op sequence, a fresh one at each `N`.
fn m_sockets(c: &Case) -> Result<String, String> {
    let t = m_tree("tree:")?;
    let mut o = lg::ModCompilerSockets::default();
    let mut res = Vec::new();
    for op in m_ops(c)? {
        res.push(match &op[..1] {
            "N" => {
                o = lg::ModCompilerSockets::default();
                format!("{},{}", m_config_text(&o.Config), m_b(o.IsBusy))
            }
            "c" => {
                m_write_config(&t, m_str(&op[1..])?.as_deref())?;
                o.Config = lg::CompilerClient::ReadConfig(&t.0);
                m_config_text(&o.Config)
            }
            "b" => {
                o.IsBusy = &op[1..] == "1";
                let r = m_b(o.IsBusy);
                o.IsBusy = false;
                r
            }
            "s" => m_ok(o.Start(&t.0)),
            "t" => m_ok(o.Stop()),
            "d" => m_ok(o.Dispose()),
            "q" => m_reply_text(&o.RequestCompilationSynchronous(&t.0, m_instr())),
            "r" => o.GetReply(&t.0, &mut m_instr()).map_or_else(m_exc, |r| m_reply_text(&r)),
            "k" => m_ok(lg::ModCompilerSockets::KillAllServerProcesses()),
            "e" => {
                lg::ModCompilerSockets::OnServerError(Some("x"));
                "ok".into()
            }
            "C" => m_ok(o.StartClient()),
            "S" => m_ok(o.StartServer(&t.0)),
            "o" => {
                lg::ModCompilerSockets::OnClientOpen();
                lg::ModCompilerSockets::OnClientClose();
                "ok".into()
            }
            _ => {
                o.OnClientMessage(m_str(&op[1..])?.unwrap_or_default().as_bytes());
                m_l(o.replies.iter().map(|(k, r)| format!("{k}:{}", m_reply_text(r))))
            }
        });
    }
    Ok(m_l(res))
}
/// A ModCompilerPipes op sequence, a fresh one at each `N`.
fn m_pipes(c: &Case) -> Result<String, String> {
    let t = m_tree("tree:")?;
    let mut o = lg::ModCompilerPipes::default();
    let mut res = Vec::new();
    for op in m_ops(c)? {
        res.push(match &op[..1] {
            "N" => {
                o = lg::ModCompilerPipes::default();
                format!("{},{}", m_config_text(&o.Config), m_b(o.IsBusy))
            }
            "c" => {
                m_write_config(&t, m_str(&op[1..])?.as_deref())?;
                o.Config = lg::CompilerClient::ReadConfig(&t.0);
                m_config_text(&o.Config)
            }
            "b" => {
                o.IsBusy = &op[1..] == "1";
                let r = m_b(o.IsBusy);
                o.IsBusy = false;
                r
            }
            "s" => m_ok(o.Start(&t.0)),
            "t" => m_ok(o.Stop()),
            "d" => m_ok(o.Dispose()),
            "q" => m_reply_text(&o.RequestCompilationSynchronous(&t.0, m_instr())),
            "r" => o.GetReply(&t.0, &mut m_instr()).map_or_else(m_exc, |r| m_reply_text(&r)),
            "k" => m_ok(lg::ModCompilerPipes::KillAllServerProcesses()),
            "e" => {
                lg::ModCompilerPipes::OnServerError(Some("x"));
                "ok".into()
            }
            "n" => m_b(o.GetPipeName() == std::process::id().to_string()),
            "v" => m_ok(o.ReceiveMessages()),
            _ => m_config_text(&o.Config),
        });
    }
    Ok(m_l(res))
}

/// A ModFolderSource on a tree row's target (`<tree>;<target>`).
fn m_folder(c: &Case) -> Result<(MTree, Result<mods::ModFolderSource, String>), String> {
    let t = m_tree(m_arg(c, 0)?)?;
    let target = t.0.join(m_str(m_arg(c, 1)?)?.unwrap_or_default());
    let s = mods::ModFolderSource::new(target).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "exc:DirectoryNotFoundException".to_string() } else { "exc:IOException".to_string() });
    Ok((t, s))
}
fn m_watcher_text(t: &MTree, s: &mods::ModFolderSource) -> String {
    let (dir, pattern, on) = s.watcher.watch_state();
    format!("{},{},{},{}", s.watcher.Interval.as_secs_f64(), m_s(Some(pattern)), m_rel(t, dir), m_b(on))
}
fn m_folder_call(c: &Case, f: fn(&MTree, &mut mods::ModFolderSource, &Case) -> Result<String, String>) -> Result<String, String> {
    let (t, s) = m_folder(c)?;
    let mut s = s?;
    let r = f(&t, &mut s, c);
    s.Dispose();
    r
}

const PURE_LEGACY: &[(&str, PureCall)] = &[
    ("ModLoader.cctor", |_| {
        let t = m_tree("tree:")?;
        Ok(m_l(lg::assembly_locations(&t.0, std::path::Path::new("<game>/People Playground_Data/Managed")).iter().map(|p| m_path(&t, Some(p)))))
    }),
    ("ModLoader.constructor", |_| {
        let _ = lg::ModLoader::default();
        Ok("ok".into())
    }),
    ("ModLoader.AlphaNumeric", |c| Ok(m_s(Some(&lg::AlphaNumeric(&m_str(m_arg(c, 0)?)?.unwrap_or_default()))))),
    ("ModLoader.GetOutputAssemblyPath", |c| m_lpath(c, lg::GetOutputAssemblyPath)),
    ("ModLoader.GetModHashPath", |c| m_lpath(c, lg::GetModHashPath)),
    ("ModLoader.GetModSourcePaths", |c| {
        let t = m_tree("tree:")?;
        let m = m_largs(&t, m_arg(c, 0)?)?;
        Ok(m_paths(&t, lg::GetModSourcePaths(&t.0, &m)))
    }),
    ("ModLoader.GetModAssemblyMeta", |c| {
        let t = m_now_tree(m_arg(c, 0)?)?;
        let m = m_largs(&t, m_arg(c, 1)?)?;
        Ok(lg::GetModAssemblyMeta(&m).map_or_else(m_exc, |b| m_bytes8(Some(&b))))
    }),
    ("ModLoader.ShouldRecompile", |c| {
        let t = m_now_tree(m_arg(c, 0)?)?;
        let m = m_largs(&t, m_arg(c, 1)?)?;
        Ok(lg::ShouldRecompile(&t.0, &m).map_or_else(m_exc, m_b))
    }),
    ("ModLoader.Compile", m_compile),
    ("ModLoader.LoadModAt", m_load_mod_at),
    ("ModLoader.UpdateJSON", m_update_json),
    ("ModLoader.GetThumbnail", m_thumbnail),
    ("ModLoader.RemoveMod", m_lists),
    ("ModLoader.GetModAssembly", m_lists),
    ("ModLoader.GetModMetaFromAssembly", m_lists),
    ("ModLoader.InvokeModListChange", m_lists),
    ("ModLoader.add_ModListChanged", m_lists),
    ("ModLoader.remove_ModListChanged", m_lists),
    ("ModificationManager.InvokeOnLoad", m_lists),
    ("ModificationManager.InvokeOnUnload", m_lists),
    ("ModificationManager.Invoke", m_lists),
    ("ModificationManager.cctor", |_| {
        let m = lg::ModificationManager::default();
        Ok(format!("{},{},{},{}", m.Modifications.len(), m.BackgroundScripts.len(), m.CustomTools.len(), m.CustomPowers.len()))
    }),
    ("CompilerClient.ReadConfig", m_read_config),
    ("CompilerConfig.constructor", |_| Ok(m_config_text(&Some(lg::CompilerConfig::default())))),
    ("CompilerClient.cctor", m_client),
    ("CompilerClient.InitialiseConfig", m_client),
    ("CompilerClient.Start", m_client),
    ("CompilerClient.Stop", m_client),
    ("CompilerClient.Dispose", m_client),
    ("CompilerClient.RequestCompilationSynchronous", m_client),
    ("ModCompilerSockets.constructor", m_sockets),
    ("ModCompilerSockets.get_Config", m_sockets),
    ("ModCompilerSockets.set_Config", m_sockets),
    ("ModCompilerSockets.get_IsBusy", m_sockets),
    ("ModCompilerSockets.set_IsBusy", m_sockets),
    ("ModCompilerSockets.Start", m_sockets),
    ("ModCompilerSockets.Stop", m_sockets),
    ("ModCompilerSockets.Dispose", m_sockets),
    ("ModCompilerSockets.RequestCompilationSynchronous", m_sockets),
    ("ModCompilerSockets.GetReply", m_sockets),
    ("ModCompilerSockets.KillAllServerProcesses", m_sockets),
    ("ModCompilerSockets.OnServerError", m_sockets),
    ("ModCompilerSockets.StartClient", m_sockets),
    ("ModCompilerSockets.StartServer", m_sockets),
    ("ModCompilerSockets.OnClientOpen", m_sockets),
    ("ModCompilerSockets.OnClientClose", m_sockets),
    ("ModCompilerSockets.OnClientMessage", m_sockets),
    ("ModCompilerPipes.constructor", m_pipes),
    ("ModCompilerPipes.get_Config", m_pipes),
    ("ModCompilerPipes.set_Config", m_pipes),
    ("ModCompilerPipes.get_IsBusy", m_pipes),
    ("ModCompilerPipes.set_IsBusy", m_pipes),
    ("ModCompilerPipes.Start", m_pipes),
    ("ModCompilerPipes.Stop", m_pipes),
    ("ModCompilerPipes.Dispose", m_pipes),
    ("ModCompilerPipes.RequestCompilationSynchronous", m_pipes),
    ("ModCompilerPipes.GetReply", m_pipes),
    ("ModCompilerPipes.KillAllServerProcesses", m_pipes),
    ("ModCompilerPipes.OnServerError", m_pipes),
    ("ModCompilerPipes.GetPipeName", m_pipes),
    ("ModCompilerPipes.ReceiveMessages", m_pipes),
    ("ModFolderSource.constructor", |c| {
        let (t, s) = m_folder(c)?;
        Ok(match s {
            Err(e) => e,
            Ok(mut s) => {
                let r = format!("ok,{},{},{}", m_rel(&t, &s.Folder), m_b(s.Folder.is_dir()), m_watcher_text(&t, &s));
                s.Dispose();
                r
            }
        })
    }),
    ("ModFolderSource.get_OnChange", |c| m_folder_call(c, |_, s, _| Ok(s.OnChange.GetPersistentEventCount().to_string()))),
    ("ModFolderSource.get_Folder", |c| m_folder_call(c, |t, s, _| Ok(m_rel(t, &s.Folder)))),
    ("ModFolderSource.Initialise", |c| m_folder_call(c, |_, s, _| {
        s.Initialise();
        Ok(m_b(s.needsUpdate.load(std::sync::atomic::Ordering::SeqCst)))
    })),
    ("ModFolderSource.OnUpdate", |c| m_folder_call(c, |_, s, c| {
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let f = fired.clone();
        s.OnChange.AddListener(std::sync::Arc::new(move || {
            f.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        s.needsUpdate.store(m_arg(c, 2)? == "1", std::sync::atomic::Ordering::SeqCst);
        s.OnUpdate();
        let f1 = fired.load(std::sync::atomic::Ordering::SeqCst);
        s.OnUpdate();
        Ok(format!("{f1},{},{}", fired.load(std::sync::atomic::Ordering::SeqCst), m_b(s.needsUpdate.load(std::sync::atomic::Ordering::SeqCst))))
    })),
    ("ModFolderSource.<.ctor>b__8_0", |c| m_folder_call(c, |_, s, _| {
        s.needsUpdate.store(false, std::sync::atomic::Ordering::SeqCst);
        s.watcher.invoke_on_change();
        Ok(m_b(s.needsUpdate.load(std::sync::atomic::Ordering::SeqCst)))
    })),
    ("ModFolderSource.Dispose", |c| m_folder_call(c, |t, s, _| {
        s.Dispose();
        let w1 = m_watcher_text(t, s);
        s.Dispose();
        Ok(w1)
    })),
    ("TrustCache.cctor", m_trust_key),
    ("TrustCache.Trust", m_trust),
    ("TrustCache.Distrust", m_trust),
    ("TrustCache.IsTrusted", m_trust),
    ("TrustCache.Save", m_trust),
    ("TrustCache.Load", m_trust),
    ("ModAPI.get_Metadata", |c| {
        let api = &mods::mod_api::STATE;
        api.lock().unwrap().metaDataIsValid = m_arg(c, 0)? == "1";
        let r = mods::mod_api::get_Metadata();
        api.lock().unwrap().metaDataIsValid = false;
        Ok(r.map_or_else(m_exc, |m| if m.is_some() { "obj".into() } else { "null".into() }))
    }),
    ("ModAPI.RegisterLiquid", sp_liquid),
    ("ModAPI.FindCartridge", sp_cartridge),
    ("ModAPI.FindPhysicalProperties", sp_properties),
    ("ModAPI.FindMaterial", sp_material),
    ("ModAPI.Register", sp_modapi_ops),
    ("ModAPI.RegisterInput", sp_modapi_ops),
    ("ModAPI.KeepExtraObjects", sp_modapi_ops),
    ("ContraptionSerialiser.LoadContraption", sp_load_row),
    ("ContraptionSerialiser.SaveContraption", sp_save),
    ("ObjectStateConverter.Convert", sp_convert),
    ("ObjectState.CreateNew", sp_create_new),
    ("ObjectState.LinkReferences", sp_link_references),
    ("ModAPI.SerialiseJSON", m_json),
    ("ModAPI.DeleteJSON", m_json),
    ("ModAPI.DeserialiseJSON", m_json),
];

/// ModAPI's JSON ops on an empty working directory (`M`/`MS` make Mods and Mods/sub, `S:`path,string SerialiseJSON,
/// `D:`path DeleteJSON, `R:`path DeserialiseJSON<string>, `W:`name,text written into Mods): each op's result, then every
/// file (ordinal order) with its bytes.
fn m_json(c: &Case) -> Result<String, String> {
    let t = m_tree("tree:")?;
    let mut res = Vec::new();
    for op in m_ops(c)? {
        let (k, rest) = op.split_once(':').unwrap_or((op, ""));
        let mut a = rest.splitn(2, ',');
        let p = m_str(a.next().filter(|x| !x.is_empty()).unwrap_or("null"))?.unwrap_or_default();
        res.push(match k {
            "M" | "MS" => {
                std::fs::create_dir_all(t.0.join(if k == "M" { "Mods" } else { "Mods/sub" })).map_err(|e| e.to_string())?;
                "ok".into()
            }
            "S" => {
                let v = m_str(a.next().ok_or("value")?)?;
                m_ok(mods::mod_api::SerialiseJSON(&t.0, v.as_ref().map(|v| lg::json_str(Some(v))).as_deref(), &p))
            }
            "D" => m_ok(mods::mod_api::DeleteJSON(&t.0, &p)),
            "R" => match mods::mod_api::DeserialiseJSON(&t.0, &p) {
                Err(e) => m_exc(e),
                Ok(text) => match super::super::jsonnet::parse(&text) {
                    Ok(Some(super::super::jsonnet::JTok::Str(s))) => m_s(Some(&s)),
                    Ok(None | Some(super::super::jsonnet::JTok::Null)) => "null".into(),
                    r => return Err(format!("DeserialiseJSON<string> of {r:?}")),
                },
            },
            _ => {
                let w = m_str(a.next().ok_or("text")?)?.unwrap_or_default();
                let f = t.0.join("Mods").join(&p);
                match std::fs::write(&f, w) {
                    Ok(()) => "ok".into(),
                    Err(e) => m_exc(lg::io_exc(&e, !f.parent().is_some_and(std::path::Path::is_dir))),
                }
            }
        });
    }
    let mut files = Vec::new();
    let mut stack = vec![t.0.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                let rel = e.path().strip_prefix(&t.0).unwrap().to_string_lossy().replace('\\', "/");
                files.push((rel, std::fs::read(e.path()).map_err(|e| e.to_string())?));
            }
        }
    }
    files.sort();
    Ok(format!("{};{}", m_l(res), m_l(files.iter().map(|(p, b)| format!("{}:{}", m_s(Some(p)), m_b64e(b))))))
}

/// TrustCache's file name for a key: Hash128.Compute(key).ToString(), an empty set.
fn m_trust_key(c: &Case) -> Result<String, String> {
    let key = m_str(m_arg(c, 0)?)?.unwrap_or_default();
    Ok(format!("{},0", m_s(Some(lg::TrustCache::with_key(&key).path()))))
}

/// TrustCache ops (`T:`/`D:`/`I:`meta, `S`ave, `L`oad, `W:`file bytes, `X` delete) on the empty cache over file `path`:
/// each op's result, then the set and the file.
fn m_trust(c: &Case) -> Result<String, String> {
    let ops = m_ops(c)?;
    let path = m_str(m_arg(c, 1)?)?.ok_or("path")?;
    let t = m_tree("tree:")?;
    let file = t.0.join(&path);
    *lg::TRUST_CACHE.lock().unwrap() = lg::TrustCache::with_path(&path);
    let mut res = Vec::new();
    for op in ops {
        let (k, rest) = op.split_once(':').unwrap_or((op, ""));
        res.push(match k {
            "T" | "D" => {
                let m = m_largs(&t, rest)?;
                let mut g = lg::TRUST_CACHE.lock().unwrap();
                m_ok(if k == "T" { g.Trust(&m) } else { g.Distrust(&m) })
            }
            "I" => m_b(lg::TrustCache::IsTrusted(&m_largs(&t, rest)?)),
            "S" => m_ok(lg::TRUST_CACHE.lock().unwrap().Save(&t.0)),
            "L" => m_ok(lg::TRUST_CACHE.lock().unwrap().Load(&t.0)),
            "W" => {
                std::fs::write(&file, m_b64(rest)?).map_err(|e| e.to_string())?;
                "ok".into()
            }
            _ => {
                let _ = std::fs::remove_file(&file);
                "ok".into()
            }
        });
    }
    let set = lg::TRUST_CACHE.lock().unwrap().trusted();
    let f = std::fs::read(&file).map_or_else(|_| "none".to_string(), |b| m_b64e(&b));
    Ok(format!("{};{},{f}", m_l(res), m_l(set.iter().map(|h| h.to_string()))))
}

// ---- tools/recorder/SaveProbe.cs (PPG_SAVEPROBE): the real game's save code on copies of contraptions, and ModAPI's
// lookups and registrations, after the Default map loaded. One rewrite Game, loaded once. ----
thread_local! {
    static SP_GAME: std::cell::RefCell<Option<Game>> = const { std::cell::RefCell::new(None) };
}
fn sp_game<R>(f: impl FnOnce(&mut Game) -> Result<R, String>) -> Result<R, String> {
    SP_GAME.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            let game = crate::game_assets::find_game(None)?;
            let mut g = crate::play::new_game(&game, "Default", None, None, 0)?;
            for _ in 0..30 {
                crate::play::step(&mut g, 1.0 / 60.0)?;
            }
            *c = Some(g);
        }
        f(c.as_mut().unwrap())
    })
}
fn sp_db<R>(f: impl FnOnce(&mut crate::assets::db::AssetDb) -> Result<R, String>) -> Result<R, String> {
    sp_game(|g| with_assets(g, |_, db| f(db)))
}
/// A logged call's result: `ok` or `exc:<class>`, then its Debug.Log lines as `<LogType>:<text>`.
fn sp_logged(r: Result<Vec<String>, lg::Exc>) -> String {
    match r {
        Ok(logs) => format!("ok;{}", m_l(logs.iter().map(|l| m_s(Some(l))))),
        Err(e) => format!("{};[]", m_exc(e)),
    }
}
fn sp_cartridge(c: &Case) -> Result<String, String> {
    let name = m_str(m_arg(c, 0)?)?.unwrap_or_default();
    Ok(match sp_db(|db| mods::mod_api::FindCartridge(db, &name))? {
        None => "null".into(),
        Some(x) => [x.Damage, x.Recoil, x.ImpactForce, x.StartSpeed, x.PenetrationRandomAngleMultiplier].map(hex).join(","),
    })
}
fn sp_properties(c: &Case) -> Result<String, String> {
    let name = m_str(m_arg(c, 0)?)?.unwrap_or_default();
    let Some(p) = sp_db(|db| mods::mod_api::FindPhysicalProperties(db, &name))? else { return Ok("null".into()) };
    let f = [p.Softness, p.Flammability, p.MagneticAttractionIntensity, p.Burnrate, p.BurningTemperatureThreshold, p.HeatTransferSpeedMultiplier, p.Buoyancy,
        p.ImpactIntensityMutliplier, p.HitVolumeMultiplier, p.SharpForceThresholdMultiplier, p.LodgeStrengthMultiplier, p.Brittleness, p.BulletSpeedAbsorptionPower].map(hex).join(",");
    let len = |n: Option<usize>| n.map_or(-1, |n| n as i64);
    Ok(format!("{f},{}{}{}{},{},{},{},{}", m_b(p.Conducting), m_b(p.SparksOnSlide), m_b(p.Sharp), m_b(p.IgnoreStabResistance), p.SoftImpact, p.HardImpact, len(p.SizzleSounds), len(p.StabSound)))
}
fn sp_material(c: &Case) -> Result<String, String> {
    let name = m_str(m_arg(c, 0)?)?.unwrap_or_default();
    sp_game(|_| Ok(()))?;
    let Some(r) = mods::mod_api::FindMaterial(&name) else { return Ok("null".into()) };
    sp_db(|db| Ok(m_s(Some(&crate::assets::serialized::Reader::new(db.data(r)).string()?))))
}
/// Register / RegisterInput / KeepExtraObjects ops (`R:`valid,modification,NameOverride; `I:`valid,id,name,key; `K:`valid)
/// on a fresh ModAPI state with a Metadata set: each op's logged result and what it changed.
fn sp_modapi_ops(c: &Case) -> Result<String, String> {
    let api = &mods::mod_api::STATE;
    let mm = &lg::MODIFICATION_MANAGER;
    mm.lock().unwrap().Modifications.clear();
    {
        let mut s = api.lock().unwrap();
        s.Modded.clear();
        s.registeredInputsByMod.clear();
    }
    mods::mod_api::set_Metadata(Some(std::sync::Arc::new(std::sync::Mutex::new(ModMetaData::new()))));
    let mut res = Vec::new();
    for op in m_ops(c)? {
        let (kind, rest) = op.split_once(':').ok_or("op")?;
        let a: Vec<&str> = rest.split(',').collect();
        api.lock().unwrap().metaDataIsValid = a[0] == "1";
        res.push(match kind {
            "R" => {
                let name = m_str(a[2])?;
                let before = mm.lock().unwrap().Modifications.len();
                let r = sp_logged(mods::mod_api::Register(a[1].parse().map_err(|_| "which")?, name.as_deref()).map(|t| vec![format!("Log:{t}")]));
                format!("{r},{}", mm.lock().unwrap().Modifications.len() - before)
            }
            "I" => {
                let (id, name) = (m_str(a[1])?.unwrap_or_default(), m_str(a[2])?.unwrap_or_default());
                let r = mods::mod_api::RegisterInput(&name, &id, a[3].parse().map_err(|_| "key")?);
                let r = sp_logged(r.map(|(added, t)| vec![format!("{}:{t}", if added { "Log" } else { "Error" })]));
                let s = api.lock().unwrap();
                let now = s.Modded.iter().find(|(i, _, _)| *i == id).map_or_else(|| "none".to_string(), |(_, n, k)| format!("{}:{k}", m_s(Some(n))));
                format!("{r},{now}")
            }
            "K" => {
                api.lock().unwrap().ShouldRemoveToRemoveWhenModded = true;
                let r = sp_logged(Ok(mods::mod_api::KeepExtraObjects().map(|t| format!("Warning:{t}")).into_iter().collect()));
                format!("{r},{}", m_b(api.lock().unwrap().ShouldRemoveToRemoveWhenModded))
            }
            _ => return Err(format!("op {op}")),
        });
    }
    {
        let mut s = api.lock().unwrap();
        s.metaDataIsValid = false;
        s.ShouldRemoveToRemoveWhenModded = true;
    }
    mods::mod_api::set_Metadata(None);
    mm.lock().unwrap().Modifications.clear();
    Ok(m_l(res))
}
/// RegisterLiquid ops (`id,allowed`) on one registry that starts as Global.Awake leaves it: each op's logged result and
/// how many liquids it added.
fn sp_liquid(c: &Case) -> Result<String, String> {
    let mut reg: Vec<String> = super::super::circulation::LIQUIDS.iter().map(|(id, _)| id.to_string()).collect();
    let api = &mods::mod_api::STATE;
    let mut res = Vec::new();
    for op in m_ops(c)? {
        let (id, on) = op.split_once(',').ok_or("op")?;
        let id = m_str(id)?.unwrap_or_default();
        api.lock().unwrap().allowedToRegisterExternalLiquids = on == "1";
        let before = reg.len();
        let r = mods::mod_api::RegisterLiquid(&mut reg, &id);
        res.push(format!("{},{}", sp_logged(r.map(|t| t.map(|t| format!("Error:{t}")).into_iter().collect())), reg.len() - before));
    }
    api.lock().unwrap().allowedToRegisterExternalLiquids = false;
    Ok(m_l(res))
}
/// The contraption a row names (`<folder>,<metadata path relative to it>`), as ContraptionFolderSource lists it.
fn sp_meta(c: &Case) -> Result<ContraptionMetaData, String> {
    let (dir, rel) = m_arg(c, 0)?.split_once(',').ok_or("args")?;
    let (mut dir, rel) = (m_str(dir)?.ok_or("dir")?, m_str(rel)?.ok_or("rel")?);
    // The recorder ran over a temp copy (a PC scratch folder, gone after the run): the same copy is kept in every
    // machine's store as reference-probe/game/bbsave-c, so a row checks on any machine.
    if !std::path::Path::new(&dir).is_dir() {
        dir = "reference-probe/game/bbsave-c".into();
    }
    let want = std::path::Path::new(&dir).join(&rel);
    ContraptionFolderSource::new(std::path::Path::new(&dir)).GetContraptions().into_iter().find(|m| m.PathToMetadata.as_deref() == Some(want.as_path())).ok_or_else(|| format!("no contraption {rel}"))
}
fn sp_load(c: &Case) -> Result<(ContraptionMetaData, Vec<super::super::save::ObjectState>, ct::LoadMessages), String> {
    let mut meta = sp_meta(c)?;
    if std::env::var_os("PPG_THROW_AT").is_some() {
        eprintln!("row {:?}", meta.Name);
    }
    // ModdingApi.FindSpawnable's migrations come from the loaded catalog.
    sp_game(|_| Ok(()))?;
    let mut msgs = ct::LoadMessages::default();
    let states = ct::load_contraption(&mut meta, &mut msgs)?;
    if let Some(Err(e)) = msgs.warnings.iter().any(|w| w.starts_with("Error deserialising")).then(|| std::fs::read(meta.PathToDataFile.clone().unwrap_or_default()).map_err(|e| e.to_string()).and_then(|b| ct::deserialise_states(&b))) {
        eprintln!("sp_load {:?}: {e}", meta.Name);
    }
    Ok((meta, states, msgs))
}
fn sp_load_row(c: &Case) -> Result<String, String> {
    let (meta, states, msgs) = sp_load(c)?;
    Ok(format!("{},{};{}", m_b64e(&ct::serialise_states(&states)?), m_s(meta.DisplayName.as_deref()), m_l(msgs.warnings.iter().map(|w| m_s(Some(&format!("Warning:{w}")))))))
}
fn sp_save(c: &Case) -> Result<String, String> {
    let (meta, states, _) = sp_load(c)?;
    let dir = std::env::temp_dir().join(format!("ppg-saveprobe-{}", std::process::id()));
    let r = ct::save_contraption(&dir, meta.Name.as_deref().unwrap_or(""), &states).and_then(|m| {
        let data = std::fs::read(m.PathToDataFile.as_ref().unwrap()).map_err(|e| e.to_string())?;
        let json = std::fs::read_to_string(m.PathToMetadata.as_ref().unwrap()).map_err(|e| e.to_string())?;
        Ok(format!("{},{}", m_b64e(&data), m_s(Some(&json))))
    });
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// The members whose value two game runs give differently (collider bounds, Time-stamped damage): left out of the compare on both sides.
const SP_UNSTABLE: [&str; 4] = ["Circumference", "InitialBounds", "damagePoints", "damagePointTimeStamps"];
/// ObjectStateConverter.Convert both ways on a fresh game: the loaded states spawned at the origin, then those objects back into states
/// (serialised as a Contraption), with SP_UNSTABLE's members dropped.
fn sp_convert(c: &Case) -> Result<String, String> {
    let (_, states, _) = sp_load(c)?;
    let mut g = sp_fresh_game()?;
    sp_pause(&mut g);
    g.random = crate::unity::Random::new(1);
    let roots = g.object_states_convert(&states, [0.0; 3], false);
    sp_seed(&mut g);
    sp_paused_frames(&mut g)?;
    sp_back(&mut g, &roots, [0.0; 3])
}
/// ObjectState.CreateNew alone, flipped at (3.25, -1.5), for every loaded state plus one whose spawnable is missing and one with a
/// child path the prefab lacks (the probe's `extra`): each one's `ok`/`exc:Exception`, then the objects back as in sp_convert.
fn sp_create_new(c: &Case) -> Result<String, String> {
    use super::super::save::{ObjectState, TransformPrototype};
    let (_, mut states, _) = sp_load(c)?;
    let s0 = states.first().cloned().ok_or("no states")?;
    states.push(ObjectState { SpawnableAssetName: "NoSuchSpawnable".into(), ..s0.clone() });
    let mut child = s0;
    child.Children.push(("NoSuchChild".into(), TransformPrototype::new([1.0, 1.0], 7.0, [1.0, 1.0, 1.0])));
    states.push(child);
    let at = [3.25, -1.5, 0.0];
    let mut g = sp_fresh_game()?;
    sp_pause(&mut g);
    g.random = crate::unity::Random::new(1);
    let made = g.object_states_create(&states, at, true, false);
    sp_seed(&mut g);
    sp_paused_frames(&mut g)?;
    let each = m_l(made.iter().map(|r| if r.is_ok() { "ok".to_string() } else { "exc:Exception".to_string() }));
    let roots: Vec<i32> = made.into_iter().flatten().collect();
    Ok(format!("{each},{}", sp_back(&mut g, &roots, at)?))
}
/// ObjectState.LinkReferences (with LinkReferencesFrom and MonoBehaviourPrototype.LinkReferencesToMonoBehaviour) alone: every loaded state, the first
/// with the probe's missing child path added, CreateNew'd unflipped at the origin and linked with Convert's pool, then again with an empty pool; each
/// set's per-object `ok` list and the objects back as in sp_convert.
fn sp_link_references(c: &Case) -> Result<String, String> {
    use super::super::save::TransformPrototype;
    let (_, mut states, _) = sp_load(c)?;
    states.first_mut().ok_or("no states")?.Children.push(("NoSuchChild".into(), TransformPrototype::new([1.0, 1.0], 7.0, [1.0, 1.0, 1.0])));
    let mut res = Vec::new();
    for full in [true, false] {
        let mut g = sp_fresh_game()?;
        sp_pause(&mut g);
        g.random = crate::unity::Random::new(1);
        let roots: Vec<i32> = g.object_states_create_linked(&states, [0.0; 3], full).into_iter().flatten().collect();
        sp_seed(&mut g);
        sp_paused_frames(&mut g)?;
        res.push(format!("{},{}", m_l(roots.iter().map(|_| "ok".to_string())), sp_back(&mut g, &roots, [0.0; 3])?));
    }
    Ok(res.join(","))
}
/// The probe's Pause(true): Global's private `paused` field (not set_Paused, so no interpolation change) and Time.timeScale = 0.
/// The probe (bbsave13 on) holds one pause until the clock is still before the first case and never unpauses between cases:
/// deltaTime 0, and smoothDeltaTime's 0.8x decay stuck at the denormal 0x00000002 (0.8 * 2 ulp rounds back to 2; bbsave13.tdiag).
fn sp_pause(g: &mut Game) {
    g.paused = true;
    g.time.timeScale = 0.0;
    g.time.deltaTime = 0.0;
    g.time.smoothDeltaTime = f32::from_bits(2);
}
/// The probe's determinism, per creation (after the Random.InitState(1) before it): the k-th new PhysicalBehaviour's GetHashCode (k from
/// 0 in first-call order, which is Start order: Start's Random.InitState(GetHashCode()) is its first) is 0x100000 * (k + 1). The hash
/// goes in `hash_code` before the queued Starts run, so physical_start seeds from it as always.
fn sp_seed(g: &mut Game) {
    let starts: Vec<usize> = g.pending_starts.iter().filter_map(|s| if let super::super::misc::PendingStart::Physical(p) = s { Some(*p) } else { None }).collect();
    for (k, p) in starts.into_iter().enumerate() {
        g.physicals[p].hash_code = 0x10_0000 * (k as i32 + 1);
    }
}
/// The probe's two paused frames: the new behaviours' Start, then Update (a WireBehaviour whose joint lost the connected body it
/// used to have destroys itself there) and LateUpdate, no physics step. The probe's coroutine creates between Update and LateUpdate, so the
/// creation frame's LateUpdate comes first, before any new Start (DrawableComponent: a Fireworks edge's cachedWorldMatrix).
fn sp_paused_frames(g: &mut Game) -> Result<(), String> {
    g.linefy_late_update();
    for _ in 0..2 {
        crate::play::step(g, 1.0 / 60.0)?;
    }
    Ok(())
}
/// A fresh Default map, run 30 frames as sp_game's (the probe converts long after the map loaded: AmbientTemperature and the rest are set).
fn sp_fresh_game() -> Result<Game, String> {
    let mut g = crate::play::new_game(&crate::game_assets::find_game(None)?, "Default", None, None, 0)?;
    // The probe's game never has the cursor on the made objects (its .diag: ShowOutline False on every body after Start), so the
    // mouse is far off: an object under it would be hovered and RefreshOutlines would show its outline.
    g.input.mouse = [1.0e5, 1.0e5, 0.0];
    for _ in 0..30 {
        crate::play::step(&mut g, 1.0 / 60.0)?;
    }
    Ok(g)
}
/// The roots back into states (ObjectStateConverter.Convert's GameObject overload at `origin`), SP_UNSTABLE's members dropped and every Guid renumbered.
fn sp_back(g: &mut Game, roots: &[i32], origin: [f32; 3]) -> Result<String, String> {
    let selected: Vec<usize> = roots.iter().filter_map(|&r| (0..g.physicals.len()).find(|&p| !g.physicals[p].destroyed && g.root_id(p) == r)).collect();
    let mut back = g.object_states_of(&selected, origin);
    // Regenerate's Guid.NewGuid differs every run: each Guid is renumbered by first appearance (Guid.Empty stays 0), as the probe does.
    let mut seen: Vec<u128> = Vec::new();
    let mut canon = |x: &mut u128| {
        if *x != 0 {
            let k = seen.iter().position(|y| y == x).unwrap_or_else(|| { seen.push(*x); seen.len() - 1 });
            *x = k as u128 + 1;
        }
    };
    use super::super::save::SavedValue;
    for st in &mut back {
        for proto in st.RootComponentData.iter_mut().chain(st.ChildComponentData.iter_mut().flat_map(|(_, v)| v.iter_mut())) {
            proto.storedFields.retain(|(k, _)| !SP_UNSTABLE.iter().any(|u| k.contains(u)));
            proto.storedProperties.retain(|(k, _)| !SP_UNSTABLE.iter().any(|u| k.contains(u)));
            for (_, v) in proto.storedFields.iter_mut().chain(proto.storedProperties.iter_mut()) {
                match v {
                    SavedValue::Guid(x) => canon(x),
                    SavedValue::Guids(xs) => xs.iter_mut().for_each(&mut canon),
                    _ => {}
                }
            }
            for (_, l) in &mut proto.storedReferenceFields {
                canon(&mut l.UniqueIdentity);
            }
        }
    }
    Ok(m_b64e(&ct::serialise_states(&back)?))
}
