//! Json.NET 13's JsonTextReader as the game reads its JSON files with it (JsonConvert.DeserializeObject /
//! DeserializeAnonymousType, default settings): comments, single-quoted and unquoted property names, trailing commas,
//! NaN / Infinity / undefined / `new Name(..)` values, any char.IsWhiteSpace as whitespace, and only whitespace or
//! comments after the root value. Numbers keep their text: each target type converts it as Json.NET does.

/// A JSON token tree. Objects keep every property in file order (duplicates too: the last one wins when read).
#[derive(Clone, Debug, PartialEq)]
pub enum JTok {
    Null,
    Undefined,
    Bool(bool),
    Str(String),
    /// A number's text as written.
    Num(String),
    /// NaN, Infinity, -Infinity.
    Special(&'static str),
    Object(Vec<(String, JTok)>),
    Array(Vec<JTok>),
    /// `new Name(args)`
    Ctor(String, Vec<JTok>),
}

/// File.ReadAllText: the encoding from the byte order mark (UTF-8 without one), bad bytes as U+FFFD.
pub fn read_all_text(path: &std::path::Path) -> std::io::Result<String> {
    let b = std::fs::read(path)?;
    let utf16 = |b: &[u8], be: bool| String::from_utf16_lossy(&b.chunks(2).map(|c| if c.len() == 2 { if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) } } else { 0xFFFD }).collect::<Vec<_>>());
    Ok(match b.as_slice() {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, 0, 0, rest @ ..] | [0, 0, 0xFE, 0xFF, rest @ ..] => {
            let be = b[0] == 0;
            rest.chunks(4).map(|c| if c.len() == 4 { char::from_u32(if be { u32::from_be_bytes([c[0], c[1], c[2], c[3]]) } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) }).unwrap_or('\u{FFFD}') } else { '\u{FFFD}' }).collect()
        }
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        _ => String::from_utf8_lossy(&b).into_owned(),
    })
}

/// DeserializeObject's reading: None for a text with no value (DeserializeObject returns null), else the root token.
pub fn parse(text: &str) -> Result<Option<JTok>, String> {
    let mut r = Reader { c: text.chars().collect(), p: 0 };
    r.skip()?;
    if r.p >= r.c.len() {
        return Ok(None);
    }
    let v = r.value()?;
    r.skip()?;
    if r.p < r.c.len() {
        return Err("Additional text encountered after finished reading JSON content".into());
    }
    Ok(Some(v))
}

