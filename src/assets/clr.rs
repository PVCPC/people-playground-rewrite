//! ECMA-335 metadata of the game's managed assemblies (Managed/*.dll): type definitions, fields and the custom
//! attributes Unity's serializer looks at. Only reading; no IL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const TYPE_REF: usize = 0x01;
pub const TYPE_DEF: usize = 0x02;
pub const FIELD: usize = 0x04;
pub const METHOD_DEF: usize = 0x06;
pub const MEMBER_REF: usize = 0x0A;
pub const CUSTOM_ATTRIBUTE: usize = 0x0C;
pub const TYPE_SPEC: usize = 0x1B;
pub const ASSEMBLY_REF: usize = 0x23;
pub const EXPORTED_TYPE: usize = 0x27;
pub const NESTED_CLASS: usize = 0x29;

/// Column kinds of the metadata tables.
#[derive(Clone, Copy)]
enum Col {
    U16,
    U32,
    Str,
    Guid,
    Blob,
    /// A simple index into table n.
    T(usize),
    /// A coded index over these tables (usize::MAX = unused tag).
    C(&'static [usize]),
}

const NA: usize = usize::MAX;
const TYPE_DEF_OR_REF: &[usize] = &[0x02, 0x01, 0x1B];
const HAS_CONSTANT: &[usize] = &[0x04, 0x08, 0x17];
const HAS_CUSTOM_ATTRIBUTE: &[usize] =
    &[0x06, 0x04, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x00, 0x0E, 0x17, 0x14, 0x11, 0x1A, 0x1B, 0x20, 0x23, 0x26, 0x27, 0x28, 0x2A, 0x2C, 0x2B];
const HAS_FIELD_MARSHAL: &[usize] = &[0x04, 0x08];
const HAS_DECL_SECURITY: &[usize] = &[0x02, 0x06, 0x20];
const MEMBER_REF_PARENT: &[usize] = &[0x02, 0x01, 0x1A, 0x06, 0x1B];
const HAS_SEMANTICS: &[usize] = &[0x14, 0x17];
const METHOD_DEF_OR_REF: &[usize] = &[0x06, 0x0A];
const MEMBER_FORWARDED: &[usize] = &[0x04, 0x06];
const IMPLEMENTATION: &[usize] = &[0x26, 0x23, 0x27];
const CUSTOM_ATTRIBUTE_TYPE: &[usize] = &[NA, NA, 0x06, 0x0A, NA];
const RESOLUTION_SCOPE: &[usize] = &[0x00, 0x1A, 0x23, 0x01];
const TYPE_OR_METHOD_DEF: &[usize] = &[0x02, 0x06];

fn schema(table: usize) -> &'static [Col] {
    use Col::*;
    match table {
        0x00 => &[U16, Str, Guid, Guid, Guid],
        0x01 => &[C(RESOLUTION_SCOPE), Str, Str],
        0x02 => &[U32, Str, Str, C(TYPE_DEF_OR_REF), T(0x04), T(0x06)],
        0x03 => &[T(0x04)],
        0x04 => &[U16, Str, Blob],
        0x05 => &[T(0x06)],
        0x06 => &[U32, U16, U16, Str, Blob, T(0x08)],
        0x07 => &[T(0x08)],
        0x08 => &[U16, U16, Str],
        0x09 => &[T(0x02), C(TYPE_DEF_OR_REF)],
        0x0A => &[C(MEMBER_REF_PARENT), Str, Blob],
        0x0B => &[U16, C(HAS_CONSTANT), Blob],
        0x0C => &[C(HAS_CUSTOM_ATTRIBUTE), C(CUSTOM_ATTRIBUTE_TYPE), Blob],
        0x0D => &[C(HAS_FIELD_MARSHAL), Blob],
        0x0E => &[U16, C(HAS_DECL_SECURITY), Blob],
        0x0F => &[U16, U32, T(0x02)],
        0x10 => &[U32, T(0x04)],
        0x11 => &[Blob],
        0x12 => &[T(0x02), T(0x14)],
        0x13 => &[T(0x14)],
        0x14 => &[U16, Str, C(TYPE_DEF_OR_REF)],
        0x15 => &[T(0x02), T(0x17)],
        0x16 => &[T(0x17)],
        0x17 => &[U16, Str, Blob],
        0x18 => &[U16, T(0x06), C(HAS_SEMANTICS)],
        0x19 => &[T(0x02), C(METHOD_DEF_OR_REF), C(METHOD_DEF_OR_REF)],
        0x1A => &[Str],
        0x1B => &[Blob],
        0x1C => &[U16, C(MEMBER_FORWARDED), Str, T(0x1A)],
        0x1D => &[U32, T(0x04)],
        0x1E => &[U32, U32],
        0x1F => &[U32],
        0x20 => &[U32, U16, U16, U16, U16, U32, Blob, Str, Str],
        0x21 => &[U32],
        0x22 => &[U32, U32, U32],
        0x23 => &[U16, U16, U16, U16, U32, Blob, Str, Str, Blob],
        0x24 => &[U32, T(0x23)],
        0x25 => &[U32, U32, U32, T(0x23)],
        0x26 => &[U32, Str, Blob],
        0x27 => &[U32, U32, Str, Str, C(IMPLEMENTATION)],
        0x28 => &[U32, U32, Str, C(IMPLEMENTATION)],
        0x29 => &[T(0x02), T(0x02)],
        0x2A => &[U16, U16, C(TYPE_OR_METHOD_DEF), Str],
        0x2B => &[C(METHOD_DEF_OR_REF), Blob],
        0x2C => &[T(0x2A), C(TYPE_DEF_OR_REF)],
        _ => &[],
    }
}

