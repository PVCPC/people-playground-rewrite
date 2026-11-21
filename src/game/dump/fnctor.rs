//! Constructor checks (the recorder's PPG_FNPROBE=ctor:<list>, tools/recorder/gen_fnctor.py): the game makes the object as
//! it does in play (AddComponent on an inactive GameObject, before Awake; or `new`) and writes each field as a
//! `fnctor <Class>.constructor <field> <value>` line; here the rewrite's struct is built by Default and every field it has
//! under the same name must be equal, bit for bit. A field the two keep in different forms (an enum by name, a reference
//! as an index, a struct) is not compared; a check that compares no field fails.

include!("fnctor_gen.rs");

/// A value as Rust's Debug writes it.
#[derive(Debug, PartialEq)]
enum V {
    /// A number, true/false, an enum variant or unit struct's name.
    Atom(String),
    Str(String),
    /// [a, b] (and a set's {a, b}).
    List(Vec<V>),
    /// Name { a: x, b: y } (and a map's {k: v}: keys as their text).
    Fields(Vec<(String, V)>),
    /// Name(a, b) and (a, b): Some(x) is Call("Some", [x]).
    Call(String, Vec<V>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn word(&mut self) -> String {
        self.ws();
        let st = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || b"_.-+:".contains(&self.s[self.i])) {
            // A path's `::` is part of the name; one ':' ends a field's name.
            if self.s[self.i] == b':' && self.s.get(self.i + 1) != Some(&b':') && !(self.i > st && self.s[self.i - 1] == b':') {
                break;
            }
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).into_owned()
    }
    fn list(&mut self, close: u8) -> Result<Vec<V>, String> {
        let mut out = Vec::new();
        while !self.eat(close) {
            out.push(self.value()?);
            if !self.eat(b',') && !self.eat(close) {
                return Err(format!("expected , or {} at {}", close as char, self.i));
            } else if self.s[self.i - 1] == close {
                break;
            }
        }
        Ok(out)
    }
    fn fields(&mut self) -> Result<Vec<(String, V)>, String> {
        let mut out = Vec::new();
        while !self.eat(b'}') {
            self.ws();
            if self.s[self.i..].starts_with(b"..") {
                self.i += 2;
                continue;
            }
            // A map's key may be any value: its text up to the ':'.
            let st = self.i;
            let key = match self.value()? {
                V::Atom(a) => a,
                V::Str(s) => s,
                _ => String::from_utf8_lossy(&self.s[st..self.i]).into_owned(),
            };
            if !self.eat(b':') {
                // A set: {a, b}.
                let mut l = vec![V::Atom(key)];
                if self.eat(b',') {
                    l.extend(self.list(b'}')?);
                } else if !self.eat(b'}') {
                    return Err(format!("expected : at {}", self.i));
                }
                return Ok(l.into_iter().enumerate().map(|(i, v)| (format!("#{i}"), v)).collect());
            }
            out.push((key, self.value()?));
            if !self.eat(b',') && !self.eat(b'}') {
                return Err(format!("expected , or }} at {}", self.i));
            } else if self.s[self.i - 1] == b'}' {
                break;
            }
        }
        Ok(out)
    }
    fn value(&mut self) -> Result<V, String> {
        self.ws();
        match self.s.get(self.i) {
            None => Err("end of text".into()),
            Some(b'"') | Some(b'\'') => {
                let q = self.s[self.i];
                self.i += 1;
                let mut out = Vec::new();
                while self.i < self.s.len() && self.s[self.i] != q {
                    if self.s[self.i] == b'\\' {
                        self.i += 1;
                        match self.s.get(self.i) {
                            Some(b'n') => out.push(b'\n'),
                            Some(b't') => out.push(b'\t'),
                            Some(b'r') => out.push(b'\r'),
                            Some(b'0') => out.push(0),
                            Some(b'u') => {
                                // \u{..}
                                let end = self.s[self.i..].iter().position(|&c| c == b'}').map_or(self.s.len(), |p| self.i + p);
                                let hex = String::from_utf8_lossy(&self.s[self.i + 2..end]).into_owned();
                                let c = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('?');
                                out.extend(c.to_string().bytes());
                                self.i = end;
                            }
                            Some(&c) => out.push(c),
                            None => {}
                        }
                    } else {
                        out.push(self.s[self.i]);
                    }
                    self.i += 1;
                }
                self.i += 1;
                Ok(V::Str(String::from_utf8_lossy(&out).into_owned()))
            }
            Some(b'[') => {
                self.i += 1;
                Ok(V::List(self.list(b']')?))
            }
            Some(b'(') => {
                self.i += 1;
                Ok(V::Call(String::new(), self.list(b')')?))
            }
            Some(b'{') => {
                self.i += 1;
                let f = self.fields()?;
                Ok(if f.iter().all(|(k, _)| k.starts_with('#')) { V::List(f.into_iter().map(|(_, v)| v).collect()) } else { V::Fields(f) })
            }
            Some(_) => {
                let w = self.word();
                if w.is_empty() {
                    return Err(format!("unexpected {:?} at {}", self.s[self.i] as char, self.i));
                }
                if self.eat(b'(') {
                    // A hand-written Debug ("Curve(2 keys)"): the text up to the closing bracket, not compared.
                    let at = self.i;
                    return Ok(match self.list(b')') {
                        Ok(xs) => V::Call(w, xs),
                        Err(_) => {
                            self.i = at;
                            let mut depth = 1;
                            while self.i < self.s.len() && depth > 0 {
                                match self.s[self.i] {
                                    b'(' => depth += 1,
                                    b')' => depth -= 1,
                                    _ => {}
                                }
                                self.i += 1;
                            }
                            V::Atom(format!("{w}(..)"))
                        }
                    });
                }
                self.ws();
                if self.s.get(self.i) == Some(&b'{') {
                    self.i += 1;
                    return Ok(V::Fields(self.fields()?));
                }
                Ok(V::Atom(w))
            }
        }
    }
}

