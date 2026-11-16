//! StatManager and NonSteamStatManager, both static classes. Rewrite feature (local achievements, no Steam): Steam's
//! user stats and achievements are the local store (`crate::achievements`), and SteamworksInitialiser.IsInitialised is
//! true for it. NonSteamStatManager keeps its StatCollection in the `stats` file (Ceras, byte for byte), in the
//! rewrite's own save folder: never the game's folder.
#![allow(non_snake_case)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, MutexGuard};

use crate::ui::data::{logger_log_error, logger_log_error_format, logger_log_warning, machine_name};

/// StatManager.Stat..cctor: the nine Stats, as their API names.
pub const UNIQUE_SPAWN_COUNT: &str = "UNIQUE_SPAWN_COUNT";
pub const BODY_COUNT: &str = "BODY_COUNT";
pub const RAGDOLL_AIR_TIME: &str = "RAGDOLL_AIR_TIME";
pub const TOTAL_SPAWNED_ITEMS: &str = "TOTAL_SPAWNED_ITEMS";
pub const SPAWNED_HUMANS: &str = "SPAWNED_HUMANS";
pub const SGNR: &str = "SGNRSTAT";
pub const WDJZM: &str = "WDJZMSTAT";
pub const PACIFIST: &str = "PACIFISTSTAT";
pub const KEYPAD: &str = "KEYPADSTAT";

/// StatManager.Stat: a Steam stat, named by its API name.
#[derive(Default, Clone, Debug)]
pub struct Stat {
    /// null (None) until the constructor's apiName.
    pub APIName: Option<String>,
}

impl Stat {
    /// Stat..ctor(apiName): APIName = apiName.
    pub fn new(api_name: &str) -> Self {
        Stat { APIName: Some(api_name.to_string()) }
    }

    /// Stat.ToString: APIName.
    pub fn ToString(&self) -> Option<String> {
        self.APIName.clone()
    }

    /// Stat.Equals(object): `obj is Stat stat` and the API names are equal.
    pub fn Equals(&self, obj: &dyn std::any::Any) -> bool {
        match obj.downcast_ref::<Stat>() {
            Some(stat) => self.APIName == stat.APIName,
            None => false,
        }
    }

    /// Stat.GetHashCode: -1814423452 + EqualityComparer<string>.Default.GetHashCode(APIName) (Mono's string hash; 0 for null), wrapping.
    pub fn GetHashCode(&self) -> i32 {
        (-1814423452i32).wrapping_add(self.APIName.as_deref().map_or(0, super::decal_renderer::string_hash))
    }
}

/// StatManager.MaxAttempts
const MAX_ATTEMPTS: i16 = 64;

/// StatManager's static fields.
#[derive(Default)]
pub struct StatManager {
    integers: HashMap<String, i32>,
    floats: HashMap<String, f32>,
    achievementsUnlockedThisSession: HashSet<String>,
    attempts: i16,
}

static STAT_MANAGER: LazyLock<Mutex<StatManager>> = LazyLock::new(Default::default);

fn manager() -> MutexGuard<'static, StatManager> {
    STAT_MANAGER.lock().unwrap_or_else(|e| e.into_inner())
}

/// SteamworksInitialiser.IsInitialised: the local store stands in for Steam (rewrite feature).
pub const STEAMWORKS_INITIALISED: bool = true;

impl StatManager {
    /// StatManager.SteamManagerCheck
    fn steam_manager_check(&mut self) -> bool {
        if self.attempts >= MAX_ATTEMPTS {
            return false;
        }
        if !STEAMWORKS_INITIALISED {
            self.attempts += 1;
            if self.attempts >= MAX_ATTEMPTS {
                logger_log_error(crate::il!(str "StatManager::SteamManagerCheck" 0));
            }
            return false;
        }
        true
    }

    /// StatManager.ClearCache
    fn clear_cache(&mut self) {
        self.integers.clear();
        self.floats.clear();
    }

