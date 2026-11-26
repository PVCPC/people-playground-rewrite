//! The legacy ([Obsolete]) C# mod path of 1.28: ModLoader, ModificationManager, TrustCache, CompilerClient with its two
//! compiler links (ModCompilerSockets over WatsonTcp, ModCompilerPipes over a named pipe) and CompilerServerBehaviour.
//! Nothing in 1.28 runs it (ModLoadStep loads Lua mods; no scene holds a CompilerServerBehaviour) and the compiler server,
//! ppgModCompiler/PPGModCompiler.exe, is not shipped. The game's relative paths ("CompiledMods", "ppgModCompiler") resolve
//! against its working directory, the `base` these functions take. Exceptions are `Exc` (the class and Mono's message).
#![allow(non_snake_case)]

use super::jsonnet::{self, JTok};
use super::mods::ModMetaData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

/// A thrown exception: its class, Message and stack trace (Mono's frame lines, where the catcher shows them).
#[derive(Clone, Debug, PartialEq)]
pub struct Exc {
    pub class: &'static str,
    pub message: String,
    pub trace: Vec<String>,
}

impl Exc {
    pub fn new(class: &'static str, message: impl Into<String>) -> Exc {
        Exc { class, message: message.into(), trace: Vec::new() }
    }
    pub fn nre() -> Exc {
        Exc::new("NullReferenceException", "Object reference not set to an instance of an object")
    }
    /// The exception with its stack trace.
    fn at(self, trace: Vec<String>) -> Exc {
        Exc { trace, ..self }
    }
    /// Exception.ToString as Mono writes it: `System.<class>: <message>`, then the trace's lines, each after "\r\n".
    pub fn to_text(&self) -> String {
        let ns = match self.class {
            "Win32Exception" => "System.ComponentModel.",
            "SocketException" => "System.Net.Sockets.",
            "FileNotFoundException" | "DirectoryNotFoundException" | "IOException" => "System.IO.",
            _ => "System.",
        };
        let mut s = format!("{ns}{}: {}", self.class, self.message);
        for f in &self.trace {
            s += "\r\n";
            s += f;
        }
        s
    }
}

/// A frame of a Mono stack trace in the player (no symbols): `  at <method> [0x<IL offset>] in <<assembly's Mvid>>:0 `.
fn frame(assembly: &str, method: &str, il: u32) -> String {
    format!("  at {method} [0x{il:05x}] in <{}>:0 ", crate::assets::il::mvid(assembly))
}

/// Win32Exception(code).Message as Mono on Windows makes it: FormatMessage's text (with its "\r\n") in a 256-char buffer,
/// the unused rest NUL.
pub fn win32_message(code: i32) -> String {
    let text = match code {
        2 => "The system cannot find the file specified.",
        5 => "Access is denied.",
        267 => "The directory name is invalid.",
        10060 => "A connection attempt failed because the connected party did not properly respond after a period of time, or established connection failed because connected host has failed to respond.",
        10061 => "No connection could be made because the target machine actively refused it.",
        11001 => "No such host is known.",
        _ => "Unknown error",
    };
    let mut s = format!("{text}\r\n");
    while s.chars().count() < 256 {
        s.push('\0');
    }
    s
}

pub(crate) fn io_exc(e: &std::io::Error, dir_missing: bool) -> Exc {
    match e.kind() {
        std::io::ErrorKind::NotFound if dir_missing => Exc::new("DirectoryNotFoundException", "Could not find a part of the path."),
        std::io::ErrorKind::NotFound => Exc::new("FileNotFoundException", "Could not find file."),
        std::io::ErrorKind::PermissionDenied => Exc::new("UnauthorizedAccessException", "Access to the path is denied."),
        _ => Exc::new("IOException", e.to_string()),
    }
}

// ---- System.IO.Path (Mono on Windows) ----

fn bad_path_char(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '"' | '<' | '>' | '|' | '\0'..='\u{1f}'))
}
fn is_rooted(p: &str) -> bool {
    p.starts_with(['/', '\\']) || (p.len() >= 2 && p.as_bytes()[1] == b':')
}

/// Path.Combine(a, b): a null part throws ArgumentNullException, an invalid path char ArgumentException; an empty part gives
/// the other, a rooted `b` wins, else `a` + a separator (unless `a` ends in one or ':') + `b`.
pub fn path_combine(a: Option<&str>, b: Option<&str>) -> Result<String, &'static str> {
    let (a, b) = (a.ok_or("ArgumentNullException")?, b.ok_or("ArgumentNullException")?);
    if bad_path_char(a) || bad_path_char(b) {
        return Err("ArgumentException");
    }
    if a.is_empty() {
        return Ok(b.into());
    }
    if b.is_empty() {
        return Ok(a.into());
    }
    if is_rooted(b) {
        return Ok(b.into());
    }
    let sep = if a.ends_with(['/', '\\', ':']) { "" } else { "/" };
    Ok(format!("{a}{sep}{b}"))
}

/// Path.GetFullPath(p) against the working directory `base` (Err: the exception's class); `full_path` has its message.
pub fn get_full_path(base: &Path, p: &str) -> Result<String, &'static str> {
    full_path(base, p).map_err(|e| e.class)
}

/// Path.GetFullPath(p) as Mono's Path.InsecureGetFullPath makes it on Windows, against the working directory `base`:
/// WindowsDriveAdjustment (a drive-relative `X:rest` through GetFullPathNameW), the UNC check, the working folder (or its
/// drive) put in front, then CanonicalizePath, and the trailing separator kept. Separators come out as `/` (Mono's `\`;
/// the few places Mono keeps a `/` look the same). A `base` with no drive (not Windows) stands as drive C: for the
/// working folder, and a result on C: loses the "C:" again. Checked against Mono's GetFullPath in the game's Mono:
/// full_path_cases.tsv (811 paths, two working folders).
pub fn full_path(base: &Path, p: &str) -> Result<String, Exc> {
    let mut cwd = base.to_string_lossy().replace('/', "\\");
    while cwd.len() > 3 && cwd.ends_with('\\') {
        cwd.pop();
    }
    let no_drive = cwd.as_bytes().get(1) != Some(&b':');
    if no_drive {
        cwd = format!("C:{cwd}");
    }
    let out = mono_full_path(p, &cwd.chars().collect::<Vec<_>>())?;
    let out = if no_drive { out.strip_prefix("C:").map(str::to_string).unwrap_or(out) } else { out };
    Ok(out.replace('\\', "/"))
}