fn parse(s: &str) -> Result<V, String> {
    let mut p = P { s: s.as_bytes(), i: 0 };
    p.value().map_err(|e| format!("{e} near {:?}", String::from_utf8_lossy(&p.s[p.i.saturating_sub(60)..(p.i + 30).min(p.s.len())])))
}

/// The struct's fields by name, with those of the structs it holds (a base class's) where the name is not taken.
fn flatten<'a>(v: &'a V, out: &mut Vec<(&'a str, &'a V)>) {
    if let V::Fields(fs) = v {
        for (k, x) in fs {
            if !out.iter().any(|(n, _)| n == k) {
                out.push((k, x));
            }
        }
        // A base class's fields (script!'s `base`); other nested structs are the rewrite's own, not the class's fields.
        for (_, x) in fs.iter().filter(|(k, _)| k == "base") {
            flatten(x, out);
        }
    }
}

/// The game's list text "[a;b]" split at its top level.
fn game_items(g: &str) -> Option<Vec<&str>> {
    // A HashSet's or Dictionary's {a;b} too (its entries in the game's order: only an empty one compares as a list).
    let inner = g.strip_prefix('[').and_then(|g| g.strip_suffix(']')).or_else(|| g.strip_prefix('{')?.strip_suffix('}').filter(|i| i.is_empty()))?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let (mut out, mut depth, mut st) = (Vec::new(), 0i32, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            ';' if depth == 0 => {
                out.push(&inner[st..i]);
                st = i + 1;
            }
            _ => {}
        }
    }
    out.push(&inner[st..]);
    Some(out)
}

fn hex8(g: &str) -> Option<u32> {
    if g.len() == 8 { u32::from_str_radix(g, 16).ok() } else { None }
}

