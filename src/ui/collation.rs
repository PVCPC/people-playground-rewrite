//! `string.CompareTo` of the game's Mono (CompareInfo.Compare, CompareOptions.None, the current culture = en-US):
//! Mono.Globalization.Unicode.SimpleCollator.Compare over the collation tables Mono embeds in mscorlib.dll
//! (resources collation.core.bin and collation.tailoring.bin, read from the user's install at run time) and
//! CharUnicodeInfo's category tables (static arrays in the same assembly).
//!
//! The culture is fixed to en-US: its tailoring chain is 1033, 9 (en), then the invariant entry (127), so there are no
//! CJK tables, no French sort, and the Italian-only U+2015 case never applies.

use crate::assets::clr::Assembly;
use std::cmp::Ordering;
use std::sync::OnceLock;

/// CultureInfo.CurrentCulture.LCID and its parents' (en-US, en); LCID 127 (invariant) ends the chain.
const CULTURE_CHAIN: [u32; 2] = [1033, 9];

struct Contraction {
    source: Vec<u16>,
    replacement: Option<Vec<u16>>,
    sort_key: Option<[u8; 4]>,
}

struct Tables {
    ignorable: Vec<u8>,
    category: Vec<u8>,
    level1: Vec<u8>,
    level2: Vec<u8>,
    level3: Vec<u8>,
    /// CharUnicodeInfo: s_pCategoryLevel1Index (as bytes, little endian) and s_pCategoriesValue.
    char_index: Vec<u8>,
    char_values: Vec<u8>,
    contractions: Vec<Contraction>,
    invariant: Vec<Contraction>,
    french_sort: bool,
    /// SimpleCollator.unsafeFlags
    unsafe_flags: [u8; 96],
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).ok_or_else(|| "collation table truncated".to_string())
}

/// MSCompatUnicodeTable's static constructor: (tailoring infos (lcid, index, count, french), tailoring chars).
type Tailoring = (Vec<(u32, usize, usize, bool)>, Vec<u16>);

fn parse_tailoring(b: &[u8]) -> Result<Tailoring, String> {
    if b.first() != Some(&3) {
        return Err("collation.tailoring.bin: unknown version".into());
    }
    let n = u32_at(b, 1)? as usize;
    let mut o = 5;
    let mut infos = Vec::new();
    for _ in 0..n {
        infos.push((u32_at(b, o)?, u32_at(b, o + 4)? as usize, u32_at(b, o + 8)? as usize, *b.get(o + 12).ok_or("truncated")? != 0));
        o += 13;
    }
    o += 2;
    let count = u32_at(b, o)? as usize;
    o += 4;
    let chars = b.get(o..o + 2 * count).ok_or("collation.tailoring.bin truncated")?;
    Ok((infos, chars.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()))
}

/// MSCompatUnicodeTable.BuildTailoringTables (the contractions; level-2 maps only matter for Korean).
fn build_contractions(chars: &[u16], index: usize, count: usize) -> Result<Vec<Contraction>, String> {
    let mut list: Vec<(usize, Contraction)> = Vec::new();
    let (mut i, end) = (index, index + count);
    while i < end {
        let mut j = i + 1;
        match chars[i] {
            1 => {
                i += 1;
                while chars[j] != 0 {
                    j += 1;
                }
                let key = [0, 1, 2, 3].map(|k| chars[j + 1 + k] as u8);
                list.push((list.len(), Contraction { source: chars[i..j].to_vec(), replacement: None, sort_key: Some(key) }));
                i = j + 6;
            }
            2 => i += 3,
            3 => {
                i += 1;
                while chars[j] != 0 {
                    j += 1;
                }
                let source = chars[i..j].to_vec();
                j += 1;
                let mut k = j;
                while chars[k] != 0 {
                    k += 1;
                }
                list.push((list.len(), Contraction { source, replacement: Some(chars[j..k].to_vec()), sort_key: None }));
                i = k + 1;
            }
            _ => return Err(format!("collation tailoring table is broken at {i:#x}")),
        }
    }
    // ContractionComparer: by source chars, then length, then index.
    list.sort_by(|(ia, a), (ib, b)| {
        let n = a.source.len().min(b.source.len());
        a.source[..n].cmp(&b.source[..n]).then(a.source.len().cmp(&b.source.len())).then(ia.cmp(ib))
    });
    Ok(list.into_iter().map(|(_, c)| c).collect())
}