    /// StatManager.GetFloat: SteamUserStats.GetStatFloat, cached.
    fn get_float(&mut self, stat: &str) -> f32 {
        if !self.steam_manager_check() || self.attempts >= MAX_ATTEMPTS {
            return f32::NAN;
        }
        if let Some(&v) = self.floats.get(stat) {
            return v;
        }
        // Facepunch's GetStatFloat gives 0 when Steam's GetStat fails (an unknown stat, an INT stat); it never throws, so
        // the catch (attempts++, "Stat retrieval failed") is not taken: fngb5's IncrementFloat on BODY_COUNT counts one attempt.
        let v = crate::achievements::store().get(stat, true).unwrap_or(0.0) as f32;
        self.floats.insert(stat.to_string(), v);
        v
    }

    /// StatManager.GetInt: SteamUserStats.GetStatInt, cached (a failed GetStat is 0, as GetFloat's).
    fn get_int(&mut self, stat: &str) -> i32 {
        if !self.steam_manager_check() || self.attempts >= MAX_ATTEMPTS {
            return -1;
        }
        if let Some(&v) = self.integers.get(stat) {
            return v;
        }
        let v = crate::achievements::store().get(stat, false).unwrap_or(0.0) as i32;
        self.integers.insert(stat.to_string(), v);
        v
    }

    /// StatManager.SetFloat (StoreStats: the store saves in `set`).
    fn set_float(&mut self, stat: &str, value: f32) {
        if self.steam_manager_check() && self.attempts < MAX_ATTEMPTS && !value.is_nan() && !crate::achievements::store().set(stat, true, value as f64) {
            self.attempts += 1;
            logger_log_warning(&format!("Stat setting failed: {stat}"));
        }
    }

    /// StatManager.SetInt
    fn set_int(&mut self, stat: &str, value: i32) {
        if self.steam_manager_check() && self.attempts < MAX_ATTEMPTS && !crate::achievements::store().set(stat, false, value as f64) {
            self.attempts += 1;
            logger_log_warning(&format!("Stat setting failed: {stat}"));
        }
    }
}

/// StatManager.UnlockAchievement: once per session; Debug.Log("Achievement unlocked: " + name) goes to the console;
/// Achievement.Trigger failing is logged (Logger.LogErrorFormat).
pub fn unlock_achievement(name: &str) {
    let mut m = manager();
    if m.achievementsUnlockedThisSession.contains(name) {
        return;
    }
    m.achievementsUnlockedThisSession.insert(name.to_string());
    if !crate::achievements::store().trigger(name) {
        logger_log_error_format(crate::il!(str "StatManager::UnlockAchievement" 1), &[name, "Achievement.Trigger failed"]);
    }
}

/// StatManager.IncrementInteger(stat, value)
pub fn increment_integer(stat: &str, value: i32) {
    let mut m = manager();
    if m.steam_manager_check() && m.attempts < MAX_ATTEMPTS {
        let n = m.get_int(stat) + value;
        m.set_int(stat, n);
        m.clear_cache();
    }
}

/// StatManager.IncrementFloat(stat, value)
pub fn increment_float(stat: &str, value: f32) {
    let mut m = manager();
    if m.steam_manager_check() && m.attempts < MAX_ATTEMPTS {
        let n = m.get_float(stat) + value;
        m.set_float(stat, n);
        m.clear_cache();
    }
}

/// fn-check: StatManager.attempts as a case starts (the caches are empty: every Increment* ends with ClearCache).
pub fn check_seed(attempts: i16) {
    let mut m = manager();
    m.attempts = attempts;
    m.clear_cache();
}

pub fn check_attempts() -> i16 {
    manager().attempts
}

/// StatManager.SteamManagerCheck
pub fn steam_manager_check() -> bool {
    manager().steam_manager_check()
}

/// StatManager.SetFloat
pub fn set_float(stat: &str, value: f32) {
    manager().set_float(stat, value);
}

/// StatManager.GetFloat
pub fn get_float(stat: &str) -> f32 {
    manager().get_float(stat)
}

