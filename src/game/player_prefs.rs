//! PlayerPrefs (UnityPlayer's Windows storage) and UserPreferenceManager.LoadFromLegacyStorage, which reads it.
//!
//! A value is a registry value under HKEY_CURRENT_USER\Software\<companyName>\<productName>, named `<key>_h<hash>`, where
//! hash is djb2 with each byte XORed in (the native loop sign-extends the byte; keys here are ASCII). Verified against the
//! values stored on this machine. Encodings, from PlayerPrefs::SetString / SetInt / SetFloat in UnityPlayer.dll: a string
//! is its UTF-8 bytes and a NUL (REG_BINARY); an int is REG_DWORD; a float is the double of the float (SetFloat widens it
//! with cvtss2sd and writes 8 bytes). The getters (GetString_Impl, GetNumber_Impl<int,int>, GetNumber_Impl<float,double>,
//! HasKey) read the hashed name first and the plain `<key>` name when that fails.
#![allow(non_snake_case)]

use super::Prefs;

/// ProjectSettings.asset: companyName and productName.
const COMPANY_NAME: &str = "Studio Minus";
const PRODUCT_NAME: &str = "People Playground";

/// The registry name of a PlayerPrefs key: djb2 with each byte XORed in, as Unity hashes it.
pub fn value_name(key: &str) -> String {
    let mut h: u32 = 5381;
    for &b in key.as_bytes() {
        h = h.wrapping_mul(33) ^ (b as i8 as i32 as u32);
    }
    format!("{key}_h{h}")
}

/// The game's PlayerPrefs store: the typed reads Unity's PlayerPrefs getters give (None keeps the caller's default).
pub trait PlayerPrefsStore {
    /// PlayerPrefs.GetString's stored value.
    fn string(&self, key: &str) -> Option<String>;
    /// PlayerPrefs.GetFloat's stored value.
    fn float(&self, key: &str) -> Option<f32>;
    /// PlayerPrefs.HasKey.
    fn has_key(&self, key: &str) -> bool;
}

/// The registry values of the PlayerPrefs key by exact value name: their registry type and bytes.
pub trait RegistryValues {
    fn raw(&self, name: &str) -> Option<(u32, Vec<u8>)>;
}

const REG_SZ: u32 = 1;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;

/// GetNumber_Impl's read: RegQueryValueExA into a buffer of `capacity` bytes on `<key>_h<hash>`; when that fails (absent, or more
/// data than the buffer holds: ERROR_MORE_DATA) the plain `<key>` is read the same way.
fn read_capped<R: RegistryValues + ?Sized>(r: &R, key: &str, capacity: usize) -> Option<(u32, Vec<u8>)> {
    match r.raw(&value_name(key)) {
        Some(v) if v.1.len() <= capacity => Some(v),
        _ => r.raw(key).filter(|v| v.1.len() <= capacity),
    }
}

/// PlayerPrefs::GetString (GetString_Impl): the first query (hashed name, else the plain one) sizes the value; a size of 0 keeps the
/// default. REG_BINARY: the bytes up to the first NUL. REG_SZ: the same, but only when every byte is below 0x80. Any other type
/// keeps the default.
pub fn registry_string<R: RegistryValues + ?Sized>(r: &R, key: &str) -> Option<String> {
    let (kind, bytes) = r.raw(&value_name(key)).or_else(|| r.raw(key))?;
    if bytes.is_empty() {
        return None;
    }
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    match kind {
        REG_BINARY => Some(String::from_utf8_lossy(&bytes[..end]).into_owned()),
        REG_SZ if bytes.iter().all(|&b| b < 0x80) => Some(String::from_utf8_lossy(&bytes[..end]).into_owned()),
        _ => None,
    }
}

/// PlayerPrefs::GetFloat (GetNumber_Impl<float,double>): REG_BINARY or REG_DWORD of exactly 8 bytes, a double, as a float.
pub fn registry_float<R: RegistryValues + ?Sized>(r: &R, key: &str) -> Option<f32> {
    let (kind, bytes) = read_capped(r, key, 8)?;
    ((kind == REG_BINARY || kind == REG_DWORD) && bytes.len() == 8).then(|| f64::from_le_bytes(bytes.try_into().unwrap()) as f32)
}