fn mono_sep(c: char) -> bool {
    c == '\\' || c == '/'
}
fn arg_exc(m: &str) -> Exc {
    Exc::new("ArgumentException", m)
}
/// Path.IsPathRooted: an invalid char throws; a separator first, or ':' second.
fn mono_rooted(p: &[char]) -> Result<bool, Exc> {
    if p.is_empty() {
        return Ok(false);
    }
    if p.iter().any(|&c| matches!(c, '"' | '<' | '>' | '|' | '\0'..='\u{1f}')) {
        return Err(arg_exc("Illegal characters in path."));
    }
    Ok(mono_sep(p[0]) || (p.len() > 1 && p[1] == ':'))
}
/// Path.GetPathRoot
fn mono_root(p: &[char], cwd: &[char]) -> Result<Vec<char>, Exc> {
    if p.iter().all(|c| c.is_whitespace()) {
        return Err(arg_exc("The specified path is not of a legal form."));
    }
    if !mono_rooted(p)? {
        return Ok(vec![]);
    }
    if p.len() == 1 && mono_sep(p[0]) {
        return Ok(vec!['\\']);
    }
    if p.len() < 2 {
        return Ok(vec![]);
    }
    if mono_sep(p[0]) && mono_sep(p[1]) {
        return Ok(['\\', '\\'].into_iter().chain(server_share(p)).collect());
    }
    if mono_sep(p[0]) {
        return Ok(vec!['\\']);
    }
    if p[1] != ':' {
        return Ok(cwd[..2].to_vec());
    }
    Ok(p[..if p.len() >= 3 && mono_sep(p[2]) { 3 } else { 2 }].to_vec())
}
/// Path.GetServerAndShare: after the leading two chars, up to the second separator, `/` as `\`.
fn server_share(p: &[char]) -> Vec<char> {
    let mut n = 2;
    while n < p.len() && !mono_sep(p[n]) {
        n += 1;
    }
    if n < p.len() {
        n += 1;
        while n < p.len() && !mono_sep(p[n]) {
            n += 1;
        }
    }
    p[2..n].iter().map(|&c| if c == '/' { '\\' } else { c }).collect()
}
/// Path.SameRoot
fn same_root(root: &[char], p: &[char]) -> bool {
    if root.len() < 2 || p.len() < 2 {
        return false;
    }
    if mono_sep(root[0]) && mono_sep(root[1]) {
        let low = |v: Vec<char>| v.into_iter().collect::<String>().to_lowercase();
        return mono_sep(p[0]) && mono_sep(p[1]) && low(server_share(root)) == low(server_share(p));
    }
    if root[0] != p[0] || p[1] != ':' {
        return false;
    }
    if root.len() > 2 && p.len() > 2 {
        return mono_sep(root[2]) && mono_sep(p[2]);
    }
    true
}
/// A drive letter as RtlUpcaseUnicodeChar makes it (one char; one that would grow stays).
fn upper1(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}
/// GetFullPathNameW on `X:rest` (rest not starting with a separator): the drive's current folder (the working folder
/// when it is that drive, else its root: the game's process has no other drive's) joined with rest; `.` and `..`
/// resolved, a middle part's single trailing `.` dropped, the last part's trailing dots and spaces, a part left empty
/// gone, and a trailing separator kept when rest ends in one (or in a part trimmed away).
fn win32_full_path(p: &[char], cwd: &[char]) -> Vec<char> {
    let d = upper1(p[0]);
    let start: Vec<char> = if d == upper1(cwd[0]) { cwd.to_vec() } else { vec![d, ':', '\\'] };
    let rest: String = p[2..].iter().map(|&c| if c == '/' { '\\' } else { c }).collect();
    let head: String = start.iter().skip(3).collect();
    let mut segs: Vec<&str> = if head.is_empty() { vec![] } else { head.split('\\').collect() };
    if !rest.is_empty() {
        segs.extend(rest.split('\\'));
    }
    let (mut parts, mut trailing) = (Vec::<String>::new(), false);
    for (k, s) in segs.iter().enumerate() {
        let last = k + 1 == segs.len();
        let s = match *s {
            "" => {
                trailing = last;
                continue;
            }
            "." => continue,
            ".." => {
                parts.pop();
                continue;
            }
            s if last => {
                let t = s.trim_end_matches(['.', ' ']);
                trailing = t.is_empty();
                t
            }
            s => match s.strip_suffix('.') {
                Some(t) if !t.ends_with('.') => t,
                _ => s,
            },
        };
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    let mut out: String = start[..3].iter().collect::<String>() + &parts.join("\\");
    if trailing && !parts.is_empty() {
        out.push('\\');
    }
    out.chars().collect()
}
/// Path.WindowsDriveAdjustment: `\` or `/` alone is the working folder's root; `X:` (X a letter) the working folder when
/// X is its drive (case-sensitive), else GetFullPathNameW; `X:rest` without a separator after the ':' likewise
/// Path.Combine(working folder, rest) or GetFullPathNameW.
fn drive_adjust(p: Vec<char>, cwd: &[char]) -> Result<Vec<char>, Exc> {
    if p.len() < 2 {
        if p.len() == 1 && mono_sep(p[0]) {
            return mono_root(cwd, cwd);
        }
        return Ok(p);
    }
    if p[1] != ':' || !p[0].is_alphabetic() {
        return Ok(p);
    }
    if p.len() == 2 {
        return Ok(if cwd[0] == p[0] { cwd.to_vec() } else { win32_full_path(&p, cwd) });
    }
    if mono_sep(p[2]) {
        return Ok(p);
    }
    if cwd[0] != p[0] {
        return Ok(win32_full_path(&p, cwd));
    }
    // Path.Combine(cwd, rest): a rooted rest wins.
    let rest = &p[2..];
    if mono_rooted(rest)? {
        return Ok(rest.to_vec());
    }
    let mut out = cwd.to_vec();
    if !matches!(out.last(), Some('\\' | '/' | ':')) {
        out.push('\\');
    }
    out.extend_from_slice(rest);
    Ok(out)
}
/// Path.CanonicalizePath (Windows): trimmed; split at both separators; each part's trailing white space trimmed, `.` and
/// empty parts (but the first) dropped, `..` going back but never below a UNC path's server and share; joined with `\`
/// and the root put back in front (or the working folder's drive).
fn canonicalize(path: &[char], cwd: &[char]) -> Result<Vec<char>, Exc> {
    let s: String = path.iter().collect();
    let path: Vec<char> = s.trim().chars().collect();
    if path.is_empty() {
        return Ok(path);
    }
    let root = mono_root(&path, cwd)?;
    let text: String = path.iter().collect();
    let mut dirs: Vec<String> = text.split(['\\', '/']).map(|d| d.trim_end().to_string()).collect();
    let unc = root.len() > 2 && mono_sep(root[0]) && mono_sep(root[1]);
    let limit = if unc { 3 } else { 0 };
    let mut t = 0;
    for i in 0..dirs.len() {
        if dirs[i] == "." || (i != 0 && dirs[i].is_empty()) {
            continue;
        } else if dirs[i] == ".." {
            if t > limit {
                t -= 1;
            }
        } else {
            dirs[t] = dirs[i].clone();
            t += 1;
        }
    }
    if t == 0 || (t == 1 && dirs[0].is_empty()) {
        return Ok(root);
    }
    let mut ret: Vec<char> = dirs[..t].join("\\").chars().collect();
    if unc {
        ret.insert(0, '\\');
    }
    if !same_root(&root, &ret) {
        ret = root.iter().chain(ret.iter()).copied().collect();
    }
    if unc {
        return Ok(ret);
    }
    if !mono_sep(path[0]) && same_root(&root, &path) {
        if ret.len() <= 2 && ret.last() != Some(&'\\') {
            ret.push('\\');
        }
        return Ok(ret);
    }
    if ret.is_empty() || mono_sep(ret[0]) {
        ret.push('\\');
    }
    Ok(cwd[..2].iter().chain(ret.iter()).copied().collect())
}
/// Path.InsecureGetFullPath (Windows), `cwd` the working folder with its drive.
fn mono_full_path(p: &str, cwd: &[char]) -> Result<String, Exc> {
    if p.trim().is_empty() {
        return Err(arg_exc(EMPTY_PATH));
    }
    let mut path = drive_adjust(p.chars().collect(), cwd)?;
    let end = *path.last().unwrap();
    if path.len() >= 2 && mono_sep(path[0]) && mono_sep(path[1]) {
        if path.len() == 2 || !path[2..].contains(&path[0]) {
            return Err(arg_exc("UNC paths should be of the form \\\\server\\share."));
        }
        if path[0] != '\\' {
            path = path.into_iter().map(|c| if c == '/' { '\\' } else { c }).collect();
        }
    } else if !mono_rooted(&path)? {
        let mut full = cwd.to_vec();
        if full.last() != Some(&'\\') {
            full.push('\\');
        }
        full.extend(path);
        path = full;
    } else if path.len() >= 2 && mono_sep(path[0]) && !mono_sep(path[1]) {
        path = cwd[..2].iter().chain(path.iter()).copied().collect();
    }
    let mut path = canonicalize(&path, cwd)?;
    if mono_sep(end) && path.last() != Some(&'\\') {
        path.push('\\');
    }
    Ok(path.into_iter().collect())
}
fn root_len(p: &str) -> usize {
    if p.len() >= 3 && p.as_bytes()[1] == b':' { 3 } else { 1 }
}

/// Path.GetDirectoryName (a path with a separator): the part before the last one, `/`-separated.
fn directory_name(p: &str) -> Option<String> {
    let i = p.rfind(['/', '\\'])?;
    Some(p[..i.max(root_len(p).min(i + 1) - 1).max(i)].replace('\\', "/"))
}

// ---- Json.NET for the legacy types ----

/// A ModMetaData from mod.json (JsonConvert.DeserializeObject<ModMetaData>): None for "null" or no value; a root that is
/// not an object, or a member that does not convert, is an error. [NonSerialized] fields and the [JsonIgnore] properties
/// are not read.
pub fn meta_from_json(text: &str) -> Result<Option<ModMetaData>, String> {
    let Some(root) = jsonnet::parse(text)?.filter(|r| *r != JTok::Null) else { return Ok(None) };
    if !matches!(root, JTok::Object(_)) {
        return Err("Cannot deserialize the JSON value into ModMetaData".into());
    }
    let mut m = ModMetaData::new();
    // Each property in file order sets its member (OrdinalIgnoreCase); unknown ones are skipped.
    let JTok::Object(props) = &root else { unreachable!() };
    let up = |c: char| {
        let mut u = c.to_uppercase();
        match (u.next(), u.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    };
    for (k, v) in props {
        let is = |n: &str| k.chars().map(up).eq(n.chars().map(up));
        if is("Name") {
            m.Name = v.as_string()?;
        } else if is("Author") {
            m.Author = v.as_string()?;
        } else if is("Description") {
            m.Description = v.as_string()?;
        } else if is("ModVersion") {
            m.ModVersion = v.as_string()?;
        } else if is("GameVersion") {
            m.GameVersion = v.as_string()?;
        } else if is("ThumbnailPath") {
            m.ThumbnailPath = v.as_string()?;
        } else if is("EntryPoint") {
            m.EntryPoint = v.as_string()?;
        } else if is("Tags") {
            m.Tags = v.as_string_array()?;
        } else if is("Scripts") {
            m.Scripts = v.as_string_array()?;
        } else if is("Active") {
            m.Active = v.as_bool()?;
        } else if is("UGCIdentity") {
            m.UGCIdentity = v.as_string()?;
        } else if is("CreatorUGCIdentity") {
            m.CreatorUGCIdentity = v.as_string()?;
        } else if is("AssemblyName") {
            m.AssemblyName = v.as_string()?;
        }
    }
    Ok(Some(m))
}

/// A Json.NET string literal (StringEscapeHandling.Default).
pub(crate) fn json_str(s: Option<&str>) -> String {
    let Some(s) = s else { return "null".into() };
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            '\0'..='\u{1f}' | '\u{85}' | '\u{2028}' | '\u{2029}' => o.push_str(&format!("\\u{:04x}", c as u32)),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

/// JsonConvert.SerializeObject(meta, Formatting.Indented): the serialised fields in declaration order, two-space indents,
/// "\r\n" line ends.
pub fn meta_to_json_indented(m: &ModMetaData) -> String {
    let arr = |a: &Option<Vec<Option<String>>>| match a {
        None => "null".to_string(),
        Some(v) if v.is_empty() => "[]".to_string(),
        Some(v) => format!("[\r\n{}\r\n  ]", v.iter().map(|x| format!("    {}", json_str(x.as_deref()))).collect::<Vec<_>>().join(",\r\n")),
    };
    let fields = [
        ("Name", json_str(m.Name.as_deref())),
        ("Author", json_str(m.Author.as_deref())),
        ("Description", json_str(m.Description.as_deref())),
        ("ModVersion", json_str(m.ModVersion.as_deref())),
        ("GameVersion", json_str(m.GameVersion.as_deref())),
        ("ThumbnailPath", json_str(m.ThumbnailPath.as_deref())),
        ("EntryPoint", json_str(m.EntryPoint.as_deref())),
        ("Tags", arr(&m.Tags)),
        ("Scripts", arr(&m.Scripts)),
        ("Active", (if m.Active { "true" } else { "false" }).to_string()),
        ("UGCIdentity", json_str(m.UGCIdentity.as_deref())),
        ("CreatorUGCIdentity", json_str(m.CreatorUGCIdentity.as_deref())),
        ("AssemblyName", json_str(m.AssemblyName.as_deref())),
    ];
    format!("{{\r\n{}\r\n}}", fields.iter().map(|(k, v)| format!("  \"{k}\": {v}")).collect::<Vec<_>>().join(",\r\n"))
}

/// CompilerConfig (ModModels).
#[derive(Clone, Debug, PartialEq)]
pub struct CompilerConfig {
    pub Hostname: Option<String>,
    pub Port: i32,
    pub ShutdownWhenGameNotFound: bool,
}

impl Default for CompilerConfig {
    /// CompilerConfig..ctor: 127.0.0.1:32513, ShutdownWhenGameNotFound.
    fn default() -> Self {
        CompilerConfig { Hostname: Some("127.0.0.1".into()), Port: 32513, ShutdownWhenGameNotFound: true }
    }
}

/// DeserializeObject<CompilerConfig>: None for "null" or no value.
pub fn config_from_json(text: &str) -> Result<Option<CompilerConfig>, String> {
    let Some(root) = jsonnet::parse(text)?.filter(|r| *r != JTok::Null) else { return Ok(None) };
    if !matches!(root, JTok::Object(_)) {
        return Err("Cannot deserialize the JSON value into CompilerConfig".into());
    }
    let mut c = CompilerConfig::default();
    let JTok::Object(props) = &root else { unreachable!() };
    for (k, v) in props {
        match k.to_ascii_uppercase().as_str() {
            "HOSTNAME" => c.Hostname = v.as_string()?,
            "PORT" => c.Port = v.as_i32()?,
            "SHUTDOWNWHENGAMENOTFOUND" => c.ShutdownWhenGameNotFound = v.as_bool()?,
            _ => {}
        }
    }
    Ok(Some(c))
}

/// CompilationState (ModModels): Unknown 0, Success 1, Error 2 (any int a reply sends).
pub type CompilationState = i32;
pub const STATE_SUCCESS: CompilationState = 1;
pub const STATE_ERROR: CompilationState = 2;
/// CompilationState's ToString: the name, else the number.
pub fn state_name(s: CompilationState) -> String {
    match s {
        0 => "Unknown".into(),
        1 => "Success".into(),
        2 => "Error".into(),
        n => n.to_string(),
    }
}

/// CompilerReply (ModModels, a struct).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompilerReply {
    pub ID: i32,
    pub State: CompilationState,
    pub Message: Option<String>,
    pub Suspicious: bool,
}

impl CompilerReply {
    fn error(message: impl Into<String>) -> CompilerReply {
        CompilerReply { State: STATE_ERROR, Message: Some(message.into()), ..Default::default() }
    }
}

/// DeserializeObject<CompilerReply>: a struct, so "null" or no value is an error too.
pub fn reply_from_json(text: &str) -> Result<CompilerReply, String> {
    let root = jsonnet::parse(text)?.ok_or("Cannot convert null to CompilerReply")?;
    if !matches!(root, JTok::Object(_)) {
        return Err("Cannot deserialize the JSON value into CompilerReply".into());
    }
    let mut r = CompilerReply::default();
    let JTok::Object(props) = &root else { unreachable!() };
    for (k, v) in props {
        match k.to_ascii_uppercase().as_str() {
            "ID" => r.ID = v.as_i32()?,
            "STATE" => r.State = v.as_enum(&["Unknown", "Success", "Error"])?,
            "MESSAGE" => r.Message = v.as_string()?,
            "SUSPICIOUS" => r.Suspicious = v.as_bool()?,
            _ => {}
        }
    }
    Ok(r)
}

/// ModCompileInstructions (ModModels, a struct).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModCompileInstructions {
    pub ID: i32,
    pub OutputFileName: Option<String>,
    pub Paths: Option<Arc<Vec<String>>>,
    pub AssemblyReferenceLocations: Option<Vec<String>>,
    pub MainClass: Option<String>,
    pub Directory: Option<String>,
    pub RejectShadyCode: bool,
    pub FromWorkshop: bool,
    pub InsertSourceB64: Option<String>,
    pub AssemblyName: Option<String>,
}

/// ModCompilationResult.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModCompilationResult {
    pub Success: bool,
    pub Errors: Option<String>,
    pub Suspicious: bool,
}

/// ModScript. LoadedAssembly: the loaded assembly's identity (none load in the rewrite).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModScript {
    pub RelativePath: Option<String>,
    pub AbsoluteScriptPaths: Option<Vec<String>>,
    pub AssemblyData: Option<Vec<u8>>,
    pub LoadedAssembly: Option<u64>,
}

/// A ModMetaData object: the lists and dictionaries hold references (identity, not value).
pub type ModRef = Arc<Mutex<ModMetaData>>;

/// Dictionary<K, V> with reference keys, in Dictionary's enumeration order: a removed entry's slot is reused by the next Add
/// (the most recently freed first).
#[derive(Clone, Debug)]
pub struct RefDict<V> {
    entries: Vec<Option<(ModRef, V)>>,
    free: Vec<usize>,
}

impl<V> Default for RefDict<V> {
    fn default() -> Self {
        RefDict { entries: Vec::new(), free: Vec::new() }
    }
}

impl<V> RefDict<V> {
    pub fn get(&self, k: &ModRef) -> Option<&V> {
        self.iter().find(|(e, _)| Arc::ptr_eq(e, k)).map(|(_, v)| v)
    }
    /// Add: a key already there throws ArgumentException.
    pub fn add(&mut self, k: ModRef, v: V) -> Result<(), Exc> {
        if self.get(&k).is_some() {
            return Err(Exc::new("ArgumentException", "An item with the same key has already been added."));
        }
        match self.free.pop() {
            Some(i) => self.entries[i] = Some((k, v)),
            None => self.entries.push(Some((k, v))),
        }
        Ok(())
    }
    pub fn remove(&mut self, k: &ModRef) -> bool {
        let Some(i) = self.entries.iter().position(|e| e.as_ref().is_some_and(|(e, _)| Arc::ptr_eq(e, k))) else { return false };
        self.entries[i] = None;
        self.free.push(i);
        true
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.free.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ModRef, &V)> {
        self.entries.iter().flatten().map(|(k, v)| (k, v))
    }
}

/// BackgroundItemLoaderStatusBehaviour.SetDisplayState from the legacy path (which runs off the main thread): the state
/// the status shows.
pub static DISPLAY_STATE: Mutex<String> = Mutex::new(String::new());
fn set_display_state(s: &str) {
    *DISPLAY_STATE.lock().unwrap() = s.to_string();
}

// ---- ModLoader ----

pub type Handler = Arc<dyn Fn() + Send + Sync>;

/// ModLoader's statics.
#[derive(Default)]
pub struct ModLoader {
    pub LoadedMods: Vec<ModRef>,
    pub ModScripts: RefDict<ModScript>,
    /// The thumbnails (ModLoader.LoadTexture's sprites).
    thumbnails: RefDict<Picture>,
    pub alreadyInitialisedMods: Vec<ModRef>,
    pub assemblyLocations: Vec<String>,
    ModListChanged: Vec<Handler>,
}

/// The game's ModLoader (its static constructor run against the working directory).
pub static MOD_LOADER: LazyLock<Mutex<ModLoader>> = LazyLock::new(|| Mutex::new(ModLoader::cctor(&std::env::current_dir().unwrap_or_default())));