/// StatManager.GetInt
pub fn get_int(stat: &str) -> i32 {
    manager().get_int(stat)
}

/// Global.Start (execution order 50, after StatLoader.Start at 30), with SteamworksInitialiser.IsInitialised: the
/// local BODY_COUNT and TOTAL_SPAWNED_ITEMS raised to the Steam ones (Mathf.Max(int, float)).
pub fn global_start() {
    if STEAMWORKS_INITIALISED {
        for stat in [BODY_COUNT, TOTAL_SPAWNED_ITEMS] {
            let steam = get_int(stat) as f32;
            with_stats(|s| {
                let local = s.get_stat(stat, 0.0);
                s.set_stat(stat, if steam > local { steam } else { local });
            });
        }
    }
}

/// Global.LastKillTime (static: it outlives a level).
static LAST_KILL_TIME: Mutex<f32> = Mutex::new(0.0);

pub fn last_kill_time() -> f32 {
    *LAST_KILL_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_last_kill_time(t: f32) {
    *LAST_KILL_TIME.lock().unwrap_or_else(|e| e.into_inner()) = t;
}

/// NonSteamStatManager.StatCollection: Stats keeps the Dictionary's insertion order (nothing is ever removed).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct StatCollection {
    pub Identifier: String,
    pub Stats: Vec<(String, f32)>,
}

impl StatCollection {
    /// StatCollection..ctor: Identifier = Hash128.Compute(Environment.MachineName).ToString().
    pub fn new() -> Self {
        StatCollection { Identifier: hash128_to_string(spooky_hash128(machine_name().as_bytes())), Stats: Vec::new() }
    }

    /// StatCollection.GetStat(key, fallback): a missing key is added with the fallback.
    pub fn get_stat(&mut self, key: &str, fallback: f32) -> f32 {
        if let Some(&(_, v)) = self.Stats.iter().find(|(k, _)| k == key) {
            return v;
        }
        self.Stats.push((key.to_string(), fallback));
        fallback
    }

    /// StatCollection.SetStat
    pub fn set_stat(&mut self, key: &str, value: f32) {
        match self.Stats.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.Stats.push((key.to_string(), value)),
        }
    }

    /// StatCollection.HasStat
    #[allow(dead_code)]
    pub fn has_stat(&self, key: &str) -> bool {
        self.Stats.iter().any(|(k, _)| k == key)
    }

    /// StatCollection.Increment(key, delta)
    pub fn increment(&mut self, key: &str, delta: f32) {
        let stat = self.get_stat(key, 0.0);
        self.set_stat(key, stat + delta);
    }

    /// CerasSerializer.Serialize<StatCollection>: the reference marker NewValueSameType (-3 + bias 5) for the object
    /// and for the Dictionary, its count, each key (UTF-8 byte count + 1, bytes) and float, then Identifier. Ceras
    /// orders the members Stats, Identifier.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![2, 2];
        var_int(&mut out, self.Stats.len() as u64);
        for (k, v) in &self.Stats {
            string(&mut out, k);
            out.extend(v.to_le_bytes());
        }
        string(&mut out, &self.Identifier);
        out
    }

    /// CerasSerializer.Deserialize<StatCollection>: the inverse of `serialize` (bytes after it are not read); `None` is a
    /// null StatCollection. Any other reference marker, a duplicate key (Dictionary.Add) or running out of bytes is an exception.
    /// A null Identifier is `None` (it never equals the machine's).
    pub fn deserialize(b: &[u8]) -> Result<Option<(Option<String>, Vec<(String, f32)>)>, String> {
        let mut r = CerasReader { b, at: 0 };
        for what in ["StatCollection", "Dictionary"] {
            match r.var_int()? as i64 - 5 {
                -3 => {}
                -1 if what == "StatCollection" => return Ok(None),
                -1 => unimplemented!("mod-only: a stats file with a null {what} (NullReferenceException outside the catch)"),
                m => return Err(format!("{what}: reference marker {m}")),
            }
        }
        let n = r.var_int()?;
        let mut Stats: Vec<(String, f32)> = Vec::new();
        for _ in 0..n {
            let k = r.string()?.ok_or("a null Dictionary key (ArgumentNullException)")?;
            let v = f32::from_le_bytes(r.take(4)?.try_into().unwrap());
            if Stats.iter().any(|(x, _)| *x == k) {
                return Err(format!("duplicate key {k}"));
            }
            Stats.push((k, v));
        }
        Ok(Some((r.string()?, Stats)))
    }
}