/// PlayerPrefs::GetInt (GetNumber_Impl<int,int>): REG_BINARY or REG_DWORD of exactly 4 bytes.
#[allow(dead_code)]
pub fn registry_int<R: RegistryValues + ?Sized>(r: &R, key: &str) -> Option<i32> {
    let (kind, bytes) = read_capped(r, key, 4)?;
    ((kind == REG_BINARY || kind == REG_DWORD) && bytes.len() == 4).then(|| i32::from_le_bytes(bytes.try_into().unwrap()))
}

/// PlayerPrefs::HasKey: either registry name exists.
pub fn registry_has_key<R: RegistryValues + ?Sized>(r: &R, key: &str) -> bool {
    r.raw(&value_name(key)).is_some() || r.raw(key).is_some()
}

/// PlayerPrefs.GetString(key, default).
pub fn get_string(store: &dyn PlayerPrefsStore, key: &str, default: &str) -> String {
    store.string(key).unwrap_or_else(|| default.to_owned())
}

/// PlayerPrefs.GetFloat(key, default).
pub fn get_float(store: &dyn PlayerPrefsStore, key: &str, default: f32) -> f32 {
    store.float(key).unwrap_or(default)
}

/// UserPreferenceManager.LoadFromLegacyStorage: the preference values the simulation reads, from PlayerPrefs. The fields
/// the simulation does not read (window, bloom, resolution-independent rendering and volume settings) are left out.
pub fn legacy_preferences(store: &dyn PlayerPrefsStore) -> Prefs {
    let mut p = Prefs::default();
    let on = |key: &str, default: &str| get_string(store, key, default) == crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 1);
    p.DropOnDeath = on("dropOnDeath", crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 1));
    p.LimbCrushing = on(crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 3), crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 1));
    p.TracerBullets = on(crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 6), crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 7));
    p.FancyEffects = on("extraDecals", crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 1));
    p.Decals = on(crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 12), crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 1));
    p.CrushForceMultiplier = get_float(store, "crushForce", 1.0);
    p.ShakeIntensity = get_float(store, "shakeIntensity", 1.0);
    // CollisionQuality: "continuous" or else Discrete (0).
    p.CollisionQuality = if get_string(store, crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 51), crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 52)) == crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 53) { 1 } else { 0 };
    // Mathf.RoundToInt(float) rounds half to even.
    p.PhysicsIterations = get_float(store, "physicsIterations", 16.0).round_ties_even() as i32;
    // The resolution is only parsed (Split('x'), int.Parse of [0], then of [1]): a malformed "WxH" throws in the game.
    let resolution = get_string(store, crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 61), crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 62));
    if resolution != crate::il!(str "UserPreferenceManager::LoadFromLegacyStorage" 62) {
        let parts: Vec<&str> = resolution.split('x').collect();
        int_parse(parts[0]);
        match parts.get(1) {
            Some(y) => int_parse(y),
            None => super::menu::throw(format!("IndexOutOfRangeException: LoadFromLegacyStorage resolution {resolution:?}")),
        };
    }
    p
}

/// int.Parse(string) (NumberStyles.Integer, invariant signs): white space (tab to CR, space) around, one sign, digits.
/// No digits or another character: FormatException; past Int32: OverflowException.
fn int_parse(s: &str) -> i32 {
    let t = s.trim_matches(|c: char| matches!(c, '\t'..='\r' | ' ')).trim_end_matches('\0');
    let d = t.strip_prefix(['+', '-']).unwrap_or(t);
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        super::menu::throw(format!("FormatException: int.Parse {s:?}"));
    }
    t.parse().unwrap_or_else(|_| super::menu::throw(format!("OverflowException: int.Parse {s:?}")))
}

/// PlayerPrefs' Windows store: HKEY_CURRENT_USER\Software\<company>\<product>, opened read-only. A missing key holds no values.
#[cfg(windows)]
pub struct Registry(Option<windows_sys::Win32::System::Registry::HKEY>);

#[cfg(windows)]
impl Registry {
    pub fn open() -> Registry {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegOpenKeyExW};
        let path: Vec<u16> = format!("Software\\{COMPANY_NAME}\\{PRODUCT_NAME}").encode_utf16().chain(std::iter::once(0)).collect();
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is a NUL-terminated UTF-16 string and `key` is a valid out pointer.
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) };
        if status == ERROR_SUCCESS { Registry(Some(key)) } else { Registry(None) }
    }
}