/// The assemblies ModLoader..cctor lists for the compiler, as typeof(X).Assembly.Location gives them (`managed`, the
/// game's People Playground_Data/Managed), netstandard by Path.GetFullPath; Distinct.
pub fn assembly_locations(base: &Path, managed: &Path) -> Vec<String> {
    let m = |n: &str| format!("{}/{n}.dll", managed.to_string_lossy().replace('\\', "/"));
    let all = [
        get_full_path(base, crate::il!(str "ModLoader::.cctor" 0)).unwrap_or_default(),
        m("mscorlib"),
        m("UnityEngine.ParticleSystemModule"),
        m("UnityEngine.AudioModule"),
        m("UnityEngine.InputLegacyModule"),
        m("UnityEngine.CoreModule"),
        m("UnityEngine.TextRenderingModule"),
        m("Unity.TextMeshPro"),
        m("UnityEngine.UI"),
        m("UnityEngine.VideoModule"),
        m("UnityEngine.UIModule"),
        m("UnityEngine.Physics2DModule"),
        m("Newtonsoft.Json"),
        m("UnityEngine.PhysicsModule"),
        m("Ceras"),
        m("UnityEngine.CoreModule"),
        m("Assembly-CSharp"),
        m("UnityEngine.SpriteMaskModule"),
        m("UnityEngine.CoreModule"),
        m("mscorlib"),
        m("UnityEngine.AnimationModule"),
        m("System.Core"),
        m("System"),
        m("mscorlib"),
        m("UnityEngine.AssetBundleModule"),
        m("Unity.Postprocessing.Runtime"),
    ];
    let mut out: Vec<String> = Vec::new();
    for a in all {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// ModLoader.AlphaNumeric: Regex.Replace(a, "[^a-zA-Z0-9 -]", "").
pub fn AlphaNumeric(a: &str) -> String {
    a.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-').collect()
}

fn stem(m: &ModMetaData) -> Result<String, Exc> {
    match m.AssemblyName.as_deref() {
        Some(a) if !a.is_empty() => Ok(a.to_string()),
        _ => m.GetUniqueName().map_err(|_| Exc::nre()),
    }
}
pub(crate) fn path_exc(e: &'static str) -> Exc {
    match e {
        "ArgumentNullException" => Exc::new(e, "Value cannot be null.\r\nParameter name: path"),
        "ArgumentException" => Exc::new(e, "Illegal characters in path."),
        _ => Exc::new(e, "The given path's format is not supported."),
    }
}

/// ModLoader.GetOutputAssemblyPath: GetFullPath("CompiledMods/" + AlphaNumeric(AssemblyName, or GetUniqueName when empty) + ".dll").
pub fn GetOutputAssemblyPath(base: &Path, m: &ModMetaData) -> Result<String, Exc> {
    full_path(base, &format!("CompiledMods/{}.dll", AlphaNumeric(&stem(m)?)))
}

/// ModLoader.GetModHashPath: GetFullPath("CompiledMods/." + the same name + ".hash").
pub fn GetModHashPath(base: &Path, m: &ModMetaData) -> Result<String, Exc> {
    full_path(base, &format!("CompiledMods/.{}.hash", AlphaNumeric(&stem(m)?)))
}

/// ModLoader.GetModSourcePaths: each script's GetFullPath(Path.Combine(MetaLocation, script)); a null Scripts throws
/// NullReferenceException (when enumerated), Path.Combine's null part ArgumentNullException (path1 the location, path2 the
/// script).
pub fn GetModSourcePaths(base: &Path, m: &ModMetaData) -> Result<Vec<String>, Exc> {
    let scripts = m.Scripts.as_ref().ok_or_else(Exc::nre)?;
    let combine = |s: Option<&str>| {
        path_combine(m.MetaLocation.as_deref(), s).map_err(|e| match e {
            "ArgumentNullException" => Exc::new(e, format!("Value cannot be null.\r\nParameter name: path{}", if m.MetaLocation.is_none() { 1 } else { 2 })),
            _ => path_exc(e),
        })
    };
    scripts.iter().map(|s| full_path(base, &combine(s.as_deref())?)).collect()
}

/// Path.GetFullPath's ArgumentException for a blank path.
const EMPTY_PATH: &str = "The specified path is not of a legal form (empty).";

/// ModHasher.Compute(mod): the hash of MetaLocation's .cs / .dll files (a null MetaLocation throws ArgumentNullException).
fn hasher_compute(m: &ModMetaData) -> Result<u32, Exc> {
    let loc = m.MetaLocation.as_deref().ok_or_else(|| path_exc("ArgumentNullException"))?;
    super::mods::mod_hasher_compute(Path::new(loc)).map_err(|e| io_exc(&e, true))
}

/// ModLoader.GetModAssemblyMeta: BitConverter.GetBytes(ModHasher.Compute(mod)).
pub fn GetModAssemblyMeta(m: &ModMetaData) -> Result<Vec<u8>, Exc> {
    Ok(hasher_compute(m)?.to_le_bytes().to_vec())
}

/// ModLoader.ShouldRecompile: it had errors, its hash or output file is missing, it has no ContentHash, or the hash file's
/// first four bytes differ from it (a failure there: true).
pub fn ShouldRecompile(base: &Path, m: &ModMetaData) -> Result<bool, Exc> {
    if m.HasErrors {
        return Ok(true);
    }
    let hash = GetModHashPath(base, m)?;
    let out = GetOutputAssemblyPath(base, m)?;
    if !Path::new(&hash).is_file() || !Path::new(&out).is_file() {
        return Ok(true);
    }
    if m.ContentHash == 0 {
        return Ok(true);
    }
    match std::fs::read(&hash) {
        Ok(b) if b.len() >= 4 => Ok(m.ContentHash != u32::from_le_bytes(b[..4].try_into().unwrap())),
        _ => Ok(true),
    }
}

/// Directory.Delete(path, true) when Directory.Exists(path).
fn delete_dir_if_exists(p: &str) -> Result<(), Exc> {
    if Path::new(p).is_dir() {
        std::fs::remove_dir_all(p).map_err(|e| io_exc(&e, true))?;
    }
    Ok(())
}

impl ModLoader {
    /// ModLoader..cctor: empty lists, and the compiler's assembly list.
    pub fn cctor(base: &Path) -> ModLoader {
        ModLoader { assemblyLocations: assembly_locations(base, &base.join("People Playground_Data/Managed")), ..Default::default() }
    }

    /// ModLoader.add_ModListChanged
    pub fn add_ModListChanged(&mut self, h: Handler) {
        self.ModListChanged.push(h);
    }
    /// ModLoader.remove_ModListChanged: the last subscription of that handler.
    pub fn remove_ModListChanged(&mut self, h: &Handler) {
        if let Some(i) = self.ModListChanged.iter().rposition(|x| Arc::ptr_eq(x, h)) {
            self.ModListChanged.remove(i);
        }
    }
    /// ModLoader.InvokeModListChange: the handlers to call (in order), outside the lock.
    pub fn InvokeModListChange(&self) -> Vec<Handler> {
        self.ModListChanged.clone()
    }

    /// ModLoader.GetModAssembly: its script's LoadedAssembly, none when it has no script.
    pub fn GetModAssembly(&self, m: &ModRef) -> Option<u64> {
        self.ModScripts.get(m).and_then(|s| s.LoadedAssembly)
    }
    /// ModLoader.GetModMetaFromAssembly: the first script (Dictionary order) whose LoadedAssembly is `a` (a null one for None).
    pub fn GetModMetaFromAssembly(&self, a: Option<u64>) -> Option<ModRef> {
        self.ModScripts.iter().find(|(_, s)| s.LoadedAssembly == a).map(|(k, _)| k.clone())
    }
    /// ModLoader.RemoveMod: out of LoadedMods (the first) and ModScripts.
    pub fn RemoveMod(&mut self, m: &ModRef) {
        if let Some(i) = self.LoadedMods.iter().position(|x| Arc::ptr_eq(x, m)) {
            self.LoadedMods.remove(i);
        }
        self.ModScripts.remove(m);
    }

    /// ModLoader.LoadModAt: mod.json read (File.ReadAllText, DeserializeObject), MetaPath and MetaLocation set, a README.txt
    /// beside it of 5000 bytes or less as RichReadme (TextMeshPro's page/link/font/sprite/style tags opened as `<`),
    /// ContentHash computed, then added to LoadedMods. Any failure: None (nothing added).
    pub fn LoadModAt(&mut self, file_path: &str) -> Option<ModRef> {
        let load = || -> Result<ModMetaData, String> {
            let text = jsonnet::read_all_text(Path::new(file_path)).map_err(|e| e.to_string())?;
            let mut m = meta_from_json(&text)?.ok_or("NullReferenceException")?;
            m.MetaPath = Some(file_path.to_string());
            m.MetaLocation = directory_name(file_path);
            let readme = format!("{}/README.txt", m.MetaLocation.as_deref().unwrap_or(""));
            if Path::new(&readme).is_file() {
                let len = std::fs::metadata(&readme).map_err(|e| e.to_string())?.len();
                if len <= 5000 {
                    let mut t = jsonnet::read_all_text(Path::new(&readme)).map_err(|e| e.to_string())?;
                    for tag in ["<page", "<link", "<font", "<sprite", "<style"] {
                        t = t.replace(tag, "<");
                    }
                    m.RichReadme = Some(t);
                }
            }
            m.ContentHash = hasher_compute(&m).map_err(|e| e.message)?;
            Ok(m)
        };
        let m = Arc::new(Mutex::new(load().ok()?));
        self.LoadedMods.push(m.clone());
        Some(m)
    }

    /// ModLoader.GetThumbnail: the cached one, else Path.Combine(MetaLocation, ThumbnailPath) read by LoadTexture and cached
    /// (none when the file is missing, or on any exception there, which is logged: a bad path, an unreadable file, a picture
    /// Texture2D.LoadImage refuses).
    pub fn GetThumbnail(&mut self, m: &ModRef) -> Option<Picture> {
        if let Some(t) = self.thumbnails.get(m) {
            return Some(t.clone());
        }
        let (loc, thumb) = {
            let g = m.lock().unwrap();
            (g.MetaLocation.clone(), g.ThumbnailPath.clone())
        };
        let p = path_combine(loc.as_deref(), thumb.as_deref()).ok()?;
        if !Path::new(&p).is_file() {
            return None;
        }
        let pic = Arc::new(LoadTexture(Path::new(&p), false).ok()?);
        self.thumbnails.add(m.clone(), pic.clone()).ok()?;
        Some(pic)
    }
}

/// A loaded mod picture (a Sprite over its Texture2D).
pub type Picture = Arc<super::contraption::LoadedSprite>;

/// ModLoader.LoadTexture(fullPath, mode = Bilinear): File.ReadAllBytes (its exception), new Texture2D(0, 0).LoadImage false
/// throws InvalidDataException, filterMode `mode`, wrapMode Clamp, then Sprite.Create over the whole texture, pivot (0.5, 0.5),
/// 256 pixels per unit.
pub fn LoadTexture(full_path: &Path, point_filter: bool) -> Result<super::contraption::LoadedSprite, Exc> {
    let data = std::fs::read(full_path).map_err(|e| io_exc(&e, !full_path.parent().is_some_and(Path::is_dir)))?;
    let image = super::contraption::decode_png(&data).map_err(|_| Exc::new("InvalidDataException", format!("Texture at {} cannot be loaded as a PNG", full_path.display())))?;
    let rect = [0.0, 0.0, image.width as f32, image.height as f32];
    Ok(super::contraption::LoadedSprite { texture: super::contraption::LoadedTexture { image, point_filter }, rect, pivot: [0.5, 0.5], pixels_per_unit: 256.0 })
}

/// ModLoader.SetModActive: TrustCache.VerifyOrigin (game code only calls it), a null MetaLocation throws ArgumentException,
/// Active set, OnLoad invoked when active, then UpdateJSON.
pub fn SetModActive(loader: &Mutex<ModLoader>, m: &ModRef, active: bool) -> Result<(), Exc> {
    TrustCache::VerifyOrigin(false)?;
    {
        let mut g = m.lock().unwrap();
        if g.MetaLocation.is_none() {
            return Err(Exc::new("ArgumentException", format!("{} is unloaded", g.Name.as_deref().unwrap_or(""))));
        }
        g.Active = active;
    }
    if active {
        ModificationManager::InvokeOnLoad_mod(&mut loader.lock().unwrap(), m);
    }
    UpdateJSON(&m.lock().unwrap())
}

/// ModLoader.UpdateJSON: SerializeObject(Indented) written to MetaPath (File.WriteAllText, UTF-8 without a BOM).
pub fn UpdateJSON(m: &ModMetaData) -> Result<(), Exc> {
    let text = meta_to_json_indented(m);
    let p = m.MetaPath.as_deref().ok_or_else(|| path_exc("ArgumentNullException"))?;
    if p.trim().is_empty() || bad_path_char(p) {
        return Err(path_exc("ArgumentException"));
    }
    std::fs::write(p, text).map_err(|e| io_exc(&e, !Path::new(p).parent().is_some_and(Path::is_dir)))
}

/// ModLoader.Compile (`reject_shady`: UserPreferenceManager.Current.RejectShadyCode): the compiled assembly read back when it need not be recompiled (a failure there falls through), else
/// TryCompileMod, HasErrors = !Success.
pub fn Compile(base: &Path, m: &ModRef, reject_shady: bool) -> Result<(ModCompilationResult, ModScript), Exc> {
    let snapshot = m.lock().unwrap().clone();
    if !ShouldRecompile(base, &snapshot)? {
        set_display_state("Reading");
        let out = GetOutputAssemblyPath(base, &snapshot)?;
        let read = || -> Result<ModScript, Exc> {
            let data = std::fs::read(&out).map_err(|e| io_exc(&e, false))?;
            Ok(ModScript { AssemblyData: Some(data), AbsoluteScriptPaths: Some(GetModSourcePaths(base, &snapshot)?), RelativePath: snapshot.MetaPath.clone(), LoadedAssembly: None })
        };
        if let Ok(s) = read() {
            return Ok((ModCompilationResult { Success: true, Errors: None, Suspicious: false }, s));
        }
    }
    let (r, s) = TryCompileMod(base, m, reject_shady)?;
    m.lock().unwrap().HasErrors = !r.Success;
    Ok((r, s))
}

/// What TryCompileMod catches, with the stack trace Mono gives it in the game (Exception.ToString is the mod's error
/// text): GetModSourcePaths' exceptions (thrown in its iterator, run by ToArray), new DirectoryInfo(MetaLocation)'s,
/// Directory.CreateDirectory("CompiledMods")'s over a file of that name and File.Delete's on a read-only output. Measured on
/// the game's Mono with its own ModLoader (1.28.41 and 1.28.54: the same frames, each with its assembly's Mvid; the test's
/// texts).
fn tcm_frame(il: u32) -> String {
    frame("Assembly-CSharp", "ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript)", il)
}
fn source_paths_trace(e: Exc) -> Exc {
    let (thrower, il) = match (e.class, e.message.as_str()) {
        ("NullReferenceException", _) => (vec![], 0x6f),
        ("ArgumentNullException", m) => (vec![frame("mscorlib", "System.IO.Path.Combine (System.String path1, System.String path2)", if m.ends_with("path1") { 0x03 } else { 0x11 })], 0x33),
        ("ArgumentException", EMPTY_PATH) => (vec![frame("mscorlib", "System.IO.Path.InsecureGetFullPath (System.String path)", 0x25), frame("mscorlib", "System.IO.Path.GetFullPath (System.String path)", 0)], 0x45),
        ("ArgumentException", "Illegal characters in path.") => (vec![frame("mscorlib", "System.IO.Path.Combine (System.String path1, System.String path2)", 0x57)], 0x33),
        _ => return e,
    };
    let mut t = thrower;
    t.push(frame("Assembly-CSharp", "ModLoader+<GetModSourcePaths>d__25.MoveNext ()", il));
    t.push(frame("System.Core", "System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items)", 0x5e));
    t.push(frame("System.Core", "System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source)", 0x33));
    t.push(frame("System.Core", "System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source)", 0x18));
    t.push(tcm_frame(0x26));
    e.at(t)
}
/// new DirectoryInfo(p).FullName: DirectoryInfo.CheckPath (null, empty, an invalid char, a ':' first found past index 1),
/// then GetFullPath.
fn directory_info_full_name(base: &Path, p: Option<&str>) -> Result<String, Exc> {
    let ctor = |mut t: Vec<String>| {
        t.push(frame("mscorlib", "System.IO.DirectoryInfo..ctor (System.String path)", 0));
        t.push("  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)".into());
        t.push(tcm_frame(0xde));
        t
    };
    let check = |class, message: &str, il| {
        Err(Exc::new(class, message).at(ctor(vec![
            frame("mscorlib", "System.IO.DirectoryInfo.CheckPath (System.String path)", il),
            frame("mscorlib", "System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath)", 6),
        ])))
    };
    let Some(p) = p else { return check("ArgumentNullException", "Value cannot be null.\r\nParameter name: path", 0x03) };
    if p.is_empty() {
        return check("ArgumentException", "An empty file name is not valid.", 0x16);
    }
    if bad_path_char(p) {
        return check("ArgumentException", "Illegal characters in path.", 0x2f);
    }
    if p.chars().position(|c| c == ':').is_some_and(|i| i != 1) {
        return check("ArgumentException", "path", 0x52);
    }
    full_path(base, p).map_err(|e| match e.message.as_str() {
        EMPTY_PATH => e.at(ctor(vec![
            frame("mscorlib", "System.IO.Path.InsecureGetFullPath (System.String path)", 0x25),
            frame("mscorlib", "System.IO.Path.GetFullPath (System.String path)", 0),
            frame("mscorlib", "System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath)", 0x0d),
        ])),
        _ => e,
    })
}

/// ModLoader.TryCompileMod: a workshop mod's bin/ and obj/ deleted ("Workshop is disabled"); a local one sent to the
/// compiler (CompilerClient) and its reply read. A failure deletes the hash file and returns its text.
fn TryCompileMod(base: &Path, m: &ModRef, reject_shady: bool) -> Result<(ModCompilationResult, ModScript), Exc> {
    set_display_state("Preparing");
    let mut script = ModScript::default();
    let meta = m.lock().unwrap().clone();
    let hash_path = GetModHashPath(base, &meta)?;
    let fail_text = |e: Exc| {
        if Path::new(&hash_path).is_file() {
            let _ = std::fs::remove_file(&hash_path);
        }
        ModCompilationResult { Success: false, Errors: Some(e.to_text()), Suspicious: false }
    };
    let mut body = || -> Result<ModCompilationResult, Exc> {
        script.AbsoluteScriptPaths = Some(GetModSourcePaths(base, &meta).map_err(source_paths_trace)?);
        script.RelativePath = meta.MetaLocation.clone();
        if meta.FromWorkshop {
            let loc = meta.MetaLocation.clone().unwrap_or_default();
            delete_dir_if_exists(&format!("{loc}/bin/"))?;
            delete_dir_if_exists(&format!("{loc}/obj/"))?;
            return Ok(ModCompilationResult { Success: false, Errors: Some(crate::il!(str "ModLoader::TryCompileMod" 3).into()), Suspicious: false });
        }
        let output = GetOutputAssemblyPath(base, &meta)?;
        let instructions = ModCompileInstructions {
            MainClass: meta.EntryPoint.clone(),
            Paths: script.AbsoluteScriptPaths.clone().map(Arc::new),
            FromWorkshop: meta.FromWorkshop,
            OutputFileName: Some(output.clone()),
            RejectShadyCode: !TrustCache::IsTrusted(&meta) && reject_shady,
            AssemblyReferenceLocations: Some(MOD_LOADER.lock().unwrap().assemblyLocations.clone()),
            Directory: Some(directory_info_full_name(base, meta.MetaLocation.as_deref())?),
            AssemblyName: meta.AssemblyName.clone(),
            ..Default::default()
        };
        let compiled = base.join("CompiledMods");
        if compiled.is_file() {
            let trace = vec![frame("mscorlib", "System.IO.Directory.CreateDirectory (System.String path)", 0x6a), tcm_frame(0x13c)];
            return Err(Exc::new("IOException", "Cannot create CompiledMods because a file with the same name already exists.").at(trace));
        }
        if !compiled.is_dir() {
            std::fs::create_dir_all(&compiled).map_err(|e| io_exc(&e, true))?;
        }
        if Path::new(&output).is_file() {
            // File.Delete refuses a read-only file (DeleteFile does; Rust's remove_file would clear the attribute).
            let denied = || {
                Exc::new("UnauthorizedAccessException", format!("Access to the path \"{}\" is denied.", output.replace('/', "\\")))
                    .at(vec![frame("mscorlib", "System.IO.File.Delete (System.String path)", 0x73), tcm_frame(0x159)])
            };
            if std::fs::metadata(&output).is_ok_and(|m| m.permissions().readonly()) {
                return Err(denied());
            }
            std::fs::remove_file(&output).map_err(|e| match e.kind() {
                std::io::ErrorKind::PermissionDenied => denied(),
                _ => io_exc(&e, false),
            })?;
        }
        set_display_state(crate::il!(str "ModLoader::TryCompileMod" 6));
        let reply = COMPILER_CLIENT.lock().unwrap().RequestCompilationSynchronous(base, instructions);
        set_display_state("Reading");
        std::thread::sleep(Duration::from_millis(16));
        match reply.State {
            STATE_ERROR => {
                if Path::new(&hash_path).is_file() {
                    std::fs::remove_file(&hash_path).map_err(|e| io_exc(&e, false))?;
                }
                Ok(ModCompilationResult { Success: false, Errors: reply.Message, Suspicious: reply.Suspicious })
            }
            STATE_SUCCESS => {
                let r = std::fs::read(&output).map_err(|e| io_exc(&e, false)).and_then(|d| {
                    script.AssemblyData = Some(d);
                    std::fs::write(&hash_path, hasher_compute(&meta)?.to_le_bytes()).map_err(|e| io_exc(&e, true))
                });
                if let Err(e) = r {
                    if Path::new(&hash_path).is_file() {
                        let _ = std::fs::remove_file(&hash_path);
                    }
                    return Ok(ModCompilationResult { Success: false, Errors: Some(e.to_text()), Suspicious: reply.Suspicious });
                }
                UpdateJSON(&meta)?;
                Ok(ModCompilationResult { Success: true, Errors: None, Suspicious: false })
            }
            _ => Ok(ModCompilationResult { Success: false, Errors: Some(crate::il!(str "ModLoader::TryCompileMod" 9).into()), Suspicious: reply.Suspicious }),
        }
    };
    let r = body().unwrap_or_else(fail_text);
    Ok((r, script))
}

// ---- ModificationManager ----

/// ModificationManager's statics. Modifications: Dictionary<SpawnableModification, ModMetaData>; the type lists hold the
/// mods' MonoBehaviour types (none load in the rewrite).
#[derive(Default)]
pub struct ModificationManager {
    /// By the modification's identity (the mod's SpawnableModification object), in insertion order.
    pub Modifications: Vec<(usize, Option<ModRef>)>,
    pub BackgroundScripts: Vec<String>,
    pub CustomTools: Vec<String>,
    pub CustomPowers: Vec<String>,
}

pub static MODIFICATION_MANAGER: LazyLock<Mutex<ModificationManager>> = LazyLock::new(Default::default);

impl ModificationManager {
    /// ModificationManager.Invoke: always false (1.28 loads no C# mod assemblies).
    pub fn Invoke(_method: &str, _m: &ModRef) -> bool {
        false
    }
    /// ModificationManager.InvokeOnLoad(meta): zero when already initialised, else OnLoad invoked (initialised when it ran)
    /// and the time it took.
    pub fn InvokeOnLoad_mod(loader: &mut ModLoader, m: &ModRef) -> Duration {
        if loader.alreadyInitialisedMods.iter().any(|x| Arc::ptr_eq(x, m)) {
            return Duration::ZERO;
        }
        let t = std::time::Instant::now();
        if Self::Invoke("OnLoad", m) {
            loader.alreadyInitialisedMods.push(m.clone());
        }
        t.elapsed()
    }
    /// ModificationManager.InvokeOnUnload(meta): zero when not initialised, else OnUnload invoked (no longer initialised
    /// when it ran) and the time it took.
    pub fn InvokeOnUnload(loader: &mut ModLoader, m: &ModRef) -> Duration {
        let Some(i) = loader.alreadyInitialisedMods.iter().position(|x| Arc::ptr_eq(x, m)) else { return Duration::ZERO };
        let t = std::time::Instant::now();
        if Self::Invoke("OnUnload", m) {
            loader.alreadyInitialisedMods.remove(i);
        }
        t.elapsed()
    }
    /// ModificationManager.InvokeOnLoad(): OnLoad for each active loaded mod; a total of 500 ms or more gives the freeze
    /// dialog's text when ShowModLoadingFreeze (`show_freeze`) is on (the mods over 100 ms, slowest first, up to 4).
    pub fn InvokeOnLoad(loader: &mut ModLoader, show_freeze: bool) -> Option<String> {
        let mut total = Duration::ZERO;
        let mut list = Vec::new();
        for m in loader.LoadedMods.clone() {
            if m.lock().unwrap().Active {
                let t = Self::InvokeOnLoad_mod(loader, &m);
                list.push((m, t));
                total += t;
            }
        }
        let ms = total.as_secs_f64() * 1000.0;
        if !(ms >= 500.0) || !show_freeze {
            return None;
        }
        let rounded = (ms as f32).round_ties_even() as i32;
        let mut slow: Vec<_> = list.into_iter().filter(|(_, t)| t.as_secs_f64() * 1000.0 > 100.0).collect();
        slow.sort_by(|a, b| b.1.cmp(&a.1));
        Some(if slow.is_empty() {
            format!("Some mods caused a {rounded}ms freeze!\nConsider disabling some mods for faster load times.")
        } else {
            let names: Vec<String> = slow.iter().take(4).map(|(m, _)| m.lock().unwrap().Name.clone().unwrap_or_default()).collect();
            format!("Some mods caused a {rounded}ms freeze!\nConsider disabling these mods for faster load times:\n\n{}", names.join("\n"))
        })
    }
    /// ModificationManager.InvokeMain: ModAPI.ClearEvents, BackgroundScripts and Modifications cleared, Main for each loaded mod.
    pub fn InvokeMain(&mut self, loader: &ModLoader) {
        super::mods::mod_api::ClearEvents();
        self.BackgroundScripts.clear();
        self.Modifications.clear();
        for m in &loader.LoadedMods {
            Self::Invoke("Main", m);
        }
    }
}

// ---- TrustCache ----

/// TrustCache's statics: the trusted content hashes and the cache file's name.
pub struct TrustCache {
    /// HashSet<uint> in its enumeration order: slots, a removed one reused by the next Add (the latest freed first).
    trusted: Vec<Option<u32>>,
    free: Vec<usize>,
    path: String,
}

pub static TRUST_CACHE: LazyLock<Mutex<TrustCache>> = LazyLock::new(|| Mutex::new(TrustCache::cctor()));

impl TrustCache {
    /// TrustCache..cctor: the file name is Hash128.Compute(MachineName + UserName + UserDomainName + OSVersion + ProcessorCount).
    pub fn cctor() -> TrustCache {
        Self::with_key(&machine_key(&os_version()))
    }
    /// The cache for a machine key (the string the file name hashes).
    pub fn with_key(key: &str) -> TrustCache {
        TrustCache { trusted: Vec::new(), free: Vec::new(), path: super::stats::hash128_to_string(super::stats::spooky_hash128(key.as_bytes())) }
    }
    /// The cache over the file name `path` (as its static constructor leaves it, empty).
    pub fn with_path(path: &str) -> TrustCache {
        TrustCache { trusted: Vec::new(), free: Vec::new(), path: path.to_string() }
    }
    /// The cache file's name.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// The trusted hashes in enumeration order.
    pub fn trusted(&self) -> Vec<u32> {
        self.trusted.iter().flatten().copied().collect()
    }
    fn add(&mut self, h: u32) {
        if self.trusted.contains(&Some(h)) {
            return;
        }
        match self.free.pop() {
            Some(i) => self.trusted[i] = Some(h),
            None => self.trusted.push(Some(h)),
        }
    }
    /// TrustCache.VerifyOrigin: every caller in the stack is the game's or the engine's (the rewrite has no other code), and a
    /// UI call comes from the engine: never throws "Shady stack trace :S" here.
    pub fn VerifyOrigin(_is_ui: bool) -> Result<(), Exc> {
        Ok(())
    }
    /// TrustCache.IsTrusted: its ContentHash (not 0) is trusted.
    pub fn IsTrusted(m: &ModMetaData) -> bool {
        m.ContentHash != 0 && TRUST_CACHE.lock().unwrap().trusted.contains(&Some(m.ContentHash))
    }
    /// TrustCache.Trust: a ContentHash of 0 throws.
    pub fn Trust(&mut self, m: &ModMetaData) -> Result<(), Exc> {
        Self::VerifyOrigin(false)?;
        if m.ContentHash == 0 {
            return Err(Exc::new("Exception", crate::il!(str "TrustCache::Trust" 0)));
        }
        self.add(m.ContentHash);
        Ok(())
    }
    /// TrustCache.Distrust: a ContentHash of 0 throws; GetUniqueName runs (its NullReferenceException too).
    pub fn Distrust(&mut self, m: &ModMetaData) -> Result<(), Exc> {
        Self::VerifyOrigin(false)?;
        if m.ContentHash == 0 {
            return Err(Exc::new("Exception", crate::il!(str "TrustCache::Distrust" 0)));
        }
        m.GetUniqueName().map_err(|_| Exc::nre())?;
        if let Some(i) = self.trusted.iter().position(|h| *h == Some(m.ContentHash)) {
            self.trusted[i] = None;
            self.free.push(i);
        }
        Ok(())
    }
    /// TrustCache.Load: the file (in the working directory) read as Ceras' HashSet<uint>, replacing the set; a missing
    /// file or a bad one leaves it (logged).
    pub fn Load(&mut self, base: &Path) -> Result<(), Exc> {
        Self::VerifyOrigin(false)?;
        // Deserialize, then Clear and AddRange: a throw while reading leaves the set; a null set is cleared and AddRange(null)
        // throws (logged).
        if let Ok(b) = std::fs::read(base.join(&self.path)) {
            if let Ok(set) = decode_uint_hashset(&b) {
                self.trusted.clear();
                self.free.clear();
                for h in set.unwrap_or_default() {
                    self.add(h);
                }
            }
        }
        Ok(())
    }
    /// TrustCache.Save: the set written as Ceras' HashSet<uint> into `base`, the rewrite's own data folder (never the game's).
    pub fn Save(&self, base: &Path) -> Result<(), Exc> {
        Self::VerifyOrigin(false)?;
        std::fs::write(base.join(&self.path), encode_uint_hashset(&self.trusted())).map_err(|e| io_exc(&e, true))
    }
}

/// TrustCache's static constructor's key: MachineName + UserName + UserDomainName + Environment.OSVersion + ProcessorCount,
/// with `os` as the OSVersion string.
fn machine_key(os: &str) -> String {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    // Mono off Windows: MachineName is gethostname's (uname's node name), UserName USERNAME else USER.
    let machine = if cfg!(windows) { env("COMPUTERNAME") } else { uname("-n") };
    let user = std::env::var("USERNAME").or_else(|e| if cfg!(windows) { Err(e) } else { std::env::var("USER") }).unwrap_or_default();
    let domain = std::env::var("USERDOMAIN").unwrap_or_else(|_| machine.clone());
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    format!("{machine}{user}{domain}{os}{cpus}")
}

/// Environment.OSVersion.ToString() on Windows: "Microsoft Windows NT major.minor.build.0" (the Unity player declares
/// Windows 10 support, so GetVersionEx reports the real version: read here from the registry).
/// Elsewhere (the game ships for Windows only, so no game build is there to match; unchecked) Mono's Unix form:
/// "Unix " + Environment.CreateVersionFromString(uname's release).
// Checked: the Windows key hashes to the trust file the real game wrote on the developer's PC (the test below).
fn os_version() -> String {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
        let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let key = w("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let dword = |name: &str| {
            let mut v = 0u32;
            let mut n = 4u32;
            (RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), w(name).as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as _, &mut n) == 0).then_some(v)
        };
        let mut buf = [0u16; 32];
        let mut n = 64u32;
        let build = if RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), w("CurrentBuildNumber").as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as _, &mut n) == 0 {
            String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
        } else {
            "0".into()
        };
        format!("Microsoft Windows NT {}.{}.{build}.0", dword("CurrentMajorVersionNumber").unwrap_or(6), dword("CurrentMinorVersionNumber").unwrap_or(2))
    }
    #[cfg(not(windows))]
    format!("Unix {}", create_version_from_string(&uname("-r")))
}