struct CerasReader<'a> {
    b: &'a [u8],
    at: usize,
}

impl CerasReader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("IndexOutOfRangeException")?;
        self.at += n;
        Ok(s)
    }

    /// SerializerBinary.ReadVarInt(bits 32)
    fn var_int(&mut self) -> Result<u64, String> {
        let (mut shift, mut v) = (0u32, 0u64);
        loop {
            let byte = self.take(1)?[0];
            v |= ((byte & 0x7f) as u64).wrapping_shl(shift);
            if shift > 32 {
                return Err("Malformed VarInt".into());
            }
            if byte & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }

    /// SerializerBinary.ReadString: ReadUInt32Bias(1) as int, -1 is null; UTF8Encoding.GetString.
    fn string(&mut self) -> Result<Option<String>, String> {
        let n = (self.var_int()? as u32 as i32).wrapping_sub(1);
        if n == -1 {
            return Ok(None);
        }
        let n = usize::try_from(n).map_err(|_| "ArgumentOutOfRangeException")?;
        Ok(Some(String::from_utf8_lossy(self.take(n)?).into_owned()))
    }
}

/// SerializerBinary.WriteVarInt
fn var_int(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        out.push(if v != 0 { byte | 0x80 } else { byte });
        if v == 0 {
            return;
        }
    }
}

/// SerializerBinary.WriteString: UTF-8 byte count with bias 1, then the bytes.
fn string(out: &mut Vec<u8>, s: &str) {
    var_int(out, s.len() as u64 + 1);
    out.extend(s.as_bytes());
}

/// NonSteamStatManager's static fields and the save folder (None in tests and replays: no file is touched).
struct NonSteam {
    Stats: Option<StatCollection>,
    dir: Option<PathBuf>,
}

static NON_STEAM: LazyLock<Mutex<NonSteam>> = LazyLock::new(|| Mutex::new(NonSteam { Stats: None, dir: None }));

fn non_steam() -> MutexGuard<'static, NonSteam> {
    NON_STEAM.lock().unwrap_or_else(|e| e.into_inner())
}

/// At start-up of the real game: `stats` lives in the rewrite's save folder.
pub fn init(save_dir: &std::path::Path) {
    non_steam().dir = Some(save_dir.to_path_buf());
}

/// NonSteamStatManager.Stats (created on first use, like the static initialiser).
pub fn with_stats<R>(f: impl FnOnce(&mut StatCollection) -> R) -> R {
    let mut n = non_steam();
    f(n.Stats.get_or_insert_with(StatCollection::new))
}

/// The rewrite's save folder (None in tests and replays).
pub fn save_dir() -> Option<PathBuf> {
    non_steam().dir.clone()
}

/// NonSteamStatManager.DefaultPath ("stats") in the save folder.
fn stats_path() -> Option<PathBuf> {
    non_steam().dir.as_ref().map(|d| d.join("stats"))
}

/// NonSteamStatManager.SaveToFile(DefaultPath): File.WriteAllBytes throws an IOException (or its access / missing-path kin), which
/// leaves the caller (Global.OnDestroy) and is returned as Debug.LogException's text.
pub fn save_to_file() -> Result<(), String> {
    let Some(path) = stats_path() else { return Ok(()) };
    let bytes = with_stats(|s| s.serialize());
    std::fs::write(&path, bytes).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!("UnauthorizedAccessException: Access to the path \"{}\" is denied.", path.display()),
        std::io::ErrorKind::NotFound => format!("DirectoryNotFoundException: Could not find a part of the path \"{}\".", path.display()),
        _ => format!("IOException: {} ({e})", path.display()),
    })
}