struct Reader {
    c: Vec<char>,
    p: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.c.get(self.p).copied()
    }
    /// Whitespace and comments.
    fn skip(&mut self) -> Result<(), String> {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.p += 1;
            } else if ch == '/' {
                match self.c.get(self.p + 1) {
                    Some('*') => {
                        self.p += 2;
                        loop {
                            match self.peek() {
                                None => return Err("Unexpected end while parsing comment".into()),
                                Some('*') if self.c.get(self.p + 1) == Some(&'/') => {
                                    self.p += 2;
                                    break;
                                }
                                _ => self.p += 1,
                            }
                        }
                    }
                    Some('/') => {
                        while self.peek().is_some_and(|c| c != '\r' && c != '\n') {
                            self.p += 1;
                        }
                    }
                    _ => return Err("Error parsing comment".into()),
                }
            } else {
                break;
            }
        }
        Ok(())
    }
    fn separator_next(&self) -> bool {
        self.peek().map_or(true, |c| c.is_whitespace() || matches!(c, ',' | '}' | ']' | ')' | '/'))
    }
    fn word(&mut self, w: &str) -> Result<(), String> {
        for ch in w.chars() {
            if self.peek() != Some(ch) {
                return Err(format!("Error parsing {w} value"));
            }
            self.p += 1;
        }
        if !self.separator_next() {
            return Err(format!("Error parsing {w} value"));
        }
        Ok(())
    }
    fn string(&mut self, q: char) -> Result<String, String> {
        self.p += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let Some(ch) = self.peek() else { return Err("Unterminated string".into()) };
            self.p += 1;
            if ch == q {
                return Ok(String::from_utf16_lossy(&units));
            }
            if ch != '\\' {
                let mut b = [0u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut b));
                continue;
            }
            let Some(e) = self.peek() else { return Err("Unterminated string".into()) };
            self.p += 1;
            let u = match e {
                'b' => 8,
                't' => 9,
                'n' => 10,
                'f' => 12,
                'r' => 13,
                '\\' => '\\' as u16,
                '"' => '"' as u16,
                '\'' => '\'' as u16,
                '/' => '/' as u16,
                'u' => {
                    let h: String = self.c.get(self.p..self.p + 4).ok_or("Unexpected end while parsing Unicode escape sequence")?.iter().collect();
                    self.p += 4;
                    u16::from_str_radix(&h, 16).map_err(|_| "Invalid Unicode escape sequence")?
                }
                _ => return Err(format!("Bad JSON escape sequence: \\{e}")),
            };
            units.push(u);
        }
    }
    fn number(&mut self) -> Result<JTok, String> {
        let at = self.p;
        while self.peek().is_some_and(|c| matches!(c, '-' | '.' | '0'..='9' | 'a'..='f' | 'A'..='F' | 'x' | 'X' | '+')) {
            self.p += 1;
        }
        if !self.separator_next() {
            return Err("Unexpected character encountered while parsing number".into());
        }
        Ok(JTok::Num(self.c[at..self.p].iter().collect()))
    }
    fn value(&mut self) -> Result<JTok, String> {
        self.skip()?;
        let Some(ch) = self.peek() else { return Err("Unexpected end when reading JSON".into()) };
        match ch {
            '"' | '\'' => self.string(ch).map(JTok::Str),
            't' => self.word("true").map(|_| JTok::Bool(true)),
            'f' => self.word("false").map(|_| JTok::Bool(false)),
            'n' if self.c.get(self.p + 1) == Some(&'e') => self.ctor(),
            'n' => self.word("null").map(|_| JTok::Null),
            'u' => self.word("undefined").map(|_| JTok::Undefined),
            'N' => self.word("NaN").map(|_| JTok::Special("NaN")),
            'I' => self.word("Infinity").map(|_| JTok::Special("Infinity")),
            '-' if self.c.get(self.p + 1) == Some(&'I') => self.word("-Infinity").map(|_| JTok::Special("-Infinity")),
            '-' | '.' | '0'..='9' => self.number(),
            '{' => self.object(),
            '[' => {
                self.p += 1;
                let mut out = Vec::new();
                loop {
                    self.skip()?;
                    if self.peek() == Some(']') {
                        self.p += 1;
                        return Ok(JTok::Array(out));
                    }
                    out.push(self.value()?);
                    self.skip()?;
                    match self.peek() {
                        Some(',') => self.p += 1,
                        Some(']') => {}
                        _ => return Err("After parsing a value an unexpected character was encountered".into()),
                    }
                }
            }
            _ => Err(format!("Unexpected character encountered while parsing value: {ch}")),
        }
    }
    fn object(&mut self) -> Result<JTok, String> {
        self.p += 1;
        let mut out = Vec::new();
        loop {
            self.skip()?;
            let name = match self.peek() {
                Some('}') => {
                    self.p += 1;
                    return Ok(JTok::Object(out));
                }
                Some(q @ ('"' | '\'')) => self.string(q)?,
                _ => {
                    let at = self.p;
                    while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                        self.p += 1;
                    }
                    if self.p == at || !self.peek().is_some_and(|c| c.is_whitespace() || c == ':') {
                        return Err("Invalid property identifier character".into());
                    }
                    self.c[at..self.p].iter().collect()
                }
            };
            while self.peek().is_some_and(char::is_whitespace) {
                self.p += 1;
            }
            if self.peek() != Some(':') {
                return Err("Invalid character after parsing property name".into());
            }
            self.p += 1;
            let v = self.value()?;
            out.push((name, v));
            self.skip()?;
            match self.peek() {
                Some(',') => self.p += 1,
                Some('}') => {}
                _ => return Err("After parsing a value an unexpected character was encountered".into()),
            }
        }
    }
    fn ctor(&mut self) -> Result<JTok, String> {
        self.word_prefix("new")?;
        self.skip()?;
        let at = self.p;
        while self.peek().is_some_and(|c| c.is_alphanumeric()) {
            self.p += 1;
        }
        let name: String = self.c[at..self.p].iter().collect();
        if name.is_empty() {
            return Err("Constructor name not found".into());
        }
        self.skip()?;
        if self.peek() != Some('(') {
            return Err("Unexpected character while parsing constructor".into());
        }
        self.p += 1;
        let mut args = Vec::new();
        loop {
            self.skip()?;
            if self.peek() == Some(')') {
                self.p += 1;
                return Ok(JTok::Ctor(name, args));
            }
            args.push(self.value()?);
            self.skip()?;
            match self.peek() {
                Some(',') => self.p += 1,
                Some(')') => {}
                _ => return Err("After parsing a value an unexpected character was encountered".into()),
            }
        }
    }
    fn word_prefix(&mut self, w: &str) -> Result<(), String> {
        for ch in w.chars() {
            if self.peek() != Some(ch) {
                return Err(format!("Unexpected content while parsing JSON"));
            }
            self.p += 1;
        }
        Ok(())
    }
}