#[cfg(windows)]
impl Drop for Registry {
    fn drop(&mut self) {
        if let Some(key) = self.0 {
            // SAFETY: `key` was opened by RegOpenKeyExW and is closed once.
            unsafe { windows_sys::Win32::System::Registry::RegCloseKey(key) };
        }
    }
}

#[cfg(windows)]
impl RegistryValues for Registry {
    /// RegQueryValueExW: the size first, then the data; every failed status is an absent value (Unity treats nonzero statuses alike).
    fn raw(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::RegQueryValueExW;
        let hkey = self.0?;
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: the name is NUL-terminated; a null data pointer asks for the size only.
        let status = unsafe { RegQueryValueExW(hkey, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size) };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut bytes = vec![0u8; size as usize];
        // SAFETY: `bytes` has the size the first query reported.
        let status = unsafe { RegQueryValueExW(hkey, name.as_ptr(), std::ptr::null(), &mut kind, bytes.as_mut_ptr(), &mut size) };
        if status != ERROR_SUCCESS {
            return None;
        }
        bytes.truncate(size as usize);
        Some((kind, bytes))
    }
}

#[cfg(windows)]
impl PlayerPrefsStore for Registry {
    fn string(&self, key: &str) -> Option<String> {
        registry_string(self, key)
    }
    fn float(&self, key: &str) -> Option<f32> {
        registry_float(self, key)
    }
    fn has_key(&self, key: &str) -> bool {
        registry_has_key(self, key)
    }
}

/// The game's PlayerPrefs store.
#[cfg(windows)]
pub fn store() -> Registry {
    Registry::open()
}

/// PlayerPrefs outside Windows: the macOS player keeps them in NSUserDefaults, the app domain `unity.<company>.<product>`
/// (~/Library/Preferences/unity.<company>.<product>.plist) under the plain key; the Linux player in the XML file
/// ~/.config/unity3d/<company>/<product>/prefs. A value is a string or a number; a number reads as the text of its value and
/// a string as its number (NSUserDefaults stringForKey / floatForKey).
#[cfg(not(windows))]
pub struct FileStore;

#[cfg(not(windows))]
impl FileStore {
    fn text(&self, key: &str) -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        if cfg!(target_os = "macos") {
            let path = format!("{home}/Library/Preferences/unity.{COMPANY_NAME}.{PRODUCT_NAME}.plist");
            // plutil -extract <keypath> raw: the value's text; a dot in the key is escaped in the key path.
            let out = std::process::Command::new("plutil").args(["-extract", &key.replace('.', "\\."), "raw", "-o", "-", &path]).output().ok()?;
            return out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let xml = std::fs::read_to_string(format!("{home}/.config/unity3d/{COMPANY_NAME}/{PRODUCT_NAME}/prefs")).ok()?;
        let open = format!("<pref name=\"{}\" type=\"", xml_escape(key));
        let rest = &xml[xml.find(&open)? + open.len()..];
        let rest = &rest[rest.find('>')? + 1..];
        Some(xml_unescape(&rest[..rest.find("</pref>")?]))
    }
}

#[cfg(not(windows))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(not(windows))]
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(not(windows))]
impl PlayerPrefsStore for FileStore {
    fn string(&self, key: &str) -> Option<String> {
        self.text(key)
    }
    fn float(&self, key: &str) -> Option<f32> {
        self.text(key)?.trim().parse().ok()
    }
    fn has_key(&self, key: &str) -> bool {
        self.text(key).is_some()
    }
}