/// NonSteamStatManager.LoadFromFile(DefaultPath): a corrupt file or one from another machine is deleted
/// (Debug.LogWarning, console only). A file holding a null StatCollection throws at statCollection.Identifier, outside the
/// catch: the exception's text is returned for StatLoader.Start's caller to log, and the file stays.
pub fn load_from_file() -> Option<String> {
    let path = stats_path()?;
    let bytes = std::fs::read(&path).ok()?;
    let (identifier, stats) = match StatCollection::deserialize(&bytes) {
        Ok(Some(s)) => s,
        Ok(None) => return Some("NullReferenceException: Object reference not set to an instance of an object".into()),
        Err(_) => {
            // Debug.LogWarning("Corrupt stats file. File will be deleted.")
            let _ = std::fs::remove_file(&path);
            return None;
        }
    };
    match identifier {
        Some(id) if with_stats(|s| s.Identifier == id) => non_steam().Stats = Some(StatCollection { Identifier: id, Stats: stats }),
        _ => {
            // Debug.LogWarning("Attempted to load stats file from a different machine. File will be deleted.")
            let _ = std::fs::remove_file(&path);
        }
    }
    None
}

/// Hash128.ToString: the two 64-bit halves' bytes in memory order, as lowercase hex.
/// Hash128.Compute of the developer's PC name: the Identifier in its `stats` file (the tests check files from that PC).
pub(crate) const DEV_PC: &str = "c24c09c2265c8cdc9625df5a93846fae";

