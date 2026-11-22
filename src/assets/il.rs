//! The game's own text and tables, read from the player's Assembly-CSharp.dll instead of written into the rewrite.
//!
//! The rewrite's logic and its tuning numbers are ours, in the code where modders change them; the texts the game's C#
//! holds (its ldstr strings, attribute arguments, enum names) and its tables stay in the player's DLL.
//! `il!(str "ContextMenuBehaviour::SaveAction" 0)` = the first `ldstr` in that method's IL.
//! A reference names `Type::Method` (nested types as `Outer/Inner`, lambdas and iterators by their compiler names;
//! the methods of one name in table order, overloads included), a kind and its index k among that kind's
//! instructions there:
//! - `r4` (`ldc.r4`, f32 bits as written), `r8` (`ldc.r8`, f64), `i4` (every `ldc.i4` form, any integer type),
//!   `i8` (`ldc.i8`), `str` (`ldstr`), `arr` (`ldtoken` of a FieldRVA field: a static array's initial bytes).
//!
//! `load` builds the table once at startup and refuses any DLL other than 1.28.41's (its SHA-256), so every reference
//! the tests resolved against 1.28.41 resolves for the player too.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use super::clr::{AttrArg, Assembly, FIELD, METHOD_DEF, TYPE_DEF};

/// SHA-256 of People Playground 1.28.41's Assembly-CSharp.dll.
const DLL_SHA256: &str = "36b6db3364d94e24899943994f739859663760d75a520647a9c09c4d73c69352";
/// SHA-256 of the newer build's: 1.28.54 (GameInfo.Version; Steam, 2026-11-22). "1.28.45" is the rewrite's name for the
/// newer build: it was 1.28.51 (6825452f..., Steam 2026-11-17), then 1.28.52 (d9b0e6fb..., 2026-11-20), then 1.28.53
/// (a5f500c4..., 2026-11-21), until 1.28.54 replaced it; the older ones are no longer supported, as Steam updates every install.
const DLL_SHA256_45: &str = "dfb48880d92d07fa05be0ed1d0a7e4f2d714e76130de6665d1f931d24b436ac7";

/// The installed game's version: the rewrite follows each version's code where 1.28.45 (now 1.28.54) changed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameVersion {
    V1_28_41,
    V1_28_45,
}

static VERSION: OnceLock<GameVersion> = OnceLock::new();

/// The loaded game's version (1.28.41 before `load`, as in the tests that build no table).
pub fn version() -> GameVersion {
    // Tests: the game PPG_GAME_DIR names decides, whichever test runs first (else a 1.28.54 run took 1.28.41 paths in
    // every test that never loaded the table).
    #[cfg(test)]
    if VERSION.get().is_none() {
        if let Ok(game) = crate::game_assets::find_game(None) {
            let _ = load(&game);
        }
    }
    *VERSION.get().unwrap_or(&GameVersion::V1_28_41)
}

/// Whether the installed game is the newer build (1.28.54; the rewrite's name for it is 1.28.45): the switch for every place
/// the updates since 1.28.41 changed.
pub fn is_1_28_45() -> bool {
    version() == GameVersion::V1_28_45
}

#[derive(Default)]
pub struct Consts {
    pub i4: Vec<i32>,
    pub i8: Vec<i64>,
    pub r4: Vec<f32>,
    pub r8: Vec<f64>,
    pub str: Vec<&'static str>,
    pub arr: Vec<Vec<u8>>,
}