/// A decoded coded index: (table, 1-based row).
pub type Token = (usize, u32);

pub struct Assembly {
    pub path: PathBuf,
    bytes: Vec<u8>,
    strings: usize,
    blobs: usize,
    /// The #US (user strings) heap.
    user_strings: usize,
    guids: usize,
    wide_str: bool,
    wide_guid: bool,
    wide_blob: bool,
    rows: [u32; 64],
    table_start: [usize; 64],
    row_size: [usize; 64],
    /// (namespace, name) -> TypeDef row, top-level types only; nested types keyed as "Outer/Inner".
    types: HashMap<(String, String), u32>,
    /// Field row -> owning TypeDef row.
    field_owner: Vec<u32>,
    /// Custom attributes by parent token.
    attributes: HashMap<Token, Vec<Token>>,
    /// PE sections: (virtual address, size, file offset).
    sections: Vec<(u32, u32, u32)>,
    /// RVA of the CLI managed-resources directory.
    resources_rva: u32,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// A compressed unsigned integer (II.23.2) and its length.
pub fn compressed(b: &[u8]) -> Result<(u32, usize), String> {
    let first = *b.first().ok_or("empty signature")?;
    Ok(if first & 0x80 == 0 {
        (first as u32, 1)
    } else if first & 0xC0 == 0x80 {
        ((((first & 0x3F) as u32) << 8) | *b.get(1).ok_or("short signature")? as u32, 2)
    } else {
        let b = b.get(..4).ok_or("short signature")?;
        ((((first & 0x1F) as u32) << 24) | (b[1] as u32) << 16 | (b[2] as u32) << 8 | b[3] as u32, 4)
    })
}

/// A custom attribute's fixed argument (II.23.3).
#[derive(Clone, Debug, PartialEq)]
pub enum AttrArg {
    Bool(bool),
    I4(i32),
    R4(f32),
    R8(f64),
    Str(Option<String>),
    Arr(Vec<AttrArg>),
}

/// The fixed arguments of a custom attribute value blob, typed by its constructor's signature; enums read as int32.
fn attr_args(sig: &[u8], value: &[u8]) -> Option<Vec<AttrArg>> {
    let (count, n) = compressed(sig.get(1..)?).ok()?;
    let mut s = 1 + n + 1; // callconv, param count, void return
    let mut v = 2; // prolog 0x0001
    if value.get(..2)? != [1, 0] {
        return None;
    }
    fn skip_type(sig: &[u8], s: &mut usize) -> Option<u8> {
        let t = *sig.get(*s)?;
        *s += 1;
        match t {
            0x1D => {
                skip_type(sig, s)?;
            }
            0x11 | 0x12 => *s += compressed(sig.get(*s..)?).ok()?.1,
            _ => {}
        }
        Some(t)
    }
    fn read(t: u8, elem: Option<u8>, value: &[u8], v: &mut usize) -> Option<AttrArg> {
        let take = |v: &mut usize, n: usize| -> Option<&[u8]> {
            let b = value.get(*v..*v + n)?;
            *v += n;
            Some(b)
        };
        Some(match t {
            0x02 => AttrArg::Bool(take(v, 1)?[0] != 0),
            0x08 | 0x11 => AttrArg::I4(i32::from_le_bytes(take(v, 4)?.try_into().ok()?)),
            0x0C => AttrArg::R4(f32::from_le_bytes(take(v, 4)?.try_into().ok()?)),
            0x0D => AttrArg::R8(f64::from_le_bytes(take(v, 8)?.try_into().ok()?)),
            0x0E => {
                if value.get(*v) == Some(&0xFF) {
                    *v += 1;
                    AttrArg::Str(None)
                } else {
                    let (n, len) = compressed(value.get(*v..)?).ok()?;
                    *v += len;
                    AttrArg::Str(Some(String::from_utf8(take(v, n as usize)?.to_vec()).ok()?))
                }
            }
            0x1D => {
                let n = u32::from_le_bytes(take(v, 4)?.try_into().ok()?);
                let e = elem?;
                AttrArg::Arr(if n == u32::MAX { Vec::new() } else { (0..n).map(|_| read(e, None, value, v)).collect::<Option<_>>()? })
            }
            _ => return None,
        })
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let start = s;
        let t = skip_type(sig, &mut s)?;
        let elem = (t == 0x1D).then(|| sig[start + 1]);
        out.push(read(t, elem, value, &mut v)?);
    }
    Some(out)
}

impl Assembly {
    pub fn open(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let b = &bytes;
        let pe = u32_at(b, 0x3C) as usize;
        if b.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(format!("{}: not a PE file", path.display()));
        }
        let sections = u16_at(b, pe + 6) as usize;
        let optional = pe + 24;
        let optional_size = u16_at(b, pe + 20) as usize;
        let dirs = optional + if u16_at(b, optional) == 0x20b { 112 } else { 96 };
        let cli_rva = u32_at(b, dirs + 14 * 8);
        let section_table = optional + optional_size;
        let to_offset = |rva: u32| -> Result<usize, String> {
            for i in 0..sections {
                let s = section_table + i * 40;
                let (va, size, raw) = (u32_at(b, s + 12), u32_at(b, s + 8).max(u32_at(b, s + 16)), u32_at(b, s + 20));
                if rva >= va && rva < va + size {
                    return Ok((rva - va + raw) as usize);
                }
            }
            Err(format!("RVA {rva:#x} outside the sections"))
        };
        let cli = to_offset(cli_rva)?;
        let resources_rva = u32_at(b, cli + 24);
        let sections_list = (0..sections).map(|i| {
            let s = section_table + i * 40;
            (u32_at(b, s + 12), u32_at(b, s + 8).max(u32_at(b, s + 16)), u32_at(b, s + 20))
        }).collect();
        let root = to_offset(u32_at(b, cli + 8))?;
        if u32_at(b, root) != 0x424A5342 {
            return Err(format!("{}: no metadata root", path.display()));
        }
        let version_len = u32_at(b, root + 12) as usize;
        let mut o = root + 16 + version_len + 2;
        let streams = u16_at(b, o) as usize;
        o += 2;
        let (mut tables, mut strings, mut blobs, mut user_strings, mut guids) = (0, 0, 0, 0, 0);
        for _ in 0..streams {
            let offset = root + u32_at(b, o) as usize;
            let name_start = o + 8;
            let name_len = b[name_start..].iter().position(|&c| c == 0).unwrap();
            match &b[name_start..name_start + name_len] {
                b"#~" | b"#-" => tables = offset,
                b"#Strings" => strings = offset,
                b"#Blob" => blobs = offset,
                b"#US" => user_strings = offset,
                b"#GUID" => guids = offset,
                _ => {}
            }
            o = name_start + ((name_len + 4) & !3);
        }
        let heaps = b[tables + 6];
        let valid = u64::from_le_bytes(b[tables + 8..tables + 16].try_into().unwrap());
        let mut rows = [0u32; 64];
        let mut o = tables + 24;
        for (t, r) in rows.iter_mut().enumerate() {
            if valid >> t & 1 != 0 {
                *r = u32_at(b, o);
                o += 4;
            }
        }
        let (wide_str, wide_guid, wide_blob) = (heaps & 1 != 0, heaps & 2 != 0, heaps & 4 != 0);
        let col_size = |c: Col| match c {
            Col::U16 => 2,
            Col::U32 => 4,
            Col::Str => 2 + 2 * wide_str as usize,
            Col::Guid => 2 + 2 * wide_guid as usize,
            Col::Blob => 2 + 2 * wide_blob as usize,
            Col::T(t) => 2 + 2 * (rows[t] > 0xFFFF) as usize,
            Col::C(ts) => {
                let bits = usize::BITS - (ts.len() - 1).leading_zeros();
                let max = ts.iter().filter(|&&t| t != NA).map(|&t| rows[t]).max().unwrap_or(0);
                2 + 2 * (max >= 1 << (16 - bits)) as usize
            }
        };
        let (mut table_start, mut row_size) = ([0usize; 64], [0usize; 64]);
        for t in 0..64 {
            if valid >> t & 1 == 0 {
                continue;
            }
            if t > 0x2C {
                return Err(format!("{}: unknown metadata table {t:#x}", path.display()));
            }
            row_size[t] = schema(t).iter().map(|&c| col_size(c)).sum();
            table_start[t] = o;
            o += row_size[t] * rows[t] as usize;
        }
        let mut a = Self {
            path: path.into(),
            bytes,
            strings,
            blobs,
            user_strings,
            guids,
            wide_str,
            wide_guid,
            wide_blob,
            rows,
            table_start,
            row_size,
            types: HashMap::new(),
            field_owner: Vec::new(),
            attributes: HashMap::new(),
            sections: sections_list,
            resources_rva,
        };
        a.index();
        Ok(a)
    }

