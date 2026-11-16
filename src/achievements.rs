//! Rewrite feature: local achievements (no Steam). The game hands achievements and its Steam stats to Steamworks;
//! the rewrite never connects to Steam (the owner's choice), so StatManager's Steam calls land here instead.
//! The definitions are read at runtime from the user's own Steam schema cache
//! (appcache/stats/UserGameStatsSchema_1118200.bin); nothing from it ships with the rewrite. Unlocks and stat values
//! are kept in ppr-achievements.json next to the executable.
//!
//! Steam's rules kept here: an achievement unlocks once; a progress achievement ("statvalue" of a stat) unlocks when
//! its stat reaches max_val; SetStat on a stat the schema does not define fails.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const APP_ID: &str = "1118200";

#[derive(Clone, Debug, PartialEq)]
pub struct Achievement {
    pub name: String,
    pub title: String,
    pub desc: String,
    pub hidden: bool,
    /// The colour and grey icons, when the user's Steam has them cached.
    pub icon: Option<PathBuf>,
    pub icon_gray: Option<PathBuf>,
    /// (stat, max_val) for a progress achievement.
    pub progress: Option<(String, f64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schema {
    /// (name, is float, default)
    pub stats: Vec<(String, bool, f64)>,
    pub achievements: Vec<Achievement>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Saved {
    stats: BTreeMap<String, f64>,
    /// Name → unlock time (Unix seconds).
    unlocked: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct Store {
    /// None when no Steam schema cache was found.
    pub schema: Option<Schema>,
    saved: Saved,
    /// Where to save; None in tests and replays (memory only).
    path: Option<PathBuf>,
    /// Titles of achievements unlocked since the UI last looked (the toast).
    pub toasts: Vec<String>,
}

/// 1.28.41's Steam stats (the user's UserGameStatsSchema_1118200.bin, copied by hand; every default is 0):
/// (name, FLOAT type, min, maxchange, incrementonly). SetStat enforces these; GetStat/SetStat of the other type fail.
pub const STATS: [(&str, bool, Option<f64>, Option<f64>, bool); 8] = [
    ("BODY_COUNT", false, None, None, false),
    ("RAGDOLL_AIR_TIME", true, None, None, false),
    ("TOTAL_SPAWNED_ITEMS", false, None, None, false),
    ("SPAWNED_HUMANS", false, Some(0.0), None, false),
    ("SGNRSTAT", false, Some(0.0), Some(1.0), true),
    ("WDJZMSTAT", false, Some(0.0), Some(1.0), true),
    ("PACIFISTSTAT", false, Some(0.0), Some(1.0), true),
    ("KEYPADSTAT", false, Some(0.0), Some(1.0), false),
];

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(Default::default);

pub fn store() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// At start-up of the real game (not tests or replays): read the schema from the user's Steam and the saved unlocks.
pub fn init(save_dir: &Path, game: Option<&Path>) {
    let path = save_dir.join("ppr-achievements.json");
    let saved = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let schema = steam_roots(game).into_iter().find_map(|r| load_schema(&r));
    *store() = Store { schema, saved, path: Some(path), toasts: Vec::new() };
}

/// Steam folders to look in: STEAM_DIR, the library holding the game (…/steamapps/common/People Playground), Steam's
/// own SteamPath (HKCU\Software\Valve\Steam), the standard install places.
fn steam_roots(game: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = std::env::var_os("STEAM_DIR").map(PathBuf::from).into_iter().collect();
    roots.extend(game.and_then(|g| g.ancestors().nth(3)).map(Path::to_path_buf));
    roots.extend(registry_steam_path());
    roots.push(r"C:\Program Files (x86)\Steam".into());
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.extend([home.join(".steam/steam"), home.join(".local/share/Steam"), home.join("Library/Application Support/Steam")]);
    }
    roots
}

pub fn load_schema(steam: &Path) -> Option<Schema> {
    let bytes = std::fs::read(steam.join(format!("appcache/stats/UserGameStatsSchema_{APP_ID}.bin"))).ok()?;
    let icons = steam.join("appcache/librarycache").join(APP_ID);
    parse_schema(&bytes, &icons)
}

/// Steam's binary KeyValues.
#[derive(Debug)]
enum Kv {
    Map(Vec<(String, Kv)>),
    Str(String),
    Int(i64),
    Float(f32),
}

impl Kv {
    fn get(&self, k: &str) -> Option<&Kv> {
        match self {
            Kv::Map(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    fn path(&self, p: &[&str]) -> Option<&Kv> {
        p.iter().try_fold(self, |v, k| v.get(k))
    }
    fn str(&self) -> Option<&str> {
        if let Kv::Str(s) = self { Some(s) } else { None }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Kv::Int(i) => Some(*i as f64),
            Kv::Float(f) => Some(*f as f64),
            Kv::Str(s) => s.parse().ok(),
            Kv::Map(_) => None,
        }
    }
}

fn parse_kv(b: &[u8], i: &mut usize) -> Option<Kv> {
    let mut out = Vec::new();
    loop {
        let t = *b.get(*i)?;
        *i += 1;
        if t == 8 {
            return Some(Kv::Map(out));
        }
        let cstr = |i: &mut usize| {
            let end = *i + b.get(*i..)?.iter().position(|&c| c == 0)?;
            let s = String::from_utf8_lossy(&b[*i..end]).into_owned();
            *i = end + 1;
            Some(s)
        };
        let key = cstr(i)?;
        let mut take = |n: usize| {
            let s = b.get(*i..*i + n)?;
            *i += n;
            Some(s)
        };
        let v = match t {
            0 => parse_kv(b, i)?,
            1 => Kv::Str(cstr(i)?),
            2 | 4 | 6 => Kv::Int(i32::from_le_bytes(take(4)?.try_into().ok()?) as i64),
            3 => Kv::Float(f32::from_le_bytes(take(4)?.try_into().ok()?)),
            7 | 10 => Kv::Int(i64::from_le_bytes(take(8)?.try_into().ok()?)),
            _ => return None,
        };
        out.push((key, v));
    }
}

fn parse_schema(bytes: &[u8], icons: &Path) -> Option<Schema> {
    let root = parse_kv(bytes, &mut 0)?;
    let Kv::Map(stats) = root.path(&[APP_ID, "stats"])? else { return None };
    let icon = |a: &Kv, k: &str| a.path(&["display", k]).and_then(Kv::str).map(|f| icons.join(f)).filter(|p| p.is_file());
    let mut schema = Schema::default();
    for (_, s) in stats {
        let ty = s.get("type").and_then(|t| t.str().map(str::to_owned).or_else(|| t.num().map(|n| n.to_string())));
        match ty.as_deref() {
            // Type 4 is ACHIEVEMENTS in schemas that store it as a number.
            Some("ACHIEVEMENTS" | "4") => {
                let Some(Kv::Map(bits)) = s.get("bits") else { continue };
                for (_, a) in bits {
                    let text = |k: &str| a.path(&["display", k, "english"]).and_then(Kv::str).unwrap_or_default().to_owned();
                    let progress = a.get("progress").and_then(|p| {
                        let stat = p.path(&["value", "operand1"])?.str()?.to_owned();
                        Some((stat, p.get("max_val")?.num()?))
                    });
                    schema.achievements.push(Achievement {
                        name: a.get("name").and_then(Kv::str)?.to_owned(),
                        title: text("name"),
                        desc: text("desc"),
                        hidden: a.path(&["display", "hidden"]).and_then(Kv::num).unwrap_or(0.0) != 0.0,
                        icon: icon(a, "icon"),
                        icon_gray: icon(a, "icon_gray"),
                        progress,
                    });
                }
            }
            Some(t @ ("INT" | "FLOAT" | "AVGRATE" | "1" | "2" | "3")) => {
                let Some(name) = s.get("name").and_then(Kv::str) else { continue };
                let default = s.get("default").and_then(Kv::num).unwrap_or(0.0);
                schema.stats.push((name.to_owned(), !matches!(t, "INT" | "1"), default));
            }
            _ => {}
        }
    }
    Some(schema)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Store {
    pub fn unlocked(&self, name: &str) -> Option<u64> {
        self.saved.unlocked.get(name).copied()
    }

    /// Achievement.Trigger. False when there is no schema or the name is not in it (Steam's failure).
    pub fn trigger(&mut self, name: &str) -> bool {
        let Some(a) = self.schema.as_ref().and_then(|s| s.achievements.iter().find(|a| a.name == name)) else { return false };
        if !self.saved.unlocked.contains_key(name) {
            let title = a.title.clone();
            self.saved.unlocked.insert(name.to_owned(), now());
            self.toasts.push(title);
            self.save();
        }
        true
    }

    /// SteamUserStats.GetStatInt / GetStatFloat (Facepunch: GetStat's false is 0, never a throw): the stored value, else
    /// the default 0; None when Steam's GetStat fails (an unknown stat, or `float` not the stat's type).
    pub fn get(&self, name: &str, float: bool) -> Option<f64> {
        STATS.iter().find(|s| s.0 == name && s.1 == float)?;
        Some(self.saved.stats.get(name).copied().unwrap_or(0.0))
    }

    /// The stored value as is (fn-check: the recorded Steam value as a case starts).
    pub fn seed(&mut self, name: &str, value: f64) {
        self.saved.stats.insert(name.to_owned(), value);
    }

    /// SteamUserStats.SetStat(name, int or float), then the progress achievements on that stat. False, nothing stored,
    /// for an unknown stat, the other type, a value under min, a fall on an incrementonly stat, or a step over maxchange.
    pub fn set(&mut self, name: &str, float: bool, value: f64) -> bool {
        let Some(&(_, _, min, maxchange, increment_only)) = STATS.iter().find(|s| s.0 == name && s.1 == float) else { return false };
        let old = self.saved.stats.get(name).copied().unwrap_or(0.0);
        if min.is_some_and(|m| value < m) || (increment_only && value < old) || maxchange.is_some_and(|m| (value - old).abs() > m) {
            return false;
        }
        self.saved.stats.insert(name.to_owned(), value);
        let reached: Vec<String> = self.schema.iter().flat_map(|s| &s.achievements)
            .filter(|a| a.progress.as_ref().is_some_and(|(s, max)| s == name && value >= *max))
            .map(|a| a.name.clone())
            .collect();
        for a in reached {
            self.trigger(&a);
        }
        self.save();
        true
    }

    fn save(&self) {
        if let Some(path) = &self.path {
            if let Ok(t) = serde_json::to_string_pretty(&self.saved) {
                let _ = std::fs::write(path, t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schema in Steam's binary KeyValues: one stat, one plain and one progress achievement.
    fn schema_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        let open = |b: &mut Vec<u8>, k: &str| { b.push(0); b.extend(k.as_bytes()); b.push(0); };
        let s = |b: &mut Vec<u8>, k: &str, v: &str| { b.push(1); b.extend(k.as_bytes()); b.push(0); b.extend(v.as_bytes()); b.push(0); };
        let int = |b: &mut Vec<u8>, k: &str, v: i32| { b.push(2); b.extend(k.as_bytes()); b.push(0); b.extend(v.to_le_bytes()); };
        open(&mut b, APP_ID);
        open(&mut b, "stats");
        open(&mut b, "3");
        s(&mut b, "type", "ACHIEVEMENTS");
        open(&mut b, "bits");
        for (bit, name, title, max) in [("0", "BH", "Black hole", None), ("1", "KILL_PERSON", "Murderer", Some(1))] {
            open(&mut b, bit);
            s(&mut b, "name", name);
            open(&mut b, "display");
            open(&mut b, "name");
            s(&mut b, "english", title);
            b.push(8);
            int(&mut b, "hidden", 0);
            b.push(8);
            if let Some(max) = max {
                open(&mut b, "progress");
                int(&mut b, "max_val", max);
                open(&mut b, "value");
                s(&mut b, "operand1", "BODY_COUNT");
                b.extend([8, 8]);
            }
            b.push(8);
        }
        b.extend([8, 8]);
        open(&mut b, "4");
        s(&mut b, "type", "INT");
        s(&mut b, "name", "BODY_COUNT");
        int(&mut b, "default", 0);
        b.extend([8, 8, 8, 8]);
        b
    }

    #[test]
    fn schema_stats_and_unlocks() {
        let schema = parse_schema(&schema_bytes(), Path::new("")).unwrap();
        assert_eq!(schema.achievements.len(), 2);
        assert_eq!(schema.achievements[1].progress, Some(("BODY_COUNT".into(), 1.0)));
        assert_eq!(schema.stats, vec![("BODY_COUNT".into(), false, 0.0)]);
        let mut st = Store { schema: Some(schema), ..Default::default() };
        assert!(!st.trigger("NOPE"));
        assert!(st.trigger("BH") && st.trigger("BH"));
        assert_eq!(st.toasts, ["Black hole"]);
        assert!(!st.set("NOPE", false, 1.0));
        assert_eq!(st.get("BODY_COUNT", false), Some(0.0));
        assert_eq!(st.get("BODY_COUNT", true), None);
        assert!(!st.set("BODY_COUNT", true, 1.0));
        assert!(!st.set("KEYPADSTAT", false, 5.0));
        assert!(st.set("BODY_COUNT", false, 1.0));
        assert!(st.unlocked("KILL_PERSON").is_some());
        assert_eq!(st.toasts.len(), 2);
    }

    /// The user's real schema, when their Steam has it.
    #[test]
    fn installed_steam_schema() {
        let Some(s) = steam_roots(None).into_iter().find_map(|r| load_schema(&r)) else {
            println!("no Steam schema cache for {APP_ID}; skipped");
            return;
        };
        assert!(s.achievements.iter().any(|a| a.name == "BH"));
        assert!(s.stats.iter().any(|t| t.0 == "BODY_COUNT"));
        // The hand copy (STATS) matches the user's schema: name and type.
        for t in &s.stats {
            assert!(STATS.iter().any(|x| x.0 == t.0 && x.1 == t.1), "{t:?} not in STATS");
        }
        // The hand copy matches the user's schema (name, type).
        for t in &s.stats {
            assert!(STATS.iter().any(|x| x.0 == t.0 && x.1 == t.1), "{t:?} not in STATS");
        }
    }
}

/// HKCU\Software\Valve\Steam's SteamPath (a REG_SZ), when Steam set it.
#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, value) = (wide(r"Software\Valve\Steam"), wide("SteamPath"));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the strings end in 0 and `len` is buf's size in bytes.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) };
    if status != 0 {
        return None;
    }
    buf.truncate((len as usize / 2).saturating_sub(1));
    Some(PathBuf::from(String::from_utf16_lossy(&buf)))
}

#[cfg(not(windows))]
fn registry_steam_path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod registry_tests {
    /// SteamPath, when Steam set it, names a folder that exists.
    #[test]
    fn steam_path_exists_when_set() {
        if let Some(p) = super::registry_steam_path() {
            assert!(p.is_dir(), "{}", p.display());
        }
    }
}