impl JTok {
    /// A member read from an object as Json.NET populates it: every property whose name matches sets it in file order
    /// (the last one wins, an error in any stops the read); None when no property matches.
    pub fn read<T>(&self, name: &str, f: impl Fn(&JTok) -> Result<T, String>) -> Result<Option<T>, String> {
        let JTok::Object(o) = self else { return Err("not an object".into()) };
        let up = |c: char| { let mut u = c.to_uppercase(); match (u.next(), u.next()) { (Some(x), None) => x, _ => c } };
        let mut out = None;
        for (_, v) in o.iter().filter(|(k, _)| k.chars().map(up).eq(name.chars().map(up))) {
            out = Some(f(v)?);
        }
        Ok(out)
    }

    /// The last property matching `name` (OrdinalIgnoreCase).
    pub fn prop(&self, name: &str) -> Option<&JTok> {
        let JTok::Object(o) = self else { return None };
        // Each property in the file sets the member it matches (StringComparison.OrdinalIgnoreCase: each char's simple upper case).
        let up = |c: char| { let mut u = c.to_uppercase(); match (u.next(), u.next()) { (Some(x), None) => x, _ => c } };
        o.iter().rev().find(|(k, _)| k.chars().map(up).eq(name.chars().map(up))).map(|(_, v)| v)
    }

    /// JsonReader.ReadAsString: a string, a number's text (when it parses), true / false, NaN / Infinity; null for null
    /// and undefined; an object, array or constructor is an error.
    pub fn as_string(&self) -> Result<Option<String>, String> {
        match self {
            JTok::Null | JTok::Undefined => Ok(None),
            JTok::Str(s) => Ok(Some(s.clone())),
            JTok::Bool(b) => Ok(Some(if *b { "true" } else { "false" }.into())),
            JTok::Special(s) => Ok(Some(s.to_string())),
            JTok::Num(s) => {
                if non_base10(s) {
                    radix_int(s).map_err(|_| format!("Input string '{s}' is not a valid number"))?;
                } else if parse_double(s).is_none() {
                    return Err(format!("Input string '{s}' is not a valid number"));
                }
                Ok(Some(s.clone()))
            }
            _ => Err("Unexpected token while reading a string".into()),
        }
    }