fn load() -> Result<Tables, String> {
    let game = crate::game_assets::find_game(None)?;
    let asm = Assembly::open(&game.join("People Playground_Data/Managed/mscorlib.dll"))?;
    let core = asm.resource("collation.core.bin").ok_or("mscorlib has no collation.core.bin")?;
    let tailoring = asm.resource("collation.tailoring.bin").ok_or("mscorlib has no collation.tailoring.bin")?;
    if core.first() != Some(&3) {
        return Err("collation.core.bin: unknown version".into());
    }
    let mut o = 1;
    let mut parts = Vec::new();
    for _ in 0..5 {
        let n = u32_at(core, o)? as usize;
        o += 4;
        parts.push(core.get(o..o + n).ok_or("collation.core.bin truncated")?.to_vec());
        o += n;
    }
    let (infos, chars) = parse_tailoring(tailoring)?;
    // SimpleCollator's constructor: the first tailoring found walking the culture chain, else the invariant one (127).
    let find = |lcid: u32| infos.iter().find(|i| i.0 == lcid);
    let own = CULTURE_CHAIN.iter().find_map(|&l| find(l)).or_else(|| find(127)).ok_or("no invariant collation tailoring")?;
    let inv = find(127).ok_or("no invariant collation tailoring")?;
    let contractions = build_contractions(&chars, own.1, own.2)?;
    let invariant = build_contractions(&chars, inv.1, inv.2)?;
    let mut unsafe_flags = [0u8; 96];
    for c in contractions.iter().chain(&invariant).filter(|c| c.source.len() > 1) {
        for &ch in &c.source {
            if let Some(f) = unsafe_flags.get_mut(ch as usize / 8) {
                *f |= 1 << (ch & 7);
            }
        }
    }
    let mut it = parts.into_iter();
    let mut next = || it.next().unwrap();
    Ok(Tables {
        ignorable: next(),
        category: next(),
        level1: next(),
        level2: next(),
        level3: next(),
        char_index: asm.field_data(10626 * 2).ok_or("mscorlib: CharUnicodeInfo category index not found")?.to_vec(),
        char_values: asm.field_data(162).ok_or("mscorlib: CharUnicodeInfo category values not found")?.to_vec(),
        contractions,
        invariant,
        french_sort: own.3,
        unsafe_flags,
    })
}

/// CodePointIndexer.ToIndex over (start, end) ranges packed one after another; `default` outside them.
fn to_index(ranges: &[(i32, i32)], default: i32, cp: i32) -> i32 {
    let mut index_start = 0;
    for &(start, end) in ranges {
        if cp < start {
            return default;
        }
        if cp < end {
            return cp - start + index_start;
        }
        index_start += end - start;
    }
    default
}

// MSCompatUnicodeTableUtil
const IGNORABLE: [(i32, i32); 3] = [(0, 13312), (40960, 42240), (63744, 65536)];
const CATEGORY: [(i32, i32); 6] = [(0, 4608), (7680, 10240), (12288, 13312), (19968, 40960), (44032, 55216), (63744, 65536)];
const LEVEL2: [(i32, i32); 4] = [(0, 3840), (7680, 10240), (12288, 13312), (64256, 65536)];
const LEVEL3: [(i32, i32); 4] = [(0, 4608), (7680, 10240), (12288, 13312), (64256, 65536)];

#[derive(Clone, Copy, PartialEq)]
enum Ext {
    None,
    Simple,
    Voiced,
    Conditional,
    Buggy,
}