/// The game's value text against the rewrite's: Some(equal), or None when the two are not the same kind of thing.
fn same(g: &str, v: &V) -> Option<bool> {
    // A UnityEvent (the recorder's `UnityEvent(persistent=n runtime=m)`): the rewrite's listeners are both kinds.
    if let (Some(m), V::Call(n, xs)) = (g.starts_with("UnityEvent(persistent=").then(|| game_members(g)).flatten(), v) {
        if n == "UnityEvent" && xs.len() == 1 {
            let count = |k: &str| m.iter().find(|(n, _)| *n == k).and_then(|(_, x)| x.parse::<usize>().ok());
            let V::Atom(ours) = &xs[0] else { return None };
            return Some(count("persistent")? + count("runtime")? == ours.parse::<usize>().ok()?);
        }
    }
    // A post-process FloatParameter/IntParameter/BoolParameter the rewrite keeps as its bare value.
    if let (true, V::Atom(_)) = (g.contains("Parameter(value="), v) {
        let m = game_members(g)?;
        return same(m.iter().find(|(k, _)| *k == "value")?.1, v);
    }
    // An OptionalInt/OptionalFloat (Active, Value) is the rewrite's Option.
    if g.starts_with("Optional") {
        if let Some(m) = game_members(g) {
            let get = |k: &str| m.iter().find(|(n, _)| *n == k).map(|(_, x)| *x);
            return match (get("Active")?, v) {
                ("0", V::Atom(a)) => Some(a == "None"),
                ("1", V::Call(n, xs)) if n == "Some" && xs.len() == 1 => same(get("Value")?, &xs[0]),
                ("1", V::Atom(a)) if a == "None" => Some(false),
                _ => None,
            };
        }
    }
    // A ValueTuple (Item1, Item2, ...) is the rewrite's tuple.
    if let (true, V::Call(n, xs)) = (g.starts_with("ValueTuple"), v) {
        if n.is_empty() {
            let m = game_members(g)?;
            return if m.len() == xs.len() { all(m.iter().zip(xs).map(|((_, g), v)| same(g, v))) } else { None };
        }
    }
    match v {
        // None: C#'s null; a C# struct or list the rewrite keeps as an Option is not the same form.
        V::Atom(a) if a == "None" => if g == "null" { Some(true) } else if g.starts_with(['"', '@']) || hex8(g).is_some() { Some(false) } else { None },
        V::Atom(a) if a == "true" || a == "false" => matches!(g, "0" | "1").then(|| (g == "1") == (a == "true")),
        V::Atom(a) if a.starts_with(|c: char| c.is_ascii_digit() || c == '-') || a == "inf" || a == "NaN" => {
            let float = a.contains(['.', 'e', 'E']) || a.ends_with("inf") || a == "NaN";
            if !float {
                // An integer: the game's decimal, or a LayerMask's bits (a float field the rewrite keeps as an integer is
                // not compared).
                let g = g.strip_prefix("LayerMask(m_Mask=").and_then(|g| g.strip_suffix(')')).unwrap_or(g);
                // A buffer's length: a C# array the rewrite sizes by a constant.
                if let Some(items) = game_items(g) {
                    return (!items.contains(&"...")).then(|| items.len() as i128 == a.parse::<i128>().unwrap_or(-1));
                }
                return Some(g.parse::<i128>().ok()? == a.parse::<i128>().ok()?);
            }
            if let Some(bits) = hex8(g) {
                let ours: f32 = a.parse().ok()?;
                let theirs = f32::from_bits(bits);
                return Some(ours.to_bits() == bits || ours.is_nan() && theirs.is_nan());
            }
            if g.len() == 16 {
                let theirs = f64::from_bits(u64::from_str_radix(g, 16).ok()?);
                let ours: f64 = a.parse().ok()?;
                return Some(ours.to_bits() == theirs.to_bits() || ours.is_nan() && theirs.is_nan());
            }
            None
        }
        V::Atom(_) => None,
        V::Str(s) => {
            let t = g.strip_prefix('"')?.strip_suffix('"')?;
            Some(t == s.replace(' ', "_"))
        }
        V::Call(n, xs) if n == "Some" && xs.len() == 1 => if g == "null" { Some(false) } else { same(g, &xs[0]) },
        V::List(xs) => {
            // gapB r7: a non-empty Dictionary `{"key":value;...}` (its entries in the game's order) against the rewrite's
            // [(key, value), ...]: same count, then each key and value.
            if let Some(inner) = g.strip_prefix('{').and_then(|i| i.strip_suffix('}')).filter(|i| !i.is_empty()) {
                let items = game_items(&format!("[{inner}]"))?.into_iter().map(str::to_string).collect::<Vec<_>>();
                if items.len() != xs.len() {
                    return Some(false);
                }
                return all(items.iter().zip(xs).map(|(item, v)| {
                    let (k, gv) = item.split_once(':')?;
                    let V::Call(n, kv) = v else { return None };
                    if !n.is_empty() || kv.len() != 2 {
                        return None;
                    }
                    Some(same(k, &kv[0])? && same(gv, &kv[1])?)
                }));
            }
            if let Some(items) = game_items(g) {
                if items.iter().any(|i| *i == "...") {
                    return None;
                }
                // A C# buffer of nulls or zeroed structs (a preallocated NonAlloc array) the rewrite keeps as an empty Vec:
                // not the same form.
                let blank = |i: &str| i == "null" || i.contains('(') && i.split('=').skip(1).all(|v| v.split([' ', ')']).next().unwrap_or("").chars().all(|c| c == '0' || c == ','));
                if xs.is_empty() && !items.is_empty() && items.iter().all(|i| blank(i)) {
                    return None;
                }
                if items.len() != xs.len() {
                    return Some(false);
                }
                if xs.is_empty() {
                    return Some(true);
                }
                return all(items.iter().zip(xs).map(|(g, v)| same(g, v)));
            }
            // A Vector2/3/4, Quaternion or Color: its float bits; a Vector3 the rewrite keeps as [f32; 2] has z 0.
            let parts: Vec<&str> = g.split(',').collect();
            if parts.len() < 2 || parts.iter().any(|p| hex8(p).is_none()) {
                return None;
            }
            let parts = if parts.len() == 3 && xs.len() == 2 && matches!(parts[2], "00000000" | "80000000") { &parts[..2] } else { &parts[..] };
            if parts.len() != xs.len() {
                return None;
            }
            all(parts.iter().zip(xs).map(|(g, v)| same(g, v)))
        }
        // A C# struct `Name(a=x b=y)` against the rewrite's `Name { a: x }`: its same-named members.
        V::Fields(fs) => {
            let members = game_members(g)?;
            let norm = |s: &str| s.replace('_', "").to_lowercase();
            all(members.iter().filter_map(|(k, gv)| fs.iter().find(|(f, _)| norm(f) == norm(k)).map(|(_, v)| same(gv, v))))
        }
        _ => None,
    }
}