pub struct Table {
    methods: HashMap<String, Consts>,
    /// "Type::Field" -> its custom attributes (class name, fixed arguments).
    field_attrs: HashMap<String, Vec<(String, Vec<AttrArg>)>>,
    /// "Type::Field" -> the enum type a field holds.
    field_enum: HashMap<String, String>,
    /// Enum type -> member names in declaration order, and their values.
    enums: HashMap<String, (Vec<&'static str>, Vec<i64>)>,
    /// Type -> its fields' names in declaration order.
    fields: HashMap<String, Vec<&'static str>>,
}

static TABLE: OnceLock<Table> = OnceLock::new();

/// Reads the player's DLL into the table; an error (missing file, other version) means the rewrite must not start.
pub fn load(game: &Path) -> Result<(), String> {
    if TABLE.get().is_some() {
        return Ok(());
    }
    let path = game.join("People Playground_Data/Managed/Assembly-CSharp.dll");
    let a = Assembly::open(&path)?;
    let hash = sha256(a.bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    // Both versions run: 1.28.41 and 1.28.45 (the changed code branches on is_1_28_45()).
    let v = if hash == DLL_SHA256 {
        GameVersion::V1_28_41
    } else if hash == DLL_SHA256_45 {
        GameVersion::V1_28_45
    } else {
        return Err(format!(
            "{} is not People Playground 1.28.41's or 1.28.54's (the current Steam build) (SHA-256 {hash}). The rewrite reads the game's numbers \
             and text from those exact builds' Assembly-CSharp.dll; install one of them.",
            path.display()
        ));
    };
    let _ = VERSION.set(v);
    let _ = TABLE.set(Table::build(&a)?);
    Ok(())
}

/// The table (`load` ran at startup; tests and tools load it from the installed game here).
pub fn get() -> &'static Table {
    if let Some(t) = TABLE.get() {
        return t;
    }
    if let Err(e) = crate::game_assets::find_game(None).and_then(|g| load(&g)) {
        panic!("{e}");
    }
    TABLE.get().unwrap()
}

impl Table {
    fn build(a: &Assembly) -> Result<Self, String> {
        let mut type_names = vec![String::new(); a.rows(TYPE_DEF) as usize + 1];
        for t in 1..=a.rows(TYPE_DEF) {
            let (ns, name) = a.type_name((TYPE_DEF, t)).unwrap_or_default();
            type_names[t as usize] = if ns.is_empty() { name } else { format!("{ns}.{name}") };
        }
        let mut methods: HashMap<String, Consts> = HashMap::new();
        for t in 1..=a.rows(TYPE_DEF) {
            let (start, end) = a.method_range(t);
            for m in start..end {
                let Some(il) = a.method_il(m) else { continue };
                let key = format!("{}::{}", type_names[t as usize], a.string(METHOD_DEF, m, 3));
                scan(a, il, methods.entry(key).or_default()).map_err(|e| format!("{}: {e}", a.path.display()))?;
            }
        }
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let mut enums = HashMap::new();
        let mut field_enum = HashMap::new();
        let mut fields: HashMap<String, Vec<&'static str>> = HashMap::new();
        for t in 1..=a.rows(TYPE_DEF) {
            let extends = a.coded(TYPE_DEF, t, 3);
            if a.type_name(extends).is_some_and(|(n, c)| n == "System" && c == "Enum") {
                let m = a.enum_members(t);
                enums.insert(type_names[t as usize].clone(), (m.iter().map(|x| leak(x.0.clone())).collect(), m.iter().map(|x| x.1).collect()));
            }
            let (start, end) = a.field_range(t);
            for f in start..end {
                fields.entry(type_names[t as usize].clone()).or_insert_with(Vec::new).push(leak(a.string(FIELD, f, 1).to_string()));
                let sig = a.blob(FIELD, f, 2);
                if sig.len() > 2 && sig[0] == 6 && sig[1] == 0x11 {
                    let (v, _) = super::clr::compressed(&sig[2..])?;
                    if let (TYPE_DEF, e) = Assembly::type_def_or_ref(v) {
                        field_enum.insert(format!("{}::{}", type_names[t as usize], a.string(FIELD, f, 1)), type_names[e as usize].clone());
                    }
                }
            }
        }
        let mut field_attrs: HashMap<String, Vec<(String, Vec<AttrArg>)>> = HashMap::new();
        for ((table, row), _, class, args) in a.attribute_values() {
            if table == FIELD {
                let key = format!("{}::{}", type_names[a.field_owner(row) as usize], a.string(FIELD, row, 1));
                field_attrs.entry(key).or_default().push((class, args));
            }
        }
        Ok(Self { methods, field_attrs, field_enum, enums, fields })
    }