/// SimpleCollator.GetExtenderType (lcid 16, Italian, is the only culture where U+2015 is an extender).
fn extender(i: i32) -> Ext {
    if !(12293..=65392).contains(&i) {
        return Ext::None;
    }
    match i {
        65148 | 65149 => Ext::Simple,
        65392 => Ext::Conditional,
        65438 | 65439 => Ext::Voiced,
        i if i > 12542 => Ext::None,
        12293 => Ext::Buggy,
        12337 | 12338 | 12445 | 12541 => Ext::Simple,
        12446 | 12542 => Ext::Voiced,
        12540 => Ext::Conditional,
        _ => Ext::None,
    }
}

fn has_special_weight(c: i32) -> bool {
    if c < 0x3041 {
        return false;
    }
    if (0xFF66..0xFF9E).contains(&c) {
        return true;
    }
    if c >= 0x3300 {
        return false;
    }
    if c < 0x309D {
        return c < 0x3099;
    }
    if c < 0x3100 {
        return c != 0x30FB;
    }
    if c < 0x32D0 {
        return false;
    }
    c < 0x32FF
}

fn is_half_width_kana(c: i32) -> bool {
    (0xFF66..=0xFF9D).contains(&c)
}

fn is_hiragana(c: i32) -> bool {
    (0x3041..=0x3094).contains(&c)
}

fn is_japanese_small_letter(c: i32) -> bool {
    (0xFF67..=0xFF6F).contains(&c)
        || matches!(c, 0x3041 | 0x3043 | 0x3045 | 0x3047 | 0x3049 | 0x3063 | 0x3083 | 0x3085 | 0x3087 | 0x308E | 0x30A1 | 0x30A3 | 0x30A5 | 0x30A7 | 0x30A9 | 0x30C3 | 0x30E3 | 0x30E5 | 0x30E7 | 0x30EE | 0x30F5 | 0x30F6)
}

/// SimpleCollator.ToDashTypeValue with CompareOptions.None.
fn dash_type(e: Ext) -> i32 {
    match e {
        Ext::None => 3,
        Ext::Conditional => 5,
        _ => 4,
    }
}

fn pair(b1: bool, b2: bool) -> i32 {
    match (b1, b2) {
        (false, true) => -1,
        (true, false) => 1,
        _ => 0,
    }
}

impl Tables {
    fn category(&self, cp: i32) -> u8 {
        self.category[to_index(&CATEGORY, 0, cp) as usize]
    }
    fn level1(&self, cp: i32) -> u8 {
        self.level1[to_index(&CATEGORY, 0, cp) as usize]
    }
    fn level2(&self, cp: i32, ext: Ext) -> u8 {
        match ext {
            Ext::Buggy => 5,
            Ext::Conditional => 0,
            _ => self.level2[to_index(&LEVEL2, 0, cp) as usize],
        }
    }
    fn level3(&self, cp: i32) -> u8 {
        self.level3[to_index(&LEVEL3, 0, cp) as usize]
    }

    /// char.GetUnicodeCategory(c) == OtherNotAssigned (CharUnicodeInfo.InternalGetCategoryValue).
    fn unassigned(&self, ch: i32) -> bool {
        let w = |i: usize| u16::from_le_bytes([self.char_index[2 * i], self.char_index[2 * i + 1]]) as usize;
        let n = w((ch >> 8) as usize);
        let n = w(n + ((ch >> 4) & 0xF) as usize);
        self.char_values[self.char_index[2 * n + (ch & 0xF) as usize] as usize * 2] == 29
    }

    /// MSCompatUnicodeTable.IsIgnorable(cp, flag).
    fn ignorable(&self, cp: i32, flag: u8) -> bool {
        if cp == 0 {
            return true;
        }
        if flag & 1 != 0 && (self.unassigned(cp) || (55424..56192).contains(&cp)) {
            return true;
        }
        let i = to_index(&IGNORABLE, -1, cp);
        i >= 0 && self.ignorable[i as usize] & flag != 0
    }

    fn is_safe(&self, i: i32) -> bool {
        self.unsafe_flags.get(i as usize / 8).is_none_or(|f| f & (1 << (i % 8)) == 0)
    }