    /// A ulong member: Json.NET's token value (a long, a BigInteger, a double, a string, a bool) through
    /// Convert.ChangeType(value, typeof(ulong), InvariantCulture); null is an error (a value type).
    pub fn as_u64(&self) -> Result<u64, String> {
        let over = || Err::<u64, String>("Value was either too large or too small for a UInt64".into());
        match self {
            JTok::Num(s) => {
                if non_base10(s) {
                    // Convert.ToInt64(s, 16 / 8): the long, then to ulong.
                    let v = radix_int(s)? as i64;
                    return if v < 0 { over() } else { Ok(v as u64) };
                }
                if s.contains(['.', 'e', 'E']) {
                    return parse_double(s).ok_or_else(|| format!("Input string '{s}' is not a valid number")).and_then(double_to_u64);
                }
                let digits = s.strip_prefix('-').unwrap_or(s);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("Input string '{s}' is not a valid integer"));
                }
                // A long, else a BigInteger: either converts when it is in ulong's range.
                if s.starts_with('-') {
                    return if digits.bytes().all(|b| b == b'0') { Ok(0) } else { over() };
                }
                digits.parse::<u64>().or_else(|_| over())
            }
            JTok::Special(s) => double_to_u64(match *s { "NaN" => f64::NAN, "Infinity" => f64::INFINITY, _ => f64::NEG_INFINITY }),
            JTok::Str(s) => {
                // UInt64.Parse(s, NumberStyles.Integer, InvariantCulture): surrounding white space, a sign, digits.
                let t = s.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' '));
                let (neg, d) = match t.as_bytes().first() {
                    Some(b'-') => (true, &t[1..]),
                    Some(b'+') => (false, &t[1..]),
                    _ => (false, t),
                };
                if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("Input string was not in a correct format".into());
                }
                if neg {
                    return if d.bytes().all(|b| b == b'0') { Ok(0) } else { over() };
                }
                d.parse::<u64>().or_else(|_| over())
            }
            JTok::Bool(b) => Ok(*b as u64),
            _ => Err("Error converting value to type 'System.UInt64'".into()),
        }
    }

    /// JsonReader.ReadAsInt32: a number (a leading 0: Convert.ToInt32 of base 16 / 8; a fraction or out of range: an error);
    /// a string by int.TryParse; anything else (null too, for an int member) an error.
    pub fn as_i32(&self) -> Result<i32, String> {
        let bad = |s: &str| Err(format!("Input string '{s}' is not a valid integer"));
        match self {
            JTok::Num(s) if non_base10(s) => {
                let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    Some(h) => u32::from_str_radix(h, 16),
                    None => u32::from_str_radix(s, 8),
                };
                r.map(|v| v as i32).map_err(|_| format!("Could not convert '{s}'"))
            }
            JTok::Num(s) => s.parse::<i32>().or_else(|_| bad(s)),
            JTok::Str(s) => {
                let t = s.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' '));
                let t = t.strip_prefix('+').filter(|r| !r.starts_with(['+', '-'])).unwrap_or(t);
                t.parse::<i32>().map_err(|_| format!("Could not convert string to integer: {s}"))
            }
            _ => Err("Error reading integer".into()),
        }
    }

    /// JsonReader.ReadAsBoolean: a bool; a number nonzero (NaN too); a string by bool.TryParse; anything else (null too,
    /// for a bool member) an error.
    pub fn as_bool(&self) -> Result<bool, String> {
        match self {
            JTok::Bool(b) => Ok(*b),
            JTok::Special(_) => Ok(true),
            JTok::Num(s) if non_base10(s) => radix_int(s).map(|v| v != 0),
            JTok::Num(s) if !s.contains(['.', 'e', 'E']) => {
                let d = s.strip_prefix('-').unwrap_or(s);
                if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("Input string '{s}' is not a valid integer"));
                }
                Ok(d.bytes().any(|b| b != b'0'))
            }
            JTok::Num(s) => parse_double(s).map(|v| v != 0.0).ok_or_else(|| format!("Input string '{s}' is not a valid number")),
            JTok::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(format!("Could not convert string to boolean: {s}")),
            },
            _ => Err("Error reading boolean".into()),
        }
    }

    /// An int enum member (CompilationState): a number as an int, a string as a name (ignoring case) or an int.
    pub fn as_enum(&self, names: &[&str]) -> Result<i32, String> {
        match self {
            JTok::Str(s) => names.iter().position(|n| n.eq_ignore_ascii_case(s.trim())).map(|i| i as i32).or_else(|| s.trim().parse().ok()).ok_or_else(|| format!("Error converting value \"{s}\"")),
            JTok::Num(_) => self.as_i32(),
            _ => Err("Error converting value to an enum".into()),
        }
    }

    /// A string[] / List<string> member: null for null; an array's items by ReadAsString; anything else an error.
    pub fn as_string_array(&self) -> Result<Option<Vec<Option<String>>>, String> {
        match self {
            JTok::Null | JTok::Undefined => Ok(None),
            JTok::Array(a) => a.iter().map(JTok::as_string).collect::<Result<Vec<_>, _>>().map(Some),
            _ => Err("Cannot deserialize the JSON value into a string array".into()),
        }
    }
}