/// uname's node name (`-n`) or release (`-r`), empty when it cannot run.
#[cfg_attr(windows, allow(dead_code))]
fn uname(flag: &str) -> String {
    std::process::Command::new("uname").arg(flag).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Environment.CreateVersionFromString (Mono): up to four digit runs, each ended by any other char, as
/// major.minor.build.revision (missing ones 0), then Version.ToString.
#[cfg_attr(windows, allow(dead_code))]
fn create_version_from_string(info: &str) -> String {
    let mut parts = [0i64; 4];
    let mut state = 0;
    let mut number: Option<i64> = None;
    for c in info.chars() {
        if state == 4 {
            break;
        }
        if let Some(d) = c.to_digit(10) {
            number = Some(number.unwrap_or(0) * 10 + d as i64);
        } else if let Some(n) = number.take() {
            parts[state] = n;
            state += 1;
        }
    }
    if let (Some(n), true) = (number, state < 4) {
        parts[state] = n;
    }
    format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3])
}

/// CerasSerializer.Serialize(HashSet<uint>) with the default config: the root reference (-3, a new value of the static
/// type, biased by 5), the count and each item as VarInts (UInt32Formatter, CollectionFormatter).
pub fn encode_uint_hashset(items: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    let mut varint = |mut v: u64| loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            b.push(byte);
            break;
        }
        b.push(byte | 0x80);
    };
    varint(2);
    varint(items.len() as u64);
    for &h in items {
        varint(h as u64);
    }
    b
}