    fn rva_offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find(|&&(va, size, _)| rva >= va && rva < va + size).map(|&(va, _, raw)| (rva - va + raw) as usize)
    }

    /// An embedded manifest resource's bytes.
    pub fn resource(&self, name: &str) -> Option<&[u8]> {
        let base = self.rva_offset(self.resources_rva)?;
        let r = (1..=self.rows[0x28]).find(|&r| self.string(0x28, r, 2) == name && self.coded(0x28, r, 3).1 == 0)?;
        let o = base + self.cell(0x28, r, 0) as usize;
        self.bytes.get(o + 4..o + 4 + u32_at(&self.bytes, o) as usize)
    }

    /// The initial data (FieldRVA) of the static-array field whose value type has ClassSize `size`: `None` unless exactly one such field exists.
    pub fn field_data(&self, size: u32) -> Option<&[u8]> {
        let sized: Vec<u32> = (1..=self.rows[0x0F]).filter(|&r| self.cell(0x0F, r, 1) == size).map(|r| self.cell(0x0F, r, 2)).collect();
        let mut found = (1..=self.rows[0x1D]).filter(|&r| {
            let sig = self.blob(FIELD, self.cell(0x1D, r, 1), 2);
            sig.len() > 2 && sig[0] == 6 && sig[1] == 0x11 && compressed(&sig[2..]).is_ok_and(|(v, _)| {
                let t = Self::type_def_or_ref(v);
                t.0 == TYPE_DEF && sized.contains(&t.1)
            })
        });
        let r = found.next().filter(|_| found.next().is_none())?;
        let o = self.rva_offset(self.cell(0x1D, r, 0))?;
        self.bytes.get(o..o + size as usize)
    }

    /// The TypeDef that declares a field.
    pub fn field_owner(&self, f: u32) -> u32 {
        self.field_owner[f as usize]
    }

    /// The whole file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The TypeDef's methods, [start, end) rows of the MethodDef table.
    pub fn method_range(&self, t: u32) -> (u32, u32) {
        let start = self.cell(TYPE_DEF, t, 5);
        let end = if t < self.rows[TYPE_DEF] { self.cell(TYPE_DEF, t + 1, 5) } else { self.rows[METHOD_DEF] + 1 };
        (start, end)
    }

    /// A MethodDef's IL code (II.25.4: tiny or fat header); None for abstract/extern methods.
    pub fn method_il(&self, m: u32) -> Option<&[u8]> {
        let rva = self.cell(METHOD_DEF, m, 0);
        let o = self.rva_offset(rva).filter(|_| rva != 0)?;
        let b = &self.bytes;
        let (start, size) = match b[o] & 3 {
            2 => (o + 1, (b[o] >> 2) as usize),
            3 => (o + 4 * (u16_at(b, o) >> 12) as usize, u32_at(b, o + 4) as usize),
            _ => return None,
        };
        b.get(start..start + size)
    }

    /// A #US string (an ldstr token's low 24 bits), UTF-16 decoded.
    pub fn user_string(&self, index: u32) -> Option<String> {
        let o = self.user_strings + index as usize;
        let (n, len) = compressed(self.bytes.get(o..)?).ok()?;
        let units = self.bytes.get(o + len..o + len + (n as usize & !1))?;
        String::from_utf16(&units.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).ok()
    }

    /// A field's FieldRVA initial data: as many bytes as its value type's ClassSize.
    pub fn field_rva_data(&self, field: u32) -> Option<&[u8]> {
        let r = (1..=self.rows[0x1D]).find(|&r| self.cell(0x1D, r, 1) == field)?;
        let sig = self.blob(FIELD, field, 2);
        let t = (sig.len() > 2 && sig[0] == 6 && sig[1] == 0x11).then(|| compressed(&sig[2..]).ok()).flatten()?;
        let t = Self::type_def_or_ref(t.0);
        let size = (1..=self.rows[0x0F]).find(|&c| t.0 == TYPE_DEF && self.cell(0x0F, c, 2) == t.1).map(|c| self.cell(0x0F, c, 1))?;
        let o = self.rva_offset(self.cell(0x1D, r, 0))?;
        self.bytes.get(o..o + size as usize)
    }

    /// Every custom attribute: (parent, namespace, class name, fixed constructor arguments). Named arguments are not read.
    pub fn attribute_values(&self) -> Vec<(Token, String, String, Vec<AttrArg>)> {
        let mut out = Vec::new();
        for r in 1..=self.rows[CUSTOM_ATTRIBUTE] {
            let ctor = self.coded(CUSTOM_ATTRIBUTE, r, 1);
            let Some((ns, name)) = self.attribute_class(ctor) else { continue };
            let sig = match ctor.0 {
                METHOD_DEF => self.blob(METHOD_DEF, ctor.1, 4),
                _ => self.blob(MEMBER_REF, ctor.1, 2),
            };
            let args = attr_args(sig, self.blob(CUSTOM_ATTRIBUTE, r, 2)).unwrap_or_default();
            out.push((self.coded(CUSTOM_ATTRIBUTE, r, 0), ns, name, args));
        }
        out
    }

    /// An enum's members in declaration order: (name, value of its Constant).
    pub fn enum_members(&self, t: u32) -> Vec<(String, i64)> {
        let (start, end) = self.field_range(t);
        (start..end)
            .filter(|&f| self.cell(FIELD, f, 0) & 0x40 != 0) // static: the literals, not value__
            .filter_map(|f| {
                let c = (1..=self.rows[0x0B]).find(|&c| self.coded(0x0B, c, 1) == (FIELD, f))?;
                let b = self.blob(0x0B, c, 2);
                let mut v = [0u8; 8];
                v[..b.len().min(8)].copy_from_slice(&b[..b.len().min(8)]);
                let v = match b.len() {
                    1 => v[0] as i8 as i64,
                    2 => i16::from_le_bytes([v[0], v[1]]) as i64,
                    4 => i32::from_le_bytes(v[..4].try_into().unwrap()) as i64,
                    _ => i64::from_le_bytes(v),
                };
                Some((self.string(FIELD, f, 1).to_string(), v))
            })
            .collect()
    }

    pub fn rows(&self, table: usize) -> u32 {
        self.rows[table]
    }

    /// Column `col` of `row` (1-based): the raw value, coded indices decoded with `token`.
    pub fn cell(&self, table: usize, row: u32, col: usize) -> u32 {
        let cols = schema(table);
        let mut o = self.table_start[table] + (row as usize - 1) * self.row_size[table];
        for (i, &c) in cols.iter().enumerate() {
            let size = match c {
                Col::U16 => 2,
                Col::U32 => 4,
                Col::Str => 2 + 2 * self.wide_str as usize,
                Col::Guid => 2 + 2 * self.wide_guid as usize,
                Col::Blob => 2 + 2 * self.wide_blob as usize,
                Col::T(t) => 2 + 2 * (self.rows[t] > 0xFFFF) as usize,
                Col::C(ts) => {
                    let bits = usize::BITS - (ts.len() - 1).leading_zeros();
                    let max = ts.iter().filter(|&&t| t != NA).map(|&t| self.rows[t]).max().unwrap_or(0);
                    2 + 2 * (max >= 1 << (16 - bits)) as usize
                }
            };
            if i == col {
                return if size == 2 { u16_at(&self.bytes, o) as u32 } else { u32_at(&self.bytes, o) };
            }
            o += size;
        }
        panic!("column {col} past table {table:#x}");
    }

    /// A coded-index column decoded to (table, row); row 0 = null.
    pub fn coded(&self, table: usize, row: u32, col: usize) -> Token {
        let Col::C(ts) = schema(table)[col] else { panic!("column {col} of {table:#x} is not coded") };
        let bits = usize::BITS - (ts.len() - 1).leading_zeros();
        let v = self.cell(table, row, col);
        (ts[(v & ((1 << bits) - 1)) as usize], v >> bits)
    }

    pub fn string(&self, table: usize, row: u32, col: usize) -> &str {
        let o = self.strings + self.cell(table, row, col) as usize;
        let n = self.bytes[o..].iter().position(|&c| c == 0).unwrap();
        std::str::from_utf8(&self.bytes[o..o + n]).unwrap_or("")
    }

    /// The module's Mvid as Mono prints it in a stack frame: the GUID as 32 hex digits (its three leading fields
    /// big-endian, as written out, then the last 8 bytes).
    pub fn mvid(&self) -> String {
        let o = self.guids + (self.cell(0x00, 1, 2) as usize - 1) * 16;
        [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15].iter().map(|i| format!("{:02x}", self.bytes[o + i])).collect()
    }

    pub fn blob(&self, table: usize, row: u32, col: usize) -> &[u8] {
        let o = self.blobs + self.cell(table, row, col) as usize;
        let (n, len) = compressed(&self.bytes[o..]).unwrap();
        &self.bytes[o + len..o + len + n as usize]
    }

    /// A TypeDefOrRefEncoded value from a signature.
    pub fn type_def_or_ref(v: u32) -> Token {
        (TYPE_DEF_OR_REF[(v & 3) as usize], v >> 2)
    }

    fn index(&mut self) {
        let mut nested_in = HashMap::new();
        for r in 1..=self.rows[NESTED_CLASS] {
            nested_in.insert(self.cell(NESTED_CLASS, r, 0), self.cell(NESTED_CLASS, r, 1));
        }
        for t in 1..=self.rows[TYPE_DEF] {
            let mut name = self.string(TYPE_DEF, t, 1).to_string();
            let mut outer = t;
            while let Some(&o) = nested_in.get(&outer) {
                name = format!("{}/{name}", self.string(TYPE_DEF, o, 1));
                outer = o;
            }
            let namespace = self.string(TYPE_DEF, outer, 2).to_string();
            self.types.insert((namespace, name), t);
        }
        self.field_owner = vec![0; self.rows[FIELD] as usize + 1];
        for t in 1..=self.rows[TYPE_DEF] {
            let (start, end) = self.field_range(t);
            for f in start..end {
                self.field_owner[f as usize] = t;
            }
        }
        for r in 1..=self.rows[CUSTOM_ATTRIBUTE] {
            let parent = self.coded(CUSTOM_ATTRIBUTE, r, 0);
            let ctor = self.coded(CUSTOM_ATTRIBUTE, r, 1);
            self.attributes.entry(parent).or_default().push(ctor);
        }
    }

    pub fn type_def(&self, namespace: &str, name: &str) -> Option<u32> {
        self.types.get(&(namespace.to_string(), name.to_string())).copied()
    }

    /// The TypeDef's fields, [start, end) rows of the Field table.
    pub fn field_range(&self, t: u32) -> (u32, u32) {
        let start = self.cell(TYPE_DEF, t, 4);
        let end = if t < self.rows[TYPE_DEF] { self.cell(TYPE_DEF, t + 1, 4) } else { self.rows[FIELD] + 1 };
        (start, end)
    }

    /// (namespace, name) of the class that declares a custom attribute's constructor.
    pub fn attribute_class(&self, ctor: Token) -> Option<(String, String)> {
        let class = match ctor.0 {
            METHOD_DEF => (TYPE_DEF, self.method_owner(ctor.1)?),
            MEMBER_REF => self.coded(MEMBER_REF, ctor.1, 0),
            _ => return None,
        };
        self.type_name(class)
    }

    fn method_owner(&self, m: u32) -> Option<u32> {
        (1..=self.rows[TYPE_DEF]).rev().find(|&t| self.cell(TYPE_DEF, t, 5) <= m)
    }

    /// (namespace, name) of a TypeDef or TypeRef.
    pub fn type_name(&self, t: Token) -> Option<(String, String)> {
        match t.0 {
            TYPE_DEF => self.types.iter().find(|(_, r)| **r == t.1).map(|(k, _)| k.clone()),
            TYPE_REF => {
                let mut name = self.string(TYPE_REF, t.1, 1).to_string();
                let mut cur = t.1;
                while let (TYPE_REF, outer) = self.coded(TYPE_REF, cur, 0) {
                    name = format!("{}/{name}", self.string(TYPE_REF, outer, 1));
                    cur = outer;
                }
                Some((self.string(TYPE_REF, cur, 2).to_string(), name))
            }
            _ => None,
        }
    }

    /// The assembly a TypeRef resolves into (None = this one).
    pub fn type_ref_assembly(&self, r: u32) -> Option<String> {
        let mut cur = r;
        loop {
            match self.coded(TYPE_REF, cur, 0) {
                (TYPE_REF, outer) => cur = outer,
                (ASSEMBLY_REF, a) => return Some(self.string(ASSEMBLY_REF, a, 6).to_string()),
                _ => return None,
            }
        }
    }

    /// The assembly an ExportedType forwards (namespace, name) to.
    pub fn forwarded(&self, namespace: &str, name: &str) -> Option<String> {
        (1..=self.rows[EXPORTED_TYPE])
            .find(|&r| self.string(EXPORTED_TYPE, r, 2) == name && self.string(EXPORTED_TYPE, r, 3) == namespace)
            .and_then(|r| match self.coded(EXPORTED_TYPE, r, 4) {
                (ASSEMBLY_REF, a) => Some(self.string(ASSEMBLY_REF, a, 6).to_string()),
                _ => None,
            })
    }

    pub fn attributes(&self, parent: Token) -> &[Token] {
        self.attributes.get(&parent).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn has_attribute(&self, parent: Token, namespace: &str, name: &str) -> bool {
        self.attributes(parent)
            .iter()
            .any(|&c| self.attribute_class(c).is_some_and(|(n, c)| n == namespace && c == name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tools/progress/methods.tsv (namespace, type path, method, IL bytes) for the DLL in PPG_DLL, written to PPG_OUT:
    /// the rewrite map's list of game functions, rebuilt after a game update.
    #[test]
    #[ignore]
    fn dump_methods() {
        let (Ok(dll), Ok(out)) = (std::env::var("PPG_DLL"), std::env::var("PPG_OUT")) else { return };
        let a = Assembly::open(Path::new(&dll)).unwrap();
        let mut types: Vec<(&u32, &(String, String))> = a.types.iter().map(|(k, v)| (v, k)).collect();
        types.sort();
        let mut text = String::new();
        for (&t, (ns, name)) in types {
            // A nested type is listed without its outer type's namespace, as before.
            let ns = if name.contains('/') { "" } else { ns.as_str() };
            let (start, end) = a.method_range(t);
            for m in start..end {
                if let Some(il) = a.method_il(m) {
                    text += &format!("{ns}	{name}	{}	{}
", a.string(METHOD_DEF, m, 3), il.len());
                }
            }
        }
        std::fs::write(out, text).unwrap();
    }

    #[test]
    fn assembly_csharp_types() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let a = Assembly::open(&game.join("People Playground_Data/Managed/Assembly-CSharp.dll")).unwrap();
        let t = a.type_def("", "LimbBehaviour").expect("LimbBehaviour");
        let (start, end) = a.field_range(t);
        let names: Vec<&str> = (start..end).map(|f| a.string(FIELD, f, 1)).collect();
        assert!(names.contains(&"Health") && names.contains(&"InitialHealth"), "{names:?}");
        assert_eq!(a.type_name((TYPE_DEF, t)).unwrap(), (String::new(), "LimbBehaviour".into()));
    }
}