    fn get_contraction(&self, s: &[u16], start: i32, end: i32) -> Option<&Contraction> {
        let find = |list: &'_ [Contraction]| -> Option<usize> {
            for (k, c) in list.iter().enumerate() {
                let d = c.source[0] as i32 - s[start as usize] as i32;
                if d > 0 {
                    return None;
                }
                if d < 0 || ((end - start) as usize) < c.source.len() {
                    continue;
                }
                if s[start as usize..start as usize + c.source.len()] == c.source[..] {
                    return Some(k);
                }
            }
            None
        };
        find(&self.contractions).map(|k| &self.contractions[k]).or_else(|| find(&self.invariant).map(|k| &self.invariant[k]))
    }

    /// SimpleCollator.FilterExtender (CompareOptions.None).
    fn filter_extender(&self, i: i32, ext: Ext) -> i32 {
        if ext == Ext::Conditional && has_special_weight(i) {
            let half = is_half_width_kana(i);
            let katakana = !is_hiragana(i);
            let (hira, kata, hw) = match self.level1(i) & 7 {
                2 => (12354, 12450, 65393),
                3 => (12356, 12452, 65394),
                4 => (12358, 12454, 65395),
                5 => (12360, 12456, 65396),
                6 => (12362, 12458, 65397),
                _ => return i,
            };
            return if half { hw } else if katakana { kata } else { hira };
        }
        i
    }
}

struct Escape {
    src: Vec<u16>,
    start: i32,
    index: i32,
    end: i32,
    optional: i32,
}