/// CerasSerializer.Deserialize<HashSet<uint>> (no checksum, references on): the root reference is a VarInt biased by 5:
/// -3 a new HashSet<uint> (its count, then each item, a HashSet so repeats once), -1 null (Ok(None)); -2 a type
/// (TypeFormatter) then the new value, read as a HashSet<uint> when the type is that and thrown otherwise; -5 a type as the
/// value, null for a null type and an InvalidCastException otherwise. -4 an external object (no resolver) and an existing
/// object id (none is known) throw. Bytes after the set are ignored. Checked against the game's own Ceras.dll under its
/// Mono (the test's bytes).
pub fn decode_uint_hashset(d: &[u8]) -> Result<Option<Vec<u32>>, String> {
    let mut o = 0;
    // SerializerBinary.ReadVarInt(bits 32): a 6th byte (shift 35) is malformed; reading past the end throws.
    let varint = |o: &mut usize| -> Result<u64, String> {
        let (mut r, mut shift) = (0u64, 0u32);
        loop {
            let b = *d.get(*o).ok_or("IndexOutOfRangeException")? as u64;
            *o += 1;
            r |= (b & 0x7f) << shift;
            if shift > 32 {
                return Err("ArgumentOutOfRangeException: Malformed VarInt".into());
            }
            if b & 0x80 == 0 {
                return Ok(r);
            }
            shift += 7;
        }
    };
    let bias = |o: &mut usize, b: i32| varint(o).map(|v| (v as u32 as i32).wrapping_sub(b));
    // TypeFormatter.Deserialize, as far as it matters here: whether the type is HashSet<uint> (Some(true)), another one
    // (Some(false); one that does not resolve throws, the same outcome) or null (None). The type cache starts empty for each
    // Deserialize: the first type read can name no cached id, and a cached id read later is the generic definition, never
    // UInt32, so an id never makes HashSet<uint>.
    let name = |o: &mut usize| -> Result<String, String> {
        let n = bias(o, 1)?;
        let n = usize::try_from(n).map_err(|_| "ArgumentNullException")?;
        let s = d.get(*o..*o + n).ok_or("ArgumentOutOfRangeException")?;
        *o += n;
        Ok(String::from_utf8_lossy(s).into_owned())
    };
    let read_type = |o: &mut usize| -> Result<Option<bool>, String> {
        match bias(o, 3)? {
            -1 => Ok(None),
            // A single name: Assembly.GetType's (Mono's) reading of it.
            -3 => Ok(Some(names_hashset_uint(&name(o)?))),
            // A generic one: its definition (a single name: cached ids and nested generics never make the definition), its
            // argument count byte, the arguments, then MakeGenericType.
            -2 => {
                if bias(o, 3)? != -3 {
                    return Err("Ceras: no generic definition".into());
                }
                let def = name(o)?;
                let n = *d.get(*o).ok_or("IndexOutOfRangeException")?;
                *o += 1;
                if n != 1 || bias(o, 3)? != -3 {
                    return Err("Ceras: not one named argument".into());
                }
                let arg = name(o)?;
                Ok(Some(def.trim_start_matches(' ') == "System.Collections.Generic.HashSet`1" && arg.trim_start_matches(' ') == "System.UInt32"))
            }
            _ => Err("Ceras: no such cached type".into()),
        }
    };
    match bias(&mut o, 5)? {
        -3 => {}
        -1 => return Ok(None),
        // `value = (T)(object)type`: null casts, a type never does.
        -5 => return if read_type(&mut o)?.is_none() { Ok(None) } else { Err("InvalidCastException".into()) },
        -2 => {
            if read_type(&mut o)? != Some(true) {
                return Err("Ceras: the value's type is not HashSet<uint>".into());
            }
        }
        _ => return Err("Ceras: no such object".into()),
    }
    let n = varint(&mut o)? as u32;
    let mut items: Vec<u32> = Vec::new();
    for _ in 0..n {
        let v = varint(&mut o)? as u32;
        if !items.contains(&v) {
            items.push(v);
        }
    }
    Ok(Some(items))
}

/// Whether Mono's Assembly.GetType (searched over the loaded assemblies, as SimpleTypeBinder does) gives HashSet<uint> for
/// `name`: leading spaces skipped before each type name; the argument bare or `[name, assembly]` with mscorlib or
/// mscorlib.dll (any case, spaces around it, any further assembly properties, not an empty last one). Measured on the
/// game's Mono (the test's names): a trailing space, a tab or a space before the comma does not resolve.
fn names_hashset_uint(name: &str) -> bool {
    let Some(arg) = name.trim_start_matches(' ').strip_prefix("System.Collections.Generic.HashSet`1[").and_then(|r| r.strip_suffix(']')) else { return false };
    let arg = arg.trim_start_matches(' ');
    if arg == "System.UInt32" {
        return true;
    }
    let Some((t, asm)) = arg.strip_prefix('[').and_then(|r| r.strip_suffix(']')).and_then(|r| r.split_once(',')) else { return false };
    let first = asm.split(',').next().unwrap().trim().to_ascii_lowercase();
    t.trim_start_matches(' ') == "System.UInt32" && (first == "mscorlib" || first == "mscorlib.dll") && !asm.ends_with(',')
}

// ---- the compiler links ----

/// Process.GetProcessesByName("PPGModCompiler") each Kill()ed.
fn kill_all_server_processes() -> Result<(), Exc> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Ok(());
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut pids = Vec::new();
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let n = String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len())]);
            if n.eq_ignore_ascii_case("PPGModCompiler.exe") {
                pids.push(e.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
        for pid in pids {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() || TerminateProcess(h, u32::MAX) == 0 {
                return Err(Exc::new("Win32Exception", win32_message(5)));
            }
            CloseHandle(h);
        }
    }
    Ok(())
}

/// Process.Start(startInfo): `file` found (a bare name as ShellExecute looks for one: the working directory, then the
/// game's, then PATH) and started in `dir` with `arg`; missing, Win32Exception 2; a missing working directory, 267.
fn start_process(base: &Path, file: &str, dir: &str, arg: &str, shell: bool) -> Result<std::process::Child, Exc> {
    let found = if Path::new(file).is_absolute() {
        Some(PathBuf::from(file)).filter(|p| p.is_file())
    } else {
        let mut look = vec![PathBuf::from(dir), base.to_path_buf()];
        look.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        look.into_iter().map(|d| d.join(file)).find(|p| p.is_file())
    };
    let Some(exe) = found else { return Err(Exc::new("Win32Exception", win32_message(2))) };
    if !shell && !Path::new(dir).is_dir() {
        return Err(Exc::new("Win32Exception", win32_message(267)));
    }
    std::process::Command::new(exe).arg(arg).current_dir(dir).spawn().map_err(|e| Exc::new("Win32Exception", win32_message(e.raw_os_error().unwrap_or(2))))
}

/// WatsonTcpClient as ModCompilerSockets drives it.
#[derive(Debug)]
pub struct WatsonClient {
    host: String,
    port: i32,
    Connected: bool,
    /// Dispose nulled its settings: Connect then throws NullReferenceException.
    disposed: bool,
}

impl WatsonClient {
    /// WatsonTcpClient(serverIp, serverPort): a null or empty host throws ArgumentNullException, a negative port
    /// ArgumentOutOfRangeException.
    pub fn new(host: Option<&str>, port: i32) -> Result<WatsonClient, Exc> {
        let host = host.filter(|h| !h.is_empty()).ok_or_else(|| Exc::new("ArgumentNullException", "Value cannot be null.\r\nParameter name: serverIp"))?;
        if port < 0 {
            return Err(Exc::new("ArgumentOutOfRangeException", "Specified argument was out of the range of valid values.\r\nParameter name: serverPort"));
        }
        Ok(WatsonClient { host: host.to_string(), port, Connected: false, disposed: false })
    }
    /// WatsonTcpClient.Connect: TcpClient.BeginConnect(host, port) waited on for ConnectTimeoutSeconds (5); a refusal or a
    /// failed lookup throws its SocketException, the timeout TimeoutException.
    pub fn Connect(&mut self) -> Result<(), Exc> {
        if self.Connected {
            return Err(Exc::new("InvalidOperationException", "Already connected to the server."));
        }
        if self.disposed {
            return Err(Exc::nre());
        }
        if self.port > 65535 {
            return Err(Exc::new("ArgumentOutOfRangeException", "Specified argument was out of the range of valid values.\r\nParameter name: port"));
        }
        use std::net::ToSocketAddrs;
        let addrs: Vec<_> = (self.host.as_str(), self.port as u16).to_socket_addrs().map_err(|_| Exc::new("SocketException", win32_message(11001)))?.collect();
        let start = std::time::Instant::now();
        let mut last = Exc::new("SocketException", win32_message(11001));
        for a in addrs {
            let left = Duration::from_secs(5).saturating_sub(start.elapsed());
            if left.is_zero() {
                break;
            }
            match std::net::TcpStream::connect_timeout(&a, left) {
                Ok(_) => unimplemented!("mod-only: talking WatsonTcp to a PPGModCompiler server (1.28 ships none)"),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Err(Exc::new("TimeoutException", format!("Timeout connecting to {}:{}", self.host, self.port))),
                Err(e) => last = Exc::new("SocketException", win32_message(if e.kind() == std::io::ErrorKind::ConnectionRefused { 10061 } else { 10060 })),
            }
        }
        Err(last)
    }
    /// WatsonTcpClient.Disconnect: not connected, InvalidOperationException.
    pub fn Disconnect(&mut self) -> Result<(), Exc> {
        if !self.Connected {
            return Err(Exc::new("InvalidOperationException", "Not connected to the server."));
        }
        self.Connected = false;
        Ok(())
    }
    /// WatsonTcpClient.Dispose: disconnected (when connected), its settings, events and callbacks nulled.
    pub fn Dispose(&mut self) -> Result<(), Exc> {
        if self.Connected {
            self.Disconnect()?;
        }
        self.disposed = true;
        Ok(())
    }
}

/// ModCompilerSockets: the compiler link over WatsonTcp (the one CompilerClient uses).
#[derive(Debug, Default)]
pub struct ModCompilerSockets {
    client: Option<WatsonClient>,
    serverProcess: Option<std::process::Child>,
    pub replies: std::collections::BTreeMap<i32, CompilerReply>,
    pub Config: Option<CompilerConfig>,
    pub IsBusy: bool,
}

impl ModCompilerSockets {
    /// ModCompilerSockets.Start: StartServer.
    pub fn Start(&mut self, base: &Path) -> Result<(), Exc> {
        self.StartServer(base)
    }
    /// ModCompilerSockets.RequestCompilationSynchronous: GetReply, any exception as an Error reply with its message.
    pub fn RequestCompilationSynchronous(&mut self, base: &Path, mut instructions: ModCompileInstructions) -> CompilerReply {
        while self.IsBusy {
            std::thread::sleep(Duration::from_millis(16));
        }
        self.IsBusy = true;
        let r = self.GetReply(base, &mut instructions).unwrap_or_else(|e| CompilerReply::error(e.message));
        self.IsBusy = false;
        r
    }
    /// ModCompilerSockets.Stop: a client is told "quit" (when connected), disconnected (not connected: throws
    /// InvalidOperationException) and disposed; the replies cleared, the server killed, every PPGModCompiler process killed.
    pub fn Stop(&mut self) -> Result<(), Exc> {
        if let Some(c) = self.client.as_mut() {
            c.Disconnect()?;
            c.Dispose()?;
        }
        self.replies.clear();
        if let Some(mut p) = self.serverProcess.take() {
            p.kill().map_err(|_| Exc::new("InvalidOperationException", "Cannot process request because the process has exited."))?;
        }
        kill_all_server_processes()?;
        self.client = None;
        Ok(())
    }
    /// ModCompilerSockets.Dispose: Stop.
    pub fn Dispose(&mut self) -> Result<(), Exc> {
        self.Stop()
    }
    /// ModCompilerSockets.KillAllServerProcesses
    pub fn KillAllServerProcesses() -> Result<(), Exc> {
        kill_all_server_processes()
    }
    /// ModCompilerSockets.OnServerError: logged.
    pub fn OnServerError(_data: Option<&str>) {}
    /// ModCompilerSockets.OnClientOpen: logged.
    pub fn OnClientOpen() {}
    /// ModCompilerSockets.OnClientClose: logged.
    pub fn OnClientClose() {}
    /// ModCompilerSockets.StartClient: the old client disposed, a new one for Config's host and port (a null Config throws
    /// NullReferenceException, a bad host or port the constructor's exception, keeping the old one), connected, greeted.
    pub fn StartClient(&mut self) -> Result<(), Exc> {
        if let Some(c) = self.client.as_mut() {
            c.Dispose()?;
        }
        let cfg = self.Config.as_ref().ok_or_else(Exc::nre)?;
        self.client = Some(WatsonClient::new(cfg.Hostname.as_deref(), cfg.Port)?);
        self.client.as_mut().unwrap().Connect()?;
        unimplemented!("mod-only: SendAndWait to a connected PPGModCompiler server")
    }
    /// ModCompilerSockets.StartServer: every PPGModCompiler process killed, then PPGModCompiler.exe started (ShellExecute) in
    /// GetFullPath("ppgModCompiler") with this process's id.
    pub fn StartServer(&mut self, base: &Path) -> Result<(), Exc> {
        kill_all_server_processes()?;
        let dir = full_path(base, "ppgModCompiler")?;
        let p = start_process(base, "PPGModCompiler.exe", &dir, &std::process::id().to_string(), true)?;
        self.serverProcess = Some(p);
        Ok(())
    }
    /// ModCompilerSockets.OnClientMessage: the message's CompilerReply added by its ID (an ID of -1, an ID already there or
    /// a bad message: logged, not added).
    pub fn OnClientMessage(&mut self, data: &[u8]) {
        if let Ok(r) = reply_from_json(&String::from_utf8_lossy(data)) {
            if r.ID != -1 {
                self.replies.entry(r.ID).or_insert(r);
            }
        }
    }
    /// ModCompilerSockets.GetReply: a client started when there is none; with no connection and no running server, the
    /// server started ("Starting server"); then connecting for up to 15 s, the instructions sent, the reply waited for.
    pub fn GetReply(&mut self, base: &Path, instructions: &mut ModCompileInstructions) -> Result<CompilerReply, Exc> {
        if self.client.is_none() {
            self.StartClient()?;
        }
        let exited = match self.serverProcess.as_mut() {
            None => true,
            Some(p) => p.try_wait().map_or(true, |s| s.is_some()),
        };
        if !self.client.as_ref().unwrap().Connected && exited {
            set_display_state(crate::il!(str "ModCompilerSockets::GetReply" 0));
            // An exited serverProcess is Start()ed again: the same command started anew.
            self.StartServer(base)?;
        }
        // instructions.Paths.GetHashCode(): the array's identity hash.
        instructions.ID = instructions.Paths.as_ref().map_or(0, |p| Arc::as_ptr(p) as usize as i32);
        let mut waited = Duration::ZERO;
        let step = Duration::from_secs_f64(0.05000000074505806);
        loop {
            std::thread::sleep(step);
            waited += step;
            if waited > Duration::from_secs(15) {
                return Err(Exc::new("Exception", crate::il!(str "ModCompilerSockets::GetReply" 1)));
            }
            let c = self.client.as_mut().unwrap();
            if c.Connected {
                break;
            }
            c.Connect()?;
            unimplemented!("mod-only: SendAndWait to a connected PPGModCompiler server");
        }
        unimplemented!("mod-only: sending instructions to a PPGModCompiler server")
    }
}