pub(crate) fn hash128_to_string((h1, h2): (u64, u64)) -> String {
    h1.to_le_bytes().iter().chain(&h2.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

/// Hash128.Compute(string) (Hash128_CUSTOM_ComputeFromString, 180962920): SpookyHash::Hash128 (180530760) of the
/// string's UTF-8 bytes with seeds 0, 0.
pub(crate) fn spooky_hash128(msg: &[u8]) -> (u64, u64) {
    if msg.len() >= 192 {
        unimplemented!("mod-only: SpookyHash::Hash128 of 192 bytes or more (the long path)");
    }
    spooky_short(msg, 0, 0)
}

const SC_CONST: u64 = 0xdeadbeefdeadbeef;

/// SpookyHash::Short (180530cf0)
fn spooky_short(msg: &[u8], seed1: u64, seed2: u64) -> (u64, u64) {
    let word = |i: usize| u64::from_le_bytes(msg[i..i + 8].try_into().unwrap());
    let length = msg.len();
    let mut rem = length % 32;
    let (mut a, mut b, mut c, mut d) = (seed1, seed2, SC_CONST, SC_CONST);
    let mut i = 0;
    if length > 15 {
        while i + 32 <= length {
            c = c.wrapping_add(word(i));
            d = d.wrapping_add(word(i + 8));
            short_mix(&mut a, &mut b, &mut c, &mut d);
            a = a.wrapping_add(word(i + 16));
            b = b.wrapping_add(word(i + 24));
            i += 32;
        }
        if rem >= 16 {
            c = c.wrapping_add(word(i));
            d = d.wrapping_add(word(i + 8));
            short_mix(&mut a, &mut b, &mut c, &mut d);
            i += 16;
            rem -= 16;
        }
    }
    d = d.wrapping_add((length as u64) << 56);
    let le = |s: &[u8]| s.iter().rev().fold(0u64, |v, &x| (v << 8) | x as u64);
    let tail = &msg[i..];
    match rem {
        0 => {
            c = c.wrapping_add(SC_CONST);
            d = d.wrapping_add(SC_CONST);
        }
        1..=8 => c = c.wrapping_add(le(&tail[..rem])),
        _ => {
            d = d.wrapping_add(le(&tail[8..rem]));
            c = c.wrapping_add(le(&tail[..8]));
        }
    }
    short_end(&mut a, &mut b, &mut c, &mut d);
    (a, b)
}

fn short_mix(h0: &mut u64, h1: &mut u64, h2: &mut u64, h3: &mut u64) {
    let step = |x: &mut u64, y: u64, z: &mut u64, k: u32| {
        *x = x.rotate_left(k).wrapping_add(y);
        *z ^= *x;
    };
    step(h2, *h3, h0, 50);
    step(h3, *h0, h1, 52);
    step(h0, *h1, h2, 30);
    step(h1, *h2, h3, 41);
    step(h2, *h3, h0, 54);
    step(h3, *h0, h1, 48);
    step(h0, *h1, h2, 38);
    step(h1, *h2, h3, 37);
    step(h2, *h3, h0, 62);
    step(h3, *h0, h1, 34);
    step(h0, *h1, h2, 5);
    step(h1, *h2, h3, 36);
}

fn short_end(h0: &mut u64, h1: &mut u64, h2: &mut u64, h3: &mut u64) {
    let step = |x: &mut u64, y: &mut u64, k: u32| {
        *x ^= *y;
        *y = y.rotate_left(k);
        *x = x.wrapping_add(*y);
    };
    step(h3, h2, 15);
    step(h0, h3, 52);
    step(h1, h0, 26);
    step(h2, h1, 51);
    step(h3, h2, 28);
    step(h0, h3, 9);
    step(h1, h0, 47);
    step(h2, h1, 54);
    step(h3, h2, 32);
    step(h0, h3, 25);
    step(h1, h0, 63);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real game's `stats` file (157 bytes, written by the game on the developer's PC) round-trips byte for
    /// byte, and its Identifier is Hash128.Compute of that PC's name (DEV_PC).
    #[test]
    fn stats_file_round_trip() {
        let hex = "0202060b424f44595f434f554e5400b828460b57495245535f4c4149440000e841134e55434c4541525f4558504c4f53494f4e530000000013434f4e5452415054494f4e535f53415645440000000011454e5449544945535f524556495645440000154314544f54414c5f535041574e45445f4954454d530030dd45216332346330396332323635633863646339363235646635613933383436666165";
        let hex: String = hex.chars().filter(|c| !c.is_whitespace() && *c != '\\').collect();
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        assert_eq!(bytes.len(), 157);
        let (id, stats) = StatCollection::deserialize(&bytes).unwrap().unwrap();
        let s = StatCollection { Identifier: id.unwrap(), Stats: stats };
        assert_eq!(s.Stats[0], ("BODY_COUNT".to_string(), 10798.0));
        assert_eq!(s.Stats.len(), 6);
        assert_eq!(s.serialize(), bytes);
        assert_eq!(s.Identifier, DEV_PC);
        // A null StatCollection (reference marker -1).
        assert_eq!(StatCollection::deserialize(&[4]), Ok(None));
    }

    #[test]
    fn stat_members() {
        let a = Stat::new(SGNR);
        assert_eq!(a.ToString().as_deref(), Some("SGNRSTAT"));
        assert!(a.Equals(&Stat::new("SGNRSTAT")) && !a.Equals(&Stat::new(WDJZM)) && !a.Equals(&"SGNRSTAT"));
        assert_eq!(Stat::new("").GetHashCode(), -1442566302);
        assert_eq!(Stat::new("ab").GetHashCode(), -720792917);
        assert_eq!(a.GetHashCode(), Stat::new("SGNRSTAT").GetHashCode());
    }

    #[test]
    fn get_stat_adds_the_fallback() {
        let mut s = StatCollection { Identifier: String::new(), Stats: Vec::new() };
        s.increment("WIRES_LAID", 1.0);
        s.increment("WIRES_LAID", 1.0);
        assert_eq!(s.get_stat("X", 3.0), 3.0);
        assert_eq!(s.Stats, vec![("WIRES_LAID".to_string(), 2.0), ("X".to_string(), 3.0)]);
    }
}