/// SimpleCollator.Compare(s1, s2) with CompareOptions.None (CompareInternal, skipHeadingExtenders, no immediate break-up).
fn compare(t: &Tables, a: &str, b: &str) -> Ordering {
    let mut s1: Vec<u16> = a.encode_utf16().collect();
    let mut s2: Vec<u16> = b.encode_utf16().collect();
    let (mut start1, mut start2) = (0i32, 0i32);
    let (mut end1, mut end2) = (s1.len() as i32, s2.len() as i32);
    let (mut idx1, mut idx2) = (0i32, 0i32);
    // Context (buffers and previous info of the first string) and PreviousInfo of the second.
    let (mut buf1, mut buf2) = ([0u8; 4], [0u8; 4]);
    let (mut prev_code1, mut prev_key1, mut prev_code2, mut prev_key2) = (-1i32, false, -1i32, false);
    let (mut res, mut level) = (0i32, 5);
    let (mut pos1, mut pos2, mut wt1, mut wt2) = (-1i32, -1i32, 0i32, 0i32);
    while idx1 < end1 && extender(s1[idx1 as usize] as i32) != Ext::None {
        idx1 += 1;
    }
    while idx2 < end2 && extender(s2[idx2 as usize] as i32) != Ext::None {
        idx2 += 1;
    }
    let (mut ext1, mut ext2) = (Ext::None, Ext::None);
    let (mut run1, mut run2) = (idx1, idx2);
    let (mut esc1, mut esc2): (Option<Escape>, Option<Escape>) = (None, None);
    loop {
        if idx1 < end1 && t.ignorable(s1[idx1 as usize] as i32, 1) {
            idx1 += 1;
            continue;
        }
        while idx2 < end2 && t.ignorable(s2[idx2 as usize] as i32, 1) {
            idx2 += 1;
        }
        if idx1 >= end1 {
            let Some(e) = esc1.take() else { break };
            (s1, start1, idx1, end1, run1) = (e.src, e.start, e.index, e.end, e.optional);
            continue;
        }
        if idx2 >= end2 {
            let Some(e) = esc2.take() else { break };
            (s2, start2, idx2, end2, run2) = (e.src, e.start, e.index, e.end, e.optional);
            continue;
        }
        if run1 < idx1 && run2 < idx2 {
            while idx1 < end1 && idx2 < end2 && s1[idx1 as usize] == s2[idx2 as usize] {
                idx1 += 1;
                idx2 += 1;
            }
            if idx1 == end1 || idx2 == end2 {
                continue;
            }
            let (back1, back2) = (run1, run2);
            run1 = idx1;
            run2 = idx2;
            idx1 -= 1;
            idx2 -= 1;
            while idx1 > back1 && t.category(s1[idx1 as usize] as i32) == 1 {
                idx1 -= 1;
            }
            while idx2 > back2 && t.category(s2[idx2 as usize] as i32) == 1 {
                idx2 -= 1;
            }
            while idx1 > back1 && !t.is_safe(s1[idx1 as usize] as i32) {
                idx1 -= 1;
            }
            while idx2 > back2 && !t.is_safe(s2[idx2 as usize] as i32) {
                idx2 -= 1;
            }
        }
        let (at1, at2) = (idx1, idx2);
        let (mut ptr1, mut ptr2) = (false, false);
        let mut c1 = s1[idx1 as usize] as i32;
        let mut c2 = s2[idx2 as usize] as i32;
        let (mut sp1, mut sp2) = (false, false);
        ext1 = extender(c1);
        if ext1 != Ext::None {
            if prev_code1 < 0 {
                if !prev_key1 {
                    idx1 += 1;
                    continue;
                }
                ptr1 = true;
            } else {
                c1 = t.filter_extender(prev_code1, ext1);
            }
        }
        ext2 = extender(c2);
        if ext2 != Ext::None {
            if prev_code2 < 0 {
                if !prev_key2 {
                    idx2 += 1;
                    continue;
                }
                ptr2 = true;
            } else {
                c2 = t.filter_extender(prev_code2, ext2);
            }
        }
        let (b1, b2) = (t.category(c1), t.category(c2));
        // Category 6 (hyphen, apostrophe, controls): skipped, only the first difference position and weight decide a tie.
        if b1 == 6 {
            if level == 5 {
                pos1 = esc1.as_ref().map_or(at1 - start1, |e| e.index - e.start);
                wt1 = (t.level1(c1) as i32).wrapping_shl(8 + t.level3(c1) as u32);
            }
            prev_code1 = c1;
            idx1 += 1;
        }
        if b2 == 6 {
            if level == 5 {
                pos2 = esc2.as_ref().map_or(at2 - start2, |e| e.index - e.start);
                wt2 = (t.level1(c2) as i32).wrapping_shl(8 + t.level3(c2) as u32);
            }
            prev_code2 = c2;
            idx2 += 1;
        }
        if b1 == 6 || b2 == 6 {
            if level == 5 {
                if wt1 == wt2 {
                    (pos1, pos2, wt1, wt2) = (-1, -1, 0, 0);
                } else {
                    level = 4;
                }
            }
            continue;
        }
        let contraction1 = if ext1 == Ext::None { t.get_contraction(&s1, idx1, end1) } else { None };
        let mut advance1 = 1;
        if ptr1 {
        } else if let Some(c) = contraction1 {
            advance1 = c.source.len() as i32;
            if let Some(key) = c.sort_key {
                buf1 = key;
                ptr1 = true;
                prev_code1 = -1;
                prev_key1 = true;
            } else if esc1.is_none() {
                let rep = c.replacement.clone().expect("contraction has a replacement");
                esc1 = Some(Escape { src: std::mem::replace(&mut s1, rep), start: start1, index: at1 + advance1, end: end1, optional: run1 });
                (idx1, start1, end1, run1) = (0, 0, s1.len() as i32, 0);
                continue;
            } else {
                // Mono dereferences the null SortKey of a replacement-only contraction found inside a replacement (no table of the game's
                // mscorlib has one: no_contraction_inside_a_replacement).
                crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
            }
        } else {
            buf1[0] = b1;
            buf1[1] = t.level1(c1);
            if level > 1 {
                buf1[2] = t.level2(c1, ext1);
            }
            if level > 2 {
                buf1[3] = t.level3(c1);
            }
            if level > 3 {
                sp1 = has_special_weight(c1);
            }
            if b1 > 1 {
                prev_code1 = c1;
            }
            ptr1 = true;
        }
        let contraction2 = if ext2 == Ext::None { t.get_contraction(&s2, idx2, end2) } else { None };
        if ptr2 {
            idx2 += 1;
        } else if let Some(c) = contraction2 {
            idx2 += c.source.len() as i32;
            if let Some(key) = c.sort_key {
                buf2 = key;
                ptr2 = true;
                prev_code2 = -1;
                prev_key2 = true;
            } else if esc2.is_none() {
                let rep = c.replacement.clone().expect("contraction has a replacement");
                esc2 = Some(Escape { src: std::mem::replace(&mut s2, rep), start: start2, index: at2 + c.source.len() as i32, end: end2, optional: run2 });
                (idx2, start2, end2, run2) = (0, 0, s2.len() as i32, 0);
                continue;
            } else {
                // Mono dereferences the null SortKey of a replacement-only contraction found inside a replacement (no table of the game's
                // mscorlib has one: no_contraction_inside_a_replacement).
                crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
            }
        } else {
            buf2[0] = b2;
            buf2[1] = t.level1(c2);
            if level > 1 {
                buf2[2] = t.level2(c2, ext2);
            }
            if level > 2 {
                buf2[3] = t.level3(c2);
            }
            if level > 3 {
                sp2 = has_special_weight(c2);
            }
            if b2 > 1 {
                prev_code2 = c2;
            }
            idx2 += 1;
            ptr2 = true;
        }
        debug_assert!(ptr1 && ptr2);
        idx1 += advance1;
        // Combining marks (category 1) after a base character add to its level 2 weight.
        while idx1 < end1 && t.category(s1[idx1 as usize] as i32) == 1 {
            if buf1[2] == 0 {
                buf1[2] = 2;
            }
            buf1[2] = buf1[2].wrapping_add(t.level2(s1[idx1 as usize] as i32, Ext::None));
            idx1 += 1;
        }
        while idx2 < end2 && t.category(s2[idx2 as usize] as i32) == 1 {
            if buf2[2] == 0 {
                buf2[2] = 2;
            }
            buf2[2] = buf2[2].wrapping_add(t.level2(s2[idx2 as usize] as i32, Ext::None));
            idx2 += 1;
        }
        let mut d = buf1[0] as i32 - buf2[0] as i32;
        if d == 0 {
            d = buf1[1] as i32 - buf2[1] as i32;
        }
        if d != 0 {
            return d.cmp(&0);
        }
        if level == 1 {
            continue;
        }
        d = buf1[2] as i32 - buf2[2] as i32;
        if d != 0 {
            res = d;
            level = if t.french_sort { 2 } else { 1 };
            continue;
        }
        if level == 2 {
            continue;
        }
        d = buf1[3] as i32 - buf2[3] as i32;
        if d != 0 {
            res = d;
            level = 2;
        } else {
            if level == 3 {
                continue;
            }
            if sp1 != sp2 {
                res = if sp1 { 1 } else { -1 };
                level = 3;
            } else {
                if !sp1 {
                    continue;
                }
                d = pair(!is_japanese_small_letter(c1), !is_japanese_small_letter(c2));
                if d == 0 {
                    d = dash_type(ext1) - dash_type(ext2);
                }
                if d == 0 {
                    d = pair(is_hiragana(c1), is_hiragana(c2));
                }
                if d == 0 {
                    d = pair(!is_half_width_kana(c1), !is_half_width_kana(c2));
                }
                if d != 0 {
                    res = d;
                    level = 3;
                }
            }
        }
    }
    let nonspacing = |c: u16| t.ignorable(c as i32, 4);
    if res != 0 && level > 2 {
        while idx1 < end1 && idx2 < end2 && nonspacing(s1[idx1 as usize]) && nonspacing(s2[idx2 as usize]) {
            res = t.level2(s1[idx1 as usize] as i32, ext1) as i32 - t.level2(s2[idx2 as usize] as i32, ext2) as i32;
            if res != 0 {
                break;
            }
            idx1 += 1;
            idx2 += 1;
            (ext1, ext2) = (Ext::None, Ext::None);
        }
    }
    if level == 1 && res != 0 {
        while idx1 < end1 && nonspacing(s1[idx1 as usize]) {
            idx1 += 1;
        }
        while idx2 < end2 && nonspacing(s2[idx2 as usize]) {
            idx2 += 1;
        }
    }
    if res == 0 {
        res = if pos1 < 0 && pos2 >= 0 {
            -1
        } else if pos2 < 0 && pos1 >= 0 {
            1
        } else if pos1 - pos2 != 0 {
            pos1 - pos2
        } else {
            wt1 - wt2
        };
    }
    if idx1 == end1 {
        if idx2 != end2 { Ordering::Less } else { res.cmp(&0) }
    } else {
        Ordering::Greater
    }
}