/// `Name(a=x b=y)`'s members (values may nest brackets and quotes).
fn game_members(g: &str) -> Option<Vec<(&str, &str)>> {
    let inner = g[g.find('(')? + 1..].strip_suffix(')')?;
    let (mut out, mut depth, mut st, mut quoted) = (Vec::new(), 0i32, 0, false);
    for (i, c) in inner.char_indices().chain(std::iter::once((inner.len(), ' '))) {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ' ' if depth == 0 && !quoted => {
                if let Some((k, v)) = inner[st..i].split_once('=') {
                    out.push((k, v));
                }
                st = i + 1;
            }
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Equal when no part differs and at least one compares.
fn all(it: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut any = None;
    for r in it {
        match r {
            Some(false) => return Some(false),
            Some(true) => any = Some(true),
            None => {}
        }
    }
    any
}

/// gapB r7: InputSystem.Defaults as the game's dictionary: the rewrite's ACTIONS (ui/tables.rs) in order, each an InputAction
/// with its KeyCode values and its ActionCategory / ActionUniverse enum values.
fn input_defaults() -> Vec<(&'static str, InputAction)> {
    let code = |name: &str| crate::game::keycode::KEYCODES.iter().find(|k| k.1 == name).unwrap_or_else(|| panic!("no KeyCode {name}")).0;
    let pos = |all: &[&str], name: &str| all.iter().position(|x| *x == name).unwrap_or_else(|| panic!("no enum value {name}"));
    crate::ui::tables::ACTIONS
        .iter()
        .map(|a| {
            (a.id, InputAction {
                Name: a.name,
                Key: code(a.key),
                SecondaryKey: code(a.secondary),
                InvisibleInMenu: a.invisible,
                Category: pos(&["General", "UserInterface", "Camera", "MapEditor", "Modded"], a.category),
                Universe: pos(&["None", "All", "Game", "MapEditor"], a.universe),
            })
        })
        .collect()
}

/// InputAction's fields as the recorder prints them (input_defaults).
#[allow(non_snake_case, dead_code)]
#[derive(Debug)]
struct InputAction {
    Name: &'static str,
    Key: i32,
    SecondaryKey: i32,
    InvisibleInMenu: bool,
    Category: usize,
    Universe: usize,
}

/// Constructors that draw from UnityEngine.Random: made from the recorded state, as the game's field initialisers ran on it.
const RANDOM: &[(&str, fn([u32; 4]) -> String)] = &[
    // Connection..ctor: TimeVariation = Random.Range(0.95f, 2f).
    ("Connection.constructor", |s| {
        let mut g = crate::game::Game::default();
        g.random.state = s;
        format!("{:?}", crate::game::coupler::Connection::new(&mut g, 0, 0, 0, [0.0; 2]))
    }),
];

/// The `fnctor` lines' checks: each name with its compared field count, or why it fails.
/// gapB r4: the constructors with parameters: the rewrite's object from the recorder's args (Recorder.Fmt, ';'-joined).
const ARGS: &[(&str, fn(&[&str]) -> Result<String, String>)] = &[
    // DeltaInt(int value): Value = PreviousValueFixed = PreviousValue = value.
    ("DeltaInt.constructor", |a| Ok(format!("{:?}", crate::game::continuous::DeltaInt::new(a[0].parse().map_err(|_| format!("not an int: {}", a[0]))?)))),
    // fnrec r1: GoreStringBehaviour.TissueString(float pos1, float pos2, float thickness, float strength, float thicknessRatio).
    ("TissueString.constructor", |a| {
        let x = |i: usize| a.get(i).ok_or(format!("too few args: {a:?}")).and_then(|t| u32::from_str_radix(t, 16).map(f32::from_bits).map_err(|_| format!("not a float: {t}")));
        Ok(format!("{:?}", crate::game::limb::TissueString::new(x(0)?, x(1)?, x(2)?, x(3)?, x(4)?)))
    }),
    // ActivationTarget(NodeBehaviour node, ushort channelMask): Channels compared (Node is a reference, the rewrite's an index: not compared).
    ("ActivationTarget.constructor", |a| Ok(format!("{:?}", crate::game::activations::NodeTarget::new(0, a.get(1).and_then(|c| c.parse().ok()).ok_or(format!("no channel mask: {a:?}"))?, 0)))),
    // gapB r7: LiquidMixInstructions(Liquid[] sourceLiquids, Liquid targetLiquid, float ratePerSecond): the array's items print as
    // `Class(.. Color=..)` (its `;` splits the args too), a lone Liquid as its registry ID; each liquid compares by its Color, as
    // the Liquid constructors do.
    ("LiquidMixInstructions.constructor", |a| {
        use crate::game::circulation::{liquid_color, LiquidMixInstructions, LIQUIDS};
        let [head @ .., target, rate] = a else { return Err(format!("too few args: {a:?}")) };
        let one = |t: &str| -> Result<usize, String> {
            match t.split_once('(') {
                Some((class, _)) => LIQUIDS.iter().position(|l| l.1 == class).ok_or(format!("no liquid class {class}")),
                None => LIQUIDS.iter().position(|l| l.0 == t.replace('_', " ")).ok_or(format!("no liquid {t}")),
            }
        };
        let joined = head.join(";");
        let inner = joined.strip_prefix('[').and_then(|j| j.strip_suffix(']')).unwrap_or(&joined);
        let sources = game_items(&format!("[{inner}]")).ok_or("sources")?.into_iter().map(one).collect::<Result<Vec<_>, _>>()?;
        let rate = u32::from_str_radix(rate, 16).map(f32::from_bits).map_err(|_| format!("not a float: {rate}"))?;
        let m = LiquidMixInstructions::new(sources, one(target)?, rate);
        let col = |l: usize| format!("Liquid {{ Color: {:?} }}", liquid_color(l));
        Ok(format!(
            "LiquidMixInstructions {{ SourceLiquids: [{}], TargetLiquid: {}, RatePerSecond: {:?} }}",
            m.SourceLiquids.iter().map(|&l| col(l)).collect::<Vec<_>>().join(", "),
            col(m.TargetLiquid),
            m.RatePerSecond
        ))
    }),
    // gapC r24: RefFloat(float v): Raw = v (set_Raw's clamp); Previous 0.
    ("RefFloat.constructor", |a| {
        let v = u32::from_str_radix(a.first().ok_or("no arg")?, 16).map(f32::from_bits).map_err(|_| format!("not a float: {a:?}"))?;
        Ok(format!("{:?}", crate::game::circulation::RefFloat::new(v)))
    }),
    // gapC r24: BallisticsEmitter(MonoBehaviour connectedBehaviour, Cartridge cartridge): the field initialisers and the
    // constructor's LayerMask.GetMask pair (machinegun.rs). ConnectedBehaviour (a reference) and Cartridge (the rewrite's
    // copy of the ScriptableObject's values) are not compared.
    ("BallisticsEmitter.constructor", |_| {
        use crate::game::machinegun::{LAYERS_TO_HIT, WATER_LAYER};
        Ok(format!("{:?}", crate::game::ballistics::Emitter::new(Default::default(), LAYERS_TO_HIT, WATER_LAYER)))
    }),
    // FreezeObjectAction(GameObject relevant, string displayName).
    ("FreezeObjectAction.constructor", |a| {
        let [go, text] = a else { return Err(format!("not 2 args: {a:?}")) };
        let id = super::reference(go).ok_or(format!("not a reference: {go}"))?;
        Ok(format!("{:?}", crate::game::undo::FreezeObjectAction { displayName: text.trim_matches('"').to_string(), RelevantObject: id }))
    }),
];

/// The game's fields against the rewrite object's Debug `text`: every field it has under the same name, bit for bit.
fn compare(text: &str, game: &[(&str, &str)]) -> Result<usize, String> {
    compare_v(parse(text).map_err(|e| format!("Debug not parsed ({e}): {}", &text[..text.len().min(200)]))?, game)
}
fn compare_v(ours: V, game: &[(&str, &str)]) -> Result<usize, String> {
    let mut fields = Vec::new();
    flatten(&ours, &mut fields);
    let (mut n, mut diffs) = (0, Vec::new());
    for (f, g) in game {
        // The C# name, else the same name in snake_case (time_variation for TimeVariation).
        let norm = |s: &str| s.replace('_', "").to_lowercase();
        let found = fields.iter().find(|(k, _)| k == f).or_else(|| fields.iter().find(|(k, _)| norm(k) == norm(f)));
        let Some(&(_, v)) = found else { continue };
        match same(g, v) {
            Some(true) => n += 1,
            Some(false) => diffs.push(format!("{f}: game {g} ours {}", show(v))),
            None => {}
        }
    }
    if !diffs.is_empty() {
        Err(diffs.into_iter().take(6).collect::<Vec<_>>().join(" | "))
    } else if n == 0 {
        Err(format!("no field compares ({} game fields)", game.len()))
    } else {
        Ok(n)
    }
}

pub(super) fn check(lines: &[&str], keep: impl Fn(&str) -> bool) -> Vec<(String, Result<usize, String>)> {
    // "<Class>.static_setup" (older recordings: "<Class>.static setup", two words) is the map's "static setup".
    let mut by: std::collections::BTreeMap<String, Vec<(&str, &str)>> = Default::default();
    for l in lines {
        let (name, rest) = match l.split_once(".static setup ").or_else(|| l.split_once(".static_setup ")) {
            Some((c, rest)) => (format!("{c}.static setup"), rest),
            None => match l.split_once(' ') {
                Some((n, rest)) => (n.to_string(), rest),
                None => continue,
            },
        };
        let (field, value) = rest.split_once(' ').unwrap_or((rest, ""));
        // An auto-property's backing field is the property.
        let field = field.strip_prefix('<').and_then(|f| f.strip_suffix(">k__BackingField")).unwrap_or(field);
        let e = by.entry(name).or_default();
        // The header lines can come twice (the snapshot reader's and the file's).
        if !e.iter().any(|(f, _)| *f == field) {
            e.push((field, value));
        }
    }
    let mut out = Vec::new();
    for (name, game) in by {
        let name = name.as_str();
        if !keep(name) {
            continue;
        }
        // A static setup: the rewrite's constants named as the static fields (gen_fnctor.py), as one struct.
        let statics = STATICS.iter().chain(STATICS_HAND).find(|(n, _)| *n == name).map(|(_, f)| *f);
        // The UnityEngine.Random state the recorder wrote before the constructor ("@random s0,s1,s2,s3", hex).
        let random: Option<[u32; 4]> = game.iter().find(|(f, _)| *f == "@random").and_then(|(_, v)| {
            let s: Vec<u32> = v.split(',').filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
            s.try_into().ok()
        });
        // gapB r4: a constructor with parameters (the recorder's "@args a;b", Recorder.Fmt): ARGS builds the rewrite's.
        let args = game.iter().find(|(f, _)| *f == "@args").map(|(_, v)| *v);
        let game: Vec<(&str, &str)> = game.into_iter().filter(|(f, _)| *f != "@random" && *f != "@via" && *f != "@args").collect();
        // gapB r6: "<field>.Length n" (Recorder CtorRun): a buffer cut at Dump.Format's 256 items ("...") compares by its length.
        let lens: Vec<(&str, &str)> = game.iter().filter_map(|(f, v)| f.strip_suffix(".Length").map(|b| (b, *v))).collect();
        let game: Vec<(&str, &str)> = game
            .into_iter()
            .filter(|(f, _)| !f.ends_with(".Length"))
            .map(|(f, v)| match lens.iter().find(|(b, _)| *b == f) {
                Some((_, n)) if v.contains("...") => (f, *n),
                _ => (f, v),
            })
            .collect();
        if let (Some(a), Some((_, f))) = (args, ARGS.iter().find(|(n, _)| *n == name)) {
            // PPG_MUTATE (fncheck.rs mutated): the rewrite constructor skipped: every field at its type's default (gapC r24:
            // blank_v of the made object; the old mutant was the class's Default, missing for a class with no CTORS entry).
            let mutated = std::env::var("PPG_MUTATE").is_ok_and(|m| m.split(',').any(|x| x == "all" || x == name));
            let made = f(&a.split(';').collect::<Vec<_>>());
            out.push((name.to_string(), made.and_then(|t| if mutated { compare_v(blank_v(&parse(&t)?), &game) } else { compare(&t, &game) })));
            continue;
        }
        // gapB r5: PPG_MUTATE on a static setup: each rewrite static left at its type's default or empty (blank).
        let mutated = std::env::var("PPG_MUTATE").is_ok_and(|m| m.split(',').any(|x| x == "all" || x == name));
        let drawn = RANDOM.iter().find(|(n, _)| *n == name).map(|(_, f)| *f);
        let make: fn() -> String = match (CTORS.iter().chain(CTORS_HAND).find(|(n, _)| *n == name), statics, drawn) {
            (_, _, Some(_)) if random.is_none() => {
                out.push((name.to_string(), Err("a Random draw without the recorded state (@random)".into())));
                continue;
            }
            (_, _, Some(_)) => || String::new(),
            (Some((_, make)), _, _) => *make,
            (None, Some(_), _) => || String::new(),
            (None, None, None) => {
                out.push((name.to_string(), Err("no rewrite struct listed".into())));
                continue;
            }
        };
        let text = match std::panic::catch_unwind(|| match (statics, drawn) {
            (Some(f), _) => format!("Statics {{ {} }}", f().iter().map(|(k, v)| format!("{k}: {}", if mutated { blank(v) } else { v.as_str() })).collect::<Vec<_>>().join(", ")),
            (None, Some(d)) => d(random.unwrap()),
            (None, None) => make(),
        }) {
            Ok(t) => t,
            Err(_) => {
                out.push((name.to_string(), Err("panic in Default".into())));
                continue;
            }
        };
        // gapC r24: PPG_MUTATE on a Default-made object: the constructor skipped, so every field at its type's default (0,
        // false, null, empty): the field initialisers are what is checked (an all-default constructor has no live mutant).
        if mutated && statics.is_none() && drawn.is_none() {
            out.push((name.to_string(), parse(&text).map_err(|e| format!("Debug not parsed ({e})")).and_then(|v| compare_v(blank_v(&v), &game))));
            continue;
        }
        out.push((name.to_string(), compare(&text, &game)));
    }
    out
}

/// gapC r24: a parsed Debug value with every leaf at its C# default (the constructor's field initialisers not run).
fn blank_v(v: &V) -> V {
    match v {
        V::Atom(a) if a == "true" || a == "false" => V::Atom("false".into()),
        V::Atom(a) if a.contains(['.', 'e', 'E']) && a.parse::<f64>().is_ok() || a == "inf" || a == "-inf" || a == "NaN" => V::Atom("0.0".into()),
        V::Atom(a) if a.parse::<i128>().is_ok() => V::Atom("0".into()),
        V::Atom(a) => V::Atom(a.clone()),
        V::Str(_) => V::Str(String::new()),
        V::List(_) => V::List(Vec::new()),
        V::Fields(fs) => V::Fields(fs.iter().map(|(k, x)| (k.clone(), blank_v(x))).collect()),
        V::Call(n, _) if n == "Some" => V::Atom("None".into()),
        V::Call(n, xs) => V::Call(n.clone(), xs.iter().map(blank_v).collect()),
    }
}

/// gapA r20: static setups of the legacy mod path (legacy_mods.rs), not in gen_fnctor.py's list. A list is compared by its
/// length (a C# buffer's length rule in `same`): ModLoader's assemblyLocations are machine paths (netstandard by the working
/// directory, the rest by the Managed folder), so their count (the Distinct of the 26 typeof(..).Assembly.Location) is what
/// both sides share. A Dictionary `{}` is not compared.
const STATICS_HAND: &[(&str, fn() -> Vec<(&'static str, String)>)] = &[
    ("ModLoader.static setup", || {
        let l = crate::game::legacy_mods::ModLoader::cctor(&std::env::current_dir().unwrap_or_default());
        vec![("LoadedMods", l.LoadedMods.len().to_string()), ("alreadyInitialisedMods", l.alreadyInitialisedMods.len().to_string()), ("assemblyLocations", l.assemblyLocations.len().to_string())]
    }),
    ("ModificationManager.static setup", || {
        let m = crate::game::legacy_mods::ModificationManager::default();
        vec![("BackgroundScripts", m.BackgroundScripts.len().to_string()), ("CustomTools", m.CustomTools.len().to_string()), ("CustomPowers", m.CustomPowers.len().to_string())]
    }),
    ("CompilerClient.static setup", || vec![("compiler", format!("{:?}", crate::game::legacy_mods::CompilerClient::default().compiler))]),
];

/// gapC r24: constructors gen_fnctor.py excludes as "shares the struct of another class": one script! struct holds every
/// class's initialisers (only the game class's own fields are compared, by name).
const CTORS_HAND: &[(&str, fn() -> String)] = &[
    // TemperatureGaugeBehaviour: GaugeBehaviour's LabelCount/MinAngle/MaxAngle/GaugeRadius/LabelOffset, its MaxTemperatureDisplay.
    ("TemperatureGaugeBehaviour.constructor", || format!("{:?}", crate::game::gauge::PowerGauge::default())),
];

/// A static's Debug text as its type's default: 0, false, "" or an empty list (the static constructor skipped).
fn blank(v: &str) -> &'static str {
    match v.as_bytes().first() {
        Some(b'[') => "[]",
        Some(b'"') => "\"\"",
        Some(b't' | b'f') => "false",
        _ => "0",
    }
}

fn show(v: &V) -> String {
    let s = format!("{v:?}");
    s[..s.len().min(120)].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct Base {
        Speed: f32,
        Name: String,
    }
    #[derive(Debug)]
    struct Thing {
        A: f32,
        B: bool,
        C: [f32; 2],
        D: Option<i32>,
        E: Vec<f32>,
        base: Base,
        F: std::collections::BTreeMap<i32, f32>,
    }

    /// Every generated struct's Debug parses as fields.
    #[test]
    fn every_ctor_parses() {
        for (name, make) in CTORS {
            let text = make();
            if !matches!(parse(&text), Ok(V::Fields(_))) { eprintln!("PARSEFAIL {name}: {:?}", parse(&text).err()); continue }
            assert!(matches!(parse(&text), Ok(V::Fields(_))), "{name}: {:?} {}", parse(&text).err(), &text[..text.len().min(300)]);
        }
    }

    #[test]
    fn debug_against_game_fields() {
        let t = Thing { A: 2.357, B: true, C: [1.0, -0.5], D: None, E: vec![], base: Base { Speed: 1e-7, Name: "a b".into() }, F: [(1, 2.0)].into() };
        let v = parse(&format!("{t:?}")).unwrap();
        let mut f = Vec::new();
        flatten(&v, &mut f);
        let get = |n: &str| f.iter().find(|(k, _)| *k == n).unwrap().1;
        assert_eq!(same(&format!("{:08x}", 2.357f32.to_bits()), get("A")), Some(true));
        assert_eq!(same("1", get("B")), Some(true));
        assert_eq!(same("3f800000,bf000000,00000000", get("C")), Some(true));
        assert_eq!(same("null", get("D")), Some(true));
        assert_eq!(same("[]", get("E")), Some(true));
        assert_eq!(same("[null;null]", get("E")), None);
        assert_eq!(same(&format!("{:08x}", 1e-7f32.to_bits()), get("Speed")), Some(true));
        assert_eq!(same("\"a_b\"", get("Name")), Some(true));
        assert_eq!(same("3f800000", get("Speed")), Some(false));
        assert_eq!(same("{1:40000000}", get("F")), None);
    }
}