#[cfg(not(windows))]
pub fn store() -> FileStore {
    FileStore
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Registry values by name, as SetString / SetFloat would store them.
    #[derive(Default)]
    struct Fake(HashMap<String, (u32, Vec<u8>)>);
    impl Fake {
        fn set_string(&mut self, key: &str, s: &str) {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            self.0.insert(value_name(key), (REG_BINARY, bytes));
        }
        fn set_float(&mut self, key: &str, f: f32) {
            self.0.insert(value_name(key), (REG_DWORD, (f as f64).to_le_bytes().to_vec()));
        }
    }
    impl RegistryValues for Fake {
        fn raw(&self, name: &str) -> Option<(u32, Vec<u8>)> {
            self.0.get(name).cloned()
        }
    }
    impl PlayerPrefsStore for Fake {
        fn string(&self, key: &str) -> Option<String> {
            registry_string(self, key)
        }
        fn float(&self, key: &str) -> Option<f32> {
            registry_float(self, key)
        }
        fn has_key(&self, key: &str) -> bool {
            registry_has_key(self, key)
        }
    }

    /// The names and hashes read from the game's own registry key on this machine (the 13 stored values).
    #[test]
    fn value_names_hash_like_unity() {
        for (key, hash) in [
            ("Screenmanager Stereo 3D", 1665754519u32),
            ("Screenmanager Resolution Width Default", 680557497),
            ("Screenmanager Resolution Height Default", 1380706816),
            ("Screenmanager Resolution Use Native Default", 1405981789),
            ("Screenmanager Fullscreen mode Default", 401710285),
            ("Screenmanager Resolution Width", 182942802),
            ("Screenmanager Resolution Height", 2627697771),
            ("Screenmanager Fullscreen mode", 3630240806),
            ("Screenmanager Resolution Use Native", 1405027254),
            ("unity.player_session_count", 922449978),
            ("unity.player_sessionid", 1351336811),
            ("unity.cloud_userid", 2665564582),
            ("UnitySelectMonitor", 17969598),
        ] {
            assert_eq!(value_name(key), format!("{key}_h{hash}"));
        }
    }

    #[test]
    fn strings_stop_at_the_nul_and_floats_are_doubles() {
        let mut s = Fake::default();
        s.set_string("collision", "continuous");
        s.set_float("crushForce", 2.5);
        assert_eq!(get_string(&s, "collision", "discrete"), "continuous");
        assert_eq!(get_float(&s, "crushForce", 1.0), 2.5);
        assert_eq!(get_string(&s, "missing", "d"), "d");
        assert_eq!(get_float(&s, "missing", 7.0), 7.0);
    }

    /// GetString_Impl / GetNumber_Impl on the other value types and the plain-name fallback.
    #[test]
    fn registry_types_follow_the_native_getters() {
        let mut s = Fake::default();
        s.0.insert(value_name("a"), (REG_SZ, b"on\0".to_vec()));
        s.0.insert(value_name("b"), (REG_SZ, vec![0xc3, 0xa9, 0]));
        s.0.insert(value_name("c"), (REG_DWORD, vec![1, 0, 0, 0]));
        s.0.insert("d".to_string(), (REG_BINARY, 2.5f64.to_le_bytes().to_vec()));
        s.0.insert(value_name("e"), (REG_BINARY, vec![0; 16]));
        assert_eq!(registry_string(&s, "a").as_deref(), Some("on"));
        assert_eq!(registry_string(&s, "b"), None);
        assert_eq!(registry_string(&s, "c"), None);
        assert_eq!(registry_int(&s, "c"), Some(1));
        assert_eq!(registry_float(&s, "c"), None);
        assert_eq!(registry_float(&s, "d"), Some(2.5));
        assert_eq!(registry_float(&s, "e"), None);
        assert!(registry_has_key(&s, "d") && !registry_has_key(&s, "zz"));
    }

    #[test]
    fn legacy_preferences_read_the_stored_values() {
        let mut s = Fake::default();
        s.set_string("collision", "continuous");
        s.set_string("tracers", "on");
        s.set_string("dropOnDeath", "off");
        s.set_float("physicsIterations", 9.5);
        s.set_string("resolution", "1920x1080");
        let p = legacy_preferences(&s);
        assert_eq!((p.CollisionQuality, p.TracerBullets, p.DropOnDeath), (1, true, false));
        // Mathf.RoundToInt(9.5) is 10 (half to even: 9.5 -> 10, 8.5 -> 8).
        assert_eq!(p.PhysicsIterations, 10);
        // Nothing stored: the legacy defaults (physicsIterations 16, collision discrete, tracers off).
        let empty = Fake::default();
        let d = legacy_preferences(&empty);
        assert_eq!((d.CollisionQuality, d.TracerBullets, d.PhysicsIterations, d.LimbCrushing), (0, false, 16, true));
    }
}