static TABLES: OnceLock<Result<Tables, String>> = OnceLock::new();

/// Reads the collation tables from the game's mscorlib at startup: a game that cannot supply its own mscorlib does not start, like
/// one that is not installed.
pub fn preload() -> Result<(), String> {
    TABLES.get_or_init(load).as_ref().map(|_| ()).map_err(|e| format!("the collation tables could not be read from the game's mscorlib: {e}"))
}

/// string.CompareTo(string): Mono's SimpleCollator over the tables of the installed game (read from its mscorlib by `preload`).
pub fn culture_cmp(a: &str, b: &str) -> Ordering {
    match TABLES.get_or_init(load) {
        Ok(t) => compare(t, a, b),
        Err(e) => panic!("the collation tables could not be read from the game's mscorlib: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::*;

    /// None when there is no game install to read the tables from.
    fn cmp(a: &str, b: &str) -> Option<Ordering> {
        crate::game_assets::find_game(None).ok().map(|_| culture_cmp(a, b))
    }

    /// No replacement of the tables the collator walks holds another replacement-only contraction, so a contraction cannot be found
    /// inside a contraction's replacement.
    #[test]
    fn no_contraction_inside_a_replacement() {
        if crate::game_assets::find_game(None).is_err() {
            return;
        }
        let t = TABLES.get_or_init(load).as_ref().unwrap();
        let mut replaced = 0;
        for c in t.contractions.iter().chain(&t.invariant) {
            let Some(rep) = &c.replacement else { continue };
            replaced += 1;
            for i in 0..rep.len() {
                if let Some(inner) = t.get_contraction(rep, i as i32, rep.len() as i32) {
                    assert!(inner.sort_key.is_some(), "{:?} inside the replacement of {:?}", inner.source, c.source);
                }
            }
        }
        println!("{replaced} replacements");
    }

    #[test]
    fn tables_load_from_the_game() {
        if crate::game_assets::find_game(None).is_err() {
            return;
        }
        let t = load().unwrap();
        // Table 0 of the tailoring resource: the invariant culture has contractions (e.g. the ligature expansions).
        assert!(!t.invariant.is_empty());
    }

    #[test]
    fn non_ascii_order() {
        // Level 1 (letter) decides before accents: "e" < "é" < "f", and an accent is a level 2 tie-break only.
        assert!(matches!(cmp("e", "\u{e9}"), None | Some(Less)));
        assert!(matches!(cmp("\u{e9}", "f"), None | Some(Less)));
        assert!(matches!(cmp("\u{e9}a", "eb"), None | Some(Less)));
        assert!(matches!(cmp("eb", "\u{e9}a"), None | Some(Greater)));
        // Expansion: U+00E6 (ae ligature) sorts as "ae" first, then breaks the tie against it at a later level.
        assert!(matches!(cmp("\u{e6}", "af"), None | Some(Less)));
        assert!(matches!(cmp("\u{e6}", "ad"), None | Some(Greater)));
        // U+200D (zero width joiner) is ignorable in the table: no weight at all.
        assert!(matches!(cmp("co\u{200d}op", "coop"), None | Some(Equal)));
        // U+00AD is category 6 like the hyphen: skipped, then only a tie-break.
        assert!(matches!(cmp("co\u{ad}op", "coop"), None | Some(Greater)));
        assert!(matches!(cmp("co\u{ad}op", "cool"), None | Some(Greater)));
        // Expansion (sharp s = "ss"): between "sr" and "st".
        assert!(matches!(cmp("\u{df}", "sr"), None | Some(Greater)));
        assert!(matches!(cmp("\u{df}", "st"), None | Some(Less)));
    }
}