/// ModCompilerPipes: the named-pipe compiler link (nothing makes one in 1.28).
#[derive(Debug, Default)]
pub struct ModCompilerPipes {
    /// The NamedPipeServerStream (made, never connected).
    pipeServer: bool,
    pipeClient: Option<std::process::Child>,
    shouldStop: bool,
    pub replies: std::collections::BTreeMap<i32, CompilerReply>,
    pub Config: Option<CompilerConfig>,
    pub IsBusy: bool,
}

impl ModCompilerPipes {
    /// ModCompilerPipes.GetPipeName: this process's id.
    pub fn GetPipeName(&self) -> String {
        std::process::id().to_string()
    }
    /// ModCompilerPipes.Start: every PPGModCompiler process killed, the pipe server made, ppgModCompiler/PPGModCompiler.exe
    /// started (CreateProcess, its full path) with the pipe's name, then its connection waited for.
    pub fn Start(&mut self, base: &Path) -> Result<(), Exc> {
        let name = self.GetPipeName();
        self.shouldStop = false;
        kill_all_server_processes()?;
        let exe = full_path(base, "ppgModCompiler/PPGModCompiler.exe")?;
        // NamedPipeServerStream: no OS pipe is made, since nothing gets past the connection wait below (mod-only), so no
        // compiler could ever use it.
        self.pipeServer = true;
        let dir = full_path(base, "ppgModCompiler")?;
        self.pipeClient = None;
        self.pipeClient = Some(start_process(base, &exe, &dir, &name, false)?);
        unimplemented!("mod-only: waiting for PPGModCompiler to connect to the pipe")
    }
    /// ModCompilerPipes.ReceiveMessages: a StreamReader on the pipe (none: ArgumentNullException); unless stopping, ReadLine
    /// on the unconnected pipe throws InvalidOperationException.
    pub fn ReceiveMessages(&mut self) -> Result<(), Exc> {
        if !self.pipeServer {
            return Err(Exc::new("ArgumentNullException", "Value cannot be null.\r\nParameter name: stream"));
        }
        if !self.shouldStop {
            return Err(Exc::new("InvalidOperationException", "Pipe hasn't been connected yet."));
        }
        Ok(())
    }
    /// ModCompilerPipes.RequestCompilationSynchronous: GetReply, any exception as "Unknown error occurred...".
    pub fn RequestCompilationSynchronous(&mut self, base: &Path, mut instructions: ModCompileInstructions) -> CompilerReply {
        while self.IsBusy {
            std::thread::sleep(Duration::from_millis(16));
        }
        self.IsBusy = true;
        let r = self.GetReply(base, &mut instructions).unwrap_or_else(|_| CompilerReply::error(crate::il!(str "ModCompilerPipes::RequestCompilationSynchronous" 0)));
        self.IsBusy = false;
        r
    }
    /// ModCompilerPipes.Stop: shouldStop set, then receiveTask.Wait on a task never started (the pipe never connects):
    /// NullReferenceException.
    pub fn Stop(&mut self) -> Result<(), Exc> {
        self.shouldStop = true;
        Err(Exc::nre())
    }
    /// ModCompilerPipes.Dispose: writer.Dispose on a writer never made: NullReferenceException.
    pub fn Dispose(&mut self) -> Result<(), Exc> {
        Err(Exc::nre())
    }
    /// ModCompilerPipes.KillAllServerProcesses
    pub fn KillAllServerProcesses() -> Result<(), Exc> {
        kill_all_server_processes()
    }
    /// ModCompilerPipes.OnServerError: logged.
    pub fn OnServerError(_data: Option<&str>) {}
    /// ModCompilerPipes.GetReply: no pipe throws "Compiler client isn't running."; an unconnected one with no running
    /// compiler starts it ("Starting server").
    pub fn GetReply(&mut self, base: &Path, _instructions: &mut ModCompileInstructions) -> Result<CompilerReply, Exc> {
        if !self.pipeServer {
            return Err(Exc::new("Exception", crate::il!(str "ModCompilerPipes::GetReply" 0)));
        }
        let exited = match self.pipeClient.as_mut() {
            None => true,
            Some(p) => p.try_wait().map_or(true, |s| s.is_some()),
        };
        if exited {
            set_display_state(crate::il!(str "ModCompilerPipes::GetReply" 1));
            self.Start(base)?;
        }
        unimplemented!("mod-only: waiting for PPGModCompiler on the pipe")
    }
}

// ---- CompilerClient, CompilerServerBehaviour ----

/// CompilerClient: its one compiler, a ModCompilerSockets.
#[derive(Debug, Default)]
pub struct CompilerClient {
    pub compiler: ModCompilerSockets,
}

pub static COMPILER_CLIENT: LazyLock<Mutex<CompilerClient>> = LazyLock::new(Default::default);

impl CompilerClient {
    /// CompilerClient.ReadConfig: ppgModCompiler\config.json deserialised (no value: None); a failure gives a new CompilerConfig.
    pub fn ReadConfig(base: &Path) -> Option<CompilerConfig> {
        match jsonnet::read_all_text(&base.join("ppgModCompiler").join("config.json")).map_err(|e| e.to_string()).and_then(|t| config_from_json(&t)) {
            Ok(c) => c,
            Err(_) => Some(CompilerConfig::default()),
        }
    }
    /// CompilerClient.InitialiseConfig
    pub fn InitialiseConfig(&mut self, base: &Path) {
        self.compiler.Config = Self::ReadConfig(base);
    }
    /// CompilerClient.Start
    pub fn Start(&mut self, base: &Path) -> Result<(), Exc> {
        self.compiler.Start(base)
    }
    /// CompilerClient.Stop
    pub fn Stop(&mut self) -> Result<(), Exc> {
        self.compiler.Stop()
    }
    /// CompilerClient.Dispose
    pub fn Dispose(&mut self) -> Result<(), Exc> {
        self.compiler.Dispose()
    }
    /// CompilerClient.RequestCompilationSynchronous
    pub fn RequestCompilationSynchronous(&mut self, base: &Path, instructions: ModCompileInstructions) -> CompilerReply {
        self.compiler.RequestCompilationSynchronous(base, instructions)
    }
}

/// CompilerServerBehaviour.Awake: DontDestroyOnLoad, CompilerClient.InitialiseConfig and Start.
pub fn compiler_server_awake(base: &Path) -> Result<(), Exc> {
    let mut c = COMPILER_CLIENT.lock().unwrap();
    c.InitialiseConfig(base);
    c.Start(base)
}
/// CompilerServerBehaviour.OnApplicationQuit: CompilerClient.Dispose.
pub fn compiler_server_on_application_quit() -> Result<(), Exc> {
    COMPILER_CLIENT.lock().unwrap().Dispose()
}

// ---- ModCompilerStatusBehaviour ----

/// ModCompilerStatusBehaviour (LegacyMods): the compile status overlay. State: Compiling 0, Reading 1, Loading 2.
#[derive(Debug, Default)]
pub struct ModCompilerStatusBehaviour {
    modName: Option<String>,
    state: i32,
    /// TextMeshPro.text
    pub text: Option<String>,
    /// Thumbnail.sprite: the mod's thumbnail, None for DefaultModSprite.
    pub thumbnail: Option<Picture>,
    /// gameObject.activeSelf
    pub active: bool,
}

/// ModCompilerStatusBehaviour.Instance
pub static STATUS_INSTANCE: Mutex<Option<ModCompilerStatusBehaviour>> = Mutex::new(None);

impl ModCompilerStatusBehaviour {
    /// ModCompilerStatusBehaviour..ctor: modName empty.
    pub fn new() -> Self {
        ModCompilerStatusBehaviour { modName: Some(String::new()), active: true, ..Default::default() }
    }
    /// ModCompilerStatusBehaviour.Awake: Instance = this, Hide.
    pub fn Awake(self) {
        *STATUS_INSTANCE.lock().unwrap() = Some(self);
        Self::Hide();
    }
    /// ModCompilerStatusBehaviour.SetMod: its name shown, its thumbnail (else the default sprite).
    pub fn SetMod(&mut self, loader: &mut ModLoader, m: &ModRef) {
        self.modName = m.lock().unwrap().Name.clone();
        self.text = self.modName.clone();
        self.thumbnail = loader.GetThumbnail(m);
    }
    /// ModCompilerStatusBehaviour.Update: `<color=#ccc>{state}</color>\n{modName}`.
    pub fn Update(&mut self) {
        let st = match self.state {
            0 => "Compiling".to_string(),
            1 => "Reading".to_string(),
            2 => "Loading".to_string(),
            n => n.to_string(),
        };
        self.text = Some(format!("<color=#ccc>{st}</color>\n{}", self.modName.as_deref().unwrap_or("")));
    }
    /// ModCompilerStatusBehaviour.SetDisplayState
    pub fn SetDisplayState(&mut self, state: i32) {
        self.state = state;
    }
    /// ModCompilerStatusBehaviour.Show: with an Instance, SetMod and shown.
    pub fn Show(loader: &mut ModLoader, m: &ModRef) {
        if let Some(i) = STATUS_INSTANCE.lock().unwrap().as_mut() {
            i.SetMod(loader, m);
            i.active = true;
        }
    }
    /// ModCompilerStatusBehaviour.Hide: with an Instance, hidden.
    pub fn Hide() {
        if let Some(i) = STATUS_INSTANCE.lock().unwrap().as_mut() {
            i.active = false;
        }
    }
}

// ---- the legacy mod list (ModListBehaviour / ModEntryBehaviour) ----
// Only a prefab asset holds a ModListBehaviour (the Mods page's Enable/Disable all buttons target it, with no entries) and
// nothing instantiates it in 1.28; its entries would be ModLoader.LoadedMods. The UI objects are their state here: texts,
// the active flags of the check marks and buttons, and the coroutines as pending steps.

/// Color.white / Color.yellow / Color.red.
pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub const YELLOW: [f32; 4] = [1.0, 0.921_568_6, 0.015_686_275, 1.0];
pub const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// ModEntryBehaviour's state.
#[derive(Clone, Debug)]
pub struct ModEntryBehaviour {
    pub ModMeta: Option<ModRef>,
    pub active: bool,
    pub Title: String,
    pub Subtitle: String,
    pub Description: String,
    pub DescriptionColor: [f32; 4],
    pub Thumbnail: Option<Picture>,
    pub ActiveCheck: bool,
    pub InactiveCheck: bool,
    pub ErrorCheck: bool,
    pub CompilingCheck: bool,
    pub SuspiciousCheck: bool,
    pub UpdateCheck: bool,
    pub RecompileButton: bool,
    pub ToggleModButton: bool,
    pub UploadButton: bool,
    pub TrustButton: bool,
    pub UpdateButton: bool,
    pub OpenInWorkshopButton: bool,
    lastClickTime: f32,
    isBusy: bool,
    /// Recompile's coroutine, waiting out its two 0.1 s waits.
    pub recompiling: bool,
}

impl ModEntryBehaviour {
    /// ModEntryBehaviour..ctor (the prefab's objects as instantiated: all active).
    pub fn new(m: Option<ModRef>) -> Self {
        ModEntryBehaviour {
            ModMeta: m,
            active: true,
            Title: String::new(),
            Subtitle: String::new(),
            Description: String::new(),
            DescriptionColor: WHITE,
            Thumbnail: None,
            ActiveCheck: true,
            InactiveCheck: true,
            ErrorCheck: true,
            CompilingCheck: true,
            SuspiciousCheck: true,
            UpdateCheck: true,
            RecompileButton: true,
            ToggleModButton: true,
            UploadButton: true,
            TrustButton: true,
            UpdateButton: true,
            OpenInWorkshopButton: true,
            lastClickTime: f32::NEG_INFINITY,
            isBusy: false,
            recompiling: false,
        }
    }

    /// ModEntryBehaviour.Start: UpdateUi.
    pub fn Start(&mut self, loader: &mut ModLoader) -> Result<(), Exc> {
        self.UpdateUi(loader)
    }

    /// ModEntryBehaviour.ShowDescription: the list's description text (a null ModMeta throws).
    pub fn ShowDescription(&self) -> Result<String, Exc> {
        let m = self.ModMeta.as_ref().ok_or_else(Exc::nre)?.lock().unwrap().clone();
        let s = |o: &Option<String>| o.clone().unwrap_or_default();
        let rest = m.RichReadme.clone().unwrap_or_else(|| format!("{}\n<noparse>A mod by {} for game version {}</noparse>", s(&m.Description), s(&m.Author), s(&m.GameVersion)));
        Ok(format!("<size=1.5em>{}</size>\n{rest}", s(&m.Name)))
    }