    /// The fixed arguments of a field's custom attribute (by class name, e.g. "SettingAttribute"), if it has one.
    pub fn field_attr(&'static self, ty: &str, field: &str, class: &str) -> Option<&'static [AttrArg]> {
        let list = self.field_attrs.get(&format!("{ty}::{field}"))?;
        list.iter().find(|(c, _)| c == class).map(|(_, args)| args.as_slice())
    }

    /// A string argument of a field's custom attribute ("" when missing).
    pub fn field_attr_str(&'static self, ty: &str, field: &str, class: &str, arg: usize) -> &'static str {
        match self.field_attr(ty, field, class).and_then(|a| a.get(arg)) {
            Some(AttrArg::Str(Some(s))) => s.as_str(),
            _ => "",
        }
    }

    /// A type's field names in declaration order.
    pub fn fields(&'static self, ty: &str) -> &'static [&'static str] {
        self.fields.get(ty).unwrap_or_else(|| panic!("Assembly-CSharp.dll has no type {ty} with fields"))
    }

    /// An enum's member names in declaration order.
    pub fn enum_names(&'static self, ty: &str) -> &'static [&'static str] {
        &self.enums.get(ty).unwrap_or_else(|| panic!("Assembly-CSharp.dll has no enum {ty}")).0
    }

    /// The name of an enum's member with this value (the first one declared).
    pub fn enum_name(&'static self, ty: &str, value: i64) -> &'static str {
        let (names, values) = self.enums.get(ty).unwrap_or_else(|| panic!("Assembly-CSharp.dll has no enum {ty}"));
        values.iter().position(|&v| v == value).map(|i| names[i]).unwrap_or_else(|| panic!("{ty} has no member {value}"))
    }

    /// The member names of the enum a field holds.
    pub fn field_enum_names(&'static self, ty: &str, field: &str) -> &'static [&'static str] {
        let e = self.field_enum.get(&format!("{ty}::{field}")).unwrap_or_else(|| panic!("{ty}::{field} is not an enum field"));
        self.enum_names(e)
    }

    fn method(&self, m: &str) -> &Consts {
        self.methods.get(m).unwrap_or_else(|| panic!("Assembly-CSharp.dll has no method {m}"))
    }

    fn miss(m: &str, kind: &str, k: usize) -> ! {
        panic!("Assembly-CSharp.dll: {m} has no {kind} #{k}")
    }

    pub fn r4(&self, m: &str, k: usize) -> f32 {
        *self.method(m).r4.get(k).unwrap_or_else(|| Self::miss(m, "ldc.r4", k))
    }
    pub fn r8(&self, m: &str, k: usize) -> f64 {
        *self.method(m).r8.get(k).unwrap_or_else(|| Self::miss(m, "ldc.r8", k))
    }
    pub fn i4(&self, m: &str, k: usize) -> i32 {
        *self.method(m).i4.get(k).unwrap_or_else(|| Self::miss(m, "ldc.i4", k))
    }
    pub fn i8(&self, m: &str, k: usize) -> i64 {
        *self.method(m).i8.get(k).unwrap_or_else(|| Self::miss(m, "ldc.i8", k))
    }
    pub fn str(&'static self, m: &str, k: usize) -> &'static str {
        self.method(m).str.get(k).copied().unwrap_or_else(|| Self::miss(m, "ldstr", k))
    }
    pub fn arr(&'static self, m: &str, k: usize) -> &'static [u8] {
        self.method(m).arr.get(k).unwrap_or_else(|| Self::miss(m, "static array", k))
    }

    /// Whether a reference resolves (the source-scan test).
    pub fn has(&self, m: &str, kind: &str, k: usize) -> bool {
        k < self.count(m, kind)
    }

    /// How many constants of a kind a method loads.
    pub fn count(&self, m: &str, kind: &str) -> usize {
        self.methods.get(m).map_or(0, |c| {
            match kind {
                "r4" => c.r4.len(),
                "r8" => c.r8.len(),
                "i4" => c.i4.len(),
                "i8" => c.i8.len(),
                "str" => c.str.len(),
                "arr" => c.arr.len(),
                _ => 0,
            }
        })
    }
}

/// Walks one method body's instructions (ECMA-335 III), keeping the constants it loads.
fn scan(a: &Assembly, il: &[u8], c: &mut Consts) -> Result<(), String> {
    let mut i = 0;
    let u32_at = |o: usize| -> Result<u32, String> {
        il.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| "IL cut short".to_string())
    };
    while i < il.len() {
        let op = il[i];
        i += 1;
        let operand = match op {
            0x15..=0x1E => {
                c.i4.push(op as i32 - 0x16);
                0
            }
            0x1F => {
                c.i4.push(*il.get(i).ok_or("IL cut short")? as i8 as i32);
                1
            }
            0x20 => {
                c.i4.push(u32_at(i)? as i32);
                4
            }
            0x21 => {
                c.i8.push(i64::from_le_bytes(il.get(i..i + 8).ok_or("IL cut short")?.try_into().unwrap()));
                8
            }
            0x22 => {
                c.r4.push(f32::from_bits(u32_at(i)?));
                4
            }
            0x23 => {
                c.r8.push(f64::from_bits(u64::from_le_bytes(il.get(i..i + 8).ok_or("IL cut short")?.try_into().unwrap())));
                8
            }
            0x72 => {
                let t = u32_at(i)?;
                c.str.push(Box::leak(a.user_string(t & 0xFF_FFFF).ok_or("bad ldstr token")?.into_boxed_str()));
                4
            }
            0xD0 => {
                let t = u32_at(i)?;
                if t >> 24 == FIELD as u32 {
                    if let Some(data) = a.field_rva_data(t & 0xFF_FFFF) {
                        c.arr.push(data.to_vec());
                    }
                }
                4
            }
            0x00..=0x0D | 0x14 | 0x25 | 0x26 | 0x2A | 0x46..=0x6E | 0x76 | 0x7A | 0x82..=0x8B | 0x8E | 0x90..=0xA2
            | 0xB3..=0xBA | 0xC3 | 0xD1..=0xDC | 0xDF | 0xE0 => 0,
            0x0E..=0x13 | 0x2B..=0x37 | 0xDE => 1,
            0x27..=0x29 | 0x38..=0x44 | 0x6F..=0x75 | 0x79 | 0x7B..=0x81 | 0x8C | 0x8D | 0x8F | 0xA3..=0xA5 | 0xC2 | 0xC6 | 0xDD => 4,
            0x45 => 4 + 4 * u32_at(i)? as usize,
            0xFE => {
                let op2 = *il.get(i).ok_or("IL cut short")?;
                i += 1;
                match op2 {
                    0x00..=0x05 | 0x0F | 0x11 | 0x13 | 0x14 | 0x17 | 0x18 | 0x1A | 0x1D | 0x1E => 0,
                    0x12 | 0x19 => 1,
                    0x09..=0x0E => 2,
                    0x06 | 0x07 | 0x15 | 0x16 | 0x1C => 4,
                    _ => return Err(format!("unknown opcode FE {op2:02X}")),
                }
            }
            _ => return Err(format!("unknown opcode {op:02X}")),
        };
        i += operand;
    }
    Ok(())
}

/// An `ldc.i4` constant as whatever integer type the code uses.
pub trait FromI4 {
    fn from_i4(v: i32) -> Self;
}
macro_rules! from_i4 {
    ($($t:ty)*) => { $(impl FromI4 for $t { fn from_i4(v: i32) -> Self { v as $t } })* };
}
from_i4!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

/// One of the game's constants: `il!(kind "Type::Method" k)` (see the module docs). Each use site reads the table once.
#[macro_export]
macro_rules! il {
    (r4 $m:literal $k:literal) => {{
        static C: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
        *C.get_or_init(|| $crate::assets::il::get().r4($m, $k))
    }};
    (r8 $m:literal $k:literal) => {{
        static C: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
        *C.get_or_init(|| $crate::assets::il::get().r8($m, $k))
    }};
    (i4 $m:literal $k:literal) => {{
        static C: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
        $crate::assets::il::FromI4::from_i4(*C.get_or_init(|| $crate::assets::il::get().i4($m, $k)))
    }};
    (i8 $m:literal $k:literal) => {{
        static C: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
        *C.get_or_init(|| $crate::assets::il::get().i8($m, $k))
    }};
    (str $m:literal $k:literal) => {{
        static C: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
        *C.get_or_init(|| $crate::assets::il::get().str($m, $k))
    }};
    (arr $m:literal $k:literal) => {
        $crate::assets::il::get().arr($m, $k)
    };
}

/// A constant 1.28.45 moved: `il_v!(kind "Type::Method" k, "Type::Method45" k45)` reads the first under 1.28.41 and the second under 1.28.45.
#[macro_export]
macro_rules! il_v {
    ($kind:tt $m:literal $k:literal, $m45:literal $k45:literal) => {
        if $crate::assets::il::is_1_28_45() { $crate::il!($kind $m45 $k45) } else { $crate::il!($kind $m $k) }
    };
}

/// A constant only 1.28.45's DLL has (its new classes, v45-new): `il45!(kind "Type::Method" k)`; never read on 1.28.41.
#[macro_export]
macro_rules! il45 {
    ($kind:tt $m:literal $k:literal) => {{
        debug_assert!($crate::assets::il::is_1_28_45(), "{} is 1.28.45's", $m);
        $crate::il!($kind $m $k)
    }};
}

/// FIPS 180-4 SHA-256.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for t in 0..64 {
            w[t] = if t < 16 {
                u32::from_be_bytes(block[t * 4..t * 4 + 4].try_into().unwrap())
            } else {
                let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
                let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
                w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1)
            };
        }
        let mut v = h;
        for t in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known() {
        let hex = |d: &[u8]| sha256(d).iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// Every `il!` reference in src resolves against the installed 1.28.41 DLL.
    #[test]
    fn every_reference_resolves() {
        if crate::game_assets::find_game(None).is_err() {
            return;
        }
        let table = get();
        let mut missing = Vec::new();
        let mut count = 0;
        let mut stack = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || p.ends_with("assets/il.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                for chunk in text.split("il!(").skip(1) {
                    let mut it = chunk.splitn(2, ' ');
                    let kind = it.next().unwrap();
                    let rest = it.next().unwrap_or("");
                    let Some((m, rest)) = rest.strip_prefix('"').and_then(|r| r.split_once('"')) else { continue };
                    let k: usize = rest.trim_start().split(')').next().unwrap().trim().parse().unwrap();
                    count += 1;
                    if !table.has(m, kind, k) {
                        missing.push(format!("{}: {kind} {m} {k}", p.display()));
                    }
                }
                // il45!(kind "m" k): 1.28.45's only, checked when 1.28.45 is loaded.
                for chunk in text.split("il45!(").skip(1).filter(|_| is_1_28_45()) {
                    let Some((kind, rest)) = chunk.split_once(' ') else { continue };
                    let Some((m, rest)) = rest.strip_prefix('"').and_then(|r| r.split_once('"')) else { continue };
                    let k: usize = rest.trim_start().split(')').next().unwrap().trim().parse().unwrap();
                    count += 1;
                    if !table.has(m, kind, k) {
                        missing.push(format!("{}: {kind} {m} {k}", p.display()));
                    }
                }
                // il_v!(kind "m" k, "m45" k45): the pair of the loaded version.
                for chunk in text.split("il_v!(").skip(1) {
                    let Some((kind, rest)) = chunk.split_once(' ') else { continue };
                    let Some((m, rest)) = rest.strip_prefix('"').and_then(|r| r.split_once('"')) else { continue };
                    let Some((k, rest)) = rest.split_once(',') else { continue };
                    let Some((m45, rest)) = rest.trim_start().strip_prefix('"').and_then(|r| r.split_once('"')) else { continue };
                    let k45 = rest.trim_start().split(')').next().unwrap().trim();
                    let (m, k) = if is_1_28_45() { (m45, k45) } else { (m, k.trim()) };
                    let k: usize = k.parse().unwrap();
                    count += 1;
                    if !table.has(m, kind, k) {
                        missing.push(format!("{}: {kind} {m} {k}", p.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "{count} references, unresolved: {missing:#?}");
    }

    /// The whole table as TSV (method, kind, k, value) for the conversion tool: `IL_DUMP=path cargo test il_dump -- --ignored`.
    #[test]
    #[ignore]
    fn il_dump() {
        let Ok(out) = std::env::var("IL_DUMP") else { return };
        let t = get();
        let mut lines = Vec::new();
        for (m, c) in &t.methods {
            for (k, v) in c.r4.iter().enumerate() {
                lines.push(format!("{m}\tr4\t{k}\t{:08x}", v.to_bits()));
            }
            for (k, v) in c.r8.iter().enumerate() {
                lines.push(format!("{m}\tr8\t{k}\t{:016x}", v.to_bits()));
            }
            for (k, v) in c.i4.iter().enumerate() {
                lines.push(format!("{m}\ti4\t{k}\t{v}"));
            }
            for (k, v) in c.i8.iter().enumerate() {
                lines.push(format!("{m}\ti8\t{k}\t{v}"));
            }
            for (k, v) in c.str.iter().enumerate() {
                lines.push(format!("{m}\tstr\t{k}\t{}", serde_json::to_string(v).unwrap()));
            }
            for (k, v) in c.arr.iter().enumerate() {
                lines.push(format!("{m}\tarr\t{k}\t{}", v.iter().map(|b| format!("{b:02x}")).collect::<String>()));
            }
        }
        lines.sort();
        std::fs::write(out, lines.join("\n")).unwrap();
    }

    /// Every `il!` reference whose constant differs between two DLLs (a changed method moves its constants):
    /// `IL_OLD=<1.28.41 dll> IL_NEW=<1.28.45 dll> cargo test il_version_diff -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn il_version_diff() {
        let (Ok(old), Ok(new)) = (std::env::var("IL_OLD"), std::env::var("IL_NEW")) else { return };
        let old = Table::build(&Assembly::open(Path::new(&old)).unwrap()).unwrap();
        let new = Table::build(&Assembly::open(Path::new(&new)).unwrap()).unwrap();
        let val = |t: &Table, m: &str, kind: &str, k: usize| -> Option<String> {
            let c = t.methods.get(m)?;
            match kind {
                "r4" => c.r4.get(k).map(|v| format!("{v:?}")),
                "r8" => c.r8.get(k).map(|v| format!("{v:?}")),
                "i4" => c.i4.get(k).map(|v| v.to_string()),
                "i8" => c.i8.get(k).map(|v| v.to_string()),
                "str" => c.str.get(k).map(|v| format!("{v:?}")),
                "arr" => c.arr.get(k).map(|v| format!("{v:02x?}")),
                _ => None,
            }
        };
        let mut stack = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || p.ends_with("assets/il.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                for chunk in text.split("il!(").skip(1) {
                    let mut it = chunk.splitn(2, ' ');
                    let kind = it.next().unwrap();
                    let rest = it.next().unwrap_or("");
                    let Some((m, rest)) = rest.strip_prefix('"').and_then(|r| r.split_once('"')) else { continue };
                    let Ok(k) = rest.trim_start().split(')').next().unwrap().trim().parse::<usize>() else { continue };
                    let (a, b) = (val(&old, m, kind, k), val(&new, m, kind, k));
                    if a != b {
                        // Where 1.28.45 holds the old constant: the same method (shifted) or a renamed lambda of its type.
                        let ty = m.split("::").next().unwrap();
                        let mut at = Vec::new();
                        for nm in new.methods.keys().filter(|n| *n == m || (n.starts_with(&format!("{ty}::<")) && m.contains("::<"))) {
                            for k2 in 0..new.count(nm, kind) {
                                if a.is_some() && val(&new, nm, kind, k2) == a {
                                    at.push(format!("{nm} {k2}"));
                                }
                            }
                        }
                        println!("ILDIFF\t{}\t{kind} {m} {k}\t{a:?}\t{b:?}\t45: {at:?}", p.display());
                    }
                }
            }
        }
    }
}