/// Json.NET's octal / hex number: a leading 0 not followed by `.`, `e` or `E`.
fn non_base10(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 1 && b[0] == b'0' && !matches!(b[1], b'.' | b'e' | b'E')
}
/// Convert.ToInt64(s, 16) for `0x...`, else Convert.ToInt64(s, 8): the 64-bit pattern.
fn radix_int(s: &str) -> Result<u64, String> {
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16),
        None => u64::from_str_radix(s, 8),
    };
    r.map_err(|_| format!("Could not convert '{s}'"))
}
/// double.TryParse(s, NumberStyles.Float, InvariantCulture) as Mono has it: an out-of-range value fails.
fn parse_double(s: &str) -> Option<f64> {
    let ok = !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'));
    s.parse::<f64>().ok().filter(|v| ok && v.is_finite())
}
/// Convert.ToUInt64(double): rounded to the nearest, a half to even.
fn double_to_u64(v: f64) -> Result<u64, String> {
    if !(v > -0.5 && v < 18446744073709551615.5) {
        return Err("Value was either too large or too small for a UInt64".into());
    }
    let mut r = v as u64;
    let dif = v - r as f64;
    if dif > 0.5 || (dif == 0.5 && r & 1 != 0) {
        r += 1;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_net_corners() {
        let id = |t: &str| parse(t).ok().flatten().and_then(|v| v.prop("Identifier").map(|p| p.as_string()));
        assert!(parse("{\"Identifier\":[1],\"identifier\":\"a\"}").unwrap().unwrap().read("Identifier", JTok::as_string).is_err());
        assert_eq!(id("/*c*/{'Identifier':'a',}\n// x"), Some(Ok(Some("a".into()))));
        assert_eq!(id("{Identifier:12}"), Some(Ok(Some("12".into()))));
        assert_eq!(id("{\"Identifier\":true}"), Some(Ok(Some("true".into()))));
        assert!(matches!(id("{\"Identifier\":1e400}"), Some(Err(_))));
        assert!(parse("{\"a\":1} x").is_err());
        assert_eq!(parse("{\"IDENTIFIER\":\"up\",\"Identifier\":\"exact\"}").unwrap().unwrap().prop("Identifier"), Some(&JTok::Str("exact".into())));
        assert_eq!(JTok::Num("1.5".into()).as_u64(), Ok(2));
        assert_eq!(JTok::Num("2.5".into()).as_u64(), Ok(2));
        assert!(JTok::Num("-1".into()).as_u64().is_err());
        assert_eq!(JTok::Num("18446744073709551615".into()).as_u64(), Ok(u64::MAX));
        assert!(JTok::Num("18446744073709551616".into()).as_u64().is_err());
        assert_eq!(JTok::Str("77".into()).as_u64(), Ok(77));
    }
}