    /// ModEntryBehaviour.UpdateUi: nothing without a ModMeta.
    pub fn UpdateUi(&mut self, loader: &mut ModLoader) -> Result<(), Exc> {
        let Some(r) = self.ModMeta.clone() else { return Ok(()) };
        let m = r.lock().unwrap().clone();
        let esc = |o: &Option<String>| crate::game::utils::escape_rich_text(o.as_deref().unwrap_or(""));
        self.Title = esc(&m.Name);
        self.Subtitle = format!("by <b>{}</b> for {}", esc(&m.Author), esc(&m.GameVersion));
        if let Some(t) = loader.GetThumbnail(&r) {
            self.Thumbnail = Some(t);
        }
        let (up, err, sus) = (m.NeedsUpdateApproval, m.HasErrors, m.Suspicious);
        self.ActiveCheck = !up && m.Active && !err;
        self.InactiveCheck = !up && !m.Active && !err;
        self.ErrorCheck = !up && err && !sus;
        self.SuspiciousCheck = !up && err && sus;
        self.ToggleModButton = !err;
        self.RecompileButton = !up && err;
        self.TrustButton = !up && err && sus;
        self.UpdateButton = up;
        self.CompilingCheck = false;
        self.UpdateCheck = up;
        self.OpenInWorkshopButton = m.UGCIdentity.is_some() || m.CreatorUGCIdentity.is_some();
        if up {
            self.DescriptionColor = WHITE;
            self.Description = crate::il!(str "ModEntryBehaviour::UpdateUi" 2).into();
        } else if m.Errors.as_deref().is_some_and(|e| !e.trim().is_empty()) && err {
            // string.Join("\n", Errors) of the one string: the string.
            let e = m.Errors.clone().unwrap_or_default();
            if sus {
                self.DescriptionColor = YELLOW;
                self.Description = format!("This mod contains potentially harmful code. You may accept this risk, but you might regret it.\n{e}");
            } else {
                self.DescriptionColor = RED;
                self.Description = e;
            }
        } else {
            self.DescriptionColor = WHITE;
            self.Description = m.Description.clone().unwrap_or_default();
        }
        // HasDoNotReuploadSign only runs when the first two hold (&&).
        self.UploadButton = !err && m.UGCIdentity.as_deref().map_or(true, str::is_empty) && !m.HasDoNotReuploadSign().map_err(|e| Exc::new(e, ""))?;
        Ok(())
    }

    /// ModEntryBehaviour.ToggleMod: when not busy, SetModActive(!Active), then UpdateUi.
    pub fn ToggleMod(&mut self, loader: &Mutex<ModLoader>) -> Result<(), Exc> {
        if self.isBusy {
            return Ok(());
        }
        TrustCache::VerifyOrigin(true)?;
        let r = self.ModMeta.clone().ok_or_else(Exc::nre)?;
        let active = r.lock().unwrap().Active;
        SetModActive(loader, &r, !active)?;
        self.UpdateUi(&mut loader.lock().unwrap())
    }

    /// ModEntryBehaviour.TrustMod: when not busy and no dialog box is open, the "Are you sure?" dialog (Cancel, and
    /// Continue whose action is `trust_mod_continue`): its text when it opens.
    pub fn TrustMod(&self, any_dialog_open: bool) -> Option<&'static str> {
        (!self.isBusy && !any_dialog_open)
            .then_some(crate::il!(str "ModEntryBehaviour::TrustMod" 0))
    }

    /// TrustMod's Continue: TrustCache.Trust, TrustCache.Save (in the working directory `base`), ForceReload.
    pub fn trust_mod_continue(&mut self, base: &Path) -> Result<(), Exc> {
        let m = self.ModMeta.clone().ok_or_else(Exc::nre)?.lock().unwrap().clone();
        {
            let mut tc = TRUST_CACHE.lock().unwrap();
            tc.Trust(&m)?;
            tc.Save(base)?;
        }
        self.ForceReload()
    }

    /// ModEntryBehaviour.UpdateMod: NeedsUpdateApproval cleared, ForceReload.
    pub fn UpdateMod(&mut self) -> Result<(), Exc> {
        TrustCache::VerifyOrigin(true)?;
        self.ModMeta.as_ref().ok_or_else(Exc::nre)?.lock().unwrap().NeedsUpdateApproval = false;
        self.ForceReload()
    }

    /// ModEntryBehaviour.ForceReload: when not busy, every mark off but "compiling", and Recompile started.
    pub fn ForceReload(&mut self) -> Result<(), Exc> {
        if self.isBusy {
            return Ok(());
        }
        self.ActiveCheck = false;
        self.InactiveCheck = false;
        self.SuspiciousCheck = false;
        self.ErrorCheck = false;
        self.ToggleModButton = false;
        self.RecompileButton = false;
        self.TrustButton = false;
        self.UpdateButton = false;
        self.UpdateCheck = false;
        self.CompilingCheck = true;
        self.Recompile()
    }

    /// ModEntryBehaviour.OpenInWorkshop: the workshop page of UGCIdentity ?? CreatorUGCIdentity (Utils.OpenURL), if any.
    pub fn OpenInWorkshop(&self) -> Result<Option<String>, Exc> {
        let m = self.ModMeta.as_ref().ok_or_else(Exc::nre)?.lock().unwrap().clone();
        Ok(m.UGCIdentity.or(m.CreatorUGCIdentity).map(|id| format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")))
    }

    /// ModEntryBehaviour.Recompile up to its waits: busy, description cleared (it reads BackgroundItemLoader.Instance,
    /// which the rewrite always has).
    fn Recompile(&mut self) -> Result<(), Exc> {
        if self.isBusy {
            return Ok(());
        }
        TrustCache::VerifyOrigin(false)?;
        self.isBusy = true;
        self.Description.clear();
        self.recompiling = true;
        Ok(())
    }

    /// Recompile after its waits: UpdateUi, the entry destroyed (false) when it has no ModMeta, not busy.
    pub fn recompile_finish(&mut self, loader: &mut ModLoader) -> Result<bool, Exc> {
        self.recompiling = false;
        self.UpdateUi(loader)?;
        let keep = self.ModMeta.is_some();
        self.isBusy = false;
        Ok(keep)
    }

    /// ModEntryBehaviour.OnSelect: ShowDescription.
    pub fn OnSelect(&self) -> Result<String, Exc> {
        self.ShowDescription()
    }

    /// ModEntryBehaviour.OnPointerDown: a second press within 0.3 s (unscaled time) toggles the mod.
    pub fn OnPointerDown(&mut self, loader: &Mutex<ModLoader>, unscaled_time: f32) -> Result<(), Exc> {
        let r = if (self.lastClickTime - unscaled_time).abs() < 0.3 { self.ToggleMod(loader) } else { Ok(()) };
        self.lastClickTime = unscaled_time;
        r
    }
}

/// ModListBehaviour's state.
#[derive(Debug, Default)]
pub struct ModListBehaviour {
    pub modEntries: Vec<ModEntryBehaviour>,
    pub NoModsFound: bool,
    pub ModDescription: String,
    pub FallbackModDescription: String,
    /// ResetView's coroutine: the scroll view reset at the end of the frame.
    pub reset_pending: bool,
}

impl ModListBehaviour {
    /// ModListBehaviour..ctor
    pub fn new() -> Self {
        ModListBehaviour { FallbackModDescription: crate::il!(str "ModListBehaviour::.ctor" 0).into(), ..Default::default() }
    }

    /// ModListBehaviour.Awake: ModLoader.ModListChanged subscribed (ModLoader_ModListChanged), Generate, and the
    /// filter's onValueChanged calls ApplySearchFilter.
    pub fn Awake(&mut self, loader: &mut ModLoader) -> Result<(), Exc> {
        self.Generate(loader)
    }

    /// ModListBehaviour.Start (Container defaults to its own transform): ResetView.
    pub fn Start(&mut self) {
        self.ResetView();
    }

    /// ModListBehaviour.Generate: the old entries destroyed, an entry per non-null loaded mod (UpdateUi each), "no mods"
    /// shown for an empty list, ResetView.
    pub fn Generate(&mut self, loader: &mut ModLoader) -> Result<(), Exc> {
        self.modEntries.clear();
        let mods = loader.LoadedMods.clone();
        for m in &mods {
            let mut e = ModEntryBehaviour::new(Some(m.clone()));
            e.UpdateUi(loader)?;
            self.modEntries.push(e);
        }
        self.NoModsFound = mods.is_empty();
        self.ResetView();
        Ok(())
    }

    /// ModListBehaviour.ApplySearchFilter: the entries whose name or author holds the text ToLower'd, Normalize'd (NFC) and
    /// trimmed shown (all for blank text, none without a ModMeta); a null name or author throws, leaving the rest as they were.
    pub fn ApplySearchFilter(&mut self, arg0: &str) -> Result<(), Exc> {
        use unicode_normalization::UnicodeNormalization;
        let norm = |s: &str| to_lower(s).nfc().collect::<String>();
        let value = norm(arg0).trim().to_string();
        let blank = value.is_empty();
        for e in &mut self.modEntries {
            e.active = if blank {
                true
            } else if let Some(m) = &e.ModMeta {
                let m = m.lock().unwrap();
                norm(m.Name.as_deref().ok_or_else(Exc::nre)?).contains(&value) || norm(m.Author.as_deref().ok_or_else(Exc::nre)?).contains(&value)
            } else {
                false
            };
        }
        Ok(())
    }

    /// ModListBehaviour.ModLoader_ModListChanged: Generate.
    pub fn ModLoader_ModListChanged(&mut self, loader: &mut ModLoader) -> Result<(), Exc> {
        self.Generate(loader)
    }

    /// ModListBehaviour.ResetView: OnNextFrame's coroutine puts the scroll view at the top at the end of the frame.
    pub fn ResetView(&mut self) {
        self.reset_pending = true;
    }

    /// ModListBehaviour.OnPageSelected: ResetView.
    pub fn OnPageSelected(&mut self) {
        self.ResetView();
    }

    /// ModListBehaviour.OnNextFrame: the reset done.
    pub fn OnNextFrame(&mut self) {
        self.reset_pending = false;
    }

    /// ModListBehaviour.OnDestroy: unsubscribed from ModListChanged and the filter.
    pub fn OnDestroy(&mut self) {}

    /// ModListBehaviour.SetAllModStatus: SetModActive and UpdateUi for each entry.
    pub fn SetAllModStatus(&mut self, loader: &Mutex<ModLoader>, active: bool) -> Result<(), Exc> {
        for e in &mut self.modEntries {
            SetModActive(loader, e.ModMeta.as_ref().ok_or_else(Exc::nre)?, active)?;
            e.UpdateUi(&mut loader.lock().unwrap())?;
        }
        Ok(())
    }
}

/// String.ToLower() in the game's culture (en-US, as ui/collation.rs fixes it): Mono's TextInfo.ToLower, each UTF-16 unit by
/// its simple lowercase mapping, so U+0130 (I with a dot) is a plain i and there is no final-sigma rule. Unchecked against
/// the game: a letter outside the BMP (a surrogate pair) is left as it is.
fn to_lower(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{130}' => 'i',
            c if (c as u32) > 0xffff => c,
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_filter_lower_case_and_version_text() {
        assert_eq!(to_lower("\u{130}STANBUL \u{3a3}ON \u{1D400}"), "istanbul \u{3c3}on \u{1D400}");
        assert_eq!(create_version_from_string("6.5.0-28-generic"), "6.5.0.28");
        assert_eq!(create_version_from_string("10.1"), "10.1.0.0");
        let meta = |name: Option<&str>, author: &str| {
            let mut m = ModMetaData::new();
            m.Name = name.map(str::to_string);
            m.Author = Some(author.into());
            Some(std::sync::Arc::new(std::sync::Mutex::new(m)))
        };
        let mut list = ModListBehaviour::new();
        list.modEntries = vec![ModEntryBehaviour::new(meta(Some("Cafe\u{301} Guns"), "Ann")), ModEntryBehaviour::new(meta(Some("Tools"), "\u{130}lker")), ModEntryBehaviour::new(None)];
        let shown = |l: &ModListBehaviour| l.modEntries.iter().map(|e| e.active).collect::<Vec<_>>();
        list.ApplySearchFilter("  ILKER ").unwrap();
        assert_eq!(shown(&list), [false, true, false]);
        list.ApplySearchFilter("caf\u{e9}").unwrap();
        assert_eq!(shown(&list), [true, false, false]);
        list.ApplySearchFilter(" ").unwrap();
        assert_eq!(shown(&list), [true, true, true]);
        list.modEntries.insert(1, ModEntryBehaviour::new(meta(None, "x")));
        assert_eq!(list.ApplySearchFilter("ann"), Err(Exc::nre()));
        assert_eq!(shown(&list), [true, true, true, true]);
    }

    /// ModLoader.GetThumbnail / LoadTexture over a thumbnail made here, and TrustCache.Save into a temp folder.
    #[test]
    fn thumbnails_and_trust_cache_save_in_a_temp_folder() {
        let root = std::env::temp_dir().join(format!("ppg-legacy-thumb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("thumb.png"), crate::game::mods::tests::test_png(3, 5)).unwrap();
        std::fs::write(root.join("bad.png"), b"nope").unwrap();
        let s = LoadTexture(&root.join("thumb.png"), false).unwrap();
        assert_eq!((s.rect, s.pixels_per_unit, s.texture.point_filter), ([0.0, 0.0, 3.0, 5.0], 256.0, false));
        assert_eq!(LoadTexture(&root.join("bad.png"), false).unwrap_err().class, "InvalidDataException");
        assert_eq!(LoadTexture(&root.join("gone.png"), false).unwrap_err().class, "FileNotFoundException");
        let mut loader = ModLoader::default();
        let m: ModRef = {
            let mut m = ModMetaData::new();
            m.MetaLocation = Some(root.display().to_string());
            m.ThumbnailPath = Some("thumb.png".into());
            std::sync::Arc::new(std::sync::Mutex::new(m))
        };
        let t = loader.GetThumbnail(&m).unwrap();
        assert!(std::sync::Arc::ptr_eq(&t, &loader.GetThumbnail(&m).unwrap()));
        m.lock().unwrap().ThumbnailPath = Some("bad.png".into());
        let bad = m.lock().unwrap().clone();
        assert!(loader.GetThumbnail(&std::sync::Arc::new(std::sync::Mutex::new(bad))).is_none());
        let cache = TrustCache::cctor();
        cache.Save(&root).unwrap();
        assert_eq!(std::fs::read(root.join(cache.path())).unwrap(), encode_uint_hashset(&cache.trusted()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Path.GetFullPath as the game's Mono gives it (full_path_cases.tsv: 811 paths, two working folders): drive-relative
    /// paths, ':' anywhere, UNC, `..`, blanks, trailing dots and spaces, random mixes.
    #[test]
    fn full_path_matches_mono() {
        for line in include_str!("full_path_cases.tsv").lines().filter(|l| !l.starts_with('#')) {
            let c: Vec<&str> = line.split('\t').collect();
            let p = String::from_utf8((0..c[0].len()).step_by(2).map(|i| u8::from_str_radix(&c[0][i..i + 2], 16).unwrap()).collect()).unwrap();
            for (base, kind, want) in [("C:\\Windows\\System32", c[1], c[2]), ("F:\\Users\\billy\\Projects", c[3], c[4])] {
                let got = get_full_path(Path::new(base), &p);
                match kind {
                    "ok" => assert_eq!(got, Ok(want.replace('\\', "/")), "{p:?} in {base}"),
                    _ => assert_eq!(got, Err(want.split(':').next().unwrap()), "{p:?} in {base}"),
                }
                if kind == "exc" {
                    let e = full_path(Path::new(base), &p).unwrap_err();
                    assert_eq!(format!("{}: {}", e.class, e.message), want, "{p:?} in {base}");
                }
            }
        }
        // new DirectoryInfo("C:x:y") (CheckPath passes: the first ':' is second): Mono's GetFullPath of it.
        assert_eq!(directory_info_full_name(Path::new("C:\\Windows\\System32"), Some("C:x:y")), Ok("x:y".into()));
        assert_eq!(directory_info_full_name(Path::new("F:\\Users\\billy\\Projects"), Some("C:x:y")), Ok("C:/x:y".into()));
    }

    /// The text a failed compile gives the mod (TryCompileMod's catch: Exception.ToString with Mono's stack trace), against the
    /// game's own ModLoader.TryCompileMod run under its Mono (1.28.54 and 1.28.41 gave the same text but for Assembly-CSharp's
    /// Mvid; 1.28.57's Mvid is read from its DLL, its text not re-run): {A} {M} {C} are Assembly-CSharp's, mscorlib's and System.Core's Mvid, {B} the working folder.
    #[test]
    fn compile_errors_read_as_the_game() {
        let want_mvid = if crate::assets::il::is_1_28_45() { "003f2f86540b480386d7a8d0b40d72bd" } else { "fcdb535469eb49fa8183bb51f5a78228" };
        let (a, m, c) = (crate::assets::il::mvid("Assembly-CSharp"), crate::assets::il::mvid("mscorlib"), crate::assets::il::mvid("System.Core"));
        assert_eq!((a.as_str(), m.as_str(), c.as_str()), (want_mvid, "eae584ce26bc40229c1b1aa476bfa589", "351e49e2a5bf4fd6beabb458ce2255f3"));
        let base = std::env::temp_dir().join(format!("ppg-legacy-compile-test-{}", std::process::id()));
        let rows: &[(Option<Vec<Option<&str>>>, Option<&str>, bool, &str, &str)] = &[
            (None, Some("Mods/A"), false, "", "System.NullReferenceException: Object reference not set to an instance of an object\r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x0006f] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
            (Some(vec![None]), Some("Mods/A"), false, "", "System.ArgumentNullException: Value cannot be null.\r\nParameter name: path2\r\n  at System.IO.Path.Combine (System.String path1, System.String path2) [0x00011] in <{M}>:0 \r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x00033] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
            (Some(vec![Some("a.cs")]), None, false, "", "System.ArgumentNullException: Value cannot be null.\r\nParameter name: path1\r\n  at System.IO.Path.Combine (System.String path1, System.String path2) [0x00003] in <{M}>:0 \r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x00033] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
            (Some(vec![Some("a<b.cs")]), Some("Mods/A"), false, "", "System.ArgumentException: Illegal characters in path.\r\n  at System.IO.Path.Combine (System.String path1, System.String path2) [0x00057] in <{M}>:0 \r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x00033] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
            (Some(vec![]), None, false, "", "System.ArgumentNullException: Value cannot be null.\r\nParameter name: path\r\n  at System.IO.DirectoryInfo.CheckPath (System.String path) [0x00003] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath) [0x00006] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path) [0x00000] in <{M}>:0 \r\n  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)\r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x000de] in <{A}>:0 "),
            (Some(vec![]), Some("Mods/A<"), false, "", "System.ArgumentException: Illegal characters in path.\r\n  at System.IO.DirectoryInfo.CheckPath (System.String path) [0x0002f] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath) [0x00006] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path) [0x00000] in <{M}>:0 \r\n  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)\r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x000de] in <{A}>:0 "),
            (Some(vec![]), Some("Mo:ds"), false, "", "System.ArgumentException: path\r\n  at System.IO.DirectoryInfo.CheckPath (System.String path) [0x00052] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath) [0x00006] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path) [0x00000] in <{M}>:0 \r\n  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)\r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x000de] in <{A}>:0 "),
            (Some(vec![]), Some("  "), false, "", "System.ArgumentException: The specified path is not of a legal form (empty).\r\n  at System.IO.Path.InsecureGetFullPath (System.String path) [0x00025] in <{M}>:0 \r\n  at System.IO.Path.GetFullPath (System.String path) [0x00000] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath) [0x0000d] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path) [0x00000] in <{M}>:0 \r\n  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)\r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x000de] in <{A}>:0 "),
            (Some(vec![]), Some("Mods/A"), false, "file", "System.IO.IOException: Cannot create CompiledMods because a file with the same name already exists.\r\n  at System.IO.Directory.CreateDirectory (System.String path) [0x0006a] in <{M}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x0013c] in <{A}>:0 "),
            (None, Some("Mods/A"), true, "", "System.NullReferenceException: Object reference not set to an instance of an object\r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x0006f] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
            (Some(vec![]), Some(""), false, "", "System.ArgumentException: An empty file name is not valid.\r\n  at System.IO.DirectoryInfo.CheckPath (System.String path) [0x00016] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path, System.Boolean simpleOriginalPath) [0x00006] in <{M}>:0 \r\n  at System.IO.DirectoryInfo..ctor (System.String path) [0x00000] in <{M}>:0 \r\n  at (wrapper remoting-invoke-with-check) System.IO.DirectoryInfo..ctor(string)\r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x000de] in <{A}>:0 "),
            (Some(vec![]), Some("Mods/A"), false, "ro", "System.UnauthorizedAccessException: Access to the path \"{B}\\CompiledMods\\me-Box-local.dll\" is denied.\r\n  at System.IO.File.Delete (System.String path) [0x00073] in <{M}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00159] in <{A}>:0 "),
            (Some(vec![Some("  ")]), Some(""), false, "", "System.ArgumentException: The specified path is not of a legal form (empty).\r\n  at System.IO.Path.InsecureGetFullPath (System.String path) [0x00025] in <{M}>:0 \r\n  at System.IO.Path.GetFullPath (System.String path) [0x00000] in <{M}>:0 \r\n  at ModLoader+<GetModSourcePaths>d__25.MoveNext () [0x00045] in <{A}>:0 \r\n  at System.Collections.Generic.LargeArrayBuilder`1[T].AddRange (System.Collections.Generic.IEnumerable`1[T] items) [0x0005e] in <{C}>:0 \r\n  at System.Collections.Generic.EnumerableHelpers.ToArray[T] (System.Collections.Generic.IEnumerable`1[T] source) [0x00033] in <{C}>:0 \r\n  at System.Linq.Enumerable.ToArray[TSource] (System.Collections.Generic.IEnumerable`1[T] source) [0x00018] in <{C}>:0 \r\n  at ModLoader.TryCompileMod (ModMetaData modmeta, ModScript& modScript) [0x00026] in <{A}>:0 "),
        ];
        for (scripts, loc, workshop, pre, want) in rows {
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            let mut meta = ModMetaData::new();
            meta.Name = Some("Box".into());
            meta.Author = Some("me".into());
            meta.Scripts = scripts.as_ref().map(|v| v.iter().map(|s| s.map(str::to_string)).collect());
            meta.MetaLocation = loc.map(str::to_string);
            meta.FromWorkshop = *workshop;
            let out = base.join("CompiledMods/me-Box-local.dll");
            match *pre {
                "file" => std::fs::write(base.join("CompiledMods"), "x").unwrap(),
                "ro" => {
                    std::fs::create_dir_all(base.join("CompiledMods")).unwrap();
                    std::fs::write(&out, "x").unwrap();
                    let mut p = std::fs::metadata(&out).unwrap().permissions();
                    p.set_readonly(true);
                    std::fs::set_permissions(&out, p).unwrap();
                }
                _ => {}
            }
            let (r, _) = TryCompileMod(&base, &Arc::new(Mutex::new(meta)), true).unwrap();
            if *pre == "ro" {
                let mut p = std::fs::metadata(&out).unwrap().permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                std::fs::set_permissions(&out, p).unwrap();
            }
            // {B}: the working folder as the game prints it, Windows separators (a Mac temp folder /var/... as \var\...).
            let want = want.replace("{A}", &a).replace("{M}", &m).replace("{C}", &c).replace("{B}", &base.display().to_string().replace('/', "\\"));
            assert_eq!(r.Errors.as_deref(), Some(want.as_str()), "{scripts:?} {loc:?} {pre}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The game's own Ceras (Ceras.dll of the 1.28.54 hotfix under its Mono, CerasSerializer(null).Deserialize<HashSet<uint>>)
    /// on root references -2 and -5 with type names written every way TypeFormatter reads them: Some(set or null), None where
    /// it threw.
    #[test]
    fn trust_cache_decodes_typed_roots_as_the_game() {
        let rows: &[(&str, Option<Option<Vec<u32>>>)] = &[
            ("02020102", Some(Some(vec![1,2]))),
            ("04", Some(None)),
            ("0002", Some(None)),
            ("00000e53797374656d2e55496e743332", None),
            ("0001002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000e53797374656d2e55496e743332", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000e53797374656d2e55496e743332020507", Some(Some(vec![5,7]))),
            ("03003453797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e7433325d020507", Some(Some(vec![5,7]))),
            ("03004053797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206d73636f726c69625d5d020507", Some(Some(vec![5,7]))),
            ("0302020507", None),
            ("", None),
            ("0301002253797374656d2e436f6c6c656374696f6e732e47656e657269632e4c697374603101000e53797374656d2e55496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460310103020507", None),
            ("03010301000e53797374656d2e55496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603102000e53797374656d2e55496e743332000e53797374656d2e55496e743332020507", None),
            ("", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101001853797374656d2e55496e7433322c206d73636f726c6962020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f2053797374656d2e55496e743332020507", Some(Some(vec![5,7]))),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000e73797374656d2e75696e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000d53797374656d2e496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000e53797374656d2e55496e743332020507ffff", Some(Some(vec![5,7]))),
            ("030000020507", None),
            ("03002553797374656d2e436f6c6c656374696f6e732e47656e657269632e486173685365746031020507", None),
            ("03000e53797374656d2e55496e743332020507", None),
            ("0301002753797374656d2e436f6c6c656374696f6e732e47656e657269632e536f72746564536574603101000e53797374656d2e55496e743332020507", None),
            ("0301003253797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460312c2053797374656d2e436f726501000e53797374656d2e55496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f53797374656d2e55496e74333220020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e486173685365746031010010202053797374656d2e55496e743332020507", Some(Some(vec![5,7]))),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f0953797374656d2e55496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f53797374656d2e55496e74333209020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f53797374656d2e55496e74333226020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f53797374656d2e55496e7433322a020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101001053797374656d2e55496e7433325b5d020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000f0a53797374656d2e55496e743332020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101001753797374656d2e55496e7433322c6d73636f726c6962020507", None),
            ("0301002553797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460310100112053797374656d202e2055496e743332020507", None),
            ("0301002653797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460312001000e53797374656d2e55496e743332020507", None),
            ("030100262053797374656d2e436f6c6c656374696f6e732e47656e657269632e48617368536574603101000e53797374656d2e55496e743332020507", Some(Some(vec![5,7]))),
            ("0301002753797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5d01000e53797374656d2e55496e743332020507", None),
            ("0301002753797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460312b5801000e53797374656d2e55496e743332020507", None),
            ("03003553797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b2053797374656d2e55496e7433325d020507", Some(Some(vec![5,7]))),
            ("03003553797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e743332205d020507", None),
            ("03003f53797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69625d5d020507", Some(Some(vec![5,7]))),
            ("03005153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206d73636f726c69622c2056657273696f6e3d312e322e332e345d5d020507", Some(Some(vec![5,7]))),
            ("03004053797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c204d53434f524c49425d5d020507", Some(Some(vec![5,7]))),
            ("03004353797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c2053797374656d2e436f72655d5d020507", None),
            ("03003e53797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206e6f737563685d5d020507", None),
            ("03004253797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b205b53797374656d2e55496e7433322c206d73636f726c69625d205d020507", None),
            ("03004153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e7433325d2c2053797374656d2e436f7265020507", None),
            ("03003653797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e7433325d2020020507", None),
            ("0300352053797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e7433325d020507", Some(Some(vec![5,7]))),
            ("03003353797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e743332020507", None),
            ("03003353797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e496e7433325d020507", None),
            ("0300830153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206d73636f726c69622c2056657273696f6e3d342e302e302e302c2043756c747572653d6e65757472616c2c205075626c69634b6579546f6b656e3d623737613563353631393334653038395d5d020507", Some(Some(vec![5,7]))),
            ("03004153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b2053797374656d2e55496e7433322c206d73636f726c69625d5d020507", Some(Some(vec![5,7]))),
            ("03004153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206d73636f726c6962205d5d020507", Some(Some(vec![5,7]))),
            ("03004153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e743332202c206d73636f726c69625d5d020507", None),
            ("03004253797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b53797374656d2e55496e7433322c53797374656d2e55496e7433325d020507", None),
            ("", None),
            ("03004553797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c206a756e6b5d5d020507", Some(Some(vec![5,7]))),
            ("03004c53797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c2056657273696f6e3d6162635d5d020507", Some(Some(vec![5,7]))),
            ("03005253797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c205075626c69634b6579546f6b656e3d30305d5d020507", Some(Some(vec![5,7]))),
            ("0300820153797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c2056657273696f6e3d342e302e302e302c2043756c747572653d6e65757472616c2c205075626c69634b6579546f6b656e3d623737613563353631393334653038395d5d020507", Some(Some(vec![5,7]))),
            ("03004053797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c5d5d020507", None),
            ("03004b53797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622c2043756c747572653d66725d5d020507", Some(Some(vec![5,7]))),
            ("03004353797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c6d73636f726c69622e646c6c5d5d020507", Some(Some(vec![5,7]))),
            ("03004e53797374656d2e436f6c6c656374696f6e732e47656e657269632e4861736853657460315b5b53797374656d2e55496e7433322c206d73636f726c6962202c2056657273696f6e3d312e305d5d020507", Some(Some(vec![5,7]))),
        ];
        for (hex, want) in rows {
            let b: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            assert_eq!(&decode_uint_hashset(&b).ok(), want, "{hex}");
        }
    }

    /// The trust cache the real game wrote on the developer's PC (TrustCache.Save of an empty set): its bytes decode and encode back,
    /// and on that PC the rewrite's static constructor names the same file. Skipped where the game is not installed.
    #[test]
    fn trust_cache_file_matches_the_game() {
        assert_eq!(decode_uint_hashset(&[2, 0]), Ok(Some(vec![])));
        assert_eq!(encode_uint_hashset(&[]), vec![2, 0]);
        let game = std::path::Path::new(r"F:\SteamLibrary\steamapps\common\People Playground\4d3234dc29bbe861a0163a7ebb272261");
        if cfg!(windows) && game.exists() && std::env::var("COMPUTERNAME").is_ok_and(|n| crate::game::stats::hash128_to_string(crate::game::stats::spooky_hash128(n.as_bytes())) == crate::game::stats::DEV_PC) {
            assert_eq!(std::fs::read(game).unwrap(), vec![2, 0]);
            // The game wrote it on 2026-10-23 under Windows build 26200; the name hashes Environment.OSVersion, so once Windows
            // updates (26300 since) the game's own static constructor names another file. The name is that day's key.
            assert_eq!(TrustCache::with_key(&machine_key("Microsoft Windows NT 10.0.26200.0")).path(), "4d3234dc29bbe861a0163a7ebb272261");
        }
    }
}
